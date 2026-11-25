//! Onde `rippleOut` d'un bouton (`styles.css:7502-7504`, `app.js:2622-2638`) et messages d'appui et de survol du widget [`Focus`].
//!
//! iced 0.14 ne découpe qu'en rectangle et ne floute rien : la coupe arrondie de `.ripple-host` (`overflow: hidden` avec
//! `border-radius: inherit`) est faite en géométrie, par l'intersection exacte du disque et du rectangle arrondi, et le
//! `blur(2px)` du disque par cinq disques concentriques qui portent chacun un cinquième de l'opacité (quantiles d'une loi normale).
//!
//! [`Focus`]: super::focus::Focus

use super::hover::Hover;
use crate::motion::{self, ControlKey, RipplePose};
use crate::theme::tokens::touch as tokens;
use crate::ui::UiMessage;
use iced::advanced::graphics::geometry;
use iced::advanced::renderer;
use iced::widget::canvas::{Fill, Path};
use iced::{Color, Point, Rectangle, Size, Theme, Vector};

/// Côtés du polygone d'un disque : la flèche de l'arc reste sous 0,01 px pour un rayon de 100 px.
const CIRCLE_SIDES: usize = 64;
/// Côtés de l'arc d'un coin arrondi.
const CORNER_SIDES: usize = 12;

/// Ce que `Focus` publie et dessine pour un bouton : survol, appui avec point du clic, relâchement, ondes en cours.
pub struct Touch<'a, Message> {
  pub(crate) hover: Option<Hover<'a, Message>>,
  pub(crate) press: Box<dyn Fn(Point, Size) -> Message + 'a>,
  pub(crate) release: Message,
  pub(crate) ripples: Vec<RipplePose>,
  /// `currentColor` du bouton (`styles.css:7503`).
  pub(crate) color: Box<dyn Fn(&Theme) -> Color + 'a>,
  /// Rayon de coin courant du bouton : celui de `border-radius: inherit`.
  pub(crate) radius: f32,
}

/// Message qui sait porter un [`UiMessage`] : celui de l'application. Les autres (actions de la barre de titre) n'ont ni survol animé
/// ni onde.
pub trait Touchable: Clone + Sized {
  fn from_ui(message: UiMessage) -> Option<Self>;
}

impl Touchable for crate::app::Message {
  fn from_ui(message: UiMessage) -> Option<Self> {
    Some(Self::Ui(message))
  }
}

impl Touchable for crate::window::Action {
  fn from_ui(_: UiMessage) -> Option<Self> {
    None
  }
}

impl<'a, Message: Touchable + 'a> Touch<'a, Message> {
  /// Appui et onde de `key` ; le survol animé est ajouté par [`Touch::hover`]. `None` pour un message qui ne porte pas `UiMessage`.
  #[must_use]
  pub fn new(key: ControlKey, color: impl Fn(&Theme) -> Color + 'a, radius: f32) -> Option<Self> {
    let release = Message::from_ui(UiMessage::Touch(key, None))?;
    let fallback = release.clone();
    Some(Self {
      hover: None,
      press: Box::new(move |origin, size| {
        Message::from_ui(UiMessage::Touch(key, Some((origin, size))))
          .unwrap_or_else(|| fallback.clone())
      }),
      release,
      ripples: motion::view_ripples(key),
      color: Box::new(color),
      radius,
    })
  }

  /// Survol animé de `key` : fond, calque `--hl` et rayon (`styles.css:7505-7524`).
  #[must_use]
  pub fn hover(mut self, key: ControlKey) -> Self {
    let fallback = self.release.clone();
    self.hover = Some(Hover {
      key,
      send: Box::new(move |message| Message::from_ui(message).unwrap_or_else(|| fallback.clone())),
    });
    self
  }
}

/// Part d'opacité de chacun des disques empilés pour que leur composition donne `alpha` à l'intérieur.
fn layer_alpha(alpha: f32) -> f32 {
  1.0 - (1.0 - alpha).powf(1.0 / tokens::RIPPLE_EDGE.len() as f32)
}

/// Rayons des disques empilés pour un disque de rayon `radius` : le bord flou (écart type `RIPPLE_BLUR`) est étagé en cinq marches.
fn layer_radii(radius: f32) -> impl Iterator<Item = f32> {
  tokens::RIPPLE_EDGE
    .into_iter()
    .map(move |offset| radius + offset * tokens::RIPPLE_BLUR)
    .filter(|radius| *radius > 0.0)
}

fn circle_polygon(center: Point, radius: f32) -> Vec<Point> {
  (0..CIRCLE_SIDES)
    .map(|side| {
      let angle = std::f32::consts::TAU * side as f32 / CIRCLE_SIDES as f32;
      Point::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
      )
    })
    .collect()
}

/// Contour de `[0, size]` aux coins de rayon `radius` (borné à la moitié du petit côté, comme CSS).
fn rounded_polygon(size: Size, radius: f32) -> Vec<Point> {
  let radius = radius.min(size.width.min(size.height) / 2.0).max(0.0);
  if radius < 0.25 {
    return vec![
      Point::new(0.0, 0.0),
      Point::new(size.width, 0.0),
      Point::new(size.width, size.height),
      Point::new(0.0, size.height),
    ];
  }
  // Centres des quatre arcs, dans l'ordre haut gauche, haut droit, bas droit, bas gauche, et angle de départ de chacun.
  let corners = [
    (Point::new(radius, radius), std::f32::consts::PI),
    (
      Point::new(size.width - radius, radius),
      1.5 * std::f32::consts::PI,
    ),
    (Point::new(size.width - radius, size.height - radius), 0.0),
    (
      Point::new(radius, size.height - radius),
      0.5 * std::f32::consts::PI,
    ),
  ];
  let mut points = Vec::with_capacity(4 * (CORNER_SIDES + 1));
  for (center, start) in corners {
    for step in 0..=CORNER_SIDES {
      let angle = start + std::f32::consts::FRAC_PI_2 * step as f32 / CORNER_SIDES as f32;
      points.push(Point::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
      ));
    }
  }
  // Une pilule fait coïncider la fin d'un arc et le début du suivant : les arêtes de longueur nulle fausseraient le test de côté.
  points.dedup_by(|a, b| (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3);
  if points.len() > 1
    && (points[0].x - points[points.len() - 1].x).abs() < 1e-3
    && (points[0].y - points[points.len() - 1].y).abs() < 1e-3
  {
    points.pop();
  }
  points
}

fn signed_area(polygon: &[Point]) -> f32 {
  polygon
    .iter()
    .zip(polygon.iter().cycle().skip(1))
    .map(|(a, b)| a.x * b.y - b.x * a.y)
    .sum::<f32>()
    / 2.0
}

/// Intersection de `subject` avec le polygone convexe `clip` (Sutherland-Hodgman).
fn clip_polygon(subject: &[Point], clip: &[Point]) -> Vec<Point> {
  let sign = signed_area(clip).signum();
  let mut output = subject.to_vec();
  for (index, a) in clip.iter().enumerate() {
    let b = clip[(index + 1) % clip.len()];
    let side = |p: Point| ((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)) * sign;
    let input = std::mem::take(&mut output);
    let Some(mut previous) = input.last().copied() else {
      break;
    };
    for current in input {
      let (inside_now, inside_before) = (side(current) >= 0.0, side(previous) >= 0.0);
      if inside_now != inside_before {
        let (from, to) = (side(previous), side(current));
        let t = from / (from - to);
        output.push(Point::new(
          previous.x + t * (current.x - previous.x),
          previous.y + t * (current.y - previous.y),
        ));
      }
      if inside_now {
        output.push(current);
      }
      previous = current;
    }
  }
  output
}

/// Surface de `polygon`, pour les tests.
#[cfg(test)]
fn area(polygon: &[Point]) -> f32 {
  signed_area(polygon).abs()
}

/// Contours (coordonnées du bouton) des disques d'une onde, coupés au rectangle arrondi du bouton, du plus petit au plus grand.
fn ripple_shapes(pose: &RipplePose, size: Size, radius: f32) -> Vec<Vec<Point>> {
  let clip = rounded_polygon(size, radius);
  layer_radii(pose.radius)
    .map(|layer| clip_polygon(&circle_polygon(pose.origin, layer), &clip))
    .filter(|shape| shape.len() >= 3)
    .collect()
}

/// Dessine les ondes dans `bounds`, au-dessus du contenu du bouton.
pub fn draw_ripples<Renderer>(
  renderer: &mut Renderer,
  bounds: Rectangle,
  touch: &Touch<'_, impl Sized>,
  theme: &Theme,
) where
  Renderer: renderer::Renderer + geometry::Renderer,
{
  let visible: Vec<&RipplePose> = touch
    .ripples
    .iter()
    .filter(|pose| pose.alpha > 0.0005 && pose.radius > 0.0)
    .collect();
  if visible.is_empty() || bounds.width < 1.0 || bounds.height < 1.0 {
    return;
  }
  let color = (touch.color)(theme);
  let shapes: Vec<(Color, Vec<Point>)> = visible
    .into_iter()
    .flat_map(|pose| {
      let tint = color.scale_alpha(layer_alpha(pose.alpha));
      ripple_shapes(pose, bounds.size(), touch.radius)
        .into_iter()
        .map(move |shape| (tint, shape))
    })
    .collect();
  // Disques entièrement hors du bouton : rien à dessiner, donc pas de couche.
  if shapes.is_empty() {
    return;
  }
  let mut frame = geometry::Frame::new(renderer, bounds.size());
  for (tint, shape) in shapes {
    let path = Path::new(|builder| {
      builder.move_to(shape[0]);
      for point in &shape[1..] {
        builder.line_to(*point);
      }
      builder.close();
    });
    frame.fill(&path, Fill::from(tint));
  }
  // `.ripple-host` est le dernier enfant du bouton (`app.js:2630`) : l'onde est peinte par-dessus le libellé et l'icône. iced peint le
  // texte après la géométrie d'une même couche, donc l'onde a sa couche ; elle n'est ouverte que si une onde est visible (règle 8 :
  // jamais de couche vide), et son cadre déborde de 1 px pour que l'arrondi de la découpe ne rogne pas le bord adouci.
  renderer.with_layer(bounds.expand(1.0), |renderer| {
    renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
      renderer.draw_geometry(frame.into_geometry());
    });
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  const BOX: Size = Size::new(100.0, 36.0);

  fn near(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
  }

  #[test]
  fn the_rounded_polygon_has_the_area_of_a_rounded_rectangle() {
    // Le polygone inscrit perd 0,3 % de l'arc : la tolérance est de 3 px² sur 3 500.
    let radius = 10.0_f32;
    let exact = BOX.width * BOX.height - (4.0 - std::f32::consts::PI) * radius * radius;
    assert!(near(area(&rounded_polygon(BOX, radius)), exact, 3.0));
    // CSS borne le rayon à la moitié du petit côté : 28 px sur 36 px de haut donne une pilule de rayon 18.
    let pill = BOX.width * BOX.height - (4.0 - std::f32::consts::PI) * 18.0 * 18.0;
    assert!(near(area(&rounded_polygon(BOX, 28.0)), pill, 3.0));
    assert!(near(
      area(&rounded_polygon(BOX, 0.0)),
      BOX.width * BOX.height,
      1e-3
    ));
  }

  #[test]
  fn a_disc_inside_the_button_is_not_cut() {
    let disc = circle_polygon(Point::new(50.0, 18.0), 10.0);
    let clipped = clip_polygon(&disc, &rounded_polygon(BOX, 18.0));
    assert!(near(area(&clipped), area(&disc), 1e-2));
  }

  #[test]
  fn a_disc_covering_the_button_becomes_the_rounded_rectangle() {
    let disc = circle_polygon(Point::new(50.0, 18.0), 400.0);
    let clip = rounded_polygon(BOX, 18.0);
    let clipped = clip_polygon(&disc, &clip);
    assert!(near(area(&clipped), area(&clip), 0.5));
  }

  #[test]
  fn the_cut_follows_the_corner_arc_where_a_square_clip_would_spill() {
    // Disque de rayon 30 posé dans le coin haut gauche (clic à 4 px des bords) : une coupe rectangulaire garderait le coin carré.
    let radius = 18.0;
    let disc = circle_polygon(Point::new(4.0, 4.0), 30.0);
    let rounded = area(&clip_polygon(&disc, &rounded_polygon(BOX, radius)));
    let square = area(&clip_polygon(&disc, &rounded_polygon(BOX, 0.0)));
    // Le coin carré de côté `radius` dépasse l'arc de `radius^2 (1 - pi/4)` : l'écart que la coupe rectangulaire aurait laissé.
    let corner = radius * radius * (1.0 - std::f32::consts::FRAC_PI_4);
    assert!(
      square - rounded > 0.8 * corner,
      "{square} {rounded} {corner}"
    );
  }

  #[test]
  fn five_layers_compose_to_the_requested_opacity() {
    let alpha = 0.16;
    let layer = layer_alpha(alpha);
    let composite = 1.0 - (1.0 - layer).powi(5);
    assert!(near(composite, alpha, 1e-6));
  }

  #[test]
  fn the_blurred_edge_spans_the_gaussian_quantiles() {
    let radii: Vec<f32> = layer_radii(20.0).collect();
    assert_eq!(radii.len(), 5);
    assert!(near(radii[2], 20.0, 1e-6));
    assert!(near(
      radii[4] - radii[0],
      2.0 * 1.2816 * tokens::RIPPLE_BLUR,
      1e-4
    ));
    // Un disque plus petit que le flou perd les marches de rayon négatif.
    assert!(layer_radii(0.5).count() < 5);
    assert_eq!(layer_radii(0.0).count(), 2);
  }
}
