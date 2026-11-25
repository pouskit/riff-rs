//! `riff_core::services::lyrics`.
//!
//! Le dossier de données est partagé par tous les tests du binaire et `lyrics` y écrit un cache disque : chaque test
//! d'erreur utilise un couple titre/artiste que nul autre test ne charge avec succès.

use crate::common::{core_http_500, core_offline, core_recorded, json_of, misses, query};
use riff_core::services::lyrics;
use serde_json::json;

fn no_misses() {
  let m = misses();
  assert!(m.is_empty(), "fixtures manquantes :\n{}", m.join("\n"));
}

#[test]
fn is_lyrics_explicit_flags_strong_words() {
  assert!(lyrics::is_lyrics_explicit("what the fuck"));
  assert!(lyrics::is_lyrics_explicit("[00:01.00] Shit happens"));
}

#[test]
fn is_lyrics_explicit_ignores_clean_text_and_empty() {
  assert!(!lyrics::is_lyrics_explicit(""));
  assert!(!lyrics::is_lyrics_explicit(
    "Ligne factice 1, ligne factice 2"
  ));
  assert!(!lyrics::is_lyrics_explicit("Scunthorpe classic"));
}

#[tokio::test]
async fn lyrics_returns_the_recorded_synced_lyrics() {
  let core = core_recorded();
  let params = query(&[("title", "One More Time"), ("artist", "Daft Punk")]);
  let (status, body) = json_of(lyrics::lyrics(&core, params).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["success"], true);
  assert_eq!(body["isCustom"], false);
  assert_eq!(body["isExplicit"], false);
  let text = body["lyrics"].as_str().expect("texte");
  assert!(text.starts_with("[00:30.75] Ligne factice 1"), "{text}");
}

#[tokio::test]
async fn lyrics_of_an_instrumental_title_needs_no_request() {
  let core = core_offline();
  let params = query(&[
    ("title", "Night Drive (Instrumental)"),
    ("artist", "Nobody Instr"),
  ]);
  let (status, body) = json_of(lyrics::lyrics(&core, params).await);
  assert_eq!(status, 200);
  assert_eq!(body["instrumental"], true);
  assert!(misses().is_empty());
}

#[tokio::test]
async fn lyrics_without_network_succeeds_with_empty_text() {
  let core = core_offline();
  let params = query(&[("title", "Offline Song"), ("artist", "Offline Artist")]);
  let (status, body) = json_of(lyrics::lyrics(&core, params).await);
  assert_eq!(status, 200);
  assert_eq!(body["lyrics"], "");
  assert!(
    misses().iter().any(|m| m.contains("lrclib.net")),
    "lrclib a été interrogé"
  );
}

#[tokio::test]
async fn lyrics_http_500_from_lrclib_falls_through_to_empty_text() {
  let core = core_http_500(&[(
    "GET",
    "https://lrclib.net/api/get?artist_name=Err500+Artist&track_name=Err500+Song",
  )]);
  let params = query(&[("title", "Err500 Song"), ("artist", "Err500 Artist")]);
  let (status, body) = json_of(lyrics::lyrics(&core, params).await);
  assert_eq!(status, 200);
  assert_eq!(body["lyrics"], "");
  let missed = misses();
  assert!(
    !missed.iter().any(|m| m.contains("lrclib.net/api/get")),
    "le 500 de /api/get a été rejoué, la recherche lrclib a suivi"
  );
  assert!(!missed.is_empty(), "les autres sources ont été essayées");
}

#[tokio::test]
async fn lyrics_offline_mode_does_not_touch_the_network() {
  let core = core_offline();
  let params = query(&[
    ("title", "Local Only"),
    ("artist", "Nobody Local"),
    ("online", "0"),
  ]);
  let (_, body) = json_of(lyrics::lyrics(&core, params).await);
  assert_eq!(
    body,
    json!({"success": true, "lyrics": "", "isCustom": false, "isExplicit": false})
  );
  assert!(misses().is_empty());
}

#[tokio::test]
async fn lyrics_words_returns_the_recorded_word_timing() {
  let core = core_recorded();
  let params = query(&[("title", "One More Time"), ("artist", "Daft Punk")]);
  let (status, body) = json_of(lyrics::lyrics_words(&core, params).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["success"], true);
  let lines = body["lines"].as_array().expect("lignes");
  assert!(!lines.is_empty());
  assert!(lines[0]["words"].is_array() && lines[0]["text"].is_string());
}

#[tokio::test]
async fn lyrics_words_without_network_is_not_a_success() {
  let core = core_offline();
  let params = query(&[("title", "Offline Song"), ("artist", "Offline Artist")]);
  let (status, body) = json_of(lyrics::lyrics_words(&core, params).await);
  assert_eq!(
    (status, body),
    (200, json!({"success": false, "lines": []}))
  );
  assert!(!misses().is_empty());
}
