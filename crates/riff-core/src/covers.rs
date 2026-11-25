//! Pochettes, miniatures et icônes.

use crate::core::Core;
use crate::net::Http;
use crate::paths;
use crate::reply::{CoreResult, Params, Reply, json_response, text_response};
use crate::util::{clean_track_id, encode_uri_component, http_get_json, now_millis, sha1_hex};
use bytes::Bytes;
use reqwest::header;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

pub async fn fetch_track_cover_url(client: &Http, artist: &str, title: &str) -> String {
  if artist.is_empty() && title.is_empty() {
    return String::new();
  }
  let term = encode_uri_component(format!("{artist} {title}").trim());
  let itunes = format!("https://itunes.apple.com/search?term={term}&entity=song&limit=1");
  if let Some(data) = http_get_json(client, &itunes, 6000, "Mozilla/5.0").await
    && let Some(art) = data
      .pointer("/results/0/artworkUrl100")
      .and_then(Value::as_str)
    && !art.is_empty()
  {
    return art.replace("100x100", "600x600");
  }
  let deezer = format!("https://api.deezer.com/search?q={term}&limit=1");
  if let Some(data) = http_get_json(client, &deezer, 6000, "Mozilla/5.0").await {
    for key in ["cover_xl", "cover_big", "cover_medium"] {
      if let Some(c) = data
        .pointer(&format!("/data/0/album/{key}"))
        .and_then(Value::as_str)
        && !c.is_empty()
      {
        return c.to_string();
      }
    }
  }
  String::new()
}

pub async fn track_cover(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let artist = query.get("artist").map(String::as_str).unwrap_or("");
  let title = query.get("title").map(String::as_str).unwrap_or("");
  let cover = fetch_track_cover_url(&core.net.http_client, artist, title).await;
  json_response(
    200,
    &json!({ "success": !cover.is_empty(), "cover": cover }),
  )
}

pub async fn local_thumbnail(params: Params) -> CoreResult {
  let query = &params.query;
  let track_id = query.get("id").map(String::as_str).unwrap_or("");
  let clean_id = clean_track_id(track_id);
  let thumb_path = paths::thumbnails_dir().join(format!("{clean_id}.jpg"));
  if thumb_path.is_file()
    && let Ok(bytes) = fs::read(&thumb_path)
  {
    return Ok(Reply::buffered(
      200,
      vec![
        ("content-type".to_string(), "image/jpeg".to_string()),
        (
          "cache-control".to_string(),
          "public, max-age=31536000".to_string(),
        ),
      ],
      Bytes::from(bytes),
    ));
  }
  text_response(404, "Thumbnail not cached")
}

pub fn brand_icon_color(brand: &str) -> &'static str {
  match brand {
    "spotify" => "1ED760",
    "youtubemusic" | "youtube" => "FF0000",
    "soundcloud" => "FF5500",
    "applemusic" => "FA243C",
    "deezer" => "A238FF",
    "yandexmusic" => "FFCC00",
    "json" => "7E7A8A",
    _ => "",
  }
}

pub async fn brand_icon(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let raw_name = query.get("name").map(String::as_str).unwrap_or("");
  let brand: String = raw_name
    .to_ascii_lowercase()
    .chars()
    .filter(|c| c.is_ascii_alphanumeric())
    .collect();
  if brand.is_empty() {
    return text_response(400, "");
  }

  let color = brand_icon_color(&brand);
  let cached_file = paths::thumbnails_dir().join(format!("brand_{brand}.svg"));
  if cached_file.is_file()
    && let Ok(bytes) = fs::read(&cached_file)
  {
    return Ok(Reply::buffered(
      200,
      vec![
        ("content-type".to_string(), "image/svg+xml".to_string()),
        (
          "cache-control".to_string(),
          "public, max-age=604800".to_string(),
        ),
      ],
      Bytes::from(bytes),
    ));
  }

  let cdn_url = if !color.is_empty() {
    format!("https://cdn.simpleicons.org/{brand}/{color}")
  } else {
    format!("https://cdn.simpleicons.org/{brand}")
  };

  if let Ok(resp) = core
    .net
    .http_client
    .get(&cdn_url)
    .timeout(Duration::from_millis(6000))
    .send()
    .await
    && resp.status().is_success()
    && let Ok(svg_text) = resp.text().await
    && svg_text.contains("<svg")
  {
    let _ = fs::write(&cached_file, svg_text.as_bytes());
    return Ok(Reply::buffered(
      200,
      vec![
        ("content-type".to_string(), "image/svg+xml".to_string()),
        (
          "cache-control".to_string(),
          "public, max-age=604800".to_string(),
        ),
      ],
      Bytes::from(svg_text),
    ));
  }

  Ok(Reply::empty_with_headers(
    302,
    vec![("location".to_string(), cdn_url.to_string())],
  ))
}

pub fn image_content_type(buf: &[u8]) -> &'static str {
  if buf.len() > 3 && buf[0] == 0xff && buf[1] == 0xd8 {
    return "image/jpeg";
  }
  if buf.len() > 8 && buf[0] == 0x89 && buf[1] == 0x50 {
    return "image/png";
  }
  if buf.len() > 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WEBP" {
    return "image/webp";
  }
  if buf.len() > 6 && &buf[0..3] == b"GIF" {
    return "image/gif";
  }
  "image/jpeg"
}

pub async fn lookup_cover_art(core: &Core, artist: &str, title: &str) -> Option<PathBuf> {
  let key: String = sha1_hex(&format!("{artist}\0{title}").to_lowercase())
    .chars()
    .take(16)
    .collect();
  let dest = paths::thumbnails_dir().join(format!("cover-{key}.jpg"));
  if dest.is_file() {
    return Some(dest);
  }
  let term = encode_uri_component(&format!("{artist} {title}"));
  let search_url = format!("https://itunes.apple.com/search?term={term}&entity=song&limit=5");
  let data = http_get_json(&core.net.http_client, &search_url, 8000, "Mozilla/5.0").await?;
  let art_100 = data
    .get("results")
    .and_then(Value::as_array)
    .and_then(|arr| arr.first())
    .and_then(|item| item.get("artworkUrl100"))
    .and_then(Value::as_str)?;
  if art_100.is_empty() {
    return None;
  }
  let art_600 = art_100.replace("100x100", "600x600");
  let resp = core
    .net
    .image_client
    .get(&art_600)
    .timeout(Duration::from_millis(8000))
    .send()
    .await
    .ok()?;
  if !resp.status().is_success() {
    return None;
  }
  let bytes = resp.bytes().await.ok()?;
  if bytes.len() < 500 {
    return None;
  }
  fs::write(&dest, &bytes).ok()?;
  Some(dest)
}

pub async fn image_proxy(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let target = query.get("url").map(String::as_str).unwrap_or("");
  let lower = target.to_ascii_lowercase();
  if !lower.starts_with("http://") && !lower.starts_with("https://") {
    return text_response(400, "");
  }
  let Ok(parsed) = Url::parse(target) else {
    return text_response(400, "");
  };
  if core.net.image_client.check(&parsed).is_err() {
    return text_response(403, "Blocked target");
  }

  let key = sha1_hex(target);
  let cache_path = paths::thumbnails_dir().join(format!("proxy-{key}.img"));
  let mut buffer: Option<Vec<u8>> = fs::read(&cache_path).ok();

  let host = parsed.host_str().unwrap_or("").to_string();
  let now = now_millis();
  let host_dead = !host.is_empty()
    && core
      .caches
      .thumb_host_fails
      .lock()
      .ok()
      .and_then(|m| m.get(&host).copied())
      .unwrap_or(0)
      > now;

  if buffer.is_none() && !host_dead {
    let fetched = async {
      let resp = core
        .net
        .image_client
        .get(target)
        .header(
          header::USER_AGENT,
          "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36",
        )
        .timeout(Duration::from_millis(6000))
        .send()
        .await
        .ok()?;
      if !resp.status().is_success() {
        return None;
      }
      let data = resp.bytes().await.ok()?.to_vec();
      if data.len() < 500 {
        return None;
      }
      Some(data)
    }
    .await;

    if let Some(data) = fetched {
      let _ = fs::write(&cache_path, &data);
      buffer = Some(data);
    } else if !host.is_empty()
      && let Ok(mut guard) = core.caches.thumb_host_fails.lock()
    {
      guard.insert(host, now_millis() + 600_000);
    }
  }

  if buffer.is_none() {
    let artist = query.get("artist").map(String::as_str).unwrap_or("");
    let title = query.get("title").map(String::as_str).unwrap_or("");
    if !artist.is_empty()
      && !title.is_empty()
      && let Some(alt) = lookup_cover_art(core, artist, title).await
    {
      buffer = fs::read(alt).ok();
    }
  }

  let Some(buf) = buffer else {
    return Ok(Reply::empty_with_headers(
      302,
      vec![("location".to_string(), target.to_string())],
    ));
  };

  let ctype = image_content_type(&buf);
  Ok(Reply::buffered(
    200,
    vec![
      ("content-type".to_string(), ctype.to_string()),
      (
        "cache-control".to_string(),
        "public, max-age=31536000".to_string(),
      ),
    ],
    Bytes::from(buf),
  ))
}
