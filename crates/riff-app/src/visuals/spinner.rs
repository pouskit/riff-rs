//! Spinners à forme qui se fond dans la suivante : `.m3-btn-shape-spinner` de Riff (`app.js:9944-9980`, bouton lecture et loader de
//! recherche), avec les 11 formes de `nextSpinnerShape` (`shapes.js:418-480`) : les 4 de `shapes.js` et les 7 de `RiffSpinnerShapes`.
//!
//! Le temps vient de l'horloge de `App` : la forme montrée à un instant se déduit de la graine du tirage et de l'état de la boucle
//! partagée ([`ShapeSpinnerLoop`]), qui ne bouge qu'à l'apparition et à la disparition des spinners ; le banc de rendu peut donc
//! figer n'importe quel instant.

use crate::theme::tokens::shape_spinner as tokens;
use crate::visuals::shapes::{self, SAMPLE_COUNT, ease_in_out_cubic};
use crate::visuals::state::bezier_ease;
use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::{Color, Element, Point, Rectangle, Renderer, Theme, mouse};
use std::f64::consts::PI;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// Contour d'un `Path2D` : sous-chemins en polylignes fines, avec les opérations de `shapes.js:317-402`.
struct Outline {
  subs: Vec<Vec<[f64; 2]>>,
  point: [f64; 2],
}

impl Outline {
  fn new() -> Self {
    Self {
      subs: Vec::new(),
      point: [0.0, 0.0],
    }
  }

  fn move_to(&mut self, x: f64, y: f64) {
    self.subs.push(vec![[x, y]]);
    self.point = [x, y];
  }

  /// `lineTo` : sans point courant, comme `moveTo`.
  fn line_to(&mut self, x: f64, y: f64) {
    match self.subs.last_mut() {
      Some(sub) => sub.push([x, y]),
      None => self.subs.push(vec![[x, y]]),
    }
    self.point = [x, y];
  }

  /// `closePath` : le point courant revient au début du sous-chemin, un nouveau sous-chemin y commence.
  fn close(&mut self) {
    if let Some(start) = self.subs.last().and_then(|sub| sub.first().copied()) {
      self.subs.push(vec![start]);
      self.point = start;
    }
  }

  fn bezier_to(&mut self, c1: [f64; 2], c2: [f64; 2], to: [f64; 2]) {
    let from = self.point;
    for step in 1..=64 {
      let t = f64::from(step) / 64.0;
      let u = 1.0 - t;
      let mix = |axis: usize| {
        u * u * u * from[axis]
          + 3.0 * u * u * t * c1[axis]
          + 3.0 * u * t * t * c2[axis]
          + t * t * t * to[axis]
      };
      self.line_to(mix(0), mix(1));
    }
  }

  /// `ellipse(cx, cy, rx, ry, 0, start, end)` sens horaire : un trait vers le début de l'arc, puis l'arc.
  fn ellipse(&mut self, center: [f64; 2], radii: [f64; 2], start: f64, end: f64) {
    let mut sweep = end - start;
    if sweep >= 2.0 * PI {
      sweep = 2.0 * PI;
    } else {
      while sweep < 0.0 {
        sweep += 2.0 * PI;
      }
    }
    for step in 0..=256 {
      let angle = start + sweep * f64::from(step) / 256.0;
      self.line_to(
        center[0] + radii[0] * angle.cos(),
        center[1] + radii[1] * angle.sin(),
      );
    }
  }

  fn arc(&mut self, center: [f64; 2], radius: f64, start: f64, end: f64) {
    self.ellipse(center, [radius, radius], start, end);
  }

  /// `arcTo(x1, y1, x2, y2, radius)` : arc tangent aux deux droites, toujours le court.
  fn arc_to(&mut self, corner: [f64; 2], to: [f64; 2], radius: f64) {
    let from = self.point;
    let u0 = [from[0] - corner[0], from[1] - corner[1]];
    let u1 = [to[0] - corner[0], to[1] - corner[1]];
    let (l0, l1) = (u0[0].hypot(u0[1]), u1[0].hypot(u1[1]));
    let cross = u0[0] * u1[1] - u0[1] * u1[0];
    if l0 == 0.0 || l1 == 0.0 || radius == 0.0 || cross.abs() < 1e-9 {
      self.line_to(corner[0], corner[1]);
      return;
    }
    let (u0, u1) = ([u0[0] / l0, u0[1] / l0], [u1[0] / l1, u1[1] / l1]);
    let angle = (u0[0] * u1[0] + u0[1] * u1[1]).clamp(-1.0, 1.0).acos();
    let tangent = radius / (angle / 2.0).tan();
    let first = [corner[0] + u0[0] * tangent, corner[1] + u0[1] * tangent];
    let last = [corner[0] + u1[0] * tangent, corner[1] + u1[1] * tangent];
    let bisector = [u0[0] + u1[0], u0[1] + u1[1]];
    let length = bisector[0].hypot(bisector[1]);
    let reach = radius / (angle / 2.0).sin();
    let center = [
      corner[0] + bisector[0] / length * reach,
      corner[1] + bisector[1] / length * reach,
    ];
    self.line_to(first[0], first[1]);
    let start = (first[1] - center[1]).atan2(first[0] - center[0]);
    let end = (last[1] - center[1]).atan2(last[0] - center[0]);
    let mut sweep = end - start;
    while sweep > PI {
      sweep -= 2.0 * PI;
    }
    while sweep < -PI {
      sweep += 2.0 * PI;
    }
    for step in 1..=64 {
      let angle = start + sweep * f64::from(step) / 64.0;
      self.line_to(
        center[0] + radius * angle.cos(),
        center[1] + radius * angle.sin(),
      );
    }
  }

  /// `isPointInPath` : règle non nulle, sous-chemins fermés implicitement.
  fn contains(&self, x: f64, y: f64) -> bool {
    let mut winding = 0_i32;
    for sub in &self.subs {
      if sub.len() < 3 {
        continue;
      }
      for (index, a) in sub.iter().enumerate() {
        let b = sub[(index + 1) % sub.len()];
        let side = (b[0] - a[0]) * (y - a[1]) - (x - a[0]) * (b[1] - a[1]);
        if a[1] <= y {
          if b[1] > y && side > 0.0 {
            winding += 1;
          }
        } else if b[1] <= y && side < 0.0 {
          winding -= 1;
        }
      }
    }
    winding != 0
  }
}

/// `roundedPolygon` (`shapes.js:318-331`).
fn rounded_polygon(points: &[[f64; 2]], radii: &[f64]) -> Outline {
  let count = points.len();
  let mid = |a: [f64; 2], b: [f64; 2]| [f64::midpoint(a[0], b[0]), f64::midpoint(a[1], b[1])];
  let mut outline = Outline::new();
  let start = mid(points[0], points[1]);
  outline.move_to(start[0], start[1]);
  for index in 1..=count {
    let vertex = points[index % count];
    outline.arc_to(
      vertex,
      mid(vertex, points[(index + 1) % count]),
      radii[index % count],
    );
  }
  outline.close();
  outline
}

fn ghost() -> Outline {
  let mut p = Outline::new();
  p.move_to(0.0, 144.762);
  p.ellipse([152.0, 144.762], [152.0, 144.762], PI, 2.0 * PI);
  p.line_to(304.0, 246.095);
  p.ellipse([243.2, 246.095], [60.8, 57.905], 0.0, PI / 2.0);
  p.bezier_to([208.0, 297.0], [180.0, 275.048], [156.327, 275.048]);
  p.line_to(147.673, 275.048);
  p.bezier_to([124.0, 275.048], [96.0, 297.0], [60.8, 304.0]);
  p.ellipse([60.8, 246.095], [60.8, 57.905], PI / 2.0, PI);
  p.close();
  p
}

fn dumbbell() -> Outline {
  let (r, w, h) = (81.36_f64, 310.0_f64, 320.0_f64);
  let dy = h / 2.0 - r;
  let dx = (r * r - dy * dy).sqrt();
  let a = dy.atan2(dx);
  let mut p = Outline::new();
  p.move_to(r, 0.0);
  p.line_to(w - r, 0.0);
  p.arc([w - r, r], r, -PI / 2.0, a);
  p.arc([w - r, h - r], r, -a, PI / 2.0);
  p.line_to(r, h);
  p.arc([r, h - r], r, PI / 2.0, PI + a);
  p.arc([r, r], r, PI - a, 1.5 * PI);
  p.close();
  p
}

fn capsule() -> Outline {
  let (s, r) = (300.0_f64, 128.95_f64);
  let mut p = Outline::new();
  p.arc([s - r, r], r, -0.75 * PI, 0.25 * PI);
  p.arc([r, s - r], r, 0.25 * PI, 1.25 * PI);
  p.close();
  p
}

fn arch() -> Outline {
  let (s, r, c) = (304.0_f64, 152.0_f64, 50.28_f64);
  let mut p = Outline::new();
  p.move_to(s, r);
  p.line_to(s, s - c);
  p.arc_to([s, s], [s - c, s], c);
  p.line_to(c, s);
  p.arc_to([0.0, s], [0.0, s - c], c);
  p.line_to(0.0, r);
  p.arc([r, r], r, PI, 2.0 * PI);
  p.close();
  p
}

fn pentagon() -> Outline {
  let cx = 160.0;
  rounded_polygon(
    &[
      [cx, -14.7],
      [cx + 169.0, 116.0],
      [cx + 105.0, 319.8],
      [cx - 105.0, 319.8],
      [cx - 169.0, 116.0],
    ],
    &[87.0; 5],
  )
}

fn hexagon() -> Outline {
  let cx = 160.0;
  rounded_polygon(
    &[
      [cx + 104.66, 0.0],
      [cx + 167.7, 127.0],
      [cx + 104.66, 254.0],
      [cx - 104.66, 254.0],
      [cx - 167.7, 127.0],
      [cx - 104.66, 0.0],
    ],
    &[68.7, 96.4, 68.7, 68.7, 96.4, 68.7],
  )
}

fn triangle() -> Outline {
  let cx = 157.6;
  rounded_polygon(
    &[[cx, -52.4], [cx + 215.0, 279.0], [cx - 215.0, 279.0]],
    &[62.7, 69.5, 69.5],
  )
}

/// `samplePathToRadialProfile` (`shapes.js:422-452`) : la forme, centrée dans sa boîte `w` x `h` et mise à l'échelle 270 / max(w, h)
/// sur un canevas de 320, est parcourue sur 360 angles. Chaque rayon est cherché par dichotomie en 9 pas sur `isPointInPath`
/// entre 0 et 160 (d'où les paliers de 0,31), puis les rayons sont ramenés à un maximum de 160.
fn radial_profile(outline: &Outline, w: f64, h: f64) -> Vec<f32> {
  let scale = 270.0 / w.max(h);
  let mut radii = vec![0.0_f64; SAMPLE_COUNT];
  for (index, radius) in radii.iter_mut().enumerate() {
    let angle = -PI + 2.0 * PI * index as f64 / SAMPLE_COUNT as f64;
    let (cos, sin) = (angle.cos(), angle.sin());
    let (mut low, mut high) = (0.0_f64, 160.0_f64);
    for _ in 0..9 {
      let mid = f64::midpoint(low, high);
      if outline.contains(mid * cos / scale + w / 2.0, mid * sin / scale + h / 2.0) {
        low = mid;
      } else {
        high = mid;
      }
    }
    *radius = f64::midpoint(low, high);
  }
  let largest = radii.iter().copied().fold(0.0, f64::max);
  let norm = 160.0 / if largest == 0.0 { 1.0 } else { largest };
  radii
    .into_iter()
    .map(|radius| (radius * norm) as f32)
    .collect()
}

/// Noms des 7 formes de `RiffSpinnerShapes.list` (`shapes.js:394-403`), après les 4 de `shapes.js`.
pub const EXTRA_NAMES: [&str; 7] = [
  "ghost", "dumbbell", "capsule", "arch", "pentagon", "hexagon", "triangle",
];

/// Profils des 7 formes de `RiffSpinnerShapes.list`, dans l'ordre de la liste.
#[must_use]
pub fn extra_profiles() -> Vec<Vec<f32>> {
  [
    (ghost(), 304.0, 304.0),
    (dumbbell(), 310.0, 320.0),
    (capsule(), 300.0, 300.0),
    (arch(), 304.0, 304.0),
    (pentagon(), 320.0, 320.0),
    (hexagon(), 320.0, 254.0),
    (triangle(), 315.3, 279.0),
  ]
  .iter()
  .map(|(outline, w, h)| radial_profile(outline, *w, *h))
  .collect()
}

/// Les 11 profils de `allProfiles` (`shapes.js:459-463`) : les formes de `shapes.js`, puis celles de `RiffSpinnerShapes`.
pub static PROFILES: LazyLock<Vec<Vec<f32>>> = LazyLock::new(|| {
  let mut all = shapes::all_profiles();
  all.extend(extra_profiles());
  all
});

/// Générateur xorshift64* : le tirage de `Math.random`, avec une graine pour que le banc soit reproductible.
struct Rng(u64);

impl Rng {
  fn new(seed: u64) -> Self {
    // splitmix64 : une graine proche d'une autre donne une suite sans rapport, et jamais l'état nul.
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    Self((z ^ (z >> 31)) | 1)
  }

  fn below(&mut self, bound: usize) -> usize {
    self.0 ^= self.0 >> 12;
    self.0 ^= self.0 << 25;
    self.0 ^= self.0 >> 27;
    (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % bound
  }
}

/// `nextSpinnerShape` (`shapes.js:465-475`) rejoué depuis le début : les `len` premiers indices montrés. L'indice 0 est la forme
/// de départ (`spinnerHistory = [0]`) ; chaque tirage évite les 3 dernières formes de l'historique, borné à 10.
#[must_use]
pub fn sequence(seed: u64, count: usize, len: usize) -> Vec<usize> {
  let mut rng = Rng::new(seed);
  let mut history = vec![0_usize];
  let mut shown = vec![0_usize];
  for _ in 1..len {
    let recent = &history[history.len().saturating_sub(tokens::RECENT)..];
    let candidates: Vec<usize> = (0..count).filter(|index| !recent.contains(index)).collect();
    let chosen = if candidates.is_empty() {
      0
    } else {
      candidates[rng.below(candidates.len())]
    };
    history.push(chosen);
    if history.len() > tokens::HISTORY {
      history.remove(0);
    }
    shown.push(chosen);
  }
  shown
}

/// Forme à un instant : fondu de `from` vers `to` au progrès `ease`, rotation `rotation` en radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
  pub from: usize,
  pub to: usize,
  pub ease: f32,
  pub rotation: f32,
}

/// Boucle unique de `startButtonShapeSpinners` (`app.js:9944-9980`), partagée par tous les `.m3-btn-shape-spinner:not(.hidden)` :
/// loader de recherche, `#playbar-spinner`, `#tvk-play-spinner`, boutons « Checking... » et « Signing in... ». Elle ne garde que
/// `fromIdx` (position dans le tirage, `toIdx` est la suivante) et `stepStart` : pendant un affichage continu la pose est une
/// fonction pure de `now`, donc le banc de rendu fige n'importe quel instant. Sans spinner affiché la boucle n'avance pas
/// (`app.js:9953`, `if (spinners.length > 0)`) : [`Self::hide`] fige le dernier pas atteint, [`Self::show`] reprend dessus.
///
/// Écart connu : Riff recale `stepStart` sur l'image qui franchit 1200 ms (retard inférieur à une image par pas) ; ici les pas
/// tombent exactement tous les 1200 ms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShapeSpinnerLoop {
  /// Position de `fromIdx` dans `sequence` ; 0 = forme de départ (`spinnerHistory = [0]`), `toIdx` est déjà la position 1.
  from_pos: usize,
  /// `stepStart` ; `None` tant qu'aucun spinner n'a été affiché : la page est alors inactive depuis plus de 1,2 s et le premier
  /// pas a lieu dès la première image (`app.js:9952-9956`).
  step_start: Option<Instant>,
  /// Au moins un spinner est affiché.
  shown: bool,
}

impl ShapeSpinnerLoop {
  /// Au moins un spinner est affiché.
  #[must_use]
  pub fn shown(&self) -> bool {
    self.shown
  }

  /// Apparition du premier spinner à `at` : un pas si le dernier remonte à 1200 ms ou plus (`app.js:9952`), sans effet si la
  /// boucle tourne déjà.
  pub fn show(&mut self, at: Instant) {
    if self.shown {
      return;
    }
    self.shown = true;
    let due = self.step_start.is_none_or(|start| {
      at.saturating_duration_since(start) >= Duration::from_millis(tokens::STEP_MS)
    });
    if due {
      self.from_pos += 1;
      self.step_start = Some(at);
    }
  }

  /// Disparition du dernier spinner à `at` : fige la position et `stepStart` au dernier pas atteint.
  pub fn hide(&mut self, at: Instant) {
    if !self.shown {
      return;
    }
    self.shown = false;
    if let Some(start) = self.step_start {
      let steps = at.saturating_duration_since(start).as_millis() / u128::from(tokens::STEP_MS);
      self.from_pos += steps as usize;
      self.step_start = Some(start + Duration::from_millis(tokens::STEP_MS * steps as u64));
    }
  }

  /// Pose commune à tous les spinners à `now`. La rotation suit l'horloge de la page (`now % 3000`, `app.js:9963`), soit
  /// `now - epoch`. Avant tout affichage, la pose est celle d'un affichage qui commence à `now`.
  #[must_use]
  pub fn pose(&self, seed: u64, count: usize, epoch: Instant, now: Instant) -> Pose {
    let step_ms = tokens::STEP_MS;
    let (base, elapsed) = match self.step_start {
      Some(start) => (self.from_pos, now.saturating_duration_since(start)),
      None => (self.from_pos + 1, Duration::ZERO),
    };
    let step = (elapsed.as_millis() / u128::from(step_ms)) as usize;
    let into = elapsed.as_secs_f64() * 1000.0 - (step as f64) * step_ms as f64;
    let from = base + step;
    let shown = sequence(seed, count, from + 2);
    let turn = now.saturating_duration_since(epoch).as_millis() % u128::from(tokens::TURN_MS);
    Pose {
      from: shown[from],
      to: shown[from + 1],
      ease: ease_in_out_cubic((into / step_ms as f64) as f32),
      rotation: (turn as f64 / tokens::TURN_MS as f64 * 2.0 * PI) as f32,
    }
  }
}

/// Pose à `now` d'une boucle qui démarre à `started` sur une page inactive : le premier pas va de la 2e à la 3e forme du tirage
/// (la 2e est `toIdx`, choisie au chargement, `app.js:9946`).
#[must_use]
pub fn pose(seed: u64, count: usize, started: Instant, epoch: Instant, now: Instant) -> Pose {
  let mut spinners = ShapeSpinnerLoop::default();
  spinners.show(started);
  spinners.pose(seed, count, epoch, now)
}

/// Trace la forme de `pose` dans `frame`, centrée en `center`, dans une boîte de `side` pixels (`viewBox="-180 -180 360 360"`) :
/// un point sur quatre des 360, comme `app.js:9965-9970`.
pub fn draw(
  frame: &mut Frame,
  center: Point,
  side: f32,
  pose: &Pose,
  profiles: &[Vec<f32>],
  color: Color,
) {
  let scale = side / tokens::VIEW_BOX;
  let (from, to) = (
    &profiles[pose.from % profiles.len()],
    &profiles[pose.to % profiles.len()],
  );
  let path = Path::new(|path| {
    for sample in (0..SAMPLE_COUNT).step_by(tokens::SAMPLE_STEP) {
      let angle = -std::f32::consts::PI
        + std::f32::consts::TAU * sample as f32 / SAMPLE_COUNT as f32
        + pose.rotation;
      let radius = (from[sample] + (to[sample] - from[sample]) * pose.ease) * scale;
      let point = Point::new(
        center.x + angle.cos() * radius,
        center.y + angle.sin() * radius,
      );
      if sample == 0 {
        path.move_to(point);
      } else {
        path.line_to(point);
      }
    }
    path.close();
  });
  frame.fill(&path, color);
}

struct ShapeSpinner {
  pose: Pose,
  color: Color,
}

impl<Message> canvas::Program<Message> for ShapeSpinner {
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
      &self.pose,
      &PROFILES,
      self.color,
    );
    vec![frame.into_geometry()]
  }
}

/// `.m3-btn-shape-spinner` de `side` pixels, de couleur `color`, à la pose `pose`.
pub fn shape_spinner<'a, Message: 'a>(side: f32, pose: Pose, color: Color) -> Element<'a, Message> {
  canvas::Canvas::new(ShapeSpinner { pose, color })
    .width(side)
    .height(side)
    .into()
}

/// Attente de la lecture (`updatePlayPauseButton`, `app.js:14030-14065`) : le bouton prend l'aspect `.playing` tout de suite, puis
/// `.buffering` (spinner visible, icône qui s'efface) seulement si le chargement dure encore 500 ms plus tard.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayWait {
  /// Début de l'attente en cours.
  pub since: Option<Instant>,
  /// Fin de la dernière attente : instant, et part d'icône effacée à cet instant (le retour part de là).
  released: Option<(Instant, f32)>,
}

impl PlayWait {
  /// Suit l'état de chargement : `waiting` vaut `player.loading || playback.buffering`.
  pub fn track(&mut self, waiting: bool, now: Instant) {
    match (waiting, self.since) {
      (true, None) => {
        self.since = Some(now);
        self.released = None;
      }
      (false, Some(_)) => {
        let hidden = self.icon_hidden(now);
        self.since = None;
        self.released = Some((now, hidden));
      }
      _ => {}
    }
  }

  /// Début du spinner : l'anti-rebond de 500 ms écoulé, `.buffering` et `.hidden` retiré (`app.js:14046-14055`).
  #[must_use]
  pub fn spinner_since(&self) -> Option<Instant> {
    self
      .since
      .map(|since| since + Duration::from_millis(tokens::PLAY_DEBOUNCE_MS))
  }

  /// Le spinner est visible à `now`.
  #[must_use]
  pub fn spinner_shown(&self, now: Instant) -> bool {
    self.spinner_since().is_some_and(|start| now >= start)
  }

  /// Part effacée de l'icône (0 = pleine, 1 = effacée et à l'échelle 0,65) : `transition: opacity 260ms, transform 260ms`
  /// de `cubic-bezier(0.2, 0, 0, 1)` (`styles.css:3147-3156`), depuis la valeur courante.
  #[must_use]
  pub fn icon_hidden(&self, now: Instant) -> f32 {
    let fade = Duration::from_millis(tokens::PLAY_FADE_MS).as_secs_f32();
    if let Some(start) = self.spinner_since().filter(|_| self.since.is_some()) {
      let elapsed = now.saturating_duration_since(start).as_secs_f32();
      return bezier_ease(tokens::PLAY_FADE_CURVE, elapsed / fade);
    }
    self.released.map_or(0.0, |(at, from)| {
      let elapsed = now.saturating_duration_since(at).as_secs_f32();
      from * (1.0 - bezier_ease(tokens::PLAY_FADE_CURVE, elapsed / fade))
    })
  }

  /// L'attente ou le retour de l'icône demande des images d'animation.
  #[must_use]
  pub fn animating(&self, now: Instant) -> bool {
    self.since.is_some() || self.icon_hidden(now) > 0.0
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
  }

  #[test]
  fn there_are_eleven_profiles_normalised_to_160() {
    assert_eq!(PROFILES.len(), 4 + EXTRA_NAMES.len());
    for (index, profile) in PROFILES.iter().enumerate() {
      assert_eq!(profile.len(), SAMPLE_COUNT, "forme {index}");
      let largest = profile.iter().copied().fold(0.0, f32::max);
      assert!((largest - 160.0).abs() < 0.01, "forme {index} : {largest}");
      assert!(profile.iter().all(|radius| *radius > 40.0), "forme {index}");
    }
  }

  #[test]
  fn the_hexagon_matches_the_measured_profile() {
    // `ui-reference/motion/16-spinner-lecture/samples.json` à 14 ms : début de la forme (hexagone), rayons des 8 premiers points
    // (un sur quatre) lus dans `d` (x, y arrondis à 0,1).
    let measured = [
      (146.4_f32, -64.6_f32),
      (150.2, -54.2),
      (152.8, -43.3),
      (154.4, -32.3),
      (154.8, -21.3),
      (154.4, -10.3),
      (153.6, 0.5),
      (153.2, 11.2),
    ];
    let hexagon = &PROFILES[4 + 5];
    for (index, (x, y)) in measured.iter().enumerate() {
      let radius = x.hypot(*y);
      assert!(
        (hexagon[index * 4] - radius).abs() < 0.6,
        "point {index} : {} contre {radius}",
        hexagon[index * 4]
      );
    }
  }

  #[test]
  fn the_sequence_avoids_the_last_three_shapes_and_is_reproducible() {
    let shown = sequence(7, 11, 200);
    assert_eq!(shown, sequence(7, 11, 200));
    assert_ne!(shown, sequence(8, 11, 200));
    assert_eq!(shown[0], 0);
    for window in shown.windows(4) {
      assert!(!window[..3].contains(&window[3]), "{window:?}");
    }
  }

  #[test]
  fn the_pose_morphs_for_1200_ms_then_moves_to_the_next_pair() {
    let start = Instant::now();
    let at = |t: u64| pose(1, 11, start, start, start + ms(t));
    let seq = sequence(1, 11, 5);
    let first = at(0);
    assert_eq!((first.from, first.to, first.ease), (seq[1], seq[2], 0.0));
    let middle = at(600);
    assert!((middle.ease - 0.5).abs() < 1e-6);
    let second = at(1200);
    assert_eq!((second.from, second.to), (seq[2], seq[3]));
    assert!(second.ease.abs() < 1e-6);
    assert!(at(1199).ease > 0.99);
  }

  #[test]
  fn the_loop_resumes_on_its_last_target_after_a_dead_time_of_1200_ms_or_more() {
    let t0 = Instant::now();
    let seq = sequence(1, 11, 8);
    let mut spinners = ShapeSpinnerLoop::default();
    spinners.show(t0);
    spinners.hide(t0 + ms(3000));
    // Pas à 0, 1200 et 2400 ms : la cible gelée est seq[4].
    spinners.show(t0 + ms(3600));
    let resumed = spinners.pose(1, 11, t0, t0 + ms(3600));
    assert_eq!((resumed.from, resumed.to), (seq[4], seq[5]));
    assert!(resumed.ease.abs() < 1e-6);
    assert_eq!(spinners.pose(1, 11, t0, t0 + ms(3600 + 1200)).from, seq[5]);
  }

  #[test]
  fn a_dead_time_under_1200_ms_keeps_the_step_in_progress() {
    let t0 = Instant::now();
    let seq = sequence(1, 11, 8);
    let mut spinners = ShapeSpinnerLoop::default();
    spinners.show(t0);
    let before = spinners.pose(1, 11, t0, t0 + ms(2999));
    spinners.hide(t0 + ms(3000));
    spinners.show(t0 + ms(3599));
    let after = spinners.pose(1, 11, t0, t0 + ms(3599));
    assert_eq!((before.from, before.to), (seq[3], seq[4]));
    assert_eq!((after.from, after.to), (seq[3], seq[4]));
    assert!(after.ease > before.ease && after.ease > 0.99);
    // Le pas finit à 3600 ms comme s'il n'y avait pas eu de temps mort.
    assert_eq!(spinners.pose(1, 11, t0, t0 + ms(3600)).from, seq[4]);
  }

  #[test]
  fn the_loop_ignores_a_second_show_and_a_hide_before_any_show() {
    let t0 = Instant::now();
    let mut spinners = ShapeSpinnerLoop::default();
    spinners.hide(t0);
    assert_eq!(spinners, ShapeSpinnerLoop::default());
    spinners.show(t0);
    let first = spinners;
    spinners.show(t0 + ms(5000));
    assert_eq!(spinners, first);
    assert!(spinners.shown());
  }

  #[test]
  fn the_rotation_follows_the_page_clock() {
    let epoch = Instant::now();
    let rotation = |t: u64| pose(1, 11, epoch, epoch, epoch + ms(t)).rotation;
    assert!(rotation(0).abs() < 1e-6);
    assert!((rotation(750) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert!(rotation(3000).abs() < 1e-6);
  }

  #[test]
  fn the_spinner_waits_500_ms_and_the_icon_fades_over_260_ms() {
    let start = Instant::now();
    let mut wait = PlayWait::default();
    wait.track(true, start);
    assert!(!wait.spinner_shown(start + ms(499)));
    assert!(wait.icon_hidden(start + ms(499)).abs() < f32::EPSILON);
    assert!(wait.spinner_shown(start + ms(500)));
    // `16-spinner-lecture/samples.json` : bascule vers 14,4 ms, icône à 0,67 d'opacité (0,33 effacée) à 51,4 ms, soit 37 ms plus tard.
    assert!((wait.icon_hidden(start + ms(537)) - 0.33).abs() < 0.02);
    assert!((wait.icon_hidden(start + ms(760)) - 1.0).abs() < 1e-6);
    assert!(wait.animating(start + ms(5000)));
  }

  #[test]
  fn a_short_wait_never_shows_the_spinner_and_the_release_restores_the_icon() {
    let start = Instant::now();
    let mut wait = PlayWait::default();
    wait.track(true, start);
    wait.track(false, start + ms(300));
    assert!(!wait.spinner_shown(start + ms(600)));
    assert!(!wait.animating(start + ms(300)));
    let mut long = PlayWait::default();
    long.track(true, start);
    long.track(false, start + ms(1000));
    assert!(long.icon_hidden(start + ms(1000)) > 0.99);
    assert!(long.animating(start + ms(1100)));
    assert!(long.icon_hidden(start + ms(1260)).abs() < 1e-6);
    assert!(!long.animating(start + ms(1260)));
  }
}
