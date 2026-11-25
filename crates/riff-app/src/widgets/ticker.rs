//! Minuteur d'animation à 60 Hz, calé sur les images (décision 4). `window::frames()` suit le rafraîchissement de l'écran,
//! 240 Hz sur la machine de mesure : chaque message reconstruirait la vue quatre fois trop souvent.

use iced::widget::canvas::{self, Action, Event, Geometry};
use iced::{Rectangle, Renderer, Theme, mouse, window};
use std::time::{Duration, Instant};

/// Période visée.
pub const PERIOD: Duration = Duration::from_nanos(16_666_667);
/// Une image qui arrive un peu avant l'échéance compte : sinon, elle attendrait une période d'écran de plus.
const SLACK: Duration = Duration::from_millis(1);

/// Décision pour une image reçue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
  /// Publier une image d'animation.
  Publish,
  /// Réveiller l'interface à cet instant.
  WaitUntil(Instant),
}

/// État du minuteur.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickerState {
  next: Option<Instant>,
}

impl TickerState {
  /// Décide pour une image reçue à `now`. L'échéance avance d'une période fixe pour ne pas dériver, sauf après un retard.
  pub fn step(&mut self, now: Instant) -> Step {
    let Some(next) = self.next else {
      self.next = Some(now + PERIOD);
      return Step::Publish;
    };
    if now + SLACK < next {
      return Step::WaitUntil(next);
    }
    let following = next + PERIOD;
    self.next = Some(if following <= now {
      now + PERIOD
    } else {
      following
    });
    Step::Publish
  }
}

/// Widget sans taille qui publie le message donné à 60 Hz tant qu'il est dans la vue.
#[derive(Debug, Clone, Copy)]
pub struct Ticker<Message: Clone> {
  pub on_frame: fn(Instant) -> Message,
}

impl<Message: Clone> canvas::Program<Message> for Ticker<Message> {
  type State = TickerState;

  fn update(
    &self,
    state: &mut TickerState,
    event: &Event,
    _bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Option<Action<Message>> {
    let Event::Window(window::Event::RedrawRequested(now)) = event else {
      return None;
    };
    Some(match state.step(*now) {
      Step::Publish => Action::publish((self.on_frame)(*now)),
      Step::WaitUntil(at) => Action::request_redraw_at(at),
    })
  }

  fn draw(
    &self,
    _state: &TickerState,
    _renderer: &Renderer,
    _theme: &Theme,
    _bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    Vec::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_first_frame_publishes() {
    assert_eq!(TickerState::default().step(Instant::now()), Step::Publish);
  }

  #[test]
  fn an_early_frame_waits_for_the_deadline() {
    let start = Instant::now();
    let mut state = TickerState::default();
    state.step(start);
    assert_eq!(
      state.step(start + Duration::from_millis(4)),
      Step::WaitUntil(start + PERIOD)
    );
    assert_eq!(
      state.step(start + Duration::from_millis(8)),
      Step::WaitUntil(start + PERIOD)
    );
  }

  #[test]
  fn a_frame_at_the_deadline_or_just_before_publishes() {
    let start = Instant::now();
    let mut state = TickerState::default();
    state.step(start);
    assert_eq!(state.step(start + PERIOD), Step::Publish);
    let mut near = TickerState::default();
    near.step(start);
    assert_eq!(
      near.step(start + PERIOD - Duration::from_micros(900)),
      Step::Publish
    );
    let mut far = TickerState::default();
    far.step(start);
    assert!(matches!(
      far.step(start + PERIOD - Duration::from_millis(2)),
      Step::WaitUntil(_)
    ));
  }

  #[test]
  fn the_cadence_does_not_drift_on_a_fast_screen() {
    // 240 Hz : une image toutes les 4,1667 ms. Publier toutes les 4 images donne 60 Hz exactement.
    let start = Instant::now();
    let mut state = TickerState::default();
    let mut published = 0;
    for frame in 0..=2400_u32 {
      if state.step(start + Duration::from_nanos(4_166_667) * frame) == Step::Publish {
        published += 1;
      }
    }
    assert_eq!(published, 601);
  }

  #[test]
  fn a_60_hz_screen_publishes_every_frame() {
    let start = Instant::now();
    let mut state = TickerState::default();
    for frame in 0..600_u32 {
      assert_eq!(
        state.step(start + PERIOD * frame),
        Step::Publish,
        "image {frame}"
      );
    }
  }

  #[test]
  fn a_late_frame_does_not_cause_a_burst() {
    let start = Instant::now();
    let mut state = TickerState::default();
    state.step(start);
    let late = start + Duration::from_millis(500);
    assert_eq!(state.step(late), Step::Publish);
    assert_eq!(
      state.step(late + Duration::from_millis(1)),
      Step::WaitUntil(late + PERIOD)
    );
  }
}
