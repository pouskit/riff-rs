//! Égaliseur : 3 bandes (`eqLow`, `eqMid`, `eqHigh`) puis 10 bandes.

use super::biquad::{Biquad, BiquadCoeffs};

pub const BAND_FREQS: [f64; 10] = [
  32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
const BAND_Q: f64 = 1.1;
const MID_Q: f64 = 0.9;
const LOW_FREQ: f64 = 200.0;
const MID_FREQ: f64 = 1000.0;
const HIGH_FREQ: f64 = 4000.0;
const MAX_GAIN_DB: f32 = 12.0;
const SMOOTHING_SECS: f64 = 0.02;
const SNAP_DB: f32 = 1e-3;
const FILTERS: usize = 13;
const SHELVES: usize = 3;

/// Gains demandés, en dB. Bornés à ±12 dB à l'application.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EqGains {
  pub low: f32,
  pub mid: f32,
  pub high: f32,
  pub bands: [f32; 10],
}

impl EqGains {
  fn as_array(&self) -> [f32; FILTERS] {
    let mut out = [0.0; FILTERS];
    out[0] = self.low;
    out[1] = self.mid;
    out[2] = self.high;
    out[SHELVES..].copy_from_slice(&self.bands);
    out
  }
}

pub struct Equalizer {
  fs: f64,
  filters: [Biquad; FILTERS],
  current: [f32; FILTERS],
  target: [f32; FILTERS],
  bypassed: bool,
}

impl Equalizer {
  pub fn new(fs: f64) -> Self {
    Self {
      fs,
      filters: std::array::from_fn(|_| Biquad::default()),
      current: [0.0; FILTERS],
      target: [0.0; FILTERS],
      bypassed: true,
    }
  }

  pub fn set_gains(&mut self, gains: &EqGains) {
    self.target = gains.as_array().map(|g| {
      if g.is_finite() {
        g.clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
      } else {
        0.0
      }
    });
    if self.bypassed && self.target != self.current {
      self.bypassed = false;
      self.filters.iter_mut().for_each(Biquad::reset);
    }
  }

  /// Applique les gains demandés sans lissage (rendu hors ligne).
  pub fn settle(&mut self) {
    self.current = self.target;
    for index in 0..FILTERS {
      self.filters[index].set_coeffs(self.coeffs_of(index));
    }
    self.bypassed = self.target == [0.0; FILTERS];
  }

  pub fn is_bypassed(&self) -> bool {
    self.bypassed
  }

  pub fn current_gains(&self) -> [f32; FILTERS] {
    self.current
  }

  /// Applique la chaîne. `tap` reçoit la sortie de `eqHigh` si l'égaliseur est actif.
  pub fn process(
    &mut self,
    left: &mut [f32],
    right: &mut [f32],
    mut tap: impl FnMut(&[f32], &[f32]),
  ) {
    if self.bypassed {
      // Écart volontaire avec Riff : la normalisation a besoin du niveau, même sans égaliseur.
      tap(left, right);
      return;
    }
    self.advance_smoothing(left.len());
    let (shelves, bands) = self.filters.split_at_mut(SHELVES);
    for filter in shelves {
      filter.process(left, right);
    }
    tap(left, right);
    for filter in bands {
      filter.process(left, right);
    }
    self.bypassed = self.target == [0.0; FILTERS] && self.current == [0.0; FILTERS];
  }

  fn advance_smoothing(&mut self, frames: usize) {
    if self.current == self.target {
      return;
    }
    let k = 1.0 - (-(frames as f64) / (self.fs * SMOOTHING_SECS)).exp();
    for i in 0..FILTERS {
      let (current, target) = (self.current[i], self.target[i]);
      if current == target {
        continue;
      }
      let next = f64::from(current) + (f64::from(target) - f64::from(current)) * k;
      let next = next as f32;
      self.current[i] = if (next - target).abs() < SNAP_DB {
        target
      } else {
        next
      };
      self.filters[i].set_coeffs(self.coeffs_of(i));
    }
  }

  fn coeffs_of(&self, index: usize) -> BiquadCoeffs {
    let gain = f64::from(self.current[index]);
    match index {
      0 => BiquadCoeffs::low_shelf(self.fs, LOW_FREQ, gain),
      1 => BiquadCoeffs::peaking(self.fs, MID_FREQ, MID_Q, gain),
      2 => BiquadCoeffs::high_shelf(self.fs, HIGH_FREQ, gain),
      band => BiquadCoeffs::peaking(self.fs, BAND_FREQS[band - SHELVES], BAND_Q, gain),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f64::consts::PI;

  const FS: f64 = 48_000.0;

  fn sine(freq: f64, frames: usize) -> Vec<f32> {
    (0..frames)
      .map(|i| ((2.0 * PI * freq * i as f64 / FS).sin() * 0.1) as f32)
      .collect()
  }

  fn run(eq: &mut Equalizer, left: &mut [f32], right: &mut [f32]) {
    for (l, r) in left.chunks_mut(128).zip(right.chunks_mut(128)) {
      eq.process(l, r, |_, _| {});
    }
  }

  fn isolated_sum_db(gains: &EqGains, freq: f64) -> f64 {
    let mut sum =
      BiquadCoeffs::low_shelf(FS, LOW_FREQ, f64::from(gains.low)).magnitude_db(FS, freq);
    sum += BiquadCoeffs::peaking(FS, MID_FREQ, MID_Q, f64::from(gains.mid)).magnitude_db(FS, freq);
    sum += BiquadCoeffs::high_shelf(FS, HIGH_FREQ, f64::from(gains.high)).magnitude_db(FS, freq);
    for (band, gain) in BAND_FREQS.iter().zip(gains.bands) {
      sum += BiquadCoeffs::peaking(FS, *band, BAND_Q, f64::from(gain)).magnitude_db(FS, freq);
    }
    sum
  }

  #[test]
  fn cumulative_response_matches_sum_of_isolated_filters() {
    let gains = EqGains {
      low: 3.0,
      mid: -2.0,
      high: 4.0,
      bands: [6.0, -3.0, 2.0, 0.0, -4.0, 5.0, 1.0, -6.0, 3.0, 2.0],
    };
    let mut eq = Equalizer::new(FS);
    eq.set_gains(&gains);
    for freq in BAND_FREQS {
      let frames = 96_000;
      let mut l = sine(freq, frames);
      let mut r = l.clone();
      run(&mut eq, &mut l, &mut r);
      // RMS sur 1 s (nombre entier de périodes) : à 16 kHz et 48 kHz, une période vaut
      // 3 échantillons et le pic échantillonné sous-estime l'amplitude de 1,25 dB.
      let energy: f64 = l[frames / 2..].iter().map(|v| f64::from(*v).powi(2)).sum();
      let rms = (energy / (frames / 2) as f64).sqrt();
      let got = 20.0 * (rms * std::f64::consts::SQRT_2 / 0.1).log10();
      let want = isolated_sum_db(&gains, freq);
      assert!((got - want).abs() < 0.5, "f={freq} got={got} want={want}");
    }
  }

  #[test]
  fn gains_are_clamped_to_twelve_db() {
    let mut eq = Equalizer::new(FS);
    eq.set_gains(&EqGains {
      low: 40.0,
      mid: -40.0,
      high: 12.0,
      bands: [99.0; 10],
    });
    let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
    run(&mut eq, &mut l, &mut r);
    let g = eq.current_gains();
    assert_eq!(g[0], 12.0);
    assert_eq!(g[1], -12.0);
    assert_eq!(g[3], 12.0);
  }

  #[test]
  fn all_zero_gains_bypass_the_chain() {
    let mut eq = Equalizer::new(FS);
    let input = sine(440.0, 512);
    let (mut l, mut r) = (input.clone(), input.clone());
    let mut tapped = false;
    eq.process(&mut l, &mut r, |_, _| tapped = true);
    assert!(eq.is_bypassed());
    assert!(
      tapped,
      "le niveau reste mesuré, écart volontaire avec Riff (décision 1)"
    );
    assert_eq!(l, input);
  }

  #[test]
  fn non_zero_gain_activates_the_chain_and_the_level_tap() {
    let mut eq = Equalizer::new(FS);
    eq.set_gains(&EqGains {
      bands: [0.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0],
      ..EqGains::default()
    });
    assert!(!eq.is_bypassed());
    let (mut l, mut r) = (sine(1000.0, 128), sine(1000.0, 128));
    let mut tapped = 0;
    eq.process(&mut l, &mut r, |a, _| tapped = a.len());
    assert_eq!(tapped, 128);
  }

  #[test]
  fn bypass_returns_once_smoothing_has_reached_zero() {
    let mut eq = Equalizer::new(FS);
    eq.set_gains(&EqGains {
      low: 6.0,
      ..EqGains::default()
    });
    let (mut l, mut r) = (vec![0.0; 48_000], vec![0.0; 48_000]);
    run(&mut eq, &mut l, &mut r);
    eq.set_gains(&EqGains::default());
    assert!(!eq.is_bypassed(), "le retour à zéro reste lissé");
    run(&mut eq, &mut l, &mut r);
    assert!(eq.is_bypassed());
  }

  #[test]
  fn settle_applies_the_gains_immediately() {
    let mut eq = Equalizer::new(FS);
    eq.set_gains(&EqGains {
      mid: 6.0,
      ..EqGains::default()
    });
    eq.settle();
    assert_eq!(eq.current_gains()[1], 6.0);
    let mut l = sine(1000.0, 96_000);
    let mut r = l.clone();
    eq.process(&mut l[..128], &mut r[..128], |_, _| {});
    assert_eq!(eq.current_gains()[1], 6.0);
    eq.set_gains(&EqGains::default());
    eq.settle();
    assert!(eq.is_bypassed());
  }

  #[test]
  fn gain_change_follows_the_time_constant() {
    let mut eq = Equalizer::new(FS);
    eq.set_gains(&EqGains {
      bands: [10.0; 10],
      ..EqGains::default()
    });
    let frames = (SMOOTHING_SECS * FS) as usize;
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    run(&mut eq, &mut l, &mut r);
    let got = f64::from(eq.current_gains()[3]);
    let want = 10.0 * (1.0 - (-1f64).exp());
    assert!((got - want).abs() < 0.2, "got={got} want={want}");
  }
}
