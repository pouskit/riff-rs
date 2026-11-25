//! Éditeur de thème : nom, mode, couleur source, overrides, avec contrôle de contraste. L'aperçu est
//! l'application elle-même, mise à jour à chaque changement valide.

use super::chrome::tip;
use crate::app::{App, Message};
use crate::theme::icons;
use crate::theme_editor::{PRESETS, ThemeDraft, ThemeEditorMessage, free_roles};
use crate::widgets::button::{filled, icon, text_button, tonal};
use crate::widgets::text_field::text_field;
use iced::alignment::Vertical;
use iced::widget::{Column, button, column, container, pick_list, row, space, text, toggler};
use iced::{Color, Element, Fill, Theme, border};

const SWATCH: f32 = 22.0;
const NAME_ID: &str = "theme-editor-name";
const SOURCE_ID: &str = "theme-editor-source";
const OVERRIDE_ID: &str = "theme-editor-override";

fn send(message: ThemeEditorMessage) -> Message {
  Message::ThemeEditor(message)
}

fn swatch<'a>(hex: &str, message: Option<Message>) -> Element<'a, Message> {
  let color = hex.parse::<Color>().unwrap_or(Color::TRANSPARENT);
  let body = container(space())
    .width(SWATCH)
    .height(SWATCH)
    .style(move |_: &Theme| {
      container::Style::default().background(color).border(
        border::rounded(6)
          .width(1)
          .color(Color::from_rgba(0.5, 0.5, 0.5, 0.6)),
      )
    });
  match message {
    Some(message) => button(body).padding(0).on_press(message).into(),
    None => body.into(),
  }
}

/// Sélecteur de couleur : nuancier d'un clic, ou saisie `#RRGGBB`.
fn color_picker<'a>(
  id: &'static str,
  value: &'a str,
  change: impl Fn(String) -> Message + Clone + 'a,
) -> Element<'a, Message> {
  let presets = PRESETS
    .iter()
    .map(|hex| swatch(hex, Some(change(hex.to_string()))));
  column![
    row![
      swatch(value, None),
      container(text_field(id, "#RRGGBB", value, change.clone(), None)).width(120),
    ]
    .spacing(8)
    .align_y(Vertical::Center),
    iced::widget::Row::with_children(presets).spacing(4),
  ]
  .spacing(6)
  .into()
}

/// Panneau de l'éditeur de thème ouvert.
#[must_use]
pub fn panel<'a>(app: &'a App, draft: &'a ThemeDraft) -> Element<'a, Message> {
  let overrides = draft
    .overrides
    .iter()
    .enumerate()
    .map(|(index, (role, color))| {
      row![
        text(role.clone()).size(12).width(220),
        color_picker(OVERRIDE_ID, color, move |value| {
          send(ThemeEditorMessage::OverrideColor(index, value))
        }),
        tip(
          icon(
            icons::CLOSE,
            Some(send(ThemeEditorMessage::RemoveOverride(index))),
            28,
            28
          ),
          app.i18n.t("Remove")
        ),
      ]
      .spacing(8)
      .align_y(Vertical::Center)
      .into()
    });
  let check: Element<'_, Message> = match &draft.problem {
    None => text(app.i18n.t("Contrast OK: all 13 text pairs reach 4.5:1."))
      .size(12)
      .into(),
    Some(problem) => text(problem.clone())
      .size(12)
      .color(app.theme.theme.palette().danger)
      .into(),
  };
  column![
    row![
      text(app.i18n.t("Name")).size(13).width(120),
      container(text_field(
        NAME_ID,
        "",
        &draft.name,
        |value| send(ThemeEditorMessage::Name(value)),
        None
      ))
      .width(240),
      text(format!("{}.json", draft.id)).size(12),
    ]
    .spacing(8)
    .align_y(Vertical::Center),
    row![
      text(app.i18n.t("Dark")).size(13).width(120),
      toggler(draft.dark).on_toggle(|dark| send(ThemeEditorMessage::Dark(dark))),
    ]
    .spacing(8)
    .align_y(Vertical::Center),
    row![
      text(app.i18n.t("Source color")).size(13).width(120),
      color_picker(SOURCE_ID, &draft.source, |value| send(
        ThemeEditorMessage::Source(value)
      )),
    ]
    .spacing(8),
    text(app.i18n.t("Overrides")).size(13),
    Column::with_children(overrides).spacing(6),
    row![
      pick_list(free_roles(draft), draft.role.clone(), |role| send(
        ThemeEditorMessage::Role(role)
      ))
      .placeholder(app.i18n.t("Role")),
      text_button(
        app.i18n.t("Add override"),
        draft
          .role
          .is_some()
          .then(|| send(ThemeEditorMessage::AddOverride))
      ),
    ]
    .spacing(8)
    .align_y(Vertical::Center),
    check,
    row![
      text_button(
        app.i18n.t("Edit as text"),
        Some(send(ThemeEditorMessage::EditAsText))
      ),
      space().width(Fill),
      tonal(app.i18n.t("Cancel"), Some(send(ThemeEditorMessage::Cancel))),
      filled(
        app.i18n.t("Save"),
        draft
          .problem
          .is_none()
          .then(|| send(ThemeEditorMessage::Save))
      ),
    ]
    .spacing(8)
    .align_y(Vertical::Center),
  ]
  .spacing(10)
  .width(Fill)
  .into()
}
