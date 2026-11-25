//! Mesures lancées par des arguments : défilement (`--bench-scroll`), délai du premier son (`--latency-log`)
//! et scènes (`--scene`).

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Budget d'une image à 60 Hz.
const BUDGET_MS: f32 = 1000.0 / 60.0;
const SEVERE_MS: f32 = 2000.0 / 60.0;
const WARMUP: Duration = Duration::from_secs(3);
const CAPTURE: Duration = Duration::from_secs(30);
/// Vitesse du défilement automatique : 2 000 pixels par seconde, comme le prototype.
const SCROLL_PER_SECOND: f32 = 2000.0;

/// Nombre de vues construites : vaut 0 de plus par seconde quand rien n'est animé.
pub static VIEW_COUNT: AtomicU64 = AtomicU64::new(0);
/// Durée du plus long calcul de fond flouté (décision 3), en microsecondes.
pub static BLUR_MAX_MICROS: AtomicU64 = AtomicU64::new(0);

/// Arguments d'une scène : `--scene a --scene-file f --scene-log l --scene-seconds n [--scene-pause s]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneArgs {
  pub name: String,
  pub file: PathBuf,
  pub log: PathBuf,
  pub seconds: u64,
  /// Met la lecture en pause après ce nombre de secondes, pour compter les vues au repos.
  pub pause_after: Option<u64>,
  /// `--scene-unthrottled` : images au rythme de l'écran, pour mesurer la capacité.
  pub unthrottled: bool,
}

/// Temps de chauffe d'une scène : le titre démarre et les textures se chargent.
const SCENE_WARMUP: Duration = Duration::from_secs(8);
/// Après la pause, attente avant de compter : le défilement et les barres finissent de retomber.
const REST_SETTLE: Duration = Duration::from_secs(5);

/// Images d'une scène, écrites à la fin.
#[derive(Debug, Default)]
pub struct SceneFrames {
  started: Option<Instant>,
  last: Option<Instant>,
  intervals: Vec<f32>,
}

impl SceneFrames {
  /// Une image reçue.
  pub fn frame(&mut self, at: Instant) {
    let started = *self.started.get_or_insert(at);
    if at.saturating_duration_since(started) >= SCENE_WARMUP
      && let Some(last) = self.last
    {
      self
        .intervals
        .push(at.saturating_duration_since(last).as_secs_f32() * 1000.0);
    }
    self.last = Some(at);
  }
}

/// Fin d'une scène, dans son propre fil : pas de message ni d'image en plus dans la mesure.
pub fn finish_scene_after(args: SceneArgs, frames: Arc<Mutex<SceneFrames>>) {
  let spawned = std::thread::Builder::new().name("riff-scene".into()).spawn(move || {
    let total = Duration::from_secs(args.seconds);
    let mut rest_views = None;
    if let Some(pause) = args.pause_after {
      std::thread::sleep(Duration::from_secs(pause) + REST_SETTLE);
      let before = VIEW_COUNT.load(Ordering::Relaxed);
      let rest = total.saturating_sub(Duration::from_secs(pause) + REST_SETTLE);
      std::thread::sleep(rest);
      rest_views = Some((VIEW_COUNT.load(Ordering::Relaxed) - before, rest.as_secs()));
    } else {
      std::thread::sleep(total);
    }
    let stats = frames.lock().map(|frames| frame_stats(&frames.intervals)).ok();
    let mut report = String::from("metric,value\n");
    if let Some(stats) = stats {
      let _ = write!(
        report,
        "frames,{}\np50_ms,{:.2}\np95_ms,{:.2}\np99_ms,{:.2}\nmax_ms,{:.2}\nmissed,{}\nsevere,{}\n",
        stats.frames, stats.p50, stats.p95, stats.p99, stats.max, stats.missed, stats.severe
      );
    }
    let _ = writeln!(report, "views,{}", VIEW_COUNT.load(Ordering::Relaxed));
    let _ = writeln!(report, "blur_max_ms,{:.2}", BLUR_MAX_MICROS.load(Ordering::Relaxed) as f64 / 1000.0);
    if let Some((views, seconds)) = rest_views {
      let _ = write!(report, "rest_views,{views}\nrest_seconds,{seconds}\n");
    }
    if let Err(err) = std::fs::write(&args.log, report) {
      tracing::warn!(error = %err, "écriture de la mesure de scène");
    }
    std::process::exit(0);
  });
  if let Err(err) = spawned {
    tracing::error!(error = %err, "fil de la mesure de scène");
  }
}

/// Résumé des intervalles entre images.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameStats {
  pub frames: usize,
  pub p50: f32,
  pub p95: f32,
  pub p99: f32,
  pub max: f32,
  pub missed: usize,
  pub severe: usize,
}

/// Calcule les statistiques d'intervalles en millisecondes.
#[must_use]
pub fn frame_stats(intervals: &[f32]) -> FrameStats {
  let mut sorted = intervals.to_vec();
  sorted.sort_by(f32::total_cmp);
  let percentile = |p: f32| -> f32 {
    if sorted.is_empty() {
      return 0.0;
    }
    let rank = (p / 100.0 * sorted.len() as f32).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
  };
  FrameStats {
    frames: sorted.len(),
    p50: percentile(50.0),
    p95: percentile(95.0),
    p99: percentile(99.0),
    max: sorted.last().copied().unwrap_or(0.0),
    missed: sorted.iter().filter(|value| **value > BUDGET_MS).count(),
    severe: sorted.iter().filter(|value| **value > SEVERE_MS).count(),
  }
}

/// Défilement automatique mesuré.
#[derive(Debug, Clone)]
pub struct ScrollBench {
  path: PathBuf,
  started: Option<Instant>,
  last: Option<Instant>,
  intervals: Vec<f32>,
  direction: f32,
}

impl ScrollBench {
  #[must_use]
  pub const fn new(path: PathBuf) -> Self {
    Self {
      path,
      started: None,
      last: None,
      intervals: Vec::new(),
      direction: 1.0,
    }
  }

  /// Retient une image. Renvoie le déplacement à appliquer, `None` quand la capture est finie.
  pub fn frame(&mut self, at: Instant, offset: f32, content_end: f32) -> Option<f32> {
    let started = *self.started.get_or_insert(at);
    let elapsed = at.saturating_duration_since(started);
    let delta = self
      .last
      .map_or(Duration::ZERO, |last| at.saturating_duration_since(last));
    if elapsed >= WARMUP && self.last.is_some() {
      self.intervals.push(delta.as_secs_f32() * 1000.0);
    }
    self.last = Some(at);
    if elapsed >= WARMUP + CAPTURE {
      return None;
    }
    if offset >= content_end {
      self.direction = -1.0;
    } else if offset <= 0.0 {
      self.direction = 1.0;
    }
    Some(self.direction * SCROLL_PER_SECOND * delta.as_secs_f32())
  }

  /// Écrit le résultat.
  ///
  /// # Errors
  /// Erreur du système de fichiers.
  pub fn write(&self) -> std::io::Result<()> {
    write_stats(&self.path, &frame_stats(&self.intervals))
  }
}

fn write_stats(path: &Path, stats: &FrameStats) -> std::io::Result<()> {
  let mut csv = String::new();
  let _ = writeln!(csv, "frames,{}", stats.frames);
  let _ = writeln!(csv, "p50_ms,{:.3}", stats.p50);
  let _ = writeln!(csv, "p95_ms,{:.3}", stats.p95);
  let _ = writeln!(csv, "p99_ms,{:.3}", stats.p99);
  let _ = writeln!(csv, "max_ms,{:.3}", stats.max);
  let _ = writeln!(csv, "missed_over_16_67ms,{}", stats.missed);
  let _ = writeln!(csv, "severe_over_33_33ms,{}", stats.severe);
  std::fs::write(path, csv)
}

/// Délai entre la demande d'un titre et sa première position : la source est prête et l'audio part.
#[derive(Debug, Clone)]
pub struct LatencyLog {
  path: PathBuf,
  started: Option<Instant>,
  resolved: Option<Instant>,
}

impl LatencyLog {
  #[must_use]
  pub const fn new(path: PathBuf) -> Self {
    Self {
      path,
      started: None,
      resolved: None,
    }
  }

  pub fn track_requested(&mut self, at: Instant) {
    self.started = Some(at);
    self.resolved = None;
  }

  pub fn source_ready(&mut self, at: Instant) {
    self.resolved = Some(at);
  }

  /// Première position du titre : écrit `source_ms,son_ms`.
  pub fn first_position(&mut self, at: Instant) {
    let Some(started) = self.started.take() else {
      return;
    };
    let ms = |to: Instant| to.saturating_duration_since(started).as_millis();
    let line = format!("{},{}\n", self.resolved.take().map_or(0, ms), ms(at));
    let written = OpenOptions::new()
      .create(true)
      .append(true)
      .open(&self.path)
      .and_then(|mut file| file.write_all(line.as_bytes()));
    if let Err(err) = written {
      tracing::warn!(error = %err, "écriture de la mesure de délai");
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn scene_frames_ignore_the_warmup() {
    let start = Instant::now();
    let mut frames = SceneFrames::default();
    frames.frame(start);
    frames.frame(start + Duration::from_secs(1));
    assert!(frames.intervals.is_empty());
    frames.frame(start + SCENE_WARMUP);
    frames.frame(start + SCENE_WARMUP + Duration::from_millis(16));
    assert_eq!(frames.intervals.len(), 2);
    assert!((frames.intervals[1] - 16.0).abs() < 1e-3);
  }

  #[test]
  fn statistics_follow_the_definitions_of_the_prototype() {
    let stats = frame_stats(&[10.0, 16.0, 17.0, 40.0]);
    assert_eq!(stats.frames, 4);
    assert_eq!(stats.p50, 16.0);
    assert_eq!(stats.p95, 40.0);
    assert_eq!(stats.max, 40.0);
    assert_eq!(stats.missed, 2);
    assert_eq!(stats.severe, 1);
  }

  #[test]
  fn no_frame_gives_zeros() {
    assert_eq!(frame_stats(&[]).p95, 0.0);
  }

  #[test]
  fn the_scroll_runs_down_then_up_and_stops_after_the_capture() {
    let start = Instant::now();
    let mut bench = ScrollBench::new(PathBuf::from("x.csv"));
    assert_eq!(bench.frame(start, 0.0, 100.0), Some(0.0));
    let step = bench
      .frame(start + Duration::from_millis(10), 0.0, 100.0)
      .unwrap();
    assert!((step - 20.0).abs() < 0.01, "{step}");
    let up = bench
      .frame(start + Duration::from_millis(20), 100.0, 100.0)
      .unwrap();
    assert!(up < 0.0);
    assert!(bench.intervals.is_empty(), "préchauffage");
    bench.frame(start + WARMUP, 50.0, 100.0);
    bench.frame(start + WARMUP + Duration::from_millis(16), 50.0, 100.0);
    assert_eq!(bench.intervals.len(), 2);
    assert_eq!(bench.frame(start + WARMUP + CAPTURE, 50.0, 100.0), None);
  }

  #[test]
  fn the_latency_is_written_once_per_track() {
    let dir = crate::test_temp::TempDir::new("latence");
    let path = dir.join("latence.csv");
    let start = Instant::now();
    let mut log = LatencyLog::new(path.clone());
    log.first_position(start);
    assert!(!path.exists());
    log.track_requested(start);
    log.source_ready(start + Duration::from_millis(5));
    log.first_position(start + Duration::from_millis(9));
    log.first_position(start + Duration::from_millis(99));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "5,9\n");
  }
}
