//! Opérations sur les données de l'utilisateur, sans E/S.

use crate::track_sort;
use crate::user_data::{
  BlockedTrack, FavoriteArtist, Playlist, SavedAlbum, SearchEntry, Track, UserData,
};
use regex::Regex;
use riff_core::util::{get_track_normalized_key, strip_youtube_topic};
use serde_json::{Map, Value, json};
use std::collections::hash_map::Entry as MapEntry;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const HISTORY_MAX: usize = 100;
const SEARCH_HISTORY_MAX: usize = 25;
const SIDEBAR_PINS_MAX: usize = 50;
const WAVE_SKIPS_MAX: usize = 200;
pub const FAVORITES_SCOPE: &str = "favorites";

static FEATURING_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)\s*[\(\[]\s*(?:feat\.?|ft\.?|featuring)\s[^\)\]]*[\)\]]").expect("regex valide")
});
static FEATURING_TAIL: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)\s+(?:feat\.?|ft\.?|featuring)\s+.+$").expect("regex valide"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("regex valide"));
static NOT_WORD: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}\s]").expect("regex valide"));
static ARTIST_NOISE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)\b(official|vevo|records|topic)\b").expect("regex valide"));
static ARTIST_SYMBOLS: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"[^\w\s&-]").expect("regex valide"));
static ARTIST_FEAT_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)\s*[\(\[]\s*(?:feat\.?|ft\.?|featuring|prod\.?\s*by)\s+([^\)\]]+)[\)\]]")
    .expect("regex valide")
});
static ARTIST_SEPARATOR: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)\s+(?:feat\.?|ft\.?|featuring|with|vs\.?|\x{00d7}|x)\s+|\s*(?:&|,|;|/|\+)\s*")
    .expect("regex valide")
});
static AUTOPLAY_SEPARATOR: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i),|&| feat\.? | ft\.? | x ").expect("regex valide"));
static ARTIST_NAME_NOISE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i) - Topic|VEVO|Official|Records").expect("regex valide"));
const MULTI_ARTIST_EXCEPTIONS: [&str; 26] = [
  "simon & garfunkel",
  "hall & oates",
  "brooks & dunn",
  "tegan and sara",
  "earth, wind & fire",
  "crosby, stills & nash",
  "crosby, stills, nash & young",
  "mumford & sons",
  "florence + the machine",
  "florence & the machine",
  "belle & sebastian",
  "belle and sebastian",
  "emerson, lake & palmer",
  "kc & the sunshine band",
  "kool & the gang",
  "sly & the family stone",
  "echo & the bunnymen",
  "coheed and cambria",
  "angus & julia stone",
  "above & beyond",
  "chase & status",
  "gnarls barkley",
  "twenty one pilots",
  "ac/dc",
  "t.a.t.u.",
  "tyler, the creator",
];

/// `normalizeArtistName`.
#[must_use]
pub fn normalize_artist_name(name: &str) -> String {
  let text = ARTIST_NOISE.replace_all(name, "");
  let text = ARTIST_SYMBOLS.replace_all(&text, " ");
  SPACES.replace_all(&text, " ").trim().to_lowercase()
}

/// `normalizeMatchText`.
#[must_use]
pub fn normalize_match_text(text: &str) -> String {
  let lower = text.to_lowercase();
  let kept = NOT_WORD.replace_all(&lower, "");
  SPACES.replace_all(&kept, " ").trim().to_string()
}

/// `stripFeaturing`.
#[must_use]
pub fn strip_featuring(title: &str) -> String {
  let text = FEATURING_BRACKET.replace_all(title, "");
  let text = FEATURING_TAIL.replace(&text, "");
  SPACES.replace_all(&text, " ").trim().to_string()
}

/// Nom d'artiste nettoyé à l'ouverture de sa page (`openArtistProfile`).
#[must_use]
pub fn clean_artist_name(name: &str) -> String {
  ARTIST_NAME_NOISE.replace_all(name, "").trim().to_string()
}

/// Artistes d'un texte, séparateurs retirés (`splitArtistTokens`).
#[must_use]
pub fn split_artists(raw: &str) -> Vec<String> {
  let raw = strip_youtube_topic(raw);
  let text = raw.trim();
  if text.is_empty() {
    return vec!["Unknown artist".to_string()];
  }
  if MULTI_ARTIST_EXCEPTIONS.contains(&text.to_lowercase().as_str()) {
    return vec![text.to_string()];
  }
  let clean = ARTIST_FEAT_BRACKET.replace_all(text, " feat. $1");
  let parts: Vec<String> = ARTIST_SEPARATOR
    .split(&clean)
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .map(str::to_string)
    .collect();
  if parts.is_empty() {
    vec![text.to_string()]
  } else {
    parts
  }
}

/// Morceau d'un texte d'artistes : un nom ou le séparateur d'origine, espaces compris.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtistToken {
  Artist(String),
  Sep(String),
}

/// Noms et séparateurs d'un texte d'artistes, tels que Riff les affiche (`splitArtistTokens`, `app.js:11569`).
#[must_use]
pub fn split_artist_tokens(raw: &str) -> Vec<ArtistToken> {
  let raw = strip_youtube_topic(raw);
  let text = raw.trim();
  if text.is_empty() {
    return vec![ArtistToken::Artist("Unknown artist".to_string())];
  }
  if MULTI_ARTIST_EXCEPTIONS.contains(&text.to_lowercase().as_str()) {
    return vec![ArtistToken::Artist(text.to_string())];
  }
  let clean = ARTIST_FEAT_BRACKET.replace_all(text, " feat. $1");
  let mut tokens = Vec::new();
  let mut last = 0;
  let push_name = |part: &str, tokens: &mut Vec<ArtistToken>| {
    let name = part.trim();
    if !name.is_empty() {
      tokens.push(ArtistToken::Artist(name.to_string()));
    }
  };
  for separator in ARTIST_SEPARATOR.find_iter(&clean) {
    push_name(&clean[last..separator.start()], &mut tokens);
    tokens.push(ArtistToken::Sep(separator.as_str().to_string()));
    last = separator.end();
  }
  push_name(&clean[last..], &mut tokens);
  // Un seul nom : le texte d'origine (`parts.length <= 1`).
  if tokens
    .iter()
    .filter(|token| matches!(token, ArtistToken::Artist(_)))
    .count()
    <= 1
    && !tokens
      .iter()
      .any(|token| matches!(token, ArtistToken::Sep(_)))
  {
    return vec![ArtistToken::Artist(text.to_string())];
  }
  tokens
}

/// `autoplayArtistList`.
#[must_use]
pub fn autoplay_artists(artist: &str) -> Vec<String> {
  AUTOPLAY_SEPARATOR
    .split(artist)
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .map(str::to_string)
    .collect()
}

/// `rankSearchTracks` : les correspondances exactes d'abord.
#[must_use]
pub fn rank_search_tracks(query: &str, tracks: Vec<Track>) -> Vec<Track> {
  let wanted = normalize_match_text(&strip_featuring(query));
  if tracks.len() < 2 || wanted.is_empty() {
    return tracks;
  }
  let (exact, rest): (Vec<_>, Vec<_>) = tracks.into_iter().partition(|track| {
    let title = normalize_match_text(&strip_featuring(track.title.as_deref().unwrap_or("")));
    let artist = normalize_match_text(track.artist.as_deref().unwrap_or(""));
    title == wanted
      || (!artist.is_empty()
        && (wanted == format!("{artist} {title}") || wanted == format!("{title} {artist}")))
  });
  exact.into_iter().chain(rest).collect()
}

/// Largeur de la barre latérale dépliée par défaut.
pub const SIDEBAR_DEFAULT_WIDTH: u32 = 288;
/// Largeur de la barre latérale repliée.
pub const SIDEBAR_COLLAPSED_WIDTH: u32 = 84;
const SIDEBAR_EXPAND_THRESHOLD: f32 = 180.0;
const SIDEBAR_MIN_WIDTH: f32 = 256.0;
const SIDEBAR_MAX_WIDTH: f32 = 390.0;

/// `applySidebarWidth`.
#[must_use]
pub fn snap_sidebar_width(width: f32) -> u32 {
  if width >= SIDEBAR_EXPAND_THRESHOLD {
    width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH).round() as u32
  } else {
    SIDEBAR_COLLAPSED_WIDTH
  }
}

/// `formatDuration`.
#[must_use]
pub fn format_duration(seconds: f64) -> String {
  if !seconds.is_finite() || seconds <= 0.0 {
    return "0:00".to_string();
  }
  let total = seconds as u64;
  format!("{}:{:02}", total / 60, total % 60)
}

/// Durée totale d'une liste, lisible : `28 hr 7 min`, `4 min 5 sec`, `59 sec`. Les unités nulles de fin sont omises.
/// Vide si la durée n'est pas finie ou nulle.
#[must_use]
pub fn format_total_duration(seconds: f64) -> String {
  if !seconds.is_finite() || seconds < 1.0 {
    return String::new();
  }
  let total = seconds as u64;
  let (hours, minutes, secs) = (total / 3600, total % 3600 / 60, total % 60);
  match (hours, minutes, secs) {
    (0, 0, s) => format!("{s} sec"),
    (0, m, 0) => format!("{m} min"),
    (0, m, s) => format!("{m} min {s} sec"),
    (h, 0, _) => format!("{h} hr"),
    (h, m, _) => format!("{h} hr {m} min"),
  }
}

/// Lien de partage : l'adresse directe du titre (page YouTube, SoundCloud...), jamais un service tiers. Pas d'adresse connue
/// (fichier local, flux sur 127.0.0.1) : `None`, rien n'est copié. L'URL du flux local est exclue : elle contient l'adresse IP de l'utilisateur.
#[must_use]
pub fn share_url(track: &Track) -> Option<String> {
  let is_http = |url: &str| {
    (url.starts_with("http://") || url.starts_with("https://")) && !url.contains("127.0.0.1")
  };
  track.url.clone().filter(|url| is_http(url)).or_else(|| {
    track
      .id
      .as_deref()
      .filter(|id| {
        !id.is_empty() && !matches!(track.platform.as_deref(), Some("soundcloud" | "local"))
      })
      .map(|id| format!("https://www.youtube.com/watch?v={id}"))
  })
}

/// Export d'une playlist (`exportPlaylistsAsJson`, `formatTrackForExport`).
#[must_use]
pub fn playlist_export(playlist: &Playlist, exported_at: &str) -> Value {
  tracks_export(
    playlist.name.as_deref().unwrap_or("untitled"),
    playlist.description.as_deref().unwrap_or(""),
    &playlist.tracks,
    exported_at,
  )
}

fn non_blank(text: Option<&str>) -> Option<&str> {
  text.map(str::trim).filter(|text| !text.is_empty())
}

/// Même format pour une liste de titres hors playlist (« Liked Songs »).
#[must_use]
pub fn tracks_export(name: &str, description: &str, tracks: &[Track], exported_at: &str) -> Value {
  let basename = |value: &str| {
    let clean = value.trim_start_matches("file://").replace('\\', "/");
    clean
      .rsplit('/')
      .find(|part| !part.is_empty())
      .unwrap_or(value)
      .to_string()
  };
  let tracks: Vec<Value> = tracks
    .iter()
    .map(|track| {
      let local = track.is_local();
      let pick = |value: &Option<String>| {
        let value = value.as_deref().unwrap_or("");
        if local {
          basename(value)
        } else {
          value.to_string()
        }
      };
      let cover = track
        .cover
        .as_deref()
        .or(track.thumbnail.as_deref())
        .unwrap_or("");
      let cover =
        if cover.starts_with("file:") || cover.starts_with('/') || cover.get(1..2) == Some(":") {
          basename(cover)
        } else {
          cover.to_string()
        };
      json!({
        "title": track.title.as_deref().unwrap_or(""),
        "artist": track.artist.as_deref().unwrap_or(""),
        "album": track.album.as_deref().unwrap_or(""),
        "duration": track.duration.unwrap_or(0.0),
        "id": pick(&track.id),
        "url": pick(&track.url),
        "platform": track.platform.as_deref().unwrap_or(""),
        "cover": cover,
      })
    })
    .collect();
  json!({
    "app": "Riff",
    "appVersion": "44.1.1",
    "exportedAt": exported_at,
    "playlists": [{
      "name": name,
      "description": description,
      "tracks": tracks,
    }],
  })
}

/// Nom de fichier proposé à l'export.
#[must_use]
pub fn safe_file_name(name: &str) -> String {
  name
    .chars()
    .map(|c| if "/\\?%*:|\"<>".contains(c) { '_' } else { c })
    .collect()
}

/// Jour local au format de Riff, depuis les millisecondes et le décalage du fuseau.
#[must_use]
pub fn local_day(now_ms: i64, offset_minutes: i64) -> String {
  let days = (now_ms + offset_minutes * 60_000).div_euclid(86_400_000);
  // Conversion jours vers date civile (algorithme de Howard Hinnant).
  let z = days + 719_468;
  let era = z.div_euclid(146_097);
  let doe = z.rem_euclid(146_097);
  let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let day = doy - (153 * mp + 2) / 5 + 1;
  let month = if mp < 10 { mp + 3 } else { mp - 9 };
  let year = yoe + era * 400 + i64::from(month <= 2);
  format!("{year}-{month:02}-{day:02}")
}

fn value_text<'a>(value: &'a Value, key: &str) -> &'a str {
  value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn track_key(track: &Track) -> String {
  get_track_normalized_key(
    track.artist.as_deref().unwrap_or(""),
    track.title.as_deref().unwrap_or(""),
  )
}

impl UserData {
  #[must_use]
  pub fn is_favorite(&self, track: &Track) -> bool {
    self
      .favorites
      .iter()
      .any(|favorite| favorite.is_same(track, &self.platform))
  }

  /// `toggleFavoriteTrack`. Renvoie `true` si le titre rejoint les favoris (et sort des titres bloqués).
  pub fn toggle_favorite(&mut self, track: &Track, now_ms: i64) -> bool {
    if self.is_favorite(track) {
      let platform = self.platform.clone();
      self
        .favorites
        .retain(|favorite| !favorite.is_same(track, &platform));
      false
    } else {
      self.unblock_track(track);
      self.favorites.insert(
        0,
        Track {
          added_at: Some(now_ms),
          ..track.clone()
        },
      );
      true
    }
  }

  /// Ajoute ceux qui manquent (sélection). Renvoie le nombre ajouté. Un titre aimé n'est plus bloqué.
  pub fn add_favorites(&mut self, tracks: &[Track], now_ms: i64) -> usize {
    let mut added = 0;
    for track in tracks {
      if !self.is_favorite(track) {
        self.unblock_track(track);
        self.favorites.insert(
          0,
          Track {
            added_at: Some(now_ms),
            ..track.clone()
          },
        );
        added += 1;
      }
    }
    added
  }

  /// Le titre passe en tête de l'historique (`playTrack`).
  pub fn record_played(&mut self, track: &Track, now_ms: i64) {
    let platform = self.platform.clone();
    self
      .history
      .retain(|entry| !entry.is_same(track, &platform));
    self.history.insert(
      0,
      Track {
        played_at: Some(now_ms),
        ..track.clone()
      },
    );
    self.history.truncate(HISTORY_MAX);
  }

  /// `applyTrackRenames`.
  pub fn apply_rename(&self, track: &mut Track) {
    let Some(rename) = self.track_renames.get(&track.canonical_id(&self.platform)) else {
      return;
    };
    if let Some(title) = rename
      .get("title")
      .and_then(Value::as_str)
      .filter(|text| !text.is_empty())
    {
      track.title = Some(title.to_string());
    }
    if let Some(artist) = rename
      .get("artist")
      .and_then(Value::as_str)
      .filter(|text| !text.is_empty())
    {
      track.artist = Some(artist.to_string());
    }
  }

  /// Renomme un titre partout (`btnConfirmRenameTrack`).
  pub fn rename_track(&mut self, track: &Track, title: &str, artist: &str, now_ms: i64) {
    let title = if title.trim().is_empty() {
      "Untitled track"
    } else {
      title.trim()
    };
    let artist = if artist.trim().is_empty() {
      "Unknown artist"
    } else {
      artist.trim()
    };
    let id = track.canonical_id(&self.platform);
    self.track_renames.insert(
      id,
      json!({ "title": title, "artist": artist, "updatedAt": now_ms }),
    );
    let platform = self.platform.clone();
    let lists = [&mut self.history, &mut self.favorites]
      .into_iter()
      .chain(self.playlists.iter_mut().map(|list| &mut list.tracks));
    for list in lists {
      for entry in list
        .iter_mut()
        .filter(|entry| entry.is_same(track, &platform))
      {
        entry.title = Some(title.to_string());
        entry.artist = Some(artist.to_string());
      }
    }
  }

  /// `isArtistBlocked`.
  #[must_use]
  pub fn is_artist_blocked(&self, name: &str) -> bool {
    let target = normalize_artist_name(name);
    !name.trim().is_empty()
      && self
        .blocked_artists
        .iter()
        .any(|blocked| normalize_artist_name(blocked) == target)
  }

  fn blocked_entry(&self, track: &Track) -> Option<usize> {
    let key = (track.artist.is_some() && track.title.is_some()).then(|| track_key(track));
    let title = track.title.as_deref().unwrap_or("").trim().to_lowercase();
    let artist = normalize_artist_name(track.artist.as_deref().unwrap_or(""));
    self.blocked_tracks.iter().position(|blocked| {
      if track.id.is_some() && track.id != Some(String::new()) && blocked.id == track.id {
        return true;
      }
      if key.is_some() && blocked.key == key {
        return true;
      }
      let same_title = !title.is_empty()
        && blocked
          .title
          .as_deref()
          .is_some_and(|text| text.trim().to_lowercase() == title);
      let other = normalize_artist_name(blocked.artist.as_deref().unwrap_or(""));
      same_title && (artist.is_empty() || other.is_empty() || artist == other)
    })
  }

  /// `isTrackBlocked`.
  #[must_use]
  pub fn is_track_blocked(&self, track: &Track) -> bool {
    self.blocked_entry(track).is_some()
  }

  /// Bloqué directement ou par son artiste.
  #[must_use]
  pub fn is_blocked(&self, track: &Track) -> bool {
    self.is_track_blocked(track)
      || track
        .artist
        .as_deref()
        .is_some_and(|artist| self.is_artist_blocked(artist))
  }

  /// `toggleBlockTrack`. Renvoie `true` si le titre est désormais bloqué (et sorti des favoris).
  pub fn toggle_block_track(&mut self, track: &Track) -> bool {
    if let Some(index) = self.blocked_entry(track) {
      self.blocked_tracks.remove(index);
      return false;
    }
    self.blocked_tracks.push(BlockedTrack {
      key: Some(track_key(track)),
      id: track.id.clone(),
      title: track.title.clone(),
      artist: track.artist.clone(),
      ..BlockedTrack::default()
    });
    // Après l'insertion : retire aussi les favoris que l'entrée couvre (autre id, même artiste-titre).
    let blocked = std::mem::take(&mut self.favorites);
    self.favorites = blocked
      .into_iter()
      .filter(|favorite| !self.is_track_blocked(favorite))
      .collect();
    true
  }

  /// Retire le blocage direct du titre (pas celui de son artiste).
  pub(crate) fn unblock_track(&mut self, track: &Track) {
    while let Some(index) = self.blocked_entry(track) {
      self.blocked_tracks.remove(index);
    }
  }

  /// `toggleBlockArtist`. Renvoie `true` si l'artiste est désormais bloqué.
  pub fn toggle_block_artist(&mut self, name: &str) -> bool {
    let clean = name.trim();
    let target = normalize_artist_name(clean);
    if let Some(index) = self
      .blocked_artists
      .iter()
      .position(|blocked| normalize_artist_name(blocked) == target)
    {
      self.blocked_artists.remove(index);
      return false;
    }
    self.blocked_artists.push(clean.to_string());
    true
  }

  /// Identifiants épinglés d'une portée.
  #[must_use]
  pub fn pinned_ids(&self, scope: &str) -> Vec<&str> {
    self
      .pinned_tracks
      .get(scope)
      .and_then(Value::as_array)
      .map_or_else(Vec::new, |ids| {
        ids.iter().filter_map(Value::as_str).collect()
      })
  }

  #[must_use]
  pub fn is_pinned(&self, scope: &str, track: &Track) -> bool {
    let id = track.canonical_id(&self.platform);
    self.pinned_ids(scope).contains(&id.as_str())
  }

  /// `togglePinTrack`. Renvoie `true` si le titre est désormais épinglé.
  pub fn toggle_pin(&mut self, scope: &str, track: &Track) -> bool {
    let id = track.canonical_id(&self.platform);
    let mut ids: Vec<String> = self
      .pinned_ids(scope)
      .into_iter()
      .map(str::to_string)
      .collect();
    let pinned = if let Some(at) = ids.iter().position(|pinned| *pinned == id) {
      ids.remove(at);
      false
    } else {
      ids.push(id);
      true
    };
    if ids.is_empty() {
      self.pinned_tracks.remove(scope);
    } else {
      self.pinned_tracks.insert(scope.to_string(), json!(ids));
    }
    pinned
  }

  /// Ordre d'affichage d'une liste : tri, puis épinglés en tête (`sortTracksFor`).
  #[must_use]
  pub fn display_order(
    &self,
    tracks: &[Track],
    mode: crate::user_data::SortMode,
    scope: &str,
  ) -> Vec<usize> {
    let mut order = track_sort::sorted_indices(tracks, mode);
    track_sort::float_pinned(&mut order, tracks, &self.pinned_ids(scope), &self.platform);
    order
  }

  #[must_use]
  pub fn is_pinned_to_sidebar(&self, track: &Track) -> bool {
    self
      .sidebar_pinned_tracks
      .iter()
      .any(|pinned| pinned.is_same(track, &self.platform))
  }

  /// `togglePinTrackToSidebar`. Renvoie `true` si le titre est désormais épinglé.
  pub fn toggle_sidebar_pin(&mut self, track: &Track) -> bool {
    let platform = self.platform.clone();
    if self.is_pinned_to_sidebar(track) {
      self
        .sidebar_pinned_tracks
        .retain(|pinned| !pinned.is_same(track, &platform));
      return false;
    }
    let mut pinned = track.clone();
    if pinned.platform.as_deref().unwrap_or("").is_empty() {
      pinned.platform = Some(platform);
    }
    pinned.thumbnail = track.thumb_url().or(pinned.thumbnail);
    self.sidebar_pinned_tracks.insert(0, pinned);
    self.sidebar_pinned_tracks.truncate(SIDEBAR_PINS_MAX);
    true
  }

  /// `createPlaylist`. Renvoie l'identifiant, `None` si le nom est vide.
  pub fn create_playlist(
    &mut self,
    name: &str,
    tracks: Vec<Track>,
    now_ms: i64,
    unique: u64,
  ) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
      return None;
    }
    // Le séparateur rend le couple injectif (l'hexadécimal n'a pas de `_`). Une playlist
    // existante (importée comprise) de même identifiant fait avancer le compteur.
    let mut counter = unique;
    let id = loop {
      let candidate = format!("pl_{now_ms:x}_{counter:x}");
      if self.playlist(&candidate).is_none() {
        break candidate;
      }
      counter = counter.wrapping_add(1);
    };
    let tracks = tracks
      .into_iter()
      .map(|track| {
        if track.added_at.is_some() {
          track
        } else {
          Track {
            added_at: Some(now_ms),
            ..track
          }
        }
      })
      .collect();
    self.playlists.push(Playlist {
      id: Some(id.clone()),
      name: Some(name.to_string()),
      tracks,
      ..Playlist::default()
    });
    Some(id)
  }

  #[must_use]
  pub fn playlist(&self, id: &str) -> Option<&Playlist> {
    self
      .playlists
      .iter()
      .find(|playlist| playlist.id.as_deref() == Some(id))
  }

  pub fn playlist_mut(&mut self, id: &str) -> Option<&mut Playlist> {
    self
      .playlists
      .iter_mut()
      .find(|playlist| playlist.id.as_deref() == Some(id))
  }

  /// Nom choisi pour les titres aimés ; vide ou en blanc : `None`, l'appelant prend le libellé traduit.
  #[must_use]
  pub fn liked_custom_name(&self) -> Option<&str> {
    non_blank(self.liked_name.as_deref())
  }

  /// Image choisie pour les titres aimés ; `None` : cœur par défaut.
  #[must_use]
  pub fn liked_custom_cover(&self) -> Option<&str> {
    non_blank(self.liked_cover.as_deref())
  }

  /// Supprime une playlist et ses épingles.
  pub fn delete_playlist(&mut self, id: &str) {
    self
      .playlists
      .retain(|playlist| playlist.id.as_deref() != Some(id));
    let scope = format!("pl:{id}");
    self.pinned_tracks.remove(&scope);
    self.shuffle_lists.retain(|entry| *entry != scope);
  }

  /// L'aléatoire est-il actif pour cette portée ?
  #[must_use]
  pub fn shuffle_on(&self, scope: &str) -> bool {
    self.shuffle_lists.iter().any(|entry| entry == scope)
  }

  /// Inverse l'aléatoire d'une portée et rend le nouvel état.
  pub fn toggle_shuffle_scope(&mut self, scope: &str) -> bool {
    if let Some(at) = self.shuffle_lists.iter().position(|entry| entry == scope) {
      self.shuffle_lists.remove(at);
      false
    } else {
      self.shuffle_lists.push(scope.to_string());
      true
    }
  }

  /// Fixe l'aléatoire d'une portée.
  pub fn set_shuffle_scope(&mut self, scope: &str, on: bool) {
    if self.shuffle_on(scope) != on {
      self.toggle_shuffle_scope(scope);
    }
  }

  /// Comme Riff, le même titre peut figurer plusieurs fois dans une playlist.
  pub fn add_to_playlist(&mut self, id: &str, tracks: &[Track], now_ms: i64) -> bool {
    let Some(playlist) = self.playlist_mut(id) else {
      return false;
    };
    playlist.tracks.extend(tracks.iter().map(|track| Track {
      added_at: Some(now_ms),
      ..track.clone()
    }));
    true
  }

  /// `removeTrackFromCurrentPlaylist`.
  pub fn remove_from_playlist(&mut self, id: &str, track: &Track) {
    if let Some(playlist) = self.playlist_mut(id) {
      playlist.tracks.retain(|entry| entry.id != track.id);
    }
  }

  fn saved_album_index(&self, album: &SavedAlbum) -> Option<usize> {
    self.saved_albums.iter().position(|saved| {
      (album.id.is_some() && saved.id == album.id)
        || (album
          .title
          .as_deref()
          .is_some_and(|title| !title.is_empty())
          && saved.title == album.title
          && saved.artist == album.artist)
    })
  }

  #[must_use]
  pub fn is_album_saved(&self, album: &SavedAlbum) -> bool {
    self.saved_album_index(album).is_some()
  }

  /// Bouton « Save album ». Renvoie `true` si l'album est désormais enregistré.
  pub fn toggle_saved_album(&mut self, album: &SavedAlbum) -> bool {
    if let Some(index) = self.saved_album_index(album) {
      self.saved_albums.remove(index);
      return false;
    }
    self.saved_albums.insert(
      0,
      SavedAlbum {
        extra: Map::new(),
        ..album.clone()
      },
    );
    true
  }

  /// `isArtistFollowed`.
  #[must_use]
  pub fn is_followed(&self, name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    self
      .favorite_artists
      .iter()
      .any(|artist| artist.name.trim().to_lowercase() == lower)
  }

  /// `toggleFollowArtist`. Renvoie `true` si l'artiste est désormais suivi.
  pub fn toggle_follow(&mut self, name: &str, avatar: Option<&str>, fans: Option<f64>) -> bool {
    let clean = name.trim();
    let lower = clean.to_lowercase();
    let same = |other: &str| other.trim().to_lowercase() == lower;
    if self.is_followed(clean) {
      self.favorite_artists.retain(|artist| !same(&artist.name));
      if !self.unfollowed_artists.iter().any(|other| same(other)) {
        self.unfollowed_artists.push(clean.to_string());
      }
      return false;
    }
    self.unfollowed_artists.retain(|other| !same(other));
    self.favorite_artists.push(FavoriteArtist {
      name: clean.to_string(),
      avatar: avatar.map(str::to_string),
      fans,
      ..FavoriteArtist::default()
    });
    true
  }

  /// `getFavoriteArtistsList` : suivis, sans doublon ni artiste quitté.
  #[must_use]
  pub fn followed_artists(&self) -> Vec<&FavoriteArtist> {
    let unfollowed: HashSet<String> = self
      .unfollowed_artists
      .iter()
      .map(|name| name.trim().to_lowercase())
      .collect();
    let mut seen = HashSet::new();
    self
      .favorite_artists
      .iter()
      .filter(|artist| {
        let lower = artist.name.trim().to_lowercase();
        !lower.is_empty() && !unfollowed.contains(&lower) && seen.insert(lower)
      })
      .collect()
  }

  /// Retient l'avatar et les abonnés d'un artiste suivi.
  pub fn update_artist_meta(&mut self, name: &str, avatar: Option<String>, fans: Option<f64>) {
    let lower = name.trim().to_lowercase();
    for artist in self
      .favorite_artists
      .iter_mut()
      .filter(|artist| artist.name.trim().to_lowercase() == lower)
    {
      artist.avatar = avatar.clone().or(artist.avatar.take());
      artist.fans = fans.or(artist.fans);
    }
  }

  fn record_search(&mut self, entry: SearchEntry, now_ms: i64) {
    let key = entry.key.clone();
    self.search_history.retain(|other| other.key != key);
    let mut entry = entry;
    entry.extra.insert("ts".to_string(), json!(now_ms));
    self.search_history.insert(0, entry);
    self.search_history.truncate(SEARCH_HISTORY_MAX);
  }

  /// `recordSearchQuery`.
  pub fn record_search_query(&mut self, query: &str, thumb: Option<String>, now_ms: i64) {
    let query = query.trim();
    if query.is_empty() {
      return;
    }
    self.record_search(
      SearchEntry {
        kind: Some("query".to_string()),
        key: Some(format!("q:{}", normalize_match_text(query))),
        title: Some(query.to_string()),
        query: Some(query.to_string()),
        thumb,
        ..SearchEntry::default()
      },
      now_ms,
    );
  }

  /// `recordSearchTrack`.
  pub fn record_search_track(&mut self, track: &Track, now_ms: i64) {
    let Some(title) = track.title.as_deref().filter(|title| !title.is_empty()) else {
      return;
    };
    let mut extra = Map::new();
    extra.insert("explicit".to_string(), json!(track.is_explicit()));
    self.record_search(
      SearchEntry {
        kind: Some("track".to_string()),
        key: Some(format!("t:{}", track.canonical_id(&self.platform))),
        title: Some(strip_featuring(title)),
        subtitle: track.artist.clone(),
        thumb: track.thumb_url(),
        track: Some(track.clone()),
        extra,
        ..SearchEntry::default()
      },
      now_ms,
    );
  }

  /// `recordSearchArtist`.
  pub fn record_search_artist(
    &mut self,
    name: &str,
    thumb: Option<String>,
    verified: bool,
    now_ms: i64,
  ) {
    let name = name.trim();
    if name.is_empty() {
      return;
    }
    let mut extra = Map::new();
    extra.insert("verified".to_string(), json!(verified));
    self.record_search(
      SearchEntry {
        kind: Some("artist".to_string()),
        key: Some(format!("a:{}", normalize_match_text(name))),
        title: Some(name.to_string()),
        thumb,
        extra,
        ..SearchEntry::default()
      },
      now_ms,
    );
  }

  /// Lignes affichées sous le champ : 12 au plus, filtrées par le texte saisi.
  #[must_use]
  pub fn search_history_matching(&self, typed: &str) -> Vec<&SearchEntry> {
    let typed = normalize_match_text(typed);
    self
      .search_history
      .iter()
      .filter(|entry| {
        typed.is_empty()
          || normalize_match_text(&format!(
            "{} {}",
            entry.title.as_deref().unwrap_or(""),
            entry.subtitle.as_deref().unwrap_or("")
          ))
          .contains(&typed)
      })
      .take(12)
      .collect()
  }

  fn local_key(track: &Track) -> Option<&str> {
    track
      .path
      .as_deref()
      .or_else(|| track.extra.get("filePath").and_then(Value::as_str))
      .or(track.url.as_deref())
      .filter(|key| !key.is_empty())
  }

  /// `mergeLocalFiles`. Renvoie le nombre de pistes ajoutées.
  pub fn merge_local_files(&mut self, new_tracks: Vec<Track>) -> usize {
    let added_keys: HashSet<String> = new_tracks
      .iter()
      .flat_map(|track| {
        [
          track.path.clone(),
          track
            .extra
            .get("filePath")
            .and_then(Value::as_str)
            .map(str::to_string),
          track.url.clone(),
          track.id.clone(),
        ]
      })
      .flatten()
      .collect();
    self
      .removed_local_paths
      .retain(|path| !added_keys.contains(path));
    let mut existing: HashSet<String> = self
      .local_files
      .iter()
      .filter_map(|track| Self::local_key(track).map(str::to_string))
      .collect();
    let mut added = 0;
    for track in new_tracks {
      if let Some(key) = Self::local_key(&track).map(str::to_string)
        && existing.insert(key)
      {
        self.local_files.push(track);
        added += 1;
      }
    }
    added
  }

  /// `removeTracksFromLocalLibrary`. Renvoie les identifiants pour `/api/library-remove`.
  pub fn remove_local_files(&mut self, tracks: &[Track]) -> Vec<String> {
    let ids: HashSet<String> = tracks
      .iter()
      .filter_map(|track| track.id.clone())
      .filter(|id| !id.is_empty())
      .collect();
    let paths: HashSet<String> = tracks
      .iter()
      .filter_map(|track| Self::local_key(track).map(str::to_string))
      .collect();
    self.local_files.retain(|track| {
      !track.id.as_ref().is_some_and(|id| ids.contains(id))
        && !Self::local_key(track).is_some_and(|key| paths.contains(key))
    });
    let mut list: Vec<String> = self.removed_local_paths.clone();
    for value in paths.iter().chain(ids.iter()) {
      if !list.contains(value) {
        list.push(value.clone());
      }
    }
    self.removed_local_paths = list;
    ids.into_iter().collect()
  }

  /// Retire les pistes de la liste noire (`renderLocalFiles`).
  pub fn drop_removed_local_files(&mut self) {
    let removed: HashSet<&str> = self
      .removed_local_paths
      .iter()
      .map(String::as_str)
      .collect();
    self.local_files.retain(|track| {
      !Self::local_key(track).is_some_and(|key| removed.contains(key))
        && !track.id.as_deref().is_some_and(|id| removed.contains(id))
    });
  }

  /// Fichiers à relire : sans pochette ou sans artiste (`renderLocalFiles`).
  #[must_use]
  pub fn local_files_to_rescan(&self) -> Vec<String> {
    self
      .local_files
      .iter()
      .filter(|track| {
        track.thumbnail.as_deref().unwrap_or("").is_empty()
          || track.artist.as_deref() == Some("Unknown Artist")
      })
      .filter_map(|track| track.path.clone())
      .collect()
  }

  /// Applique les métadonnées relues.
  pub fn apply_rescan(&mut self, fresh: Vec<Track>) {
    let by_path: HashMap<String, Track> = fresh
      .into_iter()
      .filter_map(|track| Some((track.path.clone()?, track)))
      .collect();
    for track in &mut self.local_files {
      if let Some(new) = track.path.as_ref().and_then(|path| by_path.get(path)) {
        let (added_at, played_at) = (track.added_at, track.played_at);
        let mut extra = track.extra.clone();
        extra.extend(new.extra.clone());
        *track = Track {
          added_at: added_at.or(new.added_at),
          played_at,
          extra,
          ..new.clone()
        };
      }
    }
  }

  /// `recordWaveSkip`.
  pub fn record_wave_skip(&mut self, track: &Track, now_ms: i64) {
    let artist = track.artist.as_deref().unwrap_or("").trim().to_string();
    let key = track_key(track);
    if key.is_empty() && artist.is_empty() {
      return;
    }
    self
      .wave_skips
      .push(json!({ "key": key, "artist": artist, "t": now_ms }));
    if self.wave_skips.len() > WAVE_SKIPS_MAX {
      let excess = self.wave_skips.len() - WAVE_SKIPS_MAX;
      self.wave_skips.drain(..excess);
    }
  }

  fn feedback_list(&self, key: &str) -> Vec<Value> {
    self
      .mix_feedback
      .get(key)
      .and_then(Value::as_array)
      .cloned()
      .unwrap_or_default()
  }

  /// Note d'un mix (`ratingsByMix`).
  #[must_use]
  pub fn mix_rating(&self, mix_id: &str) -> Option<&str> {
    self.mix_feedback.get("ratingsByMix")?.get(mix_id)?.as_str()
  }

  /// `handleDailyMixRating`.
  pub fn rate_mix(&mut self, mix_id: &str, artist: Option<&str>, tracks: &[Track], like: bool) {
    let rating = if like { "like" } else { "dislike" };
    let ratings = self
      .mix_feedback
      .entry("ratingsByMix".to_string())
      .or_insert_with(|| json!({}));
    if !ratings.is_object() {
      *ratings = json!({});
    }
    ratings[mix_id] = json!(rating);
    let (tracks_key, artists_key) = if like {
      ("likedTracks", "likedArtists")
    } else {
      ("dislikedTracks", "dislikedArtists")
    };
    let mut known_tracks = self.feedback_list(tracks_key);
    let mut known_artists = self.feedback_list(artists_key);
    let mut add_artist = |name: &str| {
      let name = name.trim();
      if !name.is_empty()
        && !known_artists
          .iter()
          .any(|known| known.as_str() == Some(name))
      {
        known_artists.push(json!(name));
      }
    };
    if let Some(artist) = artist.filter(|artist| *artist != "Daily Mix") {
      artist.split(',').for_each(&mut add_artist);
    }
    for track in tracks {
      if let Some(artist) = &track.artist {
        add_artist(artist);
      }
      let id = track.id.as_deref().unwrap_or("");
      let key = if track.artist.is_some() && track.title.is_some() {
        track_key(track)
      } else {
        id.to_string()
      };
      let same = |known: &Value| {
        (!id.is_empty() && value_text(known, "id") == id)
          || (!key.is_empty() && value_text(known, "key") == key)
      };
      if !known_tracks.iter().any(same) {
        known_tracks.push(json!({
          "id": id,
          "title": track.title.as_deref().unwrap_or(""),
          "artist": track.artist.as_deref().unwrap_or(""),
          "key": key,
        }));
      }
    }
    self
      .mix_feedback
      .insert(tracks_key.to_string(), Value::Array(known_tracks));
    self
      .mix_feedback
      .insert(artists_key.to_string(), Value::Array(known_artists));
  }

  /// `getWaveAvoid`.
  #[must_use]
  pub fn wave_avoid(&self) -> Value {
    let mut keys: Vec<String> = Vec::new();
    let mut counts: HashMap<String, (u32, String)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for skip in &self.wave_skips {
      let key = value_text(skip, "key");
      if !key.is_empty() {
        keys.push(key.to_string());
      }
      let artist = value_text(skip, "artist").trim();
      let normalized = normalize_artist_name(artist);
      if !normalized.is_empty() {
        let entry = counts.entry(normalized.clone()).or_insert_with(|| {
          order.push(normalized);
          (0, artist.to_string())
        });
        entry.0 += 1;
      }
    }
    let mut artists: Vec<String> = order
      .iter()
      .filter_map(|name| {
        counts
          .get(name)
          .filter(|(count, _)| *count >= 3)
          .map(|(_, display)| display.clone())
      })
      .collect();
    let mut add_artist = |name: &str| {
      let name = name.trim();
      if !name.is_empty() && !artists.iter().any(|known| known == name) {
        artists.push(name.to_string());
      }
    };
    for disliked in self.feedback_list("dislikedTracks") {
      if let Some(text) = disliked.as_str() {
        keys.push(text.to_string());
        continue;
      }
      for field in ["key", "id"] {
        let value = value_text(&disliked, field);
        if !value.is_empty() {
          keys.push(value.to_string());
        }
      }
      let (artist, title) = (
        value_text(&disliked, "artist"),
        value_text(&disliked, "title"),
      );
      if !artist.is_empty() && !title.is_empty() {
        keys.push(get_track_normalized_key(artist, title));
      }
    }
    for artist in self.feedback_list("dislikedArtists") {
      if let Some(name) = artist.as_str() {
        add_artist(name);
      }
    }
    self
      .blocked_artists
      .iter()
      .for_each(|name| add_artist(name));
    for blocked in &self.blocked_tracks {
      keys.extend(blocked.key.clone());
      keys.extend(blocked.id.clone());
      if let (Some(artist), Some(title)) = (&blocked.artist, &blocked.title) {
        keys.push(get_track_normalized_key(artist, title));
      }
    }
    let mut seen = HashSet::new();
    keys.retain(|key| seen.insert(key.clone()));
    json!({ "keys": keys, "artists": artists })
  }

  /// `computeTasteProfile`.
  #[must_use]
  pub fn taste_profile(&self, max_seeds: usize, now_ms: i64) -> Value {
    #[derive(Default)]
    struct Entry {
      artist: String,
      title: String,
      id: String,
      platform: String,
      weight: f64,
      seen: HashSet<String>,
      favorite: bool,
    }

    let avoid = self.wave_avoid();
    let disliked_artists: HashSet<String> = avoid["artists"]
      .as_array()
      .into_iter()
      .flatten()
      .filter_map(Value::as_str)
      .map(normalize_artist_name)
      .collect();
    let disliked_keys: HashSet<String> = avoid["keys"]
      .as_array()
      .into_iter()
      .flatten()
      .filter_map(Value::as_str)
      .map(|key| key.trim().to_lowercase())
      .collect();
    let identity = |track: &Track, normalized: &str| -> String {
      track
        .id
        .clone()
        .or_else(|| track.url.clone())
        .or_else(|| {
          track
            .extra
            .get("key")
            .and_then(Value::as_str)
            .map(str::to_string)
        })
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| normalized.to_string())
    };

    let mut recency: HashMap<String, f64> = HashMap::new();
    let mut repeat: HashMap<String, f64> = HashMap::new();
    for (index, track) in (0_i32..).zip(&self.history) {
      let id = identity(track, &track_key(track));
      if id.is_empty() || id == "::" {
        continue;
      }
      match recency.entry(id) {
        MapEntry::Occupied(known) => {
          let bonus = repeat.entry(known.key().clone()).or_insert(0.0);
          *bonus = (*bonus + 0.5).min(2.0);
        }
        MapEntry::Vacant(slot) => {
          slot.insert(match track.played_at.filter(|at| *at > 0) {
            Some(at) => 0.5_f64.powf(((now_ms - at).max(0) as f64 / 86_400_000.0) / 21.0),
            None => 0.97_f64.powi(index),
          });
        }
      }
    }

    let mut merged: Vec<Entry> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    let platform = self.platform.clone();
    let mut entry_for = |track: &Track, merged: &mut Vec<Entry>| -> Option<(usize, String)> {
      let artist = track.artist.as_deref()?.trim();
      let title = track.title.as_deref()?.trim();
      if artist.is_empty()
        || title.is_empty()
        || disliked_artists.contains(&normalize_artist_name(artist))
      {
        return None;
      }
      let key = get_track_normalized_key(artist, title);
      if key.is_empty() || key == "::" || disliked_keys.contains(&key.to_lowercase()) {
        return None;
      }
      if track
        .id
        .as_deref()
        .is_some_and(|id| disliked_keys.contains(&id.to_lowercase()))
      {
        return None;
      }
      let at = *positions.entry(key.clone()).or_insert_with(|| {
        merged.push(Entry {
          artist: artist.to_string(),
          title: title.to_string(),
          id: track.id.clone().unwrap_or_default(),
          platform: track.platform.clone().unwrap_or_else(|| platform.clone()),
          ..Entry::default()
        });
        merged.len() - 1
      });
      let entry = &mut merged[at];
      if entry.id.is_empty() && track.id.is_some() {
        entry.id = track.id.clone().unwrap_or_default();
        entry.platform = track.platform.clone().unwrap_or_else(|| platform.clone());
      }
      Some((at, key))
    };

    for track in &self.favorites {
      if let Some((at, _)) = entry_for(track, &mut merged)
        && !merged[at].favorite
      {
        merged[at].weight += 3.0;
        merged[at].favorite = true;
      }
    }
    for track in &self.history {
      if let Some((at, key)) = entry_for(track, &mut merged) {
        let id = identity(track, &key);
        if merged[at].seen.insert(id.clone()) {
          merged[at].weight +=
            recency.get(&id).copied().unwrap_or(0.0) + repeat.get(&id).copied().unwrap_or(0.0);
        }
      }
    }
    for liked in self.feedback_list("likedTracks") {
      let track = Track {
        id: Some(value_text(&liked, "id").to_string()).filter(|id| !id.is_empty()),
        title: Some(value_text(&liked, "title").to_string()),
        artist: Some(value_text(&liked, "artist").to_string()),
        ..Track::default()
      };
      if let Some((at, _)) = entry_for(&track, &mut merged) {
        merged[at].weight += 3.0;
      }
    }

    let mut artist_weights: Vec<(String, String, f64)> = Vec::new();
    let mut add_weight = |name: &str, weight: f64| {
      let normalized = normalize_artist_name(name);
      if normalized.is_empty() {
        return;
      }
      match artist_weights
        .iter_mut()
        .find(|(known, _, _)| *known == normalized)
      {
        Some(entry) => entry.2 += weight,
        None => artist_weights.push((normalized, name.trim().to_string(), weight)),
      }
    };
    for entry in &merged {
      add_weight(&entry.artist, entry.weight);
    }
    for liked in self.feedback_list("likedArtists") {
      if let Some(name) = liked.as_str() {
        add_weight(name, 4.0);
      }
    }
    artist_weights.sort_by(|a, b| b.2.total_cmp(&a.2));
    let top_artists: Vec<&str> = artist_weights
      .iter()
      .take(5)
      .map(|(_, display, _)| display.as_str())
      .collect();

    let track_count = merged.len().max(self.favorites.len());
    merged.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    let seeds: Vec<Value> = merged
      .iter()
      .take(max_seeds)
      .map(|seed| {
        json!({
          "artist": seed.artist,
          "title": seed.title,
          "id": seed.id,
          "platform": seed.platform,
          "weight": (seed.weight * 1000.0).round() / 1000.0,
        })
      })
      .collect();

    let all = self.favorites.iter().chain(&self.history);
    let (mut cyrillic, mut total, mut duration_sum, mut duration_count) =
      (0_usize, 0_usize, 0.0, 0_u32);
    for track in all {
      total += 1;
      if track
        .title
        .as_deref()
        .is_some_and(riff_core::discovery::has_cyrillic)
      {
        cyrillic += 1;
      }
      if let Some(duration) = track.duration.filter(|duration| *duration > 0.0) {
        duration_sum += duration;
        duration_count += 1;
      }
    }
    let script = if total > 0 && cyrillic as f64 > total as f64 * 0.35 {
      "Cyrillic"
    } else {
      "Latin"
    };
    let average = if duration_count > 0 {
      (duration_sum / f64::from(duration_count)).round() as i64
    } else {
      0
    };
    json!({
      "trackCount": track_count,
      "topArtists": top_artists,
      "dominantScript": script,
      "avgDuration": average,
      "bpmLabel": null,
      "energyLabel": null,
      "seeds": seeds,
    })
  }

  /// Corps d'une requête `/api/vibe` pour l'accueil et les mixes.
  #[must_use]
  pub fn vibe_payload(&self, limit: u32, refresh: bool, now_ms: i64) -> Value {
    let profile = self.taste_profile(8, now_ms);
    let mut payload = json!({
      "history": self.history.iter().take(30).collect::<Vec<_>>(),
      "favorites": self.favorites.iter().take(25).collect::<Vec<_>>(),
      "platform": "youtube",
      "limit": limit,
      "seeds": profile["seeds"],
      "clientProfile": profile,
      "avoid": self.wave_avoid(),
    });
    if refresh {
      payload["refresh"] = json!(true);
    }
    payload
  }
}

/// Groupe d'artistes d'un mix quotidien (`planMixes`).
#[derive(Debug, Clone, PartialEq)]
pub struct MixBucket {
  pub id: String,
  pub title: String,
  pub artists: Vec<String>,
  pub seeds: Vec<Value>,
}

/// `planMixes`, sans le tirage au sort : le classement par poids décide.
#[must_use]
pub fn plan_mixes(profile: &Value) -> Vec<MixBucket> {
  struct Artist {
    name: String,
    tracks: Vec<Value>,
    weight: f64,
  }
  let mut artists: Vec<Artist> = Vec::new();
  let mut positions: HashMap<String, usize> = HashMap::new();
  for seed in profile["seeds"].as_array().into_iter().flatten() {
    let name = value_text(seed, "artist");
    let normalized = normalize_artist_name(name);
    if normalized.is_empty() {
      continue;
    }
    let at = *positions.entry(normalized).or_insert_with(|| {
      artists.push(Artist {
        name: name.to_string(),
        tracks: Vec::new(),
        weight: 0.0,
      });
      artists.len() - 1
    });
    if artists[at].tracks.len() < 2 {
      artists[at].weight += seed["weight"].as_f64().unwrap_or(0.0);
      artists[at].tracks.push(seed.clone());
    }
  }
  artists.sort_by(|a, b| b.weight.total_cmp(&a.weight));
  artists.truncate(28);
  if artists.len() < 4 {
    return Vec::new();
  }
  let count = (artists.len() / 2).min(5);
  let mut buckets: Vec<MixBucket> = (0..count)
    .map(|_| MixBucket {
      id: String::new(),
      title: String::new(),
      artists: Vec::new(),
      seeds: Vec::new(),
    })
    .collect();
  for (rank, artist) in artists.into_iter().enumerate() {
    let bucket = &mut buckets[rank % count];
    bucket.artists.push(artist.name);
    bucket.seeds.extend(artist.tracks);
  }
  for bucket in &mut buckets {
    bucket.seeds.truncate(6);
  }
  let first = buckets[0].clone();
  let mut valid: Vec<MixBucket> = buckets
    .into_iter()
    .filter(|bucket| bucket.artists.len() >= 2)
    .collect();
  if valid.is_empty() {
    valid.push(first);
  }
  for (index, bucket) in valid.iter_mut().enumerate() {
    bucket.id = format!("daily-mix-{}", index + 1);
    bucket.title = format!("Daily Mix {}", index + 1);
  }
  valid
}

#[cfg(test)]
mod tests {
  use super::{
    ArtistToken::{Artist, Sep},
    split_artist_tokens,
  };

  fn tokens(raw: &str) -> Vec<super::ArtistToken> {
    split_artist_tokens(raw)
  }

  #[test]
  fn artist_tokens_keep_the_original_separators() {
    assert_eq!(
      tokens("Daft Punk & Julian Casablancas"),
      [
        Artist("Daft Punk".into()),
        Sep(" & ".into()),
        Artist("Julian Casablancas".into())
      ]
    );
    assert_eq!(
      tokens("A, B,C"),
      [
        Artist("A".into()),
        Sep(", ".into()),
        Artist("B".into()),
        Sep(",".into()),
        Artist("C".into())
      ]
    );
    assert_eq!(
      tokens("Song Artist (feat. Guest)"),
      [
        Artist("Song Artist".into()),
        Sep(" feat. ".into()),
        Artist("Guest".into())
      ]
    );
  }

  #[test]
  fn artist_tokens_of_a_single_or_missing_name() {
    assert_eq!(tokens("NAY"), [Artist("NAY".into())]);
    assert_eq!(tokens("  "), [Artist("Unknown artist".into())]);
  }

  #[test]
  fn the_sidebar_width_snaps_like_riff() {
    assert_eq!(super::snap_sidebar_width(0.0), 84);
    assert_eq!(super::snap_sidebar_width(179.9), 84);
    assert_eq!(super::snap_sidebar_width(180.0), 256);
    assert_eq!(super::snap_sidebar_width(300.4), 300);
    assert_eq!(super::snap_sidebar_width(1000.0), 390);
    assert_eq!(super::snap_sidebar_width(f32::NAN), 84);
  }

  use super::*;
  use crate::user_data::SortMode;

  const NOW: i64 = 1_800_000_000_000;

  fn track(id: &str, artist: &str, title: &str) -> Track {
    Track {
      id: Some(id.into()),
      artist: Some(artist.into()),
      title: Some(title.into()),
      ..Track::default()
    }
  }

  fn data() -> UserData {
    UserData::default()
  }

  #[test]
  fn toggling_a_favorite_adds_it_first_with_a_date_then_removes_it() {
    let mut data = data();
    data.favorites.push(track("old", "A", "Old"));
    let new = track("new", "B", "New");
    assert!(data.toggle_favorite(&new, NOW));
    assert_eq!(data.favorites[0].id.as_deref(), Some("new"));
    assert_eq!(data.favorites[0].added_at, Some(NOW));
    assert!(data.is_favorite(&new));
    assert!(!data.toggle_favorite(&new, NOW));
    assert_eq!(data.favorites.len(), 1);
  }

  #[test]
  fn bulk_favorites_skip_those_already_there() {
    let mut data = data();
    data.favorites.push(track("a", "A", "A"));
    assert_eq!(
      data.add_favorites(&[track("a", "A", "A"), track("b", "B", "B")], NOW),
      1
    );
    assert_eq!(data.favorites.len(), 2);
  }

  #[test]
  fn the_history_moves_a_track_first_and_stays_bounded() {
    let mut data = data();
    for index in 0..120 {
      data.record_played(&track(&format!("t{index}"), "A", "T"), NOW);
    }
    assert_eq!(data.history.len(), 100);
    data.record_played(&track("t110", "A", "T"), NOW + 1);
    assert_eq!(data.history.len(), 100);
    assert_eq!(data.history[0].id.as_deref(), Some("t110"));
    assert_eq!(data.history[0].played_at, Some(NOW + 1));
  }

  #[test]
  fn a_rename_applies_everywhere_and_to_future_tracks() {
    let mut data = data();
    let original = track("a", "Old artist", "Old title");
    data.favorites.push(original.clone());
    data.history.push(original.clone());
    data.create_playlist("P", vec![original.clone()], NOW, 1);
    data.rename_track(&original, " New ", "", NOW);
    assert_eq!(data.favorites[0].title.as_deref(), Some("New"));
    assert_eq!(data.favorites[0].artist.as_deref(), Some("Unknown artist"));
    assert_eq!(data.history[0].title.as_deref(), Some("New"));
    assert_eq!(data.playlists[0].tracks[0].title.as_deref(), Some("New"));
    let mut fresh = original;
    data.apply_rename(&mut fresh);
    assert_eq!(fresh.title.as_deref(), Some("New"));
  }

  #[test]
  fn blocking_and_liking_a_track_exclude_each_other() {
    let mut data = data();
    let song = track("a", "Artist", "Song");
    data.favorites.push(song.clone());
    assert!(data.toggle_block_track(&song));
    assert!(!data.is_favorite(&song));
    assert!(data.toggle_favorite(&song, NOW));
    assert!(data.is_favorite(&song));
    assert!(!data.is_track_blocked(&song));
    assert!(data.toggle_block_track(&song));
    assert_eq!(data.add_favorites(std::slice::from_ref(&song), NOW), 1);
    assert!(!data.is_track_blocked(&song));
  }

  #[test]
  fn blocking_a_track_matches_by_id_key_or_title() {
    let mut data = data();
    let song = track("a", "Artist", "Song");
    assert!(data.toggle_block_track(&song));
    assert!(data.is_track_blocked(&song));
    assert!(data.is_track_blocked(&track("other", "Artist", "Song")));
    assert!(!data.is_track_blocked(&track("other", "Someone Else", "Another")));
    assert!(!data.toggle_block_track(&song));
    assert!(!data.is_track_blocked(&song));
  }

  #[test]
  fn blocking_an_artist_ignores_noise_words() {
    let mut data = data();
    assert!(data.toggle_block_artist("Daft Punk"));
    assert!(data.is_artist_blocked("daft punk VEVO"));
    assert!(data.is_blocked(&track("a", "Daft Punk", "T")));
    assert!(!data.is_artist_blocked("  "));
    assert!(!data.toggle_block_artist("DAFT PUNK"));
    assert!(data.blocked_artists.is_empty());
  }

  #[test]
  fn pins_float_in_the_display_order() {
    let mut data = data();
    let list = [
      track("a", "A", "A"),
      track("b", "B", "B"),
      track("c", "C", "C"),
    ];
    assert!(data.toggle_pin(FAVORITES_SCOPE, &list[2]));
    assert_eq!(
      data.display_order(&list, SortMode::Manual, FAVORITES_SCOPE),
      [2, 0, 1]
    );
    assert_eq!(
      data.display_order(&list, SortMode::Manual, "pl:x"),
      [0, 1, 2]
    );
    assert!(data.is_pinned(FAVORITES_SCOPE, &list[2]));
    assert!(!data.toggle_pin(FAVORITES_SCOPE, &list[2]));
    assert!(data.pinned_tracks.is_empty());
  }

  #[test]
  fn sidebar_pins_go_first_and_stay_bounded() {
    let mut data = data();
    for index in 0..60 {
      assert!(data.toggle_sidebar_pin(&track(&format!("t{index}"), "A", "T")));
    }
    assert_eq!(data.sidebar_pinned_tracks.len(), 50);
    assert_eq!(data.sidebar_pinned_tracks[0].id.as_deref(), Some("t59"));
    assert!(!data.toggle_sidebar_pin(&track("t59", "A", "T")));
    assert!(!data.is_pinned_to_sidebar(&track("t59", "A", "T")));
  }

  #[test]
  fn a_playlist_needs_a_name_and_gets_dated_tracks() {
    let mut data = data();
    assert_eq!(data.create_playlist("  ", Vec::new(), NOW, 1), None);
    let id = data
      .create_playlist(" Ma liste ", vec![track("a", "A", "A")], NOW, 1)
      .unwrap();
    assert!(id.starts_with("pl_"));
    let list = data.playlist(&id).unwrap();
    assert_eq!(list.name.as_deref(), Some("Ma liste"));
    assert_eq!(list.tracks[0].added_at, Some(NOW));
  }

  #[test]
  fn tracks_are_added_to_and_removed_from_a_playlist() {
    let mut data = data();
    let id = data.create_playlist("P", Vec::new(), NOW, 1).unwrap();
    let song = track("a", "A", "A");
    assert!(data.add_to_playlist(&id, &[song.clone(), track("b", "B", "B")], NOW));
    assert!(!data.add_to_playlist("absente", std::slice::from_ref(&song), NOW));
    assert_eq!(data.playlist(&id).unwrap().tracks.len(), 2);
    data.remove_from_playlist(&id, &song);
    assert_eq!(
      data.playlist(&id).unwrap().tracks[0].id.as_deref(),
      Some("b")
    );
  }

  #[test]
  fn a_playlist_mixes_youtube_and_soundcloud_tracks_and_keeps_their_platform() {
    let mut data = data();
    let id = data.create_playlist("Mixte", Vec::new(), NOW, 1).unwrap();
    let on = |id: &str, platform: &str, url: &str| Track {
      platform: Some(platform.to_string()),
      url: Some(url.to_string()),
      ..track(id, "A", "T")
    };
    let youtube = on(
      "dQw4w9WgXcQ",
      "youtube",
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    );
    let soundcloud = on("123456789", "soundcloud", "https://soundcloud.com/a/b");
    assert!(data.add_to_playlist(&id, &[youtube.clone(), soundcloud.clone()], NOW));
    let list = data.playlist(&id).unwrap().clone();
    let exported = playlist_export(&list, "2026-11-25T00:00:00Z");
    let raw = serde_json::to_string(&exported).unwrap();
    let back = crate::manage::playlists_from_json(&raw);
    let tracks = &back[0].tracks;
    assert_eq!(tracks[0].platform.as_deref(), Some("youtube"));
    assert_eq!(tracks[0].url, youtube.url);
    assert_eq!(tracks[1].platform.as_deref(), Some("soundcloud"));
    assert_eq!(tracks[1].url, soundcloud.url);
    assert_eq!(tracks[1].id.as_deref(), Some("123456789"));
  }

  #[test]
  fn deleting_a_playlist_removes_its_pins() {
    let mut data = data();
    let id = data.create_playlist("P", Vec::new(), NOW, 1).unwrap();
    data.toggle_pin(&format!("pl:{id}"), &track("a", "A", "A"));
    data.delete_playlist(&id);
    assert!(data.playlists.is_empty());
    assert!(data.pinned_tracks.is_empty());
  }

  #[test]
  fn an_album_is_saved_first_then_removed() {
    let mut data = data();
    let album = SavedAlbum {
      id: Some("x".into()),
      title: Some("T".into()),
      ..SavedAlbum::default()
    };
    assert!(data.toggle_saved_album(&album));
    assert!(data.is_album_saved(&SavedAlbum {
      title: Some("T".into()),
      ..SavedAlbum::default()
    }));
    assert!(!data.toggle_saved_album(&album));
    assert!(data.saved_albums.is_empty());
  }

  #[test]
  fn following_keeps_the_lists_of_riff_in_sync() {
    let mut data = data();
    assert!(data.toggle_follow("Artist", Some("a.png"), Some(3.0)));
    assert!(data.is_followed(" artist "));
    assert_eq!(data.followed_artists()[0].avatar.as_deref(), Some("a.png"));
    assert!(!data.toggle_follow("ARTIST", None, None));
    assert_eq!(data.unfollowed_artists, ["ARTIST"]);
    assert!(data.toggle_follow("artist", None, None));
    assert!(data.unfollowed_artists.is_empty());
  }

  #[test]
  fn unfollowed_and_duplicate_artists_are_not_listed() {
    let mut data = data();
    data.favorite_artists = vec![
      FavoriteArtist {
        name: "A".into(),
        ..FavoriteArtist::default()
      },
      FavoriteArtist {
        name: "a".into(),
        ..FavoriteArtist::default()
      },
      FavoriteArtist {
        name: "B".into(),
        ..FavoriteArtist::default()
      },
    ];
    data.unfollowed_artists = vec!["b".into()];
    let names: Vec<_> = data
      .followed_artists()
      .iter()
      .map(|artist| artist.name.clone())
      .collect();
    assert_eq!(names, ["A"]);
  }

  #[test]
  fn the_search_history_moves_entries_first_and_stays_bounded() {
    let mut data = data();
    for index in 0..30 {
      data.record_search_query(&format!("q{index}"), None, NOW);
    }
    assert_eq!(data.search_history.len(), 25);
    data.record_search_query("Q5", None, NOW);
    assert_eq!(data.search_history.len(), 25);
    data.record_search_query("Q29", None, NOW);
    assert_eq!(data.search_history[0].title.as_deref(), Some("Q29"));
    assert_eq!(
      data
        .search_history
        .iter()
        .filter(|entry| entry.key.as_deref() == Some("q:q29"))
        .count(),
      1
    );
  }

  #[test]
  fn the_search_history_records_tracks_and_artists_and_filters_by_text() {
    let mut data = data();
    data.record_search_track(&track("a", "Daft Punk", "One More Time (feat. X)"), NOW);
    data.record_search_artist("Justice", Some("j.png".into()), true, NOW);
    assert_eq!(
      data.search_history[1].title.as_deref(),
      Some("One More Time")
    );
    assert_eq!(
      data.search_history[0].extra.get("verified"),
      Some(&json!(true))
    );
    assert_eq!(data.search_history_matching("").len(), 2);
    assert_eq!(data.search_history_matching("daft").len(), 1);
    assert!(data.search_history_matching("rien").is_empty());
  }

  #[test]
  fn a_history_row_takes_its_own_thumb_then_the_one_of_its_track() {
    let mut data = data();
    let mut tagged = track("a", "Daft Punk", "One More Time");
    tagged.thumbnail = Some("t.png".into());
    data.record_search_track(&tagged, NOW);
    data.record_search_query("daft", Some(String::new()), NOW);
    data.record_search_artist("Justice", Some("j.png".into()), true, NOW);
    assert_eq!(data.search_history[0].thumb_url().as_deref(), Some("j.png"));
    assert_eq!(data.search_history[1].thumb_url(), None, "vignette vide");
    assert_eq!(data.search_history[2].thumb_url().as_deref(), Some("t.png"));
    assert!(data.search_history[0].is_verified());
    assert!(!data.search_history[2].is_verified());
    assert!(!data.search_history[2].is_explicit());
  }

  #[test]
  fn local_files_are_merged_without_duplicates_and_leave_the_blacklist() {
    let mut data = data();
    data.removed_local_paths = vec!["D:/a.mp3".into(), "D:/z.mp3".into()];
    let file = |path: &str| Track {
      path: Some(path.into()),
      platform: Some("local".into()),
      ..Track::default()
    };
    assert_eq!(
      data.merge_local_files(vec![file("D:/a.mp3"), file("D:/b.mp3"), file("D:/a.mp3")]),
      2
    );
    assert_eq!(data.local_files.len(), 2);
    assert_eq!(data.removed_local_paths, ["D:/z.mp3"]);
    assert_eq!(data.merge_local_files(vec![file("D:/b.mp3")]), 0);
  }

  #[test]
  fn removing_a_local_file_blacklists_it_so_it_does_not_come_back() {
    let mut data = data();
    let file = Track {
      id: Some("l1".into()),
      path: Some("D:/a.mp3".into()),
      ..Track::default()
    };
    data.local_files = vec![
      file.clone(),
      Track {
        path: Some("D:/b.mp3".into()),
        ..Track::default()
      },
    ];
    assert_eq!(data.remove_local_files(std::slice::from_ref(&file)), ["l1"]);
    assert_eq!(data.local_files.len(), 1);
    assert!(data.removed_local_paths.contains(&"D:/a.mp3".to_string()));
    data.local_files.push(file);
    data.drop_removed_local_files();
    assert_eq!(data.local_files.len(), 1);
  }

  #[test]
  fn files_without_metadata_are_rescanned_and_updated() {
    let mut data = data();
    let mut known = Track {
      path: Some("a".into()),
      thumbnail: Some("t".into()),
      artist: Some("X".into()),
      ..Track::default()
    };
    known.added_at = Some(5);
    data.local_files = vec![
      Track {
        path: Some("b".into()),
        thumbnail: None,
        added_at: Some(9),
        ..Track::default()
      },
      known,
      Track {
        path: Some("c".into()),
        thumbnail: Some("t".into()),
        artist: Some("Unknown Artist".into()),
        ..Track::default()
      },
    ];
    assert_eq!(data.local_files_to_rescan(), ["b", "c"]);
    data.apply_rescan(vec![Track {
      path: Some("b".into()),
      title: Some("Lu".into()),
      thumbnail: Some("n".into()),
      ..Track::default()
    }]);
    assert_eq!(data.local_files[0].title.as_deref(), Some("Lu"));
    assert_eq!(data.local_files[0].added_at, Some(9));
  }

  #[test]
  fn searching_ranks_exact_matches_first() {
    let list = vec![
      track("a", "X", "Something else"),
      track("b", "Daft Punk", "One More Time (feat. Y)"),
      track("c", "Z", "daft punk one more time"),
    ];
    let ids: Vec<_> = rank_search_tracks("Daft Punk One More Time", list)
      .into_iter()
      .map(|t| t.id.unwrap())
      .collect();
    assert_eq!(ids, ["b", "c", "a"]);
  }

  #[test]
  fn artists_are_split_like_riff() {
    assert_eq!(split_artists("A, B & C"), ["A", "B", "C"]);
    assert_eq!(split_artists("A feat. B"), ["A", "B"]);
    assert_eq!(split_artists("A (feat. B)"), ["A", "B"]);
    assert_eq!(split_artists("Simon & Garfunkel"), ["Simon & Garfunkel"]);
    assert_eq!(split_artists("Name - Topic"), ["Name"]);
    assert_eq!(split_artists(""), ["Unknown artist"]);
    assert_eq!(autoplay_artists("A, B feat. C"), ["A", "B", "C"]);
  }

  #[test]
  fn the_artist_page_name_is_cleaned() {
    assert_eq!(clean_artist_name("Muse - Topic"), "Muse");
    assert_eq!(clean_artist_name("MuseVEVO"), "Muse");
  }

  #[test]
  fn durations_are_formatted_like_riff() {
    assert_eq!(format_duration(0.0), "0:00");
    assert_eq!(format_duration(f64::NAN), "0:00");
    assert_eq!(format_duration(65.9), "1:05");
    assert_eq!(format_duration(3700.0), "61:40");
  }

  #[test]
  fn total_durations_are_readable() {
    for (seconds, expected) in [
      (101_228.0, "28 hr 7 min"),
      (100_994.0, "28 hr 3 min"),
      (3600.0, "1 hr"),
      (3661.0, "1 hr 1 min"),
      (60.0, "1 min"),
      (59.0, "59 sec"),
      (245.0, "4 min 5 sec"),
      (0.0, ""),
      (f64::NAN, ""),
    ] {
      assert_eq!(format_total_duration(seconds), expected, "{seconds}");
    }
  }

  #[test]
  fn the_share_url_is_the_direct_address_of_the_track() {
    let mut song = track("dQw4w9WgXcQ", "A B", "T");
    assert_eq!(
      share_url(&song).as_deref(),
      Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );
    song.url = Some("https://soundcloud.com/a/b".into());
    assert_eq!(
      share_url(&song).as_deref(),
      Some("https://soundcloud.com/a/b")
    );
    let stream = Track {
      url: Some("http://127.0.0.1:1/x".into()),
      ..track("dQw4w9WgXcQ", "A", "T")
    };
    assert_eq!(
      share_url(&stream).as_deref(),
      Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );
  }

  #[test]
  fn the_share_url_is_absent_without_a_public_address() {
    let local = Track {
      platform: Some("local".into()),
      url: Some("http://127.0.0.1:1/x".into()),
      ..track("abc", "A", "T")
    };
    assert_eq!(share_url(&local), None);
    let soundcloud = Track {
      platform: Some("soundcloud".into()),
      ..track("123", "A", "T")
    };
    assert_eq!(share_url(&soundcloud), None);
  }

  #[test]
  fn the_export_hides_local_paths() {
    let playlist = Playlist {
      name: Some("P".into()),
      tracks: vec![Track {
        id: Some("D:\\m\\a.mp3".into()),
        url: Some("file:///D:/m/a.mp3".into()),
        platform: Some("local".into()),
        cover: Some("D:\\m\\c.png".into()),
        ..Track::default()
      }],
      ..Playlist::default()
    };
    let export = playlist_export(&playlist, "2026-11-03T00:00:00Z");
    assert_eq!(export["app"], "Riff");
    let first = &export["playlists"][0]["tracks"][0];
    assert_eq!(first["id"], "a.mp3");
    assert_eq!(first["url"], "a.mp3");
    assert_eq!(first["cover"], "c.png");
    assert_eq!(safe_file_name("a/b:c"), "a_b_c");
  }

  #[test]
  fn a_track_list_exports_like_a_playlist() {
    let tracks = [track("1", "A", "T")];
    let export = tracks_export("Liked Songs", "", &tracks, "2026-11-03T00:00:00Z");
    let list = &export["playlists"][0];
    assert_eq!(list["name"], "Liked Songs");
    assert_eq!(list["tracks"][0]["title"], "T");
  }

  #[test]
  fn local_days_follow_the_calendar() {
    assert_eq!(local_day(0, 0), "1970-01-01");
    assert_eq!(local_day(1_793_664_000_000, 0), "2026-11-03");
    assert_eq!(local_day(1_793_664_000_000, -60), "2026-11-02");
    assert_eq!(local_day(951_782_400_000, 0), "2000-02-29");
  }

  #[test]
  fn avoided_tracks_gather_skips_dislikes_and_blocks() {
    let mut data = data();
    for _ in 0..3 {
      data.record_wave_skip(&track("a", "Skipped", "Song"), NOW);
    }
    data.record_wave_skip(&track("b", "Once", "Other"), NOW);
    data.toggle_block_artist("Blocked");
    data.toggle_block_track(&track("bt", "Who", "What"));
    let avoid = data.wave_avoid();
    let artists = avoid["artists"].as_array().unwrap();
    assert!(artists.contains(&json!("Skipped")));
    assert!(artists.contains(&json!("Blocked")));
    assert!(!artists.contains(&json!("Once")));
    let keys = avoid["keys"].as_array().unwrap();
    assert!(keys.contains(&json!("bt")));
    assert_eq!(
      keys
        .iter()
        .filter(|key| **key == json!(get_track_normalized_key("Skipped", "Song")))
        .count(),
      1
    );
  }

  #[test]
  fn the_taste_profile_weighs_favorites_and_recent_plays() {
    let mut data = data();
    data.favorites.push(track("fav", "Fav", "Song"));
    data.history.push(Track {
      played_at: Some(NOW),
      ..track("h1", "Heard", "Hit")
    });
    data.history.push(Track {
      played_at: Some(NOW - 21 * 86_400_000),
      ..track("h2", "Old", "Tune")
    });
    let profile = data.taste_profile(8, NOW);
    let seeds = profile["seeds"].as_array().unwrap();
    assert_eq!(seeds[0]["artist"], "Fav");
    assert_eq!(seeds[0]["weight"], 3.0);
    assert_eq!(seeds[1]["weight"], 1.0);
    assert_eq!(seeds[2]["weight"], 0.5);
    assert_eq!(profile["trackCount"], 3);
    assert_eq!(profile["topArtists"][0], "Fav");
    assert_eq!(profile["dominantScript"], "Latin");
  }

  #[test]
  fn the_taste_profile_skips_avoided_artists_and_counts_repeats() {
    let mut data = data();
    data.toggle_block_artist("Blocked");
    data.history.push(track("b", "Blocked", "No"));
    data.history.push(Track {
      played_at: Some(NOW),
      ..track("r", "Rep", "Eat")
    });
    data.history.push(Track {
      played_at: Some(NOW),
      ..track("r", "Rep", "Eat")
    });
    let profile = data.taste_profile(8, NOW);
    let seeds = profile["seeds"].as_array().unwrap();
    assert_eq!(seeds.len(), 1);
    assert_eq!(seeds[0]["weight"], 1.5);
  }

  #[test]
  fn the_vibe_payload_has_the_shape_of_riff() {
    let mut data = data();
    data.favorites.push(track("fav", "Fav", "Song"));
    let payload = data.vibe_payload(10, true, NOW);
    assert_eq!(payload["limit"], 10);
    assert_eq!(payload["platform"], "youtube");
    assert_eq!(payload["refresh"], true);
    assert_eq!(payload["seeds"][0]["artist"], "Fav");
    assert_eq!(payload["favorites"][0]["id"], "fav");
    assert!(payload["avoid"]["keys"].is_array());
    assert!(data.vibe_payload(10, false, NOW).get("refresh").is_none());
  }

  #[test]
  fn rating_a_mix_records_its_tracks_and_artists() {
    let mut data = data();
    let tracks = [track("a", "Art", "T")];
    data.rate_mix("daily-mix-1", Some("X, Y"), &tracks, false);
    assert_eq!(data.mix_rating("daily-mix-1"), Some("dislike"));
    assert_eq!(
      data.mix_feedback["dislikedArtists"],
      json!(["X", "Y", "Art"])
    );
    assert_eq!(data.mix_feedback["dislikedTracks"][0]["id"], "a");
    data.rate_mix("daily-mix-1", Some("X"), &tracks, false);
    assert_eq!(
      data.mix_feedback["dislikedTracks"]
        .as_array()
        .unwrap()
        .len(),
      1
    );
  }

  #[test]
  fn mixes_group_artists_by_weight() {
    let seed =
      |artist: &str, weight: f64| json!({ "artist": artist, "title": "T", "weight": weight });
    let profile = json!({ "seeds": [seed("A", 8.0), seed("B", 7.0), seed("C", 6.0), seed("D", 5.0), seed("E", 4.0), seed("A", 1.0), seed("A", 1.0)] });
    let buckets = plan_mixes(&profile);
    assert_eq!(buckets.len(), 2);
    assert_eq!(buckets[0].id, "daily-mix-1");
    assert_eq!(buckets[0].title, "Daily Mix 1");
    assert_eq!(buckets[0].artists, ["A", "C", "E"]);
    assert_eq!(buckets[1].artists, ["B", "D"]);
    assert_eq!(buckets[0].seeds.len(), 4);
  }

  #[test]
  fn too_few_artists_give_no_mix() {
    let profile =
      json!({ "seeds": [{ "artist": "A", "weight": 1 }, { "artist": "B", "weight": 1 }] });
    assert!(plan_mixes(&profile).is_empty());
  }
}
