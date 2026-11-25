//! Réglage multi-instance : écriture de `startup.json` dans un dossier de données isolé.

use riff_app::manage::{multi_instance_enabled, write_multi_instance};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, Once};

#[path = "support/temp.rs"]
mod temp;

use temp::SharedDir;

/// Le dossier de données est une variable d'environnement du processus : les tests passent un par un.
fn data_dir() -> (MutexGuard<'static, ()>, PathBuf) {
  static LOCK: Mutex<()> = Mutex::new(());
  static DIR: SharedDir = SharedDir::new("app-multi-instance");
  static ENV: Once = Once::new();
  let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
  let dir = DIR.path();
  // SAFETY : posé une seule fois sous le verrou, avant toute lecture de l'environnement par ce binaire.
  ENV.call_once(|| unsafe { std::env::set_var("RIFF_DATA_DIR", &dir) });
  DIR.lease(|dir| std::fs::create_dir_all(dir).expect("dossier de test"));
  let startup = dir.join("startup.json");
  if startup.is_dir() {
    std::fs::remove_dir(&startup).expect("dossier de test");
  }
  let _ = std::fs::remove_file(&startup);
  (guard, dir)
}

#[test]
fn the_default_is_off() {
  let (_guard, _dir) = data_dir();
  assert!(!multi_instance_enabled());
}

#[test]
fn the_value_is_written_then_read_back() {
  let (_guard, dir) = data_dir();
  assert_eq!(write_multi_instance(true), Ok(()));
  assert!(multi_instance_enabled());
  let raw = std::fs::read_to_string(dir.join("startup.json")).expect("fichier écrit");
  assert_eq!(
    serde_json::from_str::<serde_json::Value>(&raw).expect("JSON")["multiInstance"],
    true
  );
  assert_eq!(write_multi_instance(false), Ok(()));
  assert!(!multi_instance_enabled());
}

#[test]
fn the_other_keys_of_startup_json_are_kept() {
  let (_guard, dir) = data_dir();
  std::fs::write(
    dir.join("startup.json"),
    r#"{"hardwareAcceleration":false,"nested":{"a":1}}"#,
  )
  .expect("écriture de test");
  assert_eq!(write_multi_instance(true), Ok(()));
  let raw = std::fs::read_to_string(dir.join("startup.json")).expect("fichier écrit");
  let value: serde_json::Value = serde_json::from_str(&raw).expect("JSON");
  assert_eq!(value["hardwareAcceleration"], false);
  assert_eq!(value["nested"]["a"], 1);
  assert_eq!(value["multiInstance"], true);
}

#[test]
fn a_failed_write_gives_back_the_value_read() {
  let (_guard, dir) = data_dir();
  assert_eq!(write_multi_instance(true), Ok(()));
  // Un dossier à la place du fichier : l'écriture échoue, la lecture rend le défaut.
  std::fs::remove_file(dir.join("startup.json")).expect("fichier de test");
  std::fs::create_dir(dir.join("startup.json")).expect("dossier de test");
  assert_eq!(write_multi_instance(true), Err(false));
}
