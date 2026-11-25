//! Pistes échangées avec un plugin : champs autorisés seulement, sans chemin local ni URL arbitraire.

use crate::manifest::clean_text;
use serde_json::{Map, Value};

const TEXT_KEYS: [&str; 4] = ["title", "artist", "album", "platform"];
const URL_KEYS: [&str; 2] = ["url", "thumbnail"];
const MAX_TEXT_CHARS: usize = 300;
const MAX_ID_CHARS: usize = 200;
const MAX_URL_CHARS: usize = 2_048;

fn text(value: Option<&Value>, max: usize) -> Option<String> {
  let raw = match value? {
    Value::String(text) => text.clone(),
    Value::Number(number) => number.to_string(),
    _ => return None,
  };
  Some(clean_text(&raw, max)).filter(|text| !text.is_empty())
}

/// Copie d'une piste réduite aux champs connus : `id`, `title`, `artist`, `album`, `duration`, `platform`,
/// `url` et `thumbnail` en `https://` seulement. Un identifiant qui ressemble à un chemin est retiré.
#[must_use]
pub fn sanitize(value: &Value) -> Option<Value> {
  let source = value.as_object()?;
  let mut out = Map::new();
  if let Some(id) = text(source.get("id"), MAX_ID_CHARS)
    && !id.contains(['/', '\\', ':'])
  {
    out.insert("id".into(), Value::String(id));
  }
  for key in TEXT_KEYS {
    if let Some(value) = text(source.get(key), MAX_TEXT_CHARS) {
      out.insert(key.into(), Value::String(value));
    }
  }
  for key in URL_KEYS {
    if let Some(url) = source.get(key).and_then(Value::as_str)
      && url.len() <= MAX_URL_CHARS
      && url.starts_with("https://")
      && !url.chars().any(char::is_control)
    {
      out.insert(key.into(), Value::String(url.to_string()));
    }
  }
  if let Some(duration) = source.get("duration").and_then(Value::as_f64)
    && duration.is_finite()
    && duration >= 0.0
  {
    out.insert("duration".into(), Value::from(duration));
  }
  Some(Value::Object(out))
}

/// Piste proposée par un plugin : nettoyée, avec un titre et un identifiant ou une URL.
#[must_use]
pub fn incoming(value: &Value) -> Option<Value> {
  let track = sanitize(value)?;
  let has_title = track.get("title").is_some();
  let has_key = track.get("id").is_some() || track.get("url").is_some();
  (has_title && has_key).then_some(track)
}

/// `getTrackCanonicalId` : plateforme, puis identifiant, URL, ou titre, artiste et durée.
#[must_use]
pub fn canonical_id(track: &Value) -> String {
  let field = |key: &str| {
    track
      .get(key)
      .and_then(Value::as_str)
      .filter(|value| !value.is_empty())
  };
  let platform = field("platform").unwrap_or("");
  if let Some(id) = track.get("id").and_then(|id| match id {
    Value::String(text) if !text.is_empty() => Some(text.clone()),
    Value::Number(number) => Some(number.to_string()),
    _ => None,
  }) {
    return format!("{platform}:{id}");
  }
  if let Some(url) = field("url") {
    return format!("{platform}:{url}");
  }
  let lower = |key: &str| field(key).unwrap_or("").trim().to_lowercase();
  let duration = track
    .get("duration")
    .and_then(Value::as_f64)
    .unwrap_or(0.0)
    .round() as i64;
  format!(
    "{platform}:{}|{}|{duration}",
    lower("title"),
    lower("artist")
  )
}

/// `isSameTrack`.
#[must_use]
pub fn same_track(a: &Value, b: &Value) -> bool {
  a.is_object() && b.is_object() && canonical_id(a) == canonical_id(b)
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn unknown_fields_local_paths_and_non_https_urls_are_removed() {
    let raw = json!({
      "id": "abc", "title": "T", "artist": "A", "duration": 200.5, "platform": "youtube",
      "url": "file:///C:/Users/x/music.mp3", "thumbnail": "https://i.ytimg.com/a.jpg",
      "path": "C:\\Users\\x\\music.mp3", "localThumbnail": "C:\\x.jpg", "cookies": "secret"
    });
    assert_eq!(
      sanitize(&raw).unwrap(),
      json!({ "id": "abc", "title": "T", "artist": "A", "platform": "youtube",
        "thumbnail": "https://i.ytimg.com/a.jpg", "duration": 200.5 })
    );
  }

  #[test]
  fn an_id_that_looks_like_a_path_is_dropped() {
    for id in ["C:\\music\\a.mp3", "/home/a.mp3", "riff://local/a"] {
      assert!(
        sanitize(&json!({ "id": id })).unwrap().get("id").is_none(),
        "{id}"
      );
    }
  }

  #[test]
  fn text_is_cleaned_and_bounded() {
    let long = "x".repeat(1_000);
    let track = sanitize(&json!({ "title": format!("a\u{1b}[2Jb{long}") })).unwrap();
    let title = track["title"].as_str().unwrap();
    assert!(title.starts_with("a[2Jb"));
    assert_eq!(title.chars().count(), MAX_TEXT_CHARS);
  }

  #[test]
  fn an_incoming_track_needs_a_title_and_an_id_or_url() {
    assert!(incoming(&json!({ "title": "T", "id": "1" })).is_some());
    assert!(incoming(&json!({ "title": "T", "url": "https://youtu.be/x" })).is_some());
    assert!(incoming(&json!({ "title": "T" })).is_none());
    assert!(incoming(&json!({ "id": "1" })).is_none());
    assert!(incoming(&json!({ "title": "T", "url": "http://insecure/x" })).is_none());
    assert!(incoming(&json!("not a track")).is_none());
  }

  #[test]
  fn same_track_follows_the_canonical_id() {
    let a = json!({ "platform": "youtube", "id": "1", "title": "A" });
    let b = json!({ "platform": "youtube", "id": "1", "title": "Autre titre" });
    let c = json!({ "platform": "soundcloud", "id": "1" });
    assert!(same_track(&a, &b));
    assert!(!same_track(&a, &c));
    let d = json!({ "title": " Song ", "artist": "X", "duration": 120.2 });
    let e = json!({ "title": "song", "artist": "x", "duration": 119.9 });
    assert!(same_track(&d, &e));
    assert!(!same_track(&json!(null), &json!(null)));
  }
}
