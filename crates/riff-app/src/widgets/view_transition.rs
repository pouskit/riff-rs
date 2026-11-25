//! Vue qui s'ouvre (`m3ContainerOpen`, `styles.css:1009-1011`) : translation et échelle autour du haut centre (`transform-origin:
//! center top`, `styles.css:995`), contenu coupé à la zone de l'écran. iced n'a pas d'opacité de groupe : le fondu est un voile de la
//! couleur du fond, dessiné par-dessus avec l'opacité restante, exact sur un fond uni (voir `App::view`).
//! Au repos (`pose` vaut `None`), le contenu se dessine tel quel, sans couche ni transformation.
//! `pop` : variante des fenêtres modales (carte des réglages, `transform: scale() translateY()` autour du centre) : origine au
//! centre et voile arrondi dessiné dans le repère transformé, sur la carte seule.

use crate::motion::ViewPose;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay};
use iced::{Color, Element, Event, Length, Rectangle, Size, Transformation, Vector, mouse};

pub struct ViewTransition<'a, Message, Theme, Renderer> {
  content: Element<'a, Message, Theme, Renderer>,
  pose: Option<ViewPose>,
  /// Couleur du fond sous la vue, pour le fondu ; `None` si le fond n'est pas uni (image, halo).
  veil: Option<Color>,
  /// Origine au centre et voile arrondi de ce rayon dans le repère transformé ; `None` : voile plein.
  pop_radius: Option<f32>,
  /// Origine au centre de la zone au lieu du haut centre (`transform-origin` par défaut).
  centered: bool,
  /// `translateX` appliqué avant l'échelle, en pixels écran.
  shift_x: f32,
  /// Faux : le contenu se dessine mais ne reçoit aucun événement (`pointer-events: none`).
  interactive: bool,
  /// Hauteur ajoutée sous la zone de coupe : le contenu translaté vers le bas déborde de `bounds` (aucun `overflow` en CSS).
  spill: f32,
  /// `clip-path: inset(0 -side bottom -side)` : seule la part `visible` de `hauteur + below` est dessinée depuis le haut.
  reveal: Option<Reveal>,
  /// Marge transparente de chaque côté de `bounds` (raccords qui débordent du panneau) : ni la découpe ni le voile ne la comptent.
  margin: f32,
}

/// Découpe qui se déroule du haut vers le bas, dans le repère du contenu (elle suit sa transformation).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reveal {
  visible: f32,
  side: f32,
  below: f32,
}

impl Reveal {
  /// Hauteur visible depuis le haut : `visible * (hauteur + below)`.
  fn shown_height(&self, height: f32) -> f32 {
    (height + self.below) * self.visible.clamp(0.0, 1.0)
  }
}

/// Enveloppe `content` ; sans `pose`, l'enveloppe est transparente pour le rendu et les événements.
pub fn view_transition<'a, Message, Theme, Renderer>(
  content: impl Into<Element<'a, Message, Theme, Renderer>>,
  pose: Option<ViewPose>,
  veil: Option<Color>,
) -> ViewTransition<'a, Message, Theme, Renderer> {
  ViewTransition {
    content: content.into(),
    pose,
    veil,
    pop_radius: None,
    centered: false,
    shift_x: 0.0,
    interactive: true,
    spill: 0.0,
    reveal: None,
    margin: 0.0,
  }
}

impl<Message, Theme, Renderer> ViewTransition<'_, Message, Theme, Renderer> {
  /// Fenêtre modale : transformation autour du centre, voile aux coins arrondis de `radius`.
  #[must_use]
  pub fn pop(mut self, radius: f32) -> Self {
    self.pop_radius = Some(radius);
    self
  }

  /// Origine au centre de la zone (`transform-origin: 50% 50%`).
  #[must_use]
  pub fn centered(mut self) -> Self {
    self.centered = true;
    self
  }

  /// `translateX` en pixels écran, appliqué avant l'échelle.
  #[must_use]
  pub fn shift_x(mut self, shift_x: f32) -> Self {
    self.shift_x = shift_x;
    self
  }

  /// Coupe agrandie de `spill` pixels écran vers le bas.
  #[must_use]
  pub fn spill(mut self, spill: f32) -> Self {
    self.spill = spill;
    self
  }

  /// Découpe `inset(0 -side bottom -side)` d'une transition de `clip-path` : la hauteur visible, depuis le haut du contenu, vaut
  /// `visible * (hauteur + below)`, et la découpe suit la transformation. Le voile ne couvre que cette zone.
  #[must_use]
  pub fn reveal(mut self, visible: f32, side: f32, below: f32) -> Self {
    self.reveal = Some(Reveal {
      visible,
      side,
      below,
    });
    self
  }

  /// Le contenu déborde de `margin` pixels de chaque côté de la boîte qui se découpe et se voile : `inset(.. -side ..)` se mesure
  /// depuis cette boîte, pas depuis la marge.
  #[must_use]
  pub fn margin(mut self, margin: f32) -> Self {
    self.margin = margin;
    self
  }

  /// Sans interaction : le contenu ne reçoit ni événement ni curseur.
  #[must_use]
  pub fn interactive(mut self, interactive: bool) -> Self {
    self.interactive = interactive;
    self
  }

  /// Transformation du contenu : origine en haut au centre de `bounds`, ou au centre avec `centered` ou `pop`.
  fn transformation(&self, pose: ViewPose, bounds: Rectangle) -> Transformation {
    let origin = (
      bounds.x + bounds.width / 2.0,
      if self.centered || self.pop_radius.is_some() {
        bounds.y + bounds.height / 2.0
      } else {
        bounds.y
      },
    );
    Transformation::translate(origin.0 + self.shift_x, origin.1 + pose.offset_y)
      * Transformation::scale(pose.scale)
      * Transformation::translate(-origin.0, -origin.1)
  }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
  for ViewTransition<'_, Message, Theme, Renderer>
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
    if !self.interactive {
      return;
    }
    let Some(pose) = self.pose else {
      self.content.as_widget_mut().update(
        tree, event, layout, cursor, renderer, clipboard, shell, viewport,
      );
      return;
    };
    let inverse = self.transformation(pose, layout.bounds()).inverse();
    self.content.as_widget_mut().update(
      tree,
      event,
      layout,
      cursor * inverse,
      renderer,
      clipboard,
      shell,
      &(*viewport * inverse),
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
    let Some(pose) = self.pose else {
      self
        .content
        .as_widget()
        .draw(tree, renderer, theme, style, layout, cursor, viewport);
      return;
    };
    let bounds = layout.bounds();
    let transformation = self.transformation(pose, bounds);
    let inverse = transformation.inverse();
    // Le glissement horizontal sort de `bounds` : la coupe s'élargit d'autant de chaque côté.
    let local_clip = self.reveal.map(|reveal| Rectangle {
      x: bounds.x + self.margin - reveal.side,
      width: bounds.width - 2.0 * self.margin + 2.0 * reveal.side,
      height: reveal.shown_height(bounds.height),
      ..bounds
    });
    // Les couches des enfants (défilement, bordures) ne s'intersectent pas avec celle-ci : la découpe est aussi leur zone visible.
    let content_viewport = match local_clip {
      Some(local) => local.intersection(&(*viewport * inverse)),
      None => Some(*viewport * inverse),
    };
    let Some(content_viewport) = content_viewport else {
      return;
    };
    let clip = match local_clip {
      Some(local) => local * transformation,
      None => Rectangle {
        x: bounds.x - self.shift_x.abs(),
        width: bounds.width + 2.0 * self.shift_x.abs(),
        height: bounds.height + self.spill,
        ..bounds
      },
    };
    let Some(clip) = clip.intersection(viewport) else {
      return;
    };
    renderer.with_layer(clip, |renderer| {
      renderer.with_transformation(transformation, |renderer| {
        self.content.as_widget().draw(
          tree,
          renderer,
          theme,
          style,
          layout,
          cursor * inverse,
          &content_viewport,
        );
        if let (Some(radius), Some(veil)) = (self.pop_radius, self.veil)
          && pose.opacity < 1.0
        {
          // Nouvelle couche : dans une même couche, les quads passent sous le texte.
          let Some(veil_bounds) = bounds.intersection(&(*viewport * inverse)) else {
            return;
          };
          renderer.with_layer(veil_bounds, |renderer| {
            renderer.fill_quad(
              Quad {
                bounds,
                border: iced::border::rounded(radius),
                ..Quad::default()
              },
              Color {
                a: 1.0 - pose.opacity,
                ..veil
              },
            );
          });
        }
      });
    });
    if self.pop_radius.is_none()
      && let Some(veil) = self.veil
      && pose.opacity < 1.0
    {
      // Nouvelle couche : dans une même couche, les quads passent sous le texte.
      // Sous une découpe, le voile couvre la boîte découverte et rien des marges ni de l'excédent de la découpe.
      let veil_area = match self.reveal {
        Some(reveal) => {
          Rectangle {
            x: bounds.x + self.margin,
            width: bounds.width - 2.0 * self.margin,
            height: bounds.height.min(reveal.shown_height(bounds.height)),
            ..bounds
          } * transformation
        }
        None => bounds,
      };
      let Some(veil_bounds) = veil_area.intersection(viewport) else {
        return;
      };
      renderer.with_layer(veil_bounds, |renderer| {
        renderer.fill_quad(
          Quad {
            bounds: veil_area,
            ..Quad::default()
          },
          Color {
            a: 1.0 - pose.opacity,
            ..veil
          },
        );
      });
    }
  }

  fn mouse_interaction(
    &self,
    tree: &Tree,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
    viewport: &Rectangle,
    renderer: &Renderer,
  ) -> mouse::Interaction {
    if !self.interactive {
      return mouse::Interaction::default();
    }
    let Some(pose) = self.pose else {
      return self
        .content
        .as_widget()
        .mouse_interaction(tree, layout, cursor, viewport, renderer);
    };
    let inverse = self.transformation(pose, layout.bounds()).inverse();
    self.content.as_widget().mouse_interaction(
      tree,
      layout,
      cursor * inverse,
      &(*viewport * inverse),
      renderer,
    )
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

impl<'a, Message, Theme, Renderer> From<ViewTransition<'a, Message, Theme, Renderer>>
  for Element<'a, Message, Theme, Renderer>
where
  Message: 'a,
  Theme: 'a,
  Renderer: iced::advanced::Renderer + 'a,
{
  fn from(widget: ViewTransition<'a, Message, Theme, Renderer>) -> Self {
    Self::new(widget)
  }
}

/// Calque dessiné en repère écran dans un contenu que `ViewTransition` déplace : `child` (de la taille de la fenêtre) reprend sa
/// place, ni décalé ni mis à l'échelle, et n'apparaît que dans la boîte du contenu (marges `margin` exclues, découpe `reveal`
/// comprise). C'est le voile d'une opacité de groupe sur un fond qui n'est pas uni : le fond du dessous, rejoué à `1 - opacité`.
/// Le calque occupe les bornes de la `ViewTransition` (même origine de transformation, haut au centre) et se place dans une
/// `stack!` au-dessus du contenu.
pub struct ScreenLayer<'a, Message, Theme, Renderer> {
  child: Element<'a, Message, Theme, Renderer>,
  pose: ViewPose,
  window: Size,
  margin: f32,
  /// Part de `hauteur + below` visible depuis le haut (`ViewTransition::reveal`).
  reveal: Option<(f32, f32)>,
}

impl<'a, Message, Theme, Renderer> ScreenLayer<'a, Message, Theme, Renderer> {
  /// `pose` : celle de la `ViewTransition` parente ; `window` : taille de la fenêtre, donnée au calque pour sa mise en page.
  pub fn new(
    child: impl Into<Element<'a, Message, Theme, Renderer>>,
    pose: ViewPose,
    window: Size,
  ) -> Self {
    Self {
      child: child.into(),
      pose,
      window,
      margin: 0.0,
      reveal: None,
    }
  }

  /// Marge transparente de chaque côté (voir `ViewTransition::margin`).
  #[must_use]
  pub fn margin(mut self, margin: f32) -> Self {
    self.margin = margin;
    self
  }

  /// Même découpe que `ViewTransition::reveal`.
  #[must_use]
  pub fn reveal(mut self, visible: f32, below: f32) -> Self {
    self.reveal = Some((visible, below));
    self
  }
}

/// Transformation qui ramène un point du repère de la mise en page (fenêtre entière, origine en haut à gauche de l'écran) à sa place
/// à l'écran à travers la pose de la `ViewTransition` : `inverse` de celle-ci, précédée du déplacement qui amène l'origine du calque
/// (`bounds`) à celle de l'écran.
fn screen_transformation(bounds: Rectangle, pose: ViewPose) -> Transformation {
  let origin = (bounds.x + bounds.width / 2.0, bounds.y);
  let pose = Transformation::translate(origin.0, origin.1 + pose.offset_y)
    * Transformation::scale(pose.scale)
    * Transformation::translate(-origin.0, -origin.1);
  pose.inverse() * Transformation::translate(-bounds.x, -bounds.y)
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
  for ScreenLayer<'_, Message, Theme, Renderer>
where
  Renderer: iced::advanced::Renderer,
{
  fn children(&self) -> Vec<Tree> {
    vec![Tree::new(&self.child)]
  }

  fn diff(&self, tree: &mut Tree) {
    tree.diff_children(std::slice::from_ref(&self.child));
  }

  fn size(&self) -> Size<Length> {
    Size::new(Length::Fill, Length::Fill)
  }

  fn layout(
    &mut self,
    tree: &mut Tree,
    renderer: &Renderer,
    limits: &layout::Limits,
  ) -> layout::Node {
    let child = self.child.as_widget_mut().layout(
      &mut tree.children[0],
      renderer,
      &layout::Limits::new(Size::ZERO, self.window),
    );
    layout::Node::with_children(
      limits.resolve(Length::Fill, Length::Fill, Size::ZERO),
      vec![child],
    )
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
    let Some(child_layout) = layout.children().next() else {
      return;
    };
    let bounds = layout.bounds();
    let shown = self.reveal.map_or(bounds.height, |(visible, below)| {
      (bounds.height + below) * visible.clamp(0.0, 1.0)
    });
    let area = Rectangle {
      x: bounds.x + self.margin,
      width: bounds.width - 2.0 * self.margin,
      height: bounds.height.min(shown),
      ..bounds
    };
    let Some(clip) = area.intersection(viewport) else {
      return;
    };
    // Les couches ouvertes par le calque (les `stack!` du fond) ne s'intersectent pas avec celle-ci : leur zone visible est la découpe, ramenée
    // dans le repère de la mise en page du calque.
    let transformation = screen_transformation(bounds, self.pose);
    let visible = clip * transformation.inverse();
    renderer.with_layer(clip, |renderer| {
      renderer.with_transformation(transformation, |renderer| {
        self.child.as_widget().draw(
          &tree.children[0],
          renderer,
          theme,
          style,
          child_layout,
          mouse::Cursor::Unavailable,
          &visible,
        );
      });
    });
  }
}

impl<'a, Message, Theme, Renderer> From<ScreenLayer<'a, Message, Theme, Renderer>>
  for Element<'a, Message, Theme, Renderer>
where
  Message: 'a,
  Theme: 'a,
  Renderer: iced::advanced::Renderer + 'a,
{
  fn from(layer: ScreenLayer<'a, Message, Theme, Renderer>) -> Self {
    Self::new(layer)
  }
}

#[cfg(test)]
mod tests {
  use super::{Reveal, screen_transformation};
  use crate::motion::ViewPose;
  use iced::{Point, Rectangle, Size, Transformation};

  #[test]
  fn the_revealed_height_counts_the_overflow_below_and_clamps_the_progress() {
    let reveal = |visible| Reveal {
      visible,
      side: 0.0,
      below: 20.0,
    };
    assert_eq!(reveal(0.5).shown_height(100.0), 60.0);
    assert_eq!(reveal(2.0).shown_height(100.0), 120.0);
    assert_eq!(reveal(-1.0).shown_height(100.0), 0.0);
  }

  #[test]
  fn the_screen_layer_lands_unmoved_whatever_the_pose() {
    let bounds = Rectangle::new(Point::new(780.0, 0.0), Size::new(410.0, 300.0));
    let pose = ViewPose {
      opacity: 0.5,
      offset_y: -12.0,
      scale: 0.97,
    };
    // Transformation de la `ViewTransition` : origine en haut au centre de `bounds`.
    let origin = (bounds.x + bounds.width / 2.0, bounds.y);
    let parent = Transformation::translate(origin.0, origin.1 + pose.offset_y)
      * Transformation::scale(pose.scale)
      * Transformation::translate(-origin.0, -origin.1);
    // Un point de la mise en page du calque, à `(u, v)` de son origine, arrive à `(u, v)` à l'écran.
    let point = Point::new(bounds.x + 333.0, bounds.y + 71.0);
    let landed = point * (parent * screen_transformation(bounds, pose));
    assert!((landed.x - 333.0).abs() < 1e-3, "{landed:?}");
    assert!((landed.y - 71.0).abs() < 1e-3, "{landed:?}");
  }
}
