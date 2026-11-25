//! Données de l'utilisateur dans `user-data.json`, import unique depuis `storage.json`.

use crate::settings::LegacyStorage;
use crate::track_sort;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "user-data.json";
pub const DEFAULT_PLATFORM: &str = "ytmusic";
const DEFAULT_VOLUME: f32 = 0.85;

fn de_string<'de, D: Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
  Ok(match Value::deserialize(de)? {
    Value::String(text) => Some(text),
    Value::Number(number) => Some(number.to_string()),
    _ => None,
  })
}

fn de_f64<'de, D: Deserializer<'de>>(de: D) -> Result<Option<f64>, D::Error> {
  Ok(match Value::deserialize(de)? {
    Value::Number(number) => number.as_f64(),
    Value::String(text) => text.trim().parse().ok(),
    _ => None,
  })
}

fn de_i64<'de, D: Deserializer<'de>>(de: D) -> Result<Option<i64>, D::Error> {
  Ok(
    de_f64(de)?
      .filter(|value| value.is_finite())
      .map(|value| value as i64),
  )
}

fn de_bool<'de, D: Deserializer<'de>>(de: D) -> Result<bool, D::Error> {
  Ok(Value::deserialize(de)?.as_bool().unwrap_or(false))
}

/// Une durée entière reste un entier dans le fichier.
fn ser_f64<S: Serializer>(value: &Option<f64>, ser: S) -> Result<S::Ok, S::Error> {
  match value {
    Some(number) if number.fract() == 0.0 && number.abs() < 9e15 => {
      ser.serialize_i64(*number as i64)
    }
    Some(number) => ser.serialize_f64(*number),
    None => ser.serialize_none(),
  }
}

const fn is_false(value: &bool) -> bool {
  !*value
}

fn de_vec_lenient<'de, T: serde::de::DeserializeOwned, D: Deserializer<'de>>(
  de: D,
) -> Result<Vec<T>, D::Error> {
  Ok(items_from(Value::deserialize(de)?))
}

/// Éléments lisibles d'un tableau JSON : un élément invalide est ignoré, le reste est conservé.
fn items_from<T: serde::de::DeserializeOwned>(value: Value) -> Vec<T> {
  match value {
    Value::Array(items) => items
      .into_iter()
      .filter_map(|item| serde_json::from_value(item).ok())
      .collect(),
    _ => Vec::new(),
  }
}

/// Piste. Les champs inconnus de Riff restent dans `extra`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub id: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub title: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub artist: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub album: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_f64",
    serialize_with = "ser_f64",
    skip_serializing_if = "Option::is_none"
  )]
  pub duration: Option<f64>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub thumbnail: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub cover: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub url: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub platform: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub path: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub search_query: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_i64",
    skip_serializing_if = "Option::is_none"
  )]
  pub added_at: Option<i64>,
  #[serde(
    default,
    deserialize_with = "de_i64",
    skip_serializing_if = "Option::is_none"
  )]
  pub played_at: Option<i64>,
  #[serde(
    default,
    deserialize_with = "de_bool",
    skip_serializing_if = "is_false"
  )]
  pub is_cached: bool,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

fn non_empty(value: Option<&String>) -> Option<&str> {
  value.map(String::as_str).filter(|text| !text.is_empty())
}

/// Identifiant YouTube de 11 caractères, sans préfixe interne de Riff.
#[must_use]
pub fn is_youtube_id(id: &str) -> bool {
  id.len() == 11
    && id
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    && ![
      "cand_", "dz_", "it_", "sp_", "ym_", "alb_", "local_", "track_",
    ]
    .iter()
    .any(|prefix| id.len() >= prefix.len() && id[..prefix.len()].eq_ignore_ascii_case(prefix))
}

impl Track {
  /// `getTrackCanonicalId` : identifie une piste entre les listes.
  #[must_use]
  pub fn canonical_id(&self, default_platform: &str) -> String {
    let platform = non_empty(self.platform.as_ref()).unwrap_or(default_platform);
    if let Some(id) = non_empty(self.id.as_ref()) {
      return format!("{platform}:{id}");
    }
    if let Some(url) = non_empty(self.url.as_ref()) {
      return format!("{platform}:{url}");
    }
    let title = self.title.as_deref().unwrap_or("").trim().to_lowercase();
    let artist = self.artist.as_deref().unwrap_or("").trim().to_lowercase();
    format!(
      "{platform}:{title}|{artist}|{}",
      self.duration.unwrap_or(0.0).round() as i64
    )
  }

  /// `isSameTrack`.
  #[must_use]
  pub fn is_same(&self, other: &Self, default_platform: &str) -> bool {
    self.canonical_id(default_platform) == other.canonical_id(default_platform)
  }

  /// Fichier local de la machine.
  #[must_use]
  pub fn is_local(&self) -> bool {
    self.platform.as_deref() == Some("local")
  }

  #[must_use]
  pub fn title_text(&self) -> &str {
    non_empty(self.title.as_ref()).unwrap_or("Untitled")
  }

  /// Artiste affiché, sans le suffixe « - Topic » de YouTube.
  #[must_use]
  pub fn artist_text(&self) -> String {
    non_empty(self.artist.as_ref()).map_or_else(
      || "Unknown artist".to_string(),
      riff_core::util::strip_youtube_topic,
    )
  }

  /// `getTrackThumbUrl`, sans les pochettes personnalisées.
  #[must_use]
  pub fn thumb_url(&self) -> Option<String> {
    let text = |key: &str| {
      self
        .extra
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    };
    text("localThumbnail")
      .or_else(|| non_empty(self.thumbnail.as_ref()))
      .or_else(|| non_empty(self.cover.as_ref()))
      .or_else(|| text("image"))
      .or_else(|| text("artworkUrl"))
      .map(str::to_string)
      .or_else(|| {
        non_empty(self.id.as_ref())
          .filter(|id| is_youtube_id(id))
          .map(|id| format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"))
      })
  }

  /// `isTrackExplicit`, sans le cache par identifiant.
  #[must_use]
  pub fn is_explicit(&self) -> bool {
    let flag = |key: &str| {
      self
        .extra
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
    };
    if flag("isExplicit") || flag("explicit") || flag("explicit_lyrics") {
      return true;
    }
    let badges = self.extra.get("badges").and_then(Value::as_array);
    if badges.is_some_and(|list| {
      list.iter().any(|badge| {
        ["label", "name", "iconType"].iter().any(|key| {
          badge.get(key).and_then(Value::as_str).is_some_and(|text| {
            let lower = text.to_lowercase();
            lower.contains("explicit") || lower.contains("18+")
          })
        })
      })
    }) {
      return true;
    }
    let extra_text = |key: &str| self.extra.get(key).and_then(Value::as_str).unwrap_or("");
    let text = format!(
      "{} {} {} {}",
      self.title.as_deref().unwrap_or(""),
      self.album.as_deref().unwrap_or(""),
      self.search_query.as_deref().unwrap_or(""),
      extra_text("subtitle")
    );
    riff_core::util::is_text_explicit(&text)
  }
}

/// Playlist.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub id: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub name: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub description: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub cover: Option<String>,
  #[serde(default, deserialize_with = "de_vec_lenient")]
  pub tracks: Vec<Track>,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

/// Album enregistré, ou album à ouvrir.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAlbum {
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub id: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub title: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub artist: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub cover: Option<String>,
  #[serde(
    default,
    rename = "type",
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub kind: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub year: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_f64",
    serialize_with = "ser_f64",
    skip_serializing_if = "Option::is_none"
  )]
  pub track_count: Option<f64>,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

/// Artiste suivi. Riff accepte aussi une simple chaîne.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "FavoriteArtistRaw")]
pub struct FavoriteArtist {
  pub name: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub avatar: Option<String>,
  #[serde(serialize_with = "ser_f64", skip_serializing_if = "Option::is_none")]
  pub fans: Option<f64>,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FavoriteArtistRaw {
  Name(String),
  Full {
    #[serde(default, deserialize_with = "de_string")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_string")]
    avatar: Option<String>,
    #[serde(default, deserialize_with = "de_f64")]
    fans: Option<f64>,
    #[serde(flatten)]
    extra: Map<String, Value>,
  },
}

impl From<FavoriteArtistRaw> for FavoriteArtist {
  fn from(raw: FavoriteArtistRaw) -> Self {
    match raw {
      FavoriteArtistRaw::Name(name) => Self {
        name,
        ..Self::default()
      },
      FavoriteArtistRaw::Full {
        name,
        avatar,
        fans,
        extra,
      } => Self {
        name: name.unwrap_or_default(),
        avatar,
        fans,
        extra,
      },
    }
  }
}

/// Mode de tri d'une liste de pistes (`riff_favorites_sort`, `riff_playlist_sort`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
  #[default]
  #[serde(rename = "manual")]
  Manual,
  #[serde(rename = "added-desc")]
  AddedDesc,
  #[serde(rename = "added-asc")]
  AddedAsc,
}

/// Mix quotidien.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DailyMix {
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub id: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub title: Option<String>,
  #[serde(default)]
  pub artists: Vec<String>,
  #[serde(default, deserialize_with = "de_vec_lenient")]
  pub tracks: Vec<Track>,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

/// Mixes de la journée `day` (`riff_daily_mixes`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DailyMixes {
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub day: Option<String>,
  #[serde(default, deserialize_with = "de_vec_lenient")]
  pub mixes: Vec<DailyMix>,
}

/// Ligne de l'historique de recherche.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchEntry {
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub kind: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub key: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub title: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub subtitle: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub thumb: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub query: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub track: Option<Track>,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

impl SearchEntry {
  /// Vignette d'une ligne : `entry.thumb || getTrackThumbUrl(entry.track)` (`app.js:10165`).
  #[must_use]
  pub fn thumb_url(&self) -> Option<String> {
    non_empty(self.thumb.as_ref())
      .map(str::to_string)
      .or_else(|| self.track.as_ref().and_then(Track::thumb_url))
  }

  /// Pastille « artiste vérifié » (`entry.verified`).
  #[must_use]
  pub fn is_verified(&self) -> bool {
    self
      .extra
      .get("verified")
      .and_then(Value::as_bool)
      .unwrap_or(false)
  }

  /// Pastille « E » (`entry.explicit`, `app.js:10190`).
  #[must_use]
  pub fn is_explicit(&self) -> bool {
    self
      .extra
      .get("explicit")
      .and_then(Value::as_bool)
      .unwrap_or(false)
  }
}

/// Titre bloqué (`riff_blocked_tracks`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockedTrack {
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub id: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub key: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub title: Option<String>,
  #[serde(
    default,
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub artist: Option<String>,
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

/// Pistes mises en cache par l'accueil (`riff_my_wave_cache`, `riff_trending_cache`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackCache {
  #[serde(
    default,
    deserialize_with = "de_i64",
    skip_serializing_if = "Option::is_none"
  )]
  pub ts: Option<i64>,
  #[serde(default, deserialize_with = "de_vec_lenient")]
  pub tracks: Vec<Track>,
}

/// Contenu de `user-data.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UserData {
  pub wave_cache: Option<TrackCache>,
  pub trending_cache: Option<TrackCache>,
  pub favorites: Vec<Track>,
  pub playlists: Vec<Playlist>,
  pub history: Vec<Track>,
  pub local_files: Vec<Track>,
  pub removed_local_paths: Vec<String>,
  pub saved_albums: Vec<SavedAlbum>,
  pub favorite_artists: Vec<FavoriteArtist>,
  pub unfollowed_artists: Vec<String>,
  pub search_history: Vec<SearchEntry>,
  pub daily_mixes: Option<DailyMixes>,
  /// Identifiants canoniques épinglés, par portée (`favorites`, `pl:<id>`).
  pub pinned_tracks: Map<String, Value>,
  pub sidebar_pinned_tracks: Vec<Track>,
  pub sidebar_order: Vec<String>,
  pub sidebar_width: u32,
  pub favorites_sort: SortMode,
  pub playlist_sort: SortMode,
  /// Écart voulu avec Riff (demandé par Hugo) : nom, description et affiche des titres aimés, comme ceux d'une
  /// playlist. Clés propres à riff-rs, absentes de Riff : jamais lues ni écrites dans les clés de Riff.
  #[serde(
    rename = "riff_rs_liked_name",
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub liked_name: Option<String>,
  #[serde(
    rename = "riff_rs_liked_description",
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub liked_description: Option<String>,
  /// Chemin de l'image choisie ; `None` : cœur par défaut.
  #[serde(
    rename = "riff_rs_liked_cover",
    deserialize_with = "de_string",
    skip_serializing_if = "Option::is_none"
  )]
  pub liked_cover: Option<String>,
  /// Portées (`favorites`, `pl:<id>`) dont l'aléatoire est actif. Clé propre à riff-rs, omise si vide.
  #[serde(
    rename = "riff_rs_shuffle_lists",
    deserialize_with = "de_vec_lenient",
    skip_serializing_if = "Vec::is_empty"
  )]
  pub shuffle_lists: Vec<String>,
  pub blocked_tracks: Vec<BlockedTrack>,
  pub blocked_artists: Vec<String>,
  /// Titres renommés, par identifiant canonique.
  pub track_renames: Map<String, Value>,
  /// Découpes par titre (`riff_track_trims`).
  pub track_trims: Map<String, Value>,
  pub wave_skips: Vec<Value>,
  pub mix_feedback: Map<String, Value>,
  pub platform: String,
  pub shuffle: bool,
  pub repeat: bool,
  pub volume: f32,
  pub muted: bool,
  pub crossfade: u32,
  pub normalize: bool,
  pub search_artists: bool,
  pub search_pos: String,
  pub show_yt_platform: bool,
  pub autoplay: bool,
  pub like_album_tracks: bool,
  pub audio_quality: String,
}

impl Default for UserData {
  fn default() -> Self {
    Self {
      wave_cache: None,
      trending_cache: None,
      favorites: Vec::new(),
      playlists: Vec::new(),
      history: Vec::new(),
      local_files: Vec::new(),
      removed_local_paths: Vec::new(),
      saved_albums: Vec::new(),
      favorite_artists: Vec::new(),
      unfollowed_artists: Vec::new(),
      search_history: Vec::new(),
      daily_mixes: None,
      pinned_tracks: Map::new(),
      sidebar_pinned_tracks: Vec::new(),
      sidebar_order: Vec::new(),
      sidebar_width: crate::library::SIDEBAR_DEFAULT_WIDTH,
      favorites_sort: SortMode::default(),
      playlist_sort: SortMode::default(),
      liked_name: None,
      liked_description: None,
      liked_cover: None,
      shuffle_lists: Vec::new(),
      blocked_tracks: Vec::new(),
      blocked_artists: Vec::new(),
      track_renames: Map::new(),
      track_trims: Map::new(),
      wave_skips: Vec::new(),
      mix_feedback: Map::new(),
      platform: DEFAULT_PLATFORM.to_string(),
      shuffle: false,
      repeat: false,
      volume: DEFAULT_VOLUME,
      muted: false,
      crossfade: 0,
      normalize: false,
      search_artists: true,
      search_pos: "middle".to_string(),
      show_yt_platform: false,
      autoplay: true,
      like_album_tracks: false,
      audio_quality: "medium".to_string(),
    }
  }
}

/// Valeur JSON d'une clé : Riff range du JSON dans des chaînes. Illisible : `None`.
fn stored_json(storage: &LegacyStorage, key: &str) -> Option<Value> {
  match storage.get(key)? {
    Value::String(raw) => serde_json::from_str(raw).ok(),
    other => Some(other.clone()),
  }
}

fn stored_items<T: serde::de::DeserializeOwned>(storage: &LegacyStorage, key: &str) -> Vec<T> {
  stored_json(storage, key).map_or_else(Vec::new, items_from)
}

fn stored_map(storage: &LegacyStorage, key: &str) -> Map<String, Value> {
  match stored_json(storage, key) {
    Some(Value::Object(map)) => map,
    _ => Map::new(),
  }
}

fn stored_text<'a>(storage: &'a LegacyStorage, key: &str) -> Option<&'a str> {
  storage.get(key).and_then(Value::as_str)
}

fn stored_sort(storage: &LegacyStorage, key: &str) -> SortMode {
  match stored_text(storage, key) {
    Some("added-desc") => SortMode::AddedDesc,
    Some("added-asc") => SortMode::AddedAsc,
    _ => SortMode::Manual,
  }
}

fn stored_sidebar_width(storage: &LegacyStorage) -> u32 {
  if stored_text(storage, "riff_sidebar_default_wide_v2") != Some("1") {
    return crate::library::SIDEBAR_DEFAULT_WIDTH;
  }
  stored_text(storage, "riff_sidebar_width")
    .and_then(|value| value.trim().parse::<f32>().ok())
    .filter(|value| value.is_finite())
    .map_or(
      crate::library::SIDEBAR_DEFAULT_WIDTH,
      crate::library::snap_sidebar_width,
    )
}

impl UserData {
  /// Reprend les données de Riff. Une valeur illisible garde le défaut.
  #[must_use]
  pub fn imported_from(storage: &LegacyStorage, now_ms: i64) -> Self {
    let defaults = Self::default();
    let flag = |key: &str| stored_text(storage, key) == Some("1");
    let local_files = [
      "devsize_local_files",
      "riff_local_files",
      "devsize_local_tracks",
    ]
    .into_iter()
    .map(|key| stored_items::<Track>(storage, key))
    // Les trois clés sont lues : le suivi de `LegacyStorage` ne doit pas en laisser passer une.
    .collect::<Vec<_>>()
    .into_iter()
    .find(|files| !files.is_empty())
    .unwrap_or_default();
    let mut favorites: Vec<Track> = stored_items(storage, "devsize_favorites");
    track_sort::backfill_added_at(&mut favorites, true, now_ms);
    let mut playlists: Vec<Playlist> = stored_items(storage, "devsize_playlists");
    for playlist in &mut playlists {
      track_sort::backfill_added_at(&mut playlist.tracks, false, now_ms);
    }
    let track_cache = |key: &str| {
      stored_json(storage, key)
        .filter(Value::is_object)
        .and_then(|value| serde_json::from_value(value).ok())
    };
    Self {
      wave_cache: track_cache("riff_my_wave_cache"),
      trending_cache: track_cache("riff_trending_cache"),
      favorites,
      playlists,
      history: stored_items(storage, "devsize_history"),
      local_files,
      removed_local_paths: stored_items(storage, "riff_removed_local_paths"),
      saved_albums: stored_items(storage, "riff_saved_albums"),
      favorite_artists: stored_items(storage, "riff_favorite_artists"),
      unfollowed_artists: stored_items(storage, "riff_unfollowed_artists"),
      search_history: stored_items(storage, "riff_search_history_v2"),
      daily_mixes: stored_json(storage, "riff_daily_mixes")
        .filter(Value::is_object)
        .and_then(|value| serde_json::from_value(value).ok()),
      pinned_tracks: stored_map(storage, "riff_pinned_tracks"),
      sidebar_pinned_tracks: stored_items(storage, "riff_sidebar_pinned_tracks"),
      sidebar_order: stored_items(storage, "riff_sidebar_order"),
      sidebar_width: stored_sidebar_width(storage),
      favorites_sort: stored_sort(storage, "riff_favorites_sort"),
      playlist_sort: stored_sort(storage, "riff_playlist_sort"),
      liked_name: None,
      liked_description: None,
      liked_cover: None,
      shuffle_lists: Vec::new(),
      blocked_tracks: stored_items(storage, "riff_blocked_tracks"),
      blocked_artists: stored_items(storage, "riff_blocked_artists"),
      track_renames: stored_map(storage, "riff_track_renames"),
      track_trims: stored_map(storage, "riff_track_trims"),
      wave_skips: stored_items(storage, "riff_wave_skips"),
      mix_feedback: stored_map(storage, "riff_mix_feedback"),
      platform: match stored_text(storage, "riff_platform") {
        Some(platform @ ("ytmusic" | "soundcloud")) => platform.to_string(),
        _ => defaults.platform,
      },
      shuffle: flag("riff_shuffle"),
      repeat: flag("riff_repeat"),
      volume: stored_text(storage, "riff_volume")
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map_or(defaults.volume, |value| value.clamp(0.0, 1.0)),
      muted: flag("riff_muted"),
      crossfade: stored_text(storage, "riff_crossfade")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(defaults.crossfade),
      normalize: flag("riff_audio_normalize"),
      search_artists: stored_text(storage, "riff_search_artists") != Some("0"),
      search_pos: stored_text(storage, "riff_search_pos")
        .map_or(defaults.search_pos, str::to_string),
      show_yt_platform: flag("riff_show_yt_platform"),
      autoplay: stored_text(storage, "riff_adv_autoplay")
        .map_or(defaults.autoplay, |value| value == "true"),
      like_album_tracks: stored_text(storage, "riff_like_album_tracks") == Some("true"),
      audio_quality: stored_text(storage, "riff_audio_quality")
        .map_or(defaults.audio_quality, str::to_string),
    }
  }

  /// Clés de Riff, pour l'export de compte. Aucun jeton : ils restent dans le coffre.
  #[must_use]
  pub fn to_legacy_entries(&self) -> Map<String, Value> {
    fn encoded<T: Serialize>(value: &T) -> Value {
      Value::String(serde_json::to_string(value).unwrap_or_default())
    }
    let bit = |value: bool| Value::String(if value { "1" } else { "0" }.to_string());
    let word = |value: bool| Value::String(value.to_string());
    let text = |value: &str| Value::String(value.to_string());
    let mut out = Map::new();
    out.insert("devsize_favorites".to_string(), encoded(&self.favorites));
    out.insert("devsize_playlists".to_string(), encoded(&self.playlists));
    out.insert("devsize_history".to_string(), encoded(&self.history));
    out.insert("riff_local_files".to_string(), encoded(&self.local_files));
    out.insert(
      "riff_removed_local_paths".to_string(),
      encoded(&self.removed_local_paths),
    );
    out.insert("riff_saved_albums".to_string(), encoded(&self.saved_albums));
    out.insert(
      "riff_favorite_artists".to_string(),
      encoded(&self.favorite_artists),
    );
    out.insert(
      "riff_unfollowed_artists".to_string(),
      encoded(&self.unfollowed_artists),
    );
    out.insert(
      "riff_search_history_v2".to_string(),
      encoded(&self.search_history),
    );
    out.insert(
      "riff_pinned_tracks".to_string(),
      encoded(&self.pinned_tracks),
    );
    out.insert(
      "riff_sidebar_pinned_tracks".to_string(),
      encoded(&self.sidebar_pinned_tracks),
    );
    out.insert(
      "riff_sidebar_order".to_string(),
      encoded(&self.sidebar_order),
    );
    out.insert(
      "riff_sidebar_width".to_string(),
      Value::String(self.sidebar_width.to_string()),
    );
    out.insert(
      "riff_blocked_tracks".to_string(),
      encoded(&self.blocked_tracks),
    );
    out.insert(
      "riff_blocked_artists".to_string(),
      encoded(&self.blocked_artists),
    );
    out.insert(
      "riff_track_renames".to_string(),
      encoded(&self.track_renames),
    );
    out.insert("riff_track_trims".to_string(), encoded(&self.track_trims));
    out.insert(
      "riff_favorites_sort".to_string(),
      serde_json::to_value(self.favorites_sort).unwrap_or(Value::Null),
    );
    out.insert(
      "riff_playlist_sort".to_string(),
      serde_json::to_value(self.playlist_sort).unwrap_or(Value::Null),
    );
    out.insert("riff_platform".to_string(), text(&self.platform));
    out.insert("riff_shuffle".to_string(), bit(self.shuffle));
    out.insert("riff_repeat".to_string(), bit(self.repeat));
    out.insert(
      "riff_volume".to_string(),
      Value::String(self.volume.to_string()),
    );
    out.insert("riff_muted".to_string(), bit(self.muted));
    out.insert(
      "riff_crossfade".to_string(),
      Value::String(self.crossfade.to_string()),
    );
    out.insert("riff_audio_normalize".to_string(), bit(self.normalize));
    out.insert("riff_search_artists".to_string(), bit(self.search_artists));
    out.insert("riff_search_pos".to_string(), text(&self.search_pos));
    out.insert(
      "riff_show_yt_platform".to_string(),
      bit(self.show_yt_platform),
    );
    out.insert("riff_adv_autoplay".to_string(), word(self.autoplay));
    out.insert(
      "riff_like_album_tracks".to_string(),
      word(self.like_album_tracks),
    );
    out.insert("riff_audio_quality".to_string(), text(&self.audio_quality));
    out
  }

  /// Lit le fichier. Absent : import depuis `storage`, puis écriture. Corrompu : mis de côté, puis import.
  #[must_use]
  pub fn load_or_import(path: &Path, storage: &LegacyStorage, now_ms: i64) -> Self {
    match fs::read_to_string(path) {
      Ok(raw) => match serde_json::from_str(&raw) {
        Ok(data) => return data,
        Err(err) => {
          tracing::warn!(error = %err, "user-data.json illisible, mis de côté");
          if let Err(err) = fs::rename(path, path.with_extension("json.corrupt")) {
            tracing::warn!(error = %err, "mise de côté de user-data.json");
          }
        }
      },
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
      Err(err) => tracing::warn!(error = %err, "lecture de user-data.json"),
    }
    let data = Self::imported_from(storage, now_ms);
    if let Err(err) = data.save(path) {
      tracing::warn!(error = %err, "écriture de user-data.json");
    }
    data
  }

  /// Écrit le fichier sans jamais laisser un fichier à moitié écrit.
  ///
  /// # Errors
  /// Erreur du système de fichiers.
  pub fn save(&self, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(self)?)?;
    fs::rename(&tmp, path)
  }
}

/// Chemin de `user-data.json`.
#[must_use]
pub fn user_data_path() -> PathBuf {
  riff_core::paths::resolve_data_dir().join(FILE_NAME)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;
  use serde_json::json;

  const NOW: i64 = 1_800_000_000_000;

  fn temp_dir() -> TempDir {
    TempDir::new("app-user-data")
  }

  fn storage_of(value: &Value) -> (TempDir, LegacyStorage) {
    let dir = temp_dir();
    let path = dir.join("storage.json");
    fs::write(&path, value.to_string()).unwrap();
    let storage = LegacyStorage::open(&path);
    (dir, storage)
  }

  /// Fixture écrite à la main : valeurs JSON dans une chaîne, comme le vidage de `localStorage`.
  fn fixture() -> Value {
    json!({
      "devsize_favorites": json!([
        { "id": "a", "title": "Un", "artist": "X", "duration": 200, "addedAt": 111, "platform": "ytmusic", "videoFoo": { "bar": [1, 2] } },
        { "id": 42, "title": "Deux", "artist": "Y" }
      ]).to_string(),
      "devsize_playlists": json!([
        { "id": "pl_1", "name": "Ma liste", "description": "Texte", "cover": "c.png", "couleur": "bleu",
          "tracks": [ { "id": "b", "title": "B" }, { "id": "c", "title": "C", "addedAt": 5 } ] }
      ]).to_string(),
      "devsize_history": json!([ { "id": "h", "title": "H", "playedAt": 99 } ]).to_string(),
      "devsize_local_files": json!([ { "id": "l", "path": "D:\\m\\a.mp3", "platform": "local", "trackNumber": 3 } ]).to_string(),
      "riff_removed_local_paths": json!(["D:\\m\\z.mp3"]).to_string(),
      "riff_saved_albums": json!([ { "id": "al", "title": "T", "type": "Album", "year": "2020", "trackCount": 9 } ]).to_string(),
      "riff_favorite_artists": json!([ "Nom simple", { "name": "Complet", "avatar": "a.png", "fans": 12, "plays": 3 } ]).to_string(),
      "riff_unfollowed_artists": json!(["Parti"]).to_string(),
      "riff_search_history_v2": json!([ { "kind": "query", "key": "q:un", "title": "un", "ts": 7 } ]).to_string(),
      "riff_daily_mixes": json!({ "day": "2026-11-03", "mixes": [ { "id": "daily-mix-1", "title": "Daily Mix 1", "artists": ["X"], "tracks": [ { "id": "m" } ] } ] }).to_string(),
      "riff_pinned_tracks": json!({ "favorites": ["ytmusic:a"] }).to_string(),
      "riff_sidebar_pinned_tracks": json!([ { "id": "p", "title": "P" } ]).to_string(),
      "riff_sidebar_order": json!(["queue", "favorites"]).to_string(),
      "riff_favorites_sort": "added-desc",
      "riff_playlist_sort": "added-asc",
      "riff_blocked_tracks": json!([ { "id": "bl", "title": "Bloque" } ]).to_string(),
      "riff_blocked_artists": json!(["Bloqué"]).to_string(),
      "riff_track_renames": json!({ "ytmusic:a": { "title": "Nouveau" } }).to_string(),
      "riff_track_trims": json!({ "ytmusic:a": { "start": 5, "end": 50 } }).to_string(),
      "riff_wave_skips": json!([ { "key": "k", "artist": "X", "t": 1 } ]).to_string(),
      "riff_mix_feedback": json!({ "ratingsByMix": {} }).to_string(),
      "riff_platform": "soundcloud",
      "riff_shuffle": "1",
      "riff_repeat": "1",
      "riff_volume": "0.5",
      "riff_muted": "1",
      "riff_crossfade": "6",
      "riff_audio_normalize": "1",
      "riff_search_artists": "0",
      "riff_search_pos": "left",
      "riff_show_yt_platform": "1",
      "riff_adv_autoplay": "false",
      "riff_like_album_tracks": "true",
      "riff_audio_quality": "high",
      "riff_my_wave_cache": json!({ "ts": 5, "tracks": [ { "id": "w" } ] }).to_string(),
      "riff_trending_cache": json!({ "ts": 6, "tracks": [ { "id": "t" } ] }).to_string(),
      "riff_deepl_api_key": "jeton-de-test"
    })
  }

  #[test]
  fn the_sidebar_width_is_imported_when_riff_has_saved_one() {
    let width = |value: &str| {
      let (_path, storage) =
        storage_of(&json!({ "riff_sidebar_default_wide_v2": "1", "riff_sidebar_width": value }));
      UserData::imported_from(&storage, NOW).sidebar_width
    };
    assert_eq!(width("84"), 84);
    assert_eq!(width("300"), 300);
    assert_eq!(width("9999"), 390);
    assert_eq!(width("rien"), 288);
    assert_eq!(width("NaN"), 288);
    let (_path, storage) = storage_of(&json!({ "riff_sidebar_width": "84" }));
    assert_eq!(
      UserData::imported_from(&storage, NOW).sidebar_width,
      288,
      "sans le drapeau de Riff : largeur par défaut"
    );
  }

  #[test]
  fn an_old_data_file_gets_the_default_sidebar_width() {
    let data: UserData = serde_json::from_value(json!({})).unwrap();
    assert_eq!(data.sidebar_width, 288);
  }

  #[test]
  fn the_canonical_id_uses_the_id_then_the_url_then_the_text() {
    let by_id = Track {
      id: Some("a".into()),
      platform: Some("soundcloud".into()),
      ..Track::default()
    };
    assert_eq!(by_id.canonical_id("ytmusic"), "soundcloud:a");
    let by_url = Track {
      url: Some("u".into()),
      ..Track::default()
    };
    assert_eq!(by_url.canonical_id("ytmusic"), "ytmusic:u");
    let by_text = Track {
      title: Some(" T ".into()),
      artist: Some("A".into()),
      duration: Some(9.6),
      ..Track::default()
    };
    assert_eq!(by_text.canonical_id("ytmusic"), "ytmusic:t|a|10");
    assert!(by_id.is_same(&by_id.clone(), "ytmusic"));
    assert!(!by_id.is_same(&by_url, "ytmusic"));
  }

  #[test]
  fn the_thumbnail_falls_back_on_the_youtube_id() {
    let track = Track {
      id: Some("dQw4w9WgXcQ".into()),
      ..Track::default()
    };
    assert_eq!(
      track.thumb_url().as_deref(),
      Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
    );
    let cover = Track {
      cover: Some("c".into()),
      ..track.clone()
    };
    assert_eq!(cover.thumb_url().as_deref(), Some("c"));
    assert_eq!(
      Track {
        id: Some("alb_123_456".into()),
        ..Track::default()
      }
      .thumb_url(),
      None
    );
  }

  #[test]
  fn explicit_tracks_are_recognized() {
    let mut flagged = Track::default();
    flagged.extra.insert("isExplicit".into(), json!(true));
    assert!(flagged.is_explicit());
    assert!(
      Track {
        title: Some("Song (Explicit)".into()),
        ..Track::default()
      }
      .is_explicit()
    );
    let mut badge = Track::default();
    badge
      .extra
      .insert("badges".into(), json!([{ "label": "Explicit" }]));
    assert!(badge.is_explicit());
    assert!(
      !Track {
        title: Some("Song".into()),
        ..Track::default()
      }
      .is_explicit()
    );
  }

  #[test]
  fn favorites_keep_their_fields_and_unknown_ones() {
    let (_, storage) = storage_of(&fixture());
    let data = UserData::imported_from(&storage, NOW);
    assert_eq!(data.favorites.len(), 2);
    let first = &data.favorites[0];
    assert_eq!(first.id.as_deref(), Some("a"));
    assert_eq!(first.duration, Some(200.0));
    assert_eq!(first.added_at, Some(111));
    assert_eq!(first.extra.get("videoFoo"), Some(&json!({ "bar": [1, 2] })));
    assert_eq!(data.favorites[1].id.as_deref(), Some("42"));
  }

  #[test]
  fn missing_added_at_is_backfilled_like_riff() {
    let (_, storage) = storage_of(&fixture());
    let data = UserData::imported_from(&storage, NOW);
    assert_eq!(data.favorites[1].added_at, Some(NOW - 1000));
    let tracks = &data.playlists[0].tracks;
    assert_eq!(tracks[0].added_at, Some(NOW - 2000));
    assert_eq!(tracks[1].added_at, Some(5));
  }

  #[test]
  fn every_listed_key_is_imported() {
    let (_, storage) = storage_of(&fixture());
    let data = UserData::imported_from(&storage, NOW);
    assert_eq!(data.playlists[0].name.as_deref(), Some("Ma liste"));
    assert_eq!(data.playlists[0].extra.get("couleur"), Some(&json!("bleu")));
    assert_eq!(data.history[0].played_at, Some(99));
    assert_eq!(
      data.local_files[0].extra.get("trackNumber"),
      Some(&json!(3))
    );
    assert_eq!(data.removed_local_paths, ["D:\\m\\z.mp3"]);
    assert_eq!(data.saved_albums[0].track_count, Some(9.0));
    assert_eq!(data.favorite_artists[0].name, "Nom simple");
    assert_eq!(data.favorite_artists[1].fans, Some(12.0));
    assert_eq!(data.favorite_artists[1].extra.get("plays"), Some(&json!(3)));
    assert_eq!(data.unfollowed_artists, ["Parti"]);
    assert_eq!(data.search_history[0].key.as_deref(), Some("q:un"));
    assert_eq!(
      data.daily_mixes.as_ref().unwrap().mixes[0].tracks[0]
        .id
        .as_deref(),
      Some("m")
    );
    assert_eq!(
      data.pinned_tracks.get("favorites"),
      Some(&json!(["ytmusic:a"]))
    );
    assert_eq!(data.sidebar_pinned_tracks[0].id.as_deref(), Some("p"));
    assert_eq!(data.sidebar_order, ["queue", "favorites"]);
    assert_eq!(data.favorites_sort, SortMode::AddedDesc);
    assert_eq!(data.playlist_sort, SortMode::AddedAsc);
    assert_eq!(data.blocked_tracks[0].id.as_deref(), Some("bl"));
    assert_eq!(data.blocked_artists, ["Bloqué"]);
    assert!(data.track_renames.contains_key("ytmusic:a"));
    assert!(data.track_trims.contains_key("ytmusic:a"));
    assert_eq!(data.wave_skips.len(), 1);
    assert!(data.mix_feedback.contains_key("ratingsByMix"));
    assert_eq!(data.platform, "soundcloud");
    assert!(data.shuffle && data.repeat && data.muted && data.normalize);
    assert_eq!(data.volume, 0.5);
    assert_eq!(data.crossfade, 6);
    assert_eq!(
      data.sidebar_width, 288,
      "sans riff_sidebar_default_wide_v2 : largeur par défaut"
    );
    assert!(!data.search_artists);
    assert_eq!(data.search_pos, "left");
    assert!(data.show_yt_platform);
    assert!(!data.autoplay);
    assert!(data.like_album_tracks);
    assert_eq!(data.audio_quality, "high");
    assert_eq!(
      data.wave_cache.as_ref().unwrap().tracks[0].id.as_deref(),
      Some("w")
    );
    assert_eq!(data.trending_cache.as_ref().unwrap().ts, Some(6));
  }

  #[test]
  fn the_manual_order_is_kept() {
    let (_, storage) = storage_of(&json!({
      "devsize_favorites": json!([ { "id": "z", "addedAt": 1 }, { "id": "a", "addedAt": 3 }, { "id": "m", "addedAt": 2 } ]).to_string()
    }));
    let ids: Vec<_> = UserData::imported_from(&storage, NOW)
      .favorites
      .iter()
      .map(|t| t.id.clone().unwrap())
      .collect();
    assert_eq!(ids, ["z", "a", "m"]);
  }

  #[test]
  fn local_files_fall_back_on_the_older_keys() {
    let (_, storage) =
      storage_of(&json!({ "riff_local_files": json!([ { "id": "r" } ]).to_string() }));
    assert_eq!(
      UserData::imported_from(&storage, NOW).local_files[0]
        .id
        .as_deref(),
      Some("r")
    );
    let (_, storage) =
      storage_of(&json!({ "devsize_local_tracks": json!([ { "id": "d" } ]).to_string() }));
    assert_eq!(
      UserData::imported_from(&storage, NOW).local_files[0]
        .id
        .as_deref(),
      Some("d")
    );
  }

  #[test]
  fn a_corrupt_value_keeps_the_default_without_panic() {
    let (_, storage) = storage_of(&json!({
      "devsize_favorites": "{ pas du json",
      "devsize_playlists": "12",
      "devsize_history": "[1, \"x\", { \"id\": \"garde\" }]",
      "riff_volume": "beaucoup",
      "riff_crossfade": "-3",
      "riff_favorites_sort": "pas un tri",
      "riff_daily_mixes": "[]",
      "riff_platform": 12
    }));
    let data = UserData::imported_from(&storage, NOW);
    assert!(data.favorites.is_empty());
    assert!(data.playlists.is_empty());
    assert_eq!(data.history.len(), 1);
    assert_eq!(data.volume, DEFAULT_VOLUME);
    assert_eq!(data.crossfade, 0);
    assert_eq!(data.sidebar_width, 288);
    assert_eq!(data.favorites_sort, SortMode::Manual);
    assert_eq!(data.platform, DEFAULT_PLATFORM);
  }

  #[test]
  fn the_old_youtube_platform_becomes_youtube_music() {
    let (_, storage) = storage_of(&json!({ "riff_platform": "youtube" }));
    assert_eq!(
      UserData::imported_from(&storage, NOW).platform,
      DEFAULT_PLATFORM
    );
  }

  #[test]
  fn an_empty_store_gives_the_defaults() {
    assert_eq!(
      UserData::imported_from(&LegacyStorage::default(), NOW),
      UserData::default()
    );
  }

  #[test]
  fn a_round_trip_loses_nothing() {
    let (dir, storage) = storage_of(&fixture());
    let path = dir.join(FILE_NAME);
    let data = UserData::imported_from(&storage, NOW);
    data.save(&path).unwrap();
    let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw["favorites"][0]["duration"], json!(200));
    assert_eq!(raw["favorites"][0]["videoFoo"], json!({ "bar": [1, 2] }));
    assert_eq!(raw["savedAlbums"][0]["type"], json!("Album"));
    assert_eq!(
      UserData::load_or_import(&path, &LegacyStorage::default(), NOW),
      data
    );
  }

  #[test]
  fn first_launch_imports_writes_and_never_touches_the_store() {
    let (dir, storage) = storage_of(&fixture());
    let store = dir.join("storage.json");
    let before = (
      fs::read(&store).unwrap(),
      fs::metadata(&store).unwrap().modified().unwrap(),
    );
    let path = dir.join(FILE_NAME);
    let data = UserData::load_or_import(&path, &storage, NOW);
    assert_eq!(data.favorites.len(), 2);
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("Ma liste"));
    assert!(!written.contains("jeton-de-test"));
    assert!(!dir.join("user-data.json.tmp").exists());
    assert_eq!(
      before,
      (
        fs::read(&store).unwrap(),
        fs::metadata(&store).unwrap().modified().unwrap()
      )
    );
  }

  #[test]
  fn the_import_only_happens_once() {
    let (dir, storage) = storage_of(&fixture());
    let path = dir.join(FILE_NAME);
    let mut data = UserData::load_or_import(&path, &storage, NOW);
    data.favorites.clear();
    data.save(&path).unwrap();
    assert!(
      UserData::load_or_import(&path, &storage, NOW)
        .favorites
        .is_empty()
    );
  }

  #[test]
  fn a_corrupt_file_is_kept_aside_and_the_store_is_imported_again() {
    let (dir, storage) = storage_of(&fixture());
    let path = dir.join(FILE_NAME);
    fs::write(&path, "{ pas du json").unwrap();
    let data = UserData::load_or_import(&path, &storage, NOW);
    assert_eq!(data.favorites.len(), 2);
    assert_eq!(
      fs::read_to_string(dir.join("user-data.json.corrupt")).unwrap(),
      "{ pas du json"
    );
  }

  #[test]
  fn the_liked_name_description_and_cover_survive_a_save_and_a_reload() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    let data = UserData {
      liked_name: Some("Mes coups de coeur".to_string()),
      liked_description: Some("Ceux que j'aime".to_string()),
      liked_cover: Some("C:/images/affiche.png".to_string()),
      ..UserData::default()
    };
    data.save(&path).unwrap();
    let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw["riff_rs_liked_name"], json!("Mes coups de coeur"));
    assert_eq!(raw["riff_rs_liked_description"], json!("Ceux que j'aime"));
    assert_eq!(raw["riff_rs_liked_cover"], json!("C:/images/affiche.png"));
    let loaded = UserData::load_or_import(&path, &LegacyStorage::default(), NOW);
    assert_eq!(loaded, data);
    assert_eq!(loaded.liked_custom_name(), Some("Mes coups de coeur"));
    assert_eq!(loaded.liked_custom_cover(), Some("C:/images/affiche.png"));
  }

  #[test]
  fn without_a_custom_liked_name_the_fallback_applies_and_nothing_is_written() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    let mut data = UserData::default();
    assert_eq!(data.liked_custom_name(), None);
    data.save(&path).unwrap();
    let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(raw.get("riff_rs_liked_name").is_none());
    assert!(raw.get("riff_rs_liked_cover").is_none());
    // Un nom vide ou en blanc (champ effacé) donne aussi le libellé de repli.
    data.liked_name = Some(String::new());
    assert_eq!(data.liked_custom_name(), None);
    data.liked_name = Some("   ".to_string());
    assert_eq!(data.liked_custom_name(), None);
    data.liked_cover = Some(" ".to_string());
    assert_eq!(data.liked_custom_cover(), None);
  }

  #[test]
  fn a_file_without_the_liked_keys_and_the_riff_import_keep_the_defaults() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    fs::write(&path, r#"{"favorites":[{"id":"a"}]}"#).unwrap();
    let loaded = UserData::load_or_import(&path, &LegacyStorage::default(), NOW);
    assert_eq!(loaded.favorites.len(), 1);
    assert_eq!(loaded.liked_name, None);
    let (_, storage) = storage_of(&fixture());
    let imported = UserData::imported_from(&storage, NOW);
    assert_eq!(imported.liked_name, None);
    assert_eq!(imported.liked_description, None);
    assert_eq!(imported.liked_cover, None);
    let exported = imported.to_legacy_entries();
    assert!(exported.keys().all(|key| !key.starts_with("riff_rs_")));
  }

  #[test]
  fn the_shuffle_lists_survive_a_save_and_a_reload_and_stay_out_of_the_riff_export() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    let data = UserData {
      shuffle_lists: vec!["favorites".to_string(), "pl:abc".to_string()],
      ..UserData::default()
    };
    data.save(&path).unwrap();
    let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw["riff_rs_shuffle_lists"], json!(["favorites", "pl:abc"]));
    let loaded = UserData::load_or_import(&path, &LegacyStorage::default(), NOW);
    assert_eq!(loaded, data);
    assert!(
      data
        .to_legacy_entries()
        .keys()
        .all(|key| !key.starts_with("riff_rs_"))
    );
  }

  #[test]
  fn an_empty_shuffle_set_is_not_written_and_a_bad_value_is_ignored() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    UserData::default().save(&path).unwrap();
    let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(raw.get("riff_rs_shuffle_lists").is_none());
    fs::write(&path, r#"{"riff_rs_shuffle_lists":["pl:a",3,null]}"#).unwrap();
    let loaded = UserData::load_or_import(&path, &LegacyStorage::default(), NOW);
    assert_eq!(loaded.shuffle_lists, ["pl:a"]);
    fs::write(&path, r#"{"riff_rs_shuffle_lists":"oops"}"#).unwrap();
    let loaded = UserData::load_or_import(&path, &LegacyStorage::default(), NOW);
    assert!(loaded.shuffle_lists.is_empty());
  }
}
