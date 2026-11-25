//! Couleur dominante d'une pochette.

use image::imageops::FilterType;
use material_colors::color::Rgb;
use material_colors::image::extract_color;

const SAMPLE_SIZE: u32 = 128;

/// Couleur source extraite d'une image PNG ou JPEG. Calcul long : à lancer hors du fil de l'interface.
///
/// # Errors
/// Image illisible.
pub fn source_from_image(bytes: &[u8]) -> Result<Rgb, image::ImageError> {
  let small = image::load_from_memory(bytes)?
    .resize(SAMPLE_SIZE, SAMPLE_SIZE, FilterType::Triangle)
    .into_rgb8();
  let pixels: Vec<Rgb> = small.pixels().map(|p| Rgb::new(p[0], p[1], p[2])).collect();
  Ok(extract_color(&pixels))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::theme::{contrast, scheme};
  use std::path::Path;

  fn cover(index: u32) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/data/cover{index}.png"));
    std::fs::read(path).expect("pochette")
  }

  #[test]
  fn three_covers_give_readable_schemes_in_both_modes() {
    for index in 1..=3 {
      let source = source_from_image(&cover(index)).expect("image");
      for dark in [true, false] {
        let schema = scheme::from_source(source, dark);
        for (pair, foreground, background) in contrast::pairs(&schema) {
          let ratio = contrast::ratio(foreground, background);
          assert!(
            ratio >= contrast::REQUIRED_RATIO,
            "pochette {index} dark={dark} {pair} {ratio:.2}"
          );
        }
      }
    }
  }

  #[test]
  fn different_covers_give_different_sources() {
    let sources: Vec<Rgb> = (1..=3)
      .map(|index| source_from_image(&cover(index)).expect("image"))
      .collect();
    assert_ne!(sources[0], sources[1]);
    assert_ne!(sources[1], sources[2]);
  }

  #[test]
  fn invalid_bytes_give_an_error() {
    assert!(source_from_image(b"pas une image").is_err());
  }
}
