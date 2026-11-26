//! Ambient cover-art wallpaper renderer (1:1 port of `wallpaper-sync-mod`).

use super::config::{MonitorTile, RenderParams};
use image::ImageEncoder;
use image::codecs::jpeg::JpegEncoder;
use std::f64::consts::PI;
use std::sync::OnceLock;

const NOISE_TILE_SIZE: usize = 512;
const NOISE_BASE_STDDEV: f64 = 64.0;
const MAX_DITHER_SIGMA: f64 = 24.0;

/// Packed RGB24 image buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbImage {
  pub width: u32,
  pub height: u32,
  pub data: Vec<u8>,
}

impl RgbImage {
  #[must_use]
  pub fn new(width: u32, height: u32) -> Self {
    let w = width.max(1);
    let h = height.max(1);
    Self {
      width: w,
      height: h,
      data: vec![0; w as usize * h as usize * 3],
    }
  }

  #[must_use]
  pub fn filled(width: u32, height: u32, rgb: [u8; 3]) -> Self {
    let mut img = Self::new(width, height);
    img.fill(rgb);
    img
  }

  pub fn fill(&mut self, rgb: [u8; 3]) {
    for px in self.data.as_chunks_mut::<3>().0 {
      *px = rgb;
    }
  }
}

#[inline]
fn clamp_u8(v: f64) -> u8 {
  if v <= 0.0 {
    0
  } else if v >= 255.0 {
    255
  } else {
    v.round() as u8
  }
}

/// Decodes raw image bytes (PNG/JPEG/WebP) into an RGB24 buffer.
///
/// # Errors
/// Returns an error when the bytes cannot be decoded or dimensions are zero.
pub fn decode_rgb(bytes: &[u8]) -> Result<RgbImage, image::ImageError> {
  let decoded = image::load_from_memory(bytes)?.into_rgb8();
  let (width, height) = (decoded.width(), decoded.height());
  if width == 0 || height == 0 {
    return Err(image::ImageError::Limits(
      image::error::LimitError::from_kind(image::error::LimitErrorKind::DimensionError),
    ));
  }
  Ok(RgbImage {
    width,
    height,
    data: decoded.into_raw(),
  })
}

/// Encodes an [`RgbImage`] as JPEG at quality 95 for the disk cache.
///
/// # Errors
/// Returns an error if the JPEG encoder fails.
pub fn encode_jpeg_95(img: &RgbImage) -> Result<Vec<u8>, image::ImageError> {
  let mut out = Vec::new();
  let encoder = JpegEncoder::new_with_quality(&mut out, 95);
  encoder.write_image(
    &img.data,
    img.width,
    img.height,
    image::ExtendedColorType::Rgb8,
  )?;
  Ok(out)
}

/// Encodes an [`RgbImage`] as an uncompressed 24-bit bottom-up Windows BMP.
#[must_use]
pub fn encode_bmp_24(img: &RgbImage) -> Vec<u8> {
  let row_bytes = img.width as usize * 3;
  let padded_row = row_bytes + (4 - (row_bytes % 4)) % 4;
  let pixel_bytes = padded_row * img.height as usize;
  let file_size = 54 + pixel_bytes;
  let mut buf = vec![0u8; file_size];

  buf[0..2].copy_from_slice(b"BM");
  buf[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
  buf[10..14].copy_from_slice(&54u32.to_le_bytes());
  buf[14..18].copy_from_slice(&40u32.to_le_bytes());
  buf[18..22].copy_from_slice(&(img.width as i32).to_le_bytes());
  buf[22..26].copy_from_slice(&(img.height as i32).to_le_bytes());
  buf[26..28].copy_from_slice(&1u16.to_le_bytes());
  buf[28..30].copy_from_slice(&24u16.to_le_bytes());
  buf[34..38].copy_from_slice(&(pixel_bytes as u32).to_le_bytes());
  buf[38..42].copy_from_slice(&2835i32.to_le_bytes());
  buf[42..46].copy_from_slice(&2835i32.to_le_bytes());

  let h = img.height as usize;
  for r in 0..h {
    let src_off = (h - 1 - r) * row_bytes;
    let dst_off = 54 + r * padded_row;
    let src_row = &img.data[src_off..src_off + row_bytes];
    let dst_row = &mut buf[dst_off..dst_off + row_bytes];
    for (s, d) in src_row
      .as_chunks::<3>()
      .0
      .iter()
      .zip(dst_row.as_chunks_mut::<3>().0)
    {
      d[0] = s[2];
      d[1] = s[1];
      d[2] = s[0];
    }
  }
  buf
}

/// Root-mean-square average RGB color of an image (`ie`).
#[must_use]
pub fn rms_average_color(img: &RgbImage) -> [u8; 3] {
  let count = (img.data.len() / 3).max(1) as f64;
  let (mut r, mut g, mut b) = (0f64, 0f64, 0f64);
  for px in img.data.as_chunks::<3>().0 {
    let pr = f64::from(px[0]);
    let pg = f64::from(px[1]);
    let pb = f64::from(px[2]);
    r += pr * pr;
    g += pg * pg;
    b += pb * pb;
  }
  [
    (r / count).sqrt().round() as u8,
    (g / count).sqrt().round() as u8,
    (b / count).sqrt().round() as u8,
  ]
}

#[inline]
fn perceived_luminance(rgb: [f64; 3]) -> f64 {
  (0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]) / 255.0
}

/// Iteratively darkens `rgb` by `step` until perceived luminance drops below `cap` (`oe`).
#[must_use]
pub fn darken_color(rgb: [u8; 3], cap: f64, step: f64) -> [u8; 3] {
  let step = step.clamp(0.1, 0.99);
  let mut r = f64::from(rgb[0]);
  let mut g = f64::from(rgb[1]);
  let mut b = f64::from(rgb[2]);
  while perceived_luminance([r, g, b]) >= cap {
    r *= step;
    g *= step;
    b *= step;
    if r.max(g).max(b) < 1.0 {
      break;
    }
  }
  [r.round() as u8, g.round() as u8, b.round() as u8]
}

#[must_use]
pub fn album_base_color(cover: &RgbImage, params: &RenderParams) -> [u8; 3] {
  darken_color(
    rms_average_color(cover),
    f64::from(params.luminosity_cap),
    f64::from(params.darken_step),
  )
}

#[must_use]
pub fn canvas_bg_color(cover: &RgbImage, params: &RenderParams) -> [u8; 3] {
  let base = album_base_color(cover, params);
  let b = f64::from(params.brightness);
  [
    clamp_u8(f64::from(base[0]) * b),
    clamp_u8(f64::from(base[1]) * b),
    clamp_u8(f64::from(base[2]) * b),
  ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
  Bilinear,
  Bicubic,
  Lanczos,
}

impl Filter {
  const fn support(self) -> f64 {
    match self {
      Self::Bilinear => 1.0,
      Self::Bicubic => 2.0,
      Self::Lanczos => 3.0,
    }
  }

  fn eval(self, x: f64) -> f64 {
    let ax = x.abs();
    match self {
      Self::Bilinear => {
        if ax < 1.0 {
          1.0 - ax
        } else {
          0.0
        }
      }
      Self::Bicubic => {
        let a = -0.5;
        if ax < 1.0 {
          (1.5 * ax - 2.5) * ax * ax + 1.0
        } else if ax < 2.0 {
          ((ax - 5.0) * ax + 8.0) * ax * a - 4.0 * a
        } else {
          0.0
        }
      }
      Self::Lanczos => {
        if ax < 3.0 {
          sinc(ax) * sinc(ax / 3.0)
        } else {
          0.0
        }
      }
    }
  }
}

#[inline]
fn sinc(x: f64) -> f64 {
  if x == 0.0 {
    1.0
  } else {
    let px = PI * x;
    px.sin() / px
  }
}

struct KernelTable {
  ksize: usize,
  bounds: Vec<usize>,
  weights: Vec<f64>,
}

fn build_kernels(src_len: u32, dst_len: u32, filter: Filter) -> KernelTable {
  let src_f = f64::from(src_len);
  let dst_usize = dst_len as usize;
  let ratio = src_f / f64::from(dst_len);
  let scale = ratio.max(1.0);
  let support = filter.support() * scale;
  let ksize = (support.ceil() as usize) * 2 + 1;
  let inv_scale = 1.0 / scale;

  let mut bounds = vec![0usize; dst_usize * 2];
  let mut weights = vec![0.0f64; dst_usize * ksize];

  for i in 0..dst_usize {
    let center = (i as f64 + 0.5) * ratio;
    let start = (center - support + 0.5).trunc().max(0.0) as usize;
    let end = ((center + support + 0.5).trunc() as usize).min(src_len as usize);
    let count = end.saturating_sub(start);
    let base = i * ksize;
    let mut sum = 0.0;
    for k in 0..count {
      let w = filter.eval(((start + k) as f64 - center + 0.5) * inv_scale);
      weights[base + k] = w;
      sum += w;
    }
    if sum != 0.0 {
      for k in 0..count {
        weights[base + k] /= sum;
      }
    }
    bounds[i * 2] = start;
    bounds[i * 2 + 1] = count;
  }

  KernelTable {
    ksize,
    bounds,
    weights,
  }
}

/// Separable resampling matching `D()` in `wallpaper-sync-mod`.
#[must_use]
pub fn resize(img: &RgbImage, dst_w: u32, dst_h: u32, filter: Filter) -> RgbImage {
  let dst_w = dst_w.max(1);
  let dst_h = dst_h.max(1);
  if img.width == dst_w && img.height == dst_h {
    return img.clone();
  }

  let horiz = if img.width == dst_w {
    None
  } else {
    let table = build_kernels(img.width, dst_w, filter);
    let mut out = RgbImage::new(dst_w, img.height);
    let src_stride = img.width as usize * 3;
    let dst_stride = dst_w as usize * 3;
    for y in 0..img.height as usize {
      let s_row = y * src_stride;
      let d_row = y * dst_stride;
      for x in 0..dst_w as usize {
        let start = table.bounds[x * 2];
        let count = table.bounds[x * 2 + 1];
        let w_base = x * table.ksize;
        let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
        for k in 0..count {
          let w = table.weights[w_base + k];
          let idx = s_row + (start + k) * 3;
          r += w * f64::from(img.data[idx]);
          g += w * f64::from(img.data[idx + 1]);
          b += w * f64::from(img.data[idx + 2]);
        }
        let d_idx = d_row + x * 3;
        out.data[d_idx] = clamp_u8(r);
        out.data[d_idx + 1] = clamp_u8(g);
        out.data[d_idx + 2] = clamp_u8(b);
      }
    }
    Some(out)
  };

  let mid = horiz.as_ref().unwrap_or(img);
  if mid.height == dst_h {
    return horiz.unwrap_or_else(|| img.clone());
  }

  let table = build_kernels(mid.height, dst_h, filter);
  let mut out = RgbImage::new(dst_w, dst_h);
  let stride = dst_w as usize * 3;
  for y in 0..dst_h as usize {
    let start = table.bounds[y * 2];
    let count = table.bounds[y * 2 + 1];
    let w_base = y * table.ksize;
    let d_row = y * stride;
    for x in 0..dst_w as usize {
      let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
      for k in 0..count {
        let w = table.weights[w_base + k];
        let idx = (start + k) * stride + x * 3;
        r += w * f64::from(mid.data[idx]);
        g += w * f64::from(mid.data[idx + 1]);
        b += w * f64::from(mid.data[idx + 2]);
      }
      let d_idx = d_row + x * 3;
      out.data[d_idx] = clamp_u8(r);
      out.data[d_idx + 1] = clamp_u8(g);
      out.data[d_idx + 2] = clamp_u8(b);
    }
  }
  out
}

fn crop(img: &RgbImage, x: u32, y: u32, w: u32, h: u32) -> RgbImage {
  let w = w.min(img.width.saturating_sub(x)).max(1);
  let h = h.min(img.height.saturating_sub(y)).max(1);
  let mut out = RgbImage::new(w, h);
  let src_stride = img.width as usize * 3;
  let row_bytes = w as usize * 3;
  for row in 0..h as usize {
    let src_off = (y as usize + row) * src_stride + x as usize * 3;
    let dst_off = row * row_bytes;
    out.data[dst_off..dst_off + row_bytes].copy_from_slice(&img.data[src_off..src_off + row_bytes]);
  }
  out
}

fn paste(dst: &mut RgbImage, src: &RgbImage, x: i32, y: i32) {
  let x0 = x.max(0) as u32;
  let y0 = y.max(0) as u32;
  if x0 >= dst.width || y0 >= dst.height {
    return;
  }
  let src_x0 = (-x).max(0) as u32;
  let src_y0 = (-y).max(0) as u32;
  if src_x0 >= src.width || src_y0 >= src.height {
    return;
  }
  let copy_w = (src.width - src_x0).min(dst.width - x0) as usize;
  let copy_h = (src.height - src_y0).min(dst.height - y0) as usize;
  let row_bytes = copy_w * 3;
  let src_stride = src.width as usize * 3;
  let dst_stride = dst.width as usize * 3;
  for row in 0..copy_h {
    let s_off = (src_y0 as usize + row) * src_stride + src_x0 as usize * 3;
    let d_off = (y0 as usize + row) * dst_stride + x0 as usize * 3;
    dst.data[d_off..d_off + row_bytes].copy_from_slice(&src.data[s_off..s_off + row_bytes]);
  }
}

fn cover_fit(img: &RgbImage, dst_w: u32, dst_h: u32) -> RgbImage {
  let src_ratio = f64::from(img.width) / f64::from(img.height);
  let dst_ratio = f64::from(dst_w) / f64::from(dst_h);
  let (scaled_w, scaled_h) = if src_ratio > dst_ratio {
    (
      ((f64::from(dst_h) * src_ratio).round() as u32).max(1),
      dst_h,
    )
  } else {
    (
      dst_w,
      ((f64::from(dst_w) / src_ratio).round() as u32).max(1),
    )
  };
  let scaled = resize(img, scaled_w, scaled_h, Filter::Lanczos);
  let cx = scaled_w.saturating_sub(dst_w) / 2;
  let cy = scaled_h.saturating_sub(dst_h) / 2;
  crop(&scaled, cx, cy, dst_w, dst_h)
}

fn box_blur_pass(img: &RgbImage, radius: f64, vertical: bool) -> RgbImage {
  let r_int = radius.trunc() as isize;
  let inv = 1.0 / (radius * 2.0 + 1.0);
  let edge_weight = (1.0 - ((r_int * 2 + 1) as f64) * inv) / 2.0;

  let mut out = RgbImage::new(img.width, img.height);
  let (outer_len, inner_len, step, stride) = if vertical {
    (
      img.width as usize,
      img.height as isize,
      img.width as usize * 3,
      3usize,
    )
  } else {
    (
      img.height as usize,
      img.width as isize,
      3usize,
      img.width as usize * 3,
    )
  };
  let max_idx = inner_len - 1;

  for outer in 0..outer_len {
    let base = outer * stride;
    for ch in 0..3 {
      let sample = |idx: isize| -> f64 {
        let clamped = idx.clamp(0, max_idx) as usize;
        f64::from(img.data[base + clamped * step + ch])
      };
      let mut acc = 0.0;
      for k in -r_int..=r_int {
        acc += sample(k);
      }
      for i in 0..=max_idx {
        let val = acc * inv + (sample(i - r_int - 1) + sample(i + r_int + 1)) * edge_weight;
        out.data[base + (i as usize) * step + ch] = clamp_u8(val);
        acc += sample(i + r_int + 1) - sample(i - r_int);
      }
    }
  }
  out
}

/// Three-pass fractional box blur approximating a Gaussian blur of standard deviation `sigma` (`pe`).
#[must_use]
pub fn gaussian_blur(img: &RgbImage, sigma: f64) -> RgbImage {
  if sigma <= 0.0 {
    return img.clone();
  }
  let n = sigma * sigma / 3.0;
  let rf = (((12.0 * n + 1.0).sqrt() - 1.0) / 2.0).floor();
  let radius =
    rf + (2.0 * rf + 1.0) * (rf * (rf + 1.0) - 3.0 * n) / (6.0 * (n - (rf + 1.0).powi(2)));
  let mut cur = img.clone();
  for _ in 0..3 {
    cur = box_blur_pass(&cur, radius, false);
  }
  for _ in 0..3 {
    cur = box_blur_pass(&cur, radius, true);
  }
  cur
}

fn flip_h(img: &RgbImage) -> RgbImage {
  let mut out = RgbImage::new(img.width, img.height);
  let w = img.width as usize;
  for y in 0..img.height as usize {
    for x in 0..w {
      let src = (y * w + (w - 1 - x)) * 3;
      let dst = (y * w + x) * 3;
      out.data[dst..dst + 3].copy_from_slice(&img.data[src..src + 3]);
    }
  }
  out
}

fn flip_v(img: &RgbImage) -> RgbImage {
  let mut out = RgbImage::new(img.width, img.height);
  let row_bytes = img.width as usize * 3;
  let h = img.height as usize;
  for y in 0..h {
    let src = (h - 1 - y) * row_bytes;
    let dst = y * row_bytes;
    out.data[dst..dst + row_bytes].copy_from_slice(&img.data[src..src + row_bytes]);
  }
  out
}

fn noise_tiles() -> &'static [RgbImage; 4] {
  static TILES: OnceLock<[RgbImage; 4]> = OnceLock::new();
  TILES.get_or_init(|| {
    let mut s = [24301u32, 608135816, 3084996962, 2654435769];
    let mut next_u01 = || -> f64 {
      let t = s[0] ^ (s[0] << 11);
      s[0] = s[1];
      s[1] = s[2];
      s[2] = s[3];
      s[3] = s[3] ^ (s[3] >> 19) ^ (t ^ (t >> 8));
      f64::from(s[3]) / 4_294_967_296.0
    };
    let mut base = RgbImage::new(NOISE_TILE_SIZE as u32, NOISE_TILE_SIZE as u32);
    for b in &mut base.data {
      let u1 = next_u01().max(1e-12);
      let u2 = next_u01();
      let z = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
      *b = clamp_u8(128.0 + z * NOISE_BASE_STDDEV);
    }
    let h = flip_h(&base);
    let v = flip_v(&base);
    let hv = flip_v(&h);
    [base, h, v, hv]
  })
}

/// Applies deterministic tiled Gaussian dithering to prevent 8-bit gradient banding (`xe`).
pub fn apply_dither(img: &mut RgbImage, sigma: f64) {
  if sigma <= 0.0 {
    return;
  }
  let scale = (sigma.min(MAX_DITHER_SIGMA) / NOISE_BASE_STDDEV).min(1.0);
  let mut delta_lut = [0i16; 256];
  for (v, slot) in delta_lut.iter_mut().enumerate() {
    let scaled = clamp_u8(128.0 + (v as f64 - 128.0) * scale);
    *slot = i16::from(scaled) - 128;
  }

  let tiles = noise_tiles();
  let w = img.width as usize;
  let h = img.height as usize;

  let mut ty = 0usize;
  let mut y = 0usize;
  while y < h {
    let bh = NOISE_TILE_SIZE.min(h - y);
    let mut tx = 0usize;
    let mut x = 0usize;
    while x < w {
      let bw = NOISE_TILE_SIZE.min(w - x);
      let tile = &tiles[(ty * 3 + tx) % 4];
      for row in 0..bh {
        let t_off = row * NOISE_TILE_SIZE * 3;
        let i_off = ((y + row) * w + x) * 3;
        let len = bw * 3;
        let dst = &mut img.data[i_off..i_off + len];
        let noise = &tile.data[t_off..t_off + len];
        for (px, &n) in dst.iter_mut().zip(noise.iter()) {
          let sum = i16::from(*px) + delta_lut[n as usize];
          *px = sum.clamp(0, 255) as u8;
        }
      }
      x += NOISE_TILE_SIZE;
      tx += 1;
    }
    y += NOISE_TILE_SIZE;
    ty += 1;
  }
}

/// Applies album-color mix and global brightness using a 3x256 LUT (`we`).
pub fn apply_color_mix_and_brightness(
  img: &mut RgbImage,
  bg: [u8; 3],
  color_mix_ratio: u8,
  brightness: f64,
) {
  let mix = (f64::from(color_mix_ratio) / 100.0).clamp(0.0, 1.0);
  if mix == 0.0 && (brightness - 1.0).abs() < f64::EPSILON {
    return;
  }
  let keep = (1.0 - mix) * brightness;
  let mut lut = [0u8; 768];
  for c in 0..3 {
    let add = f64::from(bg[c]) * mix * brightness;
    for v in 0..256 {
      lut[c * 256 + v] = clamp_u8(v as f64 * keep + add);
    }
  }
  for px in img.data.as_chunks_mut::<3>().0 {
    px[0] = lut[px[0] as usize];
    px[1] = lut[256 + px[1] as usize];
    px[2] = lut[512 + px[2] as usize];
  }
}

/// Renders one monitor canvas (`F`).
#[must_use]
pub fn render_single(cover: &RgbImage, width: u32, height: u32, params: &RenderParams) -> RgbImage {
  let width = width.max(1);
  let height = height.max(1);
  let scale = width
    .min(height)
    .checked_div(params.gradient_height)
    .map_or(1, |s| s.max(1));
  let gw = ((f64::from(width) / f64::from(scale)).round() as u32).max(1);
  let gh = ((f64::from(height) / f64::from(scale)).round() as u32).max(1);

  let bg = album_base_color(cover, params);
  let mut canvas = RgbImage::filled(gw, gh, bg);

  let glow_w =
    ((f64::from(gw) * f64::from(params.glow_size_percent) / 100.0).trunc() as u32).max(1);
  let glow_h =
    ((f64::from(gh) * f64::from(params.glow_size_percent) / 100.0).trunc() as u32).max(1);

  let mut glow = cover_fit(cover, glow_w, glow_h);
  let qw = params.quality_px.min(glow_w).max(1);
  let qh = ((f64::from(qw) * f64::from(glow_h) / f64::from(glow_w)).round() as u32).max(1);
  glow = resize(&glow, qw, qh, Filter::Bilinear);
  glow = resize(&glow, glow_w, glow_h, Filter::Bilinear);

  if params.glow_opacity < 1.0 {
    let op = f64::from(params.glow_opacity);
    for px in glow.data.as_chunks_mut::<3>().0 {
      for c in 0..3 {
        let base = f64::from(bg[c]);
        px[c] = clamp_u8(base + (f64::from(px[c]) - base) * op);
      }
    }
  }

  let ox = (gw as i32 - glow_w as i32).div_euclid(2);
  let oy = (gh as i32 - glow_h as i32).div_euclid(2);
  paste(&mut canvas, &glow, ox, oy);

  let blur_sigma =
    f64::from(params.blur_px) * f64::from(gw) / f64::from(params.blur_ref_width.max(1));
  canvas = gaussian_blur(&canvas, blur_sigma);
  if gw != width || gh != height {
    canvas = resize(&canvas, width, height, Filter::Bicubic);
  }

  if params.dither > 0.0 {
    let eff =
      ((1.0 - f64::from(params.color_mix_ratio) / 100.0) * f64::from(params.brightness)).max(0.01);
    apply_dither(&mut canvas, f64::from(params.dither) / eff);
  }
  apply_color_mix_and_brightness(
    &mut canvas,
    bg,
    params.color_mix_ratio,
    f64::from(params.brightness),
  );
  canvas
}

/// Renders one ambient glow per monitor tile and composes them onto the virtual desktop canvas (`Te`).
#[must_use]
pub fn render_span(
  cover: &RgbImage,
  width: u32,
  height: u32,
  tiles: &[MonitorTile],
  params: &RenderParams,
) -> RgbImage {
  let mut canvas = RgbImage::filled(width.max(1), height.max(1), canvas_bg_color(cover, params));
  for tile in tiles {
    if tile.width == 0 || tile.height == 0 {
      continue;
    }
    let rendered = render_single(cover, tile.width, tile.height, params);
    paste(&mut canvas, &rendered, tile.x, tile.y);
  }
  canvas
}

#[cfg(test)]
mod tests {
  use super::super::config::WallpaperSyncConfig;
  use super::*;

  fn sample_cover() -> RgbImage {
    let mut img = RgbImage::new(64, 64);
    for y in 0..64u32 {
      for x in 0..64u32 {
        let i = (y as usize * 64 + x as usize) * 3;
        img.data[i] = (x * 4).min(255) as u8;
        img.data[i + 1] = (y * 4).min(255) as u8;
        img.data[i + 2] = 180;
      }
    }
    img
  }

  #[test]
  fn darken_color_caps_perceived_luminance() {
    let dark = darken_color([255, 255, 255], 0.5, 0.9);
    let lum = perceived_luminance([f64::from(dark[0]), f64::from(dark[1]), f64::from(dark[2])]);
    assert!(lum < 0.5, "luminance {lum}");
    assert_eq!(darken_color([10, 20, 30], 0.5, 0.9), [10, 20, 30]);
  }

  #[test]
  fn resizes_produce_requested_dimensions_and_preserve_solid_colors() {
    let solid = RgbImage::filled(32, 24, [42, 128, 210]);
    for filter in [Filter::Bilinear, Filter::Bicubic, Filter::Lanczos] {
      let out = resize(&solid, 48, 30, filter);
      assert_eq!((out.width, out.height), (48, 30));
      assert!(
        out
          .data
          .as_chunks::<3>()
          .0
          .iter()
          .all(|p| *p == [42, 128, 210])
      );
    }
  }

  #[test]
  fn gaussian_blur_smooths_step_edge() {
    let mut img = RgbImage::new(21, 1);
    for x in 10..21 {
      img.data[x * 3] = 200;
    }
    let blurred = gaussian_blur(&img, 3.0);
    assert!(blurred.data[0] < blurred.data[10 * 3]);
    assert!(blurred.data[10 * 3] < 200);
  }

  #[test]
  fn dither_is_deterministic_and_modifies_flat_surface_subtly() {
    let mut a = RgbImage::filled(64, 64, [100, 100, 100]);
    let mut b = a.clone();
    apply_dither(&mut a, 4.0);
    apply_dither(&mut b, 4.0);
    assert_eq!(a, b);
    assert!(a.data.iter().any(|&v| v != 100));
    assert!(a.data.iter().all(|&v| (80..=120).contains(&v)));
  }

  #[test]
  fn render_single_and_span_produce_expected_sizes() {
    let cover = sample_cover();
    let params = WallpaperSyncConfig::default().render_params();
    let single = render_single(&cover, 320, 180, &params);
    assert_eq!((single.width, single.height), (320, 180));
    assert_eq!(single.data.len(), 320 * 180 * 3);

    let tiles = [
      MonitorTile {
        x: 0,
        y: 0,
        width: 160,
        height: 120,
      },
      MonitorTile {
        x: 160,
        y: 0,
        width: 160,
        height: 120,
      },
    ];
    let span = render_span(&cover, 320, 120, &tiles, &params);
    assert_eq!((span.width, span.height), (320, 120));
    assert_eq!(span.data.len(), 320 * 120 * 3);
  }

  #[test]
  fn bmp_24_writes_valid_header_padding_and_bottom_up_bgr() {
    // 1x2 image (row = 3 bytes -> padded to 4 bytes; total pixels = 8 bytes).
    let img = RgbImage {
      width: 1,
      height: 2,
      data: vec![
        10, 20, 30, // top row RGB
        40, 50, 60, // bottom row RGB
      ],
    };
    let bmp = encode_bmp_24(&img);
    assert_eq!(&bmp[0..2], b"BM");
    assert_eq!(bmp.len(), 54 + 8);
    assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()), 62);
    // Bottom-up: row 0 in BMP is bottom row in BGR.
    assert_eq!(&bmp[54..58], &[60, 50, 40, 0]);
    assert_eq!(&bmp[58..62], &[30, 20, 10, 0]);
  }

  #[test]
  fn default_preset_preserves_spatial_color_variation() {
    // High-contrast 4-quadrant cover (purple, teal, gold, magenta) rendered at 320x180.
    let mut cover = RgbImage::new(64, 64);
    for y in 0..64u32 {
      for x in 0..64u32 {
        let idx = (y as usize * 64 + x as usize) * 3;
        let rgb = match (x < 32, y < 32) {
          (true, true) => [90, 40, 200],
          (false, true) => [20, 180, 190],
          (true, false) => [230, 140, 40],
          (false, false) => [210, 50, 140],
        };
        cover.data[idx..idx + 3].copy_from_slice(&rgb);
      }
    }
    let params = WallpaperSyncConfig::default().render_params();
    let out = render_single(&cover, 320, 180, &params);
    let mut min_c = [255u8; 3];
    let mut max_c = [0u8; 3];
    for px in out.data.as_chunks::<3>().0 {
      for c in 0..3 {
        min_c[c] = min_c[c].min(px[c]);
        max_c[c] = max_c[c].max(px[c]);
      }
    }
    let span = (0..3).map(|c| max_c[c] - min_c[c]).max().unwrap_or(0);
    assert!(
      span >= 20,
      "expected visible spatial variation, got span={span}"
    );
  }
}
