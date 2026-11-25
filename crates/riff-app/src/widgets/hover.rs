//! Survol d'un contrôle recalculé à chaque événement, comme le fait Chromium après une mise en page.
//!
//! Riff : un élément retiré du DOM perd son état, un élément en `display: none` perd `:hover` et n'a pas de transition à
//! l'affichage, un élément qui apparaît sous un pointeur immobile devient survolé à l'image suivante (Chromium recalcule le survol
//! après une mise en page et envoie alors les événements d'entrée et de sortie : `codereview.chromium.org/2956023004`, commit
//! `c0b6b772`). `iced::widget::MouseArea` ne publie `on_exit` que si son widget existe encore : un contrôle qui disparaît sous le
//! pointeur garderait sa cible à 1 dans `ControlHover`.
//!
//! [`HoverTrack`] est l'état de survol de chaque widget qui porte une clé de `ControlHover` (`Focus`, [`HoverArea`]) : il compare
//! le survol à chaque événement (redessin compris, `MouseArea` d'iced 0.14 fait de même : `iced_widget-0.14.2/src/mouse_area.rs:342`),
//! publie l'entrée et la sortie, la sortie de l'ancienne clé quand le widget change d'instance à sa place, et signale sa disparition
//! (`Drop`) à l'application, qui oublie la clé au prochain message (`motion::take_gone`).

use crate::motion::{self, ControlKey};
use crate::ui::UiMessage;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector};

/// Clé de survol d'un widget et façon d'en faire un message de l'application.
pub struct Hover<'a, Message> {
  pub(crate) key: ControlKey,
  pub(crate) send: Box<dyn Fn(UiMessage) -> Message + 'a>,
}

/// État de survol d'un widget : instance qu'il représente et pointeur dessus ou non.
#[derive(Debug, Default)]
pub struct HoverTrack {
  key: Option<ControlKey>,
  over: bool,
}

impl HoverTrack {
  /// Recalcule le survol de l'instance `key` : `over` est l'état du pointeur sur les bornes courantes. Renvoie les messages à publier :
  /// l'oubli de l'instance qui occupait la place avant (la liste a changé sous le pointeur), puis l'entrée ou la sortie.
  /// Un widget neuf part hors survol : s'il apparaît sous un pointeur immobile, la première image publie l'entrée.
  #[must_use]
  pub fn sync(&mut self, key: ControlKey, over: bool) -> Vec<UiMessage> {
    let mut messages = Vec::new();
    if self.key != Some(key) {
      if let (true, Some(old)) = (self.over, self.key) {
        messages.push(UiMessage::ControlGone(old));
      }
      self.key = Some(key);
      self.over = false;
    }
    if over != self.over {
      self.over = over;
      messages.push(UiMessage::ControlHover(key, over));
    }
    messages
  }
}

impl Drop for HoverTrack {
  /// Le widget quitte l'arbre : aucune sortie du pointeur n'arrivera pour l'élément retiré.
  fn drop(&mut self) {
    if let (true, Some(key)) = (self.over, self.key) {
      motion::mark_gone(key);
    }
  }
}

/// Zone de survol d'un contenu : `mouse_area(content).on_enter().on_exit()` qui suit [`HoverTrack`].
pub struct HoverArea<'a, Message, Renderer = iced::Renderer> {
  content: Element<'a, Message, Theme, Renderer>,
  hover: Hover<'a, Message>,
}

/// Enveloppe `content` pour publier le survol de `key`.
pub fn area<'a, Message, Renderer>(
  key: ControlKey,
  send: impl Fn(UiMessage) -> Message + 'a,
  content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> HoverArea<'a, Message, Renderer> {
  HoverArea {
    content: content.into(),
    hover: Hover {
      key,
      send: Box::new(send),
    },
  }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for HoverArea<'_, Message, Renderer>
where
  Renderer: iced::advanced::Renderer,
{
  fn tag(&self) -> tree::Tag {
    tree::Tag::of::<HoverTrack>()
  }

  fn state(&self) -> tree::State {
    tree::State::new(HoverTrack::default())
  }

  fn children(&self) -> Vec<Tree> {
    vec![Tree::new(&self.content)]
  }

  fn diff(&self, tree: &mut Tree) {
    // `keyed_column` appelle `diff` sans comparer les types quand les clés changent : l'arbre peut être celui d'un autre widget.
    if tree.tag == tree::Tag::of::<HoverTrack>() {
      tree.diff_children(std::slice::from_ref(&self.content));
    } else {
      tree.tag = tree::Tag::of::<HoverTrack>();
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
    let over = cursor.is_over(layout.bounds());
    for message in tree
      .state
      .downcast_mut::<HoverTrack>()
      .sync(self.hover.key, over)
    {
      shell.publish((self.hover.send)(message));
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

impl<'a, Message, Renderer> From<HoverArea<'a, Message, Renderer>>
  for Element<'a, Message, Theme, Renderer>
where
  Message: 'a,
  Renderer: iced::advanced::Renderer + 'a,
{
  fn from(area: HoverArea<'a, Message, Renderer>) -> Self {
    Element::new(area)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use iced::widget::text;
  use iced::{Point, window};
  use iced_test::Simulator;
  use std::time::Instant;

  fn key(name: &'static str) -> ControlKey {
    ControlKey::from(name)
  }

  /// Messages sous une forme comparable : `Enter`, `Exit` ou `Gone`, et la clé.
  fn shape(messages: Vec<UiMessage>) -> Vec<(&'static str, ControlKey)> {
    messages
      .into_iter()
      .map(|message| match message {
        UiMessage::ControlHover(key, true) => ("enter", key),
        UiMessage::ControlHover(key, false) => ("exit", key),
        UiMessage::ControlGone(key) => ("gone", key),
        other => panic!("message inattendu : {other:?}"),
      })
      .collect()
  }

  #[test]
  fn a_fresh_widget_under_a_still_pointer_enters_on_its_first_event() {
    let mut track = HoverTrack::default();
    assert_eq!(shape(track.sync(key("a"), true)), [("enter", key("a"))]);
    assert!(track.sync(key("a"), true).is_empty());
  }

  #[test]
  fn leaving_publishes_the_exit_once() {
    let mut track = HoverTrack::default();
    let _ = track.sync(key("a"), true);
    assert_eq!(shape(track.sync(key("a"), false)), [("exit", key("a"))]);
    assert!(track.sync(key("a"), false).is_empty());
  }

  #[test]
  fn a_new_instance_in_the_same_place_forgets_the_old_one_then_enters() {
    let mut track = HoverTrack::default();
    let _ = track.sync(key("a"), true);
    assert_eq!(
      shape(track.sync(key("b"), true)),
      [("gone", key("a")), ("enter", key("b"))]
    );
  }

  #[test]
  fn a_new_instance_in_the_same_place_does_not_forget_an_instance_that_was_not_hovered() {
    let mut track = HoverTrack::default();
    let _ = track.sync(key("a"), false);
    assert!(track.sync(key("b"), false).is_empty());
  }

  #[test]
  fn dropping_a_hovered_widget_queues_its_key_and_a_quiet_one_queues_nothing() {
    let _ = motion::take_gone();
    drop(HoverTrack::default());
    let mut quiet = HoverTrack::default();
    let _ = quiet.sync(key("quiet"), false);
    drop(quiet);
    assert!(motion::take_gone().is_empty());
    let mut hovered = HoverTrack::default();
    let _ = hovered.sync(key("hovered"), true);
    drop(hovered);
    assert_eq!(motion::take_gone(), vec![key("hovered")]);
    assert!(motion::take_gone().is_empty());
  }

  /// Zone de survol d'un texte dans un simulateur d'iced : le texte occupe le coin haut gauche.
  fn simulator<'a>(name: &'static str) -> Simulator<'a, UiMessage> {
    let content: Element<'a, UiMessage> = text("Row").into();
    Simulator::new(area::<UiMessage, iced::Renderer>(
      key(name),
      |message| message,
      content,
    ))
  }

  fn redraw() -> Event {
    Event::Window(window::Event::RedrawRequested(Instant::now()))
  }

  #[test]
  fn an_area_under_a_still_pointer_enters_on_the_first_redraw_without_any_movement() {
    let mut sim = simulator("row");
    sim.point_at(Point::new(4.0, 4.0));
    let _ = sim.simulate([redraw()]);
    assert_eq!(
      shape(sim.into_messages().collect()),
      [("enter", key("row"))]
    );
  }

  #[test]
  fn an_area_away_from_the_pointer_publishes_nothing() {
    let mut sim = simulator("row");
    sim.point_at(Point::new(600.0, 500.0));
    let _ = sim.simulate([redraw()]);
    assert!(sim.into_messages().next().is_none());
  }

  #[test]
  fn a_hovered_area_removed_from_the_tree_queues_its_key() {
    let _ = motion::take_gone();
    let mut sim = simulator("removed");
    sim.point_at(Point::new(4.0, 4.0));
    let _ = sim.simulate([redraw()]);
    assert!(motion::take_gone().is_empty(), "encore dans l'arbre");
    drop(sim);
    assert_eq!(motion::take_gone(), vec![key("removed")]);
  }
}
