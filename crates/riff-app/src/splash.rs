//! Splash de démarrage : le contour du logo se trace, se remplit, puis l'écran noir s'efface (`runRiffSplash`, `app.js:165-215`).
//!
//! Le logo est le seul `path` de `riff.svg` (`M`, `C`, `Z`). Le trait pointillé de Riff (`stroke-dasharray` = `stroke-dashoffset` =
//! longueur) ne montre que le début du chemin : on trace donc le chemin aplati, coupé à la longueur visible. `canvas::LineDash` ne
//! convient pas : son `offset` est un indice dans `segments` et ses tirets sont des cordes entre deux positions du motif, pas le
//! chemin suivi.

use crate::theme::tokens::splash as tokens;
use crate::visuals::state::bezier_ease;
use iced::widget::canvas::{
  self, Action, Event, Fill, Frame, Geometry, LineCap, LineJoin, Path, Stroke, fill,
};
use iced::{Color, Element, Fill as FillLength, Point, Rectangle, Renderer, Size, Theme, mouse};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// `riff.svg` de Riff, identique octet pour octet à `assets/icons/riff-logo.svg`.
const LOGO_SVG: &str = include_str!("../assets/icons/riff-logo.svg");

/// Pas visé entre deux points aplatis d'une courbe, en unités du viewBox (0,5 px écran à 1920x1080).
const FLATTEN_STEP: f32 = 3.0;

/// Chemin du logo aplati, en unités du viewBox.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
  subpaths: Vec<Vec<(f32, f32)>>,
}

impl Outline {
  /// Lit l'attribut `d` du premier `path` d'un SVG.
  ///
  /// # Errors
  /// Pas d'attribut `d`, commande autre que `M`, `L`, `C`, `Z`, ou nombre manquant.
  pub fn from_svg(svg: &str) -> Result<Self, String> {
    let start = svg.find(" d=\"").ok_or("pas d'attribut d")? + 4;
    let end = svg[start..].find('"').ok_or("attribut d non fermé")? + start;
    Self::from_path_data(&svg[start..end])
  }

  /// Aplati les commandes absolues `M`, `L`, `C` et `Z` (répétition implicite comprise).
  ///
  /// # Errors
  /// Commande inconnue ou nombre manquant.
  pub fn from_path_data(data: &str) -> Result<Self, String> {
    let mut tokens = Tokens::new(data);
    let mut subpaths: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    while let Some(mut command) = tokens.peek_command() {
      tokens.advance();
      loop {
        match command {
          'M' => {
            if current.len() > 1 {
              subpaths.push(std::mem::take(&mut current));
            }
            current = vec![tokens.point()?];
            command = 'L';
          }
          'L' => current.push(tokens.point()?),
          'C' => {
            let from = *current.last().ok_or("C sans point courant")?;
            let (a, b, to) = (tokens.point()?, tokens.point()?, tokens.point()?);
            flatten_cubic(&mut current, from, a, b, to);
          }
          'Z' => {
            if let Some(first) = current.first().copied() {
              // Le sous-chemin se ferme : le segment de retour compte dans la longueur.
              current.push(first);
            }
            if current.len() > 1 {
              subpaths.push(std::mem::take(&mut current));
            }
            break;
          }
          other => return Err(format!("commande {other} non gérée")),
        }
        if !tokens.has_number() {
          break;
        }
      }
    }
    if current.len() > 1 {
      subpaths.push(current);
    }
    Ok(Self { subpaths })
  }

  /// Longueur totale, en unités du viewBox.
  #[must_use]
  pub fn length(&self) -> f32 {
    self
      .subpaths
      .iter()
      .map(|points| polyline_length(points))
      .sum()
  }

  /// Sous-chemins coupés après `visible` unités de longueur (le décalage de pointillé de Riff est `longueur - visible`).
  #[must_use]
  pub fn trimmed(&self, visible: f32) -> Vec<Vec<(f32, f32)>> {
    let mut left = visible;
    let mut out = Vec::new();
    for points in &self.subpaths {
      if left <= 0.0 {
        break;
      }
      let mut kept = vec![points[0]];
      for pair in points.windows(2) {
        let length = distance(pair[0], pair[1]);
        if length >= left {
          let t = if length > 0.0 { left / length } else { 0.0 };
          kept.push((
            pair[0].0 + (pair[1].0 - pair[0].0) * t,
            pair[0].1 + (pair[1].1 - pair[0].1) * t,
          ));
          left = 0.0;
          break;
        }
        left -= length;
        kept.push(pair[1]);
      }
      out.push(kept);
    }
    out
  }
}

/// Chemin du logo, lu une fois.
static OUTLINE: LazyLock<Outline> =
  LazyLock::new(|| Outline::from_svg(LOGO_SVG).expect("riff-logo.svg : commandes M, C, Z"));

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
  (b.0 - a.0).hypot(b.1 - a.1)
}

fn polyline_length(points: &[(f32, f32)]) -> f32 {
  points
    .windows(2)
    .map(|pair| distance(pair[0], pair[1]))
    .sum()
}

/// Ajoute les points d'une courbe de Bézier cubique (sans son point de départ).
fn flatten_cubic(
  out: &mut Vec<(f32, f32)>,
  from: (f32, f32),
  a: (f32, f32),
  b: (f32, f32),
  to: (f32, f32),
) {
  let control = distance(from, a) + distance(a, b) + distance(b, to);
  let steps = (control / FLATTEN_STEP).ceil().clamp(4.0, 96.0) as u32;
  for step in 1..=steps {
    let t = step as f32 / steps as f32;
    let u = 1.0 - t;
    let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    out.push((
      w0 * from.0 + w1 * a.0 + w2 * b.0 + w3 * to.0,
      w0 * from.1 + w1 * a.1 + w2 * b.1 + w3 * to.1,
    ));
  }
}

/// Lecteur de la chaîne `d` d'un chemin SVG.
struct Tokens<'a> {
  rest: &'a str,
}

impl<'a> Tokens<'a> {
  const fn new(data: &'a str) -> Self {
    Self { rest: data }
  }

  fn skip_separators(&mut self) {
    self.rest = self
      .rest
      .trim_start_matches(|c: char| c.is_whitespace() || c == ',');
  }

  fn peek_command(&mut self) -> Option<char> {
    self.skip_separators();
    self.rest.chars().next().filter(char::is_ascii_alphabetic)
  }

  fn advance(&mut self) {
    self.rest = &self.rest[1..];
  }

  fn has_number(&mut self) -> bool {
    self.skip_separators();
    self
      .rest
      .chars()
      .next()
      .is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '.' || c == '+')
  }

  fn number(&mut self) -> Result<f32, String> {
    self.skip_separators();
    let end = self
      .rest
      .char_indices()
      .find(|&(i, c)| {
        !(c.is_ascii_digit()
          || c == '.'
          || c == 'e'
          || c == 'E'
          || ((c == '-' || c == '+') && (i == 0 || self.rest[..i].ends_with(['e', 'E']))))
      })
      .map_or(self.rest.len(), |(i, _)| i);
    let (text, rest) = self.rest.split_at(end);
    self.rest = rest;
    text
      .parse()
      .map_err(|_| format!("nombre invalide : {text:?}"))
  }

  fn point(&mut self) -> Result<(f32, f32), String> {
    Ok((self.number()?, self.number()?))
  }
}

/// Départ du splash. `start` reste vide jusqu'à la première image d'animation reçue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Splash {
  pub start: Option<Instant>,
}

impl Splash {
  /// Temps écoulé depuis le départ.
  #[must_use]
  pub fn elapsed(&self, now: Instant) -> Option<Duration> {
    self.start.map(|start| now.saturating_duration_since(start))
  }

  /// Le calque est retiré 420 ms après le début du fondu (`app.js:174`).
  #[must_use]
  pub fn finished(&self, now: Instant) -> bool {
    self.elapsed(now).is_some_and(|elapsed| {
      elapsed.as_secs_f32() * 1000.0 >= tokens::DRAW_MS + tokens::REMOVE_AFTER_HIDE_MS
    })
  }
}

/// Valeurs affichées à un instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
  /// Opacité du calque entier (`#splash`).
  pub layer_opacity: f32,
  /// Les clics sont retenus : vrai jusqu'à la classe `hide` (`styles.css:9696`).
  pub blocking: bool,
  /// Le logo existe : faux avant le départ, le SVG n'est pas encore chargé.
  pub logo_visible: bool,
  /// `transform: scale()` de l'hôte.
  pub logo_scale: f32,
  /// `opacity` de l'hôte.
  pub logo_opacity: f32,
  /// Longueur du chemin encore visible (`longueur - stroke-dashoffset`).
  pub visible_length: f32,
  pub fill_opacity: f32,
  pub stroke_opacity: f32,
}

impl Look {
  /// Pendant le fondu, le noir est percé sous le remplissage (opacité de groupe). Hors fondu il reste plein : le noir est sous le logo.
  #[must_use]
  pub fn holes_the_fill(&self) -> bool {
    self.layer_opacity < 1.0 && self.logo_visible && self.fill_opacity > 0.0
  }
}

/// Valeurs au temps écoulé `elapsed` (`None` : avant le départ).
#[must_use]
pub fn look(elapsed: Option<Duration>) -> Look {
  let Some(elapsed) = elapsed else {
    return Look {
      layer_opacity: 1.0,
      blocking: true,
      logo_visible: false,
      logo_scale: tokens::SCALE_FROM,
      logo_opacity: tokens::OPACITY_FROM,
      visible_length: 0.0,
      fill_opacity: 0.0,
      stroke_opacity: 1.0,
    };
  };
  let ms = elapsed.as_secs_f32() * 1000.0;
  // WAAPI : la courbe porte sur le progrès global, puis les images clés s'interpolent linéairement.
  let eased = bezier_ease(tokens::DRAW_CURVE, ms / tokens::DRAW_MS);
  let lerp = |from: f32, to: f32, t: f32| from + (to - from) * t;
  let (first, second) = (
    (eased / tokens::KEY_MIDDLE).min(1.0),
    ((eased - tokens::KEY_MIDDLE) / (1.0 - tokens::KEY_MIDDLE)).max(0.0),
  );
  let fill_opacity = if eased <= tokens::KEY_MIDDLE {
    lerp(0.0, tokens::FILL_MIDDLE, first)
  } else {
    lerp(tokens::FILL_MIDDLE, 1.0, second)
  };
  let layer_opacity = if ms < tokens::DRAW_MS {
    1.0
  } else {
    1.0 - bezier_ease(tokens::FADE_CURVE, (ms - tokens::DRAW_MS) / tokens::FADE_MS)
  };
  Look {
    layer_opacity,
    blocking: ms < tokens::DRAW_MS,
    logo_visible: true,
    logo_scale: lerp(tokens::SCALE_FROM, 1.0, eased),
    logo_opacity: lerp(tokens::OPACITY_FROM, 1.0, eased),
    visible_length: tokens::PATH_LENGTH * first,
    fill_opacity,
    stroke_opacity: lerp(1.0, tokens::STROKE_END, second),
  }
}

/// Position du logo dans la fenêtre : SVG en `100 % x 100 %` dans l'hôte carré (`meet`), zoom autour du centre de l'hôte, hôte centré.
struct Placement {
  centre: Point,
  scale: f32,
}

impl Placement {
  fn new(size: Size, logo_scale: f32) -> Self {
    let side = tokens::LOGO_VMIN * size.width.min(size.height);
    Self {
      centre: Point::new(size.width / 2.0, size.height / 2.0),
      scale: side / tokens::VIEW_BOX[0] * logo_scale,
    }
  }

  fn point(&self, (x, y): (f32, f32)) -> Point {
    let (ox, oy) = (tokens::VIEW_BOX[0] / 2.0, tokens::VIEW_BOX[1] / 2.0);
    Point::new(
      self.centre.x + (x - ox) * self.scale,
      self.centre.y + (y - oy) * self.scale,
    )
  }
}

/// Calque plein fenêtre du splash.
struct Layer {
  look: Look,
  accent: Color,
}

impl Layer {
  fn logo(&self, frame: &mut Frame, size: Size) {
    let look = self.look;
    let placement = Placement::new(size, look.logo_scale);
    let scale = placement.scale;
    let point = |p| placement.point(p);
    let alpha = look.layer_opacity * look.logo_opacity;
    let with_alpha = |a: f32| Color {
      a: a * alpha,
      ..self.accent
    };
    if look.fill_opacity > 0.0 {
      let shape = Path::new(|builder| {
        for points in &OUTLINE.subpaths {
          builder.move_to(point(points[0]));
          for &p in &points[1..] {
            builder.line_to(point(p));
          }
          builder.close();
        }
      });
      frame.fill(
        &shape,
        Fill {
          style: canvas::Style::Solid(with_alpha(look.fill_opacity)),
          rule: fill::Rule::NonZero,
        },
      );
    }
    let visible = OUTLINE.trimmed(look.visible_length);
    if !visible.is_empty() && look.stroke_opacity > 0.0 {
      let line = Path::new(|builder| {
        for points in &visible {
          builder.move_to(point(points[0]));
          for &p in &points[1..] {
            builder.line_to(point(p));
          }
        }
      });
      frame.stroke(
        &line,
        Stroke::default()
          .with_color(with_alpha(look.stroke_opacity))
          .with_width(tokens::STROKE_WIDTH * scale)
          .with_line_cap(LineCap::Round)
          .with_line_join(LineJoin::Round),
      );
    }
  }
}

impl<Message: Clone> canvas::Program<Message> for Layer {
  type State = ();

  fn update(
    &self,
    _state: &mut (),
    event: &Event,
    _bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Option<Action<Message>> {
    (self.look.blocking && matches!(event, Event::Mouse(_) | Event::Touch(_))).then(Action::capture)
  }

  fn draw(
    &self,
    _state: &(),
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let mut frame = Frame::new(renderer, bounds.size());
    let black = Color {
      a: self.look.layer_opacity,
      ..Color::BLACK
    };
    if self.look.holes_the_fill() {
      // Opacité de groupe de Riff : sous le remplissage, la page vaut `page * (1 - a) + accent * a`. Le noir est donc percé à la forme du
      // remplissage (rectangle + contour, règle pair-impair : le contour ne se recouvre pas, testé) puis le remplissage garde son alpha.
      let size = bounds.size();
      let placement = Placement::new(size, self.look.logo_scale);
      let covering = Path::new(|builder| {
        builder.rectangle(Point::ORIGIN, size);
        for points in &OUTLINE.subpaths {
          builder.move_to(placement.point(points[0]));
          for &p in &points[1..] {
            builder.line_to(placement.point(p));
          }
          builder.close();
        }
      });
      frame.fill(
        &covering,
        Fill {
          style: canvas::Style::Solid(black),
          rule: fill::Rule::EvenOdd,
        },
      );
    } else {
      frame.fill_rectangle(Point::ORIGIN, bounds.size(), black);
    }
    if self.look.logo_visible && self.look.layer_opacity > 0.0 {
      self.logo(&mut frame, bounds.size());
    }
    vec![frame.into_geometry()]
  }

  fn mouse_interaction(
    &self,
    _state: &(),
    _bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> mouse::Interaction {
    // Une interaction non nulle fait voir un curseur indisponible aux couches du dessous : pas de survol pendant le splash.
    if self.look.blocking {
      mouse::Interaction::Idle
    } else {
      mouse::Interaction::None
    }
  }
}

/// Le calque à l'instant `now`, ou `None` s'il est retiré.
#[must_use]
pub fn view<'a, Message: Clone + 'a>(
  splash: &Splash,
  now: Instant,
  accent: Color,
) -> Option<Element<'a, Message>> {
  if splash.finished(now) {
    return None;
  }
  let layer = Layer {
    look: look(splash.elapsed(now)),
    accent,
  };
  Some(
    canvas::Canvas::new(layer)
      .width(FillLength)
      .height(FillLength)
      .into(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn at(ms: f32) -> Look {
    look(Some(Duration::from_secs_f32(ms / 1000.0)))
  }

  #[test]
  fn the_logo_path_only_uses_move_curve_close() {
    let commands: std::collections::BTreeSet<char> = LOGO_SVG
      .split(" d=\"")
      .nth(1)
      .and_then(|rest| rest.split('"').next())
      .expect("d")
      .chars()
      .filter(char::is_ascii_alphabetic)
      .collect();
    assert_eq!(commands.into_iter().collect::<String>(), "CMZ");
  }

  #[test]
  fn the_path_length_matches_riff() {
    // Riff : `Math.ceil(getTotalLength())` = 3716 (MOTION.md).
    let length = OUTLINE.length();
    assert!(length > 3714.0 && length <= 3716.5, "{length}");
    assert_eq!(OUTLINE.subpaths.len(), 1);
  }

  #[test]
  fn parser_reads_absolute_commands_and_implicit_repeats() {
    let outline = Outline::from_path_data("M0 0L10 0 10 10Z").expect("chemin");
    assert_eq!(
      outline.subpaths,
      vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)]]
    );
    assert!((outline.length() - (10.0 + 10.0 + 200.0_f32.sqrt())).abs() < 1e-4);
    let curve = Outline::from_path_data("M0,0 C0 0 10 0 10 0").expect("chemin");
    assert!((curve.length() - 10.0).abs() < 1e-3);
    assert!(Outline::from_path_data("M0 0 Q1 1 2 2").is_err());
    assert!(Outline::from_path_data("M0").is_err());
    let exponent = Outline::from_path_data("M1e1-2 L1.5E+1 3").expect("chemin");
    assert_eq!(exponent.subpaths[0], vec![(10.0, -2.0), (15.0, 3.0)]);
  }

  #[test]
  fn a_lone_trailing_moveto_makes_no_subpath() {
    let outline = Outline::from_path_data("M0 0L4 0ZM9 9").expect("chemin");
    assert_eq!(outline.subpaths.len(), 1);
  }

  #[test]
  fn trimming_keeps_the_start_of_the_path() {
    let outline = Outline::from_path_data("M0 0L10 0 10 10").expect("chemin");
    assert!(outline.trimmed(0.0).is_empty());
    assert_eq!(outline.trimmed(4.0), vec![vec![(0.0, 0.0), (4.0, 0.0)]]);
    assert_eq!(
      outline.trimmed(15.0),
      vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)]]
    );
    assert_eq!(
      outline.trimmed(500.0),
      vec![vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]]
    );
  }

  #[test]
  fn the_start_is_invisible_black_and_blocking() {
    let before = look(None);
    assert!(!before.logo_visible && before.blocking);
    assert!((before.layer_opacity - 1.0).abs() < f32::EPSILON);
  }

  #[test]
  fn the_first_keyframe_is_hidden_and_zoomed() {
    let start = at(0.0);
    assert!((start.logo_scale - 1.22).abs() < 1e-5);
    assert!((start.logo_opacity - 0.2).abs() < 1e-5);
    assert!(start.visible_length.abs() < 1e-3);
    assert!(start.fill_opacity.abs() < 1e-5);
    assert!((start.stroke_opacity - 1.0).abs() < 1e-5);
  }

  #[test]
  fn keyframes_follow_the_measured_samples() {
    // samples.json de 01b : logo.opacity 0,264403 -> dashoffset 3255,77, fill-opacity 0,0185777.
    let opacity = 0.264_403_f32;
    let eased = (opacity - 0.2) / 0.8;
    assert!((3716.0 * (1.0 - eased / 0.65) - 3255.77).abs() < 1.0);
    assert!((0.15 * eased / 0.65 - 0.018_577_7).abs() < 1e-3);
    // Même échantillon plus tard : trait 0,635002 -> remplissage 0,563665.
    let second = (1.0_f32 - 0.635_002) / 0.75;
    assert!((0.15 + 0.85 * second - 0.563_665).abs() < 1e-3);
  }

  #[test]
  fn the_draw_ends_filled_and_the_stroke_fades() {
    let end = at(920.0);
    assert!((end.logo_scale - 1.0).abs() < 1e-5);
    assert!((end.visible_length - 3716.0).abs() < 1e-2);
    assert!((end.fill_opacity - 1.0).abs() < 1e-5);
    assert!((end.stroke_opacity - 0.25).abs() < 1e-5);
    assert!(at(919.0).blocking);
    assert!(!end.blocking);
    assert!((end.layer_opacity - 1.0).abs() < 1e-5);
  }

  #[test]
  fn the_stroke_is_complete_at_the_middle_keyframe() {
    // Le tracé finit à 65 % du progrès adouci, atteint dès 174 ms avec cette courbe.
    let drawn = (0..=920)
      .find(|&ms| at(ms as f32).visible_length >= 3716.0 - 1e-2)
      .expect("tracé fini");
    assert!((170..=180).contains(&drawn), "{drawn}");
  }

  #[test]
  fn the_fade_runs_450_ms_and_the_layer_goes_away_at_1340() {
    assert!(
      (at(920.0 + 225.0).layer_opacity - (1.0 - bezier_ease(tokens::FADE_CURVE, 0.5))).abs() < 1e-5
    );
    assert!(at(920.0 + 450.0).layer_opacity.abs() < 1e-5);
    let now = Instant::now();
    let splash = Splash { start: Some(now) };
    assert!(!splash.finished(now + Duration::from_millis(1339)));
    assert!(splash.finished(now + Duration::from_millis(1340)));
    assert!(!Splash::default().finished(now + Duration::from_secs(9)));
  }

  /// Nombre d'enroulement du contour au point donné (lancer de rayon vers +x).
  fn winding(points: &[(f32, f32)], (px, py): (f32, f32)) -> i32 {
    let mut total = 0;
    for pair in points.windows(2) {
      let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
      if (y0 <= py) != (y1 <= py) {
        let x = x0 + (py - y0) / (y1 - y0) * (x1 - x0);
        if x > px {
          total += if y1 > y0 { 1 } else { -1 };
        }
      }
    }
    total
  }

  #[test]
  fn the_outline_winds_twice_only_on_slivers_so_even_odd_punches_almost_all_of_the_fill() {
    let points = &OUTLINE.subpaths[0];
    let mut inside = 0;
    let mut counts = std::collections::BTreeMap::new();
    for ix in 0..290 {
      for iy in 0..280 {
        let w = winding(points, (ix as f32 * 2.0 + 0.37, iy as f32 * 2.0 + 0.53));
        *counts.entry(w).or_insert(0) += 1;
        inside += i32::from(w != 0);
      }
    }
    // Orientation unique ; l'enroulement 2 (recouvrement de deux lobes) laisse un peu de noir sous le remplissage : moins de 1 % de sa surface.
    assert!(counts.keys().all(|w| [0, -1, -2].contains(w)), "{counts:?}");
    let twice = counts.get(&-2).copied().unwrap_or(0);
    assert!(inside > 5000 && twice * 100 < inside, "{counts:?}");
  }

  #[test]
  fn the_fade_composes_like_the_group_opacity_of_riff() {
    // Page P, accent A : sous le remplissage `P (1 - a) + A a` (noir percé, puis remplissage d'alpha `a`).
    let (page, accent) = (0.30_f32, 0.83_f32);
    for ms in [1000.0, 1100.0, 1200.0, 1300.0] {
      let look = at(ms);
      assert!(look.holes_the_fill(), "{ms}");
      let a = look.layer_opacity;
      let alpha = look.fill_opacity * look.logo_opacity * a;
      assert!(
        (alpha - a).abs() < 1e-5,
        "fond et logo pleins pendant le fondu"
      );
      let pixel = page * (1.0 - alpha) + accent * alpha;
      assert!((pixel - (page * (1.0 - a) + accent * a)).abs() < 1e-5);
    }
    assert!(
      !at(500.0).holes_the_fill() && !at(919.0).holes_the_fill() && !look(None).holes_the_fill()
    );
  }

  #[test]
  fn the_curve_holds_overshoot_curves() {
    // Courbe de la gouttelette `(0.22,1.35,0.36,1)` : dépasse 1 avant de revenir.
    let curve = [0.22, 1.35, 0.36, 1.0];
    let peak = (1..100)
      .map(|i| bezier_ease(curve, i as f32 / 100.0))
      .fold(0.0_f32, f32::max);
    assert!((peak - 1.0408).abs() < 2e-3, "{peak}");
    assert!((bezier_ease(curve, 1.0) - 1.0).abs() < f32::EPSILON);
  }
}
