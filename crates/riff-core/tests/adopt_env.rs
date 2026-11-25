//! Reprise et choix du `storage.json` depuis l'environnement, sur des dossiers temporaires : jamais les vrais.
//! Binaire à part : il change l'environnement du processus.

use riff_core::adopt::{self, Adoption};
use riff_core::paths;
use std::fs;

#[path = "support/temp.rs"]
mod temp;

#[test]
fn adoption_follows_the_environment_and_ignores_a_test_profile() {
  let root = temp::TempDir::new("adopt-env");
  let (local, roaming, xdg_data, xdg_config, home) = (
    root.join("local"),
    root.join("roaming"),
    root.join("xdg-data"),
    root.join("xdg-config"),
    root.join("home"),
  );
  let test_profile = root.join("profil-essai");
  fs::create_dir_all(&test_profile).unwrap();
  // SAFETY : un seul test dans ce binaire, aucun autre fil ne lit l'environnement.
  unsafe {
    std::env::set_var("LOCALAPPDATA", &local);
    std::env::set_var("APPDATA", &roaming);
    std::env::set_var("XDG_DATA_HOME", &xdg_data);
    std::env::set_var("XDG_CONFIG_HOME", &xdg_config);
    std::env::set_var("HOME", &home);
    std::env::set_var("RIFF_DATA_DIR", &test_profile);
  }
  let old_data = paths::previous_data_dir();
  let old_config = paths::previous_config_dir();
  for dir in [&old_data, &old_config] {
    fs::create_dir_all(dir).unwrap();
  }
  fs::write(old_data.join("native-settings.json"), "{}").unwrap();
  fs::write(old_data.join("user-data.json"), "{\"x\":1}").unwrap();
  fs::write(old_config.join("storage.json"), "{\"tauri\":1}").unwrap();
  let electron = paths::electron_user_data_dir();
  fs::create_dir_all(&electron).unwrap();
  fs::write(electron.join("storage.json"), "{\"electron\":1}").unwrap();

  // Profil d'essai : aucune reprise, le dossier réel n'est pas créé, et seul son `storage.json` compte.
  assert_eq!(adopt::adopt_previous_data(), Adoption::Skipped);
  assert!(!paths::default_data_dir().exists());
  assert_eq!(
    adopt::import_storage_path(),
    fs::canonicalize(&test_profile)
      .unwrap()
      .join("storage.json")
  );
  fs::write(test_profile.join("storage.json"), "{\"k\":1}").unwrap();
  assert_eq!(adopt::import_storage_path(), paths::storage_path());

  // Sans profil d'essai : reprise, une seule fois.
  // SAFETY : voir ci-dessus.
  unsafe { std::env::remove_var("RIFF_DATA_DIR") };
  assert!(matches!(
    adopt::adopt_previous_data(),
    Adoption::Done { .. }
  ));
  assert!(paths::default_data_dir().join("user-data.json").is_file());
  assert_eq!(adopt::adopt_previous_data(), Adoption::Skipped);
  assert_eq!(
    fs::read_to_string(old_data.join("user-data.json")).unwrap(),
    "{\"x\":1}"
  );

  // Import : Riff d'origine ou Riff Tauri, jamais copié dans le nouveau dossier.
  let picked = adopt::import_storage_path();
  assert!(picked == electron.join("storage.json") || picked == old_config.join("storage.json"));
  assert!(!paths::state_dir().join("storage.json").exists());
}
