//! Mode HQ : compresseur, trois filtres, saturation et limiteur (`_applyHqParams` de `audio.js`).

use super::biquad::{Biquad, BiquadCoeffs};
use super::dynamics::{Compressor, CompressorParams};
use super::waveshaper::{Oversample, WaveShaper, harmonic_curve};

const BASS_FREQ: f64 = 80.0;
const VOCAL_FREQ: f64 = 2500.0;
const VOCAL_Q: f64 = 1.2;
const AIR_FREQ: f64 = 9000.0;
const MAX_GAIN_DB: f64 = 4.0;
const SETTING_SCALE: f64 = 0.08;

const COMPRESSOR: CompressorParams = CompressorParams {
  threshold: -12.0,
  knee: 10.0,
  ratio: 1.5,
  attack: 0.01,
  release: 0.1,
};
const LIMITER: CompressorParams = CompressorParams {
  threshold: -1.0,
  knee: 0.0,
  ratio: 20.0,
  attack: 0.005,
  release: 0.05,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HqEngine {
  #[default]
  HqMusic3,
  Glyphs7B,
  Fyro11,
}

/// Réglages HQ : moteur et décalages `bass`, `vocal`, `air` (0 par défaut).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HqSettings {
  pub engine: HqEngine,
  pub bass: f32,
  pub vocal: f32,
  pub air: f32,
}

/// Gains des filtres, en dB : `(bass, vocal, air)`.
pub fn filter_gains(settings: &HqSettings) -> (f64, f64, f64) {
  let (base_bass, base_air) = match settings.engine {
    HqEngine::HqMusic3 => (0.0, 0.0),
    HqEngine::Glyphs7B => (0.0, 2.5),
    HqEngine::Fyro11 => (3.0, 0.0),
  };
  let gain = |base: f64, setting: f32| {
    (base + f64::from(setting) * SETTING_SCALE).clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
  };
  (
    gain(base_bass, settings.bass),
    gain(0.0, settings.vocal),
    gain(base_air, settings.air),
  )
}

fn saturation_amount(engine: HqEngine) -> f32 {
  match engine {
    HqEngine::HqMusic3 => 0.5,
    HqEngine::Glyphs7B | HqEngine::Fyro11 => 0.2,
  }
}

fn compressor_params(engine: HqEngine) -> CompressorParams {
  match engine {
    HqEngine::Fyro11 => CompressorParams {
      ratio: 2.0,
      ..COMPRESSOR
    },
    HqEngine::HqMusic3 | HqEngine::Glyphs7B => COMPRESSOR,
  }
}

pub struct HqChain {
  fs: f64,
  compressor: Compressor,
  bass: Biquad,
  vocal: Biquad,
  air: Biquad,
  saturation: WaveShaper,
  limiter: Compressor,
}

impl HqChain {
  pub fn new(fs: f64, oversample: Oversample, settings: HqSettings) -> Self {
    let curve = harmonic_curve(saturation_amount(settings.engine));
    let mut chain = Self {
      fs,
      compressor: Compressor::new(fs as f32, compressor_params(settings.engine)),
      bass: Biquad::default(),
      vocal: Biquad::default(),
      air: Biquad::default(),
      saturation: WaveShaper::new(curve, oversample),
      limiter: Compressor::new(fs as f32, LIMITER),
    };
    chain.set_settings(settings);
    chain
  }

  pub fn set_settings(&mut self, settings: HqSettings) {
    let (bass, vocal, air) = filter_gains(&settings);
    self
      .compressor
      .set_params(compressor_params(settings.engine));
    self
      .bass
      .set_coeffs(BiquadCoeffs::low_shelf(self.fs, BASS_FREQ, bass));
    self
      .vocal
      .set_coeffs(BiquadCoeffs::peaking(self.fs, VOCAL_FREQ, VOCAL_Q, vocal));
    self
      .air
      .set_coeffs(BiquadCoeffs::high_shelf(self.fs, AIR_FREQ, air));
    self
      .saturation
      .set_curve(harmonic_curve(saturation_amount(settings.engine)));
  }

  pub fn reset(&mut self) {
    self.compressor.reset();
    self.bass.reset();
    self.vocal.reset();
    self.air.reset();
    self.limiter.reset();
  }

  pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
    self.compressor.process(left, right);
    self.bass.process(left, right);
    self.vocal.process(left, right);
    self.air.process(left, right);
    self.saturation.process(left, right);
    self.limiter.process(left, right);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f64::consts::TAU;

  const FS: f64 = 48_000.0;

  fn settings(engine: HqEngine) -> HqSettings {
    HqSettings {
      engine,
      ..HqSettings::default()
    }
  }

  #[test]
  fn engine_bases_follow_the_original() {
    assert_eq!(filter_gains(&settings(HqEngine::HqMusic3)), (0.0, 0.0, 0.0));
    assert_eq!(filter_gains(&settings(HqEngine::Glyphs7B)), (0.0, 0.0, 2.5));
    assert_eq!(filter_gains(&settings(HqEngine::Fyro11)), (3.0, 0.0, 0.0));
  }

  #[test]
  fn filter_gains_are_bounded_to_four_db() {
    let high = HqSettings {
      engine: HqEngine::Fyro11,
      bass: 100.0,
      vocal: 100.0,
      air: 100.0,
    };
    assert_eq!(filter_gains(&high), (4.0, 4.0, 4.0));
    let low = HqSettings {
      engine: HqEngine::Glyphs7B,
      bass: -100.0,
      vocal: -100.0,
      air: -100.0,
    };
    assert_eq!(filter_gains(&low), (-4.0, -4.0, -4.0));
    let (_, vocal, _) = filter_gains(&HqSettings {
      vocal: 10.0,
      ..HqSettings::default()
    });
    assert!((vocal - 0.8).abs() < 1e-6, "vocal={vocal}");
  }

  fn rms_gain_db(chain: &mut HqChain, freq: f64) -> f64 {
    let amplitude = 0.003f64;
    let frames = 48_000;
    let mut l: Vec<f32> = (0..frames)
      .map(|i| (amplitude * (TAU * freq * i as f64 / FS).sin()) as f32)
      .collect();
    let mut r = l.clone();
    for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
      chain.process(cl, cr);
    }
    let body = &l[24_000..];
    // Retire la composante continue produite par la courbe (décalage de Web Audio).
    let mean = body.iter().map(|v| f64::from(*v)).sum::<f64>() / body.len() as f64;
    let energy: f64 = body.iter().map(|v| (f64::from(*v) - mean).powi(2)).sum();
    let rms = (energy / body.len() as f64).sqrt();
    20.0 * (rms * std::f64::consts::SQRT_2 / amplitude).log10()
  }

  #[test]
  fn engines_apply_their_own_filter_gain() {
    let mut neutral = HqChain::new(FS, Oversample::X2, settings(HqEngine::HqMusic3));
    let reference_high = rms_gain_db(&mut neutral, 16_000.0);
    neutral.reset();
    let reference_low = rms_gain_db(&mut neutral, 30.0);

    let mut glyphs = HqChain::new(FS, Oversample::X2, settings(HqEngine::Glyphs7B));
    let want = BiquadCoeffs::high_shelf(FS, AIR_FREQ, 2.5).magnitude_db(FS, 16_000.0);
    let got = rms_gain_db(&mut glyphs, 16_000.0) - reference_high;
    assert!((got - want).abs() < 0.5, "glyphs {got} au lieu de {want}");

    // Le ratio 2,0 de fyro1.1 change aussi la compensation du compresseur.
    let low_ratio = Compressor::new(FS as f32, COMPRESSOR).static_output(0.001);
    let high_ratio = Compressor::new(
      FS as f32,
      CompressorParams {
        ratio: 2.0,
        ..COMPRESSOR
      },
    )
    .static_output(0.001);
    let makeup = 20.0 * f64::from(high_ratio / low_ratio).log10();
    let mut fyro = HqChain::new(FS, Oversample::X2, settings(HqEngine::Fyro11));
    let want = BiquadCoeffs::low_shelf(FS, BASS_FREQ, 3.0).magnitude_db(FS, 30.0) + makeup;
    let got = rms_gain_db(&mut fyro, 30.0) - reference_low;
    assert!((got - want).abs() < 0.5, "fyro {got} au lieu de {want}");
  }

  #[test]
  fn chain_stays_finite_and_limited_at_full_scale() {
    for engine in [HqEngine::HqMusic3, HqEngine::Glyphs7B, HqEngine::Fyro11] {
      for oversample in [Oversample::X2, Oversample::X4] {
        let mut chain = HqChain::new(FS, oversample, settings(engine));
        let mut l: Vec<f32> = (0..48_000)
          .map(|i| (TAU * 1000.0 * f64::from(i) / FS).sin() as f32)
          .collect();
        let mut r = l.clone();
        for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
          chain.process(cl, cr);
        }
        assert!(l.iter().all(|v| v.is_finite()));
        let peak = l[24_000..].iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(
          peak > 0.5 && peak < 1.1,
          "{engine:?} {oversample:?} pic={peak}"
        );
      }
    }
  }

  #[test]
  fn switching_engine_updates_the_chain_without_reallocating_state() {
    let mut chain = HqChain::new(FS, Oversample::X2, settings(HqEngine::HqMusic3));
    let mut before = HqChain::new(FS, Oversample::X2, settings(HqEngine::Glyphs7B));
    chain.set_settings(settings(HqEngine::Glyphs7B));
    assert!((rms_gain_db(&mut chain, 16_000.0) - rms_gain_db(&mut before, 16_000.0)).abs() < 0.01);
  }
}
