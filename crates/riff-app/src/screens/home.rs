//! Accueil `#view-discover` de Riff 44.1.1, disposition « spotify » (`riff_home_layout`, défaut d'un
//! Riff vierge) : « Pour toi », « Accueil » et « Tendance maintenant », puis les artistes favoris
//! (`index.html:549-617`, `styles.css:9281-9420`, `app.js:24907-25600`). Les dimensions sont en
//! pixels CSS convertis par [`tokens::css`] ; voir [`tokens::home`].
//!
//! Le fondu des textes (`.sp-fade`) et des bords des rangées (`updateRowFade`) est un masque CSS dans
//! Riff : ici, un dégradé de la couleur de la page posé par-dessus.

use super::chrome::{tip, tips};
use super::window_frame::overlay;
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_CARD};
use crate::theme::tokens::home as metrics;
use crate::theme::{fonts, icons, tokens};
use crate::ui::{HomeRow, ListId, PlayerMessage, UiMessage};
use crate::user_data::{DailyMix, Track};
use crate::visuals::state::bezier_ease;
use crate::widgets::focus::focusable;
use crate::widgets::wheel::horizontal_wheel;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke};
use iced::widget::scrollable::{self, Direction, Scrollbar};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
  Column, Row, button, canvas as canvas_widget, column, container, image, mouse_area, responsive,
  row, scrollable as scrollable_widget, space, stack, text,
};
use iced::{
  Background, Color, ContentFit, Element, Fill, Gradient, Length, Padding, Point, Radians,
  Rectangle, Renderer, Size, Theme, border, gradient, mouse,
};
use material_colors::color::Rgb;
use std::collections::HashSet;
use std::time::Duration;

/// `getMix4Covers` et `coverCandidatesFromTracks` n'examinent que les premiers titres.
const CANDIDATE_TRACKS: usize = 8;
/// Cartes « Daily Mix » en attente quand `_plannedMixCount` est inconnu (`app.js:25492`).
const DEFAULT_PLANNED_MIXES: usize = 5;
/// Tuiles de l'accès rapide hors « Liked songs » (`app.js:24941`).
const QUICK_TILES: usize = 5;

/// Rôles de couleur de Riff utilisés par l'accueil (`--md-sys-color-*`).
#[derive(Clone, Copy)]
pub(super) struct Roles {
  pub(super) on_surface: Color,
  pub(super) variant: Color,
  pub(super) outline: Color,
  pub(super) container: Color,
  pub(super) high: Color,
  pub(super) highest: Color,
  pub(super) primary: Color,
  pub(super) on_primary: Color,
  pub(super) primary_container: Color,
  pub(super) on_primary_container: Color,
  /// `--page-solid` : fond de la page, sous les rangées.
  pub(super) page: Color,
}

fn rgb(color: Rgb) -> Color {
  Color::from_rgb8(color.red, color.green, color.blue)
}

impl Roles {
  pub(super) fn of(app: &App) -> Self {
    let s = &app.theme.scheme;
    Self {
      on_surface: rgb(s.on_surface),
      variant: rgb(s.on_surface_variant),
      outline: rgb(s.outline),
      container: rgb(s.surface_container),
      high: rgb(s.surface_container_high),
      highest: rgb(s.surface_container_highest),
      primary: rgb(s.primary),
      on_primary: rgb(s.on_primary),
      primary_container: rgb(s.primary_container),
      on_primary_container: rgb(s.on_primary_container),
      page: rgb(s.surface_dim),
    }
  }
}

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

/// `color` posé à `alpha` sur `below`.
pub(super) fn blend(color: Color, below: Color, alpha: f32) -> Color {
  Color::from_rgb(
    color.r * alpha + below.r * (1.0 - alpha),
    color.g * alpha + below.g * (1.0 - alpha),
    color.b * alpha + below.b * (1.0 - alpha),
  )
}

/// Premières pochettes distinctes d'un mix, complétées par des doublons : `getMix4Covers`.
fn mix_grid_covers(mix: &DailyMix) -> Vec<String> {
  let thumbs = || mix.tracks.iter().filter_map(Track::thumb_url);
  let mut seen = HashSet::new();
  let mut covers: Vec<String> = thumbs()
    .filter(|url| seen.insert(url.clone()))
    .take(4)
    .collect();
  if covers.len() < 4 {
    covers.extend(thumbs().take(4 - covers.len()));
  }
  covers
}

/// Pochette seule d'un mix : première image parmi les premiers titres (`coverCandidatesFromTracks`).
fn mix_cover(mix: &DailyMix) -> Option<String> {
  mix
    .tracks
    .iter()
    .take(CANDIDATE_TRACKS)
    .find_map(Track::thumb_url)
}

/// Pochette de la carte d'un mix : mosaïque dès quatre images (`spCard`, `gridCovers`).
fn mix_card_cover(mix: &DailyMix) -> Cover {
  let grid = mix_grid_covers(mix);
  if grid.len() >= 4 {
    Cover::Grid(grid)
  } else {
    Cover::Url(mix_cover(mix))
  }
}

/// Pochettes de l'accueil.
pub fn cover_urls(app: &App, out: &mut Vec<String>) {
  let home = &app.ui.home;
  out.extend(
    home
      .wave
      .iter()
      .take(CANDIDATE_TRACKS)
      .find_map(Track::thumb_url),
  );
  if let Some(mixes) = &app.data.daily_mixes {
    for mix in &mixes.mixes {
      match mix_card_cover(mix) {
        Cover::Grid(urls) => out.extend(urls),
        Cover::Url(url) => out.extend(url),
        Cover::Spinner => {}
      }
    }
  }
  out.extend(app.data.liked_custom_cover().map(str::to_string));
  out.extend(
    app
      .recent_tracks()
      .iter()
      .filter_map(|track| track.thumb_url()),
  );
  out.extend(
    app
      .data
      .playlists
      .iter()
      .take(QUICK_TILES)
      .filter_map(super::common::playlist_cover_url),
  );
  out.extend(home.trending.iter().filter_map(Track::thumb_url));
  out.extend(
    app
      .data
      .followed_artists()
      .iter()
      .filter_map(|artist| artist.avatar.clone()),
  );
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  let mut page = Column::new()
    .spacing(metrics::SECTION_GAP)
    .width(Fill)
    .padding(Padding {
      top: metrics::HOME_SIDE,
      right: metrics::HOME_SIDE,
      bottom: metrics::HOME_BOTTOM,
      left: metrics::HOME_SIDE,
    });
  if let Some(promo) = spotify_promo(app) {
    page = page.push(container(promo).padding(Padding::ZERO.bottom(metrics::PROMO_MARGIN)));
  }
  for section in app.home_sections() {
    let element = match section {
      "sp-made-sec" => made_section(app, roles),
      "sp-quick-sec" => Some(quick_section(app, roles)),
      "sp-trend-sec" => trending_section(app, roles),
      _ => artists_section(app, roles),
    };
    page = page.extend(element);
  }
  // `.main-content` défile avec son remplissage ; pouce transparent tant que la souris n'est pas dessus.
  let content = container(page).padding(Padding {
    top: metrics::MAIN_TOP,
    right: metrics::MAIN_RIGHT,
    bottom: metrics::MAIN_BOTTOM,
    left: metrics::MAIN_LEFT,
  });
  scrollable_widget(content)
    .direction(Direction::Vertical(
      Scrollbar::new()
        .width(metrics::SCROLLBAR)
        .scroller_width(metrics::SCROLLBAR)
        .margin(0),
    ))
    .style(move |_, status| page_scroll_style(roles, status))
    .width(Fill)
    .height(Fill)
    .into()
}

pub(super) fn page_scroll_style(roles: Roles, status: scrollable::Status) -> scrollable::Style {
  let shown = matches!(
    status,
    scrollable::Status::Hovered {
      is_vertical_scrollbar_hovered: true,
      ..
    } | scrollable::Status::Dragged {
      is_vertical_scrollbar_dragged: true,
      ..
    }
  );
  let color = if shown {
    Color {
      a: metrics::SCROLLER_SHARE,
      ..roles.on_surface
    }
  } else {
    Color::TRANSPARENT
  };
  let rail = scrollable::Rail {
    background: None,
    border: border::rounded(9999),
    scroller: scrollable::Scroller {
      background: Background::Color(color),
      border: border::rounded(9999),
    },
  };
  scrollable::Style {
    container: container::Style::default(),
    vertical_rail: rail,
    horizontal_rail: rail,
    gap: None,
    auto_scroll: scrollable::default(&Theme::Dark, status).auto_scroll,
  }
}

/// Dégradé horizontal d'une bande de `color` : opaque à droite (`opaque_right`) ou à gauche. L'angle d'iced se lit
/// comme en CSS : `FRAC_PI_2` va de gauche à droite (`iced_core-0.14.0/src/angle.rs:87`), `0` irait de bas en haut.
pub(super) fn fade_strip(
  color: Color,
  opaque_right: bool,
  width: f32,
) -> Element<'static, Message> {
  let clear = Color { a: 0.0, ..color };
  let linear = gradient::Linear::new(Radians(std::f32::consts::FRAC_PI_2));
  let linear = if opaque_right {
    linear.add_stop(0.0, clear).add_stop(1.0, color)
  } else {
    linear.add_stop(0.0, color).add_stop(1.0, clear)
  };
  container(space())
    .width(width)
    .height(Fill)
    .style(move |_| container::Style::default().background(Gradient::Linear(linear)))
    .into()
}

/// Texte sur une ligne rognée, avec le fondu `.sp-fade` sur ses 28 derniers pixels.
fn faded<'a>(
  content: Element<'a, Message>,
  height: f32,
  background: Option<Color>,
) -> Element<'a, Message> {
  let clipped = container(content).width(Fill).height(height).clip(true);
  match background {
    None => clipped.into(),
    Some(color) => stack![
      clipped,
      container(fade_strip(color, true, metrics::TEXT_FADE))
        .width(Fill)
        .height(Fill)
        .align_x(Horizontal::Right),
    ]
    .width(Fill)
    .height(height)
    .into(),
  }
}

pub(super) fn line<'a>(
  label: String,
  size: f32,
  font: iced::Font,
  color: Color,
) -> Element<'a, Message> {
  text(label)
    .size(size)
    .font(font)
    .color(color)
    .line_height(LineHeight::Relative(metrics::TEXT_LINE))
    .wrapping(Wrapping::None)
    .into()
}

/// Fondu des textes : absent quand un fond personnalisé remplace la page.
pub(super) fn text_fade(app: &App, roles: Roles) -> Option<Color> {
  (!app.settings.visual.has_background()).then_some(roles.page)
}

/// Contenu de la pochette d'une carte.
enum Cover {
  /// Image seule, ou pochette vide (`showCoverPlaceholder`).
  Url(Option<String>),
  /// Mosaïque 2 x 2 (`.sp-grid-cover`).
  Grid(Vec<String>),
  /// Indicateur de chargement (`.m3-spinner-container`).
  Spinner,
}

pub(super) fn loaded<'a>(
  app: &App,
  url: &str,
  width: f32,
  height: f32,
  radius: border::Radius,
) -> Option<Element<'a, Message>> {
  let handle = app.images.get(&ImageKey::new(url, SIZE_CARD))?;
  Some(
    image(handle.clone())
      .width(width)
      .height(height)
      .content_fit(ContentFit::Cover)
      .border_radius(radius)
      .into(),
  )
}

/// Boîte unie de `background`, contenu centré.
pub(super) fn flat<'a>(
  width: f32,
  height: f32,
  background: Color,
  radius: border::Radius,
  content: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
  container(content.unwrap_or_else(|| space().into()))
    .width(width)
    .height(height)
    .align_x(Horizontal::Center)
    .align_y(Vertical::Center)
    .style(move |_| {
      container::Style::default()
        .background(background)
        .border(border::Border {
          radius,
          ..border::Border::default()
        })
    })
    .into()
}

/// Icône sans image, à `share` du côté et à 60 % d'opacité (`.sp-card-cover-empty svg`).
fn empty_icon<'a>(css_px: f32, roles: Roles) -> Element<'a, Message> {
  container(icons::glyph_px(icons::SP_COVER_EMPTY, css_px))
    .style(move |_| {
      container::Style::default().color(Color {
        a: metrics::EMPTY_ICON_OPACITY,
        ..roles.variant
      })
    })
    .into()
}

/// `.m3-circular-progress` : anneau qui tourne en 1,4 s, arc de tête qui s'allonge puis se résorbe
/// (`m3Rotate`, `m3Dash`, `styles.css:3353-3391`). `elapsed` vient de l'horloge de `App` (`SpinnerClock`) : la vue reste pure.
struct Spinner {
  track: Color,
  head: Color,
  elapsed: Duration,
}

impl Spinner {
  const RADIUS: f32 = 18.0;
  const WIDTH: f32 = 4.0;
  const VIEW_BOX: f32 = 48.0;

  /// Progrès (0 à 1) dans la période de 1,4 s, partagée par `m3Rotate` et `m3Dash`.
  fn progress(elapsed: Duration) -> f32 {
    let period = metrics::SPINNER_PERIOD_SECS;
    (elapsed.as_secs_f32() % period) / period
  }

  /// `(dasharray, dashoffset, rotation en radians)` à `elapsed`. Sous la réduction (`reduced`), `m3Rotate` et `m3Dash` (non `!important`,
  /// `styles.css:3373`, `3384`) finissent à l'instant et le spinner revient à `styles.css:3381-3382` : arc de 80 unités depuis 3 h, sans rotation.
  fn pose(elapsed: Duration, reduced: bool) -> (f32, f32, f32) {
    if reduced {
      return (metrics::SPINNER_BASE_DASH, 0.0, 0.0);
    }
    let t = Self::progress(elapsed);
    let (dash, offset) = Self::dash(t);
    (dash, offset, std::f32::consts::TAU * t)
  }

  /// `(stroke-dasharray, stroke-dashoffset)` de `m3Dash` à `t` (0 à 1) : `ease-in-out` s'applique à chacun des deux
  /// intervalles `0 % -> 50 %` et `50 % -> 100 %`, comme pour une règle `@keyframes`.
  fn dash(t: f32) -> (f32, f32) {
    if t < 0.5 {
      let e = bezier_ease(metrics::SPINNER_DASH_CURVE, t * 2.0);
      (
        metrics::SPINNER_DASH_START + (metrics::SPINNER_DASH_END - metrics::SPINNER_DASH_START) * e,
        metrics::SPINNER_OFFSET_MID * e,
      )
    } else {
      let e = bezier_ease(metrics::SPINNER_DASH_CURVE, (t - 0.5) * 2.0);
      (
        metrics::SPINNER_DASH_END,
        metrics::SPINNER_OFFSET_MID
          + (metrics::SPINNER_OFFSET_END - metrics::SPINNER_OFFSET_MID) * e,
      )
    }
  }
}

impl canvas::Program<Message> for Spinner {
  type State = ();

  fn draw(
    &self,
    (): &(),
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let scale = bounds.width / Self::VIEW_BOX;
    let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
    let radius = Self::RADIUS * scale;
    let width = Self::WIDTH * scale;
    let mut frame = Frame::new(renderer, bounds.size());
    frame.stroke(
      &Path::circle(center, radius),
      Stroke::default().with_color(self.track).with_width(width),
    );
    let circumference = std::f32::consts::TAU * Self::RADIUS;
    let (dash, offset, spin) = Self::pose(self.elapsed, crate::motion::Gate::REDUCED.instant());
    let (from, to) = (-offset, (-offset + dash).min(circumference));
    if from < circumference && to > from {
      let arc = Path::new(|path| {
        path.arc(canvas::path::Arc {
          center,
          radius,
          start_angle: Radians(from / Self::RADIUS + spin),
          end_angle: Radians(to / Self::RADIUS + spin),
        });
      });
      frame.stroke(
        &arc,
        Stroke::default()
          .with_color(self.head)
          .with_width(width)
          .with_line_cap(LineCap::Round),
      );
    }
    vec![frame.into_geometry()]
  }
}

fn spinner<'a>(side: f32, roles: Roles, elapsed: Duration) -> Element<'a, Message> {
  canvas_widget(Spinner {
    track: roles.highest,
    head: roles.primary,
    elapsed,
  })
  .width(side)
  .height(side)
  .into()
}

/// `.g-spinner` (`styles.css:6248-6251`), au temps écoulé `elapsed` de l'horloge de `App` (voir `widgets::spinner`).
pub(super) fn g_spinner<'a>(side: f32, roles: Roles, elapsed: Duration) -> Element<'a, Message> {
  crate::widgets::spinner::g_spinner(side, roles.primary, elapsed)
}

/// Pochette carrée d'une carte (`.sp-card-cover`) : image, mosaïque ou attente sur `surface-container-high`.
fn card_cover<'a>(app: &App, roles: Roles, cover: &Cover) -> Element<'a, Message> {
  let side = metrics::CARD_WIDTH;
  let radius = border::Radius::from(metrics::COVER_RADIUS);
  match cover {
    Cover::Url(Some(url)) => loaded(app, url, side, side, radius)
      .unwrap_or_else(|| flat(side, side, roles.high, radius, None)),
    Cover::Url(None) => flat(
      side,
      side,
      roles.high,
      radius,
      Some(empty_icon(
        metrics::EMPTY_ICON_SHARE * metrics::CARD_WIDTH / tokens::ZOOM,
        roles,
      )),
    ),
    Cover::Spinner => flat(
      side,
      side,
      roles.high,
      radius,
      Some(spinner(
        metrics::SPINNER_SVG,
        roles,
        app.home_spinner.elapsed(app.now),
      )),
    ),
    Cover::Grid(urls) => {
      let cell = (side - 3.0 * metrics::GRID_GAP) / 2.0;
      let small = border::Radius::from(metrics::GRID_RADIUS);
      let at = |index: usize| -> Element<'a, Message> {
        urls
          .get(index)
          .and_then(|url| loaded(app, url, cell, cell, small))
          .unwrap_or_else(|| space().width(cell).height(cell).into())
      };
      container(
        column![
          row![at(0), at(1)].spacing(metrics::GRID_GAP),
          row![at(2), at(3)].spacing(metrics::GRID_GAP),
        ]
        .spacing(metrics::GRID_GAP),
      )
      .padding(metrics::GRID_GAP)
      .width(side)
      .height(side)
      .style(move |_| {
        container::Style::default()
          .background(roles.high)
          .border(border::rounded(radius))
      })
      .into()
    }
  }
}

/// Carte d'une rangée (`.sp-card`) : pochette, titre et sous-titre.
struct Card {
  row: HomeRow,
  index: usize,
  cover: Cover,
  title: String,
  sub: String,
  explicit: bool,
  /// Sans message, la carte est une attente (`Daily Mix n` pendant la génération).
  message: Option<Message>,
  /// Bouton « Play » posé sur la pochette, montré au survol seulement.
  play: Option<Message>,
}

/// `.explicit-badge` : carré de 14 px, « E » de 9 px (`styles.css:9434`).
pub(super) fn explicit_badge<'a>(roles: Roles) -> Element<'a, Message> {
  let side = tokens::css(14.0);
  container(
    text("E")
      .size(tokens::css(9.0))
      .font(fonts::BOLD)
      .color(Color::from_rgb8(0x12, 0x12, 0x12)),
  )
  .width(side)
  .height(side)
  .align_x(Horizontal::Center)
  .align_y(Vertical::Center)
  .style(move |_| {
    container::Style::default()
      .background(roles.variant)
      .border(border::rounded(tokens::css(3.0)))
  })
  .into()
}

fn card<'a>(app: &App, roles: Roles, spec: Card) -> Element<'a, Message> {
  let key = (spec.row, spec.index);
  let hovered = app.ui.home.hover == Some(key);
  let fade = text_fade(app, roles);
  let title_height = tokens::text::CARD_TITLE * metrics::TEXT_LINE;
  let sub_height = tokens::text::SUBTITLE * metrics::TEXT_LINE;
  // `.sp-card:hover .sp-card-cover` : `translateY(-2px)` en `transition: transform 180ms ease` (`styles.css:9378-9381`). La levée
  // de 2 px est réservée au repos ; sans piste (état posé sans message), la valeur suit `hover`.
  let raised = app
    .ui
    .home
    .lift
    .value(key, app.now, f32::from(u8::from(hovered)));
  let lift = metrics::COVER_LIFT * (1.0 - raised);
  let cover = card_cover(app, roles, &spec.cover);
  // Bouton seulement sous le pointeur : sans animation, rien ne s'ajoute aux images des cartes au repos. Seule la pochette
  // change de forme, elle n'a pas d'état ; la carte, son bouton et sa zone de survol gardent le leur.
  let cover = match spec.play.filter(|_| hovered) {
    Some(play) => stack![
      cover,
      container(play_button(app, roles, play))
        .padding(metrics::CARD_PLAY_MARGIN)
        .align_right(Fill)
        .align_bottom(Fill)
    ]
    .into(),
    None => cover,
  };
  let mut body = column![
    space().height(lift),
    cover,
    space().height(metrics::TITLE_MARGIN + metrics::COVER_LIFT - lift),
    faded(
      line(
        spec.title,
        tokens::text::CARD_TITLE,
        fonts::SEMIBOLD,
        roles.on_surface
      ),
      title_height,
      fade
    ),
    space().height(metrics::SUB_MARGIN),
  ]
  .width(metrics::CARD_WIDTH);
  let sub = line(spec.sub, tokens::text::SUBTITLE, fonts::UI, roles.variant);
  let sub = if spec.explicit {
    row![explicit_badge(roles), space().width(tokens::css(6.0)), sub]
      .align_y(Vertical::Center)
      .into()
  } else {
    sub
  };
  body = body.push(faded(sub, sub_height, fade));
  let Some(message) = spec.message else {
    return container(body).width(metrics::CARD_WIDTH).into();
  };
  let widget = button(body)
    .padding(0)
    .width(metrics::CARD_WIDTH)
    .style(|_, _| button::Style::default())
    .on_press(message.clone());
  let (row_id, index) = (spec.row, spec.index);
  mouse_area(
    focusable(widget)
      .on_activate(Some(message))
      .radius(metrics::COVER_RADIUS),
  )
  .on_enter(ui(UiMessage::HomeHover(row_id, index, true)))
  .on_exit(ui(UiMessage::HomeHover(row_id, index, false)))
  .into()
}

/// Bouton rond « Play » d'une carte : `primary`, calque `on-primary` de 8 % au survol et 12 % à l'appui, sans transition.
fn play_button<'a>(app: &App, roles: Roles, message: Message) -> Element<'a, Message> {
  let side = metrics::CARD_PLAY;
  let widget =
    button(container(icons::glyph_px(icons::PL_PLAY, metrics::CARD_PLAY_ICON)).center(Fill))
      .padding(0)
      .width(side)
      .height(side)
      .style(move |_, status| {
        let fill = match status {
          button::Status::Hovered => overlay(roles.primary, roles.on_primary, 0.08),
          button::Status::Pressed => overlay(roles.primary, roles.on_primary, 0.12),
          _ => roles.primary,
        };
        button::Style {
          background: Some(Background::Color(fill)),
          text_color: roles.on_primary,
          border: border::rounded(side / 2.0),
          ..button::Style::default()
        }
      })
      .on_press(message);
  tip(widget.into(), app.i18n.t("Play"))
}

/// Colonnes d'une rangée : largeur d'une carte, écart et hauteur de la rangée.
#[derive(Clone, Copy)]
struct RowSize {
  item_width: f32,
  gap: f32,
  height: f32,
}

impl RowSize {
  const CARDS: Self = Self {
    item_width: metrics::CARD_WIDTH,
    gap: metrics::CARD_GAP,
    height: metrics::CARD_HEIGHT,
  };
}

/// Rangée de cartes qui défile (`.sp-row`) : barre masquée, fondu de bord au-delà de 4 px, molette
/// verticale laissée à la page.
fn card_row<'a>(
  app: &'a App,
  roles: Roles,
  row_id: HomeRow,
  count: usize,
  build: impl Fn(usize) -> Element<'a, Message> + 'a,
  size: RowSize,
) -> Element<'a, Message> {
  let RowSize {
    item_width,
    gap,
    height,
  } = size;
  let offset = row_id.index().map_or(0.0, |slot| app.ui.home.scroll[slot]);
  let fade = text_fade(app, roles);
  responsive(move |size: Size| {
    let cards = Row::with_children((0..count).map(&build)).spacing(gap);
    let scroller = scrollable_widget(cards)
      .id(row_id.scroll_id())
      .direction(Direction::Horizontal(
        Scrollbar::new().width(0).scroller_width(0).margin(0),
      ))
      .style(move |_, status| page_scroll_style(roles, status))
      .on_scroll(move |viewport| {
        ui(UiMessage::HomeRowScrolled(
          row_id,
          viewport.absolute_offset().x,
        ))
      })
      .width(Fill)
      .height(height);
    let scroller: Element<'a, Message> = horizontal_wheel(scroller).into();
    let Some(page) = fade else {
      return scroller;
    };
    let content = count as f32 * item_width + count.saturating_sub(1) as f32 * gap;
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

/// Bouton rond `.btn-icon-pill` : `surface-container`, puis `surface-container-highest` et calque
/// `--hl` de 8 % au survol, 12 % à l'appui, rayon à 40 % au survol (`styles.css:3285`, `7501-7525`).
fn pill_style(roles: Roles, side: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |_, status| {
    let (fill, label, radius) = match status {
      button::Status::Hovered => (
        overlay(roles.highest, roles.on_surface, 0.08),
        roles.on_surface,
        side * 0.4,
      ),
      button::Status::Pressed => (
        overlay(roles.highest, roles.on_surface, 0.12),
        roles.on_surface,
        tokens::css(12.0),
      ),
      _ => (roles.container, roles.variant, side / 2.0),
    };
    button::Style {
      background: Some(Background::Color(fill)),
      text_color: label,
      border: border::rounded(radius),
      ..button::Style::default()
    }
  }
}

pub(super) fn icon_pill<'a>(
  roles: Roles,
  glyph: &'a str,
  css_px: f32,
  side: f32,
  message: Message,
) -> Element<'a, Message> {
  let widget = button(container(icons::glyph_px(glyph, css_px)).center(Fill))
    .padding(0)
    .width(side)
    .height(side)
    .style(pill_style(roles, side))
    .on_press(message.clone());
  focusable(widget)
    .ripple(glyph, roles.variant, side / 2.0)
    .on_activate(Some(message))
    .radius(side / 2.0)
    .into()
}

/// `icon_pill` à deux états : actif, plein `primary` comme les boutons à bascule de la barre de lecture.
pub(super) fn icon_toggle_pill<'a>(
  roles: Roles,
  glyph: &'a str,
  css_px: f32,
  side: f32,
  active: bool,
  message: Message,
) -> Element<'a, Message> {
  if !active {
    return icon_pill(roles, glyph, css_px, side, message);
  }
  let style = move |_: &Theme, status: button::Status| {
    let share = match status {
      button::Status::Hovered => 0.08,
      button::Status::Pressed => 0.12,
      _ => 0.0,
    };
    button::Style {
      background: Some(Background::Color(overlay(
        roles.primary,
        roles.on_surface,
        share,
      ))),
      text_color: roles.on_primary,
      border: border::rounded(side / 2.0),
      ..button::Style::default()
    }
  };
  let widget = button(container(icons::glyph_px(glyph, css_px)).center(Fill))
    .padding(0)
    .width(side)
    .height(side)
    .style(style)
    .on_press(message.clone());
  focusable(widget)
    .ripple(glyph, roles.on_primary, side / 2.0)
    .on_activate(Some(message))
    .radius(side / 2.0)
    .into()
}

/// `icon_pill` d'une ligne de titre : Riff y pose l'onde sur `.track-row` (`closest(RIPPLE_SEL)`, `app.js:2626`), pas sur le bouton.
pub(super) fn row_icon_pill<'a>(
  roles: Roles,
  glyph: &'a str,
  css_px: f32,
  side: f32,
  message: Message,
) -> Element<'a, Message> {
  let widget = button(container(icons::glyph_px(glyph, css_px)).center(Fill))
    .padding(0)
    .width(side)
    .height(side)
    .style(pill_style(roles, side))
    .on_press(message.clone());
  focusable(widget)
    .on_activate(Some(message))
    .radius(side / 2.0)
    .into()
}

/// `.sp-section h2` : icône, texte en graisse 700 et, pour « Pour toi », le bouton d'actualisation.
fn section<'a>(
  roles: Roles,
  glyph: &'a str,
  label: String,
  action: Option<Element<'a, Message>>,
  body: Element<'a, Message>,
  card_row: bool,
) -> Element<'a, Message> {
  let mut header = row![
    icons::glyph_px(glyph, metrics::H2_ICON),
    text(label)
      .size(tokens::text::SECTION_TITLE)
      .font(fonts::BOLD)
      .color(roles.on_surface)
      .line_height(LineHeight::Relative(metrics::H2_LINE)),
  ]
  .spacing(metrics::H2_GAP)
  .align_y(Vertical::Center);
  if let Some(action) = action {
    header = header.push(space().width(Fill)).push(action);
  }
  // La levée de 2 px réservée au-dessus des pochettes prend sur la marge du titre : rien ne bouge.
  let margin = if card_row {
    metrics::H2_MARGIN - metrics::COVER_LIFT
  } else {
    metrics::H2_MARGIN
  };
  column![
    container(header)
      .padding(Padding::ZERO.bottom(margin))
      .width(Fill),
    body
  ]
  .width(Fill)
  .into()
}

/// Un `.m3-circular-progress` des cartes réservées est à l'écran : carte « Pour toi » sans vague, ou « Daily Mix » en attente.
#[must_use]
pub fn made_spinner_shown(app: &App) -> bool {
  if app.screen != super::Screen::Home {
    return false;
  }
  let home = &app.ui.home;
  let mixes = app
    .data
    .daily_mixes
    .as_ref()
    .map_or(0, |mixes| mixes.mixes.len());
  let has_wave = !home.wave.is_empty();
  (has_wave || mixes > 0 || home.mixes_busy)
    && (!has_wave
      || (home.mixes_busy
        && mixes
          < if home.mixes_planned > 0 {
            home.mixes_planned
          } else {
            DEFAULT_PLANNED_MIXES
          }))
}

/// Le spinner de la ligne des tendances est à l'écran (`renderTrending`, `app.js:25172`).
#[must_use]
pub fn trending_spinner_shown(app: &App) -> bool {
  app.screen == super::Screen::Home
    && app.ui.home.trending.is_empty()
    && app.ui.home.trending_loading
}

fn made_section<'a>(app: &'a App, roles: Roles) -> Option<Element<'a, Message>> {
  let home = &app.ui.home;
  let mixes: &[DailyMix] = app
    .data
    .daily_mixes
    .as_ref()
    .map_or(&[], |mixes| mixes.mixes.as_slice());
  let has_wave = !home.wave.is_empty();
  if !has_wave && mixes.is_empty() && !home.mixes_busy {
    return None;
  }
  let planned = if home.mixes_busy {
    mixes.len().max(if home.mixes_planned > 0 {
      home.mixes_planned
    } else {
      DEFAULT_PLANNED_MIXES
    })
  } else {
    mixes.len()
  };
  let count = 1 + planned;
  let row_element = card_row(
    app,
    roles,
    HomeRow::Made,
    count,
    move |index| {
      let spec = if index == 0 {
        Card {
          row: HomeRow::Made,
          index,
          cover: if has_wave {
            Cover::Url(
              home
                .wave
                .iter()
                .take(CANDIDATE_TRACKS)
                .find_map(Track::thumb_url),
            )
          } else {
            Cover::Spinner
          },
          title: "For you".to_string(),
          sub: "Based on your taste".to_string(),
          explicit: false,
          message: Some(Message::Player(PlayerMessage::PlayMyWave)),
          play: None,
        }
      } else if let Some(mix) = mixes.get(index - 1) {
        Card {
          row: HomeRow::Made,
          index,
          cover: mix_card_cover(mix),
          title: mix
            .title
            .clone()
            .unwrap_or_else(|| format!("Daily Mix {index}")),
          sub: mix.artists.join(", "),
          explicit: false,
          message: Some(ui(UiMessage::OpenMix(index - 1))),
          play: Some(ui(UiMessage::PlayMix(index - 1))),
        }
      } else {
        Card {
          row: HomeRow::Made,
          index,
          cover: Cover::Spinner,
          title: format!("Daily Mix {index}"),
          sub: String::new(),
          explicit: false,
          message: None,
          play: None,
        }
      };
      card(app, roles, spec)
    },
    RowSize::CARDS,
  );
  let refresh = tip(
    icon_pill(
      roles,
      icons::SP_REFRESH,
      metrics::REFRESH_ICON,
      metrics::REFRESH,
      ui(UiMessage::HomeRefresh),
    ),
    app.i18n.t(tips::REFRESH),
  );
  Some(section(
    roles,
    icons::SECTION_FOR_YOU,
    app.i18n.t("For you"),
    Some(refresh),
    row_element,
    true,
  ))
}

/// Contenu de la pochette d'une tuile (`.sp-tile-cover`).
enum TileCover {
  Heart,
  Url(Option<String>),
}

struct Tile {
  index: usize,
  cover: TileCover,
  title: String,
  sub: Option<String>,
  message: Message,
}

fn tile<'a>(app: &App, roles: Roles, spec: Tile) -> Element<'a, Message> {
  let hovered = app.ui.home.hover == Some((HomeRow::Quick, spec.index));
  let side = metrics::TILE_HEIGHT;
  let left = border::Radius {
    top_left: metrics::TILE_RADIUS,
    top_right: 0.0,
    bottom_right: 0.0,
    bottom_left: metrics::TILE_RADIUS,
  };
  let cover: Element<'a, Message> = match &spec.cover {
    TileCover::Heart => flat(
      side,
      side,
      roles.primary_container,
      left,
      Some(
        container(icons::glyph_px(
          icons::LIKED_HEART,
          metrics::TILE_HEART_ICON,
        ))
        .style(move |_| container::Style::default().color(roles.on_primary_container))
        .into(),
      ),
    ),
    TileCover::Url(Some(url)) => loaded(app, url, side, side, left)
      .unwrap_or_else(|| flat(side, side, roles.highest, left, None)),
    TileCover::Url(None) => flat(
      side,
      side,
      roles.highest,
      left,
      Some(empty_icon(metrics::TILE_EMPTY_ICON / tokens::ZOOM, roles)),
    ),
  };
  let resting = Color {
    a: metrics::TILE_ALPHA,
    ..roles.high
  };
  // Fond sous le texte, pour le fondu : la page sous `surface-container-high` à 70 %, ou `highest`.
  let behind = if hovered {
    roles.highest
  } else {
    blend(roles.high, roles.page, metrics::TILE_ALPHA)
  };
  let fade = text_fade(app, roles).map(|_| behind);
  let title_height = tokens::text::CARD_TITLE * metrics::TEXT_LINE;
  let sub_height = tokens::text::SUBTITLE * metrics::TEXT_LINE;
  let mut lines = column![faded(
    line(
      spec.title,
      tokens::text::CARD_TITLE,
      fonts::SEMIBOLD,
      roles.on_surface
    ),
    title_height,
    fade
  )];
  if let Some(sub) = spec.sub {
    lines = lines.push(faded(
      line(sub, tokens::text::SUBTITLE, fonts::UI, roles.variant),
      sub_height,
      fade,
    ));
  }
  let body = row![
    cover,
    container(lines)
      .width(Fill)
      .align_y(Vertical::Center)
      .height(Fill),
  ]
  .spacing(metrics::TILE_GAP)
  .padding(Padding::ZERO.right(metrics::TILE_PADDING_RIGHT));
  let widget = button(body)
    .padding(0)
    .width(Fill)
    .height(side)
    .style(move |_, status| {
      let hot = hovered || matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: Some(Background::Color(if hot { roles.highest } else { resting })),
        text_color: roles.on_surface,
        border: border::rounded(metrics::TILE_RADIUS),
        ..button::Style::default()
      }
    })
    .on_press(spec.message.clone());
  let index = spec.index;
  mouse_area(
    focusable(widget)
      .on_activate(Some(spec.message))
      .radius(metrics::TILE_RADIUS),
  )
  .on_enter(ui(UiMessage::HomeHover(HomeRow::Quick, index, true)))
  .on_exit(ui(UiMessage::HomeHover(HomeRow::Quick, index, false)))
  .into()
}

/// `renderQuickAccess` : « Liked songs », jusqu'à cinq titres récents, puis des playlists pour
/// compléter ces cinq tuiles.
fn quick_tiles(app: &App) -> Vec<Tile> {
  let favorites = app.data.favorites.len();
  let mut tiles = vec![Tile {
    index: 0,
    cover: app
      .data
      .liked_custom_cover()
      .map_or(TileCover::Heart, |url| {
        TileCover::Url(Some(url.to_string()))
      }),
    title: app.liked_title(),
    sub: (favorites > 0).then(|| format!("{favorites} songs")),
    message: Message::Navigate(crate::screens::Screen::Liked),
  }];
  let recent = app.recent_tracks();
  for (index, track) in recent.iter().enumerate() {
    tiles.push(Tile {
      index: tiles.len(),
      cover: TileCover::Url(track.thumb_url()),
      title: track.title_text().to_string(),
      sub: Some(track.artist_text()),
      message: ui(UiMessage::RowActivated(ListId::Recent, index)),
    });
  }
  if recent.len() < QUICK_TILES {
    for playlist in app.data.playlists.iter().take(QUICK_TILES - recent.len()) {
      tiles.push(Tile {
        index: tiles.len(),
        cover: TileCover::Url(super::common::playlist_cover_url(playlist)),
        title: playlist
          .name
          .clone()
          .unwrap_or_else(|| "Untitled".to_string()),
        sub: Some(format!("{} tracks", playlist.tracks.len())),
        message: ui(UiMessage::OpenPlaylist(
          playlist.id.clone().unwrap_or_default(),
        )),
      });
    }
  }
  tiles
}

fn quick_section<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let mut tiles = quick_tiles(app)
    .into_iter()
    .map(|spec| tile(app, roles, spec))
    .collect::<Vec<_>>()
    .into_iter();
  let mut grid = Column::new().spacing(metrics::QUICK_GAP).width(Fill);
  loop {
    let mut line = Row::new().spacing(metrics::QUICK_GAP).width(Fill);
    let mut filled = 0;
    for cell in tiles.by_ref().take(metrics::QUICK_COLUMNS) {
      line = line.push(cell);
      filled += 1;
    }
    if filled == 0 {
      break;
    }
    // Colonnes `minmax(0, 1fr)` : une ligne incomplète garde la largeur des autres.
    for _ in filled..metrics::QUICK_COLUMNS {
      line = line.push(space().width(Fill));
    }
    grid = grid.push(line);
    if filled < metrics::QUICK_COLUMNS {
      break;
    }
  }
  section(
    roles,
    icons::SECTION_HOME,
    app.i18n.t("Home"),
    None,
    grid.into(),
    false,
  )
}

fn trending_section<'a>(app: &'a App, roles: Roles) -> Option<Element<'a, Message>> {
  let home = &app.ui.home;
  let body = if home.trending.is_empty() {
    if !home.trending_loading {
      return None;
    }
    // `row.innerHTML = <spinner>` : conteneur de 48 px à fond `surface-container`, centré, marge de 24 px.
    let disc = container(spinner(
      metrics::SPINNER_BIG_SVG,
      roles,
      app.trending_spinner.elapsed(app.now),
    ))
    .padding(tokens::css(6.0))
    .style(move |_| {
      container::Style::default()
        .background(roles.container)
        .border(border::rounded(tokens::radius::FULL))
    });
    container(disc)
      .center_x(Fill)
      .padding(Padding::from([metrics::SPINNER_BIG_MARGIN, 0.0]))
      .into()
  } else {
    let show_explicit = app.settings.show_explicit;
    card_row(
      app,
      roles,
      HomeRow::Trending,
      home.trending.len(),
      move |index| {
        let track = &home.trending[index];
        card(
          app,
          roles,
          Card {
            row: HomeRow::Trending,
            index,
            cover: Cover::Url(track.thumb_url()),
            title: track.title_text().to_string(),
            sub: track.artist_text(),
            explicit: show_explicit && track.is_explicit(),
            message: Some(ui(UiMessage::RowActivated(ListId::Trending, index))),
            play: None,
          },
        )
      },
      RowSize::CARDS,
    )
  };
  Some(section(
    roles,
    icons::SECTION_TRENDING,
    app.i18n.t("Trending now"),
    None,
    body,
    !home.trending.is_empty(),
  ))
}

/// `formatCount` en anglais : `Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 })`.
fn compact_count(count: f64) -> String {
  let (scaled, suffix) = if count >= 1e12 {
    (count / 1e12, "T")
  } else if count >= 1e9 {
    (count / 1e9, "B")
  } else if count >= 1e6 {
    (count / 1e6, "M")
  } else if count >= 1e3 {
    (count / 1e3, "K")
  } else {
    (count, "")
  };
  let rounded = (scaled * 10.0).round() / 10.0;
  if rounded.fract() == 0.0 {
    format!("{rounded:.0}{suffix}")
  } else {
    format!("{rounded:.1}{suffix}")
  }
}

/// Texte de `.fav-artist-plays` (`app.js:11767`).
fn plays_text(plays: f64, fans: f64) -> String {
  if plays > 0.0 {
    let unit = if (plays - 1.0).abs() < f64::EPSILON {
      ""
    } else {
      "s"
    };
    format!("{} play{unit}", compact_count(plays))
  } else if fans > 0.0 {
    format!("{} listeners", compact_count(fans))
  } else {
    "0 plays".to_string()
  }
}

fn artist_card<'a>(
  app: &App,
  roles: Roles,
  index: usize,
  artist: &crate::user_data::FavoriteArtist,
) -> Element<'a, Message> {
  let hovered = app.ui.home.hover == Some((HomeRow::Artists, index));
  let side = metrics::ARTIST_CIRCLE;
  let round = border::Radius::from(side / 2.0);
  let circle: Element<'a, Message> = match artist.avatar.as_deref() {
    Some(url) if !url.is_empty() => loaded(app, url, side, side, round)
      .unwrap_or_else(|| flat(side, side, roles.high, round, None)),
    // `onerror` : disque `#2a2a2a` de l'image de secours.
    _ => flat(side, side, Color::from_rgb8(0x2a, 0x2a, 0x2a), round, None),
  };
  let plays = artist
    .extra
    .get("plays")
    .and_then(serde_json::Value::as_f64)
    .unwrap_or(0.0);
  let pill = container(
    row![
      icons::glyph_px(icons::SP_PLAYS, 10.0),
      text(plays_text(plays, artist.fans.unwrap_or(0.0)))
        .size(metrics::PLAYS_TEXT)
        .font(fonts::SEMIBOLD)
        .wrapping(Wrapping::None),
    ]
    .spacing(metrics::PLAYS_GAP)
    .align_y(Vertical::Center),
  )
  .padding(Padding::from([
    metrics::PLAYS_PADDING_Y,
    metrics::PLAYS_PADDING_X,
  ]))
  .style(move |_| {
    container::Style::default()
      .background(roles.highest)
      .color(roles.on_surface)
      .border(border::rounded(tokens::radius::FULL))
  });
  // `.fav-artist-card:hover .fav-artist-plays { opacity: 0 }`.
  let layers = if hovered {
    stack![circle]
  } else {
    stack![
      circle,
      container(pill)
        .width(Fill)
        .height(Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Bottom)
        .padding(Padding::ZERO.bottom(metrics::PLAYS_BOTTOM)),
    ]
  }
  .width(side)
  .height(side);
  let name = faded(
    line(
      artist.name.clone(),
      metrics::ARTIST_NAME_TEXT,
      fonts::SEMIBOLD,
      roles.on_surface,
    ),
    metrics::ARTIST_NAME_TEXT * metrics::TEXT_LINE,
    text_fade(app, roles),
  );
  let body = column![
    layers,
    container(name)
      .width(side)
      .padding(Padding::ZERO.top(metrics::ARTIST_NAME_GAP)),
  ]
  .align_x(Horizontal::Center)
  .width(metrics::ARTIST_WIDTH);
  let message = ui(UiMessage::OpenFollowedArtist(index));
  let widget = button(body)
    .padding(0)
    .width(metrics::ARTIST_WIDTH)
    .style(|_, _| button::Style::default())
    .on_press(message.clone());
  mouse_area(
    focusable(widget)
      .on_activate(Some(message))
      .radius(side / 2.0),
  )
  .on_enter(ui(UiMessage::HomeHover(HomeRow::Artists, index, true)))
  .on_exit(ui(UiMessage::HomeHover(HomeRow::Artists, index, false)))
  .into()
}

fn artists_section<'a>(app: &'a App, roles: Roles) -> Option<Element<'a, Message>> {
  let artists = app.data.followed_artists();
  if artists.is_empty() {
    return None;
  }
  let height = metrics::ARTIST_CIRCLE
    + metrics::ARTIST_NAME_GAP
    + metrics::ARTIST_NAME_TEXT * metrics::TEXT_LINE
    + metrics::ROW_PADDING_BOTTOM;
  let body = card_row(
    app,
    roles,
    HomeRow::Artists,
    artists.len(),
    move |index| artist_card(app, roles, index, artists[index]),
    RowSize {
      item_width: metrics::ARTIST_WIDTH,
      gap: metrics::ARTIST_GAP,
      height,
    },
  );
  Some(section(
    roles,
    icons::SECTION_ARTISTS,
    app.i18n.t("Favorite artists"),
    None,
    body,
    false,
  ))
}

/// Pastille primaire `.btn-pill-primary` : plein `primary`, rayon de 18 px au survol, calque `--hl`.
pub(super) fn primary_pill_style(roles: Roles) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |_, status| {
    let (share, radius) = match status {
      button::Status::Hovered => (0.08, tokens::css(18.0)),
      button::Status::Pressed => (0.12, tokens::css(12.0)),
      _ => (0.0, tokens::radius::FULL),
    };
    button::Style {
      background: Some(Background::Color(overlay(
        roles.primary,
        roles.on_surface,
        share,
      ))),
      text_color: roles.on_primary,
      border: border::rounded(radius),
      ..button::Style::default()
    }
  }
}

/// Encart d'import (`initSpotifyPromo`) : masqué une fois fermé ou dès qu'une playlist existe.
#[must_use]
pub fn spotify_promo(app: &App) -> Option<Element<'_, Message>> {
  if !app.spotify_promo_visible() {
    return None;
  }
  let roles = Roles::of(app);
  let badge = |glyph: &'static str, color_box: Color| -> Element<'_, Message> {
    container(icons::glyph(glyph, 20.0))
      .width(metrics::PROMO_ICON_BOX)
      .height(metrics::PROMO_ICON_BOX)
      .align_x(Horizontal::Center)
      .align_y(Vertical::Center)
      .style(move |_| {
        container::Style::default()
          .background(color_box)
          .border(border::rounded(tokens::radius::FULL))
      })
      .into()
  };
  let brands = Row::with_children(
    [
      icons::PROMO_SPOTIFY,
      icons::PROMO_YTMUSIC,
      icons::PROMO_SOUNDCLOUD,
      icons::PROMO_APPLE,
      icons::PROMO_YANDEX,
    ]
    .map(|glyph| badge(glyph, roles.high)),
  )
  .spacing(metrics::PROMO_ICON_GAP);
  let texts = column![
    text(
      app
        .i18n
        .t("Import your music from Spotify, YouTube Music, SoundCloud, Apple & Yandex Music!")
    )
    .size(metrics::PROMO_TITLE)
    .font(fonts::BOLD),
    text(app.i18n.t(
      "Transfer your playlists and liked songs in a few clicks in Settings \u{2192} Integrations"
    ))
    .size(metrics::PROMO_SUB)
    .color(roles.outline),
  ]
  .spacing(metrics::PROMO_TEXT_GAP)
  .width(Fill);
  let open = Message::Ui(UiMessage::SpotifyPromoOpen);
  let open_button = button(
    text(app.i18n.t("Open Integrations"))
      .size(metrics::PROMO_BUTTON_TEXT)
      .font(fonts::MEDIUM),
  )
  .padding(Padding::from([
    metrics::PROMO_BUTTON_PADDING_Y,
    metrics::PROMO_BUTTON_PADDING_X,
  ]))
  .style(primary_pill_style(roles))
  .on_press(open.clone());
  let open_button = focusable(open_button)
    .ripple("Open Integrations", roles.on_primary, tokens::css(18.0))
    .on_activate(Some(open))
    .radius(tokens::css(18.0));
  let close = tip(
    icon_pill(
      roles,
      icons::CLOSE,
      metrics::PROMO_CLOSE_ICON,
      metrics::PROMO_CLOSE,
      Message::Ui(UiMessage::SpotifyPromoClose),
    ),
    app.i18n.t(tips::CLOSE),
  );
  Some(
    container(
      row![brands, texts, open_button, close]
        .spacing(metrics::PROMO_GAP)
        .align_y(Vertical::Center),
    )
    .padding(Padding::from([
      metrics::PROMO_PADDING_Y,
      metrics::PROMO_PADDING_X,
    ]))
    .width(Length::Fill)
    .style(move |_| {
      container::Style::default()
        .background(roles.container)
        .color(roles.on_surface)
        .border(border::rounded(metrics::PROMO_RADIUS))
    })
    .into(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn track(thumbnail: Option<&str>) -> Track {
    Track {
      thumbnail: thumbnail.map(str::to_string),
      ..Track::default()
    }
  }

  fn mix(thumbs: &[Option<&str>]) -> DailyMix {
    DailyMix {
      tracks: thumbs.iter().map(|thumb| track(*thumb)).collect(),
      ..DailyMix::default()
    }
  }

  #[test]
  fn a_mix_cover_is_the_first_image_among_the_first_titles() {
    let mix = mix(&[None, Some("b"), Some("c")]);
    assert_eq!(mix_cover(&mix).as_deref(), Some("b"));
  }

  #[test]
  fn a_mix_without_any_cover_has_none() {
    assert_eq!(mix_cover(&mix(&[None])), None);
    assert_eq!(mix_cover(&DailyMix::default()), None);
  }

  #[test]
  fn four_distinct_covers_make_a_mosaic_in_track_order() {
    let mix = mix(&[
      Some("a"),
      Some("a"),
      Some("b"),
      None,
      Some("c"),
      Some("d"),
      Some("e"),
    ]);
    assert_eq!(mix_grid_covers(&mix), ["a", "b", "c", "d"]);
  }

  #[test]
  fn missing_covers_are_filled_with_repeats_like_get_mix_4_covers() {
    let mix = mix(&[Some("a"), Some("b"), Some("a")]);
    assert_eq!(mix_grid_covers(&mix), ["a", "b", "a", "b"]);
    let three = DailyMix {
      tracks: vec![track(Some("a")), track(Some("b")), track(Some("c"))],
      ..DailyMix::default()
    };
    assert_eq!(mix_grid_covers(&three).len(), 4);
    assert!(matches!(mix_card_cover(&three), Cover::Grid(_)));
  }

  #[test]
  fn fewer_than_four_images_in_total_fall_back_to_a_single_cover() {
    let mix = mix(&[Some("a"), None]);
    assert!(matches!(mix_card_cover(&mix), Cover::Url(Some(url)) if url == "a"));
  }

  #[test]
  fn counts_are_compacted_like_intl() {
    assert_eq!(compact_count(950.0), "950");
    assert_eq!(compact_count(1_000.0), "1K");
    assert_eq!(compact_count(1_250.0), "1.3K");
    assert_eq!(compact_count(3_400_000.0), "3.4M");
  }

  #[test]
  fn the_artist_pill_prefers_plays_then_listeners() {
    assert_eq!(plays_text(1.0, 9.0), "1 play");
    assert_eq!(plays_text(2_500.0, 9.0), "2.5K plays");
    assert_eq!(plays_text(0.0, 12_000.0), "12K listeners");
    assert_eq!(plays_text(0.0, 0.0), "0 plays");
  }

  #[test]
  fn the_spinner_falls_back_to_its_base_arc_when_reduced() {
    assert_eq!(
      Spinner::pose(Duration::from_millis(350), true),
      (80.0, 0.0, 0.0)
    );
    let (dash, offset, spin) = Spinner::pose(Duration::from_millis(350), false);
    assert!(
      (dash - 45.5).abs() < 0.1 && (offset + 17.5).abs() < 0.1,
      "{dash} {offset}"
    );
    assert!((spin - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
  }

  #[test]
  fn the_spinner_arc_grows_then_vanishes_like_the_keyframes() {
    let (dash, offset) = Spinner::dash(0.0);
    assert!((dash - 1.0).abs() < 1e-4 && offset.abs() < 1e-4);
    let (dash, offset) = Spinner::dash(0.5);
    assert!((dash - 90.0).abs() < 1e-3 && (offset + 35.0).abs() < 1e-3);
    let (dash, offset) = Spinner::dash(1.0);
    assert!((dash - 90.0).abs() < 1e-3 && (offset + 125.0).abs() < 1e-3);
  }

  #[test]
  fn the_dash_uses_ease_in_out_on_each_segment() {
    // `cubic-bezier(.42,0,.58,1)` vaut 0,5 au milieu (symétrique) et reste sous la diagonale au quart (0,129 contre 0,25).
    let (dash, offset) = Spinner::dash(0.125);
    let e = bezier_ease(metrics::SPINNER_DASH_CURVE, 0.25);
    assert!((e - 0.129).abs() < 0.002, "{e}");
    assert!((dash - (1.0 + 89.0 * e)).abs() < 1e-3 && (offset + 35.0 * e).abs() < 1e-3);
    // Second segment : le progrès repart de 0 à 50 %.
    let (dash, offset) = Spinner::dash(0.75);
    assert!(
      (dash - 90.0).abs() < 1e-3 && (offset + 80.0).abs() < 0.02,
      "{offset}"
    );
  }

  #[test]
  fn the_progress_wraps_every_1400_ms() {
    assert!((Spinner::progress(Duration::from_millis(700)) - 0.5).abs() < 1e-4);
    assert!(Spinner::progress(Duration::from_millis(1400)) < 1e-4);
    assert!((Spinner::progress(Duration::from_millis(2100)) - 0.5).abs() < 1e-4);
  }
}
