//! Historique de recherche `#search-history-panel` : panneau sous la pastille de recherche
//! (`initSearchHistory`, `app.js:10098-10246`, `styles.css:11003-11081`). Les dimensions sont des
//! pixels CSS convertis par [`tokens::css`] ; voir [`tokens::search`].

use super::chrome::tip;
use super::home::{Roles, explicit_badge, page_scroll_style};
use super::window_frame::search_geometry;
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_ROW};
use crate::theme::tokens::search as metrics;
use crate::theme::{fonts, icons, tokens};
use crate::ui::UiMessage;
use crate::user_data::SearchEntry;
use crate::widgets::focus::focusable;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
  Column, button, column, container, image, mouse_area, opaque, pin, row, scrollable, space, stack,
  text,
};
use iced::{Background, Color, ContentFit, Element, Fill, Padding, Point, border};

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

fn line<'a>(label: String, size: f32, font: iced::Font, color: Color) -> Element<'a, Message> {
  text(label)
    .size(size)
    .font(font)
    .color(color)
    .line_height(LineHeight::Relative(metrics::TEXT_LINE))
    .wrapping(Wrapping::None)
    .into()
}

/// `.sh-thumb` : vignette carrée ou ronde (artiste), icône de repli tant que l'image manque.
fn thumb<'a>(app: &App, roles: Roles, entry: &SearchEntry) -> Element<'a, Message> {
  let artist = entry.kind.as_deref() == Some("artist");
  let radius = if artist {
    metrics::THUMB / 2.0
  } else {
    metrics::THUMB_RADIUS
  };
  let loaded = entry
    .thumb_url()
    .and_then(|url| app.images.get(&ImageKey::new(&url, SIZE_ROW)).cloned());
  let content: Element<'a, Message> = match loaded {
    Some(handle) => image(handle)
      .width(metrics::THUMB)
      .height(metrics::THUMB)
      .content_fit(ContentFit::Cover)
      .border_radius(radius)
      .into(),
    None => icons::glyph_px(
      if artist {
        icons::SH_ARTIST
      } else {
        icons::SEARCH
      },
      metrics::THUMB_ICON,
    ),
  };
  container(content)
    .width(metrics::THUMB)
    .height(metrics::THUMB)
    .align_x(Horizontal::Center)
    .align_y(Vertical::Center)
    .clip(true)
    .style(move |_| {
      container::Style::default()
        .background(roles.highest)
        .color(roles.on_surface)
        .border(border::rounded(radius))
    })
    .into()
}

/// `.verified-check` : disque `primary`, coche `on-primary`.
pub(super) fn verified_check<'a>(roles: Roles) -> Element<'a, Message> {
  let layer = |glyph: &'static str, color: Color| {
    container(icons::glyph_px(glyph, metrics::VERIFIED))
      .style(move |_| container::Style::default().color(color))
  };
  stack![
    layer(icons::VERIFIED_DISC, roles.primary),
    layer(icons::VERIFIED_TICK, roles.on_primary),
  ]
  .into()
}

/// `.sh-info` : titre (graisse 600) et ligne « Type • sous-titre ».
fn info<'a>(app: &App, roles: Roles, entry: &SearchEntry) -> Element<'a, Message> {
  let kind = match entry.kind.as_deref() {
    Some("track") => "Track",
    Some("artist") => "Artist",
    _ => "Search",
  };
  let sub = match entry.subtitle.as_deref().filter(|sub| !sub.is_empty()) {
    Some(sub) => format!("{} \u{2022} {sub}", app.i18n.t(kind)),
    None => app.i18n.t(kind),
  };
  let mut title = row![line(
    entry.title.clone().unwrap_or_default(),
    metrics::TITLE_TEXT,
    fonts::SEMIBOLD,
    roles.on_surface,
  )]
  .spacing(metrics::TITLE_GAP)
  .align_y(Vertical::Center);
  if entry.kind.as_deref() == Some("artist") && entry.is_verified() {
    title = title.push(verified_check(roles));
  }
  let mut sub_line = row![].spacing(metrics::SUB_GAP).align_y(Vertical::Center);
  if entry.kind.as_deref() == Some("track") && entry.is_explicit() {
    sub_line = sub_line.push(explicit_badge(roles));
  }
  sub_line = sub_line.push(line(sub, metrics::SUB_TEXT, fonts::UI, roles.outline));
  column![title.clip(true), sub_line.clip(true)]
    .width(Fill)
    .clip(true)
    .into()
}

/// `.sh-remove` : croix ronde, invisible hors survol de la ligne (`opacity: 0`).
fn remove_button<'a>(
  app: &App,
  roles: Roles,
  row_hovered: bool,
  index: usize,
) -> Element<'a, Message> {
  let message = ui(UiMessage::HistoryRemove(index));
  let entry = focusable(
    button(icons::glyph_px(icons::SH_REMOVE, metrics::REMOVE_ICON))
      .padding(metrics::REMOVE_PADDING)
      .on_press(message.clone())
      .style(move |_, status| button::Style {
        text_color: if !row_hovered {
          Color::TRANSPARENT
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
          roles.on_surface
        } else {
          roles.outline
        },
        border: border::rounded(9999),
        ..button::Style::default()
      }),
  )
  .on_activate(Some(message))
  .radius(tokens::radius::FULL);
  tip(entry.into(), app.i18n.t("Remove"))
}

/// `.sh-row` : vignette, texte et croix ; fond `surface-container-high` au survol.
fn entry_row<'a>(
  app: &App,
  roles: Roles,
  index: usize,
  entry: &SearchEntry,
) -> Element<'a, Message> {
  let hovered = app.ui.search.hover == Some(index);
  let open = ui(UiMessage::HistoryOpen(index));
  let body = focusable(
    button(
      row![thumb(app, roles, entry), info(app, roles, entry)]
        .spacing(metrics::ROW_GAP)
        .align_y(Vertical::Center),
    )
    .padding(0)
    .width(Fill)
    .style(|_, _| button::Style::default())
    .on_press(open.clone()),
  )
  .on_activate(Some(open))
  .radius(metrics::ROW_RADIUS);
  let line = row![body, remove_button(app, roles, hovered, index)]
    .spacing(metrics::ROW_GAP)
    .align_y(Vertical::Center);
  let padded = container(line)
    .padding(Padding::from([
      metrics::ROW_PADDING_Y,
      metrics::ROW_PADDING_X,
    ]))
    .width(Fill)
    .style(move |_| {
      let style = container::Style::default().border(border::rounded(metrics::ROW_RADIUS));
      if hovered {
        style.background(Background::Color(roles.high))
      } else {
        style
      }
    });
  mouse_area(padded)
    .on_enter(ui(UiMessage::HistoryHover(Some(index))))
    .on_exit(ui(UiMessage::HistoryHover(None)))
    .into()
}

/// `.sh-head` : « Search history » et bouton « Clear ».
fn head<'a>(app: &App, roles: Roles) -> Element<'a, Message> {
  // La couleur vient du style du bouton : `.sh-clear` passe de `outline` à `on-surface` au survol.
  let clear = button(
    text(app.i18n.t("Clear"))
      .size(metrics::CLEAR_TEXT)
      .font(fonts::SEMIBOLD)
      .line_height(LineHeight::Relative(metrics::TEXT_LINE)),
  )
  .padding(0)
  .on_press(ui(UiMessage::HistoryClear))
  .style(move |_, status| button::Style {
    text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
      roles.on_surface
    } else {
      roles.outline
    },
    ..button::Style::default()
  });
  container(
    row![
      line(
        app.i18n.t("Search history"),
        metrics::HEAD_TEXT,
        fonts::BOLD,
        roles.on_surface,
      ),
      space().width(Fill),
      clear,
    ]
    .align_y(Vertical::Center),
  )
  .padding(Padding {
    top: metrics::HEAD_TOP,
    right: metrics::HEAD_SIDE,
    bottom: metrics::HEAD_BOTTOM,
    left: metrics::HEAD_SIDE,
  })
  .width(Fill)
  .into()
}

/// Panneau d'historique, absent tant que le champ n'a pas le focus ou que la liste est vide.
pub fn view(app: &App) -> Option<Element<'_, Message>> {
  if !app.ui.search.history_open {
    return None;
  }
  let entries = app.data.search_history_matching(&app.ui.search.input);
  if entries.is_empty() {
    return None;
  }
  let roles = Roles::of(app);
  let rows = Column::with_children(
    entries
      .into_iter()
      .enumerate()
      .map(|(index, entry)| entry_row(app, roles, index, entry)),
  );
  let content = container(column![head(app, roles), rows]).padding(metrics::PANEL_PADDING);
  let list = scrollable(content)
    .direction(Direction::Vertical(
      Scrollbar::new()
        .width(tokens::home::SCROLLBAR)
        .scroller_width(tokens::home::SCROLLBAR)
        .margin(0),
    ))
    .style(move |_, status| page_scroll_style(roles, status));
  let (x, y, width) = search_geometry(app);
  let panel = container(list)
    .width(width)
    .max_height(metrics::PANEL_MAX_HEIGHT)
    .clip(true)
    .style(move |_| {
      container::Style::default()
        .background(roles.high)
        .color(roles.on_surface)
        .border(border::rounded(metrics::PANEL_RADIUS))
    });
  Some(pin(opaque(panel)).position(Point::new(x, y)).into())
}
