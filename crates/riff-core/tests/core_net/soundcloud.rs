//! `riff_core::services::soundcloud`.
//!
//! Aucune fixture SoundCloud n'est enregistrée (le `client_id` est extrait de bundles JavaScript de plusieurs Mo).
//! Les cas de succès utilisent donc des fixtures écrites à la main qui respectent les expressions régulières de
//! `scrape_sc_client_id` : une page avec un `<script src="https://a-v2.sndcdn.com/assets/...js">` et un bundle qui
//! contient `client_id:"<32 caractères>"`. Les données sont factices.

use crate::common::{core_hand_written, core_offline, json_of, misses, query, write_fixture};
use riff_core::services::search;
use riff_core::services::soundcloud::{
  fetch_direct_sc_stream, fetch_sc_search, normalize_soundcloud_url, sc_client_id,
};
use riff_core::services::stream::{prefetch_track, stream_info};
use serde_json::json;
use std::path::Path;

const CID: &str = "abcdefghij0123456789ABCDEFGHIJ01";
const BUNDLE: &str = "https://a-v2.sndcdn.com/assets/app-fake.js";

fn no_misses() {
  let m = misses();
  assert!(m.is_empty(), "fixtures manquantes :\n{}", m.join("\n"));
}

/// Page d'accueil factice et bundle factice qui donnent `CID`.
fn write_scrape(dir: &Path) {
  write_fixture(
    dir,
    "GET",
    "https://soundcloud.com/",
    200,
    &format!(r#"<html><script crossorigin src="{BUNDLE}"></script></html>"#),
    "text/html",
  );
  write_fixture(
    dir,
    "GET",
    BUNDLE,
    200,
    &format!(r#"var cfg={{client_id:"{CID}",env:"production"}};"#),
    "application/javascript",
  );
}

const SEARCH_BODY: &str = r#"{"collection":[
  {"id":123456789,"title":"Fake Track","user":{"username":"Fake User"},"duration":200000,
   "artwork_url":"https://i1.sndcdn.com/artworks-fake-large.jpg","permalink_url":"https://soundcloud.com/fake/fake-track"},
  {"id":0,"title":""}
]}"#;

fn search_url(query: &str) -> String {
  format!("https://api-v2.soundcloud.com/search/tracks?q={query}&limit=40&client_id={CID}")
}

#[test]
fn normalize_soundcloud_url_rewrites_track_urns() {
  assert_eq!(
    normalize_soundcloud_url("soundcloud:tracks:42"),
    "https://api.soundcloud.com/tracks/42"
  );
  assert_eq!(
    normalize_soundcloud_url("https%3A%2F%2Fx%2Fsoundcloud%3Atracks%3A7"),
    "https://api.soundcloud.com/tracks/7"
  );
}

#[test]
fn normalize_soundcloud_url_keeps_other_input() {
  assert_eq!(
    normalize_soundcloud_url("https://soundcloud.com/a/b"),
    "https://soundcloud.com/a/b"
  );
  assert_eq!(normalize_soundcloud_url(""), "");
}

#[tokio::test]
async fn sc_client_id_is_scraped_then_cached() {
  let core = core_hand_written(write_scrape);
  assert_eq!(sc_client_id(&core, None).await, CID);
  no_misses();
  // Deuxième appel : valeur en cache, aucune requête (les fixtures resteraient lisibles, mais le cache suffit).
  assert_eq!(sc_client_id(&core, None).await, CID);
  no_misses();
}

#[tokio::test]
async fn sc_client_id_without_network_is_empty() {
  let core = core_offline();
  assert_eq!(sc_client_id(&core, None).await, "");
  assert_eq!(misses().len(), 1, "seule la page d'accueil est demandée");
}

#[tokio::test]
async fn sc_client_id_without_a_matching_bundle_is_empty() {
  let core = core_hand_written(|dir| {
    write_scrape(dir);
    write_fixture(
      dir,
      "GET",
      BUNDLE,
      200,
      "no id in this bundle",
      "text/javascript",
    );
  });
  assert_eq!(sc_client_id(&core, None).await, "");
  no_misses();
}

#[tokio::test]
async fn fetch_sc_search_maps_a_hand_written_collection() {
  let core = core_hand_written(|dir| {
    write_scrape(dir);
    write_fixture(
      dir,
      "GET",
      &search_url("lofi"),
      200,
      SEARCH_BODY,
      "application/json",
    );
  });
  let tracks = fetch_sc_search(&core, "lofi", 40).await;
  no_misses();
  assert_eq!(tracks.len(), 1, "l'entrée sans titre est écartée");
  assert_eq!(tracks[0]["id"], "123456789");
  assert_eq!(tracks[0]["artist"], "Fake User");
  assert_eq!(tracks[0]["duration"], 200);
  assert_eq!(tracks[0]["platform"], "soundcloud");
  assert_eq!(
    tracks[0]["thumbnail"],
    "https://i1.sndcdn.com/artworks-fake-t500x500.jpg"
  );
}

#[tokio::test]
async fn fetch_sc_search_without_network_is_empty() {
  let core = core_offline();
  assert!(fetch_sc_search(&core, "lofi", 40).await.is_empty());
  assert!(!misses().is_empty());
}

#[tokio::test]
async fn fetch_sc_search_http_500_is_empty() {
  let core = core_hand_written(|dir| {
    write_scrape(dir);
    write_fixture(
      dir,
      "GET",
      &search_url("err500"),
      500,
      "Internal Server Error",
      "text/plain",
    );
  });
  assert!(fetch_sc_search(&core, "err500", 40).await.is_empty());
  no_misses();
}

#[tokio::test]
async fn fetch_direct_sc_stream_resolves_a_progressive_transcoding() {
  let media =
    "https://api-v2.soundcloud.com/media/soundcloud:tracks:123456789/fake/stream/progressive";
  let core = core_hand_written(|dir| {
    write_scrape(dir);
    write_fixture(
      dir,
      "GET",
      &format!("https://api-v2.soundcloud.com/tracks/123456789?client_id={CID}"),
      200,
      &format!(
        r#"{{"id":123456789,"track_authorization":"auth tok","media":{{"transcodings":[
          {{"url":"{media}","format":{{"protocol":"progressive","mime_type":"audio/mpeg"}}}}]}}}}"#
      ),
      "application/json",
    );
    write_fixture(
      dir,
      "GET",
      &format!("{media}?client_id={CID}&track_authorization=auth%20tok"),
      200,
      r#"{"url":"https://cf-media.sndcdn.com/fake.mp3"}"#,
      "application/json",
    );
  });
  let url = fetch_direct_sc_stream(&core, "123456789", "").await;
  no_misses();
  assert_eq!(url.as_deref(), Some("https://cf-media.sndcdn.com/fake.mp3"));
}

#[tokio::test]
async fn fetch_direct_sc_stream_without_network_is_none() {
  let core = core_offline();
  assert_eq!(
    fetch_direct_sc_stream(&core, "123456789", "lofi").await,
    None
  );
  assert!(!misses().is_empty());
}

#[tokio::test]
async fn fetch_direct_sc_stream_rejects_preview_urls() {
  let media =
    "https://api-v2.soundcloud.com/media/soundcloud:tracks:222222222/fake/stream/progressive";
  let core = core_hand_written(|dir| {
    write_scrape(dir);
    write_fixture(
      dir,
      "GET",
      &format!("https://api-v2.soundcloud.com/tracks/222222222?client_id={CID}"),
      200,
      &format!(
        r#"{{"id":222222222,"media":{{"transcodings":[{{"url":"{media}","format":{{"protocol":"progressive"}}}}]}}}}"#
      ),
      "application/json",
    );
    write_fixture(
      dir,
      "GET",
      &format!("{media}?client_id={CID}"),
      200,
      r#"{"url":"https://cf-preview-media.sndcdn.com/preview/x.mp3"}"#,
      "application/json",
    );
  });
  assert_eq!(fetch_direct_sc_stream(&core, "222222222", "").await, None);
  no_misses();
}

// ------------------------------------------------- HLS SoundCloud et stream_info

const FAKE_ARTIST_TITLE_URL: &str =
  "https://api-v2.soundcloud.com/search/tracks?q=Fake%20Artist%20-%20Fake%20Title&limit=5";

/// Recherche SoundCloud factice dont l'unique piste a une transcodification `protocol` qui rend `final_url`.
fn write_sc_search_with_transcoding(dir: &Path, protocol: &str, final_url: &str) {
  let media = "https://api-v2.soundcloud.com/media/soundcloud:tracks:123456789/fake/stream/x";
  write_scrape(dir);
  write_fixture(
    dir,
    "GET",
    &format!("{FAKE_ARTIST_TITLE_URL}&client_id={CID}"),
    200,
    &format!(
      r#"{{"collection":[{{"id":123456789,"duration":200000,"media":{{"transcodings":[
        {{"url":"{media}","format":{{"protocol":"{protocol}","mime_type":"audio/mpeg"}}}}]}}}}]}}"#
    ),
    "application/json",
  );
  write_fixture(
    dir,
    "GET",
    &format!("{media}?client_id={CID}"),
    200,
    &format!(r#"{{"url":"{final_url}"}}"#),
    "application/json",
  );
}

fn youtube_params() -> riff_core::reply::Params {
  query(&[
    ("url", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
    ("id", "dQw4w9WgXcQ"),
    ("platform", "youtube"),
    ("title", "Fake Title"),
    ("artist", "Fake Artist"),
    ("timeout", "5"),
  ])
}

#[tokio::test]
async fn stream_info_of_a_youtube_id_discards_a_soundcloud_hls_stream() {
  let core = core_hand_written(|dir| {
    write_sc_search_with_transcoding(dir, "hls", "https://cf-hls.sndcdn.com/fake/playlist.m3u8");
  });
  let (status, out) = json_of(stream_info(&core, youtube_params()).await);
  // yt-dlp est un faux exécutable et la recherche YouTube n'a pas de fixture : la résolution va jusqu'au bout et échoue.
  assert_eq!(status, 500, "{out}");
  assert_eq!(out["success"], false);
  assert!(out.get("rawStreamUrl").is_none() && out.get("isHls").is_none());
}

#[tokio::test]
async fn stream_info_of_a_youtube_id_still_accepts_a_progressive_soundcloud_stream() {
  let core = core_hand_written(|dir| {
    write_sc_search_with_transcoding(dir, "progressive", "https://cf-media.sndcdn.com/fake.mp3");
  });
  let (status, out) = json_of(stream_info(&core, youtube_params()).await);
  assert_eq!(status, 200, "{out}");
  assert_eq!(out["rawStreamUrl"], "https://cf-media.sndcdn.com/fake.mp3");
  assert_eq!(out["isHls"], false);
}

#[tokio::test]
async fn stream_info_of_a_soundcloud_track_keeps_its_hls_stream() {
  let media = "https://api-v2.soundcloud.com/media/soundcloud:tracks:123456789/fake/stream/hls";
  let hls = "https://cf-hls.sndcdn.com/fake/playlist.m3u8";
  let core = core_hand_written(|dir| {
    write_scrape(dir);
    write_fixture(
      dir,
      "GET",
      &format!("https://api-v2.soundcloud.com/tracks/123456789?client_id={CID}"),
      200,
      &format!(
        r#"{{"id":123456789,"media":{{"transcodings":[
          {{"url":"{media}","format":{{"protocol":"hls","mime_type":"audio/mpeg"}}}}]}}}}"#
      ),
      "application/json",
    );
    write_fixture(
      dir,
      "GET",
      &format!("{media}?client_id={CID}"),
      200,
      &format!(r#"{{"url":"{hls}"}}"#),
      "application/json",
    );
  });
  let params = query(&[
    ("id", "123456789"),
    ("platform", "soundcloud"),
    ("timeout", "5"),
  ]);
  let (status, out) = json_of(stream_info(&core, params).await);
  assert_eq!(status, 200, "{out}");
  assert_eq!(out["rawStreamUrl"], hls);
  assert_eq!(out["isHls"], true);
}

const PERMALINK: &str = "https://soundcloud.com/fake/fake-track";

/// Piste dont le permalink ne contient pas d'id : seule la résolution par id numérique passe.
fn write_hls_track_by_id(dir: &Path, hls: &str) {
  let media = "https://api-v2.soundcloud.com/media/soundcloud:tracks:123456789/fake/stream/hls";
  write_scrape(dir);
  write_fixture(
    dir,
    "GET",
    &format!("https://api-v2.soundcloud.com/tracks/123456789?client_id={CID}"),
    200,
    &format!(
      r#"{{"id":123456789,"media":{{"transcodings":[
        {{"preset":"aac_160k","url":"{media}","format":{{"protocol":"hls","mime_type":"audio/mp4; codecs=\"mp4a.40.2\""}}}}]}}}}"#
    ),
    "application/json",
  );
  write_fixture(
    dir,
    "GET",
    &format!("{media}?client_id={CID}"),
    200,
    &format!(r#"{{"url":"{hls}"}}"#),
    "application/json",
  );
}

#[tokio::test]
async fn stream_info_of_a_permalink_with_a_numeric_id_resolves_the_hls_stream_directly() {
  let hls = "https://cf-hls.sndcdn.com/fake/playlist.m3u8?expires=4102444800";
  let core = core_hand_written(|dir| write_hls_track_by_id(dir, hls));
  let params = query(&[
    ("url", PERMALINK),
    ("id", "123456789"),
    ("platform", "soundcloud"),
    ("timeout", "5"),
  ]);
  let (status, out) = json_of(stream_info(&core, params).await);
  no_misses();
  assert_eq!(status, 200, "{out}");
  assert_eq!(out["rawStreamUrl"], hls);
  assert_eq!(out["isHls"], true);
}

#[tokio::test]
async fn prefetch_of_a_permalink_with_a_numeric_id_fills_the_cache() {
  let hls = "https://cf-hls.sndcdn.com/fake/playlist.m3u8?expires=4102444800";
  let core = core_hand_written(|dir| write_hls_track_by_id(dir, hls));
  prefetch_track(&core, PERMALINK, "123456789", "soundcloud").await;
  no_misses();
  let cache = core.caches.stream_url_cache.lock().expect("verrou");
  let (_, (_, data)) = cache
    .iter()
    .find(|(key, _)| key.starts_with("123456789_"))
    .expect("entrée de cache");
  assert_eq!(data["rawStreamUrl"], hls);
}

#[tokio::test]
async fn soundcloud_search_without_transport_reports_offline() {
  let core = core_offline();
  let (status, body) = json_of(
    search::search(
      &core,
      query(&[("q", "hors ligne"), ("platform", "soundcloud")]),
    )
    .await,
  );
  assert_eq!(status, 200);
  assert_eq!(
    body,
    json!({"success": false, "error": "Offline: no internet connection", "offline": true})
  );
}

#[tokio::test]
async fn soundcloud_search_with_a_response_but_no_result_stays_a_success() {
  let core = core_hand_written(|dir| {
    write_scrape(dir);
    write_fixture(
      dir,
      "GET",
      &search_url("aucun"),
      200,
      r#"{"collection":[]}"#,
      "application/json",
    );
  });
  let (status, body) =
    json_of(search::search(&core, query(&[("q", "aucun"), ("platform", "soundcloud")])).await);
  assert_eq!(status, 200);
  assert_eq!(body["success"], true);
  assert_eq!(body["tracks"], json!([]));
}
