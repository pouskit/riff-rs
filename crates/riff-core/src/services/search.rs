use super::*;

const POOL_TTL_MS: u64 = 300_000;
const POOL_MAX_ENTRIES: usize = 35;

fn pool_get(core: &Core, key: &str) -> Option<Vec<Value>> {
  let mut guard = core.caches.search_cache.lock().ok()?;
  let pos = guard.iter().position(|(k, _)| k == key)?;
  if now_millis() < guard[pos].1.0 {
    return Some(guard[pos].1.1.clone());
  }
  guard.remove(pos);
  None
}

fn pool_put(core: &Core, key: String, pool: &[Value]) {
  if let Ok(mut guard) = core.caches.search_cache.lock() {
    guard.retain(|(k, _)| k != &key);
    if guard.len() >= POOL_MAX_ENTRIES {
      guard.remove(0);
    }
    guard.push((key, (now_millis() + POOL_TTL_MS, pool.to_vec())));
  }
}

// Filtre les compilations ("3 hours", "playlist of") et dédoublonne par id puis par titre/artiste normalisés.
fn clean_search_results(tracks: Vec<Value>) -> Vec<Value> {
  static RE_HOURS: OnceLock<Regex> = OnceLock::new();
  static RE_COMPIL: OnceLock<Regex> = OnceLock::new();
  let re_hours =
    RE_HOURS.get_or_init(|| Regex::new(r"(?i)\b\d+\s*(?:hours?|hrs?)\b").expect("regex"));
  let re_compil =
    RE_COMPIL.get_or_init(|| Regex::new(r"(?i)\b(?:playlist|compilation)\s+of\b").expect("regex"));
  let mut seen_ids: HashSet<String> = HashSet::new();
  let mut seen_keys: HashSet<String> = HashSet::new();
  tracks
    .into_iter()
    .filter(|track| {
      let Some(clean) = normalize_track_obj(track) else {
        return false;
      };
      let title = value_to_string(clean.get("title"));
      if re_hours.is_match(&title) || re_compil.is_match(&title) {
        return false;
      }
      let id = value_to_string(clean.get("id"));
      let id_key = if id.is_empty() {
        String::new()
      } else {
        format!("{}:{id}", value_to_string(clean.get("platform")))
      };
      let fingerprint = get_track_normalized_key(&value_to_string(clean.get("artist")), &title);
      if !id_key.is_empty() && seen_ids.contains(&id_key) {
        return false;
      }
      if !fingerprint.is_empty() && seen_keys.contains(&fingerprint) {
        return false;
      }
      if !id_key.is_empty() {
        seen_ids.insert(id_key);
      }
      if !fingerprint.is_empty() {
        seen_keys.insert(fingerprint);
      }
      true
    })
    .collect()
}

// Repli Deezer puis iTunes quand YouTube ne renvoie rien (métadonnées seules, pas de lecture directe).
// Rend aussi « une réponse HTTP est arrivée de Deezer ou d'iTunes » (faux = échec de transport partout).
async fn fetch_metadata_search_fallback_reached(
  core: &Core,
  query: &str,
  limit: usize,
) -> (Vec<Value>, bool) {
  let q = query.trim();
  if q.is_empty() {
    return (Vec::new(), true);
  }
  let client = &core.net.http_client;
  let (dz, dz_reached) = http_get_json_reached(
    client,
    &format!(
      "https://api.deezer.com/search?q={}&limit={limit}",
      encode_uri_component(q)
    ),
    5000,
    "Mozilla/5.0",
  )
  .await;
  if let Some(items) = dz
    .as_ref()
    .and_then(|d| d.get("data"))
    .and_then(Value::as_array)
    && !items.is_empty()
  {
    let tracks = items
      .iter()
      .filter(|it| is_truthy(it.get("title")))
      .map(|it| {
        let raw_artist = it
          .get("artist")
          .and_then(|a| a.get("name"))
          .and_then(Value::as_str);
        let art = strip_youtube_topic(
          raw_artist
            .filter(|s| !s.is_empty())
            .unwrap_or("Unknown Artist"),
        );
        let album = it.get("album");
        let cover = ["cover_xl", "cover_big", "cover_medium"]
          .iter()
          .find_map(|k| {
            album
              .and_then(|a| a.get(*k))
              .and_then(Value::as_str)
              .filter(|s| !s.is_empty())
          })
          .unwrap_or("");
        let title = value_to_string(it.get("title"));
        let duration = it
          .get("duration")
          .and_then(Value::as_f64)
          .filter(|d| *d != 0.0)
          .map_or(180, |d| d as i64);
        let explicit = is_truthy(it.get("explicit_lyrics"))
          || it.get("explicit_content_lyrics").and_then(Value::as_i64) == Some(1);
        json!({
          "id": format!("dz_{}", value_to_string(it.get("id"))),
          "title": title,
          "artist": art,
          "album": album.and_then(|a| a.get("title")).and_then(Value::as_str).unwrap_or(""),
          "duration": duration,
          "thumbnail": cover,
          "cover": cover,
          "url": "",
          "searchQuery": format!("{art} - {title}").trim(),
          "platform": "ytmusic",
          "isExplicit": explicit
        })
      })
      .collect();
    return (tracks, true);
  }
  let (it, it_reached) = http_get_json_reached(
    client,
    &format!(
      "https://itunes.apple.com/search?term={}&entity=song&limit={limit}",
      encode_uri_component(q)
    ),
    5000,
    "Mozilla/5.0",
  )
  .await;
  if let Some(items) = it
    .as_ref()
    .and_then(|d| d.get("results"))
    .and_then(Value::as_array)
  {
    let tracks = items
      .iter()
      .filter(|it| is_truthy(it.get("trackName")))
      .map(|it| {
        let art = it
          .get("artistName")
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
          .unwrap_or("Unknown Artist");
        let cover = value_to_string(it.get("artworkUrl100")).replace("100x100", "600x600");
        let title = value_to_string(it.get("trackName"));
        let ms = it
          .get("trackTimeMillis")
          .and_then(Value::as_f64)
          .filter(|d| *d != 0.0)
          .unwrap_or(180_000.0);
        json!({
          "id": format!("it_{}", value_to_string(it.get("trackId"))),
          "title": title,
          "artist": art,
          "album": value_to_string(it.get("collectionName")),
          "duration": (ms / 1000.0).round() as i64,
          "thumbnail": cover,
          "cover": cover,
          "url": "",
          "searchQuery": format!("{art} - {title}").trim(),
          "platform": "ytmusic",
          "isExplicit": it.get("trackExplicitness").and_then(Value::as_str) == Some("explicit")
        })
      })
      .collect();
    return (tracks, true);
  }
  (Vec::new(), dz_reached || it_reached)
}

// Recherche YouTube Music (WEB_REMIX, filtre "Songs"). `use_fallback` : recherche YouTube directe si la requête échoue.
pub async fn ytmusic_search(
  core: &Core,
  query: &str,
  timeout_ms: u64,
  max_retries: u32,
  use_fallback: bool,
) -> Vec<Value> {
  let client = &core.net.http_client;
  let Some(json_val) = ytm_search_request(
    client,
    query,
    Some(YTM_SONGS_PARAMS),
    timeout_ms,
    max_retries,
  )
  .await
  else {
    if use_fallback {
      return direct_youtube_search(client, query, true, 40).await;
    }
    return Vec::new();
  };
  parse_ytm_songs(&json_val)
    .into_iter()
    .map(|s| {
      let id = value_to_string(s.get("id"));
      let artist = strip_youtube_topic(s.get("artist").and_then(Value::as_str).unwrap_or(""));
      let duration = s
        .get("duration")
        .and_then(Value::as_i64)
        .filter(|d| *d != 0)
        .unwrap_or(180);
      let thumbnail = s
        .get("thumbnail")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"));
      json!({
        "id": id,
        "title": s.get("title").cloned().unwrap_or(Value::Null),
        "artist": artist,
        "album": s.get("album").cloned().unwrap_or(Value::Null),
        "duration": duration,
        "thumbnail": thumbnail,
        "url": format!("https://www.youtube.com/watch?v={id}"),
        "platform": "youtube",
        "isExplicit": is_truthy(s.get("isExplicit"))
      })
    })
    .collect()
}

// Rend le pool et « une réponse HTTP est arrivée au dernier maillon » (faux : pool vide à cause du réseau).
async fn build_pool(core: &Core, platform: &str, q: &str) -> (Vec<Value>, bool) {
  let client = &core.net.http_client;
  let mut reached = true;
  let raw = match platform {
    "soundcloud" => {
      let (tracks, got) = soundcloud::fetch_sc_search_reached(core, q, 40).await;
      reached = got;
      tracks
    }
    "youtube" => direct_youtube_search(client, q, false, 40).await,
    _ => {
      let ytm = ytmusic_search(core, q, 10_000, 2, true).await;
      if ytm.is_empty() {
        direct_youtube_search(client, q, false, 40).await
      } else {
        ytm
      }
    }
  };
  let mut pool = clean_search_results(raw);
  if pool.is_empty() && platform != "soundcloud" {
    (pool, reached) = fetch_metadata_search_fallback_reached(core, q, 30).await;
  }
  (pool, reached)
}

pub async fn search(core: &Arc<Core>, params: Params) -> CoreResult {
  let query = &params.query;
  let q = query.get("q").map(|s| s.trim()).unwrap_or("");
  let platform = query
    .get("platform")
    .map(String::as_str)
    .filter(|s| !s.is_empty())
    .unwrap_or("ytmusic");
  let parse_num = |key: &str, default: usize| {
    query
      .get(key)
      .and_then(|v| v.trim().parse::<i64>().ok())
      .map(|n| n.max(0) as usize)
      .unwrap_or(default)
  };
  let limit = match parse_num("limit", 12) {
    0 => 12,
    n => n,
  };
  let offset = parse_num("offset", 0);

  if q.is_empty() {
    return json_response(
      200,
      &json!({ "success": true, "tracks": [], "hasMore": false }),
    );
  }

  let key = format!("pool:{platform}:{}", q.to_lowercase());
  let pool = match pool_get(core, &key) {
    Some(p) => p,
    None => {
      let (p, reached) = build_pool(core, platform, q).await;
      if p.is_empty() && !reached && offset == 0 {
        return json_response(
          200,
          &json!({ "success": false, "error": "Offline: no internet connection", "offline": true }),
        );
      }
      if !p.is_empty() {
        pool_put(core, key, &p);
      }
      p
    }
  };

  let mut sliced: Vec<Value> = pool.iter().skip(offset).take(limit).cloned().collect();
  for t in &mut sliced {
    if t.get("isCached").is_none()
      && let Some(id) = t
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
      let cached = find_cached_file(id).is_some();
      if let Some(obj) = t.as_object_mut() {
        obj.insert("isCached".to_string(), Value::Bool(cached));
      }
    }
  }
  if offset == 0
    && let Some(first) = sliced.first()
    && let Some(url) = first
      .get("url")
      .and_then(Value::as_str)
      .filter(|u| !u.is_empty())
  {
    let id = value_to_string(first.get("id"));
    let plat = first
      .get("platform")
      .and_then(Value::as_str)
      .filter(|p| !p.is_empty())
      .unwrap_or("youtube");
    stream::spawn_prefetch(core, url.to_string(), id, plat.to_string());
  }
  json_response(
    200,
    &json!({ "success": true, "tracks": sliced, "hasMore": offset + limit < pool.len() }),
  )
}

pub const YTM_SONGS_PARAMS: &str = "EgWKAQIIAWoMEA4QChADEAQQCRAF";

pub const YTM_CLIENT_VERSION: &str = "1.20250101.01.00";

pub fn parse_ytm_songs(json_val: &Value) -> Vec<Value> {
  let mut out = Vec::new();
  let Some(tabs) = json_val
    .get("contents")
    .and_then(|c| c.get("tabbedSearchResultsRenderer"))
    .and_then(|t| t.get("tabs"))
    .and_then(Value::as_array)
  else {
    return out;
  };
  let Some(sections) = tabs
    .first()
    .and_then(|t| t.get("tabRenderer"))
    .and_then(|tr| tr.get("content"))
    .and_then(|c| c.get("sectionListRenderer"))
    .and_then(|s| s.get("contents"))
    .and_then(Value::as_array)
  else {
    return out;
  };

  static RE_DUR: OnceLock<Regex> = OnceLock::new();
  static RE_THUMB_SIZE: OnceLock<Regex> = OnceLock::new();
  let re_dur = RE_DUR.get_or_init(|| Regex::new(r"^\d+(:\d{1,2}){1,2}$").expect("regex"));
  let re_thumb = RE_THUMB_SIZE.get_or_init(|| Regex::new(r"=w\d+-h\d+[^&]*$").expect("regex"));

  for section in sections {
    let mut items: Vec<&Value> = Vec::new();
    for shelf_key in [
      "musicShelfRenderer",
      "itemSectionRenderer",
      "musicCardShelfRenderer",
    ] {
      if let Some(arr) = section
        .get(shelf_key)
        .and_then(|s| s.get("contents"))
        .and_then(Value::as_array)
      {
        items.extend(arr.iter());
      }
    }

    for wrap in items {
      let Some(r) = wrap.get("musicResponsiveListItemRenderer") else {
        continue;
      };
      let video_id = r
        .get("playlistItemData")
        .and_then(|p| p.get("videoId"))
        .and_then(Value::as_str)
        .or_else(|| {
          r.get("overlay")
            .and_then(|o| o.get("musicItemThumbnailOverlayRenderer"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.get("musicPlayButtonRenderer"))
            .and_then(|b| b.get("playNavigationEndpoint"))
            .and_then(|p| p.get("watchEndpoint"))
            .and_then(|w| w.get("videoId"))
            .and_then(Value::as_str)
        })
        .unwrap_or("");
      if video_id.len() != 11
        || !video_id
          .chars()
          .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
      {
        continue;
      }

      let cols = r
        .get("flexColumns")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
      let runs_of = |idx: usize| -> Vec<&str> {
        cols
          .get(idx)
          .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
          .and_then(|f| f.get("text"))
          .and_then(|t| t.get("runs"))
          .and_then(Value::as_array)
          .map(|arr| {
            arr
              .iter()
              .filter_map(|x| x.get("text").and_then(Value::as_str))
              .collect()
          })
          .unwrap_or_default()
      };

      let title = runs_of(0).join("").trim().to_string();
      let mut segments: Vec<String> = Vec::new();
      let mut cur = String::new();
      for txt in runs_of(1) {
        if txt == " • " {
          segments.push(cur.trim().to_string());
          cur.clear();
        } else {
          cur.push_str(txt);
        }
      }
      segments.push(cur.trim().to_string());
      let mut clean: Vec<String> = segments
        .into_iter()
        .filter(|s| {
          !s.is_empty() && !s.eq_ignore_ascii_case("song") && !s.eq_ignore_ascii_case("video")
        })
        .collect();

      let mut duration: i64 = 0;
      if let Some(last) = clean.last()
        && re_dur.is_match(last.trim())
      {
        let parsed = parse_duration_seconds(last);
        if parsed > 0 {
          duration = parsed;
          clean.pop();
        }
      }

      let raw_artist = clean.first().map(String::as_str).unwrap_or("");
      let artist = strip_youtube_topic(raw_artist).trim().to_string();
      let album = clean.get(1).cloned().unwrap_or_default();

      let thumbs = r
        .get("thumbnail")
        .and_then(|t| t.get("musicThumbnailRenderer"))
        .and_then(|m| m.get("thumbnail"))
        .and_then(|t| t.get("thumbnails"))
        .and_then(Value::as_array);
      let raw_thumb = thumbs
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));
      let thumbnail = re_thumb.replace(&raw_thumb, "=w544-h544").into_owned();

      let badge_explicit = r
        .get("badges")
        .and_then(Value::as_array)
        .map(|badges| {
          badges.iter().any(|b| {
            let Some(badge) = b.get("musicInlineBadgeRenderer") else {
              return false;
            };
            let icon_exp = badge
              .get("icon")
              .and_then(|i| i.get("iconType"))
              .and_then(Value::as_str)
              == Some("MUSIC_EXPLICIT_BADGE");
            let label = badge
              .get("accessibilityData")
              .and_then(|a| a.get("accessibilityData"))
              .and_then(|a| a.get("label"))
              .and_then(Value::as_str)
              .unwrap_or("")
              .to_lowercase();
            icon_exp
              || label.contains("explicit")
              || label.contains("нецензурн")
              || label.contains("18+")
          })
        })
        .unwrap_or(false);
      let is_exp = badge_explicit || is_text_explicit(&title) || is_text_explicit(&album);

      if title.is_empty() || (duration != 0 && duration < 30) {
        continue;
      }

      out.push(json!({
        "id": video_id,
        "title": title,
        "artist": if artist.is_empty() { "Unknown Artist".to_string() } else { artist },
        "album": album,
        "duration": duration,
        "thumbnail": thumbnail,
        "isExplicit": is_exp
      }));
    }
  }
  out
}

pub async fn ytm_search_request(
  client: &Http,
  query: &str,
  params: Option<&str>,
  timeout_ms: u64,
  max_retries: u32,
) -> Option<Value> {
  let mut payload = json!({
    "context": {
      "client": {
        "clientName": "WEB_REMIX",
        "clientVersion": YTM_CLIENT_VERSION,
        "hl": "en",
        "gl": "US"
      }
    },
    "query": query
  });
  if let Some(p) = params
    && let Some(obj) = payload.as_object_mut()
  {
    obj.insert("params".to_string(), Value::String(p.to_string()));
  }

  for _ in 0..=max_retries {
    if let Ok(resp) = client
      .post("https://music.youtube.com/youtubei/v1/search?prettyPrint=false")
      .header(header::CONTENT_TYPE, "application/json")
      .header(header::ORIGIN, "https://music.youtube.com")
      .header(header::REFERER, "https://music.youtube.com/")
      .header(
        header::USER_AGENT,
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
      )
      .timeout(Duration::from_millis(timeout_ms))
      .json(&payload)
      .send()
      .await
      && resp.status().is_success()
      && let Ok(val) = resp.json::<Value>().await
    {
      return Some(val);
    }
  }
  None
}

pub async fn direct_youtube_search(
  client: &Http,
  query: &str,
  is_music: bool,
  limit: usize,
) -> Vec<Value> {
  let payload = json!({
    "context": {
      "client": {
        "clientName": "WEB",
        "clientVersion": "2.20250101.00.00",
        "hl": "en",
        "gl": "US"
      }
    },
    "query": query
  });

  let Ok(resp) = client
    .post("https://www.youtube.com/youtubei/v1/search?prettyPrint=false")
    .header(header::CONTENT_TYPE, "application/json")
    .header(
      header::USER_AGENT,
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    )
    .timeout(Duration::from_millis(8000))
    .json(&payload)
    .send()
    .await
  else {
    return Vec::new();
  };
  let Ok(json_val) = resp.json::<Value>().await else {
    return Vec::new();
  };

  let contents = json_val
    .get("contents")
    .and_then(|c| {
      c.get("twoColumnSearchResultsRenderer")
        .and_then(|t| t.get("primaryContents"))
        .and_then(|p| p.get("sectionListRenderer"))
        .and_then(|s| s.get("contents"))
        .or_else(|| c.get("sectionListRenderer").and_then(|s| s.get("contents")))
    })
    .and_then(Value::as_array);
  let Some(sections) = contents else {
    return Vec::new();
  };

  let mut tracks = Vec::new();
  'search_loop: for section in sections {
    let Some(items) = section
      .get("itemSectionRenderer")
      .and_then(|i| i.get("contents"))
      .and_then(Value::as_array)
    else {
      continue;
    };
    for item in items {
      if tracks.len() >= limit {
        break 'search_loop;
      }
      let Some(v) = item.get("videoRenderer") else {
        continue;
      };
      let Some(vid_id) = v
        .get("videoId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
      else {
        continue;
      };
      let title = v
        .get("title")
        .and_then(|t| t.get("runs"))
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|r| r.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("untitled");
      let raw_artist = v
        .get("ownerText")
        .and_then(|o| o.get("runs"))
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|r| r.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown Artist");
      let artist = strip_youtube_topic(raw_artist);
      let dur_str = v
        .get("lengthText")
        .and_then(|l| l.get("simpleText"))
        .and_then(Value::as_str)
        .unwrap_or("0:00");
      let dur_sec = parse_duration_seconds(dur_str);
      let thumbnail = v
        .get("thumbnail")
        .and_then(|t| t.get("thumbnails"))
        .and_then(Value::as_array)
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("");

      if is_music && is_spam_or_unofficial(title, dur_sec, &artist) {
        continue;
      }

      let is_cached = find_cached_file(vid_id).is_some();
      let badge_exp = v
        .get("badges")
        .and_then(Value::as_array)
        .map(|arr| {
          arr.iter().any(|b| {
            let Some(mb) = b.get("metadataBadgeRenderer") else {
              return false;
            };
            let lbl = mb
              .get("label")
              .and_then(Value::as_str)
              .unwrap_or("")
              .to_lowercase();
            let style = mb
              .get("style")
              .and_then(Value::as_str)
              .unwrap_or("")
              .to_lowercase();
            let acc = mb
              .get("accessibilityData")
              .and_then(|a| a.get("label"))
              .and_then(Value::as_str)
              .unwrap_or("")
              .to_lowercase();
            lbl.contains("explicit")
              || lbl.contains("18+")
              || style.contains("explicit")
              || acc.contains("explicit")
          })
        })
        .unwrap_or(false);
      let is_exp = badge_exp || is_text_explicit(title);

      tracks.push(json!({
        "id": vid_id,
        "title": title,
        "artist": artist,
        "duration": dur_sec,
        "thumbnail": thumbnail,
        "url": format!("https://www.youtube.com/watch?v={vid_id}"),
        "platform": "youtube",
        "isCached": is_cached,
        "isExplicit": is_exp
      }));
    }
  }
  tracks
}
