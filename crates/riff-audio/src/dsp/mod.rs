//! Chaîne de traitement du signal.

pub mod biquad;
pub mod dynamics;
pub mod echo;
pub mod eq;
pub mod hq;
pub mod reverb;
pub mod waveshaper;

use std::sync::Arc;

use crate::AudioSettings;
use crate::analyser::SampleRing;
use dynamics::{Compressor, CompressorParams};
use echo::Echo;
use eq::Equalizer;
use hq::{HqChain, HqSettings};
use reverb::Reverb;
use waveshaper::{Oversample, WaveShaper, distortion_curve};

/// Quantum de rendu de Web Audio.
pub const QUANTUM: usize = 128;

const NORM_LIMITER: CompressorParams = CompressorParams {
  threshold: -2.0,
  knee: 0.0,
  ratio: 20.0,
  attack: 0.003,
  release: 0.1,
};
const NORM_TIME_CONSTANT: f64 = 0.8;
const PAUSE_TIME_CONSTANT: f64 = 0.035;
const FADE_FLOOR: f32 = 0.001;

/// Valeur appliquée en rampe linéaire sur un quantum, pour éviter les claquements.
#[derive(Clone, Copy, Debug)]
struct Ramp {
  current: f32,
  target: f32,
}

impl Ramp {
  const fn new(value: f32) -> Self {
    Self {
      current: value,
      target: value,
    }
  }

  #[inline]
  fn apply_to(&mut self, next: f32, samples: [&mut [f32]; 2]) {
    let frames = samples[0].len();
    if self.current == next {
      if next != 1.0 {
        for channel in samples {
          channel.iter_mut().for_each(|v| *v *= next);
        }
      }
    } else {
      let step = (next - self.current) / frames as f32;
      for channel in samples {
        let mut gain = self.current;
        for sample in channel.iter_mut() {
          gain += step;
          *sample *= gain;
        }
      }
    }
    self.current = next;
  }

  #[inline]
  fn step_to_target(&mut self, samples: [&mut [f32]; 2]) {
    self.apply_to(self.target, samples);
  }

  /// Approche exponentielle de la cible (`setTargetAtTime`).
  #[inline]
  fn step_exponential(&mut self, coefficient: f32, samples: [&mut [f32]; 2]) {
    let next = self.current + (self.target - self.current) * coefficient;
    let next = if (next - self.target).abs() < 1e-6 {
      self.target
    } else {
      next
    };
    self.apply_to(next, samples);
  }
}

/// Fondu exponentiel de `masterGain` (`exponentialRampToValueAtTime`).
#[derive(Clone, Copy, Debug)]
struct MasterFade {
  remaining: usize,
  /// Rapport par échantillon.
  ratio: f64,
  end: f32,
}

/// Configuration fixe d'une chaîne.
#[derive(Clone, Copy, Debug)]
pub struct ChainConfig {
  pub sample_rate: f64,
  pub hq_oversample: Oversample,
  /// `normGain` et `normLimiter` : présents en direct, absents du rendu hors ligne.
  pub output_stage: bool,
}

/// Traitement complet d'un flux stéréo. Aucune allocation dans `process` ni dans les réglages.
pub struct Chain {
  fs: f64,
  output_stage: bool,
  eq: Equalizer,
  echo: Echo,
  reverb: Reverb,
  distortion: WaveShaper,
  hq: HqChain,
  limiter: Compressor,
  hq_enabled: bool,
  reverb_on: bool,
  echo_on: bool,
  distortion_on: bool,
  distortion_amount: f32,
  dry: Ramp,
  distortion_gain: Ramp,
  echo_gain: Ramp,
  reverb_gain: Ramp,
  element: Ramp,
  master: Ramp,
  master_base: f32,
  muted: bool,
  fade: Option<MasterFade>,
  norm: Ramp,
  norm_coefficient: f32,
  pause: Ramp,
  pause_coefficient: f32,
  bus: [Box<[f32; QUANTUM]>; 2],
  wet: [Box<[f32; QUANTUM]>; 2],
  level_tap: Option<Arc<SampleRing>>,
  analyser_tap: Option<Arc<SampleRing>>,
}

fn boxed_buffers() -> [Box<[f32; QUANTUM]>; 2] {
  [Box::new([0.0; QUANTUM]), Box::new([0.0; QUANTUM])]
}

fn exponential_coefficient(frames: usize, time_constant: f64, fs: f64) -> f32 {
  (1.0 - (-(frames as f64) / (fs * time_constant)).exp()) as f32
}

impl Chain {
  pub fn new(config: ChainConfig) -> Self {
    let fs = config.sample_rate;
    Self {
      fs,
      output_stage: config.output_stage,
      eq: Equalizer::new(fs),
      echo: Echo::new(fs),
      reverb: Reverb::new(fs),
      distortion: WaveShaper::new(distortion_curve(0.0), Oversample::None),
      hq: HqChain::new(fs, config.hq_oversample, HqSettings::default()),
      limiter: Compressor::new(fs as f32, NORM_LIMITER),
      hq_enabled: false,
      reverb_on: false,
      echo_on: false,
      distortion_on: false,
      distortion_amount: 0.0,
      dry: Ramp::new(1.0),
      distortion_gain: Ramp::new(0.0),
      echo_gain: Ramp::new(0.0),
      reverb_gain: Ramp::new(0.0),
      element: Ramp::new(1.0),
      master: Ramp::new(1.0),
      master_base: 1.0,
      muted: false,
      fade: None,
      norm: Ramp::new(1.0),
      norm_coefficient: exponential_coefficient(QUANTUM, NORM_TIME_CONSTANT, fs),
      pause: Ramp::new(1.0),
      pause_coefficient: exponential_coefficient(QUANTUM, PAUSE_TIME_CONSTANT, fs),
      bus: boxed_buffers(),
      wet: boxed_buffers(),
      level_tap: None,
      analyser_tap: None,
    }
  }

  /// Amène tous les réglages à leur cible sans lissage (rendu hors ligne).
  pub fn settle(&mut self) {
    self.eq.settle();
    for ramp in [
      &mut self.dry,
      &mut self.distortion_gain,
      &mut self.echo_gain,
      &mut self.reverb_gain,
      &mut self.element,
      &mut self.master,
      &mut self.norm,
    ] {
      ramp.current = ramp.target;
    }
  }

  /// Prises de l'analyseur (après le limiteur) et du niveau (après `eqHigh`).
  pub fn set_taps(&mut self, analyser: Option<Arc<SampleRing>>, level: Option<Arc<SampleRing>>) {
    self.analyser_tap = analyser;
    self.level_tap = level;
  }

  pub fn set_settings(&mut self, settings: &AudioSettings) {
    self.eq.set_gains(&settings.eq);
    let reverb = settings.reverb.max(0.0);
    let echo = settings.echo.max(0.0);
    let distortion = settings.distortion.max(0.0);
    self.reverb_on = reverb > 0.0;
    self.echo_on = echo > 0.0;
    self.distortion_on = distortion > 0.0;
    self.reverb_gain.target = reverb / 100.0;
    self.echo_gain.target = echo / 100.0;
    if distortion != self.distortion_amount {
      self.distortion_amount = distortion;
      self.distortion.set_curve(distortion_curve(distortion));
    }
    self.distortion_gain.target = distortion / 100.0 * 0.75;
    self.dry.target = 1.0 - distortion / 100.0 * 0.4;
    self.master_base = if settings.volume.is_finite() {
      settings.volume
    } else {
      1.0
    };
    self.apply_master_target();
  }

  pub fn set_hq(&mut self, enabled: bool, settings: &HqSettings) {
    self.hq_enabled = enabled;
    self.hq.set_settings(*settings);
  }

  /// Volume utilisateur : `clamp(volume, 0, 1)²` à l'entrée du graphe, comme l'élément audio.
  pub fn set_volume(&mut self, volume: f32, muted: bool) {
    let raw = if muted {
      0.0
    } else if volume.is_finite() {
      volume.clamp(0.0, 1.0)
    } else {
      0.85
    };
    self.element.target = raw * raw;
    self.muted = muted;
    self.apply_master_target();
  }

  fn apply_master_target(&mut self) {
    let target = if self.muted { 0.0 } else { self.master_base };
    match &mut self.fade {
      None => self.master.target = target,
      Some(fade) if fade.end > FADE_FLOOR => fade.end = target.max(FADE_FLOOR),
      Some(_) => {}
    }
  }

  pub fn set_normalize_gain_db(&mut self, db: f32) {
    self.norm.target = 10f32.powf(db / 20.0);
  }

  pub fn fade_out(&mut self, seconds: f32) {
    let start = self.master.current.max(FADE_FLOOR);
    self.start_fade(start, FADE_FLOOR, seconds);
  }

  pub fn fade_in(&mut self, seconds: f32) {
    let base = if self.master_base == 0.0 {
      1.0
    } else {
      self.master_base
    };
    let target = if self.muted { 0.0 } else { base };
    self.start_fade(FADE_FLOOR, target.max(FADE_FLOOR), seconds);
  }

  fn start_fade(&mut self, start: f32, end: f32, seconds: f32) {
    let frames = ((f64::from(seconds) * self.fs).round() as usize).max(1);
    self.master.current = start;
    let ratio = (f64::from(end) / f64::from(start)).powf(1.0 / frames as f64);
    self.fade = Some(MasterFade {
      remaining: frames,
      ratio,
      end,
    });
  }

  /// Fondu de `fadeGain` à la pause : `setTargetAtTime(0, t, 0.035)`.
  pub fn begin_pause_fade(&mut self) {
    self.pause.target = 0.0;
  }

  /// `fadeGain` à 1 à la reprise.
  pub fn end_pause_fade(&mut self) {
    self.pause.target = 1.0;
    self.pause.current = 1.0;
  }

  /// `flushEffects` : vide l'écho et la réverbération.
  pub fn flush_effects(&mut self) {
    self.echo.reset();
    self.reverb.reset();
  }

  pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
    for (l, r) in left.chunks_mut(QUANTUM).zip(right.chunks_mut(QUANTUM)) {
      self.process_quantum(l, r);
    }
  }

  fn process_quantum(&mut self, left: &mut [f32], right: &mut [f32]) {
    let frames = left.len();
    self.element.step_to_target([&mut *left, &mut *right]);
    if self.hq_enabled {
      self.hq.process(left, right);
    }
    let level_tap = self.level_tap.as_deref();
    self.eq.process(left, right, |l, r| {
      if let Some(ring) = level_tap {
        ring.push_stereo(l, r);
      }
    });
    self.mix_effects(left, right);
    self.step_master(frames, left, right);
    if self.output_stage {
      let coefficient = if frames == QUANTUM {
        self.norm_coefficient
      } else {
        exponential_coefficient(frames, NORM_TIME_CONSTANT, self.fs)
      };
      self
        .norm
        .step_exponential(coefficient, [&mut *left, &mut *right]);
      self.limiter.process(left, right);
    }
    if let Some(ring) = &self.analyser_tap {
      ring.push_stereo(left, right);
    }
    if self.pause.current != self.pause.target || self.pause.current != 1.0 {
      let coefficient = if frames == QUANTUM {
        self.pause_coefficient
      } else {
        exponential_coefficient(frames, PAUSE_TIME_CONSTANT, self.fs)
      };
      self
        .pause
        .step_exponential(coefficient, [&mut *left, &mut *right]);
    }
  }

  fn mix_effects(&mut self, left: &mut [f32], right: &mut [f32]) {
    let frames = left.len();
    let any_effect = self.distortion_on || self.echo_on || self.reverb_on;
    if !any_effect {
      self.dry.step_to_target([left, right]);
      return;
    }
    let [bus_left, bus_right] = &mut self.bus;
    let (bus_left, bus_right) = (&mut bus_left[..frames], &mut bus_right[..frames]);
    bus_left.copy_from_slice(left);
    bus_right.copy_from_slice(right);
    self.dry.step_to_target([&mut *left, &mut *right]);

    let [wet_left, wet_right] = &mut self.wet;
    let (wet_left, wet_right) = (&mut wet_left[..frames], &mut wet_right[..frames]);
    if self.distortion_on {
      wet_left.copy_from_slice(bus_left);
      wet_right.copy_from_slice(bus_right);
      self.distortion.process(wet_left, wet_right);
      self
        .distortion_gain
        .step_to_target([&mut *wet_left, &mut *wet_right]);
      add_into(left, wet_left);
      add_into(right, wet_right);
    }
    if self.echo_on {
      self.echo.process(bus_left, bus_right, wet_left, wet_right);
      self
        .echo_gain
        .step_to_target([&mut *wet_left, &mut *wet_right]);
      add_into(left, wet_left);
      add_into(right, wet_right);
    }
    if self.reverb_on {
      self
        .reverb
        .process(bus_left, bus_right, wet_left, wet_right);
      self
        .reverb_gain
        .step_to_target([&mut *wet_left, &mut *wet_right]);
      add_into(left, wet_left);
      add_into(right, wet_right);
    }
  }

  fn step_master(&mut self, frames: usize, left: &mut [f32], right: &mut [f32]) {
    let Some(fade) = &mut self.fade else {
      self.master.step_to_target([left, right]);
      return;
    };
    let steps = frames.min(fade.remaining);
    let next = (f64::from(self.master.current) * fade.ratio.powi(steps as i32)) as f32;
    fade.remaining -= steps;
    let finished = fade.remaining == 0;
    let end = fade.end;
    let next = if finished { end } else { next };
    self.master.apply_to(next, [left, right]);
    if finished {
      self.fade = None;
      self.master.target = end;
    }
  }
}

#[inline]
fn add_into(target: &mut [f32], source: &[f32]) {
  for (t, s) in target.iter_mut().zip(source) {
    *t += *s;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::dsp::reverb::{impulse_response, normalization_scale};
  use std::f64::consts::TAU;

  const FS: f64 = 48_000.0;

  fn chain(output_stage: bool) -> Chain {
    Chain::new(ChainConfig {
      sample_rate: FS,
      hq_oversample: Oversample::X2,
      output_stage,
    })
  }

  fn sine(freq: f64, amplitude: f64, frames: usize) -> Vec<f32> {
    (0..frames)
      .map(|i| (amplitude * (TAU * freq * i as f64 / FS).sin()) as f32)
      .collect()
  }

  fn run(chain: &mut Chain, input: &[f32]) -> Vec<f32> {
    let mut l = input.to_vec();
    let mut r = input.to_vec();
    chain.process(&mut l, &mut r);
    l
  }

  fn peak(signal: &[f32]) -> f32 {
    signal.iter().fold(0f32, |m, v| m.max(v.abs()))
  }

  #[test]
  fn neutral_chain_without_output_stage_is_transparent() {
    let mut c = chain(false);
    let input = sine(440.0, 0.5, 4800);
    assert_eq!(run(&mut c, &input), input);
  }

  #[test]
  fn neutral_chain_applies_limiter_makeup_and_pre_delay() {
    let mut c = chain(true);
    run(&mut c, &vec![0.0; 48_000]);
    let input = sine(1000.0, 0.1, 48_000);
    let out = run(&mut c, &input);
    let makeup = 10f32.powf(1.14 / 20.0);
    for i in 24_000..24_100 {
      assert!((out[i] - input[i - 288] * makeup).abs() < 1e-3, "i={i}");
    }
  }

  #[test]
  fn block_size_does_not_change_the_output() {
    let settings = AudioSettings {
      reverb: 30.0,
      echo: 15.0,
      distortion: 40.0,
      ..AudioSettings::default()
    };
    let input = sine(440.0, 0.3, 9600);
    let mut reference = chain(true);
    reference.set_settings(&settings);
    let want = run(&mut reference, &input);
    let mut c = chain(true);
    c.set_settings(&settings);
    let (mut l, mut r) = (input.clone(), input.clone());
    for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
      c.process(cl, cr);
    }
    assert_eq!(l, want);
  }

  #[test]
  fn volume_is_squared_at_the_input() {
    let mut c = chain(false);
    c.set_volume(0.5, false);
    let out = run(&mut c, &vec![1.0; 2048]);
    assert!((out[1024] - 0.25).abs() < 1e-6);
  }

  #[test]
  fn mute_silences_the_output() {
    let mut c = chain(true);
    c.set_volume(1.0, true);
    let out = run(&mut c, &sine(440.0, 0.5, 4800));
    assert!(peak(&out[1024..]) == 0.0);
  }

  #[test]
  fn master_volume_setting_scales_the_output() {
    let mut c = chain(false);
    c.set_settings(&AudioSettings {
      volume: 1.25,
      ..AudioSettings::default()
    });
    let out = run(&mut c, &vec![0.5; 2048]);
    assert!((out[1024] - 0.625).abs() < 1e-6);
  }

  #[test]
  fn echo_adds_a_delayed_copy_scaled_by_its_gain() {
    let mut c = chain(false);
    c.set_settings(&AudioSettings {
      echo: 50.0,
      ..AudioSettings::default()
    });
    run(&mut c, &vec![0.0; 256]);
    let mut input = vec![0.0; 48_000];
    input[0] = 1.0;
    let out = run(&mut c, &input);
    assert!((out[0] - 1.0).abs() < 1e-6);
    assert!((out[14_400] - 0.5).abs() < 1e-5);
    assert!((out[28_928] - 0.5 * 0.35).abs() < 1e-5);
  }

  #[test]
  fn reverb_adds_the_normalized_response_scaled_by_its_gain() {
    let mut c = chain(false);
    c.set_settings(&AudioSettings {
      reverb: 50.0,
      ..AudioSettings::default()
    });
    run(&mut c, &vec![0.0; 256]);
    let mut input = vec![0.0; 4096];
    input[0] = 1.0;
    let out = run(&mut c, &input);
    let ir = impulse_response(FS);
    let scale = normalization_scale(&ir, FS);
    for i in 1..4096 {
      let want = ir[0][i] * scale * 0.5;
      assert!((out[i] - want).abs() < 1e-5, "i={i}");
    }
  }

  #[test]
  fn distortion_mixes_dry_and_shaped_branches() {
    let mut c = chain(false);
    c.set_settings(&AudioSettings {
      distortion: 50.0,
      ..AudioSettings::default()
    });
    run(&mut c, &vec![0.0; 256]);
    let curve = waveshaper::distortion_curve(50.0);
    let out = run(&mut c, &vec![0.5; 1024]);
    let want = 0.5 * (1.0 - 0.5 * 0.4) + waveshaper::shape(0.5, &curve) * 0.5 * 0.75;
    assert!(
      (out[512] - want).abs() < 1e-5,
      "{} au lieu de {want}",
      out[512]
    );
  }

  #[test]
  fn disabled_effects_keep_their_tail_until_flushed() {
    let mut c = chain(false);
    c.set_settings(&AudioSettings {
      echo: 100.0,
      ..AudioSettings::default()
    });
    let mut input = vec![0.0; 1024];
    input[0] = 1.0;
    run(&mut c, &input);
    c.flush_effects();
    let out = run(&mut c, &vec![0.0; 48_000]);
    assert!(peak(&out) < 1e-6);
  }

  #[test]
  fn normalize_gain_follows_the_time_constant() {
    let mut c = chain(true);
    run(&mut c, &vec![0.0; 48_000]);
    c.set_normalize_gain_db(6.0);
    run(&mut c, &vec![0.0; (0.8 * FS) as usize]);
    let target = 10f32.powf(6.0 / 20.0);
    let want = 1.0 + (target - 1.0) * (1.0 - (-1f32).exp());
    assert!(
      (c.norm.current - want).abs() < 0.01,
      "{} au lieu de {want}",
      c.norm.current
    );
  }

  #[test]
  fn fade_out_reaches_the_floor_exponentially() {
    let mut c = chain(false);
    run(&mut c, &vec![0.0; 256]);
    c.fade_out(0.25);
    let out = run(&mut c, &vec![1.0; 24_000]);
    let quarter = out[(0.125 * FS) as usize];
    let want = (1f32 * 0.001).sqrt();
    assert!(
      (quarter / want - 1.0).abs() < 0.05,
      "{quarter} au lieu de {want}"
    );
    assert!((out[23_999] - 0.001).abs() < 1e-4);
  }

  #[test]
  fn fade_in_starts_at_the_floor_and_reaches_the_target() {
    let mut c = chain(false);
    c.set_settings(&AudioSettings {
      volume: 0.5,
      ..AudioSettings::default()
    });
    run(&mut c, &vec![0.0; 256]);
    c.fade_in(0.35);
    let out = run(&mut c, &vec![1.0; 24_000]);
    assert!(out[0] < 0.01);
    let middle = out[(0.175 * FS) as usize];
    let want = (0.001f32 * 0.5).sqrt();
    assert!(
      (middle / want - 1.0).abs() < 0.05,
      "{middle} au lieu de {want}"
    );
    assert!((out[23_999] - 0.5).abs() < 1e-4);
  }

  #[test]
  fn pause_fade_follows_its_time_constant_and_resume_is_immediate() {
    let mut c = chain(false);
    run(&mut c, &vec![0.0; 256]);
    c.begin_pause_fade();
    let out = run(&mut c, &vec![1.0; 9600]);
    let want = (-1f32).exp();
    let at_tau = out[(0.035 * FS) as usize];
    assert!((at_tau - want).abs() < 0.02, "{at_tau} au lieu de {want}");
    assert!(out[8159] < 0.01, "silencieux après 170 ms : {}", out[8159]);
    c.end_pause_fade();
    let out = run(&mut c, &vec![1.0; 256]);
    assert!((out[0] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn level_tap_sees_audio_with_or_without_the_equalizer() {
    let ring = Arc::new(SampleRing::new(4096));
    let mut c = chain(false);
    c.set_taps(None, Some(ring.clone()));
    run(&mut c, &vec![0.5; 1024]);
    assert_eq!(
      ring.written(),
      1024,
      "égaliseur contourné : niveau mesuré, écart volontaire avec Riff"
    );
    let gains = crate::dsp::eq::EqGains {
      low: 3.0,
      ..Default::default()
    };
    c.set_settings(&AudioSettings {
      eq: gains,
      ..AudioSettings::default()
    });
    run(&mut c, &vec![0.5; 1024]);
    assert_eq!(ring.written(), 2048);
  }

  #[test]
  fn analyser_tap_sees_the_signal_after_the_limiter_and_before_the_pause_fade() {
    let ring = Arc::new(SampleRing::new(4096));
    let mut c = chain(true);
    c.set_taps(Some(ring.clone()), None);
    run(&mut c, &vec![0.0; 48_000]);
    let input = sine(1000.0, 0.1, 9600);
    c.begin_pause_fade();
    let out = run(&mut c, &input);
    let mut seen = vec![0.0; 2048];
    ring.read_latest(&mut seen);
    assert!(peak(&out[out.len() - 1024..]) < 1e-3);
    let makeup = 10f32.powf(1.14 / 20.0);
    assert!((peak(&seen) - 0.1 * makeup).abs() < 2e-3, "{}", peak(&seen));
  }

  #[test]
  fn hq_routing_replaces_nothing_but_precedes_the_equalizer() {
    let mut c = chain(false);
    c.set_hq(true, &HqSettings::default());
    let out = run(&mut c, &sine(1000.0, 0.01, 9600));
    assert!(peak(&out) > 0.005);
    c.set_hq(false, &HqSettings::default());
    c.end_pause_fade();
    let input = sine(1000.0, 0.01, 4800);
    assert_eq!(run(&mut c, &input), input);
  }
}
