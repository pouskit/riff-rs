//! Vue « Détail d'album » `#view-album` de Riff 44.1.1 (`index.html:770-790`, `app.js:9518-9760`,
//! `styles.css:8495-8700`, `9640-9646`) : fond `#album-hero-fx` (pochette floutée), bouton retour rond, puis la grille
//! `.album-layout` avec la colonne `.album-side` (pochette, titre, artiste cliquable, détails, bouton d'enregistrement ou
//! pastille de note d'un mix) et la liste des titres. La page défile d'un bloc. Sert aussi au mix quotidien. Voir
//! [`tokens::album_detail`].

use super::chrome::{tip, tips};
use super::common::{main_page, track_row};
use super::home::{Roles, g_spinner, icon_pill, loaded};
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_HERO};
use crate::library::{ArtistToken, split_artist_tokens};
use crate::theme::tokens;
use crate::theme::tokens::album_detail as metrics;
use crate::theme::tokens::home as page;
use crate::theme::{fonts, icons};
use crate::ui::{ListId, UiMessage};
use crate::user_data::SavedAlbum;
use crate::widgets::focus::focusable;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::LineHeight;
use iced::widget::{button, column, container, image, mouse_area, responsive, row, space, text};
use iced::{Background, Color, ContentFit, Element, Fill, Padding, border, mouse};

const PAGE_ID: &str = "view-album";
/// Nom que Riff ne rend pas cliquable (`app.js:11607`).
const UNKNOWN_ARTIST: &str = "unknown artist";

/// Ligne de détails `#album-detail-count` : type, année, nombre de titres (`app.js:9640-9645`).
fn meta_line(album: &SavedAlbum, count: usize) -> String {
  let plural = if count == 1 { "" } else { "s" };
  [
    Some(album.kind.clone().unwrap_or_else(|| "Album".to_string())),
    album.year.clone().filter(|year| !year.is_empty()),
    (count > 0).then(|| format!("{count} track{plural}")),
  ]
  .into_iter()
  .flatten()
  .collect::<Vec<_>>()
  .join(" · ")
}

/// Un nom d'artiste ouvre sa page, sauf « Unknown artist » (`app.js:11607`).
fn is_openable(name: &str) -> bool {
  !name.is_empty() && name.to_lowercase() != UNKNOWN_ARTIST
}

/// `#album-detail-artist` : un lien `.artist-link-part` par artiste (`renderClickableArtists`), séparateurs à 75 %
/// d'opacité (`.artist-sep`), le tout en graisse 600 et couleur `primary`. Un mix n'a qu'un texte simple.
fn artist_line<'a>(roles: Roles, raw: &str, mix: bool) -> Element<'a, Message> {
  let size = metrics::ARTIST_TEXT;
  let line = |label: String, color: Color| {
    text(label)
      .size(size)
      .font(fonts::SEMIBOLD)
      .color(color)
      .wrapping(text::Wrapping::None)
  };
  if mix {
    return text(raw.to_string())
      .size(size)
      .font(fonts::SEMIBOLD)
      .color(roles.primary)
      .into();
  }
  let parts = split_artist_tokens(raw)
    .into_iter()
    .map(|token| -> Element<'a, Message> {
      match token {
        ArtistToken::Sep(separator) => line(
          separator,
          Color {
            a: 0.75,
            ..roles.variant
          },
        )
        .into(),
        ArtistToken::Artist(name) => {
          let label = line(name.clone(), roles.primary);
          if is_openable(&name) {
            mouse_area(label)
              .interaction(mouse::Interaction::Pointer)
              .on_press(Message::Ui(UiMessage::OpenArtist(name)))
              .into()
          } else {
            label.into()
          }
        }
      }
    });
  row(parts).wrap().into()
}

/// `#album-detail-cover` : image carrée à coins de 20 px, mosaïque 2 x 2 d'un mix, ou aplat `surface-container`.
fn cover<'a>(app: &App, roles: Roles, side: f32) -> Element<'a, Message> {
  let view = &app.ui.album;
  let radius = border::Radius::from(metrics::COVER_RADIUS);
  let flat = move |content: Option<Element<'a, Message>>| -> Element<'a, Message> {
    let mut frame = container(content.unwrap_or_else(|| space().into()))
      .width(side)
      .height(side)
      .style(move |_| {
        container::Style::default()
          .background(roles.container)
          .border(border::Border::default().rounded(radius))
      });
    frame = frame.clip(true);
    frame.into()
  };
  if view.grid_covers.len() >= 4 {
    let gap = metrics::COVER_GRID_GAP;
    let cell = (side - 3.0 * gap) / 2.0;
    let small = border::Radius::from(metrics::COVER_GRID_RADIUS);
    let at = |index: usize| -> Element<'a, Message> {
      view
        .grid_covers
        .get(index)
        .and_then(|url| loaded(app, url, cell, cell, small))
        .unwrap_or_else(|| space().width(cell).height(cell).into())
    };
    let grid = column![
      row![at(0), at(1)].spacing(gap),
      row![at(2), at(3)].spacing(gap)
    ]
    .spacing(gap);
    return container(grid)
      .padding(gap)
      .width(side)
      .height(side)
      .style(move |_| {
        container::Style::default()
          .background(roles.container)
          .border(border::Border::default().rounded(radius))
      })
      .into();
  }
  let handle = view
    .album
    .cover
    .as_deref()
    .and_then(|url| app.images.get(&ImageKey::new(url, SIZE_HERO)));
  match handle {
    Some(handle) => image(handle.clone())
      .width(side)
      .height(side)
      .content_fit(ContentFit::Cover)
      .border_radius(radius)
      .into(),
    None => flat(None),
  }
}

/// Bouton rond de la pastille de note d'un mix (`.daily-mix-rate-btn`) : `surface-container-high`, `highest` au survol.
fn rate_button<'a>(roles: Roles, glyph: &'a str, message: Message) -> Element<'a, Message> {
  let side = metrics::MIX_BUTTON;
  let widget = button(container(icons::glyph_px(glyph, metrics::MIX_ICON)).center(Fill))
    .padding(0)
    .width(side)
    .height(side)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: Some(Background::Color(if hovered {
          roles.highest
        } else {
          roles.high
        })),
        text_color: if hovered {
          roles.on_surface
        } else {
          roles.variant
        },
        border: border::rounded(side / 2.0),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  focusable(widget)
    .on_activate(Some(message))
    .radius(side / 2.0)
    .into()
}

/// `.daily-mix-feedback` : « Do you like these tracks? » et les deux pouces, tant que le mix n'a pas de note.
fn mix_feedback<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let label = text(app.i18n.t("Do you like these tracks?"))
    .size(metrics::MIX_LABEL_TEXT)
    .font(fonts::MEDIUM)
    .color(roles.variant)
    .wrapping(text::Wrapping::None);
  let buttons = row![
    rate_button(
      roles,
      icons::MIX_LIKE,
      Message::Ui(UiMessage::RateMix(true))
    ),
    rate_button(
      roles,
      icons::MIX_DISLIKE,
      Message::Ui(UiMessage::RateMix(false))
    ),
  ]
  .spacing(metrics::MIX_BUTTONS_GAP);
  let pill = container(
    row![label, buttons]
      .spacing(metrics::MIX_GAP)
      .align_y(Vertical::Center),
  )
  .padding([metrics::MIX_PADDING_Y, metrics::MIX_PADDING_X])
  .style(move |_| {
    container::Style::default()
      .background(roles.container)
      .border(border::rounded(9999))
  });
  column![space().height(metrics::MIX_MARGIN_TOP), pill].into()
}

/// `.album-side` : fond `surface-container` à 72 % sur le fond flou, coins de 24 px.
fn side<'a>(app: &'a App, roles: Roles, window_width: f32) -> Element<'a, Message> {
  let view = &app.ui.album;
  let album = &view.album;
  let inner = metrics::SIDE_WIDTH - 2.0 * metrics::SIDE_PADDING;
  let count = if view.tracks.is_empty() {
    album.track_count.unwrap_or(0.0) as usize
  } else {
    view.tracks.len()
  };
  let title = text(
    album
      .title
      .clone()
      .unwrap_or_else(|| "Untitled album".to_string()),
  )
  .size(metrics::title_size(window_width))
  .font(fonts::BOLD)
  .color(roles.on_surface)
  .line_height(LineHeight::Relative(metrics::TITLE_LINE))
  .width(Fill);
  let meta = text(meta_line(album, count))
    .size(metrics::META_TEXT)
    .color(roles.variant)
    .line_height(LineHeight::Relative(metrics::META_LINE));
  let mut body = column![
    cover(app, roles, inner),
    title,
    artist_line(roles, album.artist.as_deref().unwrap_or(""), view.is_mix()),
    meta,
  ]
  .spacing(metrics::SIDE_GAP)
  .width(Fill);
  if view.is_mix() {
    if album
      .id
      .as_deref()
      .is_some_and(|id| app.data.mix_rating(id).is_none())
    {
      body = body.push(mix_feedback(app, roles));
    }
  } else if app.settings.show_albums {
    // `body.hide-albums-tab #btn-save-album { display: none }` (`styles.css:9741`), `riff_show_albums` à 1 pour l'afficher.
    let glyph = if app.data.is_album_saved(album) {
      icons::FAVORITE_FILLED
    } else {
      icons::FAVORITE
    };
    body = body.push(tip(
      icon_pill(
        roles,
        glyph,
        metrics::SAVE_ICON,
        metrics::SAVE_SIZE,
        Message::Ui(UiMessage::ToggleSaveAlbum),
      ),
      app.i18n.t(tips::save_album(app.data.is_album_saved(album))),
    ));
  }
  container(body)
    .padding(metrics::SIDE_PADDING)
    .width(metrics::SIDE_WIDTH)
    .style(move |_| {
      container::Style::default()
        .background(Color {
          a: metrics::SIDE_SHARE,
          ..roles.container
        })
        .border(border::rounded(metrics::SIDE_RADIUS))
    })
    .into()
}

/// Liste des titres, ou l'indicateur de chargement, ou le message d'erreur (`.empty-hint` en couleur `error`).
fn tracks<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let view = &app.ui.album;
  if view.loading {
    return container(g_spinner(
      metrics::LOADER_SIZE,
      roles,
      app.now.saturating_duration_since(app.epoch),
    ))
    .padding([metrics::LOADER_PADDING_Y, 0.0])
    .width(Fill)
    .align_x(Horizontal::Center)
    .into();
  }
  if let Some(error) = &view.error {
    let scheme = app.theme.scheme.error;
    let color = Color::from_rgb8(scheme.red, scheme.green, scheme.blue);
    return container(
      text(format!("Failed to load album: {error}"))
        .size(metrics::HINT_TEXT)
        .color(color),
    )
    .padding([metrics::HINT_PADDING_Y, 0.0])
    .width(Fill)
    .align_x(Horizontal::Center)
    .into();
  }
  app
    .ui
    .list(ListId::Album)
    .window(app.list_len(ListId::Album), move |index| {
      track_row(app, ListId::Album, index)
    })
}

/// Décalage de la liste dans la page qui défile : remplissage, bouton retour et remplissage de la grille.
#[must_use]
pub fn list_top() -> f32 {
  page::MAIN_TOP + metrics::BACK_SIZE + metrics::BACK_MARGIN + metrics::LAYOUT_PADDING_TOP
}

/// Hauteur approximative du contenu, pour savoir si la barre de défilement prend sa place.
fn content_height(app: &App) -> f32 {
  let list = if app.ui.album.loading || app.ui.album.error.is_some() {
    2.0 * metrics::LOADER_PADDING_Y + metrics::LOADER_SIZE
  } else {
    app.list_len(ListId::Album) as f32 * (metrics::ROW_HEIGHT + tokens::track_row::LIST_GAP)
  };
  page::MAIN_TOP
    + metrics::BACK_SIZE
    + metrics::BACK_MARGIN
    + metrics::LAYOUT_PADDING_TOP
    + list.max(metrics::SIDE_HEIGHT_ESTIMATE)
    + metrics::LAYOUT_PADDING_BOTTOM
    + metrics::PAGE_BOTTOM_EXTRA
    + page::MAIN_BOTTOM
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  let window_width = app.window.size.unwrap_or(crate::app::DEFAULT_WINDOW).width;
  responsive(move |size| {
    let layout = container(
      row![side(app, roles, window_width), tracks(app, roles)]
        .spacing(metrics::LAYOUT_GAP)
        .align_y(Vertical::Top),
    )
    .padding(Padding {
      top: metrics::LAYOUT_PADDING_TOP,
      right: metrics::LAYOUT_PADDING_X,
      bottom: metrics::LAYOUT_PADDING_BOTTOM,
      left: metrics::LAYOUT_PADDING_X,
    });
    let back = tip(
      icon_pill(
        roles,
        icons::ALBUM_BACK,
        metrics::BACK_ICON,
        metrics::BACK_SIZE,
        Message::Back,
      ),
      app.i18n.t(tips::BACK),
    );
    let body = column![
      back,
      space().height(metrics::BACK_MARGIN),
      layout,
      space().height(metrics::PAGE_BOTTOM_EXTRA),
    ]
    .width(Fill);
    main_page(
      roles,
      body.into(),
      PAGE_ID,
      content_height(app) > size.height,
    )
    .on_scroll(|viewport| {
      Message::Ui(UiMessage::AlbumScrolled(
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
  use crate::theme::tokens;

  fn album(kind: Option<&str>, year: Option<&str>) -> SavedAlbum {
    SavedAlbum {
      kind: kind.map(str::to_string),
      year: year.map(str::to_string),
      ..SavedAlbum::default()
    }
  }

  /// 08 : « Album · 2013 · 13 tracks ».
  #[test]
  fn the_meta_line_lists_type_year_and_count() {
    assert_eq!(
      meta_line(&album(None, Some("2013")), 13),
      "Album · 2013 · 13 tracks"
    );
    assert_eq!(
      meta_line(&album(None, Some("2013")), 1),
      "Album · 2013 · 1 track"
    );
  }

  /// `[type, year, count].filter(Boolean)` : une année vide ou aucun titre sont omis.
  #[test]
  fn the_meta_line_skips_what_is_missing() {
    assert_eq!(meta_line(&album(None, Some("")), 0), "Album");
    assert_eq!(
      meta_line(&album(Some("Single"), None), 2),
      "Single · 2 tracks"
    );
  }

  #[test]
  fn an_unknown_artist_is_not_a_link() {
    assert!(is_openable("Daft Punk"));
    assert!(!is_openable("Unknown artist"));
    assert!(!is_openable(""));
  }

  /// `clamp(26px, 2.6vw, 36px)` : 36 px CSS au viewport de la cible (2031 px CSS), 26 px sous 1000 px CSS.
  #[test]
  fn the_title_follows_the_viewport_width_between_26_and_36_css_px() {
    assert!((metrics::title_size(1920.0) - tokens::css(36.0)).abs() < 1e-4);
    assert!((metrics::title_size(800.0) - tokens::css(26.0)).abs() < 1e-4);
    let mid = metrics::title_size(1200.0);
    assert!(mid > tokens::css(26.0) && mid < tokens::css(36.0), "{mid}");
  }

  /// 08, 1920 x 1080 : le panneau latéral couvre 314 à 616 px, la liste commence à 658 px.
  #[test]
  fn the_side_and_the_list_sit_where_the_capture_puts_them() {
    let left = 272.0 + page::MAIN_LEFT + metrics::LAYOUT_PADDING_X;
    assert!((left - 314.0).abs() < 1.5, "{left}");
    let list = left + metrics::SIDE_WIDTH + metrics::LAYOUT_GAP;
    assert!((list - 658.0).abs() < 1.5, "{list}");
  }
}
