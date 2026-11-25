//! Budget CPU : traitement d'une seconde de stéréo à 48 kHz, tout activé.
//!
//! `cargo run --release -p riff-audio --example bench`

use std::f64::consts::TAU;
use std::sync::Arc;
use std::time::Instant;

use riff_audio::AudioSettings;
use riff_audio::analyser::SampleRing;
use riff_audio::dsp::eq::EqGains;
use riff_audio::dsp::hq::{HqEngine, HqSettings};
use riff_audio::dsp::waveshaper::Oversample;
use riff_audio::dsp::{Chain, ChainConfig};

const RATE: usize = 48_000;
const SECONDS: usize = 30;

fn signal() -> (Vec<f32>, Vec<f32>) {
  let left = (0..RATE)
    .map(|i| {
      let t = i as f64 / RATE as f64;
      (0.3 * (TAU * 220.0 * t).sin() + 0.2 * (TAU * 1760.0 * t).sin()) as f32
    })
    .collect::<Vec<_>>();
  let right = left.iter().rev().copied().collect();
  (left, right)
}

fn measure(name: &str, chain: &mut Chain, source: &(Vec<f32>, Vec<f32>)) -> f64 {
  let (mut left, mut right) = source.clone();
  // Le signal se répète ; recopié à chaque seconde pour rester à niveau constant.
  let start = Instant::now();
  for _ in 0..SECONDS {
    left.copy_from_slice(&source.0);
    right.copy_from_slice(&source.1);
    for (l, r) in left.chunks_mut(480).zip(right.chunks_mut(480)) {
      chain.process(l, r);
    }
    std::hint::black_box(&left);
  }
  let factor = SECONDS as f64 / start.elapsed().as_secs_f64();
  println!("{name:<34} {factor:>8.1}x temps réel");
  factor
}

fn new_chain() -> Chain {
  let mut chain = Chain::new(ChainConfig {
    sample_rate: RATE as f64,
    hq_oversample: Oversample::X2,
    output_stage: true,
  });
  chain.set_taps(
    Some(Arc::new(SampleRing::new(4096))),
    Some(Arc::new(SampleRing::new(4096))),
  );
  chain.set_volume(0.85, false);
  chain
}

fn main() {
  let source = signal();
  let eq = EqGains {
    low: 2.0,
    mid: -1.0,
    high: 3.0,
    bands: [1.0; 10],
  };

  measure("neutre (limiteur seul)", &mut new_chain(), &source);

  let mut only_eq = new_chain();
  only_eq.set_settings(&AudioSettings {
    eq,
    ..AudioSettings::default()
  });
  measure("égaliseur 13 filtres", &mut only_eq, &source);

  let mut only_reverb = new_chain();
  only_reverb.set_settings(&AudioSettings {
    reverb: 60.0,
    ..AudioSettings::default()
  });
  measure("réverbération", &mut only_reverb, &source);

  let mut only_hq = new_chain();
  only_hq.set_hq(
    true,
    &HqSettings {
      engine: HqEngine::Glyphs7B,
      ..HqSettings::default()
    },
  );
  measure("HQ (suréchantillonnage 2x)", &mut only_hq, &source);

  let mut all = new_chain();
  all.set_settings(&AudioSettings {
    reverb: 60.0,
    echo: 30.0,
    distortion: 50.0,
    eq,
    ..AudioSettings::default()
  });
  all.set_hq(
    true,
    &HqSettings {
      engine: HqEngine::Glyphs7B,
      ..HqSettings::default()
    },
  );
  let factor = measure("TOUT activé", &mut all, &source);
  println!(
    "critère >= 50x : {}",
    if factor >= 50.0 {
      "atteint"
    } else {
      "NON atteint"
    }
  );
}
