//! `riff_core::discovery` : un test de succès et un test d'erreur par fonction publique.
//!
//! Succès : réponses enregistrées (`tests/fixtures/http`), zéro fixture manquante exigé, valeur tirée de la fixture.
//! Erreur : entrée invalide, fixture manquante (réseau indisponible) ou réponse 500 écrite à la main. Plusieurs
//! fonctions avalent l'échec réseau et renvoient un résultat vide : le test vérifie alors que la requête est bien
//! partie (`misses()` non vide ou 500 rejoué).

use crate::common::{
  body, core_hand_written, core_http_500, core_offline, core_recorded, json_of, misses, query,
  write_fixture,
};
use riff_core::discovery;
use serde_json::{Value, json};
use std::collections::HashSet;

fn no_misses() {
  let m = misses();
  assert!(m.is_empty(), "fixtures manquantes :\n{}", m.join("\n"));
}

fn tracks_of(body: &Value) -> &Vec<Value> {
  body["tracks"].as_array().expect("tracks")
}

const DAFT_SEED: &str = r#"{"seeds":[{"title":"One More Time","artist":"Daft Punk"}],"limit":5}"#;

fn daft_seed() -> Value {
  json!({ "artist": "Daft Punk", "title": "One More Time", "id": "seed-id" })
}

// ---------------------------------------------------------------- album_search

#[tokio::test]
async fn album_search_returns_the_recorded_albums() {
  let core = core_recorded();
  let (status, body) = json_of(discovery::album_search(&core, query(&[("q", "daft punk")])).await);
  no_misses();
  assert_eq!(status, 200);
  let albums = body["albums"].as_array().expect("albums");
  assert!(albums.len() <= 12 && !albums.is_empty());
  assert!(
    albums
      .iter()
      .any(|a| a["title"] == "Discovery" && a["artist"] == "Daft Punk")
  );
}

#[tokio::test]
async fn album_search_without_network_returns_an_empty_list() {
  let core = core_offline();
  let (status, body) = json_of(discovery::album_search(&core, query(&[("q", "daft punk")])).await);
  assert_eq!(status, 200);
  assert_eq!(body, json!({ "success": true, "albums": [] }));
  assert_eq!(misses().len(), 2, "les deux recherches iTunes sont parties");
}

#[tokio::test]
async fn album_search_survives_http_500() {
  let core = core_http_500(&[
    (
      "GET",
      "https://itunes.apple.com/search?term=daft%20punk&entity=album&media=music&limit=25",
    ),
    (
      "GET",
      "https://itunes.apple.com/search?term=daft%20punk&media=music&limit=25",
    ),
  ]);
  let (status, body) = json_of(discovery::album_search(&core, query(&[("q", "daft punk")])).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["albums"], json!([]));
}

#[tokio::test]
async fn album_search_blank_query_returns_nothing_without_a_request() {
  let core = core_offline();
  let (status, body) = json_of(discovery::album_search(&core, query(&[("q", "  ")])).await);
  assert_eq!((status, &body["albums"]), (200, &json!([])));
  no_misses();
}

// ---------------------------------------------------------------- album_tracks

#[tokio::test]
async fn album_tracks_returns_the_recorded_tracks() {
  let core = core_recorded();
  let params = query(&[("title", "Discovery"), ("artist", "Daft Punk")]);
  let (status, body) = json_of(discovery::album_tracks(&core, params).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["album"]["title"], "Discovery");
  assert_eq!(body["album"]["artist"], "Daft Punk");
  let tracks = tracks_of(&body);
  assert!(tracks.iter().any(|t| t["title"] == "Short Circuit"));
  assert_eq!(body["album"]["trackCount"], json!(tracks.len()));
}

#[tokio::test]
async fn album_tracks_rejects_a_request_without_id_or_title() {
  let core = core_offline();
  let (status, body) = json_of(discovery::album_tracks(&core, query(&[("artist", "x")])).await);
  assert_eq!(status, 400);
  assert_eq!(body["success"], false);
  no_misses();
}

#[tokio::test]
async fn album_tracks_without_network_is_a_404() {
  let core = core_offline();
  let params = query(&[("title", "Discovery"), ("artist", "Daft Punk")]);
  let (status, body) = json_of(discovery::album_tracks(&core, params).await);
  assert_eq!(
    (status, &body["error"]),
    (404, &json!("Failed to load album"))
  );
  assert_eq!(misses().len(), 1);
}

#[tokio::test]
async fn album_tracks_http_500_from_every_provider_is_a_404() {
  let core = core_http_500(&[
    ("GET", "https://itunes.apple.com/lookup?id=42&entity=song"),
    ("GET", "https://api.deezer.com/album/42/tracks?limit=50"),
    (
      "GET",
      "https://itunes.apple.com/search?term=Z%20Y&entity=song&limit=25",
    ),
  ]);
  let params = query(&[("id", "42"), ("title", "Y"), ("artist", "Z")]);
  let (status, _) = json_of(discovery::album_tracks(&core, params).await);
  no_misses();
  assert_eq!(status, 404);
}

// ---------------------------------------------------- map_itunes_song_tracks

#[test]
fn map_itunes_song_tracks_sorts_and_upgrades_covers() {
  let results = [
    json!({"wrapperType":"collection","collectionName":"Album"}),
    json!({"wrapperType":"track","kind":"song","trackId":2,"trackNumber":2,"discNumber":1,"trackName":"B","artistName":"","trackTimeMillis":1500,"artworkUrl100":"https://x/100x100bb.jpg"}),
    json!({"wrapperType":"track","kind":"song","trackId":1,"trackNumber":1,"discNumber":1,"trackName":"A","artistName":"Real","trackTimeMillis":2000}),
  ];
  let tracks = discovery::map_itunes_song_tracks(&results, "Fallback");
  assert_eq!(tracks.len(), 2);
  assert_eq!(tracks[0]["title"], "A");
  assert_eq!(tracks[1]["artist"], "Fallback");
  assert_eq!(tracks[1]["duration"], 2);
  assert_eq!(tracks[1]["thumbnail"], "https://x/600x600bb.jpg");
}

#[test]
fn map_itunes_song_tracks_without_songs_is_empty() {
  assert!(discovery::map_itunes_song_tracks(&[], "x").is_empty());
  let only_album = [json!({"wrapperType":"collection"})];
  assert!(discovery::map_itunes_song_tracks(&only_album, "x").is_empty());
}

// ------------------------------------------------------ resolve_candidate_obj

#[test]
fn resolve_candidate_obj_cleans_and_completes_the_candidate() {
  let cand =
    json!({"artist":"A","title":"T","match":0.9,"source":"deezer","seedWeight":3,"key":"k"});
  let out = discovery::resolve_candidate_obj(&cand).expect("objet");
  assert_eq!(out["searchQuery"], "A - T");
  assert_eq!(out["platform"], "youtube");
  assert!(!out["id"].as_str().unwrap_or("").is_empty());
  for dropped in ["match", "source", "seedWeight", "key"] {
    assert!(out.get(dropped).is_none(), "{dropped} doit disparaître");
  }
}

#[test]
fn resolve_candidate_obj_rejects_non_objects() {
  assert!(discovery::resolve_candidate_obj(&json!("texte")).is_none());
  assert!(discovery::resolve_candidate_obj(&json!([1])).is_none());
}

// -------------------------------------------------------------------- hot_picks

#[tokio::test]
async fn hot_picks_returns_the_recorded_chart() {
  let core = core_recorded();
  let (status, body) = json_of(discovery::hot_picks(&core, query(&[("limit", "5")])).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["success"], true);
  assert!(
    body["title"]
      .as_str()
      .unwrap_or("")
      .starts_with("Hot picks ")
  );
  let tracks = tracks_of(&body);
  assert!(!tracks.is_empty() && tracks.len() <= 12);
  assert!(tracks.iter().any(|t| t["title"] == "Dai Dai"));
  // Le plafond `limit` s'applique à la réponse en cache.
  let (_, cached) = json_of(discovery::hot_picks(&core, query(&[("limit", "2")])).await);
  assert_eq!(tracks_of(&cached).len(), 2);
}

#[tokio::test]
async fn hot_picks_without_network_is_not_a_success() {
  let core = core_offline();
  let (status, body) = json_of(discovery::hot_picks(&core, query(&[])).await);
  assert_eq!((status, body), (200, json!({ "success": false })));
  assert_eq!(misses().len(), 1);
}

#[tokio::test]
async fn hot_picks_http_500_is_not_a_success() {
  let core = core_http_500(&[("GET", "https://api.deezer.com/chart/0/tracks?limit=40")]);
  let (_, body) = json_of(discovery::hot_picks(&core, query(&[])).await);
  no_misses();
  assert_eq!(body, json!({ "success": false }));
}

// ------------------------------------------------ fonctions pures de texte

#[test]
fn normalize_artist_name_strips_labels_and_symbols() {
  assert_eq!(
    discovery::normalize_artist_name("Daft Punk Official"),
    "daft punk"
  );
  assert_eq!(discovery::normalize_artist_name("A.B,C & D"), "a b c & d");
}

#[test]
fn normalize_artist_name_of_nothing_is_empty() {
  assert_eq!(discovery::normalize_artist_name(""), "");
  assert_eq!(discovery::normalize_artist_name(" VEVO !!! "), "");
}

#[test]
fn clean_title_for_matching_drops_decorations() {
  assert_eq!(
    discovery::clean_title_for_matching("One More Time (Official Video)"),
    "one more time"
  );
  assert_eq!(discovery::clean_title_for_matching("A - B, C!"), "a b c");
}

#[test]
fn clean_title_for_matching_of_only_punctuation_is_empty() {
  assert_eq!(discovery::clean_title_for_matching(""), "");
  assert_eq!(discovery::clean_title_for_matching("(...) - !?"), "");
}

#[test]
fn has_cyrillic_detects_the_script() {
  assert!(discovery::has_cyrillic("Привет"));
  assert!(discovery::has_cyrillic("mix Кино"));
}

#[test]
fn has_cyrillic_rejects_latin_and_empty() {
  assert!(!discovery::has_cyrillic("Daft Punk"));
  assert!(!discovery::has_cyrillic(""));
}

#[test]
fn format_thousands_groups_digits() {
  assert_eq!(discovery::format_thousands(0), "0");
  assert_eq!(discovery::format_thousands(999), "999");
  assert_eq!(discovery::format_thousands(1_234_567), "1,234,567");
}

#[test]
fn format_thousands_handles_the_largest_value() {
  assert_eq!(
    discovery::format_thousands(u64::MAX),
    "18,446,744,073,709,551,615"
  );
}

#[test]
fn pick_weighted_seeds_picks_distinct_seeds_up_to_the_count() {
  let seeds: Vec<Value> = (0..6)
    .map(|i| json!({ "title": i, "weight": 1.0 }))
    .collect();
  let picked = discovery::pick_weighted_seeds(&seeds, 4);
  assert_eq!(picked.len(), 4);
  let titles: HashSet<String> = picked.iter().map(|s| s["title"].to_string()).collect();
  assert_eq!(titles.len(), 4, "sans remise");
  assert_eq!(discovery::pick_weighted_seeds(&seeds, 99).len(), 6);
}

#[test]
fn pick_weighted_seeds_with_no_seed_or_count_is_empty() {
  assert!(discovery::pick_weighted_seeds(&[], 4).is_empty());
  assert!(discovery::pick_weighted_seeds(&[json!({"title":"a"})], 0).is_empty());
}

#[test]
fn build_discovery_queries_uses_top_artists() {
  let profile = json!({ "topArtists": ["Daft Punk", {"name": "Justice"}] });
  let queries = discovery::build_discovery_queries(&profile);
  assert_eq!(queries.len(), 2);
  assert!(queries.contains(&"Daft Punk".to_string()));
  assert!(queries.contains(&"Justice".to_string()));
  let cyrillic = json!({ "topArtists": ["Кино"], "dominantScript": "Cyrillic" });
  assert_eq!(
    discovery::build_discovery_queries(&cyrillic),
    vec!["Кино".to_string(), "Кино похожие песни".to_string()]
  );
}

#[test]
fn build_discovery_queries_without_artists_falls_back_to_generic_queries() {
  assert_eq!(
    discovery::build_discovery_queries(&json!({})),
    vec![
      "synthwave chill".to_string(),
      "indie electronic".to_string()
    ]
  );
}

// ------------------------------------------------------------ search_ytm_tracks

#[tokio::test]
async fn search_ytm_tracks_returns_recorded_songs() {
  let core = core_recorded();
  let tracks = discovery::search_ytm_tracks(&core, "Daft Punk One More Time mix", 14).await;
  no_misses();
  assert!(!tracks.is_empty() && tracks.len() <= 14);
  assert!(tracks.iter().all(|t| t["platform"] == "youtube"));
  assert!(
    tracks
      .iter()
      .all(|t| t["url"].as_str().unwrap_or("").contains("watch?v="))
  );
}

#[tokio::test]
async fn search_ytm_tracks_without_network_is_empty() {
  let core = core_offline();
  let tracks = discovery::search_ytm_tracks(&core, "introuvable", 5).await;
  assert!(tracks.is_empty());
  assert!(!misses().is_empty(), "la recherche est bien partie");
}

// ------------------------------------------------------- fetch_itunes_top_songs

#[tokio::test]
async fn fetch_itunes_top_songs_maps_the_recorded_feed() {
  let core = core_recorded();
  let tracks = discovery::fetch_itunes_top_songs(&core.net.http_client, 24).await;
  no_misses();
  assert!(!tracks.is_empty() && tracks.len() <= 24);
  assert_eq!(tracks[0]["title"], "I Don't Trust Tomorrow");
  assert!(tracks[0]["id"].as_str().unwrap_or("").starts_with("it-"));
  assert!(
    tracks[0]["cover"]
      .as_str()
      .unwrap_or("")
      .contains("600x600bb.")
  );
}

#[tokio::test]
async fn fetch_itunes_top_songs_http_500_is_empty() {
  let core = core_http_500(&[(
    "GET",
    "https://itunes.apple.com/us/rss/topsongs/limit=7/json",
  )]);
  let tracks = discovery::fetch_itunes_top_songs(&core.net.http_client, 7).await;
  no_misses();
  assert!(tracks.is_empty());
}

// --------------------------------------------------------------------- trending

#[tokio::test]
async fn trending_returns_the_recorded_top_songs() {
  let core = core_recorded();
  let (status, body) = json_of(discovery::trending(&core).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["success"], true);
  assert_eq!(tracks_of(&body)[0]["title"], "I Don't Trust Tomorrow");
}

#[tokio::test]
async fn trending_without_network_succeeds_with_no_tracks() {
  let core = core_offline();
  let (status, body) = json_of(discovery::trending(&core).await);
  assert_eq!(status, 200);
  assert_eq!(body, json!({ "success": true, "tracks": [] }));
  assert!(!misses().is_empty());
}

#[tokio::test]
async fn trending_falls_back_to_the_deezer_chart_when_itunes_fails() {
  let core = core_http_500(&[(
    "GET",
    "https://itunes.apple.com/us/rss/topsongs/limit=24/json",
  )]);
  let (_, body) = json_of(discovery::trending(&core).await);
  // Le classement Deezer limit=24 n'est pas enregistré : repli suivant (YouTube Music), lui aussi absent.
  assert_eq!(body["tracks"], json!([]));
  assert!(!misses().is_empty());
}

// ------------------------------------------------------------------ dz_artist_id

#[tokio::test]
async fn dz_artist_id_finds_the_recorded_artist() {
  let core = core_recorded();
  assert_eq!(
    discovery::dz_artist_id(&core, "Daft Punk").await.as_deref(),
    Some("27")
  );
  no_misses();
}

#[tokio::test]
async fn dz_artist_id_fails_on_blank_name_and_on_http_500() {
  let core = core_offline();
  assert_eq!(discovery::dz_artist_id(&core, " VEVO ").await, None);
  assert!(misses().is_empty(), "nom vide : aucune requête");
  let core = core_http_500(&[(
    "GET",
    "https://api.deezer.com/search/artist?q=Daft%20Punk&limit=1",
  )]);
  assert_eq!(discovery::dz_artist_id(&core, "Daft Punk").await, None);
  no_misses();
}

// ------------------------------------------------------------ dz_related_tracks

#[tokio::test]
async fn dz_related_tracks_returns_deezer_candidates() {
  let core = core_recorded();
  let tracks = discovery::dz_related_tracks(&core, "Daft Punk").await;
  no_misses();
  assert!(!tracks.is_empty() && tracks.len() <= 24);
  assert!(tracks.iter().all(|t| t["source"] == "deezer"));
}

#[tokio::test]
async fn dz_related_tracks_without_network_is_empty() {
  let core = core_offline();
  assert!(
    discovery::dz_related_tracks(&core, "Daft Punk")
      .await
      .is_empty()
  );
  assert_eq!(misses().len(), 1, "seule la recherche d'artiste est partie");
}

// -------------------------------------------------------------- dz_radio_tracks

#[tokio::test]
async fn dz_radio_tracks_returns_deezer_candidates() {
  let core = core_recorded();
  let tracks = discovery::dz_radio_tracks(&core, "Daft Punk").await;
  no_misses();
  assert!(!tracks.is_empty() && tracks.len() <= 25);
  assert!(tracks.iter().all(|t| t["match"] == 0.7));
}

#[tokio::test]
async fn dz_radio_tracks_http_500_on_the_radio_is_empty() {
  let core = core_hand_written(|dir| {
    write_fixture(
      dir,
      "GET",
      "https://api.deezer.com/search/artist?q=Daft%20Punk&limit=1",
      200,
      r#"{"data":[{"id":27}]}"#,
      "application/json",
    );
    write_fixture(
      dir,
      "GET",
      "https://api.deezer.com/artist/27/radio?limit=40",
      500,
      "Internal Server Error",
      "text/plain",
    );
  });
  assert!(
    discovery::dz_radio_tracks(&core, "Daft Punk")
      .await
      .is_empty()
  );
  no_misses();
}

// ---------------------------------------------------------------- yt_mix_tracks

#[tokio::test]
async fn yt_mix_tracks_returns_recorded_mix_entries() {
  let core = core_recorded();
  let tracks = discovery::yt_mix_tracks(&core, &daft_seed()).await;
  no_misses();
  assert!(!tracks.is_empty());
  assert!(
    tracks
      .iter()
      .all(|t| t["source"] == "ytmix" && t["playable"] == true)
  );
  assert!(tracks.iter().all(|t| t["id"] != "seed-id"));
}

#[tokio::test]
async fn yt_mix_tracks_with_empty_seed_or_without_network_is_empty() {
  let core = core_offline();
  assert!(discovery::yt_mix_tracks(&core, &json!({})).await.is_empty());
  assert!(misses().is_empty());
  assert!(
    discovery::yt_mix_tracks(&core, &daft_seed())
      .await
      .is_empty()
  );
  assert!(!misses().is_empty());
}

// ------------------------------------------------------------- gather_candidates

#[tokio::test]
async fn gather_candidates_merges_the_three_sources() {
  let core = core_recorded();
  let cands = discovery::gather_candidates(
    &core,
    &[json!({"artist":"Daft Punk","title":"One More Time","weight":2.0})],
  )
  .await;
  no_misses();
  assert!(cands.iter().any(|c| c["source"] == "ytmix"));
  assert!(cands.iter().any(|c| c["source"] == "deezer"));
  assert!(cands.iter().all(|c| c["seedWeight"] == 2.0));
}

#[tokio::test]
async fn gather_candidates_without_seeds_or_network_is_empty() {
  let core = core_offline();
  assert!(discovery::gather_candidates(&core, &[]).await.is_empty());
  let seeds = [daft_seed()];
  assert!(discovery::gather_candidates(&core, &seeds).await.is_empty());
  assert!(!misses().is_empty());
}

// -------------------------------------------------------------- rank_candidates

#[tokio::test]
async fn rank_candidates_groups_by_track_and_skips_heard_ones() {
  let core = core_offline();
  let cands = [
    json!({"artist":"Artist A","title":"Song One","match":1.0,"source":"deezer"}),
    json!({"artist":"Artist A","title":"Song One","match":1.0,"source":"deezer"}),
    json!({"artist":"Artist B","title":"Song Two","match":0.5,"source":"deezer"}),
    json!({"artist":"Artist C","title":"Heard","match":1.0,"source":"deezer"}),
  ];
  let heard: HashSet<String> = [riff_core::util::get_track_normalized_key(
    "Artist C", "Heard",
  )]
  .into_iter()
  .collect();
  let none = HashSet::new();
  let ranked = discovery::rank_candidates(&core, &cands, &json!({}), &heard, &none, &none, 4);
  assert_eq!(ranked.len(), 2);
  assert!(ranked.iter().all(|c| c["title"] != "Heard"));
  assert!(
    ranked
      .iter()
      .all(|c| c["rankScore"].is_number() && c["isTop"] == false)
  );
}

#[tokio::test]
async fn rank_candidates_of_nothing_is_empty() {
  let core = core_offline();
  let none = HashSet::new();
  assert!(discovery::rank_candidates(&core, &[], &json!({}), &none, &none, &none, 4).is_empty());
  let avoided: HashSet<String> = ["artist a".to_string()].into_iter().collect();
  let cands = [json!({"artist":"Artist A","title":"Song","match":1.0})];
  assert!(
    discovery::rank_candidates(&core, &cands, &json!({}), &none, &none, &avoided, 4).is_empty()
  );
}

// ---------------------------------------------------- score_discovery_candidate

fn candidate() -> Value {
  json!({
    "id": "abc123", "title": "Song One", "artist": "Artist A", "duration": 200,
    "url": "https://www.youtube.com/watch?v=abc123", "platform": "youtube"
  })
}

#[tokio::test]
async fn score_discovery_candidate_scores_a_playable_track() {
  let core = core_offline();
  let none = HashSet::new();
  let profile =
    json!({ "topArtists": ["artist a"], "dominantScript": "Latin", "typicalDuration": 200 });
  let (score, track) =
    discovery::score_discovery_candidate(&core, &candidate(), &profile, &none, &none)
      .expect("candidat retenu");
  // 1 (base) + 3 (artiste connu) + 1 (script) + 2 (durée) avant l'aléa [0, 1,5).
  assert!((7.0..8.5).contains(&score), "score {score}");
  assert_eq!(track["score"], json!(score));
}

#[tokio::test]
async fn score_discovery_candidate_rejects_heard_or_incomplete_tracks() {
  let core = core_offline();
  let none = HashSet::new();
  let heard_ids: HashSet<String> = ["abc123".to_string()].into_iter().collect();
  assert!(
    discovery::score_discovery_candidate(&core, &candidate(), &json!({}), &none, &heard_ids)
      .is_none()
  );
  let mut no_url = candidate();
  no_url["url"] = json!("");
  assert!(discovery::score_discovery_candidate(&core, &no_url, &json!({}), &none, &none).is_none());
  assert!(
    discovery::score_discovery_candidate(&core, &json!("x"), &json!({}), &none, &none).is_none()
  );
}

// ---------------------------------------------------------- recommend_vibe_tracks

#[tokio::test]
async fn recommend_vibe_tracks_returns_recorded_recommendations() {
  let core = core_recorded();
  let extra = json!({ "seeds": [daft_seed()] });
  let result = discovery::recommend_vibe_tracks(&core, &[], &[], 5, None, &extra).await;
  no_misses();
  let tracks = tracks_of(&result);
  assert!(!tracks.is_empty() && tracks.len() <= 5);
  assert!(
    tracks
      .iter()
      .all(|t| t["id"].as_str().is_some_and(|s| !s.is_empty()))
  );
  assert_eq!(result["profile"]["dominantScript"], "Latin");
}

#[tokio::test]
async fn recommend_vibe_tracks_with_too_little_history_returns_no_tracks() {
  let core = core_offline();
  let history = [
    json!({"id":"a1","title":"T","artist":"A","platform":"youtube","url":"https://www.youtube.com/watch?v=a1"}),
  ];
  let result = discovery::recommend_vibe_tracks(&core, &history, &[], 5, None, &json!({})).await;
  assert_eq!(result["tracks"], json!([]));
  assert_eq!(result["profile"]["trackCount"], 1);
  assert!(
    misses().is_empty(),
    "aucune requête sous le seuil de 5 titres"
  );
}

// ------------------------------------------------------- vibe_recommendations

#[tokio::test]
async fn vibe_recommendations_answers_the_recorded_request() {
  let core = core_recorded();
  let (status, body) = json_of(discovery::vibe_recommendations(&core, body(DAFT_SEED)).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["success"], true);
  assert!(!tracks_of(&body).is_empty());
}

#[tokio::test]
async fn vibe_recommendations_rejects_an_oversized_body() {
  let core = core_offline();
  let huge = "x".repeat(riff_core::core::MAX_BODY_BYTES + 1);
  let (status, body) = json_of(discovery::vibe_recommendations(&core, body(&huge)).await);
  assert_eq!(status, 413);
  assert_eq!(body["error"], "payload too large");
}

#[tokio::test]
async fn vibe_recommendations_without_network_has_no_tracks() {
  let core = core_offline();
  let (status, body) = json_of(discovery::vibe_recommendations(&core, body(DAFT_SEED)).await);
  assert_eq!(status, 200);
  assert_eq!(tracks_of(&body).len(), 0);
  assert!(!misses().is_empty());
}

// ----------------------------------------------------------------- fetch_music_bio

#[tokio::test]
async fn fetch_music_bio_returns_the_recorded_lastfm_summary() {
  let core = core_recorded();
  let bio = discovery::fetch_music_bio(&core.net.http_client, "Daft Punk").await;
  no_misses();
  assert!(bio.chars().count() > 20);
  assert!(!bio.contains('<') && !bio.contains("Read more on Last.fm"));
}

#[tokio::test]
async fn fetch_music_bio_of_blank_name_or_without_network_is_empty() {
  let core = core_offline();
  assert_eq!(
    discovery::fetch_music_bio(&core.net.http_client, "  ").await,
    ""
  );
  assert!(misses().is_empty());
  assert_eq!(
    discovery::fetch_music_bio(&core.net.http_client, "Daft Punk").await,
    ""
  );
  assert!(!misses().is_empty());
}

// --------------------------------------------------------------- fetch_artist_info

#[tokio::test]
async fn fetch_artist_info_builds_the_recorded_profile() {
  let core = core_recorded();
  let info = discovery::fetch_artist_info(&core, "Daft Punk", "").await;
  no_misses();
  assert_eq!(info["name"], "Daft Punk");
  assert_eq!(info["isKnownArtist"], true);
  assert!(info["fans"].as_u64().unwrap_or(0) > 0);
  assert!(!info["tracks"].as_array().expect("tracks").is_empty());
  assert!(!info["albums"].as_array().expect("albums").is_empty());
  assert!(!info["bio"].as_str().unwrap_or("").is_empty());
}

#[tokio::test]
async fn fetch_artist_info_blank_name_is_an_empty_profile() {
  let core = core_offline();
  let info = discovery::fetch_artist_info(&core, " - Topic", "").await;
  assert_eq!(info["name"], "");
  assert_eq!(info["isKnownArtist"], false);
  assert_eq!(info["tracks"], json!([]));
  assert!(misses().is_empty());
}

#[tokio::test]
async fn fetch_artist_info_http_500_leaves_an_unknown_artist() {
  let core = core_http_500(&[(
    "GET",
    "https://api.deezer.com/search/artist?q=Daft%20Punk&limit=10",
  )]);
  let info = discovery::fetch_artist_info(&core, "Daft Punk", "").await;
  assert_eq!(info["name"], "Daft Punk");
  assert_eq!(info["albums"], json!([]));
  assert_eq!(info["fans"], 0);
  assert_eq!(info["monthlyListeners"], "");
  assert!(
    !misses().is_empty(),
    "les replis ont été tentés sans fixture"
  );
}

// ---------------------------------------------------------------------- artist

#[tokio::test]
async fn artist_returns_the_recorded_profile() {
  let core = core_recorded();
  let (status, body) = json_of(discovery::artist(&core, query(&[("name", "Daft Punk")])).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body["artist"]["name"], "Daft Punk");
}

#[tokio::test]
async fn artist_without_a_name_is_a_400() {
  let core = core_offline();
  let (status, body) = json_of(discovery::artist(&core, query(&[])).await);
  assert_eq!(
    (status, &body["error"]),
    (400, &json!("Missing artist name"))
  );
}
