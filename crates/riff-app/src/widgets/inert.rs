//! Calque sans interaction : `pointer-events: none` de Riff sur un menu ou une modale qui se ferme (`styles.css:10035`, `7378`).
//! Le contenu se dessine tel quel (curseur absent, donc sans survol) ; événements, curseur et opérations de focus ne le traversent pas.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{Element, Event, Length, Rectangle, Size, mouse};

pub struct Inert<'a, Message, Theme, Renderer> {
  content: Element<'a, Message, Theme, Renderer>,
}

/// Enveloppe `content` : il reste visible, rien ne l'atteint.
pub fn inert<'a, Message, Theme, Renderer>(
  content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Inert<'a, Message, Theme, Renderer> {
  Inert {
    content: content.into(),
  }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
  for Inert<'_, Message, Theme, Renderer>
where
  Renderer: iced::advanced::Renderer,
{
  fn tag(&self) -> tree::Tag {
    self.content.as_widget().tag()
  }

  fn state(&self) -> tree::State {
    self.content.as_widget().state()
  }

  fn children(&self) -> Vec<Tree> {
    self.content.as_widget().children()
  }

  fn diff(&self, tree: &mut Tree) {
    self.content.as_widget().diff(tree);
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
    self.content.as_widget_mut().layout(tree, renderer, limits)
  }

  fn update(
    &mut self,
    _tree: &mut Tree,
    _event: &Event,
    _layout: Layout<'_>,
    _cursor: mouse::Cursor,
    _renderer: &Renderer,
    _clipboard: &mut dyn Clipboard,
    _shell: &mut Shell<'_, Message>,
    _viewport: &Rectangle,
  ) {
  }

  fn draw(
    &self,
    tree: &Tree,
    renderer: &mut Renderer,
    theme: &Theme,
    style: &renderer::Style,
    layout: Layout<'_>,
    _cursor: mouse::Cursor,
    viewport: &Rectangle,
  ) {
    self.content.as_widget().draw(
      tree,
      renderer,
      theme,
      style,
      layout,
      mouse::Cursor::Unavailable,
      viewport,
    );
  }

  fn mouse_interaction(
    &self,
    _tree: &Tree,
    _layout: Layout<'_>,
    _cursor: mouse::Cursor,
    _viewport: &Rectangle,
    _renderer: &Renderer,
  ) -> mouse::Interaction {
    mouse::Interaction::None
  }
}

impl<'a, Message, Theme, Renderer> From<Inert<'a, Message, Theme, Renderer>>
  for Element<'a, Message, Theme, Renderer>
where
  Message: 'a,
  Theme: 'a,
  Renderer: iced::advanced::Renderer + 'a,
{
  fn from(widget: Inert<'a, Message, Theme, Renderer>) -> Self {
    Self::new(widget)
  }
}
