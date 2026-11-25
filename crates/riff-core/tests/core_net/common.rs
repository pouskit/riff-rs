//! Environnement et transports de rejeu partagés par les tests de ce binaire.

use bytes::Bytes;
use riff_core::core::{Core, CoreConfig};
use riff_core::net::replay::{self, Fixture, Replay};
use riff_core::reply::{CoreResult, Params, Reply};
use riff_core::secrets::{MemoryStore, SecretStore};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Once};
use url::Url;

use crate::temp::SharedDir;

/// Dossier racine du binaire : données, config, fixtures vides. Les variables sont posées une seule fois.
pub fn root() -> PathBuf {
  static DIR: SharedDir = SharedDir::new("core-net");
  static ENV: Once = Once::new();
  let dir = DIR.path();
  ENV.call_once(|| {
    // SAFETY : `call_once` bloque les autres tests tant que les variables ne sont pas posées ; ce binaire n'a pas
    // d'autre fil qui lit l'environnement avant le premier appel à `root`.
    unsafe {
      std::env::set_var("RIFF_DATA_DIR", dir.join("data"));
      std::env::set_var("LOCALAPPDATA", dir.join("local"));
      std::env::set_var("XDG_CONFIG_HOME", dir.join("xdg"));
      // macOS résout le dossier de config depuis le dossier personnel : jamais le vrai.
      #[cfg(unix)]
      std::env::set_var("HOME", dir.join("home"));
      std::env::set_var(
        "RIFF_KEYRING_SERVICE",
        format!("riff-rs-core-net-{}", std::process::id()),
      );
      std::env::set_var("RIFF_YTDLP", dir.join("tools/yt-dlp"));
      std::env::set_var("RIFF_FFMPEG", dir.join("tools/ffmpeg"));
      // Un client construit sans rejeu explicite échoue (dossier vide) au lieu d'aller sur le réseau.
      std::env::set_var("RIFF_HTTP_FIXTURES", dir.join("no-fixtures"));
      std::env::remove_var("RIFF_HTTP_RECORD");
      std::env::remove_var("RIFF_LASTFM_KEY");
      std::env::remove_var("RIFF_LASTFM_SECRET");
    }
  });
  DIR.lease(|dir| {
    std::fs::create_dir_all(dir.join("no-fixtures")).expect("dossier");
    std::fs::create_dir_all(dir.join("tools")).expect("dossier");
    for tool in ["yt-dlp", "ffmpeg"] {
      std::fs::write(dir.join("tools").join(tool), [0u8; 4]).expect("faux outil");
    }
    let config = riff_core::paths::config_path();
    std::fs::create_dir_all(config.parent().expect("parent")).expect("dossier de config");
    std::fs::write(
      config,
      serde_json::json!({ "downloadDir": dir.join("downloads") }).to_string(),
    )
    .expect("config");
  })
}

/// Fixtures enregistrées du dépôt.
pub fn recorded_dir() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http")
}

fn temp_dir() -> PathBuf {
  static N: AtomicU32 = AtomicU32::new(0);
  let dir = root().join(format!("replay-{}", N.fetch_add(1, Ordering::Relaxed)));
  std::fs::create_dir_all(&dir).expect("dossier");
  dir
}

/// `Core` dont les trois clients rejouent `dir`, sans enregistrement possible.
pub fn core_with(dir: &Path) -> Arc<Core> {
  core_with_store(dir, Arc::new(MemoryStore::default()))
}

/// Comme [`core_with`], avec le coffre de secrets donné.
pub fn core_with_store(dir: &Path, store: Arc<dyn SecretStore>) -> Arc<Core> {
  root();
  let mut core = Core::new(CoreConfig {
    local_base_url: "http://127.0.0.1:1234".to_string(),
    secret_store: Some(store),
  });
  let replay = || Replay::new(dir).recording(false);
  let state = &mut Arc::get_mut(&mut core).expect("Core non partagé").net;
  state.http_client = state.http_client.with_replay(replay());
  state.yt_client = state.yt_client.with_replay(replay());
  state.image_client = state.image_client.with_replay(replay());
  let _ = replay::take_misses();
  core
}

/// Coffre qui refuse toute opération, pour les chemins d'erreur de lecture, d'écriture et de suppression.
#[derive(Debug)]
pub struct BrokenStore;

impl SecretStore for BrokenStore {
  fn get(
    &self,
    _: riff_core::secrets::SecretName,
  ) -> Result<Option<String>, riff_core::secrets::SecretError> {
    Err(riff_core::secrets::SecretError(
      "coffre de test cassé".into(),
    ))
  }
  fn set(
    &self,
    _: riff_core::secrets::SecretName,
    _: &str,
  ) -> Result<(), riff_core::secrets::SecretError> {
    Err(riff_core::secrets::SecretError(
      "coffre de test cassé".into(),
    ))
  }
  fn delete(
    &self,
    _: riff_core::secrets::SecretName,
  ) -> Result<(), riff_core::secrets::SecretError> {
    Err(riff_core::secrets::SecretError(
      "coffre de test cassé".into(),
    ))
  }
}

/// `Core` hors ligne dont le coffre échoue toujours.
pub fn core_broken_vault() -> Arc<Core> {
  core_with_store(&temp_dir(), Arc::new(BrokenStore))
}

/// Réponses enregistrées.
pub fn core_recorded() -> Arc<Core> {
  core_with(&recorded_dir())
}

/// Réseau indisponible : aucune fixture, toute requête échoue avec « no fixture ».
pub fn core_offline() -> Arc<Core> {
  core_with(&temp_dir())
}

/// Écrit une réponse 500 écrite à la main pour chaque `(méthode, URL)` et renvoie le `Core` qui la rejoue.
pub fn core_http_500(requests: &[(&str, &str)]) -> Arc<Core> {
  let dir = temp_dir();
  for (method, url) in requests {
    write_fixture(
      &dir,
      method,
      url,
      500,
      "Internal Server Error",
      "text/plain",
    );
  }
  core_with(&dir)
}

/// Fixture écrite à la main (format de `Fixture`, `recorded_at` le dit). Le corps de requête est vide.
pub fn write_fixture(
  dir: &Path,
  method: &str,
  url: &str,
  status: u16,
  body: &str,
  content_type: &str,
) {
  let parsed = Url::parse(url).expect("URL");
  let path = replay::fixture_path(dir, method, &parsed, b"");
  std::fs::create_dir_all(path.parent().expect("parent")).expect("dossier");
  let fixture = Fixture {
    recorded_at: "écrit à la main".to_string(),
    method: method.to_string(),
    url: url.to_string(),
    body_sha256: String::new(),
    status,
    headers: BTreeMap::from([("content-type".to_string(), content_type.to_string())]),
    final_url: None,
    body: Some(body.to_string()),
    body_base64: None,
  };
  std::fs::write(&path, serde_json::to_vec_pretty(&fixture).expect("JSON")).expect("fixture");
}

/// `Core` qui rejoue un dossier de fixtures écrites à la main par `build`.
pub fn core_hand_written(build: impl FnOnce(&Path)) -> Arc<Core> {
  let dir = temp_dir();
  build(&dir);
  core_with(&dir)
}

/// Fixtures manquantes vues par ce fil depuis la dernière lecture.
pub fn misses() -> Vec<String> {
  replay::take_misses()
}

pub fn query(pairs: &[(&str, &str)]) -> Params {
  Params {
    query: pairs
      .iter()
      .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
      .collect::<HashMap<_, _>>(),
    ..Params::default()
  }
}

pub fn body(json: &str) -> Params {
  Params {
    body: Bytes::copy_from_slice(json.as_bytes()),
    ..Params::default()
  }
}

/// Statut et corps JSON d'une réponse du core. Une erreur du core devient `(status, {"error": ...})`.
pub fn json_of(result: CoreResult) -> (u16, Value) {
  match result {
    Ok(Reply::Json { status, body }) => (status, body),
    Ok(other) => panic!("attendu : Reply::Json, reçu {other:?}"),
    Err(err) => (
      err.status(),
      serde_json::json!({ "error": err.to_string() }),
    ),
  }
}

/// Comme [`write_fixture`], pour une requête à corps (POST) : `request_body` est l'octet exact envoyé par le code.
pub fn write_fixture_with_body(
  dir: &Path,
  method: &str,
  url: &str,
  request_body: &[u8],
  status: u16,
  body: &str,
  content_type: &str,
) {
  let parsed = Url::parse(url).expect("URL");
  let path = replay::fixture_path(dir, method, &parsed, request_body);
  std::fs::create_dir_all(path.parent().expect("parent")).expect("dossier");
  let fixture = Fixture {
    recorded_at: "écrit à la main".to_string(),
    method: method.to_string(),
    url: url.to_string(),
    body_sha256: String::new(),
    status,
    headers: BTreeMap::from([("content-type".to_string(), content_type.to_string())]),
    final_url: None,
    body: Some(body.to_string()),
    body_base64: None,
  };
  std::fs::write(&path, serde_json::to_vec_pretty(&fixture).expect("JSON")).expect("fixture");
}

/// Statut et texte d'une réponse `Reply::Text`.
pub fn text_of(result: CoreResult) -> (u16, String) {
  match result {
    Ok(Reply::Text { status, text }) => (status, text),
    other => panic!("attendu : Reply::Text, reçu {other:?}"),
  }
}

/// Exclusion mutuelle des tests qui changent le réglage global de traduction (fichier de config partagé).
pub async fn translation_setting(enabled: bool) -> tokio::sync::MutexGuard<'static, ()> {
  static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
  let guard = LOCK.lock().await;
  root();
  riff_core::config::set_translation_enabled(enabled);
  guard
}

/// Statut, en-têtes et corps complet d'une réponse `Reply::Stream`.
pub async fn stream_of(result: CoreResult) -> (u16, Vec<(String, String)>, Vec<u8>) {
  use futures_util::StreamExt;
  match result {
    Ok(Reply::Stream {
      status,
      headers,
      mut body,
    }) => {
      let mut bytes = Vec::new();
      while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk.expect("morceau"));
      }
      (status, headers, bytes)
    }
    other => panic!("attendu : Reply::Stream, reçu {other:?}"),
  }
}

/// `Core` hors ligne dont le coffre est fourni (par exemple pré-rempli).
pub fn core_offline_with_store(store: Arc<dyn SecretStore>) -> Arc<Core> {
  core_with_store(&temp_dir(), store)
}
