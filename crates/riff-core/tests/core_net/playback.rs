//! `riff_core::services::playback`. Les fixtures du proxy audio sont écrites à la main (données factices).

use crate::common::{
  core_hand_written, core_offline, json_of, misses, query, root, stream_of, text_of, write_fixture,
};
use riff_core::reply::{HeaderLite, Method, Params, Reply};
use riff_core::services::playback::{
  AUDIO_EXTENSIONS, audio_output_info, audio_proxy, downloaded_track, downloads_dir,
  evict_stream_cache,
};
use serde_json::json;

const GV: &str = "https://rr1---sn-fake.googlevideo.com/videoplayback?c=IOS&id=1";

fn no_misses() {
  let m = misses();
  assert!(m.is_empty(), "fixtures manquantes :\n{}", m.join("\n"));
}

fn proxy_params(url: &str) -> Params {
  query(&[("url", url)])
}

#[test]
fn downloads_dir_is_the_configured_folder_and_exists() {
  let dir = downloads_dir();
  assert_eq!(dir, root().join("downloads"));
  assert!(dir.is_dir());
}

#[tokio::test]
async fn downloaded_track_serves_a_file_and_honours_ranges() {
  let dir = downloads_dir();
  std::fs::write(dir.join("pb_track.mp3"), (0..100u8).collect::<Vec<_>>()).expect("fichier");
  let full = downloaded_track(query(&[("name", "pb_track.mp3")])).await;
  match full {
    Ok(Reply::File {
      range: None, mime, ..
    }) => assert_eq!(mime, "audio/mpeg"),
    other => panic!("fichier entier attendu : {other:?}"),
  }
  let mut params = query(&[("name", "pb_track.mp3")]);
  params.headers = HeaderLite::new(vec![("Range".to_string(), "bytes=0-9".to_string())]);
  match downloaded_track(params).await {
    Ok(Reply::File {
      range: Some((0, 9)),
      ..
    }) => {}
    other => panic!("plage 0-9 attendue : {other:?}"),
  }
}

#[tokio::test]
async fn downloaded_track_rejects_unknown_files_and_non_audio_names() {
  let (status, text) = text_of(downloaded_track(query(&[("name", "pb_missing.mp3")])).await);
  assert_eq!(
    (status, text.as_str()),
    (404, "File not found in downloads")
  );
  for name in ["", "notes.txt", r"..\..\secret.mp3.txt", "noext"] {
    let (status, _) = text_of(downloaded_track(query(&[("name", name)])).await);
    assert_eq!(status, 404, "{name:?}");
  }
}

#[tokio::test]
async fn downloaded_track_ignores_path_components() {
  let (status, _) =
    text_of(downloaded_track(query(&[("name", "../../etc/pb_missing2.mp3")])).await);
  assert_eq!(
    status, 404,
    "le nom est réduit à son dernier composant, puis cherché dans le dossier"
  );
}

#[test]
fn audio_extensions_cover_the_supported_containers() {
  assert!(AUDIO_EXTENSIONS.contains(&"mp3") && AUDIO_EXTENSIONS.contains(&"flac"));
}

#[tokio::test]
async fn evict_stream_cache_drops_matching_entries_only() {
  let core = core_offline();
  {
    let mut cache = core.caches.stream_url_cache.lock().expect("verrou");
    cache.push(("a".into(), (u64::MAX, json!({"rawStreamUrl": "https://x/raw", "streamUrl": "/api/audio-proxy?url=https%3A%2F%2Fx%2Fraw"}))));
    cache.push((
      "b".into(),
      (u64::MAX, json!({"rawStreamUrl": "https://x/other"})),
    ));
  }
  evict_stream_cache(&core, "https://x/raw");
  let keys: Vec<String> = core
    .caches
    .stream_url_cache
    .lock()
    .expect("verrou")
    .iter()
    .map(|(k, _)| k.clone())
    .collect();
  assert_eq!(keys, ["b"]);
}

#[tokio::test]
async fn evict_stream_cache_with_an_empty_url_changes_nothing() {
  let core = core_offline();
  core
    .caches
    .stream_url_cache
    .lock()
    .expect("verrou")
    .push(("a".into(), (u64::MAX, json!({"rawStreamUrl": ""}))));
  evict_stream_cache(&core, "");
  assert_eq!(
    core.caches.stream_url_cache.lock().expect("verrou").len(),
    1
  );
}

#[tokio::test]
async fn audio_proxy_streams_the_hand_written_upstream_body() {
  let core =
    core_hand_written(|dir| write_fixture(dir, "GET", GV, 200, "FAKE-AUDIO", "audio/mpeg"));
  let (status, headers, body) =
    stream_of(audio_proxy(&core, Method::Get, proxy_params(GV)).await).await;
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(body, b"FAKE-AUDIO");
  assert!(
    headers
      .iter()
      .any(|(k, v)| k == "access-control-allow-origin" && v == "*")
  );
  assert!(
    headers
      .iter()
      .any(|(k, v)| k == "content-type" && v == "audio/mpeg")
  );
}

#[tokio::test]
async fn audio_proxy_head_returns_headers_without_a_body() {
  let core = core_hand_written(|dir| write_fixture(dir, "HEAD", GV, 200, "", "audio/mpeg"));
  let (status, headers, body) =
    stream_of(audio_proxy(&core, Method::Head, proxy_params(GV)).await).await;
  no_misses();
  assert_eq!(status, 200);
  assert!(body.is_empty());
  assert!(
    headers
      .iter()
      .any(|(k, v)| k == "content-length" && v == "0")
  );
}

#[tokio::test]
async fn audio_proxy_upstream_error_is_a_502_and_evicts_the_cache() {
  let core = core_hand_written(|dir| {
    write_fixture(dir, "GET", GV, 500, "Internal Server Error", "text/plain")
  });
  core
    .caches
    .stream_url_cache
    .lock()
    .expect("verrou")
    .push(("k".into(), (u64::MAX, json!({"rawStreamUrl": GV}))));
  let (status, _, body) = stream_of(audio_proxy(&core, Method::Get, proxy_params(GV)).await).await;
  no_misses();
  assert_eq!(
    (status, body.as_slice()),
    (502, b"Upstream error".as_slice())
  );
  assert!(
    core
      .caches
      .stream_url_cache
      .lock()
      .expect("verrou")
      .is_empty()
  );
}

#[tokio::test]
async fn audio_proxy_without_network_is_a_502() {
  let core = core_offline();
  let (status, _, body) = stream_of(audio_proxy(&core, Method::Get, proxy_params(GV)).await).await;
  assert_eq!((status, body.as_slice()), (502, b"Proxy error".as_slice()));
  assert_eq!(
    misses().len(),
    2,
    "un essai plus la reprise sur connexion morte"
  );
}

#[tokio::test]
async fn audio_proxy_rejects_bad_requests() {
  let core = core_offline();
  let cases: [(Method, Params, u16, &str); 5] = [
    (Method::Get, Params::default(), 400, "Missing url"),
    (Method::Post, proxy_params(GV), 405, "Method not allowed"),
    (Method::Get, proxy_params("pas une url"), 400, "Invalid url"),
    (
      Method::Get,
      proxy_params("ftp://rr1.googlevideo.com/x"),
      400,
      "Unsupported protocol",
    ),
    (
      Method::Get,
      proxy_params("https://evil.example/x.mp3"),
      403,
      "Blocked target",
    ),
  ];
  for (method, params, want_status, want_text) in cases {
    let (status, text) = text_of(audio_proxy(&core, method, params).await);
    assert_eq!((status, text.as_str()), (want_status, want_text));
  }
  assert!(misses().is_empty(), "aucune requête n'est partie");
}

#[tokio::test]
async fn audio_output_info_reports_the_default_output() {
  let (status, body) = json_of(audio_output_info().await);
  assert_eq!(status, 200);
  assert_eq!(body, json!({"name": "Default output", "battery": null}));
}
