//! Carte dévoilée du bas vers le haut (`clip-path: inset(top -30px 0 -30px round 24px 24px 0 0)`, `connectedExpand` et `connectedCollapse`,
//! `styles.css:7585-7586`). iced ne coupe qu'en rectangle : le fond est dessiné ici avec les coins du haut arrondis à la hauteur visible,
//! et le contenu est coupé au rectangle visible, coins carrés. Au repos (`visible` vaut `None`), le contenu se dessine tel quel.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay};
use iced::{Border, Color, Element, Event, Length, Rectangle, Size, Vector, border, mouse};

pub struct Reveal<'a, Message, Theme, Renderer> {
  content: Element<'a, Message, Theme, Renderer>,
  /// Part de la hauteur visible depuis le bas ; `None` : carte entière, fond du contenu.
  visible: Option<f32>,
  background: Color,
  radius: f32,
}

/// Enveloppe `content` ; `background` et `radius` fournissent le fond de la carte pendant le dévoilement.
pub fn reveal<'a, Message, Theme, Renderer>(
  content: impl Into<Element<'a, Message, Theme, Renderer>>,
  visible: Option<f32>,
  background: Color,
  radius: f32,
) -> Reveal<'a, Message, Theme, Renderer> {
  Reveal {
    content: content.into(),
    visible,
    background,
    radius,
  }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
  for Reveal<'_, Message, Theme, Renderer>
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
    tree: &mut Tree,
    event: &Event,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
    renderer: &Renderer,
    clipboard: &mut dyn Clipboard,
    shell: &mut Shell<'_, Message>,
    viewport: &Rectangle,
  ) {
    self.content.as_widget_mut().update(
      tree, event, layout, cursor, renderer, clipboard, shell, viewport,
    );
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
    let Some(visible) = self.visible else {
      self
        .content
        .as_widget()
        .draw(tree, renderer, theme, style, layout, cursor, viewport);
      return;
    };
    let bounds = layout.bounds();
    let hidden = bounds.height * (1.0 - visible.clamp(0.0, 1.0));
    let shown = Rectangle {
      y: bounds.y + hidden,
      height: bounds.height - hidden,
      ..bounds
    };
    // Une couche vide fausserait la fusion des couches du dessus (`Stack::merge`) : rien à dessiner sous un pixel.
    if shown.height < 1.0 {
      return;
    }
    let Some(clip) = shown.intersection(viewport) else {
      return;
    };
    renderer.fill_quad(
      Quad {
        bounds: shown,
        border: Border {
          radius: border::Radius {
            top_left: self.radius,
            top_right: self.radius,
            bottom_right: 0.0,
            bottom_left: 0.0,
          },
          ..Border::default()
        },
        ..Quad::default()
      },
      self.background,
    );
    renderer.with_layer(clip, |renderer| {
      // Les couches ouvertes par le contenu ne reprennent pas la coupe de celle-ci : on réduit le `viewport` qu'elles intersectent.
      self
        .content
        .as_widget()
        .draw(tree, renderer, theme, style, layout, cursor, &clip);
    });
  }

  fn mouse_interaction(
    &self,
    tree: &Tree,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
    viewport: &Rectangle,
    renderer: &Renderer,
  ) -> mouse::Interaction {
    self
      .content
      .as_widget()
      .mouse_interaction(tree, layout, cursor, viewport, renderer)
  }

  fn operate(
    &mut self,
    tree: &mut Tree,
    layout: Layout<'_>,
    renderer: &Renderer,
    operation: &mut dyn widget::Operation,
  ) {
    self
      .content
      .as_widget_mut()
      .operate(tree, layout, renderer, operation);
  }

  fn overlay<'a>(
    &'a mut self,
    tree: &'a mut Tree,
    layout: Layout<'a>,
    renderer: &Renderer,
    viewport: &Rectangle,
    translation: Vector,
  ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
    self
      .content
      .as_widget_mut()
      .overlay(tree, layout, renderer, viewport, translation)
  }
}

impl<'a, Message, Theme, Renderer> From<Reveal<'a, Message, Theme, Renderer>>
  for Element<'a, Message, Theme, Renderer>
where
  Message: 'a,
  Theme: 'a,
  Renderer: iced::advanced::Renderer + 'a,
{
  fn from(widget: Reveal<'a, Message, Theme, Renderer>) -> Self {
    Self::new(widget)
  }
}
