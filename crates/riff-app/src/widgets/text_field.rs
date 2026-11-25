//! Champ de saisie : focus visible par Tab, validation par Entrée, Échap retire le focus (`text_input`).

use crate::theme::tokens;
use iced::widget::text_input;
use iced::widget::text_input::{Status, Style};
use iced::{Background, Border, Element, Theme};

const RADIUS: f32 = tokens::radius::S;

fn style(theme: &Theme, status: Status) -> Style {
  let palette = theme.extended_palette();
  let border = match status {
    Status::Focused { .. } => Border {
      color: palette.primary.base.color,
      width: 2.0,
      radius: RADIUS.into(),
    },
    Status::Hovered => Border {
      color: palette.background.base.text,
      width: 1.0,
      radius: RADIUS.into(),
    },
    Status::Active | Status::Disabled => Border {
      color: palette.background.strongest.color,
      width: 1.0,
      radius: RADIUS.into(),
    },
  };
  Style {
    background: Background::Color(palette.background.weak.color),
    border,
    icon: palette.background.base.text,
    placeholder: palette.background.strongest.color,
    value: palette.background.base.text,
    selection: palette.primary.weak.color,
  }
}

/// Champ de saisie. `id` permet de lui donner le focus depuis une tâche.
pub fn text_field<'a, Message: Clone + 'a>(
  id: &'static str,
  placeholder: &str,
  value: &str,
  on_input: impl Fn(String) -> Message + 'a,
  on_submit: Option<Message>,
) -> Element<'a, Message> {
  text_input(placeholder, value)
    .id(id)
    .on_input(on_input)
    .on_submit_maybe(on_submit)
    .padding([9, 12])
    .size(14)
    .style(style)
    .into()
}

/// Champ masqué, pour un secret.
pub fn secret_field<'a, Message: Clone + 'a>(
  id: &'static str,
  placeholder: &str,
  value: &str,
  on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
  text_input(placeholder, value)
    .id(id)
    .secure(true)
    .on_input(on_input)
    .padding([9, 12])
    .size(14)
    .style(style)
    .into()
}
