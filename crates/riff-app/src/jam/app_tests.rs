//! Tests du jam au niveau `App` : aucune fenêtre, aucun réseau, `App` piloté par `Message::Jam`.

use super::protocol::{
  Current, Entry, JamSound, Notice, NoticeKind, Op, Participant, PlaybackState, QueueState,
  Refusal, SharedSound, WireTrack,
};
use super::{EndCause, JamEvent, JamMessage, JamOut};
use crate::app::{App, Effect, Message};
use crate::audio_prefs::{AudioMessage, Sound, SoundField};
use crate::i18n::I18n;
use crate::manage::ManageMessage;
use crate::settings::NativeSettings;
use crate::stream::{ResolveOptions, stream_info_query};
use crate::ui::{ListId, PlayerMessage, TrackCommand, UiMessage};
use crate::user_data::{Track, UserData};
use crate::window::PersistedWindowState;
use riff_audio::Event as AudioEvent;
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

fn local(id: &str) -> Track {
  Track {
    platform: Some("local".to_string()),
    path: Some(format!("C:/musique/{id}.mp3")),
    ..track(id)
  }
}

fn wire(id: &str, platform: &str) -> WireTrack {
  WireTrack {
    platform: platform.to_string(),
    id: Some(id.to_string()),
    title: Some(format!("Titre {id}")),
    artist: Some(format!("Artiste {id}")),
    duration: Some(200.0),
    url: Some(format!("https://exemple.org/{id}")),
    ..WireTrack::default()
  }
}

fn base_app(favorites: Vec<Track>) -> App {
  App::new(
    PersistedWindowState::default(),
    NativeSettings::default(),
    I18n::default(),
  )
  .with_data(UserData {
    favorites,
    autoplay: false,
    ..UserData::default()
  })
}

fn jam(app: &mut App, message: JamMessage) -> Vec<Effect> {
  app.update(Message::Jam(message))
}

fn event(app: &mut App, event: JamEvent) -> Vec<Effect> {
  jam(app, JamMessage::Event(1, Box::new(event)))
}

fn player(app: &mut App, message: PlayerMessage) -> Vec<Effect> {
  app.update(Message::Player(message))
}

fn audio(app: &mut App, event: AudioEvent) -> Vec<Effect> {
  app.update(Message::Audio(event))
}

fn loaded(app: &mut App) -> Vec<Effect> {
  audio(
    app,
    AudioEvent::Loaded {
      duration: Some(Duration::from_secs(200)),
      sample_rate: 48_000,
    },
  )
}

fn current_id(app: &App) -> &str {
  app
    .player
    .current
    .as_ref()
    .and_then(|track| track.id.as_deref())
    .unwrap_or("")
}

fn queue_ids(app: &App) -> Vec<&str> {
  app
    .player
    .queue
    .iter()
    .map(|track| track.id.as_deref().unwrap_or(""))
    .collect()
}

fn participants() -> Vec<Participant> {
  vec![
    Participant {
      id: 0,
      name: "User".to_string(),
      host: true,
    },
    Participant {
      id: 1,
      name: "Ana".to_string(),
      host: false,
    },
  ]
}

/// Hôte en lecture : file a, b, c, titre a chargé, jam démarré et prêt, une invitée.
fn host() -> (App, Vec<Effect>) {
  let mut app = base_app(vec![track("a"), track("b"), track("c")]);
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  loaded(&mut app);
  let started = jam(&mut app, JamMessage::Start);
  assert!(
    started
      .iter()
      .any(|effect| matches!(effect, Effect::JamHost { .. }))
  );
  event(&mut app, JamEvent::Participants(participants()));
  let effects = event(
    &mut app,
    JamEvent::Hosting {
      ticket: "riffjam-test".to_string(),
    },
  );
  (app, effects)
}

fn sent(effects: &[Effect]) -> Vec<&JamOut> {
  effects
    .iter()
    .filter_map(|effect| match effect {
      Effect::JamSend(out) => Some(out),
      _ => None,
    })
    .collect()
}

fn playbacks(effects: &[Effect]) -> Vec<&PlaybackState> {
  sent(effects)
    .into_iter()
    .filter_map(|out| match out {
      JamOut::Playback { state, .. } => Some(&**state),
      _ => None,
    })
    .collect()
}

fn queues(effects: &[Effect]) -> Vec<&QueueState> {
  sent(effects)
    .into_iter()
    .filter_map(|out| match out {
      JamOut::Queue(queue) => Some(queue),
      _ => None,
    })
    .collect()
}

fn notices(effects: &[Effect]) -> Vec<&Notice> {
  sent(effects)
    .into_iter()
    .filter_map(|out| match out {
      JamOut::Notice(notice) => Some(notice),
      _ => None,
    })
    .collect()
}

fn has_toast(effects: &[Effect], text: &str) -> bool {
  effects
    .iter()
    .any(|effect| matches!(effect, Effect::Toast(toast, _) if toast.contains(text)))
}

fn guest_op(app: &mut App, op: Op) -> Vec<Effect> {
  event(app, JamEvent::GuestOp { from: 1, id: 7, op })
}

fn uid_of(app: &App, id: &str) -> u64 {
  let index = queue_ids(app)
    .iter()
    .position(|queued| *queued == id)
    .unwrap();
  app.jam.as_ref().unwrap().entries[index].uid
}

// ---- Hôte ----

#[test]
fn hosting_broadcasts_the_queue_and_the_playback_for_the_welcome_cache() {
  let (app, effects) = host();
  let queue = queues(&effects);
  assert_eq!(queue.len(), 1);
  assert_eq!(queue[0].entries.len(), 3);
  let playback = playbacks(&effects);
  assert_eq!(playback.len(), 1);
  assert!(playback[0].playing);
  assert_eq!(
    playback[0].current.as_ref().unwrap().uid,
    Some(uid_of(&app, "a"))
  );
  assert_eq!(
    app.jam.as_ref().unwrap().ticket.as_deref(),
    Some("riffjam-test")
  );
}

#[test]
fn starting_a_jam_drops_local_files_and_keeps_entries_parallel() {
  let mut app = base_app(vec![track("a"), local("l"), track("b")]);
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  let effects = jam(&mut app, JamMessage::Start);
  assert_eq!(queue_ids(&app), ["a", "b"]);
  assert_eq!(app.jam.as_ref().unwrap().entries.len(), 2);
  assert!(has_toast(&effects, "1 local files skipped"));
  assert!(!app.player.radio && !app.player.my_wave);
}

#[test]
fn adding_a_local_file_in_a_jam_toasts_and_sends_nothing() {
  let (mut app, _) = host();
  let effects = app.update(Message::Ui(UiMessage::Track(
    Box::new(local("l")),
    TrackCommand::AddToQueue,
  )));
  assert!(has_toast(&effects, "Local files can't be played in a Jam"));
  assert!(sent(&effects).is_empty());
  assert_eq!(app.player.queue.len(), 3);
}

#[test]
fn a_guest_add_extends_the_queue_and_notifies() {
  let (mut app, _) = host();
  let effects = guest_op(
    &mut app,
    Op::Add {
      tracks: vec![wire("x", "soundcloud")],
      next: false,
    },
  );
  assert_eq!(queue_ids(&app), ["a", "b", "c", "x"]);
  let added = &app.jam.as_ref().unwrap().entries[3];
  assert_eq!((added.by, added.by_name.as_str()), (1, "Ana"));
  assert_eq!(app.player.queue[3].platform.as_deref(), Some("soundcloud"));
  let queue = queues(&effects);
  assert_eq!(queue.len(), 1);
  assert_eq!(queue[0].entries.len(), 4);
  let notice = notices(&effects);
  assert_eq!(notice.len(), 1);
  assert_eq!(notice[0].kind, NoticeKind::Added);
  assert_eq!(notice[0].title.as_deref(), Some("Titre x"));
  assert!(has_toast(&effects, "Ana added Titre x"));
}

#[test]
fn a_guest_pause_pauses_the_host_and_broadcasts() {
  let (mut app, _) = host();
  let effects = guest_op(&mut app, Op::Pause);
  assert!(effects.contains(&Effect::AudioPause));
  let playback = playbacks(&effects);
  assert_eq!(playback.len(), 1);
  assert!(!playback[0].playing);
  assert_eq!(notices(&effects)[0].kind, NoticeKind::Paused);
  assert!(has_toast(&effects, "Ana paused"));
}

#[test]
fn a_second_next_from_the_old_entry_is_ignored() {
  let (mut app, _) = host();
  let from = uid_of(&app, "a");
  guest_op(&mut app, Op::Next { from: Some(from) });
  assert_eq!(current_id(&app), "b");
  let effects = guest_op(&mut app, Op::Next { from: Some(from) });
  assert_eq!(current_id(&app), "b");
  assert!(effects.is_empty());
}

#[test]
fn a_guest_seek_seeks_the_host_and_bumps_the_seek_counter() {
  let (mut app, _) = host();
  let before = app.jam.as_ref().unwrap().seek;
  let effects = guest_op(&mut app, Op::Seek { pos_ms: 90_000 });
  assert!(effects.contains(&Effect::AudioSeek(Duration::from_secs(90))));
  let playback = playbacks(&effects);
  assert_eq!(playback.len(), 1);
  assert_eq!(playback[0].seek, before + 1);
  assert!(playback[0].pos_ms >= 90_000);
}

#[test]
fn the_host_shuffle_stays_out_of_the_saved_data() {
  let (mut app, _) = host();
  let saved = app.data.shuffle;
  let effects = player(&mut app, PlayerMessage::ToggleShuffle);
  assert!(app.player.shuffle);
  assert_eq!(app.data.shuffle, saved);
  assert!(playbacks(&effects)[0].shuffle);
}

#[test]
fn the_end_of_the_queue_does_not_prepare_the_autoplay_in_a_jam() {
  let mut app = base_app(vec![track("a")]);
  app.data.autoplay = true;
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  loaded(&mut app);
  jam(&mut app, JamMessage::Start);
  event(&mut app, JamEvent::Participants(participants()));
  event(
    &mut app,
    JamEvent::Hosting {
      ticket: "riffjam-test".to_string(),
    },
  );
  let effects = player(&mut app, PlayerMessage::Next);
  assert!(
    !effects
      .iter()
      .any(|effect| matches!(effect, Effect::PrepareAutoplay { .. }))
  );
  assert!(!app.ui.autoplay_waiting);
}

#[test]
fn the_host_closing_the_window_stops_the_jam_first() {
  let (mut app, _) = host();
  let effects = app.update(Message::CloseStateKnown {
    is_maximized: false,
    is_full_screen: false,
  });
  let stop = effects
    .iter()
    .position(|effect| matches!(effect, Effect::JamSend(JamOut::Stop)))
    .expect("stop");
  let exit = effects
    .iter()
    .position(|effect| matches!(effect, Effect::SaveWindowAndExit(..)))
    .expect("exit");
  assert!(stop < exit);
}

#[test]
fn the_heartbeat_only_runs_for_a_playing_host_with_a_guest() {
  let (app, _) = host();
  assert!(app.jam_heartbeat_needed());
  let alone = base_app(Vec::new());
  assert!(!alone.jam_heartbeat_needed());
}

#[test]
fn the_radio_is_not_available_during_a_jam() {
  let (mut app, _) = host();
  let effects = player(&mut app, PlayerMessage::PlayMyWave);
  assert!(has_toast(&effects, "Not available during a Jam"));
}

// ---- Invité ----

/// Invité connecté : file a, b ; le jeu `entries` donne `(id, plateforme)` ; `current` est l'index joué.
fn guest(
  entries: &[(&str, &str)],
  current: usize,
  pos_ms: u64,
  playing: bool,
  local_queue: Vec<Track>,
) -> (App, Vec<Effect>, Instant) {
  guest_on(base_app(local_queue), entries, current, pos_ms, playing)
}

fn guest_on(
  mut app: App,
  entries: &[(&str, &str)],
  current: usize,
  pos_ms: u64,
  playing: bool,
) -> (App, Vec<Effect>, Instant) {
  jam(&mut app, JamMessage::CodeInput("riffjam-test".to_string()));
  let started = jam(&mut app, JamMessage::Join);
  assert!(
    started
      .iter()
      .any(|effect| matches!(effect, Effect::JamJoin { .. }))
  );
  let anchor = Instant::now();
  let effects = event(
    &mut app,
    JamEvent::Joined {
      you: 1,
      participants: participants(),
      queue: QueueState {
        qv: 1,
        entries: entries
          .iter()
          .enumerate()
          .map(|(index, (id, platform))| Entry {
            uid: index as u64 + 1,
            by: 0,
            by_name: "User".to_string(),
            track: wire(id, platform),
          })
          .collect(),
      },
      playback: playback_state(entries, current, 1, 1, pos_ms, playing),
      anchor,
    },
  );
  (app, effects, anchor)
}

fn playback_state(
  entries: &[(&str, &str)],
  current: usize,
  pv: u64,
  start: u64,
  pos_ms: u64,
  playing: bool,
) -> PlaybackState {
  PlaybackState {
    pv,
    current: Some(Current {
      uid: Some(current as u64 + 1),
      track: wire(entries[current].0, entries[current].1),
    }),
    start,
    seek: 0,
    playing,
    pos_ms,
    ..PlaybackState::default()
  }
}

fn resolved(effects: &[Effect]) -> Option<(&Track, &ResolveOptions, Duration)> {
  effects.iter().find_map(|effect| match effect {
    Effect::ResolveTrack {
      track,
      options,
      start,
      ..
    } => Some((&**track, options, *start)),
    _ => None,
  })
}

const AB: [(&str, &str); 2] = [("a", "ytmusic"), ("b", "ytmusic")];

#[test]
fn a_guest_toggle_play_only_sends_a_command() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  loaded(&mut app);
  let effects = player(&mut app, PlayerMessage::TogglePlay);
  assert_eq!(effects.len(), 1);
  assert!(matches!(
    &effects[0],
    Effect::JamSend(JamOut::Command { op: Op::Pause, .. })
  ));
}

#[test]
fn joining_mid_track_starts_at_the_expected_position_and_mirrors_the_queue() {
  let (app, effects, _) = guest(&AB, 0, 30_000, true, Vec::new());
  let (track, _, start) = resolved(&effects).expect("resolve");
  assert_eq!(track.id.as_deref(), Some("a"));
  assert!(start >= Duration::from_secs(30) && start < Duration::from_secs(31));
  assert_eq!(queue_ids(&app), ["a", "b"]);
  assert_eq!(app.player.index, Some(0));
  assert!(has_toast(&effects, "You joined User's Jam"));
}

#[test]
fn a_mixed_queue_resolves_each_entry_from_its_own_platform() {
  let entries = [("sc", "soundcloud"), ("yt", "ytmusic")];
  let mut ytmusic = base_app(Vec::new());
  ytmusic.data.platform = "ytmusic".to_string();
  let (_, effects, _) = guest_on(ytmusic, &entries, 0, 0, true);
  let (track, options, _) = resolved(&effects).expect("resolve");
  assert_eq!(track.platform.as_deref(), Some("soundcloud"));
  assert_eq!(options.platform, "ytmusic");
  assert_eq!(stream_info_query(track, options)["platform"], "soundcloud");

  let mut soundcloud = base_app(Vec::new());
  soundcloud.data.platform = "soundcloud".to_string();
  let (mut app, _, anchor) = guest_on(soundcloud, &entries, 0, 0, true);
  loaded(&mut app);
  let effects = event(
    &mut app,
    JamEvent::Playback {
      state: playback_state(&entries, 1, 2, 2, 0, true),
      anchor,
    },
  );
  let (track, options, _) = resolved(&effects).expect("resolve");
  assert_eq!(track.id.as_deref(), Some("yt"));
  assert_eq!(options.platform, "soundcloud");
  assert_eq!(stream_info_query(track, options)["platform"], "ytmusic");
}

#[test]
fn a_paused_host_pauses_the_guest_when_the_track_is_loaded() {
  let (mut app, _, anchor) = guest(&AB, 0, 0, true, Vec::new());
  event(
    &mut app,
    JamEvent::Playback {
      state: playback_state(&AB, 0, 2, 1, 0, false),
      anchor,
    },
  );
  let effects = loaded(&mut app);
  assert!(effects.contains(&Effect::AudioPause));
  assert!(!app.playback.is_playing);
}

#[test]
fn a_drift_of_three_seconds_seeks_once_per_cooldown() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  loaded(&mut app);
  let first = audio(&mut app, AudioEvent::Position(Duration::from_secs(3)));
  assert!(
    first
      .iter()
      .any(|effect| matches!(effect, Effect::AudioSeek(_)))
  );
  let second = audio(&mut app, AudioEvent::Position(Duration::from_secs(6)));
  assert!(
    !second
      .iter()
      .any(|effect| matches!(effect, Effect::AudioSeek(_)))
  );
}

#[test]
fn the_end_of_a_track_does_not_advance_a_guest() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  loaded(&mut app);
  app.playback.position = Duration::from_secs(199);
  let effects = audio(&mut app, AudioEvent::Ended);
  assert!(effects.is_empty());
  assert_eq!(current_id(&app), "a");
  assert!(!app.playback.is_playing);
}

#[test]
fn a_failed_resolution_reports_the_entry_to_the_host() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  let token = app.player.token;
  let effects = player(
    &mut app,
    PlayerMessage::Resolved {
      token,
      start: Duration::ZERO,
      crossfade: None,
      result: Err("boom".to_string()),
    },
  );
  assert!(has_toast(
    &effects,
    "Couldn't play this track, waiting for the next one"
  ));
  assert!(effects.contains(&Effect::JamSend(JamOut::Report { uid: 1 })));
}

#[test]
fn the_end_of_the_jam_gives_the_guest_back_the_previous_queue() {
  let before = vec![track("x"), track("y")];
  let mut app = base_app(before.clone());
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 1)));
  app.player.shuffle = true;
  let saved_token = app.player.token;
  jam(&mut app, JamMessage::CodeInput("riffjam-test".to_string()));
  jam(&mut app, JamMessage::Join);
  event(
    &mut app,
    JamEvent::Joined {
      you: 1,
      participants: participants(),
      queue: QueueState {
        qv: 1,
        entries: vec![Entry {
          uid: 1,
          by: 0,
          by_name: "User".to_string(),
          track: wire("a", "ytmusic"),
        }],
      },
      playback: playback_state(&AB[..1], 0, 1, 1, 0, true),
      anchor: Instant::now(),
    },
  );
  assert_eq!(queue_ids(&app), ["a"]);
  let jam_token = app.player.token;
  assert!(jam_token > saved_token);
  let effects = event(&mut app, JamEvent::Ended(EndCause::HostEnded));
  assert!(app.jam.is_none());
  assert_eq!(queue_ids(&app), ["x", "y"]);
  assert_eq!(app.player.index, Some(1));
  assert_eq!(current_id(&app), "y");
  assert!(app.player.shuffle);
  assert!(app.player.token > jam_token);
  assert!(effects.contains(&Effect::AudioPause));
  assert!(!app.playback.is_playing);
  assert!(has_toast(&effects, "The host ended the Jam"));
}

#[test]
fn the_host_left_alone_gets_an_info_toast_and_keeps_the_queue() {
  let (mut app, _) = host();
  let before = queue_ids(&app)
    .iter()
    .map(|id| id.to_string())
    .collect::<Vec<_>>();
  let effects = event(&mut app, JamEvent::Ended(EndCause::Alone));
  assert!(app.jam.is_none());
  assert_eq!(queue_ids(&app), before);
  assert_eq!(current_id(&app), "a");
  assert!(app.playback.is_playing);
  assert!(has_toast(
    &effects,
    "The Jam ended because everyone else left"
  ));
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::Toast(text, crate::widgets::toast::ToastKind::Info) if text.contains("everyone else left")
  )));
}

// ---- Effets partagés ----

fn audio_pref(app: &mut App, message: AudioMessage) -> Vec<Effect> {
  app.update(Message::AudioPrefs(message))
}

fn slowed() -> JamSound {
  JamSound {
    speed_pitch: 0.82,
    reverb: 65.0,
    echo: 15.0,
    ..JamSound::default()
  }
}

fn sound_op(sound: JamSound) -> Op {
  Op::Sound {
    sound,
    smooth: false,
    preset: None,
  }
}

/// Réglages envoyés au moteur par ces effets.
fn engine_sounds(effects: &[Effect]) -> Vec<riff_audio::AudioSettings> {
  effects
    .iter()
    .filter_map(|effect| match effect {
      Effect::AudioSound(settings) => Some(**settings),
      _ => None,
    })
    .collect()
}

/// Son que ces effets écrivent dans `native-settings.json`.
fn saved_sounds(effects: &[Effect]) -> Vec<Sound> {
  effects
    .iter()
    .filter_map(|effect| match effect {
      Effect::SaveSettings(settings) => Some(settings.sound.audio),
      _ => None,
    })
    .collect()
}

fn commands(effects: &[Effect]) -> Vec<(u64, &Op)> {
  sent(effects)
    .into_iter()
    .filter_map(|out| match out {
      JamOut::Command { id, op } => Some((*id, op)),
      _ => None,
    })
    .collect()
}

fn shared_of(sound: JamSound, sv: u64) -> SharedSound {
  SharedSound {
    sv,
    sound,
    ..SharedSound::default()
  }
}

/// Invité admis avec des effets, `age` après l'ancre de l'hôte.
fn guest_joined_with(
  mut app: App,
  shared: SharedSound,
  pos_ms: u64,
  age: Duration,
  playing: bool,
) -> (App, Vec<Effect>) {
  jam(&mut app, JamMessage::CodeInput("riffjam-test".to_string()));
  jam(&mut app, JamMessage::Join);
  let anchor = Instant::now().checked_sub(age).expect("instant");
  let mut playback = playback_state(&AB, 0, 1, 1, pos_ms, playing);
  playback.sound = shared;
  let effects = event(
    &mut app,
    JamEvent::Joined {
      you: 1,
      participants: participants(),
      queue: QueueState {
        qv: 1,
        entries: AB
          .iter()
          .enumerate()
          .map(|(index, (id, platform))| Entry {
            uid: index as u64 + 1,
            by: 0,
            by_name: "User".to_string(),
            track: wire(id, platform),
          })
          .collect(),
      },
      playback,
      anchor,
    },
  );
  (app, effects)
}

/// Même état de lecture qu'à l'admission, avec de nouveaux effets.
fn sound_state(shared: SharedSound, pv: u64) -> PlaybackState {
  let mut state = playback_state(&AB, 0, pv, 1, 0, true);
  state.sound = shared;
  state
}

/// Hôte dont le son est réglé avant le démarrage du jam.
fn host_with_sound(prepare: impl FnOnce(&mut App)) -> (App, Vec<Effect>) {
  let mut app = base_app(vec![track("a"), track("b"), track("c")]);
  prepare(&mut app);
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  loaded(&mut app);
  jam(&mut app, JamMessage::Start);
  event(&mut app, JamEvent::Participants(participants()));
  let effects = event(
    &mut app,
    JamEvent::Hosting {
      ticket: "riffjam-test".to_string(),
    },
  );
  (app, effects)
}

#[test]
fn the_effects_of_the_host_become_those_of_the_jam() {
  let (app, effects) = host_with_sound(|app| {
    app.ui.audio.live.speed_pitch = 0.82;
    app.ui.audio.live.reverb = 65.0;
    app.ui.audio.live.echo = 15.0;
    app.settings.sound.active_preset = "p_slowed".to_string();
  });
  let jam = app.jam.as_ref().unwrap();
  let shared = jam.shared.as_ref().expect("effets du jam");
  assert_eq!(shared.sound, slowed());
  assert_eq!(shared.sv, 0);
  assert_eq!(shared.preset.as_deref(), Some("p_slowed"));
  assert_eq!(
    jam.personal.expect("son personnel").speed_pitch.to_bits(),
    0.82_f32.to_bits()
  );
  // Le premier état diffusé (et donc le cache de `welcome`) porte les effets.
  assert_eq!(playbacks(&effects)[0].sound.sound, slowed());
}

#[test]
fn a_host_without_effects_shares_no_effect() {
  let (app, effects) = host();
  assert_eq!(
    app.jam.as_ref().unwrap().shared.as_ref().unwrap().sound,
    JamSound::default()
  );
  assert_eq!(playbacks(&effects)[0].sound.sound, JamSound::default());
}

#[test]
fn a_guest_sound_op_is_applied_by_the_host_and_broadcast_with_new_counters() {
  let (mut app, _) = host();
  let pv = app.jam.as_ref().unwrap().pv;
  let effects = guest_op(&mut app, sound_op(slowed()));
  let engine = engine_sounds(&effects);
  assert_eq!(engine.len(), 1);
  assert!((engine[0].speed_pitch - 0.82).abs() < f32::EPSILON);
  assert!((engine[0].reverb - 65.0).abs() < f32::EPSILON);
  let states = playbacks(&effects);
  assert_eq!(states.len(), 1);
  assert_eq!(states[0].pv, pv + 1);
  assert_eq!(states[0].sound.sv, 1);
  assert_eq!(states[0].sound.by, 1);
  assert_eq!(states[0].sound.sound, slowed());
  assert_eq!(states[0].sound.ack_of(1), Some(7));
  assert_eq!(states[0].pos_ms / 1000, 0);
}

#[test]
fn a_sound_op_is_sanitized_by_the_host() {
  let (mut app, _) = host();
  let wild = JamSound {
    speed: 50.0,
    reverb: f32::NAN,
    ..JamSound::default()
  };
  let effects = guest_op(
    &mut app,
    Op::Sound {
      sound: wild,
      smooth: false,
      preset: Some("custom_1".to_string()),
    },
  );
  let state = playbacks(&effects)[0];
  assert!((state.sound.sound.speed - 2.0).abs() < f32::EPSILON);
  assert!(state.sound.sound.reverb.abs() < f32::EPSILON);
  assert_eq!(state.sound.preset, None);
  let kept = guest_op(
    &mut app,
    Op::Sound {
      sound: slowed(),
      smooth: false,
      preset: Some("p_slowed".to_string()),
    },
  );
  assert_eq!(
    playbacks(&kept)[0].sound.preset.as_deref(),
    Some("p_slowed")
  );
}

#[test]
fn the_host_keeps_its_volume_and_equalizer_when_a_guest_sets_effects() {
  let (mut app, _) = host();
  audio_pref(&mut app, AudioMessage::Set(SoundField::Volume, 1.4));
  audio_pref(&mut app, AudioMessage::Set(SoundField::Low, 3.0));
  audio_pref(&mut app, AudioMessage::Set(SoundField::Band(2), -4.0));
  let effects = guest_op(&mut app, sound_op(slowed()));
  let engine = engine_sounds(&effects);
  assert!((engine[0].volume - 1.4).abs() < f32::EPSILON);
  assert!((engine[0].eq.low - 3.0).abs() < f32::EPSILON);
  assert!((engine[0].eq.bands[2] + 4.0).abs() < f32::EPSILON);
  assert!((app.ui.audio.live.volume - 1.4).abs() < f32::EPSILON);
}

#[test]
fn a_smooth_sound_op_moves_the_host_through_the_transition() {
  let (mut app, _) = host();
  let effects = guest_op(
    &mut app,
    Op::Sound {
      sound: slowed(),
      smooth: true,
      preset: None,
    },
  );
  assert!(engine_sounds(&effects).is_empty());
  assert!(app.ui.audio.tween.is_some());
  for _ in 0..30 {
    audio_pref(&mut app, AudioMessage::TweenTick);
  }
  assert!(app.ui.audio.tween.is_none());
  assert!((app.ui.audio.live.speed_pitch - 0.82).abs() < f32::EPSILON);
}

#[test]
fn two_guests_setting_effects_together_converge_on_the_last_one_with_both_acks() {
  let (mut app, _) = host();
  event(
    &mut app,
    JamEvent::GuestOp {
      from: 1,
      id: 7,
      op: sound_op(slowed()),
    },
  );
  let last = JamSound {
    distortion: 75.0,
    ..JamSound::default()
  };
  let effects = event(
    &mut app,
    JamEvent::GuestOp {
      from: 2,
      id: 3,
      op: sound_op(last),
    },
  );
  let state = playbacks(&effects)[0];
  assert_eq!(state.sound.sv, 2);
  assert_eq!(state.sound.by, 2);
  assert_eq!(state.sound.sound, last);
  assert_eq!(state.sound.ack_of(1), Some(7));
  assert_eq!(state.sound.ack_of(2), Some(3));
}

#[test]
fn a_guest_leaving_keeps_the_effects_of_the_others() {
  let (mut app, _) = host();
  guest_op(&mut app, sound_op(slowed()));
  let alone = vec![participants()[0].clone()];
  let effects = event(&mut app, JamEvent::Participants(alone));
  assert!(engine_sounds(&effects).is_empty());
  assert!((app.ui.audio.live.speed_pitch - 0.82).abs() < f32::EPSILON);
}

#[test]
fn setting_a_slider_alone_sends_nothing_and_releasing_it_sends_the_effects() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  for step in 1..=20_i16 {
    let effects = audio_pref(
      &mut app,
      AudioMessage::Set(SoundField::Reverb, f32::from(step)),
    );
    assert!(sent(&effects).is_empty());
  }
  assert!((app.ui.audio.live.reverb - 20.0).abs() < f32::EPSILON);
  let released = audio_pref(&mut app, AudioMessage::Commit);
  let sent = commands(&released);
  assert_eq!(sent.len(), 1);
  match sent[0].1 {
    Op::Sound { sound, .. } => assert!((sound.reverb - 20.0).abs() < f32::EPSILON),
    other => panic!("Op::Sound attendu, reçu {other:?}"),
  }
  assert_eq!(app.jam.as_ref().unwrap().sound_pending, Some(sent[0].0));
}

#[test]
fn releasing_a_personal_slider_sends_no_effect() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  audio_pref(&mut app, AudioMessage::Set(SoundField::Volume, 1.5));
  audio_pref(&mut app, AudioMessage::Set(SoundField::Mid, 4.0));
  let released = audio_pref(&mut app, AudioMessage::Commit);
  assert!(commands(&released).is_empty());
  assert!(app.jam.as_ref().unwrap().sound_pending.is_none());
}

#[test]
fn releasing_a_personal_slider_during_a_received_transition_keeps_the_effects_of_the_jam() {
  let (mut app, _) = host();
  guest_op(
    &mut app,
    Op::Sound {
      sound: slowed(),
      smooth: true,
      preset: None,
    },
  );
  audio_pref(&mut app, AudioMessage::TweenTick);
  assert!(app.ui.audio.tween.is_some());
  audio_pref(&mut app, AudioMessage::Set(SoundField::Volume, 1.5));
  let released = audio_pref(&mut app, AudioMessage::Commit);
  assert!(playbacks(&released).is_empty());
  let shared = app.jam.as_ref().unwrap().shared.as_ref().unwrap();
  assert_eq!(shared.sv, 1);
  assert!((shared.sound.reverb - 65.0).abs() < f32::EPSILON);
  assert!((app.ui.audio.live.reverb - 65.0).abs() < f32::EPSILON);
}

#[test]
fn the_host_releasing_a_slider_before_the_relay_is_ready_shares_the_effects() {
  let mut app = base_app(vec![track("a"), track("b"), track("c")]);
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  loaded(&mut app);
  jam(&mut app, JamMessage::Start);
  audio_pref(&mut app, AudioMessage::Set(SoundField::Reverb, 50.0));
  audio_pref(&mut app, AudioMessage::Commit);
  let shared = app.jam.as_ref().unwrap().shared.clone().unwrap();
  assert_eq!(shared.sv, 1);
  assert!((shared.sound.reverb - 50.0).abs() < f32::EPSILON);
}

#[test]
fn starting_a_jam_during_a_preset_transition_still_writes_the_new_sound() {
  let mut app = base_app(vec![track("a"), track("b"), track("c")]);
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  loaded(&mut app);
  assert!(app.settings.sound.smooth_presets);
  audio_pref(
    &mut app,
    AudioMessage::ApplyPreset("p_distortion".to_string()),
  );
  let target = app.ui.audio.tween.expect("transition").to;
  let effects = jam(&mut app, JamMessage::Start);
  assert!(saved_sounds(&effects).contains(&target));
}

#[test]
fn the_host_releasing_a_slider_broadcasts_the_effects() {
  let (mut app, _) = host();
  audio_pref(&mut app, AudioMessage::Set(SoundField::Echo, 30.0));
  let none = guest_op(&mut app, Op::Play);
  assert!(playbacks(&none).iter().all(|state| state.sound.sv == 0));
  let released = audio_pref(&mut app, AudioMessage::Commit);
  let state = playbacks(&released)[0];
  assert_eq!(state.sound.sv, 1);
  assert_eq!(state.sound.by, 0);
  assert!((state.sound.sound.echo - 30.0).abs() < f32::EPSILON);
}

#[test]
fn a_preset_in_a_jam_sends_the_effects_only_and_keeps_the_personal_volume() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  app.settings.sound.smooth_presets = false;
  let effects = audio_pref(
    &mut app,
    AudioMessage::ApplyPreset("p_distortion".to_string()),
  );
  let sent = commands(&effects);
  match sent[0].1 {
    Op::Sound { sound, preset, .. } => {
      assert!((sound.distortion - 75.0).abs() < f32::EPSILON);
      assert_eq!(preset.as_deref(), Some("p_distortion"));
    }
    other => panic!("Op::Sound attendu, reçu {other:?}"),
  }
  // Aperçu immédiat, sans le gain de 1,25 du préréglage.
  assert!((app.ui.audio.live.distortion - 75.0).abs() < f32::EPSILON);
  assert!((app.ui.audio.live.volume - 1.0).abs() < f32::EPSILON);
  assert_eq!(app.settings.sound.active_preset, "p_default");
}

#[test]
fn a_custom_preset_sends_no_preset_id_and_the_host_keeps_the_active_preset_out_of_the_settings() {
  let (mut app, _) = host();
  app.settings.sound.smooth_presets = false;
  app
    .settings
    .sound
    .custom_presets
    .push(crate::audio_prefs::Preset {
      id: "custom_9".to_string(),
      name: "Mien".to_string(),
      settings: Sound {
        reverb: 40.0,
        ..Sound::default()
      },
    });
  let effects = audio_pref(&mut app, AudioMessage::ApplyPreset("custom_9".to_string()));
  let state = playbacks(&effects)[0];
  assert_eq!(state.sound.preset, None);
  assert!((state.sound.sound.reverb - 40.0).abs() < f32::EPSILON);
  let effects = audio_pref(&mut app, AudioMessage::ApplyPreset("p_slowed".to_string()));
  assert_eq!(
    playbacks(&effects)[0].sound.preset.as_deref(),
    Some("p_slowed")
  );
  assert_eq!(app.active_preset(), "p_slowed");
  assert_eq!(app.settings.sound.active_preset, "p_default");
}

#[test]
fn resetting_the_audio_settings_in_a_jam_resets_the_shared_effects() {
  let (mut app, _) = host();
  guest_op(&mut app, sound_op(slowed()));
  let effects = audio_pref(&mut app, AudioMessage::ResetAll);
  let state = playbacks(&effects).into_iter().last().expect("état");
  assert_eq!(state.sound.sound, JamSound::default());
  assert_eq!(state.sound.preset.as_deref(), Some("p_default"));
  assert_eq!(app.ui.audio.live, Sound::default());
}

#[test]
fn a_guest_follows_the_effects_of_the_host() {
  let (mut app, _, anchor) = guest(&AB, 0, 0, true, Vec::new());
  let effects = event(
    &mut app,
    JamEvent::Playback {
      state: sound_state(
        SharedSound {
          by: 0,
          ..shared_of(slowed(), 1)
        },
        2,
      ),
      anchor,
    },
  );
  assert!(!engine_sounds(&effects).is_empty() || app.ui.audio.tween.is_some());
  for _ in 0..30 {
    audio_pref(&mut app, AudioMessage::TweenTick);
  }
  assert!((app.ui.audio.live.speed_pitch - 0.82).abs() < f32::EPSILON);
  assert!((app.ui.audio.live.reverb - 65.0).abs() < f32::EPSILON);
  // Un état identique (battement) ne rejoue rien.
  let again = event(
    &mut app,
    JamEvent::Playback {
      state: sound_state(shared_of(slowed(), 1), 3),
      anchor,
    },
  );
  assert!(engine_sounds(&again).is_empty());
  assert!(app.ui.audio.tween.is_none());
  // L'hôte retire l'effet : l'invité aussi.
  let removed = event(
    &mut app,
    JamEvent::Playback {
      state: sound_state(shared_of(JamSound::default(), 2), 4),
      anchor,
    },
  );
  let mut shown = engine_sounds(&removed);
  if shown.is_empty() {
    for _ in 0..30 {
      shown.extend(engine_sounds(&audio_pref(
        &mut app,
        AudioMessage::TweenTick,
      )));
    }
  }
  assert!(
    shown
      .last()
      .is_some_and(|last| last.reverb.abs() < f32::EPSILON)
  );
  assert!(app.ui.audio.live.speed_pitch.to_bits() == 1.0_f32.to_bits());
}

#[test]
fn a_guest_keeps_its_volume_and_equalizer_under_the_effects_of_the_host() {
  let mut app = base_app(Vec::new());
  app.ui.audio.live.volume = 1.4;
  app.ui.audio.live.eq.high = 5.0;
  let (app, effects) = guest_joined_with(
    app,
    SharedSound {
      smooth: false,
      ..shared_of(slowed(), 1)
    },
    0,
    Duration::ZERO,
    true,
  );
  let engine = engine_sounds(&effects);
  assert!((engine[0].speed_pitch - 0.82).abs() < f32::EPSILON);
  assert!((engine[0].volume - 1.4).abs() < f32::EPSILON);
  assert!((engine[0].eq.high - 5.0).abs() < f32::EPSILON);
  assert!((app.ui.audio.live.volume - 1.4).abs() < f32::EPSILON);
}

#[test]
fn joining_mid_track_applies_the_effects_before_loading_and_expects_the_position_at_the_rate() {
  let (app, effects) = guest_joined_with(
    base_app(Vec::new()),
    SharedSound {
      smooth: false,
      ..shared_of(slowed(), 1)
    },
    10_000,
    Duration::from_secs(10),
    true,
  );
  let sound_at = effects
    .iter()
    .position(|effect| matches!(effect, Effect::AudioSound(_)))
    .expect("effets appliqués");
  let load_at = effects
    .iter()
    .position(|effect| matches!(effect, Effect::ResolveTrack { .. }))
    .expect("titre chargé");
  assert!(sound_at < load_at);
  let (_, _, start) = resolved(&effects).expect("resolve");
  // 10 s de position plus 10 s écoulées à 0,82, pas 20 s.
  assert!(start >= Duration::from_millis(18_150) && start < Duration::from_millis(18_500));
  assert!((app.ui.audio.live.speed_pitch - 0.82).abs() < f32::EPSILON);
}

#[test]
fn joining_mid_track_applies_a_smooth_effect_at_once_before_loading() {
  let (app, effects) = guest_joined_with(
    base_app(Vec::new()),
    SharedSound {
      smooth: true,
      ..shared_of(slowed(), 1)
    },
    10_000,
    Duration::from_secs(10),
    true,
  );
  let sound_at = effects
    .iter()
    .position(|effect| matches!(effect, Effect::AudioSound(_)))
    .expect("effets appliqués");
  let load_at = effects
    .iter()
    .position(|effect| matches!(effect, Effect::ResolveTrack { .. }))
    .expect("titre chargé");
  assert!(sound_at < load_at);
  assert!(app.ui.audio.tween.is_none());
  assert!((app.ui.audio.live.speed_pitch - 0.82).abs() < f32::EPSILON);
}

#[test]
fn no_correction_is_needed_for_twenty_seconds_of_playback_at_the_rate() {
  let (mut app, _) = guest_joined_with(
    base_app(Vec::new()),
    SharedSound {
      smooth: false,
      ..shared_of(slowed(), 1)
    },
    0,
    Duration::from_secs(20),
    true,
  );
  loaded(&mut app);
  // 20 s écoulées à 0,82 : le moteur est à 16,4 s.
  let at_rate = audio(
    &mut app,
    AudioEvent::Position(Duration::from_millis(16_400)),
  );
  assert!(
    !at_rate
      .iter()
      .any(|effect| matches!(effect, Effect::AudioSeek(_)))
  );
  // Sans effet partagé, le même écart (3,6 s) est une dérive.
  let (mut plain, _) = guest_joined_with(
    base_app(Vec::new()),
    shared_of(JamSound::default(), 0),
    0,
    Duration::from_secs(20),
    true,
  );
  loaded(&mut plain);
  let drift = audio(
    &mut plain,
    AudioEvent::Position(Duration::from_millis(16_400)),
  );
  assert!(
    drift
      .iter()
      .any(|effect| matches!(effect, Effect::AudioSeek(_)))
  );
}

#[test]
fn a_host_publishes_its_position_at_the_rate_of_its_engine() {
  let (mut app, _) = host();
  guest_op(&mut app, sound_op(slowed()));
  audio(&mut app, AudioEvent::Position(Duration::from_secs(30)));
  let effects = player(&mut app, PlayerMessage::TogglePlay);
  let state = playbacks(&effects).into_iter().last().expect("état");
  assert!(state.pos_ms >= 30_000 && state.pos_ms < 31_000);
}

#[test]
fn a_guest_ignores_received_effects_until_its_own_command_is_acknowledged() {
  let (mut app, _, anchor) = guest(&AB, 0, 0, true, Vec::new());
  app.settings.sound.smooth_presets = false;
  audio_pref(&mut app, AudioMessage::Set(SoundField::Reverb, 40.0));
  let released = audio_pref(&mut app, AudioMessage::Commit);
  let id = commands(&released)[0].0;
  // Un état de l'hôte sans acquittement : l'aperçu local reste.
  let ignored = event(
    &mut app,
    JamEvent::Playback {
      state: sound_state(shared_of(JamSound::default(), 1), 2),
      anchor,
    },
  );
  assert!(engine_sounds(&ignored).is_empty());
  assert!((app.ui.audio.live.reverb - 40.0).abs() < f32::EPSILON);
  assert_eq!(app.jam.as_ref().unwrap().sound_pending, Some(id));
  // L'acquittement revient avec le son retenu par l'hôte (un autre invité est passé après).
  let mut acked = SharedSound {
    smooth: false,
    ..shared_of(
      JamSound {
        echo: 20.0,
        ..JamSound::default()
      },
      2,
    )
  };
  acked.acknowledge(1, id);
  let applied = event(
    &mut app,
    JamEvent::Playback {
      state: sound_state(acked, 3),
      anchor,
    },
  );
  assert!(!engine_sounds(&applied).is_empty());
  assert!(app.jam.as_ref().unwrap().sound_pending.is_none());
  assert!(app.ui.audio.live.reverb.abs() < f32::EPSILON);
  assert!((app.ui.audio.live.echo - 20.0).abs() < f32::EPSILON);
}

#[test]
fn a_rejected_sound_command_gives_the_effects_back_to_the_host() {
  let (mut app, _, anchor) = guest(&AB, 0, 0, true, Vec::new());
  app.settings.sound.smooth_presets = false;
  audio_pref(&mut app, AudioMessage::Set(SoundField::Reverb, 40.0));
  let released = audio_pref(&mut app, AudioMessage::Commit);
  let id = commands(&released)[0].0;
  event(
    &mut app,
    JamEvent::Playback {
      state: sound_state(shared_of(JamSound::default(), 1), 2),
      anchor,
    },
  );
  assert!((app.ui.audio.live.reverb - 40.0).abs() < f32::EPSILON);
  let effects = event(
    &mut app,
    JamEvent::Rejected {
      id,
      reason: crate::jam::protocol::RejectReason::Rate,
    },
  );
  assert!(app.jam.as_ref().unwrap().sound_pending.is_none());
  assert!(app.ui.audio.live.reverb.abs() < f32::EPSILON);
  assert!(!engine_sounds(&effects).is_empty());
}

#[test]
fn a_command_older_than_the_pending_one_does_not_release_it() {
  let (mut app, _, anchor) = guest(&AB, 0, 0, true, Vec::new());
  audio_pref(&mut app, AudioMessage::Set(SoundField::Reverb, 40.0));
  let first = commands(&audio_pref(&mut app, AudioMessage::Commit))[0].0;
  audio_pref(&mut app, AudioMessage::Set(SoundField::Reverb, 55.0));
  let second = commands(&audio_pref(&mut app, AudioMessage::Commit))[0].0;
  assert!(second > first);
  let mut state = shared_of(
    JamSound {
      reverb: 40.0,
      ..JamSound::default()
    },
    1,
  );
  state.acknowledge(1, first);
  event(
    &mut app,
    JamEvent::Playback {
      state: sound_state(state, 2),
      anchor,
    },
  );
  assert_eq!(app.jam.as_ref().unwrap().sound_pending, Some(second));
  assert!((app.ui.audio.live.reverb - 55.0).abs() < f32::EPSILON);
}

#[test]
fn the_personal_sound_is_kept_at_the_first_join_and_not_overwritten_by_a_reconnection() {
  let mut app = base_app(Vec::new());
  app.ui.audio.live.speed = 1.5;
  app.ui.audio.live.pitch = 2.0;
  let personal = app.ui.audio.live;
  let (mut app, _) = guest_joined_with(
    app,
    SharedSound {
      smooth: false,
      ..shared_of(slowed(), 1)
    },
    0,
    Duration::ZERO,
    true,
  );
  assert_eq!(app.jam.as_ref().unwrap().personal, Some(personal));
  assert!((app.ui.audio.live.speed_pitch - 0.82).abs() < f32::EPSILON);
  event(&mut app, JamEvent::Reconnecting { attempt: 1 });
  let mut playback = playback_state(&AB, 0, 5, 1, 0, true);
  playback.sound = SharedSound {
    smooth: false,
    ..shared_of(slowed(), 1)
  };
  event(
    &mut app,
    JamEvent::Joined {
      you: 1,
      participants: participants(),
      queue: QueueState::default(),
      playback,
      anchor: Instant::now(),
    },
  );
  assert_eq!(app.jam.as_ref().unwrap().personal, Some(personal));
}

#[test]
fn every_end_of_a_jam_gives_the_guest_its_own_effects_back() {
  for cause in [EndCause::HostEnded, EndCause::Lost, EndCause::Stopped] {
    let mut app = base_app(Vec::new());
    app.ui.audio.live.speed = 1.5;
    app.ui.audio.live.echo = 33.0;
    let personal = app.ui.audio.live;
    let (mut app, _) = guest_joined_with(
      app,
      SharedSound {
        smooth: false,
        ..shared_of(slowed(), 1)
      },
      0,
      Duration::ZERO,
      true,
    );
    assert_ne!(app.ui.audio.live, personal);
    let effects = event(&mut app, JamEvent::Ended(cause.clone()));
    assert_eq!(app.ui.audio.live, personal, "{cause:?}");
    let engine = engine_sounds(&effects);
    assert_eq!(
      engine.last().map(|e| e.echo.to_bits()),
      Some(33.0_f32.to_bits())
    );
    assert!(
      saved_sounds(&effects).is_empty(),
      "la fin du jam n'écrit rien"
    );
  }
}

#[test]
fn a_task_ending_gives_the_effects_back_too() {
  let mut app = base_app(Vec::new());
  app.ui.audio.live.reverb = 12.0;
  let personal = app.ui.audio.live;
  let (mut app, _) = guest_joined_with(
    app,
    SharedSound {
      smooth: false,
      ..shared_of(slowed(), 1)
    },
    0,
    Duration::ZERO,
    true,
  );
  jam(&mut app, JamMessage::TaskEnded(1, EndCause::Lost));
  assert!(app.jam.is_none());
  assert_eq!(app.ui.audio.live, personal);
}

#[test]
fn the_host_gets_its_own_effects_back_when_the_jam_ends_or_when_it_is_left_alone() {
  for cause in [EndCause::Stopped, EndCause::Alone] {
    let (mut app, _) = host_with_sound(|app| {
      app.ui.audio.live.speed = 1.2;
    });
    app.settings.sound.smooth_presets = false;
    let before = app.ui.audio.live;
    guest_op(&mut app, sound_op(slowed()));
    assert_ne!(app.ui.audio.live, before);
    let effects = event(&mut app, JamEvent::Ended(cause.clone()));
    assert_eq!(app.ui.audio.live, before, "{cause:?}");
    assert_eq!(
      engine_sounds(&effects).last().map(|e| e.speed.to_bits()),
      Some(1.2_f32.to_bits())
    );
  }
}

#[test]
fn a_volume_set_during_a_jam_survives_the_end_of_the_jam() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  audio_pref(&mut app, AudioMessage::Set(SoundField::Volume, 1.5));
  let saved = audio_pref(&mut app, AudioMessage::Commit);
  assert!(
    saved_sounds(&saved)
      .iter()
      .all(|sound| (sound.volume - 1.5).abs() < f32::EPSILON)
  );
  event(&mut app, JamEvent::Ended(EndCause::HostEnded));
  assert!((app.ui.audio.live.volume - 1.5).abs() < f32::EPSILON);
}

#[test]
fn no_audio_message_writes_the_effects_of_the_jam_to_the_settings() {
  let cases: Vec<(&str, AudioMessage)> = vec![
    ("Commit", AudioMessage::Commit),
    (
      "ApplyPreset",
      AudioMessage::ApplyPreset("p_distortion".to_string()),
    ),
    ("ResetBands", AudioMessage::ResetBands),
    ("ResetAll", AudioMessage::ResetAll),
    ("ToggleHq", AudioMessage::ToggleHq(true)),
    ("HqEngine", AudioMessage::HqEngine("glyphs-7B".to_string())),
    ("TogglePersist", AudioMessage::TogglePersist(true)),
    ("ToggleSmooth", AudioMessage::ToggleSmooth(false)),
    ("Device", AudioMessage::Device(Some("sortie".to_string()))),
    (
      "DeletePreset",
      AudioMessage::DeletePreset("custom_9".to_string()),
    ),
    ("PresetSave", AudioMessage::PresetSave),
  ];
  for (name, message) in cases {
    let mut app = base_app(Vec::new());
    app.settings.sound.smooth_presets = false;
    let (mut app, _) = guest_joined_with(
      app,
      SharedSound {
        smooth: false,
        ..shared_of(slowed(), 1)
      },
      0,
      Duration::ZERO,
      true,
    );
    assert!((app.ui.audio.live.reverb - 65.0).abs() < f32::EPSILON);
    if name == "PresetSave" {
      audio_pref(&mut app, AudioMessage::NewPreset);
      audio_pref(&mut app, AudioMessage::PresetName("Nom".to_string()));
    }
    let effects = audio_pref(&mut app, message);
    let written = saved_sounds(&effects);
    for sound in &written {
      assert!(
        sound.reverb.abs() < f32::EPSILON
          && sound.echo.abs() < f32::EPSILON
          && (sound.speed_pitch - 1.0).abs() < f32::EPSILON,
        "{name} écrit les effets du jam : {sound:?}"
      );
    }
    assert!(
      app.settings.sound.audio.reverb.abs() < f32::EPSILON,
      "{name} laisse les effets du jam dans les réglages"
    );
    assert_eq!(app.settings.sound.active_preset, "p_default", "{name}");
  }
}

#[test]
fn the_end_of_a_transition_in_a_jam_writes_the_personal_sound() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  audio_pref(&mut app, AudioMessage::ApplyPreset("p_slowed".to_string()));
  assert!(app.ui.audio.tween.is_some());
  let mut written = Vec::new();
  for _ in 0..40 {
    written.extend(saved_sounds(&audio_pref(&mut app, AudioMessage::TweenTick)));
  }
  assert!(app.ui.audio.tween.is_none());
  assert!((app.ui.audio.live.reverb - 65.0).abs() < f32::EPSILON);
  assert_eq!(written.len(), 1);
  assert!(written[0].reverb.abs() < f32::EPSILON);
}

#[test]
fn a_new_preset_saved_in_a_jam_does_not_become_the_active_preset_of_the_settings() {
  let (mut app, _) = host();
  guest_op(&mut app, sound_op(slowed()));
  audio_pref(&mut app, AudioMessage::NewPreset);
  audio_pref(&mut app, AudioMessage::PresetName("Capté".to_string()));
  audio_pref(&mut app, AudioMessage::PresetSave);
  assert_eq!(app.settings.sound.custom_presets.len(), 1);
  assert_eq!(app.settings.sound.active_preset, "p_default");
}

#[test]
fn the_effects_of_the_jam_come_from_the_host_in_the_active_preset_of_a_guest() {
  let (mut app, _, anchor) = guest(&AB, 0, 0, true, Vec::new());
  assert_eq!(app.active_preset(), "");
  let mut shared = shared_of(slowed(), 1);
  shared.preset = Some("p_slowed".to_string());
  shared.by = 0;
  event(
    &mut app,
    JamEvent::Playback {
      state: sound_state(shared, 2),
      anchor,
    },
  );
  assert_eq!(app.active_preset(), "p_slowed");
  assert_eq!(app.settings.sound.active_preset, "p_default");
}

#[test]
fn operations_are_blocked_while_reconnecting() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  event(&mut app, JamEvent::Reconnecting { attempt: 1 });
  let effects = player(&mut app, PlayerMessage::TogglePlay);
  assert!(sent(&effects).is_empty());
  assert!(has_toast(&effects, "Reconnecting to the Jam…"));
}

#[test]
fn a_refused_join_ends_with_a_toast_and_no_state() {
  let mut app = base_app(Vec::new());
  jam(&mut app, JamMessage::CodeInput("riffjam-test".to_string()));
  jam(&mut app, JamMessage::Join);
  let effects = event(&mut app, JamEvent::Ended(EndCause::Refused(Refusal::Room)));
  assert!(app.jam.is_none());
  assert!(has_toast(&effects, "Wrong or expired invite code"));
}

#[test]
fn a_code_not_found_ends_with_a_toast_and_no_state() {
  let mut app = base_app(Vec::new());
  jam(&mut app, JamMessage::CodeInput("k7q-m2x".to_string()));
  jam(&mut app, JamMessage::Join);
  let effects = jam(&mut app, JamMessage::TaskEnded(1, EndCause::CodeNotFound));
  assert!(app.jam.is_none());
  assert!(has_toast(&effects, "Code not found or the Jam ended"));
}

#[test]
fn the_code_field_is_uppercased_while_typing_and_formatted_when_pasted() {
  let mut app = base_app(Vec::new());
  jam(&mut app, JamMessage::CodeInput("k7q-m".to_string()));
  assert_eq!(app.ui.jam_panel.code, "K7Q-M");
  jam(
    &mut app,
    JamMessage::Pasted(Some("  k7q m2x \n".to_string())),
  );
  assert_eq!(app.ui.jam_panel.code, "K7Q-M2X");
  jam(&mut app, JamMessage::Pasted(Some("k7qm2x".to_string())));
  assert_eq!(app.ui.jam_panel.code, "K7Q-M2X");
  jam(
    &mut app,
    JamMessage::Pasted(Some(" not a code \n".to_string())),
  );
  assert_eq!(app.ui.jam_panel.code, "NOT A CODE");
  jam(&mut app, JamMessage::Pasted(None));
  assert_eq!(app.ui.jam_panel.code, "NOT A CODE");
}

#[test]
fn an_old_long_ticket_is_kept_as_typed_or_pasted() {
  let mut app = base_app(Vec::new());
  let ticket = "riffjamabcdefghijklmnop234567";
  jam(&mut app, JamMessage::CodeInput(ticket.to_string()));
  assert_eq!(app.ui.jam_panel.code, ticket);
  jam(&mut app, JamMessage::Pasted(Some(format!(" {ticket}\n"))));
  assert_eq!(app.ui.jam_panel.code, ticket);
}

#[test]
fn joining_sends_the_code_and_the_rendezvous_setting() {
  let mut app = base_app(Vec::new());
  jam(&mut app, JamMessage::Pasted(Some("k7q-m2x".to_string())));
  let effects = jam(&mut app, JamMessage::Join);
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::JamJoin { ticket, rendezvous: None, .. } if ticket == "K7Q-M2X"
  )));
  let mut app = base_app(Vec::new());
  app.settings.jam_rendezvous = "  https://pkarr.example/pkarr  ".to_string();
  jam(&mut app, JamMessage::CodeInput("K7QM2X".to_string()));
  let effects = jam(&mut app, JamMessage::Join);
  assert!(effects.iter().any(|effect| matches!(
    effect,
    Effect::JamJoin { rendezvous: Some(url), .. } if url == "https://pkarr.example/pkarr"
  )));
}

#[test]
fn hosting_sends_the_rendezvous_setting_only_when_it_is_set() {
  for (setting, expected) in [
    ("", None),
    ("  ", None),
    (" https://pkarr.example/ ", Some("https://pkarr.example/")),
  ] {
    let mut app = base_app(vec![track("a")]);
    app.settings.jam_rendezvous = setting.to_string();
    let effects = jam(&mut app, JamMessage::Start);
    assert!(
      effects.iter().any(|effect| matches!(
        effect,
        Effect::JamHost { rendezvous, .. } if rendezvous.as_deref() == expected
      )),
      "{setting:?}"
    );
  }
}

#[test]
fn a_code_taken_over_replaces_the_shown_code_and_warns_the_host() {
  let (mut app, _) = host();
  assert_eq!(
    app.jam.as_ref().unwrap().ticket.as_deref(),
    Some("riffjam-test")
  );
  let effects = event(
    &mut app,
    JamEvent::CodeChanged {
      ticket: "ABC-DEF".to_string(),
    },
  );
  assert!(has_toast(&effects, "Jam code compromised"));
  assert_eq!(app.jam.as_ref().unwrap().ticket.as_deref(), Some("ABC-DEF"));
  let copied = jam(&mut app, JamMessage::CopyCode);
  assert!(
    copied
      .iter()
      .any(|effect| matches!(effect, Effect::CopyText(text) if text == "ABC-DEF"))
  );
}

#[test]
fn a_guest_ignores_a_code_change_it_cannot_receive() {
  let (mut app, _, _) = guest(&AB, 0, 0, true, Vec::new());
  let effects = event(
    &mut app,
    JamEvent::CodeChanged {
      ticket: "ABC-DEF".to_string(),
    },
  );
  assert!(effects.is_empty());
  assert_eq!(app.jam.as_ref().unwrap().ticket, None);
}

#[test]
fn a_guest_never_writes_the_shuffle_into_the_saved_data() {
  let (mut app, _, anchor) = guest(&AB, 0, 0, true, Vec::new());
  let saved = (app.data.shuffle, app.data.repeat);
  event(
    &mut app,
    JamEvent::Playback {
      state: PlaybackState {
        shuffle: true,
        repeat: true,
        ..playback_state(&AB, 0, 2, 1, 0, true)
      },
      anchor,
    },
  );
  assert!(app.player.shuffle && app.player.repeat);
  assert_eq!((app.data.shuffle, app.data.repeat), saved);
}

#[test]
fn a_task_ending_carries_its_own_cause() {
  let mut app = base_app(Vec::new());
  jam(&mut app, JamMessage::CodeInput("riffjam-test".to_string()));
  jam(&mut app, JamMessage::Join);
  let effects = jam(
    &mut app,
    JamMessage::TaskEnded(1, EndCause::Refused(Refusal::Full)),
  );
  assert!(app.jam.is_none());
  assert!(has_toast(&effects, "This Jam is full"));
}

#[test]
fn starting_a_jam_clears_pending_radio_and_autoplay() {
  let mut app = base_app(vec![track("a")]);
  app.ui.autoplay_waiting = true;
  app.ui.autoplay_loading = true;
  app.ui.autoplay_found = Some(vec![track("z")]);
  app.ui.radio_waiting = true;
  app.ui.radio_loading = true;
  jam(&mut app, JamMessage::Start);
  assert!(!app.ui.autoplay_waiting && !app.ui.autoplay_loading && !app.ui.radio_waiting);
  assert!(!app.ui.radio_loading && app.ui.autoplay_found.is_none());
}

#[test]
fn late_radio_and_autoplay_replies_do_not_extend_the_jam_queue() {
  let (mut app, _) = host();
  let before = queue_ids(&app).len();
  app.ui.radio_waiting = true;
  let reply = serde_json::json!({ "tracks": [{ "id": "r", "title": "Radio", "url": "https://exemple.org/r" }] });
  app.update(Message::Ui(UiMessage::Reply(
    crate::ui::ReplyTag::RadioRefill,
    Ok(reply),
  )));
  app.ui.autoplay_waiting = true;
  player(&mut app, PlayerMessage::AutoplayReady(vec![track("z")]));
  assert_eq!(queue_ids(&app).len(), before);
  assert!(!app.ui.radio_waiting && !app.ui.autoplay_waiting);
  assert!(app.ui.autoplay_found.is_none());
}

#[test]
fn a_big_add_counts_local_files_and_cut_tracks_separately() {
  let (mut app, _) = host();
  let mut tracks: Vec<Track> = (0..205).map(|n| track(&format!("t{n}"))).collect();
  tracks.push(local("l1"));
  tracks.push(local("l2"));
  let effects = app.jam_add(tracks, false);
  assert!(has_toast(&effects, "2 local files skipped"));
  assert!(has_toast(&effects, "5 tracks over the limit were skipped"));
}

#[test]
fn ending_while_starting_sends_stop_at_once() {
  let mut app = base_app(vec![track("a")]);
  jam(&mut app, JamMessage::Start);
  assert_eq!(
    app.jam.as_ref().unwrap().phase,
    super::state::Phase::Starting
  );
  let effects = jam(&mut app, JamMessage::End);
  assert!(sent(&effects).contains(&&JamOut::Stop));
}

#[test]
fn a_lyrics_line_click_seeks_through_the_jam() {
  let (mut guest_app, _, _) = guest(
    &[("a", "ytmusic"), ("b", "ytmusic")],
    0,
    0,
    true,
    Vec::new(),
  );
  guest_app.lyrics.lyrics = Some(crate::lyrics::model::Lyrics::parse(
    "[00:01.00]one\n[00:03.00]two",
    false,
  ));
  guest_app.relayout();
  let effects = guest_app.update_lyrics(crate::lyrics::update::LyricsMessage::LineClicked(1));
  assert!(!effects.iter().any(|e| matches!(e, Effect::AudioSeek(_))));
  assert!(sent(&effects).iter().any(|out| matches!(
    out,
    JamOut::Command {
      op: Op::Seek { pos_ms: 3000 },
      ..
    }
  )));
}

#[test]
fn starting_a_jam_freezes_the_platform_of_the_host_queue() {
  let bare = |id: &str| Track {
    platform: None,
    ..track(id)
  };
  let mut app = base_app(vec![bare("a"), bare("b")]);
  app.data.platform = "ytmusic".to_string();
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  jam(&mut app, JamMessage::Start);
  app.data.platform = "soundcloud".to_string();
  assert!(
    app
      .player
      .queue
      .iter()
      .all(|track| track.platform.as_deref() == Some("ytmusic"))
  );
  assert_eq!(
    app
      .player
      .current
      .as_ref()
      .and_then(|t| t.platform.as_deref()),
    Some("ytmusic")
  );
}

#[test]
fn a_blocked_track_is_played_in_a_jam_and_stays_blocked() {
  let (mut app, _) = host();
  app.data.toggle_block_track(&track("b"));
  let from = uid_of(&app, "a");
  guest_op(&mut app, Op::Next { from: Some(from) });
  assert_eq!(current_id(&app), "b");
  assert!(app.data.is_track_blocked(&track("b")));
  let effects = player(&mut app, PlayerMessage::Next);
  assert!(!effects.is_empty());
  assert_eq!(current_id(&app), "c");
}

#[test]
fn a_blocked_track_can_be_added_to_the_jam_queue_and_stays_blocked() {
  let (mut app, _) = host();
  app.data.toggle_block_track(&track("x"));
  guest_op(
    &mut app,
    Op::Add {
      tracks: vec![wire("x", "ytmusic")],
      next: false,
    },
  );
  assert_eq!(queue_ids(&app), ["a", "b", "c", "x"]);
  assert!(app.data.is_track_blocked(&track("x")));
}

#[test]
fn disliking_the_current_track_in_a_jam_blocks_without_skipping() {
  let (mut app, _) = host();
  app.update(Message::Ui(UiMessage::CurrentCommand(
    TrackCommand::BlockTrack,
  )));
  assert!(app.data.is_track_blocked(&track("a")));
  assert_eq!(current_id(&app), "a");
}

#[test]
fn after_the_jam_the_blocked_tracks_are_skipped_again() {
  let (mut app, _) = host();
  app.data.toggle_block_track(&track("b"));
  let from = uid_of(&app, "a");
  guest_op(&mut app, Op::Next { from: Some(from) });
  assert_eq!(current_id(&app), "b");
  loaded(&mut app);
  assert!(app.playback.is_playing);
  event(&mut app, JamEvent::Ended(EndCause::Stopped));
  assert!(app.jam.is_none());
  assert_eq!(current_id(&app), "c", "le titre bloqué joué cède la place");
  assert!(app.data.is_track_blocked(&track("b")));
  app.player.play_at(0);
  player(&mut app, PlayerMessage::Next);
  assert_eq!(current_id(&app), "c", "Next saute le bloqué");
}

#[test]
fn leaving_the_jam_paused_on_a_blocked_track_does_nothing() {
  let (mut app, _) = host();
  app.data.toggle_block_track(&track("b"));
  let from = uid_of(&app, "a");
  guest_op(&mut app, Op::Next { from: Some(from) });
  guest_op(&mut app, Op::Pause);
  assert!(!app.playback.is_playing);
  event(&mut app, JamEvent::Ended(EndCause::Stopped));
  assert_eq!(current_id(&app), "b");
}

#[test]
fn a_blocked_track_played_in_a_jam_is_neither_in_the_history_nor_scrobbled() {
  let (mut app, _) = host();
  app.ui.manage.lastfm.connected = true;
  app.data.toggle_block_track(&track("b"));
  let from = uid_of(&app, "a");
  guest_op(&mut app, Op::Next { from: Some(from) });
  assert_eq!(current_id(&app), "b");
  assert!(
    !app
      .data
      .history
      .iter()
      .any(|played| played.id.as_deref() == Some("b"))
  );
  assert!(app.lastfm_tick(5.0, 100.0).is_empty());
  app.data.toggle_block_track(&track("b"));
  assert!(
    !app.lastfm_tick(6.0, 100.0).is_empty(),
    "titre sain : scrobblé"
  );
}

#[test]
fn blocking_the_artist_of_the_current_track_in_a_jam_does_not_skip() {
  let (mut app, _) = host();
  app.update(Message::Ui(UiMessage::CurrentCommand(
    TrackCommand::BlockArtist,
  )));
  assert!(app.data.is_artist_blocked("Artiste a"));
  assert_eq!(current_id(&app), "a");
}

#[test]
fn importing_an_account_is_not_available_during_a_jam() {
  let (mut app, _) = host();
  let effects = app.update(Message::Manage(ManageMessage::ImportAccount));
  assert!(!effects.contains(&Effect::PickAccount));
  assert!(has_toast(&effects, "Not available during a Jam"));
  let favorites = app.data.favorites.len();
  let effects = app.update(Message::Manage(ManageMessage::AccountRead(Ok(Some(
    serde_json::json!({ "localStorage": {}, "riff_main_always_on_top": "true" }),
  )))));
  assert!(has_toast(&effects, "Not available during a Jam"));
  assert_eq!(app.data.favorites.len(), favorites);
  assert!(app.jam.is_some());
}

#[test]
fn the_host_gets_back_the_shuffle_and_the_repeat_of_the_settings_at_the_end_of_the_jam() {
  let (mut app, _) = host();
  player(&mut app, PlayerMessage::ToggleShuffle);
  player(&mut app, PlayerMessage::ToggleRepeat);
  assert!(app.player.shuffle && app.player.repeat);
  assert!(!app.data.shuffle && !app.data.repeat);
  event(&mut app, JamEvent::Ended(EndCause::Stopped));
  assert!(!app.player.shuffle && !app.player.repeat);
}

#[test]
fn the_host_gets_back_the_shuffle_of_the_playing_list_at_the_end_of_the_jam() {
  let mut app = base_app(vec![track("a"), track("b"), track("c")]);
  app
    .data
    .toggle_shuffle_scope(crate::library::FAVORITES_SCOPE);
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 0)));
  loaded(&mut app);
  jam(&mut app, JamMessage::Start);
  event(&mut app, JamEvent::Participants(participants()));
  player(&mut app, PlayerMessage::ToggleShuffle);
  assert!(!app.player.shuffle);
  event(&mut app, JamEvent::Ended(EndCause::Stopped));
  assert!(app.player.shuffle);
}

#[test]
fn a_plugin_pick_of_a_blocked_track_is_played_in_a_jam() {
  let (mut app, _) = host();
  app.data.toggle_block_track(&track("b"));
  app.player.shuffle = true;
  app.plugins.active = true;
  app.plugins.picks = true;
  let effects = player(&mut app, PlayerMessage::Next);
  let Some(Effect::PluginPick { id, .. }) = effects
    .iter()
    .find(|effect| matches!(effect, Effect::PluginPick { .. }))
  else {
    panic!("attendu : tirage du plugin");
  };
  let id = *id;
  app.update(Message::Plugin(crate::plugins::PluginMessage::Picked(
    id,
    Ok(Some(
      serde_json::json!({ "id": "b", "platform": "ytmusic" }),
    )),
  )));
  assert_eq!(current_id(&app), "b");
  assert!(app.data.is_track_blocked(&track("b")));
}

#[test]
fn joining_and_leaving_a_jam_writes_no_resume_state() {
  let before = vec![track("x"), track("y")];
  let mut app = base_app(before);
  app.update(Message::Ui(UiMessage::RowActivated(ListId::Favorites, 1)));
  let saved = |effects: &[Effect]| {
    effects.iter().any(|effect| {
      matches!(
        effect,
        Effect::SaveResume(_) | Effect::SaveResumePosition(_)
      )
    })
  };
  let mut all = jam(&mut app, JamMessage::CodeInput("riffjam-test".to_string()));
  all.extend(jam(&mut app, JamMessage::Join));
  all.extend(event(
    &mut app,
    JamEvent::Joined {
      you: 1,
      participants: participants(),
      queue: QueueState {
        qv: 1,
        entries: vec![Entry {
          uid: 1,
          by: 0,
          by_name: "User".to_string(),
          track: wire("a", "ytmusic"),
        }],
      },
      playback: playback_state(&AB[..1], 0, 1, 1, 0, true),
      anchor: Instant::now(),
    },
  ));
  all.extend(loaded(&mut app));
  all.extend(event(&mut app, JamEvent::Ended(EndCause::HostEnded)));
  assert!(!saved(&all), "{all:?}");
  // La file personnelle est revenue : le fichier de file est déjà à jour.
  let later = audio(&mut app, AudioEvent::Position(Duration::from_secs(30)));
  assert!(
    !later
      .iter()
      .any(|effect| matches!(effect, Effect::SaveResume(_)))
  );
}

#[test]
fn a_blocked_track_played_in_a_jam_is_never_saved_for_resume() {
  let (mut app, _) = host();
  app.data.toggle_block_track(&track("b"));
  let from = uid_of(&app, "a");
  let mut during = guest_op(&mut app, Op::Next { from: Some(from) });
  during.extend(loaded(&mut app));
  during.extend(audio(
    &mut app,
    AudioEvent::Position(Duration::from_secs(30)),
  ));
  assert_eq!(current_id(&app), "b");
  assert!(
    !during.iter().any(|effect| matches!(
      effect,
      Effect::SaveResume(_) | Effect::SaveResumePosition(_)
    )),
    "{during:?}"
  );
  let ended = event(&mut app, JamEvent::Ended(EndCause::Stopped));
  assert_eq!(current_id(&app), "c");
  let saved: Vec<_> = ended
    .iter()
    .filter_map(|effect| match effect {
      Effect::SaveResume(state) => Some(state),
      _ => None,
    })
    .collect();
  assert_eq!(saved.len(), 1, "{ended:?}");
  assert_eq!(
    saved[0]
      .current
      .as_ref()
      .and_then(|track| track.id.as_deref()),
    Some("c")
  );
}
