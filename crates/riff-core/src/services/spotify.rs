use super::*;
use crate::events::CoreEvent;
use crate::reply::Reply;
use crate::secrets::{SecretName, SecretStore};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use sha2::Sha256;
use std::net::Ipv4Addr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const CALLBACK_PATH: &str = "/api/spotify-callback";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(120);
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CALLBACK_HEAD: usize = 8 * 1024;
const MAX_CALLBACK_CONNECTIONS: usize = 32;
const SPOTIFY_SCOPES: &str = "user-library-read playlist-read-private playlist-read-collaborative";
const SPOTIFY_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Default, Clone)]
pub struct SpotifyAuth {
  verifier: String,
  state: String,
  client_id: String,
  pub(super) token: String,
  pub(super) refresh_token: String,
  expires_at: u64,
}

#[derive(Default)]
pub struct SpotifyShared {
  pub(super) auth: SpotifyAuth,
  error: String,
  loaded: bool,
  // Incrémenté à chaque nouvelle session (auth-start, logout) pour invalider les requêtes en vol.
  session: u64,
}

struct TokenReply {
  status: u16,
  data: Value,
}

pub struct ApiErr {
  msg: String,
  status: Option<u16>,
}

impl ApiErr {
  fn plain(msg: impl Into<String>) -> Self {
    Self {
      msg: msg.into(),
      status: None,
    }
  }
}

#[cfg(windows)]
fn dpapi_unprotect(data: &[u8]) -> Option<Vec<u8>> {
  use windows::Win32::Foundation::{HLOCAL, LocalFree};
  use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};
  let input = CRYPT_INTEGER_BLOB {
    cbData: data.len() as u32,
    pbData: data.as_ptr() as *mut u8,
  };
  let mut out = CRYPT_INTEGER_BLOB::default();
  unsafe {
    CryptUnprotectData(&input, None, None, None, None, 0, &mut out).ok()?;
    let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
    Some(bytes)
  }
}

#[cfg(not(windows))]
fn dpapi_unprotect(_data: &[u8]) -> Option<Vec<u8>> {
  None
}

// Ancien stockage de riff-rs (DPAPI), migré vers le coffre à la première lecture.
fn legacy_auth_path() -> PathBuf {
  paths::resolve_data_dir().join("spotify-auth.dpapi")
}

/// Connexion sauvegardée inutilisable.
#[derive(Debug, PartialEq, Eq)]
enum SavedAuthError {
  /// Fichier présent mais illisible : l'utilisateur doit se reconnecter.
  Unreadable,
  /// Lecture du coffre en échec et aucun ancien fichier pour la remplacer : la connexion existe peut-être.
  VaultUnavailable,
}

// Renvoie le JSON sauvegardé. L'ancien fichier n'est supprimé qu'après écriture et relecture réussies dans le coffre.
// Coffre indisponible : le fichier reste en place et sert pour cette session.
fn read_saved_auth(
  store: &dyn SecretStore,
  legacy: &Path,
) -> Result<Option<String>, SavedAuthError> {
  let (stored, vault_failed) = match store.get(SecretName::SpotifyAuth) {
    Ok(stored) => (stored, false),
    Err(_) => (None, true),
  };
  if stored.is_some() {
    let _ = fs::remove_file(legacy);
    return Ok(stored);
  }
  let raw = match fs::read(legacy) {
    Ok(raw) => raw,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
      return if vault_failed {
        Err(SavedAuthError::VaultUnavailable)
      } else {
        Ok(None)
      };
    }
    Err(_) => return Err(SavedAuthError::Unreadable),
  };
  let plain = dpapi_unprotect(&raw)
    .and_then(|p| String::from_utf8(p).ok())
    .ok_or(SavedAuthError::Unreadable)?;
  let migrated = store.set(SecretName::SpotifyAuth, &plain).is_ok()
    && store
      .get(SecretName::SpotifyAuth)
      .is_ok_and(|back| back.as_deref() == Some(plain.as_str()));
  if migrated {
    let _ = fs::remove_file(legacy);
  }
  Ok(Some(plain))
}

fn is_client_id(id: &str) -> bool {
  id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn lock(core: &Core) -> std::sync::MutexGuard<'_, SpotifyShared> {
  core.auth.spotify.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn load_auth(core: &Core, shared: &mut SpotifyShared) {
  if shared.loaded {
    return;
  }
  shared.loaded = true;
  let saved = read_saved_auth(core.auth.secrets.as_ref(), &legacy_auth_path())
    .map(|text| text.map(|t| serde_json::from_str::<Value>(&t)));
  match saved {
    Ok(None) => {}
    Ok(Some(Ok(saved))) => {
      let client_id = s_of(saved.get("clientId"));
      let refresh = s_of(saved.get("refreshToken"));
      if is_client_id(&client_id) && !refresh.is_empty() {
        shared.auth.client_id = client_id;
        shared.auth.refresh_token = refresh;
      }
    }
    Ok(Some(Err(_))) | Err(SavedAuthError::Unreadable) => {
      shared.error = "Stored Spotify login could not be read. Please connect again.".to_string();
    }
    Err(SavedAuthError::VaultUnavailable) => {
      shared.error =
        "Secure storage is unavailable. Spotify login could not be loaded.".to_string();
    }
  }
}

fn s_of(v: Option<&Value>) -> String {
  v.and_then(Value::as_str).unwrap_or("").to_string()
}

fn save_auth(core: &Core, auth: &SpotifyAuth) -> bool {
  let payload =
    json!({ "clientId": auth.client_id, "refreshToken": auth.refresh_token }).to_string();
  core
    .auth
    .secrets
    .set(SecretName::SpotifyAuth, &payload)
    .is_ok()
}

fn clear_stored_auth(core: &Core) -> Result<(), ()> {
  let vault = core
    .auth
    .secrets
    .delete(SecretName::SpotifyAuth)
    .map_err(|_| ());
  let legacy = match fs::remove_file(legacy_auth_path()) {
    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(()),
    _ => Ok(()),
  };
  vault.and(legacy)
}

fn redirect_uri(core: &Core) -> String {
  core.local_url(CALLBACK_PATH)
}

fn random_bytes<const N: usize>() -> Option<[u8; N]> {
  let mut buf = [0u8; N];
  getrandom::fill(&mut buf).ok()?;
  Some(buf)
}

/// Vérificateur PKCE, son défi S256 et le `state` OAuth (128 bits). `None` si l'OS ne fournit pas d'aléa.
pub(super) fn new_login_secrets() -> Option<(String, String, String)> {
  let verifier = URL_SAFE_NO_PAD.encode(random_bytes::<48>()?);
  let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
  let oauth_state: String = random_bytes::<16>()?
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect();
  Some((verifier, challenge, oauth_state))
}

fn authorize_url(client_id: &str, redirect: &str, oauth_state: &str, challenge: &str) -> String {
  format!(
    "https://accounts.spotify.com/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}&state={}&code_challenge_method=S256&code_challenge={}",
    encode_uri_component(client_id),
    encode_uri_component(SPOTIFY_SCOPES),
    encode_uri_component(redirect),
    encode_uri_component(oauth_state),
    encode_uri_component(challenge)
  )
}

fn html_response(status: u16, body: &str) -> CoreResult {
  Ok(Reply::Bytes {
    status,
    content_type: "text/html; charset=utf-8".to_string(),
    body: Bytes::from(format!(
      "<!DOCTYPE html><html><body><p>{body}</p></body></html>"
    )),
  })
}

async fn request_token(client: &Http, params: &[(&str, &str)]) -> Result<TokenReply, String> {
  let body = url::form_urlencoded::Serializer::new(String::new())
    .extend_pairs(params)
    .finish();
  let resp = client
    .post("https://accounts.spotify.com/api/token")
    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
    .body(body)
    .timeout(Duration::from_secs(15))
    .send()
    .await
    .map_err(|e| {
      if e.is_timeout() {
        "Token exchange timed out".to_string()
      } else {
        e.to_string()
      }
    })?;
  let status = resp.status().as_u16();
  let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
  Ok(TokenReply {
    status,
    data: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
  })
}

pub async fn auth_start(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let client_id = query.get("clientId").cloned().unwrap_or_default();
  if !is_client_id(&client_id) {
    return json_response(
      400,
      &json!({ "success": false, "error": "Invalid Client ID" }),
    );
  }
  let Some((verifier, challenge, oauth_state)) = new_login_secrets() else {
    return json_response(
      500,
      &json!({ "success": false, "error": "No secure random source" }),
    );
  };
  {
    let mut g = lock(core);
    load_auth(core, &mut g);
    g.loaded = true;
    g.session += 1;
    g.auth = SpotifyAuth {
      verifier,
      state: oauth_state.clone(),
      client_id: client_id.clone(),
      ..Default::default()
    };
    g.error.clear();
  }
  let redirect = redirect_uri(core);
  let url = authorize_url(&client_id, &redirect, &oauth_state, &challenge);
  json_response(
    200,
    &json!({ "success": true, "url": url, "redirectUri": redirect }),
  )
}

pub async fn callback(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let code = query.get("code").cloned().unwrap_or_default();
  let got_state = query.get("state").cloned().unwrap_or_default();
  let err_param = query.get("error").cloned().unwrap_or_default();
  let (session, client_id, verifier) = {
    let mut g = lock(core);
    if g.auth.state.is_empty() || got_state.is_empty() || got_state != g.auth.state {
      return html_response(400, "Invalid Spotify login session.");
    }
    if !err_param.is_empty() || code.is_empty() {
      g.error = if err_param == "access_denied" {
        "Spotify authorization was denied.".to_string()
      } else {
        "Spotify login failed. Check the Client ID, allowed account and redirect URI.".to_string()
      };
      g.auth.state.clear();
      return html_response(400, "Spotify login failed. Return to Riff for details.");
    }
    (g.session, g.auth.client_id.clone(), g.auth.verifier.clone())
  };
  let redirect = redirect_uri(core);
  let result = request_token(
    &core.net.http_client,
    &[
      ("grant_type", "authorization_code"),
      ("code", &code),
      ("redirect_uri", &redirect),
      ("client_id", &client_id),
      ("code_verifier", &verifier),
    ],
  )
  .await;
  let (status, message) = apply_token_result(core, session, result);
  html_response(status, message)
}

// Applique le résultat de l'échange du code. Renvoie le statut et le message à afficher dans le navigateur.
fn apply_token_result(
  core: &Core,
  session: u64,
  result: Result<TokenReply, String>,
) -> (u16, &'static str) {
  let mut g = lock(core);
  match result {
    Err(e) => {
      g.auth.state.clear();
      g.error = format!("Could not reach Spotify: {e}");
      (500, "Spotify login failed. Return to Riff for details.")
    }
    Ok(reply) => {
      if g.session != session {
        return (
          409,
          "Spotify login session changed. Return to Riff and try again.",
        );
      }
      g.auth.state.clear();
      let access = s_of(reply.data.get("access_token"));
      if (200..300).contains(&reply.status) && !access.is_empty() {
        g.auth.token = access;
        g.auth.refresh_token = s_of(reply.data.get("refresh_token"));
        let expires_in = reply
          .data
          .get("expires_in")
          .and_then(Value::as_f64)
          .filter(|e| *e > 0.0)
          .unwrap_or(3600.0);
        g.auth.expires_at = now_millis() + (expires_in * 1000.0) as u64;
        if g.auth.refresh_token.is_empty() {
          g.error = "Spotify did not provide a refresh token; reconnect when this session expires."
            .to_string();
        } else if !save_auth(core, &g.auth) {
          g.error = "Spotify login could not be saved securely; reconnect after restarting Riff."
            .to_string();
        }
        (200, "Riff is connected to Spotify. You can close this tab.")
      } else {
        let reason = ["error_description", "error"]
          .iter()
          .map(|k| s_of(reply.data.get(*k)))
          .find(|v| !v.is_empty())
          .unwrap_or_else(|| format!("HTTP {}", reply.status));
        g.error = format!("Spotify token exchange failed: {reason}");
        (400, "Spotify login failed. Return to Riff for details.")
      }
    }
  }
}

/// Connexion lancée par [`start_login`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
  /// Page d'autorisation à ouvrir dans le navigateur.
  pub url: String,
  /// Adresse de retour, avec le port choisi par l'OS. À enregistrer sans le port : `http://127.0.0.1/api/spotify-callback`.
  pub redirect_uri: String,
}

/// Échec du démarrage de [`start_login`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoginError {
  #[error("Invalid Client ID")]
  InvalidClientId,
  #[error("No secure random source")]
  NoRandom,
  #[error("Could not open the local login port: {0}")]
  Listener(String),
}

/// Lance la connexion Spotify sans serveur permanent : un écouteur `127.0.0.1` à port choisi par l'OS attend le retour,
/// à usage unique, 120 s au plus. La fin est annoncée par [`CoreEvent::SpotifyLogin`](crate::events::CoreEvent).
///
/// # Errors
/// [`LoginError`] si le Client ID est invalide, si l'aléa manque ou si le port ne s'ouvre pas.
pub async fn start_login(core: &Arc<Core>, client_id: &str) -> Result<LoginStart, LoginError> {
  if !is_client_id(client_id) {
    return Err(LoginError::InvalidClientId);
  }
  let (verifier, challenge, oauth_state) = new_login_secrets().ok_or(LoginError::NoRandom)?;
  let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
    .await
    .map_err(|e| LoginError::Listener(e.to_string()))?;
  let port = listener
    .local_addr()
    .map_err(|e| LoginError::Listener(e.to_string()))?
    .port();
  let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
  let session = {
    let mut g = lock(core);
    load_auth(core, &mut g);
    g.session += 1;
    g.auth = SpotifyAuth {
      client_id: client_id.to_string(),
      ..Default::default()
    };
    g.error.clear();
    g.session
  };
  let url = authorize_url(client_id, &redirect, &oauth_state, &challenge);
  let task_core = Arc::clone(core);
  let login = PendingLogin {
    session,
    port,
    client_id: client_id.to_string(),
    verifier,
    oauth_state,
    redirect: redirect.clone(),
  };
  tokio::spawn(async move {
    let exchange = async |code: String, login: &PendingLogin| {
      let params = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", login.redirect.as_str()),
        ("client_id", login.client_id.as_str()),
        ("code_verifier", login.verifier.as_str()),
      ];
      request_token(&task_core.net.http_client, &params).await
    };
    run_login(&task_core, listener, login, LOGIN_TIMEOUT, exchange).await;
  });
  Ok(LoginStart {
    url,
    redirect_uri: redirect,
  })
}

struct PendingLogin {
  session: u64,
  port: u16,
  client_id: String,
  verifier: String,
  oauth_state: String,
  redirect: String,
}

fn finish_login(core: &Core, session: u64, failure: Option<String>) {
  let mut g = lock(core);
  if g.session != session {
    return;
  }
  if let Some(message) = failure {
    g.error = message;
  }
  let (connected, error) = (!g.auth.token.is_empty(), g.error.clone());
  drop(g);
  core.publish(CoreEvent::SpotifyLogin { connected, error });
}

async fn run_login(
  core: &Core,
  listener: TcpListener,
  login: PendingLogin,
  timeout: Duration,
  exchange: impl AsyncFnOnce(String, &PendingLogin) -> Result<TokenReply, String>,
) {
  let Some(hit) = wait_for_callback(listener, login.port, &login.oauth_state, timeout).await else {
    finish_login(
      core,
      login.session,
      Some("Spotify login timed out. Try again.".to_string()),
    );
    return;
  };
  let CallbackHit {
    mut stream,
    callback,
  } = hit;
  if !callback.error.is_empty() || callback.code.is_empty() {
    let message = if callback.error == "access_denied" {
      "Spotify authorization was denied."
    } else {
      "Spotify login failed. Check the Client ID, allowed account and redirect URI."
    };
    write_http(
      &mut stream,
      400,
      "Spotify login failed. Return to Riff for details.",
    )
    .await;
    finish_login(core, login.session, Some(message.to_string()));
    return;
  }
  let result = exchange(callback.code, &login).await;
  let (status, message) = apply_token_result(core, login.session, result);
  write_http(&mut stream, status, message).await;
  finish_login(core, login.session, None);
}

pub(super) struct Callback {
  pub(super) code: String,
  pub(super) error: String,
}

pub(super) struct CallbackHit {
  pub(super) stream: TcpStream,
  pub(super) callback: Callback,
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
  if a.len() != b.len() {
    return false;
  }
  let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
  std::hint::black_box(diff) == 0
}

// Contrôle la requête reçue par l'écouteur. Err = statut et message de refus.
#[cfg(test)]
fn check_callback(
  head: &str,
  port: u16,
  oauth_state: &str,
) -> Result<Callback, (u16, &'static str)> {
  check_callback_at(
    head,
    CALLBACK_PATH,
    port,
    oauth_state,
    "Invalid Spotify login session.",
  )
}

fn check_callback_at(
  head: &str,
  callback_path: &str,
  port: u16,
  oauth_state: &str,
  bad_state: &'static str,
) -> Result<Callback, (u16, &'static str)> {
  let mut lines = head.split("\r\n");
  let mut request_line = lines.next().unwrap_or("").split(' ');
  let (method, target) = (
    request_line.next().unwrap_or(""),
    request_line.next().unwrap_or(""),
  );
  if method != "GET" {
    return Err((405, "Method not allowed"));
  }
  let (path, query) = target.split_once('?').unwrap_or((target, ""));
  if path != callback_path {
    return Err((404, "Not found"));
  }
  let hosts: Vec<&str> = lines
    .filter_map(|line| line.split_once(':'))
    .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
    .map(|(_, value)| value.trim())
    .collect();
  if hosts != [format!("127.0.0.1:{port}").as_str()] {
    return Err((403, "Forbidden"));
  }
  let param = |key: &str| {
    url::form_urlencoded::parse(query.as_bytes())
      .find(|(k, _)| k == key)
      .map(|(_, v)| v.into_owned())
      .unwrap_or_default()
  };
  if !constant_time_eq(param("state").as_bytes(), oauth_state.as_bytes()) {
    return Err((400, bad_state));
  }
  Ok(Callback {
    code: param("code"),
    error: param("error"),
  })
}

// Lit l'en-tête d'une requête. `None` : rien de complet avant le délai ou trop long.
async fn read_head(mut stream: TcpStream) -> (TcpStream, Option<String>) {
  let read = async {
    let mut head = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while head.len() < MAX_CALLBACK_HEAD {
      let n = stream.read(&mut chunk).await.ok()?;
      if n == 0 {
        return None;
      }
      head.extend_from_slice(&chunk[..n]);
      if head.windows(4).any(|w| w == b"\r\n\r\n") {
        return String::from_utf8(head).ok();
      }
    }
    None
  };
  let head = tokio::time::timeout(CALLBACK_READ_TIMEOUT, read)
    .await
    .ok()
    .flatten();
  (stream, head)
}

pub(super) async fn write_http(stream: &mut TcpStream, status: u16, message: &str) {
  let body = format!("<!DOCTYPE html><html><body><p>{message}</p></body></html>");
  let reply = format!(
    "HTTP/1.1 {status} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
    if status == 200 { "OK" } else { "Refused" },
    body.len(),
  );
  let _ = stream.write_all(reply.as_bytes()).await;
  let _ = stream.shutdown().await;
}

// Attend la première requête valide. Les requêtes refusées laissent l'écouteur ouvert : une page locale hostile ne
// peut pas faire échouer la connexion. L'écouteur se ferme au retour de cette fonction.
async fn wait_for_callback(
  listener: TcpListener,
  port: u16,
  oauth_state: &str,
  timeout: Duration,
) -> Option<CallbackHit> {
  wait_for_callback_at(
    listener,
    CALLBACK_PATH,
    port,
    oauth_state,
    "Invalid Spotify login session.",
    timeout,
  )
  .await
}

/// Comme pour Spotify, à une autre adresse de retour.
pub(super) async fn wait_for_callback_at(
  listener: TcpListener,
  callback_path: &str,
  port: u16,
  oauth_state: &str,
  bad_state: &'static str,
  timeout: Duration,
) -> Option<CallbackHit> {
  let deadline = tokio::time::sleep(timeout);
  tokio::pin!(deadline);
  let mut reads = tokio::task::JoinSet::new();
  loop {
    tokio::select! {
      () = &mut deadline => return None,
      accepted = listener.accept(), if reads.len() < MAX_CALLBACK_CONNECTIONS => {
        if let Ok((stream, _)) = accepted {
          reads.spawn(read_head(stream));
        }
      }
      Some(done) = reads.join_next(), if !reads.is_empty() => {
        let Ok((mut stream, head)) = done else { continue };
        let Some(head) = head else { continue };
        match check_callback_at(&head, callback_path, port, oauth_state, bad_state) {
          Ok(callback) => return Some(CallbackHit { stream, callback }),
          Err((status, message)) => {
            tokio::spawn(async move { write_http(&mut stream, status, message).await });
          }
        }
      }
    }
  }
}

async fn get_access_token(core: &Core, force: bool) -> Result<String, String> {
  let valid = |g: &SpotifyShared| {
    !g.auth.token.is_empty()
      && (g.auth.expires_at == 0 || now_millis() + 60_000 < g.auth.expires_at)
  };
  {
    let mut g = lock(core);
    load_auth(core, &mut g);
    if !force && valid(&g) {
      return Ok(g.auth.token.clone());
    }
    if g.auth.refresh_token.is_empty() {
      return Err("Spotify session expired. Connect your account again.".to_string());
    }
  }
  let _refreshing = core.auth.spotify_refresh.lock().await;
  let (session, refresh, client_id) = {
    let g = lock(core);
    if !force && valid(&g) {
      return Ok(g.auth.token.clone());
    }
    (
      g.session,
      g.auth.refresh_token.clone(),
      g.auth.client_id.clone(),
    )
  };
  if refresh.is_empty() {
    return Err("Spotify session expired. Connect your account again.".to_string());
  }
  let mut reply: Option<TokenReply> = None;
  for attempt in 0..3u64 {
    match request_token(
      &core.net.http_client,
      &[
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh),
        ("client_id", &client_id),
      ],
    )
    .await
    {
      Ok(r) => reply = Some(r),
      Err(e) => {
        reply = None;
        if attempt == 2 {
          return Err(e);
        }
      }
    }
    if let Some(r) = &reply
      && r.status != 429
      && r.status < 500
    {
      break;
    }
    if attempt < 2 {
      tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
    }
  }
  let reply = reply.ok_or_else(|| "Spotify token refresh failed. Try again later.".to_string())?;
  let mut g = lock(core);
  if g.session != session {
    return Err("Spotify session changed. Try again.".to_string());
  }
  if s_of(reply.data.get("error")) == "invalid_grant" {
    g.auth.token.clear();
    g.auth.refresh_token.clear();
    g.auth.expires_at = 0;
    let _ = clear_stored_auth(core);
    return Err("Spotify login expired. Connect your account again.".to_string());
  }
  let access = s_of(reply.data.get("access_token"));
  let expires_in = reply
    .data
    .get("expires_in")
    .and_then(Value::as_f64)
    .filter(|e| *e > 0.0);
  let Some(expires_in) = expires_in.filter(|_| reply.status == 200 && !access.is_empty()) else {
    return Err(format!(
      "Spotify token refresh failed (HTTP {}). Try again later.",
      reply.status
    ));
  };
  g.auth.token = access;
  g.auth.expires_at = now_millis() + (expires_in * 1000.0) as u64;
  g.error.clear();
  let new_refresh = s_of(reply.data.get("refresh_token"));
  if !new_refresh.is_empty() {
    g.auth.refresh_token = new_refresh;
    if !save_auth(core, &g.auth) {
      g.error =
        "Spotify login could not be saved securely; reconnect after restarting Riff.".to_string();
    }
  }
  Ok(g.auth.token.clone())
}

pub async fn status(core: &Core) -> CoreResult {
  let has_refresh = {
    let mut g = lock(core);
    load_auth(core, &mut g);
    !g.auth.refresh_token.is_empty()
  };
  if has_refresh && let Err(e) = get_access_token(core, false).await {
    lock(core).error = e;
  }
  let g = lock(core);
  let connected =
    !g.auth.token.is_empty() && (g.auth.expires_at == 0 || now_millis() < g.auth.expires_at);
  json_response(200, &json!({ "connected": connected, "error": g.error }))
}

pub async fn logout(core: &Core) -> CoreResult {
  if clear_stored_auth(core).is_err() {
    return json_response(
      500,
      &json!({ "success": false, "error": "Could not remove saved Spotify login" }),
    );
  }
  let mut g = lock(core);
  g.session += 1;
  g.auth = SpotifyAuth::default();
  g.error.clear();
  json_response(200, &json!({ "success": true }))
}

pub struct ApiReply {
  pub status: u16,
  retry_after: Option<String>,
  pub body: String,
}

async fn api_get(client: &Http, url: &str, token: &str) -> Result<ApiReply, ApiErr> {
  let parsed = Url::parse(url).map_err(|_| ApiErr::plain("Invalid Spotify API URL"))?;
  if parsed.scheme() != "https" || parsed.host_str() != Some("api.spotify.com") {
    return Err(ApiErr::plain("Invalid Spotify API URL"));
  }
  let resp = client
    .get(parsed)
    .header(header::AUTHORIZATION, format!("Bearer {token}"))
    .header(header::USER_AGENT, SPOTIFY_UA)
    .timeout(Duration::from_secs(15))
    .send()
    .await
    .map_err(|e| {
      ApiErr::plain(if e.is_timeout() {
        "Spotify API timed out".to_string()
      } else {
        e.to_string()
      })
    })?;
  let status = resp.status().as_u16();
  let retry_after = resp
    .headers()
    .get(header::RETRY_AFTER)
    .and_then(|v| v.to_str().ok())
    .map(str::to_string);
  let body = resp
    .text()
    .await
    .map_err(|e| ApiErr::plain(e.to_string()))?;
  Ok(ApiReply {
    status,
    retry_after,
    body,
  })
}

fn retry_after_seconds(reply: &ApiReply) -> f64 {
  reply
    .retry_after
    .as_deref()
    .and_then(|r| r.trim().parse::<f64>().ok())
    .filter(|s| s.is_finite() && *s >= 0.0)
    .unwrap_or(1.0)
}

pub async fn get_with_retry(core: &Core, url: &str) -> Result<ApiReply, ApiErr> {
  let client = &core.net.http_client;
  let mut token = get_access_token(core, false).await.map_err(ApiErr::plain)?;
  let mut reply = api_get(client, url, &token).await?;
  if reply.status == 401 && !lock(core).auth.refresh_token.is_empty() {
    token = get_access_token(core, true).await.map_err(ApiErr::plain)?;
    reply = api_get(client, url, &token).await?;
  }
  for _ in 0..3 {
    if reply.status != 429 {
      break;
    }
    let wait = retry_after_seconds(&reply);
    if wait > 30.0 {
      return Ok(reply);
    }
    tokio::time::sleep(Duration::from_secs_f64(wait)).await;
    reply = api_get(client, url, &token).await?;
  }
  Ok(reply)
}

fn api_failure(reply: &ApiReply, endpoint: &str) -> ApiErr {
  let detail = serde_json::from_str::<Value>(&reply.body)
    .ok()
    .and_then(|v| {
      let e = v.get("error")?;
      Some(
        e.as_str()
          .map(str::to_string)
          .unwrap_or_else(|| s_of(e.get("message"))),
      )
    })
    .unwrap_or_default();
  let msg = if reply.status == 429 {
    "Spotify rate limited the request. Try again later.".to_string()
  } else if detail.is_empty() {
    format!("Spotify {endpoint} returned HTTP {}", reply.status)
  } else {
    format!(
      "Spotify {endpoint} returned HTTP {}: {detail}",
      reply.status
    )
  };
  ApiErr {
    msg,
    status: Some(reply.status),
  }
}

fn is_ok(reply: &ApiReply) -> bool {
  (200..300).contains(&reply.status)
}

fn parse_items(reply: &ApiReply, what: &str) -> Result<Value, ApiErr> {
  let data: Value = serde_json::from_str(&reply.body).map_err(|e| ApiErr::plain(e.to_string()))?;
  if !data.get("items").is_some_and(Value::is_array) {
    return Err(ApiErr::plain(format!("Invalid Spotify {what} response")));
  }
  Ok(data)
}

fn collect_tracks(data: &Value, out: &mut Vec<Value>, cap: usize) {
  for it in data
    .get("items")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
  {
    if out.len() >= cap {
      break;
    }
    let t = it
      .get("item")
      .filter(|v| is_truthy(Some(v)))
      .or_else(|| it.get("track"));
    let Some(t) = t.filter(|t| t.is_object()) else {
      continue;
    };
    if is_truthy(t.get("is_local")) || t.get("type").and_then(Value::as_str) == Some("episode") {
      continue;
    }
    let artist = t
      .pointer("/artists/0/name")
      .and_then(Value::as_str)
      .unwrap_or("");
    let title = s_of(t.get("name"));
    if title.is_empty() {
      continue;
    }
    let mut entry = json!({ "title": title, "artist": artist });
    if let Some(ms) = t
      .get("duration_ms")
      .and_then(Value::as_f64)
      .filter(|m| *m != 0.0)
    {
      entry["duration"] = json!((ms / 1000.0).round() as i64);
    }
    out.push(entry);
  }
}

fn next_url(data: &Value) -> Option<String> {
  data
    .get("next")
    .and_then(Value::as_str)
    .filter(|n| !n.is_empty())
    .map(str::to_string)
}

async fn fetch_library(
  core: &Core,
  want_likes: bool,
  want_playlists: bool,
) -> Result<Value, ApiErr> {
  let mut likes: Vec<Value> = Vec::new();
  let mut playlists: Vec<Value> = Vec::new();
  let mut skipped: Vec<Value> = Vec::new();

  if want_likes {
    let mut next = Some("https://api.spotify.com/v1/me/tracks?limit=50".to_string());
    while let Some(url) = next {
      if likes.len() >= 5000 {
        break;
      }
      let reply = get_with_retry(core, &url).await?;
      if !is_ok(&reply) {
        return Err(api_failure(&reply, "saved tracks"));
      }
      let data = parse_items(&reply, "saved tracks")?;
      collect_tracks(&data, &mut likes, 5000);
      next = next_url(&data);
    }
  }

  if want_playlists {
    let mut user_playlists: Vec<(String, String)> = Vec::new();
    let mut next = Some("https://api.spotify.com/v1/me/playlists?limit=50".to_string());
    while let Some(url) = next {
      let reply = get_with_retry(core, &url).await?;
      if !is_ok(&reply) {
        return Err(api_failure(&reply, "playlists"));
      }
      let data = parse_items(&reply, "playlists")?;
      for pl in data["items"].as_array().into_iter().flatten() {
        let (id, name) = (s_of(pl.get("id")), s_of(pl.get("name")));
        if !id.is_empty() && !name.is_empty() {
          user_playlists.push((id, name));
        }
      }
      next = next_url(&data);
    }

    'playlists: for (id, name) in user_playlists {
      let mut tracks: Vec<Value> = Vec::new();
      let endpoint = format!("playlist items ({name})");
      let enc_id = encode_uri_component(&id);
      let mut reply = get_with_retry(
        core,
        &format!("https://api.spotify.com/v1/playlists/{enc_id}/items?limit=50"),
      )
      .await?;
      if reply.status == 404 || reply.status == 400 {
        reply = get_with_retry(
          core,
          &format!("https://api.spotify.com/v1/playlists/{enc_id}/tracks?limit=50"),
        )
        .await?;
      }
      if reply.status == 403 {
        skipped.push(json!({ "name": name, "error": api_failure(&reply, &endpoint).msg }));
        continue;
      }
      if !is_ok(&reply) {
        return Err(api_failure(&reply, &endpoint));
      }
      let mut data = parse_items(&reply, &endpoint)?;
      collect_tracks(&data, &mut tracks, usize::MAX);
      while let Some(url) = next_url(&data) {
        let reply = get_with_retry(core, &url).await?;
        if reply.status == 403 {
          skipped.push(json!({ "name": name, "error": api_failure(&reply, &endpoint).msg }));
          continue 'playlists;
        }
        if !is_ok(&reply) {
          return Err(api_failure(&reply, &endpoint));
        }
        data = parse_items(&reply, &endpoint)?;
        collect_tracks(&data, &mut tracks, usize::MAX);
      }
      playlists.push(json!({ "name": name, "tracks": tracks }));
    }
  }

  Ok(json!({
    "success": true,
    "partial": !skipped.is_empty(),
    "likes": likes,
    "playlists": playlists,
    "skippedPlaylists": skipped
  }))
}

pub async fn library(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  {
    let mut g = lock(core);
    load_auth(core, &mut g);
    if g.auth.token.is_empty() && g.auth.refresh_token.is_empty() {
      return json_response(401, &json!({ "success": false, "error": "Unauthorized" }));
    }
  }
  let flag = |k: &str| matches!(query.get(k).map(String::as_str), Some("1" | "true"));
  match fetch_library(core, flag("likes"), flag("playlists")).await {
    Ok(body) => json_response(200, &body),
    Err(e) => {
      let status = match e.status {
        Some(403) => 403,
        Some(429) => 429,
        _ => 500,
      };
      json_response(status, &json!({ "success": false, "error": e.msg }))
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::secrets::{MemoryStore, SecretError};
  use crate::test_temp::TempDir;
  use tokio::net::TcpStream as Client;

  const STATE: &str = "0123456789abcdef0123456789abcdef";

  /// Crée un fichier DPAPI comme le faisait l'ancienne version.
  #[cfg(windows)]
  fn dpapi_protect(data: &[u8]) -> Vec<u8> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData};
    use windows::core::w;
    let input = CRYPT_INTEGER_BLOB {
      cbData: data.len() as u32,
      pbData: data.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY : `input` pointe sur `data`, vivant pendant l'appel ; `out` est alloué par l'API et libéré ici.
    unsafe {
      CryptProtectData(&input, w!("riff-rs"), None, None, None, 0, &mut out).expect("protect");
      let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
      let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
      bytes
    }
  }

  fn temp_file(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new("spotify-test");
    let path = dir.join(name);
    (dir, path)
  }

  /// Coffre qui accepte tout mais ne relit jamais la bonne valeur, ou qui refuse d'écrire.
  #[derive(Debug)]
  enum BrokenStore {
    ReadsBackWrong,
    RefusesWrites,
  }

  impl SecretStore for BrokenStore {
    fn get(&self, _name: SecretName) -> Result<Option<String>, SecretError> {
      match self {
        Self::ReadsBackWrong => Ok(None),
        Self::RefusesWrites => Err(SecretError("no store".to_string())),
      }
    }
    fn set(&self, _name: SecretName, _value: &str) -> Result<(), SecretError> {
      match self {
        Self::ReadsBackWrong => Ok(()),
        Self::RefusesWrites => Err(SecretError("no store".to_string())),
      }
    }
    fn delete(&self, _name: SecretName) -> Result<(), SecretError> {
      Ok(())
    }
  }

  const SAVED: &str = r#"{"clientId":"x","refreshToken":"y"}"#;

  #[cfg(windows)]
  #[test]
  fn the_dpapi_file_is_migrated_then_removed() {
    let (_dir, path) = temp_file("migrate");
    fs::write(&path, dpapi_protect(SAVED.as_bytes())).unwrap();
    let store = MemoryStore::default();
    assert_eq!(
      read_saved_auth(&store, &path).ok().flatten().as_deref(),
      Some(SAVED)
    );
    assert_eq!(
      store.get(SecretName::SpotifyAuth),
      Ok(Some(SAVED.to_string()))
    );
    assert!(!path.exists(), "l'ancien fichier doit disparaître");
  }

  #[cfg(windows)]
  #[test]
  fn the_dpapi_file_stays_when_the_store_fails_or_reads_back_wrong() {
    for (name, store) in [
      ("refuse", BrokenStore::RefusesWrites),
      ("relit", BrokenStore::ReadsBackWrong),
    ] {
      let (_dir, path) = temp_file(name);
      fs::write(&path, dpapi_protect(SAVED.as_bytes())).unwrap();
      assert_eq!(
        read_saved_auth(&store, &path).ok().flatten().as_deref(),
        Some(SAVED),
        "{name}"
      );
      assert!(path.exists(), "{name} : le fichier doit rester");
    }
  }

  #[test]
  fn an_unreadable_dpapi_file_is_reported_and_kept() {
    let (_dir, path) = temp_file("illisible");
    fs::write(&path, b"pas du dpapi").unwrap();
    assert!(read_saved_auth(&MemoryStore::default(), &path).is_err());
    assert!(path.exists());
  }

  #[test]
  fn nothing_saved_reads_as_none() {
    let (_dir, absent) = temp_file("absent");
    assert!(matches!(
      read_saved_auth(&MemoryStore::default(), &absent),
      Ok(None)
    ));
    assert!(matches!(
      read_saved_auth(&BrokenStore::ReadsBackWrong, &absent),
      Ok(None)
    ));
  }

  #[test]
  fn a_failing_store_without_a_legacy_file_is_reported() {
    let (_dir, absent) = temp_file("absent-coffre");
    assert_eq!(
      read_saved_auth(&BrokenStore::RefusesWrites, &absent),
      Err(SavedAuthError::VaultUnavailable)
    );
  }

  #[test]
  fn a_failing_store_with_an_unreadable_legacy_file_reports_the_file() {
    let (_dir, path) = temp_file("coffre-et-illisible");
    fs::write(&path, b"pas du dpapi").unwrap();
    assert_eq!(
      read_saved_auth(&BrokenStore::RefusesWrites, &path),
      Err(SavedAuthError::Unreadable)
    );
    assert!(path.exists());
  }

  #[test]
  fn the_store_wins_and_a_leftover_file_is_removed() {
    let (_dir, path) = temp_file("reste");
    fs::write(&path, b"ancien").unwrap();
    let store = MemoryStore::default();
    store.set(SecretName::SpotifyAuth, SAVED).unwrap();
    assert_eq!(
      read_saved_auth(&store, &path).ok().flatten().as_deref(),
      Some(SAVED)
    );
    assert!(!path.exists());
  }

  #[test]
  fn constant_time_eq_compares_content_and_length() {
    assert!(constant_time_eq(b"abc", b"abc"));
    assert!(!constant_time_eq(b"abc", b"abd"));
    assert!(!constant_time_eq(b"abc", b"abcd"));
    assert!(!constant_time_eq(b"", b"a"));
  }

  fn request(method: &str, target: &str, host: &str) -> String {
    format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\n\r\n")
  }

  fn good_target() -> String {
    format!("{CALLBACK_PATH}?code=C&state={STATE}")
  }

  #[test]
  fn a_valid_callback_returns_the_code() {
    let cb = check_callback(
      &request("GET", &good_target(), "127.0.0.1:5000"),
      5000,
      STATE,
    )
    .ok()
    .unwrap();
    assert_eq!((cb.code.as_str(), cb.error.as_str()), ("C", ""));
    let denied = format!("{CALLBACK_PATH}?error=access_denied&state={STATE}");
    let cb = check_callback(&request("GET", &denied, "127.0.0.1:5000"), 5000, STATE)
      .ok()
      .unwrap();
    assert_eq!(cb.error, "access_denied");
  }

  #[test]
  fn invalid_callbacks_are_refused_with_the_right_status() {
    let refused = |head: String| {
      check_callback(&head, 5000, STATE)
        .err()
        .map(|(status, _)| status)
    };
    let target = good_target();
    assert_eq!(
      refused(request("POST", &target, "127.0.0.1:5000")),
      Some(405)
    );
    assert_eq!(
      refused(request("GET", "/autre?state=x", "127.0.0.1:5000")),
      Some(404)
    );
    assert_eq!(refused(request("GET", &target, "evil.example")), Some(403));
    assert_eq!(
      refused(request("GET", &target, "127.0.0.1:5001")),
      Some(403)
    );
    assert_eq!(
      refused(request("GET", &target, "localhost:5000")),
      Some(403)
    );
    assert_eq!(refused(format!("GET {target} HTTP/1.1\r\n\r\n")), Some(403));
    assert_eq!(
      refused(format!(
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:5000\r\nHost: evil.example\r\n\r\n"
      )),
      Some(403)
    );
    assert_eq!(
      refused(request(
        "GET",
        &format!("{CALLBACK_PATH}?code=C&state=mauvais"),
        "127.0.0.1:5000"
      )),
      Some(400)
    );
    assert_eq!(
      refused(request(
        "GET",
        &format!("{CALLBACK_PATH}?code=C"),
        "127.0.0.1:5000"
      )),
      Some(400)
    );
  }

  async fn listen() -> (TcpListener, u16) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
  }

  async fn send_raw(port: u16, raw: &str) -> String {
    let mut stream = Client::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut reply = String::new();
    // Borné : une régression doit faire échouer le test, pas le bloquer.
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_string(&mut reply)).await;
    reply
  }

  #[tokio::test]
  async fn the_listener_survives_refusals_and_closes_after_the_first_valid_request() {
    let (listener, port) = listen().await;
    let waiting =
      tokio::spawn(async move { wait_for_callback(listener, port, STATE, LOGIN_TIMEOUT).await });
    let host = format!("127.0.0.1:{port}");

    assert!(
      send_raw(
        port,
        &request(
          "GET",
          &format!("{CALLBACK_PATH}?code=C&state=mauvais"),
          &host
        )
      )
      .await
      .starts_with("HTTP/1.1 400")
    );
    assert!(
      send_raw(port, &request("GET", &good_target(), "evil.example"))
        .await
        .starts_with("HTTP/1.1 403")
    );
    assert!(!waiting.is_finished());

    let mut good = Client::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
    good
      .write_all(request("GET", &good_target(), &host).as_bytes())
      .await
      .unwrap();
    let mut hit = waiting.await.unwrap().expect("requête valide");
    assert_eq!(hit.callback.code, "C");
    write_http(&mut hit.stream, 200, "ok").await;

    // Deuxième requête : l'écouteur est fermé.
    assert!(Client::connect((Ipv4Addr::LOCALHOST, port)).await.is_err());
  }

  #[tokio::test]
  async fn an_idle_connection_does_not_block_the_valid_request() {
    let (listener, port) = listen().await;
    let waiting =
      tokio::spawn(async move { wait_for_callback(listener, port, STATE, LOGIN_TIMEOUT).await });
    let _idle = Client::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
    let mut good = Client::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
    good
      .write_all(request("GET", &good_target(), &format!("127.0.0.1:{port}")).as_bytes())
      .await
      .unwrap();
    assert!(waiting.await.unwrap().is_some());
  }

  #[tokio::test(start_paused = true)]
  async fn the_listener_gives_up_after_120_seconds_and_closes() {
    assert_eq!(LOGIN_TIMEOUT, Duration::from_secs(120));
    let (listener, port) = listen().await;
    let start = tokio::time::Instant::now();
    assert!(
      wait_for_callback(listener, port, STATE, LOGIN_TIMEOUT)
        .await
        .is_none()
    );
    assert_eq!(start.elapsed().as_secs(), 120);
    assert!(Client::connect((Ipv4Addr::LOCALHOST, port)).await.is_err());
  }
}
