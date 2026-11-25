//! `riff_core::services::lastfm::lastfm`.
//!
//! Les fixtures de succès sont écrites à la main : la requête Last.fm est signée (MD5 des paramètres triés plus le
//! secret), le test recalcule la même signature avec une paire de clés factice passée dans le corps (`apiKey`,
//! `apiSecret`).

use crate::common::{
  body, core_hand_written, core_offline, json_of, misses, text_of, write_fixture,
  write_fixture_with_body,
};
use md5::{Digest, Md5};
use riff_core::reply::{Method, Params};
use riff_core::services::lastfm::lastfm;

const KEY: &str = "fakekey0123456789fakekey0123456789";
const SECRET: &str = "fakesecret";

fn no_misses() {
  let m = misses();
  assert!(m.is_empty(), "fixtures manquantes :\n{}", m.join("\n"));
}

fn sign(pairs: &[(&str, &str)]) -> String {
  let mut sorted = pairs.to_vec();
  sorted.sort();
  let mut sig: String = sorted.iter().map(|(k, v)| format!("{k}{v}")).collect();
  sig.push_str(SECRET);
  Md5::digest(sig.as_bytes())
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect()
}

fn with_keys(extra: &str) -> Params {
  body(&format!(
    r#"{{"apiKey":"{KEY}","apiSecret":"{SECRET}"{extra}}}"#
  ))
}

#[tokio::test]
async fn token_is_returned_with_its_authorization_url() {
  let sig = sign(&[("method", "auth.getToken"), ("api_key", KEY)]);
  let core = core_hand_written(|dir| {
    write_fixture(
      dir,
      "GET",
      &format!(
        "https://ws.audioscrobbler.com/2.0/?method=auth.getToken&api_key={KEY}&api_sig={sig}&format=json"
      ),
      200,
      r#"{"token":"tok123"}"#,
      "application/json",
    );
  });
  let (status, out) =
    json_of(lastfm(&core, Method::Post, "/api/lastfm/token", with_keys("")).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(out["success"], true);
  assert_eq!(out["token"], "tok123");
  let url = out["authUrl"].as_str().expect("authUrl");
  assert!(
    url.starts_with("https://www.last.fm/api/auth/?api_key=") && url.ends_with("&token=tok123")
  );
}

#[tokio::test]
async fn token_without_a_configured_secret_is_an_error() {
  // Ni paire personnalisée, ni variables d'environnement, ni secret enregistré (coffre mémoire vide).
  let core = core_offline();
  let (status, out) =
    json_of(lastfm(&core, Method::Get, "/api/lastfm/token", Params::default()).await);
  assert_eq!(status, 200);
  assert_eq!(out["success"], false);
  assert_eq!(
    out["error"],
    "Last.fm API key and secret are not configured"
  );
  assert!(misses().is_empty());
}

#[tokio::test]
async fn token_without_network_reports_the_transport_error() {
  let core = core_offline();
  let (_, out) = json_of(lastfm(&core, Method::Post, "/api/lastfm/token", with_keys("")).await);
  assert_eq!(out["success"], false);
  assert!(out["error"].as_str().unwrap_or("").contains("no fixture"));
  assert_eq!(misses().len(), 1);
}

#[tokio::test]
async fn token_http_500_is_an_invalid_response() {
  let sig = sign(&[("method", "auth.getToken"), ("api_key", KEY)]);
  let core = core_hand_written(|dir| {
    write_fixture(
      dir,
      "GET",
      &format!(
        "https://ws.audioscrobbler.com/2.0/?method=auth.getToken&api_key={KEY}&api_sig={sig}&format=json"
      ),
      500,
      "Internal Server Error",
      "text/plain",
    );
  });
  let (_, out) = json_of(lastfm(&core, Method::Post, "/api/lastfm/token", with_keys("")).await);
  no_misses();
  assert_eq!(out["success"], false);
  assert_eq!(out["error"], "Invalid Last.fm response");
}

#[tokio::test]
async fn now_playing_posts_a_signed_form_and_reports_success() {
  let pairs = [
    ("method", "track.updateNowPlaying"),
    ("sk", "sess123"),
    ("artist", "Daft Punk"),
    ("track", "One More Time"),
    ("api_key", KEY),
  ];
  let sig = sign(&pairs);
  let mut form = url::form_urlencoded::Serializer::new(String::new());
  form.extend_pairs(pairs);
  form.append_pair("api_sig", &sig);
  form.append_pair("format", "json");
  let form = form.finish();
  let core = core_hand_written(|dir| {
    write_fixture_with_body(
      dir,
      "POST",
      "https://ws.audioscrobbler.com/2.0/",
      form.as_bytes(),
      200,
      r##"{"nowplaying":{"track":{"#text":"One More Time"}}}"##,
      "application/json",
    );
  });
  let params = with_keys(r#","sk":"sess123","artist":"Daft Punk","track":"One More Time""#);
  let (_, out) = json_of(lastfm(&core, Method::Post, "/api/lastfm/now-playing", params).await);
  no_misses();
  assert_eq!(out["success"], true);
  assert_eq!(out["data"]["nowplaying"]["track"]["#text"], "One More Time");
}

#[tokio::test]
async fn scrobble_without_session_artist_or_track_is_refused() {
  let core = core_offline();
  let (status, out) =
    json_of(lastfm(&core, Method::Post, "/api/lastfm/scrobble", with_keys("")).await);
  assert_eq!(status, 200);
  assert_eq!(out["success"], false);
  assert_eq!(out["error"], "Missing sk/artist/track");
  assert!(misses().is_empty());
}

#[tokio::test]
async fn unknown_route_is_a_404() {
  let core = core_offline();
  let (status, text) =
    text_of(lastfm(&core, Method::Get, "/api/lastfm/nope", Params::default()).await);
  assert_eq!((status, text.as_str()), (404, "Not found"));
}
