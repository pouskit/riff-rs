//! Rangée à défilement horizontal qui laisse la molette verticale à la page.
//!
//! `bindHorizontalWheel` (`app.js:9783`) ne détourne que la molette horizontale ou Maj + molette :
//! la molette verticale fait défiler la page même au-dessus d'une rangée. Le `scrollable` d'iced
//! convertit toute molette en défilement horizontal ; ce conteneur ne lui transmet que les mêmes cas.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::keyboard;
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector};

#[derive(Debug, Clone, Copy, Default)]
struct State {
  shift: bool,
}

/// Contenu dont seule la molette horizontale est transmise. Voir [`horizontal_wheel`].
pub struct HorizontalWheel<'a, Message, Renderer = iced::Renderer> {
  content: Element<'a, Message, Theme, Renderer>,
}

/// Enveloppe `content` (un `scrollable` horizontal).
pub fn horizontal_wheel<'a, Message, Renderer>(
  content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> HorizontalWheel<'a, Message, Renderer> {
  HorizontalWheel {
    content: content.into(),
  }
}

/// Vrai si la molette doit atteindre la rangée : plus horizontale que verticale, ou Maj tenue.
fn reaches_row(x: f32, y: f32, shift: bool) -> bool {
  shift || x.abs() > y.abs()
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for HorizontalWheel<'_, Message, Renderer>
where
  Renderer: iced::advanced::Renderer,
{
  fn tag(&self) -> tree::Tag {
    tree::Tag::of::<State>()
  }

  fn state(&self) -> tree::State {
    tree::State::new(State::default())
  }

  fn children(&self) -> Vec<Tree> {
    vec![Tree::new(&self.content)]
  }

  fn diff(&self, tree: &mut Tree) {
    if tree.tag == tree::Tag::of::<State>() {
      tree.diff_children(std::slice::from_ref(&self.content));
    } else {
      tree.tag = tree::Tag::of::<State>();
      tree.state = self.state();
      tree.children = self.children();
    }
  }

  fn size(&self) -> Size<Length> {
    self.content.as_widget().size()
  }

  fn size_hint(&self) -> Size<Length> {
    self.content.as_widget().size_hint()
  }

  fn layout(
    &mut self,
    tree: &mut Tree,
    renderer: &Renderer,
    limits: &layout::Limits,
  ) -> layout::Node {
    self
      .content
      .as_widget_mut()
      .layout(&mut tree.children[0], renderer, limits)
  }

  fn operate(
    &mut self,
    tree: &mut Tree,
    layout: Layout<'_>,
    renderer: &Renderer,
    operation: &mut dyn Operation,
  ) {
    self
      .content
      .as_widget_mut()
      .operate(&mut tree.children[0], layout, renderer, operation);
  }

  fn update(
    &mut self,
    tree: &mut Tree,
    event: &Event,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
    renderer: &Renderer,
    clipboard: &mut dyn Clipboard,
    shell: &mut Shell<'_, Message>,
    viewport: &Rectangle,
  ) {
    let state = tree.state.downcast_mut::<State>();
    match event {
      Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
        state.shift = modifiers.shift();
      }
      Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
        let (mouse::ScrollDelta::Lines { x, y } | mouse::ScrollDelta::Pixels { x, y }) = *delta;
        if !reaches_row(x, y, state.shift) {
          return;
        }
      }
      _ => {}
    }
    self.content.as_widget_mut().update(
      &mut tree.children[0],
      event,
      layout,
      cursor,
      renderer,
      clipboard,
      shell,
      viewport,
    );
  }

  fn mouse_interaction(
    &self,
    tree: &Tree,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
    viewport: &Rectangle,
    renderer: &Renderer,
  ) -> mouse::Interaction {
    self.content.as_widget().mouse_interaction(
      &tree.children[0],
      layout,
      cursor,
      viewport,
      renderer,
    )
  }

  fn draw(
    &self,
    tree: &Tree,
    renderer: &mut Renderer,
    theme: &Theme,
    style: &renderer::Style,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
    viewport: &Rectangle,
  ) {
    self.content.as_widget().draw(
      &tree.children[0],
      renderer,
      theme,
      style,
      layout,
      cursor,
      viewport,
    );
  }

  fn overlay<'b>(
    &'b mut self,
    tree: &'b mut Tree,
    layout: Layout<'b>,
    renderer: &Renderer,
    viewport: &Rectangle,
    translation: Vector,
  ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
    self.content.as_widget_mut().overlay(
      &mut tree.children[0],
      layout,
      renderer,
      viewport,
      translation,
    )
  }
}

impl<'a, Message, Renderer> From<HorizontalWheel<'a, Message, Renderer>>
  for Element<'a, Message, Theme, Renderer>
where
  Message: 'a,
  Renderer: iced::advanced::Renderer + 'a,
{
  fn from(wheel: HorizontalWheel<'a, Message, Renderer>) -> Self {
    Element::new(wheel)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_the_horizontal_wheel_or_shift_reaches_the_row() {
    assert!(!reaches_row(0.0, 3.0, false));
    assert!(!reaches_row(1.0, -3.0, false));
    assert!(reaches_row(3.0, 1.0, false));
    assert!(reaches_row(0.0, 3.0, true));
  }
}
