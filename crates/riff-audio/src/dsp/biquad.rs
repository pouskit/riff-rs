//! Filtres biquad de la spécification Web Audio (section BiquadFilterNode).

use std::f64::consts::PI;

/// Coefficients normalisés (`a0 = 1`), calculés en `f64`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiquadCoeffs {
  pub b0: f64,
  pub b1: f64,
  pub b2: f64,
  pub a1: f64,
  pub a2: f64,
}

impl BiquadCoeffs {
  pub const IDENTITY: Self = Self {
    b0: 1.0,
    b1: 0.0,
    b2: 0.0,
    a1: 0.0,
    a2: 0.0,
  };

  pub fn peaking(fs: f64, f0: f64, q: f64, gain_db: f64) -> Self {
    let freq = normalized(fs, f0);
    if freq <= 0.0 || freq >= 1.0 {
      return Self::IDENTITY;
    }
    let a = 10f64.powf(gain_db / 40.0);
    if q <= 0.0 {
      return Self::gain(a * a);
    }
    let w0 = PI * freq;
    let alpha = w0.sin() / (2.0 * q);
    let k = w0.cos();
    Self::normalize(
      1.0 + alpha * a,
      -2.0 * k,
      1.0 - alpha * a,
      1.0 + alpha / a,
      -2.0 * k,
      1.0 - alpha / a,
    )
  }

  pub fn low_shelf(fs: f64, f0: f64, gain_db: f64) -> Self {
    let freq = normalized(fs, f0);
    let a = 10f64.powf(gain_db / 40.0);
    if freq >= 1.0 {
      return Self::gain(a * a);
    }
    if freq <= 0.0 {
      return Self::IDENTITY;
    }
    let (k, k2) = shelf_terms(freq, a);
    let (ap, am) = (a + 1.0, a - 1.0);
    Self::normalize(
      a * (ap - am * k + k2),
      2.0 * a * (am - ap * k),
      a * (ap - am * k - k2),
      ap + am * k + k2,
      -2.0 * (am + ap * k),
      ap + am * k - k2,
    )
  }

  pub fn high_shelf(fs: f64, f0: f64, gain_db: f64) -> Self {
    let freq = normalized(fs, f0);
    let a = 10f64.powf(gain_db / 40.0);
    if freq >= 1.0 {
      return Self::IDENTITY;
    }
    if freq <= 0.0 {
      return Self::gain(a * a);
    }
    let (k, k2) = shelf_terms(freq, a);
    let (ap, am) = (a + 1.0, a - 1.0);
    Self::normalize(
      a * (ap + am * k + k2),
      -2.0 * a * (am + ap * k),
      a * (ap + am * k - k2),
      ap - am * k + k2,
      2.0 * (am - ap * k),
      ap - am * k - k2,
    )
  }

  pub fn low_pass(fs: f64, f0: f64, q: f64) -> Self {
    let freq = normalized(fs, f0);
    if freq >= 1.0 {
      return Self::IDENTITY;
    }
    if freq <= 0.0 {
      return Self::gain(0.0);
    }
    let w0 = PI * freq;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * q.max(1e-6));
    let b1 = 1.0 - cos_w0;
    let b0 = b1 * 0.5;
    Self::normalize(b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
  }

  const fn gain(g: f64) -> Self {
    Self {
      b0: g,
      b1: 0.0,
      b2: 0.0,
      a1: 0.0,
      a2: 0.0,
    }
  }

  fn normalize(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
    let inv = 1.0 / a0;
    Self {
      b0: b0 * inv,
      b1: b1 * inv,
      b2: b2 * inv,
      a1: a1 * inv,
      a2: a2 * inv,
    }
  }

  /// Gain en dB à `freq`, par évaluation de `H(e^jω)`.
  pub fn magnitude_db(&self, fs: f64, freq: f64) -> f64 {
    let w = 2.0 * PI * freq / fs;
    let (c1, s1) = (w.cos(), w.sin());
    let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
    let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
    let num_im = -(self.b1 * s1 + self.b2 * s2);
    let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
    let den_im = -(self.a1 * s1 + self.a2 * s2);
    let num = num_re * num_re + num_im * num_im;
    let den = den_re * den_re + den_im * den_im;
    10.0 * (num / den).log10()
  }
}

fn normalized(fs: f64, f0: f64) -> f64 {
  (f0 / (fs / 2.0)).clamp(0.0, 1.0)
}

/// Termes communs des shelfs, avec `S = 1` : `(cos ω0, 2·√A·αS)`.
fn shelf_terms(freq: f64, a: f64) -> (f64, f64) {
  let w0 = PI * freq;
  let alpha = 0.5 * w0.sin() * 2f64.sqrt();
  (w0.cos(), 2.0 * a.sqrt() * alpha)
}

#[inline]
fn flush_denormal(v: f32) -> f32 {
  if v.abs() < 1e-20 { 0.0 } else { v }
}

/// Filtre stéréo en forme directe transposée II, état en `f32`.
#[derive(Clone, Debug)]
pub struct Biquad {
  b0: f32,
  b1: f32,
  b2: f32,
  a1: f32,
  a2: f32,
  z: [[f32; 2]; 2],
}

impl Default for Biquad {
  fn default() -> Self {
    Self::new(BiquadCoeffs::IDENTITY)
  }
}

impl Biquad {
  pub fn new(c: BiquadCoeffs) -> Self {
    let mut filter = Self {
      b0: 1.0,
      b1: 0.0,
      b2: 0.0,
      a1: 0.0,
      a2: 0.0,
      z: [[0.0; 2]; 2],
    };
    filter.set_coeffs(c);
    filter
  }

  pub fn set_coeffs(&mut self, c: BiquadCoeffs) {
    self.b0 = c.b0 as f32;
    self.b1 = c.b1 as f32;
    self.b2 = c.b2 as f32;
    self.a1 = c.a1 as f32;
    self.a2 = c.a2 as f32;
  }

  pub fn reset(&mut self) {
    self.z = [[0.0; 2]; 2];
  }

  pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
    let (b0, b1, b2, a1, a2) = (self.b0, self.b1, self.b2, self.a1, self.a2);
    for (samples, z) in [left, right].into_iter().zip(&mut self.z) {
      let [mut z1, mut z2] = *z;
      for sample in samples.iter_mut() {
        let x = *sample;
        let y = b0 * x + z1;
        z1 = b1 * x - a1 * y + z2;
        z2 = b2 * x - a2 * y;
        *sample = y;
      }
      *z = [flush_denormal(z1), flush_denormal(z2)];
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const RATES: [f64; 2] = [44_100.0, 48_000.0];
  const GAINS: [f64; 6] = [-12.0, -6.0, -1.5, 1.5, 6.0, 12.0];

  #[test]
  fn peaking_center_gain_matches_request() {
    for fs in RATES {
      for f0 in [32.0, 1000.0, 2500.0, 16000.0] {
        for q in [0.9, 1.1, 1.2] {
          for g in GAINS {
            let got = BiquadCoeffs::peaking(fs, f0, q, g).magnitude_db(fs, f0);
            assert!(
              (got - g).abs() < 0.01,
              "fs={fs} f0={f0} q={q} g={g} got={got}"
            );
          }
        }
      }
    }
  }

  #[test]
  fn low_shelf_reaches_gain_towards_dc() {
    for fs in RATES {
      for f0 in [80.0, 200.0] {
        for g in GAINS {
          let c = BiquadCoeffs::low_shelf(fs, f0, g);
          let low = c.magnitude_db(fs, 1.0);
          let high = c.magnitude_db(fs, fs * 0.49);
          assert!((low - g).abs() < 0.1, "f0={f0} g={g} low={low}");
          assert!(high.abs() < 0.1, "f0={f0} g={g} high={high}");
        }
      }
    }
  }

  #[test]
  fn high_shelf_reaches_gain_towards_nyquist() {
    for fs in RATES {
      for f0 in [4000.0, 9000.0] {
        for g in GAINS {
          let c = BiquadCoeffs::high_shelf(fs, f0, g);
          let low = c.magnitude_db(fs, 10.0);
          let high = c.magnitude_db(fs, fs * 0.4999);
          assert!(low.abs() < 0.1, "f0={f0} g={g} low={low}");
          assert!((high - g).abs() < 0.1, "f0={f0} g={g} high={high}");
        }
      }
    }
  }

  #[test]
  fn zero_gain_is_identity() {
    let fs = 48_000.0;
    let all = [
      BiquadCoeffs::peaking(fs, 1000.0, 1.1, 0.0),
      BiquadCoeffs::low_shelf(fs, 200.0, 0.0),
      BiquadCoeffs::high_shelf(fs, 4000.0, 0.0),
    ];
    for coeffs in all {
      let mut filter = Biquad::new(coeffs);
      let input: Vec<f32> = (0..256)
        .map(|i| ((i * 37 % 101) as f32 / 50.0) - 1.0)
        .collect();
      let (mut l, mut r) = (input.clone(), input.clone());
      filter.process(&mut l, &mut r);
      for (a, b) in input.iter().zip(&l) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
      }
      assert_eq!(l, r);
    }
  }

  #[test]
  fn nyquist_edge_cases_follow_chromium() {
    let fs = 22_050.0;
    let a2 = 10f64.powf(6.0 / 20.0);
    let low = BiquadCoeffs::low_shelf(fs, 16_000.0, 6.0);
    assert!((low.b0 - a2).abs() < 1e-9 && low.b1 == 0.0 && low.a1 == 0.0);
    assert_eq!(
      BiquadCoeffs::high_shelf(fs, 16_000.0, 6.0),
      BiquadCoeffs::IDENTITY
    );
    assert_eq!(
      BiquadCoeffs::peaking(fs, 16_000.0, 1.1, 6.0),
      BiquadCoeffs::IDENTITY
    );
  }

  #[test]
  fn processing_matches_frequency_response() {
    let fs = 48_000.0;
    let coeffs = BiquadCoeffs::peaking(fs, 1000.0, 1.1, 6.0);
    let mut filter = Biquad::new(coeffs);
    let n = 48_000;
    let mut l: Vec<f32> = (0..n)
      .map(|i| (2.0 * PI * 1000.0 * i as f64 / fs).sin() as f32 * 0.25)
      .collect();
    let mut r = l.clone();
    for (cl, cr) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
      filter.process(cl, cr);
    }
    let peak = l[24_000..].iter().fold(0f32, |m, v| m.max(v.abs()));
    let got = 20.0 * f64::from(peak / 0.25).log10();
    assert!((got - 6.0).abs() < 0.05, "got={got}");
  }

  #[test]
  fn channels_have_independent_state() {
    let mut filter = Biquad::new(BiquadCoeffs::peaking(48_000.0, 1000.0, 1.1, 6.0));
    let mut l = vec![1.0f32; 16];
    let mut r = vec![0.0f32; 16];
    filter.process(&mut l, &mut r);
    assert!(r.iter().all(|v| *v == 0.0));
    assert!(l[1] != 1.0);
  }

  #[test]
  fn low_pass_matches_butterworth_response() {
    let q = std::f64::consts::FRAC_1_SQRT_2;
    for fs in RATES {
      let c = BiquadCoeffs::low_pass(fs, 1000.0, q);
      let dc = c.magnitude_db(fs, 1.0);
      let cutoff = c.magnitude_db(fs, 1000.0);
      let high = c.magnitude_db(fs, 8000.0);
      assert!(dc.abs() < 0.05, "fs={fs} dc={dc}");
      assert!((cutoff + 3.0103).abs() < 0.05, "fs={fs} cutoff={cutoff}");
      assert!(high < -20.0, "fs={fs} high={high}");
    }
  }
}
