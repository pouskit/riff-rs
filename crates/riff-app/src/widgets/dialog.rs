//! Boîte de dialogue modale.

use super::OVERLAY_ID;
use super::button::{filled, text_button};
use iced::widget::{column, container, mouse_area, opaque, row, space, text};
use iced::{Color, Element, Fill, Theme, border};

const SCRIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.5);
const WIDTH: f32 = 420.0;

/// Contenu d'un dialogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
  pub title: String,
  pub body: String,
  pub cancel: String,
  /// Libellé de la confirmation. Sans libellé : un seul bouton.
  pub confirm: Option<String>,
}

fn card_style(theme: &Theme) -> container::Style {
  let palette = theme.extended_palette();
  container::Style::default()
    .background(palette.background.neutral.color)
    .color(palette.background.base.text)
    .border(border::rounded(16))
}

/// Couche du dialogue, à poser au-dessus du contenu. Un clic sur le voile le ferme.
pub fn view<'a, Message: super::touch::Touchable + 'a>(
  dialog: &'a Dialog,
  on_close: Message,
  on_confirm: Message,
) -> Element<'a, Message> {
  let mut actions = row![
    space().width(Fill),
    text_button(dialog.cancel.as_str(), Some(on_close.clone()))
  ]
  .spacing(8);
  if let Some(confirm) = &dialog.confirm {
    actions = actions.push(filled(confirm.as_str(), Some(on_confirm)));
  }
  let card = container(
    column![
      text(dialog.title.as_str()).size(20),
      text(dialog.body.as_str()).size(14),
      actions
    ]
    .spacing(16),
  )
  .id(OVERLAY_ID)
  .padding(24)
  .width(WIDTH)
  .style(card_style);
  let scrim = container(opaque(card))
    .center(Fill)
    .style(|_| container::Style::default().background(SCRIM));
  opaque(mouse_area(scrim).on_press(on_close))
}
