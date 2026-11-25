//! `riff_core::services::playlist::playlist_import`, chemin Deezer (fixture `playlist/3155776842`).
//!
//! Les chemins YouTube et SoundCloud passent par `yt-dlp` (processus externe) : non testés ici.

use crate::common::{core_http_500, core_offline, core_recorded, json_of, misses, query};
use riff_core::services::playlist::playlist_import;

fn deezer(url: &str) -> riff_core::reply::Params {
  query(&[("url", url)])
}

#[tokio::test]
async fn deezer_playlist_is_imported_from_the_recorded_response() {
  let core = core_recorded();
  let (status, out) =
    json_of(playlist_import(&core, deezer("https://www.deezer.com/playlist/3155776842")).await);
  assert!(misses().is_empty(), "fixtures manquantes : {:?}", misses());
  assert_eq!(status, 200);
  assert_eq!(out["name"], "Top Worldwide");
  assert!(out["cover"].as_str().unwrap_or("").starts_with("https://"));
  let tracks = out["tracks"].as_array().expect("tracks");
  assert!(!tracks.is_empty());
  assert!(
    tracks
      .iter()
      .all(|t| t["platform"] == "deezer" && t["id"].is_string())
  );
}

#[tokio::test]
async fn deezer_playlist_without_network_is_a_502() {
  let core = core_offline();
  let (status, out) =
    json_of(playlist_import(&core, deezer("https://www.deezer.com/playlist/1")).await);
  assert_eq!(status, 502);
  assert_eq!(out["error"], "Deezer playlist was not found");
  assert_eq!(misses().len(), 1);
}

#[tokio::test]
async fn deezer_playlist_http_500_is_a_502() {
  let core = core_http_500(&[("GET", "https://api.deezer.com/playlist/2")]);
  let (status, out) =
    json_of(playlist_import(&core, deezer("https://www.deezer.com/playlist/2")).await);
  assert_eq!(status, 502);
  assert_eq!(out["success"], false);
  assert!(misses().is_empty(), "le 500 écrit à la main a été rejoué");
}

#[tokio::test]
async fn unsupported_links_are_a_400() {
  let core = core_offline();
  for url in [
    "",
    "not a url",
    "https://example.com/playlist/1",
    "https://www.deezer.com/album/1",
  ] {
    let (status, out) = json_of(playlist_import(&core, deezer(url)).await);
    assert_eq!(status, 400, "{url}");
    assert_eq!(out["success"], false);
  }
  assert!(misses().is_empty());
}

/// Comme l'original (`server.js`, motif `^/playlist/(\d+)`), un lien avec préfixe de langue est refusé.
#[tokio::test]
async fn deezer_link_with_a_locale_prefix_is_unsupported_like_the_original() {
  let core = core_offline();
  let (status, out) = json_of(
    playlist_import(
      &core,
      deezer("https://www.deezer.com/fr/playlist/3155776842"),
    )
    .await,
  );
  assert_eq!(
    (status, out["error"].as_str()),
    (400, Some("Unsupported Deezer link"))
  );
  assert!(misses().is_empty());
}
