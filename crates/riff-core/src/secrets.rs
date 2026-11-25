//! Secrets : clé publique Last.fm et coffre du système pour les jetons utilisateur.
//!
//! Repli si le coffre est indisponible (Linux sans Secret Service) : refus de stocker pour les connexions Spotify et
//! Google. L'utilisateur reste connecté jusqu'à la fermeture et un message le prévient. Les trois jetons de
//! `storage.json` (DeepL, Last.fm) restent dans le fichier, comme dans Riff d'origine. Un fichier chiffré avec une clé locale posée à côté ne protégerait
//! rien de plus qu'un fichier en clair.

use crate::config;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Mutex;

/// Clé d'API Last.fm de l'application, en lecture seule et sans `api_secret` : elle identifie le client mais n'ouvre
/// aucun compte. Tout client distribué la contient forcément : elle reste donc dans le binaire, et les données restent
/// dans un fichier à part pour qu'elle n'apparaisse qu'à un seul endroit. Surchargeable par `lastfmApiKey` dans `config.json`.
pub const LASTFM_PUBLIC_API_KEY: &str =
  include_str!("../assets/lastfm-public-api-key.txt").trim_ascii();

/// Clé Last.fm à utiliser : celle de l'utilisateur si elle est réglée, sinon celle de l'application.
#[must_use]
pub fn lastfm_api_key() -> String {
  let config = config::read_config();
  let custom = config
    .get("lastfmApiKey")
    .and_then(Value::as_str)
    .unwrap_or("")
    .trim();
  if custom.is_empty() {
    LASTFM_PUBLIC_API_KEY.to_string()
  } else {
    custom.to_string()
  }
}

/// Secrets gardés dans le coffre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretName {
  SpotifyAuth,
  LastfmSession,
  DeeplKey,
  GoogleAuth,
  LastfmApiSecret,
}

impl SecretName {
  const fn account(self) -> &'static str {
    match self {
      Self::SpotifyAuth => "spotify-auth",
      Self::LastfmSession => "lastfm-session",
      Self::DeeplKey => "deepl-key",
      Self::GoogleAuth => "google-auth",
      Self::LastfmApiSecret => "lastfm-api-secret",
    }
  }
}

/// Échec du coffre. Ne porte jamais le contenu du secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("secret store unavailable: {0}")]
pub struct SecretError(pub String);

/// Coffre de secrets.
pub trait SecretStore: Send + Sync + Debug {
  /// Lit un secret. `Ok(None)` s'il n'existe pas.
  ///
  /// # Errors
  /// [`SecretError`] si le coffre est inaccessible.
  fn get(&self, name: SecretName) -> Result<Option<String>, SecretError>;

  /// Écrit un secret.
  ///
  /// # Errors
  /// [`SecretError`] si le coffre est inaccessible ou refuse la valeur.
  fn set(&self, name: SecretName, value: &str) -> Result<(), SecretError>;

  /// Supprime un secret. Supprimer un secret absent n'est pas une erreur.
  ///
  /// # Errors
  /// [`SecretError`] si le coffre est inaccessible.
  fn delete(&self, name: SecretName) -> Result<(), SecretError>;
}

/// Coffre du système : Gestionnaire d'identifiants sous Windows, Trousseau sous macOS, Secret Service sous Linux.
#[derive(Debug, Clone)]
pub struct KeyringStore {
  service: String,
}

impl Default for KeyringStore {
  /// Service `riff-rs`. `RIFF_KEYRING_SERVICE` le remplace, pour que les tests et les scripts n'approchent jamais
  /// les vrais jetons de l'utilisateur.
  fn default() -> Self {
    let service = std::env::var("RIFF_KEYRING_SERVICE")
      .ok()
      .filter(|s| !s.is_empty());
    Self {
      service: service.unwrap_or_else(|| "riff-rs".to_string()),
    }
  }
}

// Le détail de `keyring::Error` peut embarquer les octets du secret (`BadEncoding`) : seule sa nature est gardée.
fn store_error(err: &keyring::Error) -> SecretError {
  let kind = match err {
    keyring::Error::NoStorageAccess(_) => "no access",
    keyring::Error::NoDefaultStore => "no store",
    keyring::Error::TooLong(..) => "value too long",
    keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => "unreadable entry",
    _ => "platform failure",
  };
  SecretError(kind.to_string())
}

impl KeyringStore {
  fn entry(&self, name: SecretName) -> Result<keyring::Entry, SecretError> {
    keyring::Entry::new(&self.service, name.account()).map_err(|e| store_error(&e))
  }
}

impl SecretStore for KeyringStore {
  fn get(&self, name: SecretName) -> Result<Option<String>, SecretError> {
    match self.entry(name)?.get_password() {
      Ok(value) => Ok(Some(value)),
      Err(keyring::Error::NoEntry) => Ok(None),
      Err(e) => Err(store_error(&e)),
    }
  }

  fn set(&self, name: SecretName, value: &str) -> Result<(), SecretError> {
    self
      .entry(name)?
      .set_password(value)
      .map_err(|e| store_error(&e))
  }

  fn delete(&self, name: SecretName) -> Result<(), SecretError> {
    match self.entry(name)?.delete_credential() {
      Ok(()) | Err(keyring::Error::NoEntry | keyring::Error::NoDefaultStore) => Ok(()),
      Err(e) => Err(store_error(&e)),
    }
  }
}

/// Coffre en mémoire, perdu à la fermeture : tests, et tout usage où rien ne doit toucher le coffre du système.
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<HashMap<SecretName, String>>);

impl SecretStore for MemoryStore {
  fn get(&self, name: SecretName) -> Result<Option<String>, SecretError> {
    Ok(
      self
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&name)
        .cloned(),
    )
  }

  fn set(&self, name: SecretName, value: &str) -> Result<(), SecretError> {
    self
      .0
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .insert(name, value.to_string());
    Ok(())
  }

  fn delete(&self, name: SecretName) -> Result<(), SecretError> {
    self
      .0
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .remove(&name);
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_public_lastfm_key_is_a_clean_token() {
    assert_eq!(LASTFM_PUBLIC_API_KEY.len(), 32);
    assert!(LASTFM_PUBLIC_API_KEY.chars().all(|c| c.is_ascii_hexdigit()));
  }

  #[test]
  fn memory_store_round_trips() {
    let store = MemoryStore::default();
    assert_eq!(store.get(SecretName::DeeplKey), Ok(None));
    store.set(SecretName::DeeplKey, "k").unwrap();
    assert_eq!(store.get(SecretName::DeeplKey), Ok(Some("k".to_string())));
    assert_eq!(store.get(SecretName::LastfmSession), Ok(None));
    store.delete(SecretName::DeeplKey).unwrap();
    store.delete(SecretName::DeeplKey).unwrap();
    assert_eq!(store.get(SecretName::DeeplKey), Ok(None));
  }

  /// Écrit puis efface une entrée dédiée dans le vrai coffre du système.
  #[cfg(windows)]
  #[test]
  fn keyring_store_round_trips_in_the_system_store() {
    let store = KeyringStore {
      service: format!("riff-rs-test-{}", std::process::id()),
    };
    let name = SecretName::GoogleAuth;
    assert_eq!(store.get(name), Ok(None));
    store.delete(name).unwrap();
    store.set(name, "valeur de test é").unwrap();
    let read_back = store.get(name);
    store.delete(name).unwrap();
    assert_eq!(read_back, Ok(Some("valeur de test é".to_string())));
    assert_eq!(store.get(name), Ok(None));
  }
}
