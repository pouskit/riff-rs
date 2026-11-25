//! Tests du core sur un dossier de données isolé. Binaire à part : les variables d'environnement ne concernent que lui.

use bytes::Bytes;
use riff_core::core::{Core, CoreConfig};
use riff_core::events::{self, CoreEvent};
use riff_core::reply::{Method, Params, Reply};
use riff_core::secrets::MemoryStore;
use riff_core::services::spotify;
use riff_core::{config, dispatch, library, paths, storage};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Once};

#[path = "support/temp.rs"]
mod temp;

use temp::SharedDir;

const BASE: &str = "http://127.0.0.1:1234";

/// Un seul dossier par exécution : les variables sont posées une fois, avant toute lecture par les tests.
fn root() -> PathBuf {
  static DIR: SharedDir = SharedDir::new("core-tests");
  static ENV: Once = Once::new();
  let dir = DIR.path();
  ENV.call_once(|| {
    // SAFETY : `call_once` bloque les autres tests tant que les variables ne sont pas posées, et ce binaire
    // n'a aucun autre fil d'exécution qui lit l'environnement avant le premier appel à `root`.
    unsafe {
      std::env::set_var("RIFF_DATA_DIR", dir.join("data"));
      std::env::set_var("LOCALAPPDATA", dir.join("local"));
      std::env::set_var("XDG_CONFIG_HOME", dir.join("xdg"));
      // macOS résout le dossier de config depuis le dossier personnel : jamais le vrai.
      #[cfg(unix)]
      std::env::set_var("HOME", dir.join("home"));
      // `storage` passe par le coffre du système : jamais celui des vrais jetons.
      std::env::set_var(
        "RIFF_KEYRING_SERVICE",
        format!("riff-rs-core-env-{}", std::process::id()),
      );
    }
  });
  DIR.lease(|dir| std::fs::create_dir_all(dir.join("data")).expect("dossier de test"))
}

fn new_core() -> Arc<Core> {
  root();
  Core::new(CoreConfig {
    local_base_url: BASE.to_string(),
    secret_store: Some(Arc::new(MemoryStore::default())),
  })
}

fn json_body(reply: Reply) -> Value {
  match reply {
    Reply::Json { body, .. } => body,
    other => panic!("attendu : Reply::Json, reçu {other:?}"),
  }
}

fn with_query(pairs: &[(&str, &str)]) -> Params {
  let query: HashMap<String, String> = pairs
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
  Params {
    query,
    ..Params::default()
  }
}

#[test]
fn local_url_uses_the_configured_base() {
  assert_eq!(
    new_core().local_url("/api/x?id=1"),
    format!("{BASE}/api/x?id=1")
  );
}

#[tokio::test]
async fn subscribers_receive_the_app_state() {
  let core = new_core();
  let mut events_rx = core.subscribe();
  let params = Params {
    body: Bytes::from_static(br#"{"isPlaying":true}"#),
    ..Params::default()
  };
  let reply = events::app_state(&core, params).await.unwrap();
  assert_eq!(json_body(reply), json!({ "success": true }));
  assert_eq!(
    events_rx.recv().await.unwrap(),
    CoreEvent::AppState(json!({ "isPlaying": true }))
  );
  assert_eq!(core.current_app_state(), json!({ "isPlaying": true }));
}

#[tokio::test]
async fn invalid_app_state_is_rejected_without_changing_anything() {
  let core = new_core();
  let params = Params {
    body: Bytes::from_static(b"{pas du json"),
    ..Params::default()
  };
  assert_eq!(
    events::app_state(&core, params).await.unwrap().status(),
    400
  );
  assert_eq!(core.current_app_state(), json!({}));
}

#[tokio::test]
async fn spotify_authorize_url_carries_the_oauth_state_and_the_local_redirect() {
  let core = new_core();
  let client_id = "0123456789abcdef0123456789abcdef";
  let body = json_body(
    spotify::auth_start(&core, with_query(&[("clientId", client_id)]))
      .await
      .unwrap(),
  );
  let url = body["url"].as_str().unwrap();
  assert!(url.contains("&state="), "paramètre state absent : {url}");
  assert!(!url.contains("&core="), "paramètre renommé : {url}");
  assert_eq!(
    body["redirectUri"],
    json!(format!("{BASE}/api/spotify-callback"))
  );
}

#[tokio::test]
async fn cached_track_url_uses_the_local_base() {
  let core = new_core();
  std::fs::create_dir_all(paths::tracks_dir()).unwrap();
  std::fs::write(
    paths::tracks_dir().join("cachedtrack1.mp3"),
    vec![0u8; 70_000],
  )
  .unwrap();
  let body = json_body(
    library::check_cache(&core, with_query(&[("id", "cachedtrack1")]))
      .await
      .unwrap(),
  );
  assert_eq!(body["cached"], json!(true));
  assert_eq!(
    body["streamUrl"],
    json!(format!("{BASE}/api/local-track?id=cachedtrack1"))
  );
}

#[test]
fn data_dir_follows_the_environment_variable() {
  let expected = std::fs::canonicalize(root().join("data")).unwrap();
  assert_eq!(paths::resolve_data_dir(), expected);
  assert_eq!(paths::tracks_dir(), expected.join("tracks"));
  assert_eq!(paths::library_path(), expected.join("library.json"));
  assert_eq!(paths::log_path(), expected.join("logs").join("riff.log"));
}

#[test]
fn download_dir_config_round_trips() {
  root();
  assert_eq!(config::resolve_download_dir(), None);
  let chosen = root().join("musique").to_string_lossy().into_owned();
  assert_eq!(
    config::set_download_dir_config(Some(&chosen)),
    Some(PathBuf::from(&chosen))
  );
  assert_eq!(config::read_config()["downloadDir"], json!(chosen));
  assert_eq!(config::set_download_dir_config(None), None);
  assert_eq!(config::resolve_download_dir(), None);
}

#[test]
fn startup_config_merges_patches() {
  root();
  let mut first = Map::new();
  first.insert("minimized".to_string(), json!(true));
  assert!(config::write_startup_config(&first));
  let mut second = Map::new();
  second.insert("autostart".to_string(), json!(false));
  assert!(config::write_startup_config(&second));
  let merged = config::read_startup_config();
  assert_eq!(merged["minimized"], json!(true));
  assert_eq!(merged["autostart"], json!(false));
}

#[test]
fn storage_round_trips_and_refuses_what_is_not_an_object() {
  root();
  let data = json!({ "riff_theme": "dark" });
  assert!(storage::write_storage_disk(&data));
  assert_eq!(storage::read_storage_disk(), Some(data));
  assert!(!storage::write_storage_disk(&json!([1, 2])));
  storage::write_cache_disk(&json!({ "k": 1 }));
  assert_eq!(storage::read_cache_disk(), Some(json!({ "k": 1 })));
}

#[test]
fn saved_track_metadata_has_a_clean_url() {
  let core = new_core();
  let mut extra = Map::new();
  extra.insert(
    "url".to_string(),
    json!("https://www.youtube.com/watch?v=abc&si=x"),
  );
  core.save_track_metadata_entry("cleanurltrack", extra);
  let saved = core.sync_metadata_from_disk();
  assert_eq!(
    saved["cleanurltrack"]["url"],
    json!("https://www.youtube.com/watch?v=abc")
  );
}

#[tokio::test]
async fn translation_sends_nothing_while_disabled() {
  let core = new_core();
  let params = Params {
    body: Bytes::from_static(br#"{"texts":["bonjour"],"targetLang":"en"}"#),
    ..Params::default()
  };
  let body = json_body(
    riff_core::services::translate::translate(&core, params)
      .await
      .unwrap(),
  );
  assert_eq!(body["success"], json!(false));
  assert_eq!(body["translations"], json!([]));
}

#[tokio::test]
async fn i18n_keeps_source_texts_while_translation_is_disabled() {
  let core = new_core();
  let params = Params {
    body: Bytes::from_static(br#"{"texts":["Bonjour"],"target":"de"}"#),
    ..Params::default()
  };
  let body = json_body(
    riff_core::services::translate::i18n_batch(&core, params)
      .await
      .unwrap(),
  );
  assert_eq!(body["translations"], json!(["Bonjour"]));
}

#[tokio::test]
async fn start_login_opens_an_ephemeral_loopback_listener_with_pkce_and_state() {
  let core = new_core();
  let client_id = "0123456789abcdef0123456789abcdef";
  let start = spotify::start_login(&core, client_id).await.unwrap();
  let port = start
    .redirect_uri
    .strip_prefix("http://127.0.0.1:")
    .and_then(|r| r.strip_suffix("/api/spotify-callback"))
    .unwrap();
  let port: u16 = port.parse().unwrap();
  assert_ne!(port, 0);
  let parsed = url::Url::parse(&start.url).unwrap();
  assert_eq!(parsed.host_str(), Some("accounts.spotify.com"));
  let pairs: HashMap<String, String> = parsed
    .query_pairs()
    .map(|(k, v)| (k.into_owned(), v.into_owned()))
    .collect();
  assert_eq!(pairs["redirect_uri"], start.redirect_uri);
  assert_eq!(pairs["code_challenge_method"], "S256");
  assert_eq!(pairs["state"].len(), 32, "128 bits en hexadécimal");
  assert!(!pairs["code_challenge"].is_empty());

  let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
    .await
    .unwrap();
  let raw = format!(
    "GET /api/spotify-callback?code=x&state=mauvais HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
  );
  tokio::io::AsyncWriteExt::write_all(&mut stream, raw.as_bytes())
    .await
    .unwrap();
  let mut reply = String::new();
  tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut reply)
    .await
    .unwrap();
  assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
}

#[tokio::test]
async fn start_login_refuses_an_invalid_client_id() {
  let core = new_core();
  assert_eq!(
    spotify::start_login(&core, "court").await,
    Err(spotify::LoginError::InvalidClientId)
  );
}

#[tokio::test]
async fn dispatch_returns_none_for_an_unknown_path() {
  let core = new_core();
  assert!(
    dispatch::call(&core, Method::Get, "/api/inconnu", Params::default())
      .await
      .is_none()
  );
  assert!(
    dispatch::call(&core, Method::Get, "/autre", Params::default())
      .await
      .is_none()
  );
}

#[tokio::test]
async fn dispatch_replies_404_to_a_forbidden_method() {
  let core = new_core();
  for (method, path) in [
    (Method::Get, "/api/state"),
    (Method::Post, "/api/audio-output-info"),
    (Method::Other, "/api/i18n-batch"),
  ] {
    let reply = dispatch::call(&core, method, path, Params::default())
      .await
      .expect("route connue")
      .expect("réponse");
    assert!(
      matches!(reply, Reply::Text { status: 404, ref text } if text == "Not found"),
      "{path} : {reply:?}"
    );
  }
}

#[tokio::test]
async fn dispatch_calls_the_operation_of_a_known_path() {
  let core = new_core();
  let params = Params {
    body: Bytes::from_static(b"{\"a\":1}"),
    ..Params::default()
  };
  let reply = dispatch::call(&core, Method::Post, "/api/state", params)
    .await
    .expect("route connue")
    .expect("réponse");
  assert!(matches!(reply, Reply::Json { status: 200, .. }));
  assert_eq!(core.current_app_state(), json!({ "a": 1 }));
}
