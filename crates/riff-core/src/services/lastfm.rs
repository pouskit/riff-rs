use super::*;
use crate::secrets::SecretName;
use md5::Md5;

const LASTFM_UA: &str = "Riff-Desktop-Player/43.0";

fn lastfm_sign(params: &[(String, String)], secret: &str) -> String {
  let mut sorted: Vec<&(String, String)> = params
    .iter()
    .filter(|(k, _)| k != "format" && k != "callback")
    .collect();
  sorted.sort_by(|a, b| a.0.cmp(&b.0));
  let mut sig = String::new();
  for (k, v) in sorted {
    sig.push_str(k);
    sig.push_str(v);
  }
  sig.push_str(secret);
  let mut hasher = Md5::new();
  hasher.update(sig.as_bytes());
  let mut hex = String::with_capacity(32);
  for b in hasher.finalize() {
    hex.push_str(&format!("{b:02x}"));
  }
  hex
}

fn lastfm_error(message: &str) -> Value {
  json!({ "error": 1, "message": message })
}

async fn call_lastfm(
  client: &Http,
  mut params: Vec<(String, String)>,
  api_key: &str,
  secret: &str,
  post: bool,
) -> Value {
  if api_key.is_empty() || secret.is_empty() {
    return lastfm_error("Last.fm API key and secret are not configured");
  }
  params.push(("api_key".to_string(), api_key.to_string()));
  let sig = lastfm_sign(&params, secret);
  params.push(("api_sig".to_string(), sig));
  params.push(("format".to_string(), "json".to_string()));
  let body = url::form_urlencoded::Serializer::new(String::new())
    .extend_pairs(params.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    .finish();
  let request = if post {
    client
      .post("https://ws.audioscrobbler.com/2.0/")
      .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
      .body(body)
  } else {
    client.get(format!("https://ws.audioscrobbler.com/2.0/?{body}"))
  };
  let resp = match request
    .header(header::USER_AGENT, LASTFM_UA)
    .timeout(Duration::from_secs(10))
    .send()
    .await
  {
    Ok(r) => r,
    Err(e) if e.is_timeout() => return lastfm_error("Last.fm request timeout"),
    Err(e) => return lastfm_error(&e.to_string()),
  };
  match resp.bytes().await {
    Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
      .unwrap_or_else(|_| lastfm_error("Invalid Last.fm response")),
    Err(e) => lastfm_error(&e.to_string()),
  }
}

fn str_field(body: &Value, key: &str) -> String {
  value_to_string(body.get(key))
}

pub async fn lastfm(core: &Core, method: Method, path: &str, params: Params) -> CoreResult {
  let body = &params.body;
  let payload: Value = if method == Method::Post && body.len() <= 100_000 {
    serde_json::from_slice(body).unwrap_or_else(|_| json!({}))
  } else {
    json!({})
  };
  let payload = if payload.is_object() {
    payload
  } else {
    json!({})
  };

  let custom_key = str_field(&payload, "apiKey").trim().to_string();
  let custom_secret = str_field(&payload, "apiSecret").trim().to_string();
  // Pas de clé par défaut dans le code : paire personnalisée de l'UI, sinon variables d'environnement.
  let (api_key, api_secret) = if !custom_key.is_empty() && !custom_secret.is_empty() {
    (custom_key, custom_secret)
  } else {
    let env_key = std::env::var("RIFF_LASTFM_KEY").unwrap_or_default();
    let env_secret = std::env::var("RIFF_LASTFM_SECRET").unwrap_or_default();
    if env_key.is_empty() || env_secret.is_empty() {
      // Secret enregistré par la fenêtre Last.fm : jamais dans un fichier en clair.
      let stored = core
        .auth
        .secrets
        .get(SecretName::LastfmApiSecret)
        .ok()
        .flatten()
        .unwrap_or_default();
      (crate::secrets::lastfm_api_key(), stored)
    } else {
      (env_key, env_secret)
    }
  };
  let client = &core.net.http_client;
  let pair = |k: &str, v: String| (k.to_string(), v);
  let message = |r: &Value| value_to_string(r.get("message"));
  let session_of = |r: &Value| {
    let key = value_to_string(r.get("session").and_then(|s| s.get("key")));
    let name = value_to_string(r.get("session").and_then(|s| s.get("name")));
    (key, name)
  };

  let out = match path {
    "/api/lastfm/token" => {
      let r = call_lastfm(
        client,
        vec![pair("method", "auth.getToken".into())],
        &api_key,
        &api_secret,
        false,
      )
      .await;
      let token = value_to_string(r.get("token"));
      let auth_url = if token.is_empty() {
        String::new()
      } else {
        format!(
          "https://www.last.fm/api/auth/?api_key={}&token={}",
          encode_uri_component(&api_key),
          encode_uri_component(&token)
        )
      };
      json!({ "success": !token.is_empty(), "token": token, "authUrl": auth_url, "error": message(&r) })
    }
    "/api/lastfm/session" => {
      let r = call_lastfm(
        client,
        vec![
          pair("method", "auth.getSession".into()),
          pair("token", str_field(&payload, "token")),
        ],
        &api_key,
        &api_secret,
        false,
      )
      .await;
      let (key, name) = session_of(&r);
      json!({ "success": !key.is_empty(), "sessionKey": key, "username": name, "error": message(&r) })
    }
    "/api/lastfm/mobile-session" => {
      let r = call_lastfm(
        client,
        vec![
          pair("method", "auth.getMobileSession".into()),
          pair(
            "username",
            str_field(&payload, "username").trim().to_string(),
          ),
          pair("password", str_field(&payload, "password")),
        ],
        &api_key,
        &api_secret,
        true,
      )
      .await;
      let (key, name) = session_of(&r);
      json!({ "success": !key.is_empty(), "sessionKey": key, "username": name, "error": message(&r) })
    }
    "/api/lastfm/now-playing" | "/api/lastfm/scrobble" => {
      let scrobble = path.ends_with("scrobble");
      let session_key = if is_truthy(payload.get("sk")) {
        str_field(&payload, "sk")
      } else {
        core
          .auth
          .secrets
          .get(SecretName::LastfmSession)
          .ok()
          .flatten()
          .unwrap_or_default()
      };
      if session_key.is_empty()
        || !is_truthy(payload.get("artist"))
        || !is_truthy(payload.get("track"))
      {
        return json_response(
          200,
          &json!({ "success": false, "error": "Missing sk/artist/track" }),
        );
      }
      let idx = |k: &str| {
        if scrobble {
          format!("{k}[0]")
        } else {
          k.to_string()
        }
      };
      let mut params = vec![
        pair(
          "method",
          if scrobble {
            "track.scrobble"
          } else {
            "track.updateNowPlaying"
          }
          .to_string(),
        ),
        pair("sk", session_key),
        pair(&idx("artist"), str_field(&payload, "artist")),
        pair(&idx("track"), str_field(&payload, "track")),
      ];
      if scrobble {
        let ts = if is_truthy(payload.get("timestamp")) {
          str_field(&payload, "timestamp")
        } else {
          (now_millis() / 1000).to_string()
        };
        params.push(pair("timestamp[0]", ts));
      }
      if is_truthy(payload.get("album")) {
        params.push(pair(&idx("album"), str_field(&payload, "album")));
      }
      let duration = normalize_number_value(payload.get("duration"));
      if let Some(d) = duration.as_f64()
        && d > 0.0
      {
        params.push(pair(&idx("duration"), format!("{}", d.round() as i64)));
      }
      let r = call_lastfm(client, params, &api_key, &api_secret, true).await;
      json!({ "success": !is_truthy(r.get("error")), "data": r, "error": message(&r) })
    }
    _ => return text_response(404, "Not found"),
  };
  json_response(200, &out)
}
