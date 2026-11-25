//! Personnalisation du plein écran des paroles (`#tvk-panel`) : police, graisse, animation des lignes.

use super::layout::target_scroll;
use super::scroll::{Easing, clamp_speed, transition_duration};
use crate::app::{App, Effect};
use crate::theme::fonts;
use crate::theme::tokens::ZOOM;
use crate::theme::tokens::karaoke::POP_CURVE;
use crate::theme::tokens::lyrics::LINE_OPACITY;
use crate::visuals::settings::{TvkAlign, TvkAnim, TvkStyle};
use crate::visuals::state::bezier_ease;
use iced::font::{Family, Weight};
use iced::{Font, Size};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Police des paroles de Riff dans l'application (`lyric_font`), toujours proposée : `TVK_FONTS.default` vaut `inherit`
/// (`app.js:7222`), donc `--font-family`, Google Sans.
pub const DEFAULT_FAMILY: &str = fonts::UI_FAMILY;
/// `translateY(24px)` de l'animation `slide`.
const SLIDE_OFFSET: f32 = 24.0;
/// `@keyframes tvk-pop` : 1, 1,08, 1.
const POP_SCALE: f32 = 1.08;
/// `transform: scale(0.92)` des lignes inactives de l'animation `smooth`.
const SMOOTH_SCALE: f32 = 0.92;
/// `clamp(260px, 48vh, 560px)` de la pochette du mode pochette.
const COVER_MIN: f32 = 260.0;
const COVER_MAX: f32 = 560.0;
/// `.tv-karaoke-layout` : `padding: 40px` en haut et en bas.
const LAYOUT_PADDING_Y: f32 = 40.0;
/// `.lyrics-container` : `gap: 16px`.
const CONTAINER_GAP: f32 = 16.0;
/// `setTimeout(..., 3000)` de `resetTvIdleTimer`.
pub const IDLE_DELAY: Duration = Duration::from_secs(3);

/// Choix de la liste « Font ». Les piles CSS de Riff n'ont pas d'équivalent : chaque choix a ses familles candidates,
/// la première installée est utilisée comme dans une pile CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TvkFont {
  Default,
  SfPro,
  Serif,
  Rounded,
  Mono,
  Condensed,
}

impl TvkFont {
  pub const ALL: [Self; 6] = [
    Self::Default,
    Self::SfPro,
    Self::Serif,
    Self::Rounded,
    Self::Mono,
    Self::Condensed,
  ];

  /// Libellé de `index.html`.
  #[must_use]
  pub const fn label(self) -> &'static str {
    match self {
      Self::Default => "Default",
      Self::SfPro => "SF Pro",
      Self::Serif => "Serif",
      Self::Rounded => "Rounded",
      Self::Mono => "Mono",
      Self::Condensed => "Condensed",
    }
  }

  /// `data-value` de `index.html`, écrit dans `riff_tvk`.
  #[must_use]
  pub const fn stored(self) -> &'static str {
    match self {
      Self::Default => "inherit",
      Self::SfPro => {
        "-apple-system, BlinkMacSystemFont, \"SF Pro Display\", \"SF Pro Text\", \"Helvetica Neue\", sans-serif"
      }
      Self::Serif => "Georgia, 'Times New Roman', serif",
      Self::Rounded => "ui-rounded, 'Nunito', system-ui, sans-serif",
      Self::Mono => "ui-monospace, 'JetBrains Mono', monospace",
      Self::Condensed => "'Arial Narrow', 'Roboto Condensed', sans-serif",
    }
  }

  /// Familles de la pile CSS qu'une machine Windows peut avoir. `ui-rounded`, `-apple-system` et `system-ui` n'existent pas
  /// ici ; `monospace` vaut Consolas dans Chrome sous Windows (déduit, non vérifié).
  const fn candidates(self) -> &'static [&'static str] {
    match self {
      Self::Default => &[DEFAULT_FAMILY],
      Self::SfPro => &["SF Pro Display", "SF Pro Text", "Helvetica Neue"],
      Self::Serif => &["Georgia", "Times New Roman"],
      Self::Rounded => &["Nunito"],
      Self::Mono => &["JetBrains Mono", "Consolas"],
      Self::Condensed => &["Arial Narrow", "Roboto Condensed"],
    }
  }

  /// `TVK_FONTS` : alias courts et piles exactes. Une valeur inconnue s'affiche avec la police par défaut.
  #[must_use]
  pub fn from_stored(value: &str) -> Self {
    Self::ALL
      .into_iter()
      .find(|font| {
        font.stored() == value
          || match font {
            Self::Default => value == "default" || value == "inherit",
            Self::SfPro => value == "sf" || value == "SF Pro",
            Self::Serif => value == "serif",
            Self::Rounded => value == "rounded",
            Self::Mono => value == "mono",
            Self::Condensed => value == "condensed",
          }
      })
      .unwrap_or(Self::Default)
  }
}

/// Choix de police dont une famille est installée, avec la famille retenue. « Default » reste toujours proposé.
#[must_use]
pub fn available_fonts(installed: &dyn Fn(&str) -> bool) -> Vec<(TvkFont, &'static str)> {
  TvkFont::ALL
    .into_iter()
    .filter_map(|font| match font {
      TvkFont::Default => Some((font, DEFAULT_FAMILY)),
      _ => font
        .candidates()
        .iter()
        .copied()
        .find(|name| installed(name))
        .map(|name| (font, name)),
    })
    .collect()
}

/// Famille d'un choix, ou celle par défaut s'il n'est pas dans la liste.
#[must_use]
pub fn family_in(list: &[(TvkFont, &'static str)], font: TvkFont) -> &'static str {
  list
    .iter()
    .find(|(listed, _)| *listed == font)
    .map_or(DEFAULT_FAMILY, |(_, family)| family)
}

/// Polices proposées sur cette machine, lues une fois dans la base de polices d'iced (celle que le rendu interroge).
/// Le premier appel a lieu dans `relayout`, jamais dans `view`.
#[must_use]
pub fn fonts() -> &'static [(TvkFont, &'static str)] {
  static LIST: OnceLock<Vec<(TvkFont, &'static str)>> = OnceLock::new();
  LIST.get_or_init(|| {
    let Ok(mut system) = iced::advanced::graphics::text::font_system().write() else {
      return available_fonts(&|name| name == DEFAULT_FAMILY);
    };
    let database = system.raw().db();
    available_fonts(&|name| {
      database
        .faces()
        .any(|face| face.families.iter().any(|(family, _)| family == name))
    })
  })
}

/// Les neuf graisses d'iced : le CSS arrondi à la centaine la plus proche.
#[must_use]
pub fn weight_of(css: u32) -> Weight {
  match (css.clamp(100, 900) + 50) / 100 {
    1 => Weight::Thin,
    2 => Weight::ExtraLight,
    3 => Weight::Light,
    4 => Weight::Normal,
    5 => Weight::Medium,
    6 => Weight::Semibold,
    7 => Weight::Bold,
    8 => Weight::ExtraBold,
    _ => Weight::Black,
  }
}

/// Police des lignes : famille choisie (ou celle par défaut) et graisse.
#[must_use]
pub fn face(style: &TvkStyle) -> Font {
  Font {
    family: Family::Name(family_in(fonts(), TvkFont::from_stored(&style.font))),
    weight: weight_of(style.weight),
    ..Font::DEFAULT
  }
}

/// Valeurs de `applyTvk` ramenées dans les bornes des curseurs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
  /// `--tvk-scale`.
  pub scale: f32,
  /// `--tvk-gap` : espace entre deux lignes.
  pub gap: f32,
  /// `--tvk-speed`.
  pub speed: Duration,
}

#[must_use]
pub fn metrics(style: &TvkStyle) -> Metrics {
  let style = style.bounded();
  Metrics {
    scale: style.size as f32 / 100.0,
    gap: style.gap as f32,
    speed: Duration::from_millis(u64::from(style.speed)),
  }
}

/// Espace entre deux lignes : `margin: var(--tvk-gap) 0` de chaque ligne (`styles.css:9096-9100`, sans fusion des marges dans une
/// colonne flex) et `gap: 16px` du conteneur (`styles.css:2428-2445`), en pixels écran.
#[must_use]
pub fn line_spacing(style: &TvkStyle) -> f32 {
  (2.0 * metrics(style).gap + CONTAINER_GAP) * ZOOM
}

/// Côté de la pochette du mode pochette pour une fenêtre de cette hauteur.
#[must_use]
pub fn cover_side(window_height: f32) -> f32 {
  (window_height / ZOOM * 0.48).clamp(COVER_MIN, COVER_MAX) * ZOOM
}

/// Côté de la pochette sans paroles (`.tv-nolyrics .tv-cover`) : `clamp(220px, 36vh, 440px)` (`styles.css:6630-6633`).
#[must_use]
pub fn nolyrics_cover_side(window_height: f32) -> f32 {
  (window_height / ZOOM * 0.36).clamp(220.0, 440.0) * ZOOM
}

/// Mesures de `.tv-karaoke-layout` (`styles.css:6461-6473`, `6477-6491`, `6531-6553`, `9060-9090`) en pixels écran.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
  /// `padding: 40px clamp(32px, 6vw, 96px)` : marge horizontale.
  pub pad_x: f32,
  /// Marge verticale du plateau.
  pub pad_y: f32,
  /// `gap: clamp(32px, 5vw, 72px)`.
  pub gap: f32,
  /// `clamp(200px, 28vw, 420px)` : largeur de `.tv-side`, de la pochette et du bloc de titre.
  pub side: f32,
  /// Largeur de la zone des paroles (`.tv-karaoke-content`).
  pub content: f32,
  /// `.lyric-line` : `max-width: 900px` dans un conteneur de `padding-inline: 8vw`.
  pub line: f32,
}

#[must_use]
pub fn geometry(window: Size) -> Geometry {
  let width = window.width / ZOOM;
  let pad_x = (width * 0.06).clamp(32.0, 96.0);
  let gap = (width * 0.05).clamp(32.0, 72.0);
  let side = (width * 0.28).clamp(200.0, 420.0);
  let content = (width - 2.0 * pad_x - side - gap).max(0.0);
  let line = (content - 2.0 * width * 0.08).clamp(0.0, 900.0);
  Geometry {
    pad_x: pad_x * ZOOM,
    pad_y: LAYOUT_PADDING_Y * ZOOM,
    gap: gap * ZOOM,
    side: side * ZOOM,
    content: content * ZOOM,
    line: line * ZOOM,
  }
}

/// Curseurs du panneau, avec les bornes et le pas de `index.html`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TvkSlider {
  Size,
  Weight,
  Gap,
  Speed,
}

impl TvkSlider {
  pub const ALL: [Self; 4] = [Self::Size, Self::Weight, Self::Gap, Self::Speed];

  #[must_use]
  pub const fn label(self) -> &'static str {
    match self {
      Self::Size => "Size",
      Self::Weight => "Weight",
      Self::Gap => "Line spacing",
      Self::Speed => "Animation speed",
    }
  }

  #[must_use]
  pub const fn range(self) -> (i32, i32) {
    let (low, high) = match self {
      Self::Size => TvkStyle::SIZE,
      Self::Weight => TvkStyle::WEIGHT,
      Self::Gap => TvkStyle::GAP,
      Self::Speed => TvkStyle::SPEED,
    };
    (low as i32, high as i32)
  }

  #[must_use]
  pub const fn step(self) -> i32 {
    match self {
      Self::Size => 5,
      Self::Weight => 100,
      Self::Gap => 1,
      Self::Speed => 25,
    }
  }

  #[must_use]
  pub fn format(self, value: i32) -> String {
    match self {
      Self::Size => format!("{value}%"),
      Self::Weight => value.to_string(),
      Self::Gap => format!("{value}px"),
      Self::Speed => format!("{value}ms"),
    }
  }

  /// Valeur affichée : celle du style, ramenée dans les bornes.
  #[must_use]
  pub fn value(self, style: &TvkStyle) -> i32 {
    let (low, high) = self.range();
    let raw = match self {
      Self::Size => style.size,
      Self::Weight => style.weight,
      Self::Gap => style.gap,
      Self::Speed => style.speed,
    };
    i32::try_from(raw).unwrap_or(i32::MAX).clamp(low, high)
  }

  fn set(self, style: &mut TvkStyle, value: i32) {
    let (low, high) = self.range();
    let value = value.clamp(low, high) as u32;
    match self {
      Self::Size => style.size = value,
      Self::Weight => style.weight = value,
      Self::Gap => style.gap = value,
      Self::Speed => style.speed = value,
    }
  }
}

/// Messages du panneau. Visualiseur avec paroles, opacité, flou et onde connectée passent par `VisualMessage`.
#[derive(Debug, Clone)]
pub enum TvkMessage {
  TogglePanel,
  /// `openTvkPanel` et `closeTvkPanel` (`app.js:7480-7500`).
  SetPanel(bool),
  /// Mouvement de souris : `resetTvIdleTimer` (`app.js:6540`).
  Activity,
  /// Une seconde de plus : après 3 s sans mouvement, la vue passe en repos, sauf panneau ouvert.
  IdleCheck,
  Font(TvkFont),
  Align(TvkAlign),
  Anim(TvkAnim),
  /// Appliqué à chaque pas ; l'écriture suit au relâchement (`SettingsMessage::CommitInt`).
  Slider(TvkSlider, i32),
  /// `riff_tvk_visualizer`.
  Visualizer(bool),
  ToggleCoverMode,
  Reset,
}

/// Changement de ligne active en cours d'animation. Il n'existe que pendant `speed` ms : c'est lui qui demande les images.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineTransition {
  /// La ligne qui était active.
  pub prev: Option<usize>,
  pub started: Instant,
  pub duration: Duration,
}

impl LineTransition {
  /// Avancement brut de 0 à 1 à l'instant `now` de l'horloge de `App`.
  #[must_use]
  pub fn progress_at(&self, now: Instant) -> f32 {
    if crate::motion::Gate::REDUCED.instant() {
      return 1.0;
    }
    (now.saturating_duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32())
      .min(1.0)
  }
}

/// Rôle d'une ligne pendant une transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRole {
  Idle,
  Active,
  Leaving,
}

/// Aspect d'une ligne : `opacity` et `transform` des règles `.tv-karaoke[data-tvk-anim] .lyric-line`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineLook {
  pub opacity: f32,
  /// Décalage vers le bas, en pixels.
  pub offset: f32,
  /// Facteur de la taille du texte.
  pub scale: f32,
}

/// Opacité d'une ligne inactive.
#[must_use]
pub const fn inactive_opacity(anim: TvkAnim) -> f32 {
  match anim {
    TvkAnim::Smooth => 0.45,
    TvkAnim::Slide => 0.35,
    TvkAnim::Fade => 0.3,
    TvkAnim::Pop | TvkAnim::None => 0.5,
  }
}

/// 0 : état inactif, 1 : état actif. `eased` est l'avancement après la courbe, `None` au repos.
fn activity(role: LineRole, eased: Option<f32>) -> f32 {
  match (role, eased) {
    (LineRole::Idle, _) | (LineRole::Leaving, None) => 0.0,
    (LineRole::Active, None) => 1.0,
    (LineRole::Active, Some(eased)) => eased,
    (LineRole::Leaving, Some(eased)) => 1.0 - eased,
  }
}

/// Opacité d'une ligne du panneau : `transition: opacity var(--lyric-duration) var(--lyric-ease)` de `.lyric-line` (`styles.css:2562`)
/// entre `opacity: 0.48` et `opacity: 1` (`styles.css:2558`, `2571`). `progress` : avancement brut, `None` au repos.
#[must_use]
pub fn panel_line_opacity(easing: Easing, role: LineRole, progress: Option<f32>) -> f32 {
  let active = activity(
    role,
    progress.map(|value| easing.apply_css(value.clamp(0.0, 1.0))),
  );
  LINE_OPACITY + (1.0 - LINE_OPACITY) * active
}

/// `@keyframes tvk-pop` (`scale(1)`, `scale(1.08)` à 50 %, `scale(1)`) : la courbe `cubic-bezier(.2,0,0,1)` de `animation` s'applique à
/// chaque segment entre images clés, pas à l'ensemble (`styles.css:9110-9114`, `9166`). `progress` : avancement brut, de 0 à 1.
fn pop_scale(progress: f32) -> f32 {
  let (from, to, local) = if progress < 0.5 {
    (1.0, POP_SCALE, progress * 2.0)
  } else {
    (POP_SCALE, 1.0, progress * 2.0 - 1.0)
  };
  from + (to - from) * bezier_ease(POP_CURVE, local)
}

/// Aspect d'une ligne. `progress` est l'avancement brut de la transition en cours, `None` au repos.
#[must_use]
pub fn line_look(anim: TvkAnim, easing: Easing, role: LineRole, progress: Option<f32>) -> LineLook {
  let progress = progress.map(|value| value.clamp(0.0, 1.0));
  let active = activity(role, progress.map(|value| easing.apply(value)));
  let mix = |inactive: f32, on: f32| inactive * (1.0 - active) + on * active;
  let scale = match (anim, role, progress) {
    (TvkAnim::Smooth, ..) => mix(SMOOTH_SCALE, 1.0),
    (TvkAnim::Pop, LineRole::Active, Some(progress)) => pop_scale(progress),
    _ => 1.0,
  };
  LineLook {
    opacity: mix(inactive_opacity(anim), 1.0),
    offset: if anim == TvkAnim::Slide {
      SLIDE_OFFSET * (1.0 - active)
    } else {
      0.0
    },
    scale,
  }
}

impl App {
  /// Il y a des paroles à montrer (`tv-nolyrics` absent).
  #[must_use]
  pub fn tvk_has_lyrics(&self) -> bool {
    self
      .lyrics
      .lyrics
      .as_ref()
      .is_some_and(|lyrics| !lyrics.lines.is_empty())
  }

  /// La vue plein écran se dessine : ouverte, ou en train de s'effacer (`tvk-out`, l'écran reste affiché jusqu'au retrait à 320 ms).
  #[must_use]
  pub fn tvk_shown(&self) -> bool {
    self.lyrics.tvk_open || self.lyrics.tvk_motion.leaving(self.now)
  }

  /// Les barres de la vue plein écran : `riff_tvk_visualizer_lyrics` avec des paroles, `riff_tvk_visualizer` sans.
  #[must_use]
  pub fn tvk_visualizer_shown(&self) -> bool {
    let visual = &self.settings.visual;
    self.lyrics.tvk_open
      && if self.tvk_has_lyrics() {
        visual.tvk_visualizer_lyrics
      } else {
        visual.tvk_visualizer
      }
  }

  /// Les paroles sont montrées : le mode pochette les masque.
  #[must_use]
  pub fn tvk_lyrics_shown(&self) -> bool {
    !self.settings.visual.tvk_cover_mode
  }

  /// Aspect de la ligne `index` dans la vue plein écran.
  #[must_use]
  pub fn tvk_look(&self, index: usize) -> LineLook {
    let visual = &self.settings.visual;
    let state = &self.lyrics;
    let role = if state.active == Some(index) {
      LineRole::Active
    } else if state
      .transition
      .is_some_and(|transition| transition.prev == Some(index))
    {
      LineRole::Leaving
    } else {
      LineRole::Idle
    };
    let anim = visual.tvk.anim;
    // `smooth` et `slide` suivent `--lyric-ease` ; `fade` et `pop` ont leur courbe, approchée par une sortie rapide.
    let easing = match anim {
      TvkAnim::Smooth | TvkAnim::Slide => Easing::from_key(&visual.lyrics_ease),
      _ => Easing::Out,
    };
    line_look(
      anim,
      easing,
      role,
      state
        .transition
        .map(|transition| transition.progress_at(self.now)),
    )
  }

  /// Opacité d'une ligne du panneau droit : transition de `.lyric-line`, sur la courbe et la durée de `riff_lyrics_ease`.
  #[must_use]
  pub fn panel_look(&self, index: usize) -> LineLook {
    let state = &self.lyrics;
    let role = if state.active == Some(index) {
      LineRole::Active
    } else if state
      .transition
      .is_some_and(|transition| transition.prev == Some(index))
    {
      LineRole::Leaving
    } else {
      LineRole::Idle
    };
    let easing = Easing::from_key(&self.settings.visual.lyrics_ease);
    LineLook {
      opacity: panel_line_opacity(
        easing,
        role,
        state
          .transition
          .map(|transition| transition.progress_at(self.now)),
      ),
      offset: 0.0,
      scale: 1.0,
    }
  }

  /// Transition d'une ligne qui devient active. Plein écran : `none` n'anime rien, la durée est `--tvk-speed`. Panneau droit : durée
  /// `--lyric-duration`. Les mini-paroles ont leurs propres règles, sans transition ici.
  pub(crate) fn line_transition(&self, prev: Option<usize>, at: Instant) -> Option<LineTransition> {
    let visual = &self.settings.visual;
    let duration = if self.lyrics.tvk_open {
      (visual.tvk.anim != TvkAnim::None).then(|| metrics(&visual.tvk).speed)
    } else {
      self.panel_visible().then(|| {
        transition_duration(
          Easing::from_key(&visual.lyrics_ease),
          clamp_speed(visual.lyrics_speed),
        )
      })
    }?;
    Some(LineTransition {
      prev,
      started: at,
      duration,
    })
  }

  /// Une transition finie disparaît : elle ne demande plus d'image.
  pub(crate) fn line_advance(&mut self, at: Instant) {
    if self
      .lyrics
      .transition
      .is_some_and(|transition| transition.progress_at(at) >= 1.0)
    {
      self.lyrics.transition = None;
    }
  }

  /// Recalcule la mise en page en gardant la ligne active et en la recentrant, sans nouvelle transition.
  fn relayout_in_place(&mut self) -> Vec<Effect> {
    let active = self.lyrics.active;
    self.relayout();
    let Some(index) = active else {
      return Vec::new();
    };
    let state = &mut self.lyrics;
    let Some(line) = state.layout.boxes.get(index).copied() else {
      return Vec::new();
    };
    let target = target_scroll(line, state.viewport);
    state.active = Some(index);
    state.offset = target;
    state.last_set_offset = target;
    vec![Effect::LyricsScroll(target)]
  }

  /// `.open` posé ou retiré sur `#tvk-panel` (`openTvkPanel`, `closeTvkPanel`, `app.js:7482-7494`) : sans effet si le panneau est déjà
  /// dans cet état. En application, la transition part de l'instant réel (`now` date de la dernière image).
  pub(crate) fn set_tvk_panel(&mut self, open: bool) {
    if self.lyrics.tvk_panel == open {
      return;
    }
    if self.lyrics.panel_motion.enabled {
      self.now = std::time::Instant::now();
    }
    self.lyrics.panel_motion.set_open(open, self.now);
    self.lyrics.tvk_panel = open;
  }

  /// Messages du panneau de personnalisation.
  pub(crate) fn update_tvk(&mut self, message: TvkMessage) -> Vec<Effect> {
    let visual = &mut self.settings.visual;
    match message {
      TvkMessage::TogglePanel => {
        self.set_tvk_panel(!self.lyrics.tvk_panel);
        return Vec::new();
      }
      TvkMessage::SetPanel(open) => {
        self.set_tvk_panel(open);
        if open {
          self.lyrics.tvk_idle = false;
        }
        return Vec::new();
      }
      TvkMessage::Activity => {
        let state = &mut self.lyrics;
        state.tvk_idle = false;
        state.tvk_active = Some(Instant::now());
        return Vec::new();
      }
      TvkMessage::IdleCheck => {
        let state = &mut self.lyrics;
        let quiet = state.tvk_active.is_none_or(|at| at.elapsed() >= IDLE_DELAY);
        if state.tvk_open && !state.tvk_panel && quiet {
          state.tvk_idle = true;
        }
        return Vec::new();
      }
      TvkMessage::Font(font) => {
        visual.tvk.font = font.stored().to_string();
        let mut effects = self.relayout_in_place();
        effects.extend(self.save_settings());
        return effects;
      }
      TvkMessage::Align(align) => visual.tvk.align = align,
      TvkMessage::Anim(anim) => {
        visual.tvk.anim = anim;
        self.lyrics.transition = None;
      }
      TvkMessage::Slider(slider, value) => {
        slider.set(&mut visual.tvk, value);
        return if slider == TvkSlider::Speed {
          Vec::new()
        } else {
          self.relayout_in_place()
        };
      }
      TvkMessage::Visualizer(on) => visual.tvk_visualizer = on,
      TvkMessage::ToggleCoverMode => visual.tvk_cover_mode = !visual.tvk_cover_mode,
      // `app.js:7734` : le style, les deux visualiseurs et l'opacité. Ni le flou, ni le mode pochette. Les clés inconnues restent.
      TvkMessage::Reset => {
        visual.tvk = TvkStyle {
          extra: std::mem::take(&mut visual.tvk.extra),
          ..TvkStyle::default()
        };
        visual.tvk_visualizer = true;
        visual.tvk_visualizer_lyrics = true;
        visual.tvk_visualizer_opacity = 85;
        self.lyrics.transition = None;
        let mut effects = self.relayout_in_place();
        effects.extend(self.save_settings());
        return effects;
      }
    }
    self.save_settings()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::Message;
  use crate::i18n::I18n;
  use crate::lyrics::model::Lyrics;
  use crate::lyrics::update::LyricsMessage;
  use crate::prefs_visual::{VisualBool, VisualInt, VisualMessage};
  use crate::settings::NativeSettings;
  use crate::user_data::{Track, UserData};
  use crate::visuals::settings::{TvkAlign, TvkAnim, TvkStyle};
  use crate::visuals::viz::FREQ_BINS;
  use crate::window::PersistedWindowState;
  use iced::Size;
  use iced::font::Weight;
  use std::time::{Duration, Instant};

  const SYNCED: &str = "[00:01.00]one\n[00:03.00]two\n[00:05.00]three\n[00:07.00]four";

  fn app() -> App {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
    .with_data(UserData::default());
    app.window.size = Some(Size::new(1280.0, 800.0));
    app.settings.visual.m3_cover = false;
    app.player.current = Some(Track {
      id: Some("a".into()),
      title: Some("Titre".into()),
      platform: Some("ytmusic".into()),
      ..Track::default()
    });
    app
  }

  fn with_lyrics() -> App {
    let mut app = app();
    app.lyrics.lyrics = Some(Lyrics::parse(SYNCED, false));
    app.lyrics.tvk_open = true;
    app.relayout();
    app
  }

  fn send(app: &mut App, message: TvkMessage) -> Vec<Effect> {
    app.update(Message::Lyrics(LyricsMessage::Tvk(message)))
  }

  fn saved(effects: &[Effect]) -> bool {
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::SaveSettings(_)))
  }

  fn tick(app: &mut App, at: Instant) {
    app.now = at;
    app.animation_tick(at, &[0; FREQ_BINS]);
  }

  /// Une lecture en pause, positionnée dans la deuxième ligne.
  fn paused_on_second_line(app: &mut App) {
    app.playback.is_playing = false;
    app.playback.position = Duration::from_secs_f64(3.5);
  }

  #[test]
  fn the_panel_opens_and_closes_and_the_karaoke_closes_it() {
    let mut app = with_lyrics();
    assert!(!app.lyrics.tvk_panel);
    send(&mut app, TvkMessage::TogglePanel);
    assert!(app.lyrics.tvk_panel);
    send(&mut app, TvkMessage::TogglePanel);
    assert!(!app.lyrics.tvk_panel);
    send(&mut app, TvkMessage::TogglePanel);
    app.update(Message::Lyrics(LyricsMessage::CloseTvk));
    assert!(!app.lyrics.tvk_panel && !app.lyrics.tvk_open);
  }

  #[test]
  fn the_panel_transitions_in_both_directions_and_asks_for_frames() {
    let mut app = with_lyrics();
    app.lyrics.panel_motion = crate::motion::TvkPanelMotion::launched();
    send(&mut app, TvkMessage::SetPanel(true));
    let opened = app.now;
    assert!(app.lyrics.tvk_panel);
    assert!(app.lyrics.panel_motion.pose(opened).is_some());
    assert!(app.layers_need_frames(), "ouverture en cours");
    // Le survol répété de la zone ne relance rien.
    send(&mut app, TvkMessage::SetPanel(true));
    assert_eq!(app.now, opened);
    assert_eq!(
      app
        .lyrics
        .panel_motion
        .pose(opened + Duration::from_millis(380)),
      None
    );
    assert!(
      !app
        .lyrics
        .panel_motion
        .running(opened + Duration::from_millis(380))
    );
    // Panneau resté ouvert depuis une seconde : la fermeture part de la forme pleine.
    app.lyrics.panel_motion = crate::motion::TvkPanelMotion::launched();
    app
      .lyrics
      .panel_motion
      .set_open(true, Instant::now() - Duration::from_secs(1));
    send(&mut app, TvkMessage::SetPanel(false));
    let closed = app.now;
    assert!(!app.lyrics.tvk_panel);
    assert!(
      app
        .lyrics
        .panel_motion
        .visible(false, closed + Duration::from_millis(379)),
      "`visibility` tient pendant la fermeture"
    );
    assert!(
      !app
        .lyrics
        .panel_motion
        .visible(false, closed + Duration::from_millis(380))
    );
  }

  #[test]
  fn escape_closes_the_panel_before_the_karaoke() {
    let mut app = with_lyrics();
    send(&mut app, TvkMessage::TogglePanel);
    app.update(Message::Escape);
    assert!(!app.lyrics.tvk_panel && app.lyrics.tvk_open);
    app.update(Message::Escape);
    assert!(!app.lyrics.tvk_open);
  }

  #[test]
  fn the_sliders_keep_the_bounds_and_steps_of_riff() {
    let specs = [
      (TvkSlider::Size, (60, 160), 5, 100),
      (TvkSlider::Weight, (400, 900), 100, 700),
      (TvkSlider::Gap, (4, 40), 1, 16),
      (TvkSlider::Speed, (150, 900), 25, 900),
    ];
    assert_eq!(TvkSlider::ALL.len(), specs.len());
    let mut app = with_lyrics();
    for (slider, range, step, default) in specs {
      assert_eq!((slider.range(), slider.step()), (range, step));
      assert_eq!(slider.value(&app.settings.visual.tvk), default);
      send(&mut app, TvkMessage::Slider(slider, i32::MIN));
      assert_eq!(slider.value(&app.settings.visual.tvk), range.0);
      send(&mut app, TvkMessage::Slider(slider, i32::MAX));
      assert_eq!(slider.value(&app.settings.visual.tvk), range.1);
    }
    assert_eq!(TvkSlider::Size.format(130), "130%");
    assert_eq!(TvkSlider::Weight.format(700), "700");
    assert_eq!(TvkSlider::Gap.format(16), "16px");
    assert_eq!(TvkSlider::Speed.format(900), "900ms");
  }

  #[test]
  fn a_slider_applies_at_once_and_is_written_on_release() {
    let mut app = with_lyrics();
    let effects = send(&mut app, TvkMessage::Slider(TvkSlider::Size, 130));
    assert!(!saved(&effects));
    assert_eq!(app.settings.visual.tvk.size, 130);
    let released = app.update(Message::Settings(crate::prefs::SettingsMessage::CommitInt));
    let written = released.iter().find_map(|effect| match effect {
      Effect::SaveSettings(settings) => Some(settings.visual.tvk.size),
      _ => None,
    });
    assert_eq!(written, Some(130));
  }

  #[test]
  fn the_choices_are_written_at_once() {
    let mut app = with_lyrics();
    assert!(saved(&send(&mut app, TvkMessage::Align(TvkAlign::Right))));
    assert!(saved(&send(&mut app, TvkMessage::Anim(TvkAnim::Pop))));
    assert!(saved(&send(&mut app, TvkMessage::Font(TvkFont::Serif))));
    assert!(saved(&send(&mut app, TvkMessage::Visualizer(false))));
    assert!(saved(&send(&mut app, TvkMessage::ToggleCoverMode)));
    let visual = &app.settings.visual;
    assert_eq!(
      (visual.tvk.align, visual.tvk.anim),
      (TvkAlign::Right, TvkAnim::Pop)
    );
    assert_eq!(visual.tvk.font, TvkFont::Serif.stored());
    assert!(!visual.tvk_visualizer && visual.tvk_cover_mode);
  }

  #[test]
  fn reset_restores_the_values_of_riff_and_nothing_else() {
    let mut app = with_lyrics();
    let visual = &mut app.settings.visual;
    visual.tvk = TvkStyle {
      font: TvkFont::Mono.stored().to_string(),
      align: TvkAlign::Left,
      size: 150,
      weight: 400,
      gap: 30,
      anim: TvkAnim::None,
      speed: 200,
      extra: serde_json::from_str(r#"{"future":1}"#).unwrap(),
    };
    visual.tvk_visualizer = false;
    visual.tvk_visualizer_lyrics = false;
    visual.tvk_visualizer_opacity = 30;
    visual.tvk_bg_blur = 50;
    visual.tvk_cover_mode = true;
    app.relayout();
    let effects = send(&mut app, TvkMessage::Reset);
    assert!(saved(&effects));
    let visual = &app.settings.visual;
    assert_eq!(
      TvkStyle {
        extra: serde_json::Map::new(),
        ..visual.tvk.clone()
      },
      TvkStyle::default()
    );
    assert!(visual.tvk.extra.contains_key("future"));
    assert!(visual.tvk_visualizer && visual.tvk_visualizer_lyrics);
    assert_eq!(visual.tvk_visualizer_opacity, 85);
    assert_eq!(visual.tvk_bg_blur, 50, "Riff ne touche pas au flou du fond");
    assert!(visual.tvk_cover_mode, "ni au mode pochette");
    let params = app.lyrics.layout.params.expect("mise en page");
    assert_eq!(params.spacing, line_spacing(&TvkStyle::default()));
    assert_eq!(params.font, crate::lyrics::state::tvk_font(1280.0));
  }

  #[test]
  fn the_layout_follows_the_riff_clamps() {
    // 1920 px écran = 2031 px CSS : 6 vw = 122 donc 96, 5 vw = 101 donc 72, 28 vw = 569 donc 420.
    let geo = geometry(Size::new(1920.0, 1080.0));
    assert!((geo.pad_x - 96.0 * ZOOM).abs() < 1e-3);
    assert!((geo.gap - 72.0 * ZOOM).abs() < 1e-3);
    assert!((geo.side - 420.0 * ZOOM).abs() < 1e-3);
    assert!((geo.pad_y - 40.0 * ZOOM).abs() < 1e-3);
    // Zone des paroles 1347 px CSS, lignes plafonnées à 900.
    assert!((geo.content - (2031.7 - 192.0 - 420.0 - 72.0) * ZOOM).abs() < 1.0);
    assert!((geo.line - 900.0 * ZOOM).abs() < 1e-3);
    let narrow = geometry(Size::new(500.0, 800.0));
    assert!((narrow.side - 200.0 * ZOOM).abs() < 1e-3);
    assert!(narrow.line <= narrow.content);
  }

  #[test]
  fn the_line_spacing_adds_both_margins_and_the_container_gap() {
    let style = TvkStyle::default();
    assert!((line_spacing(&style) - 48.0 * ZOOM).abs() < 1e-3);
  }

  #[test]
  fn the_view_goes_idle_after_three_quiet_seconds_unless_the_panel_is_open() {
    let mut app = app();
    app.update_lyrics(LyricsMessage::OpenTvk);
    app.update_tvk(TvkMessage::IdleCheck);
    assert!(!app.lyrics.tvk_idle, "moins de 3 s");
    app.lyrics.tvk_active = Some(Instant::now() - IDLE_DELAY);
    app.lyrics.tvk_panel = true;
    app.update_tvk(TvkMessage::IdleCheck);
    assert!(!app.lyrics.tvk_idle, "panneau ouvert");
    app.lyrics.tvk_panel = false;
    app.update_tvk(TvkMessage::IdleCheck);
    assert!(app.lyrics.tvk_idle);
    app.update_tvk(TvkMessage::Activity);
    assert!(!app.lyrics.tvk_idle, "un mouvement réveille la vue");
  }

  #[test]
  fn the_panel_reuses_the_existing_visual_controls() {
    let mut app = with_lyrics();
    let visual = |message| Message::Settings(crate::prefs::SettingsMessage::Visual(message));
    app.update(visual(VisualMessage::Bool(
      VisualBool::TvkVisualizerLyrics,
      false,
    )));
    app.update(visual(VisualMessage::Int(VisualInt::TvkBgBlur, 10)));
    assert!(!app.settings.visual.tvk_visualizer_lyrics);
    assert_eq!(app.settings.visual.tvk_bg_blur, 10);
  }

  #[test]
  fn the_visualizer_shows_with_the_setting_that_matches_the_lyrics() {
    let mut app = with_lyrics();
    let cases = [
      // (paroles, visualiseur sans paroles, visualiseur avec paroles, affiché)
      (true, true, true, true),
      (true, true, false, false),
      (true, false, true, true),
      (false, true, false, true),
      (false, false, true, false),
      (false, true, true, true),
      (false, false, false, false),
      (true, false, false, false),
    ];
    for (lyrics, without, with, shown) in cases {
      app.lyrics.lyrics = lyrics.then(|| Lyrics::parse(SYNCED, false));
      app.settings.visual.tvk_visualizer = without;
      app.settings.visual.tvk_visualizer_lyrics = with;
      assert_eq!(
        app.tvk_visualizer_shown(),
        shown,
        "paroles {lyrics}, sans {without}, avec {with}"
      );
    }
    app.lyrics.tvk_open = false;
    app.settings.visual.tvk_visualizer = true;
    app.settings.visual.tvk_visualizer_lyrics = true;
    assert!(!app.tvk_visualizer_shown(), "vue fermée");
  }

  #[test]
  fn the_analyser_and_the_frames_follow_the_same_rule() {
    let mut app = with_lyrics();
    app.playback.is_playing = true;
    app.settings.visual.tvk_visualizer_lyrics = false;
    app.settings.visual.tvk_visualizer = true;
    assert!(
      !app.visualizer_visible(),
      "paroles présentes, barres coupées"
    );
    app.lyrics.lyrics = None;
    assert!(app.visualizer_visible(), "sans paroles, barres allumées");
    app.settings.visual.tvk_visualizer = false;
    assert!(!app.visualizer_visible());
  }

  #[test]
  fn the_cover_mode_hides_the_lyrics_and_centres_the_cover() {
    let mut app = with_lyrics();
    assert!(app.tvk_lyrics_shown());
    send(&mut app, TvkMessage::ToggleCoverMode);
    assert!(!app.tvk_lyrics_shown());
    send(&mut app, TvkMessage::ToggleCoverMode);
    assert!(app.tvk_lyrics_shown());
    assert!((cover_side(400.0) - 260.0 * ZOOM).abs() < 1e-3);
    assert!((cover_side(800.0) - 384.0).abs() < 1e-3);
    assert!((cover_side(2000.0) - 560.0 * ZOOM).abs() < 1e-3);
  }

  #[test]
  fn the_cover_mode_asks_for_the_large_cover() {
    let mut app = with_lyrics();
    app.player.current.as_mut().unwrap().thumbnail = Some("https://exemple.org/p.jpg".into());
    let large = |app: &App| {
      app
        .wanted_images()
        .iter()
        .any(|key| key.size == crate::images::SIZE_HERO)
    };
    assert!(!large(&app));
    app.settings.visual.tvk_cover_mode = true;
    assert!(large(&app));
    app.lyrics.tvk_open = false;
    assert!(!large(&app));
  }

  #[test]
  fn the_inactive_opacity_follows_the_animation() {
    let expected = [
      (TvkAnim::Smooth, 0.45),
      (TvkAnim::Slide, 0.35),
      (TvkAnim::Fade, 0.3),
      (TvkAnim::Pop, 0.5),
      (TvkAnim::None, 0.5),
    ];
    for (anim, opacity) in expected {
      let look = line_look(anim, Easing::Linear, LineRole::Idle, None);
      assert_eq!(look.opacity, opacity, "{anim:?}");
      assert_eq!(
        line_look(anim, Easing::Linear, LineRole::Active, None).opacity,
        1.0
      );
    }
  }

  #[test]
  fn a_slide_moves_the_inactive_line_down_by_24_pixels() {
    let rest = line_look(TvkAnim::Slide, Easing::Linear, LineRole::Idle, None);
    assert_eq!((rest.offset, rest.scale), (24.0, 1.0));
    assert_eq!(
      line_look(TvkAnim::Slide, Easing::Linear, LineRole::Active, None).offset,
      0.0
    );
    for anim in [TvkAnim::Smooth, TvkAnim::Fade, TvkAnim::Pop, TvkAnim::None] {
      assert_eq!(
        line_look(anim, Easing::Linear, LineRole::Idle, None).offset,
        0.0
      );
    }
  }

  #[test]
  fn a_transition_moves_both_lines_between_their_two_looks() {
    let entering = line_look(TvkAnim::Slide, Easing::Linear, LineRole::Active, Some(0.5));
    let leaving = line_look(TvkAnim::Slide, Easing::Linear, LineRole::Leaving, Some(0.5));
    // La ligne qui arrive part de l'état inactif ; l'autre y retourne.
    assert!((entering.opacity - 0.675).abs() < 1e-6 && (entering.offset - 12.0).abs() < 1e-6);
    assert!((leaving.opacity - 0.675).abs() < 1e-6 && (leaving.offset - 12.0).abs() < 1e-6);
    let start = line_look(TvkAnim::Slide, Easing::Linear, LineRole::Active, Some(0.0));
    assert_eq!((start.opacity, start.offset), (0.35, 24.0));
    let end = line_look(TvkAnim::Slide, Easing::Linear, LineRole::Active, Some(1.0));
    assert_eq!((end.opacity, end.offset), (1.0, 0.0));
  }

  #[test]
  fn the_popping_line_grows_then_comes_back() {
    let scale = |progress| {
      line_look(
        TvkAnim::Pop,
        Easing::Linear,
        LineRole::Active,
        Some(progress),
      )
      .scale
    };
    assert_eq!(scale(0.0), 1.0);
    assert!((scale(0.5) - 1.08).abs() < 1e-6);
    assert_eq!(scale(1.0), 1.0);
    // La courbe `cubic-bezier(.2,0,0,1)` s'applique à chaque segment : à 25 % de la durée, la ligne a déjà fait plus de la moitié
    // de la montée (un triangle linéaire donnerait 1,04), et à 75 % elle est déjà redescendue de la même part.
    let eased = bezier_ease(POP_CURVE, 0.5);
    assert!(eased > 0.8, "{eased}");
    assert!((scale(0.25) - (1.0 + 0.08 * eased)).abs() < 1e-6);
    assert!((scale(0.75) - (1.08 - 0.08 * eased)).abs() < 1e-6);
    assert_eq!(
      line_look(TvkAnim::Pop, Easing::Linear, LineRole::Active, None).scale,
      1.0
    );
    let leaving = line_look(TvkAnim::Pop, Easing::Linear, LineRole::Leaving, Some(0.5));
    assert_eq!(leaving.scale, 1.0);
    let smooth = line_look(TvkAnim::Smooth, Easing::Linear, LineRole::Idle, None);
    assert_eq!(
      smooth.scale, 0.92,
      "le CSS de smooth réduit les lignes inactives"
    );
  }

  #[test]
  fn a_transition_only_runs_while_a_line_changes() {
    let mut app = with_lyrics();
    paused_on_second_line(&mut app);
    assert!(!app.animation_needed(), "au repos : aucun minuteur");
    let start = Instant::now();
    tick(&mut app, start);
    assert!(app.lyrics.transition.is_some());
    assert!(app.animation_needed());
    tick(&mut app, start + Duration::from_millis(450));
    let progress = app
      .lyrics
      .transition
      .expect("encore en cours")
      .progress_at(app.now);
    assert!(progress > 0.4 && progress < 0.6, "{progress}");
    let mut at = start + Duration::from_millis(900);
    tick(&mut app, at);
    assert!(app.lyrics.transition.is_none(), "finie après la durée");
    let mut frames = 0;
    while app.animation_needed() {
      at += Duration::from_millis(500);
      tick(&mut app, at);
      frames += 1;
      assert!(frames < 20, "le défilement ne s'arrête pas");
    }
    tick(&mut app, at);
    assert!(app.lyrics.transition.is_none() && !app.animation_needed());
  }

  #[test]
  fn the_cover_mode_needs_no_frames_for_the_hidden_lyrics() {
    let mut app = with_lyrics();
    app.playback.is_playing = true;
    app.settings.visual.tvk_visualizer = false;
    app.settings.visual.tvk_visualizer_lyrics = false;
    assert!(app.animation_needed(), "paroles synchronisées visibles");
    app.settings.visual.tvk_cover_mode = true;
    assert!(!app.animation_needed());
    assert!(!app.main_ticker());
  }

  #[test]
  fn an_open_panel_and_a_paused_karaoke_need_no_frames() {
    let mut app = with_lyrics();
    app.settings.visual.tvk_visualizer = false;
    app.settings.visual.tvk_visualizer_lyrics = false;
    send(&mut app, TvkMessage::TogglePanel);
    send(&mut app, TvkMessage::Slider(TvkSlider::Size, 120));
    send(&mut app, TvkMessage::Anim(TvkAnim::Pop));
    assert!(!app.animation_needed() && !app.main_ticker());
  }

  #[test]
  fn the_karaoke_stays_drawn_while_it_leaves_and_the_frames_stop_afterwards() {
    let mut app = app();
    app.lyrics.lyrics = Some(Lyrics::parse(SYNCED, false));
    app.settings.visual.tvk_visualizer = false;
    app.settings.visual.tvk_visualizer_lyrics = false;
    app.lyrics.tvk_motion = crate::motion::TvkMotion::launched();
    app.update(Message::Lyrics(LyricsMessage::OpenTvk));
    let opened = app.now;
    assert!(app.tvk_shown() && app.animation_needed(), "tvk-in en cours");
    app.now = opened + Duration::from_millis(1000);
    assert!(
      app.animation_needed(),
      "les orbes tournent tant que la vue est ouverte"
    );
    app.update(Message::Lyrics(LyricsMessage::CloseTvk));
    let closed = app.now;
    assert!(
      !app.lyrics.tvk_open,
      "le reste de l'application voit la vue fermée"
    );
    assert!(
      app.tvk_shown() && app.animation_needed(),
      "tvk-out en cours"
    );
    app.now = closed + Duration::from_millis(319);
    assert!(app.tvk_shown());
    app.now = closed + Duration::from_millis(320);
    assert!(
      !app.tvk_shown() && !app.animation_needed(),
      "`.hidden` : plus rien à animer"
    );
  }

  #[test]
  fn opening_twice_keeps_the_first_entrance() {
    let mut app = app();
    app.lyrics.tvk_motion = crate::motion::TvkMotion::launched();
    app.update(Message::Lyrics(LyricsMessage::OpenTvk));
    let first = app.now;
    app.update(Message::Lyrics(LyricsMessage::OpenTvk));
    assert_eq!(app.lyrics.tvk_motion.orbit_seconds(first), 0.0);
    assert!(app.lyrics.tvk_motion.pose(first).is_some());
  }

  #[test]
  fn the_duration_of_a_transition_is_the_speed_setting() {
    let mut app = with_lyrics();
    app.settings.visual.tvk.speed = 300;
    paused_on_second_line(&mut app);
    let start = Instant::now();
    tick(&mut app, start);
    assert_eq!(
      app.lyrics.transition.map(|transition| transition.duration),
      Some(Duration::from_millis(300))
    );
    tick(&mut app, start + Duration::from_millis(300));
    assert!(app.lyrics.transition.is_none());
  }

  #[test]
  fn the_none_animation_and_the_other_surfaces_never_start_a_transition() {
    let mut app = with_lyrics();
    app.settings.visual.tvk.anim = TvkAnim::None;
    paused_on_second_line(&mut app);
    tick(&mut app, Instant::now());
    assert!(app.lyrics.transition.is_none());

    // Ni le panneau fermé ni les mini-paroles n'ont de transition ici.
    let mut hidden = with_lyrics();
    hidden.lyrics.tvk_open = false;
    hidden.lyrics.panel_open = false;
    hidden.relayout();
    paused_on_second_line(&mut hidden);
    tick(&mut hidden, Instant::now());
    assert!(hidden.lyrics.transition.is_none());
  }

  #[test]
  fn the_panel_transition_lasts_the_lyric_duration_and_follows_the_ease() {
    let mut panel = with_lyrics();
    panel.lyrics.tvk_open = false;
    panel.lyrics.panel_open = true;
    panel.relayout();
    paused_on_second_line(&mut panel);
    let start = Instant::now();
    tick(&mut panel, start);
    assert_eq!(
      panel
        .lyrics
        .transition
        .map(|transition| transition.duration),
      Some(Duration::from_millis(480)),
      "`--lyric-duration` de inout"
    );
    assert_eq!(
      panel.panel_look(1).opacity,
      0.48,
      "départ de la ligne qui s'allume"
    );
    assert_eq!(panel.panel_look(0).opacity, 0.48);
    tick(&mut panel, start + Duration::from_millis(240));
    let expected = 0.48 + 0.52 * Easing::InOut.apply_css(0.5);
    assert!((panel.panel_look(1).opacity - expected).abs() < 1e-4);
    assert!(
      panel.panel_look(1).opacity > 0.9,
      "inout est déjà à 88 % à mi-course"
    );
    panel.settings.visual.lyrics_ease = "linear".to_string();
    assert!(
      (panel.panel_look(1).opacity - 0.74).abs() < 1e-4,
      "linéaire : 240 ms sur 480 ms"
    );
    tick(&mut panel, start + Duration::from_millis(480));
    assert!(panel.lyrics.transition.is_none());
    assert_eq!(panel.panel_look(1).opacity, 1.0);
    assert_eq!(panel.panel_look(0).opacity, 0.48);
    assert!(
      panel.lyrics.animation.is_none() && !panel.animation_needed(),
      "l'abonnement s'arrête"
    );
  }

  #[test]
  fn the_panel_leaving_line_fades_out_while_the_new_one_lights() {
    let mut panel = with_lyrics();
    panel.lyrics.tvk_open = false;
    panel.lyrics.panel_open = true;
    panel.relayout();
    panel.playback.is_playing = false;
    panel.playback.position = Duration::from_secs_f64(1.5);
    let start = Instant::now();
    tick(&mut panel, start);
    assert_eq!(panel.lyrics.active, Some(0));
    panel.playback.position = Duration::from_secs_f64(3.5);
    tick(&mut panel, start + Duration::from_secs(1));
    assert_eq!(panel.lyrics.active, Some(1));
    let half = start + Duration::from_millis(1240);
    tick(&mut panel, half);
    let (entering, leaving) = (panel.panel_look(1).opacity, panel.panel_look(0).opacity);
    assert!(
      (entering + leaving - 1.48).abs() < 1e-4,
      "{entering} {leaving}"
    );
    assert!(entering > leaving);
  }

  #[test]
  fn the_look_of_a_line_comes_from_the_running_transition() {
    let mut app = with_lyrics();
    paused_on_second_line(&mut app);
    let rest = app.tvk_look(0);
    assert_eq!(rest.opacity, 0.35, "pas encore de ligne active");
    let start = Instant::now();
    tick(&mut app, start);
    assert_eq!(app.lyrics.active, Some(1));
    assert_eq!(app.tvk_look(1).opacity, 0.35, "au début de la transition");
    tick(&mut app, start + Duration::from_millis(900));
    assert_eq!(app.tvk_look(1).opacity, 1.0);
    assert_eq!(app.tvk_look(0).opacity, 0.35);
    assert_eq!(app.tvk_look(2).opacity, 0.35);
  }

  #[test]
  fn size_weight_gap_and_font_change_the_layout_and_alignment_does_not() {
    let mut app = with_lyrics();
    let base = app.lyrics.layout.clone();
    let base_params = base.params.expect("mise en page");
    assert_eq!(
      base_params.spacing,
      line_spacing(&TvkStyle::default()),
      "le défaut garde l'espacement actuel"
    );
    assert_eq!(base_params.face.weight, Weight::Bold);
    send(&mut app, TvkMessage::Slider(TvkSlider::Size, 150));
    let params = app.lyrics.layout.params.unwrap();
    assert_eq!(params.font, base_params.font * 1.5);
    send(&mut app, TvkMessage::Slider(TvkSlider::Gap, 30));
    assert!((app.lyrics.layout.params.unwrap().spacing - 76.0 * ZOOM).abs() < 1e-3);
    send(&mut app, TvkMessage::Slider(TvkSlider::Weight, 400));
    assert_eq!(
      app.lyrics.layout.params.unwrap().face.weight,
      Weight::Normal
    );
    let gap = app.lyrics.layout.boxes[1].top - app.lyrics.layout.boxes[0].top;
    assert!(gap > base.boxes[1].top - base.boxes[0].top);
    let before = app.lyrics.layout.clone();
    send(&mut app, TvkMessage::Align(TvkAlign::Left));
    send(&mut app, TvkMessage::Anim(TvkAnim::Fade));
    assert_eq!(app.lyrics.layout, before);
  }

  #[test]
  fn the_panel_surface_keeps_its_own_layout() {
    let mut app = with_lyrics();
    app.lyrics.tvk_open = false;
    app.settings.visual.tvk.size = 160;
    app.relayout();
    let params = app.lyrics.layout.params.unwrap();
    // Pixels CSS de Riff convertis en pixels écran : la taille choisie pour le plein écran ne passe pas.
    assert_eq!(
      params.font,
      crate::theme::tokens::css(crate::lyrics::state::PANEL_FONT)
    );
    assert_eq!(
      params.spacing,
      crate::theme::tokens::css(crate::lyrics::state::LINE_SPACING)
    );
  }

  #[test]
  fn the_weights_map_to_the_nine_steps_of_iced() {
    let expected = [
      (100, Weight::Thin),
      (200, Weight::ExtraLight),
      (300, Weight::Light),
      (400, Weight::Normal),
      (500, Weight::Medium),
      (600, Weight::Semibold),
      (700, Weight::Bold),
      (800, Weight::ExtraBold),
      (900, Weight::Black),
      (650, Weight::Bold),
      (999, Weight::Black),
    ];
    for (css, weight) in expected {
      assert_eq!(weight_of(css), weight, "{css}");
    }
  }

  #[test]
  fn a_stored_font_maps_to_its_choice_through_the_aliases_of_riff() {
    assert_eq!(TvkFont::from_stored("inherit"), TvkFont::Default);
    assert_eq!(TvkFont::from_stored("default"), TvkFont::Default);
    assert_eq!(TvkFont::from_stored("sf"), TvkFont::SfPro);
    assert_eq!(TvkFont::from_stored("SF Pro"), TvkFont::SfPro);
    assert_eq!(TvkFont::from_stored("serif"), TvkFont::Serif);
    assert_eq!(TvkFont::from_stored("rounded"), TvkFont::Rounded);
    assert_eq!(TvkFont::from_stored("mono"), TvkFont::Mono);
    assert_eq!(TvkFont::from_stored("condensed"), TvkFont::Condensed);
    assert_eq!(TvkFont::from_stored("Comic Sans"), TvkFont::Default);
    for font in TvkFont::ALL {
      assert_eq!(TvkFont::from_stored(font.stored()), font, "{font:?}");
    }
    assert_eq!(TvkFont::Default.stored(), "inherit");
    assert_eq!(TvkFont::Serif.stored(), "Georgia, 'Times New Roman', serif");
  }

  #[test]
  fn only_the_fonts_that_resolve_are_listed() {
    let none = |_: &str| false;
    assert_eq!(
      available_fonts(&none),
      vec![(TvkFont::Default, DEFAULT_FAMILY)]
    );
    let some = |name: &str| ["Georgia", "Consolas"].contains(&name);
    assert_eq!(
      available_fonts(&some),
      vec![
        (TvkFont::Default, DEFAULT_FAMILY),
        (TvkFont::Serif, "Georgia"),
        (TvkFont::Mono, "Consolas"),
      ]
    );
    let stack = |name: &str| ["Times New Roman", "JetBrains Mono", "Consolas"].contains(&name);
    let listed = available_fonts(&stack);
    assert!(
      listed.contains(&(TvkFont::Serif, "Times New Roman")),
      "pile CSS : premier présent"
    );
    assert!(listed.contains(&(TvkFont::Mono, "JetBrains Mono")));
  }

  #[test]
  fn a_font_that_is_not_listed_renders_as_the_default() {
    let list = vec![
      (TvkFont::Default, DEFAULT_FAMILY),
      (TvkFont::Serif, "Georgia"),
    ];
    assert_eq!(family_in(&list, TvkFont::Serif), "Georgia");
    assert_eq!(family_in(&list, TvkFont::Mono), DEFAULT_FAMILY);
    assert_eq!(family_in(&list, TvkFont::Default), DEFAULT_FAMILY);
  }
}

#[cfg(test)]
mod font_system_tests {
  use super::*;

  #[test]
  fn the_list_read_from_the_font_system_starts_with_the_default() {
    let list = fonts();
    assert_eq!(list[0], (TvkFont::Default, DEFAULT_FAMILY));
    let style = TvkStyle {
      font: TvkFont::Serif.stored().to_string(),
      ..TvkStyle::default()
    };
    let expected = family_in(list, TvkFont::Serif);
    assert_eq!(face(&style).family, Family::Name(expected));
  }
}
