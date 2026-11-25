//! Analyseur : tampon circulaire écrit par le callback, calcul à la demande dans le fil appelant.

use std::f64::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// Capacité des tampons, en échantillons.
pub const RING_CAPACITY: usize = 4096;
/// `analyserNode.fftSize` de Riff.
pub const DEFAULT_FFT_SIZE: usize = 128;
/// `analyserNode.fftSize` quand l'interface est masquée.
pub const HIDDEN_FFT_SIZE: usize = 32;
const LEVEL_FFT_SIZE: usize = 2048;
const SMOOTHING: f64 = 0.8;
const MIN_DECIBELS: f64 = -100.0;
const MAX_DECIBELS: f64 = -30.0;
const BLACKMAN_ALPHA: f64 = 0.16;
const SILENCE_DB: f32 = -120.0;

/// Tampon circulaire d'échantillons mono. Un seul écrivain, des lectures sans verrou : une lecture
/// concurrente peut mêler ancien et nouveau échantillon, sans conséquence pour un affichage.
pub struct SampleRing {
  data: Box<[AtomicU32]>,
  written: AtomicUsize,
}

impl SampleRing {
  /// `capacity` : puissance de 2.
  pub fn new(capacity: usize) -> Self {
    assert!(
      capacity.is_power_of_two(),
      "capacité du tampon de l'analyseur"
    );
    Self {
      data: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
      written: AtomicUsize::new(0),
    }
  }

  /// Nombre total d'échantillons écrits depuis la création.
  pub fn written(&self) -> usize {
    self.written.load(Ordering::Acquire)
  }

  /// Réduit en mono par `0,5·(L+R)` (règle « speakers » de Web Audio) et écrit.
  pub fn push_stereo(&self, left: &[f32], right: &[f32]) {
    let mask = self.data.len() - 1;
    let mut index = self.written.load(Ordering::Relaxed);
    for (l, r) in left.iter().zip(right) {
      self.data[index & mask].store((0.5 * (l + r)).to_bits(), Ordering::Relaxed);
      index = index.wrapping_add(1);
    }
    self.written.store(index, Ordering::Release);
  }

  /// Copie les `out.len()` derniers échantillons, du plus ancien au plus récent.
  pub fn read_latest(&self, out: &mut [f32]) {
    let mask = self.data.len() - 1;
    let end = self.written();
    let count = out.len().min(self.data.len());
    let start = end.wrapping_sub(count);
    for (i, slot) in out.iter_mut().enumerate() {
      *slot = if i < count {
        f32::from_bits(self.data[start.wrapping_add(i) & mask].load(Ordering::Relaxed))
      } else {
        0.0
      };
    }
  }

  /// Remet le tampon à zéro.
  pub fn clear(&self) {
    for slot in &self.data {
      slot.store(0, Ordering::Relaxed);
    }
  }
}

/// Analyseur des visualiseurs et mesure de niveau (lecture seule du tampon).
///
/// Formules de la section `AnalyserNode` de la spécification Web Audio, voir `NOTES.md`.
pub struct Analyser {
  ring: Arc<SampleRing>,
  level_ring: Arc<SampleRing>,
  planner: FftPlanner<f32>,
  fft: Arc<dyn Fft<f32>>,
  fft_size: usize,
  window: Vec<f32>,
  samples: Vec<f32>,
  buffer: Vec<Complex32>,
  scratch: Vec<Complex32>,
  magnitudes: Vec<f32>,
  level_samples: Vec<f32>,
  last_written: Option<usize>,
}

impl Analyser {
  pub fn new(ring: Arc<SampleRing>, level_ring: Arc<SampleRing>) -> Self {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(DEFAULT_FFT_SIZE);
    let mut analyser = Self {
      ring,
      level_ring,
      planner,
      fft,
      fft_size: 0,
      window: Vec::new(),
      samples: Vec::new(),
      buffer: Vec::new(),
      scratch: Vec::new(),
      magnitudes: Vec::new(),
      level_samples: vec![0.0; LEVEL_FFT_SIZE],
      last_written: None,
    };
    analyser.set_fft_size(DEFAULT_FFT_SIZE);
    analyser
  }

  /// Change la taille de FFT : 128 normalement, 32 quand l'interface est masquée.
  pub fn set_fft_size(&mut self, size: usize) {
    if size == self.fft_size || !size.is_power_of_two() || size > RING_CAPACITY {
      return;
    }
    self.fft_size = size;
    self.fft = self.planner.plan_fft_forward(size);
    self.window = (0..size).map(|n| blackman(n, size)).collect();
    self.samples = vec![0.0; size];
    self.buffer = vec![Complex32::default(); size];
    self.scratch = vec![Complex32::default(); self.fft.get_inplace_scratch_len()];
    self.magnitudes = vec![0.0; size / 2];
    self.last_written = None;
  }

  fn analyse(&mut self) {
    let written = self.ring.written();
    if self.last_written == Some(written) {
      return;
    }
    self.last_written = Some(written);
    self.ring.read_latest(&mut self.samples);
    for ((slot, sample), window) in self.buffer.iter_mut().zip(&self.samples).zip(&self.window) {
      *slot = Complex32::new(sample * window, 0.0);
    }
    self
      .fft
      .process_with_scratch(&mut self.buffer, &mut self.scratch);
    let scale = 1.0 / self.fft_size as f64;
    for (magnitude, bin) in self.magnitudes.iter_mut().zip(&self.buffer) {
      let current = f64::from(bin.norm()) * scale;
      let value = SMOOTHING * f64::from(*magnitude) + (1.0 - SMOOTHING) * current;
      let value = value as f32;
      *magnitude = if value.is_finite() { value } else { 0.0 };
    }
  }

  /// `getByteFrequencyData` : `floor(255/(max−min)·(dB−min))`, borné à 0..=255.
  pub fn frequency_bytes(&mut self, out: &mut [u8]) {
    self.analyse();
    let factor = 255.0 / (MAX_DECIBELS - MIN_DECIBELS);
    for (slot, magnitude) in out.iter_mut().zip(&self.magnitudes) {
      let db = 20.0 * f64::from(*magnitude).log10();
      *slot = (factor * (db - MIN_DECIBELS)).floor().clamp(0.0, 255.0) as u8;
    }
  }

  /// `getByteTimeDomainData` : `128·(x+1)`, borné à 0..=255.
  pub fn time_bytes(&self, out: &mut [u8]) {
    let mut samples = vec![0.0; self.fft_size];
    self.ring.read_latest(&mut samples);
    for (slot, sample) in out.iter_mut().zip(&samples) {
      *slot = (128.0 * (f64::from(*sample) + 1.0)).clamp(0.0, 255.0) as u8;
    }
  }

  /// `getLevelDb` : RMS des 2 048 derniers échantillons, −120 si nul.
  pub fn level_db(&mut self) -> f32 {
    self.level_ring.read_latest(&mut self.level_samples);
    let energy: f64 = self
      .level_samples
      .iter()
      .map(|v| f64::from(*v).powi(2))
      .sum();
    let rms = (energy / LEVEL_FFT_SIZE as f64).sqrt();
    if rms > 0.0 {
      (20.0 * rms.log10()) as f32
    } else {
      SILENCE_DB
    }
  }
}

fn blackman(n: usize, size: usize) -> f32 {
  let x = n as f64 / size as f64;
  let a0 = 0.5 * (1.0 - BLACKMAN_ALPHA);
  let a2 = 0.5 * BLACKMAN_ALPHA;
  (a0 - 0.5 * (TAU * x).cos() + a2 * (2.0 * TAU * x).cos()) as f32
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f64::consts::TAU;

  fn analyser_with(samples: &[f32], size: usize) -> Analyser {
    let ring = Arc::new(SampleRing::new(RING_CAPACITY));
    ring.push_stereo(samples, samples);
    let mut analyser = Analyser::new(ring.clone(), ring);
    analyser.set_fft_size(size);
    analyser
  }

  fn sine(freq: f64, amplitude: f64, frames: usize) -> Vec<f32> {
    (0..frames)
      .map(|i| (amplitude * (TAU * freq * i as f64 / 48_000.0).sin()) as f32)
      .collect()
  }

  #[test]
  fn ring_returns_the_latest_samples_in_order() {
    let ring = SampleRing::new(8);
    let values: Vec<f32> = (0..20).map(|v| v as f32).collect();
    ring.push_stereo(&values, &values);
    let mut out = [0.0; 4];
    ring.read_latest(&mut out);
    assert_eq!(out, [16.0, 17.0, 18.0, 19.0]);
    assert_eq!(ring.written(), 20);
  }

  #[test]
  fn ring_downmixes_with_the_speakers_rule() {
    let ring = SampleRing::new(8);
    ring.push_stereo(&[1.0], &[0.0]);
    let mut out = [0.0; 1];
    ring.read_latest(&mut out);
    assert_eq!(out, [0.5]);
  }

  #[test]
  fn peak_of_a_1khz_sine_is_in_the_right_bin() {
    let mut analyser = analyser_with(&sine(1000.0, 0.01, 4096), 2048);
    let mut bytes = vec![0u8; 1024];
    analyser.frequency_bytes(&mut bytes);
    let peak = bytes
      .iter()
      .enumerate()
      .max_by_key(|(_, v)| **v)
      .map(|(i, _)| i)
      .unwrap_or(0) as i64;
    let expected = (1000.0f64 / (48_000.0 / 2048.0)).round() as i64;
    assert!(
      (peak - expected).abs() <= 1,
      "case {peak} au lieu de {expected}"
    );
    assert!(bytes[peak as usize] > 0);
  }

  #[test]
  fn frequency_bytes_follow_the_web_audio_formulas() {
    // Constante 1 : X[0] = somme(w)/N = a0 = 0,42 ; lissage τ = 0,8 sur un tampon nul.
    let ring = Arc::new(SampleRing::new(RING_CAPACITY));
    let mut analyser = Analyser::new(ring.clone(), ring.clone());
    analyser.set_fft_size(128);
    let scale = 1e-3f32;
    ring.push_stereo(&[scale; 256], &[scale; 256]);
    let mut bytes = [0u8; 64];
    analyser.frequency_bytes(&mut bytes);
    let first = 0.2 * 0.42 * f64::from(scale);
    let want = (255.0 / 70.0 * (20.0 * first.log10() + 100.0)).floor() as u8;
    assert!(
      bytes[0].abs_diff(want) <= 1,
      "{} au lieu de {want}",
      bytes[0]
    );

    ring.push_stereo(&[scale; 256], &[scale; 256]);
    analyser.frequency_bytes(&mut bytes);
    let second = 0.8 * first + 0.2 * 0.42 * f64::from(scale);
    let want = (255.0 / 70.0 * (20.0 * second.log10() + 100.0)).floor() as u8;
    assert!(
      bytes[0].abs_diff(want) <= 1,
      "{} au lieu de {want}",
      bytes[0]
    );
  }

  #[test]
  fn repeated_calls_without_new_audio_do_not_smooth_again() {
    let mut analyser = analyser_with(&sine(1000.0, 0.01, 4096), 128);
    let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
    analyser.frequency_bytes(&mut a);
    analyser.frequency_bytes(&mut b);
    assert_eq!(a, b);
  }

  #[test]
  fn silence_gives_zero_bytes_and_clips_above_max_decibels() {
    let mut silent = analyser_with(&[0.0; 256], 128);
    let mut bytes = [255u8; 64];
    silent.frequency_bytes(&mut bytes);
    assert!(bytes.iter().all(|v| *v == 0));

    let mut loud = analyser_with(&vec![1.0; 4096], 128);
    for _ in 0..20 {
      loud.ring.push_stereo(&[1.0; 128], &[1.0; 128]);
      loud.frequency_bytes(&mut bytes);
    }
    assert_eq!(bytes[0], 255);
  }

  #[test]
  fn time_bytes_follow_the_web_audio_formula() {
    let analyser = analyser_with(&[0.0, 0.5, 1.0, -1.0, 2.0, -0.5], 32);
    let mut bytes = [0u8; 6];
    analyser.time_bytes(&mut bytes);
    // Les 32 derniers échantillons, on regarde les 6 derniers.
    let mut all = [0u8; 32];
    analyser.time_bytes(&mut all);
    assert_eq!(all[26..], [128, 192, 255, 0, 255, 64]);
  }

  #[test]
  fn hidden_mode_uses_a_smaller_fft() {
    let mut analyser = analyser_with(&sine(1000.0, 0.01, 4096), 32);
    let mut bytes = [0u8; 16];
    analyser.frequency_bytes(&mut bytes);
    assert!(bytes.iter().any(|v| *v > 0));
  }

  #[test]
  fn level_of_a_full_scale_sine_is_minus_three_db() {
    let got = analyser_with(&sine(1000.0, 1.0, 4800), 128).level_db();
    assert!((got - (-3.01)).abs() < 0.05, "niveau {got}");
  }

  #[test]
  fn level_of_silence_is_minus_120() {
    assert_eq!(analyser_with(&[0.0; 4096], 128).level_db(), SILENCE_DB);
  }
}
