//! Étage de vitesse : étirement sans changement de hauteur et varispeed (fil de décodage).

use std::num::{NonZeroU16, NonZeroU32};

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
  Adjustable, Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters,
  SincInterpolationType, WindowFunction,
};
use wsola::{Config, TimeStretch};

use crate::AudioError;

const CHANNELS: usize = 2;
const CHUNK_FRAMES: usize = 1024;
const SINC_LEN: usize = 128;
const OVERSAMPLING: usize = 256;
const MAX_RELATIVE_RATIO: f64 = 8.0;
/// Fenêtre OLA de 20 ms et recherche de 30 ms au total : valeurs de `AudioRendererAlgorithm`.
const WSOLA_CONFIG: Config = Config {
  hop_ms: 10.0,
  search_ms: 15.0,
};
const PULL_MULTIPLE: usize = 8;

/// Réglages de vitesse de Riff (`applySettings` de `audio.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoParams {
  pub speed: f32,
  pub speed_pitch: f32,
  pub pitch: f32,
}

impl Default for TempoParams {
  fn default() -> Self {
    Self {
      speed: 1.0,
      speed_pitch: 1.0,
      pitch: 0.0,
    }
  }
}

/// Facteurs appliqués : étirement et varispeed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoFactors {
  pub stretch: f64,
  pub varispeed: f64,
}

impl TempoParams {
  pub fn factors(&self) -> TempoFactors {
    if self.speed_pitch != 1.0 || self.pitch != 0.0 {
      let varispeed = f64::from(self.speed_pitch) * 2f64.powf(f64::from(self.pitch) / 12.0);
      TempoFactors {
        stretch: 1.0,
        varispeed,
      }
    } else {
      TempoFactors {
        stretch: f64::from(self.speed),
        varispeed: 1.0,
      }
    }
  }

  /// Facteur de vitesse global (`speedFactor` de `renderOffline`).
  pub fn speed_factor(&self) -> f64 {
    let factors = self.factors();
    factors.stretch * factors.varispeed
  }
}

fn resample_error(err: impl std::fmt::Display) -> AudioError {
  AudioError::Resample(err.to_string())
}

/// Convertit un flux stéréo entrelacé vers la fréquence de sortie, à la vitesse demandée.
pub struct TempoStage {
  base_ratio: f64,
  varispeed: f64,
  stretch_factor: f64,
  stretcher: TimeStretch,
  stretched: Vec<f32>,
  pull_buffer: Vec<f32>,
  resampler: Async<f32>,
  resampling: bool,
  pending: Vec<f32>,
  resampled: Vec<f32>,
  skip_frames: usize,
  expected_frames: f64,
  produced_frames: u64,
}

impl TempoStage {
  /// # Errors
  /// Fréquences invalides, refusées par `wsola` ou `rubato`.
  pub fn new(source_rate: u32, output_rate: u32) -> Result<Self, AudioError> {
    let rate = NonZeroU32::new(source_rate).ok_or_else(|| resample_error("fréquence nulle"))?;
    let channels = NonZeroU16::new(2).ok_or_else(|| resample_error("canaux nuls"))?;
    let stretcher =
      TimeStretch::with_config(rate, channels, WSOLA_CONFIG).map_err(resample_error)?;
    let base_ratio = f64::from(output_rate) / f64::from(source_rate);
    let params = SincInterpolationParameters::new(SINC_LEN, WindowFunction::Blackman2)
      .oversampling_factor(OVERSAMPLING)
      .interpolation(SincInterpolationType::Linear);
    let resampler = Async::<f32>::new_sinc(
      base_ratio,
      MAX_RELATIVE_RATIO,
      &params,
      CHUNK_FRAMES,
      CHANNELS,
      FixedAsync::Input,
    )
    .map_err(resample_error)?;
    let pull_buffer = vec![0.0; stretcher.min_output_buff_size() * PULL_MULTIPLE];
    let resampled = vec![0.0; resampler.output_frames_max() * CHANNELS];
    let mut stage = Self {
      base_ratio,
      varispeed: 1.0,
      stretch_factor: 1.0,
      stretcher,
      stretched: Vec::new(),
      pull_buffer,
      resampler,
      resampling: false,
      pending: Vec::new(),
      resampled,
      skip_frames: 0,
      expected_frames: 0.0,
      produced_frames: 0,
    };
    stage.update_resampling(false)?;
    Ok(stage)
  }

  /// # Errors
  /// Rapport hors des bornes du rééchantillonneur.
  pub fn set_params(&mut self, params: TempoParams) -> Result<(), AudioError> {
    let factors = params.factors();
    let stretch = factors
      .stretch
      .clamp(f64::from(wsola::MIN_TEMPO), f64::from(wsola::MAX_TEMPO));
    if stretch != self.stretch_factor {
      if self.stretch_factor == 1.0 {
        self.stretcher.reset();
      }
      self.stretch_factor = stretch;
      self.stretcher.set_tempo(stretch as f32);
    }
    let bound = MAX_RELATIVE_RATIO * 0.999;
    let varispeed = factors.varispeed.clamp(1.0 / bound, bound);
    if varispeed != self.varispeed {
      self.varispeed = varispeed;
      self.update_resampling(true)?;
    }
    Ok(())
  }

  fn current_ratio(&self) -> f64 {
    self.base_ratio / self.varispeed
  }

  fn update_resampling(&mut self, ramp: bool) -> Result<(), AudioError> {
    let needed = self.current_ratio() != 1.0;
    if needed && !self.resampling {
      self.restart_resampler();
      self
        .resampler
        .set_resample_ratio(self.current_ratio(), false)
        .map_err(resample_error)?;
    } else if needed {
      self
        .resampler
        .set_resample_ratio(self.current_ratio(), ramp)
        .map_err(resample_error)?;
    }
    self.resampling = needed;
    Ok(())
  }

  fn restart_resampler(&mut self) {
    self.resampler.reset();
    self.skip_frames = self.resampler.output_delay();
    self.expected_frames = 0.0;
    self.produced_frames = 0;
  }

  /// Ajoute `input` et écrit à la suite de `output` ce qui est disponible.
  ///
  /// # Errors
  /// Échec du rééchantillonneur.
  pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) -> Result<(), AudioError> {
    if self.stretch_factor == 1.0 {
      return self.resample(input, output);
    }
    self.stretcher.push(input);
    self.flush_stretched(output)
  }

  fn flush_stretched(&mut self, output: &mut Vec<f32>) -> Result<(), AudioError> {
    self.drain_stretcher()?;
    let stretched = std::mem::take(&mut self.stretched);
    let result = self.resample(&stretched, output);
    self.stretched = stretched;
    self.stretched.clear();
    result
  }

  fn drain_stretcher(&mut self) -> Result<(), AudioError> {
    loop {
      match self.stretcher.pull(&mut self.pull_buffer) {
        Ok(0) | Err(wsola::Error::NotEnoughInput) => return Ok(()),
        Ok(written) => self
          .stretched
          .extend_from_slice(&self.pull_buffer[..written]),
        Err(err) => return Err(resample_error(err)),
      }
    }
  }

  fn resample(&mut self, input: &[f32], output: &mut Vec<f32>) -> Result<(), AudioError> {
    if !self.resampling {
      output.extend_from_slice(input);
      return Ok(());
    }
    self.pending.extend_from_slice(input);
    let mut offset = 0;
    while self.pending.len() / CHANNELS - offset >= self.resampler.input_frames_next() {
      offset += self.resample_chunk(offset, None, output)?;
    }
    self.pending.drain(..offset * CHANNELS);
    Ok(())
  }

  fn resample_chunk(
    &mut self,
    offset: usize,
    partial: Option<usize>,
    output: &mut Vec<f32>,
  ) -> Result<usize, AudioError> {
    let frames = self.pending.len() / CHANNELS;
    let source = InterleavedSlice::new(&self.pending, CHANNELS, frames).map_err(resample_error)?;
    let capacity = self.resampled.len() / CHANNELS;
    let mut target =
      InterleavedSlice::new_mut(&mut self.resampled, CHANNELS, capacity).map_err(resample_error)?;
    let mut indexing = Indexing::new();
    indexing.input_offset = offset;
    indexing.partial_len = partial;
    let (consumed, written) = self
      .resampler
      .process_into_buffer(&source, &mut target, Some(&indexing))
      .map_err(resample_error)?;
    let real_input = partial.unwrap_or(consumed);
    self.expected_frames += real_input as f64 * self.base_ratio / self.varispeed;
    let skipped = self.skip_frames.min(written);
    self.skip_frames -= skipped;
    output.extend_from_slice(&self.resampled[skipped * CHANNELS..written * CHANNELS]);
    self.produced_frames += (written - skipped) as u64;
    Ok(real_input)
  }

  /// Vide les tampons internes en fin de flux.
  ///
  /// # Errors
  /// Échec du rééchantillonneur.
  pub fn finish(&mut self, output: &mut Vec<f32>) -> Result<(), AudioError> {
    if self.stretch_factor != 1.0 {
      self.stretcher.set_draining();
      self.flush_stretched(output)?;
    }
    if !self.resampling {
      return Ok(());
    }
    let remaining = self.pending.len() / CHANNELS;
    if remaining > 0 {
      self.resample_chunk(0, Some(remaining), output)?;
      self.pending.clear();
    }
    // Pousse du silence pour extraire ce que le filtre retient encore, puis coupe à la durée exacte.
    let wanted = self.expected_frames.round() as u64;
    let start = output.len();
    while self.produced_frames < wanted {
      self.resample_chunk(0, Some(0), output)?;
    }
    let written = (output.len() - start) / CHANNELS;
    let excess = usize::try_from(self.produced_frames - wanted).unwrap_or(written);
    output.truncate(start + written.saturating_sub(excess) * CHANNELS);
    self.restart_resampler();
    Ok(())
  }

  pub fn reset(&mut self) {
    self.stretcher.reset();
    self.stretched.clear();
    self.pending.clear();
    self.restart_resampler();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f64::consts::TAU;

  const RATE: u32 = 48_000;

  fn stereo_sine(freq: f64, seconds: f64, rate: u32) -> Vec<f32> {
    let frames = (seconds * f64::from(rate)) as usize;
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
      let v = ((TAU * freq * i as f64 / f64::from(rate)).sin() * 0.5) as f32;
      out.extend_from_slice(&[v, v]);
    }
    out
  }

  fn left(interleaved: &[f32]) -> Vec<f32> {
    interleaved
      .as_chunks::<2>()
      .0
      .iter()
      .map(|f| f[0])
      .collect()
  }

  fn dominant_freq(signal: &[f32], rate: u32) -> f64 {
    // Passages à zéro montants, sur la partie centrale pour ignorer les bords.
    let mid = &signal[signal.len() / 4..signal.len() * 3 / 4];
    let crossings: Vec<usize> = mid
      .windows(2)
      .enumerate()
      .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
      .map(|(i, _)| i)
      .collect();
    let span = (crossings[crossings.len() - 1] - crossings[0]) as f64;
    (crossings.len() - 1) as f64 * f64::from(rate) / span
  }

  fn run(stage: &mut TempoStage, input: &[f32]) -> Vec<f32> {
    let mut out = Vec::new();
    for block in input.chunks(2048) {
      stage.process(block, &mut out).unwrap();
    }
    stage.finish(&mut out).unwrap();
    out
  }

  #[test]
  fn factors_follow_riff_rules() {
    let speed = TempoParams {
      speed: 1.35,
      ..TempoParams::default()
    };
    assert_eq!(
      speed.factors(),
      TempoFactors {
        stretch: 1.35f32 as f64,
        varispeed: 1.0
      }
    );

    let vari = TempoParams {
      speed: 1.35,
      speed_pitch: 0.82,
      pitch: 0.0,
    };
    let f = vari.factors();
    assert_eq!(f.stretch, 1.0, "speedPitch ignore speed");
    assert!((f.varispeed - 0.82).abs() < 1e-6);

    let pitch = TempoParams {
      speed: 1.0,
      speed_pitch: 1.0,
      pitch: 12.0,
    };
    assert!((pitch.factors().varispeed - 2.0).abs() < 1e-9);
  }

  #[test]
  fn neutral_settings_pass_samples_unchanged() {
    let mut stage = TempoStage::new(RATE, RATE).unwrap();
    let input = stereo_sine(440.0, 0.5, RATE);
    assert_eq!(run(&mut stage, &input), input);
  }

  #[test]
  fn resampling_keeps_frequency_and_duration() {
    let mut stage = TempoStage::new(44_100, RATE).unwrap();
    let out = run(&mut stage, &stereo_sine(1000.0, 2.0, 44_100));
    let got = dominant_freq(&left(&out), RATE);
    assert!((got - 1000.0).abs() < 1.0, "freq={got}");
    let seconds = out.len() as f64 / 2.0 / f64::from(RATE);
    assert!((seconds - 2.0).abs() < 0.02, "durée={seconds}");
  }

  #[test]
  fn varispeed_changes_pitch_and_duration_together() {
    for factor in [0.82f32, 1.3] {
      let mut stage = TempoStage::new(RATE, RATE).unwrap();
      stage
        .set_params(TempoParams {
          speed_pitch: factor,
          ..TempoParams::default()
        })
        .unwrap();
      let out = run(&mut stage, &stereo_sine(1000.0, 2.0, RATE));
      let got = dominant_freq(&left(&out), RATE);
      let want = 1000.0 * f64::from(factor);
      assert!(
        (got - want).abs() < want * 0.002,
        "factor={factor} freq={got}"
      );
      let seconds = out.len() as f64 / 2.0 / f64::from(RATE);
      let expected = 2.0 / f64::from(factor);
      assert!(
        (seconds - expected).abs() < 0.02,
        "factor={factor} durée={seconds}"
      );
    }
  }

  #[test]
  fn changing_the_varispeed_ratio_midway_has_no_discontinuity() {
    let mut stage = TempoStage::new(RATE, RATE).unwrap();
    stage
      .set_params(TempoParams {
        speed_pitch: 1.1,
        ..TempoParams::default()
      })
      .unwrap();
    let input = stereo_sine(200.0, 4.0, RATE);
    let mut out = Vec::new();
    let (first, second) = input.split_at(input.len() / 2);
    stage.process(first, &mut out).unwrap();
    stage
      .set_params(TempoParams {
        speed_pitch: 0.9,
        ..TempoParams::default()
      })
      .unwrap();
    stage.process(second, &mut out).unwrap();
    stage.finish(&mut out).unwrap();
    let signal = left(&out);
    // Une sinusoïde de 200 Hz d'amplitude 0,5 varie de moins de 0,02 par échantillon.
    let start = 4096;
    let end = signal.len() - 4096;
    let jump = signal[start..end]
      .windows(2)
      .fold(0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(jump < 0.02, "saut={jump}");
  }

  #[test]
  fn stretching_changes_duration_but_not_pitch() {
    for speed in [0.8f32, 1.35] {
      let mut stage = TempoStage::new(RATE, RATE).unwrap();
      stage
        .set_params(TempoParams {
          speed,
          ..TempoParams::default()
        })
        .unwrap();
      let out = run(&mut stage, &stereo_sine(1000.0, 4.0, RATE));
      let got = dominant_freq(&left(&out), RATE);
      assert!((got - 1000.0).abs() < 10.0, "speed={speed} freq={got}");
      let seconds = out.len() as f64 / 2.0 / f64::from(RATE);
      let expected = 4.0 / f64::from(speed);
      assert!(
        (seconds - expected).abs() < expected * 0.02,
        "speed={speed} durée={seconds}"
      );
    }
  }

  #[test]
  fn stretching_also_applies_sample_rate_conversion() {
    let mut stage = TempoStage::new(44_100, RATE).unwrap();
    stage
      .set_params(TempoParams {
        speed: 1.35,
        ..TempoParams::default()
      })
      .unwrap();
    let out = run(&mut stage, &stereo_sine(1000.0, 4.0, 44_100));
    let got = dominant_freq(&left(&out), RATE);
    assert!((got - 1000.0).abs() < 10.0, "freq={got}");
  }

  #[test]
  fn reset_forgets_buffered_audio() {
    let mut stage = TempoStage::new(44_100, RATE).unwrap();
    let mut out = Vec::new();
    stage
      .process(&stereo_sine(1000.0, 0.1, 44_100), &mut out)
      .unwrap();
    stage.reset();
    out.clear();
    stage.finish(&mut out).unwrap();
    let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(peak < 1e-3, "pic={peak}");
  }
}
