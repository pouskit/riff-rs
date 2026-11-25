//! Rendu hors ligne vers PCM `f32` et WAV 16 bits (`renderOffline` et `audioBufferToWav`).

use crate::AudioSettings;
use crate::dsp::hq::HqSettings;
use crate::dsp::waveshaper::Oversample;
use crate::dsp::{Chain, ChainConfig};
use crate::tempo::{TempoParams, TempoStage};

const MIN_SPEED_FACTOR: f64 = 0.1;
const STAGE_BLOCK_FRAMES: usize = 4096;

/// Rend `pcm` (entrelacé, `channels` canaux) avec les effets. Sortie : même nombre de canaux, durée
/// `ceil(longueur / max(0,1; facteur de vitesse))`.
///
/// Mêmes structures que le temps réel, avec les différences de `renderOffline` : pas de `normGain`
/// ni de `normLimiter`, suréchantillonnage 4x, branche HQ sans dry/distorsion/écho/réverbération,
/// `playbackRate` sans conservation de hauteur. Au-delà de 2 canaux, seuls les 2 premiers sont
/// traités, les autres restent muets.
pub fn render(
  pcm: &[f32],
  channels: usize,
  rate: u32,
  settings: &AudioSettings,
  hq: Option<&HqSettings>,
) -> Vec<f32> {
  let channels = channels.max(1);
  let frames = pcm.len() / channels;
  let factor = settings.tempo().speed_factor().max(MIN_SPEED_FACTOR);
  let output_frames = (frames as f64 / factor).ceil() as usize;

  let stereo = to_stereo(pcm, channels);
  let mut played = varispeed(&stereo, rate, factor);
  played.resize(output_frames * 2, 0.0);
  let (mut left, mut right): (Vec<f32>, Vec<f32>) = played
    .as_chunks::<2>()
    .0
    .iter()
    .map(|frame| (frame[0], frame[1]))
    .unzip();

  let mut chain = Chain::new(ChainConfig {
    sample_rate: f64::from(rate),
    hq_oversample: Oversample::X4,
    output_stage: false,
  });
  chain.set_volume(1.0, false);
  match hq {
    Some(hq_settings) => {
      chain.set_hq(true, hq_settings);
      chain.set_settings(&AudioSettings {
        reverb: 0.0,
        echo: 0.0,
        distortion: 0.0,
        ..*settings
      });
    }
    None => chain.set_settings(settings),
  }
  chain.settle();
  chain.process(&mut left, &mut right);
  from_stereo(&left, &right, channels)
}

fn varispeed(stereo: &[f32], rate: u32, factor: f64) -> Vec<f32> {
  if factor == 1.0 {
    return stereo.to_vec();
  }
  let mut out = Vec::with_capacity((stereo.len() as f64 / factor) as usize + STAGE_BLOCK_FRAMES);
  let params = TempoParams {
    speed: 1.0,
    speed_pitch: factor as f32,
    pitch: 0.0,
  };
  let stage = TempoStage::new(rate, rate).and_then(|mut stage| {
    stage.set_params(params)?;
    for block in stereo.chunks(STAGE_BLOCK_FRAMES * 2) {
      stage.process(block, &mut out)?;
    }
    stage.finish(&mut out)
  });
  if let Err(err) = stage {
    tracing::error!(error = %err, "rééchantillonnage hors ligne");
  }
  out
}

fn to_stereo(pcm: &[f32], channels: usize) -> Vec<f32> {
  match channels {
    1 => pcm.iter().flat_map(|v| [*v, *v]).collect(),
    2 => pcm.to_vec(),
    _ => pcm
      .chunks_exact(channels)
      .flat_map(|frame| [frame[0], frame[1]])
      .collect(),
  }
}

fn from_stereo(left: &[f32], right: &[f32], channels: usize) -> Vec<f32> {
  let mut out = Vec::with_capacity(left.len() * channels);
  for (l, r) in left.iter().zip(right) {
    match channels {
      1 => out.push(0.5 * (l + r)),
      _ => {
        out.extend_from_slice(&[*l, *r]);
        out.resize(out.len() + channels - 2, 0.0);
      }
    }
  }
  out
}

/// WAV PCM 16 bits, en-tête de 44 octets écrit à la main (`audioBufferToWav`).
pub fn encode_wav_16(samples: &[f32], channels: u16, rate: u32) -> Vec<u8> {
  let block_align = channels * 2;
  let data_size = (samples.len() * 2) as u32;
  let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
  bytes.extend_from_slice(b"RIFF");
  bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
  bytes.extend_from_slice(b"WAVEfmt ");
  bytes.extend_from_slice(&16u32.to_le_bytes());
  bytes.extend_from_slice(&1u16.to_le_bytes());
  bytes.extend_from_slice(&channels.to_le_bytes());
  bytes.extend_from_slice(&rate.to_le_bytes());
  bytes.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
  bytes.extend_from_slice(&block_align.to_le_bytes());
  bytes.extend_from_slice(&16u16.to_le_bytes());
  bytes.extend_from_slice(b"data");
  bytes.extend_from_slice(&data_size.to_le_bytes());
  for sample in samples {
    let clamped = sample.clamp(-1.0, 1.0);
    let scaled = if clamped < 0.0 {
      clamped * 32_768.0
    } else {
      clamped * 32_767.0
    };
    bytes.extend_from_slice(&(scaled as i16).to_le_bytes());
  }
  bytes
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::dsp::eq::EqGains;
  use crate::dsp::hq::HqEngine;
  use crate::dsp::reverb::{impulse_response, normalization_scale};
  use std::f64::consts::TAU;

  const RATE: u32 = 48_000;

  fn stereo_sine(freq: f64, amplitude: f64, frames: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
      let v = (amplitude * (TAU * freq * i as f64 / f64::from(RATE)).sin()) as f32;
      out.extend_from_slice(&[v, v]);
    }
    out
  }

  #[test]
  fn neutral_render_returns_the_input_unchanged() {
    let input = stereo_sine(440.0, 0.5, 4800);
    assert_eq!(
      render(&input, 2, RATE, &AudioSettings::default(), None),
      input
    );
  }

  #[test]
  fn output_length_follows_the_speed_factor() {
    let input = stereo_sine(440.0, 0.5, 48_000);
    let cases = [
      (
        AudioSettings {
          speed: 1.35,
          ..AudioSettings::default()
        },
        1.35f64,
      ),
      (
        AudioSettings {
          speed_pitch: 0.82,
          ..AudioSettings::default()
        },
        0.82,
      ),
      (
        AudioSettings {
          speed: 1.35,
          speed_pitch: 1.3,
          ..AudioSettings::default()
        },
        1.3,
      ),
      (
        AudioSettings {
          pitch: 12.0,
          ..AudioSettings::default()
        },
        2.0,
      ),
      (
        AudioSettings {
          speed: 0.01,
          ..AudioSettings::default()
        },
        0.1,
      ),
    ];
    for (settings, factor) in cases {
      let out = render(&input, 2, RATE, &settings, None);
      let want = (48_000.0 / f64::from(factor as f32).max(0.1)).ceil() as usize;
      assert!(
        out.len().abs_diff(want * 2) <= 2,
        "{settings:?} : {} au lieu de {}",
        out.len(),
        want * 2
      );
    }
  }

  #[test]
  fn speed_changes_pitch_as_in_the_original_buffer_source() {
    // `renderOffline` règle `playbackRate` sans conservation de hauteur, même pour `speed`.
    let input = stereo_sine(1000.0, 0.5, 96_000);
    let settings = AudioSettings {
      speed: 1.5,
      ..AudioSettings::default()
    };
    let out = render(&input, 2, RATE, &settings, None);
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    let middle = &left[left.len() / 4..left.len() * 3 / 4];
    let crossings = middle
      .windows(2)
      .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
      .count();
    let freq = crossings as f64 / (middle.len() as f64 / f64::from(RATE));
    assert!((freq - 1500.0).abs() < 5.0, "{freq} Hz");
  }

  #[test]
  fn reverb_tail_is_cut_at_the_output_length() {
    let mut input = vec![0.0; 9600];
    input[0] = 1.0;
    input[1] = 1.0;
    let settings = AudioSettings {
      reverb: 100.0,
      ..AudioSettings::default()
    };
    let out = render(&input, 2, RATE, &settings, None);
    assert_eq!(out.len(), input.len());
    let ir = impulse_response(f64::from(RATE));
    let scale = normalization_scale(&ir, f64::from(RATE));
    for frame in 1..4800 {
      let want = ir[0][frame] * scale;
      assert!((out[frame * 2] - want).abs() < 1e-5, "image {frame}");
    }
  }

  #[test]
  fn hq_branch_skips_effects_like_the_original() {
    let input = stereo_sine(1000.0, 0.01, 9600);
    let hq = HqSettings {
      engine: HqEngine::HqMusic3,
      ..HqSettings::default()
    };
    let plain = render(&input, 2, RATE, &AudioSettings::default(), Some(&hq));
    let with_effects = AudioSettings {
      reverb: 100.0,
      echo: 100.0,
      distortion: 100.0,
      ..AudioSettings::default()
    };
    assert_eq!(render(&input, 2, RATE, &with_effects, Some(&hq)), plain);
  }

  #[test]
  fn hq_and_equalizer_change_the_signal() {
    let input = stereo_sine(1000.0, 0.01, 9600);
    let settings = AudioSettings {
      eq: EqGains {
        mid: 6.0,
        ..EqGains::default()
      },
      ..AudioSettings::default()
    };
    assert_ne!(render(&input, 2, RATE, &settings, None), input);
    let hq = HqSettings::default();
    assert_ne!(
      render(&input, 2, RATE, &AudioSettings::default(), Some(&hq)),
      input
    );
  }

  #[test]
  fn volume_setting_is_applied_but_there_is_no_output_limiter() {
    let input = vec![0.5f32; 19_200];
    let settings = AudioSettings {
      volume: 4.0,
      ..AudioSettings::default()
    };
    let out = render(&input, 2, RATE, &settings, None);
    assert!(
      (out[4800] - 2.0).abs() < 1e-5,
      "pas de limiteur hors ligne : {}",
      out[4800]
    );
  }

  #[test]
  fn mono_stays_mono() {
    let input = vec![0.25f32; 4800];
    let out = render(&input, 1, RATE, &AudioSettings::default(), None);
    assert_eq!(out, input);
  }

  #[test]
  fn wav_header_and_samples_follow_the_original() {
    let bytes = encode_wav_16(&[0.0, 1.0, -1.0, 2.0, -2.0, 0.5], 2, 44_100);
    assert_eq!(bytes.len(), 44 + 12);
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(
      u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
      36 + 12
    );
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(
      u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
      16
    );
    assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
    assert_eq!(
      u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
      44_100
    );
    assert_eq!(
      u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]),
      176_400
    );
    assert_eq!(u16::from_le_bytes([bytes[32], bytes[33]]), 4);
    assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(
      u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]),
      12
    );
    let samples: Vec<i16> = bytes[44..]
      .chunks(2)
      .map(|c| i16::from_le_bytes([c[0], c[1]]))
      .collect();
    assert_eq!(samples, [0, 32_767, -32_768, 32_767, -32_768, 16_383]);
  }

  #[test]
  fn wav_output_is_decodable() {
    let input = stereo_sine(1000.0, 0.5, 4800);
    let bytes = encode_wav_16(&input, 2, RATE);
    let mut decoder =
      crate::decode::Decoder::open(&crate::source::Source::from(bytes)).expect("WAV lisible");
    assert_eq!(decoder.sample_rate(), RATE);
    let mut out = Vec::new();
    while decoder.next_block(&mut out).expect("décodage").is_some() {}
    assert_eq!(out.len(), input.len());
    assert!((out[100] - input[100]).abs() < 1e-3);
  }
}
