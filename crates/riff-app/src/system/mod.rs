//! Intégration système : canal local, zone de notification, touches média, raccourcis globaux.

pub mod autostart;
pub mod bridge;
pub mod channel;
pub mod cli;
pub mod events;
pub mod host;
#[cfg(any(windows, target_os = "linux"))]
pub mod hotkeys;
pub mod launcher;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
pub mod media;
pub mod now_playing;
pub mod platform;
pub mod protocol;
pub mod side;
pub mod startup;
#[cfg(any(windows, target_os = "linux"))]
pub mod tray;
pub mod update_install;
pub mod updates;

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Commande venue du système : touche média, session système, menu de la zone de notification, raccourci global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
  Toggle,
  Play,
  Pause,
  Next,
  Previous,
  Stop,
  VolumeUp,
  VolumeDown,
  /// Position demandée par la session système.
  SeekTo(Duration),
}

/// Événement du système vers l'application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemEvent {
  /// Seconde instance : montrer la fenêtre, ouvrir ces fichiers.
  Show(Vec<PathBuf>),
  Command(Command),
  /// Menu de la zone de notification : afficher ou masquer la fenêtre.
  ToggleWindow,
  /// Menu de la zone de notification : bascule « Always on top » de la fenêtre principale.
  ToggleAlwaysOnTop,
  /// Menu de la zone de notification : bascule « Start with Windows ».
  ToggleAutostart,
  /// L'icône de la zone de notification n'a pas pu être créée : une fenêtre cachée doit réapparaître.
  TrayUnavailable,
  /// Menu de la zone de notification : quitter.
  Quit,
}

/// Délai pendant lequel une commande du même groupe est ignorée (`handleMediaCommand`, `app.js:1017`).
pub const DEDUP_WINDOW: Duration = Duration::from_millis(350);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
  PlayPause,
  Next,
  Previous,
  Stop,
}

impl Command {
  /// Groupe de déduplication. Les volumes et la position n'en ont pas : un raccourci répété doit agir à chaque fois.
  fn group(self) -> Option<Group> {
    match self {
      Self::Toggle | Self::Play | Self::Pause => Some(Group::PlayPause),
      Self::Next => Some(Group::Next),
      Self::Previous => Some(Group::Previous),
      Self::Stop => Some(Group::Stop),
      Self::VolumeUp | Self::VolumeDown | Self::SeekTo(_) => None,
    }
  }
}

/// Ignore une commande si le même groupe vient d'être traité (touche média reçue deux fois par deux chemins).
#[derive(Debug, Default)]
pub struct Dedup {
  last: Option<(Group, Instant)>,
}

impl Dedup {
  /// `true` si la commande doit être exécutée.
  pub fn accept(&mut self, command: Command, now: Instant) -> bool {
    let Some(group) = command.group() else {
      return true;
    };
    if let Some((previous, at)) = self.last
      && previous == group
      && now.saturating_duration_since(at) < DEDUP_WINDOW
    {
      return false;
    }
    self.last = Some((group, now));
    true
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn same_group_inside_the_window_is_dropped() {
    let mut dedup = Dedup::default();
    let t0 = Instant::now();
    assert!(dedup.accept(Command::Toggle, t0));
    assert!(!dedup.accept(Command::Toggle, t0 + Duration::from_millis(349)));
  }

  #[test]
  fn same_group_after_the_window_is_kept() {
    let mut dedup = Dedup::default();
    let t0 = Instant::now();
    assert!(dedup.accept(Command::Next, t0));
    assert!(dedup.accept(Command::Next, t0 + Duration::from_millis(350)));
  }

  #[test]
  fn toggle_play_and_pause_share_a_group() {
    let mut dedup = Dedup::default();
    let t0 = Instant::now();
    assert!(dedup.accept(Command::Play, t0));
    assert!(!dedup.accept(Command::Toggle, t0 + Duration::from_millis(10)));
    assert!(!dedup.accept(Command::Pause, t0 + Duration::from_millis(20)));
  }

  #[test]
  fn different_groups_do_not_block_each_other() {
    let mut dedup = Dedup::default();
    let t0 = Instant::now();
    assert!(dedup.accept(Command::Next, t0));
    assert!(dedup.accept(Command::Previous, t0 + Duration::from_millis(10)));
    assert!(dedup.accept(Command::Stop, t0 + Duration::from_millis(20)));
  }

  #[test]
  fn volume_and_seek_are_never_dropped() {
    let mut dedup = Dedup::default();
    let t0 = Instant::now();
    assert!(dedup.accept(Command::VolumeUp, t0));
    assert!(dedup.accept(Command::VolumeUp, t0));
    assert!(dedup.accept(Command::SeekTo(Duration::from_secs(3)), t0));
    assert!(dedup.accept(Command::SeekTo(Duration::from_secs(3)), t0));
  }

  #[test]
  fn a_dropped_command_does_not_extend_the_window() {
    let mut dedup = Dedup::default();
    let t0 = Instant::now();
    assert!(dedup.accept(Command::Toggle, t0));
    assert!(!dedup.accept(Command::Toggle, t0 + Duration::from_millis(300)));
    assert!(dedup.accept(Command::Toggle, t0 + Duration::from_millis(360)));
  }
}
