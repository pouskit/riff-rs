//! Fenêtre de l'éditeur de code : coloration, barre d'état (ligne, colonne), recherche, enregistrement.
//!
//! Pas de colonne de numéros de ligne : l'API publique de `text_editor` (iced 0.14.0) ne donne pas son
//! défilement, la colonne ne pourrait pas le suivre. La barre d'état donne la position du curseur.

use crate::app::{App, Message};
use crate::code_editor::{CodeEditor, EditorMessage};
use crate::widgets::button::{filled, text_button, tonal};
use crate::widgets::text_field::text_field;
use iced::alignment::Vertical;
use iced::highlighter;
use iced::keyboard::{self, key};
use iced::widget::text_editor::{Binding, KeyPress, Status};
use iced::widget::{column, container, opaque, row, space, text, text_editor, toggler};
use iced::{Color, Element, Fill, Font, Theme, border};

const SCRIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.5);
const FIND_ID: &str = "code-editor-find";
const REPLACE_ID: &str = "code-editor-replace";

fn send(message: EditorMessage) -> Message {
  Message::Editor(message)
}

/// Ctrl+S enregistre, Ctrl+F ouvre la recherche, Échap demande la fermeture ; le reste suit iced.
fn binding(press: KeyPress) -> Option<Binding<Message>> {
  if !matches!(press.status, Status::Focused { .. }) {
    return None;
  }
  if press.modifiers.command() {
    match press.key.as_ref() {
      keyboard::Key::Character("s") => return Some(Binding::Custom(send(EditorMessage::Save))),
      keyboard::Key::Character("f") => {
        return Some(Binding::Custom(send(EditorMessage::ToggleFind)));
      }
      _ => {}
    }
  }
  if press.key == keyboard::Key::Named(key::Named::Escape) {
    return Some(Binding::Custom(send(EditorMessage::Close)));
  }
  Binding::from_key_press(press)
}

fn find_bar<'a>(app: &'a App, editor: &'a CodeEditor) -> Element<'a, Message> {
  row![
    container(text_field(
      FIND_ID,
      &app.i18n.t("Find"),
      &editor.find,
      |value| send(EditorMessage::Find(value)),
      Some(send(EditorMessage::Next)),
    ))
    .width(180),
    text_button(app.i18n.t("Previous"), Some(send(EditorMessage::Previous))),
    text_button(app.i18n.t("Next"), Some(send(EditorMessage::Next))),
    container(text_field(
      REPLACE_ID,
      &app.i18n.t("Replace with"),
      &editor.replace,
      |value| send(EditorMessage::Replace(value)),
      None,
    ))
    .width(160),
    text_button(app.i18n.t("Replace"), Some(send(EditorMessage::ReplaceOne))),
    text_button(
      app.i18n.t("Replace all"),
      Some(send(EditorMessage::ReplaceAll))
    ),
    toggler(editor.case_sensitive)
      .label(app.i18n.t("Match case"))
      .on_toggle(|_| send(EditorMessage::ToggleCase)),
  ]
  .spacing(6)
  .align_y(Vertical::Center)
  .into()
}

/// Superposition de l'éditeur, s'il est ouvert.
#[must_use]
pub fn view(app: &App) -> Option<Element<'_, Message>> {
  let editor = app.ui.editor.as_ref()?;
  let theme = if app.theme.dark {
    highlighter::Theme::Base16Ocean
  } else {
    highlighter::Theme::InspiredGitHub
  };
  let field = text_editor(&editor.content.0)
    .on_action(|action| send(EditorMessage::Action(action)))
    .key_binding(binding)
    .highlight(editor.target.syntax(), theme)
    .font(Font::MONOSPACE)
    .size(13)
    .height(Fill)
    .padding(10);
  let title = format!(
    "{}{}",
    editor.target.label(),
    if editor.modified { " •" } else { "" }
  );
  let (line, column) = editor.cursor_label();
  let status = format!(
    "{} {line}, {} {column} · {} {}{}",
    app.i18n.t("Line"),
    app.i18n.t("Column"),
    editor.content.0.line_count(),
    app.i18n.t("lines"),
    editor
      .status
      .as_ref()
      .map(|status| format!(" · {}", app.i18n.t(status)))
      .unwrap_or_default()
  );
  let actions: Element<'_, Message> = if editor.confirm_close {
    row![
      text(app.i18n.t("Unsaved changes. Close anyway?"))
        .size(13)
        .width(Fill),
      tonal(
        app.i18n.t("Keep editing"),
        Some(send(EditorMessage::KeepEditing))
      ),
      filled(
        app.i18n.t("Close without saving"),
        Some(send(EditorMessage::CloseAnyway))
      ),
    ]
    .spacing(8)
    .align_y(Vertical::Center)
    .into()
  } else {
    row![
      text_button(app.i18n.t("Find"), Some(send(EditorMessage::ToggleFind))),
      space().width(Fill),
      tonal(app.i18n.t("Close"), Some(send(EditorMessage::Close))),
      filled(app.i18n.t("Save"), Some(send(EditorMessage::Save))),
    ]
    .spacing(8)
    .align_y(Vertical::Center)
    .into()
  };
  let mut body = column![text(title).size(18)].spacing(10);
  if editor.find_open {
    body = body.push(find_bar(app, editor));
  }
  body = body.push(field).push(text(status).size(12)).push(actions);
  let surface = container(body)
    .padding(20)
    .width(Fill)
    .height(Fill)
    .style(|theme: &Theme| {
      container::Style::default()
        .background(theme.extended_palette().background.weak.color)
        .color(theme.palette().text)
        .border(border::rounded(16))
    });
  Some(opaque(container(surface).padding(40).center(Fill).style(
    |_: &Theme| container::Style::default().background(SCRIM),
  )))
}
