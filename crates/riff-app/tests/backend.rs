//! Tuyau vers le core sur un dossier de données isolé.

use bytes::Bytes;
use iced::futures::StreamExt;
use riff_app::app::Message;
use riff_app::backend::{Backend, BackendError};
use riff_app::core_sub;
use riff_core::core::CoreConfig;
use riff_core::events::CoreEvent;
use riff_core::reply::Reply;
use riff_core::secrets::MemoryStore;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Once};
use std::time::Duration;

#[path = "support/temp.rs"]
mod temp;

use temp::SharedDir;

fn backend() -> Backend {
  static DIR: SharedDir = SharedDir::new("app-tests");
  static ENV: Once = Once::new();
  let dir = DIR.path();
  ENV.call_once(|| {
    // SAFETY : `call_once` bloque les autres tests tant que les variables ne sont pas posées, et ce binaire
    // n'a aucun autre fil d'exécution qui lit l'environnement avant le premier appel.
    unsafe {
      std::env::set_var("RIFF_DATA_DIR", dir.join("data"));
      std::env::set_var("LOCALAPPDATA", dir.join("local"));
      std::env::set_var("XDG_CONFIG_HOME", dir.join("xdg"));
      // macOS résout le dossier de config depuis le dossier personnel : jamais le vrai.
      #[cfg(unix)]
      std::env::set_var("HOME", dir.join("home"));
      std::env::set_var(
        "RIFF_KEYRING_SERVICE",
        format!("riff-rs-app-{}", std::process::id()),
      );
    }
  });
  DIR.lease(|dir| std::fs::create_dir_all(dir.join("data")).expect("dossier de test"));
  let config = CoreConfig {
    secret_store: Some(Arc::new(MemoryStore::default())),
    ..CoreConfig::default()
  };
  Backend::with_config(config).expect("moteur").0
}

#[tokio::test]
async fn local_urls_of_the_core_use_the_riff_scheme() {
  assert_eq!(backend().core.local_url("/api/x"), "riff://local/api/x");
}

#[tokio::test]
async fn resolve_url_replies_with_the_operation() {
  let reply = backend()
    .resolve_url("riff://local/api/library")
    .await
    .expect("réponse");
  assert!(
    matches!(reply, Reply::Json { status: 200, .. }),
    "{reply:?}"
  );
}

#[tokio::test]
async fn resolve_url_decodes_the_query() {
  let reply = backend()
    .resolve_url("riff://local/api/search?q=&platform=a%20b")
    .await
    .expect("réponse");
  let Reply::Json { status: 200, body } = reply else {
    panic!("attendu : JSON 200")
  };
  assert_eq!(body["tracks"], json!([]));
}

#[tokio::test]
async fn resolve_url_refuses_another_scheme_or_host() {
  let backend = backend();
  for url in [
    "http://127.0.0.1:38472/api/library",
    "https://local/api/library",
    "riff://autre/api/library",
    "pas une url",
  ] {
    assert!(
      matches!(backend.resolve_url(url).await, Err(BackendError::BadUrl(_))),
      "{url}"
    );
  }
}

#[tokio::test]
async fn resolve_url_replies_404_to_an_unknown_path() {
  let reply = backend()
    .resolve_url("riff://local/api/inconnu")
    .await
    .expect("réponse");
  assert!(
    matches!(reply, Reply::Text { status: 404, .. }),
    "{reply:?}"
  );
}

#[tokio::test]
async fn call_posts_when_a_body_is_given() {
  let backend = backend();
  let body = Some(Bytes::from_static(b"{\"a\":1}"));
  let reply = backend
    .call("/api/state", HashMap::new(), body)
    .await
    .expect("réponse");
  assert!(
    matches!(reply, Reply::Json { status: 200, .. }),
    "{reply:?}"
  );
  assert_eq!(backend.core.current_app_state(), json!({ "a": 1 }));
  let reply = backend
    .call("/api/state", HashMap::new(), None)
    .await
    .expect("réponse");
  assert!(
    matches!(reply, Reply::Text { status: 404, .. }),
    "{reply:?}"
  );
}

#[tokio::test]
async fn reply_bytes_reads_every_kind_of_body() {
  let image = Reply::Bytes {
    status: 200,
    content_type: "image/png".into(),
    body: Bytes::from_static(b"abc"),
  };
  assert_eq!(
    Backend::reply_bytes(image).await.unwrap(),
    Bytes::from_static(b"abc")
  );

  let stream = Reply::buffered(200, Vec::new(), Bytes::from_static(b"flux"));
  assert_eq!(
    Backend::reply_bytes(stream).await.unwrap(),
    Bytes::from_static(b"flux")
  );

  let dir = temp::TempDir::new("app-bytes");
  let path = dir.join("bytes");
  std::fs::write(&path, b"0123456789").unwrap();
  let whole = Reply::File {
    path: path.clone(),
    mime: "image/png".into(),
    range: None,
  };
  assert_eq!(
    Backend::reply_bytes(whole).await.unwrap(),
    Bytes::from_static(b"0123456789")
  );
  let part = Reply::File {
    path: path.clone(),
    mime: "image/png".into(),
    range: Some((2, 4)),
  };
  assert_eq!(
    Backend::reply_bytes(part).await.unwrap(),
    Bytes::from_static(b"234")
  );
}

#[tokio::test]
async fn reply_bytes_refuses_errors_and_json() {
  let not_found = Reply::Text {
    status: 404,
    text: "Not found".into(),
  };
  assert!(matches!(
    Backend::reply_bytes(not_found).await,
    Err(BackendError::Status(404))
  ));
  let json = Reply::Json {
    status: 200,
    body: json!({}),
  };
  assert!(matches!(
    Backend::reply_bytes(json).await,
    Err(BackendError::NotBytes)
  ));
}

#[tokio::test]
async fn a_published_app_state_arrives_as_a_message() {
  let backend = backend();
  let mut messages = Box::pin(core_sub::stream(&backend.core));
  let body = Some(Bytes::from_static(b"{\"a\":2}"));
  backend
    .call("/api/state", HashMap::new(), body)
    .await
    .expect("réponse");
  let received = tokio::time::timeout(Duration::from_secs(2), messages.next())
    .await
    .expect("message reçu à temps");
  let Some(Message::Core(CoreEvent::AppState(state))) = received else {
    panic!("attendu : AppState, reçu {received:?}");
  };
  assert_eq!(state, json!({ "a": 2 }));
}

#[tokio::test]
async fn the_stream_survives_a_slow_consumer() {
  let backend = backend();
  let mut messages = Box::pin(core_sub::stream(&backend.core));
  for index in 0..200 {
    let body = Some(Bytes::from(format!("{{\"n\":{index}}}")));
    backend
      .call("/api/state", HashMap::new(), body)
      .await
      .expect("réponse");
  }
  let received = tokio::time::timeout(Duration::from_secs(2), messages.next())
    .await
    .expect("message reçu à temps");
  assert!(
    matches!(received, Some(Message::Core(CoreEvent::AppState(_)))),
    "{received:?}"
  );
}
