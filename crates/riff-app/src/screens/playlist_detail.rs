//! Vue « Détail d'une playlist » `#view-playlist-detail` de Riff 44.1.1 (`index.html:716-762`, `app.js:13352-13450`,
//! `styles.css:5003-5230`, `9588-9596`, `9689-9693`) : bouton retour rond, en-tête (pochette de 160 px, surtitre, titre et
//! description éditables sur place, nombre de titres, menu de tri, boutons), champ de recherche, liste de titres. La page
//! défile d'un bloc et le halo `.hero-fx` de la pochette passe derrière toute la fenêtre (`screens::backdrop::hero_glow`).
//! Voir [`tokens::playlist_detail`].
//!
//! Écart voulu avec Riff (demandé par Hugo) : la page « Liked songs » (`screens::liked`) réutilise cette page, avec son nom,
//! sa description et son affiche éditables. [`Kind`] dit laquelle des deux est affichée. Sans objet pour les favoris, le
//! bouton de suppression est masqué.

use super::chrome::{tip, tips};
use super::common::{empty_hint, main_page, playlist_cover_url, track_row};
use super::home::{Roles, icon_pill, icon_toggle_pill, primary_pill_style};
use super::library::{hint, outline_style, search_box};
use super::right_panel::text_width;
use super::sidebar::liked_background;
use super::window_frame::overlay;
use crate::app::{App, Message};
use crate::download_ui::DownloadMessage;
use crate::images::{ImageKey, SIZE_HERO};
use crate::library::format_total_duration;
use crate::theme::tokens::home as page;
use crate::theme::tokens::local as pills;
use crate::theme::tokens::playlist_detail as metrics;
use crate::theme::tokens::track_row::PITCH;
use crate::theme::{fonts, icons, tokens};
use crate::ui::{ListId, UiMessage};
use crate::user_data::SortMode;
use crate::widgets::focus::focusable;
use iced::alignment::Vertical;
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::text_input::{self, Status as InputStatus};
use iced::widget::{
  Column, Row, button, column, container, image, mouse_area, pin, responsive, row, space, stack,
  text, text_input as input,
};
use iced::{
  Background, Border, Color, ContentFit, Element, Fill, Font, Length, Padding, Point, Theme,
  border, mouse,
};

const TITLE_ID: &str = "playlist-title";
const DESCRIPTION_ID: &str = "playlist-description";
const SEARCH_ID: &str = "playlist-search";
const LIKED_SEARCH_ID: &str = "favorites-search";

/// Décalage de la liste dans la page qui défile : remplissage, bouton retour, en-tête et champ de recherche.
#[must_use]
pub fn list_top() -> f32 {
  page::MAIN_TOP
    + metrics::BACK_SIZE
    + metrics::BACK_MARGIN
    + metrics::HEADER_HEIGHT
    + metrics::HEADER_MARGIN
    + metrics::SEARCH_HEIGHT
    + metrics::SEARCH_MARGIN
}

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

fn rgb(red: u8, green: u8, blue: u8) -> Color {
  Color::from_rgb8(red, green, blue)
}

/// Page affichée : le détail d'une playlist ou les titres aimés.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
  Playlist,
  Liked,
}

impl Kind {
  const fn list(self) -> ListId {
    match self {
      Self::Playlist => ListId::Playlist,
      Self::Liked => ListId::Favorites,
    }
  }

  fn query(self, app: &App) -> &str {
    match self {
      Self::Playlist => &app.ui.playlist_query,
      Self::Liked => &app.ui.favorites_query,
    }
  }

  fn sort_mode(self, app: &App) -> SortMode {
    match self {
      Self::Playlist => app.data.playlist_sort,
      Self::Liked => app.data.favorites_sort,
    }
  }

  fn pick_cover(self) -> Message {
    ui(match self {
      Self::Playlist => UiMessage::PlaylistPickCover,
      Self::Liked => UiMessage::FavoritesPickCover,
    })
  }
}

/// Ce que la page affiche : valeurs des champs éditables, affiche, nombre de titres et durée.
struct Subject {
  kind: Kind,
  /// Identifiant de la playlist (vide pour les titres aimés).
  id: String,
  /// Valeur du champ du nom, et libellé affiché quand il est vide.
  name: String,
  name_hint: String,
  description: String,
  cover: Option<String>,
  total: usize,
  seconds: f64,
}

/// Retrait de 10 px à gauche des éléments qui ne sont pas des champs éditables (marge négative de Riff).
fn indent(element: Element<'_, Message>) -> Element<'_, Message> {
  container(element)
    .padding(Padding::ZERO.left(metrics::EDIT_PADDING_X))
    .into()
}

/// Libellé du tri (`#playlist-sort-menu`, `index.html:726-728`).
const fn sort_label(mode: SortMode) -> &'static str {
  match mode {
    SortMode::Manual => "Manual order",
    SortMode::AddedDesc => "Date added (newest)",
    SortMode::AddedAsc => "Date added (oldest)",
  }
}

/// Durée totale affichée (`28 hr 7 min`) seulement si elle n'est pas nulle (`app.js:13392-13395`).
pub(super) fn count_text(total: usize, shown: usize, query: bool, seconds: f64) -> String {
  let duration = match format_total_duration(seconds) {
    text if text.is_empty() => text,
    text => format!(" \u{2022} {text}"),
  };
  if query {
    format!("{shown} of {total} tracks{duration}")
  } else {
    format!("{total} tracks{duration}")
  }
}

/// `#playlist-detail-title` et `#playlist-detail-desc` : champ éditable sur place, bordure transparente de 1,5 px ;
/// survol `on-surface` à 6 % et bordure `outline` à 35 %, focus bordure `primary` sur `surface-container-high`.
fn edit_style(
  roles: Roles,
  value: Color,
  placeholder: Color,
) -> impl Fn(&Theme, InputStatus) -> text_input::Style {
  move |theme, status| {
    let (background, color) = match status {
      InputStatus::Focused { .. } => (roles.high, roles.primary),
      InputStatus::Hovered => (
        Color {
          a: metrics::EDIT_HOVER_SHARE,
          ..roles.on_surface
        },
        Color {
          a: metrics::EDIT_HOVER_BORDER_SHARE,
          ..roles.outline
        },
      ),
      _ => (Color::TRANSPARENT, Color::TRANSPARENT),
    };
    text_input::Style {
      background: Background::Color(background),
      border: Border {
        color,
        width: metrics::EDIT_BORDER,
        radius: metrics::EDIT_RADIUS.into(),
      },
      icon: roles.on_surface,
      placeholder,
      value,
      selection: theme.extended_palette().primary.weak.color,
    }
  }
}

/// Police, hauteur de ligne, couleurs et tailles d'un champ éditable.
struct Field {
  font: Font,
  size: f32,
  line: f32,
  color: Color,
  hint_color: Color,
  box_height: f32,
  max_width: f32,
}

/// Champ éditable de la largeur de son texte (`width: fit-content; max-width: 100%`). Sa boîte déborde de 4 px en haut et
/// en bas et de 10 px à gauche (marges négatives de Riff) : l'appelant décale les voisins, voir [`view`].
fn editable<'a>(
  roles: Roles,
  id: &'static str,
  placeholder: &str,
  value: &str,
  field: Field,
  on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
  let Field {
    font,
    size,
    line,
    color,
    hint_color,
    box_height,
    max_width,
  } = field;
  let sides = metrics::EDIT_BORDER + metrics::EDIT_PADDING_X;
  let shown = if value.is_empty() { placeholder } else { value };
  let width = (text_width(shown, size, font) + 2.0 * sides + 2.0).min(max_width);
  let top = metrics::EDIT_BORDER + metrics::EDIT_PADDING_Y;
  input(placeholder, value)
    .id(id)
    .on_input(on_input)
    .font(font)
    .size(size)
    .line_height(LineHeight::Absolute(line.into()))
    .padding(Padding {
      top,
      right: sides,
      bottom: (box_height - top - line).max(0.0),
      left: sides,
    })
    .width(width)
    .style(edit_style(roles, color, hint_color))
    .into()
}

/// `.playlist-type-badge` : majuscules, graisse 700, `letter-spacing: .08em` (une cellule par lettre).
fn badge<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let letters = app
    .i18n
    .t("Playlist")
    .to_uppercase()
    .chars()
    .map(|letter| {
      text(letter.to_string())
        .size(metrics::BADGE_TEXT)
        .font(fonts::BOLD)
        .color(roles.variant)
        .wrapping(Wrapping::None)
        .into()
    })
    .collect::<Vec<Element<'a, Message>>>();
  container(Row::from_vec(letters).spacing(metrics::BADGE_SPACING * metrics::BADGE_TEXT))
    .height(metrics::BADGE_HEIGHT)
    .align_y(Vertical::Center)
    .into()
}

/// `#playlist-detail-cover` : image choisie ou première pochette, sinon la note de musique sur `surface-container-high`
/// (le cœur sur le dégradé de la barre latérale pour les titres aimés). Clic : choisir une image ; clic droit : menu.
fn cover<'a>(app: &App, roles: Roles, subject: &Subject) -> Element<'a, Message> {
  let handle = subject
    .cover
    .as_ref()
    .and_then(|url| app.images.get(&ImageKey::new(url, SIZE_HERO)).cloned());
  let content: Element<'a, Message> = match (handle, subject.kind) {
    (Some(handle), _) => image(handle)
      .width(metrics::COVER)
      .height(metrics::COVER)
      .content_fit(ContentFit::Cover)
      .border_radius(metrics::COVER_RADIUS)
      .into(),
    (None, Kind::Liked) => container(icons::glyph_px(icons::LIKED_HEART, metrics::COVER_ICON))
      .center(Length::Fixed(metrics::COVER))
      .style(move |_| {
        container::Style::default()
          .background(liked_background(roles.primary))
          .color(roles.on_primary)
          .border(border::rounded(metrics::COVER_RADIUS))
      })
      .into(),
    (None, Kind::Playlist) => container(icons::glyph_px(icons::ROW_FALLBACK, metrics::COVER_ICON))
      .center(Length::Fixed(metrics::COVER))
      .style(move |_| {
        container::Style::default()
          .background(roles.high)
          .color(roles.outline)
          .border(border::rounded(metrics::COVER_RADIUS))
      })
      .into(),
  };
  let pick = subject.kind.pick_cover();
  focusable(
    mouse_area(content)
      .interaction(mouse::Interaction::Pointer)
      .on_press(pick.clone()),
  )
  .on_activate(Some(pick))
  .on_context(|position| ui(UiMessage::CoverMenu(position)))
  .radius(metrics::COVER_RADIUS)
  .into()
}

/// `.btn-pill-primary` (Jouer) ou `.btn-outline-pill` : icône, libellé en graisse 500. Sans `riff_highlight_buttons`, le
/// bouton primaire prend le style de l'autre (`styles.css:7564`).
pub(super) fn pill<'a>(
  app: &App,
  roles: Roles,
  label: String,
  glyph: &'a str,
  icon: f32,
  primary: bool,
  message: Message,
) -> Element<'a, Message> {
  let highlighted = primary && app.settings.highlight_buttons;
  let key_label = label.clone();
  let content = row![
    icons::glyph_px(glyph, icon),
    text(label).size(pills::BUTTON_TEXT).font(fonts::MEDIUM),
  ]
  .spacing(pills::BUTTON_GAP)
  .align_y(Vertical::Center);
  let side = if primary {
    metrics::PLAY_PADDING_X
  } else {
    pills::OUTLINE_PADDING_X
  };
  let widget = button(container(content).height(Fill).align_y(Vertical::Center))
    .padding(Padding::ZERO.left(side).right(side))
    .height(pills::BUTTON_HEIGHT)
    .on_press(message.clone());
  let radius = tokens::css(18.0);
  if highlighted {
    focusable(widget.style(primary_pill_style(roles)))
      .ripple(&key_label, roles.on_primary, tokens::css(18.0))
      .on_activate(Some(message))
      .radius(radius)
      .into()
  } else {
    focusable(widget.style(outline_style(roles)))
      .ripple(&key_label, roles.variant, tokens::css(18.0))
      .on_activate(Some(message))
      .radius(radius)
      .into()
  }
}

/// `#btn-delete-playlist` : rond de 44 px, `error` sur `surface-container-high` ; survol `error-container`, calque de 8 %
/// et rayon à 40 % (`styles.css:5169-5192`, `7501-7522`).
fn delete_button<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let scheme = &app.theme.scheme;
  let (error, container_fill, on_container) = (
    rgb(scheme.error.red, scheme.error.green, scheme.error.blue),
    rgb(
      scheme.error_container.red,
      scheme.error_container.green,
      scheme.error_container.blue,
    ),
    rgb(
      scheme.on_error_container.red,
      scheme.on_error_container.green,
      scheme.on_error_container.blue,
    ),
  );
  let side = metrics::DELETE_SIZE;
  let message = ui(UiMessage::PlaylistDelete);
  let widget =
    button(container(icons::glyph_px(icons::PL_DELETE, metrics::DELETE_ICON)).center(Fill))
      .padding(0)
      .width(side)
      .height(side)
      .style(move |_, status| {
        let (fill, label, radius) = match status {
          button::Status::Hovered => (
            overlay(container_fill, roles.on_surface, 0.08),
            on_container,
            side * 0.4,
          ),
          button::Status::Pressed => (
            overlay(container_fill, roles.on_surface, 0.12),
            on_container,
            tokens::css(12.0),
          ),
          _ => (roles.high, error, side / 2.0),
        };
        button::Style {
          background: Some(Background::Color(fill)),
          text_color: label,
          border: border::rounded(radius),
          ..button::Style::default()
        }
      })
      .on_press(message.clone());
  tip(
    focusable(widget)
      .on_activate(Some(message))
      .radius(side / 2.0)
      .into(),
    app.i18n.t(tips::DELETE_PLAYLIST),
  )
}

/// `#playlist-sort-trigger` : `surface-container-highest`, libellé en graisse 500 et flèche ; ouvert, les coins du bas passent à 6 px.
fn sort_trigger<'a>(app: &App, roles: Roles, open: bool, mode: SortMode) -> Element<'a, Message> {
  let message = ui(UiMessage::PlaylistSortOpen(!open));
  let radius = border::Radius {
    top_left: metrics::SORT_RADIUS,
    top_right: metrics::SORT_RADIUS,
    bottom_right: if open {
      metrics::SORT_OPEN_RADIUS
    } else {
      metrics::SORT_RADIUS
    },
    bottom_left: if open {
      metrics::SORT_OPEN_RADIUS
    } else {
      metrics::SORT_RADIUS
    },
  };
  let content = row![
    text(app.i18n.t(sort_label(mode)))
      .size(metrics::SORT_TEXT)
      .font(fonts::MEDIUM)
      .wrapping(Wrapping::None)
      .width(Fill),
    container(icons::glyph_px(
      if open {
        icons::PL_CHEVRON_UP
      } else {
        icons::PL_CHEVRON_DOWN
      },
      metrics::SORT_ARROW,
    ))
    .style(move |_| container::Style::default().color(roles.variant)),
  ]
  .spacing(metrics::SORT_GAP)
  .align_y(Vertical::Center);
  let widget = button(container(content).height(Fill).align_y(Vertical::Center))
    .padding(
      Padding::ZERO
        .left(metrics::SORT_PADDING_X)
        .right(metrics::SORT_PADDING_X),
    )
    .width(metrics::SORT_WIDTH)
    .height(metrics::SORT_HEIGHT)
    .style(move |_, status| button::Style {
      background: Some(Background::Color(if status == button::Status::Hovered {
        roles.high
      } else {
        roles.highest
      })),
      text_color: roles.on_surface,
      border: border::Border {
        radius,
        ..Border::default()
      },
      ..button::Style::default()
    })
    .on_press(message.clone());
  focusable(widget)
    .on_activate(Some(message))
    .radius(metrics::SORT_RADIUS)
    .into()
}

/// `#playlist-sort-menu` : trois options, la courante en `primary`, survol `primary` à 18 %.
fn sort_menu<'a>(app: &App, roles: Roles, kind: Kind) -> Element<'a, Message> {
  let current = kind.sort_mode(app);
  let options = [SortMode::Manual, SortMode::AddedDesc, SortMode::AddedAsc]
    .into_iter()
    .map(|mode| {
      let selected = mode == current;
      let message = ui(match kind {
        Kind::Playlist => UiMessage::SetPlaylistSort(mode),
        Kind::Liked => UiMessage::SetFavoritesSort(mode),
      });
      let label = text(app.i18n.t(sort_label(mode)))
        .size(metrics::SORT_TEXT)
        .font(if selected { fonts::MEDIUM } else { fonts::UI })
        .wrapping(Wrapping::None);
      let widget = button(container(label).height(Fill).align_y(Vertical::Center))
        .padding(
          Padding::ZERO
            .left(metrics::OPTION_PADDING_X)
            .right(metrics::OPTION_PADDING_X),
        )
        .width(Fill)
        .height(metrics::OPTION_HEIGHT)
        .style(move |_, status| {
          let fill = if selected {
            Some(roles.primary)
          } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Some(Color {
              a: metrics::OPTION_HOVER_SHARE,
              ..roles.primary
            })
          } else {
            None
          };
          button::Style {
            background: fill.map(Background::Color),
            text_color: if selected {
              roles.on_primary
            } else {
              roles.on_surface
            },
            border: border::rounded(metrics::OPTION_RADIUS),
            ..button::Style::default()
          }
        })
        .on_press(message);
      Element::from(widget)
    });
  container(Column::with_children(options))
    .padding(metrics::MENU_PADDING)
    .width(metrics::SORT_WIDTH)
    .style(move |_| {
      container::Style::default()
        .background(Color {
          a: metrics::MENU_SHARE,
          ..roles.high
        })
        .border(border::rounded(metrics::MENU_RADIUS))
    })
    .into()
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let Some(playlist) = app
    .ui
    .playlist_id
    .as_deref()
    .and_then(|id| app.data.playlist(id))
  else {
    return empty_hint(app, "This playlist is empty");
  };
  page(
    app,
    Subject {
      kind: Kind::Playlist,
      id: playlist.id.clone().unwrap_or_default(),
      name: playlist.name.clone().unwrap_or_default(),
      name_hint: app.i18n.t("Untitled"),
      description: playlist.description.clone().unwrap_or_default(),
      cover: playlist_cover_url(playlist),
      total: playlist.tracks.len(),
      seconds: playlist
        .tracks
        .iter()
        .filter_map(|track| track.duration)
        .sum(),
    },
  )
}

/// Page des titres aimés : le même en-tête, avec le nom, la description et l'affiche enregistrés dans `UserData`.
#[must_use]
pub fn liked_view(app: &App) -> Element<'_, Message> {
  let fallback = app.i18n.t("Liked songs");
  page(
    app,
    Subject {
      kind: Kind::Liked,
      id: String::new(),
      name: app
        .data
        .liked_name
        .clone()
        .unwrap_or_else(|| fallback.clone()),
      name_hint: fallback,
      description: app.data.liked_description.clone().unwrap_or_default(),
      cover: app.data.liked_custom_cover().map(str::to_string),
      total: app.data.favorites.len(),
      seconds: app
        .data
        .favorites
        .iter()
        .filter_map(|track| track.duration)
        .sum(),
    },
  )
}

fn page(app: &App, subject: Subject) -> Element<'_, Message> {
  let roles = Roles::of(app);
  let kind = subject.kind;
  let list = kind.list();
  responsive(move |size| {
    let subject = &subject;
    let shown = app.list_len(list);
    let filtered = !kind.query(app).trim().is_empty();
    let content_height = list_top() + shown as f32 * PITCH + page::MAIN_BOTTOM;
    let overflow = content_height > size.height;

    // Largeur disponible pour la colonne de texte, pour borner les champs éditables (`max-width: 100%`).
    let text_max = size.width
      - page::MAIN_LEFT
      - page::MAIN_RIGHT
      - if overflow { page::SCROLLBAR } else { 0.0 }
      - 2.0 * metrics::HEADER_PADDING_X
      - metrics::COVER
      - metrics::HEADER_GAP
      + metrics::EDIT_PADDING_X;
    let title_line = metrics::TITLE_TEXT * metrics::TITLE_LINE;
    let desc_line = metrics::DESC_TEXT * metrics::LINE;
    let edit_pad = 2.0 * (metrics::EDIT_BORDER + metrics::EDIT_PADDING_Y);
    let title = editable(
      roles,
      TITLE_ID,
      &subject.name_hint,
      &subject.name,
      Field {
        font: fonts::BOLD,
        size: metrics::TITLE_TEXT,
        line: title_line,
        color: roles.on_surface,
        hint_color: roles.outline,
        box_height: title_line + edit_pad,
        max_width: text_max,
      },
      move |name| {
        ui(match kind {
          Kind::Playlist => UiMessage::PlaylistRename(name),
          Kind::Liked => UiMessage::FavoritesRename(name),
        })
      },
    );
    let description = editable(
      roles,
      DESCRIPTION_ID,
      &app.i18n.t("Add a description..."),
      &subject.description,
      Field {
        font: fonts::UI,
        size: metrics::DESC_TEXT,
        line: desc_line,
        color: roles.variant,
        hint_color: Color {
          a: metrics::DESC_EMPTY_SHARE,
          ..roles.variant
        },
        box_height: metrics::DESC_MIN - metrics::DESC_TRIM + edit_pad,
        max_width: text_max,
      },
      move |text| {
        ui(match kind {
          Kind::Playlist => UiMessage::PlaylistDescription(text),
          Kind::Liked => UiMessage::FavoritesDescription(text),
        })
      },
    );
    let count = text(count_text(subject.total, shown, filtered, subject.seconds))
      .size(metrics::COUNT_TEXT)
      .color(roles.variant)
      .wrapping(Wrapping::None);
    let sort = sort_trigger(app, roles, app.ui.playlist_sort_open, kind.sort_mode(app));
    let play = |message: UiMessage| {
      pill(
        app,
        roles,
        app.i18n.t("Play"),
        icons::PL_PLAY,
        metrics::PLAY_ICON,
        true,
        ui(message),
      )
    };
    let download = |message: DownloadMessage| {
      tip(
        pill(
          app,
          roles,
          app.i18n.t("Download all"),
          icons::PL_DOWNLOAD,
          metrics::PILL_ICON,
          false,
          Message::Download(message),
        ),
        app.i18n.t("Download all songs"),
      )
    };
    let export = |message: UiMessage| {
      pill(
        app,
        roles,
        app.i18n.t("Export as JSON"),
        icons::PL_EXPORT,
        metrics::PILL_ICON,
        false,
        ui(message),
      )
    };
    // Bascule de l'aléatoire de cette liste, mémorisée pour elle seule. Bouton icône de l'encombrement de la corbeille.
    let shuffle_on = app
      .pin_scope()
      .is_some_and(|scope| app.data.shuffle_on(&scope));
    let shuffle = tip(
      icon_toggle_pill(
        roles,
        icons::SHUFFLE,
        metrics::DELETE_ICON,
        metrics::DELETE_SIZE,
        shuffle_on,
        ui(UiMessage::ListShuffleToggle),
      ),
      app.i18n.t(tips::shuffle(shuffle_on)),
    );
    let actions = match kind {
      Kind::Playlist => row![
        sort,
        play(UiMessage::PlaylistPlay),
        shuffle,
        download(DownloadMessage::OpenAll),
        export(UiMessage::PlaylistExport(subject.id.clone())),
        delete_button(app, roles),
      ],
      // Pas de bouton de suppression : les titres aimés ne se suppriment pas.
      Kind::Liked => row![
        sort,
        play(UiMessage::FavoritesPlay),
        shuffle,
        download(DownloadMessage::OpenAllFavorites),
        export(UiMessage::FavoritesExport),
      ],
    }
    .spacing(metrics::ACTIONS_GAP)
    .align_y(Vertical::Center);
    // Les marges négatives de Riff sont reprises en retirant 4 px aux écarts autour des champs éditables.
    let gap = metrics::META_GAP - metrics::EDIT_PADDING_Y;
    let meta = column![
      indent(badge(app, roles)),
      space().height(gap),
      title,
      space().height(metrics::META_GAP - 2.0 * metrics::EDIT_PADDING_Y),
      description,
      space().height(gap),
      indent(
        container(count)
          .height(metrics::COUNT_HEIGHT)
          .align_y(Vertical::Center)
          .into()
      ),
      space().height(metrics::META_GAP + metrics::ACTIONS_MARGIN + metrics::ACTIONS_EXTRA),
      indent(
        container(actions)
          .height(metrics::ACTIONS_HEIGHT)
          .align_y(Vertical::Center)
          .into()
      ),
    ]
    .width(Fill);
    let header = container(
      row![cover(app, roles, subject), meta]
        .spacing(metrics::HEADER_GAP - metrics::EDIT_PADDING_X)
        .align_y(Vertical::Bottom),
    )
    .padding(Padding {
      top: metrics::HEADER_PADDING_TOP,
      right: metrics::HEADER_PADDING_X,
      bottom: metrics::HEADER_PADDING_BOTTOM,
      left: metrics::HEADER_PADDING_X,
    })
    .width(Fill);

    let back = row![tip(
      icon_pill(
        roles,
        icons::PL_BACK,
        metrics::BACK_ICON,
        metrics::BACK_SIZE,
        Message::Back,
      ),
      app.i18n.t(tips::BACK),
    )];
    let (search_label, search_id) = match kind {
      Kind::Playlist => ("Search in playlist...", SEARCH_ID),
      Kind::Liked => ("Search favorites...", LIKED_SEARCH_ID),
    };
    let mut body = Column::new()
      .width(Fill)
      .push(back)
      .push(space().height(metrics::BACK_MARGIN))
      .push(header)
      .push(space().height(metrics::HEADER_MARGIN))
      .push(search_box(
        roles,
        search_id,
        &app.i18n.t(search_label),
        kind.query(app),
        move |query| {
          ui(match kind {
            Kind::Playlist => UiMessage::PlaylistQuery(query),
            Kind::Liked => UiMessage::FavoritesQuery(query),
          })
        },
      ))
      .push(space().height(metrics::SEARCH_MARGIN));
    body = body.push(if shown == 0 {
      match kind {
        // `renderTracks` : le même message vide pour « aucun favori » et « aucun résultat » (`app.js:11436-11440`).
        Kind::Liked => hint(
          app,
          roles,
          "No favourites yet. Use the heart on a track to save it",
        ),
        Kind::Playlist => hint(app, roles, "This playlist is empty"),
      }
    } else {
      app
        .ui
        .list(list)
        .window(shown, move |index| track_row(app, list, index))
    });
    let mut layers = stack![body].width(Fill);
    if app.ui.playlist_sort_open {
      // Le menu s'ouvre sous le déclencheur : `top: calc(100% + 2px)`, sur sa largeur.
      let menu_x = metrics::HEADER_PADDING_X + metrics::COVER + metrics::HEADER_GAP;
      let menu_y = metrics::BACK_SIZE
        + metrics::BACK_MARGIN
        + metrics::HEADER_PADDING_TOP
        + metrics::CONTENT_HEIGHT
        - (metrics::ACTIONS_HEIGHT - metrics::SORT_HEIGHT) / 2.0
        + metrics::MENU_GAP;
      layers = layers
        .push(
          mouse_area(space().width(Fill).height(Fill))
            .on_press(ui(UiMessage::PlaylistSortOpen(false))),
        )
        .push(pin(sort_menu(app, roles, kind)).position(Point::new(menu_x, menu_y)));
    }
    main_page(roles, layers.into(), list.scroll_id(), overflow)
      .on_scroll(move |viewport| {
        let (offset, height) = (viewport.absolute_offset().y, viewport.bounds().height);
        ui(match kind {
          Kind::Playlist => UiMessage::PlaylistScrolled(offset, height),
          Kind::Liked => UiMessage::FavoritesScrolled(offset, height),
        })
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

  /// 07, 1920 x 1080 : le champ de recherche couvre 432 à 466 px, la première ligne commence vers 480 px, la page à 97 px.
  #[test]
  fn the_list_starts_under_the_search_box() {
    assert!((list_top() + 97.0 - 480.0).abs() < 3.0, "{}", list_top());
  }

  #[test]
  fn the_count_shows_the_duration_only_when_known() {
    assert_eq!(
      count_text(567, 567, false, 100_994.0),
      "567 tracks \u{2022} 28 hr 3 min"
    );
    assert_eq!(count_text(0, 0, false, 0.0), "0 tracks");
    assert_eq!(count_text(5, 2, true, 60.0), "2 of 5 tracks \u{2022} 1 min");
  }

  #[test]
  fn every_sort_mode_has_a_label_of_riff() {
    assert_eq!(sort_label(SortMode::Manual), "Manual order");
    assert_eq!(sort_label(SortMode::AddedDesc), "Date added (newest)");
    assert_eq!(sort_label(SortMode::AddedAsc), "Date added (oldest)");
  }
}
