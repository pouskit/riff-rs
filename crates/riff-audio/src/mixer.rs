//! Cœur du callback audio : deux voix (titre courant, queue de fondu) et chaîne de traitement.
//!
//! Interdit ici : allocation, verrou, E/S, attente. Les voix retirées repartent par `trash` pour être
//! libérées hors du callback.

use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use rtrb::{Consumer, Producer, PushError};

use crate::AudioSettings;
use crate::dsp::Chain;
use crate::dsp::hq::HqSettings;

const CHANNELS: usize = 2;
const SCRATCH_FRAMES: usize = 1024;
const MIN_TAIL_SECONDS: f32 = 0.5;
const MIN_TAIL_GAIN: f32 = 0.01;
const MAX_SEGMENTS: usize = 512;

/// Correspondance entre échantillons écrits et position dans le titre.
#[derive(Clone, Copy, Debug)]
struct Segment {
  written: u64,
  position: Duration,
  speed: f64,
}

/// État partagé entre le fil de décodage, le contrôle et le callback.
pub struct VoiceShared {
  output_rate: u32,
  consumed: AtomicU64,
  seek_request: AtomicU32,
  seek_decoded: AtomicU32,
  seek_ack: AtomicU32,
  finished: AtomicBool,
  drained: AtomicBool,
  starved: AtomicBool,
  failed: AtomicBool,
  /// Jamais touché par le callback.
  segments: Mutex<VecDeque<Segment>>,
}

impl VoiceShared {
  fn new(output_rate: u32) -> Self {
    Self {
      output_rate,
      consumed: AtomicU64::new(0),
      seek_request: AtomicU32::new(0),
      seek_decoded: AtomicU32::new(0),
      seek_ack: AtomicU32::new(0),
      finished: AtomicBool::new(false),
      drained: AtomicBool::new(false),
      starved: AtomicBool::new(false),
      failed: AtomicBool::new(false),
      segments: Mutex::new(VecDeque::new()),
    }
  }

  /// Le fil de décodage note la position du bloc qu'il va écrire.
  pub fn record_segment(&self, written: u64, position: Duration, speed: f64) {
    if let Ok(mut segments) = self.segments.lock() {
      if segments.len() == MAX_SEGMENTS {
        segments.pop_front();
      }
      segments.push_back(Segment {
        written,
        position,
        speed,
      });
    }
  }

  /// Position de lecture, d'après le nombre d'échantillons consommés par le callback.
  pub fn position(&self) -> Option<Duration> {
    let consumed = self.consumed.load(Ordering::Acquire);
    let mut segments = self.segments.lock().ok()?;
    while segments.len() > 1 && segments.get(1).is_some_and(|s| s.written <= consumed) {
      segments.pop_front();
    }
    let segment = segments.front()?;
    let frames = consumed.saturating_sub(segment.written);
    let seconds = frames as f64 / f64::from(self.output_rate) * segment.speed;
    Some(segment.position + Duration::from_secs_f64(seconds))
  }

  /// Demande un déplacement et renvoie son numéro.
  pub fn request_seek(&self) -> u32 {
    self
      .seek_request
      .fetch_add(1, Ordering::AcqRel)
      .wrapping_add(1)
  }

  /// Numéro du dernier déplacement demandé.
  pub fn seek_request_now(&self) -> u32 {
    self.seek_request.load(Ordering::Acquire)
  }

  /// Le fil de décodage a déplacé le décodeur : le callback peut vider le tampon.
  pub fn mark_seek_decoded(&self, generation: u32) {
    if let Ok(mut segments) = self.segments.lock() {
      segments.clear();
    }
    self.finished.store(false, Ordering::Release);
    self.drained.store(false, Ordering::Release);
    self.seek_decoded.store(generation, Ordering::Release);
  }

  /// Vrai quand le callback a vidé le tampon pour ce déplacement.
  pub fn seek_acknowledged(&self, generation: u32) -> bool {
    self.seek_ack.load(Ordering::Acquire) == generation
  }

  pub fn mark_finished(&self) {
    self.finished.store(true, Ordering::Release);
  }

  /// Échantillons déjà consommés par le callback (écoutés ou vidés).
  pub fn consumed_frames(&self) -> u64 {
    self.consumed.load(Ordering::Acquire)
  }

  /// Le titre n'a pas pu être ouvert.
  pub fn mark_failed(&self) {
    self.failed.store(true, Ordering::Release);
  }

  pub fn has_failed(&self) -> bool {
    self.failed.load(Ordering::Acquire)
  }

  /// Vrai quand tout a été joué.
  pub fn is_drained(&self) -> bool {
    self.drained.load(Ordering::Acquire)
  }

  pub fn is_starved(&self) -> bool {
    self.starved.load(Ordering::Acquire)
  }
}

/// Côté callback d'une voix.
pub struct Voice {
  consumer: Consumer<f32>,
  shared: Arc<VoiceShared>,
}

/// Crée une voix : côté écriture pour le décodage, côté lecture pour le callback.
pub fn voice_channel(frames: usize, output_rate: u32) -> (Producer<f32>, Voice, Arc<VoiceShared>) {
  let (producer, consumer) = rtrb::RingBuffer::new(frames * CHANNELS);
  let shared = Arc::new(VoiceShared::new(output_rate));
  (
    producer,
    Voice {
      consumer,
      shared: shared.clone(),
    },
    shared,
  )
}

/// Commandes envoyées au callback. Aucune ne demande d'allocation à l'application.
pub enum MixerCommand {
  SetVoice(Voice),
  ClearVoice,
  Crossfade { next: Voice, seconds: f32 },
  Settings(AudioSettings),
  Hq { enabled: bool, settings: HqSettings },
  FadeOut(f32),
  FadeIn(f32),
  BeginPauseFade,
  Pause,
  Resume,
  FlushEffects,
}

/// Valeurs continues, lues à chaque bloc sans passer par la file de commandes.
pub struct MixerControls {
  volume: AtomicU32,
  muted: AtomicBool,
  normalize_db: AtomicU32,
}

impl MixerControls {
  pub fn new(volume: f32, muted: bool) -> Self {
    Self {
      volume: AtomicU32::new(volume.to_bits()),
      muted: AtomicBool::new(muted),
      normalize_db: AtomicU32::new(0f32.to_bits()),
    }
  }

  pub fn set_volume(&self, volume: f32, muted: bool) {
    self.volume.store(volume.to_bits(), Ordering::Relaxed);
    self.muted.store(muted, Ordering::Relaxed);
  }

  pub fn set_normalize_db(&self, db: f32) {
    self.normalize_db.store(db.to_bits(), Ordering::Relaxed);
  }

  pub fn volume(&self) -> (f32, bool) {
    (
      f32::from_bits(self.volume.load(Ordering::Relaxed)),
      self.muted.load(Ordering::Relaxed),
    )
  }
}

/// Queue de fondu enchaîné : un second lecteur hors du graphe dans l'original.
struct Tail {
  voice: Voice,
  elapsed_frames: f32,
  total_frames: f32,
  start_gain: f32,
}

fn retire(trash: &mut Producer<Voice>, voice: Voice) {
  if let Err(PushError::Full(voice)) = trash.push(voice) {
    // File pleine (le contrôle vide toutes les 50 ms) : fuite préférée à une libération ici.
    std::mem::forget(voice);
  }
}

/// Lit jusqu'à `left.len()` échantillons stéréo et complète par du silence.
fn pull_voice(voice: &mut Voice, left: &mut [f32], right: &mut [f32]) -> usize {
  let frames = left.len();
  let available = voice.consumer.slots() / CHANNELS;
  let count = available.min(frames);
  let mut read = 0;
  if count > 0
    && let Ok(chunk) = voice.consumer.read_chunk(count * CHANNELS)
  {
    let (first, second) = chunk.as_slices();
    let mut samples = first.iter().chain(second).copied();
    for (l, r) in left[..count].iter_mut().zip(&mut right[..count]) {
      *l = samples.next().unwrap_or(0.0);
      *r = samples.next().unwrap_or(0.0);
    }
    chunk.commit_all();
    read = count;
    voice
      .shared
      .consumed
      .fetch_add(count as u64, Ordering::AcqRel);
  }
  left[read..].fill(0.0);
  right[read..].fill(0.0);
  read
}

fn discard_all(voice: &mut Voice) {
  let slots = voice.consumer.slots();
  if slots > 0
    && let Ok(chunk) = voice.consumer.read_chunk(slots)
  {
    chunk.commit_all();
    voice
      .shared
      .consumed
      .fetch_add((slots / CHANNELS) as u64, Ordering::AcqRel);
  }
}

/// Mélange : voix courante dans la chaîne, queue de fondu à côté.
pub struct Mixer {
  chain: Chain,
  commands: Consumer<MixerCommand>,
  trash: Producer<Voice>,
  controls: Arc<MixerControls>,
  sample_rate: f32,
  current: Option<Voice>,
  tail: Option<Tail>,
  paused: bool,
  master_volume: f32,
  applied_volume: (u32, bool),
  applied_normalize: u32,
  /// Fondu d'entrée à lancer dès les premiers échantillons du titre suivant.
  pending_fade_in: Option<f32>,
  tail_left: Box<[f32; SCRATCH_FRAMES]>,
  tail_right: Box<[f32; SCRATCH_FRAMES]>,
}

impl Mixer {
  pub fn new(
    chain: Chain,
    commands: Consumer<MixerCommand>,
    trash: Producer<Voice>,
    controls: Arc<MixerControls>,
    sample_rate: f32,
  ) -> Self {
    Self {
      chain,
      commands,
      trash,
      controls,
      sample_rate,
      current: None,
      tail: None,
      paused: false,
      master_volume: 1.0,
      applied_volume: (f32::NAN.to_bits(), false),
      applied_normalize: 0f32.to_bits(),
      pending_fade_in: None,
      tail_left: Box::new([0.0; SCRATCH_FRAMES]),
      tail_right: Box::new([0.0; SCRATCH_FRAMES]),
    }
  }

  fn apply_commands(&mut self) {
    while let Ok(command) = self.commands.pop() {
      match command {
        MixerCommand::SetVoice(voice) => {
          if let Some(old) = self.current.replace(voice) {
            retire(&mut self.trash, old);
          }
        }
        MixerCommand::ClearVoice => {
          if let Some(old) = self.current.take() {
            retire(&mut self.trash, old);
          }
        }
        MixerCommand::Crossfade { next, seconds } => self.start_crossfade(next, seconds),
        MixerCommand::Settings(settings) => {
          self.master_volume = settings.volume;
          self.chain.set_settings(&settings);
        }
        MixerCommand::Hq { enabled, settings } => self.chain.set_hq(enabled, &settings),
        MixerCommand::FadeOut(seconds) => self.chain.fade_out(seconds),
        MixerCommand::FadeIn(seconds) => self.chain.fade_in(seconds),
        MixerCommand::BeginPauseFade => self.chain.begin_pause_fade(),
        MixerCommand::Pause => {
          self.paused = true;
          self.chain.flush_effects();
        }
        MixerCommand::Resume => {
          self.paused = false;
          self.chain.end_pause_fade();
        }
        MixerCommand::FlushEffects => self.chain.flush_effects(),
      }
    }
  }

  fn start_crossfade(&mut self, next: Voice, seconds: f32) {
    if let Some(old_tail) = self.tail.take() {
      retire(&mut self.trash, old_tail.voice);
    }
    let (volume, muted) = self.controls.volume();
    let raw = if muted { 0.0 } else { volume.clamp(0.0, 1.0) };
    let master = if self.master_volume == 0.0 {
      1.0
    } else {
      self.master_volume
    };
    let start_gain = (raw * raw * master).clamp(0.0, 1.0);
    if let Some(old) = self.current.replace(next) {
      if start_gain > MIN_TAIL_GAIN {
        self.tail = Some(Tail {
          voice: old,
          elapsed_frames: 0.0,
          total_frames: seconds.max(MIN_TAIL_SECONDS) * self.sample_rate,
          start_gain,
        });
      } else {
        retire(&mut self.trash, old);
      }
    }
    self.pending_fade_in = Some(seconds);
  }

  fn apply_controls(&mut self) {
    let (volume, muted) = self.controls.volume();
    if self.applied_volume != (volume.to_bits(), muted) {
      self.applied_volume = (volume.to_bits(), muted);
      self.chain.set_volume(volume, muted);
    }
    let normalize = self.controls.normalize_db.load(Ordering::Relaxed);
    if self.applied_normalize != normalize {
      self.applied_normalize = normalize;
      self.chain.set_normalize_gain_db(f32::from_bits(normalize));
    }
  }

  /// Remplit la sortie stéréo.
  pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
    self.apply_commands();
    self.apply_controls();
    self.pull_current(left, right);
    self.chain.process(left, right);
    self.mix_tail(left, right);
  }

  fn pull_current(&mut self, left: &mut [f32], right: &mut [f32]) {
    let Some(voice) = &mut self.current else {
      left.fill(0.0);
      right.fill(0.0);
      return;
    };
    let shared = &voice.shared;
    let request = shared.seek_request.load(Ordering::Acquire);
    if request != shared.seek_ack.load(Ordering::Relaxed) {
      discard_all(voice);
      let shared = &voice.shared;
      if shared.seek_decoded.load(Ordering::Acquire) == request {
        shared.seek_ack.store(request, Ordering::Release);
      }
      left.fill(0.0);
      right.fill(0.0);
      return;
    }
    if self.paused {
      left.fill(0.0);
      right.fill(0.0);
      return;
    }
    let read = pull_voice(voice, left, right);
    if read > 0
      && let Some(seconds) = self.pending_fade_in.take()
    {
      self.chain.fade_in(seconds);
    }
    let shared = &voice.shared;
    let finished = shared.finished.load(Ordering::Acquire);
    if read < left.len() {
      if finished && voice.consumer.is_empty() {
        shared.drained.store(true, Ordering::Release);
        shared.starved.store(false, Ordering::Relaxed);
      } else {
        shared.starved.store(true, Ordering::Relaxed);
      }
    } else {
      shared.starved.store(false, Ordering::Relaxed);
      if finished && voice.consumer.is_empty() {
        shared.drained.store(true, Ordering::Release);
      }
    }
  }

  fn mix_tail(&mut self, left: &mut [f32], right: &mut [f32]) {
    let Some(tail) = &mut self.tail else {
      return;
    };
    if self.paused {
      return;
    }
    let mut done = false;
    for (out_left, out_right) in left
      .chunks_mut(SCRATCH_FRAMES)
      .zip(right.chunks_mut(SCRATCH_FRAMES))
    {
      let frames = out_left.len();
      let (tail_left, tail_right) = (
        &mut self.tail_left[..frames],
        &mut self.tail_right[..frames],
      );
      pull_voice(&mut tail.voice, tail_left, tail_right);
      let samples = tail_left.iter().zip(tail_right.iter());
      for ((l, r), (tl, tr)) in out_left.iter_mut().zip(out_right.iter_mut()).zip(samples) {
        let progress = (tail.elapsed_frames / tail.total_frames).min(1.0);
        let gain = tail.start_gain * (progress * FRAC_PI_2).cos();
        *l += tl * gain;
        *r += tr * gain;
        tail.elapsed_frames += 1.0;
      }
      if tail.elapsed_frames >= tail.total_frames {
        done = true;
        break;
      }
    }
    if done && let Some(finished) = self.tail.take() {
      retire(&mut self.trash, finished.voice);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::dsp::ChainConfig;
  use crate::dsp::waveshaper::Oversample;

  const RATE: u32 = 48_000;

  struct Fixture {
    mixer: Mixer,
    commands: Producer<MixerCommand>,
    trash: Consumer<Voice>,
    controls: Arc<MixerControls>,
  }

  fn fixture() -> Fixture {
    let chain = Chain::new(ChainConfig {
      sample_rate: f64::from(RATE),
      hq_oversample: Oversample::X2,
      output_stage: false,
    });
    let (commands, command_rx) = rtrb::RingBuffer::new(32);
    let (trash_tx, trash) = rtrb::RingBuffer::new(8);
    let controls = Arc::new(MixerControls::new(1.0, false));
    let mixer = Mixer::new(chain, command_rx, trash_tx, controls.clone(), RATE as f32);
    Fixture {
      mixer,
      commands,
      trash,
      controls,
    }
  }

  fn send(fixture: &mut Fixture, command: MixerCommand) {
    assert!(
      fixture.commands.push(command).is_ok(),
      "file de commandes pleine"
    );
  }

  fn filled_voice(value: f32, frames: usize) -> (Voice, Arc<VoiceShared>, Producer<f32>) {
    let (mut producer, voice, shared) = voice_channel(frames.max(64), RATE);
    for _ in 0..frames * CHANNELS {
      assert!(producer.push(value).is_ok());
    }
    (voice, shared, producer)
  }

  fn render(fixture: &mut Fixture, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (vec![9.0; frames], vec![9.0; frames]);
    fixture.mixer.render(&mut left, &mut right);
    (left, right)
  }

  #[test]
  fn empty_mixer_outputs_silence() {
    let mut f = fixture();
    let (left, right) = render(&mut f, 256);
    assert!(left.iter().chain(&right).all(|v| *v == 0.0));
  }

  #[test]
  fn voice_samples_reach_the_output_and_are_counted() {
    let mut f = fixture();
    let (voice, shared, _producer) = filled_voice(0.5, 480);
    send(&mut f, MixerCommand::SetVoice(voice));
    let (left, right) = render(&mut f, 128);
    assert!((left[64] - 0.5).abs() < 1e-6 && (right[64] - 0.5).abs() < 1e-6);
    assert_eq!(shared.consumed.load(Ordering::Relaxed), 128);
  }

  #[test]
  fn interleaved_channels_stay_separated() {
    let mut f = fixture();
    let (mut producer, voice, _shared) = voice_channel(64, RATE);
    for _ in 0..16 {
      assert!(producer.push(0.25).is_ok());
      assert!(producer.push(-0.25).is_ok());
    }
    send(&mut f, MixerCommand::SetVoice(voice));
    let (left, right) = render(&mut f, 16);
    assert!((left[8] - 0.25).abs() < 1e-6);
    assert!((right[8] + 0.25).abs() < 1e-6);
  }

  #[test]
  fn underrun_outputs_silence_and_flags_starvation() {
    let mut f = fixture();
    let (voice, shared, _producer) = filled_voice(0.5, 64);
    send(&mut f, MixerCommand::SetVoice(voice));
    let (left, _) = render(&mut f, 128);
    assert!((left[32] - 0.5).abs() < 1e-6);
    assert_eq!(left[100], 0.0);
    assert!(shared.is_starved());
    assert!(!shared.is_drained());
  }

  #[test]
  fn finished_voice_reports_drained_once_empty() {
    let mut f = fixture();
    let (voice, shared, _producer) = filled_voice(0.5, 64);
    shared.mark_finished();
    send(&mut f, MixerCommand::SetVoice(voice));
    render(&mut f, 32);
    assert!(!shared.is_drained());
    render(&mut f, 64);
    assert!(shared.is_drained());
    assert!(
      !shared.is_starved(),
      "fin de titre : pas d'attente de données"
    );
  }

  #[test]
  fn pause_stops_consuming_and_resume_continues() {
    let mut f = fixture();
    let (voice, shared, _producer) = filled_voice(0.5, 480);
    send(&mut f, MixerCommand::SetVoice(voice));
    render(&mut f, 64);
    send(&mut f, MixerCommand::Pause);
    let (left, _) = render(&mut f, 64);
    assert!(left.iter().all(|v| *v == 0.0));
    assert_eq!(shared.consumed.load(Ordering::Relaxed), 64);
    assert!(!shared.is_starved());
    send(&mut f, MixerCommand::Resume);
    let (left, _) = render(&mut f, 64);
    assert!((left[32] - 0.5).abs() < 1e-6);
    assert_eq!(shared.consumed.load(Ordering::Relaxed), 128);
  }

  #[test]
  fn seek_discards_old_audio_until_the_decoder_is_ready() {
    let mut f = fixture();
    let (voice, shared, mut producer) = filled_voice(0.5, 256);
    send(&mut f, MixerCommand::SetVoice(voice));
    render(&mut f, 16);
    let generation = shared.request_seek();
    let (left, _) = render(&mut f, 64);
    assert!(
      left.iter().all(|v| *v == 0.0),
      "ancien son coupé dès la demande"
    );
    assert!(!shared.seek_acknowledged(generation));

    shared.mark_seek_decoded(generation);
    render(&mut f, 16);
    assert!(shared.seek_acknowledged(generation));
    for _ in 0..64 * CHANNELS {
      assert!(producer.push(0.75).is_ok());
    }
    let (left, _) = render(&mut f, 64);
    assert!((left[10] - 0.75).abs() < 1e-6);
  }

  #[test]
  fn replacing_the_voice_sends_the_old_one_to_the_trash() {
    let mut f = fixture();
    let (first, _a, _pa) = filled_voice(0.5, 64);
    let (second, _b, _pb) = filled_voice(0.25, 64);
    send(&mut f, MixerCommand::SetVoice(first));
    render(&mut f, 16);
    send(&mut f, MixerCommand::SetVoice(second));
    let (left, _) = render(&mut f, 16);
    assert!((left[8] - 0.25).abs() < 1e-6);
    assert!(
      f.trash.pop().is_ok(),
      "ancienne voix récupérée hors du callback"
    );
  }

  #[test]
  fn crossfade_tail_decays_as_a_cosine_outside_the_chain() {
    let mut f = fixture();
    // Le titre courant joue à 0,5 ; la chaîne coupe tout (muet exclu) : réverbération inutile.
    let (current, _a, _pa) = filled_voice(0.5, 48_000);
    let (next, _b, _pb) = filled_voice(0.0, 48_000);
    send(&mut f, MixerCommand::SetVoice(current));
    render(&mut f, 128);
    send(&mut f, MixerCommand::Crossfade { next, seconds: 0.5 });
    let frames = 24_000;
    let (left, _) = render(&mut f, frames);
    let at = |seconds: f32| left[(seconds * RATE as f32) as usize];
    let expected = |seconds: f32| 0.5 * (seconds / 0.5 * FRAC_PI_2).cos();
    for seconds in [0.1f32, 0.25, 0.4] {
      assert!(
        (at(seconds) - expected(seconds)).abs() < 2e-3,
        "à {seconds} s : {}",
        at(seconds)
      );
    }
    let (left, _) = render(&mut f, 256);
    assert!(left.iter().all(|v| v.abs() < 1e-6), "queue terminée");
    assert!(f.trash.pop().is_ok(), "queue rendue au contrôle");
  }

  #[test]
  fn next_title_fades_in_from_its_first_samples() {
    let mut f = fixture();
    let (current, _a, _pa) = filled_voice(0.0, 48_000);
    let (mut producer, next, _shared) = voice_channel(48_000, RATE);
    send(&mut f, MixerCommand::SetVoice(current));
    render(&mut f, 64);
    send(&mut f, MixerCommand::Crossfade { next, seconds: 0.5 });
    // Le fondu attend les premiers échantillons : un chargement lent ne le consomme pas.
    render(&mut f, 4800);
    let frames = 24_000;
    for _ in 0..frames * CHANNELS {
      assert!(producer.push(1.0).is_ok());
    }
    let (left, _) = render(&mut f, frames);
    assert!(left[0] < 0.01, "départ à {}", left[0]);
    let middle = left[(0.25 * RATE as f32) as usize];
    let want = 0.001f32.sqrt();
    assert!(
      (middle / want - 1.0).abs() < 0.05,
      "milieu {middle} au lieu de {want}"
    );
    assert!(
      (left[frames - 1] - 1.0).abs() < 1e-3,
      "fin {}",
      left[frames - 1]
    );
  }

  /// `app.js`, ligne 8683 : `audioEngine.fadeIn(X)`, sur toute la durée du fondu enchaîné.
  #[test]
  fn next_title_fades_in_over_the_whole_crossfade() {
    let mut f = fixture();
    let (current, _a, _pa) = filled_voice(0.0, 48_000);
    let (mut producer, next, _shared) = voice_channel(96_000, RATE);
    send(&mut f, MixerCommand::SetVoice(current));
    render(&mut f, 64);
    send(&mut f, MixerCommand::Crossfade { next, seconds: 2.0 });
    let frames = 96_000;
    for _ in 0..frames * CHANNELS {
      assert!(producer.push(1.0).is_ok());
    }
    let (left, _) = render(&mut f, frames);
    let middle = left[frames / 2];
    let want = 0.001f32.sqrt();
    assert!(
      (middle / want - 1.0).abs() < 0.05,
      "milieu {middle} au lieu de {want}"
    );
    assert!(
      (left[frames - 1] - 1.0).abs() < 1e-3,
      "fin {}",
      left[frames - 1]
    );
  }

  #[test]
  fn crossfade_duration_has_a_minimum_of_half_a_second() {
    let mut f = fixture();
    let (current, _a, _pa) = filled_voice(0.5, 48_000);
    let (next, _b, _pb) = filled_voice(0.0, 48_000);
    send(&mut f, MixerCommand::SetVoice(current));
    render(&mut f, 16);
    send(&mut f, MixerCommand::Crossfade { next, seconds: 0.1 });
    let (left, _) = render(&mut f, 12_000);
    assert!(left[11_000] > 0.05, "toujours audible après 0,23 s");
  }

  #[test]
  fn tail_starts_at_user_volume_squared_times_master() {
    let mut f = fixture();
    f.controls.set_volume(0.5, false);
    send(
      &mut f,
      MixerCommand::Settings(AudioSettings {
        volume: 0.8,
        ..AudioSettings::default()
      }),
    );
    let (current, _a, _pa) = filled_voice(1.0, 48_000);
    let (next, _b, _pb) = filled_voice(0.0, 48_000);
    send(&mut f, MixerCommand::SetVoice(current));
    render(&mut f, 256);
    send(&mut f, MixerCommand::Crossfade { next, seconds: 1.0 });
    let (left, _) = render(&mut f, 64);
    assert!((left[1] - 0.25 * 0.8).abs() < 2e-3, "{}", left[1]);
  }

  #[test]
  fn silent_volume_skips_the_tail() {
    let mut f = fixture();
    f.controls.set_volume(0.0, false);
    let (current, _a, _pa) = filled_voice(1.0, 4800);
    let (next, _b, _pb) = filled_voice(0.0, 4800);
    send(&mut f, MixerCommand::SetVoice(current));
    render(&mut f, 64);
    send(&mut f, MixerCommand::Crossfade { next, seconds: 1.0 });
    let (left, _) = render(&mut f, 64);
    assert!(left.iter().all(|v| *v == 0.0));
    assert!(f.trash.pop().is_ok(), "voix inutile rendue immédiatement");
  }

  #[test]
  fn controls_apply_volume_without_a_command() {
    let mut f = fixture();
    let (voice, _shared, _producer) = filled_voice(1.0, 4800);
    send(&mut f, MixerCommand::SetVoice(voice));
    f.controls.set_volume(0.5, false);
    render(&mut f, 256);
    let (left, _) = render(&mut f, 64);
    assert!((left[32] - 0.25).abs() < 1e-5);
  }

  #[test]
  fn position_follows_the_consumed_samples_and_the_speed() {
    let mut f = fixture();
    let (voice, shared, _producer) = filled_voice(0.5, 48_000);
    shared.record_segment(0, Duration::from_secs(10), 1.5);
    send(&mut f, MixerCommand::SetVoice(voice));
    render(&mut f, 4800);
    let got = shared.position().expect("position").as_secs_f64();
    assert!((got - 10.15).abs() < 1e-6, "{got}");
  }
}
