//! Pont entre l'état de l'application et le système : commandes reçues, état envoyé (session média, Discord, paroles).

use super::Command;
use super::now_playing::NowPlaying;
use super::protocol::LyricsState;
use crate::app::{App, Message};
use crate::ui::PlayerMessage;
use crate::user_data::Track;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// Pas de volume d'un raccourci global : `PlayerMessage::VolumeBy` ne lit que le signe.
const VOLUME_SIGN: f32 = 1.0;

/// Messages à appliquer pour une commande du système. `Play` et `Pause` sont idempotents ;
/// `Stop` met en pause, comme dans l'original (`handleMediaCommand`, `app.js:1026`).
#[must_use]
pub fn messages_for(command: Command, playing: bool, has_track: bool) -> Vec<Message> {
  let player = |message| Message::Player(message);
  match command {
    Command::Toggle => vec![player(PlayerMessage::TogglePlay)],
    Command::Play if !playing && has_track => vec![player(PlayerMessage::TogglePlay)],
    Command::Pause | Command::Stop if playing => vec![player(PlayerMessage::TogglePlay)],
    Command::Next => vec![player(PlayerMessage::Next)],
    Command::Previous => vec![player(PlayerMessage::Prev)],
    Command::VolumeUp => vec![player(PlayerMessage::VolumeBy(VOLUME_SIGN))],
    Command::VolumeDown => vec![player(PlayerMessage::VolumeBy(-VOLUME_SIGN))],
    Command::SeekTo(position) if has_track => {
      vec![
        player(PlayerMessage::SeekPreview(position.as_secs_f32())),
        player(PlayerMessage::SeekRelease),
      ]
    }
    Command::Play | Command::Pause | Command::Stop | Command::SeekTo(_) => Vec::new(),
  }
}

fn artist_of(track: &Track) -> String {
  riff_core::util::strip_youtube_topic(track.artist.as_deref().unwrap_or_default())
}

/// Titre en cours. `cover_file` : pochette déjà enregistrée sur disque pour ce titre.
#[must_use]
pub fn now_playing(app: &App, cover_file: Option<PathBuf>) -> NowPlaying {
  let Some(track) = &app.player.current else {
    return NowPlaying::default();
  };
  let listed = track
    .duration
    .filter(|seconds| *seconds > 0.0 && seconds.is_finite())
    .map(Duration::from_secs_f64);
  let duration = app.playback.duration.or(listed);
  NowPlaying {
    title: track.title.clone().unwrap_or_default(),
    artist: artist_of(track),
    album: track.album.clone().unwrap_or_default(),
    cover_file,
    duration,
    position: app.playback.position,
    playing: app.playback.is_playing,
  }
}

/// État diffusé à `riff lyrics` : titre, artiste et ligne courante.
#[must_use]
pub fn lyrics_state(app: &App) -> LyricsState {
  let Some(track) = &app.player.current else {
    return LyricsState::default();
  };
  let line = app
    .lyrics
    .lyrics
    .as_ref()
    .zip(app.lyrics.active)
    .and_then(|(lyrics, index)| lyrics.lines.get(index))
    .filter(|line| !line.is_gap)
    .map(|line| line.text.trim().to_string())
    .unwrap_or_default();
  LyricsState {
    title: track.title.clone().unwrap_or_default(),
    artist: artist_of(track),
    line,
    playing: app.playback.is_playing,
  }
}

/// Activité Discord, ou `None` pour l'effacer : seulement un titre en cours de lecture, comme `sendDiscordRpcUpdate`.
/// Seule une pochette `http(s)` est transmise (Discord la charge de son côté, comme dans l'original).
#[must_use]
pub fn discord_info(app: &App, unix_now: i64) -> Option<Value> {
  let track = app.player.current.as_ref()?;
  if !app.playback.is_playing {
    return None;
  }
  let thumbnail = [track.thumbnail.as_deref(), track.cover.as_deref()]
    .into_iter()
    .flatten()
    .find(|url| {
      (url.starts_with("http://") || url.starts_with("https://")) && !url.contains("127.0.0.1")
    })
    .unwrap_or("");
  let start = unix_now - i64::try_from(app.playback.position.as_secs()).unwrap_or(0);
  Some(json!({
    "title": track.title.as_deref().filter(|title| !title.is_empty()).unwrap_or("Unknown Track"),
    "artist": artist_of(track),
    "artistInStatus": app.settings.discord_artist_status,
    "duration": track.duration.unwrap_or(0.0),
    "thumbnail": thumbnail,
    "platform": track.platform.as_deref().unwrap_or(""),
    "startTimestamp": start,
  }))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::I18n;
  use crate::lyrics::model::{Line, Lyrics};
  use crate::settings::NativeSettings;
  use crate::window::PersistedWindowState;

  fn app() -> App {
    App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
  }

  fn track() -> Track {
    Track {
      id: Some("id".into()),
      title: Some("Song".into()),
      artist: Some("Band - Topic".into()),
      duration: Some(200.0),
      thumbnail: Some("https://img.example/c.jpg".into()),
      platform: Some("youtube".into()),
      ..Track::default()
    }
  }

  fn playing_app() -> App {
    let mut app = app();
    app.player.current = Some(track());
    app.playback.is_playing = true;
    app.playback.position = Duration::from_secs(30);
    app
  }

  fn is_player(message: &Message, wanted: &PlayerMessage) -> bool {
    matches!(message, Message::Player(got) if std::mem::discriminant(got) == std::mem::discriminant(wanted))
  }

  #[test]
  fn toggle_next_previous_map_to_the_player() {
    assert!(is_player(
      &messages_for(Command::Toggle, true, true)[0],
      &PlayerMessage::TogglePlay
    ));
    assert!(is_player(
      &messages_for(Command::Next, true, true)[0],
      &PlayerMessage::Next
    ));
    assert!(is_player(
      &messages_for(Command::Previous, true, true)[0],
      &PlayerMessage::Prev
    ));
  }

  #[test]
  fn play_and_pause_are_idempotent() {
    assert!(messages_for(Command::Play, true, true).is_empty());
    assert_eq!(messages_for(Command::Play, false, true).len(), 1);
    assert!(messages_for(Command::Pause, false, true).is_empty());
    assert_eq!(messages_for(Command::Pause, true, true).len(), 1);
  }

  #[test]
  fn play_without_a_track_does_nothing() {
    assert!(messages_for(Command::Play, false, false).is_empty());
  }

  #[test]
  fn stop_pauses_only_while_playing() {
    assert_eq!(messages_for(Command::Stop, true, true).len(), 1);
    assert!(messages_for(Command::Stop, false, true).is_empty());
  }

  #[test]
  fn volume_shortcuts_step_in_the_right_direction() {
    let up = messages_for(Command::VolumeUp, true, true);
    let down = messages_for(Command::VolumeDown, true, true);
    assert!(matches!(up[0], Message::Player(PlayerMessage::VolumeBy(delta)) if delta > 0.0));
    assert!(matches!(down[0], Message::Player(PlayerMessage::VolumeBy(delta)) if delta < 0.0));
  }

  #[test]
  fn seek_previews_then_releases_at_the_requested_second() {
    let messages = messages_for(Command::SeekTo(Duration::from_secs(42)), true, true);
    assert!(
      matches!(messages[0], Message::Player(PlayerMessage::SeekPreview(seconds)) if (seconds - 42.0).abs() < f32::EPSILON)
    );
    assert!(is_player(&messages[1], &PlayerMessage::SeekRelease));
    assert!(messages_for(Command::SeekTo(Duration::from_secs(1)), true, false).is_empty());
  }

  #[test]
  fn now_playing_is_empty_without_a_track() {
    assert!(now_playing(&app(), None).is_empty());
  }

  #[test]
  fn now_playing_copies_the_track_and_cleans_the_artist() {
    let now = now_playing(&playing_app(), Some(PathBuf::from("c.png")));
    assert_eq!((now.title.as_str(), now.artist.as_str()), ("Song", "Band"));
    assert_eq!(now.duration, Some(Duration::from_secs(200)));
    assert_eq!(now.position, Duration::from_secs(30));
    assert!(now.playing);
    assert_eq!(now.cover_file, Some(PathBuf::from("c.png")));
  }

  #[test]
  fn the_engine_duration_wins_over_the_listed_one() {
    let mut app = playing_app();
    app.playback.duration = Some(Duration::from_secs(180));
    assert_eq!(
      now_playing(&app, None).duration,
      Some(Duration::from_secs(180))
    );
  }

  #[test]
  fn lyrics_state_carries_the_active_line_only() {
    let mut app = playing_app();
    app.lyrics.lyrics = Some(Lyrics {
      lines: vec![
        Line {
          time: 0.0,
          text: "first".into(),
          ..Line::default()
        },
        Line {
          time: 5.0,
          text: "second ".into(),
          ..Line::default()
        },
        Line {
          time: 9.0,
          is_gap: true,
          ..Line::default()
        },
      ],
      is_custom: false,
    });
    app.lyrics.active = Some(1);
    let state = lyrics_state(&app);
    assert_eq!(
      (
        state.title.as_str(),
        state.artist.as_str(),
        state.line.as_str()
      ),
      ("Song", "Band", "second")
    );
    app.lyrics.active = Some(2);
    assert_eq!(lyrics_state(&app).line, "");
    app.lyrics.active = None;
    assert_eq!(lyrics_state(&app).line, "");
  }

  #[test]
  fn discord_activity_is_only_for_a_playing_track() {
    let mut app = playing_app();
    let info = discord_info(&app, 1_000_000).expect("activity");
    assert_eq!(info["title"], "Song");
    assert_eq!(info["artist"], "Band");
    assert_eq!(info["startTimestamp"], 999_970);
    assert_eq!(info["thumbnail"], "https://img.example/c.jpg");
    assert_eq!(info["artistInStatus"], true);
    app.playback.is_playing = false;
    assert!(discord_info(&app, 1_000_000).is_none());
    assert!(discord_info(&super::tests::app(), 0).is_none());
  }

  #[test]
  fn discord_never_gets_a_local_or_file_cover() {
    let mut app = playing_app();
    for url in [
      "http://127.0.0.1:1/x.jpg",
      "C:\\x\\c.jpg",
      "file:///c.jpg",
      "",
    ] {
      app.player.current.as_mut().unwrap().thumbnail = Some(url.into());
      assert_eq!(discord_info(&app, 0).unwrap()["thumbnail"], "", "{url}");
    }
  }

  #[test]
  fn discord_artist_status_follows_the_setting() {
    let mut app = playing_app();
    app.settings.discord_artist_status = false;
    assert_eq!(discord_info(&app, 0).unwrap()["artistInStatus"], false);
  }
}
