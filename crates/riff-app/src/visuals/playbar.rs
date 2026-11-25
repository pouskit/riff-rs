//! Animations de la barre de lecture : bouton lecture/pause (largeur, rayon), icône qui se transforme, phase de l'onde et suivi
//! de la barre de progression. Tout se calcule depuis l'horloge de `App` (`App::now`) : les vues ne lisent jamais l'heure.

use crate::app::App;
use crate::theme::tokens::playbar as bar;
use crate::visuals::state::bezier_ease;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Theme, Vector, mouse};
use std::f32::consts::TAU;
use std::time::{Duration, Instant};

/// Écart sous lequel deux valeurs d'une transition sont la même.
const EPSILON: f32 = 1e-4;

/// `transition` CSS d'une valeur scalaire, depuis la valeur affichée quand la cible change. Si la cible redevient la valeur de départ
/// d'une transition en cours (aller-retour), la durée est raccourcie comme le fait CSS Transitions 1 (« reversing shortening factor »).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
  from: f32,
  to: f32,
  start: Instant,
  /// Durée de la transition en cours (raccourcie après un retour en arrière).
  duration: Duration,
  /// Durée de `transition-duration`.
  base: Duration,
  curve: [f32; 4],
  /// Facteur de raccourcissement de la transition en cours (1 hors retour en arrière).
  factor: f32,
  /// « Reversing-adjusted start value » : valeur dont le retour déclenche un raccourcissement.
  adjusted_start: f32,
}

impl Transition {
  /// Transition terminée, au repos sur `value`.
  #[must_use]
  pub fn rest(value: f32, millis: u64, curve: [f32; 4], now: Instant) -> Self {
    Self {
      from: value,
      to: value,
      start: now,
      duration: Duration::from_millis(millis),
      base: Duration::from_millis(millis),
      curve,
      factor: 1.0,
      adjusted_start: value,
    }
  }

  fn linear(&self, now: Instant) -> f32 {
    let duration = self.duration.as_secs_f32();
    if duration <= 0.0 {
      return 1.0;
    }
    now.saturating_duration_since(self.start).as_secs_f32() / duration
  }

  /// Progrès après la courbe de la transition (peut dépasser 1 avec une courbe à dépassement).
  fn eased(&self, now: Instant) -> f32 {
    bezier_ease(self.curve, self.linear(now))
  }

  /// Valeur affichée à `now`.
  #[must_use]
  pub fn value(&self, now: Instant) -> f32 {
    self.from + (self.to - self.from) * self.eased(now)
  }

  /// La transition n'est pas arrivée à sa fin.
  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    (self.to - self.from).abs() > EPSILON && self.linear(now) < 1.0
  }

  /// Nouvelle cible à `now` : repart de la valeur affichée.
  pub fn retarget(&mut self, to: f32, now: Instant) {
    if (to - self.to).abs() < EPSILON {
      return;
    }
    let current = self.value(now);
    let reversing = self.running(now) && (to - self.adjusted_start).abs() < EPSILON;
    let factor = if reversing {
      (self.eased(now) * self.factor + (1.0 - self.factor))
        .abs()
        .clamp(0.0, 1.0)
    } else {
      1.0
    };
    self.adjusted_start = if reversing { self.to } else { current };
    self.from = current;
    self.to = to;
    self.start = now;
    self.duration = self.base.mul_f32(factor);
    self.factor = factor;
  }
}

/// Bouton lecture/pause : largeur et rayon (jelly, 200 ms) et forme de l'icône (400 ms), de 0 (lecture) à 1 (pause, classe `.playing`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayButton {
  state: Option<ButtonState>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ButtonState {
  playing: bool,
  jelly: Transition,
  morph: Transition,
}

/// Aspect du bouton à un instant, en pixels écran.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayPose {
  pub width: f32,
  pub radius: f32,
  /// Forme de l'icône pendant sa transformation (0 = lecture, 1 = pause) ; `None` au repos : l'icône SVG s'affiche telle quelle.
  pub morph: Option<f32>,
}

impl PlayButton {
  /// Suit l'état `.playing` du bouton. Le premier état observé n'est pas animé. Vrai si l'état a changé.
  pub fn track(&mut self, playing: bool, now: Instant) -> bool {
    let target = if playing { 1.0 } else { 0.0 };
    match &mut self.state {
      None => {
        self.state = Some(ButtonState {
          playing,
          jelly: Transition::rest(target, bar::PLAY_JELLY_MS, bar::PLAY_JELLY_CURVE, now),
          morph: Transition::rest(target, bar::PLAY_MORPH_MS, bar::PLAY_MORPH_CURVE, now),
        });
        false
      }
      Some(state) if state.playing != playing => {
        state.playing = playing;
        state.jelly.retarget(target, now);
        state.morph.retarget(target, now);
        true
      }
      Some(_) => false,
    }
  }

  /// Aspect du bouton quand la vue montre l'état `playing`. Si le suivi n'a pas vu cet état (état posé sans passer par `update`),
  /// le bouton est au repos sur `playing`.
  #[must_use]
  pub fn pose(&self, playing: bool, now: Instant) -> PlayPose {
    let rest = if playing { 1.0 } else { 0.0 };
    let (jelly, morph) = match &self.state {
      Some(state) if state.playing == playing => (
        state.jelly.value(now),
        state.morph.running(now).then(|| state.morph.value(now)),
      ),
      _ => (rest, None),
    };
    // `a * (1 - t) + b * t` rend `a` et `b` exacts aux extrémités (pas d'écart d'un ulp au repos).
    let mix = |paused: f32, playing: f32| paused * (1.0 - jelly) + playing * jelly;
    let width = mix(bar::PLAY_WIDTH_PAUSED, bar::PLAY_WRAPPER_WIDTH);
    let radius = mix(bar::PLAY_RADIUS_PAUSED, bar::PLAY_RADIUS_PLAYING);
    PlayPose {
      width,
      // Le rayon calculé n'est pas borné par CSS, le rendu le réduit à la moitié du plus petit côté.
      radius: radius.clamp(0.0, width.min(bar::PLAY_HEIGHT) / 2.0),
      morph,
    }
  }

  /// Une transition du bouton est en cours.
  #[must_use]
  pub fn animating(&self, now: Instant) -> bool {
    self
      .state
      .is_some_and(|state| state.jelly.running(now) || state.morph.running(now))
  }
}

/// Phase de l'onde `scrubPhase` : avance de 3,5 rad/s pendant la lecture, figée en pause, jamais remise à zéro par un changement de
/// titre ou une recherche (`app.js:3453-3455`). Elle se calcule depuis l'instant de reprise, pas depuis un compteur d'images.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Wave {
  /// Phase en radians à la dernière reprise ou pause. Le premier état observé la prend de la position (au lancement : 0, comme Riff).
  base: Option<f64>,
  /// Instant de la reprise en cours.
  since: Option<Instant>,
}

impl Wave {
  /// Onde en lecture depuis `since`, à la phase `phase` (radians) à cet instant.
  #[must_use]
  pub fn running(phase: f32, since: Instant) -> Self {
    Self {
      base: Some(f64::from(phase)),
      since: Some(since),
    }
  }

  /// Onde en pause sur la phase `phase` (radians).
  #[must_use]
  pub fn frozen(phase: f32) -> Self {
    Self {
      base: Some(f64::from(phase)),
      since: None,
    }
  }

  /// Suit l'état de lecture à l'instant réel `at`.
  pub fn track(&mut self, playing: bool, position: f32, at: Instant) {
    let base = *self
      .base
      .get_or_insert_with(|| f64::from((position * bar::WAVE_RATE as f32) % TAU));
    match (playing, self.since) {
      (true, None) => self.since = Some(at),
      (false, Some(since)) => {
        let elapsed = at.saturating_duration_since(since).as_secs_f64();
        self.base = Some((base + bar::WAVE_RATE * elapsed) % f64::from(TAU));
        self.since = None;
      }
      _ => {}
    }
  }

  /// Phase de l'onde à `now`, en radians dans `[0, 2π)`. `position` sert tant que le suivi n'a rien observé.
  #[must_use]
  pub fn phase(&self, now: Instant, position: f32) -> f32 {
    let Some(base) = self.base else {
      return (position * bar::WAVE_RATE as f32) % TAU;
    };
    let elapsed = self.since.map_or(0.0, |since| {
      now.saturating_duration_since(since).as_secs_f64()
    });
    if elapsed <= 0.0 {
      return base as f32;
    }
    ((base + bar::WAVE_RATE * elapsed) % f64::from(TAU)) as f32
  }
}

/// Suivi de la position par la barre : `scrubProgress += (cible - scrubProgress) * min(1, dt * k)` (`app.js:3430-3432`), `k` = 45, ou 32
/// pendant un glissement. Riff avance ce lerp à chaque image : à 60 images par seconde il laisse `1 - k/60` de l'écart par image. L'approche
/// exponentielle de même constante de temps, `exp(-r * dt)` avec `r = -60 * ln(1 - k/60)`, donne la même valeur à chaque image de 1/60 s
/// et ne dépend plus de la cadence.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Follow {
  state: Option<FollowState>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FollowState {
  value: f32,
  target: f32,
  at: Instant,
  rate: f32,
  /// Titre suivi : un nouveau titre remet le suivi à la cible (`scrubProgress = 0`, `app.js:4514`).
  key: u64,
}

/// Constante de l'approche exponentielle équivalente à `lerp(dt * k)` à 60 images par seconde.
#[must_use]
pub fn follow_rate(k: f32) -> f32 {
  let hz = bar::SCRUB_FOLLOW_REFERENCE_HZ;
  -hz * (1.0 - (k / hz).min(1.0 - 1e-6)).ln()
}

impl FollowState {
  fn at(&self, now: Instant) -> f32 {
    let elapsed = now.saturating_duration_since(self.at).as_secs_f32();
    self.target + (self.value - self.target) * (-self.rate * elapsed).exp()
  }
}

impl Follow {
  /// Nouvelle cible à l'instant réel `at`.
  pub fn track(&mut self, target: f32, dragging: bool, key: u64, at: Instant) {
    let k = if dragging {
      bar::SCRUB_FOLLOW_RATE_DRAG
    } else {
      bar::SCRUB_FOLLOW_RATE
    };
    let value = match &self.state {
      Some(state) if state.key == key => state.at(at),
      _ => target,
    };
    self.state = Some(FollowState {
      value,
      target,
      at,
      rate: follow_rate(k),
      key,
    });
  }

  /// Progression affichée à `now` pour la cible `target`. Une cible que le suivi n'a pas vue s'affiche telle quelle.
  #[must_use]
  pub fn value(&self, now: Instant, target: f32) -> f32 {
    match &self.state {
      Some(state) if (state.target - target).abs() < 1e-6 => state.at(now),
      _ => target,
    }
  }

  /// La progression affichée n'a pas rejoint la cible (moins de 0,06 px sur 600 px).
  #[must_use]
  pub fn settling(&self, now: Instant) -> bool {
    self
      .state
      .is_some_and(|state| (state.at(now) - state.target).abs() > 1e-4)
  }
}

/// Valeurs d'entrée du suivi, lues dans `App` après chaque message.
#[derive(Debug, Clone, Copy)]
pub struct BarInput {
  /// Le bouton a l'aspect `.playing` : lecture, ou attente du chargement (`app.js:14030`).
  pub button_playing: bool,
  pub playing: bool,
  pub position: f32,
  pub progress: f32,
  pub dragging: bool,
  pub key: u64,
}

/// Animations de la barre de lecture.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BarMotion {
  pub button: PlayButton,
  pub wave: Wave,
  pub scrub: Follow,
}

impl BarMotion {
  /// Suit l'état de la barre à l'instant réel `at`. Vrai si le bouton a changé d'aspect : l'horloge de `App` doit alors avancer à `at`.
  pub fn track(&mut self, input: &BarInput, at: Instant) -> bool {
    self.wave.track(input.playing, input.position, at);
    self
      .scrub
      .track(input.progress, input.dragging, input.key, at);
    self.button.track(input.button_playing, at)
  }

  /// Une des animations demande des images.
  #[must_use]
  pub fn animating(&self, now: Instant) -> bool {
    self.button.animating(now) || self.scrub.settling(now)
  }
}

/// Durée du titre et position affichée (l'aperçu d'une recherche en cours, sinon la lecture), en secondes.
#[must_use]
pub fn scrub_extent(app: &App) -> (f32, f32) {
  let duration = app
    .playback
    .duration
    .map(|duration| duration.as_secs_f32())
    .or_else(|| {
      app
        .player
        .current
        .as_ref()
        .and_then(|track| track.duration)
        .map(|duration| duration as f32)
    })
    .unwrap_or(0.0);
  let position = app.playback.position.as_secs_f32();
  let shown = app
    .ui
    .seek_preview
    .unwrap_or(position)
    .min(duration.max(0.001));
  (duration, shown)
}

/// Part du titre atteinte par `shown` (`targetProgress`, `app.js:3428`).
#[must_use]
pub fn scrub_target(duration: f32, shown: f32) -> f32 {
  if duration > 0.0 {
    (shown / duration).clamp(0.0, 1.0)
  } else {
    0.0
  }
}

impl App {
  /// Entrées du suivi de la barre à l'état actuel.
  fn bar_input(&self) -> BarInput {
    let (duration, shown) = scrub_extent(self);
    BarInput {
      button_playing: self.playback.is_playing || self.player.loading || self.playback.buffering,
      playing: self.playback.is_playing,
      position: self.playback.position.as_secs_f32(),
      progress: scrub_target(duration, shown),
      dragging: self.ui.seek_preview.is_some(),
      key: self.player.token,
    }
  }

  /// Date les changements d'aspect de la barre de lecture (voir `BarMotion::track`).
  pub(crate) fn track_playbar(&mut self) {
    let at = Instant::now();
    let input = self.bar_input();
    if self.playbar_motion.track(&input, at) {
      self.now = at;
    }
  }

  /// Progression de la barre affichée à `App::now`, suivi de la position compris.
  #[must_use]
  pub fn scrub_progress(&self, duration: f32, shown: f32) -> f32 {
    self
      .playbar_motion
      .scrub
      .value(self.now, scrub_target(duration, shown))
  }

  /// Phase de l'onde à `App::now`.
  #[must_use]
  pub fn wave_phase(&self) -> f32 {
    self
      .playbar_motion
      .wave
      .phase(self.now, self.playback.position.as_secs_f32())
  }
}

/// Points des deux chemins de `#play-icon` (`index.html:2824-2825`, `styles.css:3119-3130`) : lecture, puis pause. Même structure
/// (quatre points, chemin fermé), donc `d` s'interpole point par point.
const PLAY_PATHS: [[(f32, f32); 4]; 2] = [
  [(8.8, 5.8), (13.5, 8.8), (13.5, 15.2), (8.8, 18.2)],
  [(13.5, 8.8), (18.0, 12.0), (18.0, 12.0), (13.5, 15.2)],
];
const PAUSE_PATHS: [[(f32, f32); 4]; 2] = [
  [(6.8, 5.8), (9.2, 5.8), (9.2, 18.2), (6.8, 18.2)],
  [(14.8, 5.8), (17.2, 5.8), (17.2, 18.2), (14.8, 18.2)],
];

/// Points des deux chemins quand `d` est à la part `morph` de lecture (0) vers pause (1).
#[must_use]
pub fn morph_points(morph: f32) -> [[(f32, f32); 4]; 2] {
  let mut out = PLAY_PATHS;
  for (path, (play, pause)) in out.iter_mut().zip(PLAY_PATHS.iter().zip(&PAUSE_PATHS)) {
    for (point, (from, to)) in path.iter_mut().zip(play.iter().zip(pause)) {
      *point = (
        from.0 + (to.0 - from.0) * morph,
        from.1 + (to.1 - from.1) * morph,
      );
    }
  }
  out
}

/// Icône lecture/pause en cours de transformation : deux chemins pleins avec un trait rond de 1,6, `currentColor`.
pub struct MorphIcon {
  pub morph: f32,
  pub color: Color,
  /// Côté du SVG en pixels écran (22 px CSS).
  pub size: f32,
}

impl<Message> canvas::Program<Message> for MorphIcon {
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
    // Le SVG de 22 px reste centré dans sa cellule quand `size` rétrécit (échelle 0,65 pendant l'attente).
    frame.translate(Vector::new(
      (bounds.width - self.size) / 2.0,
      (bounds.height - self.size) / 2.0,
    ));
    frame.scale(self.size / bar::PLAY_ICON_VIEW_BOX);
    for points in morph_points(self.morph) {
      let path = Path::new(|builder| {
        builder.move_to(Point::new(points[0].0, points[0].1));
        for point in &points[1..] {
          builder.line_to(Point::new(point.0, point.1));
        }
        builder.close();
      });
      frame.fill(&path, self.color);
      frame.stroke(
        &path,
        Stroke::default()
          .with_color(self.color)
          .with_width(bar::PLAY_ICON_STROKE)
          .with_line_cap(LineCap::Round)
          .with_line_join(LineJoin::Round),
      );
    }
    vec![frame.into_geometry()]
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
  }

  fn jelly(now: Instant) -> Transition {
    Transition::rest(0.0, bar::PLAY_JELLY_MS, bar::PLAY_JELLY_CURVE, now)
  }

  #[test]
  fn the_jelly_curve_overshoots_and_settles() {
    let curve = bar::PLAY_JELLY_CURVE;
    let peak = (1..100)
      .map(|step| bezier_ease(curve, step as f32 / 100.0))
      .fold(0.0_f32, f32::max);
    assert!(peak > 1.05 && peak < 1.15, "{peak}");
    assert_eq!(
      (bezier_ease(curve, 0.0), bezier_ease(curve, 1.0)),
      (0.0, 1.0)
    );
    // Valeur calculée à la main : s = 0,5 donne x = 0,605 et y = 1,04.
    let x =
      |s: f32| 3.0 * (1.0 - s) * (1.0 - s) * s * 0.34 + 3.0 * (1.0 - s) * s * s * 0.64 + s * s * s;
    let y = |s: f32| 3.0 * (1.0 - s) * (1.0 - s) * s * 1.56 + 3.0 * (1.0 - s) * s * s + s * s * s;
    assert!((bezier_ease(curve, x(0.5)) - y(0.5)).abs() < 0.01);
  }

  #[test]
  fn the_button_width_follows_the_measured_jelly_between_play_and_pause() {
    // `10-lecture-pause/samples.json`, de 70 à 48 px : 64,15 px à 67,3 ms, 62,40 à 71,5, 52,99 à 100,7, 47,40 à 134, creux de 46,02 à
    // 183,9 ms, 48 à 254,8 ms. La CSS part à 55 ms après le déclencheur (latence d'injection : écart total de 0,3 px sur ces 5 points
    // pour 55 ms, 5,5 px pour 50 ms et 4 px pour 58 ms). Largeur = 48 + 22 s.
    let t0 = Instant::now();
    let mut transition = Transition::rest(1.0, bar::PLAY_JELLY_MS, bar::PLAY_JELLY_CURVE, t0);
    transition.retarget(0.0, t0);
    let width = |at: u64| 48.0 + 22.0 * transition.value(t0 + ms(at));
    assert!((width(0) - 70.0).abs() < 1e-3);
    let trough = (0..=200).map(&width).fold(f32::MAX, f32::min);
    assert!(trough < 46.5 && trough > 45.5, "{trough}");
    assert!((width(200) - 48.0).abs() < 1e-3);
    assert!((width(400) - 48.0).abs() < 1e-3);
    for (measured_at, measured) in [
      (67.3, 64.15),
      (71.5, 62.40),
      (100.7, 52.99),
      (134.0, 47.40),
      (183.9, 46.02),
    ] {
      let at = (measured_at - 55.0_f32).round() as u64;
      assert!(
        (width(at) - measured).abs() < 0.5,
        "{measured_at} ms : {}",
        width(at)
      );
    }
  }

  #[test]
  fn the_radius_goes_beyond_its_target_like_the_measure() {
    // Rayon mesuré : 24 -> 18 donne 24,54 au creux de la largeur (jelly), rayon = 24 - 6 s.
    let now = Instant::now();
    let mut state = PlayButton::default();
    assert!(!state.track(true, now));
    assert!(state.track(false, now));
    let css = |value: f32| value / crate::theme::tokens::ZOOM;
    let radius = |at: u64| css(state.pose(false, now + ms(at)).radius);
    assert!((radius(0) - 18.0).abs() < 1e-3);
    assert!((radius(300) - 24.0).abs() < 1e-3);
    // Au creux de la largeur (46 px) le rendu borne le rayon calculé (24,5) à la moitié du côté : 23 px.
    let trough = state.pose(false, now + ms(128));
    assert!(trough.radius <= trough.width / 2.0 + 1e-3);
    assert!(
      (css(trough.width) - 46.0).abs() < 0.2,
      "{}",
      css(trough.width)
    );
    assert!(
      (css(trough.radius) - 23.0).abs() < 0.1,
      "{}",
      css(trough.radius)
    );
    // Pause vers lecture : le rayon passe sous 18 (17,4 mesuré à 1405 ms).
    let mut back = PlayButton::default();
    back.track(false, now);
    back.track(true, now);
    let narrowest = (0..=200)
      .map(|at| css(back.pose(true, now + ms(at)).radius))
      .fold(f32::MAX, f32::min);
    assert!((narrowest - 17.41).abs() < 0.1, "{narrowest}");
  }

  #[test]
  fn the_first_state_is_not_animated_and_rest_has_no_morph() {
    let now = Instant::now();
    let mut state = PlayButton::default();
    assert!(!state.track(true, now));
    let pose = state.pose(true, now);
    assert_eq!(pose.morph, None);
    assert!((pose.width - bar::PLAY_WRAPPER_WIDTH).abs() < 1e-3);
    assert!(!state.animating(now));
    // Un état que le suivi n'a pas vu s'affiche au repos.
    let paused = state.pose(false, now);
    assert!((paused.width - bar::PLAY_WIDTH_PAUSED).abs() < 1e-3);
    assert_eq!(paused.morph, None);
  }

  #[test]
  fn the_icon_morph_interpolates_the_points_over_400_ms() {
    // `10-lecture-pause/samples.json` : à 67,3 ms le point 1 du chemin 1 est (6,81964 ; 5,8) soit 0,0098 de la course de 2 px.
    let now = Instant::now();
    let mut state = PlayButton::default();
    state.track(true, now);
    assert!(state.track(false, now));
    assert!(state.animating(now + ms(100)));
    assert!(state.animating(now + ms(399)));
    assert!(!state.animating(now + ms(401)));
    let morph = |at: u64| state.pose(false, now + ms(at)).morph;
    // Lecture -> pause part de la pause (1) vers la lecture (0).
    assert!(morph(0).is_some_and(|value| (value - 1.0).abs() < 1e-3));
    let mid = morph(100).expect("en cours");
    assert!(mid > 0.0 && mid < 1.0);
    // Abscisse du premier point du chemin 1 mesurée (6,8 vers 8,8) : la CSS part à 55 ms après le déclencheur.
    for (measured_at, measured) in [
      (67.3, 6.81964),
      (100.7, 7.22837),
      (134.0, 7.78993),
      (250.7, 8.54275),
      (379.8, 8.77362),
    ] {
      let at = (measured_at - 55.0_f32).round() as u64;
      let x = morph_points(morph(at).expect("en cours"))[0][0].0;
      assert!((x - measured).abs() < 0.01, "{measured_at} ms : {x}");
    }
    let points = morph_points(0.0);
    assert_eq!(points[0][0], (8.8, 5.8));
    assert_eq!(points[1][1], (18.0, 12.0));
    let pause = morph_points(1.0);
    assert_eq!(pause[0][0], (6.8, 5.8));
    assert_eq!(pause[1][2], (17.2, 18.2));
    let half = morph_points(0.5);
    assert!((half[0][0].0 - 7.8).abs() < 1e-5 && (half[0][1].0 - 11.35).abs() < 1e-5);
  }

  #[test]
  fn a_click_during_the_animation_restarts_from_the_displayed_value() {
    let t0 = Instant::now();
    let mut transition = jelly(t0);
    transition.retarget(1.0, t0);
    let before = transition.value(t0 + ms(60));
    assert!(before > 0.0 && before < 1.5);
    // Cible différente du départ : pas un retour en arrière (départ à 0, nouvelle cible 2).
    transition.retarget(2.0, t0 + ms(60));
    assert!((transition.value(t0 + ms(60)) - before).abs() < 1e-4);
    assert_eq!(transition.duration, ms(200));
  }

  #[test]
  fn reversing_shortens_the_duration_like_css_transitions() {
    let t0 = Instant::now();
    let mut transition = Transition::rest(0.0, 400, [0.0, 0.0, 1.0, 1.0], t0);
    transition.retarget(1.0, t0);
    // Linéaire : à 100 ms la valeur vaut 0,25 ; le retour vers 0 dure 400 * 0,25 = 100 ms.
    transition.retarget(0.0, t0 + ms(100));
    assert!(transition.duration.abs_diff(ms(100)) < Duration::from_micros(100));
    assert!((transition.value(t0 + ms(100)) - 0.25).abs() < 1e-4);
    assert!(transition.value(t0 + ms(150)) < 0.2);
    assert!(transition.value(t0 + ms(200)).abs() < 1e-4);
    // Après un retour, un nouvel aller est raccourci par le facteur cumulé : 0,5 * 0,25 + 0,75 = 0,875.
    transition.retarget(1.0, t0 + ms(150));
    assert!(
      (transition.factor - 0.875).abs() < 1e-3,
      "{}",
      transition.factor
    );
  }

  #[test]
  fn the_wave_advances_only_while_playing_and_resumes_where_it_stopped() {
    let t0 = Instant::now();
    let mut wave = Wave::default();
    wave.track(true, 0.0, t0);
    let phase = |wave: &Wave, at: u64| wave.phase(t0 + ms(at), 0.0);
    assert!(phase(&wave, 0).abs() < 1e-6);
    assert!((phase(&wave, 1000) - 3.5).abs() < 1e-4);
    // 3,5 rad/s : un tour (2π) en 1,795 s.
    assert!((phase(&wave, 2000) - (7.0 - TAU)).abs() < 1e-4);
    wave.track(false, 0.0, t0 + ms(1000));
    assert!((phase(&wave, 5000) - 3.5).abs() < 1e-4);
    wave.track(true, 0.0, t0 + ms(5000));
    assert!((phase(&wave, 5500) - (3.5 + 1.75)).abs() < 1e-4);
  }

  #[test]
  fn the_wave_before_any_tracking_keeps_the_position_based_phase() {
    let wave = Wave::default();
    let now = Instant::now();
    assert!((wave.phase(now, 105.0) - (105.0_f32 * 3.5) % TAU).abs() < 1e-6);
    // Premier état observé : même phase, puis l'horloge la fait avancer.
    let mut tracked = Wave::default();
    tracked.track(true, 105.0, now);
    assert_eq!(tracked.phase(now, 105.0), wave.phase(now, 105.0));
  }

  #[test]
  fn the_follow_matches_riffs_lerp_at_sixty_frames_per_second() {
    // Riff : p += (cible - p) * min(1, dt * 45) à chaque image de 1/60 s.
    let t0 = Instant::now();
    let mut follow = Follow::default();
    follow.track(0.0, false, 1, t0);
    follow.track(1.0, false, 1, t0);
    let mut lerp = 0.0_f32;
    for frame in 1..=10_u32 {
      lerp += (1.0 - lerp) * (45.0_f32 / 60.0).min(1.0);
      let at = t0 + Duration::from_secs_f64(f64::from(frame) / 60.0);
      assert!((follow.value(at, 1.0) - lerp).abs() < 1e-3, "image {frame}");
    }
    // Pendant un glissement : 32.
    let mut drag = Follow::default();
    drag.track(0.0, true, 1, t0);
    drag.track(1.0, true, 1, t0);
    let mut lerp = 0.0_f32;
    for frame in 1..=10_u32 {
      lerp += (1.0 - lerp) * (32.0_f32 / 60.0).min(1.0);
      let at = t0 + Duration::from_secs_f64(f64::from(frame) / 60.0);
      assert!((drag.value(at, 1.0) - lerp).abs() < 1e-3, "image {frame}");
    }
  }

  #[test]
  fn the_follow_snaps_on_a_new_track_and_settles() {
    let t0 = Instant::now();
    let mut follow = Follow::default();
    follow.track(0.8, false, 1, t0);
    assert_eq!(follow.value(t0, 0.8), 0.8);
    assert!(!follow.settling(t0));
    follow.track(0.0, false, 2, t0 + ms(10));
    assert_eq!(follow.value(t0 + ms(10), 0.0), 0.0);
    follow.track(0.5, false, 2, t0 + ms(20));
    assert!(follow.settling(t0 + ms(20)));
    assert!(!follow.settling(t0 + ms(500)));
    // Une cible que le suivi n'a pas vue s'affiche telle quelle.
    assert_eq!(follow.value(t0, 0.3), 0.3);
  }
}
