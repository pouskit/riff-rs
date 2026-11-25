//! Vue « Historique » `#view-history` de Riff 44.1.1 (`index.html:696-704`, `app.js:12462-12465`,
//! `styles.css:1483-1493`, `3323-3340`) : le titre et le sous-titre sont masqués (`styles.css:1483-1488`),
//! l'en-tête ne garde que le bouton « Clear history » aligné à droite, puis la liste des titres écoutés.
//! La page défile d'un bloc. Voir [`tokens::history_view`].

use super::common::{main_page, track_row};
use super::home::Roles;
use super::library::{hint, outline_style};
use crate::app::{App, Message};
use crate::theme::tokens::history_view as metrics;
use crate::theme::tokens::home as page;
use crate::theme::tokens::local as button_metrics;
use crate::theme::tokens::track_row::PITCH;
use crate::theme::{fonts, tokens};
use crate::ui::{ListId, UiMessage};
use crate::widgets::focus::focusable;
use iced::alignment::Vertical;
use iced::widget::{Column, button, container, responsive, row, space, text};
use iced::{Element, Fill, Padding};

const PAGE_ID: &str = "list-history";

/// Décalage de la liste dans la page qui défile : remplissage, en-tête et marge sous l'en-tête.
#[must_use]
pub fn list_top() -> f32 {
  page::MAIN_TOP + metrics::HEADER_HEIGHT + metrics::HEADER_MARGIN
}

/// `#btn-clear-history` : `.btn-outline-pill` sans icône.
fn clear_button<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let message = Message::Ui(UiMessage::ClearHistory);
  let widget = button(
    container(
      text(app.i18n.t("Clear history"))
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
  .style(outline_style(roles));
  focusable(widget)
    .on_activate(Some(message))
    .radius(tokens::css(18.0))
    .into()
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  responsive(move |size| {
    let shown = app.data.history.len();
    let header = row![space().width(Fill), clear_button(app, roles)].align_y(Vertical::Center);
    let mut page = Column::new()
      .width(Fill)
      .push(header)
      .push(space().height(metrics::HEADER_MARGIN));
    // `renderTracks` : le contexte `history` n'a pas de message propre, `app.js:11436-11444`.
    page = page.push(if shown == 0 {
      hint(app, roles, "No tracks to show")
    } else {
      app
        .ui
        .list(ListId::History)
        .window(shown, move |index| track_row(app, ListId::History, index))
    });
    let content_height = list_top() + shown as f32 * PITCH + page::MAIN_BOTTOM;
    main_page(roles, page.into(), PAGE_ID, content_height > size.height)
      .on_scroll(|viewport| {
        Message::Ui(UiMessage::HistoryScrolled(
          viewport.absolute_offset().y,
          viewport.bounds().height,
        ))
      })
      .into()
  })
  .width(Fill)
  .height(Fill)
  .into()
}

#[cfg(test)]
mod tests {
  use super::*;

  /// 10, 1920 x 1080 : le bouton couvre 119 à 156 px, la première ligne (centre à 202 px) commence vers 171 px, la page à 97 px.
  #[test]
  fn the_list_starts_under_the_clear_button() {
    assert!((list_top() + 97.0 - 171.0).abs() < 2.0, "{}", list_top());
  }
}
