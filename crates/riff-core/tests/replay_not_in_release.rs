//! Le binaire de release (`riff-app`, sans ses dépendances de développement) ne doit pas activer la feature `replay`.

use std::process::Command;

/// Arbre inversé des activations de features de `riff-core` pour `riff-app`, hors dépendances de développement.
fn features_of_riff_core(extra: &[&str]) -> String {
  let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
  let output = Command::new(cargo)
    .args([
      "tree",
      "-p",
      "riff-app",
      "-e",
      "no-dev,features",
      "-i",
      "riff-core",
    ])
    .args(["--offline", "--locked"])
    .args(extra)
    .current_dir(env!("CARGO_MANIFEST_DIR"))
    .output()
    .expect("cargo tree");
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn replay_is_not_enabled_in_the_release_dependency_graph() {
  let tree = features_of_riff_core(&[]);
  assert!(
    tree.contains("riff-core feature \"default\""),
    "graphe inattendu :\n{tree}"
  );
  assert!(
    !tree.contains("feature \"replay\""),
    "feature replay active hors tests :\n{tree}"
  );
}

#[test]
fn the_detector_sees_replay_when_it_is_enabled() {
  let tree = features_of_riff_core(&["--features", "riff-core/replay"]);
  assert!(tree.contains("feature \"replay\""), "{tree}");
}
