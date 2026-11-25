use super::*;
use crate::error::CoreError;
use crate::library::library_mime_type;
use crate::reply::{Reply, serve_file_with_range};
use bytes::Bytes;
use futures_util::StreamExt;

pub const AUDIO_EXTENSIONS: [&str; 10] = [
  "mp3", "m4a", "flac", "opus", "wav", "ogg", "oga", "aac", "webm", "weba",
];

pub fn downloads_dir() -> PathBuf {
  let dir = config::resolve_download_dir().unwrap_or_else(paths::default_download_dir);
  if !dir.exists() {
    let _ = fs::create_dir_all(&dir);
  }
  dir
}

pub async fn downloaded_track(params: Params) -> CoreResult {
  let (query, headers) = (&params.query, &params.headers);
  let raw = query.get("name").map(String::as_str).unwrap_or("");
  let safe_name = raw.rsplit(['/', '\\']).next().unwrap_or("");
  let ext = Path::new(safe_name)
    .extension()
    .and_then(|e| e.to_str())
    .map(str::to_lowercase)
    .unwrap_or_default();
  if safe_name.is_empty() || !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
    return text_response(404, "File not found in downloads");
  }
  let file_path = downloads_dir().join(safe_name);
  serve_file_with_range(
    &file_path,
    headers,
    library_mime_type(&ext),
    "File not found in downloads",
    404,
    "File not found in downloads",
  )
}

const UA_IOS: &str =
  "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)";
const UA_ANDROID_MUSIC: &str = "com.google.android.apps.youtube.music/7.27.52 (Linux; U; Android 14; en_US; sdk_gphone64_x86_64 Build/UE1A.230829.036) gzip";
const UA_VISIONOS: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const UA_WEB: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

fn query_param(url: &Url, name: &str) -> String {
  url
    .query_pairs()
    .find(|(k, _)| k == name)
    .map(|(_, v)| v.into_owned())
    .unwrap_or_default()
}

pub(crate) fn google_video_user_agent(url: &Url) -> &'static str {
  let client = query_param(url, "c");
  if client.eq_ignore_ascii_case("ANDROID_MUSIC") {
    UA_ANDROID_MUSIC
  } else if client.eq_ignore_ascii_case("VISIONOS") {
    UA_VISIONOS
  } else {
    UA_IOS
  }
}

// Nom du client YouTube de l'URL (paramètre `c`), prêt pour le journal : majuscules, `A-Z0-9_`, 24 caractères au plus.
pub(crate) fn client_label(url: &Url) -> String {
  let label: String = query_param(url, "c")
    .to_ascii_uppercase()
    .chars()
    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
    .take(24)
    .collect();
  if label.is_empty() {
    "inconnu".to_string()
  } else {
    label
  }
}

// Retire du cache les résolutions dont l'URL brute ou proxifiée correspond (URL expirée, refusée ou coupée).
pub fn evict_stream_cache(core: &Core, raw_url: &str) {
  if raw_url.is_empty() {
    return;
  }
  let encoded = encode_uri_component(raw_url);
  if let Ok(mut guard) = core.caches.stream_url_cache.lock() {
    guard.retain(|(_, (_, data))| {
      let raw = data.get("rawStreamUrl").and_then(Value::as_str);
      let stream = data.get("streamUrl").and_then(Value::as_str);
      !(raw == Some(raw_url)
        || stream == Some(raw_url)
        || stream.is_some_and(|s| s.contains(&encoded)))
    });
  }
}

fn proxy_error(status: u16, msg: &'static str) -> CoreResult {
  Ok(Reply::buffered(
    status,
    vec![("access-control-allow-origin".to_string(), "*".to_string())],
    Bytes::from_static(msg.as_bytes()),
  ))
}

pub async fn audio_proxy(core: &Arc<Core>, method: Method, params: Params) -> CoreResult {
  let (query, headers) = (&params.query, &params.headers);
  let target = query.get("url").map(String::as_str).unwrap_or("");
  if target.is_empty() {
    return text_response(400, "Missing url");
  }
  if method != Method::Get && method != Method::Head {
    return text_response(405, "Method not allowed");
  }
  let Ok(mut current) = Url::parse(target) else {
    return text_response(400, "Invalid url");
  };
  let initial = target.to_string();

  let mut redirects_left = 5;
  let upstream = loop {
    if current.scheme() != "http" && current.scheme() != "https" {
      return text_response(400, "Unsupported protocol");
    }
    if core.net.yt_client.check(&current).is_err() {
      return text_response(403, "Blocked target");
    }

    let host = current.host_str().unwrap_or("").to_lowercase();
    let is_gv = host.ends_with("googlevideo.com");
    let upstream_method = if method == Method::Head {
      reqwest::Method::HEAD
    } else {
      reqwest::Method::GET
    };
    let mut req = core
      .net
      .yt_client
      .request(upstream_method, current.as_str())
      .streaming();
    for name in [header::RANGE, header::ACCEPT, header::ACCEPT_LANGUAGE] {
      if let Some(v) = headers.get(name.as_str()) {
        req = req.header(name, v);
      }
    }
    if is_gv {
      if headers.get("range").is_none() {
        req = req.header(header::RANGE, "bytes=0-");
      }
      req = req.header(header::USER_AGENT, google_video_user_agent(&current));
      let client_name = query_param(&current, "c").to_uppercase();
      if matches!(client_name.as_str(), "WEB" | "WEB_REMIX" | "TVHTML5")
        && let Some(auth) = ytdlp::yt_auth_headers("https://www.youtube.com")
        && let Some((_, cookie)) = auth.into_iter().find(|(k, _)| *k == "Cookie")
      {
        req = req.header(header::COOKIE, cookie);
      }
    } else {
      let ua = headers.get("user-agent").unwrap_or(UA_WEB);
      req = req.header(header::USER_AGENT, ua);
    }

    // Un seul nouvel essai si la connexion réutilisée est morte avant toute réponse (GET/HEAD sans corps).
    let retry = req.try_clone();
    let mut sent = tokio::time::timeout(Duration::from_secs(30), req.send()).await;
    if matches!(sent, Ok(Err(_)))
      && let Some(again) = retry
    {
      sent = tokio::time::timeout(Duration::from_secs(30), again.send()).await;
    }
    let range = headers.get("range").unwrap_or("-");
    let resp = match sent {
      Ok(Ok(r)) => r,
      failed => {
        let cause = match failed {
          Ok(Err(error)) => error.to_string(),
          _ => "délai de 30 s dépassé".to_string(),
        };
        logging::log_entry(
          "WARN",
          &format!(
            "audio-proxy 502 : {host} sans réponse ({cause}), range {range}, c={}",
            client_label(&current)
          ),
        );
        evict_stream_cache(core, &initial);
        evict_stream_cache(core, current.as_str());
        return proxy_error(502, "Proxy error");
      }
    };

    let status = resp.status();
    if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
      && redirects_left > 0
      && let Some(next) = resp
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|loc| current.join(loc).ok())
    {
      redirects_left -= 1;
      current = next;
      continue;
    }
    if status.as_u16() >= 400 {
      logging::log_entry(
        "WARN",
        &format!(
          "audio-proxy 502 : {host} a répondu {}, range {range}, c={}",
          status.as_u16(),
          client_label(&current)
        ),
      );
      evict_stream_cache(core, &initial);
      evict_stream_cache(core, current.as_str());
      return proxy_error(502, "Upstream error");
    }
    break resp;
  };

  let status = upstream.status().as_u16();
  const DROP: [&str; 10] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "access-control-allow-origin",
    "access-control-allow-credentials",
  ];
  const OVERRIDDEN: [&str; 2] = [
    "access-control-allow-headers",
    "access-control-expose-headers",
  ];
  let mut out: Vec<(String, String)> = Vec::new();
  for (name, value) in upstream.headers() {
    if DROP.contains(&name.as_str())
      || OVERRIDDEN.contains(&name.as_str())
      || name == header::X_FRAME_OPTIONS
    {
      continue;
    }
    out.push((
      name.as_str().to_string(),
      String::from_utf8_lossy(value.as_bytes()).into_owned(),
    ));
  }
  out.push(("access-control-allow-origin".to_string(), "*".to_string()));
  out.push(("access-control-allow-headers".to_string(), "*".to_string()));
  out.push((
    "access-control-expose-headers".to_string(),
    "Content-Length, Content-Range, Accept-Ranges".to_string(),
  ));

  if method == Method::Head {
    if !out.iter().any(|(name, _)| name == "content-length") {
      out.push(("content-length".to_string(), "0".to_string()));
    }
    return Ok(Reply::Stream {
      status,
      headers: out,
      body: futures_util::stream::empty().boxed(),
    });
  }
  let core = Arc::clone(core);
  let final_url = current.to_string();
  let host = current.host_str().unwrap_or("").to_string();
  let body = upstream
    .bytes_stream()
    .map(move |chunk| {
      if chunk.is_err() {
        evict_stream_cache(&core, &initial);
        evict_stream_cache(&core, &final_url);
      }
      chunk.map_err(|_| CoreError::Upstream {
        host: host.clone(),
        status: None,
      })
    })
    .boxed();
  Ok(Reply::Stream {
    status,
    headers: out,
    body,
  })
}

pub async fn audio_output_info() -> CoreResult {
  json_response(
    200,
    &json!({
      "name": "Default output",
      "battery": null
    }),
  )
}
