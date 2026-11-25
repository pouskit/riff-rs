//! `RIFF_DATA_DIR` déplace aussi les fichiers d'état de l'interface (`storage.json`, `window-state.json`, `cache.json`,
//! `local-covers`) : une instance de test ou une instance multiple ne touche jamais ceux de l'instance réelle.
//! Binaire à part : il change l'environnement du processus.

use riff_core::paths;

#[path = "support/temp.rs"]
mod temp;

#[test]
fn state_files_follow_riff_data_dir_and_fall_back_to_the_config_dir_without_it() {
  let root = temp::TempDir::new("paths-override");
  let data = root.join("data");
  std::fs::create_dir_all(&data).unwrap();
  // SAFETY : un seul test dans ce binaire, aucun autre fil ne lit l'environnement.
  unsafe {
    std::env::set_var("LOCALAPPDATA", root.join("local"));
    std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
    // macOS résout le dossier de config depuis le dossier personnel : jamais le vrai.
    #[cfg(unix)]
    std::env::set_var("HOME", root.join("home"));
    std::env::remove_var("RIFF_DATA_DIR");
  }
  let config = paths::config_dir();
  assert_eq!(paths::storage_path(), config.join("storage.json"));
  assert_eq!(paths::window_state_path(), config.join("window-state.json"));

  // SAFETY : voir ci-dessus.
  unsafe { std::env::set_var("RIFF_DATA_DIR", &data) };
  let resolved = paths::resolve_data_dir();
  assert_eq!(paths::storage_path(), resolved.join("storage.json"));
  assert_eq!(
    paths::window_state_path(),
    resolved.join("window-state.json")
  );
  assert_eq!(paths::cache_path(), resolved.join("cache.json"));
  assert_eq!(paths::local_covers_dir(), resolved.join("local-covers"));

  // SAFETY : voir ci-dessus.
  unsafe { std::env::set_var("RIFF_DATA_DIR", "   ") };
  assert_eq!(
    paths::storage_path(),
    config.join("storage.json"),
    "valeur vide : ignorée comme par resolve_data_dir"
  );
}
