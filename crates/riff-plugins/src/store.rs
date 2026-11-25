//! Dossier des plugins : liste, import par copie, suppression, exemple du premier lancement.

use crate::consent::{CONSENT_FILE, Consent, ConsentState};
use crate::manifest::{self, MAX_SOURCE_BYTES, Manifest, PluginSource};
use crate::runtime::state_path;
use std::fs;
use std::path::Path;

/// Plugin d'exemple copié au premier lancement, comme `example-hello.js` dans Riff.
pub const HELLO_FILE: &str = "hello.rhai";
pub const HELLO_SOURCE: &str = include_str!("../../../docs/plugins/examples/hello.rhai");

/// Entrée de la liste des plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginKind {
  Native {
    manifest: Manifest,
    consent: ConsentState,
  },
  /// `.rhai` refusé : nom, taille, encodage ou en-tête.
  Invalid(String),
  /// Plugin JavaScript de Riff : jamais exécuté.
  Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEntry {
  pub file: String,
  pub kind: PluginKind,
}

/// Résultat d'une lecture du dossier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
  pub entries: Vec<PluginEntry>,
  pub consent: Consent,
  /// Plugins consentis avec leur source actuelle : à activer.
  pub granted: Vec<PluginSource>,
}

/// Premier lancement (pas de fichier de consentement) : écrit l'exemple `hello.rhai`, désactivé.
fn first_launch(dir: &Path) {
  let consent = dir.join(CONSENT_FILE);
  if consent.exists() {
    return;
  }
  let _ = fs::create_dir_all(dir);
  let hello = dir.join(HELLO_FILE);
  if !hello.exists() {
    let _ = fs::write(&hello, HELLO_SOURCE);
  }
  let _ = Consent::default().save(&consent);
}

/// Lit le dossier des plugins : `.rhai` validés, `.js` incompatibles, le reste ignoré.
#[must_use]
pub fn scan(dir: &Path) -> Scan {
  first_launch(dir);
  let consent = Consent::load(&dir.join(CONSENT_FILE));
  let mut names: Vec<String> = fs::read_dir(dir)
    .map(|entries| {
      entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
    })
    .unwrap_or_default();
  names.sort();
  let mut scan = Scan {
    consent,
    ..Scan::default()
  };
  for file in names {
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".js") {
      scan.entries.push(PluginEntry {
        file,
        kind: PluginKind::Incompatible,
      });
      continue;
    }
    if !lower.ends_with(".rhai") {
      continue;
    }
    let kind = match manifest::load(dir, &file) {
      Ok(source) => {
        let state = scan.consent.state(&source);
        let manifest = source.manifest.clone();
        if state == ConsentState::Granted {
          scan.granted.push(source);
        }
        PluginKind::Native {
          manifest,
          consent: state,
        }
      }
      Err(err) => PluginKind::Invalid(err.to_string()),
    };
    scan.entries.push(PluginEntry { file, kind });
  }
  scan
}

/// Copie un fichier choisi par l'utilisateur dans le dossier des plugins, après validation.
/// Un plugin importé n'est jamais activé : l'utilisateur lit ses permissions d'abord.
///
/// # Errors
/// Nom, taille, lien symbolique, encodage, en-tête ou écriture refusés, avec la raison.
pub fn import(dir: &Path, source_path: &Path) -> Result<String, String> {
  let file = source_path
    .file_name()
    .and_then(|name| name.to_str())
    .ok_or_else(|| "nom de fichier illisible".to_string())?
    .to_string();
  manifest::validate_file_name(&file).map_err(|err| err.to_string())?;
  let meta = fs::symlink_metadata(source_path).map_err(|err| err.to_string())?;
  if meta.file_type().is_symlink() {
    return Err(format!("lien symbolique refusé : {file}"));
  }
  if meta.len() > MAX_SOURCE_BYTES {
    return Err(format!(
      "fichier trop gros : {} octets (maximum {MAX_SOURCE_BYTES})",
      meta.len()
    ));
  }
  let bytes = fs::read(source_path).map_err(|err| err.to_string())?;
  manifest::from_bytes(&file, &bytes).map_err(|err| err.to_string())?;
  fs::create_dir_all(dir).map_err(|err| err.to_string())?;
  fs::write(dir.join(&file), &bytes).map_err(|err| err.to_string())?;
  Ok(file)
}

/// Supprime un plugin et son état.
///
/// # Errors
/// Nom invalide ou suppression impossible.
pub fn remove(dir: &Path, state_dir: &Path, file: &str) -> Result<(), String> {
  let lower = file.to_ascii_lowercase();
  if lower.ends_with(".js") {
    if file.contains(['/', '\\']) || file.contains("..") {
      return Err(format!("nom de fichier invalide : {file}"));
    }
  } else {
    manifest::validate_file_name(file).map_err(|err| err.to_string())?;
  }
  fs::remove_file(dir.join(file)).map_err(|err| err.to_string())?;
  let _ = fs::remove_file(state_path(state_dir, file));
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_support::temp_dir;

  #[test]
  fn the_first_launch_writes_the_disabled_hello_example_once() {
    let dir = temp_dir("first");
    let scan = scan(&dir);
    assert_eq!(scan.entries.len(), 1);
    assert_eq!(scan.entries[0].file, HELLO_FILE);
    assert!(matches!(
      &scan.entries[0].kind,
      PluginKind::Native { consent: ConsentState::Off, manifest } if manifest.name == "Hello"
    ));
    assert!(scan.granted.is_empty());
    fs::remove_file(dir.join(HELLO_FILE)).unwrap();
    assert!(super::scan(&dir).entries.is_empty(), "pas réécrit");
  }

  #[test]
  fn the_list_marks_javascript_plugins_incompatible_and_refused_files_invalid() {
    let dir = temp_dir("list");
    Consent::default().save(&dir.join(CONSENT_FILE)).unwrap();
    fs::write(dir.join("old.js"), "riff.on('x')").unwrap();
    fs::write(dir.join("bad.rhai"), "fn x() {}").unwrap();
    fs::write(dir.join("ok.rhai"), "// @name Ok\n").unwrap();
    fs::write(dir.join("notes.txt"), "rien").unwrap();
    let scan = scan(&dir);
    let kinds: Vec<(&str, &PluginKind)> = scan
      .entries
      .iter()
      .map(|entry| (entry.file.as_str(), &entry.kind))
      .collect();
    assert_eq!(kinds.len(), 3);
    assert!(
      matches!(kinds[0], ("bad.rhai", PluginKind::Invalid(reason)) if reason.contains("@name"))
    );
    assert!(matches!(kinds[1], ("ok.rhai", PluginKind::Native { .. })));
    assert!(matches!(kinds[2], ("old.js", PluginKind::Incompatible)));
  }

  #[test]
  fn only_plugins_with_a_matching_consent_are_granted() {
    let dir = temp_dir("granted");
    fs::write(dir.join("a.rhai"), "// @name A\n").unwrap();
    fs::write(dir.join("b.rhai"), "// @name B\n").unwrap();
    let mut consent = Consent::default();
    consent.grant(&manifest::load(&dir, "a.rhai").unwrap());
    consent.grant(&manifest::load(&dir, "b.rhai").unwrap());
    consent.save(&dir.join(CONSENT_FILE)).unwrap();
    fs::write(dir.join("b.rhai"), "// @name B\n// @permissions search\n").unwrap();
    let scan = scan(&dir);
    let granted: Vec<&str> = scan.granted.iter().map(|s| s.file.as_str()).collect();
    assert_eq!(granted, ["a.rhai"]);
    assert!(matches!(
      scan.entries[1].kind,
      PluginKind::Native {
        consent: ConsentState::Changed,
        ..
      }
    ));
  }

  #[test]
  fn import_copies_a_valid_file_and_refuses_bad_names_and_sizes() {
    let from = temp_dir("import-from");
    let dir = temp_dir("import-to");
    fs::write(from.join("good.rhai"), "// @name Bon\n").unwrap();
    assert_eq!(
      import(&dir, &from.join("good.rhai")),
      Ok("good.rhai".to_string())
    );
    assert!(dir.join("good.rhai").exists());

    fs::write(from.join("évil.rhai"), "// @name Mal\n").unwrap();
    assert!(
      import(&dir, &from.join("évil.rhai"))
        .unwrap_err()
        .contains("nom")
    );
    fs::write(from.join("plugin.js"), "// @name Js\n").unwrap();
    assert!(
      import(&dir, &from.join("plugin.js"))
        .unwrap_err()
        .contains("nom")
    );
    let big = fs::File::create(from.join("big.rhai")).unwrap();
    big.set_len(MAX_SOURCE_BYTES + 1).unwrap();
    assert!(
      import(&dir, &from.join("big.rhai"))
        .unwrap_err()
        .contains("trop gros")
    );
    fs::write(from.join("nohead.rhai"), "fn x() {}").unwrap();
    assert!(
      import(&dir, &from.join("nohead.rhai"))
        .unwrap_err()
        .contains("@name")
    );
    assert!(!dir.join("nohead.rhai").exists());
  }

  #[test]
  fn remove_deletes_the_plugin_and_its_state_but_never_leaves_the_folder() {
    let dir = temp_dir("remove");
    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();
    fs::write(dir.join("a.rhai"), "// @name A\n").unwrap();
    fs::write(state_path(&state, "a.rhai"), "{}").unwrap();
    remove(&dir, &state, "a.rhai").unwrap();
    assert!(!dir.join("a.rhai").exists());
    assert!(!state_path(&state, "a.rhai").exists());
    assert!(remove(&dir, &state, "../a.rhai").is_err());
    assert!(remove(&dir, &state, "..\\x.js").is_err());
  }

  #[test]
  fn the_hello_example_is_a_valid_plugin() {
    let source = manifest::from_bytes(HELLO_FILE, HELLO_SOURCE.as_bytes()).unwrap();
    assert_eq!(source.manifest.permissions, vec![crate::Permission::Notify]);
    crate::sandbox::compile(HELLO_SOURCE).unwrap();
  }
}
