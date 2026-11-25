//! Titre en cours tel que le système (session média, Discord, `riff lyrics`) le voit, et décision d'envoi.

use std::path::PathBuf;
use std::time::Duration;

/// Écart entre la position annoncée et la position extrapolée au-delà duquel le système est averti (saut de lecture).
pub const POSITION_DRIFT: Duration = Duration::from_millis(1500);

/// Titre et état de lecture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NowPlaying {
  pub title: String,
  pub artist: String,
  pub album: String,
  /// Fichier de pochette local : la session système ne télécharge jamais une pochette elle-même.
  pub cover_file: Option<PathBuf>,
  pub duration: Option<Duration>,
  pub position: Duration,
  pub playing: bool,
}

impl NowPlaying {
  /// Aucun titre en cours.
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.title.is_empty() && self.artist.is_empty()
  }

  /// Même titre, mêmes métadonnées : seul l'état de lecture ou la position diffère.
  fn same_item(&self, other: &Self) -> bool {
    self.title == other.title
      && self.artist == other.artist
      && self.album == other.album
      && self.cover_file == other.cover_file
      && self.duration == other.duration
  }
}

/// Décide quand pousser l'état au système : à chaque changement de titre ou d'état, et sur un saut de position.
/// La position qui avance normalement n'est pas renvoyée : le système l'extrapole.
#[derive(Debug, Default)]
pub struct SyncGate {
  sent: Option<NowPlaying>,
  /// Instant d'envoi, en durée de lecture écoulée côté application.
  sent_at: Option<Duration>,
}

impl SyncGate {
  /// `clock` : horloge monotone de l'application. Rend `true` s'il faut envoyer `now`.
  pub fn should_send(&mut self, now: &NowPlaying, clock: Duration) -> bool {
    let send = match (&self.sent, self.sent_at) {
      (None, _) => true,
      (Some(previous), at) => {
        if !previous.same_item(now) || previous.playing != now.playing {
          true
        } else {
          let elapsed = if now.playing {
            at.map_or(Duration::ZERO, |at| clock.saturating_sub(at))
          } else {
            Duration::ZERO
          };
          let expected = previous.position + elapsed;
          let drift = now.position.abs_diff(expected);
          drift > POSITION_DRIFT
        }
      }
    };
    if send {
      self.sent = Some(now.clone());
      self.sent_at = Some(clock);
    }
    send
  }

  /// Oublie l'état envoyé : le prochain état est renvoyé (session recréée après une veille).
  pub fn reset(&mut self) {
    self.sent = None;
    self.sent_at = None;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn track(position_secs: u64, playing: bool) -> NowPlaying {
    NowPlaying {
      title: "T".into(),
      artist: "A".into(),
      album: String::new(),
      cover_file: None,
      duration: Some(Duration::from_secs(200)),
      position: Duration::from_secs(position_secs),
      playing,
    }
  }

  const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
  }

  #[test]
  fn first_state_is_always_sent() {
    assert!(SyncGate::default().should_send(&track(0, false), secs(0)));
  }

  #[test]
  fn normal_playback_progress_is_not_resent() {
    let mut gate = SyncGate::default();
    assert!(gate.should_send(&track(10, true), secs(100)));
    assert!(!gate.should_send(&track(11, true), secs(101)));
    assert!(!gate.should_send(&track(25, true), secs(115)));
  }

  #[test]
  fn a_seek_forward_or_back_is_resent() {
    let mut gate = SyncGate::default();
    gate.should_send(&track(10, true), secs(100));
    assert!(gate.should_send(&track(60, true), secs(101)));
    assert!(gate.should_send(&track(5, true), secs(102)));
  }

  #[test]
  fn pause_and_resume_are_resent() {
    let mut gate = SyncGate::default();
    gate.should_send(&track(10, true), secs(100));
    assert!(gate.should_send(&track(11, false), secs(101)));
    assert!(gate.should_send(&track(11, true), secs(150)));
  }

  #[test]
  fn paused_position_does_not_extrapolate() {
    let mut gate = SyncGate::default();
    gate.should_send(&track(10, false), secs(100));
    assert!(!gate.should_send(&track(10, false), secs(160)));
    assert!(gate.should_send(&track(30, false), secs(161)));
  }

  #[test]
  fn another_track_is_resent() {
    let mut gate = SyncGate::default();
    gate.should_send(&track(10, true), secs(100));
    let mut next = track(0, true);
    next.title = "Other".into();
    assert!(gate.should_send(&next, secs(101)));
  }

  #[test]
  fn a_late_cover_is_resent() {
    let mut gate = SyncGate::default();
    gate.should_send(&track(10, true), secs(100));
    let mut with_cover = track(11, true);
    with_cover.cover_file = Some(PathBuf::from("c.jpg"));
    assert!(gate.should_send(&with_cover, secs(101)));
  }

  #[test]
  fn reset_forces_the_next_send() {
    let mut gate = SyncGate::default();
    gate.should_send(&track(10, true), secs(100));
    gate.reset();
    assert!(gate.should_send(&track(11, true), secs(101)));
  }

  #[test]
  fn empty_means_no_title_and_no_artist() {
    assert!(NowPlaying::default().is_empty());
    assert!(!track(0, false).is_empty());
  }
}
