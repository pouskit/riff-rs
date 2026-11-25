//! Écran d'import au premier lancement : état d'attente, choix de l'utilisateur, écritures sur disque.
//!
//! Tant qu'un choix est attendu, ni `native-settings.json` ni `user-data.json` ne sont écrits
//! ([`App::suspend_writes`]) et rien ne part sur le réseau (le démarrage attend, [`App::held_startup`]).
//! `storage.json` n'est jamais écrit.

use crate::app::{App, Effect};
use crate::import_report::{
  ImportOffer, ImportReport, LegacySecret, import_offer, legacy_lastfm_api_key, legacy_secrets,
};
use crate::settings::{ImportSource, LegacyStorage, NativeSettings};
use crate::ui::Modal;
use crate::user_data::UserData;
use crate::widgets::toast::ToastKind;
use riff_core::secrets::SecretStore;
use serde_json::Value;
use std::path::Path;

/// Choix de l'écran d'import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportChoice {
  /// « Import from Riff ».
  Riff,
  /// « Start fresh ».
  Fresh,
}

/// Premier lancement en attente d'un choix. Garde `storage.json` en mémoire pour l'import.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingImport {
  pub offer: ImportOffer,
  pub storage: LegacyStorage,
  /// Un choix est parti, son résultat n'est pas revenu.
  pub working: bool,
}

/// État de l'écran d'import dans [`App`].
#[derive(Debug, Clone, Default)]
pub struct ImportState {
  pub pending: Option<PendingImport>,
  /// La fenêtre s'est ouverte pendant l'attente : le démarrage reste à lancer après le choix.
  startup_held: bool,
}

/// Réglages et données du lancement, avec l'offre d'import s'il y en a une.
#[derive(Debug)]
pub struct Startup {
  pub settings: NativeSettings,
  pub data: UserData,
  pub pending: Option<PendingImport>,
}

/// Décide du démarrage. Avec une offre : réglages et données de Riff en mémoire, AUCUN fichier écrit.
/// Sans offre : comportement d'avant (`load_or_import`, qui écrit les fichiers absents et met le corrompu de côté).
#[must_use]
pub fn startup_state(
  settings_path: &Path,
  user_data_path: &Path,
  storage: LegacyStorage,
  now_ms: i64,
) -> Startup {
  match import_offer(settings_path, user_data_path, &storage) {
    Some(offer) => Startup {
      settings: NativeSettings::imported_from(&storage),
      data: UserData::imported_from(&storage, now_ms),
      pending: Some(PendingImport {
        offer,
        storage,
        working: false,
      }),
    },
    None => {
      let settings = NativeSettings::load_or_import(settings_path, &storage);
      // L'ordre compte : `write_files` écrit les réglages avant les données, donc `fresh` est lu ici d'abord.
      let data = if settings.import_source == ImportSource::Fresh {
        UserData::load_or_import(user_data_path, &LegacyStorage::default(), now_ms)
      } else {
        UserData::load_or_import(user_data_path, &storage, now_ms)
      };
      Startup {
        settings,
        data,
        pending: None,
      }
    }
  }
}

/// Résultat d'un choix, rendu par la tâche d'écriture.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOutcome {
  pub choice: ImportChoice,
  pub settings: NativeSettings,
  pub data: UserData,
  /// Rapport de l'import (jamais pour « Start fresh »).
  pub report: Option<ImportReport>,
  /// Clé d'API Last.fm de Riff, à reprendre dans la config du core si elle n'en a pas (`with_lastfm_key`).
  pub lastfm_api_key: Option<String>,
}

/// Messages de l'écran d'import.
#[derive(Debug, Clone)]
pub enum ImportMessage {
  Choose(ImportChoice),
  Done(Box<ImportOutcome>),
  /// La tâche d'écriture a planté : l'écran reste, un nouveau choix est possible.
  Failed,
}

/// Écrit les secrets dans le coffre. Un échec laisse le secret non écrit et le marque « à ressaisir ».
pub fn store_secrets(store: &dyn SecretStore, secrets: &[LegacySecret], report: &mut ImportReport) {
  for secret in secrets {
    if let Err(err) = store.set(secret.name, secret.value()) {
      tracing::warn!(error = %err, secret = secret.key, "secret de Riff non repris");
      report.mark_secret_to_reenter(secret.name);
    }
  }
}

/// Config du core avec la clé Last.fm de Riff. `None` : la config en a déjà une, ou la clé est vide.
#[must_use]
pub fn with_lastfm_key(config: &Value, key: &str) -> Option<Value> {
  let key = key.trim();
  let present = config
    .get("lastfmApiKey")
    .and_then(Value::as_str)
    .is_some_and(|current| !current.trim().is_empty());
  if key.is_empty() || present {
    return None;
  }
  let mut merged = config.as_object().cloned().unwrap_or_default();
  merged.insert("lastfmApiKey".to_string(), Value::String(key.to_string()));
  Some(Value::Object(merged))
}

fn write_files(
  settings: &NativeSettings,
  data: &UserData,
  settings_path: &Path,
  user_data_path: &Path,
) {
  if let Err(err) = settings.save(settings_path) {
    tracing::warn!(error = %err, "écriture de native-settings.json");
  }
  if let Err(err) = data.save(user_data_path) {
    tracing::warn!(error = %err, "écriture de user-data.json");
  }
}

/// Exécute un choix : écrit les deux fichiers natifs (et les secrets pour l'import). Ne touche pas à `storage.json`.
#[must_use]
pub fn run_choice(
  choice: ImportChoice,
  storage: &LegacyStorage,
  settings_path: &Path,
  user_data_path: &Path,
  secrets: &dyn SecretStore,
  now_ms: i64,
) -> ImportOutcome {
  match choice {
    ImportChoice::Fresh => {
      let settings = NativeSettings::fresh();
      let data = UserData::default();
      write_files(&settings, &data, settings_path, user_data_path);
      ImportOutcome {
        choice,
        settings,
        data,
        report: None,
        lastfm_api_key: None,
      }
    }
    ImportChoice::Riff => {
      let settings = NativeSettings::imported_from(storage);
      let data = UserData::imported_from(storage, now_ms);
      let mut report = ImportReport::new(storage);
      // Les secrets d'abord : un arrêt avant l'écriture des fichiers ramène l'écran, sans perdre de jeton.
      store_secrets(secrets, &legacy_secrets(storage), &mut report);
      write_files(&settings, &data, settings_path, user_data_path);
      ImportOutcome {
        choice,
        settings,
        data,
        report: Some(report),
        lastfm_api_key: legacy_lastfm_api_key(storage),
      }
    }
  }
}

impl App {
  /// Donne l'offre d'import à l'application.
  #[must_use]
  pub fn with_import(mut self, pending: Option<PendingImport>) -> Self {
    self.import.pending = pending;
    self
  }

  /// Vrai tant que l'écran d'import attend un choix.
  #[must_use]
  pub const fn import_waiting(&self) -> bool {
    self.import.pending.is_some()
  }

  /// Point de passage unique des écritures : sans choix, ni réglages ni données ne partent sur disque.
  /// L'état de la fenêtre s'écrit toujours.
  pub(crate) fn suspend_writes(&self, effects: Vec<Effect>) -> Vec<Effect> {
    if !self.import_waiting() {
      return effects;
    }
    effects
      .into_iter()
      .filter_map(|effect| match effect {
        Effect::SaveSettings(_)
        | Effect::ScheduleSave(_)
        | Effect::WriteUserData(_)
        | Effect::SaveResume(_)
        | Effect::SaveResumePosition(_) => None,
        Effect::SaveWindowAndExit(state, _) => Some(Effect::SaveWindowAndExit(state, None)),
        other => Some(other),
      })
      .collect()
  }

  /// Fin du démarrage : tout ce qui lit les réglages ou les données, ou qui peut toucher le réseau.
  fn held_startup(&mut self) -> Vec<Effect> {
    self.resume.startup_done = true;
    let mut effects = vec![
      Effect::AudioMix(self.data.volume, self.data.muted),
      Effect::UpdateYtDlp,
    ];
    effects.extend(self.startup_sound());
    effects.extend(self.startup_proxy());
    effects.extend(self.start_home());
    // Après le premier écran : la traduction réseau éventuelle ne part jamais avant.
    if self.settings.ui_lang != "en" {
      effects.push(Effect::LoadLanguage(self.settings.ui_lang.clone()));
    }
    // Après le son et le périphérique : la reprise de la lecture du lancement.
    effects.extend(self.resume_start());
    effects
  }

  /// La fenêtre s'ouvre : le démarrage part, ou attend le choix de l'écran d'import.
  pub(crate) fn start_or_hold(&mut self) -> Vec<Effect> {
    // Déjà parti (secours de la reprise avant l'ouverture de la fenêtre) : une seule fois.
    if self.resume.startup_done {
      Vec::new()
    } else if self.import_waiting() {
      self.import.startup_held = true;
      Vec::new()
    } else {
      self.held_startup()
    }
  }

  /// Messages de l'écran d'import.
  pub(crate) fn update_import(&mut self, message: ImportMessage) -> Vec<Effect> {
    match message {
      ImportMessage::Choose(choice) => match &mut self.import.pending {
        Some(pending) if !pending.working => {
          pending.working = true;
          vec![Effect::ImportChoice(
            choice,
            Box::new(pending.storage.clone()),
          )]
        }
        _ => Vec::new(),
      },
      ImportMessage::Done(outcome) => self.import_done(*outcome),
      ImportMessage::Failed => {
        if let Some(pending) = &mut self.import.pending {
          pending.working = false;
        }
        vec![Effect::Toast(
          "Could not save the setting.".to_string(),
          ToastKind::Error,
        )]
      }
    }
  }

  fn import_done(&mut self, outcome: ImportOutcome) -> Vec<Effect> {
    if self.import.pending.take().is_none() {
      return Vec::new();
    }
    let keep_window = self.settings.screen;
    self.settings = outcome.settings;
    self.settings.screen = keep_window;
    crate::motion::apply_settings(self.settings.reduce_motion, self.settings.perf_mode);
    self.data = outcome.data;
    self.player.shuffle = self.data.shuffle;
    self.player.scope = None;
    self.player.repeat = self.data.repeat;
    self.player.crossfade = self.data.crossfade;
    self.refresh_theme();
    self.refresh_orders();
    let mut effects = if std::mem::take(&mut self.import.startup_held) {
      self.held_startup()
    } else {
      Vec::new()
    };
    effects.push(self.main_level_effect());
    let language_sent = effects
      .iter()
      .any(|effect| matches!(effect, Effect::LoadLanguage(_)));
    if !language_sent && self.i18n.lang() != self.settings.ui_lang {
      effects.push(Effect::LoadLanguage(self.settings.ui_lang.clone()));
    }
    if let Some(report) = outcome.report {
      effects.extend(self.open_modal(Modal::ImportReport(Box::new(report))));
    }
    effects
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{App, Effect, Message};
  use crate::i18n::I18n;
  use crate::import_report::SecretState;
  use crate::test_temp::TempDir;
  use crate::ui::{Modal, UiMessage};
  use crate::user_data::UserData;
  use crate::window::PersistedWindowState;
  use riff_core::secrets::{MemoryStore, SecretError, SecretName, SecretStore};
  use serde_json::json;
  use std::fs;
  use std::path::{Path, PathBuf};

  fn temp_dir() -> TempDir {
    TempDir::new("app-import-flow")
  }

  const STORAGE: &str = r#"{
    "devsize_favorites": "[{\"id\":\"a\",\"title\":\"A\"},{\"id\":\"b\",\"title\":\"B\"}]",
    "devsize_playlists": "[{\"id\":\"p\",\"name\":\"P\",\"tracks\":[]}]",
    "riff_ui_lang": "uk",
    "riff_font": "VALEUR-FONT",
    "riff_win_global_media_keys": "false",
    "riff_lastfm_apikey": "VALEUR-APIKEY",
    "riff_lastfm_apisecret": "VALEUR-SECRET-1",
    "riff_lastfm_sk": "VALEUR-SECRET-2",
    "riff_deepl_api_key": "VALEUR-SECRET-3"
  }"#;

  struct Dirs {
    _dir: TempDir,
    storage: PathBuf,
    settings: PathBuf,
    data: PathBuf,
  }

  fn dirs(content: &str) -> Dirs {
    let dir = temp_dir();
    let storage = dir.join("storage.json");
    fs::write(&storage, content).unwrap();
    Dirs {
      storage,
      settings: dir.join("native-settings.json"),
      data: dir.join("user-data.json"),
      _dir: dir,
    }
  }

  fn files_in(path: &Path) -> usize {
    fs::read_dir(path.parent().unwrap()).unwrap().count()
  }

  #[derive(Debug)]
  struct DownStore;

  impl SecretStore for DownStore {
    fn get(&self, _: SecretName) -> Result<Option<String>, SecretError> {
      Err(SecretError("down".into()))
    }
    fn set(&self, _: SecretName, _: &str) -> Result<(), SecretError> {
      Err(SecretError("down".into()))
    }
    fn delete(&self, _: SecretName) -> Result<(), SecretError> {
      Err(SecretError("down".into()))
    }
  }

  fn waiting_app() -> App {
    let dirs = dirs(STORAGE);
    let startup = startup_state(
      &dirs.settings,
      &dirs.data,
      LegacyStorage::open(&dirs.storage),
      0,
    );
    App::new(
      PersistedWindowState::default(),
      startup.settings,
      I18n::default(),
    )
    .with_data(startup.data)
    .with_import(startup.pending)
  }

  fn open(app: &mut App) -> Vec<Effect> {
    app.update(Message::WindowOpened {
      position: None,
      size: iced::Size::new(800.0, 600.0),
    })
  }

  fn choose(app: &mut App, choice: ImportChoice) -> Vec<Effect> {
    app.update(Message::Import(ImportMessage::Choose(choice)))
  }

  fn outcome_of(app: &App, choice: ImportChoice, store: &dyn SecretStore) -> ImportOutcome {
    let pending = app.import.pending.as_ref().unwrap();
    let dirs = dirs("{}");
    run_choice(
      choice,
      &pending.storage,
      &dirs.settings,
      &dirs.data,
      store,
      0,
    )
  }

  // Déclencheur.

  #[test]
  fn an_offer_starts_in_memory_and_writes_nothing() {
    let dirs = dirs(STORAGE);
    let startup = startup_state(
      &dirs.settings,
      &dirs.data,
      LegacyStorage::open(&dirs.storage),
      0,
    );
    let pending = startup.pending.expect("offre");
    assert_eq!(pending.offer.favorites, 2);
    assert_eq!(pending.offer.playlists, 1);
    assert_eq!(startup.settings.ui_lang, "uk");
    assert_eq!(startup.data.favorites.len(), 2);
    assert_eq!(files_in(&dirs.storage), 1, "seul storage.json existe");
  }

  #[test]
  fn user_data_alone_gives_no_screen_and_the_file_stays_intact() {
    let dirs = dirs(STORAGE);
    fs::write(&dirs.data, "{}").unwrap();
    let startup = startup_state(
      &dirs.settings,
      &dirs.data,
      LegacyStorage::open(&dirs.storage),
      0,
    );
    assert!(startup.pending.is_none());
    assert_eq!(fs::read_to_string(&dirs.data).unwrap(), "{}");
    assert!(startup.data.favorites.is_empty());
  }

  #[test]
  fn an_absent_storage_gives_no_screen() {
    let dirs = dirs(STORAGE);
    let startup = startup_state(
      &dirs.settings,
      &dirs.data,
      LegacyStorage::open(&dirs.storage.with_file_name("absent.json")),
      0,
    );
    assert!(startup.pending.is_none());
  }

  #[test]
  fn a_corrupt_settings_file_gives_no_screen_and_keeps_the_silent_import() {
    let dirs = dirs(STORAGE);
    fs::write(&dirs.settings, "{ cassé").unwrap();
    let startup = startup_state(
      &dirs.settings,
      &dirs.data,
      LegacyStorage::open(&dirs.storage),
      0,
    );
    assert!(startup.pending.is_none());
    assert_eq!(startup.settings.ui_lang, "uk");
    assert!(dirs.settings.with_extension("json.corrupt").exists());
  }

  // Choix.

  #[test]
  fn start_fresh_writes_the_defaults_and_imports_nothing() {
    let dirs = dirs(STORAGE);
    let storage = LegacyStorage::open(&dirs.storage);
    let outcome = run_choice(
      ImportChoice::Fresh,
      &storage,
      &dirs.settings,
      &dirs.data,
      &MemoryStore::default(),
      0,
    );
    assert_eq!(outcome.settings, NativeSettings::fresh());
    assert_eq!(outcome.settings.import_source, ImportSource::Fresh);
    assert_eq!(outcome.data, UserData::default());
    assert!(outcome.report.is_none());
    assert!(outcome.lastfm_api_key.is_none());
    let written = fs::read_to_string(&dirs.settings).unwrap();
    assert!(written.contains("\"importSource\": \"fresh\""), "{written}");
    assert!(dirs.data.exists());
  }

  #[test]
  fn after_start_fresh_the_next_launch_reads_nothing_from_storage() {
    let dirs = dirs(STORAGE);
    let storage = LegacyStorage::open(&dirs.storage);
    let _ = run_choice(
      ImportChoice::Fresh,
      &storage,
      &dirs.settings,
      &dirs.data,
      &MemoryStore::default(),
      0,
    );
    let again = LegacyStorage::open(&dirs.storage);
    assert!(import_offer_of(&dirs, &again).is_none(), "plus d'écran");
    let settings = NativeSettings::load_or_import(&dirs.settings, &again);
    let data = UserData::load_or_import(&dirs.data, &again, 0);
    assert_eq!(settings.ui_lang, NativeSettings::default().ui_lang);
    assert!(settings.media_session, "touches média : défaut natif");
    assert!(settings.legacy.is_empty());
    assert!(data.favorites.is_empty());
    assert!(data.playlists.is_empty());
  }

  #[test]
  fn fresh_settings_without_user_data_import_nothing_from_storage() {
    let dirs = dirs(STORAGE);
    fs::write(&dirs.settings, r#"{"importSource":"fresh"}"#).unwrap();
    let before = fs::read(&dirs.storage).unwrap();
    let startup = startup_state(
      &dirs.settings,
      &dirs.data,
      LegacyStorage::open(&dirs.storage),
      0,
    );
    assert!(startup.pending.is_none());
    assert_eq!(startup.settings.import_source, ImportSource::Fresh);
    assert!(startup.data.favorites.is_empty());
    assert!(startup.data.playlists.is_empty());
    assert_eq!(fs::read(&dirs.storage).unwrap(), before);
  }

  #[test]
  fn riff_settings_without_user_data_still_import_the_data() {
    let dirs = dirs(STORAGE);
    fs::write(&dirs.settings, r#"{"importSource":"riff"}"#).unwrap();
    let startup = startup_state(
      &dirs.settings,
      &dirs.data,
      LegacyStorage::open(&dirs.storage),
      0,
    );
    assert!(startup.pending.is_none());
    assert_eq!(startup.data.favorites.len(), 2);
    assert_eq!(startup.data.playlists.len(), 1);
  }

  fn import_offer_of(
    dirs: &Dirs,
    storage: &LegacyStorage,
  ) -> Option<crate::import_report::ImportOffer> {
    crate::import_report::import_offer(&dirs.settings, &dirs.data, storage)
  }

  #[test]
  fn import_applies_the_data_and_writes_both_files() {
    let dirs = dirs(STORAGE);
    let storage = LegacyStorage::open(&dirs.storage);
    let outcome = run_choice(
      ImportChoice::Riff,
      &storage,
      &dirs.settings,
      &dirs.data,
      &MemoryStore::default(),
      0,
    );
    assert_eq!(outcome.settings.ui_lang, "uk");
    assert_eq!(outcome.data.favorites.len(), 2);
    assert_eq!(outcome.data.playlists.len(), 1);
    assert!(outcome.report.is_some());
    let saved: NativeSettings =
      serde_json::from_str(&fs::read_to_string(&dirs.settings).unwrap()).unwrap();
    assert_eq!(saved.ui_lang, "uk");
    let saved: UserData = serde_json::from_str(&fs::read_to_string(&dirs.data).unwrap()).unwrap();
    assert_eq!(saved.favorites.len(), 2);
  }

  #[test]
  fn import_takes_the_media_keys_and_exposes_the_lastfm_key() {
    let dirs = dirs(STORAGE);
    let outcome = run_choice(
      ImportChoice::Riff,
      &LegacyStorage::open(&dirs.storage),
      &dirs.settings,
      &dirs.data,
      &MemoryStore::default(),
      0,
    );
    assert!(!outcome.settings.media_session);
    assert_eq!(outcome.lastfm_api_key.as_deref(), Some("VALEUR-APIKEY"));
  }

  #[test]
  fn the_lastfm_key_never_overwrites_the_one_of_the_config() {
    assert_eq!(
      with_lastfm_key(&json!({}), "abc"),
      Some(json!({ "lastfmApiKey": "abc" }))
    );
    assert_eq!(
      with_lastfm_key(&json!({ "lastfmApiKey": "  ", "autre": 1 }), "abc"),
      Some(json!({ "lastfmApiKey": "abc", "autre": 1 }))
    );
    assert_eq!(
      with_lastfm_key(&json!({ "lastfmApiKey": "mienne" }), "abc"),
      None
    );
    assert_eq!(with_lastfm_key(&json!({}), "  "), None);
  }

  #[test]
  fn the_secrets_go_to_the_store_and_the_report_says_so() {
    let dirs = dirs(STORAGE);
    let store = MemoryStore::default();
    let outcome = run_choice(
      ImportChoice::Riff,
      &LegacyStorage::open(&dirs.storage),
      &dirs.settings,
      &dirs.data,
      &store,
      0,
    );
    assert_eq!(
      store.get(SecretName::LastfmApiSecret).unwrap().as_deref(),
      Some("VALEUR-SECRET-1")
    );
    assert_eq!(
      store.get(SecretName::LastfmSession).unwrap().as_deref(),
      Some("VALEUR-SECRET-2")
    );
    assert_eq!(
      store.get(SecretName::DeeplKey).unwrap().as_deref(),
      Some("VALEUR-SECRET-3")
    );
    let report = outcome.report.unwrap();
    assert_eq!(report.secrets.len(), 3);
    assert!(
      report
        .secrets
        .iter()
        .all(|secret| secret.state == SecretState::Imported)
    );
  }

  #[test]
  fn an_unavailable_store_writes_nothing_and_asks_to_reenter() {
    let dirs = dirs(STORAGE);
    let outcome = run_choice(
      ImportChoice::Riff,
      &LegacyStorage::open(&dirs.storage),
      &dirs.settings,
      &dirs.data,
      &DownStore,
      0,
    );
    let report = outcome.report.unwrap();
    assert_eq!(report.secrets.len(), 3);
    assert!(
      report
        .secrets
        .iter()
        .all(|secret| secret.state == SecretState::ToReenter)
    );
  }

  #[test]
  fn one_failing_secret_is_the_only_one_to_reenter() {
    #[derive(Debug)]
    struct NoSession(MemoryStore);
    impl SecretStore for NoSession {
      fn get(&self, name: SecretName) -> Result<Option<String>, SecretError> {
        self.0.get(name)
      }
      fn set(&self, name: SecretName, value: &str) -> Result<(), SecretError> {
        if name == SecretName::LastfmSession {
          Err(SecretError("refusé".into()))
        } else {
          self.0.set(name, value)
        }
      }
      fn delete(&self, name: SecretName) -> Result<(), SecretError> {
        self.0.delete(name)
      }
    }
    let store = NoSession(MemoryStore::default());
    let dirs = dirs(STORAGE);
    let outcome = run_choice(
      ImportChoice::Riff,
      &LegacyStorage::open(&dirs.storage),
      &dirs.settings,
      &dirs.data,
      &store,
      0,
    );
    let states: Vec<_> = outcome
      .report
      .unwrap()
      .secrets
      .iter()
      .map(|secret| (secret.key, secret.state))
      .collect();
    assert_eq!(
      states,
      [
        ("riff_lastfm_apisecret", SecretState::Imported),
        ("riff_lastfm_sk", SecretState::ToReenter),
        ("riff_deepl_api_key", SecretState::Imported),
      ]
    );
    assert_eq!(store.get(SecretName::LastfmSession).unwrap(), None);
  }

  #[test]
  fn no_secret_reaches_the_files() {
    let dirs = dirs(STORAGE);
    let _ = run_choice(
      ImportChoice::Riff,
      &LegacyStorage::open(&dirs.storage),
      &dirs.settings,
      &dirs.data,
      &MemoryStore::default(),
      0,
    );
    for file in [&dirs.settings, &dirs.data] {
      let raw = fs::read_to_string(file).unwrap();
      assert!(!raw.contains("VALEUR-SECRET"), "{file:?}");
      assert!(!raw.contains("VALEUR-APIKEY"), "{file:?}");
    }
  }

  #[test]
  fn storage_json_is_identical_after_each_choice() {
    for choice in [ImportChoice::Riff, ImportChoice::Fresh] {
      let dirs = dirs(STORAGE);
      let before = fs::read(&dirs.storage).unwrap();
      let _ = run_choice(
        choice,
        &LegacyStorage::open(&dirs.storage),
        &dirs.settings,
        &dirs.data,
        &MemoryStore::default(),
        0,
      );
      assert_eq!(fs::read(&dirs.storage).unwrap(), before, "{choice:?}");
    }
  }

  // Suspension des écritures.

  fn writes_of(effects: &[Effect]) -> usize {
    effects
      .iter()
      .filter(|effect| {
        matches!(
          effect,
          Effect::SaveSettings(_) | Effect::ScheduleSave(_) | Effect::WriteUserData(_)
        )
      })
      .count()
  }

  #[test]
  fn while_waiting_no_settings_or_data_write_leaves_update() {
    let mut app = waiting_app();
    let effects = app.update(Message::SetThemeMode(crate::settings::ThemeMode::Light));
    assert_eq!(writes_of(&effects), 0, "{effects:?}");
    let effects = app.update(Message::SetLanguage("ru".into()));
    assert_eq!(writes_of(&effects), 0, "{effects:?}");
    assert!(effects.contains(&Effect::LoadLanguage("ru".into())));
    let effects = app.update(Message::Player(crate::ui::PlayerMessage::ToggleMute));
    assert_eq!(writes_of(&effects), 0, "{effects:?}");
    assert!(app.data.muted, "l'état en mémoire change quand même");
    let effects = app.update(Message::Ui(UiMessage::SaveDue(app.data_generation)));
    assert_eq!(writes_of(&effects), 0, "{effects:?}");
  }

  #[test]
  fn the_same_messages_write_without_a_waiting_screen() {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    let effects = app.update(Message::SetThemeMode(crate::settings::ThemeMode::Light));
    assert_eq!(writes_of(&effects), 1, "{effects:?}");
    let effects = app.update(Message::Player(crate::ui::PlayerMessage::ToggleMute));
    assert_eq!(writes_of(&effects), 1, "{effects:?}");
    let effects = app.update(Message::Ui(UiMessage::SaveDue(app.data_generation)));
    assert_eq!(writes_of(&effects), 1, "{effects:?}");
  }

  #[test]
  fn the_filter_keeps_the_other_effects_in_order() {
    let app = waiting_app();
    let kept = app.suspend_writes(vec![
      Effect::QueryScale,
      Effect::ScheduleSave(3),
      Effect::UpdateYtDlp,
    ]);
    assert_eq!(kept, vec![Effect::QueryScale, Effect::UpdateYtDlp]);
  }

  #[test]
  fn closing_without_a_choice_writes_only_the_window_state() {
    let mut app = waiting_app();
    let effects = app.update(Message::CloseStateKnown {
      is_maximized: false,
      is_full_screen: false,
    });
    let [Effect::SaveWindowAndExit(_, data)] = effects.as_slice() else {
      panic!("{effects:?}")
    };
    assert!(data.is_none());
  }

  #[test]
  fn without_a_waiting_screen_the_data_is_written_on_exit() {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    let effects = app.update(Message::CloseStateKnown {
      is_maximized: false,
      is_full_screen: false,
    });
    let [Effect::SaveWindowAndExit(_, data)] = effects.as_slice() else {
      panic!("{effects:?}")
    };
    assert!(data.is_some());
  }

  #[test]
  fn the_window_opening_waits_for_the_choice() {
    let mut app = waiting_app();
    let effects = open(&mut app);
    assert_eq!(effects[0], Effect::QueryScale);
    assert!(!effects.contains(&Effect::UpdateYtDlp));
    // Seules restent deux lectures locales du core (état des outils et de Google, sans réseau).
    assert!(effects.iter().all(|effect| match effect {
      Effect::CoreJson { path, .. } => ["/api/tools-status", "/api/google-status"].contains(path),
      other => !matches!(other, Effect::LoadLanguage(_) | Effect::AudioMix(..)),
    }));
    assert!(!app.ui.home.started);
  }

  #[test]
  fn while_waiting_no_image_is_wanted_and_after_the_choice_they_are() {
    let dirs = dirs(
      r#"{"devsize_favorites": "[{\"id\":\"a\",\"title\":\"A\",\"thumbnail\":\"https://i.ytimg.com/vi/a/hq.jpg\"}]"}"#,
    );
    let startup = startup_state(
      &dirs.settings,
      &dirs.data,
      LegacyStorage::open(&dirs.storage),
      0,
    );
    let mut app = App::new(
      PersistedWindowState::default(),
      startup.settings,
      I18n::default(),
    )
    .with_data(startup.data)
    .with_import(startup.pending);
    app.screen = crate::screens::Screen::Liked;
    open(&mut app);
    assert!(app.import_waiting());
    assert!(
      app.wanted_images().is_empty(),
      "aucune image avant le choix : {:?}",
      app.wanted_images()
    );
    let done = outcome_of(&app, ImportChoice::Riff, &MemoryStore::default());
    app.update(Message::Import(ImportMessage::Done(Box::new(done))));
    assert!(!app.wanted_images().is_empty());
  }

  #[test]
  fn the_choice_starts_the_application_once() {
    let mut app = waiting_app();
    open(&mut app);
    let effects = choose(&mut app, ImportChoice::Fresh);
    let [Effect::ImportChoice(ImportChoice::Fresh, _)] = effects.as_slice() else {
      panic!("{effects:?}")
    };
    let done = outcome_of(&app, ImportChoice::Fresh, &MemoryStore::default());
    let effects = app.update(Message::Import(ImportMessage::Done(Box::new(done.clone()))));
    assert_eq!(
      effects
        .iter()
        .filter(|effect| **effect == Effect::UpdateYtDlp)
        .count(),
      1,
      "{effects:?}"
    );
    assert!(effects.iter().any(|effect| matches!(
      effect,
      Effect::CoreJson {
        path: "/api/vibe",
        ..
      }
    )));
    assert!(app.import.pending.is_none());
    let again = app.update(Message::Import(ImportMessage::Done(Box::new(done))));
    assert!(!again.contains(&Effect::UpdateYtDlp));
  }

  #[test]
  fn a_choice_before_the_window_opens_defers_nothing() {
    let mut app = waiting_app();
    choose(&mut app, ImportChoice::Fresh);
    let done = outcome_of(&app, ImportChoice::Fresh, &MemoryStore::default());
    let effects = app.update(Message::Import(ImportMessage::Done(Box::new(done))));
    assert!(!effects.contains(&Effect::UpdateYtDlp));
    let effects = open(&mut app);
    assert!(effects.contains(&Effect::UpdateYtDlp));
  }

  #[test]
  fn a_second_click_during_the_work_does_nothing() {
    let mut app = waiting_app();
    assert_eq!(choose(&mut app, ImportChoice::Riff).len(), 1);
    assert!(choose(&mut app, ImportChoice::Fresh).is_empty());
    assert!(choose(&mut app, ImportChoice::Riff).is_empty());
  }

  #[test]
  fn nothing_to_choose_without_a_waiting_screen() {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    assert!(choose(&mut app, ImportChoice::Fresh).is_empty());
  }

  #[test]
  fn the_choice_effect_carries_the_storage_of_the_offer() {
    let mut app = waiting_app();
    let effects = choose(&mut app, ImportChoice::Riff);
    let [Effect::ImportChoice(_, storage)] = effects.as_slice() else {
      panic!("{effects:?}")
    };
    assert_eq!(**storage, app.import.pending.as_ref().unwrap().storage);
  }

  #[test]
  fn writes_flow_again_after_the_choice() {
    let mut app = waiting_app();
    choose(&mut app, ImportChoice::Fresh);
    let done = outcome_of(&app, ImportChoice::Fresh, &MemoryStore::default());
    app.update(Message::Import(ImportMessage::Done(Box::new(done))));
    let effects = app.update(Message::SetThemeMode(crate::settings::ThemeMode::Light));
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::SaveSettings(_)))
    );
    let effects = app.update(Message::CloseStateKnown {
      is_maximized: false,
      is_full_screen: false,
    });
    assert!(matches!(
      effects.as_slice(),
      [Effect::SaveWindowAndExit(_, Some(_))]
    ));
  }

  #[test]
  fn start_fresh_replaces_the_state_and_shows_no_report() {
    let mut app = waiting_app();
    assert_eq!(app.data.favorites.len(), 2);
    choose(&mut app, ImportChoice::Fresh);
    let done = outcome_of(&app, ImportChoice::Fresh, &MemoryStore::default());
    let effects = app.update(Message::Import(ImportMessage::Done(Box::new(done))));
    assert!(app.data.favorites.is_empty());
    assert_eq!(app.settings.import_source, ImportSource::Fresh);
    assert!(app.ui.modal.is_none());
    assert!(effects.contains(&Effect::LoadLanguage("en".into())));
  }

  #[test]
  fn the_waiting_screen_and_the_report_window_build() {
    let mut app = waiting_app();
    drop(app.view());
    choose(&mut app, ImportChoice::Riff);
    drop(app.view());
    let done = outcome_of(&app, ImportChoice::Riff, &MemoryStore::default());
    app.update(Message::Import(ImportMessage::Done(Box::new(done))));
    assert!(matches!(app.ui.modal, Some(Modal::ImportReport(_))));
    drop(app.view());
  }
  #[test]
  fn the_import_keeps_the_state_and_opens_the_report() {
    let mut app = waiting_app();
    choose(&mut app, ImportChoice::Riff);
    let done = outcome_of(&app, ImportChoice::Riff, &MemoryStore::default());
    app.update(Message::Import(ImportMessage::Done(Box::new(done))));
    assert_eq!(app.data.favorites.len(), 2);
    let Some(Modal::ImportReport(report)) = &app.ui.modal else {
      panic!("{:?}", app.ui.modal)
    };
    assert!(report.imported.contains(&"riff_ui_lang".to_string()));
  }

  #[test]
  fn the_window_geometry_survives_the_choice() {
    let mut app = waiting_app();
    app.settings.screen = crate::screens::Screen::Library;
    choose(&mut app, ImportChoice::Fresh);
    let done = outcome_of(&app, ImportChoice::Fresh, &MemoryStore::default());
    app.update(Message::Import(ImportMessage::Done(Box::new(done))));
    assert_eq!(app.settings.screen, crate::screens::Screen::Library);
  }

  // Raccourcis.

  #[test]
  fn the_six_letters_and_the_other_keys_do_nothing_while_waiting() {
    use crate::lyrics::update::LyricsMessage;
    use crate::ui::PlayerMessage;
    let mut app = waiting_app();
    let shortcut = |message| Message::Shortcut(Box::new(message));
    for message in [
      shortcut(Message::Player(PlayerMessage::ToggleMute)),
      shortcut(Message::Player(PlayerMessage::ToggleShuffle)),
      shortcut(Message::Player(PlayerMessage::ToggleRepeat)),
      shortcut(Message::Lyrics(LyricsMessage::TogglePanel)),
      shortcut(Message::Lyrics(LyricsMessage::OpenTvk)),
      shortcut(Message::ToggleMiniMode),
      Message::Ui(UiMessage::FocusSearch),
      Message::Player(PlayerMessage::TogglePlay),
      Message::Player(PlayerMessage::Next),
      Message::Player(PlayerMessage::SeekBy(5.0)),
      Message::Player(PlayerMessage::VolumeBy(1.0)),
    ] {
      let before = format!(
        "{:?}",
        (app.data.muted, app.data.shuffle, app.lyrics.panel_open)
      );
      assert!(app.update(message.clone()).is_empty(), "{message:?}");
      let after = format!(
        "{:?}",
        (app.data.muted, app.data.shuffle, app.lyrics.panel_open)
      );
      assert_eq!(before, after, "{message:?}");
    }
    assert!(!app.lyrics.tvk_open);
  }

  // Rapport après un compte .rfacc.

  #[test]
  fn an_account_import_opens_the_report_with_secrets_to_reenter() {
    use crate::manage::ManageMessage;
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    let account = json!({ "localStorage": {
      "riff_ui_lang": "ru",
      "riff_font": "VALEUR-FONT",
      "riff_lastfm_sk": "VALEUR-SECRET-2",
      "riff_cle_inconnue": "VALEUR-INCONNUE"
    }});
    app.update(Message::Manage(ManageMessage::AccountRead(Ok(Some(
      account,
    )))));
    let Some(Modal::ImportReport(report)) = &app.ui.modal else {
      panic!("{:?}", app.ui.modal)
    };
    assert!(report.imported.contains(&"riff_ui_lang".to_string()));
    assert_eq!(report.unknown, ["riff_cle_inconnue"]);
    assert_eq!(report.not_ported[0].key, "riff_font");
    assert!(
      report
        .secrets
        .iter()
        .all(|secret| secret.state == SecretState::ToReenter),
      "un compte n'apporte aucun jeton"
    );
    assert!(!format!("{report:?}").contains("VALEUR"));
  }
}
