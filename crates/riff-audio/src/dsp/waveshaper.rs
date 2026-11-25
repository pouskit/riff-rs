//! Waveshaper : courbes de distorsion et d'harmoniques de Riff, application selon Chromium.

use std::f64::consts::{PI, TAU};

pub const CURVE_LEN: usize = 256;
pub type Curve = [f32; CURVE_LEN];

const UP_KERNEL: usize = 128;
const DOWN_KERNEL: usize = 256;
/// Taille maximale d'un bloc à la fréquence d'entrée (quantum de Web Audio).
const MAX_BLOCK: usize = 128;
const BLACKMAN_ALPHA: f64 = 0.16;

fn grid(i: usize) -> f64 {
  (i as f64 * 2.0) / CURVE_LEN as f64 - 1.0
}

/// Courbe de distorsion (`_makeDistortionCurve`, `amount` de 0 à 100).
pub fn distortion_curve(amount: f32) -> Curve {
  let k = f64::from(amount) * 2.5;
  std::array::from_fn(|i| {
    let x = grid(i);
    let y = if k <= 0.0 {
      x
    } else {
      ((3.0 + k) * x * 20.0 * (PI / 180.0)) / (PI + k * x.abs())
    };
    y as f32
  })
}

/// Courbe d'harmoniques (`_makeHarmonicCurve`).
pub fn harmonic_curve(amount: f32) -> Curve {
  let k = f64::from(amount) * 0.02;
  std::array::from_fn(|i| {
    let x = grid(i);
    (x - k * x.powi(3)) as f32
  })
}

/// Valeur de la courbe pour un échantillon (`WaveShaperCurveValue` de Chromium).
#[inline]
pub fn shape(input: f32, curve: &Curve) -> f32 {
  let last = CURVE_LEN - 1;
  let virtual_index = 0.5 * (f64::from(input) + 1.0) * last as f64;
  if virtual_index < 0.0 {
    return curve[0];
  }
  if virtual_index >= last as f64 {
    return curve[last];
  }
  let index = virtual_index as usize;
  let frac = virtual_index - index as f64;
  ((1.0 - frac) * f64::from(curve[index]) + frac * f64::from(curve[index + 1])) as f32
}

fn blackman(x: f64) -> f64 {
  let a0 = 0.5 * (1.0 - BLACKMAN_ALPHA);
  let a2 = 0.5 * BLACKMAN_ALPHA;
  a0 - 0.5 * (TAU * x).cos() + a2 * (TAU * 2.0 * x).cos()
}

fn sinc(s: f64) -> f64 {
  if s == 0.0 { 1.0 } else { s.sin() / s }
}

/// Noyau d'interpolation de `UpSampler` : sinc décalée de −0,5 fenêtrée.
fn up_kernel() -> Vec<f32> {
  let half = (UP_KERNEL / 2) as f64;
  (0..UP_KERNEL)
    .map(|i| {
      let i = i as f64;
      let window = blackman((i + 0.5) / UP_KERNEL as f64);
      (sinc(PI * (i - half + 0.5)) * window) as f32
    })
    .collect()
}

/// Noyau réduit de `DownSampler` : termes impairs d'un demi-bande de 256 prises.
fn down_kernel() -> Vec<f32> {
  let half = (DOWN_KERNEL / 2) as f64;
  (0..DOWN_KERNEL / 2)
    .map(|index| {
      let i = (2 * index + 1) as f64;
      let window = blackman(i / DOWN_KERNEL as f64);
      (0.5 * sinc(0.5 * PI * (i - half)) * window) as f32
    })
    .collect()
}

/// Produit scalaire par groupes de 8 pour permettre la vectorisation par le compilateur.
#[inline]
pub(super) fn dot(a: &[f32], b: &[f32]) -> f32 {
  let mut acc = [0.0f32; 8];
  let ((ca, ra), (cb, rb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
  for (x, y) in ca.iter().zip(cb) {
    for ((slot, u), v) in acc.iter_mut().zip(x).zip(y) {
      *slot += u * v;
    }
  }
  let tail: f32 = ra.iter().zip(rb).map(|(u, v)| u * v).sum();
  acc.iter().sum::<f32>() + tail
}

/// Filtre FIR direct, noyau retourné, historique en tête du tampon.
struct Fir {
  reversed: Box<[f32]>,
  buffer: Box<[f32]>,
}

impl Fir {
  fn new(kernel: &[f32], max_block: usize) -> Self {
    let reversed: Box<[f32]> = kernel.iter().rev().copied().collect();
    let buffer = vec![0.0; kernel.len() - 1 + max_block].into_boxed_slice();
    Self { reversed, buffer }
  }

  fn process(&mut self, input: &[f32], mut output: impl FnMut(usize, f32)) {
    let history = self.reversed.len() - 1;
    self.buffer[history..history + input.len()].copy_from_slice(input);
    for i in 0..input.len() {
      output(i, dot(&self.reversed, &self.buffer[i..=i + history]));
    }
    self
      .buffer
      .copy_within(input.len()..input.len() + history, 0);
  }
}

/// `UpSampler` de Chromium : pairs retardés, impairs interpolés.
struct UpSampler {
  fir: Fir,
  delay: Box<[f32]>,
}

impl UpSampler {
  fn new(kernel: &[f32], max_block: usize) -> Self {
    let delay = vec![0.0; UP_KERNEL / 2 + max_block].into_boxed_slice();
    Self {
      fir: Fir::new(kernel, max_block),
      delay,
    }
  }

  fn process(&mut self, source: &[f32], dest: &mut [f32]) {
    let half = UP_KERNEL / 2;
    self.delay[half..half + source.len()].copy_from_slice(source);
    for (i, sample) in self.delay[..source.len()].iter().enumerate() {
      dest[i * 2] = *sample;
    }
    self.fir.process(source, |i, value| dest[i * 2 + 1] = value);
    self.delay.copy_within(source.len()..source.len() + half, 0);
  }
}

/// `DownSampler` de Chromium : noyau réduit sur les impairs, terme central 0,5 sur les pairs.
struct DownSampler {
  fir: Fir,
  delay: Box<[f32]>,
  odd: Box<[f32]>,
}

impl DownSampler {
  fn new(kernel: &[f32], max_block: usize) -> Self {
    let half = DOWN_KERNEL / 2;
    Self {
      fir: Fir::new(kernel, max_block / 2),
      delay: vec![0.0; half + max_block].into_boxed_slice(),
      odd: vec![0.0; max_block / 2].into_boxed_slice(),
    }
  }

  fn process(&mut self, source: &[f32], dest: &mut [f32]) {
    let half = DOWN_KERNEL / 2;
    let frames = source.len() / 2;
    self.delay[half..half + source.len()].copy_from_slice(source);
    for (i, slot) in self.odd[..frames].iter_mut().enumerate() {
      *slot = self.delay[half - 1 + i * 2];
    }
    let delay = &self.delay;
    self.fir.process(&self.odd[..frames], |i, value| {
      dest[i] = value + 0.5 * delay[i * 2]
    });
    self.delay.copy_within(source.len()..source.len() + half, 0);
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Oversample {
  None,
  X2,
  X4,
}

impl Oversample {
  /// Latence en échantillons à la fréquence d'entrée (`WaveShaperHandler::LatencyTime`).
  pub const fn latency_frames(self) -> usize {
    match self {
      Self::None => 0,
      Self::X2 => UP_KERNEL,
      Self::X4 => UP_KERNEL + UP_KERNEL / 2,
    }
  }
}

/// État de suréchantillonnage d'un canal.
struct OversampledChannel {
  up1: UpSampler,
  down1: DownSampler,
  stage2: Option<(UpSampler, DownSampler)>,
  x2: Box<[f32]>,
  x4: Box<[f32]>,
}

impl OversampledChannel {
  fn new(mode: Oversample, up: &[f32], down: &[f32]) -> Self {
    let stage2 = matches!(mode, Oversample::X4).then(|| {
      (
        UpSampler::new(up, MAX_BLOCK * 2),
        DownSampler::new(down, MAX_BLOCK * 4),
      )
    });
    Self {
      up1: UpSampler::new(up, MAX_BLOCK),
      down1: DownSampler::new(down, MAX_BLOCK * 2),
      stage2,
      x2: vec![0.0; MAX_BLOCK * 2].into_boxed_slice(),
      x4: vec![0.0; MAX_BLOCK * 4].into_boxed_slice(),
    }
  }

  fn process(&mut self, samples: &mut [f32], curve: &Curve) {
    let frames = samples.len();
    let x2 = &mut self.x2[..frames * 2];
    self.up1.process(samples, x2);
    if let Some((up2, down2)) = &mut self.stage2 {
      let x4 = &mut self.x4[..frames * 4];
      up2.process(x2, x4);
      x4.iter_mut().for_each(|v| *v = shape(*v, curve));
      down2.process(x4, x2);
    } else {
      x2.iter_mut().for_each(|v| *v = shape(*v, curve));
    }
    self.down1.process(x2, samples);
  }
}

pub struct WaveShaper {
  curve: Curve,
  channels: Option<Box<[OversampledChannel; 2]>>,
}

impl WaveShaper {
  pub fn new(curve: Curve, oversample: Oversample) -> Self {
    let channels = (oversample != Oversample::None).then(|| {
      let (up, down) = (up_kernel(), down_kernel());
      Box::new([
        OversampledChannel::new(oversample, &up, &down),
        OversampledChannel::new(oversample, &up, &down),
      ])
    });
    Self { curve, channels }
  }

  pub fn set_curve(&mut self, curve: Curve) {
    self.curve = curve;
  }

  /// Traite un bloc stéréo de longueur quelconque, découpé en blocs de 128 au plus.
  pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
    let curve = &self.curve;
    match &mut self.channels {
      None => {
        left
          .iter_mut()
          .chain(right.iter_mut())
          .for_each(|v| *v = shape(*v, curve));
      }
      Some(channels) => {
        let [cl, cr] = &mut **channels;
        for block in left.chunks_mut(MAX_BLOCK) {
          cl.process(block, curve);
        }
        for block in right.chunks_mut(MAX_BLOCK) {
          cr.process(block, curve);
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f64::consts::PI;

  const INDICES: [usize; 5] = [0, 64, 128, 192, 255];

  fn grid(i: usize) -> f64 {
    (i as f64 * 2.0) / 256.0 - 1.0
  }

  #[test]
  fn distortion_curve_matches_formula_at_reference_indices() {
    for amount in [0.0f64, 50.0, 100.0] {
      let curve = distortion_curve(amount as f32);
      let k = amount * 2.5;
      for i in INDICES {
        let x = grid(i);
        let want = if k <= 0.0 {
          x
        } else {
          ((3.0 + k) * x * 20.0 * (PI / 180.0)) / (PI + k * x.abs())
        };
        assert_eq!(curve[i], want as f32, "amount={amount} i={i}");
      }
    }
  }

  #[test]
  fn harmonic_curve_matches_formula_at_reference_indices() {
    for amount in [0.0f64, 0.2, 0.5] {
      let curve = harmonic_curve(amount as f32);
      let k = f64::from(amount as f32) * 0.02;
      for i in INDICES {
        let x = grid(i);
        let want = x - k * x.powi(3);
        assert!(
          (f64::from(curve[i]) - want).abs() < 1e-6,
          "amount={amount} i={i}"
        );
      }
    }
  }

  #[test]
  fn shape_uses_web_audio_bounds_and_interpolation() {
    let curve = distortion_curve(50.0);
    assert_eq!(shape(-1.5, &curve), curve[0]);
    assert_eq!(shape(1.0, &curve), curve[255]);
    assert_eq!(shape(3.0, &curve), curve[255]);
    assert_eq!(shape(-1.0, &curve), curve[0]);
    let want = 0.5 * (f64::from(curve[127]) + f64::from(curve[128]));
    assert!((f64::from(shape(0.0, &curve)) - want).abs() < 1e-7);
  }

  fn run(shaper: &mut WaveShaper, input: &[f32]) -> Vec<f32> {
    let mut l = input.to_vec();
    let mut r = input.to_vec();
    for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
      shaper.process(cl, cr);
    }
    assert_eq!(l, r);
    l
  }

  fn sine(freq: f64, frames: usize) -> Vec<f32> {
    (0..frames)
      .map(|i| ((2.0 * PI * freq * i as f64 / 48_000.0).sin() * 0.5) as f32)
      .collect()
  }

  #[test]
  fn no_oversampling_applies_curve_sample_by_sample() {
    let curve = distortion_curve(50.0);
    let input = sine(1000.0, 256);
    let got = run(&mut WaveShaper::new(curve, Oversample::None), &input);
    for (x, y) in input.iter().zip(&got) {
      assert_eq!(*y, shape(*x, &curve));
    }
  }

  #[test]
  fn oversampled_paths_are_delayed_by_declared_latency() {
    let curve = harmonic_curve(0.0);
    for mode in [Oversample::X2, Oversample::X4] {
      let input = sine(1000.0, 4096);
      let got = run(&mut WaveShaper::new(curve, mode), &input);
      let latency = mode.latency_frames();
      assert!(latency > 0);
      for i in 1024..4096 {
        let want = shape(input[i - latency], &curve);
        assert!(
          (got[i] - want).abs() < 2e-3,
          "{mode:?} i={i} got={} want={want}",
          got[i]
        );
      }
    }
  }

  #[test]
  fn oversampling_reduces_aliasing_of_the_harmonic_curve() {
    // Une sinusoïde de 15 kHz saturée fortement : sa troisième harmonique (45 kHz) se replie à 3 kHz
    // sans suréchantillonnage ; le filtre de décimation l'élimine sinon.
    let mut curve = [0.0; CURVE_LEN];
    for (i, value) in curve.iter_mut().enumerate() {
      let x = grid(i);
      *value = (x - 0.3 * x.powi(3)) as f32;
    }
    let input = sine(15_000.0, 48_000);
    let direct = run(&mut WaveShaper::new(curve, Oversample::None), &input);
    let oversampled = run(&mut WaveShaper::new(curve, Oversample::X2), &input);
    let alias = |signal: &[f32]| -> f64 {
      let (mut re, mut im) = (0.0, 0.0);
      for (i, v) in signal[4096..].iter().enumerate() {
        let w = 2.0 * PI * 3000.0 * i as f64 / 48_000.0;
        re += f64::from(*v) * w.cos();
        im += f64::from(*v) * w.sin();
      }
      (re * re + im * im).sqrt()
    };
    assert!(alias(&oversampled) * 10.0 < alias(&direct));
  }
}
