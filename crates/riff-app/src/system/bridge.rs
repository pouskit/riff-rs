//! Pont entre l'état de l'application et le système : commandes reçues, état envoyé (session média, Discord, paroles).

use super::Command;
use super::now_playing::NowPlaying;
use super::protocol::LyricsState;
use crate::app::{App, Message};
use crate::settings::NativeSettings;
use crate::ui::PlayerMessage;
use crate::user_data::Track;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// Icône publique de l'application Discord de Riff (`1520100510877749358`), pour un titre sans pochette publique.
/// Discord la charge depuis son CDN, riff-rs ne la demande jamais (hors de `riff_core::net::allowlist`).
/// L'application n'a aucun asset ; l'adresse casse si les auteurs de Riff changent l'icône.
const DISCORD_FALLBACK_COVER: &str = "https://cdn.discordapp.com/app-icons/1520100510877749358/b1a6475151bda2cd50daec6650febffa.png?size=512";

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

/// `start` de l'activité Discord, en secondes arrondies : instant Unix où la lecture aurait commencé sans pause ni
/// saut. `unix_ms` : heure Unix en millisecondes.
#[must_use]
pub fn discord_start(app: &App, unix_ms: i64) -> i64 {
  let position = (app.playback.position.as_secs_f64() * 1000.0 / discord_rate(app)).round() as i64;
  (unix_ms - position + 500).div_euclid(1000)
}

/// Vitesse de lecture réelle (`speed_factor` du moteur, pitch compris) : la position de la piste avance de `rate` s par
/// seconde réelle. Toujours finie et strictement positive. Lue sur le réglage enregistré (`settings.sound.audio`), pas
/// sur `ui.audio.live` : celui-ci change à chaque pas d'un curseur ou d'une transition de préréglage, l'autre une fois
/// au relâchement ou à la fin de la transition. Pendant un jam, `settings.sound.audio` garde le son personnel : la vitesse
/// est lue sur les effets partagés (`jam.shared`), ceux que joue le moteur.
#[must_use]
pub fn discord_rate(app: &App) -> f64 {
  if let Some(shared) = app.jam.as_ref().and_then(|jam| jam.shared.as_ref()) {
    return shared.sound.rate();
  }
  let rate = app.settings.sound.audio.to_engine().tempo().speed_factor();
  if rate.is_finite() && rate > 0.0 {
    rate
  } else {
    1.0
  }
}

/// Libellé de pause de l'activité Discord dans la langue de l'interface, `None` en lecture. Un titre en cours de
/// résolution compte comme lu : `Loaded` le lance, ce n'est pas une pause.
#[must_use]
pub fn discord_paused_label(app: &App) -> Option<String> {
  (!app.playback.is_playing && !app.player.loading).then(|| app.i18n.t("Paused"))
}

/// Image de l'activité Discord. Seule une pochette `http(s)` est transmise (Discord la charge de son côté, comme dans
/// l'original) ; sans elle, l'icône de Riff sur le CDN de Discord. « Discord cover » coupé : aucune image.
#[must_use]
pub fn discord_cover<'a>(track: &'a Track, settings: &NativeSettings) -> &'a str {
  if !settings.discord_cover {
    return "";
  }
  [track.thumbnail.as_deref(), track.cover.as_deref()]
    .into_iter()
    .flatten()
    .find(|url| {
      (url.starts_with("http://") || url.starts_with("https://")) && !url.contains("127.0.0.1")
    })
    .unwrap_or(DISCORD_FALLBACK_COVER)
}

/// Activité Discord, ou `None` pour l'effacer : un titre en lecture, comme `sendDiscordRpcUpdate`, ou en pause si
/// « Show when paused » est actif. `unix_ms` : heure Unix en millisecondes.
#[must_use]
pub fn discord_info(app: &App, unix_ms: i64) -> Option<Value> {
  let track = app.player.current.as_ref()?;
  let settings = &app.settings;
  let paused_label = discord_paused_label(app);
  if paused_label.is_some() && !settings.discord_paused {
    return None;
  }
  let album = track
    .album
    .as_deref()
    .filter(|_| settings.discord_album)
    .unwrap_or("");
  let mut info = json!({
    "title": track.title.as_deref().filter(|title| !title.is_empty()).unwrap_or("Unknown Track"),
    "artist": artist_of(track),
    "album": album,
    "statusDisplay": settings.discord_status.key(),
    "timeDisplay": settings.discord_time.key(),
    "paused": paused_label.is_some(),
    "duration": track.duration.unwrap_or(0.0) / discord_rate(app),
    "repoUrl": format!("https://github.com/{}", super::updates::REPOSITORY),
    "thumbnail": discord_cover(track, settings),
    "platform": track.platform.as_deref().unwrap_or(""),
    "startTimestamp": discord_start(app, unix_ms),
  });
  if let Some(label) = paused_label {
    info["pausedLabel"] = json!(label);
  }
  Some(info)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::I18n;
  use crate::lyrics::model::{Line, Lyrics};
  use crate::settings::{DiscordStatus, DiscordTime, NativeSettings};
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
    let info = discord_info(&app, 1_000_000_000).expect("activity");
    assert_eq!(info["title"], "Song");
    assert_eq!(info["artist"], "Band");
    assert_eq!(info["startTimestamp"], 999_970);
    assert_eq!(info["thumbnail"], "https://img.example/c.jpg");
    assert_eq!(info["statusDisplay"], "artist");
    assert_eq!(info["timeDisplay"], "progress");
    assert_eq!(info["paused"], false);
    assert!(info.get("pausedLabel").is_none());
    app.playback.is_playing = false;
    assert!(discord_info(&app, 1_000_000_000).is_none());
    assert!(discord_info(&super::tests::app(), 0).is_none());
  }

  #[test]
  fn the_discord_start_is_rounded_from_milliseconds() {
    let mut app = playing_app();
    app.playback.position = Duration::from_millis(30_400);
    assert_eq!(discord_start(&app, 1_000_000_000), 999_970);
    app.playback.position = Duration::from_millis(30_600);
    assert_eq!(discord_start(&app, 1_000_000_000), 999_969);
  }

  #[test]
  fn discord_gets_the_repository_url_not_the_profile() {
    let info = discord_info(&playing_app(), 0).unwrap();
    assert_eq!(info["repoUrl"], "https://github.com/pouskit/riff-rs");
  }

  #[test]
  fn discord_start_and_duration_follow_the_playback_rate() {
    let mut app = playing_app();
    app.settings.sound.audio.speed = 2.0;
    let info = discord_info(&app, 1_000_000_000).unwrap();
    assert_eq!(
      info["startTimestamp"], 999_985,
      "30 s de lecture = 15 s réelles"
    );
    assert_eq!(info["duration"], 100.0);
    app.settings.sound.audio.speed = 0.82;
    let info = discord_info(&app, 1_000_000_000).unwrap();
    assert_eq!(info["startTimestamp"], 999_963);
    assert!((info["duration"].as_f64().unwrap() - 200.0 / 0.82).abs() < 1e-3);
  }

  #[test]
  fn the_pitch_changes_the_discord_rate_as_the_engine_does() {
    let mut app = playing_app();
    app.settings.sound.audio.speed = 2.0;
    app.settings.sound.audio.speed_pitch = 1.5;
    assert!(
      (discord_rate(&app) - 1.5).abs() < 1e-6,
      "speed_pitch, pas speed x pitch"
    );
    app.settings.sound.audio.speed_pitch = 1.0;
    app.settings.sound.audio.speed = 1.0;
    assert_eq!(discord_rate(&app), 1.0);
  }

  #[test]
  fn a_track_being_resolved_is_not_shown_as_paused() {
    let mut app = playing_app();
    app.settings.discord_paused = true;
    app.playback.is_playing = false;
    app.playback.position = Duration::ZERO;
    app.player.loading = true;
    let info = discord_info(&app, 1_000_000_000).expect("activity");
    assert_eq!(info["paused"], false);
    assert!(info.get("pausedLabel").is_none());
    app.settings.discord_paused = false;
    assert!(discord_info(&app, 1_000_000_000).is_some());
  }

  #[test]
  fn discord_keeps_a_paused_track_only_when_asked() {
    let mut app = playing_app();
    app.playback.is_playing = false;
    app.settings.discord_paused = true;
    let info = discord_info(&app, 1_000_000_000).expect("activity");
    assert_eq!(info["paused"], true);
    assert_eq!(info["pausedLabel"], "Paused");
    assert_eq!(info["title"], "Song");
    app.i18n = std::sync::Arc::new(I18n::load("fr", std::path::Path::new("absent")));
    assert_eq!(discord_info(&app, 0).unwrap()["pausedLabel"], "En pause");
  }

  #[test]
  fn discord_never_gets_a_local_or_file_cover_but_the_riff_icon() {
    let mut app = playing_app();
    for url in [
      "http://127.0.0.1:1/x.jpg",
      "C:\\x\\c.jpg",
      "file:///c.jpg",
      "",
    ] {
      app.player.current.as_mut().unwrap().thumbnail = Some(url.into());
      assert_eq!(
        discord_info(&app, 0).unwrap()["thumbnail"],
        DISCORD_FALLBACK_COVER,
        "{url}"
      );
    }
  }

  #[test]
  fn discord_gets_no_image_with_the_cover_switch_off() {
    let mut app = playing_app();
    app.settings.discord_cover = false;
    assert_eq!(discord_info(&app, 0).unwrap()["thumbnail"], "");
    app.player.current.as_mut().unwrap().thumbnail = None;
    assert_eq!(discord_info(&app, 0).unwrap()["thumbnail"], "");
  }

  #[test]
  fn discord_album_and_modes_follow_the_settings() {
    let mut app = playing_app();
    app.player.current.as_mut().unwrap().album = Some("Record".into());
    assert_eq!(discord_info(&app, 0).unwrap()["album"], "Record");
    app.settings.discord_album = false;
    app.settings.discord_status = DiscordStatus::TrackArtist;
    app.settings.discord_time = DiscordTime::Elapsed;
    let info = discord_info(&app, 0).unwrap();
    assert_eq!(info["album"], "");
    assert_eq!(info["statusDisplay"], "track-artist");
    assert_eq!(info["timeDisplay"], "elapsed");
  }
}
