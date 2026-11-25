//! Consentement : plugins activés, liés au SHA-256 de leur source. Une source modifiée n'est plus active.
//!
//! Fichier propre à l'interface native (`riff-plugins.json`), distinct du `plugins.json` de l'ancienne interface.

use crate::manifest::PluginSource;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;

/// Nom du fichier de consentement dans le dossier des plugins.
pub const CONSENT_FILE: &str = "riff-plugins.json";
const MAX_CONSENT_BYTES: u64 = 256 * 1024;

/// Plugins activés par l'utilisateur, et plugins désactivés automatiquement.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Consent {
  /// Fichier vers le hachage de la source acceptée.
  pub enabled: BTreeMap<String, String>,
  /// Fichier vers la raison de la dernière désactivation automatique.
  pub disabled: BTreeMap<String, String>,
}

/// État d'un plugin vis-à-vis du consentement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentState {
  /// Jamais activé, ou désactivé.
  Off,
  /// Activé avec cette source exacte.
  Granted,
  /// Activé, mais la source a changé depuis : à réactiver après lecture des permissions.
  Changed,
}

impl Consent {
  /// Lit le fichier ; absent ou illisible : rien d'activé.
  #[must_use]
  pub fn load(path: &Path) -> Self {
    fs::metadata(path)
      .ok()
      .filter(|meta| meta.len() <= MAX_CONSENT_BYTES)
      .and_then(|_| fs::read_to_string(path).ok())
      .and_then(|text| serde_json::from_str(&text).ok())
      .unwrap_or_default()
  }

  /// Écriture atomique.
  ///
  /// # Errors
  /// Écriture impossible.
  pub fn save(&self, path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
      fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)
  }

  #[must_use]
  pub fn state(&self, source: &PluginSource) -> ConsentState {
    match self.enabled.get(&source.file) {
      None => ConsentState::Off,
      Some(hash) if *hash == source.hash => ConsentState::Granted,
      Some(_) => ConsentState::Changed,
    }
  }

  /// L'utilisateur active ce plugin après avoir vu ses permissions.
  pub fn grant(&mut self, source: &PluginSource) {
    self
      .enabled
      .insert(source.file.clone(), source.hash.clone());
    self.disabled.remove(&source.file);
  }

  pub fn revoke(&mut self, file: &str) {
    self.enabled.remove(file);
  }

  /// Désactivation automatique (budget, échecs), conservée au redémarrage.
  pub fn disable(&mut self, file: &str, reason: &str) {
    self.enabled.remove(file);
    self.disabled.insert(file.to_string(), reason.to_string());
  }

  /// Plugin supprimé.
  pub fn forget(&mut self, file: &str) {
    self.enabled.remove(file);
    self.disabled.remove(file);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::manifest::from_bytes;

  #[test]
  fn a_modified_file_loses_its_consent() {
    let original = from_bytes("a.rhai", b"// @name A\n// @permissions notify\n").unwrap();
    let mut consent = Consent::default();
    assert_eq!(consent.state(&original), ConsentState::Off);
    consent.grant(&original);
    assert_eq!(consent.state(&original), ConsentState::Granted);
    let modified = from_bytes(
      "a.rhai",
      b"// @name A\n// @permissions notify, search, queue\n",
    )
    .unwrap();
    assert_eq!(consent.state(&modified), ConsentState::Changed);
  }

  #[test]
  fn an_automatic_disable_is_kept_and_cleared_by_a_new_grant() {
    let source = from_bytes("a.rhai", b"// @name A\n").unwrap();
    let mut consent = Consent::default();
    consent.grant(&source);
    consent.disable("a.rhai", "3 échecs");
    assert_eq!(consent.state(&source), ConsentState::Off);
    assert_eq!(consent.disabled["a.rhai"], "3 échecs");
    consent.grant(&source);
    assert!(consent.disabled.is_empty());
  }

  #[test]
  fn the_file_round_trips_and_a_broken_file_enables_nothing() {
    let dir = crate::test_support::temp_dir("consent");
    let path = dir.join(CONSENT_FILE);
    let source = from_bytes("a.rhai", b"// @name A\n").unwrap();
    let mut consent = Consent::default();
    consent.grant(&source);
    consent.save(&path).unwrap();
    assert_eq!(Consent::load(&path), consent);
    fs::write(&path, "{ cassé").unwrap();
    assert_eq!(Consent::load(&path), Consent::default());
  }
}
