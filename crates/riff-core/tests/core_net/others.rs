//! Fonctions publiques de `services` sans réseau enregistré : `youtube`, `stream`, `tools`, `download`, `spotify`,
//! `google`, `imports`, `ytdlp`. Aucun processus externe n'est lancé (`RIFF_YTDLP` et `RIFF_FFMPEG` pointent vers des
//! fichiers de 4 octets non exécutables). Ce qui exige `yt-dlp`, `ffmpeg` ou une page réelle n'est pas couvert ici.

use crate::common::{
  body, core_broken_vault, core_hand_written, core_offline, json_of, misses, query, root, text_of,
  write_fixture, write_fixture_with_body,
};
use riff_core::reply::{Method, Params, Reply};
use riff_core::services::{download, google, imports, spotify, stream, tools, youtube, ytdlp};
use serde_json::json;
use std::time::Duration;

// ------------------------------------------------------------------- youtube

#[test]
fn extract_yt_id_from_any_reads_ids_and_urls() {
  let id = "dQw4w9WgXcQ";
  assert_eq!(youtube::extract_yt_id_from_any(id), id);
  assert_eq!(
    youtube::extract_yt_id_from_any(&format!("https://www.youtube.com/watch?v={id}&t=3")),
    id
  );
  assert_eq!(
    youtube::extract_yt_id_from_any(&format!("https://youtu.be/{id}")),
    id
  );
}

#[test]
fn extract_yt_id_from_any_refuses_foreign_ids() {
  for input in [
    "",
    "  ",
    "dz_123456789ab",
    "local_abcdefghij",
    "not an id",
    "https://example.com/x",
  ] {
    assert_eq!(youtube::extract_yt_id_from_any(input), "", "{input:?}");
  }
}

#[tokio::test]
async fn fetch_direct_yt_stream_with_an_invalid_id_makes_no_request() {
  let core = core_offline();
  assert_eq!(
    youtube::fetch_direct_yt_stream(&core, "dz_123", "medium").await,
    None
  );
  assert!(misses().is_empty());
}

#[tokio::test]
async fn fetch_direct_yt_stream_without_network_is_none() {
  let core = core_offline();
  assert_eq!(
    youtube::fetch_direct_yt_stream(&core, "dQw4w9WgXcQ", "medium").await,
    None
  );
  assert!(!misses().is_empty(), "la résolution a bien tenté le réseau");
}

#[tokio::test]
async fn verify_stream_playable_accepts_hls_playlists_without_a_request() {
  let core = core_offline();
  assert!(youtube::verify_stream_playable(&core, "https://x.example/master.m3u8").await);
  assert!(misses().is_empty());
}

#[tokio::test]
async fn verify_stream_playable_rejects_invalid_and_unreachable_urls() {
  let core = core_offline();
  assert!(!youtube::verify_stream_playable(&core, "pas une url").await);
  assert!(misses().is_empty());
  assert!(
    !youtube::verify_stream_playable(
      &core,
      "https://rr1---sn-fake.googlevideo.com/videoplayback?c=IOS"
    )
    .await
  );
  assert!(!misses().is_empty());
}

// Fixtures écrites à la main pour `fetch_direct_yt_stream`. Le rejeu ignore les en-têtes (dont `Range`) : une URL
// googlevideo a donc une seule réponse, quelle que soit la plage. Les corps POST doivent être ceux du code, octet pour octet.
const UA_VISIONOS: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const INNERTUBE: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";
const INNERTUBE_MUSIC: &str = "https://music.youtube.com/youtubei/v1/player?prettyPrint=false";

fn innertube_body(vid: &str, name: &str, version: &str, context: serde_json::Value) -> Vec<u8> {
  let mut client = json!({ "clientName": name, "clientVersion": version, "hl": "en", "gl": "US" });
  for (k, v) in context.as_object().expect("objet") {
    client[k] = v.clone();
  }
  let body = json!({
    "videoId": vid,
    "context": { "client": client },
    "contentCheckOk": true,
    "racyCheckOk": true
  });
  serde_json::to_vec(&body).expect("JSON")
}

fn visionos_body(vid: &str) -> Vec<u8> {
  innertube_body(
    vid,
    "VISIONOS",
    "1.02",
    json!({
      "deviceMake": "Apple",
      "deviceModel": "RealityDevice17,1",
      "userAgent": UA_VISIONOS,
      "osName": "visionOS",
      "osVersion": "26.5.23O471",
      "timeZone": "UTC",
      "utcOffsetMinutes": 0
    }),
  )
}

fn ios_body(vid: &str) -> Vec<u8> {
  innertube_body(
    vid,
    "IOS",
    "20.10.4",
    json!({
      "deviceMake": "Apple",
      "deviceModel": "iPhone16,2",
      "osName": "iOS",
      "osVersion": "18.3.2.22D60"
    }),
  )
}

fn android_music_body(vid: &str) -> Vec<u8> {
  innertube_body(
    vid,
    "ANDROID_MUSIC",
    "7.27.52",
    json!({ "androidSdkVersion": 34 }),
  )
}

fn player_json(url: &str) -> String {
  json!({
    "playabilityStatus": { "status": "OK" },
    "streamingData": { "adaptiveFormats": [
      { "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000, "url": url }
    ] }
  })
  .to_string()
}

fn gv(client: &str, id: &str) -> String {
  format!(
    "https://rr1---sn-test.googlevideo.com/videoplayback?c={client}&itag=140&clen=5000000&id={id}"
  )
}

fn write_post(dir: &std::path::Path, url: &str, body: &[u8], status: u16, text: &str) {
  write_fixture_with_body(dir, "POST", url, body, status, text, "application/json");
}

fn write_home(dir: &std::path::Path) {
  write_fixture(
    dir,
    "GET",
    "https://www.youtube.com/",
    200,
    r#"<html>"visitorData":"VD-TEST"</html>"#,
    "text/html",
  );
}

#[tokio::test]
async fn fetch_direct_yt_stream_returns_the_visionos_url_when_the_probe_passes() {
  let vid = "vis0000test";
  let ok = gv("VISIONOS", "a");
  let core = core_hand_written(|dir| {
    write_home(dir);
    write_post(dir, INNERTUBE, &visionos_body(vid), 200, &player_json(&ok));
    write_fixture(dir, "GET", &ok, 206, "xx", "audio/mp4");
  });
  assert_eq!(
    youtube::fetch_direct_yt_stream(&core, vid, "medium").await,
    Some(ok)
  );
}

#[tokio::test]
async fn fetch_direct_yt_stream_rejects_an_ios_url_that_answers_403_to_the_open_range() {
  let vid = "ios00000tes";
  let bad = gv("IOS", "b");
  let core = core_hand_written(|dir| {
    write_home(dir);
    // VISIONOS échoue (HTTP 500), IOS rend une URL refusée, ANDROID_MUSIC n'a pas de réponse.
    write_post(dir, INNERTUBE, &visionos_body(vid), 500, "erreur");
    write_post(dir, INNERTUBE, &ios_body(vid), 200, &player_json(&bad));
    write_fixture(dir, "GET", &bad, 403, "denied", "text/plain");
  });
  assert_eq!(
    youtube::fetch_direct_yt_stream(&core, vid, "medium").await,
    None
  );
}

#[tokio::test]
async fn fetch_direct_yt_stream_rejects_a_visionos_url_refused_by_the_probe() {
  let vid = "vis403test";
  let bad = gv("VISIONOS", "c");
  let core = core_hand_written(|dir| {
    write_home(dir);
    write_post(dir, INNERTUBE, &visionos_body(vid), 200, &player_json(&bad));
    write_fixture(dir, "GET", &bad, 403, "denied", "text/plain");
  });
  assert_eq!(
    youtube::fetch_direct_yt_stream(&core, vid, "medium").await,
    None
  );
}

#[tokio::test]
async fn fetch_direct_yt_stream_falls_back_to_the_first_client_whose_url_passes_the_probe() {
  let vid = "fall0000tes";
  let bad = gv("IOS", "d");
  let good = gv("ANDROID_MUSIC", "e");
  let core = core_hand_written(|dir| {
    write_home(dir);
    write_post(dir, INNERTUBE, &visionos_body(vid), 500, "erreur");
    write_post(dir, INNERTUBE, &ios_body(vid), 200, &player_json(&bad));
    write_post(
      dir,
      INNERTUBE_MUSIC,
      &android_music_body(vid),
      200,
      &player_json(&good),
    );
    write_fixture(dir, "GET", &bad, 403, "denied", "text/plain");
    write_fixture(dir, "GET", &good, 206, "xx", "audio/mp4");
  });
  assert_eq!(
    youtube::fetch_direct_yt_stream(&core, vid, "medium").await,
    Some(good)
  );
}

#[tokio::test]
async fn accept_stream_url_follows_redirects_and_judges_the_final_status() {
  let first = gv("TVHTML5_SIMPLY", "f");
  let target = "https://rr2---sn-test.googlevideo.com/videoplayback?c=TVHTML5_SIMPLY&id=g";
  let core = core_hand_written(|dir| {
    // Une fixture 3xx est servie telle quelle (voir `replay.rs`).
    let parsed = url::Url::parse(&first).expect("URL");
    let path = riff_core::net::replay::fixture_path(dir, "GET", &parsed, b"");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dossier");
    let fixture = riff_core::net::replay::Fixture {
      recorded_at: "écrit à la main".to_string(),
      method: "GET".to_string(),
      url: first.clone(),
      body_sha256: String::new(),
      status: 302,
      headers: std::collections::BTreeMap::from([("location".to_string(), target.to_string())]),
      final_url: None,
      body: Some(String::new()),
      body_base64: None,
    };
    std::fs::write(&path, serde_json::to_vec(&fixture).expect("JSON")).expect("fixture");
    write_fixture(dir, "GET", target, 206, "xx", "audio/mp4");
  });
  assert!(youtube::accept_stream_url(&core, &first, "test").await);
}

#[tokio::test]
async fn accept_stream_url_refuses_403_unreachable_and_invalid_urls() {
  let refused = gv("IOS", "h");
  let core = core_hand_written(|dir| {
    write_fixture(dir, "GET", &refused, 403, "denied", "text/plain");
  });
  assert!(!youtube::accept_stream_url(&core, &refused, "test").await);
  assert!(!youtube::accept_stream_url(&core, &gv("IOS", "absente"), "test").await);
  assert!(!youtube::accept_stream_url(&core, "pas une url", "test").await);
}

// ------------------------------------------- stream_info : `fresh` et `forceYtDlp`

fn relaunch_params(vid: &str, extra: Option<(&str, &str)>) -> Params {
  let mut pairs = vec![
    ("url", "https://www.youtube.com/watch?v=x"),
    ("id", vid),
    ("platform", "youtube"),
    ("title", "Fake Title"),
    ("artist", "Fake Artist"),
    ("timeout", "5"),
  ];
  pairs.extend(extra);
  query(&pairs)
}

/// Réécrit la réponse innertube VISIONOS du dossier : l'URL rendue change, comme après un réveil de veille.
fn write_visionos(dir: &std::path::Path, vid: &str, url: &str) {
  write_home(dir);
  write_post(dir, INNERTUBE, &visionos_body(vid), 200, &player_json(url));
  write_fixture(dir, "GET", url, 206, "xx", "audio/mp4");
}

#[tokio::test]
async fn stream_info_fresh_ignores_the_cache_and_returns_the_new_innertube_url() {
  let vid = "fresh00test";
  let (old, new) = (gv("VISIONOS", "vieille"), gv("VISIONOS", "neuve"));
  let mut fixtures = None;
  let core = core_hand_written(|dir| {
    write_visionos(dir, vid, &old);
    fixtures = Some(dir.to_path_buf());
  });
  let dir = fixtures.expect("dossier");
  let (status, out) = json_of(stream::stream_info(&core, relaunch_params(vid, None)).await);
  assert_eq!((status, out["rawStreamUrl"].as_str()), (200, Some(&*old)));
  // L'innertube rend maintenant une autre URL ; l'entrée de cache garde la vieille.
  write_visionos(&dir, vid, &new);
  let (_, cached) = json_of(stream::stream_info(&core, relaunch_params(vid, None)).await);
  assert_eq!(cached["rawStreamUrl"].as_str(), Some(&*old));
  let fresh = relaunch_params(vid, Some(("fresh", "1")));
  let (status, out) = json_of(stream::stream_info(&core, fresh).await);
  assert_eq!((status, out["rawStreamUrl"].as_str()), (200, Some(&*new)));
  // L'entrée de cache est remplacée par la nouvelle URL.
  let (_, again) = json_of(stream::stream_info(&core, relaunch_params(vid, None)).await);
  assert_eq!(again["rawStreamUrl"].as_str(), Some(&*new));
}

#[tokio::test]
async fn stream_info_force_yt_dlp_skips_the_direct_innertube_resolution() {
  let vid = "force00test";
  let url = gv("VISIONOS", "directe");
  let core = core_hand_written(|dir| write_visionos(dir, vid, &url));
  // Sans le paramètre, l'innertube rend l'URL (fixture présente).
  let (status, out) = json_of(stream::stream_info(&core, relaunch_params(vid, None)).await);
  assert_eq!((status, out["rawStreamUrl"].as_str()), (200, Some(&*url)));
  // Avec `forceYtDlp=1`, la résolution directe est sautée malgré la fixture et le cache vidé : le faux `yt-dlp`
  // n'exécute rien, la résolution échoue.
  let forced = relaunch_params(vid, Some(("forceYtDlp", "1")));
  let (status, out) = json_of(stream::stream_info(&core, forced).await);
  assert_eq!(status, 500, "{out}");
  assert_eq!(out["success"], false);
}

#[tokio::test]
async fn stream_info_after_a_failed_fresh_attempt_still_reaches_the_yt_dlp_path() {
  let vid = "twice00test";
  let url = gv("VISIONOS", "morte");
  let core = core_hand_written(|dir| write_visionos(dir, vid, &url));
  let fresh = relaunch_params(vid, Some(("fresh", "1")));
  let (status, out) = json_of(stream::stream_info(&core, fresh).await);
  assert_eq!((status, out["rawStreamUrl"].as_str()), (200, Some(&*url)));
  // La seconde relance écarte l URL directe malgré la fixture : le chemin yt-dlp (faux exécutable ici) est le seul restant.
  let forced = relaunch_params(vid, Some(("forceYtDlp", "1")));
  let (status, _) = json_of(stream::stream_info(&core, forced).await);
  assert_eq!(status, 500);
}

// -------------------------------------------------------------------- stream

#[test]
fn sanitize_filename_strips_reserved_characters() {
  assert_eq!(
    stream::sanitize_filename("AC/DC: Back  In <Black>?"),
    "ACDC Back In Black"
  );
}

#[test]
fn sanitize_filename_of_nothing_is_a_default_name() {
  assert_eq!(stream::sanitize_filename(""), "track");
  assert_eq!(stream::sanitize_filename("???"), "");
}

#[test]
fn invalidate_downloads_index_is_idempotent() {
  stream::invalidate_downloads_index();
  stream::invalidate_downloads_index();
}

#[tokio::test]
async fn prefetch_acknowledges_the_request() {
  let core = core_offline();
  // Sans URL ni identifiant, la tâche lancée retourne aussitôt : aucune requête.
  let (status, out) = json_of(stream::prefetch(&core, query(&[])).await);
  assert_eq!((status, out), (200, json!({"success": true})));
  tokio::task::yield_now().await;
  assert!(misses().is_empty());
}

#[tokio::test]
async fn prefetch_track_without_a_key_returns_without_a_request() {
  let core = core_offline();
  stream::prefetch_track(&core, "", "", "youtube").await;
  assert!(misses().is_empty());
  assert!(core.playback.prefetching.lock().expect("verrou").is_empty());
}

#[tokio::test]
async fn stream_info_of_an_unresolvable_track_is_an_error_status() {
  let core = core_offline();
  let params = query(&[
    ("url", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
    ("id", "dQw4w9WgXcQ"),
    ("timeout", "5"),
  ]);
  let (status, out) = json_of(stream::stream_info(&core, params).await);
  assert!(status >= 400, "statut {status} : {out}");
  assert_eq!(out["success"], false);
}

// --------------------------------------------------------------------- tools

#[tokio::test]
async fn tools_status_reports_the_installed_tools() {
  let core = core_offline();
  let (status, out) = json_of(tools::tools_status(&core).await);
  assert_eq!(status, 200);
  // Les faux outils de `root()` sont résolus par RIFF_YTDLP et RIFF_FFMPEG.
  assert_eq!(out["ytDlp"], true);
  assert_eq!(out["ffmpeg"], true);
  assert_eq!(out["percent"], 0);
}

#[tokio::test]
async fn proxy_test_refuses_other_methods_and_oversized_bodies() {
  let core = core_offline();
  let (status, text) = text_of(tools::proxy_test(&core, Method::Get, Params::default()).await);
  assert_eq!((status, text.as_str()), (404, "Not found"));
  let huge = "x".repeat(riff_core::core::MAX_BODY_BYTES + 1);
  let (status, out) = json_of(tools::proxy_test(&core, Method::Post, body(&huge)).await);
  assert_eq!(
    (status, out["error"].as_str()),
    (413, Some("payload too large"))
  );
}

#[tokio::test]
async fn check_proxy_alive_sees_a_local_listener() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("port local");
  let port = listener.local_addr().expect("adresse").port();
  assert!(tools::check_proxy_alive(&format!("127.0.0.1:{port}"), Duration::from_secs(2)).await);
  assert!(
    tools::check_proxy_alive(
      &format!("socks5://127.0.0.1:{port}"),
      Duration::from_secs(2)
    )
    .await
  );
}

#[tokio::test]
async fn check_proxy_alive_rejects_empty_portless_and_closed_targets() {
  for target in [
    "",
    "socks5://127.0.0.1",
    "http://127.0.0.1:0",
    "not a proxy",
  ] {
    assert!(
      !tools::check_proxy_alive(target, Duration::from_millis(200)).await,
      "{target:?}"
    );
  }
  let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("port local");
  let port = closed.local_addr().expect("adresse").port();
  drop(closed);
  assert!(
    !tools::check_proxy_alive(&format!("127.0.0.1:{port}"), Duration::from_millis(500)).await
  );
}

#[tokio::test]
async fn proxy_config_disabled_clears_the_active_proxy() {
  let core = core_offline();
  let (status, out) = json_of(tools::proxy_config(&core, body(r#"{"enabled":false}"#)).await);
  assert_eq!(status, 200);
  assert_eq!(out["success"], true);
  assert_eq!(*core.net.server_active_proxy.lock().expect("verrou"), "");
}

#[tokio::test]
async fn proxy_config_rejects_an_oversized_body() {
  let core = core_offline();
  let huge = "x".repeat(riff_core::core::MAX_BODY_BYTES + 1);
  let (status, _) = json_of(tools::proxy_config(&core, body(&huge)).await);
  assert_eq!(status, 413);
}

// ------------------------------------------------------------------ download

#[test]
fn is_valid_tmp_wav_name_accepts_plain_wav_names() {
  assert!(download::is_valid_tmp_wav_name("preview_1.wav"));
  assert!(download::is_valid_tmp_wav_name("my track-2.v1.wav"));
}

#[test]
fn is_valid_tmp_wav_name_rejects_traversal_and_other_extensions() {
  for name in [
    "", ".wav", "a.mp3", "../a.wav", "a/b.wav", r"a\b.wav", "a..b.wav", "é.wav",
  ] {
    assert!(!download::is_valid_tmp_wav_name(name), "{name:?}");
  }
}

#[tokio::test]
async fn tmp_file_serves_a_preview_from_the_downloads_tmp_folder() {
  root();
  let dir = riff_core::config::resolve_download_dir()
    .expect("dossier configuré")
    .join(".tmp");
  std::fs::create_dir_all(&dir).expect("dossier");
  std::fs::write(dir.join("pb_preview.wav"), [1u8, 2, 3]).expect("fichier");
  match download::tmp_file(query(&[("name", "pb_preview.wav")])).await {
    Ok(Reply::Bytes {
      status: 200, body, ..
    }) => assert_eq!(body.as_ref(), [1u8, 2, 3]),
    other => panic!("octets attendus : {other:?}"),
  }
}

#[tokio::test]
async fn tmp_file_refuses_bad_names_and_missing_files() {
  let (status, out) = json_of(download::tmp_file(query(&[("name", "../x.wav")])).await);
  assert_eq!(
    (status, out["error"].as_str()),
    (400, Some("Invalid file name"))
  );
  let (status, out) = json_of(download::tmp_file(query(&[("name", "pb_absent.wav")])).await);
  assert_eq!(
    (status, out["error"].as_str()),
    (404, Some("File not found"))
  );
}

// ------------------------------------------------------------------- spotify

#[tokio::test]
async fn spotify_auth_start_builds_a_pkce_authorization_url() {
  let core = core_offline();
  let client_id = "0123456789abcdef0123456789abcdef";
  let (status, out) = json_of(spotify::auth_start(&core, query(&[("clientId", client_id)])).await);
  assert_eq!(status, 200);
  assert_eq!(out["success"], true);
  let url = out["url"].as_str().expect("url");
  assert!(url.starts_with("https://accounts.spotify.com/authorize"));
  assert!(url.contains(client_id) && url.contains("code_challenge="));
  assert_eq!(
    out["redirectUri"],
    "http://127.0.0.1:1234/api/spotify-callback"
  );
}

#[tokio::test]
async fn spotify_auth_start_refuses_an_invalid_client_id() {
  let core = core_offline();
  for id in ["", "short", "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"] {
    let (status, out) = json_of(spotify::auth_start(&core, query(&[("clientId", id)])).await);
    assert_eq!(
      (status, out["error"].as_str()),
      (400, Some("Invalid Client ID")),
      "{id:?}"
    );
  }
}

#[tokio::test]
async fn spotify_callback_refuses_a_session_it_did_not_start() {
  let core = core_offline();
  let reply = spotify::callback(&core, query(&[("code", "c"), ("state", "unknown")])).await;
  assert_eq!(reply.expect("réponse").status(), 400);
  assert!(
    misses().is_empty(),
    "aucun échange de code sans état valide"
  );
}

#[tokio::test]
async fn spotify_callback_with_a_matching_state_but_denied_access_is_an_error_page() {
  let core = core_offline();
  let client_id = "0123456789abcdef0123456789abcdef";
  let started = json_of(spotify::auth_start(&core, query(&[("clientId", client_id)])).await).1;
  let url = url::Url::parse(started["url"].as_str().expect("url")).expect("URL");
  let state = url
    .query_pairs()
    .find(|(k, _)| k == "state")
    .expect("state")
    .1
    .into_owned();
  let reply = spotify::callback(
    &core,
    query(&[("state", &state), ("error", "access_denied")]),
  )
  .await;
  assert_eq!(reply.expect("réponse").status(), 400);
  let (_, status_body) = json_of(spotify::status(&core).await);
  assert_eq!(status_body["connected"], false);
  assert_eq!(status_body["error"], "Spotify authorization was denied.");
}

#[tokio::test]
async fn spotify_status_and_logout_on_a_fresh_vault() {
  let core = core_offline();
  let (status, out) = json_of(spotify::status(&core).await);
  assert_eq!(
    (status, out),
    (200, json!({"connected": false, "error": ""}))
  );
  let (status, out) = json_of(spotify::logout(&core).await);
  assert_eq!((status, out), (200, json!({"success": true})));
}

#[tokio::test]
async fn spotify_logout_reports_a_vault_failure() {
  let core = core_broken_vault();
  let (status, out) = json_of(spotify::logout(&core).await);
  assert_eq!(status, 500);
  assert_eq!(out["error"], "Could not remove saved Spotify login");
}

#[tokio::test]
async fn spotify_load_auth_reads_nothing_from_an_empty_vault() {
  let core = core_offline();
  {
    let mut shared = spotify::lock(&core);
    spotify::load_auth(&core, &mut shared);
  }
  let (_, out) = json_of(spotify::status(&core).await);
  assert_eq!(out, json!({"connected": false, "error": ""}));
}

#[tokio::test]
async fn spotify_load_auth_flags_an_unavailable_vault() {
  let core = core_broken_vault();
  {
    let mut shared = spotify::lock(&core);
    spotify::load_auth(&core, &mut shared);
  }
  let (_, out) = json_of(spotify::status(&core).await);
  assert_eq!(out["connected"], false);
  assert_eq!(
    out["error"],
    "Secure storage is unavailable. Spotify login could not be loaded."
  );
}

#[tokio::test]
async fn spotify_auth_start_clears_the_unavailable_vault_error() {
  let core = core_broken_vault();
  {
    let mut shared = spotify::lock(&core);
    spotify::load_auth(&core, &mut shared);
  }
  let (status, out) = json_of(
    spotify::auth_start(
      &core,
      query(&[("clientId", "0123456789abcdef0123456789abcdef")]),
    )
    .await,
  );
  assert_eq!((status, out["success"].as_bool()), (200, Some(true)));
  let (_, out) = json_of(spotify::status(&core).await);
  assert_eq!(out["error"], "");
}

#[tokio::test]
async fn spotify_library_without_a_login_is_a_401() {
  let core = core_offline();
  let (status, out) = json_of(spotify::library(&core, query(&[])).await);
  assert_eq!((status, out["error"].as_str()), (401, Some("Unauthorized")));
}

#[tokio::test]
async fn spotify_get_with_retry_without_a_login_fails_before_any_request() {
  let core = core_offline();
  let result = spotify::get_with_retry(&core, "https://api.spotify.com/v1/me").await;
  assert!(result.is_err());
  assert!(misses().is_empty());
}

// -------------------------------------------------------------------- google

#[tokio::test]
async fn google_status_and_logout_on_a_fresh_vault() {
  let core = core_offline();
  let (status, out) = json_of(google::status(&core).await);
  assert_eq!(status, 200);
  assert_eq!(out["connected"], false);
  let (status, out) = json_of(google::logout(&core).await);
  assert_eq!((status, out), (200, json!({"success": true})));
}

#[tokio::test]
async fn google_logout_reports_a_vault_failure() {
  let core = core_broken_vault();
  let (status, out) = json_of(google::logout(&core).await);
  assert_eq!(status, 500);
  assert_eq!(out["error"], "Could not remove saved Google login");
}

#[tokio::test]
async fn google_library_flags_an_unavailable_vault() {
  let core = core_broken_vault();
  let (status, out) = json_of(google::library(&core, query(&[])).await);
  assert_eq!(status, 500);
  assert_eq!(out["success"], false);
  assert_eq!(
    out["error"],
    "Secure storage is unavailable. Google login could not be loaded."
  );
}

#[tokio::test]
async fn google_status_of_an_unconfigured_build_is_unchanged_with_a_failing_vault() {
  let core = core_broken_vault();
  let (status, out) = json_of(google::status(&core).await);
  assert_eq!(status, 200);
  assert_eq!(out, json!({"enabled": false, "connected": false}));
}

#[tokio::test]
async fn google_unreadable_saved_login_keeps_the_sign_in_message() {
  use riff_core::secrets::{MemoryStore, SecretName, SecretStore};
  use std::sync::Arc;
  let store = Arc::new(MemoryStore::default());
  store
    .set(SecretName::GoogleAuth, "{pas du json")
    .expect("coffre mémoire");
  let core = crate::common::core_offline_with_store(store);
  let (status, out) = json_of(google::library(&core, query(&[])).await);
  assert_eq!(status, 500);
  assert_eq!(
    out["error"],
    "Sign in to Google before importing your YouTube library"
  );
}

#[tokio::test]
async fn google_library_without_a_login_is_an_error_status() {
  let core = core_offline();
  let (status, out) = json_of(google::library(&core, query(&[])).await);
  assert!(status >= 400, "statut {status} : {out}");
  assert_eq!(out["success"], false);
  assert!(misses().is_empty());
}

// ------------------------------------------------------------------- imports

#[tokio::test]
async fn spotify_import_refuses_a_link_that_is_not_a_playlist() {
  let core = core_offline();
  let (status, out) =
    json_of(imports::spotify_import(&core, query(&[("url", "https://example.com/x")])).await);
  assert_eq!(status, 400);
  assert_eq!(out["code"], "spotify_error");
  assert!(misses().is_empty());
}

#[tokio::test]
async fn spotify_import_maps_a_hand_written_404_to_not_found() {
  let core = core_hand_written(|dir| {
    write_fixture(
      dir,
      "GET",
      "https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M",
      404,
      "Not Found",
      "text/html",
    );
  });
  let params = query(&[(
    "url",
    "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
  )]);
  let (status, out) = json_of(imports::spotify_import(&core, params).await);
  assert_eq!(
    (status, out["code"].as_str()),
    (404, Some("spotify_not_found"))
  );
}

#[tokio::test]
async fn spotify_import_without_network_is_a_500() {
  let core = core_offline();
  let params = query(&[(
    "url",
    "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
  )]);
  let (status, out) = json_of(imports::spotify_import(&core, params).await);
  assert_eq!(
    (status, out["error"].as_str()),
    (500, Some("Network error connecting to Spotify"))
  );
  assert!(!misses().is_empty());
}

#[tokio::test]
async fn apple_import_refuses_a_link_that_is_not_a_playlist() {
  let core = core_offline();
  let (status, out) =
    json_of(imports::apple_import(&core, query(&[("url", "https://example.com/x")])).await);
  assert_eq!(
    (status, out["error"].as_str()),
    (400, Some("Invalid Apple Music playlist URL"))
  );
}

#[tokio::test]
async fn apple_import_without_network_is_an_error() {
  let core = core_offline();
  let params = query(&[(
    "url",
    "https://music.apple.com/us/playlist/fake/pl.0123456789abcdef0123456789abcdef",
  )]);
  let (status, out) = json_of(imports::apple_import(&core, params).await);
  assert!(status >= 400, "statut {status} : {out}");
  assert_eq!(out["success"], false);
  assert!(!misses().is_empty());
}

#[tokio::test]
async fn yandex_import_refuses_an_unsupported_link() {
  let core = core_offline();
  let (status, out) =
    json_of(imports::yandex_import(&core, query(&[("url", "https://example.com/x")])).await);
  assert_eq!(
    (status, out["error"].as_str()),
    (400, Some("unsupported link"))
  );
}

#[tokio::test]
async fn yandex_import_without_network_is_an_error() {
  let core = core_offline();
  let params = query(&[(
    "url",
    "https://music.yandex.com/users/someone/playlists/1000",
  )]);
  let (status, out) = json_of(imports::yandex_import(&core, params).await);
  assert!(status >= 400, "statut {status} : {out}");
  assert_eq!(out["success"], false);
  assert!(!misses().is_empty());
}

// --------------------------------------------------------------------- ytdlp

/// Cycle de vie du fichier de cookies (état disque partagé : un seul test, étapes dans l'ordre).
#[test]
fn cookies_file_lifecycle() {
  let core_root = root();
  let path = ytdlp::yt_cookies_path();
  assert_eq!(
    path.file_name().and_then(|n| n.to_str()),
    Some("yt-cookies.txt")
  );
  // Erreur : pas de fichier.
  let _ = std::fs::remove_file(&path);
  assert!(!ytdlp::has_yt_login_cookies());
  assert!(ytdlp::yt_auth_headers("https://www.youtube.com").is_none());
  assert!(
    ytdlp::delete_cookies(),
    "supprimer un fichier absent n'est pas une erreur"
  );
  // Succès : un fichier avec une session valide.
  let jar = core_root.join("jar.txt");
  std::fs::write(
    &jar,
    "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tLOGIN_INFO\tabc\n.youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tsapval\n",
  )
  .expect("jar");
  ytdlp::import_cookies(&jar).expect("import");
  assert!(ytdlp::has_yt_login_cookies());
  let headers = ytdlp::yt_auth_headers("https://www.youtube.com").expect("en-têtes");
  assert!(
    headers
      .iter()
      .any(|(k, v)| *k == "Authorization" && v.starts_with("SAPISIDHASH "))
  );
  assert!(ytdlp::delete_cookies());
  assert!(!ytdlp::has_yt_login_cookies());
}

#[tokio::test]
async fn execute_yt_dlp_reports_a_binary_that_cannot_run() {
  let core = core_offline();
  let result = ytdlp::execute_yt_dlp(&core, &["--version".to_string()], 5_000, true).await;
  assert!(
    result.is_err(),
    "le faux yt-dlp de 4 octets ne s'exécute pas : {result:?}"
  );
  assert!(misses().is_empty());
}

#[tokio::test]
async fn spotify_load_auth_flags_an_unreadable_saved_login() {
  use riff_core::secrets::{MemoryStore, SecretName, SecretStore};
  use std::sync::Arc;
  let store = Arc::new(MemoryStore::default());
  store
    .set(SecretName::SpotifyAuth, "{pas du json")
    .expect("coffre mémoire");
  let core = crate::common::core_offline_with_store(store);
  {
    let mut shared = spotify::lock(&core);
    spotify::load_auth(&core, &mut shared);
  }
  let (_, out) = json_of(spotify::status(&core).await);
  assert_eq!(out["connected"], false);
  assert_eq!(
    out["error"],
    "Stored Spotify login could not be read. Please connect again."
  );
}

/// Garde-fou : un client qui n'a pas reçu de rejeu explicite échoue (dossier vide de `RIFF_HTTP_FIXTURES`), il
/// n'atteint jamais le réseau réel.
#[tokio::test]
async fn a_core_without_explicit_replay_fails_closed() {
  root();
  let core = riff_core::core::Core::new(riff_core::core::CoreConfig::default());
  for client in [
    &core.net.http_client,
    &core.net.yt_client,
    &core.net.image_client,
  ] {
    let Err(err) = client
      .get("https://api.deezer.com/chart/0/tracks")
      .send()
      .await
    else {
      panic!("une réponse est arrivée sans fixture");
    };
    assert!(err.to_string().contains("no fixture"), "{err}");
  }
  assert_eq!(misses().len(), 3);
}
