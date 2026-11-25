//! Références de `tests/golden/` rejouées sur `riff_core::dispatch::call`, en mémoire, sans serveur HTTP.
//!
//! Les 91 fichiers ont été enregistrés sur l'ancien serveur HTTP (`src-tauri/src/server.rs`, retiré depuis). L'app native n'a pas de
//! serveur (`riff://local`, routeur `dispatch`) : seuls le statut et le corps sont comparés, jamais les en-têtes
//! enregistrés (CORS, `content-length`, `accept-ranges`, `content-range`, `content-type`). Les corps binaires sont
//! comparés par `sha256` et longueur, comme dans l'ancien `scripts/golden.ps1`.
//!
//! Catégories (le test `golden_files_are_all_accounted_for` impose que leur somme égale le nombre de fichiers) :
//! - 69 cas exacts : statut et corps identiques, dans l'ordre du script (état partagé entre les cas) ;
//! - 2 cas `None` : `05` et `06`, le 501 « pas encore porté » et le 404 « Not found » sont produits par le serveur ;
//! - 1 cas de forme non réseau : `67-spotify-auth-start` ;
//! - 13 cas réseau `79` à `91`, rejoués depuis `tests/fixtures/http` (forme comparée, zéro fixture manquante exigé) ;
//! - 6 cas sans objet : `01` à `04` (OPTIONS, origine et hôte interdits : couche HTTP qui ne sera pas livrée),
//!   `09-devsize-memes` (route retirée avec l'ancienne interface) et `12-lyrics-stream` (cadrage SSE du serveur ; la part du core, `subscribe` et `current_app_state`, est testée
//!   dans `core_env.rs`).

use bytes::Bytes;
use futures_util::StreamExt;
use riff_core::core::{Core, CoreConfig};
use riff_core::reply::{CoreResult, HeaderLite, Method, Params, Reply};
use riff_core::secrets::MemoryStore;
use riff_core::{dispatch, paths};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

#[path = "support/temp.rs"]
mod temp;

use temp::SharedDir;

const BASE: &str = "http://127.0.0.1:<PORT>";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
  /// Statut et corps exacts.
  Exact,
  /// `dispatch::call` doit renvoyer `None`.
  Absent,
  /// Statut exact, forme du JSON (clés conservées, valeurs remplacées par `*`).
  Shape,
  /// Comme `Shape`, via le rejeu réseau.
  Network,
  /// Produit par le serveur HTTP seulement.
  NotApplicable,
}

struct Case {
  n: u8,
  name: &'static str,
  method: &'static str,
  path: &'static str,
  body: Option<&'static str>,
  headers: &'static [(&'static str, &'static str)],
  kind: Kind,
}

const TRACK: &str = r#"{"items":[{"id":"local_0123456789ab","file":"local_0123456789ab.mp3","name":"01 - Artist A - Song One","ext":"mp3"}]}"#;

const fn c(
  n: u8,
  name: &'static str,
  method: &'static str,
  path: &'static str,
  kind: Kind,
) -> Case {
  Case {
    n,
    name,
    method,
    path,
    body: None,
    headers: &[],
    kind,
  }
}

const fn with_body(mut case: Case, body: &'static str) -> Case {
  case.body = Some(body);
  case
}

const fn with_headers(mut case: Case, headers: &'static [(&'static str, &'static str)]) -> Case {
  case.headers = headers;
  case
}

use Kind::{Absent, Exact, Network, NotApplicable, Shape};

/// Ordre et numéros de l'ancien `scripts/golden.ps1`.
fn cases() -> Vec<Case> {
  vec![
    c(1, "options", "OPTIONS", "/api/library", NotApplicable),
    c(2, "origin-interdite", "GET", "/api/library", NotApplicable),
    c(
      3,
      "origin-tauri",
      "GET",
      "/api/audio-output-info",
      NotApplicable,
    ),
    c(4, "host-interdit", "GET", "/api/library", NotApplicable),
    c(5, "route-api-inconnue", "GET", "/api/n-existe-pas", Absent),
    c(6, "route-inconnue", "GET", "/autre", Absent),
    c(7, "state-mauvaise-methode", "GET", "/api/state", Exact),
    c(
      8,
      "audio-output-info",
      "GET",
      "/api/audio-output-info",
      Exact,
    ),
    c(
      9,
      "devsize-memes",
      "GET",
      "/api/devsize-memes",
      NotApplicable,
    ),
    with_body(
      c(10, "state", "POST", "/api/state", Exact),
      r#"{"currentTrack":{"id":"testvid1234","title":"Song One"},"isPlaying":true}"#,
    ),
    with_body(
      c(11, "state-json-invalide", "POST", "/api/state", Exact),
      "{pas du json",
    ),
    c(
      12,
      "lyrics-stream",
      "GET",
      "/api/lyrics-stream",
      NotApplicable,
    ),
    c(13, "library-vide", "GET", "/api/library", Exact),
    with_body(
      c(14, "library-add", "POST", "/api/library-add", Exact),
      TRACK,
    ),
    with_body(
      c(
        15,
        "library-add-invalide",
        "POST",
        "/api/library-add",
        Exact,
      ),
      "{pas du json",
    ),
    c(16, "library", "GET", "/api/library", Exact),
    with_body(
      c(17, "library-update", "POST", "/api/library-update", Exact),
      r#"{"id":"local_0123456789ab","duration":215}"#,
    ),
    c(18, "library-apres-update", "GET", "/api/library", Exact),
    with_headers(
      c(
        19,
        "library-file-range",
        "GET",
        "/api/library-file?id=local_0123456789ab",
        Exact,
      ),
      &[("Range", "bytes=0-15")],
    ),
    c(
      20,
      "library-file-complet",
      "GET",
      "/api/library-file?id=local_0123456789ab",
      Exact,
    ),
    with_headers(
      c(
        21,
        "library-file-416",
        "GET",
        "/api/library-file?id=local_0123456789ab",
        Exact,
      ),
      &[("Range", "bytes=999999-")],
    ),
    c(
      22,
      "library-file-absent",
      "GET",
      "/api/library-file?id=local_ffffffffffff",
      Exact,
    ),
    with_body(
      c(23, "library-remove", "POST", "/api/library-remove", Exact),
      r#"{"id":"local_0123456789ab"}"#,
    ),
    c(24, "library-apres-remove", "GET", "/api/library", Exact),
    with_headers(
      c(
        25,
        "local-track-range",
        "GET",
        "/api/local-track?id=testvid1234",
        Exact,
      ),
      &[("Range", "bytes=0-31")],
    ),
    c(
      26,
      "local-track-complet",
      "GET",
      "/api/local-track?id=testvid1234",
      Exact,
    ),
    c(
      27,
      "local-track-absent",
      "GET",
      "/api/local-track?id=inconnu",
      Exact,
    ),
    with_headers(
      c(
        28,
        "downloaded-track-range",
        "GET",
        "/api/downloaded-track?name=song.mp3",
        Exact,
      ),
      &[("Range", "bytes=0-9")],
    ),
    c(
      29,
      "downloaded-track-absent",
      "GET",
      "/api/downloaded-track?name=absent.mp3",
      Exact,
    ),
    c(
      30,
      "local-thumbnail",
      "GET",
      "/api/local-thumbnail?id=testvid1234",
      Exact,
    ),
    c(
      31,
      "local-thumbnail-absent",
      "GET",
      "/api/local-thumbnail?id=inconnu",
      Exact,
    ),
    c(32, "offline-library", "GET", "/api/offline-library", Exact),
    c(
      33,
      "check-cache",
      "GET",
      "/api/check-cache?id=testvid1234",
      Exact,
    ),
    c(
      34,
      "check-cache-absent",
      "GET",
      "/api/check-cache?id=inconnu",
      Exact,
    ),
    with_body(
      c(
        35,
        "track-analysis-post",
        "POST",
        "/api/track-analysis",
        Exact,
      ),
      r#"{"id":"testvid1234","analysis":{"bpm":120}}"#,
    ),
    c(
      36,
      "track-analysis-get",
      "GET",
      "/api/track-analysis?id=testvid1234",
      Exact,
    ),
    with_body(
      c(
        37,
        "track-analysis-invalide",
        "POST",
        "/api/track-analysis",
        Exact,
      ),
      "[1]",
    ),
    c(
      38,
      "offline-delete",
      "POST",
      "/api/offline-delete?id=testvid1234",
      Exact,
    ),
    c(
      39,
      "check-cache-apres-delete",
      "GET",
      "/api/check-cache?id=testvid1234",
      Exact,
    ),
    c(
      40,
      "clear-offline-library",
      "POST",
      "/api/clear-offline-library",
      Exact,
    ),
    c(
      41,
      "offline-library-vide",
      "GET",
      "/api/offline-library",
      Exact,
    ),
    c(42, "tools-status", "GET", "/api/tools-status", Exact),
    c(43, "tools-status-alias", "GET", "/api/tools/status", Exact),
    with_body(
      c(44, "proxy-config", "POST", "/api/proxy-config", Exact),
      r#"{"enabled":false}"#,
    ),
    c(45, "proxy-test-get", "GET", "/api/proxy-test", Exact),
    with_body(
      c(
        46,
        "save-custom-lyrics",
        "POST",
        "/api/save-custom-lyrics?title=Song%20One&artist=Artist%20A",
        Exact,
      ),
      r#"{"lyrics":"[00:01.00] Hello world"}"#,
    ),
    with_body(
      c(
        47,
        "save-custom-lyrics-vide",
        "POST",
        "/api/save-custom-lyrics?title=Song%20One&artist=Artist%20A",
        Exact,
      ),
      "{}",
    ),
    c(
      48,
      "lyrics-custom",
      "GET",
      "/api/lyrics?title=Song%20One&artist=Artist%20A&online=0",
      Exact,
    ),
    c(
      49,
      "tmp-file",
      "GET",
      "/api/tmp-file?name=preview.wav",
      Exact,
    ),
    c(
      50,
      "tmp-file-nom-invalide",
      "GET",
      "/api/tmp-file?name=..%2Fevil.wav",
      Exact,
    ),
    c(
      51,
      "tmp-file-absent",
      "GET",
      "/api/tmp-file?name=absent.wav",
      Exact,
    ),
    c(52, "brand-icon-sans-nom", "GET", "/api/brand-icon", Exact),
    c(
      53,
      "image-proxy-url-invalide",
      "GET",
      "/api/image-proxy?url=ftp://x",
      Exact,
    ),
    c(54, "audio-proxy-sans-url", "GET", "/api/audio-proxy", Exact),
    c(
      55,
      "audio-proxy-ssrf",
      "GET",
      "/api/audio-proxy?url=http://127.0.0.1:1/x",
      Exact,
    ),
    with_body(
      c(
        56,
        "audio-proxy-post",
        "POST",
        "/api/audio-proxy?url=https://example.com/x",
        Exact,
      ),
      "{}",
    ),
    c(57, "encode-get", "GET", "/api/encode", Exact),
    c(58, "download-sans-id", "GET", "/api/download", Exact),
    c(
      59,
      "playlist-import-sans-url",
      "GET",
      "/api/playlist-import",
      Exact,
    ),
    c(
      60,
      "spotify-import-sans-url",
      "GET",
      "/api/spotify-import",
      Exact,
    ),
    c(
      61,
      "apple-import-sans-url",
      "GET",
      "/api/apple-import",
      Exact,
    ),
    c(
      62,
      "yandex-import-sans-url",
      "GET",
      "/api/yandex-import",
      Exact,
    ),
    c(63, "spotify-status", "GET", "/api/spotify-status", Exact),
    with_body(
      c(64, "spotify-logout", "POST", "/api/spotify-logout", Exact),
      "{}",
    ),
    c(
      65,
      "spotify-library-sans-session",
      "GET",
      "/api/spotify-library",
      Exact,
    ),
    c(
      66,
      "spotify-callback-sans-code",
      "GET",
      "/api/spotify-callback",
      Exact,
    ),
    c(
      67,
      "spotify-auth-start",
      "GET",
      "/api/spotify-auth-start",
      Shape,
    ),
    with_body(
      c(68, "lastfm-inconnue", "POST", "/api/lastfm/inconnue", Exact),
      "{}",
    ),
    c(69, "stream-info-sans-id", "GET", "/api/stream-info", Exact),
    c(70, "prefetch-sans-id", "GET", "/api/prefetch", Exact),
    c(71, "search-sans-requete", "GET", "/api/search", Exact),
    with_body(
      c(72, "translate-vide", "POST", "/api/translate", Exact),
      "{}",
    ),
    with_body(
      c(73, "i18n-batch-vide", "POST", "/api/i18n-batch", Exact),
      "{}",
    ),
    c(74, "artist-sans-nom", "GET", "/api/artist", Exact),
    c(75, "albums-search-vide", "GET", "/api/albums/search", Exact),
    c(76, "album-tracks-vide", "GET", "/api/albums/tracks", Exact),
    c(77, "lyrics-words-vide", "GET", "/api/lyrics-words", Exact),
    c(78, "track-cover-vide", "GET", "/api/track-cover", Exact),
    c(79, "net-hot", "GET", "/api/hot", Network),
    c(80, "net-trending", "GET", "/api/trending", Network),
    c(
      81,
      "net-albums-search",
      "GET",
      "/api/albums/search?q=daft%20punk",
      Network,
    ),
    c(
      82,
      "net-album-tracks",
      "GET",
      "/api/albums/tracks?title=Discovery&artist=Daft%20Punk",
      Network,
    ),
    c(
      83,
      "net-track-cover",
      "GET",
      "/api/track-cover?artist=Daft%20Punk&title=One%20More%20Time",
      Network,
    ),
    c(
      84,
      "net-lyrics",
      "GET",
      "/api/lyrics?title=One%20More%20Time&artist=Daft%20Punk",
      Network,
    ),
    c(
      85,
      "net-lyrics-words",
      "GET",
      "/api/lyrics-words?title=One%20More%20Time&artist=Daft%20Punk",
      Network,
    ),
    c(
      86,
      "net-search",
      "GET",
      "/api/search?q=daft%20punk&source=ytmusic",
      Network,
    ),
    c(
      87,
      "net-artist",
      "GET",
      "/api/artist?name=Daft%20Punk",
      Network,
    ),
    with_body(
      c(88, "net-vibe", "POST", "/api/vibe", Network),
      r#"{"seeds":[{"title":"One More Time","artist":"Daft Punk"}],"limit":5}"#,
    ),
    c(
      89,
      "net-brand-icon",
      "GET",
      "/api/brand-icon?name=spotify",
      Network,
    ),
    with_body(
      c(90, "net-translate", "POST", "/api/translate", Network),
      r#"{"texts":["hello"],"targetLang":"fr"}"#,
    ),
    c(91, "net-lastfm-token", "GET", "/api/lastfm/token", Network),
  ]
}

fn golden_dir() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

fn pattern_file(path: &Path, size: usize) {
  fs::create_dir_all(path.parent().expect("parent")).expect("dossier");
  let bytes: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
  fs::write(path, bytes).expect("fichier");
}

/// Un seul dossier par exécution, comme `core_env.rs`. Les données reproduisent celles de l'ancien `scripts/golden.ps1`.
fn root() -> PathBuf {
  static DIR: SharedDir = SharedDir::new("core-golden");
  static ENV: Once = Once::new();
  let dir = DIR.path();
  let data = dir.join("data");
  let downloads = dir.join("downloads");
  ENV.call_once(|| {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/http");
    // SAFETY : `call_once` bloque les autres tests tant que les variables ne sont pas posées, et ce binaire
    // n'a aucun autre fil qui lit l'environnement avant le premier appel à `root`.
    unsafe {
      std::env::set_var("RIFF_DATA_DIR", &data);
      std::env::set_var("LOCALAPPDATA", dir.join("local"));
      std::env::set_var("XDG_CONFIG_HOME", dir.join("xdg"));
      // macOS résout le dossier de config depuis le dossier personnel : jamais le vrai.
      #[cfg(unix)]
      std::env::set_var("HOME", dir.join("home"));
      std::env::set_var(
        "RIFF_KEYRING_SERVICE",
        format!("riff-rs-golden-{}", std::process::id()),
      );
      std::env::set_var("RIFF_YTDLP", dir.join("tools/yt-dlp"));
      std::env::set_var("RIFF_FFMPEG", dir.join("tools/ffmpeg"));
      // Toute requête réseau passe par le rejeu : sans fixture elle échoue, jamais de réseau réel.
      std::env::set_var("RIFF_HTTP_FIXTURES", &fixtures);
    }
  });
  DIR.lease(|dir| {
    pattern_file(&data.join("tracks/testvid1234.mp3"), 70000);
    pattern_file(&data.join("thumbnails/testvid1234.jpg"), 10);
    pattern_file(&data.join("library/local_0123456789ab.mp3"), 70000);
    pattern_file(&downloads.join(".tmp/preview.wav"), 16);
    pattern_file(&downloads.join("song.mp3"), 1000);
    pattern_file(&dir.join("tools/yt-dlp"), 4);
    pattern_file(&dir.join("tools/ffmpeg"), 4);
    fs::write(
      data.join("metadata.json"),
      r#"{"testvid1234":{"id":"testvid1234","title":"Song One","artist":"Artist A","duration":215.5,"thumbnail":"","platform":"youtube","lastPlayed":1700000000000}}"#,
    )
    .expect("metadata");
    let config = paths::config_path();
    fs::create_dir_all(config.parent().expect("parent")).expect("dossier de config");
    // La traduction est coupée par défaut (envoi de texte à un tiers) ; le cas 90 a été enregistré avec elle active.
    fs::write(
      config,
      json!({ "downloadDir": downloads, "translationEnabled": true }).to_string(),
    )
    .expect("config");
  })
}

fn new_core() -> Arc<Core> {
  root();
  Core::new(CoreConfig {
    local_base_url: BASE.to_string(),
    secret_store: Some(Arc::new(MemoryStore::default())),
  })
}

fn method_of(name: &str) -> Method {
  match name {
    "GET" => Method::Get,
    "HEAD" => Method::Head,
    "POST" => Method::Post,
    _ => Method::Other,
  }
}

fn params_of(case: &Case) -> (Method, &str, Params) {
  let (path, query) = case.path.split_once('?').unwrap_or((case.path, ""));
  let query: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
    .map(|(k, v)| (k.into_owned(), v.into_owned()))
    .collect();
  let params = Params {
    query,
    body: case
      .body
      .map(|b| Bytes::copy_from_slice(b.as_bytes()))
      .unwrap_or_default(),
    headers: HeaderLite::new(
      case
        .headers
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect(),
    ),
  };
  (method_of(case.method), path, params)
}

fn sha_text(bytes: &[u8]) -> String {
  let digest = Sha256::digest(bytes);
  let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
  format!("sha256:{hex} len:{}", bytes.len())
}

/// Corps normalisé comme l'ancien `scripts/golden.ps1` : binaire audio ou image en empreinte, sinon JSON ou texte.
fn body_value(content_type: &str, bytes: &[u8], shape: bool) -> Value {
  if content_type.starts_with("audio/") || content_type.starts_with("image/") {
    return Value::String(if shape {
      "(binaire)".to_string()
    } else {
      sha_text(bytes)
    });
  }
  let raw = String::from_utf8_lossy(bytes);
  let text = normalize(&raw);
  match serde_json::from_str::<Value>(&text) {
    Ok(parsed) if !text.is_empty() => {
      if shape {
        shape_of(&parsed)
      } else {
        parsed
      }
    }
    _ if shape => Value::String("(texte)".to_string()),
    _ => Value::String(text),
  }
}

/// Chemins du dossier temporaire et port remplacés par des jetons stables.
fn normalize(text: &str) -> String {
  let root = root().to_string_lossy().into_owned();
  text
    .replace(&root.replace('\\', "\\\\"), "<ROOT>")
    .replace(&root, "<ROOT>")
}

/// Forme d'un JSON : clés conservées, valeurs remplacées, tableaux réduits à une seule forme qui réunit les clés de
/// tous leurs éléments (le tirage aléatoire de `vibe` change quel titre vient en premier, et ses clés).
fn shape_of(value: &Value) -> Value {
  match value {
    Value::Object(map) => Value::Object(
      map
        .iter()
        .map(|(k, v)| (k.clone(), shape_of(v)))
        .collect::<Map<_, _>>(),
    ),
    Value::Array(items) => Value::Array(
      items
        .iter()
        .map(shape_of)
        .reduce(merge_shapes)
        .into_iter()
        .collect(),
    ),
    _ => Value::String("*".to_string()),
  }
}

fn merge_shapes(a: Value, b: Value) -> Value {
  match (a, b) {
    (Value::Object(mut a), Value::Object(b)) => {
      for (k, v) in b {
        let merged = match a.remove(&k) {
          Some(existing) => merge_shapes(existing, v),
          None => v,
        };
        a.insert(k, merged);
      }
      Value::Object(a)
    }
    (Value::Array(a), Value::Array(b)) => Value::Array(
      a.into_iter()
        .chain(b)
        .reduce(merge_shapes)
        .into_iter()
        .collect(),
    ),
    (a, _) => a,
  }
}

struct Outcome {
  status: u16,
  body: Value,
}

async fn outcome(result: CoreResult, shape: bool) -> Outcome {
  let reply = match result {
    Ok(reply) => reply,
    // Même traduction que `reply_to_response` de l'ancien `src-tauri/src/server.rs`.
    Err(err) => {
      let body = json!({ "success": false, "error": err.to_string() });
      return Outcome {
        status: err.status(),
        body: if shape { shape_of(&body) } else { body },
      };
    }
  };
  let status = reply.status();
  let body = match reply {
    Reply::Json { body, .. } => {
      let body = serde_json::from_str(&normalize(&body.to_string())).expect("JSON normalisé");
      if shape { shape_of(&body) } else { body }
    }
    Reply::Text { text, .. } => body_value("text/plain", text.as_bytes(), shape),
    Reply::Bytes {
      content_type, body, ..
    } => body_value(&content_type, &body, shape),
    Reply::File { path, mime, range } => {
      let all = fs::read(&path).expect("fichier servi");
      let bytes = match range {
        Some((start, end)) => all[start as usize..=end as usize].to_vec(),
        None => all,
      };
      body_value(&mime, &bytes, shape)
    }
    Reply::Stream {
      headers, mut body, ..
    } => {
      let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
      let mut bytes = Vec::new();
      while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk.expect("flux"));
      }
      body_value(&content_type, &bytes, shape)
    }
  };
  Outcome { status, body }
}

fn golden_of(case: &Case) -> Value {
  let file = golden_dir().join(format!("{:02}-{}.json", case.n, case.name));
  let text = fs::read_to_string(&file).unwrap_or_else(|e| panic!("{} : {e}", file.display()));
  let mut golden: Value = serde_json::from_str(&text).expect("JSON de référence");
  // Enregistré sous Windows (`false`) ; en production ffmpeg est optionnel sous macOS seulement.
  if matches!(case.n, 42 | 43) {
    golden["body"]["ffmpegOptional"] = json!(cfg!(target_os = "macos"));
  }
  golden
}

fn expected_status(golden: &Value) -> u16 {
  golden["status"]
    .as_str()
    .and_then(|s| s.parse().ok())
    .expect("statut de référence")
}

fn short(value: &Value) -> String {
  let text = value.to_string();
  if text.chars().count() > 400 {
    format!("{}…", text.chars().take(400).collect::<String>())
  } else {
    text
  }
}

/// `None` si le cas est conforme, sinon le résumé de l'écart.
async fn check(core: &Arc<Core>, case: &Case, shape: bool) -> Option<String> {
  let (method, path, params) = params_of(case);
  let result = dispatch::call(core, method, path, params).await?;
  let got = outcome(result, shape).await;
  let golden = golden_of(case);
  let want_status = expected_status(&golden);
  let want_body = &golden["body"];
  if got.status == want_status && &got.body == want_body {
    return None;
  }
  Some(format!(
    "{:02}-{} : statut {} (attendu {want_status}) ; corps obtenu {} ; corps attendu {}",
    case.n,
    case.name,
    got.status,
    short(&got.body),
    short(want_body)
  ))
}

#[tokio::test]
async fn local_cases_match_the_references() {
  let core = new_core();
  let mut diffs = Vec::new();
  let mut compared = 0;
  for case in cases() {
    if !matches!(case.kind, Exact | Absent | Shape) {
      continue;
    }
    let (method, path, params) = params_of(&case);
    if case.kind == Absent {
      let reply = dispatch::call(&core, method, path, params).await;
      assert!(
        reply.is_none(),
        "{:02}-{} : une route existe",
        case.n,
        case.name
      );
      compared += 1;
      continue;
    }
    if let Some(diff) = check(&core, &case, case.kind == Shape).await {
      diffs.push(diff);
    }
    compared += 1;
  }
  assert_eq!(compared, 72, "69 exacts + 2 absents + 1 forme");
  assert!(
    diffs.is_empty(),
    "{} écart(s) :\n{}",
    diffs.len(),
    diffs.join("\n")
  );
}

async fn network_case(n: u8) {
  let core = new_core();
  let all = cases();
  let case = all.iter().find(|c| c.n == n).expect("cas");
  assert_eq!(case.kind, Network);
  let diff = check(&core, case, true).await;
  // La forme est la même pour un succès et pour une erreur avalée : seule l'absence de fixture manquante prouve le rejeu.
  let misses = riff_core::net::replay::take_misses();
  assert!(
    misses.is_empty(),
    "fixtures manquantes :\n{}",
    misses.join("\n")
  );
  if let Some(diff) = diff {
    panic!("{diff}");
  }
}

macro_rules! network_tests {
  ($($name:ident => $n:expr),* $(,)?) => {
    $(
      #[tokio::test]
      async fn $name() {
        network_case($n).await;
      }
    )*
  };
}

network_tests! {
  net_hot => 79,
  net_trending => 80,
  net_albums_search => 81,
  net_album_tracks => 82,
  net_track_cover => 83,
  net_lyrics => 84,
  net_lyrics_words => 85,
  net_search => 86,
  net_artist => 87,
  net_vibe => 88,
  net_brand_icon => 89,
  net_translate => 90,
  net_lastfm_token => 91,
}

/// Un fichier ajouté ou oublié dans `tests/golden/` fait échouer ce test.
#[test]
fn golden_files_are_all_accounted_for() {
  let all = cases();
  let count = |kind: Kind| all.iter().filter(|c| c.kind == kind).count();
  assert_eq!(count(Exact), 69);
  assert_eq!(count(Absent), 2);
  assert_eq!(count(Shape), 1);
  assert_eq!(count(Network), 13);
  assert_eq!(count(NotApplicable), 6);

  let mut on_disk: Vec<String> = fs::read_dir(golden_dir())
    .expect("tests/golden")
    .map(|e| {
      e.expect("entrée")
        .file_name()
        .to_string_lossy()
        .into_owned()
    })
    .collect();
  on_disk.sort();
  let mut listed: Vec<String> = all
    .iter()
    .map(|c| format!("{:02}-{}.json", c.n, c.name))
    .collect();
  listed.sort();
  assert_eq!(
    on_disk, listed,
    "fichiers de référence et cas déclarés différents"
  );

  for case in &all {
    let golden = golden_of(case);
    assert_eq!(
      golden["request"],
      format!("{} {}", case.method, case.path),
      "{:02}-{}",
      case.n,
      case.name
    );
    let expected_kind = match case.kind {
      Exact | Absent => "local",
      Shape | Network => "shape",
      NotApplicable => {
        assert!(["local", "headers"].contains(&golden["kind"].as_str().unwrap_or("")));
        continue;
      }
    };
    assert_eq!(golden["kind"], expected_kind, "{:02}-{}", case.n, case.name);
  }
}
