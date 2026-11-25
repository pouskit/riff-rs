//! Rend un widget focalisable au clavier : anneau de focus, activation par Entrée et Espace, menu contextuel.
//!
//! `iced::widget::button` n'a pas de focus clavier en 0.14.0 : ce conteneur l'ajoute à n'importe quel contenu.

use super::hover::HoverTrack;
use super::touch::{Touch, Touchable, draw_ripples};
use crate::motion::ControlKey;
use iced::advanced::graphics::geometry;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::operation::{self, Focusable};
use iced::advanced::widget::{self, Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::keyboard::{self, key};
use iced::{Border, Color, Element, Event, Length, Point, Rectangle, Size, Theme, Vector};
use std::sync::{Arc, Mutex};

const RING_WIDTH: f32 = 2.0;
const RING_OFFSET: f32 = 4.0;
// La bordure est dessinée à l'intérieur du rectangle : sans écart, l'anneau se confond avec un bouton plein.
const _: () = assert!(RING_OFFSET > RING_WIDTH);

#[derive(Debug, Default)]
struct State {
  hover: HoverTrack,
  pressed: bool,
  focused: bool,
}

impl Focusable for State {
  fn is_focused(&self) -> bool {
    self.focused
  }

  fn focus(&mut self) {
    self.focused = true;
  }

  fn unfocus(&mut self) {
    self.focused = false;
  }
}

/// Donne le focus au widget focalisable de rang `target` et le retire aux autres.
struct FocusAt {
  target: Option<usize>,
  current: usize,
}

impl<T> Operation<T> for FocusAt {
  fn focusable(&mut self, _id: Option<&widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
    if self.target == Some(self.current) {
      state.focus();
    } else {
      state.unfocus();
    }
    self.current += 1;
  }

  fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
    operate(self);
  }
}

/// Rang suivant ou précédent, sans passage par « aucun focus » en fin de cycle.
const fn cycle_target(focused: Option<usize>, total: usize, back: bool) -> Option<usize> {
  if total == 0 {
    return None;
  }
  Some(match (focused, back) {
    (None, false) => 0,
    (None, true) => total - 1,
    (Some(index), false) => (index + 1) % total,
    (Some(index), true) => (index + total - 1) % total,
  })
}

/// Tab : `focus_next` d'iced passe par un état sans focus en fin de cycle, pas celle-ci.
pub fn cycle_next<T: Send + 'static>() -> impl Operation<T> {
  operation::then(operation::focusable::count(), |count| FocusAt {
    target: cycle_target(count.focused, count.total, false),
    current: 0,
  })
}

/// Maj+Tab.
pub fn cycle_previous<T: Send + 'static>() -> impl Operation<T> {
  operation::then(operation::focusable::count(), |count| FocusAt {
    target: cycle_target(count.focused, count.total, true),
    current: 0,
  })
}

/// Donne le focus au widget de rang `ordinal`.
#[must_use]
pub const fn focus_nth(ordinal: usize) -> impl Operation<()> {
  FocusAt {
    target: Some(ordinal),
    current: 0,
  }
}

struct FocusedOrdinal {
  current: usize,
  slot: Arc<Mutex<Option<usize>>>,
}

impl Operation<()> for FocusedOrdinal {
  fn focusable(&mut self, _id: Option<&widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
    if state.is_focused()
      && let Ok(mut slot) = self.slot.lock()
    {
      *slot = Some(self.current);
    }
    self.current += 1;
  }

  fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
    operate(self);
  }
}

/// Note le rang du widget qui a le focus, lu dans le second élément une fois l'opération faite.
#[must_use]
pub fn focused_ordinal() -> (impl Operation<()>, Arc<Mutex<Option<usize>>>) {
  let slot = Arc::new(Mutex::new(None));
  (
    FocusedOrdinal {
      current: 0,
      slot: Arc::clone(&slot),
    },
    slot,
  )
}

/// Contenu focalisable. Voir [`focusable`].
pub struct Focus<'a, Message, Renderer = iced::Renderer> {
  content: Element<'a, Message, Theme, Renderer>,
  on_activate: Option<Message>,
  on_context: Option<Box<dyn Fn(Point) -> Message + 'a>>,
  on_left_press: Option<Box<dyn Fn(Point) -> Message + 'a>>,
  on_any_press: Option<Message>,
  in_tab_order: bool,
  touch: Option<Touch<'a, Message>>,
  radius: f32,
}

/// Enveloppe `content` pour le rendre atteignable par Tab.
pub fn focusable<'a, Message, Renderer>(
  content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Focus<'a, Message, Renderer> {
  Focus {
    content: content.into(),
    on_activate: None,
    on_context: None,
    on_left_press: None,
    on_any_press: None,
    in_tab_order: true,
    touch: None,
    radius: 6.0,
  }
}

impl<'a, Message, Renderer> Focus<'a, Message, Renderer> {
  /// Message envoyé par Entrée ou Espace quand le contenu a le focus.
  #[must_use]
  pub fn on_activate(mut self, message: Option<Message>) -> Self {
    self.on_activate = message;
    self
  }

  /// Message du menu contextuel : clic droit, ou touche Menu et Maj+F10 au clavier.
  #[must_use]
  pub fn on_context(mut self, on_context: impl Fn(Point) -> Message + 'a) -> Self {
    self.on_context = Some(Box::new(on_context));
    self
  }

  /// Message d'un clic gauche, avec sa position : bouton qui ouvre un menu.
  #[must_use]
  pub fn on_left_press(mut self, on_left_press: impl Fn(Point) -> Message + 'a) -> Self {
    self.on_left_press = Some(Box::new(on_left_press));
    self
  }

  /// Message d'un clic gauche sur le contenu, même si le contenu le garde (champ de saisie).
  /// Le conteneur sort alors de l'ordre de tabulation : le contenu y est déjà.
  #[must_use]
  pub fn observe_press(mut self, message: Message) -> Self {
    self.on_any_press = Some(message);
    self.in_tab_order = false;
    self
  }

  /// Survol animé, appui et onde du bouton (`widgets/touch.rs`).
  #[must_use]
  pub fn touch(mut self, touch: Touch<'a, Message>) -> Self {
    self.touch = Some(touch);
    self
  }

  /// Comme [`Focus::ripple`], avec la clé de l'instance donnée par l'appelant (identifiant de liste et rang, hachage d'un titre) : deux
  /// rangées d'une même liste ne partagent pas leur onde.
  #[must_use]
  pub fn ripple_key(mut self, key: ControlKey, color: Color, radius: f32) -> Self
  where
    Message: Touchable + 'a,
  {
    self.touch = Touch::new(key, move |_| color, radius);
    self
  }

  /// Onde du clic seule (`rippleOut`), sans survol animé ni `.pressed` : `label`, le fichier de l'appel et le rang d'apparition dans la
  /// vue donnent la clé (`motion::touch_key`), `color` est `currentColor` et `radius` le rayon de coin du bouton.
  #[track_caller]
  #[must_use]
  pub fn ripple(mut self, label: &str, color: Color, radius: f32) -> Self
  where
    Message: Touchable + 'a,
  {
    let key = crate::motion::touch_key(std::panic::Location::caller(), label);
    self.touch = Touch::new(key, move |_| color, radius);
    self
  }

  /// Comme [`Focus::ripple`], avec `currentColor` lue dans le thème : le texte du style du bouton au repos.
  #[track_caller]
  #[must_use]
  pub fn ripple_by(mut self, label: &str, color: impl Fn(&Theme) -> Color + 'a, radius: f32) -> Self
  where
    Message: Touchable + 'a,
  {
    let key = crate::motion::touch_key(std::panic::Location::caller(), label);
    self.touch = Touch::new(key, color, radius);
    self
  }

  /// Rayon de l'anneau de focus.
  #[must_use]
  pub fn radius(mut self, radius: f32) -> Self {
    self.radius = radius;
    self
  }
}

impl<Message, Renderer> Widget<Message, Theme, Renderer> for Focus<'_, Message, Renderer>
where
  Message: Clone,
  Renderer: iced::advanced::Renderer + geometry::Renderer,
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
    // `keyed_column` appelle `diff` sans comparer les types quand les clés changent : l'arbre peut être celui d'un autre widget.
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
    if self.in_tab_order {
      operation.focusable(None, layout.bounds(), tree.state.downcast_mut::<State>());
    }
    operation.traverse(&mut |operation| {
      self
        .content
        .as_widget_mut()
        .operate(&mut tree.children[0], layout, renderer, operation);
    });
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
    if let (Some(message), Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))) =
      (&self.on_any_press, event)
      && cursor.is_over(layout.bounds())
    {
      shell.publish(message.clone());
    }
    self.touch_event(
      tree.state.downcast_mut::<State>(),
      event,
      layout.bounds(),
      cursor,
      shell,
    );
    if shell.is_event_captured() {
      return;
    }
    let state = tree.state.downcast_mut::<State>();
    let bounds = layout.bounds();
    match event {
      Event::Mouse(mouse::Event::ButtonPressed(button)) => {
        if state.focused {
          state.focused = false;
          shell.request_redraw();
        }
        let on_position = match *button {
          mouse::Button::Right => self.on_context.as_ref(),
          mouse::Button::Left => self.on_left_press.as_ref(),
          _ => None,
        };
        if let (Some(on_position), Some(position)) = (on_position, cursor.position_over(bounds)) {
          shell.publish(on_position(position));
          shell.capture_event();
        }
      }
      Event::Keyboard(keyboard::Event::KeyPressed {
        key,
        modifiers,
        repeat: false,
        ..
      }) if state.focused => match key {
        keyboard::Key::Named(key::Named::Enter | key::Named::Space) => {
          if let Some(message) = &self.on_activate {
            shell.publish(message.clone());
            shell.capture_event();
          }
        }
        keyboard::Key::Named(key::Named::ContextMenu) => self.open_context(bounds, shell),
        keyboard::Key::Named(key::Named::F10) if modifiers.shift() => {
          self.open_context(bounds, shell)
        }
        _ => {}
      },
      _ => {}
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
    if let Some(touch) = &self.touch {
      draw_ripples(renderer, layout.bounds(), touch, theme);
    }
    if tree.state.downcast_ref::<State>().focused {
      renderer.fill_quad(
        renderer::Quad {
          bounds: layout.bounds().expand(RING_OFFSET),
          border: Border {
            color: theme.extended_palette().primary.base.color,
            width: RING_WIDTH,
            radius: self.radius.into(),
          },
          ..renderer::Quad::default()
        },
        Color::TRANSPARENT,
      );
    }
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

impl<Message: Clone, Renderer> Focus<'_, Message, Renderer> {
  /// Survol, appui et relâchement du bouton : l'appui est vu avant le contenu (`pointerdown` en capture, `app.js:2607`).
  fn touch_event(
    &self,
    state: &mut State,
    event: &Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
    shell: &mut Shell<'_, Message>,
  ) {
    let Some(touch) = &self.touch else {
      return;
    };
    // Le survol est recalculé à chaque événement, redessin compris : un bouton qui apparaît sous un pointeur immobile est survolé à
    // l'image suivante, un bouton remplacé par une autre instance à sa place laisse partir l'ancienne.
    if let Some(hover) = &touch.hover {
      for message in state.hover.sync(hover.key, cursor.is_over(bounds)) {
        shell.publish((hover.send)(message));
      }
    }
    let Event::Mouse(event) = event else {
      return;
    };
    match event {
      mouse::Event::ButtonPressed(mouse::Button::Left) => {
        if let Some(position) = cursor.position_over(bounds) {
          state.pressed = true;
          let origin = Point::new(position.x - bounds.x, position.y - bounds.y);
          shell.publish((touch.press)(origin, bounds.size()));
        }
      }
      mouse::Event::ButtonReleased(mouse::Button::Left) if state.pressed => {
        state.pressed = false;
        shell.publish(touch.release.clone());
      }
      _ => {}
    }
  }
}

impl<Message, Renderer> Focus<'_, Message, Renderer> {
  fn open_context(&self, bounds: Rectangle, shell: &mut Shell<'_, Message>) {
    if let Some(on_context) = &self.on_context {
      shell.publish(on_context(bounds.center()));
      shell.capture_event();
    }
  }
}

impl<'a, Message, Renderer> From<Focus<'a, Message, Renderer>>
  for Element<'a, Message, Theme, Renderer>
where
  Message: Clone + 'a,
  Renderer: iced::advanced::Renderer + geometry::Renderer + 'a,
{
  fn from(focus: Focus<'a, Message, Renderer>) -> Self {
    Element::new(focus)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_focus_cycle_never_passes_through_nothing() {
    assert_eq!(cycle_target(None, 3, false), Some(0));
    assert_eq!(cycle_target(None, 3, true), Some(2));
    assert_eq!(cycle_target(Some(2), 3, false), Some(0));
    assert_eq!(cycle_target(Some(0), 3, true), Some(2));
    assert_eq!(cycle_target(Some(0), 3, false), Some(1));
    assert_eq!(cycle_target(None, 0, false), None);
  }
  use iced::advanced::clipboard;
  use iced::advanced::widget::operation::focusable as ops;
  use iced::keyboard::{Location, Modifiers};
  use iced::widget::space;

  #[derive(Debug, Clone, PartialEq)]
  enum Msg {
    Activate,
    Context(Point),
  }

  type TestFocus = Focus<'static, Msg, ()>;

  fn widget() -> TestFocus {
    focusable(space().width(100).height(40))
      .on_activate(Some(Msg::Activate))
      .on_context(Msg::Context)
  }

  fn node() -> layout::Node {
    layout::Node::new(Size::new(100.0, 40.0))
  }

  fn key_press(key: keyboard::Key, modifiers: Modifiers, repeat: bool) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
      modified_key: key.clone(),
      physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
      key,
      location: Location::Standard,
      modifiers,
      text: None,
      repeat,
    })
  }

  fn send(
    widget: &mut TestFocus,
    tree: &mut Tree,
    event: &Event,
    cursor: mouse::Cursor,
  ) -> Vec<Msg> {
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    let node = node();
    let viewport = Rectangle::with_size(Size::new(100.0, 40.0));
    widget.update(
      tree,
      event,
      Layout::new(&node),
      cursor,
      &(),
      &mut clipboard::Null,
      &mut shell,
      &viewport,
    );
    messages
  }

  fn named(name: key::Named) -> Event {
    key_press(keyboard::Key::Named(name), Modifiers::default(), false)
  }

  fn tree_of(widget: &TestFocus, focused: bool) -> Tree {
    let mut tree = Tree {
      tag: tree::Tag::of::<State>(),
      state: widget.state(),
      children: widget.children(),
    };
    tree.state.downcast_mut::<State>().focused = focused;
    tree
  }

  // `keyed_column` appelle `diff` directement sur l'état d'un autre widget quand les clés changent.
  #[test]
  fn diff_on_the_tree_of_another_widget_gives_a_usable_state() {
    let mut widget = widget();
    let mut tree = Tree::empty();
    Widget::diff(&widget, &mut tree);
    assert!(
      send(
        &mut widget,
        &mut tree,
        &named(key::Named::Enter),
        mouse::Cursor::Unavailable
      )
      .is_empty()
    );
    assert_eq!(tree.children.len(), 1);
  }

  #[test]
  fn diff_keeps_the_focus_of_the_same_widget() {
    let widget = widget();
    let mut tree = tree_of(&widget, true);
    Widget::diff(&widget, &mut tree);
    assert!(tree.state.downcast_ref::<State>().focused);
  }

  #[test]
  fn enter_and_space_activate_when_focused() {
    let mut widget = widget();
    let mut tree = tree_of(&widget, true);
    for name in [key::Named::Enter, key::Named::Space] {
      assert_eq!(
        send(
          &mut widget,
          &mut tree,
          &named(name),
          mouse::Cursor::Unavailable
        ),
        vec![Msg::Activate]
      );
    }
  }

  #[test]
  fn keys_do_nothing_without_focus() {
    let mut widget = widget();
    let mut tree = tree_of(&widget, false);
    assert!(
      send(
        &mut widget,
        &mut tree,
        &named(key::Named::Enter),
        mouse::Cursor::Unavailable
      )
      .is_empty()
    );
  }

  #[test]
  fn key_repeat_does_not_activate_again() {
    let mut widget = widget();
    let mut tree = tree_of(&widget, true);
    let event = key_press(
      keyboard::Key::Named(key::Named::Enter),
      Modifiers::default(),
      true,
    );
    assert!(send(&mut widget, &mut tree, &event, mouse::Cursor::Unavailable).is_empty());
  }

  #[test]
  fn a_disabled_widget_does_not_activate() {
    let mut widget: TestFocus = focusable(space().width(100).height(40)).on_activate(None);
    let mut tree = tree_of(&widget, true);
    assert!(
      send(
        &mut widget,
        &mut tree,
        &named(key::Named::Enter),
        mouse::Cursor::Unavailable
      )
      .is_empty()
    );
  }

  #[test]
  fn a_right_click_opens_the_context_menu_at_the_cursor() {
    let mut widget = widget();
    let mut tree = tree_of(&widget, false);
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
    let position = Point::new(30.0, 10.0);
    assert_eq!(
      send(
        &mut widget,
        &mut tree,
        &press,
        mouse::Cursor::Available(position)
      ),
      vec![Msg::Context(position)]
    );
    let outside = mouse::Cursor::Available(Point::new(300.0, 10.0));
    assert!(send(&mut widget, &mut tree, &press, outside).is_empty());
  }

  #[test]
  fn the_menu_key_and_shift_f10_open_the_context_menu_at_the_center() {
    let mut widget = widget();
    let mut tree = tree_of(&widget, true);
    let center = Msg::Context(Point::new(50.0, 20.0));
    assert_eq!(
      send(
        &mut widget,
        &mut tree,
        &named(key::Named::ContextMenu),
        mouse::Cursor::Unavailable
      ),
      vec![center.clone()]
    );
    let shift_f10 = key_press(
      keyboard::Key::Named(key::Named::F10),
      Modifiers::SHIFT,
      false,
    );
    assert_eq!(
      send(
        &mut widget,
        &mut tree,
        &shift_f10,
        mouse::Cursor::Unavailable
      ),
      vec![center]
    );
  }

  #[test]
  fn a_click_removes_the_keyboard_focus() {
    let mut widget = widget();
    let mut tree = tree_of(&widget, true);
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    send(
      &mut widget,
      &mut tree,
      &press,
      mouse::Cursor::Available(Point::new(1.0, 1.0)),
    );
    assert!(!tree.state.downcast_ref::<State>().focused);
  }

  #[test]
  fn tab_operations_reach_the_widget() {
    let mut widget = widget();
    let mut tree = tree_of(&widget, false);
    let node = node();
    let mut operation = ops::focus_next::<()>();
    Widget::operate(
      &mut widget,
      &mut tree,
      Layout::new(&node),
      &(),
      &mut operation,
    );
    let iced::advanced::widget::operation::Outcome::Chain(mut next) =
      iced::advanced::widget::Operation::finish(&operation)
    else {
      panic!("attendu : opération en deux temps");
    };
    Widget::operate(
      &mut widget,
      &mut tree,
      Layout::new(&node),
      &(),
      next.as_mut(),
    );
    assert!(tree.state.downcast_ref::<State>().focused);
  }
}
