//! Compresseur : portage de `DynamicsCompressor` de Chromium, sans le calcul de `reduction`.

use std::f32::consts::FRAC_PI_2;

const PRE_DELAY_SECONDS: f32 = 0.006;
const MAX_PRE_DELAY_FRAMES: usize = 1024;
const PRE_DELAY_MASK: usize = MAX_PRE_DELAY_FRAMES - 1;
const DIVISION_FRAMES: usize = 32;
const SAT_RELEASE_SECONDS: f32 = 0.0025;
const MIN_ATTACK_SECONDS: f32 = 0.001;
const MAKEUP_EXPONENT: f32 = 0.6;
const KNEE_SEARCH_STEPS: usize = 15;

// Comme en C++ : littéraux en double, zones en float élargies, résultat réduit en float.
const RELEASE_ZONE_1: f64 = 0.09f32 as f64;
const RELEASE_ZONE_2: f64 = 0.16f32 as f64;
const RELEASE_ZONE_3: f64 = 0.42f32 as f64;
const RELEASE_ZONE_4: f64 = 0.98f32 as f64;

const A_BASE: f32 = (0.999_999_999_999_999_8 * RELEASE_ZONE_1
  + 1.843_221_968_432_392_3e-16 * RELEASE_ZONE_2
  - 1.937_339_435_167_642_3e-16 * RELEASE_ZONE_3
  + 8.824_516_011_816_245e-18 * RELEASE_ZONE_4) as f32;
const B_BASE: f32 = (-1.578_832_035_284_588_8 * RELEASE_ZONE_1
  + 2.330_583_703_207_428_6 * RELEASE_ZONE_2
  - 0.914_119_420_484_042_9 * RELEASE_ZONE_3
  + 0.162_367_752_561_203_2 * RELEASE_ZONE_4) as f32;
const C_BASE: f32 = (0.533_414_286_910_642_4 * RELEASE_ZONE_1
  - 1.272_736_789_213_631 * RELEASE_ZONE_2
  + 0.925_885_604_220_751_2 * RELEASE_ZONE_3
  - 0.186_563_101_917_762_26 * RELEASE_ZONE_4) as f32;
const D_BASE: f32 = (0.087_834_631_382_072_34 * RELEASE_ZONE_1
  - 0.169_416_296_792_562_2 * RELEASE_ZONE_2
  + 0.085_880_579_515_952_72 * RELEASE_ZONE_3
  - 0.004_298_914_105_462_83 * RELEASE_ZONE_4) as f32;
const E_BASE: f32 = (-0.042_416_883_008_123_074 * RELEASE_ZONE_1
  + 0.111_569_382_798_760_2 * RELEASE_ZONE_2
  - 0.097_646_763_252_658_72 * RELEASE_ZONE_3
  + 0.028_494_263_462_021_576 * RELEASE_ZONE_4) as f32;

#[inline]
fn linear_to_db(linear: f32) -> f32 {
  20.0 * linear.log10()
}

#[inline]
fn db_to_linear(db: f32) -> f32 {
  10f32.powf(0.05 * db)
}

#[inline]
fn ensure_finite(value: f32, default: f32) -> f32 {
  if value.is_finite() { value } else { default }
}

#[inline]
fn flush_denormal(value: f32) -> f32 {
  if value.abs() < f32::MIN_POSITIVE {
    0.0
  } else {
    value
  }
}

/// Réglages `threshold` (dB), `knee` (dB), `ratio`, `attack` (s), `release` (s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompressorParams {
  pub threshold: f32,
  pub knee: f32,
  pub ratio: f32,
  pub attack: f32,
  pub release: f32,
}

/// Courbe statique, recalculée seulement quand seuil, genou ou ratio changent.
#[derive(Clone, Copy, Debug)]
struct StaticCurve {
  linear_threshold: f32,
  slope: f32,
  knee_threshold: f32,
  db_knee_threshold: f32,
  db_y_knee_threshold: f32,
  k: f32,
  post_gain: f32,
}

impl StaticCurve {
  fn new(params: &CompressorParams) -> Self {
    let linear_threshold = db_to_linear(params.threshold);
    let slope = 1.0 / params.ratio;
    let db_knee_threshold = params.threshold + params.knee;
    let knee_threshold = db_to_linear(db_knee_threshold);
    let mut curve = Self {
      linear_threshold,
      slope,
      knee_threshold,
      db_knee_threshold,
      db_y_knee_threshold: 0.0,
      k: 0.0,
      post_gain: 1.0,
    };
    curve.k = curve.k_at_slope(params, slope);
    curve.db_y_knee_threshold = linear_to_db(curve.knee_curve(knee_threshold, curve.k));
    curve.post_gain = (1.0 / curve.saturate(1.0)).powf(MAKEUP_EXPONENT);
    curve
  }

  #[inline]
  fn knee_curve(&self, x: f32, k: f32) -> f32 {
    if x < self.linear_threshold {
      return x;
    }
    self.linear_threshold + (1.0 - (-k * (x - self.linear_threshold)).exp()) / k
  }

  #[inline]
  fn saturate(&self, x: f32) -> f32 {
    if x < self.knee_threshold {
      return self.knee_curve(x, self.k);
    }
    let db_y = self.db_y_knee_threshold + self.slope * (linear_to_db(x) - self.db_knee_threshold);
    db_to_linear(db_y)
  }

  fn k_at_slope(&self, params: &CompressorParams, desired_slope: f32) -> f32 {
    let db_x = params.threshold + params.knee;
    let x = db_to_linear(db_x);
    let mut x2 = 1.0;
    let mut db_x2 = 0.0;
    if x >= self.linear_threshold {
      x2 = x * 1.001;
      db_x2 = linear_to_db(x2);
    }
    let (mut min_k, mut max_k, mut k) = (0.1f32, 10_000f32, 5f32);
    let mut slope = 1.0;
    for _ in 0..KNEE_SEARCH_STEPS {
      if x >= self.linear_threshold {
        let db_y = linear_to_db(self.knee_curve(x, k));
        let db_y2 = linear_to_db(self.knee_curve(x2, k));
        slope = (db_y2 - db_y) / (db_x2 - db_x);
      }
      if slope < desired_slope {
        max_k = k;
      } else {
        min_k = k;
      }
      k = (min_k * max_k).sqrt();
    }
    k
  }
}

pub struct Compressor {
  fs: f32,
  params: CompressorParams,
  curve: StaticCurve,
  pre_delay: [Box<[f32; MAX_PRE_DELAY_FRAMES]>; 2],
  read_index: usize,
  write_index: usize,
  pre_delay_frames: usize,
  detector_average: f32,
  compressor_gain: f32,
  max_attack_diff_db: f32,
}

impl Compressor {
  pub fn new(fs: f32, params: CompressorParams) -> Self {
    let pre_delay_frames = ((PRE_DELAY_SECONDS * fs) as usize).min(MAX_PRE_DELAY_FRAMES - 1);
    Self {
      fs,
      params,
      curve: StaticCurve::new(&params),
      pre_delay: [
        Box::new([0.0; MAX_PRE_DELAY_FRAMES]),
        Box::new([0.0; MAX_PRE_DELAY_FRAMES]),
      ],
      read_index: 0,
      write_index: pre_delay_frames,
      pre_delay_frames,
      detector_average: 0.0,
      compressor_gain: 1.0,
      max_attack_diff_db: -1.0,
    }
  }

  pub fn set_params(&mut self, params: CompressorParams) {
    let curve_changed = params.threshold != self.params.threshold
      || params.knee != self.params.knee
      || params.ratio != self.params.ratio;
    self.params = params;
    if curve_changed {
      self.curve = StaticCurve::new(&params);
    }
  }

  pub fn reset(&mut self) {
    self
      .pre_delay
      .iter_mut()
      .for_each(|buffer| buffer.fill(0.0));
    self.read_index = 0;
    self.write_index = self.pre_delay_frames;
    self.detector_average = 0.0;
    self.compressor_gain = 1.0;
    self.max_attack_diff_db = -1.0;
  }

  /// Courbe statique avec gain de compensation : niveau de sortie stabilisé pour une crête `input`.
  pub fn static_output(&self, input: f32) -> f32 {
    self.curve.saturate(input) * self.curve.post_gain
  }

  pub fn latency_frames(&self) -> usize {
    self.pre_delay_frames
  }

  /// Taux d'enveloppe du bloc courant : `< 1` en attaque, `> 1` en relâchement.
  fn envelope_rate(&mut self, scaled_desired: f32, attack_frames: f32, release_frames: f32) -> f32 {
    let releasing = scaled_desired > self.compressor_gain;
    let diff_db = if scaled_desired == 0.0 {
      if releasing { -1.0 } else { 1.0 }
    } else {
      linear_to_db(self.compressor_gain / scaled_desired)
    };
    if releasing {
      self.max_attack_diff_db = -1.0;
      let diff_db = ensure_finite(diff_db, -1.0);
      let x = 0.25 * (diff_db.clamp(-12.0, 0.0) + 12.0);
      let (x2, x3) = (x * x, x * x * x);
      let x4 = x2 * x2;
      let calc_release =
        release_frames * (A_BASE + B_BASE * x + C_BASE * x2 + D_BASE * x3 + E_BASE * x4);
      db_to_linear(5.0 / calc_release)
    } else {
      let diff_db = ensure_finite(diff_db, 1.0);
      if self.max_attack_diff_db == -1.0 || self.max_attack_diff_db < diff_db {
        self.max_attack_diff_db = diff_db;
      }
      let effective = self.max_attack_diff_db.max(0.5);
      1.0 - (0.25 / effective).powf(1.0 / attack_frames)
    }
  }

  pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
    let attack_frames = self.params.attack.max(MIN_ATTACK_SECONDS) * self.fs;
    let release_frames = self.fs * self.params.release;
    let sat_release_frames = SAT_RELEASE_SECONDS * self.fs;
    let curve = self.curve;
    let frames = left.len().min(right.len());
    let mut frame = 0;
    while frame < frames {
      self.detector_average = ensure_finite(self.detector_average, 1.0);
      let scaled_desired = self.detector_average.asin() / FRAC_PI_2;
      let envelope_rate = self.envelope_rate(scaled_desired, attack_frames, release_frames);

      let mut read = self.read_index;
      let mut write = self.write_index;
      let mut detector = self.detector_average;
      let mut gain = self.compressor_gain;
      let end = (frame + DIVISION_FRAMES).min(frames);
      let [delay_left, delay_right] = &mut self.pre_delay;
      for (l, r) in left[frame..end].iter_mut().zip(&mut right[frame..end]) {
        delay_left[write] = *l;
        delay_right[write] = *r;
        let input = l.abs().max(r.abs());
        let shaped = curve.saturate(input);
        let attenuation = if input <= 0.0001 { 1.0 } else { shaped / input };
        let db_attenuation = (-linear_to_db(attenuation)).max(2.0);
        let rate = if attenuation > detector {
          db_to_linear(db_attenuation / sat_release_frames) - 1.0
        } else {
          1.0
        };
        detector += (attenuation - detector) * rate;
        detector = ensure_finite(detector.min(1.0), 1.0);

        if envelope_rate < 1.0 {
          gain += (scaled_desired - gain) * envelope_rate;
        } else {
          gain = (gain * envelope_rate).min(1.0);
        }
        let total_gain = curve.post_gain * (FRAC_PI_2 * gain).sin();
        *l = delay_left[read] * total_gain;
        *r = delay_right[read] * total_gain;
        read = (read + 1) & PRE_DELAY_MASK;
        write = (write + 1) & PRE_DELAY_MASK;
      }
      self.read_index = read;
      self.write_index = write;
      self.detector_average = flush_denormal(detector);
      self.compressor_gain = flush_denormal(gain);
      frame = end;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::f64::consts::TAU;

  const FS: f32 = 48_000.0;
  const NORM_LIMITER: CompressorParams = CompressorParams {
    threshold: -2.0,
    knee: 0.0,
    ratio: 20.0,
    attack: 0.003,
    release: 0.1,
  };
  const HQ_COMPRESSOR: CompressorParams = CompressorParams {
    threshold: -12.0,
    knee: 10.0,
    ratio: 1.5,
    attack: 0.01,
    release: 0.1,
  };
  const HQ_LIMITER: CompressorParams = CompressorParams {
    threshold: -1.0,
    knee: 0.0,
    ratio: 20.0,
    attack: 0.005,
    release: 0.05,
  };
  const CHROMIUM_DEFAULT: CompressorParams = CompressorParams {
    threshold: -24.0,
    knee: 30.0,
    ratio: 12.0,
    attack: 0.003,
    release: 0.25,
  };

  fn db(linear: f64) -> f64 {
    20.0 * linear.log10()
  }

  fn sine(amplitude: f64, frames: usize) -> Vec<f32> {
    (0..frames)
      .map(|i| (amplitude * (TAU * 1000.0 * i as f64 / f64::from(FS)).sin()) as f32)
      .collect()
  }

  fn run(comp: &mut Compressor, signal: &[f32]) -> Vec<f32> {
    let mut left = signal.to_vec();
    let mut right = signal.to_vec();
    for (l, r) in left.chunks_mut(128).zip(right.chunks_mut(128)) {
      comp.process(l, r);
    }
    assert_eq!(left, right);
    left
  }

  fn peak(signal: &[f32]) -> f64 {
    signal.iter().fold(0f32, |m, v| m.max(v.abs())).into()
  }

  /// (a) Courbe statique : sinus de 1 kHz, à ±1 dB de la courbe avec genou et compensation.
  #[test]
  fn static_curve_of_a_1khz_sine_matches_the_knee_formula() {
    for params in [NORM_LIMITER, HQ_COMPRESSOR, HQ_LIMITER, CHROMIUM_DEFAULT] {
      for input_db in [-40.0f64, -24.0, -12.0, -6.0, -2.0, 0.0, 6.0] {
        let amplitude = 10f64.powf(input_db / 20.0);
        let mut comp = Compressor::new(FS, params);
        let want = db(f64::from(comp.static_output(amplitude as f32)));
        let out = run(&mut comp, &sine(amplitude, 48_000));
        let got = db(peak(&out[24_000..]));
        assert!(
          (got - want).abs() < 1.0,
          "{params:?} entrée {input_db} dB : obtenu {got:.2}, attendu {want:.2}"
        );
      }
    }
  }

  #[test]
  fn makeup_gain_is_the_chromium_value() {
    let comp = Compressor::new(FS, NORM_LIMITER);
    // Sous le seuil, seule la compensation agit : (1/Saturate(1))^0,6 avec Saturate(1) = −1,9 dB.
    let got = db(f64::from(comp.static_output(0.1)) / 0.1);
    assert!((got - 1.14).abs() < 0.01, "compensation {got}");
  }

  /// (b) Attaque : après `attack`, l'écart de gain est réduit au rapport `0,25/écart_dB`
  /// (définition de Chromium). Mesuré sur le gain avant la déformation en sinus.
  #[test]
  fn attack_time_matches_the_requested_constant() {
    for params in [NORM_LIMITER, HQ_COMPRESSOR, HQ_LIMITER] {
      let quiet = vec![0.05f32; 48_000];
      let loud = vec![2.0f32; 32];
      let settled = {
        let mut comp = Compressor::new(FS, params);
        run(&mut comp, &quiet);
        run(&mut comp, &vec![2.0f32; 48_000]);
        f64::from(comp.compressor_gain)
      };
      let mut comp = Compressor::new(FS, params);
      run(&mut comp, &quiet);
      let start = f64::from(comp.compressor_gain);
      let span_db = db(start / settled).max(0.5);
      let target = settled + (start - settled) * (0.25 / span_db);
      // Le détecteur ne réagit qu'au bloc suivant de 32 échantillons : ce retard est retiré,
      // la constante d'attaque est mesurée à partir du début de la descente du gain.
      let mut frames = 0.0;
      let mut previous = start;
      loop {
        run(&mut comp, &loud);
        let gain = f64::from(comp.compressor_gain);
        if gain <= target {
          frames += loud.len() as f64 * (previous - target) / (previous - gain);
          break;
        }
        frames += loud.len() as f64;
        previous = gain;
        assert!(frames < 48_000.0, "{params:?} : cible non atteinte");
      }
      let seconds = (frames - loud.len() as f64) / f64::from(FS);
      let want = f64::from(params.attack);
      assert!(
        (seconds - want).abs() <= want * 0.25,
        "{params:?} : attaque mesurée {seconds:.5} s (écart {span_db:.2} dB)"
      );
    }
  }

  /// (b) Relâchement : près de la fin, la pente vaut 5 dB pour `0,98·release`.
  #[test]
  fn release_time_matches_the_requested_constant() {
    for params in [NORM_LIMITER, HQ_COMPRESSOR, HQ_LIMITER] {
      let mut comp = Compressor::new(FS, params);
      run(&mut comp, &sine(2.0, 48_000));
      let quiet = sine(1e-3, 32);
      let mut last = f64::from(comp.compressor_gain);
      let mut slopes = Vec::new();
      for _ in 0..3000 {
        run(&mut comp, &quiet);
        let gain = f64::from(comp.compressor_gain);
        // Fin du relâchement : à moins de 0,5 dB de la cible, tant que le gain monte encore.
        if gain < 0.999 && gain > 0.95 && gain > last {
          slopes.push(db(gain / last) / (32.0 / f64::from(FS)));
        }
        last = gain;
      }
      assert!(!slopes.is_empty(), "{params:?} : aucun point de mesure");
      let mean = slopes.iter().sum::<f64>() / slopes.len() as f64;
      let measured_release = 5.0 / mean / 0.98;
      let want = f64::from(params.release);
      assert!(
        (measured_release - want).abs() <= want * 0.25,
        "{params:?} : relâchement mesuré {measured_release:.4} s"
      );
    }
  }

  /// (c) Limiteur de sortie. La spec demandait −1,5 dBFS, impossible avec la compensation de Chromium
  /// que l'utilisateur a choisi de garder. Calcul : −0,46 dBFS ; mesure sur sinus : −0,26 dBFS, car le
  /// gain ondule à chaque période. Plafond retenu : −0,2 dBFS.
  #[test]
  fn output_limiter_never_exceeds_the_ceiling_at_plus_six_dbfs() {
    let ceiling = 10f64.powf(-0.2 / 20.0);
    let amplitude = 10f64.powf(6.0 / 20.0);
    let square: Vec<f32> = sine(amplitude, 96_000)
      .iter()
      .map(|v| {
        if *v >= 0.0 {
          amplitude as f32
        } else {
          -amplitude as f32
        }
      })
      .collect();
    for (name, signal) in [("sinus", sine(amplitude, 96_000)), ("carré", square)] {
      let mut comp = Compressor::new(FS, NORM_LIMITER);
      let out = run(&mut comp, &signal);
      let got = peak(&out);
      assert!(got <= ceiling, "{name} : pic {:.3} dBFS", db(got));
    }
  }

  /// (d) Pas de NaN.
  #[test]
  fn silence_and_full_scale_stay_finite() {
    for params in [NORM_LIMITER, HQ_COMPRESSOR, HQ_LIMITER, CHROMIUM_DEFAULT] {
      let mut comp = Compressor::new(FS, params);
      for signal in [
        vec![0.0; 48_000],
        sine(1.0, 48_000),
        vec![1.0; 48_000],
        vec![0.0; 48_000],
      ] {
        let out = run(&mut comp, &signal);
        assert!(out.iter().all(|v| v.is_finite()), "{params:?}");
      }
    }
  }

  #[test]
  fn output_is_delayed_by_the_pre_delay() {
    let mut comp = Compressor::new(FS, NORM_LIMITER);
    assert_eq!(comp.latency_frames(), 288);
    run(&mut comp, &vec![0.0f32; 48_000]);
    let mut signal = vec![0.0f32; 1024];
    signal[0] = 0.1;
    let out = run(&mut comp, &signal);
    assert_eq!(out[..288].iter().filter(|v| v.abs() > 1e-6).count(), 0);
    assert!(out[288].abs() > 0.05);
  }

  #[test]
  fn reset_returns_to_the_initial_state() {
    let signal = sine(2.0, 4096);
    let mut comp = Compressor::new(FS, NORM_LIMITER);
    let first = run(&mut comp, &signal);
    comp.reset();
    assert_eq!(first, run(&mut comp, &signal));
  }
}
