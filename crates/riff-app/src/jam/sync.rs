//! Synchronisation de la lecture : horloge estimée, position attendue, décision de correction. Fonctions
//! pures : l'instant courant est toujours passé en argument.

use super::{CORRECTION_COOLDOWN, SEEK_THRESHOLD};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const MAX_SAMPLES: usize = 8;
/// Au-delà, la position de l'hôte n'est plus extrapolée : le moteur a cessé de publier.
const MAX_EXTRAPOLATION: Duration = Duration::from_secs(1);

/// Écart entre l'horloge de l'hôte et celle de l'invité, estimé au plus petit aller-retour.
#[derive(Debug, Clone, Default)]
pub struct ClockSync {
  /// `(rtt_ms, offset_ms)`, les plus récents en dernier.
  samples: VecDeque<(u64, i64)>,
}

impl ClockSync {
  fn push(&mut self, rtt: u64, offset: i64) {
    if self.samples.len() == MAX_SAMPLES {
      self.samples.pop_front();
    }
    self.samples.push_back((rtt, offset));
  }

  /// `t0`, `t2` : ms de l'invité à l'envoi et à la réception ; `t1` : ms de l'hôte.
  /// `rtt = t2 - t0`, `offset = t1 - (t0 + rtt / 2)`. Un `t2 < t0` (horloge incohérente) est ignoré.
  pub fn add(&mut self, t0: u64, t1: u64, t2: u64) {
    let Some(rtt) = t2.checked_sub(t0) else {
      return;
    };
    let offset = i128::from(t1) - i128::from(t0 + rtt / 2);
    if let Ok(offset) = i64::try_from(offset) {
      self.push(rtt, offset);
    }
  }

  /// Premier écart, à `welcome` : `host - guest`, sans mesure d'aller-retour.
  pub fn seed(&mut self, host_ms: u64, guest_ms: u64) {
    if let Ok(offset) = i64::try_from(i128::from(host_ms) - i128::from(guest_ms)) {
      self.push(u64::MAX, offset);
    }
  }

  /// Écart (ms) de l'échantillon au plus petit aller-retour ; le plus récent départage.
  #[must_use]
  pub fn offset(&self) -> Option<i64> {
    self
      .samples
      .iter()
      .rev()
      .min_by_key(|(rtt, _)| *rtt)
      .map(|&(_, offset)| offset)
  }
}

/// Instant local qui correspond à `at_ms` de l'hôte ; repli sur `now` si le calcul sort des bornes.
#[must_use]
pub fn host_to_local(at_ms: u64, offset_ms: i64, guest_epoch: Instant, now: Instant) -> Instant {
  let guest_ms = i128::from(at_ms) - i128::from(offset_ms);
  u64::try_from(guest_ms)
    .ok()
    .and_then(|ms| guest_epoch.checked_add(Duration::from_millis(ms)))
    .unwrap_or(now)
}

/// Position que doit avoir l'invité : `pos + (now - anchor)` en lecture, bornée à `duration` ; figée sinon.
#[must_use]
pub fn expected_position(
  pos_ms: u64,
  anchor: Instant,
  playing: bool,
  now: Instant,
  duration: Option<Duration>,
) -> Duration {
  let pos = Duration::from_millis(pos_ms);
  let position = if playing {
    if now >= anchor {
      pos.saturating_add(now.duration_since(anchor))
    } else {
      pos.saturating_sub(anchor.duration_since(now))
    }
  } else {
    pos
  };
  duration.map_or(position, |duration| position.min(duration))
}

/// Écart au-delà de `SEEK_THRESHOLD` et dernière correction plus vieille que `CORRECTION_COOLDOWN`.
#[must_use]
pub fn needs_correction(
  local: Duration,
  expected: Duration,
  last: Option<Instant>,
  now: Instant,
) -> bool {
  let gap = local.abs_diff(expected);
  gap > SEEK_THRESHOLD
    && last.is_none_or(|last| now.saturating_duration_since(last) > CORRECTION_COOLDOWN)
}

/// Position de l'hôte à `now` : dernière position du moteur plus le temps écoulé en lecture, au plus 1 s.
#[must_use]
pub fn host_position(
  position: Duration,
  position_at: Option<Instant>,
  playing: bool,
  now: Instant,
) -> Duration {
  match position_at {
    Some(at) if playing => position + now.saturating_duration_since(at).min(MAX_EXTRAPOLATION),
    _ => position,
  }
}
