//! `.g-spinner` de Riff : cercle de rayon 20 dans un viewBox de 48 qui tourne en 1,4 s (`g-rot`) pendant que son arc
//! s'allonge puis se résorbe (`g-dash`, `styles.css:6248-6251`). Le temps écoulé vient de l'horloge de `App` : la vue est
//! pure et le banc de rendu peut la figer à un instant donné.

use crate::theme::tokens::g_spinner as tokens;
use crate::visuals::state::bezier_ease;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke};
use iced::{Color, Element, Point, Radians, Rectangle, Renderer, Theme, Vector, mouse};
use std::f32::consts::TAU;
use std::time::Duration;

/// Progrès (0 à 1) dans la période de 1,4 s.
#[must_use]
pub fn progress(elapsed: Duration) -> f32 {
  let period = Duration::from_millis(tokens::PERIOD_MS);
  (elapsed.as_secs_f32() % period.as_secs_f32()) / period.as_secs_f32()
}

/// `(stroke-dasharray, stroke-dashoffset)` de `g-dash` au progrès `t` : la courbe `ease-in-out` s'applique à chacun des deux
/// intervalles (`0 % -> 50 %` puis `50 % -> 100 %`), comme pour une règle `@keyframes`.
#[must_use]
pub fn dash(t: f32) -> (f32, f32) {
  if t < 0.5 {
    let e = bezier_ease(tokens::DASH_CURVE, t * 2.0);
    (
      tokens::DASH_START + (tokens::DASH_END - tokens::DASH_START) * e,
      tokens::OFFSET_MID * e,
    )
  } else {
    let e = bezier_ease(tokens::DASH_CURVE, (t - 0.5) * 2.0);
    (
      tokens::DASH_END,
      tokens::OFFSET_MID + (tokens::OFFSET_END - tokens::OFFSET_MID) * e,
    )
  }
}

/// Dessine le spinner dans `frame`, centré en `center`, dans une boîte de `side` pixels (le viewBox de 48 y est mis à l'échelle).
///
/// Le tiret de SVG (`stroke-dasharray` de `dash` unités, décalé de `-stroke-dashoffset`) est un arc du cercle : tracé tel quel,
/// coupé à la fin du cercle comme le fait SVG (le motif ne boucle pas). `LineDash` d'iced ne convient pas : son décalage est un
/// indice d'intervalle, pas une distance, et `dashed()` (`iced_wgpu-0.14.0/src/geometry.rs:743`) relie les bornes de chaque tiret
/// par une corde au lieu de suivre la courbe (constaté sur le rendu : un trait droit à la place des 3/4 de cercle).
///
/// `pen` multiplie l'épaisseur du trait : iced ne la suit pas quand le `Frame` est mis à l'échelle (1 sans transformation).
pub fn draw(
  frame: &mut Frame,
  center: Point,
  side: f32,
  color: Color,
  elapsed: Duration,
  pen: f32,
) {
  let scale = side / tokens::VIEW_BOX;
  let t = progress(elapsed);
  let (len, offset) = dash(t);
  let circumference = TAU * tokens::RADIUS;
  let start = -offset;
  let len = len.min(circumference - start);
  if len <= 0.0 {
    return;
  }
  let arc = Path::new(|path| {
    path.arc(canvas::path::Arc {
      center: Point::ORIGIN,
      radius: tokens::RADIUS * scale,
      start_angle: Radians(0.0),
      end_angle: Radians(len / tokens::RADIUS),
    });
  });
  frame.with_save(|frame| {
    frame.translate(Vector::new(center.x, center.y));
    // `g-rot` (linéaire) puis le début de l'arc sur le cercle, qui part à 3 h et tourne dans le sens horaire.
    frame.rotate(Radians(t * TAU + start / tokens::RADIUS));
    frame.stroke(
      &arc,
      Stroke::default()
        .with_color(color)
        .with_width(tokens::STROKE * scale * pen)
        .with_line_cap(LineCap::Round),
    );
  });
}

struct GSpinner {
  color: Color,
  elapsed: Duration,
}

impl<Message> canvas::Program<Message> for GSpinner {
  type State = ();

  fn draw(
    &self,
    _state: &(),
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let mut frame = Frame::new(renderer, bounds.size());
    let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
    draw(
      &mut frame,
      center,
      bounds.width,
      self.color,
      self.elapsed,
      1.0,
    );
    vec![frame.into_geometry()]
  }
}

/// `.g-spinner` de `side` pixels, en `color`, au temps écoulé `elapsed`.
pub fn g_spinner<'a, Message: 'a>(
  side: f32,
  color: Color,
  elapsed: Duration,
) -> Element<'a, Message> {
  canvas::Canvas::new(GSpinner { color, elapsed })
    .width(side)
    .height(side)
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.05
  }

  #[test]
  fn the_dash_follows_the_keyframes() {
    assert_eq!(dash(0.0), (1.0, 0.0));
    let (len, offset) = dash(0.5);
    assert!(close(len, 90.0) && close(offset, -35.0), "{len} {offset}");
    let (len, offset) = dash(0.999_999);
    assert!(close(len, 90.0) && close(offset, -124.0), "{len} {offset}");
  }

  #[test]
  fn each_interval_is_eased_separately() {
    // `ease-in-out` vaut 0,5 au milieu de chaque intervalle : 25 % et 75 % de la période.
    let (len, offset) = dash(0.25);
    assert!(close(len, 45.5) && close(offset, -17.5), "{len} {offset}");
    let (len, offset) = dash(0.75);
    assert!(close(len, 90.0) && close(offset, -79.5), "{len} {offset}");
  }

  #[test]
  fn the_dash_matches_the_measured_samples() {
    // `03b-toast-spinner/samples.json` : début de l'animation à 63,5 ms (milieu de `g-dash` à 763,5 ms : 90, -35,006).
    let at = |sample_ms: f32| {
      dash(progress(Duration::from_secs_f32(
        (sample_ms - 63.5) / 1000.0,
      )))
    };
    let (len, offset) = at(700.9);
    assert!(
      (len - 88.79).abs() < 0.8 && (offset + 34.52).abs() < 0.4,
      "{len} {offset}"
    );
    let (len, offset) = at(1042.7);
    assert!(
      (len - 90.0).abs() < 0.01 && (offset + 65.24).abs() < 2.0,
      "{len} {offset}"
    );
    let (len, offset) = at(1284.4);
    assert!(
      (len - 90.0).abs() < 0.01 && (offset + 112.5).abs() < 2.0,
      "{len} {offset}"
    );
  }

  #[test]
  fn the_period_wraps_at_1400_ms() {
    assert!(close(progress(Duration::from_millis(350)), 0.25));
    assert!(close(progress(Duration::from_millis(1750)), 0.25));
    assert!(progress(Duration::ZERO).abs() < f32::EPSILON);
  }
}
