//! Menu contextuel en superposition : `.riff-ctx-menu` de Riff (`styles.css:10016-10111`).

use super::OVERLAY_ID;
use super::focus::focusable;
use super::inert::inert;
use crate::screens::right_panel;
use crate::theme::tokens::context_menu as metrics;
use crate::theme::{fonts, icons};
use iced::alignment::Vertical;
use iced::widget::button::{self, Status};
use iced::widget::text::Wrapping;
use iced::widget::{
  Column, Row, button as button_widget, container, hover, mouse_area, opaque, pin, space, stack,
  text as text_widget,
};
use iced::{Background, Color, Element, Fill, Point, Size, border};
use material_colors::color::Rgb;
use material_colors::scheme::Scheme;

/// Entrée de menu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem<Message> {
  pub label: String,
  pub message: Message,
  /// Icône de `theme::icons` ; sans icône, seul le libellé est affiché.
  pub icon: Option<&'static str>,
  /// `.riff-ctx-item.danger` : survol rouge.
  pub danger: bool,
  /// `.riff-ctx-item.is-active` : icône à la couleur d'accent au repos.
  pub active: bool,
  /// `.riff-ctx-divider` avant cette entrée.
  pub divider_before: bool,
}

impl<Message> MenuItem<Message> {
  #[must_use]
  pub fn new(label: impl Into<String>, message: Message) -> Self {
    Self {
      label: label.into(),
      message,
      icon: None,
      danger: false,
      active: false,
      divider_before: false,
    }
  }

  #[must_use]
  pub fn icon(mut self, icon: &'static str) -> Self {
    self.icon = Some(icon);
    self
  }

  #[must_use]
  pub fn danger(mut self) -> Self {
    self.danger = true;
    self
  }

  #[must_use]
  pub fn active(mut self, active: bool) -> Self {
    self.active = active;
    self
  }

  #[must_use]
  pub fn divider_before(mut self) -> Self {
    self.divider_before = true;
    self
  }
}

/// Menu ouvert.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu<Message> {
  pub position: Point,
  pub items: Vec<MenuItem<Message>>,
}

/// Couleurs du menu, lues dans le schéma du thème (`--md-sys-color-*`).
#[derive(Debug, Clone, Copy)]
pub struct Colors {
  /// `surface-container-high` : fond (`styles.css:11020`, opaque quand `--ui-alpha` vaut 100 %).
  pub surface: Color,
  pub on_surface: Color,
  /// `on-surface-variant` : icônes au repos.
  pub variant: Color,
  pub primary: Color,
  pub outline_variant: Color,
}

impl Colors {
  /// Couleurs à `opacity` : `opacity` de `.riff-ctx-menu` en transition. iced n'a pas d'opacité de groupe, l'alpha passe sur chaque
  /// couleur (fond, texte, icônes, séparateur, survol) : exact hors des zones où un élément en recouvre un autre.
  #[must_use]
  fn faded(self, opacity: f32) -> Self {
    Self {
      surface: self.surface.scale_alpha(opacity),
      on_surface: self.on_surface.scale_alpha(opacity),
      variant: self.variant.scale_alpha(opacity),
      primary: self.primary.scale_alpha(opacity),
      outline_variant: self.outline_variant.scale_alpha(opacity),
    }
  }

  #[must_use]
  pub fn of(scheme: &Scheme) -> Self {
    let rgb = |color: Rgb| Color::from_rgb8(color.red, color.green, color.blue);
    Self {
      surface: rgb(scheme.surface_container_high),
      on_surface: rgb(scheme.on_surface),
      variant: rgb(scheme.on_surface_variant),
      primary: rgb(scheme.primary),
      outline_variant: rgb(scheme.outline_variant),
    }
  }
}

/// Mêmes paramètres de `Text` que `right_panel::text_width` (vérifié champ par champ) : le cache borné est partagé.
fn text_width(content: &str) -> f32 {
  right_panel::text_width(content, metrics::ITEM_TEXT, fonts::MEDIUM)
}

/// Taille du menu : `min-width: 216px`, sinon la largeur de l'entrée la plus large (`.riff-ctx-item` en flex).
#[must_use]
pub fn size<Message>(items: &[MenuItem<Message>]) -> Size {
  let widest = items
    .iter()
    .map(|item| {
      let icon = if item.icon.is_some() {
        metrics::ICON_SIZE + metrics::ITEM_GAP
      } else {
        0.0
      };
      2.0 * metrics::ITEM_PADDING_X + icon + text_width(&item.label).ceil()
    })
    .fold(0.0, f32::max);
  let width = (widest + 2.0 * metrics::PADDING).max(metrics::MIN_WIDTH);
  let dividers = items.iter().filter(|item| item.divider_before).count();
  let children = items.len() + dividers;
  let dividers_height =
    dividers as f32 * (metrics::DIVIDER_HEIGHT + 2.0 * metrics::DIVIDER_MARGIN_Y);
  let height = 2.0 * metrics::PADDING
    + items.len() as f32 * metrics::ITEM_HEIGHT
    + dividers_height
    + children.saturating_sub(1) as f32 * metrics::GAP;
  Size::new(width, height)
}

/// Position du menu (`openTrackContextMenu`, `app.js:10891-10894`) : à gauche ou au-dessus du pointeur
/// quand il manque 12 px, jamais à moins de 8 px du bord haut ou gauche.
#[must_use]
pub fn place(position: Point, menu: Size, window: Size) -> Point {
  let flip = |at: f32, length: f32, limit: f32| {
    let placed = if at + length + metrics::EDGE_FLIP > limit {
      at - length
    } else {
      at
    };
    placed.max(metrics::EDGE_MIN)
  };
  Point::new(
    flip(position.x, menu.width, window.width),
    flip(position.y, menu.height, window.height),
  )
}

fn card_style(colors: Colors) -> container::Style {
  container::Style::default()
    .background(colors.surface)
    .color(colors.on_surface)
    .border(border::rounded(metrics::RADIUS))
}

/// Contenu d'une entrée : icône et libellé, avec les couleurs du repos ou du survol.
fn content<'a, Message: 'a>(
  item: &'a MenuItem<Message>,
  icon: Color,
  label: Color,
) -> Element<'a, Message> {
  let mut row = Row::new()
    .spacing(metrics::ITEM_GAP)
    .align_y(Vertical::Center);
  if let Some(glyph) = item.icon {
    row = row.push(
      container(icons::glyph_px(glyph, 20.0))
        .style(move |_| container::Style::default().color(icon)),
    );
  }
  row
    .push(
      text_widget(item.label.as_str())
        .size(metrics::ITEM_TEXT)
        .font(fonts::MEDIUM)
        .wrapping(Wrapping::None)
        .color(label),
    )
    .into()
}

fn item_view<'a, Message: Clone + 'a>(
  item: &'a MenuItem<Message>,
  message: Message,
  colors: Colors,
  opacity: f32,
) -> Element<'a, Message> {
  let rest_icon = if item.active {
    colors.primary
  } else {
    colors.variant
  };
  let (hover_icon, hover_label, hover_fill) = if item.danger {
    (
      metrics::DANGER_TEXT.scale_alpha(opacity),
      metrics::DANGER_TEXT.scale_alpha(opacity),
      metrics::DANGER_BACKGROUND.scale_alpha(opacity),
    )
  } else {
    (
      colors.primary,
      colors.on_surface,
      colors.on_surface.scale_alpha(metrics::HOVER_SHARE),
    )
  };
  let rest = button_widget(
    container(content(item, rest_icon, colors.on_surface))
      .height(Fill)
      .align_y(Vertical::Center),
  )
  .on_press(message.clone())
  .padding([0.0, metrics::ITEM_PADDING_X])
  .height(metrics::ITEM_HEIGHT)
  .width(Fill)
  .style(|_, _: Status| button::Style::default());
  // Survol : l'entrée est redessinée par-dessus avec son fond, son icône et son texte de survol.
  let hovered = container(content(item, hover_icon, hover_label))
    .padding([0.0, metrics::ITEM_PADDING_X])
    .height(metrics::ITEM_HEIGHT)
    .width(Fill)
    .align_y(Vertical::Center)
    .style(move |_| {
      container::Style::default()
        .background(Background::Color(hover_fill))
        .border(border::rounded(metrics::ITEM_RADIUS))
    });
  focusable(hover(rest, hovered))
    .on_activate(Some(message))
    .radius(metrics::ITEM_RADIUS)
    .into()
}

fn divider<'a, Message: 'a>(colors: Colors) -> Element<'a, Message> {
  let color = colors.outline_variant.scale_alpha(metrics::DIVIDER_SHARE);
  container(
    container(space())
      .width(Fill)
      .height(metrics::DIVIDER_HEIGHT)
      .style(move |_| container::Style::default().background(color)),
  )
  .padding([metrics::DIVIDER_MARGIN_Y, metrics::DIVIDER_MARGIN_X])
  .into()
}

/// Carte du menu et sa position, à `opacity` (1 : exactement le rendu au repos).
fn card_at<'a, Message: Clone + 'a>(
  menu: &'a Menu<Message>,
  window: Size,
  colors: Colors,
  to_message: impl Fn(Message) -> Message,
  opacity: f32,
) -> (Element<'a, Message>, Point) {
  let colors = colors.faded(opacity);
  let menu_size = size(&menu.items);
  let position = place(menu.position, menu_size, window);
  let mut column = Column::new()
    .spacing(metrics::GAP)
    .width(menu_size.width - 2.0 * metrics::PADDING);
  for item in &menu.items {
    if item.divider_before {
      column = column.push(divider(colors));
    }
    column = column.push(item_view(
      item,
      to_message(item.message.clone()),
      colors,
      opacity,
    ));
  }
  let card = container(column)
    .id(OVERLAY_ID)
    .padding(metrics::PADDING)
    .style(move |_| card_style(colors));
  (card.into(), position)
}

/// Menu qui s'efface après sa fermeture : dessiné à `opacity`, sans interaction (`pointer-events: none`, `styles.css:10036`).
pub fn fading<'a, Message: Clone + 'a>(
  menu: &'a Menu<Message>,
  window: Size,
  colors: Colors,
  to_message: impl Fn(Message) -> Message,
  opacity: f32,
) -> Element<'a, Message> {
  let (card, position) = card_at(menu, window, colors, to_message, opacity);
  inert(pin(card).position(position)).into()
}

/// Couche du menu, à poser au-dessus du contenu. Un clic hors du menu le ferme. `opacity` : transition d'ouverture (1 au repos).
pub fn view<'a, Message: Clone + 'a>(
  menu: &'a Menu<Message>,
  window: Size,
  colors: Colors,
  to_message: impl Fn(Message) -> Message,
  on_close: Message,
  opacity: f32,
) -> Element<'a, Message> {
  let (card, position) = card_at(menu, window, colors, to_message, opacity);
  let backdrop = mouse_area(space().width(Fill).height(Fill))
    .on_press(on_close.clone())
    .on_right_press(on_close);
  opaque(
    stack![backdrop, pin(opaque(card)).position(position)]
      .width(Fill)
      .height(Fill),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  const WINDOW: Size = Size::new(1000.0, 700.0);
  const MENU: Size = Size::new(200.0, 120.0);

  #[test]
  fn a_menu_with_room_stays_at_the_cursor() {
    assert_eq!(
      place(Point::new(100.0, 200.0), MENU, WINDOW),
      Point::new(100.0, 200.0)
    );
  }

  #[test]
  fn a_menu_near_the_right_edge_opens_to_the_left_of_the_cursor() {
    assert_eq!(place(Point::new(990.0, 200.0), MENU, WINDOW).x, 790.0);
  }

  #[test]
  fn a_menu_without_12_px_of_margin_flips() {
    let edge = WINDOW.width - MENU.width - metrics::EDGE_FLIP;
    assert_eq!(place(Point::new(edge, 0.0), MENU, WINDOW).x, edge);
    assert_eq!(
      place(Point::new(edge + 1.0, 0.0), MENU, WINDOW).x,
      edge + 1.0 - MENU.width
    );
  }

  #[test]
  fn a_menu_near_the_bottom_edge_opens_above_the_cursor() {
    assert_eq!(place(Point::new(100.0, 690.0), MENU, WINDOW).y, 570.0);
  }

  #[test]
  fn a_menu_never_goes_above_the_8_px_margin() {
    assert_eq!(
      place(Point::new(100.0, 90.0), MENU, Size::new(1000.0, 100.0)).y,
      metrics::EDGE_MIN
    );
    assert_eq!(
      place(Point::new(0.0, 0.0), MENU, WINDOW),
      Point::new(metrics::EDGE_MIN, metrics::EDGE_MIN)
    );
  }

  #[test]
  fn height_counts_gaps_and_dividers() {
    let items = vec![
      MenuItem::new("a", ()),
      MenuItem::new("b", ()).divider_before(),
    ];
    let expected = 2.0 * metrics::PADDING
      + 2.0 * metrics::ITEM_HEIGHT
      + 2.0 * metrics::GAP
      + metrics::DIVIDER_HEIGHT
      + 2.0 * metrics::DIVIDER_MARGIN_Y;
    assert!((size(&items).height - expected).abs() < 1e-3);
  }

  #[test]
  fn width_is_at_least_the_minimum() {
    assert!((size(&[MenuItem::new("a", ())]).width - metrics::MIN_WIDTH).abs() < 1e-3);
  }
}
