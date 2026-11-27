//! Moteur audio natif de Riff.

pub mod analyser;
pub mod decode;
pub mod dsp;
pub mod engine;
pub mod mixer;
pub mod normalize;
pub mod offline;
pub mod output;
pub mod source;
pub mod tempo;
pub mod waveform;

pub use dsp::eq::EqGains;
pub use dsp::hq::{HqEngine, HqSettings};
pub use engine::{Engine, EngineConfig, Event};
pub use output::DeviceInfo;
pub use source::{RangeFetcher, SegmentFetcher, Segmented, Source};
use tempo::TempoParams;

/// Réglages d'effets de Riff (`audioSettings` de `audio.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioSettings {
  pub speed: f32,
  pub speed_pitch: f32,
  pub pitch: f32,
  pub reverb: f32,
  pub distortion: f32,
  pub echo: f32,
  pub muffled: f32,
  pub volume: f32,
  pub eq: EqGains,
}

impl Default for AudioSettings {
  fn default() -> Self {
    Self {
      speed: 1.0,
      speed_pitch: 1.0,
      pitch: 0.0,
      reverb: 0.0,
      distortion: 0.0,
      echo: 0.0,
      muffled: 0.0,
      volume: 1.0,
      eq: EqGains::default(),
    }
  }
}

impl AudioSettings {
  pub fn tempo(&self) -> TempoParams {
    TempoParams {
      speed: self.speed,
      speed_pitch: self.speed_pitch,
      pitch: self.pitch,
    }
  }
}

/// Erreurs du moteur audio.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AudioError {
  #[error("aucun périphérique de sortie")]
  NoDevice,
  #[error("flux de sortie : {0}")]
  Stream(String),
  #[error("source : {0}")]
  Source(String),
  #[error("décodage : {0}")]
  Decode(String),
  #[error("rééchantillonnage : {0}")]
  Resample(String),
}
