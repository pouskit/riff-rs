//! Raccourcis du Menu Démarrer et du Bureau (`launcher.js`). Windows : `.lnk`. Linux : fichier `.desktop`. macOS : non pris en charge.
//! Le nom diffère de l'original (`Riff.lnk`) tant que le vrai Riff peut coexister : `Riff (natif)`.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Nom des raccourcis, sans extension.
pub const NAME: &str = "Riff (natif)";
/// Identité de la barre des tâches. Différente de celle du vrai Riff (`io.github.rootscripts.riff`) : les deux
/// applications ne se regroupent pas.
pub const APP_ID: &str = "io.github.rootscripts.riff.native";
const COMMENT: &str = "Riff - music player";
#[cfg(target_os = "linux")]
const ICON_PNG: &[u8] = include_bytes!("../../assets/icon-512.png");

/// État du lanceur, au format de `launcher-info` de l'original.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
  pub platform: &'static str,
  pub can_install: bool,
  pub installed: bool,
  pub reason: String,
}

/// Dossiers d'écriture. Les essais passent des dossiers temporaires ; l'application passe [`system_dirs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
  /// Windows : programmes du Menu Démarrer. Linux : `~/.local/share/applications`.
  pub menu: PathBuf,
  pub desktop: Option<PathBuf>,
  /// Linux : dossier des icônes (`~/.local/share/icons`).
  pub icons: Option<PathBuf>,
}

/// Dossiers réels de l'utilisateur. `None` : pas de lanceur sur ce système.
#[must_use]
pub fn system_dirs() -> Option<Dirs> {
  if cfg!(windows) {
    Some(Dirs {
      menu: dirs::data_dir()?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs"),
      desktop: dirs::desktop_dir().filter(|dir| dir.exists()),
      icons: None,
    })
  } else if cfg!(target_os = "linux") {
    let data = dirs::data_dir()?;
    Some(Dirs {
      menu: data.join("applications"),
      desktop: None,
      icons: Some(data.join("icons")),
    })
  } else {
    None
  }
}

fn menu_entry(dirs: &Dirs) -> PathBuf {
  if cfg!(windows) {
    dirs.menu.join(format!("{NAME}.lnk"))
  } else {
    dirs.menu.join("riff-native.desktop")
  }
}

/// Le lanceur peut-il s'installer, et l'est-il déjà. Rien n'est écrit.
#[must_use]
pub fn info_for(dirs: Option<&Dirs>) -> Info {
  let platform = if cfg!(windows) {
    "win32"
  } else if cfg!(target_os = "macos") {
    "darwin"
  } else {
    "linux"
  };
  match dirs {
    Some(dirs) => Info {
      platform,
      can_install: true,
      installed: menu_entry(dirs).exists(),
      reason: String::new(),
    },
    None => Info {
      platform,
      can_install: false,
      installed: false,
      reason: "Not supported on this system".into(),
    },
  }
}

#[must_use]
pub fn info() -> Info {
  info_for(system_dirs().as_ref())
}

/// Écrit les raccourcis de l'exécutable `exe`. Rend le chemin à montrer à l'utilisateur.
pub fn install_into(dirs: &Dirs, exe: &Path) -> Result<PathBuf, String> {
  std::fs::create_dir_all(&dirs.menu).map_err(|error| error.to_string())?;
  let entry = menu_entry(dirs);
  write_entry(&entry, exe, dirs)?;
  let mut shown = entry;
  if let Some(desktop) = &dirs.desktop {
    let link = desktop.join(format!("{NAME}.lnk"));
    if write_entry(&link, exe, dirs).is_ok() {
      shown = link;
    }
  }
  Ok(shown)
}

/// Installe dans le vrai Menu Démarrer et sur le Bureau. N'est appelé que par un clic de l'utilisateur.
pub fn install() -> Result<PathBuf, String> {
  let dirs = system_dirs().ok_or("Cannot install launcher on this system")?;
  let exe = std::env::current_exe().map_err(|error| error.to_string())?;
  install_into(&dirs, &exe)
}

#[cfg(windows)]
fn write_entry(path: &Path, exe: &Path, _dirs: &Dirs) -> Result<(), String> {
  super::platform::windows_shell::create_shortcut(path, exe, COMMENT, APP_ID)
}

#[cfg(target_os = "linux")]
fn write_entry(path: &Path, exe: &Path, dirs: &Dirs) -> Result<(), String> {
  let icon = match &dirs.icons {
    Some(icons) => {
      let folder = icons.join("hicolor").join("512x512").join("apps");
      std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
      let file = folder.join("riff-native.png");
      std::fs::write(&file, ICON_PNG).map_err(|error| error.to_string())?;
      file.to_string_lossy().into_owned()
    }
    None => "riff-native".to_string(),
  };
  std::fs::write(path, desktop_entry(exe, &icon)).map_err(|error| error.to_string())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn write_entry(_path: &Path, _exe: &Path, _dirs: &Dirs) -> Result<(), String> {
  Err("Cannot install launcher on this system".into())
}

/// Contenu d'un fichier `.desktop` (spécification freedesktop). Le chemin est mis entre guillemets avec ses
/// caractères spéciaux protégés ; `%F` reçoit les fichiers ouverts depuis le gestionnaire.
#[must_use]
pub fn desktop_entry(exe: &Path, icon: &str) -> String {
  let quoted: String = exe
    .to_string_lossy()
    .chars()
    .flat_map(|c| match c {
      '"' | '`' | '$' | '\\' => vec!['\\', c],
      '%' => vec!['%', '%'],
      other => vec![other],
    })
    .collect();
  format!(
    "[Desktop Entry]\nType=Application\nName={NAME}\nComment={COMMENT}\nExec=\"{quoted}\" %F\nIcon={icon}\nTerminal=false\nCategories=AudioVideo;Audio;Player;\nMimeType=audio/mpeg;audio/flac;audio/x-flac;audio/ogg;audio/mp4;audio/x-wav;\nStartupWMClass=riff-app\n"
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(windows)]
  fn temp(name: &str) -> crate::test_temp::TempDir {
    crate::test_temp::TempDir::new(&format!("launcher-{name}"))
  }

  #[test]
  fn the_name_does_not_overwrite_the_real_riff_shortcut() {
    assert_ne!(NAME, "Riff");
    assert_ne!(APP_ID, "io.github.rootscripts.riff");
  }

  #[test]
  fn without_folders_the_launcher_cannot_install() {
    let info = info_for(None);
    assert!(!info.can_install && !info.installed);
    assert_eq!(info.reason, "Not supported on this system");
  }

  #[test]
  fn info_serialises_like_the_original() {
    let json = serde_json::to_value(info_for(None)).expect("json");
    assert_eq!(json["canInstall"], false);
    assert!(
      json.get("installed").is_some()
        && json.get("reason").is_some()
        && json.get("platform").is_some()
    );
  }

  #[test]
  fn desktop_entry_quotes_the_executable_and_declares_a_player() {
    let entry = desktop_entry(Path::new("/opt/my \"riff\"/riff-app"), "riff-native");
    assert!(entry.starts_with("[Desktop Entry]\n"));
    assert!(entry.contains("Exec=\"/opt/my \\\"riff\\\"/riff-app\" %F\n"));
    assert!(entry.contains("Name=Riff (natif)\n"));
    assert!(entry.contains("Categories=AudioVideo;Audio;Player;"));
    assert!(entry.contains("Terminal=false"));
  }

  #[test]
  fn desktop_entry_doubles_percent_signs() {
    assert!(desktop_entry(Path::new("/a%b/x"), "i").contains("Exec=\"/a%%b/x\" %F"));
  }

  #[cfg(windows)]
  #[test]
  fn writes_readable_shortcuts_with_the_taskbar_identity_into_temp_folders() {
    let root = temp("win");
    let dirs = Dirs {
      menu: root.join("menu"),
      desktop: Some(root.join("desktop")),
      icons: None,
    };
    std::fs::create_dir_all(dirs.desktop.as_ref().unwrap()).unwrap();
    assert!(!info_for(Some(&dirs)).installed);
    let exe = std::env::current_exe().unwrap();
    let shown = install_into(&dirs, &exe).expect("install");
    assert!(shown.starts_with(root.join("desktop")), "{shown:?}");
    assert!(info_for(Some(&dirs)).installed);
    for lnk in [dirs.menu.join("Riff (natif).lnk"), shown] {
      let (target, app_id) =
        super::super::platform::windows_shell::read_shortcut(&lnk).expect("read");
      assert_eq!(Path::new(&target), exe.as_path(), "{lnk:?}");
      assert_eq!(app_id, APP_ID, "{lnk:?}");
    }
  }

  #[cfg(windows)]
  #[test]
  fn a_missing_desktop_folder_still_installs_the_menu_entry() {
    let root = temp("nodesktop");
    let dirs = Dirs {
      menu: root.join("menu"),
      desktop: None,
      icons: None,
    };
    let shown = install_into(&dirs, &std::env::current_exe().unwrap()).expect("install");
    assert_eq!(shown, dirs.menu.join("Riff (natif).lnk"));
  }
}
