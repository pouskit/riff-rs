//! Reprise de l'ancien dossier par défaut dans le dossier de Riff (natif), et choix du `storage.json` à importer.
//!
//! L'ancien dossier (`%LOCALAPPDATA%\Riff` sous Windows) appartient aussi à Riff d'origine et à Riff Tauri :
//! il n'est jamais déplacé, modifié ni supprimé. Seules des copies (ou des liens physiques) en partent.

use crate::paths::{self, Platform, Roots};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Posé dans le nouveau dossier par la reprise, pour ne jamais la refaire.
pub const MARKER: &str = ".reprise-riff-natif";

/// Fichiers qui n'existent que si Riff (natif) a tourné dans ce dossier.
const OWN_FILES: [&str; 3] = ["native-settings.json", "user-data.json", MARKER];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
  /// Copie. Un échec annule la reprise.
  Copy,
  /// Copie dont l'échec est toléré : cache ou binaire que l'application sait refaire.
  Optional,
  /// Lien physique (copie si le volume diffère) : fichiers jamais réécrits sur place, sans doublon de gigaoctets.
  Link,
}

/// Entrées de l'ancien dossier de configuration (côté `config_dir`).
const CONFIG_ENTRIES: [(&str, Mode); 3] = [
  ("config.json", Mode::Copy),
  ("window-state.json", Mode::Copy),
  ("local-covers", Mode::Optional),
];

/// Entrées de l'ancien dossier de données. Absentes de cette liste, volontairement :
/// `tracks/`, `thumbnails/`, `metadata.json` (cache que « Clear cache » vide, retéléchargé à la lecture),
/// `logs/`, `storage.json`, `cache.json` (état de Riff), `google-oauth.enc` et `spotify-auth.enc` (chiffrés par Electron,
/// illisibles ici : les jetons passent par le coffre du système).
const DATA_ENTRIES: [(&str, Mode); 15] = [
  ("native-settings.json", Mode::Copy),
  ("user-data.json", Mode::Copy),
  // `library/` se lit par nom de fichier relatif au dossier de données, avec `library.json` : les deux vont ensemble.
  ("library.json", Mode::Copy),
  ("library", Mode::Link),
  ("custom-lyrics", Mode::Copy),
  ("lyrics", Mode::Optional),
  ("plugins", Mode::Copy),
  ("themes", Mode::Copy),
  ("startup.json", Mode::Copy),
  ("yt-cookies.txt", Mode::Copy),
  // Jeton Spotify de DPAPI, lié au compte Windows et non au dossier : la copie reste lisible (`services/spotify.rs:83`).
  ("spotify-auth.dpapi", Mode::Copy),
  (".yt-logged-out", Mode::Copy),
  (".rifftutorial", Mode::Optional),
  ("i18n", Mode::Optional),
  // `yt-dlp -U` réécrit ces binaires sur place : copie, jamais de lien.
  ("bin", Mode::Optional),
];

/// Les quatre dossiers en jeu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
  pub old_data: PathBuf,
  pub old_config: PathBuf,
  pub new_data: PathBuf,
  pub new_config: PathBuf,
}

impl Layout {
  #[must_use]
  pub fn new(platform: Platform, roots: &Roots) -> Self {
    Self {
      old_data: paths::previous_data_dir_in(platform, roots),
      old_config: paths::previous_config_dir_in(platform, roots),
      new_data: paths::data_dir_in(platform, roots),
      new_config: paths::config_dir_in(platform, roots),
    }
  }
}

/// Issue de la reprise, à journaliser par l'appelant (le journal n'existe pas encore au moment de la reprise).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adoption {
  /// Rien à faire : profil d'essai, déjà repris, ou ancien dossier sans données de Riff (natif).
  Skipped,
  Done {
    from: PathBuf,
    /// Nombre d'entrées reprises.
    entries: usize,
    /// Entrées facultatives qui n'ont pas pu être copiées.
    skipped: Vec<String>,
  },
  Failed {
    from: PathBuf,
    error: String,
  },
}

/// Reprise au lancement. Sans effet avec `RIFF_DATA_DIR` : le profil d'essai ne touche jamais aux dossiers réels.
/// À appeler avant tout accès au dossier de données. Ne panique pas et ne bloque jamais le démarrage.
#[must_use]
pub fn adopt_previous_data() -> Adoption {
  if paths::data_dir_override().is_some() {
    return Adoption::Skipped;
  }
  adopt_previous(&Layout::new(Platform::current(), &Roots::from_env()))
}

/// Reprise entre quatre dossiers donnés (testable sur des dossiers temporaires).
#[must_use]
pub fn adopt_previous(layout: &Layout) -> Adoption {
  if has_own_entries(&layout.new_data) {
    return Adoption::Skipped;
  }
  // Un `dataDir` dans l'ancien `config.json` : les données de Riff (natif) y vivent, la reprise ne les copie pas.
  let custom = custom_data_dir(&layout.old_config);
  let source_data = custom.clone().unwrap_or_else(|| layout.old_data.clone());
  if !OWN_FILES[..2]
    .iter()
    .any(|name| source_data.join(name).is_file())
  {
    return Adoption::Skipped;
  }
  let data_entries: &[(&str, Mode)] = if custom.is_some() { &[] } else { &DATA_ENTRIES };
  let marker = format!("from={}\n", layout.old_data.display());
  let mut entries = 0;
  let mut skipped = Vec::new();
  let outcome = if layout.new_config == layout.new_data {
    let all: Vec<(&str, Mode)> = CONFIG_ENTRIES.iter().chain(data_entries).copied().collect();
    run_job(&layout.old_data, &layout.new_data, &all, Some(&marker))
      .map(|done| accumulate(&mut entries, &mut skipped, done))
  } else {
    run_job(
      &layout.old_config,
      &layout.new_config,
      &CONFIG_ENTRIES,
      None,
    )
    .map(|done| accumulate(&mut entries, &mut skipped, done))
    .and_then(|()| {
      run_job(
        &layout.old_data,
        &layout.new_data,
        data_entries,
        Some(&marker),
      )
      .map(|done| accumulate(&mut entries, &mut skipped, done))
    })
  };
  match outcome {
    Ok(()) => Adoption::Done {
      from: layout.old_data.clone(),
      entries,
      skipped,
    },
    Err(err) => Adoption::Failed {
      from: layout.old_data.clone(),
      error: err.to_string(),
    },
  }
}

fn accumulate(entries: &mut usize, skipped: &mut Vec<String>, done: JobDone) {
  *entries += done.copied;
  skipped.extend(done.skipped);
}

fn has_own_entries(dir: &Path) -> bool {
  OWN_FILES.iter().any(|name| dir.join(name).exists())
}

fn custom_data_dir(old_config: &Path) -> Option<PathBuf> {
  let raw = fs::read_to_string(old_config.join("config.json")).ok()?;
  let parsed: Value = serde_json::from_str(&raw).ok()?;
  let configured = parsed.get("dataDir")?.as_str()?.trim();
  (!configured.is_empty()).then(|| PathBuf::from(configured))
}

struct JobDone {
  copied: usize,
  skipped: Vec<String>,
}

/// Copie `entries` de `src` dans un dossier voisin de `dest`, puis le renomme en `dest`.
fn run_job(
  src: &Path,
  dest: &Path,
  entries: &[(&str, Mode)],
  marker: Option<&str>,
) -> io::Result<JobDone> {
  let name = dest
    .file_name()
    .ok_or_else(|| io::Error::other("dossier de destination sans nom"))?;
  let mut staging_name = name.to_os_string();
  staging_name.push(".reprise");
  let staging = dest.with_file_name(staging_name);
  // Reste d'une reprise interrompue : il nous appartient.
  if staging.exists() {
    fs::remove_dir_all(&staging)?;
  }
  let built = fill_staging(src, &staging, entries, marker)
    .and_then(|done| place(&staging, dest).map(|()| done));
  if built.is_err() {
    let _ = fs::remove_dir_all(&staging);
  }
  built
}

fn fill_staging(
  src: &Path,
  staging: &Path,
  entries: &[(&str, Mode)],
  marker: Option<&str>,
) -> io::Result<JobDone> {
  fs::create_dir_all(staging)?;
  let mut done = JobDone {
    copied: 0,
    skipped: Vec::new(),
  };
  for (name, mode) in entries {
    let from = src.join(name);
    let Ok(meta) = fs::metadata(&from) else {
      continue;
    };
    let to = staging.join(name);
    let link = *mode == Mode::Link;
    let result = if meta.is_dir() {
      copy_tree(&from, &to, link)
    } else if meta.is_file() {
      fs::copy(&from, &to).map(|_| ())
    } else {
      continue;
    };
    match result {
      Ok(()) => done.copied += 1,
      Err(_) if *mode == Mode::Optional => {
        let _ = fs::remove_dir_all(&to).or_else(|_| fs::remove_file(&to));
        done.skipped.push((*name).to_string());
      }
      Err(err) => return Err(io::Error::new(err.kind(), format!("{name} : {err}"))),
    }
  }
  if let Some(text) = marker {
    fs::write(staging.join(MARKER), text)?;
  }
  Ok(done)
}

/// Copie récursive en lecture seule sur la source. `link` : lien physique d'abord, copie si le lien échoue.
fn copy_tree(from: &Path, to: &Path, link: bool) -> io::Result<()> {
  fs::create_dir_all(to)?;
  for entry in fs::read_dir(from)? {
    let entry = entry?;
    let src = entry.path();
    let dst = to.join(entry.file_name());
    let kind = entry.file_type()?;
    if kind.is_dir() {
      copy_tree(&src, &dst, link)?;
    } else if kind.is_file() {
      if link {
        if fs::hard_link(&src, &dst).is_err() {
          fs::copy(&src, &dst)?;
        }
      } else {
        fs::copy(&src, &dst)?;
      }
    }
  }
  Ok(())
}

/// Renomme `staging` en `dest`. Si `dest` existe déjà (créé par l'initialisation du cœur avant la reprise : journal,
/// dossiers vides), fusionne : tout fichier absent de `dest` y est déplacé, aucun fichier existant n'est écrasé.
/// Le marqueur passe en dernier.
fn place(staging: &Path, dest: &Path) -> io::Result<()> {
  if !dest.exists() {
    if let Some(parent) = dest.parent() {
      fs::create_dir_all(parent)?;
    }
    return fs::rename(staging, dest);
  }
  merge_into(staging, dest, true)?;
  let marker = staging.join(MARKER);
  if marker.exists() && !dest.join(MARKER).exists() {
    fs::rename(marker, dest.join(MARKER))?;
  }
  fs::remove_dir_all(staging)
}

/// Déplace de `from` vers `to` ce qui manque dans `to`. Deux dossiers de même nom sont fusionnés récursivement.
fn merge_into(from: &Path, to: &Path, top: bool) -> io::Result<()> {
  for entry in fs::read_dir(from)? {
    let entry = entry?;
    if top && entry.file_name() == MARKER {
      continue;
    }
    let target = to.join(entry.file_name());
    if !target.exists() {
      fs::rename(entry.path(), target)?;
    } else if entry.file_type()?.is_dir() && target.is_dir() {
      merge_into(&entry.path(), &target, false)?;
    }
  }
  Ok(())
}

/// `storage.json` à importer, en lecture seule.
///
/// - `RIFF_DATA_DIR` actif : `<dossier>/storage.json` seulement, comme avant.
/// - Sinon : `storage.json` du dossier d'état de Riff (natif) s'il y est, puis le plus récent (date de modification)
///   entre celui de Riff d'origine (`userData` d'Electron) et celui de Riff Tauri (ancien dossier d'état).
///   Le plus récent est celui que l'utilisateur a utilisé en dernier ; à date égale ou illisible, Riff d'origine.
#[must_use]
pub fn import_storage_path() -> PathBuf {
  if paths::data_dir_override().is_some() {
    return paths::storage_path();
  }
  let platform = Platform::current();
  let roots = Roots::from_env();
  pick_storage(
    &paths::config_dir_in(platform, &roots).join("storage.json"),
    &paths::electron_user_data_dir_in(platform, &roots).join("storage.json"),
    &paths::previous_config_dir_in(platform, &roots).join("storage.json"),
  )
}

/// Choix entre trois chemins : le propre, puis Riff d'origine, puis Riff Tauri.
#[must_use]
pub fn pick_storage(own: &Path, electron: &Path, tauri: &Path) -> PathBuf {
  if usable(own).is_some() {
    return own.to_path_buf();
  }
  match (usable(electron), usable(tauri)) {
    (Some(_), None) => electron.to_path_buf(),
    (None, Some(_)) => tauri.to_path_buf(),
    (Some(e), Some(t)) if t > e => tauri.to_path_buf(),
    (Some(_), Some(_)) => electron.to_path_buf(),
    // Aucun : le chemin propre, comme avant la reprise (stockage vide, pas d'écran d'import).
    (None, None) => own.to_path_buf(),
  }
}

/// Date de modification d'un `storage.json` qui est un objet JSON non vide. `UNIX_EPOCH` si la date est illisible.
fn usable(path: &Path) -> Option<SystemTime> {
  let raw = fs::read_to_string(path).ok()?;
  let Value::Object(fields) = serde_json::from_str(&raw).ok()? else {
    return None;
  };
  if fields.is_empty() {
    return None;
  }
  Some(
    fs::metadata(path)
      .and_then(|meta| meta.modified())
      .unwrap_or(SystemTime::UNIX_EPOCH),
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;
  use std::time::Duration;

  fn temp_dir() -> TempDir {
    TempDir::new("core-adopt")
  }

  fn layout(root: &Path) -> Layout {
    Layout {
      old_data: root.join("Riff"),
      old_config: root.join("Riff"),
      new_data: root.join("Riff (natif)"),
      new_config: root.join("Riff (natif)"),
    }
  }

  fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
  }

  /// Chemin relatif, taille et date de chaque fichier, dossiers compris.
  fn snapshot(dir: &Path) -> Vec<(String, u64, SystemTime)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, u64, SystemTime)>) {
      for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let meta = fs::metadata(&path).unwrap();
        let rel = path
          .strip_prefix(base)
          .unwrap()
          .to_string_lossy()
          .to_string();
        out.push((rel, meta.len(), meta.modified().unwrap()));
        if meta.is_dir() {
          walk(base, &path, out);
        }
      }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
  }

  fn fill_old(old: &Path) {
    write(&old.join("native-settings.json"), "{\"a\":1}");
    write(&old.join("user-data.json"), "{\"b\":2}");
    write(&old.join("window-state.json"), "{}");
    write(&old.join("config.json"), "{\"translationEnabled\":true}");
    write(&old.join("library.json"), "{}");
    write(&old.join("library/x.mp3"), "audio");
    write(&old.join("themes/t.json"), "{}");
    write(&old.join("storage.json"), "{\"riff_theme\":\"dark\"}");
    write(&old.join("cache.json"), "{}");
    write(&old.join("tracks/cached.opus"), "cache");
    write(&old.join("thumbnails/a.jpg"), "img");
    write(&old.join("metadata.json"), "{}");
    write(&old.join("logs/riff.log"), "log");
    write(&old.join("google-oauth.enc"), "enc");
    write(&old.join("spotify-auth.dpapi"), "dpapi");
  }

  #[test]
  fn adoption_copies_own_entries_once_and_leaves_the_old_folder_intact() {
    let root = temp_dir();
    let l = layout(&root);
    fill_old(&l.old_data);
    let before = snapshot(&l.old_data);

    let first = adopt_previous(&l);
    let Adoption::Done {
      entries, skipped, ..
    } = first
    else {
      panic!("reprise attendue : {first:?}");
    };
    assert_eq!(entries, 8);
    assert!(skipped.is_empty());
    for kept in [
      "native-settings.json",
      "user-data.json",
      "window-state.json",
      "config.json",
      "library.json",
      "library/x.mp3",
      "themes/t.json",
      "spotify-auth.dpapi",
      MARKER,
    ] {
      assert!(l.new_data.join(kept).is_file(), "{kept} absent");
    }
    for left in [
      "storage.json",
      "cache.json",
      "tracks",
      "thumbnails",
      "metadata.json",
      "logs",
      "google-oauth.enc",
    ] {
      assert!(
        !l.new_data.join(left).exists(),
        "{left} ne devait pas venir"
      );
    }
    assert_eq!(
      fs::read_to_string(l.new_data.join("native-settings.json")).unwrap(),
      "{\"a\":1}"
    );
    assert!(
      !l.new_data.with_file_name("Riff (natif).reprise").exists(),
      "dossier temporaire laissé"
    );
    assert_eq!(snapshot(&l.old_data), before, "l'ancien dossier a changé");

    // Une seconde reprise ne fait rien, même si l'utilisateur change un réglage entre-temps.
    write(&l.new_data.join("native-settings.json"), "{\"a\":99}");
    write(&l.old_data.join("user-data.json"), "{\"b\":3}");
    assert_eq!(adopt_previous(&l), Adoption::Skipped);
    assert_eq!(
      fs::read_to_string(l.new_data.join("native-settings.json")).unwrap(),
      "{\"a\":99}"
    );
  }

  #[test]
  fn adoption_needs_riff_native_files_in_the_old_folder() {
    let root = temp_dir();
    let l = layout(&root);
    // Dossier de Riff d'origine ou de Tauri seul : pas de reprise.
    write(&l.old_data.join("storage.json"), "{\"k\":1}");
    write(&l.old_data.join("config.json"), "{}");
    write(&l.old_data.join("library/x.mp3"), "audio");
    assert_eq!(adopt_previous(&l), Adoption::Skipped);
    assert!(!l.new_data.exists());
    // Pas d'ancien dossier du tout.
    let empty = temp_dir();
    assert_eq!(adopt_previous(&layout(&empty)), Adoption::Skipped);
    assert!(!layout(&empty).new_data.exists());
  }

  #[test]
  fn adoption_merges_into_a_new_folder_that_only_holds_a_log() {
    let root = temp_dir();
    let l = layout(&root);
    fill_old(&l.old_data);
    write(&l.new_data.join("logs/riff-app.log"), "premier lancement");
    assert!(matches!(adopt_previous(&l), Adoption::Done { .. }));
    assert!(l.new_data.join("user-data.json").is_file());
    assert!(l.new_data.join("logs/riff-app.log").is_file());
  }

  #[test]
  fn adoption_merges_into_a_new_folder_with_the_empty_dirs_made_by_core_init() {
    let root = temp_dir();
    let l = layout(&root);
    write(&l.old_data.join("native-settings.json"), "{\"a\":1}");
    write(&l.old_data.join("user-data.json"), "{\"b\":2}");
    write(&l.old_data.join("library/a.mp3"), "audio");
    write(&l.old_data.join("custom-lyrics/x.json"), "{}");
    for name in [
      "custom-lyrics",
      "i18n",
      "library",
      "lyrics",
      "thumbnails",
      "tracks",
    ] {
      fs::create_dir_all(l.new_data.join(name)).unwrap();
    }
    let before = snapshot(&l.old_data);
    assert!(matches!(adopt_previous(&l), Adoption::Done { .. }));
    for kept in [
      "native-settings.json",
      "user-data.json",
      "library/a.mp3",
      "custom-lyrics/x.json",
      MARKER,
    ] {
      assert!(l.new_data.join(kept).is_file(), "{kept} absent");
    }
    for dir in ["i18n", "lyrics", "thumbnails", "tracks"] {
      assert!(l.new_data.join(dir).is_dir(), "{dir} disparu");
    }
    assert!(!l.new_data.with_file_name("Riff (natif).reprise").exists());
    assert_eq!(snapshot(&l.old_data), before, "l'ancien dossier a changé");
  }

  #[test]
  fn adoption_keeps_existing_files_of_a_new_folder_and_adds_the_missing_ones() {
    let root = temp_dir();
    let l = layout(&root);
    write(&l.old_data.join("user-data.json"), "{}");
    write(&l.old_data.join("library/a.mp3"), "ancien-a");
    write(&l.old_data.join("library/b.mp3"), "ancien-b");
    write(&l.new_data.join("library/b.mp3"), "nouveau-b");
    assert!(matches!(adopt_previous(&l), Adoption::Done { .. }));
    assert_eq!(
      fs::read_to_string(l.new_data.join("library/b.mp3")).unwrap(),
      "nouveau-b"
    );
    assert_eq!(
      fs::read_to_string(l.new_data.join("library/a.mp3")).unwrap(),
      "ancien-a"
    );
    assert!(l.new_data.join(MARKER).is_file());
    assert_eq!(
      fs::read_to_string(l.old_data.join("library/b.mp3")).unwrap(),
      "ancien-b"
    );
  }

  #[test]
  fn a_new_folder_with_riff_native_files_blocks_the_adoption() {
    let root = temp_dir();
    let l = layout(&root);
    fill_old(&l.old_data);
    write(&l.new_data.join("user-data.json"), "{}");
    assert_eq!(adopt_previous(&l), Adoption::Skipped);
    assert!(!l.new_data.join("native-settings.json").exists());
  }

  #[test]
  fn a_failed_required_copy_leaves_no_partial_folder_and_does_not_panic() {
    let root = temp_dir();
    let l = layout(&root);
    fill_old(&l.old_data);
    // Un fichier à la place d'un dossier voisin : le dossier temporaire ne peut pas se créer.
    write(&root.join("Riff (natif).reprise"), "obstacle");
    let outcome = adopt_previous(&l);
    assert!(matches!(outcome, Adoption::Failed { .. }), "{outcome:?}");
    assert!(!l.new_data.exists());
  }

  #[test]
  fn a_custom_data_dir_is_carried_over_without_copying_data() {
    let root = temp_dir();
    let l = layout(&root);
    let custom = root.join("ailleurs");
    write(&custom.join("native-settings.json"), "{}");
    write(
      &l.old_config.join("config.json"),
      &format!(
        "{{\"dataDir\":{}}}",
        Value::String(custom.display().to_string())
      ),
    );
    write(&l.old_data.join("user-data.json"), "{\"stale\":1}");
    assert!(matches!(adopt_previous(&l), Adoption::Done { .. }));
    assert!(l.new_data.join("config.json").is_file());
    assert!(!l.new_data.join("user-data.json").exists());
    assert!(l.new_data.join(MARKER).is_file());
  }

  #[test]
  fn linux_layout_copies_config_and_data_sides_separately() {
    let root = temp_dir();
    let l = Layout {
      old_data: root.join("cache/devsize"),
      old_config: root.join("config/devsize"),
      new_data: root.join("share/riff-native"),
      new_config: root.join("config/riff-native"),
    };
    write(&l.old_data.join("native-settings.json"), "{}");
    write(&l.old_data.join("user-data.json"), "{}");
    write(&l.old_config.join("config.json"), "{}");
    write(&l.old_config.join("window-state.json"), "{}");
    write(&l.old_config.join("storage.json"), "{\"k\":1}");
    assert!(matches!(adopt_previous(&l), Adoption::Done { .. }));
    assert!(l.new_config.join("config.json").is_file());
    assert!(l.new_config.join("window-state.json").is_file());
    assert!(!l.new_config.join("storage.json").exists());
    assert!(l.new_data.join("user-data.json").is_file());
    assert!(l.new_data.join(MARKER).is_file());
    assert!(!l.new_data.join("config.json").exists());
  }

  fn set_age(path: &Path, age_secs: u64) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file
      .set_modified(SystemTime::now() - Duration::from_secs(age_secs))
      .unwrap();
  }

  #[test]
  fn storage_source_priority() {
    let root = temp_dir();
    let own = root.join("own/storage.json");
    let electron = root.join("electron/storage.json");
    let tauri = root.join("tauri/storage.json");
    // Aucune source : le chemin propre.
    assert_eq!(pick_storage(&own, &electron, &tauri), own);
    // Tauri seul.
    write(&tauri, "{\"k\":1}");
    assert_eq!(pick_storage(&own, &electron, &tauri), tauri);
    // Les deux : le plus récent.
    write(&electron, "{\"k\":2}");
    set_age(&tauri, 1000);
    set_age(&electron, 10);
    assert_eq!(pick_storage(&own, &electron, &tauri), electron);
    set_age(&electron, 5000);
    assert_eq!(pick_storage(&own, &electron, &tauri), tauri);
    // Égalité de dates : Riff d'origine.
    let stamp = SystemTime::now() - Duration::from_secs(60);
    for file in [&electron, &tauri] {
      fs::OpenOptions::new()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    }
    assert_eq!(pick_storage(&own, &electron, &tauri), electron);
    // Un fichier vide ou corrompu ne compte pas.
    write(&electron, "");
    assert_eq!(pick_storage(&own, &electron, &tauri), tauri);
    write(&tauri, "{}");
    assert_eq!(pick_storage(&own, &electron, &tauri), own);
    // Le fichier propre prime.
    write(&own, "{\"k\":3}");
    write(&electron, "{\"k\":4}");
    assert_eq!(pick_storage(&own, &electron, &tauri), own);
  }
}
