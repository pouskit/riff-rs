//! Banc d'essai de l'étirement temporel : `wsola` contre `timestretch`.
//!
//! `cargo run --release -p riff-audio --example tempo_bench [dossier]` affiche les mesures et écrit
//! des fichiers WAV (16 bits) pour l'écoute. Les signaux sont synthétiques.

use std::f64::consts::TAU;
use std::fs;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rustfft::{FftPlanner, num_complex::Complex};
use timestretch::{Parameters, SessionConfig, TempoRate, render};
use wsola::{Config, TimeStretch};

const RATE: u32 = 48_000;
const SECONDS: f64 = 12.0;
const CHORD: [f64; 3] = [220.0, 277.18, 329.63];
const HARMONICS: [f64; 4] = [1.0, 2.0, 3.0, 4.0];
const PARTIAL_HALF_WIDTH_HZ: f64 = 3.0;

type Engine = (&'static str, fn(&[f32], f64) -> Vec<f32>);

fn wsola_with(input: &[f32], tempo: f64, config: Config) -> Vec<f32> {
  let (Some(rate), Some(channels)) = (NonZeroU32::new(RATE), NonZeroU16::new(2)) else {
    unreachable!("constantes non nulles");
  };
  let mut stretch = TimeStretch::with_config(rate, channels, config).expect("configuration wsola");
  stretch.set_tempo(tempo as f32);
  let mut tmp = vec![0.0; stretch.min_output_buff_size() * 8];
  let mut out = Vec::new();
  for block in input.chunks(4096) {
    stretch.push(block);
    while let Ok(written) = stretch.pull(&mut tmp) {
      if written == 0 {
        break;
      }
      out.extend_from_slice(&tmp[..written]);
    }
  }
  stretch.set_draining();
  while let Ok(written) = stretch.pull(&mut tmp) {
    if written == 0 {
      break;
    }
    out.extend_from_slice(&tmp[..written]);
  }
  out
}

fn wsola_chromium(input: &[f32], tempo: f64) -> Vec<f32> {
  wsola_with(
    input,
    tempo,
    Config {
      hop_ms: 10.0,
      search_ms: 15.0,
    },
  )
}

fn wsola_default(input: &[f32], tempo: f64) -> Vec<f32> {
  wsola_with(input, tempo, Config::default())
}

fn timestretch_keylock(input: &[f32], tempo: f64) -> Vec<f32> {
  let rate = TempoRate::new(tempo).expect("tempo valide");
  let config = SessionConfig {
    source_sample_rate: RATE,
    output_sample_rate: RATE,
    channels: 2,
    parameters: Parameters::keylock(rate),
    ..SessionConfig::default()
  };
  render(input, config).expect("rendu timestretch").samples
}

fn chord_sample(t: f64) -> f64 {
  let mut value = 0.0;
  for base in CHORD {
    for (i, harmonic) in HARMONICS.iter().enumerate() {
      value += (TAU * base * harmonic * t).sin() / (i as f64 + 1.0);
    }
  }
  value * 0.08
}

fn stereo(frames: usize, mut sample: impl FnMut(usize) -> f64) -> Vec<f32> {
  let mut out = Vec::with_capacity(frames * 2);
  for i in 0..frames {
    let value = sample(i) as f32;
    out.extend_from_slice(&[value, value]);
  }
  out
}

fn frames() -> usize {
  (SECONDS * f64::from(RATE)) as usize
}

fn chord() -> Vec<f32> {
  stereo(frames(), |i| chord_sample(i as f64 / f64::from(RATE)))
}

/// Accord, grosse caisse toutes les 0,5 s, bruit court toutes les 0,25 s.
fn music() -> Vec<f32> {
  let mut seed = 0x1234_5678u32;
  stereo(frames(), |i| {
    let t = i as f64 / f64::from(RATE);
    let beat = t % 0.5;
    let kick = (TAU * 60.0 * beat).sin() * (-beat * 20.0).exp() * 0.5;
    seed ^= seed << 13;
    seed ^= seed >> 17;
    seed ^= seed << 5;
    let noise = f64::from(seed) / f64::from(u32::MAX) * 2.0 - 1.0;
    let hat_time = t % 0.25;
    chord_sample(t) + kick + noise * (-hat_time * 200.0).exp() * 0.2
  })
}

/// Impulsions à 120 Hz filtrées par deux formants, 0,25 s de voyelle puis 0,1 s de pause.
/// Approximation grossière de la parole : ne remplace pas une vraie voix à l'écoute.
fn speech_like() -> Vec<f32> {
  let rate = f64::from(RATE);
  let resonator = |freq: f64, bandwidth: f64| {
    let r = (-std::f64::consts::PI * bandwidth / rate).exp();
    (2.0 * r * (TAU * freq / rate).cos(), -r * r)
  };
  let (a1, a2) = resonator(800.0, 90.0);
  let (b1, b2) = resonator(1200.0, 110.0);
  let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
  let period = (rate / 120.0) as usize;
  stereo(frames(), |i| {
    let t = i as f64 / rate;
    let voiced = t % 0.35 < 0.25;
    let excitation = if voiced && i % period == 0 { 1.0 } else { 0.0 };
    let x = excitation + a1 * x1 + a2 * x2;
    x2 = x1;
    x1 = x;
    let y = excitation + b1 * y1 + b2 * y2;
    y2 = y1;
    y1 = y;
    (x + y) * 0.03
  })
}

/// Clics isolés toutes les 0,25 s.
fn clicks() -> Vec<f32> {
  let spacing = (0.25 * f64::from(RATE)) as usize;
  stereo(frames(), |i| if i % spacing == 100 { 0.9 } else { 0.0 })
}

fn left(interleaved: &[f32]) -> Vec<f64> {
  interleaved
    .as_chunks::<2>()
    .0
    .iter()
    .map(|f| f64::from(f[0]))
    .collect()
}

fn hann_window(n: usize) -> Vec<f64> {
  (0..n)
    .map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos())
    .collect()
}

/// Energie hors des partiels de l'accord, en dB par rapport au total (plus bas = plus propre).
fn out_of_partials_db(signal: &[f64]) -> f64 {
  let n = (4 * RATE) as usize;
  let start = (signal.len() - n) / 2;
  let window = hann_window(n);
  let mut buffer: Vec<Complex<f64>> = signal[start..start + n]
    .iter()
    .zip(&window)
    .map(|(s, w)| Complex::new(s * w, 0.0))
    .collect();
  FftPlanner::new().plan_fft_forward(n).process(&mut buffer);
  let bin_hz = f64::from(RATE) / n as f64;
  let half = (PARTIAL_HALF_WIDTH_HZ / bin_hz) as usize;
  let power: Vec<f64> = buffer[..n / 2].iter().map(Complex::norm_sqr).collect();
  let total: f64 = power.iter().sum();
  let mut inside = vec![false; power.len()];
  for base in CHORD {
    for harmonic in HARMONICS {
      let center = (base * harmonic / bin_hz).round() as usize;
      for flag in &mut inside[center - half..=center + half] {
        *flag = true;
      }
    }
  }
  let outside: f64 = power
    .iter()
    .zip(&inside)
    .filter(|(_, i)| !**i)
    .map(|(p, _)| p)
    .sum();
  10.0 * (outside / total).max(1e-12).log10()
}

/// Variation du niveau RMS sur 20 ms (écart-type / moyenne, en %) : fluctuation d'amplitude.
fn flutter_percent(signal: &[f64]) -> f64 {
  let window = (0.02 * f64::from(RATE)) as usize;
  let start = signal.len() / 4;
  let body = &signal[start..signal.len() - start];
  let levels: Vec<f64> = body
    .chunks_exact(window)
    .map(|c| (c.iter().map(|v| v * v).sum::<f64>() / window as f64).sqrt())
    .collect();
  let mean = levels.iter().sum::<f64>() / levels.len() as f64;
  let variance = levels.iter().map(|l| (l - mean).powi(2)).sum::<f64>() / levels.len() as f64;
  100.0 * variance.sqrt() / mean
}

/// Clics : hauteur moyenne conservée (rapport au clic d'origine) et étalement à 10 % de la crête,
/// en échantillons.
fn click_stats(signal: &[f64], spacing_out: f64) -> (f64, f64, f64) {
  let spacing = spacing_out as usize;
  let (mut peaks, mut widths, mut count, mut lost) = (0.0, 0.0, 0.0, 0.0);
  let first = signal.len() / 4;
  let mut position = first;
  while position + spacing < signal.len() * 3 / 4 {
    let window = &signal[position..position + spacing];
    let peak = window.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if peak < 0.09 {
      lost += 1.0;
    } else {
      let above: Vec<usize> = window
        .iter()
        .enumerate()
        .filter(|(_, v)| v.abs() > peak * 0.1)
        .map(|(i, _)| i)
        .collect();
      let width = above.last().copied().unwrap_or(0) - above.first().copied().unwrap_or(0) + 1;
      peaks += peak / 0.9;
      widths += width as f64;
      count += 1.0;
    }
    position += spacing;
  }
  (peaks / count, widths / count, 100.0 * lost / (lost + count))
}

fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
  let data_size = u32::try_from(samples.len() * 2).expect("taille du WAV");
  let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
  bytes.extend_from_slice(b"RIFF");
  bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
  bytes.extend_from_slice(b"WAVEfmt ");
  bytes.extend_from_slice(&16u32.to_le_bytes());
  bytes.extend_from_slice(&1u16.to_le_bytes());
  bytes.extend_from_slice(&2u16.to_le_bytes());
  bytes.extend_from_slice(&RATE.to_le_bytes());
  bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
  bytes.extend_from_slice(&4u16.to_le_bytes());
  bytes.extend_from_slice(&16u16.to_le_bytes());
  bytes.extend_from_slice(b"data");
  bytes.extend_from_slice(&data_size.to_le_bytes());
  for sample in samples {
    let clamped = sample.clamp(-1.0, 1.0);
    let scaled = if clamped < 0.0 {
      clamped * 32768.0
    } else {
      clamped * 32767.0
    };
    bytes.extend_from_slice(&(scaled as i16).to_le_bytes());
  }
  fs::write(path, bytes)
}

fn main() -> std::io::Result<()> {
  let dir = std::env::args()
    .nth(1)
    .map_or_else(|| PathBuf::from("target/tempo_bench"), PathBuf::from);
  fs::create_dir_all(&dir)?;
  let engines: [Engine; 3] = [
    ("wsola-chromium", wsola_chromium),
    ("wsola-defaut", wsola_default),
    ("timestretch", timestretch_keylock),
  ];
  let (chord_in, music_in, speech_in, clicks_in) = (chord(), music(), speech_like(), clicks());
  write_wav(&dir.join("original_musique.wav"), &music_in)?;
  write_wav(&dir.join("original_parole.wav"), &speech_in)?;
  println!(
    "moteur            tempo  xTR    durée%  horsPart dB  fluct%  clicHaut  clicLarg  clicPerdu%"
  );
  let chord_ref = left(&chord_in);
  let (height, width, lost) = click_stats(&left(&clicks_in), 0.25 * f64::from(RATE));
  println!(
    "{:<17} {:<5}  {:<6} {:<7} {:<12.1} {:<7.2} {height:<9.2} {width:<9.1} {lost:<9.1}",
    "original",
    1.0,
    "-",
    "-",
    out_of_partials_db(&chord_ref),
    flutter_percent(&chord_ref),
  );
  for (name, engine) in engines {
    for tempo in [0.8, 1.35] {
      let start = Instant::now();
      let chord_out = engine(&chord_in, tempo);
      let elapsed = start.elapsed().as_secs_f64();
      let expected = SECONDS / tempo;
      let duration = chord_out.len() as f64 / 2.0 / f64::from(RATE);
      let chord_left = left(&chord_out);
      let clicks_out = left(&engine(&clicks_in, tempo));
      let spacing = 0.25 * f64::from(RATE) / tempo;
      let (height, width, lost) = click_stats(&clicks_out, spacing);
      println!(
        "{name:<17} {tempo:<5}  {:<6.0} {:<+7.2} {:<12.1} {:<7.2} {height:<9.2} {width:<9.1} {lost:<9.1}",
        SECONDS / elapsed,
        100.0 * (duration - expected) / expected,
        out_of_partials_db(&chord_left),
        flutter_percent(&chord_left),
      );
      write_wav(
        &dir.join(format!("{name}_musique_{tempo}.wav")),
        &engine(&music_in, tempo),
      )?;
      write_wav(
        &dir.join(format!("{name}_parole_{tempo}.wav")),
        &engine(&speech_in, tempo),
      )?;
    }
  }
  println!("fichiers d'écoute : {}", dir.display());
  Ok(())
}
