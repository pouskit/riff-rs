//! Étape 2 : événements, API hôte, permissions, état persistant, journal.

use crate::api::{HostRequest, Lifecycle, PluginEvent};
use crate::runtime::STATE_MAX_BYTES;
use crate::test_support::FakeHost;
use serde_json::json;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn ready_runs_at_activation_and_events_reach_their_function() {
  let mut fake = FakeHost::new("events");
  fake.start(
    "events.rhai",
    "// @name Events\n// @permissions notify\n\
     fn on_ready() { notify(\"ready\") }\n\
     fn on_play() { notify(\"play\") }\n\
     fn on_pause() { notify(\"pause\") }\n\
     fn on_trackchange(track) { notify(`change ${track.title}`) }\n",
  );
  assert!(fake.wait_notification("ready"));
  fake.host.dispatch(&PluginEvent::Play);
  fake.host.dispatch(&PluginEvent::Pause);
  fake
    .host
    .dispatch(&PluginEvent::TrackChange(json!({ "title": "Song" })));
  assert!(fake.wait_notification("change Song"));
  assert_eq!(
    fake.notifications(),
    vec!["ready", "play", "pause", "change Song"]
  );
}

#[test]
fn a_missing_event_function_is_not_an_error() {
  let mut fake = FakeHost::new("missing");
  fake.start("missing.rhai", "// @name Rien\nfn on_play() { 1 }\n");
  fake.host.dispatch(&PluginEvent::Pause);
  fake.host.dispatch(&PluginEvent::Play);
  std::thread::sleep(Duration::from_millis(200));
  assert!(fake.errors().is_empty(), "{:?}", fake.errors());
}

/// Chaque fonction gardée, appelée sans sa permission : fonction absente, erreur qui nomme la permission.
#[test]
fn every_guarded_function_is_refused_without_its_permission() {
  let calls = [
    ("queue()", "queue"),
    ("queue_index()", "queue"),
    ("queue_context()", "queue"),
    ("queue_append([])", "queue"),
    ("favorites()", "library"),
    ("history()", "library"),
    ("search(\"a\")", "search"),
    ("search(\"a\", #{ limit: 3 })", "search"),
    ("vibe(#{})", "search"),
    ("notify(\"x\")", "notify"),
    ("notify(\"x\", \"error\")", "notify"),
    ("shuffle_on()", "shuffle"),
  ];
  for (index, (call, permission)) in calls.iter().enumerate() {
    let mut fake = FakeHost::new(&format!("deny{index}"));
    let file = format!("deny{index}.rhai");
    fake.start(
      &file,
      &format!("// @name Refus\nfn on_ready() {{ {call} }}\n"),
    );
    assert!(
      fake.wait_for(WAIT, |seen| seen
        .lifecycle
        .iter()
        .any(|e| matches!(e, Lifecycle::Error { .. }))),
      "{call}"
    );
    let errors = fake.errors();
    assert!(
      errors[0].contains(&format!("permission refusée : {permission}")),
      "{call}: {errors:?}"
    );
    assert!(fake.seen.lock().unwrap().requests.is_empty(), "{call}");
  }
}

/// Avec sa permission, chaque fonction gardée atteint l'application et renvoie sa réponse.
#[test]
fn every_guarded_function_works_with_its_permission() {
  let mut fake = FakeHost::new("allow");
  fake.start(
    "allow.rhai",
    "// @name Tout\n// @permissions queue, library, search, shuffle, notify\n\
     fn on_ready() {\n\
       let n = queue().len() + favorites().len() + history().len();\n\
       let found = search(\"x\", #{ limit: 99, platform: \"youtube\" }).len() + vibe(#{ limit: 1 }).len();\n\
       let added = queue_append([#{ title: \"Nouveau\", id: \"n1\" }]).len();\n\
       notify(`${n} ${found} ${added} ${queue_index()} ${queue_context()} ${shuffle_on()}`, \"success\");\n\
     }\n",
  );
  assert!(fake.wait_notification("9 2 1 0 playlist true"));
  let seen = fake.seen.lock().unwrap();
  let search = seen.requests.iter().find_map(|(_, request)| match request {
    HostRequest::Search {
      limit, platform, ..
    } => Some((*limit, platform.clone())),
    _ => None,
  });
  assert_eq!(search, Some((30, Some("youtube".to_string()))));
}

#[test]
fn functions_without_permission_are_always_available() {
  let mut fake = FakeHost::new("free");
  fake.start(
    "free.rhai",
    "// @name Libre\n// @permissions notify\n\
     fn on_ready() {\n\
       let t = track();\n\
       let same = same_track(t, #{ platform: \"youtube\", id: \"1\" });\n\
       let r = random();\n\
       log(\"bonjour\");\n\
       notify(`${t.title} ${is_playing()} ${same} ${r >= 0.0 && r < 1.0}`);\n\
     }\n",
  );
  assert!(fake.wait_notification("Un true true true"));
  assert!(fake.wait_for(WAIT, |seen| {
    seen
      .requests
      .iter()
      .any(|(_, r)| matches!(r, HostRequest::Log(text) if text == "bonjour"))
  }));
}

#[test]
fn appended_tracks_are_cleaned_and_need_a_title_and_an_id() {
  let mut fake = FakeHost::new("append");
  fake.start(
    "append.rhai",
    "// @name Ajout\n// @permissions queue\n\
     fn on_ready() {\n\
       queue_append([\n\
         #{ title: \"Bon\", id: \"a1\", path: \"C:/secret.mp3\", url: \"file:///C:/x\" },\n\
         #{ title: \"Sans id\" },\n\
         \"pas une piste\"\n\
       ]);\n\
     }\n",
  );
  assert!(fake.wait_for(WAIT, |seen| {
    seen
      .requests
      .iter()
      .any(|(_, r)| matches!(r, HostRequest::QueueAppend(_)))
  }));
  let seen = fake.seen.lock().unwrap();
  let tracks = seen
    .requests
    .iter()
    .find_map(|(_, r)| match r {
      HostRequest::QueueAppend(tracks) => Some(tracks.clone()),
      _ => None,
    })
    .unwrap();
  assert_eq!(tracks, vec![json!({ "id": "a1", "title": "Bon" })]);
}

#[test]
fn notifications_and_logs_are_cleaned_and_rate_limited() {
  let mut fake = FakeHost::new("rate");
  fake.start(
    "rate.rhai",
    "// @name Débit\n// @permissions notify\n\
     fn on_ready() {\n\
       let sent = 0;\n\
       for i in 0..20 { if notify(\"n\" + i + \"\\x1b[2J\") { sent += 1; } }\n\
       for i in 0..100 { log(\"l\"); }\n\
       this.sent = sent;\n\
     }\n\
     fn on_play() { notify(`sent ${this.sent}`) }\n",
  );
  std::thread::sleep(Duration::from_millis(300));
  let notes = fake.notifications();
  assert_eq!(notes.len(), 5, "{notes:?}");
  assert_eq!(notes[0], "n0[2J");
  let logs = fake
    .seen
    .lock()
    .unwrap()
    .requests
    .iter()
    .filter(|(_, r)| matches!(r, HostRequest::Log(_)))
    .count();
  assert_eq!(logs, 20);
  assert!(fake.errors().is_empty(), "{:?}", fake.errors());
}

#[test]
fn state_is_this_and_survives_a_restart() {
  let mut fake = FakeHost::new("state");
  let code = "// @name État\n// @permissions notify\n\
     fn on_ready() { if this.runs == () { this.runs = 0; } this.runs += 1; this._temp = \"x\"; notify(`runs ${this.runs}`); }\n";
  fake.start("state.rhai", code);
  assert!(fake.wait_notification("runs 1"));
  fake.host.deactivate("state.rhai");
  assert!(fake.wait_for(WAIT, |seen| {
    seen
      .lifecycle
      .iter()
      .any(|e| matches!(e, Lifecycle::Stopped { .. }))
  }));
  let saved =
    std::fs::read_to_string(crate::runtime::state_path(&fake.state_dir, "state.rhai")).unwrap();
  assert_eq!(saved, r#"{"runs":1}"#);
  fake.start("state.rhai", code);
  assert!(fake.wait_notification("runs 2"));
}

#[test]
fn a_state_over_256_kb_is_refused_and_not_written() {
  let mut fake = FakeHost::new("bigstate");
  fake.start(
    "big.rhai",
    "// @name Gros\n\
     fn on_ready() { let s = \"\"; s.pad(200000, \"x\"); this.a = s; this.b = s; }\n",
  );
  assert!(fake.wait_for(WAIT, |seen| {
    seen
      .lifecycle
      .iter()
      .any(|e| matches!(e, Lifecycle::Error { .. }))
  }));
  assert!(
    fake.errors()[0].contains("état trop gros"),
    "{:?}",
    fake.errors()
  );
  assert!(fake.errors()[0].contains(&STATE_MAX_BYTES.to_string()));
  assert!(!crate::runtime::state_path(&fake.state_dir, "big.rhai").exists());
}

#[test]
fn stop_runs_on_deactivation_then_the_plugin_is_gone() {
  let mut fake = FakeHost::new("stop");
  fake.start(
    "stop.rhai",
    "// @name Stop\n// @permissions notify\nfn stop() { notify(\"bye\") }\n",
  );
  assert!(fake.host.is_running("stop.rhai"));
  fake.host.deactivate("stop.rhai");
  assert!(!fake.host.is_active());
  assert!(fake.wait_notification("bye"));
}

#[test]
fn shuffle_pick_returns_a_track_nothing_or_an_error() {
  let mut fake = FakeHost::new("pick");
  fake.start(
    "pick.rhai",
    "// @name Choix\n// @permissions shuffle\n\
     fn shuffle_pick(ctx) {\n\
       if ctx.mode == \"none\" { return (); }\n\
       if ctx.mode == \"error\" { throw \"raté\"; }\n\
       ctx.queue[1]\n\
     }\n",
  );
  assert!(fake.wait_for(WAIT, |_| fake_picks(&fake)));
  let queue = crate::test_support::sample_queue();
  let ask = |mode: &str| {
    fake
      .host
      .shuffle_pick(json!({ "queue": queue, "mode": mode }))
      .unwrap()
      .recv_timeout(WAIT)
      .unwrap()
  };
  assert_eq!(ask("track"), Ok(Some(queue[1].clone())));
  assert_eq!(ask("none"), Ok(None));
  assert!(ask("error").unwrap_err().contains("raté"));
}

fn fake_picks(fake: &FakeHost) -> bool {
  fake.host.picks()
}

#[test]
fn shuffle_pick_needs_the_shuffle_permission() {
  let mut fake = FakeHost::new("nopick");
  fake.start(
    "nopick.rhai",
    "// @name Sans\nfn shuffle_pick(ctx) { ctx.queue[0] }\n",
  );
  std::thread::sleep(Duration::from_millis(100));
  assert!(!fake.host.picks());
  assert!(fake.host.shuffle_pick(json!({})).is_none());
}
