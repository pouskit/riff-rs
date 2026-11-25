//! Catégorie « Plugins » : liste, permissions, activation, import, suppression.

use super::chrome::tip;
use crate::app::{App, Message};
use crate::code_editor::{EditorMessage, EditorTarget};
use crate::plugins::PluginMessage;
use crate::theme::icons;
use crate::widgets::button::{icon, text_button};
use crate::widgets::focus::focusable;
use iced::alignment::Vertical;
use iced::widget::{Column, column, row, text, toggler};
use iced::{Element, Fill};
use riff_plugins::{ConsentState, Manifest, Permission, PluginEntry, PluginKind};

/// Ouvre le fichier dans l'éditeur de code (refusé pour un nom invalide).
fn edit_button<'a>(app: &App, file: &str) -> Element<'a, Message> {
  let target = EditorTarget::Plugin(file.to_string());
  let message = target
    .path()
    .is_ok()
    .then(|| Message::Editor(EditorMessage::Open(target)));
  text_button(app.i18n.t("Edit"), message)
}

fn send(message: PluginMessage) -> Message {
  Message::Plugin(message)
}

/// Ce que chaque permission autorise, montré avant l'activation.
const fn permission_text(permission: Permission) -> &'static str {
  match permission {
    Permission::Queue => "queue: read and add to the play queue",
    Permission::Library => "library: read favorites and history",
    Permission::Search => "search: search and get recommendations through Riff",
    Permission::Shuffle => "shuffle: choose the next track in shuffle",
    Permission::Notify => "notify: show notifications",
  }
}

fn permissions_view<'a>(app: &App, manifest: &Manifest) -> Element<'a, Message> {
  if manifest.permissions.is_empty() {
    return text(app.i18n.t("No permission")).size(12).into();
  }
  Column::with_children(manifest.permissions.iter().map(|perm| {
    text(format!("• {}", app.i18n.t(permission_text(*perm))))
      .size(12)
      .into()
  }))
  .spacing(2)
  .into()
}

fn native_row<'a>(
  app: &'a App,
  entry: &'a PluginEntry,
  manifest: &'a Manifest,
  consent: ConsentState,
) -> Element<'a, Message> {
  let file = entry.file.clone();
  let on = consent == ConsentState::Granted && app.plugins.names.contains_key(&file);
  let toggle = focusable(toggler(on).on_toggle({
    let file = file.clone();
    move |next| send(PluginMessage::Toggle(file.clone(), next))
  }))
  .on_activate(Some(send(PluginMessage::Toggle(file.clone(), !on))));
  let mut title = manifest.name.clone();
  if !manifest.version.is_empty() {
    title.push_str(&format!(" {}", manifest.version));
  }
  if !manifest.author.is_empty() {
    title.push_str(&format!(" — {}", manifest.author));
  }
  let mut details = column![
    row![
      text(title).size(14).width(Fill),
      edit_button(app, &entry.file),
      toggle,
      tip(
        icon(
          icons::CLOSE,
          Some(send(PluginMessage::Remove(file.clone()))),
          28,
          28
        ),
        app.i18n.t("Delete")
      ),
    ]
    .spacing(8)
    .align_y(Vertical::Center),
    text(format!("{} · {}", entry.file, manifest.description)).size(12),
    permissions_view(app, manifest),
  ]
  .spacing(4);
  if consent == ConsentState::Changed {
    details = details.push(
      text(
        app
          .i18n
          .t("Changed since you enabled it. Check its permissions, then enable it again."),
      )
      .size(12),
    );
  }
  let problem = app
    .plugins
    .errors
    .get(&file)
    .or_else(|| app.plugins.consent.disabled.get(&file));
  if let Some(problem) = problem {
    details = details.push(text(format!("{}: {problem}", app.i18n.t("Error"))).size(12));
  }
  details.into()
}

fn entry_view<'a>(app: &'a App, entry: &'a PluginEntry) -> Element<'a, Message> {
  match &entry.kind {
    PluginKind::Native { manifest, consent } => native_row(app, entry, manifest, *consent),
    PluginKind::Invalid(reason) => row![
      text(format!("{} : {reason}", entry.file)).size(12).width(Fill),
      edit_button(app, &entry.file),
      tip(
        icon(
          icons::CLOSE,
          Some(send(PluginMessage::Remove(entry.file.clone()))),
          28,
          28
        ),
        app.i18n.t("Delete")
      ),
    ]
    .align_y(Vertical::Center)
    .into(),
    PluginKind::Incompatible => row![
      text(format!(
        "{} : {}",
        entry.file,
        app.i18n.t(
          "JavaScript plugin from Riff, incompatible. See docs/plugins/PORTAGE.md to port it to Rhai."
        )
      ))
      .size(12)
      .width(Fill),
      tip(
        icon(
          icons::CLOSE,
          Some(send(PluginMessage::Remove(entry.file.clone()))),
          28,
          28
        ),
        app.i18n.t("Delete")
      ),
    ]
    .align_y(Vertical::Center)
    .into(),
  }
}

/// Panneau des plugins.
#[must_use]
pub fn panel(app: &App) -> Element<'_, Message> {
  let actions = row![
    text_button(
      app.i18n.t("New plugin"),
      Some(Message::Editor(EditorMessage::NewPlugin))
    ),
    text_button(
      app.i18n.t("Import plugin"),
      Some(send(PluginMessage::Import))
    ),
    text_button(
      app.i18n.t("Open plugins folder"),
      Some(send(PluginMessage::OpenFolder))
    ),
    text_button(app.i18n.t("Refresh"), Some(send(PluginMessage::Scan))),
  ]
  .spacing(8)
  .wrap();
  let list: Element<'_, Message> = if app.plugins.entries.is_empty() {
    text(app.i18n.t("No plugin found.")).size(13).into()
  } else {
    Column::with_children(
      app
        .plugins
        .entries
        .iter()
        .map(|entry| crate::motion::keyed(&entry.file, || entry_view(app, entry))),
    )
    .spacing(14)
    .width(Fill)
    .into()
  };
  column![
    text(app.i18n.t(
      "Plugins run in a sandbox: no file, network or system access, only the permissions listed below."
    ))
    .size(12),
    actions,
    list
  ]
  .spacing(12)
  .width(Fill)
  .into()
}
