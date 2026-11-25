//! Fichiers de configuration, démarrage et tutoriel.

use crate::paths::{config_dir, config_path, home_dir, resolve_data_dir, startup_path};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::PathBuf;

pub fn read_config() -> Value {
  let path = config_path();
  if let Ok(raw) = fs::read_to_string(path)
    && let Ok(parsed) = serde_json::from_str::<Value>(&raw)
    && parsed.is_object()
  {
    return parsed;
  }
  json!({})
}

pub fn write_config(cfg: &Value) -> std::io::Result<()> {
  let dir = config_dir();
  fs::create_dir_all(&dir)?;
  let text = serde_json::to_string_pretty(cfg).map_err(std::io::Error::other)?;
  let path = config_path();
  let tmp = path.with_extension("json.tmp");
  fs::write(&tmp, text)?;
  fs::rename(&tmp, path)
}

pub fn set_data_dir_config(dir: Option<&str>) -> std::io::Result<PathBuf> {
  let mut cfg = read_config();
  if !cfg.is_object() {
    cfg = json!({});
  }
  if let Some(obj) = cfg.as_object_mut() {
    match dir.map(str::trim).filter(|s| !s.is_empty()) {
      Some(d) => {
        obj.insert("dataDir".to_string(), Value::String(d.to_string()));
      }
      None => {
        obj.remove("dataDir");
      }
    }
  }
  write_config(&cfg)?;
  Ok(resolve_data_dir())
}

pub fn resolve_download_dir() -> Option<PathBuf> {
  let cfg = read_config();
  if let Some(configured) = cfg.get("downloadDir").and_then(Value::as_str) {
    let trimmed = configured.trim();
    if !trimmed.is_empty() {
      return Some(PathBuf::from(trimmed));
    }
  }
  None
}

pub fn set_download_dir_config(dir: Option<&str>) -> Option<PathBuf> {
  let mut cfg = read_config();
  if !cfg.is_object() {
    cfg = json!({});
  }
  if let Some(obj) = cfg.as_object_mut() {
    match dir.map(str::trim).filter(|s| !s.is_empty()) {
      Some(d) => {
        obj.insert("downloadDir".to_string(), Value::String(d.to_string()));
      }
      None => {
        obj.remove("downloadDir");
      }
    }
  }
  let _ = write_config(&cfg);
  resolve_download_dir()
}

fn flag(key: &str, default: bool) -> bool {
  read_config()
    .get(key)
    .and_then(Value::as_bool)
    .unwrap_or(default)
}

fn set_flag(key: &str, value: bool) {
  let mut cfg = read_config();
  if let Some(obj) = cfg.as_object_mut() {
    obj.insert(key.to_string(), Value::Bool(value));
  }
  let _ = write_config(&cfg);
}

/// Envoi de textes aux services de traduction. Coupé par défaut.
pub fn translation_enabled() -> bool {
  flag("translationEnabled", false)
}

pub fn set_translation_enabled(enabled: bool) {
  set_flag("translationEnabled", enabled);
}

/// Vérification automatique de yt-dlp. Active par défaut (YouTube casse souvent les anciennes versions).
pub fn yt_dlp_auto_update() -> bool {
  flag("ytdlpAutoUpdate", true)
}

pub fn set_yt_dlp_auto_update(enabled: bool) {
  set_flag("ytdlpAutoUpdate", enabled);
}

pub fn read_startup_config() -> Value {
  if let Ok(raw) = fs::read_to_string(startup_path())
    && let Ok(parsed) = serde_json::from_str::<Value>(&raw)
    && parsed.is_object()
  {
    return parsed;
  }
  json!({})
}

pub fn write_startup_config(patch: &Map<String, Value>) -> bool {
  let data_dir = resolve_data_dir();
  if fs::create_dir_all(&data_dir).is_err() {
    return false;
  }
  let mut current = read_startup_config();
  if !current.is_object() {
    current = json!({});
  }
  if let Some(obj) = current.as_object_mut() {
    for (k, v) in patch {
      obj.insert(k.clone(), v.clone());
    }
  }
  if let Ok(s) = serde_json::to_string(&current) {
    fs::write(startup_path(), s).is_ok()
  } else {
    false
  }
}

pub fn tutorial_markers() -> Vec<PathBuf> {
  let primary = if cfg!(target_os = "windows") {
    PathBuf::from("C:\\Riff\\.rifftutorial")
  } else {
    home_dir().join(".rifftutorial")
  };
  let fallback = resolve_data_dir().join(".rifftutorial");
  vec![primary, fallback]
}

pub fn tutorial_done(app_version: &str) -> bool {
  for file in tutorial_markers() {
    if let Ok(content) = fs::read_to_string(file)
      && content.trim() == app_version
    {
      return true;
    }
  }
  false
}

pub fn mark_tutorial_done(app_version: &str) -> bool {
  for file in tutorial_markers() {
    if let Some(parent) = file.parent()
      && fs::create_dir_all(parent).is_ok()
      && fs::write(&file, app_version).is_ok()
    {
      return true;
    }
  }
  false
}
