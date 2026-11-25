//! Rendu hors ligne vers un WAV 16 bits.
//!
//! `cargo run -p riff-audio --example render -- <entrée> <sortie.wav> [--reverb 30] [--echo 15]
//! [--distortion 40] [--eq 0,0,3,0,0,0,0,0,0,0] [--speed 1.35] [--speed-pitch 1.3] [--pitch 2]
//! [--hq glyphs-7B]`

mod common;

use std::path::PathBuf;
use std::process::ExitCode;

use common::EffectOptions;
use riff_audio::Source;
use riff_audio::decode::Decoder;
use riff_audio::offline::{encode_wav_16, render};

fn run() -> Result<(), String> {
  let mut args = std::env::args().skip(1);
  let input = args
    .next()
    .ok_or("usage : render <entrée> <sortie.wav> [options]")?;
  let output = args
    .next()
    .ok_or("usage : render <entrée> <sortie.wav> [options]")?;
  let mut options = EffectOptions::default();
  while let Some(flag) = args.next() {
    if !options.apply(&flag, &mut args)? {
      return Err(format!("option inconnue : {flag}"));
    }
  }

  let mut decoder =
    Decoder::open(&Source::File(PathBuf::from(&input))).map_err(|e| e.to_string())?;
  let rate = decoder.sample_rate();
  let mut pcm = Vec::new();
  while decoder
    .next_block(&mut pcm)
    .map_err(|e| e.to_string())?
    .is_some()
  {}

  let rendered = render(&pcm, 2, rate, &options.settings, options.hq.as_ref());
  std::fs::write(&output, encode_wav_16(&rendered, 2, rate)).map_err(|e| e.to_string())?;
  println!(
    "{output} : {:.2} s à {rate} Hz (entrée {:.2} s)",
    rendered.len() as f64 / 2.0 / f64::from(rate),
    pcm.len() as f64 / 2.0 / f64::from(rate),
  );
  Ok(())
}

fn main() -> ExitCode {
  match run() {
    Ok(()) => ExitCode::SUCCESS,
    Err(err) => {
      eprintln!("{err}");
      ExitCode::FAILURE
    }
  }
}
