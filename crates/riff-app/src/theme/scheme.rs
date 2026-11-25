//! Schémas M3 et thème iced correspondant.

use crate::theme::contrast;
use iced::theme::Palette;
use iced::theme::palette::{
  Background, Danger, Extended, Pair, Primary, Secondary, Success, Warning,
};
use iced::{Color, Theme};
use material_colors::color::Rgb;
use material_colors::dynamic_color::Variant;
use material_colors::hct::Hct;
use material_colors::scheme::Scheme;
use material_colors::theme::ThemeBuilder;

/// Couleur source d'un Riff vierge (`app.js:594`, `:183`, `:817`). Le lavande `rgb(180, 159, 224)` des captures vient
/// de l'accent `#7342D7` du profil de l'utilisateur, pas de ce défaut.
pub const DEFAULT_SOURCE: Rgb = Rgb::new(0x52, 0x47, 0xD1);
/// Fond sombre de Riff (`--page-solid`).
pub const DARK_SURFACE: Rgb = Rgb::new(0x12, 0x12, 0x12);

/// Schéma M3 généré à partir d'une couleur source.
#[must_use]
pub fn from_source(source: Rgb, dark: bool) -> Scheme {
  let schemes = ThemeBuilder::with_source(source).build().schemes;
  if dark { schemes.dark } else { schemes.light }
}

/// Schéma du thème par couleur d'accent : fond de Riff en sombre, primaire égale à l'accent si le contraste le permet.
#[must_use]
pub fn from_accent(accent: Rgb, dark: bool) -> Scheme {
  let mut scheme = from_source(accent, dark);
  if dark {
    scheme.surface = DARK_SURFACE;
    scheme.background = DARK_SURFACE;
  }
  if contrast::ratio(scheme.on_primary, accent) >= contrast::REQUIRED_RATIO {
    scheme.primary = accent;
  }
  scheme
}

/// Style de palette de la page de réglages (`PALETTE_STYLES`, `app.js:1099`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteStyle {
  Classic,
  TonalSpot,
  Expressive,
  Vibrant,
  Neutral,
}

impl PaletteStyle {
  /// Valeur inconnue : `classic`, comme `PALETTE_STYLES[styleKey] || PALETTE_STYLES.classic`.
  #[must_use]
  pub fn parse(value: &str) -> Self {
    match value {
      "tonal-spot" => Self::TonalSpot,
      "expressive" => Self::Expressive,
      "vibrant" => Self::Vibrant,
      "neutral" => Self::Neutral,
      _ => Self::Classic,
    }
  }

  /// Décalages de teinte secondaire et tertiaire, échelles de saturation primaire et des conteneurs (`PALETTE_STYLES`).
  const fn dark_scales(self) -> (f64, f64, f64, f64) {
    match self {
      Self::Classic => (0.0, 0.0, 1.0, 1.0),
      Self::TonalSpot => (0.0, 60.0, 1.0, 0.9),
      Self::Expressive => (60.0, 120.0, 1.25, 1.3),
      Self::Vibrant => (0.0, 60.0, 1.4, 1.4),
      Self::Neutral => (0.0, 0.0, 0.35, 0.25),
    }
  }

  const fn surface_scale(self) -> f64 {
    match self {
      Self::Classic => 1.0,
      Self::TonalSpot => 0.8,
      Self::Expressive => 1.5,
      Self::Vibrant => 1.2,
      Self::Neutral => 0.2,
    }
  }
}

/// Réglages du thème : luminosité et intensité en pourcentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeStyle {
  pub brightness: u8,
  pub intensity: u8,
  pub palette: PaletteStyle,
  pub intensity_bg: bool,
  pub highlight_buttons: bool,
}

/// Intensité par défaut de Riff : l'accent y reste exact.
const REFERENCE_INTENSITY: f64 = 85.0;
/// `isMonochrome` (`app.js:1195`) : sous ce seuil, plus aucune couleur.
const MONOCHROME_BELOW: u8 = 8;
const TONAL_SPOT_TERTIARY_SHIFT: f64 = 60.0;

/// `hslToHex` de Riff, `saturation` et `lightness` en pourcentage.
fn hsl(hue: f64, saturation: f64, lightness: f64) -> Rgb {
  let (s, l) = (saturation / 100.0, lightness / 100.0);
  let a = s * l.min(1.0 - l);
  let channel = |n: f64| {
    let k = (n + hue / 30.0) % 12.0;
    let value = l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
    (value * 255.0).round() as u8
  };
  Rgb::new(channel(0.0), channel(8.0), channel(4.0))
}

/// Teinte et saturation HSL d'une couleur, arrondies à l'entier comme `rgbToHsl` (`app.js:1056`).
fn hue_saturation(color: Rgb) -> (f64, f64) {
  let (r, g, b) = (
    f64::from(color.red) / 255.0,
    f64::from(color.green) / 255.0,
    f64::from(color.blue) / 255.0,
  );
  let (max, min) = (r.max(g).max(b), r.min(g).min(b));
  let delta = max - min;
  if delta <= f64::EPSILON {
    return (0.0, 0.0);
  }
  let lightness = (max + min) / 2.0;
  let saturation = delta
    / if lightness > 0.5 {
      2.0 - max - min
    } else {
      max + min
    };
  let hue = if (max - r).abs() <= f64::EPSILON {
    ((g - b) / delta).rem_euclid(6.0)
  } else if (max - g).abs() <= f64::EPSILON {
    (b - r) / delta + 2.0
  } else {
    (r - g) / delta + 4.0
  };
  ((hue * 60.0).round(), (saturation * 100.0).round())
}

/// Schéma M3 avec les réglages de la page « Thème ». Les surfaces suivent les formules de `app.js:1199` et `:1260`,
/// les couleurs viennent de `material-colors`, avec une chrominance réduite par l'intensité.
#[must_use]
pub fn styled(accent: Rgb, dark: bool, style: ThemeStyle) -> Scheme {
  let mut source = Hct::new(accent);
  let chroma = source.get_chroma();
  source.set_chroma(chroma * f64::from(style.intensity) / REFERENCE_INTENSITY);
  let scaled: Rgb = source.into();
  let monochrome = style.intensity < MONOCHROME_BELOW;
  let variant = match style.palette {
    _ if monochrome => Variant::Monochrome,
    PaletteStyle::Classic | PaletteStyle::TonalSpot => Variant::TonalSpot,
    PaletteStyle::Expressive => Variant::Expressive,
    PaletteStyle::Vibrant => Variant::Vibrant,
    PaletteStyle::Neutral => Variant::Neutral,
  };
  let mut builder = ThemeBuilder::with_source(scaled).variant(variant);
  if style.palette == PaletteStyle::Classic && !monochrome {
    // `classic` garde la même teinte pour la couleur tertiaire (`tertHueShift: 0`) : `TonalSpot` y ajoute 60 degrés.
    let mut tertiary = Hct::new(scaled);
    let hue = tertiary.get_hue();
    tertiary.set_hue(hue - TONAL_SPOT_TERTIARY_SHIFT);
    builder = builder.tertiary(tertiary.into());
  }
  let schemes = builder.build().schemes;
  let mut scheme = if dark { schemes.dark } else { schemes.light };
  if !monochrome
    && matches!(
      style.palette,
      PaletteStyle::Classic | PaletteStyle::TonalSpot
    )
    && contrast::ratio(scheme.on_primary, scaled) >= contrast::REQUIRED_RATIO
  {
    scheme.primary = scaled;
  }
  apply_surfaces(&mut scheme, accent, dark, style);
  if dark && !monochrome {
    apply_dark_roles(&mut scheme, accent, style);
  }
  apply_error(&mut scheme, dark, monochrome);
  scheme
}

/// Erreur : `generateM3Palette` ne la pose qu'en monochrome (`app.js:1225`, `:1286`) ; sinon la valeur de `:root`
/// (`styles.css:58`) reste. Le gris clair monochrome de Riff (`#777777` sur blanc, 4,48:1) passe sous le contraste
/// exigé : il garde l'erreur de `material-colors`.
fn apply_error(scheme: &mut Scheme, dark: bool, monochrome: bool) {
  let (error, on_error) = match (monochrome, dark) {
    (false, _) => (Rgb::new(0xf2, 0xb8, 0xb5), Rgb::new(0x60, 0x14, 0x10)),
    (true, true) => (Rgb::new(0x8a, 0x8a, 0x8a), Rgb::new(0x12, 0x12, 0x12)),
    (true, false) => return,
  };
  scheme.error = error;
  scheme.on_error = on_error;
}

/// Rôles de couleur du thème sombre de Riff (`generateM3Palette`, `app.js:1285`) : formules HSL de l'application
/// d'origine, mesurées sur `tokens.json` (accent de repos lavande, texte sur accent `#2c1d49`).
fn apply_dark_roles(scheme: &mut Scheme, accent: Rgb, style: ThemeStyle) {
  let (hue, _) = hue_saturation(accent);
  let (secondary_shift, tertiary_shift, primary_scale, container_scale) =
    style.palette.dark_scales();
  let factor = f64::from(style.intensity) / 100.0;
  let percent = |base: f64, scale: f64| (base * factor * scale).round().min(100.0);
  let (sat, sat_med) = (percent(60.0, primary_scale), percent(50.0, primary_scale));
  let (sat_low, sat_low2) = (
    percent(15.0, container_scale),
    percent(30.0, container_scale),
  );
  let (secondary_hue, tertiary_hue) = (
    (hue + secondary_shift) % 360.0,
    (hue + tertiary_shift) % 360.0,
  );
  let primary = hsl(hue, sat, 75.0);
  scheme.primary = primary;
  scheme.on_primary = hsl(hue, sat_med, 20.0);
  // `rgba(primary, 0.18)` sur le fond de la page.
  scheme.primary_container = mix(scheme.surface, primary, 0.18);
  scheme.on_primary_container = primary;
  scheme.secondary = hsl(secondary_hue, sat_low2, 75.0);
  scheme.secondary_container = hsl(secondary_hue, sat_low, 22.0);
  scheme.on_secondary_container = hsl(secondary_hue, sat_low2, 85.0);
  scheme.tertiary = hsl(tertiary_hue, sat_low2, 75.0);
  scheme.tertiary_container = hsl(tertiary_hue, sat_low, 22.0);
  scheme.on_surface = Rgb::new(0xe2, 0xe2, 0xe9);
  scheme.on_surface_variant = Rgb::new(0xc4, 0xc6, 0xd0);
  scheme.outline = Rgb::new(0x8e, 0x90, 0x99);
  scheme.outline_variant = Rgb::new(0x28, 0x2a, 0x33);
}

/// Mélange linéaire par canal : `weight` de `over` sur `base`.
fn mix(base: Rgb, over: Rgb, weight: f64) -> Rgb {
  let channel =
    |b: u8, o: u8| (f64::from(b) * (1.0 - weight) + f64::from(o) * weight).round() as u8;
  Rgb::new(
    channel(base.red, over.red),
    channel(base.green, over.green),
    channel(base.blue, over.blue),
  )
}

fn apply_surfaces(scheme: &mut Scheme, accent: Rgb, dark: bool, style: ThemeStyle) {
  let (hue, saturation) = hue_saturation(accent);
  let saturation = if saturation > 0.0 { saturation } else { 60.0 };
  let factor = f64::from(style.intensity) / 100.0;
  let tint = if style.intensity_bg && style.intensity >= MONOCHROME_BELOW {
    (saturation * factor * style.palette.surface_scale() * 0.65)
      .round()
      .min(28.0)
  } else {
    0.0
  };
  let br = f64::from(style.brightness) / 100.0;
  let level = |value: f64| {
    if dark {
      (value * (0.35 + br * 0.95)).round().clamp(0.0, 24.0)
    } else {
      (value + (br - 0.5) * 24.0).round().clamp(0.0, 100.0)
    }
  };
  let tone = |value: f64| hsl(hue, tint, level(value));
  if dark {
    scheme.background = tone(7.0);
    scheme.surface = tone(8.0);
    scheme.surface_dim = tone(7.0);
    scheme.surface_bright = tone(24.0);
    scheme.surface_container_lowest = tone(5.0);
    scheme.surface_container_low = tone(10.0);
    scheme.surface_container = tone(13.0);
    scheme.surface_container_high = tone(16.0);
    scheme.surface_container_highest = tone(20.0);
  } else {
    scheme.background = tone(99.0);
    scheme.surface = tone(98.0);
    scheme.surface_dim = tone(87.0);
    scheme.surface_bright = tone(98.0);
    scheme.surface_container_lowest = tone(100.0);
    scheme.surface_container_low = tone(96.0);
    scheme.surface_container = tone(94.0);
    scheme.surface_container_high = tone(92.0);
    scheme.surface_container_highest = tone(90.0);
  }
}

/// Thème iced : chaque widget de base lit les rôles M3 par `extended_palette`.
#[must_use]
pub fn to_iced_theme(name: String, scheme: &Scheme, dark: bool) -> Theme {
  to_iced_theme_with(name, scheme, dark, true)
}

/// Comme [`to_iced_theme`]. Sans `highlight`, les boutons principaux sont neutres (`body:not(.highlight-buttons)`).
#[must_use]
pub fn to_iced_theme_with(name: String, scheme: &Scheme, dark: bool, highlight: bool) -> Theme {
  let s = scheme;
  let palette = Palette {
    background: color(s.surface),
    text: color(s.on_surface),
    primary: color(s.primary),
    success: color(s.tertiary),
    warning: color(s.tertiary_container),
    danger: color(s.error),
  };
  let on_surface = |surface: Rgb| Pair {
    color: color(surface),
    text: color(s.on_surface),
  };
  let pair = |background: Rgb, text: Rgb| Pair {
    color: color(background),
    text: color(text),
  };
  let hovered = |background: Rgb, text: Rgb| Pair {
    color: state_layer(background, text),
    text: color(text),
  };
  let extended = Extended {
    background: Background {
      base: on_surface(s.surface),
      weakest: on_surface(s.surface_container_lowest),
      weaker: on_surface(s.surface_container_low),
      weak: on_surface(s.surface_container),
      neutral: on_surface(s.surface_container_high),
      strong: on_surface(s.surface_container_highest),
      stronger: on_surface(s.outline_variant),
      strongest: on_surface(s.outline),
    },
    primary: Primary {
      base: if highlight {
        pair(s.primary, s.on_primary)
      } else {
        on_surface(s.surface_container_high)
      },
      weak: pair(s.primary_container, s.on_primary_container),
      strong: hovered(s.primary, s.on_primary),
    },
    secondary: Secondary {
      base: pair(s.secondary, s.on_secondary),
      weak: pair(s.secondary_container, s.on_secondary_container),
      strong: hovered(s.secondary, s.on_secondary),
    },
    success: Success {
      base: pair(s.tertiary, s.on_tertiary),
      weak: pair(s.tertiary_container, s.on_tertiary_container),
      strong: hovered(s.tertiary, s.on_tertiary),
    },
    warning: Warning {
      base: pair(s.tertiary_container, s.on_tertiary_container),
      weak: pair(s.tertiary_container, s.on_tertiary_container),
      strong: hovered(s.tertiary_container, s.on_tertiary_container),
    },
    danger: Danger {
      base: pair(s.error, s.on_error),
      weak: pair(s.error_container, s.on_error_container),
      strong: hovered(s.error, s.on_error),
    },
    is_dark: dark,
  };
  Theme::custom_with_fn(name, palette, |_| extended)
}

fn color(rgb: Rgb) -> Color {
  Color::from_rgb8(rgb.red, rgb.green, rgb.blue)
}

/// Couche d'état M3 du survol : 8 % de la couleur du texte.
fn state_layer(background: Rgb, text: Rgb) -> Color {
  const OPACITY: f32 = 0.08;
  let (b, t) = (color(background), color(text));
  Color::from_rgb(
    b.r + (t.r - b.r) * OPACITY,
    b.g + (t.g - b.g) * OPACITY,
    b.b + (t.b - b.b) * OPACITY,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Lavande des captures, accent de test.
  const LAVENDER: Rgb = Rgb::new(0xB4, 0x9E, 0xE0);

  #[test]
  fn default_scheme_uses_the_colors_of_riff() {
    let scheme = from_accent(LAVENDER, true);
    assert_eq!(scheme.primary, LAVENDER);
    assert_eq!(scheme.surface, DARK_SURFACE);
    assert_eq!(scheme.background, DARK_SURFACE);
  }

  const DEFAULT_STYLE: ThemeStyle = ThemeStyle {
    brightness: 20,
    intensity: 85,
    palette: PaletteStyle::Classic,
    intensity_bg: false,
    highlight_buttons: true,
  };

  fn chroma(color: Rgb) -> f64 {
    Hct::new(color).get_chroma()
  }

  fn tone(color: Rgb) -> f64 {
    Hct::new(color).get_tone()
  }

  #[test]
  fn the_default_style_gives_the_rest_lavender_of_riff() {
    assert_eq!(
      styled(LAVENDER, true, DEFAULT_STYLE).primary,
      Rgb::new(180, 159, 224)
    );
  }

  #[test]
  fn the_dark_surface_follows_the_formula_of_riff() {
    // B(7) = round(7 * (0.35 + 0.2 * 0.95)) = 4 %, soit #0A0A0A.
    assert_eq!(
      styled(LAVENDER, true, DEFAULT_STYLE).surface,
      Rgb::new(10, 10, 10)
    );
  }

  #[test]
  fn brightness_changes_the_surface_in_both_modes() {
    for dark in [true, false] {
      let low = styled(
        LAVENDER,
        dark,
        ThemeStyle {
          brightness: 0,
          ..DEFAULT_STYLE
        },
      );
      let high = styled(
        LAVENDER,
        dark,
        ThemeStyle {
          brightness: 100,
          ..DEFAULT_STYLE
        },
      );
      assert!(tone(high.surface) > tone(low.surface), "dark={dark}");
      assert!(
        tone(high.surface_container_highest) > tone(low.surface_container_highest),
        "dark={dark}"
      );
    }
  }

  #[test]
  fn intensity_reduces_the_chroma_of_the_primary() {
    let strong = styled(
      LAVENDER,
      true,
      ThemeStyle {
        intensity: 100,
        ..DEFAULT_STYLE
      },
    );
    let weak = styled(
      LAVENDER,
      true,
      ThemeStyle {
        intensity: 30,
        ..DEFAULT_STYLE
      },
    );
    assert!(
      chroma(strong.primary) > chroma(weak.primary) + 10.0,
      "{} {}",
      chroma(strong.primary),
      chroma(weak.primary)
    );
  }

  #[test]
  fn a_very_low_intensity_gives_a_monochrome_scheme() {
    let scheme = styled(
      LAVENDER,
      true,
      ThemeStyle {
        intensity: 5,
        ..DEFAULT_STYLE
      },
    );
    let primary = scheme.primary;
    assert!(
      primary.red == primary.green && primary.green == primary.blue,
      "{primary:?}"
    );
  }

  #[test]
  fn each_palette_style_gives_a_different_scheme() {
    let schemes: Vec<Scheme> = [
      PaletteStyle::Classic,
      PaletteStyle::TonalSpot,
      PaletteStyle::Expressive,
      PaletteStyle::Vibrant,
      PaletteStyle::Neutral,
    ]
    .into_iter()
    .map(|palette| {
      styled(
        LAVENDER,
        true,
        ThemeStyle {
          palette,
          ..DEFAULT_STYLE
        },
      )
    })
    .collect();
    for (index, scheme) in schemes.iter().enumerate() {
      for other in &schemes[index + 1..] {
        assert!(
          (scheme.primary, scheme.secondary, scheme.tertiary)
            != (other.primary, other.secondary, other.tertiary),
          "{index}"
        );
      }
    }
  }

  #[test]
  fn an_unknown_palette_style_falls_back_to_classic() {
    assert_eq!(PaletteStyle::parse("rose"), PaletteStyle::Classic);
    assert_eq!(PaletteStyle::parse("tonal-spot"), PaletteStyle::TonalSpot);
  }

  #[test]
  fn the_background_is_tinted_only_when_asked() {
    let plain = styled(
      LAVENDER,
      true,
      ThemeStyle {
        brightness: 100,
        ..DEFAULT_STYLE
      },
    );
    let tinted = styled(
      LAVENDER,
      true,
      ThemeStyle {
        brightness: 100,
        intensity_bg: true,
        ..DEFAULT_STYLE
      },
    );
    assert!(chroma(plain.surface) < 1.0, "{}", chroma(plain.surface));
    assert!(
      chroma(tinted.surface) > chroma(plain.surface) + 1.0,
      "{}",
      chroma(tinted.surface)
    );
  }

  #[test]
  fn without_highlight_the_main_buttons_are_neutral() {
    let scheme = styled(LAVENDER, true, DEFAULT_STYLE);
    let on = to_iced_theme_with("a".to_string(), &scheme, true, true);
    let off = to_iced_theme_with("a".to_string(), &scheme, true, false);
    assert_eq!(
      on.extended_palette().primary.base.color,
      color(scheme.primary)
    );
    assert_eq!(
      off.extended_palette().primary.base.color,
      color(scheme.surface_container_high)
    );
  }

  #[test]
  fn every_style_keeps_the_required_contrast() {
    for accent in [
      LAVENDER,
      Rgb::new(0x52, 0x47, 0xD1),
      Rgb::new(0xE0, 0x30, 0x30),
      Rgb::new(0xF0, 0xE0, 0x40),
    ] {
      for dark in [true, false] {
        for brightness in [0, 20, 50, 100] {
          for intensity in [0, 7, 8, 50, 85, 100] {
            for palette in [
              PaletteStyle::Classic,
              PaletteStyle::TonalSpot,
              PaletteStyle::Expressive,
              PaletteStyle::Vibrant,
              PaletteStyle::Neutral,
            ] {
              for intensity_bg in [false, true] {
                let style = ThemeStyle {
                  brightness,
                  intensity,
                  palette,
                  intensity_bg,
                  highlight_buttons: true,
                };
                let scheme = styled(accent, dark, style);
                assert!(
                  contrast::check(&scheme).is_ok(),
                  "{accent:?} {dark} {style:?} {:?}",
                  contrast::check(&scheme).err()
                );
              }
            }
          }
        }
      }
    }
  }

  #[test]
  fn accent_schemes_pass_the_contrast_check_in_both_modes() {
    for accent in [
      LAVENDER,
      Rgb::new(0x52, 0x47, 0xD1),
      Rgb::new(0xFF, 0xD5, 0x00),
      Rgb::new(0x10, 0x10, 0x10),
    ] {
      for dark in [true, false] {
        assert_eq!(
          contrast::check(&from_accent(accent, dark)),
          Ok(()),
          "{accent} dark={dark}"
        );
      }
    }
  }

  #[test]
  fn an_accent_without_contrast_does_not_become_the_primary() {
    let accent = Rgb::new(0x52, 0x47, 0xD1);
    assert_ne!(from_accent(accent, true).primary, accent);
  }

  #[test]
  fn iced_theme_exposes_the_m3_roles() {
    let scheme = from_source(LAVENDER, true);
    let theme = to_iced_theme("test".to_string(), &scheme, true);
    let palette = theme.extended_palette();
    assert_eq!(palette.background.base.color, color(scheme.surface));
    assert_eq!(palette.background.base.text, color(scheme.on_surface));
    assert_eq!(
      palette.background.weak.color,
      color(scheme.surface_container)
    );
    assert_eq!(palette.primary.base.color, color(scheme.primary));
    assert_eq!(palette.primary.base.text, color(scheme.on_primary));
    assert_eq!(palette.primary.weak.color, color(scheme.primary_container));
    assert_eq!(
      palette.secondary.weak.color,
      color(scheme.secondary_container)
    );
    assert_eq!(
      palette.secondary.weak.text,
      color(scheme.on_secondary_container)
    );
    assert_eq!(palette.danger.base.color, color(scheme.error));
    assert!(palette.is_dark);
    let light = to_iced_theme("clair".to_string(), &from_source(LAVENDER, false), false);
    assert!(!light.extended_palette().is_dark);
  }
}
