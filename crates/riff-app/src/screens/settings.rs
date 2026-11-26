//! Réglages `#view-settings` de Riff 44.1.1 : fenêtre modale par-dessus la page ouverte (`styles.css:7356-7480`),
//! navigation des catégories à gauche, panneau `.settings-pane` qui défile à droite. Voir [`tokens::settings`].

use super::chrome::tip;
use super::common::{chip, setting_fill, setting_hover};
use super::home::{self, Roles};
use crate::app::{App, Message};
use crate::languages::LANGUAGES;
use crate::manage::{ImportKind, ManageMessage};
use crate::motion::ControlKey;
use crate::prefs::{
  self, Category, Control, Entry, IntKey, OrderList, Panel, SettingsMessage, TextKey,
};
use crate::prefs_visual::{self, MediaTarget, VisualChoice, VisualInt, VisualMessage};
use crate::theme::tokens::settings as metrics;
use crate::theme::{fonts, icons, tokens};
use crate::widgets::button::{icon, text_button};
use crate::widgets::focus::focusable;
use crate::widgets::inert::inert;
use crate::widgets::text_field::text_field;
use crate::widgets::view_transition::view_transition;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
  Column, Row, button, column, container, mouse_area, opaque, pick_list, responsive, row,
  scrollable, slider, space, stack, text, text_input,
};
use iced::{Background, Border, Color, Element, Fill, Padding, Theme, border};
use material_colors::color::Rgb;

const CONTROL_WIDTH: f32 = 260.0;
const SEARCH_ID: &str = "settings-search";
const ACCENT_ID: &str = "settings-accent";
pub(super) const NICKNAME_ID: &str = "settings-nickname";
const JAM_RELAY_ID: &str = "settings-jam-relay";
const JAM_RENDEZVOUS_ID: &str = "settings-jam-rendezvous";
/// Version affichée par `.settings-version-tag`.
pub(super) const VERSION_TAG: &str = "v44.1.1";

pub(super) fn send(message: SettingsMessage) -> Message {
  Message::Settings(message)
}

fn visual(message: VisualMessage) -> Message {
  send(SettingsMessage::Visual(message))
}

fn rgb(color: Rgb) -> Color {
  Color::from_rgb8(color.red, color.green, color.blue)
}

/// Rôles de couleur de la fenêtre, lus dans le thème actif.
#[derive(Clone, Copy)]
pub(super) struct Palette {
  pub(super) roles: Roles,
  /// `surface-container-low` : fond de la fenêtre.
  pub(super) low: Color,
  /// `surface-container-lowest` : fond de la navigation.
  pub(super) lowest: Color,
  pub(super) secondary_container: Color,
  pub(super) on_secondary_container: Color,
  /// Pastilles de `.palette-preview-dots` : `secondary`, `tertiary`, `surface` et la bordure `outline-variant`.
  pub(super) secondary: Color,
  pub(super) tertiary: Color,
  pub(super) surface: Color,
  pub(super) outline_variant: Color,
}

impl Palette {
  /// `.setting-item.is-disabled { opacity: 0.45 }` (`styles.css:3502`) : chaque couleur posée à `alpha` sur le fond du panneau.
  fn faded(self, alpha: f32) -> Self {
    let fade = |color: Color| home::blend(color, self.low, alpha);
    let r = self.roles;
    Self {
      roles: Roles {
        on_surface: fade(r.on_surface),
        variant: fade(r.variant),
        outline: fade(r.outline),
        container: fade(r.container),
        high: fade(r.high),
        highest: fade(r.highest),
        primary: fade(r.primary),
        on_primary: fade(r.on_primary),
        primary_container: fade(r.primary_container),
        on_primary_container: fade(r.on_primary_container),
        page: fade(r.page),
      },
      low: self.low,
      lowest: self.lowest,
      secondary_container: fade(self.secondary_container),
      on_secondary_container: fade(self.on_secondary_container),
      secondary: fade(self.secondary),
      tertiary: fade(self.tertiary),
      surface: fade(self.surface),
      outline_variant: fade(self.outline_variant),
    }
  }

  pub(super) fn of(app: &App) -> Self {
    let s = &app.theme.scheme;
    Self {
      roles: Roles::of(app),
      low: rgb(s.surface_container_low),
      lowest: rgb(s.surface_container_lowest),
      secondary_container: rgb(s.secondary_container),
      on_secondary_container: rgb(s.on_secondary_container),
      secondary: rgb(s.secondary),
      tertiary: rgb(s.tertiary),
      surface: rgb(s.surface),
      outline_variant: rgb(s.outline_variant),
    }
  }
}

pub(super) fn with_alpha(color: Color, alpha: f32) -> Color {
  Color { a: alpha, ..color }
}

fn label_text<'a>(p: Palette, label: String) -> Element<'a, Message> {
  text(label)
    .size(metrics::NAME_TEXT)
    .font(fonts::MEDIUM)
    .color(p.roles.on_surface)
    .into()
}

/// Description d'une rangée : avec la classe `.setting-hint` (opacité 0,78, interligne 1,35) ou en style en ligne (couleur pleine).
fn hint_text<'a>(p: Palette, hint: String, classed: bool) -> Element<'a, Message> {
  let caption = text(hint).size(metrics::HINT_TEXT);
  if classed {
    caption
      .line_height(metrics::HINT_LINE)
      .color(with_alpha(p.roles.outline, metrics::HINT_OPACITY))
      .into()
  } else {
    caption
      .color(with_alpha(p.roles.outline, metrics::HINT_INLINE_OPACITY))
      .into()
  }
}

/// Nom et description d'une rangée.
fn label_column<'a>(app: &App, p: Palette, entry: &Entry) -> Column<'a, Message> {
  let mut label = column![label_text(p, app.i18n.t(entry.label))]
    .spacing(tokens::css(2.0))
    .width(Fill);
  if !entry.hint.is_empty() {
    label = label.push(hint_text(
      p,
      app.i18n.t(entry.hint),
      has_hint_class(entry.label),
    ));
  }
  label
}

/// `.effect-switch-btn` : interrupteur de 52 x 32 px.
pub(super) fn switch<'a>(p: Palette, on: bool, message: Message) -> Element<'a, Message> {
  let roles = p.roles;
  switch_colors(
    [
      roles.primary,
      roles.on_primary,
      roles.outline,
      roles.highest,
    ],
    on,
    message,
  )
}

/// L'interrupteur à partir de ses couleurs `[primary, on-primary, outline, surface-container-highest]` : la fenêtre Audio a ses propres rôles.
pub(super) fn switch_colors<'a>(
  [primary, on_primary, outline, highest]: [Color; 4],
  on: bool,
  message: Message,
) -> Element<'a, Message> {
  let (size, left) = if on {
    (metrics::SWITCH_THUMB_ON, metrics::SWITCH_THUMB_LEFT_ON)
  } else {
    (metrics::SWITCH_THUMB_OFF, metrics::SWITCH_THUMB_LEFT_OFF)
  };
  let thumb_color = if on { on_primary } else { outline };
  let thumb = container(space())
    .width(size)
    .height(size)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(thumb_color)
        .border(border::rounded(size / 2.0))
    });
  let track = container(thumb)
    .padding(Padding::ZERO.left(metrics::SWITCH_BORDER + left))
    .align_y(Vertical::Center)
    .width(metrics::SWITCH_WIDTH)
    .height(metrics::SWITCH_HEIGHT)
    .style(move |_: &Theme| {
      let (background, border_color) = if on {
        (primary, Color::TRANSPARENT)
      } else {
        (highest, outline)
      };
      container::Style::default().background(background).border(
        Border::default()
          .rounded(metrics::SWITCH_HEIGHT / 2.0)
          .width(metrics::SWITCH_BORDER)
          .color(border_color),
      )
    });
  let widget = button(track)
    .padding(0)
    .on_press(message.clone())
    .style(|_, _| button::Style::default());
  focusable(widget)
    .on_activate(Some(message))
    .radius(metrics::SWITCH_HEIGHT / 2.0)
    .into()
}

/// `.sl` : rail de 16 px (plein à gauche du pouce, 16 % de `on-surface` à droite), pouce de 6 px.
fn m3_slider<'a>(
  p: Palette,
  range: (i32, i32),
  step: i32,
  value: i32,
  change: impl Fn(i32) -> Message + 'a,
) -> Element<'a, Message> {
  slider(range.0..=range.1, value, change)
    .step(step)
    .on_release(send(SettingsMessage::CommitInt))
    .height(metrics::SLIDER_HEIGHT)
    .width(Fill)
    .style(move |_, _| slider::Style {
      rail: slider::Rail {
        backgrounds: (
          Background::Color(p.roles.primary),
          Background::Color(with_alpha(
            p.roles.on_surface,
            metrics::SLIDER_INACTIVE_SHARE,
          )),
        ),
        width: metrics::SLIDER_RAIL,
        border: border::rounded(metrics::SLIDER_RAIL / 2.0),
      },
      handle: slider::Handle {
        shape: slider::HandleShape::Rectangle {
          width: metrics::SLIDER_THUMB.round() as u16,
          border_radius: border::radius(metrics::SLIDER_THUMB / 2.0),
        },
        background: Background::Color(p.roles.primary),
        border_width: 0.0,
        border_color: Color::TRANSPARENT,
      },
    })
    .into()
}

/// `.slider-ticks` : repères à chaque pas, majeurs là où un libellé est posé, libellés dessous.
fn ticks<'a>(
  p: Palette,
  range: (i32, i32),
  step: i32,
  labels: Vec<(i32, String)>,
) -> Element<'a, Message> {
  let span = (range.1 - range.0).max(1) as f32;
  let count = ((range.1 - range.0) / step.max(1)).max(1);
  responsive(move |size| {
    let thumb = metrics::SLIDER_THUMB;
    let at = |value: i32| thumb / 2.0 + (size.width - thumb) * ((value - range.0) as f32 / span);
    let mut layers = stack![space().width(Fill).height(metrics::TICKS_HEIGHT)]
      .width(Fill)
      .height(metrics::TICKS_HEIGHT);
    for index in 0..=count {
      let value = range.0 + index * step;
      let major = labels.iter().any(|(at, _)| *at == value);
      layers = layers.push(
        container(
          container(space())
            .width(metrics::TICK_WIDTH)
            .height(if major {
              metrics::TICK_MAJOR
            } else {
              metrics::TICK_MINOR
            })
            .style(move |_: &Theme| {
              container::Style::default()
                .background(if major {
                  p.roles.variant
                } else {
                  with_alpha(p.roles.outline, 0.6)
                })
                .border(border::rounded(1))
            }),
        )
        .padding(Padding::ZERO.left(at(value) - metrics::TICK_WIDTH / 2.0)),
      );
    }
    // `.slider-ticks i.major:first-child span` et `:last-child` : collés aux bords ; les autres, centrés sur leur repère.
    for (value, content) in &labels {
      let wide = 160.0;
      let caption = text(content.clone())
        .size(metrics::SCALE_TEXT)
        .color(p.roles.variant);
      let top = Padding::ZERO.top(metrics::TICK_LABEL_TOP);
      layers = layers.push(if *value <= range.0 {
        container(caption).padding(top)
      } else if *value >= range.1 {
        container(caption)
          .width(Fill)
          .align_x(Horizontal::Right)
          .padding(top)
      } else {
        container(container(caption).width(wide).align_x(Horizontal::Center))
          .padding(top.left((at(*value) - wide / 2.0).max(0.0)))
      });
    }
    layers.into()
  })
  .into()
}

/// `.setting-hints` des curseurs de `index.html` : libellé de la rangée, attribut `value` (la valeur par défaut) et les trois textes.
const SLIDER_HINTS: &[(&str, i32, [&str; 3])] = &[
  ("UI scale", 105, ["Compact", "Default", "Large"]),
  ("Intensity", 0, ["Gray", "Soft", "Vivid"]),
  ("Brightness", 20, ["Dark", "Default", "Bright"]),
  ("Mesh gradient brightness", 100, ["Off", "Subtle", "Full"]),
  ("Banner blur", 3, ["0px", "3px", "12px"]),
  ("Banner opacity", 100, ["20%", "60%", "100%"]),
  ("Banner fade", 100, ["Top", "Default", "Deep"]),
  ("Opacity", 100, ["5%", "50%", "100%"]),
  ("Blur", 0, ["0px", "20px", "40px"]),
  ("Cover background brightness", 45, ["5%", "50%", "100%"]),
  ("Crossfade", 0, ["Off", "6s", "12s"]),
  (
    "Lyrics animation speed",
    10,
    ["0.5x (Slow)", "1.0x (Normal)", "2.5x (Fast)"],
  ),
  (
    "Gradient tightness",
    2,
    ["2% (Sharp)", "Default", "24% (Soft)"],
  ),
];

fn slider_hints(label: &str) -> Option<(i32, [&'static str; 3])> {
  SLIDER_HINTS
    .iter()
    .find(|(name, ..)| *name == label)
    .map(|(_, default, texts)| (*default, *texts))
}

/// Libellés posés sur les repères (`addSliderTicks`, `app.js:18108-18126`) : minimum, valeur par défaut, maximum ;
/// un libellé posé sur une valeur déjà prise la remplace.
fn tick_labels(range: (i32, i32), default: i32, texts: [&str; 3]) -> Vec<(i32, &str)> {
  let mut labels: Vec<(i32, &str)> = Vec::new();
  for (value, text) in [
    (range.0, texts[0]),
    (default, texts[1]),
    (range.1, texts[2]),
  ] {
    match labels.iter_mut().find(|(at, _)| *at == value) {
      Some(slot) => slot.1 = text,
      None => labels.push((value, text)),
    }
  }
  labels
}

/// Au-delà de 20 pas, Riff garde le `.setting-hints` d'origine : les trois textes, sans repères (`app.js:18135-18138`).
fn has_ticks(range: (i32, i32), step: i32) -> bool {
  (range.1 - range.0) / step.max(1) <= 20
}

/// `.setting-hints` : trois textes de 11 px, `justify-content: space-between`.
fn plain_hints<'a>(p: Palette, texts: [String; 3]) -> Element<'a, Message> {
  let [first, middle, last] = texts;
  let caption = |content: String| {
    text(content)
      .size(metrics::HINTS_TEXT)
      .color(p.roles.outline)
  };
  row![
    caption(first),
    space().width(Fill),
    caption(middle),
    space().width(Fill),
    caption(last)
  ]
  .into()
}

/// Valeur affichée à droite du nom d'un curseur d'`IntKey`.
fn int_text(key: IntKey, value: i32) -> String {
  match key {
    IntKey::UiScale | IntKey::Brightness | IntKey::Intensity => format!("{value}%"),
    IntKey::Hue => format!("{value}°"),
    IntKey::Crossfade if value > 0 => format!("{value}s"),
    IntKey::Crossfade => "Off".to_string(),
  }
}

fn int_step(key: IntKey) -> i32 {
  match key {
    IntKey::UiScale => 5,
    _ => 1,
  }
}

/// `.setting-item-top` d'un curseur : nom et description à gauche, valeur à droite.
fn slider_head<'a>(
  app: &App,
  p: Palette,
  entry: &Entry,
  shown: String,
  extra: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
  let mut head = row![label_column(app, p, entry)]
    .spacing(metrics::ROW_TOP_GAP)
    .align_y(Vertical::Center);
  if let Some(extra) = extra {
    head = head.push(extra);
  }
  head
    .push(
      text(shown)
        .size(metrics::NAME_TEXT)
        .font(fonts::MEDIUM)
        .color(p.roles.on_surface),
    )
    .into()
}

/// Nom, valeur, curseur et repères d'une rangée à curseur.
fn slider_body<'a>(
  app: &App,
  p: Palette,
  entry: &Entry,
  (range, step): ((i32, i32), i32),
  value: i32,
  head: Element<'a, Message>,
  change: impl Fn(i32) -> Message + 'a,
) -> Element<'a, Message> {
  let mut body = column![head, m3_slider(p, range, step, value, change)].spacing(metrics::ROW_GAP);
  if let Some((default, texts)) = slider_hints(entry.label) {
    body = body.push(if has_ticks(range, step) {
      ticks(
        p,
        range,
        step,
        tick_labels(range, default, texts)
          .into_iter()
          .map(|(value, content)| (value, app.i18n.t(content)))
          .collect(),
      )
    } else {
      plain_hints(p, texts.map(|content| app.i18n.t(content)))
    });
  }
  body.into()
}

fn switch_body<'a>(
  app: &App,
  p: Palette,
  entry: &Entry,
  on: bool,
  message: Message,
) -> Element<'a, Message> {
  row![label_column(app, p, entry), switch(p, on, message)]
    .spacing(metrics::ROW_TOP_GAP)
    .align_y(Vertical::Center)
    .into()
}

/// Une option d'une liste déroulante : la valeur enregistrée et le libellé affiché.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Option_ {
  pub(super) value: String,
  pub(super) label: String,
}

impl std::fmt::Display for Option_ {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(&self.label)
  }
}

/// `.m3-dropdown-trigger` : fond `surface-container-highest`, coins de 12 px, 14 px graisse 500.
pub(super) fn dropdown<'a>(
  p: Palette,
  options: Vec<Option_>,
  current: &str,
  select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
  let selected = options
    .iter()
    .find(|option| option.value == current)
    .cloned();
  let list = pick_list(options, selected, move |option: Option_| {
    select(option.value)
  })
  .handle(pick_list::Handle::None)
  .text_size(metrics::DROPDOWN_TEXT)
  .font(fonts::MEDIUM)
  .padding(Padding::from([
    metrics::DROPDOWN_PADDING_Y,
    metrics::DROPDOWN_PADDING_X,
  ]))
  .width(Fill)
  .style(move |_, status| {
    let background = if matches!(status, pick_list::Status::Hovered) {
      p.roles.high
    } else {
      p.roles.highest
    };
    pick_list::Style {
      text_color: p.roles.on_surface,
      placeholder_color: p.roles.variant,
      handle_color: p.roles.variant,
      background: Background::Color(background),
      border: border::rounded(metrics::DROPDOWN_RADIUS),
    }
  })
  .menu_style(move |_| iced::overlay::menu::Style {
    background: Background::Color(p.roles.highest),
    border: border::rounded(metrics::DROPDOWN_RADIUS),
    text_color: p.roles.on_surface,
    selected_text_color: p.roles.on_surface,
    selected_background: Background::Color(with_alpha(p.roles.on_surface, 0.12)),
    shadow: iced::Shadow::default(),
  });
  // `.m3-dropdown-arrow` : le chevron fin de Riff est un SVG, posé par-dessus la liste (les clics la traversent).
  let arrow = container(
    container(icons::glyph_px(icons::SET_ARROW, 18.0))
      .style(move |_: &Theme| container::Style::default().color(p.roles.variant)),
  )
  .padding(Padding::ZERO.right(metrics::DROPDOWN_PADDING_X))
  .align_x(Horizontal::Right)
  .align_y(Vertical::Center)
  .width(Fill)
  .height(Fill);
  stack![list, arrow].width(Fill).into()
}

/// `.theme-mode-toggle` : boutons à plat, le choix actif sur `secondary-container`, graisse 700.
fn segmented<'a>(
  p: Palette,
  options: Vec<Option_>,
  current: &str,
  select: impl Fn(String) -> Message + 'a,
  vertical: bool,
) -> Element<'a, Message> {
  let buttons = options.into_iter().map(|option| {
    let active = option.value == current;
    let label = text(option.label)
      .size(metrics::SEGMENT_TEXT)
      .font(if active { fonts::BOLD } else { fonts::MEDIUM })
      .color(if active {
        p.on_secondary_container
      } else {
        p.roles.variant
      });
    let widget = button(label)
      .padding([
        metrics::SEGMENT_BUTTON_PADDING_Y,
        metrics::SEGMENT_BUTTON_PADDING_X,
      ])
      .on_press(select(option.value))
      .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let background = if active {
          Some(Background::Color(p.secondary_container))
        } else if hovered {
          Some(Background::Color(with_alpha(p.roles.on_surface, 0.08)))
        } else {
          None
        };
        button::Style {
          background,
          border: border::rounded(9999),
          ..button::Style::default()
        }
      });
    Element::from(widget)
  });
  let group: Element<'a, Message> = if vertical {
    Column::with_children(buttons)
      .spacing(metrics::SEGMENT_PADDING)
      .into()
  } else {
    Row::with_children(buttons)
      .spacing(metrics::SEGMENT_PADDING)
      .into()
  };
  container(group)
    .padding(metrics::SEGMENT_PADDING)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(p.roles.container)
        .border(border::rounded(9999))
    })
    .into()
}

pub(super) fn options_of(app: &App, options: &[(&str, &str)]) -> Vec<Option_> {
  options
    .iter()
    .map(|(value, label)| Option_ {
      value: (*value).to_string(),
      label: app.i18n.t(label),
    })
    .collect()
}

fn languages_options() -> Vec<Option_> {
  LANGUAGES
    .iter()
    .map(|(code, name)| Option_ {
      value: (*code).to_string(),
      label: format!("{name} ({code})"),
    })
    .collect()
}

/// `.theme-swatch` de `index.html:1540-1546` : couleur et titre.
const ACCENT_SWATCHES: [&str; 7] = [
  "#B69DF8", "#7AA7FF", "#56C5C5", "#6DD58C", "#F5B83D", "#F28B82", "#F48FB1",
];

/// Une pastille de 36 px ; la couleur choisie porte un anneau de 3 px à 3 px (`.theme-swatch.active`).
fn swatch_cell<'a>(
  p: Palette,
  fill: Color,
  ring: bool,
  content: Option<Element<'a, Message>>,
  message: Message,
) -> Element<'a, Message> {
  let margin = metrics::SWATCH_RING + metrics::SWATCH_RING_OFFSET;
  let size = metrics::SWATCH_SIZE;
  let disc = container(content.unwrap_or_else(|| space().into()))
    .center(size)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(fill)
        .color(p.roles.on_surface)
        .border(border::rounded(size / 2.0))
    });
  let ringed = container(disc).padding(margin).style(move |_: &Theme| {
    container::Style::default().border(
      Border::default()
        .rounded(size / 2.0 + margin)
        .width(if ring { metrics::SWATCH_RING } else { 0.0 })
        .color(p.roles.on_surface),
    )
  });
  let widget = button(ringed)
    .padding(0)
    .on_press(message)
    .style(|_, _| button::Style::default());
  widget.into()
}

/// `.theme-palette-picker` : sept pastilles et le bouton « + ». Le champ hexadécimal, absent de Riff, s'ouvre sous la rangée avec « + ».
fn accent_control(app: &App) -> Element<'_, Message> {
  let p = Palette::of(app);
  let current = app.settings.theme_accent.to_uppercase();
  let margin = metrics::SWATCH_RING + metrics::SWATCH_RING_OFFSET;
  let cells = ACCENT_SWATCHES.iter().map(|hex| {
    let fill = hex.parse::<Color>().unwrap_or(Color::BLACK);
    swatch_cell(
      p,
      fill,
      current == *hex,
      None,
      send(SettingsMessage::AccentInput((*hex).to_string())),
    )
  });
  let plus = swatch_cell(
    p,
    p.roles.highest,
    false,
    Some(icons::glyph_px(icons::ADD, 14.0)),
    send(SettingsMessage::AccentToggle),
  );
  // Les anneaux débordent des pastilles : la rangée se resserre de leur marge pour garder le pas de 36 + 10 px de Riff.
  let picker = Row::with_children(cells.chain([plus]))
    .spacing(metrics::SWATCH_GAP - 2.0 * margin)
    .align_y(Vertical::Center);
  let mut body =
    column![container(picker).padding(Padding::ZERO.top(metrics::SWATCH_MARGIN_TOP - margin))];
  if app.ui.prefs.accent_open {
    let value = app
      .ui
      .prefs
      .accent_input
      .as_deref()
      .unwrap_or(&app.settings.theme_accent);
    body = body.push(
      container(focusable(text_field(
        ACCENT_ID,
        "#RRGGBB",
        value,
        |next| send(SettingsMessage::AccentInput(next)),
        None,
      )))
      .width(CONTROL_WIDTH),
    );
  }
  body.spacing(metrics::ROW_GAP).into()
}

/// `.palette-preview-dots` : `primary`, `primary-container`, `secondary`, `tertiary`, `surface`.
fn palette_dots<'a>(p: Palette) -> Element<'a, Message> {
  let colors = [
    p.roles.primary,
    p.roles.primary_container,
    p.secondary,
    p.tertiary,
    p.surface,
  ];
  let dots = colors.into_iter().map(|color| {
    container(space())
      .width(metrics::PALETTE_DOT)
      .height(metrics::PALETTE_DOT)
      .style(move |_: &Theme| {
        container::Style::default().background(color).border(
          Border::default()
            .rounded(metrics::PALETTE_DOT / 2.0)
            .width(metrics::PALETTE_DOT_BORDER)
            .color(p.outline_variant),
        )
      })
      .into()
  });
  container(Row::with_children(dots).spacing(metrics::PALETTE_DOT_GAP))
    .padding(
      Padding::from([
        metrics::PALETTE_DOTS_PADDING_Y,
        metrics::PALETTE_DOTS_PADDING_X,
      ])
      .top(metrics::PALETTE_DOTS_MARGIN_TOP + metrics::PALETTE_DOTS_PADDING_Y),
    )
    .into()
}

/// `hsl(h 75% 55%)` de `initHueSegments` (`app.js:16728`).
fn hue_color(degrees: f32) -> Color {
  let (s, l) = (metrics::HUE_SATURATION, metrics::HUE_LIGHTNESS);
  let a = s * l.min(1.0 - l);
  let channel = |n: f32| {
    let k = (n + degrees / 30.0) % 12.0;
    l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
  };
  Color::from_rgb(channel(0.0), channel(8.0), channel(4.0))
}

/// `.sl.hue-seg` : douze segments en dégradé, une trouée de chaque côté du pouce, qui reste un trait de 6 px.
fn hue_slider<'a>(p: Palette, value: i32) -> Element<'a, Message> {
  let (low, high) = IntKey::Hue.range();
  let share = (value - low) as f32 / (high - low) as f32;
  responsive(move |size| {
    let knob = metrics::SLIDER_THUMB;
    let center = knob / 2.0 + (size.width - knob) * share;
    let count = metrics::HUE_SEGMENTS;
    let segments = (0..count).map(|index| {
      let (outer, inner) = (metrics::HUE_SEGMENT_OUTER, metrics::HUE_SEGMENT_INNER);
      let radius = border::Radius {
        top_left: if index == 0 { outer } else { inner },
        bottom_left: if index == 0 { outer } else { inner },
        top_right: if index + 1 == count { outer } else { inner },
        bottom_right: if index + 1 == count { outer } else { inner },
      };
      let from = index as f32 * 30.0;
      let gradient = iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
        .add_stop(0.0, hue_color(from))
        .add_stop(1.0, hue_color(from + 30.0));
      container(space())
        .width(Fill)
        .height(metrics::HUE_TRACK_HEIGHT)
        .style(move |_: &Theme| {
          container::Style::default()
            .background(Background::Gradient(gradient.into()))
            .border(Border::default().rounded(radius))
        })
        .into()
    });
    let band = container(Row::with_children(segments).spacing(metrics::HUE_SEGMENT_GAP))
      .align_y(Vertical::Center)
      .width(Fill)
      .height(metrics::HUE_HEIGHT);
    let clearance = metrics::HUE_KNOB_CLEARANCE;
    let hole = container(
      container(space())
        .width(2.0 * clearance)
        .height(metrics::HUE_TRACK_HEIGHT)
        .style(move |_: &Theme| container::Style::default().background(p.roles.container)),
    )
    .padding(Padding::ZERO.left((center - clearance).max(0.0)))
    .align_y(Vertical::Center)
    .height(metrics::HUE_HEIGHT);
    let input = slider(low..=high, value, |next| {
      send(SettingsMessage::SetInt(IntKey::Hue, next))
    })
    .on_release(send(SettingsMessage::CommitInt))
    .height(metrics::HUE_HEIGHT)
    .width(Fill)
    .style(move |_, _| slider::Style {
      rail: slider::Rail {
        backgrounds: (
          Background::Color(Color::TRANSPARENT),
          Background::Color(Color::TRANSPARENT),
        ),
        width: metrics::SLIDER_RAIL,
        border: border::rounded(0),
      },
      handle: slider::Handle {
        shape: slider::HandleShape::Rectangle {
          width: knob.round() as u16,
          border_radius: border::radius(knob / 2.0),
        },
        background: Background::Color(p.roles.on_surface),
        border_width: 0.0,
        border_color: Color::TRANSPARENT,
      },
    });
    stack![band, hole, input]
      .width(Fill)
      .height(metrics::HUE_HEIGHT)
      .into()
  })
  .height(metrics::HUE_HEIGHT)
  .into()
}

/// `.btn-pill-primary` (plein : `primary` si « Highlight buttons » est actif, sinon `surface-container-high`) et `.btn-outline-pill`.
fn big_pill<'a>(
  app: &App,
  p: Palette,
  label: String,
  primary: bool,
  message: Option<Message>,
) -> Element<'a, Message> {
  let filled = primary && app.bool_pref(prefs::BoolKey::HighlightButtons);
  let padding = if primary {
    metrics::BUTTON_PRIMARY_PADDING_X
  } else {
    metrics::BUTTON_OUTLINE_PADDING_X
  };
  let widget = button(
    container(
      text(label)
        .size(metrics::BUTTON_TEXT)
        .font(fonts::MEDIUM)
        .wrapping(text::Wrapping::None),
    )
    .height(Fill)
    .align_y(Vertical::Center),
  )
  .padding(Padding::from([0.0, padding]))
  .height(metrics::BUTTON_HEIGHT)
  .on_press_maybe(message.clone())
  .style(move |_, status| {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, color) = if filled {
      (p.roles.primary, p.roles.on_primary)
    } else if hovered {
      (p.roles.highest, p.roles.on_surface)
    } else {
      (p.roles.high, p.roles.variant)
    };
    button::Style {
      background: Some(Background::Color(background)),
      text_color: color,
      border: border::rounded(9999),
      ..button::Style::default()
    }
  });
  focusable(widget)
    .on_activate(message)
    .radius(metrics::BUTTON_HEIGHT / 2.0)
    .into()
}

fn input_control<'a>(app: &'a App, key: TextKey) -> Element<'a, Message> {
  let value = app.text_pref(key);
  let id = match key {
    TextKey::JamRelay => JAM_RELAY_ID,
    TextKey::JamRendezvous => JAM_RENDEZVOUS_ID,
    _ => NICKNAME_ID,
  };
  container(focusable(text_field(
    id,
    "",
    value,
    move |next| send(SettingsMessage::SetText(key, next)),
    None,
  )))
  .width(CONTROL_WIDTH)
  .into()
}

fn blocked_panel(app: &App) -> Element<'_, Message> {
  let artists = app
    .data
    .blocked_artists
    .iter()
    .enumerate()
    .map(|(index, name)| {
      // Une entrée retirée décale le rang des suivantes : le sel suit l'entrée, pas sa place.
      crate::motion::keyed(("blocked-artist", name), || {
        row![
          text(name.clone()).size(13).width(Fill),
          tip(
            icon(
              icons::CLOSE,
              Some(send(SettingsMessage::UnblockArtist(index))),
              28,
              28
            ),
            app.i18n.t("Unblock artist")
          )
        ]
        .align_y(Vertical::Center)
        .into()
      })
    });
  let tracks = app
    .data
    .blocked_tracks
    .iter()
    .enumerate()
    .map(|(index, track)| {
      let line = format!(
        "{} - {}",
        track.artist.as_deref().unwrap_or(""),
        track.title.as_deref().unwrap_or("")
      );
      crate::motion::keyed(("blocked-track", line.clone()), || {
        row![
          text(line).size(13).width(Fill),
          tip(
            icon(
              icons::CLOSE,
              Some(send(SettingsMessage::UnblockTrack(index))),
              28,
              28
            ),
            app.i18n.t("Unblock track")
          )
        ]
        .align_y(Vertical::Center)
        .into()
      })
    });
  if app.data.blocked_artists.is_empty() && app.data.blocked_tracks.is_empty() {
    return text(app.i18n.t("No blocked artists or tracks"))
      .size(metrics::BLOCKED_EMPTY_TEXT)
      .color(Palette::of(app).roles.outline)
      .into();
  }
  Column::with_children(artists.chain(tracks))
    .spacing(2)
    .width(Fill)
    .into()
}

fn themes_panel(app: &App) -> Element<'_, Message> {
  use crate::code_editor::{EditorMessage, EditorTarget};
  use crate::theme::file::ThemeStatus;
  use crate::theme_editor::ThemeEditorMessage;
  if let Some(draft) = &app.ui.theme_editor {
    return super::theme_editor::panel(app, draft);
  }
  let new_theme = text_button(
    app.i18n.t("New theme"),
    Some(Message::ThemeEditor(ThemeEditorMessage::New)),
  );
  if app.ui.prefs.themes.is_empty() {
    return column![
      text(app.i18n.t("No native theme found.")).size(13),
      new_theme
    ]
    .spacing(8)
    .into();
  }
  let rows = app.ui.prefs.themes.iter().map(|theme| {
    let selected = app.settings.native_theme.as_deref() == Some(theme.id.as_str());
    let message = if selected {
      Message::ClearThemeFile
    } else {
      Message::SelectThemeFile(theme.id.clone())
    };
    let native = !matches!(theme.status, ThemeStatus::Incompatible);
    let mut line = row![
      chip(theme.name.clone(), selected, message),
      space().width(Fill)
    ]
    .spacing(6)
    .align_y(Vertical::Center);
    if native {
      line = line
        .push(text_button(
          app.i18n.t("Edit"),
          Some(Message::ThemeEditor(ThemeEditorMessage::Edit(
            theme.id.clone(),
          ))),
        ))
        .push(text_button(
          app.i18n.t("Edit as text"),
          Some(Message::Editor(EditorMessage::Open(EditorTarget::Theme(
            theme.id.clone(),
          )))),
        ));
    }
    line.into()
  });
  column![
    Column::with_children(rows).spacing(4).width(Fill),
    new_theme
  ]
  .spacing(8)
  .width(Fill)
  .into()
}

/// `.setting-item-top` d'un réglage de média : le nom à gauche, « Choose file » et le bouton pour revenir à l'image du titre ou au fond uni à droite.
/// Riff n'affiche pas le fichier choisi : riff-rs le garde sous le nom, seulement quand un fichier est posé.
fn media_row<'a>(
  app: &App,
  p: Palette,
  entry: &Entry,
  target: MediaTarget,
) -> Element<'a, Message> {
  let (current, clear) = match target {
    MediaTarget::Banner => (
      app.settings.visual.banner_media.as_str(),
      "Reset to track art",
    ),
    // Les cibles du titre n'ont pas de panneau de réglages : elles vivent dans la fenêtre du titre.
    _ => (app.settings.visual.bg_url.as_str(), "Remove"),
  };
  let mut name = column![label_text(p, app.i18n.t(entry.label))]
    .spacing(tokens::css(2.0))
    .width(Fill);
  if !current.is_empty() {
    name = name.push(hint_text(p, prefs_visual::media_label(current), false));
  }
  row![
    name,
    row![
      big_pill(
        app,
        p,
        app.i18n.t("Choose file"),
        true,
        Some(visual(VisualMessage::PickMedia(target)))
      ),
      big_pill(
        app,
        p,
        app.i18n.t(clear),
        false,
        (!current.is_empty()).then(|| visual(VisualMessage::ClearMedia(target)))
      ),
    ]
    .spacing(metrics::BUTTON_GAP)
  ]
  .spacing(metrics::ROW_TOP_GAP)
  .align_y(Vertical::Center)
  .into()
}

/// `#mesh-preview-box` (280 x 160, bordure de 1 px, coins de 12 px, `overflow: hidden`) et son `.mesh-preview-bg` : même dégradé,
/// même flou et même `meshFloat` que le fond, avec `inset: -20px` (`styles.css:6878-6895`). Riff ne remplit l'aperçu que si le
/// maillage a déjà été activé (`previewBg.style.background`, `app.js:1721`, `16208`) ; sinon la boîte garde le fond du thème.
fn mesh_preview(app: &App) -> Element<'_, Message> {
  use tokens::mesh as geometry;
  let p = Palette::of(app);
  let visual = &app.settings.visual;
  let background = app.theme.theme.extended_palette().background.base.color;
  let inner = geometry::PREVIEW_RADIUS - geometry::PREVIEW_BORDER;
  let content: Element<'_, Message> = if visual.mesh_gradient && visual.mesh_brightness > 0 {
    let program = super::backdrop::mesh_program(
      app,
      app.ui.prefs.mesh_opened,
      geometry::PREVIEW_INSET,
      inner,
      1,
    );
    iced::widget::shader(program)
      .width(Fill)
      .height(Fill)
      .into()
  } else {
    space().into()
  };
  let card = container(content)
    .width(geometry::PREVIEW_WIDTH)
    .height(geometry::PREVIEW_HEIGHT)
    .padding(geometry::PREVIEW_BORDER)
    .style(move |_: &Theme| {
      container::Style::default().background(background).border(
        Border::default()
          .rounded(geometry::PREVIEW_RADIUS)
          .width(geometry::PREVIEW_BORDER)
          .color(p.outline_variant),
      )
    });
  container(card).center_x(Fill).into()
}

/// `#mesh-customizer-modal` : les deux points glissés dans l'aperçu de Riff deviennent quatre curseurs.
fn mesh_panel(app: &App) -> Element<'_, Message> {
  const SLIDERS: [(&str, VisualInt); 5] = [
    ("Animation speed", VisualInt::MeshSpeed),
    ("First point, horizontal", VisualInt::MeshP1X),
    ("First point, vertical", VisualInt::MeshP1Y),
    ("Second point, horizontal", VisualInt::MeshP2X),
    ("Second point, vertical", VisualInt::MeshP2Y),
  ];
  let rows = SLIDERS.into_iter().map(|(label, key)| {
    row![
      text(app.i18n.t(label)).size(13).width(Fill),
      visual_slider(app, key)
    ]
    .spacing(16)
    .align_y(Vertical::Center)
    .width(Fill)
    .into()
  });
  column![
    mesh_preview(app),
    Column::with_children(rows).spacing(6).width(Fill),
    text_button(app.i18n.t("Reset"), Some(visual(VisualMessage::ResetMesh)))
  ]
  .spacing(8)
  .width(Fill)
  .into()
}

fn panel<'a>(app: &'a App, panel: Panel) -> Element<'a, Message> {
  match panel {
    Panel::SidebarOrder | Panel::HomeOrder => customizer_panel(app),
    Panel::Blocked => blocked_panel(app),
    Panel::NativeThemes => themes_panel(app),
    // Rendus par `media_row` : la rangée entière change de forme.
    Panel::BannerMedia | Panel::BackgroundMedia => space().into(),
    Panel::Mesh => mesh_panel(app),
    Panel::Plugins => super::plugins::panel(app),
    Panel::Version => super::system::version_panel(app),
    Panel::Tools => super::manage::tools_panel(app),
    Panel::Imports => super::manage::imports_panel(app),
    // Ces quatre comptes ont des boutons de même libellé (« Log out », « Log in », « Import library ») dont la présence dépend de la
    // connexion : sans sel, la connexion d'un compte décale le rang des suivants et le survol saute sur un autre bouton.
    Panel::Spotify => crate::motion::keyed("spotify", || super::manage::spotify_panel(app)),
    Panel::Lastfm => crate::motion::keyed("lastfm", || super::manage::lastfm_panel(app)),
    Panel::Google => crate::motion::keyed("google", || super::manage::google_panel(app)),
    Panel::YoutubeSignIn => crate::motion::keyed("youtube", || super::manage::youtube_panel(app)),
    Panel::Folders => super::manage::folders_panel(app),
    Panel::Cache => super::manage::cache_panel(app),
    Panel::Backups => super::manage::backups_panel(app),
    Panel::Proxy => super::manage::proxy_panel(app),
    Panel::YtDlp => super::manage::yt_dlp_panel(app),
    Panel::MultiInstance => super::manage::multi_instance_panel(app),
    Panel::Diagnostics => super::manage::diagnostics_panel(app),
    Panel::AccountBackup => super::manage::account_panel(app),
    Panel::Shortcuts => super::system::shortcuts_panel(app),
    Panel::Launcher => super::system::launcher_panel(app),
    Panel::MiniWindow => text_button(
      app.i18n.t("Switch to compact window"),
      Some(Message::ToggleMiniMode),
    ),
    Panel::AudioWindow => text_button(
      app.i18n.t("Open audio settings"),
      Some(Message::AudioPrefs(crate::audio_prefs::AudioMessage::Open)),
    ),
  }
}

/// Contrôle posé sous le nom : ceux qui ne tiennent pas à droite du nom.
fn wide_control<'a>(app: &'a App, control: Control) -> Element<'a, Message> {
  match control {
    Control::Accent => accent_control(app),
    Control::Input(key) => input_control(app, key),
    Control::Panel(which) => panel(app, which),
    _ => space().into(),
  }
}

/// `.setting-item` dont la description porte la classe `.setting-hint` (ou qui contient un `input[type=text]`) :
/// `padding: 14px 18px` au lieu de `10px 16px` (`styles.css:3482-3492`, `index.html` pour chaque rangée).
fn has_hint_class(label: &str) -> bool {
  matches!(
    label,
    "Interface language"
      | "Show YouTube platform"
      | "Playbar visualizer position"
      | "Play/Pause button background"
      | "Connected liquid visualizer"
      | "Enable banners"
      | "Track banners"
      | "Classic rounded track cards"
      | "Translation language"
  )
}

fn entry_body<'a>(app: &'a App, p: Palette, entry: &'static Entry) -> Element<'a, Message> {
  let stacked = |control: Element<'a, Message>| -> Element<'a, Message> {
    column![label_column(app, p, entry), control]
      .spacing(metrics::ROW_GAP)
      .into()
  };
  match entry.control {
    Control::Toggle(key) => {
      let on = app.bool_pref(key);
      switch_body(app, p, entry, on, send(SettingsMessage::SetBool(key, !on)))
    }
    Control::VisualToggle(key) => {
      let on = app.visual_bool(key);
      switch_body(app, p, entry, on, visual(VisualMessage::Bool(key, !on)))
    }
    Control::Slider(IntKey::Hue) => {
      let value = app.int_pref(IntKey::Hue);
      column![
        slider_head(app, p, entry, int_text(IntKey::Hue, value), None),
        hue_slider(p, value)
      ]
      .spacing(metrics::ROW_GAP)
      .into()
    }
    Control::Slider(key) => {
      let value = app.int_pref(key);
      let shown = match int_text(key, value) {
        off if off == "Off" => app.i18n.t("Off"),
        other => other,
      };
      slider_body(
        app,
        p,
        entry,
        (key.range(), int_step(key)),
        value,
        slider_head(app, p, entry, shown, None),
        move |next| send(SettingsMessage::SetInt(key, next)),
      )
    }
    // Riff : « Configure » ouvre la fenêtre du maillage ; riff-rs déplie le panneau sous le curseur.
    Control::VisualSlider(VisualInt::MeshBrightness) => {
      let key = VisualInt::MeshBrightness;
      let value = app.visual_int(key);
      let open = app.ui.prefs.mesh_open;
      let body = slider_body(
        app,
        p,
        entry,
        (key.range(), key.step()),
        value,
        slider_head(
          app,
          p,
          entry,
          key.format(value),
          Some(pill_button(
            p,
            app.i18n.t("Configure"),
            send(SettingsMessage::MeshToggle),
          )),
        ),
        move |next| visual(VisualMessage::Int(key, next)),
      );
      if open {
        column![body, mesh_panel(app)]
          .spacing(metrics::ROW_GAP)
          .into()
      } else {
        body
      }
    }
    Control::VisualSlider(key) => {
      let value = app.visual_int(key);
      slider_body(
        app,
        p,
        entry,
        (key.range(), key.step()),
        value,
        slider_head(app, p, entry, key.format(value), None),
        move |next| visual(VisualMessage::Int(key, next)),
      )
    }
    Control::Choice(TextKey::PaletteStyle, options) => column![
      label_column(app, p, entry),
      dropdown(
        p,
        options_of(app, options),
        app.text_pref(TextKey::PaletteStyle),
        |value| send(SettingsMessage::SetText(TextKey::PaletteStyle, value)),
      ),
      palette_dots(p)
    ]
    .spacing(metrics::ROW_GAP)
    .into(),
    Control::Segmented(key, options) => stacked(segmented(
      p,
      options_of(app, options),
      app.text_pref(key),
      move |value| send(SettingsMessage::SetText(key, value)),
      false,
    )),
    Control::Import(kind) => row![
      label_text(p, app.i18n.t(entry.label)),
      space().width(Fill),
      big_pill(
        app,
        p,
        app.i18n.t("Import"),
        true,
        Some(Message::Manage(ManageMessage::OpenImport(kind)))
      )
    ]
    .spacing(metrics::ROW_TOP_GAP)
    .align_y(Vertical::Center)
    .into(),
    Control::Choice(key, options) => stacked(dropdown(
      p,
      options_of(app, options),
      app.text_pref(key),
      move |value| send(SettingsMessage::SetText(key, value)),
    )),
    Control::Languages(key) => stacked(dropdown(
      p,
      languages_options(),
      app.text_pref(key),
      move |value| send(SettingsMessage::SetText(key, value)),
    )),
    Control::VisualChoice(key) => {
      let options: Vec<Option_> = key
        .options()
        .iter()
        .map(|(value, label)| Option_ {
          value: (*value).to_string(),
          label: app.i18n.t(label),
        })
        .collect();
      let select = move |value| visual(VisualMessage::Choice(key, value));
      // Seul `.theme-mode-selector` (« Playbar visualizer position ») est en boutons à droite du nom ; les autres choix sont des listes.
      if key == VisualChoice::BarVizPos {
        row![
          label_column(app, p, entry),
          segmented(p, options, app.visual_choice(key), select, true)
        ]
        .spacing(metrics::ROW_TOP_GAP)
        .align_y(Vertical::Center)
        .into()
      } else {
        stacked(dropdown(p, options, app.visual_choice(key), select))
      }
    }
    Control::Accent => column![
      slider_head(app, p, entry, "Custom".to_string(), None),
      accent_control(app)
    ]
    .spacing(metrics::ROW_GAP)
    .into(),
    Control::Panel(panel @ (Panel::BannerMedia | Panel::BackgroundMedia)) => media_row(
      app,
      p,
      entry,
      if panel == Panel::BannerMedia {
        MediaTarget::Banner
      } else {
        MediaTarget::Background
      },
    ),
    // `.setting-item-top` en colonne : le gap de 14 px de `.setting-item-top` l'emporte sur le `gap: 4px` en ligne.
    Control::Panel(Panel::Blocked) => column![
      label_text(p, app.i18n.t(entry.label)),
      hint_text(p, app.i18n.t(entry.hint), false),
      container(blocked_panel(app)).padding(Padding::ZERO.top(tokens::css(6.0)))
    ]
    .spacing(tokens::css(14.0))
    .into(),
    Control::Panel(Panel::Spotify) => column![
      row![
        label_text(p, app.i18n.t(entry.label)),
        space().width(Fill),
        big_pill(
          app,
          p,
          app.i18n.t("Import"),
          true,
          Some(Message::Manage(ManageMessage::OpenImport(
            ImportKind::Spotify
          )))
        )
      ]
      .align_y(Vertical::Center),
      panel(app, Panel::Spotify)
    ]
    .spacing(metrics::ROW_GAP)
    .into(),
    Control::Panel(Panel::SidebarOrder | Panel::HomeOrder) => column![
      row![
        label_column(app, p, entry),
        pill_button(p, app.i18n.t("Reset"), send(SettingsMessage::OrderResetAll))
      ]
      .spacing(metrics::ROW_TOP_GAP)
      .align_y(Vertical::Center),
      customizer_panel(app)
    ]
    .spacing(tokens::css(14.0))
    .into(),
    Control::Input(_) | Control::Panel(_) => stacked(wide_control(app, entry.control)),
  }
}

/// `.m3-group > *` : coins de 28 px aux extrémités du groupe, de 6 px entre deux rangées (`styles.css:7481-7484`).
pub(super) fn row_radius(index: usize, count: usize) -> border::Radius {
  let top = if index == 0 {
    metrics::GROUP_OUTER
  } else {
    metrics::GROUP_INNER
  };
  let bottom = if index + 1 == count {
    metrics::GROUP_OUTER
  } else {
    metrics::GROUP_INNER
  };
  border::Radius {
    top_left: top,
    top_right: top,
    bottom_right: bottom,
    bottom_left: bottom,
  }
}

/// `.setting-item.is-disabled` : « Intensity » et « Brightness » en mode de thème automatique (`app.js:1819-1827`).
fn is_disabled(label: &str, theme_mode: &str) -> bool {
  theme_mode == "auto" && matches!(label, "Intensity" | "Brightness")
}

fn entry_view<'a>(
  app: &'a App,
  p: Palette,
  entry: &'static Entry,
  index: usize,
  count: usize,
) -> Element<'a, Message> {
  let radius = row_radius(index, count);
  let disabled = is_disabled(entry.label, app.text_pref(TextKey::ThemeMode));
  // `.setting-item:hover { --hl: 4% }` (`styles.css:7520`) : calque `on-surface` sur le fond de la rangée ; `.is-disabled` : `pointer-events: none`.
  let key = ControlKey::new(("setting-item", entry.label));
  let background = if disabled {
    p.faded(metrics::DISABLED_OPACITY).roles.container
  } else {
    setting_fill(p.roles.on_surface, p.roles.container, key)
  };
  // Stockage, Avancé, À propos et Compte : rangées propres, voir `settings_system.rs`.
  let (body, padding) = super::settings_system::body(app, p, entry).unwrap_or_else(|| {
    let padding = if has_hint_class(entry.label) {
      Padding::from([metrics::ROW_HINT_PADDING_Y, metrics::ROW_HINT_PADDING_X])
    } else {
      Padding::from([metrics::ROW_PADDING_Y, metrics::ROW_PADDING_X])
    };
    // `pointer-events: none` : un calque opaque pose sur la rangée avale les clics.
    let body: Element<'a, Message> = if disabled {
      let faded = p.faded(metrics::DISABLED_OPACITY);
      let guard = opaque(container(space()).width(Fill).height(Fill));
      stack![entry_body(app, faded, entry), guard].into()
    } else {
      entry_body(app, p, entry)
    };
    (body, padding)
  });
  let row = container(body)
    .padding(padding)
    .width(Fill)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(background)
        .border(Border::default().rounded(radius))
    });
  if disabled {
    row.into()
  } else {
    setting_hover(key, row)
  }
}

fn category_icon(category: Category) -> &'static str {
  match category {
    Category::Interface => icons::SET_INTERFACE,
    Category::Theme | Category::CustomThemes => icons::SET_THEME,
    Category::Banners => icons::SET_BANNERS,
    Category::Tracks => icons::SET_TRACKS,
    Category::Integrations => icons::SET_INTEGRATIONS,
    Category::Storage => icons::SET_STORAGE,
    Category::Advanced => icons::SET_ADVANCED,
    Category::About | Category::Account => icons::SET_ABOUT,
    Category::Plugins => icons::SET_PLUGINS,
  }
}

/// `.settings-nav-btn` : pastille de 48 px ; la catégorie ouverte prend `primary`.
fn nav_button<'a>(
  app: &App,
  p: Palette,
  category: Category,
  selected: bool,
) -> Element<'a, Message> {
  let content = row![
    icons::glyph_px(category_icon(category), 20.0),
    text(app.i18n.t(category.label()))
      .size(metrics::NAV_BUTTON_TEXT)
      .font(fonts::MEDIUM)
      .wrapping(text::Wrapping::None)
  ]
  .spacing(metrics::NAV_BUTTON_GAP)
  .align_y(Vertical::Center);
  let widget = button(container(content).height(Fill).align_y(Vertical::Center))
    .padding(Padding::from([0.0, metrics::NAV_BUTTON_PADDING_X]))
    .width(Fill)
    .height(metrics::NAV_BUTTON_HEIGHT)
    .on_press(send(SettingsMessage::OpenCategory(category)))
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      let (background, color) = if selected {
        (Some(p.roles.primary), p.roles.on_primary)
      } else if hovered {
        (
          Some(with_alpha(p.roles.on_surface, metrics::NAV_HOVER_SHARE)),
          p.roles.on_surface,
        )
      } else {
        (None, p.roles.variant)
      };
      button::Style {
        background: background.map(Background::Color),
        text_color: color,
        border: border::rounded(9999),
        ..button::Style::default()
      }
    });
  focusable(widget)
    .ripple(
      category.label(),
      if selected {
        p.roles.on_primary
      } else {
        p.roles.on_surface
      },
      metrics::NAV_BUTTON_HEIGHT / 2.0,
    )
    .on_activate(Some(send(SettingsMessage::OpenCategory(category))))
    .radius(metrics::NAV_BUTTON_HEIGHT / 2.0)
    .into()
}

/// `.settings-search-box` : pastille de 42 px sur `surface-container-high`.
fn search_box<'a>(app: &App, p: Palette) -> Element<'a, Message> {
  let query = app.ui.prefs.query.clone();
  let input = text_input(&app.i18n.t("Search settings…"), &query)
    .id(SEARCH_ID)
    .on_input(|next| send(SettingsMessage::Search(next)))
    .size(metrics::SEARCH_TEXT)
    .font(fonts::MEDIUM)
    .padding(0)
    .style(move |_, _| iced::widget::text_input::Style {
      background: Background::Color(Color::TRANSPARENT),
      border: Border::default(),
      icon: p.roles.variant,
      placeholder: with_alpha(p.roles.variant, 0.7),
      value: p.roles.on_surface,
      selection: with_alpha(p.roles.primary, 0.4),
    });
  let glyph = container(icons::glyph_px(icons::SET_SEARCH, 18.0))
    .style(move |_: &Theme| container::Style::default().color(p.roles.variant));
  let pill = container(
    row![glyph, focusable(input)]
      .spacing(metrics::SEARCH_ICON_GAP)
      .align_y(Vertical::Center),
  )
  .padding(Padding::from([0.0, metrics::SEARCH_PADDING_X]))
  .align_y(Vertical::Center)
  .width(Fill)
  .height(metrics::SEARCH_HEIGHT)
  .style(move |_: &Theme| {
    container::Style::default()
      .background(p.roles.high)
      .color(p.roles.on_surface)
      .border(border::rounded(9999))
  });
  container(pill)
    .padding(
      Padding::ZERO
        .left(metrics::SEARCH_MARGIN_X)
        .right(metrics::SEARCH_MARGIN_X)
        .bottom(metrics::SEARCH_MARGIN_BOTTOM),
    )
    .into()
}

/// Catégories de la navigation : le compte a son propre bouton en bas (`#settings-nav-account`).
fn nav_categories() -> impl Iterator<Item = Category> {
  Category::ALL
    .into_iter()
    .filter(|category| *category != Category::Account)
}

/// `.settings-nav { min-height: calc(100vh - 210px) }` (`styles.css:11756`) : plus haute que la fenêtre, la navigation défile
/// (barre masquée, `styles.css:7260`) et le bouton du compte reste sous le bord.
const NAV_MIN_HEIGHT: f32 = tokens::css(870.0);

/// `#view-settings .settings-nav` : fond `surface-container-lowest`, coins gauches de la fenêtre.
fn navigation(app: &App, p: Palette, current: Category) -> Element<'_, Message> {
  let searching = !app.ui.prefs.query.trim().is_empty();
  let entries = nav_categories()
    .map(|category| nav_button(app, p, category, !searching && category == current));
  let account =
    super::settings_system::account_nav_button(app, p, !searching && current == Category::Account);
  // Intercalaire `flex: 1 1 auto; min-height: 12px` : le bouton du compte se pose au bas de la navigation.
  let list = column![
    Column::with_children(entries).spacing(metrics::NAV_GAP),
    space().height(Fill),
    account
  ]
  .spacing(metrics::NAV_GAP)
  .height(NAV_MIN_HEIGHT);
  let nav = scrollable(list)
    .direction(Direction::Vertical(Scrollbar::hidden()))
    .height(Fill);
  let radius = border::Radius {
    top_left: metrics::MODAL_RADIUS,
    top_right: 0.0,
    bottom_right: 0.0,
    bottom_left: metrics::MODAL_RADIUS,
  };
  container(column![search_box(app, p), nav].spacing(metrics::NAV_GAP))
    .padding([metrics::NAV_PADDING_Y, metrics::NAV_PADDING_X])
    .width(metrics::NAV_WIDTH)
    .height(Fill)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(p.lowest)
        .border(Border::default().rounded(radius))
    })
    .into()
}

/// `.settings-credits` : crédit et pastille de version, à 50 % d'opacité (`styles.css:3841-3885`).
fn credits<'a>(p: Palette) -> Element<'a, Message> {
  let faded = with_alpha(p.roles.outline, 0.5);
  let tag = container(
    text(VERSION_TAG)
      .size(metrics::SCALE_TEXT)
      .font(fonts::MEDIUM)
      .color(faded),
  )
  .padding([tokens::css(2.0), tokens::css(8.0)])
  .style(move |_: &Theme| {
    container::Style::default()
      .background(with_alpha(p.roles.high, 0.5))
      .border(border::rounded(9999))
  });
  container(
    row![
      text("Riff made by rootless, support me: t.me/riffplayer")
        .size(metrics::SCALE_TEXT)
        .color(faded),
      space().width(Fill),
      tag
    ]
    .align_y(Vertical::Center),
  )
  .padding(
    Padding::ZERO
      .top(tokens::css(24.0))
      .right(tokens::css(8.0))
      .bottom(tokens::css(12.0))
      .left(tokens::css(8.0)),
  )
  .into()
}

/// `.settings-pane` : groupe de rangées qui défile, sans barre visible.
fn pane(app: &App, p: Palette, current: Category) -> Element<'_, Message> {
  let query = &app.ui.prefs.query;
  let searching = !query.trim().is_empty();
  let collapsed = !searching && super::settings_system::hides(app, current);
  let entries: Vec<&'static Entry> = if searching {
    prefs::matching(query, |label| app.i18n.t(label))
  } else if collapsed {
    Vec::new()
  } else {
    prefs::in_category(current)
      .filter(|entry| super::settings_system::grouped(entry))
      .collect()
  };
  let count = entries.len();
  let cards = (!searching)
    .then(|| super::settings_system::custom_group(app, p, current))
    .flatten();
  let body: Element<'_, Message> = if let Some(cards) = cards {
    cards
  } else if collapsed {
    space().into()
  } else if entries.is_empty() {
    container(
      text(app.i18n.t("No settings found."))
        .size(metrics::NAME_TEXT)
        .color(p.roles.variant),
    )
    .center_x(Fill)
    .padding(32)
    .into()
  } else {
    Column::with_children(
      entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| entry_view(app, p, entry, index, count)),
    )
    .spacing(metrics::GROUP_GAP)
    .padding(metrics::GROUP_PADDING)
    .width(Fill)
    .into()
  };
  let head = (!searching)
    .then(|| super::settings_system::head(app, p, current))
    .flatten();
  let tail = (!searching)
    .then(|| super::settings_system::tail(app, p, current))
    .flatten();
  // `.settings-section` : `gap: 12px` entre l'en-tête et le groupe.
  let mut section = Column::new().spacing(tokens::css(12.0));
  if let Some(head) = head {
    section = section.push(head);
  }
  if !collapsed {
    section = section.push(body);
  }
  if let Some(tail) = tail {
    section = section.push(tail);
  }
  // `settingsIn` : opacité et `translateY(6px)` de la section qui vient d'apparaître (`styles.css:7349-7351`), voile de la carte dessous.
  let section = view_transition(section, app.entry_motion.section_pose(app.now), Some(p.low))
    .pop(0.0)
    .spill(metrics::SECTION_OFFSET);
  scrollable(
    container(column![section, credits(p)].spacing(tokens::css(16.0))).padding(
      Padding::ZERO
        .top(metrics::PANE_PADDING_TOP)
        .right(metrics::PANE_PADDING_RIGHT)
        .bottom(metrics::PANE_PADDING_BOTTOM)
        .left(metrics::PANE_PADDING_LEFT),
    ),
  )
  .direction(Direction::Vertical(Scrollbar::hidden()))
  .width(Fill)
  .height(Fill)
  .into()
}

/// `.settings-modal-close-btn` : cercle de 36 px bordé, « ESC » dessous.
fn close_button<'a>(p: Palette) -> Element<'a, Message> {
  let circle = container(icons::glyph_px(icons::SET_CLOSE, 18.0))
    .center(metrics::CLOSE_CIRCLE)
    .style(move |_: &Theme| {
      container::Style::default().color(p.roles.variant).border(
        Border::default()
          .rounded(metrics::CLOSE_CIRCLE / 2.0)
          .width(metrics::CLOSE_BORDER)
          .color(with_alpha(p.roles.outline, metrics::CLOSE_BORDER_SHARE)),
      )
    });
  let esc = text("ESC")
    .size(metrics::CLOSE_TEXT)
    .font(fonts::BOLD)
    .color(with_alpha(p.roles.variant, metrics::CLOSE_ESC_OPACITY));
  let widget = button(
    column![circle, esc]
      .spacing(metrics::CLOSE_GAP)
      .align_x(Horizontal::Center),
  )
  .padding(0)
  .on_press(Message::Back)
  .style(|_, _| button::Style::default());
  focusable(widget)
    .on_activate(Some(Message::Back))
    .radius(metrics::CLOSE_CIRCLE / 2.0)
    .into()
}

/// Fenêtre des réglages et son voile, par-dessus la page qui était ouverte. Ouverture et fermeture : `app.overlay_motion`
/// (`#view-settings` en `opacity`, `.settings-view` en `opacity` et `transform`, `styles.css:7377-7405`). Le fondu de la carte est un
/// voile de la couleur de la page sous le voile noir, exact sur un fond uni (pas d'opacité de groupe dans iced).
#[must_use]
pub fn modal(app: &App) -> Element<'_, Message> {
  let pose = app.overlay_motion.settings_pose(app.now);
  let p = Palette::of(app);
  let current = Category::parse(&app.settings.settings_cat);
  let radius = metrics::MODAL_RADIUS;
  let card = container(row![navigation(app, p, current), pane(app, p, current)])
    .width(metrics::MODAL_WIDTH)
    .height(metrics::MODAL_HEIGHT)
    .clip(true)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(p.low)
        .border(border::rounded(radius))
    });
  let close = container(close_button(p))
    .padding(
      Padding::ZERO
        .top(metrics::CLOSE_OFFSET)
        .right(metrics::CLOSE_OFFSET),
    )
    .align_x(Horizontal::Right)
    .align_y(Vertical::Top)
    .width(Fill)
    .height(Fill);
  let window = stack![card, close]
    .width(metrics::MODAL_WIDTH)
    .height(metrics::MODAL_HEIGHT);
  let scrim_alpha = pose.map_or(metrics::SCRIM_ALPHA, |pose| {
    metrics::SCRIM_ALPHA * pose.scrim
  });
  let scrim = mouse_area(
    container(space())
      .width(Fill)
      .height(Fill)
      .style(move |_: &Theme| {
        container::Style::default().background(Color::from_rgba(0.0, 0.0, 0.0, scrim_alpha))
      }),
  )
  .on_press(Message::Back);
  // Sous la carte : la page assombrie par le voile à son opacité du moment.
  let veil = (!app.settings.visual.has_background()).then(|| {
    let dark = 1.0 - scrim_alpha;
    Color::from_rgb(
      p.roles.page.r * dark,
      p.roles.page.g * dark,
      p.roles.page.b * dark,
    )
  });
  let card_layer = view_transition(window, pose.map(|pose| pose.card), veil).pop(radius);
  let layers = stack![scrim, container(opaque(card_layer)).center(Fill)]
    .width(Fill)
    .height(Fill);
  if app.overlay_motion.settings_leaving(app.now) {
    // `pointer-events: none` tant que la fenêtre se ferme.
    inert(layers).into()
  } else {
    opaque(layers)
  }
}

/// Page visible derrière la fenêtre : celle d'où l'on vient, `Accueil` à défaut.
#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  use super::Screen;
  let behind = match app.back_stack.last().copied() {
    Some(Screen::Settings | Screen::Dev) | None => Screen::Home,
    Some(screen) => screen,
  };
  behind.view(app)
}

/// Curseur de la fenêtre du maillage, avec sa valeur à droite.
fn visual_slider<'a>(app: &App, key: VisualInt) -> Element<'a, Message> {
  let value = app.visual_int(key);
  row![
    m3_slider(
      Palette::of(app),
      key.range(),
      key.step(),
      value,
      move |next| visual(VisualMessage::Int(key, next))
    ),
    text(key.format(value)).size(metrics::NAME_TEXT).width(48),
  ]
  .spacing(10)
  .align_y(Vertical::Center)
  .width(CONTROL_WIDTH)
  .into()
}

/// Nom d'une entrée de la barre latérale dans la liste (`app.js` : `renderSettingsSidebarReplica`).
fn replica_label(list: OrderList, id: &str) -> &'static str {
  match (list, id) {
    (OrderList::Sidebar, "favorites") => "Favorites",
    (OrderList::Sidebar, _) => prefs::sidebar_entry_label(id),
    (OrderList::Home, "sp-made-sec") => "For you",
    (OrderList::Home, _) => prefs::home_section_label(id),
  }
}

fn replica_icon(list: OrderList, id: &str) -> &'static str {
  match (list, id) {
    (OrderList::Sidebar, "favorites") => icons::FAVORITE,
    (OrderList::Sidebar, "albums") => icons::ALBUM,
    (OrderList::Sidebar, "local") => icons::FOLDER,
    (OrderList::Sidebar, "history") => icons::HISTORY,
    (OrderList::Sidebar, "queue") => icons::NAV_QUEUE,
    (OrderList::Sidebar, _) => icons::SECTION_HOME,
    (OrderList::Home, "sp-made-sec") => icons::SECTION_FOR_YOU,
    (OrderList::Home, "sp-quick-sec") => icons::SECTION_HOME,
    (OrderList::Home, "sp-trend-sec") => icons::SECTION_TRENDING,
    (OrderList::Home, _) => icons::SECTION_ARTISTS,
  }
}

/// `.drag-handle` : six points.
fn grip<'a>(p: Palette) -> Element<'a, Message> {
  let dot = move || {
    container(space())
      .width(metrics::CUSTOMIZER_GRIP_DOT)
      .height(metrics::CUSTOMIZER_GRIP_DOT)
      .style(move |_: &Theme| {
        container::Style::default()
          .background(p.roles.outline)
          .border(border::rounded(metrics::CUSTOMIZER_GRIP_DOT / 2.0))
      })
  };
  let line = move || {
    row![dot(), dot()]
      .spacing(metrics::CUSTOMIZER_GRIP_GAP)
      .into()
  };
  Column::with_children([line(), line(), line()])
    .spacing(metrics::CUSTOMIZER_GRIP_GAP)
    .into()
}

/// `.customizer-drag-row` : poignée, icône, nom, boutons « haut » et « bas ».
fn replica_row<'a>(
  app: &App,
  p: Palette,
  list: OrderList,
  index: usize,
  count: usize,
  id: &str,
) -> Element<'a, Message> {
  let dragged = app
    .ui
    .prefs
    .order_drag
    .filter(|(dragging, _)| *dragging == list)
    .map(|(_, at)| at);
  let step = |glyph: &'static str, to: usize| {
    icon_px_button(
      glyph,
      Some(send(SettingsMessage::OrderMove(list, index, to))),
    )
  };
  let line = container(
    row![
      grip(p),
      container(icons::glyph_px(replica_icon(list, id), 16.0))
        .center_x(20.0)
        .style(move |_: &Theme| container::Style::default().color(p.roles.variant)),
      text(app.i18n.t(replica_label(list, id)))
        .size(metrics::CUSTOMIZER_ROW_TEXT)
        .font(fonts::MEDIUM)
        .color(p.roles.on_surface)
        .width(Fill),
      step(icons::PL_CHEVRON_UP, index.saturating_sub(1)),
      step(icons::PL_CHEVRON_DOWN, (index + 1).min(count - 1)),
    ]
    .spacing(metrics::CUSTOMIZER_ROW_GAP)
    .align_y(Vertical::Center),
  )
  .padding([
    metrics::CUSTOMIZER_ROW_PADDING_Y,
    metrics::CUSTOMIZER_ROW_PADDING_X,
  ])
  .width(Fill)
  .style(move |_: &Theme| {
    let background = if dragged == Some(index) {
      p.roles.highest
    } else {
      p.roles.container
    };
    container::Style::default()
      .background(background)
      .border(border::rounded(metrics::CUSTOMIZER_ROW_RADIUS))
  });
  mouse_area(line)
    .on_press(send(SettingsMessage::OrderPress(list, index)))
    .on_enter(send(SettingsMessage::OrderEnter(list, index)))
    .on_release(send(SettingsMessage::OrderRelease))
    .into()
}

fn icon_px_button<'a>(glyph: &'static str, message: Option<Message>) -> Element<'a, Message> {
  crate::widgets::button::icon_px(glyph, 14.0, message, 24, 24)
}

/// `.customizer-box` : titre et liste d'une colonne.
fn replica_box<'a>(
  app: &App,
  p: Palette,
  list: OrderList,
  title: &'static str,
) -> Element<'a, Message> {
  let order = match list {
    OrderList::Sidebar => app.sidebar_order(),
    OrderList::Home => app.settings.home_order.clone(),
  };
  let count = order.len();
  let rows = order
    .iter()
    .enumerate()
    .map(|(index, id)| replica_row(app, p, list, index, count, id));
  container(
    column![
      text(app.i18n.t(title))
        .size(metrics::CUSTOMIZER_TITLE_TEXT)
        .font(fonts::SEMIBOLD)
        .color(p.roles.on_surface),
      Column::with_children(rows).spacing(metrics::CUSTOMIZER_LIST_GAP)
    ]
    .spacing(metrics::CUSTOMIZER_TITLE_MARGIN),
  )
  .padding(metrics::CUSTOMIZER_BOX_PADDING)
  .width(Fill)
  .style(move |_: &Theme| {
    container::Style::default()
      .background(p.low)
      .border(border::rounded(metrics::CUSTOMIZER_BOX_RADIUS))
  })
  .into()
}

/// Les deux listes côte à côte de `#setting-sidebar-home-customizer`.
fn customizer_panel(app: &App) -> Element<'_, Message> {
  let p = Palette::of(app);
  row![
    replica_box(app, p, OrderList::Sidebar, "Sidebar navigation"),
    replica_box(app, p, OrderList::Home, "Home sections"),
  ]
  .spacing(metrics::CUSTOMIZER_GAP)
  .width(Fill)
  .into()
}

/// `.btn-outline-pill` d'une rangée : fond `surface-container-high`, 30 px de haut.
fn pill_button<'a>(p: Palette, label: String, message: Message) -> Element<'a, Message> {
  let widget = button(
    container(
      text(label)
        .size(metrics::CUSTOMIZER_RESET_TEXT)
        .font(fonts::MEDIUM)
        .color(p.roles.variant),
    )
    .height(Fill)
    .align_y(Vertical::Center),
  )
  .padding(Padding::from([0.0, metrics::CUSTOMIZER_RESET_PADDING_X]))
  .height(metrics::CUSTOMIZER_RESET_HEIGHT)
  .on_press(message.clone())
  .style(move |_, status| {
    let background = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
      p.roles.highest
    } else {
      p.roles.high
    };
    button::Style {
      background: Some(Background::Color(background)),
      text_color: p.roles.variant,
      border: border::rounded(9999),
      ..button::Style::default()
    }
  });
  focusable(widget)
    .on_activate(Some(message))
    .radius(metrics::CUSTOMIZER_RESET_HEIGHT / 2.0)
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::window::PersistedWindowState;

  #[test]
  fn the_group_rounds_its_ends_and_squares_the_rows_between() {
    let (outer, inner) = (metrics::GROUP_OUTER, metrics::GROUP_INNER);
    let first = row_radius(0, 3);
    assert_eq!((first.top_left, first.bottom_left), (outer, inner));
    let middle = row_radius(1, 3);
    assert_eq!((middle.top_right, middle.bottom_right), (inner, inner));
    let last = row_radius(2, 3);
    assert_eq!((last.top_left, last.bottom_right), (inner, outer));
    let only = row_radius(0, 1);
    assert_eq!((only.top_left, only.bottom_right), (outer, outer));
  }

  #[test]
  fn only_the_ui_scale_slider_steps_by_five() {
    assert_eq!(
      slider_hints("UI scale"),
      Some((105, ["Compact", "Default", "Large"]))
    );
    assert!(slider_hints("Hue").is_none());
    assert_eq!(int_step(IntKey::UiScale), 5);
    assert_eq!(int_step(IntKey::Hue), 1);
  }

  #[test]
  fn integer_sliders_show_their_unit() {
    assert_eq!(int_text(IntKey::UiScale, 105), "105%");
    assert_eq!(int_text(IntKey::Hue, 301), "301°");
    assert_eq!(int_text(IntKey::Crossfade, 0), "Off");
    assert_eq!(int_text(IntKey::Crossfade, 6), "6s");
  }

  #[test]
  fn intensity_and_brightness_are_disabled_only_in_auto_mode() {
    // `app.js:1819-1827` : `isAuto` désactive ces deux curseurs.
    assert!(is_disabled("Intensity", "auto"));
    assert!(is_disabled("Brightness", "auto"));
    assert!(!is_disabled("Intensity", "dark"));
    assert!(!is_disabled("Hue", "auto"));
    assert!(!is_disabled("Apply intensity to background", "auto"));
  }

  #[test]
  fn a_faded_palette_moves_every_color_toward_the_pane() {
    let app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    let p = Palette::of(&app);
    let faded = p.faded(metrics::DISABLED_OPACITY);
    let expected = home::blend(p.roles.on_surface, p.low, metrics::DISABLED_OPACITY);
    assert_eq!(faded.roles.on_surface, expected);
    assert_eq!(faded.low, p.low);
  }

  #[test]
  fn ticks_only_exist_up_to_twenty_steps() {
    assert!(has_ticks((80, 130), 5));
    assert!(has_ticks((5, 100), 5));
    assert!(has_ticks((5, 25), 1));
    assert!(!has_ticks((0, 100), 1));
    assert!(!has_ticks((20, 100), 2));
  }

  #[test]
  fn a_default_on_an_end_replaces_the_end_label_like_riff() {
    // `app.js:18115` : la valeur par défaut écrase le libellé du minimum ou du maximum qui porte la même valeur.
    assert_eq!(
      tick_labels((20, 100), 100, ["20%", "60%", "100%"]),
      vec![(20, "20%"), (100, "100%")]
    );
    assert_eq!(
      tick_labels((0, 12), 0, ["Off", "6s", "12s"]),
      vec![(0, "6s"), (12, "12s")]
    );
    assert_eq!(
      tick_labels((80, 130), 105, ["Compact", "Default", "Large"]).len(),
      3
    );
  }

  #[test]
  fn the_hue_segments_follow_the_hsl_wheel() {
    let red = hue_color(0.0);
    assert!(red.r > red.g && red.r > red.b);
    let green = hue_color(120.0);
    assert!(green.g > green.r && green.g > green.b);
    let blue = hue_color(240.0);
    assert!(blue.b > blue.r && blue.b > blue.g);
  }

  #[test]
  fn the_swatches_of_riff_are_seven_and_hexadecimal() {
    assert_eq!(ACCENT_SWATCHES.len(), 7);
    assert!(
      ACCENT_SWATCHES
        .iter()
        .all(|hex| hex.parse::<Color>().is_ok())
    );
  }

  #[test]
  fn the_three_settings_with_a_hint_class_pad_wider() {
    assert!(has_hint_class("Enable banners"));
    assert!(has_hint_class("Classic rounded track cards"));
    assert!(!has_hint_class("Show Albums tab"));
  }

  #[test]
  fn account_is_not_in_the_navigation_of_riff() {
    assert_eq!(nav_categories().count(), 10);
    assert!(nav_categories().all(|category| category != Category::Account));
    assert_eq!(category_icon(Category::CustomThemes), icons::SET_THEME);
  }
}
