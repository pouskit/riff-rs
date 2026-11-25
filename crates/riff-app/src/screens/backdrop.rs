//! Fonds de la fenêtre et visualiseur de fond de la barre.

use crate::app::{App, DEFAULT_WINDOW, Message};
use crate::theme::ThemeSource;
use crate::theme::tokens::mesh as mesh_tokens;
use crate::theme::tokens::playlist_detail as hero;
use crate::visuals::canvas::BarCanvas;
use crate::visuals::mesh::{self, MeshKey, MeshProgram};
use crate::visuals::state::{Layer, crossfade_opacities};
use crate::visuals::viz;
use iced::gradient::Linear;
use iced::widget::image::Handle;
use iced::widget::{canvas, container, image, shader, space, stack};
use iced::{Color, ContentFit, Element, Fill, Gradient, Length, Padding, Radians, Rectangle};
use std::time::Instant;

/// Arrondi de `Math.round` : la moitié va vers le haut.
fn js_round(value: f64) -> f64 {
  (value + 0.5).floor()
}

/// `rgbToHsl` (`app.js:1056-1071`) : teinte en degrés, saturation et luminosité en pour cent, arrondies à l'entier.
fn rgb_to_hsl([red, green, blue]: [u8; 3]) -> (f64, f64, f64) {
  let [r, g, b] = [red, green, blue].map(|value| f64::from(value) / 255.0);
  let (max, min) = (r.max(g).max(b), r.min(g).min(b));
  let lightness = f64::midpoint(max, min);
  if max == min {
    return (0.0, 0.0, js_round(lightness * 100.0));
  }
  let delta = max - min;
  let saturation = if lightness > 0.5 {
    delta / (2.0 - max - min)
  } else {
    delta / (max + min)
  };
  let hue = if max == r {
    ((g - b) / delta + if g < b { 6.0 } else { 0.0 }) / 6.0
  } else if max == g {
    ((b - r) / delta + 2.0) / 6.0
  } else {
    ((r - g) / delta + 4.0) / 6.0
  };
  (
    js_round(hue * 360.0),
    js_round(saturation * 100.0),
    js_round(lightness * 100.0),
  )
}

/// `hslToHex` (`app.js:1073-1078`).
fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [u8; 3] {
  let (s, l) = (saturation / 100.0, lightness / 100.0);
  let a = s * l.min(1.0 - l);
  let channel = |n: f64| {
    let k = (n + hue / 30.0) % 12.0;
    let value = l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0);
    js_round(value * 255.0).clamp(0.0, 255.0) as u8
  };
  [channel(0.0), channel(8.0), channel(4.0)]
}

/// `mixHex(a, b, t)` (`app.js:1558-1565`) : mélange entier par canal.
fn mix_hex(a: [u8; 3], b: [u8; 3], part: f64) -> [u8; 3] {
  let channel = |a: u8, b: u8| {
    js_round(f64::from(a) + (f64::from(b) - f64::from(a)) * part).clamp(0.0, 255.0) as u8
  };
  [
    channel(a[0], b[0]),
    channel(a[1], b[1]),
    channel(a[2], b[2]),
  ]
}

/// Les deux couleurs que `applyTheme` passe à `updateMeshGradient` (`app.js:1831-1838`) puis que celle-ci mélange au gris
/// (`app.js:1688-1693`) : l'accent brut et sa teinte plus 40 degrés (`app.js:1827-1830`, `accent2` nul), `#9E9E9E` et `#757575`
/// sous 8 % d'intensité, puis `mixHex(c, '#9E9E9E', 1 - k)` avec `riff_adv_mesh_follow_intensity` (vrai par défaut).
#[must_use]
pub fn mesh_colors(accent: [u8; 3], intensity: u8, follow_intensity: bool) -> ([u8; 3], [u8; 3]) {
  let (hue, saturation, lightness) = rgb_to_hsl(accent);
  let second = hsl_to_rgb((hue + 40.0) % 360.0, saturation, lightness);
  let (first, second) = if intensity < 8 {
    ([0x9E; 3], [0x75; 3])
  } else {
    (accent, second)
  };
  if follow_intensity {
    let keep = (f64::from(intensity) / 100.0).clamp(0.0, 1.0);
    (
      mix_hex(first, [0x9E; 3], 1.0 - keep),
      mix_hex(second, [0x9E; 3], 1.0 - keep),
    )
  } else {
    (first, second)
  }
}

/// Accent brut du thème : la couleur des réglages, ou la couleur tirée de la pochette en thème « Auto (album) » (riff-rs ne tire
/// qu'une couleur de la pochette : `analysis.accent2` n'existe pas, la seconde couleur est donc la teinte plus 40 degrés).
fn mesh_accent(app: &App) -> [u8; 3] {
  let rgb = match &app.theme.source {
    ThemeSource::Cover(rgb) => *rgb,
    _ => app
      .settings
      .theme_accent
      .parse()
      .unwrap_or(crate::theme::scheme::DEFAULT_SOURCE),
  };
  [rgb.red, rgb.green, rgb.blue]
}

/// Couleurs et mode du calque : tout ce qui entre dans `meshKey` (`app.js:1695`), un changement redémarre `meshFloat`.
#[must_use]
pub fn mesh_key(app: &App) -> MeshKey {
  let (first, second) = mesh_colors(
    mesh_accent(app),
    app.settings.theme_intensity,
    app.settings.visual.mesh_follow_intensity,
  );
  MeshKey {
    first,
    second,
    dark: app.theme.dark,
    brightness: app.settings.visual.mesh_brightness,
    follow_intensity: app.settings.visual.mesh_follow_intensity,
  }
}

/// Dessin du maillage (`updateMeshGradient`) : le calque de fond (`inset: -40px`, `slot` 0) ou l'aperçu des réglages (`inset: -20px`,
/// `slot` 1). `started` : instant d'affichage du calque, d'où se déduit le point de `meshFloat` sur l'horloge de `App`.
#[must_use]
pub fn mesh_program(
  app: &App,
  started: Option<Instant>,
  inset: f32,
  radius: f32,
  slot: u32,
) -> MeshProgram {
  let visual = &app.settings.visual;
  let key = mesh_key(app);
  let background = app.theme.scheme.background;
  // `meshFloat` est `infinite` : la réduction la laisse finir en une itération sans remplissage (retour à `transform: none`, la
  // première image clé) et `body.perf` la retire (`styles.css:7542`). Dans les deux cas le calque reste affiché, immobile.
  let phase = visual
    .mesh
    .period_secs()
    .zip(started)
    .filter(|_| !crate::motion::mesh_frozen())
    .map_or(0.0, |(period, start)| {
      mesh::phase(app.now.saturating_duration_since(start), period)
    });
  MeshProgram {
    config: visual.mesh,
    first: Color::from_rgb8(key.first[0], key.first[1], key.first[2]),
    second: Color::from_rgb8(key.second[0], key.second[1], key.second[2]),
    // `--md-sys-color-background` (`app.js:1273`, `1310`) : `scheme.background`, pas `surface` (ton 8).
    background: Color::from_rgb8(background.red, background.green, background.blue),
    brightness: visual.mesh_brightness as f32 / 100.0,
    dark: key.dark,
    phase,
    inset,
    radius,
    slot,
  }
}

/// Calques de fond, du plus bas au plus haut. `#cover-bg` masque `#custom-bg` et le mesh, comme dans Riff.
#[must_use]
pub fn layers(app: &App) -> Vec<Element<'_, Message>> {
  let visual = &app.settings.visual;
  // `body.perf #cover-bg { display: none }` (`styles.css:8393`) ; `body.cover-bg-on` cache déjà le maillage et le fond personnalisé.
  if visual.cover_bg && crate::motion::perf() {
    return Vec::new();
  }
  if let Some(picture) = layer_image(
    &app.visuals.cover_bg,
    ContentFit::Cover,
    visual.cover_bg_brightness as f32 / 100.0,
  ) {
    return vec![picture];
  }
  if visual.cover_bg {
    return Vec::new();
  }
  let mut layers = Vec::new();
  if app.mesh_shown() {
    let program = mesh_program(app, app.visuals.mesh_started, mesh_tokens::INSET, 0.0, 0);
    layers.push(shader(program).width(Fill).height(Fill).into());
  }
  layers.extend(layer_image(
    &app.visuals.custom_bg,
    ContentFit::Cover,
    visual.bg_opacity as f32 / 100.0,
  ));
  layers
}

/// Image d'un visuel : seule, ou en fondu croisé entre l'ancienne (dessous) et la nouvelle (dessus). Un côté vide donne
/// un fondu vers ou depuis le vide.
#[must_use]
pub fn layer_image(layer: &Layer, fit: ContentFit, opacity: f32) -> Option<Element<'_, Message>> {
  layer_image_rounded(layer, fit, opacity, 0.0)
}

/// Comme [`layer_image`], coins arrondis : l'`overflow: hidden` d'un parent à `border-radius`.
#[must_use]
pub fn layer_image_rounded(
  layer: &Layer,
  fit: ContentFit,
  opacity: f32,
  radius: f32,
) -> Option<Element<'_, Message>> {
  let picture = |handle: &Handle, opacity: f32| {
    image(handle.clone())
      .width(Fill)
      .height(Fill)
      .content_fit(fit)
      .opacity(opacity)
      .border_radius(radius)
  };
  let Some(fade) = &layer.fade else {
    return layer
      .current
      .as_ref()
      .map(|current| picture(current, opacity).into());
  };
  match (&fade.from, &layer.current) {
    (None, None) => None,
    (Some(from), None) => Some(picture(from, opacity * (1.0 - fade.progress)).into()),
    (None, Some(current)) => Some(picture(current, opacity * fade.progress).into()),
    (Some(from), Some(current)) => {
      let (below, top) = crossfade_opacities(opacity, fade.progress);
      Some(
        stack![picture(from, below), picture(current, top)]
          .width(Fill)
          .height(Fill)
          .into(),
      )
    }
  }
}

/// `.hero-fx` des pages de détail : la pochette floutée (`blur(70px) saturate(1.5) brightness(0.5)`, `scale(1.3)`) sur les
/// 70 % du haut de la fenêtre, estompée par un masque du haut vers le bas (`styles.css:5017-5031`, `8403`). `left` est le bord
/// gauche de la page : la barre latérale reste au-dessus. Le flou est précalculé dans `visuals::blur` ; `brightness(0.5)` est un
/// voile noir et le masque un dégradé de la couleur de la page, ce qui donne le même résultat sur le fond de la page.
#[must_use]
pub fn hero_glow(app: &App, page: Color, left: f32) -> Option<Element<'_, Message>> {
  if app.settings.visual.has_background() {
    return None;
  }
  let layer = &app.visuals.hero_bg;
  let window = app.window.size.unwrap_or(DEFAULT_WINDOW);
  let height = window.height * hero::HERO_HEIGHT_SHARE * hero::HERO_SCALE;
  let picture = |handle: &Handle, opacity: f32| {
    let mut picture = image(handle.clone())
      .width(Fill)
      .height(Length::Fixed(height))
      .content_fit(ContentFit::Fill)
      .opacity(opacity);
    if let Handle::Rgba {
      width,
      height: rows,
      ..
    } = handle
    {
      let start = ((left / window.width) * *width as f32).round() as u32;
      picture = picture.crop(Rectangle {
        x: start.min(width.saturating_sub(1)),
        y: 0,
        width: width.saturating_sub(start).max(1),
        height: *rows,
      });
    }
    picture
  };
  let (pictures, visible): (Element<'_, Message>, f32) = match (&layer.fade, &layer.current) {
    (None, None) => return None,
    (None, Some(current)) => (picture(current, 1.0).into(), 1.0),
    (Some(fade), current) => match (&fade.from, current) {
      (None, None) => return None,
      (Some(from), None) => (
        picture(from, 1.0 - fade.progress).into(),
        1.0 - fade.progress,
      ),
      (None, Some(current)) => (picture(current, fade.progress).into(), fade.progress),
      (Some(from), Some(current)) => {
        let (below, top) = crossfade_opacities(1.0, fade.progress);
        (
          stack![picture(from, below), picture(current, top)].into(),
          1.0,
        )
      }
    },
  };
  let dim = Color::from_rgba(0.0, 0.0, 0.0, (1.0 - hero::HERO_BRIGHTNESS) * visible);
  let clear = Color { a: 0.0, ..page };
  let mask = Linear::new(Radians::PI)
    .add_stop(0.0, clear)
    .add_stop(hero::HERO_MASK_OPAQUE, clear)
    .add_stop(1.0, page);
  let layer = |style: container::Style| {
    container(space())
      .width(Fill)
      .height(Length::Fixed(height))
      .style(move |_| style)
  };
  Some(
    container(stack![
      pictures,
      layer(container::Style::default().background(dim)),
      layer(container::Style::default().background(Gradient::Linear(mask))),
    ])
    .width(Fill)
    .height(Fill)
    .clip(true)
    .into(),
  )
}

/// `#bottom-visualizer` : au-dessus de la barre de lecture, sans prendre de place.
#[must_use]
pub fn bar_visualizer(app: &App, bar_height: f32) -> Option<Element<'_, Message>> {
  let visual = &app.settings.visual;
  if !visual.bar_wave || !visual.bar_viz_behind || app.player.current.is_none() {
    return None;
  }
  let primary = app.theme.theme.palette().primary;
  let program = BarCanvas {
    heights: app.visuals.bar.symmetric(),
    liquid: app.visuals.liquid_points.clone(),
    connected: visual.viz_connected,
    color: Color {
      a: visual.bar_viz_opacity as f32 / 100.0,
      ..primary
    },
  };
  Some(
    container(canvas(program).width(Fill).height(viz::BAR_HEIGHT))
      .width(Fill)
      .height(Fill)
      .padding(Padding {
        top: 0.0,
        right: 16.0,
        bottom: bar_height - 4.0,
        left: 16.0,
      })
      .align_bottom(Fill)
      .into(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_mesh_colors_follow_the_raw_accent_and_its_hue_plus_forty() {
    // Valeurs recalculées avec `rgbToHsl`, `hslToHex` et `mixHex` copiés de `app.js:1056-1078`, `1558-1565` (script node).
    let accent = [0x52, 0x47, 0xD1];
    assert_eq!(
      mesh_colors(accent, 85, true),
      ([93, 84, 201], [172, 84, 201])
    );
    assert_eq!(
      mesh_colors(accent, 100, true),
      ([82, 71, 209], [175, 71, 209])
    );
    assert_eq!(
      mesh_colors(accent, 85, false),
      ([82, 71, 209], [175, 71, 209]),
      "sans suivi de l'intensité : couleurs brutes"
    );
    assert_eq!(
      mesh_colors([0xD1, 0x47, 0x52], 85, true),
      ([201, 84, 93], [201, 153, 84])
    );
  }

  #[test]
  fn the_mesh_goes_grey_below_eight_percent_of_intensity() {
    // `mono = intensity < 8` : `#9E9E9E` et `#757575`, encore mélangés au gris par `mixHex` (`app.js:1838`, `1690-1693`).
    assert_eq!(
      mesh_colors([0x52, 0x47, 0xD1], 5, true),
      ([158; 3], [156; 3])
    );
    assert_eq!(
      mesh_colors([0x52, 0x47, 0xD1], 5, false),
      ([0x9E; 3], [0x75; 3])
    );
    assert_eq!(mesh_colors([0, 0, 0], 50, true), ([79; 3], [79; 3]));
  }
}
