//! File de notifications, comme `showToast` de `app.js` : une à la fois, 10 en attente au maximum. Chaque notification est une
//! gouttelette qui tombe du haut de la fenêtre (`dropletToastIn`) puis remonte (`dropletToastOut`), dessinée dans un canevas
//! parce que `transform: translateY() scaleY() scaleX()` de Riff n'existe pas pour les widgets d'iced.

use crate::theme::fonts;
use crate::theme::tokens::toast as tokens;
use crate::visuals::state::bezier_ease;
use crate::widgets::spinner;
use iced::alignment::Vertical;
use iced::widget::canvas::{
  self, Action, Event, Frame, Geometry, LineCap, LineJoin, Path, Stroke, Text,
};
use iced::widget::text::Shaping;
use iced::{
  Color, Element, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, mouse, window,
};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Notifications en attente au maximum : au-delà, la plus ancienne est retirée (`MAX_QUEUED_TOASTS`).
pub const MAX_QUEUED: usize = 10;
/// Durée d'affichage par défaut.
pub const DEFAULT_DURATION: Duration = Duration::from_millis(3500);

/// Genre d'une notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastKind {
  #[default]
  Info,
  Success,
  Warning,
  Error,
  /// `.riff-share-spinner-toast` (`app.js:10474-10487`) : boîte de 44 px avec un `.g-spinner`, sans message ni minuterie.
  /// Elle reste jusqu'à `Message::ToastExpired`.
  Spinner,
}

/// Notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
  pub id: u64,
  pub message: String,
  pub kind: ToastKind,
  pub duration: Duration,
  /// Instant d'apparition (départ de `dropletToastIn`). `None` : jamais posé, la notification est affichée sans animation.
  pub shown_at: Option<Instant>,
  /// Instant de la classe `.leaving` (départ de `dropletToastOut`).
  pub leaving_at: Option<Instant>,
  /// Durée de la minuterie de fermeture en cours : `remaining` de `app.js:508`, amputée du temps écoulé à chaque entrée du survol.
  pub remaining: Duration,
  /// Départ de la minuterie reprise à la sortie du survol (`startTimestamp`, `app.js:524`). `None` : la minuterie date de `shown_at`.
  pub timer_from: Option<Instant>,
  /// Survolée : la minuterie est arrêtée (`clearTimeout`, `app.js:535`).
  pub paused: bool,
  /// Génération de la minuterie lancée : une minuterie arrivée après un survol n'est plus la bonne (`clearTimeout` ne peut pas rappeler
  /// une tâche déjà partie).
  pub timer_gen: u32,
}

/// Valeurs animées de la notification : l'opacité, et la transformation `translateY(shift x hauteur) scaleY() scaleX()` autour
/// du haut au centre (`transform-origin: top center`, `styles.css:4665`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
  pub opacity: f32,
  /// Décalage vertical en fraction de la hauteur de la boîte (`-1` = `translateY(-100%)`).
  pub shift: f32,
  pub scale_x: f32,
  pub scale_y: f32,
}

impl Pose {
  /// Notification posée : aucune transformation.
  pub const SETTLED: Self = Self {
    opacity: 1.0,
    shift: 0.0,
    scale_x: 1.0,
    scale_y: 1.0,
  };
}

fn lerp(from: f32, to: f32, e: f32) -> f32 {
  from * (1.0 - e) + to * e
}

/// `dropletToastIn` à `elapsed` après l'apparition. La courbe `animation-timing-function` s'applique à chaque intervalle entre
/// deux images clés (0 % -> 65 % puis 65 % -> 100 %), `fill-mode: both` retient l'image à 0 % avant le départ.
#[must_use]
pub fn enter_pose(elapsed: Duration) -> Pose {
  let t = (elapsed.as_secs_f32() / (tokens::ENTER_MS as f32 / 1000.0)).clamp(0.0, 1.0);
  if t < tokens::ENTER_KEY {
    let e = bezier_ease(tokens::ENTER_CURVE, t / tokens::ENTER_KEY);
    Pose {
      opacity: lerp(0.0, 1.0, e).clamp(0.0, 1.0),
      shift: lerp(-1.0, 0.0, e),
      scale_x: lerp(0.75, 0.97, e),
      scale_y: lerp(0.4, 1.06, e),
    }
  } else {
    let e = bezier_ease(
      tokens::ENTER_CURVE,
      (t - tokens::ENTER_KEY) / (1.0 - tokens::ENTER_KEY),
    );
    Pose {
      opacity: 1.0,
      shift: 0.0,
      scale_x: lerp(0.97, 1.0, e),
      scale_y: lerp(1.06, 1.0, e),
    }
  }
}

/// `dropletToastOut` à `elapsed` après `.leaving` : un seul intervalle, `fill-mode: forwards`.
#[must_use]
pub fn leave_pose(elapsed: Duration) -> Pose {
  let t = (elapsed.as_secs_f32() / (tokens::LEAVE_MS as f32 / 1000.0)).clamp(0.0, 1.0);
  let e = bezier_ease(tokens::LEAVE_CURVE, t);
  Pose {
    opacity: lerp(1.0, 0.0, e),
    shift: lerp(0.0, -1.0, e),
    scale_x: lerp(1.0, 0.8, e),
    scale_y: lerp(1.0, 0.5, e),
  }
}

impl Toast {
  /// Valeurs animées à l'instant `now`.
  #[must_use]
  pub fn pose(&self, now: Instant) -> Pose {
    // `body.perf .riff-toast, .leaving { animation: none }` (`styles.css:4674`). La réduction de mouvement n'y change rien :
    // `.riff-toast { animation: dropletToastIn ... !important }` (`styles.css:4666`) vient après `.force-reduced-motion *`.
    if crate::motion::perf() {
      return Pose::SETTLED;
    }
    if let Some(left) = self.leaving_at {
      return leave_pose(now.saturating_duration_since(left));
    }
    self.shown_at.map_or(Pose::SETTLED, |shown| {
      enter_pose(now.saturating_duration_since(shown))
    })
  }

  /// Délai entre `.leaving` et le retrait : `animationend` (200 ms), ou 180 ms pour le toast spinner (`app.js:10485`).
  fn leave_delay(&self) -> Duration {
    Duration::from_millis(if self.kind == ToastKind::Spinner {
      tokens::SPINNER_LEAVE_MS
    } else {
      tokens::LEAVE_MS
    })
  }

  /// Une image d'animation est nécessaire : entrée ou sortie en cours, ou spinner qui tourne.
  fn animating(&self, now: Instant) -> bool {
    if self.leaving_at.is_some() || self.kind == ToastKind::Spinner {
      return true;
    }
    self.shown_at.is_some_and(|shown| {
      now.saturating_duration_since(shown) < Duration::from_millis(tokens::ENTER_MS)
    })
  }
}

/// File des notifications.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toasts {
  pub active: Option<Toast>,
  pub queue: VecDeque<Toast>,
  next_id: u64,
}

impl Toasts {
  /// Ajoute une notification. Renvoie la notification à afficher maintenant, si aucune n'est affichée.
  pub fn push(
    &mut self,
    message: &str,
    kind: ToastKind,
    duration: Option<Duration>,
  ) -> Option<&Toast> {
    if message.is_empty() && kind != ToastKind::Spinner {
      return None;
    }
    self.next_id += 1;
    let toast = Toast {
      id: self.next_id,
      message: capitalized(message),
      kind,
      duration: duration.unwrap_or(DEFAULT_DURATION),
      shown_at: None,
      leaving_at: None,
      remaining: duration.unwrap_or(DEFAULT_DURATION),
      timer_from: None,
      paused: false,
      timer_gen: 0,
    };
    if self.active.is_none() {
      self.active = Some(toast);
      return self.active.as_ref();
    }
    if self.queue.len() >= MAX_QUEUED {
      self.queue.pop_front();
    }
    self.queue.push_back(toast);
    None
  }

  /// Retire tout de suite la notification affichée si c'est bien `id`. Renvoie la suivante à afficher.
  pub fn dismiss(&mut self, id: u64) -> Option<&Toast> {
    if self.active.as_ref().is_none_or(|toast| toast.id != id) {
      return None;
    }
    self.active = self.queue.pop_front();
    self.active.as_ref()
  }

  /// Pose l'instant d'apparition de la notification affichée si elle n'en a pas.
  pub fn stamp(&mut self, now: Instant) {
    if let Some(toast) = &mut self.active {
      toast.shown_at.get_or_insert(now);
    }
  }

  /// `dismiss` de `app.js:513-525` : la classe `.leaving` part de `now`. Sans effet si `id` n'est pas la notification
  /// affichée ou si elle part déjà (le clic et la minuterie peuvent arriver tous les deux).
  pub fn begin_leave(&mut self, id: u64, now: Instant) {
    if let Some(toast) = self.active.as_mut().filter(|toast| toast.id == id) {
      toast.leaving_at.get_or_insert(now);
    }
  }

  /// Retire la notification dont la sortie est finie et pose le départ de la suivante. Renvoie la suivante, à afficher
  /// (`finish` puis `processToastQueue`, `app.js:517-521`).
  pub fn finish_leaving(&mut self, now: Instant) -> Option<&Toast> {
    let done = self.active.as_ref().is_some_and(|toast| {
      toast
        .leaving_at
        .is_some_and(|left| now.saturating_duration_since(left) >= toast.leave_delay())
    });
    if !done {
      return None;
    }
    self.active = self.queue.pop_front();
    self.stamp(now);
    self.active.as_ref()
  }

  /// `mouseenter` (`app.js:534-538`) : la minuterie s'arrête et ce qu'il reste est retenu, au moins 500 ms. Sans effet si `id` n'est
  /// pas la notification affichée, si elle part déjà (`.leaving`) ou si c'est le toast spinner, qui n'a pas d'écouteur.
  pub fn hover_enter(&mut self, id: u64, now: Instant) {
    let floor = Duration::from_millis(tokens::HOVER_FLOOR_MS);
    if let Some(toast) = self.active.as_mut().filter(|toast| toast.id == id) {
      if toast.leaving_at.is_some() || toast.paused || toast.kind == ToastKind::Spinner {
        return;
      }
      let from = toast.timer_from.or(toast.shown_at).unwrap_or(now);
      toast.remaining = toast
        .remaining
        .saturating_sub(now.saturating_duration_since(from))
        .max(floor);
      toast.paused = true;
      toast.timer_gen += 1;
    }
  }

  /// `mouseleave` (`app.js:540-544`) : si elle ne part pas, la minuterie repart pour ce qu'il reste. Renvoie la notification dont la
  /// minuterie est à relancer (`Effect::ToastTimer`, génération et durée).
  pub fn hover_leave(&mut self, id: u64, now: Instant) -> Option<&Toast> {
    let toast = self
      .active
      .as_mut()
      .filter(|toast| toast.id == id && toast.paused && toast.leaving_at.is_none())?;
    toast.paused = false;
    toast.timer_from = Some(now);
    toast.timer_gen += 1;
    Some(&*toast)
  }

  /// Fin de la minuterie `generation` de `id`, arrivée à `at` : la sortie part, sauf si le survol l'a annulée ou remplacée.
  pub fn timeout(&mut self, id: u64, generation: u32, at: Instant) -> bool {
    let Some(toast) = self.active.as_mut().filter(|toast| {
      toast.id == id && toast.timer_gen == generation && !toast.paused && toast.leaving_at.is_none()
    }) else {
      return false;
    };
    toast.leaving_at = Some(at);
    true
  }

  /// Une image d'animation est nécessaire à `now`.
  #[must_use]
  pub fn animating(&self, now: Instant) -> bool {
    self
      .active
      .as_ref()
      .is_some_and(|toast| toast.animating(now))
  }
}

fn capitalized(message: &str) -> String {
  let mut chars = message.chars();
  chars.next().map_or_else(String::new, |first| {
    first.to_uppercase().chain(chars).collect()
  })
}

/// Couleurs de la notification : le fond suit `--toast-bg`, soit `surface-container-highest-solid` que `app.js:1170-1183` règle
/// sur `surface-container-highest` (`styles.css:4647`) ; le texte suit `on-surface`, l'icône `info` suit `primary`, `error`
/// suit `error` (`styles.css:4683-4723`).
#[derive(Debug, Clone, Copy)]
pub struct Colors {
  pub background: Color,
  pub text: Color,
  pub primary: Color,
  pub error: Color,
}

/// Dimensions de la boîte, sans les coins concaves.
#[derive(Debug, Clone, PartialEq)]
struct Shape {
  width: f32,
  height: f32,
  /// Message tronqué par `text-overflow: ellipsis`.
  text: String,
}

fn shape_of(toast: &Toast) -> Shape {
  if toast.kind == ToastKind::Spinner {
    return Shape {
      width: tokens::SPINNER_BOX,
      height: tokens::SPINNER_BOX,
      text: String::new(),
    };
  }
  use crate::screens::right_panel::{ellipsize, text_width};
  let inner = tokens::ICON + tokens::GAP;
  let wanted = text_width(&toast.message, tokens::TEXT, fonts::MEDIUM);
  let width = (2.0 * tokens::PADDING_X + inner + wanted.min(tokens::TEXT_MAX_WIDTH))
    .clamp(tokens::MIN_WIDTH, tokens::MAX_WIDTH);
  let room = (width - 2.0 * tokens::PADDING_X - inner).min(tokens::TEXT_MAX_WIDTH);
  Shape {
    width,
    height: tokens::PADDING_TOP + tokens::ICON + tokens::PADDING_BOTTOM,
    text: ellipsize(&toast.message, tokens::TEXT, fonts::MEDIUM, room),
  }
}

/// Notification affichée, en haut au centre (`.toast-container`, `styles.css:4622-4633`). Un clic la ferme (`app.js:546`), l'entrée et
/// la sortie du pointeur arrêtent et relancent sa minuterie (`on_hover(id, survolée, instant)`).
pub fn view<'a, Message: Clone + 'a>(
  toast: &Toast,
  now: Instant,
  colors: Colors,
  on_close: Message,
  on_hover: fn(u64, bool, Instant) -> Message,
) -> Element<'a, Message> {
  let shape = shape_of(toast);
  // La boîte dépasse de 6 % vers le bas pendant l'entrée, et l'ombre du toast spinner de 4 + 16 px.
  let height = shape.height * 1.1 + tokens::SPINNER_SHADOW_OFFSET + tokens::SPINNER_SHADOW_BLUR;
  let width = shape.width + 2.0 * tokens::FILLET;
  canvas::Canvas::new(Droplet {
    id: toast.id,
    kind: toast.kind,
    pose: toast.pose(now),
    spin: toast
      .shown_at
      .map_or(Duration::ZERO, |shown| now.saturating_duration_since(shown)),
    shape,
    colors,
    on_close,
    on_hover,
  })
  .width(width)
  .height(height)
  .into()
}

struct Droplet<Message> {
  id: u64,
  kind: ToastKind,
  pose: Pose,
  /// Temps depuis l'apparition : phase de `g-rot` et `g-dash` du toast spinner.
  spin: Duration,
  shape: Shape,
  colors: Colors,
  on_close: Message,
  on_hover: fn(u64, bool, Instant) -> Message,
}

fn with_alpha(color: Color, alpha: f32) -> Color {
  Color {
    a: color.a * alpha,
    ..color
  }
}

/// Fonction de répartition de la loi normale centrée réduite (approximation logistique, écart maximal 0,01).
fn normal_cdf(x: f32) -> f32 {
  1.0 / (1.0 + (-1.702 * x).exp())
}

/// Boîte de `left` à `right`, de `top` à `bottom`, aux coins du bas arrondis de rayon `radius`.
fn rounded_bottom(left: f32, right: f32, top: f32, bottom: f32, radius: f32) -> Path {
  let radius = radius.max(0.0);
  Path::new(|path| {
    path.move_to(Point::new(left, top));
    path.line_to(Point::new(right, top));
    path.line_to(Point::new(right, bottom - radius));
    path.arc_to(
      Point::new(right, bottom),
      Point::new(right - radius, bottom),
      radius,
    );
    path.line_to(Point::new(left + radius, bottom));
    path.arc_to(
      Point::new(left, bottom),
      Point::new(left, bottom - radius),
      radius,
    );
    path.close();
  })
}

/// Boîte et coins concaves `::before`/`::after` : le plafond s'évase d'un quart de cercle de rayon `--trb` de chaque côté
/// (`radial-gradient(circle at 0 100%, transparent calc(var(--trb) - 0.5px), bg var(--trb))`, `styles.css:4670-4671`).
fn body(left: f32, right: f32, height: f32, radius: f32) -> Path {
  let fillet = tokens::FILLET;
  Path::new(|path| {
    path.move_to(Point::new(left - fillet, 0.0));
    path.line_to(Point::new(right + fillet, 0.0));
    path.arc_to(Point::new(right, 0.0), Point::new(right, fillet), fillet);
    path.line_to(Point::new(right, height - radius));
    path.arc_to(
      Point::new(right, height),
      Point::new(right - radius, height),
      radius,
    );
    path.line_to(Point::new(left + radius, height));
    path.arc_to(
      Point::new(left, height),
      Point::new(left, height - radius),
      radius,
    );
    path.line_to(Point::new(left, fillet));
    path.arc_to(
      Point::new(left, 0.0),
      Point::new(left - fillet, 0.0),
      fillet,
    );
    path.close();
  })
}

/// Ombre `0 4px 16px rgba(0, 0, 0, 0.4)` du toast spinner. `iced` n'a pas de flou dans un canevas : l'ombre est un empilement
/// de boîtes agrandies, dont l'opacité cumulée suit la courbe d'une gaussienne d'écart-type `blur / 2` (le flou CSS).
fn shadow(frame: &mut Frame, left: f32, right: f32, height: f32, radius: f32, opacity: f32) {
  const LAYERS: u8 = 9;
  let blur = tokens::SPINNER_SHADOW_BLUR;
  let sigma = blur / 2.0;
  let mut before = 0.0_f32;
  for layer in 0..LAYERS {
    let grow = blur * (1.0 - f32::from(layer) / 6.0);
    let cumulative = tokens::SPINNER_SHADOW_ALPHA * (1.0 - normal_cdf(grow / sigma));
    let alpha = 1.0 - (1.0 - cumulative) / (1.0 - before);
    before = cumulative;
    let offset = tokens::SPINNER_SHADOW_OFFSET;
    frame.fill(
      &rounded_bottom(
        left - grow,
        right + grow,
        offset - grow,
        height + offset + grow,
        radius + grow,
      ),
      Color::from_rgba(0.0, 0.0, 0.0, alpha * opacity),
    );
  }
}

/// Icône SVG de 24 x 24 de `app.js:488-497`, mise à l'échelle dans la boîte de `tokens::ICON` dont le coin haut gauche est `at`.
/// `pen` : épaisseur de trait déjà multipliée par l'échelle de la transformation courante.
fn icon(frame: &mut Frame, kind: ToastKind, at: Point, color: Color, pen: f32) {
  let k = tokens::ICON / 24.0;
  let p = |x: f32, y: f32| Point::new(at.x + x * k, at.y + y * k);
  let stroke = |width: f32, join: LineJoin| {
    Stroke::default()
      .with_color(color)
      .with_width(width * k * pen)
      .with_line_cap(LineCap::Round)
      .with_line_join(join)
  };
  let line = |from: Point, to: Point| {
    Path::new(|path| {
      path.move_to(from);
      path.line_to(to);
    })
  };
  match kind {
    ToastKind::Error => {
      let pen = stroke(2.6, LineJoin::Miter);
      frame.stroke(&line(p(12.0, 5.0), p(12.0, 14.0)), pen);
      frame.stroke(&line(p(12.0, 19.0), p(12.01, 19.0)), pen);
    }
    ToastKind::Success => {
      let check = Path::new(|path| {
        path.move_to(p(5.0, 12.5));
        path.line_to(p(10.0, 17.5));
        path.line_to(p(19.0, 7.5));
      });
      frame.stroke(&check, stroke(2.6, LineJoin::Miter));
    }
    ToastKind::Warning => {
      // `M10.29 3.86 L1.82 18 a2 2 0 0 0 1.71 3 h16.94 a2 2 0 0 0 1.71-3 L13.71 3.86 a2 2 0 0 0-3.42 0z` : triangle dont les
      // trois sommets (12 1, 0.02 21, 23.98 21) sont arrondis par des arcs de rayon 2.
      let (top, left, right) = (p(12.0, 1.007), p(0.0225, 21.0), p(23.9775, 21.0));
      let triangle = Path::new(|path| {
        path.move_to(p(12.0, 21.0));
        path.arc_to(right, top, 2.0 * k);
        path.arc_to(top, left, 2.0 * k);
        path.arc_to(left, p(12.0, 21.0), 2.0 * k);
        path.close();
      });
      let pen = stroke(2.0, LineJoin::Miter);
      frame.stroke(&triangle, pen);
      frame.stroke(&line(p(12.0, 9.0), p(12.0, 13.0)), pen);
      frame.stroke(&line(p(12.0, 17.0), p(12.01, 17.0)), pen);
    }
    ToastKind::Info | ToastKind::Spinner => {
      let pen = stroke(2.2, LineJoin::Round);
      frame.stroke(&Path::circle(p(12.0, 12.0), 10.0 * k), pen);
      frame.stroke(&line(p(12.0, 16.0), p(12.0, 12.0)), pen);
      frame.stroke(&line(p(12.0, 8.0), p(12.01, 8.0)), pen);
    }
  }
}

impl<Message> Droplet<Message> {
  /// Zone cliquable : la boîte à sa place de repos (`pointer-events: auto`, `cursor: pointer`).
  fn hit(&self, bounds: Rectangle) -> Rectangle {
    Rectangle::new(
      Point::new(bounds.x + tokens::FILLET, bounds.y),
      Size::new(self.shape.width, self.shape.height),
    )
  }

  /// Boîte telle que dessinée, transformation comprise (`translateY` puis échelle autour du haut au centre) : le navigateur teste
  /// le survol sur le rectangle transformé.
  fn drawn_hit(&self, bounds: Rectangle) -> Rectangle {
    let rest = self.hit(bounds);
    let Pose {
      shift,
      scale_x,
      scale_y,
      ..
    } = self.pose;
    let origin = bounds.x + bounds.width / 2.0;
    Rectangle::new(
      Point::new(
        origin + (rest.x - origin) * scale_x,
        bounds.y + shift * rest.height + (rest.y - bounds.y) * scale_y,
      ),
      Size::new(rest.width * scale_x, rest.height * scale_y),
    )
  }

  fn radius(&self) -> f32 {
    if self.kind == ToastKind::Spinner {
      tokens::SPINNER_RADIUS
    } else {
      tokens::RADIUS
    }
  }

  fn icon_color(&self) -> Color {
    match self.kind {
      ToastKind::Success => tokens::SUCCESS,
      ToastKind::Warning => tokens::WARNING,
      ToastKind::Error => self.colors.error,
      ToastKind::Info | ToastKind::Spinner => self.colors.primary,
    }
  }

  fn paint(&self, frame: &mut Frame, canvas_width: f32) {
    let Pose {
      opacity,
      shift,
      scale_x,
      scale_y,
    } = self.pose;
    if opacity <= 0.0 {
      return;
    }
    let (width, height) = (self.shape.width, self.shape.height);
    let (left, right) = (tokens::FILLET, tokens::FILLET + width);
    let origin = canvas_width / 2.0;
    // Les traits d'iced ne suivent pas l'échelle du `Frame` : leur épaisseur prend la moyenne des deux échelles.
    let pen = (scale_x * scale_y).sqrt();
    frame.with_save(|frame| {
      if self.pose != Pose::SETTLED {
        frame.translate(Vector::new(origin, shift * height));
        frame.scale_nonuniform(Vector::new(scale_x, scale_y));
        frame.translate(Vector::new(-origin, 0.0));
      }
      if self.kind == ToastKind::Spinner {
        shadow(frame, left, right, height, self.radius(), opacity);
      }
      frame.fill(
        &body(left, right, height, self.radius()),
        with_alpha(self.colors.background, opacity),
      );
      if self.kind == ToastKind::Spinner {
        let center = Point::new(origin, height / 2.0);
        spinner::draw(
          frame,
          center,
          tokens::SPINNER_SIZE,
          with_alpha(self.colors.primary, opacity),
          self.spin,
          pen,
        );
        return;
      }
      let content_x = left + tokens::PADDING_X;
      icon(
        frame,
        self.kind,
        Point::new(content_x, tokens::PADDING_TOP),
        with_alpha(self.icon_color(), opacity),
        pen,
      );
      frame.fill_text(Text {
        content: self.shape.text.clone(),
        position: Point::new(
          content_x + tokens::ICON + tokens::GAP,
          tokens::PADDING_TOP + tokens::ICON / 2.0 + tokens::TEXT_NUDGE,
        ),
        color: with_alpha(self.colors.text, opacity),
        size: Pixels(tokens::TEXT),
        font: fonts::MEDIUM,
        align_y: Vertical::Center,
        shaping: Shaping::Advanced,
        ..Text::default()
      });
    });
  }
}

impl<Message: Clone> canvas::Program<Message> for Droplet<Message> {
  /// Identifiant de la notification sous le pointeur : une notification qui en remplace une autre sous un pointeur immobile est
  /// entrée à son tour.
  type State = Option<u64>;

  fn update(
    &self,
    hovered: &mut Option<u64>,
    event: &Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> Option<Action<Message>> {
    // Le toast spinner n'a ni clic ni survol (`app.js:10474-10487`).
    if self.kind == ToastKind::Spinner {
      return None;
    }
    match event {
      Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor
        .is_over(self.hit(bounds))
        .then(|| Action::publish(self.on_close.clone()).and_capture()),
      // Le survol est recalculé à chaque événement, redessin compris : Chromium recalcule le survol après une mise en page sans que
      // le pointeur bouge et envoie alors l'entrée ou la sortie (`codereview.chromium.org/2956023004`). Une notification apparue
      // sous un pointeur immobile est survolée dès que sa boîte le couvre.
      _ => {
        let over = cursor.is_over(self.drawn_hit(bounds));
        let was = *hovered == Some(self.id);
        *hovered = over.then_some(self.id);
        // Instant du redessin quand c'est lui qui change le survol ; sinon celui de l'événement du pointeur, que le widget lit
        // lui-même (les événements de souris d'iced 0.14 n'ont pas d'horodatage).
        let at = match event {
          Event::Window(window::Event::RedrawRequested(at)) => *at,
          _ => Instant::now(),
        };
        (over != was).then(|| Action::publish((self.on_hover)(self.id, over, at)))
      }
    }
  }

  fn draw(
    &self,
    _state: &Option<u64>,
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let mut frame = Frame::new(renderer, bounds.size());
    self.paint(&mut frame, bounds.width);
    vec![frame.into_geometry()]
  }

  fn mouse_interaction(
    &self,
    _state: &Option<u64>,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> mouse::Interaction {
    if cursor.is_over(self.hit(bounds)) {
      mouse::Interaction::Pointer
    } else {
      mouse::Interaction::default()
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use iced_test::Simulator;

  fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
  }

  fn near(
    pose: Pose,
    opacity: f32,
    shift: f32,
    scale_x: f32,
    scale_y: f32,
    tolerance: f32,
  ) -> bool {
    (pose.opacity - opacity).abs() <= tolerance
      && (pose.shift - shift).abs() <= tolerance
      && (pose.scale_x - scale_x).abs() <= tolerance
      && (pose.scale_y - scale_y).abs() <= tolerance
  }

  #[test]
  fn the_first_toast_shows_at_once() {
    let mut toasts = Toasts::default();
    let shown = toasts
      .push("un", ToastKind::Info, None)
      .cloned()
      .expect("affichée");
    assert_eq!(shown.message, "Un");
    assert_eq!(shown.duration, DEFAULT_DURATION);
    assert!(toasts.queue.is_empty());
  }

  #[test]
  fn the_next_toasts_wait() {
    let mut toasts = Toasts::default();
    toasts.push("un", ToastKind::Info, None);
    assert!(toasts.push("deux", ToastKind::Error, None).is_none());
    assert_eq!(toasts.queue.len(), 1);
    assert_eq!(
      toasts.active.as_ref().map(|t| t.message.as_str()),
      Some("Un")
    );
  }

  #[test]
  fn dismissing_shows_the_next_one() {
    let mut toasts = Toasts::default();
    let first = toasts
      .push("un", ToastKind::Info, None)
      .map(|t| t.id)
      .unwrap();
    toasts.push("deux", ToastKind::Error, Some(Duration::from_secs(9)));
    let next = toasts.dismiss(first).cloned().expect("suivante");
    assert_eq!(next.message, "Deux");
    assert_eq!(next.kind, ToastKind::Error);
    assert_eq!(next.duration, Duration::from_secs(9));
    assert!(toasts.dismiss(next.id).is_none());
    assert_eq!(toasts.active, None);
  }

  #[test]
  fn a_late_timer_does_not_remove_another_toast() {
    let mut toasts = Toasts::default();
    let first = toasts
      .push("un", ToastKind::Info, None)
      .map(|t| t.id)
      .unwrap();
    toasts.dismiss(first);
    toasts.push("deux", ToastKind::Info, None);
    assert!(toasts.dismiss(first).is_none());
    assert_eq!(
      toasts.active.as_ref().map(|t| t.message.as_str()),
      Some("Deux")
    );
  }

  #[test]
  fn the_queue_keeps_ten_toasts_and_drops_the_oldest() {
    let mut toasts = Toasts::default();
    toasts.push("affichée", ToastKind::Info, None);
    for index in 0..12 {
      toasts.push(&format!("attente {index}"), ToastKind::Info, None);
    }
    assert_eq!(toasts.queue.len(), MAX_QUEUED);
    assert_eq!(
      toasts.queue.front().map(|t| t.message.as_str()),
      Some("Attente 2")
    );
    assert_eq!(
      toasts.queue.back().map(|t| t.message.as_str()),
      Some("Attente 11")
    );
    assert_eq!(
      toasts.active.as_ref().map(|t| t.message.as_str()),
      Some("Affichée")
    );
  }

  #[test]
  fn an_empty_message_is_ignored() {
    let mut toasts = Toasts::default();
    assert!(toasts.push("", ToastKind::Info, None).is_none());
    assert_eq!(toasts, Toasts::default());
  }

  #[test]
  fn the_spinner_toast_needs_no_message() {
    let mut toasts = Toasts::default();
    assert!(toasts.push("", ToastKind::Spinner, None).is_some());
  }

  #[test]
  fn accented_first_letters_are_capitalized() {
    let mut toasts = Toasts::default();
    assert_eq!(
      toasts
        .push("été", ToastKind::Info, None)
        .map(|t| t.message.as_str()),
      Some("Été")
    );
  }

  #[test]
  fn the_entry_keyframes_are_interpolated() {
    assert_eq!(
      enter_pose(Duration::ZERO),
      Pose {
        opacity: 0.0,
        shift: -1.0,
        scale_x: 0.75,
        scale_y: 0.4
      }
    );
    // 65 % de 340 ms : `translateY(0) scaleY(1.06) scaleX(0.97)`, opacité 1.
    assert!(near(enter_pose(ms(221)), 1.0, 0.0, 0.97, 1.06, 0.01));
    assert_eq!(enter_pose(ms(340)), Pose::SETTLED);
    assert_eq!(enter_pose(ms(5000)), Pose::SETTLED);
  }

  #[test]
  fn the_entry_matches_the_measured_samples() {
    // `ui-reference/motion/03-toast/samples.json` : opacité 0,701, matrice (0,904, 0,863, ty -11,94 sur 39,96) à 33,5 ms.
    let pose = enter_pose(Duration::from_micros(33_500));
    assert!(near(pose, 0.70, -0.30, 0.904, 0.863, 0.02), "{pose:?}");
    // Dépassement de la courbe (y1 = 1,35) : le sommet de scaleY (1,0864) tombe vers 217 ms - 100 ms de départ.
    assert!(enter_pose(ms(116)).scale_y > 1.06);
  }

  #[test]
  fn the_exit_goes_up_and_fades() {
    assert_eq!(leave_pose(Duration::ZERO), Pose::SETTLED);
    let end = leave_pose(ms(200));
    assert_eq!(
      (end.opacity, end.shift, end.scale_x, end.scale_y),
      (0.0, -1.0, 0.8, 0.5)
    );
    // `samples.json` de `03-toast` : `.leaving` vers 1630 ms, opacité 0,505, matrice (0,901, 0,7526), ty -19,78 sur 39,96 à 1758,7 ms.
    let pose = leave_pose(Duration::from_micros(128_700));
    assert!(near(pose, 0.505, -0.495, 0.901, 0.7526, 0.02), "{pose:?}");
  }

  #[test]
  fn a_toast_leaves_after_its_exit_animation() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    let first = toasts
      .push("un", ToastKind::Info, None)
      .map(|t| t.id)
      .unwrap();
    toasts.push("deux", ToastKind::Info, None);
    toasts.stamp(start);
    assert!(toasts.animating(start));
    assert!(!toasts.animating(start + ms(340)));
    toasts.begin_leave(first, start + ms(3500));
    assert!(toasts.animating(start + ms(3600)));
    assert!(toasts.finish_leaving(start + ms(3699)).is_none());
    let next = toasts
      .finish_leaving(start + ms(3700))
      .cloned()
      .expect("suivante");
    assert_eq!(next.message, "Deux");
    assert_eq!(next.shown_at, Some(start + ms(3700)));
  }

  #[test]
  fn leaving_twice_keeps_the_first_start() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    let id = toasts
      .push("un", ToastKind::Info, None)
      .map(|t| t.id)
      .unwrap();
    toasts.begin_leave(id, start);
    toasts.begin_leave(id, start + ms(50));
    toasts.begin_leave(id + 1, start + ms(60));
    assert_eq!(
      toasts.active.as_ref().and_then(|t| t.leaving_at),
      Some(start)
    );
  }

  #[test]
  fn the_spinner_toast_is_removed_after_180_ms_and_never_settles() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    let id = toasts
      .push("", ToastKind::Spinner, None)
      .map(|t| t.id)
      .unwrap();
    toasts.stamp(start);
    assert!(toasts.animating(start + ms(60_000)));
    toasts.begin_leave(id, start + ms(1000));
    assert!(toasts.finish_leaving(start + ms(1179)).is_none());
    assert!(toasts.finish_leaving(start + ms(1180)).is_none());
    assert_eq!(toasts.active, None);
  }

  #[test]
  fn an_unstamped_toast_is_shown_without_animation() {
    let mut toasts = Toasts::default();
    toasts.push("un", ToastKind::Info, None);
    let toast = toasts.active.as_ref().unwrap();
    assert_eq!(toast.pose(Instant::now()), Pose::SETTLED);
    assert!(!toasts.animating(Instant::now()));
  }

  /// Notification de 3 s affichée à `start`, et son identifiant.
  fn shown(start: Instant, duration: Duration) -> (Toasts, u64) {
    let mut toasts = Toasts::default();
    let id = toasts
      .push("un", ToastKind::Info, Some(duration))
      .map(|t| t.id)
      .unwrap();
    toasts.stamp(start);
    (toasts, id)
  }

  fn timer_generation(toasts: &Toasts) -> u32 {
    toasts.active.as_ref().unwrap().timer_gen
  }

  #[test]
  fn hovering_at_1_s_then_leaving_at_10_s_closes_at_12_s() {
    let start = Instant::now();
    let (mut toasts, id) = shown(start, ms(3000));
    let first = timer_generation(&toasts);
    toasts.hover_enter(id, start + ms(1000));
    assert_eq!(toasts.active.as_ref().unwrap().remaining, ms(2000));
    // La minuterie de départ (3 s) arrive pendant le survol ou après : sans effet.
    assert!(!toasts.timeout(id, first, start + ms(3000)));
    assert!(!toasts.timeout(id, first, start + ms(10_000)));
    let resumed = toasts.hover_leave(id, start + ms(10_000)).cloned().unwrap();
    assert_eq!(resumed.remaining, ms(2000));
    assert_ne!(resumed.timer_gen, first);
    assert!(
      !toasts.timeout(id, first, start + ms(12_000)),
      "ancienne minuterie"
    );
    assert!(toasts.timeout(id, resumed.timer_gen, start + ms(12_000)));
    assert_eq!(
      toasts.active.as_ref().and_then(|t| t.leaving_at),
      Some(start + ms(12_000))
    );
  }

  #[test]
  fn the_remaining_time_never_drops_below_500_ms() {
    let start = Instant::now();
    let (mut toasts, id) = shown(start, ms(3000));
    toasts.hover_enter(id, start + ms(2800));
    assert_eq!(toasts.active.as_ref().unwrap().remaining, ms(500));
    // Survol après l'échéance (la minuterie n'est pas encore arrivée) : même plancher.
    let (mut late, id) = shown(start, ms(3000));
    late.hover_enter(id, start + ms(3100));
    assert_eq!(late.active.as_ref().unwrap().remaining, ms(500));
  }

  #[test]
  fn a_second_hover_counts_from_the_resume() {
    let start = Instant::now();
    let (mut toasts, id) = shown(start, ms(3000));
    toasts.hover_enter(id, start + ms(1000));
    toasts.hover_leave(id, start + ms(5000));
    toasts.hover_enter(id, start + ms(5600));
    assert_eq!(toasts.active.as_ref().unwrap().remaining, ms(1400));
    toasts.hover_leave(id, start + ms(6000));
    let generation = timer_generation(&toasts);
    assert!(toasts.timeout(id, generation, start + ms(7400)));
  }

  #[test]
  fn hovering_a_leaving_toast_changes_nothing() {
    let start = Instant::now();
    let (mut toasts, id) = shown(start, ms(3000));
    toasts.begin_leave(id, start + ms(3000));
    let before = toasts.clone();
    toasts.hover_enter(id, start + ms(3050));
    assert_eq!(toasts, before);
    assert!(toasts.hover_leave(id, start + ms(3100)).is_none());
    assert_eq!(toasts, before);
  }

  #[test]
  fn each_toast_keeps_its_own_timer() {
    let start = Instant::now();
    let (mut toasts, first) = shown(start, ms(3000));
    toasts.push("deux", ToastKind::Info, Some(ms(4000)));
    toasts.hover_enter(first, start + ms(1000));
    toasts.begin_leave(first, start + ms(1500));
    let second = toasts.finish_leaving(start + ms(1700)).cloned().unwrap();
    assert_eq!(second.remaining, ms(4000));
    assert!(!second.paused);
    // Un survol destiné à la précédente ne touche pas la suivante.
    toasts.hover_enter(first, start + ms(1800));
    assert!(!toasts.active.as_ref().unwrap().paused);
    assert!(toasts.timeout(second.id, second.timer_gen, start + ms(5700)));
  }

  #[test]
  fn a_hovered_settled_toast_needs_no_frames() {
    let start = Instant::now();
    let (mut toasts, id) = shown(start, ms(3000));
    toasts.hover_enter(id, start + ms(1000));
    assert!(!toasts.animating(start + ms(1000)));
    assert!(!toasts.animating(start + ms(60_000)));
  }

  #[test]
  fn the_spinner_toast_ignores_the_hover() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    let id = toasts
      .push("", ToastKind::Spinner, None)
      .map(|t| t.id)
      .unwrap();
    toasts.stamp(start);
    let before = toasts.clone();
    toasts.hover_enter(id, start + ms(100));
    assert_eq!(toasts, before);
    assert!(toasts.hover_leave(id, start + ms(200)).is_none());
  }

  #[test]
  fn the_hover_zone_follows_the_drawn_box() {
    let drop = |pose: Pose| Droplet {
      id: 1,
      kind: ToastKind::Info,
      pose,
      spin: Duration::ZERO,
      shape: Shape {
        width: 200.0,
        height: 40.0,
        text: String::new(),
      },
      colors: Colors {
        background: Color::BLACK,
        text: Color::WHITE,
        primary: Color::WHITE,
        error: Color::WHITE,
      },
      on_close: (),
      on_hover: |_, _, _| (),
    };
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(200.0 + 2.0 * tokens::FILLET, 80.0));
    let rest = drop(Pose::SETTLED).drawn_hit(bounds);
    let hit = drop(Pose::SETTLED).hit(bounds);
    assert!((rest.x - hit.x).abs() < 1e-3 && rest.y == hit.y && rest.size() == hit.size());
    // `translateY(-50 %) scaleY(0.5) scaleX(0.5)` autour du haut au centre.
    let pose = Pose {
      opacity: 1.0,
      shift: -0.5,
      scale_x: 0.5,
      scale_y: 0.5,
    };
    let moved = drop(pose).drawn_hit(bounds);
    let center = bounds.width / 2.0;
    assert!((moved.width - 100.0).abs() < 1e-3 && (moved.height - 20.0).abs() < 1e-3);
    assert!((moved.x + moved.width / 2.0 - center).abs() < 1e-3);
    assert!((moved.y + 20.0).abs() < 1e-3);
  }

  /// Vue d'une notification posée dans un simulateur d'iced (1024 x 768, centrée en haut).
  fn toast_simulator<'a>(shown_at: Instant, now: Instant) -> (Simulator<'a, (u64, bool)>, u64) {
    let mut toasts = Toasts::default();
    toasts.push("Message", ToastKind::Info, None);
    toasts.stamp(shown_at);
    let toast = toasts.active.clone().unwrap();
    let colors = Colors {
      background: Color::BLACK,
      text: Color::WHITE,
      primary: Color::WHITE,
      error: Color::WHITE,
    };
    let view = view(&toast, now, colors, (0, false), |id, over, _| (id, over));
    let centered = iced::widget::container(view)
      .width(iced::Fill)
      .align_x(iced::alignment::Horizontal::Center);
    (Simulator::new(centered), toast.id)
  }

  fn redraw(at: Instant) -> Event {
    Event::Window(window::Event::RedrawRequested(at))
  }

  #[test]
  fn a_toast_under_a_still_pointer_is_entered_on_a_redraw_and_left_when_the_pointer_goes() {
    let start = Instant::now();
    let (mut sim, id) = toast_simulator(start, start + ms(400));
    // Le pointeur n'a pas bougé depuis l'apparition : seul un redessin arrive.
    sim.point_at(Point::new(512.0, 20.0));
    let _ = sim.simulate([redraw(start + ms(400))]);
    sim.point_at(Point::new(10.0, 700.0));
    let _ = sim.simulate([redraw(start + ms(416))]);
    let messages: Vec<_> = sim.into_messages().collect();
    assert_eq!(messages, [(id, true), (id, false)]);
  }

  #[test]
  fn a_toast_whose_box_does_not_cover_the_pointer_yet_is_not_entered() {
    let start = Instant::now();
    // 0 ms d'entrée : la boîte est à `translateY(-100 %)`, hors de la fenêtre.
    let (mut sim, _) = toast_simulator(start, start);
    sim.point_at(Point::new(512.0, 20.0));
    let _ = sim.simulate([redraw(start)]);
    assert!(sim.into_messages().next().is_none());
  }
}
