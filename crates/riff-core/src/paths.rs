//! Dossiers de données et de configuration, recherche des binaires.

use crate::config::read_config;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const RIFF_DATA_ENTRIES: &[&str] = &[
  "tracks",
  "custom-lyrics",
  "lyrics",
  "thumbnails",
  "library",
  "bin",
  "plugins",
  "themes",
  "logs",
  "i18n",
  "metadata.json",
  "library.json",
  "startup.json",
  "user-data.json",
  "native-settings.json",
  "yt-cookies.txt",
  "google-oauth.enc",
  "spotify-auth.enc",
  ".yt-logged-out",
  ".rifftutorial",
];

pub fn home_dir() -> PathBuf {
  dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Nom du dossier de Riff (natif) sous Windows et macOS : distinct de `Riff`, que se partagent Riff d'origine et Riff Tauri.
pub const NATIVE_DIR_NAME: &str = "Riff (natif)";
/// Nom du dossier de Riff (natif) sous Linux, dans `XDG_DATA_HOME` et `XDG_CONFIG_HOME`.
pub const LINUX_DIR_NAME: &str = "riff-native";

/// Système dont on calcule les dossiers : un paramètre pour tester les trois depuis une seule machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
  Windows,
  MacOs,
  /// Linux et autres systèmes XDG.
  Unix,
}

impl Platform {
  #[must_use]
  pub const fn current() -> Self {
    if cfg!(target_os = "windows") {
      Self::Windows
    } else if cfg!(target_os = "macos") {
      Self::MacOs
    } else {
      Self::Unix
    }
  }
}

/// Dossiers de base du système, résolus une fois depuis l'environnement. Aucun accès disque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
  pub home: PathBuf,
  /// `%LOCALAPPDATA%` (Windows).
  pub local_app_data: PathBuf,
  /// `%APPDATA%` (Windows).
  pub app_data: PathBuf,
  /// `$XDG_DATA_HOME`, sinon `~/.local/share`.
  pub xdg_data: PathBuf,
  /// `$XDG_CONFIG_HOME`, sinon `~/.config`.
  pub xdg_config: PathBuf,
}

impl Roots {
  #[must_use]
  pub fn from_env() -> Self {
    let home = home_dir();
    let var = |name: &str| {
      env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    };
    Self {
      local_app_data: var("LOCALAPPDATA")
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| home.join("AppData").join("Local")),
      app_data: var("APPDATA")
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| home.join("AppData").join("Roaming")),
      xdg_data: var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share")),
      xdg_config: var("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
      home,
    }
  }

  fn mac_support(&self) -> PathBuf {
    self.home.join("Library").join("Application Support")
  }
}

/// Dossier de données de Riff (natif).
#[must_use]
pub fn data_dir_in(platform: Platform, roots: &Roots) -> PathBuf {
  match platform {
    Platform::Windows => roots.local_app_data.join(NATIVE_DIR_NAME),
    Platform::MacOs => roots.mac_support().join(NATIVE_DIR_NAME),
    Platform::Unix => roots.xdg_data.join(LINUX_DIR_NAME),
  }
}

/// Dossier de configuration de Riff (natif) : le même que les données sous Windows et macOS.
#[must_use]
pub fn config_dir_in(platform: Platform, roots: &Roots) -> PathBuf {
  match platform {
    Platform::Windows | Platform::MacOs => data_dir_in(platform, roots),
    Platform::Unix => roots.xdg_config.join(LINUX_DIR_NAME),
  }
}

/// Ancien dossier de données par défaut, celui de Riff d'origine et de Riff Tauri (et des premières versions de Riff natif).
#[must_use]
pub fn previous_data_dir_in(platform: Platform, roots: &Roots) -> PathBuf {
  match platform {
    Platform::Windows => roots.local_app_data.join("Riff"),
    Platform::MacOs => roots.mac_support().join("Riff"),
    Platform::Unix => roots.home.join(".cache").join("devsize"),
  }
}

/// Ancien dossier de configuration par défaut.
#[must_use]
pub fn previous_config_dir_in(platform: Platform, roots: &Roots) -> PathBuf {
  match platform {
    Platform::Windows | Platform::MacOs => previous_data_dir_in(platform, roots),
    Platform::Unix => roots.xdg_config.join("devsize"),
  }
}

/// `userData` d'Electron de Riff d'origine : `appData` + nom de l'application (`app.setName('Riff')`, `main.js:24`).
/// Windows : vérifié sur une machine avec Riff installé ; macOS et Linux : déduits de la documentation d'Electron.
#[must_use]
pub fn electron_user_data_dir_in(platform: Platform, roots: &Roots) -> PathBuf {
  match platform {
    Platform::Windows => roots.app_data.join("Riff"),
    Platform::MacOs => roots.mac_support().join("Riff"),
    Platform::Unix => roots.xdg_config.join("Riff"),
  }
}

pub fn default_data_dir() -> PathBuf {
  data_dir_in(Platform::current(), &Roots::from_env())
}

pub fn config_dir() -> PathBuf {
  config_dir_in(Platform::current(), &Roots::from_env())
}

/// Ancien dossier de données par défaut (voir [`previous_data_dir_in`]).
pub fn previous_data_dir() -> PathBuf {
  previous_data_dir_in(Platform::current(), &Roots::from_env())
}

/// Ancien dossier de configuration par défaut (voir [`previous_config_dir_in`]).
pub fn previous_config_dir() -> PathBuf {
  previous_config_dir_in(Platform::current(), &Roots::from_env())
}

/// `userData` d'Electron de Riff d'origine (voir [`electron_user_data_dir_in`]).
pub fn electron_user_data_dir() -> PathBuf {
  electron_user_data_dir_in(Platform::current(), &Roots::from_env())
}

/// `RIFF_DATA_DIR` posé et non vide : profil d'essai ou instance multiple.
pub fn data_dir_override() -> Option<PathBuf> {
  let value = env::var("RIFF_DATA_DIR").ok()?;
  let trimmed = value.trim();
  (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// Dossier des fichiers d'état de l'interface (`storage.json`, `window-state.json`, `cache.json`, `local-covers`).
/// `RIFF_DATA_DIR` le déplace avec les données : essais et instances multiples n'écrasent pas l'instance réelle.
/// Le réglage `dataDir` de `config.json` ne le déplace pas, comme dans Riff.
pub fn state_dir() -> PathBuf {
  if data_dir_override().is_some() {
    resolve_data_dir()
  } else {
    config_dir()
  }
}

pub fn config_path() -> PathBuf {
  config_dir().join("config.json")
}

pub fn resolve_data_dir() -> PathBuf {
  if let Some(p) = data_dir_override() {
    return fs::canonicalize(&p).unwrap_or(p);
  }
  let cfg = read_config();
  if let Some(configured) = cfg.get("dataDir").and_then(Value::as_str) {
    let trimmed = configured.trim();
    if !trimmed.is_empty() {
      return PathBuf::from(trimmed);
    }
  }
  default_data_dir()
}

pub fn default_download_dir() -> PathBuf {
  if cfg!(target_os = "windows") {
    let d_drive = Path::new("D:\\");
    if d_drive.is_dir() {
      return d_drive.join("Downloads");
    }
  }
  dirs::download_dir().unwrap_or_else(|| home_dir().join("Downloads"))
}

pub fn tracks_dir() -> PathBuf {
  resolve_data_dir().join("tracks")
}

pub fn custom_lyrics_dir() -> PathBuf {
  resolve_data_dir().join("custom-lyrics")
}

pub fn thumbnails_dir() -> PathBuf {
  resolve_data_dir().join("thumbnails")
}

pub fn metadata_path() -> PathBuf {
  resolve_data_dir().join("metadata.json")
}

pub fn library_dir() -> PathBuf {
  resolve_data_dir().join("library")
}

pub fn library_path() -> PathBuf {
  resolve_data_dir().join("library.json")
}

pub fn themes_dir() -> PathBuf {
  resolve_data_dir().join("themes")
}

pub fn plugins_dir() -> PathBuf {
  resolve_data_dir().join("plugins")
}

pub fn log_dir() -> PathBuf {
  resolve_data_dir().join("logs")
}

pub fn log_path() -> PathBuf {
  log_dir().join("riff.log")
}

pub fn startup_path() -> PathBuf {
  resolve_data_dir().join("startup.json")
}

pub fn storage_path() -> PathBuf {
  state_dir().join("storage.json")
}

pub fn cache_path() -> PathBuf {
  state_dir().join("cache.json")
}

pub fn window_state_path() -> PathBuf {
  state_dir().join("window-state.json")
}

pub fn local_covers_dir() -> PathBuf {
  state_dir().join("local-covers")
}

pub fn copy_dir_contents(from_dir: &Path, to_dir: &Path) -> std::io::Result<()> {
  fs::create_dir_all(to_dir)?;
  for entry in fs::read_dir(from_dir)? {
    let entry = entry?;
    let name = entry.file_name();
    if name == "config.json" {
      continue;
    }
    let src = entry.path();
    let dest = to_dir.join(&name);
    let ft = entry.file_type()?;
    if ft.is_dir() {
      copy_dir_contents(&src, &dest)?;
    } else if ft.is_file() {
      fs::copy(&src, &dest)?;
    }
  }
  Ok(())
}

pub fn copy_data_entries(from_dir: &Path, to_dir: &Path) -> std::io::Result<()> {
  fs::create_dir_all(to_dir)?;
  for name in RIFF_DATA_ENTRIES {
    let src = from_dir.join(name);
    let Ok(meta) = fs::metadata(&src) else {
      continue;
    };
    let dest = to_dir.join(name);
    if meta.is_dir() {
      copy_dir_contents(&src, &dest)?;
    } else if meta.is_file() {
      fs::copy(&src, &dest)?;
    }
  }
  Ok(())
}

pub fn remove_data_entries(dir: &Path) {
  for name in RIFF_DATA_ENTRIES {
    let target = dir.join(name);
    if let Ok(meta) = fs::metadata(&target) {
      if meta.is_dir() {
        let _ = fs::remove_dir_all(&target);
      } else {
        let _ = fs::remove_file(&target);
      }
    }
  }
}

pub fn resolve_binary(name: &str) -> Option<PathBuf> {
  let exe = if cfg!(target_os = "windows") {
    format!("{name}.exe")
  } else {
    name.to_string()
  };
  let env_key = format!(
    "RIFF_{}",
    name
      .chars()
      .filter(|c| c.is_ascii_alphanumeric())
      .collect::<String>()
      .to_uppercase()
  );

  let mut candidates: Vec<PathBuf> = Vec::new();
  if let Ok(env_val) = env::var(&env_key)
    && !env_val.trim().is_empty()
  {
    candidates.push(PathBuf::from(env_val.trim()));
  }
  candidates.push(resolve_data_dir().join("bin").join(&exe));
  if let Ok(cur_exe) = env::current_exe()
    && let Some(exe_dir) = cur_exe.parent()
  {
    candidates.push(exe_dir.join("bin").join(&exe));
    candidates.push(exe_dir.join(&exe));
  }

  for c in candidates {
    if c.is_file() {
      return Some(c);
    }
  }

  let path_var = env::var_os("PATH").or_else(|| env::var_os("Path"))?;
  let exts: &[&str] = if cfg!(target_os = "windows") {
    &[".exe", ""]
  } else {
    &[""]
  };
  for dir in env::split_paths(&path_var) {
    for ext in exts {
      let candidate = dir.join(format!("{name}{ext}"));
      if candidate.is_file() {
        return Some(candidate);
      }
    }
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;

  fn roots() -> Roots {
    Roots {
      home: PathBuf::from("/h"),
      local_app_data: PathBuf::from("/local"),
      app_data: PathBuf::from("/roaming"),
      xdg_data: PathBuf::from("/xdg-data"),
      xdg_config: PathBuf::from("/xdg-config"),
    }
  }

  #[test]
  fn native_dirs_per_platform() {
    let r = roots();
    let dirs = |p| (data_dir_in(p, &r), config_dir_in(p, &r));
    assert_eq!(
      dirs(Platform::Windows),
      (
        PathBuf::from("/local/Riff (natif)"),
        PathBuf::from("/local/Riff (natif)")
      )
    );
    assert_eq!(
      dirs(Platform::MacOs),
      (
        PathBuf::from("/h/Library/Application Support/Riff (natif)"),
        PathBuf::from("/h/Library/Application Support/Riff (natif)")
      )
    );
    assert_eq!(
      dirs(Platform::Unix),
      (
        PathBuf::from("/xdg-data/riff-native"),
        PathBuf::from("/xdg-config/riff-native")
      )
    );
  }

  #[test]
  fn previous_and_electron_dirs_per_platform() {
    let r = roots();
    assert_eq!(
      previous_data_dir_in(Platform::Windows, &r),
      PathBuf::from("/local/Riff")
    );
    assert_eq!(
      electron_user_data_dir_in(Platform::Windows, &r),
      PathBuf::from("/roaming/Riff")
    );
    assert_eq!(
      previous_data_dir_in(Platform::MacOs, &r),
      PathBuf::from("/h/Library/Application Support/Riff")
    );
    assert_eq!(
      electron_user_data_dir_in(Platform::MacOs, &r),
      PathBuf::from("/h/Library/Application Support/Riff")
    );
    assert_eq!(
      previous_data_dir_in(Platform::Unix, &r),
      PathBuf::from("/h/.cache/devsize")
    );
    assert_eq!(
      previous_config_dir_in(Platform::Unix, &r),
      PathBuf::from("/xdg-config/devsize")
    );
    assert_eq!(
      electron_user_data_dir_in(Platform::Unix, &r),
      PathBuf::from("/xdg-config/Riff")
    );
  }

  #[test]
  fn resolve_binary_ignores_current_dir_bin() {
    let dir = TempDir::new("cwd-bin");
    let bin_dir = dir.join("bin");
    let _ = std::fs::create_dir_all(&bin_dir);
    let exe_name = if cfg!(windows) {
      "riff-fake-tool-xyz.exe"
    } else {
      "riff-fake-tool-xyz"
    };
    let fake = bin_dir.join(exe_name);
    let _ = std::fs::write(&fake, b"fake");
    let prev = env::current_dir().ok();
    let _ = env::set_current_dir(&dir);
    let found = resolve_binary("riff-fake-tool-xyz");
    if let Some(p) = prev {
      let _ = env::set_current_dir(p);
    }
    assert_eq!(found, None);
  }
}
