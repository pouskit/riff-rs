//! Thèmes Material Design 3.

pub mod contrast;
pub mod cover;
pub mod file;
pub mod fonts;
pub mod icons;
pub mod scheme;
pub mod tokens;

use crate::settings::{NativeSettings, ThemeMode};
use file::LoadedTheme;
use iced::Theme;
use material_colors::color::Rgb;
use material_colors::scheme::Scheme;

/// Origine du schéma actif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSource {
  /// Couleur d'accent des réglages, `#5247D1` par défaut.
  Accent,
  /// Couleur dominante de la pochette du titre en cours.
  Cover(Rgb),
  /// Fichier de thème natif, par identifiant.
  File(String),
}

fn style_of(settings: &NativeSettings) -> scheme::ThemeStyle {
  scheme::ThemeStyle {
    brightness: settings.theme_brightness.min(100),
    intensity: settings.theme_intensity.min(100),
    palette: scheme::PaletteStyle::parse(&settings.palette_style),
    intensity_bg: settings.intensity_bg,
    highlight_buttons: settings.highlight_buttons,
  }
}

/// Thème appliqué à l'interface.
#[derive(Debug, Clone)]
pub struct ActiveTheme {
  pub source: ThemeSource,
  pub scheme: Scheme,
  pub dark: bool,
  pub theme: Theme,
}

impl ActiveTheme {
  fn new(source: ThemeSource, name: String, scheme: Scheme, dark: bool) -> Self {
    Self::with_highlight(source, name, scheme, dark, true)
  }

  fn with_highlight(
    source: ThemeSource,
    name: String,
    scheme: Scheme,
    dark: bool,
    highlight: bool,
  ) -> Self {
    let theme = scheme::to_iced_theme_with(name, &scheme, dark, highlight);
    Self {
      source,
      scheme,
      dark,
      theme,
    }
  }

  /// Thème de la couleur d'accent des réglages.
  #[must_use]
  pub fn from_accent(settings: &NativeSettings) -> Self {
    let dark = settings.theme_mode != ThemeMode::Light;
    let accent = settings
      .theme_accent
      .parse()
      .unwrap_or(scheme::DEFAULT_SOURCE);
    let style = style_of(settings);
    Self::with_highlight(
      ThemeSource::Accent,
      "Riff".to_string(),
      scheme::styled(accent, dark, style),
      dark,
      style.highlight_buttons,
    )
  }

  /// Thème de la pochette, sur une base sombre comme « Auto (album) » dans Riff.
  #[must_use]
  pub fn from_cover(source: Rgb, settings: &NativeSettings) -> Self {
    let style = style_of(settings);
    Self::with_highlight(
      ThemeSource::Cover(source),
      "Pochette".to_string(),
      scheme::styled(source, true, style),
      true,
      style.highlight_buttons,
    )
  }

  /// Thème d'un fichier validé.
  #[must_use]
  pub fn from_file(id: String, loaded: LoadedTheme) -> Self {
    Self::new(
      ThemeSource::File(id),
      loaded.name,
      loaded.scheme,
      loaded.dark,
    )
  }
}
