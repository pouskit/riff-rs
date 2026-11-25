//! Transitions CSS de Riff (`transition`, `animation` de `.content-view`) calculées sur l'horloge de `App` : l'instant de départ
//! vit dans l'état, le progrès se déduit de `App::now`, donc le banc peut rendre n'importe quel instant.

use crate::screens::Screen;
use crate::theme::tokens::artist as artist_tokens;
use crate::theme::tokens::audio;
use crate::theme::tokens::fade_up as fade;
use crate::theme::tokens::karaoke as tvk_tokens;
use crate::theme::tokens::motion as tokens;
use crate::theme::tokens::queue;
use crate::theme::tokens::search_results as search_tokens;
use crate::theme::tokens::touch as tokens_touch;
use crate::theme::tokens::{context_menu as menu_tokens, settings as settings_tokens};
use crate::theme::tokens::{sidebar as sidebar_tokens, track_row as row_tokens};
use crate::visuals::state::bezier_ease;
use iced::{Color, Point, Size};
use std::time::{Duration, Instant};

/// Interrupteurs globaux du mouvement : `html.force-reduced-motion` (`app.js:1094-1097`) et `body.perf` (`app.js:17010-17014`).
/// Un seul état par interrupteur, lu par toutes les animations ; les tests de la bibliothèque en ont un par fil (pas de fuite d'un
/// test à l'autre), l'application et le banc un seul pour le processus.
#[cfg(not(test))]
mod switches {
  use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

  static REDUCED: AtomicBool = AtomicBool::new(false);
  static PERF: AtomicBool = AtomicBool::new(false);
  /// 0 : lire le système, 1 : animations actives, 2 : réduction demandée.
  static SYSTEM: AtomicU8 = AtomicU8::new(0);

  pub fn reduced() -> bool {
    REDUCED.load(Ordering::Relaxed)
  }
  pub fn perf() -> bool {
    PERF.load(Ordering::Relaxed)
  }
  pub fn set_reduced(on: bool) {
    REDUCED.store(on, Ordering::Relaxed);
  }
  pub fn set_perf(on: bool) {
    PERF.store(on, Ordering::Relaxed);
  }
  pub fn system_override() -> Option<bool> {
    match SYSTEM.load(Ordering::Relaxed) {
      0 => None,
      value => Some(value == 2),
    }
  }
  pub fn set_system_override(value: Option<bool>) {
    SYSTEM.store(
      value.map_or(0, |reduced| 1 + u8::from(reduced)),
      Ordering::Relaxed,
    );
  }
  /// Sans injection : la préférence du système.
  pub fn read_system() -> bool {
    crate::system::platform::prefers_reduced_motion()
  }
}

#[cfg(test)]
mod switches {
  use std::cell::Cell;

  thread_local! {
    static REDUCED: Cell<bool> = const { Cell::new(false) };
    static PERF: Cell<bool> = const { Cell::new(false) };
    static SYSTEM: Cell<Option<bool>> = const { Cell::new(None) };
  }

  pub fn reduced() -> bool {
    REDUCED.with(Cell::get)
  }
  pub fn perf() -> bool {
    PERF.with(Cell::get)
  }
  pub fn set_reduced(on: bool) {
    REDUCED.with(|flag| flag.set(on));
  }
  pub fn set_perf(on: bool) {
    PERF.with(|flag| flag.set(on));
  }
  pub fn system_override() -> Option<bool> {
    SYSTEM.with(Cell::get)
  }
  pub fn set_system_override(value: Option<bool>) {
    SYSTEM.with(|flag| flag.set(value));
  }
  /// Les tests ne lisent jamais le système : animations actives.
  pub fn read_system() -> bool {
    false
  }
}

/// `animation-duration` et `transition-duration` imposées par `html.force-reduced-motion *` (`styles.css:182-189`).
pub const REDUCED_DURATION_MS: f32 = 0.001;

/// Réduction de mouvement active (`html.force-reduced-motion`).
#[must_use]
pub fn reduced() -> bool {
  switches::reduced()
}

/// Mode performance actif (`body.perf`).
#[must_use]
pub fn perf() -> bool {
  switches::perf()
}

/// Pose la réduction de mouvement (`applyReducedMotionSetting`).
pub fn set_reduced(on: bool) {
  switches::set_reduced(on);
}

/// Pose le mode performance (`applyPerfMode`).
pub fn set_perf(on: bool) {
  switches::set_perf(on);
}

/// Règles de Riff qui arrêtent une animation : `html.force-reduced-motion *` (`styles.css:182-189`) ne l'emporte que sur une
/// déclaration sans `!important`, ou plus tôt dans la feuille et moins spécifique ; `body.perf` agit par ses propres règles
/// (`transition: none`, `animation: none`) et par des gardes de `app.js`. Chaque animation dit lesquelles la visent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gate {
  reduced: bool,
  perf: bool,
}

impl Gate {
  /// Aucune des deux règles ne l'atteint.
  pub const NONE: Self = Self {
    reduced: false,
    perf: false,
  };
  /// Seule la réduction de mouvement l'atteint.
  pub const REDUCED: Self = Self {
    reduced: true,
    perf: false,
  };
  /// Seul `body.perf` l'atteint.
  pub const PERF: Self = Self {
    reduced: false,
    perf: true,
  };
  /// Les deux l'atteignent.
  pub const BOTH: Self = Self {
    reduced: true,
    perf: true,
  };

  /// Durée nulle : la valeur tient son départ pendant le délai, puis passe à la fin.
  #[must_use]
  pub fn instant(self) -> bool {
    (self.reduced && reduced()) || (self.perf && perf())
  }
}

/// `meshFloat` est arrêtée par `body.perf` (`styles.css:7542`), par la réduction (déclaration sans `!important`) et par
/// `@media (prefers-reduced-motion: reduce)` (`styles.css:175-180`), qui suit la préférence du système même si le réglage dit autre chose.
#[must_use]
pub fn mesh_frozen() -> bool {
  Gate::BOTH.instant() || system_prefers_reduced()
}

/// Préférence « réduire les animations » du système (`systemPrefersReducedMotion`, `app.js:1090`). Le banc l'impose par
/// [`inject_system_preference`] pour ne pas dépendre de la machine ; sinon Windows lit `SPI_GETCLIENTAREAANIMATION`.
#[must_use]
pub fn system_prefers_reduced() -> bool {
  switches::system_override().unwrap_or_else(switches::read_system)
}

/// Impose la préférence du système (`None` : la lire de nouveau).
pub fn inject_system_preference(value: Option<bool>) {
  switches::set_system_override(value);
}

/// Applique les deux réglages : la réduction suit la préférence du système tant qu'elle n'a pas été choisie (`getAdv('reduce_motion',
/// systemPrefersReducedMotion())`, `app.js:1095`).
pub fn apply_settings(reduce_motion: Option<bool>, perf_mode: bool) {
  set_reduced(reduce_motion.unwrap_or_else(system_prefers_reduced));
  set_perf(perf_mode);
}

/// Durée effective d'une transition ou d'une animation finie : `.001ms` quand la règle de `gate` l'atteint.
#[must_use]
pub fn effective_ms(gate: Gate, duration_ms: f32) -> f32 {
  if gate.instant() {
    REDUCED_DURATION_MS
  } else {
    duration_ms
  }
}

/// Millisecondes écoulées depuis `start` pour une animation finie sans décalage : infini quand elle est déjà terminée sous une
/// règle de `gate`, donc toute comparaison à sa durée la trouve finie.
#[must_use]
pub fn elapsed_ms(gate: Gate, start: Instant, now: Instant) -> f32 {
  if gate.instant() {
    return f32::INFINITY;
  }
  raw_elapsed_ms(start, now)
}

/// Millisecondes écoulées, sans égard aux interrupteurs : animations que ni la réduction ni le mode performance n'arrêtent.
#[must_use]
pub fn raw_elapsed_ms(start: Instant, now: Instant) -> f32 {
  now.saturating_duration_since(start).as_secs_f32() * 1000.0
}

/// Durée et courbe d'une transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
  pub duration_ms: f32,
  pub curve: [f32; 4],
  pub gate: Gate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Run {
  from: f32,
  to: f32,
  start: Instant,
  duration_ms: f32,
  curve: [f32; 4],
  gate: Gate,
}

impl Run {
  fn progress(&self, now: Instant) -> f32 {
    if self.gate.instant() {
      // Durée nulle : la valeur tient son départ pendant `transition-delay`, puis passe à la fin.
      return if now < self.start { 0.0 } else { 1.0 };
    }
    if self.duration_ms <= 0.0 {
      return 1.0;
    }
    (now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 / self.duration_ms).min(1.0)
  }

  /// Entrée finie sur `start` (`m3FadeUp` et voisines), de durée `duration_ms` sous `gate`.
  const fn entry(gate: Gate, start: Instant, duration_ms: f32, curve: [f32; 4]) -> Self {
    Self {
      from: 0.0,
      to: 1.0,
      start,
      duration_ms,
      curve,
      gate,
    }
  }

  /// Progrès adouci de l'entrée après `delay_ms` de décalage, `None` quand elle est finie ; avant le décalage : 0.
  fn entered(&self, delay_ms: f32, now: Instant) -> Option<f32> {
    let elapsed_ms = raw_elapsed_ms(self.start, now);
    let progress = ((elapsed_ms - delay_ms) / effective_ms(self.gate, self.duration_ms)).max(0.0);
    (progress < 1.0).then(|| bezier_ease(self.curve, progress))
  }

  /// `m3FadeUp` et entrées voisines : progrès adouci d'une entrée de courbe `curve` partie de `start` (`None` : jamais lancée),
  /// `None` après `départ + décalage + durée` ; avant le décalage : 0.
  fn fade_up(
    gate: Gate,
    start: Option<Instant>,
    delay_ms: f32,
    duration_ms: f32,
    curve: [f32; 4],
    now: Instant,
  ) -> Option<f32> {
    Self::entry(gate, start?, duration_ms, curve).entered(delay_ms, now)
  }

  fn value(&self, now: Instant) -> f32 {
    let progress = self.progress(now);
    if progress >= 1.0 {
      return self.to;
    }
    self.from + (self.to - self.from) * bezier_ease(self.curve, progress)
  }

  /// Durée d'une transition qui revient vers le départ de celle-ci : la durée pleine raccourcie du chemin déjà parcouru
  /// (`reversing shortening factor`, CSS Transitions 1, section 3) ; la durée pleine si `target` n'est pas le départ.
  fn reversed_ms(&self, duration_ms: f32, current: f32, target: f32) -> f32 {
    if (self.from - target).abs() < f32::EPSILON && (self.to - self.from).abs() > f32::EPSILON {
      duration_ms * ((current - target) / (self.to - self.from)).abs().min(1.0)
    } else {
      duration_ms
    }
  }
}

/// Valeur animée d'une propriété : une `transition` CSS repart de la valeur courante, et une inversion de la transition en cours
/// raccourcit la durée du chemin déjà parcouru (`reversing shortening factor`, CSS Transitions 1, section 3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Track {
  target: f32,
  run: Option<Run>,
}

impl Track {
  #[must_use]
  pub const fn at(value: f32) -> Self {
    Self {
      target: value,
      run: None,
    }
  }

  #[must_use]
  pub fn value(&self, now: Instant) -> f32 {
    self.run.map_or(self.target, |run| run.value(now))
  }

  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    self.run.is_some_and(|run| run.progress(now) < 1.0)
  }

  /// Vise `target` à `now` ; sans effet si la cible est déjà celle-là.
  pub fn set(&mut self, target: f32, now: Instant, timing: Timing) {
    self.set_after(target, now, 0.0, timing);
  }

  /// `transition-delay` : la valeur courante tient `delay_ms` avant que la transition ne parte de `now`.
  pub fn set_after(&mut self, target: f32, now: Instant, delay_ms: f32, timing: Timing) {
    if (self.target - target).abs() < f32::EPSILON {
      return;
    }
    let current = self.value(now);
    let duration_ms = self.run.map_or(timing.duration_ms, |run| {
      run.reversed_ms(timing.duration_ms, current, target)
    });
    self.target = target;
    self.run = Some(Run {
      from: current,
      to: target,
      start: now + Duration::from_secs_f32(delay_ms / 1000.0),
      duration_ms,
      curve: timing.curve,
      gate: timing.gate,
    });
  }
}

/// Pistes par clé (carte, bouton) : une clé absente vaut `fallback`, l'état posé sans message (le banc, les réglages).
#[derive(Debug, Clone, PartialEq)]
pub struct Tracks<K> {
  entries: Vec<(K, Track)>,
}

impl<K> Default for Tracks<K> {
  fn default() -> Self {
    Self {
      entries: Vec::new(),
    }
  }
}

impl<K: Copy + PartialEq> Tracks<K> {
  /// Pose la cible 0 ou 1 de `key` : la piste part de 0 si elle n'existait pas.
  pub fn set(&mut self, key: K, target: f32, now: Instant, timing: Timing) {
    self.set_after(key, target, now, 0.0, timing);
  }

  /// Comme [`Tracks::set`], avec un `transition-delay` de `delay_ms`.
  pub fn set_after(&mut self, key: K, target: f32, now: Instant, delay_ms: f32, timing: Timing) {
    self
      .entries
      .retain(|(other, track)| *other == key || track.running(now) || track.target > 0.0);
    if let Some((_, track)) = self.entries.iter_mut().find(|(other, _)| *other == key) {
      track.set_after(target, now, delay_ms, timing);
      return;
    }
    let mut track = Track::at(0.0);
    track.set_after(target, now, delay_ms, timing);
    self.entries.push((key, track));
  }

  /// Valeur de `key` à `now`, ou `fallback` sans piste.
  #[must_use]
  pub fn value(&self, key: K, now: Instant, fallback: f32) -> f32 {
    self
      .entries
      .iter()
      .find(|(other, _)| *other == key)
      .map_or(fallback, |(_, track)| track.value(now))
  }

  /// Une piste existe pour `key`.
  #[must_use]
  pub fn contains(&self, key: K) -> bool {
    self.entries.iter().any(|(other, _)| *other == key)
  }

  /// Cible posée pour `key` (0 sans piste).
  #[must_use]
  pub fn target(&self, key: K) -> f32 {
    self
      .entries
      .iter()
      .find(|(other, _)| *other == key)
      .map_or(0.0, |(_, track)| track.target)
  }

  #[must_use]
  pub fn animating(&self, now: Instant) -> bool {
    self.entries.iter().any(|(_, track)| track.running(now))
  }

  /// Clés qui ont une piste.
  pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
    self.entries.iter().map(|(key, _)| *key)
  }

  /// La piste de `key` est au repos (cible 0, rien ne bouge) ou n'existe pas : elle peut être retirée sans changer le rendu.
  #[must_use]
  pub fn settled(&self, key: K, now: Instant) -> bool {
    self
      .entries
      .iter()
      .find(|(other, _)| *other == key)
      .is_none_or(|(_, track)| track.target <= 0.0 && !track.running(now))
  }

  /// Retire la piste de `key`.
  pub fn remove(&mut self, key: K) {
    self.entries.retain(|(other, _)| *other != key);
  }
}

/// `--hl`, `background-color` : `.18s ease` (`styles.css:7509`).
pub const HOVER: Timing = Timing {
  duration_ms: tokens::HOVER_MS,
  curve: tokens::HOVER_CURVE,
  gate: Gate::PERF,
};
/// `border-radius` des boutons : `.22s cubic-bezier(.2, 0, 0, 1)` (`styles.css:7509`).
pub const RADIUS: Timing = Timing {
  duration_ms: tokens::RADIUS_MS,
  curve: tokens::RADIUS_CURVE,
  gate: Gate::PERF,
};
/// Levée de la pochette d'une carte : `transform 180ms ease` (`styles.css:9378`).
pub const CARD: Timing = Timing {
  duration_ms: tokens::CARD_MS,
  curve: tokens::CARD_CURVE,
  gate: Gate::REDUCED,
};
/// Fond de survol d'une ligne de titre : `background-color 140ms ease !important` (`styles.css:1589`).
pub const ROW_HOVER: Timing = Timing {
  duration_ms: row_tokens::HOVER_MS,
  curve: row_tokens::HOVER_CURVE,
  gate: Gate::PERF,
};

/// Survol, appui et onde d'un bouton : fond et calque `--hl` d'un côté, rayon de l'autre, à des durées différentes, puis l'état
/// `.pressed` (`app.js:2607-2620`) et les ondes `rippleOut` (`app.js:2622-2638`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ControlHover {
  color: Tracks<ControlKey>,
  shape: Tracks<ControlKey>,
  presses: Vec<PressRun>,
  ripples: Vec<Ripple>,
}

/// État animé d'un bouton rond : 0 au repos, 1 survolé, pour la couleur et pour le rayon, et 1 tant que `.pressed` est posée.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HoverMix {
  pub color: f32,
  pub shape: f32,
  /// Avancement du fond de `.pressed` (`tokens::touch::PRESS_MS`).
  pub press: f32,
  /// Cible de la transition : le pointeur est dessus.
  pub hovered: bool,
  /// Une piste de survol existe : un message d'entrée ou de sortie du pointeur est arrivé. Sans piste, `Status` d'iced fait foi (écrans posés).
  pub tracked: bool,
}

/// Onde posée par un clic : point d'origine dans le bouton, distance au coin le plus éloigné, instant du `pointerdown`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ripple {
  key: ControlKey,
  origin: Point,
  reach: f32,
  start: Instant,
}

/// Disque d'une onde à un instant : centre dans le bouton, rayon et opacité courants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RipplePose {
  pub origin: Point,
  pub radius: f32,
  pub alpha: f32,
}

/// Point d'entrée unique du mode performance : `body.perf` coupe les ondes (`ripple-host` en `display: none`, `styles.css:7540` ;
/// garde de `app.js:2624`). La réduction de mouvement ne les coupe pas : `rippleOut` (`styles.css:7503`, sans `!important`) finit à
/// l'instant, voir `Ripple::progress`.
#[must_use]
pub fn ripples_enabled() -> bool {
  !perf()
}

/// `app.js:2630` : le diamètre de l'onde est le double de la distance du point au coin le plus éloigné ; ici le rayon.
#[must_use]
pub fn ripple_reach(origin: Point, size: Size) -> f32 {
  (origin.x.max(size.width - origin.x)).hypot(origin.y.max(size.height - origin.y))
}

impl ControlHover {
  pub fn set(&mut self, key: impl Into<ControlKey>, hovered: bool, now: Instant) {
    let key = key.into();
    let target = if hovered { 1.0 } else { 0.0 };
    self.color.set(key, target, now, HOVER);
    self.shape.set(key, target, now, RADIUS);
    self.prune(now);
  }

  /// L'élément de `key` n'existe plus sous le pointeur : un élément retiré du DOM perd son état, un élément en `display: none` perd
  /// `:hover`, et au réaffichage l'état est posé tel quel, sans transition. Survol, appui et ondes sont retirés d'un coup ; l'élément
  /// recréé n'est survolé que si le pointeur est dessus (`widgets::hover::HoverTrack`).
  pub fn forget(&mut self, key: impl Into<ControlKey>) {
    let key = key.into();
    self.color.remove(key);
    self.shape.remove(key);
    self.presses.retain(|run| run.key != key);
    self.ripples.retain(|ripple| ripple.key != key);
  }

  /// Retire l'état des clés dont l'animation est finie : survol revenu au repos (couleur et rayon), appui descendu, onde terminée.
  /// Sans cela chaque bouton touché laisserait une entrée pour toute la durée du processus.
  pub fn prune(&mut self, now: Instant) {
    let settled: Vec<ControlKey> = self
      .color
      .keys()
      .chain(self.shape.keys())
      .filter(|key| self.color.settled(*key, now) && self.shape.settled(*key, now))
      .collect();
    for key in settled {
      self.color.remove(key);
      self.shape.remove(key);
    }
    self.presses.retain(|run| !run.finished(now));
    self.ripples.retain(|ripple| ripple.running(now));
  }

  /// Nombre de clés qui portent encore un état (survol, appui ou onde).
  #[must_use]
  pub fn tracked(&self) -> usize {
    self.color.keys().count() + self.shape.keys().count() + self.presses.len() + self.ripples.len()
  }

  /// `pointerdown` : `.pressed` est posée (`app.js:2610`), le fond monte en 180 ms (`tokens::touch::PRESS_MS`) depuis sa valeur courante.
  pub fn press(&mut self, key: impl Into<ControlKey>, now: Instant) {
    let key = key.into();
    let from = self.press_value(key, now);
    self
      .presses
      .retain(|run| run.key != key && !run.finished(now));
    self.presses.push(PressRun {
      key,
      start: now,
      from,
      release_at: None,
    });
  }

  /// `pointerup` : `.pressed` part `PRESS_MIN` après le `pointerdown` (`app.js:2615`), au plus tôt maintenant ; le fond redescend
  /// ensuite en 180 ms (`tokens::touch::PRESS_MS`).
  pub fn release(&mut self, key: impl Into<ControlKey>, now: Instant) {
    let key = key.into();
    if let Some(run) = self
      .presses
      .iter_mut()
      .find(|run| run.key == key && run.release_at.is_none())
    {
      let minimum =
        run.start + Duration::from_micros((tokens_touch::PRESS_MIN_MS * 1000.0).round() as u64);
      run.release_at = Some(now.max(minimum));
    }
  }

  fn press_value(&self, key: ControlKey, now: Instant) -> f32 {
    self
      .presses
      .iter()
      .rev()
      .find(|run| run.key == key)
      .map_or(0.0, |run| run.value(now))
  }

  /// Onde au point `origin` (relatif au bouton) d'un bouton de taille `size` (`app.js:2625-2637`).
  pub fn ripple(&mut self, key: impl Into<ControlKey>, origin: Point, size: Size, now: Instant) {
    if !ripples_enabled() {
      return;
    }
    self.ripples.retain(|ripple| ripple.running(now));
    self.ripples.push(Ripple {
      key: key.into(),
      origin,
      reach: ripple_reach(origin, size),
      start: now,
    });
  }

  #[must_use]
  pub fn mix(&self, key: impl Into<ControlKey>, now: Instant) -> HoverMix {
    let key = key.into();
    HoverMix {
      color: self.color.value(key, now, 0.0),
      shape: self.shape.value(key, now, 0.0),
      press: self.press_value(key, now),
      hovered: self.color.target(key) > 0.5,
      tracked: self.color.contains(key),
    }
  }

  /// Ondes en cours de `key` : rayon `scale(0)` vers `scale(1)` et opacité `.16` vers 0 sur la même courbe (`styles.css:7504`).
  #[must_use]
  pub fn ripples_of(&self, key: impl Into<ControlKey>, now: Instant) -> Vec<RipplePose> {
    let key = key.into();
    self
      .ripples
      .iter()
      .filter(|ripple| ripple.key == key && ripple.running(now))
      .map(|ripple| {
        let eased = bezier_ease(tokens_touch::RIPPLE_CURVE, ripple.progress(now));
        RipplePose {
          origin: ripple.origin,
          radius: ripple.reach * eased,
          alpha: tokens_touch::RIPPLE_ALPHA * (1.0 - eased),
        }
      })
      .collect()
  }

  #[must_use]
  pub fn animating(&self, now: Instant) -> bool {
    self.color.animating(now)
      || self.shape.animating(now)
      || self.presses.iter().any(|run| run.running(now))
      || self.ripples.iter().any(|ripple| ripple.running(now))
  }
}

/// `.pressed` d'un bouton : le fond monte en 180 ms (`tokens::touch::PRESS_MS`) depuis `from` au `pointerdown`, tient jusqu'à `release_at` (relâchement, mais pas
/// avant `PRESS_MIN`), puis redescend en 180 ms (`tokens::touch::PRESS_MS`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct PressRun {
  key: ControlKey,
  start: Instant,
  from: f32,
  release_at: Option<Instant>,
}

impl PressRun {
  /// `transition: none !important` de `body.perf` (`styles.css:7541`) l'emporte sur le `!important` de `styles.css:7509` : le fond est instantané.
  fn duration_ms() -> f32 {
    effective_ms(Gate::PERF, tokens_touch::PRESS_MS)
  }

  fn ease(from_instant: Instant, now: Instant) -> f32 {
    let progress = (now.saturating_duration_since(from_instant).as_secs_f32() * 1000.0
      / Self::duration_ms())
    .min(1.0);
    bezier_ease(tokens_touch::PRESS_CURVE, progress)
  }

  fn rise(&self, now: Instant) -> f32 {
    self.from + (1.0 - self.from) * Self::ease(self.start, now)
  }

  fn value(&self, now: Instant) -> f32 {
    match self.release_at {
      Some(at) if now >= at => self.rise(at) * (1.0 - Self::ease(at, now)),
      _ => self.rise(now),
    }
  }

  fn fall_end(&self) -> Option<Instant> {
    self
      .release_at
      .map(|at| at + Duration::from_micros((Self::duration_ms() * 1000.0).round() as u64))
  }

  /// La valeur change encore : montée, attente du relâchement ou descente.
  fn running(&self, now: Instant) -> bool {
    let rising =
      now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 < Self::duration_ms();
    rising || self.fall_end().is_some_and(|end| now < end)
  }

  fn finished(&self, now: Instant) -> bool {
    self.fall_end().is_some_and(|end| now >= end)
  }
}

impl Ripple {
  /// `rippleOut ... forwards` (`styles.css:7503`) n'est pas `!important` : sous la réduction elle dure 0,001 ms et finit à opacité 0.
  fn progress(&self, now: Instant) -> f32 {
    (now.saturating_duration_since(self.start).as_secs_f32() * 1000.0
      / effective_ms(Gate::REDUCED, tokens_touch::RIPPLE_MS))
    .min(1.0)
  }

  fn running(&self, now: Instant) -> bool {
    self.progress(now) < 1.0
  }
}

/// Clé d'un bouton dans [`ControlHover`] : hachage de 64 bits, sans chaîne ni fuite mémoire. Chaque instance d'un bouton a la sienne
/// (voir [`touch_key`] et [`keyed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlKey(u64);

impl ControlKey {
  /// Clé de n'importe quelle valeur hachable (identifiant de liste et rang, libellé, ...). Le hachage est celui de `DefaultHasher::new()`,
  /// stable pendant tout le processus.
  #[must_use]
  pub fn new(parts: impl std::hash::Hash) -> Self {
    use std::hash::Hasher;
    let mut hasher = std::hash::DefaultHasher::new();
    parts.hash(&mut hasher);
    Self(hasher.finish())
  }
}

/// Nom donné par l'appelant : `"queue"`, un glyphe d'icône.
impl From<&'static str> for ControlKey {
  fn from(name: &'static str) -> Self {
    Self::new(("name", name))
  }
}

/// Contexte de construction d'une vue : état animé, horloge, sels posés par [`keyed`] et rang d'apparition de chaque clé.
struct ViewScope {
  hover: ControlHover,
  now: Instant,
  salts: Vec<u64>,
  seen: std::collections::HashMap<ControlKey, u32>,
}

thread_local! {
  /// État du survol et des ondes lu par les boutons pendant `App::view` (leurs constructeurs n'ont pas accès à `App`).
  static VIEW_SCOPE: std::cell::RefCell<Option<ViewScope>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
  /// Clés des widgets survolés qui ont quitté l'arbre depuis le dernier message : leur sortie du pointeur n'arrivera jamais.
  static GONE: std::cell::RefCell<Vec<ControlKey>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Signale qu'un widget survolé de clé `key` quitte l'arbre (`Drop` de `widgets::hover::HoverTrack`).
pub fn mark_gone(key: ControlKey) {
  // `Drop` peut tourner pendant la destruction des variables locales du fil : la file n'existe plus, personne ne la lira.
  let _ = GONE.try_with(|gone| gone.borrow_mut().push(key));
}

/// Clés signalées par [`mark_gone`] depuis le dernier appel : `App::update` les oublie avant de traiter chaque message.
pub fn take_gone() -> Vec<ControlKey> {
  GONE.with(|gone| std::mem::take(&mut *gone.borrow_mut()))
}

/// Exécute `build` (la construction de la vue) avec l'état animé des boutons : `button::filled` et les autres le lisent par
/// [`view_mix`] et [`view_ripples`]. L'horloge est celle de `App`, jamais `Instant::now()`.
pub fn with_view_scope<R>(hover: &ControlHover, now: Instant, build: impl FnOnce() -> R) -> R {
  let scope = ViewScope {
    hover: hover.clone(),
    now,
    salts: Vec::new(),
    seen: std::collections::HashMap::new(),
  };
  let previous = VIEW_SCOPE.with(|slot| slot.replace(Some(scope)));
  let result = build();
  VIEW_SCOPE.with(|slot| *slot.borrow_mut() = previous);
  result
}

/// Donne `salt` (identifiant de liste et rang, hachage d'un titre) aux boutons construits par `build` : deux lignes d'une liste dont
/// les boutons ont le même lieu d'appel et le même libellé ont ainsi des clés distinctes, qui suivent la ligne si la liste change d'ordre.
pub fn keyed<R>(salt: impl std::hash::Hash, build: impl FnOnce() -> R) -> R {
  let salt = ControlKey::new(salt).0;
  VIEW_SCOPE.with(|slot| {
    if let Some(scope) = slot.borrow_mut().as_mut() {
      scope.salts.push(salt);
    }
  });
  let result = build();
  VIEW_SCOPE.with(|slot| {
    if let Some(scope) = slot.borrow_mut().as_mut() {
      scope.salts.pop();
    }
  });
  result
}

/// Survol et appui animés de `key` dans la vue en construction ; au repos hors de `with_view_scope` (tests de widgets).
#[must_use]
pub fn view_mix(key: impl Into<ControlKey>) -> HoverMix {
  let key = key.into();
  VIEW_SCOPE.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map_or_else(HoverMix::default, |scope| scope.hover.mix(key, scope.now))
  })
}

/// Ondes en cours de `key` dans la vue en construction.
#[must_use]
pub fn view_ripples(key: impl Into<ControlKey>) -> Vec<RipplePose> {
  let key = key.into();
  VIEW_SCOPE.with(|slot| {
    slot
      .borrow()
      .as_ref()
      .map_or_else(Vec::new, |scope| scope.hover.ripples_of(key, scope.now))
  })
}

/// Fin du chemin d'un fichier source : la clé ne dépend pas de l'emplacement du dépôt.
fn file_name(path: &str) -> &str {
  path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Clé d'un bouton dont l'appelant n'en donne pas : fichier de l'appel, libellé, sels de [`keyed`] et rang d'apparition dans la vue en
/// construction (le n-ième bouton de même fichier, même libellé et même sel). Deux boutons du même lieu d'appel et du même libellé
/// ont donc deux clés. La ligne n'y entre pas : le banc retrouve un bouton par [`button_key`].
#[must_use]
pub fn touch_key(site: &std::panic::Location<'_>, label: &str) -> ControlKey {
  VIEW_SCOPE.with(|slot| {
    let mut slot = slot.borrow_mut();
    let salts: &[u64] = slot.as_ref().map_or(&[], |scope| &scope.salts);
    let base = ControlKey::new((file_name(site.file()), label, salts));
    let Some(scope) = slot.as_mut() else {
      return base;
    };
    let rank = scope.seen.entry(base).or_insert(0);
    let key = ControlKey::new((base, *rank));
    *rank += 1;
    key
  })
}

/// Clé du bouton de `file` (fin du chemin source) portant `label`, au rang `rank` d'apparition dans la vue (0 pour le premier), hors
/// sel de [`keyed`] : le banc pose ainsi le survol d'un bouton précis sans connaître sa ligne.
#[must_use]
pub fn button_key(file: &str, label: &str, rank: u32) -> ControlKey {
  let no_salt: &[u64] = &[];
  ControlKey::new((ControlKey::new((file, label, no_salt)), rank))
}

/// Comme [`button_key`] pour un bouton construit sous [`keyed`]`(salt, ..)`, au rang `rank` parmi les boutons de ce sel.
#[must_use]
pub fn button_key_with(
  salt: impl std::hash::Hash,
  file: &str,
  label: &str,
  rank: u32,
) -> ControlKey {
  let salts: &[u64] = &[ControlKey::new(salt).0];
  ControlKey::new((ControlKey::new((file, label, salts)), rank))
}

/// Pose d'une vue pendant `m3ContainerOpen` (`styles.css:1009-1011`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewPose {
  pub opacity: f32,
  /// `translateY`, en pixels écran.
  pub offset_y: f32,
  pub scale: f32,
}

/// Pose d'une entrée de progrès adouci `eased` : l'opacité suit `eased`, le décalage et l'échelle partent de `offset` et `scale_from`
/// (`1.0` : pas d'échelle) et rejoignent le repos avec le reste `1 - eased`.
fn eased_pose(eased: f32, offset: f32, scale_from: f32) -> ViewPose {
  let rest = 1.0 - eased;
  ViewPose {
    opacity: eased,
    offset_y: offset * rest,
    scale: 1.0 - (1.0 - scale_from) * rest,
  }
}

/// Transition de vue (`.content-view.active`, `switchView` `app.js:13724`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewMotion {
  /// Faux hors de l'application lancée (banc, tests) : la navigation y pose l'écran au repos, comme le splash.
  pub enabled: bool,
  pub start: Option<Instant>,
}

impl ViewMotion {
  /// `.content-view` de `index.html` : les réglages (modale), le tiroir des playlists et l'écran de développement n'en sont pas.
  #[must_use]
  pub const fn is_view(screen: Screen) -> bool {
    !matches!(screen, Screen::Settings | Screen::Playlists | Screen::Dev)
  }

  /// Changement d'écran : la vue qui devient active rejoue l'animation, la sortante disparaît sans fondu.
  pub fn navigate(&mut self, from: Screen, to: Screen, now: Instant) {
    if self.enabled && from != to && Self::is_view(from) && Self::is_view(to) {
      self.start = Some(now);
    }
  }

  fn progress(&self, now: Instant) -> Option<f32> {
    let elapsed_ms = elapsed_ms(Gate::REDUCED, self.start?, now);
    (elapsed_ms < tokens::VIEW_MS).then(|| elapsed_ms / tokens::VIEW_MS)
  }

  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    self.progress(now).is_some()
  }

  /// `None` au repos : la vue se dessine sans transformation.
  #[must_use]
  pub fn pose(&self, now: Instant) -> Option<ViewPose> {
    let eased = bezier_ease(tokens::VIEW_CURVE, self.progress(now)?);
    Some(eased_pose(
      eased,
      tokens::VIEW_OFFSET,
      tokens::VIEW_SCALE_FROM,
    ))
  }
}

/// Entrée et sortie de la vue plein écran karaoké (`.tv-karaoke`, `tvk-in` et `tvk-out`, `styles.css:6266-6292`) et horloge des orbes
/// du fond (`tvkOrbit1-4`, `styles.css:6365-6384`). L'instant d'ouverture date `tvk-in` et lance les orbes ; la fermeture date
/// `tvk-out` et fige les orbes (`.tvk-bg-paused`, `app.js:6852-6856`). Faux hors de l'application lancée (banc, tests) : la vue
/// se pose ouverte, orbes à la première image clé.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TvkMotion {
  pub enabled: bool,
  opened: Option<Instant>,
  closing: Option<Instant>,
}

impl TvkMotion {
  /// Transitions actives, vue fermée.
  #[must_use]
  pub fn launched() -> Self {
    Self {
      enabled: true,
      ..Self::default()
    }
  }

  /// `openTvKaraoke` : `.hidden` et `.closing` retirés, `tvk-in` et les orbes repartent de zéro.
  pub fn open(&mut self, now: Instant) {
    self.opened = Some(now);
    self.closing = None;
  }

  /// `closeTvKaraoke` : `.closing` posé, la vue reste affichée pendant `tvk-out`.
  pub fn close(&mut self, now: Instant) {
    if self.enabled && self.opened.is_some() && self.closing.is_none() {
      self.closing = Some(now);
    }
  }

  /// La vue fermée finit de s'effacer : `tvk-out` dure 300 ms, puis `opacity: 0` reste jusqu'au retrait à 320 ms (`both`).
  /// `.tv-karaoke.closing` pose `animation: tvk-out ... !important` (`styles.css:6291`) : plus spécifique que
  /// `.force-reduced-motion *`, elle ignore la réduction, et `body.perf` n'en dit rien.
  #[must_use]
  pub fn leaving(&self, now: Instant) -> bool {
    self
      .closing
      .is_some_and(|start| raw_elapsed_ms(start, now) < tvk_tokens::HIDE_MS)
  }

  /// Une image de plus est nécessaire : entrée ou sortie en cours.
  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    self.enabled
      && (self.leaving(now)
        || (self.closing.is_none()
          && self
            .opened
            .is_some_and(|start| elapsed_ms(Gate::REDUCED, start, now) < tvk_tokens::ENTER_MS)))
  }

  /// `scale(s) translateY(y)` autour du centre : le `translateY` est mis à l'échelle (`matrix(0.96, 0, 0, 0.96, 0, 30.72)`).
  fn pose_at(rest: f32) -> ViewPose {
    let scale = 1.0 - (1.0 - tvk_tokens::POSE_SCALE_FROM) * rest;
    ViewPose {
      opacity: 1.0 - rest,
      offset_y: tvk_tokens::POSE_OFFSET * rest * scale,
      scale,
    }
  }

  /// `None` au repos : la vue se dessine sans transformation ni opacité.
  #[must_use]
  pub fn pose(&self, now: Instant) -> Option<ViewPose> {
    if !self.enabled {
      return None;
    }
    if let Some(start) = self.closing {
      let progress = (raw_elapsed_ms(start, now) / tvk_tokens::EXIT_MS).min(1.0);
      return Some(Self::pose_at(bezier_ease(tvk_tokens::EXIT_CURVE, progress)));
    }
    let progress = elapsed_ms(Gate::REDUCED, self.opened?, now) / tvk_tokens::ENTER_MS;
    (progress < 1.0).then(|| Self::pose_at(1.0 - bezier_ease(tvk_tokens::ENTER_CURVE, progress)))
  }

  /// Secondes écoulées sur l'horloge des orbes : 0 hors de l'application lancée, figée à la fermeture. `body.perf` les met en pause
  /// (`animation-play-state: paused`, `styles.css:6359`), donc sur la première image clé. La réduction de mouvement ne les arrête pas :
  /// `animation: tvkOrbit1-4 ... infinite alternate !important` (`styles.css:6337-6354`) est plus spécifique que `.force-reduced-motion *`.
  #[must_use]
  pub fn orbit_seconds(&self, now: Instant) -> f32 {
    match (self.enabled, self.opened) {
      (true, Some(start)) if !perf() => raw_elapsed_ms(start, self.closing.unwrap_or(now)) / 1000.0,
      _ => 0.0,
    }
  }

  /// Les orbes avancent : vue plein écran ouverte, mode performance éteint.
  #[must_use]
  pub fn orbiting(&self) -> bool {
    self.enabled && !perf()
  }
}

/// Pose du panneau de personnalisation de la vue plein écran pendant une transition (`.tv-karaoke .tvk-panel`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TvkPanelPose {
  pub opacity: f32,
  /// `translate3d(.., y, 0)`, en pixels écran, appliqué avant l'échelle.
  pub offset_y: f32,
  pub scale: f32,
  /// Part de `hauteur + 20px` visible depuis le haut : `clip-path: inset(0 -35px bottom -35px)`.
  pub reveal: f32,
}

impl TvkPanelPose {
  /// Panneau ouvert et au repos : ni décalage, ni échelle, découpe de `.open` tenue.
  pub const REST: Self = Self {
    opacity: 1.0,
    offset_y: 0.0,
    scale: 1.0,
    reveal: 1.0,
  };
}

/// Durées d'un panneau qui s'ouvre et se ferme par deux pistes (forme, opacité) : la forme a une durée pour les deux sens, l'opacité
/// une durée par sens, et la fermeture peut retarder le fondu.
struct PanelTimings {
  shape: Timing,
  fade_in: Timing,
  fade_out: Timing,
  close_delay_ms: f32,
}

/// Les deux pistes d'un panneau : forme (`transform`, `clip-path`) et opacité, 0 fermé et 1 ouvert. `visibility` ne passe à `hidden`
/// qu'à la fin de la fermeture, donc le panneau reste dessiné tant qu'une piste court.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct PanelTracks {
  opacity: Track,
  shape: Track,
}

impl PanelTracks {
  const fn at(rest: f32) -> Self {
    Self {
      opacity: Track::at(rest),
      shape: Track::at(rest),
    }
  }

  fn set(&mut self, open: bool, now: Instant, timings: &PanelTimings) {
    if open {
      self.shape.set(1.0, now, timings.shape);
      self.opacity.set(1.0, now, timings.fade_in);
    } else {
      self.shape.set(0.0, now, timings.shape);
      self
        .opacity
        .set_after(0.0, now, timings.close_delay_ms, timings.fade_out);
    }
  }

  fn running(&self, now: Instant) -> bool {
    self.opacity.running(now) || self.shape.running(now)
  }

  fn visible(&self, open: bool, now: Instant) -> bool {
    open || self.running(now)
  }
}

/// Ouverture et fermeture du panneau de la vue plein écran : transitions de `clip-path`, `transform` et `opacity` (`styles.css:8863-8882`).
/// Les deux sens sont des transitions ; `visibility` ne passe à `hidden` qu'à la fin de la fermeture, donc `visible` reste vrai tant
/// qu'une piste court.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TvkPanelMotion {
  /// Faux hors de l'application lancée (banc, tests) : le panneau est à son état ouvert ou fermé.
  pub enabled: bool,
  tracks: PanelTracks,
  /// `.tvk-panel-inner` : `transform` de 220 ms, 0 fermé et 1 ouvert.
  inner: Track,
}

impl TvkPanelMotion {
  const INNER: Timing = Timing {
    duration_ms: tvk_tokens::INNER_MS,
    curve: tvk_tokens::INNER_CURVE,
    gate: Gate::NONE,
  };
  const PANEL: PanelTimings = PanelTimings {
    shape: Timing {
      duration_ms: tvk_tokens::PANEL_SHAPE_MS,
      curve: tvk_tokens::PANEL_SHAPE_CURVE,
      gate: Gate::PERF,
    },
    fade_in: Timing {
      duration_ms: tvk_tokens::PANEL_OPEN_FADE_MS,
      curve: tvk_tokens::PANEL_FADE_CURVE,
      gate: Gate::PERF,
    },
    fade_out: Timing {
      duration_ms: tvk_tokens::PANEL_CLOSE_FADE_MS,
      curve: tvk_tokens::PANEL_FADE_CURVE,
      gate: Gate::PERF,
    },
    close_delay_ms: tvk_tokens::PANEL_CLOSE_FADE_DELAY_MS,
  };

  #[must_use]
  pub fn launched() -> Self {
    Self {
      enabled: true,
      ..Self::default()
    }
  }

  /// `.open` posé (`openTvkPanel`) ou retiré (`closeTvkPanel`, `app.js:7483-7494`).
  pub fn set_open(&mut self, open: bool, now: Instant) {
    if !self.enabled {
      return;
    }
    self.set_inner(open, now);
    self.tracks.set(open, now, &Self::PANEL);
  }

  /// `.open` posé ou retiré sur `.tvk-panel-inner` : la transition de `transform` du contenu, indépendante des trois autres.
  pub fn set_inner(&mut self, open: bool, now: Instant) {
    if self.enabled {
      self.inner.set(f32::from(u8::from(open)), now, Self::INNER);
    }
  }

  /// `translateY` du contenu (`.tvk-panel-inner`), en pixels écran : `-6px` fermé, 0 ouvert. 0 hors de l'application lancée.
  #[must_use]
  pub fn inner_offset(&self, now: Instant) -> f32 {
    if self.enabled {
      -tvk_tokens::INNER_OFFSET * (1.0 - self.inner.value(now))
    } else {
      0.0
    }
  }

  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    self.enabled && (self.tracks.running(now) || self.inner.running(now))
  }

  /// Le panneau se dessine : ouvert, ou en train de se fermer.
  #[must_use]
  pub fn visible(&self, open: bool, now: Instant) -> bool {
    open || self.running(now)
  }

  /// `None` au repos : le panneau se dessine tel quel, sans coupe ni transformation.
  #[must_use]
  pub fn pose(&self, now: Instant) -> Option<TvkPanelPose> {
    if !self.running(now) {
      return None;
    }
    let shape = self.tracks.shape.value(now);
    let scale = tvk_tokens::PANEL_SCALE_FROM + (1.0 - tvk_tokens::PANEL_SCALE_FROM) * shape;
    Some(TvkPanelPose {
      opacity: self.tracks.opacity.value(now),
      offset_y: -tvk_tokens::PANEL_OFFSET * (1.0 - shape),
      scale,
      reveal: shape,
    })
  }
}

/// `opacity 180ms ease` du menu contextuel (`styles.css:10035`).
pub const MENU_FADE: Timing = Timing {
  duration_ms: menu_tokens::FADE_MS,
  curve: menu_tokens::FADE_CURVE,
  gate: Gate::NONE,
};
const SCRIM_FADE: Timing = Timing {
  duration_ms: settings_tokens::SCRIM_MS,
  curve: settings_tokens::FADE_CURVE,
  gate: Gate::NONE,
};
const CARD_FADE: Timing = Timing {
  duration_ms: settings_tokens::CARD_OPACITY_MS,
  curve: settings_tokens::FADE_CURVE,
  gate: Gate::NONE,
};
const CARD_POSE: Timing = Timing {
  duration_ms: settings_tokens::CARD_POSE_MS,
  curve: settings_tokens::FADE_CURVE,
  gate: Gate::NONE,
};

/// Pose de la modale des réglages : voile et carte (`#view-settings`, `.settings-view`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettingsPose {
  /// `opacity` du voile : elle s'applique aussi à la carte qu'il contient.
  pub scrim: f32,
  /// Pose de la carte : opacité du groupe (voile fois carte) et `scale(..) translateY(..)` autour de son centre.
  pub card: ViewPose,
}

/// Ouverture et fermeture du menu contextuel et de la modale des réglages (transitions CSS sur `opacity` et `transform`).
/// Le menu reste dans le DOM après `closeTrackContextMenu` et les réglages 200 ms après `closeSettingsModal` (`app.js:13707-13720`) :
/// les vues les dessinent tant que `menu_fading` et `settings_leaving` le disent, sans interaction (`pointer-events: none`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OverlayMotion {
  /// Faux hors de l'application lancée (banc, tests) : menu et réglages se posent à leur état ouvert.
  pub enabled: bool,
  menu: Option<Track>,
  scrim: Track,
  card_opacity: Track,
  card_pose: Track,
  settings_engaged: bool,
  settings_open: bool,
}

impl Default for Track {
  fn default() -> Self {
    Self::at(0.0)
  }
}

impl OverlayMotion {
  /// Transitions actives, sans menu ni modale posés.
  #[must_use]
  pub fn launched() -> Self {
    Self {
      enabled: true,
      ..Self::default()
    }
  }

  /// Le menu s'ouvre : `.open` revient (`app.js:10886`).
  pub fn open_menu(&mut self, now: Instant) {
    self
      .menu
      .get_or_insert_with(Track::default)
      .set(1.0, now, MENU_FADE);
  }

  /// `.open` est retiré : le menu s'efface sur place.
  pub fn close_menu(&mut self, now: Instant) {
    if let Some(track) = &mut self.menu {
      track.set(0.0, now, MENU_FADE);
    }
  }

  /// Opacité du menu : 1 au repos (aucune transition posée), sinon la valeur de la transition.
  #[must_use]
  pub fn menu_opacity(&self, now: Instant) -> f32 {
    match (&self.menu, self.enabled) {
      (Some(track), true) => track.value(now),
      _ => 1.0,
    }
  }

  /// Le menu fermé est encore visible : c'est la fin de son fondu.
  #[must_use]
  pub fn menu_fading(&self, now: Instant) -> bool {
    self.enabled && self.menu.is_some_and(|track| track.value(now) > 0.0)
  }

  /// Changement d'écran : les réglages s'ouvrent ou se ferment (`openSettingsModal`, `closeSettingsModal`).
  pub fn navigate(&mut self, from: Screen, to: Screen, now: Instant) {
    if !self.enabled || from == to {
      return;
    }
    if to == Screen::Settings {
      self.settings_engaged = true;
      self.settings_open = true;
    } else if from == Screen::Settings {
      self.settings_open = false;
    } else {
      return;
    }
    let target = if self.settings_open { 1.0 } else { 0.0 };
    self.scrim.set(target, now, SCRIM_FADE);
    self.card_opacity.set(target, now, CARD_FADE);
    self.card_pose.set(target, now, CARD_POSE);
  }

  /// `None` au repos : la modale se dessine ouverte, sans transformation ni fondu.
  #[must_use]
  pub fn settings_pose(&self, now: Instant) -> Option<SettingsPose> {
    if !self.enabled || !self.settings_engaged || (self.settings_open && !self.animating(now)) {
      return None;
    }
    let scrim = self.scrim.value(now);
    let card = self.card_opacity.value(now);
    let pose = self.card_pose.value(now);
    let scale = settings_tokens::CARD_SCALE_FROM + (1.0 - settings_tokens::CARD_SCALE_FROM) * pose;
    Some(SettingsPose {
      scrim,
      card: ViewPose {
        opacity: scrim * card,
        offset_y: settings_tokens::CARD_OFFSET * (1.0 - pose) * scale,
        scale,
      },
    })
  }

  /// La modale fermée se dessine encore : le voile n'a pas fini de s'effacer.
  #[must_use]
  pub fn settings_leaving(&self, now: Instant) -> bool {
    self.enabled && self.settings_engaged && !self.settings_open && self.scrim.value(now) > 0.0
  }

  #[must_use]
  pub fn animating(&self, now: Instant) -> bool {
    self.enabled
      && (self.menu.is_some_and(|track| track.running(now))
        || self.scrim.running(now)
        || self.card_opacity.running(now)
        || self.card_pose.running(now))
  }
}

/// Pose du panneau de file d'attente pendant sa transition (`.np-queue-panel`, `styles.css:10908-10941`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPose {
  pub opacity: f32,
  /// `translateX`, en pixels écran.
  pub offset_x: f32,
  pub scale: f32,
}

/// Transitions du panneau de file d'attente : `opacity` et `transform` ont chacune leur durée et leur courbe, dans les deux sens.
/// Tant que le panneau n'a jamais basculé (banc, tests), il est au repos selon `open`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PanelMotion {
  tracks: PanelTracks,
  touched: bool,
}

impl PanelMotion {
  const PANEL: PanelTimings = PanelTimings {
    shape: Timing {
      duration_ms: queue::SHAPE_MS,
      curve: queue::SHAPE_CURVE,
      gate: Gate::BOTH,
    },
    fade_in: Self::OPACITY,
    fade_out: Self::OPACITY,
    close_delay_ms: 0.0,
  };
  const OPACITY: Timing = Timing {
    duration_ms: queue::OPACITY_MS,
    curve: queue::OPACITY_CURVE,
    gate: Gate::BOTH,
  };

  /// `.open` posé ou retiré : `was_open` est l'état avant la bascule.
  pub fn set_open(&mut self, was_open: bool, open: bool, now: Instant) {
    if !self.touched {
      let rest = if was_open { 1.0 } else { 0.0 };
      self.tracks = PanelTracks::at(rest);
      self.touched = true;
    }
    self.tracks.set(open, now, &Self::PANEL);
  }

  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    self.tracks.running(now)
  }

  /// `visibility: hidden` ne vient qu'à la fin de la transition de fermeture (`visibility 0s linear 260ms`).
  #[must_use]
  pub fn visible(&self, open: bool, now: Instant) -> bool {
    self.tracks.visible(open, now)
  }

  /// `None` au repos ouvert : le panneau se dessine sans transformation ni opacité.
  #[must_use]
  pub fn pose(&self, now: Instant) -> Option<PanelPose> {
    if !self.touched {
      return None;
    }
    let opacity = self.tracks.opacity.value(now);
    let shape = self.tracks.shape.value(now);
    if !self.running(now) && opacity >= 1.0 {
      return None;
    }
    let pose = eased_pose(shape, queue::CLOSED_OFFSET_X, queue::CLOSED_SCALE);
    Some(PanelPose {
      opacity,
      offset_x: pose.offset_y,
      scale: pose.scale,
    })
  }
}

/// Pose de la carte audio connectée pendant `connectedExpand` ou `connectedCollapse` (`styles.css:7585-7592`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioPose {
  /// Part de la hauteur encore visible depuis le bas : le `clip-path: inset(top ...)` laisse `1 - top`.
  pub visible: f32,
  /// Fondu et décalage de l'en-tête et de `.audio-modal-layout` (`scale` vaut 1).
  pub content: ViewPose,
}

/// Animations CSS de la fenêtre audio connectée : départ de chacune dans l'état, progrès déduit de `App::now`. Sans départ posé
/// (banc, tests, application sans animation), tout est au repos.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AudioMotion {
  /// `connected-editor` posé : début de `connectedExpand`.
  opened: Option<Instant>,
  /// `connected-closing` posé : début de `connectedCollapse`. La carte est retirée 200 ms plus tard.
  closed: Option<Instant>,
  /// `.audio-panel` devenu visible : début d'`audioPanelIn`.
  panel: Option<Instant>,
  /// `#sleep-timer-status::before` apparu : début de `sleepDot`.
  dot: Option<Instant>,
}

impl AudioMotion {
  /// `toggleConnectedAudio` ouvre la carte : elle sort de la modale masquée et rejoue tout (`app.js:14983-14987`). Le panneau
  /// actif et le point passent de `display: none` à affichés au même instant.
  pub fn open(&mut self, now: Instant) {
    self.opened = Some(now);
    self.closed = None;
    self.panel = Some(now);
    self.dot = Some(now);
  }

  /// `closeConnected` pose `connected-closing` (`app.js:14961-14962`) : le dévoilement est remplacé, sans reprise du trajet parcouru.
  pub fn close(&mut self, now: Instant) {
    self.closed = Some(now);
  }

  /// Un autre `.audio-panel` devient `.active` (`app.js:14785-14790`) ; le point recommence si c'est celui du minuteur.
  pub fn switch_panel(&mut self, now: Instant, sleep: bool) {
    self.panel = Some(now);
    if sleep {
      self.dot = Some(now);
    }
  }

  /// `.active` arrive sur `#sleep-timer-status` (`updateSleepTimerUI`, `app.js:15335-15341`).
  pub fn start_dot(&mut self, now: Instant) {
    self.dot = Some(now);
  }

  /// La carte fermée est encore dessinée : `connectedCollapse` n'a pas fini (la minuterie de `closeConnected` : 200 ms).
  #[must_use]
  pub fn closing(&self, now: Instant) -> bool {
    self
      .closed
      .is_some_and(|start| elapsed_ms(Gate::REDUCED, start, now) < audio::COLLAPSE_MS)
  }

  /// Une transition de carte ou de panneau n'est pas finie. Le point, infini, se demande par [`Self::dot_started`].
  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    if self.closed.is_some() {
      return self.closing(now);
    }
    self
      .opened
      .is_some_and(|start| elapsed_ms(Gate::REDUCED, start, now) < audio::EXPAND_MS)
      || self
        .panel
        .is_some_and(|start| elapsed_ms(Gate::REDUCED, start, now) < audio::PANEL_MS)
  }

  #[must_use]
  pub fn dot_started(&self) -> bool {
    self.dot.is_some()
  }

  /// `None` au repos : la carte se dessine entière, sans coupe ni fondu.
  #[must_use]
  pub fn card_pose(&self, now: Instant) -> Option<AudioPose> {
    if let Some(start) = self.closed {
      let ms = elapsed_ms(Gate::REDUCED, start, now);
      if ms >= audio::COLLAPSE_MS {
        return None;
      }
      let collapse = bezier_ease(audio::COLLAPSE_CURVE, (ms / audio::COLLAPSE_MS).min(1.0));
      let out = bezier_ease(
        audio::CONTENT_OUT_CURVE,
        (ms / audio::CONTENT_OUT_MS).min(1.0),
      );
      return Some(AudioPose {
        visible: 1.0 - collapse,
        content: ViewPose {
          opacity: 1.0 - out,
          offset_y: audio::CONTENT_OUT_OFFSET * out,
          scale: 1.0,
        },
      });
    }
    let start = self.opened?;
    let progress = elapsed_ms(Gate::REDUCED, start, now) / audio::EXPAND_MS;
    if progress >= 1.0 {
      return None;
    }
    // `0%, 30%` tient l'image de départ ; la courbe court ensuite de 30 % à 100 %.
    let content = if progress <= audio::CONTENT_HOLD {
      0.0
    } else {
      bezier_ease(
        audio::CONTENT_CURVE,
        (progress - audio::CONTENT_HOLD) / (1.0 - audio::CONTENT_HOLD),
      )
    };
    Some(AudioPose {
      visible: bezier_ease(audio::EXPAND_CURVE, progress),
      content: ViewPose {
        opacity: content,
        offset_y: audio::CONTENT_OFFSET * (1.0 - content),
        scale: 1.0,
      },
    })
  }

  /// `audioPanelIn` : de `opacity: 0; translateY(6px)` à l'identité. `None` au repos.
  #[must_use]
  pub fn panel_pose(&self, now: Instant) -> Option<ViewPose> {
    let progress = elapsed_ms(Gate::REDUCED, self.panel?, now) / audio::PANEL_MS;
    if progress >= 1.0 {
      return None;
    }
    let eased = bezier_ease(audio::PANEL_CURVE, progress);
    Some(ViewPose {
      opacity: eased,
      offset_y: audio::PANEL_OFFSET * (1.0 - eased),
      scale: 1.0,
    })
  }

  /// `sleepDot` : opacité et échelle du point. Sans départ posé : l'état sans animation (`animation: none`, opacité 1, échelle 1).
  #[must_use]
  pub fn dot_pose(&self, now: Instant) -> DotPose {
    // `sleepDot` est `infinite` : la réduction la laisse finir en une itération (sans remplissage, retour au style de base) et
    // `body.perf` la retire (`styles.css:9906`). Dans les deux cas : l'état sans animation.
    let Some(start) = self
      .dot
      .filter(|_| !(Gate::BOTH.instant() || system_prefers_reduced()))
    else {
      return DotPose {
        opacity: 1.0,
        scale: 1.0,
      };
    };
    let phase = elapsed_ms(Gate::BOTH, start, now) % audio::DOT_PERIOD_MS;
    let half = audio::DOT_PERIOD_MS / 2.0;
    // `0%` vers `50%` monte, `50%` vers `100%` redescend ; `ease-in-out` sur chaque segment.
    let mix = if phase < half {
      bezier_ease(audio::DOT_CURVE, phase / half)
    } else {
      1.0 - bezier_ease(audio::DOT_CURVE, (phase - half) / half)
    };
    DotPose {
      opacity: audio::DOT_MIN_OPACITY + (1.0 - audio::DOT_MIN_OPACITY) * mix,
      scale: audio::DOT_MIN_SCALE + (1.0 - audio::DOT_MIN_SCALE) * mix,
    }
  }
}

/// Point du minuteur de sommeil.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DotPose {
  pub opacity: f32,
  pub scale: f32,
}

/// Espace où Chromium interpole une couleur en `transition` : les couleurs héritées (`rgb()`, hexadécimal) en sRGB, les autres
/// (`color(srgb ...)`) en Oklab (CSS Color 4, section 12.1). `13-theme-titre` : le fond de `body` passe par `oklab(...)`, le bouton
/// lecture par `rgb(...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
  Srgb,
  Oklab,
}

fn to_linear(value: f32) -> f32 {
  if value <= 0.040_45 {
    value / 12.92
  } else {
    ((value + 0.055) / 1.055).powf(2.4)
  }
}

fn from_linear(value: f32) -> f32 {
  if value <= 0.003_130_8 {
    value * 12.92
  } else {
    1.055 * value.powf(1.0 / 2.4) - 0.055
  }
}

/// sRGB vers Oklab (Björn Ottosson).
fn to_oklab(color: Color) -> [f32; 3] {
  let (r, g, b) = (to_linear(color.r), to_linear(color.g), to_linear(color.b));
  let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
  let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
  let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
  [
    0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
    1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
    0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
  ]
}

fn from_oklab([lightness, a, b]: [f32; 3], alpha: f32) -> Color {
  let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
  let m = (lightness - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
  let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
  let channel = |linear: f32| from_linear(linear).clamp(0.0, 1.0);
  Color {
    r: channel(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_93 * s),
    g: channel(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s),
    b: channel(-0.004_196_086 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    a: alpha,
  }
}

/// Couleur à la fraction `progress` du chemin de `from` vers `to`.
#[must_use]
pub fn mix_color(from: Color, to: Color, progress: f32, space: ColorSpace) -> Color {
  if progress <= 0.0 {
    return from;
  }
  if progress >= 1.0 {
    return to;
  }
  let lerp = |a: f32, b: f32| a + (b - a) * progress;
  match space {
    ColorSpace::Srgb => Color {
      r: lerp(from.r, to.r),
      g: lerp(from.g, to.g),
      b: lerp(from.b, to.b),
      a: lerp(from.a, to.a),
    },
    ColorSpace::Oklab => {
      let (a, b) = (to_oklab(from), to_oklab(to));
      from_oklab(
        [lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2])],
        lerp(from.a, to.a),
      )
    }
  }
}

/// Couleur animée par `transition` : même sémantique que `Track` (départ depuis la valeur affichée, durée raccourcie à l'inversion),
/// interpolée dans `space`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorFade {
  from: Color,
  target: Color,
  space: ColorSpace,
  run: Option<Run>,
}

impl ColorFade {
  #[must_use]
  pub const fn at(color: Color, space: ColorSpace) -> Self {
    Self {
      from: color,
      target: color,
      space,
      run: None,
    }
  }

  #[must_use]
  pub fn target(&self) -> Color {
    self.target
  }

  #[must_use]
  pub fn value(&self, now: Instant) -> Color {
    match self.run {
      Some(run) if run.progress(now) < 1.0 => {
        mix_color(self.from, self.target, run.value(now), self.space)
      }
      _ => self.target,
    }
  }

  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    self.run.is_some_and(|run| run.progress(now) < 1.0)
  }

  /// Vise `target` à `now` ; sans effet si la cible est déjà celle-là.
  pub fn set(&mut self, target: Color, now: Instant, timing: Timing) {
    if self.target == target {
      return;
    }
    // La piste interne va de 0 à 1 : revenir à la couleur de départ, c'est revenir à 0.
    let duration_ms = match self.run {
      Some(run) if self.from == target => {
        run.reversed_ms(timing.duration_ms, run.value(now).max(0.0), 0.0)
      }
      _ => timing.duration_ms,
    };
    self.from = self.value(now);
    self.target = target;
    self.run = Some(Run {
      from: 0.0,
      to: 1.0,
      start: now,
      duration_ms,
      curve: timing.curve,
      gate: timing.gate,
    });
  }
}

/// Couleurs du thème que Riff interpole quand `--md-sys-color-*` change d'un coup (les propriétés enregistrées par `@property`
/// ne portent aucune transition, `styles.css:3-7`) : le fond de `html`/`body` et le fond du bouton lecture. Tout le reste change
/// d'un coup dans le banc, faute de mesure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeFade {
  pub body: ColorFade,
  pub play: ColorFade,
}

/// `html`, `body` : `background-color var(--transition-emphasized)` (`styles.css:112-122`).
pub const THEME_BODY: Timing = Timing {
  duration_ms: tokens::THEME_BODY_MS,
  curve: tokens::THEME_BODY_CURVE,
  gate: Gate::REDUCED,
};
/// `.btn-play-pause` : `background-color 150ms ease` (`styles.css:3059`).
pub const THEME_PLAY: Timing = Timing {
  duration_ms: tokens::THEME_PLAY_MS,
  curve: tokens::THEME_PLAY_CURVE,
  gate: Gate::NONE,
};

impl ThemeFade {
  /// État au repos sur ces couleurs.
  #[must_use]
  pub const fn at(body: Color, play: Color) -> Self {
    Self {
      body: ColorFade::at(body, ColorSpace::Oklab),
      play: ColorFade::at(play, ColorSpace::Srgb),
    }
  }

  /// Vrai si une des cibles diffère de celles posées.
  #[must_use]
  pub fn differs(&self, body: Color, play: Color) -> bool {
    self.body.target() != body || self.play.target() != play
  }

  pub fn set(&mut self, body: Color, play: Color, now: Instant) {
    self.body.set(body, now, THEME_BODY);
    self.play.set(play, now, THEME_PLAY);
  }

  #[must_use]
  pub fn running(&self, now: Instant) -> bool {
    self.body.running(now) || self.play.running(now)
  }
}

/// Entrées `m3FadeUp` (`styles.css:1016-1019`) : un nœud inséré dans le DOM rejoue l'animation, donc l'instant de départ est celui de
/// l'évènement qui recrée la liste, la carte ou les choix. `backwards` et `both` : avant la fin du décalage le nœud est à l'image de
/// départ (opacité 0), après la dernière image il est au repos (`pose` vaut `None`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EntryMotion {
  /// Faux hors de l'application lancée (banc, tests) : les évènements ne démarrent rien, les scènes posent les départs elles-mêmes.
  pub enabled: bool,
  /// Lignes de titres (`renderTracks`, `createTrackRow`).
  pub rows: Option<Instant>,
  /// Voile choisi au départ du fondu (`None` : état courant, scènes du banc).
  pub plain: Option<bool>,
  /// Lignes ajoutées par « charger plus » (`app.js:10420`) : départ propre et rang de la première nouvelle ligne.
  pub more: Option<(Instant, usize)>,
  /// Lot précédent de « charger plus » encore en fondu quand le suivant arrive : ses lignes gardent leur départ.
  pub earlier: Option<(Instant, usize)>,
  /// Carte d'artiste des résultats (`fetchArtistCardForSearch`).
  pub artist: Option<Instant>,
  /// Choix de playlist (`app.js:13543`).
  pub picks: Option<Instant>,
  /// Rangées de la bibliothèque latérale recréées par un clic sur un onglet (`renderSidebarLibraryFeed(true)`, `app.js:27577`).
  pub sidebar: Option<Instant>,
  /// Section des réglages dont `cat-hidden` vient d'être retiré (`showSettingsCategory`, `app.js:17555`) : `settingsIn`.
  pub section: Option<Instant>,
  /// Boîte d'erreur de la recherche créée par `stopM3SearchLoaderWithError` (`app.js:9982`) : `m3ErrFadeIn`.
  pub error: Option<Instant>,
}

impl EntryMotion {
  pub fn start_rows(&mut self, now: Instant) {
    if self.enabled {
      self.rows = Some(now);
      self.more = None;
      self.earlier = None;
    }
  }

  /// Les `count` premières lignes existent déjà : seules les suivantes sont de nouveaux nœuds.
  pub fn start_more(&mut self, now: Instant, first: usize) {
    if self.enabled {
      let running = |start: Instant| raw_elapsed_ms(start, now) < Self::rows_total_ms();
      self.earlier = self.more.filter(|&(start, _)| running(start));
      self.more = Some((now, first));
    }
  }

  pub fn start_artist(&mut self, now: Instant) {
    if self.enabled {
      self.artist = Some(now);
    }
  }

  pub fn start_picks(&mut self, now: Instant) {
    if self.enabled {
      self.picks = Some(now);
    }
  }

  /// Clic sur un onglet de la bibliothèque latérale : toutes les rangées sont recréées avec `sbRowFadeIn`.
  pub fn start_sidebar(&mut self, now: Instant) {
    if self.enabled {
      self.sidebar = Some(now);
    }
  }

  /// `animation-delay: calc(min(var(--i, 0), 12) * 20ms)` de `.sb-lib-row` (`styles.css:11596`).
  fn sidebar_delay_ms(rank: usize) -> f32 {
    rank.min(sidebar_tokens::ENTER_MAX_RANK) as f32 * sidebar_tokens::ENTER_STEP_MS
  }

  /// Pose de la rangée latérale de rang `rank` (`--i`) : `opacity: 0; translateY(8px) scale(0.985)` vers le repos, `both` : avant le décalage
  /// elle est à l'image de départ, après la fin `None`. Une seule courbe pour les deux images clés.
  #[must_use]
  pub fn sidebar_pose(&self, rank: usize, now: Instant) -> Option<ViewPose> {
    let eased = Run::fade_up(
      Gate::BOTH,
      self.sidebar,
      Self::sidebar_delay_ms(rank),
      sidebar_tokens::ENTER_MS,
      sidebar_tokens::ENTER_CURVE,
      now,
    )?;
    Some(ViewPose {
      opacity: eased,
      offset_y: sidebar_tokens::ENTER_OFFSET * (1.0 - eased),
      scale: sidebar_tokens::ENTER_SCALE_FROM + (1.0 - sidebar_tokens::ENTER_SCALE_FROM) * eased,
    })
  }

  pub fn start_section(&mut self, now: Instant) {
    if self.enabled {
      self.section = Some(now);
    }
  }

  pub fn start_error(&mut self, now: Instant) {
    if self.enabled {
      self.error = Some(now);
    }
  }

  /// Fin de la dernière ligne décalée : `7 * 30 + 350` ms.
  fn rows_total_ms() -> f32 {
    Self::row_delay_ms(fade::ROW_STAGGERED - 1) + effective_ms(Gate::REDUCED, fade::ROW_MS)
  }

  /// `animation-delay` de `.track-row:nth-child(rang + 1)`.
  fn row_delay_ms(rank: usize) -> f32 {
    if rank < fade::ROW_STAGGERED {
      rank as f32 * fade::ROW_STEP_MS
    } else {
      0.0
    }
  }

  /// Pose de la ligne de rang `rank` (0 pour la première) : opacité seule, la translation est annulée par `!important`.
  #[must_use]
  pub fn row_pose(&self, rank: usize, now: Instant) -> Option<ViewPose> {
    let start = match (self.more, self.earlier) {
      (Some((start, first)), _) | (_, Some((start, first))) if rank >= first => Some(start),
      _ => self.rows,
    };
    let eased = Run::fade_up(
      Gate::REDUCED,
      start,
      Self::row_delay_ms(rank),
      fade::ROW_MS,
      fade::CURVE,
      now,
    )?;
    Some(eased_pose(eased, fade::ROW_OFFSET, 1.0))
  }

  #[must_use]
  pub fn artist_pose(&self, now: Instant) -> Option<ViewPose> {
    let eased = Run::fade_up(
      Gate::BOTH,
      self.artist,
      0.0,
      fade::ARTIST_MS,
      fade::CURVE,
      now,
    )?;
    Some(eased_pose(eased, fade::OFFSET, 1.0))
  }

  /// Pose de la section des réglages : `settingsIn`, `ease` sur l'unique segment `from` vers la valeur courante.
  #[must_use]
  pub fn section_pose(&self, now: Instant) -> Option<ViewPose> {
    let eased = Run::fade_up(
      Gate::REDUCED,
      self.section,
      0.0,
      settings_tokens::SECTION_MS,
      settings_tokens::SECTION_CURVE,
      now,
    )?;
    Some(eased_pose(eased, settings_tokens::SECTION_OFFSET, 1.0))
  }

  /// Pose de la boîte d'erreur : invisible pendant les 220 ms de délai (style de base), puis `m3ErrFadeIn` ; `forwards` la laisse au repos.
  #[must_use]
  pub fn error_pose(&self, now: Instant) -> Option<ViewPose> {
    let eased = Run::fade_up(
      Gate::REDUCED,
      self.error,
      search_tokens::ERROR_FADE_DELAY_MS,
      search_tokens::ERROR_FADE_MS,
      search_tokens::ERROR_FADE_CURVE,
      now,
    )?;
    Some(eased_pose(eased, search_tokens::ERROR_FADE_OFFSET, 1.0))
  }

  /// Icône de l'erreur (`.m3-search-error-icon`) : `opacity` et `transform` sont des transitions de 320 ms lancées après 180 ms quand
  /// `.is-error` est posé, au même instant que la boîte. `opacity` est la part de l'opacité du repos (0,85), `scale` l'échelle ; avant
  /// le délai l'icône est à son style de base (invisible, `scale(0.75)`), après 500 ms `None` : au repos.
  #[must_use]
  pub fn error_icon_pose(&self, now: Instant) -> Option<ViewPose> {
    let at = |curve| {
      Run::fade_up(
        Gate::REDUCED,
        self.error,
        search_tokens::ERROR_ICON_DELAY_MS,
        search_tokens::ERROR_ICON_MS,
        curve,
        now,
      )
    };
    let opacity = at(search_tokens::ERROR_ICON_FADE_CURVE)?;
    let scale = at(search_tokens::ERROR_ICON_SCALE_CURVE)?;
    Some(ViewPose {
      opacity,
      offset_y: 0.0,
      scale: search_tokens::ERROR_ICON_SCALE_FROM
        + (1.0 - search_tokens::ERROR_ICON_SCALE_FROM) * scale,
    })
  }

  /// Pose du choix de rang `rank` : décalage `rang * 25 ms`.
  #[must_use]
  pub fn pick_pose(&self, rank: usize, now: Instant) -> Option<ViewPose> {
    let delay = rank as f32 * fade::PICK_STEP_MS;
    let eased = Run::fade_up(
      Gate::REDUCED,
      self.picks,
      delay,
      fade::PICK_MS,
      fade::CURVE,
      now,
    )?;
    Some(eased_pose(eased, fade::OFFSET, 1.0))
  }

  /// Une entrée est en cours. Les lignes au-delà de la huitième n'ont pas de décalage : la dernière finit à `7 * 30 + 350` ms ;
  /// `pick_count` choix de playlist finissent à `(n - 1) * 25 + 200` ms.
  #[must_use]
  pub fn running(&self, now: Instant, pick_count: usize) -> bool {
    // Les décalages (`animation-delay`) restent sous la réduction et sous `body.perf` : seule la durée devient nulle.
    let within = |start: Option<Instant>, total_ms: f32| {
      start.is_some_and(|start| raw_elapsed_ms(start, now) < total_ms)
    };
    within(self.rows, Self::rows_total_ms())
      || within(self.more.map(|(start, _)| start), Self::rows_total_ms())
      || within(self.artist, effective_ms(Gate::BOTH, fade::ARTIST_MS))
      || within(
        self.section,
        effective_ms(Gate::REDUCED, settings_tokens::SECTION_MS),
      )
      || within(
        self.error,
        search_tokens::ERROR_FADE_DELAY_MS
          + effective_ms(Gate::REDUCED, search_tokens::ERROR_FADE_MS),
      )
      || within(
        self.sidebar,
        Self::sidebar_delay_ms(sidebar_tokens::ENTER_MAX_RANK)
          + effective_ms(Gate::BOTH, sidebar_tokens::ENTER_MS),
      )
      || within(
        self.picks,
        pick_count.saturating_sub(1) as f32 * fade::PICK_STEP_MS
          + effective_ms(Gate::REDUCED, fade::PICK_MS),
      )
  }
}

/// Horloge d'un groupe de `.m3-circular-progress` de l'accueil : une animation CSS repart quand son nœud est créé ou que la vue sort
/// de `display: none`. Le départ est l'instant où le groupe devient visible (effacé quand le dernier disparaît) ou où son rendu le
/// recrée (`recreate`) ; les spinners d'un même rendu partagent ce départ. `App` en tient deux : les cartes réservées
/// (`renderMadeForYou`, `app.js:25391`) et les tendances (`renderTrending`, `app.js:25149`), recréées par des évènements différents.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpinnerClock {
  /// Faux hors de l'application lancée (banc, tests) : les évènements ne démarrent rien, les scènes posent le départ elles-mêmes.
  pub enabled: bool,
  pub start: Option<Instant>,
  /// Un rendu de Riff vient de recréer les nœuds : le prochain `track` repart de 0 (l'instant est daté par `App`, pas dans l'évènement).
  pub recreated: bool,
}

impl SpinnerClock {
  /// Pose ou efface le départ selon qu'un spinner est à l'écran ; des nœuds recréés depuis le dernier appel repartent de `now`.
  pub fn track(&mut self, shown: bool, now: Instant) {
    let recreated = std::mem::take(&mut self.recreated);
    self.start = shown.then(|| {
      if recreated {
        now
      } else {
        self.start.unwrap_or(now)
      }
    });
  }

  /// Le rendu de Riff qui insère ces spinners vient de passer (sans effet hors de l'application lancée).
  pub const fn recreate(&mut self) {
    if self.enabled {
      self.recreated = true;
    }
  }

  /// Temps écoulé depuis le départ ; sans départ, 0 (première image).
  #[must_use]
  pub fn elapsed(&self, now: Instant) -> Duration {
    self
      .start
      .map_or(Duration::ZERO, |start| now.saturating_duration_since(start))
  }

  /// Un spinner est à l'écran (le départ est posé).
  #[must_use]
  pub const fn running(&self) -> bool {
    self.start.is_some()
  }

  /// Un spinner change d'image : l'abonnement aux images doit rester actif (`infinite`). `m3Rotate` et `m3Dash` ne sont pas
  /// `!important` : sous la réduction (`iteration-count: 1`, 0,001 ms) elles finissent et le spinner revient à son style de base.
  #[must_use]
  pub fn animating(&self) -> bool {
    self.running() && !Gate::REDUCED.instant()
  }
}

/// Hauteur d'une barre d'égaliseur (fraction de `.sp-playing-bars`) après `elapsed_ms` de temps écoulé hors pause : `spBarEq`
/// (`styles.css:1711-1721`). Avant la fin du délai la barre garde `height: 100%` ; ensuite chaque itération va de 20 % à 100 %, les
/// itérations impaires en sens inverse (`alternate`, le progrès dirigé vaut `1 - p`), avec `ease-in-out` sur le progrès dirigé.
#[must_use]
pub fn bar_height(bar: usize, elapsed_ms: f32) -> f32 {
  let (delay, duration) = (
    row_tokens::BAR_DELAY_MS[bar],
    row_tokens::BAR_DURATION_MS[bar],
  );
  if elapsed_ms < delay {
    return row_tokens::BAR_HIGH;
  }
  let iterations = (elapsed_ms - delay) / duration;
  let whole = iterations.floor();
  let progress = iterations - whole;
  let directed = if whole % 2.0 < 1.0 {
    progress
  } else {
    1.0 - progress
  };
  row_tokens::BAR_LOW
    + (row_tokens::BAR_HIGH - row_tokens::BAR_LOW) * bezier_ease(row_tokens::BAR_CURVE, directed)
}

/// Barres d'égaliseur de la ligne en cours (`.sp-playing-bars .bar`). Les animations repartent de zéro quand `.playing` pose la ligne
/// (`highlightPlayingRow`, `app.js:11508`, à chaque titre lancé et à chaque liste recréée) ; `body.is-playing` retiré (`app.js:8699`)
/// ou `body.app-paused` les met en pause à 35 % (`styles.css:1724-1727`) et elles reprennent là où elles s'étaient arrêtées. Le temps de
/// lecture cumulé se déduit des intervalles de lecture, donc de l'horloge de `App` seule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BarsMotion {
  /// Faux hors de l'application lancée (banc, tests) : `App` ne date rien, les scènes posent les intervalles elles-mêmes.
  pub enabled: bool,
  /// Jeton du titre lancé (`PlayerState::token`) au dernier départ des barres.
  token: u64,
  /// Temps de lecture déjà cumulé par les intervalles retirés de `runs`.
  base_ms: f32,
  /// Intervalles de lecture depuis le départ : début et fin (`None` : en cours).
  runs: Vec<(Instant, Option<Instant>)>,
}

/// Intervalles gardés avant de les replier dans `base_ms`.
const MAX_RUNS: usize = 16;

impl BarsMotion {
  /// Les animations repartent de zéro, en lecture ou en pause selon `running`.
  pub fn restart(&mut self, now: Instant, running: bool) {
    self.base_ms = 0.0;
    self.runs.clear();
    if running {
      self.runs.push((now, None));
    }
  }

  fn open(&self) -> bool {
    self.runs.last().is_some_and(|(_, end)| end.is_none())
  }

  /// `body.is-playing` ajouté ou retiré à `now`.
  pub fn set_running(&mut self, running: bool, now: Instant) {
    match (running, self.open()) {
      (true, false) => {
        if self.runs.len() >= MAX_RUNS {
          let (start, end) = self.runs.remove(0);
          self.base_ms += end.map_or(0.0, |end| span_ms(start, end));
        }
        self.runs.push((now, None));
      }
      (false, true) => {
        if let Some(last) = self.runs.last_mut() {
          last.1 = Some(now);
        }
      }
      _ => {}
    }
  }

  /// Suit le titre lancé (`token`) et l'état de lecture ; vrai si les barres ont changé à `now`.
  pub fn track(&mut self, token: u64, running: bool, now: Instant) -> bool {
    if token != self.token {
      self.token = token;
      self.restart(now, running);
      return true;
    }
    if self.open() == running {
      return false;
    }
    self.set_running(running, now);
    true
  }

  /// Lecture à `now` d'après les intervalles ; `None` tant qu'aucun n'a été posé (alors `live` décide, au temps zéro).
  fn running_at(&self, now: Instant) -> Option<bool> {
    if self.runs.is_empty() {
      return None;
    }
    Some(
      self
        .runs
        .iter()
        .any(|&(start, end)| start <= now && end.is_none_or(|end| now < end)),
    )
  }

  /// Les barres bougent à `now` : `live` vaut l'état de lecture de l'application quand aucun intervalle n'est posé.
  #[must_use]
  pub fn running(&self, now: Instant, live: bool) -> bool {
    self.running_at(now).unwrap_or(live)
  }

  /// Temps de lecture écoulé depuis le départ des animations.
  #[must_use]
  pub fn elapsed_ms(&self, now: Instant) -> f32 {
    self.base_ms
      + self
        .runs
        .iter()
        .map(|&(start, end)| span_ms(start, end.map_or(now, |end| end.min(now))))
        .sum::<f32>()
  }

  /// Hauteurs des quatre barres à `now` : 35 % en pause (`height: 35% !important`).
  #[must_use]
  pub fn heights(&self, now: Instant, live: bool) -> [f32; 4] {
    // `body.perf` les fige à 35 % (`styles.css:1725`). Sous la réduction, `spBarEq` s'achève en une itération sans remplissage : retour
    // au `height: 100%` de base, sauf en pause (`height: 35% !important`).
    if perf() || !self.running(now, live) {
      return [row_tokens::BAR_REST; 4];
    }
    if reduced() {
      return [row_tokens::BAR_HIGH; 4];
    }
    let elapsed = self.elapsed_ms(now);
    std::array::from_fn(|bar| bar_height(bar, elapsed))
  }
}

/// Millisecondes de `start` à `end`, 0 si `end` précède `start`.
fn span_ms(start: Instant, end: Instant) -> f32 {
  end.saturating_duration_since(start).as_secs_f32() * 1000.0
}

/// Mouvements de la page artiste sur l'horloge de `App` : tour du badge « vérifié » (`cookieBadgeRotate`), forme du chargeur
/// (`renderM3Loader`) et entrée d'un onglet (`artistPaneSlideIn`). Chaque départ est celui du changement de `display` qui rend
/// l'élément visible : une animation CSS repart quand son nœud ou un de ses ancêtres sort de `display: none`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ArtistMotion {
  /// Faux hors de l'application lancée (banc, tests) : les évènements ne démarrent rien, les scènes posent les départs elles-mêmes.
  pub enabled: bool,
  /// `.artist-m3-cookie-badge` : `hidden` retiré (`app.js:12060`) ou page réaffichée. Sans départ, angle 0.
  pub badge: Option<Instant>,
  /// `renderM3Loader(el.artistTracksList)` (`app.js:11993`) : `stepStart` et référence de la rotation. Sans départ, première forme, angle 0.
  pub loader: Option<Instant>,
  /// Panneau affiché : page ouverte ou clic sur un onglet (`app.js:11902-11918`).
  pub pane: Option<Instant>,
}

/// Angle du badge en tours, `linear infinite` sur 14 s : `rotate(0)` vers `rotate(360deg)`.
#[must_use]
pub fn badge_turns(start: Option<Instant>, now: Instant) -> f32 {
  // Réduction : l'itération unique s'achève sans remplissage, retour à `transform: none` ; `body.perf` : `animation: none`.
  let Some(start) = start.filter(|_| !Gate::BOTH.instant()) else {
    return 0.0;
  };
  let elapsed = now.saturating_duration_since(start).as_secs_f32();
  (elapsed % artist_tokens::BADGE_TURN_SECS) / artist_tokens::BADGE_TURN_SECS
}

/// Forme et angle du chargeur : rang de la forme (`shapeIdx`, une de plus toutes les 1,2 s) et tour en 4 s, en tours.
/// Riff lit `now % 4000` sur l'horloge de la page : la phase absolue n'est pas reproductible, la rotation part de 0 au départ.
#[must_use]
pub fn loader_state(start: Option<Instant>, now: Instant) -> (usize, f32) {
  let Some(start) = start else { return (0, 0.0) };
  let elapsed = now.saturating_duration_since(start);
  // Millisecondes entières : 3600 / 1200 vaut exactement 3, ce que `3,6 / 1,2` en flottant ne garantit pas.
  let step_ms = (artist_tokens::LOADER_STEP_SECS * 1000.0) as u128;
  (
    (elapsed.as_millis() / step_ms) as usize,
    (elapsed.as_secs_f32() % artist_tokens::LOADER_TURN_SECS) / artist_tokens::LOADER_TURN_SECS,
  )
}

impl ArtistMotion {
  pub fn start_badge(&mut self, now: Instant) {
    if self.enabled {
      self.badge = Some(now);
    }
  }

  pub fn start_loader(&mut self, now: Instant) {
    if self.enabled {
      self.loader = Some(now);
    }
  }

  pub fn start_pane(&mut self, now: Instant) {
    if self.enabled {
      self.pane = Some(now);
    }
  }

  /// Pose du panneau : opacité, `translateY(10px)` et `scale(0.99)` vers l'identité ; `None` après les 260 ms (état stable).
  #[must_use]
  pub fn pane_pose(&self, now: Instant) -> Option<ViewPose> {
    let progress = elapsed_ms(Gate::BOTH, self.pane?, now) / artist_tokens::PANE_MS;
    (progress < 1.0).then(|| {
      let eased = bezier_ease(artist_tokens::PANE_CURVE, progress);
      ViewPose {
        opacity: eased,
        offset_y: artist_tokens::PANE_OFFSET * (1.0 - eased),
        scale: artist_tokens::PANE_SCALE + (1.0 - artist_tokens::PANE_SCALE) * eased,
      }
    })
  }

  /// L'entrée d'un onglet est en cours.
  #[must_use]
  pub fn pane_running(&self, now: Instant) -> bool {
    self.pane_pose(now).is_some()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  const EASE: Timing = Timing {
    duration_ms: tokens::HOVER_MS,
    curve: tokens::HOVER_CURVE,
    gate: Gate::BOTH,
  };

  fn ms(base: Instant, value: u64) -> Instant {
    base + Duration::from_millis(value)
  }

  #[test]
  fn view_pose_follows_the_computed_style_of_riff() {
    // `07-changement-vue` : à +75 ms du départ, opacité 0,467106 et matrix(0.978684, 0, 0, 0.978684, 0, 8.5263).
    let t0 = Instant::now();
    let motion = ViewMotion {
      enabled: true,
      start: Some(t0),
    };
    let pose = motion.pose(ms(t0, 75)).expect("en cours");
    assert!((pose.opacity - 0.467_106).abs() < 0.01, "{pose:?}");
    assert!((pose.scale - 0.978_684).abs() < 0.001, "{pose:?}");
    assert!(
      (pose.offset_y / tokens::VIEW_OFFSET * 16.0 - 8.5263).abs() < 0.2,
      "{pose:?}"
    );
    let start = motion.pose(t0).expect("départ");
    assert!((start.opacity, start.scale) == (0.0, 0.96));
    assert!((start.offset_y - tokens::VIEW_OFFSET).abs() < 1e-4);
    assert_eq!(motion.pose(ms(t0, 400)), None);
    assert!(!motion.running(ms(t0, 400)));
  }

  /// Remet les interrupteurs à zéro à la sortie d'un test (le fil de test peut être réutilisé).
  struct Switches;

  impl Switches {
    fn set(reduced: bool, perf: bool) -> Self {
      set_reduced(reduced);
      set_perf(perf);
      Self
    }
  }

  impl Drop for Switches {
    fn drop(&mut self) {
      set_reduced(false);
      set_perf(false);
      inject_system_preference(None);
    }
  }

  #[test]
  fn reduced_motion_ends_every_transition_at_once_but_keeps_its_delay() {
    let _switches = Switches::set(true, false);
    let t0 = Instant::now();
    let mut track = Track::at(0.0);
    track.set(1.0, t0, EASE);
    assert_eq!(
      track.value(t0),
      1.0,
      "durée de 0,001 ms : la fin dès le départ"
    );
    assert!(!track.running(t0));
    let mut delayed = Track::at(0.0);
    delayed.set_after(1.0, t0, 100.0, EASE);
    assert_eq!(delayed.value(ms(t0, 50)), 0.0, "`transition-delay` gardé");
    assert!(delayed.running(ms(t0, 50)));
    assert_eq!(delayed.value(ms(t0, 101)), 1.0);
    assert!(!delayed.running(ms(t0, 101)));
  }

  #[test]
  fn the_performance_mode_ends_transitions_too() {
    let _switches = Switches::set(false, true);
    let t0 = Instant::now();
    let mut track = Track::at(0.0);
    track.set(1.0, t0, EASE);
    assert_eq!(track.value(t0), 1.0);
    let mut hover = ControlHover::default();
    hover.set("play", true, t0);
    let mix = hover.mix("play", t0);
    assert_eq!((mix.color, mix.shape), (1.0, 1.0));
    assert!(!hover.animating(t0));
  }

  #[test]
  fn without_the_switches_a_transition_still_takes_its_time() {
    let _switches = Switches::set(false, false);
    let t0 = Instant::now();
    let mut track = Track::at(0.0);
    track.set(1.0, t0, EASE);
    assert!(track.value(ms(t0, 50)) < 1.0);
    assert!(track.running(ms(t0, 50)));
  }

  #[test]
  fn a_view_change_is_at_rest_from_the_first_instant_when_reduced() {
    let _switches = Switches::set(true, false);
    let t0 = Instant::now();
    let motion = ViewMotion {
      enabled: true,
      start: Some(t0),
    };
    for t in [0, 50, 400] {
      assert_eq!(motion.pose(ms(t0, t)), None, "t = {t} ms");
      assert!(!motion.running(ms(t0, t)));
    }
  }

  #[test]
  fn entries_keep_their_stagger_but_appear_at_once_when_reduced() {
    let _switches = Switches::set(true, false);
    let t0 = Instant::now();
    let mut entries = EntryMotion {
      enabled: true,
      ..EntryMotion::default()
    };
    entries.start_rows(t0);
    // `animation-delay` de 30 ms par rang : avant lui la ligne est à l'image de départ (`backwards`), puis la fin.
    let before = entries.row_pose(3, ms(t0, 60)).expect("avant le décalage");
    assert_eq!(before.opacity, 0.0);
    assert_eq!(entries.row_pose(3, ms(t0, 91)), None);
    assert_eq!(entries.row_pose(0, ms(t0, 1)), None);
    assert!(
      entries.running(ms(t0, 100), 0),
      "décalage de la 8e ligne : 210 ms"
    );
    assert!(!entries.running(ms(t0, 211), 0));
  }

  #[test]
  fn the_equalizer_bars_go_back_to_their_base_height_when_reduced_and_rest_in_performance_mode() {
    let t0 = Instant::now();
    let mut bars = BarsMotion::default();
    bars.restart(t0, true);
    let playing = ms(t0, 700);
    let _animated = Switches::set(false, false);
    assert_ne!(bars.heights(playing, true), [row_tokens::BAR_HIGH; 4]);
    set_reduced(true);
    assert_eq!(bars.heights(playing, true), [row_tokens::BAR_HIGH; 4]);
    bars.set_running(false, ms(t0, 800));
    assert_eq!(
      bars.heights(ms(t0, 900), true),
      [row_tokens::BAR_REST; 4],
      "en pause : `height: 35% !important`"
    );
    set_reduced(false);
    set_perf(true);
    let mut again = BarsMotion::default();
    again.restart(t0, true);
    assert_eq!(again.heights(playing, true), [row_tokens::BAR_REST; 4]);
  }

  #[test]
  fn the_ripple_ends_at_once_when_reduced_and_is_not_created_in_performance_mode() {
    let t0 = Instant::now();
    let (origin, size) = (Point::new(10.0, 10.0), Size::new(40.0, 40.0));
    let _switches = Switches::set(false, false);
    let mut hover = ControlHover::default();
    hover.ripple("a", origin, size, t0);
    assert_eq!(hover.ripples_of("a", ms(t0, 100)).len(), 1);
    assert!(hover.animating(ms(t0, 100)));
    set_reduced(true);
    assert!(
      hover.ripples_of("a", ms(t0, 100)).is_empty(),
      "`rippleOut` finie, opacité 0"
    );
    let mut reduced = ControlHover::default();
    reduced.ripple("a", origin, size, t0);
    assert!(!reduced.animating(ms(t0, 1)));
    set_reduced(false);
    set_perf(true);
    assert!(!ripples_enabled());
    let mut perf = ControlHover::default();
    perf.ripple("a", origin, size, t0);
    assert!(
      perf.ripples_of("a", ms(t0, 100)).is_empty(),
      "`.ripple-host` en `display: none`"
    );
  }

  #[test]
  fn hover_and_press_are_not_reduced_but_jump_in_performance_mode() {
    let t0 = Instant::now();
    let _switches = Switches::set(true, false);
    let mut hover = ControlHover::default();
    hover.set("b", true, t0);
    hover.press("b", t0);
    let mid = hover.mix("b", ms(t0, 90));
    assert!(
      mid.color > 0.0 && mid.color < 1.0,
      "`!important` : non réduit {mid:?}"
    );
    assert!(mid.press > 0.0 && mid.press < 1.0, "{mid:?}");
    set_reduced(false);
    set_perf(true);
    let mut hover = ControlHover::default();
    hover.set("b", true, t0);
    hover.press("b", t0);
    let after = hover.mix("b", ms(t0, 1));
    assert_eq!(
      (after.color, after.shape, after.press),
      (1.0, 1.0, 1.0),
      "`transition: none`"
    );
  }

  #[test]
  fn the_karaoke_panel_content_is_not_reduced_and_the_error_icon_is() {
    let t0 = Instant::now();
    let _switches = Switches::set(true, false);
    let mut panel = TvkPanelMotion::launched();
    panel.set_inner(true, t0);
    let mid = panel.inner_offset(ms(t0, 110));
    assert!(
      mid < 0.0 && mid > -tvk_tokens::INNER_OFFSET,
      "`!important` : {mid}"
    );
    let mut entries = EntryMotion {
      enabled: true,
      ..EntryMotion::default()
    };
    entries.start_error(t0);
    let held = entries
      .error_icon_pose(ms(t0, 100))
      .expect("délai de 180 ms");
    assert_eq!(held.opacity, 0.0);
    assert_eq!(
      entries.error_icon_pose(ms(t0, 181)),
      None,
      "état final tout de suite"
    );
    set_reduced(false);
    set_perf(true);
    assert!(
      entries.error_icon_pose(ms(t0, 181)).is_some(),
      "aucune règle perf"
    );
    let mut panel = TvkPanelMotion::launched();
    panel.set_inner(true, t0);
    assert!(
      panel.inner_offset(ms(t0, 110)) < 0.0,
      "perf ne vise que le panneau"
    );
  }

  #[test]
  fn the_home_spinner_stops_asking_for_frames_when_reduced_and_turns_in_performance_mode() {
    let t0 = Instant::now();
    let mut clock = SpinnerClock::default();
    clock.track(true, t0);
    let _switches = Switches::set(false, true);
    assert!(clock.animating());
    set_reduced(true);
    assert!(clock.running() && !clock.animating());
  }

  #[test]
  fn the_endless_loops_stop_on_their_base_state() {
    let t0 = Instant::now();
    let _switches = Switches::set(false, false);
    assert!(badge_turns(Some(t0), ms(t0, 3500)) > 0.2);
    set_reduced(true);
    assert_eq!(
      badge_turns(Some(t0), ms(t0, 3500)),
      0.0,
      "`transform: none`"
    );
    set_reduced(false);
    set_perf(true);
    assert_eq!(
      badge_turns(Some(t0), ms(t0, 3500)),
      0.0,
      "`animation: none`"
    );
    let mut audio = AudioMotion::default();
    audio.start_dot(t0);
    let moving = ms(t0, 500);
    set_perf(false);
    assert!(audio.dot_pose(moving).opacity < 1.0);
    set_reduced(true);
    let rest = audio.dot_pose(moving);
    assert_eq!((rest.opacity, rest.scale), (1.0, 1.0));
  }

  /// Une transition ne s'arrête que par la règle de Riff qui la vise : `!important` plus tard dans la feuille bat la réduction.
  #[test]
  fn each_transition_stops_only_under_the_rules_that_reach_it() {
    let t0 = Instant::now();
    let probe = |timing: Timing| {
      let mut track = Track::at(0.0);
      track.set(1.0, t0, timing);
      track.running(ms(t0, 1))
    };
    let all = [
      ("HOVER", HOVER, (true, false)),
      ("RADIUS", RADIUS, (true, false)),
      ("ROW_HOVER", ROW_HOVER, (true, false)),
      ("CARD", CARD, (false, true)),
      ("THEME_BODY", THEME_BODY, (false, true)),
      ("THEME_PLAY", THEME_PLAY, (true, true)),
      ("MENU_FADE", MENU_FADE, (true, true)),
      ("SCRIM_FADE", SCRIM_FADE, (true, true)),
      ("CARD_FADE", CARD_FADE, (true, true)),
      ("CARD_POSE", CARD_POSE, (true, true)),
      ("PANEL_OPACITY", PanelMotion::OPACITY, (false, false)),
      ("PANEL_SHAPE", PanelMotion::PANEL.shape, (false, false)),
      ("TVK_SHAPE", TvkPanelMotion::PANEL.shape, (true, false)),
      ("TVK_FADE_IN", TvkPanelMotion::PANEL.fade_in, (true, false)),
      (
        "TVK_FADE_OUT",
        TvkPanelMotion::PANEL.fade_out,
        (true, false),
      ),
    ];
    for (name, timing, (reduced_runs, perf_runs)) in all {
      for (reduced, perf) in [(true, false), (false, true)] {
        let _switches = Switches::set(reduced, perf);
        let expected = if reduced { reduced_runs } else { perf_runs };
        assert_eq!(
          probe(timing),
          expected,
          "{name}, réduit {reduced}, performance {perf}"
        );
      }
    }
  }

  #[test]
  fn the_system_preference_alone_freezes_the_mesh_and_the_sleep_dot() {
    let _switches = Switches::set(false, false);
    let t0 = Instant::now();
    let mut audio = AudioMotion::default();
    audio.start_dot(t0);
    assert!(!mesh_frozen());
    assert!(audio.dot_pose(ms(t0, 500)).opacity < 1.0);
    inject_system_preference(Some(true));
    assert!(
      mesh_frozen(),
      "`@media (prefers-reduced-motion)` (`styles.css:175`)"
    );
    assert_eq!(
      audio.dot_pose(ms(t0, 500)).opacity,
      1.0,
      "`styles.css:9899`"
    );
    assert!(!reduced(), "le réglage reste celui posé");
  }

  #[test]
  fn the_karaoke_orbits_stop_only_in_performance_mode() {
    let t0 = Instant::now();
    let mut tvk = TvkMotion::launched();
    tvk.open(t0);
    let _switches = Switches::set(true, false);
    assert!(
      (tvk.orbit_seconds(ms(t0, 2500)) - 2.5).abs() < 1e-4,
      "`!important` plus spécifique"
    );
    assert!(tvk.orbiting());
    set_reduced(false);
    set_perf(true);
    assert_eq!(
      tvk.orbit_seconds(ms(t0, 2500)),
      0.0,
      "`animation-play-state: paused`"
    );
    assert!(!tvk.orbiting());
  }

  #[test]
  fn the_karaoke_entry_is_instant_when_reduced_but_its_exit_is_not() {
    let t0 = Instant::now();
    let mut tvk = TvkMotion::launched();
    tvk.open(t0);
    let _switches = Switches::set(true, false);
    assert_eq!(tvk.pose(ms(t0, 100)), None, "`tvk-in` réduite");
    tvk.close(ms(t0, 1000));
    let leaving = tvk
      .pose(ms(t0, 1100))
      .expect("`tvk-out !important` continue");
    assert!(
      leaving.opacity > 0.0 && leaving.opacity < 1.0,
      "{leaving:?}"
    );
    assert!(tvk.leaving(ms(t0, 1100)));
  }

  #[test]
  fn the_audio_card_and_the_artist_pane_are_final_at_once_when_reduced() {
    let t0 = Instant::now();
    let _switches = Switches::set(true, false);
    let mut audio = AudioMotion::default();
    audio.open(t0);
    assert_eq!(audio.card_pose(t0), None);
    assert_eq!(audio.panel_pose(t0), None);
    assert!(!audio.running(t0));
    audio.close(ms(t0, 10));
    assert!(!audio.closing(ms(t0, 10)));
    let artist = ArtistMotion {
      enabled: true,
      pane: Some(t0),
      ..ArtistMotion::default()
    };
    assert_eq!(artist.pane_pose(t0), None);
    assert!(!artist.pane_running(t0));
  }

  #[test]
  fn the_injected_system_preference_decides_the_start_value() {
    let _switches = Switches::set(false, false);
    assert!(
      !system_prefers_reduced(),
      "les tests ne lisent pas le système"
    );
    inject_system_preference(Some(true));
    assert!(system_prefers_reduced());
    apply_settings(None, false);
    assert!(reduced(), "réglage absent : le système décide");
    apply_settings(Some(false), true);
    assert!(!reduced() && perf(), "réglage posé : il l'emporte");
    inject_system_preference(Some(false));
    apply_settings(Some(true), false);
    assert!(reduced() && !perf());
  }

  /// Pose attendue d'après `samples.json` de `12-karaoke` : `(opacité, échelle, translateY en px CSS)`.
  fn assert_tvk_pose(pose: ViewPose, (opacity, scale, y): (f32, f32, f32)) {
    assert!((pose.opacity - opacity).abs() < 0.005, "{pose:?}");
    assert!((pose.scale - scale).abs() < 0.0005, "{pose:?}");
    assert!(
      (pose.offset_y / crate::theme::tokens::ZOOM - y).abs() < 0.1,
      "{pose:?}"
    );
  }

  #[test]
  fn the_karaoke_enters_on_the_measured_curve() {
    // `tvk-in` part à 316,2 ms de l'horloge de la page.
    let t0 = Instant::now();
    let mut motion = TvkMotion::launched();
    motion.open(t0);
    assert_tvk_pose(motion.pose(t0).expect("départ"), (0.0, 0.96, 30.72));
    // 407,8 ms, 512 ms et 616,3 ms de la page.
    assert_tvk_pose(
      motion.pose(ms(t0, 92)).expect("en cours"),
      (0.565_976, 0.982_639, 13.6476),
    );
    assert_tvk_pose(
      motion.pose(ms(t0, 196)).expect("en cours"),
      (0.871_167, 0.994_847, 4.10141),
    );
    assert_tvk_pose(
      motion.pose(ms(t0, 300)).expect("en cours"),
      (0.975_519, 0.999_021, 0.782_637),
    );
    assert!(motion.running(ms(t0, 399)));
    assert_eq!(
      motion.pose(ms(t0, 400)),
      None,
      "au repos : sans transformation"
    );
    assert!(!motion.running(ms(t0, 400)));
  }

  #[test]
  fn the_karaoke_leaves_on_the_measured_curve_then_disappears() {
    // `tvk-out` part à 2587,3 ms de la page et `.hidden` vient 320 ms après la fermeture.
    let t0 = Instant::now();
    let mut motion = TvkMotion::launched();
    motion.open(t0);
    motion.close(ms(t0, 1000));
    // 2745,6 ms et 2858,1 ms de la page.
    assert_tvk_pose(
      motion.pose(ms(t0, 1158)).expect("en cours"),
      (0.644_832, 0.985_793, 11.2039),
    );
    assert_tvk_pose(
      motion.pose(ms(t0, 1271)).expect("en cours"),
      (0.152_276, 0.966_091, 26.2073),
    );
    assert!(motion.leaving(ms(t0, 1319)));
    assert!(motion.running(ms(t0, 1319)));
    assert!(!motion.leaving(ms(t0, 1320)));
    assert!(!motion.running(ms(t0, 1320)));
    // `fill-mode: both` : l'opacité reste à 0 entre 300 et 320 ms.
    assert_eq!(
      motion.pose(ms(t0, 1310)).map(|pose| pose.opacity),
      Some(0.0)
    );
  }

  #[test]
  fn the_karaoke_motion_is_inert_outside_the_launched_application() {
    let t0 = Instant::now();
    let mut motion = TvkMotion::default();
    motion.open(t0);
    motion.close(ms(t0, 10));
    assert_eq!(motion.pose(t0), None);
    assert!(!motion.running(t0) && !motion.leaving(t0));
    assert_eq!(
      motion.orbit_seconds(ms(t0, 5000)),
      0.0,
      "banc : première image clé"
    );
  }

  #[test]
  fn the_orbit_clock_runs_from_the_opening_and_freezes_on_closing() {
    let t0 = Instant::now();
    let mut motion = TvkMotion::launched();
    assert_eq!(motion.orbit_seconds(ms(t0, 3000)), 0.0, "jamais ouvert");
    motion.open(t0);
    assert!((motion.orbit_seconds(ms(t0, 2500)) - 2.5).abs() < 1e-4);
    motion.close(ms(t0, 4000));
    assert!((motion.orbit_seconds(ms(t0, 9000)) - 4.0).abs() < 1e-4);
    motion.open(ms(t0, 9000));
    assert_eq!(
      motion.orbit_seconds(ms(t0, 9000)),
      0.0,
      "`display: none` relance les animations"
    );
  }

  #[test]
  fn navigation_starts_only_between_content_views_and_when_enabled() {
    let now = Instant::now();
    let mut motion = ViewMotion::default();
    motion.navigate(Screen::Home, Screen::Liked, now);
    assert_eq!(motion.start, None, "banc : au repos");
    motion.enabled = true;
    motion.navigate(Screen::Home, Screen::Settings, now);
    motion.navigate(Screen::Settings, Screen::Home, now);
    motion.navigate(Screen::Home, Screen::Home, now);
    assert_eq!(motion.start, None);
    motion.navigate(Screen::Home, Screen::Liked, now);
    assert_eq!(motion.start, Some(now));
  }

  fn gray(level: u8) -> Color {
    Color::from_rgb8(level, level, level)
  }

  fn at_ms(base: Instant, value: f32) -> Instant {
    base + Duration::from_secs_f32(value / 1000.0)
  }

  #[test]
  fn oklab_matches_the_values_chromium_prints() {
    // `13-theme-titre` : `color(srgb 0.0588235 ...)` (#0f0f0f) devient `oklab(0.168415 0.0000076741 0.00000338256)`.
    let [lightness, a, b] = to_oklab(gray(0x0f));
    assert!((lightness - 0.168_415).abs() < 1e-4, "{lightness}");
    assert!(a.abs() < 1e-4 && b.abs() < 1e-4, "{a} {b}");
    for level in [0x0d, 0x0f, 0x80, 0xff] {
      let back = from_oklab(to_oklab(gray(level)), 1.0);
      assert!((back.r - gray(level).r).abs() < 1e-3, "{level} {back:?}");
    }
  }

  #[test]
  fn body_background_follows_the_computed_style_of_riff() {
    // `13-theme-titre` : le fond passe de #0f0f0f à #0d0d0d ; départ de la transition à 308,6 ms (fin à 708,6 ms), courbe
    // `cubic-bezier(0.2, 0, 0, 1)` sur 400 ms. L de `oklab(L a b)` mesuré : 0,16224 à 416,9 ms, 0,16027 à 500,1 ms, 0,159204 à 625,1 ms.
    let t0 = Instant::now();
    let mut fade = ColorFade::at(gray(0x0f), ColorSpace::Oklab);
    fade.set(gray(0x0d), t0, THEME_BODY);
    assert_eq!(fade.value(t0), gray(0x0f));
    for (at, lightness) in [(108.3, 0.162_24), (191.5, 0.160_27), (316.5, 0.159_204)] {
      let shown = to_oklab(fade.value(at_ms(t0, at)))[0];
      assert!((shown - lightness).abs() < 0.000_7, "{at} {shown}");
    }
    assert!(fade.running(at_ms(t0, 399.0)));
    assert!(!fade.running(at_ms(t0, 400.0)));
    assert_eq!(fade.value(at_ms(t0, 400.0)), gray(0x0d));
  }

  #[test]
  fn play_button_follows_the_computed_style_of_riff() {
    // `13-theme-titre` : `background-color 150ms ease`, de rgb(182, 198, 201) à rgb(212, 172, 170), départ vers 305 ms ;
    // mesuré : (190, 191, 192) à 333,5 ms, (207, 176, 175) à 383,5 ms, (211, 173, 171) à 416,9 ms, (212, 172, 170) à 500,1 ms.
    let t0 = Instant::now();
    let mut fade = ColorFade::at(Color::from_rgb8(182, 198, 201), ColorSpace::Srgb);
    fade.set(Color::from_rgb8(212, 172, 170), t0, THEME_PLAY);
    for (at, expected) in [
      (28.5, [190.0, 191.0, 192.0]),
      (78.5, [207.0, 176.0, 175.0]),
      (111.9, [211.0, 173.0, 171.0]),
      (195.1, [212.0, 172.0, 170.0]),
    ] {
      let shown = fade.value(at_ms(t0, at));
      for (got, want) in [shown.r, shown.g, shown.b].into_iter().zip(expected) {
        assert!((got * 255.0 - want).abs() < 3.0, "{at} {shown:?}");
      }
    }
    assert!(!fade.running(at_ms(t0, 150.0)));
  }

  #[test]
  fn a_new_target_restarts_from_the_displayed_color() {
    let t0 = Instant::now();
    let mut fade = ColorFade::at(gray(0), ColorSpace::Srgb);
    fade.set(gray(0xff), t0, THEME_PLAY);
    let mid = at_ms(t0, 60.0);
    let shown = fade.value(mid);
    assert!(shown.r > 0.2 && shown.r < 0.95, "{shown:?}");
    let blue = Color::from_rgb8(0, 0, 0xff);
    fade.set(blue, mid, THEME_PLAY);
    assert_eq!(fade.value(mid), shown, "continu au changement de cible");
    assert!(fade.running(at_ms(mid, 149.0)));
    assert_eq!(fade.value(at_ms(mid, 150.0)), blue);
    fade.set(blue, at_ms(mid, 10.0), THEME_PLAY);
    assert!(
      fade.running(at_ms(mid, 149.0)),
      "même cible : pas de redémarrage"
    );
  }

  #[test]
  fn reversing_a_fade_shortens_it() {
    // CSS Transitions 1, section 3 : revenir à la valeur de départ dure `durée * progrès eased`.
    let t0 = Instant::now();
    let mut fade = ColorFade::at(gray(0), ColorSpace::Srgb);
    fade.set(gray(0xff), t0, THEME_PLAY);
    let mid = at_ms(t0, 30.0);
    let eased = bezier_ease(tokens::THEME_PLAY_CURVE, 0.2);
    fade.set(gray(0), mid, THEME_PLAY);
    assert!(fade.running(at_ms(mid, 150.0 * eased - 1.0)));
    assert!(!fade.running(at_ms(mid, 150.0 * eased + 1.0)));
  }

  #[test]
  fn the_theme_fade_rests_and_stops_asking_for_frames() {
    let t0 = Instant::now();
    let mut theme = ThemeFade::at(gray(0x0f), Color::from_rgb8(182, 198, 201));
    assert!(!theme.running(t0));
    assert!(!theme.differs(gray(0x0f), Color::from_rgb8(182, 198, 201)));
    assert!(theme.differs(gray(0x0d), Color::from_rgb8(182, 198, 201)));
    theme.set(gray(0x0d), Color::from_rgb8(212, 172, 170), t0);
    assert!(theme.running(at_ms(t0, 399.0)));
    assert!(!theme.running(at_ms(t0, 400.0)));
    assert_eq!(theme.body.value(at_ms(t0, 400.0)), gray(0x0d));
  }

  #[test]
  fn panel_pose_follows_the_computed_style_of_riff() {
    // `06-file-attente` : départ à +279 ms de l'horloge de `samples.json`. À 329,4 : opacité 0,358923 et
    // matrix(0.989633, 0, 0, 0.989633, 8.29395, 0) ; à 287,6 : 0,0224635 et 15,8337 ; à 446 : 0,963138 et 0,880316.
    let t0 = Instant::now();
    let mut panel = PanelMotion::default();
    assert_eq!(panel.pose(t0), None, "jamais basculé : repos");
    panel.set_open(false, true, t0);
    let css_x = |pose: PanelPose| pose.offset_x / queue::CLOSED_OFFSET_X * 16.0;
    let start = panel.pose(t0).expect("départ");
    assert_eq!((start.opacity, start.scale), (0.0, 0.98));
    assert!((css_x(start) - 16.0).abs() < 1e-4);
    for (at, opacity, scale, x) in [
      (8.6, 0.022_463_5, 0.980_208, 15.8337),
      (50.4, 0.358_923, 0.989_633, 8.29395),
      (167.0, 0.963_138, 0.9989, 0.880_316),
    ] {
      let pose = panel
        .pose(t0 + Duration::from_secs_f32(at / 1000.0))
        .expect("en cours");
      assert!((pose.opacity - opacity).abs() < 0.01, "{at} {pose:?}");
      assert!((pose.scale - scale).abs() < 0.001, "{at} {pose:?}");
      assert!((css_x(pose) - x).abs() < 0.2, "{at} {pose:?}");
    }
    assert!(panel.running(ms(t0, 250)) && !panel.running(ms(t0, 260)));
    assert_eq!(panel.pose(ms(t0, 260)), None, "ouvert : repos");
  }

  #[test]
  fn panel_stays_visible_until_the_close_transition_ends() {
    let t0 = Instant::now();
    let mut panel = PanelMotion::default();
    panel.set_open(true, false, t0);
    assert_eq!(panel.pose(t0).map(|pose| pose.opacity), Some(1.0));
    assert!(panel.visible(false, ms(t0, 259)));
    assert!(!panel.visible(false, ms(t0, 260)));
    // Opacité finie à 220 ms, transform à 260 ms : le glissement continue sans fondu.
    let late = panel.pose(ms(t0, 240)).expect("transform en cours");
    assert_eq!(late.opacity, 0.0);
    assert!(late.offset_x > 0.0 && late.scale < 1.0);
    assert!(
      panel.visible(true, ms(t0, 900)),
      "ouvert : toujours visible"
    );
  }

  #[test]
  fn panel_reverses_from_the_current_pose() {
    let t0 = Instant::now();
    let mut panel = PanelMotion::default();
    panel.set_open(false, true, t0);
    let mid = panel.pose(ms(t0, 100)).expect("en cours");
    panel.set_open(true, false, ms(t0, 100));
    let back = panel.pose(ms(t0, 100)).expect("inversé");
    assert!((back.opacity - mid.opacity).abs() < 1e-6 && (back.scale - mid.scale).abs() < 1e-6);
    assert!(!panel.running(ms(t0, 100 + 261)));
  }

  #[test]
  fn audio_card_unveils_from_the_bottom_and_holds_its_content_until_30_percent() {
    // `connectedExpand` : la courbe court sur tout le trajet ; `connectedContentIn` tient l'image de départ jusqu'à 96 ms.
    let t0 = Instant::now();
    let mut motion = AudioMotion::default();
    assert_eq!(motion.card_pose(t0), None);
    motion.open(t0);
    let start = motion.card_pose(t0).expect("départ");
    assert_eq!(start.visible, 0.0);
    assert_eq!(start.content.opacity, 0.0);
    assert!((start.content.offset_y - audio::CONTENT_OFFSET).abs() < 1e-5);
    let mid = motion.card_pose(ms(t0, 160)).expect("milieu");
    assert!((mid.visible - bezier_ease(audio::EXPAND_CURVE, 0.5)).abs() < 1e-6);
    let held = motion.card_pose(ms(t0, 96)).expect("fin du maintien");
    assert_eq!(held.content.opacity, 0.0);
    assert!((held.content.offset_y - audio::CONTENT_OFFSET).abs() < 1e-5);
    let after = motion.card_pose(ms(t0, 200)).expect("segment 30 % - 100 %");
    let eased = bezier_ease(audio::CONTENT_CURVE, (200.0 / 320.0 - 0.3) / 0.7);
    assert!((after.content.opacity - eased).abs() < 1e-5);
    assert!((after.content.offset_y - audio::CONTENT_OFFSET * (1.0 - eased)).abs() < 1e-5);
    assert!(motion.running(ms(t0, 319)));
    assert_eq!(motion.card_pose(ms(t0, 320)), None);
    assert!(!motion.running(ms(t0, 320)));
  }

  #[test]
  fn audio_card_collapses_in_200_ms_and_its_content_leaves_in_120_ms() {
    let t0 = Instant::now();
    let mut motion = AudioMotion::default();
    motion.open(t0);
    motion.close(ms(t0, 1000));
    let start = motion.card_pose(ms(t0, 1000)).expect("départ");
    assert_eq!((start.visible, start.content.opacity), (1.0, 1.0));
    assert_eq!(start.content.offset_y, 0.0);
    let mid = motion.card_pose(ms(t0, 1060)).expect("60 ms");
    let collapse = bezier_ease(audio::COLLAPSE_CURVE, 0.3);
    let out = bezier_ease(audio::CONTENT_OUT_CURVE, 0.5);
    assert!((mid.visible - (1.0 - collapse)).abs() < 1e-6);
    assert!((mid.content.opacity - (1.0 - out)).abs() < 1e-6);
    assert!((mid.content.offset_y - audio::CONTENT_OUT_OFFSET * out).abs() < 1e-5);
    // `both` : le contenu reste à l'opacité 0 de 120 ms à 200 ms, la carte est retirée à 200 ms.
    let late = motion.card_pose(ms(t0, 1150)).expect("150 ms");
    assert_eq!(late.content.opacity, 0.0);
    assert!(motion.closing(ms(t0, 1199)) && motion.running(ms(t0, 1199)));
    assert!(!motion.closing(ms(t0, 1200)) && !motion.running(ms(t0, 1200)));
    assert_eq!(motion.card_pose(ms(t0, 1200)), None);
  }

  #[test]
  fn reopening_the_audio_card_replaces_the_collapse() {
    let t0 = Instant::now();
    let mut motion = AudioMotion::default();
    motion.close(t0);
    motion.open(ms(t0, 100));
    assert!(!motion.closing(ms(t0, 120)));
    assert_eq!(motion.card_pose(ms(t0, 100)).expect("reparti").visible, 0.0);
  }

  #[test]
  fn audio_panel_fades_in_over_220_ms() {
    let t0 = Instant::now();
    let mut motion = AudioMotion::default();
    assert_eq!(motion.panel_pose(t0), None);
    motion.switch_panel(t0, false);
    let start = motion.panel_pose(t0).expect("départ");
    assert_eq!(start.opacity, 0.0);
    assert!((start.offset_y - audio::PANEL_OFFSET).abs() < 1e-5);
    let mid = motion.panel_pose(ms(t0, 110)).expect("milieu");
    let eased = bezier_ease(audio::PANEL_CURVE, 0.5);
    assert!((mid.opacity - eased).abs() < 1e-6);
    assert!((mid.offset_y - audio::PANEL_OFFSET * (1.0 - eased)).abs() < 1e-5);
    assert_eq!(motion.panel_pose(ms(t0, 220)), None);
    assert!(!motion.running(ms(t0, 220)));
    assert!(
      !motion.dot_started(),
      "un autre onglet ne lance pas le point"
    );
    motion.switch_panel(ms(t0, 300), true);
    assert!(motion.dot_started());
  }

  #[test]
  fn sleep_dot_pulses_between_the_two_keyframes_with_ease_in_out() {
    let t0 = Instant::now();
    let mut motion = AudioMotion::default();
    assert_eq!(motion.dot_pose(t0).opacity, 1.0, "sans départ : au repos");
    motion.start_dot(t0);
    let low = motion.dot_pose(t0);
    assert_eq!((low.opacity, low.scale), (0.35, 0.75));
    let quarter = motion.dot_pose(ms(t0, 500));
    let eased = bezier_ease(audio::DOT_CURVE, 0.5);
    assert!((quarter.opacity - (0.35 + 0.65 * eased)).abs() < 1e-5);
    assert!((quarter.scale - (0.75 + 0.25 * eased)).abs() < 1e-5);
    let peak = motion.dot_pose(ms(t0, 1000));
    assert_eq!((peak.opacity, peak.scale), (1.0, 1.0));
    let back = motion.dot_pose(ms(t0, 1500));
    assert!((back.opacity - quarter.opacity).abs() < 1e-3);
    let wrapped = motion.dot_pose(ms(t0, 2500));
    assert!((wrapped.opacity - quarter.opacity).abs() < 1e-4);
    assert!((motion.dot_pose(ms(t0, 2000)).opacity - 0.35).abs() < 1e-5);
  }
  #[test]
  fn track_goes_through_the_ease_curve() {
    let t0 = Instant::now();
    let mut track = Track::at(0.0);
    track.set(1.0, t0, EASE);
    assert_eq!(track.value(t0), 0.0);
    let half = track.value(ms(t0, 90));
    assert!((half - bezier_ease(tokens::HOVER_CURVE, 0.5)).abs() < 1e-5);
    assert_eq!(track.value(ms(t0, 180)), 1.0);
    assert!(track.running(ms(t0, 100)) && !track.running(ms(t0, 180)));
  }

  #[test]
  fn track_reverses_from_the_current_value_with_a_shortened_duration() {
    let t0 = Instant::now();
    let mut track = Track::at(0.0);
    track.set(1.0, t0, EASE);
    let mid = track.value(ms(t0, 60));
    track.set(0.0, ms(t0, 60), EASE);
    assert!(
      (track.value(ms(t0, 60)) - mid).abs() < 1e-6,
      "repart de la valeur courante"
    );
    // Durée raccourcie au chemin parcouru : 180 ms * mid.
    let end = 60.0 + 180.0 * mid + 1.0;
    assert_eq!(track.value(t0 + Duration::from_secs_f32(end / 1000.0)), 0.0);
    // Même cible : rien ne change.
    let before = track;
    track.set(0.0, ms(t0, 70), EASE);
    assert_eq!(track, before);
  }

  #[test]
  fn tracks_fall_back_without_a_track_and_keep_keys_apart() {
    let t0 = Instant::now();
    let mut tracks = Tracks::<u8>::default();
    assert_eq!(tracks.value(1, t0, 1.0), 1.0);
    tracks.set(1, 1.0, t0, EASE);
    tracks.set(2, 1.0, ms(t0, 10), EASE);
    assert_eq!(tracks.value(1, ms(t0, 500), 0.0), 1.0);
    assert!(tracks.animating(ms(t0, 20)) && !tracks.animating(ms(t0, 500)));
    tracks.set(1, 0.0, ms(t0, 600), EASE);
    tracks.set(2, 0.0, ms(t0, 600), EASE);
    tracks.set(3, 0.0, ms(t0, 900), EASE);
    assert_eq!(tracks.value(1, ms(t0, 900), 0.0), 0.0);
    assert_eq!(
      tracks.entries.len(),
      1,
      "les pistes finies à 0 sont retirées"
    );
  }
  /// Les trois transitions de l'ouverture des réglages et du menu, comparées à `getComputedStyle` de `samples.json`
  /// (instants relatifs au départ ajusté : 277,7 ms et 1402,8 ms pour `05`, 255,5 ms et 1659,7 ms pour `04`).
  #[test]
  fn the_menu_fade_follows_the_samples_of_riff_in_both_directions() {
    let t0 = Instant::now();
    let mut motion = OverlayMotion {
      enabled: true,
      ..OverlayMotion::default()
    };
    assert_eq!(
      motion.menu_opacity(t0),
      1.0,
      "aucune transition posée : au repos"
    );
    motion.open_menu(t0);
    for (at, riff) in [
      (0.0, 0.0),
      (4.2, 0.0120668),
      (8.3, 0.029_429_8),
      (41.7, 0.367_567),
      (83.4, 0.763_422),
      (137.5, 0.965_113),
      (183.0, 1.0),
    ] {
      let value = motion.menu_opacity(t0 + Duration::from_secs_f32(at / 1000.0));
      assert!(
        (value - riff).abs() < 0.01,
        "ouverture à {at} ms : {value} contre {riff}"
      );
    }
    let close = ms(t0, 1000);
    motion.close_menu(close);
    for (at, riff) in [
      (54.2, 0.484_249),
      (120.9, 0.072_567_3),
      (162.6, 0.005_284_76),
      (183.5, 0.0),
    ] {
      let value = motion.menu_opacity(close + Duration::from_secs_f32(at / 1000.0));
      assert!(
        (value - riff).abs() < 0.01,
        "fermeture à {at} ms : {value} contre {riff}"
      );
    }
    assert!(motion.menu_fading(ms(close, 100)) && !motion.menu_fading(ms(close, 181)));
    assert!(motion.animating(ms(close, 100)) && !motion.animating(ms(close, 181)));
  }

  #[test]
  fn a_reversed_menu_fade_restarts_from_the_current_opacity() {
    let t0 = Instant::now();
    let mut motion = OverlayMotion {
      enabled: true,
      ..OverlayMotion::default()
    };
    motion.open_menu(t0);
    let mid = motion.menu_opacity(ms(t0, 60));
    motion.close_menu(ms(t0, 60));
    assert!((motion.menu_opacity(ms(t0, 60)) - mid).abs() < 1e-6);
    assert!(motion.menu_opacity(ms(t0, 70)) < mid);
    motion.open_menu(ms(t0, 70));
    assert!(motion.menu_opacity(ms(t0, 80)) > motion.menu_opacity(ms(t0, 70)));
  }

  #[test]
  fn the_settings_pose_follows_the_samples_of_riff_in_both_directions() {
    let t0 = Instant::now();
    let mut motion = OverlayMotion {
      enabled: true,
      ..OverlayMotion::default()
    };
    assert_eq!(motion.settings_pose(t0), None, "jamais ouverte : au repos");
    motion.navigate(Screen::Home, Screen::Settings, t0);
    let css = |pose: SettingsPose| pose.card.offset_y / crate::theme::tokens::css(1.0);
    // `04-reglages` : (instant depuis le départ, voile, carte, échelle, translateY en px CSS).
    for (at, scrim, card, scale, ty) in [
      (0.0, 0.0, 0.0, 0.95, 7.6),
      (12.4, 0.054_108_2, 0.061_830_9, 0.952_386, 7.255_46),
      (41.5, 0.543_391, 0.569_343, 0.975_915, 3.760_84),
      (116.6, 0.934_644, 0.947_243, 0.996_086, 0.623_857),
    ] {
      let pose = motion
        .settings_pose(t0 + Duration::from_secs_f32(at / 1000.0))
        .expect("en cours");
      assert!((pose.scrim - scrim).abs() < 0.01, "voile à {at} : {pose:?}");
      assert!(
        (pose.card.opacity - scrim * card).abs() < 0.01,
        "carte à {at} : {pose:?}"
      );
      assert!(
        (pose.card.scale - scale).abs() < 0.001,
        "échelle à {at} : {pose:?}"
      );
      assert!((css(pose) - ty).abs() < 0.05, "décalage à {at} : {pose:?}");
    }
    assert_eq!(
      motion.settings_pose(ms(t0, 200)),
      None,
      "ouverte : au repos"
    );
    assert!(!motion.settings_leaving(ms(t0, 500)));
    let close = ms(t0, 1000);
    motion.navigate(Screen::Settings, Screen::Home, close);
    assert!(motion.settings_leaving(ms(close, 100)));
    for (at, scrim, card, scale, ty) in [
      (37.5, 0.506_062, 0.479_165, 0.976_601, 3.656_22),
      (83.4, 0.164_544, 0.146_336, 0.959_125, 6.2727),
      (200.0, 0.0, 0.0, 0.95, 7.6),
    ] {
      let pose = motion
        .settings_pose(close + Duration::from_secs_f32(at / 1000.0))
        .expect("en cours");
      assert!((pose.scrim - scrim).abs() < 0.01, "voile à {at} : {pose:?}");
      assert!(
        (pose.card.opacity - scrim * card).abs() < 0.01,
        "carte à {at} : {pose:?}"
      );
      assert!(
        (pose.card.scale - scale).abs() < 0.001,
        "échelle à {at} : {pose:?}"
      );
      assert!((css(pose) - ty).abs() < 0.05, "décalage à {at} : {pose:?}");
    }
    assert!(!motion.settings_leaving(ms(close, 191)));
    assert!(motion.animating(ms(close, 195)) && !motion.animating(ms(close, 201)));
  }

  #[test]
  fn the_settings_transition_ignores_other_navigation_and_a_disabled_motion() {
    let t0 = Instant::now();
    let mut motion = OverlayMotion::default();
    motion.navigate(Screen::Home, Screen::Settings, t0);
    assert_eq!(motion.settings_pose(t0), None, "banc : au repos");
    motion.enabled = true;
    motion.navigate(Screen::Home, Screen::Liked, t0);
    motion.navigate(Screen::Settings, Screen::Settings, t0);
    assert_eq!(motion.settings_pose(t0), None);
    assert!(!motion.animating(t0));
  }

  fn entries(t0: Instant) -> EntryMotion {
    EntryMotion {
      enabled: true,
      rows: Some(t0),
      plain: None,
      more: None,
      earlier: None,
      artist: Some(t0),
      picks: Some(t0),
      sidebar: None,
      section: None,
      error: None,
    }
  }

  #[test]
  fn a_row_waits_for_its_delay_then_fades_without_moving() {
    let t0 = Instant::now();
    let motion = entries(t0);
    // `backwards` : la 8e ligne (délai 210 ms) reste à l'image de départ jusqu'à son départ.
    let waiting = motion.row_pose(7, ms(t0, 100)).expect("en attente");
    assert_eq!(waiting.opacity, 0.0);
    // 175 ms après le départ de la première ligne : progrès 0,5 de `cubic-bezier(0.2, 0, 0, 1)`.
    let half = motion.row_pose(0, ms(t0, 175)).expect("en cours");
    assert!(
      (half.opacity - bezier_ease(fade::CURVE, 0.5)).abs() < 1e-5,
      "{half:?}"
    );
    assert_eq!(half.offset_y, 0.0, "`transform: none !important`");
    assert_eq!(motion.row_pose(0, ms(t0, 350)), None);
    // Au-delà de la huitième : aucun décalage.
    let late = motion.row_pose(20, ms(t0, 175)).expect("en cours");
    assert_eq!(late.opacity, half.opacity);
    assert_eq!(
      motion
        .row_pose(7, ms(t0, 559))
        .map(|pose| pose.opacity > 0.99),
      Some(true)
    );
    assert_eq!(motion.row_pose(7, ms(t0, 560)), None);
  }

  #[test]
  fn two_load_more_batches_keep_their_own_start_while_they_animate() {
    let t0 = Instant::now();
    let mut motion = EntryMotion {
      enabled: true,
      ..EntryMotion::default()
    };
    motion.start_more(t0, 10);
    motion.start_more(ms(t0, 200), 20);
    let at = ms(t0, 300);
    // Rang 12 (1er lot) : 300 ms écoulés, pas de saut à l'opacité 1.
    let first = motion.row_pose(12, at).expect("1er lot en fondu");
    assert!((first.opacity - bezier_ease(fade::CURVE, 300.0 / fade::ROW_MS)).abs() < 1e-5);
    // Rang 25 (2e lot) : 100 ms écoulés.
    let second = motion.row_pose(25, at).expect("2e lot en fondu");
    assert!((second.opacity - bezier_ease(fade::CURVE, 100.0 / fade::ROW_MS)).abs() < 1e-5);
    // Un lot déjà fini n'est pas gardé.
    motion.start_more(ms(t0, 2000), 30);
    assert_eq!(motion.earlier, None);
  }

  #[test]
  fn the_artist_card_and_the_pick_items_rise_by_10_px() {
    let t0 = Instant::now();
    let motion = entries(t0);
    let card = motion.artist_pose(ms(t0, 65)).expect("en cours");
    let eased = bezier_ease(fade::CURVE, 0.25);
    assert!((card.opacity - eased).abs() < 1e-5);
    assert!((card.offset_y - fade::OFFSET * (1.0 - eased)).abs() < 1e-4);
    assert_eq!(motion.artist_pose(ms(t0, 260)), None);
    // Le 3e choix (rang 2) part à 50 ms et dure 200 ms.
    assert_eq!(
      motion.pick_pose(2, ms(t0, 50)).map(|pose| pose.opacity),
      Some(0.0)
    );
    assert!(motion.pick_pose(2, ms(t0, 150)).is_some());
    assert_eq!(motion.pick_pose(2, ms(t0, 250)), None);
  }

  #[test]
  fn entries_start_only_when_enabled_and_stop_running_at_the_last_end() {
    let t0 = Instant::now();
    let mut motion = EntryMotion::default();
    motion.start_rows(t0);
    motion.start_artist(t0);
    motion.start_picks(t0);
    assert_eq!(motion, EntryMotion::default(), "banc : au repos");
    assert!(!motion.running(t0, 3));
    motion.enabled = true;
    motion.start_rows(t0);
    assert!(motion.running(ms(t0, 559), 0));
    assert!(!motion.running(ms(t0, 560), 0));
    motion.start_picks(t0);
    // 4 choix : le dernier finit à 3 * 25 + 200 = 275 ms.
    assert!(motion.running(ms(t0, 274), 4) && motion.running(ms(t0, 559), 4));
    motion.rows = None;
    assert!(!motion.running(ms(t0, 275), 4));
  }

  /// Valeur d'une barre attendue : `20 % + 80 % * ease-in-out(p)`, `p` étant le progrès dirigé de l'itération.
  fn bar_at(directed: f32) -> f32 {
    0.2 + 0.8 * bezier_ease(row_tokens::BAR_CURVE, directed)
  }

  #[test]
  fn a_bar_keeps_its_rule_height_during_the_delay_then_alternates() {
    // `b1` : délai 0,1 s, durée 0,75 s (`styles.css:1713`). `fill-mode: none` : 100 % avant le délai.
    assert_eq!(bar_height(0, 0.0), 1.0);
    assert_eq!(bar_height(0, 99.0), 1.0);
    assert!((bar_height(0, 100.0) - 0.2).abs() < 1e-6, "départ à 20 %");
    assert!((bar_height(0, 475.0) - 0.6).abs() < 1e-4, "milieu : 60 %");
    assert!((bar_height(0, 100.0 + 187.5) - bar_at(0.25)).abs() < 1e-5);
    // Itération 2 (`alternate`) : de 100 % vers 20 %, progrès dirigé `1 - p`.
    assert!((bar_height(0, 850.0) - 1.0).abs() < 1e-6);
    assert!((bar_height(0, 850.0 + 187.5) - bar_at(0.75)).abs() < 1e-5);
    assert!(
      (bar_height(0, 850.0 + 750.0) - 0.2).abs() < 1e-5,
      "itération 3 : repart à 20 %"
    );
    // `b3` : aucun délai, 0,85 s.
    assert!((bar_height(2, 0.0) - 0.2).abs() < 1e-6);
    assert!((bar_height(2, 425.0) - 0.6).abs() < 1e-4);
    assert!((bar_height(2, 850.0) - 1.0).abs() < 1e-6);
    // `b2` : délai 0,3 s, durée 0,55 s ; `b4` : 0,2 s et 0,65 s.
    assert_eq!(bar_height(1, 299.0), 1.0);
    assert!((bar_height(1, 300.0 + 275.0) - 0.6).abs() < 1e-4);
    assert_eq!(bar_height(3, 199.0), 1.0);
    assert!((bar_height(3, 200.0 + 325.0) - 0.6).abs() < 1e-4);
  }

  #[test]
  fn the_bars_pause_at_35_percent_and_resume_where_they_stopped() {
    let t0 = Instant::now();
    let mut bars = BarsMotion::default();
    bars.restart(t0, true);
    bars.set_running(false, ms(t0, 400));
    bars.set_running(true, ms(t0, 900));
    assert_eq!(bars.elapsed_ms(ms(t0, 300)), 300.0);
    assert_eq!(bars.elapsed_ms(ms(t0, 600)), 400.0, "figé pendant la pause");
    assert_eq!(bars.heights(ms(t0, 600), true), [row_tokens::BAR_REST; 4]);
    assert_eq!(bars.elapsed_ms(ms(t0, 1000)), 500.0);
    let resumed = bars.heights(ms(t0, 1000), true);
    assert!((resumed[2] - bar_height(2, 500.0)).abs() < 1e-6);
    // `body.is-playing` retiré à l'instant 400 : la barre `.b3` valait alors `bar_height(2, 400)`.
    assert!((bars.heights(ms(t0, 399), true)[2] - bar_height(2, 399.0)).abs() < 1e-6);
  }

  #[test]
  fn bars_without_a_timeline_follow_the_live_state_at_time_zero() {
    let bars = BarsMotion::default();
    let now = Instant::now();
    assert_eq!(bars.heights(now, false), [row_tokens::BAR_REST; 4]);
    // Temps zéro : `b3` (sans délai) à 20 %, les autres encore dans leur délai à 100 %.
    assert_eq!(bars.heights(now, true), [1.0, 1.0, 0.2, 1.0]);
  }

  #[test]
  fn the_bars_restart_on_a_new_track_and_ignore_unchanged_states() {
    let t0 = Instant::now();
    let mut bars = BarsMotion::default();
    assert!(bars.track(1, true, t0), "titre lancé");
    assert!(!bars.track(1, true, ms(t0, 100)), "rien n'a changé");
    assert!(bars.track(1, false, ms(t0, 200)), "pause");
    assert!(bars.track(1, true, ms(t0, 500)), "reprise");
    assert_eq!(bars.elapsed_ms(ms(t0, 600)), 300.0);
    assert!(
      bars.track(2, true, ms(t0, 700)),
      "autre titre : retour à zéro"
    );
    assert_eq!(bars.elapsed_ms(ms(t0, 700)), 0.0);
    assert!(bars.track(3, false, ms(t0, 800)), "lancé en pause");
    assert_eq!(bars.elapsed_ms(ms(t0, 900)), 0.0);
    assert!(!bars.running(ms(t0, 900), false), "lancé en pause");
  }

  #[test]
  fn many_pauses_fold_into_the_base_without_changing_the_elapsed_time() {
    let t0 = Instant::now();
    let mut bars = BarsMotion::default();
    bars.restart(t0, true);
    for step in 0..40_u64 {
      bars.set_running(false, ms(t0, step * 200 + 100));
      bars.set_running(true, ms(t0, step * 200 + 200));
    }
    assert!(bars.runs.len() <= MAX_RUNS);
    // 40 intervalles fermés de 100 ms, puis le dernier, rouvert à 8000 ms, vu à 8500 ms.
    assert!((bars.elapsed_ms(ms(t0, 8500)) - 4500.0).abs() < 0.5);
  }

  #[test]
  fn a_sidebar_row_waits_for_its_rank_then_rises_by_8_px_and_scales() {
    let t0 = Instant::now();
    let mut motion = EntryMotion {
      enabled: true,
      ..EntryMotion::default()
    };
    motion.start_sidebar(t0);
    // Rang 3 : part à 60 ms, à l'image de départ avant (`both`).
    let waiting = motion.sidebar_pose(3, ms(t0, 59)).expect("avant le départ");
    assert_eq!(waiting.opacity, 0.0);
    assert!((waiting.offset_y - sidebar_tokens::ENTER_OFFSET).abs() < 1e-4);
    assert!((waiting.scale - 0.985).abs() < 1e-6);
    // 120 ms après son départ : moitié de la durée, courbe `(0.2, 0, 0, 1)`.
    let pose = motion.sidebar_pose(3, ms(t0, 180)).expect("en cours");
    let eased = bezier_ease(sidebar_tokens::ENTER_CURVE, 0.5);
    assert!((pose.opacity - eased).abs() < 1e-5);
    assert!((pose.offset_y - sidebar_tokens::ENTER_OFFSET * (1.0 - eased)).abs() < 1e-4);
    assert!((pose.scale - (0.985 + 0.015 * eased)).abs() < 1e-6);
    assert_eq!(
      motion.sidebar_pose(3, ms(t0, 300)),
      None,
      "fini à 60 + 240 ms"
    );
    // Le décalage plafonne au rang 12 : les rangs 12 et 40 partent ensemble à 240 ms.
    assert_eq!(
      motion
        .sidebar_pose(12, ms(t0, 239))
        .map(|pose| pose.opacity),
      Some(0.0)
    );
    assert_eq!(
      motion.sidebar_pose(40, ms(t0, 360)),
      motion.sidebar_pose(12, ms(t0, 360))
    );
    assert!(motion.running(ms(t0, 479), 0));
    assert!(
      !motion.running(ms(t0, 480), 0),
      "dernière rangée : 12 * 20 + 240 ms"
    );
  }

  #[test]
  fn the_sidebar_entry_starts_only_when_enabled() {
    let t0 = Instant::now();
    let mut motion = EntryMotion::default();
    motion.start_sidebar(t0);
    assert_eq!(motion.sidebar, None);
    assert_eq!(motion.sidebar_pose(0, t0), None);
  }

  #[test]
  fn the_badge_turns_once_in_14_seconds_from_its_start() {
    let t0 = Instant::now();
    assert_eq!(badge_turns(None, t0), 0.0, "sans départ : angle 0");
    assert_eq!(badge_turns(Some(t0), t0), 0.0);
    assert!((badge_turns(Some(t0), ms(t0, 3500)) - 0.25).abs() < 1e-5);
    assert!((badge_turns(Some(t0), ms(t0, 7000)) - 0.5).abs() < 1e-5);
    assert!((badge_turns(Some(t0), ms(t0, 10_500)) - 0.75).abs() < 1e-5);
    // `infinite` : 14 000 ms revient à 0 (360 deg et 0 deg dessinent la même forme).
    assert!(badge_turns(Some(t0), ms(t0, 14_000)).abs() < 1e-5);
    assert!((badge_turns(Some(t0), ms(t0, 17_500)) - 0.25).abs() < 1e-5);
  }

  #[test]
  fn the_loader_changes_shape_every_1200_ms_and_turns_in_4_seconds() {
    let t0 = Instant::now();
    assert_eq!(loader_state(None, t0), (0, 0.0));
    assert_eq!(loader_state(Some(t0), ms(t0, 1199)).0, 0);
    assert_eq!(loader_state(Some(t0), ms(t0, 1200)).0, 1);
    assert_eq!(loader_state(Some(t0), ms(t0, 3600)).0, 3);
    let (_, turn) = loader_state(Some(t0), ms(t0, 1000));
    assert!((turn - 0.25).abs() < 1e-5);
    let (_, turn) = loader_state(Some(t0), ms(t0, 5000));
    assert!((turn - 0.25).abs() < 1e-5);
  }

  #[test]
  fn a_tab_pane_slides_in_over_260_ms_then_rests() {
    let t0 = Instant::now();
    let mut motion = ArtistMotion::default();
    motion.start_pane(t0);
    assert_eq!(motion.pane, None, "banc : au repos");
    motion.enabled = true;
    motion.start_pane(t0);
    let start = motion.pane_pose(t0).expect("au départ");
    assert_eq!(start.opacity, 0.0);
    assert!((start.offset_y - artist_tokens::PANE_OFFSET).abs() < 1e-5);
    assert!((start.scale - 0.99).abs() < 1e-6);
    // 130 ms : progrès 0,5 de `cubic-bezier(0.2, 0, 0, 1)`.
    let eased = bezier_ease(artist_tokens::PANE_CURVE, 0.5);
    let half = motion.pane_pose(ms(t0, 130)).expect("en cours");
    assert!((half.opacity - eased).abs() < 1e-5);
    assert!((half.offset_y - artist_tokens::PANE_OFFSET * (1.0 - eased)).abs() < 1e-4);
    assert!((half.scale - (0.99 + 0.01 * eased)).abs() < 1e-5);
    assert!(motion.pane_running(ms(t0, 259)));
    assert_eq!(motion.pane_pose(ms(t0, 260)), None);
    assert!(!motion.pane_running(ms(t0, 260)));
    // Un clic repart de zéro, même sur l'onglet déjà actif (`app.js:11910-11915`).
    motion.start_pane(ms(t0, 1000));
    assert_eq!(
      motion.pane_pose(ms(t0, 1000)).map(|pose| pose.opacity),
      Some(0.0)
    );
  }

  #[test]
  fn the_settings_section_rises_by_6_px_over_200_ms_with_ease() {
    let t0 = Instant::now();
    let mut motion = EntryMotion::default();
    motion.start_section(t0);
    assert_eq!(motion.section_pose(t0), None, "banc : au repos");
    motion.enabled = true;
    motion.start_section(t0);
    let start = motion.section_pose(t0).expect("départ");
    assert_eq!(start.opacity, 0.0);
    assert!((start.offset_y - settings_tokens::SECTION_OFFSET).abs() < 1e-5);
    // `ease` sur l'unique segment : à 100 ms le progrès est 0,5.
    let mid = motion.section_pose(ms(t0, 100)).expect("milieu");
    let eased = bezier_ease([0.25, 0.1, 0.25, 1.0], 0.5);
    assert!((mid.opacity - eased).abs() < 1e-5, "{mid:?}");
    assert!((mid.offset_y - settings_tokens::SECTION_OFFSET * (1.0 - eased)).abs() < 1e-4);
    assert_eq!(mid.scale, 1.0);
    assert!(motion.section_pose(ms(t0, 199)).is_some());
    assert_eq!(motion.section_pose(ms(t0, 200)), None);
    assert!(motion.running(ms(t0, 199), 0) && !motion.running(ms(t0, 200), 0));
  }

  #[test]
  fn the_search_error_waits_220_ms_then_fades_in_over_320_ms() {
    let t0 = Instant::now();
    let mut motion = EntryMotion::default();
    motion.start_error(t0);
    assert_eq!(motion.error_pose(t0), None, "banc : au repos");
    motion.enabled = true;
    motion.start_error(t0);
    for at in [0, 100, 219, 220] {
      let pose = motion.error_pose(ms(t0, at)).expect("délai");
      assert_eq!(pose.opacity, 0.0, "{at} ms");
      assert!((pose.offset_y - search_tokens::ERROR_FADE_OFFSET).abs() < 1e-5);
    }
    let mid = motion.error_pose(ms(t0, 380)).expect("milieu");
    let eased = bezier_ease([0.25, 0.1, 0.25, 1.0], 0.5);
    assert!((mid.opacity - eased).abs() < 1e-5, "{mid:?}");
    assert!((mid.offset_y - search_tokens::ERROR_FADE_OFFSET * (1.0 - eased)).abs() < 1e-4);
    assert!(motion.error_pose(ms(t0, 539)).is_some());
    assert_eq!(motion.error_pose(ms(t0, 540)), None, "forwards : au repos");
    assert!(motion.running(ms(t0, 539), 0) && !motion.running(ms(t0, 540), 0));
  }

  #[test]
  fn the_spinner_clock_starts_when_a_spinner_appears_and_stops_when_the_last_goes() {
    let t0 = Instant::now();
    let mut clock = SpinnerClock::default();
    assert_eq!(clock.elapsed(t0), Duration::ZERO, "sans départ");
    assert!(!clock.running());
    clock.track(true, t0);
    assert!(clock.running());
    assert_eq!(clock.elapsed(ms(t0, 350)), Duration::from_millis(350));
    // Un second spinner ne relance pas l'horloge.
    clock.track(true, ms(t0, 500));
    assert_eq!(clock.start, Some(t0));
    clock.track(false, ms(t0, 900));
    assert!(!clock.running(), "l'abonnement s'arrête");
    clock.track(true, ms(t0, 2000));
    assert_eq!(clock.start, Some(ms(t0, 2000)), "nœud recréé : repart de 0");
  }

  #[test]
  fn recreated_spinner_nodes_restart_from_zero_and_kept_ones_keep_their_start() {
    let t0 = Instant::now();
    let mut clock = SpinnerClock::default();
    clock.track(true, t0);
    // Hors de l'application lancée, un rendu n'est pas daté.
    clock.recreate();
    clock.track(true, ms(t0, 400));
    assert_eq!(clock.start, Some(t0));
    clock.enabled = true;
    clock.recreate();
    clock.track(true, ms(t0, 900));
    assert_eq!(clock.start, Some(ms(t0, 900)), "innerHTML : nouveau nœud");
    clock.track(true, ms(t0, 1500));
    assert_eq!(clock.start, Some(ms(t0, 900)), "nœud gardé");
    // Rendu sans spinner : l'horloge s'efface et le drapeau est consommé.
    clock.recreate();
    clock.track(false, ms(t0, 2000));
    assert_eq!(clock.start, None);
    clock.track(true, ms(t0, 2500));
    assert_eq!(clock.start, Some(ms(t0, 2500)));
  }

  #[test]
  fn the_karaoke_panel_opens_with_a_wipe_a_rise_and_a_180_ms_fade() {
    let t0 = Instant::now();
    let mut panel = TvkPanelMotion::default();
    panel.set_open(true, t0);
    assert_eq!(panel.pose(t0), None, "banc : au repos");
    assert!(!panel.visible(false, t0));
    let mut panel = TvkPanelMotion::launched();
    panel.set_open(true, t0);
    let start = panel.pose(t0).expect("départ");
    assert_eq!((start.opacity, start.reveal, start.scale), (0.0, 0.0, 0.98));
    assert!((start.offset_y + tvk_tokens::PANEL_OFFSET).abs() < 1e-5);
    let shape = bezier_ease(tvk_tokens::PANEL_SHAPE_CURVE, 190.0 / 380.0);
    let mid = panel.pose(ms(t0, 190)).expect("milieu");
    assert!((mid.reveal - shape).abs() < 1e-5, "{mid:?}");
    assert!((mid.scale - (0.98 + 0.02 * shape)).abs() < 1e-5);
    assert!((mid.offset_y + tvk_tokens::PANEL_OFFSET * (1.0 - shape)).abs() < 1e-4);
    assert!((mid.opacity - 1.0).abs() < 1e-6, "le fondu finit à 180 ms");
    let fade = panel.pose(ms(t0, 90)).expect("fondu").opacity;
    assert!((fade - bezier_ease(tvk_tokens::PANEL_FADE_CURVE, 0.5)).abs() < 1e-5);
    assert!(panel.pose(ms(t0, 379)).is_some());
    assert_eq!(panel.pose(ms(t0, 380)), None);
    assert!(!panel.running(ms(t0, 380)));
  }

  #[test]
  fn the_karaoke_panel_closes_by_transition_and_hides_after_380_ms() {
    let t0 = Instant::now();
    let mut panel = TvkPanelMotion::launched();
    panel.set_open(true, t0);
    let closed = ms(t0, 1000);
    panel.set_open(false, closed);
    // L'opacité tient 80 ms avant de partir, la forme part tout de suite.
    let early = panel.pose(ms(closed, 40)).expect("fermeture");
    assert_eq!(early.opacity, 1.0);
    assert!(early.reveal < 1.0);
    let fade = panel.pose(ms(closed, 80 + 130)).expect("fondu").opacity;
    assert!((fade - (1.0 - bezier_ease(tvk_tokens::PANEL_FADE_CURVE, 0.5))).abs() < 1e-5);
    // Le fondu finit à 340 ms, la découpe à 380 ms : `visibility` tient jusque-là.
    let late = panel.pose(ms(closed, 350)).expect("fin de la découpe");
    assert_eq!(late.opacity, 0.0);
    assert!(panel.visible(false, ms(closed, 379)));
    assert!(!panel.visible(false, ms(closed, 380)));
    assert_eq!(panel.pose(ms(closed, 380)), None);
  }

  #[test]
  fn the_karaoke_panel_inner_slides_6_px_in_220_ms_both_ways() {
    let t0 = Instant::now();
    let mut panel = TvkPanelMotion::default();
    panel.set_open(true, t0);
    assert_eq!(panel.inner_offset(t0), 0.0, "banc : au repos");
    let mut panel = TvkPanelMotion::launched();
    assert!(
      (panel.inner_offset(t0) + tvk_tokens::INNER_OFFSET).abs() < 1e-5,
      "fermé : -6 px"
    );
    panel.set_open(true, t0);
    assert!((panel.inner_offset(t0) + tvk_tokens::INNER_OFFSET).abs() < 1e-5);
    for (at, expected) in [(55.0, -1.05), (110.0, -0.17)] {
      let css = panel.inner_offset(ms(t0, at as u64)) / crate::theme::tokens::ZOOM;
      assert!((css - expected).abs() < 0.01, "{at} ms : {css}");
    }
    assert_eq!(panel.inner_offset(ms(t0, 220)), 0.0);
    // La forme dure 380 ms : le contenu a fini avant elle et reste immobile.
    assert!(panel.running(ms(t0, 300)));
    let closed = ms(t0, 1000);
    panel.set_open(false, closed);
    let mid = panel.inner_offset(ms(closed, 55)) / crate::theme::tokens::ZOOM;
    assert!((mid + 4.95).abs() < 0.01, "{mid}");
    assert!((panel.inner_offset(ms(closed, 220)) + tvk_tokens::INNER_OFFSET).abs() < 1e-5);
  }

  #[test]
  fn the_search_error_icon_waits_180_ms_then_fades_and_grows_over_320_ms() {
    let t0 = Instant::now();
    let mut motion = EntryMotion::default();
    motion.start_error(t0);
    assert_eq!(motion.error_icon_pose(t0), None, "banc : au repos");
    motion.enabled = true;
    motion.start_error(t0);
    for at in [0, 100, 180] {
      let pose = motion.error_icon_pose(ms(t0, at)).expect("délai");
      assert_eq!(pose.opacity, 0.0, "{at} ms");
      assert_eq!(pose.scale, 0.75, "{at} ms");
      assert_eq!(pose.offset_y, 0.0);
    }
    // 260 ms : progrès 0,25 ; 340 ms : 0,5 (valeurs de `getComputedStyle` calculées depuis les deux courbes).
    let quarter = motion.error_icon_pose(ms(t0, 260)).expect("quart");
    assert!(
      (quarter.opacity * 0.85 - 0.347).abs() < 0.002,
      "{quarter:?}"
    );
    assert!((quarter.scale - 0.9518).abs() < 0.001, "{quarter:?}");
    let half = motion.error_icon_pose(ms(t0, 340)).expect("moitié");
    assert!((half.opacity * 0.85 - 0.682).abs() < 0.002, "{half:?}");
    assert!((half.scale - 0.9906).abs() < 0.001, "{half:?}");
    assert!(motion.error_icon_pose(ms(t0, 499)).is_some());
    assert_eq!(
      motion.error_icon_pose(ms(t0, 500)),
      None,
      "au repos : 0,85 et échelle 1"
    );
    assert!(
      motion.running(ms(t0, 499), 0),
      "les images tournent jusqu'à la fin de la boîte"
    );
  }

  #[test]
  fn reopening_the_karaoke_panel_restarts_from_the_current_shape() {
    let t0 = Instant::now();
    let mut panel = TvkPanelMotion::launched();
    panel.set_open(true, t0);
    panel.set_open(false, ms(t0, 100));
    let at = panel.pose(ms(t0, 100)).expect("inversion").reveal;
    panel.set_open(true, ms(t0, 100));
    let again = panel.pose(ms(t0, 100)).expect("reprise").reveal;
    assert!((at - again).abs() < 1e-5);
    assert!(panel.pose(ms(t0, 479)).is_some());
    assert_eq!(panel.pose(ms(t0, 480)), None);
  }

  const BUTTON: Size = Size::new(34.0, 34.0);

  #[test]
  fn the_ripple_reaches_the_farthest_corner() {
    assert!((ripple_reach(Point::new(17.0, 17.0), BUTTON) - 17.0_f32.hypot(17.0)).abs() < 1e-4);
    assert!((ripple_reach(Point::new(4.0, 10.0), BUTTON) - 30.0_f32.hypot(24.0)).abs() < 1e-4);
    assert!((ripple_reach(Point::new(34.0, 0.0), BUTTON) - 34.0_f32.hypot(34.0)).abs() < 1e-4);
  }

  #[test]
  fn the_ripple_grows_and_fades_over_550_ms_on_one_curve() {
    let t0 = Instant::now();
    let mut hover = ControlHover::default();
    hover.ripple("k", Point::new(4.0, 10.0), BUTTON, t0);
    let reach = ripple_reach(Point::new(4.0, 10.0), BUTTON);
    let start = hover.ripples_of("k", t0)[0];
    assert_eq!(start.radius, 0.0);
    assert!((start.alpha - 0.16).abs() < 1e-6);
    let mid = hover.ripples_of("k", ms(t0, 140))[0];
    let eased = bezier_ease([0.2, 0.0, 0.0, 1.0], 140.0 / 550.0);
    assert!((mid.radius - reach * eased).abs() < 1e-3);
    assert!((mid.alpha - 0.16 * (1.0 - eased)).abs() < 1e-5);
    assert!(hover.animating(ms(t0, 549)));
    assert!(hover.ripples_of("k", ms(t0, 550)).is_empty());
    assert!(
      !hover.animating(ms(t0, 550)),
      "animationend : l'abonnement s'arrête"
    );
    assert!(hover.ripples_of("autre", ms(t0, 100)).is_empty());
  }

  #[test]
  fn pressed_holds_180_ms_after_a_brief_click_then_fades_in_180_ms() {
    let t0 = Instant::now();
    let mut hover = ControlHover::default();
    hover.press("k", t0);
    hover.release("k", ms(t0, 60));
    // Montée en 180 ms (`ease`, `styles.css:7509`), la classe part à 180 ms (PRESS_MIN) puis redescend en 180 ms.
    let rise = |t| hover.mix("k", ms(t0, t)).press;
    assert_eq!(rise(0), 0.0);
    assert!((rise(90) - bezier_ease([0.25, 0.1, 0.25, 1.0], 0.5)).abs() < 1e-5);
    assert_eq!(rise(180), 1.0);
    assert!(rise(179) > 0.999, "tenue jusqu'à PRESS_MIN");
    assert!((rise(270) - (1.0 - bezier_ease([0.25, 0.1, 0.25, 1.0], 0.5))).abs() < 1e-5);
    assert_eq!(rise(360), 0.0);
    assert!(hover.animating(ms(t0, 359)));
    assert!(!hover.animating(ms(t0, 360)));
  }

  #[test]
  fn pressed_leaves_at_once_when_the_pointer_was_down_longer_than_180_ms() {
    let t0 = Instant::now();
    let mut hover = ControlHover::default();
    hover.press("k", t0);
    assert!(!hover.animating(ms(t0, 400)), "bouton tenu : rien ne bouge");
    assert_eq!(hover.mix("k", ms(t0, 400)).press, 1.0);
    hover.release("k", ms(t0, 400));
    assert_eq!(hover.mix("k", ms(t0, 400)).press, 1.0);
    assert!(hover.mix("k", ms(t0, 475)).press < 0.6);
    assert_eq!(hover.mix("k", ms(t0, 580)).press, 0.0);
  }

  #[test]
  fn a_second_press_starts_from_the_current_value() {
    let t0 = Instant::now();
    let mut hover = ControlHover::default();
    hover.press("k", t0);
    hover.release("k", ms(t0, 10));
    let before = hover.mix("k", ms(t0, 255)).press;
    hover.press("k", ms(t0, 255));
    assert!((hover.mix("k", ms(t0, 255)).press - before).abs() < 1e-6);
    assert_eq!(hover.mix("k", ms(t0, 435)).press, 1.0);
  }

  #[test]
  fn view_scope_gives_buttons_the_state_at_the_app_clock() {
    let t0 = Instant::now();
    let mut hover = ControlHover::default();
    hover.set("k", true, t0);
    assert_eq!(view_mix("k"), HoverMix::default(), "hors vue : repos");
    with_view_scope(&hover, ms(t0, 180), || {
      assert_eq!(view_mix("k").color, 1.0);
      assert!(view_mix("k").tracked);
    });
    assert_eq!(view_mix("k"), HoverMix::default());
  }

  /// Deux boutons créés au même lieu d'appel (une rangée par élément d'une liste) avec le même libellé.
  #[track_caller]
  fn row_button(label: &str) -> ControlKey {
    touch_key(std::panic::Location::caller(), label)
  }

  #[test]
  fn two_buttons_of_one_call_site_and_label_get_two_keys_that_repeat_on_the_next_view() {
    let hover = ControlHover::default();
    let t0 = Instant::now();
    let build = || {
      with_view_scope(&hover, t0, || {
        (0..3).map(|_| row_button("Remove")).collect::<Vec<_>>()
      })
    };
    let first = build();
    assert_ne!(first[0], first[1]);
    assert_ne!(first[1], first[2]);
    assert_eq!(first, build(), "la vue suivante retrouve les mêmes clés");
    assert_ne!(first[0], row_button("Remove"), "hors vue : un seul rang");
  }

  #[test]
  fn hovering_one_of_two_identical_buttons_leaves_the_other_at_rest() {
    let t0 = Instant::now();
    let mut hover = ControlHover::default();
    let keys = with_view_scope(&hover, t0, || {
      (0..2).map(|_| row_button("Remove")).collect::<Vec<_>>()
    });
    hover.set(keys[0], true, t0);
    hover.ripple(keys[0], Point::new(4.0, 4.0), BUTTON, t0);
    with_view_scope(&hover, ms(t0, 180), || {
      let (a, b) = (row_button("Remove"), row_button("Remove"));
      assert_eq!(view_mix(a).color, 1.0);
      assert!(view_mix(a).hovered);
      assert_eq!(view_mix(b), HoverMix::default());
      assert_eq!(view_ripples(a).len(), 1);
      assert!(view_ripples(b).is_empty());
    });
  }

  #[test]
  fn a_salt_given_by_the_caller_makes_the_key_follow_the_row() {
    let hover = ControlHover::default();
    let t0 = Instant::now();
    with_view_scope(&hover, t0, || {
      let a = keyed(("playlist", 7_u32), || row_button("Remove"));
      let b = keyed(("playlist", 9_u32), || row_button("Remove"));
      assert_ne!(a, b);
      // La liste change d'ordre : les clés suivent l'identifiant, pas le rang.
      let again = with_view_scope(&hover, t0, || {
        let b = keyed(("playlist", 9_u32), || row_button("Remove"));
        let a = keyed(("playlist", 7_u32), || row_button("Remove"));
        (a, b)
      });
      assert_eq!(again, (a, b));
    });
  }

  #[test]
  fn the_bench_finds_a_button_by_file_label_and_rank() {
    let hover = ControlHover::default();
    let t0 = Instant::now();
    let keys = with_view_scope(&hover, t0, || {
      (0..2).map(|_| row_button("Save")).collect::<Vec<_>>()
    });
    assert_eq!(keys[0], button_key("motion.rs", "Save", 0));
    assert_eq!(keys[1], button_key("motion.rs", "Save", 1));
  }

  #[test]
  fn the_bench_finds_a_keyed_button_by_its_salt() {
    let hover = ControlHover::default();
    let t0 = Instant::now();
    let keys = with_view_scope(&hover, t0, || {
      ["a", "b"]
        .map(|id| keyed(id, || (row_button("Del"), row_button("Del"))))
        .to_vec()
    });
    assert_eq!(keys[1].0, button_key_with("b", "motion.rs", "Del", 0));
    assert_eq!(keys[1].1, button_key_with("b", "motion.rs", "Del", 1));
    assert_ne!(keys[0].0, keys[1].0);
  }

  #[test]
  fn the_state_of_a_key_is_dropped_once_its_animation_is_over() {
    let t0 = Instant::now();
    let mut hover = ControlHover::default();
    let (a, b) = (ControlKey::new("a"), ControlKey::new("b"));
    hover.set(a, true, t0);
    hover.press(b, t0);
    hover.release(b, t0);
    hover.ripple(b, Point::new(4.0, 4.0), BUTTON, t0);
    assert!(hover.tracked() > 0);
    hover.prune(ms(t0, 600));
    assert_eq!(
      hover.tracked(),
      2,
      "a reste survolé : sa couleur et son rayon"
    );
    hover.set(a, false, ms(t0, 600));
    hover.prune(ms(t0, 700));
    assert!(hover.tracked() > 0, "retour au repos en cours");
    hover.prune(ms(t0, 900));
    assert_eq!(hover.tracked(), 0, "tout est fini : plus aucune entrée");
    assert!(!hover.mix(a, ms(t0, 900)).tracked);
    // Mille boutons touchés puis finis ne laissent rien.
    for index in 0..1000 {
      hover.set(ControlKey::new(index), true, ms(t0, 800));
      hover.set(ControlKey::new(index), false, ms(t0, 800));
    }
    hover.prune(ms(t0, 1200));
    assert_eq!(hover.tracked(), 0);
  }

  const PLAIN: Timing = Timing {
    duration_ms: 200.0,
    curve: [0.0, 0.0, 1.0, 1.0],
    gate: Gate::NONE,
  };

  #[test]
  fn reversing_shortens_by_the_distance_already_travelled() {
    let t0 = Instant::now();
    let run = Run {
      from: 0.0,
      to: 1.0,
      start: t0,
      duration_ms: 200.0,
      curve: PLAIN.curve,
      gate: Gate::NONE,
    };
    assert_eq!(run.reversed_ms(200.0, 0.25, 0.0), 50.0);
    assert_eq!(run.reversed_ms(200.0, 1.0, 0.0), 200.0);
    assert_eq!(
      run.reversed_ms(200.0, 0.25, 1.0),
      200.0,
      "pas un retour au départ"
    );
    let mut track = Track::at(0.0);
    track.set(1.0, t0, PLAIN);
    track.set(0.0, ms(t0, 100), PLAIN);
    assert!(track.running(ms(t0, 195)) && !track.running(ms(t0, 205)));
    let mut fade = ColorFade::at(gray(0), ColorSpace::Srgb);
    fade.set(gray(255), t0, PLAIN);
    fade.set(gray(0), ms(t0, 100), PLAIN);
    assert!(fade.running(ms(t0, 195)) && !fade.running(ms(t0, 205)));
  }

  #[test]
  fn an_entry_waits_for_its_delay_then_ends() {
    let t0 = Instant::now();
    let at = |t| Run::fade_up(Gate::NONE, Some(t0), 100.0, 200.0, PLAIN.curve, ms(t0, t));
    assert_eq!(
      Run::fade_up(Gate::NONE, None, 0.0, 200.0, PLAIN.curve, t0),
      None
    );
    assert_eq!(at(50), Some(0.0));
    assert!((at(200).unwrap() - 0.5).abs() < 1e-3);
    assert_eq!(at(300), None);
  }

  #[test]
  fn the_shared_pose_rests_with_one_minus_the_progress() {
    let pose = eased_pose(0.25, 8.0, 0.9);
    assert_eq!(pose.opacity, 0.25);
    assert_eq!(pose.offset_y, 8.0 * 0.75);
    assert!((pose.scale - 0.925).abs() < 1e-6);
    let plain = eased_pose(0.4, 8.0, 1.0);
    assert_eq!(plain.scale, 1.0);
    let done = eased_pose(1.0, 8.0, 0.9);
    assert_eq!((done.offset_y, done.scale), (0.0, 1.0));
  }

  #[test]
  fn the_panel_tracks_open_close_with_a_delayed_fade() {
    let t0 = Instant::now();
    let timings = PanelTimings {
      shape: PLAIN,
      fade_in: PLAIN,
      fade_out: PLAIN,
      close_delay_ms: 100.0,
    };
    let mut tracks = PanelTracks::at(0.0);
    assert!(!tracks.visible(false, t0));
    tracks.set(true, t0, &timings);
    assert!(tracks.visible(false, ms(t0, 100)));
    for value in [
      tracks.shape.value(ms(t0, 100)),
      tracks.opacity.value(ms(t0, 100)),
    ] {
      assert!((value - 0.5).abs() < 1e-3);
    }
    tracks.set(false, ms(t0, 200), &timings);
    assert_eq!(
      tracks.opacity.value(ms(t0, 250)),
      1.0,
      "le fondu attend son délai"
    );
    assert!(tracks.shape.value(ms(t0, 250)) < 1.0);
    assert!(tracks.running(ms(t0, 350)) && !tracks.running(ms(t0, 600)));
  }
}
