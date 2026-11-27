//! Côté système du `Program` : fil système, déduplication, envoi de l'état (session média, Discord, paroles), pochette.

use super::host::{Host, HostInit, HostRequest, Report, TrayLabels};
use super::now_playing::SyncGate;
use super::wallpaper::{WallpaperController, WallpaperTrack};
use super::{Command, Dedup, platform, startup};
use super::{autostart, bridge};
use crate::app::{App, Message};
use crate::backend::Backend;
use crate::images::Decoded;
use crate::settings::{DiscordStatus, DiscordTime};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Écart, en millisecondes, entre la position réelle et la position attendue à partir duquel l'activité est
/// renvoyée : saut de lecture, attente du flux. Moins que cela : retard de la position entre deux `Event::Position`
/// (`POSITION_PERIOD`, 250 ms), pas un saut.
const DISCORD_DRIFT_MS: f64 = 2000.0;

/// Ce que Discord a reçu en dernier : un changement de l'un de ces éléments renvoie l'activité.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscordKey {
  enabled: bool,
  status: DiscordStatus,
  time: DiscordTime,
  cover: bool,
  paused_shown: bool,
  album_shown: bool,
  /// Vitesse de lecture au millième : un changement renvoie `start` et la durée.
  rate_milli: u32,
  /// `None` : aucune activité (rien en lecture, ou en pause sans « Show when paused »).
  track: Option<DiscordTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscordTrack {
  title: String,
  artist: String,
  album: String,
  /// Image envoyée : une pochette publique qui arrive après le premier envoi la remplace.
  cover: String,
  /// Libellé de pause traduit, `None` en lecture : un changement de langue renvoie la carte en pause.
  paused: Option<String>,
}

/// Position attendue depuis le dernier envoi, quand un minutage est affiché. Elle avance au rythme de la lecture
/// (vitesse comprise) : seule une discontinuité renvoie l'activité, pas la vitesse ni le retard de la position.
#[derive(Debug, Clone, Copy)]
struct DiscordClock {
  /// Position attendue à `at_ms`, en millisecondes.
  expected_ms: f64,
  /// Heure Unix de la dernière synchronisation, en millisecondes.
  at_ms: i64,
}

/// Dernier envoi à Discord : l'état, et la position suivie quand un minutage est affiché.
#[derive(Debug)]
struct DiscordSent {
  key: DiscordKey,
  clock: Option<DiscordClock>,
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
  wallpaper_allowed: bool,
  wallpaper: WallpaperController,
  gate: SyncGate,
  dedup: Dedup,
  started: Instant,
  cover: Cover,
  discord: Option<DiscordSent>,
  media_wanted: Option<bool>,
  /// Dernier état de la case « Always on top » envoyé au menu.
  always_on_top_sent: bool,
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
        always_on_top: app.settings.main_always_on_top,
        autostart: autostart::current_state(),
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
      wallpaper_allowed: host.is_some(),
      wallpaper: WallpaperController::new(),
      host,
      gate: SyncGate::default(),
      dedup: Dedup::default(),
      started: Instant::now(),
      cover: Cover::default(),
      discord: None,
      media_wanted: None,
      always_on_top_sent: app.settings.main_always_on_top,
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
      wallpaper_allowed: false,
      wallpaper: WallpaperController::new(),
      gate: SyncGate::default(),
      dedup: Dedup::default(),
      started: Instant::now(),
      cover: Cover::default(),
      discord: None,
      media_wanted: None,
      always_on_top_sent: false,
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

  /// Le fil système crée une zone de notification : une fenêtre cachée au lancement pourra y être rappelée.
  #[must_use]
  pub fn expects_tray(&self) -> bool {
    cfg!(any(windows, target_os = "linux")) && self.host.is_some()
  }

  /// Bascule « Start with Windows » et remet la case du menu d'aplomb d'après le registre.
  pub fn toggle_autostart(&self) {
    if let Some(host) = &self.host {
      host.send(HostRequest::Autostart(autostart::toggle()));
    }
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
    if let Some(host) = &self.host
      && self.always_on_top_sent != app.settings.main_always_on_top
    {
      self.always_on_top_sent = app.settings.main_always_on_top;
      host.send(HostRequest::AlwaysOnTop(self.always_on_top_sent));
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
    let unix_ms = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(0));
    self.push_discord(app, unix_ms);
  }

  /// Renvoie l'activité Discord si elle a changé ; `true` si un message est parti vers la tâche Discord.
  /// Seule la position réelle compte : un glissement de la barre (`SeekPreview`) ne la change pas. Pendant une
  /// résolution ou une attente du flux, la position est figée : l'écart n'est mesuré qu'une fois la lecture repartie.
  fn push_discord(&mut self, app: &App, unix_ms: i64) -> bool {
    let settings = &app.settings;
    let paused = bridge::discord_paused_label(app);
    let key = DiscordKey {
      enabled: settings.discord_rpc,
      status: settings.discord_status,
      time: settings.discord_time,
      cover: settings.discord_cover,
      paused_shown: settings.discord_paused,
      album_shown: settings.discord_album,
      rate_milli: (bridge::discord_rate(app) * 1000.0).round() as u32,
      track: app
        .player
        .current
        .as_ref()
        .filter(|_| paused.is_none() || settings.discord_paused)
        .map(|track| DiscordTrack {
          title: track.title.clone().unwrap_or_default(),
          artist: track.artist.clone().unwrap_or_default(),
          album: track.album.clone().unwrap_or_default(),
          cover: bridge::discord_cover(track, settings).to_owned(),
          paused: paused.clone(),
        }),
    };
    // Présence coupée, en pause ou sans minutage affiché, `start` n'est pas envoyé : il ne compte pas.
    let timed =
      key.enabled && paused.is_none() && key.track.is_some() && key.time != DiscordTime::Hidden;
    let position_ms = app.playback.position.as_secs_f64() * 1000.0;
    if let Some(sent) = &mut self.discord
      && sent.key == key
    {
      match (&mut sent.clock, timed) {
        (None, false) => return false,
        (Some(clock), true) => {
          let speed = bridge::discord_rate(app);
          clock.expected_ms += (unix_ms - clock.at_ms) as f64 * speed;
          clock.at_ms = unix_ms;
          let waiting = app.player.loading || app.playback.buffering;
          if waiting || (position_ms - clock.expected_ms).abs() < DISCORD_DRIFT_MS {
            return false;
          }
        }
        _ => {}
      }
    }
    if self
      .discord
      .as_ref()
      .is_none_or(|previous| previous.key.enabled != key.enabled)
    {
      riff_core::discord::set_enabled(key.enabled);
    }
    match bridge::discord_info(app, unix_ms) {
      Some(info) if key.enabled => riff_core::discord::update(info),
      _ => riff_core::discord::clear(),
    }
    let clock = timed.then_some(DiscordClock {
      expected_ms: position_ms,
      at_ms: unix_ms,
    });
    self.discord = Some(DiscordSent { key, clock });
    true
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

  /// Pushes current playback state and configuration to the Windows wallpaper sync worker.
  pub fn sync_wallpaper(&mut self, app: &App, backend: &Arc<Backend>) {
    if !self.wallpaper_allowed {
      return;
    }
    if !app.settings.wallpaper_sync.enabled {
      self.wallpaper.disable();
      return;
    }
    let current = app.player.current.as_ref().and_then(|track| {
      let cover_url = track.thumb_url().filter(|u| !u.is_empty())?;
      let duration = app.playback.duration.or_else(|| {
        track
          .duration
          .filter(|d| *d > 0.0 && d.is_finite())
          .map(Duration::from_secs_f64)
      });
      let remaining_secs = duration.map(|d| d.saturating_sub(app.playback.position).as_secs_f64());
      Some(WallpaperTrack {
        id: track.id.clone().unwrap_or_else(|| cover_url.clone()),
        title: track.title.clone().unwrap_or_default(),
        artist: track.artist.clone().unwrap_or_default(),
        cover_url,
        remaining_secs,
        paused: !app.playback.is_playing,
      })
    });

    let next = if !app.player.shuffle && !app.player.repeat {
      app.player.index.and_then(|idx| {
        app
          .player
          .queue
          .iter()
          .skip(idx + 1)
          .find(|track| !app.data.is_blocked(track))
          .and_then(|track| {
            let cover_url = track.thumb_url().filter(|u| !u.is_empty())?;
            Some(WallpaperTrack {
              id: track.id.clone().unwrap_or_else(|| cover_url.clone()),
              title: track.title.clone().unwrap_or_default(),
              artist: track.artist.clone().unwrap_or_default(),
              cover_url,
              remaining_secs: None,
              paused: false,
            })
          })
      })
    } else {
      None
    };

    self
      .wallpaper
      .sync(&app.settings.wallpaper_sync, current, next, backend);
  }

  pub fn request_stop_wallpaper(&mut self) {
    self.wallpaper.request_stop();
  }

  pub fn stop_wallpaper(&mut self) {
    self.wallpaper.stop();
  }
}

impl Drop for SystemSide {
  fn drop(&mut self) {
    self.wallpaper.stop();
  }
}

fn labels(app: &App) -> TrayLabels {
  TrayLabels {
    show_hide: app.i18n.t("Show/Hide"),
    play_pause: app.i18n.t("Play/Pause"),
    always_on_top: app.i18n.t("Always on top"),
    start_with_windows: app.i18n.t("Start with Windows"),
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

  fn playing_app() -> App {
    let mut app = app();
    app.player.current = Some(crate::user_data::Track {
      title: Some("Song".into()),
      artist: Some("Band".into()),
      duration: Some(200.0),
      ..Default::default()
    });
    app.playback.is_playing = true;
    app.playback.position = std::time::Duration::from_secs(30);
    app
  }

  #[test]
  fn discord_is_not_resent_while_the_track_plays_on() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    assert!(side.push_discord(&app, 1_000_000));
    assert!(!side.push_discord(&app, 1_000_000));
    assert!(
      !side.push_discord(&app, 1_001_000),
      "position figée : moins de 2 s"
    );
    app.playback.position = std::time::Duration::from_secs(40);
    assert!(!side.push_discord(&app, 1_010_000), "lecture normale");
  }

  /// Position telle que `Event::Position` la publie : un tick toutes les 250 ms, lue en retard entre deux ticks.
  fn at(app: &mut App, seconds: f64) {
    app.playback.position = std::time::Duration::from_secs_f64(seconds);
  }

  #[test]
  fn a_stale_position_is_not_taken_for_a_seek() {
    // Début réel à 1000,9 s : la position lue a jusqu'à 250 ms de retard, l'arrondi des secondes donnait 1000 puis 1002.
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    at(&mut app, 1000.05);
    assert!(side.push_discord(&app, 2_000_950));
    at(&mut app, 1008.95);
    assert!(
      !side.push_discord(&app, 2_010_050),
      "survol 200 ms après un tick"
    );
    at(&mut app, 1011.05);
    assert!(!side.push_discord(&app, 2_011_950), "tick suivant");
  }

  #[test]
  fn discord_is_not_resent_at_another_speed() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    app.settings.sound.audio.speed = 2.0;
    assert!(side.push_discord(&app, 1_000_000));
    for tick in 1..=40 {
      at(&mut app, 30.0 + f64::from(tick) * 0.5);
      assert!(
        !side.push_discord(&app, 1_000_000 + i64::from(tick) * 250),
        "{tick}"
      );
    }
    at(&mut app, 90.0);
    assert!(side.push_discord(&app, 1_010_250), "saut à vitesse 2");
  }

  #[test]
  fn discord_follows_the_jam_speed_not_the_personal_one() {
    use crate::jam::protocol::{JamSound, SharedSound};
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    let mut jam = crate::jam::state::JamState::new(crate::jam::Role::Guest, "moi");
    jam.shared = Some(SharedSound {
      sv: 1,
      sound: JamSound {
        speed: 2.0,
        ..JamSound::default()
      },
      ..SharedSound::default()
    });
    app.jam = Some(Box::new(jam));
    assert_eq!(app.settings.sound.audio.speed, 1.0);
    assert_eq!(bridge::discord_rate(&app), 2.0);
    assert!(side.push_discord(&app, 1_000_000));
    for tick in 1..=40 {
      at(&mut app, 30.0 + f64::from(tick) * 0.5);
      assert!(
        !side.push_discord(&app, 1_000_000 + i64::from(tick) * 250),
        "{tick}"
      );
    }
    let shared = app.jam.as_mut().and_then(|jam| jam.shared.as_mut());
    let shared = shared.expect("effets partagés");
    shared.sv = 2;
    shared.sound.speed = 1.0;
    assert!(
      side.push_discord(&app, 1_010_000),
      "la vitesse du jam change"
    );
    assert!(!side.push_discord(&app, 1_010_000));
    app.jam = None;
    app.settings.sound.audio.speed = 1.5;
    assert!(side.push_discord(&app, 1_010_000), "fin du jam");
  }

  #[test]
  fn a_speed_drag_resends_discord_only_on_release() {
    use crate::app::Message;
    use crate::audio_prefs::{AudioMessage, SoundField};
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    assert!(side.push_discord(&app, 1_000_000));
    for step in 1..=20_i16 {
      app.update(Message::AudioPrefs(AudioMessage::Set(
        SoundField::Speed,
        1.0 + f32::from(step) * 0.05,
      )));
      assert!(!side.push_discord(&app, 1_000_000), "pas {step}");
    }
    app.update(Message::AudioPrefs(AudioMessage::Commit));
    assert!(side.push_discord(&app, 1_000_000), "relâché à vitesse 2");
    assert!(!side.push_discord(&app, 1_000_000));
  }

  #[test]
  fn a_smooth_preset_resends_discord_once_when_it_ends() {
    use crate::app::Message;
    use crate::audio_prefs::AudioMessage;
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    assert!(side.push_discord(&app, 1_000_000));
    app.update(Message::AudioPrefs(AudioMessage::ApplyPreset(
      "p_slowed".to_string(),
    )));
    let mut resent = 0;
    while app.ui.audio.tween.is_some() {
      app.update(Message::AudioPrefs(AudioMessage::TweenTick));
      resent += usize::from(side.push_discord(&app, 1_000_000));
    }
    assert_eq!(resent, 1);
    assert!(app.settings.sound.audio.speed_pitch < 1.0);
  }

  #[test]
  fn a_speed_change_resends_discord_once() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    assert!(side.push_discord(&app, 1_000_000));
    app.settings.sound.audio.speed = 2.0;
    assert!(side.push_discord(&app, 1_000_000), "vitesse 2");
    assert!(!side.push_discord(&app, 1_000_000));
    app.settings.sound.audio.speed = 2.0004;
    assert!(!side.push_discord(&app, 1_000_000), "sous le millième");
    app.settings.sound.audio.speed = 0.82;
    assert!(side.push_discord(&app, 1_000_000), "vitesse 0,82");
  }

  #[test]
  fn a_stall_resends_discord_once_playback_goes_on() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    assert!(side.push_discord(&app, 1_000_000));
    app.playback.buffering = true;
    for tick in 1..=20 {
      assert!(
        !side.push_discord(&app, 1_000_000 + i64::from(tick) * 250),
        "{tick}"
      );
    }
    app.playback.buffering = false;
    assert!(side.push_discord(&app, 1_005_250), "minutage recalé");
    at(&mut app, 30.25);
    assert!(!side.push_discord(&app, 1_005_500));
  }

  #[test]
  fn the_next_track_resolving_is_not_a_pause() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    app.settings.discord_paused = true;
    assert!(side.push_discord(&app, 1_000_000));
    app.player.current.as_mut().unwrap().title = Some("Song 2".into());
    app.playback.is_playing = false;
    app.player.loading = true;
    at(&mut app, 0.0);
    assert!(side.push_discord(&app, 1_000_000), "nouveau titre");
    assert!(!side.push_discord(&app, 1_001_000));
    app.player.loading = false;
    app.playback.is_playing = true;
    assert!(
      !side.push_discord(&app, 1_001_500),
      "chargé en moins de 2 s"
    );
  }

  #[test]
  fn a_late_public_cover_resends_discord() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    app.settings.discord_time = DiscordTime::Hidden;
    assert!(side.push_discord(&app, 1_000_000));
    app.player.current.as_mut().unwrap().thumbnail = Some("https://img/a.jpg".into());
    assert!(side.push_discord(&app, 1_000_000));
    assert!(!side.push_discord(&app, 1_000_000));
  }

  #[test]
  fn a_language_change_resends_the_paused_card() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    app.settings.discord_paused = true;
    app.playback.is_playing = false;
    assert!(side.push_discord(&app, 1_000_000));
    app.i18n = Arc::new(I18n::load("fr", Path::new("absent")));
    assert!(side.push_discord(&app, 1_000_000), "« En pause »");
    assert!(!side.push_discord(&app, 1_000_000));
  }

  #[test]
  fn a_seek_resends_discord_but_dragging_the_bar_does_not() {
    use crate::ui::PlayerMessage;
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    assert!(side.push_discord(&app, 1_000_000));
    let _ = app.update(Message::Player(PlayerMessage::SeekPreview(120.0)));
    assert!(
      !side.push_discord(&app, 1_000_000),
      "aperçu : position inchangée"
    );
    let _ = app.update(Message::Player(PlayerMessage::SeekRelease));
    assert_eq!(app.playback.position, std::time::Duration::from_secs(120));
    assert!(side.push_discord(&app, 1_000_000));
    app.playback.position = std::time::Duration::from_secs(118);
    assert!(side.push_discord(&app, 1_000_000), "2 s en arrière");
  }

  #[test]
  fn a_pause_resends_discord_and_the_paused_card_stays_still() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    assert!(side.push_discord(&app, 1_000_000));
    app.playback.is_playing = false;
    assert!(side.push_discord(&app, 1_005_000), "effacée en pause");
    assert!(!side.push_discord(&app, 1_100_000));
    app.settings.discord_paused = true;
    assert!(side.push_discord(&app, 1_100_000), "carte en pause");
    assert!(
      !side.push_discord(&app, 1_200_000),
      "aucun minutage en pause"
    );
    app.playback.is_playing = true;
    assert!(side.push_discord(&app, 1_200_000), "reprise");
  }

  #[test]
  fn every_discord_setting_and_the_album_resend_the_activity() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    assert!(side.push_discord(&app, 1_000_000));
    let changes: [fn(&mut App); 7] = [
      |app| app.settings.discord_status = DiscordStatus::Track,
      |app| app.settings.discord_time = DiscordTime::Elapsed,
      |app| app.settings.discord_cover = false,
      |app| app.settings.discord_paused = true,
      |app| app.settings.discord_album = false,
      |app| app.player.current.as_mut().unwrap().album = Some("Record".into()),
      |app| app.settings.discord_rpc = false,
    ];
    for change in changes {
      change(&mut app);
      assert!(side.push_discord(&app, 1_000_000));
      assert!(!side.push_discord(&app, 1_000_000));
    }
  }

  #[test]
  fn the_hidden_time_ignores_the_position() {
    let mut side = SystemSide::inactive();
    let mut app = playing_app();
    app.settings.discord_time = DiscordTime::Hidden;
    assert!(side.push_discord(&app, 1_000_000));
    app.playback.position = std::time::Duration::from_secs(150);
    assert!(!side.push_discord(&app, 1_000_000));
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
