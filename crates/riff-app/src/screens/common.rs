//! Éléments communs des écrans : pochettes, cartes, lignes de piste.

use super::chrome::{tip, tips};
use super::home::{Roles, blend, explicit_badge, row_icon_pill};
use super::right_panel::ellipsize;
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_CARD, SIZE_HERO, SIZE_ROW};
use crate::library::{ArtistToken, format_duration, split_artist_tokens, split_artists};
use crate::motion::ControlKey;
use crate::theme::tokens::artist;
use crate::theme::tokens::track_row as metrics;
use crate::theme::{fonts, icons, tokens};
use crate::ui::{ListId, ROW_HEIGHT, TrackCommand, UiMessage};
use crate::user_data::{Playlist, SavedAlbum, SortMode, Track};
use crate::widgets::button::{text_style, tonal_style};
use crate::widgets::focus::focusable;
use crate::widgets::hover;
use crate::widgets::view_transition::view_transition;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
  Column, Row, button, column, container, image, mouse_area, responsive, row, scrollable, space,
  text,
};
use iced::{
  Background, Border, Color, ContentFit, Element, Fill, Length, Point, Theme, border, mouse,
};

const CARD_WIDTH: f32 = 148.0;
pub const SIZE_DETAIL: f32 = 200.0;

/// Part de `on-surface` du calque `--hl` d'une rangée de réglage à l'instant de la vue : `.setting-item:hover { --hl: 4% }`
/// (`tokens::touch::SETTING_HOVER_SHARE`), animé en `.18s ease` par `ControlHover`.
#[must_use]
pub fn setting_hl(key: ControlKey) -> f32 {
  tokens::touch::SETTING_HOVER_SHARE * crate::motion::view_mix(key).color
}

/// Fond d'une rangée de réglage : `base` (fond de repos) sous le calque `on-surface` de `--hl`.
#[must_use]
pub fn setting_fill(on_surface: Color, base: Color, key: ControlKey) -> Color {
  let share = setting_hl(key);
  if share > 0.0 {
    blend(on_surface, base, share)
  } else {
    base
  }
}

/// Entrée et sortie du pointeur d'une rangée de réglage : instants du survol animé (`ControlHover`). À omettre pour une rangée
/// `.is-disabled` (`pointer-events: none`, `styles.css:3502-3506`).
pub fn setting_hover<'a>(
  key: ControlKey,
  row: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
  hover::area(key, Message::Ui, row).into()
}

pub fn placeholder_style(theme: &Theme) -> container::Style {
  container::Style::default()
    .background(theme.extended_palette().background.weak.color)
    .border(border::rounded(8))
}

/// Pochette de la taille affichée `display`, décodée à `size`. Absente ou pas encore chargée : un aplat.
pub fn cover<'a>(
  app: &App,
  url: Option<&str>,
  display: f32,
  size: u32,
  round: bool,
) -> Element<'a, Message> {
  let radius = if round { display / 2.0 } else { 8.0 };
  let handle = url.and_then(|url| app.images.get(&ImageKey::new(url, size)));
  match handle {
    Some(handle) => image(handle.clone())
      .width(Length::Fixed(display))
      .height(Length::Fixed(display))
      .content_fit(ContentFit::Cover)
      .border_radius(radius)
      .into(),
    None => container(space())
      .width(Length::Fixed(display))
      .height(Length::Fixed(display))
      .style(move |theme: &Theme| placeholder_style(theme).border(border::rounded(radius)))
      .into(),
  }
}

/// Grande pochette d'une page.
pub fn big_cover<'a>(
  app: &App,
  url: Option<String>,
  display: f32,
  round: bool,
) -> Element<'a, Message> {
  cover(app, url.as_deref(), display, SIZE_HERO, round)
}

/// Pochette à 4 images d'un mix.
pub fn grid_cover<'a>(app: &App, urls: &[String], display: f32) -> Element<'a, Message> {
  let side = (display - 4.0) / 2.0;
  let cell = |index: usize| {
    cover(
      app,
      urls.get(index).map(String::as_str),
      side,
      SIZE_CARD,
      false,
    )
  };
  column![
    row![cell(0), cell(1)].spacing(4),
    row![cell(2), cell(3)].spacing(4)
  ]
  .spacing(4)
  .into()
}

/// Pochette d'une playlist : image choisie, sinon première pochette (`renderPlaylistCover`).
#[must_use]
pub fn playlist_cover_url(playlist: &Playlist) -> Option<String> {
  playlist
    .cover
    .as_deref()
    .map(str::trim)
    .filter(|cover| !cover.is_empty())
    .map(str::to_string)
    .or_else(|| playlist.tracks.iter().find_map(Track::thumb_url))
}

/// Titre et sous-titre d'un écran.
pub fn title_line<'a>(title: String, subtitle: String) -> Element<'a, Message> {
  column![text(title).size(26), text(subtitle).size(13)]
    .spacing(2)
    .width(Fill)
    .into()
}

pub fn section_title<'a>(title: String) -> Element<'a, Message> {
  text(title)
    .size(tokens::text::SECTION_TITLE)
    .font(fonts::BOLD)
    .into()
}

/// Message centré d'une liste vide.
pub fn empty_hint<'a>(app: &App, key: &str) -> Element<'a, Message> {
  container(text(app.i18n.t(key)).size(14))
    .center_x(Fill)
    .padding(32)
    .into()
}

/// Bouton bascule.
pub fn chip<'a>(label: String, selected: bool, message: Message) -> Element<'a, Message> {
  let entry = button(text(label).size(13))
    .padding([6, 14])
    .style(if selected { tonal_style } else { text_style })
    .on_press(message.clone());
  focusable(entry)
    .on_activate(Some(message))
    .radius(20.0)
    .into()
}

/// Choix du tri : manuel, plus récent, plus ancien.
pub fn sort_chips<'a>(
  app: &App,
  current: SortMode,
  on_select: fn(SortMode) -> Message,
) -> Element<'a, Message> {
  let entry =
    |label: &str, mode: SortMode| chip(app.i18n.t(label), current == mode, on_select(mode));
  row![
    entry("Manual order", SortMode::Manual),
    entry("Date added (newest)", SortMode::AddedDesc),
    entry("Date added (oldest)", SortMode::AddedAsc),
  ]
  .spacing(4)
  .into()
}

/// Contenu d'une carte.
pub struct Card {
  pub cover: Option<String>,
  pub title: String,
  pub sub: String,
  pub round: bool,
  pub message: Message,
}

fn card_body<'a>(app: &App, card: Card) -> (iced::widget::Button<'a, Message>, Message) {
  let message = card.message;
  let body = column![
    cover(
      app,
      card.cover.as_deref(),
      CARD_WIDTH - 16.0,
      SIZE_CARD,
      card.round
    ),
    text(card.title)
      .size(tokens::text::CARD_TITLE)
      .wrapping(text::Wrapping::None)
      .width(Fill),
    text(card.sub)
      .size(tokens::text::SUBTITLE)
      .wrapping(text::Wrapping::None)
      .width(Fill),
  ]
  .spacing(4)
  .clip(true);
  (
    button(body)
      .padding(8)
      .width(CARD_WIDTH)
      .style(text_style)
      .on_press(message.clone()),
    message,
  )
}

/// Carte d'album, d'artiste ou de piste.
pub fn card<'a>(app: &App, card: Card) -> Element<'a, Message> {
  let (entry, message) = card_body(app, card);
  focusable(entry)
    .on_activate(Some(message))
    .radius(12.0)
    .into()
}

/// Carte avec menu contextuel.
pub fn card_with_context<'a>(
  app: &App,
  card: Card,
  on_context: impl Fn(Point) -> Message + 'a,
) -> Element<'a, Message> {
  let (entry, message) = card_body(app, card);
  focusable(entry)
    .on_activate(Some(message))
    .on_context(on_context)
    .radius(12.0)
    .into()
}

/// Rangée de cartes qui défile horizontalement.
pub fn cards_row<'a>(
  cards: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
  let items: Vec<Element<'a, Message>> = cards.into_iter().collect();
  scrollable(
    Row::from_vec(items)
      .spacing(4)
      .padding(iced::Padding::ZERO.bottom(12)),
  )
  .direction(scrollable::Direction::Horizontal(
    scrollable::Scrollbar::new().width(4).scroller_width(4),
  ))
  .width(Fill)
  .into()
}

/// Cartes qui passent à la ligne.
pub fn wrap_cards<'a>(cards: Vec<Element<'a, Message>>) -> Element<'a, Message> {
  Row::from_vec(cards).spacing(4).wrap().into()
}

/// Grille d'albums.
pub fn album_grid<'a>(
  app: &App,
  albums: &[SavedAlbum],
  open: impl Fn(usize) -> Message,
) -> Element<'a, Message> {
  let cards = albums.iter().enumerate().map(|(index, album)| {
    let sub: Vec<&str> = [
      album.kind.as_deref().unwrap_or("Album"),
      album.artist.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect();
    card(
      app,
      Card {
        cover: album.cover.clone(),
        title: album.title.clone().unwrap_or_default(),
        sub: sub.join(" · "),
        round: false,
        message: open(index),
      },
    )
  });
  wrap_cards(cards.collect())
}

/// Artistes cliquables, séparés par des virgules.
pub fn artist_links<'a>(
  raw: &str,
  size: f32,
  open: impl Fn(String) -> Message,
) -> Element<'a, Message> {
  let names = split_artists(raw);
  let last = names.len().saturating_sub(1);
  let parts = names.into_iter().enumerate().map(|(index, name)| {
    let label = if index < last {
      format!("{name},")
    } else {
      name.clone()
    };
    mouse_area(text(label).size(size).wrapping(text::Wrapping::None))
      .interaction(mouse::Interaction::Pointer)
      .on_press(open(name))
      .into()
  });
  Row::with_children(parts).spacing(4).clip(true).into()
}

/// Boutons d'une ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
  Share,
  Favorite,
  Art,
  AddToPlaylist,
  Download,
  RemoveLocal,
  RemoveFromPlaylist,
  RemoveFromQueue,
}

impl RowAction {
  pub fn command(self, index: usize) -> TrackCommand {
    match self {
      Self::Share => TrackCommand::Share,
      Self::Favorite => TrackCommand::ToggleFavorite,
      Self::Art => TrackCommand::Art,
      Self::AddToPlaylist => TrackCommand::AddToPlaylist,
      Self::Download => TrackCommand::Download,
      Self::RemoveLocal => TrackCommand::RemoveLocal,
      Self::RemoveFromPlaylist => TrackCommand::RemoveFromPlaylist,
      Self::RemoveFromQueue => TrackCommand::RemoveFromQueue(index),
    }
  }
}

/// Boutons d'une ligne dans l'ordre de Riff (`createTrackRow`) : partage, favori, pochette et bannière, ajout à une
/// playlist, puis téléchargement ou retrait des fichiers, puis retrait de la playlist ou de la file.
pub fn row_actions(list: ListId, track: &Track) -> Vec<RowAction> {
  let local = list == ListId::Local || track.is_local();
  let mut actions = vec![
    RowAction::Share,
    RowAction::Favorite,
    RowAction::Art,
    RowAction::AddToPlaylist,
    if local {
      RowAction::RemoveLocal
    } else {
      RowAction::Download
    },
  ];
  match list {
    ListId::Playlist => actions.push(RowAction::RemoveFromPlaylist),
    ListId::Queue => actions.push(RowAction::RemoveFromQueue),
    _ => {}
  }
  actions
}

/// Boutons visibles d'une ligne : `.track-row .track-row-actions > button:not(.btn-fav):not(.btn-remove-from-queue)`
/// est masqué (`styles.css:10013`). Les autres actions passent par le menu contextuel.
const fn shown_in_row(action: RowAction) -> bool {
  matches!(action, RowAction::Favorite | RowAction::RemoveFromQueue)
}

/// Clé i18n de l'infobulle d'un bouton de ligne : le favori suit son état.
const fn row_action_key(action: RowAction, favorite: bool) -> &'static str {
  match action {
    RowAction::Favorite => tips::favorite(favorite),
    RowAction::RemoveFromQueue => tips::REMOVE_FROM_QUEUE,
    _ => "Remove",
  }
}

/// Numéro de la ligne : `index + 1`, ou `track.no` dans un album (`app.js:11198`).
fn row_number(list: ListId, track: &Track, index: usize) -> usize {
  let album_no = (list == ListId::Album)
    .then(|| track.extra.get("no").and_then(serde_json::Value::as_u64))
    .flatten();
  album_no.map_or(index + 1, |no| no as usize)
}

fn row_line<'a>(label: String, size: f32, font: iced::Font, color: Color) -> Element<'a, Message> {
  text(label)
    .size(size)
    .font(font)
    .color(color)
    .line_height(LineHeight::Relative(tokens::home::H2_LINE))
    .wrapping(Wrapping::None)
    .into()
}

/// `.sp-playing-bars` : quatre barres de hauteur `heights` (fractions de 14 px, `App::bar_heights`), à la place du numéro de la piste en cours.
fn playing_bars<'a>(roles: Roles, heights: [f32; 4]) -> Element<'a, Message> {
  let bar = |height: f32| {
    container(space())
      .width(metrics::BAR_WIDTH)
      .height(metrics::BARS_HEIGHT * height)
      .style(move |_| {
        container::Style::default()
          .background(roles.primary)
          .border(border::rounded(tokens::css(1.0)))
      })
  };
  container(
    row![
      bar(heights[0]),
      bar(heights[1]),
      bar(heights[2]),
      bar(heights[3])
    ]
    .spacing(metrics::BAR_GAP),
  )
  .width(metrics::BARS_WIDTH)
  .height(metrics::BARS_HEIGHT)
  .align_x(Horizontal::Center)
  .align_y(Vertical::Bottom)
  .into()
}

/// `.track-row-thumb-wrap` : icône de repli sous l'image, `surface-container-high`.
fn row_thumb<'a>(
  app: &App,
  roles: Roles,
  url: Option<&str>,
  side: f32,
  radius: f32,
) -> Element<'a, Message> {
  let handle = url.and_then(|url| app.images.get(&ImageKey::new(url, SIZE_ROW)));
  let content: Element<'a, Message> = match handle {
    Some(handle) => image(handle.clone())
      .width(side)
      .height(side)
      .content_fit(ContentFit::Cover)
      .border_radius(radius)
      .into(),
    None => icons::glyph_px(icons::ROW_FALLBACK, metrics::THUMB_ICON),
  };
  container(content)
    .width(side)
    .height(side)
    .align_x(Horizontal::Center)
    .align_y(Vertical::Center)
    .clip(true)
    .style(move |_| {
      container::Style::default()
        .background(roles.high)
        .color(roles.outline)
        .border(border::rounded(radius))
    })
    .into()
}

/// `.track-row-artist-wrap` : pastille explicite puis un lien par artiste (`renderClickableArtists`),
/// séparés par `, ` à 75 % d'opacité (`.artist-sep`, `styles.css:1743`).
fn row_artist<'a>(
  roles: Roles,
  track: &Track,
  explicit: bool,
  list: ListId,
  index: usize,
  size: f32,
) -> Element<'a, Message> {
  let mut wrap = Row::new().align_y(Vertical::Center);
  if explicit {
    wrap = wrap
      .push(explicit_badge(roles))
      .push(space().width(metrics::BADGE_GAP));
  }
  for token in split_artist_tokens(&track.artist_text()) {
    match token {
      ArtistToken::Sep(separator) => {
        wrap = wrap.push(row_line(
          separator,
          size,
          fonts::UI,
          Color {
            a: 0.75,
            ..roles.variant
          },
        ));
      }
      ArtistToken::Artist(name) => {
        let open = Message::Ui(UiMessage::RowCommand(
          list,
          index,
          TrackCommand::OpenArtist(name.clone()),
        ));
        wrap = wrap.push(
          mouse_area(row_line(name, size, fonts::UI, roles.variant))
            .interaction(mouse::Interaction::Pointer)
            .on_press(open),
        );
      }
    }
  }
  wrap.clip(true).into()
}

/// Couleur du voile qui rend l'opacité d'un groupe (`m3FadeUp`) : celle de `base` sur le fond uni de la page, aucune sur une image de fond
/// ou un halo (iced n'a pas d'opacité de groupe, voir `widgets::view_transition`).
pub fn fade_veil(app: &App, base: Color) -> Option<Color> {
  // Le choix du départ ne tient plus si un halo apparaît pendant le fondu : le voile ne se peint jamais sur lui.
  (app
    .entry_motion
    .plain
    .unwrap_or_else(|| app.entry_plain(app.screen))
    && app.hero_url().is_none())
  .then_some(base)
}

/// Largeur maximale du libellé « Added by » d'une rangée de la file en jam.
const ADDED_BY_WIDTH: f32 = 140.0;

/// Ligne d'une piste (`createTrackRow`, `styles.css:1577-1780`) : numéro, vignette, titre et artiste,
/// durée, puis le bouton favori qui n'apparaît qu'au survol. Dans la page artiste, `#view-artist` change les
/// mesures (`styles.css:2034-2082`), retire le numéro et remplace le favori par le bouton « ... » toujours visible.
pub fn track_row(app: &App, list: ListId, index: usize) -> Element<'_, Message> {
  let in_artist = list == ListId::Artist;
  let pitch = if in_artist {
    artist::ROW_PITCH
  } else if list == ListId::Album {
    tokens::album_detail::ROW_HEIGHT + metrics::LIST_GAP
  } else {
    ROW_HEIGHT
  };
  let Some(track) = app.list_track(list, index) else {
    return space().height(pitch).into();
  };
  let roles = Roles::of(app);
  let platform = &app.data.platform;
  let current = app
    .player
    .current
    .as_ref()
    .is_some_and(|playing| playing.is_same(track, platform));
  let selected = !app.ui.selection.is_empty()
    && app.ui.selection_list == Some(list)
    && app.ui.selection.contains(&track.canonical_id(platform));
  let drop_target = app.ui.press.is_some_and(|press| {
    press.list == list && press.over == index && press.from != index && list.can_reorder()
  });
  let hovered = app.ui.row_hover == Some((list, index));
  // `background-color 140ms ease` : sans piste (état posé sans message, banc), le survol vaut 0 ou 1.
  let hover_mix = app
    .ui
    .row_fade
    .value((list, index), app.now, if hovered { 1.0 } else { 0.0 });
  let ui = |message: UiMessage| Message::Ui(message);
  let command = |command: TrackCommand| ui(UiMessage::RowCommand(list, index, command));

  // Mesures de la ligne : celles de `.track-row`, ou de `#view-artist .track-row` (`styles.css:2034-2068`).
  let (title_size, artist_size, duration_size, thumb, thumb_radius, pad_x, pad_y, gap, height) =
    if in_artist {
      (
        artist::TITLE_TEXT_ROW,
        artist::ARTIST_TEXT_ROW,
        artist::DURATION_TEXT_ROW,
        artist::THUMB,
        artist::THUMB_RADIUS,
        artist::ROW_PADDING_X,
        artist::ROW_PADDING_Y,
        artist::ROW_GAP,
        artist::ROW_HEIGHT,
      )
    } else {
      (
        metrics::TITLE_TEXT,
        metrics::ARTIST_TEXT,
        metrics::DURATION_TEXT,
        metrics::THUMB,
        metrics::THUMB_RADIUS,
        metrics::PADDING_X,
        metrics::PADDING_Y,
        metrics::GAP,
        if list == ListId::Album {
          tokens::album_detail::ROW_HEIGHT
        } else {
          metrics::HEIGHT
        },
      )
    };
  let number: Element<'_, Message> = if current {
    playing_bars(roles, app.bar_heights())
  } else {
    row_line(
      row_number(list, track, index).to_string(),
      metrics::NUM_TEXT,
      fonts::UI,
      roles.outline,
    )
  };
  let number = container(number)
    .width(metrics::NUM_WIDTH)
    .align_x(Horizontal::Center);
  let title_color = if current {
    roles.primary
  } else {
    roles.on_surface
  };
  // `.track-row-title` : `text-overflow: ellipsis` (`styles.css:1681`) sur la largeur de la colonne.
  let title_text = track.title_text().to_string();
  let title_height = title_size * tokens::home::H2_LINE;
  let title = responsive(move |size| {
    row_line(
      ellipsize(&title_text, title_size, fonts::MEDIUM, size.width),
      title_size,
      fonts::MEDIUM,
      title_color,
    )
  })
  .width(Fill)
  .height(title_height);
  let explicit = track.is_explicit() && app.settings.show_explicit;
  let info = column![
    title,
    row_artist(roles, track, explicit, list, index, artist_size)
  ]
  .spacing(metrics::INFO_GAP)
  .width(Fill)
  .clip(true);
  let duration = row_line(
    format_duration(track.duration.unwrap_or(0.0)),
    duration_size,
    fonts::UI,
    if in_artist {
      roles.variant
    } else {
      roles.outline
    },
  );
  let favorite = app.data.is_favorite(track);
  // `.track-row-actions` : `opacity: 0` hors survol, la place reste réservée. Page artiste : `opacity: 1`
  // et seul `.btn-row-more` reste affiché (`styles.css:2069-2081`, `app.js:11375`).
  let actions = if in_artist {
    Row::new().push(tip(
      row_icon_pill(
        roles,
        icons::MORE,
        metrics::ACTION_ICON,
        metrics::ACTION,
        command(TrackCommand::Download),
      ),
      app.i18n.t(tips::DOWNLOAD),
    ))
  } else {
    Row::with_children(
      row_actions(list, track)
        .into_iter()
        .filter(|action| shown_in_row(*action))
        .map(|action| -> Element<'_, Message> {
          if !hovered {
            return space()
              .width(metrics::ACTION)
              .height(metrics::ACTION)
              .into();
          }
          let glyph = match action {
            RowAction::Favorite if favorite => icons::FAVORITE_FILLED,
            RowAction::Favorite => icons::FAVORITE,
            _ => icons::REMOVE,
          };
          tip(
            row_icon_pill(
              roles,
              glyph,
              metrics::ACTION_ICON,
              metrics::ACTION,
              command(action.command(index)),
            ),
            app.i18n.t(row_action_key(action, favorite)),
          )
        }),
    )
  };
  let mut line = Row::new().spacing(gap).align_y(Vertical::Center);
  if !in_artist {
    line = line.push(number);
  }
  if list != ListId::Album {
    line = line.push(row_thumb(
      app,
      roles,
      app.cover_of(track).as_deref(),
      thumb,
      thumb_radius,
    ));
  }
  line = line.push(info);
  // En jam, la file dit qui a ajouté le titre, juste avant la durée.
  if list == ListId::Queue
    && let Some(by) = super::queue_panel::added_by(app, Some(index))
  {
    line = line.push(row_line(
      ellipsize(&by, duration_size, fonts::UI, ADDED_BY_WIDTH),
      duration_size,
      fonts::UI,
      roles.outline,
    ));
  }
  line = line.push(duration).push(actions);
  // `#album-tracks-list .track-row` : toujours 20 px, survol `surface-container-high` (`styles.css:8650-8655`).
  let in_album = list == ListId::Album;
  let radius = if in_album {
    tokens::album_detail::ROW_RADIUS
  } else if app.settings.classic_track_rows {
    metrics::CLASSIC_RADIUS
  } else {
    metrics::RADIUS
  };
  let background = if selected {
    Some(blend(roles.primary, roles.high, metrics::SELECTED_SHARE))
  } else if current {
    Some(roles.high)
  } else if hover_mix > 0.0 {
    // `transparent` vers la couleur : interpolation prémultipliée, la teinte reste, seule l'opacité monte.
    let target = if in_album {
      roles.high
    } else {
      roles.container
    };
    Some(Color {
      a: target.a * hover_mix,
      ..target
    })
  } else {
    None
  };
  let body = container(line)
    .padding(iced::Padding {
      top: pad_y,
      right: pad_x,
      bottom: pad_y,
      left: pad_x,
    })
    .height(height)
    .width(Fill)
    .style(move |_| container::Style {
      background: background.map(Background::Color),
      border: if drop_target {
        Border {
          color: roles.primary,
          width: 2.0,
          radius: radius.into(),
        }
      } else {
        border::rounded(radius)
      },
      ..container::Style::default()
    });
  let area = mouse_area(body)
    .on_press(ui(UiMessage::RowPressed(list, index)))
    .on_enter(ui(UiMessage::RowEntered(list, index)))
    .on_exit(ui(UiMessage::RowLeft(list, index)))
    .on_release(ui(UiMessage::RowReleased));
  let entry = focusable(area)
    .on_activate(Some(ui(UiMessage::RowActivated(list, index))))
    .on_context(move |position| Message::Ui(UiMessage::RowMenu(list, index, position)))
    .ripple_key(ControlKey::new((list, index)), roles.on_surface, radius)
    .radius(radius);
  // `m3FadeUp` de `.track-row` : opacité seule (voir `tokens::fade_up::ROW_OFFSET`), rang = `:nth-child` de la ligne.
  let entry: Element<'_, Message> = view_transition(
    entry,
    app.entry_motion.row_pose(index, app.now),
    fade_veil(app, roles.page),
  )
  .into();
  column![entry, space().height(metrics::LIST_GAP)]
    .height(pitch)
    .into()
}

/// Liste virtualisée de pistes.
pub fn track_list(app: &App, list: ListId) -> Element<'_, Message> {
  app.ui.list(list).view(
    list.scroll_id(),
    app.list_len(list),
    move |viewport| {
      Message::Ui(UiMessage::ListScrolled(
        list,
        viewport.absolute_offset().y,
        viewport.bounds().height,
      ))
    },
    move |index| track_row(app, list, index),
  )
}

/// Barre de sélection (`updateBulkSelectBar`).
pub fn selection_bar(app: &App) -> Option<Element<'_, Message>> {
  if app.ui.selection.is_empty() {
    return None;
  }
  let ui = |message: UiMessage| Some(Message::Ui(message));
  let mut bar = row![
    text(format!("{} selected", app.ui.selection.len()))
      .size(13)
      .width(Fill),
    crate::widgets::button::text_button(app.i18n.t("Select all"), ui(UiMessage::SelectAll)),
    crate::widgets::button::text_button(
      app.i18n.t("Add to playlist"),
      ui(UiMessage::SelectionToPlaylist)
    ),
    crate::widgets::button::text_button(
      app.i18n.t("Add to favorites"),
      ui(UiMessage::SelectionToFavorites)
    ),
  ]
  .spacing(8)
  .align_y(iced::alignment::Vertical::Center);
  if matches!(
    app.ui.selection_list,
    Some(ListId::Local | ListId::Playlist | ListId::Queue)
  ) {
    bar = bar.push(crate::widgets::button::text_button(
      app.i18n.t("Remove"),
      ui(UiMessage::SelectionRemove),
    ));
  }
  bar = bar.push(crate::widgets::button::text_button(
    app.i18n.t("Cancel"),
    ui(UiMessage::ClearSelection),
  ));
  Some(
    container(bar)
      .padding([6, 16])
      .width(Fill)
      .style(|theme: &Theme| {
        container::Style::default().background(theme.extended_palette().background.weak.color)
      })
      .into(),
  )
}

/// `.main-content` : page qui défile d'un bloc, avec le remplissage du panneau central. La barre de
/// 5 px ne prend sa place que si la page déborde (mesuré : bord droit à 1572 px sur `04e`, 1568 px sur `03`).
pub(super) fn main_page<'a>(
  roles: Roles,
  content: Element<'a, Message>,
  id: &'static str,
  overflow: bool,
) -> scrollable::Scrollable<'a, Message> {
  use tokens::home as page;
  let right = page::MAIN_RIGHT + if overflow { page::SCROLLBAR } else { 0.0 };
  scrollable(container(content).padding(iced::Padding {
    top: page::MAIN_TOP,
    right,
    bottom: page::MAIN_BOTTOM,
    left: page::MAIN_LEFT,
  }))
  .id(id)
  .direction(scrollable::Direction::Vertical(
    scrollable::Scrollbar::new()
      .width(page::SCROLLBAR)
      .scroller_width(page::SCROLLBAR)
      .margin(0),
  ))
  .style(move |_, status| super::home::page_scroll_style(roles, status))
  .width(Fill)
  .height(Fill)
}

/// Colonne de lignes simples pour la barre latérale.
pub fn plain_column<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
  Column::from_vec(items).spacing(2).width(Fill).into()
}

#[cfg(test)]
mod tests {
  use super::*;
  use RowAction::*;

  fn remote() -> Track {
    Track {
      platform: Some("ytmusic".to_string()),
      ..Track::default()
    }
  }

  fn local() -> Track {
    Track {
      platform: Some("local".to_string()),
      ..Track::default()
    }
  }

  #[test]
  fn a_remote_row_ends_with_the_download_button() {
    assert_eq!(
      row_actions(ListId::Search, &remote()),
      [Share, Favorite, Art, AddToPlaylist, Download]
    );
  }

  #[test]
  fn a_local_row_removes_the_file_and_has_no_download_button() {
    for list in [ListId::Local, ListId::Favorites] {
      let actions = row_actions(list, &local());
      assert_eq!(actions, [Share, Favorite, Art, AddToPlaylist, RemoveLocal]);
    }
    assert!(!row_actions(ListId::Local, &remote()).contains(&Download));
  }

  #[test]
  fn a_playlist_row_ends_with_its_own_removal() {
    assert_eq!(
      row_actions(ListId::Playlist, &remote()),
      [
        Share,
        Favorite,
        Art,
        AddToPlaylist,
        Download,
        RemoveFromPlaylist
      ]
    );
    assert_eq!(
      row_actions(ListId::Playlist, &local()),
      [
        Share,
        Favorite,
        Art,
        AddToPlaylist,
        RemoveLocal,
        RemoveFromPlaylist
      ]
    );
  }

  #[test]
  fn a_queue_row_ends_with_the_removal_from_the_queue() {
    assert_eq!(
      row_actions(ListId::Queue, &remote()),
      [
        Share,
        Favorite,
        Art,
        AddToPlaylist,
        Download,
        RemoveFromQueue
      ]
    );
    assert_eq!(RemoveFromQueue.command(3), TrackCommand::RemoveFromQueue(3));
  }

  #[test]
  fn each_row_button_sends_its_command() {
    assert_eq!(Share.command(0), TrackCommand::Share);
    assert_eq!(Art.command(0), TrackCommand::Art);
    assert_eq!(Download.command(0), TrackCommand::Download);
    assert_eq!(RemoveLocal.command(0), TrackCommand::RemoveLocal);
  }

  #[test]
  fn only_the_favorite_and_queue_removal_buttons_stay_on_a_row() {
    for action in [
      Share,
      Art,
      AddToPlaylist,
      Download,
      RemoveLocal,
      RemoveFromPlaylist,
    ] {
      assert!(!shown_in_row(action), "{action:?}");
    }
    assert!(shown_in_row(Favorite) && shown_in_row(RemoveFromQueue));
  }

  #[test]
  fn a_row_is_numbered_from_one_and_an_album_row_by_its_track_number() {
    let mut track = remote();
    assert_eq!(row_number(ListId::Search, &track, 0), 1);
    assert_eq!(row_number(ListId::Album, &track, 4), 5);
    track.extra.insert("no".to_string(), 9.into());
    assert_eq!(row_number(ListId::Album, &track, 4), 9);
    assert_eq!(row_number(ListId::Search, &track, 4), 5);
  }
}
