//! Polices embarquées : Google Sans (OFL 1.1) et Material Symbols (Apache 2.0).
//!
//! Fichiers statiques par graisse dans `assets/fonts/`, sous-ensembles latin, grec et cyrillique
//! pour le texte. Material Symbols ne garde que `discover_tune`, le seul glyphe de ce jeu que Riff
//! affiche (`theme::icons`) ; les autres icônes sont des SVG.

use iced::Font;
use iced::font::{Family, Weight};
use std::borrow::Cow;

/// Famille du texte de l'interface, `--font-family` de Riff.
pub const UI_FAMILY: &str = "Google Sans";
/// Famille des icônes.
pub const ICON_FAMILY: &str = "Material Symbols Outlined";

/// Police par défaut de l'interface (graisse 400).
pub const UI: Font = ui_weight(Weight::Normal);
/// Graisse 500 : titre de la barre de lecture.
pub const MEDIUM: Font = ui_weight(Weight::Medium);
/// Graisse 600.
pub const SEMIBOLD: Font = ui_weight(Weight::Semibold);
/// Graisse 700 : titres de section.
pub const BOLD: Font = ui_weight(Weight::Bold);

/// Glyphe `discover_tune` au contour (`FILL` 0).
pub const ICONS: Font = Font::with_name(ICON_FAMILY);

const fn ui_weight(weight: Weight) -> Font {
  Font {
    family: Family::Name(UI_FAMILY),
    weight,
    ..Font::DEFAULT
  }
}

/// Octets des polices à charger au démarrage (`iced::daemon(..).font(..)`, `Settings::fonts`).
#[must_use]
pub fn bytes() -> Vec<Cow<'static, [u8]>> {
  vec![
    Cow::Borrowed(include_bytes!("../../assets/fonts/GoogleSans-Regular.ttf")),
    Cow::Borrowed(include_bytes!("../../assets/fonts/GoogleSans-Medium.ttf")),
    Cow::Borrowed(include_bytes!("../../assets/fonts/GoogleSans-SemiBold.ttf")),
    Cow::Borrowed(include_bytes!("../../assets/fonts/GoogleSans-Bold.ttf")),
    Cow::Borrowed(include_bytes!(
      "../../assets/fonts/MaterialSymbols-Outlined.ttf"
    )),
  ]
}
