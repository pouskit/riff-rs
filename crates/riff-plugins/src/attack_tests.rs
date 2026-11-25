//! Étape 3 : tests d'attaque. Chaque plugin hostile doit être arrêté proprement, sans panique de l'application
//! et sans bloquer l'appelant de `dispatch`.

use crate::api::{EVENT_BUDGET, HostRequest, HostResponse, Lifecycle, PluginEvent};
use crate::test_support::{FakeHost, default_answer};
use std::sync::Arc;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(20);

fn disabled_reason(fake: &FakeHost) -> Option<String> {
  fake
    .seen
    .lock()
    .unwrap()
    .lifecycle
    .iter()
    .find_map(|event| match event {
      Lifecycle::Disabled { reason, .. } => Some(reason.clone()),
      _ => None,
    })
}

fn wait_disabled(fake: &FakeHost, timeout: Duration) -> Option<String> {
  fake.wait_for(timeout, |seen| {
    seen
      .lifecycle
      .iter()
      .any(|event| matches!(event, Lifecycle::Disabled { .. }))
  });
  disabled_reason(fake)
}

fn wait_errors(fake: &FakeHost, count: usize) -> Vec<String> {
  fake.wait_for(WAIT, |seen| {
    seen
      .lifecycle
      .iter()
      .filter(|event| matches!(event, Lifecycle::Error { .. }))
      .count()
      >= count
  });
  fake.errors()
}

/// `dispatch` ne doit jamais attendre le plugin.
fn dispatch_is_instant(fake: &FakeHost) {
  let started = Instant::now();
  for _ in 0..100 {
    fake.host.dispatch(&PluginEvent::Play);
  }
  assert!(started.elapsed() < Duration::from_millis(250));
}

#[test]
fn an_infinite_loop_is_stopped_by_the_time_budget_and_disables_the_plugin() {
  let mut fake = FakeHost::new("loop");
  let started = Instant::now();
  fake.start("loop.rhai", "// @name Boucle\nfn on_ready() { loop { } }\n");
  dispatch_is_instant(&fake);
  let reason = wait_disabled(&fake, WAIT).expect("désactivé");
  assert!(reason.contains("budget"), "{reason}");
  assert!(
    started.elapsed() >= EVENT_BUDGET && started.elapsed() < EVENT_BUDGET * 10,
    "{:?}",
    started.elapsed()
  );
}

#[test]
fn a_busy_loop_inside_the_shuffle_pick_gets_its_own_longer_budget() {
  let mut fake = FakeHost::new("pickloop");
  fake.start(
    "pickloop.rhai",
    "// @name Choix lent\n// @permissions shuffle\nfn shuffle_pick(ctx) { loop { } }\n",
  );
  assert!(fake.wait_for(WAIT, |_| true) && fake.host.picks());
  let started = Instant::now();
  let answer = fake
    .host
    .shuffle_pick(serde_json::json!({}))
    .unwrap()
    .recv_timeout(WAIT)
    .unwrap();
  assert!(answer.unwrap_err().contains("budget"));
  let elapsed = started.elapsed();
  assert!(
    elapsed >= Duration::from_secs(3) && elapsed < Duration::from_secs(6),
    "{elapsed:?}"
  );
  assert!(wait_disabled(&fake, WAIT).is_some());
}

#[test]
fn infinite_recursion_is_stopped_at_the_call_depth() {
  let mut fake = FakeHost::new("recursion");
  fake.start(
    "rec.rhai",
    "// @name Récursion\nfn f(n) { f(n + 1) }\nfn on_ready() { f(0) }\n",
  );
  let errors = wait_errors(&fake, 1);
  assert!(errors[0].to_lowercase().contains("stack"), "{errors:?}");
}

#[test]
fn deep_expressions_are_refused_at_compile_time() {
  let depth = 5_000;
  let code = format!(
    "// @name Profond\nfn on_ready() {{ {}1{} }}\n",
    "(".repeat(depth),
    ")".repeat(depth)
  );
  let mut fake = FakeHost::new("deep");
  fake.start("deep.rhai", &code);
  let failed = fake
    .seen
    .lock()
    .unwrap()
    .lifecycle
    .iter()
    .any(|event| matches!(event, Lifecycle::Failed { reason, .. } if reason.contains("ligne")));
  assert!(failed);
}

#[test]
fn a_100_mb_string_is_refused() {
  let mut fake = FakeHost::new("string");
  fake.start(
    "string.rhai",
    "// @name Chaîne\n\
     fn on_ready() { let s = \"\"; s.pad(100000000, \"x\"); }\n\
     fn on_play() { let s = \"xxxxxxxxxx\"; loop { s += s; } }\n",
  );
  let errors = wait_errors(&fake, 1);
  assert!(
    errors[0].contains("too large") || errors[0].contains("Length"),
    "{errors:?}"
  );
  fake.host.dispatch(&PluginEvent::Play);
  let errors = wait_errors(&fake, 2);
  assert!(
    errors[1].contains("too large") || errors[1].contains("Length"),
    "{errors:?}"
  );
}

#[test]
fn a_giant_array_is_refused() {
  let mut fake = FakeHost::new("array");
  fake.start(
    "array.rhai",
    "// @name Tableau\n\
     fn on_ready() { let a = []; a.pad(100000000, 0); }\n\
     fn on_play() { let a = [1]; loop { a = [a, a, a, a]; } }\n\
     fn on_pause() { let a = []; a.pad(20000, 0); }\n",
  );
  let errors = wait_errors(&fake, 1);
  assert!(
    errors[0].contains("too large") || errors[0].contains("Size"),
    "{errors:?}"
  );
  fake.host.dispatch(&PluginEvent::Play);
  let errors = wait_errors(&fake, 2);
  assert!(
    errors[1].contains("too large") || errors[1].contains("Size"),
    "{errors:?}"
  );
  // Le troisième échec de suite désactive le plugin.
  fake.host.dispatch(&PluginEvent::Pause);
  let reason = wait_disabled(&fake, WAIT).expect("désactivé");
  assert!(reason.contains("3 échecs"), "{reason}");
}

#[test]
fn a_giant_map_is_refused() {
  let mut fake = FakeHost::new("map");
  fake.start(
    "map.rhai",
    "// @name Carte\nfn on_ready() { let m = #{}; let i = 0; loop { m[`k${i}`] = i; i += 1; } }\n",
  );
  // Selon la vitesse de la machine, la limite de taille ou le budget de temps arrête le script le premier.
  fake.wait_for(WAIT, |seen| {
    seen
      .lifecycle
      .iter()
      .any(|event| matches!(event, Lifecycle::Error { .. } | Lifecycle::Disabled { .. }))
  });
  let errors = fake.errors();
  let stopped = errors
    .first()
    .is_some_and(|error| error.contains("too large") || error.contains("Size"))
    || disabled_reason(&fake).is_some_and(|reason| reason.contains("budget"));
  assert!(stopped, "{errors:?} {:?}", disabled_reason(&fake));
}

#[test]
fn a_host_call_that_never_answers_times_out_without_blocking() {
  let silent: crate::test_support::Responder = Arc::new(|request: &HostRequest| match request {
    HostRequest::Queue => None,
    other => default_answer(other),
  });
  let mut fake = FakeHost::with("silent", silent, Some(1));
  fake.start(
    "silent.rhai",
    "// @name Muet\n// @permissions queue\nfn on_play() { queue() }\n",
  );
  let started = Instant::now();
  fake.host.dispatch(&PluginEvent::Play);
  assert!(started.elapsed() < Duration::from_millis(250));
  let errors = wait_errors(&fake, 1);
  assert!(errors[0].contains("n'a pas répondu"), "{errors:?}");
  // L'attente de l'application n'est pas comptée dans le budget : le plugin reste actif.
  assert!(disabled_reason(&fake).is_none());
}

#[test]
fn a_stopped_executor_gives_an_error_at_once() {
  let dropped: crate::test_support::Responder = Arc::new(|request: &HostRequest| match request {
    HostRequest::Queue => Some(HostResponse::Error(crate::test_support::DROP.to_string())),
    other => default_answer(other),
  });
  let mut fake = FakeHost::with("dropped", dropped, Some(1));
  fake.start(
    "dropped.rhai",
    "// @name Abandon\n// @permissions queue\nfn on_play() { queue() }\n",
  );
  let started = Instant::now();
  fake.host.dispatch(&PluginEvent::Play);
  let errors = wait_errors(&fake, 1);
  assert!(
    started.elapsed() < Duration::from_millis(900),
    "{:?}",
    started.elapsed()
  );
  assert!(errors[0].contains("arrêtée"), "{errors:?}");
}

#[test]
fn a_host_error_reaches_the_plugin_as_an_error() {
  let failing: crate::test_support::Responder = Arc::new(|request: &HostRequest| match request {
    HostRequest::Queue => Some(HostResponse::Error("file indisponible".to_string())),
    other => default_answer(other),
  });
  let mut fake = FakeHost::with("hosterr", failing, Some(1));
  fake.start(
    "hosterr.rhai",
    "// @name Erreur\n// @permissions queue\nfn on_ready() { queue() }\n",
  );
  assert!(wait_errors(&fake, 1)[0].contains("file indisponible"));
}

#[test]
fn a_plugin_that_throws_on_every_event_is_disabled_after_three_failures() {
  let mut fake = FakeHost::new("throws");
  fake.start(
    "throws.rhai",
    "// @name Erreurs\nfn on_play() { throw \"toujours\"; }\n",
  );
  for _ in 0..2 {
    fake.host.dispatch(&PluginEvent::Play);
  }
  assert_eq!(wait_errors(&fake, 2).len(), 2);
  assert!(disabled_reason(&fake).is_none());
  fake.host.dispatch(&PluginEvent::Play);
  let reason = wait_disabled(&fake, WAIT).expect("désactivé");
  assert!(
    reason.contains("3 échecs") && reason.contains("toujours"),
    "{reason}"
  );
}

#[test]
fn a_success_resets_the_failure_count() {
  let mut fake = FakeHost::new("reset");
  fake.start(
    "reset.rhai",
    "// @name Alterné\nfn on_play() { throw \"non\"; }\nfn on_pause() { 1 }\n",
  );
  for event in [
    PluginEvent::Play,
    PluginEvent::Play,
    PluginEvent::Pause,
    PluginEvent::Play,
    PluginEvent::Play,
  ] {
    fake.host.dispatch(&event);
    std::thread::sleep(Duration::from_millis(30));
  }
  assert_eq!(wait_errors(&fake, 4).len(), 4);
  std::thread::sleep(Duration::from_millis(100));
  assert!(disabled_reason(&fake).is_none());
}

#[test]
fn files_network_processes_and_sleep_do_not_exist() {
  for (index, call) in [
    "open(\"C:/Windows/win.ini\")",
    "read_file(\"secret.txt\")",
    "fetch(\"https://example.com\")",
    "http_get(\"http://127.0.0.1\")",
    "system(\"calc\")",
    "sleep(10)",
    "eval(\"1\")",
  ]
  .iter()
  .enumerate()
  {
    let mut fake = FakeHost::new(&format!("io{index}"));
    let file = format!("io{index}.rhai");
    fake.start(
      &file,
      &format!("// @name E/S\nfn on_ready() {{ {call} }}\n"),
    );
    let refused = fake.wait_for(WAIT, |seen| {
      seen
        .lifecycle
        .iter()
        .any(|event| matches!(event, Lifecycle::Error { .. } | Lifecycle::Failed { .. }))
    });
    assert!(refused, "{call}");
    assert!(fake.seen.lock().unwrap().requests.is_empty(), "{call}");
  }
}

#[test]
fn an_import_never_reads_a_file() {
  let mut fake = FakeHost::new("import");
  fake.start(
    "import.rhai",
    "// @name Import\nimport \"../../secret\" as s;\nfn on_ready() { 1 }\n",
  );
  // Les instructions globales s'exécutent avant chaque appel : l'import échoue, sans lecture de fichier.
  let errors = wait_errors(&fake, 1);
  assert!(errors[0].to_lowercase().contains("module"), "{errors:?}");
}

#[test]
fn a_panic_in_the_engine_disables_the_plugin_without_killing_the_app() {
  let mut fake = FakeHost::new("panic");
  fake.start(
    "panic.rhai",
    "// @name Panique\nfn on_ready() { __test_panic() }\n",
  );
  let reason = wait_disabled(&fake, WAIT).expect("désactivé");
  assert!(reason.contains("panique"), "{reason}");
  // L'hôte continue de servir les autres plugins.
  fake.start(
    "after.rhai",
    "// @name Après\n// @permissions notify\nfn on_ready() { notify(\"vivant\") }\n",
  );
  assert!(fake.wait_notification("vivant"));
}

#[test]
fn a_disabled_plugin_receives_nothing_more() {
  let mut fake = FakeHost::new("after-disable");
  fake.start("dead.rhai", "// @name Mort\nfn on_ready() { loop {} }\n");
  assert!(wait_disabled(&fake, WAIT).is_some());
  fake.host.forget("dead.rhai");
  assert!(!fake.host.is_active());
  dispatch_is_instant(&fake);
}

#[test]
fn deactivation_interrupts_a_running_call() {
  let mut fake = FakeHost::new("interrupt");
  fake.start(
    "slow.rhai",
    "// @name Lent\n// @permissions notify\nfn on_play() { loop {} }\nfn stop() { notify(\"arrêté\") }\n",
  );
  fake.host.dispatch(&PluginEvent::Play);
  std::thread::sleep(Duration::from_millis(50));
  let started = Instant::now();
  fake.host.deactivate("slow.rhai");
  assert!(started.elapsed() < Duration::from_millis(100));
  assert!(fake.wait_notification("arrêté"));
  assert!(disabled_reason(&fake).is_none());
}
