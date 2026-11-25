//! Moteur : fil de contrôle, fils de décodage, événements.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer};

use crate::analyser::{Analyser, DEFAULT_FFT_SIZE, HIDDEN_FFT_SIZE, RING_CAPACITY, SampleRing};
use crate::decode::Decoder;
use crate::dsp::hq::HqSettings;
use crate::dsp::waveshaper::Oversample;
use crate::dsp::{Chain, ChainConfig};
use crate::mixer::{Mixer, MixerCommand, MixerControls, Voice, VoiceShared, voice_channel};
use crate::output::{self, CpalBackend, DeviceInfo, OutputBackend, OutputNotice, OutputStream};
use crate::source::Source;
use crate::tempo::{TempoParams, TempoStage};
use crate::{AudioError, AudioSettings};

const TICK: Duration = Duration::from_millis(50);
const POSITION_PERIOD: Duration = Duration::from_millis(250);
/// Délai de `pauseSmooth` avant l'arrêt réel et le vidage des effets.
const PAUSE_FADE: Duration = Duration::from_millis(170);
/// `suspendAudioContextDebounced(5000)`.
const SUSPEND_DELAY: Duration = Duration::from_secs(5);
/// Délai entre deux essais de réouverture de la sortie.
const OUTPUT_RETRY: Duration = Duration::from_millis(500);
/// Fréquence de vérification du périphérique par défaut.
const DEVICE_POLL: Duration = Duration::from_millis(500);
const BUFFER_SECONDS: f64 = 0.25;
const COMMAND_CAPACITY: usize = 256;
const TRASH_CAPACITY: usize = 16;
const DEFAULT_VOLUME: f32 = 0.85;
const RING_WAIT: Duration = Duration::from_millis(5);
const IDLE_WAIT: Duration = Duration::from_millis(20);
const SEEK_ACK_TIMEOUT: Duration = Duration::from_millis(500);
const SEEK_ACK_POLL: Duration = Duration::from_millis(1);

/// Réglages de démarrage.
#[derive(Clone, Debug, Default)]
pub struct EngineConfig {
  /// Identifiant de périphérique, `None` pour le périphérique par défaut.
  pub output_device: Option<String>,
}

/// Événements émis par le moteur.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
  Loaded {
    duration: Option<Duration>,
    sample_rate: u32,
  },
  /// Émis 4 fois par seconde au plus.
  Position(Duration),
  Ended,
  Buffering(bool),
  DeviceChanged(String),
  Error(AudioError),
}

enum Control {
  Load {
    source: Source,
    start: Duration,
    autoplay: bool,
  },
  Play,
  Pause,
  Stop,
  Seek(Duration),
  Settings(AudioSettings),
  Hq {
    enabled: bool,
    settings: HqSettings,
  },
  FadeOut(f32),
  FadeIn(f32),
  Crossfade {
    next: Source,
    seconds: f32,
  },
  SetDevice(Option<String>),
  OutputLost(String),
  OutputRerouted,
  Shutdown,
}

enum DecodeCommand {
  Seek { to: Duration, generation: u32 },
  Tempo(TempoParams),
}

struct ActiveVoice {
  shared: Arc<VoiceShared>,
  commands: Sender<DecodeCommand>,
  seek_pending: Option<u32>,
}

struct OpenOutput {
  stream: Box<dyn OutputStream>,
  commands: Producer<MixerCommand>,
  trash: Consumer<Voice>,
  sample_rate: u32,
  suspended: bool,
}

/// Lecteur audio.
pub struct Engine {
  control: Sender<Control>,
  controls: Arc<MixerControls>,
  analyser: Mutex<Analyser>,
  thread: Option<JoinHandle<()>>,
}

impl Engine {
  /// Crée le moteur. L'absence de périphérique n'est pas une erreur ici : elle est signalée par
  /// `Event::Error(AudioError::NoDevice)` au premier chargement.
  ///
  /// # Errors
  /// Impossible de lancer le fil de contrôle.
  pub fn new(cfg: EngineConfig) -> Result<(Self, Receiver<Event>), AudioError> {
    Self::with_backend(cfg, Box::new(CpalBackend))
  }

  pub(crate) fn with_backend(
    cfg: EngineConfig,
    backend: Box<dyn OutputBackend>,
  ) -> Result<(Self, Receiver<Event>), AudioError> {
    let (events, event_rx) = mpsc::channel();
    let (control, control_rx) = mpsc::channel();
    let controls = Arc::new(MixerControls::new(DEFAULT_VOLUME, false));
    let analyser_ring = Arc::new(SampleRing::new(RING_CAPACITY));
    let level_ring = Arc::new(SampleRing::new(RING_CAPACITY));
    let analyser = Mutex::new(Analyser::new(analyser_ring.clone(), level_ring.clone()));
    // La sortie (`cpal::Stream`) n'est pas forcément `Send` : le contrôleur naît dans son fil.
    let device = cfg.output_device;
    let (thread_control, thread_controls) = (control.clone(), controls.clone());
    let thread = thread::Builder::new()
      .name("riff-audio-control".into())
      .spawn(move || {
        let controller = Controller {
          backend,
          device,
          control: thread_control,
          events,
          controls: thread_controls,
          analyser_ring,
          level_ring,
          settings: AudioSettings::default(),
          hq: (false, HqSettings::default()),
          output: None,
          source: None,
          voice: None,
          playing: false,
          position: Duration::ZERO,
          last_emit: None,
          ended: false,
          buffering: false,
          pause_at: None,
          suspend_at: None,
          retry_at: None,
          next_device_poll: Instant::now() + DEVICE_POLL,
          resume_playing: false,
        };
        controller.run(&control_rx);
      })
      .map_err(|err| AudioError::Stream(err.to_string()))?;
    Ok((
      Self {
        control,
        controls,
        analyser,
        thread: Some(thread),
      },
      event_rx,
    ))
  }

  fn send(&self, command: Control) {
    if self.control.send(command).is_err() {
      tracing::error!("fil de contrôle audio arrêté");
    }
  }

  pub fn load(&self, src: Source, start: Duration, autoplay: bool) {
    self.send(Control::Load {
      source: src,
      start,
      autoplay,
    });
  }

  pub fn play(&self) {
    self.send(Control::Play);
  }

  pub fn pause(&self) {
    self.send(Control::Pause);
  }

  pub fn stop(&self) {
    self.send(Control::Stop);
  }

  pub fn seek(&self, to: Duration) {
    self.send(Control::Seek(to));
  }

  /// `volume` de 0 à 1 : le carré est appliqué par la chaîne.
  pub fn set_volume(&self, volume: f32, muted: bool) {
    self.controls.set_volume(volume, muted);
  }

  pub fn set_settings(&self, settings: AudioSettings) {
    self.send(Control::Settings(settings));
  }

  pub fn set_hq(&self, enabled: bool, settings: HqSettings) {
    self.send(Control::Hq { enabled, settings });
  }

  pub fn set_normalize_gain_db(&self, db: f32) {
    self.controls.set_normalize_db(db);
  }

  pub fn fade_out(&self, secs: f32) {
    self.send(Control::FadeOut(secs));
  }

  pub fn fade_in(&self, secs: f32) {
    self.send(Control::FadeIn(secs));
  }

  /// Le titre courant devient la queue de fondu, `next` démarre avec un fondu d'entrée.
  pub fn crossfade_to(&self, next: Source, secs: f32) {
    self.send(Control::Crossfade {
      next,
      seconds: secs,
    });
  }

  pub fn set_output_device(&self, id: Option<String>) {
    self.send(Control::SetDevice(id));
  }

  pub fn list_devices() -> Vec<DeviceInfo> {
    output::list_devices()
  }

  pub fn frequency_bytes(&self, out: &mut [u8]) {
    if let Ok(mut analyser) = self.analyser.lock() {
      analyser.frequency_bytes(out);
    }
  }

  pub fn time_bytes(&self, out: &mut [u8]) {
    if let Ok(analyser) = self.analyser.lock() {
      analyser.time_bytes(out);
    }
  }

  pub fn level_db(&self) -> f32 {
    self
      .analyser
      .lock()
      .map_or(-120.0, |mut analyser| analyser.level_db())
  }

  pub fn set_analyser_hidden(&self, hidden: bool) {
    if let Ok(mut analyser) = self.analyser.lock() {
      analyser.set_fft_size(if hidden {
        HIDDEN_FFT_SIZE
      } else {
        DEFAULT_FFT_SIZE
      });
    }
  }
}

impl Drop for Engine {
  fn drop(&mut self) {
    self.send(Control::Shutdown);
    if let Some(thread) = self.thread.take()
      && thread.join().is_err()
    {
      tracing::error!("fil de contrôle audio terminé en erreur");
    }
  }
}

struct Controller {
  backend: Box<dyn OutputBackend>,
  device: Option<String>,
  control: Sender<Control>,
  events: Sender<Event>,
  controls: Arc<MixerControls>,
  analyser_ring: Arc<SampleRing>,
  level_ring: Arc<SampleRing>,
  settings: AudioSettings,
  hq: (bool, HqSettings),
  output: Option<OpenOutput>,
  source: Option<Source>,
  voice: Option<ActiveVoice>,
  playing: bool,
  position: Duration,
  last_emit: Option<Instant>,
  ended: bool,
  buffering: bool,
  pause_at: Option<Instant>,
  suspend_at: Option<Instant>,
  /// Sortie indisponible : prochain essai de réouverture.
  retry_at: Option<Instant>,
  next_device_poll: Instant,
  /// La lecture reprendra au retour de la sortie.
  resume_playing: bool,
}

impl Controller {
  fn run(mut self, commands: &Receiver<Control>) {
    loop {
      match commands.recv_timeout(TICK) {
        Ok(Control::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
        Ok(command) => self.handle(command),
        Err(RecvTimeoutError::Timeout) => {}
      }
      self.tick();
    }
  }

  fn emit(&self, event: Event) {
    // Le récepteur peut avoir été abandonné par l'appelant : rien à faire de plus.
    let _ = self.events.send(event);
  }

  fn handle(&mut self, command: Control) {
    match command {
      Control::Load {
        source,
        start,
        autoplay,
      } => self.load(source, start, autoplay),
      Control::Play => self.play(),
      Control::Pause => self.pause(),
      Control::Stop => self.stop(),
      Control::Seek(to) => self.seek(to),
      Control::Settings(settings) => self.apply_settings(settings),
      Control::Hq { enabled, settings } => {
        self.hq = (enabled, settings);
        self.send_mixer(MixerCommand::Hq { enabled, settings });
      }
      Control::FadeOut(seconds) => self.send_mixer(MixerCommand::FadeOut(seconds)),
      Control::FadeIn(seconds) => self.send_mixer(MixerCommand::FadeIn(seconds)),
      Control::Crossfade { next, seconds } => self.crossfade(next, seconds),
      Control::SetDevice(id) => {
        self.device = id;
        self.reopen_output();
      }
      Control::OutputLost(reason) => {
        tracing::warn!(%reason, "sortie audio perdue");
        self.reopen_output();
      }
      Control::OutputRerouted => {
        if let Some(output) = &self.output {
          self.emit(Event::DeviceChanged(output.stream.name()));
        }
      }
      Control::Shutdown => {}
    }
  }

  fn send_mixer(&mut self, command: MixerCommand) {
    if let Some(output) = &mut self.output
      && output.commands.push(command).is_err()
    {
      tracing::warn!("file de commandes audio pleine, commande perdue");
    }
  }

  /// Ouvre la sortie si besoin. Échec : erreur signalée une fois, lecture en pause, nouvel essai
  /// planifié (un périphérique en cours de retrait peut refuser l'ouverture un instant).
  fn ensure_output(&mut self) -> bool {
    if self.output.is_some() {
      return true;
    }
    match self.open_output() {
      Ok(output) => {
        self.output = Some(output);
        self.retry_at = None;
        let settings = self.settings;
        let (enabled, hq) = self.hq;
        self.send_mixer(MixerCommand::Settings(settings));
        self.send_mixer(MixerCommand::Hq {
          enabled,
          settings: hq,
        });
        true
      }
      Err(err) => {
        self.resume_playing |= self.playing;
        self.playing = false;
        if self.retry_at.is_none() {
          self.emit(Event::Error(err));
        } else {
          tracing::debug!(error = %err, "sortie toujours indisponible");
        }
        self.retry_at = Some(Instant::now() + OUTPUT_RETRY);
        false
      }
    }
  }

  fn open_output(&mut self) -> Result<OpenOutput, AudioError> {
    let (commands, command_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
    let (trash_tx, trash) = rtrb::RingBuffer::new(TRASH_CAPACITY);
    let mut parts = Some((command_rx, trash_tx));
    let (controls, analyser_ring, level_ring) = (
      self.controls.clone(),
      self.analyser_ring.clone(),
      self.level_ring.clone(),
    );
    let mut build = move |rate: u32| {
      let mut chain = Chain::new(ChainConfig {
        sample_rate: f64::from(rate),
        hq_oversample: Oversample::X2,
        output_stage: true,
      });
      chain.set_taps(Some(analyser_ring.clone()), Some(level_ring.clone()));
      let (command_rx, trash_tx) = parts.take().expect("mélangeur construit une seule fois");
      Mixer::new(chain, command_rx, trash_tx, controls.clone(), rate as f32)
    };
    let control = self.control.clone();
    let notifier: output::OutputNotifier = Arc::new(move |notice| {
      let _ = control.send(match notice {
        OutputNotice::Lost(reason) => Control::OutputLost(reason),
        OutputNotice::Rerouted => Control::OutputRerouted,
      });
    });
    let stream = self
      .backend
      .open(self.device.as_deref(), &mut build, notifier)?;
    let sample_rate = stream.sample_rate();
    Ok(OpenOutput {
      stream,
      commands,
      trash,
      sample_rate,
      suspended: false,
    })
  }

  /// Rouvre la sortie et reprend à la dernière position connue.
  fn reopen_output(&mut self) {
    self.output = None;
    self.voice = None;
    let was_playing = self.playing;
    if !self.ensure_output() {
      return;
    }
    self.resume_playing = false;
    if let Some(output) = &self.output {
      let name = output.stream.name();
      self.emit(Event::DeviceChanged(name));
    }
    if self.source.is_some() {
      self.start_voice(self.position, was_playing);
    }
  }

  fn spawn_voice(&self, source: Source, start: Duration, rate: u32) -> (Voice, ActiveVoice) {
    let frames = (BUFFER_SECONDS * f64::from(rate)) as usize;
    let (producer, voice, shared) = voice_channel(frames, rate);
    let (commands, command_rx) = mpsc::channel();
    let events = self.events.clone();
    let tempo = self.settings.tempo();
    let thread_shared = shared.clone();
    let task = DecodeTask {
      source,
      start,
      rate,
      tempo,
      events,
      shared: thread_shared,
    };
    let spawned = thread::Builder::new()
      .name("riff-audio-decode".into())
      .spawn(move || task.run(producer, &command_rx));
    if let Err(err) = spawned {
      self.emit(Event::Error(AudioError::Decode(err.to_string())));
    }
    (
      voice,
      ActiveVoice {
        shared,
        commands,
        seek_pending: None,
      },
    )
  }

  fn start_voice(&mut self, start: Duration, autoplay: bool) {
    let (Some(source), Some(output)) = (self.source.clone(), &self.output) else {
      return;
    };
    let rate = output.sample_rate;
    let (voice, active) = self.spawn_voice(source, start, rate);
    self.voice = Some(active);
    self.ended = false;
    self.buffering = false;
    self.position = start;
    self.playing = autoplay;
    self.last_emit = None;
    self.resume_stream();
    self.send_mixer(if autoplay {
      MixerCommand::Resume
    } else {
      MixerCommand::Pause
    });
    self.send_mixer(MixerCommand::SetVoice(voice));
    self.pause_at = None;
    self.suspend_at = (!autoplay).then(|| Instant::now() + SUSPEND_DELAY);
  }

  fn resume_stream(&mut self) {
    if let Some(output) = &mut self.output
      && output.suspended
    {
      output.stream.resume();
      output.suspended = false;
    }
  }

  fn load(&mut self, source: Source, start: Duration, autoplay: bool) {
    self.resume_playing = false;
    self.source = Some(source);
    self.position = start;
    self.playing = autoplay;
    self.voice = None;
    if self.ensure_output() {
      self.start_voice(start, autoplay);
    }
  }

  /// Nouvel essai d'ouverture : au succès, reprend là où la lecture s'était arrêtée.
  fn retry_output(&mut self) {
    if !self.ensure_output() {
      return;
    }
    let resume = std::mem::take(&mut self.resume_playing);
    if let Some(output) = &self.output {
      let name = output.stream.name();
      self.emit(Event::DeviceChanged(name));
    }
    if self.source.is_some() {
      self.start_voice(self.position, resume);
    }
  }

  fn play(&mut self) {
    if self.output.is_none() {
      self.playing = true;
      if !self.ensure_output() {
        return;
      }
      self.resume_playing = false;
      self.start_voice(self.position, true);
      return;
    }
    if self.voice.is_none() {
      self.start_voice(self.position, true);
      return;
    }
    self.pause_at = None;
    self.suspend_at = None;
    self.resume_stream();
    self.send_mixer(MixerCommand::Resume);
    self.playing = true;
  }

  fn pause(&mut self) {
    self.resume_playing = false;
    if !self.playing {
      return;
    }
    self.playing = false;
    self.send_mixer(MixerCommand::BeginPauseFade);
    self.pause_at = Some(Instant::now() + PAUSE_FADE);
  }

  fn stop(&mut self) {
    self.playing = false;
    self.resume_playing = false;
    self.retry_at = None;
    self.source = None;
    self.voice = None;
    self.position = Duration::ZERO;
    self.pause_at = None;
    self.send_mixer(MixerCommand::ClearVoice);
    self.send_mixer(MixerCommand::FlushEffects);
    self.suspend_at = Some(Instant::now() + SUSPEND_DELAY);
  }

  fn seek(&mut self, to: Duration) {
    self.position = to;
    self.ended = false;
    self.resume_stream();
    if !self.playing {
      self.suspend_at = Some(Instant::now() + SUSPEND_DELAY);
    }
    if let Some(voice) = &mut self.voice {
      let generation = voice.shared.request_seek();
      voice.seek_pending = Some(generation);
      if voice
        .commands
        .send(DecodeCommand::Seek { to, generation })
        .is_err()
      {
        tracing::warn!("fil de décodage arrêté, déplacement ignoré");
      }
    }
  }

  fn apply_settings(&mut self, settings: AudioSettings) {
    self.settings = settings;
    self.send_mixer(MixerCommand::Settings(settings));
    if let Some(voice) = &self.voice
      && voice
        .commands
        .send(DecodeCommand::Tempo(settings.tempo()))
        .is_err()
    {
      tracing::warn!("fil de décodage arrêté, vitesse ignorée");
    }
  }

  fn crossfade(&mut self, next: Source, seconds: f32) {
    self.source = Some(next.clone());
    if !self.ensure_output() {
      return;
    }
    let Some(rate) = self.output.as_ref().map(|output| output.sample_rate) else {
      return;
    };
    self.resume_stream();
    let (voice, active) = self.spawn_voice(next, Duration::ZERO, rate);
    // L'ancien fil de décodage continue pour la queue : il s'arrête quand sa voix est libérée.
    self.voice = Some(active);
    self.position = Duration::ZERO;
    self.ended = false;
    self.buffering = false;
    self.playing = true;
    self.last_emit = None;
    self.pause_at = None;
    self.suspend_at = None;
    self.send_mixer(MixerCommand::Resume);
    self.send_mixer(MixerCommand::Crossfade {
      next: voice,
      seconds,
    });
  }

  fn tick(&mut self) {
    let now = Instant::now();
    if self.retry_at.is_some_and(|at| now >= at) {
      self.retry_output();
    }
    if now >= self.next_device_poll {
      self.next_device_poll = now + DEVICE_POLL;
      if self
        .output
        .as_ref()
        .is_some_and(|output| output.stream.device_changed())
      {
        self.reopen_output();
      }
    }
    if let Some(output) = &mut self.output {
      while output.trash.pop().is_ok() {}
    }
    if self.pause_at.is_some_and(|at| now >= at) {
      self.pause_at = None;
      self.send_mixer(MixerCommand::Pause);
      self.suspend_at = Some(now + SUSPEND_DELAY);
    }
    if self.suspend_at.is_some_and(|at| now >= at) {
      self.suspend_at = None;
      if let Some(output) = &mut self.output
        && !output.suspended
      {
        output.stream.suspend();
        output.suspended = true;
      }
    }
    self.observe_voice(now);
  }

  fn observe_voice(&mut self, now: Instant) {
    let Some(voice) = &mut self.voice else {
      return;
    };
    if voice.shared.has_failed() {
      self.voice = None;
      self.playing = false;
      self.send_mixer(MixerCommand::ClearVoice);
      return;
    }
    if let Some(generation) = voice.seek_pending {
      if voice.shared.seek_acknowledged(generation) {
        voice.seek_pending = None;
      } else {
        return;
      }
    }
    let position = voice.shared.position();
    let (drained, starved) = (voice.shared.is_drained(), voice.shared.is_starved());
    if let Some(position) = position {
      self.position = position;
    }
    if drained && !self.ended {
      self.ended = true;
      self.playing = false;
      self.suspend_at = Some(now + SUSPEND_DELAY);
      self.emit(Event::Ended);
      return;
    }
    if !self.playing {
      return;
    }
    if starved != self.buffering {
      self.buffering = starved;
      self.emit(Event::Buffering(starved));
    }
    if position.is_some() && self.last_emit.is_none_or(|at| now - at >= POSITION_PERIOD) {
      self.last_emit = Some(now);
      self.emit(Event::Position(self.position));
    }
  }
}

/// Fil de décodage : source → décodeur → vitesse et rééchantillonnage → tampon du callback.
struct DecodeTask {
  source: Source,
  start: Duration,
  rate: u32,
  tempo: TempoParams,
  events: Sender<Event>,
  shared: Arc<VoiceShared>,
}

impl DecodeTask {
  fn fail(&self, err: AudioError) {
    self.shared.mark_failed();
    let _ = self.events.send(Event::Error(err));
  }

  fn run(self, mut producer: Producer<f32>, commands: &Receiver<DecodeCommand>) {
    let mut decoder = match Decoder::open(&self.source) {
      Ok(decoder) => decoder,
      Err(err) => return self.fail(err),
    };
    let _ = self.events.send(Event::Loaded {
      duration: decoder.duration(),
      sample_rate: decoder.sample_rate(),
    });
    let mut stage = match TempoStage::new(decoder.sample_rate(), self.rate) {
      Ok(stage) => stage,
      Err(err) => return self.fail(err),
    };
    if let Err(err) = stage.set_params(self.tempo) {
      return self.fail(err);
    }
    if !self.start.is_zero()
      && let Err(err) = decoder.seek(self.start)
    {
      let _ = self.events.send(Event::Error(err));
    }
    let mut speed = self.tempo.speed_factor();
    let (mut block, mut pending) = (Vec::new(), Vec::new());
    let (mut offset, mut written, mut eof) = (0usize, 0u64, false);
    while !producer.is_abandoned() {
      for command in commands.try_iter() {
        match command {
          DecodeCommand::Tempo(params) => {
            speed = params.speed_factor();
            if let Err(err) = stage.set_params(params) {
              let _ = self.events.send(Event::Error(err));
            }
          }
          DecodeCommand::Seek { to, generation } => {
            if let Err(err) = decoder.seek(to) {
              let _ = self.events.send(Event::Error(err));
            }
            stage.reset();
            pending.clear();
            offset = 0;
            eof = false;
            self.wait_for_flush(generation, &producer);
            written = self.shared.consumed_frames();
          }
        }
      }
      if offset < pending.len() {
        let pushed = push_samples(&mut producer, &pending[offset..]);
        offset += pushed;
        written += (pushed / 2) as u64;
        if pushed == 0 {
          thread::sleep(RING_WAIT);
        }
      } else if eof {
        self.shared.mark_finished();
        thread::sleep(IDLE_WAIT);
      } else {
        pending.clear();
        offset = 0;
        block.clear();
        let result = match decoder.next_block(&mut block) {
          Ok(Some(position)) => {
            self.shared.record_segment(written, position, speed);
            stage.process(&block, &mut pending)
          }
          Ok(None) => {
            eof = true;
            stage.finish(&mut pending)
          }
          Err(err) => {
            eof = true;
            let _ = self.events.send(Event::Error(err));
            stage.finish(&mut pending)
          }
        };
        if let Err(err) = result {
          let _ = self.events.send(Event::Error(err));
        }
      }
    }
  }

  fn wait_for_flush(&self, generation: u32, producer: &Producer<f32>) {
    self.wait_for_flush_within(generation, producer, SEEK_ACK_TIMEOUT);
  }

  fn wait_for_flush_within(&self, generation: u32, producer: &Producer<f32>, timeout: Duration) {
    self.shared.mark_seek_decoded(generation);
    let deadline = Instant::now() + timeout;
    while !self.shared.seek_acknowledged(generation)
      && self.shared.seek_request_now() == generation
      && !producer.is_abandoned()
      && Instant::now() < deadline
    {
      thread::sleep(SEEK_ACK_POLL);
    }
  }
}

/// Écrit des images entières dans le tampon, renvoie le nombre d'échantillons écrits.
fn push_samples(producer: &mut Producer<f32>, samples: &[f32]) -> usize {
  let count = producer.slots().min(samples.len()) & !1;
  if count == 0 {
    return 0;
  }
  match producer.write_chunk_uninit(count) {
    Ok(chunk) => chunk.fill_from_iter(samples[..count].iter().copied()),
    Err(_) => 0,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::decode::test_support::ramp_wav;
  use crate::mixer::Mixer;
  use crate::output::OutputNotifier;
  use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
  use std::sync::{Arc, Mutex};
  use std::thread;
  use std::time::Instant;

  const RATE: u32 = 48_000;
  const TIMEOUT: Duration = Duration::from_secs(5);

  /// Sortie simulée : appelle le mélangeur depuis un fil, à vitesse réelle multipliée par `speed`.
  #[derive(Clone)]
  struct FakeOutput {
    state: Arc<FakeState>,
  }

  struct FakeState {
    available: AtomicBool,
    speed: Mutex<u32>,
    opens: AtomicU64,
    frames: AtomicU64,
    peak_bits: AtomicU64,
    captured: Mutex<Vec<f32>>,
    default_changed: AtomicBool,
    notifier: Mutex<Option<OutputNotifier>>,
  }

  impl FakeOutput {
    fn new(speed: u32) -> Self {
      Self {
        state: Arc::new(FakeState {
          available: AtomicBool::new(true),
          speed: Mutex::new(speed),
          opens: AtomicU64::new(0),
          frames: AtomicU64::new(0),
          peak_bits: AtomicU64::new(0),
          captured: Mutex::new(Vec::new()),
          default_changed: AtomicBool::new(false),
          notifier: Mutex::new(None),
        }),
      }
    }

    fn peak(&self) -> f64 {
      f64::from_bits(self.state.peak_bits.load(Ordering::Relaxed))
    }

    fn reset_peak(&self) {
      self.state.peak_bits.store(0, Ordering::Relaxed);
    }

    fn unplug(&self) {
      let notifier = self.state.notifier.lock().expect("verrou").take();
      if let Some(notifier) = notifier {
        notifier(OutputNotice::Lost("périphérique débranché".to_string()));
      }
    }

    fn reroute(&self) {
      if let Some(notifier) = self.state.notifier.lock().expect("verrou").as_ref() {
        notifier(OutputNotice::Rerouted);
      }
    }
  }

  struct FakeStream {
    stop: Arc<AtomicBool>,
    suspended: Arc<AtomicBool>,
    state: Arc<FakeState>,
    thread: Option<thread::JoinHandle<()>>,
  }

  impl OutputStream for FakeStream {
    fn sample_rate(&self) -> u32 {
      RATE
    }

    fn name(&self) -> String {
      "simulé".to_string()
    }

    fn device_changed(&self) -> bool {
      self.state.default_changed.swap(false, Ordering::AcqRel)
    }

    fn suspend(&self) {
      self.suspended.store(true, Ordering::Release);
    }

    fn resume(&self) {
      self.suspended.store(false, Ordering::Release);
    }
  }

  impl Drop for FakeStream {
    fn drop(&mut self) {
      self.stop.store(true, Ordering::Release);
      if let Some(thread) = self.thread.take() {
        let _ = thread.join();
      }
    }
  }

  impl OutputBackend for FakeOutput {
    fn open(
      &mut self,
      _device: Option<&str>,
      build: &mut dyn FnMut(u32) -> Mixer,
      notifier: OutputNotifier,
    ) -> Result<Box<dyn OutputStream>, AudioError> {
      if !self.state.available.load(Ordering::Acquire) {
        return Err(AudioError::NoDevice);
      }
      self.state.opens.fetch_add(1, Ordering::Relaxed);
      *self.state.notifier.lock().expect("verrou") = Some(notifier);
      let mut mixer = build(RATE);
      let stop = Arc::new(AtomicBool::new(false));
      let suspended = Arc::new(AtomicBool::new(false));
      let state = self.state.clone();
      let stream_state = self.state.clone();
      let (thread_stop, thread_suspended) = (stop.clone(), suspended.clone());
      let thread = thread::spawn(move || {
        let (mut left, mut right) = (vec![0.0; 480], vec![0.0; 480]);
        while !thread_stop.load(Ordering::Acquire) {
          if !thread_suspended.load(Ordering::Acquire) {
            mixer.render(&mut left, &mut right);
            state.frames.fetch_add(480, Ordering::Relaxed);
            state
              .captured
              .lock()
              .expect("verrou")
              .extend_from_slice(&left);
            let peak = left.iter().fold(0f32, |m, v| m.max(v.abs()));
            let previous = f64::from_bits(state.peak_bits.load(Ordering::Relaxed));
            if f64::from(peak) > previous {
              state
                .peak_bits
                .store(f64::from(peak).to_bits(), Ordering::Relaxed);
            }
          }
          let speed = *state.speed.lock().expect("verrou");
          thread::sleep(Duration::from_micros(10_000 / u64::from(speed.max(1))));
        }
      });
      Ok(Box::new(FakeStream {
        stop,
        suspended,
        state: stream_state,
        thread: Some(thread),
      }))
    }
  }

  fn tone(seconds: u32) -> Source {
    Source::from(ramp_wav(RATE, 2, RATE * seconds))
  }

  fn engine(output: &FakeOutput) -> (Engine, Receiver<Event>) {
    Engine::with_backend(EngineConfig::default(), Box::new(output.clone())).expect("moteur")
  }

  /// Garde contre un test suspendu (fil de sortie simulée mort) : jamais atteinte quand la machine
  /// est seulement lente.
  const HANG_GUARD: Duration = Duration::from_secs(60);

  /// Comme `wait_for`, mais la limite est un budget de temps audio rendu par la sortie simulée, pas
  /// une durée d'horloge : chaque bloc rendu coûte un réveil du fil de sortie, qui sous charge dure
  /// des dizaines de millisecondes. Un événement qui n'arrive pas malgré `budget` de son rendu est
  /// un vrai défaut du moteur ; `HANG_GUARD` borne seulement la durée d'un test suspendu.
  fn wait_for_rendered(
    events: &Receiver<Event>,
    output: &FakeOutput,
    budget: Duration,
    what: &str,
    test: impl Fn(&Event) -> bool,
  ) -> Event {
    let start = output.state.frames.load(Ordering::Relaxed);
    let budget_frames = (budget.as_secs_f64() * f64::from(RATE)) as u64;
    let guard = Instant::now() + HANG_GUARD;
    loop {
      match events.recv_timeout(Duration::from_millis(10)) {
        Ok(event) if test(&event) => return event,
        Ok(_) | Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => panic!("moteur arrêté avant : {what}"),
      }
      let rendered = output.state.frames.load(Ordering::Relaxed) - start;
      assert!(
        rendered < budget_frames,
        "événement attendu non reçu après {budget:?} de son rendu : {what}"
      );
      assert!(Instant::now() < guard, "test suspendu : {what}");
    }
  }

  fn wait_for(events: &Receiver<Event>, what: &str, test: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + TIMEOUT;
    loop {
      let remaining = deadline.saturating_duration_since(Instant::now());
      match events.recv_timeout(remaining) {
        Ok(event) if test(&event) => return event,
        Ok(_) => {}
        Err(_) => panic!("événement attendu non reçu : {what}"),
      }
    }
  }

  #[test]
  fn load_reports_properties_positions_and_the_end() {
    let output = FakeOutput::new(10);
    let (engine, events) = engine(&output);
    engine.load(tone(1), Duration::ZERO, true);
    let loaded = wait_for(&events, "Loaded", |e| matches!(e, Event::Loaded { .. }));
    assert_eq!(
      loaded,
      Event::Loaded {
        duration: Some(Duration::from_secs(1)),
        sample_rate: RATE
      }
    );
    let budget = Duration::from_secs(10);
    let position = |e: &Event| matches!(e, Event::Position(_));
    wait_for_rendered(&events, &output, budget, "Position", position);
    wait_for_rendered(&events, &output, budget, "Ended", |e| {
      matches!(e, Event::Ended)
    });
  }

  #[test]
  fn start_offset_is_honoured() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(4), Duration::from_secs(2), true);
    let position = wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    let Event::Position(at) = position else {
      unreachable!()
    };
    assert!(
      at >= Duration::from_secs(2) && at < Duration::from_secs(3),
      "{at:?}"
    );
  }

  #[test]
  fn positions_are_emitted_at_most_four_times_per_second() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(10), Duration::ZERO, true);
    let budget = Duration::from_secs(10);
    wait_for_rendered(&events, &output, budget, "Position", |e| {
      matches!(e, Event::Position(_))
    });
    // Fenêtre de 1,5 s de son rendu, pas d'horloge : la sortie simulée ne rend jamais plus vite que
    // le temps réel, donc la fenêtre dure au moins 1,5 s, davantage sous charge. La cadence est
    // limitée par l'horloge (`POSITION_PERIOD`) : on la compare à la durée réelle mesurée.
    let window = Instant::now();
    let rendered_at_start = output.state.frames.load(Ordering::Relaxed);
    let window_frames = (1.5 * f64::from(RATE)) as u64;
    let mut count = 0u32;
    while output.state.frames.load(Ordering::Relaxed) - rendered_at_start < window_frames {
      if let Ok(Event::Position(_)) = events.recv_timeout(Duration::from_millis(10)) {
        count += 1;
      }
      assert!(window.elapsed() < HANG_GUARD, "test suspendu : positions");
    }
    let elapsed = window.elapsed();
    // Un événement par période au plus, plus un de marge (le premier a été émis avant la fenêtre).
    let at_most = (elapsed.as_secs_f64() / POSITION_PERIOD.as_secs_f64()) as u32 + 1;
    assert!(
      (2..=at_most).contains(&count),
      "{count} positions en {elapsed:?} (de 2 à {at_most} attendues)"
    );
  }

  #[test]
  fn seek_moves_the_position() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(10), Duration::ZERO, true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    engine.seek(Duration::from_secs(7));
    wait_for(
      &events,
      "Position après déplacement",
      |e| matches!(e, Event::Position(at) if *at >= Duration::from_secs(7)),
    );
  }

  #[test]
  fn segmented_source_reports_the_sum_of_durations_and_seeks() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    let (source, _) = crate::decode::test_support::hls_fixture();
    engine.load(source, Duration::ZERO, true);
    let loaded = wait_for(&events, "Loaded", |e| matches!(e, Event::Loaded { .. }));
    let Event::Loaded { duration, .. } = loaded else {
      unreachable!()
    };
    let secs = duration.expect("durée").as_secs_f64();
    assert!((secs - 3.041_814).abs() < 1e-6, "{secs} s");
    engine.seek(Duration::from_millis(2500));
    wait_for(
      &events,
      "Position après déplacement",
      |e| matches!(e, Event::Position(at) if *at >= Duration::from_millis(2500)),
    );
  }

  #[test]
  fn superseded_seek_does_not_wait_for_the_previous_flush_timeout() {
    let (producer, _voice, shared) = voice_channel(1024, RATE);
    let (events, _rx) = mpsc::channel();
    let task = DecodeTask {
      source: tone(2),
      start: Duration::ZERO,
      rate: RATE,
      tempo: TempoParams::default(),
      events,
      shared: shared.clone(),
    };
    let g1 = shared.request_seek();
    // Délai bien plus long que `TIMEOUT` : seul un déplacement plus récent peut libérer le fil.
    let waiter = thread::spawn(move || {
      task.wait_for_flush_within(g1, &producer, Duration::from_secs(60));
      producer
    });
    thread::sleep(Duration::from_millis(5));
    let _g2 = shared.request_seek();
    wait_until("fil libéré par le second déplacement", || {
      waiter.is_finished()
    });
    let _producer = waiter.join().expect("fil de décodage");
  }

  #[test]
  fn pause_stops_the_positions_and_play_resumes_them() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(10), Duration::ZERO, true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    engine.pause();
    thread::sleep(Duration::from_millis(300));
    while events.try_recv().is_ok() {}
    assert!(
      events.recv_timeout(Duration::from_millis(400)).is_err(),
      "aucun événement en pause"
    );
    output.reset_peak();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(output.peak(), 0.0, "silence en pause");
    engine.play();
    wait_for(&events, "Position après reprise", |e| {
      matches!(e, Event::Position(_))
    });
  }

  #[test]
  fn load_without_autoplay_stays_silent_until_play() {
    let output = FakeOutput::new(5);
    let (engine, events) = engine(&output);
    engine.load(tone(2), Duration::ZERO, false);
    wait_for(&events, "Loaded", |e| matches!(e, Event::Loaded { .. }));
    thread::sleep(Duration::from_millis(200));
    assert_eq!(output.peak(), 0.0);
    engine.play();
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
  }

  #[test]
  fn unreadable_source_reports_an_error() {
    let output = FakeOutput::new(5);
    let (engine, events) = engine(&output);
    engine.load(Source::from(vec![0u8; 512]), Duration::ZERO, true);
    wait_for(&events, "Error", |e| {
      matches!(e, Event::Error(AudioError::Decode(_)))
    });
    let after = events.recv_timeout(Duration::from_millis(400));
    assert!(
      after.is_err(),
      "rien après l'erreur, surtout pas Ended : {after:?}"
    );
    engine.play();
    assert!(
      events.recv_timeout(Duration::from_millis(500)).is_ok(),
      "nouvel essai signalé"
    );
  }

  #[test]
  fn automatic_reroute_reports_the_device_without_reopening() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(20), Duration::ZERO, true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    output.reroute();
    wait_for(&events, "DeviceChanged", |e| {
      matches!(e, Event::DeviceChanged(_))
    });
    assert_eq!(
      output.state.opens.load(Ordering::Relaxed),
      1,
      "le flux reste ouvert"
    );
    wait_for(&events, "Position après bascule", |e| {
      matches!(e, Event::Position(_))
    });
  }

  #[test]
  fn no_device_reports_an_error_without_panicking() {
    let output = FakeOutput::new(5);
    output.state.available.store(false, Ordering::Release);
    let (engine, events) = engine(&output);
    engine.load(tone(1), Duration::ZERO, true);
    wait_for(&events, "NoDevice", |e| {
      matches!(e, Event::Error(AudioError::NoDevice))
    });
    output.state.available.store(true, Ordering::Release);
    engine.play();
    wait_for(&events, "Position après retour du périphérique", |e| {
      matches!(e, Event::Position(_))
    });
  }

  #[test]
  fn device_loss_reopens_the_output_and_resumes_at_the_last_position() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(20), Duration::from_secs(5), true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    output.unplug();
    wait_for(&events, "DeviceChanged", |e| {
      matches!(e, Event::DeviceChanged(_))
    });
    assert_eq!(output.state.opens.load(Ordering::Relaxed), 2);
    let position = wait_for(&events, "Position après reprise", |e| {
      matches!(e, Event::Position(_))
    });
    let Event::Position(at) = position else {
      unreachable!()
    };
    assert!(
      at >= Duration::from_secs(5) && at < Duration::from_secs(7),
      "{at:?}"
    );
    drop(engine);
  }

  #[test]
  fn device_loss_without_replacement_reports_no_device() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(20), Duration::ZERO, true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    output.state.available.store(false, Ordering::Release);
    output.unplug();
    wait_for(&events, "NoDevice", |e| {
      matches!(e, Event::Error(AudioError::NoDevice))
    });
    drop(engine);
  }

  #[test]
  fn change_of_default_device_reopens_the_output_at_the_last_position() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(20), Duration::from_secs(5), true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    output.state.default_changed.store(true, Ordering::Release);
    wait_for(&events, "DeviceChanged", |e| {
      matches!(e, Event::DeviceChanged(_))
    });
    assert_eq!(output.state.opens.load(Ordering::Relaxed), 2);
    let position = wait_for(&events, "Position après bascule", |e| {
      matches!(e, Event::Position(_))
    });
    let Event::Position(at) = position else {
      unreachable!()
    };
    assert!(
      at >= Duration::from_secs(5) && at < Duration::from_secs(7),
      "{at:?}"
    );
  }

  #[test]
  fn playback_resumes_by_itself_when_a_device_comes_back() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(20), Duration::from_secs(5), true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    output.state.available.store(false, Ordering::Release);
    output.unplug();
    wait_for(&events, "NoDevice", |e| {
      matches!(e, Event::Error(AudioError::NoDevice))
    });
    thread::sleep(Duration::from_millis(700));
    output.state.available.store(true, Ordering::Release);
    wait_for(&events, "DeviceChanged", |e| {
      matches!(e, Event::DeviceChanged(_))
    });
    let position = wait_for(&events, "Position après retour", |e| {
      matches!(e, Event::Position(_))
    });
    let Event::Position(at) = position else {
      unreachable!()
    };
    assert!(
      at >= Duration::from_secs(5) && at < Duration::from_secs(7),
      "{at:?}"
    );
  }

  #[test]
  fn missing_device_reports_a_single_error_while_retrying() {
    let output = FakeOutput::new(1);
    output.state.available.store(false, Ordering::Release);
    let (engine, events) = engine(&output);
    engine.load(tone(20), Duration::ZERO, true);
    wait_for(&events, "NoDevice", |e| {
      matches!(e, Event::Error(AudioError::NoDevice))
    });
    assert!(
      events.recv_timeout(Duration::from_millis(1500)).is_err(),
      "une seule erreur pendant les nouveaux essais"
    );
  }

  #[test]
  fn pause_during_the_absence_stays_paused_when_the_device_returns() {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.load(tone(20), Duration::ZERO, true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    output.state.available.store(false, Ordering::Release);
    output.unplug();
    wait_for(&events, "NoDevice", |e| {
      matches!(e, Event::Error(AudioError::NoDevice))
    });
    engine.pause();
    thread::sleep(Duration::from_millis(100));
    output.state.available.store(true, Ordering::Release);
    wait_for(&events, "DeviceChanged", |e| {
      matches!(e, Event::DeviceChanged(_))
    });
    wait_for(&events, "Loaded", |e| matches!(e, Event::Loaded { .. }));
    output.reset_peak();
    assert!(
      events.recv_timeout(Duration::from_millis(500)).is_err(),
      "reste en pause"
    );
    assert_eq!(output.peak(), 0.0);
  }

  /// Le tampon d'une voix dure 250 ms : la queue d'un fondu doit continuer à être décodée.
  #[test]
  fn crossfade_tail_keeps_decoding_past_its_buffer() {
    let output = FakeOutput::new(10);
    let (engine, events) = engine(&output);
    engine.load(tone(30), Duration::ZERO, true);
    wait_for(&events, "premier Loaded", |e| {
      matches!(e, Event::Loaded { .. })
    });
    let mut silent = ramp_wav(RATE, 2, RATE * 30);
    assert_eq!(&silent[36..40], b"data");
    silent[44..].fill(0);
    engine.crossfade_to(Source::from(silent), 2.0);
    wait_for(&events, "second Loaded", |e| {
      matches!(e, Event::Loaded { .. })
    });
    output.state.captured.lock().expect("verrou").clear();
    let frames = |seconds: f32| (seconds * RATE as f32) as usize;
    wait_until_within("sortie rendue", HANG_GUARD, || {
      output.state.captured.lock().expect("verrou").len() >= frames(1.2)
    });
    let samples = output.state.captured.lock().expect("verrou").clone();
    assert!(samples.len() >= frames(1.2), "sortie trop courte");
    let window = &samples[frames(0.8)..frames(1.2)];
    let peak = window.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.01, "queue muette après son tampon : crête {peak}");
  }

  #[test]
  fn crossfade_loads_the_next_title_and_reports_its_end_only() {
    let output = FakeOutput::new(10);
    let (engine, events) = engine(&output);
    engine.load(tone(30), Duration::ZERO, true);
    let budget = Duration::from_secs(10);
    wait_for_rendered(&events, &output, budget, "premier Loaded", |e| {
      matches!(e, Event::Loaded { .. })
    });
    engine.crossfade_to(tone(1), 0.5);
    let next = wait_for_rendered(&events, &output, budget, "second Loaded", |e| {
      matches!(e, Event::Loaded { .. })
    });
    assert_eq!(
      next,
      Event::Loaded {
        duration: Some(Duration::from_secs(1)),
        sample_rate: RATE
      }
    );
    wait_for_rendered(&events, &output, budget, "Ended du titre suivant", |e| {
      matches!(e, Event::Ended)
    });
  }
  /// Attend que la sortie simulée ait rendu `frames` échantillons de plus : l'attente suit le temps
  /// audio rendu, pas l'horloge, donc une machine chargée n'affame pas le test.
  fn wait_rendered(output: &FakeOutput, frames: u64) {
    let target = output.state.frames.load(Ordering::Relaxed) + frames;
    wait_until_within("échantillons rendus", HANG_GUARD, || {
      output.state.frames.load(Ordering::Relaxed) >= target
    });
  }

  fn wait_until(what: &str, condition: impl FnMut() -> bool) {
    wait_until_within(what, TIMEOUT, condition);
  }

  fn wait_until_within(what: &str, limit: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !condition() {
      assert!(Instant::now() < deadline, "condition non atteinte : {what}");
      thread::sleep(Duration::from_millis(1));
    }
  }

  #[test]
  fn volume_settings_and_hq_reach_the_output() {
    let output = FakeOutput::new(2);
    let (engine, events) = engine(&output);
    engine.set_volume(1.0, true);
    engine.load(tone(10), Duration::from_secs(1), true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    // 50 ms puis 100 ms de signal rendu (sortie à 48 kHz).
    wait_rendered(&output, 2_400);
    output.reset_peak();
    wait_rendered(&output, 4_800);
    assert_eq!(output.peak(), 0.0, "muet");
    engine.set_volume(1.0, false);
    engine.set_hq(true, HqSettings::default());
    engine.set_settings(AudioSettings {
      reverb: 30.0,
      ..AudioSettings::default()
    });
    engine.set_normalize_gain_db(3.0);
    wait_until("son rétabli", || output.peak() > 0.0);
  }
  fn sine_wav(freq: f64, amplitude: f64, seconds: u32) -> Source {
    let frames = RATE * seconds;
    let data_size = frames * 4;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for i in 0..frames {
      let t = f64::from(i) / f64::from(RATE);
      let value = (amplitude * (std::f64::consts::TAU * freq * t).sin() * 32_767.0) as i16;
      bytes.extend_from_slice(&value.to_le_bytes());
      bytes.extend_from_slice(&value.to_le_bytes());
    }
    Source::from(bytes)
  }

  fn settled_peak(settings: AudioSettings) -> f64 {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.set_volume(1.0, false);
    // Même ordre que l'exemple play : réglages d'abord, chargement ensuite.
    engine.set_settings(settings);
    engine.load(sine_wav(64.0, 0.05, 10), Duration::ZERO, true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    thread::sleep(Duration::from_millis(400));
    output.reset_peak();
    thread::sleep(Duration::from_millis(300));
    output.peak()
  }

  #[test]
  fn equalizer_set_before_loading_changes_the_output() {
    let neutral = settled_peak(AudioSettings::default());
    let eq = crate::dsp::eq::EqGains {
      bands: [9.0, 9.0, 9.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
      ..Default::default()
    };
    let boosted = settled_peak(AudioSettings {
      eq,
      ..AudioSettings::default()
    });
    let gain_db = 20.0 * (boosted / neutral).log10();
    eprintln!("neutre {neutral:.4}, égaliseur {boosted:.4}, écart {gain_db:.2} dB");
    assert!(gain_db > 6.0, "écart mesuré {gain_db:.2} dB");
  }

  fn output_frequency(settings: AudioSettings) -> f64 {
    let output = FakeOutput::new(1);
    let (engine, events) = engine(&output);
    engine.set_volume(1.0, false);
    engine.set_settings(settings);
    engine.load(sine_wav(1000.0, 0.1, 10), Duration::ZERO, true);
    let budget = Duration::from_secs(10);
    wait_for_rendered(&events, &output, budget, "Position", |e| {
      matches!(e, Event::Position(_))
    });
    wait_rendered(&output, u64::from(RATE / 2));
    output.state.captured.lock().expect("verrou").clear();
    // Une demi-seconde de son rendu, pas d'horloge : sous charge, l'horloge en rendrait moins.
    wait_until_within("demi-seconde rendue", HANG_GUARD, || {
      output.state.captured.lock().expect("verrou").len() >= (RATE / 2) as usize
    });
    let samples = output.state.captured.lock().expect("verrou").clone();
    let rising: Vec<usize> = samples
      .windows(2)
      .enumerate()
      .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
      .map(|(i, _)| i)
      .collect();
    assert!(rising.len() > 10, "signal absent en sortie");
    let span = (rising[rising.len() - 1] - rising[0]) as f64;
    (rising.len() - 1) as f64 * f64::from(RATE) / span
  }

  #[test]
  fn speed_pitch_lowers_the_frequency_at_the_output() {
    let neutral = output_frequency(AudioSettings::default());
    assert!((neutral - 1000.0).abs() < 5.0, "neutre {neutral} Hz");
    let slowed = output_frequency(AudioSettings {
      speed_pitch: 0.82,
      ..AudioSettings::default()
    });
    assert!(
      (slowed - 820.0).abs() < 5.0,
      "speedPitch 0,82 : {slowed} Hz"
    );
  }

  #[test]
  fn analyser_and_level_see_the_output_without_the_equalizer() {
    let output = FakeOutput::new(2);
    let (engine, events) = engine(&output);
    engine.set_volume(1.0, false);
    engine.load(tone(10), Duration::from_secs(1), true);
    wait_for(&events, "Position", |e| matches!(e, Event::Position(_)));
    let mut time = [0u8; 128];
    wait_until_within("signal visible", HANG_GUARD, || {
      engine.time_bytes(&mut time);
      time.iter().any(|v| *v != 128)
    });
    wait_until_within("égaliseur contourné : niveau mesuré", HANG_GUARD, || {
      engine.level_db() > -120.0
    });
    let mut freq = [0u8; 64];
    engine.frequency_bytes(&mut freq);
    engine.set_analyser_hidden(true);
    let mut hidden = [0u8; 16];
    engine.frequency_bytes(&mut hidden);

    let eq = crate::dsp::eq::EqGains {
      low: 3.0,
      ..Default::default()
    };
    engine.set_settings(AudioSettings {
      eq,
      ..AudioSettings::default()
    });
    wait_until_within("égaliseur actif : niveau mesuré", HANG_GUARD, || {
      engine.level_db() > -120.0
    });
  }

  #[test]
  fn every_command_without_a_device_is_harmless_and_silent() {
    let output = FakeOutput::new(1);
    output.state.available.store(false, Ordering::Release);
    let (engine, events) = engine(&output);
    engine.set_volume(0.5, false);
    engine.set_settings(AudioSettings {
      reverb: 30.0,
      echo: 30.0,
      distortion: 30.0,
      ..AudioSettings::default()
    });
    engine.set_hq(true, HqSettings::default());
    engine.set_normalize_gain_db(3.0);
    engine.load(tone(1), Duration::ZERO, true);
    engine.play();
    engine.seek(Duration::from_millis(500));
    engine.fade_out(0.1);
    engine.fade_in(0.1);
    engine.crossfade_to(tone(1), 0.1);
    engine.pause();
    engine.set_output_device(Some("absent".to_string()));
    engine.stop();
    // Les commandes sont traitées dans l'ordre : fermer le moteur joint le fil de contrôle, ce
    // qui rend la liste d'événements complète sans attente chronométrée.
    let mut time = [0u8; 64];
    engine.time_bytes(&mut time);
    assert!(time.iter().all(|v| *v == 128), "aucun signal sans sortie");
    assert!(engine.level_db() <= -120.0);
    drop(engine);
    let received: Vec<Event> = events.try_iter().collect();
    assert!(
      received
        .iter()
        .any(|e| matches!(e, Event::Error(AudioError::NoDevice))),
      "{received:?}"
    );
    assert!(
      received.iter().all(|e| matches!(
        e,
        Event::Error(_) | Event::Buffering(_) | Event::Loaded { .. }
      )),
      "aucune position ni fin sans sortie : {received:?}"
    );
    assert_eq!(output.state.opens.load(Ordering::Relaxed), 0);
    assert_eq!(output.state.frames.load(Ordering::Relaxed), 0);
  }

  /// Latence de seek en images rendues (temps audio, pas horloge) : entre l'appel de `seek` et la
  /// première image de la nouvelle position dans la sortie simulée.
  #[test]
  fn seek_reaches_the_output_within_a_quarter_of_a_second_of_audio() {
    let output = FakeOutput::new(2);
    let (engine, events) = engine(&output);
    engine.set_volume(1.0, false);
    engine.load(tone(10), Duration::ZERO, true);
    wait_for_rendered(&events, &output, Duration::from_secs(10), "Position", |e| {
      matches!(e, Event::Position(_))
    });
    wait_rendered(&output, 4_800);
    let before = output.state.captured.lock().expect("verrou").len();
    // La rampe de `tone` vaut `image / 32768` : l'image 30 000 est à 0,92, loin de l'avant-seek.
    engine.seek(Duration::from_millis(625));
    let mut index = None;
    // Borné par deux secondes de son rendu : passé ce budget, le seek n'a pas atteint la sortie.
    wait_until_within("nouvelle position en sortie", HANG_GUARD, || {
      let captured = output.state.captured.lock().expect("verrou");
      index = captured[before..].iter().position(|v| *v > 0.6);
      index.is_some() || captured.len() - before > 2 * RATE as usize
    });
    let latency = index.expect("trouvé") as f64 / f64::from(RATE);
    eprintln!(
      "latence de seek : {:.1} ms de temps audio",
      latency * 1000.0
    );
    assert!(latency < 0.25, "{latency:.3} s");
    let max_before = output.state.captured.lock().expect("verrou")[..before]
      .iter()
      .fold(0f32, |m, v| m.max(*v));
    assert!(
      max_before < 0.6,
      "contenu avant le déplacement : {max_before}"
    );
  }
}
