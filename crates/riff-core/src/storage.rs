//! Lecture et écriture atomique de storage.json et cache.json. Les jetons de l'interface passent par le coffre.

use crate::paths;
use crate::secrets::{KeyringStore, SecretName, SecretStore};
use serde_json::Value;
use std::fs;

pub fn read_storage_disk() -> Option<Value> {
  let path = paths::storage_path();
  let raw = fs::read_to_string(path).ok()?;
  let mut parsed = serde_json::from_str::<Value>(&raw).ok()?;
  if !parsed.is_object() {
    return None;
  }
  let store = KeyringStore::default();
  // Fichier d'une version précédente : les jetons en clair quittent le disque dès la première lecture.
  if SECRET_KEYS.iter().any(|(key, _)| parsed.get(key).is_some()) {
    write_storage_disk(&parsed);
  }
  merge_secrets(&mut parsed, &store);
  Some(parsed)
}

pub fn write_storage_disk(data: &Value) -> bool {
  if !data.is_object() {
    return false;
  }
  let data = &split_secrets(data, &KeyringStore::default());
  let path = paths::storage_path();
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  let tmp = path.with_extension("json.tmp");
  let Ok(serialized) = serde_json::to_string(data) else {
    return false;
  };
  if fs::write(&tmp, serialized).is_ok() {
    if fs::rename(&tmp, &path).is_ok() {
      return true;
    }
    if fs::copy(&tmp, &path).is_ok() {
      let _ = fs::remove_file(&tmp);
      return true;
    }
    let _ = fs::remove_file(&tmp);
  }
  false
}

pub fn read_cache_disk() -> Option<Value> {
  let path = paths::cache_path();
  let raw = fs::read_to_string(path).ok()?;
  let parsed = serde_json::from_str::<Value>(&raw).ok()?;
  if parsed.is_object() {
    Some(parsed)
  } else {
    None
  }
}

pub fn write_cache_disk(data: &Value) {
  if !data.is_object() {
    return;
  }
  let path = paths::cache_path();
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  let tmp = path.with_extension("json.tmp");
  if let Ok(serialized) = serde_json::to_string(data)
    && fs::write(&tmp, serialized).is_ok()
    && fs::rename(&tmp, &path).is_err()
  {
    let _ = fs::copy(&tmp, &path);
    let _ = fs::remove_file(&tmp);
  }
}

// Jetons que l'interface garde dans son stockage : ils vivent dans le coffre, et dans le fichier seulement si le coffre les refuse.
const SECRET_KEYS: [(&str, SecretName); 3] = [
  ("riff_deepl_api_key", SecretName::DeeplKey),
  ("riff_lastfm_sk", SecretName::LastfmSession),
  ("riff_lastfm_apisecret", SecretName::LastfmApiSecret),
];

/// Copie à écrire sur le disque. Les jetons passent au coffre ; une clé absente est effacée du coffre (l'interface
/// l'a supprimée). Si le coffre refuse l'écriture, le jeton reste dans le fichier, comme dans Riff d'origine : sinon il serait perdu.
fn split_secrets(data: &Value, store: &dyn SecretStore) -> Value {
  let mut on_disk = data.clone();
  let Some(fields) = on_disk.as_object_mut() else {
    return on_disk;
  };
  for (key, name) in SECRET_KEYS {
    let current = store.get(name).ok().flatten();
    let token = fields
      .get(key)
      .and_then(Value::as_str)
      .filter(|t| !t.is_empty())
      .map(str::to_string);
    match token {
      Some(token) => {
        if current.as_deref() == Some(token.as_str()) || store.set(name, &token).is_ok() {
          fields.remove(key);
        }
      }
      None => {
        fields.remove(key);
        if current.is_some() {
          let _ = store.delete(name);
        }
      }
    }
  }
  on_disk
}

/// Remet dans le stockage lu les jetons du coffre. Un jeton déjà dans le fichier est celui que le coffre a refusé,
/// donc le plus récent : il n'est pas remplacé.
fn merge_secrets(data: &mut Value, store: &dyn SecretStore) {
  let Some(fields) = data.as_object_mut() else {
    return;
  };
  for (key, name) in SECRET_KEYS {
    if fields
      .get(key)
      .and_then(Value::as_str)
      .is_some_and(|t| !t.is_empty())
    {
      continue;
    }
    if let Ok(Some(token)) = store.get(name) {
      fields.insert(key.to_string(), Value::String(token));
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::secrets::{MemoryStore, SecretError};
  use serde_json::json;

  #[test]
  fn tokens_leave_the_file_and_go_to_the_store() {
    let store = MemoryStore::default();
    let data = json!({ "riff_theme": "dark", "riff_deepl_api_key": "D", "riff_lastfm_sk": "S", "riff_lastfm_apisecret": "A" });
    let on_disk = split_secrets(&data, &store);
    assert_eq!(on_disk, json!({ "riff_theme": "dark" }));
    assert_eq!(store.get(SecretName::DeeplKey), Ok(Some("D".to_string())));
    assert_eq!(
      store.get(SecretName::LastfmSession),
      Ok(Some("S".to_string()))
    );
    assert_eq!(
      store.get(SecretName::LastfmApiSecret),
      Ok(Some("A".to_string()))
    );
  }

  #[test]
  fn reading_gives_the_tokens_back_to_the_interface() {
    let store = MemoryStore::default();
    let data = json!({ "riff_theme": "dark", "riff_deepl_api_key": "D", "riff_lastfm_sk": "S" });
    let mut read = split_secrets(&data, &store);
    merge_secrets(&mut read, &store);
    assert_eq!(read, data);
  }

  #[test]
  fn a_token_removed_by_the_interface_leaves_the_store() {
    let store = MemoryStore::default();
    split_secrets(&json!({ "riff_deepl_api_key": "D" }), &store);
    split_secrets(&json!({ "riff_theme": "dark" }), &store);
    assert_eq!(store.get(SecretName::DeeplKey), Ok(None));
  }

  #[derive(Debug)]
  struct NoStore;

  impl SecretStore for NoStore {
    fn get(&self, _name: SecretName) -> Result<Option<String>, SecretError> {
      Err(SecretError("no store".to_string()))
    }
    fn set(&self, _name: SecretName, _value: &str) -> Result<(), SecretError> {
      Err(SecretError("no store".to_string()))
    }
    fn delete(&self, _name: SecretName) -> Result<(), SecretError> {
      Err(SecretError("no store".to_string()))
    }
  }

  #[test]
  fn without_a_store_a_token_stays_in_the_file() {
    let data = json!({ "riff_theme": "dark", "riff_deepl_api_key": "D" });
    let on_disk = split_secrets(&data, &NoStore);
    assert_eq!(on_disk, data);
    let mut read = on_disk;
    merge_secrets(&mut read, &NoStore);
    assert_eq!(read, data);
  }

  /// Coffre dont l'écriture échoue pour les noms listés, et dont la suppression échoue si `fail_delete`.
  #[derive(Debug, Default)]
  struct FlakyStore {
    inner: MemoryStore,
    fail_set: Vec<SecretName>,
    fail_delete: bool,
  }

  impl SecretStore for FlakyStore {
    fn get(&self, name: SecretName) -> Result<Option<String>, SecretError> {
      self.inner.get(name)
    }
    fn set(&self, name: SecretName, value: &str) -> Result<(), SecretError> {
      if self.fail_set.contains(&name) {
        return Err(SecretError("refused".to_string()));
      }
      self.inner.set(name, value)
    }
    fn delete(&self, name: SecretName) -> Result<(), SecretError> {
      if self.fail_delete {
        return Err(SecretError("refused".to_string()));
      }
      self.inner.delete(name)
    }
  }

  #[test]
  fn a_refused_write_keeps_the_token_in_the_file() {
    let store = FlakyStore {
      fail_set: vec![SecretName::DeeplKey],
      ..Default::default()
    };
    let data = json!({ "riff_theme": "dark", "riff_deepl_api_key": "D" });
    assert_eq!(split_secrets(&data, &store), data);
    assert_eq!(store.get(SecretName::DeeplKey), Ok(None));
  }

  #[test]
  fn only_the_refused_token_stays_in_the_file() {
    let store = FlakyStore {
      fail_set: vec![SecretName::LastfmSession],
      ..Default::default()
    };
    let data =
      json!({ "riff_deepl_api_key": "D", "riff_lastfm_sk": "S", "riff_lastfm_apisecret": "A" });
    assert_eq!(
      split_secrets(&data, &store),
      json!({ "riff_lastfm_sk": "S" })
    );
    assert_eq!(store.get(SecretName::DeeplKey), Ok(Some("D".to_string())));
    assert_eq!(
      store.get(SecretName::LastfmApiSecret),
      Ok(Some("A".to_string()))
    );
    assert_eq!(store.get(SecretName::LastfmSession), Ok(None));
  }

  #[test]
  fn the_file_token_wins_over_an_older_store_value() {
    let store = FlakyStore {
      fail_set: vec![SecretName::DeeplKey],
      ..Default::default()
    };
    store.inner.set(SecretName::DeeplKey, "old").unwrap();
    let data = json!({ "riff_deepl_api_key": "new" });
    let mut read = split_secrets(&data, &store);
    assert_eq!(read, data);
    merge_secrets(&mut read, &store);
    assert_eq!(read, data);
    assert_eq!(store.get(SecretName::DeeplKey), Ok(Some("old".to_string())));
  }

  #[test]
  fn a_token_leaves_the_file_once_the_store_accepts_it() {
    let refusing = FlakyStore {
      fail_set: vec![SecretName::DeeplKey],
      ..Default::default()
    };
    let data = json!({ "riff_theme": "dark", "riff_deepl_api_key": "D" });
    let kept = split_secrets(&data, &refusing);
    assert_eq!(kept, data);
    let accepting = MemoryStore::default();
    assert_eq!(
      split_secrets(&kept, &accepting),
      json!({ "riff_theme": "dark" })
    );
    assert_eq!(
      accepting.get(SecretName::DeeplKey),
      Ok(Some("D".to_string()))
    );
  }

  #[test]
  fn a_failed_delete_leaves_the_token_in_the_store() {
    let store = FlakyStore {
      fail_delete: true,
      ..Default::default()
    };
    store.inner.set(SecretName::DeeplKey, "D").unwrap();
    let mut on_disk = split_secrets(&json!({ "riff_theme": "dark" }), &store);
    assert_eq!(on_disk, json!({ "riff_theme": "dark" }));
    assert_eq!(store.get(SecretName::DeeplKey), Ok(Some("D".to_string())));
    merge_secrets(&mut on_disk, &store);
    assert_eq!(
      on_disk,
      json!({ "riff_theme": "dark", "riff_deepl_api_key": "D" })
    );
  }
}
