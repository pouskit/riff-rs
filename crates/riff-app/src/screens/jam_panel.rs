//! Panneau du jam : même carte, même ancrage et même mouvement que le panneau de la file (`queue_panel.rs`).
//! Le contenu suit l'état du jam (voir `docs/specs/21-jam.md`, section 10.2). Aucune mesure de texte hors
//! `ellipsize` (cache borné) : les textes longs passent à la ligne par le `wrapping` du widget.

use super::chrome::{tip, tips};
use super::home::{Roles, blend, page_scroll_style};
use super::library::outline_style;
use super::queue_panel::{fade_roles, heading, layer, rule};
use super::right_panel::ellipsize;
use super::sidebar::{faded, rgb};
use crate::app::{App, Message};
use crate::jam::JamMessage;
use crate::jam::state::{JamState, Phase};
use crate::theme::tokens::local as button_metrics;
use crate::theme::tokens::queue as metrics;
use crate::theme::{fonts, icons};
use crate::widgets::focus::focusable;
use crate::widgets::text_field::text_field;
use iced::alignment::Vertical;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Column, button, column, container, row, scrollable, space, text};
use iced::{Background, Element, Fill, Padding, border};

/// Identifiants des champs du panneau.
pub const CODE_ID: &str = "jam-code";
pub const NAME_ID: &str = "jam-name";

/// Largeur du code d'invitation tronqué : de quoi laisser la place du bouton « Copy ».
const TICKET_WIDTH: f32 = 220.0;
const BODY_TEXT: f32 = 13.0;
const TITLE_TEXT: f32 = 18.0;
const CONTENT_GAP: f32 = 12.0;
const CLOSE_RADIUS: f32 = 9999.0;

fn jam(message: JamMessage) -> Message {
  Message::Jam(message)
}

/// Bouton texte : `.btn-outline-pill`, comme « Clear queue ».
fn action<'a>(app: &App, c: Roles, label: &str, message: Message) -> Element<'a, Message> {
  let widget = button(
    container(
      text(app.i18n.t(label))
        .size(button_metrics::BUTTON_TEXT)
        .font(fonts::MEDIUM),
    )
    .height(Fill)
    .align_y(Vertical::Center),
  )
  .padding(
    Padding::ZERO
      .left(button_metrics::OUTLINE_PADDING_X)
      .right(button_metrics::OUTLINE_PADDING_X),
  )
  .height(metrics::HEADER_HEIGHT)
  .on_press(message.clone())
  .style(outline_style(c));
  focusable(widget)
    .on_activate(Some(message))
    .radius(crate::theme::tokens::css(18.0))
    .into()
}

/// Paragraphe : passe à la ligne par le widget, sans mesure.
fn paragraph<'a>(c: Roles, content: String) -> Element<'a, Message> {
  text(content)
    .size(BODY_TEXT)
    .color(c.variant)
    .width(Fill)
    .into()
}

/// `.np-queue-close` : même bouton que celui de la file, pour le panneau du jam.
fn close_button<'a>(app: &App, c: Roles) -> Element<'a, Message> {
  let message = jam(JamMessage::TogglePanel);
  let entry = button(container(icons::glyph_px(icons::CLOSE, metrics::CLOSE_ICON)).center(Fill))
    .padding(0)
    .width(metrics::CLOSE_SIZE)
    .height(metrics::CLOSE_SIZE)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: hovered
          .then(|| Background::Color(faded(c.on_surface, metrics::CLOSE_HOVER_SHARE))),
        text_color: c.on_surface,
        border: border::rounded(CLOSE_RADIUS),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  tip(
    focusable(entry)
      .on_activate(Some(message))
      .radius(metrics::CLOSE_SIZE / 2.0)
      .into(),
    app.i18n.t(tips::CLOSE),
  )
}

fn firewall<'a>(app: &App, c: Roles) -> Element<'a, Message> {
  paragraph(
    c,
    app
      .i18n
      .t("Windows may ask you to allow Riff on the network the first time."),
  )
}

/// Participants, l'hôte marqué « (host) ».
fn participants<'a>(app: &App, c: Roles, state: &JamState) -> Vec<Element<'a, Message>> {
  let host = app.i18n.t("(host)");
  let mut rows = vec![heading(app, c, "Participants", false)];
  rows.extend(state.participants.iter().map(|participant| {
    let label = if participant.host {
      format!("{} {host}", participant.name)
    } else {
      participant.name.clone()
    };
    text(label)
      .size(BODY_TEXT)
      .font(fonts::MEDIUM)
      .color(c.on_surface)
      .width(Fill)
      .into()
  }));
  rows
}

/// Contenu hors jam : démarrer ou rejoindre.
fn idle<'a>(app: &App, c: Roles) -> Vec<Element<'a, Message>> {
  let panel = &app.ui.jam_panel;
  let code = row![
    text_field(
      CODE_ID,
      &app.i18n.t("Paste an invite code"),
      &panel.code,
      |value| jam(JamMessage::CodeInput(value)),
      Some(jam(JamMessage::Join)),
    ),
    action(app, c, "Paste", jam(JamMessage::Paste)),
  ]
  .spacing(8)
  .align_y(Vertical::Center);
  vec![
    paragraph(
      c,
      app
        .i18n
        .t("Listen to the same music with friends. Everyone can add tracks, pause and skip."),
    ),
    text_field(
      NAME_ID,
      &app.i18n.t("Your name"),
      &panel.name,
      |value| jam(JamMessage::NameInput(value)),
      None,
    ),
    action(app, c, "Start a Jam", jam(JamMessage::Start)),
    rule(faded(c.outline, 0.4)),
    heading(app, c, "Join a Jam", true),
    code.into(),
    action(app, c, "Join", jam(JamMessage::Join)),
  ]
}

/// Contenu de l'hôte.
fn host<'a>(app: &App, c: Roles, state: &JamState) -> Vec<Element<'a, Message>> {
  if state.phase == Phase::Starting {
    return vec![
      paragraph(c, app.i18n.t("Starting the Jam…")),
      firewall(app, c),
      action(app, c, "End Jam", jam(JamMessage::End)),
    ];
  }
  let ticket = state.ticket.as_deref().unwrap_or_default();
  let mut rows = vec![
    heading(app, c, "Invite code", true),
    row![
      text(ellipsize(ticket, BODY_TEXT, fonts::UI, TICKET_WIDTH))
        .size(BODY_TEXT)
        .color(c.on_surface)
        .wrapping(iced::widget::text::Wrapping::None),
      space().width(Fill),
      action(app, c, "Copy", jam(JamMessage::CopyCode)),
    ]
    .align_y(Vertical::Center)
    .into(),
    paragraph(
      c,
      app.i18n.t(
        "Share this code with your friends. The connection is direct when possible, otherwise through an encrypted relay.",
      ),
    ),
    firewall(app, c),
  ];
  rows.extend(participants(app, c, state));
  rows.push(action(app, c, "End Jam", jam(JamMessage::End)));
  rows
}

/// Contenu de l'invité.
fn guest<'a>(app: &App, c: Roles, state: &JamState) -> Vec<Element<'a, Message>> {
  let mut rows: Vec<Element<'a, Message>> = Vec::new();
  match state.phase {
    Phase::Starting => rows.push(paragraph(c, app.i18n.t("Connecting…"))),
    Phase::Live => {
      rows.push(paragraph(
        c,
        app
          .i18n
          .t("Jam hosted by {name}")
          .replace("{name}", &state.host_name()),
      ));
      rows.extend(participants(app, c, state));
    }
    Phase::Reconnecting(attempt) => rows.push(paragraph(
      c,
      app
        .i18n
        .t("Reconnecting ({n}/3)…")
        .replace("{n}", &attempt.to_string()),
    )),
  }
  rows.push(action(app, c, "Leave Jam", jam(JamMessage::Leave)));
  rows
}

fn content<'a>(app: &App, c: Roles) -> Vec<Element<'a, Message>> {
  match app.jam.as_deref() {
    None => idle(app, c),
    Some(state) if state.is_host() => host(app, c, state),
    Some(state) => guest(app, c, state),
  }
}

/// Panneau du jam : calque seulement ouvert ou en train de se fermer.
pub fn jam_panel(app: &App) -> Option<Element<'_, Message>> {
  let panel = &app.ui.jam_panel;
  if !panel.motion.visible(panel.open, app.now) {
    return None;
  }
  let pose = panel.motion.pose(app.now);
  let alpha = pose.map_or(1.0, |pose| pose.opacity);
  let c = Roles::of(app);
  let line = faded(
    blend(rgb(app.theme.scheme.outline_variant), c.high, 0.55),
    alpha,
  );
  let c = fade_roles(c, alpha);
  let title = text(app.i18n.t("Jam"))
    .size(TITLE_TEXT)
    .font(fonts::BOLD)
    .color(c.on_surface);
  let head = container(
    row![title, space().width(Fill), close_button(app, c)]
      .spacing(metrics::HEAD_GAP)
      .align_y(Vertical::Center),
  )
  .padding(Padding {
    top: metrics::HEAD_PADDING_TOP,
    right: metrics::HEAD_PADDING_X,
    bottom: metrics::CLOSE_MARGIN_BOTTOM,
    left: metrics::HEAD_PADDING_X + 6.0,
  });
  let body = scrollable(
    Column::from_vec(content(app, c))
      .spacing(CONTENT_GAP)
      .width(Fill)
      .padding(Padding {
        top: CONTENT_GAP,
        right: metrics::HEAD_PADDING_X,
        bottom: metrics::BODY_PADDING_BOTTOM,
        left: metrics::HEAD_PADDING_X,
      }),
  )
  .direction(Direction::Vertical(
    Scrollbar::new().width(5).scroller_width(5).margin(0),
  ))
  .style(move |_, status| page_scroll_style(c, status))
  .width(Fill)
  .height(Fill);
  let card = container(column![head, rule(line), body])
    .width(metrics::PANEL_WIDTH)
    .height(Fill)
    .clip(true)
    .style(move |_| {
      container::Style::default()
        .background(c.high)
        .border(border::rounded(metrics::PANEL_RADIUS))
    });
  Some(layer(card, pose, panel.open))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::jam::Role;

  fn app() -> App {
    App::new(
      crate::window::PersistedWindowState::default(),
      crate::settings::NativeSettings::default(),
      crate::i18n::I18n::default(),
    )
  }

  #[test]
  fn the_jam_panel_is_a_layer_only_while_open() {
    let mut app = app();
    assert!(jam_panel(&app).is_none());
    app.update(Message::Jam(JamMessage::TogglePanel));
    assert!(jam_panel(&app).is_some());
    app.update(Message::Jam(JamMessage::TogglePanel));
    assert!(jam_panel(&app).is_none());
  }

  #[test]
  fn opening_one_panel_closes_the_other() {
    let mut app = app();
    app.update(Message::Ui(crate::ui::UiMessage::ToggleQueuePanel));
    assert!(app.ui.queue_panel.open);
    app.update(Message::Jam(JamMessage::TogglePanel));
    assert!(app.ui.jam_panel.open && !app.ui.queue_panel.open);
    app.update(Message::Ui(crate::ui::UiMessage::ToggleQueuePanel));
    assert!(app.ui.queue_panel.open && !app.ui.jam_panel.open);
  }

  #[test]
  fn every_jam_state_has_a_panel_content() {
    let mut app = app();
    let c = Roles::of(&app);
    assert_eq!(content(&app, c).len(), 7);
    for (role, phase) in [
      (Role::Host, Phase::Starting),
      (Role::Host, Phase::Live),
      (Role::Guest, Phase::Starting),
      (Role::Guest, Phase::Live),
      (Role::Guest, Phase::Reconnecting(2)),
    ] {
      let mut state = JamState::new(role, "Ana");
      state.phase = phase;
      state.ticket = Some("riffjam".repeat(20));
      app.jam = Some(Box::new(state));
      assert!(!content(&app, c).is_empty());
    }
  }

  /// Au repos, panneau fermé comme ouvert, le panneau ne demande aucune image.
  #[test]
  fn the_jam_panel_asks_for_no_frames_at_rest() {
    let mut app = app();
    assert!(!app.animation_needed());
    app.view_motion.enabled = true;
    app.update(Message::Jam(JamMessage::TogglePanel));
    assert!(app.ui.jam_panel.motion.running(app.now));
    assert!(app.animation_needed());
    app.now += std::time::Duration::from_secs(5);
    assert!(app.ui.jam_panel.open && !app.animation_needed());
    app.update(Message::Jam(JamMessage::TogglePanel));
    assert!(app.animation_needed());
    app.now += std::time::Duration::from_secs(5);
    assert!(!app.ui.jam_panel.open && !app.animation_needed());
    assert!(jam_panel(&app).is_none());
  }
}
