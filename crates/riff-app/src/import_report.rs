//! Import depuis Riff : détection du premier lancement, rapport des clés de `storage.json`, secrets à reprendre.
//!
//! Logique pure : rien n'est écrit ici (ni fichier natif, ni coffre). Le rapport ne contient que des noms de clés.

use crate::settings::{LegacyStorage, NativeSettings, UI_SCALE_RANGE};
use crate::user_data::UserData;
use material_colors::color::Rgb;
use riff_core::secrets::SecretName;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

/// Jetons de Riff qui vivent dans le coffre, jamais dans `legacy` ni dans un fichier natif.
pub const SECRET_KEYS: [(&str, SecretName); 3] = [
  ("riff_lastfm_apisecret", SecretName::LastfmApiSecret),
  ("riff_lastfm_sk", SecretName::LastfmSession),
  ("riff_deepl_api_key", SecretName::DeeplKey),
];

/// Clé d'API Last.fm : reprise dans `lastfmApiKey` de la config du core (pas un secret du coffre).
pub const LASTFM_API_KEY: &str = "riff_lastfm_apikey";

/// Clés de Riff sans équivalent natif, avec la raison (tirée de `08-reglages.md` et `08b-options.md`).
/// Les raisons sont les chaînes anglaises que l'écran passera à `i18n`.
pub const NOT_PORTED: &[(&str, &str)] = &[
  ("riff_font", "The native font is fixed at build time"),
  (
    "riff_font_scale",
    "The native interface has no global text scale",
  ),
  ("riff_corner_radius", "Corner radii are set per widget"),
  ("riff_ui_alpha", "The native window is opaque"),
  ("riff_ui_alpha_auto", "The native window is opaque"),
  ("riff_ui_alpha_prev", "The native window is opaque"),
  ("riff_ui_alpha_light_auto", "The native window is opaque"),
  ("riff_ui_alpha_light_prev", "The native window is opaque"),
  (
    "riff_panel_blur",
    "Blurring what is behind a panel is not available",
  ),
  ("riff_icon_pack", "Icons are Unicode characters"),
  ("riff_app_icon", "No SVG rendering for the home button icon"),
  (
    "riff_wavy_progress",
    "The native progress bar has no wavy shape",
  ),
  ("riff_progress_knob", "The native progress bar has no knob"),
  (
    "riff_progress_bg",
    "The native progress bar has no canvas background",
  ),
  (
    "riff_no_hand_cursor",
    "Cursors are set per widget, not globally",
  ),
  (
    "riff_adv_classic_sidebar",
    "There is a single collapsible sidebar natively",
  ),
  (
    "riff_custom_css",
    "Custom CSS is not supported natively (deliberate difference)",
  ),
  (
    "riff_custom_css_backup",
    "Custom CSS is not supported natively (deliberate difference)",
  ),
  (
    "riff_m3_hero_covers",
    "Shaped covers in Now Playing and Karaoke are not ported",
  ),
  (
    "riff_home_pane",
    "The native home shows every section, without panes",
  ),
  (
    "riff_ignored_update_ver",
    "Updates are checked on click only, nothing to ignore",
  ),
  ("riff_adv_ui_alpha_auto", "The native window is opaque"),
  ("riff_adv_clip_fx", "No CSS effects to clip natively"),
  (
    "riff_adv_page_glow_gradients",
    "CSS page decoration has no native equivalent",
  ),
  (
    "riff_adv_my_wave_ambient",
    "CSS ambient light has no native equivalent",
  ),
  (
    "riff_adv_mesh_cover_color",
    "Extracting the cover colors for the mesh is not ported",
  ),
  (
    "devsize_vfx",
    "Visual effects are forced off in Riff and absent from the product",
  ),
  ("riff_ctx_tip_v2", "Tip counter of the web interface"),
  (
    "riff_gradient_tight",
    "Old name of the gradient tightness, read only when the new key is absent",
  ),
  ("riff_queue_panel_open", "Cleared by Riff at every launch"),
  (
    "riff_artist_meta_cache",
    "Cache replaced by an in-memory cache",
  ),
  ("riff_scale_rebased_v2", "Riff internal migration marker"),
  (
    "riff_defaults_v44_applied",
    "Riff internal migration marker",
  ),
  ("riff_defaults_v44_hue246", "Riff internal migration marker"),
  ("riff_tvk_blur_reset_v1", "Riff internal migration marker"),
  ("riff_stats", "Dead key, removed by Riff at launch"),
  (
    "riff_keyboard_shortcuts",
    "Dead key, removed by Riff at launch",
  ),
];

/// Clés toujours conservées dans `legacy`, même si un importeur les lit.
const KEPT_IN_LEGACY: &[&str] = &[
  "riff_ignored_update_ver",
  "riff_font",
  "riff_font_scale",
  "riff_corner_radius",
  "riff_ui_alpha",
  "riff_panel_blur",
  "riff_icon_pack",
  "riff_app_icon",
  "riff_wavy_progress",
  "riff_progress_knob",
  "riff_progress_bg",
  "riff_no_hand_cursor",
  "riff_custom_css",
  "riff_m3_hero_covers",
  "riff_home_pane",
];

/// Marqueurs de migration et clés mortes de Riff : jamais dans `legacy`.
const DROPPED: &[&str] = &[
  "riff_queue_panel_open",
  "riff_artist_meta_cache",
  "riff_scale_rebased_v2",
  "riff_defaults_v44_applied",
  "riff_defaults_v44_hue246",
  "riff_tvk_blur_reset_v1",
  "riff_stats",
  "riff_keyboard_shortcuts",
];

/// Clés dont la valeur est du JSON rangé dans une chaîne : illisible = valeur invalide.
const JSON_KEYS: &[&str] = &[
  "devsize_favorites",
  "devsize_history",
  "devsize_playlists",
  "devsize_local_files",
  "riff_local_files",
  "devsize_local_tracks",
  "riff_removed_local_paths",
  "riff_saved_albums",
  "riff_favorite_artists",
  "riff_unfollowed_artists",
  "riff_search_history_v2",
  "riff_daily_mixes",
  "riff_pinned_tracks",
  "riff_sidebar_pinned_tracks",
  "riff_sidebar_order",
  "riff_blocked_tracks",
  "riff_blocked_artists",
  "riff_track_renames",
  "riff_track_trims",
  "riff_wave_skips",
  "riff_mix_feedback",
  "riff_my_wave_cache",
  "riff_trending_cache",
  "riff_home_order",
  "riff_proxy_list",
  "riff_audio_settings",
  "riff_hq_settings",
  "devsize_custom_presets",
  "riff_mesh_cfg",
  "riff_tvk",
  "riff_track_art",
];

fn is_secret(key: &str) -> bool {
  SECRET_KEYS.iter().any(|(secret, _)| *secret == key)
}

fn is_cache(key: &str) -> bool {
  key.starts_with("riff_i18n_") || (key.starts_with("riff_") && key.ends_with("_cache"))
}

fn is_dropped(key: &str) -> bool {
  DROPPED.contains(&key) || is_cache(key)
}

/// Raison pour laquelle une clé n'a pas d'équivalent natif.
#[must_use]
pub fn not_ported_reason(key: &str) -> Option<&'static str> {
  if let Some((_, reason)) = NOT_PORTED.iter().find(|(name, _)| *name == key) {
    return Some(reason);
  }
  if key.starts_with("riff_i18n_") {
    Some("Translation cache, rebuilt natively")
  } else if is_cache(key) {
    Some("Cache, rebuilt on demand")
  } else {
    None
  }
}

/// Clés sans équivalent natif à conserver dans `NativeSettings::legacy`. À appeler après tous les importeurs :
/// une clé déjà lue par l'un d'eux a un équivalent natif. Exclus : secrets, clé d'API Last.fm, caches, marqueurs.
pub(crate) fn legacy_entries(storage: &LegacyStorage) -> Map<String, Value> {
  let read = storage.read_keys();
  storage
    .entries()
    .iter()
    .filter(|(key, _)| {
      let key = key.as_str();
      KEPT_IN_LEGACY.contains(&key)
        || (!read.contains(key) && !is_dropped(key) && !is_secret(key) && key != LASTFM_API_KEY)
    })
    .map(|(key, value)| (key.clone(), value.clone()))
    .collect()
}

/// Secret de Riff à reprendre dans le coffre. Le `Debug` masque la valeur.
#[derive(Clone, PartialEq, Eq)]
pub struct LegacySecret {
  pub name: SecretName,
  /// Nom de la clé de `storage.json`.
  pub key: &'static str,
  value: String,
}

impl LegacySecret {
  /// La valeur, pour l'écriture dans le coffre uniquement.
  #[must_use]
  pub fn value(&self) -> &str {
    &self.value
  }
}

impl fmt::Debug for LegacySecret {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("LegacySecret")
      .field("name", &self.name)
      .field("key", &self.key)
      .field("value", &"***")
      .finish()
  }
}

/// Secrets présents (texte non vide) dans `storage.json`. N'écrit rien nulle part.
#[must_use]
pub fn legacy_secrets(storage: &LegacyStorage) -> Vec<LegacySecret> {
  SECRET_KEYS
    .iter()
    .filter_map(|(key, name)| {
      let value = storage.text(key)?.trim();
      (!value.is_empty()).then(|| LegacySecret {
        name: *name,
        key,
        value: value.to_string(),
      })
    })
    .collect()
}

/// Clé d'API Last.fm de Riff, à reprendre dans `lastfmApiKey` de la config du core si elle n'en a pas.
#[must_use]
pub fn legacy_lastfm_api_key(storage: &LegacyStorage) -> Option<String> {
  let key = storage.text(LASTFM_API_KEY)?.trim();
  (!key.is_empty()).then(|| key.to_string())
}

/// Vrai si la valeur d'une clé lue ne donne pas ce que Riff voulait dire (le défaut natif est gardé).
/// Vérifié pour les clés à valeur contrôlée ; toute autre clé lue est comptée comme reprise si son type est le bon.
fn is_invalid(key: &str, value: &Value) -> bool {
  if JSON_KEYS.contains(&key) {
    return value
      .as_str()
      .is_some_and(|raw| serde_json::from_str::<Value>(raw).is_err());
  }
  let Some(text) = value.as_str() else {
    return true;
  };
  match key {
    "riff_theme_mode" => !matches!(text, "dark" | "light" | "auto"),
    "riff_theme_accent" => text.parse::<Rgb>().is_err(),
    "riff_theme_brightness" | "riff_theme_intensity" => {
      !text.trim().parse::<u8>().is_ok_and(|v| v <= 100)
    }
    "devsize_ui_scale" => !text
      .trim()
      .parse::<u32>()
      .is_ok_and(|v| (UI_SCALE_RANGE.0..=UI_SCALE_RANGE.1).contains(&v)),
    "riff_home_layout" => !matches!(text, "spotify" | "riff"),
    "riff_platform" => !matches!(text, "ytmusic" | "soundcloud"),
    "riff_volume" => !text.trim().parse::<f32>().is_ok_and(f32::is_finite),
    "riff_user_nickname" => text.trim().is_empty(),
    _ => false,
  }
}

/// État d'un secret dans le rapport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretState {
  /// Repris dans le coffre.
  Imported,
  /// Pas repris (coffre indisponible, ou pas encore écrit) : à ressaisir.
  ToReenter,
}

/// Un secret, par le nom de sa clé, jamais par sa valeur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretReport {
  pub key: &'static str,
  pub state: SecretState,
}

/// Une clé sans équivalent natif et sa raison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotPortedKey {
  pub key: String,
  pub reason: &'static str,
}

/// Les clés de `storage.json` rangées en quatre catégories, plus les secrets. Que des noms, jamais une valeur.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
  /// Reprises.
  pub imported: Vec<String>,
  /// Lues mais invalides : le défaut natif est gardé.
  pub invalid: Vec<String>,
  /// Sans équivalent natif, avec la raison.
  pub not_ported: Vec<NotPortedKey>,
  /// Ni lues ni connues.
  pub unknown: Vec<String>,
  /// Secrets présents dans `storage.json`, repris par défaut.
  pub secrets: Vec<SecretReport>,
}

impl ImportReport {
  /// Calcule le rapport : clés de `storage.json` moins les clés lues par les importeurs. Les importeurs sont lancés ici
  /// sur une copie, donc le résultat ne dépend pas de ce que l'appelant a déjà lu.
  #[must_use]
  pub fn new(storage: &LegacyStorage) -> Self {
    let view = storage.untracked_copy();
    let _ = NativeSettings::imported_from(&view);
    let _ = legacy_lastfm_api_key(&view);
    let read = view.read_keys();
    let mut report = Self::default();
    let keys: BTreeSet<&String> = view.entries().keys().collect();
    for key in keys {
      let key = key.as_str();
      if is_secret(key) {
        continue;
      }
      if read.contains(key) {
        let value = &view.entries()[key];
        if is_invalid(key, value) {
          report.invalid.push(key.to_string());
        } else {
          report.imported.push(key.to_string());
        }
      } else if let Some(reason) = not_ported_reason(key) {
        report.not_ported.push(NotPortedKey {
          key: key.to_string(),
          reason,
        });
      } else {
        report.unknown.push(key.to_string());
      }
    }
    report.secrets = legacy_secrets(&view)
      .iter()
      .map(|secret| SecretReport {
        key: secret.key,
        state: SecretState::Imported,
      })
      .collect();
    report
  }

  /// Le coffre n'a rien pu écrire : tous les secrets sont à ressaisir.
  pub fn mark_secrets_to_reenter(&mut self) {
    for secret in &mut self.secrets {
      secret.state = SecretState::ToReenter;
    }
  }

  /// Un seul secret est à ressaisir (échec d'écriture isolé). Sans effet si la clé est absente du rapport.
  pub fn mark_secret_to_reenter(&mut self, name: SecretName) {
    for secret in &mut self.secrets {
      if SECRET_KEYS
        .iter()
        .any(|(key, secret_name)| *key == secret.key && *secret_name == name)
      {
        secret.state = SecretState::ToReenter;
      }
    }
  }
}

/// Ce que l'écran d'import annonce : combien de données et de réglages sont trouvés dans `storage.json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportOffer {
  pub favorites: usize,
  pub playlists: usize,
  pub local_files: usize,
  /// Clés de réglages : ni données, ni secrets, ni caches ou marqueurs.
  pub settings: usize,
}

impl ImportOffer {
  /// Compte ce que `storage.json` apporte.
  #[must_use]
  pub fn count(storage: &LegacyStorage) -> Self {
    let view = storage.untracked_copy();
    let data = UserData::imported_from(&view, 0);
    let data_keys = view.read_keys();
    let settings = storage
      .entries()
      .keys()
      .filter(|key| {
        let key = key.as_str();
        !data_keys.contains(key) && !is_secret(key) && !is_dropped(key)
      })
      .count();
    Self {
      favorites: data.favorites.len(),
      playlists: data.playlists.len(),
      local_files: data.local_files.len(),
      settings,
    }
  }
}

fn is_absent(path: &Path) -> bool {
  matches!(
    std::fs::metadata(path),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound
  )
}

/// Premier lancement : `native-settings.json` et `user-data.json` absents ET `storage.json` avec au moins une clé.
/// Un fichier natif présent, même corrompu, ou illisible pour une autre raison qu'une absence : pas d'écran.
/// Ne lit, n'écrit ni ne déplace aucun fichier natif.
#[must_use]
pub fn import_offer(
  settings_path: &Path,
  user_data_path: &Path,
  storage: &LegacyStorage,
) -> Option<ImportOffer> {
  (is_absent(settings_path) && is_absent(user_data_path) && !storage.is_empty())
    .then(|| ImportOffer::count(storage))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;
  use serde_json::json;
  use std::fs;

  fn temp_dir() -> TempDir {
    TempDir::new("app-import-report")
  }

  fn storage_of(content: &str) -> LegacyStorage {
    let dir = temp_dir();
    let path = dir.join("storage.json");
    fs::write(&path, content).unwrap();
    LegacyStorage::open(&path)
  }

  const STORAGE: &str = r#"{
    "devsize_favorites": "[{\"id\":\"a\",\"title\":\"A\"},{\"id\":\"b\",\"title\":\"B\"}]",
    "devsize_playlists": "[{\"id\":\"p\",\"name\":\"P\",\"tracks\":[]}]",
    "riff_local_files": "[{\"id\":\"l\",\"title\":\"L\",\"path\":\"C:/x.mp3\"}]",
    "riff_ui_lang": "uk",
    "riff_font": "VALEUR-FONT"
  }"#;

  #[test]
  fn the_offer_needs_both_native_files_absent_and_a_non_empty_storage() {
    let dir = temp_dir();
    let offer = import_offer(
      &dir.join("native-settings.json"),
      &dir.join("user-data.json"),
      &storage_of(STORAGE),
    );
    assert_eq!(
      offer,
      Some(ImportOffer {
        favorites: 2,
        playlists: 1,
        local_files: 1,
        settings: 2,
      })
    );
  }

  #[test]
  fn user_data_alone_means_no_offer_and_the_file_stays_intact() {
    let dir = temp_dir();
    let user_data = dir.join("user-data.json");
    fs::write(&user_data, "{}").unwrap();
    assert_eq!(
      import_offer(
        &dir.join("native-settings.json"),
        &user_data,
        &storage_of(STORAGE)
      ),
      None
    );
    assert_eq!(fs::read_to_string(&user_data).unwrap(), "{}");
  }

  #[test]
  fn settings_alone_means_no_offer() {
    let dir = temp_dir();
    let settings = dir.join("native-settings.json");
    fs::write(&settings, "{}").unwrap();
    assert_eq!(
      import_offer(&settings, &dir.join("user-data.json"), &storage_of(STORAGE)),
      None
    );
  }

  #[test]
  fn a_missing_empty_or_unreadable_storage_means_no_offer() {
    let dir = temp_dir();
    let (settings, user_data) = (dir.join("native-settings.json"), dir.join("user-data.json"));
    for storage in [
      LegacyStorage::open(&dir.join("absent.json")),
      storage_of("{}"),
      storage_of("{ pas du json"),
      storage_of("[1, 2]"),
    ] {
      assert_eq!(import_offer(&settings, &user_data, &storage), None);
    }
  }

  #[test]
  fn a_corrupt_native_file_means_no_offer() {
    let dir = temp_dir();
    let settings = dir.join("native-settings.json");
    fs::write(&settings, "{ cassé").unwrap();
    assert_eq!(
      import_offer(&settings, &dir.join("user-data.json"), &storage_of(STORAGE)),
      None
    );
    assert_eq!(fs::read_to_string(&settings).unwrap(), "{ cassé");
    assert!(!settings.with_extension("json.corrupt").exists());
  }

  #[test]
  fn the_offer_writes_nothing() {
    let dir = temp_dir();
    let _ = import_offer(
      &dir.join("native-settings.json"),
      &dir.join("user-data.json"),
      &storage_of(STORAGE),
    );
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
  }

  const REPORTED: &str = r#"{
    "riff_ui_lang": "VALEUR-LANG",
    "riff_theme_brightness": "VALEUR-BRIGHT",
    "riff_theme_mode": "VALEUR-MODE",
    "riff_tr_lang": 12,
    "riff_font": "VALEUR-FONT",
    "riff_queue_panel_open": "VALEUR-QUEUE",
    "riff_i18n_fr": "VALEUR-I18N",
    "riff_cle_inconnue": "VALEUR-INCONNUE",
    "riff_win_global_media_keys": "false",
    "riff_lastfm_apikey": "VALEUR-APIKEY",
    "riff_lastfm_apisecret": "VALEUR-SECRET-1",
    "riff_lastfm_sk": "VALEUR-SECRET-2",
    "riff_deepl_api_key": "VALEUR-SECRET-3",
    "devsize_favorites": "[]"
  }"#;

  #[test]
  fn the_report_has_four_categories_and_no_value() {
    let report = ImportReport::new(&storage_of(REPORTED));
    assert_eq!(
      report.imported,
      [
        "devsize_favorites",
        "riff_lastfm_apikey",
        "riff_ui_lang",
        "riff_win_global_media_keys"
      ]
    );
    assert_eq!(
      report.invalid,
      ["riff_theme_brightness", "riff_theme_mode", "riff_tr_lang"]
    );
    assert_eq!(
      report
        .not_ported
        .iter()
        .map(|entry| entry.key.as_str())
        .collect::<Vec<_>>(),
      ["riff_font", "riff_i18n_fr", "riff_queue_panel_open"]
    );
    assert!(
      report
        .not_ported
        .iter()
        .all(|entry| !entry.reason.is_empty())
    );
    assert_eq!(report.unknown, ["riff_cle_inconnue"]);
    let shown = format!("{report:?}");
    assert!(!shown.contains("VALEUR"), "{shown}");
  }

  #[test]
  fn the_queue_panel_reason_is_the_one_of_riff() {
    let report = ImportReport::new(&storage_of(REPORTED));
    let reason = report
      .not_ported
      .iter()
      .find(|entry| entry.key == "riff_queue_panel_open")
      .map(|entry| entry.reason);
    assert_eq!(reason, Some("Cleared by Riff at every launch"));
  }

  #[test]
  fn the_report_names_the_secrets_with_a_state_and_never_their_value() {
    let mut report = ImportReport::new(&storage_of(REPORTED));
    assert_eq!(
      report.secrets,
      [
        SecretReport {
          key: "riff_lastfm_apisecret",
          state: SecretState::Imported
        },
        SecretReport {
          key: "riff_lastfm_sk",
          state: SecretState::Imported
        },
        SecretReport {
          key: "riff_deepl_api_key",
          state: SecretState::Imported
        },
      ]
    );
    report.mark_secret_to_reenter(SecretName::LastfmSession);
    assert_eq!(report.secrets[1].state, SecretState::ToReenter);
    assert_eq!(report.secrets[0].state, SecretState::Imported);
    report.mark_secrets_to_reenter();
    assert!(
      report
        .secrets
        .iter()
        .all(|secret| secret.state == SecretState::ToReenter)
    );
    assert!(!format!("{report:?}").contains("VALEUR"));
    for category in [&report.imported, &report.invalid, &report.unknown] {
      assert!(category.iter().all(|key| !key.contains("secret")));
    }
  }

  #[test]
  fn the_report_does_not_depend_on_what_the_caller_already_read() {
    let storage = storage_of(REPORTED);
    let before = ImportReport::new(&storage);
    let _ = NativeSettings::imported_from(&storage);
    assert_eq!(ImportReport::new(&storage), before);
    assert_eq!(storage, storage_of(REPORTED));
  }

  #[test]
  fn the_secrets_are_extracted_without_being_written_anywhere() {
    let storage = storage_of(REPORTED);
    let secrets = legacy_secrets(&storage);
    assert_eq!(
      secrets
        .iter()
        .map(|secret| (secret.name, secret.value()))
        .collect::<Vec<_>>(),
      [
        (SecretName::LastfmApiSecret, "VALEUR-SECRET-1"),
        (SecretName::LastfmSession, "VALEUR-SECRET-2"),
        (SecretName::DeeplKey, "VALEUR-SECRET-3"),
      ]
    );
  }

  #[test]
  fn the_debug_of_a_secret_masks_its_value() {
    let secrets = legacy_secrets(&storage_of(REPORTED));
    let shown = format!("{secrets:?}");
    assert!(!shown.contains("VALEUR"), "{shown}");
    assert!(shown.contains("***"));
    assert!(shown.contains("LastfmSession"));
  }

  #[test]
  fn empty_secrets_are_ignored() {
    let storage = storage_of(r#"{ "riff_lastfm_sk": "  ", "riff_deepl_api_key": 5 }"#);
    assert!(legacy_secrets(&storage).is_empty());
    assert!(ImportReport::new(&storage).secrets.is_empty());
  }

  #[test]
  fn the_lastfm_api_key_is_exposed_trimmed() {
    assert_eq!(
      legacy_lastfm_api_key(&storage_of(r#"{ "riff_lastfm_apikey": " abc123 " }"#)),
      Some("abc123".to_string())
    );
    assert_eq!(
      legacy_lastfm_api_key(&storage_of(r#"{ "riff_lastfm_apikey": "" }"#)),
      None
    );
    assert_eq!(legacy_lastfm_api_key(&storage_of("{}")), None);
  }

  #[test]
  fn no_secret_or_api_key_reaches_legacy_or_the_serialized_settings() {
    let settings = NativeSettings::imported_from(&storage_of(REPORTED));
    let serialized = serde_json::to_string_pretty(&settings).unwrap();
    assert!(!serialized.contains("VALEUR-SECRET"));
    assert!(!serialized.contains("VALEUR-APIKEY"));
    assert_eq!(
      settings.legacy.keys().collect::<Vec<_>>(),
      ["riff_font", "riff_cle_inconnue"]
    );
    assert!(!settings.media_session);
    assert_eq!(
      settings.legacy.get("riff_font"),
      Some(&json!("VALEUR-FONT"))
    );
  }

  #[test]
  fn every_reason_of_the_table_is_filled_and_keys_are_unique() {
    let mut seen = BTreeSet::new();
    for (key, reason) in NOT_PORTED {
      assert!(seen.insert(*key), "{key} en double");
      assert!(!reason.is_empty(), "{key}");
    }
  }
}
