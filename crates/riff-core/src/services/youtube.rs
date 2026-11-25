use super::*;
use futures_util::future::select_ok;

pub fn extract_yt_id_from_any(url_or_id: &str) -> String {
  static RE_SKIP: OnceLock<Regex> = OnceLock::new();
  static RE_ID: OnceLock<Regex> = OnceLock::new();
  static RE_URL: OnceLock<Regex> = OnceLock::new();
  let re_skip = RE_SKIP.get_or_init(|| {
    Regex::new(r"(?i)^(cand_|dz_|it_|sp_|ym_|alb_|local_|track_|dz-|it-)").expect("regex")
  });
  let re_id = RE_ID.get_or_init(|| Regex::new(r"^[a-zA-Z0-9_-]{11}$").expect("regex"));
  let re_url = RE_URL.get_or_init(|| {
    Regex::new(r"(?:v=|youtu\.be/|/embed/|/v/)([a-zA-Z0-9_-]{11})").expect("regex")
  });
  let s = url_or_id.trim();
  if s.is_empty() || re_skip.is_match(s) {
    return String::new();
  }
  if re_id.is_match(s) {
    return s.to_string();
  }
  match re_url.captures(s) {
    Some(c) if !re_skip.is_match(&c[1]) => c[1].to_string(),
    _ => String::new(),
  }
}

struct InnertubeClient {
  name: &'static str,
  version: &'static str,
  user_agent: &'static str,
  host: &'static str,
  extra: Vec<(&'static str, String)>,
  context: Value,
}

async fn query_innertube(
  client: &Http,
  c: InnertubeClient,
  vid: &str,
  quality: &str,
) -> Result<String, String> {
  let mut ctx_client =
    json!({ "clientName": c.name, "clientVersion": c.version, "hl": "en", "gl": "US" });
  if let (Some(dst), Some(src)) = (ctx_client.as_object_mut(), c.context.as_object()) {
    for (k, v) in src {
      dst.insert(k.clone(), v.clone());
    }
  }
  let body = json!({
    "videoId": vid,
    "context": { "client": ctx_client },
    "contentCheckOk": true,
    "racyCheckOk": true
  });
  let mut req = client
    .post(format!(
      "https://{}/youtubei/v1/player?prettyPrint=false",
      c.host
    ))
    .header(header::CONTENT_TYPE, "application/json")
    .header(header::USER_AGENT, c.user_agent)
    .timeout(Duration::from_millis(2200))
    .json(&body);
  if c.name != "VISIONOS" {
    req = req.header("X-Goog-Api-Format-Version", "2");
  }
  for (k, v) in c.extra {
    req = req.header(k, v);
  }
  let transport = |e: net::Error| {
    if e.is_timeout() {
      "délai dépassé".to_string()
    } else {
      "erreur réseau ou réponse illisible".to_string()
    }
  };
  let resp = req.send().await.map_err(transport)?;
  if resp.status().as_u16() != 200 {
    return Err(format!("HTTP {}", resp.status().as_u16()));
  }
  let data: Value = resp.json().await.map_err(transport)?;
  select_player_stream(&data, quality)
}

// Les causes d'échec vont au journal : jamais le texte `reason` de YouTube, seulement `playabilityStatus.status`
// (une énumération : `LOGIN_REQUIRED`, `UNPLAYABLE`...), filtré par prudence.
fn select_player_stream(data: &Value, quality: &str) -> Result<String, String> {
  if let Some(status) = data.get("playabilityStatus").and_then(|p| p.get("status"))
    && status.as_str() != Some("OK")
  {
    let label: String = status
      .as_str()
      .unwrap_or("")
      .chars()
      .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
      .take(32)
      .collect();
    return Err(format!("playabilityStatus {label}"));
  }
  let streaming = data.get("streamingData");
  let list = |key: &str| -> Vec<Value> {
    streaming
      .and_then(|s| s.get(key))
      .and_then(Value::as_array)
      .cloned()
      .unwrap_or_default()
  };
  let muxed = list("formats");
  let usable = |f: &Value| {
    is_truthy(f.get("url")) && !is_truthy(f.get("signatureCipher")) && !is_truthy(f.get("cipher"))
  };
  let mime = |f: &Value| {
    f.get("mimeType")
      .and_then(Value::as_str)
      .unwrap_or("")
      .to_string()
  };
  let bitrate = |f: &Value| f.get("bitrate").and_then(Value::as_f64).unwrap_or(0.0);

  let audio: Vec<Value> = list("adaptiveFormats")
    .into_iter()
    .chain(muxed.iter().cloned())
    .filter(|f| usable(f) && mime(f).starts_with("audio/"))
    .collect();
  if !audio.is_empty() {
    let by_bitrate_desc = |a: &Value, b: &Value| {
      bitrate(b)
        .partial_cmp(&bitrate(a))
        .unwrap_or(std::cmp::Ordering::Equal)
    };
    let by_bitrate_asc = |a: &Value, b: &Value| {
      bitrate(a)
        .partial_cmp(&bitrate(b))
        .unwrap_or(std::cmp::Ordering::Equal)
    };
    let m4a: Vec<Value> = audio
      .iter()
      .filter(|f| mime(f).contains("mp4") || mime(f).contains("m4a"))
      .cloned()
      .collect();
    let webm: Vec<Value> = audio
      .iter()
      .filter(|f| mime(f).contains("webm") || mime(f).contains("opus"))
      .cloned()
      .collect();
    let mut chosen = if !m4a.is_empty() {
      m4a
    } else if !webm.is_empty() {
      webm
    } else {
      audio
    };
    if quality == "low" {
      chosen.sort_by(by_bitrate_asc);
    } else {
      chosen.sort_by(by_bitrate_desc);
    }
    return chosen
      .first()
      .and_then(|f| f.get("url"))
      .and_then(Value::as_str)
      .map(str::to_string)
      .ok_or_else(|| "aucun format audio utilisable".to_string());
  }
  muxed
    .iter()
    .find(|f| f.get("itag").and_then(Value::as_i64) == Some(18) && usable(f))
    .and_then(|f| f.get("url"))
    .and_then(Value::as_str)
    .map(str::to_string)
    .ok_or_else(|| "aucun format audio utilisable".to_string())
}

const UA_CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36";
const UA_VISIONOS: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

// Identifiant de visiteur anonyme lu sur la page d'accueil (réutilisé 6 h). Sans lui, innertube répond « Sign in to confirm you're not a bot ».
async fn visitor_data(client: &Http) -> Option<String> {
  static CACHE: Mutex<Option<(std::time::Instant, String)>> = Mutex::new(None);
  if let Ok(guard) = CACHE.lock()
    && let Some((at, vd)) = guard.as_ref()
    && at.elapsed() < Duration::from_secs(6 * 3600)
  {
    return Some(vd.clone());
  }
  let html = client
    .get("https://www.youtube.com/")
    .header(header::USER_AGENT, UA_CHROME)
    .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
    .timeout(Duration::from_secs(4))
    .send()
    .await
    .ok()?
    .text()
    .await
    .ok()?;
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| Regex::new(r#""visitorData":"([^"]+)""#).expect("regex"));
  let vd = re.captures(&html)?[1].to_string();
  if let Ok(mut guard) = CACHE.lock() {
    *guard = Some((std::time::Instant::now(), vd.clone()));
  }
  Some(vd)
}

// Résolution directe via les clients innertube publics. VISIONOS d'abord (comme yt-dlp) : les URL du client IOS sont
// refusées (403) au-delà du premier Mo sans jeton PO. Repli : IOS et ANDROID_MUSIC en parallèle (premier succès gagne).
pub async fn fetch_direct_yt_stream(core: &Core, video_id: &str, quality: &str) -> Option<String> {
  let vid = extract_yt_id_from_any(video_id);
  if vid.is_empty() {
    return None;
  }
  if let Some(vd) = visitor_data(&core.net.yt_client).await {
    let visionos = InnertubeClient {
      name: "VISIONOS",
      version: "1.02",
      user_agent: UA_VISIONOS,
      host: "www.youtube.com",
      extra: vec![
        ("X-Youtube-Client-Name", "101".to_string()),
        ("X-Youtube-Client-Version", "1.02".to_string()),
        ("Origin", "https://www.youtube.com".to_string()),
        ("X-Goog-Visitor-Id", vd),
      ],
      context: json!({
        "deviceMake": "Apple",
        "deviceModel": "RealityDevice17,1",
        "userAgent": UA_VISIONOS,
        "osName": "visionOS",
        "osVersion": "26.5.23O471",
        "timeZone": "UTC",
        "utcOffsetMinutes": 0
      }),
    };
    if let Ok(url) = resolve_client(core, visionos, &vid, quality).await {
      return Some(url);
    }
  } else {
    logging::log_entry("WARN", "youtube VISIONOS : visitorData indisponible");
  }
  let mut clients = vec![
    InnertubeClient {
      name: "IOS",
      version: "20.10.4",
      user_agent: UA_IOS_APP,
      host: "www.youtube.com",
      extra: Vec::new(),
      context: json!({
        "deviceMake": "Apple",
        "deviceModel": "iPhone16,2",
        "osName": "iOS",
        "osVersion": "18.3.2.22D60"
      }),
    },
    InnertubeClient {
      name: "ANDROID_MUSIC",
      version: "7.27.52",
      user_agent: "com.google.android.apps.youtube.music/7.27.52 (Linux; U; Android 14; en_US; Quest 3) gzip",
      host: "music.youtube.com",
      extra: Vec::new(),
      context: json!({ "androidSdkVersion": 34 }),
    },
  ];
  if let Some(auth) = ytdlp::yt_auth_headers("https://www.youtube.com") {
    clients.insert(
      0,
      InnertubeClient {
        name: "TVHTML5",
        version: "7.20250312.16.00",
        user_agent: "Mozilla/5.0 (SMART-TV; LINUX; Tizen 6.5) AppleWebKit/537.36 (KHTML, like Gecko) 85.0.4183.93/6.5 TV Safari/537.36",
        host: "www.youtube.com",
        extra: auth,
        context: json!({}),
      },
    );
  }
  let futures = clients
    .into_iter()
    .map(|c| Box::pin(resolve_client(core, c, &vid, quality)))
    .collect::<Vec<_>>();
  select_ok(futures).await.ok().map(|(url, _)| url)
}

// Requête innertube, puis sonde de l'URL rendue : un client dont l'URL est refusée compte comme un échec.
async fn resolve_client(
  core: &Core,
  c: InnertubeClient,
  vid: &str,
  quality: &str,
) -> Result<String, ()> {
  let name = c.name;
  let url = match query_innertube(&core.net.yt_client, c, vid, quality).await {
    Ok(url) => url,
    Err(cause) => {
      logging::log_entry("WARN", &format!("youtube {name} : échec ({cause})"));
      return Err(());
    }
  };
  if accept_stream_url(core, &url, "innertube").await {
    Ok(url)
  } else {
    Err(())
  }
}

// Sonde réaliste d'une URL googlevideo : GET `Range: <range>` avec le User-Agent que `audio_proxy` appliquera, redirections
// suivies, corps non lu. `Err(Some(statut))` : refus ; `Err(None)` : pas de réponse (réseau, délai, redirections).
// Hors googlevideo, rien à vérifier.
async fn probe_range(core: &Core, url: &Url, range: &str) -> Result<(), Option<u16>> {
  if !url
    .host_str()
    .is_some_and(|h| h.ends_with("googlevideo.com"))
  {
    return Ok(());
  }
  let work = async {
    let mut current = url.clone();
    for _ in 0..=3 {
      let resp = core
        .net
        .yt_client
        .get(current.as_str())
        .header(header::RANGE, range)
        .header(
          header::USER_AGENT,
          playback::google_video_user_agent(&current),
        )
        .send()
        .await
        .map_err(|_| None)?;
      let status = resp.status().as_u16();
      if matches!(status, 301 | 302 | 303 | 307 | 308)
        && let Some(next) = resp
          .headers()
          .get(header::LOCATION)
          .and_then(|v| v.to_str().ok())
          .and_then(|loc| current.join(loc).ok())
      {
        current = next;
        continue;
      }
      return if status == 200 || status == 206 {
        Ok(())
      } else {
        Err(Some(status))
      };
    }
    Err(None)
  };
  tokio::time::timeout(Duration::from_secs(3), work)
    .await
    .unwrap_or(Err(None))
}

// Décide si une URL de flux YouTube peut être rendue au lecteur (plage ouverte `bytes=0-`, comme lui) et journalise
// le verdict : client (`c`) et statut, jamais l'URL. `source` : « innertube » ou « yt-dlp ».
pub async fn accept_stream_url(core: &Core, stream_url: &str, source: &str) -> bool {
  let Ok(url) = Url::parse(stream_url) else {
    return false;
  };
  let client = playback::client_label(&url);
  match probe_range(core, &url, "bytes=0-").await {
    Ok(()) => {
      logging::log_entry(
        "INFO",
        &format!("youtube flux retenu : source {source}, client {client}"),
      );
      true
    }
    Err(status) => {
      let got = status.map_or("sans réponse".to_string(), |s| format!("statut {s}"));
      logging::log_entry(
        "WARN",
        &format!("youtube flux écarté par la sonde : source {source}, client {client}, {got}"),
      );
      false
    }
  }
}

const UA_IOS_APP: &str =
  "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)";

// Les URL googlevideo sans jeton PO refusent (403) les plages ouvertes, parfois dès `bytes=0-`, parfois au-delà du
// premier Mo selon le client et le moment : on sonde une plage ouverte après 1 Mo, comme le lecteur la demande.
async fn probe_second_megabyte(core: &Core, url: &Url) -> bool {
  let clen: u64 = url
    .query_pairs()
    .find(|(k, _)| k == "clen")
    .and_then(|(_, v)| v.parse().ok())
    .unwrap_or(0);
  clen < 2 * 1024 * 1024 || probe_range(core, url, "bytes=1048576-").await.is_ok()
}

// Vérifie qu'une URL de flux renvoie bien de l'audio (16 Ko lus ou fin de flux), jusqu'à 3 redirections.
pub async fn verify_stream_playable(core: &Core, stream_url: &str) -> bool {
  if stream_url.contains(".m3u8") {
    return true;
  }
  let Ok(mut current) = Url::parse(stream_url) else {
    return false;
  };
  if !probe_second_megabyte(core, &current).await {
    return false;
  }
  let work = async {
    for _ in 0..=3 {
      let c_param = current
        .query_pairs()
        .find(|(k, _)| k == "c")
        .map(|(_, v)| v.to_uppercase())
        .unwrap_or_default();
      let ua = match c_param.as_str() {
        "IOS" => UA_IOS_APP,
        "ANDROID_VR" => {
          "com.google.android.apps.youtube.vr.oculus/1.61.48 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip"
        }
        _ => {
          "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36"
        }
      };
      let mut req = core
        .net
        .yt_client
        .get(current.as_str())
        .header(header::RANGE, "bytes=0-")
        .header(header::USER_AGENT, ua)
        .header(
          header::ACCEPT,
          "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header(header::ACCEPT_LANGUAGE, "en-us,en;q=0.5")
        .header("Sec-Fetch-Mode", "navigate");
      if current
        .host_str()
        .is_some_and(|h| h.ends_with("googlevideo.com"))
      {
        req = req
          .header(header::ORIGIN, "https://www.youtube.com")
          .header(header::REFERER, "https://www.youtube.com/");
      }
      let Ok(mut resp) = req.send().await else {
        return false;
      };
      let status = resp.status().as_u16();
      if matches!(status, 301 | 302 | 303 | 307 | 308)
        && let Some(next) = resp
          .headers()
          .get(header::LOCATION)
          .and_then(|v| v.to_str().ok())
          .and_then(|loc| current.join(loc).ok())
      {
        current = next;
        continue;
      }
      if status != 200 && status != 206 {
        return false;
      }
      let mut received = 0usize;
      while let Ok(Some(chunk)) = resp.chunk().await {
        received += chunk.len();
        if received >= 16384 {
          return true;
        }
      }
      return received > 0;
    }
    false
  };
  tokio::time::timeout(Duration::from_secs(8), work)
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
  use super::select_player_stream;
  use serde_json::json;

  #[test]
  fn parse_player_stream_prefers_m4a_across_all_qualities() {
    let data = json!({
      "playabilityStatus": { "status": "OK" },
      "streamingData": {
        "adaptiveFormats": [
          { "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000, "url": "https://x/opus-high" },
          { "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 48000, "url": "https://x/opus-low" },
          { "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000, "url": "https://x/m4a-128" },
          { "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "bitrate": 64000, "url": "https://x/m4a-64" }
        ]
      }
    });

    assert_eq!(
      select_player_stream(&data, "low").unwrap(),
      "https://x/m4a-64"
    );
    assert_eq!(
      select_player_stream(&data, "medium").unwrap(),
      "https://x/m4a-128"
    );
    assert_eq!(
      select_player_stream(&data, "high").unwrap(),
      "https://x/m4a-128"
    );
    assert_eq!(
      select_player_stream(&data, "max").unwrap(),
      "https://x/m4a-128"
    );
  }
}
