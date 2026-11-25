//! Ossature de la fenêtre de Riff 44.1.1 : barre de titre, champ de recherche en pastille, boutons
//! de plateforme et panneau central arrondi (`index.html:329-390`, `styles.css:225-440`,
//! `10600-10690`). Les dimensions sont en pixels CSS convertis par [`tokens::css`] ; voir
//! [`tokens::shell`].
//!
//! Dans Riff, la barre latérale occupe toute la hauteur au-dessus de la barre de lecture, puis la
//! barre de titre et la barre de navigation se posent à sa droite. Le panneau central n'est pas un
//! élément arrondi : la page (`surface-dim`) est découpée par les coins concaves de la barre latérale
//! et de la barre de lecture, sur un fond de panneau (`surface-container-low`).

use super::chrome::{SEARCH_ID, SIDEBAR_EDGE_WIDTH, mini_button_specs, tip, tips};
use crate::app::{App, DEFAULT_WINDOW, Message};
use crate::images::{ImageKey, SIZE_ROW};
use crate::theme::tokens::shell;
use crate::theme::{fonts, icons, tokens};
use crate::ui::UiMessage;
use crate::widgets::button::icon_px;
use crate::widgets::focus::focusable;
use crate::widgets::view_transition::view_transition;
use crate::window::Action;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::LineHeight;
use iced::widget::text_input::{self, Status};
use iced::widget::{button, column, container, image, mouse_area, row, space, stack, text};
use iced::{
  Background, Border, Color, ContentFit, Element, Fill, Gradient, Padding, Radians, Theme, border,
  gradient, mouse,
};
use material_colors::color::Rgb;

/// Bandeau de rappel de la barre de titre masquée (`body.hide-titlebar::before`).
const TITLEBAR_EDGE: f32 = 6.0;

/// Rôles de couleur de Riff utilisés par le haut de fenêtre (`--md-sys-color-*`).
#[derive(Clone, Copy)]
struct Roles {
  on_surface: Color,
  variant: Color,
  outline: Color,
  container: Color,
  highest: Color,
  high: Color,
  secondary_container: Color,
  on_secondary_container: Color,
  primary: Color,
  error: Color,
  on_error: Color,
  /// `--page-solid` : fond de la page.
  page: Color,
  /// `--panel-bg` : fond de la barre latérale et de la barre de lecture.
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
      highest: rgb(s.surface_container_highest),
      high: rgb(s.surface_container_high),
      secondary_container: rgb(s.secondary_container),
      on_secondary_container: rgb(s.on_secondary_container),
      primary: rgb(s.primary),
      error: rgb(s.error),
      on_error: rgb(s.on_error),
      page: rgb(s.surface_dim),
      panel: rgb(s.surface_container_low),
    }
  }
}

/// Calque `--hl` de Riff : `share` de `on_surface` posé sur `base` (`styles.css:7501`).
pub(super) fn overlay(base: Color, on_surface: Color, share: f32) -> Color {
  Color::from_rgb(
    on_surface.r * share + base.r * (1.0 - share),
    on_surface.g * share + base.g * (1.0 - share),
    on_surface.b * share + base.b * (1.0 - share),
  )
}

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

fn sidebar_width(app: &App) -> f32 {
  tokens::css(app.data.sidebar_width as f32)
}

/// Barre de titre masquée (`riff_adv_hide_titlebar`) tant que la souris n'est pas au bord haut.
fn titlebar_hidden(app: &App) -> bool {
  app.settings.hide_titlebar && !app.ui.titlebar_hovered
}

fn titlebar_height(app: &App) -> f32 {
  if titlebar_hidden(app) {
    TITLEBAR_EDGE
  } else {
    shell::TITLEBAR_HEIGHT
  }
}

/// Largeur du bloc des plateformes (`--engine-w`) et marge qui le sépare du champ (`--engine-gap`).
fn engine_space(app: &App) -> (f32, f32) {
  if app.settings.hide_engine_btns {
    (0.0, 0.0)
  } else if app.settings.compact_engine_btns {
    (shell::ENGINE_WIDTH_COMPACT, shell::ENGINE_GAP)
  } else {
    (shell::ENGINE_WIDTH, shell::ENGINE_GAP)
  }
}

/// Bouton de fenêtre `.titlebar-btn.win-btn` : fond au survol seulement, rouge pour la fermeture.
fn window_button<'a>(
  glyph: &'a str,
  message: Message,
  roles: Roles,
  closes: bool,
) -> Element<'a, Message> {
  let content = container(icons::glyph_px(glyph, 14.0)).center(Fill);
  let widget = button(content)
    .padding(0)
    .width(shell::WINDOW_BUTTON_WIDTH)
    .height(shell::WINDOW_BUTTON_HEIGHT)
    .style(move |_, status| {
      let (fill, label) = if closes {
        (roles.error, roles.on_error)
      } else {
        (roles.high, roles.on_surface)
      };
      let (background, text_color, share) = match status {
        button::Status::Hovered => (Some(fill), label, 0.08),
        button::Status::Pressed => (Some(fill), label, 0.12),
        button::Status::Active | button::Status::Disabled => (None, roles.variant, 0.0),
      };
      button::Style {
        background: background
          .map(|color| Background::Color(overlay(color, roles.on_surface, share))),
        text_color,
        border: border::rounded(if background.is_some() {
          shell::WINDOW_BUTTON_HOVER_RADIUS
        } else {
          shell::WINDOW_BUTTON_HEIGHT / 2.0
        }),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  focusable(widget)
    .ripple(
      glyph,
      if closes {
        roles.on_error
      } else {
        roles.on_surface
      },
      shell::WINDOW_BUTTON_HOVER_RADIUS,
    )
    .on_activate(Some(message))
    .radius(shell::WINDOW_BUTTON_HOVER_RADIUS)
    .into()
}

/// `#titlebar` : zone de glissement, logo au centre de la fenêtre, boutons à droite.
fn titlebar(app: &App, roles: Roles) -> Element<'_, Message> {
  let chrome = Message::Chrome;
  let maximize = if app.window.is_maximized {
    icons::RESTORE
  } else {
    icons::MAXIMIZE
  };
  let [_, mini] = mini_button_specs();
  // `#btn-mini-lyrics` n'existe à l'écran qu'en mode compact (`styles.css:7163`).
  let controls = row![
    tip(
      window_button(icons::MINI_PLAYER, mini.2, roles, false),
      app.i18n.t(mini.1)
    ),
    tip(
      window_button(icons::MINIMIZE, chrome(Action::Minimize), roles, false),
      app.i18n.t(tips::MINIMIZE)
    ),
    tip(
      window_button(maximize, chrome(Action::ToggleMaximize), roles, false),
      app.i18n.t(tips::maximize(app.window.is_maximized))
    ),
    tip(
      window_button(icons::WIN_CLOSE, chrome(Action::Close), roles, true),
      app.i18n.t(tips::CLOSE)
    ),
  ]
  .spacing(shell::WINDOW_BUTTON_GAP)
  .align_y(Vertical::Center);
  let drag = mouse_area(container(space()).width(Fill).height(Fill))
    .on_press(chrome(Action::Drag))
    .on_double_click(chrome(Action::ToggleMaximize));
  let bar = row![drag, controls]
    .padding([0.0, shell::TITLEBAR_PADDING])
    .align_y(Vertical::Center);
  // `.app-logo` : centre de la fenêtre (`left: calc(50vw - var(--sidebar-w))`), pas de la barre.
  let logo = container(icons::glyph_px(icons::BANNER_LOGO, 20.0))
    .style(move |_| container::Style::default().color(roles.primary));
  let logo = container(logo).center(Fill).padding(Padding {
    right: sidebar_width(app),
    ..Padding::ZERO
  });
  let titlebar = stack![bar, logo].width(Fill).height(shell::TITLEBAR_HEIGHT);
  if app.settings.hide_titlebar {
    mouse_area(titlebar)
      .on_exit(ui(UiMessage::TitlebarHover(false)))
      .into()
  } else {
    titlebar.into()
  }
}

/// Style du champ : la pastille `.search-box` est le champ lui-même, `surface-container`, puis
/// `surface-container-highest` au focus (`.search-box:focus-within`).
fn search_style(roles: Roles) -> impl Fn(&Theme, Status) -> text_input::Style {
  move |theme, status| text_input::Style {
    background: Background::Color(if matches!(status, Status::Focused { .. }) {
      roles.highest
    } else {
      roles.container
    }),
    border: Border {
      radius: (shell::SEARCH_HEIGHT / 2.0).into(),
      width: 0.0,
      color: Color::TRANSPARENT,
    },
    icon: roles.on_surface,
    placeholder: roles.outline,
    value: roles.on_surface,
    selection: theme.extended_palette().primary.weak.color,
  }
}

/// `.search-box` : icône, champ et bouton d'effacement.
fn search_box(app: &App, roles: Roles) -> Element<'_, Message> {
  let search = &app.ui.search;
  let clear_size = tokens::css(22.0);
  let has_text = !search.input.is_empty();
  let cover = search
    .icon_cover
    .as_deref()
    .map(|url| app.images.get(&ImageKey::new(url, SIZE_ROW)));
  let icon_width = if cover.is_some() {
    tokens::search_results::ICON_COVER
  } else {
    tokens::search_results::ICON_LOUPE
  };
  let left = shell::SEARCH_PADDING + icon_width + shell::SEARCH_ICON_GAP;
  let right = shell::SEARCH_PADDING + if has_text { clear_size } else { 0.0 };
  let field = text_input::TextInput::new(
    &app.i18n.t("Search tracks, artists, or albums..."),
    &search.input,
  )
  .id(SEARCH_ID)
  .on_input(|value| ui(UiMessage::SearchInput(value)))
  .on_submit(ui(UiMessage::SearchSubmit))
  .size(shell::SEARCH_TEXT)
  .line_height(LineHeight::Absolute(shell::SEARCH_HEIGHT.into()))
  .padding(Padding {
    top: 0.0,
    right,
    bottom: 0.0,
    left,
  })
  .style(search_style(roles));
  let field = focusable(field)
    .observe_press(ui(UiMessage::SearchFocus))
    .radius(shell::SEARCH_HEIGHT / 2.0);
  // `.search-icon-cover` remplace la loupe tant que la dernière recherche a une pochette.
  let icon: Element<'_, Message> = match cover {
    Some(Some(handle)) => image(handle.clone())
      .width(tokens::search_results::ICON_COVER)
      .height(tokens::search_results::ICON_COVER)
      .content_fit(ContentFit::Cover)
      .border_radius(tokens::search_results::ICON_COVER_RADIUS)
      .into(),
    Some(None) => space()
      .width(tokens::search_results::ICON_COVER)
      .height(tokens::search_results::ICON_COVER)
      .into(),
    None => container(icons::glyph_px(icons::SEARCH, 18.0))
      .style(move |_| container::Style::default().color(roles.variant))
      .into(),
  };
  // `#search-icon-btn` : un clic relance la recherche du texte saisi (`app.js:14442`).
  let mut icon = mouse_area(icon).interaction(mouse::Interaction::Pointer);
  if has_text {
    icon = icon.on_press(ui(UiMessage::SearchSubmit));
  }
  let glyph = container(icon)
    .width(Fill)
    .height(Fill)
    .align_y(Vertical::Center)
    .padding(Padding {
      left: shell::SEARCH_PADDING,
      ..Padding::ZERO
    });
  let mut layers = stack![field, glyph]
    .width(Fill)
    .height(shell::SEARCH_HEIGHT);
  if has_text {
    let clear = icon_px(
      icons::SEARCH_CLEAR,
      14.0,
      Some(ui(UiMessage::SearchClear)),
      clear_size,
      clear_size,
    );
    layers = layers.push(
      container(clear)
        .width(Fill)
        .height(Fill)
        .align_x(Horizontal::Right)
        .align_y(Vertical::Center)
        .padding(Padding {
          right: shell::SEARCH_PADDING,
          ..Padding::ZERO
        }),
    );
  }
  layers.into()
}

/// `.platform-btn` : pastille tonale quand la plateforme est active.
fn platform_button<'a>(
  app: &'a App,
  roles: Roles,
  label: &str,
  glyph: &'a str,
  id: &str,
) -> Element<'a, Message> {
  let active = app.data.platform == id;
  let compact = app.settings.compact_engine_btns;
  let mut content = row![icons::glyph_px(glyph, 16.0)]
    .spacing(tokens::css(8.0))
    .align_y(Vertical::Center);
  if !compact {
    content = content.push(
      text(label.to_string())
        .size(shell::PLATFORM_TEXT)
        .font(if active { fonts::BOLD } else { fonts::MEDIUM }),
    );
  }
  let (pad_x, pad_y) = if compact {
    (tokens::css(10.0), tokens::css(8.0))
  } else {
    (tokens::css(16.0), 0.0)
  };
  let entry = button(container(content).center_y(Fill))
    .padding([pad_y, pad_x])
    .height(shell::PLATFORM_BUTTON_HEIGHT)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: active.then_some(Background::Color(roles.secondary_container)),
        text_color: if active {
          roles.on_secondary_container
        } else if hovered {
          roles.on_surface
        } else {
          roles.variant
        },
        border: border::rounded(shell::PLATFORM_BUTTON_HEIGHT / 2.0),
        ..button::Style::default()
      }
    })
    .on_press(ui(UiMessage::SetPlatform(id.to_string())));
  // Sans texte visible (boutons compacts), le nom de la plateforme passe en infobulle.
  if compact {
    tip(entry.into(), label.to_string())
  } else {
    entry.into()
  }
}

/// `.platform-pill-switch`.
fn platform_switch(app: &App, roles: Roles) -> Element<'_, Message> {
  let mut pills = row![].spacing(shell::PLATFORM_PILL_PADDING);
  pills = pills.push(platform_button(
    app,
    roles,
    "YouTube Music",
    icons::PLATFORM_YTM,
    "ytmusic",
  ));
  pills = pills.push(platform_button(
    app,
    roles,
    "SoundCloud",
    icons::PLATFORM_SC,
    "soundcloud",
  ));
  if app.data.show_yt_platform {
    pills = pills.push(platform_button(
      app,
      roles,
      "YouTube",
      icons::PLATFORM_YT,
      "youtube",
    ));
  }
  container(pills)
    .padding(shell::PLATFORM_PILL_PADDING)
    .style(move |_| {
      container::Style::default()
        .background(roles.container)
        .border(border::rounded(
          shell::PLATFORM_BUTTON_HEIGHT / 2.0 + shell::PLATFORM_PILL_PADDING,
        ))
    })
    .into()
}

/// `.top-nav` : champ de recherche (centre, gauche ou droite selon `riff_search_pos`) et plateformes.
fn top_nav(app: &App, roles: Roles) -> Element<'_, Message> {
  let sidebar = sidebar_width(app);
  let (engine, gap) = engine_space(app);
  let wrapper = container(search_box(app, roles))
    .width(Fill)
    .max_width(shell::SEARCH_MAX_WIDTH);
  let (align, padding) = match app.data.search_pos.as_str() {
    "left" => (
      Horizontal::Left,
      Padding {
        left: shell::TOP_NAV_PADDING,
        right: shell::TOP_NAV_PADDING + engine + gap,
        ..Padding::ZERO
      },
    ),
    "right" => (
      Horizontal::Right,
      Padding {
        left: shell::TOP_NAV_PADDING,
        right: shell::TOP_NAV_PADDING + engine + gap,
        ..Padding::ZERO
      },
    ),
    // Centré sur la fenêtre et non sur la barre : `left: calc(50vw - var(--sidebar-w))`.
    _ => {
      let reserved = engine + tokens::css(36.0);
      let left = (reserved - sidebar).max(0.0);
      (
        Horizontal::Center,
        Padding {
          left,
          right: left + sidebar,
          ..Padding::ZERO
        },
      )
    }
  };
  let search = container(wrapper)
    .width(Fill)
    .height(Fill)
    .align_x(align)
    .align_y(Vertical::Center)
    .padding(padding);
  let mut layers = stack![search].width(Fill).height(shell::TOP_NAV_HEIGHT);
  if !app.settings.hide_engine_btns {
    layers = layers.push(
      container(platform_switch(app, roles))
        .width(Fill)
        .height(Fill)
        .align_x(Horizontal::Right)
        .align_y(Vertical::Center)
        .padding(Padding {
          right: shell::TOP_NAV_PADDING,
          ..Padding::ZERO
        }),
    );
  }
  layers.into()
}

/// Barre de titre puis barre de navigation, à droite de la barre latérale.
fn top_bar(app: &App, roles: Roles) -> Element<'_, Message> {
  let title: Element<'_, Message> = if titlebar_hidden(app) {
    mouse_area(container(space()).width(Fill).height(TITLEBAR_EDGE))
      .on_enter(ui(UiMessage::TitlebarHover(true)))
      .into()
  } else {
    titlebar(app, roles)
  };
  column![title, top_nav(app, roles)].into()
}

/// Dégradé vertical de la page vers la transparence : le masque de `#main-content`. L'angle d'iced se lit
/// depuis le haut : `to_distance` retranche `FRAC_PI_2` (`iced_core-0.14.0/src/angle.rs:87`), donc `PI`
/// va du haut vers le bas et `FRAC_PI_2` serait horizontal.
fn fade(color: Color, from_top: bool) -> Element<'static, Message> {
  let clear = Color { a: 0.0, ..color };
  let linear = gradient::Linear::new(Radians::PI);
  let linear = if from_top {
    linear.add_stop(0.0, color).add_stop(1.0, clear)
  } else {
    linear.add_stop(0.0, clear).add_stop(1.0, color)
  };
  container(space())
    .width(Fill)
    .height(if from_top {
      shell::FADE_TOP
    } else {
      shell::FADE_BOTTOM
    })
    .style(move |_| container::Style::default().background(Gradient::Linear(linear)))
    .into()
}

/// Zone au-dessus de la barre de lecture : barre latérale pleine hauteur, puis la page arrondie
/// (barre de titre, barre de navigation, écran et panneau droit).
pub fn main_area<'a>(
  app: &'a App,
  sidebar: Element<'a, Message>,
  edge: Element<'a, Message>,
  screen: Element<'a, Message>,
  side: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
  let roles = Roles::of(app);
  let has_background = app.settings.visual.has_background();
  let glow = super::backdrop::hero_glow(app, roles.page, sidebar_width(app));
  // `m3ContainerOpen` : le fondu est un voile de la couleur de la page, exact seulement sur le fond uni (ni image ni halo).
  let veil = (!has_background && glow.is_none()).then_some(roles.page);
  let screen: Element<'a, Message> =
    view_transition(screen, app.view_motion.pose(app.now), veil).into();
  // Le halo remplace les fondus de la page : `#main-content` les fait par un masque, qui laisse voir le halo.
  let screen = if has_background || glow.is_some() {
    screen
  } else {
    stack![
      screen,
      container(fade(roles.page, true))
        .width(Fill)
        .height(Fill)
        .align_y(Vertical::Top),
      container(fade(roles.page, false))
        .width(Fill)
        .height(Fill)
        .align_y(Vertical::Bottom),
    ]
    .width(Fill)
    .height(Fill)
    .into()
  };
  let mut body = iced::widget::Row::new()
    .push(screen)
    .spacing(shell::PANEL_GAP)
    .padding(Padding {
      right: shell::PANEL_PADDING_RIGHT,
      ..Padding::ZERO
    })
    .height(Fill);
  if let Some(side) = side {
    body = body.push(side);
  }
  let content: Element<'a, Message> = column![top_bar(app, roles), body].into();
  let content = match glow {
    Some(glow) => stack![glow, content].width(Fill).height(Fill).into(),
    None => content,
  };
  let page = container(content).width(Fill).height(Fill).style(move |_| {
    if has_background {
      return container::Style::default();
    }
    // Coins concaves de la barre latérale (haut et bas) et de la barre de lecture (droite, bas).
    let radius = border::Radius {
      top_left: shell::PANEL_RADIUS,
      top_right: 0.0,
      bottom_right: shell::PANEL_RADIUS,
      bottom_left: shell::PANEL_RADIUS,
    };
    container::Style::default()
      .background(roles.page)
      .border(border::Border::default().rounded(radius))
  });
  // Le bord de la barre latérale chevauche la barre : il est centré sur sa limite droite.
  let grip = container(edge).width(Fill).height(Fill).padding(Padding {
    left: (sidebar_width(app) - SIDEBAR_EDGE_WIDTH / 2.0).max(0.0),
    ..Padding::ZERO
  });
  container(stack![row![sidebar, page].height(Fill), grip])
    .width(Fill)
    .height(Fill)
    .style(move |_| {
      if has_background {
        container::Style::default()
      } else {
        container::Style::default().background(roles.panel)
      }
    })
    .into()
}

/// Position et largeur de la pastille de recherche en pixels de la fenêtre : le panneau d'historique
/// s'y accroche (`.search-history-panel` : `top: calc(100% + 8px)`, `left: 0`, `right: 0`).
pub fn search_geometry(app: &App) -> (f32, f32, f32) {
  let window = app.window.size.unwrap_or(DEFAULT_WINDOW).width;
  let sidebar = sidebar_width(app);
  let (engine, gap) = engine_space(app);
  let pad = shell::TOP_NAV_PADDING;
  let (start, available) = match app.data.search_pos.as_str() {
    "left" => (sidebar + pad, window - sidebar - 2.0 * pad - engine - gap),
    "right" => (sidebar + pad, window - sidebar - 2.0 * pad - engine - gap),
    _ => {
      let left = (engine + tokens::css(36.0) - sidebar).max(0.0);
      (sidebar + left, window - sidebar - 2.0 * left - sidebar)
    }
  };
  let width = available.clamp(shell::SEARCH_MIN_WIDTH, shell::SEARCH_MAX_WIDTH);
  let x = match app.data.search_pos.as_str() {
    "left" => start,
    "right" => start + available - width,
    _ => start + (available - width) / 2.0,
  };
  let y = titlebar_height(app)
    + (shell::TOP_NAV_HEIGHT + shell::SEARCH_HEIGHT) / 2.0
    + tokens::search::PANEL_GAP;
  (x, y, width)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::window::PersistedWindowState;
  use iced::Size;

  fn app() -> App {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    app.window.size = Some(Size::new(1920.0, 1080.0));
    app
  }

  #[test]
  fn the_search_pill_is_centered_on_the_window_like_riff() {
    let mut app = app();
    app.data.sidebar_width = 288;
    let (x, _, width) = search_geometry(&app);
    assert!((x + width / 2.0 - 960.0).abs() < 1e-3);
    // `layout.json`, `search-box` : 559,99 px CSS de large, centrée à 960 px.
    assert!((width - tokens::css(560.0)).abs() < 1e-3);
  }

  #[test]
  fn the_search_pill_follows_the_search_position_setting() {
    let mut app = app();
    app.data.sidebar_width = 288;
    app.data.search_pos = "left".to_string();
    let (x, _, _) = search_geometry(&app);
    assert!((x - (tokens::css(288.0) + shell::TOP_NAV_PADDING)).abs() < 1e-3);
    app.data.search_pos = "right".to_string();
    let (x, _, width) = search_geometry(&app);
    let (engine, gap) = engine_space(&app);
    let expected = 1920.0 - shell::TOP_NAV_PADDING - engine - gap;
    assert!((x + width - expected).abs() < 1e-3);
  }

  #[test]
  fn the_history_panel_sits_8_css_pixels_under_the_pill() {
    let app = app();
    let (_, y, _) = search_geometry(&app);
    // Barre de titre 40 + marge de 10 au-dessus de la pastille de 44 : bas à 94, puis 8.
    assert!((y - tokens::css(102.0)).abs() < 1e-3);
  }

  #[test]
  fn the_hidden_title_bar_leaves_only_the_edge() {
    let mut app = app();
    app.settings.hide_titlebar = true;
    assert!((titlebar_height(&app) - TITLEBAR_EDGE).abs() < 1e-3);
    app.ui.titlebar_hovered = true;
    assert!((titlebar_height(&app) - shell::TITLEBAR_HEIGHT).abs() < 1e-3);
  }
}
