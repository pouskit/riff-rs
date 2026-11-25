//! Barre de lecture `#playbar` de Riff 44.1.1 (`index.html:2781-2878`, `styles.css:2808-3320`,
//! `9599-9625`, `10769-10782`). Les dimensions sont en pixels CSS convertis par [`tokens::css`] ;
//! voir [`tokens::playbar`].
//!
//! Grille en trois colonnes : pochette, titre et boutons de la piste à gauche ; transport et ligne
//! ondulée au centre ; panneaux, audio et volume à droite. La pochette est posée par-dessus la
//! barre : son anneau (120 px) déborde de la ligne, comme `#m3c-canvas`.

use super::chrome::{queue_button_spec, tip_at, tips};
use super::common::artist_links;
use super::window_frame::overlay;
use crate::app::{App, DEFAULT_WINDOW, Message};
use crate::images::{ImageKey, SIZE_CARD};
use crate::jam::JamMessage;
use crate::library::format_duration;
use crate::lyrics::update::LyricsMessage;
use crate::motion::{self, ControlKey, HoverMix};
use crate::theme::tokens::playbar as bar;
use crate::theme::{fonts, icons, tokens};
use crate::ui::{PlayerMessage, TrackCommand, UiMessage};
use crate::user_data::Track;
use crate::visuals::canvas::{CoverLayer, MiniBarsCanvas, ShapeCover, shape_radii};
use crate::visuals::playbar::{MorphIcon, scrub_extent};
use crate::visuals::{shapes, spinner, viz};
use crate::widgets::focus::focusable;
use crate::widgets::touch::Touch;
use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::widget::text::LineHeight;
use iced::widget::{
  Row, button, canvas as canvas_widget, column, container, image, mouse_area, pin, row, space,
  stack, text,
};
use iced::{
  Background, Border, Color, Element, Event, Fill, Gradient, Length, Padding, Point, Radians,
  Rectangle, Renderer, Shadow, Size, Theme, Vector, border, gradient, mouse,
};
use material_colors::color::Rgb;

/// Infobulle au-dessus du bouton : la barre touche le bas de la fenêtre.
fn tip<'a>(content: Element<'a, Message>, label: String) -> Element<'a, Message> {
  tip_at(content, label, iced::widget::tooltip::Position::Top)
}

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

fn player(message: PlayerMessage) -> Message {
  Message::Player(message)
}

/// Rôles de couleur de Riff utilisés par la barre (`--md-sys-color-*`).
#[derive(Clone, Copy)]
struct Roles {
  on_surface: Color,
  variant: Color,
  outline: Color,
  container: Color,
  high: Color,
  highest: Color,
  primary: Color,
  on_primary: Color,
  /// Fond de `.btn-play-pause` : `primary` qui fond en 150 ms quand le thème change (`styles.css:3059`).
  play: Color,
  /// `--panel-bg` : fond de la barre.
  panel: Color,
}

fn rgb(color: Rgb) -> Color {
  Color::from_rgb8(color.red, color.green, color.blue)
}

impl Roles {
  fn of(app: &App) -> Self {
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
      play: app.theme_fade.play.value(app.now),
      panel: rgb(s.surface_container_low),
    }
  }
}

/// `color` posé à `alpha` sur `below` : l'`opacity` d'un élément entier.
fn blend(color: Color, below: Color, alpha: f32) -> Color {
  Color::from_rgb(
    color.r * alpha + below.r * (1.0 - alpha),
    color.g * alpha + below.g * (1.0 - alpha),
    color.b * alpha + below.b * (1.0 - alpha),
  )
}

/// Rayon d'un bouton rond selon son état : cercle, 40 % au survol, 12 px à l'appui
/// (`styles.css:7522`, `4969`).
pub(super) fn state_radius(size: f32, status: button::Status) -> f32 {
  match status {
    button::Status::Hovered => size * bar::HOVER_RADIUS_SHARE,
    button::Status::Pressed => bar::PRESSED_RADIUS,
    button::Status::Active | button::Status::Disabled => size / 2.0,
  }
}

fn round_style(
  size: f32,
  status: button::Status,
  background: Color,
  label: Color,
) -> button::Style {
  button::Style {
    background: Some(Background::Color(background)),
    text_color: label,
    border: border::rounded(state_radius(size, status)),
    ..button::Style::default()
  }
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
  from + (to - from) * t
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
  Color::from_rgb(
    lerp(from.r, to.r, t),
    lerp(from.g, to.g, t),
    lerp(from.b, to.b, t),
  )
}

/// Bouton rond entre le repos et le survol, à l'avancement de la transition : fond et texte en `.18s ease`, rayon de 50 % à 40 %
/// en `.22s` (`styles.css:7509`). L'appui reste instantané. Les couleurs sont `(fond, texte)`.
fn hover_style(
  size: f32,
  status: button::Status,
  mix: HoverMix,
  rest: (Color, Color),
  hovered: (Color, Color),
  pressed: (Color, Color),
) -> button::Style {
  if status == button::Status::Pressed {
    return round_style(size, status, pressed.0, pressed.1);
  }
  let (fill, label) = (
    lerp_color(rest.0, hovered.0, mix.color),
    lerp_color(rest.1, hovered.1, mix.color),
  );
  // `.pressed` tient au moins 180 ms après le relâchement (`app.js:2607-2620`) puis part en `PRESS` (150 ms) ; le rayon de 12 px
  // est celui de `:active` seul (`styles.css:4969`), pas de `.pressed`.
  button::Style {
    background: Some(Background::Color(lerp_color(fill, pressed.0, mix.press))),
    text_color: lerp_color(label, pressed.1, mix.press),
    border: border::rounded(lerp(size / 2.0, size * bar::HOVER_RADIUS_SHARE, mix.shape)),
    ..button::Style::default()
  }
}

/// `.btn-icon-pill` : fond `surface-container`, `surface-container-highest` au survol, calque `--hl`
/// de 12 % à l'appui (`styles.css:3285-3301`, `7501-7525`). `lit` : `#btn-toggle-queue-panel.active`.
/// Au survol le fond est `highest` seul (`background: highest` de `:hover` remet `background-image: none`, mesuré sur
/// `08b-survol-bouton`) ; à la sortie le calque `--hl` repart à 8 % et s'efface avec le fond.
fn pill_style(
  roles: Roles,
  lit: bool,
  mix: HoverMix,
) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |_, status| {
    let label = if lit { roles.primary } else { roles.variant };
    let hot = if lit { roles.primary } else { roles.on_surface };
    let mut style = hover_style(
      bar::PILL,
      status,
      mix,
      (roles.container, label),
      (roles.highest, hot),
      (overlay(roles.highest, roles.on_surface, 0.12), hot),
    );
    if !mix.hovered
      && mix.color > 0.0
      && status != button::Status::Pressed
      && let Some(Background::Color(fill)) = style.background
    {
      style.background = Some(Background::Color(overlay(
        fill,
        roles.on_surface,
        bar::HOVER_LAYER * mix.color,
      )));
    }
    style
  }
}

/// `#btn-shuffle`, `#btn-repeat` : `surface-container-high` à 85 % d'opacité, plein `primary` une
/// fois actifs (`styles.css:2966-3008`).
fn toggle_style(
  roles: Roles,
  active: bool,
  mix: HoverMix,
) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |_, status| {
    if active {
      let tint = |share| {
        (
          overlay(roles.primary, roles.on_surface, share),
          roles.on_primary,
        )
      };
      return hover_style(bar::PILL, status, mix, tint(0.0), tint(0.08), tint(0.12));
    }
    let hot = |share| {
      (
        overlay(roles.highest, roles.on_surface, share),
        roles.on_surface,
      )
    };
    hover_style(
      bar::PILL,
      status,
      mix,
      (
        blend(roles.high, roles.panel, 0.85),
        blend(roles.variant, roles.panel, 0.85),
      ),
      hot(0.08),
      hot(0.12),
    )
  }
}

/// `#btn-prev`, `#btn-next` : `surface-container-high`, texte `on-surface` (`styles.css:3010-3023`).
fn skip_style(roles: Roles, mix: HoverMix) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |_, status| {
    hover_style(
      bar::SKIP,
      status,
      mix,
      (roles.high, roles.on_surface),
      (
        overlay(roles.highest, roles.on_surface, 0.08),
        roles.on_surface,
      ),
      (
        overlay(roles.highest, roles.on_surface, 0.12),
        roles.on_surface,
      ),
    )
  }
}

/// `.btn-play-pause` : `primary` (92 % au survol, mélangé de blanc), ou transparent avec
/// `play-btn-nobg` (`styles.css:3040-3090`, `11801-11815`).
fn play_style(
  roles: Roles,
  filled: bool,
  radius: f32,
) -> impl Fn(&Theme, button::Status) -> button::Style {
  move |_, status| {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, label) = if filled {
      let fill = if hovered {
        blend(roles.play, Color::WHITE, 0.92)
      } else {
        roles.play
      };
      (Some(fill), roles.on_primary)
    } else {
      let fill = match status {
        button::Status::Hovered => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.08)),
        button::Status::Pressed => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.14)),
        _ => None,
      };
      (fill, roles.on_surface)
    };
    button::Style {
      background: background.map(Background::Color),
      text_color: label,
      border: border::rounded(radius),
      ..button::Style::default()
    }
  }
}

/// Clé du survol animé d'un bouton : les deux cœurs de `#fav-icon` sont un seul bouton dont seul l'attribut `fill` change.
fn hover_key(glyph: &'static str) -> ControlKey {
  ControlKey::from(if glyph == icons::FAVORITE_FILLED {
    icons::FAVORITE
  } else {
    glyph
  })
}

/// Bouton de la barre : glyphe centré, focus clavier, activation par message. `ripple` : `currentColor` de l'onde du clic.
fn control<'a>(
  glyph: &'static str,
  message: Message,
  width: f32,
  height: f32,
  focus_radius: f32,
  ripple: Color,
  style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, Message> {
  // Le survol animé a besoin des instants d'entrée et de sortie : ils passent par l'état de l'application.
  let key = hover_key(glyph);
  let mix = motion::view_mix(key);
  let size = width.min(height);
  let radius = lerp(size / 2.0, size * bar::HOVER_RADIUS_SHARE, mix.shape);
  control_with(
    container(icons::glyph(glyph, 16.0)).center(Fill).into(),
    message,
    width,
    height,
    focus_radius,
    Touch::new(key, move |_| ripple, radius).map(|touch| touch.hover(key)),
    style,
  )
}

/// Bouton de la barre dont le contenu est donné.
fn control_with<'a>(
  content: Element<'a, Message>,
  message: Message,
  width: f32,
  height: f32,
  focus_radius: f32,
  touch: Option<Touch<'a, Message>>,
  style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, Message> {
  let widget = button(content)
    .padding(0)
    .width(width)
    .height(height)
    .style(style)
    .on_press(message.clone());
  let mut focus = focusable(widget)
    .on_activate(Some(message))
    .radius(focus_radius);
  if let Some(touch) = touch {
    focus = focus.touch(touch);
  }
  focus.into()
}

fn pill<'a>(
  app: &App,
  roles: Roles,
  glyph: &'static str,
  message: Message,
  lit: bool,
) -> Element<'a, Message> {
  control(
    glyph,
    message,
    bar::PILL,
    bar::PILL,
    bar::PILL / 2.0,
    if lit { roles.primary } else { roles.variant },
    pill_style(roles, lit, app.ui.controls.mix(hover_key(glyph), app.now)),
  )
}

/// Mélange, aléatoire ou répétition : le bouton de 36 px dans la cellule de 40 px qui porte
/// l'anneau `box-shadow: 0 0 0 2px primary-container, 0 2px 8px rgba(0, 0, 0, .25)` de l'état actif.
fn toggle<'a>(
  app: &App,
  roles: Roles,
  glyph: &'static str,
  active: bool,
  message: Message,
) -> Element<'a, Message> {
  let button = control(
    glyph,
    message,
    bar::PILL,
    bar::PILL,
    bar::PILL / 2.0,
    if active {
      roles.on_primary
    } else {
      roles.on_surface
    },
    toggle_style(roles, active, app.ui.controls.mix(glyph, app.now)),
  );
  let cell = bar::PILL + 2.0 * bar::RING;
  container(button)
    .center(cell)
    .style(move |_| {
      if !active {
        return container::Style::default();
      }
      container::Style {
        border: Border {
          color: Color {
            a: 0.18,
            ..roles.primary
          },
          width: bar::RING,
          radius: (cell / 2.0).into(),
        },
        shadow: Shadow {
          color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
          offset: Vector::new(0.0, tokens::css(2.0)),
          blur_radius: tokens::css(8.0),
        },
        ..container::Style::default()
      }
    })
    .into()
}

/// Boutons de la piste en cours dans l'ordre de Riff : favori, dislike (bloquer le titre),
/// renommer, télécharger (titre non local), partager.
pub fn current_commands(track: &Track) -> Vec<TrackCommand> {
  let mut commands = vec![
    TrackCommand::ToggleFavorite,
    TrackCommand::BlockTrack,
    TrackCommand::Rename,
  ];
  if !track.is_local() {
    commands.push(TrackCommand::Download);
  }
  commands.push(TrackCommand::Share);
  commands
}

/// Profils des formes : calculés une seule fois.
pub(super) static PROFILES: std::sync::LazyLock<Vec<Vec<f32>>> =
  std::sync::LazyLock::new(shapes::all_profiles);

/// Pochette de la piste (de la taille de la barre, déjà placée) et l'anneau à poser au-dessus de la barre : carré de 72 px à coins
/// de 12 px, ou forme (`#m3c-canvas`, 120 px centré sur la pochette de 72 px, `styles.css:9269-9271`). L'image et son masque sont
/// coupés aux bords de la barre ; l'anneau de progression est un second canvas de la taille de la barre, qui déborde au survol
/// (`.playbar` : `overflow: visible`, `styles.css:5387`). Les canvas ont la taille de la barre : un `pin` au centre du canvas de
/// 120 px aurait une ordonnée négative et ses limites rognées le décaleraient vers le haut (mesuré : 6,6 px).
fn current_cover<'a>(
  app: &App,
  track: &Track,
  roles: Roles,
  center: Point,
) -> (Element<'a, Message>, Option<Element<'a, Message>>) {
  let url = app.cover_of(track);
  let picture = url
    .as_deref()
    .and_then(|url| app.images.get(&ImageKey::new(url, SIZE_CARD)).cloned());
  if !app.m3_cover_on() || app.lyrics.tvk_open {
    // `.track-artwork` : `border-radius: var(--md-shape-corner-medium)`.
    let square: Element<'a, Message> = match picture {
      Some(handle) => image(handle)
        .width(bar::COVER)
        .height(bar::COVER)
        .content_fit(iced::ContentFit::Cover)
        .border_radius(tokens::radius::M)
        .into(),
      // `.artwork-fallback` : `surface-container-highest`.
      None => container(space())
        .width(bar::COVER)
        .height(bar::COVER)
        .style(move |_| {
          container::Style::default()
            .background(roles.highest)
            .border(border::rounded(tokens::radius::M))
        })
        .into(),
    };
    let square = pin(square)
      .x(center.x - bar::COVER / 2.0)
      .y(center.y - bar::COVER / 2.0);
    return (square.into(), None);
  }
  let duration = app
    .playback
    .duration
    .map_or(0.0, |duration| duration.as_secs_f32());
  let progress = if duration > 0.0 {
    (app.playback.position.as_secs_f32() / duration).clamp(0.0, 1.0)
  } else {
    0.0
  };
  let hover = app.visuals.shape.hover_progress;
  let program = |layer, center| ShapeCover {
    radii: shape_radii(&app.visuals.shape, &PROFILES),
    rotation: app.visuals.shape.rotation,
    phase: app.visuals.shape.phase,
    progress,
    has_image: picture.is_some(),
    ring: roles.primary,
    background: roles.panel,
    fill: roles.highest,
    cover: bar::SHAPE_COVER,
    hover,
    unit: 1.0,
    center,
    layer,
  };
  let shape = canvas_widget(program(CoverLayer::Body, Some(center)))
    .width(Fill)
    .height(bar::HEIGHT);
  // Un `Frame` coupe son dessin à ses bords (comme le `<canvas>` de Riff) : le canvas de l'anneau monte au-dessus de la barre, jusqu'au
  // haut du canvas de 120 px de Riff (centré sur la pochette), pour que l'anneau levé et agrandi dépasse du bord haut de la barre.
  let above = (bar::COVER_CANVAS / 2.0 - center.y).max(0.0);
  let ring = pin(
    canvas_widget(program(
      CoverLayer::Ring,
      Some(Point::new(center.x, center.y + above)),
    ))
    .width(Fill)
    .height(bar::HEIGHT + above),
  )
  .y(-above);
  // `#playbar-artwork-wrapper` : 72 px, fixe pendant le survol (le survol ne dépend pas de la pochette qui grandit).
  let hitbox = container(
    mouse_area(space().width(bar::COVER).height(bar::COVER))
      .on_enter(ui(UiMessage::CoverHover(true)))
      .on_exit(ui(UiMessage::CoverHover(false))),
  )
  .padding(Padding {
    left: center.x - bar::COVER / 2.0,
    top: center.y - bar::COVER / 2.0,
    ..Padding::ZERO
  })
  .width(Fill)
  .height(bar::HEIGHT);
  let element = match picture {
    Some(handle) => {
      // `imgSize = Math.ceil(currentCover * 1.06)` (`app.js:16910`), en pixels CSS, centrée sur la pochette levée.
      let side = tokens::css(
        (70.0_f32 * (1.0 + tokens::halo::HOVER_SCALE * hover) * tokens::halo::IMAGE_SCALE).ceil(),
      );
      let lift = tokens::css(tokens::halo::HOVER_LIFT) * hover;
      let under = container(
        image(handle)
          .width(side)
          .height(side)
          .content_fit(iced::ContentFit::Cover),
      )
      .padding(Padding {
        left: (center.x - side / 2.0).max(0.0),
        top: (center.y - lift - side / 2.0).max(0.0),
        ..Padding::ZERO
      })
      .width(Fill)
      .height(bar::HEIGHT);
      stack![under, shape, hitbox]
    }
    None => stack![shape, hitbox],
  };
  let element = element.width(Fill).height(bar::HEIGHT);
  (element.into(), Some(ring.into()))
}

/// Fondu de droite des titres : `mask-image` sur les 24 derniers pixels, rendu par un dégradé du
/// fond de la barre posé par-dessus.
fn fade_right<'a>(color: Color) -> Element<'a, Message> {
  let linear = gradient::Linear::new(Radians(std::f32::consts::FRAC_PI_2))
    .add_stop(0.0, Color { a: 0.0, ..color })
    .add_stop(1.0, color);
  container(space())
    .width(bar::FADE)
    .height(Fill)
    .style(move |_| container::Style::default().background(Gradient::Linear(linear)))
    .into()
}

fn left_column<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let Some(track) = app.player.current.as_ref() else {
    return space().into();
  };
  let favorite = app.data.is_favorite(track);
  let blocked = app.data.is_track_blocked(track);
  let buttons = current_commands(track).into_iter().map(|command| {
    let button = |glyph| {
      pill(
        app,
        roles,
        glyph,
        ui(UiMessage::CurrentCommand(command.clone())),
        false,
      )
    };
    match command {
      TrackCommand::ToggleFavorite => tip(
        button(if favorite {
          icons::FAVORITE_FILLED
        } else {
          icons::FAVORITE
        }),
        app.i18n.t(tips::favorite(favorite)),
      ),
      TrackCommand::BlockTrack => tip(
        pill(
          app,
          roles,
          icons::MIX_DISLIKE,
          ui(UiMessage::CurrentCommand(command.clone())),
          blocked,
        ),
        app.i18n.t(tips::dislike(blocked)),
      ),
      TrackCommand::Rename => tip(button(icons::EDIT), app.i18n.t(tips::RENAME)),
      TrackCommand::Download => tip(button(icons::DOWNLOAD), app.i18n.t(tips::DOWNLOAD)),
      _ => tip(button(icons::SHARE), app.i18n.t(tips::SHARE)),
    }
  });
  let title = container(
    text(track.title_text().to_string())
      .size(tokens::text::PLAYBAR_TITLE)
      .font(fonts::MEDIUM)
      .line_height(LineHeight::Relative(bar::LINE_HEIGHT))
      .wrapping(text::Wrapping::None),
  )
  .style(move |_| container::Style::default().color(roles.on_surface));
  let artist = container(artist_links(
    track.artist.as_deref().unwrap_or(""),
    bar::ARTIST_TEXT,
    |name| Message::Ui(UiMessage::OpenArtist(name)),
  ))
  .style(move |_| container::Style::default().color(roles.variant));
  let lines = column![title, artist].spacing(bar::META_GAP).clip(true);
  let meta = container(
    stack![
      lines,
      container(fade_right(roles.panel))
        .width(Fill)
        .height(Fill)
        .align_right(Fill)
    ]
    .width(Fill),
  )
  .width(Fill)
  .padding(Padding {
    right: bar::META_PADDING_RIGHT,
    ..Padding::ZERO
  })
  .clip(true);
  // La pochette est dessinée par-dessus la barre : ici, sa place dans la ligne.
  let slot = space().width(bar::COVER).height(bar::COVER);
  let content = focusable(
    row![slot, meta]
      .extend(buttons)
      .spacing(bar::LEFT_GAP)
      .align_y(Vertical::Center),
  )
  .on_context(|position| Message::Ui(UiMessage::CurrentMenu(position)));
  container(content)
    .padding(Padding {
      left: bar::LEFT_PADDING,
      ..Padding::ZERO
    })
    .into()
}

/// Largeur de la colonne centrale : `clamp(340px, 40vw, 580px)`.
fn center_width(app: &App) -> f32 {
  let window = app.window.size.unwrap_or(DEFAULT_WINDOW).width;
  (window * bar::CENTER_VIEWPORT_SHARE).clamp(bar::CENTER_MIN, bar::CENTER_MAX)
}

/// Contenu du bouton lecture : le glyphe, ou pendant l'attente le glyphe qui s'efface (opacité et échelle 0,65) sous le spinner
/// `#playbar-spinner` (`styles.css:3139-3160`, `index.html:2828`). Sans attente, le glyphe seul.
fn play_content<'a>(
  app: &App,
  roles: Roles,
  glyph: &'a str,
  morph: Option<f32>,
) -> Element<'a, Message> {
  let wait = &app.play_wait;
  let hidden = wait.icon_hidden(app.now);
  let spinner_start = wait.spinner_since().filter(|_| wait.spinner_shown(app.now));
  let filled = app.settings.play_btn_bg;
  let label = if filled {
    roles.on_primary
  } else {
    roles.on_surface
  };
  if hidden <= 0.0 && spinner_start.is_none() {
    return container(play_icon(glyph, morph, label, 1.0))
      .center(Fill)
      .into();
  }
  let faded = Color {
    a: label.a * (1.0 - hidden),
    ..label
  };
  let scale = 1.0 - (1.0 - tokens::shape_spinner::PLAY_ICON_SCALE) * hidden;
  let mut layers = stack![
    container(
      container(play_icon(glyph, morph, faded, scale))
        .style(move |_| container::Style::default().color(faded))
    )
    .center(Fill)
  ];
  if spinner_start.is_some() {
    // `.btn-play-pause .m3-btn-shape-spinner` : `on-primary`, ou `primary` avec `play-btn-nobg` (`styles.css:10843`, `11817`).
    let color = if filled {
      roles.on_primary
    } else {
      roles.primary
    };
    layers = layers.push(
      container(spinner::shape_spinner(
        tokens::shape_spinner::PLAY_SIZE,
        app.shape_pose(),
        color,
      ))
      .center(Fill),
    );
  }
  layers.width(Fill).height(Fill).into()
}

/// `#play-icon` : l'icône SVG au repos ; pendant la transformation de `d` (`styles.css:3117`), les deux chemins interpolés. L'icône SVG
/// ignore `scale` (comme avant), le dessin interpolé le suit.
fn play_icon<'a>(
  glyph: &'a str,
  morph: Option<f32>,
  color: Color,
  scale: f32,
) -> Element<'a, Message> {
  match morph {
    Some(morph) => canvas_widget(MorphIcon {
      morph,
      color,
      size: tokens::PLAY_ICON_SIZE * scale,
    })
    .width(tokens::PLAY_ICON_SIZE)
    .height(tokens::PLAY_ICON_SIZE)
    .into(),
    None => icons::glyph(glyph, 16.0 * scale),
  }
}

fn transport<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  // `updatePlayPauseButton` (`app.js:14030-14065`) : pendant l'attente le bouton prend l'aspect `.playing` tout de suite.
  let waiting = app.player.loading || app.playback.buffering;
  let playing = app.playback.is_playing || waiting;
  let play_glyph = if playing { icons::PAUSE } else { icons::PLAY };
  let play_message = player(PlayerMessage::TogglePlay);
  // `transition: width, border-radius 200ms jelly` et `d` 400 ms depuis la valeur affichée (`styles.css:3059`, `3117`).
  let pose = app.playbar_motion.button.pose(playing, app.now);
  let play = container(tip(
    control_with(
      play_content(app, roles, play_glyph, pose.morph),
      play_message,
      pose.width,
      bar::PLAY_HEIGHT,
      pose.radius,
      None,
      play_style(roles, app.settings.play_btn_bg, pose.radius),
    ),
    app.i18n.t(tips::play(playing)),
  ))
  .center_x(bar::PLAY_WRAPPER_WIDTH)
  .center_y(bar::PLAY_HEIGHT);
  let skip = |glyph, message, label: &str| {
    tip(
      control(
        glyph,
        message,
        bar::SKIP,
        bar::SKIP,
        bar::SKIP / 2.0,
        roles.on_surface,
        skip_style(roles, app.ui.controls.mix(glyph, app.now)),
      ),
      app.i18n.t(label),
    )
  };
  // Cellules de 40 px pour les anneaux de 2 px : l'écart suivant perd 2 px de chaque côté.
  let edge = bar::CONTROL_GAP + bar::SKIP_MARGIN - bar::RING;
  row![
    tip(
      toggle(
        app,
        roles,
        icons::SHUFFLE,
        app.player.shuffle,
        player(PlayerMessage::ToggleShuffle)
      ),
      app.i18n.t(tips::shuffle(app.player.shuffle))
    ),
    space().width(edge),
    skip(icons::PREVIOUS, player(PlayerMessage::Prev), tips::PREVIOUS),
    space().width(bar::CONTROL_GAP),
    play,
    space().width(bar::CONTROL_GAP),
    skip(icons::NEXT, player(PlayerMessage::Next), tips::NEXT),
    space().width(edge),
    tip(
      toggle(
        app,
        roles,
        icons::REPEAT,
        app.player.repeat,
        player(PlayerMessage::ToggleRepeat)
      ),
      app.i18n.t(tips::repeat(app.player.repeat))
    ),
  ]
  .align_y(Vertical::Center)
  .height(bar::PLAY_HEIGHT)
  .into()
}

fn time_label<'a>(seconds: f32, color: Color) -> Element<'a, Message> {
  container(
    text(format_duration(f64::from(seconds)))
      .size(bar::TIME_TEXT)
      .color(color),
  )
  .center_x(bar::TIME_WIDTH)
  .into()
}

fn center_column<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let (duration, shown) = scrub_extent(app);
  let scrubber = Scrubber {
    duration,
    progress: app.scrub_progress(duration, shown),
    phase: app.wave_phase(),
    wavy: !crate::motion::perf(),
    primary: roles.primary,
    track: roles.on_surface,
    squash: 1.0,
  };
  // Temps écoulé et durée toujours affichés, sans masquage au repos (demande de Hugo, ex-réglage `autohide`).
  let label_color = roles.outline;
  let timeline = row![
    time_label(shown, label_color),
    canvas_widget(scrubber)
      .width(Fill)
      .height(bar::SCRUBBER_HEIGHT),
    time_label(duration, label_color),
  ]
  .spacing(bar::TIME_GAP)
  .align_y(Vertical::Center);
  column![container(transport(app, roles)).center_x(Fill), timeline]
    .spacing(bar::CENTER_GAP)
    .padding(Padding {
      top: bar::CENTER_PADDING_TOP,
      bottom: bar::CENTER_PADDING_BOTTOM,
      ..Padding::ZERO
    })
    .width(center_width(app))
    .into()
}

/// Diamètre de la pastille du nombre de participants.
const JAM_BADGE: f32 = 14.0;
const JAM_BADGE_TEXT: f32 = 10.0;

/// Bouton « Jam » : allumé panneau ouvert ou jam actif, pastille du nombre de participants pendant un jam.
fn jam_button<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let count = app.jam.as_ref().map_or(0, |jam| jam.participants.len());
  let base = pill(
    app,
    roles,
    icons::JAM,
    Message::Jam(JamMessage::TogglePanel),
    app.ui.jam_panel.open || app.jam.is_some(),
  );
  if count == 0 {
    return tip(base, app.i18n.t(tips::JAM));
  }
  let badge = container(
    text(count.to_string())
      .size(JAM_BADGE_TEXT)
      .font(fonts::BOLD)
      .color(roles.on_primary),
  )
  .center(JAM_BADGE)
  .style(move |_| {
    container::Style::default()
      .background(roles.primary)
      .border(border::rounded(JAM_BADGE / 2.0))
  });
  let layers = stack![
    base,
    container(badge)
      .width(Fill)
      .height(Fill)
      .align_right(Fill)
      .align_top(Fill)
  ];
  tip(
    layers.into(),
    app
      .i18n
      .t(tips::JAM_LIVE)
      .replace("{count}", &count.to_string()),
  )
}

fn right_column<'a>(app: &'a App, roles: Roles) -> Element<'a, Message> {
  let visual = &app.settings.visual;
  let mut right = Row::new().spacing(bar::RIGHT_GAP).align_y(Vertical::Center);
  if visual.bar_wave && !visual.bar_viz_behind {
    let bars = MiniBarsCanvas {
      heights: viz::mini_bars(&app.visuals.freq),
      color: Color::WHITE,
    };
    right = right.push(canvas_widget(bars).width(60).height(viz::MINI_HEIGHT));
  }
  let (queue_label, queue_message) = queue_button_spec();
  let queue = tip(
    pill(
      app,
      roles,
      icons::QUEUE_LIST,
      queue_message,
      app.ui.queue_panel.open,
    ),
    app.i18n.t(queue_label),
  );
  let lyrics = tip(
    pill(
      app,
      roles,
      icons::MUSIC_NOTE,
      Message::Lyrics(LyricsMessage::TogglePanel),
      false,
    ),
    app.i18n.t(tips::LYRICS),
  );
  let audio = tip(
    pill(
      app,
      roles,
      icons::TUNE,
      Message::AudioPrefs(crate::audio_prefs::AudioMessage::Open),
      false,
    ),
    app.i18n.t(tips::AUDIO),
  );
  let muted = app.data.muted || app.data.volume <= 0.0;
  let mute = tip(
    pill(
      app,
      roles,
      if muted {
        icons::VOLUME_OFF
      } else {
        icons::VOLUME_UP
      },
      player(PlayerMessage::ToggleMute),
      false,
    ),
    app.i18n.t(tips::mute(muted)),
  );
  let volume = mouse_area(
    canvas_widget(VolumeSlider {
      value: if muted { 0.0 } else { app.data.volume },
      primary: roles.primary,
      track: roles.on_surface,
    })
    .width(bar::VOLUME_WIDTH)
    .height(bar::VOLUME_HEIGHT),
  )
  .on_scroll(|delta| {
    let y = match delta {
      mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => y,
    };
    player(PlayerMessage::VolumeBy(y))
  });
  let right = right
    .push(jam_button(app, roles))
    .push(queue)
    .push(lyrics)
    .push(audio)
    .push(
      row![mute, volume]
        .spacing(bar::VOLUME_GAP)
        .align_y(Vertical::Center),
    );
  container(right)
    .padding(Padding {
      right: bar::RIGHT_PADDING,
      ..Padding::ZERO
    })
    .width(Length::FillPortion(1))
    .align_right(Fill)
    .into()
}

/// Barre de lecture.
pub fn player_bar(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  let line = row![
    container(left_column(app, roles)).width(Length::FillPortion(1)),
    center_column(app, roles),
    right_column(app, roles),
  ]
  .spacing(bar::COLUMN_GAP)
  .align_y(Vertical::Center)
  .height(bar::ROW_HEIGHT);
  let base = container(line)
    .padding(Padding {
      top: bar::PADDING_TOP,
      right: bar::PADDING_X,
      bottom: 0.0,
      left: bar::PADDING_X,
    })
    .width(Fill)
    .height(bar::HEIGHT)
    .style(move |_| container::Style::default().background(roles.panel));
  // Centre de la pochette : 36 px CSS de la ligne de gauche, au milieu de la ligne de la grille.
  let center = Point::new(
    bar::PADDING_X + bar::LEFT_PADDING + bar::COVER / 2.0,
    bar::PADDING_TOP + bar::ROW_HEIGHT / 2.0,
  );
  let (cover, ring): (Option<Element<'_, Message>>, Option<Element<'_, Message>>) =
    match app.player.current.as_ref() {
      Some(track) => {
        let (cover, ring) = current_cover(app, track, roles, center);
        (Some(cover), ring)
      }
      None => (None, None),
    };
  // Clip aux bords de la barre : l'image levée par le survol et son masque ne doivent pas déborder. Sans morceau, aucun enfant
  // n'est ajouté : `Stack` ouvre une couche par enfant, et une couche vide aux bornes de la barre posée juste au-dessus d'une couche
  // pleine écran devient la cible de fusion de `iced_graphics::layer::Stack::merge` (`target_index = current`, layer.rs:178) ; les
  // quads des calques suivants (voile et fond des modales) héritent alors des bornes de la barre et sont coupés.
  let clipped = match cover {
    Some(cover) => stack![base, cover]
      .width(Fill)
      .height(bar::HEIGHT)
      .clip(true)
      .into(),
    None => Element::from(base),
  };
  let bar = match ring {
    Some(ring) => stack![clipped, ring].width(Fill).height(bar::HEIGHT),
    None => stack![clipped].width(Fill).height(bar::HEIGHT),
  };
  bar.into()
}

/// Barre de progression `#squiggly-canvas` (`app.js:3377-3520`) : ligne ondulée jusqu'au curseur,
/// piste droite après lui, curseur de 7,5 x 24 px. Clic et glisser cherchent dans le morceau.
pub(super) struct Scrubber {
  pub(super) duration: f32,
  /// Part du titre affichée (`scrubProgress`), après le suivi de la position.
  pub(super) progress: f32,
  /// Phase de l'onde en radians (`scrubPhase`, `App::wave_phase`).
  pub(super) phase: f32,
  /// Onde (faux sous `body.no-wavy`, posé par `body.perf` : `app.js:3493`) ou trait droit.
  pub(super) wavy: bool,
  pub(super) primary: Color,
  pub(super) track: Color,
  /// Écrasement vertical du dessin (1 dans la barre ; mode compact : canvas de 18 px pour un bitmap de 40 px).
  pub(super) squash: f32,
}

/// Marges du canvas : `padLeft = padRight = 5.5` (`app.js:3447`), en pixels CSS.
const SCRUB_PAD: f32 = 5.5;
const WAVE_AMPLITUDE: f32 = 5.0;
const WAVE_LENGTH: f32 = 42.0;
const WAVE_WIDTH: f32 = 9.5;
const TRACK_WIDTH: f32 = 11.0;
const KNOB_REST: (f32, f32) = (7.5, 24.0);
const KNOB_DRAG: (f32, f32) = (9.0, 32.0);
/// `updateProgressKnobUI` (`app.js:16752`) : `no-knob` sauf si `riff_progress_knob` vaut `true`. Pas de réglage natif
/// pour cette clé : le curseur reste désactivé, comme dans un Riff vierge, y compris pendant un glissement.
const SHOW_KNOB: bool = false;

#[derive(Default)]
pub(super) struct DragState {
  dragging: bool,
}

impl Scrubber {
  /// Instant visé par l'abscisse `x` (pixels écran) dans le canvas : `updateScrubberPositionFromEvent`.
  fn time_at(&self, x: f32, width: f32) -> f32 {
    let pad = tokens::css(SCRUB_PAD);
    let span = width - 2.0 * pad;
    let position = if span > 0.0 { (x - pad) / span } else { 0.0 };
    position.clamp(0.0, 1.0) * self.duration
  }
}

impl canvas::Program<Message> for Scrubber {
  type State = DragState;

  fn update(
    &self,
    state: &mut DragState,
    event: &Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> Option<canvas::Action<Message>> {
    match event {
      Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        if self.duration > 0.0 && cursor.is_over(bounds) =>
      {
        let at = cursor.position_in(bounds)?;
        state.dragging = true;
        Some(
          canvas::Action::publish(player(PlayerMessage::SeekPreview(
            self.time_at(at.x, bounds.width),
          )))
          .and_capture(),
        )
      }
      Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
        Some(canvas::Action::publish(player(PlayerMessage::SeekPreview(
          self.time_at(position.x - bounds.x, bounds.width),
        ))))
      }
      Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
        state.dragging = false;
        Some(canvas::Action::publish(player(PlayerMessage::SeekRelease)))
      }
      _ => None,
    }
  }

  fn draw(
    &self,
    state: &DragState,
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let mut frame = Frame::new(renderer, bounds.size());
    // Le dessin de Riff est en pixels CSS : une échelle suffit pour passer aux pixels écran.
    frame.scale_nonuniform(Vector::new(tokens::ZOOM, tokens::ZOOM * self.squash));
    let width = bounds.width / tokens::ZOOM;
    let height = bounds.height / (tokens::ZOOM * self.squash);
    let center_y = height / 2.0;
    let progress = self.progress.clamp(0.0, 1.0);
    let (knob_width, knob_height) = if state.dragging { KNOB_DRAG } else { KNOB_REST };
    let knob_radius = knob_width / 2.0;
    let track_width = (width - 2.0 * SCRUB_PAD).max(0.0);
    let knob_x = (SCRUB_PAD + progress * track_width).clamp(SCRUB_PAD, width - SCRUB_PAD);
    let wave_end = if SHOW_KNOB {
      knob_x - knob_width / 2.0 - 4.5 - 9.5 / 2.0
    } else {
      knob_x
    };
    let rest_start = if SHOW_KNOB {
      knob_x + knob_width / 2.0 + 4.0
    } else {
      knob_x
    };
    let rest_end = width - SCRUB_PAD;
    if rest_start < rest_end - 1.0 {
      let rest = Path::line(
        Point::new(rest_start, center_y),
        Point::new(rest_end, center_y),
      );
      frame.stroke(
        &rest,
        Stroke::default()
          .with_color(Color {
            a: 0.3,
            ..self.track
          })
          .with_width(TRACK_WIDTH * self.squash)
          .with_line_cap(LineCap::Round),
      );
    }
    if wave_end > SCRUB_PAD {
      let phase = self.phase % std::f32::consts::TAU;
      let wave_at = |x: f32| {
        let angle = (x - SCRUB_PAD) * std::f32::consts::TAU / WAVE_LENGTH + phase;
        Point::new(x, center_y + angle.sin() * WAVE_AMPLITUDE)
      };
      let wave = if self.wavy {
        Path::new(|path| {
          path.move_to(wave_at(SCRUB_PAD));
          let mut x = SCRUB_PAD + 2.0;
          while x <= wave_end {
            path.line_to(wave_at(x));
            x += 2.0;
          }
          path.line_to(wave_at(wave_end));
        })
      } else {
        Path::line(
          Point::new(SCRUB_PAD, center_y),
          Point::new(wave_end, center_y),
        )
      };
      frame.stroke(
        &wave,
        Stroke::default()
          .with_color(self.primary)
          .with_width(WAVE_WIDTH * self.squash)
          .with_line_cap(LineCap::Round)
          .with_line_join(LineJoin::Round),
      );
    }
    if SHOW_KNOB {
      let knob = Path::rounded_rectangle(
        Point::new(knob_x - knob_width / 2.0, center_y - knob_height / 2.0),
        Size::new(knob_width, knob_height),
        knob_radius.into(),
      );
      frame.fill(&knob, self.primary);
    }
    vec![frame.into_geometry()]
  }

  fn mouse_interaction(
    &self,
    state: &DragState,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> mouse::Interaction {
    if state.dragging {
      mouse::Interaction::Grabbing
    } else if cursor.is_over(bounds) {
      mouse::Interaction::Pointer
    } else {
      mouse::Interaction::default()
    }
  }
}

/// Curseur de volume `.sl` (`styles.css:7568-7576`, `app.js:16696-16715`) : segment actif, segment
/// inactif à 16 % de `on-surface` et poignée de 6 x 28 px, séparés de 5 px.
struct VolumeSlider {
  value: f32,
  primary: Color,
  track: Color,
}

impl VolumeSlider {
  /// `value` de l'`input[type=range]` pour l'abscisse `x` (pixels écran) : poignée de 6 px.
  fn value_at(x: f32, width: f32) -> f32 {
    let handle = tokens::css(6.0);
    let span = width - handle;
    let raw = if span > 0.0 {
      (x - handle / 2.0) / span
    } else {
      0.0
    };
    (raw.clamp(0.0, 1.0) * 100.0).round() / 100.0
  }
}

impl canvas::Program<Message> for VolumeSlider {
  type State = DragState;

  fn update(
    &self,
    state: &mut DragState,
    event: &Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> Option<canvas::Action<Message>> {
    match event {
      Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if cursor.is_over(bounds) => {
        let at = cursor.position_in(bounds)?;
        state.dragging = true;
        Some(
          canvas::Action::publish(player(PlayerMessage::Volume(Self::value_at(
            at.x,
            bounds.width,
          ))))
          .and_capture(),
        )
      }
      Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
        Some(canvas::Action::publish(player(PlayerMessage::Volume(
          Self::value_at(position.x - bounds.x, bounds.width),
        ))))
      }
      Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
        state.dragging = false;
        Some(canvas::Action::request_redraw())
      }
      _ => None,
    }
  }

  fn draw(
    &self,
    _state: &DragState,
    renderer: &Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let mut frame = Frame::new(renderer, bounds.size());
    frame.scale(tokens::ZOOM);
    let width = bounds.width / tokens::ZOOM;
    let height = bounds.height / tokens::ZOOM;
    // `sync` : la position visuelle part de 6 % pour que la poignée ne touche pas le bord.
    let visual = if self.value > 0.0 {
      0.06 + self.value.clamp(0.0, 1.0) * 0.94
    } else {
      0.0
    };
    let center = (width - 6.0) * visual + 3.0;
    let active = (center - 8.0).max(0.0);
    let inactive = (width - center - 8.0).max(0.0);
    let (top, bar_height) = (6.0 * height / 28.0, 16.0 * height / 28.0);
    let radius = |left: f32, right: f32| border::Radius {
      top_left: left,
      top_right: right,
      bottom_right: right,
      bottom_left: left,
    };
    frame.fill(
      &Path::rounded_rectangle(
        Point::new(0.0, top),
        Size::new(active, bar_height),
        radius(8.0, 3.0),
      ),
      self.primary,
    );
    frame.fill(
      &Path::rounded_rectangle(
        Point::new(width - inactive, top),
        Size::new(inactive, bar_height),
        radius(3.0, 8.0),
      ),
      Color {
        a: 0.16,
        ..self.track
      },
    );
    frame.fill(
      &Path::rounded_rectangle(
        Point::new(center - 3.0, 0.0),
        Size::new(6.0, height),
        3.0.into(),
      ),
      self.primary,
    );
    vec![frame.into_geometry()]
  }

  fn mouse_interaction(
    &self,
    state: &DragState,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> mouse::Interaction {
    if state.dragging {
      mouse::Interaction::Grabbing
    } else if cursor.is_over(bounds) {
      mouse::Interaction::Pointer
    } else {
      mouse::Interaction::default()
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn track(platform: &str) -> Track {
    Track {
      platform: Some(platform.to_string()),
      ..Track::default()
    }
  }

  #[test]
  fn the_favorite_button_keeps_one_hover_key_for_both_hearts() {
    // Le pointeur reste sur le bouton après le clic : le survol ne doit pas repartir de zéro avec l'autre icône (`:hover` de CSS).
    assert_eq!(
      hover_key(icons::FAVORITE_FILLED),
      hover_key(icons::FAVORITE)
    );
    assert_ne!(hover_key(icons::EDIT), hover_key(icons::FAVORITE));
  }

  #[test]
  fn the_player_bar_downloads_between_rename_and_share() {
    assert_eq!(
      current_commands(&track("ytmusic")),
      [
        TrackCommand::ToggleFavorite,
        TrackCommand::BlockTrack,
        TrackCommand::Rename,
        TrackCommand::Download,
        TrackCommand::Share
      ]
    );
  }

  #[test]
  fn the_player_bar_hides_the_download_of_a_local_track() {
    let commands = current_commands(&track("local"));
    assert!(!commands.contains(&TrackCommand::Download));
    assert_eq!(commands.len(), 4);
  }

  #[test]
  fn the_player_bar_puts_dislike_right_after_the_heart() {
    for platform in ["ytmusic", "local"] {
      let commands = current_commands(&track(platform));
      assert_eq!(commands[0], TrackCommand::ToggleFavorite);
      assert_eq!(commands[1], TrackCommand::BlockTrack);
    }
  }

  #[test]
  fn the_bar_is_96_css_pixels_high_and_its_columns_fill_the_measured_width() {
    // `layout.json`, `playbar.px.h` : 90,72 px écran.
    assert!((bar::HEIGHT - 90.72).abs() < 1e-3);
    // 1920 px : 2 x 17,01 de marge, 2 x 15,12 d'écart, colonne centrale de 548,1 px.
    let side = (1920.0 - 2.0 * bar::PADDING_X - 2.0 * bar::COLUMN_GAP - bar::CENTER_MAX) / 2.0;
    assert!((side - 653.8).abs() < 0.1, "{side}");
  }

  #[test]
  fn the_scrubber_maps_the_pointer_like_riff() {
    let scrubber = Scrubber {
      duration: 200.0,
      progress: 0.0,
      phase: 0.0,
      wavy: true,
      primary: Color::WHITE,
      track: Color::WHITE,
      squash: 1.0,
    };
    let width = 484.0 * tokens::ZOOM;
    let pad = tokens::css(SCRUB_PAD);
    assert_eq!(scrubber.time_at(-10.0, width), 0.0);
    assert_eq!(scrubber.time_at(width + 10.0, width), 200.0);
    assert!((scrubber.time_at(width / 2.0, width) - 100.0).abs() < 1e-3);
    assert!((scrubber.time_at(pad, width)).abs() < 1e-3);
  }

  #[test]
  fn the_volume_slider_rounds_to_one_hundredth() {
    let width = tokens::css(80.0);
    assert_eq!(VolumeSlider::value_at(0.0, width), 0.0);
    assert_eq!(VolumeSlider::value_at(width, width), 1.0);
    assert_eq!(VolumeSlider::value_at(width / 2.0, width), 0.5);
  }
}
