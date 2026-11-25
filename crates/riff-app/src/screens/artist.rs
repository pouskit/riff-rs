//! Page artiste `#view-artist` de Riff 44.1.1 (`index.html:790-857`, `app.js:11885-12196`,
//! `styles.css:1779-2135`) : bannière pleine largeur avec le bouton retour, les boutons Follow, Block et Bio,
//! le nom, la pastille « vérifié » et les auditeurs mensuels, puis les onglets Tracks et Albums, « Popular Songs »
//! et ses lignes. La page défile d'un bloc, sans barre visible (`styles.css:1786`, `1797`). Voir [`tokens::artist`].
//!
//! Le flou du bouton retour (`backdrop-filter`) et le masque de l'image (`mask-image`) n'existent pas dans iced :
//! le premier est un fond noir à 55 %, le second est approché par `opacity(0,8)` de l'image et le dégradé `::after`, opaque en bas.

use super::chrome::{tip, tips};
use super::common::{fade_veil, track_row};
use super::home::{Roles, primary_pill_style};
use super::library::hint;
use super::playbar::PROFILES;
use super::search::{AlbumCard, album_card};
use super::window_frame::overlay;
use crate::app::{App, DEFAULT_WINDOW, Message};
use crate::images::{ImageKey, SIZE_HERO};
use crate::motion;
use crate::theme::tokens::artist as metrics;
use crate::theme::tokens::search_results as card;
use crate::theme::{fonts, icons, tokens};
use crate::ui::{ArtistTab, CardGrid, ListId, UiMessage};
use crate::visuals::shapes::SAMPLE_COUNT;
use crate::widgets::focus::focusable;
use crate::widgets::view_transition::view_transition;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
  Column, Row, button, canvas as canvas_widget, column, container, image, responsive, row,
  scrollable, space, stack, text,
};
use iced::{
  Background, Color, ContentFit, Element, Fill, Gradient, Length, Padding, Point, Radians,
  Rectangle, Renderer, Size, Theme, border, gradient, mouse,
};
use material_colors::color::Rgb;
use std::f32::consts::TAU;

const PAGE_ID: &str = "page-artist";

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

fn rgb(color: Rgb) -> Color {
  Color::from_rgb8(color.red, color.green, color.blue)
}

fn red() -> Color {
  let [r, g, b] = metrics::BLOCK_RED;
  Color::from_rgb8(r, g, b)
}

/// Dégradé vertical, du haut vers le bas : angle CSS 180deg, `Radians::PI` pour iced (`iced_core-0.14.0/src/angle.rs:87`).
fn vertical(stops: &[(f32, Color)]) -> Element<'static, Message> {
  let mut linear = gradient::Linear::new(Radians::PI);
  for (offset, color) in stops {
    linear = linear.add_stop(*offset, *color);
  }
  container(space())
    .width(Fill)
    .height(Fill)
    .style(move |_| container::Style::default().background(Gradient::Linear(linear)))
    .into()
}

/// Zone de l'image visible : `object-fit: cover; object-position: center 25%`.
fn banner_crop(image: (u32, u32), width: f32, height: f32) -> Rectangle<u32> {
  let (iw, ih) = (image.0 as f32, image.1 as f32);
  if iw * height > ih * width {
    let wanted = (ih * width / height).round().clamp(1.0, iw);
    Rectangle {
      x: ((iw - wanted) / 2.0).round() as u32,
      y: 0,
      width: wanted as u32,
      height: image.1,
    }
  } else {
    let wanted = (iw * height / width).round().clamp(1.0, ih);
    Rectangle {
      x: 0,
      y: ((ih - wanted) * metrics::HERO_FOCUS_Y).round() as u32,
      width: image.0,
      height: wanted as u32,
    }
  }
}

/// `.btn-pill-primary`, `.btn-outline-pill` : pastille de 36 px, libellé de 13 px.
fn pill<'a>(
  label: String,
  font: iced::Font,
  style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
  message: Message,
) -> Element<'a, Message> {
  let style = std::rc::Rc::new(style);
  let label_key = label.clone();
  let content = container(text(label).size(metrics::ACTION_TEXT).font(font))
    .height(Fill)
    .align_y(Vertical::Center);
  let widget = button(content)
    .padding(
      Padding::ZERO
        .left(metrics::ACTION_PADDING_X)
        .right(metrics::ACTION_PADDING_X),
    )
    .height(metrics::ACTION_HEIGHT)
    .style({
      let style = std::rc::Rc::clone(&style);
      move |theme, status| style(theme, status)
    })
    .on_press(message.clone());
  focusable(widget)
    .ripple_by(
      &label_key,
      move |theme| style(theme, button::Status::Active).text_color,
      tokens::css(18.0),
    )
    .on_activate(Some(message))
    .radius(tokens::css(18.0))
    .into()
}

/// `.btn-outline-pill` : `surface-container-high`, texte `label` ; au survol `highest` à 8 %, rayon de 18 px.
fn outline_style(
  roles: Roles,
  label: Option<Color>,
  fill: Option<Color>,
) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |_, status| {
    let rest = fill.unwrap_or(roles.high);
    let (background, text_color, radius) = match status {
      button::Status::Hovered => (
        if fill.is_some() {
          overlay(Color::TRANSPARENT, roles.on_surface, 0.08)
        } else {
          overlay(roles.highest, roles.on_surface, 0.08)
        },
        label.unwrap_or(roles.on_surface),
        tokens::css(18.0),
      ),
      button::Status::Pressed => (
        overlay(roles.highest, roles.on_surface, 0.12),
        label.unwrap_or(roles.on_surface),
        tokens::css(12.0),
      ),
      _ => (rest, label.unwrap_or(roles.variant), tokens::radius::FULL),
    };
    button::Style {
      background: Some(Background::Color(background)),
      text_color,
      border: border::rounded(radius),
      ..button::Style::default()
    }
  }
}

/// Follow, Block et Bio (`index.html:799-803`). `.following` impose `primary` ; sans `riff_highlight_buttons`,
/// Follow prend sinon le style des autres boutons (`styles.css:7564`, `9662`).
fn actions<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let artist = &app.ui.artist;
  let followed = app.data.is_followed(&artist.name);
  let blocked = app.data.is_artist_blocked(&artist.name);
  let follow = pill(
    app.i18n.t(if followed { "Following" } else { "Follow" }),
    fonts::SEMIBOLD,
    {
      let primary = primary_pill_style(roles);
      let other = outline_style(roles, None, None);
      let highlighted = followed || app.settings.highlight_buttons;
      move |theme: &Theme, status| {
        if highlighted {
          primary(theme, status)
        } else {
          other(theme, status)
        }
      }
    },
    ui(UiMessage::ArtistFollow),
  );
  let block_fill = blocked.then_some(Color {
    a: metrics::BLOCKED_SHARE,
    ..red()
  });
  let block = pill(
    app.i18n.t(if blocked { "Blocked" } else { "Block" }),
    fonts::MEDIUM,
    outline_style(roles, Some(red()), block_fill),
    ui(UiMessage::ArtistBlock),
  );
  let mut buttons = Row::new()
    .spacing(metrics::ACTIONS_GAP)
    .align_y(Vertical::Center)
    .push(follow)
    .push(block);
  if !artist.bio.is_empty() {
    buttons = buttons.push(pill(
      app.i18n.t("Bio"),
      fonts::MEDIUM,
      outline_style(roles, None, None),
      ui(UiMessage::ArtistBio),
    ));
  }
  buttons.into()
}

/// `.artist-m3-cookie-badge` : forme `m3-cookie-8` qui tourne en 14 s, coche de la couleur `on-primary`. `turns` : angle en tours,
/// calculé par la vue sur l'horloge de `App` (`motion::badge_turns`).
struct Badge {
  fill: Color,
  check: Color,
  turns: f32,
}

impl<Message> canvas::Program<Message> for Badge {
  type State = ();

  fn draw(
    &self,
    (): &(),
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let turn = self.turns * TAU;
    let side = bounds.width;
    let center = Point::new(side / 2.0, side / 2.0);
    let shape = Path::new(|path| {
      const STEPS: usize = 120;
      for step in 0..=STEPS {
        let angle = TAU * step as f32 / STEPS as f32;
        let radius = (metrics::BADGE_RADIUS + metrics::BADGE_WAVE * (8.0 * angle).cos()) * side;
        let rotated = angle + turn;
        let point = Point::new(
          center.x + radius * rotated.cos(),
          center.y + radius * rotated.sin(),
        );
        if step == 0 {
          path.move_to(point);
        } else {
          path.line_to(point);
        }
      }
      path.close();
    });
    let mut frame = Frame::new(renderer, bounds.size());
    frame.fill(&shape, self.fill);
    // Polyligne `20 6, 9 17, 4 12` d'un SVG de 24, posé au centre à 16 px CSS.
    let scale = metrics::BADGE_CHECK / 24.0;
    let origin = Point::new(center.x - 12.0 * scale, center.y - 12.0 * scale);
    let at = |x: f32, y: f32| Point::new(origin.x + x * scale, origin.y + y * scale);
    let tick = Path::new(|path| {
      path.move_to(at(20.0, 6.0));
      path.line_to(at(9.0, 17.0));
      path.line_to(at(4.0, 12.0));
    });
    frame.stroke(
      &tick,
      Stroke::default()
        .with_color(self.check)
        .with_width(metrics::BADGE_CHECK_STROKE * metrics::BADGE_CHECK)
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round),
    );
    vec![frame.into_geometry()]
  }
}

fn badge<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  canvas_widget(Badge {
    fill: roles.primary,
    check: roles.on_primary,
    turns: motion::badge_turns(app.artist_motion.badge, app.now),
  })
  .width(metrics::BADGE)
  .height(metrics::BADGE)
  .into()
}

/// Nom, pastille « vérifié » et auditeurs mensuels.
fn identity<'a>(app: &App, roles: Roles, window_width: f32) -> Element<'a, Message> {
  let artist = &app.ui.artist;
  let mut title = Row::new().align_y(Vertical::Center).push(
    text(artist.name.clone())
      .size(metrics::title_size(window_width))
      .font(fonts::BOLD)
      .color(Color::WHITE)
      .line_height(LineHeight::Relative(metrics::TITLE_LINE))
      .wrapping(Wrapping::None),
  );
  if artist.verified {
    title = title.push(
      container(badge(app, roles))
        .padding(Padding::ZERO.left(metrics::TITLE_GAP + metrics::BADGE_MARGIN)),
    );
  }
  let mut info = Column::new().push(title);
  if !artist.fans_line.is_empty() {
    info = info.push(space().height(metrics::FANS_MARGIN)).push(
      text(artist.fans_line.clone())
        .size(metrics::FANS_TEXT)
        .color(roles.variant)
        .line_height(LineHeight::Relative(tokens::home::H2_LINE)),
    );
  }
  info.into()
}

/// `.btn-back-artist` : rond de 40 px, noir à 55 %, flèche blanche ; au survol `highest` à 8 %.
fn back<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let widget =
    button(container(icons::glyph_px(icons::ARTIST_BACK, metrics::BACK_ICON)).center(Fill))
      .padding(0)
      .width(metrics::BACK_SIZE)
      .height(metrics::BACK_SIZE)
      .style(move |_, status| {
        let fill = match status {
          button::Status::Hovered => overlay(roles.highest, roles.on_surface, 0.08),
          button::Status::Pressed => overlay(roles.highest, roles.on_surface, 0.12),
          _ => Color {
            a: metrics::BACK_FILL,
            ..Color::BLACK
          },
        };
        button::Style {
          background: Some(Background::Color(fill)),
          text_color: Color::WHITE,
          border: border::rounded(metrics::BACK_SIZE / 2.0),
          ..button::Style::default()
        }
      })
      .on_press(Message::Back);
  tip(
    focusable(widget)
      .on_activate(Some(Message::Back))
      .radius(metrics::BACK_SIZE / 2.0)
      .into(),
    app.i18n.t(tips::BACK),
  )
}

/// `.artist-hero-wrap` : fond `primary-container` vers `surface`, bannière, dégradés, retour et identité.
fn hero<'a>(app: &'a App, roles: Roles, size: Size) -> Element<'a, Message> {
  let window = app.window.size.unwrap_or(DEFAULT_WINDOW);
  let height = metrics::hero_height(window.height);
  let scheme = &app.theme.scheme;
  let (accent, surface, low) = (
    roles.primary_container,
    rgb(scheme.surface),
    rgb(scheme.surface_container_low),
  );
  let mut layers = stack![vertical(&[(0.0, accent), (1.0, surface)])]
    .width(Fill)
    .height(height);
  let key = app
    .ui
    .artist
    .banner
    .as_deref()
    .map(|url| ImageKey::new(url, SIZE_HERO));
  if let Some(key) = key
    && let Some(handle) = app.images.get(&key)
  {
    let mut picture = image(handle.clone())
      .width(Fill)
      .height(height)
      .content_fit(ContentFit::Fill)
      .opacity(metrics::HERO_BRIGHTNESS);
    if let Some(dimensions) = app.images.dimensions(&key) {
      picture = picture.crop(banner_crop(dimensions, size.width, height));
    }
    layers = layers.push(picture);
  }
  layers = layers.push(vertical(&[
    (
      0.0,
      Color::from_rgba(0.0, 0.0, 0.0, metrics::HERO_SHADE_TOP),
    ),
    (
      0.5,
      Color::from_rgba(0.0, 0.0, 0.0, metrics::HERO_SHADE_MID),
    ),
    (1.0, low),
  ]));
  layers = layers.push(
    container(back(app, roles))
      .padding(
        Padding::ZERO
          .top(metrics::BACK_TOP)
          .left(metrics::BACK_LEFT),
      )
      .width(Fill)
      .height(Fill),
  );
  let info = column![
    actions(app, roles),
    space().height(metrics::ACTIONS_MARGIN),
    identity(app, roles, window.width),
  ];
  layers = layers.push(
    container(info)
      .padding(
        Padding::ZERO
          .left(metrics::INFO_LEFT)
          .bottom(metrics::INFO_BOTTOM),
      )
      .width(Fill)
      .height(Fill)
      .align_y(Vertical::Bottom),
  );
  layers.into()
}

/// `.artist-horiz-tab` : icône et libellé, fond `on-surface` à 10 % si actif, 5 % au survol, trait de 42 px.
fn tab<'a>(
  roles: Roles,
  label: String,
  glyph: &'a str,
  active: bool,
  message: Message,
) -> Element<'a, Message> {
  let inner = metrics::TAB_MIN_WIDTH - 2.0 * metrics::TAB_PADDING_X;
  let content = container(
    column![
      icons::glyph_px(glyph, metrics::TAB_ICON),
      text(label)
        .size(metrics::TAB_TEXT)
        .font(if active {
          fonts::SEMIBOLD
        } else {
          fonts::MEDIUM
        })
        .wrapping(Wrapping::None),
    ]
    .spacing(metrics::TAB_GAP)
    .align_x(Horizontal::Center),
  )
  .width(inner)
  .align_x(Horizontal::Center);
  let radius = border::Radius {
    top_left: metrics::TAB_RADIUS,
    top_right: metrics::TAB_RADIUS,
    bottom_right: 0.0,
    bottom_left: 0.0,
  };
  let widget = button(content)
    .padding(Padding {
      top: metrics::TAB_PADDING_TOP,
      right: metrics::TAB_PADDING_X,
      bottom: metrics::TAB_PADDING_BOTTOM,
      left: metrics::TAB_PADDING_X,
    })
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      let (share, text_color) = if active {
        (metrics::TAB_ACTIVE_SHARE, roles.on_surface)
      } else if hovered {
        (metrics::TAB_HOVER_SHARE, roles.on_surface)
      } else {
        (0.0, roles.variant)
      };
      button::Style {
        background: Some(Background::Color(Color {
          a: share,
          ..roles.on_surface
        })),
        text_color,
        border: border::Border {
          radius,
          ..border::Border::default()
        },
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  let widget = focusable(widget)
    .on_activate(Some(message))
    .radius(metrics::TAB_RADIUS);
  let mut layers = stack![widget];
  if active {
    layers = layers.push(
      container(
        container(space())
          .width(metrics::TAB_BAR_WIDTH)
          .height(metrics::TAB_BAR_HEIGHT)
          .style(move |_| {
            container::Style::default()
              .background(roles.on_surface)
              .border(border::Border {
                radius: border::Radius {
                  top_left: metrics::TAB_BAR_HEIGHT,
                  top_right: metrics::TAB_BAR_HEIGHT,
                  bottom_right: 0.0,
                  bottom_left: 0.0,
                },
                ..border::Border::default()
              })
          }),
      )
      .width(Fill)
      .height(Fill)
      .align_x(Horizontal::Center)
      .align_y(Vertical::Bottom),
    );
  }
  layers.into()
}

/// `.artist-horiz-tabs` : onglets centrés, bordure basse de 1 px et marge de 4 px.
fn tabs<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let selected = app.ui.artist.tab;
  let line = rgb(app.theme.scheme.outline_variant);
  let strip = row![
    tab(
      roles,
      app.i18n.t("Tracks"),
      icons::ROW_FALLBACK,
      selected == ArtistTab::Tracks,
      ui(UiMessage::ArtistTab(ArtistTab::Tracks)),
    ),
    tab(
      roles,
      app.i18n.t("Albums"),
      icons::ARTIST_TAB_ALBUMS,
      selected == ArtistTab::Albums,
      ui(UiMessage::ArtistTab(ArtistTab::Albums)),
    ),
  ]
  .spacing(metrics::TABS_GAP);
  column![
    container(strip)
      .padding(Padding::ZERO.top(metrics::TABS_PADDING_TOP))
      .width(Fill)
      .align_x(Horizontal::Center),
    container(space()).width(Fill).height(1).style(move |_| {
      container::Style::default().background(Color {
        a: metrics::TABS_BORDER_SHARE,
        ..line
      })
    }),
    space().height(metrics::TABS_MARGIN),
  ]
  .into()
}

/// `.artist-tracks-header` : titre de 22 px et, à droite, le bouton « Show all ».
fn header<'a>(
  roles: Roles,
  title: String,
  toggle: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
  let mut line = Row::new().align_y(Vertical::Center).push(
    text(title)
      .size(tokens::text::SECTION_TITLE)
      .font(fonts::BOLD)
      .color(roles.on_surface)
      .line_height(LineHeight::Relative(tokens::home::H2_LINE))
      .width(Fill),
  );
  if let Some(toggle) = toggle {
    line = line.push(toggle);
  }
  container(line)
    .padding(Padding {
      top: metrics::HEADER_PADDING_TOP,
      right: metrics::HEADER_PADDING_X,
      bottom: metrics::HEADER_PADDING_BOTTOM,
      left: metrics::HEADER_PADDING_X,
    })
    .width(Fill)
    .into()
}

/// `.artist-tracks-toggle` : texte de 14 px (600), `on-surface` au survol.
fn toggle<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let label = if app.ui.artist.expanded {
    "Show less"
  } else {
    "Show all"
  };
  let message = ui(UiMessage::ArtistToggleExpanded);
  let widget = button(
    text(app.i18n.t(label))
      .size(metrics::TOGGLE_TEXT)
      .font(fonts::SEMIBOLD),
  )
  .padding([metrics::TOGGLE_PADDING_Y, metrics::TOGGLE_PADDING_X])
  .style(move |_, status| button::Style {
    text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
      roles.on_surface
    } else {
      roles.variant
    },
    ..button::Style::default()
  })
  .on_press(message.clone());
  focusable(widget)
    .on_activate(Some(message))
    .radius(metrics::TOGGLE_PADDING_Y)
    .into()
}

/// Forme du chargeur de `renderM3Loader` : une forme par 1,2 s sans fondu, tour en 4 s, couleur `primary`. `step` et `turns` viennent
/// de la vue, sur l'horloge de `App` (`motion::loader_state`).
struct Spinner {
  fill: Color,
  step: usize,
  turns: f32,
}

impl<Message> canvas::Program<Message> for Spinner {
  type State = ();

  fn draw(
    &self,
    (): &(),
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let profile = &PROFILES[self.step % PROFILES.len()];
    let rotation = self.turns * TAU;
    let scale = bounds.width * metrics::LOADER_SHAPE / metrics::LOADER_PROFILE_UNITS;
    let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
    let path = Path::new(|path| {
      for sample in (0..SAMPLE_COUNT).step_by(4) {
        let angle = -std::f32::consts::PI + TAU * sample as f32 / SAMPLE_COUNT as f32 + rotation;
        let radius = profile[sample] * scale;
        let point = Point::new(
          center.x + angle.cos() * radius,
          center.y + angle.sin() * radius,
        );
        if sample == 0 {
          path.move_to(point);
        } else {
          path.line_to(point);
        }
      }
      path.close();
    });
    let mut frame = Frame::new(renderer, bounds.size());
    frame.fill(&path, self.fill);
    vec![frame.into_geometry()]
  }
}

/// `.m3-loader-container` : forme qui tourne, `padding: 48px 0`.
fn loading<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let (step, turns) = motion::loader_state(app.artist_motion.loader, app.now);
  container(
    canvas_widget(Spinner {
      fill: roles.primary,
      step,
      turns,
    })
    .width(metrics::LOADER_SIZE)
    .height(metrics::LOADER_SIZE),
  )
  .padding([metrics::LOADER_PADDING_Y, 0.0])
  .width(Fill)
  .align_x(Horizontal::Center)
  .into()
}

/// `#artist-pane-tracks` : « Popular Songs », puis les cinq premières lignes ou toutes.
fn tracks_pane<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let artist = &app.ui.artist;
  let ready = !artist.loading && !artist.failed;
  let more = (ready && artist.tracks.len() > 5).then(|| toggle(app, roles));
  let head = header(roles, app.i18n.t("Popular Songs"), more);
  let body: Element<'a, Message> = if artist.loading {
    loading(app, roles)
  } else if artist.failed || (artist.tracks.is_empty() && artist.albums.is_empty()) {
    hint(app, roles, "No tracks found for this artist.")
  } else {
    let rows = (0..artist.visible_tracks()).map(|index| track_row(app, ListId::Artist, index));
    container(Column::with_children(rows).width(Fill))
      .padding(
        Padding::ZERO
          .left(metrics::LIST_PADDING_X)
          .right(metrics::LIST_PADDING_X),
      )
      .into()
  };
  column![head, body].width(Fill).into()
}

/// Colonnes de `repeat(auto-fill, minmax(156px, 1fr))` pour une largeur utile `width` : nombre et côté d'une carte.
#[must_use]
pub fn grid_columns(width: f32) -> (usize, f32) {
  let columns = (((width + metrics::GRID_GAP) / (metrics::GRID_MIN + metrics::GRID_GAP)).floor()
    as usize)
    .max(1);
  let gaps = (columns - 1) as f32 * metrics::GRID_GAP;
  (columns, (width - gaps) / columns as f32)
}

/// `#artist-pane-albums` : titre « Albums », puis la grille de cartes « artiste · année ».
fn albums_pane<'a>(app: &'a App, roles: Roles, width: f32) -> Element<'a, Message> {
  let artist = &app.ui.artist;
  let head = header(roles, app.i18n.t("Albums"), None);
  let (columns, side) = grid_columns(width - 2.0 * metrics::GRID_PADDING_X);
  let hovered = match app.ui.card_hover {
    Some((CardGrid::Artist, index)) => Some(index),
    _ => None,
  };
  let mut cards = artist.albums.iter().enumerate().map(|(index, album)| {
    let name = album.artist.clone().unwrap_or_else(|| artist.name.clone());
    let sub = match album.year.as_deref().filter(|year| !year.is_empty()) {
      Some(year) => format!("{name} \u{b7} {year}"),
      None => name,
    };
    album_card(
      app,
      roles,
      AlbumCard {
        cover: album.cover.as_deref(),
        title: album.title.clone().unwrap_or_default(),
        sub,
        side,
        hovered: hovered == Some(index),
        inset: (card::CARD_LIFT, 0.0),
        open: ui(UiMessage::OpenArtistAlbum(index)),
        enter: ui(UiMessage::CardHover(CardGrid::Artist, index, true)),
        exit: ui(UiMessage::CardHover(CardGrid::Artist, index, false)),
      },
    )
  });
  let mut rows = Column::new().spacing(metrics::GRID_GAP - card::CARD_LIFT);
  loop {
    let line: Vec<_> = cards.by_ref().take(columns).collect();
    if line.is_empty() {
      break;
    }
    rows = rows.push(Row::with_children(line).spacing(metrics::GRID_GAP));
  }
  let grid = container(rows).padding(Padding {
    top: metrics::GRID_PADDING_TOP - card::CARD_LIFT,
    right: metrics::GRID_PADDING_X,
    bottom: metrics::GRID_PADDING_BOTTOM,
    left: metrics::GRID_PADDING_X,
  });
  column![head, grid].width(Fill).into()
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  responsive(move |size| {
    let artist = &app.ui.artist;
    let tabs_shown = !artist.loading && !artist.failed && !artist.albums.is_empty();
    let mut section = Column::new().width(Fill).spacing(metrics::CONTENT_GAP);
    if tabs_shown {
      section = section.push(tabs(app, roles));
    }
    let pane = if tabs_shown && artist.tab == ArtistTab::Albums {
      albums_pane(app, roles, size.width)
    } else {
      tracks_pane(app, roles)
    };
    // `artistPaneSlideIn` : le panneau affiché glisse de 10 px et passe de 0,99 à 1 autour de son centre, sans coupe en CSS.
    section = section.push(
      view_transition(
        pane,
        app.artist_motion.pane_pose(app.now),
        fade_veil(app, roles.page),
      )
      .pop(0.0)
      .spill(metrics::PANE_OFFSET),
    );
    let page = column![
      hero(app, roles, size),
      section,
      space().height(metrics::PAGE_BOTTOM)
    ]
    .width(Fill);
    scrollable(page)
      .id(PAGE_ID)
      .direction(Direction::Vertical(Scrollbar::hidden()))
      .width(Fill)
      .height(Fill)
      .into()
  })
  .width(Length::Fill)
  .height(Length::Fill)
  .into()
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Une image carrée dans une bannière 4 fois plus large que haute : bande de 25 % de la hauteur, à 25 % de la marge.
  #[test]
  fn a_square_banner_is_cropped_to_a_strip_at_a_quarter_of_the_margin() {
    let crop = banner_crop((400, 400), 1000.0, 250.0);
    assert_eq!((crop.x, crop.width), (0, 400));
    assert_eq!(crop.height, 100);
    assert_eq!(crop.y, 75);
  }

  #[test]
  fn a_wide_banner_is_cropped_in_the_middle() {
    let crop = banner_crop((1000, 100), 500.0, 250.0);
    assert_eq!((crop.y, crop.height), (0, 100));
    assert_eq!((crop.x, crop.width), (400, 200));
  }

  /// 09, 1920 x 1080 : hero de 453,6 px écran (480 px CSS), nom de 68 px (72 px CSS).
  #[test]
  fn the_captured_window_gives_the_measured_hero_and_title() {
    assert!((metrics::hero_height(1080.0) - 453.6).abs() < 0.1);
    assert!((metrics::title_size(1920.0) - 68.04).abs() < 0.1);
    assert!((metrics::hero_height(500.0) - metrics::HERO_MIN).abs() < 1e-3);
    assert!((metrics::title_size(400.0) - metrics::TITLE_MIN).abs() < 1e-3);
  }

  /// 09 : contenu de 272 à 1582 px, soit 1310 px écran ; grille de 156 px CSS minimum.
  #[test]
  fn the_albums_grid_fills_the_width_with_cards_of_at_least_156_css_px() {
    let (columns, side) = grid_columns(1310.0 - 2.0 * metrics::GRID_PADDING_X);
    assert_eq!(columns, 7);
    assert!(side >= metrics::GRID_MIN);
    let used = columns as f32 * side + (columns - 1) as f32 * metrics::GRID_GAP;
    assert!((used - (1310.0 - 2.0 * metrics::GRID_PADDING_X)).abs() < 0.01);
    assert_eq!(grid_columns(10.0).0, 1);
  }

  /// 09 : lignes de 72 px CSS espacées de 4 px CSS (pas de 71,8 px écran mesuré entre 741 et 812).
  #[test]
  fn a_row_of_the_artist_page_has_the_measured_pitch() {
    assert!((metrics::ROW_PITCH - 71.8).abs() < 0.1);
  }
}
