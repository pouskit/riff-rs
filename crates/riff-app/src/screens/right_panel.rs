//! Panneau droit `#right-panel` de Riff 44.1.1 (`index.html:2719-2769`, `styles.css:2137-2545`,
//! `10761-10766`, `11704-11736`). Les dimensions sont en pixels CSS convertis par [`tokens::css`] ;
//! voir [`tokens::right_panel`].
//!
//! Deux dispositions : sans paroles (`.lyrics-empty`, état au lancement) la boîte de pochette est
//! centrée dans la hauteur du panneau ; avec des paroles, la pochette passe en haut et la liste des
//! lignes occupe le reste. Les commandes de `.right-panel-header` n'apparaissent que sous le
//! pointeur (`.rp-controls-visible`).

use super::chrome::tip;
use super::lyrics::panel_lyrics;
use super::window_frame::overlay;
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_CARD};
use crate::library::split_artists;
use crate::lyrics::update::LyricsMessage;
use crate::theme::tokens::right_panel as rp;
use crate::theme::{fonts, icons, tokens};
use crate::ui::UiMessage;
use crate::user_data::Track;
use crate::widgets::focus::focusable;
use iced::advanced::graphics::text::Paragraph;
use iced::advanced::text::{self as advanced_text, Paragraph as _};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::{LineHeight, Shaping};
use iced::widget::{Column, Row, button, column, container, image, mouse_area, space, stack, text};
use iced::{
  Background, Border, Color, ContentFit, Element, Fill, Font, Gradient, Length, Padding, Pixels,
  Radians, Size, Theme, border, gradient, mouse,
};
use material_colors::color::Rgb;
use std::cell::RefCell;
use std::collections::HashMap;

/// Rôles de couleur de Riff utilisés par le panneau (`--md-sys-color-*`).
#[derive(Clone, Copy)]
struct Roles {
  on_surface: Color,
  on_primary: Color,
  variant: Color,
  outline: Color,
  container: Color,
  high: Color,
  highest: Color,
  low: Color,
  primary: Color,
  /// `--page-bg`.
  page: Color,
}

fn rgb(color: Rgb) -> Color {
  Color::from_rgb8(color.red, color.green, color.blue)
}

impl Roles {
  fn of(app: &App) -> Self {
    let s = &app.theme.scheme;
    Self {
      on_surface: rgb(s.on_surface),
      on_primary: rgb(s.on_primary),
      variant: rgb(s.on_surface_variant),
      outline: rgb(s.outline),
      container: rgb(s.surface_container),
      high: rgb(s.surface_container_high),
      highest: rgb(s.surface_container_highest),
      low: rgb(s.surface_container_low),
      primary: rgb(s.primary),
      page: rgb(s.surface_dim),
    }
  }
}

fn with_alpha(color: Color, alpha: f32) -> Color {
  Color {
    a: color.a * alpha,
    ..color
  }
}

/// Fond du panneau : `color-mix(in srgb, var(--page-bg) 98%, white 2%)` (`styles.css:10764`).
#[must_use]
pub(super) fn background(app: &App) -> Color {
  let page = Roles::of(app).page;
  let mix = |channel: f32| channel * (1.0 - rp::WHITE_SHARE) + rp::WHITE_SHARE;
  Color::from_rgb(mix(page.r), mix(page.g), mix(page.b))
}

/// Dispositif sans paroles (`.lyrics-empty`) : aucune ligne à montrer.
fn lyrics_empty(app: &App) -> bool {
  app
    .lyrics
    .lyrics
    .as_ref()
    .is_none_or(|lyrics| lyrics.lines.is_empty())
}

/// Largeur d'un texte sur une ligne, mémoïsée comme `ellipsize` (`menu::text_width` y passe aussi).
pub(crate) fn text_width(content: &str, size: f32, font: Font) -> f32 {
  const CAPACITY: usize = 512;
  type Key = (String, u32, Font);
  thread_local! {
    static CACHE: RefCell<HashMap<Key, f32>> = RefCell::new(HashMap::new());
  }
  let key = (content.to_string(), size.to_bits(), font);
  if let Some(hit) = CACHE.with_borrow(|cache| cache.get(&key).copied()) {
    return hit;
  }
  let width = text_width_uncached(content, size, font);
  CACHE.with_borrow_mut(|cache| {
    if cache.len() >= CAPACITY {
      cache.clear();
    }
    cache.insert(key, width);
  });
  width
}

#[expect(
  clippy::disallowed_methods,
  reason = "mesure de référence, derrière les caches bornés de `text_width` et d'`ellipsize` (dont la recherche dichotomique appelle cette mesure directement)"
)]
fn text_width_uncached(content: &str, size: f32, font: Font) -> f32 {
  Paragraph::with_text(advanced_text::Text {
    content,
    bounds: Size::new(f32::INFINITY, f32::INFINITY),
    size: Pixels(size),
    line_height: advanced_text::LineHeight::default(),
    font,
    align_x: advanced_text::Alignment::Default,
    align_y: Vertical::Top,
    shaping: advanced_text::Shaping::Advanced,
    wrapping: advanced_text::Wrapping::None,
  })
  .min_bounds()
  .width
}

/// `text-overflow: ellipsis` : le plus long début qui tient avec « … » dans `max`.
pub(crate) fn ellipsize(content: &str, size: f32, font: Font, max: f32) -> String {
  const CAPACITY: usize = 512;
  type Key = (String, u32, Font, u32);
  thread_local! {
    static CACHE: RefCell<HashMap<Key, String>> = RefCell::new(HashMap::new());
  }
  let key = (content.to_string(), size.to_bits(), font, max.to_bits());
  if let Some(hit) = CACHE.with_borrow(|cache| cache.get(&key).cloned()) {
    return hit;
  }
  let shortened = ellipsize_uncached(content, size, font, max);
  CACHE.with_borrow_mut(|cache| {
    if cache.len() >= CAPACITY {
      cache.clear();
    }
    cache.insert(key, shortened.clone());
  });
  shortened
}

fn ellipsize_uncached(content: &str, size: f32, font: Font, max: f32) -> String {
  if text_width_uncached(content, size, font) <= max {
    return content.to_string();
  }
  let chars: Vec<char> = content.chars().collect();
  let (mut low, mut high) = (0, chars.len());
  while low < high {
    let mid = (low + high).div_ceil(2);
    let candidate: String = chars[..mid].iter().collect::<String>() + "\u{2026}";
    if text_width_uncached(&candidate, size, font) <= max {
      low = mid;
    } else {
      high = mid - 1;
    }
  }
  chars[..low].iter().collect::<String>() + "\u{2026}"
}

/// Pochette (`#right-panel-cover`) ou, sans image, `#right-panel-fallback` : `surface-container`,
/// coins `--md-shape-corner-l`, icône de lecture de 36 px.
fn cover<'a>(app: &App, roles: Roles, track: Option<&Track>, side: f32) -> Element<'a, Message> {
  let url = track.and_then(|track| app.cover_of(track));
  let handle = url
    .as_deref()
    .and_then(|url| app.images.get(&ImageKey::new(url, SIZE_CARD)));
  match handle {
    Some(handle) => image(handle.clone())
      .width(side)
      .height(side)
      .content_fit(ContentFit::Cover)
      .border_radius(tokens::radius::L)
      .into(),
    None => {
      let fallback = url.is_none().then(|| {
        container(icons::glyph_px(icons::COVER_FALLBACK, 36.0))
          .center(Fill)
          .style(move |_| container::Style::default().color(roles.outline))
      });
      let base = container(fallback.map_or_else(|| space().into(), Element::from))
        .width(side)
        .height(side)
        .style(move |_| {
          container::Style::default()
            .background(roles.container)
            .border(border::rounded(tokens::radius::L))
        });
      base.into()
    }
  }
}

/// Artiste : un lien par nom (`renderClickableArtists`), graisse 500.
fn artist_row<'a>(raw: &str, size: f32, line: f32) -> Element<'a, Message> {
  let names = split_artists(raw);
  let last = names.len().saturating_sub(1);
  let parts = names.into_iter().enumerate().map(|(index, name)| {
    let label = if index < last {
      format!("{name},")
    } else {
      name.clone()
    };
    mouse_area(
      text(label)
        .size(size)
        .font(fonts::MEDIUM)
        .line_height(LineHeight::Absolute(Pixels(line)))
        .shaping(Shaping::Advanced)
        .wrapping(text::Wrapping::None),
    )
    .interaction(mouse::Interaction::Pointer)
    .on_press(Message::Ui(UiMessage::OpenArtist(name)))
    .into()
  });
  Row::with_children(parts)
    .spacing(tokens::css(4.0))
    .clip(true)
    .into()
}

/// `.right-panel-artwork-box` : pochette, titre et artiste centrés.
fn artwork<'a>(app: &'a App, roles: Roles, width_css: f32, empty: bool) -> Element<'a, Message> {
  let track = app.player.current.as_ref();
  let side = rp::cover_side(width_css, empty);
  let (title_size, title_font, title_line) = if empty {
    (rp::TITLE_TEXT_EMPTY, fonts::BOLD, rp::TITLE_LINE_BOLD)
  } else {
    (rp::TITLE_TEXT, fonts::MEDIUM, rp::TEXT_LINE)
  };
  let (artist_size, artist_font) = if empty {
    (rp::ARTIST_TEXT_EMPTY, fonts::MEDIUM)
  } else {
    (rp::ARTIST_TEXT, fonts::MEDIUM)
  };
  let title_height = title_size * title_line;
  let artist_height = artist_size * rp::TEXT_LINE;
  let available = tokens::css(width_css) - rp::PADDING_SIDE * 2.0;
  let title = track.map_or_else(
    || app.i18n.t("No track playing"),
    |track| track.title_text().to_string(),
  );
  let title = ellipsize(&title, title_size, title_font, available);
  let title = container(
    text(title)
      .size(title_size)
      .font(title_font)
      .line_height(LineHeight::Absolute(Pixels(title_height)))
      .shaping(Shaping::Advanced)
      .wrapping(text::Wrapping::None),
  )
  .style(move |_| container::Style::default().color(roles.on_surface));
  let artist: Element<'a, Message> = match track {
    Some(track) => artist_row(
      track.artist.as_deref().unwrap_or(""),
      artist_size,
      artist_height,
    ),
    None => text(app.i18n.t("Select a track to listen"))
      .size(artist_size)
      .font(artist_font)
      .line_height(LineHeight::Absolute(Pixels(artist_height)))
      .shaping(Shaping::Advanced)
      .wrapping(text::Wrapping::None)
      .into(),
  };
  let artist = container(artist).style(move |_| container::Style::default().color(roles.variant));
  let meta = column![title, artist]
    .spacing(rp::META_GAP)
    .align_x(Horizontal::Center);
  let meta = container(meta)
    .width(Fill)
    .center_x(Fill)
    .padding(Padding {
      left: rp::PADDING_SIDE,
      right: rp::PADDING_SIDE,
      ..Padding::ZERO
    })
    .clip(true);
  column![
    container(cover(app, roles, track, side)).center_x(Fill),
    meta
  ]
  .spacing(rp::ARTWORK_GAP)
  .width(Fill)
  .into()
}

/// Opacité de `.right-panel-banner::after` à la hauteur relative `t` (`styles.css:2168`).
fn veil_alpha(t: f32) -> f32 {
  let stops = [(0.0, 0.15), (0.45, 0.55), (1.0, 0.75)];
  stops
    .windows(2)
    .find(|pair| t <= pair[1].0)
    .map_or(0.75, |pair| {
      let (from, to) = (pair[0], pair[1]);
      from.1 + (to.1 - from.1) * (t - from.0) / (to.0 - from.0)
    })
}

/// Masque de `.right-panel-banner` à `t` : plein jusqu'à 55 % de `fade`, nul à `fade` (`styles.css:2162`).
fn banner_mask(t: f32, fade: f32) -> f32 {
  let start = fade * 0.55;
  if t <= start {
    1.0
  } else if t >= fade {
    0.0
  } else {
    (fade - t) / (fade - start)
  }
}

/// Bannière (`#right-panel-banner`) : image floutée, voile de `surface-container-low`, le tout fondu
/// vers le panneau par le masque. Le fondu masque l'ensemble : un voile de la couleur du fond, dont
/// l'opacité et la teinte suivent les points de rupture du masque et du voile, donne le même résultat.
fn banner<'a>(app: &'a App, roles: Roles) -> Option<Element<'a, Message>> {
  let visual = &app.settings.visual;
  let (_, opacity, fade) = app.banner_effects();
  let fade = (fade as f32 / 100.0).clamp(0.0, 1.0);
  if fade <= 0.0 {
    return None;
  }
  let picture = super::backdrop::layer_image_rounded(
    &app.visuals.banner,
    banner_fit(&visual.banner_fit),
    opacity as f32 / 100.0,
    rp::RADIUS,
  )?;
  let panel = background(app);
  let mut cuts = vec![0.0, 0.45, fade * 0.55, fade, 1.0_f32];
  cuts.sort_by(f32::total_cmp);
  cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
  let mut linear = gradient::Linear::new(Radians(std::f32::consts::PI));
  for cut in cuts.into_iter().take(8) {
    let mask = banner_mask(cut, fade);
    let cover = veil_alpha(cut);
    let alpha = 1.0 - mask * (1.0 - cover);
    let mixed = |below: f32, above: f32| ((1.0 - mask) * below + mask * cover * above) / alpha;
    let color = Color {
      r: mixed(panel.r, roles.low.r),
      g: mixed(panel.g, roles.low.g),
      b: mixed(panel.b, roles.low.b),
      a: alpha,
    };
    linear = linear.add_stop(cut, color);
  }
  let veil = container(space())
    .width(Fill)
    .height(Fill)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(Background::Gradient(Gradient::Linear(linear)))
        .border(border::rounded(rp::RADIUS))
    });
  Some(stack![picture, veil].width(Fill).height(Fill).into())
}

/// `riff_banner_fit` : `object-fit` de la bannière. Toute valeur inconnue donne `zoom`, comme dans Riff.
fn banner_fit(key: &str) -> ContentFit {
  match key {
    "fit" => ContentFit::Contain,
    "stretch" => ContentFit::Fill,
    _ => ContentFit::Cover,
  }
}

/// Bouton rond de `.lyrics-action-btn` : 32 px, fond blanc à 12 %, `primary` au survol
/// (`styles.css:2498-2545`).
fn action_button<'a>(
  app: &App,
  roles: Roles,
  glyph: &'a str,
  label: &str,
  message: Message,
) -> Element<'a, Message> {
  let widget = button(container(icons::glyph_px(glyph, rp::ICON)).center(Fill))
    .padding(0)
    .width(rp::BUTTON)
    .height(rp::BUTTON)
    .style(move |_, status| {
      let (fill, label, opacity) = match status {
        button::Status::Hovered => (roles.primary, roles.on_primary, 1.0),
        button::Status::Pressed => (roles.primary, roles.on_primary, 0.85),
        button::Status::Active | button::Status::Disabled => (
          with_alpha(Color::WHITE, rp::BUTTON_FILL),
          roles.on_surface,
          rp::BUTTON_OPACITY,
        ),
      };
      button::Style {
        background: Some(Background::Color(with_alpha(fill, opacity))),
        text_color: with_alpha(label, opacity),
        border: border::rounded(rp::BUTTON / 2.0),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  tip(
    focusable(widget)
      .on_activate(Some(message))
      .radius(rp::BUTTON / 2.0)
      .into(),
    app.i18n.t(label),
  )
}

/// `#btn-lyrics-mode` : pastille `surface-container`, texte de 11 px par `--font-scale`.
fn mode_button<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let message = Message::Lyrics(LyricsMessage::ToggleMode);
  let label = app
    .i18n
    .t(if app.lyrics.word_mode { "Word" } else { "Line" });
  let widget = button(
    container(text(label).size(rp::MODE_TEXT).font(fonts::MEDIUM))
      .center_y(Fill)
      .padding([0.0, rp::MODE_PADDING_X]),
  )
  .padding(0)
  .height(rp::BUTTON)
  .style(move |_, status| {
    let (fill, label, opacity) = match status {
      button::Status::Hovered | button::Status::Pressed => (roles.high, roles.on_surface, 1.0),
      button::Status::Active | button::Status::Disabled => {
        (roles.container, roles.variant, rp::BUTTON_OPACITY)
      }
    };
    button::Style {
      background: Some(Background::Color(with_alpha(fill, opacity))),
      text_color: with_alpha(label, opacity),
      border: border::rounded(rp::BUTTON / 2.0),
      ..button::Style::default()
    }
  })
  .on_press(message.clone());
  tip(
    focusable(widget)
      .on_activate(Some(message))
      .radius(rp::BUTTON / 2.0)
      .into(),
    app.i18n.t("Toggle line mode"),
  )
}

/// `#btn-close-right-panel` : `.btn-icon-pill` de 32 px (`styles.css:3285-3301`, `7501-7525`).
fn close_button<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let message = Message::Lyrics(LyricsMessage::TogglePanel);
  let widget = button(container(icons::glyph_px(icons::CLOSE, rp::ICON)).center(Fill))
    .padding(0)
    .width(rp::BUTTON)
    .height(rp::BUTTON)
    .style(move |_, status| {
      let (fill, label, opacity, radius) = match status {
        button::Status::Hovered => (
          overlay(roles.highest, roles.on_surface, 0.08),
          roles.on_surface,
          1.0,
          rp::BUTTON * 0.4,
        ),
        button::Status::Pressed => (
          overlay(roles.highest, roles.on_surface, 0.12),
          roles.on_surface,
          1.0,
          tokens::playbar::PRESSED_RADIUS,
        ),
        button::Status::Active | button::Status::Disabled => (
          roles.container,
          roles.variant,
          rp::BUTTON_OPACITY,
          rp::BUTTON / 2.0,
        ),
      };
      button::Style {
        background: Some(Background::Color(with_alpha(fill, opacity))),
        text_color: with_alpha(label, opacity),
        border: border::rounded(radius),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  tip(
    focusable(widget)
      .on_activate(Some(message))
      .radius(rp::BUTTON / 2.0)
      .into(),
    app.i18n.t("Close panel"),
  )
}

/// `.right-panel-header-actions` : pastille flottante en haut du panneau, centrée.
fn header<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let lyric = Message::Lyrics;
  let mut actions = Row::new()
    .spacing(rp::HEADER_GAP)
    .align_y(Vertical::Center)
    .push(action_button(
      app,
      roles,
      icons::LYRICS_EDIT,
      "Edit lyrics",
      lyric(LyricsMessage::OpenEditor),
    ))
    .push(action_button(
      app,
      roles,
      icons::LYRICS_SYNC,
      "Sync lyrics",
      lyric(LyricsMessage::OpenSync),
    ));
  if app.translation_available() {
    actions = actions.push(action_button(
      app,
      roles,
      icons::LYRICS_TRANSLATE,
      "Translate lyrics",
      lyric(LyricsMessage::ToggleTranslate),
    ));
  }
  let actions = actions
    .push(action_button(
      app,
      roles,
      icons::LYRICS_FULLSCREEN,
      "Fullscreen karaoke",
      lyric(LyricsMessage::OpenTvk),
    ))
    .push(mode_button(app, roles))
    .push(close_button(app, roles));
  let pill = container(actions)
    .padding([rp::HEADER_PADDING_Y, rp::HEADER_PADDING_X])
    .style(move |_| {
      container::Style::default()
        .background(with_alpha(roles.highest, rp::HEADER_ALPHA))
        .border(border::rounded(
          (rp::BUTTON + rp::HEADER_PADDING_Y * 2.0) / 2.0,
        ))
    });
  container(pill)
    .width(Fill)
    .align_x(Horizontal::Center)
    .padding(Padding {
      top: rp::HEADER_INSET,
      left: rp::HEADER_INSET,
      right: rp::HEADER_INSET,
      bottom: 0.0,
    })
    .into()
}

/// Place de `#right-panel` quand il est masqué : largeur nulle, l'écart de la grille reste.
#[must_use]
pub fn hidden<'a>() -> Element<'a, Message> {
  space().width(0).height(Fill).into()
}

/// Panneau de droite.
#[must_use]
pub fn panel(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  let width_css = app.settings.visual.right_panel_width;
  let empty = lyrics_empty(app);
  let artwork = artwork(app, roles, width_css, empty);
  let body: Element<'_, Message> = if empty {
    container(artwork).center_y(Fill).into()
  } else {
    // `openTvKaraoke` déplace `#lyrics-container` dans `.tv-karaoke-content` : le panneau garde sa pochette et son titre, sans lignes.
    let moved = app.lyrics.tvk_open;
    let lines: Element<'_, Message> = if moved {
      space().into()
    } else {
      panel_lyrics(app)
    };
    let lyrics = container(lines).width(Fill).height(Fill).padding(Padding {
      left: rp::PADDING_SIDE + rp::LYRICS_PADDING_X,
      right: rp::PADDING_SIDE + rp::LYRICS_PADDING_X,
      ..Padding::ZERO
    });
    Column::new()
      .push(artwork)
      .push(lyrics)
      .spacing(rp::GAP)
      .height(Fill)
      .into()
  };
  let content = container(body).width(Fill).height(Fill).padding(Padding {
    top: rp::PADDING_TOP,
    bottom: rp::PADDING_BOTTOM,
    ..Padding::ZERO
  });
  let mut layers = stack![].width(Fill).height(Fill);
  if let Some(picture) = banner(app, roles) {
    layers = layers.push(picture);
  }
  layers = layers.push(content);
  if app.ui.panel_controls.visible {
    layers = layers.push(header(app, roles));
  }
  let fill = background(app);
  let surface = container(
    mouse_area(layers)
      .on_enter(Message::Ui(UiMessage::PanelMoved))
      .on_move(|_| Message::Ui(UiMessage::PanelMoved))
      .on_exit(Message::Ui(UiMessage::PanelLeft)),
  )
  .width(tokens::css(width_css))
  .height(Fill)
  .clip(true)
  .style(move |_: &Theme| {
    container::Style::default().background(fill).border(Border {
      radius: rp::RADIUS.into(),
      ..Border::default()
    })
  });
  container(surface)
    .height(Fill)
    .width(Length::Shrink)
    .padding(Padding {
      bottom: rp::MARGIN_BOTTOM,
      ..Padding::ZERO
    })
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_banner_fit_of_riff_maps_to_a_content_fit() {
    assert_eq!(banner_fit("fit"), ContentFit::Contain);
    assert_eq!(banner_fit("zoom"), ContentFit::Cover);
    assert_eq!(banner_fit("stretch"), ContentFit::Fill);
    assert_eq!(banner_fit("autre"), ContentFit::Cover);
  }

  /// `layout.json` : `cover-container` 311,99 px CSS sans paroles, 294,83 px écran.
  #[test]
  fn the_cover_follows_the_clamps_of_riff() {
    assert!((rp::cover_side(340.0, true) - 294.83).abs() < 0.05);
    assert!((rp::cover_side(340.0, false) - tokens::css(304.0)).abs() < 1e-3);
    assert!((rp::cover_side(260.0, false) - tokens::css(240.0)).abs() < 1e-3);
    assert!((rp::cover_side(720.0, true) - tokens::css(356.0)).abs() < 1e-3);
  }

  /// `layout.json` : `right-panel-artwork-box` 355,14 px de haut sans paroles.
  #[test]
  fn the_artwork_box_has_the_measured_height() {
    assert!((rp::artwork_height(340.0, true) - 355.14).abs() < 0.5);
  }

  #[test]
  fn the_lyrics_zone_starts_under_the_title_and_ends_above_the_bar() {
    let window = 1080.0;
    let zone = window - rp::chrome_height(340.0);
    // Écran de 1080 px : 1142 px CSS moins 104 (barres), 96 (lecture), 10, 56, 24, 12 et la boîte.
    assert!((zone - 455.0).abs() < 4.0, "zone {zone}");
  }

  #[test]
  fn the_banner_fades_into_the_panel() {
    assert_eq!(banner_mask(0.0, 1.0), 1.0);
    assert_eq!(banner_mask(0.55, 1.0), 1.0);
    assert_eq!(banner_mask(1.0, 1.0), 0.0);
    assert!((banner_mask(0.775, 1.0) - 0.5).abs() < 1e-5);
    assert!((veil_alpha(0.0) - 0.15).abs() < 1e-6);
    assert!((veil_alpha(0.45) - 0.55).abs() < 1e-6);
    assert!((veil_alpha(1.0) - 0.75).abs() < 1e-6);
  }

  #[test]
  fn a_long_title_is_cut_with_an_ellipsis_that_fits() {
    let font = fonts::MEDIUM;
    let long = "Instant Crush (feat. Julian Casablancas) et encore beaucoup plus de texte";
    let max = 200.0;
    let cut = ellipsize(long, 19.0, font, max);
    assert!(cut.ends_with('\u{2026}') && cut.len() < long.len());
    assert!(text_width(&cut, 19.0, font) <= max);
    assert_eq!(ellipsize("Court", 19.0, font, max), "Court");
  }
}
