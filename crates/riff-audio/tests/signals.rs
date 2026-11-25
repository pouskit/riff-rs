//! Signaux de référence générés par les tests (sinus, balayage, bruit à graine fixe) et mesures
//! de niveau et de fréquence, sans fichier audio ni périphérique.

use std::f64::consts::TAU;
use std::sync::Arc;

use riff_audio::AudioSettings;
use riff_audio::dsp::eq::EqGains;
use riff_audio::dsp::waveshaper::Oversample;
use riff_audio::dsp::{Chain, ChainConfig};
use riff_audio::mixer::{Mixer, MixerCommand, MixerControls, voice_channel};
use riff_audio::offline::render;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

const RATE: u32 = 48_000;

/// Sinus mono.
fn sine(freq: f64, amplitude: f64, frames: usize) -> Vec<f32> {
  (0..frames)
    .map(|i| (amplitude * (TAU * freq * i as f64 / f64::from(RATE)).sin()) as f32)
    .collect()
}

/// Balayage logarithmique de `from` à `to` Hz.
fn sweep(from: f64, to: f64, amplitude: f64, frames: usize) -> Vec<f32> {
  let seconds = frames as f64 / f64::from(RATE);
  let k = (to / from).ln();
  (0..frames)
    .map(|i| {
      let t = i as f64 / f64::from(RATE);
      let phase = TAU * from * seconds / k * ((k * t / seconds).exp() - 1.0);
      (amplitude * phase.sin()) as f32
    })
    .collect()
}

/// Bruit blanc uniforme dans `[-amplitude, amplitude]`, xorshift32 : même graine, même suite.
fn noise(seed: u32, amplitude: f32, frames: usize) -> Vec<f32> {
  let mut state = seed.max(1);
  (0..frames)
    .map(|_| {
      state ^= state << 13;
      state ^= state >> 17;
      state ^= state << 5;
      (f64::from(state) / f64::from(u32::MAX) * 2.0 - 1.0) as f32 * amplitude
    })
    .collect()
}

fn stereo(mono: &[f32]) -> Vec<f32> {
  mono.iter().flat_map(|v| [*v, *v]).collect()
}

fn left(interleaved: &[f32]) -> Vec<f32> {
  interleaved.iter().step_by(2).copied().collect()
}

fn rms_db(samples: &[f32]) -> f64 {
  let sum: f64 = samples.iter().map(|v| f64::from(*v).powi(2)).sum();
  20.0 * (sum / samples.len() as f64).sqrt().log10()
}

/// Fréquence dominante (fenêtre de Hann, interpolation parabolique du pic).
fn dominant_frequency(samples: &[f32]) -> f64 {
  let n = samples.len().next_power_of_two() / 2;
  let n = n.max(1024);
  let mut buffer: Vec<Complex<f64>> = (0..n)
    .map(|i| {
      let window = 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos();
      Complex::new(f64::from(samples[i]) * window, 0.0)
    })
    .collect();
  FftPlanner::new().plan_fft_forward(n).process(&mut buffer);
  let magnitude: Vec<f64> = buffer[..n / 2].iter().map(|c| c.norm()).collect();
  let peak = (1..magnitude.len() - 1)
    .max_by(|a, b| magnitude[*a].total_cmp(&magnitude[*b]))
    .expect("spectre non vide");
  let (a, b, c) = (
    magnitude[peak - 1].ln(),
    magnitude[peak].ln(),
    magnitude[peak + 1].ln(),
  );
  let shift = 0.5 * (a - c) / (a - 2.0 * b + c);
  (peak as f64 + shift) * f64::from(RATE) / n as f64
}

/// Sortie simulée du chemin temps réel : voix remplie d'avance, mélangeur appelé par blocs de
/// 480 images comme le ferait le callback, sans fil ni horloge.
fn through_mixer(
  mono: &[f32],
  settings: AudioSettings,
  volume: f32,
  normalize_db: f32,
) -> Vec<f32> {
  let chain = Chain::new(ChainConfig {
    sample_rate: f64::from(RATE),
    hq_oversample: Oversample::X2,
    output_stage: true,
  });
  let (mut commands, command_rx) = rtrb::RingBuffer::new(8);
  let (trash_tx, _trash) = rtrb::RingBuffer::new(4);
  let controls = Arc::new(MixerControls::new(volume, false));
  controls.set_normalize_db(normalize_db);
  let mut mixer = Mixer::new(chain, command_rx, trash_tx, controls, RATE as f32);
  let (mut producer, voice, shared) = voice_channel(mono.len(), RATE);
  for value in mono {
    assert!(producer.push(*value).is_ok() && producer.push(*value).is_ok());
  }
  shared.mark_finished();
  assert!(commands.push(MixerCommand::Settings(settings)).is_ok());
  assert!(commands.push(MixerCommand::SetVoice(voice)).is_ok());
  let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
  let mut out = Vec::with_capacity(mono.len());
  while out.len() < mono.len() {
    mixer.render(&mut l, &mut r);
    out.extend_from_slice(&l);
  }
  out.truncate(mono.len());
  out
}

fn neutral() -> AudioSettings {
  AudioSettings::default()
}

fn with_eq(eq: EqGains) -> AudioSettings {
  AudioSettings {
    eq,
    ..AudioSettings::default()
  }
}

/// Gain mesuré en dB entre une entrée et une sortie, en ignorant les 0,5 premières secondes.
fn gain_db(input: &[f32], output: &[f32]) -> f64 {
  rms_db(&output[24_000..]) - rms_db(&input[24_000..])
}

fn offline_mono(mono: &[f32], settings: &AudioSettings) -> Vec<f32> {
  left(&render(&stereo(mono), 2, RATE, settings, None))
}

#[test]
fn generators_are_deterministic_and_calibrated() {
  assert_eq!(noise(7, 0.5, 4800), noise(7, 0.5, 4800));
  assert_ne!(noise(7, 0.5, 64), noise(8, 0.5, 64));
  // Suite figée : un changement du générateur invaliderait toutes les références du fichier.
  let head = noise(7, 0.5, 4);
  let want = [-0.499_559_34f32, -0.390_478_97, 0.403_896_42, 0.214_787_62];
  for (got, want) in head.iter().zip(want) {
    assert!((got - want).abs() < 1e-6, "{head:?}");
  }
  // Bruit uniforme d'amplitude a : RMS a / racine de 3, soit -10,79 dBFS pour a = 0,5.
  assert!((rms_db(&noise(7, 0.5, 48_000)) + 10.79).abs() < 0.1);
  // Sinus d'amplitude a : a / racine de 2, soit -9,03 dBFS pour a = 0,5.
  assert!((rms_db(&sine(1000.0, 0.5, 48_000)) + 9.03).abs() < 0.01);
  assert!((dominant_frequency(&sine(1000.0, 0.5, 16_384)) - 1000.0).abs() < 1.0);
  // Balayage : la fréquence instantanée suit f0 * exp(k t / T), mesurée au centre de la fenêtre.
  let frames = 96_000;
  let sw = sweep(20.0, 20_000.0, 0.5, frames);
  for centre in [0.5f64, 1.0, 1.5] {
    let start = (centre * f64::from(RATE)) as usize - 1024;
    let got = dominant_frequency(&sw[start..start + 2048]);
    let want = 20.0 * (1000f64.ln() * centre / 2.0).exp();
    assert!(
      (got / want - 1.0).abs() < 0.05,
      "t = {centre} s : {got} Hz au lieu de {want}"
    );
  }
}

#[test]
fn neutral_offline_render_keeps_level_and_frequency_of_sines() {
  for freq in [100.0, 1000.0, 8000.0] {
    let input = sine(freq, 0.25, 48_000);
    let out = offline_mono(&input, &neutral());
    assert!(gain_db(&input, &out).abs() < 0.01, "{freq} Hz");
    let got = dominant_frequency(&out[8192..8192 + 16_384]);
    assert!((got - freq).abs() < 1.0, "{freq} Hz mesuré {got} Hz");
  }
}

#[test]
fn neutral_offline_render_is_bit_exact_for_sweep_and_noise() {
  for input in [sweep(20.0, 20_000.0, 0.5, 48_000), noise(42, 0.8, 48_000)] {
    assert_eq!(offline_mono(&input, &neutral()), input);
  }
}

#[test]
fn equalizer_gains_are_measured_at_the_output() {
  let mid = EqGains {
    mid: 6.0,
    ..EqGains::default()
  };
  let band = EqGains {
    bands: [0.0, 0.0, 0.0, 0.0, 0.0, 12.0, 0.0, 0.0, 0.0, 0.0],
    ..EqGains::default()
  };
  let low = EqGains {
    low: -12.0,
    ..EqGains::default()
  };
  let high = EqGains {
    high: 9.0,
    ..EqGains::default()
  };
  // (réglage, fréquence du sinus, gain attendu en dB, tolérance)
  let cases = [
    (mid, 1000.0, 6.0, 0.1),
    (band, 1000.0, 12.0, 0.1),
    (band, 100.0, 0.0, 0.5),
    (band, 10_000.0, 0.0, 0.5),
    (low, 100.0, -11.1, 0.3),
    (low, 10_000.0, 0.0, 0.1),
    (high, 10_000.0, 8.8, 0.3),
    (high, 100.0, 0.0, 0.1),
  ];
  for (eq, freq, want, tolerance) in cases {
    let input = sine(freq, 0.25, 48_000);
    let out = offline_mono(&input, &with_eq(eq));
    let got = gain_db(&input, &out);
    assert!(
      (got - want).abs() < tolerance,
      "{eq:?} à {freq} Hz : {got:.2} dB au lieu de {want}"
    );
    // Un égaliseur ne déplace pas la fréquence.
    let measured = dominant_frequency(&out[8192..8192 + 16_384]);
    assert!(
      (measured - freq).abs() < 1.0,
      "{freq} Hz mesuré {measured} Hz"
    );
  }
}

#[test]
fn pitch_and_speed_move_the_measured_frequency() {
  let input = sine(1000.0, 0.5, 48_000);
  let cases = [
    (
      AudioSettings {
        pitch: 12.0,
        ..neutral()
      },
      2000.0,
    ),
    (
      AudioSettings {
        speed_pitch: 0.82,
        ..neutral()
      },
      820.0,
    ),
    (
      AudioSettings {
        speed: 1.5,
        ..neutral()
      },
      1500.0,
    ),
  ];
  for (settings, want) in cases {
    let out = offline_mono(&input, &settings);
    let got = dominant_frequency(&out[4096..4096 + 16_384]);
    assert!(
      (got - want).abs() < 1.0,
      "{settings:?} : {got} Hz au lieu de {want}"
    );
  }
}

#[test]
fn realtime_path_keeps_the_frequency_of_a_sine() {
  let input = sine(1000.0, 0.5, 48_000);
  let out = through_mixer(&input, neutral(), 1.0, 0.0);
  let got = dominant_frequency(&out[8192..8192 + 16_384]);
  assert!((got - 1000.0).abs() < 1.0, "{got} Hz");
}

#[test]
fn realtime_path_follows_the_sweep() {
  let sw = sweep(20.0, 20_000.0, 0.25, 96_000);
  let out = through_mixer(&sw, neutral(), 1.0, 0.0);
  let delay = delay_of(&noise(5, 0.3, 48_000), neutral());
  for centre in [0.5f64, 1.0, 1.5] {
    // La sortie est en retard de la latence du chemin temps réel ; la référence est la même
    // fenêtre du signal d'entrée (le biais de la fenêtre sur un balayage rapide est commun).
    let start = (centre * f64::from(RATE)) as usize - 1024;
    let want = dominant_frequency(&sw[start..start + 2048]);
    let got = dominant_frequency(&out[start + delay..start + delay + 2048]);
    assert!(
      (got / want - 1.0).abs() < 0.01,
      "t = {centre} s : {got} Hz au lieu de {want}"
    );
  }
}

/// Retard en images du chemin temps réel, par intercorrélation d'un bruit à graine fixe.
fn delay_of(input: &[f32], settings: AudioSettings) -> usize {
  let output = through_mixer(input, settings, 1.0, 0.0);
  let window = &input[10_000..20_000];
  let correlation = |lag: usize| -> f64 {
    window
      .iter()
      .zip(&output[10_000 + lag..])
      .map(|(a, b)| f64::from(*a) * f64::from(*b))
      .sum()
  };
  (0..3000)
    .max_by(|a, b| correlation(*a).total_cmp(&correlation(*b)))
    .expect("décalages")
}

#[test]
fn realtime_path_latency_is_a_few_milliseconds() {
  let delay = delay_of(&noise(5, 0.3, 48_000), neutral());
  // Mesuré : 288 images (6 ms) à 48 kHz.
  assert!((200..400).contains(&delay), "{delay} images");
}

#[test]
fn user_volume_follows_the_square_law_at_the_output() {
  let input = sine(1000.0, 0.5, 48_000);
  let full = through_mixer(&input, neutral(), 1.0, 0.0);
  let half = through_mixer(&input, neutral(), 0.5, 0.0);
  // Volume utilisateur au carré (`clamp(volume, 0, 1)²`) : 0,5 donne 0,25, soit -12,04 dB.
  let drop = rms_db(&half[24_000..]) - rms_db(&full[24_000..]);
  assert!((drop + 12.04).abs() < 0.1, "{drop:.2} dB");
}

#[test]
fn muted_volume_gives_silence_after_the_ramp() {
  let input = noise(3, 0.5, 96_000);
  let out = through_mixer(&input, neutral(), 0.0, 0.0);
  assert!(out[24_000..].iter().all(|v| *v == 0.0), "reste du signal");
}

#[test]
fn normalization_gain_raises_the_level() {
  // Constante de temps de 0,8 s : on mesure la dernière demi-seconde d'un signal de 5 s, à un
  // niveau trop bas pour que le compresseur de sortie intervienne.
  let input = sine(1000.0, 0.01, 240_000);
  let flat = through_mixer(&input, neutral(), 1.0, 0.0);
  let boosted = through_mixer(&input, neutral(), 1.0, 6.0);
  let tail = 216_000..;
  let gain = rms_db(&boosted[tail.clone()]) - rms_db(&flat[tail]);
  assert!((gain - 6.0).abs() < 0.3, "{gain:.2} dB");
}

#[test]
fn full_chain_on_noise_stays_finite_and_bounded() {
  let all = AudioSettings {
    reverb: 60.0,
    echo: 30.0,
    distortion: 50.0,
    eq: EqGains {
      low: 6.0,
      mid: -3.0,
      high: 6.0,
      bands: [3.0; 10],
    },
    ..neutral()
  };
  let input = noise(7, 0.9, 96_000);
  let out = through_mixer(&input, all, 0.85, 0.0);
  assert!(out.iter().all(|v| v.is_finite()));
  let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
  assert!(peak > 0.1 && peak < 4.0, "crête {peak}");
}

#[test]
fn silence_stays_silent_without_distortion() {
  let all = AudioSettings {
    reverb: 60.0,
    echo: 30.0,
    eq: EqGains {
      low: 6.0,
      mid: -3.0,
      high: 6.0,
      bands: [3.0; 10],
    },
    ..neutral()
  };
  let out = through_mixer(&vec![0.0; 96_000], all, 0.85, 0.0);
  assert!(out.iter().all(|v| *v == 0.0));
}
