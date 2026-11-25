//! `riff_core::services::translate`.
//!
//! Le réglage `translationEnabled` vit dans le fichier de config partagé : les tests qui le fixent se sérialisent par
//! `translation_setting`.

use crate::common::{
  body, core_hand_written, core_offline, core_recorded, json_of, misses, translation_setting,
  write_fixture_with_body,
};
use riff_core::services::translate;
use serde_json::json;

fn no_misses() {
  let m = misses();
  assert!(m.is_empty(), "fixtures manquantes :\n{}", m.join("\n"));
}

const HELLO_FR: &str = r#"{"texts":["hello"],"targetLang":"fr"}"#;

#[tokio::test]
async fn translate_returns_the_recorded_translation() {
  let _on = translation_setting(true).await;
  let core = core_recorded();
  let (status, out) = json_of(translate::translate(&core, body(HELLO_FR)).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(out, json!({"success": true, "translations": ["Bonjour"]}));
}

#[tokio::test]
async fn translate_is_refused_when_the_user_has_not_opted_in() {
  let _off = translation_setting(false).await;
  let core = core_offline();
  let (status, out) = json_of(translate::translate(&core, body(HELLO_FR)).await);
  assert_eq!(status, 200);
  assert_eq!(out["success"], false);
  assert_eq!(out["error"], "translation disabled");
  assert!(misses().is_empty(), "rien n'est envoyé à un tiers");
}

#[tokio::test]
async fn translate_without_network_returns_the_original_lines() {
  let _on = translation_setting(true).await;
  let core = core_offline();
  let (status, out) =
    json_of(translate::translate(&core, body(r#"{"texts":["hello","world"],"to":"fr"}"#)).await);
  assert_eq!(status, 200);
  assert_eq!(
    out,
    json!({"success": true, "translations": ["hello", "world"]})
  );
  assert!(!misses().is_empty());
}

#[tokio::test]
async fn translate_http_500_returns_the_original_lines() {
  let _on = translation_setting(true).await;
  let core = core_hand_written(|dir| {
    write_fixture_with_body(
      dir,
      "POST",
      "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl=xx&dt=t",
      b"q=hello",
      500,
      "Internal Server Error",
      "text/plain",
    );
  });
  let (_, out) =
    json_of(translate::translate(&core, body(r#"{"texts":["hello"],"targetLang":"xx"}"#)).await);
  assert_eq!(out["translations"], json!(["hello"]));
  no_misses();
}

#[tokio::test]
async fn translate_rejects_an_oversized_body_and_ignores_empty_input() {
  let core = core_offline();
  let huge = "x".repeat(riff_core::core::MAX_BODY_BYTES + 1);
  let (status, out) = json_of(translate::translate(&core, body(&huge)).await);
  assert_eq!(
    (status, out["error"].as_str()),
    (413, Some("payload too large"))
  );
  let (status, out) = json_of(translate::translate(&core, body("{pas du json")).await);
  assert_eq!(
    (status, out),
    (200, json!({"success": true, "translations": []}))
  );
  assert!(misses().is_empty());
}

#[test]
fn cleanup_i18n_dir_removes_poisoned_dictionaries_only() {
  crate::common::root();
  let dir = riff_core::paths::resolve_data_dir().join("i18n");
  std::fs::create_dir_all(&dir).expect("dossier");
  std::fs::write(dir.join("zz_bad.json"), r#"{"a":"visit linux.do"}"#).expect("fichier");
  std::fs::write(dir.join("zz_good.json"), r#"{"a":"bonjour"}"#).expect("fichier");
  translate::cleanup_i18n_dir();
  assert!(!dir.join("zz_bad.json").exists());
  assert!(dir.join("zz_good.json").exists());
}

#[test]
fn cleanup_i18n_dir_skips_entries_it_cannot_read() {
  crate::common::root();
  let dir = riff_core::paths::resolve_data_dir().join("i18n");
  std::fs::create_dir_all(dir.join("zz_subdir.json")).expect("sous-dossier");
  std::fs::write(dir.join("zz_binary.json"), [0xff, 0xfe, 0x00]).expect("fichier");
  translate::cleanup_i18n_dir();
  assert!(dir.join("zz_subdir.json").is_dir());
  assert!(dir.join("zz_binary.json").exists());
}

#[tokio::test]
async fn i18n_batch_translates_missing_keys_from_the_recorded_service() {
  let _on = translation_setting(true).await;
  let core = core_recorded();
  let (status, out) = json_of(translate::i18n_batch(&core, body(HELLO_FR)).await);
  no_misses();
  assert_eq!(status, 200);
  assert_eq!(out, json!({"success": true, "translations": ["Bonjour"]}));
}

#[tokio::test]
async fn i18n_batch_without_network_keeps_the_source_text() {
  let _on = translation_setting(true).await;
  let core = core_offline();
  let (status, out) = json_of(
    translate::i18n_batch(
      &core,
      body(r#"{"texts":["untranslated line"],"target":"de"}"#),
    )
    .await,
  );
  assert_eq!(status, 200);
  assert_eq!(out["translations"], json!(["untranslated line"]));
  assert!(!misses().is_empty());
}

#[tokio::test]
async fn i18n_batch_rejects_an_oversized_body() {
  let core = core_offline();
  let huge = "x".repeat(riff_core::core::MAX_BODY_BYTES + 1);
  let (status, _) = json_of(translate::i18n_batch(&core, body(&huge)).await);
  assert_eq!(status, 413);
}
