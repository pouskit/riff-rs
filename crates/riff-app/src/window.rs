//! Barre de titre, poignées et `window-state.json`.

use crate::theme::icons;
use crate::widgets::button::icon;
use iced::widget::{column, container, mouse_area, row, space, stack, text};
use iced::window::{self, Direction};
use iced::{Element, Fill, Length, Point, Size, mouse};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const MIN_WIDTH: u32 = 800;
pub const MIN_HEIGHT: u32 = 540;
const GRIP: f32 = 6.0;
pub const BAR_HEIGHT: f32 = 36.0;
pub const BAR_BUTTON_WIDTH: f32 = 32.0;
/// Position donnée par Windows à une fenêtre réduite.
pub const MINIMIZED_POSITION: f32 = -30000.0;
/// Mode fenêtre compacte (`toggle-mini-window` de Riff) : taille à l'entrée et minimum.
pub const MINI_SIZE: Size = Size::new(380.0, 160.0);
pub const MINI_MIN: Size = Size::new(280.0, 115.0);
/// Mini-paroles (`set-mini-lyrics`, `main.js:1274-1297`) : minimum et hauteur minimale avec les paroles.
pub const MINI_LYRICS_MIN: Size = Size::new(320.0, 240.0);
pub const MINI_LYRICS_HEIGHT: f32 = 360.0;
/// Paroles retirées : minimum de `main.js:1291` et hauteur de repli quand l'ancienne n'est pas connue.
pub const MINI_PLAIN_MIN: Size = Size::new(320.0, 140.0);
pub const MINI_PLAIN_HEIGHT: f32 = 160.0;
const _: () = assert!(
  MINI_SIZE.width >= MINI_MIN.width
    && MINI_SIZE.height >= MINI_MIN.height
    && MINI_MIN.width < MIN_WIDTH as f32
    && MINI_MIN.height < MIN_HEIGHT as f32
);

/// Bornes de la fenêtre en pixels physiques, unité du fichier écrit par Riff (Tauri).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBounds {
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

impl Default for WindowBounds {
  fn default() -> Self {
    Self {
      x: 100,
      y: 80,
      width: 1280,
      height: 800,
    }
  }
}

impl WindowBounds {
  #[must_use]
  pub fn from_logical(position: Point, size: Size, scale: f32) -> Self {
    Self {
      x: (position.x * scale).round() as i32,
      y: (position.y * scale).round() as i32,
      width: ((size.width * scale).round() as u32).max(MIN_WIDTH),
      height: ((size.height * scale).round() as u32).max(MIN_HEIGHT),
    }
  }

  #[must_use]
  pub fn to_logical(self, scale: f32) -> (Point, Size) {
    (
      Point::new(self.x as f32 / scale, self.y as f32 / scale),
      Size::new(self.width as f32 / scale, self.height as f32 / scale),
    )
  }
}

/// Contenu de `window-state.json`, compatible avec Riff (Tauri) dans les deux sens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PersistedWindowState {
  pub bounds: WindowBounds,
  #[serde(rename = "isMaximized", default)]
  pub is_maximized: bool,
  #[serde(rename = "isFullScreen", default)]
  pub is_full_screen: bool,
}

impl PersistedWindowState {
  /// Lit le fichier. Absent ou corrompu : `None`.
  #[must_use]
  pub fn read(path: &Path) -> Option<Self> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
  }

  /// Écrit le fichier sans jamais laisser un fichier à moitié écrit.
  ///
  /// # Errors
  /// Erreur du système de fichiers.
  pub fn write(&self, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
    fs::rename(&tmp, path)
  }

  /// Réglages d'ouverture. L'échelle n'est connue qu'une fois la fenêtre créée : voir [`WindowTracker::rescale_for_restore`].
  #[must_use]
  pub fn settings(&self) -> window::Settings {
    let (position, size) = self.bounds.to_logical(1.0);
    window::Settings {
      size,
      position: window::Position::Specific(position),
      maximized: self.is_maximized,
      fullscreen: self.is_full_screen,
      min_size: Some(Size::new(MIN_WIDTH as f32, MIN_HEIGHT as f32)),
      decorations: false,
      exit_on_close_request: false,
      #[cfg(windows)]
      platform_specific: window::settings::PlatformSpecific {
        undecorated_shadow: true,
        ..Default::default()
      },
      ..window::Settings::default()
    }
  }
}

/// États d'une fenêtre créée cachée, rendus à sa première apparition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeferredMode {
  pub maximized: bool,
  pub fullscreen: bool,
  /// La fenêtre de mini-paroles voulue par les réglages, pas encore ouverte.
  pub mini_lyrics: bool,
}

/// Réglages d'une fenêtre qui naît cachée (lancement `--tray`). Agrandie ou en plein écran dès la création, Windows
/// la montrerait un instant : ces deux états sont mis de côté et rendus par [`DeferredMode`] à la première apparition.
pub fn hide_at_creation(settings: &mut window::Settings) -> DeferredMode {
  settings.visible = false;
  DeferredMode {
    maximized: std::mem::take(&mut settings.maximized),
    fullscreen: std::mem::take(&mut settings.fullscreen),
    mini_lyrics: false,
  }
}

/// Suit la fenêtre : garde les dernières bornes de la fenêtre ni agrandie ni réduite.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowTracker {
  pub saved: PersistedWindowState,
  pub scale: Option<f32>,
  pub position: Option<Point>,
  pub size: Option<Size>,
  /// Dernière taille non nulle : Windows rapporte 0x0 à la réduction, la mise en page ne doit pas en dépendre.
  pub visible_size: Option<Size>,
  pub is_maximized: bool,
  pub main_hidden: bool,
  /// Bornes d'avant le mode compact ; `Some` tant que la fenêtre est compacte.
  pub mini: Option<PreMini>,
  /// `body.mini-lyrics-on` : les paroles sont affichées dans la fenêtre compacte.
  pub mini_lyrics: bool,
  /// `preMiniLyricsH` : hauteur d'avant l'ouverture des paroles.
  pub mini_lyrics_restore: Option<f32>,
}

/// Ce que le mode compact doit rétablir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreMini {
  pub bounds: WindowBounds,
  pub is_maximized: bool,
}

impl WindowTracker {
  #[must_use]
  pub fn new(saved: PersistedWindowState) -> Self {
    Self {
      saved,
      is_maximized: saved.is_maximized,
      ..Self::default()
    }
  }

  pub fn moved(&mut self, position: Point) {
    self.position = Some(position);
  }

  pub fn resized(&mut self, size: Size) {
    self.size = Some(size);
    if size.width > 0.0 && size.height > 0.0 {
      self.visible_size = Some(size);
    }
  }

  /// Taille qui décide de la mise en page : la taille reçue, sauf 0x0 (fenêtre réduite) où la dernière taille visible reste.
  #[must_use]
  pub fn layout_size(&self) -> Option<Size> {
    self
      .size
      .filter(|size| size.width > 0.0 && size.height > 0.0)
      .or(self.visible_size)
  }

  fn current_bounds(&self) -> Option<WindowBounds> {
    let (position, size, scale) = (self.position?, self.size?, self.scale?);
    let visible = position.x > MINIMIZED_POSITION && size.width > 0.0 && size.height > 0.0;
    visible.then(|| WindowBounds::from_logical(position, size, scale))
  }

  /// Réponse à la question « la fenêtre est-elle agrandie ? » posée après un déplacement ou un redimensionnement.
  pub fn maximized_state(&mut self, is_maximized: bool) {
    if self.mini.is_some() {
      return;
    }
    self.is_maximized = is_maximized;
    if !is_maximized && let Some(bounds) = self.current_bounds() {
      self.saved.bounds = bounds;
    }
  }

  /// État à écrire à la fermeture.
  #[must_use]
  pub fn closing_state(&self, is_maximized: bool, is_full_screen: bool) -> PersistedWindowState {
    if let Some(before) = self.mini {
      return PersistedWindowState {
        bounds: before.bounds,
        is_maximized: before.is_maximized,
        is_full_screen: false,
      };
    }
    let bounds = if is_maximized || is_full_screen {
      None
    } else {
      self.current_bounds()
    };
    PersistedWindowState {
      bounds: bounds.unwrap_or(self.saved.bounds),
      is_maximized,
      is_full_screen,
    }
  }

  /// Passe en mode compact en gardant les bornes actuelles (celles d'avant l'agrandissement si la fenêtre est agrandie).
  /// Faux si le mode est déjà actif.
  pub fn enter_mini(&mut self) -> bool {
    if self.mini.is_some() {
      return false;
    }
    let bounds = if self.is_maximized {
      self.saved.bounds
    } else {
      self.current_bounds().unwrap_or(self.saved.bounds)
    };
    self.mini = Some(PreMini {
      bounds,
      is_maximized: self.is_maximized,
    });
    true
  }

  /// Quitte le mode compact. Rend position et taille logiques à rétablir, et si la fenêtre était agrandie.
  pub fn leave_mini(&mut self) -> Option<(Point, Size, bool)> {
    let before = self.mini.take()?;
    self.mini_lyrics_restore = None;
    let (position, size) = before.bounds.to_logical(self.scale.unwrap_or(1.0));
    self.saved.bounds = before.bounds;
    self.is_maximized = before.is_maximized;
    Some((position, size, before.is_maximized))
  }

  /// Affiche ou masque les paroles du mode compact. Rend la taille minimale et la taille à appliquer (`set-mini-lyrics`).
  /// `None` hors du mode compact.
  pub fn toggle_mini_lyrics(&mut self) -> Option<(Size, Size)> {
    self.mini?;
    self.mini_lyrics = !self.mini_lyrics;
    Some(self.mini_lyrics_bounds(self.size.unwrap_or(MINI_SIZE)))
  }

  /// Taille du mode compact selon l'état des paroles, depuis la taille `size` de la fenêtre : Riff garde la largeur,
  /// et rend la hauteur d'avant les paroles.
  pub fn mini_lyrics_bounds(&mut self, size: Size) -> (Size, Size) {
    if self.mini_lyrics {
      self.mini_lyrics_restore.get_or_insert(size.height);
      let height = size.height.max(MINI_LYRICS_HEIGHT);
      (MINI_LYRICS_MIN, Size::new(size.width, height))
    } else {
      let height = self.mini_lyrics_restore.take().unwrap_or(MINI_PLAIN_HEIGHT);
      (MINI_PLAIN_MIN, Size::new(size.width, height))
    }
  }

  /// Taille et position logiques à appliquer une fois l'échelle connue, si l'échelle n'est pas 1.
  pub fn rescale_for_restore(&mut self, scale: f32) -> Option<(Point, Size)> {
    self.scale = Some(scale);
    let restored = !self.saved.is_maximized && !self.saved.is_full_screen;
    (restored && (scale - 1.0).abs() > 0.001).then(|| self.saved.bounds.to_logical(scale))
  }
}

/// Actions de la barre de titre et des poignées.
#[derive(Debug, Clone, Copy)]
pub enum Action {
  Drag,
  Resize(Direction),
  ToggleMaximize,
  Minimize,
  Close,
}

impl PartialEq for Action {
  fn eq(&self, other: &Self) -> bool {
    match (self, other) {
      (Self::Resize(a), Self::Resize(b)) => std::mem::discriminant(a) == std::mem::discriminant(b),
      _ => std::mem::discriminant(self) == std::mem::discriminant(other),
    }
  }
}

fn grip<'a>(
  direction: Direction,
  width: Length,
  height: Length,
  cursor: mouse::Interaction,
) -> Element<'a, Action> {
  mouse_area(space().width(width).height(height))
    .on_press(Action::Resize(direction))
    .interaction(cursor)
    .into()
}

/// Les 8 poignées de redimensionnement, à poser au-dessus du contenu.
#[must_use]
pub fn grips<'a>() -> Element<'a, Action> {
  use mouse::Interaction::{
    ResizingDiagonallyDown as DiagDown, ResizingDiagonallyUp as DiagUp,
    ResizingHorizontally as Horizontal, ResizingVertically as Vertical,
  };
  let fixed = Length::Fixed(GRIP);
  column![
    row![
      grip(Direction::NorthWest, fixed, fixed, DiagDown),
      grip(Direction::North, Fill, fixed, Vertical),
      grip(Direction::NorthEast, fixed, fixed, DiagUp),
    ],
    row![
      grip(Direction::West, fixed, Fill, Horizontal),
      space().width(Fill).height(Fill),
      grip(Direction::East, fixed, Fill, Horizontal),
    ]
    .height(Fill),
    row![
      grip(Direction::SouthWest, fixed, fixed, DiagUp),
      grip(Direction::South, Fill, fixed, Vertical),
      grip(Direction::SouthEast, fixed, fixed, DiagDown),
    ],
  ]
  .into()
}

fn bar_button<'a>(glyph: &'a str, action: Action) -> Element<'a, Action> {
  icon(glyph, Some(action), BAR_BUTTON_WIDTH, BAR_HEIGHT)
}

/// Zone du titre de la barre : elle déplace la fenêtre.
#[must_use]
pub fn title_zone<'a>(title: &'a str) -> Element<'a, Action> {
  mouse_area(
    container(text(title).size(13))
      .padding([10, 12])
      .width(Fill)
      .height(BAR_HEIGHT),
  )
  .on_press(Action::Drag)
  .on_double_click(Action::ToggleMaximize)
  .into()
}

/// Zone vide qui déplace la fenêtre.
#[must_use]
pub fn drag_zone<'a>() -> Element<'a, Action> {
  mouse_area(container(space()).width(Fill).height(BAR_HEIGHT))
    .on_press(Action::Drag)
    .on_double_click(Action::ToggleMaximize)
    .into()
}

/// Boutons réduire, agrandir, fermer.
#[must_use]
pub fn window_buttons<'a>(is_maximized: bool) -> Element<'a, Action> {
  let maximize = if is_maximized {
    icons::RESTORE
  } else {
    icons::MAXIMIZE
  };
  row![
    bar_button(icons::MINIMIZE, Action::Minimize),
    bar_button(maximize, Action::ToggleMaximize),
    bar_button(icons::WIN_CLOSE, Action::Close),
  ]
  .height(BAR_HEIGHT)
  .into()
}

/// Fenêtre complète : contenu, et poignées seulement si la fenêtre n'est pas agrandie.
#[must_use]
pub fn frame<'a, Message: 'a>(
  content: Element<'a, Message>,
  is_maximized: bool,
  on_action: fn(Action) -> Message,
) -> Element<'a, Message> {
  if is_maximized {
    content
  } else {
    stack![content, grips().map(on_action)]
      .width(Fill)
      .height(Fill)
      .into()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp_path(name: &str) -> (crate::test_temp::TempDir, std::path::PathBuf) {
    let dir = crate::test_temp::TempDir::new("app-window");
    let path = dir.join(name);
    (dir, path)
  }

  #[test]
  fn a_window_born_hidden_defers_its_maximized_and_fullscreen_states() {
    let mut settings = PersistedWindowState {
      is_maximized: true,
      ..PersistedWindowState::default()
    }
    .settings();
    assert!(settings.visible && settings.maximized);
    let deferred = hide_at_creation(&mut settings);
    assert!(!settings.visible && !settings.maximized && !settings.fullscreen);
    assert_eq!(
      deferred,
      DeferredMode {
        maximized: true,
        fullscreen: false,
        mini_lyrics: false
      }
    );
  }

  #[test]
  fn bounds_round_trip_through_logical_pixels() {
    let bounds = WindowBounds {
      x: -1913,
      y: 37,
      width: 1537,
      height: 863,
    };
    for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
      let (position, size) = bounds.to_logical(scale);
      assert_eq!(
        WindowBounds::from_logical(position, size, scale),
        bounds,
        "échelle {scale}"
      );
    }
  }

  #[test]
  fn logical_pixels_are_physical_pixels_divided_by_the_scale() {
    let (position, size) = WindowBounds {
      x: 300,
      y: 150,
      width: 1500,
      height: 900,
    }
    .to_logical(1.5);
    assert_eq!(position, Point::new(200.0, 100.0));
    assert_eq!(size, Size::new(1000.0, 600.0));
  }

  #[test]
  fn from_logical_applies_the_minimum_size_like_riff() {
    let bounds = WindowBounds::from_logical(Point::new(0.0, 0.0), Size::new(100.0, 100.0), 1.0);
    assert_eq!((bounds.width, bounds.height), (MIN_WIDTH, MIN_HEIGHT));
  }

  #[test]
  fn reads_the_file_written_by_riff() {
    let (_dir, path) = temp_path("tauri.json");
    let raw = "{\n  \"bounds\": {\n    \"x\": 12,\n    \"y\": -4,\n    \"width\": 1400,\n    \"height\": 900\n  },\n  \"isMaximized\": true,\n  \"isFullScreen\": false\n}";
    std::fs::write(&path, raw).unwrap();
    let state = PersistedWindowState::read(&path).expect("état");
    assert_eq!(
      state.bounds,
      WindowBounds {
        x: 12,
        y: -4,
        width: 1400,
        height: 900
      }
    );
    assert!(state.is_maximized);
    assert!(!state.is_full_screen);
  }

  #[test]
  fn writes_the_field_names_of_riff_and_reads_them_back() {
    let (_dir, path) = temp_path("native.json");
    let state = PersistedWindowState {
      bounds: WindowBounds {
        x: 1,
        y: 2,
        width: 900,
        height: 600,
      },
      is_maximized: true,
      is_full_screen: false,
    };
    state.write(&path).unwrap();
    let raw: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw["isMaximized"], serde_json::json!(true));
    assert_eq!(raw["bounds"]["width"], serde_json::json!(900));
    assert_eq!(PersistedWindowState::read(&path), Some(state));
    assert!(!path.with_extension("json.tmp").exists());
  }

  #[test]
  fn corrupt_or_missing_file_gives_none() {
    let (_dir, path) = temp_path("corrupt.json");
    std::fs::write(&path, "{ pas du json").unwrap();
    assert_eq!(PersistedWindowState::read(&path), None);
    assert_eq!(
      PersistedWindowState::read(&temp_path("absent.json").1),
      None
    );
  }

  fn tracker() -> WindowTracker {
    let mut tracker = WindowTracker::new(PersistedWindowState::default());
    tracker.rescale_for_restore(1.0);
    tracker
  }

  #[test]
  fn tracker_keeps_the_bounds_of_a_normal_window() {
    let mut tracker = tracker();
    tracker.moved(Point::new(50.0, 60.0));
    tracker.resized(Size::new(1000.0, 700.0));
    tracker.maximized_state(false);
    assert_eq!(
      tracker.closing_state(false, false).bounds,
      WindowBounds {
        x: 50,
        y: 60,
        width: 1000,
        height: 700
      }
    );
  }

  #[test]
  fn tracker_keeps_the_previous_bounds_when_maximized() {
    let mut tracker = tracker();
    tracker.moved(Point::new(50.0, 60.0));
    tracker.resized(Size::new(1000.0, 700.0));
    tracker.maximized_state(false);
    tracker.moved(Point::new(0.0, 0.0));
    tracker.resized(Size::new(2560.0, 1400.0));
    tracker.maximized_state(true);
    let state = tracker.closing_state(true, false);
    assert!(state.is_maximized);
    assert_eq!(
      state.bounds,
      WindowBounds {
        x: 50,
        y: 60,
        width: 1000,
        height: 700
      }
    );
  }

  #[test]
  fn tracker_ignores_a_minimized_window() {
    let mut tracker = tracker();
    tracker.moved(Point::new(50.0, 60.0));
    tracker.resized(Size::new(1000.0, 700.0));
    tracker.maximized_state(false);
    tracker.moved(Point::new(-32000.0, -32000.0));
    tracker.resized(Size::new(0.0, 0.0));
    tracker.maximized_state(false);
    assert_eq!(
      tracker.closing_state(false, false).bounds,
      WindowBounds {
        x: 50,
        y: 60,
        width: 1000,
        height: 700
      }
    );
  }

  #[test]
  fn tracker_uses_the_scale_for_the_saved_bounds() {
    let mut tracker = WindowTracker::new(PersistedWindowState::default());
    tracker.rescale_for_restore(1.5);
    tracker.moved(Point::new(100.0, 100.0));
    tracker.resized(Size::new(1000.0, 600.0));
    tracker.maximized_state(false);
    assert_eq!(
      tracker.closing_state(false, false).bounds,
      WindowBounds {
        x: 150,
        y: 150,
        width: 1500,
        height: 900
      }
    );
  }

  #[test]
  fn restore_is_only_needed_when_the_scale_is_not_one() {
    let saved = PersistedWindowState {
      bounds: WindowBounds {
        x: 300,
        y: 150,
        width: 1500,
        height: 900,
      },
      ..Default::default()
    };
    assert_eq!(WindowTracker::new(saved).rescale_for_restore(1.0), None);
    assert_eq!(
      WindowTracker::new(saved).rescale_for_restore(1.5),
      Some((Point::new(200.0, 100.0), Size::new(1000.0, 600.0)))
    );
  }

  #[test]
  fn restore_does_not_move_a_maximized_window() {
    let saved = PersistedWindowState {
      is_maximized: true,
      ..Default::default()
    };
    assert_eq!(WindowTracker::new(saved).rescale_for_restore(2.0), None);
  }

  fn in_normal_window(tracker: &mut WindowTracker) {
    tracker.moved(Point::new(50.0, 60.0));
    tracker.resized(Size::new(1000.0, 700.0));
    tracker.maximized_state(false);
  }

  #[test]
  fn entering_the_compact_mode_remembers_the_normal_bounds_and_restores_them() {
    let mut tracker = tracker();
    in_normal_window(&mut tracker);
    assert!(tracker.enter_mini());
    assert!(!tracker.enter_mini(), "déjà en mode compact");
    tracker.moved(Point::new(1500.0, 900.0));
    tracker.resized(MINI_SIZE);
    tracker.maximized_state(false);
    let (position, size, maximized) = tracker.leave_mini().expect("mode compact");
    assert_eq!(
      (position, size, maximized),
      (Point::new(50.0, 60.0), Size::new(1000.0, 700.0), false)
    );
    assert!(tracker.leave_mini().is_none());
  }

  #[test]
  fn the_compact_window_never_overwrites_the_saved_window_state() {
    let mut tracker = tracker();
    in_normal_window(&mut tracker);
    tracker.enter_mini();
    tracker.moved(Point::new(1500.0, 900.0));
    tracker.resized(MINI_SIZE);
    tracker.maximized_state(false);
    let state = tracker.closing_state(false, false);
    assert_eq!(
      state.bounds,
      WindowBounds {
        x: 50,
        y: 60,
        width: 1000,
        height: 700
      }
    );
    assert!(!state.is_maximized);
  }

  #[test]
  fn a_maximized_window_comes_back_maximized_with_its_last_normal_bounds() {
    let mut tracker = tracker();
    in_normal_window(&mut tracker);
    tracker.moved(Point::new(0.0, 0.0));
    tracker.resized(Size::new(2560.0, 1400.0));
    tracker.maximized_state(true);
    tracker.enter_mini();
    let (position, size, maximized) = tracker.leave_mini().expect("mode compact");
    assert_eq!(
      (position, size, maximized),
      (Point::new(50.0, 60.0), Size::new(1000.0, 700.0), true)
    );
    assert!(tracker.closing_state(true, false).is_maximized);
  }
}
