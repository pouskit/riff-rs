//! Lecture sans interface : affiche la position et le niveau.
//!
//! `cargo run --release -p riff-audio --example play -- <fichier|url> [--eq 0,0,3,0,0,0,0,0,0,0]
//! [--reverb 30] [--echo 15] [--distortion 40] [--speed 1.35] [--speed-pitch 1.3] [--pitch 2]
//! [--hq glyphs-7B] [--volume 0.85] [--seconds 10] [--seek 30]
//! [--crossfade-to <fichier> --crossfade 5 --crossfade-after 20] [--list-devices]`
//!
//! Une URL est lue par plages avec `curl` : exemple seulement, l'application fournit son propre
//! `RangeFetcher` à partir de `riff_core::net`.

mod common;

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::EffectOptions;
use riff_audio::{Engine, EngineConfig, Event, RangeFetcher, Source};

struct CurlFetcher {
  url: String,
  len: Option<u64>,
}

impl CurlFetcher {
  fn new(url: String) -> Self {
    let len = Command::new("curl")
      .args(["-sSIL", url.as_str()])
      .output()
      .ok()
      .and_then(|out| String::from_utf8(out.stdout).ok())
      .and_then(|headers| {
        headers.lines().rev().find_map(|line| {
          let (name, value) = line.split_once(':')?;
          name
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())?
        })
      });
    Self { url, len }
  }
}

struct CurlReader(Child);

impl Read for CurlReader {
  fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
    self
      .0
      .stdout
      .as_mut()
      .map_or(Ok(0), |stdout| stdout.read(buffer))
  }
}

impl Drop for CurlReader {
  fn drop(&mut self) {
    // Le processus peut déjà être terminé : les deux erreurs sont sans conséquence.
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

impl RangeFetcher for CurlFetcher {
  fn fetch(&self, start: u64, end: Option<u64>) -> io::Result<Box<dyn Read + Send>> {
    let range = match end {
      Some(end) => format!("{start}-{}", end.saturating_sub(1)),
      None => format!("{start}-"),
    };
    let child = Command::new("curl")
      .args(["-sSL", "-r", range.as_str(), self.url.as_str()])
      .stdout(Stdio::piped())
      .spawn()?;
    Ok(Box::new(CurlReader(child)))
  }

  fn len(&self) -> Option<u64> {
    self.len
  }
}

fn source_of(target: &str) -> Source {
  if target.starts_with("http://") || target.starts_with("https://") {
    Source::Http(Arc::new(CurlFetcher::new(target.to_string())))
  } else {
    Source::File(PathBuf::from(target))
  }
}

#[derive(Default)]
struct Options {
  target: Option<String>,
  effects: EffectOptions,
  volume: Option<f32>,
  seconds: Option<f64>,
  seek: Option<f64>,
  crossfade_to: Option<String>,
  crossfade: f32,
  crossfade_after: f64,
}

fn parse_number<T: std::str::FromStr>(flag: &str, value: Option<String>) -> Result<T, String> {
  let value = value.ok_or_else(|| format!("{flag} : valeur manquante"))?;
  value
    .parse()
    .map_err(|_| format!("{flag} : nombre invalide : {value}"))
}

fn parse_args() -> Result<Option<Options>, String> {
  let mut args = std::env::args().skip(1);
  let mut options = Options {
    crossfade: 5.0,
    crossfade_after: 20.0,
    ..Options::default()
  };
  while let Some(arg) = args.next() {
    match arg.as_str() {
      "--list-devices" => {
        for device in Engine::list_devices() {
          let mark = if device.is_default { " (défaut)" } else { "" };
          println!("{}{mark} : {}", device.name, device.id);
        }
        return Ok(None);
      }
      "--volume" => options.volume = Some(parse_number(&arg, args.next())?),
      "--seconds" => options.seconds = Some(parse_number(&arg, args.next())?),
      "--seek" => options.seek = Some(parse_number(&arg, args.next())?),
      "--crossfade-to" => options.crossfade_to = args.next(),
      "--crossfade" => options.crossfade = parse_number(&arg, args.next())?,
      "--crossfade-after" => options.crossfade_after = parse_number(&arg, args.next())?,
      flag if flag.starts_with("--") => {
        if !options.effects.apply(flag, &mut args)? {
          return Err(format!("option inconnue : {flag}"));
        }
      }
      target => options.target = Some(target.to_string()),
    }
  }
  if options.target.is_none() {
    return Err("usage : play <fichier|url> [options]".to_string());
  }
  Ok(Some(options))
}

/// Niveau RMS mesuré en sortie du limiteur, sur les derniers échantillons de l'analyseur.
fn output_db(engine: &Engine) -> f64 {
  let mut bytes = [128u8; 128];
  engine.time_bytes(&mut bytes);
  let energy: f64 = bytes
    .iter()
    .map(|b| ((f64::from(*b) - 128.0) / 128.0).powi(2))
    .sum();
  let rms = (energy / bytes.len() as f64).sqrt();
  if rms > 0.0 {
    20.0 * rms.log10()
  } else {
    -120.0
  }
}

fn run() -> Result<(), String> {
  let Some(options) = parse_args()? else {
    return Ok(());
  };
  let target = options.target.as_deref().unwrap_or_default();
  let (engine, events) = Engine::new(EngineConfig::default()).map_err(|e| e.to_string())?;
  engine.set_volume(options.volume.unwrap_or(0.85), false);
  engine.set_settings(options.effects.settings);
  if let Some(hq) = options.effects.hq {
    engine.set_hq(true, hq);
  }
  let start = Duration::from_secs_f64(options.seek.unwrap_or(0.0));
  println!("ouverture de {target}");
  engine.load(source_of(target), start, true);

  let begin = Instant::now();
  let mut crossfade = options.crossfade_to.clone();
  let mut errors = 0;
  let mut loaded = false;
  loop {
    if let Some(limit) = options.seconds
      && begin.elapsed().as_secs_f64() >= limit
    {
      println!("arrêt après {limit} s");
      break;
    }
    if begin.elapsed().as_secs_f64() >= options.crossfade_after
      && let Some(next) = crossfade.take()
    {
      println!("fondu enchaîné vers {next} ({} s)", options.crossfade);
      engine.crossfade_to(source_of(&next), options.crossfade);
    }
    match events.recv_timeout(Duration::from_millis(100)) {
      Ok(Event::Loaded {
        duration,
        sample_rate,
      }) => {
        loaded = true;
        println!("chargé : durée {duration:?}, {sample_rate} Hz");
      }
      Ok(Event::Position(position)) => {
        println!(
          "position {:7.2} s  niveau {:7.2} dB  sortie {:7.2} dB",
          position.as_secs_f64(),
          engine.level_db(),
          output_db(&engine),
        );
      }
      Ok(Event::Ended) => {
        println!("fin du titre");
        break;
      }
      Ok(Event::Buffering(state)) => println!("attente de données : {state}"),
      Ok(Event::DeviceChanged(name)) => println!("périphérique : {name}"),
      Ok(Event::Error(err)) => {
        errors += 1;
        // Sans titre chargé, plus aucun événement n'arrivera : attendre bloquerait pour rien.
        if !loaded || errors > 3 {
          return Err(format!("erreur : {err}"));
        }
        eprintln!("erreur : {err}");
      }
      Err(_) => {}
    }
  }
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
