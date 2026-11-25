//! Cache et diagnostic de la page de réglages. Même logique que `get_cache_size`, `clear_cache` et `get_diagnostics`
//! de l'ancienne interface Tauri.

use crate::{logging, paths};
use std::env;
use std::fs;
use std::path::Path;

const AUDIO_EXTENSIONS: [&str; 4] = ["opus", "webm", "m4a", "mp3"];

fn is_cached_audio(path: &Path) -> bool {
  path
    .extension()
    .and_then(|ext| ext.to_str())
    .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn size_in(tracks: &Path, thumbnails: &Path) -> u64 {
  let mut total = 0;
  for (dir, audio_only) in [(tracks, true), (thumbnails, false)] {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
      if audio_only && !is_cached_audio(&entry.path()) {
        continue;
      }
      if let Ok(meta) = entry.metadata()
        && meta.is_file()
      {
        total += meta.len();
      }
    }
  }
  total
}

fn clear_in(tracks: &Path, thumbnails: &Path) {
  for (dir, audio_only) in [(tracks, true), (thumbnails, false)] {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
      if !audio_only || is_cached_audio(&entry.path()) {
        let _ = fs::remove_file(entry.path());
      }
    }
  }
}

/// Taille du cache audio et des miniatures, en octets.
#[must_use]
pub fn cache_size() -> u64 {
  size_in(&paths::tracks_dir(), &paths::thumbnails_dir())
}

/// Vide le cache audio et les miniatures.
pub fn clear_cache() {
  clear_in(&paths::tracks_dir(), &paths::thumbnails_dir());
}

/// Diagnostic à copier : version, système, dossiers, outils, fin du journal masquée.
#[must_use]
pub fn diagnostics(app_version: &str) -> String {
  let mut lines = vec![
    format!("Riff {app_version} (native)"),
    format!("OS {} {}", env::consts::OS, env::consts::ARCH),
    format!("Data {}", paths::resolve_data_dir().to_string_lossy()),
  ];
  for name in ["yt-dlp", "ffmpeg"] {
    let found = paths::resolve_binary(name).map_or_else(
      || "NOT FOUND".to_string(),
      |path| path.to_string_lossy().into_owned(),
    );
    lines.push(format!("{name}: {found}"));
  }
  lines.push("--- last log lines ---".to_string());
  lines.extend(
    logging::tail_log(60)
      .iter()
      .map(|line| logging::redact(line)),
  );
  lines.join("\n")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;

  fn temp_dirs(name: &str) -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let root = TempDir::new(&format!("maintenance-{name}"));
    let (tracks, thumbnails) = (root.join("tracks"), root.join("thumbnails"));
    fs::create_dir_all(&tracks).unwrap();
    fs::create_dir_all(&thumbnails).unwrap();
    (root, tracks, thumbnails)
  }

  #[test]
  fn only_cached_audio_and_thumbnails_are_counted() {
    let (_root, tracks, thumbnails) = temp_dirs("size");
    fs::write(tracks.join("a.mp3"), [0; 10]).unwrap();
    fs::write(tracks.join("b.OPUS"), [0; 5]).unwrap();
    fs::write(tracks.join("notes.txt"), [0; 100]).unwrap();
    fs::write(thumbnails.join("c.jpg"), [0; 2]).unwrap();
    assert_eq!(size_in(&tracks, &thumbnails), 17);
  }

  #[test]
  fn clearing_keeps_everything_else() {
    let (_root, tracks, thumbnails) = temp_dirs("clear");
    fs::write(tracks.join("a.mp3"), [0; 10]).unwrap();
    fs::write(tracks.join("notes.txt"), [0; 10]).unwrap();
    fs::write(thumbnails.join("c.jpg"), [0; 2]).unwrap();
    clear_in(&tracks, &thumbnails);
    assert!(!tracks.join("a.mp3").exists());
    assert!(tracks.join("notes.txt").exists());
    assert!(!thumbnails.join("c.jpg").exists());
    assert_eq!(size_in(&tracks, &thumbnails), 0);
  }

  #[test]
  fn missing_folders_count_as_empty() {
    let absent = Path::new("Z:/absent/riff");
    assert_eq!(size_in(absent, absent), 0);
    clear_in(absent, absent);
  }

  #[test]
  fn the_diagnostics_name_the_version_and_the_tools() {
    let text = diagnostics("9.9.9");
    assert!(text.starts_with("Riff 9.9.9 (native)"));
    assert!(text.contains("yt-dlp: "));
    assert!(text.contains("ffmpeg: "));
  }
}
