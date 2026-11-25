//! Mise en page des paroles : mots en rangées (décision 1), hauteur et position de chaque ligne.

use std::ops::Range;

/// Rangées de mots d'une ligne : un mot trop long reste seul sur sa rangée.
#[must_use]
pub fn wrap_words(widths: &[f32], available: f32, gap: f32) -> Vec<Range<usize>> {
  let mut rows = Vec::new();
  let mut start = 0;
  let mut used = 0.0;
  for (index, width) in widths.iter().enumerate() {
    if index > start && used + gap + width > available {
      rows.push(start..index);
      start = index;
      used = 0.0;
    }
    used += if index > start { gap + width } else { *width };
  }
  if start < widths.len() {
    rows.push(start..widths.len());
  }
  rows
}

/// Position verticale d'une ligne dans le contenu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
  pub top: f32,
  pub height: f32,
}

/// Empile les lignes. `padding` est ajouté en tête du contenu, `spacing` entre deux lignes.
#[must_use]
pub fn stack_lines(heights: &[f32], padding: f32, spacing: f32) -> Vec<LineBox> {
  let mut top = padding;
  heights
    .iter()
    .map(|height| {
      let line = LineBox {
        top,
        height: *height,
      };
      top += height + spacing;
      line
    })
    .collect()
}

/// `getLyricTargetScroll` : la ligne au centre de la zone, jamais avant le début.
#[must_use]
pub fn target_scroll(line: LineBox, viewport: f32) -> f32 {
  (line.top - viewport / 2.0 + line.height / 2.0).max(0.0)
}

/// Lignes qui touchent la zone visible, avec une marge.
#[must_use]
pub fn visible_lines(boxes: &[LineBox], offset: f32, viewport: f32, margin: f32) -> Range<usize> {
  let (low, high) = (offset - margin, offset + viewport + margin);
  let start = boxes.partition_point(|line| line.top + line.height <= low);
  let end = boxes.partition_point(|line| line.top < high).max(start);
  start..end
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn words_stay_on_one_row_when_they_fit() {
    assert_eq!(wrap_words(&[10.0, 20.0], 34.0, 4.0), vec![0..2]);
  }

  #[test]
  fn the_gap_counts_when_wrapping() {
    assert_eq!(wrap_words(&[10.0, 20.0], 33.0, 4.0), vec![0..1, 1..2]);
  }

  #[test]
  fn a_word_wider_than_the_zone_stays_alone() {
    assert_eq!(
      wrap_words(&[5.0, 100.0, 5.0], 20.0, 4.0),
      vec![0..1, 1..2, 2..3]
    );
  }

  #[test]
  fn no_word_gives_no_row() {
    assert_eq!(wrap_words(&[], 20.0, 4.0), Vec::<Range<usize>>::new());
  }

  #[test]
  fn lines_are_stacked_after_the_padding() {
    assert_eq!(
      stack_lines(&[10.0, 20.0, 5.0], 100.0, 2.0),
      vec![
        LineBox {
          top: 100.0,
          height: 10.0
        },
        LineBox {
          top: 112.0,
          height: 20.0
        },
        LineBox {
          top: 134.0,
          height: 5.0
        },
      ]
    );
  }

  #[test]
  fn the_target_centers_the_line() {
    assert_eq!(
      target_scroll(
        LineBox {
          top: 500.0,
          height: 40.0
        },
        200.0
      ),
      420.0
    );
  }

  #[test]
  fn the_target_stops_at_the_start() {
    assert_eq!(
      target_scroll(
        LineBox {
          top: 10.0,
          height: 40.0
        },
        200.0
      ),
      0.0
    );
  }

  #[test]
  fn only_lines_near_the_zone_are_visible() {
    let boxes = stack_lines(&[100.0; 10], 0.0, 0.0);
    assert_eq!(visible_lines(&boxes, 250.0, 200.0, 0.0), 2..5);
    assert_eq!(visible_lines(&boxes, 250.0, 200.0, 100.0), 1..6);
    assert_eq!(visible_lines(&boxes, 5000.0, 200.0, 0.0), 10..10);
    assert_eq!(visible_lines(&[], 0.0, 200.0, 0.0), 0..0);
  }
}
