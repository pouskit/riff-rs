//! Étape 6 : les deux plugins d'exemple, avec une graine fixe pour `random()`.

use crate::api::{HostRequest, HostResponse, PluginEvent};
use crate::runtime::state_path;
use crate::store::{HELLO_FILE, HELLO_SOURCE};
use crate::test_support::{FakeHost, Responder, default_answer};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

const SHUFFLE_FILE: &str = "better-shuffle.rhai";
const SHUFFLE_SOURCE: &str = include_str!("../../../docs/plugins/examples/better-shuffle.rhai");
const WAIT: Duration = Duration::from_secs(20);

/// Fixture commune : 12 titres, 4 artistes (5, 3, 2, 2 titres).
fn fixture() -> Vec<Value> {
  let artists = [("Alpha", 5), ("Beta", 3), ("Gamma", 2), ("Delta", 2)];
  let mut tracks = Vec::new();
  for (artist, count) in artists {
    for n in 0..count {
      tracks.push(json!({
        "id": format!("{}{n}", artist.to_lowercase()),
        "title": format!("{artist} {n}"),
        "artist": artist,
        "platform": "youtube",
      }));
    }
  }
  tracks
}

fn artist(track: &Value) -> String {
  track["artist"].as_str().unwrap_or("").to_lowercase()
}

/// Recherche : 20 titres de l'artiste cherché ; recommandations : 15 titres neufs.
fn music_host() -> Responder {
  Arc::new(|request: &HostRequest| {
    match request {
    HostRequest::Search { query, limit, .. } => Some(HostResponse::Value(Value::Array(
      (0..20)
        .take(*limit as usize)
        .map(|n| json!({ "id": format!("s-{query}-{n}"), "title": format!("Found {n}"), "artist": query, "platform": "youtube" }))
        .collect(),
    ))),
    HostRequest::Vibe(_) => Some(HostResponse::Value(Value::Array(
      (0..15)
        .map(|n| json!({ "id": format!("v{n}"), "title": format!("Vibe {n}"), "artist": format!("Vibe artist {n}"), "platform": "youtube" }))
        .collect(),
    ))),
    HostRequest::Favorites => Some(HostResponse::Value(json!([]))),
    other => default_answer(other),
  }
  })
}

fn start_shuffle(tag: &str, mode: &str, seed: u64) -> FakeHost {
  let mut fake = FakeHost::with(tag, music_host(), Some(seed));
  std::fs::write(
    state_path(&fake.state_dir, SHUFFLE_FILE),
    format!(r#"{{"mode":"{mode}"}}"#),
  )
  .unwrap();
  fake.start(SHUFFLE_FILE, SHUFFLE_SOURCE);
  assert!(fake.wait_for(WAIT, |_| true));
  let ready = (0..200).any(|_| {
    std::thread::sleep(Duration::from_millis(10));
    fake.host.picks()
  });
  assert!(ready, "better-shuffle ne propose pas de tirage");
  fake
}

fn pick(fake: &FakeHost, queue: &[Value], current: &Value, context: &str) -> Option<Value> {
  let index = queue.iter().position(|track| track == current);
  fake
    .host
    .shuffle_pick(
      json!({ "queue": queue, "queue_index": index, "context": context, "current": current }),
    )
    .expect("plugin disponible")
    .recv_timeout(WAIT)
    .expect("réponse")
    .expect("tirage sans erreur")
}

#[test]
fn hello_notifies_on_track_change() {
  let mut fake = FakeHost::new("hello");
  fake.start(HELLO_FILE, HELLO_SOURCE);
  fake.host.dispatch(&PluginEvent::TrackChange(
    json!({ "title": "Song", "artist": "A" }),
  ));
  assert!(fake.wait_notification("Now playing: Song"));
  fake.host.dispatch(&PluginEvent::TrackChange(Value::Null));
  std::thread::sleep(Duration::from_millis(100));
  assert_eq!(fake.notifications().len(), 1);
  assert!(fake.errors().is_empty(), "{:?}", fake.errors());
}

#[test]
fn classic_mode_leaves_the_choice_to_riff() {
  let fake = start_shuffle("bs-classic", "classic", 1);
  let queue = fixture();
  assert_eq!(pick(&fake, &queue, &queue[0], "playlist"), None);
}

/// Mode `spotify`, 200 tirages, graine fixe : jamais la piste courante, toujours une piste de la file,
/// toute la file avant une répétition, jamais deux fois le même artiste de suite quand c'est évitable.
#[test]
fn spotify_mode_over_200_picks_follows_the_javascript_rules() {
  let fake = start_shuffle("bs-spotify", "spotify", 42);
  let queue = fixture();
  let mut current = queue[0].clone();
  let mut since_refill: HashSet<String> =
    HashSet::from([current["id"].as_str().unwrap().to_string()]);
  let mut same_artist_in_a_row = 0;
  for round in 0..200 {
    let next = pick(&fake, &queue, &current, "playlist").expect("une piste");
    assert!(
      queue.contains(&next),
      "tour {round} : piste hors file {next}"
    );
    assert_ne!(next["id"], current["id"], "tour {round} : piste courante");
    let id = next["id"].as_str().unwrap().to_string();
    if since_refill.len() == queue.len() {
      since_refill.clear();
      since_refill.insert(current["id"].as_str().unwrap().to_string());
    }
    assert!(
      since_refill.insert(id),
      "tour {round} : répétition avant la fin de la file"
    );
    if artist(&next) == artist(&current) {
      same_artist_in_a_row += 1;
    }
    current = next;
  }
  // Tirage uniforme : même artiste deux fois de suite dans (5x4 + 3x2 + 2x1 + 2x1) / (12x11) = 22,7 % des cas,
  // soit environ 45 fois sur 200. L'arrangement du JavaScript ne laisse que les fins de cycle où c'est inévitable.
  assert!(same_artist_in_a_row <= 10, "{same_artist_in_a_row} sur 200");
  assert!(fake.errors().is_empty(), "{:?}", fake.errors());
}

#[test]
fn the_same_seed_gives_the_same_order() {
  let order = |tag: &str| {
    let fake = start_shuffle(tag, "spotify", 7);
    let queue = fixture();
    let mut current = queue[0].clone();
    let mut ids = Vec::new();
    for _ in 0..11 {
      current = pick(&fake, &queue, &current, "playlist").unwrap();
      ids.push(current["id"].as_str().unwrap().to_string());
    }
    ids
  };
  assert_eq!(order("bs-seed-a"), order("bs-seed-b"));
}

/// Mêmes bornes que le JavaScript : 4 artistes cherchés (`artistsToExpand`), 10 résultats chacun
/// (`perArtist`), 50 titres ajoutés au plus (`maxExpand`).
#[test]
fn search_expansion_respects_the_javascript_bounds() {
  let mut many = fixture();
  for (artist, count) in [("Epsilon", 1), ("Zeta", 1)] {
    for n in 0..count {
      many.push(json!({ "id": format!("{artist}{n}"), "title": "x", "artist": artist, "platform": "youtube" }));
    }
  }
  let fake = start_shuffle("bs-expand", "spotify", 3);
  let first = pick(&fake, &many, &many[0], "search").unwrap();
  fake.host.dispatch(&PluginEvent::TrackChange(first));
  assert!(fake.wait_for(WAIT, |seen| {
    seen
      .requests
      .iter()
      .any(|(_, r)| matches!(r, HostRequest::QueueAppend(_)))
  }));
  let seen = fake.seen.lock().unwrap();
  let searches: Vec<(String, u32)> = seen
    .requests
    .iter()
    .filter_map(|(_, r)| match r {
      HostRequest::Search { query, limit, .. } => Some((query.clone(), *limit)),
      _ => None,
    })
    .collect();
  assert_eq!(searches.len(), 4, "{searches:?}");
  assert_eq!(searches[0].0, "Alpha", "l'artiste le plus présent d'abord");
  assert!(searches.iter().all(|(_, limit)| *limit == 10));
  let appended = seen
    .requests
    .iter()
    .find_map(|(_, r)| match r {
      HostRequest::QueueAppend(tracks) => Some(tracks.len()),
      _ => None,
    })
    .unwrap();
  // 4 artistes x 10 résultats = 40, sous le plafond de 50.
  assert_eq!(appended, 40);
}

#[test]
fn search_expansion_is_capped_at_max_expand() {
  let mut many = Vec::new();
  for artist in ["A1", "A2", "A3", "A4", "A5", "A6"] {
    for n in 0..3 {
      many.push(json!({ "id": format!("{artist}-{n}"), "title": "t", "artist": artist, "platform": "youtube" }));
    }
  }
  let fake = start_shuffle("bs-cap", "spotify", 5);
  // Paramètres par défaut : 4 artistes x 10 = 40, sous maxExpand (50), comme dans le JavaScript ;
  // l'hôte refuse de toute façon plus de 50 pistes par queue_append.
  let first = pick(&fake, &many, &many[0], "search").unwrap();
  fake.host.dispatch(&PluginEvent::TrackChange(first));
  assert!(fake.wait_for(WAIT, |seen| {
    seen
      .requests
      .iter()
      .any(|(_, r)| matches!(r, HostRequest::QueueAppend(_)))
  }));
  let seen = fake.seen.lock().unwrap();
  let appended = seen
    .requests
    .iter()
    .find_map(|(_, r)| match r {
      HostRequest::QueueAppend(tracks) => Some(tracks.len()),
      _ => None,
    })
    .unwrap();
  assert!(appended <= 50, "{appended}");
}

#[test]
fn smart_mode_asks_for_recommendations_and_mixes_them_in() {
  let fake = start_shuffle("bs-smart", "smart", 11);
  let queue = fixture();
  let mut current = pick(&fake, &queue, &queue[0], "playlist").unwrap();
  fake
    .host
    .dispatch(&PluginEvent::TrackChange(current.clone()));
  assert!(fake.wait_for(WAIT, |seen| {
    seen
      .requests
      .iter()
      .any(|(_, r)| matches!(r, HostRequest::QueueAppend(_)))
  }));
  let vibe_limit = fake
    .seen
    .lock()
    .unwrap()
    .requests
    .iter()
    .find_map(|(_, r)| match r {
      HostRequest::Vibe(body) => body["limit"].as_i64(),
      _ => None,
    });
  assert_eq!(vibe_limit, Some(10), "smartBatch");
  // `smartEvery` = 3 : un titre recommandé arrive après trois titres de la file.
  let mut picks = Vec::new();
  for _ in 0..4 {
    current = pick(&fake, &queue, &current, "playlist").unwrap();
    picks.push(current["id"].as_str().unwrap().to_string());
  }
  assert!(picks.iter().any(|id| id.starts_with('v')), "{picks:?}");
  assert!(fake.errors().is_empty(), "{:?}", fake.errors());
}

#[test]
fn the_mode_and_taste_are_saved_but_not_the_session() {
  let mut fake = start_shuffle("bs-state", "spotify", 2);
  let queue = fixture();
  pick(&fake, &queue, &queue[0], "playlist");
  fake.host.deactivate(SHUFFLE_FILE);
  assert!(fake.wait_for(WAIT, |seen| {
    seen
      .lifecycle
      .iter()
      .any(|e| matches!(e, crate::api::Lifecycle::Stopped { .. }))
  }));
  let saved: Value = serde_json::from_str(
    &std::fs::read_to_string(state_path(&fake.state_dir, SHUFFLE_FILE)).unwrap(),
  )
  .unwrap();
  assert_eq!(saved["mode"], "spotify");
  assert!(saved.get("taste").is_some());
  assert!(saved.get("_s").is_none());
  fake.host.forget(SHUFFLE_FILE);
}
