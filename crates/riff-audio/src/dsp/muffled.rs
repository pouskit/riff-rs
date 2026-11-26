//! Effet « salle de bain en soirée » : transmission à travers un mur/une porte et petite acoustique carrelée.
//!
//! Chaîne sans allocation temps réel :
//! 1. Résonance murale bas-médium (peaking à 135 Hz, +2,5 dB max) pour conserver la frappe du kick.
//! 2. Passe-bas d'ordre 4 (Butterworth en cascade) balayant 16 kHz -> 320 Hz, plus 4 % de fuite sous la porte.
//! 3. Rétrécissement stéréo Mid/Side (la source provient d'une seule paroi).
//! 4. Premières réflexions asymétriques + queue courte type carrelage (4 peignes amortis + 2 passe-tout).

use std::f64::consts::FRAC_1_SQRT_2;

use super::QUANTUM;
use super::biquad::{Biquad, BiquadCoeffs};

const WALL_RESONANCE_HZ: f64 = 135.0;
const WALL_RESONANCE_Q: f64 = 0.9;
const WALL_RESONANCE_MAX_DB: f64 = 2.5;

const LP_START_HZ: f64 = 16_000.0;
const LP_END_HZ: f64 = 320.0;
const LP_CURVE: f64 = 0.75;

const DOOR_LEAK_RATIO: f64 = 2.8;
const DOOR_LEAK_MIX: f32 = 0.04;

const STEREO_NARROWING: f32 = 0.75;
const MAKEUP_GAIN: f32 = 0.35;
const WET_CROSSFADE_END: f32 = 0.15;
const ROOM_MIX: f32 = 0.28;
const SMOOTH_STEP_PER_QUANTUM: f32 = 0.08;

const COMB_FEEDBACK: f32 = 0.68;
const COMB_DAMP: f32 = 0.35;
const ALLPASS_GAIN: f32 = 0.5;

const ER_LEFT_MS: [f64; 3] = [5.3, 11.7, 19.1];
const ER_RIGHT_MS: [f64; 3] = [7.1, 14.3, 23.7];
const ER_GAINS: [f32; 3] = [0.35, -0.25, 0.18];

const COMB_LEFT_MS: [f64; 4] = [21.1, 25.7, 31.3, 37.9];
const COMB_RIGHT_MS: [f64; 4] = [21.6, 26.2, 31.8, 38.4];

const ALLPASS_LEFT_MS: [f64; 2] = [4.7, 6.9];
const ALLPASS_RIGHT_MS: [f64; 2] = [5.1, 7.3];

#[inline]
fn flush_denormal(v: f32) -> f32 {
  if v.abs() < 1e-20 { 0.0 } else { v }
}

fn delay_samples(fs: f64, ms: f64) -> usize {
  let safe_fs = if fs.is_finite() && fs > 0.0 {
    fs
  } else {
    48_000.0
  };
  ((ms * 0.001 * safe_fs).round() as usize).max(1)
}

#[derive(Clone, Debug)]
struct EarlyReflections {
  buf: Box<[f32]>,
  write: usize,
  taps: [usize; 3],
}

impl EarlyReflections {
  fn new(fs: f64, taps_ms: [f64; 3]) -> Self {
    let taps = taps_ms.map(|ms| delay_samples(fs, ms));
    let max_tap = taps.into_iter().max().unwrap_or(1);
    Self {
      buf: vec![0.0; max_tap + 1].into_boxed_slice(),
      write: 0,
      taps,
    }
  }

  fn reset(&mut self) {
    self.buf.fill(0.0);
    self.write = 0;
  }

  #[inline]
  fn process_sample(&mut self, input: f32) -> f32 {
    let len = self.buf.len();
    self.buf[self.write] = input;
    let mut sum = 0.0;
    for (tap, gain) in self.taps.into_iter().zip(ER_GAINS) {
      let idx = (self.write + len - tap) % len;
      sum += self.buf[idx] * gain;
    }
    self.write = if self.write + 1 == len {
      0
    } else {
      self.write + 1
    };
    sum
  }
}

#[derive(Clone, Debug)]
struct DampedComb {
  buf: Box<[f32]>,
  pos: usize,
  filter_state: f32,
}

impl DampedComb {
  fn new(len: usize) -> Self {
    Self {
      buf: vec![0.0; len.max(1)].into_boxed_slice(),
      pos: 0,
      filter_state: 0.0,
    }
  }

  fn reset(&mut self) {
    self.buf.fill(0.0);
    self.pos = 0;
    self.filter_state = 0.0;
  }

  #[inline]
  fn process_sample(&mut self, input: f32) -> f32 {
    let out = self.buf[self.pos];
    self.filter_state = flush_denormal(out * (1.0 - COMB_DAMP) + self.filter_state * COMB_DAMP);
    self.buf[self.pos] = flush_denormal(input + self.filter_state * COMB_FEEDBACK);
    self.pos = if self.pos + 1 == self.buf.len() {
      0
    } else {
      self.pos + 1
    };
    out
  }
}

#[derive(Clone, Debug)]
struct Allpass {
  buf: Box<[f32]>,
  pos: usize,
}

impl Allpass {
  fn new(len: usize) -> Self {
    Self {
      buf: vec![0.0; len.max(1)].into_boxed_slice(),
      pos: 0,
    }
  }

  fn reset(&mut self) {
    self.buf.fill(0.0);
    self.pos = 0;
  }

  #[inline]
  fn process_sample(&mut self, input: f32) -> f32 {
    let buf_out = self.buf[self.pos];
    let v = flush_denormal(input + buf_out * ALLPASS_GAIN);
    self.buf[self.pos] = v;
    self.pos = if self.pos + 1 == self.buf.len() {
      0
    } else {
      self.pos + 1
    };
    buf_out - ALLPASS_GAIN * v
  }
}

#[derive(Clone, Debug)]
struct BathroomChannel {
  early: EarlyReflections,
  combs: [DampedComb; 4],
  allpasses: [Allpass; 2],
}

impl BathroomChannel {
  fn new(fs: f64, er_ms: [f64; 3], comb_ms: [f64; 4], allpass_ms: [f64; 2]) -> Self {
    Self {
      early: EarlyReflections::new(fs, er_ms),
      combs: comb_ms.map(|ms| DampedComb::new(delay_samples(fs, ms))),
      allpasses: allpass_ms.map(|ms| Allpass::new(delay_samples(fs, ms))),
    }
  }

  fn reset(&mut self) {
    self.early.reset();
    for comb in &mut self.combs {
      comb.reset();
    }
    for allpass in &mut self.allpasses {
      allpass.reset();
    }
  }

  #[inline]
  fn process_sample(&mut self, input: f32) -> f32 {
    let early = self.early.process_sample(input);
    let mut tail = 0.0;
    for comb in &mut self.combs {
      tail += comb.process_sample(input);
    }
    tail *= 0.25;
    for allpass in &mut self.allpasses {
      tail = allpass.process_sample(tail);
    }
    early + tail * 0.65
  }
}

/// Filtre mural + acoustique de petite pièce carrelée (« Party bathroom »).
#[derive(Clone, Debug)]
pub struct Muffled {
  fs: f64,
  current: f32,
  target: f32,
  coeffs_amount: f32,
  wall_resonance: Biquad,
  lpf_stage1: Biquad,
  lpf_stage2: Biquad,
  door_leak_lpf: Biquad,
  room: [BathroomChannel; 2],
  dry: [[f32; QUANTUM]; 2],
  leak: [[f32; QUANTUM]; 2],
}

impl Muffled {
  #[must_use]
  pub fn new(fs: f64) -> Self {
    let safe_fs = if fs.is_finite() && fs > 0.0 {
      fs
    } else {
      48_000.0
    };
    Self {
      fs: safe_fs,
      current: 0.0,
      target: 0.0,
      coeffs_amount: -1.0,
      wall_resonance: Biquad::default(),
      lpf_stage1: Biquad::default(),
      lpf_stage2: Biquad::default(),
      door_leak_lpf: Biquad::default(),
      room: [
        BathroomChannel::new(safe_fs, ER_LEFT_MS, COMB_LEFT_MS, ALLPASS_LEFT_MS),
        BathroomChannel::new(safe_fs, ER_RIGHT_MS, COMB_RIGHT_MS, ALLPASS_RIGHT_MS),
      ],
      dry: [[0.0; QUANTUM]; 2],
      leak: [[0.0; QUANTUM]; 2],
    }
  }

  pub fn set_amount(&mut self, percent: f32) {
    self.target = if percent.is_finite() {
      (percent / 100.0).clamp(0.0, 1.0)
    } else {
      0.0
    };
  }

  pub fn settle(&mut self) {
    self.current = self.target;
    if self.current == 0.0 {
      self.reset();
    } else {
      self.update_coeffs(self.current);
    }
  }

  pub fn reset(&mut self) {
    self.wall_resonance.reset();
    self.lpf_stage1.reset();
    self.lpf_stage2.reset();
    self.door_leak_lpf.reset();
    for ch in &mut self.room {
      ch.reset();
    }
  }

  fn update_coeffs(&mut self, amount: f32) {
    if (self.coeffs_amount - amount).abs() < 1e-6 {
      return;
    }
    self.coeffs_amount = amount;
    let a = f64::from(amount);
    let resonance_db = WALL_RESONANCE_MAX_DB * a;
    self.wall_resonance.set_coeffs(BiquadCoeffs::peaking(
      self.fs,
      WALL_RESONANCE_HZ,
      WALL_RESONANCE_Q,
      resonance_db,
    ));
    let cutoff = LP_START_HZ * (LP_END_HZ / LP_START_HZ).powf(a.powf(LP_CURVE));
    let lp_coeffs = BiquadCoeffs::low_pass(self.fs, cutoff, FRAC_1_SQRT_2);
    self.lpf_stage1.set_coeffs(lp_coeffs);
    self.lpf_stage2.set_coeffs(lp_coeffs);
    let leak_cutoff = (cutoff * DOOR_LEAK_RATIO).min(self.fs * 0.45);
    self
      .door_leak_lpf
      .set_coeffs(BiquadCoeffs::low_pass(self.fs, leak_cutoff, FRAC_1_SQRT_2));
  }

  /// Traite des tranches arbitraires par paquets d'au plus [`QUANTUM`] échantillons.
  pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
    for (l, r) in left.chunks_mut(QUANTUM).zip(right.chunks_mut(QUANTUM)) {
      self.process_quantum(l, r);
    }
  }

  /// Traite un bloc d'au plus [`QUANTUM`] échantillons sans allocation.
  pub fn process_quantum(&mut self, left: &mut [f32], right: &mut [f32]) {
    let frames = left.len().min(right.len()).min(QUANTUM);
    if frames == 0 {
      return;
    }
    if self.current == 0.0 && self.target == 0.0 {
      return;
    }

    let max_delta = SMOOTH_STEP_PER_QUANTUM * (frames as f32 / QUANTUM as f32);
    let diff = self.target - self.current;
    if diff.abs() <= max_delta {
      self.current = self.target;
    } else {
      self.current += diff.signum() * max_delta;
    }

    let a = self.current;
    if a == 0.0 {
      self.reset();
      return;
    }
    self.update_coeffs(a);

    let (left, right) = (&mut left[..frames], &mut right[..frames]);
    self.dry[0][..frames].copy_from_slice(left);
    self.dry[1][..frames].copy_from_slice(right);

    self.wall_resonance.process(left, right);
    self.leak[0][..frames].copy_from_slice(left);
    self.leak[1][..frames].copy_from_slice(right);

    self.lpf_stage1.process(left, right);
    self.lpf_stage2.process(left, right);
    let [leak_l, leak_r] = &mut self.leak;
    let (leak_l, leak_r) = (&mut leak_l[..frames], &mut leak_r[..frames]);
    self.door_leak_lpf.process(leak_l, leak_r);

    let width = 1.0 - STEREO_NARROWING * a;
    let makeup = 1.0 + MAKEUP_GAIN * a;
    let room_amount = ROOM_MIX * a;
    let wet_mix = (a / WET_CROSSFADE_END).clamp(0.0, 1.0);
    let dry_mix = 1.0 - wet_mix;

    let [room_left, room_right] = &mut self.room;
    for i in 0..frames {
      let l_wall = left[i] * (1.0 - DOOR_LEAK_MIX) + leak_l[i] * DOOR_LEAK_MIX;
      let r_wall = right[i] * (1.0 - DOOR_LEAK_MIX) + leak_r[i] * DOOR_LEAK_MIX;

      let mid = 0.5 * (l_wall + r_wall);
      let side = 0.5 * (l_wall - r_wall) * width;
      let l_narrow = mid + side;
      let r_narrow = mid - side;

      let l_room = room_left.process_sample(l_narrow);
      let r_room = room_right.process_sample(r_narrow);

      let l_wet = (l_narrow + l_room * room_amount) * makeup;
      let r_wet = (r_narrow + r_room * room_amount) * makeup;

      left[i] = self.dry[0][i] * dry_mix + l_wet * wet_mix;
      right[i] = self.dry[1][i] * dry_mix + r_wet * wet_mix;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f64::consts::TAU;

  const FS: f64 = 48_000.0;

  fn sine(freq: f64, frames: usize) -> Vec<f32> {
    (0..frames)
      .map(|i| (0.25 * (TAU * freq * i as f64 / FS).sin()) as f32)
      .collect()
  }

  fn rms(samples: &[f32]) -> f64 {
    let mean = samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / samples.len() as f64;
    mean.sqrt()
  }

  #[test]
  fn zero_amount_is_bit_exact_bypass() {
    let mut m = Muffled::new(FS);
    let input = sine(1000.0, 1024);
    let (mut l, mut r) = (input.clone(), input.clone());
    m.process(&mut l, &mut r);
    assert_eq!(l, input);
    assert_eq!(r, input);
  }

  #[test]
  fn high_frequencies_are_strongly_attenuated_while_bass_passes() {
    let mut m_bass = Muffled::new(FS);
    m_bass.set_amount(85.0);
    m_bass.settle();
    let bass_in = sine(100.0, 24_000);
    let (mut bl, mut br) = (bass_in.clone(), bass_in.clone());
    m_bass.process(&mut bl, &mut br);

    let mut m_high = Muffled::new(FS);
    m_high.set_amount(85.0);
    m_high.settle();
    let high_in = sine(4000.0, 24_000);
    let (mut hl, mut hr) = (high_in.clone(), high_in.clone());
    m_high.process(&mut hl, &mut hr);

    let bass_ratio_db = 20.0 * (rms(&bl[12_000..]) / rms(&bass_in[12_000..])).log10();
    let high_ratio_db = 20.0 * (rms(&hl[12_000..]) / rms(&high_in[12_000..])).log10();
    assert!(bass_ratio_db > -1.0, "bass_ratio_db={bass_ratio_db:.2}");
    assert!(high_ratio_db < -20.0, "high_ratio_db={high_ratio_db:.2}");
  }

  #[test]
  fn narrowed_stereo_with_tiled_room_tail_stays_finite_and_decays() {
    let mut m = Muffled::new(FS);
    m.set_amount(85.0);
    m.settle();
    let mut l = vec![0.0f32; 48_000];
    let mut r = vec![0.0f32; 48_000];
    l[0] = 1.0;
    m.process(&mut l, &mut r);

    assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()));
    let early_right = r[200..2000].iter().fold(0f32, |acc, v| acc.max(v.abs()));
    assert!(
      early_right > 1e-4,
      "missing tiled stereo reflections: {early_right}"
    );
    let late_tail = l[40_000..].iter().fold(0f32, |acc, v| acc.max(v.abs()));
    assert!(
      late_tail < early_right * 0.05 && late_tail < 1e-4,
      "tail did not decay: late_tail={late_tail} early_right={early_right}"
    );
    assert_ne!(l[200..2000], r[200..2000]);

    m.reset();
    let mut zero_l = vec![0.0f32; 1024];
    let mut zero_r = vec![0.0f32; 1024];
    m.process(&mut zero_l, &mut zero_r);
    assert!(zero_l.iter().chain(zero_r.iter()).all(|v| *v == 0.0));
  }

  #[test]
  fn block_size_independence_and_non_finite_inputs() {
    let input = sine(300.0, 2000);
    let mut ref_m = Muffled::new(FS);
    ref_m.set_amount(f32::NAN);
    ref_m.settle();
    let (mut l_nan, mut r_nan) = (input.clone(), input.clone());
    ref_m.process(&mut l_nan, &mut r_nan);
    assert_eq!(l_nan, input);

    let mut m1 = Muffled::new(FS);
    m1.set_amount(75.0);
    m1.settle();
    let (mut l1, mut r1) = (input.clone(), input.clone());
    m1.process(&mut l1, &mut r1);

    let mut m2 = Muffled::new(FS);
    m2.set_amount(75.0);
    m2.settle();
    let (mut l2, mut r2) = (input.clone(), input);
    for (cl, cr) in l2.chunks_mut(37).zip(r2.chunks_mut(37)) {
      m2.process_quantum(cl, cr);
    }
    assert_eq!(l1, l2);
    assert_eq!(r1, r2);
  }
}
