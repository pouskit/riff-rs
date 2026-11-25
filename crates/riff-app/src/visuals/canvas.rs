//! Dessin des visualiseurs en `Canvas` (décision 3). Un dessin par image : pas de cache, le contenu change à chaque image.

use super::shapes::SAMPLE_COUNT;
use super::viz::{self, BAR_HALF, MINI_BARS, MINI_HEIGHT, TVK_BARS};
use crate::theme::tokens::{css, halo};
use iced::widget::canvas::{
  self, Fill, Frame, Geometry, LineCap, LineJoin, Path, Stroke, Style, fill,
};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};
use std::f32::consts::{PI, TAU};

/// Dessine le contour fermé de `drawConnectedLiquidWave` (tension 0,18).
fn liquid_path(points: &[(f32, f32)], height: f32) -> Path {
  const TENSION: f32 = 0.18;
  Path::new(|path| {
    path.move_to(Point::new(0.0, height));
    for index in 0..points.len().saturating_sub(1) {
      let before = points[index.saturating_sub(1)];
      let (x1, y1) = points[index];
      let (x2, y2) = points[index + 1];
      let after = points[(index + 2).min(points.len() - 1)];
      let first = Point::new(
        x1 + (x2 - before.0) * TENSION,
        (y1 + (y2 - before.1) * TENSION).clamp(0.5, height),
      );
      let second = Point::new(
        x2 - (after.0 - x1) * TENSION,
        (y2 - (after.1 - y1) * TENSION).clamp(0.5, height),
      );
      path.bezier_curve_to(first, second, Point::new(x2, y2));
    }
    path.close();
  })
}

/// Barre de fond (`#bottom-visualizer`).
#[derive(Debug, Clone)]
pub struct BarCanvas {
  pub heights: [f32; BAR_HALF * 2],
  pub liquid: Vec<(f32, f32)>,
  pub connected: bool,
  pub color: Color,
}

impl<Message> canvas::Program<Message> for BarCanvas {
  type State = ();

  fn draw(
    &self,
    _state: &(),
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    const GAP: f32 = 2.0;
    let mut frame = Frame::new(renderer, bounds.size());
    let height = bounds.height;
    if self.connected {
      frame.fill(&liquid_path(&self.liquid, height), self.color);
    } else {
      let count = self.heights.len() as f32;
      let total_gap = GAP * (count - 1.0);
      let width = ((bounds.width - total_gap) / count).max(2.0);
      let start = ((bounds.width - (count * width + total_gap)) / 2.0).max(0.0);
      for (index, bar) in self.heights.iter().enumerate() {
        let bar = bar.clamp(2.0, height - 2.0);
        let x = start + index as f32 * (width + GAP);
        let path = Path::rounded_rectangle(
          Point::new(x, height - bar),
          Size::new(width, bar),
          3.0.into(),
        );
        frame.fill(&path, self.color);
      }
    }
    vec![frame.into_geometry()]
  }
}

/// Petit visualiseur de la barre de lecture (`#visualizer-canvas`).
#[derive(Debug, Clone)]
pub struct MiniBarsCanvas {
  pub heights: [f32; MINI_BARS],
  pub color: Color,
}

impl<Message> canvas::Program<Message> for MiniBarsCanvas {
  type State = ();

  fn draw(
    &self,
    _state: &(),
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    const BAR: f32 = 3.0;
    const GAP: f32 = 2.0;
    let mut frame = Frame::new(renderer, bounds.size());
    let start = (bounds.width - MINI_BARS as f32 * (BAR + GAP)) / 2.0;
    for (index, bar) in self.heights.iter().enumerate() {
      let path = Path::rounded_rectangle(
        Point::new(start + index as f32 * (BAR + GAP), MINI_HEIGHT - bar),
        Size::new(BAR, *bar),
        2.0.into(),
      );
      frame.fill(&path, self.color);
    }
    vec![frame.into_geometry()]
  }
}

/// Barres de la vue plein écran (`#tvk-visualizer-canvas`).
#[derive(Debug, Clone)]
pub struct TvkCanvas {
  pub heights: [f32; TVK_BARS],
  pub color: Color,
}

impl<Message> canvas::Program<Message> for TvkCanvas {
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
    // Le dessin de Riff est en pixels CSS (`app.js:2847-2857`) : une échelle passe aux pixels écran.
    frame.scale(crate::theme::tokens::ZOOM);
    let bounds = Size::new(
      bounds.width / crate::theme::tokens::ZOOM,
      bounds.height / crate::theme::tokens::ZOOM,
    );
    let count = TVK_BARS as f32;
    let gap = (bounds.width / 115.0).round().clamp(3.0, 6.0);
    let total_gap = gap * (count - 1.0);
    let width = ((bounds.width - total_gap) / count).floor().max(4.0);
    let start = ((bounds.width - (count * width + total_gap)) / 2.0).max(0.0);
    let radius = (width / 2.0).min(3.0);
    for (index, bar) in self.heights.iter().enumerate() {
      let x = start + index as f32 * (width + gap);
      let path = Path::rounded_rectangle(
        Point::new(x, bounds.height - bar),
        Size::new(width, *bar),
        radius.into(),
      );
      frame.fill(&path, self.color);
    }
    vec![frame.into_geometry()]
  }
}

/// Pochette en forme avec son anneau de progression (`m3cDraw`).
#[derive(Debug, Clone)]
pub struct ShapeCover {
  pub radii: Vec<f32>,
  pub rotation: f32,
  pub phase: f32,
  pub progress: f32,
  pub has_image: bool,
  pub ring: Color,
  pub background: Color,
  /// `--md-sys-color-surface-container-highest-solid` : remplissage de la forme sans image (`app.js:16913`).
  pub fill: Color,
  pub cover: f32,
  /// `m3c.hoverProgress` : de 0 à 1.
  pub hover: f32,
  /// Pixels écran par pixel CSS du canvas de 120 px de Riff (1 dans la barre, `side / 120` dans le mode compact).
  pub unit: f32,
  /// Centre de la pochette au repos dans le canvas ; `None` : le centre du canvas.
  pub center: Option<Point>,
  pub layer: CoverLayer,
}

/// Part du dessin de `m3cDraw`. Dans la barre, l'image et son masque restent coupés aux bords de la barre, mais l'anneau levé et
/// agrandi par le survol sort par le haut (`.playbar` et `.playbar-left` : `overflow: visible`, `styles.css:2862`, `5387`) : deux
/// canvas, l'un dans la barre coupée (`Body`), l'autre au-dessus de la page (`Ring`). Le mode compact dessine tout (`Both`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverLayer {
  Both,
  Body,
  Ring,
}

/// `RING_GAP` et `RING_W` de `m3cDraw` (`app.js:16770`) : 5 et 2,4 pixels CSS du canvas de 120 px.
const RING_GAP: f32 = css(5.0);
const RING_WIDTH: f32 = css(2.4);
/// Côté du canvas de Riff, `CANVAS_PX` (`app.js:16769`), en pixels écran dans la barre.
pub const CANVAS_CSS: f32 = css(120.0);

impl ShapeCover {
  /// Rayon interpolé à un angle de la forme, en pixels.
  fn radius_at(&self, angle: f32, scale: f32) -> f32 {
    let count = SAMPLE_COUNT as f32;
    let position = (angle + PI) / TAU * count;
    let wrapped = position.floor().rem_euclid(count);
    let first = wrapped as usize % SAMPLE_COUNT;
    let second = (first + 1) % SAMPLE_COUNT;
    let weight = position - position.floor();
    (self.radii[first] + (self.radii[second] - self.radii[first]) * weight) * scale
  }
}

/// `setLineDash([0.1, 5.5])` à la main : des traits de `length` tous les `pitch`, comptés le long de la ligne brisée depuis son
/// premier point. Le tiret qui franchit un sommet suit la direction du segment où il commence.
fn dash_segments(points: &[Point], pitch: f32, length: f32) -> Vec<(Point, Point)> {
  let mut segments = Vec::new();
  let mut start = 0.0_f32;
  let mut next = 0.0_f32;
  for pair in points.windows(2) {
    let (from, to) = (pair[0], pair[1]);
    let size = from.distance(to);
    if size <= f32::EPSILON {
      continue;
    }
    let direction = Vector::new((to.x - from.x) / size, (to.y - from.y) / size);
    while next <= start + size {
      let along = next - start;
      let begin = Point::new(from.x + direction.x * along, from.y + direction.y * along);
      let end = Point::new(
        begin.x + direction.x * length,
        begin.y + direction.y * length,
      );
      segments.push((begin, end));
      next += pitch;
    }
    start += size;
  }
  segments
}

fn dashes(points: &[Point], pitch: f32, length: f32) -> Path {
  Path::new(|path| {
    for (begin, end) in dash_segments(points, pitch, length) {
      path.move_to(begin);
      path.line_to(end);
    }
  })
}

impl<Message> canvas::Program<Message> for ShapeCover {
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
    let unit = self.unit;
    let (gap, width) = (RING_GAP * unit, RING_WIDTH * unit);
    // `hoverScale` et `hoverLift` (`app.js:16863-16866`) : la pochette grandit de 16 % et monte de 4 px, l'anneau suit.
    let cover = self.cover * (1.0 + halo::HOVER_SCALE * self.hover);
    let lift = css(halo::HOVER_LIFT) * unit * self.hover;
    let middle = self.center.unwrap_or_else(|| frame.center());
    let center = Point::new(middle.x, middle.y - lift);
    let scale = (cover / 2.0) / 160.0 * 1.02;
    let outline = |path: &mut canvas::path::Builder| {
      for sample in 0..=SAMPLE_COUNT {
        let angle = -PI + TAU * (sample % SAMPLE_COUNT) as f32 / SAMPLE_COUNT as f32;
        let radius = self.radii[sample % SAMPLE_COUNT] * scale;
        let point = center
          + Vector::new(
            (angle + self.rotation).cos() * radius,
            (angle + self.rotation).sin() * radius,
          );
        if sample == 0 {
          path.move_to(point);
        } else {
          path.line_to(point);
        }
      }
      path.close();
    };
    if self.layer != CoverLayer::Ring && self.has_image {
      // `Canvas` ne découpe pas une image selon un chemin, et dessine ses images au-dessus de ses formes : l'image est un widget
      // placé dessous dans un `Stack`, et le dehors de la forme est repeint à la couleur de la barre.
      // Seule l'image est masquée (carré de `ceil(cover * 1,06)`, plus 2 px) : le reste du canvas déborde sur
      // la barre et doit rester transparent.
      // Pendant le survol l'image, posée par le moteur à une position arrondie, déborde de 1 px : marge de 4 px.
      let side = (cover * halo::IMAGE_SCALE).ceil() + if self.hover > 0.0 { 4.0 } else { 2.0 };
      let mask = Path::new(|path| {
        path.rectangle(
          Point::new(center.x - side / 2.0, center.y - side / 2.0),
          Size::new(side, side),
        );
        outline(path);
      });
      frame.fill(
        &mask,
        Fill {
          style: Style::Solid(self.background),
          rule: fill::Rule::EvenOdd,
        },
      );
    } else if self.layer != CoverLayer::Ring {
      frame.fill(&Path::new(|path| outline(path)), self.fill);
    }
    if self.layer == CoverLayer::Body {
      return vec![frame.into_geometry()];
    }
    let ring_point = |angle: f32, wave: f32| {
      let radius = self.radius_at(angle - self.rotation, scale) + gap + width / 2.0 + wave;
      center + Vector::new(angle.cos() * radius, angle.sin() * radius)
    };
    let steps = halo::RING_STEPS;
    let start = -PI / 2.0;
    let angle_of = |step: usize| start + TAU * step as f32 / steps as f32;
    let rest_from = (self.progress * steps as f32).round() as usize;
    let rest: Vec<Point> = (rest_from..=steps)
      .map(|step| ring_point(angle_of(step), 0.0))
      .collect();
    frame.stroke(
      &dashes(
        &rest,
        css(halo::DASH_PITCH) * unit,
        css(halo::DASH_LENGTH) * unit,
      ),
      Stroke {
        style: Style::Solid(Color {
          a: halo::REST_ALPHA,
          ..self.ring
        }),
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
      },
    );
    if self.progress > 0.0 {
      let played = ((self.progress * steps as f32).round() as usize).max(2);
      let path = Path::new(|path| {
        for step in 0..=played {
          let angle = angle_of(step);
          let wave =
            (angle * halo::WAVE_LOBES + self.phase).sin() * css(halo::WAVE_AMPLITUDE) * unit;
          let point = ring_point(angle, wave);
          if step == 0 {
            path.move_to(point);
          } else {
            path.line_to(point);
          }
        }
      });
      frame.stroke(
        &path,
        Stroke {
          style: Style::Solid(self.ring),
          width,
          line_cap: LineCap::Round,
          line_join: LineJoin::Round,
          ..Stroke::default()
        },
      );
    }
    vec![frame.into_geometry()]
  }
}

/// Rayons courants de la forme.
#[must_use]
pub fn shape_radii(shape: &viz::CoverShape, profiles: &[Vec<f32>]) -> Vec<f32> {
  (0..SAMPLE_COUNT)
    .map(|sample| shape.radius(profiles, sample))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_dashes_start_at_the_first_point_and_repeat_every_pitch_along_the_polyline() {
    // Une ligne brisée de 12 px (6 + 6) : des tirets en 0, 5,6 et 11,2 px, comme `setLineDash([0.1, 5.5])`.
    let points = [
      Point::new(0.0, 0.0),
      Point::new(6.0, 0.0),
      Point::new(6.0, 6.0),
    ];
    let dashes = dash_segments(&points, 5.6, 0.1);
    assert_eq!(dashes.len(), 3);
    assert_eq!(dashes[0].0, Point::new(0.0, 0.0));
    assert!((dashes[1].0.x - 5.6).abs() < 1e-5 && dashes[1].0.y == 0.0);
    assert!((dashes[2].0.y - 5.2).abs() < 1e-4, "{:?}", dashes[2]);
    assert!((dashes[0].1.x - 0.1).abs() < 1e-6);
  }

  #[test]
  fn a_ring_without_rest_has_no_dash() {
    assert!(dash_segments(&[Point::new(1.0, 1.0)], 5.6, 0.1).is_empty());
  }
}

/// Raccords concaves du panneau du karaoké (`.tvk-panel::before` et `::after`, `styles.css:8885-8902`). Le canvas fait la largeur du
/// panneau plus deux marges de `radius` ; chaque raccord est le coin haut d'un carré de `radius`, creusé d'un quart de disque centré
/// sur le coin bas extérieur du carré (`radial-gradient(circle 26px at 0 100%, transparent 25.5px, fond 26px)`).
#[derive(Debug, Clone, Copy)]
pub struct PanelEars {
  pub radius: f32,
  pub color: Color,
}

impl PanelEars {
  /// Quart de cercle de rayon `r` en Bézier cubique : la poignée vaut `4/3 * (sqrt(2) - 1) * r`.
  const KAPPA: f32 = 0.552_284_8;

  fn path(self, width: f32) -> Path {
    let r = self.radius;
    let k = Self::KAPPA * r;
    Path::new(|path| {
      // Gauche : carré `[0, r]`, centre du disque en `(0, r)`.
      path.move_to(Point::new(0.0, 0.0));
      path.line_to(Point::new(r, 0.0));
      path.line_to(Point::new(r, r));
      path.bezier_curve_to(
        Point::new(r, r - k),
        Point::new(k, 0.0),
        Point::new(0.0, 0.0),
      );
      path.close();
      // Droite : carré `[width - r, width]`, centre du disque en `(width, r)`.
      path.move_to(Point::new(width, 0.0));
      path.line_to(Point::new(width - r, 0.0));
      path.line_to(Point::new(width - r, r));
      path.bezier_curve_to(
        Point::new(width - r, r - k),
        Point::new(width - k, 0.0),
        Point::new(width, 0.0),
      );
      path.close();
    })
  }
}

impl<Message> canvas::Program<Message> for PanelEars {
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
    frame.fill(&self.path(bounds.width), self.color);
    vec![frame.into_geometry()]
  }
}
