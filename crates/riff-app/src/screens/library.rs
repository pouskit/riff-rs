//! Vue « Mes fichiers » `#view-local` de Riff 44.1.1 (`index.html:668-690`, `app.js:12653-12814`,
//! `styles.css:1483-1493`, `1543-1570`, `3303-3340`) : en-tête réduit aux deux boutons d'ajout (titre et
//! sous-titre masqués par `styles.css:1483`), champ de recherche, rangée d'albums, puis la liste des titres ou
//! le message vide. La page défile d'un bloc. Voir [`tokens::local`].

use super::common::{main_page, track_row};
use super::home::{Roles, primary_pill_style};
use super::search::{AlbumCard, album_card, albums_strip};
use super::window_frame::overlay;
use crate::app::{App, Message};
use crate::theme::tokens::home as page;
use crate::theme::tokens::local as metrics;
use crate::theme::tokens::search_results as card;
use crate::theme::tokens::track_row::PITCH;
use crate::theme::{fonts, icons, tokens};
use crate::ui::{CardGrid, ListId, UiMessage};
use crate::user_data::Track;
use crate::widgets::focus::focusable;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::LineHeight;
use iced::widget::text_input::{self, Status as InputStatus};
use iced::widget::{
  Column, button, column, container, responsive, row, space, stack, text, text_input as input,
};
use iced::{Background, Border, Color, Element, Fill, Padding, Theme};
use std::collections::HashMap;

const SEARCH_ID: &str = "local-search";
const PAGE_ID: &str = "list-local";

/// Albums des fichiers locaux, dans l'ordre d'apparition (`Map` de `renderLocalFiles`, `app.js:12746`) :
/// nom, artiste du premier titre, première pochette trouvée, nombre de titres.
#[must_use]
pub fn local_albums(files: &[Track]) -> Vec<(String, String, Option<String>, usize)> {
  let mut albums: Vec<(String, String, Option<String>, usize)> = Vec::new();
  let mut index: HashMap<String, usize> = HashMap::new();
  for track in files {
    let name = track.album.as_deref().unwrap_or("").trim();
    if name.is_empty() {
      continue;
    }
    let at = *index.entry(name.to_string()).or_insert_with(|| {
      albums.push((
        name.to_string(),
        track.artist.clone().unwrap_or_default(),
        None,
        0,
      ));
      albums.len() - 1
    });
    let entry = &mut albums[at];
    entry.2 = entry.2.take().or_else(|| track.thumb_url());
    entry.3 += 1;
  }
  albums
}

/// Au moins un titre a un album : la rangée d'albums est affichée.
#[must_use]
pub fn has_albums(files: &[Track]) -> bool {
  files
    .iter()
    .any(|track| !track.album.as_deref().unwrap_or("").trim().is_empty())
}

/// Décalage de la liste des titres dans la page qui défile : remplissage, en-tête, champ de recherche et,
/// s'il y en a, rangée d'albums.
#[must_use]
pub fn list_top(has_albums: bool) -> f32 {
  let mut top = page::MAIN_TOP
    + metrics::BUTTON_HEIGHT
    + metrics::HEADER_MARGIN
    + metrics::SEARCH_HEIGHT
    + metrics::SEARCH_MARGIN;
  if has_albums {
    top += metrics::ALBUMS_HEIGHT + metrics::ALBUMS_MARGIN;
  }
  top
}

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

/// `.btn-outline-pill` : `surface-container-high`, texte `on-surface-variant` ; au survol `highest` à 8 %,
/// rayon de 18 px (`styles.css:3323-3340`, `7521`).
pub(super) fn outline_style(roles: Roles) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |_, status| {
    let (fill, label, radius) = match status {
      button::Status::Hovered => (
        overlay(roles.highest, roles.on_surface, 0.08),
        roles.on_surface,
        tokens::css(18.0),
      ),
      button::Status::Pressed => (
        overlay(roles.highest, roles.on_surface, 0.12),
        roles.on_surface,
        tokens::css(12.0),
      ),
      _ => (roles.high, roles.variant, tokens::radius::FULL),
    };
    button::Style {
      background: Some(Background::Color(fill)),
      text_color: label,
      border: Border {
        radius: radius.into(),
        ..Border::default()
      },
      ..button::Style::default()
    }
  }
}

/// `.btn-pill-primary` ou `.btn-outline-pill` : icône de 16 px, libellé en graisse 500.
/// Sans `riff_highlight_buttons`, le bouton primaire prend le style de l'autre (`styles.css:7564`).
fn pill<'a>(
  app: &App,
  roles: Roles,
  label: String,
  glyph: &'a str,
  primary: bool,
  message: Message,
) -> Element<'a, Message> {
  let primary = primary && app.settings.highlight_buttons;
  let key_label = label.clone();
  let content = row![
    icons::glyph_px(glyph, metrics::BUTTON_ICON),
    text(label).size(metrics::BUTTON_TEXT).font(fonts::MEDIUM),
  ]
  .spacing(metrics::BUTTON_GAP)
  .align_y(Vertical::Center);
  let side = if primary {
    metrics::PRIMARY_PADDING_X
  } else {
    metrics::OUTLINE_PADDING_X
  };
  let widget = button(container(content).height(Fill).align_y(Vertical::Center))
    .padding(Padding::ZERO.left(side).right(side))
    .height(metrics::BUTTON_HEIGHT)
    .on_press(message.clone());
  let widget: Element<'a, Message> = if primary {
    focusable(widget.style(primary_pill_style(roles)))
      .ripple(&key_label, roles.on_primary, tokens::css(18.0))
      .on_activate(Some(message))
      .radius(tokens::css(18.0))
      .into()
  } else {
    focusable(widget.style(outline_style(roles)))
      .ripple(&key_label, roles.variant, tokens::css(18.0))
      .on_activate(Some(message))
      .radius(tokens::css(18.0))
      .into()
  };
  widget
}

fn add_files<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  pill(
    app,
    roles,
    app.i18n.t("Add files"),
    icons::ADD,
    true,
    ui(UiMessage::PickFiles),
  )
}

fn add_folder<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  pill(
    app,
    roles,
    app.i18n.t("Add folder"),
    icons::LOCAL_ADD_FOLDER,
    false,
    ui(UiMessage::PickFolder),
  )
}

/// `.view-search-box` : la pastille est le champ lui-même, `surface-container`, puis `highest` au focus.
fn search_style(roles: Roles) -> impl Fn(&Theme, InputStatus) -> text_input::Style {
  move |theme, status| text_input::Style {
    background: Background::Color(if matches!(status, InputStatus::Focused { .. }) {
      roles.highest
    } else {
      roles.container
    }),
    border: Border {
      radius: tokens::radius::FULL.into(),
      width: 0.0,
      color: Color::TRANSPARENT,
    },
    icon: roles.on_surface,
    placeholder: roles.outline,
    value: roles.on_surface,
    selection: theme.extended_palette().primary.weak.color,
  }
}

/// `.view-search-box` (`#local-search-box`, `#favorites-search-box`) : loupe de 16 px en `outline`, champ de 13 px.
pub(super) fn search_box<'a>(
  roles: Roles,
  id: &'static str,
  placeholder: &str,
  value: &str,
  on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
  let left = metrics::SEARCH_PADDING_X + tokens::css(metrics::SEARCH_ICON) + metrics::SEARCH_GAP;
  let field = input(placeholder, value)
    .id(id)
    .on_input(on_input)
    .size(metrics::SEARCH_TEXT)
    .line_height(LineHeight::Absolute(metrics::SEARCH_HEIGHT.into()))
    .padding(Padding {
      top: 0.0,
      right: metrics::SEARCH_PADDING_X,
      bottom: 0.0,
      left,
    })
    .style(search_style(roles));
  let glyph = container(
    container(icons::glyph_px(icons::SEARCH, metrics::SEARCH_ICON))
      .style(move |_| container::Style::default().color(roles.outline)),
  )
  .width(Fill)
  .height(Fill)
  .align_y(Vertical::Center)
  .padding(Padding::ZERO.left(metrics::SEARCH_PADDING_X));
  stack![field, glyph]
    .width(Fill)
    .height(metrics::SEARCH_HEIGHT)
    .into()
}

/// `.search-album-card` des albums locaux : sous-titre « artiste · N tracks » (`app.js:12776`).
fn albums_row<'a>(app: &'a App, roles: Roles) -> Option<Element<'a, Message>> {
  let albums = local_albums(&app.data.local_files);
  if albums.is_empty() {
    return None;
  }
  let hovered = match app.ui.card_hover {
    Some((CardGrid::Local, index)) => Some(index),
    _ => None,
  };
  let count = albums.len();
  Some(albums_strip(
    app,
    roles,
    count,
    app.ui.local_albums_scroll,
    |offset| ui(UiMessage::LocalAlbumsScrolled(offset)),
    move || {
      albums
        .iter()
        .enumerate()
        .map(|(index, (name, artist, cover, tracks))| {
          let plural = if *tracks == 1 { "" } else { "s" };
          let sub = if artist.is_empty() {
            format!("{tracks} track{plural}")
          } else {
            format!("{artist} \u{b7} {tracks} track{plural}")
          };
          album_card(
            app,
            roles,
            AlbumCard {
              cover: cover.as_deref(),
              title: name.clone(),
              sub,
              side: card::CARD_WIDTH,
              hovered: hovered == Some(index),
              inset: (card::ROW_PADDING_TOP, card::ROW_PADDING_BOTTOM),
              open: ui(UiMessage::OpenLocalAlbum(name.clone())),
              enter: ui(UiMessage::CardHover(CardGrid::Local, index, true)),
              exit: ui(UiMessage::CardHover(CardGrid::Local, index, false)),
            },
          )
        })
        .collect()
    },
  ))
}

/// `.empty-hint` du texte seul.
pub(super) fn hint<'a>(app: &App, roles: Roles, key: &str) -> Element<'a, Message> {
  container(
    text(app.i18n.t(key))
      .size(card::HINT_TEXT)
      .color(roles.outline),
  )
  .padding([card::HINT_PADDING_Y, 0.0])
  .width(Fill)
  .align_x(Horizontal::Center)
  .into()
}

/// Bibliothèque vide : message, puis les deux boutons d'ajout (`app.js:12796-12805`).
fn empty<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let buttons = row![add_files(app, roles), add_folder(app, roles)]
    .spacing(metrics::EMPTY_BUTTONS_GAP)
    .align_y(Vertical::Center);
  container(
    column![
      text(app.i18n.t("No local tracks in library yet."))
        .size(card::HINT_TEXT)
        .color(roles.outline),
      buttons
    ]
    .spacing(metrics::EMPTY_GAP)
    .align_x(Horizontal::Center),
  )
  .padding([card::HINT_PADDING_Y, 0.0])
  .width(Fill)
  .align_x(Horizontal::Center)
  .into()
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  responsive(move |size| {
    let total = app.data.local_files.len();
    let shown = app.list_len(ListId::Local);
    let albums = albums_row(app, roles);
    let mut page = Column::new().width(Fill).push(
      row![
        space().width(Fill),
        add_files(app, roles),
        add_folder(app, roles)
      ]
      .spacing(metrics::HEADER_GAP)
      .align_y(Vertical::Center),
    );
    page = page
      .push(space().height(metrics::HEADER_MARGIN))
      .push(search_box(
        roles,
        SEARCH_ID,
        &app.i18n.t("Search my files..."),
        &app.ui.local_query,
        |query| ui(UiMessage::LocalQuery(query)),
      ))
      .push(space().height(metrics::SEARCH_MARGIN));
    let has_albums = albums.is_some();
    if let Some(albums) = albums {
      page = page
        .push(albums)
        .push(space().height(metrics::ALBUMS_MARGIN));
    }
    let list: Element<'_, Message> = if total == 0 {
      empty(app, roles)
    } else if shown == 0 {
      hint(
        app,
        roles,
        "Your library is empty. Add some music to get started",
      )
    } else {
      app
        .ui
        .list(ListId::Local)
        .window(shown, move |index| track_row(app, ListId::Local, index))
    };
    page = page.push(list);
    let content_height = list_top(has_albums) + shown as f32 * PITCH + page::MAIN_BOTTOM;
    main_page(roles, page.into(), PAGE_ID, content_height > size.height)
      .on_scroll(|viewport| {
        ui(UiMessage::LocalScrolled(
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

  fn track(album: &str, artist: &str, cover: Option<&str>) -> Track {
    Track {
      album: Some(album.into()),
      artist: Some(artist.into()),
      thumbnail: cover.map(str::to_string),
      ..Track::default()
    }
  }

  #[test]
  fn tracks_are_grouped_by_album_with_the_first_cover() {
    let albums = local_albums(&[
      track("X", "A", None),
      track("X", "A", Some("c")),
      track(" ", "A", None),
      track("Y", "A", None),
    ]);
    assert_eq!(albums.len(), 2);
    assert_eq!(
      albums[0],
      ("X".to_string(), "A".to_string(), Some("c".to_string()), 2)
    );
  }

  /// `Map` de Riff : l'ordre est celui de la première apparition, pas l'ordre alphabétique.
  #[test]
  fn albums_keep_the_order_of_first_appearance() {
    let albums = local_albums(&[
      track("Zebra", "A", None),
      track("Alpha", "A", None),
      track("Zebra", "B", None),
    ]);
    let names: Vec<_> = albums.iter().map(|album| album.0.as_str()).collect();
    assert_eq!(names, ["Zebra", "Alpha"]);
    assert_eq!(albums[0].1, "A", "artiste du premier titre");
    assert_eq!(albums[0].3, 2);
  }

  #[test]
  fn the_album_row_exists_only_when_a_track_has_an_album() {
    assert!(!has_albums(&[]));
    assert!(!has_albums(&[Track::default(), track(" ", "A", None)]));
    assert!(has_albums(&[Track::default(), track("X", "A", None)]));
  }

  /// 04e, 1920 x 1080 : le champ de recherche se termine à 205 px, la page commence à 97 px.
  #[test]
  fn the_list_starts_under_the_search_box_and_the_album_row() {
    let bare = list_top(false);
    assert!((bare + 97.0 - 220.0).abs() < 2.0, "{bare}");
    let with = list_top(true);
    assert!((with - bare - (metrics::ALBUMS_HEIGHT + metrics::ALBUMS_MARGIN)).abs() < 0.01);
  }
}
