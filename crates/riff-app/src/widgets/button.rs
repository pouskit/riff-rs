//! Boutons Material Design 3 : plein, tonal, texte, icône, et entrée de navigation.
//!
//! Chaque bouton actif porte le survol animé (`--hl` et fond en `.18s`, rayon en `.22s`), l'état `.pressed` (fond `on-surface` à
//! 12 %, tenu au moins 180 ms) et l'onde du clic (`styles.css:6239-6246`, `7502-7524`, `app.js:2607-2638`), tous lus dans
//! `ui.controls` par la clé du bouton : lieu de l'appel et libellé.

use super::focus::focusable;
use super::touch::{Touch, Touchable};
use crate::motion::{self, ControlKey, HoverMix};
use crate::theme::tokens::touch as touch_tokens;
use crate::theme::{icons, tokens};
use iced::border;
use iced::widget::button::{Status, Style};
use iced::widget::{Button, button, container, text};
use iced::{Background, Color, Element, Fill, Length, Theme};
use std::panic::Location;

const RADIUS: f32 = tokens::radius::BUTTON;
const LABEL_SIZE: f32 = tokens::text::CARD_TITLE;
const DISABLED_OPACITY: f32 = 0.38;
/// Rayon de l'anneau de focus des boutons icône.
const ICON_FOCUS_RADIUS: f32 = 4.0;

/// Couleurs d'un bouton : fond au repos, fond au survol, libellé.
#[derive(Debug, Clone, Copy)]
struct Palette {
  background: Option<Color>,
  /// Fond au survol des styles statiques (`filled_style` et les autres) ; le survol animé peint `--hl` sur le repos.
  hovered: Option<Color>,
  label: Color,
}

/// Forme du bouton dans Riff : la pilule perd 2 px de rayon au survol (`28px` borné à 20 vers `18px`), le bouton rond 10 % de son côté
/// (`50%` vers `40%`, `styles.css:7511-7520`).
#[derive(Debug, Clone, Copy)]
enum Shape {
  Pill,
  Round(f32),
  /// Entrée de navigation : `.nav-item` n'est pas dans la liste de `styles.css:7508`, ni fond ni rayon n'animent.
  Fixed,
}

impl Shape {
  /// Rayon perdu au survol, en pixels écran.
  fn shrink(self) -> f32 {
    match self {
      Self::Pill => {
        touch_tokens::PILL_REST.min(touch_tokens::PILL_HEIGHT / 2.0) - touch_tokens::PILL_HOVER
      }
      Self::Round(side) => (touch_tokens::ROUND_REST - touch_tokens::ROUND_HOVER) * side,
      Self::Fixed => 0.0,
    }
  }
}

fn filled_palette(theme: &Theme) -> Palette {
  let primary = theme.extended_palette().primary;
  Palette {
    background: Some(primary.base.color),
    hovered: Some(primary.strong.color),
    label: primary.base.text,
  }
}

fn tonal_palette(theme: &Theme) -> Palette {
  let palette = theme.extended_palette();
  Palette {
    background: Some(palette.secondary.weak.color),
    hovered: Some(palette.background.neutral.color),
    label: palette.secondary.weak.text,
  }
}

fn text_palette(theme: &Theme) -> Palette {
  let palette = theme.extended_palette();
  Palette {
    background: None,
    hovered: Some(palette.background.weak.color),
    label: palette.background.base.text,
  }
}

fn style(palette: Palette, status: Status) -> Style {
  let fill = match status {
    Status::Hovered | Status::Pressed => palette.hovered.or(palette.background),
    Status::Active | Status::Disabled => palette.background,
  };
  let opacity = if status == Status::Disabled {
    DISABLED_OPACITY
  } else {
    1.0
  };
  Style {
    background: fill.map(|color| Background::Color(color.scale_alpha(opacity))),
    text_color: palette.label.scale_alpha(opacity),
    border: border::rounded(RADIUS),
    ..Style::default()
  }
}

/// Mélange de deux fonds en alpha prémultiplié, comme une transition CSS de `background-color` ; `None` vaut transparent.
fn mix_fill(from: Option<Color>, to: Option<Color>, share: f32) -> Option<Color> {
  if share <= 0.0 {
    return from;
  }
  if share >= 1.0 {
    return to;
  }
  let side =
    |color: Option<Color>| color.map_or([0.0; 4], |c| [c.r * c.a, c.g * c.a, c.b * c.a, c.a]);
  let (a, b) = (side(from), side(to));
  let lerp = |i: usize| a[i] + (b[i] - a[i]) * share;
  let alpha = lerp(3);
  (alpha > 0.0).then(|| Color::from_rgba(lerp(0) / alpha, lerp(1) / alpha, lerp(2) / alpha, alpha))
}

/// `layer` peint par-dessus `fill` (composition « source over »).
fn over(fill: Color, layer: Color) -> Color {
  let alpha = layer.a + fill.a * (1.0 - layer.a);
  let channel = |top: f32, below: f32| (top * layer.a + below * fill.a * (1.0 - layer.a)) / alpha;
  Color::from_rgba(
    channel(layer.r, fill.r),
    channel(layer.g, fill.g),
    channel(layer.b, fill.b),
    alpha,
  )
}

/// Avancement du survol : celui de la transition une fois le pointeur vu entrer ou sortir, sinon l'état d'iced (écran posé).
fn hover_share(status: Status, mix: HoverMix, tracked: f32) -> f32 {
  if mix.tracked {
    tracked
  } else {
    f32::from(u8::from(matches!(
      status,
      Status::Hovered | Status::Pressed
    )))
  }
}

fn press_share(status: Status, mix: HoverMix) -> f32 {
  if mix.tracked || status != Status::Pressed {
    mix.press
  } else {
    1.0
  }
}

/// Rayon de coin à l'avancement `mix` du survol.
fn radius_at(shape: Shape, status: Status, mix: HoverMix) -> f32 {
  RADIUS - shape.shrink() * hover_share(status, mix, mix.shape)
}

/// Style animé : fond entre repos et survol, puis fond de `.pressed` : `on-surface` à 12 % sous le calque `--hl` de 12 %, soit
/// 22,56 % d'opacité (`styles.css:6244`, `7523`).
fn animated_style(
  theme: &Theme,
  palette: Palette,
  shape: Shape,
  status: Status,
  mix: HoverMix,
) -> Style {
  if status == Status::Disabled {
    return style(palette, status);
  }
  let on_surface = theme.extended_palette().background.base.text;
  let rest = palette.background;
  // `--hl: 8%` : calque `on-surface` 8 % peint sur le fond de repos (`styles.css:7507`, `7517`), jamais une autre couleur de fond.
  let layer = on_surface.scale_alpha(touch_tokens::HOVER_SHARE);
  let hovered = Some(rest.map_or(layer, |fill| over(fill, layer)));
  let fill = mix_fill(rest, hovered, hover_share(status, mix, mix.color));
  let layers = 1.0 - (1.0 - touch_tokens::PRESS_SHARE) * (1.0 - touch_tokens::PRESS_SHARE);
  let fill = mix_fill(
    fill,
    Some(on_surface.scale_alpha(layers)),
    press_share(status, mix),
  );
  Style {
    background: fill.map(Background::Color),
    text_color: palette.label,
    border: border::rounded(radius_at(shape, status, mix)),
    ..Style::default()
  }
}

/// Style du bouton plein.
#[must_use]
pub fn filled_style(theme: &Theme, status: Status) -> Style {
  style(filled_palette(theme), status)
}

/// Style du bouton tonal.
#[must_use]
pub fn tonal_style(theme: &Theme, status: Status) -> Style {
  style(tonal_palette(theme), status)
}

/// Style du bouton texte.
#[must_use]
pub fn text_style(theme: &Theme, status: Status) -> Style {
  style(text_palette(theme), status)
}

/// Enveloppe focalisable avec l'appui, l'onde et, selon `shape`, le survol animé. Sans message le bouton est désactivé : rien de tout cela.
fn wrap<'a, Message: Touchable + 'a>(
  widget: Button<'a, Message>,
  message: Option<Message>,
  key: ControlKey,
  flavour: fn(&Theme) -> Palette,
  shape: Shape,
) -> Element<'a, Message> {
  let enabled = message.is_some();
  let mix = motion::view_mix(key);
  let mut focus = focusable(widget.on_press_maybe(message.clone()))
    .on_activate(message)
    .radius(match shape {
      Shape::Round(_) => ICON_FOCUS_RADIUS,
      _ => RADIUS,
    });
  if enabled {
    let status = if mix.hovered {
      Status::Hovered
    } else {
      Status::Active
    };
    let touch = Touch::new(
      key,
      move |theme| flavour(theme).label,
      radius_at(shape, status, mix),
    );
    if let Some(touch) = touch {
      focus = focus.touch(match shape {
        Shape::Fixed => touch,
        _ => touch.hover(key),
      });
    }
  }
  focus.into()
}

#[track_caller]
fn labeled<'a, Message: Touchable + 'a>(
  label: String,
  message: Option<Message>,
  flavour: fn(&Theme) -> Palette,
) -> Element<'a, Message> {
  let key = motion::touch_key(Location::caller(), &label);
  let mix = motion::view_mix(key);
  let widget = button(text(label).size(LABEL_SIZE))
    .padding([9, 20])
    .style(move |theme, status| animated_style(theme, flavour(theme), Shape::Pill, status, mix));
  wrap(widget, message, key, flavour, Shape::Pill)
}

/// Bouton plein. Sans message : désactivé.
#[track_caller]
pub fn filled<'a, Message: Touchable + 'a>(
  label: impl Into<String>,
  message: Option<Message>,
) -> Element<'a, Message> {
  labeled(label.into(), message, filled_palette)
}

/// Bouton tonal. Sans message : désactivé.
#[track_caller]
pub fn tonal<'a, Message: Touchable + 'a>(
  label: impl Into<String>,
  message: Option<Message>,
) -> Element<'a, Message> {
  labeled(label.into(), message, tonal_palette)
}

/// Bouton texte. Sans message : désactivé.
#[track_caller]
pub fn text_button<'a, Message: Touchable + 'a>(
  label: impl Into<String>,
  message: Option<Message>,
) -> Element<'a, Message> {
  labeled(label.into(), message, text_palette)
}

/// Bouton icône de la taille donnée. Sans message : désactivé.
#[track_caller]
pub fn icon<'a, Message: Touchable + 'a>(
  glyph: &'a str,
  message: Option<Message>,
  width: impl Into<Length>,
  height: impl Into<Length>,
) -> Element<'a, Message> {
  let key = motion::touch_key(Location::caller(), glyph);
  icon_button(icons::glyph(glyph, LABEL_SIZE), key, message, width, height)
}

/// Comme [`icon`], avec l'icône à `css_px` pixels CSS : Riff dessine le même SVG à 14, 15, 16
/// ou 18 px selon l'endroit.
#[track_caller]
pub fn icon_px<'a, Message: Touchable + 'a>(
  glyph: &'a str,
  css_px: f32,
  message: Option<Message>,
  width: impl Into<Length>,
  height: impl Into<Length>,
) -> Element<'a, Message> {
  let key = motion::touch_key(Location::caller(), glyph);
  icon_button(icons::glyph_px(glyph, css_px), key, message, width, height)
}

fn icon_button<'a, Message: Touchable + 'a>(
  label: Element<'a, Message>,
  key: ControlKey,
  message: Option<Message>,
  width: impl Into<Length>,
  height: impl Into<Length>,
) -> Element<'a, Message> {
  let (width, height) = (width.into(), height.into());
  let side = match (width, height) {
    (Length::Fixed(w), Length::Fixed(h)) => w.min(h),
    (Length::Fixed(side), _) | (_, Length::Fixed(side)) => side,
    _ => touch_tokens::PILL_HEIGHT,
  };
  let shape = Shape::Round(side);
  let mix = motion::view_mix(key);
  let widget = button(container(label).center(Fill))
    .padding(0)
    .width(width)
    .height(height)
    .style(move |theme, status| animated_style(theme, text_palette(theme), shape, status, mix));
  wrap(widget, message, key, text_palette, shape)
}

/// Entrée de navigation de la barre latérale.
#[track_caller]
pub fn nav_item<'a, Message: Touchable + 'a>(
  label: String,
  selected: bool,
  message: Message,
) -> Element<'a, Message> {
  let key = motion::touch_key(Location::caller(), &label);
  let flavour: fn(&Theme) -> Palette = if selected {
    tonal_palette
  } else {
    text_palette
  };
  let widget = button(text(label).size(LABEL_SIZE))
    .padding([9, 16])
    .width(Fill)
    .style(move |theme, status| style(flavour(theme), status));
  wrap(widget, Some(message), key, flavour, Shape::Fixed)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn tracked(color: f32, shape: f32, press: f32) -> HoverMix {
    HoverMix {
      color,
      shape,
      press,
      hovered: color > 0.5,
      tracked: true,
    }
  }

  #[test]
  fn fills_mix_in_premultiplied_alpha_so_a_transparent_end_keeps_the_hue() {
    let red = Color::from_rgb(1.0, 0.0, 0.0);
    let half = mix_fill(None, Some(red), 0.5).expect("moitié");
    assert!((half.a - 0.5).abs() < 1e-6);
    assert!(
      (half.r - 1.0).abs() < 1e-6 && half.g == 0.0,
      "teinte conservée"
    );
    assert_eq!(mix_fill(None, None, 0.5), None);
    assert_eq!(mix_fill(Some(red), None, 0.0), Some(red));
    assert_eq!(mix_fill(Some(red), None, 1.0), None);
  }

  #[test]
  fn a_tracked_button_follows_the_transition_not_the_iced_status() {
    let theme = Theme::Dark;
    let rest = animated_style(
      &theme,
      text_palette(&theme),
      Shape::Pill,
      Status::Active,
      tracked(0.0, 0.0, 0.0),
    );
    // Pointeur déjà dessus mais la piste vient de partir : encore le repos.
    let early = animated_style(
      &theme,
      text_palette(&theme),
      Shape::Pill,
      Status::Hovered,
      tracked(0.0, 0.0, 0.0),
    );
    assert_eq!(rest.background, early.background);
    assert_eq!(rest.border.radius, early.border.radius);
    // Écran posé (aucune piste) : l'état d'iced fait foi.
    let still = animated_style(
      &theme,
      text_palette(&theme),
      Shape::Pill,
      Status::Hovered,
      HoverMix::default(),
    );
    let done = animated_style(
      &theme,
      text_palette(&theme),
      Shape::Pill,
      Status::Hovered,
      tracked(1.0, 1.0, 0.0),
    );
    assert_eq!(still.background, done.background);
    assert_eq!(still.border.radius, done.border.radius);
  }

  #[test]
  fn the_pill_loses_two_pixels_of_radius_and_a_round_button_a_tenth_of_its_side() {
    let theme = Theme::Dark;
    let radius = |shape, mix| {
      animated_style(&theme, text_palette(&theme), shape, Status::Active, mix)
        .border
        .radius
        .top_left
    };
    assert!((radius(Shape::Pill, tracked(0.0, 0.0, 0.0)) - RADIUS).abs() < 1e-5);
    let pill = radius(Shape::Pill, tracked(1.0, 1.0, 0.0));
    assert!((RADIUS - pill - 2.0 * tokens::ZOOM).abs() < 1e-4, "{pill}");
    let round = radius(Shape::Round(34.0), tracked(1.0, 1.0, 0.0));
    assert!((RADIUS - round - 3.4).abs() < 1e-4, "{round}");
    assert!((radius(Shape::Fixed, tracked(1.0, 1.0, 0.0)) - RADIUS).abs() < 1e-5);
  }

  #[test]
  fn hover_paints_on_surface_at_8_percent_over_the_rest_fill() {
    let theme = Theme::Dark;
    let on_surface = theme.extended_palette().background.base.text;
    let fill = |palette: Palette, mix| {
      let style = animated_style(&theme, palette, Shape::Pill, Status::Active, mix);
      let Some(Background::Color(color)) = style.background else {
        panic!("fond attendu");
      };
      color
    };
    let primary = filled_palette(&theme).background.expect("repos");
    let lit = fill(filled_palette(&theme), tracked(1.0, 1.0, 0.0));
    assert!((lit.r - (0.92 * primary.r + 0.08 * on_surface.r)).abs() < 1e-5);
    assert!(
      lit.r > primary.r && lit.g > primary.g,
      "le bouton s'éclaircit"
    );
    let text = fill(text_palette(&theme), tracked(1.0, 1.0, 0.0));
    assert!((text.a - 0.08).abs() < 1e-6 && (text.r - on_surface.r).abs() < 1e-6);
  }

  #[test]
  fn pressed_replaces_the_fill_by_on_surface_at_22_56_percent() {
    let theme = Theme::Dark;
    let style = animated_style(
      &theme,
      filled_palette(&theme),
      Shape::Pill,
      Status::Active,
      tracked(0.0, 0.0, 1.0),
    );
    let Some(Background::Color(fill)) = style.background else {
      panic!("fond attendu");
    };
    assert!((fill.a - (1.0 - 0.88 * 0.88)).abs() < 1e-5);
  }
}
