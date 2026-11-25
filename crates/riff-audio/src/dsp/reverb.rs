//! Réverbération : réponse impulsionnelle de Riff et convolution partitionnée.
//!
//! La convolution est sans latence, comme `ConvolverNode` : les 128 premières prises sont appliquées
//! en direct, le reste par deux étages FFT dont le retard égale exactement le décalage de leur
//! segment dans la réponse.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

const IR_SECONDS: f64 = 0.6;
const DECAY_SECONDS: f64 = 0.15;
/// Graine fixe : réponse déterministe, pour des tests et des rendus reproductibles.
const IR_SEED: u64 = 0x5249_4646_2d52_5331;
/// `kGainCalibration` de Chromium, en dB.
const GAIN_CALIBRATION_DB: f64 = -58.0;
const GAIN_CALIBRATION_RATE: f64 = 44_100.0;
const MIN_POWER: f64 = 0.000125;
const DIRECT_TAPS: usize = 128;
const SMALL_BLOCK: usize = 128;
const LARGE_BLOCK: usize = 1024;

/// Générateur xorshift64*, suffisant pour du bruit (`Math.random()` dans l'original).
struct Prng(u64);

impl Prng {
  fn next_unit(&mut self) -> f64 {
    self.0 ^= self.0 >> 12;
    self.0 ^= self.0 << 25;
    self.0 ^= self.0 >> 27;
    let value = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
    (value >> 11) as f64 / (1u64 << 53) as f64
  }
}

/// Réponse impulsionnelle stéréo de `_createReverbImpulse`, avant normalisation.
pub fn impulse_response(fs: f64) -> [Vec<f32>; 2] {
  let len = (fs * IR_SECONDS).floor() as usize;
  let mut prng = Prng(IR_SEED);
  let mut left = Vec::with_capacity(len);
  let mut right = Vec::with_capacity(len);
  for i in 0..len {
    let decay = (-(i as f64) / (fs * DECAY_SECONDS)).exp();
    left.push(((prng.next_unit() * 2.0 - 1.0) * decay) as f32);
    right.push(((prng.next_unit() * 2.0 - 1.0) * decay) as f32);
  }
  [left, right]
}

/// `CalculateNormalizationScale` de Chromium (`ConvolverNode.normalize`, vrai par défaut).
pub fn normalization_scale(ir: &[Vec<f32>; 2], fs: f64) -> f32 {
  let samples = (ir[0].len() + ir[1].len()) as f64;
  let energy: f64 = ir.iter().flatten().map(|v| f64::from(*v).powi(2)).sum();
  let mut power = (energy / samples).sqrt();
  if !power.is_finite() || power < MIN_POWER {
    power = MIN_POWER;
  }
  (10f64.powf(GAIN_CALIBRATION_DB / 20.0) / power * (GAIN_CALIBRATION_RATE / fs)) as f32
}

/// Convolution uniforme par recouvrement-sauvegarde : retard égal à `block`.
struct FftStage {
  block: usize,
  forward: Arc<dyn Fft<f32>>,
  inverse: Arc<dyn Fft<f32>>,
  scratch: Box<[Complex32]>,
  partitions: Box<[Complex32]>,
  history: Box<[Complex32]>,
  head: usize,
  count: usize,
  input: Box<[f32]>,
  output: Box<[f32]>,
  spectrum: Box<[Complex32]>,
  time: Box<[Complex32]>,
  fill: usize,
}

impl FftStage {
  fn new(segment: &[f32], block: usize, planner: &mut FftPlanner<f32>, scale: f32) -> Self {
    let size = block * 2;
    let bins = block + 1;
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);
    let scratch_len = forward
      .get_inplace_scratch_len()
      .max(inverse.get_inplace_scratch_len())
      .max(1);
    let mut scratch = vec![Complex32::default(); scratch_len].into_boxed_slice();
    let count = segment.len().div_ceil(block);
    let mut partitions = vec![Complex32::default(); count * bins].into_boxed_slice();
    let mut buffer = vec![Complex32::default(); size];
    for (slot, chunk) in partitions.chunks_exact_mut(bins).zip(segment.chunks(block)) {
      buffer.fill(Complex32::default());
      for (target, value) in buffer.iter_mut().zip(chunk) {
        target.re = value * scale;
      }
      forward.process_with_scratch(&mut buffer, &mut scratch);
      slot.copy_from_slice(&buffer[..bins]);
    }
    Self {
      block,
      forward,
      inverse,
      scratch,
      partitions,
      history: vec![Complex32::default(); count * bins].into_boxed_slice(),
      head: 0,
      count,
      input: vec![0.0; size].into_boxed_slice(),
      output: vec![0.0; block].into_boxed_slice(),
      spectrum: vec![Complex32::default(); bins].into_boxed_slice(),
      time: vec![Complex32::default(); size].into_boxed_slice(),
      fill: 0,
    }
  }

  fn reset(&mut self) {
    self.history.fill(Complex32::default());
    self.input.fill(0.0);
    self.output.fill(0.0);
    self.head = 0;
    self.fill = 0;
  }

  /// Ajoute la sortie de l'étage à `out`.
  fn process(&mut self, input: &[f32], out: &mut [f32]) {
    let mut pos = 0;
    while pos < input.len() {
      let n = (self.block - self.fill).min(input.len() - pos);
      let start = self.block + self.fill;
      self.input[start..start + n].copy_from_slice(&input[pos..pos + n]);
      for (slot, value) in out[pos..pos + n]
        .iter_mut()
        .zip(&self.output[self.fill..self.fill + n])
      {
        *slot += *value;
      }
      self.fill += n;
      pos += n;
      if self.fill == self.block {
        self.run_block();
        self.fill = 0;
      }
    }
  }

  fn run_block(&mut self) {
    let size = self.block * 2;
    let bins = self.block + 1;
    for (slot, value) in self.time.iter_mut().zip(self.input.iter()) {
      *slot = Complex32::new(*value, 0.0);
    }
    self.input.copy_within(self.block..size, 0);
    self
      .forward
      .process_with_scratch(&mut self.time, &mut self.scratch);

    self.head = if self.head == 0 {
      self.count - 1
    } else {
      self.head - 1
    };
    let start = self.head * bins;
    self.history[start..start + bins].copy_from_slice(&self.time[..bins]);

    self.spectrum.fill(Complex32::default());
    let mut index = self.head;
    for partition in self.partitions.chunks_exact(bins) {
      let older = &self.history[index * bins..(index + 1) * bins];
      for ((slot, x), h) in self.spectrum.iter_mut().zip(older).zip(partition) {
        slot.re += x.re * h.re - x.im * h.im;
        slot.im += x.re * h.im + x.im * h.re;
      }
      index = if index + 1 == self.count {
        0
      } else {
        index + 1
      };
    }

    self.time[..bins].copy_from_slice(&self.spectrum);
    for k in 1..self.block {
      self.time[size - k] = self.spectrum[k].conj();
    }
    self
      .inverse
      .process_with_scratch(&mut self.time, &mut self.scratch);
    let norm = 1.0 / size as f32;
    for (slot, value) in self.output.iter_mut().zip(&self.time[self.block..]) {
      *slot = value.re * norm;
    }
  }
}

/// Filtre direct des premières prises, sans latence.
struct DirectStage {
  reversed: Box<[f32]>,
  buffer: Box<[f32]>,
}

impl DirectStage {
  fn new(taps: &[f32], scale: f32) -> Self {
    let reversed: Box<[f32]> = taps.iter().rev().map(|v| v * scale).collect();
    let buffer = vec![0.0; reversed.len().saturating_sub(1) + SMALL_BLOCK].into_boxed_slice();
    Self { reversed, buffer }
  }

  fn reset(&mut self) {
    self.buffer.fill(0.0);
  }

  fn process(&mut self, input: &[f32], out: &mut [f32]) {
    let history = self.reversed.len().saturating_sub(1);
    for (block, target) in input.chunks(SMALL_BLOCK).zip(out.chunks_mut(SMALL_BLOCK)) {
      self.buffer[history..history + block.len()].copy_from_slice(block);
      for (i, slot) in target.iter_mut().enumerate() {
        *slot += super::waveshaper::dot(&self.reversed, &self.buffer[i..=i + history]);
      }
      self
        .buffer
        .copy_within(block.len()..block.len() + history, 0);
    }
  }
}

struct Convolver {
  direct: DirectStage,
  small: Option<FftStage>,
  large: Option<FftStage>,
}

impl Convolver {
  fn new(ir: &[f32], scale: f32, planner: &mut FftPlanner<f32>) -> Self {
    let direct_end = DIRECT_TAPS.min(ir.len());
    let small_end = LARGE_BLOCK.min(ir.len());
    let small_segment = &ir[direct_end..small_end];
    let large_segment = &ir[small_end..];
    Self {
      direct: DirectStage::new(&ir[..direct_end], scale),
      small: (!small_segment.is_empty())
        .then(|| FftStage::new(small_segment, SMALL_BLOCK, planner, scale)),
      large: (!large_segment.is_empty())
        .then(|| FftStage::new(large_segment, LARGE_BLOCK, planner, scale)),
    }
  }

  fn reset(&mut self) {
    self.direct.reset();
    self.small.iter_mut().for_each(FftStage::reset);
    self.large.iter_mut().for_each(FftStage::reset);
  }

  fn process(&mut self, input: &[f32], out: &mut [f32]) {
    out.fill(0.0);
    self.direct.process(input, out);
    if let Some(stage) = &mut self.small {
      stage.process(input, out);
    }
    if let Some(stage) = &mut self.large {
      stage.process(input, out);
    }
  }
}

pub struct Reverb {
  channels: [Convolver; 2],
}

impl Reverb {
  pub fn new(fs: f64) -> Self {
    let ir = impulse_response(fs);
    let scale = normalization_scale(&ir, fs);
    Self::with_impulse(&ir, scale)
  }

  pub fn with_impulse(ir: &[Vec<f32>; 2], scale: f32) -> Self {
    let mut planner = FftPlanner::new();
    Self {
      channels: [
        Convolver::new(&ir[0], scale, &mut planner),
        Convolver::new(&ir[1], scale, &mut planner),
      ],
    }
  }

  pub fn reset(&mut self) {
    self.channels.iter_mut().for_each(Convolver::reset);
  }

  /// Écrit la sortie de la convolution (avant le gain `reverb/100`) dans `out_*`.
  pub fn process(
    &mut self,
    in_left: &[f32],
    in_right: &[f32],
    out_left: &mut [f32],
    out_right: &mut [f32],
  ) {
    let [left, right] = &mut self.channels;
    left.process(in_left, out_left);
    right.process(in_right, out_right);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const FS: f64 = 48_000.0;

  fn noise(frames: usize) -> Vec<f32> {
    let mut prng = Prng(0xDEAD_BEEF_CAFE_F00D);
    (0..frames)
      .map(|_| (prng.next_unit() * 2.0 - 1.0) as f32 * 0.5)
      .collect()
  }

  fn direct(input: &[f32], ir: &[f32], scale: f32, frames: usize) -> Vec<f32> {
    let mut out = vec![0f64; frames];
    for (n, slot) in out.iter_mut().enumerate() {
      for (k, h) in ir.iter().enumerate().take(n + 1) {
        if let Some(x) = input.get(n - k) {
          *slot += f64::from(*x) * f64::from(*h);
        }
      }
    }
    out.iter().map(|v| (*v * f64::from(scale)) as f32).collect()
  }

  fn run(reverb: &mut Reverb, left: &[f32], right: &[f32], block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut out_l = vec![0.0; left.len()];
    let mut out_r = vec![0.0; right.len()];
    let inputs = left.chunks(block).zip(right.chunks(block));
    let outputs = out_l.chunks_mut(block).zip(out_r.chunks_mut(block));
    for ((il, ir), (ol, or)) in inputs.zip(outputs) {
      reverb.process(il, ir, ol, or);
    }
    (out_l, out_r)
  }

  #[test]
  fn impulse_has_expected_length_and_is_deterministic() {
    let ir = impulse_response(FS);
    assert_eq!(ir[0].len(), 28_800);
    assert_eq!(ir[1].len(), 28_800);
    assert_eq!(impulse_response(44_100.0)[0].len(), 26_460);
    assert_eq!(ir, impulse_response(FS));
    assert_ne!(ir[0], ir[1], "canaux décorrélés");
  }

  #[test]
  fn impulse_energy_decays_exponentially() {
    let ir = impulse_response(FS);
    let window = 2048;
    for (w, chunk) in ir[0].chunks_exact(window).enumerate().take(8) {
      let energy: f64 = chunk.iter().map(|v| f64::from(*v).powi(2)).sum();
      let want: f64 = (0..window)
        .map(|j| {
          let i = (w * window + j) as f64;
          (-2.0 * i / (FS * DECAY_SECONDS)).exp() / 3.0
        })
        .sum();
      assert!(
        (energy / want - 1.0).abs() < 0.15,
        "fenêtre {w} : {energy} au lieu de {want}"
      );
    }
  }

  #[test]
  fn normalization_follows_chromium_formula() {
    let ir = impulse_response(44_100.0);
    let scale = f64::from(normalization_scale(&ir, 44_100.0));
    let power: f64 = ir
      .iter()
      .flatten()
      .map(|v| f64::from(*v).powi(2))
      .sum::<f64>()
      / (2.0 * ir[0].len() as f64);
    let want = 10f64.powf(-58.0 / 20.0) / power.sqrt();
    assert!(
      (scale / want - 1.0).abs() < 1e-3,
      "{scale} au lieu de {want}"
    );

    let ir = impulse_response(FS);
    let at_48k = f64::from(normalization_scale(&ir, FS));
    let power: f64 = ir
      .iter()
      .flatten()
      .map(|v| f64::from(*v).powi(2))
      .sum::<f64>()
      / (2.0 * ir[0].len() as f64);
    let want = 10f64.powf(-58.0 / 20.0) / power.sqrt() * (44_100.0 / FS);
    assert!(
      (at_48k / want - 1.0).abs() < 1e-3,
      "{at_48k} au lieu de {want}"
    );
  }

  #[test]
  fn silent_impulse_uses_minimum_power() {
    let ir = [vec![0.0; 100], vec![0.0; 100]];
    let want = 10f64.powf(-58.0 / 20.0) / 0.000125 * (44_100.0 / FS);
    let got = f64::from(normalization_scale(&ir, FS));
    assert!((got / want - 1.0).abs() < 1e-3);
  }

  #[test]
  fn convolution_matches_direct_form_on_4096_samples() {
    let ir = impulse_response(FS);
    let scale = normalization_scale(&ir, FS);
    let (left, right) = (
      noise(4096),
      noise(4096).iter().rev().copied().collect::<Vec<_>>(),
    );
    let want_l = direct(&left, &ir[0], scale, 4096);
    let want_r = direct(&right, &ir[1], scale, 4096);
    for block in [128, 64, 480, 1000] {
      let mut reverb = Reverb::new(FS);
      let (got_l, got_r) = run(&mut reverb, &left, &right, block);
      for i in 0..4096 {
        assert!((got_l[i] - want_l[i]).abs() < 1e-4, "bloc {block} L[{i}]");
        assert!((got_r[i] - want_r[i]).abs() < 1e-4, "bloc {block} R[{i}]");
      }
    }
  }

  #[test]
  fn impulse_input_reproduces_the_whole_scaled_response() {
    let ir = impulse_response(FS);
    let scale = normalization_scale(&ir, FS);
    let frames = ir[0].len() + 4096;
    let mut input = vec![0.0; frames];
    input[0] = 1.0;
    let mut reverb = Reverb::new(FS);
    let (left, right) = run(&mut reverb, &input, &input, 128);
    for i in 0..frames {
      let want = ir[0].get(i).copied().unwrap_or(0.0) * scale;
      assert!(
        (left[i] - want).abs() < 1e-5,
        "L[{i}] = {} au lieu de {want}",
        left[i]
      );
      let want = ir[1].get(i).copied().unwrap_or(0.0) * scale;
      assert!(
        (right[i] - want).abs() < 1e-5,
        "R[{i}] = {} au lieu de {want}",
        right[i]
      );
    }
  }

  #[test]
  fn reset_clears_the_tail() {
    let mut reverb = Reverb::new(FS);
    let input = noise(4096);
    run(&mut reverb, &input, &input, 128);
    reverb.reset();
    let silence = vec![0.0; 32_768];
    let (left, _) = run(&mut reverb, &silence, &silence, 128);
    assert!(left.iter().all(|v| *v == 0.0));
  }
}
