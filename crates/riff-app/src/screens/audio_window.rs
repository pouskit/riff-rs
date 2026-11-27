//! Fenêtre Audio de Riff (`#audio-settings-modal`) : carte fixée au-dessus de la barre de lecture, cinq onglets
//! (Enhancer, Speed & Pitch, Audio FX, Equalizer, Sleep) et le pied « Reset audio settings ». Le sixième onglet,
//! Output, n'existe pas dans Riff : il garde les préréglages, la sortie audio et le fondu enchaîné de riff-rs.

use super::chrome::tip;
use super::common::{setting_fill, setting_hl, setting_hover};
use super::home::blend;
use super::sidebar::rgb;
use crate::app::{App, Message};
use crate::audio_prefs::{
  AudioMessage, AudioTab, HQ_ENGINES, SLEEP_MINUTES, SleepChoice, SoundField,
};
use crate::motion::{self, AudioPose, ControlKey, DotPose, ViewPose};
use crate::prefs::{BoolKey, IntKey, SettingsMessage};
use crate::theme::tokens::{self, audio as t};
use crate::theme::{fonts, icons};
use crate::ui::UiMessage;
use crate::widgets::OVERLAY_ID;
use crate::widgets::button::icon;
use crate::widgets::focus::focusable;
use crate::widgets::inert::inert;
use crate::widgets::reveal::reveal;
use crate::widgets::text_field::text_field;
use crate::widgets::view_transition::view_transition;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
  Column, Row, button, column, container, mouse_area, opaque, responsive, row, scrollable, slider,
  space, stack, svg, text, vertical_slider,
};
use iced::{Background, Border, Color, Element, Fill, Length, Padding, Theme, border};
use std::time::Instant;

const PRESET_NAME_ID: &str = "audio-preset-name";
const BAND_LABELS: [&str; 10] = [
  "32", "64", "125", "250", "500", "1k", "2k", "4k", "8k", "16k",
];

fn send(message: AudioMessage) -> Message {
  Message::AudioPrefs(message)
}

/// Rôles de couleur de Riff utilisés par la fenêtre (`--md-sys-color-*`).
#[derive(Clone, Copy)]
struct Roles {
  on_surface: Color,
  variant: Color,
  outline: Color,
  outline_variant: Color,
  secondary_container: Color,
  on_secondary_container: Color,
  primary: Color,
  on_primary: Color,
  primary_container: Color,
  on_primary_container: Color,
  container: Color,
  high: Color,
  highest: Color,
}

impl Roles {
  fn of(app: &App) -> Self {
    let s = &app.theme.scheme;
    Self {
      on_surface: rgb(s.on_surface),
      variant: rgb(s.on_surface_variant),
      outline: rgb(s.outline),
      outline_variant: rgb(s.outline_variant),
      secondary_container: rgb(s.secondary_container),
      on_secondary_container: rgb(s.on_secondary_container),
      primary: rgb(s.primary),
      on_primary: rgb(s.on_primary),
      primary_container: rgb(s.primary_container),
      on_primary_container: rgb(s.on_primary_container),
      container: rgb(s.surface_container),
      high: rgb(s.surface_container_high),
      highest: rgb(s.surface_container_highest),
    }
  }
}

/// `+N` pour une valeur positive, `N` sinon : `${v > 0 ? '+' : ''}${v}` de `app.js:3746`.
fn signed(value: f32) -> String {
  let value = value.round() as i32;
  if value > 0 {
    format!("+{value}")
  } else {
    value.to_string()
  }
}

fn percent(value: f32) -> String {
  format!("{}%", value.round() as i32)
}

/// Texte de la valeur d'un curseur, comme `syncSettingsSlidersToState` (`app.js:3738`).
fn shown(field: SoundField, value: f32) -> String {
  match field {
    SoundField::Speed | SoundField::SpeedPitch => format!("{value:.2}x"),
    SoundField::Pitch => format!("{} st", signed(value)),
    SoundField::Volume => percent(value * 100.0),
    SoundField::Reverb | SoundField::Distortion | SoundField::Echo | SoundField::Muffled => {
      percent(value)
    }
    SoundField::Low | SoundField::Mid | SoundField::High | SoundField::Band(_) => {
      format!("{} dB", signed(value))
    }
    SoundField::HqVocal | SoundField::HqAir | SoundField::HqBass => {
      format!("{}%", signed(value))
    }
  }
}

fn name<'a>(roles: Roles, label: String) -> Element<'a, Message> {
  text(label)
    .size(t::NAME_SIZE)
    .font(fonts::MEDIUM)
    .color(roles.on_surface)
    .into()
}

fn head<'a>(
  roles: Roles,
  label: String,
  value: String,
  value_color: Color,
) -> Element<'a, Message> {
  head_with(
    roles,
    label,
    text(value)
      .size(t::NAME_SIZE)
      .font(fonts::MEDIUM)
      .color(value_color)
      .into(),
  )
}

/// `.setting-item-top` dont la valeur est un élément (le statut du minuteur porte son point).
fn head_with<'a>(roles: Roles, label: String, value: Element<'a, Message>) -> Element<'a, Message> {
  row![name(roles, label), space().width(Length::Fill), value]
    .spacing(t::ROW_GAP)
    .height(t::ROW_HEIGHT)
    .align_y(Vertical::Center)
    .into()
}

/// `.setting-item` : sans fond visible, il a la couleur du panneau (`surface-container-high` dans la capture).
fn item<'a>(content: impl Into<Element<'a, Message>>, padding_y: f32) -> Element<'a, Message> {
  container(content)
    .width(Fill)
    .padding(Padding {
      top: padding_y,
      bottom: padding_y,
      left: t::ITEM_PADDING_X,
      right: t::ITEM_PADDING_X,
    })
    .into()
}

fn svg_icon<'a>(bytes: &'static [u8], size: f32, color: Color) -> Element<'a, Message> {
  svg(svg::Handle::from_memory(bytes))
    .width(size)
    .height(size)
    .style(move |_: &Theme, _| svg::Style { color: Some(color) })
    .into()
}

fn tab_icon<'a>(tab: AudioTab, color: Color) -> Element<'a, Message> {
  let bytes: &'static [u8] = match tab {
    AudioTab::Enhance => include_bytes!("../../assets/icons/audio-tab-enhance.svg"),
    AudioTab::Tempo => include_bytes!("../../assets/icons/audio-tab-tempo.svg"),
    AudioTab::Effects => include_bytes!("../../assets/icons/audio-tab-effects.svg"),
    AudioTab::Equalizer => include_bytes!("../../assets/icons/audio.svg"),
    AudioTab::Sleep => include_bytes!("../../assets/icons/audio-tab-sleep.svg"),
    AudioTab::Output => include_bytes!("../../assets/icons/volume.svg"),
  };
  svg_icon(bytes, t::TAB_ICON, color)
}

/// `.audio-tab`.
fn tab<'a>(roles: Roles, app: &App, tab: AudioTab) -> Element<'a, Message> {
  let active = app.ui.audio.tab == tab;
  let color = if active {
    roles.on_primary
  } else {
    roles.variant
  };
  let content = row![
    tab_icon(tab, color),
    text(app.i18n.t(tab.label()))
      .size(t::TAB_TEXT)
      .wrapping(Wrapping::None)
      .color(color),
  ]
  .spacing(t::TAB_GAP)
  .align_y(Vertical::Center);
  let message = send(AudioMessage::Tab(tab));
  let widget = button(container(content).height(Fill).align_y(Vertical::Center))
    .padding(Padding::from([0.0, t::TAB_PADDING]))
    .width(Fill)
    .height(t::TAB_HEIGHT)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      let fill = if active {
        Some(roles.primary)
      } else if hovered {
        Some(blend(roles.on_surface, roles.high, t::TAB_HOVER_SHARE))
      } else {
        None
      };
      button::Style {
        background: fill.map(Background::Color),
        text_color: if active || !hovered {
          color
        } else {
          roles.on_surface
        },
        border: border::rounded(tokens::radius::FULL),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  focusable(widget)
    .ripple(tab.label(), color, t::TAB_HEIGHT / 2.0)
    .on_activate(Some(message))
    .radius(t::TAB_HEIGHT / 2.0)
    .into()
}

/// `.btn-outline-pill` : fond `surface-container-high` (invisible sur la carte), actif en `secondary-container`.
fn pill<'a>(
  roles: Roles,
  label: String,
  active: bool,
  message: Option<Message>,
) -> Element<'a, Message> {
  let enabled = message.is_some();
  let key_label = label.clone();
  let widget = button(
    container(text(label).size(t::PILL_TEXT).font(fonts::MEDIUM))
      .height(Fill)
      .align_y(Vertical::Center),
  )
  .padding(Padding::from([0.0, t::PILL_PADDING_X]))
  .height(t::PILL_HEIGHT)
  .style(move |_, status| {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (fill, label) = if active {
      (roles.secondary_container, roles.on_secondary_container)
    } else if hovered {
      (roles.highest, roles.on_surface)
    } else {
      (roles.high, roles.variant)
    };
    button::Style {
      background: Some(Background::Color(fill)),
      text_color: label,
      border: border::rounded(tokens::radius::FULL),
      ..button::Style::default()
    }
  })
  .on_press_maybe(message.clone());
  let mut focus = focusable(widget)
    .on_activate(message)
    .radius(t::PILL_HEIGHT / 2.0);
  if enabled {
    focus = focus.ripple(&key_label, roles.on_surface, t::PILL_HEIGHT / 2.0);
  }
  focus.into()
}

fn slider_style(roles: Roles, status: slider::Status) -> slider::Style {
  let clear = Background::Color(Color::TRANSPARENT);
  slider::Style {
    rail: slider::Rail {
      backgrounds: (clear, clear),
      width: 0.0,
      border: Border::default(),
    },
    handle: slider::Handle {
      shape: slider::HandleShape::Rectangle {
        width: if status == slider::Status::Dragged {
          t::SLIDER_THUMB_PRESSED
        } else {
          t::SLIDER_THUMB
        },
        border_radius: border::radius(t::SLIDER_INNER_RADIUS),
      },
      background: Background::Color(roles.primary),
      border_width: 0.0,
      border_color: Color::TRANSPARENT,
    },
  }
}

/// `.sl` : segment plein jusqu'à 8 px du curseur, segment vide de 16 % d'`on-surface` après, curseur de 6 x 28 px.
/// Les segments sont posés sous un `slider` d'iced dont la piste est transparente : lui seul porte le curseur et la souris.
fn pill_slider<F>(
  roles: Roles,
  (low, high, step): (f32, f32, f32),
  value: f32,
  on_change: F,
  on_release: Message,
) -> Element<'static, Message>
where
  F: Fn(f32) -> Message + Clone + 'static,
{
  responsive(move |size| {
    let ratio = if high > low {
      ((value - low) / (high - low)).clamp(0.0, 1.0)
    } else {
      0.0
    };
    let thumb = f32::from(t::SLIDER_THUMB);
    let center = (size.width - thumb) * ratio + thumb / 2.0;
    let active = (center - t::SLIDER_GAP).max(0.0);
    let inactive = (size.width - center - t::SLIDER_GAP).max(0.0);
    let (outer, inner) = (t::SLIDER_OUTER_RADIUS, t::SLIDER_INNER_RADIUS);
    let segment = move |width: f32, color: Color, left: f32, right: f32, align: Horizontal| {
      let bar = container(space())
        .width(width)
        .height(t::SLIDER_TRACK_HEIGHT)
        .style(move |_: &Theme| {
          container::Style::default()
            .background(color)
            .border(Border {
              radius: border::Radius {
                top_left: left,
                bottom_left: left,
                top_right: right,
                bottom_right: right,
              },
              ..Border::default()
            })
        });
      container(bar)
        .width(Fill)
        .height(t::SLIDER_HEIGHT)
        .padding(Padding::new(0.0).top(t::SLIDER_TRACK_TOP))
        .align_x(align)
    };
    let change = on_change.clone();
    let handle = slider(low..=high, value, change)
      .step(step)
      .on_release(on_release.clone())
      .height(t::SLIDER_HEIGHT)
      .style(move |_: &Theme, status| slider_style(roles, status));
    let empty = blend(roles.on_surface, roles.high, t::SLIDER_INACTIVE_SHARE);
    stack![
      segment(active, roles.primary, outer, inner, Horizontal::Left),
      segment(inactive, empty, inner, outer, Horizontal::Right),
      handle,
    ]
    .width(Fill)
    .height(t::SLIDER_HEIGHT)
    .into()
  })
  .width(Fill)
  .height(t::SLIDER_HEIGHT)
  .into()
}

fn sound_slider(roles: Roles, app: &App, field: SoundField) -> Element<'static, Message> {
  pill_slider(
    roles,
    field.range(),
    app.sound_value(field),
    move |next| send(AudioMessage::Set(field, next)),
    send(AudioMessage::Commit),
  )
}

/// Un réglage à curseur : nom et valeur, puis le curseur (`.setting-item` de `index.html:3067`).
fn slider_item(
  roles: Roles,
  app: &App,
  label: &str,
  field: SoundField,
) -> Element<'static, Message> {
  titled_slider_item(roles, app, app.i18n.t(label), field)
}

/// Comme `slider_item`, avec un titre déjà traduit.
fn titled_slider_item(
  roles: Roles,
  app: &App,
  title: String,
  field: SoundField,
) -> Element<'static, Message> {
  let value = shown(field, app.sound_value(field));
  item(
    column![
      head(roles, title, value, roles.on_surface),
      sound_slider(roles, app, field)
    ]
    .spacing(t::ITEM_GAP),
    t::ITEM_PADDING_Y,
  )
}

/// Note d'un jam en haut d'un onglet : les effets sont partagés (et qui les a réglés), ou restent personnels.
fn jam_note(roles: Roles, app: &App) -> Option<Element<'static, Message>> {
  let jam = app.jam.as_ref()?;
  let note = |line: String, color: Color| -> Element<'static, Message> {
    text(line)
      .size(t::NAME_SIZE)
      .font(fonts::MEDIUM)
      .color(color)
      .into()
  };
  let lines = match app.ui.audio.tab {
    AudioTab::Tempo | AudioTab::Effects => {
      let mut lines = vec![note(
        app.i18n.t("Effects are shared in the Jam"),
        roles.primary,
      )];
      if let Some(shared) = jam.shared.as_ref().filter(|shared| shared.sv > 0) {
        lines.push(note(
          app
            .i18n
            .t("Set by {name}")
            .replace("{name}", &jam.name_of(shared.by)),
          roles.variant,
        ));
      }
      lines
    }
    AudioTab::Equalizer => vec![note(app.i18n.t("Only for you"), roles.variant)],
    _ => return None,
  };
  Some(
    container(Column::with_children(lines).spacing(t::ITEM_GAP))
      .width(Fill)
      .padding(Padding::from([0.0, t::ITEM_PADDING_X]))
      .into(),
  )
}

fn panel<'a>(items: Vec<Element<'a, Message>>, gap: f32) -> Element<'a, Message> {
  Column::with_children(items).spacing(gap).width(Fill).into()
}

/// Coins d'une rangée `.setting-item` d'un panneau : `.audio-panel > .setting-item` (`styles.css:7682-7685`), 16 px partout dans Enhancer.
fn item_radius(enhance: bool, index: usize, count: usize) -> border::Radius {
  if enhance {
    return border::radius(t::ENHANCE_ITEM_RADIUS);
  }
  let (top, bottom) = (
    if index == 0 {
      t::ITEM_OUTER_RADIUS
    } else {
      t::ITEM_INNER_RADIUS
    },
    if index + 1 == count {
      t::ITEM_OUTER_RADIUS
    } else {
      t::ITEM_INNER_RADIUS
    },
  );
  border::Radius {
    top_left: top,
    top_right: top,
    bottom_right: bottom,
    bottom_left: bottom,
  }
}

/// Panneau de rangées `.setting-item` : chacune prend le calque `--hl` de `.setting-item:hover` (`styles.css:7520`, `on-surface` 4 % sur
/// `surface-container-high`), animé par `ControlHover`. Au repos la rangée n'a aucun fond propre, le rendu est celui de [`panel`].
fn setting_panel(
  roles: Roles,
  id: &'static str,
  items: Vec<Element<'static, Message>>,
  gap: f32,
) -> Element<'static, Message> {
  let count = items.len();
  let rows = items.into_iter().enumerate().map(|(index, item)| {
    let key = ControlKey::new(("audio-item", id, index));
    let radius = item_radius(id == "enhance", index, count);
    let fill = setting_fill(roles.on_surface, roles.high, key);
    let lit = setting_hl(key) > 0.0;
    let row = container(item)
      .width(Fill)
      .style(move |_: &Theme| container::Style {
        background: lit.then_some(Background::Color(fill)),
        border: border::rounded(radius),
        ..container::Style::default()
      });
    setting_hover(key, row)
  });
  Column::with_children(rows).spacing(gap).width(Fill).into()
}

/// `.beta-badge`.
fn badge<'a>(roles: Roles) -> Element<'a, Message> {
  container(text("BETA").size(t::BADGE_SIZE).font(fonts::BOLD))
    .padding(Padding::from([t::BADGE_PADDING_Y, t::BADGE_PADDING_X]))
    .style(move |_: &Theme| {
      container::Style::default()
        .background(roles.primary_container)
        .color(roles.on_primary_container)
        .border(border::rounded(t::BADGE_RADIUS))
    })
    .into()
}

fn hq_label<'a>(roles: Roles, label: String) -> Element<'a, Message> {
  text(label)
    .size(t::HQ_LABEL_SIZE)
    .font(fonts::SEMIBOLD)
    .color(roles.variant)
    .into()
}

/// `.hq-slider-group`.
fn hq_group(roles: Roles, app: &App, label: &str, field: SoundField) -> Element<'static, Message> {
  let value = shown(field, app.sound_value(field));
  container(
    column![
      head(roles, app.i18n.t(label), value, roles.primary),
      sound_slider(roles, app, field)
    ]
    .spacing(t::HQ_GROUP_GAP),
  )
  .width(Fill)
  .padding(Padding::from([
    t::HQ_GROUP_PADDING_Y,
    t::HQ_GROUP_PADDING_X,
  ]))
  .style(move |_: &Theme| {
    container::Style::default()
      .background(roles.container)
      .border(border::rounded(t::HQ_GROUP_RADIUS))
  })
  .into()
}

/// Choix du moteur : Riff ouvre un menu déroulant (`#hq-engine-dropdown`), ici les trois choix sont posés à plat.
fn hq_engines(roles: Roles, app: &App) -> Element<'static, Message> {
  let current = &app.settings.sound.hq.engine;
  let options = HQ_ENGINES.iter().map(|(id, label)| {
    let selected = current == id;
    let message = send(AudioMessage::HqEngine((*id).to_string()));
    let widget = button(
      container(
        text(app.i18n.t(label))
          .size(t::NAME_SIZE)
          .wrapping(Wrapping::None),
      )
      .height(Fill)
      .align_y(Vertical::Center),
    )
    .width(Fill)
    .height(t::HQ_ENGINE_HEIGHT)
    .padding(Padding::from([0.0, t::HQ_GROUP_PADDING_X]))
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      let fill = if selected {
        roles.secondary_container
      } else if hovered {
        roles.highest
      } else {
        roles.container
      };
      button::Style {
        background: Some(Background::Color(fill)),
        text_color: if selected {
          roles.on_secondary_container
        } else {
          roles.on_surface
        },
        border: border::rounded(t::HQ_GROUP_RADIUS),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
    focusable(widget)
      .on_activate(Some(message))
      .radius(t::HQ_GROUP_RADIUS)
      .into()
  });
  column![
    hq_label(roles, app.i18n.t("Enhancement Engine")),
    Column::with_children(options).spacing(t::HQ_GROUP_GAP)
  ]
  .spacing(t::HQ_GROUP_GAP)
  .into()
}

fn enhance(roles: Roles, app: &App) -> Element<'static, Message> {
  let sound = &app.settings.sound;
  let title = row![
    row![
      name(roles, app.i18n.t("High Quality")),
      container(badge(roles)).padding(Padding::new(0.0).left(t::BADGE_MARGIN)),
    ]
    .align_y(Vertical::Center),
    space().width(Length::Fill),
    super::settings::switch_colors(
      [
        roles.primary,
        roles.on_primary,
        roles.outline,
        roles.highest
      ],
      sound.hq_enabled,
      send(AudioMessage::ToggleHq(!sound.hq_enabled))
    ),
  ]
  .align_y(Vertical::Center);
  let mut quality = Column::new().push(title);
  if sound.hq_enabled {
    quality = quality.push(
      column![
        hq_engines(roles, app),
        hq_group(roles, app, "Vocal Clarity", SoundField::HqVocal),
        hq_group(roles, app, "High-End Air", SoundField::HqAir),
        hq_group(roles, app, "Bass Depth", SoundField::HqBass),
      ]
      .spacing(t::HQ_GAP)
      .padding(Padding::new(0.0).top(t::HQ_GAP)),
    );
  }
  let normalize = bool_row(
    roles,
    app,
    "Normalize volume",
    Some("Keeps loudness similar between tracks (adjusts live, up to 9 dB)"),
    app.bool_pref(BoolKey::Normalize),
    Message::Settings(SettingsMessage::SetBool(
      BoolKey::Normalize,
      !app.bool_pref(BoolKey::Normalize),
    )),
  );
  setting_panel(
    roles,
    "enhance",
    vec![item(quality, t::ITEM_PADDING_Y), normalize],
    t::ENHANCE_GAP,
  )
}

/// Un réglage à bascule : nom, note facultative et interrupteur à droite.
fn bool_row(
  roles: Roles,
  app: &App,
  label: &str,
  hint: Option<&str>,
  on: bool,
  message: Message,
) -> Element<'static, Message> {
  let mut texts = Column::new()
    .push(name(roles, app.i18n.t(label)))
    .spacing(2);
  if let Some(hint) = hint {
    texts = texts.push(
      text(app.i18n.t(hint))
        .size(t::HINT_SIZE)
        .line_height(LineHeight::Relative(t::HINT_LINE))
        .color(blend(roles.outline, roles.high, t::HINT_OPACITY)),
    );
  }
  item(
    row![
      texts.width(Fill),
      super::settings::switch_colors(
        [
          roles.primary,
          roles.on_primary,
          roles.outline,
          roles.highest
        ],
        on,
        message,
      )
    ]
    .spacing(t::ROW_GAP)
    .align_y(Vertical::Center),
    if hint.is_some() {
      t::ITEM_PADDING_TALL
    } else {
      t::ITEM_PADDING_Y
    },
  )
}

/// Gain de volume : personnel, même en jam (il y porte la mention).
fn volume_item(roles: Roles, app: &App) -> Element<'static, Message> {
  let mut title = app.i18n.t("Volume gain");
  if app.jam.is_some() {
    title = format!("{title} - {}", app.i18n.t("Only for you"));
  }
  titled_slider_item(roles, app, title, SoundField::Volume)
}

fn tempo(roles: Roles, app: &App) -> Element<'static, Message> {
  setting_panel(
    roles,
    "tempo",
    vec![
      slider_item(roles, app, "Speed", SoundField::Speed),
      slider_item(roles, app, "Speed (pitch)", SoundField::SpeedPitch),
      slider_item(roles, app, "Pitch", SoundField::Pitch),
      volume_item(roles, app),
    ],
    t::PANEL_GAP,
  )
}

fn effects(roles: Roles, app: &App) -> Element<'static, Message> {
  setting_panel(
    roles,
    "effects",
    vec![
      slider_item(roles, app, "Reverb", SoundField::Reverb),
      slider_item(roles, app, "Distortion", SoundField::Distortion),
      slider_item(roles, app, "Echo", SoundField::Echo),
      slider_item(roles, app, "Muffled", SoundField::Muffled),
    ],
    t::PANEL_GAP,
  )
}

/// `.eq-row` : nom, curseur et valeur sur trois colonnes.
fn eq_row(roles: Roles, app: &App, label: &str, field: SoundField) -> Element<'static, Message> {
  container(
    row![
      container(
        text(app.i18n.t(label))
          .size(t::NAME_SIZE)
          .font(fonts::MEDIUM)
          .color(roles.variant)
      )
      .width(t::EQ_NAME_WIDTH),
      sound_slider(roles, app, field),
      container(
        text(shown(field, app.sound_value(field)))
          .size(t::EQ_VALUE_SIZE)
          .font(fonts::MEDIUM)
          .color(roles.primary)
      )
      .width(t::EQ_VALUE_WIDTH)
      .align_x(Horizontal::Right),
    ]
    .spacing(t::EQ_ROW_GAP)
    .align_y(Vertical::Center),
  )
  .width(Fill)
  .padding(Padding::from([t::EQ_ROW_PADDING_Y, t::EQ_ROW_PADDING_X]))
  .into()
}

/// `.eq-band` : curseur vertical de 180 px (piste de 4 px, curseur de 6 x 20 px) et sa fréquence.
fn band(roles: Roles, app: &App, index: usize) -> Element<'static, Message> {
  let field = SoundField::Band(index);
  let (low, high, step) = field.range();
  let fader = vertical_slider(low..=high, app.sound_value(field), move |next| {
    send(AudioMessage::Set(field, next))
  })
  .step(step)
  .on_release(send(AudioMessage::Commit))
  .width(t::EQ_THUMB_WIDTH)
  .height(t::EQ_FADER_HEIGHT)
  .style(move |_: &Theme, _| {
    let rail = Background::Color(roles.secondary_container);
    slider::Style {
      rail: slider::Rail {
        backgrounds: (rail, rail),
        width: t::EQ_RAIL,
        border: border::rounded(t::EQ_RAIL / 2.0),
      },
      handle: slider::Handle {
        shape: slider::HandleShape::Rectangle {
          width: t::EQ_THUMB_LENGTH,
          border_radius: border::radius(t::EQ_THUMB_RADIUS),
        },
        background: Background::Color(roles.primary),
        border_width: 0.0,
        border_color: Color::TRANSPARENT,
      },
    }
  });
  column![
    container(fader).width(Fill).center_x(Fill),
    container(
      text(BAND_LABELS[index])
        .size(t::EQ_FREQ_SIZE)
        .color(roles.variant)
    )
    .width(Fill)
    .center_x(Fill)
    .padding(Padding::new(0.0).top(t::EQ_FREQ_MARGIN)),
  ]
  .width(Fill)
  .into()
}

fn grid_line(roles: Roles) -> Element<'static, Message> {
  let color = blend(roles.outline_variant, roles.high, t::EQ_GRID_OPACITY);
  container(space())
    .width(Fill)
    .height(1)
    .style(move |_: &Theme| container::Style::default().background(color))
    .into()
}

/// `.eq-faders-container` : échelle `+12 / 0 / -12`, lignes de repère et dix curseurs.
fn faders(roles: Roles, app: &App) -> Element<'static, Message> {
  let scale_text = |label: &'static str| text(label).size(t::EQ_SCALE_SIZE).color(roles.variant);
  let scale = container(
    column![
      scale_text("+12"),
      space().height(Length::Fill),
      scale_text("0"),
      space().height(Length::Fill),
      scale_text("-12"),
    ]
    .align_x(Horizontal::Right),
  )
  .width(t::EQ_SCALE_WIDTH)
  .height(t::EQ_FADER_HEIGHT)
  .padding(Padding {
    top: t::EQ_SCALE_PADDING_Y,
    bottom: t::EQ_SCALE_PADDING_Y,
    right: t::EQ_SCALE_PADDING_RIGHT,
    left: 0.0,
  });
  let lines = column![
    space().height(t::EQ_GRID_INSET),
    grid_line(roles),
    space().height(Length::Fill),
    grid_line(roles),
    space().height(Length::Fill),
    grid_line(roles),
    space().height(t::EQ_GRID_INSET),
  ]
  .height(t::EQ_FADER_HEIGHT);
  let bands = Row::with_children((0..BAND_LABELS.len()).map(|index| band(roles, app, index)))
    .spacing(t::EQ_BANDS_GAP)
    .width(Fill);
  row![scale, stack![lines, bands].width(Fill)]
    .spacing(t::EQ_FADERS_GAP)
    .width(Fill)
    .into()
}

fn equalizer(roles: Roles, app: &App) -> Element<'static, Message> {
  let divider = container(space())
    .width(Fill)
    .height(1)
    .style(move |_: &Theme| container::Style::default().background(roles.outline_variant));
  let header = row![
    text(app.i18n.t("10-band Equalizer"))
      .size(tokens::text::BODY)
      .font(fonts::MEDIUM)
      .color(roles.on_surface),
    space().width(Length::Fill),
    pill(
      roles,
      app.i18n.t("Reset"),
      false,
      Some(send(AudioMessage::ResetBands))
    ),
  ]
  .align_y(Vertical::Center);
  let section = column![
    divider,
    column![header, faders(roles, app)]
      .spacing(t::EQ_HEADER_GAP)
      .padding(Padding::new(0.0).top(t::EQ_SECTION_PADDING)),
  ];
  panel(
    vec![
      eq_row(roles, app, "Low", SoundField::Low),
      eq_row(roles, app, "Mid", SoundField::Mid),
      eq_row(roles, app, "High", SoundField::High),
      container(section)
        .padding(Padding::new(0.0).top(t::EQ_SECTION_MARGIN))
        .into(),
    ],
    t::PANEL_GAP,
  )
}

/// `#sleep-timer-status.active::before` : point de 6 px `currentColor`, `sleepDot` en opacité et en échelle autour de son centre.
fn sleep_dot<'a>(color: Color, pose: DotPose) -> Element<'a, Message> {
  let size = t::DOT_SIZE * pose.scale;
  let fill = Color {
    a: color.a * pose.opacity,
    ..color
  };
  container(
    container(space())
      .width(size)
      .height(size)
      .style(move |_: &Theme| {
        container::Style::default()
          .background(fill)
          .border(border::rounded(size / 2.0))
      }),
  )
  .center(t::DOT_SIZE)
  .into()
}

fn sleep(roles: Roles, app: &App) -> Element<'static, Message> {
  let timer = app.ui.audio.sleep;
  let t_ = |key: &str| app.i18n.t(key);
  let minutes = SLEEP_MINUTES.into_iter().map(|minutes| {
    let choice = SleepChoice::Minutes(minutes);
    (format!("{minutes} {}", t_("min")), choice)
  });
  let choices = minutes.chain([
    (t_("End of track"), SleepChoice::EndOfTrack),
    (t_("Turn off"), SleepChoice::Off),
  ]);
  let buttons = choices.map(|(label, choice)| {
    pill(
      roles,
      label,
      choice != SleepChoice::Off && timer.selects(choice),
      Some(send(AudioMessage::Sleep(choice))),
    )
  });
  let counting = timer != crate::audio_prefs::SleepTimer::Off;
  let status = timer.status(Instant::now(), t_);
  let options = Row::with_children(buttons)
    .spacing(t::SLEEP_GAP)
    .wrap()
    .vertical_spacing(t::SLEEP_GAP);
  setting_panel(
    roles,
    "sleep",
    vec![item(
      column![
        if counting {
          // `display: inline-flex; gap: 6px` : le point précède le texte (`styles.css:9881-9886`).
          let value: Element<'static, Message> = row![
            sleep_dot(roles.primary, app.ui.audio.motion.dot_pose(app.now)),
            text(status)
              .size(t::NAME_SIZE)
              .font(fonts::MEDIUM)
              .color(roles.primary),
          ]
          .spacing(t::DOT_GAP)
          .align_y(Vertical::Center)
          .into();
          head_with(roles, t_("Sleep timer"), value)
        } else {
          head(roles, t_("Sleep timer"), status, roles.on_surface)
        },
        container(options).padding(Padding::new(0.0).top(t::SLEEP_MARGIN)),
        container(
          text(t_("Fades out and pauses when the timer runs out."))
            .size(t::HINT_SIZE)
            .color(roles.outline)
        )
        .padding(Padding::new(0.0).top(t::SLEEP_HINT_MARGIN)),
      ]
      .spacing(t::ITEM_GAP),
      t::ITEM_PADDING_Y,
    )],
    t::PANEL_GAP,
  )
}

/// Préréglages (Riff les range dans le tiroir de la barre latérale, `index.html:538`).
fn presets(roles: Roles, app: &App) -> Element<'static, Message> {
  let active = app.active_preset();
  let chips = app.all_presets().into_iter().map(|preset| {
    let selected = preset.id == active;
    let is_custom = app
      .settings
      .sound
      .custom_presets
      .iter()
      .any(|custom| custom.id == preset.id);
    let label = if is_custom {
      preset.name.clone()
    } else {
      app.i18n.t(&preset.name)
    };
    // Les boutons de la ligne portent l'identifiant du préréglage : la clé suit la ligne quand la liste change.
    motion::keyed(preset.id.clone(), || {
      let mut line = Row::new()
        .push(pill(
          roles,
          label,
          selected,
          Some(send(AudioMessage::ApplyPreset(preset.id.clone()))),
        ))
        .align_y(Vertical::Center);
      if is_custom {
        line = line
          .push(tip(
            icon(
              icons::EDIT,
              Some(send(AudioMessage::RenamePreset(preset.id.clone()))),
              24,
              24,
            ),
            app.i18n.t("Edit"),
          ))
          .push(tip(
            icon(
              icons::CLOSE,
              Some(send(AudioMessage::DeletePreset(preset.id))),
              24,
              24,
            ),
            app.i18n.t("Delete"),
          ));
      }
      line.into()
    })
  });
  let mut block = column![
    row![
      name(roles, app.i18n.t("Presets")),
      space().width(Length::Fill),
      pill(
        roles,
        app.i18n.t("Save preset"),
        false,
        Some(send(AudioMessage::NewPreset))
      )
    ]
    .align_y(Vertical::Center),
    Row::with_children(chips)
      .spacing(t::SLEEP_GAP)
      .wrap()
      .vertical_spacing(t::SLEEP_GAP),
  ]
  .spacing(t::ITEM_GAP);
  if let Some(edit) = &app.ui.audio.edit {
    block = block.push(
      row![
        focusable(text_field(
          PRESET_NAME_ID,
          &app.i18n.t("Preset name..."),
          &edit.name,
          |name| send(AudioMessage::PresetName(name)),
          Some(send(AudioMessage::PresetSave))
        )),
        pill(
          roles,
          app.i18n.t("Save"),
          true,
          Some(send(AudioMessage::PresetSave))
        ),
        pill(
          roles,
          app.i18n.t("Cancel"),
          false,
          Some(send(AudioMessage::PresetCancel))
        ),
      ]
      .spacing(t::SLEEP_GAP)
      .align_y(Vertical::Center),
    );
  }
  item(block, t::ITEM_PADDING_Y)
}

/// Sortie audio, fondu enchaîné et options du son : absents de la fenêtre de Riff (ils sont dans ses réglages).
fn output(roles: Roles, app: &App) -> Element<'static, Message> {
  let sound = &app.settings.sound;
  let mut devices = Column::new().spacing(t::SLEEP_GAP).push(pill(
    roles,
    app.i18n.t("Default device"),
    sound.output_device.is_none(),
    Some(send(AudioMessage::Device(None))),
  ));
  for device in &app.ui.audio.devices {
    devices = devices.push(pill(
      roles,
      device.name.clone(),
      sound.output_device.as_deref() == Some(device.id.as_str()),
      Some(send(AudioMessage::Device(Some(device.id.clone())))),
    ));
  }
  let crossfade = app.int_pref(IntKey::Crossfade);
  let fade = item(
    column![
      head(
        roles,
        app.i18n.t("Crossfade"),
        format!("{crossfade} s"),
        roles.on_surface
      ),
      pill_slider(
        roles,
        (0.0, 12.0, 1.0),
        crossfade as f32,
        |next| Message::Settings(SettingsMessage::SetInt(
          IntKey::Crossfade,
          next.round() as i32
        )),
        Message::Settings(SettingsMessage::CommitInt),
      ),
    ]
    .spacing(t::ITEM_GAP),
    t::ITEM_PADDING_Y,
  );
  panel(
    vec![
      presets(roles, app),
      item(
        column![name(roles, app.i18n.t("Audio output device")), devices].spacing(t::ITEM_GAP),
        t::ITEM_PADDING_Y,
      ),
      fade,
      bool_row(
        roles,
        app,
        "Smooth preset transitions",
        None,
        sound.smooth_presets,
        send(AudioMessage::ToggleSmooth(!sound.smooth_presets)),
      ),
      bool_row(
        roles,
        app,
        "Save sound settings",
        None,
        sound.persist,
        send(AudioMessage::TogglePersist(!sound.persist)),
      ),
    ],
    t::PANEL_GAP,
  )
}

/// `.settings-card-header` : icône et titre à gauche, bouton de fermeture rond à droite.
fn header(roles: Roles, app: &App) -> Element<'static, Message> {
  let close = {
    let message = Message::Ui(UiMessage::ModalClose);
    let widget = button(container(icons::glyph_px(icons::CLOSE, 16.0)).center(Fill))
      .padding(0)
      .width(t::CLOSE_SIZE)
      .height(t::CLOSE_SIZE)
      .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
          background: Some(Background::Color(if hovered {
            roles.highest
          } else {
            roles.container
          })),
          text_color: if hovered {
            roles.on_surface
          } else {
            roles.variant
          },
          border: border::rounded(tokens::radius::FULL),
          ..button::Style::default()
        }
      })
      .on_press(message.clone());
    focusable(widget)
      .on_activate(Some(message))
      .radius(t::CLOSE_SIZE / 2.0)
  };
  row![
    svg_icon(
      include_bytes!("../../assets/icons/audio.svg"),
      t::TITLE_ICON,
      roles.on_surface
    ),
    text(app.i18n.t("Audio"))
      .size(t::TITLE_SIZE)
      .font(fonts::MEDIUM)
      .color(roles.on_surface),
    space().width(Length::Fill),
    close,
  ]
  .spacing(t::TITLE_GAP)
  .align_y(Vertical::Center)
  .into()
}

/// Contenu animé : fondu par un voile de la couleur du fond de la carte (uni sous le contenu), décalage vertical dans le repère de
/// la vue ; la coupe s'étend du décalage vers le bas (aucun `overflow` sur ces éléments en CSS).
fn animated<'a>(
  content: Element<'a, Message>,
  pose: Option<ViewPose>,
  veil: Color,
) -> Element<'a, Message> {
  let spill = pose.map_or(0.0, |pose| pose.offset_y.max(0.0));
  view_transition(content, pose, Some(veil))
    .pop(0.0)
    .spill(spill)
    .into()
}

/// Carte de la fenêtre. `pose` : dévoilement ou repli en cours (`None` au repos).
fn card(app: &App, pose: Option<AudioPose>) -> Element<'_, Message> {
  let roles = Roles::of(app);
  let tabs = Column::with_children(AudioTab::ALL.into_iter().map(|id| tab(roles, app, id)))
    .spacing(t::TABS_GAP)
    .width(t::TABS_WIDTH);
  let panel = match app.ui.audio.tab {
    AudioTab::Enhance => enhance(roles, app),
    AudioTab::Tempo => tempo(roles, app),
    AudioTab::Effects => effects(roles, app),
    AudioTab::Equalizer => equalizer(roles, app),
    AudioTab::Sleep => sleep(roles, app),
    AudioTab::Output => output(roles, app),
  };
  let panel = match jam_note(roles, app) {
    Some(note) => column![note, panel]
      .spacing(t::PANEL_GAP)
      .width(Fill)
      .into(),
    None => panel,
  };
  let panel = animated(panel, app.ui.audio.motion.panel_pose(app.now), roles.high);
  let body = scrollable(container(panel).width(Fill).padding(Padding {
    top: t::BODY_PADDING_TOP + t::PANEL_PADDING_TOP,
    bottom: t::BODY_PADDING_BOTTOM + t::PANEL_PADDING_BOTTOM,
    left: t::BODY_PADDING_X + t::PANEL_PADDING_X,
    right: t::BODY_PADDING_X + t::PANEL_PADDING_X,
  }))
  .direction(Direction::Vertical(Scrollbar::hidden()))
  .width(Fill)
  .height(Fill);
  let footer = container(pill(
    roles,
    app.i18n.t("Reset audio settings"),
    false,
    Some(send(AudioMessage::ResetAll)),
  ))
  .width(Fill)
  .align_x(Horizontal::Right)
  .padding(Padding {
    top: t::FOOTER_PADDING_TOP,
    right: t::FOOTER_PADDING_X,
    bottom: t::FOOTER_PADDING_BOTTOM,
    left: t::FOOTER_PADDING_X,
  });
  let content = pose.map(|pose| pose.content);
  let card = container(
    column![
      animated(header(roles, app), content, roles.high),
      animated(
        row![tabs, body].spacing(t::GAP).height(Fill).into(),
        content,
        roles.high
      ),
      footer
    ]
    .spacing(t::GAP),
  )
  .id(OVERLAY_ID)
  .width(t::CARD_WIDTH)
  .height(t::CARD_HEIGHT)
  .padding(t::CARD_PADDING)
  .style(move |_: &Theme| {
    let style = container::Style::default().color(roles.on_surface);
    // Pendant le dévoilement, `reveal` dessine le fond aux coins arrondis de la partie visible.
    if pose.is_some() {
      return style;
    }
    style.background(roles.high).border(Border {
      radius: border::Radius {
        top_left: t::CARD_RADIUS,
        top_right: t::CARD_RADIUS,
        bottom_right: 0.0,
        bottom_left: 0.0,
      },
      ..Border::default()
    })
  });
  reveal(
    card,
    pose.map(|pose| pose.visible),
    roles.high,
    t::CARD_RADIUS,
  )
  .into()
}

/// Carte collée au bord droit et posée sur la barre de lecture.
fn anchored<'a>(card: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
  container(card)
    .width(Fill)
    .height(Fill)
    .align_x(Horizontal::Right)
    .align_y(Vertical::Bottom)
    .padding(Padding {
      right: t::CARD_RIGHT,
      bottom: tokens::playbar::HEIGHT,
      ..Padding::ZERO
    })
    .into()
}

/// Fenêtre ouverte : sans voile, collée au bord droit et posée sur la barre de lecture ; un clic dehors la ferme.
#[must_use]
pub fn overlay(app: &App) -> Element<'_, Message> {
  let outside = mouse_area(container(space()).width(Fill).height(Fill))
    .on_press(Message::Ui(UiMessage::ModalClose));
  let pose = app.ui.audio.motion.card_pose(app.now);
  let anchored = anchored(opaque(card(app, pose)));
  opaque(stack![outside, anchored].width(Fill).height(Fill))
}

/// Carte refermée : `connected-closing` la garde 200 ms (`app.js:14961-14967`), visible et sans interaction.
#[must_use]
pub fn closing(app: &App) -> Option<Element<'_, Message>> {
  if app.ui.modal == Some(crate::ui::Modal::Audio) || !app.ui.audio.motion.closing(app.now) {
    return None;
  }
  let pose = app.ui.audio.motion.card_pose(app.now)?;
  Some(anchored(inert(card(app, Some(pose)))))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn values_follow_the_formats_of_riff() {
    assert_eq!(shown(SoundField::Speed, 1.0), "1.00x");
    assert_eq!(shown(SoundField::SpeedPitch, 0.85), "0.85x");
    assert_eq!(shown(SoundField::Pitch, 0.0), "0 st");
    assert_eq!(shown(SoundField::Pitch, 3.0), "+3 st");
    assert_eq!(shown(SoundField::Pitch, -2.0), "-2 st");
    assert_eq!(shown(SoundField::Volume, 1.0), "100%");
    assert_eq!(shown(SoundField::Volume, 1.25), "125%");
    assert_eq!(shown(SoundField::Reverb, 0.0), "0%");
    assert_eq!(shown(SoundField::Echo, 15.0), "15%");
    assert_eq!(shown(SoundField::Muffled, 85.0), "85%");
    assert_eq!(shown(SoundField::Low, 0.0), "0 dB");
    assert_eq!(shown(SoundField::High, 4.0), "+4 dB");
    assert_eq!(shown(SoundField::Band(2), -6.0), "-6 dB");
    assert_eq!(shown(SoundField::HqVocal, 12.0), "+12%");
    assert_eq!(shown(SoundField::HqBass, -5.0), "-5%");
  }

  #[test]
  fn every_tab_builds_its_view() {
    use crate::i18n::I18n;
    use crate::settings::NativeSettings;
    use crate::window::PersistedWindowState;
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    for tab in AudioTab::ALL {
      app.ui.audio.tab = tab;
      let _ = overlay(&app);
    }
  }
}
