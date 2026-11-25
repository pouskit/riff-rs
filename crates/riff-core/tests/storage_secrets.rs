//! Jetons de `storage.json`, avec le vrai coffre du système sous un service de test, laissé vide à la fin.
//! Binaire à part : le fichier et le coffre ne sont partagés avec aucun autre test.
// Pas de Secret Service sur un runner Linux sans session : Windows et macOS gardent ce test.
#![cfg(not(target_os = "linux"))]

use riff_core::{paths, storage};
use serde_json::json;

#[path = "support/temp.rs"]
mod temp;

#[test]
fn a_plain_token_left_in_an_old_storage_file_moves_to_the_store() {
  let root = temp::TempDir::new("storage-secrets");
  std::fs::create_dir_all(root.join("data")).unwrap();
  // SAFETY : aucun autre fil n'existe encore dans ce binaire à un seul test.
  unsafe {
    std::env::set_var("RIFF_DATA_DIR", root.join("data"));
    std::env::set_var("LOCALAPPDATA", root.join("local"));
    std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
    std::env::set_var(
      "RIFF_KEYRING_SERVICE",
      format!("riff-rs-storage-test-{}", std::process::id()),
    );
  }
  let file = paths::storage_path();
  std::fs::create_dir_all(file.parent().unwrap()).unwrap();
  std::fs::write(
    &file,
    r#"{"riff_theme":"dark","riff_deepl_api_key":"JETON-CLAIR"}"#,
  )
  .unwrap();

  let read = storage::read_storage_disk().unwrap();
  assert_eq!(
    read["riff_deepl_api_key"],
    json!("JETON-CLAIR"),
    "l'interface garde sa clé"
  );
  let on_disk = std::fs::read_to_string(&file).unwrap();
  assert!(
    !on_disk.contains("JETON-CLAIR"),
    "clé encore en clair : {on_disk}"
  );
  assert!(on_disk.contains("riff_theme"));

  // Une nouvelle lecture la retrouve dans le coffre ; une écriture sans la clé l'efface du coffre.
  assert_eq!(
    storage::read_storage_disk().unwrap()["riff_deepl_api_key"],
    json!("JETON-CLAIR")
  );
  assert!(storage::write_storage_disk(
    &json!({ "riff_theme": "dark" })
  ));
  assert_eq!(
    storage::read_storage_disk().unwrap(),
    json!({ "riff_theme": "dark" })
  );
}
