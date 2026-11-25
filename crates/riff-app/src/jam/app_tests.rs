//! Tests du jam au niveau `App` : aucune fenêtre, aucun réseau, `App` piloté par `Message::Jam`.

use super::protocol::{
  Current, Entry, Notice, NoticeKind, Op, Participant, PlaybackState, QueueState, Refusal,
  WireTrack,
};
use super::{EndCause, JamEvent, JamMessage, JamOut};
use crate::app::{App, Effect, Message};
use crate::i18n::I18n;
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
