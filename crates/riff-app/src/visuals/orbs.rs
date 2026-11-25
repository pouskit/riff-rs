//! Orbes du fond de la vue plein écran (`.tvk-ambient-blob.b1-b4`, `tvkOrbit1-4`, `styles.css:6323-6384`) : quatre ellipses de la
//! pochette qui dérivent sur l'horloge de `App`. Le flou du conteneur (`filter: blur`) est précalculé dans chaque orbe
//! (`visuals::blur`) : iced ne floute pas un calque, et un flou refait à chaque image coûterait trop. Le dessin reçoit le temps
//! écoulé, jamais une horloge propre : le banc rend n'importe quel instant.

use super::blur::AMBIENT_WIDTH;
use super::state::{Layer, bezier_ease, crossfade_opacities};
use crate::theme::tokens::karaoke::{
  ORB_PRESENCE, ORBIT_CURVE, ORBS, Orb, OrbitFrame, VIGNETTE_TYPICAL,
};
use iced::widget::canvas::{self, Frame, Geometry, Image};
use iced::widget::image::{FilterMethod, Handle};
use iced::{Radians, Rectangle, Renderer, Size, Theme, Vector, mouse};

/// Image clé de `orb` à `seconds` d'animation. `ease-in-out infinite alternate` : l'itération paire va de 0 à 100 %, l'impaire revient
/// de 100 à 0 % ; la courbe s'applique à chaque segment entre deux images clés (`@keyframes` CSS), puis `translate3d`, `scale` et
/// `rotate` s'interpolent terme à terme.
#[must_use]
pub fn orbit_pose(orb: &Orb, seconds: f32) -> OrbitFrame {
  let cycles = seconds.max(0.0) / orb.seconds;
  let iteration = cycles.floor();
  let phase = cycles - iteration;
  let progress = if iteration % 2.0 < 1.0 {
    phase
  } else {
    1.0 - phase
  };
  let (from, to, local) = if progress < 0.5 {
    (orb.frames[0], orb.frames[1], progress * 2.0)
  } else {
    (orb.frames[1], orb.frames[2], progress * 2.0 - 1.0)
  };
  let eased = bezier_ease(ORBIT_CURVE, local);
  let mix = |a: f32, b: f32| a + (b - a) * eased;
  OrbitFrame {
    x: mix(from.x, to.x),
    y: mix(from.y, to.y),
    scale: mix(from.scale, to.scale),
    degrees: mix(from.degrees, to.degrees),
  }
}

/// Opacités du fond pendant `tvk-in` et `tvk-out`. Riff compose le fond (aplat opaque, quatre orbes, vignette) puis fond le résultat à
/// `fade` sur l'écran du dessous : la part du dessous est `1 - fade` partout et chaque couleur du fond pèse `fade` fois son poids final.
/// iced n'a pas d'opacité de groupe : les calques se superposent chacun sur le dessous. Pour retrouver les mêmes poids on règle chaque
/// calque, du dessus vers le dessous, pour que `k * a * Π(1 - k' * a')` des calques au-dessus vaille `fade` fois son poids final
/// `a * Π(1 - a')`. Les opacités locales `a` (orbes floutés, vignette en dégradé) varient avec l'endroit : on prend leur valeur
/// typique (`ORB_PRESENCE`, `VIGNETTE_TYPICAL`), donc l'opacité effective est exacte pour un pixel typique seulement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupOpacity {
  /// Aplat opaque sous les orbes.
  pub base: f32,
  /// Facteur sur l'opacité de chaque orbe.
  pub orbs: [f32; 4],
  /// Facteur sur l'opacité de la vignette.
  pub vignette: f32,
}

impl GroupOpacity {
  /// Calques du fond, du dessus vers le dessous : opacité typique à pleine opacité de groupe.
  fn typical() -> [f32; 5] {
    [
      VIGNETTE_TYPICAL,
      ORBS[3].opacity * ORB_PRESENCE,
      ORBS[2].opacity * ORB_PRESENCE,
      ORBS[1].opacity * ORB_PRESENCE,
      ORBS[0].opacity * ORB_PRESENCE,
    ]
  }

  /// Opacités pour l'opacité de groupe `fade` (1 au repos).
  #[must_use]
  pub fn at(fade: f32) -> Self {
    let fade = fade.clamp(0.0, 1.0);
    let typical = Self::typical();
    // `kept` : produit des transparences des calques déjà posés au-dessus, à pleine opacité puis à l'opacité réglée.
    let (mut full, mut set) = (1.0_f32, 1.0_f32);
    let mut factors = [0.0_f32; 5];
    for (factor, alpha) in factors.iter_mut().zip(typical) {
      *factor = (fade * full / set).clamp(0.0, 1.0);
      full *= 1.0 - alpha;
      set *= 1.0 - *factor * alpha;
    }
    Self {
      base: (fade * full / set).clamp(0.0, 1.0),
      orbs: [factors[4], factors[3], factors[2], factors[1]],
      vignette: factors[0],
    }
  }
}

/// Rang de la vignette parmi les calques du fond.
const SHADE_INDEX: usize = 4;

/// Image d'un orbe et son opacité de dessin.
#[derive(Debug, Clone)]
struct Sprite {
  handle: Handle,
  opacity: f32,
}

/// Les orbes du moment : pour chacun, l'image qui s'efface (dessous) et l'image montrée (dessus) pendant le fondu de 600 ms au
/// changement de pochette (`transition: background-image 600ms ease`, `styles.css:6333`).
#[derive(Debug, Clone)]
pub struct Orbs {
  sprites: [[Option<Sprite>; 2]; 4],
  seconds: f32,
  /// Hauteur du conteneur dans l'image de sortie, pour les orbes à la proportion de la page.
  baked_height: Option<f32>,
}

impl Orbs {
  /// `layers` : les quatre orbes puis la vignette, dont la hauteur donne celle du conteneur cuit. `factors` : facteur de chaque orbe
  /// sur son opacité (`GroupOpacity`, pendant `tvk-in` et `tvk-out`).
  #[must_use]
  pub fn new(layers: &[Layer; 5], seconds: f32, factors: [f32; 4]) -> Self {
    let sprites = std::array::from_fn(|index| {
      Self::sprites_of(&layers[index], ORBS[index].opacity * factors[index])
    });
    let baked_height = layers[SHADE_INDEX]
      .current
      .as_ref()
      .and_then(|handle| match handle {
        Handle::Rgba { height, .. } => Some(*height as f32),
        _ => None,
      });
    Self {
      sprites,
      seconds,
      baked_height,
    }
  }

  /// Les images d'un calque : l'ancienne (dessous) et la montrée (dessus) pendant le fondu de changement de pochette, à `opacity`.
  fn sprites_of(layer: &Layer, opacity: f32) -> [Option<Sprite>; 2] {
    let sprite = |handle: &Handle, opacity: f32| {
      Some(Sprite {
        handle: handle.clone(),
        opacity,
      })
    };
    let Some(fading) = &layer.fade else {
      return [
        None,
        layer
          .current
          .as_ref()
          .and_then(|handle| sprite(handle, opacity)),
      ];
    };
    match (&fading.from, &layer.current) {
      (None, None) => [None, None],
      (Some(from), None) => [sprite(from, opacity * (1.0 - fading.progress)), None],
      (None, Some(current)) => [None, sprite(current, opacity * fading.progress)],
      (Some(from), Some(current)) => {
        let (below, top) = crossfade_opacities(opacity, fading.progress);
        [sprite(from, below), sprite(current, top)]
      }
    }
  }

  /// Rien à dessiner : aucun orbe n'a d'image.
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.sprites.iter().flatten().all(Option::is_none)
  }
}

/// Un échantillon bilinéaire en alpha droit d'une image RGBA, coordonnées de texel (centre du texel à `.5`), bords prolongés.
fn texel(pixels: &[u8], width: u32, height: u32, x: f32, y: f32) -> [f32; 4] {
  let (x, y) = (x - 0.5, y - 0.5);
  let (x0, y0) = (x.floor(), y.floor());
  let (fx, fy) = (x - x0, y - y0);
  let at = |dx: f32, dy: f32| {
    let column = (x0 + dx).clamp(0.0, width as f32 - 1.0) as usize;
    let row = (y0 + dy).clamp(0.0, height as f32 - 1.0) as usize;
    let start = (row * width as usize + column) * 4;
    [0, 1, 2, 3].map(|channel| f32::from(pixels[start + channel]))
  };
  let (a, b, c, d) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
  [0, 1, 2, 3].map(|channel| {
    let top = a[channel] + (b[channel] - a[channel]) * fx;
    let bottom = c[channel] + (d[channel] - c[channel]) * fx;
    top + (bottom - top) * fy
  })
}

impl Orbs {
  /// Fond composé en une seule image : l'aplat `base`, les orbes tels que `draw` les place, puis la vignette `shade` étirée sur
  /// `size`, échantillonnés au centre de cellules de `step` pixels (le fond est flou, sans détail fin). C'est le « dessous » d'une
  /// opacité de groupe : une image unique dessinée à `1 - opacité` par-dessus le groupe donne `groupe * opacité + dessous * (1 -
  /// opacité)` en chaque pixel, ce que des calques superposés ne donnent qu'en un pixel typique (voir `GroupOpacity`).
  #[must_use]
  pub fn flatten(&self, base: [u8; 3], shade: &Layer, size: Size, step: f32) -> Handle {
    let columns = (size.width / step).ceil().max(1.0) as u32;
    let rows = (size.height / step).ceil().max(1.0) as u32;
    let (cell_w, cell_h) = (size.width / columns as f32, size.height / rows as f32);
    let kx = size.width / AMBIENT_WIDTH as f32;
    let ky = self.baked_height.map_or(kx, |baked| size.height / baked);
    // Pour chaque image d'orbe : centre, échelle, sinus et cosinus de la rotation, taille dessinée.
    struct Placed<'a> {
      centre: Vector,
      scale: f32,
      sin: f32,
      cos: f32,
      width: f32,
      height: f32,
      pixels: (&'a [u8], u32, u32),
      opacity: f32,
    }
    let mut placed = Vec::new();
    for (orb, sprites) in ORBS.iter().zip(&self.sprites) {
      let pose = orbit_pose(orb, self.seconds);
      let centre = Vector::new(
        (orb.left + orb.width / 2.0 + pose.x * orb.width) * size.width,
        (orb.top + orb.height / 2.0 + pose.y * orb.height) * size.height,
      );
      let (sin, cos) = pose.degrees.to_radians().sin_cos();
      for sprite in sprites.iter().flatten() {
        let Handle::Rgba {
          width,
          height,
          pixels,
          ..
        } = &sprite.handle
        else {
          continue;
        };
        placed.push(Placed {
          centre,
          scale: pose.scale,
          sin,
          cos,
          width: *width as f32 * kx,
          height: *height as f32 * ky,
          pixels: (pixels, *width, *height),
          opacity: sprite.opacity,
        });
      }
    }
    let shade = Self::sprites_of(shade, 1.0);
    let mut out = Vec::with_capacity((columns * rows * 4) as usize);
    for row in 0..rows {
      for column in 0..columns {
        let (x, y) = ((column as f32 + 0.5) * cell_w, (row as f32 + 0.5) * cell_h);
        let mut color = base.map(f32::from);
        let mut over = |sample: [f32; 3], alpha: f32| {
          for channel in 0..3 {
            color[channel] = sample[channel] * alpha + color[channel] * (1.0 - alpha);
          }
        };
        for orb in &placed {
          // Inverse de `translate`, `scale` puis `rotate`.
          let (dx, dy) = (x - orb.centre.x, y - orb.centre.y);
          let u = (dx * orb.cos + dy * orb.sin) / orb.scale;
          let v = (-dx * orb.sin + dy * orb.cos) / orb.scale;
          if u.abs() >= orb.width / 2.0 || v.abs() >= orb.height / 2.0 {
            continue;
          }
          let (pixels, pw, ph) = orb.pixels;
          let [r, g, b, a] = texel(
            pixels,
            pw,
            ph,
            (u / orb.width + 0.5) * pw as f32,
            (v / orb.height + 0.5) * ph as f32,
          );
          over([r, g, b], a / 255.0 * orb.opacity);
        }
        for sprite in shade.iter().flatten() {
          let Handle::Rgba {
            width,
            height,
            pixels,
            ..
          } = &sprite.handle
          else {
            continue;
          };
          let [r, g, b, a] = texel(
            pixels,
            *width,
            *height,
            x / size.width * *width as f32,
            y / size.height * *height as f32,
          );
          over([r, g, b], a / 255.0 * sprite.opacity);
        }
        out.extend(color.map(|value| value.round().clamp(0.0, 255.0) as u8));
        out.push(255);
      }
    }
    Handle::from_rgba(columns, rows, out)
  }
}

impl<Message> canvas::Program<Message> for Orbs {
  type State = ();

  fn draw(
    &self,
    _state: &(),
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let mut frame = Frame::new(renderer, bounds.size());
    let Size { width, height } = bounds.size();
    // Les images sont cuites dans le conteneur de `AMBIENT_WIDTH` pixels sur `baked_height` : elles s'étirent jusqu'à la fenêtre.
    let (kx, ky) = (
      width / AMBIENT_WIDTH as f32,
      self
        .baked_height
        .map_or(width / AMBIENT_WIDTH as f32, |baked| height / baked),
    );
    for (orb, sprites) in ORBS.iter().zip(&self.sprites) {
      let pose = orbit_pose(orb, self.seconds);
      // `translate3d(x%, y%)` : parts de la taille de l'orbe, hors de l'échelle.
      let centre = Vector::new(
        (orb.left + orb.width / 2.0 + pose.x * orb.width) * width,
        (orb.top + orb.height / 2.0 + pose.y * orb.height) * height,
      );
      for sprite in sprites.iter().flatten() {
        let Handle::Rgba {
          width: pixels_w,
          height: pixels_h,
          ..
        } = &sprite.handle
        else {
          continue;
        };
        let (w, h) = (*pixels_w as f32 * kx, *pixels_h as f32 * ky);
        frame.with_save(|frame| {
          // `transform-origin` au centre : translation, échelle puis rotation, comme `translate3d() scale() rotate()`.
          frame.translate(centre);
          frame.scale(pose.scale);
          frame.rotate(Radians(pose.degrees.to_radians()));
          frame.draw_image(
            Rectangle::new(iced::Point::new(-w / 2.0, -h / 2.0), Size::new(w, h)),
            Image::new(sprite.handle.clone())
              .opacity(sprite.opacity)
              .filter_method(FilterMethod::Linear),
          );
        });
      }
    }
    vec![frame.into_geometry()]
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
  }

  #[test]
  fn an_orbit_starts_on_its_first_keyframe() {
    for orb in &ORBS {
      assert_eq!(orbit_pose(orb, 0.0), orb.frames[0]);
    }
  }

  #[test]
  fn an_orbit_reaches_the_middle_keyframe_at_half_the_duration_and_the_last_at_the_end() {
    for orb in &ORBS {
      let half = orbit_pose(orb, orb.seconds / 2.0);
      let end = orbit_pose(orb, orb.seconds);
      assert!(close(half.x, orb.frames[1].x) && close(half.degrees, orb.frames[1].degrees));
      assert!(close(end.x, orb.frames[2].x) && close(end.scale, orb.frames[2].scale));
    }
  }

  #[test]
  fn the_curve_applies_to_each_segment_not_to_the_whole() {
    // `.b2` : 4 s, images clés à 0, 2 et 4 s. À 1 s, la moitié du premier segment : `ease-in-out` vaut 0,5 au milieu, donc la
    // moitié du chemin ; à 0,5 s, un quart du segment : bien moins qu'un quart.
    let orb = &ORBS[1];
    let middle = orbit_pose(orb, 1.0);
    assert!(close(middle.x, orb.frames[1].x / 2.0), "{middle:?}");
    let early = orbit_pose(orb, 0.5);
    assert!(early.x.abs() < orb.frames[1].x.abs() * 0.25, "{early:?}");
    // Au milieu du second segment (3 s), le milieu des deux dernières images clés.
    let late = orbit_pose(orb, 3.0);
    assert!(
      close(late.y, f32::midpoint(orb.frames[1].y, orb.frames[2].y)),
      "{late:?}"
    );
  }

  #[test]
  fn the_alternate_direction_comes_back_on_the_next_iteration() {
    let orb = &ORBS[2];
    // Le deuxième cycle de `.b3` (6 s) revient vers la première image clé : 7,5 s et 4,5 s sont symétriques.
    let back = orbit_pose(orb, 6.0 + 1.5);
    let forward = orbit_pose(orb, 6.0 - 1.5);
    assert!(
      close(back.x, forward.x) && close(back.scale, forward.scale),
      "{back:?} {forward:?}"
    );
    assert!(
      close(orbit_pose(orb, 12.0).x, 0.0),
      "deux cycles : de retour au départ"
    );
    let third = orbit_pose(orb, 12.0 + 1.5);
    let first = orbit_pose(orb, 1.5);
    assert!(close(third.x, first.x));
  }

  #[test]
  fn the_group_opacity_is_full_at_rest_and_empty_before_the_start() {
    let rest = GroupOpacity::at(1.0);
    assert!(close(rest.base, 1.0) && close(rest.vignette, 1.0));
    assert!(rest.orbs.iter().all(|factor| close(*factor, 1.0)));
    let start = GroupOpacity::at(0.0);
    assert!(close(start.base, 0.0) && close(start.vignette, 0.0));
    assert!(start.orbs.iter().all(|factor| close(*factor, 0.0)));
  }

  #[test]
  fn what_shows_through_the_background_is_one_minus_the_group_opacity() {
    // Pixel typique : la part du dessous est le produit des transparences de tous les calques, comme dans Riff où le groupe se fond
    // à `fade` sur l'écran du dessous (et non à `(1 - fade)^2` avec deux aplats opaques).
    let typical = GroupOpacity::typical();
    for fade in [0.1, 0.25, 0.5, 0.75, 0.9] {
      let group = GroupOpacity::at(fade);
      let factors = [
        group.vignette,
        group.orbs[3],
        group.orbs[2],
        group.orbs[1],
        group.orbs[0],
      ];
      let through = factors
        .iter()
        .zip(typical)
        .fold(1.0 - group.base, |share, (factor, alpha)| {
          share * (1.0 - factor * alpha)
        });
      assert!((through - (1.0 - fade)).abs() < 1e-3, "{fade}: {through}");
    }
  }

  #[test]
  fn the_group_opacity_grows_with_the_fade() {
    let mut previous = GroupOpacity::at(0.0);
    for step in 1..=20 {
      let group = GroupOpacity::at(step as f32 / 20.0);
      assert!(group.base >= previous.base - 1e-6, "{step}");
      assert!(group.vignette >= previous.vignette - 1e-6, "{step}");
      previous = group;
    }
  }

  fn pixel(handle: &Handle, width: u32, x: u32, y: u32) -> [u8; 4] {
    let Handle::Rgba { pixels, .. } = handle else {
      panic!("image RGBA attendue");
    };
    let start = ((y * width + x) * 4) as usize;
    [
      pixels[start],
      pixels[start + 1],
      pixels[start + 2],
      pixels[start + 3],
    ]
  }

  #[test]
  fn the_flat_background_is_the_base_where_nothing_is_drawn() {
    let layers = <[Layer; 5]>::default();
    let flat = Orbs::new(&layers, 0.0, [1.0; 4]).flatten(
      [100, 50, 20],
      &layers[4],
      Size::new(64.0, 32.0),
      8.0,
    );
    let Handle::Rgba { width, height, .. } = &flat else {
      panic!("image RGBA attendue");
    };
    assert_eq!((*width, *height), (8, 4));
    assert_eq!(pixel(&flat, 8, 3, 2), [100, 50, 20, 255]);
  }

  #[test]
  fn the_flat_background_composes_an_orb_then_the_vignette_like_the_layers_do() {
    let mut layers = <[Layer; 5]>::default();
    // Un orbe rouge opaque de 100 x 100 texels, la vignette en noir à 50 % (alpha droit).
    layers[0].current = Some(Handle::from_rgba(
      100,
      100,
      [255, 0, 0, 255].repeat(100 * 100),
    ));
    layers[4].current = Some(Handle::from_rgba(1, 1, vec![0, 0, 0, 128]));
    let size = Size::new(256.0, 256.0);
    let flat = Orbs::new(&layers, 0.0, [1.0; 4]).flatten([100, 50, 20], &layers[4], size, 1.0);
    // Centre de `.b1` au départ : `(left + width / 2) * largeur`, `(top + height / 2) * hauteur`, soit (53,76 ; 43,52).
    let orb = 0.67_f32;
    let shade = 128.0 / 255.0;
    let expected = |base: f32, orb_color: f32| {
      ((orb_color * orb + base * (1.0 - orb)) * (1.0 - shade)).round() as u8
    };
    assert_eq!(
      pixel(&flat, 256, 53, 43),
      [
        expected(100.0, 255.0),
        expected(50.0, 0.0),
        expected(20.0, 0.0),
        255
      ]
    );
    // Loin de l'orbe : seulement la vignette sur l'aplat.
    assert_eq!(pixel(&flat, 256, 200, 200), [50, 25, 10, 255]);
  }

  #[test]
  fn no_image_means_nothing_to_draw() {
    let layers = <[Layer; 5]>::default();
    assert!(Orbs::new(&layers, 0.0, [1.0; 4]).is_empty());
  }
}
