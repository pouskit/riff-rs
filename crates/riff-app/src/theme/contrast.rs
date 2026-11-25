//! Contraste WCAG des paires de texte d'un schéma.

use material_colors::color::Rgb;
use material_colors::scheme::Scheme;

/// Contraste minimal d'une paire `on-X/X`.
pub const REQUIRED_RATIO: f64 = 4.5;

/// Paire sous le contraste minimal.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("contraste insuffisant pour {pair} : {ratio:.2}:1")]
pub struct ContrastError {
  pub pair: &'static str,
  pub ratio: f64,
}

/// Rapport de contraste WCAG de deux couleurs.
#[must_use]
pub fn ratio(a: Rgb, b: Rgb) -> f64 {
  let (la, lb) = (luminance(a), luminance(b));
  (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn linear(channel: u8) -> f64 {
  let value = f64::from(channel) / 255.0;
  if value <= 0.04045 {
    value / 12.92
  } else {
    ((value + 0.055) / 1.055).powf(2.4)
  }
}

fn luminance(color: Rgb) -> f64 {
  0.2126 * linear(color.red) + 0.7152 * linear(color.green) + 0.0722 * linear(color.blue)
}

/// Les 13 paires de texte vérifiées.
#[must_use]
pub fn pairs(s: &Scheme) -> [(&'static str, Rgb, Rgb); 13] {
  [
    ("on_surface/surface", s.on_surface, s.surface),
    (
      "on_surface_variant/surface_variant",
      s.on_surface_variant,
      s.surface_variant,
    ),
    ("on_background/background", s.on_background, s.background),
    ("on_primary/primary", s.on_primary, s.primary),
    (
      "on_primary_container/primary_container",
      s.on_primary_container,
      s.primary_container,
    ),
    ("on_secondary/secondary", s.on_secondary, s.secondary),
    (
      "on_secondary_container/secondary_container",
      s.on_secondary_container,
      s.secondary_container,
    ),
    ("on_tertiary/tertiary", s.on_tertiary, s.tertiary),
    (
      "on_tertiary_container/tertiary_container",
      s.on_tertiary_container,
      s.tertiary_container,
    ),
    ("on_error/error", s.on_error, s.error),
    (
      "on_error_container/error_container",
      s.on_error_container,
      s.error_container,
    ),
    (
      "inverse_on_surface/inverse_surface",
      s.inverse_on_surface,
      s.inverse_surface,
    ),
    (
      "on_surface/surface_container_highest",
      s.on_surface,
      s.surface_container_highest,
    ),
  ]
}

/// Vérifie toutes les paires du schéma.
///
/// # Errors
/// La première paire sous [`REQUIRED_RATIO`].
pub fn check(scheme: &Scheme) -> Result<(), ContrastError> {
  for (pair, foreground, background) in pairs(scheme) {
    let ratio = ratio(foreground, background);
    if ratio < REQUIRED_RATIO {
      return Err(ContrastError { pair, ratio });
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use material_colors::theme::ThemeBuilder;

  #[test]
  fn black_on_white_is_21() {
    assert!((ratio(Rgb::new(0, 0, 0), Rgb::new(255, 255, 255)) - 21.0).abs() < 0.01);
  }

  #[test]
  fn same_color_is_1() {
    assert!((ratio(Rgb::new(10, 200, 30), Rgb::new(10, 200, 30)) - 1.0).abs() < 1e-9);
  }

  #[test]
  fn ratio_does_not_depend_on_the_order() {
    let (a, b) = (Rgb::new(29, 185, 84), Rgb::new(18, 18, 20));
    assert!((ratio(a, b) - ratio(b, a)).abs() < 1e-9);
  }

  #[test]
  fn check_names_the_failing_pair() {
    let mut scheme = ThemeBuilder::with_source(Rgb::new(29, 185, 84))
      .build()
      .schemes
      .dark;
    assert_eq!(check(&scheme), Ok(()));
    scheme.on_primary = scheme.primary;
    let err = check(&scheme).unwrap_err();
    assert_eq!(err.pair, "on_primary/primary");
    assert!((err.ratio - 1.0).abs() < 1e-9);
  }
}
