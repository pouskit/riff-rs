//! Échoue si la liste noire de `deny.toml` est contournée : entrée retirée, ou crate de télémétrie dans `Cargo.lock`.

use std::collections::BTreeSet;
use std::path::Path;

fn workspace_file(name: &str) -> String {
  let path = Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../..")
    .join(name);
  std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn banned_crates(deny_toml: &str) -> BTreeSet<String> {
  let re = regex::Regex::new(r#"\{\s*crate\s*=\s*"([^"]+)"\s*,\s*reason\s*=\s*"télémétrie"\s*\}"#)
    .unwrap();
  re.captures_iter(deny_toml)
    .map(|c| c[1].to_string())
    .collect()
}

fn locked_crates(cargo_lock: &str) -> BTreeSet<String> {
  let re = regex::Regex::new(r#"(?m)^name = "([^"]+)"\r?$"#).unwrap();
  re.captures_iter(cargo_lock)
    .map(|c| c[1].to_string())
    .collect()
}

#[test]
fn the_telemetry_ban_list_stays_complete() {
  let banned = banned_crates(&workspace_file("deny.toml"));
  for name in [
    "sentry",
    "posthog",
    "segment",
    "opentelemetry",
    "datadog",
    "amplitude",
    "mixpanel",
    "bugsnag",
  ] {
    assert!(
      banned.contains(name),
      "{name} a disparu de la liste noire de deny.toml"
    );
  }
}

#[test]
fn no_telemetry_crate_is_locked() {
  let banned = banned_crates(&workspace_file("deny.toml"));
  let locked = locked_crates(&workspace_file("Cargo.lock"));
  assert!(
    locked.contains("reqwest"),
    "lecture de Cargo.lock incorrecte"
  );
  let found: Vec<&String> = banned.intersection(&locked).collect();
  assert!(
    found.is_empty(),
    "crates de télémétrie dans Cargo.lock : {found:?}"
  );
  let family: Vec<&String> = locked
    .iter()
    .filter(|n| {
      n.starts_with("sentry") || n.starts_with("opentelemetry") || n.starts_with("posthog")
    })
    .collect();
  assert!(
    family.is_empty(),
    "crates de télémétrie dans Cargo.lock : {family:?}"
  );
}
