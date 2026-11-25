//! Barre latérale dépliée de Riff 44.1.1 : bannière, onglets, bibliothèque et barre d'icônes du bas
//! (`index.html:393-516`, `styles.css:11203-11700`). Les dimensions de Riff sont en pixels CSS :
//! toutes passent par [`tokens::css`].
//!
//! Dans cette disposition Riff masque les entrées `nav-item` (Liked songs, Albums, My files,
//! History, Queue, Settings). Leurs destinations restent atteignables : voir [`screen_link`].

use super::Screen;
use super::chrome::tip;
use super::common::playlist_cover_url;
use super::right_panel::{ellipsize, text_width};
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_ROW};
use crate::theme::tokens::{self, css as z, sidebar as sb};
use crate::theme::{fonts, icons};
use crate::ui::{ListId, PlayerMessage, SidebarTab, TrackCommand, UiMessage};
use crate::update::tab_title;
use crate::user_data::Track;
use crate::widgets::focus::focusable;
use crate::widgets::view_transition::view_transition;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
  Column, Row, button, column, container, image, mouse_area, responsive, row, scrollable, space,
  stack, text,
};
use iced::{
  Background, Color, ContentFit, Element, Fill, Font, Gradient, Padding, Point, Radians, Theme,
  border, gradient,
};
use material_colors::color::Rgb;

/// Rôles de couleur de Riff utilisés par la barre (`--md-sys-color-*`).
#[derive(Clone, Copy)]
struct Roles {
  on_surface: Color,
  variant: Color,
  outline: Color,
  outline_variant: Color,
  secondary_container: Color,
  primary: Color,
  on_primary: Color,
  highest: Color,
  high: Color,
}

pub(super) fn rgb(color: Rgb) -> Color {
  Color::from_rgb8(color.red, color.green, color.blue)
}

impl Roles {
  fn of(app: &App) -> Self {
    let s = &app.theme.scheme;
    Self {
      on_surface: rgb(s.on_surface),
      variant: rgb(s.on_surface_variant),
      outline: rgb(s.outline),
      outline_variant: rgb(s.outline_variant),
      secondary_container: rgb(s.secondary_container),
      primary: rgb(s.primary),
      on_primary: rgb(s.on_primary),
      highest: rgb(s.surface_container_highest),
      high: rgb(s.surface_container_high),
    }
  }
}

/// `color-mix(in srgb, a share%, b)`.
fn mix(a: Color, b: Color, share: f32) -> Color {
  Color::from_rgb(
    a.r * share + b.r * (1.0 - share),
    a.g * share + b.g * (1.0 - share),
    a.b * share + b.b * (1.0 - share),
  )
}

pub(super) fn faded(color: Color, alpha: f32) -> Color {
  Color {
    a: color.a * alpha,
    ..color
  }
}

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

/// Angle CSS vers angle d'iced : les deux comptent depuis le haut, dans le sens horaire
/// (`Radians::to_distance`, `iced_core` 0.14.0 : direction `(sin a, -cos a)`).
fn css_angle(degrees: f32) -> Radians {
  Radians(degrees.to_radians())
}

/// Couleurs de la bannière `#nav-discover`. Riff les calcule sur l'icône d'application
/// (`updateHomeBannerColorsFromIcon`, `app.js:16601`) ; ce sont les teintes mesurées sur la capture
/// `01-accueil.png` pour l'icône par défaut `02-editor.svg`, fond sombre `::after` compris.
const BANNER_STOPS: [(f32, Color); 3] = [
  (0.0, Color::from_rgb8(82, 52, 53)),
  (0.5, Color::from_rgb8(83, 57, 61)),
  (1.0, Color::from_rgb8(118, 64, 64)),
];

fn banner_style(_: &Theme, _: button::Status) -> button::Style {
  let mut linear = gradient::Linear::new(css_angle(120.0));
  for (offset, color) in BANNER_STOPS {
    linear = linear.add_stop(offset, color);
  }
  button::Style {
    background: Some(Background::Gradient(Gradient::Linear(linear))),
    text_color: Color::WHITE,
    border: border::rounded(z(20.0)),
    ..button::Style::default()
  }
}

/// Motif flottant `.sb-home-float-logo` : blanc à 22 %, multiplié par l'opacité propre du motif.
fn float_logo<'a>(
  size: f32,
  opacity: f32,
  vertical: Vertical,
  edge: f32,
  right: f32,
) -> Element<'a, Message> {
  let (top, bottom) = match vertical {
    Vertical::Bottom => (0.0, edge),
    _ => (edge, 0.0),
  };
  let logo = container(icons::glyph_px(icons::BANNER_LOGO, size))
    .style(move |_| container::Style::default().color(faded(Color::WHITE, 0.22 * opacity)));
  container(logo)
    .width(Fill)
    .height(Fill)
    .align_x(Horizontal::Right)
    .align_y(vertical)
    .padding(Padding {
      top: z(top),
      right: z(right),
      bottom: z(bottom),
      left: 0.0,
    })
    .into()
}

/// Carte « Riff » : logo, nom et pastille Radio. Un clic ouvre l'accueil, la pastille lance la radio.
fn banner(app: &App) -> Element<'_, Message> {
  let radio = button(
    container(
      row![
        icons::glyph_px(icons::RADIO, 14.0),
        text(app.i18n.t("Radio"))
          .size(sb::BANNER_ACTION_TEXT)
          .font(fonts::SEMIBOLD)
      ]
      .spacing(z(6.0))
      .align_y(Vertical::Center),
    )
    .center_y(Fill),
  )
  .height(z(28.0))
  .padding([0.0, z(12.0)])
  .style(|_, status| button::Style {
    background: Some(Background::Color(Color::from_rgba(
      1.0,
      1.0,
      1.0,
      if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        0.34
      } else {
        0.22
      },
    ))),
    text_color: Color::WHITE,
    border: border::rounded(999.0),
    ..button::Style::default()
  })
  .on_press(Message::Player(PlayerMessage::PlayMyWave));
  let title = text(app.i18n.t("Riff"))
    .size(sb::BANNER_TITLE_TEXT)
    .font(fonts::BOLD)
    .color(Color::WHITE);
  let content = row![
    icons::glyph_px(icons::HOME, 58.0),
    column![title, radio].spacing(z(6.0)).padding(Padding {
      left: z(14.0),
      ..Padding::ZERO
    })
  ]
  .align_y(Vertical::Center)
  .height(Fill)
  .padding(Padding {
    left: z(18.0),
    ..Padding::ZERO
  });
  let layers = stack![
    float_logo(28.0, 0.26, Vertical::Top, 14.0, 22.0),
    float_logo(19.0, 0.18, Vertical::Bottom, 14.0, 64.0),
    float_logo(34.0, 0.15, Vertical::Bottom, 0.0, 102.0),
    float_logo(16.0, 0.2, Vertical::Top, 20.0, 138.0),
    content,
  ];
  let message = Message::Navigate(Screen::Home);
  focusable(
    button(layers)
      .padding(0)
      .width(Fill)
      .height(z(96.0))
      .style(banner_style)
      .on_press(message.clone()),
  )
  .on_activate(Some(message))
  .radius(z(20.0))
  .into()
}

/// Identifiant du flux défilant : le changement d'onglet le remet en haut.
pub const FEED_ID: &str = "sidebar-library-feed";

/// Onglet `.sb-horiz-tab` : fond au survol et actif, soulignement de 32 x 3 px sous l'actif.
fn tab(app: &App, c: Roles, value: SidebarTab, padding_x: f32) -> Element<'_, Message> {
  let active = app.ui.sidebar_tab == value;
  let label = text(app.i18n.t(tab_title(value)))
    .size(sb::TAB_TEXT)
    .font(if active {
      fonts::SEMIBOLD
    } else {
      fonts::MEDIUM
    })
    .wrapping(Wrapping::None);
  let underline: Element<'_, Message> = if active {
    container(space())
      .width(sb::TAB_UNDERLINE_WIDTH)
      .height(sb::TAB_UNDERLINE_HEIGHT)
      .style(move |_| {
        container::Style::default()
          .background(c.on_surface)
          .border(border::Border::default().rounded(border::top(sb::TAB_UNDERLINE_HEIGHT)))
      })
      .into()
  } else {
    space().into()
  };
  // La pile prend la largeur de sa première couche (le libellé, `Shrink`) : l'onglet suit son texte.
  let body = stack![
    container(label).center_y(Fill),
    container(underline)
      .width(Fill)
      .height(Fill)
      .align_x(Horizontal::Center)
      .align_y(Vertical::Bottom)
  ]
  .height(Fill);
  let message = ui(UiMessage::SidebarTab(value));
  focusable(
    button(body)
      .padding([0.0, padding_x])
      .height(sb::TAB_HEIGHT)
      .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
          background: if active {
            Some(Background::Color(faded(c.on_surface, sb::TAB_ACTIVE_SHARE)))
          } else if hovered {
            Some(Background::Color(faded(c.on_surface, sb::TAB_HOVER_SHARE)))
          } else {
            None
          },
          text_color: if active || hovered {
            c.on_surface
          } else {
            c.variant
          },
          border: border::Border::default().rounded(border::top(sb::TAB_RADIUS)),
          ..button::Style::default()
        }
      })
      .on_press(message.clone()),
  )
  .on_activate(Some(message))
  .radius(sb::TAB_RADIUS)
  .into()
}

const TABS: [SidebarTab; 3] = [SidebarTab::Playlists, SidebarTab::Albums, SidebarTab::Queue];

/// Padding horizontal des onglets : `TAB_PADDING_X`, réduit quand les libellés (`texts` : somme des
/// largeurs en graisse active) laissent moins de place dans la barre de largeur `width`.
fn tabs_padding_x(texts: f32, width: f32) -> f32 {
  let available = width - 2.0 * z(12.0) - 2.0 * sb::TABS_PADDING_X - 2.0 * sb::TABS_GAP;
  ((available - texts) / (2.0 * TABS.len() as f32)).clamp(0.0, sb::TAB_PADDING_X)
}

fn rule<'a>(color: Color) -> Element<'a, Message> {
  container(space())
    .width(Fill)
    .height(1)
    .style(move |_| container::Style::default().background(color))
    .into()
}

/// Dimensions d'une rangée : normales (Playlists, Albums) ou compactes (onglet Queue).
#[derive(Clone, Copy)]
struct Dim {
  cover: f32,
  cover_radius: f32,
  padding_y: f32,
  gap: f32,
  title: f32,
  sub: f32,
}

impl Dim {
  const NORMAL: Self = Self {
    cover: sb::COVER,
    cover_radius: sb::COVER_RADIUS,
    padding_y: sb::ROW_PADDING_Y,
    gap: sb::ROW_GAP,
    title: sb::TITLE_TEXT,
    sub: sb::SUB_TEXT,
  };
  const QUEUE: Self = Self {
    cover: sb::QUEUE_COVER,
    cover_radius: sb::QUEUE_COVER_RADIUS,
    padding_y: sb::QUEUE_ROW_PADDING_Y,
    gap: sb::QUEUE_ROW_GAP,
    title: sb::QUEUE_TITLE_TEXT,
    sub: sb::QUEUE_SUB_TEXT,
  };
}

/// Pochette de 60 px (`.sb-lib-cover`, 46 px en `Dim::QUEUE`) : image, ou fond `surface-container-high` et icône.
fn art<'a>(
  app: &App,
  c: Roles,
  dim: Dim,
  url: Option<&str>,
  fallback: Option<&'static str>,
) -> Element<'a, Message> {
  if let Some(handle) = url.and_then(|url| app.images.get(&ImageKey::new(url, SIZE_ROW))) {
    return image(handle.clone())
      .width(dim.cover)
      .height(dim.cover)
      .content_fit(ContentFit::Cover)
      .border_radius(dim.cover_radius)
      .into();
  }
  let glyph: Element<'a, Message> = match (url, fallback) {
    (None, Some(glyph)) => icons::glyph_px(glyph, sb::COVER_ICON),
    _ => space().into(),
  };
  tile(dim, glyph, c.on_surface, Background::Color(c.high))
}

fn tile<'a>(
  dim: Dim,
  content: Element<'a, Message>,
  color: Color,
  background: Background,
) -> Element<'a, Message> {
  container(content)
    .center(dim.cover)
    .style(move |_| container::Style {
      background: Some(background),
      text_color: Some(color),
      border: border::rounded(dim.cover_radius),
      ..container::Style::default()
    })
    .into()
}

/// Dégradé du cœur de « Liked songs » (`app.js:27322`), repris par l'affiche par défaut de la page des titres aimés.
pub(super) fn liked_background(primary: Color) -> Background {
  let end = mix(primary, Color::WHITE, sb::LIKED_END_SHARE);
  let linear = gradient::Linear::new(css_angle(135.0))
    .add_stop(0.0, primary)
    .add_stop(1.0, end);
  Background::Gradient(Gradient::Linear(linear))
}

/// Pochette de « Liked songs » : l'image choisie si elle est chargée, sinon le cœur sur le dégradé.
fn liked_art<'a>(app: &App, c: Roles) -> Element<'a, Message> {
  let custom = app.data.liked_custom_cover();
  if custom.is_some_and(|url| app.images.get(&ImageKey::new(url, SIZE_ROW)).is_some()) {
    return art(app, c, Dim::NORMAL, custom, None);
  }
  tile(
    Dim::NORMAL,
    icons::glyph_px(icons::LIKED_HEART, sb::COVER_ICON_LARGE),
    c.on_primary,
    liked_background(c.primary),
  )
}

fn local_art<'a>(c: Roles) -> Element<'a, Message> {
  tile(
    Dim::NORMAL,
    icons::glyph_px(icons::SB_FOLDER, sb::COVER_ICON_LARGE),
    c.primary,
    Background::Color(mix(c.primary, c.highest, sb::LOCAL_SHARE)),
  )
}

/// Ligne de texte rognée par « … » à la largeur de la colonne (`text-overflow: ellipsis`).
fn line<'a>(
  content: String,
  size: f32,
  line_height: f32,
  font: Font,
  color: Color,
) -> Element<'a, Message> {
  responsive(move |bounds| {
    text(ellipsize(&content, size, font, bounds.width))
      .size(size)
      .font(font)
      .line_height(LineHeight::Relative(line_height))
      .color(color)
      .wrapping(Wrapping::None)
      .into()
  })
  .width(Fill)
  .height(size * line_height)
  .into()
}

/// `.sb-lib-sub` : ligne de détail, précédée de l'épingle pour les entrées épinglées. Avec l'épingle
/// le détail est un enfant du conteneur flex : il est coupé par la colonne, sans « … ».
fn sub<'a>(c: Roles, dim: Dim, value: String, pinned: bool) -> Element<'a, Message> {
  if !pinned {
    return line(value, dim.sub, sb::SUB_LINE, fonts::UI, c.variant);
  }
  let pin = container(icons::glyph_px(icons::PIN, sb::PIN_ICON))
    .style(move |_| container::Style::default().color(c.primary));
  let detail = text(value)
    .size(dim.sub)
    .line_height(LineHeight::Relative(sb::SUB_LINE))
    .color(c.variant)
    .wrapping(Wrapping::None);
  row![pin, detail]
    .spacing(sb::SUB_GAP)
    .align_y(Vertical::Center)
    .into()
}

/// `.sb-lib-row-action` : bouton rond de 28 px, invisible hors survol de la rangée mais qui garde sa place.
fn action_slot<'a>(
  app: &App,
  c: Roles,
  row_hovered: bool,
  message: Message,
) -> Element<'a, Message> {
  if !row_hovered {
    return space().width(sb::ACTION).height(sb::ACTION).into();
  }
  let entry = button(container(icons::glyph_px(icons::CLOSE, sb::ACTION_ICON)).center(Fill))
    .padding(0)
    .width(sb::ACTION)
    .height(sb::ACTION)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: hovered.then_some(Background::Color(c.highest)),
        text_color: if hovered { c.on_surface } else { c.variant },
        border: border::rounded(999.0),
        ..button::Style::default()
      }
    })
    .on_press(message);
  tip(entry.into(), app.i18n.t("Remove"))
}

/// Contenu d'une rangée `.sb-lib-row`.
struct Entry<'a> {
  cover: Element<'a, Message>,
  title: String,
  detail: Element<'a, Message>,
  active: bool,
  playing: bool,
  open: Message,
  /// Bouton de la croix (`onAction`) : retrait de l'épingle ou de la file.
  action: Option<Message>,
  context: Option<Box<dyn Fn(Point) -> Message + 'a>>,
}

/// `.sb-lib-row` : 76 px, rayon 16, actif en `secondary-container`, survol à 8 %. `index` est le rang
/// de la rangée dans le flux, qui sert à retrouver celle sous le pointeur ; `rank` est son `--i` (`app.js:27452`) : le rang parmi les
/// rangées seules, sans les étiquettes de section, qui règle le décalage de `sbRowFadeIn`.
fn lib_row<'a>(
  app: &App,
  c: Roles,
  dim: Dim,
  index: usize,
  rank: usize,
  entry: Entry<'a>,
) -> Element<'a, Message> {
  let hovered = app.ui.sidebar_hover == Some(index);
  let title = line(
    entry.title,
    dim.title,
    sb::TITLE_LINE,
    fonts::SEMIBOLD,
    if entry.playing {
      c.primary
    } else {
      c.on_surface
    },
  );
  let info = column![title, entry.detail]
    .spacing(sb::INFO_GAP)
    .width(Fill)
    .clip(true);
  let mut content = Row::new()
    .push(entry.cover)
    .push(info)
    .spacing(dim.gap)
    .align_y(Vertical::Center);
  if let Some(message) = entry.action {
    content = content.push(action_slot(app, c, hovered, message));
  }
  let active = entry.active;
  let body = button(content)
    .padding(Padding {
      top: dim.padding_y,
      right: sb::ROW_PADDING_X,
      bottom: dim.padding_y,
      left: sb::ROW_PADDING_X,
    })
    .width(Fill)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: if active {
          Some(Background::Color(c.secondary_container))
        } else if hovered {
          Some(Background::Color(faded(c.on_surface, sb::ROW_HOVER_SHARE)))
        } else {
          None
        },
        text_color: c.on_surface,
        border: border::rounded(sb::ROW_RADIUS),
        ..button::Style::default()
      }
    })
    .on_press(entry.open.clone());
  let area = mouse_area(body)
    .on_enter(ui(UiMessage::SidebarRowEntered(index)))
    .on_exit(ui(UiMessage::SidebarRowLeft(index)));
  let focus = focusable(area)
    .on_activate(Some(entry.open))
    .radius(sb::ROW_RADIUS);
  let row: Element<'a, Message> = match entry.context {
    Some(context) => focus.on_context(context).into(),
    None => focus.into(),
  };
  // `sbRowFadeIn` : le fondu se rend par un voile du fond de la barre, qui ne porte ni image ni halo.
  let sidebar_bg = app.theme.theme.extended_palette().background.weaker.color;
  view_transition(
    row,
    app.entry_motion.sidebar_pose(rank, app.now),
    Some(sidebar_bg),
  )
  .centered()
  .spill(sb::ENTER_OFFSET)
  .into()
}

/// `.sb-lib-section-label`, ou lien discret vers un écran quand `target` est donné.
fn section_label<'a>(c: Roles, label: String, target: Option<Screen>) -> Element<'a, Message> {
  let content = text(label.to_uppercase())
    .size(sb::LABEL_TEXT)
    .font(fonts::BOLD)
    .wrapping(Wrapping::None);
  let padding = Padding {
    top: sb::LABEL_PADDING_TOP,
    right: sb::LABEL_PADDING_X,
    bottom: sb::LABEL_PADDING_BOTTOM,
    left: sb::LABEL_PADDING_X,
  };
  let Some(screen) = target else {
    return container(content.color(c.outline)).padding(padding).into();
  };
  let message = Message::Navigate(screen);
  focusable(
    button(content)
      .padding(padding)
      .style(move |_, status| button::Style {
        text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
          c.on_surface
        } else {
          c.outline
        },
        ..button::Style::default()
      })
      .on_press(message.clone()),
  )
  .on_activate(Some(message))
  .radius(tokens::radius::S)
  .into()
}

/// Accès discret à un écran que Riff ne montre plus dans cette disposition (`nav-item` masqués).
fn screen_link<'a>(app: &App, c: Roles, screen: Screen) -> Element<'a, Message> {
  let title = if screen == Screen::Liked {
    app.liked_title()
  } else {
    app.i18n.t(screen.title_key())
  };
  section_label(c, title, Some(screen))
}

/// `.empty-hint` du flux : texte centré, couleur `outline`.
fn empty_hint<'a>(c: Roles, label: String) -> Element<'a, Message> {
  container(
    text(label)
      .size(sb::HINT_TEXT)
      .color(c.outline)
      .width(Fill)
      .align_x(Horizontal::Center),
  )
  .padding([sb::HINT_PADDING_Y, sb::HINT_PADDING_X])
  .into()
}

/// Carte de piste (`makeTrackCard`) : un clic joue le titre seul. `list` désigne la rangée pour le menu
/// contextuel, `action` la croix qui apparaît au survol.
fn track_row<'a>(
  app: &App,
  c: Roles,
  (dim, index, rank): (Dim, usize, usize),
  track: &Track,
  pinned: bool,
  list: Option<(ListId, usize)>,
  action: Option<Message>,
) -> Element<'a, Message> {
  let playing = app
    .player
    .current
    .as_ref()
    .is_some_and(|current| current.is_same(track, &app.data.platform));
  let artist = if track.artist_text().is_empty() {
    "Unknown artist".to_string()
  } else {
    track.artist_text()
  };
  let context = list.map(|(list, at)| -> Box<dyn Fn(Point) -> Message + 'a> {
    Box::new(move |position| ui(UiMessage::RowMenu(list, at, position)))
  });
  lib_row(
    app,
    c,
    dim,
    index,
    rank,
    Entry {
      cover: art(
        app,
        c,
        dim,
        app.cover_of(track).as_deref(),
        Some(icons::MUSIC_NOTE),
      ),
      title: track.title_text().to_string(),
      detail: sub(c, dim, artist, pinned),
      active: false,
      playing,
      open: ui(UiMessage::SidebarPlayTrack(Box::new(track.clone()))),
      action,
      context,
    },
  )
}

fn plural(count: usize, one: &str, many: &str) -> String {
  format!("{count} {}", if count == 1 { one } else { many })
}

fn playlists_feed<'a>(app: &'a App, c: Roles, rows: &mut Vec<Element<'a, Message>>) {
  rows.push(lib_row(
    app,
    c,
    Dim::NORMAL,
    rows.len(),
    rows.len(),
    Entry {
      cover: liked_art(app, c),
      title: app.liked_title(),
      detail: sub(
        c,
        Dim::NORMAL,
        plural(app.data.favorites.len(), "song", "songs"),
        true,
      ),
      active: app.screen == Screen::Liked,
      playing: false,
      open: Message::Navigate(Screen::Liked),
      action: None,
      context: None,
    },
  ));
  for (at, track) in app.data.sidebar_pinned_tracks.iter().enumerate() {
    let unpin = ui(UiMessage::Track(
      Box::new(track.clone()),
      TrackCommand::PinToSidebar,
    ));
    rows.push(track_row(
      app,
      c,
      (Dim::NORMAL, rows.len(), rows.len()),
      track,
      true,
      Some((ListId::Pinned, at)),
      Some(unpin),
    ));
  }
  for playlist in &app.data.playlists {
    let Some(id) = playlist.id.clone() else {
      continue;
    };
    let active =
      app.screen == Screen::PlaylistDetail && app.ui.playlist_id.as_deref() == Some(id.as_str());
    rows.push(lib_row(
      app,
      c,
      Dim::NORMAL,
      rows.len(),
      rows.len(),
      Entry {
        cover: art(
          app,
          c,
          Dim::NORMAL,
          playlist_cover_url(playlist).as_deref(),
          None,
        ),
        title: playlist
          .name
          .clone()
          .unwrap_or_else(|| "Untitled".to_string()),
        detail: sub(
          c,
          Dim::NORMAL,
          plural(playlist.tracks.len(), "track", "tracks"),
          false,
        ),
        active,
        playing: false,
        open: ui(UiMessage::OpenPlaylist(id.clone())),
        action: None,
        context: Some(Box::new(move |position| {
          ui(UiMessage::PlaylistMenu(id.clone(), position))
        })),
      },
    ));
  }
  rows.push(screen_link(app, c, Screen::Playlists));
  if app.dev.is_some() {
    rows.push(screen_link(app, c, Screen::Dev));
  }
}

fn albums_feed<'a>(app: &'a App, c: Roles, rows: &mut Vec<Element<'a, Message>>) {
  if app.settings.show_local {
    rows.push(lib_row(
      app,
      c,
      Dim::NORMAL,
      rows.len(),
      rows.len(),
      Entry {
        cover: local_art(c),
        title: app.i18n.t("My files"),
        detail: sub(
          c,
          Dim::NORMAL,
          plural(app.data.local_files.len(), "file", "files"),
          false,
        ),
        active: app.screen == Screen::Library,
        playing: false,
        open: Message::Navigate(Screen::Library),
        action: None,
        context: None,
      },
    ));
  }
  if app.data.saved_albums.is_empty() {
    rows.push(empty_hint(c, app.i18n.t("No saved albums yet")));
  }
  for (at, album) in app.data.saved_albums.iter().enumerate() {
    rows.push(lib_row(
      app,
      c,
      Dim::NORMAL,
      rows.len(),
      rows.len(),
      Entry {
        cover: art(
          app,
          c,
          Dim::NORMAL,
          album.cover.as_deref(),
          Some(icons::ALBUM),
        ),
        title: album.title.clone().unwrap_or_else(|| "Album".to_string()),
        detail: sub(
          c,
          Dim::NORMAL,
          album.artist.clone().unwrap_or_default(),
          false,
        ),
        active: false,
        playing: false,
        open: ui(UiMessage::OpenSavedAlbum(at)),
        action: None,
        context: None,
      },
    ));
  }
  if app.settings.show_albums {
    rows.push(screen_link(app, c, Screen::Albums));
  }
}

fn queue_feed<'a>(app: &'a App, c: Roles, rows: &mut Vec<Element<'a, Message>>) {
  let upcoming: Vec<(usize, &Track)> = app
    .queue_panel_upcoming()
    .into_iter()
    .take(sb::UPCOMING_SHOWN)
    .collect();
  let recent = &app.data.history[..app.data.history.len().min(sb::RECENT_SHOWN)];
  // `--i` ne compte que les rangées : les étiquettes de section n'avancent pas le rang.
  let mut rank = 0;
  if let Some(current) = app.player.current.as_ref() {
    rows.push(section_label(c, app.i18n.t("Now playing"), None));
    let list = app.player.index.map(|index| (ListId::Queue, index));
    rows.push(track_row(
      app,
      c,
      (Dim::QUEUE, rows.len(), rank),
      current,
      false,
      list,
      None,
    ));
    rank += 1;
  }
  if !upcoming.is_empty() {
    rows.push(section_label(c, app.i18n.t("Next up"), None));
    for &(at, track) in &upcoming {
      let remove = ui(UiMessage::RowCommand(
        ListId::Queue,
        at,
        TrackCommand::RemoveFromQueue(at),
      ));
      rows.push(track_row(
        app,
        c,
        (Dim::QUEUE, rows.len(), rank),
        track,
        false,
        Some((ListId::Queue, at)),
        Some(remove),
      ));
      rank += 1;
    }
  }
  if !recent.is_empty() {
    rows.push(section_label(c, app.i18n.t("Recently played"), None));
    for (at, track) in recent.iter().enumerate() {
      rows.push(track_row(
        app,
        c,
        (Dim::QUEUE, rows.len(), rank),
        track,
        false,
        Some((ListId::History, at)),
        None,
      ));
      rank += 1;
    }
  }
  if app.player.current.is_none() && upcoming.is_empty() && recent.is_empty() {
    rows.push(empty_hint(c, app.i18n.t("Queue is empty")));
  }
  rows.push(screen_link(app, c, Screen::Queue));
  rows.push(screen_link(app, c, Screen::History));
}

/// `.fade-top` : le masque du haut n'existe que si le flux a défilé de plus de 6 px (`app.js:27454`).
fn fade_top_visible(offset: f32) -> bool {
  offset > sb::FADE_TOP_SCROLL
}

/// Masque de `#sidebar-library-feed` : dégradé du fond de la barre vers la transparence.
fn fade<'a>(background: Color, top: bool) -> Element<'a, Message> {
  let clear = Color {
    a: 0.0,
    ..background
  };
  // `to-bottom` : 180deg.
  let linear = gradient::Linear::new(css_angle(180.0));
  let linear = if top {
    linear.add_stop(0.0, background).add_stop(1.0, clear)
  } else {
    linear.add_stop(0.0, clear).add_stop(1.0, background)
  };
  let band = container(space())
    .width(Fill)
    .height(if top { sb::FADE_TOP } else { sb::FADE_BOTTOM })
    .style(move |_| container::Style::default().background(Gradient::Linear(linear)));
  container(band)
    .width(Fill)
    .height(Fill)
    .align_y(if top { Vertical::Top } else { Vertical::Bottom })
    .into()
}

/// `.sidebar-library-feed` : colonne défilante sans barre, espacement 6, marges 6 2 14 2, fondu en bas.
fn feed(app: &App, c: Roles) -> Element<'_, Message> {
  let mut rows: Vec<Element<'_, Message>> = Vec::new();
  match app.ui.sidebar_tab {
    SidebarTab::Playlists => playlists_feed(app, c, &mut rows),
    SidebarTab::Albums => albums_feed(app, c, &mut rows),
    SidebarTab::Queue => queue_feed(app, c, &mut rows),
  }
  let list = scrollable(
    Column::with_children(rows)
      .spacing(sb::FEED_GAP)
      .width(Fill)
      .padding(Padding {
        top: sb::FEED_PADDING_TOP,
        right: sb::FEED_PADDING_X,
        bottom: sb::FEED_PADDING_BOTTOM,
        left: sb::FEED_PADDING_X,
      }),
  )
  .id(FEED_ID)
  .direction(Direction::Vertical(
    Scrollbar::new().width(0).scroller_width(0),
  ))
  .on_scroll(|viewport| ui(UiMessage::SidebarFeedScrolled(viewport.absolute_offset().y)))
  .height(Fill);
  let background = app.theme.theme.extended_palette().background.weaker.color;
  let mut layers = stack![list, fade(background, false)];
  if fade_top_visible(app.ui.sidebar_scroll) {
    layers = layers.push(fade(background, true));
  }
  layers.height(Fill).into()
}

/// `.sb-bottom-icon-btn` : 40 px, rayon 12 ; l'écran actif passe en pastille `primary`.
fn bottom_button<'a>(
  app: &App,
  c: Roles,
  glyph: &'static str,
  label: &str,
  message: Message,
  active: bool,
) -> Element<'a, Message> {
  let entry = button(container(icons::glyph_px(glyph, 22.0)).center(Fill))
    .padding(0)
    .width(Fill)
    .height(z(40.0))
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      if active {
        return button::Style {
          background: Some(Background::Color(c.primary)),
          text_color: c.on_primary,
          border: border::rounded(9999.0),
          ..button::Style::default()
        };
      }
      button::Style {
        background: hovered.then(|| Background::Color(faded(c.on_surface, 0.10))),
        text_color: if hovered { c.on_surface } else { c.variant },
        border: border::rounded(z(12.0)),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  tip(
    focusable(entry)
      .on_activate(Some(message))
      .radius(z(12.0))
      .into(),
    app.i18n.t(label),
  )
}

/// `.sb-bottom-icon-bar` : réglages, réglages audio, nouvelle playlist, sous un trait fin.
fn bottom_bar(app: &App, c: Roles) -> Element<'_, Message> {
  let buttons = row![
    bottom_button(
      app,
      c,
      icons::SETTINGS,
      "Settings",
      Message::Navigate(Screen::Settings),
      app.screen == Screen::Settings,
    ),
    bottom_button(
      app,
      c,
      icons::DISCOVER_TUNE,
      "Audio presets",
      super::chrome::presets_message(),
      false,
    ),
    bottom_button(
      app,
      c,
      icons::PLAYLIST_ADD,
      "Create playlist",
      ui(UiMessage::NewPlaylist),
      false,
    ),
  ]
  .spacing(z(6.0))
  .align_y(Vertical::Center)
  .padding(Padding {
    top: z(4.0),
    right: z(6.0),
    bottom: z(4.0),
    left: z(6.0),
  })
  .height(z(44.0) - 1.0);
  column![rule(faded(c.outline_variant, 0.35)), buttons].into()
}

/// Barre latérale dépliée, `width` en pixels écran.
pub fn expanded(app: &App, width: f32) -> Element<'_, Message> {
  let c = Roles::of(app);
  let texts: f32 = TABS
    .iter()
    .map(|&value| text_width(&app.i18n.t(tab_title(value)), sb::TAB_TEXT, fonts::SEMIBOLD))
    .sum();
  let padding_x = tabs_padding_x(texts, width);
  let tabs = column![
    row(TABS.iter().map(|&value| tab(app, c, value, padding_x)))
      .spacing(sb::TABS_GAP)
      .padding(Padding {
        top: sb::TABS_PADDING_TOP,
        right: sb::TABS_PADDING_X,
        bottom: 0.0,
        left: sb::TABS_PADDING_X,
      }),
    rule(faded(c.outline_variant, sb::TABS_RULE_SHARE))
  ];
  let library = column![tabs, feed(app, c), bottom_bar(app, c)]
    .spacing(z(8.0))
    .height(Fill);
  container(
    column![
      container(banner(app)).padding(Padding {
        bottom: z(4.0),
        ..Padding::ZERO
      }),
      container(library).height(Fill).padding(Padding {
        top: z(2.0),
        ..Padding::ZERO
      })
    ]
    .spacing(z(8.0))
    .height(Fill),
  )
  .padding(Padding {
    top: z(12.0),
    right: z(12.0),
    bottom: z(10.0),
    left: z(12.0),
  })
  .width(width)
  .height(Fill)
  .style(|theme: &Theme| {
    container::Style::default().background(theme.extended_palette().background.weaker.color)
  })
  .into()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_top_fade_starts_after_six_css_pixels() {
    assert!(!fade_top_visible(0.0));
    assert!(!fade_top_visible(sb::FADE_TOP_SCROLL));
    assert!(fade_top_visible(sb::FADE_TOP_SCROLL + 0.5));
  }

  #[test]
  fn the_three_tabs_fit_the_narrowest_sidebar_in_every_language() {
    use crate::i18n::I18n;
    let dir = std::path::Path::new("");
    for lang in ["en", "ru", "uk"] {
      let i18n = I18n::load(lang, dir);
      let texts: f32 = TABS
        .iter()
        .map(|&tab| text_width(&i18n.t(tab_title(tab)), sb::TAB_TEXT, fonts::SEMIBOLD))
        .sum();
      let padding = tabs_padding_x(texts, 256.0);
      let total = texts + 2.0 * TABS.len() as f32 * padding + 2.0 * sb::TABS_GAP;
      let available = 256.0 - 2.0 * z(12.0) - 2.0 * sb::TABS_PADDING_X;
      assert!(
        total <= available + 0.01,
        "{lang} : {total} px pour {available} px"
      );
      assert!(padding > 0.0, "{lang} : plus de marge autour des libellés");
    }
  }

  #[test]
  fn the_tab_padding_shrinks_only_when_the_labels_need_the_room() {
    assert!((tabs_padding_x(100.0, 390.0) - sb::TAB_PADDING_X).abs() < 1e-6);
    assert!(tabs_padding_x(190.0, 256.0) < sb::TAB_PADDING_X);
    assert!(tabs_padding_x(500.0, 256.0).abs() < 1e-6);
  }

  #[test]
  fn the_feed_shows_forty_upcoming_and_twenty_recent_titles() {
    assert_eq!((sb::UPCOMING_SHOWN, sb::RECENT_SHOWN), (40, 20));
  }

  #[test]
  fn queue_rows_are_more_compact_than_library_rows() {
    let (queue, normal) = (Dim::QUEUE, Dim::NORMAL);
    assert!(queue.cover < normal.cover);
    assert!(queue.cover_radius < normal.cover_radius);
    assert!(queue.padding_y < normal.padding_y);
    assert!(queue.gap < normal.gap);
    assert!(queue.title < normal.title);
    assert!(queue.sub < normal.sub);
  }
}
