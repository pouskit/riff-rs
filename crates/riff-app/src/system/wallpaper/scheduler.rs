//! Predictive track-boundary fade scheduler (1:1 port of `FadeScheduler` in `wallpaper-sync-mod`).

use super::config::{TransitionAnchor, TransitionEasing};

/// Maximum learned bias correction in seconds.
pub const MAX_END_BIAS: f64 = 3.0;
/// Past this age (seconds), reported playback progress is considered stale.
pub const MAX_TIMING_AGE: f64 = 4.0;
/// Minimum interval (seconds) between two scheduled fades.
pub const MIN_FIRE_INTERVAL: f64 = 5.0;
/// Tolerance (seconds) on a fade deadline that slightly passed.
pub const LATE_TOLERANCE: f64 = 0.25;
/// Deadband (seconds) below which an unchanged deadline is not rescheduled.
pub const RESCHEDULE_EPSILON: f64 = 0.05;

#[must_use]
pub fn anchor_offset(duration: f64, anchor: TransitionAnchor) -> f64 {
  match anchor {
    TransitionAnchor::End => duration,
    TransitionAnchor::Start => 0.0,
    TransitionAnchor::Center => duration / 2.0,
  }
}

#[must_use]
pub fn ease(progress: f64, easing: TransitionEasing) -> f64 {
  let t = progress.clamp(0.0, 1.0);
  match easing {
    TransitionEasing::Linear => t,
    TransitionEasing::Smoothstep => t * t * (3.0 - 2.0 * t),
  }
}

/// Schedules pre-armed wallpaper crossfades relative to the predicted end of the current track.
#[derive(Debug, Default, Clone)]
pub struct FadeScheduler {
  t_end: Option<f64>,
  t_end_stamp: f64,
  end_bias: f64,
  pub fade_at: Option<f64>,
  last_fire_at: f64,
}

impl FadeScheduler {
  /// Realigns the predicted track end from playback progress (`remaining_secs`, `paused`).
  /// Returns a cancellation reason when playback is paused.
  pub fn note_progress(
    &mut self,
    remaining_secs: Option<f64>,
    paused: bool,
    now: f64,
  ) -> Option<&'static str> {
    if paused {
      self.t_end = None;
      return Some("playback paused");
    }
    let Some(rem) = remaining_secs else {
      self.t_end = None;
      return None;
    };
    self.t_end = Some(now + rem);
    self.t_end_stamp = now;
    None
  }

  #[must_use]
  pub fn remaining(&self, now: f64) -> Option<f64> {
    self.t_end.map(|end| end - now)
  }

  /// Updates the EMA end-bias from the gap between predicted end and observed track switch.
  pub fn learn_bias(&mut self, now: f64) -> Option<(f64, f64)> {
    let t_end = self.t_end?;
    let error = t_end - now;
    if !(error > -1.0 && error < MAX_END_BIAS) {
      return None;
    }
    let next = if self.end_bias == 0.0 {
      error
    } else {
      0.6 * self.end_bias + 0.4 * error
    };
    self.end_bias = next.clamp(0.0, MAX_END_BIAS);
    Some((error, self.end_bias))
  }

  /// Places the fade deadline relative to the predicted end.
  /// Returns `(newly_set_deadline, cancellation_reason)`.
  pub fn plan(
    &mut self,
    duration: f64,
    anchor: TransitionAnchor,
    now: f64,
  ) -> (Option<f64>, Option<&'static str>) {
    let Some(t_end) = self.t_end else {
      return (None, None);
    };
    if now - self.t_end_stamp > MAX_TIMING_AGE {
      return (None, Some("stale progress"));
    }
    if now - self.last_fire_at < MIN_FIRE_INTERVAL {
      return (None, None);
    }
    let target = t_end - self.end_bias - anchor_offset(duration, anchor);
    if target < now - LATE_TOLERANCE {
      return (None, Some("deadline already passed"));
    }
    if let Some(prev) = self.fade_at
      && (prev - target).abs() <= RESCHEDULE_EPSILON
    {
      return (None, None);
    }
    self.fade_at = Some(target);
    (Some(target), None)
  }

  #[must_use]
  pub fn due_in(&self, now: f64) -> Option<f64> {
    self.fade_at.map(|at| at - now)
  }

  pub fn cancel(&mut self) {
    self.fade_at = None;
  }

  pub fn fire(&mut self, now: f64) -> Option<f64> {
    let deadline = self.fade_at.take();
    self.last_fire_at = now;
    self.t_end = None;
    deadline
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn plan_places_fade_at_anchor_and_respects_reschedule_epsilon() {
    let mut sched = FadeScheduler::default();
    assert_eq!(sched.note_progress(Some(10.0), false, 100.0), None);
    let (deadline, reason) = sched.plan(1.5, TransitionAnchor::Center, 100.0);
    assert!(reason.is_none());
    assert!((deadline.unwrap() - 109.25).abs() < 1e-6);

    // Within RESCHEDULE_EPSILON -> no-op.
    assert_eq!(sched.note_progress(Some(9.97), false, 100.01), None);
    let (deadline2, reason2) = sched.plan(1.5, TransitionAnchor::Center, 100.01);
    assert!(deadline2.is_none() && reason2.is_none());
  }

  #[test]
  fn pause_and_stale_progress_cancel_scheduling() {
    let mut sched = FadeScheduler::default();
    sched.note_progress(Some(10.0), false, 100.0);
    assert_eq!(
      sched.note_progress(Some(10.0), true, 101.0),
      Some("playback paused")
    );
    assert_eq!(sched.remaining(101.0), None);

    sched.note_progress(Some(10.0), false, 100.0);
    let (d, r) = sched.plan(1.5, TransitionAnchor::End, 104.5);
    assert!(d.is_none());
    assert_eq!(r, Some("stale progress"));
  }

  #[test]
  fn learn_bias_ignores_outliers_and_clamps_ema() {
    let mut sched = FadeScheduler::default();
    sched.note_progress(Some(20.0), false, 100.0);
    // Manual skip (15s early) is ignored.
    assert!(sched.learn_bias(105.0).is_none());
    // Natural transition 1.0s before predicted end.
    let (err, bias) = sched.learn_bias(119.0).unwrap();
    assert!((err - 1.0).abs() < 1e-6);
    assert!((bias - 1.0).abs() < 1e-6);
  }

  #[test]
  fn fire_clears_deadline_and_enforces_min_interval() {
    let mut sched = FadeScheduler::default();
    sched.note_progress(Some(5.0), false, 100.0);
    sched.plan(1.0, TransitionAnchor::Start, 100.0);
    assert!((sched.fire(105.0).unwrap() - 105.0).abs() < 1e-6);
    sched.note_progress(Some(8.0), false, 106.0);
    let (d, _) = sched.plan(1.0, TransitionAnchor::Start, 106.0);
    assert!(d.is_none());
  }

  #[test]
  fn easing_endpoints_and_midpoint() {
    assert_eq!(ease(-0.5, TransitionEasing::Smoothstep), 0.0);
    assert_eq!(ease(0.5, TransitionEasing::Smoothstep), 0.5);
    assert_eq!(ease(1.5, TransitionEasing::Smoothstep), 1.0);
    assert!((ease(0.25, TransitionEasing::Linear) - 0.25).abs() < 1e-6);
  }
}
