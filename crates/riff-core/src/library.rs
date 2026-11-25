//! Bibliothèque locale et cache hors ligne.

use crate::core::{Core, MAX_BODY_BYTES};
use crate::paths;
use crate::reply::{
  CoreResult, Method, Params, json_response, serve_file_with_range, text_response,
};
use crate::util::{
  clean_track_id, is_truthy, lyrics_key, normalize_number_value, now_millis, value_to_string,
};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub fn find_cached_file(track_id: &str) -> Option<PathBuf> {
  let clean_id = clean_track_id(track_id);
  let tracks_dir = paths::tracks_dir();
  for ext in [".opus", ".webm", ".m4a", ".mp3"] {
    let file_path = tracks_dir.join(format!("{clean_id}{ext}"));
    if let Ok(meta) = fs::metadata(&file_path)
      && meta.is_file()
      && meta.len() > 60_000
    {
      return Some(file_path);
    }
  }
  None
}

pub fn library_mime_type(ext: &str) -> &'static str {
  match ext.to_ascii_lowercase().as_str() {
    "mp3" | ".mp3" => "audio/mpeg",
    "wav" | ".wav" => "audio/wav",
    "flac" | ".flac" => "audio/flac",
    "opus" | ".opus" => "audio/ogg; codecs=opus",
    "ogg" | ".ogg" | "oga" | ".oga" => "audio/ogg",
    "m4a" | ".m4a" => "audio/mp4",
    "aac" | ".aac" => "audio/aac",
    "webm" | ".webm" | "weba" | ".weba" => "audio/webm",
    _ => "audio/mpeg",
  }
}

pub fn local_track_mime_type(ext: &str) -> &'static str {
  match ext.to_ascii_lowercase().as_str() {
    "opus" | ".opus" => "audio/ogg; codecs=opus",
    "webm" | ".webm" => "audio/webm",
    "m4a" | ".m4a" => "audio/mp4",
    "mp3" | ".mp3" => "audio/mpeg",
    _ => "audio/mpeg",
  }
}

pub fn is_valid_library_file_name(file: &str) -> bool {
  let Some(rest) = file.strip_prefix("local_") else {
    return false;
  };
  let Some((hex_part, ext_part)) = rest.split_once('.') else {
    return false;
  };
  if hex_part.len() != 12
    || !hex_part
      .chars()
      .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
  {
    return false;
  }
  !ext_part.is_empty()
    && ext_part
      .chars()
      .all(|c: char| c.is_ascii_digit() || c.is_ascii_lowercase())
}

pub fn strip_leading_track_number(raw: &str) -> &str {
  let bytes = raw.as_bytes();
  let mut i = 0;
  while i < bytes.len() && bytes[i].is_ascii_digit() {
    i += 1;
  }
  if i == 0 {
    return raw;
  }
  let mut sep_count = 0;
  while i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b'-' || bytes[i].is_ascii_whitespace())
  {
    sep_count += 1;
    i += 1;
  }
  if sep_count == 0 {
    return raw;
  }
  while i < bytes.len() && bytes[i].is_ascii_whitespace() {
    i += 1;
  }
  &raw[i..]
}

pub fn is_offline_audio_file(file_name: &str) -> Option<&str> {
  if file_name.contains(".temp.") {
    return None;
  }
  for ext in [".opus", ".webm", ".m4a", ".mp3"] {
    if file_name.len() > ext.len()
      && file_name[file_name.len() - ext.len()..].eq_ignore_ascii_case(ext)
    {
      return Some(&file_name[..file_name.len() - ext.len()]);
    }
  }
  None
}

pub async fn check_cache(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let track_id = query.get("id").map(String::as_str).unwrap_or("");
  let clean_id = clean_track_id(track_id);
  let cached_file = find_cached_file(&clean_id);
  let stream_url = cached_file.map(|_| core.local_url(&format!("/api/local-track?id={clean_id}")));
  json_response(
    200,
    &json!({
      "cached": stream_url.is_some(),
      "streamUrl": stream_url
    }),
  )
}

pub async fn local_track(core: &Core, params: Params) -> CoreResult {
  let (query, headers) = (&params.query, &params.headers);
  let track_id = query.get("id").map(String::as_str).unwrap_or("");
  let clean_id = clean_track_id(track_id);
  let mut file_path = find_cached_file(&clean_id);

  if file_path.is_none() {
    let meta_map = core.sync_metadata_from_disk();
    let tracks_dir = paths::tracks_dir();
    for (cid, meta) in &meta_map {
      let title_matches = meta
        .get("title")
        .and_then(Value::as_str)
        .map(|t| clean_track_id(t) == clean_id)
        .unwrap_or(false);
      if cid == &clean_id || title_matches {
        for ext in [".opus", ".webm", ".m4a", ".mp3"] {
          let fp = tracks_dir.join(format!("{cid}{ext}"));
          if fp.is_file() {
            file_path = Some(fp);
            break;
          }
        }
      }
      if file_path.is_some() {
        break;
      }
    }
  }

  let Some(path) = file_path else {
    return text_response(404, "File not found in local cache");
  };

  let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp3");
  let content_type = local_track_mime_type(ext);
  serve_file_with_range(
    &path,
    headers,
    content_type,
    "File not found in local cache",
    500,
    "Error reading local file",
  )
}

pub async fn library_add(core: &Core, params: Params) -> CoreResult {
  let body = &params.body;
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let text = std::str::from_utf8(body).unwrap_or("");
  let json_input = if text.is_empty() { "{}" } else { text };
  let payload = match serde_json::from_str::<Value>(json_input) {
    Ok(v) => v,
    Err(err) => {
      return json_response(400, &json!({ "success": false, "error": err.to_string() }));
    }
  };

  let mut lib_map = core.sync_library_from_disk();
  let mut added_count: u64 = 0;
  let lib_dir = paths::library_dir();

  if let Some(items) = payload.get("items").and_then(Value::as_array) {
    for item in items {
      let Some(obj) = item.as_object() else {
        continue;
      };
      let id = value_to_string(obj.get("id"));
      let file = value_to_string(obj.get("file"));
      if id.is_empty() || file.is_empty() {
        continue;
      }
      if !is_valid_library_file_name(&file) {
        continue;
      }
      let full_path = lib_dir.join(&file);
      if !full_path.exists() {
        continue;
      }

      let raw_name = value_to_string(obj.get("name")).trim().to_string();
      let stripped = strip_leading_track_number(&raw_name).trim();
      let clean_name = if !stripped.is_empty() {
        stripped.to_string()
      } else if !raw_name.is_empty() {
        raw_name.clone()
      } else {
        "Untitled".to_string()
      };

      let (artist, title) = if let Some(dash_idx) = clean_name.find(" - ") {
        let a = clean_name[..dash_idx].trim();
        let t = clean_name[dash_idx + 3..].trim();
        (
          if a.is_empty() {
            "Unknown artist".to_string()
          } else {
            a.to_string()
          },
          if t.is_empty() {
            clean_name.clone()
          } else {
            t.to_string()
          },
        )
      } else {
        ("Unknown artist".to_string(), clean_name)
      };

      let provided_ext = value_to_string(obj.get("ext"));
      let ext = if !provided_ext.is_empty() {
        provided_ext
      } else {
        Path::new(&file)
          .extension()
          .and_then(|e| e.to_str())
          .unwrap_or("")
          .to_ascii_lowercase()
      };

      lib_map.insert(
        id.clone(),
        json!({
          "id": id,
          "file": file,
          "ext": ext,
          "title": title,
          "artist": artist,
          "duration": 0,
          "addedAt": now_millis()
        }),
      );
      added_count += 1;
    }
  }

  core.save_library(&lib_map);
  json_response(200, &json!({ "success": true, "count": added_count }))
}

pub async fn library_update(core: &Core, params: Params) -> CoreResult {
  let body = &params.body;
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let text = std::str::from_utf8(body).unwrap_or("");
  let json_input = if text.is_empty() { "{}" } else { text };
  let payload = match serde_json::from_str::<Value>(json_input) {
    Ok(v) => v,
    Err(err) => {
      return json_response(400, &json!({ "success": false, "error": err.to_string() }));
    }
  };

  let id = payload.get("id").and_then(Value::as_str).unwrap_or("");
  let duration_opt = payload.get("duration").and_then(Value::as_f64);
  if !id.is_empty()
    && let Some(dur) = duration_opt
    && dur.is_finite()
  {
    let mut lib_map = core.sync_library_from_disk();
    if let Some(Value::Object(item)) = lib_map.get_mut(id) {
      let dur_val = if dur.fract() == 0.0 && dur >= (i64::MIN as f64) && dur <= (i64::MAX as f64) {
        json!(dur as i64)
      } else {
        json!(dur)
      };
      item.insert("duration".to_string(), dur_val);
      core.save_library(&lib_map);
    }
  }

  json_response(200, &json!({ "success": true }))
}

pub async fn library(core: &Core) -> CoreResult {
  let lib_map = core.sync_library_from_disk();
  let mut entries: Vec<&Value> = lib_map.values().collect();
  entries.sort_by(|a, b| {
    let a_ts = a.get("addedAt").and_then(Value::as_f64).unwrap_or(0.0);
    let b_ts = b.get("addedAt").and_then(Value::as_f64).unwrap_or(0.0);
    b_ts.partial_cmp(&a_ts).unwrap_or(std::cmp::Ordering::Equal)
  });

  let list: Vec<Value> = entries
    .into_iter()
    .map(|item| {
      json!({
        "id": item.get("id").cloned().unwrap_or(Value::Null),
        "platform": "local",
        "title": item.get("title").cloned().unwrap_or(Value::Null),
        "artist": item.get("artist").cloned().unwrap_or(Value::Null),
        "duration": normalize_number_value(item.get("duration")),
        "thumbnail": ""
      })
    })
    .collect();

  json_response(200, &Value::Array(list))
}

pub async fn library_file(core: &Core, params: Params) -> CoreResult {
  let (query, headers) = (&params.query, &params.headers);
  let track_id = query.get("id").map(String::as_str).unwrap_or("");
  let lib_map = core.sync_library_from_disk();
  let Some(item) = lib_map.get(track_id).and_then(Value::as_object) else {
    return text_response(404, "File not found in library");
  };
  let Some(file_name) = item
    .get("file")
    .and_then(Value::as_str)
    .filter(|s| !s.is_empty())
  else {
    return text_response(404, "File not found in library");
  };

  let file_path = paths::library_dir().join(file_name);
  let ext = file_path
    .extension()
    .and_then(|e| e.to_str())
    .unwrap_or("mp3");
  let content_type = library_mime_type(ext);

  serve_file_with_range(
    &file_path,
    headers,
    content_type,
    "File not found in library",
    404,
    "File not found in library",
  )
}

pub async fn library_remove(core: &Core, params: Params) -> CoreResult {
  let (query, body) = (&params.query, &params.body);
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let text = std::str::from_utf8(body).unwrap_or("");
  let json_input = if text.is_empty() { "{}" } else { text };
  let payload = match serde_json::from_str::<Value>(json_input) {
    Ok(v) => v,
    Err(err) => {
      return json_response(400, &json!({ "success": false, "error": err.to_string() }));
    }
  };

  let mut lib_map = core.sync_library_from_disk();
  let lib_dir = paths::library_dir();

  if is_truthy(payload.get("clearAll")) {
    if let Ok(entries) = fs::read_dir(&lib_dir) {
      for entry in entries.filter_map(|e| e.ok()) {
        let _ = fs::remove_file(entry.path());
      }
    }
    lib_map.clear();
    core.save_library(&lib_map);
  } else if let Some(ids) = payload.get("ids").and_then(Value::as_array) {
    for id_val in ids {
      let id = value_to_string(Some(id_val));
      if !id.is_empty()
        && let Some(removed) = lib_map.remove(&id)
        && let Some(file_name) = removed.get("file").and_then(Value::as_str)
        && !file_name.is_empty()
      {
        let full_path = lib_dir.join(file_name);
        if full_path.exists() {
          let _ = fs::remove_file(full_path);
        }
      }
    }
    core.save_library(&lib_map);
  } else {
    let payload_id = value_to_string(payload.get("id"));
    let id = if !payload_id.is_empty() {
      payload_id
    } else {
      query.get("id").cloned().unwrap_or_default()
    };
    if !id.is_empty()
      && let Some(removed) = lib_map.remove(&id)
    {
      if let Some(file_name) = removed.get("file").and_then(Value::as_str)
        && !file_name.is_empty()
      {
        let full_path = lib_dir.join(file_name);
        if full_path.exists() {
          let _ = fs::remove_file(full_path);
        }
      }
      core.save_library(&lib_map);
    }
  }

  json_response(200, &json!({ "success": true }))
}

pub async fn offline_library(core: &Core) -> CoreResult {
  let mut tracks: Vec<Value> = Vec::new();
  let mut total_bytes: u64 = 0;
  let tracks_dir = paths::tracks_dir();
  let thumbs_dir = paths::thumbnails_dir();
  let meta_map = core.sync_metadata_from_disk();

  if let Ok(entries) = fs::read_dir(&tracks_dir) {
    let mut file_names: Vec<String> = entries
      .filter_map(|e| e.ok())
      .filter_map(|e| e.file_name().into_string().ok())
      .collect();
    file_names.sort();

    for file in file_names {
      let Some(clean_id) = is_offline_audio_file(&file) else {
        continue;
      };
      let full_path = tracks_dir.join(&file);
      let Ok(stat) = fs::metadata(&full_path) else {
        continue;
      };
      let file_size = stat.len();
      total_bytes += file_size;

      let mtime_ms = stat
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);

      let meta = meta_map.get(clean_id).and_then(Value::as_object);

      let id = meta
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(clean_id);
      let title = meta
        .and_then(|m| m.get("title"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(clean_id);
      let artist = meta
        .and_then(|m| m.get("artist"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("Unknown artist");
      let duration = normalize_number_value(meta.and_then(|m| m.get("duration")));
      let thumbnail = meta
        .and_then(|m| m.get("thumbnail"))
        .and_then(Value::as_str)
        .unwrap_or("");
      let local_thumbnail = if thumbs_dir.join(format!("{clean_id}.jpg")).exists() {
        core.local_url(&format!("/api/local-thumbnail?id={clean_id}"))
      } else {
        thumbnail.to_string()
      };
      let platform_val = meta
        .and_then(|m| m.get("platform"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("youtube");
      let last_played = match meta.and_then(|m| m.get("lastPlayed")) {
        Some(v) if is_truthy(Some(v)) => v.clone(),
        _ => json!(mtime_ms as u64),
      };
      let analysis = meta
        .and_then(|m| m.get("analysis"))
        .filter(|v| is_truthy(Some(v)))
        .cloned()
        .unwrap_or(Value::Null);

      tracks.push(json!({
        "id": id,
        "cleanId": clean_id,
        "title": title,
        "artist": artist,
        "duration": duration,
        "thumbnail": thumbnail,
        "localThumbnail": local_thumbnail,
        "platform": platform_val,
        "fileSize": file_size,
        "lastPlayed": last_played,
        "analysis": analysis
      }));
    }
  }

  json_response(
    200,
    &json!({
      "success": true,
      "count": tracks.len(),
      "totalBytes": total_bytes,
      "tracks": tracks
    }),
  )
}

pub async fn offline_delete(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let track_id = query.get("id").map(String::as_str).unwrap_or("");
  let clean_id = clean_track_id(track_id);
  if let Some(audio_path) = find_cached_file(&clean_id)
    && audio_path.exists()
  {
    let _ = fs::remove_file(audio_path);
  }
  let thumb_path = paths::thumbnails_dir().join(format!("{clean_id}.jpg"));
  if thumb_path.exists() {
    let _ = fs::remove_file(thumb_path);
  }
  let mut meta_map = core.sync_metadata_from_disk();
  meta_map.remove(&clean_id);
  core.save_metadata(&meta_map);

  json_response(200, &json!({ "success": true }))
}

pub async fn clear_offline_library(core: &Core) -> CoreResult {
  let tracks_dir = paths::tracks_dir();
  if let Ok(entries) = fs::read_dir(&tracks_dir) {
    for entry in entries.filter_map(|e| e.ok()) {
      let name = entry.file_name().to_string_lossy().to_string();
      let is_audio = [".opus", ".webm", ".m4a", ".mp3"].iter().any(|ext| {
        name.len() >= ext.len() && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext)
      });
      if is_audio {
        let _ = fs::remove_file(entry.path());
      }
    }
  }
  let thumbs_dir = paths::thumbnails_dir();
  if let Ok(entries) = fs::read_dir(&thumbs_dir) {
    for entry in entries.filter_map(|e| e.ok()) {
      let _ = fs::remove_file(entry.path());
    }
  }
  let empty_map = Map::new();
  if let Ok(mut guard) = core.library.track_metadata_index.lock() {
    *guard = empty_map;
  }
  let _ = fs::write(paths::metadata_path(), "{}");

  json_response(200, &json!({ "success": true }))
}

pub async fn save_custom_lyrics(core: &Core, params: Params) -> CoreResult {
  let (query, body) = (&params.query, &params.body);
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let title = query.get("title").map(String::as_str).unwrap_or("");
  let artist = query.get("artist").map(String::as_str).unwrap_or("");
  let text = std::str::from_utf8(body).unwrap_or("");
  let payload = match serde_json::from_str::<Value>(text) {
    Ok(v) if v.is_object() || v.is_array() || v.is_string() || v.is_number() || v.is_boolean() => v,
    Ok(_) => {
      return json_response(
        500,
        &json!({ "success": false, "error": "Cannot destructure property 'lyrics' of 'JSON.parse(...)' as it is null." }),
      );
    }
    Err(err) => {
      return json_response(500, &json!({ "success": false, "error": err.to_string() }));
    }
  };

  let Some(lyrics) = payload.get("lyrics").and_then(Value::as_str) else {
    return json_response(
      400,
      &json!({ "success": false, "error": "No lyrics provided" }),
    );
  };

  let dir = paths::custom_lyrics_dir();
  let _ = fs::create_dir_all(&dir);
  let custom_path = dir.join(format!("{}.lrc", lyrics_key(title, artist)));
  if let Err(err) = fs::write(&custom_path, lyrics.as_bytes()) {
    return json_response(500, &json!({ "success": false, "error": err.to_string() }));
  }

  if let Ok(mut cache) = core.caches.lyrics_cache.lock() {
    cache.clear();
  }
  json_response(200, &json!({ "success": true }))
}

pub async fn track_analysis(core: &Core, method: Method, params: Params) -> CoreResult {
  let (query, body) = (&params.query, &params.body);
  if method == Method::Post {
    if body.len() > MAX_BODY_BYTES {
      return json_response(
        413,
        &json!({ "success": false, "error": "payload too large" }),
      );
    }
    let text = std::str::from_utf8(body).unwrap_or("");
    let json_input = if text.is_empty() { "{}" } else { text };
    let payload = match serde_json::from_str::<Value>(json_input) {
      Ok(v) if v.is_object() => v,
      Ok(_) => {
        return json_response(400, &json!({ "error": "Invalid JSON payload" }));
      }
      Err(err) => {
        return json_response(400, &json!({ "error": err.to_string() }));
      }
    };
    let raw_id = payload.get("id").and_then(Value::as_str).unwrap_or("");
    let clean_id = clean_track_id(raw_id);
    if !clean_id.is_empty()
      && let Some(analysis) = payload.get("analysis")
      && is_truthy(Some(analysis))
    {
      let mut extra = Map::new();
      extra.insert("analysis".to_string(), analysis.clone());
      core.save_track_metadata_entry(&clean_id, extra);
    }
    return json_response(200, &json!({ "success": true }));
  }

  let raw_id = query.get("id").map(String::as_str).unwrap_or("");
  let clean_id = clean_track_id(raw_id);
  let meta_map = core.sync_metadata_from_disk();
  let analysis = meta_map
    .get(&clean_id)
    .and_then(|m| m.get("analysis"))
    .filter(|v| is_truthy(Some(v)))
    .cloned()
    .unwrap_or(Value::Null);

  json_response(
    200,
    &json!({
      "success": true,
      "analysis": analysis
    }),
  )
}
