use super::*;

const SC_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0.0.0 Safari/537.36";
const SC_CLIENT_ID_TTL_MS: u64 = 30 * 60 * 1000;

pub fn normalize_soundcloud_url(raw: &str) -> String {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| Regex::new(r"(?i)soundcloud:tracks:(\d+)").expect("regex sc url"));
  let decoded = urlencoding_decode(raw);
  match re.captures(&decoded) {
    Some(c) => format!("https://api.soundcloud.com/tracks/{}", &c[1]),
    None => raw.to_string(),
  }
}

fn urlencoding_decode(s: &str) -> String {
  let bytes = s.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  while i < bytes.len() {
    if bytes[i] == b'%'
      && let Some(hex) = s.get(i + 1..i + 3)
      && let Ok(v) = u8::from_str_radix(hex, 16)
    {
      out.push(v);
      i += 3;
    } else {
      out.push(bytes[i]);
      i += 1;
    }
  }
  String::from_utf8_lossy(&out).into_owned()
}

// Récupère le client_id public du site SoundCloud (aucune valeur par défaut codée en dur).
// `stale` : id qui vient d'être refusé (401/403), force un nouveau scrape s'il est encore en cache.
pub async fn sc_client_id(core: &Core, stale: Option<&str>) -> String {
  let current = |s: &Core| {
    s.auth
      .sc_client_id
      .lock()
      .map(|g| g.clone())
      .unwrap_or_default()
  };
  let fresh = |cur: &(String, u64)| {
    !cur.0.is_empty()
      && now_millis().saturating_sub(cur.1) < SC_CLIENT_ID_TTL_MS
      && Some(cur.0.as_str()) != stale
  };
  let cur = current(core);
  if fresh(&cur) {
    return cur.0;
  }
  let _guard = core.auth.sc_refresh.lock().await;
  let cur = current(core);
  if fresh(&cur) {
    return cur.0;
  }
  if let Some(id) = scrape_sc_client_id(&core.net.http_client).await {
    if let Ok(mut g) = core.auth.sc_client_id.lock() {
      *g = (id.clone(), now_millis());
    }
    return id;
  }
  cur.0
}

async fn scrape_sc_client_id(client: &Http) -> Option<String> {
  static RE_SCRIPT: OnceLock<Regex> = OnceLock::new();
  static RE_ID: OnceLock<Regex> = OnceLock::new();
  let re_script = RE_SCRIPT.get_or_init(|| {
    Regex::new(r#"src="(https://a-v2\.sndcdn\.com/assets/[^"]+\.js)""#).expect("regex")
  });
  let re_id = RE_ID
    .get_or_init(|| Regex::new(r#"client_id\s*[:=]\s*["']([a-zA-Z0-9]{32})["']"#).expect("regex"));
  let html = client
    .get("https://soundcloud.com")
    .header(header::USER_AGENT, SC_UA)
    .timeout(Duration::from_secs(10))
    .send()
    .await
    .ok()?
    .text()
    .await
    .ok()?;
  let scripts: Vec<String> = re_script
    .captures_iter(&html)
    .map(|c| c[1].to_string())
    .collect();
  for url in scripts.iter().rev().take(5) {
    let Ok(resp) = client
      .get(url)
      .timeout(Duration::from_secs(10))
      .send()
      .await
    else {
      continue;
    };
    let Ok(js) = resp.text().await else {
      continue;
    };
    if let Some(c) = re_id.captures(&js) {
      return Some(c[1].to_string());
    }
  }
  None
}

pub async fn fetch_sc_search(core: &Core, query: &str, limit: usize) -> Vec<Value> {
  fetch_sc_search_reached(core, query, limit).await.0
}

// Comme `fetch_sc_search`, avec « une réponse HTTP est arrivée » (faux = échec de transport à chaque essai).
pub async fn fetch_sc_search_reached(core: &Core, query: &str, limit: usize) -> (Vec<Value>, bool) {
  let mut reached = false;
  let mut cid = sc_client_id(core, None).await;
  for _ in 0..2 {
    let url = format!(
      "https://api-v2.soundcloud.com/search/tracks?q={}&limit={limit}&client_id={cid}",
      encode_uri_component(query)
    );
    let Ok(resp) = core
      .net
      .http_client
      .get(&url)
      .header(
        header::USER_AGENT,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
      )
      .timeout(Duration::from_secs(10))
      .send()
      .await
    else {
      continue;
    };
    reached = true;
    if matches!(resp.status().as_u16(), 401 | 403) {
      cid = sc_client_id(core, Some(&cid)).await;
      continue;
    }
    if !resp.status().is_success() {
      continue;
    }
    let Ok(data) = resp.json::<Value>().await else {
      continue;
    };
    let tracks: Vec<Value> = data
      .get("collection")
      .and_then(Value::as_array)
      .map(|col| col.iter().filter_map(sc_track_to_result).collect())
      .unwrap_or_default();
    if !tracks.is_empty() {
      return (tracks, true);
    }
  }
  (Vec::new(), reached)
}

fn sc_track_to_result(t: &Value) -> Option<Value> {
  if !is_truthy(t.get("id")) || !is_truthy(t.get("title")) {
    return None;
  }
  let user = t.get("user");
  let str_of = |v: Option<&Value>| {
    v.and_then(Value::as_str)
      .filter(|s| !s.is_empty())
      .map(str::to_string)
  };
  let raw_thumb = str_of(t.get("artwork_url"))
    .or_else(|| str_of(user.and_then(|u| u.get("avatar_url"))))
    .unwrap_or_default();
  let thumbnail = raw_thumb.replace("-large", "-t500x500");
  let artist = str_of(user.and_then(|u| u.get("username")))
    .or_else(|| str_of(user.and_then(|u| u.get("name"))))
    .or_else(|| str_of(t.get("publisher_metadata").and_then(|p| p.get("artist"))))
    .unwrap_or_else(|| "unknown artist".to_string());
  let ms = [t.get("duration"), t.get("full_duration")]
    .into_iter()
    .find_map(|v| v.and_then(Value::as_f64).filter(|d| *d != 0.0))
    .unwrap_or(0.0);
  Some(json!({
    "id": value_to_string(t.get("id")),
    "title": value_to_string(t.get("title")),
    "artist": artist,
    "duration": (ms / 1000.0).round() as i64,
    "thumbnail": thumbnail,
    "url": normalize_soundcloud_url(&value_to_string(t.get("permalink_url"))),
    "platform": "soundcloud",
    "isCached": false
  }))
}

fn is_preview_url(u: &str) -> bool {
  u.contains("/preview/") || u.contains("cf-preview-media")
}

fn transcoding_field<'a>(t: &'a Value, field: &str) -> &'a str {
  t.get("format")
    .and_then(|f| f.get(field))
    .and_then(Value::as_str)
    .unwrap_or("")
}

// Choisit le transcoding lisible : progressif, sinon HLS AAC (160k d'abord), sinon la playlist maîtresse.
// Les flux chiffrés et les HLS Opus/Ogg (non décodés) sont exclus, sans repli sur le premier venu.
pub(crate) fn pick_transcoding<'a>(list: &[&'a Value]) -> Option<&'a Value> {
  list
    .iter()
    .filter_map(|t| transcoding_rank(t).map(|rank| (rank, *t)))
    .min_by_key(|(rank, _)| *rank)
    .map(|(_, t)| t)
}

// Rang de préférence (0 = meilleur) ; `None` pour un flux inutilisable.
fn transcoding_rank(t: &Value) -> Option<u8> {
  let protocol = transcoding_field(t, "protocol").to_lowercase();
  let mime = transcoding_field(t, "mime_type").to_lowercase();
  if protocol.contains("encrypted")
    || mime.contains("encrypted")
    || mime.contains("opus")
    || mime.contains("ogg")
  {
    return None;
  }
  match protocol.as_str() {
    "progressive" => Some(0),
    "hls" if t.get("preset").and_then(Value::as_str) == Some("aac_160k") => Some(1),
    "hls" if mime.contains("audio/mp4") => Some(2),
    "hls" if mime.contains("audio/mpegurl") => Some(3),
    "hls" => Some(4),
    _ => None,
  }
}

// Source passée à la résolution directe : l'id numérique de la piste si on l'a (l'URL d'un résultat de
// recherche est un permalink, d'où l'on ne tire pas d'id), sinon l'URL, sinon l'id tel quel.
pub fn sc_stream_source<'a>(url: &'a str, id: &'a str) -> &'a str {
  let numeric = id.len() >= 6 && id.bytes().all(|b| b.is_ascii_digit());
  if numeric || url.is_empty() { id } else { url }
}

async fn resolve_sc_transcoding(client: &Http, track: &Value, cid: &str) -> Option<String> {
  let all = track.get("media")?.get("transcodings")?.as_array()?;
  let list: Vec<&Value> = all
    .iter()
    .filter(|t| !is_truthy(t.get("snipped")))
    .collect();
  let chosen = pick_transcoding(&list)?;
  let base = chosen
    .get("url")
    .and_then(Value::as_str)
    .filter(|s| !s.is_empty())?;
  let sep = if base.contains('?') { '&' } else { '?' };
  let auth = track
    .get("track_authorization")
    .and_then(Value::as_str)
    .filter(|s| !s.is_empty())
    .map(|a| format!("&track_authorization={}", encode_uri_component(a)))
    .unwrap_or_default();
  let resp = client
    .get(format!("{base}{sep}client_id={cid}{auth}"))
    .header(header::USER_AGENT, "Mozilla/5.0")
    .timeout(Duration::from_secs(8))
    .send()
    .await
    .ok()?;
  if !resp.status().is_success() {
    return None;
  }
  let data: Value = resp.json().await.ok()?;
  let url = data
    .get("url")
    .and_then(Value::as_str)
    .filter(|s| !s.is_empty())?;
  (!is_preview_url(url)).then(|| url.to_string())
}

// Flux SoundCloud direct : par id de piste si présent dans l'URL, sinon par recherche (`query_fallback`).
pub async fn fetch_direct_sc_stream(
  core: &Core,
  url_or_id: &str,
  query_fallback: &str,
) -> Option<String> {
  static RE_ID: OnceLock<Regex> = OnceLock::new();
  let re_id = RE_ID.get_or_init(|| Regex::new(r"(?i)(?:tracks/|^)(\d{6,})").expect("regex"));
  let client = &core.net.http_client;
  let mut cid = sc_client_id(core, None).await;
  let norm = normalize_soundcloud_url(url_or_id);
  let track_id = re_id.captures(&norm).map(|c| c[1].to_string());
  let get = |url: String| {
    client
      .get(url)
      .header(header::USER_AGENT, "Mozilla/5.0")
      .timeout(Duration::from_secs(8))
      .send()
  };
  for _ in 0..2 {
    if let Some(id) = &track_id {
      let Ok(r) = get(format!(
        "https://api-v2.soundcloud.com/tracks/{id}?client_id={cid}"
      ))
      .await
      else {
        continue;
      };
      if matches!(r.status().as_u16(), 401 | 403) {
        cid = sc_client_id(core, Some(&cid)).await;
        continue;
      }
      if r.status().is_success()
        && let Ok(track) = r.json::<Value>().await
        && let Some(u) = resolve_sc_transcoding(client, &track, &cid).await
      {
        return Some(u);
      }
    }
    if !query_fallback.is_empty() {
      let Ok(r) = get(format!(
        "https://api-v2.soundcloud.com/search/tracks?q={}&limit=5&client_id={cid}",
        encode_uri_component(query_fallback)
      ))
      .await
      else {
        continue;
      };
      if matches!(r.status().as_u16(), 401 | 403) {
        cid = sc_client_id(core, Some(&cid)).await;
        continue;
      }
      if r.status().is_success()
        && let Ok(data) = r.json::<Value>().await
        && let Some(col) = data.get("collection").and_then(Value::as_array)
      {
        let pick = col
          .iter()
          .find(|t| {
            t.get("policy").and_then(Value::as_str) != Some("SNIP")
              && t.get("duration").and_then(Value::as_f64).unwrap_or(0.0) > 30000.0
              && !is_truthy(t.get("snipped"))
          })
          .or(col.first());
        if let Some(t) = pick
          && let Some(u) = resolve_sc_transcoding(client, t, &cid).await
        {
          return Some(u);
        }
      }
    }
    break;
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;

  fn transcoding(preset: &str, protocol: &str, mime: &str) -> Value {
    json!({ "preset": preset, "format": { "protocol": protocol, "mime_type": mime } })
  }

  fn pick(list: &[Value]) -> Option<String> {
    let refs: Vec<&Value> = list.iter().collect();
    pick_transcoding(&refs)
      .and_then(|t| t.get("preset"))
      .and_then(Value::as_str)
      .map(str::to_string)
  }

  #[test]
  fn progressive_wins_then_aac_160k_then_other_hls() {
    let abr = transcoding("abr_sq", "hls", "audio/mpegurl");
    let aac96 = transcoding("aac_96k", "hls", "audio/mp4; codecs=\"mp4a.40.2\"");
    let aac160 = transcoding("aac_160k", "hls", "audio/mp4; codecs=\"mp4a.40.2\"");
    let prog = transcoding("mp3_1_0", "progressive", "audio/mpeg");
    assert_eq!(
      pick(&[abr.clone(), aac96.clone(), aac160.clone(), prog.clone()]).as_deref(),
      Some("mp3_1_0")
    );
    assert_eq!(
      pick(&[abr.clone(), aac96.clone(), aac160]).as_deref(),
      Some("aac_160k")
    );
    assert_eq!(pick(&[abr.clone(), aac96]).as_deref(), Some("aac_96k"));
    assert_eq!(pick(&[abr]).as_deref(), Some("abr_sq"));
  }

  #[test]
  fn encrypted_and_opus_streams_are_never_picked() {
    let encrypted = transcoding(
      "aac_160k",
      "hls",
      "audio/mp4; codecs=\"mp4a.40.2\"; encrypted",
    );
    let ctr = transcoding("aac_160k", "ctr-encrypted-hls", "audio/mp4");
    let opus = transcoding("opus_0_0", "hls", "audio/ogg; codecs=\"opus\"");
    assert_eq!(pick(&[encrypted, ctr, opus]), None);
    assert_eq!(pick(&[]), None);
  }

  #[test]
  fn stream_source_prefers_the_numeric_id_over_a_permalink() {
    let link = "https://soundcloud.com/artist/title";
    assert_eq!(sc_stream_source(link, "1234567"), "1234567");
    assert_eq!(sc_stream_source(link, "123"), link);
    assert_eq!(sc_stream_source(link, ""), link);
    assert_eq!(sc_stream_source("", "abc"), "abc");
    assert_eq!(sc_stream_source("", ""), "");
  }
}
