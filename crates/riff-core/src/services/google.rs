//! Connexion Google et import de la bibliothèque YouTube : `google-oauth.js` et `youtube-data.js` de Riff.
//!
//! L'identifiant client n'est pas celui de Riff : il vient de `RIFF_GOOGLE_CLIENT_ID` ou de
//! `googleClientId` dans `config.json`. Sans lui, la connexion est désactivée.

use super::spotify::{LoginError, LoginStart, new_login_secrets, wait_for_callback_at, write_http};
use super::*;
use crate::events::CoreEvent;
use crate::secrets::SecretName;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::net::TcpListener;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API_BASE: &str = "https://www.googleapis.com/youtube/v3";
const SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";
const CALLBACK_PATH: &str = "/oauth2callback";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(120);
const PAGE_SIZE: u32 = 50;
const LIKES_CAP: u64 = 1000;
const REFRESH_MARGIN_MS: u64 = 60_000;
const DISABLED: &str = "Google sign-in is not configured in this build: no client ID.";
const VAULT_UNAVAILABLE: &str = "Secure storage is unavailable. Google login could not be loaded.";

/// Identifiant client configuré.
#[must_use]
pub fn client_id() -> Option<String> {
  let from_env = std::env::var("RIFF_GOOGLE_CLIENT_ID").ok();
  let from_config = config::read_config()
    .get("googleClientId")
    .and_then(Value::as_str)
    .map(str::to_string);
  from_env
    .or(from_config)
    .map(|id| id.trim().to_string())
    .filter(|id| !id.is_empty())
}

/// Page d'autorisation : PKCE S256, `youtube.readonly` seulement.
#[must_use]
pub fn authorization_url(
  client_id: &str,
  redirect_uri: &str,
  state: &str,
  challenge: &str,
) -> String {
  let query: String = url::form_urlencoded::Serializer::new(String::new())
    .append_pair("client_id", client_id)
    .append_pair("redirect_uri", redirect_uri)
    .append_pair("response_type", "code")
    .append_pair("scope", SCOPE)
    .append_pair("state", state)
    .append_pair("code_challenge", challenge)
    .append_pair("code_challenge_method", "S256")
    .append_pair("access_type", "offline")
    .append_pair("prompt", "select_account consent")
    .finish();
  format!("{AUTH_URL}?{query}")
}

/// Corps de l'échange du code : sans secret client, application de bureau.
#[must_use]
pub fn exchange_params(
  client_id: &str,
  code: &str,
  redirect_uri: &str,
  verifier: &str,
) -> Vec<(&'static str, String)> {
  vec![
    ("client_id", client_id.to_string()),
    ("code", code.to_string()),
    ("code_verifier", verifier.to_string()),
    ("grant_type", "authorization_code".to_string()),
    ("redirect_uri", redirect_uri.to_string()),
  ]
}

/// Corps du rafraîchissement du jeton.
#[must_use]
pub fn refresh_params(client_id: &str, refresh_token: &str) -> Vec<(&'static str, String)> {
  vec![
    ("client_id", client_id.to_string()),
    ("refresh_token", refresh_token.to_string()),
    ("grant_type", "refresh_token".to_string()),
  ]
}

/// Jetons enregistrés dans le coffre.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoogleAuth {
  pub access_token: String,
  pub refresh_token: String,
  pub expires_at: u64,
}

impl GoogleAuth {
  fn to_secret(&self) -> String {
    json!({ "accessToken": self.access_token, "refreshToken": self.refresh_token, "expiresAt": self.expires_at }).to_string()
  }

  fn from_secret(raw: &str) -> Option<Self> {
    let value: Value = serde_json::from_str(raw).ok()?;
    Some(Self {
      access_token: value["accessToken"].as_str()?.to_string(),
      refresh_token: value["refreshToken"].as_str().unwrap_or("").to_string(),
      expires_at: value["expiresAt"].as_u64().unwrap_or(0),
    })
  }

  /// Applique une réponse du point de jeton. Google ne renvoie pas toujours le jeton de rafraîchissement : l'ancien reste.
  fn updated_with(&self, reply: &Value, now_ms: u64) -> Result<Self, String> {
    let Some(access) = reply["access_token"]
      .as_str()
      .filter(|token| !token.is_empty())
    else {
      let description = reply["error_description"]
        .as_str()
        .or_else(|| reply["error"].as_str())
        .unwrap_or("no access token");
      return Err(format!("Google token request failed: {description}"));
    };
    let refresh = reply["refresh_token"]
      .as_str()
      .filter(|token| !token.is_empty())
      .unwrap_or(&self.refresh_token);
    Ok(Self {
      access_token: access.to_string(),
      refresh_token: refresh.to_string(),
      expires_at: now_ms + reply["expires_in"].as_u64().unwrap_or(3600) * 1000,
    })
  }
}

/// Lecture du coffre en échec : la connexion existe peut-être, ce n'est pas « pas connecté ».
struct VaultUnavailable;

/// Coffre vide ou donnée illisible : `Ok(None)`. Seul un `get` en échec donne `Err`.
fn read_auth(core: &Core) -> Result<Option<GoogleAuth>, VaultUnavailable> {
  let stored = core
    .auth
    .secrets
    .get(SecretName::GoogleAuth)
    .map_err(|_| VaultUnavailable)?;
  Ok(stored.and_then(|raw| GoogleAuth::from_secret(&raw)))
}

fn load_auth(core: &Core) -> Option<GoogleAuth> {
  read_auth(core).ok().flatten()
}

fn save_auth(core: &Core, auth: &GoogleAuth) -> Result<(), String> {
  core
    .auth
    .secrets
    .set(SecretName::GoogleAuth, &auth.to_secret())
    .map_err(|err| err.to_string())
}

async fn post_token(core: &Core, params: &[(&'static str, String)]) -> Result<Value, String> {
  let body: String = url::form_urlencoded::Serializer::new(String::new())
    .extend_pairs(params.iter().map(|(key, value)| (*key, value.as_str())))
    .finish();
  let response = core
    .net
    .http_client
    .post(TOKEN_URL)
    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
    .body(body)
    .timeout(Duration::from_secs(30))
    .send()
    .await
    .map_err(|_| "Could not reach Google".to_string())?;
  response
    .json::<Value>()
    .await
    .map_err(|_| "Invalid reply from Google".to_string())
}

fn finish_login(core: &Core, failure: Option<String>) {
  let connected = failure.is_none() && load_auth(core).is_some();
  core.publish(CoreEvent::GoogleLogin {
    connected,
    error: failure.unwrap_or_default(),
  });
}

/// Lance la connexion : écouteur `127.0.0.1` à port choisi par l'OS, mêmes refus que Spotify, 120 s au plus.
///
/// # Errors
/// [`LoginError`] si l'aléa manque ou si le port ne s'ouvre pas.
pub async fn start_login(core: &Arc<Core>, client_id: &str) -> Result<LoginStart, LoginError> {
  let (verifier, challenge, state) = new_login_secrets().ok_or(LoginError::NoRandom)?;
  let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
    .await
    .map_err(|err| LoginError::Listener(err.to_string()))?;
  let port = listener
    .local_addr()
    .map_err(|err| LoginError::Listener(err.to_string()))?
    .port();
  let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
  let url = authorization_url(client_id, &redirect, &state, &challenge);
  let task_core = Arc::clone(core);
  let (client, redirect_uri) = (client_id.to_string(), redirect.clone());
  tokio::spawn(async move {
    let Some(hit) = wait_for_callback_at(
      listener,
      CALLBACK_PATH,
      port,
      &state,
      "Invalid Google login session.",
      LOGIN_TIMEOUT,
    )
    .await
    else {
      finish_login(
        &task_core,
        Some("Google login timed out. Try again.".to_string()),
      );
      return;
    };
    let (mut stream, callback) = (hit.stream, hit.callback);
    if !callback.error.is_empty() || callback.code.is_empty() {
      write_http(
        &mut stream,
        400,
        "Google login failed. Return to Riff for details.",
      )
      .await;
      finish_login(&task_core, Some("Google login failed.".to_string()));
      return;
    }
    let result = match post_token(
      &task_core,
      &exchange_params(&client, &callback.code, &redirect_uri, &verifier),
    )
    .await
    {
      Ok(reply) => GoogleAuth::default()
        .updated_with(&reply, now_millis())
        .and_then(|auth| save_auth(&task_core, &auth)),
      Err(err) => Err(err),
    };
    match result {
      Ok(()) => {
        write_http(&mut stream, 200, "Connected. You can return to Riff.").await;
        finish_login(&task_core, None);
      }
      Err(err) => {
        write_http(
          &mut stream,
          500,
          "Google login failed. Return to Riff for details.",
        )
        .await;
        finish_login(&task_core, Some(err));
      }
    }
  });
  Ok(LoginStart {
    url,
    redirect_uri: redirect,
  })
}

async fn access_token(core: &Core) -> Result<String, String> {
  let Some(auth) = read_auth(core).map_err(|_| VAULT_UNAVAILABLE.to_string())? else {
    return Err("Sign in to Google before importing your YouTube library".to_string());
  };
  if now_millis() + REFRESH_MARGIN_MS < auth.expires_at {
    return Ok(auth.access_token);
  }
  let Some(client) = client_id() else {
    return Err(DISABLED.to_string());
  };
  let reply = post_token(core, &refresh_params(&client, &auth.refresh_token)).await?;
  let updated = auth.updated_with(&reply, now_millis())?;
  save_auth(core, &updated)?;
  Ok(updated.access_token)
}

/// `/api/google-status`.
pub async fn status(core: &Core) -> CoreResult {
  json_response(
    200,
    &status_body(
      client_id().is_some(),
      read_auth(core).map(|auth| auth.is_some()),
    ),
  )
}

/// Comme Spotify, `error` signale un coffre indisponible, mais seulement si Google est configuré : sans identifiant
/// client (build livré), le corps reste `{enabled, connected}`.
fn status_body(enabled: bool, connected: Result<bool, VaultUnavailable>) -> Value {
  let mut body = json!({ "enabled": enabled, "connected": connected.as_ref().is_ok_and(|c| *c) });
  if enabled && connected.is_err() {
    body["error"] = json!(VAULT_UNAVAILABLE);
  }
  body
}

/// `/api/google-logout`.
pub async fn logout(core: &Core) -> CoreResult {
  match core.auth.secrets.delete(SecretName::GoogleAuth) {
    Ok(()) => json_response(200, &json!({ "success": true })),
    Err(_) => json_response(
      500,
      &json!({ "success": false, "error": "Could not remove saved Google login" }),
    ),
  }
}

/// Lecture d'une page de l'API : adresse complète, réponse JSON ou message d'erreur.
trait YoutubeApi {
  async fn get(&self, url: String) -> Result<Value, String>;
}

struct RealApi<'a> {
  core: &'a Core,
  token: String,
}

impl YoutubeApi for RealApi<'_> {
  async fn get(&self, url: String) -> Result<Value, String> {
    let response = self
      .core
      .net
      .http_client
      .get(&url)
      .bearer_auth(&self.token)
      .header(header::ACCEPT, "application/json")
      .timeout(Duration::from_secs(30))
      .send()
      .await
      .map_err(|_| "Could not reach the YouTube API".to_string())?;
    let status = response.status();
    let data = response.json::<Value>().await.unwrap_or(Value::Null);
    if status.is_success() {
      Ok(data)
    } else {
      Err(data["error"]["message"].as_str().map_or_else(
        || format!("YouTube API request failed: HTTP {}", status.as_u16()),
        str::to_string,
      ))
    }
  }
}

fn thumbnail_url(thumbnails: &Value) -> String {
  ["maxres", "standard", "high", "medium", "default"]
    .iter()
    .find_map(|key| {
      thumbnails[*key]["url"]
        .as_str()
        .filter(|url| !url.is_empty())
    })
    .unwrap_or("")
    .to_string()
}

/// `durationInSeconds` : durée ISO 8601 `PT1H2M3S`. Autre forme : 0.
#[must_use]
pub fn duration_in_seconds(value: &str) -> u64 {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| Regex::new(r"^PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?$").expect("regex"));
  let Some(found) = re.captures(value) else {
    return 0;
  };
  let part = |index: usize| {
    found
      .get(index)
      .and_then(|part| part.as_str().parse::<u64>().ok())
      .unwrap_or(0)
  };
  part(1) * 3600 + part(2) * 60 + part(3)
}

fn page_url(path: &str, params: &[(&str, &str)], page_token: &str) -> String {
  let mut query = url::form_urlencoded::Serializer::new(String::new());
  for (key, value) in params {
    query.append_pair(key, value);
  }
  if !page_token.is_empty() {
    query.append_pair("pageToken", page_token);
  }
  format!("{API_BASE}/{path}?{}", query.finish())
}

/// `listAll` : toutes les pages. Un jeton de page répété est une erreur, pas une boucle sans fin.
async fn list_all(
  api: &impl YoutubeApi,
  path: &str,
  params: &[(&str, &str)],
) -> Result<(Vec<Value>, u64), String> {
  let (mut items, mut seen, mut page_token, mut total) =
    (Vec::new(), HashSet::new(), String::new(), 0);
  loop {
    let page = api.get(page_url(path, params, &page_token)).await?;
    if let Some(found) = page["items"].as_array() {
      items.extend(found.iter().cloned());
    }
    total = total.max(page["pageInfo"]["totalResults"].as_u64().unwrap_or(0));
    page_token = page["nextPageToken"].as_str().unwrap_or("").to_string();
    if page_token.is_empty() {
      return Ok((items, total));
    }
    if !seen.insert(page_token.clone()) {
      return Err("YouTube API returned a repeated page token".to_string());
    }
  }
}

fn track_json(
  id: &str,
  title: &str,
  artist: &str,
  duration: u64,
  platform: &str,
  cover: &str,
) -> Value {
  json!({
    "title": if title.is_empty() { "untitled" } else { title },
    "artist": artist,
    "duration": duration,
    "id": id,
    "url": format!("https://www.youtube.com/watch?v={}", encode_uri_component(id)),
    "platform": platform,
    "thumbnail": cover,
    "cover": cover,
  })
}

fn map_video(video: &Value, platform: &str) -> Option<Value> {
  let id = video["id"].as_str().filter(|id| !id.is_empty())?;
  let snippet = video.get("snippet")?;
  let duration = duration_in_seconds(video["contentDetails"]["duration"].as_str().unwrap_or(""));
  Some(track_json(
    id,
    snippet["title"].as_str().unwrap_or(""),
    snippet["channelTitle"].as_str().unwrap_or(""),
    duration,
    platform,
    &thumbnail_url(&snippet["thumbnails"]),
  ))
}

fn map_item(item: &Value, platform: &str) -> Option<Value> {
  let snippet = item.get("snippet")?;
  let id = item["contentDetails"]["videoId"]
    .as_str()
    .or_else(|| snippet["resourceId"]["videoId"].as_str())
    .filter(|id| !id.is_empty())?;
  let artist = snippet["videoOwnerChannelTitle"]
    .as_str()
    .filter(|name| !name.is_empty())
    .or_else(|| snippet["channelTitle"].as_str())
    .unwrap_or("");
  Some(track_json(
    id,
    snippet["title"].as_str().unwrap_or(""),
    artist,
    0,
    platform,
    &thumbnail_url(&snippet["thumbnails"]),
  ))
}

/// `addDurations` : par lots de 50 identifiants distincts.
async fn add_durations(api: &impl YoutubeApi, playlists: &mut [Value]) -> Result<(), String> {
  let mut ids: Vec<String> = Vec::new();
  for track in playlists
    .iter()
    .flat_map(|playlist| playlist["tracks"].as_array().into_iter().flatten())
  {
    if let Some(id) = track["id"].as_str()
      && !ids.iter().any(|known| known == id)
    {
      ids.push(id.to_string());
    }
  }
  let mut durations: HashMap<String, u64> = HashMap::new();
  for batch in ids.chunks(PAGE_SIZE as usize) {
    let joined = batch.join(",");
    let page = api
      .get(page_url(
        "videos",
        &[("part", "contentDetails"), ("id", joined.as_str())],
        "",
      ))
      .await?;
    for video in page["items"].as_array().into_iter().flatten() {
      if let Some(id) = video["id"].as_str() {
        durations.insert(
          id.to_string(),
          duration_in_seconds(video["contentDetails"]["duration"].as_str().unwrap_or("")),
        );
      }
    }
  }
  for playlist in playlists {
    for track in playlist["tracks"].as_array_mut().into_iter().flatten() {
      let duration = track["id"]
        .as_str()
        .and_then(|id| durations.get(id))
        .copied()
        .unwrap_or(0);
      track["duration"] = json!(duration);
    }
  }
  Ok(())
}

/// Ce qu'il faut importer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryOptions {
  /// `youtube` ou `ytmusic`.
  pub platform: String,
  pub likes: bool,
  pub playlists: bool,
}

/// `fetchYouTubeLibrary`. Erreur : message de l'API, ou jeton de page répété.
async fn fetch_library(api: &impl YoutubeApi, options: &LibraryOptions) -> Result<Value, String> {
  let platform = if options.platform == "ytmusic" {
    "ytmusic"
  } else {
    "youtube"
  };
  let page_size = PAGE_SIZE.to_string();
  let (liked, liked_total) = if options.likes {
    list_all(
      api,
      "videos",
      &[
        ("part", "snippet,contentDetails"),
        ("myRating", "like"),
        ("maxResults", page_size.as_str()),
      ],
    )
    .await?
  } else {
    (Vec::new(), 0)
  };
  let likes: Vec<Value> = liked
    .iter()
    .filter_map(|video| map_video(video, platform))
    .collect();
  let own = if options.playlists {
    list_all(
      api,
      "playlists",
      &[
        ("part", "snippet,contentDetails"),
        ("mine", "true"),
        ("maxResults", page_size.as_str()),
      ],
    )
    .await?
    .0
  } else {
    Vec::new()
  };
  let mut playlists = Vec::with_capacity(own.len());
  for item in &own {
    let (Some(id), Some(snippet)) = (
      item["id"].as_str().filter(|id| !id.is_empty()),
      item.get("snippet"),
    ) else {
      continue;
    };
    let (tracks, _) = list_all(
      api,
      "playlistItems",
      &[
        ("part", "snippet,contentDetails"),
        ("playlistId", id),
        ("maxResults", page_size.as_str()),
      ],
    )
    .await?;
    let tracks: Vec<Value> = tracks
      .iter()
      .filter_map(|track| map_item(track, platform))
      .collect();
    let name = snippet["title"]
      .as_str()
      .filter(|name| !name.is_empty())
      .unwrap_or("Playlist");
    playlists.push(json!({ "id": id, "name": name, "cover": thumbnail_url(&snippet["thumbnails"]), "tracks": tracks }));
  }
  add_durations(api, &mut playlists).await?;
  let warning = if liked_total > LIKES_CAP {
    "YouTube API only provides the most recent 1,000 liked videos."
  } else {
    ""
  };
  Ok(json!({ "success": true, "likes": likes, "playlists": playlists, "warning": warning }))
}

/// `/api/google-library?platform=&likes=&playlists=`.
pub async fn library(core: &Core, params: Params) -> CoreResult {
  let flag = |key: &str| {
    !matches!(
      params.query.get(key).map(String::as_str),
      Some("0" | "false")
    )
  };
  let options = LibraryOptions {
    platform: params.query.get("platform").cloned().unwrap_or_default(),
    likes: flag("likes"),
    playlists: flag("playlists"),
  };
  let result = match access_token(core).await {
    Ok(token) => fetch_library(&RealApi { core, token }, &options).await,
    Err(err) => Err(err),
  };
  match result {
    Ok(body) => json_response(200, &body),
    Err(error) => {
      logging::log_entry("ERROR", &format!("Google library: {error}"));
      json_response(500, &json!({ "success": false, "error": error }))
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Mutex;

  /// API simulée : chaque adresse lue est notée, la réponse vient de la fonction donnée.
  struct FakeApi<F> {
    answer: F,
    urls: Mutex<Vec<String>>,
  }

  impl<F: Fn(&str) -> Result<Value, String>> YoutubeApi for FakeApi<F> {
    async fn get(&self, url: String) -> Result<Value, String> {
      self.urls.lock().unwrap().push(url.clone());
      (self.answer)(&url)
    }
  }

  fn fake<F: Fn(&str) -> Result<Value, String>>(answer: F) -> FakeApi<F> {
    FakeApi {
      answer,
      urls: Mutex::new(Vec::new()),
    }
  }

  fn options() -> LibraryOptions {
    LibraryOptions {
      platform: "youtube".to_string(),
      likes: true,
      playlists: true,
    }
  }

  #[test]
  fn status_reports_an_unavailable_vault_only_when_google_is_configured() {
    let message = "Secure storage is unavailable. Google login could not be loaded.";
    assert_eq!(
      status_body(true, Err(VaultUnavailable)),
      json!({ "enabled": true, "connected": false, "error": message })
    );
    assert_eq!(
      status_body(false, Err(VaultUnavailable)),
      json!({ "enabled": false, "connected": false })
    );
  }

  #[test]
  fn status_has_no_error_for_an_empty_or_filled_vault() {
    assert_eq!(
      status_body(true, Ok(false)),
      json!({ "enabled": true, "connected": false })
    );
    assert_eq!(
      status_body(true, Ok(true)),
      json!({ "enabled": true, "connected": true })
    );
  }

  fn param(url: &str, key: &str) -> Option<String> {
    url::Url::parse(url)
      .unwrap()
      .query_pairs()
      .find(|(name, _)| name == key)
      .map(|(_, value)| value.into_owned())
  }

  #[test]
  fn the_authorization_request_uses_pkce_and_the_readonly_scope_only() {
    let url = authorization_url(
      "mon-id",
      "http://127.0.0.1:49152/oauth2callback",
      "etat",
      "defi",
    );
    assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    assert_eq!(param(&url, "client_id").as_deref(), Some("mon-id"));
    assert_eq!(
      param(&url, "redirect_uri").as_deref(),
      Some("http://127.0.0.1:49152/oauth2callback")
    );
    assert_eq!(param(&url, "response_type").as_deref(), Some("code"));
    assert_eq!(
      param(&url, "code_challenge_method").as_deref(),
      Some("S256")
    );
    assert_eq!(param(&url, "code_challenge").as_deref(), Some("defi"));
    assert_eq!(param(&url, "state").as_deref(), Some("etat"));
    assert_eq!(param(&url, "access_type").as_deref(), Some("offline"));
    assert_eq!(
      param(&url, "prompt").as_deref(),
      Some("select_account consent")
    );
    assert_eq!(
      param(&url, "scope").as_deref(),
      Some("https://www.googleapis.com/auth/youtube.readonly")
    );
  }

  #[test]
  fn tokens_are_exchanged_and_refreshed_without_a_client_secret() {
    let exchange = exchange_params("id", "code", "http://127.0.0.1:1/oauth2callback", "verif");
    let get = |list: &[(&str, String)], key: &str| {
      list
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.clone())
    };
    assert_eq!(
      get(&exchange, "grant_type").as_deref(),
      Some("authorization_code")
    );
    assert_eq!(get(&exchange, "code").as_deref(), Some("code"));
    assert_eq!(get(&exchange, "code_verifier").as_deref(), Some("verif"));
    assert_eq!(get(&exchange, "client_id").as_deref(), Some("id"));
    assert!(get(&exchange, "client_secret").is_none());
    let refresh = refresh_params("id", "jeton");
    assert_eq!(
      get(&refresh, "grant_type").as_deref(),
      Some("refresh_token")
    );
    assert_eq!(get(&refresh, "refresh_token").as_deref(), Some("jeton"));
    assert!(get(&refresh, "client_secret").is_none());
  }

  #[test]
  fn a_token_reply_keeps_the_old_refresh_token_when_none_comes_back() {
    let before = GoogleAuth {
      access_token: "a".into(),
      refresh_token: "r".into(),
      expires_at: 0,
    };
    let after = before
      .updated_with(&json!({ "access_token": "b", "expires_in": 10 }), 1000)
      .unwrap();
    assert_eq!(
      after,
      GoogleAuth {
        access_token: "b".into(),
        refresh_token: "r".into(),
        expires_at: 11_000
      }
    );
    let replaced = before
      .updated_with(&json!({ "access_token": "b", "refresh_token": "s" }), 0)
      .unwrap();
    assert_eq!(replaced.refresh_token, "s");
  }

  #[test]
  fn a_refused_token_gives_the_description_of_google() {
    let error = GoogleAuth::default()
      .updated_with(
        &json!({ "error": "invalid_grant", "error_description": "expiré" }),
        0,
      )
      .unwrap_err();
    assert_eq!(error, "Google token request failed: expiré");
  }

  #[test]
  fn the_secret_round_trips() {
    let auth = GoogleAuth {
      access_token: "a".into(),
      refresh_token: "r".into(),
      expires_at: 5,
    };
    assert_eq!(GoogleAuth::from_secret(&auth.to_secret()), Some(auth));
    assert_eq!(GoogleAuth::from_secret("pas du json"), None);
  }

  #[test]
  fn the_logs_never_contain_a_token() {
    let auth = GoogleAuth {
      access_token: "ACCES".into(),
      refresh_token: "RAFRAICHIR".into(),
      expires_at: 5,
    };
    let masked = logging::redact(&format!(
      r#"{{"accessToken":"ACCES","refreshToken":"RAFRAICHIR"}} {}"#,
      auth.to_secret()
    ));
    assert!(!masked.contains("ACCES"), "{masked}");
    assert!(!masked.contains("RAFRAICHIR"), "{masked}");
    let body = logging::redact("grant_type=refresh_token&refresh_token=RAFRAICHIR&code=CODE");
    assert!(!body.contains("RAFRAICHIR"), "{body}");
    assert!(!body.contains("CODE"), "{body}");
  }

  #[test]
  fn durations_are_read_like_riff() {
    assert_eq!(duration_in_seconds("PT3M14S"), 194);
    assert_eq!(duration_in_seconds("PT1H2M3S"), 3723);
    assert_eq!(duration_in_seconds("PT45S"), 45);
    assert_eq!(duration_in_seconds("P1D"), 0);
    assert_eq!(duration_in_seconds(""), 0);
  }

  fn library_answer(url: &str) -> Result<Value, String> {
    if url.contains("/videos?") && url.contains("myRating=like") {
      if url.contains("pageToken=page2") {
        return Ok(
          json!({ "items": [{ "id": "video-2", "contentDetails": { "duration": "PT1H2M3S" }, "snippet": { "title": "Second", "channelTitle": "Artist B", "thumbnails": { "high": { "url": "https://img/2" } } } }] }),
        );
      }
      return Ok(json!({
        "items": [{ "id": "video-1", "contentDetails": { "duration": "PT3M14S" }, "snippet": { "title": "First", "channelTitle": "Artist A", "thumbnails": { "medium": { "url": "https://img/1" } } } }],
        "nextPageToken": "page2"
      }));
    }
    if url.contains("/playlists?") {
      return Ok(
        json!({ "items": [{ "id": "PL_ONE", "snippet": { "title": "My playlist", "thumbnails": { "high": { "url": "https://img/playlist" } } } }] }),
      );
    }
    if url.contains("/playlistItems?") {
      return Ok(
        json!({ "items": [{ "contentDetails": { "videoId": "video-3" }, "snippet": { "title": "Playlist track", "channelTitle": "Artist C" } }] }),
      );
    }
    if url.contains("/videos?") {
      return Ok(
        json!({ "items": [{ "id": "video-3", "contentDetails": { "duration": "PT45S" } }] }),
      );
    }
    Err(format!("requête inattendue : {url}"))
  }

  #[tokio::test]
  async fn the_library_is_paginated_and_gets_its_durations() {
    let api = fake(library_answer);
    let result = fetch_library(&api, &options()).await.unwrap();
    let likes = result["likes"].as_array().unwrap();
    let summary: Vec<(String, String, u64)> = likes
      .iter()
      .map(|track| {
        (
          track["title"].as_str().unwrap().to_string(),
          track["artist"].as_str().unwrap().to_string(),
          track["duration"].as_u64().unwrap(),
        )
      })
      .collect();
    assert_eq!(
      summary,
      [
        ("First".to_string(), "Artist A".to_string(), 194),
        ("Second".to_string(), "Artist B".to_string(), 3723)
      ]
    );
    assert_eq!(likes[0]["cover"], "https://img/1");
    assert_eq!(likes[0]["url"], "https://www.youtube.com/watch?v=video-1");
    assert_eq!(likes[0]["platform"], "youtube");
    let playlist = &result["playlists"][0];
    assert_eq!(playlist["name"], "My playlist");
    assert_eq!(playlist["cover"], "https://img/playlist");
    assert_eq!(playlist["tracks"][0]["duration"], 45);
    assert_eq!(result["warning"], "");
    assert!(
      api
        .urls
        .lock()
        .unwrap()
        .iter()
        .any(|url| param(url, "pageToken").as_deref() == Some("page2"))
    );
    assert!(
      api
        .urls
        .lock()
        .unwrap()
        .iter()
        .all(|url| url.starts_with("https://www.googleapis.com/youtube/v3/"))
    );
  }

  #[tokio::test]
  async fn ytmusic_is_kept_on_each_track() {
    let result = fetch_library(
      &fake(library_answer),
      &LibraryOptions {
        platform: "ytmusic".to_string(),
        ..options()
      },
    )
    .await
    .unwrap();
    assert_eq!(result["likes"][0]["platform"], "ytmusic");
    assert_eq!(result["playlists"][0]["tracks"][0]["platform"], "ytmusic");
  }

  #[tokio::test]
  async fn an_unknown_platform_is_youtube() {
    let result = fetch_library(
      &fake(library_answer),
      &LibraryOptions {
        platform: "autre".to_string(),
        ..options()
      },
    )
    .await
    .unwrap();
    assert_eq!(result["likes"][0]["platform"], "youtube");
  }

  #[tokio::test]
  async fn a_repeated_page_token_stops_with_an_error() {
    let api = fake(|_: &str| Ok(json!({ "items": [], "nextPageToken": "same" })));
    let error = fetch_library(&api, &options()).await.unwrap_err();
    assert_eq!(error, "YouTube API returned a repeated page token");
    assert_eq!(api.urls.lock().unwrap().len(), 2, "pas de boucle sans fin");
  }

  #[tokio::test]
  async fn more_than_1000_likes_gives_the_warning() {
    let api = fake(|url: &str| {
      if url.contains("myRating") {
        Ok(json!({ "items": [], "pageInfo": { "totalResults": 1200 } }))
      } else {
        Ok(json!({ "items": [] }))
      }
    });
    assert_eq!(
      fetch_library(&api, &options()).await.unwrap()["warning"],
      "YouTube API only provides the most recent 1,000 liked videos."
    );
    let api = fake(|_: &str| Ok(json!({ "items": [], "pageInfo": { "totalResults": 1000 } })));
    assert_eq!(
      fetch_library(&api, &options()).await.unwrap()["warning"],
      ""
    );
  }

  #[tokio::test]
  async fn durations_are_asked_by_batches_of_50_distinct_ids() {
    let api = fake(|url: &str| {
      if url.contains("/playlists?") {
        return Ok(json!({ "items": [{ "id": "PL", "snippet": { "title": "P" } }] }));
      }
      if url.contains("/playlistItems?") {
        let items: Vec<Value> = (0..120).map(|index| json!({ "contentDetails": { "videoId": format!("v{}", index % 110) }, "snippet": { "title": "t" } })).collect();
        return Ok(json!({ "items": items }));
      }
      if url.contains("part=contentDetails&id=") {
        let ids = param(url, "id").unwrap();
        let items: Vec<Value> = ids
          .split(',')
          .map(|id| json!({ "id": id, "contentDetails": { "duration": "PT5S" } }))
          .collect();
        return Ok(json!({ "items": items }));
      }
      Ok(json!({ "items": [] }))
    });
    let result = fetch_library(
      &api,
      &LibraryOptions {
        likes: false,
        ..options()
      },
    )
    .await
    .unwrap();
    let urls = api.urls.lock().unwrap();
    let batches: Vec<usize> = urls
      .iter()
      .filter(|url| url.contains("part=contentDetails&id="))
      .map(|url| param(url, "id").unwrap().split(',').count())
      .collect();
    assert_eq!(batches, [50, 50, 10]);
    assert!(
      result["playlists"][0]["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|track| track["duration"] == 5)
    );
  }

  #[tokio::test]
  async fn likes_and_playlists_can_be_skipped() {
    let api = fake(library_answer);
    let result = fetch_library(
      &api,
      &LibraryOptions {
        likes: false,
        playlists: false,
        ..options()
      },
    )
    .await
    .unwrap();
    assert!(result["likes"].as_array().unwrap().is_empty());
    assert!(result["playlists"].as_array().unwrap().is_empty());
    assert!(api.urls.lock().unwrap().is_empty());
  }

  #[tokio::test]
  async fn an_api_error_is_returned() {
    let error = fetch_library(&fake(|_: &str| Err("quota".to_string())), &options())
      .await
      .unwrap_err();
    assert_eq!(error, "quota");
  }

  #[test]
  fn a_video_without_id_or_snippet_is_skipped() {
    assert_eq!(map_video(&json!({ "snippet": {} }), "youtube"), None);
    assert_eq!(map_video(&json!({ "id": "a" }), "youtube"), None);
    assert_eq!(
      map_item(&json!({ "contentDetails": {} , "snippet": {} }), "youtube"),
      None
    );
    let resource = map_item(&json!({ "snippet": { "resourceId": { "videoId": "x" }, "videoOwnerChannelTitle": "O", "channelTitle": "C" } }), "youtube").unwrap();
    assert_eq!(resource["id"], "x");
    assert_eq!(resource["artist"], "O");
    assert_eq!(resource["title"], "untitled");
  }

  #[test]
  fn the_best_thumbnail_is_taken() {
    let thumbnails =
      json!({ "default": { "url": "d" }, "high": { "url": "h" }, "maxres": { "url": "m" } });
    assert_eq!(thumbnail_url(&thumbnails), "m");
    assert_eq!(thumbnail_url(&json!({ "default": { "url": "d" } })), "d");
    assert_eq!(thumbnail_url(&Value::Null), "");
  }

  #[tokio::test]
  async fn the_listener_refuses_a_bad_state_and_a_bad_host_then_closes() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let waiting = tokio::spawn(async move {
      wait_for_callback_at(
        listener,
        CALLBACK_PATH,
        port,
        "etat",
        "Invalid Google login session.",
        LOGIN_TIMEOUT,
      )
      .await
    });
    let host = format!("127.0.0.1:{port}");
    let request =
      |target: &str, host: &str| format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n");
    let send = async |raw: String| {
      let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
      stream.write_all(raw.as_bytes()).await.unwrap();
      let mut reply = String::new();
      stream.read_to_string(&mut reply).await.unwrap();
      reply
    };
    let good = format!("{CALLBACK_PATH}?code=C&state=etat");
    assert!(
      send(request(
        &format!("{CALLBACK_PATH}?code=C&state=mauvais"),
        &host
      ))
      .await
      .starts_with("HTTP/1.1 400")
    );
    assert!(
      send(request(&good, "evil.example"))
        .await
        .starts_with("HTTP/1.1 403")
    );
    assert!(!waiting.is_finished());
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
      .await
      .unwrap();
    stream
      .write_all(request(&good, &host).as_bytes())
      .await
      .unwrap();
    let hit = waiting.await.unwrap().expect("requête valide");
    assert_eq!(hit.callback.code, "C");
    assert!(
      TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .is_err(),
      "deuxième requête : écouteur fermé"
    );
  }

  #[tokio::test(start_paused = true)]
  async fn the_listener_gives_up_after_the_delay() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    assert!(
      wait_for_callback_at(listener, CALLBACK_PATH, port, "etat", "x", LOGIN_TIMEOUT)
        .await
        .is_none()
    );
    assert_eq!(LOGIN_TIMEOUT, Duration::from_secs(120));
  }

  #[test]
  fn the_google_hosts_are_on_the_list() {
    let client = Http::service();
    for url in [
      "https://www.googleapis.com/youtube/v3/videos",
      "https://oauth2.googleapis.com/token",
    ] {
      assert!(
        client.check(&url::Url::parse(url).unwrap()).is_ok(),
        "{url}"
      );
    }
  }
}
