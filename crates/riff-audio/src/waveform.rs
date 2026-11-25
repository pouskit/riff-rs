//! Décodage complet d'un fichier et onde affichée par la découpe. À appeler hors du fil d'interface.

use crate::AudioError;
use crate::decode::Decoder;
use crate::source::Source;

const CHANNELS: usize = 2;

/// Audio décodé : stéréo entrelacée.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
  pub samples: Vec<f32>,
  pub rate: u32,
}

impl Pcm {
  /// Durée en secondes.
  #[must_use]
  pub fn duration_secs(&self) -> f64 {
    if self.rate == 0 {
      return 0.0;
    }
    (self.samples.len() / CHANNELS) as f64 / f64::from(self.rate)
  }

  /// Extrait entre `start` et `end` secondes, bornes ramenées à la durée.
  #[must_use]
  pub fn slice(&self, start: f64, end: f64) -> &[f32] {
    let frames = self.samples.len() / CHANNELS;
    let frame =
      |seconds: f64| ((seconds.max(0.0) * f64::from(self.rate)).round() as usize).min(frames);
    let (first, last) = (frame(start), frame(end));
    if first >= last {
      return &[];
    }
    &self.samples[first * CHANNELS..last * CHANNELS]
  }
}

/// Décode toute la source.
///
/// # Errors
/// Source illisible, format inconnu, ou erreur de lecture.
pub fn decode_all(source: &Source) -> Result<Pcm, AudioError> {
  let mut decoder = Decoder::open(source)?;
  let rate = decoder.sample_rate();
  let mut samples = Vec::new();
  while decoder.next_block(&mut samples)?.is_some() {}
  Ok(Pcm { samples, rate })
}

/// Crête par colonne de l'onde, de 0 à 1.
#[must_use]
pub fn peaks(samples: &[f32], width: usize) -> Vec<f32> {
  let frames = samples.len() / CHANNELS;
  if width == 0 || frames == 0 {
    return vec![0.0; width];
  }
  (0..width)
    .map(|column| {
      let first = column * frames / width;
      let last = ((column + 1) * frames / width).max(first + 1).min(frames);
      samples[first * CHANNELS..last * CHANNELS]
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
        .min(1.0)
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::offline::encode_wav_16;

  fn stereo(values: &[f32]) -> Vec<f32> {
    values.iter().flat_map(|value| [*value, *value]).collect()
  }

  #[test]
  fn a_peak_is_found_in_its_column() {
    let peaks = peaks(&stereo(&[0.0, 0.1, 0.0, -0.8]), 2);
    assert!((peaks[0] - 0.1).abs() < 1e-6);
    assert!((peaks[1] - 0.8).abs() < 1e-6);
  }

  #[test]
  fn more_columns_than_samples_do_not_panic() {
    let peaks = peaks(&stereo(&[0.5, 0.25]), 5);
    assert_eq!(peaks.len(), 5);
    assert!(peaks.iter().all(|peak| *peak > 0.0));
  }

  #[test]
  fn nothing_to_draw_gives_silence() {
    assert_eq!(peaks(&[], 3), [0.0; 3]);
    assert!(peaks(&stereo(&[0.5]), 0).is_empty());
  }

  #[test]
  fn a_peak_above_full_scale_stays_at_one() {
    assert_eq!(peaks(&stereo(&[3.0]), 1), [1.0]);
  }

  #[test]
  fn a_wav_is_decoded_with_its_duration() {
    let rate = 8000;
    let wav = encode_wav_16(&stereo(&vec![0.5; 16000]), 2, rate);
    let pcm = decode_all(&Source::from(wav)).unwrap();
    assert_eq!(pcm.rate, rate);
    assert!((pcm.duration_secs() - 2.0).abs() < 1e-6);
    assert!(pcm.samples.iter().all(|sample| (sample - 0.5).abs() < 1e-3));
  }

  #[test]
  fn an_unreadable_source_is_an_error() {
    assert!(decode_all(&Source::from(vec![1, 2, 3])).is_err());
  }

  #[test]
  fn a_slice_stays_inside_the_audio() {
    let pcm = Pcm {
      samples: stereo(&[0.0, 1.0, 2.0, 3.0]),
      rate: 2,
    };
    assert_eq!(pcm.slice(0.5, 1.5), stereo(&[1.0, 2.0]).as_slice());
    assert_eq!(pcm.slice(-4.0, 99.0).len(), 8);
    assert!(pcm.slice(1.5, 0.5).is_empty());
    assert!(pcm.slice(99.0, 100.0).is_empty());
  }
}
