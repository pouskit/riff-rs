//! Calcul des visualiseurs de `startVisualizers`, sans dessin : hauteurs des barres, forme de la pochette.

use crate::theme::tokens::halo;
use crate::visuals::shapes;
use std::f32::consts::{PI, TAU};

/// Octets de fréquence lus à chaque image : `frequencyBinCount` pour une FFT de 128.
pub const FREQ_BINS: usize = 64;

/// Barre de fond : moitié des barres (la seconde est son miroir) et hauteur du canvas.
pub const BAR_HALF: usize = 26;
pub const BAR_HEIGHT: f32 = 36.0;
const BAR_MIN: f32 = 3.0;
/// Vue plein écran : nombre de barres et hauteur minimale.
pub const TVK_BARS: usize = 28;
pub const TVK_MIN: f32 = 4.0;
/// Petit visualiseur de la barre de lecture.
pub const MINI_BARS: usize = 12;
pub const MINI_HEIGHT: f32 = 24.0;
const REST_FACTOR: f32 = 0.15;
const REST_MARGIN: f32 = 0.5;

fn bin(freq: &[u8], index: usize) -> f32 {
  f32::from(freq.get(index).copied().unwrap_or(0))
}

fn decay_towards(heights: &mut [f32], rest: f32) -> bool {
  let mut active = false;
  for height in heights {
    *height += (rest - *height) * REST_FACTOR;
    active |= *height > rest + REST_MARGIN;
  }
  active
}

const fn rate_target(playing: bool) -> f32 {
  if playing { 1.0 } else { 0.25 }
}

/// `vizSettings` : lissage et sensibilité en pourcentage, bornés.
#[must_use]
pub fn viz_settings(smooth: u32, sens: u32) -> (f32, f32) {
  (
    (smooth as f32 / 100.0).clamp(0.0, 0.95),
    (sens as f32 / 100.0).clamp(0.0, 3.0),
  )
}

/// Barre de fond : `barBgSmoothedHeights`.
#[derive(Debug, Clone, PartialEq)]
pub struct BarBackground {
  pub heights: [f32; BAR_HALF],
  pub active: bool,
}

impl Default for BarBackground {
  fn default() -> Self {
    Self {
      heights: [0.0; BAR_HALF],
      active: false,
    }
  }
}

impl BarBackground {
  /// Une image en lecture.
  pub fn update(&mut self, freq: &[u8], smooth: f32, sens: f32) {
    let usable = BAR_HEIGHT - 6.0;
    for (index, height) in self.heights.iter_mut().enumerate() {
      let first = index * 2;
      let second = (first + 1).min(freq.len().saturating_sub(1));
      let raw = (bin(freq, first) + bin(freq, second)) * 0.5 * sens;
      let target = BAR_MIN + (raw / 255.0).clamp(0.0, 1.0).powf(1.25) * usable;
      *height = *height * smooth + target * (1.0 - smooth);
    }
    self.active = true;
  }

  /// Une image sans lecture : les barres retombent. `active` passe à faux une fois au repos.
  pub fn decay(&mut self) {
    self.active = decay_towards(&mut self.heights, BAR_MIN);
  }

  /// Hauteurs symétriques, de gauche à droite.
  #[must_use]
  pub fn symmetric(&self) -> [f32; BAR_HALF * 2] {
    let mut symmetric = [0.0; BAR_HALF * 2];
    for (index, height) in self.heights.iter().enumerate() {
      symmetric[BAR_HALF - 1 - index] = *height;
      symmetric[BAR_HALF + index] = *height;
    }
    symmetric
  }
}

/// Barres de la vue plein écran : `vizPrev` et `tvkSmoothedHeights`.
#[derive(Debug, Clone, PartialEq)]
pub struct TvkBars {
  previous: [f32; FREQ_BINS],
  pub heights: [f32; TVK_BARS],
  pub active: bool,
}

impl Default for TvkBars {
  fn default() -> Self {
    Self {
      previous: [0.0; FREQ_BINS],
      heights: [TVK_MIN; TVK_BARS],
      active: false,
    }
  }
}

impl TvkBars {
  /// Une image en lecture, pour une zone de hauteur donnée.
  pub fn update(&mut self, freq: &[u8], smooth: f32, sens: f32, height: f32) {
    for (index, previous) in self.previous.iter_mut().enumerate() {
      *previous = *previous * smooth + (bin(freq, index) * sens).min(255.0) * (1.0 - smooth);
    }
    let usable = (height - TVK_MIN - 2.0).max(10.0);
    for (index, bar) in self.heights.iter_mut().enumerate() {
      let first = index * 2;
      let raw = (self.previous[first] + self.previous[(first + 1).min(FREQ_BINS - 1)]) * 0.5;
      *bar = TVK_MIN + (raw / 255.0).powf(1.3) * 0.78 * usable;
    }
    self.active = true;
  }

  /// Une image sans lecture.
  pub fn decay(&mut self) {
    self.active = decay_towards(&mut self.heights, TVK_MIN);
  }
}

/// Hauteurs du petit visualiseur : une valeur sur deux, 2 pixels au moins.
#[must_use]
pub fn mini_bars(freq: &[u8]) -> [f32; MINI_BARS] {
  std::array::from_fn(|index| (bin(freq, index * 2) / 255.0 * (MINI_HEIGHT - 4.0)).max(2.0))
}

/// Barres d'une ligne de blanc : `h0` à `h3`.
#[must_use]
pub fn gap_bars(freq: &[u8], playing: bool, min: f32, max: f32) -> [f32; 4] {
  if !playing {
    return [min; 4];
  }
  std::array::from_fn(|index| {
    (bin(freq, index * 2 + 1) / 255.0 * max)
      .round()
      .clamp(min, max)
  })
}

/// Liquide de `drawConnectedLiquidWave` : ressorts et phase.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiquidWave {
  nodes: Vec<f32>,
  phase: f32,
}

impl LiquidWave {
  /// Une image : points du contour, du bord gauche au bord droit, en pixels.
  pub fn points(&mut self, heights: &[f32], width: f32, height: f32) -> Vec<(f32, f32)> {
    const COUNT: usize = 34;
    if heights.is_empty() || width <= 0.0 || height <= 0.0 {
      return Vec::new();
    }
    self.phase += 0.075;
    self.nodes.resize(COUNT, 0.0);
    let last = heights.len() - 1;
    let reference = (height * 0.85).max(22.0);
    for (index, node) in self.nodes.iter_mut().enumerate() {
      let x = index as f32 / (COUNT - 1) as f32;
      let distance = (x - 0.5).abs() * 2.0;
      let mapped = if x >= 0.5 {
        distance.powf(0.82) * last.min(18) as f32
      } else {
        distance.powf(0.78) * last.min(15) as f32
      };
      let low = mapped.floor() as usize;
      let high = (low + 1).min(last);
      let fraction = mapped - low as f32;
      let raw = heights[low] * (1.0 - fraction) + heights[high] * fraction;
      let boosted = ((raw - 1.8).max(0.0) / reference).min(1.0).powf(0.62) * height * 1.18;
      // En f32, sin(PI) est négatif et sa puissance vaudrait NaN.
      let envelope = (x * PI).sin().max(0.0).powf(0.58);
      let rank = index as f32;
      let harmonic = if boosted > 0.6 {
        ((rank * 0.65 + self.phase).sin() * 0.14 + (rank * 0.35 - self.phase * 1.3).cos() * 0.09)
          * boosted
      } else {
        0.0
      };
      let target = ((boosted + harmonic) * envelope).clamp(0.0, height - 1.5);
      *node += (target - *node) * if target > *node { 0.68 } else { 0.22 };
      if *node < 0.15 {
        *node = 0.0;
      }
    }
    let step = width / (COUNT + 1) as f32;
    let mut points = Vec::with_capacity(COUNT + 2);
    points.push((0.0, height));
    for index in 0..COUNT {
      let previous = self.nodes[index.saturating_sub(1)];
      let next = self.nodes[(index + 1).min(COUNT - 1)];
      let value = previous * 0.2 + self.nodes[index] * 0.6 + next * 0.2;
      points.push(((index + 1) as f32 * step, (height - value).max(1.0)));
    }
    points.push((width, height));
    points
  }
}

/// État de la pochette en forme : `m3c` (`app.js:286`). Le tirage de la forme suivante est décidé par l'appelant.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverShape {
  pub from: usize,
  pub to: usize,
  /// Temps écoulé depuis le début de la période de la forme (`now - m3c.start`), en ms.
  pub morph_ms: f64,
  pub rotation: f32,
  pub rotation_rate: f32,
  pub phase: f32,
  /// `m3c.isHovered` : la souris est sur la pochette.
  pub hovered: bool,
  /// `m3c.hoverProgress`, de 0 à 1.
  pub hover_progress: f32,
  /// La boucle de Riff a demandé une image de plus (`m3c.raf`).
  running: bool,
  /// Temps écoulé depuis la dernière image de la boucle, accumulé tant qu'elle était arrêtée (`now - m3c.last`).
  idle_ms: f64,
  history: Vec<usize>,
  seed: u32,
}

impl Default for CoverShape {
  fn default() -> Self {
    Self {
      from: 0,
      to: 0,
      morph_ms: 0.0,
      rotation: 0.0,
      rotation_rate: halo::RATE_PAUSED,
      phase: 0.0,
      hovered: false,
      hover_progress: 0.0,
      running: false,
      idle_ms: 0.0,
      history: vec![0],
      seed: 49_734_321,
    }
  }
}

/// Approche exponentielle de `value` vers `target` : `valeur += (cible - valeur) * facteur` par image de 1000/240 ms
/// (`halo::FRAME_MS`), soit `reste = (1 - facteur)^(dt / FRAME_MS)` pour une durée `dt` quelconque.
fn approach(value: f32, target: f32, factor: f32, dt_ms: f64) -> f32 {
  target + (value - target) * (1.0 - factor).powf((dt_ms / halo::FRAME_MS) as f32)
}

impl CoverShape {
  /// Une image de `m3cLoop` (`app.js:16953`), `delta_ms` après la précédente image de l'application. Tant que la boucle de Riff est
  /// arrêtée (pause, sans survol, vitesse stabilisée), l'état est figé : le temps écoulé est gardé pour la reprise, où le pas de
  /// rotation et de phase est borné à 64 ms et où la forme calcule sur `now - start`.
  pub fn advance(&mut self, delta_ms: f64, playing: bool) {
    let delta_ms = delta_ms.max(0.0);
    if !playing && !self.is_settling(playing) {
      self.idle_ms += delta_ms;
      return;
    }
    // Première image après un arrêt : une seule étape d'approche, comme la première image de Riff après `m3cStart`.
    let approach_ms = if self.running {
      delta_ms
    } else {
      halo::FRAME_MS
    };
    let elapsed = self.idle_ms + delta_ms;
    self.idle_ms = 0.0;
    let hover_target = if self.hovered { 1.0 } else { 0.0 };
    let hover_moving = (hover_target - self.hover_progress).abs() > halo::HOVER_EPSILON;
    self.hover_progress = if hover_moving {
      approach(
        self.hover_progress,
        hover_target,
        halo::HOVER_FACTOR,
        approach_ms,
      )
    } else {
      hover_target
    };
    let target = rate_target(playing);
    let rate_moving = (target - self.rotation_rate).abs() > halo::RATE_EPSILON;
    self.rotation_rate = if rate_moving {
      approach(self.rotation_rate, target, halo::RATE_FACTOR, approach_ms)
    } else {
      target
    };
    let delta = (elapsed as f32).clamp(0.0, halo::MAX_STEP_MS);
    self.rotation = (self.rotation + TAU / halo::TURN_MS * self.rotation_rate * delta) % TAU;
    if playing {
      self.phase += halo::PHASE_PER_MS * delta;
    }
    self.morph_ms += elapsed;
    if self.morph_ms >= halo::HOLD_MS - halo::CLOCK_TOLERANCE_MS {
      self.from = self.to;
      self.to = self.next_shape(self.from);
      self.morph_ms = 0.0;
    }
    self.running = self.hovered || hover_moving || rate_moving;
  }

  /// `chooseNextShape` : jamais l'une des deux dernières formes.
  fn next_shape(&mut self, current: usize) -> usize {
    if self.history.last() != Some(&current) {
      self.history.push(current);
    }
    let recent = &self.history[self.history.len().saturating_sub(2)..];
    let candidates: Vec<usize> = (0..shapes::NAMES.len())
      .filter(|shape| !recent.contains(shape))
      .collect();
    self.seed ^= self.seed << 13;
    self.seed ^= self.seed >> 17;
    self.seed ^= self.seed << 5;
    let chosen = candidates[self.seed as usize % candidates.len()];
    self.history.push(chosen);
    if self.history.len() > 10 {
      self.history.remove(0);
    }
    chosen
  }

  /// Rayon interpolé pour un indice d'échantillon.
  #[must_use]
  pub fn radius(&self, profiles: &[Vec<f32>], sample: usize) -> f32 {
    let (from, to) = (profiles[self.from][sample], profiles[self.to][sample]);
    from
      + (to - from) * shapes::ease_in_out_cubic(((self.morph_ms / halo::MORPH_MS).min(1.0)) as f32)
  }

  /// `true` tant que la boucle de Riff demande une image hors lecture : survol, approche du survol ou de la vitesse en cours, ou
  /// image d'arrêt (valeurs collées à leur cible) encore due. En lecture, la boucle tourne toujours (l'appelant le sait).
  #[must_use]
  pub fn is_settling(&self, playing: bool) -> bool {
    self.running
      || self.hovered
      || (if self.hovered { 1.0 } else { 0.0 } - self.hover_progress).abs() > halo::HOVER_EPSILON
      || (rate_target(playing) - self.rotation_rate).abs() > halo::RATE_EPSILON
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::visuals::shapes::all_profiles;

  fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
  }

  #[test]
  fn the_settings_are_bounded_like_viz_settings() {
    assert_eq!(viz_settings(55, 100), (0.55, 1.0));
    assert_eq!(viz_settings(99, 400), (0.95, 3.0));
    assert_eq!(viz_settings(0, 0), (0.0, 0.0));
  }

  #[test]
  fn the_background_bars_follow_the_formula_of_riff() {
    let mut bars = BarBackground::default();
    let freq = [255_u8; FREQ_BINS];
    bars.update(&freq, 0.0, 1.0);
    // minHeight 3 + 1^1,25 * (36 - 6)
    assert!(close(bars.heights[0], 33.0), "{}", bars.heights[0]);
    assert!(bars.active);
    let mut quiet = BarBackground::default();
    quiet.update(&[0_u8; FREQ_BINS], 0.0, 1.0);
    assert!(close(quiet.heights[0], 3.0));
  }

  #[test]
  fn the_background_bars_are_smoothed_and_sensitive() {
    let freq = [128_u8; FREQ_BINS];
    let mut smooth = BarBackground::default();
    smooth.update(&freq, 0.5, 1.0);
    let mut direct = BarBackground::default();
    direct.update(&freq, 0.0, 1.0);
    assert!(close(smooth.heights[0], direct.heights[0] * 0.5));
    let mut strong = BarBackground::default();
    strong.update(&freq, 0.0, 3.0);
    assert!(
      close(strong.heights[0], 33.0),
      "le signal est borné à 1 : {}",
      strong.heights[0]
    );
  }

  #[test]
  fn the_background_bars_mirror_around_the_center() {
    let mut bars = BarBackground::default();
    bars.heights[0] = 7.0;
    bars.heights[25] = 9.0;
    let symmetric = bars.symmetric();
    assert_eq!(symmetric[25], 7.0);
    assert_eq!(symmetric[26], 7.0);
    assert_eq!(symmetric[0], 9.0);
    assert_eq!(symmetric[51], 9.0);
  }

  #[test]
  fn the_background_bars_come_to_rest_after_a_pause() {
    let mut bars = BarBackground::default();
    bars.update(&[255_u8; FREQ_BINS], 0.0, 1.0);
    let mut frames = 0;
    while bars.active {
      bars.decay();
      frames += 1;
      assert!(frames < 200, "jamais au repos");
    }
    assert!(frames > 5);
  }

  #[test]
  fn the_full_screen_bars_follow_the_formula_of_riff() {
    let mut bars = TvkBars::default();
    bars.update(&[255_u8; FREQ_BINS], 0.0, 1.0, 64.0);
    // usable 58 ; 4 + 1^1,3 * 0,78 * 58
    assert!(close(bars.heights[0], 49.24), "{}", bars.heights[0]);
    let mut none = TvkBars::default();
    none.update(&[0_u8; FREQ_BINS], 0.0, 1.0, 64.0);
    assert!(close(none.heights[27], TVK_MIN));
  }

  #[test]
  fn the_full_screen_bars_come_to_rest_after_a_pause() {
    let mut bars = TvkBars::default();
    bars.update(&[255_u8; FREQ_BINS], 0.0, 1.0, 64.0);
    assert!(bars.active);
    let mut frames = 0;
    while bars.active {
      bars.decay();
      frames += 1;
      assert!(frames < 200, "jamais au repos");
    }
  }

  #[test]
  fn the_small_visualizer_has_a_minimum_height() {
    let mut freq = [0_u8; FREQ_BINS];
    freq[2] = 255;
    let bars = mini_bars(&freq);
    assert_eq!(bars[0], 2.0);
    assert_eq!(bars[1], 20.0);
  }

  #[test]
  fn the_gap_bars_read_odd_bins_and_rest_without_playback() {
    let mut freq = [0_u8; FREQ_BINS];
    freq[1] = 255;
    freq[3] = 128;
    assert_eq!(gap_bars(&freq, true, 3.0, 22.0), [22.0, 11.0, 3.0, 3.0]);
    assert_eq!(gap_bars(&freq, false, 3.0, 22.0), [3.0; 4]);
  }

  #[test]
  fn the_liquid_wave_starts_and_ends_on_the_bottom_edge() {
    let mut wave = LiquidWave::default();
    let points = wave.points(&[30.0; 52], 340.0, 36.0);
    assert_eq!(points.first(), Some(&(0.0, 36.0)));
    assert_eq!(points.last(), Some(&(340.0, 36.0)));
    assert_eq!(points.len(), 36);
  }

  #[test]
  fn the_liquid_wave_rises_with_the_signal_and_stays_in_the_zone() {
    let mut wave = LiquidWave::default();
    let flat: Vec<f32> = wave
      .points(&[0.0; 52], 340.0, 36.0)
      .iter()
      .map(|point| point.1)
      .collect();
    assert!(
      flat.iter().all(|y| close(*y, 36.0) || *y >= 35.0),
      "{flat:?}"
    );
    let mut active = LiquidWave::default();
    let mut last = Vec::new();
    for _ in 0..20 {
      last = active.points(&[30.0; 52], 340.0, 36.0);
    }
    let top = last
      .iter()
      .map(|point| point.1)
      .fold(f32::INFINITY, f32::min);
    assert!((1.0..20.0).contains(&top), "{top}");
  }

  #[test]
  fn the_shape_holds_then_morphs_then_changes() {
    let mut shape = CoverShape::default();
    let hold_frames = (shapes::HOLD_SECS * 1000.0 / 60.0) as usize;
    for _ in 0..hold_frames {
      shape.advance(60.0, true);
    }
    assert_eq!(shape.to, shape.from, "maintien");
    for _ in 0..3 {
      shape.advance(60.0, true);
    }
    assert_ne!(
      shape.to, shape.from,
      "la forme suivante est choisie après le maintien"
    );
  }

  #[test]
  fn the_radius_goes_from_a_profile_to_the_next() {
    let profiles = all_profiles();
    let mut shape = CoverShape {
      from: 0,
      to: 3,
      morph_ms: 0.0,
      ..CoverShape::default()
    };
    assert!(close(shape.radius(&profiles, 10), profiles[0][10]));
    shape.morph_ms = halo::MORPH_MS;
    assert!(close(shape.radius(&profiles, 10), profiles[3][10]));
  }

  #[test]
  fn the_rotation_speed_follows_playback_and_settles_after_a_pause() {
    let mut shape = CoverShape::default();
    assert!(shape.is_settling(true));
    for _ in 0..100 {
      shape.advance(16.0, true);
    }
    assert_eq!(shape.rotation_rate, 1.0);
    assert!(!shape.is_settling(true));
    assert!(shape.is_settling(false));
    for _ in 0..100 {
      shape.advance(16.0, false);
    }
    assert_eq!(shape.rotation_rate, 0.25);
    assert!(!shape.is_settling(false));
  }

  #[test]
  fn the_rotation_and_phase_use_the_delta_of_riff() {
    let mut shape = CoverShape {
      rotation_rate: 1.0,
      ..CoverShape::default()
    };
    shape.advance(16.0, true);
    assert!(close(
      shape.rotation,
      std::f32::consts::TAU / 14_000.0 * 16.0
    ));
    assert!(close(shape.phase, 0.0075 * 16.0));
    // Une image de la boucle est due (`running`) : l'écart depuis la précédente (500 ms) est borné à 64 ms.
    let mut paused = CoverShape {
      rotation_rate: 0.25,
      running: true,
      ..CoverShape::default()
    };
    paused.advance(500.0, false);
    assert!(
      close(
        paused.rotation,
        std::f32::consts::TAU / 14_000.0 * 0.25 * 64.0
      ),
      "le pas est borné à 64 ms"
    );
    assert_eq!(paused.phase, 0.0);
  }

  /// `m3cLoop` de Riff (`app.js:16953-16986`) recopié image par image, sans la forme.
  #[derive(Default)]
  struct Loop {
    hover: f32,
    hovered: bool,
    rate: f32,
    rotation: f32,
    phase: f32,
  }

  impl Loop {
    fn frame(&mut self, playing: bool, delta: f32) {
      let target_hover = if self.hovered { 1.0 } else { 0.0 };
      let diff = target_hover - self.hover;
      self.hover = if diff.abs() > 0.002 {
        self.hover + diff * 0.2
      } else {
        target_hover
      };
      let target_rate = if playing { 1.0 } else { 0.25 };
      let rate_diff = target_rate - self.rate;
      self.rate = if rate_diff.abs() > 0.01 {
        self.rate + rate_diff * 0.1
      } else {
        target_rate
      };
      let delta = delta.clamp(0.0, 64.0);
      self.rotation = (self.rotation + std::f32::consts::TAU / 14_000.0 * self.rate * delta)
        % std::f32::consts::TAU;
      if playing {
        self.phase += 0.0075 * delta;
      }
    }
  }

  const FRAME: f64 = 1000.0 / 240.0;

  #[test]
  fn the_frames_at_240_hz_follow_the_loop_of_riff() {
    let mut shape = CoverShape::default();
    let mut reference = Loop {
      rate: 0.25,
      ..Loop::default()
    };
    shape.hovered = true;
    reference.hovered = true;
    for frame in 0..300 {
      if frame == 150 {
        shape.hovered = false;
        reference.hovered = false;
      }
      let playing = frame < 200;
      // Après la pause, la boucle de Riff s'arrête : la référence n'a plus d'image.
      if !playing && !shape.is_settling(playing) {
        break;
      }
      shape.advance(FRAME, playing);
      reference.frame(playing, FRAME as f32);
      assert!(
        close(shape.hover_progress, reference.hover),
        "survol {frame}"
      );
      assert!(
        close(shape.rotation_rate, reference.rate),
        "vitesse {frame}"
      );
      assert!(
        close(shape.rotation, reference.rotation),
        "rotation {frame}"
      );
      assert!(close(shape.phase, reference.phase), "phase {frame}");
    }
  }

  #[test]
  fn the_approach_is_exponential_in_time() {
    // Deux demi-pas valent un pas : `reste = (1 - facteur)^(dt / 1000/240 ms)`.
    let whole = approach(0.0, 1.0, 0.2, FRAME);
    let halves = approach(approach(0.0, 1.0, 0.2, FRAME / 2.0), 1.0, 0.2, FRAME / 2.0);
    assert!(close(whole, 0.2) && close(halves, 0.2));
    assert!(close(
      approach(0.0, 1.0, 0.2, FRAME * 4.0),
      1.0 - 0.8_f32.powi(4)
    ));
  }

  #[test]
  fn the_hover_reaches_its_target_then_the_loop_stops() {
    let mut shape = CoverShape {
      rotation_rate: 1.0,
      ..CoverShape::default()
    };
    shape.hovered = true;
    assert!(shape.is_settling(false));
    for _ in 0..200 {
      shape.advance(FRAME, false);
    }
    assert!(
      shape.hovered && shape.is_settling(false),
      "tant que survolée"
    );
    assert_eq!(shape.hover_progress, 1.0);
    shape.hovered = false;
    let mut frames = 0;
    while shape.is_settling(false) {
      shape.advance(FRAME, false);
      frames += 1;
      assert!(frames < 400, "jamais au repos");
    }
    assert_eq!(shape.hover_progress, 0.0);
    assert_eq!(shape.rotation_rate, 0.25);
  }

  #[test]
  fn a_paused_cover_freezes_and_resumes_like_the_loop_of_riff() {
    let mut shape = CoverShape {
      rotation_rate: 0.25,
      from: 2,
      to: 5,
      morph_ms: 500.0,
      rotation: 1.0,
      phase: 3.0,
      ..CoverShape::default()
    };
    // Pause stabilisée : les images des autres calques ne changent rien, le temps est gardé.
    for _ in 0..100 {
      shape.advance(16.0, false);
    }
    assert!(!shape.is_settling(false));
    assert_eq!((shape.from, shape.to, shape.morph_ms), (2, 5, 500.0));
    assert_eq!((shape.rotation, shape.phase), (1.0, 3.0));
    // Reprise après 1600 ms : pas borné à 64 ms pour la rotation et la phase, une seule période écoulée pour la forme
    // (`now - start` = 500 + 1600 >= 2000), qui repart à 0.
    shape.advance(16.0, true);
    assert!(close(
      shape.rotation,
      1.0 + std::f32::consts::TAU / 14_000.0 * 0.325 * 64.0
    ));
    assert!(close(shape.phase, 3.0 + 0.0075 * 64.0));
    assert_eq!((shape.from, shape.morph_ms), (5, 0.0));
    assert_ne!(shape.to, 5);
  }

  #[test]
  fn the_shape_changes_after_exactly_480_frames_at_240_hz() {
    let mut shape = CoverShape::default();
    for _ in 0..479 {
      shape.advance(FRAME, true);
    }
    assert_eq!(shape.to, shape.from);
    shape.advance(FRAME, true);
    assert_ne!(shape.to, shape.from);
    assert_eq!(shape.morph_ms, 0.0);
  }
}
