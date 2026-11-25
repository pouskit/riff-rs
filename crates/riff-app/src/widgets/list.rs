//! Liste virtualisée : un seul `scrollable`, des espaces autour des lignes visibles.

use iced::widget::scrollable::Viewport;
use iced::widget::{keyed_column, scrollable, space};
use iced::{Element, Fill, Length};
use std::ops::Range;

const OVERSCAN: usize = 4;

/// Position de défilement d'une liste à lignes de hauteur fixe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualList {
  pub row_height: f32,
  pub offset: f32,
  pub viewport_height: f32,
}

impl VirtualList {
  #[must_use]
  pub const fn new(row_height: f32, viewport_height: f32) -> Self {
    Self {
      row_height,
      offset: 0.0,
      viewport_height,
    }
  }

  /// Lignes à construire pour une liste de `len` lignes, marge comprise.
  #[must_use]
  pub fn visible(&self, len: usize) -> Range<usize> {
    let first = ((self.offset / self.row_height) as usize).saturating_sub(OVERSCAN);
    let count = (self.viewport_height / self.row_height).ceil() as usize + 2 * OVERSCAN;
    first.min(len)..first.saturating_add(count).min(len)
  }

  /// Retient le défilement signalé par le `scrollable`.
  pub fn scrolled(&mut self, offset: f32, viewport_height: f32) {
    self.offset = offset;
    self.viewport_height = viewport_height;
  }

  /// Lignes visibles entre deux espaces qui gardent la hauteur totale, sans `scrollable` : pour une liste
  /// posée dans une page qui défile d'un bloc.
  pub fn window<'a, Message: Clone + 'a>(
    &self,
    len: usize,
    row: impl Fn(usize) -> Element<'a, Message>,
  ) -> Element<'a, Message> {
    let visible = self.visible(len);
    let top = visible.start as f32 * self.row_height;
    let bottom = (len - visible.end) as f32 * self.row_height;
    let rows = visible.map(|index| (index as u64, row(index)));
    keyed_column(
      std::iter::once((u64::MAX, space().height(Length::Fixed(top)).into()))
        .chain(rows)
        .chain(std::iter::once((
          u64::MAX - 1,
          space().height(Length::Fixed(bottom)).into(),
        ))),
    )
    .width(Fill)
    .into()
  }

  /// Vue de la liste : `row` construit une ligne visible.
  pub fn view<'a, Message: Clone + 'a>(
    &self,
    id: &'static str,
    len: usize,
    on_scroll: impl Fn(Viewport) -> Message + 'a,
    row: impl Fn(usize) -> Element<'a, Message>,
  ) -> Element<'a, Message> {
    scrollable(self.window(len, row))
      .id(id)
      .on_scroll(on_scroll)
      .width(Fill)
      .height(Fill)
      .into()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_empty_list_has_no_row() {
    assert_eq!(VirtualList::new(56.0, 560.0).visible(0), 0..0);
  }

  #[test]
  fn the_top_of_a_long_list_builds_the_screen_and_the_margin() {
    assert_eq!(VirtualList::new(56.0, 560.0).visible(50_000), 0..18);
  }

  #[test]
  fn scrolling_moves_the_window_of_rows() {
    let mut list = VirtualList::new(56.0, 560.0);
    list.scrolled(56.0 * 1000.0, 560.0);
    assert_eq!(list.visible(50_000), 996..1014);
  }

  #[test]
  fn the_window_never_passes_the_end() {
    let mut list = VirtualList::new(56.0, 560.0);
    list.scrolled(56.0 * 49_990.0, 560.0);
    assert_eq!(list.visible(50_000), 49_986..50_000);
    list.scrolled(1e9, 560.0);
    let beyond = list.visible(10);
    assert!(beyond.start <= beyond.end && beyond.end == 10, "{beyond:?}");
  }

  #[test]
  fn a_short_list_is_built_entirely() {
    assert_eq!(VirtualList::new(56.0, 560.0).visible(3), 0..3);
  }

  #[test]
  fn a_taller_viewport_builds_more_rows() {
    let mut list = VirtualList::new(56.0, 560.0);
    list.scrolled(0.0, 1120.0);
    assert_eq!(list.visible(50_000), 0..28);
  }
}
