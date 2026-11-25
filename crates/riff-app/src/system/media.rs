//! Session système (SMTC sous Windows, MPRIS sous Linux, centre de commandes sous macOS) par `souvlaki`.
//! Une seule session. La détruire désactive les touches média ; la recréer les rétablit.

use super::now_playing::NowPlaying;
use super::{Command, SystemEvent};
use souvlaki::{
  MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
  SeekDirection,
};
use std::ffi::c_void;
use tokio::sync::mpsc::UnboundedSender;

const DISPLAY_NAME: &str = "Riff";
const DBUS_NAME: &str = "io.github.rootscripts.riff";

/// Événement de la session vers l'application. Les touches Avance et Retour de la session sautent de titre,
/// comme `seekforward` et `seekbackward` dans l'original (`app.js:1030`).
#[must_use]
pub fn event_of(event: &MediaControlEvent) -> Option<SystemEvent> {
  let command = match event {
    MediaControlEvent::Play => Command::Play,
    MediaControlEvent::Pause => Command::Pause,
    MediaControlEvent::Toggle => Command::Toggle,
    MediaControlEvent::Next | MediaControlEvent::Seek(SeekDirection::Forward) => Command::Next,
    MediaControlEvent::Previous | MediaControlEvent::Seek(SeekDirection::Backward) => {
      Command::Previous
    }
    MediaControlEvent::Stop => Command::Stop,
    MediaControlEvent::SetPosition(MediaPosition(position)) => Command::SeekTo(*position),
    MediaControlEvent::Raise => return Some(SystemEvent::Show(Vec::new())),
    MediaControlEvent::Quit => return Some(SystemEvent::Quit),
    _ => return None,
  };
  Some(SystemEvent::Command(command))
}

/// État de lecture de la session. Sans titre : arrêté.
#[must_use]
pub fn playback_of(now: &NowPlaying) -> MediaPlayback {
  if now.is_empty() {
    return MediaPlayback::Stopped;
  }
  let progress = Some(MediaPosition(now.position));
  if now.playing {
    MediaPlayback::Playing { progress }
  } else {
    MediaPlayback::Paused { progress }
  }
}

/// URL `file://` d'une pochette locale, la forme que `souvlaki` charge lui-même sous Windows.
#[must_use]
pub fn cover_url(path: &std::path::Path) -> String {
  let raw = path.to_string_lossy();
  // `StorageFile::GetFileFromPathAsync` refuse le préfixe `\\?\` que pose `fs::canonicalize` (0x800700A1).
  if let Some(share) = raw.strip_prefix(r"\\?\UNC\") {
    format!(r"file://\\{share}")
  } else {
    format!("file://{}", raw.strip_prefix(r"\\?\").unwrap_or(&raw))
  }
}

/// Session système ouverte.
pub struct Session {
  controls: MediaControls,
}

impl Session {
  /// Crée la session. `hwnd` : fenêtre principale, obligatoire sous Windows (`souvlaki` panique sans elle).
  pub fn create(
    hwnd: Option<*mut c_void>,
    events: UnboundedSender<SystemEvent>,
  ) -> Result<Self, String> {
    if cfg!(windows) && hwnd.is_none() {
      return Err("session média : fenêtre inconnue".to_string());
    }
    let mut controls = MediaControls::new(PlatformConfig {
      display_name: DISPLAY_NAME,
      dbus_name: DBUS_NAME,
      hwnd,
    })
    .map_err(|error| format!("{error:?}"))?;
    controls
      .attach(move |event| {
        if let Some(event) = event_of(&event) {
          let _ = events.send(event);
        }
      })
      .map_err(|error| format!("{error:?}"))?;
    Ok(Self { controls })
  }

  /// Pousse titre, durée, état et position.
  pub fn update(&mut self, now: &NowPlaying) {
    let cover = now.cover_file.as_deref().map(cover_url);
    let metadata = MediaMetadata {
      title: Some(&now.title),
      artist: Some(&now.artist),
      album: (!now.album.is_empty()).then_some(now.album.as_str()),
      cover_url: cover.as_deref(),
      duration: now.duration,
    };
    if let Err(error) = self.controls.set_metadata(metadata) {
      tracing::warn!(%error, "session média : métadonnées refusées");
    }
    if let Err(error) = self.controls.set_playback(playback_of(now)) {
      tracing::warn!(%error, "session média : état refusé");
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  #[test]
  fn buttons_map_to_commands() {
    let cmd = |e| event_of(&e);
    assert_eq!(
      cmd(MediaControlEvent::Play),
      Some(SystemEvent::Command(Command::Play))
    );
    assert_eq!(
      cmd(MediaControlEvent::Pause),
      Some(SystemEvent::Command(Command::Pause))
    );
    assert_eq!(
      cmd(MediaControlEvent::Toggle),
      Some(SystemEvent::Command(Command::Toggle))
    );
    assert_eq!(
      cmd(MediaControlEvent::Next),
      Some(SystemEvent::Command(Command::Next))
    );
    assert_eq!(
      cmd(MediaControlEvent::Previous),
      Some(SystemEvent::Command(Command::Previous))
    );
    assert_eq!(
      cmd(MediaControlEvent::Stop),
      Some(SystemEvent::Command(Command::Stop))
    );
  }

  #[test]
  fn fast_forward_and_rewind_skip_tracks_like_the_original() {
    assert_eq!(
      event_of(&MediaControlEvent::Seek(SeekDirection::Forward)),
      Some(SystemEvent::Command(Command::Next))
    );
    assert_eq!(
      event_of(&MediaControlEvent::Seek(SeekDirection::Backward)),
      Some(SystemEvent::Command(Command::Previous))
    );
  }

  #[test]
  fn position_request_becomes_a_seek() {
    let wanted = Duration::from_secs(42);
    assert_eq!(
      event_of(&MediaControlEvent::SetPosition(MediaPosition(wanted))),
      Some(SystemEvent::Command(Command::SeekTo(wanted)))
    );
  }

  #[test]
  fn raise_and_quit_reach_the_window_layer() {
    assert_eq!(
      event_of(&MediaControlEvent::Raise),
      Some(SystemEvent::Show(Vec::new()))
    );
    assert_eq!(event_of(&MediaControlEvent::Quit), Some(SystemEvent::Quit));
  }

  #[test]
  fn volume_and_uri_requests_are_ignored() {
    assert_eq!(event_of(&MediaControlEvent::SetVolume(0.5)), None);
    assert_eq!(
      event_of(&MediaControlEvent::OpenUri("https://x".into())),
      None
    );
  }

  #[test]
  fn playback_follows_the_player() {
    let mut now = NowPlaying {
      title: "T".into(),
      position: Duration::from_secs(3),
      playing: true,
      ..NowPlaying::default()
    };
    assert_eq!(
      playback_of(&now),
      MediaPlayback::Playing {
        progress: Some(MediaPosition(Duration::from_secs(3)))
      }
    );
    now.playing = false;
    assert_eq!(
      playback_of(&now),
      MediaPlayback::Paused {
        progress: Some(MediaPosition(Duration::from_secs(3)))
      }
    );
    assert_eq!(playback_of(&NowPlaying::default()), MediaPlayback::Stopped);
  }

  #[test]
  fn cover_is_a_file_url() {
    assert_eq!(
      cover_url(std::path::Path::new("C:\\a b\\c.jpg")),
      "file://C:\\a b\\c.jpg"
    );
  }

  #[test]
  fn cover_url_drops_the_verbatim_prefix_of_a_canonical_path() {
    assert_eq!(
      cover_url(std::path::Path::new(r"\\?\C:\a b\c.png")),
      r"file://C:\a b\c.png"
    );
    assert_eq!(
      cover_url(std::path::Path::new(r"\\?\UNC\srv\share\c.png")),
      r"file://\\srv\share\c.png"
    );
  }
}
