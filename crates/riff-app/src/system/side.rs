//! Côté système du `Program` : fil système, déduplication, envoi de l'état (session média, Discord, paroles), pochette.

use super::bridge;
use super::host::{Host, HostInit, HostRequest, Report, TrayLabels};
use super::now_playing::SyncGate;
use super::{Command, Dedup, platform, startup};
use crate::app::{App, Message};
use crate::images::Decoded;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// Ce que Discord a reçu en dernier : un changement de l'un de ces éléments renvoie l'activité.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscordKey {
  enabled: bool,
  artist_status: bool,
  /// `None` : aucune activité (rien en lecture).
  track: Option<(String, String)>,
}

/// Pochette du titre en cours, enregistrée sur disque pour la session média.
#[derive(Debug, Default)]
struct Cover {
  /// Adresse demandée pour le titre en cours.
  url: Option<String>,
  file: Option<PathBuf>,
  slot: u8,
}

pub struct SystemSide {
  host: Option<Host>,
  gate: SyncGate,
  dedup: Dedup,
  started: Instant,
  cover: Cover,
  discord: Option<DiscordKey>,
  media_wanted: Option<bool>,
  hwnd: Option<isize>,
  /// La fenêtre principale est masquée dans la zone de notification.
  pub main_hidden: bool,
  /// Quitter pour de bon : la fermeture ne masque plus la fenêtre.
  pub quitting: bool,
}

impl SystemSide {
  /// Démarre le fil système si cette instance sert le canal local. Sans canal (essais, seconde instance en mode
  /// multi-instance), aucune icône, aucune session, aucun raccourci : une seule instance les porte.
  #[must_use]
  pub fn start(app: &App) -> Self {
    let host = startup::sender().and_then(|events| {
      let init = HostInit {
        labels: labels(app),
        media_session: app.settings.media_session,
      };
      let report = Arc::new(Report::default());
      startup::set_report(Arc::clone(&report));
      platform::spawn(events, init, report)
        .inspect_err(|error| tracing::warn!(%error, "fil système indisponible"))
        .ok()
    });
    if host.is_some() {
      riff_core::discord::init(|task| {
        startup::runtime().spawn(task);
      });
    }
    Self {
      host,
      gate: SyncGate::default(),
      dedup: Dedup::default(),
      started: Instant::now(),
      cover: Cover::default(),
      discord: None,
      media_wanted: None,
      hwnd: None,
      main_hidden: false,
      quitting: false,
    }
  }

  /// Aucun fil système : tests, ou instance secondaire.
  #[must_use]
  pub fn inactive() -> Self {
    Self {
      host: None,
      gate: SyncGate::default(),
      dedup: Dedup::default(),
      started: Instant::now(),
      cover: Cover::default(),
      discord: None,
      media_wanted: None,
      hwnd: None,
      main_hidden: false,
      quitting: false,
    }
  }

  #[must_use]
  pub fn report(&self) -> Option<&Arc<Report>> {
    self.host.as_ref().map(Host::report)
  }

  /// La fermeture de la fenêtre doit la masquer au lieu de quitter.
  #[must_use]
  pub fn hides_on_close(&self, tray_on_close: bool) -> bool {
    tray_on_close
      && !self.quitting
      && self
        .report()
        .is_some_and(|report| report.tray_active.load(Ordering::Relaxed))
  }

  /// Fenêtre principale connue : la session média peut s'ouvrir sous Windows.
  pub fn main_window_handle(&mut self, hwnd: u64) {
    self.hwnd = isize::try_from(hwnd).ok();
    self.media_wanted = None;
  }

  /// Messages d'une commande du système, après déduplication.
  pub fn messages_for(&mut self, command: Command, app: &App, now: Instant) -> Vec<Message> {
    if !self.dedup.accept(command, now) {
      return Vec::new();
    }
    bridge::messages_for(
      command,
      app.playback.is_playing,
      app.player.current.is_some(),
    )
  }

  /// Pousse l'état courant au système. Rend l'adresse de la pochette à charger si le titre a changé.
  pub fn sync(&mut self, app: &App) -> Option<String> {
    let wanted = app.settings.media_session;
    if let Some(host) = &self.host
      && (self.media_wanted != Some(wanted))
    {
      self.media_wanted = Some(wanted);
      host.send(HostRequest::MediaSession {
        enabled: wanted,
        hwnd: self.hwnd,
      });
    }
    let cover_request = self.follow_cover(app);
    if let Some(host) = &self.host {
      let now = bridge::now_playing(app, self.cover.file.clone());
      if self.gate.should_send(&now, self.started.elapsed()) {
        host.send(HostRequest::NowPlaying(now));
      }
      self.sync_discord(app);
      if startup::serving() {
        startup::publish_lyrics(bridge::lyrics_state(app));
      }
    }
    cover_request
  }

  fn sync_discord(&mut self, app: &App) {
    let key = DiscordKey {
      enabled: app.settings.discord_rpc,
      artist_status: app.settings.discord_artist_status,
      track: app
        .player
        .current
        .as_ref()
        .filter(|_| app.playback.is_playing)
        .map(|track| {
          (
            track.title.clone().unwrap_or_default(),
            track.artist.clone().unwrap_or_default(),
          )
        }),
    };
    if self.discord.as_ref() == Some(&key) {
      return;
    }
    if self
      .discord
      .as_ref()
      .is_none_or(|previous| previous.enabled != key.enabled)
    {
      riff_core::discord::set_enabled(key.enabled);
    }
    let unix_now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
    match bridge::discord_info(app, unix_now) {
      Some(info) if key.enabled => riff_core::discord::update(info),
      _ => riff_core::discord::clear(),
    }
    self.discord = Some(key);
  }

  /// Nouveau titre : oublie l'ancienne pochette et rend l'adresse de la nouvelle.
  fn follow_cover(&mut self, app: &App) -> Option<String> {
    let url = app
      .player
      .current
      .as_ref()
      .and_then(crate::user_data::Track::thumb_url);
    if url == self.cover.url {
      return None;
    }
    self.cover.file = None;
    self.cover.url.clone_from(&url);
    url
  }

  /// Pochette chargée : enregistrée, puis envoyée avec le prochain état.
  pub fn cover_loaded(&mut self, url: &str, decoded: &Decoded) {
    if self.cover.url.as_deref() != Some(url) {
      return;
    }
    self.cover.slot ^= 1;
    match write_cover(&cover_dir(), self.cover.slot, decoded) {
      Ok(path) => self.cover.file = Some(path),
      Err(error) => tracing::warn!(%error, "pochette non enregistrée pour la session média"),
    }
  }

  pub fn set_labels(&self, labels: TrayLabels) {
    if let Some(host) = &self.host {
      host.send(HostRequest::Labels(labels));
    }
  }

  /// Dernier état envoyé au système, pour les essais.
  #[must_use]
  pub fn cover_file(&self) -> Option<&Path> {
    self.cover.file.as_deref()
  }

  pub fn send(&self, request: HostRequest) {
    if let Some(host) = &self.host {
      host.send(request);
    }
  }
}

fn labels(app: &App) -> TrayLabels {
  TrayLabels {
    show_hide: app.i18n.t("Show/Hide"),
    play_pause: app.i18n.t("Play/Pause"),
    quit: app.i18n.t("Quit"),
  }
}

/// Libellés du menu de la zone de notification dans la langue de l'interface.
#[must_use]
pub fn tray_labels(app: &App) -> TrayLabels {
  labels(app)
}

fn cover_dir() -> PathBuf {
  riff_core::paths::thumbnails_dir()
}

/// Écrit la pochette en PNG. Deux fichiers en alternance : le système peut encore lire le précédent.
pub fn write_cover(dir: &Path, slot: u8, decoded: &Decoded) -> io::Result<PathBuf> {
  std::fs::create_dir_all(dir)?;
  let image = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba.clone())
    .ok_or_else(|| io::Error::other("image buffer size mismatch"))?;
  let path = dir.join(format!("now-playing-{slot}.png"));
  image
    .save_with_format(&path, image::ImageFormat::Png)
    .map_err(io::Error::other)?;
  Ok(path)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::window::PersistedWindowState;

  fn app() -> App {
    App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
  }

  #[test]
  fn an_inactive_side_does_not_hide_on_close() {
    let side = SystemSide::inactive();
    assert!(!side.hides_on_close(true));
    assert!(side.report().is_none());
  }

  #[test]
  fn an_inactive_side_syncs_without_touching_the_system() {
    let mut side = SystemSide::inactive();
    assert_eq!(side.sync(&app()), None);
  }

  #[test]
  fn commands_are_deduplicated_before_becoming_messages() {
    let mut side = SystemSide::inactive();
    let app = app();
    let t0 = Instant::now();
    assert_eq!(side.messages_for(Command::Toggle, &app, t0).len(), 1);
    assert!(
      side
        .messages_for(
          Command::Toggle,
          &app,
          t0 + std::time::Duration::from_millis(100)
        )
        .is_empty()
    );
  }

  #[test]
  fn a_new_track_asks_for_its_cover_once() {
    let mut side = SystemSide::inactive();
    let mut app = app();
    app.player.current = Some(crate::user_data::Track {
      thumbnail: Some("https://img/a.jpg".into()),
      ..Default::default()
    });
    assert_eq!(
      side.follow_cover(&app),
      Some("https://img/a.jpg".to_string())
    );
    assert_eq!(side.follow_cover(&app), None);
    app.player.current = Some(crate::user_data::Track {
      thumbnail: Some("https://img/b.jpg".into()),
      ..Default::default()
    });
    assert_eq!(
      side.follow_cover(&app),
      Some("https://img/b.jpg".to_string())
    );
  }

  #[test]
  fn a_cover_for_another_track_is_ignored() {
    let mut side = SystemSide::inactive();
    side.cover.url = Some("https://img/new.jpg".into());
    let decoded = Decoded {
      width: 1,
      height: 1,
      rgba: vec![0, 0, 0, 255],
    };
    side.cover_loaded("https://img/old.jpg", &decoded);
    assert!(side.cover_file().is_none());
  }

  #[test]
  fn write_cover_alternates_files_and_writes_a_png() {
    let dir = crate::test_temp::TempDir::new("cover-test");
    let decoded = Decoded {
      width: 2,
      height: 2,
      rgba: vec![200; 16],
    };
    let first = write_cover(&dir, 0, &decoded).expect("write");
    let second = write_cover(&dir, 1, &decoded).expect("write");
    assert_ne!(first, second);
    let bytes = std::fs::read(&first).expect("read");
    assert_eq!(&bytes[..4], b"\x89PNG");
  }

  #[test]
  fn write_cover_rejects_a_buffer_of_the_wrong_size() {
    let dir = crate::test_temp::TempDir::new("cover-bad");
    let decoded = Decoded {
      width: 2,
      height: 2,
      rgba: vec![0; 3],
    };
    assert!(write_cover(&dir, 0, &decoded).is_err());
  }
}
