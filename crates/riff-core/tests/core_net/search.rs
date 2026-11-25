//! `riff_core::services::search`.
//!
//! Les requêtes POST (YouTube) portent un corps JSON : les fixtures écrites à la main reconstruisent ce corps avec
//! la même charge utile que le code (même ordre de clés), sinon la clé de fixture ne correspond pas.

use crate::common::{
  core_hand_written, core_offline, core_recorded, json_of, misses, query, write_fixture,
  write_fixture_with_body,
};
use riff_core::services::search::{
  self, YTM_CLIENT_VERSION, YTM_SONGS_PARAMS, direct_youtube_search, parse_ytm_songs,
  ytm_search_request, ytmusic_search,
};
use serde_json::{Value, json};

const YTM_URL: &str = "https://music.youtube.com/youtubei/v1/search?prettyPrint=false";
const YT_URL: &str = "https://www.youtube.com/youtubei/v1/search?prettyPrint=false";

fn no_misses() {
  let m = misses();
  assert!(m.is_empty(), "fixtures manquantes :\n{}", m.join("\n"));
}

/// Corps exact envoyé par `ytm_search_request`.
fn ytm_payload(query: &str) -> Vec<u8> {
  let mut payload = json!({
    "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": YTM_CLIENT_VERSION, "hl": "en", "gl": "US" } },
    "query": query
  });
  payload["params"] = json!(YTM_SONGS_PARAMS);
  serde_json::to_vec(&payload).expect("JSON")
}

/// Corps exact envoyé par `direct_youtube_search`.
fn yt_payload(query: &str) -> Vec<u8> {
  let payload = json!({
    "context": { "client": { "clientName": "WEB", "clientVersion": "2.20250101.00.00", "hl": "en", "gl": "US" } },
    "query": query
  });
  serde_json::to_vec(&payload).expect("JSON")
}

/// Réponse minimale de la recherche YouTube : une vidéo musicale.
const YT_RESULT: &str = r#"{"contents":{"twoColumnSearchResultsRenderer":{"primaryContents":{"sectionListRenderer":{"contents":[{"itemSectionRenderer":{"contents":[
  {"videoRenderer":{"videoId":"hw0000000aa","title":{"runs":[{"text":"Fake Song"}]},"ownerText":{"runs":[{"text":"Fake Artist - Topic"}]},"lengthText":{"simpleText":"3:21"},"thumbnail":{"thumbnails":[{"url":"https://i.ytimg.com/vi/hw0000000aa/hq.jpg"}]}}},
  {"adSlotRenderer":{}}
]}}]}}}}}"#;

#[tokio::test]
async fn search_returns_recorded_youtube_music_results() {
  let core = core_recorded();
  let (status, body) =
    json_of(search::search(&core, query(&[("q", "daft punk"), ("limit", "5")])).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["success"], true);
  let tracks = body["tracks"].as_array().expect("tracks");
  assert!(!tracks.is_empty() && tracks.len() <= 5);
  assert!(
    tracks
      .iter()
      .all(|t| t["platform"] == "youtube" && t["isCached"] == false)
  );
  assert!(body["hasMore"].is_boolean());
}

#[tokio::test]
async fn search_with_an_empty_query_returns_nothing() {
  let core = core_offline();
  let (status, body) = json_of(search::search(&core, query(&[("q", "  ")])).await);
  assert_eq!(
    (status, body),
    (
      200,
      json!({"success": true, "tracks": [], "hasMore": false})
    )
  );
  assert!(misses().is_empty());
}

#[tokio::test]
async fn search_without_network_reports_offline_and_tries_every_source() {
  let core = core_offline();
  let (status, body) = json_of(search::search(&core, query(&[("q", "zzz introuvable")])).await);
  assert_eq!(status, 200);
  assert_eq!(body["offline"], true);
  assert_eq!(body["success"], false);
  let missed = misses().join("\n");
  assert!(missed.contains("music.youtube.com") && missed.contains("api.deezer.com"));
}

#[tokio::test]
async fn parse_ytm_songs_reads_the_recorded_response() {
  // Réponse enregistrée relue via le transport de rejeu.
  let core = core_recorded();
  let json_val = ytm_search_request(
    &core.net.http_client,
    "daft punk",
    Some(YTM_SONGS_PARAMS),
    10_000,
    0,
  )
  .await
  .expect("réponse enregistrée");
  no_misses();
  let songs = parse_ytm_songs(&json_val);
  assert!(!songs.is_empty());
  assert!(
    songs
      .iter()
      .all(|s| s["id"].as_str().is_some_and(|id| !id.is_empty()))
  );
}

#[test]
fn parse_ytm_songs_of_an_unexpected_document_is_empty() {
  assert!(parse_ytm_songs(&json!({})).is_empty());
  assert!(parse_ytm_songs(&json!({"contents": 3})).is_empty());
  assert!(parse_ytm_songs(&Value::Null).is_empty());
}

#[tokio::test]
async fn ytmusic_search_maps_the_recorded_songs() {
  let core = core_recorded();
  let tracks = ytmusic_search(&core, "daft punk", 10_000, 0, false).await;
  no_misses();
  assert!(!tracks.is_empty());
  assert!(
    tracks
      .iter()
      .all(|t| t["url"].as_str().is_some_and(|u| u.contains("watch?v=")))
  );
}

#[tokio::test]
async fn ytmusic_search_without_network_is_empty_with_and_without_fallback() {
  let core = core_offline();
  assert!(
    ytmusic_search(&core, "rien", 1000, 1, false)
      .await
      .is_empty()
  );
  assert_eq!(misses().len(), 2, "un essai plus une reprise");
  assert!(
    ytmusic_search(&core, "rien", 1000, 0, true)
      .await
      .is_empty()
  );
  let missed = misses().join("\n");
  assert!(
    missed.contains("www.youtube.com"),
    "repli vers la recherche directe"
  );
}

#[tokio::test]
async fn ytm_search_request_returns_the_recorded_document() {
  let core = core_recorded();
  let value = ytm_search_request(
    &core.net.http_client,
    "daft punk",
    Some(YTM_SONGS_PARAMS),
    10_000,
    0,
  )
  .await;
  no_misses();
  assert!(value.is_some_and(|v| v.is_object()));
}

#[tokio::test]
async fn ytm_search_request_http_500_retries_then_gives_up() {
  let core = core_hand_written(|dir| {
    write_fixture_with_body(
      dir,
      "POST",
      YTM_URL,
      &ytm_payload("err500"),
      500,
      "Internal Server Error",
      "text/plain",
    );
  });
  let value = ytm_search_request(
    &core.net.http_client,
    "err500",
    Some(YTM_SONGS_PARAMS),
    1000,
    2,
  )
  .await;
  no_misses();
  assert!(value.is_none());
}

#[tokio::test]
async fn direct_youtube_search_parses_a_hand_written_response() {
  let core = core_hand_written(|dir| {
    write_fixture_with_body(
      dir,
      "POST",
      YT_URL,
      &yt_payload("fake song"),
      200,
      YT_RESULT,
      "application/json",
    );
  });
  let tracks = direct_youtube_search(&core.net.http_client, "fake song", true, 10).await;
  no_misses();
  assert_eq!(tracks.len(), 1);
  assert_eq!(tracks[0]["id"], "hw0000000aa");
  assert_eq!(tracks[0]["title"], "Fake Song");
  assert_eq!(tracks[0]["artist"], "Fake Artist");
  assert_eq!(tracks[0]["duration"], 201);
  assert_eq!(
    tracks[0]["url"],
    "https://www.youtube.com/watch?v=hw0000000aa"
  );
}

#[tokio::test]
async fn direct_youtube_search_http_500_and_missing_fixture_are_empty() {
  let core = core_hand_written(|dir| {
    write_fixture_with_body(
      dir,
      "POST",
      YT_URL,
      &yt_payload("err500"),
      500,
      "Internal Server Error",
      "text/plain",
    );
  });
  assert!(
    direct_youtube_search(&core.net.http_client, "err500", true, 10)
      .await
      .is_empty()
  );
  no_misses();
  assert!(
    direct_youtube_search(&core.net.http_client, "inconnu", true, 10)
      .await
      .is_empty()
  );
  assert_eq!(misses().len(), 1);
}

#[tokio::test]
async fn search_without_transport_reports_offline_for_each_youtube_platform() {
  for platform in ["ytmusic", "youtube"] {
    let core = core_offline();
    let (status, body) = json_of(
      search::search(
        &core,
        query(&[("q", "zzz introuvable"), ("platform", platform)]),
      )
      .await,
    );
    assert_eq!(status, 200);
    assert_eq!(
      body,
      json!({"success": false, "error": "Offline: no internet connection", "offline": true}),
      "{platform}"
    );
  }
  misses();
}

#[tokio::test]
async fn search_with_a_response_but_no_result_stays_a_success() {
  // YouTube injoignable, mais Deezer répond (vide) : ce n'est pas une panne de transport.
  let core = core_hand_written(|dir| {
    write_fixture(
      dir,
      "GET",
      "https://api.deezer.com/search?q=aucun&limit=30",
      200,
      r#"{"data":[]}"#,
      "application/json",
    );
  });
  let (status, body) = json_of(search::search(&core, query(&[("q", "aucun")])).await);
  misses();
  assert_eq!(status, 200);
  assert_eq!(body["success"], true);
  assert_eq!(body["tracks"], json!([]));
}
