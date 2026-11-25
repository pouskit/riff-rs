//! Écran « Playlists » : liste des playlists.

use super::common::{
  Card, card_with_context, empty_hint, playlist_cover_url, title_line, wrap_cards,
};
use crate::app::{App, Message};
use crate::ui::UiMessage;
use crate::widgets::button::filled;
use iced::widget::{column, row, scrollable};
use iced::{Element, Fill};

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let lists = &app.data.playlists;
  let header = row![
    title_line(
      app.i18n.t("Playlists"),
      format!("{} playlists", lists.len())
    ),
    filled(
      app.i18n.t("Create playlist"),
      Some(Message::Ui(UiMessage::NewPlaylist))
    ),
  ];
  let body = if lists.is_empty() {
    empty_hint(app, "No playlists yet")
  } else {
    let cards = lists.iter().filter_map(|playlist| {
      let id = playlist.id.clone()?;
      let count = playlist.tracks.len();
      let menu_id = id.clone();
      Some(card_with_context(
        app,
        Card {
          cover: playlist_cover_url(playlist),
          title: playlist
            .name
            .clone()
            .unwrap_or_else(|| "untitled".to_string()),
          sub: format!("{count} track{}", if count == 1 { "" } else { "s" }),
          round: false,
          message: Message::Ui(UiMessage::OpenPlaylist(id)),
        },
        move |position| Message::Ui(UiMessage::PlaylistMenu(menu_id.clone(), position)),
      ))
    });
    scrollable(wrap_cards(cards.collect())).height(Fill).into()
  };
  column![header, body]
    .spacing(12)
    .padding(24)
    .width(Fill)
    .height(Fill)
    .into()
}
