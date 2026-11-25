//! Défilement vers la ligne active : `animateLyricsScroll`, `LYRIC_EASINGS`, `riff_lyrics_speed`.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use crate::theme::tokens::lyrics as tokens;
use crate::visuals::state::bezier_ease;

/// `customBezierEase` : 14 pas de dichotomie sur `cubic-bezier(0.786, 0.648, 0, 0.935)`.
fn custom_bezier(t: f32) -> f32 {
  if t <= 0.0 {
    return 0.0;
  }
  if t >= 1.0 {
    return 1.0;
  }
  let (mut low, mut high, mut s) = (0.0_f32, 1.0_f32, t);
  for _ in 0..14 {
    s = f32::midpoint(low, high);
    let inv = 1.0 - s;
    let x = 3.0 * inv * inv * s * 0.786 + s * s * s;
    if x < t {
      low = s;
    } else {
      high = s;
    }
  }
  let inv = 1.0 - s;
  3.0 * inv * inv * s * 0.648 + 3.0 * inv * s * s * 0.935 + s * s * s
}

/// Courbes de `riff_lyrics_ease`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Easing {
  #[default]
  InOut,
  Quad,
  Sine,
  Out,
  OutSine,
  Expo,
  Linear,
}

impl Easing {
  /// Une clé inconnue donne `inout`, comme `getLyricsEase`.
  #[must_use]
  pub fn from_key(key: &str) -> Self {
    match key {
      "quad" => Self::Quad,
      "sine" => Self::Sine,
      "out" => Self::Out,
      "outsine" => Self::OutSine,
      "expo" => Self::Expo,
      "linear" => Self::Linear,
      _ => Self::InOut,
    }
  }

  #[must_use]
  pub fn apply(self, t: f32) -> f32 {
    match self {
      Self::InOut => custom_bezier(t),
      Self::Quad => {
        if t < 0.5 {
          2.0 * t * t
        } else {
          1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
        }
      }
      Self::Sine => -((PI * t).cos() - 1.0) / 2.0,
      Self::Out => 1.0 - (1.0 - t).powi(4),
      Self::OutSine => (t * PI / 2.0).sin(),
      Self::Expo => {
        if t >= 1.0 {
          1.0
        } else {
          1.0 - 2.0_f32.powf(-10.0 * t)
        }
      }
      Self::Linear => t,
    }
  }

  /// Courbe `--lyric-ease` des transitions CSS : le `cubic-bezier` de `LYRIC_EASINGS[...].css`, pas la fonction JS du défilement
  /// (elles diffèrent sauf pour `inout`, `quad` et `sine`).
  #[must_use]
  pub fn apply_css(self, t: f32) -> f32 {
    let curve = match self {
      Self::InOut => tokens::CURVE_INOUT,
      Self::Quad => tokens::CURVE_QUAD,
      Self::Sine => tokens::CURVE_SINE,
      Self::Out => tokens::CURVE_OUT,
      Self::OutSine => tokens::CURVE_OUTSINE,
      Self::Expo => tokens::CURVE_EXPO,
      Self::Linear => return t.clamp(0.0, 1.0),
    };
    bezier_ease(curve, t)
  }
}

/// `getLyricsSpeed` : hors de 0,5 à 2,5, la vitesse vaut 1.
#[must_use]
pub fn clamp_speed(speed: f32) -> f32 {
  if (0.5..=2.5).contains(&speed) {
    speed
  } else {
    1.0
  }
}

/// Durée de l'animation d'un déplacement.
#[must_use]
pub fn duration(delta: f32, speed: f32) -> Duration {
  let ms = (tokens::SCROLL_BASE_MS + delta.abs() * tokens::SCROLL_PER_PX)
    .clamp(tokens::SCROLL_MIN_MS, tokens::SCROLL_MAX_MS);
  Duration::from_secs_f64(f64::from(ms / speed) / 1000.0)
}

/// `--lyric-duration` : durée des transitions CSS de `.lyric-line`, arrondie au centième de seconde (`toFixed(2)`).
#[must_use]
pub fn transition_duration(easing: Easing, speed: f32) -> Duration {
  let base = if easing == Easing::InOut {
    tokens::DURATION_INOUT_MS
  } else {
    tokens::DURATION_OTHER_MS
  };
  Duration::from_secs_f64((f64::from(base / speed) / 10.0).round() / 100.0)
}

/// Animation en cours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollAnimation {
  pub from: f32,
  pub to: f32,
  pub started: Instant,
  pub duration: Duration,
  pub easing: Easing,
}

impl ScrollAnimation {
  /// Un déplacement de moins d'un pixel ne s'anime pas.
  #[must_use]
  pub fn new(from: f32, to: f32, started: Instant, speed: f32, easing: Easing) -> Option<Self> {
    let delta = to - from;
    (delta.abs() >= 1.0).then(|| Self {
      from,
      to,
      started,
      duration: duration(delta, speed),
      easing,
    })
  }

  /// Position à cet instant, et vrai si l'animation est finie.
  #[must_use]
  pub fn at(&self, now: Instant) -> (f32, bool) {
    // `animateLyricsScroll` (`app.js:6174`) : `requestAnimationFrame`, ni `force-reduced-motion` ni `body.perf` n'y changent rien.
    let t = (now.saturating_duration_since(self.started).as_secs_f32()
      / self.duration.as_secs_f32())
    .min(1.0);
    if t >= 1.0 {
      return (self.to, true);
    }
    (
      self.from + (self.to - self.from) * self.easing.apply(t),
      false,
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
  }

  #[test]
  fn unknown_keys_use_the_default_easing() {
    assert_eq!(Easing::from_key("sine"), Easing::Sine);
    assert_eq!(Easing::from_key("outsine"), Easing::OutSine);
    assert_eq!(Easing::from_key("rebond"), Easing::InOut);
    assert_eq!(Easing::from_key(""), Easing::InOut);
  }

  #[test]
  fn every_easing_goes_from_zero_to_one() {
    for easing in [
      Easing::InOut,
      Easing::Quad,
      Easing::Sine,
      Easing::Out,
      Easing::OutSine,
      Easing::Linear,
    ] {
      assert!(close(easing.apply(0.0), 0.0), "{easing:?}");
      assert!(close(easing.apply(1.0), 1.0), "{easing:?}");
    }
    assert!(close(Easing::Expo.apply(1.0), 1.0));
    assert!(close(Easing::Expo.apply(0.0), 0.0));
  }

  #[test]
  fn the_values_are_those_of_the_javascript() {
    // Calculées avec les formules de LYRIC_EASINGS.
    assert!(close(Easing::Quad.apply(0.25), 0.125));
    assert!(close(Easing::Sine.apply(0.5), 0.5));
    assert!(close(Easing::Out.apply(0.5), 0.9375));
    assert!(close(
      Easing::OutSine.apply(0.5),
      std::f32::consts::FRAC_1_SQRT_2
    ));
    assert!(close(Easing::Expo.apply(0.5), 0.96875));
    assert!(close(Easing::InOut.apply(0.5), 0.88511));
  }

  #[test]
  fn the_speed_stays_in_its_range() {
    assert_eq!(clamp_speed(0.5), 0.5);
    assert_eq!(clamp_speed(2.5), 2.5);
    assert_eq!(clamp_speed(0.49), 1.0);
    assert_eq!(clamp_speed(3.0), 1.0);
    assert_eq!(clamp_speed(f32::NAN), 1.0);
  }

  #[test]
  fn the_duration_depends_on_the_distance_within_bounds() {
    assert_eq!(duration(0.0, 1.0).as_millis(), 320);
    assert_eq!(duration(100.0, 1.0).as_millis(), 348);
    assert_eq!(duration(-100.0, 1.0).as_millis(), 348);
    assert_eq!(duration(10_000.0, 1.0).as_millis(), 480);
    assert_eq!(duration(0.0, 2.0).as_millis(), 160);
  }

  #[test]
  fn the_css_transition_duration_follows_the_ease_and_the_speed() {
    // `applyLyricsEase` : 0,48 s pour inout, 0,45 s sinon, divisé par la vitesse et arrondi au centième.
    assert_eq!(transition_duration(Easing::InOut, 1.0).as_millis(), 480);
    assert_eq!(transition_duration(Easing::Sine, 1.0).as_millis(), 450);
    assert_eq!(transition_duration(Easing::InOut, 2.5).as_millis(), 190);
    assert_eq!(transition_duration(Easing::Linear, 0.5).as_millis(), 900);
  }

  #[test]
  fn the_css_curve_is_the_bezier_of_the_ease() {
    for easing in [Easing::InOut, Easing::Quad, Easing::Sine, Easing::Out] {
      assert!(close(easing.apply_css(0.0), 0.0) && close(easing.apply_css(1.0), 1.0));
    }
    // `inout` : la fonction JS et le `cubic-bezier` sont la même courbe.
    for step in 0..=10 {
      let t = step as f32 / 10.0;
      assert!(
        close(Easing::InOut.apply_css(t), Easing::InOut.apply(t)),
        "{t}"
      );
    }
    assert!(close(Easing::Linear.apply_css(0.3), 0.3));
    // `out` : `cubic-bezier(0.22, 1, 0.36, 1)` n'est pas `1 - (1 - t)^4`.
    assert!((Easing::Out.apply_css(0.1) - Easing::Out.apply(0.1)).abs() > 0.01);
  }

  #[test]
  fn a_jump_of_ninety_pixels_follows_the_measured_curve() {
    // `15-paroles` : +90 px en 345 ms (`320 + 90 * 0,28`) ; la capture atteint 95 % de la course 210 ms après le départ du défilement.
    let start = Instant::now();
    let animation = ScrollAnimation::new(532.28, 622.28, start, 1.0, Easing::InOut).unwrap();
    assert_eq!(animation.duration.as_millis(), 345);
    let (at_254, done) = animation.at(start + Duration::from_millis(254));
    assert!(!done && at_254 > 532.28 + 85.0, "{at_254}");
    let (early, _) = animation.at(start + Duration::from_millis(30));
    assert!(early > 532.28 && early < 540.0, "{early}");
  }

  #[test]
  fn a_tiny_move_is_not_animated() {
    assert_eq!(
      ScrollAnimation::new(10.0, 10.5, Instant::now(), 1.0, Easing::Linear),
      None
    );
  }

  #[test]
  fn the_animation_reaches_its_target_and_ends() {
    let start = Instant::now();
    let animation = ScrollAnimation::new(0.0, 100.0, start, 1.0, Easing::Linear).unwrap();
    let total = animation.duration;
    assert_eq!(animation.at(start), (0.0, false));
    let (middle, done) = animation.at(start + total / 2);
    assert!(close(middle, 50.0) && !done, "{middle}");
    assert_eq!(animation.at(start + total), (100.0, true));
    assert_eq!(animation.at(start + total * 2), (100.0, true));
  }
}
