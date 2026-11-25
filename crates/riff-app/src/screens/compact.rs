//! Mode compact de la fenêtre principale (`body.mini` de Riff, `styles.css:6952-7222`) : barre de titre de 28 px, puis
//! `.playbar` en grille (pochette, titre et artiste, commandes, ligne ondulée) et, quand `#btn-mini-lyrics` est
//! actif, `#mini-lyrics-container` entre les commandes et la ligne ondulée. Les tailles suivent `vmin` : voir
//! [`compact::Metrics`].

use super::chrome::{tip, tips};
use super::home::blend;
use super::playbar::{PROFILES, Scrubber, state_radius};
use super::right_panel::ellipsize;
use super::sidebar::{faded as alpha, rgb};
use super::window_frame::overlay;
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_CARD};
use crate::lyrics::model::WordState;
use crate::theme::tokens::compact;
use crate::theme::{fonts, icons, tokens};
use crate::ui::PlayerMessage;
use crate::user_data::Track;
use crate::visuals::canvas::{CoverLayer, ShapeCover, shape_radii};
use crate::visuals::playbar::scrub_extent;
use crate::widgets::focus::focusable;
use crate::window::{Action, MINI_SIZE};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::{LineHeight, Span};
use iced::widget::{
  button, canvas, column, container, image, mouse_area, rich_text, row, space, stack, text,
};
use iced::{Background, Color, Element, Fill, Font, Padding, Theme, border};

/// Rôles de couleur de Riff utilisés par le mode compact (`--md-sys-color-*`).
#[derive(Clone, Copy)]
struct Roles {
  on_surface: Color,
  variant: Color,
  outline: Color,
  high: Color,
  highest: Color,
  primary: Color,
  on_primary: Color,
  error: Color,
  on_error: Color,
  /// `body` : fond de la barre de titre.
  page: Color,
  /// Fond de `.playbar` : `#1a1a1a` mesuré sur 17a (`surface-container-low`, `tokens.json:72`) ; `styles.css:6990` demande `surface-container` (#212121), une règle plus tardive prend le dessus.
  panel: Color,
}

impl Roles {
  fn of(app: &App) -> Self {
    let s = &app.theme.scheme;
    Self {
      on_surface: rgb(s.on_surface),
      variant: rgb(s.on_surface_variant),
      outline: rgb(s.outline),
      high: rgb(s.surface_container_high),
      highest: rgb(s.surface_container_highest),
      primary: rgb(s.primary),
      on_primary: rgb(s.on_primary),
      error: rgb(s.error),
      on_error: rgb(s.on_error),
      page: rgb(s.surface_dim),
      panel: rgb(s.surface_container_low),
    }
  }
}

fn player(message: PlayerMessage) -> Message {
  Message::Player(message)
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let roles = Roles::of(app);
  let size = app.window.size.unwrap_or(MINI_SIZE);
  let metrics = compact::Metrics::of(size.width, size.height);
  let page = column![
    titlebar(app, roles),
    playbar(app, roles, &metrics, size.height)
  ]
  .width(Fill)
  .height(Fill);
  let content = container(page)
    .width(Fill)
    .height(Fill)
    .style(move |_| container::Style::default().background(roles.page));
  crate::window::frame(content.into(), app.window.is_maximized, Message::Chrome)
}

/// Bouton de la barre de titre (`.titlebar-btn.win-btn`) : fond au survol seulement, rouge pour la fermeture.
fn title_button<'a>(
  glyph: &'a str,
  message: Message,
  roles: Roles,
  closes: bool,
  lit: bool,
) -> Element<'a, Message> {
  let content = container(icons::glyph_px(glyph, compact::BUTTON_ICON)).center(Fill);
  let widget = button(content)
    .padding(0)
    .width(compact::BUTTON_WIDTH)
    .height(compact::BUTTON_HEIGHT)
    .style(move |_, status| {
      let (fill, label) = if closes {
        (roles.error, roles.on_error)
      } else {
        (roles.high, roles.on_surface)
      };
      let (background, text_color) = match status {
        button::Status::Hovered => (Some(overlay(fill, roles.on_surface, 0.08)), label),
        button::Status::Pressed => (Some(overlay(fill, roles.on_surface, 0.12)), label),
        // `body.mini.mini-lyrics-on #btn-mini-lyrics` : `color: primary` (`styles.css:7171`).
        button::Status::Active | button::Status::Disabled if lit => (None, roles.primary),
        button::Status::Active | button::Status::Disabled => (None, roles.variant),
      };
      button::Style {
        background: background.map(Background::Color),
        text_color,
        border: border::rounded(if background.is_some() {
          compact::BUTTON_HOVER_RADIUS
        } else {
          compact::BUTTON_HEIGHT / 2.0
        }),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  focusable(widget)
    .on_activate(Some(message))
    .radius(compact::BUTTON_HOVER_RADIUS)
    .into()
}

/// `#titlebar` en mode compact : zone de glissement, puis paroles, mini lecteur, réduire, agrandir, fermer.
fn titlebar(app: &App, roles: Roles) -> Element<'_, Message> {
  let chrome = Message::Chrome;
  let maximize = if app.window.is_maximized {
    icons::RESTORE
  } else {
    icons::MAXIMIZE
  };
  let controls = row![
    tip(
      title_button(
        icons::MINI_LYRICS,
        Message::ToggleMiniLyrics,
        roles,
        false,
        app.window.mini_lyrics
      ),
      app.i18n.t("Lyrics")
    ),
    tip(
      title_button(
        icons::MINI_PLAYER,
        Message::ToggleMiniMode,
        roles,
        false,
        false
      ),
      app.i18n.t("Compact mini player")
    ),
    tip(
      title_button(
        icons::MINIMIZE,
        chrome(Action::Minimize),
        roles,
        false,
        false
      ),
      app.i18n.t(tips::MINIMIZE)
    ),
    tip(
      title_button(
        maximize,
        chrome(Action::ToggleMaximize),
        roles,
        false,
        false
      ),
      app.i18n.t(tips::maximize(app.window.is_maximized))
    ),
    tip(
      title_button(icons::WIN_CLOSE, chrome(Action::Close), roles, true, false),
      app.i18n.t(tips::CLOSE)
    ),
  ]
  .spacing(compact::BUTTON_GAP)
  .align_y(Vertical::Center);
  let drag = mouse_area(container(space()).width(Fill).height(Fill)).on_press(chrome(Action::Drag));
  row![drag, controls]
    .padding([0.0, compact::TITLEBAR_PADDING])
    .align_y(Vertical::Center)
    .height(compact::TITLEBAR_HEIGHT)
    .into()
}

/// Bouton de commande : glyphe centré, focus clavier, activation par message.
fn control<'a>(
  glyph: &'a str,
  message: Message,
  size: f32,
  radius: f32,
  style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, Message> {
  let widget = button(container(icons::glyph(glyph, 16.0)).center(Fill))
    .padding(0)
    .width(size)
    .height(size)
    .style(style)
    .on_press(message.clone());
  focusable(widget)
    .on_activate(Some(message))
    .radius(radius)
    .into()
}

/// `#btn-prev`, `#btn-next` : `surface-container-high`, texte `on-surface` (`styles.css:3010-3023`).
fn skip<'a>(glyph: &'a str, message: Message, roles: Roles, size: f32) -> Element<'a, Message> {
  control(glyph, message, size, size / 2.0, move |_, status| {
    let share = match status {
      button::Status::Hovered => 0.08,
      button::Status::Pressed => 0.12,
      _ => 0.0,
    };
    let base = if share > 0.0 {
      roles.highest
    } else {
      roles.high
    };
    button::Style {
      background: Some(Background::Color(overlay(base, roles.on_surface, share))),
      text_color: roles.on_surface,
      border: border::rounded(state_radius(size, status)),
      ..button::Style::default()
    }
  })
}

/// `.btn-play-pause` : `primary` (92 % au survol, mélangé de blanc), ou transparent avec `play-btn-nobg`.
fn play<'a>(app: &App, glyph: &'a str, roles: Roles, size: f32) -> Element<'a, Message> {
  let playing = app.playback.is_playing;
  let radius = if playing {
    compact::PLAY_RADIUS_PLAYING
  } else {
    compact::PLAY_RADIUS_PAUSED
  };
  let filled = app.settings.play_btn_bg;
  control(
    glyph,
    player(PlayerMessage::TogglePlay),
    size,
    radius,
    move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      let (background, label) = if filled {
        let fill = if hovered {
          blend(roles.primary, Color::WHITE, 0.92)
        } else {
          roles.primary
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
    },
  )
}

fn controls<'a>(app: &App, roles: Roles, metrics: &compact::Metrics) -> Element<'a, Message> {
  let play_glyph = if app.player.loading || app.playback.buffering {
    "\u{2026}"
  } else if app.playback.is_playing {
    icons::PAUSE
  } else {
    icons::PLAY
  };
  let prev = container(tip(
    skip(
      icons::PREVIOUS,
      player(PlayerMessage::Prev),
      roles,
      metrics.control,
    ),
    app.i18n.t(tips::PREVIOUS),
  ))
  .padding(Padding {
    left: compact::SKIP_MARGIN,
    ..Padding::ZERO
  });
  let next = container(tip(
    skip(
      icons::NEXT,
      player(PlayerMessage::Next),
      roles,
      metrics.control,
    ),
    app.i18n.t(tips::NEXT),
  ))
  .padding(Padding {
    right: compact::SKIP_MARGIN,
    ..Padding::ZERO
  });
  let play = container(tip(
    play(app, play_glyph, roles, metrics.play),
    app.i18n.t(tips::play(app.playback.is_playing)),
  ))
  .center_x(compact::PLAY_WRAPPER)
  .center_y(metrics.play);
  row![prev, play, next]
    .spacing(compact::CONTROLS_GAP)
    .align_y(Vertical::Center)
    .into()
}

/// Pochette de la piste : carré à coins de 12 px, ou forme (`#m3c-canvas`) ramenée à la taille de la pochette.
fn cover<'a>(app: &App, roles: Roles, track: &Track, side: f32) -> Element<'a, Message> {
  let picture = app
    .cover_of(track)
    .and_then(|url| app.images.get(&ImageKey::new(&url, SIZE_CARD)).cloned());
  if !app.m3_cover_on() || app.lyrics.tvk_open {
    // `body.mini .artwork-wrapper` : `border-radius: var(--md-shape-corner-medium)` (`styles.css:7026`).
    return match picture {
      Some(handle) => image(handle)
        .width(side)
        .height(side)
        .content_fit(iced::ContentFit::Cover)
        .border_radius(tokens::radius::M)
        .into(),
      None => container(space())
        .width(side)
        .height(side)
        .style(move |_| {
          container::Style::default()
            .background(roles.highest)
            .border(border::rounded(tokens::radius::M))
        })
        .into(),
    };
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
  let program = ShapeCover {
    radii: shape_radii(&app.visuals.shape, &PROFILES),
    rotation: app.visuals.shape.rotation,
    phase: app.visuals.shape.phase,
    progress,
    has_image: picture.is_some(),
    ring: roles.primary,
    background: roles.panel,
    fill: roles.highest,
    cover: side * compact::SHAPE_SHARE,
    hover,
    unit: side / crate::visuals::canvas::CANVAS_CSS,
    center: None,
    layer: CoverLayer::Both,
  };
  let shape = canvas(program).width(side).height(side);
  // `#playbar-artwork-wrapper` est le carré de la pochette (`styles.css:7016`) : il ne bouge pas pendant le survol.
  let hitbox = mouse_area(space().width(side).height(side))
    .on_enter(Message::Ui(crate::ui::UiMessage::CoverHover(true)))
    .on_exit(Message::Ui(crate::ui::UiMessage::CoverHover(false)));
  match picture {
    Some(handle) => {
      // `imgSize = Math.ceil(currentCover * 1.06)` (`app.js:16910`) pour une pochette de 70 px sur 120, levée de 4 px sur 120.
      let under = (side
        * compact::SHAPE_SHARE
        * (1.0 + tokens::halo::HOVER_SCALE * hover)
        * tokens::halo::IMAGE_SCALE)
        .ceil();
      let under = container(
        image(handle)
          .width(under)
          .height(under)
          .content_fit(iced::ContentFit::Cover),
      )
      .padding(Padding {
        bottom: 2.0 * tokens::halo::HOVER_LIFT * side / 120.0 * hover,
        ..Padding::ZERO
      })
      .center(side);
      stack![under, shape, hitbox].into()
    }
    None => stack![shape, hitbox].into(),
  }
}

/// Titre et artiste (`.track-meta`) : colonne centrée, `gap: 2px`, texte coupé par « … ».
fn meta<'a>(
  app: &App,
  roles: Roles,
  metrics: &compact::Metrics,
  width: f32,
) -> Element<'a, Message> {
  let (title, artist) = match app.player.current.as_ref() {
    Some(track) => (
      track.title_text().to_string(),
      track.artist.clone().unwrap_or_default(),
    ),
    None => (
      app.i18n.t("No track playing"),
      app.i18n.t("Select a track to listen"),
    ),
  };
  let title = text(ellipsize(&title, metrics.title, fonts::SEMIBOLD, width))
    .size(metrics.title)
    .font(fonts::SEMIBOLD)
    .line_height(LineHeight::Relative(tokens::playbar::LINE_HEIGHT))
    .wrapping(text::Wrapping::None)
    .color(roles.on_surface);
  // `body.mini .track-artist` : `opacity: 0.7` (`styles.css:7091`).
  let artist = text(ellipsize(&artist, metrics.artist, fonts::UI, width))
    .size(metrics.artist)
    .line_height(LineHeight::Relative(tokens::playbar::LINE_HEIGHT))
    .wrapping(text::Wrapping::None)
    .color(blend(roles.variant, roles.panel, 0.7));
  column![title, artist]
    .spacing(compact::META_GAP)
    .width(width)
    .into()
}

/// Ligne ondulée `#squiggly-canvas` : 18 px de haut, bitmap de 40 px écrasé.
fn scrubber<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  let (duration, shown) = scrub_extent(app);
  let program = Scrubber {
    duration,
    progress: app.scrub_progress(duration, shown),
    phase: app.wave_phase(),
    wavy: !crate::motion::perf(),
    primary: roles.primary,
    track: roles.on_surface,
    squash: compact::SCRUBBER_HEIGHT / tokens::css(compact::SCRUBBER_BITMAP_HEIGHT),
  };
  container(canvas(program).width(Fill).height(compact::SCRUBBER_HEIGHT))
    .padding(Padding {
      bottom: compact::SCRUBBER_PADDING,
      ..Padding::ZERO
    })
    .into()
}

/// `.playbar` en mode compact : grille de trois colonnes (pochette, titre, commandes), puis une ligne de paroles
/// facultative, puis la ligne ondulée.
fn playbar<'a>(
  app: &'a App,
  roles: Roles,
  metrics: &compact::Metrics,
  window_height: f32,
) -> Element<'a, Message> {
  let width = app.window.size.unwrap_or(MINI_SIZE).width;
  let lyrics_on = app.window.mini_lyrics;
  let cell = metrics.cover + 2.0 * compact::COVER_MARGIN;
  // Avec les paroles, la première rangée mesure sur 17b 4 px CSS de moins que pochette + 2 marges : la marge basse
  // de la pochette n'y compte pas (déduit de la position des paroles, la règle CSS lue donne 4 px de plus).
  let below = if lyrics_on {
    0.0
  } else {
    compact::COVER_MARGIN
  };
  let cell_height = cell - compact::COVER_MARGIN + below;
  let cover: Element<'a, Message> = match app.player.current.as_ref() {
    Some(track) => container(cover(app, roles, track, metrics.cover))
      .padding(Padding {
        top: compact::COVER_MARGIN,
        right: compact::COVER_MARGIN,
        bottom: below,
        left: compact::COVER_MARGIN,
      })
      .into(),
    None => space().width(cell).height(cell_height).into(),
  };
  let meta_width =
    (width - 2.0 * metrics.pad_x - cell - 2.0 * metrics.col_gap - metrics.controls_width())
      .max(0.0);
  let top = row![
    cover,
    container(meta(app, roles, metrics, meta_width)).center_y(Fill),
    container(controls(app, roles, metrics)).align_right(Fill),
  ]
  .spacing(metrics.col_gap)
  .align_y(Vertical::Center);
  let mut grid = column![].spacing(metrics.row_gap);
  if lyrics_on {
    let chrome = compact::TITLEBAR_HEIGHT
      + metrics.pad_top
      + metrics.pad_bottom
      + cell_height
      + 2.0 * metrics.row_gap
      + compact::SCRUBBER_HEIGHT
      + compact::SCRUBBER_PADDING;
    let area = (window_height - chrome).max(0.0);
    grid = grid
      .push(top.height(cell_height))
      .push(lyrics(app, roles, area))
      .push(scrubber(app, roles));
  } else {
    grid = grid.push(top.height(Fill)).push(scrubber(app, roles));
  }
  container(grid)
    .padding(Padding {
      top: metrics.pad_top,
      right: metrics.pad_x,
      bottom: metrics.pad_bottom,
      left: metrics.pad_x,
    })
    .width(Fill)
    .height(Fill)
    .style(move |_| container::Style::default().background(roles.panel))
    .into()
}

/// Hauteur d'une ligne de `#mini-lyrics-container` : interligne 1,5 et `padding: 3px 0`.
fn line_height(size: f32) -> f32 {
  size * compact::LINE_HEIGHT + 2.0 * compact::LINE_PADDING
}

/// Hauteur d'une rangée : une ligne de pause ne garde que son `padding`.
fn row_height(gap: bool, active: bool) -> f32 {
  if gap {
    2.0 * compact::LINE_PADDING
  } else {
    line_height(if active {
      compact::LINE_SIZE_ACTIVE
    } else {
      compact::LINE_SIZE
    })
  }
}

/// Première ligne affichée : la ligne active reste au milieu (`scrollTo` centré, `app.js:5480`), sans dépasser le haut
/// ni le bas de la liste. Sans ligne active, la liste est en haut.
fn first_line(count: usize, visible: usize, active: Option<usize>) -> usize {
  match active {
    Some(active) => active
      .saturating_sub(visible / 2)
      .min(count.saturating_sub(visible)),
    None => 0,
  }
}

/// `#mini-lyrics-container` : `area` pixels de haut, `padding: 8px 0`.
fn lyrics<'a>(app: &'a App, roles: Roles, area: f32) -> Element<'a, Message> {
  let state = &app.lyrics;
  let inner = (area - 2.0 * compact::LYRICS_PADDING).max(0.0);
  let lines = state
    .lyrics
    .as_ref()
    .filter(|lyrics| !lyrics.lines.is_empty() && state.layout.lines.len() == lyrics.lines.len());
  let Some(lyrics) = lines else {
    // `.mini-lyrics-empty` : centré, `outline` à 0,6 (`styles.css:7209-7220`).
    return container(
      text(app.i18n.t("No lyrics"))
        .size(compact::LINE_SIZE)
        .font(fonts::MEDIUM)
        .color(alpha(roles.outline, compact::EMPTY_OPACITY)),
    )
    .center(Fill)
    .into();
  };
  let pitch = line_height(compact::LINE_SIZE);
  // La ligne active est plus haute (15 px) : sa place est retirée avant de compter les lignes qui tiennent.
  let extra = if state.active.is_some() {
    row_height(false, true) - pitch
  } else {
    0.0
  };
  let visible = (((inner - extra) / pitch).floor() as usize).max(1);
  let first = first_line(lyrics.lines.len(), visible, state.active);
  // Les lignes s'ajoutent tant qu'elles tiennent (tolérance : l'arrondi des tailles écran).
  let mut used = 0.0;
  let mut rows: Vec<Element<'a, Message>> = Vec::new();
  for index in first..lyrics.lines.len() {
    let active = state.active == Some(index);
    let gap = lyrics.lines[index].is_gap;
    let height = row_height(gap, active);
    if !rows.is_empty() && used + height > inner + 0.5 {
      break;
    }
    used += height;
    let body: Element<'a, Message> = if gap {
      // `.lyric-gap-box` n'a aucune règle dans Riff : la ligne se réduit à son `padding`.
      space().width(Fill).height(0).into()
    } else {
      line(app, roles, index, active)
    };
    let padded = container(body)
      .padding([compact::LINE_PADDING, 0.0])
      .width(Fill);
    rows.push(
      mouse_area(padded)
        .on_press(Message::Lyrics(
          crate::lyrics::update::LyricsMessage::LineClicked(index),
        ))
        .into(),
    );
  }
  container(column(rows).width(Fill))
    .padding([compact::LYRICS_PADDING, 0.0])
    .width(Fill)
    .height(Fill)
    .clip(true)
    .into()
}

/// Une ligne de paroles : `.mini-lyric-line`, ses mots en `span` (`renderMiniLyrics`, `app.js:5389-5445`).
fn line<'a>(app: &'a App, roles: Roles, index: usize, active: bool) -> Element<'a, Message> {
  let state = &app.lyrics;
  let layout = &state.layout.lines[index];
  let size = if active {
    compact::LINE_SIZE_ACTIVE
  } else {
    compact::LINE_SIZE
  };
  let font = Font {
    weight: if active {
      iced::font::Weight::Bold
    } else {
      iced::font::Weight::Medium
    },
    ..fonts::UI
  };
  // Opacité de la ligne entière, puis celle de chaque mot de la ligne active.
  let base = if active { 1.0 } else { compact::LINE_OPACITY };
  let states = if active && state.word_mode {
    state.word_states(index)
  } else {
    Vec::new()
  };
  let mut spans: Vec<Span<'a, (), Font>> = Vec::with_capacity(layout.words.len() * 2);
  for (position, word) in layout.words.iter().enumerate() {
    let (color, opacity) = match states.get(position) {
      Some(WordState::Past) => (roles.on_surface, 1.0),
      Some(WordState::Active) => (roles.primary, 1.0),
      Some(WordState::Future) => (roles.primary, compact::WORD_OPACITY),
      None if active => (roles.primary, 1.0),
      None => (roles.on_surface, 1.0),
    };
    // `.mini-lyric-line .lyric-paren` : 0,68 em et opacité 0,55, sauf mot actif.
    let paren = word.paren;
    let paren_opacity = if paren && !matches!(states.get(position), Some(WordState::Active)) {
      0.55
    } else {
      1.0
    };
    if position > 0 {
      spans.push(Span::new(" ").color(alpha(
        if active {
          roles.primary
        } else {
          roles.on_surface
        },
        base,
      )));
    }
    let word_size = if paren {
      size * compact::PAREN_SCALE
    } else {
      size
    };
    spans.push(
      Span::new(word.text.as_str())
        .size(word_size)
        .color(alpha(color, base * opacity * paren_opacity)),
    );
  }
  rich_text(spans)
    .size(size)
    .font(font)
    .line_height(LineHeight::Relative(compact::LINE_HEIGHT))
    .width(Fill)
    .align_x(Horizontal::Center)
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_active_line_stays_in_the_middle_of_the_visible_lines() {
    assert_eq!(first_line(20, 7, None), 0);
    assert_eq!(first_line(20, 7, Some(1)), 0);
    assert_eq!(first_line(20, 7, Some(10)), 7);
    assert_eq!(first_line(20, 7, Some(19)), 13);
    assert_eq!(first_line(3, 7, Some(2)), 0);
  }

  #[test]
  fn the_metrics_match_the_measured_windows() {
    // `ui-reference/screens/17a` : 380x160 -> vmin 1,693 ; pochette 48 px CSS (45,36), contrôles 32, lecture 40.
    let small = compact::Metrics::of(380.0, 160.0);
    assert!((small.cover - tokens::css(48.0)).abs() < 1e-3);
    assert!((small.control - tokens::css(32.0)).abs() < 1e-3);
    assert!((small.play - tokens::css(40.0)).abs() < 1e-3);
    assert!((small.pad_x - tokens::css(12.0)).abs() < 1e-3);
    // `17b` : 380x360 -> vmin 3,81 ; pochette 24 vmin = 91,4 px CSS, contrôles 32,4, lecture 41,9, titre 13,7.
    let tall = compact::Metrics::of(380.0, 360.0);
    assert!((tall.cover - tokens::css(91.43)).abs() < 0.1);
    assert!((tall.control - tokens::css(32.38)).abs() < 0.1);
    assert!((tall.play - tokens::css(41.9)).abs() < 0.1);
    assert!((tall.title - tokens::css(13.71)).abs() < 0.1);
  }

  #[test]
  fn the_lyrics_button_resizes_the_compact_window_like_riff() {
    use crate::window::{MINI_LYRICS_MIN, MINI_PLAIN_MIN};
    let mut window = crate::window::WindowTracker::default();
    assert!(
      window.toggle_mini_lyrics().is_none(),
      "hors du mode compact"
    );
    assert!(window.enter_mini());
    window.resized(MINI_SIZE);
    let (min, size) = window.toggle_mini_lyrics().expect("mode compact");
    assert_eq!(
      (min, size.width, size.height),
      (MINI_LYRICS_MIN, 380.0, 360.0)
    );
    window.resized(size);
    let (min, size) = window.toggle_mini_lyrics().expect("mode compact");
    assert_eq!((min, size.height), (MINI_PLAIN_MIN, 160.0));
  }
}
