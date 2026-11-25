//! Tri des listes de pistes (`track-sort.js`).

use crate::user_data::{SortMode, Track};
use std::cmp::Reverse;

/// Ordre d'affichage : indices dans `tracks`. Sans date, une piste compte comme la plus ancienne ; à date égale, l'ordre est gardé.
#[must_use]
pub fn sorted_indices(tracks: &[Track], mode: SortMode) -> Vec<usize> {
  let mut order: Vec<usize> = (0..tracks.len()).collect();
  let added = |index: usize| tracks[index].added_at.unwrap_or(0);
  match mode {
    SortMode::Manual => {}
    SortMode::AddedDesc => order.sort_by_key(|&index| Reverse(added(index))),
    SortMode::AddedAsc => order.sort_by_key(|&index| added(index)),
  }
  order
}

/// Date les pistes qui n'ont pas `addedAt` : favoris `newest_first`, playlists les derniers titres les plus récents.
/// Renvoie `true` si une piste a changé.
pub fn backfill_added_at(tracks: &mut [Track], newest_first: bool, now_ms: i64) -> bool {
  let len = tracks.len() as i64;
  let mut changed = false;
  for (index, track) in (0_i64..).zip(tracks.iter_mut()) {
    if track.added_at.is_none() {
      let age = if newest_first { index } else { len - index };
      track.added_at = Some(now_ms - age * 1000);
      changed = true;
    }
  }
  changed
}

/// Déplace un élément, comme le glisser-déposer de `enableTrackSort`.
pub fn move_item<T>(list: &mut Vec<T>, from: usize, to: usize) -> bool {
  if from == to || from >= list.len() || to >= list.len() {
    return false;
  }
  let moved = list.remove(from);
  list.insert(to, moved);
  true
}

/// Nouvel indice d'un élément de la liste après `move_item(from, to)`.
#[must_use]
pub const fn remap_after_move(index: usize, from: usize, to: usize) -> usize {
  if index == from {
    to
  } else if from < to && index > from && index <= to {
    index - 1
  } else if from > to && index >= to && index < from {
    index + 1
  } else {
    index
  }
}

/// Fait remonter les pistes épinglées : la dernière épinglée en tête (`floatPinned`).
pub fn float_pinned(
  order: &mut Vec<usize>,
  tracks: &[Track],
  pinned_ids: &[&str],
  default_platform: &str,
) {
  if pinned_ids.is_empty() {
    return;
  }
  let rank = |index: usize| {
    let id = tracks[index].canonical_id(default_platform);
    pinned_ids.iter().position(|pinned| *pinned == id)
  };
  let mut pinned: Vec<(usize, usize)> = Vec::new();
  order.retain(|&index| match rank(index) {
    Some(at) => {
      pinned.push((at, index));
      false
    }
    None => true,
  });
  pinned.sort_by_key(|&(at, _)| Reverse(at));
  order.splice(0..0, pinned.into_iter().map(|(_, index)| index));
}

#[cfg(test)]
mod tests {
  use super::*;

  const NOW: i64 = 1_800_000_000_000;

  fn track(id: &str, added_at: Option<i64>) -> Track {
    Track {
      id: Some(id.to_string()),
      added_at,
      ..Track::default()
    }
  }

  fn dated(id: &str, added_at: i64) -> Track {
    track(id, Some(added_at))
  }

  fn plain(id: &str) -> Track {
    track(id, None)
  }

  fn ids(tracks: &[Track], mode: SortMode) -> Vec<String> {
    sorted_indices(tracks, mode)
      .into_iter()
      .map(|index| tracks[index].id.clone().unwrap())
      .collect()
  }

  #[test]
  fn manual_order_keeps_the_stored_order() {
    let list = [dated("b", 3), dated("a", 1), dated("c", 2)];
    assert_eq!(ids(&list, SortMode::Manual), ["b", "a", "c"]);
  }

  #[test]
  fn date_added_newest_first() {
    let list = [dated("old", 1), dated("new", 3), dated("mid", 2)];
    assert_eq!(ids(&list, SortMode::AddedDesc), ["new", "mid", "old"]);
  }

  #[test]
  fn date_added_oldest_first() {
    let list = [dated("old", 1), dated("new", 3), dated("mid", 2)];
    assert_eq!(ids(&list, SortMode::AddedAsc), ["old", "mid", "new"]);
  }

  #[test]
  fn tracks_without_a_timestamp_count_as_oldest_and_keep_their_order() {
    let list = [plain("plain-b"), dated("dated", 5), plain("plain-a")];
    assert_eq!(
      ids(&list, SortMode::AddedDesc),
      ["dated", "plain-b", "plain-a"]
    );
  }

  #[test]
  fn sorting_does_not_mutate_the_input_list() {
    let list = [dated("a", 1), dated("b", 2)];
    let before = list.clone();
    let _ = sorted_indices(&list, SortMode::AddedDesc);
    assert_eq!(list, before);
  }

  #[test]
  fn backfill_dates_untimestamped_tracks_so_newest_first_keeps_their_current_order() {
    let mut list = [plain("first"), plain("second"), plain("third")];
    backfill_added_at(&mut list, true, NOW);
    assert!(list.iter().all(|t| t.added_at.is_some()));
    assert_eq!(
      ids(&list, SortMode::AddedDesc),
      ["first", "second", "third"]
    );
    assert_eq!(ids(&list, SortMode::AddedAsc), ["third", "second", "first"]);
  }

  #[test]
  fn backfill_dates_playlists_so_the_last_track_is_treated_as_the_newest() {
    let mut list = [plain("first"), plain("second"), plain("third")];
    backfill_added_at(&mut list, false, NOW);
    assert_eq!(
      ids(&list, SortMode::AddedDesc),
      ["third", "second", "first"]
    );
  }

  #[test]
  fn backfill_leaves_existing_timestamps_alone() {
    let mut list = [dated("old", 1), plain("plain"), dated("new", 9)];
    backfill_added_at(&mut list, true, NOW);
    assert_eq!(list[0].added_at, Some(1));
    assert_eq!(list[2].added_at, Some(9));
    assert!(list[1].added_at.is_some());
  }

  #[test]
  fn backfill_reports_no_change_when_every_track_is_already_dated() {
    let mut list = [dated("a", 1), dated("b", 2)];
    assert!(!backfill_added_at(&mut list, true, NOW));
    let mut undated = [plain("a")];
    assert!(backfill_added_at(&mut undated, true, NOW));
  }

  #[test]
  fn moving_down_and_up_reorders_the_list() {
    let mut list = vec!["a", "b", "c", "d"];
    assert!(move_item(&mut list, 0, 2));
    assert_eq!(list, ["b", "c", "a", "d"]);
    assert!(move_item(&mut list, 3, 0));
    assert_eq!(list, ["d", "b", "c", "a"]);
  }

  #[test]
  fn an_invalid_move_changes_nothing() {
    let mut list = vec!["a", "b"];
    assert!(!move_item(&mut list, 1, 1));
    assert!(!move_item(&mut list, 5, 0));
    assert!(!move_item(&mut list, 0, 5));
    assert_eq!(list, ["a", "b"]);
  }

  #[test]
  fn indices_follow_the_move() {
    assert_eq!(remap_after_move(1, 1, 3), 3);
    assert_eq!(remap_after_move(2, 1, 3), 1);
    assert_eq!(remap_after_move(3, 1, 3), 2);
    assert_eq!(remap_after_move(0, 1, 3), 0);
    assert_eq!(remap_after_move(4, 1, 3), 4);
    assert_eq!(remap_after_move(1, 3, 1), 2);
    assert_eq!(remap_after_move(2, 3, 1), 3);
    assert_eq!(remap_after_move(3, 3, 1), 1);
  }

  #[test]
  fn pinned_tracks_float_with_the_last_pinned_first() {
    let list = [plain("a"), plain("b"), plain("c"), plain("d")];
    let mut order = vec![0, 1, 2, 3];
    float_pinned(&mut order, &list, &["ytmusic:c", "ytmusic:b"], "ytmusic");
    assert_eq!(order, [1, 2, 0, 3]);
  }

  #[test]
  fn without_a_pin_the_order_is_unchanged() {
    let list = [plain("a"), plain("b")];
    let mut order = vec![1, 0];
    float_pinned(&mut order, &list, &[], "ytmusic");
    assert_eq!(order, [1, 0]);
    float_pinned(&mut order, &list, &["ytmusic:absent"], "ytmusic");
    assert_eq!(order, [1, 0]);
  }
}
