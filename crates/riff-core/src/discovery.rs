//! Découverte : albums, tendances, recommandations et fiches d'artiste.

use crate::core::{Core, MAX_BODY_BYTES};
use crate::models::normalize_track_obj;
use crate::net::Http;
use crate::reply::{CoreResult, Params, json_response};
use crate::secrets;
use crate::services::search::{
  YTM_SONGS_PARAMS, direct_youtube_search, parse_ytm_songs, ytm_search_request,
};
use crate::util::{
  encode_uri_component, fetch_itunes_json, get_track_normalized_key, http_get_json,
  is_spam_or_unofficial, is_text_explicit, is_truthy, normalize_number_value, now_millis,
  random_cand_id, random_f64, shuffle_slice, strip_youtube_topic, upgrade_artwork_url,
  value_to_string,
};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub async fn album_search(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let q = query.get("q").map(|s| s.trim()).unwrap_or("");
  if q.is_empty() {
    return json_response(200, &json!({ "success": true, "albums": [] }));
  }

  let cache_key = format!("search:{}", q.to_lowercase());
  if let Some(cached) = core.get_album_cache(&cache_key) {
    return json_response(200, &cached);
  }

  static RE_APPLE_URL: OnceLock<Regex> = OnceLock::new();
  let re_apple = RE_APPLE_URL.get_or_init(|| {
    Regex::new(r"(?i)music\.apple\.com/(?:[a-z]{2}/)?album/(?:[^/]+/)?(\d+)").expect("valid regex")
  });

  if let Some(caps) = re_apple.captures(q)
    && let Some(id_match) = caps.get(1)
  {
    let lookup_url = format!(
      "https://itunes.apple.com/lookup?id={}&entity=album",
      id_match.as_str()
    );
    if let Some(data) = fetch_itunes_json(&core.net.http_client, &lookup_url).await
      && let Some(results) = data.get("results").and_then(Value::as_array)
      && let Some(item) = results
        .iter()
        .find(|r| r.get("wrapperType").and_then(Value::as_str) == Some("collection"))
    {
      let col_id = value_to_string(item.get("collectionId"));
      let title = item
        .get("collectionName")
        .and_then(Value::as_str)
        .unwrap_or("");
      let artist = item.get("artistName").and_then(Value::as_str).unwrap_or("");
      let year: String = item
        .get("releaseDate")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(4).collect())
        .unwrap_or_default();
      let track_count = item.get("trackCount").and_then(Value::as_i64).unwrap_or(0);
      let cover = item
        .get("artworkUrl100")
        .and_then(Value::as_str)
        .unwrap_or("")
        .replace("100x100", "600x600");

      let response_data = json!({
        "success": true,
        "albums": [{
          "id": col_id,
          "title": title,
          "artist": artist,
          "year": year,
          "trackCount": track_count,
          "cover": cover
        }]
      });
      core.set_album_cache(&cache_key, response_data.clone());
      return json_response(200, &response_data);
    }
  }

  static RE_DASH: OnceLock<Regex> = OnceLock::new();
  static RE_BY: OnceLock<Regex> = OnceLock::new();
  let re_dash = RE_DASH.get_or_init(|| Regex::new(r"\s+-\s+").expect("valid regex"));
  let re_by = RE_BY.get_or_init(|| Regex::new(r"(?i)\bby\b").expect("valid regex"));
  let step1 = re_dash.replace_all(q, " ");
  let step2 = re_by.replace_all(&step1, " ");
  let clean_q = step2.split_whitespace().collect::<Vec<_>>().join(" ");

  let enc_q = encode_uri_component(q);
  let mut urls = vec![
    format!("https://itunes.apple.com/search?term={enc_q}&entity=album&media=music&limit=25"),
    format!("https://itunes.apple.com/search?term={enc_q}&media=music&limit=25"),
  ];
  if !clean_q.is_empty() && clean_q.to_lowercase() != q.to_lowercase() {
    let enc_clean = encode_uri_component(&clean_q);
    urls.push(format!(
      "https://itunes.apple.com/search?term={enc_clean}&entity=album&media=music&limit=25"
    ));
    urls.push(format!(
      "https://itunes.apple.com/search?term={enc_clean}&media=music&limit=25"
    ));
  }

  let mut results: Vec<Value> = Vec::new();
  for u in &urls {
    if let Some(data) = fetch_itunes_json(&core.net.http_client, u).await
      && let Some(arr) = data.get("results").and_then(Value::as_array)
    {
      results.extend(arr.iter().cloned());
    }
  }

  static RE_SINGLE: OnceLock<Regex> = OnceLock::new();
  static RE_EP: OnceLock<Regex> = OnceLock::new();
  let re_single = RE_SINGLE.get_or_init(|| Regex::new(r"(?i)\s-\s*Single$").expect("valid regex"));
  let re_ep = RE_EP.get_or_init(|| Regex::new(r"(?i)\s-\s*EP$").expect("valid regex"));

  let normalize = |s: &str| -> String {
    s.to_lowercase()
      .chars()
      .filter(|c| c.is_alphanumeric())
      .collect()
  };

  let mut albums: Vec<Value> = Vec::new();
  let mut seen: HashSet<String> = HashSet::new();

  for item in &results {
    let Some(col_id_val) = item.get("collectionId").filter(|v| !v.is_null()) else {
      continue;
    };
    let name = item
      .get("collectionName")
      .and_then(Value::as_str)
      .unwrap_or("");
    if re_single.is_match(name) {
      continue;
    }
    let track_count = item.get("trackCount").and_then(Value::as_i64).unwrap_or(0);
    if track_count < 4 && !re_ep.is_match(name) {
      continue;
    }
    let artist_name = item.get("artistName").and_then(Value::as_str).unwrap_or("");
    let norm_key = format!("{}:{}", normalize(name), normalize(artist_name));
    if !seen.insert(norm_key) {
      continue;
    }
    let year: String = item
      .get("releaseDate")
      .and_then(Value::as_str)
      .map(|s| s.chars().take(4).collect())
      .unwrap_or_default();
    let cover = item
      .get("artworkUrl100")
      .and_then(Value::as_str)
      .unwrap_or("")
      .replace("100x100", "600x600");

    albums.push(json!({
      "id": value_to_string(Some(col_id_val)),
      "title": name,
      "artist": artist_name,
      "year": year,
      "trackCount": track_count,
      "cover": cover
    }));
  }

  let clean_q_lower = clean_q.to_lowercase();
  let words: Vec<String> = clean_q_lower
    .chars()
    .map(|c| {
      if c.is_alphanumeric() || c.is_whitespace() {
        c
      } else {
        ' '
      }
    })
    .collect::<String>()
    .split_whitespace()
    .filter(|w| w.encode_utf16().count() > 1)
    .map(str::to_string)
    .collect();

  let score_album = |album: &Value| -> i64 {
    let title = album
      .get("title")
      .and_then(Value::as_str)
      .unwrap_or("")
      .to_lowercase();
    let artist = album
      .get("artist")
      .and_then(Value::as_str)
      .unwrap_or("")
      .to_lowercase();
    let full = format!("{title} {artist}");
    let mut score: i64 = 0;
    for w in &words {
      if title.contains(w) {
        score += 15;
      } else if artist.contains(w) {
        score += 10;
      }
    }
    if title == clean_q_lower || full == clean_q_lower {
      score += 50;
    } else if full.contains(&clean_q_lower) || clean_q_lower.contains(&title) {
      score += 25;
    }
    score
  };

  albums.sort_by_key(|a| std::cmp::Reverse(score_album(a)));
  albums.truncate(12);

  let response_data = json!({
    "success": true,
    "albums": albums
  });
  core.set_album_cache(&cache_key, response_data.clone());
  json_response(200, &response_data)
}

pub fn map_itunes_song_tracks(results: &[Value], fallback_artist: &str) -> Vec<Value> {
  let mut tracks: Vec<(i64, i64, Value)> = results
    .iter()
    .filter(|r| {
      let wt = r.get("wrapperType").and_then(Value::as_str).unwrap_or("");
      let kind = r.get("kind").and_then(Value::as_str).unwrap_or("");
      wt == "track" || kind == "song"
    })
    .map(|item| {
      let track_id = value_to_string(item.get("trackId"));
      let no = item
        .get("trackNumber")
        .and_then(Value::as_i64)
        .filter(|&n| n != 0)
        .unwrap_or(1);
      let disc = item
        .get("discNumber")
        .and_then(Value::as_i64)
        .filter(|&n| n != 0)
        .unwrap_or(1);
      let title = item.get("trackName").and_then(Value::as_str).unwrap_or("");
      let artist = item
        .get("artistName")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_artist);
      let duration_ms = item
        .get("trackTimeMillis")
        .and_then(Value::as_i64)
        .unwrap_or(0);
      let duration = ((duration_ms as f64) / 1000.0).round() as i64;
      let thumbnail = item
        .get("artworkUrl100")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| s.replace("100x100", "600x600"))
        .unwrap_or_default();
      let explicit_flag = item.get("trackExplicitness").and_then(Value::as_str) == Some("explicit")
        || is_text_explicit(title);
      (
        disc,
        no,
        json!({
          "id": track_id,
          "no": no,
          "disc": disc,
          "title": title,
          "artist": artist,
          "duration": duration,
          "durationMs": duration_ms,
          "thumbnail": thumbnail,
          "isExplicit": explicit_flag
        }),
      )
    })
    .collect();
  tracks.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
  tracks.into_iter().map(|(_, _, v)| v).collect()
}

pub async fn album_tracks(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let id = query.get("id").map(|s| s.trim()).unwrap_or("");
  let title = query.get("title").map(|s| s.trim()).unwrap_or("");
  let artist = query.get("artist").map(|s| s.trim()).unwrap_or("");

  if id.is_empty() && title.is_empty() {
    return json_response(
      400,
      &json!({ "success": false, "error": "Invalid album id" }),
    );
  }

  let cache_key = if !id.is_empty() {
    format!("tracks:{id}")
  } else {
    format!("tracks:{artist}:{title}")
  };

  if let Some(cached) = core.get_album_cache(&cache_key) {
    return json_response(200, &cached);
  }

  let mut album: Option<Value> = None;
  let mut tracks: Vec<Value> = Vec::new();

  if !id.is_empty() {
    let url = format!(
      "https://itunes.apple.com/lookup?id={}&entity=song",
      encode_uri_component(id)
    );
    if let Some(data) = fetch_itunes_json(&core.net.http_client, &url).await
      && let Some(results) = data.get("results").and_then(Value::as_array)
    {
      let album_item = results.iter().find(|r| {
        r.get("wrapperType").and_then(Value::as_str) == Some("collection")
          || r.get("collectionType").and_then(Value::as_str) == Some("Album")
      });
      let song_tracks = map_itunes_song_tracks(results, artist);
      if !song_tracks.is_empty() {
        let alb_id = album_item
          .and_then(|a| a.get("collectionId"))
          .map(|v| value_to_string(Some(v)))
          .filter(|s| !s.is_empty())
          .unwrap_or_else(|| id.to_string());
        let alb_title = album_item
          .and_then(|a| a.get("collectionName"))
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
          .unwrap_or(title);
        let alb_artist = album_item
          .and_then(|a| a.get("artistName"))
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
          .unwrap_or(artist);
        let alb_year: String = album_item
          .and_then(|a| a.get("releaseDate"))
          .and_then(Value::as_str)
          .map(|s| s.chars().take(4).collect())
          .unwrap_or_default();
        let alb_track_count = album_item
          .and_then(|a| a.get("trackCount"))
          .and_then(Value::as_i64)
          .filter(|&n| n > 0)
          .unwrap_or(song_tracks.len() as i64);
        let alb_cover = album_item
          .and_then(|a| a.get("artworkUrl100"))
          .and_then(Value::as_str)
          .map(upgrade_artwork_url)
          .unwrap_or_default();

        tracks = song_tracks;
        album = Some(json!({
          "id": alb_id,
          "title": alb_title,
          "artist": alb_artist,
          "year": alb_year,
          "trackCount": alb_track_count,
          "cover": alb_cover
        }));
      }
    }
  }

  if tracks.is_empty() && !id.is_empty() {
    let dz_url = format!(
      "https://api.deezer.com/album/{}/tracks?limit=50",
      encode_uri_component(id)
    );
    if let Some(dz_data) = http_get_json(&core.net.http_client, &dz_url, 4500, "Mozilla/5.0").await
      && let Some(arr) = dz_data.get("data").and_then(Value::as_array)
      && !arr.is_empty()
    {
      let mut dz_tracks: Vec<(i64, i64, Value)> = arr
        .iter()
        .enumerate()
        .map(|(idx, item)| {
          let t_id = value_to_string(item.get("id"));
          let no = item
            .get("track_position")
            .and_then(Value::as_i64)
            .filter(|&n| n != 0)
            .unwrap_or((idx + 1) as i64);
          let disc = item
            .get("disk_number")
            .and_then(Value::as_i64)
            .filter(|&n| n != 0)
            .unwrap_or(1);
          let t_title = item.get("title").and_then(Value::as_str).unwrap_or("");
          let t_artist = item
            .get("artist")
            .and_then(|a| a.get("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(artist);
          let duration = item.get("duration").and_then(Value::as_i64).unwrap_or(0);
          let thumb = item
            .get("album")
            .and_then(|a| {
              a.get("cover_xl")
                .or_else(|| a.get("cover_big"))
                .or_else(|| a.get("cover_medium"))
            })
            .and_then(Value::as_str)
            .unwrap_or("");
          let is_exp = is_truthy(item.get("explicit_lyrics")) || is_text_explicit(t_title);
          (
            disc,
            no,
            json!({
              "id": t_id,
              "no": no,
              "disc": disc,
              "title": t_title,
              "artist": t_artist,
              "duration": duration,
              "durationMs": duration * 1000,
              "thumbnail": thumb,
              "isExplicit": is_exp
            }),
          )
        })
        .collect();
      dz_tracks.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
      tracks = dz_tracks.into_iter().map(|(_, _, v)| v).collect();

      let first_title = tracks
        .first()
        .and_then(|t| t.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("");
      let first_artist = tracks
        .first()
        .and_then(|t| t.get("artist"))
        .and_then(Value::as_str)
        .unwrap_or("");
      album = Some(json!({
        "id": id,
        "title": if !title.is_empty() { title } else { first_title },
        "artist": if !artist.is_empty() { artist } else { first_artist },
        "year": "",
        "trackCount": tracks.len(),
        "cover": ""
      }));
    }
  }

  if tracks.is_empty() && (!title.is_empty() || !artist.is_empty()) {
    let term = format!("{artist} {title}").trim().to_string();
    let search_url = format!(
      "https://itunes.apple.com/search?term={}&entity=song&limit=25",
      encode_uri_component(&term)
    );
    if let Some(search_data) = fetch_itunes_json(&core.net.http_client, &search_url).await
      && let Some(results) = search_data.get("results").and_then(Value::as_array)
    {
      let song_tracks = map_itunes_song_tracks(results, artist);
      if !song_tracks.is_empty() {
        let first = results.first();
        let alb_id = if !id.is_empty() {
          id.to_string()
        } else {
          first
            .and_then(|f| f.get("collectionId"))
            .map(|v| value_to_string(Some(v)))
            .unwrap_or_default()
        };
        let alb_title = if !title.is_empty() {
          title
        } else {
          first
            .and_then(|f| f.get("collectionName"))
            .and_then(Value::as_str)
            .unwrap_or("")
        };
        let alb_artist = if !artist.is_empty() {
          artist
        } else {
          first
            .and_then(|f| f.get("artistName"))
            .and_then(Value::as_str)
            .unwrap_or("")
        };
        let alb_year: String = first
          .and_then(|f| f.get("releaseDate"))
          .and_then(Value::as_str)
          .map(|s| s.chars().take(4).collect())
          .unwrap_or_default();
        let alb_cover = first
          .and_then(|f| f.get("artworkUrl100"))
          .and_then(Value::as_str)
          .map(upgrade_artwork_url)
          .unwrap_or_default();

        let count = song_tracks.len();
        tracks = song_tracks;
        album = Some(json!({
          "id": alb_id,
          "title": alb_title,
          "artist": alb_artist,
          "year": alb_year,
          "trackCount": count,
          "cover": alb_cover
        }));
      }
    }
  }

  if !tracks.is_empty() {
    let alb_val = album.unwrap_or_else(|| {
      json!({
        "id": id,
        "title": title,
        "artist": artist,
        "year": "",
        "trackCount": tracks.len(),
        "cover": ""
      })
    });
    let response_data = json!({
      "success": true,
      "album": alb_val,
      "tracks": tracks
    });
    core.set_album_cache(&cache_key, response_data.clone());
    return json_response(200, &response_data);
  }

  json_response(
    404,
    &json!({ "success": false, "error": "Failed to load album" }),
  )
}

pub fn resolve_candidate_obj(c: &Value) -> Option<Value> {
  let obj = c.as_object()?;
  let mut clean = Map::new();
  for (k, v) in obj {
    if matches!(
      k.as_str(),
      "match" | "source" | "seedWeight" | "playable" | "key" | "normArtist" | "isTop" | "rankScore"
    ) {
      continue;
    }
    clean.insert(k.clone(), v.clone());
  }
  let artist = clean.get("artist").and_then(Value::as_str).unwrap_or("");
  let title = clean.get("title").and_then(Value::as_str).unwrap_or("");
  let search_query = format!("{artist} - {title}").trim().to_string();

  let id = clean
    .get("id")
    .and_then(Value::as_str)
    .filter(|s| !s.is_empty())
    .map(str::to_string)
    .unwrap_or_else(random_cand_id);
  let platform_val = clean
    .get("platform")
    .and_then(Value::as_str)
    .filter(|s| !s.is_empty())
    .unwrap_or("youtube")
    .to_string();

  clean.insert("id".to_string(), Value::String(id));
  clean.insert("platform".to_string(), Value::String(platform_val));
  clean.insert("searchQuery".to_string(), Value::String(search_query));
  Some(Value::Object(clean))
}

pub async fn hot_picks(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let limit = query
    .get("limit")
    .and_then(|s| s.parse::<usize>().ok())
    .unwrap_or(12)
    .clamp(1, 25);

  let now = now_millis();
  if let Ok(guard) = core.caches.hot_picks_cache.lock()
    && let Some((expires, ref data)) = *guard
    && now < expires
  {
    let title = data.get("title").and_then(Value::as_str).unwrap_or("");
    let tracks: Vec<Value> = data
      .get("tracks")
      .and_then(Value::as_array)
      .map(|arr| arr.iter().take(limit).cloned().collect())
      .unwrap_or_default();
    return json_response(
      200,
      &json!({
        "success": true,
        "title": title,
        "tracks": tracks
      }),
    );
  }

  let Some(data) = fetch_itunes_json(
    &core.net.http_client,
    "https://api.deezer.com/chart/0/tracks?limit=40",
  )
  .await
  else {
    return json_response(200, &json!({ "success": false }));
  };

  let raw_items = data
    .get("data")
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default();

  let mut resolved_tracks: Vec<Value> = Vec::new();
  let mut seen_ids: HashSet<String> = HashSet::new();

  for item in &raw_items {
    if resolved_tracks.len() >= 12 {
      break;
    }
    let Some(t_title) = item
      .get("title")
      .and_then(Value::as_str)
      .filter(|s| !s.is_empty())
    else {
      continue;
    };
    let Some(t_artist) = item
      .get("artist")
      .and_then(|a| a.get("name"))
      .and_then(Value::as_str)
      .filter(|s| !s.is_empty())
    else {
      continue;
    };
    let duration = normalize_number_value(item.get("duration"));
    let is_exp = is_truthy(item.get("explicit_lyrics"))
      || item.get("explicit_content_lyrics").and_then(Value::as_i64) == Some(1)
      || is_text_explicit(t_title);

    let cand = json!({
      "artist": t_artist,
      "title": t_title,
      "duration": duration,
      "isExplicit": is_exp
    });
    if let Some(mut track) = resolve_candidate_obj(&cand) {
      let tid = track
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
      if !tid.is_empty() && seen_ids.insert(tid) {
        if is_exp && let Some(obj) = track.as_object_mut() {
          obj.insert("isExplicit".to_string(), Value::Bool(true));
        }
        resolved_tracks.push(track);
      }
    }
  }

  if resolved_tracks.is_empty() {
    return json_response(200, &json!({ "success": false }));
  }

  let year = jiff::ZonedDateTime::now().year();
  let title = format!("Hot picks {year}");
  let cache_val = json!({
    "title": title,
    "tracks": resolved_tracks
  });
  if let Ok(mut guard) = core.caches.hot_picks_cache.lock() {
    *guard = Some((now_millis() + 60 * 60 * 1000, cache_val));
  }

  json_response(
    200,
    &json!({
      "success": true,
      "title": title,
      "tracks": resolved_tracks
    }),
  )
}

pub fn normalize_artist_name(name: &str) -> String {
  static RE_WORDS: OnceLock<Regex> = OnceLock::new();
  let re = RE_WORDS
    .get_or_init(|| Regex::new(r"(?i)\b(official|vevo|records|topic)\b").expect("valid regex"));
  let stripped = re.replace_all(name, "");
  let cleaned: String = stripped
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || c == '_' || c.is_whitespace() || c == '&' || c == '-' {
        c
      } else {
        ' '
      }
    })
    .collect();
  cleaned
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase()
}

pub fn clean_title_for_matching(t: &str) -> String {
  static RE_PARENS: OnceLock<Regex> = OnceLock::new();
  let re = RE_PARENS.get_or_init(|| {
    Regex::new(r"(?i)\s*[\(\[][^\)\]]*(?:official|audio|video|lyrics|клип|трек|prod|feat|ft|slowed|reverb|remix)[^\)\]]*[\)\]]").expect("valid regex")
  });
  let lower = t.to_lowercase();
  let step1 = re.replace_all(&lower, "");
  let step2: String = step1
    .chars()
    .map(|c| {
      if matches!(
        c,
        '('
          | ')'
          | '['
          | ']'
          | '{'
          | '}'
          | '-'
          | '–'
          | '—'
          | '_'
          | '/'
          | '\\'
          | '"'
          | '\''
          | 'ᐸ'
          | '>'
          | '<'
          | ','
          | '.'
          | ':'
          | ';'
          | '!'
          | '?'
      ) {
        ' '
      } else {
        c
      }
    })
    .collect();
  step2.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub async fn search_ytm_tracks(core: &Core, query: &str, limit: usize) -> Vec<Value> {
  let cache_key = format!("ytm_vibe:{limit}:{}", query.trim().to_lowercase());
  let now = now_millis();
  if let Ok(mut guard) = core.caches.search_cache.lock()
    && let Some(pos) = guard.iter().position(|(k, _)| k == &cache_key)
  {
    let (_, (expires, ref tracks)) = guard[pos];
    if now < expires {
      return tracks.clone();
    }
    guard.remove(pos);
  }

  if let Some(json_val) = ytm_search_request(
    &core.net.http_client,
    query,
    Some(YTM_SONGS_PARAMS),
    10_000,
    1,
  )
  .await
  {
    let songs = parse_ytm_songs(&json_val);
    let tracks: Vec<Value> = songs
      .into_iter()
      .take(limit)
      .map(|s| {
        let id = s.get("id").and_then(Value::as_str).unwrap_or("");
        let title = s.get("title").and_then(Value::as_str).unwrap_or("");
        let artist = s.get("artist").and_then(Value::as_str).unwrap_or("");
        let duration = s
          .get("duration")
          .and_then(Value::as_i64)
          .filter(|&d| d > 0)
          .unwrap_or(180);
        let thumbnail = s
          .get("thumbnail")
          .and_then(Value::as_str)
          .filter(|t| !t.is_empty())
          .map(str::to_string)
          .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"));
        let is_exp = is_truthy(s.get("isExplicit"));
        json!({
          "id": id,
          "title": title,
          "artist": artist,
          "duration": duration,
          "thumbnail": thumbnail,
          "url": format!("https://www.youtube.com/watch?v={id}"),
          "platform": "youtube",
          "isExplicit": is_exp
        })
      })
      .collect();
    if !tracks.is_empty() {
      if let Ok(mut guard) = core.caches.search_cache.lock() {
        if guard.len() >= 35 {
          guard.remove(0);
        }
        guard.push((cache_key, (now_millis() + 300_000, tracks.clone())));
      }
      return tracks;
    }
  }

  let dy = direct_youtube_search(&core.net.http_client, query, true, limit).await;
  dy.into_iter().take(limit).collect()
}

pub async fn fetch_itunes_top_songs(client: &Http, limit: usize) -> Vec<Value> {
  let url = format!("https://itunes.apple.com/us/rss/topsongs/limit={limit}/json");
  let Some(json_val) = http_get_json(client, &url, 6000, "Mozilla/5.0").await else {
    return Vec::new();
  };
  let Some(entries) = json_val
    .get("feed")
    .and_then(|f| f.get("entry"))
    .and_then(Value::as_array)
  else {
    return Vec::new();
  };
  static RE_BB: OnceLock<Regex> = OnceLock::new();
  let re_bb = RE_BB.get_or_init(|| Regex::new(r"(?i)/\d+x\d+bb\.").expect("valid regex"));

  entries
    .iter()
    .enumerate()
    .filter_map(|(idx, e)| {
      let raw_img = e
        .get("im:image")
        .and_then(Value::as_array)
        .and_then(|arr| arr.last())
        .and_then(|img| img.get("label"))
        .and_then(Value::as_str)
        .unwrap_or("");
      let hi_img = re_bb.replace(raw_img, "/600x600bb.").into_owned();
      let id_attr = e
        .get("id")
        .and_then(|i| i.get("attributes"))
        .and_then(|a| a.get("im:id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| idx.to_string());
      let title = e
        .get("im:name")
        .and_then(|n| n.get("label"))
        .and_then(Value::as_str)
        .unwrap_or("Untitled");
      if title.is_empty() || title == "Untitled" {
        return None;
      }
      let artist = e
        .get("im:artist")
        .and_then(|a| a.get("label"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown artist");
      Some(json!({
        "id": format!("it-{id_attr}"),
        "title": title,
        "artist": artist,
        "thumbnail": hi_img,
        "cover": hi_img,
        "duration": 0,
        "platform": "ytmusic",
        "url": ""
      }))
    })
    .collect()
}

pub async fn trending(core: &Core) -> CoreResult {
  let now = now_millis();
  if let Ok(guard) = core.caches.trending_cache.lock()
    && !guard.1.is_empty()
    && now.saturating_sub(guard.0) < 2 * 3600 * 1000
  {
    return json_response(200, &json!({ "success": true, "tracks": guard.1 }));
  }

  let mut tracks = fetch_itunes_top_songs(&core.net.http_client, 24).await;

  if tracks.is_empty()
    && let Some(dz_chart) = http_get_json(
      &core.net.http_client,
      "https://api.deezer.com/chart/0/tracks?limit=24",
      5000,
      "Riff",
    )
    .await
    && let Some(entries) = dz_chart.get("data").and_then(Value::as_array)
  {
    tracks = entries
      .iter()
      .filter_map(|item| {
        let cover = item
          .get("album")
          .and_then(|a| {
            a.get("cover_xl")
              .or_else(|| a.get("cover_big"))
              .or_else(|| a.get("cover_medium"))
          })
          .and_then(Value::as_str)
          .unwrap_or("");
        let raw_id = value_to_string(item.get("id"));
        let id = if raw_id.is_empty() {
          String::new()
        } else {
          format!("dz-{raw_id}")
        };
        let title = item
          .get("title")
          .and_then(Value::as_str)
          .unwrap_or("Untitled");
        let artist = item
          .get("artist")
          .and_then(|a| a.get("name"))
          .and_then(Value::as_str)
          .unwrap_or("Unknown artist");
        if title.is_empty() || (id.is_empty() && artist.is_empty()) {
          return None;
        }
        let duration = normalize_number_value(item.get("duration"));
        Some(json!({
          "id": id,
          "title": title,
          "artist": artist,
          "thumbnail": cover,
          "cover": cover,
          "duration": duration,
          "platform": "ytmusic",
          "url": ""
        }))
      })
      .collect();
  }

  if tracks.is_empty() {
    let ytm_songs = search_ytm_tracks(core, "Top Hits 2026", 20).await;
    tracks = ytm_songs
      .into_iter()
      .filter_map(|item| {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("");
        let title = item
          .get("title")
          .and_then(Value::as_str)
          .unwrap_or("Untitled");
        let artist = item
          .get("artist")
          .and_then(Value::as_str)
          .unwrap_or("Unknown artist");
        if title.is_empty() || (id.is_empty() && artist.is_empty()) {
          return None;
        }
        let thumb = item.get("thumbnail").and_then(Value::as_str).unwrap_or("");
        let duration = normalize_number_value(item.get("duration"));
        let url = item
          .get("url")
          .and_then(Value::as_str)
          .filter(|u| !u.is_empty())
          .map(str::to_string)
          .unwrap_or_else(|| {
            if !id.is_empty() {
              format!("https://www.youtube.com/watch?v={id}")
            } else {
              String::new()
            }
          });
        Some(json!({
          "id": id,
          "title": title,
          "artist": artist,
          "thumbnail": thumb,
          "cover": thumb,
          "duration": duration,
          "platform": "ytmusic",
          "url": url
        }))
      })
      .collect();
  }

  if !tracks.is_empty()
    && let Ok(mut guard) = core.caches.trending_cache.lock()
  {
    *guard = (now_millis(), tracks.clone());
  }

  json_response(
    200,
    &json!({
      "success": true,
      "tracks": tracks
    }),
  )
}

pub fn has_cyrillic(s: &str) -> bool {
  s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

pub fn pick_weighted_seeds(seeds: &[Value], count: usize) -> Vec<Value> {
  let mut pool: Vec<Value> = seeds.to_vec();
  let mut picked: Vec<Value> = Vec::new();
  while picked.len() < count && !pool.is_empty() {
    let total_weight: f64 = pool
      .iter()
      .map(|s| {
        s.get("weight")
          .and_then(Value::as_f64)
          .filter(|&w| w.is_finite() && w != 0.0)
          .unwrap_or(1.0)
      })
      .sum();
    let mut r = random_f64() * total_weight;
    let mut selected_idx = 0;
    for (i, item) in pool.iter().enumerate() {
      let w = item
        .get("weight")
        .and_then(Value::as_f64)
        .filter(|&w| w.is_finite() && w != 0.0)
        .unwrap_or(1.0);
      r -= w;
      if r <= 0.0 {
        selected_idx = i;
        break;
      }
    }
    picked.push(pool.remove(selected_idx));
  }
  picked
}

pub async fn dz_artist_id(core: &Core, name: &str) -> Option<String> {
  let clean = normalize_artist_name(name);
  if clean.is_empty() {
    return None;
  }
  let now = now_millis();
  if let Ok(guard) = core.caches.dz_artist_id_cache.lock()
    && let Some((expires, id_opt)) = guard.get(&clean)
    && now < *expires
  {
    return id_opt.clone();
  }

  let url = format!(
    "https://api.deezer.com/search/artist?q={}&limit=1",
    encode_uri_component(name.trim())
  );
  let data = http_get_json(&core.net.http_client, &url, 6000, "Riff").await;
  let id = data
    .and_then(|d| {
      d.get("data")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("id"))
        .map(|v| value_to_string(Some(v)))
    })
    .filter(|s| !s.is_empty());

  if let Ok(mut guard) = core.caches.dz_artist_id_cache.lock() {
    guard.insert(clean, (now_millis() + 24 * 3600 * 1000, id.clone()));
  }
  id
}

pub async fn dz_related_tracks(core: &Core, artist: &str) -> Vec<Value> {
  let Some(id) = dz_artist_id(core, artist).await else {
    return Vec::new();
  };
  let rel_url = format!(
    "https://api.deezer.com/artist/{}/related?limit=25",
    encode_uri_component(&id)
  );
  let Some(rel_data) = http_get_json(&core.net.http_client, &rel_url, 6000, "Riff").await else {
    return Vec::new();
  };
  let mut related_list = rel_data
    .get("data")
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default();
  shuffle_slice(&mut related_list);
  related_list.truncate(6);
  if related_list.is_empty() {
    return Vec::new();
  }

  let mut out = Vec::new();
  for (related_index, rel_artist) in related_list.iter().enumerate() {
    let rel_id = value_to_string(rel_artist.get("id"));
    if rel_id.is_empty() {
      continue;
    }
    let top_url = format!(
      "https://api.deezer.com/artist/{}/top?limit=15",
      encode_uri_component(&rel_id)
    );
    let Some(top_data) = http_get_json(&core.net.http_client, &top_url, 6000, "Riff").await else {
      continue;
    };
    let mut top_tracks = top_data
      .get("data")
      .and_then(Value::as_array)
      .cloned()
      .unwrap_or_default();
    shuffle_slice(&mut top_tracks);
    top_tracks.truncate(4);
    let match_val = 1.0 - ((related_index as f64) / 12.0) * 0.6;
    let rel_name = rel_artist
      .get("name")
      .and_then(Value::as_str)
      .unwrap_or("unknown artist");
    for item in top_tracks {
      let item_artist = item
        .get("artist")
        .and_then(|a| a.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(rel_name);
      let item_title = item
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("untitled");
      out.push(json!({
        "artist": item_artist,
        "title": item_title,
        "match": match_val,
        "source": "deezer"
      }));
    }
  }
  out
}

pub async fn dz_radio_tracks(core: &Core, artist: &str) -> Vec<Value> {
  let Some(id) = dz_artist_id(core, artist).await else {
    return Vec::new();
  };
  let rad_url = format!(
    "https://api.deezer.com/artist/{}/radio?limit=40",
    encode_uri_component(&id)
  );
  let Some(rad_data) = http_get_json(&core.net.http_client, &rad_url, 6000, "Riff").await else {
    return Vec::new();
  };
  let mut items = rad_data
    .get("data")
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default();
  shuffle_slice(&mut items);
  items.truncate(25);
  items
    .into_iter()
    .map(|item| {
      let a = item
        .get("artist")
        .and_then(|ar| ar.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown artist");
      let t = item
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("untitled");
      json!({
        "artist": a,
        "title": t,
        "match": 0.7,
        "source": "deezer"
      })
    })
    .collect()
}

pub async fn yt_mix_tracks(core: &Core, seed: &Value) -> Vec<Value> {
  let raw_artist = seed.get("artist").and_then(Value::as_str).unwrap_or("");
  let raw_title = seed.get("title").and_then(Value::as_str).unwrap_or("");
  if raw_artist.is_empty() && raw_title.is_empty() {
    return Vec::new();
  }
  let clean_artist = strip_youtube_topic(raw_artist).trim().to_string();
  let clean_title = raw_title.trim().to_string();
  let cache_key = format!("{clean_artist}::{clean_title}").to_lowercase();
  if cache_key.is_empty() || cache_key == "::" {
    return Vec::new();
  }

  let now = now_millis();
  if let Ok(guard) = core.caches.yt_mix_cache.lock()
    && let Some((expires, tracks)) = guard.get(&cache_key)
    && now < *expires
  {
    return tracks.clone();
  }

  let q = if !clean_artist.is_empty() {
    format!("{clean_artist} {clean_title} mix")
  } else {
    clean_title.clone()
  };
  let seed_id = seed.get("id").and_then(Value::as_str).unwrap_or("");
  let songs = search_ytm_tracks(core, &q, 14).await;
  let mut results = Vec::new();
  for (index, entry) in songs.iter().enumerate() {
    let entry_id = entry.get("id").and_then(Value::as_str).unwrap_or("");
    if entry_id.is_empty() || entry_id == seed_id {
      continue;
    }
    let duration = entry.get("duration").and_then(Value::as_i64).unwrap_or(0);
    if duration > 480 {
      continue;
    }
    let artist = strip_youtube_topic(
      entry
        .get("artist")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown artist"),
    );
    let title = entry
      .get("title")
      .and_then(Value::as_str)
      .filter(|s| !s.is_empty())
      .unwrap_or("untitled");
    if is_spam_or_unofficial(title, duration, &artist) {
      continue;
    }
    let thumbnail = entry.get("thumbnail").and_then(Value::as_str).unwrap_or("");
    let match_val = 1.0 - ((index as f64) / 20.0) * 0.5;
    results.push(json!({
      "id": entry_id,
      "title": title,
      "artist": artist,
      "duration": duration,
      "thumbnail": thumbnail,
      "url": format!("https://www.youtube.com/watch?v={entry_id}"),
      "platform": "youtube",
      "isCached": false,
      "match": match_val,
      "source": "ytmix",
      "playable": true
    }));
  }

  if let Ok(mut guard) = core.caches.yt_mix_cache.lock() {
    guard.insert(cache_key, (now_millis() + 30 * 60 * 1000, results.clone()));
  }
  results
}

pub async fn gather_candidates(core: &Core, seeds: &[Value]) -> Vec<Value> {
  let mut out = Vec::new();
  for seed in seeds.iter().take(4) {
    let weight = seed
      .get("weight")
      .and_then(Value::as_f64)
      .filter(|&w| w.is_finite() && w != 0.0)
      .unwrap_or(1.0);
    let artist = seed.get("artist").and_then(Value::as_str).unwrap_or("");
    let (ytmix, dz_rel, dz_rad) = tokio::join!(
      yt_mix_tracks(core, seed),
      dz_related_tracks(core, artist),
      dz_radio_tracks(core, artist),
    );
    for mut t in ytmix {
      if let Some(obj) = t.as_object_mut() {
        let m = obj.get("match").and_then(Value::as_f64).unwrap_or(0.7);
        obj.insert("match".to_string(), json!(m * 1.3));
        obj.insert("seedWeight".to_string(), json!(weight));
      }
      out.push(t);
    }
    for mut t in dz_rel {
      if let Some(obj) = t.as_object_mut() {
        obj.insert("seedWeight".to_string(), json!(weight));
      }
      out.push(t);
    }
    for mut t in dz_rad {
      if let Some(obj) = t.as_object_mut() {
        obj.insert("seedWeight".to_string(), json!(weight));
      }
      out.push(t);
    }
  }
  out
}

pub fn rank_candidates(
  core: &Core,
  cands: &[Value],
  profile: &Value,
  heard_keys: &HashSet<String>,
  avoid_keys: &HashSet<String>,
  avoid_artists: &HashSet<String>,
  limit: usize,
) -> Vec<Value> {
  struct GroupedCand {
    key: String,
    title: String,
    norm_artist: String,
    score_sum: f64,
    best_cand: Value,
  }

  let mut grouped: Vec<GroupedCand> = Vec::new();
  let mut key_to_idx: HashMap<String, usize> = HashMap::new();

  for raw_c in cands {
    let mut c = raw_c.clone();
    let plat = c.get("platform").and_then(Value::as_str).unwrap_or("");
    let src = c.get("source").and_then(Value::as_str).unwrap_or("");
    if plat == "youtube" || src == "ytmix" || plat.is_empty() {
      let stripped = strip_youtube_topic(c.get("artist").and_then(Value::as_str).unwrap_or(""));
      if let Some(obj) = c.as_object_mut() {
        obj.insert("artist".to_string(), Value::String(stripped));
      }
    }
    let artist = c.get("artist").and_then(Value::as_str).unwrap_or("");
    let title = c.get("title").and_then(Value::as_str).unwrap_or("");
    let key = get_track_normalized_key(artist, title);
    if key.is_empty() {
      continue;
    }
    if heard_keys.contains(&key)
      || core.is_recently_recommended_key(&key)
      || avoid_keys.contains(&key)
    {
      continue;
    }
    let norm_artist = normalize_artist_name(artist);
    if avoid_artists.contains(&norm_artist) {
      continue;
    }

    let weight = c
      .get("seedWeight")
      .and_then(Value::as_f64)
      .filter(|&w| w != 0.0)
      .unwrap_or(1.0);
    let match_val = c
      .get("match")
      .and_then(Value::as_f64)
      .filter(|&m| m != 0.0)
      .unwrap_or(0.5);
    let score_val = match_val * weight;

    if let Some(&idx) = key_to_idx.get(&key) {
      let g = &mut grouped[idx];
      g.score_sum += score_val;
      if is_truthy(c.get("playable")) && !is_truthy(g.best_cand.get("playable")) {
        g.best_cand = c;
      }
    } else {
      let idx = grouped.len();
      key_to_idx.insert(key.clone(), idx);
      grouped.push(GroupedCand {
        key,
        title: title.to_string(),
        norm_artist,
        score_sum: score_val,
        best_cand: c,
      });
    }
  }

  let top_artist_names: Vec<String> = profile
    .get("topArtists")
    .and_then(Value::as_array)
    .map(|arr| {
      arr
        .iter()
        .filter_map(|a| {
          a.as_str()
            .or_else(|| a.get("name").and_then(Value::as_str))
            .map(normalize_artist_name)
        })
        .filter(|s| !s.is_empty())
        .collect()
    })
    .unwrap_or_default();

  let is_cyrillic_profile =
    profile.get("dominantScript").and_then(Value::as_str) == Some("Cyrillic");

  let mut scored_list: Vec<(f64, bool, String, Value)> = Vec::new();
  for entry in grouped {
    let mut score = entry.score_sum;
    let is_top = top_artist_names.iter().any(|a| {
      a == &entry.norm_artist || entry.norm_artist.contains(a) || a.contains(&entry.norm_artist)
    });
    score += if is_top { -0.4 } else { 0.6 };
    if is_cyrillic_profile && has_cyrillic(&entry.title) {
      score += 0.3;
    }
    score += random_f64() * 0.15;

    let mut cand = entry.best_cand;
    if let Some(obj) = cand.as_object_mut() {
      obj.insert("key".to_string(), Value::String(entry.key));
      obj.insert(
        "normArtist".to_string(),
        Value::String(entry.norm_artist.clone()),
      );
      obj.insert("isTop".to_string(), Value::Bool(is_top));
      obj.insert("rankScore".to_string(), json!(score));
    }
    scored_list.push((score, is_top, entry.norm_artist, cand));
  }

  scored_list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

  let target_count = limit.max(1) * 2;
  let max_top_count = ((target_count as f64) * 0.3).floor().max(1.0) as usize;
  let mut artist_counts: HashMap<String, usize> = HashMap::new();
  let mut top_count = 0usize;
  let mut result = Vec::new();

  for (_, is_top, norm_artist, item) in scored_list {
    let a_count = artist_counts.get(&norm_artist).copied().unwrap_or(0);
    if a_count >= 2 {
      continue;
    }
    if is_top && top_count >= max_top_count {
      continue;
    }
    artist_counts.insert(norm_artist, a_count + 1);
    if is_top {
      top_count += 1;
    }
    result.push(item);
    if result.len() >= target_count {
      break;
    }
  }

  result
}

pub fn build_discovery_queries(profile: &Value) -> Vec<String> {
  let mut artists: Vec<String> = profile
    .get("topArtists")
    .and_then(Value::as_array)
    .map(|arr| {
      arr
        .iter()
        .filter_map(|a| {
          a.as_str()
            .or_else(|| a.get("name").and_then(Value::as_str))
            .map(str::to_string)
        })
        .filter(|s| !s.is_empty())
        .collect()
    })
    .unwrap_or_default();
  shuffle_slice(&mut artists);

  let mut queries = Vec::new();
  if let Some(first) = artists.first() {
    queries.push(first.clone());
    if let Some(second) = artists.get(1) {
      queries.push(second.clone());
    }
    if profile.get("dominantScript").and_then(Value::as_str) == Some("Cyrillic") {
      queries.push(format!("{first} похожие песни"));
    }
  }
  if queries.is_empty() {
    queries.push("synthwave chill".to_string());
    queries.push("indie electronic".to_string());
  }
  queries
}

pub fn score_discovery_candidate(
  core: &Core,
  track: &Value,
  profile: &Value,
  heard_keys: &HashSet<String>,
  heard_ids: &HashSet<String>,
) -> Option<(f64, Value)> {
  let mut normalized = normalize_track_obj(track)?;
  let id = normalized
    .get("id")
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();
  let url = normalized
    .get("url")
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();
  if id.is_empty() || url.is_empty() {
    return None;
  }
  let artist = normalized
    .get("artist")
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();
  let title = normalized
    .get("title")
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();
  let duration = normalized
    .get("duration")
    .and_then(Value::as_i64)
    .unwrap_or(0);
  let key = get_track_normalized_key(&artist, &title);

  if heard_ids.contains(&id) || (!key.is_empty() && heard_keys.contains(&key)) {
    return None;
  }
  if core.is_recently_recommended(&id, &key) {
    return None;
  }
  if is_spam_or_unofficial(&title, duration, &artist) {
    return None;
  }

  let mut score = 1.0;
  if artist.to_lowercase().trim_end().ends_with("- topic") {
    score += 1.5;
  }
  if title.contains(" - ") {
    score += 1.0;
  }
  let cand_artist = normalize_artist_name(&artist);
  let artist_names: Vec<String> = profile
    .get("topArtists")
    .and_then(Value::as_array)
    .map(|arr| {
      arr
        .iter()
        .filter_map(|a| {
          a.as_str()
            .or_else(|| a.get("name").and_then(Value::as_str))
            .map(str::to_string)
        })
        .collect()
    })
    .unwrap_or_default();
  if artist_names
    .iter()
    .any(|a| !a.is_empty() && (cand_artist.contains(a) || a.contains(&cand_artist)))
  {
    score += 3.0;
  }

  let is_cyr = has_cyrillic(&title);
  let dom_cyr = profile.get("dominantScript").and_then(Value::as_str) == Some("Cyrillic");
  if dom_cyr && is_cyr {
    score += 2.0;
  } else if !dom_cyr && !is_cyr {
    score += 1.0;
  }

  let typical_dur = profile
    .get("typicalDuration")
    .and_then(Value::as_f64)
    .unwrap_or(0.0);
  if duration > 0 && typical_dur > 0.0 {
    let delta = ((duration as f64) - typical_dur).abs();
    score += (2.0 - (delta / 60.0)).max(0.0);
  }
  score += random_f64() * 1.5;

  if let Some(obj) = normalized.as_object_mut() {
    obj.insert("score".to_string(), json!(score));
  }
  Some((score, normalized))
}

pub async fn recommend_vibe_tracks(
  core: &Core,
  history_tracks: &[Value],
  favorite_tracks: &[Value],
  limit: usize,
  client_profile: Option<&Value>,
  extra: &Value,
) -> Value {
  let normalized_history: Vec<Value> = history_tracks
    .iter()
    .filter_map(normalize_track_obj)
    .collect();
  let normalized_favorites: Vec<Value> = favorite_tracks
    .iter()
    .filter_map(normalize_track_obj)
    .collect();
  let total_user_tracks = normalized_favorites.len() + normalized_history.len();
  let has_extra_seeds = extra
    .get("seeds")
    .and_then(Value::as_array)
    .map(|a| !a.is_empty())
    .unwrap_or(false);

  let mut heard_ids: HashSet<String> = HashSet::new();
  let mut heard_keys: HashSet<String> = HashSet::new();
  for track in normalized_history.iter().chain(normalized_favorites.iter()) {
    let id = track.get("id").and_then(Value::as_str).unwrap_or("");
    if !id.is_empty() {
      heard_ids.insert(id.to_string());
    }
    let artist = track.get("artist").and_then(Value::as_str).unwrap_or("");
    let title = track.get("title").and_then(Value::as_str).unwrap_or("");
    let key = get_track_normalized_key(artist, title);
    if !key.is_empty() {
      heard_keys.insert(key);
    }
  }

  let mut artist_counts: Vec<(String, usize)> = Vec::new();
  let mut cyrillic_count = 0usize;
  let mut durations: Vec<f64> = Vec::new();

  for t in normalized_favorites.iter().chain(normalized_history.iter()) {
    let artist = t.get("artist").and_then(Value::as_str).unwrap_or("").trim();
    if !artist.is_empty() {
      let a = normalize_artist_name(artist);
      if let Some(entry) = artist_counts.iter_mut().find(|(k, _)| k == &a) {
        entry.1 += 1;
      } else {
        artist_counts.push((a, 1));
      }
    }
    let title = t.get("title").and_then(Value::as_str).unwrap_or("");
    if has_cyrillic(title) {
      cyrillic_count += 1;
    }
    let dur = t.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
    if dur > 0.0 {
      durations.push(dur);
    }
  }

  artist_counts.sort_by_key(|a| std::cmp::Reverse(a.1));
  let sorted_artists: Vec<Value> = artist_counts
    .into_iter()
    .take(5)
    .map(|(a, _)| Value::String(a))
    .collect();

  let top_artists = client_profile
    .and_then(|cp| cp.get("topArtists"))
    .and_then(Value::as_array)
    .filter(|a| !a.is_empty())
    .cloned()
    .unwrap_or(sorted_artists);

  let dominant_script = client_profile
    .and_then(|cp| cp.get("dominantScript"))
    .and_then(Value::as_str)
    .filter(|s| !s.is_empty())
    .map(str::to_string)
    .unwrap_or_else(|| {
      if (cyrillic_count as f64) > (total_user_tracks as f64) * 0.35 {
        "Cyrillic".to_string()
      } else {
        "Latin".to_string()
      }
    });

  let typical_duration = client_profile
    .and_then(|cp| cp.get("avgDuration"))
    .and_then(Value::as_i64)
    .filter(|&d| d > 0)
    .unwrap_or_else(|| {
      if !durations.is_empty() {
        let sum: f64 = durations.iter().sum();
        (sum / (durations.len() as f64)).round() as i64
      } else {
        210
      }
    });

  let profile = json!({
    "topArtists": top_artists,
    "dominantScript": dominant_script,
    "typicalDuration": typical_duration,
    "trackCount": total_user_tracks
  });

  if total_user_tracks < 5 && !has_extra_seeds {
    return json!({
      "profile": profile,
      "tracks": []
    });
  }

  let candidate_seeds: Vec<Value> = if let Some(seeds_arr) = extra
    .get("seeds")
    .and_then(Value::as_array)
    .filter(|a| !a.is_empty())
  {
    seeds_arr
      .iter()
      .map(|s| {
        let weight = s
          .get("weight")
          .and_then(Value::as_f64)
          .filter(|&w| w.is_finite() && w != 0.0)
          .unwrap_or(1.0);
        json!({
          "artist": s.get("artist").and_then(Value::as_str).unwrap_or(""),
          "title": s.get("title").and_then(Value::as_str).unwrap_or(""),
          "id": s.get("id").and_then(Value::as_str).unwrap_or(""),
          "platform": s.get("platform").and_then(Value::as_str).filter(|p| !p.is_empty()).unwrap_or("youtube"),
          "weight": weight
        })
      })
      .collect()
  } else {
    let mut seed_list: Vec<Value> = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();
    for t in &normalized_favorites {
      let a = t.get("artist").and_then(Value::as_str).unwrap_or("");
      let ti = t.get("title").and_then(Value::as_str).unwrap_or("");
      let k = get_track_normalized_key(a, ti);
      if !k.is_empty() && seen_keys.insert(k) {
        seed_list.push(json!({
          "artist": a,
          "title": ti,
          "id": t.get("id").and_then(Value::as_str).unwrap_or(""),
          "platform": t.get("platform").and_then(Value::as_str).filter(|p| !p.is_empty()).unwrap_or("youtube"),
          "weight": 3.0
        }));
      }
    }
    for t in &normalized_history {
      let a = t.get("artist").and_then(Value::as_str).unwrap_or("");
      let ti = t.get("title").and_then(Value::as_str).unwrap_or("");
      let k = get_track_normalized_key(a, ti);
      if !k.is_empty() && seen_keys.insert(k) {
        seed_list.push(json!({
          "artist": a,
          "title": ti,
          "id": t.get("id").and_then(Value::as_str).unwrap_or(""),
          "platform": t.get("platform").and_then(Value::as_str).filter(|p| !p.is_empty()).unwrap_or("youtube"),
          "weight": 1.0
        }));
      }
    }
    seed_list.truncate(8);
    seed_list
  };

  let chosen_seeds = pick_weighted_seeds(&candidate_seeds, 4);

  let avoid_keys_arr = extra
    .get("avoid")
    .and_then(|a| a.get("keys"))
    .and_then(Value::as_array);
  let avoid_artists_arr = extra
    .get("avoid")
    .and_then(|a| a.get("artists"))
    .and_then(Value::as_array);

  let avoid_keys: HashSet<String> = avoid_keys_arr
    .map(|arr| {
      arr
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
    })
    .unwrap_or_default();
  let avoid_artists: HashSet<String> = avoid_artists_arr
    .map(|arr| {
      arr
        .iter()
        .filter_map(Value::as_str)
        .map(normalize_artist_name)
        .filter(|s| !s.is_empty())
        .collect()
    })
    .unwrap_or_default();

  let avoid_keys_len = avoid_keys_arr.map(Vec::len).unwrap_or(avoid_keys.len());
  let avoid_artists_len = avoid_artists_arr
    .map(Vec::len)
    .unwrap_or(avoid_artists.len());
  let avoid_hash = format!("{avoid_keys_len}:{avoid_artists_len}");

  let mut seed_keys: Vec<String> = chosen_seeds
    .iter()
    .map(|s| {
      let a = s.get("artist").and_then(Value::as_str).unwrap_or("");
      let t = s.get("title").and_then(Value::as_str).unwrap_or("");
      let k = get_track_normalized_key(a, t);
      if !k.is_empty() {
        k
      } else {
        s.get("id")
          .and_then(Value::as_str)
          .unwrap_or("")
          .to_string()
      }
    })
    .collect();
  seed_keys.sort();
  let sorted_seed_keys = seed_keys.join("|");
  let cache_key = if !sorted_seed_keys.is_empty() {
    format!("{sorted_seed_keys}#avoid:{avoid_hash}")
  } else {
    String::new()
  };

  if !is_truthy(extra.get("refresh"))
    && !cache_key.is_empty()
    && let Ok(guard) = core.caches.vibe_cache.lock()
    && let Some((expires, cached_tracks)) = guard.get(&cache_key)
    && now_millis() < *expires
  {
    let filtered: Vec<Value> = cached_tracks
      .iter()
      .filter(|t| {
        let tid = t.get("id").and_then(Value::as_str).unwrap_or("");
        let a = t.get("artist").and_then(Value::as_str).unwrap_or("");
        let ti = t.get("title").and_then(Value::as_str).unwrap_or("");
        let k = get_track_normalized_key(a, ti);
        !core.is_recently_recommended(tid, &k)
      })
      .cloned()
      .collect();
    if filtered.len() >= limit {
      return json!({
        "profile": profile,
        "tracks": filtered.into_iter().take(limit).collect::<Vec<_>>()
      });
    }
  }

  let mut final_tracks: Vec<Value> = Vec::new();
  let cands = gather_candidates(core, &chosen_seeds).await;
  let ranked = rank_candidates(
    core,
    &cands,
    &profile,
    &heard_keys,
    &avoid_keys,
    &avoid_artists,
    limit,
  );

  for c in &ranked {
    if final_tracks.len() >= limit {
      break;
    }
    if let Some(track) = resolve_candidate_obj(c) {
      let tid = track.get("id").and_then(Value::as_str).unwrap_or("");
      if final_tracks
        .iter()
        .any(|t| t.get("id").and_then(Value::as_str) == Some(tid))
      {
        continue;
      }
      let a = track.get("artist").and_then(Value::as_str).unwrap_or("");
      let ti = track.get("title").and_then(Value::as_str).unwrap_or("");
      let k = get_track_normalized_key(a, ti);
      if !heard_ids.contains(tid)
        && (k.is_empty() || !heard_keys.contains(&k))
        && !core.is_recently_recommended(tid, &k)
      {
        final_tracks.push(track);
      }
    }
  }

  if !final_tracks.is_empty() {
    for track in &final_tracks {
      let tid = track.get("id").and_then(Value::as_str).unwrap_or("");
      let a = track.get("artist").and_then(Value::as_str).unwrap_or("");
      let ti = track.get("title").and_then(Value::as_str).unwrap_or("");
      let n_key = get_track_normalized_key(a, ti);
      core.remember_recommended(tid, &n_key);
    }
    if !cache_key.is_empty()
      && let Ok(mut guard) = core.caches.vibe_cache.lock()
    {
      guard.insert(
        cache_key,
        (now_millis() + 20 * 60 * 1000, final_tracks.clone()),
      );
    }
    return json!({
      "profile": profile,
      "tracks": final_tracks
    });
  }

  let mut queries = if profile
    .get("topArtists")
    .and_then(Value::as_array)
    .map(|a| !a.is_empty())
    .unwrap_or(false)
  {
    build_discovery_queries(&profile)
  } else {
    Vec::new()
  };
  if queries.is_empty() {
    queries = vec![
      "Top Hits 2025".to_string(),
      "Trending Music".to_string(),
      "Pop Hits".to_string(),
    ];
  }

  let mut candidate_map: Vec<(String, f64, Value)> = Vec::new();
  let mut all_found: Vec<Value> = Vec::new();

  for q in queries.iter().take(3) {
    let results = search_ytm_tracks(core, q, 16).await;
    for track in results {
      all_found.push(track.clone());
      if let Some((score, scored)) =
        score_discovery_candidate(core, &track, &profile, &heard_keys, &heard_ids)
      {
        let sid = scored
          .get("id")
          .and_then(Value::as_str)
          .unwrap_or("")
          .to_string();
        if let Some(existing) = candidate_map.iter_mut().find(|(id, _, _)| id == &sid) {
          if existing.1 < score {
            existing.1 = score;
            existing.2 = scored;
          }
        } else {
          candidate_map.push((sid, score, scored));
        }
      }
    }
  }

  if candidate_map.len() < limit {
    for track in &all_found {
      let tid = track
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
      if !tid.is_empty() && !candidate_map.iter().any(|(id, _, _)| id == &tid) {
        let mut with_score = track.clone();
        if let Some(obj) = with_score.as_object_mut() {
          obj.insert("score".to_string(), json!(50));
        }
        candidate_map.push((tid, 50.0, with_score));
      }
      if candidate_map.len() >= limit * 2 {
        break;
      }
    }
  }

  candidate_map.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
  let mut ranked_list: Vec<Value> = candidate_map.into_iter().map(|(_, _, v)| v).collect();

  let unshown: Vec<Value> = ranked_list
    .iter()
    .filter(|t| {
      let tid = t.get("id").and_then(Value::as_str).unwrap_or("");
      !core.is_recently_recommended_id(tid)
    })
    .cloned()
    .collect();
  if unshown.len() >= limit {
    ranked_list = unshown;
  }

  let mut artist_track_count: HashMap<String, usize> = HashMap::new();
  let mut fallback_tracks: Vec<Value> = Vec::new();
  for track in ranked_list {
    let a = normalize_artist_name(track.get("artist").and_then(Value::as_str).unwrap_or(""));
    let count = artist_track_count.get(&a).copied().unwrap_or(0);
    if count < 3 {
      artist_track_count.insert(a, count + 1);
      let tid = track
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
      let ar = track
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
      let ti = track
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
      let n_key = get_track_normalized_key(&ar, &ti);
      core.remember_recommended(&tid, &n_key);
      fallback_tracks.push(track);
      if fallback_tracks.len() >= limit {
        break;
      }
    }
  }

  if fallback_tracks.len() < limit {
    for track in all_found {
      let tid = track.get("id").and_then(Value::as_str).unwrap_or("");
      if !fallback_tracks
        .iter()
        .any(|t| t.get("id").and_then(Value::as_str) == Some(tid))
      {
        fallback_tracks.push(track);
        if fallback_tracks.len() >= limit {
          break;
        }
      }
    }
  }

  json!({
    "profile": profile,
    "tracks": fallback_tracks
  })
}

pub async fn vibe_recommendations(core: &Core, params: Params) -> CoreResult {
  let body = &params.body;
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let text = std::str::from_utf8(body).unwrap_or("");
  let payload = serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({}));

  let history_tracks = payload
    .get("history")
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default();
  let favorite_tracks = payload
    .get("favorites")
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default();
  let raw_limit = payload
    .get("limit")
    .and_then(|v| {
      v.as_u64()
        .map(|n| n as usize)
        .or_else(|| v.as_str().and_then(|s| s.parse::<usize>().ok()))
    })
    .unwrap_or(10);
  let limit = raw_limit.clamp(4, 16);
  let client_profile = payload.get("clientProfile");
  let extra = json!({
    "seeds": payload.get("seeds").cloned().unwrap_or(Value::Null),
    "avoid": payload.get("avoid").cloned().unwrap_or(Value::Null),
    "refresh": payload.get("refresh").cloned().unwrap_or(Value::Null)
  });

  let mut result = recommend_vibe_tracks(
    core,
    &history_tracks,
    &favorite_tracks,
    limit,
    client_profile,
    &extra,
  )
  .await;

  if let Some(tracks) = result.get_mut("tracks").and_then(Value::as_array_mut) {
    for c in tracks {
      let plat = c.get("platform").and_then(Value::as_str).unwrap_or("");
      if plat == "youtube" || plat.is_empty() {
        let stripped = strip_youtube_topic(c.get("artist").and_then(Value::as_str).unwrap_or(""));
        if let Some(obj) = c.as_object_mut() {
          obj.insert("artist".to_string(), Value::String(stripped));
        }
      }
    }
  }

  let profile = result.get("profile").cloned().unwrap_or_else(|| json!({}));
  let tracks = result.get("tracks").cloned().unwrap_or_else(|| json!([]));
  json_response(
    200,
    &json!({
      "success": true,
      "profile": profile,
      "tracks": tracks
    }),
  )
}

pub async fn fetch_music_bio(client: &Http, artist_name: &str) -> String {
  let clean = artist_name.trim();
  if clean.is_empty() {
    return String::new();
  }

  let lfm_url = format!(
    "https://ws.audioscrobbler.com/2.0/?method=artist.getinfo&artist={}&api_key={}&format=json",
    encode_uri_component(clean),
    secrets::lastfm_api_key()
  );
  if let Some(lfm_data) = http_get_json(client, &lfm_url, 3500, "Riff").await {
    let bio_summary = lfm_data
      .get("artist")
      .and_then(|a| a.get("bio"))
      .and_then(|b| {
        b.get("summary")
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
          .or_else(|| b.get("content").and_then(Value::as_str))
      })
      .unwrap_or("");
    if !bio_summary.is_empty() {
      static RE_TAGS: OnceLock<Regex> = OnceLock::new();
      static RE_READ_MORE: OnceLock<Regex> = OnceLock::new();
      let re_tags = RE_TAGS.get_or_init(|| Regex::new(r"<[^>]*>").expect("regex"));
      let re_more =
        RE_READ_MORE.get_or_init(|| Regex::new(r"(?is)\s*Read more on Last\.fm.*").expect("regex"));
      let s1 = re_tags.replace_all(bio_summary, "");
      let s2 = re_more.replace_all(&s1, "");
      let clean_bio = s2.trim();
      if clean_bio.chars().count() > 20 {
        return clean_bio.to_string();
      }
    }
  }

  let clean_lower = clean.to_lowercase();
  let is_ru = has_cyrillic(clean) || clean.eq_ignore_ascii_case("FACE");
  let wiki_host = if is_ru {
    "ru.wikipedia.org"
  } else {
    "en.wikipedia.org"
  };
  let query_term = if is_ru {
    format!("{clean} (рэпер OR музыкант OR певец OR группа)")
  } else {
    format!("{clean} (musician OR rapper OR singer OR band OR music)")
  };

  let search_url = format!(
    "https://{wiki_host}/w/api.php?action=query&list=search&srsearch={}&utf8=&format=json",
    encode_uri_component(&query_term)
  );
  if let Some(search_data) = http_get_json(client, &search_url, 3500, "Riff").await
    && let Some(hits) = search_data
      .get("query")
      .and_then(|q| q.get("search"))
      .and_then(Value::as_array)
  {
    static RE_SKIP: OnceLock<Regex> = OnceLock::new();
    static RE_BAD_BIO: OnceLock<Regex> = OnceLock::new();
    let re_skip =
      RE_SKIP.get_or_init(|| Regex::new(r"(?i)list of |discography|may refer to").expect("regex"));
    let re_bad = RE_BAD_BIO.get_or_init(|| {
      Regex::new(r"(?i)ammunition|cartridge|human face|anatomy|organism").expect("regex")
    });

    for hit in hits.iter().take(3) {
      let Some(hit_title) = hit
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
      else {
        continue;
      };
      let title_lower = hit_title.to_lowercase();
      if !title_lower.starts_with(&clean_lower) || re_skip.is_match(&title_lower) {
        continue;
      }
      let summary_url = format!(
        "https://{wiki_host}/api/rest_v1/page/summary/{}",
        encode_uri_component(hit_title)
      );
      if let Some(sum_data) = http_get_json(client, &summary_url, 3500, "Riff").await
        && let Some(ext) = sum_data
          .get("extract")
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
      {
        let sum_title = sum_data
          .get("title")
          .and_then(Value::as_str)
          .unwrap_or("")
          .to_lowercase();
        if !sum_title.starts_with(&clean_lower) {
          continue;
        }
        if re_skip.is_match(&sum_title) || re_skip.is_match(ext) || re_bad.is_match(ext) {
          continue;
        }
        let ext_lower = ext.to_lowercase();
        let word_match = clean_lower
          .split_whitespace()
          .any(|w| w.chars().count() >= 3 && ext_lower.contains(w));
        if ext_lower.contains(&clean_lower) || word_match {
          return ext.to_string();
        }
      }
    }
  }

  String::new()
}

pub fn format_thousands(n: u64) -> String {
  let s = n.to_string();
  let mut out = String::with_capacity(s.len() + s.len() / 3);
  for (i, ch) in s.chars().rev().enumerate() {
    if i > 0 && i % 3 == 0 {
      out.push(',');
    }
    out.push(ch);
  }
  out.chars().rev().collect()
}

pub async fn fetch_artist_info(core: &Core, artist_name: &str, hint_track: &str) -> Value {
  static RE_CLEAN_ARTIST: OnceLock<Regex> = OnceLock::new();
  let re_clean = RE_CLEAN_ARTIST
    .get_or_init(|| Regex::new(r"(?i) - Topic|VEVO|Official|Records").expect("valid regex"));
  let clean_name = re_clean.replace_all(artist_name, "").trim().to_string();
  if clean_name.is_empty() {
    return json!({
      "name": "",
      "bio": "",
      "avatar": "",
      "banner": "",
      "albums": [],
      "isVerified": false,
      "isKnownArtist": false,
      "fans": 0,
      "monthlyListeners": "",
      "tracks": []
    });
  }

  let norm_clean = clean_name.to_lowercase().trim().to_string();
  let cache_key = format!("{clean_name}::{hint_track}").to_lowercase();
  if let Ok(guard) = core.caches.artist_cache.lock()
    && let Some((_, cached)) = guard.iter().find(|(k, _)| k == &cache_key)
  {
    return cached.clone();
  }

  let mut avatar = String::new();
  let mut banner = String::new();
  let mut is_verified = false;
  let mut is_known_artist = false;
  let mut fans_count: u64 = 0;
  let mut monthly_listeners = String::new();
  let mut tracks: Vec<Value> = Vec::new();
  let mut albums: Vec<Value> = Vec::new();
  let mut resolved_name = clean_name.clone();

  let mut artist_match: Option<Value> = None;
  let dz_search_url = format!(
    "https://api.deezer.com/search/artist?q={}&limit=10",
    encode_uri_component(&clean_name)
  );
  if let Some(dz_res) =
    http_get_json(&core.net.http_client, &dz_search_url, 4500, "Mozilla/5.0").await
    && let Some(candidates) = dz_res.get("data").and_then(Value::as_array)
  {
    let matching: Vec<&Value> = candidates
      .iter()
      .filter(|c| {
        c.get("name")
          .and_then(Value::as_str)
          .map(|n| n.trim().to_lowercase() == norm_clean)
          .unwrap_or(false)
      })
      .collect();

    if matching.len() == 1 {
      artist_match = Some(matching[0].clone());
    } else if matching.len() > 1 {
      if !hint_track.is_empty() {
        let clean_hint = clean_title_for_matching(hint_track);
        for cand in &matching {
          let cid = value_to_string(cand.get("id"));
          if cid.is_empty() {
            continue;
          }
          let top_url = format!("https://api.deezer.com/artist/{cid}/top?limit=25");
          if let Some(cand_top) =
            http_get_json(&core.net.http_client, &top_url, 4000, "Mozilla/5.0").await
            && let Some(top_list) = cand_top.get("data").and_then(Value::as_array)
          {
            let has_match = top_list.iter().any(|t| {
              let c_tit =
                clean_title_for_matching(t.get("title").and_then(Value::as_str).unwrap_or(""));
              !c_tit.is_empty()
                && !clean_hint.is_empty()
                && (c_tit.contains(&clean_hint) || clean_hint.contains(&c_tit))
            });
            if has_match {
              artist_match = Some((*cand).clone());
              break;
            }
          }
        }
      }
      if artist_match.is_none() {
        artist_match = Some(matching[0].clone());
      }
    } else if let Some(first) = candidates.first()
      && first
        .get("name")
        .and_then(Value::as_str)
        .map(|n| n.trim().to_lowercase().contains(&norm_clean))
        .unwrap_or(false)
    {
      artist_match = Some(first.clone());
    }
  }

  if let Some(ref am) = artist_match
    && let Some(am_id_val) = am.get("id")
    && !am_id_val.is_null()
  {
    let am_id = value_to_string(Some(am_id_val));
    resolved_name = am
      .get("name")
      .and_then(Value::as_str)
      .filter(|s| !s.is_empty())
      .unwrap_or(&clean_name)
      .to_string();
    avatar = am
      .get("picture_xl")
      .or_else(|| am.get("picture_big"))
      .or_else(|| am.get("picture_medium"))
      .and_then(Value::as_str)
      .unwrap_or("")
      .to_string();
    banner = avatar.clone();
    fans_count = am.get("nb_fan").and_then(Value::as_u64).unwrap_or(0);
    is_known_artist = true;
    is_verified = fans_count > 30_000;

    let top_url = format!("https://api.deezer.com/artist/{am_id}/top?limit=100");
    if let Some(top_data) =
      http_get_json(&core.net.http_client, &top_url, 5000, "Mozilla/5.0").await
      && let Some(arr) = top_data.get("data").and_then(Value::as_array)
    {
      for item in arr {
        let primary_matches = item
          .get("artist")
          .and_then(|a| a.get("id"))
          .map(|v| value_to_string(Some(v)) == am_id)
          .unwrap_or(false);
        let contrib_matches = item
          .get("contributors")
          .and_then(Value::as_array)
          .map(|cs| cs.iter().any(|c| value_to_string(c.get("id")) == am_id))
          .unwrap_or(false);
        if !primary_matches && !contrib_matches {
          continue;
        }
        let dur = item
          .get("duration")
          .and_then(Value::as_i64)
          .filter(|&d| d > 0)
          .unwrap_or(180);
        let cover = item
          .get("album")
          .and_then(|a| {
            a.get("cover_xl")
              .or_else(|| a.get("cover_big"))
              .or_else(|| a.get("cover_medium"))
          })
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
          .unwrap_or(&avatar);
        let item_id = value_to_string(item.get("id"));
        let item_title = item
          .get("title")
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
          .unwrap_or("untitled");
        let item_artist = item
          .get("artist")
          .and_then(|a| a.get("name"))
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
          .unwrap_or(&resolved_name);
        let sq = format!("{item_artist} - {item_title} official audio");
        let is_exp = is_truthy(item.get("explicit_lyrics"))
          || is_truthy(item.get("explicit_content_lyrics"))
          || is_text_explicit(item_title);

        tracks.push(json!({
          "id": format!("dz_{item_id}"),
          "title": item_title,
          "artist": item_artist,
          "duration": dur,
          "durationMs": dur * 1000,
          "thumbnail": cover,
          "url": format!("https://www.youtube.com/results?search_query={}", encode_uri_component(&sq)),
          "searchQuery": sq,
          "platform": "youtube",
          "isCached": false,
          "isExplicit": is_exp
        }));
      }
    }

    let alb_url = format!("https://api.deezer.com/artist/{am_id}/albums?limit=40");
    if let Some(alb_data) =
      http_get_json(&core.net.http_client, &alb_url, 4500, "Mozilla/5.0").await
      && let Some(arr) = alb_data.get("data").and_then(Value::as_array)
    {
      let mut existing_titles: HashSet<String> = tracks
        .iter()
        .map(|t| clean_title_for_matching(t.get("title").and_then(Value::as_str).unwrap_or("")))
        .collect();

      for a in arr {
        let Some(a_title) = a
          .get("title")
          .and_then(Value::as_str)
          .filter(|s| !s.is_empty())
        else {
          continue;
        };
        let a_artist_id = a
          .get("artist")
          .and_then(|ar| ar.get("id"))
          .map(|v| value_to_string(Some(v)))
          .unwrap_or_default();
        let a_artist_name = a
          .get("artist")
          .and_then(|ar| ar.get("name"))
          .and_then(Value::as_str)
          .unwrap_or(&resolved_name);
        if a_artist_id != am_id && a_artist_name.trim().to_lowercase() != norm_clean {
          continue;
        }
        let nb_tracks = a.get("nb_tracks").and_then(Value::as_i64).unwrap_or(0);
        let record_type = a
          .get("record_type")
          .and_then(Value::as_str)
          .unwrap_or("album");
        let is_single = nb_tracks == 1 || (record_type == "single" && nb_tracks <= 1);
        let a_id = value_to_string(a.get("id"));
        let raw_cover = a
          .get("cover_xl")
          .or_else(|| a.get("cover_big"))
          .or_else(|| a.get("cover_medium"))
          .and_then(Value::as_str)
          .unwrap_or("");

        if is_single {
          let c_title = clean_title_for_matching(a_title);
          if !c_title.is_empty() && existing_titles.insert(c_title) {
            let thumb = upgrade_artwork_url(if raw_cover.is_empty() {
              &avatar
            } else {
              raw_cover
            });
            tracks.push(json!({
              "id": format!("dz_s_{a_id}"),
              "title": a_title,
              "artist": resolved_name,
              "duration": 180,
              "durationMs": 180000,
              "thumbnail": thumb,
              "url": "",
              "searchQuery": format!("{resolved_name} - {a_title}"),
              "platform": "ytmusic",
              "isCached": false,
              "isExplicit": is_truthy(a.get("explicit_lyrics")) || is_text_explicit(a_title)
            }));
          }
        } else {
          let year: String = a
            .get("release_date")
            .and_then(Value::as_str)
            .map(|s| s.chars().take(4).collect())
            .unwrap_or_default();
          albums.push(json!({
            "id": a_id,
            "title": a_title,
            "artist": resolved_name,
            "cover": upgrade_artwork_url(raw_cover),
            "year": year,
            "trackCount": nb_tracks,
            "recordType": record_type,
            "source": "deezer"
          }));
        }
      }
    }
  } else {
    static RE_SPLIT_ARTIST: OnceLock<Regex> = OnceLock::new();
    let re_split =
      RE_SPLIT_ARTIST.get_or_init(|| Regex::new(r"(?i)[,&/]|feat\.?|ft\.?").expect("regex"));

    let ytm_tracks = search_ytm_tracks(core, &clean_name, 25).await;
    let filtered_ytm: Vec<Value> = ytm_tracks
      .into_iter()
      .filter(|t| {
        let a = t
          .get("artist")
          .and_then(Value::as_str)
          .unwrap_or("")
          .trim()
          .to_lowercase();
        if a == norm_clean {
          return true;
        }
        re_split.split(&a).any(|s| s.trim() == norm_clean)
      })
      .collect();
    if filtered_ytm.len() >= 4 {
      tracks = filtered_ytm;
    }

    if tracks.len() < 4 {
      let it_url = format!(
        "https://itunes.apple.com/search?term={}&entity=song&limit=30",
        encode_uri_component(&clean_name)
      );
      if let Some(it_data) = fetch_itunes_json(&core.net.http_client, &it_url).await
        && let Some(results) = it_data.get("results").and_then(Value::as_array)
      {
        let it_tracks: Vec<Value> = results
          .iter()
          .filter(|item| item.get("wrapperType").and_then(Value::as_str) == Some("track"))
          .map(|item| {
            let dur_ms = item
              .get("trackTimeMillis")
              .and_then(Value::as_i64)
              .unwrap_or(0);
            let dur = ((dur_ms as f64) / 1000.0).round() as i64;
            let cover = item
              .get("artworkUrl100")
              .and_then(Value::as_str)
              .map(|s| s.replace("100x100", "600x600"))
              .unwrap_or_default();
            let t_id = value_to_string(item.get("trackId"));
            let t_name = item
              .get("trackName")
              .and_then(Value::as_str)
              .filter(|s| !s.is_empty())
              .unwrap_or("untitled");
            let a_name = item
              .get("artistName")
              .and_then(Value::as_str)
              .filter(|s| !s.is_empty())
              .unwrap_or(&resolved_name);
            let sq = format!("{a_name} - {t_name} official audio");
            let is_exp = item.get("trackExplicitness").and_then(Value::as_str) == Some("explicit")
              || is_text_explicit(t_name);
            json!({
              "id": format!("it_{t_id}"),
              "title": t_name,
              "artist": a_name,
              "duration": dur,
              "durationMs": if dur_ms > 0 { dur_ms } else { dur * 1000 },
              "thumbnail": cover,
              "url": format!("https://www.youtube.com/results?search_query={}", encode_uri_component(&sq)),
              "searchQuery": sq,
              "platform": "youtube",
              "isCached": false,
              "isExplicit": is_exp
            })
          })
          .collect();
        if !it_tracks.is_empty() {
          tracks = it_tracks;
        }
      }
    }

    if tracks.len() < 4 {
      let direct = direct_youtube_search(
        &core.net.http_client,
        &format!("{clean_name} official audio"),
        true,
        40,
      )
      .await;
      if !direct.is_empty() {
        tracks = direct;
      }
    }
  }

  let bio = fetch_music_bio(&core.net.http_client, &clean_name).await;

  let lfm_url = format!(
    "https://ws.audioscrobbler.com/2.0/?method=artist.getinfo&artist={}&api_key={}&format=json",
    encode_uri_component(&clean_name),
    secrets::lastfm_api_key()
  );
  if let Some(lfm_data) = http_get_json(&core.net.http_client, &lfm_url, 4000, "Riff").await
    && let Some(listeners_str) = lfm_data
      .get("artist")
      .and_then(|a| a.get("stats"))
      .and_then(|s| s.get("listeners"))
      .and_then(Value::as_str)
    && let Ok(lfm_listeners) = listeners_str.parse::<u64>()
    && lfm_listeners > 0
  {
    fans_count = fans_count.max(lfm_listeners);
    monthly_listeners = format!("{} monthly listeners", format_thousands(lfm_listeners));
  }

  if fans_count > 12_000 || tracks.len() >= 5 {
    is_verified = true;
    is_known_artist = true;
  }

  let first_thumb = tracks
    .first()
    .and_then(|t| t.get("thumbnail"))
    .and_then(Value::as_str)
    .unwrap_or("");
  let final_avatar = if !avatar.is_empty() {
    avatar.clone()
  } else {
    first_thumb.to_string()
  };
  let final_banner = if !banner.is_empty() {
    banner
  } else if !avatar.is_empty() {
    avatar
  } else {
    first_thumb.to_string()
  };
  tracks.truncate(100);

  let result = json!({
    "name": resolved_name,
    "bio": bio.trim(),
    "avatar": final_avatar,
    "banner": final_banner,
    "albums": albums,
    "isVerified": is_verified,
    "isKnownArtist": is_known_artist,
    "fans": fans_count,
    "monthlyListeners": monthly_listeners,
    "tracks": tracks
  });

  if let Ok(mut guard) = core.caches.artist_cache.lock() {
    if let Some(pos) = guard.iter().position(|(k, _)| k == &cache_key) {
      guard.remove(pos);
    } else if guard.len() >= 35 {
      guard.remove(0);
    }
    guard.push((cache_key, result.clone()));
  }

  result
}

pub async fn artist(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let name = query.get("name").map(String::as_str).unwrap_or("");
  let hint_track = query.get("hintTrack").map(String::as_str).unwrap_or("");
  if name.is_empty() {
    return json_response(
      400,
      &json!({ "success": false, "error": "Missing artist name" }),
    );
  }
  let info = fetch_artist_info(core, name, hint_track).await;
  json_response(
    200,
    &json!({
      "success": true,
      "artist": info
    }),
  )
}
