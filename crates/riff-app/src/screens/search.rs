//! Résultats de recherche `#search-results-container` de Riff 44.1.1 (`index.html:621-631`,
//! `app.js:9168-9232`, `9838-9910`, `10316-10400`, `26915-26943`) : pastilles de filtre, carte d'artiste,
//! rangée d'albums et liste des titres, qui défilent d'un bloc avec la page. Les dimensions sont en
//! pixels CSS convertis par [`tokens::css`] ; voir [`tokens::search_results`].

use super::common::{fade_veil, track_row};
use super::home::{
  Roles, fade_strip, flat, loaded, page_scroll_style, primary_pill_style, text_fade,
};
use super::right_panel::ellipsize;
use super::search_history::verified_check;
use crate::app::{App, Message};
use crate::motion::ViewPose;
use crate::theme::tokens::home as home_metrics;
use crate::theme::tokens::search_results as metrics;
use crate::theme::{fonts, icons, tokens};
use crate::ui::{ListId, SearchFilter, SearchState, UiMessage};
use crate::update::search_filter_shows;
use crate::user_data::SavedAlbum;
use crate::visuals::spinner;
use crate::widgets::focus::focusable;
use crate::widgets::view_transition::view_transition;
use crate::widgets::wheel::horizontal_wheel;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
  Column, Row, button, column, container, mouse_area, responsive, row, scrollable, space, stack,
  text,
};
use iced::{Background, Color, Element, Fill, Font, Padding, Size, border};

const FILTERS: [(&str, SearchFilter); 4] = [
  ("All", SearchFilter::All),
  ("Songs", SearchFilter::Songs),
  ("Artists", SearchFilter::Artists),
  ("Albums", SearchFilter::Albums),
];

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

fn line<'a>(label: String, size: f32, font: Font, color: Color) -> Element<'a, Message> {
  text(label)
    .size(size)
    .font(font)
    .color(color)
    .line_height(LineHeight::Relative(tokens::home::H2_LINE))
    .wrapping(Wrapping::None)
    .into()
}

/// Titre d'une section ou d'une rangée à partir de laquelle la liste des titres commence : décalage de
/// la liste dans la page qui défile (remplissage de `.main-content`, pastilles, carte, albums).
#[must_use]
pub fn list_top(search: &SearchState) -> f32 {
  let mut top = home_metrics::MAIN_TOP + metrics::CHIP_HEIGHT + metrics::CHIPS_MARGIN;
  if search_filter_shows(search.filter, SearchFilter::Artists) && search.artist.is_some() {
    top += metrics::ARTIST_HEIGHT;
  }
  if search_filter_shows(search.filter, SearchFilter::Albums) && !search.albums.is_empty() {
    top += metrics::ALBUMS_HEIGHT;
  }
  top
}

/// `.search-chip` : fond `surface-container`, texte `on-surface-variant` ; survol `surface-container-high` ;
/// actif `primary-container`, graisse 600 (`styles.css:10559-10588`).
fn chip<'a>(app: &App, roles: Roles, label: &str, filter: SearchFilter) -> Element<'a, Message> {
  let active = app.ui.search.filter == filter;
  let message = ui(UiMessage::SetSearchFilter(filter));
  let font = if active {
    fonts::SEMIBOLD
  } else {
    fonts::MEDIUM
  };
  let content = container(
    text(app.i18n.t(label))
      .size(metrics::CHIP_TEXT)
      .font(font)
      .line_height(LineHeight::Relative(tokens::home::H2_LINE)),
  )
  .height(metrics::CHIP_HEIGHT)
  .align_y(Vertical::Center);
  let entry = button(content)
    .padding(
      Padding::ZERO
        .left(metrics::CHIP_PADDING_X)
        .right(metrics::CHIP_PADDING_X),
    )
    .height(metrics::CHIP_HEIGHT)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      let (fill, label) = if active {
        (roles.primary_container, roles.on_primary_container)
      } else if hovered {
        (roles.high, roles.on_surface)
      } else {
        (roles.container, roles.variant)
      };
      button::Style {
        background: Some(Background::Color(fill)),
        text_color: label,
        border: border::rounded(metrics::CHIP_RADIUS),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  focusable(entry)
    .on_activate(Some(message))
    .radius(metrics::CHIP_RADIUS)
    .into()
}

fn chips<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let row = Row::with_children(
    FILTERS
      .iter()
      .map(|(label, filter)| chip(app, roles, label, *filter)),
  )
  .spacing(metrics::CHIPS_GAP);
  container(row)
    .padding(Padding::ZERO.bottom(metrics::CHIPS_MARGIN))
    .into()
}

/// `.search-artist-card` : avatar rond, nom en gras avec la pastille vérifié, « Artist • N listeners ».
fn artist_card<'a>(app: &App, roles: Roles) -> Option<Element<'a, Message>> {
  let artist = app.ui.search.artist.as_ref()?;
  let side = metrics::AVATAR;
  let radius = border::Radius::from(side / 2.0);
  let avatar = artist
    .avatar
    .as_deref()
    .and_then(|url| loaded(app, url, side, side, radius))
    .unwrap_or_else(|| flat(side, side, roles.container, radius, None));
  let label = if artist.fans > 0.0 {
    format!(
      "Artist \u{2022} {} listeners",
      riff_core::discovery::format_thousands(artist.fans as u64)
    )
  } else {
    "Artist".to_string()
  };
  let mut name = Row::new().align_y(Vertical::Center).push(
    text(artist.name.clone())
      .size(metrics::ARTIST_NAME_TEXT)
      .font(fonts::BOLD)
      .color(roles.on_surface)
      .line_height(LineHeight::Relative(metrics::ARTIST_NAME_LINE))
      .wrapping(Wrapping::None),
  );
  if artist.verified {
    name = name
      .push(space().width(metrics::VERIFIED_MARGIN))
      .push(verified_check(roles));
  }
  let info = column![
    name,
    line(
      label,
      metrics::ARTIST_LABEL_TEXT,
      fonts::MEDIUM,
      roles.variant
    )
  ]
  .spacing(metrics::ARTIST_INFO_GAP);
  let message = ui(UiMessage::OpenSearchArtist);
  let body = row![avatar, info]
    .spacing(metrics::ARTIST_GAP)
    .align_y(Vertical::Center);
  let entry = button(body)
    .padding(Padding {
      top: metrics::ARTIST_PADDING_Y,
      right: metrics::ARTIST_PADDING_X,
      bottom: metrics::ARTIST_PADDING_Y,
      left: metrics::ARTIST_PADDING_X,
    })
    .width(Fill)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: Some(Background::Color(if hovered {
          roles.high
        } else {
          roles.container
        })),
        text_color: roles.on_surface,
        border: border::rounded(metrics::ARTIST_RADIUS),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  let card = focusable(entry)
    .on_activate(Some(message))
    .radius(metrics::ARTIST_RADIUS);
  // `m3FadeUp 260ms both` : translation de 10 px et opacité, sans coupe en CSS (la coupe s'étend de la translation).
  let card: Element<'a, Message> = view_transition(
    card,
    app.entry_motion.artist_pose(app.now),
    fade_veil(app, roles.page),
  )
  .pop(0.0)
  .spill(tokens::fade_up::OFFSET)
  .into();
  Some(
    container(card)
      .padding(Padding::ZERO.bottom(metrics::ARTIST_MARGIN))
      .into(),
  )
}

/// Contenu d'une `.search-album-card`.
pub(super) struct AlbumCard<'a> {
  pub cover: Option<&'a str>,
  pub title: String,
  pub sub: String,
  /// Largeur de la carte et côté de la pochette.
  pub side: f32,
  pub hovered: bool,
  /// Marges haute et basse de la carte, qui laissent la place de la levée au survol.
  pub inset: (f32, f32),
  pub open: Message,
  /// Messages d'entrée et de sortie du pointeur.
  pub enter: Message,
  pub exit: Message,
}

/// `.search-album-card` : pochette à coins de 18, titre (600) et sous-titre rognés par « … ».
pub(super) fn album_card<'a>(app: &App, roles: Roles, card: AlbumCard<'_>) -> Element<'a, Message> {
  let AlbumCard {
    cover,
    title,
    sub,
    side,
    hovered,
    inset,
    open,
    enter,
    exit,
  } = card;
  let radius = border::Radius::from(metrics::COVER_RADIUS);
  let cover = cover
    .and_then(|url| loaded(app, url, side, side, radius))
    .unwrap_or_else(|| {
      let url_missing = cover.is_none_or(str::is_empty);
      let icon = url_missing.then(|| {
        container(icons::glyph_px(icons::ROW_FALLBACK, metrics::COVER_ICON))
          .style(move |_| container::Style::default().color(roles.outline))
          .into()
      });
      flat(side, side, roles.high, radius, icon)
    });
  let title = ellipsize(&title, metrics::TITLE_TEXT, fonts::SEMIBOLD, side);
  let sub = ellipsize(&sub, metrics::SUB_TEXT, fonts::UI, side);
  // `.search-album-card:hover` : `translateY(-2px)`. La hauteur de la carte ne change pas.
  let lift = if hovered { metrics::CARD_LIFT } else { 0.0 };
  let body = column![
    space().height(inset.0 - lift),
    cover,
    space().height(metrics::CARD_GAP),
    line(
      title,
      metrics::TITLE_TEXT,
      fonts::SEMIBOLD,
      roles.on_surface
    ),
    space().height(metrics::INFO_GAP),
    line(sub, metrics::SUB_TEXT, fonts::UI, roles.variant),
    space().height(inset.1 + lift),
  ]
  .width(side);
  let entry = button(body)
    .padding(0)
    .width(side)
    .style(|_, _| button::Style::default())
    .on_press(open.clone());
  mouse_area(
    focusable(entry)
      .on_activate(Some(open))
      .radius(metrics::COVER_RADIUS),
  )
  .on_enter(enter)
  .on_exit(exit)
  .into()
}

/// Carte d'un album des résultats : sous-titre « artiste · année ».
fn result_card<'a>(
  app: &App,
  roles: Roles,
  album: &SavedAlbum,
  index: usize,
) -> Element<'a, Message> {
  let artist = album.artist.clone().unwrap_or_default();
  let sub = match album.year.as_deref().filter(|year| !year.is_empty()) {
    Some(year) => format!("{artist} · {year}"),
    None => artist,
  };
  album_card(
    app,
    roles,
    AlbumCard {
      cover: album.cover.as_deref(),
      title: album.title.clone().unwrap_or_default(),
      sub,
      side: metrics::CARD_WIDTH,
      hovered: app.ui.search.album_hover == Some(index),
      inset: (metrics::ROW_PADDING_TOP, metrics::ROW_PADDING_BOTTOM),
      open: ui(UiMessage::OpenSearchAlbum(index)),
      enter: ui(UiMessage::SearchAlbumHover(index, true)),
      exit: ui(UiMessage::SearchAlbumHover(index, false)),
    },
  )
}

/// `.search-albums-row` : défilement horizontal sans barre, fondu de bord au-delà de 4 px (`updateRowFade`).
pub(super) fn albums_strip<'a>(
  app: &App,
  roles: Roles,
  count: usize,
  offset: f32,
  on_scroll: fn(f32) -> Message,
  cards: impl Fn() -> Vec<Element<'a, Message>> + 'a,
) -> Element<'a, Message> {
  let height = metrics::ROW_PADDING_TOP + metrics::CARD_HEIGHT + metrics::ROW_PADDING_BOTTOM;
  let fade = text_fade(app, roles);
  responsive(move |size: Size| {
    let cards = Row::with_children(cards()).spacing(metrics::ROW_GAP);
    let scroller = scrollable(cards)
      .direction(Direction::Horizontal(
        Scrollbar::new().width(0).scroller_width(0).margin(0),
      ))
      .style(move |_, status| page_scroll_style(roles, status))
      .on_scroll(move |viewport| on_scroll(viewport.absolute_offset().x))
      .width(Fill)
      .height(height);
    let scroller: Element<'a, Message> = horizontal_wheel(scroller).into();
    let Some(page) = fade else {
      return scroller;
    };
    let content =
      count as f32 * metrics::CARD_WIDTH + count.saturating_sub(1) as f32 * metrics::ROW_GAP;
    let max = (content - size.width).max(0.0);
    let left = max > 0.0 && offset > metrics::ROW_FADE_THRESHOLD;
    let right = max > 0.0 && offset < max - metrics::ROW_FADE_THRESHOLD;
    let mut layers = stack![scroller].width(Fill).height(height);
    if left {
      layers = layers.push(
        container(fade_strip(page, false, metrics::ROW_FADE))
          .width(Fill)
          .height(Fill)
          .align_x(Horizontal::Left),
      );
    }
    if right {
      layers = layers.push(
        container(fade_strip(page, true, metrics::ROW_FADE))
          .width(Fill)
          .height(Fill)
          .align_x(Horizontal::Right),
      );
    }
    layers.into()
  })
  .width(Fill)
  .height(height)
  .into()
}

fn albums_row<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let albums = &app.ui.search.albums;
  albums_strip(
    app,
    roles,
    albums.len(),
    app.ui.search.albums_scroll,
    |offset| ui(UiMessage::SearchAlbumsScrolled(offset)),
    move || {
      albums
        .iter()
        .enumerate()
        .map(|(index, album)| result_card(app, roles, album, index))
        .collect()
    },
  )
}

/// `.search-albums-section` : titre « Albums » (`h3`) puis la rangée.
fn albums_section<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  column![
    line(
      app.i18n.t("Albums"),
      metrics::H3_TEXT,
      fonts::BOLD,
      roles.on_surface
    ),
    albums_row(app, roles),
    space().height(metrics::SECTION_MARGIN),
  ]
  .width(Fill)
  .into()
}

/// `.m3-search-loader-wrap` : la forme qui tourne, et en erreur l'icône et la boîte d'erreur.
fn loader<'a>(app: &App, roles: Roles, error: Option<&str>) -> Element<'a, Message> {
  let share = if error.is_some() {
    metrics::LOADER_FILL_ERROR
  } else {
    metrics::LOADER_FILL
  };
  // `.m3-search-loader` est un `.m3-btn-shape-spinner` : la boucle de `startButtonShapeSpinners` (`app.js:9944-9980`), partagée
  // avec le bouton lecture : même pose (forme et rotation) que tous les spinners affichés, sur l'horloge de `App`.
  let shape = spinner::shape_spinner(
    metrics::LOADER_STAGE,
    app.shape_pose(),
    Color {
      a: share,
      ..roles.on_surface
    },
  );
  let mut stage = stack![shape]
    .width(metrics::LOADER_STAGE)
    .height(metrics::LOADER_STAGE);
  if error.is_some() {
    // `transition` d'opacité et d'échelle après 180 ms, démarrée avec la boîte d'erreur (`styles.css:10417-10432`) ; `transform-origin`
    // au centre de la scène (valeur par défaut). L'opacité passe par l'alpha de la couleur du glyphe.
    let pose = app.entry_motion.error_icon_pose(app.now);
    let share = pose.map_or(1.0, |pose| pose.opacity);
    stage = stage.push(
      view_transition(
        container(icons::glyph_px(icons::SEARCH_ERROR, metrics::ERROR_ICON))
          .center(Fill)
          .style(move |_| {
            container::Style::default().color(Color {
              a: metrics::ERROR_ICON_OPACITY * share,
              ..roles.on_surface
            })
          }),
        pose.map(|pose| ViewPose {
          opacity: 1.0,
          ..pose
        }),
        None,
      )
      .centered(),
    );
  }
  let slot: Element<'a, Message> = match error {
    Some(error) => error_box(app, roles, error),
    None => space().into(),
  };
  container(
    column![stage, slot]
      .spacing(metrics::LOADER_GAP)
      .align_x(Horizontal::Center),
  )
  .padding(Padding {
    top: metrics::LOADER_PADDING_Y,
    right: metrics::LOADER_PADDING_X,
    bottom: metrics::LOADER_PADDING_Y,
    left: metrics::LOADER_PADDING_X,
  })
  .width(Fill)
  .align_x(Horizontal::Center)
  .into()
}

/// `.m3-search-error-box` : titre, détail en monospace et bouton de copie du diagnostic. `m3ErrFadeIn` : invisible 220 ms, puis opacité et
/// `translateY(8px)` en 320 ms `ease` (`styles.css:10433-10445`).
fn error_box<'a>(app: &App, roles: Roles, error: &str) -> Element<'a, Message> {
  let title = text(
    app
      .i18n
      .t("Error while fetching tracks. Please report this to GitHub WITH logs / diagnostics."),
  )
  .size(metrics::ERROR_TITLE_TEXT)
  .font(fonts::SEMIBOLD)
  .color(roles.on_surface)
  .line_height(LineHeight::Relative(1.45))
  .align_x(Horizontal::Center);
  let detail = container(
    text(app.i18n.t(error))
      .size(metrics::ERROR_DETAIL_TEXT)
      .font(Font::MONOSPACE)
      .color(roles.variant),
  )
  .padding([
    metrics::ERROR_DETAIL_PADDING_Y,
    metrics::ERROR_DETAIL_PADDING_X,
  ])
  .style(move |_| {
    container::Style::default()
      .background(Color {
        a: metrics::ERROR_DETAIL_SHARE,
        ..roles.on_surface
      })
      .border(border::rounded(metrics::ERROR_DETAIL_RADIUS))
  });
  let copy = button(
    container(
      text(app.i18n.t("Copy error & diagnostics"))
        .size(metrics::ERROR_BUTTON_TEXT)
        .font(fonts::MEDIUM),
    )
    .height(Fill)
    .align_y(Vertical::Center),
  )
  .padding(
    Padding::ZERO
      .left(metrics::ERROR_BUTTON_PADDING_X)
      .right(metrics::ERROR_BUTTON_PADDING_X),
  )
  .height(metrics::ERROR_BUTTON_HEIGHT)
  .style(primary_pill_style(roles))
  .on_press(ui(UiMessage::CopySearchError));
  let content = column![
    title,
    detail,
    container(copy).padding(Padding::ZERO.top(metrics::ERROR_BUTTON_MARGIN))
  ]
  .spacing(metrics::ERROR_GAP)
  .align_x(Horizontal::Center)
  .max_width(metrics::ERROR_MAX_WIDTH);
  view_transition(
    content,
    app.entry_motion.error_pose(app.now),
    fade_veil(app, roles.page),
  )
  .pop(0.0)
  .spill(metrics::ERROR_FADE_OFFSET)
  .into()
}

/// `.empty-hint` de la liste vide.
fn no_tracks<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  container(
    text(app.i18n.t("No matching tracks. Try a different search"))
      .size(metrics::HINT_TEXT)
      .color(roles.outline),
  )
  .padding([metrics::HINT_PADDING_Y, 0.0])
  .width(Fill)
  .align_x(Horizontal::Center)
  .into()
}

/// `#search-tracks-list` : le chargeur, l'erreur, le message vide ou les lignes visibles.
fn tracks<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let search = &app.ui.search;
  if search.loading {
    loader(app, roles, None)
  } else if let Some(error) = &search.error {
    loader(app, roles, Some(error))
  } else if search.tracks.is_empty() {
    no_tracks(app, roles)
  } else {
    app
      .ui
      .list(ListId::Search)
      .window(search.tracks.len(), move |index| {
        track_row(app, ListId::Search, index)
      })
  }
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  let search = &app.ui.search;
  let mut page = Column::new().width(Fill).push(chips(app, roles));
  if search_filter_shows(search.filter, SearchFilter::Artists) {
    page = page.extend(artist_card(app, roles));
  }
  if search_filter_shows(search.filter, SearchFilter::Albums) && !search.albums.is_empty() {
    page = page.push(albums_section(app, roles));
  }
  if search_filter_shows(search.filter, SearchFilter::Songs) {
    page = page.push(tracks(app, roles));
  }
  let content = container(page).padding(Padding {
    top: home_metrics::MAIN_TOP,
    // La barre de défilement de 5 px prend sa place dans la page (mesuré : bord droit à 1568 px).
    right: home_metrics::MAIN_RIGHT + home_metrics::SCROLLBAR,
    bottom: home_metrics::MAIN_BOTTOM,
    left: home_metrics::MAIN_LEFT,
  });
  scrollable(content)
    .id(ListId::Search.scroll_id())
    .direction(Direction::Vertical(
      Scrollbar::new()
        .width(home_metrics::SCROLLBAR)
        .scroller_width(home_metrics::SCROLLBAR)
        .margin(0),
    ))
    .on_scroll(|viewport| {
      ui(UiMessage::SearchScrolled(
        viewport.absolute_offset().y,
        viewport.bounds().height,
      ))
    })
    .style(move |_, status| page_scroll_style(roles, status))
    .width(Fill)
    .height(Fill)
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::ui::ArtistCard;

  fn state(filter: SearchFilter, albums: bool, artist: bool) -> SearchState {
    SearchState {
      filter,
      albums: if albums {
        vec![SavedAlbum::default()]
      } else {
        Vec::new()
      },
      artist: artist.then(ArtistCard::default),
      ..SearchState::default()
    }
  }

  #[test]
  fn the_list_starts_under_the_chips_and_the_blocks_above_it() {
    let base = home_metrics::MAIN_TOP + metrics::CHIP_HEIGHT + metrics::CHIPS_MARGIN;
    assert_eq!(list_top(&state(SearchFilter::All, false, false)), base);
    assert_eq!(
      list_top(&state(SearchFilter::All, true, false)),
      base + metrics::ALBUMS_HEIGHT
    );
    assert_eq!(
      list_top(&state(SearchFilter::All, true, true)),
      base + metrics::ALBUMS_HEIGHT + metrics::ARTIST_HEIGHT
    );
  }

  #[test]
  fn a_filter_hides_the_blocks_it_does_not_show() {
    let base = home_metrics::MAIN_TOP + metrics::CHIP_HEIGHT + metrics::CHIPS_MARGIN;
    assert_eq!(list_top(&state(SearchFilter::Songs, true, true)), base);
    assert_eq!(
      list_top(&state(SearchFilter::Albums, true, true)),
      base + metrics::ALBUMS_HEIGHT
    );
    assert_eq!(
      list_top(&state(SearchFilter::Artists, true, true)),
      base + metrics::ARTIST_HEIGHT
    );
  }

  /// Mesuré sur `03-resultats-recherche.png` : première ligne centrée à 465 px, soit 435 px de haut de liste
  /// dans un panneau qui commence à 97 px, remplissage compris dans `list_top` (1920 x 1080).
  #[test]
  fn the_first_row_lands_where_riff_puts_it() {
    let top = list_top(&state(SearchFilter::All, true, false));
    let center = 97.2 + top + tokens::track_row::HEIGHT / 2.0;
    assert!((center - 465.0).abs() < 1.5, "{center}");
  }
}
