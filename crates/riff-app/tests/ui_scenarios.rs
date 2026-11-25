//! Scénarios de bout en bout sur l'état pur de l'interface.
//!
//! Chaque test enchaîne des `Message` passés à `App::update` et vérifie l'état et les `Effect`
//! émis. Aucune fenêtre, aucun périphérique audio, aucun réseau : les effets ne sont pas exécutés.
//! Les tests unitaires de chaque message sont dans `src/*` (`update_tests.rs`, `app.rs`,
//! `plugins.rs`, `import_flow.rs`) ; ce fichier couvre les enchaînements, via l'API publique.

use iced::Size;
use material_colors::color::Rgb;
use riff_app::app::{App, Effect, Message};
use riff_app::i18n::I18n;
use riff_app::import_flow::{ImportChoice, ImportMessage, run_choice, startup_state};
use riff_app::plugins::{PluginAnswer, PluginMessage};
use riff_app::screens::Screen;
use riff_app::settings::{LegacyStorage, NativeSettings, ThemeMode};
use riff_app::theme::{ThemeSource, file as theme_file};
use riff_app::ui::{ListId, Modal, PlayerMessage, UiMessage};
use riff_app::user_data::{Playlist, Track, UserData};
use riff_app::window::PersistedWindowState;
use riff_audio::{AudioError, Event as AudioEvent, Source};
use riff_core::secrets::MemoryStore;
use riff_plugins::{HostRequest, NotifyKind};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Dossier temporaire supprimé à la fin du test, même en cas d'échec.
struct TempDir(PathBuf);

impl TempDir {
  fn new() -> Self {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
      "riff-app-ui-scenarios-{}-{}",
      std::process::id(),
      NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    Self(dir)
  }

  fn path(&self, name: &str) -> PathBuf {
    self.0.join(name)
  }
}

impl Drop for TempDir {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.0);
  }
}

fn track(id: &str) -> Track {
  Track {
    id: Some(id.to_string()),
    title: Some(format!("Titre {id}")),
    artist: Some(format!("Artiste {id}")),
    duration: Some(200.0),
    url: Some(format!("https://exemple.org/{id}")),
    platform: Some("ytmusic".to_string()),
    ..Track::default()
  }
}

fn tracks(ids: &[&str]) -> Vec<Track> {
  ids.iter().map(|id| track(id)).collect()
}

fn app_from(settings: NativeSettings, data: UserData) -> App {
  // La machine ne décide pas : animations actives, comme un Riff vierge sur un système sans réduction.
  riff_app::motion::inject_system_preference(Some(false));
  App::new(PersistedWindowState::default(), settings, I18n::default()).with_data(data)
}

fn open(app: &mut App) -> Vec<Effect> {
  app.update(Message::WindowOpened {
    position: None,
    size: Size::new(1000.0, 700.0),
  })
}

fn current_id(app: &App) -> &str {
  app
    .player
    .current
    .as_ref()
    .and_then(|track| track.id.as_deref())
    .unwrap_or("")
}

fn resolve_token(effects: &[Effect]) -> Option<u64> {
  effects.iter().find_map(|effect| match effect {
    Effect::ResolveTrack { token, .. } => Some(*token),
    _ => None,
  })
}

fn resolve_forced(effects: &[Effect]) -> Option<bool> {
  effects.iter().find_map(|effect| match effect {
    Effect::ResolveTrack { options, .. } => Some(options.force_yt_dlp),
    _ => None,
  })
}

fn resolve_fresh(effects: &[Effect]) -> Option<bool> {
  effects.iter().find_map(|effect| match effect {
    Effect::ResolveTrack { options, .. } => Some(options.fresh),
    _ => None,
  })
}

fn toasts(effects: &[Effect]) -> Vec<&str> {
  effects
    .iter()
    .filter_map(|effect| match effect {
      Effect::Toast(text, _) => Some(text.as_str()),
      _ => None,
    })
    .collect()
}

fn published_state(effects: &[Effect]) -> Option<&serde_json::Value> {
  effects.iter().find_map(|effect| match effect {
    Effect::PublishAppState(state) => Some(state),
    _ => None,
  })
}

fn loaded(app: &mut App) -> Vec<Effect> {
  app.update(Message::Audio(AudioEvent::Loaded {
    duration: Some(Duration::from_secs(200)),
    sample_rate: 48_000,
  }))
}

fn resolved(app: &mut App, token: u64) -> Vec<Effect> {
  app.update(Message::Player(PlayerMessage::Resolved {
    token,
    start: Duration::ZERO,
    crossfade: None,
    result: Ok(Source::File(PathBuf::from("a.mp3"))),
  }))
}

fn has_save_settings(effects: &[Effect]) -> bool {
  effects
    .iter()
    .any(|effect| matches!(effect, Effect::SaveSettings(_)))
}

fn resolve_current(app: &mut App) -> Vec<Effect> {
  let token = app.player.token;
  resolved(app, token)
}

// 1. Démarrage

#[test]
fn startup_without_any_file_opens_on_the_defaults_and_starts_the_engine_once() {
  let dir = TempDir::new();
  let startup = startup_state(
    &dir.path("native-settings.json"),
    &dir.path("user-data.json"),
    LegacyStorage::open(&dir.path("absent.json")),
    0,
  );
  assert!(startup.pending.is_none(), "rien à importer, pas d'écran");
  let mut app = app_from(startup.settings, startup.data).with_import(startup.pending);
  assert!(!app.import_waiting());
  assert_eq!(app.settings.ui_lang, "en");
  assert_eq!(app.theme.source, ThemeSource::Accent);
  assert!(app.player.queue.is_empty() && app.player.current.is_none());
  assert!(!app.playback.is_playing);
  assert!(app.data.favorites.is_empty() && app.data.playlists.is_empty());

  let effects = open(&mut app);
  assert!(effects.contains(&Effect::QueryScale));
  assert!(effects.contains(&Effect::AudioMix(app.data.volume, app.data.muted)));
  assert!(effects.contains(&Effect::UpdateYtDlp));
  assert!(
    !effects
      .iter()
      .any(|effect| matches!(effect, Effect::LoadLanguage(_))),
    "l'anglais est la langue intégrée : rien à charger"
  );
}

// 2. Import de user-data.json

#[test]
fn a_saved_user_data_file_is_loaded_at_startup_and_drives_the_player_and_the_language() {
  let dir = TempDir::new();
  let data = UserData {
    favorites: tracks(&["a", "b"]),
    playlists: vec![Playlist {
      id: Some("pl_1".into()),
      name: Some("Route".into()),
      tracks: tracks(&["c"]),
      ..Playlist::default()
    }],
    volume: 0.4,
    muted: true,
    shuffle: true,
    repeat: true,
    crossfade: 5,
    ..UserData::default()
  };
  data.save(&dir.path("user-data.json")).unwrap();
  let settings = NativeSettings {
    ui_lang: "fr".into(),
    ..NativeSettings::default()
  };
  settings.save(&dir.path("native-settings.json")).unwrap();

  let startup = startup_state(
    &dir.path("native-settings.json"),
    &dir.path("user-data.json"),
    LegacyStorage::open(&dir.path("absent.json")),
    0,
  );
  assert!(startup.pending.is_none());
  assert_eq!(startup.data, data);
  let mut app = app_from(startup.settings, startup.data);
  assert_eq!(app.data.favorites.len(), 2);
  assert_eq!(app.data.playlists[0].name.as_deref(), Some("Route"));
  assert!(app.player.shuffle && app.player.repeat);
  assert_eq!(app.player.crossfade, 5);

  let effects = open(&mut app);
  assert!(effects.contains(&Effect::AudioMix(0.4, true)));
  assert!(effects.contains(&Effect::LoadLanguage("fr".into())));
}

#[test]
fn the_import_from_riff_holds_the_startup_until_the_choice_then_releases_it() {
  let dir = TempDir::new();
  fs::write(
    dir.path("storage.json"),
    r#"{
      "devsize_favorites": "[{\"id\":\"a\",\"title\":\"A\"},{\"id\":\"b\",\"title\":\"B\"}]",
      "devsize_playlists": "[{\"id\":\"p\",\"name\":\"P\",\"tracks\":[]}]",
      "riff_ui_lang": "uk"
    }"#,
  )
  .unwrap();
  let (settings_path, data_path) = (dir.path("native-settings.json"), dir.path("user-data.json"));
  let storage = LegacyStorage::open(&dir.path("storage.json"));
  let startup = startup_state(&settings_path, &data_path, storage.clone(), 0);
  let mut app = app_from(startup.settings, startup.data).with_import(startup.pending);
  assert!(app.import_waiting());

  let effects = open(&mut app);
  assert!(
    !effects
      .iter()
      .any(|effect| matches!(effect, Effect::AudioMix(..) | Effect::UpdateYtDlp)),
    "mixage et yt-dlp attendent le choix"
  );
  assert!(
    !settings_path.exists() && !data_path.exists(),
    "rien d'écrit"
  );

  let effects = app.update(Message::Import(ImportMessage::Choose(ImportChoice::Riff)));
  assert!(matches!(
    effects.as_slice(),
    [Effect::ImportChoice(ImportChoice::Riff, _)]
  ));
  let outcome = run_choice(
    ImportChoice::Riff,
    &storage,
    &settings_path,
    &data_path,
    &MemoryStore::default(),
    0,
  );
  let effects = app.update(Message::Import(ImportMessage::Done(Box::new(outcome))));
  assert!(!app.import_waiting());
  assert_eq!(app.data.favorites.len(), 2);
  assert_eq!(app.settings.ui_lang, "uk");
  assert!(effects.contains(&Effect::UpdateYtDlp), "démarrage libéré");
  assert_eq!(
    effects
      .iter()
      .filter(|effect| matches!(effect, Effect::LoadLanguage(lang) if lang == "uk"))
      .count(),
    1,
    "la langue importée n'est demandée qu'une fois"
  );
  assert!(matches!(app.ui.modal, Some(Modal::ImportReport(_))));

  // Le fichier écrit par le choix est celui que le lancement suivant relit.
  let again = startup_state(&settings_path, &data_path, storage, 0);
  assert!(again.pending.is_none(), "plus d'écran au lancement suivant");
  assert_eq!(again.data, app.data);
  assert_eq!(again.settings.ui_lang, "uk");
}

// 3. Lecture d'une file

#[test]
fn playing_a_queue_walks_through_resolution_loading_progress_and_the_end_of_each_title() {
  let mut app = app_from(
    NativeSettings::default(),
    UserData {
      favorites: tracks(&["a", "b", "c"]),
      autoplay: false,
      ..UserData::default()
    },
  );
  let effects = app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  assert_eq!(current_id(&app), "a");
  assert_eq!(app.player.queue.len(), 3);
  assert!(app.player.loading && !app.playback.is_playing);
  let first = resolve_token(&effects).expect("résolution demandée");
  assert_eq!(first, app.player.token);

  let effects = resolved(&mut app, first);
  assert!(matches!(
    effects.as_slice(),
    [Effect::AudioLoadSource(_, start)] if *start == Duration::ZERO
  ));

  let effects = loaded(&mut app);
  assert!(app.playback.is_playing && !app.player.loading);
  let state = published_state(&effects).expect("état publié");
  assert_eq!(state["title"], "Titre a");
  assert_eq!(state["isPlaying"], true);

  app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(
    12,
  ))));
  assert_eq!(app.playback.position, Duration::from_secs(12));

  // Fin prématurée (flux coupé à 12 s sur 200 s) : même titre, résolution fraîche (première reprise).
  let effects = app.update(Message::Audio(AudioEvent::Ended));
  assert_eq!(current_id(&app), "a");
  assert_eq!(resolve_forced(&effects), Some(false));
  assert_eq!(resolve_fresh(&effects), Some(true));
  resolve_current(&mut app);
  loaded(&mut app);

  // Fin réelle du premier titre : le deuxième est demandé, l'historique est tenu à jour.
  app.update(Message::Audio(AudioEvent::Position(
    Duration::from_secs_f64(199.5),
  )));
  let first = app.player.token;
  let effects = app.update(Message::Audio(AudioEvent::Ended));
  assert_eq!(current_id(&app), "b");
  assert_eq!(app.player.index, Some(1));
  let second = resolve_token(&effects).expect("résolution du suivant");
  assert_ne!(second, first);
  assert_eq!(app.data.history[0].id.as_deref(), Some("b"));
  assert_eq!(app.data.history[1].id.as_deref(), Some("a"));
  assert!(
    resolved(&mut app, first).is_empty(),
    "la réponse du premier titre arrive trop tard"
  );
  resolved(&mut app, second);
  loaded(&mut app);

  // Précédent avant 3 s joue le titre précédent ; après 3 s il revient au début.
  app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(1))));
  let effects = app.update(Message::Player(PlayerMessage::Prev));
  assert_eq!(current_id(&app), "a");
  assert!(resolve_token(&effects).is_some());
  resolve_current(&mut app);
  loaded(&mut app);
  app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(
    30,
  ))));
  assert_eq!(
    app.update(Message::Player(PlayerMessage::Prev)),
    vec![Effect::AudioSeek(Duration::ZERO),]
  );
  assert_eq!(current_id(&app), "a");

  // Suivant deux fois, puis la fin de la file sans lecture automatique.
  app.update(Message::Player(PlayerMessage::Next));
  assert_eq!(current_id(&app), "b");
  resolve_current(&mut app);
  loaded(&mut app);
  app.update(Message::Player(PlayerMessage::Next));
  assert_eq!(current_id(&app), "c");
  resolve_current(&mut app);
  loaded(&mut app);
  app.update(Message::Audio(AudioEvent::Position(
    Duration::from_secs_f64(199.5),
  )));
  let effects = app.update(Message::Audio(AudioEvent::Ended));
  assert_eq!(current_id(&app), "a", "retour au début de la file");
  assert!(resolve_token(&effects).is_some());
}

// 4. Thème

#[test]
fn the_theme_follows_the_mode_the_cover_and_a_theme_file_and_survives_a_restart() {
  let dir = TempDir::new();
  let path = dir.path("native-settings.json");
  let mut app = app_from(NativeSettings::default(), UserData::default());
  let initial_primary = app.theme.scheme.primary;

  let effects = app.update(Message::SetThemeMode(ThemeMode::Light));
  assert!(!app.theme.dark && has_save_settings(&effects));

  // En mode automatique, la pochette décide ; en mode fixe, elle est sans effet.
  app.update(Message::SetThemeMode(ThemeMode::Auto));
  let effects = app.update(Message::CoverBytes(vec![1, 2, 3].into()));
  assert!(matches!(effects.as_slice(), [Effect::ExtractCover(_)]));
  app.update(Message::CoverColor(Rgb::new(0x80, 0x20, 0x20)));
  assert!(matches!(app.theme.source, ThemeSource::Cover(_)));
  assert_ne!(app.theme.scheme.primary, initial_primary);

  // Un fichier de thème prend la main et la pochette ne le remplace plus.
  assert_eq!(
    app.update(Message::SelectThemeFile("nuit".into())),
    vec![Effect::LoadThemeFile("nuit".into())]
  );
  let nuit =
    theme_file::parse(r##"{ "name": "Nuit", "mode": "dark", "source": "#5247D1" }"##).unwrap();
  let effects = app.update(Message::ThemeFileLoaded("nuit".into(), Ok(Box::new(nuit))));
  assert!(has_save_settings(&effects));
  assert_eq!(app.theme.source, ThemeSource::File("nuit".into()));
  app.update(Message::CoverColor(Rgb::new(0x20, 0x80, 0x20)));
  assert_eq!(app.theme.source, ThemeSource::File("nuit".into()));

  // Un fichier refusé laisse le thème en place et garde la raison.
  app.update(Message::ThemeFileLoaded(
    "casse".into(),
    Err("contraste".into()),
  ));
  assert_eq!(app.theme.source, ThemeSource::File("nuit".into()));
  assert_eq!(app.last_theme_error.as_deref(), Some("contraste"));

  // Redémarrage : les réglages écrits redonnent le mode et le fichier choisis.
  app.settings.save(&path).unwrap();
  let startup = startup_state(
    &path,
    &dir.path("user-data.json"),
    LegacyStorage::open(&dir.path("absent.json")),
    0,
  );
  assert_eq!(startup.settings.theme_mode, ThemeMode::Auto);
  assert_eq!(startup.settings.native_theme.as_deref(), Some("nuit"));

  let effects = app.update(Message::ClearThemeFile);
  assert!(has_save_settings(&effects));
  assert_eq!(app.settings.native_theme, None);
  assert!(!matches!(app.theme.source, ThemeSource::File(_)));
}

// 5. Langue

#[test]
fn changing_the_language_loads_applies_saves_and_comes_back_at_the_next_launch() {
  let dir = TempDir::new();
  let path = dir.path("native-settings.json");
  let cache = dir.path("cache-i18n");
  let mut app = app_from(NativeSettings::default(), UserData::default());
  assert_eq!(app.i18n.t("Settings"), "Settings");

  let effects = app.update(Message::SetLanguage("ru".into()));
  assert_eq!(app.settings.ui_lang, "ru");
  assert_eq!(effects[0], Effect::LoadLanguage("ru".into()));
  assert!(has_save_settings(&effects));
  assert_eq!(
    app.i18n.t("Settings"),
    "Settings",
    "pas avant la fin du chargement"
  );

  app.update(Message::LanguageLoaded(Arc::new(I18n::load("ru", &cache))));
  assert_eq!(app.i18n.lang(), "ru");
  assert_eq!(app.i18n.t("Settings"), "Настройки");

  // Un choix plus récent l'emporte sur une réponse tardive de l'ancien.
  app.update(Message::SetLanguage("uk".into()));
  app.update(Message::SetLanguage("ru".into()));
  app.update(Message::LanguageLoaded(Arc::new(I18n::load("uk", &cache))));
  assert_eq!(app.i18n.lang(), "ru");

  // Le lancement suivant relit la langue et la demande après l'ouverture de la fenêtre.
  app.settings.save(&path).unwrap();
  let startup = startup_state(
    &path,
    &dir.path("user-data.json"),
    LegacyStorage::open(&dir.path("absent.json")),
    0,
  );
  let mut next = app_from(startup.settings, startup.data);
  assert!(open(&mut next).contains(&Effect::LoadLanguage("ru".into())));

  // Retour à l'anglais.
  let effects = app.update(Message::SetLanguage("en".into()));
  assert_eq!(effects[0], Effect::LoadLanguage("en".into()));
  app.update(Message::LanguageLoaded(Arc::new(I18n::load("en", &cache))));
  assert_eq!(app.i18n.t("Settings"), "Settings");
}

// 6. Plugin

#[test]
fn enabling_a_plugin_then_playing_notifying_and_disabling_it() {
  let mut app = app_from(
    NativeSettings::default(),
    UserData {
      autoplay: false,
      ..UserData::default()
    },
  );
  let queue = tracks(&["a", "b", "c", "d"]);
  app.player.play_list(queue[0].clone(), queue, 0, "ytmusic");
  app.player.shuffle = true;
  app.mark_resume_saved();

  // Sans plugin actif, aucun événement n'est construit.
  assert!(app.plugin_events().is_empty());

  let code = "// @name Bonjour\n// @permissions notify\n";
  let source = riff_plugins::manifest::from_bytes("hello.rhai", code.as_bytes()).unwrap();
  assert_eq!(
    app.update(Message::Plugin(PluginMessage::Toggle(
      "hello.rhai".into(),
      true
    ))),
    vec![Effect::LoadPlugin("hello.rhai".into())]
  );
  let effects = app.update(Message::Plugin(PluginMessage::Loaded(Ok(Box::new(
    source.clone(),
  )))));
  assert!(
    matches!(&effects[0], Effect::SavePluginConsent(consent) if consent.enabled["hello.rhai"] == source.hash)
  );
  assert!(effects.contains(&Effect::ActivatePlugin(Box::new(source.clone()))));
  assert!(effects.contains(&Effect::ScanPlugins));
  assert_eq!(app.plugins.names["hello.rhai"], "Bonjour");

  // `Program` recopie l'état de l'hôte dans l'application ; ici à la main.
  app.plugins.active = true;
  let events = app.plugin_events();
  assert_eq!(events.len(), 1, "changement de titre à la première lecture");

  // Une notification du plugin devient un toast avec le nom du plugin.
  let PluginAnswer::Silent(effects) = app.answer_plugin(
    "hello.rhai",
    &HostRequest::Notify {
      message: "Now playing".into(),
      kind: NotifyKind::Info,
    },
  ) else {
    panic!("une notification n'attend pas de réponse");
  };
  assert!(
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::ToastTimer(..)))
  );
  let toast = app.toasts.active.as_ref().expect("toast affiché");
  assert_eq!(toast.message, "Bonjour : Now playing");

  // Désactivation : consentement retiré, plugin arrêté, nom oublié.
  let effects = app.update(Message::Plugin(PluginMessage::Toggle(
    "hello.rhai".into(),
    false,
  )));
  assert_eq!(effects[0], Effect::DeactivatePlugin("hello.rhai".into()));
  assert!(matches!(&effects[1], Effect::SavePluginConsent(consent) if consent.enabled.is_empty()));
  assert!(!app.plugins.names.contains_key("hello.rhai"));
  app.plugins.active = false;
  assert!(app.plugin_events().is_empty());
}

// 7. Perte du périphérique

#[test]
fn losing_the_output_device_is_reported_and_the_recovery_keeps_the_position() {
  let mut app = app_from(
    NativeSettings::default(),
    UserData {
      favorites: tracks(&["a", "b"]),
      autoplay: false,
      ..UserData::default()
    },
  );
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  resolve_current(&mut app);
  loaded(&mut app);
  app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(
    42,
  ))));
  assert!(app.playback.is_playing);

  let effects = app.update(Message::Audio(AudioEvent::Error(AudioError::NoDevice)));
  assert_eq!(
    app.playback.error.as_deref(),
    Some("aucun périphérique de sortie")
  );
  assert_eq!(app.playback.position, Duration::from_secs(42));
  assert_eq!(current_id(&app), "a");
  // Pas de nouvelle résolution du flux, compteur de reprises intact, toast dédié, pause publiée.
  assert_eq!(resolve_forced(&effects), None);
  assert_eq!(app.player.stream_retries, 0);
  assert_eq!(toasts(&effects), ["No audio output device"]);
  assert_eq!(
    published_state(&effects).and_then(|state| state["isPlaying"].as_bool()),
    Some(false)
  );
  assert!(!app.playback.is_playing);
  assert!(!app.player.loading);

  // Retour d'un périphérique : le moteur reprend (`resume_playing`), l'état suit. Nom mémorisé,
  // position et titre conservés, aucune résolution.
  let effects = app.update(Message::Audio(AudioEvent::DeviceChanged("Casque".into())));
  assert_eq!(resolve_forced(&effects), None);
  assert!(toasts(&effects).is_empty());
  assert_eq!(
    published_state(&effects).and_then(|state| state["isPlaying"].as_bool()),
    Some(true)
  );
  assert!(app.playback.is_playing);
  assert_eq!(app.playback.device.as_deref(), Some("Casque"));
  assert_eq!(app.playback.position, Duration::from_secs(42));
  assert_eq!(current_id(&app), "a");
}

#[test]
fn a_device_change_without_a_loss_or_after_a_manual_pause_does_not_start_playback() {
  let mut app = app_from(
    NativeSettings::default(),
    UserData {
      favorites: tracks(&["a", "b"]),
      autoplay: false,
      ..UserData::default()
    },
  );
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  resolve_current(&mut app);
  loaded(&mut app);

  // Changement de périphérique en lecture : rien à faire.
  let effects = app.update(Message::Audio(AudioEvent::DeviceChanged("Enceinte".into())));
  assert!(effects.is_empty());
  assert!(app.playback.is_playing);

  // Perte, puis pause explicite pendant la panne (lecture, pause) : le moteur ne reprend pas.
  app.update(Message::Audio(AudioEvent::Error(AudioError::NoDevice)));
  app.update(Message::Player(PlayerMessage::TogglePlay));
  app.update(Message::Player(PlayerMessage::TogglePlay));
  assert!(!app.playback.is_playing);
  let effects = app.update(Message::Audio(AudioEvent::DeviceChanged("Casque".into())));
  assert!(effects.is_empty());
  assert!(!app.playback.is_playing);
}

#[test]
fn another_audio_error_still_asks_for_a_new_resolution() {
  let mut app = app_from(
    NativeSettings::default(),
    UserData {
      favorites: tracks(&["a", "b"]),
      autoplay: false,
      ..UserData::default()
    },
  );
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  resolve_current(&mut app);
  loaded(&mut app);
  let effects = app.update(Message::Audio(AudioEvent::Error(AudioError::Source(
    "x".to_string(),
  ))));
  assert_eq!(resolve_forced(&effects), Some(false));
  assert_eq!(resolve_fresh(&effects), Some(true));
  assert_eq!(app.player.stream_retries, 1);
  assert!(toasts(&effects).is_empty());
}

#[test]
fn losing_the_device_on_a_local_track_pauses_with_the_device_toast_and_resumes_on_return() {
  let local = Track {
    platform: Some("local".to_string()),
    path: Some("a.mp3".to_string()),
    ..track("l")
  };
  let mut app = app_from(
    NativeSettings::default(),
    UserData {
      local_files: vec![local],
      autoplay: false,
      ..UserData::default()
    },
  );
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Local, 0)));
  loaded(&mut app);
  let effects = app.update(Message::Audio(AudioEvent::Error(AudioError::NoDevice)));
  assert_eq!(toasts(&effects), ["No audio output device"]);
  assert_eq!(resolve_forced(&effects), None);
  assert!(!app.playback.is_playing);
  assert_eq!(current_id(&app), "l");

  let effects = app.update(Message::Audio(AudioEvent::DeviceChanged("Casque".into())));
  assert!(toasts(&effects).is_empty());
  assert!(app.playback.is_playing);
  assert_eq!(current_id(&app), "l");
}

// Défaut instruit : identifiants de playlist

#[test]
fn three_playlists_created_through_the_ui_get_distinct_ids() {
  let mut app = app_from(NativeSettings::default(), UserData::default());
  for name in ["Une", "Deux", "Trois"] {
    app.update(Message::Ui(UiMessage::NewPlaylist));
    app.update(Message::Ui(UiMessage::ModalName(name.into())));
    app.update(Message::Ui(UiMessage::ModalConfirm));
  }
  let ids: Vec<_> = app
    .data
    .playlists
    .iter()
    .map(|playlist| playlist.id.clone().unwrap())
    .collect();
  assert_eq!(ids.len(), 3);
  let mut sorted = ids.clone();
  sorted.sort();
  sorted.dedup();
  assert_eq!(sorted.len(), 3, "{ids:?}");
  assert_eq!(app.screen, Screen::PlaylistDetail);
}

#[test]
fn create_playlist_ids_never_collide_by_concatenation_nor_with_existing_ones() {
  // Mêmes `(now_ms, unique)` : la seconde playlist reçoit un autre identifiant et reste atteignable.
  let mut data = UserData::default();
  let first = data.create_playlist("Une", Vec::new(), 1_000, 1).unwrap();
  let second = data.create_playlist("Deux", Vec::new(), 1_000, 1).unwrap();
  assert_ne!(first, second);
  assert_eq!(data.playlist(&first).unwrap().name.as_deref(), Some("Une"));
  assert_eq!(
    data.playlist(&second).unwrap().name.as_deref(),
    Some("Deux")
  );

  // Couples différents dont la concaténation hexadécimale serait identique.
  let mut data = UserData::default();
  let a = data.create_playlist("A", Vec::new(), 0x1, 0x23).unwrap();
  let b = data.create_playlist("B", Vec::new(), 0x12, 0x3).unwrap();
  assert_ne!(a, b);
  assert_eq!(data.playlist(&a).unwrap().name.as_deref(), Some("A"));
  assert_eq!(data.playlist(&b).unwrap().name.as_deref(), Some("B"));

  // Playlist déjà présente (importée) portant l'identifiant qui serait produit.
  let mut data = UserData::default();
  data.playlists.push(Playlist {
    id: Some("pl_3e8_1".into()),
    name: Some("Importée".into()),
    ..Playlist::default()
  });
  let created = data.create_playlist("Neuve", Vec::new(), 1_000, 1).unwrap();
  assert_ne!(created, "pl_3e8_1");
  assert_eq!(data.playlists.len(), 2);
  assert_eq!(
    data.playlist(&created).unwrap().name.as_deref(),
    Some("Neuve")
  );
  assert_eq!(
    data.playlist("pl_3e8_1").unwrap().name.as_deref(),
    Some("Importée")
  );
}
