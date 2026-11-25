//! Rejeu des réponses réseau, pour les tests. Compilé seulement avec la feature `replay`, jamais par défaut ni en release.
//!
//! Une requête est identifiée par sa méthode, son URL normalisée (sans paramètres de suivi, sans fragment, paramètres
//! triés, secrets masqués par [`redact`]) et le hachage SHA-256 de son corps. La réponse est lue dans
//! `<dossier>/<hôte>/<hash>.json`. Sans fixture, la requête échoue avec l'URL manquante : le réseau réel n'est jamais
//! atteint, sauf avec `RIFF_HTTP_RECORD=1`, qui enregistre alors la réponse réelle (après contrôle des secrets).
//!
//! Les paramètres secrets (voir [`redact`]) entrent dans la clé par un condensé SHA-256 de leur valeur : jamais en clair.
//! Limite connue : le rejeu ne suit pas les redirections. La fixture d'une requête qui a été redirigée porte la réponse
//! finale (`final_url`) ; une fixture 3xx écrite à la main est servie telle quelle. La politique réseau par saut est
//! testée dans `net.rs`.
//!
//! Activation : `RIFF_HTTP_FIXTURES=<dossier>` avant la construction du client, ou `Http::with_replay(Replay::new(dossier))`.

use super::Error;
use crate::logging::redact;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use reqwest::{ResponseBuilderExt, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use url::Url;

/// Plafond d'un corps enregistré.
const RECORD_MAX_BYTES: usize = 16 * 1024 * 1024;
/// En-têtes de réponse conservés. Tout le reste (`Set-Cookie`, `Authorization`...) est écarté.
const KEPT_HEADERS: &[&str] = &[
  "content-type",
  "content-language",
  "location",
  "retry-after",
];

/// Fixture sur disque. Les champs sans `default` sont obligatoires, y compris pour une fixture écrite à la main.
#[derive(Debug, Serialize, Deserialize)]
pub struct Fixture {
  /// Date d'enregistrement (RFC 3339), informative.
  #[serde(default)]
  pub recorded_at: String,
  pub method: String,
  /// URL normalisée et masquée (informative : la clé est le nom du fichier).
  pub url: String,
  /// SHA-256 hexadécimal du corps de la requête (informatif).
  #[serde(default)]
  pub body_sha256: String,
  pub status: u16,
  #[serde(default)]
  pub headers: BTreeMap<String, String>,
  /// URL finale après redirections, si elle diffère de l'URL demandée.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub final_url: Option<String>,
  /// Corps en texte UTF-8.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<String>,
  /// Corps binaire en base64 (si `body` est absent).
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body_base64: Option<String>,
}

fn is_tracking(key: &str) -> bool {
  let key = key.to_ascii_lowercase();
  key.starts_with("utm_")
    || matches!(
      key.as_str(),
      "fbclid"
        | "gclid"
        | "dclid"
        | "msclkid"
        | "yclid"
        | "igshid"
        | "mc_cid"
        | "mc_eid"
        | "_ga"
        | "_gl"
        | "si"
    )
}

/// Forme canonique d'une URL pour la clé : sans identifiants, fragment ni paramètres de suivi, paramètres triés.
#[must_use]
pub fn normalize_url(url: &Url) -> String {
  let mut url = url.clone();
  url.set_fragment(None);
  let _ = url.set_username("");
  let _ = url.set_password(None);
  let mut pairs: Vec<(String, String)> = url
    .query_pairs()
    .filter(|(k, _)| !is_tracking(k))
    .map(|(k, v)| (k.into_owned(), v.into_owned()))
    .collect();
  pairs.sort();
  if pairs.is_empty() {
    url.set_query(None);
  } else {
    url.query_pairs_mut().clear().extend_pairs(&pairs);
  }
  url.into()
}

/// Paramètres que [`redact`] masque, avec leur valeur réelle, dans l'ordre de l'URL.
fn secret_pairs(url: &Url) -> Vec<(String, String)> {
  url
    .query_pairs()
    .filter(|(k, v)| {
      let pair = format!("{k}={v}");
      redact(&pair) != pair
    })
    .map(|(k, v)| (k.into_owned(), v.into_owned()))
    .collect()
}

/// Un champ JSON `"code":<nombre>` (statut d'erreur d'une API) n'est pas un secret : on le renomme avant le contrôle.
/// Une valeur entre guillemets, `sk`, `*token` et tous les autres noms restent refusés par [`redact`].
fn without_numeric_codes(text: &str) -> Cow<'_, str> {
  static NUMERIC_CODE: OnceLock<regex::Regex> = OnceLock::new();
  NUMERIC_CODE
    .get_or_init(|| regex::Regex::new(r#""code"(\s*:\s*-?\d+(?:\.\d+)?\s*[,}\]])"#).expect("regex"))
    .replace_all(text, r#""numeric_code"$1"#)
}

fn hex(bytes: &[u8]) -> String {
  bytes.iter().fold(String::new(), |mut out, b| {
    let _ = write!(out, "{b:02x}");
    out
  })
}

/// Identité d'une requête : de quoi retrouver ou écrire sa fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Id {
  method: String,
  /// URL normalisée, telle qu'elle apparaît dans les messages d'erreur.
  url: String,
  body_sha256: String,
  host_dir: String,
  hash: String,
}

impl Id {
  /// Règle de calcul : `hash = SHA-256(méthode "\n" masquer(url normalisée) "\n" SHA-256(corps))`, 32 premiers
  /// caractères hexadécimaux. Le dossier est l'hôte (et `_port`), nettoyé pour être un nom de dossier valide.
  #[must_use]
  pub fn new(method: &str, url: &Url, body: &[u8]) -> Self {
    let normalized = normalize_url(url);
    let body_sha256 = hex(&Sha256::digest(body));
    let method = method.to_ascii_uppercase();
    let mut material = format!("{method}\n{}\n{body_sha256}", redact(&normalized));
    // `redact` masque la valeur des paramètres secrets : sans ceci, `code=a` et `code=b` auraient la même clé. La
    // valeur n'entre que par son condensé, jamais en clair, ni dans le nom du fichier ni dans l'URL de la fixture.
    for (key, value) in secret_pairs(url) {
      let digest = hex(&Sha256::digest(value.as_bytes()));
      let _ = write!(material, "\n{key}:{}", &digest[..16]);
    }
    let hash = hex(&Sha256::digest(material.as_bytes()))[..32].to_string();
    let mut host = url.host_str().unwrap_or("").to_string();
    if let Some(port) = url.port() {
      let _ = write!(host, "_{port}");
    }
    let host_dir = host
      .chars()
      .map(|c| {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
          c
        } else {
          '_'
        }
      })
      .collect();
    Self {
      method,
      url: normalized,
      body_sha256,
      host_dir,
      hash,
    }
  }

  fn from_request(request: &reqwest::Request) -> Result<Self, Error> {
    let body = match request.body() {
      None => &[][..],
      Some(body) => body
        .as_bytes()
        .ok_or_else(|| Error::Replay("request body is a stream: cannot be keyed".into()))?,
    };
    Ok(Self::new(request.method().as_str(), request.url(), body))
  }
}

/// Chemin de la fixture d'une requête, pour écrire une fixture à la main.
#[must_use]
pub fn fixture_path(dir: &Path, method: &str, url: &Url, body: &[u8]) -> PathBuf {
  let id = Id::new(method, url, body);
  dir.join(&id.host_dir).join(format!("{}.json", id.hash))
}

thread_local! {
  static MISSES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Fixtures manquantes vues par le fil courant depuis le dernier appel (le code appelant peut avaler l'erreur : un
/// test qui exige zéro manque doit lire cette liste). Fiable avec un exécuteur mono-fil (`#[tokio::test]`).
#[must_use]
pub fn take_misses() -> Vec<String> {
  MISSES.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

/// Transport de rejeu : lit (et, sur demande, enregistre) des fixtures.
#[derive(Debug, Clone)]
pub struct Replay {
  dir: PathBuf,
  record: bool,
}

impl Replay {
  /// Rejeu depuis `dir`. L'enregistrement est actif si `RIFF_HTTP_RECORD=1`.
  #[must_use]
  pub fn new(dir: impl Into<PathBuf>) -> Self {
    Self {
      dir: dir.into(),
      record: std::env::var("RIFF_HTTP_RECORD").is_ok_and(|v| v == "1"),
    }
  }

  /// Rejeu depuis `RIFF_HTTP_FIXTURES`, s'il est défini et non vide.
  #[must_use]
  pub fn from_env() -> Option<Self> {
    std::env::var_os("RIFF_HTTP_FIXTURES")
      .filter(|v| !v.is_empty())
      .map(Self::new)
  }

  /// Force l'enregistrement, indépendamment de `RIFF_HTTP_RECORD`.
  #[must_use]
  pub fn recording(mut self, on: bool) -> Self {
    self.record = on;
    self
  }

  fn path(&self, id: &Id) -> PathBuf {
    self
      .dir
      .join(&id.host_dir)
      .join(format!("{}.json", id.hash))
  }

  /// Remplace l'envoi réseau. La fixture manquante donne [`Error::Replay`], sauf en mode enregistrement.
  pub(super) async fn execute(
    &self,
    client: &reqwest::Client,
    request: reqwest::Request,
    max_bytes: u64,
  ) -> Result<reqwest::Response, Error> {
    let id = Id::from_request(&request)?;
    let path = self.path(&id);
    match std::fs::read(&path) {
      Ok(bytes) => {
        let fixture: Fixture = serde_json::from_slice(&bytes)
          .map_err(|e| Error::Replay(format!("invalid fixture {}: {e}", path.display())))?;
        fixture.into_response(request.url())
      }
      Err(e) if e.kind() == ErrorKind::NotFound && self.record => {
        let response = client.execute(request).await?;
        self.record_response(&id, response, max_bytes).await
      }
      Err(e) if e.kind() == ErrorKind::NotFound => {
        let message = format!(
          "no fixture for {} {} (expected {})",
          id.method,
          id.url,
          path.display()
        );
        MISSES.with(|m| m.borrow_mut().push(message.clone()));
        Err(Error::Replay(message))
      }
      Err(e) => Err(Error::Replay(format!(
        "cannot read {}: {e}",
        path.display()
      ))),
    }
  }

  async fn record_response(
    &self,
    id: &Id,
    mut response: reqwest::Response,
    max_bytes: u64,
  ) -> Result<reqwest::Response, Error> {
    let status = response.status().as_u16();
    let final_url = response.url().clone();
    let headers = response
      .headers()
      .iter()
      .filter(|(k, _)| KEPT_HEADERS.contains(&k.as_str()))
      .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
      .collect();
    let limit = RECORD_MAX_BYTES.min(usize::try_from(max_bytes).unwrap_or(usize::MAX));
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
      body.extend_from_slice(&chunk);
      if body.len() > limit {
        return Err(Error::Replay("response too large to record".into()));
      }
    }
    let fixture = Fixture::capture(id, status, headers, &final_url, &body);
    self.write(id, &fixture)?;
    fixture.into_response(&final_url)
  }

  /// Écrit la fixture, sauf si elle contient un motif de secret connu.
  ///
  /// # Errors
  /// [`Error::Replay`] si [`redact`] modifierait la réponse (rien n'est écrit) ou si l'écriture échoue.
  pub fn write(&self, id: &Id, fixture: &Fixture) -> Result<PathBuf, Error> {
    let body = fixture.body.as_deref().map(without_numeric_codes);
    let texts = body
      .iter()
      .map(AsRef::as_ref)
      .chain(fixture.headers.values().map(String::as_str))
      .chain(fixture.final_url.as_deref());
    for text in texts {
      if redact(text) != text {
        return Err(Error::Replay(format!(
          "refused to record {} {}: the response contains a secret pattern",
          id.method, id.url
        )));
      }
    }
    let path = self.path(id);
    let json = serde_json::to_vec_pretty(fixture)
      .map_err(|e| Error::Replay(format!("cannot serialize fixture: {e}")))?;
    let write = || -> std::io::Result<()> {
      if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
      }
      std::fs::write(&path, json)
    };
    write().map_err(|e| Error::Replay(format!("cannot write {}: {e}", path.display())))?;
    Ok(path)
  }
}

impl Fixture {
  /// Fixture d'une réponse reçue. L'URL est masquée ; l'URL finale n'est conservée que si elle diffère.
  #[must_use]
  pub fn capture(
    id: &Id,
    status: u16,
    headers: BTreeMap<String, String>,
    final_url: &Url,
    body: &[u8],
  ) -> Self {
    let final_url = normalize_url(final_url);
    let (body, body_base64) = match std::str::from_utf8(body) {
      Ok(text) => (Some(text.to_string()), None),
      Err(_) => (None, Some(BASE64.encode(body))),
    };
    Self {
      recorded_at: jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string(),
      method: id.method.clone(),
      url: redact(&id.url),
      body_sha256: id.body_sha256.clone(),
      status,
      headers,
      final_url: (final_url != id.url).then(|| redact(&final_url)),
      body,
      body_base64,
    }
  }

  fn into_response(self, requested: &Url) -> Result<reqwest::Response, Error> {
    let bad = |what: &str| Error::Replay(format!("invalid fixture for {}: {what}", self.url));
    let body = match (&self.body, &self.body_base64) {
      (Some(text), _) => text.clone().into_bytes(),
      (None, Some(b64)) => BASE64.decode(b64).map_err(|_| bad("body_base64"))?,
      (None, None) => Vec::new(),
    };
    let final_url = self
      .final_url
      .as_deref()
      .map_or_else(|| Ok(requested.clone()), Url::parse)
      .map_err(|_| bad("final_url"))?;
    let mut builder = http::Response::builder()
      .status(StatusCode::from_u16(self.status).map_err(|_| bad("status"))?)
      .url(final_url);
    for (key, value) in &self.headers {
      builder = builder.header(key, value);
    }
    let response = builder
      .body(bytes::Bytes::from(body))
      .map_err(|_| bad("headers"))?;
    Ok(reqwest::Response::from(response))
  }
}

#[cfg(test)]
mod tests {
  use super::super::{Error, Http};
  use super::*;
  use crate::test_temp::TempDir;

  /// Dossier temporaire neuf par test.
  fn temp_dir() -> TempDir {
    TempDir::new("replay")
  }

  fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
  }

  /// Écrit à la main une fixture JSON pour `(méthode, url, corps)`.
  fn put(dir: &Path, method: &str, u: &str, body: &[u8], json: &str) {
    let path = fixture_path(dir, method, &url(u), body);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, json).unwrap();
  }

  fn replay(dir: &Path) -> Replay {
    Replay::new(dir).recording(false)
  }

  fn http(dir: &Path) -> Http {
    Http::service().with_replay(replay(dir))
  }

  const SEARCH: &str = "https://api.deezer.com/search?q=daft";
  const OK_JSON: &str = r#"{"method":"GET","url":"https://api.deezer.com/search?q=daft","status":200,
    "headers":{"content-type":"application/json"},"body":"{\"data\":[1,2]}"}"#;

  #[tokio::test]
  async fn a_matching_fixture_is_served() {
    let dir = temp_dir();
    put(&dir, "GET", SEARCH, b"", OK_JSON);
    let response = http(&dir).get(SEARCH).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.url().as_str(), SEARCH);
    let value: serde_json::Value = response.json().await.unwrap();
    assert_eq!(value["data"][1], 2);
  }

  #[tokio::test]
  async fn a_missing_fixture_fails_with_the_url_and_never_reaches_the_network() {
    let dir = temp_dir();
    let err = http(&dir)
      .get("https://api.deezer.com/search?q=missing")
      .send()
      .await
      .err()
      .unwrap();
    assert!(
      matches!(&err, Error::Replay(m) if m.contains("GET https://api.deezer.com/search?q=missing"))
    );
    assert!(!err.is_connect() && !err.is_request() && !err.is_timeout());
  }

  #[tokio::test]
  async fn the_request_body_is_part_of_the_key() {
    let dir = temp_dir();
    let post = "https://api.deezer.com/search";
    put(
      &dir,
      "POST",
      post,
      b"a=1",
      r#"{"method":"POST","url":"x","status":201,"body":"made"}"#,
    );
    let client = http(&dir);
    let hit = client.post(post).body("a=1").send().await.unwrap();
    assert_eq!(hit.status(), StatusCode::CREATED);
    assert_eq!(hit.text().await.unwrap(), "made");
    let other = client.post(post).body("a=2").send().await.err().unwrap();
    assert!(matches!(other, Error::Replay(_)));
    let get = client.get(post).send().await.err().unwrap();
    assert!(
      matches!(get, Error::Replay(_)),
      "la méthode fait partie de la clé"
    );
  }

  #[tokio::test]
  async fn tracking_parameters_fragment_and_order_do_not_change_the_key() {
    let dir = temp_dir();
    put(
      &dir,
      "GET",
      "https://api.deezer.com/s?a=1&b=2",
      b"",
      OK_JSON,
    );
    let client = http(&dir);
    for variant in [
      "https://api.deezer.com/s?b=2&a=1",
      "https://api.deezer.com/s?utm_source=x&a=1&fbclid=z&b=2#frag",
      "https://api.deezer.com/s?a=1&si=abc&b=2&UTM_MEDIUM=m&gclid=g",
    ] {
      assert!(client.get(variant).send().await.is_ok(), "{variant}");
    }
    assert!(
      client
        .get("https://api.deezer.com/s?a=1&b=3")
        .send()
        .await
        .is_err()
    );
  }

  #[test]
  fn normalization_drops_only_tracking_parameters() {
    let n = |s: &str| normalize_url(&url(s));
    assert_eq!(
      n("https://h.example/p?utm_x=1&q=a b&id=7"),
      "https://h.example/p?id=7&q=a+b"
    );
    assert_eq!(
      n("https://h.example/p?utm_x=1&fbclid=2"),
      "https://h.example/p"
    );
    assert_eq!(n("https://u:p@h.example/p#f"), "https://h.example/p");
  }

  #[test]
  fn recording_refuses_a_response_with_a_secret_and_writes_nothing() {
    let dir = temp_dir();
    let recorder = Replay::new(dir.to_path_buf()).recording(true);
    let u = url("https://api.deezer.com/auth");
    let id = Id::new("GET", &u, b"");
    let leaky = Fixture::capture(
      &id,
      200,
      BTreeMap::new(),
      &u,
      br#"{"access_token":"SENTINEL-TOKEN"}"#,
    );
    let err = recorder.write(&id, &leaky).unwrap_err();
    assert!(matches!(&err, Error::Replay(m) if m.contains("secret")));
    assert!(!err.to_string().contains("SENTINEL"));
    assert!(!recorder.path(&id).exists());
  }

  #[tokio::test]
  async fn a_recorded_response_is_redacted_filtered_and_replayable() {
    let dir = temp_dir();
    let recorder = Replay::new(dir.to_path_buf()).recording(true);
    let u = url("https://api.deezer.com/search?q=daft&api_key=SENTINEL-KEY&utm_source=x");
    let id = Id::new("GET", &u, b"");
    let wire = http::Response::builder()
      .status(200)
      .url(u.clone())
      .header("content-type", "application/json")
      .header("set-cookie", "sid=SENTINEL-COOKIE")
      .body(r#"{"data":[]}"#)
      .unwrap();
    let served = recorder
      .record_response(&id, reqwest::Response::from(wire), 1 << 20)
      .await
      .unwrap();
    assert_eq!(served.text().await.unwrap(), r#"{"data":[]}"#);
    let on_disk = std::fs::read_to_string(recorder.path(&id)).unwrap();
    assert!(!on_disk.contains("SENTINEL"), "{on_disk}");
    assert!(on_disk.contains("recorded_at") && on_disk.contains("application/json"));
    // Même requête, même clé : la fixture écrite est relue par le transport.
    let again = http(&dir)
      .get("https://api.deezer.com/search?q=daft&api_key=SENTINEL-KEY")
      .send()
      .await
      .unwrap();
    assert_eq!(again.text().await.unwrap(), r#"{"data":[]}"#);
  }

  #[tokio::test]
  async fn binary_bodies_round_trip_through_base64() {
    let dir = temp_dir();
    let u = url("https://i.ytimg.com/vi/x/default.jpg");
    let id = Id::new("GET", &u, b"");
    let fixture = Fixture::capture(&id, 200, BTreeMap::new(), &u, &[0xff, 0xd8, 0x00, 0x80]);
    assert!(fixture.body.is_none() && fixture.body_base64.is_some());
    Replay::new(dir.to_path_buf()).write(&id, &fixture).unwrap();
    let body = http(&dir)
      .get(u.as_str())
      .send()
      .await
      .unwrap()
      .bytes()
      .await
      .unwrap();
    assert_eq!(&body[..], &[0xff, 0xd8, 0x00, 0x80]);
  }

  #[tokio::test]
  async fn the_network_policy_still_applies_under_replay() {
    let dir = temp_dir();
    put(&dir, "GET", "https://evil.example/a", b"", OK_JSON);
    put(&dir, "GET", "http://127.0.0.1/a", b"", OK_JSON);
    let service = http(&dir);
    let err = service
      .get("https://evil.example/a")
      .send()
      .await
      .err()
      .unwrap();
    assert!(matches!(err, Error::Blocked(_)));
    let err = service
      .get("http://127.0.0.1/a")
      .send()
      .await
      .err()
      .unwrap();
    assert!(matches!(err, Error::Blocked(_)));
    let image = Http::public_image().with_replay(replay(&dir));
    let err = image.get("http://127.0.0.1/a").send().await.err().unwrap();
    assert!(matches!(err, Error::Blocked(_)));
    let local = Http::local().with_replay(replay(&dir));
    assert!(local.get("http://127.0.0.1/a").send().await.is_ok());
    let err = local
      .get("https://evil.example/a")
      .send()
      .await
      .err()
      .unwrap();
    assert!(matches!(err, Error::Blocked(_)));
  }

  fn capture_body(body: &str) -> (Id, Fixture) {
    let u = url("https://api.deezer.com/x");
    let id = Id::new("GET", &u, b"");
    let fixture = Fixture::capture(&id, 200, BTreeMap::new(), &u, body.as_bytes());
    (id, fixture)
  }

  #[test]
  fn a_numeric_code_field_is_recorded_but_a_secret_code_is_not() {
    let dir = temp_dir();
    let recorder = Replay::new(dir.to_path_buf()).recording(true);
    for ok in [
      r#"{"code":200,"data":[]}"#,
      r#"{"error":{"code":404,"message":"not found"}}"#,
      r#"{"data":[1],"code": -1 }"#,
      r#"[{"code":3}]"#,
    ] {
      let (id, fixture) = capture_body(ok);
      assert!(recorder.write(&id, &fixture).is_ok(), "{ok}");
      std::fs::remove_file(recorder.path(&id)).unwrap();
    }
    for refused in [
      r#"{"code":"abc123"}"#,
      r#"{"code":"200"}"#,
      r#"{"code":200,"access_token":"SENTINEL"}"#,
      r#"{"code":200,"access_token":12345}"#,
      r#"{"sk":5}"#,
      r#"{"refreshToken":"SENTINEL"}"#,
      "code=SENTINEL&x=1",
    ] {
      let (id, fixture) = capture_body(refused);
      let err = recorder.write(&id, &fixture).unwrap_err();
      assert!(
        matches!(&err, Error::Replay(m) if m.contains("secret")),
        "{refused}"
      );
      assert!(!recorder.path(&id).exists(), "{refused}");
    }
  }

  #[test]
  fn requests_differing_only_by_a_secret_parameter_have_distinct_keys() {
    let dir = Path::new("fixtures");
    let path = |u: &str| fixture_path(dir, "GET", &url(u), b"");
    for name in ["code", "token", "access_token", "refresh_token", "api_key"] {
      let a = path(&format!("https://api.deezer.com/p?{name}=SENTINEL-A&x=1"));
      let b = path(&format!("https://api.deezer.com/p?{name}=SENTINEL-B&x=1"));
      assert_ne!(a, b, "{name}");
      assert_eq!(
        a,
        path(&format!("https://api.deezer.com/p?x=1&{name}=SENTINEL-A")),
        "{name} : l'ordre ne compte pas"
      );
      assert!(!a.to_string_lossy().contains("SENTINEL"));
    }
    // Un paramètre ordinaire garde la clé de toujours (seule la valeur secrète est mêlée au condensé).
    assert_ne!(
      path("https://api.deezer.com/p?q=a"),
      path("https://api.deezer.com/p?q=b")
    );
    assert_eq!(
      path("https://api.deezer.com/p?q=a&utm_source=1"),
      path("https://api.deezer.com/p?q=a")
    );
  }

  #[test]
  fn the_secret_value_never_reaches_the_fixture_file() {
    let u = url("https://api.deezer.com/p?code=SENTINEL-CODE&x=1");
    let id = Id::new("GET", &u, b"");
    let fixture = Fixture::capture(&id, 200, BTreeMap::new(), &u, b"{}");
    assert!(!fixture.url.contains("SENTINEL"), "{}", fixture.url);
    let dir = temp_dir();
    let recorder = Replay::new(dir.to_path_buf()).recording(true);
    let path = recorder.write(&id, &fixture).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("SENTINEL"));
    assert!(!path.to_string_lossy().contains("SENTINEL"));
  }

  #[tokio::test]
  async fn a_secret_parameter_selects_its_own_fixture_on_replay() {
    let dir = temp_dir();
    put(&dir, "GET", "https://api.deezer.com/p?code=A", b"", OK_JSON);
    let client = http(&dir);
    assert!(
      client
        .get("https://api.deezer.com/p?code=A")
        .send()
        .await
        .is_ok()
    );
    let err = client
      .get("https://api.deezer.com/p?code=B")
      .send()
      .await
      .err()
      .unwrap();
    assert!(matches!(err, Error::Replay(_)));
  }

  /// Limite documentée : le rejeu ne suit pas les redirections. Il sert la réponse enregistrée telle quelle.
  #[tokio::test]
  async fn a_redirect_is_served_as_recorded_and_not_followed() {
    let dir = temp_dir();
    put(
      &dir,
      "GET",
      "https://api.deezer.com/old",
      b"",
      r#"{"method":"GET","url":"x","status":302,"headers":{"location":"https://api.deezer.com/new"}}"#,
    );
    let response = http(&dir)
      .get("https://api.deezer.com/old")
      .send()
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers()["location"], "https://api.deezer.com/new");
    assert_eq!(response.url().as_str(), "https://api.deezer.com/old");
  }

  #[tokio::test]
  async fn the_recorded_final_url_is_served_after_a_followed_redirect() {
    let dir = temp_dir();
    put(
      &dir,
      "GET",
      "https://api.deezer.com/old",
      b"",
      r#"{"method":"GET","url":"x","status":200,"final_url":"https://api.deezer.com/new","body":"ok"}"#,
    );
    let response = http(&dir)
      .get("https://api.deezer.com/old")
      .send()
      .await
      .unwrap();
    assert_eq!(response.url().as_str(), "https://api.deezer.com/new");
    assert_eq!(response.text().await.unwrap(), "ok");
  }

  #[tokio::test]
  async fn public_image_still_requires_an_image_type_under_replay() {
    let dir = temp_dir();
    put(&dir, "GET", "https://img.example/a.jpg", b"", OK_JSON);
    let image = Http::public_image().with_replay(replay(&dir));
    let err = image
      .get("https://img.example/a.jpg")
      .send()
      .await
      .err()
      .unwrap();
    assert!(matches!(err, Error::NotImage));
  }
}
