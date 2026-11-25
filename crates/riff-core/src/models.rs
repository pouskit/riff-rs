//! Types du domaine. Seul `Track` est typé pour l'instant, les autres réponses restent des `Value`.

use crate::error::CoreError;
use crate::privacy::clean_url;
use crate::util::{normalize_number_value, value_to_string};
use serde_json::{Value, json};

/// Titre normalisé, avec les mêmes règles que `normalize_track_obj`.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
  pub id: String,
  pub title: String,
  pub artist: String,
  /// En secondes : entier ou flottant selon la source.
  pub duration: f64,
  pub thumbnail: String,
  pub url: String,
  pub platform: String,
  pub search_query: String,
}

pub fn normalize_track_obj(track: &Value) -> Option<Value> {
  let obj = track.as_object()?;
  let title = value_to_string(obj.get("title")).trim().to_string();
  let artist = value_to_string(obj.get("artist")).trim().to_string();
  let raw_id = value_to_string(obj.get("id"));
  let raw_url = clean_url(&value_to_string(obj.get("url")));
  let fallback_id = format!("{}:{}", artist.to_lowercase(), title.to_lowercase());
  let id = if !raw_id.is_empty() {
    raw_id.trim().to_string()
  } else if !raw_url.is_empty() {
    raw_url.trim().to_string()
  } else {
    fallback_id.trim().to_string()
  };
  if id.is_empty() && title.is_empty() {
    return None;
  }
  let duration = normalize_number_value(obj.get("duration"));
  let thumbnail = value_to_string(obj.get("thumbnail"));
  let platform_val = value_to_string(obj.get("platform"));
  let search_query = value_to_string(obj.get("searchQuery"));
  Some(json!({
    "id": id,
    "title": if title.is_empty() { "untitled".to_string() } else { title },
    "artist": if artist.is_empty() { "unknown artist".to_string() } else { artist },
    "duration": duration,
    "thumbnail": thumbnail,
    "url": raw_url,
    "platform": if platform_val.is_empty() { "soundcloud".to_string() } else { platform_val },
    "searchQuery": search_query
  }))
}

impl TryFrom<&Value> for Track {
  type Error = CoreError;

  fn try_from(value: &Value) -> Result<Self, Self::Error> {
    let obj = value
      .as_object()
      .ok_or_else(|| CoreError::BadRequest("track is not an object".to_string()))?;
    let title = value_to_string(obj.get("title")).trim().to_string();
    let artist = value_to_string(obj.get("artist")).trim().to_string();
    let raw_id = value_to_string(obj.get("id"));
    let url = clean_url(&value_to_string(obj.get("url")));
    let id = if !raw_id.is_empty() {
      raw_id.trim().to_string()
    } else if !url.is_empty() {
      url.trim().to_string()
    } else {
      format!("{}:{}", artist.to_lowercase(), title.to_lowercase())
        .trim()
        .to_string()
    };
    if id.is_empty() && title.is_empty() {
      return Err(CoreError::BadRequest(
        "track has neither id nor title".to_string(),
      ));
    }
    let platform = value_to_string(obj.get("platform"));
    Ok(Self {
      id,
      title: if title.is_empty() {
        "untitled".to_string()
      } else {
        title
      },
      artist: if artist.is_empty() {
        "unknown artist".to_string()
      } else {
        artist
      },
      duration: normalize_number_value(obj.get("duration"))
        .as_f64()
        .unwrap_or(0.0),
      thumbnail: value_to_string(obj.get("thumbnail")),
      url,
      platform: if platform.is_empty() {
        "soundcloud".to_string()
      } else {
        platform
      },
      search_query: value_to_string(obj.get("searchQuery")),
    })
  }
}

impl From<&Track> for Value {
  fn from(track: &Track) -> Self {
    json!({
      "id": track.id,
      "title": track.title,
      "artist": track.artist,
      "duration": normalize_number_value(Some(&json!(track.duration))),
      "thumbnail": track.thumbnail,
      "url": track.url,
      "platform": track.platform,
      "searchQuery": track.search_query
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  fn full_track() -> Value {
    json!({
      "id": "abc123",
      "title": "One More Time",
      "artist": "Daft Punk",
      "duration": 320,
      "thumbnail": "https://img/x.jpg",
      "url": "https://youtu.be/abc123",
      "platform": "youtube",
      "searchQuery": "daft punk one more time"
    })
  }

  #[test]
  fn reads_every_field() {
    let track = Track::try_from(&full_track()).unwrap();
    assert_eq!(track.id, "abc123");
    assert_eq!(track.title, "One More Time");
    assert_eq!(track.artist, "Daft Punk");
    assert_eq!(track.duration.to_bits(), 320f64.to_bits());
    assert_eq!(track.thumbnail, "https://img/x.jpg");
    assert_eq!(track.url, "https://youtu.be/abc123");
    assert_eq!(track.platform, "youtube");
    assert_eq!(track.search_query, "daft punk one more time");
  }

  #[test]
  fn round_trip_keeps_the_exact_json() {
    let original = full_track();
    let track = Track::try_from(&original).unwrap();
    assert_eq!(Value::from(&track), original);
    assert_eq!(
      serde_json::to_string(&Value::from(&track)).unwrap(),
      serde_json::to_string(&original).unwrap()
    );
  }

  #[test]
  fn fractional_duration_stays_a_float() {
    let track = Track::try_from(&json!({ "id": "a", "title": "t", "duration": 215.5 })).unwrap();
    assert_eq!(Value::from(&track)["duration"], json!(215.5));
  }

  #[test]
  fn whole_float_duration_becomes_an_integer() {
    let track = Track::try_from(&json!({ "id": "a", "title": "t", "duration": 215.0 })).unwrap();
    assert_eq!(
      serde_json::to_string(&Value::from(&track)["duration"]).unwrap(),
      "215"
    );
  }

  #[test]
  fn duration_is_read_from_a_string_and_defaults_to_zero() {
    let track = Track::try_from(&json!({ "id": "a", "title": "t", "duration": "12.5" })).unwrap();
    assert_eq!(track.duration.to_bits(), 12.5f64.to_bits());
    let track = Track::try_from(&json!({ "id": "a", "title": "t" })).unwrap();
    assert_eq!(
      serde_json::to_string(&Value::from(&track)["duration"]).unwrap(),
      "0"
    );
  }

  #[test]
  fn id_falls_back_on_url_then_on_lowercase_artist_and_title() {
    let track = Track::try_from(&json!({ "url": "https://x/y", "title": "T" })).unwrap();
    assert_eq!(track.id, "https://x/y");
    let track = Track::try_from(&json!({ "title": "Song One", "artist": "Artist A" })).unwrap();
    assert_eq!(track.id, "artist a:song one");
  }

  #[test]
  fn empty_fields_take_their_default() {
    let track = Track::try_from(&json!({ "id": "a" })).unwrap();
    assert_eq!(track.title, "untitled");
    assert_eq!(track.artist, "unknown artist");
    assert_eq!(track.platform, "soundcloud");
    assert_eq!(track.search_query, "");
  }

  #[test]
  fn rejects_what_normalize_track_obj_rejects() {
    assert!(matches!(
      Track::try_from(&json!("texte")),
      Err(CoreError::BadRequest(_))
    ));
    assert!(matches!(
      Track::try_from(&json!({ "id": "   " })),
      Err(CoreError::BadRequest(_))
    ));
  }
}

#[cfg(test)]
mod url_cleaning_tests {
  use super::*;

  #[test]
  fn stored_track_urls_lose_their_tracking_parameters() {
    let track = json!({ "id": "a", "title": "T", "url": "https://youtu.be/abc?si=x&t=3" });
    assert_eq!(
      normalize_track_obj(&track).unwrap()["url"],
      json!("https://youtu.be/abc?t=3")
    );
    assert_eq!(
      Track::try_from(&track).unwrap().url,
      "https://youtu.be/abc?t=3"
    );
  }
}
