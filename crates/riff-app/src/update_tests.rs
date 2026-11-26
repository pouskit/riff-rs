//! Tests de `update` : lecture, recherche, listes, playlists.

use crate::app::{App, Effect, Message};
use crate::audio_prefs::{AudioMessage, SleepChoice, SleepTimer};
use crate::i18n::I18n;
use crate::library::FAVORITES_SCOPE;
use crate::prefs::{BoolKey, SettingsMessage};
use crate::resume::ResumePosition;
use crate::screens::Screen;
use crate::settings::NativeSettings;
use crate::ui::{
  ListId, Modal, PlayerMessage, QueueTab, ReplyTag, SidebarTab, TrackCommand, UiMessage,
};
use crate::user_data::{DailyMix, DailyMixes, SavedAlbum, SortMode, Track, UserData};
use crate::window::PersistedWindowState;
use iced::Point;
use iced::keyboard::Modifiers;
use riff_audio::{AudioError, Event as AudioEvent, Source};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

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

fn app_with(data: UserData) -> App {
  App::new(
    PersistedWindowState::default(),
    NativeSettings::default(),
    I18n::default(),
  )
  .with_data(data)
}

fn app() -> App {
  let data = UserData {
    favorites: tracks(&["a", "b", "c"]),
    autoplay: false,
    ..UserData::default()
  };
  app_with(data)
}

fn ui(app: &mut App, message: UiMessage) -> Vec<Effect> {
  app.update(Message::Ui(message))
}

fn player(app: &mut App, message: PlayerMessage) -> Vec<Effect> {
  app.update(Message::Player(message))
}

fn audio(app: &mut App, event: AudioEvent) -> Vec<Effect> {
  app.update(Message::Audio(event))
}

fn current_id(app: &App) -> &str {
  app
    .player
    .current
    .as_ref()
    .and_then(|track| track.id.as_deref())
    .unwrap_or("")
}

fn resolved(effects: &[Effect]) -> Option<(u64, Option<u32>, Duration, bool)> {
  effects.iter().find_map(|effect| match effect {
    Effect::ResolveTrack {
      token,
      crossfade,
      start,
      options,
      ..
    } => Some((*token, *crossfade, *start, options.force_yt_dlp)),
    _ => None,
  })
}

/// `(force_yt_dlp, fresh)` de la résolution demandée.
fn modes(effects: &[Effect]) -> Option<(bool, bool)> {
  effects.iter().find_map(|effect| match effect {
    Effect::ResolveTrack { options, .. } => Some((options.force_yt_dlp, options.fresh)),
    _ => None,
  })
}

fn loaded(app: &mut App) {
  audio(
    app,
    AudioEvent::Loaded {
      duration: Some(Duration::from_secs(200)),
      sample_rate: 48_000,
    },
  );
}

fn play_first(app: &mut App) -> Vec<Effect> {
  ui(app, UiMessage::RowActivated(ListId::Favorites, 0))
}

fn source() -> Source {
  Source::File(PathBuf::from("a.mp3"))
}

#[test]
fn clicking_a_row_makes_the_list_the_queue_and_asks_for_the_stream() {
  let mut app = app();
  let effects = play_first(&mut app);
  assert_eq!(current_id(&app), "a");
  assert_eq!(app.player.queue.len(), 3);
  assert_eq!(app.player.index, Some(0));
  assert!(app.player.loading);
  let (token, crossfade, start, force) = resolved(&effects).unwrap();
  assert_eq!(
    (token, crossfade, start, force),
    (app.player.token, None, Duration::ZERO, false)
  );
  assert!(effects.contains(&Effect::AudioPause));
  assert!(effects.iter().any(|effect| matches!(effect, Effect::PublishAppState(state) if state["title"] == "Titre a" && state["isPlaying"] == false)));
  assert_eq!(app.data.history[0].id.as_deref(), Some("a"));
}

#[test]
fn the_resolved_stream_is_loaded_and_a_late_one_is_ignored() {
  let mut app = app();
  play_first(&mut app);
  let old = app.player.token;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 1));
  let start = Duration::ZERO;
  let late = PlayerMessage::Resolved {
    token: old,
    start,
    crossfade: None,
    result: Ok(source()),
  };
  assert_eq!(player(&mut app, late), Vec::new());
  let now = PlayerMessage::Resolved {
    token: app.player.token,
    start,
    crossfade: None,
    result: Ok(source()),
  };
  assert!(matches!(
    player(&mut app, now).as_slice(),
    [Effect::AudioLoadSource(_, _)]
  ));
}

#[test]
fn a_failed_resolution_stops_and_says_why() {
  let mut app = app();
  play_first(&mut app);
  let message = PlayerMessage::Resolved {
    token: app.player.token,
    start: Duration::ZERO,
    crossfade: None,
    result: Err("raison".to_string()),
  };
  let effects = player(&mut app, message);
  assert!(!app.player.loading);
  assert!(!app.playback.is_playing);
  assert!(matches!(effects.as_slice(), [Effect::Toast(text, _)] if text == "raison"));
}

#[test]
fn loaded_starts_playing_once_and_a_device_change_keeps_the_position() {
  let mut app = app();
  play_first(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(99)));
  assert_eq!(
    app.playback.position,
    Duration::ZERO,
    "position de l'ancien titre ignorée"
  );
  loaded(&mut app);
  assert!(app.playback.is_playing);
  assert!(!app.player.loading);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(42)));
  let effects = audio(
    &mut app,
    AudioEvent::Loaded {
      duration: Some(Duration::from_secs(200)),
      sample_rate: 44_100,
    },
  );
  assert_eq!(app.playback.position, Duration::from_secs(42));
  assert_eq!(effects, Vec::new());
}

#[test]
fn a_track_without_a_duration_gets_the_one_of_its_stream() {
  let file = Track {
    platform: Some("local".to_string()),
    path: Some("a.wav".to_string()),
    id: Some("l".to_string()),
    ..Track::default()
  };
  let mut app = app_with(UserData {
    local_files: vec![file],
    ..UserData::default()
  });
  ui(&mut app, UiMessage::RowActivated(ListId::Local, 0));
  loaded(&mut app);
  assert_eq!(app.data.local_files[0].duration, Some(200.0));
  assert_eq!(app.data.history[0].duration, Some(200.0));
  assert_eq!(app.player.queue[0].duration, Some(200.0));
  assert_eq!(app.player.current.as_ref().unwrap().duration, Some(200.0));
}

#[test]
fn the_end_of_a_track_plays_the_next_one() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  let effects = audio(&mut app, AudioEvent::Ended);
  assert_eq!(current_id(&app), "b");
  assert!(resolved(&effects).is_some());
}

#[test]
fn the_end_of_the_queue_without_autoplay_or_repeat_goes_back_to_the_start() {
  let mut app = app();
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  audio(&mut app, AudioEvent::Ended);
  assert_eq!(current_id(&app), "a");
}

#[test]
fn the_end_of_the_queue_with_repeat_reloads_the_same_track() {
  let mut app = app();
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  player(&mut app, PlayerMessage::ToggleRepeat);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  let effects = audio(&mut app, AudioEvent::Ended);
  assert_eq!(current_id(&app), "c");
  assert_eq!(resolved(&effects).unwrap().2, Duration::ZERO);
  assert_eq!(app.playback.position, Duration::ZERO);
}

#[test]
fn the_end_of_the_queue_with_autoplay_appends_tracks() {
  let mut app = app();
  app.data.autoplay = true;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  assert!(
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::PrepareAutoplay { .. }))
  );
  assert_eq!(
    audio(&mut app, AudioEvent::Ended),
    Vec::new(),
    "la préparation est déjà en cours : on l'attend"
  );
  assert_eq!(current_id(&app), "c");
  let effects = player(&mut app, PlayerMessage::AutoplayReady(tracks(&["a", "x"])));
  assert_eq!(
    current_id(&app),
    "x",
    "un titre déjà dans la file n'est pas repris"
  );
  assert!(app.player.radio);
  assert!(resolved(&effects).is_some());
}

#[test]
fn autoplay_proposes_neither_liked_nor_blocked_tracks() {
  let mut app = app();
  app.data.autoplay = true;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  audio(&mut app, AudioEvent::Ended);
  app.data.favorites.push(track("l"));
  app.data.toggle_block_track(&track("x"));
  player(
    &mut app,
    PlayerMessage::AutoplayReady(tracks(&["l", "x", "new"])),
  );
  assert_eq!(current_id(&app), "new");
}

#[test]
fn autoplay_without_a_result_loops_like_riff() {
  let mut app = app();
  app.data.autoplay = true;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  audio(&mut app, AudioEvent::Ended);
  player(&mut app, PlayerMessage::AutoplayReady(Vec::new()));
  assert_eq!(current_id(&app), "a");
}

#[test]
fn autoplay_prepared_in_advance_starts_at_once_at_the_end() {
  let mut app = app();
  app.data.autoplay = true;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(6)));
  player(&mut app, PlayerMessage::AutoplayReady(tracks(&["x"])));
  assert_eq!(current_id(&app), "c", "pas encore la fin du titre");
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  let effects = audio(&mut app, AudioEvent::Ended);
  assert_eq!(current_id(&app), "x");
  assert!(resolved(&effects).is_some());
}

#[test]
fn autoplay_is_prepared_after_five_seconds_on_the_last_track() {
  let mut app = app();
  app.data.autoplay = true;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(6)));
  assert!(
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::PrepareAutoplay { .. }))
  );
}

#[test]
fn the_next_track_is_prefetched_after_five_seconds() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(6)));
  assert!(effects.iter().any(|effect| matches!(effect, Effect::CoreJson { path: "/api/prefetch", query, .. } if query.contains(&("id".to_string(), "b".to_string())))));
}

#[test]
fn a_stream_cut_is_recovered_at_the_same_position() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(60)));
  let effects = audio(&mut app, AudioEvent::Ended);
  let (_, _, start, _) = resolved(&effects).unwrap();
  assert_eq!(start, Duration::from_secs(60));
  assert_eq!(modes(&effects), Some((false, true)));
  assert_eq!(current_id(&app), "a");
}

#[test]
fn a_premature_end_then_an_error_escalates_to_yt_dlp() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(60)));
  let effects = audio(&mut app, AudioEvent::Ended);
  assert_eq!(modes(&effects), Some((false, true)));
  let error = AudioEvent::Error(AudioError::Source("x".to_string()));
  assert_eq!(modes(&audio(&mut app, error)), Some((true, false)));
}

#[test]
fn an_engine_error_retries_twice_then_tells_the_user() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  let error = || AudioEvent::Error(AudioError::Source("x".to_string()));
  // Première reprise : résolution fraîche (innertube d'abord). Seconde : `yt-dlp` forcé, dernier recours.
  assert_eq!(modes(&audio(&mut app, error())), Some((false, true)));
  assert_eq!(modes(&audio(&mut app, error())), Some((true, false)));
  let effects = audio(&mut app, error());
  assert!(
    matches!(effects.as_slice(), [Effect::Toast(text, _)] if text == "Could not load audio stream")
  );
}

#[test]
fn a_missing_local_file_says_so() {
  let mut app = app();
  let local = Track {
    platform: Some("local".to_string()),
    path: Some("a.mp3".to_string()),
    ..track("l")
  };
  app.data.local_files = vec![local];
  app.refresh_orders();
  ui(&mut app, UiMessage::RowActivated(ListId::Local, 0));
  let effects = audio(
    &mut app,
    AudioEvent::Error(AudioError::Source("x".to_string())),
  );
  assert!(
    matches!(effects.as_slice(), [Effect::Toast(text, _)] if text == "File missing from library")
  );
}

#[test]
fn the_crossfade_starts_the_next_track_without_stopping_the_current_one() {
  let mut app = app();
  app.player.crossfade = 6;
  play_first(&mut app);
  loaded(&mut app);
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(194)));
  assert_eq!(current_id(&app), "b");
  assert_eq!(resolved(&effects).unwrap().1, Some(6));
  assert!(!effects.contains(&Effect::AudioPause));
  assert!(app.player.crossfade_pending);
  assert_eq!(
    audio(&mut app, AudioEvent::Ended),
    Vec::new(),
    "fin de l'ancien titre ignorée"
  );
  let message = PlayerMessage::Resolved {
    token: app.player.token,
    start: Duration::ZERO,
    crossfade: Some(6),
    result: Ok(source()),
  };
  assert!(matches!(
    player(&mut app, message).as_slice(),
    [Effect::AudioCrossfade(_, 6)]
  ));
  loaded(&mut app);
  assert!(!app.player.crossfade_pending);
}

#[test]
fn the_crossfade_also_applies_to_autoplay_prepared_in_advance() {
  let mut app = app();
  app.data.autoplay = true;
  app.player.crossfade = 6;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(6)));
  player(&mut app, PlayerMessage::AutoplayReady(tracks(&["x"])));
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(194)));
  assert_eq!(current_id(&app), "x");
  assert_eq!(resolved(&effects).unwrap().1, Some(6));
  assert!(!effects.contains(&Effect::AudioPause));
}

#[test]
fn the_crossfade_waits_for_autoplay_results() {
  let mut app = app();
  app.data.autoplay = true;
  app.player.crossfade = 6;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(194)));
  assert_eq!(
    current_id(&app),
    "c",
    "le titre continue pendant la recherche"
  );
  audio(&mut app, AudioEvent::Ended);
  let effects = player(&mut app, PlayerMessage::AutoplayReady(tracks(&["x"])));
  assert_eq!(current_id(&app), "x");
  assert_eq!(resolved(&effects).unwrap().1, Some(6));
  assert!(!effects.contains(&Effect::AudioPause));
}

#[test]
fn the_crossfade_also_applies_to_the_radio_refill() {
  let mut app = app();
  app.player.crossfade = 6;
  app.ui.home.wave = tracks(&["w"]);
  player(&mut app, PlayerMessage::PlayMyWave);
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(194)));
  assert!(app.ui.radio_waiting);
  let reply = json!({ "tracks": tracks(&["w", "new"]) });
  let effects = ui(&mut app, UiMessage::Reply(ReplyTag::RadioRefill, Ok(reply)));
  assert_eq!(current_id(&app), "new");
  assert_eq!(resolved(&effects).unwrap().1, Some(6));
  assert!(!effects.contains(&Effect::AudioPause));
}

#[test]
fn a_manual_choice_cancels_the_crossfade_in_waiting() {
  let mut app = app();
  app.data.autoplay = true;
  app.player.crossfade = 6;
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(194)));
  let effects = play_first(&mut app);
  assert_eq!(resolved(&effects).unwrap().1, None);
  loaded(&mut app);
  player(&mut app, PlayerMessage::AutoplayReady(tracks(&["x"])));
  assert_eq!(
    current_id(&app),
    "a",
    "une réponse tardive ne remplace pas le choix de l'utilisateur"
  );
  app.player.crossfade = 0;
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  let effects = audio(&mut app, AudioEvent::Ended);
  assert_eq!(current_id(&app), "b");
  assert_eq!(
    resolved(&effects).unwrap().1,
    None,
    "fin normale, sans fondu resté en attente"
  );
}

#[test]
fn a_manual_choice_cancels_a_radio_refill_in_waiting() {
  let mut app = app();
  app.ui.home.wave = tracks(&["w"]);
  player(&mut app, PlayerMessage::PlayMyWave);
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  audio(&mut app, AudioEvent::Ended);
  assert!(app.ui.radio_waiting);
  play_first(&mut app);
  assert!(!app.ui.radio_waiting);
}

#[test]
fn previous_after_three_seconds_seeks_to_the_start() {
  let mut app = app();
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 1));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(10)));
  assert_eq!(
    player(&mut app, PlayerMessage::Prev),
    vec![Effect::AudioSeek(Duration::ZERO),]
  );
  assert_eq!(current_id(&app), "b");
  let effects = player(&mut app, PlayerMessage::Prev);
  assert_eq!(current_id(&app), "a");
  assert!(resolved(&effects).is_some());
}

#[test]
fn previous_skips_the_blocked_tracks_of_the_history() {
  let mut app = app();
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 0));
  player(&mut app, PlayerMessage::Next);
  player(&mut app, PlayerMessage::Next);
  assert_eq!(current_id(&app), "c");
  app.data.toggle_block_track(&track("b"));
  player(&mut app, PlayerMessage::Prev);
  assert_eq!(current_id(&app), "a");
}

#[test]
fn previous_skips_the_blocked_tracks_of_the_queue() {
  let mut app = app();
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  app.data.toggle_block_track(&track("b"));
  player(&mut app, PlayerMessage::Prev);
  assert_eq!(current_id(&app), "a");
}

#[test]
fn previous_does_nothing_when_every_other_track_is_blocked() {
  let mut app = app();
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  app.data.toggle_block_track(&track("a"));
  app.data.toggle_block_track(&track("b"));
  assert_eq!(player(&mut app, PlayerMessage::Prev), Vec::new());
  assert_eq!(current_id(&app), "c");
}

#[test]
fn shuffle_and_repeat_are_remembered() {
  let mut app = app();
  let effects = player(&mut app, PlayerMessage::ToggleShuffle);
  assert!(app.player.shuffle && app.data.shuffle);
  assert!(matches!(effects.as_slice(), [Effect::ScheduleSave(_)]));
  player(&mut app, PlayerMessage::ToggleRepeat);
  assert!(app.player.repeat && app.data.repeat);
}

#[test]
fn play_pause_toggles_and_without_a_track_starts_the_radio() {
  let mut app = app();
  assert_eq!(player(&mut app, PlayerMessage::TogglePlay), Vec::new());
  app.ui.home.wave = tracks(&["w", "x"]);
  player(&mut app, PlayerMessage::TogglePlay);
  assert_eq!(current_id(&app), "w");
  assert!(app.player.radio);
  loaded(&mut app);
  let effects = player(&mut app, PlayerMessage::TogglePlay);
  assert!(effects.contains(&Effect::AudioPause));
  assert!(!app.playback.is_playing);
  let effects = player(&mut app, PlayerMessage::TogglePlay);
  assert!(effects.contains(&Effect::AudioPlay));
  assert!(app.playback.is_playing);
}

#[test]
fn seeking_by_keyboard_stays_inside_the_track() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(3)));
  assert_eq!(
    player(&mut app, PlayerMessage::SeekBy(-5.0)),
    vec![Effect::AudioSeek(Duration::ZERO),]
  );
  audio(&mut app, AudioEvent::Position(Duration::from_secs(198)));
  assert_eq!(
    player(&mut app, PlayerMessage::SeekBy(5.0)),
    vec![Effect::AudioSeek(Duration::from_secs(200)),]
  );
}

#[test]
fn volume_and_mute_reach_the_engine_and_are_saved() {
  let mut app = app();
  let effects = player(&mut app, PlayerMessage::Volume(0.5));
  assert_eq!(effects[0], Effect::AudioMix(0.5, false));
  player(&mut app, PlayerMessage::VolumeBy(1.0));
  assert!((app.data.volume - 0.55).abs() < 1e-6);
  let effects = player(&mut app, PlayerMessage::ToggleMute);
  assert!(app.data.muted);
  assert_eq!(effects[0], Effect::AudioMix(app.data.volume, true));
  player(&mut app, PlayerMessage::Volume(0.2));
  assert!(!app.data.muted, "bouger le volume rétablit le son");
}

#[test]
fn normalization_follows_the_level_only_when_on_and_playing() {
  let mut app = app();
  assert_eq!(player(&mut app, PlayerMessage::Level(-20.0)), Vec::new());
  app.data.normalize = true;
  assert_eq!(player(&mut app, PlayerMessage::Level(-20.0)), Vec::new());
  play_first(&mut app);
  loaded(&mut app);
  assert_eq!(
    player(&mut app, PlayerMessage::Level(-20.0)),
    vec![Effect::NormalizeGain(3.0)]
  );
  assert_eq!(player(&mut app, PlayerMessage::Level(-60.0)), Vec::new());
  let effects = ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 1));
  assert!(
    effects.contains(&Effect::NormalizeGain(0.0)),
    "nouveau titre : mesure remise à zéro"
  );
}

#[test]
fn a_cover_is_only_fetched_for_the_theme_in_auto_mode() {
  let mut app = app();
  app.data.favorites[0].thumbnail = Some("https://x/y.jpg".to_string());
  app.refresh_orders();
  let effects = play_first(&mut app);
  assert!(
    !effects
      .iter()
      .any(|effect| matches!(effect, Effect::FetchCover(_)))
  );
  app.settings.theme_mode = crate::settings::ThemeMode::Auto;
  let effects = play_first(&mut app);
  assert!(effects.contains(&Effect::FetchCover("https://x/y.jpg".to_string())));
}

#[test]
fn every_change_of_mesh_key_restarts_the_float_but_the_same_key_does_not() {
  use crate::prefs::SettingsMessage;
  let mut app = app();
  app.settings.visual.mesh_gradient = true;
  app.settings.visual.mesh.speed = 20.0;
  let start = Instant::now();
  app.visuals.mesh_key = None;
  app.sync_mesh(start);
  assert_eq!(app.visuals.mesh_started, Some(start));
  // Même clé : `updateMeshGradient` rend la main avant de toucher aux calques (`app.js:1696`).
  app.sync_mesh(start + Duration::from_secs(12));
  assert_eq!(app.visuals.mesh_started, Some(start));
  // Nouvelle couleur : l'autre calque sort de `display: none`, `meshFloat` repart de 0 %.
  app.update(Message::Settings(SettingsMessage::AccentInput(
    "#d14752".to_string(),
  )));
  let restarted = app.visuals.mesh_started.expect("calque affiché");
  assert!(restarted > start, "le départ est celui du changement");
  // Mode, luminosité, suivi de l'intensité : chacun fait partie de `meshKey` (`app.js:1695`).
  let key = app.visuals.mesh_key;
  app.settings.visual.mesh_brightness = 60;
  app.sync_mesh(restarted + Duration::from_secs(1));
  assert_ne!(app.visuals.mesh_key, key);
  assert_eq!(
    app.visuals.mesh_started,
    Some(restarted + Duration::from_secs(1))
  );
  app.settings.visual.mesh_follow_intensity = false;
  app.sync_mesh(restarted + Duration::from_secs(2));
  assert_eq!(
    app.visuals.mesh_started,
    Some(restarted + Duration::from_secs(2))
  );
  app.settings.visual.mesh_gradient = false;
  app.sync_mesh(restarted + Duration::from_secs(3));
  assert!(app.visuals.mesh_started.is_none() && app.visuals.mesh_key.is_none());
}

#[test]
fn a_track_without_a_cover_asks_for_one_and_applies_it() {
  let mut app = app();
  app.data.favorites[0].id = Some("sans-pochette".to_string());
  app.refresh_orders();
  let effects = play_first(&mut app);
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/track-cover",
      ..
    }
  )));
  let token = app.player.token;
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::TrackCover { token },
      Ok(json!({ "cover": "c.png" })),
    ),
  );
  assert_eq!(
    app.player.current.as_ref().unwrap().thumbnail.as_deref(),
    Some("c.png")
  );
}

#[test]
fn saving_waits_for_the_last_change() {
  let mut app = app();
  let first = player(&mut app, PlayerMessage::ToggleShuffle);
  let second = player(&mut app, PlayerMessage::ToggleRepeat);
  let [Effect::ScheduleSave(old)] = first.as_slice() else {
    panic!()
  };
  let [Effect::ScheduleSave(new)] = second.as_slice() else {
    panic!()
  };
  assert_eq!(ui(&mut app, UiMessage::SaveDue(*old)), Vec::new());
  assert!(matches!(
    ui(&mut app, UiMessage::SaveDue(*new)).as_slice(),
    [Effect::WriteUserData(_)]
  ));
}

#[test]
fn closing_writes_the_data_at_once() {
  let mut app = app();
  let effects = app.update(Message::CloseStateKnown {
    is_maximized: false,
    is_full_screen: false,
  });
  assert!(
    matches!(effects.as_slice(), [Effect::SaveWindowAndExit(_, Some(data))] if data.favorites.len() == 3)
  );
}

fn searching(app: &mut App, text: &str) -> Vec<Effect> {
  ui(app, UiMessage::SearchInput(text.to_string()));
  ui(app, UiMessage::SearchSubmit)
}

#[test]
fn a_search_cancels_the_previous_one_and_opens_the_results() {
  let mut app = app();
  let effects = searching(&mut app, "daft punk");
  assert_eq!(app.screen, Screen::Search);
  assert_eq!(effects[0], Effect::AbortSearch);
  let calls: Vec<&str> = effects
    .iter()
    .filter_map(|effect| match effect {
      Effect::CoreJson {
        path, search: true, ..
      } => Some(*path),
      _ => None,
    })
    .collect();
  assert_eq!(calls, ["/api/search", "/api/albums/search"]);
  assert!(effects.iter().any(|effect| matches!(effect, Effect::CoreJson { query, .. } if query.contains(&("platform".to_string(), "ytmusic".to_string())))));
}

#[test]
fn a_short_query_does_not_ask_for_albums() {
  let mut app = app();
  let effects = searching(&mut app, "ab");
  assert!(!effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/albums/search",
      ..
    }
  )));
}

fn results(ids: &[&str]) -> Value {
  json!({ "success": true, "hasMore": true, "tracks": tracks(ids) })
}

#[test]
fn a_late_answer_of_an_old_search_is_ignored() {
  let mut app = app();
  searching(&mut app, "premier");
  let old = app.ui.search.token;
  searching(&mut app, "second");
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token: old }, Ok(results(&["x"]))),
  );
  assert!(app.ui.search.tracks.is_empty());
  assert!(app.ui.search.loading);
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&["y"]))),
  );
  assert_eq!(app.ui.search.tracks[0].id.as_deref(), Some("y"));
  assert!(!app.ui.search.loading);
  assert_eq!(app.data.search_history[0].title.as_deref(), Some("second"));
}

#[test]
fn a_search_starts_the_shared_spinner_loop_and_asks_for_frames_until_the_answer() {
  let mut app = app();
  app.screen = Screen::Search;
  assert!(!app.animation_needed());
  searching(&mut app, "premier");
  assert!(app.shape_loop.shown());
  assert!(app.animation_needed());
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Err("coupure".into())),
  );
  // L'erreur garde la forme qui tourne : la boucle reste en marche et les images continuent.
  assert!(app.shape_loop.shown());
  assert!(app.animation_needed());
  searching(&mut app, "second");
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&["y"]))),
  );
  assert!(!app.shape_loop.shown());
}

#[test]
fn a_new_search_keeps_the_loop_steps_while_a_spinner_is_still_shown() {
  let mut app = app();
  app.screen = Screen::Search;
  searching(&mut app, "premier");
  let first = app.shape_loop;
  std::thread::sleep(Duration::from_millis(3));
  // Relancée pendant l'attente : le nœud est recréé mais `stepStart` de `startButtonShapeSpinners` continue (`app.js:9946-9956`).
  searching(&mut app, "deuxième");
  assert_eq!(app.shape_loop, first);
  // Idem quand le loader reste affiché sur l'erreur.
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Err("coupure".into())),
  );
  searching(&mut app, "troisième");
  assert_eq!(app.shape_loop, first);
  // Résultats affichés : plus aucun spinner, la boucle s'arrête ; un temps mort court garde le pas en cours.
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&["y"]))),
  );
  assert!(!app.shape_loop.shown());
  searching(&mut app, "quatrième");
  assert!(app.shape_loop.shown());
}

#[test]
fn the_play_spinner_and_the_search_loader_share_one_loop() {
  let mut app = app();
  let t0 = Instant::now();
  app.now = t0;
  app.play_wait.track(true, t0);
  app.sync_shape_loop(t0 + Duration::from_millis(499));
  assert!(!app.shape_loop.shown());
  app.sync_shape_loop(t0 + Duration::from_millis(500));
  assert!(app.shape_loop.shown());
  // Le loader apparaît pendant que le spinner de lecture tourne : même boucle, aucun nouveau pas.
  let running = app.shape_loop;
  app.ui.search.loading = true;
  app.sync_shape_loop(t0 + Duration::from_millis(900));
  assert_eq!(app.shape_loop, running);
  // La boucle ne s'arrête qu'avec le dernier des deux.
  app.play_wait.track(false, t0 + Duration::from_millis(1000));
  app.sync_shape_loop(t0 + Duration::from_millis(1000));
  assert!(app.shape_loop.shown());
  app.ui.search.loading = false;
  app.sync_shape_loop(t0 + Duration::from_millis(1100));
  assert!(!app.shape_loop.shown());
}

#[test]
fn search_results_page_when_scrolling_near_the_end() {
  let mut app = app();
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&["a", "b"]))),
  );
  let effects = ui(
    &mut app,
    UiMessage::ListScrolled(ListId::Search, 0.0, 500.0),
  );
  assert!(effects.iter().any(|effect| matches!(effect, Effect::CoreJson { query, .. } if query.contains(&("offset".to_string(), "2".to_string())))));
  assert_eq!(
    ui(
      &mut app,
      UiMessage::ListScrolled(ListId::Search, 0.0, 500.0)
    ),
    Vec::new(),
    "une seule page à la fois"
  );
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::SearchMore { token },
      Ok(json!({ "tracks": tracks(&["c"]), "hasMore": false })),
    ),
  );
  assert_eq!(app.ui.search.tracks.len(), 3);
  assert!(!app.ui.search.has_more);
}

#[test]
fn a_failed_search_shows_its_error() {
  let mut app = app();
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  let effects = ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::Search { token },
      Ok(json!({ "success": false, "error": "panne" })),
    ),
  );
  assert_eq!(app.ui.search.error.as_deref(), Some("panne"));
  assert!(
    matches!(effects.as_slice(), [Effect::Toast(text, _)] if text == "YouTube Music is unavailable")
  );
}

#[test]
fn an_offline_search_shows_the_message_without_the_unavailable_toast() {
  let mut app = app();
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  let effects = ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::Search { token },
      Ok(json!({ "success": false, "error": "Offline: no internet connection", "offline": true })),
    ),
  );
  assert_eq!(
    app.ui.search.error.as_deref(),
    Some("Offline: no internet connection")
  );
  assert!(effects.is_empty());
}
#[test]
fn clearing_the_search_goes_home() {
  let mut app = app();
  searching(&mut app, "daft");
  let effects = ui(&mut app, UiMessage::SearchInput(String::new()));
  assert_eq!(app.screen, Screen::Home);
  assert!(effects.contains(&Effect::AbortSearch));
}

#[test]
fn changing_the_platform_searches_again() {
  let mut app = app();
  searching(&mut app, "daft");
  let effects = ui(&mut app, UiMessage::SetPlatform("soundcloud".to_string()));
  assert_eq!(app.data.platform, "soundcloud");
  assert!(effects.iter().any(|effect| matches!(effect, Effect::CoreJson { query, .. } if query.contains(&("platform".to_string(), "soundcloud".to_string())))));
}

#[test]
fn playing_a_result_records_it_in_the_search_history() {
  let mut app = app();
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&["a", "b"]))),
  );
  ui(&mut app, UiMessage::RowActivated(ListId::Search, 1));
  assert_eq!(current_id(&app), "b");
  assert_eq!(app.data.search_history[0].kind.as_deref(), Some("track"));
  assert_eq!(app.player.queue.len(), 2);
}

#[test]
fn the_history_panel_opens_on_the_field_and_closes_elsewhere() {
  let mut app = app();
  ui(&mut app, UiMessage::SearchFocus);
  assert!(app.ui.search.history_open);
  ui(&mut app, UiMessage::PressedElsewhere);
  assert!(!app.ui.search.history_open);
  ui(&mut app, UiMessage::FocusSearch);
  assert!(app.ui.search.history_open);
  app.update(Message::Navigate(Screen::Queue));
  assert!(!app.ui.search.history_open, "changer d'écran le ferme");
}

#[test]
fn the_history_hover_follows_the_pointer_and_resets_when_the_list_changes() {
  let mut app = app();
  ui(&mut app, UiMessage::HistoryHover(Some(2)));
  assert_eq!(app.ui.search.hover, Some(2));
  ui(&mut app, UiMessage::HistoryHover(None));
  assert_eq!(app.ui.search.hover, None);
  ui(&mut app, UiMessage::HistoryHover(Some(1)));
  ui(&mut app, UiMessage::SearchInput("a".to_string()));
  assert_eq!(app.ui.search.hover, None, "le filtre change les lignes");
  ui(&mut app, UiMessage::HistoryHover(Some(1)));
  ui(&mut app, UiMessage::PressedElsewhere);
  assert_eq!(app.ui.search.hover, None);
}

#[test]
fn removing_and_clearing_history_rows_follow_the_filtered_list() {
  let mut app = app();
  app.data.record_search_query("alpha", None, 1);
  app.data.record_search_query("beta", None, 2);
  app.data.record_search_query("alphabet", None, 3);
  ui(&mut app, UiMessage::SearchInput("alpha".to_string()));
  // Lignes visibles : « alphabet » puis « alpha » ; l'indice 1 vise « alpha », pas « beta ».
  ui(&mut app, UiMessage::HistoryRemove(1));
  let titles: Vec<_> = app
    .data
    .search_history
    .iter()
    .filter_map(|entry| entry.title.as_deref())
    .collect();
  assert_eq!(titles, ["alphabet", "beta"]);
  ui(&mut app, UiMessage::HistoryClear);
  assert!(app.data.search_history.is_empty());
  assert!(!app.ui.search.history_open, "« Clear » ferme le panneau");
}

#[test]
fn a_history_entry_reopens_its_query_track_or_artist() {
  let mut app = app();
  app.data.record_search_query("ma requete", None, 1);
  ui(&mut app, UiMessage::HistoryOpen(0));
  assert_eq!(app.ui.search.query, "ma requete");
  app.ui.search.input.clear();
  app.data.search_history.clear();
  app.data.record_search_artist("Justice", None, false, 1);
  ui(&mut app, UiMessage::HistoryOpen(0));
  assert_eq!(app.screen, Screen::Artist);
  assert_eq!(app.ui.artist.name, "Justice");
  app.data.search_history.clear();
  app.data.record_search_track(&track("z"), 1);
  ui(&mut app, UiMessage::HistoryOpen(0));
  assert_eq!(current_id(&app), "z");
}

#[test]
fn the_artist_card_needs_a_known_and_matching_artist() {
  let mut app = app();
  searching(&mut app, "justice");
  let token = app.ui.search.token;
  let reply = |name: &str, known: bool| {
    Ok(json!({ "success": true, "artist": { "name": name, "isKnownArtist": known, "fans": 12 } }))
  };
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::SearchArtist {
        token,
        via_track: false,
      },
      reply("Justice", false),
    ),
  );
  assert!(app.ui.search.artist.is_none());
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::SearchArtist {
        token,
        via_track: false,
      },
      reply("Quelqu'un d'autre", true),
    ),
  );
  assert!(app.ui.search.artist.is_none());
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::SearchArtist {
        token,
        via_track: false,
      },
      reply("Justice", true),
    ),
  );
  assert_eq!(app.ui.search.artist.as_ref().unwrap().fans, 12.0);
  let effects = ui(&mut app, UiMessage::OpenSearchArtist);
  assert_eq!(app.screen, Screen::Artist);
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/artist",
      ..
    }
  )));
}

#[test]
fn followed_artists_without_an_image_get_one() {
  let mut app = app();
  app.data.toggle_follow("Muse", None, None);
  let effects = app.update(Message::WindowOpened {
    position: None,
    size: iced::Size::new(800.0, 600.0),
  });
  assert!(effects.iter().any(|effect| matches!(effect, Effect::CoreJson { tag: ReplyTag::ArtistMeta { name }, .. } if name == "Muse")));
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::ArtistMeta {
        name: "Muse".to_string(),
      },
      Ok(json!({ "artist": { "banner": "b.png", "fans": 4 } })),
    ),
  );
  assert_eq!(
    app.data.favorite_artists[0].avatar.as_deref(),
    Some("b.png")
  );
  assert_eq!(app.data.favorite_artists[0].fans, Some(4.0));
}

#[test]
fn the_home_loads_after_the_first_screen_only_once() {
  let mut app = app();
  let effects = app.update(Message::WindowOpened {
    position: None,
    size: iced::Size::new(800.0, 600.0),
  });
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/vibe",
      ..
    }
  )));
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/trending",
      ..
    }
  )));
  let again = app.update(Message::WindowOpened {
    position: None,
    size: iced::Size::new(800.0, 600.0),
  });
  assert!(!again.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/vibe",
      ..
    }
  )));
}

#[test]
fn few_recommendations_fall_back_on_hot_picks() {
  let mut app = app();
  let effects = ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::Vibe { refresh: false },
      Ok(json!({ "success": true, "tracks": tracks(&["a"]) })),
    ),
  );
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/hot",
      ..
    }
  )));
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::Hot,
      Ok(json!({ "success": true, "tracks": tracks(&["h1", "h2"]) })),
    ),
  );
  assert_eq!(app.ui.home.wave.len(), 2);
  assert!(!app.ui.home.wave_loading);
}

#[test]
fn recommendations_are_unique_cached_and_exclude_blocked_artists() {
  let mut app = app();
  app.data.toggle_block_artist("Artiste z");
  let list = tracks(&["1", "2", "3", "4", "5", "z", "1"]);
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::Vibe { refresh: false },
      Ok(json!({ "success": true, "tracks": list })),
    ),
  );
  assert_eq!(app.ui.home.wave.len(), 5);
  assert_eq!(app.data.wave_cache.as_ref().unwrap().tracks.len(), 5);
}

fn blocked_home() -> App {
  let mut app = app();
  app.ui.home.wave = tracks(&["w1", "w2", "w3"]);
  app.ui.home.trending = tracks(&["t1", "w2"]);
  app.data.history = tracks(&["w2", "h1"]);
  app.data.daily_mixes = Some(DailyMixes {
    day: Some("2026-11-03".into()),
    mixes: vec![DailyMix {
      id: Some("daily-mix-1".into()),
      tracks: tracks(&["m1", "w2", "m2", "m3", "m4", "m5", "m6", "m7", "m8"]),
      artists: vec!["Artiste w2".into(), "Autre".into()],
      ..DailyMix::default()
    }],
  });
  app
}

fn assert_w2_is_gone(app: &App) {
  assert_eq!(ids(&app.ui.home.wave), ["w1", "w3"]);
  assert_eq!(ids(&app.ui.home.trending), ["t1"]);
  let mixes = app.data.daily_mixes.as_ref().unwrap();
  assert_eq!(
    ids(&mixes.mixes[0].tracks),
    ["m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8"]
  );
  let recent: Vec<&str> = app
    .recent_tracks()
    .iter()
    .map(|track| track.id.as_deref().unwrap_or(""))
    .collect();
  assert_eq!(recent, ["h1"]);
}

#[test]
fn blocking_a_track_removes_it_from_every_home_list() {
  let mut app = blocked_home();
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("w2")), TrackCommand::BlockTrack),
  );
  assert_w2_is_gone(&app);
}

#[test]
fn blocking_an_artist_from_a_track_removes_its_tracks_from_every_home_list() {
  let mut app = blocked_home();
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("w2")), TrackCommand::BlockArtist),
  );
  assert_w2_is_gone(&app);
}

#[test]
fn blocking_an_artist_from_its_page_removes_its_tracks_from_every_home_list() {
  let mut app = blocked_home();
  app.ui.artist.name = "Artiste w2".to_string();
  ui(&mut app, UiMessage::ArtistBlock);
  assert_w2_is_gone(&app);
}

#[test]
fn a_daily_mix_shrunk_below_the_minimum_by_a_block_is_dropped() {
  let mut app = blocked_home();
  app.data.daily_mixes.as_mut().unwrap().mixes[0].tracks = tracks(&["m1", "w2", "m2"]);
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("w2")), TrackCommand::BlockTrack),
  );
  assert!(app.data.daily_mixes.as_ref().unwrap().mixes.is_empty());
}

#[test]
fn a_blocked_artist_leaves_the_subtitle_of_the_daily_mixes() {
  let mut app = blocked_home();
  app.ui.artist.name = "Artiste w2".to_string();
  ui(&mut app, UiMessage::ArtistBlock);
  let mixes = app.data.daily_mixes.as_ref().unwrap();
  assert_eq!(mixes.mixes[0].artists, ["Autre"]);
}

#[test]
fn the_quick_tiles_never_show_a_blocked_track() {
  let mut app = blocked_home();
  app.data.toggle_block_track(&track("w2"));
  let recent = app.recent_tracks();
  assert_eq!(recent.len(), 1);
  assert_eq!(recent[0].id.as_deref(), Some("h1"));
}

#[test]
fn the_radio_never_starts_on_a_blocked_card() {
  let mut app = app();
  app.ui.home.wave = tracks(&["w1", "w2", "w3"]);
  app.data.toggle_block_track(&track("w1"));
  player(&mut app, PlayerMessage::PlayMyWave);
  assert_eq!(current_id(&app), "w2");
  assert_eq!(ids(&app.player.queue), ["w2", "w3"]);
}

#[test]
fn the_radio_does_not_start_when_every_card_is_blocked() {
  let mut app = app();
  app.ui.home.wave = tracks(&["w1"]);
  app.data.toggle_block_track(&track("w1"));
  player(&mut app, PlayerMessage::PlayMyWave);
  assert!(app.player.current.is_none());
}

#[test]
fn nothing_at_all_shows_the_retry_message() {
  let mut app = app();
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Vibe { refresh: false }, Err("panne".to_string())),
  );
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Hot, Err("panne".to_string())),
  );
  assert!(app.ui.home.wave_failed);
}

#[test]
fn refreshing_the_home_asks_again() {
  let mut app = app();
  let effects = ui(&mut app, UiMessage::HomeRefresh);
  assert!(effects.iter().any(|effect| matches!(effect, Effect::CoreJson { path: "/api/vibe", body: Some(body), .. } if body.contains("\"refresh\":true"))));
  // `#btn-regen-mixes` ne relance pas les tendances.
  assert!(!effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/trending",
      ..
    }
  )));
}

#[test]
fn refreshing_the_home_drops_the_mixes_unless_they_are_being_generated() {
  let mut app = app();
  app.data.daily_mixes = Some(DailyMixes {
    day: Some("2026-11-17".to_string()),
    mixes: vec![DailyMix::default()],
  });
  app.ui.home.mixes_busy = true;
  ui(&mut app, UiMessage::HomeRefresh);
  assert_eq!(app.data.daily_mixes.as_ref().unwrap().mixes.len(), 1);
  app.ui.home.mixes_busy = false;
  ui(&mut app, UiMessage::HomeRefresh);
  assert!(
    app
      .data
      .daily_mixes
      .as_ref()
      .is_none_or(|mixes| mixes.mixes.is_empty())
  );
}

#[test]
fn the_home_spinners_ask_for_frames_only_while_one_is_shown() {
  let mut app = launched_on(Screen::Home);
  app.home_spinner.enabled = true;
  app.trending_spinner.enabled = true;
  app.ui.home.trending.clear();
  app.ui.home.trending_loading = true;
  app.update(Message::Navigate(Screen::Home));
  let start = app.trending_spinner.start.expect("spinner des tendances");
  assert_eq!(app.home_spinner.start, None, "aucune carte réservée");
  assert!(app.animation_needed());
  // Elle reste posée tant qu'un spinner est affiché, même mise à jour d'une autre donnée.
  app.update(Message::Navigate(Screen::Home));
  assert_eq!(app.trending_spinner.start, Some(start));
  // Quitter l'accueil masque la vue (`display: none`) : le départ est effacé et l'abonnement s'arrête.
  app.update(Message::Navigate(Screen::Library));
  assert_eq!(app.trending_spinner.start, None);
  app.now += Duration::from_secs(5);
  assert!(!app.animation_needed());
  // Les tendances arrivées : plus aucun spinner, l'abonnement reste arrêté.
  app.ui.home.trending_loading = false;
  app.update(Message::Navigate(Screen::Home));
  assert_eq!(app.trending_spinner.start, None);
  app.now += Duration::from_secs(5);
  assert!(!app.animation_needed());
}

#[test]
fn each_home_spinner_restarts_when_riff_recreates_its_node() {
  let pause = || std::thread::sleep(Duration::from_millis(3));
  let mut app = launched_on(Screen::Home);
  app.home_spinner.enabled = true;
  app.trending_spinner.enabled = true;
  app.ui.home.wave.clear();
  app.ui.home.trending.clear();
  app.ui.home.trending_loading = true;
  app.ui.home.mixes_busy = true;
  app.ui.home.mixes_planned = 3;
  app.data.daily_mixes = Some(DailyMixes::default());
  // `renderSpotifyHome` insère les deux familles dans le même rendu : départ commun.
  app.update(Message::Navigate(Screen::Home));
  let first = app.home_spinner.start.expect("cartes réservées");
  assert_eq!(app.trending_spinner.start, Some(first));
  // Un mix arrivé relance `renderMadeForYou` : les cartes repartent, les tendances gardent leur nœud.
  pause();
  let ready = UiMessage::MixReady {
    day: "2026-11-03".into(),
    id: "daily-mix-1".into(),
    title: "Daily Mix 1".into(),
    artists: vec!["A".into()],
    tracks: tracks(&["1", "2", "3", "4", "5", "6", "7", "8"]),
  };
  ui(&mut app, ready);
  let second = app.home_spinner.start.expect("cartes réservées");
  assert!(second > first, "recréées");
  assert_eq!(app.trending_spinner.start, Some(first), "gardé");
  // Un évènement sans rendu des cartes ne touche à aucun départ.
  pause();
  app.update(Message::Navigate(Screen::Home));
  assert_eq!(app.home_spinner.start, Some(second));
  assert_eq!(app.trending_spinner.start, Some(first));
  // La fin de la génération rend les cartes une dernière fois : la carte « For you » sans vague repart.
  pause();
  ui(&mut app, UiMessage::MixesDone);
  let third = app.home_spinner.start.expect("carte « For you »");
  assert!(third > second);
  assert_eq!(app.trending_spinner.start, Some(first));
  // Quitter l'accueil puis y revenir recrée tout (`display: none`), au même instant.
  app.update(Message::Navigate(Screen::Library));
  pause();
  app.update(Message::Navigate(Screen::Home));
  let back = app.home_spinner.start.expect("retour");
  assert!(back > third);
  assert_eq!(app.trending_spinner.start, Some(back));
}

#[test]
fn only_the_card_under_the_pointer_clears_the_hover() {
  use crate::ui::HomeRow;
  let mut app = app();
  ui(&mut app, UiMessage::HomeHover(HomeRow::Made, 3, true));
  ui(&mut app, UiMessage::HomeHover(HomeRow::Made, 1, true));
  // La sortie de la carte 3 arrive après l'entrée sur la carte 1 : elle ne doit rien effacer.
  ui(&mut app, UiMessage::HomeHover(HomeRow::Made, 3, false));
  assert_eq!(app.ui.home.hover, Some((HomeRow::Made, 1)));
  ui(&mut app, UiMessage::HomeHover(HomeRow::Made, 1, false));
  assert_eq!(app.ui.home.hover, None);
}

#[test]
fn card_hover_lifts_the_cover_over_180_ms_then_the_timer_stops() {
  use crate::ui::HomeRow;
  let mut app = app();
  let key = (HomeRow::Trending, 2);
  assert!(!app.animation_needed());
  ui(&mut app, UiMessage::HomeHover(key.0, key.1, true));
  let start = app.now;
  assert!(app.animation_needed());
  assert_eq!(app.ui.home.lift.value(key, start, 0.0), 0.0);
  assert_eq!(
    app
      .ui
      .home
      .lift
      .value(key, start + Duration::from_millis(180), 0.0),
    1.0
  );
  app.now = start + Duration::from_millis(181);
  assert!(!app.animation_needed(), "posée : l'abonnement s'arrête");
  // Sortie aussitôt après l'entrée : la piste repart de la valeur courante (~0), sans saut.
  ui(&mut app, UiMessage::HomeHover(key.0, key.1, false));
  let leave = app.now;
  assert!(app.ui.home.lift.value(key, leave, 1.0) < 0.05);
  app.now = leave + Duration::from_millis(181);
  assert_eq!(app.ui.home.lift.value(key, app.now, 1.0), 0.0);
  assert!(!app.animation_needed());
}

#[test]
fn control_hover_animates_color_and_radius_separately() {
  let mut app = app();
  ui(&mut app, UiMessage::ControlHover("queue".into(), true));
  let start = app.now;
  let at = |ms| start + Duration::from_millis(ms);
  assert!(app.ui.controls.mix("queue", start).hovered);
  assert!(app.animation_needed());
  // `.18s` pour le fond, `.22s` pour le rayon.
  assert_eq!(app.ui.controls.mix("queue", at(180)).color, 1.0);
  assert!(app.ui.controls.mix("queue", at(180)).shape < 1.0);
  assert_eq!(app.ui.controls.mix("queue", at(220)).shape, 1.0);
  app.now = at(221);
  assert!(!app.animation_needed());
  ui(&mut app, UiMessage::ControlHover("queue".into(), false));
  assert!(!app.ui.controls.mix("queue", app.now).hovered);
}

#[test]
fn a_hovered_control_that_leaves_the_view_is_forgotten_before_the_next_message() {
  let mut app = app();
  ui(&mut app, UiMessage::ControlHover("queue".into(), true));
  assert!(app.ui.controls.mix("queue", app.now).hovered);
  // `Drop` du widget survolé retiré de l'arbre : aucune sortie du pointeur ne viendra.
  crate::motion::mark_gone("queue".into());
  ui(&mut app, UiMessage::TitlebarHover(false));
  let mix = app.ui.controls.mix("queue", app.now);
  assert!(!mix.hovered && !mix.tracked);
  assert_eq!(app.ui.controls.tracked(), 0);
  assert!(!app.animation_needed());
}

#[test]
fn control_gone_drops_hover_press_and_ripple_at_once() {
  let mut app = app();
  ui(&mut app, UiMessage::ControlHover("queue".into(), true));
  ui(
    &mut app,
    UiMessage::Touch(
      "queue".into(),
      Some((iced::Point::new(4.0, 10.0), iced::Size::new(34.0, 34.0))),
    ),
  );
  assert!(app.animation_needed());
  ui(&mut app, UiMessage::ControlGone("queue".into()));
  assert_eq!(app.ui.controls.tracked(), 0);
  assert!(!app.animation_needed(), "rien ne s'anime plus");
  // Les autres clés gardent leur état.
  ui(&mut app, UiMessage::ControlHover("other".into(), true));
  ui(&mut app, UiMessage::ControlGone("queue".into()));
  assert!(app.ui.controls.mix("other", app.now).hovered);
}

#[test]
fn a_click_starts_a_ripple_and_pressed_then_the_frames_stop() {
  let mut app = app();
  assert!(!app.animation_needed());
  ui(
    &mut app,
    UiMessage::Touch(
      "queue".into(),
      Some((iced::Point::new(4.0, 10.0), iced::Size::new(34.0, 34.0))),
    ),
  );
  let start = app.now;
  let at = |ms| start + Duration::from_millis(ms);
  assert!(app.animation_needed());
  assert_eq!(app.ui.controls.ripples_of("queue", start).len(), 1);
  assert!(!app.ui.controls.ripples_of("queue", at(549)).is_empty());
  assert_eq!(app.ui.controls.mix("queue", at(180)).press, 1.0);
  // Relâché aussitôt : `.pressed` monte en 180 ms, part à 180 ms, puis 180 ms de fondu ; l'onde dure 550 ms.
  ui(&mut app, UiMessage::Touch("queue".into(), None));
  assert_eq!(app.ui.controls.mix("queue", at(180)).press, 1.0);
  assert_eq!(app.ui.controls.mix("queue", at(360)).press, 0.0);
  app.now = at(551);
  assert!(app.ui.controls.ripples_of("queue", app.now).is_empty());
  assert!(
    !app.animation_needed(),
    "onde et appui finis : l'abonnement s'arrête"
  );
}

#[test]
fn cover_hover_grows_the_halo_then_the_frames_stop_when_paused() {
  let mut app = app();
  app.player.current = Some(track("a"));
  app.playback.is_playing = false;
  let rest = app.visuals.shape.rotation_rate;
  assert_eq!(rest, 0.25);
  let mut at = app.now;
  let step = Duration::from_micros(4167);
  let freq = [0_u8; crate::visuals::viz::FREQ_BINS];
  for _ in 0..200 {
    at += step;
    app.animation_tick(at, &freq);
  }
  assert!(
    !app.visuals.shape.is_settling(false),
    "pause posée : la boucle est arrêtée"
  );
  ui(&mut app, UiMessage::CoverHover(true));
  assert!(
    app.visuals.shape.is_settling(false),
    "le survol relance la boucle"
  );
  for _ in 0..100 {
    at += step;
    app.animation_tick(at, &freq);
  }
  assert_eq!(app.visuals.shape.hover_progress, 1.0);
  assert!(
    app.visuals.shape.is_settling(false),
    "tant que la souris reste dessus"
  );
  ui(&mut app, UiMessage::CoverHover(false));
  for _ in 0..200 {
    at += step;
    app.animation_tick(at, &freq);
  }
  assert_eq!(app.visuals.shape.hover_progress, 0.0);
  assert!(
    !app.visuals.shape.is_settling(false),
    "sortie posée : la boucle s'arrête"
  );
}

#[test]
fn view_transition_starts_only_on_a_real_view_change_and_stops_after_400_ms() {
  let mut app = app();
  app.update(Message::Navigate(Screen::Liked));
  assert!(
    !app.animation_needed(),
    "hors application lancée : au repos"
  );
  app.view_motion.enabled = true;
  app.update(Message::Navigate(Screen::Liked));
  assert!(!app.animation_needed(), "même écran : rien");
  app.update(Message::Navigate(Screen::Albums));
  let start = app.now;
  assert!(app.animation_needed());
  assert!(app.view_motion.pose(start).is_some());
  app.now = start + Duration::from_millis(399);
  assert!(app.animation_needed());
  app.now = start + Duration::from_millis(400);
  assert!(!app.animation_needed());
  assert_eq!(app.view_motion.pose(app.now), None);
  // Les réglages sont une modale : aucune transition de vue.
  app.update(Message::Navigate(Screen::Settings));
  app.now += Duration::from_secs(1);
  app.update(Message::Back);
  assert_eq!(app.view_motion.start, Some(start));
}

#[test]
fn list_screens_and_the_playlist_choice_start_their_fade_up_only_in_the_launched_app() {
  let mut app = app();
  app.update(Message::Navigate(Screen::Liked));
  let _ = app.open_modal(Modal::AddToPlaylist { tracks: Vec::new() });
  assert_eq!(
    app.entry_motion,
    crate::motion::EntryMotion::default(),
    "banc : au repos"
  );
  assert!(!app.animation_needed());
  app.entry_motion.enabled = true;
  app.update(Message::Navigate(Screen::Library));
  let start = app.now;
  assert_eq!(app.entry_motion.rows, Some(start));
  assert!(app.animation_needed());
  app.now = start + Duration::from_millis(559);
  assert!(app.animation_needed());
  app.now = start + Duration::from_millis(560);
  assert!(
    !app.animation_needed(),
    "dernière ligne décalée finie : l'abonnement s'arrête"
  );
  app.update(Message::Navigate(Screen::Home));
  assert_eq!(
    app.entry_motion.rows,
    Some(start),
    "l'accueil n'a pas de lignes de titres"
  );
  let _ = app.open_modal(Modal::AddToPlaylist { tracks: Vec::new() });
  assert!(app.entry_motion.picks.is_some());
  let artist = app.entry_motion.artist;
  assert_eq!(artist, None, "la carte d'artiste attend ses données");
}

/// App lancée (animations actives) posée sur `screen`, sans départ en cours.
fn launched_on(screen: Screen) -> App {
  let mut app = app();
  app.entry_motion.enabled = true;
  app.update(Message::Navigate(screen));
  app.entry_motion.rows = None;
  app.entry_motion.more = None;
  app
}

#[test]
fn a_settings_category_change_replays_settings_in_but_opening_the_window_does_not() {
  use crate::prefs::Category;
  let open = |app: &mut App, category| {
    app.update(Message::Settings(SettingsMessage::OpenCategory(category)))
  };
  let mut app = launched_on(Screen::Home);
  assert_eq!(app.settings.settings_cat, "interface");
  // Même catégorie : la classe `cat-hidden` ne change pas, rien ne repart.
  open(&mut app, Category::Interface);
  assert_eq!(app.entry_motion.section, None);
  open(&mut app, Category::Theme);
  assert_eq!(app.settings.settings_cat, "theme");
  let start = app.entry_motion.section.expect("section qui apparaît");
  assert!(app.entry_motion.section_pose(start).is_some());
  assert!(app.entry_motion.running(start, 0));
  open(&mut app, Category::Theme);
  assert_eq!(app.entry_motion.section, Some(start), "même catégorie");
  // L'ouverture de la fenêtre sur la dernière catégorie ne rejoue rien (`04-reglages` : la section reste à l'opacité 1).
  app.entry_motion.section = None;
  app.update(Message::Navigate(Screen::Settings));
  assert_eq!(app.entry_motion.section, None);
  // Ouverte depuis l'accueil (Spotify) : la catégorie change, la section s'anime pendant l'ouverture.
  app.update(Message::Navigate(Screen::Home));
  ui(&mut app, UiMessage::SpotifyPromoOpen);
  assert_eq!(app.settings.settings_cat, "integrations");
  assert!(app.entry_motion.section.is_some());
}

#[test]
fn a_settings_category_change_starts_nothing_when_the_motion_is_off() {
  use crate::prefs::Category;
  let mut app = app();
  app.update(Message::Settings(SettingsMessage::OpenCategory(
    Category::Theme,
  )));
  assert_eq!(app.settings.settings_cat, "theme");
  assert_eq!(app.entry_motion.section, None);
}

#[test]
fn a_failed_search_starts_the_error_box_fade_on_the_search_screen_only() {
  let mut app = launched_on(Screen::Home);
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  assert_eq!(
    app.entry_motion.error, None,
    "pas d'erreur pendant le chargeur"
  );
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::Search { token },
      Ok(json!({ "success": false, "error": "panne" })),
    ),
  );
  let start = app.entry_motion.error.expect("boîte d'erreur créée");
  assert_eq!(
    app.entry_motion.error_pose(start).map(|pose| pose.opacity),
    Some(0.0)
  );
  assert!(
    app
      .entry_motion
      .running(start + Duration::from_millis(539), 0)
  );
  assert!(
    !app
      .entry_motion
      .running(start + Duration::from_millis(540), 0)
  );
  // Erreur réseau sur une nouvelle recherche : la boîte est recréée, le départ change.
  searching(&mut app, "daft punk");
  let token = app.ui.search.token;
  app.entry_motion.error = None;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Err("coupé".to_string())),
  );
  assert!(app.entry_motion.error.is_some());
  // Réponse tardive reçue sur une autre page : rien ne démarre.
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  app.update(Message::Navigate(Screen::Liked));
  app.entry_motion.error = None;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Err("coupé".to_string())),
  );
  assert_eq!(app.entry_motion.error, None);
}

#[test]
fn search_data_starts_the_fade_up_only_on_the_search_screen() {
  let mut app = launched_on(Screen::Home);
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  app.update(Message::Navigate(Screen::Liked));
  let liked = app.entry_motion.rows;
  assert!(liked.is_some());
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&["a", "b"]))),
  );
  assert_eq!(app.entry_motion.rows, liked, "réponse reçue sur Liked");
  let known =
    Ok(json!({ "success": true, "artist": { "name": "daft", "isKnownArtist": true, "fans": 1 } }));
  let artist_reply = || {
    UiMessage::Reply(
      ReplyTag::SearchArtist {
        token,
        via_track: false,
      },
      known.clone(),
    )
  };
  ui(&mut app, artist_reply());
  assert_eq!(app.entry_motion.artist, None, "carte reçue sur Liked");
  app.update(Message::Back);
  assert_eq!(app.screen, Screen::Search);
  ui(&mut app, artist_reply());
  assert!(app.entry_motion.artist.is_some());
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&["a", "b"]))),
  );
  assert_ne!(app.entry_motion.rows, liked);
  assert!(
    app.entry_motion.rows.is_some(),
    "réponse reçue sur la recherche"
  );
}

#[test]
fn album_and_artist_replies_start_the_fade_up_only_on_their_own_screen() {
  let mut app = launched_on(Screen::Home);
  ui(&mut app, UiMessage::OpenAlbum(Box::default()));
  let run = app.ui.album.run;
  let reply = || {
    UiMessage::Reply(
      ReplyTag::AlbumTracks { run },
      Ok(json!({ "success": true, "tracks": [{ "title": "X" }] })),
    )
  };
  app.update(Message::Navigate(Screen::Liked));
  let liked = app.entry_motion.rows;
  ui(&mut app, reply());
  assert_eq!(app.entry_motion.rows, liked, "album reçu sur Liked");
  app.update(Message::Back);
  assert_eq!(app.screen, Screen::Album);
  ui(&mut app, reply());
  assert_ne!(app.entry_motion.rows, liked);
  assert!(app.entry_motion.rows.is_some());

  let mut app = launched_on(Screen::Home);
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  let run = app.ui.artist.run;
  app.update(Message::Navigate(Screen::Liked));
  let liked = app.entry_motion.rows;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Artist { run }, Ok(artist_reply())),
  );
  assert_eq!(app.entry_motion.rows, liked, "artiste reçu sur Liked");
  app.update(Message::Back);
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Artist { run }, Ok(artist_reply())),
  );
  assert_ne!(app.entry_motion.rows, liked);
  assert!(app.entry_motion.rows.is_some());
}

#[test]
fn the_player_bar_like_flips_the_heart_at_once_without_toast_or_fade_off_the_liked_screen() {
  // `toggleFavoriteTrack` (`app.js:12261`) : `renderFavoritesView` seulement si la vue courante est `favorites`, aucun toast.
  let mut app = launched_on(Screen::Home);
  app.player.current = Some(track("a"));
  assert!(app.data.is_favorite(&track("a")));
  let effects = ui(
    &mut app,
    UiMessage::CurrentCommand(TrackCommand::ToggleFavorite),
  );
  assert!(!app.data.is_favorite(&track("a")));
  assert!(effects.iter().all(|e| !matches!(e, Effect::Toast(..))));
  assert!(
    app.entry_motion.rows.is_none(),
    "pas de fondu hors de Liked"
  );
  ui(
    &mut app,
    UiMessage::CurrentCommand(TrackCommand::ToggleFavorite),
  );
  assert!(app.data.is_favorite(&track("a")));
  assert!(app.entry_motion.rows.is_none());
  assert!(!app.animation_needed());

  let mut app = launched_on(Screen::Liked);
  app.player.current = Some(track("a"));
  ui(
    &mut app,
    UiMessage::CurrentCommand(TrackCommand::ToggleFavorite),
  );
  assert!(app.entry_motion.rows.is_some(), "rejeu sur Liked");
}

#[test]
fn riff_re_renders_replay_the_fade_up_on_the_liked_screen() {
  let mut app = launched_on(Screen::Liked);
  ui(&mut app, UiMessage::FavoritesQuery("a".to_string()));
  assert!(
    app.entry_motion.rows.take().is_some(),
    "frappe dans la recherche"
  );
  ui(&mut app, UiMessage::SetFavoritesSort(SortMode::AddedDesc));
  assert!(app.entry_motion.rows.take().is_some(), "tri");
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("a")), TrackCommand::ToggleFavorite),
  );
  assert!(app.entry_motion.rows.take().is_some(), "like ou unlike");
  let pin = TrackCommand::PinToTop(crate::library::FAVORITES_SCOPE.to_string());
  ui(&mut app, UiMessage::Track(Box::new(track("b")), pin));
  assert!(app.entry_motion.rows.take().is_some(), "épinglage");
  ui(&mut app, UiMessage::LocalQuery("a".to_string()));
  assert!(
    app.entry_motion.rows.is_none(),
    "la bibliothèque locale n'est pas affichée"
  );
}

#[test]
fn riff_re_renders_replay_the_fade_up_on_the_other_lists() {
  let mut app = launched_on(Screen::Library);
  ui(&mut app, UiMessage::LocalQuery("a".to_string()));
  assert!(app.entry_motion.rows.take().is_some(), "filtre local");
  ui(&mut app, UiMessage::FavoritesQuery("a".to_string()));
  assert!(app.entry_motion.rows.is_none(), "Liked n'est pas affiché");

  let mut app = launched_on(Screen::Home);
  let first = with_playlist(&mut app);
  let second = app.data.create_playlist("Q", tracks(&["u"]), 1, 1).unwrap();
  app.entry_motion.rows = None;
  ui(&mut app, UiMessage::PlaylistQuery("x".to_string()));
  assert!(app.entry_motion.rows.take().is_some(), "filtre de playlist");
  ui(&mut app, UiMessage::SetPlaylistSort(SortMode::AddedAsc));
  assert!(app.entry_motion.rows.take().is_some(), "tri de playlist");
  ui(&mut app, UiMessage::OpenPlaylist(second));
  assert_eq!(app.screen, Screen::PlaylistDetail);
  assert!(
    app.entry_motion.rows.take().is_some(),
    "d'une playlist à l'autre"
  );
  ui(&mut app, UiMessage::OpenPlaylist(first));
  assert!(app.entry_motion.rows.is_some());

  let mut app = launched_on(Screen::Home);
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  let run = app.ui.artist.run;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Artist { run }, Ok(artist_reply())),
  );
  app.entry_motion.rows = None;
  ui(&mut app, UiMessage::ArtistToggleExpanded);
  assert!(app.entry_motion.rows.is_some(), "« Show all » de l'artiste");
}

#[test]
fn the_queue_screen_replays_the_fade_up_only_when_the_queue_changes() {
  let mut app = app();
  app.entry_motion.enabled = true;
  app.player.queue = tracks(&["a", "b"]);
  app.update(Message::Navigate(Screen::Queue));
  app.entry_motion.rows = None;
  ui(&mut app, UiMessage::QueueScrolled(0.0, 0.0));
  assert!(app.entry_motion.rows.is_none(), "rien n'a changé");
  ui(&mut app, UiMessage::ClearQueue);
  assert!(app.entry_motion.rows.is_some(), "file vidée");
  app.player.queue = tracks(&["a", "b"]);
  app.update(Message::Navigate(Screen::Liked));
  app.entry_motion.rows = None;
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("q")), TrackCommand::AddToQueue),
  );
  assert!(
    app.entry_motion.rows.is_none(),
    "la file n'est pas affichée"
  );
}

#[test]
fn load_more_animates_only_the_new_search_rows() {
  let mut app = launched_on(Screen::Home);
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&["a", "b"]))),
  );
  let rows = app.entry_motion.rows;
  app.update(Message::Navigate(Screen::Liked));
  app.entry_motion.rows = rows;
  let more = || {
    UiMessage::Reply(
      ReplyTag::SearchMore { token },
      Ok(json!({ "tracks": tracks(&["c", "d"]), "hasMore": true })),
    )
  };
  ui(&mut app, more());
  assert_eq!(app.entry_motion.more, None, "réponse reçue sur Liked");
  app.update(Message::Back);
  let before = app.ui.search.tracks.len();
  ui(&mut app, more());
  let (start, first) = app
    .entry_motion
    .more
    .expect("départ propre aux nouvelles lignes");
  assert_eq!(first, before);
  assert_eq!(
    app.entry_motion.rows, rows,
    "les anciennes lignes restent au repos"
  );
  let late = start + Duration::from_millis(1000);
  assert_eq!(app.entry_motion.row_pose(0, late), None);
  assert!(
    app
      .entry_motion
      .row_pose(2, start + Duration::from_millis(100))
      .is_some()
  );
  assert_eq!(app.entry_motion.row_pose(2, late), None);
}

#[test]
fn the_row_veil_choice_is_frozen_when_the_fade_starts() {
  let mut app = launched_on(Screen::Liked);
  let veil = |app: &App| crate::screens::common::fade_veil(app, iced::Color::BLACK);
  app.entry_motion.plain = Some(true);
  assert!(veil(&app).is_some());
  app.entry_motion.plain = Some(false);
  assert!(veil(&app).is_none());
  app.entry_motion.plain = None;
  assert!(veil(&app).is_some(), "sans départ : état courant");
  app.update(Message::Navigate(Screen::Library));
  assert_eq!(app.entry_motion.plain, Some(true), "choix posé au départ");
}

#[test]
fn the_row_veil_follows_the_halo_of_the_screen_being_entered() {
  let veil = |app: &App| crate::screens::common::fade_veil(app, iced::Color::BLACK);
  let cover = || Track {
    thumbnail: Some("https://exemple.org/c.jpg".to_string()),
    ..track("x")
  };
  // (a) Album avec halo vers Liked : le halo quitté ne supprime pas le fondu.
  let mut app = launched_on(Screen::Home);
  ui(&mut app, UiMessage::OpenAlbum(Box::default()));
  app.ui.album.album.cover = Some("https://exemple.org/c.jpg".to_string());
  assert!(app.hero_url().is_some());
  app.update(Message::Navigate(Screen::Liked));
  assert_eq!(app.entry_motion.plain, Some(true), "pas de halo sur Liked");
  assert!(veil(&app).is_some());
  for screen in [Screen::Library, Screen::History, Screen::Queue] {
    app.update(Message::Navigate(screen));
    assert_eq!(app.entry_motion.plain, Some(true), "{screen:?}");
  }
  // (b) Accueil vers une playlist avec pochette : le halo arrive, pas de voile dessus.
  let mut app = launched_on(Screen::Home);
  let id = app.data.create_playlist("P", vec![cover()], 1, 1).unwrap();
  ui(&mut app, UiMessage::OpenPlaylist(id));
  assert_eq!(app.entry_motion.plain, Some(false), "halo cible");
  assert!(veil(&app).is_none());
  // Pistes d'album reçues avant la pochette : le voile figé tombe quand le halo apparaît.
  let mut app = launched_on(Screen::Home);
  ui(&mut app, UiMessage::OpenAlbum(Box::default()));
  app.entered_rows(Screen::Album);
  assert_eq!(app.entry_motion.plain, Some(true));
  assert!(veil(&app).is_some());
  app.ui.album.album.cover = Some("https://exemple.org/c.jpg".to_string());
  assert!(veil(&app).is_none(), "jamais de voile sur un halo");
}

#[test]
fn the_history_view_replays_the_fade_up_on_play_and_on_clear() {
  let mut app = launched_on(Screen::History);
  app.player.current = Some(track("a"));
  app.start_current(None);
  assert!(app.entry_motion.rows.take().is_some(), "lecture d'un titre");
  ui(&mut app, UiMessage::ClearHistory);
  assert!(app.entry_motion.rows.take().is_some(), "historique vidé");
  app.update(Message::Navigate(Screen::Liked));
  app.entry_motion.rows = None;
  app.start_current(None);
  assert!(app.entry_motion.rows.is_none(), "historique non affiché");
}

#[test]
fn the_queue_is_hashed_only_for_messages_that_can_change_it() {
  let mut app = app();
  app.entry_motion.enabled = true;
  app.player.queue = tracks(&["a", "b"]);
  app.update(Message::Navigate(Screen::Queue));
  assert_eq!(app.queue_seen, None);
  app.update(Message::AnimationFrame(std::time::Instant::now()));
  ui(&mut app, UiMessage::RowEntered(ListId::Queue, 0));
  ui(&mut app, UiMessage::QueueScrolled(0.0, 0.0));
  assert_eq!(app.queue_seen, None, "images et souris : pas de hachage");
  ui(&mut app, UiMessage::ClearQueue);
  assert!(app.queue_seen.is_some());
  assert!(app.entry_motion.rows.is_some());
  app.entry_motion.rows = None;
  ui(&mut app, UiMessage::ClearQueue);
  assert!(app.entry_motion.rows.is_none(), "file inchangée");
  app.update(Message::Navigate(Screen::Liked));
  assert_eq!(app.queue_seen, None, "oublié en quittant l'écran");
}

#[test]
fn the_settings_modal_opens_and_closes_over_200_ms_then_the_timer_stops() {
  let mut app = app();
  app.update(Message::Navigate(Screen::Settings));
  assert!(
    !app.animation_needed(),
    "hors application lancée : au repos"
  );
  assert_eq!(app.overlay_motion.settings_pose(app.now), None);
  app.update(Message::Back);
  app.overlay_motion.enabled = true;
  app.update(Message::Navigate(Screen::Settings));
  let opened = app.now;
  assert!(app.animation_needed());
  assert_eq!(
    app
      .overlay_motion
      .settings_pose(opened)
      .map(|pose| pose.scrim),
    Some(0.0)
  );
  app.now = opened + Duration::from_millis(200);
  assert!(!app.animation_needed(), "posée : l'abonnement s'arrête");
  assert_eq!(
    app.overlay_motion.settings_pose(app.now),
    None,
    "ouverte : au repos"
  );
  // Fermeture (l'horloge de `show` est l'instant réel : la modale est posée ouverte depuis longtemps) : l'écran revient aussitôt,
  // la modale reste dessinée jusqu'à la fin du fondu.
  app.overlay_motion = crate::motion::OverlayMotion::launched();
  app.overlay_motion.navigate(
    Screen::Home,
    Screen::Settings,
    app.now - Duration::from_secs(1),
  );
  app.update(Message::Back);
  let closed = app.now;
  assert_ne!(app.screen, Screen::Settings);
  assert!(app.animation_needed());
  assert!(
    app
      .overlay_motion
      .settings_leaving(closed + Duration::from_millis(100))
  );
  app.now = closed + Duration::from_millis(200);
  assert!(!app.overlay_motion.settings_leaving(app.now) && !app.animation_needed());
}

#[test]
fn trending_is_hidden_with_the_wave_setting_like_in_riff() {
  let mut app = app();
  assert!(app.home_sections().contains(&"sp-trend-sec"));
  app.settings.home_wave = false;
  assert!(!app.home_sections().contains(&"sp-trend-sec"));
}

#[test]
fn my_wave_needs_enough_tracks_then_plays_as_a_radio() {
  let mut app = app();
  let effects = player(&mut app, PlayerMessage::PlayMyWave);
  assert!(matches!(effects.as_slice(), [Effect::Toast(text, _)] if text.contains("at least 5")));
  app.ui.home.wave = tracks(&["w"]);
  player(&mut app, PlayerMessage::PlayMyWave);
  assert!(app.player.radio && app.player.my_wave);
  assert_eq!(current_id(&app), "w");
}

#[test]
fn the_radio_is_refilled_and_continues() {
  let mut app = app();
  app.ui.home.wave = tracks(&["w"]);
  let effects = player(&mut app, PlayerMessage::PlayMyWave);
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      tag: ReplyTag::RadioRefill,
      ..
    }
  )));
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  audio(&mut app, AudioEvent::Ended);
  assert!(app.ui.radio_waiting);
  let reply = json!({ "tracks": tracks(&["w", "new"]) });
  ui(&mut app, UiMessage::Reply(ReplyTag::RadioRefill, Ok(reply)));
  assert_eq!(current_id(&app), "new");
  assert_eq!(app.player.queue.len(), 2);
}

#[test]
fn the_radio_refill_adds_neither_blocked_nor_liked_tracks() {
  let mut app = app();
  app.ui.home.wave = tracks(&["w"]);
  player(&mut app, PlayerMessage::PlayMyWave);
  app.data.toggle_block_track(&track("x"));
  app.data.toggle_block_artist("Artiste y");
  let reply = json!({ "tracks": tracks(&["a", "x", "y", "new"]) });
  ui(&mut app, UiMessage::Reply(ReplyTag::RadioRefill, Ok(reply)));
  assert_eq!(ids(&app.player.queue), ["w", "new"]);
}

#[test]
fn skipping_early_in_my_wave_is_remembered() {
  let mut app = app();
  app.ui.home.wave = tracks(&["w", "x"]);
  player(&mut app, PlayerMessage::PlayMyWave);
  loaded(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(3)));
  player(&mut app, PlayerMessage::Next);
  assert_eq!(app.data.wave_skips.len(), 1);
}

#[test]
fn mixes_are_kept_for_the_day_only_with_enough_tracks() {
  let mut app = app();
  let ready = |tracks: Vec<Track>| UiMessage::MixReady {
    day: "2026-11-03".into(),
    id: "daily-mix-1".into(),
    title: "Daily Mix 1".into(),
    artists: vec!["A".into()],
    tracks,
  };
  assert_eq!(ui(&mut app, ready(tracks(&["a"]))), Vec::new());
  assert!(app.data.daily_mixes.is_none());
  ui(
    &mut app,
    ready(tracks(&["1", "2", "3", "4", "5", "6", "7", "8"])),
  );
  assert_eq!(app.data.daily_mixes.as_ref().unwrap().mixes.len(), 1);
  ui(&mut app, UiMessage::OpenMix(0));
  assert_eq!(app.screen, Screen::Album);
  assert!(app.ui.album.is_mix());
  assert_eq!(app.ui.album.tracks.len(), 8);
  ui(&mut app, UiMessage::RateMix(false));
  assert_eq!(app.data.mix_rating("daily-mix-1"), Some("dislike"));
}

#[test]
fn a_daily_mix_drops_the_blocked_tracks_before_it_is_kept() {
  let mut app = app();
  app.data.toggle_block_track(&track("b1"));
  app.data.toggle_block_artist("Artiste b2");
  let ready = |tracks: Vec<Track>| UiMessage::MixReady {
    day: "2026-11-03".into(),
    id: "daily-mix-1".into(),
    title: "Daily Mix 1".into(),
    artists: vec!["A".into()],
    tracks,
  };
  ui(
    &mut app,
    ready(tracks(&[
      "1", "2", "3", "4", "5", "6", "7", "8", "b1", "b2",
    ])),
  );
  let mixes = app.data.daily_mixes.as_ref().unwrap();
  assert_eq!(mixes.mixes[0].tracks.len(), 8);
  ui(
    &mut app,
    ready(tracks(&["1", "2", "3", "4", "5", "6", "7", "b1"])),
  );
  assert_eq!(
    app.data.daily_mixes.as_ref().unwrap().mixes.len(),
    1,
    "sept titres sains ne suffisent pas"
  );
}

#[test]
fn opening_a_cached_daily_mix_hides_the_tracks_blocked_since() {
  let mut app = app();
  app.data.daily_mixes = Some(DailyMixes {
    day: Some("2026-11-03".into()),
    mixes: vec![DailyMix {
      id: Some("daily-mix-1".into()),
      tracks: tracks(&["1", "2", "3"]),
      ..DailyMix::default()
    }],
  });
  app.data.toggle_block_track(&track("2"));
  ui(&mut app, UiMessage::OpenMix(0));
  assert_eq!(ids(&app.ui.album.tracks), ["1", "3"]);
}

#[test]
fn the_play_button_of_a_daily_mix_plays_it_in_order_even_with_shuffle_on() {
  let mut app = app();
  app.data.shuffle = true;
  app.data.daily_mixes = Some(DailyMixes {
    day: Some("2026-11-03".into()),
    mixes: vec![DailyMix {
      id: Some("daily-mix-1".into()),
      tracks: tracks(&["1", "2", "3", "4"]),
      ..DailyMix::default()
    }],
  });
  app.data.toggle_block_track(&track("2"));
  let screen = app.screen;
  ui(&mut app, UiMessage::PlayMix(0));
  assert_eq!(ids(&app.player.queue), ["1", "3", "4"]);
  assert_eq!(app.player.index, Some(0));
  assert!(!app.player.shuffle, "file dans l'ordre du mix");
  assert!(!app.player.radio);
  assert!(app.data.shuffle, "l'aléatoire global ne change pas");
  assert_eq!(app.screen, screen, "la page reste l'accueil");
  assert!(ui(&mut app, UiMessage::PlayMix(5)).is_empty());
}

#[test]
fn opening_an_album_loads_its_tracks_and_a_late_answer_is_ignored() {
  let mut app = app();
  let album = SavedAlbum {
    id: Some("al".into()),
    title: Some("T".into()),
    artist: Some("A".into()),
    cover: Some("c".into()),
    ..SavedAlbum::default()
  };
  let effects = ui(&mut app, UiMessage::OpenAlbum(Box::new(album.clone())));
  assert_eq!(app.screen, Screen::Album);
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/albums/tracks",
      ..
    }
  )));
  let old = app.ui.album.run;
  ui(&mut app, UiMessage::OpenAlbum(Box::new(album)));
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::AlbumTracks { run: old },
      Ok(json!({ "success": true, "tracks": [{ "title": "X" }] })),
    ),
  );
  assert!(app.ui.album.tracks.is_empty());
  let run = app.ui.album.run;
  let reply = json!({ "success": true, "album": { "year": "2001" }, "tracks": [{ "title": "X (feat. Y)", "durationMs": 61000 }] });
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::AlbumTracks { run }, Ok(reply)),
  );
  let first = &app.ui.album.tracks[0];
  assert_eq!(first.id.as_deref(), Some("alb_al_0"));
  assert_eq!(first.artist.as_deref(), Some("A"));
  assert_eq!(first.duration, Some(61.0));
  assert_eq!(first.cover.as_deref(), Some("c"));
  assert_eq!(first.search_query.as_deref(), Some("A X"));
  assert_eq!(app.ui.album.album.year.as_deref(), Some("2001"));
}

#[test]
fn saving_an_album_can_also_like_its_tracks() {
  let mut app = app();
  app.data.like_album_tracks = true;
  app.ui.album.album = SavedAlbum {
    id: Some("al".into()),
    ..SavedAlbum::default()
  };
  app.ui.album.tracks = tracks(&["new"]);
  ui(&mut app, UiMessage::ToggleSaveAlbum);
  assert_eq!(app.data.saved_albums.len(), 1);
  assert!(app.data.is_favorite(&track("new")));
  ui(&mut app, UiMessage::ToggleSaveAlbum);
  assert!(app.data.saved_albums.is_empty());
}

fn artist_reply() -> Value {
  json!({ "success": true, "artist": {
    "name": "Muse", "isVerified": true, "fans": 1234, "bio": "<b>Bio</b> ici", "avatar": "a.png",
    "tracks": tracks(&["1", "2", "3", "4", "5", "6"]),
    "albums": [
      { "title": "Bon", "artist": "muse", "trackCount": 10 },
      { "title": "Single", "artist": "Muse", "trackCount": 1 },
      { "title": "Autre", "artist": "Quelqu'un", "trackCount": 10 },
      { "title": "Simple", "artist": "Muse", "recordType": "single" }
    ]
  } })
}

#[test]
fn the_artist_page_filters_albums_and_shows_five_tracks_first() {
  let mut app = app();
  ui(&mut app, UiMessage::OpenArtist("Muse - Topic".to_string()));
  assert_eq!(app.ui.artist.name, "Muse");
  let run = app.ui.artist.run;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Artist { run }, Ok(artist_reply())),
  );
  let view = &app.ui.artist;
  assert_eq!(view.albums.len(), 1);
  assert_eq!(view.bio, "Bio ici");
  assert!(view.verified);
  assert_eq!(view.fans_line, "1,234 listeners");
  assert_eq!(app.list_len(ListId::Artist), 5);
  ui(&mut app, UiMessage::ArtistToggleExpanded);
  assert_eq!(app.list_len(ListId::Artist), 6);
  ui(&mut app, UiMessage::ArtistBio);
  assert!(matches!(&app.ui.modal, Some(Modal::Biography { body, .. }) if body == "Bio ici"));
}

#[test]
fn playing_from_the_artist_page_queues_all_its_tracks() {
  let mut app = app();
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  let run = app.ui.artist.run;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Artist { run }, Ok(artist_reply())),
  );
  ui(&mut app, UiMessage::RowActivated(ListId::Artist, 1));
  assert_eq!(app.player.queue.len(), 6);
  assert_eq!(app.player.index, Some(1));
}

#[test]
fn following_and_blocking_an_artist_from_its_page() {
  let mut app = app();
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  ui(&mut app, UiMessage::ArtistFollow);
  assert!(app.data.is_followed("Muse"));
  ui(&mut app, UiMessage::ArtistBlock);
  assert!(app.data.is_artist_blocked("Muse"));
}

#[test]
fn a_failed_artist_page_says_so() {
  let mut app = app();
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  let run = app.ui.artist.run;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Artist { run }, Err("panne".to_string())),
  );
  assert!(app.ui.artist.failed && !app.ui.artist.loading);
}

#[test]
fn the_artist_page_motion_starts_only_in_the_launched_app() {
  let mut app = app();
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  assert_eq!(
    app.artist_motion,
    crate::motion::ArtistMotion::default(),
    "banc : au repos"
  );
  assert!(!app.animation_needed());
}

#[test]
fn the_artist_page_starts_its_pane_loader_and_badge_from_the_app_clock() {
  let mut app = app();
  app.artist_motion.enabled = true;
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  let motion = app.artist_motion;
  // Ouverture : le panneau entre (260 ms) et le chargeur repart ; le badge attend la réponse (`app.js:11992`, `11993`).
  assert!(motion.pane.is_some() && motion.loader.is_some());
  assert_eq!(motion.badge, None);
  assert!(app.animation_needed(), "chargeur et entrée de panneau");
  let opened = motion.pane.unwrap();
  app.now = opened + Duration::from_millis(260);
  assert!(
    app.animation_needed(),
    "le chargeur tourne tant que la réponse manque"
  );
  let run = app.ui.artist.run;
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Artist { run }, Ok(artist_reply())),
  );
  // Artiste vérifié : `hidden` retiré, le badge part de zéro ; le chargeur n'est plus affiché.
  let badge = app.artist_motion.badge.expect("badge démarré");
  assert!(app.animation_needed(), "le badge tourne sans fin");
  app.now = badge + Duration::from_secs(14);
  assert!(app.animation_needed());
  // Un onglet cliqué rejoue l'entrée, même s'il était déjà actif.
  ui(&mut app, UiMessage::ArtistTab(crate::ui::ArtistTab::Albums));
  let tab = app.artist_motion.pane;
  assert_ne!(tab, Some(opened));
  assert!(app.artist_motion.pane_running(tab.unwrap()));
  // Retour sur la page depuis un autre écran : le panneau et le badge rejouent leur animation.
  app.update(Message::Navigate(Screen::Liked));
  let before = app.artist_motion;
  app.update(Message::Back);
  assert_eq!(app.screen, Screen::Artist);
  assert!(app.artist_motion.pane >= before.pane && app.artist_motion.badge >= before.badge);
  // Fin des 260 ms et sans artiste vérifié : plus rien ne demande d'image.
  app.ui.artist.verified = false;
  app.now = app.artist_motion.pane.unwrap() + Duration::from_millis(260);
  assert!(!app.animation_needed());
}

#[test]
fn detail_pages_are_not_remembered_for_the_next_launch() {
  let mut app = app();
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  assert_eq!(app.settings.screen, Screen::Home);
}

#[test]
fn filtering_and_sorting_favorites_changes_the_rows_not_the_data() {
  let mut app = app();
  app.data.favorites[0].added_at = Some(1);
  app.data.favorites[1].added_at = Some(3);
  app.data.favorites[2].added_at = Some(2);
  ui(&mut app, UiMessage::SetFavoritesSort(SortMode::AddedDesc));
  assert_eq!(app.ui.favorites_order, [1, 2, 0]);
  assert_eq!(
    app.list_track(ListId::Favorites, 0).unwrap().id.as_deref(),
    Some("b")
  );
  ui(&mut app, UiMessage::FavoritesQuery("TITRE C".to_string()));
  assert_eq!(app.list_len(ListId::Favorites), 1);
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 0));
  assert_eq!(current_id(&app), "c");
  assert_eq!(
    app.player.queue.len(),
    3,
    "la file suit le tri, pas le filtre"
  );
  assert_eq!(app.player.index, Some(1));
}

fn drag(app: &mut App, list: ListId, from: usize, to: usize) -> Vec<Effect> {
  ui(app, UiMessage::RowPressed(list, from));
  ui(app, UiMessage::RowEntered(list, to));
  ui(app, UiMessage::RowReleased)
}

fn ids(list: &[Track]) -> Vec<&str> {
  list
    .iter()
    .map(|track| track.id.as_deref().unwrap_or(""))
    .collect()
}

#[test]
fn dragging_a_favorite_reorders_and_goes_back_to_the_manual_order() {
  let mut app = app();
  app.data.favorites_sort = SortMode::AddedAsc;
  app.refresh_orders();
  drag(&mut app, ListId::Favorites, 0, 2);
  assert_eq!(app.data.favorites_sort, SortMode::Manual);
  assert_eq!(ids(&app.data.favorites), ["b", "c", "a"]);
  assert!(
    app.player.current.is_none(),
    "un glisser ne lance pas la lecture"
  );
}

#[test]
fn dragging_is_refused_while_filtering() {
  let mut app = app();
  ui(&mut app, UiMessage::FavoritesQuery("titre".to_string()));
  assert_eq!(drag(&mut app, ListId::Favorites, 0, 2), Vec::new());
  assert_eq!(ids(&app.data.favorites), ["a", "b", "c"]);
}

#[test]
fn dragging_in_the_queue_follows_the_current_track() {
  let mut app = app();
  play_first(&mut app);
  drag(&mut app, ListId::Queue, 0, 2);
  assert_eq!(ids(&app.player.queue), ["b", "c", "a"]);
  assert_eq!(app.player.index, Some(2));
}

#[test]
fn releasing_on_the_pressed_row_is_a_click() {
  let mut app = app();
  ui(&mut app, UiMessage::RowPressed(ListId::Favorites, 1));
  ui(&mut app, UiMessage::RowReleased);
  assert_eq!(current_id(&app), "b");
}

#[test]
fn a_release_outside_any_row_cancels_the_drag() {
  let mut app = app();
  ui(&mut app, UiMessage::RowPressed(ListId::Favorites, 1));
  ui(&mut app, UiMessage::MouseReleased);
  assert_eq!(ui(&mut app, UiMessage::RowReleased), Vec::new());
  assert!(app.player.current.is_none());
}

#[test]
fn hovering_a_row_prefetches_its_stream() {
  let mut app = app();
  let effects = ui(&mut app, UiMessage::RowEntered(ListId::Favorites, 1));
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/prefetch",
      ..
    }
  )));
}

fn with_modifier(app: &mut App, modifiers: Modifiers) {
  ui(app, UiMessage::Modifiers(modifiers));
}

#[test]
fn ctrl_and_shift_select_instead_of_playing() {
  let mut app = app();
  with_modifier(&mut app, Modifiers::CTRL);
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 0));
  assert!(app.player.current.is_none());
  assert_eq!(app.ui.selection.len(), 1);
  with_modifier(&mut app, Modifiers::SHIFT);
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 2));
  assert_eq!(app.ui.selection.len(), 3);
  with_modifier(&mut app, Modifiers::CTRL);
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 1));
  assert_eq!(app.ui.selection.len(), 2);
  assert_eq!(app.update(Message::Escape), Vec::new());
  assert!(app.ui.selection.is_empty());
}

#[test]
fn select_all_toggles() {
  let mut app = app();
  with_modifier(&mut app, Modifiers::CTRL);
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 0));
  ui(&mut app, UiMessage::SelectAll);
  assert_eq!(app.ui.selection.len(), 3);
  ui(&mut app, UiMessage::SelectAll);
  assert!(app.ui.selection.is_empty());
}

#[test]
fn a_selection_goes_to_a_playlist_or_the_favorites() {
  let mut app = app();
  let id = app.data.create_playlist("P", Vec::new(), 1, 1).unwrap();
  with_modifier(&mut app, Modifiers::CTRL);
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 0));
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 1));
  ui(&mut app, UiMessage::SelectionToPlaylist);
  ui(&mut app, UiMessage::ModalChoose(id.clone()));
  assert_eq!(app.data.playlist(&id).unwrap().tracks.len(), 2);
  assert!(app.ui.modal.is_none());
  assert!(app.ui.selection.is_empty());
}

#[test]
fn removing_a_selection_from_the_queue() {
  let mut app = app();
  play_first(&mut app);
  with_modifier(&mut app, Modifiers::CTRL);
  ui(&mut app, UiMessage::RowActivated(ListId::Queue, 1));
  ui(&mut app, UiMessage::RowActivated(ListId::Queue, 2));
  ui(&mut app, UiMessage::SelectionRemove);
  assert_eq!(ids(&app.player.queue), ["a"]);
}

#[test]
fn the_row_menu_has_the_entries_of_riff_for_its_list() {
  let mut app = app();
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Favorites, 0, Point::new(1.0, 2.0)),
  );
  let labels: Vec<String> = app
    .menu
    .as_ref()
    .unwrap()
    .items
    .iter()
    .map(|item| item.label.clone())
    .collect();
  assert_eq!(
    labels,
    [
      "Remove from Favorites",
      "Add to playlist",
      "Add to queue",
      "Pin to sidebar",
      "Share link",
      "Download",
      "Change banner",
      "Rename track",
      "Block track",
      "Block artist"
    ]
  );
  app.update(Message::CloseMenu);
  play_first(&mut app);
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Queue, 0, Point::ORIGIN),
  );
  assert!(
    app
      .menu
      .as_ref()
      .unwrap()
      .items
      .iter()
      .any(|item| item.label == "Remove from queue")
  );
}

#[test]
fn the_row_menu_marks_dividers_icons_and_danger_entries_like_riff() {
  let mut app = app();
  play_first(&mut app);
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Queue, 0, Point::ORIGIN),
  );
  let items = &app.menu.as_ref().unwrap().items;
  assert!(items.iter().all(|item| item.icon.is_some()));
  let divided: Vec<&str> = items
    .iter()
    .filter(|item| item.divider_before)
    .map(|item| item.label.as_str())
    .collect();
  assert_eq!(divided, ["Remove from queue", "Block track"]);
  let danger: Vec<&str> = items
    .iter()
    .filter(|item| item.danger)
    .map(|item| item.label.as_str())
    .collect();
  assert_eq!(danger, ["Remove from queue", "Block track", "Block artist"]);
}

#[test]
fn the_menu_fades_in_and_out_over_180_ms_then_the_timer_stops() {
  let mut app = app();
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Favorites, 0, Point::ORIGIN),
  );
  assert_eq!(
    app.overlay_motion.menu_opacity(app.now),
    1.0,
    "hors application lancée : ouvert d'emblée"
  );
  assert!(!app.animation_needed());
  app.update(Message::CloseMenu);
  assert!(app.menu_leaving.is_none(), "banc : retiré aussitôt");
  app.overlay_motion.enabled = true;
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Favorites, 0, Point::ORIGIN),
  );
  let opened = app.now;
  assert_eq!(app.overlay_motion.menu_opacity(opened), 0.0);
  assert!(app.animation_needed());
  app.now = opened + Duration::from_millis(180);
  assert_eq!(app.overlay_motion.menu_opacity(app.now), 1.0);
  assert!(!app.animation_needed(), "posé : l'abonnement s'arrête");
  // Fermeture, depuis un menu ouvert depuis longtemps (l'horloge de `close_menu` est l'instant réel) : le menu quitte `menu` mais reste dessiné jusqu'à la fin du fondu.
  app.update(Message::CloseMenu);
  app.update(Message::OpenMenu(Box::new(
    app.menu_leaving.clone().unwrap(),
  )));
  app.overlay_motion = crate::motion::OverlayMotion::launched();
  app
    .overlay_motion
    .open_menu(app.now - Duration::from_secs(1));
  app.update(Message::CloseMenu);
  let closed = app.now;
  assert!(app.menu.is_none() && app.menu_leaving.is_some());
  assert!(app.animation_needed());
  assert_eq!(app.overlay_motion.menu_opacity(closed), 1.0);
  app.animation_tick(
    closed + Duration::from_millis(100),
    &[0; crate::visuals::viz::FREQ_BINS],
  );
  assert!(app.menu_leaving.is_some());
  app.animation_tick(
    closed + Duration::from_millis(181),
    &[0; crate::visuals::viz::FREQ_BINS],
  );
  assert!(app.menu_leaving.is_none());
  app.now = closed + Duration::from_millis(181);
  assert!(!app.animation_needed());
}

#[test]
fn escape_and_a_choice_fade_the_menu_and_a_new_menu_replaces_the_fading_one() {
  let mut app = app();
  app.overlay_motion.enabled = true;
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Favorites, 0, Point::ORIGIN),
  );
  app.update(Message::Escape);
  assert!(app.menu.is_none() && app.menu_leaving.is_some());
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Favorites, 1, Point::ORIGIN),
  );
  assert!(app.menu.is_some() && app.menu_leaving.is_none());
  app.update(Message::MenuChoose(Box::new(Message::Back)));
  assert!(app.menu.is_none() && app.menu_leaving.is_some());
}

#[test]
fn pinning_to_top_is_offered_on_favorites_and_playlists() {
  let mut app = app();
  app.update(Message::Navigate(Screen::Liked));
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Favorites, 2, Point::ORIGIN),
  );
  assert!(
    app
      .menu
      .as_ref()
      .unwrap()
      .items
      .iter()
      .any(|item| item.label == "Pin to top")
  );
  let track = app.list_track(ListId::Favorites, 2).cloned().unwrap();
  ui(
    &mut app,
    UiMessage::Track(
      Box::new(track),
      TrackCommand::PinToTop(crate::library::FAVORITES_SCOPE.to_string()),
    ),
  );
  assert_eq!(
    app.list_track(ListId::Favorites, 0).unwrap().id.as_deref(),
    Some("c")
  );
}

#[test]
fn track_commands_do_what_their_label_says() {
  let mut app = app();
  let command = |app: &mut App, command: TrackCommand| {
    ui(app, UiMessage::RowCommand(ListId::Favorites, 0, command))
  };
  command(&mut app, TrackCommand::ToggleFavorite);
  assert_eq!(app.data.favorites.len(), 2);
  let effects = ui(
    &mut app,
    UiMessage::Track(Box::new(track("q")), TrackCommand::AddToQueue),
  );
  assert_eq!(current_id(&app), "q", "file vide : le titre démarre");
  assert!(resolved(&effects).is_some());
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("r")), TrackCommand::AddToQueue),
  );
  assert_eq!(current_id(&app), "q");
  assert_eq!(app.player.queue.len(), 2);
  let effects = command(&mut app, TrackCommand::Share);
  assert!(matches!(&effects[0], Effect::CopyText(url) if url.starts_with("https://exemple.org/")));
  let local = Track {
    platform: Some("local".to_string()),
    url: Some("http://127.0.0.1:1/x".to_string()),
    ..track("l")
  };
  let effects = ui(
    &mut app,
    UiMessage::Track(Box::new(local), TrackCommand::Share),
  );
  assert!(
    matches!(effects.as_slice(), [Effect::Toast(text, _)] if text == "No link to share for this track")
  );
  command(&mut app, TrackCommand::BlockTrack);
  assert_eq!(app.data.blocked_tracks.len(), 1);
  command(&mut app, TrackCommand::PinToSidebar);
  assert_eq!(app.data.sidebar_pinned_tracks.len(), 1);
  command(&mut app, TrackCommand::OpenArtist("Muse".to_string()));
  assert_eq!(app.screen, Screen::Artist);
}

#[test]
fn renaming_a_track_updates_the_playing_one_and_is_published() {
  let mut app = app();
  play_first(&mut app);
  ui(&mut app, UiMessage::CurrentCommand(TrackCommand::Rename));
  ui(&mut app, UiMessage::ModalTitle("Neuf".to_string()));
  ui(&mut app, UiMessage::ModalArtist("Autre".to_string()));
  let effects = ui(&mut app, UiMessage::ModalConfirm);
  assert_eq!(
    app.player.current.as_ref().unwrap().title.as_deref(),
    Some("Neuf")
  );
  assert_eq!(app.data.favorites[0].artist.as_deref(), Some("Autre"));
  assert_eq!(app.player.queue[0].title.as_deref(), Some("Neuf"));
  assert!(app.ui.modal.is_none());
  assert!(
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::PublishAppState(state) if state["title"] == "Neuf"))
  );
}

#[test]
fn a_playlist_is_created_from_the_dialog_and_opened() {
  let mut app = app();
  assert_eq!(
    ui(&mut app, UiMessage::NewPlaylist),
    vec![Effect::FocusOverlay]
  );
  assert_eq!(
    ui(&mut app, UiMessage::ModalConfirm),
    Vec::new(),
    "nom vide refusé"
  );
  ui(&mut app, UiMessage::ModalName(" Liste ".to_string()));
  ui(&mut app, UiMessage::ModalConfirm);
  assert_eq!(app.data.playlists[0].name.as_deref(), Some("Liste"));
  assert_eq!(app.screen, Screen::PlaylistDetail);
  assert_eq!(app.ui.playlist_id, app.data.playlists[0].id);
  assert!(app.ui.modal.is_none());
}

#[test]
fn escape_closes_the_playlist_dialog() {
  let mut app = app();
  ui(&mut app, UiMessage::NewPlaylist);
  app.update(Message::Escape);
  assert!(app.ui.modal.is_none());
}

fn with_playlist(app: &mut App) -> String {
  let id = app
    .data
    .create_playlist("P", tracks(&["x", "y", "z"]), 1, 1)
    .unwrap();
  ui(app, UiMessage::OpenPlaylist(id.clone()));
  id
}

#[test]
fn a_playlist_can_be_edited_sorted_and_reordered() {
  let mut app = app();
  let id = with_playlist(&mut app);
  ui(&mut app, UiMessage::PlaylistRename("Nouveau".to_string()));
  ui(
    &mut app,
    UiMessage::PlaylistDescription("Texte".to_string()),
  );
  assert_eq!(
    app.data.playlist(&id).unwrap().name.as_deref(),
    Some("Nouveau")
  );
  assert_eq!(
    app.data.playlist(&id).unwrap().description.as_deref(),
    Some("Texte")
  );
  drag(&mut app, ListId::Playlist, 2, 0);
  assert_eq!(
    ids(&app.data.playlist(&id).unwrap().tracks),
    ["z", "x", "y"]
  );
  ui(
    &mut app,
    UiMessage::RowCommand(ListId::Playlist, 0, TrackCommand::RemoveFromPlaylist),
  );
  assert_eq!(ids(&app.data.playlist(&id).unwrap().tracks), ["x", "y"]);
  ui(&mut app, UiMessage::PlaylistQuery("TITRE Y".to_string()));
  assert_eq!(app.list_len(ListId::Playlist), 1);
}

#[test]
fn deleting_a_playlist_asks_first() {
  let mut app = app();
  let id = with_playlist(&mut app);
  assert_eq!(
    ui(&mut app, UiMessage::PlaylistDelete),
    vec![Effect::FocusOverlay]
  );
  assert!(app.dialog.is_some());
  assert!(app.data.playlist(&id).is_some());
  app.update(Message::ConfirmDialog);
  assert!(app.data.playlist(&id).is_none());
  assert_eq!(app.screen, Screen::Home);
}

#[test]
fn playing_a_playlist_keeps_its_order_while_its_shuffle_is_off() {
  let mut app = app();
  let id = with_playlist(&mut app);
  ui(&mut app, UiMessage::PlaylistPlay);
  assert!(!app.player.shuffle && !app.data.shuffle);
  assert_eq!(ids(&app.player.queue), ["x", "y", "z"]);
  assert_eq!(app.player.scope, Some(format!("pl:{id}")));
}

#[test]
fn playing_a_playlist_with_shuffle_on_mixes_it_and_leaves_the_global_setting() {
  let mut app = app();
  let id = with_playlist(&mut app);
  ui(&mut app, UiMessage::ListShuffleToggle);
  assert!(app.data.shuffle_on(&format!("pl:{id}")));
  ui(&mut app, UiMessage::PlaylistPlay);
  assert!(app.player.shuffle && !app.data.shuffle);
  let mut queue = ids(&app.player.queue);
  queue.sort_unstable();
  assert_eq!(queue, ["x", "y", "z"]);
}

#[test]
fn playing_a_shuffled_list_starts_on_an_unblocked_track_after_the_mix() {
  for _ in 0..20 {
    let mut app = app();
    let id = with_playlist(&mut app);
    ui(&mut app, UiMessage::ListShuffleToggle);
    assert!(app.data.shuffle_on(&format!("pl:{id}")));
    app.data.toggle_block_track(&track("x"));
    app.data.toggle_block_track(&track("y"));
    ui(&mut app, UiMessage::PlaylistPlay);
    assert_eq!(current_id(&app), "z");
    assert_eq!(
      app.player.queue[app.player.index.unwrap()].id.as_deref(),
      Some("z")
    );
  }
}

#[test]
fn playing_a_list_starts_on_its_first_unblocked_track() {
  let mut app = app();
  with_playlist(&mut app);
  app.data.toggle_block_track(&track("x"));
  ui(&mut app, UiMessage::PlaylistPlay);
  assert_eq!(current_id(&app), "y");
  assert_eq!(app.player.index, Some(1));
}

#[test]
fn playing_a_fully_blocked_list_tells_it_and_starts_nothing() {
  let mut app = app();
  with_playlist(&mut app);
  for id in ["x", "y", "z"] {
    app.data.toggle_block_track(&track(id));
  }
  let effects = ui(&mut app, UiMessage::PlaylistPlay);
  assert!(app.player.current.is_none());
  assert!(app.player.queue.is_empty());
  assert!(matches!(
    effects.as_slice(),
    [Effect::Toast(text, _)] if text == "All these tracks are blocked"
  ));
}

#[test]
fn the_shuffle_of_a_list_does_not_touch_the_others() {
  let mut app = app();
  let first = with_playlist(&mut app);
  let second = app
    .data
    .create_playlist("Q", tracks(&["u", "v"]), 2, 2)
    .unwrap();
  app.data.favorites = tracks(&["a", "b"]);
  ui(&mut app, UiMessage::ListShuffleToggle);
  assert_eq!(app.data.shuffle_lists, [format!("pl:{first}")]);
  ui(&mut app, UiMessage::OpenPlaylist(second.clone()));
  assert!(!app.data.shuffle_on(&format!("pl:{second}")));
  ui(&mut app, UiMessage::PlaylistPlay);
  assert!(!app.player.shuffle);
  app.update(Message::Navigate(Screen::Liked));
  ui(&mut app, UiMessage::ListShuffleToggle);
  assert!(app.data.shuffle_on(FAVORITES_SCOPE));
  assert_eq!(app.data.shuffle_lists.len(), 2);
  ui(&mut app, UiMessage::ListShuffleToggle);
  assert!(!app.data.shuffle_on(FAVORITES_SCOPE) && app.data.shuffle_on(&format!("pl:{first}")));
}

#[test]
fn the_list_shuffle_button_updates_the_player_only_for_the_playing_list() {
  let mut app = app();
  let first = with_playlist(&mut app);
  ui(&mut app, UiMessage::PlaylistPlay);
  assert!(!app.player.shuffle);
  let effects = ui(&mut app, UiMessage::ListShuffleToggle);
  assert!(matches!(
    effects.as_slice(),
    [Effect::ScheduleSave(_), Effect::SaveResume(_)]
  ));
  assert!(app.player.shuffle);
  let second = app.data.create_playlist("Q", tracks(&["u"]), 2, 2).unwrap();
  ui(&mut app, UiMessage::OpenPlaylist(second));
  ui(&mut app, UiMessage::ListShuffleToggle);
  assert_eq!(app.player.scope, Some(format!("pl:{first}")));
  assert!(app.player.shuffle, "la file en cours n'est pas celle-ci");
}

#[test]
fn a_row_of_a_list_applies_the_flag_of_that_list() {
  let mut app = app();
  app.data.favorites = tracks(&["a", "b", "c"]);
  app.data.shuffle_lists = vec![FAVORITES_SCOPE.to_string()];
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 1));
  assert!(app.player.shuffle && !app.data.shuffle);
  assert_eq!(app.player.scope.as_deref(), Some(FAVORITES_SCOPE));
  assert_eq!(ids(&app.player.queue), ["a", "b", "c"]);
  app.data.shuffle_lists.clear();
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 1));
  assert!(!app.player.shuffle);
}

#[test]
fn the_playbar_toggle_follows_the_list_of_the_queue() {
  let mut app = app();
  app.data.favorites = tracks(&["a", "b"]);
  ui(&mut app, UiMessage::FavoritesPlay);
  player(&mut app, PlayerMessage::ToggleShuffle);
  assert!(app.player.shuffle && app.data.shuffle_on(FAVORITES_SCOPE));
  assert!(!app.data.shuffle, "le réglage global reste intact");
  player(&mut app, PlayerMessage::ToggleShuffle);
  assert!(!app.player.shuffle && !app.data.shuffle_on(FAVORITES_SCOPE));
}

#[test]
fn outside_a_list_the_global_shuffle_applies_again() {
  let mut app = app();
  app.data.favorites = tracks(&["a", "b"]);
  app.data.shuffle_lists = vec![FAVORITES_SCOPE.to_string()];
  app.data.shuffle = false;
  ui(&mut app, UiMessage::FavoritesPlay);
  assert!(app.player.shuffle);
  let single = tracks(&["s"]).remove(0);
  ui(&mut app, UiMessage::SidebarPlayTrack(Box::new(single)));
  assert!(!app.player.shuffle && app.player.scope.is_none());
  player(&mut app, PlayerMessage::ToggleShuffle);
  assert!(app.data.shuffle && app.data.shuffle_lists == [FAVORITES_SCOPE]);
}

#[test]
fn deleting_a_playlist_forgets_its_shuffle() {
  let mut app = app();
  let id = with_playlist(&mut app);
  ui(&mut app, UiMessage::ListShuffleToggle);
  app.data.shuffle_lists.push(FAVORITES_SCOPE.to_string());
  ui(&mut app, UiMessage::PlaylistDeleted(id));
  assert_eq!(app.data.shuffle_lists, [FAVORITES_SCOPE]);
}

#[test]
fn toggling_shuffle_after_deleting_the_playing_playlist_leaves_no_orphan_scope() {
  let mut app = app();
  let id = with_playlist(&mut app);
  ui(&mut app, UiMessage::RowActivated(ListId::Playlist, 0));
  ui(&mut app, UiMessage::PlaylistDeleted(id));
  player(&mut app, PlayerMessage::ToggleShuffle);
  assert!(app.data.shuffle_lists.is_empty());
  assert!(app.data.shuffle);
}

#[test]
fn playing_the_favorites_follows_the_displayed_order_while_shuffle_is_off() {
  let mut app = app();
  app.data.favorites = tracks(&["a", "b", "c"]);
  app.data.shuffle = true;
  ui(&mut app, UiMessage::FavoritesPlay);
  assert!(
    !app.player.shuffle && app.data.shuffle,
    "réglage global intact"
  );
  assert_eq!(ids(&app.player.queue), ["a", "b", "c"]);
  assert_eq!(
    app.player.current.as_ref().unwrap().id.as_deref(),
    Some("a")
  );
}

#[test]
fn playing_the_favorites_with_shuffle_on_keeps_every_title() {
  let mut app = app();
  app.data.favorites = tracks(&["a", "b", "c", "d", "e"]);
  app.data.shuffle_lists = vec![FAVORITES_SCOPE.to_string()];
  ui(&mut app, UiMessage::FavoritesPlay);
  assert!(app.player.shuffle);
  let mut queue: Vec<&str> = ids(&app.player.queue);
  assert_eq!(
    queue[0],
    app.player.current.as_ref().unwrap().id.as_deref().unwrap()
  );
  queue.sort_unstable();
  assert_eq!(queue, ["a", "b", "c", "d", "e"]);
}

#[test]
fn the_favorites_play_button_does_nothing_when_there_are_none() {
  let mut app = app();
  app.data.favorites.clear();
  let effects = ui(&mut app, UiMessage::FavoritesPlay);
  assert!(
    !effects
      .iter()
      .any(|effect| matches!(effect, Effect::AudioPlay))
  );
  assert!(app.player.queue.is_empty() && app.player.current.is_none());
  assert!(!app.player.shuffle);
}

#[test]
fn exporting_the_favorites_names_the_file_liked_songs() {
  let mut app = app();
  app.data.favorites = tracks(&["a", "b"]);
  let effects = ui(&mut app, UiMessage::FavoritesExport);
  assert!(matches!(
    &effects[0],
    Effect::ExportPlaylist { name, content }
      if name == "Liked Songs.json"
        && content.contains("\"name\": \"Liked Songs\"")
        && content.contains("\"app\": \"Riff\"")
  ));
}

#[test]
fn exporting_a_playlist_prepares_its_file() {
  let mut app = app();
  let id = with_playlist(&mut app);
  let effects = ui(&mut app, UiMessage::PlaylistExport(id));
  assert!(
    matches!(&effects[0], Effect::ExportPlaylist { name, content } if name == "P.json" && content.contains("\"app\": \"Riff\""))
  );
}

#[test]
fn a_chosen_image_becomes_the_playlist_cover() {
  let mut app = app();
  let id = with_playlist(&mut app);
  assert_eq!(
    ui(&mut app, UiMessage::PlaylistPickCover),
    vec![Effect::PickPlaylistCover(id.clone())]
  );
  ui(
    &mut app,
    UiMessage::PlaylistCoverPicked(id.clone(), Some(PathBuf::from("c.png"))),
  );
  assert_eq!(
    app.data.playlist(&id).unwrap().cover.as_deref(),
    Some("c.png")
  );
}

#[test]
fn the_liked_songs_page_edits_like_a_playlist() {
  let mut app = app();
  assert_eq!(app.liked_title(), "Liked songs");
  ui(
    &mut app,
    UiMessage::FavoritesRename("Coups de coeur".to_string()),
  );
  ui(
    &mut app,
    UiMessage::FavoritesDescription("Texte".to_string()),
  );
  assert_eq!(app.liked_title(), "Coups de coeur");
  assert_eq!(app.data.liked_description.as_deref(), Some("Texte"));
  // Champ effacé : retour au libellé traduit.
  ui(&mut app, UiMessage::FavoritesRename(String::new()));
  assert_eq!(app.liked_title(), "Liked songs");
  assert_eq!(
    ui(&mut app, UiMessage::FavoritesPickCover),
    vec![Effect::PickFavoritesCover]
  );
  ui(
    &mut app,
    UiMessage::FavoritesCoverPicked(Some(PathBuf::from("c.png"))),
  );
  assert_eq!(app.data.liked_cover.as_deref(), Some("c.png"));
  assert_eq!(app.hero_url_on(Screen::Liked).as_deref(), Some("c.png"));
  // Sélecteur annulé : rien ne change.
  assert!(ui(&mut app, UiMessage::FavoritesCoverPicked(None)).is_empty());
  app.screen = Screen::Liked;
  ui(&mut app, UiMessage::CoverReset);
  assert_eq!(app.data.liked_cover, None);
  assert_eq!(app.hero_url_on(Screen::Liked), None);
}

#[test]
fn a_cover_can_be_reset_on_a_playlist_too() {
  let mut app = app();
  let id = with_playlist(&mut app);
  app.screen = Screen::PlaylistDetail;
  ui(
    &mut app,
    UiMessage::PlaylistCoverPicked(id.clone(), Some(PathBuf::from("c.png"))),
  );
  ui(&mut app, UiMessage::CoverReset);
  assert_eq!(app.data.playlist(&id).unwrap().cover, None);
}

#[test]
fn the_custom_liked_name_and_description_are_used_for_the_export() {
  let mut app = app();
  app.data.favorites = tracks(&["a"]);
  ui(
    &mut app,
    UiMessage::FavoritesRename("Coups de coeur".to_string()),
  );
  ui(
    &mut app,
    UiMessage::FavoritesDescription("Texte".to_string()),
  );
  let effects = ui(&mut app, UiMessage::FavoritesExport);
  assert!(matches!(
    &effects[0],
    Effect::ExportPlaylist { name, content }
      if name == "Coups de coeur.json"
        && content.contains("\"name\": \"Coups de coeur\"")
        && content.contains("\"description\": \"Texte\"")
  ));
}

#[test]
fn local_files_flow_from_the_picker_to_the_list() {
  let mut app = app();
  assert_eq!(
    ui(&mut app, UiMessage::PickFolder),
    vec![Effect::PickFolder]
  );
  let picked = vec![PathBuf::from("D:/m")];
  assert_eq!(
    ui(&mut app, UiMessage::LocalPicked(picked.clone(), true)),
    vec![Effect::ScanLocal(picked, true)]
  );
  let file = Track {
    path: Some("D:/m/a.mp3".into()),
    platform: Some("local".into()),
    id: Some("l".into()),
    ..Track::default()
  };
  let effects = ui(&mut app, UiMessage::LocalFound(vec![file]));
  assert_eq!(app.list_len(ListId::Local), 1);
  assert!(
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::Toast(text, _) if text == "Added 1 track"))
  );
  let effects = ui(
    &mut app,
    UiMessage::RowCommand(ListId::Local, 0, TrackCommand::RemoveLocal),
  );
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/library-remove",
      ..
    }
  )));
  assert_eq!(app.list_len(ListId::Local), 0);
}

#[test]
fn opening_my_files_rescans_missing_metadata_once() {
  let mut app = app();
  app.data.local_files = vec![Track {
    path: Some("a".into()),
    ..Track::default()
  }];
  let effects = app.update(Message::Navigate(Screen::Library));
  assert!(effects.contains(&Effect::RescanLocal(vec!["a".to_string()])));
  app.update(Message::Navigate(Screen::Home));
  let effects = app.update(Message::Navigate(Screen::Library));
  assert!(
    !effects
      .iter()
      .any(|effect| matches!(effect, Effect::RescanLocal(_)))
  );
}

#[test]
fn local_albums_open_with_their_tracks_in_order() {
  let mut app = app();
  let file = |number: i64| {
    let mut track = Track {
      album: Some(" Alb ".into()),
      artist: Some("A".into()),
      id: Some(format!("t{number}")),
      ..Track::default()
    };
    track.extra.insert("trackNumber".to_string(), json!(number));
    track
  };
  app.data.local_files = vec![file(2), file(1)];
  ui(&mut app, UiMessage::OpenLocalAlbum("Alb".to_string()));
  assert_eq!(app.screen, Screen::Album);
  assert_eq!(ids(&app.ui.album.tracks), ["t1", "t2"]);
  assert_eq!(app.ui.album.album.kind.as_deref(), Some("Local Album"));
}

#[test]
fn dropped_files_are_gathered_then_scanned_together() {
  let mut app = app();
  ui(&mut app, UiMessage::FileDropped(PathBuf::from("a.mp3")));
  let effects = ui(&mut app, UiMessage::FileDropped(PathBuf::from("b.mp3")));
  let [Effect::DropTimer(generation)] = effects.as_slice() else {
    panic!("{effects:?}")
  };
  let generation = *generation;
  assert_eq!(
    ui(&mut app, UiMessage::DropFlush(generation - 1)),
    Vec::new()
  );
  let effects = ui(&mut app, UiMessage::DropFlush(generation));
  assert_eq!(app.screen, Screen::Library);
  assert!(effects.contains(&Effect::ScanDropped(vec![
    PathBuf::from("a.mp3"),
    PathBuf::from("b.mp3")
  ])));
}

#[test]
fn clearing_history_and_queue() {
  let mut app = app();
  play_first(&mut app);
  ui(&mut app, UiMessage::ClearHistory);
  assert!(app.data.history.is_empty());
  ui(&mut app, UiMessage::ClearQueue);
  assert!(app.player.queue.is_empty());
  assert!(app.player.current.is_some());
}

#[test]
fn dragging_the_sidebar_edge_resizes_and_saves_on_release_only() {
  let mut app = app();
  assert_eq!(app.data.sidebar_width, 288);
  assert!(ui(&mut app, UiMessage::SidebarDragMoved(500.0)).is_empty());
  assert_eq!(
    app.data.sidebar_width, 288,
    "aucun effet sans glisser en cours"
  );
  assert!(ui(&mut app, UiMessage::SidebarDragStart).is_empty());
  assert!(ui(&mut app, UiMessage::SidebarDragMoved(290.0)).is_empty());
  assert_eq!(
    app.data.sidebar_width, 288,
    "le premier déplacement sert de repère"
  );
  assert!(ui(&mut app, UiMessage::SidebarDragMoved(310.0)).is_empty());
  assert_eq!(app.data.sidebar_width, 309, "20 px écran = 21,2 px CSS");
  assert!(ui(&mut app, UiMessage::SidebarDragMoved(100.0)).is_empty());
  assert_eq!(app.data.sidebar_width, 84, "sous 180 px : repliée");
  assert!(ui(&mut app, UiMessage::SidebarDragMoved(250.0)).is_empty());
  assert_eq!(
    app.data.sidebar_width, 256,
    "le repère reste celui du début"
  );
  let effects = ui(&mut app, UiMessage::SidebarDragEnd);
  assert!(matches!(effects.as_slice(), [Effect::ScheduleSave(_)]));
  assert_eq!(app.ui.sidebar_drag, None);
  assert!(
    ui(&mut app, UiMessage::SidebarDragEnd).is_empty(),
    "relâchement sans glisser"
  );
}

#[test]
fn a_double_click_on_the_sidebar_edge_folds_and_unfolds() {
  let mut app = app();
  // Suite d'iced : appui, relâchement, appui, double clic, relâchement.
  ui(&mut app, UiMessage::SidebarDragStart);
  ui(&mut app, UiMessage::SidebarDragEnd);
  ui(&mut app, UiMessage::SidebarDragStart);
  let effects = ui(&mut app, UiMessage::SidebarToggle);
  assert!(matches!(effects.as_slice(), [Effect::ScheduleSave(_)]));
  assert_eq!(app.data.sidebar_width, 84);
  ui(&mut app, UiMessage::SidebarDragMoved(90.0));
  ui(&mut app, UiMessage::SidebarDragMoved(95.0));
  assert_eq!(
    app.data.sidebar_width, 84,
    "le double clic met fin au glisser"
  );
  ui(&mut app, UiMessage::SidebarDragEnd);
  ui(&mut app, UiMessage::SidebarToggle);
  assert_eq!(app.data.sidebar_width, 288);
}

#[test]
fn the_sidebar_edge_reports_its_hover() {
  let mut app = app();
  ui(&mut app, UiMessage::SidebarEdgeHover(true));
  assert!(app.ui.sidebar_edge_hovered);
  ui(&mut app, UiMessage::SidebarEdgeHover(false));
  assert!(!app.ui.sidebar_edge_hovered);
}

#[test]
fn the_sidebar_order_can_change_and_be_reset() {
  let mut app = app();
  app.settings.show_albums = true;
  let first = |app: &App| app.sidebar_screens()[0].1;
  assert_eq!(first(&app), Screen::Home);
  ui(
    &mut app,
    UiMessage::SidebarMove("favorites".to_string(), -1),
  );
  assert_eq!(first(&app), Screen::Liked);
  ui(
    &mut app,
    UiMessage::SidebarMove("favorites".to_string(), -1),
  );
  assert_eq!(first(&app), Screen::Liked, "déjà en tête");
  ui(&mut app, UiMessage::SidebarReset);
  assert_eq!(first(&app), Screen::Home);
  assert_eq!(app.sidebar_screens().len(), 6);
}

#[test]
fn the_albums_tab_is_hidden_by_default_like_riff() {
  let app = app();
  assert!(
    !app
      .sidebar_screens()
      .iter()
      .any(|(_, screen)| *screen == Screen::Albums)
  );
}

#[test]
fn hiding_the_current_tab_goes_back_home() {
  let mut app = app();
  app.settings.show_local = true;
  app.update(Message::Navigate(Screen::Library));
  app.update(Message::Settings(SettingsMessage::SetBool(
    BoolKey::ShowLocal,
    false,
  )));
  assert_eq!(app.screen, Screen::Home);
  assert!(
    !app
      .sidebar_screens()
      .iter()
      .any(|(_, screen)| *screen == Screen::Library)
  );
}

#[test]
fn an_imported_order_keeps_known_entries_and_adds_the_missing_ones() {
  let mut app = app();
  app.settings.show_albums = true;
  app.data.sidebar_order = vec!["queue".to_string(), "inconnu".to_string()];
  let screens: Vec<Screen> = app
    .sidebar_screens()
    .into_iter()
    .map(|(_, screen)| screen)
    .collect();
  assert_eq!(screens[0], Screen::Queue);
  assert_eq!(screens.len(), 6);
  assert_eq!(screens[1], Screen::Home);
}

#[test]
fn the_sidebar_tab_changes_and_pinned_tracks_play() {
  let mut app = app();
  ui(&mut app, UiMessage::SidebarTab(SidebarTab::Queue));
  assert_eq!(app.ui.sidebar_tab, SidebarTab::Queue);
  app.data.sidebar_pinned_tracks = tracks(&["p"]);
  let pinned = app.data.sidebar_pinned_tracks[0].clone();
  ui(&mut app, UiMessage::SidebarPlayTrack(Box::new(pinned)));
  assert_eq!(current_id(&app), "p");
  assert_eq!(app.player.queue.len(), 1);
}

#[test]
fn changing_the_sidebar_tab_resets_the_feed() {
  let mut app = app();
  ui(&mut app, UiMessage::SidebarRowEntered(3));
  ui(&mut app, UiMessage::SidebarFeedScrolled(120.0));
  assert_eq!(app.ui.sidebar_hover, Some(3));
  assert_eq!(app.ui.sidebar_scroll, 120.0);
  let effects = ui(&mut app, UiMessage::SidebarTab(SidebarTab::Albums));
  assert_eq!(app.ui.sidebar_hover, None);
  assert_eq!(app.ui.sidebar_scroll, 0.0);
  assert!(
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::SidebarFeedTop))
  );
}

#[test]
fn a_sidebar_tab_click_replays_the_row_entry_only_in_the_launched_app() {
  let mut app = app();
  ui(&mut app, UiMessage::SidebarTab(SidebarTab::Albums));
  assert_eq!(app.entry_motion.sidebar, None, "banc : au repos");
  app.entry_motion.enabled = true;
  ui(&mut app, UiMessage::SidebarTab(SidebarTab::Queue));
  let start = app.now;
  assert_eq!(app.entry_motion.sidebar, Some(start));
  assert!(app.animation_needed());
  app.now = start + Duration::from_millis(479);
  assert!(app.animation_needed());
  app.now = start + Duration::from_millis(480);
  assert!(
    !app.animation_needed(),
    "rang 12 fini à 12 * 20 + 240 ms : l'abonnement s'arrête"
  );
  // Un clic sur l'onglet déjà actif rejoue aussi (`renderSidebarLibraryFeed(true)`).
  ui(&mut app, UiMessage::SidebarTab(SidebarTab::Queue));
  assert!(app.entry_motion.sidebar.is_some_and(|again| again > start));
}

#[test]
fn a_sidebar_row_stays_hovered_until_its_own_exit() {
  let mut app = app();
  ui(&mut app, UiMessage::SidebarRowEntered(1));
  ui(&mut app, UiMessage::SidebarRowEntered(2));
  // La sortie de la rangée 1 arrive après l'entrée dans la rangée 2 : elle ne doit rien effacer.
  ui(&mut app, UiMessage::SidebarRowLeft(1));
  assert_eq!(app.ui.sidebar_hover, Some(2));
  ui(&mut app, UiMessage::SidebarRowLeft(2));
  assert_eq!(app.ui.sidebar_hover, None);
}

#[test]
fn a_sidebar_track_plays_alone_from_its_fullest_copy() {
  let mut app = app();
  let mut saved = track("f");
  saved.album = Some("Album complet".to_string());
  app.data.favorites = vec![saved];
  app
    .player
    .play_list(track("a"), tracks(&["a", "b"]), 0, "ytmusic");
  let mut light = track("f");
  light.album = None;
  ui(&mut app, UiMessage::SidebarPlayTrack(Box::new(light)));
  assert_eq!(current_id(&app), "f");
  assert_eq!(app.player.queue.len(), 1);
  assert_eq!(
    app
      .player
      .current
      .as_ref()
      .and_then(|track| track.album.as_deref()),
    Some("Album complet")
  );
}

#[test]
fn escape_goes_back_from_the_album_and_artist_pages() {
  let mut app = app();
  ui(&mut app, UiMessage::OpenArtist("Muse".to_string()));
  assert_eq!(app.screen, Screen::Artist);
  app.update(Message::Escape);
  assert_eq!(app.screen, Screen::Home);
}

#[test]
fn closing_a_menu_gives_the_focus_back() {
  let mut app = app();
  ui(&mut app, UiMessage::FocusSaved(Some(7)));
  ui(
    &mut app,
    UiMessage::RowMenu(ListId::Favorites, 0, Point::ORIGIN),
  );
  assert_eq!(
    app.update(Message::CloseMenu),
    vec![Effect::RestoreFocus(Some(7))]
  );
  assert_eq!(app.ui.saved_focus, None);
}

#[test]
fn wanted_images_cover_the_visible_rows_only() {
  let mut app = app();
  app.data.favorites = (0..1000)
    .map(|index| Track {
      thumbnail: Some(format!("u{index}")),
      ..track(&format!("i{index}"))
    })
    .collect();
  app.refresh_orders();
  app.update(Message::Navigate(Screen::Liked));
  let wanted = app.wanted_images();
  assert!(wanted.iter().any(|key| key.url == "u0"));
  assert!(!wanted.iter().any(|key| key.url == "u500"));
  assert!(wanted.len() < 40, "{}", wanted.len());
  ui(
    &mut app,
    UiMessage::ListScrolled(ListId::Favorites, crate::ui::ROW_HEIGHT * 500.0, 600.0),
  );
  assert!(app.wanted_images().iter().any(|key| key.url == "u500"));
}

/// La page « Titres aimés » défile d'un bloc : le défilement de la liste est celui de la page moins l'en-tête.
#[test]
fn the_favorites_page_scroll_is_relative_to_the_list() {
  let mut app = app();
  let top = crate::screens::liked::list_top();
  ui(&mut app, UiMessage::FavoritesScrolled(top - 10.0, 600.0));
  assert_eq!(
    app.ui.list(ListId::Favorites).offset,
    0.0,
    "au-dessus de la liste"
  );
  ui(&mut app, UiMessage::FavoritesScrolled(top + 500.0, 600.0));
  assert!((app.ui.list(ListId::Favorites).offset - 500.0).abs() < 0.01);
  assert_eq!(app.ui.list(ListId::Favorites).viewport_height, 600.0);
}

/// La page d'album défile d'un bloc : le défilement de la liste est celui de la page moins le haut de la page, et la
/// liste est virtualisée au pas des lignes d'album.
#[test]
fn the_album_page_scroll_is_relative_to_the_list() {
  let mut app = app();
  let top = crate::screens::album::list_top();
  ui(&mut app, UiMessage::AlbumScrolled(top - 10.0, 600.0));
  assert_eq!(app.ui.list(ListId::Album).offset, 0.0);
  ui(&mut app, UiMessage::AlbumScrolled(top + 500.0, 600.0));
  let list = app.ui.list(ListId::Album);
  assert!((list.offset - 500.0).abs() < 0.01);
  assert!(
    list.row_height < crate::ui::ROW_HEIGHT,
    "{}",
    list.row_height
  );
}

/// La page « Historique » défile d'un bloc : le défilement de la liste est celui de la page moins l'en-tête.
#[test]
fn the_history_page_scroll_is_relative_to_the_list() {
  let mut app = app();
  let top = crate::screens::history::list_top();
  ui(&mut app, UiMessage::HistoryScrolled(top - 10.0, 600.0));
  assert_eq!(
    app.ui.list(ListId::History).offset,
    0.0,
    "au-dessus de la liste"
  );
  ui(&mut app, UiMessage::HistoryScrolled(top + 500.0, 600.0));
  assert!((app.ui.list(ListId::History).offset - 500.0).abs() < 0.01);
  assert_eq!(app.ui.list(ListId::History).viewport_height, 600.0);
}

/// « Clear history » vide l'historique d'écoute sans toucher aux favoris.
#[test]
fn clear_history_empties_only_the_history() {
  let mut app = app();
  app.data.history = vec![track("h0")];
  let favorites = app.data.favorites.len();
  ui(&mut app, UiMessage::ClearHistory);
  assert!(app.data.history.is_empty());
  assert_eq!(app.data.favorites.len(), favorites);
}

/// La page « File d'attente » défile d'un bloc : le défilement de la liste est celui de la page moins l'en-tête.
#[test]
fn the_queue_page_scroll_is_relative_to_the_list() {
  let mut app = app();
  let top = crate::screens::queue::list_top();
  ui(&mut app, UiMessage::QueueScrolled(top - 10.0, 600.0));
  assert_eq!(
    app.ui.list(ListId::Queue).offset,
    0.0,
    "au-dessus de la liste"
  );
  ui(&mut app, UiMessage::QueueScrolled(top + 500.0, 600.0));
  assert!((app.ui.list(ListId::Queue).offset - 500.0).abs() < 0.01);
  assert_eq!(app.ui.list(ListId::Queue).viewport_height, 600.0);
}

/// `renderFavoritesView` : la recherche porte sur le titre et l'artiste, sans tenir compte de la casse.
#[test]
fn the_favorites_search_matches_title_and_artist() {
  let mut app = app();
  app.data.favorites = vec![
    Track {
      title: Some("Hyrule".into()),
      artist: Some("LuvResval".into()),
      ..track("a")
    },
    Track {
      title: Some("Céleste".into()),
      artist: Some("Autre".into()),
      ..track("b")
    },
  ];
  ui(&mut app, UiMessage::FavoritesQuery("LUV".into()));
  assert_eq!(app.list_len(ListId::Favorites), 1);
  ui(&mut app, UiMessage::FavoritesQuery("célest".into()));
  assert_eq!(app.list_len(ListId::Favorites), 1);
  ui(&mut app, UiMessage::FavoritesQuery("zzz".into()));
  assert_eq!(app.list_len(ListId::Favorites), 0);
  ui(&mut app, UiMessage::FavoritesQuery(String::new()));
  assert_eq!(app.list_len(ListId::Favorites), 2);
}

/// La page du détail d'une playlist défile d'un bloc : le défilement de la liste est celui de la page moins l'en-tête.
#[test]
fn the_playlist_page_scroll_is_relative_to_the_list() {
  let mut app = app();
  let top = crate::screens::playlist_detail::list_top();
  ui(&mut app, UiMessage::PlaylistScrolled(top - 10.0, 600.0));
  assert_eq!(app.ui.list(ListId::Playlist).offset, 0.0);
  ui(&mut app, UiMessage::PlaylistScrolled(top + 500.0, 600.0));
  assert!((app.ui.list(ListId::Playlist).offset - 500.0).abs() < 0.01);
}

/// Le menu de tri s'ouvre ; choisir un tri le ferme, ouvrir une playlist le ferme aussi.
#[test]
fn the_playlist_sort_menu_closes_on_choice_and_on_open() {
  let mut app = app();
  ui(&mut app, UiMessage::PlaylistSortOpen(true));
  assert!(app.ui.playlist_sort_open);
  ui(&mut app, UiMessage::SetPlaylistSort(SortMode::AddedDesc));
  assert!(!app.ui.playlist_sort_open);
  assert_eq!(app.data.playlist_sort, SortMode::AddedDesc);
  let id = "pl-test".to_string();
  app.data.playlists.push(crate::user_data::Playlist {
    id: Some(id.clone()),
    ..Default::default()
  });
  ui(&mut app, UiMessage::PlaylistSortOpen(true));
  ui(&mut app, UiMessage::OpenPlaylist(id));
  assert!(!app.ui.playlist_sort_open);
}

fn sleep_at_end(app: &mut App) {
  app.update(Message::AudioPrefs(AudioMessage::Sleep(
    SleepChoice::EndOfTrack,
  )));
}

#[test]
fn the_end_of_track_timer_pauses_instead_of_playing_the_next_track() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  sleep_at_end(&mut app);
  audio(&mut app, AudioEvent::Position(Duration::from_secs(199)));
  let effects = audio(&mut app, AudioEvent::Ended);
  assert_eq!(current_id(&app), "a");
  assert!(resolved(&effects).is_none());
  assert!(effects.contains(&Effect::AudioPause));
  assert!(!app.playback.is_playing);
  assert_eq!(app.ui.audio.sleep, SleepTimer::Off);
}

#[test]
fn the_end_of_track_timer_also_pauses_in_repeat_mode() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  player(&mut app, PlayerMessage::ToggleRepeat);
  sleep_at_end(&mut app);
  let effects = audio(&mut app, AudioEvent::Ended);
  assert!(resolved(&effects).is_none());
  assert!(effects.contains(&Effect::AudioPause));
  assert_eq!(app.ui.audio.sleep, SleepTimer::Off);
}

#[test]
fn the_end_of_track_timer_pauses_at_the_end_of_a_trim() {
  for repeat in [false, true] {
    let mut app = app();
    play_first(&mut app);
    loaded(&mut app);
    if repeat {
      player(&mut app, PlayerMessage::ToggleRepeat);
    }
    let id = app
      .player
      .current
      .as_ref()
      .unwrap()
      .canonical_id(&app.data.platform);
    app
      .data
      .track_trims
      .insert(id, json!({ "start": 5.0, "end": 100.0 }));
    sleep_at_end(&mut app);
    let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(100)));
    assert!(effects.contains(&Effect::AudioPause), "repeat={repeat}");
    assert!(resolved(&effects).is_none());
    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::AudioSeek(_)))
    );
    assert_eq!(current_id(&app), "a");
    assert_eq!(app.ui.audio.sleep, SleepTimer::Off);
  }
}

fn trim_current(app: &mut App, start: f64, end: f64) {
  let id = app
    .player
    .current
    .as_ref()
    .unwrap()
    .canonical_id(&app.data.platform);
  app
    .data
    .track_trims
    .insert(id, json!({ "start": start, "end": end }));
}

#[test]
fn the_end_of_a_trim_loops_back_to_its_start_in_repeat_mode() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  player(&mut app, PlayerMessage::ToggleRepeat);
  trim_current(&mut app, 5.0, 100.0);
  let before = audio(&mut app, AudioEvent::Position(Duration::from_secs(99)));
  assert!(!before.contains(&Effect::AudioSeek(Duration::from_secs(5))));
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(100)));
  assert_eq!(
    effects,
    vec![
      Effect::AudioSeek(Duration::from_secs(5)),
      Effect::SaveResumePosition(ResumePosition::new(
        "ytmusic:a".into(),
        Duration::from_secs(5)
      )),
    ]
  );
  assert_eq!(current_id(&app), "a");
}

#[test]
fn the_end_of_a_trim_plays_the_next_track_without_repeat() {
  let mut app = app();
  play_first(&mut app);
  loaded(&mut app);
  trim_current(&mut app, 5.0, 100.0);
  let before = audio(&mut app, AudioEvent::Position(Duration::from_secs(99)));
  assert!(resolved(&before).is_none());
  assert_eq!(current_id(&app), "a");
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(100)));
  assert_eq!(current_id(&app), "b");
  assert!(resolved(&effects).is_some());
  assert!(
    !effects
      .iter()
      .any(|effect| matches!(effect, Effect::AudioSeek(_)))
  );
}

#[test]
fn the_end_of_track_timer_blocks_the_crossfade_until_it_is_turned_off() {
  let mut app = app();
  app.player.crossfade = 6;
  play_first(&mut app);
  loaded(&mut app);
  sleep_at_end(&mut app);
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(194)));
  assert_eq!(current_id(&app), "a");
  assert!(resolved(&effects).is_none());
  app.update(Message::AudioPrefs(AudioMessage::Sleep(SleepChoice::Off)));
  let effects = audio(&mut app, AudioEvent::Position(Duration::from_secs(195)));
  assert_eq!(
    current_id(&app),
    "b",
    "le fondu reste possible une fois la minuterie coupée"
  );
  assert_eq!(resolved(&effects).unwrap().1, Some(6));
}

#[test]
fn the_art_command_opens_the_art_window_for_its_track() {
  let mut app = app();
  let effects = ui(
    &mut app,
    UiMessage::RowCommand(ListId::Favorites, 1, TrackCommand::Art),
  );
  assert_eq!(effects, [Effect::FocusOverlay]);
  assert_eq!(app.ui.modal, Some(Modal::TrackArt(Box::new(track("b")))));
}

fn numbered(count: usize) -> Vec<Track> {
  (0..count)
    .map(|index| track(&format!("t{index}")))
    .collect()
}

fn with_queue(count: usize, playing: usize) -> App {
  let mut app = app_with(UserData {
    favorites: numbered(count),
    autoplay: false,
    ..UserData::default()
  });
  ui(
    &mut app,
    UiMessage::RowActivated(ListId::Favorites, playing),
  );
  app
}

#[test]
fn the_queue_panel_is_closed_at_launch_and_never_written() {
  let app = app();
  assert!(!app.ui.queue_panel.open);
  assert_eq!(app.ui.queue_panel.tab, QueueTab::Queue);
  let saved = serde_json::to_string(&app.settings).unwrap().to_lowercase();
  assert!(!saved.contains("queue_panel") && !saved.contains("queuepanel"));
  let data = serde_json::to_string(&app.data).unwrap().to_lowercase();
  assert!(!data.contains("queue_panel") && !data.contains("queuepanel"));
}

#[test]
fn the_queue_button_opens_and_closes_the_panel_without_any_effect() {
  let mut app = app();
  assert!(ui(&mut app, UiMessage::ToggleQueuePanel).is_empty());
  assert!(app.ui.queue_panel.open);
  assert!(ui(&mut app, UiMessage::ToggleQueuePanel).is_empty());
  assert!(!app.ui.queue_panel.open);
}

#[test]
fn the_queue_panel_has_no_timer_and_keeps_the_animation_off() {
  let mut app = with_queue(5, 0);
  let before = (app.animation_needed(), app.main_ticker());
  ui(&mut app, UiMessage::ToggleQueuePanel);
  assert_eq!((app.animation_needed(), app.main_ticker()), before);
}

#[test]
fn the_queue_panel_animates_open_and_closed_then_stops_the_frames() {
  let mut app = app();
  app.visuals.warmup_frames = 0;
  app.view_motion.enabled = true;
  let t0 = app.now;
  assert!(!app.animation_needed());
  ui(&mut app, UiMessage::ToggleQueuePanel);
  assert!(app.animation_needed(), "ouverture en cours");
  assert!(crate::screens::chrome::queue_panel(&app).is_some());
  app.now = t0 + std::time::Duration::from_millis(259);
  assert!(app.animation_needed());
  app.now = t0 + std::time::Duration::from_millis(260);
  assert!(!app.animation_needed(), "posé : l'abonnement s'arrête");
  ui(&mut app, UiMessage::ToggleQueuePanel);
  assert!(app.animation_needed(), "fermeture en cours");
  assert!(
    crate::screens::chrome::queue_panel(&app).is_some(),
    "visible jusqu'à la fin de la transition"
  );
  app.now = t0 + std::time::Duration::from_millis(520);
  assert!(!app.animation_needed());
  assert!(crate::screens::chrome::queue_panel(&app).is_none());
}

fn audio_msg(app: &mut App, message: AudioMessage) -> Vec<Effect> {
  app.update(Message::AudioPrefs(message))
}

#[test]
fn the_audio_card_unveils_then_stops_the_frames() {
  let mut app = app();
  app.visuals.warmup_frames = 0;
  app.overlay_motion = crate::motion::OverlayMotion::launched();
  assert!(!app.animation_needed());
  audio_msg(&mut app, AudioMessage::Open);
  let t0 = app.now;
  assert!(app.animation_needed(), "ouverture en cours");
  assert!(app.ui.audio.motion.card_pose(t0).is_some());
  app.now = t0 + Duration::from_millis(319);
  assert!(app.animation_needed());
  app.now = t0 + Duration::from_millis(320);
  assert!(app.ui.audio.motion.card_pose(app.now).is_none());
  assert!(!app.animation_needed(), "posée : l'abonnement s'arrête");
}

#[test]
fn the_audio_card_stays_drawn_for_200_ms_after_closing() {
  let mut app = app();
  app.visuals.warmup_frames = 0;
  app.overlay_motion = crate::motion::OverlayMotion::launched();
  audio_msg(&mut app, AudioMessage::Open);
  app.now += Duration::from_secs(1);
  assert!(crate::screens::audio_window::closing(&app).is_none());
  ui(&mut app, UiMessage::ModalClose);
  assert!(app.ui.modal.is_none());
  let t0 = app.now;
  assert!(app.animation_needed());
  assert!(
    crate::screens::audio_window::closing(&app).is_some(),
    "visible jusqu'à la fin du repli"
  );
  app.now = t0 + Duration::from_millis(199);
  assert!(crate::screens::audio_window::closing(&app).is_some());
  app.now = t0 + Duration::from_millis(200);
  assert!(crate::screens::audio_window::closing(&app).is_none());
  assert!(!app.animation_needed());
}

#[test]
fn the_audio_card_has_no_motion_outside_the_launched_app() {
  let mut app = app();
  app.visuals.warmup_frames = 0;
  audio_msg(&mut app, AudioMessage::Open);
  ui(&mut app, UiMessage::ModalClose);
  assert!(crate::screens::audio_window::closing(&app).is_none());
  assert!(!app.animation_needed());
}

#[test]
fn only_a_new_audio_tab_replays_the_panel_animation() {
  use crate::audio_prefs::AudioTab;
  let mut app = app();
  app.visuals.warmup_frames = 0;
  app.overlay_motion = crate::motion::OverlayMotion::launched();
  app.ui.modal = Some(crate::ui::Modal::Audio);
  app.ui.audio.tab = AudioTab::Tempo;
  audio_msg(&mut app, AudioMessage::Tab(AudioTab::Tempo));
  assert!(
    !app.animation_needed(),
    "même onglet : aucune classe ne change"
  );
  audio_msg(&mut app, AudioMessage::Tab(AudioTab::Effects));
  assert!(app.animation_needed());
  assert!(app.ui.audio.motion.panel_pose(app.now).is_some());
  app.now += Duration::from_millis(220);
  assert!(!app.animation_needed());
}

#[test]
fn the_sleep_dot_starts_with_the_timer_and_keeps_the_frames_while_shown() {
  use crate::audio_prefs::AudioTab;
  let mut app = app();
  app.visuals.warmup_frames = 0;
  app.overlay_motion = crate::motion::OverlayMotion::launched();
  app.ui.modal = Some(crate::ui::Modal::Audio);
  app.ui.audio.tab = AudioTab::Sleep;
  assert!(!app.animation_needed());
  audio_msg(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(15)));
  let started = app.now;
  app.now = started + Duration::from_secs(30);
  assert!(
    app.animation_needed(),
    "le point pulse tant qu'il est affiché"
  );
  // Un autre choix de minuterie ne redémarre pas `sleepDot` (la classe `.active` reste posée).
  audio_msg(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(30)));
  assert_eq!(app.ui.audio.motion.dot_pose(app.now), {
    let mut again = app.ui.audio.motion;
    again.start_dot(started);
    again.dot_pose(app.now)
  });
  app.ui.audio.tab = AudioTab::Tempo;
  assert!(!app.animation_needed(), "onglet masqué : plus de point");
  audio_msg(&mut app, AudioMessage::Sleep(SleepChoice::Off));
  assert!(!app.animation_needed());
}

#[test]
fn a_new_app_has_no_splash_and_needs_no_frames_for_it() {
  let mut app = app();
  assert!(app.splash.is_none());
  app.visuals.warmup_frames = 0;
  assert!(!app.animation_needed());
}

#[test]
fn the_splash_starts_at_the_first_frame_and_is_removed_after_1340_ms() {
  let mut app = app();
  app.visuals.warmup_frames = 0;
  app.splash = Some(crate::splash::Splash::default());
  assert!(app.animation_needed() && app.main_ticker());
  let t0 = std::time::Instant::now();
  app.splash_frame(t0);
  assert_eq!(app.splash.map(|splash| splash.start), Some(Some(t0)));
  app.splash_frame(t0 + Duration::from_millis(900));
  assert_eq!(app.splash.map(|splash| splash.start), Some(Some(t0)));
  app.splash_frame(t0 + Duration::from_millis(1339));
  assert!(app.splash.is_some());
  app.splash_frame(t0 + Duration::from_millis(1340));
  assert!(app.splash.is_none());
  assert!(!app.animation_needed() && !app.main_ticker());
}

#[test]
fn the_wave_asks_for_frames_while_playing_and_the_button_transition_ends_at_rest() {
  let mut app = app();
  app.visuals.warmup_frames = 0;
  app.player.current = Some(track("a"));
  app.playback.is_playing = true;
  // Le premier état observé n'est pas animé ; l'onde avance avec l'horloge pendant la lecture.
  app.update(Message::NoOp);
  assert!(!app.playbar_motion.button.animating(app.now));
  assert!(app.animation_needed());
  // Pause : le bouton se transforme 400 ms, l'onde s'arrête, puis plus aucune image n'est demandée.
  app.playback.is_playing = false;
  app.update(Message::NoOp);
  let click = app.now;
  assert!(
    app
      .playbar_motion
      .button
      .animating(click + Duration::from_millis(399))
  );
  assert!(app.animation_needed());
  app.now = click + Duration::from_millis(401);
  assert!(!app.playbar_motion.button.animating(app.now));
  assert!(!app.animation_needed());
  // Les deux pauses d'affilée ne relancent rien ; la reprise relance le bouton et l'onde.
  app.update(Message::NoOp);
  assert!(!app.animation_needed());
  app.playback.is_playing = true;
  app.update(Message::NoOp);
  assert!(app.animation_needed());
}

#[test]
fn the_wave_phase_freezes_in_pause_and_resumes_from_the_same_phase() {
  let mut app = app();
  app.player.current = Some(track("a"));
  app.playback.is_playing = true;
  app.update(Message::NoOp);
  let start = std::time::Instant::now();
  app.now = start + Duration::from_millis(1000);
  let playing = app.wave_phase();
  assert!(playing > 0.0 && playing < std::f32::consts::TAU);
  app.playback.is_playing = false;
  app.update(Message::NoOp);
  let frozen = app.wave_phase();
  app.now += Duration::from_secs(30);
  assert_eq!(app.wave_phase(), frozen);
}

#[test]
fn the_tabs_switch_and_keep_their_choice_when_the_panel_is_reopened() {
  let mut app = app();
  ui(&mut app, UiMessage::ToggleQueuePanel);
  assert!(ui(&mut app, UiMessage::QueuePanelTab(QueueTab::Recent)).is_empty());
  assert_eq!(app.ui.queue_panel.tab, QueueTab::Recent);
  ui(&mut app, UiMessage::ToggleQueuePanel);
  ui(&mut app, UiMessage::ToggleQueuePanel);
  assert_eq!(
    app.ui.queue_panel.tab,
    QueueTab::Recent,
    "comme `tab` de Riff"
  );
}

#[test]
fn the_upcoming_titles_start_after_the_current_one_and_stop_at_fifty() {
  let app = with_queue(80, 10);
  let upcoming = app.queue_panel_upcoming();
  assert_eq!(upcoming.len(), 50);
  assert_eq!(upcoming[0].0, 11);
  assert_eq!(upcoming[49].0, 60);
  assert_eq!(upcoming[0].1.id.as_deref(), Some("t11"));
  let app = with_queue(80, 70);
  assert_eq!(app.queue_panel_upcoming().len(), 9);
  let app = with_queue(3, 2);
  assert!(app.queue_panel_upcoming().is_empty());
}

#[test]
fn the_recent_titles_stop_at_forty() {
  let mut app = app_with(UserData {
    history: numbered(100),
    ..UserData::default()
  });
  assert_eq!(app.queue_panel_recent().len(), 40);
  assert_eq!(app.queue_panel_recent()[39].id.as_deref(), Some("t39"));
  app.data.history.truncate(7);
  assert_eq!(app.queue_panel_recent().len(), 7);
}

#[test]
fn a_click_on_an_upcoming_title_plays_that_index_of_the_queue() {
  let mut app = with_queue(10, 2);
  let (index, _) = app.queue_panel_upcoming()[3];
  assert_eq!(index, 6);
  let effects = ui(&mut app, UiMessage::QueuePanelPlay(index));
  assert_eq!(current_id(&app), "t6");
  assert_eq!(app.player.index, Some(6));
  assert_eq!(app.player.queue.len(), 10, "la file reste la même");
  assert!(resolved(&effects).is_some());
}

#[test]
fn a_click_on_a_recent_title_plays_it_in_the_list_of_the_recent_titles() {
  let mut app = app_with(UserData {
    history: numbered(100),
    favorites: numbered(3),
    autoplay: false,
    ..UserData::default()
  });
  let effects = ui(&mut app, UiMessage::QueuePanelPlayRecent(5));
  assert_eq!(current_id(&app), "t5");
  assert_eq!(app.player.queue.len(), 40, "la file devient les 40 récents");
  assert_eq!(app.player.index, Some(5));
  assert!(resolved(&effects).is_some());
  assert!(!app.player.radio && !app.player.my_wave);
}

#[test]
fn a_click_outside_the_lists_does_nothing() {
  let mut app = app();
  assert!(ui(&mut app, UiMessage::QueuePanelPlay(9)).is_empty());
  assert!(ui(&mut app, UiMessage::QueuePanelPlayRecent(9)).is_empty());
  assert!(app.player.current.is_none());
}

#[test]
fn the_queue_panel_stays_open_after_a_click_and_escape_does_not_close_it() {
  let mut app = with_queue(10, 0);
  ui(&mut app, UiMessage::ToggleQueuePanel);
  ui(&mut app, UiMessage::QueuePanelPlay(4));
  app.update(Message::Escape);
  assert!(
    app.ui.queue_panel.open,
    "Riff n'a ni Échap ni clic extérieur"
  );
}

#[test]
fn the_letter_shortcuts_still_work_with_the_queue_panel_open() {
  let mut app = app();
  ui(&mut app, UiMessage::ToggleQueuePanel);
  assert!(app.ui.modal.is_none() && app.dialog.is_none());
  let before = app.player.shuffle;
  app.update(Message::Shortcut(Box::new(Message::Player(
    PlayerMessage::ToggleShuffle,
  ))));
  assert_ne!(app.player.shuffle, before);
}

#[test]
fn the_empty_panel_uses_the_texts_of_riff() {
  let app = app();
  assert_eq!(
    crate::screens::chrome::queue_panel_empty(&app, QueueTab::Queue),
    "Nothing is playing"
  );
  assert_eq!(
    crate::screens::chrome::queue_panel_empty(&app, QueueTab::Recent),
    "Nothing played yet"
  );
  let playing = with_queue(3, 2);
  assert_eq!(
    crate::screens::chrome::queue_panel_empty(&playing, QueueTab::Queue),
    "Nothing queued next"
  );
}

// Encart d'import Spotify de l'accueil

#[test]
fn the_import_card_shows_without_a_playlist_when_not_dismissed() {
  let app = app();
  assert!(app.spotify_promo_visible());
  assert!(crate::screens::home::spotify_promo(&app).is_some());
}

#[test]
fn the_import_card_is_hidden_once_dismissed() {
  let mut app = app();
  app.settings.spotify_promo_dismissed = true;
  assert!(!app.spotify_promo_visible());
  assert!(crate::screens::home::spotify_promo(&app).is_none());
}

#[test]
fn the_import_card_is_hidden_when_a_playlist_exists() {
  let mut app = app();
  app.data.create_playlist("P", Vec::new(), 1, 1).unwrap();
  assert!(!app.spotify_promo_visible());
  assert!(crate::screens::home::spotify_promo(&app).is_none());
}

#[test]
fn closing_the_import_card_is_saved() {
  let mut app = app();
  let effects = ui(&mut app, UiMessage::SpotifyPromoClose);
  assert!(app.settings.spotify_promo_dismissed);
  assert!(!app.spotify_promo_visible());
  assert!(effects.iter().any(
    |effect| matches!(effect, Effect::SaveSettings(settings) if settings.spotify_promo_dismissed)
  ));
  assert_eq!(app.screen, Screen::Home);
}

#[test]
fn the_import_button_opens_the_integrations_settings_and_dismisses_the_card() {
  let mut app = app();
  let effects = ui(&mut app, UiMessage::SpotifyPromoOpen);
  assert_eq!(app.screen, Screen::Settings);
  assert_eq!(app.settings.settings_cat, "integrations");
  assert!(app.settings.spotify_promo_dismissed);
  assert!(effects.iter().any(
    |effect| matches!(effect, Effect::SaveSettings(settings) if settings.spotify_promo_dismissed)
  ));
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/spotify-status",
      ..
    }
  )));
}

#[test]
fn the_results_page_scrolls_the_list_from_its_own_top() {
  let mut app = app();
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  let many: Vec<String> = (0..60).map(|n| format!("t{n}")).collect();
  let ids: Vec<&str> = many.iter().map(String::as_str).collect();
  ui(
    &mut app,
    UiMessage::Reply(ReplyTag::Search { token }, Ok(results(&ids))),
  );
  let top = crate::screens::search::list_top(&app.ui.search);
  ui(&mut app, UiMessage::SearchScrolled(top - 10.0, 600.0));
  assert_eq!(
    app.ui.list(ListId::Search).offset,
    0.0,
    "au-dessus de la liste"
  );
  ui(&mut app, UiMessage::SearchScrolled(top + 500.0, 600.0));
  assert_eq!(app.ui.list(ListId::Search).offset, 500.0);
  assert_eq!(app.ui.list(ListId::Search).viewport_height, 600.0);
  assert!(!app.ui.search.loading_more, "loin de la fin de la page");
  let end = top + 60.0 * crate::ui::ROW_HEIGHT - 600.0;
  let effects = ui(&mut app, UiMessage::SearchScrolled(end - 100.0, 600.0));
  assert!(app.ui.search.loading_more);
  assert!(effects.iter().any(|effect| matches!(effect, Effect::CoreJson { query, .. } if query.contains(&("offset".to_string(), "60".to_string())))));
}

#[test]
fn a_row_is_hovered_until_the_pointer_leaves_it() {
  let mut app = app();
  ui(&mut app, UiMessage::RowEntered(ListId::Favorites, 1));
  assert_eq!(app.ui.row_hover, Some((ListId::Favorites, 1)));
  ui(&mut app, UiMessage::RowEntered(ListId::Favorites, 2));
  ui(&mut app, UiMessage::RowLeft(ListId::Favorites, 1));
  assert_eq!(
    app.ui.row_hover,
    Some((ListId::Favorites, 2)),
    "la sortie d'une ligne déjà quittée ne retire pas la suivante"
  );
  ui(&mut app, UiMessage::RowLeft(ListId::Favorites, 2));
  assert_eq!(app.ui.row_hover, None);
}

#[test]
fn row_hover_fades_the_background_over_140_ms_in_both_directions_then_the_timer_stops() {
  let mut app = app();
  let key = (ListId::Favorites, 1);
  assert!(!app.animation_needed());
  ui(&mut app, UiMessage::RowEntered(key.0, key.1));
  let start = app.now;
  let at = |ms| start + Duration::from_millis(ms);
  assert!(app.animation_needed());
  assert_eq!(app.ui.row_fade.value(key, start, 0.0), 0.0);
  // `ease` : à mi-durée le fond est déjà à plus de la moitié (cubic-bezier(0.25, 0.1, 0.25, 1) vaut 0,8024 à 0,5).
  let half = app.ui.row_fade.value(key, at(70), 0.0);
  assert!((half - 0.8024).abs() < 0.01, "{half}");
  assert_eq!(app.ui.row_fade.value(key, at(140), 0.0), 1.0);
  app.now = at(141);
  assert!(!app.animation_needed(), "posée : l'abonnement s'arrête");
  // Sortie : repart de la valeur courante, même durée.
  ui(&mut app, UiMessage::RowLeft(key.0, key.1));
  let leave = app.now;
  assert!(app.animation_needed());
  assert!(
    app
      .ui
      .row_fade
      .value(key, leave + Duration::from_millis(70), 1.0)
      < 0.3
  );
  assert_eq!(
    app
      .ui
      .row_fade
      .value(key, leave + Duration::from_millis(140), 1.0),
    0.0
  );
  app.now = leave + Duration::from_millis(141);
  assert!(!app.animation_needed());
}

#[test]
fn the_current_row_bars_run_while_playing_and_stop_the_frames_when_paused() {
  let mut app = app();
  app.bars_motion.enabled = true;
  app.update(Message::Navigate(Screen::Liked));
  app.player.current = Some(track("a"));
  app.playback.is_playing = true;
  app.update(Message::NoOp);
  let start = app.now;
  assert!(app.bars_animating(), "ligne `a` affichée en lecture");
  assert!(app.animation_needed());
  // `b3` n'a pas de délai : 20 % au départ, les autres barres attendent à 100 %.
  let heights = app.bar_heights();
  assert!(
    (heights[2] - 0.2).abs() < 0.01 && [heights[0], heights[1], heights[3]] == [1.0; 3],
    "{heights:?}"
  );
  app.playback.is_playing = false;
  app.update(Message::NoOp);
  assert!(!app.bars_animating());
  assert_eq!(
    app.bar_heights(),
    [crate::theme::tokens::track_row::BAR_REST; 4]
  );
  // Le bouton lecture/pause finit sa transition : plus aucune image demandée par les barres.
  app.now += Duration::from_secs(1);
  assert!(!app.animation_needed(), "pause : plus d'image demandée");
  assert!(app.now >= start);
  // Le titre n'est plus dans la liste affichée : pas d'image pour des barres invisibles.
  app.playback.is_playing = true;
  app.player.current = Some(track("zz"));
  app.update(Message::NoOp);
  assert!(!app.bars_animating());
  app.update(Message::Navigate(Screen::Home));
  app.player.current = Some(track("a"));
  app.update(Message::NoOp);
  assert!(!app.bars_animating(), "l'accueil n'a pas de ligne de titre");
}

#[test]
fn the_bars_do_not_date_anything_outside_the_launched_application() {
  let mut app = app();
  app.update(Message::Navigate(Screen::Liked));
  app.player.current = Some(track("a"));
  app.playback.is_playing = true;
  app.update(Message::NoOp);
  assert_eq!(app.bars_motion, crate::motion::BarsMotion::default());
}

#[test]
fn recreated_rows_restart_the_bars_from_zero() {
  let mut app = app();
  app.entry_motion.enabled = true;
  app.bars_motion.enabled = true;
  app.player.current = Some(track("a"));
  app.playback.is_playing = true;
  app.update(Message::Navigate(Screen::Liked));
  app.now += Duration::from_millis(900);
  assert!(app.bars_motion.elapsed_ms(app.now) > 800.0);
  app.update(Message::Navigate(Screen::History));
  assert!(app.bars_motion.elapsed_ms(app.now) < 1.0, "liste recréée");
}

#[test]
fn an_album_card_is_lifted_while_hovered_and_the_row_scroll_is_kept() {
  let mut app = app();
  ui(&mut app, UiMessage::SearchAlbumHover(3, true));
  ui(&mut app, UiMessage::SearchAlbumHover(4, true));
  ui(&mut app, UiMessage::SearchAlbumHover(3, false));
  assert_eq!(app.ui.search.album_hover, Some(4));
  ui(&mut app, UiMessage::SearchAlbumsScrolled(120.0));
  assert_eq!(app.ui.search.albums_scroll, 120.0);
  searching(&mut app, "autre");
  assert_eq!(
    app.ui.search.albums_scroll, 0.0,
    "nouvelle recherche, rangée au début"
  );
  assert_eq!(app.ui.search.album_hover, None);
}

#[test]
fn the_error_report_has_the_query_the_platform_and_the_diagnostics() {
  let mut app = app();
  searching(&mut app, "daft punk");
  app.ui.search.error = Some("panne".to_string());
  let effects = ui(&mut app, UiMessage::CopySearchError);
  let Some(Effect::CopyText(report)) = effects.first() else {
    panic!("rapport attendu : {effects:?}");
  };
  assert!(report.starts_with("Search Error: panne\nQuery: daft punk\nPlatform: ytmusic\n\n"));
  assert!(
    matches!(effects.get(1), Some(Effect::Toast(text, _)) if text == "Copied error & diagnostics to clipboard")
  );
}

#[test]
fn a_successful_search_puts_the_first_cover_in_the_search_field() {
  let mut app = app();
  searching(&mut app, "daft");
  let token = app.ui.search.token;
  assert_eq!(app.ui.search.icon_cover, None);
  let mut first = tracks(&["a"]);
  first[0].thumbnail = Some("https://c.invalid/a.jpg".to_string());
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::Search { token },
      Ok(json!({ "success": true, "tracks": first })),
    ),
  );
  assert_eq!(
    app.ui.search.icon_cover.as_deref(),
    Some("https://c.invalid/a.jpg")
  );
  searching(&mut app, "encore");
  assert_eq!(
    app.ui.search.icon_cover.as_deref(),
    Some("https://c.invalid/a.jpg"),
    "la vignette reste pendant la recherche suivante"
  );
  let token = app.ui.search.token;
  ui(
    &mut app,
    UiMessage::Reply(
      ReplyTag::Search { token },
      Ok(json!({ "success": true, "tracks": [] })),
    ),
  );
  assert_eq!(
    app.ui.search.icon_cover, None,
    "aucun résultat : retour à la loupe"
  );
}

fn playing_app() -> App {
  let mut app = app();
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 0));
  assert_eq!(current_id(&app), "a");
  app
}

#[test]
fn disliking_the_current_track_blocks_it_and_skips_to_the_next() {
  let mut app = playing_app();
  let effects = ui(
    &mut app,
    UiMessage::CurrentCommand(TrackCommand::BlockTrack),
  );
  assert!(app.data.is_track_blocked(&track("a")));
  assert_eq!(current_id(&app), "b");
  assert!(resolved(&effects).is_some());
  let toasts = effects
    .iter()
    .filter(|effect| matches!(effect, Effect::Toast(..)))
    .count();
  assert_eq!(toasts, 1);
}

#[test]
fn blocking_from_the_context_menu_of_the_current_track_skips_too() {
  let mut app = playing_app();
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("a")), TrackCommand::BlockTrack),
  );
  assert_eq!(current_id(&app), "b");
}

#[test]
fn unblocking_or_blocking_another_track_does_not_skip() {
  let mut app = playing_app();
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("c")), TrackCommand::BlockTrack),
  );
  assert_eq!(current_id(&app), "a", "autre titre");
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("c")), TrackCommand::BlockTrack),
  );
  assert!(!app.data.is_track_blocked(&track("c")));
  assert_eq!(current_id(&app), "a", "déblocage");
}

#[test]
fn unblocking_the_current_track_does_not_skip() {
  let mut app = playing_app();
  app.data.toggle_block_track(&track("a"));
  ui(
    &mut app,
    UiMessage::CurrentCommand(TrackCommand::BlockTrack),
  );
  assert!(!app.data.is_track_blocked(&track("a")));
  assert_eq!(current_id(&app), "a");
}

#[test]
fn disliking_a_track_already_covered_by_its_blocked_artist_still_skips() {
  let mut app = playing_app();
  app.data.blocked_artists.push("Artiste a".to_string());
  assert!(!app.data.is_track_blocked(&track("a")));
  ui(
    &mut app,
    UiMessage::CurrentCommand(TrackCommand::BlockTrack),
  );
  assert!(app.data.is_track_blocked(&track("a")));
  assert_eq!(current_id(&app), "b");
}

#[test]
fn blocking_the_artist_of_the_current_track_skips_it_and_drops_its_likes() {
  let mut app = playing_app();
  app.data.toggle_follow("Artiste a", None, None);
  let effects = ui(
    &mut app,
    UiMessage::CurrentCommand(TrackCommand::BlockArtist),
  );
  assert_eq!(current_id(&app), "b");
  assert!(resolved(&effects).is_some());
  assert_eq!(ids(&app.data.favorites), ["b", "c"]);
  assert!(!app.data.is_followed("Artiste a"));
}

#[test]
fn blocking_the_artist_from_its_page_skips_the_current_track_too() {
  let mut app = playing_app();
  app.ui.artist.name = "Artiste a".to_string();
  ui(&mut app, UiMessage::ArtistBlock);
  assert_eq!(current_id(&app), "b");
}

#[test]
fn unblocking_an_artist_does_not_skip() {
  let mut app = playing_app();
  app.data.toggle_block_artist("Artiste a");
  app.ui.artist.name = "Artiste a".to_string();
  ui(&mut app, UiMessage::ArtistBlock);
  assert!(!app.data.is_artist_blocked("Artiste a"));
  assert_eq!(current_id(&app), "a");
}

#[test]
fn disliking_the_only_track_without_autoplay_pauses() {
  let data = UserData {
    favorites: tracks(&["a"]),
    autoplay: false,
    ..UserData::default()
  };
  let mut app = app_with(data);
  ui(&mut app, UiMessage::RowActivated(ListId::Favorites, 0));
  app.playback.is_playing = true;
  let effects = ui(
    &mut app,
    UiMessage::CurrentCommand(TrackCommand::BlockTrack),
  );
  assert!(effects.contains(&Effect::AudioPause));
  assert!(!app.playback.is_playing);
}

#[test]
fn blocking_removes_every_favorite_the_entry_matches() {
  let mut data = UserData {
    favorites: tracks(&["a"]),
    ..UserData::default()
  };
  let mut twin = track("a2");
  twin.title = track("a").title;
  twin.artist = track("a").artist;
  data.favorites.push(twin.clone());
  data.toggle_block_track(&track("a"));
  assert!(data.favorites.is_empty());
  data.toggle_favorite(&twin, 0);
  assert!(!data.is_track_blocked(&track("a")));
}

#[test]
fn blocking_removes_the_track_from_the_favorites_and_liking_unblocks_it() {
  let mut app = playing_app();
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("c")), TrackCommand::BlockTrack),
  );
  assert!(!app.data.is_favorite(&track("c")));
  assert!(app.data.is_track_blocked(&track("c")));
  ui(
    &mut app,
    UiMessage::Track(Box::new(track("c")), TrackCommand::ToggleFavorite),
  );
  assert!(app.data.is_favorite(&track("c")));
  assert!(!app.data.is_track_blocked(&track("c")));
}
