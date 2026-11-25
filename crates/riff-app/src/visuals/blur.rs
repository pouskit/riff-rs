//! Fond flouté précalculé (décision 3) : réduction, flou, saturation. Calcul à lancer hors du fil de l'interface.

use crate::images::Decoded;
use image::RgbaImage;
use image::imageops::{self, FilterType};
use std::sync::Arc;

/// Largeur de l'image réduite : le flou élimine les détails, l'affichage étire sans perte visible.
pub const TARGET_WIDTH: u32 = 200;

/// Demande de flou.
#[derive(Debug, Clone, PartialEq)]
pub struct BlurRequest {
  /// Rayon CSS en pixels de la fenêtre, multiplié par 10 pour rester entier.
  pub sigma_tenths: u32,
  pub aspect_tenths: u32,
  /// Zoom de `inset: -15%`, multiplié par 100.
  pub zoom_percent: u32,
  /// `saturate()`, multiplié par 100.
  pub saturate_percent: u32,
  pub window_width: u32,
}

/// Identité d'un fond calculé.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlurKey {
  pub url: String,
  pub sigma_tenths: u32,
  pub aspect_tenths: u32,
  pub zoom_percent: u32,
  pub saturate_percent: u32,
  pub window_width: u32,
  /// Pièce du fond de la vue plein écran : un orbe `.tvk-ambient-blob` (0 à 3) ou la vignette ([`SHADE_PART`]). `None` : pochette floutée.
  pub ambient: Option<u8>,
}

impl BlurKey {
  #[must_use]
  pub fn new(url: &str, request: &BlurRequest) -> Self {
    Self {
      url: url.to_string(),
      sigma_tenths: request.sigma_tenths,
      aspect_tenths: request.aspect_tenths,
      zoom_percent: request.zoom_percent,
      saturate_percent: request.saturate_percent,
      window_width: request.window_width,
      ambient: None,
    }
  }
}

fn saturate(image: &mut RgbaImage, amount: f32) {
  // Matrice de `saturate()` en CSS.
  for pixel in image.pixels_mut() {
    let [r, g, b, _] = pixel.0;
    let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let channel = |value: f32| (luminance + (value - luminance) * amount).clamp(0.0, 255.0) as u8;
    pixel.0 = [channel(r), channel(g), channel(b), pixel.0[3]];
  }
}

/// Calcule le fond. `None` : image vide ou données invalides.
#[must_use]
pub fn compute(width: u32, height: u32, rgba: &[u8], key: &BlurKey) -> Option<Decoded> {
  let source = RgbaImage::from_raw(width, height, rgba.to_vec())?;
  if width == 0 || height == 0 {
    return None;
  }
  match key.ambient {
    Some(SHADE_PART) => return Some(shade(key)),
    Some(index) => return Some(orb(&source, key, usize::from(index).min(3))),
    None => {}
  }
  let aspect = key.aspect_tenths.max(1) as f32 / 10.0;
  let target_height = ((TARGET_WIDTH as f32 / aspect).round() as u32).max(1);
  // Découpe centrée : même cadrage que `background-size: cover`, avec le zoom demandé.
  let zoom = key.zoom_percent.max(100) as f32 / 100.0;
  let crop_width = (width as f32).min(height as f32 * aspect) / zoom;
  let crop_height = crop_width / aspect;
  let (crop_width, crop_height) = ((crop_width as u32).max(1), (crop_height as u32).max(1));
  let cropped = imageops::crop_imm(
    &source,
    (width - crop_width) / 2,
    (height - crop_height) / 2,
    crop_width,
    crop_height,
  )
  .to_image();
  let reduced = imageops::resize(&cropped, TARGET_WIDTH, target_height, FilterType::Triangle);
  let scale = TARGET_WIDTH as f32 / key.window_width.max(1) as f32;
  let sigma = key.sigma_tenths as f32 / 10.0 * scale;
  let mut blurred = if sigma > 0.05 {
    imageops::fast_blur(&reduced, sigma)
  } else {
    reduced
  };
  if key.saturate_percent != 100 {
    saturate(&mut blurred, key.saturate_percent as f32 / 100.0);
  }
  Some(Decoded {
    width: blurred.width(),
    height: blurred.height(),
    rgba: blurred.into_raw(),
  })
}

/// Largeur du fond de la vue plein écran : le flou et la vignette ne gardent aucun détail.
pub const AMBIENT_WIDTH: u32 = 256;
/// Pièce du fond plein écran qui n'est pas un orbe : la vignette de `.tv-karaoke::before`. Les orbes sont numérotés de 0 à 3.
pub const SHADE_PART: u8 = 4;
/// `#tvk-ambient-bg` : `background: #0d0d12` (`styles.css:6300-6313`).
const AMBIENT_BASE: [f32; 3] = [13.0, 13.0, 18.0];
/// `filter: ... saturate(1.95) contrast(1.12) brightness(0.76)` (`styles.css:6312`).
const AMBIENT_SATURATE: f32 = 1.95;
const AMBIENT_CONTRAST: f32 = 1.12;
const AMBIENT_BRIGHTNESS: f32 = 0.76;
/// `.tv-karaoke::before` : `radial-gradient(circle at 50% 45%, rgba(0,0,0,0.14) 0%, rgba(0,0,0,0.48) 100%)` (`styles.css:6315-6322`).
const VIGNETTE: (f32, f32, f32) = (0.45, 0.14, 0.48);

/// `contrast()` puis `brightness()` d'une composante de 0 à 1.
fn tone(value: f32) -> f32 {
  ((value - 0.5) * AMBIENT_CONTRAST + 0.5) * AMBIENT_BRIGHTNESS
}

/// Couleur du fond du conteneur `#tvk-ambient-bg` après `saturate`, `contrast` et `brightness` : le flou ne change pas un aplat.
#[must_use]
pub fn ambient_base() -> [u8; 3] {
  let [r, g, b] = AMBIENT_BASE;
  let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  [r, g, b].map(|value| {
    let saturated = (luminance + (value - luminance) * AMBIENT_SATURATE).clamp(0.0, 255.0);
    (tone(saturated / 255.0) * 255.0).clamp(0.0, 255.0).round() as u8
  })
}

/// Échantillon bilinéaire, coordonnées en pixels de l'image.
fn sample(image: &RgbaImage, x: f32, y: f32) -> [f32; 4] {
  let (width, height) = (image.width() as f32, image.height() as f32);
  let x = (x - 0.5).clamp(0.0, width - 1.0);
  let y = (y - 0.5).clamp(0.0, height - 1.0);
  let (x0, y0) = (x.floor(), y.floor());
  let (fx, fy) = (x - x0, y - y0);
  let at = |dx: f32, dy: f32| {
    let pixel = image.get_pixel(
      (x0 + dx).min(width - 1.0) as u32,
      (y0 + dy).min(height - 1.0) as u32,
    );
    pixel.0.map(f32::from)
  };
  let (a, b, c, d) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
  let mut out = [0.0; 4];
  for channel in 0..4 {
    let top = a[channel] + (b[channel] - a[channel]) * fx;
    let bottom = c[channel] + (d[channel] - c[channel]) * fx;
    out[channel] = top + (bottom - top) * fy;
  }
  out
}

/// Dimensions du conteneur du fond (`#tvk-ambient-bg`, la page entière) dans l'image de sortie, et pixels de sortie par pixel CSS du
/// conteneur de Riff, qui fait un quart de la page avant son agrandissement par 4 : le rayon du flou se compte dans cet espace.
fn ambient_space(key: &BlurKey) -> (f32, f32, f32) {
  let aspect = key.aspect_tenths.max(1) as f32 / 10.0;
  let height = ((AMBIENT_WIDTH as f32 / aspect).round() as u32).max(1);
  let page_width = key.window_width.max(1) as f32 / crate::theme::tokens::ZOOM;
  (
    AMBIENT_WIDTH as f32,
    height as f32,
    AMBIENT_WIDTH as f32 / (page_width / 4.0),
  )
}

/// Un orbe `.tvk-ambient-blob` du fond plein écran, dans son propre repère : l'ellipse de la pochette (`border-radius: 50%`,
/// `background-size: cover`) entourée de la marge du flou, centrée dans l'image. Les couleurs passent par `saturate`, `contrast` et
/// `brightness` ; l'alpha porte le bord de l'ellipse, l'opacité de l'orbe est celle du dessin. Riff floute le conteneur entier
/// après le déplacement des orbes ; ici chaque orbe est flouté avant le sien, à un rayon divisé par l'échelle moyenne de ses images
/// clés (voir `visuals::orbs`).
fn orb(source: &RgbaImage, key: &BlurKey, index: usize) -> Decoded {
  let spec = &crate::theme::tokens::karaoke::ORBS[index];
  let (container_w, container_h, scale) = ambient_space(key);
  let mean_scale =
    spec.frames.iter().map(|frame| frame.scale).sum::<f32>() / spec.frames.len() as f32;
  let sigma = key.sigma_tenths as f32 / 10.0 * scale / mean_scale;
  let (w, h) = (spec.width * container_w, spec.height * container_h);
  let pad = (sigma * 3.0).ceil() + 2.0;
  let (sprite_w, sprite_h) = ((w + 2.0 * pad).ceil() as u32, (h + 2.0 * pad).ceil() as u32);
  let cover = imageops::resize(source, 96, 96, FilterType::Triangle);
  let side = cover.width() as f32;
  // `background-size: cover` sur un carré, centré.
  let fit = w.max(h) / side;
  let mut premultiplied = RgbaImage::new(sprite_w, sprite_h);
  for (x, y, pixel) in premultiplied.enumerate_pixels_mut() {
    let u = x as f32 + 0.5 - sprite_w as f32 / 2.0;
    let v = y as f32 + 0.5 - sprite_h as f32 / 2.0;
    let radius = ((u / (w / 2.0)).powi(2) + (v / (h / 2.0)).powi(2)).sqrt();
    // Bord de l'ellipse sur un pixel.
    let edge = ((1.0 - radius) * (w.min(h) / 2.0) + 0.5).clamp(0.0, 1.0);
    if edge <= 0.0 {
      continue;
    }
    let color = sample(&cover, u / fit + side / 2.0, v / fit + side / 2.0);
    let coverage = edge * color[3] / 255.0;
    pixel.0 = [
      (color[0] * coverage).round() as u8,
      (color[1] * coverage).round() as u8,
      (color[2] * coverage).round() as u8,
      (coverage * 255.0).round() as u8,
    ];
  }
  if sigma > 0.05 {
    premultiplied = imageops::fast_blur(&premultiplied, sigma);
  }
  // Retour à l'alpha droit, puis les trois filtres de couleur du conteneur.
  for pixel in premultiplied.pixels_mut() {
    let alpha = f32::from(pixel.0[3]) / 255.0;
    if alpha <= 0.0 {
      continue;
    }
    let straight = [0, 1, 2].map(|channel| (f32::from(pixel.0[channel]) / 255.0 / alpha).min(1.0));
    let luminance = 0.2126 * straight[0] + 0.7152 * straight[1] + 0.0722 * straight[2];
    for (channel, value) in straight.into_iter().enumerate() {
      let saturated = (luminance + (value - luminance) * AMBIENT_SATURATE).clamp(0.0, 1.0);
      pixel.0[channel] = (tone(saturated) * 255.0).clamp(0.0, 255.0).round() as u8;
    }
  }
  Decoded {
    width: premultiplied.width(),
    height: premultiplied.height(),
    rgba: premultiplied.into_raw(),
  }
}

/// Vignette de `.tv-karaoke::before` : du noir dont l'alpha monte du centre (50 %, 45 %) vers le coin le plus loin, sur toute la page.
fn shade(key: &BlurKey) -> Decoded {
  let (width, height, _) = ambient_space(key);
  let (width, height) = (width as u32, height as u32);
  let (centre_x, centre_y) = (width as f32 * 0.5, height as f32 * VIGNETTE.0);
  let farthest = (width as f32 - centre_x)
    .max(centre_x)
    .hypot((height as f32 - centre_y).max(centre_y));
  let mut image = RgbaImage::new(width, height);
  for (x, y, pixel) in image.enumerate_pixels_mut() {
    let distance = (x as f32 + 0.5 - centre_x).hypot(y as f32 + 0.5 - centre_y) / farthest;
    let alpha = VIGNETTE.1 + (VIGNETTE.2 - VIGNETTE.1) * distance.min(1.0);
    pixel.0 = [0, 0, 0, (alpha * 255.0).round() as u8];
  }
  Decoded {
    width,
    height,
    rgba: image.into_raw(),
  }
}

/// Résultat partagé.
pub type Blurred = Arc<Decoded>;

#[cfg(test)]
mod tests {
  use super::*;

  fn key(sigma_tenths: u32) -> BlurKey {
    BlurKey {
      url: "a".into(),
      sigma_tenths,
      aspect_tenths: 16,
      zoom_percent: 130,
      saturate_percent: 100,
      window_width: 1280,
      ambient: None,
    }
  }

  /// Moitié gauche noire, moitié droite blanche.
  fn split(side: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for _ in 0..side {
      for x in 0..side {
        let value = if x < side / 2 { 0 } else { 255 };
        rgba.extend_from_slice(&[value, value, value, 255]);
      }
    }
    rgba
  }

  #[test]
  fn the_result_has_the_target_size_and_the_aspect_of_the_window() {
    let blurred = compute(320, 320, &split(320), &key(880)).unwrap();
    assert_eq!((blurred.width, blurred.height), (TARGET_WIDTH, 125));
    assert_eq!(blurred.rgba.len(), (TARGET_WIDTH * 125 * 4) as usize);
  }

  #[test]
  fn a_wider_sigma_softens_the_edge_more() {
    let spread = |sigma: u32| {
      let image = compute(320, 320, &split(320), &key(sigma)).unwrap();
      let row = &image.rgba[(image.width * 4 * 60) as usize..(image.width * 4 * 61) as usize];
      row
        .chunks(4)
        .filter(|pixel| pixel[0] > 10 && pixel[0] < 245)
        .count()
    };
    assert!(spread(0) < spread(200), "{} {}", spread(0), spread(200));
    assert!(spread(200) < spread(880), "{} {}", spread(200), spread(880));
  }

  fn ambient_key(part: u8) -> BlurKey {
    let mut ambient = key(220);
    ambient.ambient = Some(part);
    ambient.window_width = 1920;
    ambient
  }

  #[test]
  fn the_shade_is_a_black_vignette_darker_in_the_corners_and_matches_the_window_aspect() {
    let image = compute(64, 64, &[255; 64 * 64 * 4], &ambient_key(SHADE_PART)).unwrap();
    assert_eq!((image.width, image.height), (AMBIENT_WIDTH, 160));
    let pixel = |x: u32, y: u32| {
      let at = ((y * image.width + x) * 4) as usize;
      <[u8; 4]>::try_from(&image.rgba[at..at + 4]).unwrap()
    };
    // `rgba(0,0,0,0.14)` au centre (50 %, 45 %), `rgba(0,0,0,0.48)` au coin le plus loin.
    let centre = pixel(128, 72);
    assert_eq!(&centre[..3], &[0, 0, 0]);
    assert!(
      (f32::from(centre[3]) / 255.0 - 0.14).abs() < 0.01,
      "{centre:?}"
    );
    let corner = pixel(255, 159);
    assert!(
      (f32::from(corner[3]) / 255.0 - 0.48).abs() < 0.02,
      "{corner:?}"
    );
  }

  #[test]
  fn an_orb_is_a_soft_ellipse_of_the_cover_with_the_blur_margin() {
    let image = compute(64, 64, &[255; 64 * 64 * 4], &ambient_key(0)).unwrap();
    let pixel = |x: u32, y: u32| {
      let at = ((y * image.width + x) * 4) as usize;
      <[u8; 4]>::try_from(&image.rgba[at..at + 4]).unwrap()
    };
    // `.b1` : 78 % de la page, plus la marge de 3 sigma du flou de chaque côté.
    assert!(image.width > (0.78 * AMBIENT_WIDTH as f32) as u32);
    assert!(image.height > (0.78 * 160.0) as u32);
    // Une pochette blanche : le centre reste opaque, les coins de l'image sont transparents, et `contrast(1.12)` puis `brightness(0.76)`
    // donnent 255 x 1,06 x 0,76 = 205.
    let (cx, cy) = (image.width / 2, image.height / 2);
    assert!(
      pixel(cx, cy)[3] > 250 && pixel(cx, cy)[0] == 205,
      "{:?}",
      pixel(cx, cy)
    );
    assert_eq!(pixel(0, 0)[3], 0);
  }

  #[test]
  fn the_ambient_base_is_the_filtered_dark_background() {
    let [r, g, b] = ambient_base();
    // `#0d0d12` : `saturate(1.95)` assombrit encore le bleu, `brightness(0.76)` et `contrast(1.12)` tirent vers le noir.
    assert!(r <= 13 && g <= 13 && b <= 18, "{r} {g} {b}");
    assert!(b >= r);
  }

  #[test]
  fn saturation_follows_the_css_matrix() {
    let mut image = RgbaImage::from_raw(1, 1, vec![200, 100, 50, 255]).unwrap();
    saturate(&mut image, 1.3);
    let luminance = 0.2126 * 200.0 + 0.7152 * 100.0 + 0.0722 * 50.0;
    let expected = (luminance + (200.0 - luminance) * 1.3) as u8;
    assert_eq!(image.get_pixel(0, 0).0[0], expected);
    assert_eq!(image.get_pixel(0, 0).0[3], 255);
  }

  #[test]
  fn invalid_data_gives_nothing() {
    assert!(compute(10, 10, &[0; 3], &key(10)).is_none());
  }
}
