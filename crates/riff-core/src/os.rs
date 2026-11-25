//! Ouverture de dossiers et d'URL par le système.

use std::fs;
use std::path::Path;
use std::process::Command;

pub fn open_folder(path: &Path) {
  let _ = fs::create_dir_all(path);
  #[cfg(target_os = "windows")]
  {
    let _ = Command::new("explorer").arg(path).spawn();
  }
  #[cfg(target_os = "macos")]
  {
    let _ = Command::new("open").arg(path).spawn();
  }
  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  {
    let _ = Command::new("xdg-open").arg(path).spawn();
  }
}

pub fn is_allowed_external_url(u: &str) -> bool {
  let lower = u.trim().to_ascii_lowercase();
  lower.starts_with("https://accounts.spotify.com/")
    || lower.starts_with("https://accounts.google.com/")
    || lower.starts_with("https://last.fm/")
    || lower.starts_with("https://www.last.fm/")
    || lower.starts_with("https://github.com/")
}

pub fn open_external_url(u: &str) {
  if !is_allowed_external_url(u) {
    return;
  }
  #[cfg(target_os = "windows")]
  {
    let _ = Command::new("rundll32")
      .args(["url.dll,FileProtocolHandler", u.trim()])
      .spawn();
  }
  #[cfg(target_os = "macos")]
  {
    let _ = Command::new("open").arg(u.trim()).spawn();
  }
  #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
  {
    let _ = Command::new("xdg-open").arg(u.trim()).spawn();
  }
}
