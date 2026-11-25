//! Vue « Albums » `#view-albums` de Riff 44.1.1 (`index.html:666`, `app.js:9481-9526`) : sans en-tête, la
//! grille `#saved-albums-grid` des albums enregistrés, ou son message vide. Les dimensions sont en pixels
//! CSS convertis par [`tokens::css`] ; voir [`tokens::albums`].

use super::common::main_page;
use super::home::Roles;
use super::search::{AlbumCard, album_card};
use crate::app::{App, Message};
use crate::theme::tokens::albums as metrics;
use crate::theme::tokens::home as page;
use crate::theme::tokens::search_results as card;
use crate::ui::{CardGrid, UiMessage};
use iced::alignment::Horizontal;
use iced::widget::{Column, Row, container, responsive, space, text};
use iced::{Element, Fill, Padding, Size};

const PAGE_ID: &str = "page-albums";

/// Colonnes de `repeat(auto-fill, minmax(180px, 1fr))` pour une largeur utile `width`.
#[must_use]
pub fn grid_columns(width: f32) -> (usize, f32) {
  let columns = (((width + metrics::GRID_GAP) / (metrics::GRID_MIN + metrics::GRID_GAP)).floor()
    as usize)
    .max(1);
  let gaps = (columns - 1) as f32 * metrics::GRID_GAP;
  (columns, (width - gaps) / columns as f32)
}

/// Hauteur d'une carte de la grille de côté `side` : pochette, écart, titre, écart, sous-titre.
fn card_height(side: f32) -> f32 {
  side
    + card::CARD_GAP
    + card::TITLE_TEXT * page::H2_LINE
    + card::INFO_GAP
    + card::SUB_TEXT * page::H2_LINE
}

/// Disposition de la grille dans une page de `size` : colonnes, côté d'une carte, barre de défilement.
#[must_use]
pub fn layout(size: Size, count: usize) -> (usize, f32, bool) {
  let width = |overflow: bool| {
    size.width
      - page::MAIN_LEFT
      - page::MAIN_RIGHT
      - if overflow { page::SCROLLBAR } else { 0.0 }
      - 2.0 * metrics::GRID_PADDING_SIDE
  };
  let overflows = |columns: usize, side: f32| {
    let rows = count.div_ceil(columns) as f32;
    count > 0
      && page::MAIN_TOP
        + metrics::GRID_PADDING_TOP
        + rows * card_height(side)
        + (rows - 1.0) * metrics::GRID_GAP
        + metrics::GRID_PADDING_BOTTOM
        + page::MAIN_BOTTOM
        > size.height
  };
  let (columns, side) = grid_columns(width(false));
  if overflows(columns, side) {
    let (columns, side) = grid_columns(width(true));
    (columns, side, true)
  } else {
    (columns, side, false)
  }
}

fn hint<'a>(app: &App, roles: Roles, side: f32) -> Element<'a, Message> {
  container(
    text(
      app
        .i18n
        .t("No saved albums yet. Save albums from the album view."),
    )
    .size(metrics::HINT_TEXT)
    .color(roles.outline)
    .align_x(Horizontal::Center)
    .width(Fill),
  )
  .padding([metrics::HINT_PADDING_Y, 0.0])
  .width(side)
  .into()
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  responsive(move |size| {
    let albums = &app.data.saved_albums;
    let (columns, side, overflow) = layout(size, albums.len());
    let body: Element<'_, Message> = if albums.is_empty() {
      hint(app, roles, side)
    } else {
      let hovered = match app.ui.card_hover {
        Some((CardGrid::Saved, index)) => Some(index),
        _ => None,
      };
      let cards: Vec<Element<'_, Message>> = albums
        .iter()
        .enumerate()
        .map(|(index, album)| {
          let sub: Vec<&str> = [
            album.kind.as_deref().unwrap_or("Album"),
            album.artist.as_deref().unwrap_or(""),
          ]
          .into_iter()
          .filter(|part| !part.is_empty())
          .collect();
          album_card(
            app,
            roles,
            AlbumCard {
              cover: album.cover.as_deref(),
              title: album.title.clone().unwrap_or_default(),
              sub: sub.join(" \u{b7} "),
              side,
              hovered: hovered == Some(index),
              inset: (card::CARD_LIFT, 0.0),
              open: Message::Ui(UiMessage::OpenSavedAlbum(index)),
              enter: Message::Ui(UiMessage::CardHover(CardGrid::Saved, index, true)),
              exit: Message::Ui(UiMessage::CardHover(CardGrid::Saved, index, false)),
            },
          )
        })
        .collect();
      let mut rows = Column::new().spacing(metrics::GRID_GAP - card::CARD_LIFT);
      let mut cards = cards.into_iter();
      loop {
        let row: Vec<_> = cards.by_ref().take(columns).collect();
        if row.is_empty() {
          break;
        }
        rows = rows.push(Row::with_children(row).spacing(metrics::GRID_GAP));
      }
      rows.into()
    };
    let grid = Column::new()
      .push(body)
      .push(space().height(metrics::GRID_PADDING_BOTTOM))
      .padding(Padding {
        top: metrics::GRID_PADDING_TOP - card::CARD_LIFT,
        right: metrics::GRID_PADDING_SIDE,
        bottom: 0.0,
        left: metrics::GRID_PADDING_SIDE,
      })
      .width(Fill);
    main_page(roles, grid.into(), PAGE_ID, overflow).into()
  })
  .width(Fill)
  .height(Fill)
  .into()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::theme::tokens;

  /// 04d, 1920 x 1080 : contenu de 291 à 1572 px écran, 6 colonnes de 206,3 px CSS.
  #[test]
  fn the_captured_window_has_six_columns_of_about_206_css_px() {
    let panel = (1572.0 - 291.0) - 2.0 * metrics::GRID_PADDING_SIDE;
    let (columns, side) = grid_columns(panel);
    assert_eq!(columns, 6);
    assert!((side / tokens::ZOOM - 206.3).abs() < 1.0, "{side}");
  }

  #[test]
  fn a_narrow_panel_keeps_one_column() {
    assert_eq!(grid_columns(10.0).0, 1);
  }

  #[test]
  fn the_columns_fill_the_width_between_the_gaps() {
    let (columns, side) = grid_columns(900.0);
    let used = columns as f32 * side + (columns - 1) as f32 * metrics::GRID_GAP;
    assert!((used - 900.0).abs() < 0.01);
    assert!(side >= metrics::GRID_MIN);
  }

  #[test]
  fn the_scrollbar_only_appears_when_the_grid_is_taller_than_the_page() {
    let size = Size::new(1318.0, 900.0);
    assert!(!layout(size, 0).2);
    assert!(!layout(size, 6).2);
    assert!(layout(size, 60).2);
  }
}
