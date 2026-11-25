use crate::paths;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThemeIndexEntry {
  id: String,
  name: String,
  enabled: bool,
}

fn ensure_dir() -> PathBuf {
  let dir = paths::themes_dir();
  let _ = fs::create_dir_all(&dir);
  dir
}

fn index_file() -> PathBuf {
  paths::themes_dir().join("index.json")
}

fn sanitize_theme_id(id: &str) -> Option<String> {
  let trimmed = id.trim();
  if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..")
  {
    return None;
  }
  Some(trimmed.to_string())
}

fn read_index_entries() -> Vec<ThemeIndexEntry> {
  let Ok(raw) = fs::read_to_string(index_file()) else {
    return Vec::new();
  };
  serde_json::from_str::<Vec<ThemeIndexEntry>>(&raw).unwrap_or_default()
}

fn read_list() -> Vec<Value> {
  let dir = ensure_dir();
  let entries = read_index_entries();
  let mut out = Vec::new();
  for item in entries {
    let Some(safe_id) = sanitize_theme_id(&item.id) else {
      continue;
    };
    let css_path = dir.join(format!("{safe_id}.css"));
    if let Ok(css) = fs::read_to_string(&css_path) {
      out.push(json!({
        "id": safe_id,
        "name": item.name,
        "enabled": item.enabled,
        "css": css,
      }));
    }
  }
  out
}

fn write_list(list: &[ThemeIndexEntry]) {
  let _ = ensure_dir();
  let clean: Vec<Value> = list
    .iter()
    .map(|t| {
      json!({
        "id": t.id,
        "name": t.name,
        "enabled": t.enabled,
      })
    })
    .collect();
  if let Ok(text) = serde_json::to_string_pretty(&clean) {
    let _ = fs::write(index_file(), text);
  }
}

fn parse_theme_name(raw: &str, fallback: &str) -> String {
  for line in raw.lines() {
    let lower = line.to_ascii_lowercase();
    if let Some(pos) = lower.find("@name") {
      let after = &line[pos + 5..];
      if after.starts_with(char::is_whitespace) {
        let name = after.trim().trim_end_matches("*/").trim();
        if !name.is_empty() {
          return name.to_string();
        }
      }
    }
  }
  fallback.to_string()
}

fn generate_theme_id() -> String {
  let ms = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_millis())
    .unwrap_or(0);
  let mut rand_bytes = [0u8; 4];
  let _ = getrandom::fill(&mut rand_bytes);
  let suffix: String = rand_bytes
    .iter()
    .map(|b| {
      const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
      CHARS[(*b as usize) % CHARS.len()] as char
    })
    .collect();
  format!("theme_{ms}_{suffix}")
}

pub fn list_themes() -> Vec<Value> {
  read_list()
}

/// Importe les thèmes des fichiers donnés et renvoie la liste à jour.
pub fn import_themes_from(paths: Vec<PathBuf>) -> Vec<Value> {
  let dir = ensure_dir();
  if paths.is_empty() {
    return read_list();
  }

  let mut list: Vec<ThemeIndexEntry> = read_index_entries()
    .into_iter()
    .filter(|item| {
      sanitize_theme_id(&item.id)
        .map(|id| dir.join(format!("{id}.css")).is_file())
        .unwrap_or(false)
    })
    .collect();

  for src in paths {
    let Ok(raw) = fs::read_to_string(&src) else {
      continue;
    };
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("Theme");
    let name = parse_theme_name(&raw, stem);
    let id = generate_theme_id();
    if fs::write(dir.join(format!("{id}.css")), &raw).is_ok() {
      list.push(ThemeIndexEntry {
        id,
        name,
        enabled: true,
      });
    }
  }

  write_list(&list);
  read_list()
}

pub fn set_theme_enabled(id: &str, on: bool) {
  let mut list = read_index_entries();
  if let Some(entry) = list.iter_mut().find(|t| t.id == id) {
    entry.enabled = on;
    write_list(&list);
  }
}

pub fn disable_all_themes() {
  let mut list = read_index_entries();
  for entry in &mut list {
    entry.enabled = false;
  }
  write_list(&list);
}

pub fn remove_theme(id: &str) {
  let Some(safe_id) = sanitize_theme_id(id) else {
    return;
  };
  let mut list = read_index_entries();
  list.retain(|t| t.id != safe_id);
  let _ = fs::remove_file(paths::themes_dir().join(format!("{safe_id}.css")));
  write_list(&list);
}
