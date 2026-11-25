//! Panneau flottant « Now playing and queue » `#np-queue-panel` de Riff 44.1.1 (`app.js:26978-27144`,
//! `styles.css:10908-10976`, `11380-11445`) : deux onglets (File / Récemment joués), bouton de fermeture,
//! puis les titres. Ancré à droite, 368 px CSS. Voir [`crate::theme::tokens::queue`].
//!
//! Ouverture et fermeture : fondu et glissement de 16 px (`styles.css:10927-10940`) par [`crate::motion::PanelMotion`]. Le fondu est
//! approché par l'alpha de chaque couleur (pas d'opacité de groupe dans iced). Non repris : l'animation `artistPaneSlideIn` au
//! changement d'onglet et le flou d'arrière-plan (pas de flou dans iced).

use super::chrome::{tip, tips};
use super::home::{Roles, blend, page_scroll_style};
use super::right_panel::ellipsize;
use super::sidebar::{faded, rgb};
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_ROW};
use crate::motion::ViewPose;
use crate::theme::fonts;
use crate::theme::icons;
use crate::theme::tokens::queue as metrics;
use crate::theme::tokens::sidebar as tabs;
use crate::ui::{QueueTab, UiMessage};
use crate::user_data::Track;
use crate::widgets::focus::focusable;
use crate::widgets::view_transition::view_transition;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
  Column, Container, Row, button, column, container, image, opaque, row, scrollable, space, stack,
  text,
};
use iced::{Background, Color, ContentFit, Element, Fill, Font, Padding, border};

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

/// Texte d'un panneau sans titre à montrer (`render` de `initQueuePanel`).
#[must_use]
pub fn queue_panel_empty(app: &App, tab: QueueTab) -> String {
  app.i18n.t(match tab {
    QueueTab::Recent => "Nothing played yet",
    QueueTab::Queue if app.player.current.is_none() => "Nothing is playing",
    QueueTab::Queue => "Nothing queued next",
  })
}

/// Pochettes du panneau « Now playing and queue », seulement ouvert.
pub fn queue_panel_cover_urls(app: &App, out: &mut Vec<String>) {
  if !app.ui.queue_panel.open {
    return;
  }
  match app.ui.queue_panel.tab {
    QueueTab::Queue => {
      out.extend(
        app
          .player
          .current
          .iter()
          .filter_map(|track| app.cover_of(track)),
      );
      out.extend(
        app
          .queue_panel_upcoming()
          .into_iter()
          .filter_map(|(_, track)| app.cover_of(track)),
      );
    }
    QueueTab::Recent => out.extend(
      app
        .queue_panel_recent()
        .iter()
        .filter_map(|track| app.cover_of(track)),
    ),
  }
}

/// `.np-queue-heading` : le premier a 4 px de remplissage en haut au lieu de 12.
pub(super) fn heading<'a>(app: &App, c: Roles, label: &str, first: bool) -> Element<'a, Message> {
  container(
    text(app.i18n.t(label))
      .size(metrics::HEADING_TEXT)
      .font(fonts::BOLD)
      .line_height(LineHeight::Absolute(metrics::HEADING_LINE.into()))
      .color(c.variant),
  )
  .padding(Padding {
    top: if first {
      metrics::HEADING_PADDING_TOP_FIRST
    } else {
      metrics::HEADING_PADDING_TOP
    },
    right: metrics::HEADING_PADDING_X,
    bottom: metrics::HEADING_PADDING_BOTTOM,
    left: metrics::HEADING_PADDING_X,
  })
  .width(Fill)
  .into()
}

/// `.np-queue-empty`.
fn empty_hint<'a>(app: &App, c: Roles, tab: QueueTab) -> Element<'a, Message> {
  container(
    text(queue_panel_empty(app, tab))
      .size(metrics::EMPTY_TEXT)
      .color(c.outline),
  )
  .padding([metrics::EMPTY_PADDING_Y, metrics::EMPTY_PADDING_X])
  .center_x(Fill)
  .into()
}

/// `.np-queue-art` : pochette de 34 px, fond `surface-container-highest` tant que l'image manque. L'opacité de l'image suit celle
/// du panneau (`c.on_surface.a`, voir [`fade_roles`]).
fn art<'a>(app: &App, c: Roles, track: &Track) -> Element<'a, Message> {
  let handle = app
    .cover_of(track)
    .and_then(|url| app.images.get(&ImageKey::new(&url, SIZE_ROW)).cloned());
  match handle {
    Some(handle) => image(handle)
      .width(metrics::ART)
      .height(metrics::ART)
      .content_fit(ContentFit::Cover)
      .opacity(c.on_surface.a)
      .border_radius(metrics::ART_RADIUS)
      .into(),
    None => container(space())
      .width(metrics::ART)
      .height(metrics::ART)
      .style(move |_| {
        container::Style::default()
          .background(c.highest)
          .border(border::rounded(metrics::ART_RADIUS))
      })
      .into(),
  }
}

/// « Added by {nom} » de l'entrée `index` pendant un jam.
pub(super) fn added_by(app: &App, index: Option<usize>) -> Option<String> {
  let entry = app.jam.as_ref()?.entries.get(index?)?;
  Some(
    app
      .i18n
      .t("Added by {name}")
      .replace("{name}", &entry.by_name),
  )
}

/// Contenu d'une rangée : pochette, titre et artiste (`makeRow`). En jam, la deuxième ligne ajoute qui a ajouté le titre.
fn row_content<'a>(
  app: &App,
  c: Roles,
  track: &Track,
  playing: bool,
  added_by: Option<&str>,
) -> Row<'a, Message> {
  let title_font: Font = if playing {
    fonts::SEMIBOLD
  } else {
    fonts::MEDIUM
  };
  let title = ellipsize(
    track.title_text(),
    metrics::TITLE_TEXT,
    title_font,
    metrics::INFO_WIDTH,
  );
  let artist = track.artist.as_deref().unwrap_or_default();
  let artist = match added_by {
    Some(by) => format!("{artist} · {by}"),
    None => artist.to_string(),
  };
  let artist = ellipsize(
    &artist,
    metrics::ARTIST_TEXT,
    fonts::UI,
    metrics::INFO_WIDTH,
  );
  let info = column![
    text(title)
      .size(metrics::TITLE_TEXT)
      .font(title_font)
      .line_height(LineHeight::Relative(metrics::TEXT_LINE))
      .wrapping(Wrapping::None)
      .color(if playing { c.primary } else { c.on_surface }),
    text(artist)
      .size(metrics::ARTIST_TEXT)
      .line_height(LineHeight::Relative(metrics::TEXT_LINE))
      .wrapping(Wrapping::None)
      .color(c.variant),
  ]
  .spacing(metrics::INFO_GAP)
  .width(Fill);
  row![art(app, c, track), info]
    .spacing(metrics::ROW_GAP)
    .align_y(Vertical::Center)
}

fn row_padding() -> Padding {
  Padding::from([metrics::ROW_PADDING_Y, metrics::ROW_PADDING_X])
}

/// Rangée cliquable (`.np-queue-row`, fond `on-surface` à 7 % au survol).
fn track_row<'a>(
  app: &App,
  c: Roles,
  track: &Track,
  added_by: Option<&str>,
  message: Message,
) -> Element<'a, Message> {
  let entry = button(row_content(app, c, track, false, added_by))
    .padding(row_padding())
    .width(Fill)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: hovered
          .then(|| Background::Color(faded(c.on_surface, metrics::ROW_HOVER_SHARE))),
        text_color: c.on_surface,
        border: border::rounded(metrics::ROW_RADIUS),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  focusable(entry)
    .on_activate(Some(message))
    .radius(metrics::ROW_RADIUS)
    .into()
}

/// Rangée du titre en cours (`.np-queue-row.playing`) : sans clic, fond `primary` à 14 %.
fn playing_row<'a>(
  app: &App,
  c: Roles,
  track: &Track,
  added_by: Option<&str>,
) -> Element<'a, Message> {
  container(row_content(app, c, track, true, added_by))
    .padding(row_padding())
    .width(Fill)
    .style(move |_| {
      container::Style::default()
        .background(faded(c.primary, metrics::ROW_PLAYING_SHARE))
        .border(border::rounded(metrics::ROW_RADIUS))
    })
    .into()
}

/// `.np-queue-tab` : largeur du texte plus 10 px de chaque côté, fond `on-surface` à 10 % et trait de 32 x 3 px si actif.
fn tab<'a>(c: Roles, label: String, active: bool, message: Message) -> Element<'a, Message> {
  let label = text(label)
    .size(tabs::TAB_TEXT)
    .font(if active {
      fonts::SEMIBOLD
    } else {
      fonts::MEDIUM
    })
    .wrapping(Wrapping::None);
  let underline: Element<'_, Message> = if active {
    container(space())
      .width(tabs::TAB_UNDERLINE_WIDTH)
      .height(tabs::TAB_UNDERLINE_HEIGHT)
      .style(move |_| {
        container::Style::default()
          .background(c.on_surface)
          .border(border::Border::default().rounded(border::top(tabs::TAB_UNDERLINE_HEIGHT)))
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
  let entry = button(body)
    .padding([0.0, tabs::TAB_PADDING_X])
    .height(tabs::TAB_HEIGHT)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: if active {
          Some(Background::Color(faded(
            c.on_surface,
            tabs::TAB_ACTIVE_SHARE,
          )))
        } else if hovered {
          Some(Background::Color(faded(
            c.on_surface,
            tabs::TAB_HOVER_SHARE,
          )))
        } else {
          None
        },
        text_color: if active || hovered {
          c.on_surface
        } else {
          c.variant
        },
        border: border::Border::default().rounded(border::top(tabs::TAB_RADIUS)),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  focusable(entry)
    .on_activate(Some(message))
    .radius(tabs::TAB_RADIUS)
    .into()
}

/// `.np-queue-close` : 32 px, fond transparent, `on-surface` à 10 % au survol.
fn close_button<'a>(app: &App, c: Roles) -> Element<'a, Message> {
  let message = ui(UiMessage::ToggleQueuePanel);
  let entry = button(container(icons::glyph_px(icons::CLOSE, metrics::CLOSE_ICON)).center(Fill))
    .padding(0)
    .width(metrics::CLOSE_SIZE)
    .height(metrics::CLOSE_SIZE)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      button::Style {
        background: hovered
          .then(|| Background::Color(faded(c.on_surface, metrics::CLOSE_HOVER_SHARE))),
        text_color: c.on_surface,
        border: border::rounded(9999),
        ..button::Style::default()
      }
    })
    .on_press(message.clone());
  tip(
    focusable(entry)
      .on_activate(Some(message))
      .radius(metrics::CLOSE_SIZE / 2.0)
      .into(),
    app.i18n.t(tips::CLOSE),
  )
}

/// Filet de 1 px en `outline-variant` à 55 %.
pub(super) fn rule<'a>(color: Color) -> Element<'a, Message> {
  container(space())
    .width(Fill)
    .height(1)
    .style(move |_| container::Style::default().background(color))
    .into()
}

/// Lignes du corps selon l'onglet (`render`).
fn body_rows<'a>(app: &App, c: Roles) -> Vec<Element<'a, Message>> {
  let mut rows: Vec<Element<'a, Message>> = Vec::new();
  match (app.ui.queue_panel.tab, app.player.current.as_ref()) {
    (QueueTab::Recent, _) => {
      let recent = app.queue_panel_recent();
      if recent.is_empty() {
        rows.push(empty_hint(app, c, QueueTab::Recent));
      }
      for (index, track) in recent.iter().enumerate() {
        let message = ui(UiMessage::QueuePanelPlayRecent(index));
        rows.push(track_row(app, c, track, None, message));
      }
    }
    (QueueTab::Queue, None) => rows.push(empty_hint(app, c, QueueTab::Queue)),
    (QueueTab::Queue, Some(current)) => {
      rows.push(heading(app, c, "Now playing", true));
      rows.push(playing_row(
        app,
        c,
        current,
        added_by(app, app.player.index).as_deref(),
      ));
      let upcoming = app.queue_panel_upcoming();
      if upcoming.is_empty() {
        rows.push(empty_hint(app, c, QueueTab::Queue));
      } else {
        rows.push(heading(app, c, "Next up", false));
      }
      for (index, track) in upcoming {
        let message = ui(UiMessage::QueuePanelPlay(index));
        rows.push(track_row(
          app,
          c,
          track,
          added_by(app, Some(index)).as_deref(),
          message,
        ));
      }
    }
  }
  rows
}

/// Rôles à l'opacité `alpha` : iced 0.14 n'a pas d'opacité de groupe, chaque couleur porte donc l'alpha du fondu du panneau.
pub(super) fn fade_roles(c: Roles, alpha: f32) -> Roles {
  Roles {
    on_surface: faded(c.on_surface, alpha),
    variant: faded(c.variant, alpha),
    outline: faded(c.outline, alpha),
    container: faded(c.container, alpha),
    high: faded(c.high, alpha),
    highest: faded(c.highest, alpha),
    primary: faded(c.primary, alpha),
    on_primary: faded(c.on_primary, alpha),
    primary_container: faded(c.primary_container, alpha),
    on_primary_container: faded(c.on_primary_container, alpha),
    page: faded(c.page, alpha),
  }
}

/// Panneau flottant « Now playing and queue » (`.np-queue-panel`) : 368 px CSS, ancré à droite.
pub fn queue_panel(app: &App) -> Option<Element<'_, Message>> {
  let panel = &app.ui.queue_panel;
  if !panel.motion.visible(panel.open, app.now) {
    return None;
  }
  let pose = panel.motion.pose(app.now);
  let alpha = pose.map_or(1.0, |pose| pose.opacity);
  let c = Roles::of(app);
  let line = faded(
    blend(rgb(app.theme.scheme.outline_variant), c.high, 0.55),
    alpha,
  );
  let c = fade_roles(c, alpha);
  let nav = Row::new()
    .push(tab(
      c,
      app.i18n.t("Queue"),
      panel.tab == QueueTab::Queue,
      ui(UiMessage::QueuePanelTab(QueueTab::Queue)),
    ))
    .push(tab(
      c,
      app.i18n.t("Recently played"),
      panel.tab == QueueTab::Recent,
      ui(UiMessage::QueuePanelTab(QueueTab::Recent)),
    ))
    .spacing(tabs::TABS_GAP)
    .padding(Padding {
      top: tabs::TABS_PADDING_TOP,
      right: tabs::TABS_PADDING_X,
      bottom: 0.0,
      left: tabs::TABS_PADDING_X,
    });
  let close = container(close_button(app, c)).padding(Padding {
    bottom: metrics::CLOSE_MARGIN_BOTTOM,
    ..Padding::ZERO
  });
  let head = container(
    row![nav, space().width(Fill), close]
      .spacing(metrics::HEAD_GAP)
      .align_y(Vertical::Center),
  )
  .padding(Padding {
    top: metrics::HEAD_PADDING_TOP,
    right: metrics::HEAD_PADDING_X,
    bottom: 0.0,
    left: metrics::HEAD_PADDING_X,
  });
  let mut rules = Column::new();
  for _ in 0..metrics::RULE_COUNT {
    rules = rules.push(rule(line));
  }
  let body = scrollable(
    Column::from_vec(body_rows(app, c))
      .spacing(metrics::BODY_GAP)
      .width(Fill)
      .padding(Padding {
        top: metrics::BODY_PADDING_TOP,
        right: metrics::BODY_PADDING_X,
        bottom: metrics::BODY_PADDING_BOTTOM,
        left: metrics::BODY_PADDING_X,
      }),
  )
  .direction(Direction::Vertical(
    Scrollbar::new().width(5).scroller_width(5).margin(0),
  ))
  .style(move |_, status| page_scroll_style(c, status))
  .width(Fill)
  .height(Fill);
  let card = container(column![head, rules, body])
    .width(metrics::PANEL_WIDTH)
    .height(Fill)
    .clip(true)
    .style(move |_| {
      container::Style::default()
        .background(c.high)
        .border(border::rounded(metrics::PANEL_RADIUS))
    });
  Some(layer(card, pose, panel.open))
}

/// Carte ancrée à droite : `transform: translateX(16px) scale(0.98)` autour du centre du panneau (`transform-origin` non posé).
pub(super) fn layer<'a>(
  card: Container<'a, Message>,
  pose: Option<crate::motion::PanelPose>,
  open: bool,
) -> Element<'a, Message> {
  let card = view_transition(
    opaque(card),
    pose.map(|pose| ViewPose {
      opacity: pose.opacity,
      offset_y: 0.0,
      scale: pose.scale,
    }),
    None,
  )
  .centered()
  .shift_x(pose.map_or(0.0, |pose| pose.offset_x))
  .interactive(open);
  container(card)
    .width(Fill)
    .height(Fill)
    .align_right(Fill)
    .padding(Padding {
      top: metrics::PANEL_TOP,
      right: metrics::PANEL_RIGHT,
      bottom: metrics::PANEL_BOTTOM,
      left: 0.0,
    })
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_queue_panel_is_a_layer_only_while_open() {
    let mut app = App::new(
      crate::window::PersistedWindowState::default(),
      crate::settings::NativeSettings::default(),
      crate::i18n::I18n::default(),
    );
    assert!(queue_panel(&app).is_none());
    app.update(Message::Ui(UiMessage::ToggleQueuePanel));
    assert!(queue_panel(&app).is_some());
    let mut urls = Vec::new();
    queue_panel_cover_urls(&app, &mut urls);
    assert!(urls.is_empty());
  }

  #[test]
  fn added_by_names_the_participant_only_during_a_jam() {
    let mut app = App::new(
      crate::window::PersistedWindowState::default(),
      crate::settings::NativeSettings::default(),
      crate::i18n::I18n::default(),
    );
    assert_eq!(added_by(&app, Some(0)), None);
    let mut state = crate::jam::state::JamState::new(crate::jam::Role::Host, "Ana");
    state.entries.push(crate::jam::state::EntryMeta {
      uid: 1,
      by: 0,
      by_name: "Ana".to_string(),
    });
    app.jam = Some(Box::new(state));
    assert_eq!(added_by(&app, Some(0)).as_deref(), Some("Added by Ana"));
    assert_eq!(added_by(&app, Some(1)), None);
    assert_eq!(added_by(&app, None), None);
  }

  /// `.np-queue-panel` : 368 px CSS, soit 347,8 px écran mesurés sur 11b (1557 à 1905).
  #[test]
  fn the_panel_is_368_css_pixels_wide() {
    assert!((metrics::PANEL_WIDTH - 348.0).abs() < 1.0);
  }
}
