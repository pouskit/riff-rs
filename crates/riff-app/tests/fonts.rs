//! Rendu réel des polices embarquées : chaque graisse de Google Sans doit laisser plus d'encre que la précédente,
//! et les icônes (SVG teintés, glyphe Material) doivent se dessiner (pas de case vide ni de repli sur une autre police).

use iced::widget::{container, text};
use iced::{Color, Element, Fill, Font, Settings, Size, Theme};
use iced_test::Simulator;
use riff_app::theme::{fonts, icons};
use std::fs;

#[path = "support/temp.rs"]
mod temp;

const SAMPLE: &str = "Riff Hamburgefonstiv 0123";

/// Rend `view` hors écran et renvoie l'image.
fn render(view: Element<'_, (), Theme>) -> image::RgbaImage {
  let settings = Settings {
    default_font: fonts::UI,
    fonts: fonts::bytes(),
    ..Settings::default()
  };
  let mut simulator = Simulator::with_size(settings, Size::new(420.0, 60.0), view);
  let snapshot = simulator.snapshot(&Theme::Dark).expect("capture");
  let scratch = temp::TempDir::new("fonts-test");
  assert!(
    snapshot
      .matches_image(scratch.join("shot.png"))
      .expect("PNG")
  );
  let path = fs::read_dir(&scratch)
    .expect("lecture")
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .find(|path| path.extension().is_some_and(|ext| ext == "png"))
    .expect("PNG produit");
  image::open(&path).expect("lecture du PNG").into_rgba8()
}

/// Image d'une vue sur fond noir dont le texte est `color`.
fn shot(view: Element<'_, (), Theme>, color: Color) -> image::RgbaImage {
  let view = container(view)
    .width(Fill)
    .height(Fill)
    .style(move |_| {
      container::Style::default()
        .background(Color::BLACK)
        .color(color)
    })
    .into();
  render(view)
}

/// Somme d'un canal (0 rouge, 1 vert) d'une vue sur fond noir dont le texte est `color`.
fn channel(view: Element<'_, (), Theme>, color: Color, index: usize) -> u64 {
  shot(view, color)
    .pixels()
    .map(|pixel| u64::from(pixel[index]))
    .sum()
}

/// Encre (somme des écarts au fond noir) d'un texte rendu en blanc sur noir.
fn ink(content: &str, font: Font, size: f32) -> u64 {
  let view: Element<'_, (), Theme> = text(content.to_string()).font(font).size(size).into();
  channel(view, Color::WHITE, 0)
}

#[test]
fn the_four_google_sans_weights_render_with_growing_ink() {
  let weights =
    [fonts::UI, fonts::MEDIUM, fonts::SEMIBOLD, fonts::BOLD].map(|font| ink(SAMPLE, font, 24.0));
  assert!(
    weights.windows(2).all(|pair| pair[0] < pair[1]),
    "l'encre doit croître avec la graisse : {weights:?}"
  );
}

#[test]
fn the_embedded_font_replaces_the_system_one() {
  let google = ink(SAMPLE, fonts::UI, 24.0);
  let system = ink(SAMPLE, Font::with_name("Segoe UI"), 24.0);
  assert_ne!(google, system);
}

#[test]
fn the_material_symbols_subset_draws_discover_tune() {
  let view: Element<'_, (), Theme> = icons::glyph_px(icons::DISCOVER_TUNE, 24.0);
  assert!(channel(view, Color::WHITE, 0) > 0);
}

#[test]
fn every_svg_icon_draws() {
  for key in icons::all() {
    let view: Element<'_, (), Theme> = icons::glyph_px(key, 24.0);
    assert!(channel(view, Color::WHITE, 0) > 0, "icône vide : {key:?}");
  }
}

#[test]
fn a_tinted_svg_icon_takes_the_text_color_and_its_opacity() {
  let draw = |color, index| {
    let view: Element<'_, (), Theme> = icons::glyph_px(icons::SETTINGS, 24.0);
    channel(view, color, index)
  };
  let red = Color::from_rgb(1.0, 0.0, 0.0);
  assert!(draw(red, 0) > 0);
  assert_eq!(draw(red, 1), 0, "aucune autre couleur que celle du texte");
  let half = draw(Color { a: 0.5, ..red }, 0);
  assert!(half > 0 && half < draw(red, 0));
}

#[test]
fn the_home_logo_keeps_its_own_colors() {
  let draw = |color| shot(icons::glyph_px(icons::HOME, 48.0), color);
  let white = draw(Color::WHITE);
  assert!(
    white == draw(Color::from_rgb(1.0, 0.0, 0.0)),
    "le logo n'est pas teinté"
  );
  assert!(
    white
      .pixels()
      .any(|pixel| pixel[1] != pixel[0] || pixel[2] != pixel[0]),
    "le logo a des couleurs"
  );
}
