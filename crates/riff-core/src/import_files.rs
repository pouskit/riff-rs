//! Analyse des exports Spotify (JSON) et Apple Music (CSV, JSON), d'après `app.js` de Riff
//! (`sp-file-input`, `parseAppleCSV`, `parseAppleExportCSV`, `parseAppleExportJSON`).
//!
//! Aucun rapprochement avec une recherche : les titres sont cherchés à la lecture.

use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Taille maximale d'un fichier d'export.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// Profondeur maximale du parcours d'un JSON Apple.
const MAX_DEPTH: usize = 256;

/// Fichier d'export refusé.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
  #[error("fichier vide")]
  Empty,
  #[error("fichier de plus de 20 Mo")]
  TooLarge,
  #[error("JSON invalide : {0}")]
  InvalidJson(String),
  #[error("fichier illisible : {0}")]
  Unreadable(String),
}

/// Titre d'un export, sans identifiant.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedTrack {
  pub title: String,
  pub artist: String,
  /// En secondes.
  pub duration: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportedPlaylist {
  pub name: String,
  pub tracks: Vec<ImportedTrack>,
}

/// Contenu d'un ou de plusieurs fichiers : le `{ likes, playlists }` de `apply_library`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Library {
  pub likes: Vec<ImportedTrack>,
  pub playlists: Vec<ImportedPlaylist>,
}

impl Library {
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.likes.is_empty() && self.playlists.is_empty()
  }

  /// Nombre de titres des parties demandées.
  #[must_use]
  pub fn track_count(&self, likes: bool, playlists: bool) -> usize {
    let liked = if likes { self.likes.len() } else { 0 };
    let listed = if playlists {
      self
        .playlists
        .iter()
        .map(|playlist| playlist.tracks.len())
        .sum()
    } else {
      0
    };
    liked + listed
  }

  pub fn merge(&mut self, other: Self) {
    self.likes.extend(other.likes);
    self.playlists.extend(other.playlists);
  }

  /// `{ likes, playlists }` des parties demandées, tel que le lit `apply_library`.
  #[must_use]
  pub fn to_value(&self, likes: bool, playlists: bool) -> Value {
    let track = |track: &ImportedTrack| {
      let mut value = json!({ "title": track.title, "artist": track.artist });
      if let Some(duration) = track.duration {
        value["duration"] = json!(duration);
      }
      value
    };
    let likes: Vec<Value> = if likes {
      self.likes.iter().map(track).collect()
    } else {
      Vec::new()
    };
    let playlists: Vec<Value> = if playlists {
      self
        .playlists
        .iter()
        .map(|playlist| {
          let tracks: Vec<Value> = playlist.tracks.iter().map(track).collect();
          json!({ "name": playlist.name, "tracks": tracks })
        })
        .collect()
    } else {
      Vec::new()
    };
    json!({ "likes": likes, "playlists": playlists })
  }
}

/// Texte sans BOM (que `Blob.text()` retire), refusé s'il est trop gros ou vide.
fn checked(text: &str) -> Result<&str, ImportError> {
  if text.len() as u64 > MAX_FILE_BYTES {
    return Err(ImportError::TooLarge);
  }
  let text = text.strip_prefix('\u{feff}').unwrap_or(text);
  if text.trim().is_empty() {
    return Err(ImportError::Empty);
  }
  Ok(text)
}

fn parse_json(text: &str) -> Result<Value, ImportError> {
  serde_json::from_str(checked(text)?).map_err(|error| ImportError::InvalidJson(error.to_string()))
}

/// Texte rogné d'une valeur « vraie » (chaîne non vide, nombre non nul) ; le reste est ignoré.
fn text_of(value: Option<&Value>) -> Option<String> {
  let text = match value? {
    Value::String(text) => text.trim().to_string(),
    Value::Number(number) if number.as_f64().is_some_and(|n| n != 0.0) => number.to_string(),
    _ => return None,
  };
  (!text.is_empty()).then_some(text)
}

/// Première des clés qui donne un texte (le `a || b || c` de Riff).
fn first_text(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
  keys.iter().find_map(|key| text_of(object.get(*key)))
}

/// `Number(text)` de JS pour les formes décimales ; `None` si le résultat n'est pas un nombre fini.
fn js_number(text: &str) -> Option<f64> {
  let text = text.trim();
  if text.is_empty() {
    return Some(0.0);
  }
  text.parse::<f64>().ok().filter(|number| number.is_finite())
}

/// `Math.round(ms / 1000)` ; zéro, négatif ou illisible : pas de durée.
fn seconds_of_millis(value: &Value) -> Option<f64> {
  let millis = match value {
    Value::Number(number) => number.as_f64()?,
    Value::String(text) => js_number(text)?,
    _ => return None,
  };
  (millis.is_finite() && millis > 0.0).then(|| (millis / 1000.0).round())
}

fn first_seconds(object: &Map<String, Value>, keys: &[&str]) -> Option<f64> {
  keys
    .iter()
    .find_map(|key| object.get(*key).and_then(seconds_of_millis))
}

fn track_of(
  title: Option<String>,
  artist: Option<String>,
  duration: Option<f64>,
) -> Option<ImportedTrack> {
  Some(ImportedTrack {
    title: title?,
    artist: artist?,
    duration,
  })
}

/// Playlists regroupées par nom, dans l'ordre de première apparition (le `Map` de Riff).
#[derive(Default)]
struct Groups {
  index: HashMap<String, usize>,
  playlists: Vec<ImportedPlaylist>,
}

impl Groups {
  fn push(&mut self, name: &str, track: ImportedTrack) {
    let position = match self.index.get(name) {
      Some(position) => *position,
      None => {
        self.playlists.push(ImportedPlaylist {
          name: name.to_string(),
          tracks: Vec::new(),
        });
        self
          .index
          .insert(name.to_string(), self.playlists.len() - 1);
        self.playlists.len() - 1
      }
    };
    self.playlists[position].tracks.push(track);
  }
}

// Spotify

/// Export Spotify : `YourLibrary.json` (`tracks`) et `Playlist1.json` (`playlists[].items[].track`),
/// objet ou tableau à la racine.
///
/// # Errors
/// Fichier vide, de plus de 20 Mo ou qui n'est pas du JSON.
pub fn spotify_export(text: &str) -> Result<Library, ImportError> {
  let value = parse_json(text)?;
  let mut library = Library::default();
  let tracks = value
    .get("tracks")
    .and_then(Value::as_array)
    .or_else(|| value.as_array());
  for item in tracks.into_iter().flatten() {
    let Some(item) = item.as_object() else {
      continue;
    };
    let title = first_text(item, &["track", "trackName"]);
    let artist = first_text(item, &["artist", "artistName"]);
    let duration = first_seconds(item, &["durationMs", "duration_ms", "msPlayed"]);
    library.likes.extend(track_of(title, artist, duration));
  }
  let playlists = value
    .get("playlists")
    .and_then(Value::as_array)
    .or_else(|| {
      let root = value.as_array()?;
      root
        .first()?
        .get("items")
        .is_some_and(Value::is_array)
        .then_some(root)
    });
  for playlist in playlists.into_iter().flatten() {
    let Some(playlist) = playlist.as_object() else {
      continue;
    };
    let name = text_of(playlist.get("name")).unwrap_or_else(|| "Imported Playlist".to_string());
    let items = playlist.get("items").and_then(Value::as_array);
    let mut tracks = Vec::new();
    for item in items.into_iter().flatten() {
      let Some(track) = item.get("track").and_then(Value::as_object) else {
        continue;
      };
      let title = first_text(track, &["trackName", "track"]);
      let artist = first_text(track, &["artistName", "artist"]);
      let duration = first_seconds(track, &["durationMs", "duration_ms"]);
      tracks.extend(track_of(title, artist, duration));
    }
    if !tracks.is_empty() {
      library.playlists.push(ImportedPlaylist { name, tracks });
    }
  }
  Ok(library)
}

// Apple Music

/// `parseAppleDuration` : `m:ss`, `h:mm:ss`, millisecondes (au-dessus de 1000) ou secondes.
pub(crate) fn apple_duration(value: &str) -> Option<f64> {
  let text = value.trim();
  if text.is_empty() {
    return None;
  }
  if text.contains(':') {
    let parts: Vec<f64> = text.split(':').map(js_number).collect::<Option<_>>()?;
    let seconds = match parts.as_slice() {
      [minutes, seconds] => Some(minutes * 60.0 + seconds),
      [hours, minutes, seconds] => Some(hours * 3600.0 + minutes * 60.0 + seconds),
      _ => None,
    };
    if let Some(seconds) = seconds {
      return (seconds >= 0.0).then_some(seconds);
    }
  }
  let number = js_number(text)?;
  let seconds = if number > 1000.0 {
    (number / 1000.0).round()
  } else {
    number.round()
  };
  (seconds >= 0.0).then_some(seconds)
}

/// `parseAppleCSV` : séparateur tabulation, `;` ou `,` d'après la première ligne, guillemets doublés.
/// Comme Riff, une ligne est un enregistrement : un retour à la ligne entre guillemets le coupe.
fn parse_csv(text: &str) -> (Vec<String>, Vec<HashMap<String, String>>) {
  let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
  let Some(first) = lines.next() else {
    return (Vec::new(), Vec::new());
  };
  let delimiter = if first.contains('\t') {
    '\t'
  } else if first.contains(';') && !first.contains(',') {
    ';'
  } else {
    ','
  };
  let headers: Vec<String> = split_row(first, delimiter)
    .iter()
    .map(|header| header.to_lowercase().replace(['\'', '"'], ""))
    .collect();
  let rows = lines
    .map(|line| {
      let columns = split_row(line, delimiter);
      headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
          (
            header.clone(),
            columns.get(index).cloned().unwrap_or_default(),
          )
        })
        .collect()
    })
    .collect();
  (headers, rows)
}

fn split_row(row: &str, delimiter: char) -> Vec<String> {
  let mut fields = Vec::new();
  let mut current = String::new();
  let mut quoted = false;
  let mut chars = row.chars().peekable();
  while let Some(c) = chars.next() {
    if c == '"' {
      if quoted && chars.peek() == Some(&'"') {
        current.push('"');
        chars.next();
      } else {
        quoted = !quoted;
      }
    } else if c == delimiter && !quoted {
      fields.push(current.trim().to_string());
      current.clear();
    } else {
      current.push(c);
    }
  }
  fields.push(current.trim().to_string());
  fields
}

struct Columns {
  title: Regex,
  title_loose: Regex,
  artist: Regex,
  artist_loose: Regex,
  duration: Regex,
  playlist: Regex,
}

fn columns() -> &'static Columns {
  static COLUMNS: OnceLock<Columns> = OnceLock::new();
  COLUMNS.get_or_init(|| {
    let regex = |pattern: &str| Regex::new(pattern).expect("motif constant");
    Columns {
      title: regex(r"(?i)^(title|name|track\s*name|song(\s*name)?|track\s*description)$"),
      title_loose: regex(r"(?i)(title|song|track)"),
      artist: regex(r"(?i)^(artist(\s*name)?|artist\(s\)|creator|uploader)$"),
      artist_loose: regex(r"(?i)artist"),
      duration: regex(r"(?i)(duration|time|length)"),
      playlist: regex(r"(?i)(playlist(\s*name)?|container(\s*description|\s*name)?)"),
    }
  })
}

fn find_column<'a>(headers: &'a [String], pattern: &Regex) -> Option<&'a String> {
  headers.iter().find(|header| pattern.is_match(header))
}

/// `parseAppleExportCSV` : une ligne sans playlist est un favori.
///
/// # Errors
/// Fichier vide ou de plus de 20 Mo.
pub fn apple_export_csv(text: &str) -> Result<Library, ImportError> {
  let (headers, rows) = parse_csv(checked(text)?);
  let c = columns();
  let title_column =
    find_column(&headers, &c.title).or_else(|| find_column(&headers, &c.title_loose));
  let artist_column =
    find_column(&headers, &c.artist).or_else(|| find_column(&headers, &c.artist_loose));
  let duration_column = find_column(&headers, &c.duration);
  let playlist_column = find_column(&headers, &c.playlist);
  let cell = |row: &HashMap<String, String>, column: Option<&String>| {
    column
      .and_then(|column| row.get(column))
      .cloned()
      .unwrap_or_default()
  };
  let mut likes = Vec::new();
  let mut groups = Groups::default();
  for row in &rows {
    let title = cell(row, title_column);
    let artist = cell(row, artist_column);
    if title.is_empty() || artist.is_empty() {
      continue;
    }
    let track = ImportedTrack {
      title,
      artist,
      duration: apple_duration(&cell(row, duration_column)),
    };
    let playlist = cell(row, playlist_column);
    if playlist.is_empty() {
      likes.push(track);
    } else {
      groups.push(&playlist, track);
    }
  }
  Ok(Library {
    likes,
    playlists: groups.playlists,
  })
}

/// Valeur « vraie » de JS.
fn truthy(value: &Value) -> bool {
  match value {
    Value::Null => false,
    Value::Bool(flag) => *flag,
    Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
    Value::String(text) => !text.is_empty(),
    Value::Array(_) | Value::Object(_) => true,
  }
}

/// `parseAppleExportJSON` : parcours récursif, le nom de la playlist vient du parent
/// (`name` ou `title` avec `tracks` ou `items`) ou d'un champ `playlistName`.
#[must_use]
pub fn apple_export_json(value: &Value) -> Library {
  let mut likes = Vec::new();
  let mut groups = Groups::default();
  walk(value, "", 0, &mut likes, &mut groups);
  Library {
    likes,
    playlists: groups.playlists,
  }
}

fn walk(
  node: &Value,
  current: &str,
  depth: usize,
  likes: &mut Vec<ImportedTrack>,
  groups: &mut Groups,
) {
  if depth > MAX_DEPTH {
    return;
  }
  let object = match node {
    Value::Array(items) => {
      for item in items {
        walk(item, current, depth + 1, likes, groups);
      }
      return;
    }
    Value::Object(object) => object,
    _ => return,
  };
  let playlist = first_text(object, &["playlistName", "playlist", "playlist name"])
    .unwrap_or_else(|| current.to_string());
  if let Some(name) = first_text(object, &["name", "title"]) {
    for key in ["tracks", "items"] {
      if let Some(Value::Array(children)) = object.get(key) {
        for child in children {
          walk(child, &name, depth + 1, likes, groups);
        }
        return;
      }
    }
  }
  let title = first_text(
    object,
    &[
      "title",
      "name",
      "trackName",
      "song",
      "track name",
      "song name",
    ],
  );
  let artist = first_text(
    object,
    &["artistName", "artist", "artist_name", "artist name"],
  );
  let raw_duration = [
    "durationInMillis",
    "duration",
    "duration_ms",
    "time",
    "trackDuration",
  ]
  .iter()
  .find_map(|key| match object.get(*key)? {
    Value::Number(number) if number.as_f64().is_some_and(|n| n != 0.0) => Some(number.to_string()),
    Value::String(text) if !text.is_empty() => Some(text.clone()),
    _ => None,
  });
  let duration = raw_duration.as_deref().and_then(apple_duration);
  let is_song = matches!(
    object.get("type").and_then(Value::as_str),
    Some("song" | "songs")
  );
  let described = object.get("contentDescriptor").is_some_and(truthy);
  if let Some(track) = track_of(title, artist, duration)
    && (is_song || duration.is_some_and(|d| d != 0.0) || !playlist.is_empty() || described)
  {
    if playlist.is_empty() {
      likes.push(track);
    } else {
      groups.push(&playlist, track);
    }
    return;
  }
  for child in object.values() {
    walk(child, &playlist, depth + 1, likes, groups);
  }
}

/// Fichier Apple : JSON si l'extension est `.json`, CSV sinon (`apple-file-input`).
///
/// # Errors
/// Fichier vide, de plus de 20 Mo, ou JSON invalide pour une extension `.json`.
pub fn apple_export(name: &str, text: &str) -> Result<Library, ImportError> {
  if name.to_lowercase().ends_with(".json") {
    Ok(apple_export_json(&parse_json(text)?))
  } else {
    apple_export_csv(text)
  }
}

// Fichiers

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportSource {
  Spotify,
  Apple,
}

/// Résultat de la lecture d'une sélection de fichiers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FilesRead {
  /// Noms de tous les fichiers choisis, refusés compris.
  pub names: Vec<String>,
  pub library: Library,
  pub errors: Vec<(String, ImportError)>,
}

fn read_text(path: &Path) -> Result<String, ImportError> {
  let unreadable = |error: std::io::Error| ImportError::Unreadable(error.to_string());
  let file = File::open(path).map_err(unreadable)?;
  if file.metadata().map_err(unreadable)?.len() > MAX_FILE_BYTES {
    return Err(ImportError::TooLarge);
  }
  let mut bytes = Vec::new();
  file
    .take(MAX_FILE_BYTES + 1)
    .read_to_end(&mut bytes)
    .map_err(unreadable)?;
  if bytes.len() as u64 > MAX_FILE_BYTES {
    return Err(ImportError::TooLarge);
  }
  Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Lit et analyse les fichiers dans l'ordre. Un fichier refusé n'arrête pas les suivants.
#[must_use]
pub fn read_files(source: ExportSource, paths: &[PathBuf]) -> FilesRead {
  let mut read = FilesRead::default();
  for path in paths {
    let name = path.file_name().map_or_else(
      || path.display().to_string(),
      |name| name.to_string_lossy().into_owned(),
    );
    let parsed = read_text(path).and_then(|text| match source {
      ExportSource::Spotify => spotify_export(&text),
      ExportSource::Apple => apple_export(&name, &text),
    });
    match parsed {
      Ok(library) => read.library.merge(library),
      Err(error) => read.errors.push((name.clone(), error)),
    }
    read.names.push(name);
  }
  read
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;
  use serde_json::json;

  fn song(title: &str, artist: &str, duration: Option<f64>) -> ImportedTrack {
    ImportedTrack {
      title: title.to_string(),
      artist: artist.to_string(),
      duration,
    }
  }

  fn temp_dir() -> TempDir {
    TempDir::new("core-import-files")
  }

  // Spotify

  #[test]
  fn a_spotify_tracks_array_gives_the_likes_with_every_field_variant() {
    let raw = r#"{ "tracks": [
      { "track": "Aube", "artist": "Les Lunes", "durationMs": 215000 },
      { "trackName": "Brume", "artistName": "Nord", "duration_ms": 180499 },
      { "track": "Ciel", "artist": "Sud", "msPlayed": 61500 },
      { "track": "Sans artiste" },
      { "artist": "Sans titre" },
      null,
      42
    ] }"#;
    let library = spotify_export(raw).unwrap();
    assert_eq!(
      library.likes,
      vec![
        song("Aube", "Les Lunes", Some(215.0)),
        song("Brume", "Nord", Some(180.0)),
        song("Ciel", "Sud", Some(62.0)),
      ]
    );
    assert!(library.playlists.is_empty());
  }

  #[test]
  fn spotify_fields_are_trimmed_and_the_first_non_empty_variant_wins() {
    let raw =
      r#"{ "tracks": [{ "track": "", "trackName": "  Aube ", "artist": " Les Lunes  " }] }"#;
    let library = spotify_export(raw).unwrap();
    assert_eq!(library.likes, vec![song("Aube", "Les Lunes", None)]);
  }

  #[test]
  fn a_spotify_playlists_array_gives_playlists_without_the_play_time() {
    let raw = r#"{ "playlists": [
      { "name": "Route", "items": [
        { "track": { "trackName": "Aube", "artistName": "Les Lunes", "durationMs": 215000 } },
        { "track": { "track": "Brume", "artist": "Nord", "duration_ms": 90000 } },
        { "track": { "trackName": "Ciel", "artistName": "Sud", "msPlayed": 5000 } },
        { "track": null },
        {},
        null
      ] },
      { "name": "Vide", "items": [] },
      { "items": [{ "track": { "trackName": "X", "artistName": "Y" } }] }
    ] }"#;
    let library = spotify_export(raw).unwrap();
    assert!(library.likes.is_empty());
    assert_eq!(library.playlists.len(), 2);
    assert_eq!(library.playlists[0].name, "Route");
    assert_eq!(
      library.playlists[0].tracks,
      vec![
        song("Aube", "Les Lunes", Some(215.0)),
        song("Brume", "Nord", Some(90.0)),
        song("Ciel", "Sud", None),
      ]
    );
    assert_eq!(library.playlists[1].name, "Imported Playlist");
    assert_eq!(library.playlists[1].tracks, vec![song("X", "Y", None)]);
  }

  #[test]
  fn a_root_array_of_tracks_gives_likes() {
    let library = spotify_export(r#"[{ "track": "Aube", "artist": "Les Lunes" }]"#).unwrap();
    assert_eq!(library.likes, vec![song("Aube", "Les Lunes", None)]);
    assert!(library.playlists.is_empty());
  }

  #[test]
  fn a_root_array_of_playlists_gives_playlists() {
    let raw =
      r#"[{ "name": "P", "items": [{ "track": { "trackName": "A", "artistName": "B" } }] }]"#;
    let library = spotify_export(raw).unwrap();
    assert!(library.likes.is_empty());
    assert_eq!(library.playlists.len(), 1);
    assert_eq!(library.playlists[0].name, "P");
  }

  #[test]
  fn a_byte_order_mark_does_not_break_the_json() {
    let raw = "\u{feff}{ \"tracks\": [{ \"track\": \"A\", \"artist\": \"B\" }] }";
    assert_eq!(spotify_export(raw).unwrap().likes.len(), 1);
  }

  #[test]
  fn spotify_files_without_tracks_are_valid_and_empty() {
    for raw in ["{}", "[]", "42", "\"text\"", "null", r#"{ "tracks": 3 }"#] {
      assert!(spotify_export(raw).unwrap().is_empty(), "{raw}");
    }
  }

  #[test]
  fn an_empty_invalid_or_huge_spotify_file_is_refused() {
    assert_eq!(spotify_export(""), Err(ImportError::Empty));
    assert_eq!(spotify_export(" \r\n\t"), Err(ImportError::Empty));
    assert!(matches!(
      spotify_export("{ pas du json"),
      Err(ImportError::InvalidJson(_))
    ));
    let huge = " ".repeat(MAX_FILE_BYTES as usize + 1);
    assert_eq!(spotify_export(&huge), Err(ImportError::TooLarge));
  }

  // Apple : durée

  #[test]
  fn apple_durations_follow_the_forms_of_riff() {
    assert_eq!(apple_duration("3:45"), Some(225.0));
    assert_eq!(apple_duration(" 1:02:03 "), Some(3723.0));
    assert_eq!(apple_duration("215000"), Some(215.0));
    assert_eq!(apple_duration("215499"), Some(215.0));
    assert_eq!(apple_duration("200"), Some(200.0));
    assert_eq!(apple_duration("1000"), Some(1000.0));
    assert_eq!(apple_duration("1001"), Some(1.0));
    assert_eq!(apple_duration("12.4"), Some(12.0));
    assert_eq!(apple_duration(""), None);
    assert_eq!(apple_duration("   "), None);
    assert_eq!(apple_duration("a:b"), None);
    assert_eq!(apple_duration("1:2:3:4"), None);
    assert_eq!(apple_duration("abc"), None);
    assert_eq!(apple_duration("NaN"), None);
    assert_eq!(apple_duration("inf"), None);
  }

  // Apple : CSV

  #[test]
  fn an_apple_csv_with_commas_splits_likes_and_playlists() {
    let raw = "Title,Artist,Duration,Playlist\n\
               Aube,Les Lunes,3:35,\n\
               Brume,Nord,180000,Route\n\
               Ciel,Sud,61,Route\n\
               Orage,Est,,Soir\n";
    let library = apple_export_csv(raw).unwrap();
    assert_eq!(library.likes, vec![song("Aube", "Les Lunes", Some(215.0))]);
    assert_eq!(library.playlists.len(), 2);
    assert_eq!(library.playlists[0].name, "Route");
    assert_eq!(
      library.playlists[0].tracks,
      vec![
        song("Brume", "Nord", Some(180.0)),
        song("Ciel", "Sud", Some(61.0))
      ]
    );
    assert_eq!(library.playlists[1].name, "Soir");
    assert_eq!(
      library.playlists[1].tracks,
      vec![song("Orage", "Est", None)]
    );
  }

  #[test]
  fn an_apple_csv_with_semicolons_is_read() {
    let raw = "Name;Artist Name;Time;Playlist Name\r\nAube;Les Lunes;1:02:03;Route\r\n";
    let library = apple_export_csv(raw).unwrap();
    assert_eq!(library.playlists[0].name, "Route");
    assert_eq!(
      library.playlists[0].tracks,
      vec![song("Aube", "Les Lunes", Some(3723.0))]
    );
  }

  #[test]
  fn semicolons_are_not_a_separator_when_the_header_has_commas() {
    let raw = "Title,Artist\nA;B,C\n";
    let library = apple_export_csv(raw).unwrap();
    assert_eq!(library.likes, vec![song("A;B", "C", None)]);
  }

  #[test]
  fn an_apple_csv_with_tabs_is_read() {
    let raw = "Track Description\tArtist\tLength\nAube, version longue\tLes Lunes\t215000\n";
    let library = apple_export_csv(raw).unwrap();
    assert_eq!(
      library.likes,
      vec![song("Aube, version longue", "Les Lunes", Some(215.0))]
    );
  }

  #[test]
  fn doubled_quotes_and_quoted_separators_are_read() {
    let raw = "Title,Artist\n\"Say \"\"Hi\"\", now\",\"Duo, Les\"\n";
    let library = apple_export_csv(raw).unwrap();
    assert_eq!(
      library.likes,
      vec![song("Say \"Hi\", now", "Duo, Les", None)]
    );
  }

  #[test]
  fn apple_columns_fall_back_to_a_looser_name() {
    let raw =
      "\u{feff}\"Song Title\",\"Artist(s)\",\"Container Description\"\nAube,Les Lunes,Route\n";
    let library = apple_export_csv(raw).unwrap();
    assert_eq!(library.playlists[0].name, "Route");
    assert_eq!(
      library.playlists[0].tracks,
      vec![song("Aube", "Les Lunes", None)]
    );
  }

  #[test]
  fn apple_rows_without_title_or_artist_and_short_rows_are_skipped() {
    let raw = "Title,Artist,Playlist\n,Les Lunes,\nAube,,\nBrume\nCiel,Sud,Route\n";
    let library = apple_export_csv(raw).unwrap();
    assert!(library.likes.is_empty());
    assert_eq!(library.playlists.len(), 1);
    assert_eq!(library.playlists[0].tracks, vec![song("Ciel", "Sud", None)]);
  }

  #[test]
  fn an_apple_csv_without_a_usable_header_is_empty() {
    assert!(apple_export_csv("Foo,Bar\n1,2\n").unwrap().is_empty());
    assert!(apple_export_csv("Title,Artist\n").unwrap().is_empty());
  }

  #[test]
  fn an_empty_or_huge_apple_csv_is_refused() {
    assert_eq!(apple_export_csv(""), Err(ImportError::Empty));
    assert_eq!(apple_export_csv("\n\n"), Err(ImportError::Empty));
    let huge = "a".repeat(MAX_FILE_BYTES as usize + 1);
    assert_eq!(apple_export_csv(&huge), Err(ImportError::TooLarge));
  }

  // Apple : JSON

  #[test]
  fn a_nested_apple_json_is_walked_for_playlists_and_likes() {
    let value = json!({
      "playlists": [
        { "name": "Soir", "tracks": [
          { "name": "Aube", "artistName": "Les Lunes", "durationInMillis": 215000 },
          { "title": "Brume", "artist": "Nord", "duration": "3:05" }
        ] },
        { "title": "Matin", "items": [{ "trackName": "Ciel", "artist": "Sud", "time": "1:00" }] },
        { "playlistName": "Duo", "tracks": [{ "title": "A", "artist": "B", "type": "song" }] }
      ],
      "library": [
        { "type": "song", "name": "Orage", "artist": "Est" },
        { "name": "Sans type", "artist": "Ouest" }
      ]
    });
    let library = apple_export_json(&value);
    assert_eq!(library.likes, vec![song("Orage", "Est", None)]);
    let names: Vec<&str> = library.playlists.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Soir", "Matin", "Duo"]);
    assert_eq!(
      library.playlists[0].tracks,
      vec![
        song("Aube", "Les Lunes", Some(215.0)),
        song("Brume", "Nord", Some(185.0))
      ]
    );
    assert_eq!(
      library.playlists[1].tracks,
      vec![song("Ciel", "Sud", Some(60.0))]
    );
    assert_eq!(library.playlists[2].tracks, vec![song("A", "B", None)]);
  }

  #[test]
  fn apple_json_with_a_root_array_and_a_content_descriptor() {
    let value = json!([
      { "Track Name": "Aube", "Artist Name": "Les Lunes", "contentDescriptor": {} },
      { "song": "Brume", "artist_name": "Nord", "duration_ms": 90000 }
    ]);
    let library = apple_export_json(&value);
    assert_eq!(library.likes, vec![song("Brume", "Nord", Some(90.0))]);
    let value =
      json!([{ "track name": "Aube", "artist name": "Les Lunes", "contentDescriptor": 1 }]);
    assert_eq!(
      apple_export_json(&value).likes,
      vec![song("Aube", "Les Lunes", None)]
    );
  }

  #[test]
  fn apple_json_of_any_shape_never_panics() {
    for value in [
      json!(null),
      json!(3),
      json!("texte"),
      json!([]),
      json!({}),
      json!({ "tracks": "x", "name": "P" }),
      json!({ "items": [null, 1, [], {}], "title": "P" }),
      json!({ "title": 5, "artist": true, "duration": [] }),
    ] {
      assert!(apple_export_json(&value).is_empty(), "{value}");
    }
    let mut deep = json!({ "title": "A", "artist": "B", "type": "song" });
    for _ in 0..200 {
      deep = json!({ "next": [deep] });
    }
    let library = apple_export_json(&deep);
    assert!(library.likes.is_empty());
  }

  #[test]
  fn an_apple_file_is_read_as_json_by_its_extension_and_as_csv_otherwise() {
    let json_text = r#"{ "playlists": [{ "name": "P", "tracks": [{ "title": "A", "artist": "B", "type": "song" }] }] }"#;
    assert_eq!(
      apple_export("Library.JSON", json_text)
        .unwrap()
        .playlists
        .len(),
      1
    );
    assert!(matches!(
      apple_export("a.json", "pas du json"),
      Err(ImportError::InvalidJson(_))
    ));
    assert_eq!(
      apple_export("a.csv", "Title,Artist\nA,B\n")
        .unwrap()
        .likes
        .len(),
      1
    );
    assert_eq!(apple_export("a.json", ""), Err(ImportError::Empty));
    let huge = " ".repeat(MAX_FILE_BYTES as usize + 1);
    assert_eq!(apple_export("a.json", &huge), Err(ImportError::TooLarge));
  }

  // Bibliothèque

  #[test]
  fn the_library_value_has_the_shape_read_by_the_app() {
    let library = Library {
      likes: vec![song("A", "B", Some(10.0)), song("C", "D", None)],
      playlists: vec![ImportedPlaylist {
        name: "P".to_string(),
        tracks: vec![song("E", "F", Some(5.0))],
      }],
    };
    assert_eq!(
      library.to_value(true, true),
      json!({
        "likes": [
          { "title": "A", "artist": "B", "duration": 10.0 },
          { "title": "C", "artist": "D" }
        ],
        "playlists": [{ "name": "P", "tracks": [{ "title": "E", "artist": "F", "duration": 5.0 }] }]
      })
    );
    assert_eq!(library.to_value(false, true)["likes"], json!([]));
    assert_eq!(library.to_value(true, false)["playlists"], json!([]));
    assert_eq!(library.track_count(true, true), 3);
    assert_eq!(library.track_count(true, false), 2);
    assert_eq!(library.track_count(false, true), 1);
    assert_eq!(library.track_count(false, false), 0);
  }

  #[test]
  fn merging_keeps_the_order_of_the_files() {
    let mut first = Library {
      likes: vec![song("A", "B", None)],
      playlists: Vec::new(),
    };
    first.merge(Library {
      likes: vec![song("C", "D", None)],
      playlists: vec![ImportedPlaylist {
        name: "P".to_string(),
        tracks: vec![song("E", "F", None)],
      }],
    });
    assert_eq!(first.likes.len(), 2);
    assert_eq!(first.likes[1].title, "C");
    assert_eq!(first.playlists.len(), 1);
  }

  // Fichiers

  #[test]
  fn files_are_read_together_and_the_bad_ones_are_reported() {
    let dir = temp_dir();
    let likes = dir.join("YourLibrary.json");
    std::fs::write(
      &likes,
      r#"{ "tracks": [{ "track": "Aube", "artist": "Les Lunes" }] }"#,
    )
    .unwrap();
    let playlists = dir.join("Playlist1.json");
    std::fs::write(
      &playlists,
      r#"[{ "name": "P", "items": [{ "track": { "trackName": "A", "artistName": "B" } }] }]"#,
    )
    .unwrap();
    let empty = dir.join("vide.json");
    std::fs::write(&empty, "").unwrap();
    let broken = dir.join("casse.json");
    std::fs::write(&broken, "{ pas du json").unwrap();
    let missing = dir.join("absent.json");
    let huge = dir.join("enorme.json");
    std::fs::File::create(&huge)
      .unwrap()
      .set_len(MAX_FILE_BYTES + 1)
      .unwrap();

    let read = read_files(
      ExportSource::Spotify,
      &[likes, playlists, empty, broken, missing, huge],
    );
    assert_eq!(read.library.likes.len(), 1);
    assert_eq!(read.library.playlists.len(), 1);
    assert_eq!(
      read.names,
      [
        "YourLibrary.json",
        "Playlist1.json",
        "vide.json",
        "casse.json",
        "absent.json",
        "enorme.json"
      ]
    );
    let kinds: Vec<(&str, &ImportError)> = read
      .errors
      .iter()
      .map(|(name, error)| (name.as_str(), error))
      .collect();
    assert_eq!(kinds.len(), 4);
    assert_eq!(kinds[0], ("vide.json", &ImportError::Empty));
    assert!(matches!(
      kinds[1],
      ("casse.json", ImportError::InvalidJson(_))
    ));
    assert!(matches!(
      kinds[2],
      ("absent.json", ImportError::Unreadable(_))
    ));
    assert_eq!(kinds[3], ("enorme.json", &ImportError::TooLarge));
  }

  #[test]
  fn apple_files_follow_their_extension() {
    let dir = temp_dir();
    let csv = dir.join("Apple Music Library Tracks.csv");
    std::fs::write(&csv, "Title,Artist\nAube,Les Lunes\n").unwrap();
    let json = dir.join("Library.json");
    std::fs::write(
      &json,
      r#"[{ "name": "Orage", "artist": "Est", "type": "song" }]"#,
    )
    .unwrap();
    let read = read_files(ExportSource::Apple, &[csv, json]);
    assert_eq!(read.library.likes.len(), 2);
    assert!(read.errors.is_empty());
  }
}
