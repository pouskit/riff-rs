//! Fil système : zone de notification, raccourcis globaux et session média vivent sur un fil à boucle de messages,
//! à l'écart de la boucle d'`iced`. Les événements remontent par `SystemEvent`, les demandes descendent par `HostRequest`.

use super::SystemEvent;
use super::autostart;
use super::now_playing::NowPlaying;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tokio::sync::mpsc::UnboundedSender;

/// Libellés du menu de la zone de notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayLabels {
  pub show_hide: String,
  pub play_pause: String,
  pub always_on_top: String,
  pub start_with_windows: String,
  pub quit: String,
}

impl Default for TrayLabels {
  fn default() -> Self {
    Self {
      show_hide: "Show/Hide".into(),
      play_pause: "Play/Pause".into(),
      always_on_top: "Always on top".into(),
      start_with_windows: "Start with Windows".into(),
      quit: "Quit".into(),
    }
  }
}

/// Réglages initiaux du fil système.
#[derive(Debug, Clone, Default)]
pub struct HostInit {
  pub labels: TrayLabels,
  pub media_session: bool,
  /// État initial de la case « Always on top » du menu.
  pub always_on_top: bool,
  /// État initial de l'entrée « Start with Windows » du menu.
  pub autostart: autostart::State,
}

/// Demande de l'application au fil système.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRequest {
  NowPlaying(NowPlaying),
  /// Active ou coupe la session média. `hwnd` : fenêtre principale (Windows), connue après son ouverture.
  MediaSession {
    enabled: bool,
    hwnd: Option<isize>,
  },
  Labels(TrayLabels),
  /// Coche ou décoche l'entrée « Always on top » du menu pour suivre le réglage.
  AlwaysOnTop(bool),
  /// Coche ou grise l'entrée « Start with Windows » d'après le registre.
  Autostart(autostart::State),
  /// Ré-enregistre raccourcis et session (retour de veille ou déverrouillage).
  Resume,
  Quit,
}

/// État observable depuis l'interface.
#[derive(Debug, Default)]
pub struct Report {
  /// Raccourcis refusés (déjà pris par un autre programme), au format lisible.
  pub failed_hotkeys: Mutex<Vec<String>>,
  /// La session média est ouverte.
  pub session_active: AtomicBool,
  /// L'icône de la zone de notification est affichée.
  pub tray_active: AtomicBool,
}

impl Report {
  #[must_use]
  pub fn failed(&self) -> Vec<String> {
    self
      .failed_hotkeys
      .lock()
      .map(|list| list.clone())
      .unwrap_or_default()
  }

  #[must_use]
  pub fn session_active(&self) -> bool {
    self.session_active.load(Ordering::Relaxed)
  }
}

/// Poignée du fil système.
pub struct Host {
  requests: Sender<HostRequest>,
  wake: Box<dyn Fn() + Send + Sync>,
  join: Mutex<Option<JoinHandle<()>>>,
  report: Arc<Report>,
}

impl Host {
  pub fn new(
    requests: Sender<HostRequest>,
    wake: Box<dyn Fn() + Send + Sync>,
    join: JoinHandle<()>,
    report: Arc<Report>,
  ) -> Self {
    Self {
      requests,
      wake,
      join: Mutex::new(Some(join)),
      report,
    }
  }

  /// Envoie une demande et réveille la boucle de messages.
  pub fn send(&self, request: HostRequest) {
    if self.requests.send(request).is_ok() {
      (self.wake)();
    }
  }

  #[must_use]
  pub fn report(&self) -> &Arc<Report> {
    &self.report
  }
}

impl Drop for Host {
  fn drop(&mut self) {
    self.send(HostRequest::Quit);
    if let Ok(mut join) = self.join.lock()
      && let Some(handle) = join.take()
    {
      let _ = handle.join();
    }
  }
}

/// Partie commune aux plateformes : la session média et le dernier titre connu.
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
pub struct Core {
  events: UnboundedSender<SystemEvent>,
  report: Arc<Report>,
  session: Option<super::media::Session>,
  wanted: bool,
  hwnd: Option<isize>,
  last: Option<NowPlaying>,
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
impl Core {
  #[must_use]
  pub fn new(events: UnboundedSender<SystemEvent>, report: Arc<Report>, wanted: bool) -> Self {
    Self {
      events,
      report,
      session: None,
      wanted,
      hwnd: None,
      last: None,
    }
  }

  /// Applique une demande commune. `false` : l'application demande l'arrêt.
  pub fn apply(&mut self, request: HostRequest) -> bool {
    match request {
      HostRequest::NowPlaying(now) => {
        if let Some(session) = &mut self.session {
          session.update(&now);
        }
        self.last = Some(now);
      }
      HostRequest::MediaSession { enabled, hwnd } => {
        self.wanted = enabled;
        if hwnd.is_some() {
          self.hwnd = hwnd;
        }
        self.sync_session();
      }
      HostRequest::Resume => self.recreate_session(),
      HostRequest::Quit => return false,
      HostRequest::Labels(_) | HostRequest::AlwaysOnTop(_) | HostRequest::Autostart(_) => {}
    }
    true
  }

  /// Ouvre ou ferme la session pour correspondre au réglage. Fermer la session libère les touches média.
  fn sync_session(&mut self) {
    match (self.wanted, self.session.is_some()) {
      (true, false) => {
        // Windows : pas de session sans fenêtre. Linux et macOS n'en ont pas besoin.
        if cfg!(windows) && self.hwnd.is_none() {
          return;
        }
        let hwnd = self.hwnd.map(|handle| handle as *mut std::ffi::c_void);
        match super::media::Session::create(hwnd, self.events.clone()) {
          Ok(mut session) => {
            if let Some(last) = &self.last {
              session.update(last);
            }
            self.session = Some(session);
          }
          Err(error) => tracing::warn!(%error, "session média indisponible"),
        }
      }
      (false, true) => self.session = None,
      _ => {}
    }
    self
      .report
      .session_active
      .store(self.session.is_some(), Ordering::Relaxed);
  }

  /// Retour de veille : la session est détruite puis recréée avec le dernier titre.
  pub fn recreate_session(&mut self) {
    self.session = None;
    self.sync_session();
  }
}
