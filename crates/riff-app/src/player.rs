//! File de lecture et transport, sans E/S (`playTrack`, `playNext`, `playPrev`, événements de l'élément audio).

use crate::user_data::Track;
use std::time::Duration;

/// Seuil au-delà duquel « précédent » recommence le titre (`playPrev`).
pub const PREVIOUS_RESTART_AFTER: Duration = Duration::from_secs(3);
const QUEUE_HISTORY_MAX: usize = 200;
const PLAY_HISTORY_MAX: usize = 100;
/// Préchargement du suivant et préparation de la lecture auto, après ce temps de lecture.
const PREPARE_AFTER: Duration = Duration::from_secs(5);
const RADIO_REFILL_REMAINING: usize = 3;
pub(crate) const MAX_STREAM_RETRIES: u32 = 2;
const CUT_MIN_DURATION: f64 = 25.0;
const CUT_MARGIN: f64 = 12.0;
const CROSSFADE_MIN_DURATION: f64 = 20.0;

/// Ce que « suivant » demande à l'application.
#[derive(Debug, Clone, PartialEq)]
pub enum NextStep {
  /// File vide, ou radio sans suite pour le moment.
  Nothing,
  Play(Box<Track>),
  /// Fin de la radio : compléter la file, puis rappeler [`PlayerState::advance`].
  RefillRadio,
  /// Fin de file : chercher des titres, puis [`PlayerState::append_autoplay`] ou [`PlayerState::advance`].
  Autoplay,
}

/// Ce que « précédent » demande à l'application.
#[derive(Debug, Clone, PartialEq)]
pub enum PrevStep {
  Nothing,
  /// Recommencer le titre en cours.
  Restart,
  Play(Box<Track>),
}

/// Ce qu'une fin de titre demande à l'application.
#[derive(Debug, Clone, PartialEq)]
pub enum EndStep {
  /// Répétition : relire le titre depuis le début.
  Restart,
  /// Flux coupé avant la fin : reprendre à cette position.
  Recover(Duration),
  /// Enchaîner : le titre suivant, par [`PlayerState::next`].
  Next,
}

/// Déclencheurs portés par la position de lecture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PositionTriggers {
  /// Lancer le fondu enchaîné vers le suivant, de cette durée.
  pub crossfade: Option<u32>,
  /// Précharger le flux du suivant et préparer la lecture auto.
  pub prepare_next: bool,
}

/// État de la file et du transport.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
  pub queue: Vec<Track>,
  /// `queueIndex` : `None` si aucun titre de la file n'est en cours.
  pub index: Option<usize>,
  pub queue_history: Vec<usize>,
  pub play_history: Vec<Track>,
  pub current: Option<Track>,
  pub shuffle: bool,
  pub repeat: bool,
  /// `isRadioQueue`.
  pub radio: bool,
  /// `myWaveActive`.
  pub my_wave: bool,
  /// Portée (`favorites`, `pl:<id>`) de la liste qui a produit la file ; `None` : autre origine.
  pub scope: Option<String>,
  /// Durée du fondu enchaîné en secondes, 0 : désactivé.
  pub crossfade: u32,
  /// Le titre demandé n'a pas encore commencé.
  pub loading: bool,
  /// Le chargement en cours se termine par un fondu : le titre précédent joue encore.
  pub crossfade_pending: bool,
  /// Jeton du dernier titre demandé : une réponse plus ancienne est ignorée.
  pub token: u64,
  pub stream_retries: u32,
  crossfade_triggered: bool,
  prepared: bool,
  rng: u64,
}

impl PlayerState {
  #[must_use]
  pub fn new(seed: u64) -> Self {
    Self {
      queue: Vec::new(),
      index: None,
      queue_history: Vec::new(),
      play_history: Vec::new(),
      current: None,
      shuffle: false,
      repeat: false,
      radio: false,
      my_wave: false,
      scope: None,
      crossfade: 0,
      loading: false,
      crossfade_pending: false,
      token: 0,
      stream_retries: 0,
      crossfade_triggered: false,
      prepared: false,
      rng: seed | 1,
    }
  }

  /// Générateur xorshift : `update` reste pur et les tests reproductibles.
  fn random_below(&mut self, bound: usize) -> usize {
    self.rng ^= self.rng << 13;
    self.rng ^= self.rng >> 7;
    self.rng ^= self.rng << 17;
    (self.rng % bound.max(1) as u64) as usize
  }

  /// Mélange de Fisher-Yates (bouton « Lire » d'une playlist).
  pub fn shuffle_list<T>(&mut self, list: &mut [T]) {
    for index in (1..list.len()).rev() {
      let other = self.random_below(index + 1);
      list.swap(index, other);
    }
  }

  fn record_played(&mut self, index: usize) {
    if self.queue_history.last() != Some(&index) {
      self.queue_history.push(index);
    }
    if self.queue_history.len() > QUEUE_HISTORY_MAX {
      self.queue_history.remove(0);
    }
  }

  fn push_play_history(&mut self, track: Track) {
    if self
      .play_history
      .last()
      .is_some_and(|last| last.id == track.id)
    {
      return;
    }
    self.play_history.push(track);
    if self.play_history.len() > PLAY_HISTORY_MAX {
      self.play_history.remove(0);
    }
  }

  fn start(&mut self, track: Track, going_back: bool) {
    if let Some(current) = self.current.take()
      && current.id != track.id
      && !going_back
    {
      self.push_play_history(current);
    }
    self.current = Some(track);
    self.loading = true;
    self.token += 1;
    self.stream_retries = 0;
    self.crossfade_triggered = false;
    self.crossfade_pending = false;
    self.prepared = false;
  }

  /// `playTrack` avec une liste : la liste devient la file.
  pub fn play_list(&mut self, track: Track, list: Vec<Track>, start: usize, platform: &str) {
    self.start(track.clone(), false);
    self.scope = None;
    self.queue = list;
    let index = if self
      .queue
      .get(start)
      .is_some_and(|at| at.is_same(&track, platform))
    {
      Some(start)
    } else {
      self
        .queue
        .iter()
        .position(|at| at.is_same(&track, platform))
    };
    let index = index.unwrap_or_else(|| {
      self.queue.insert(0, track);
      0
    });
    self.index = Some(index);
    self.queue_history = vec![index];
  }

  /// `playTrack` sans liste : le titre rejoint la file s'il n'y est pas.
  pub fn play_single(&mut self, track: Track, platform: &str) {
    self.start(track.clone(), false);
    let index = self
      .queue
      .iter()
      .position(|at| at.is_same(&track, platform))
      .unwrap_or_else(|| {
        self.queue.push(track);
        self.queue.len() - 1
      });
    self.index = Some(index);
    self.record_played(index);
  }

  /// Titre `index` de la file, choisi par l'utilisateur ou par `next`.
  fn play_index(&mut self, index: usize) -> Option<Track> {
    let track = self.queue.get(index)?.clone();
    self.start(track.clone(), false);
    self.index = Some(index);
    self.record_played(index);
    Some(track)
  }

  /// Faut-il compléter la radio après le démarrage d'un titre ?
  #[must_use]
  pub fn radio_needs_refill(&self) -> bool {
    self.radio && self.remaining() <= RADIO_REFILL_REMAINING
  }

  fn remaining(&self) -> usize {
    self.index.map_or(self.queue.len(), |index| {
      self.queue.len().saturating_sub(index + 1)
    })
  }

  fn at_end(&self) -> bool {
    self.remaining() == 0
  }

  /// `playNext`. `blocked` dit si un titre est bloqué.
  pub fn next(&mut self, blocked: &dyn Fn(&Track) -> bool, autoplay: bool) -> NextStep {
    if self.queue.is_empty() {
      return NextStep::Nothing;
    }
    if self.radio && self.at_end() {
      return NextStep::RefillRadio;
    }
    if !self.radio && !self.shuffle && self.at_end() && autoplay {
      return NextStep::Autoplay;
    }
    self.advance(blocked)
  }

  /// Suite de `playNext` sans compléter la file : mélange, sinon le suivant, sinon retour au début.
  pub fn advance(&mut self, blocked: &dyn Fn(&Track) -> bool) -> NextStep {
    if self.queue.is_empty() {
      return NextStep::Nothing;
    }
    if self.shuffle {
      let eligible: Vec<usize> = (0..self.queue.len())
        .filter(|&index| !blocked(&self.queue[index]))
        .collect();
      if eligible.is_empty() {
        return NextStep::Nothing;
      }
      let keep = (eligible.len() / 2).max(1);
      let recent = &self.queue_history[self.queue_history.len().saturating_sub(keep)..];
      let pool: Vec<usize> = eligible
        .iter()
        .copied()
        .filter(|index| !recent.contains(index))
        .collect();
      let candidates = if pool.is_empty() { &eligible } else { &pool };
      let pick = candidates[self.random_below(candidates.len())];
      return self
        .play_index(pick)
        .map_or(NextStep::Nothing, |track| NextStep::Play(Box::new(track)));
    }
    let len = self.queue.len();
    let mut next = self.index.map_or(0, |index| index + 1);
    let mut attempts = 0;
    while attempts < len {
      if next >= len {
        if self.my_wave {
          return NextStep::Nothing;
        }
        next = 0;
      }
      if !blocked(&self.queue[next]) {
        break;
      }
      next += 1;
      attempts += 1;
    }
    let next = if next >= len { 0 } else { next };
    self
      .play_index(next)
      .map_or(NextStep::Nothing, |track| NextStep::Play(Box::new(track)))
  }

  /// Le prochain « suivant » sera un tirage du mélange (et non un complément de radio).
  #[must_use]
  pub fn shuffle_applies(&self) -> bool {
    self.shuffle && !self.queue.is_empty() && !(self.radio && self.at_end())
  }

  /// Joue le titre `index` de la file, choisi par un plugin de mélange.
  pub fn play_at(&mut self, index: usize) -> NextStep {
    self
      .play_index(index)
      .map_or(NextStep::Nothing, |track| NextStep::Play(Box::new(track)))
  }

  /// Titre imposé par l'hôte d'un jam : la file ne change pas, `index` est sa place dans la file.
  pub fn play_external(&mut self, track: Track, index: Option<usize>) {
    self.start(track, false);
    self.index = index;
    self.queue_history = index.into_iter().collect();
  }

  /// La radio a reçu des titres : joue le premier qui n'est pas bloqué, à partir du suivant.
  pub fn play_after_refill(&mut self, blocked: &dyn Fn(&Track) -> bool) -> NextStep {
    let start = self.index.map_or(0, |index| index + 1);
    let found = (start..self.queue.len()).find(|&index| !blocked(&self.queue[index]));
    match found {
      Some(index) => self
        .play_index(index)
        .map_or(NextStep::Nothing, |track| NextStep::Play(Box::new(track))),
      None => self.advance(blocked),
    }
  }

  /// `startAutoplay` : les titres trouvés rejoignent la file, qui devient une radio.
  pub fn append_autoplay(&mut self, found: Vec<Track>) -> NextStep {
    if found.is_empty() {
      return NextStep::Nothing;
    }
    let start = self.queue.len();
    self.queue.extend(found);
    self.radio = true;
    self
      .play_index(start)
      .map_or(NextStep::Nothing, |track| NextStep::Play(Box::new(track)))
  }

  /// `playPrev`.
  pub fn prev(&mut self, position: Duration, platform: &str) -> PrevStep {
    if position > PREVIOUS_RESTART_AFTER {
      return PrevStep::Restart;
    }
    if let Some(previous) = self.play_history.pop() {
      self.start(previous.clone(), true);
      if let Some(index) = self
        .queue
        .iter()
        .position(|at| at.id == previous.id || at.is_same(&previous, platform))
      {
        self.index = Some(index);
      }
      return PrevStep::Play(Box::new(previous));
    }
    if self.queue.is_empty() {
      return PrevStep::Nothing;
    }
    if self.queue_history.len() > 1 {
      self.queue_history.pop();
      if let Some(&index) = self.queue_history.last()
        && index < self.queue.len()
      {
        let track = self.queue[index].clone();
        self.start(track.clone(), false);
        self.index = Some(index);
        return PrevStep::Play(Box::new(track));
      }
    }
    let previous = self
      .index
      .filter(|&index| index > 0)
      .map_or(self.queue.len() - 1, |index| index - 1);
    let track = self.queue[previous].clone();
    self.start(track.clone(), false);
    self.index = Some(previous);
    self.queue_history = vec![previous];
    PrevStep::Play(Box::new(track))
  }

  /// Fin du titre, événement `ended`.
  pub fn ended(&mut self, position: Duration, duration: f64) -> EndStep {
    if self.repeat {
      return EndStep::Restart;
    }
    let local = self.current.as_ref().is_none_or(Track::is_local);
    let pos = position.as_secs_f64();
    if !local
      && duration > CUT_MIN_DURATION
      && pos < duration - CUT_MARGIN
      && self.stream_retries < MAX_STREAM_RETRIES
    {
      self.stream_retries += 1;
      return EndStep::Recover(position);
    }
    EndStep::Next
  }

  /// `true` si le flux peut être repris après une erreur (`recoverCurrentStream`).
  pub fn retry_stream(&mut self) -> bool {
    if self.current.as_ref().is_none_or(Track::is_local)
      || self.stream_retries >= MAX_STREAM_RETRIES
    {
      return false;
    }
    self.stream_retries += 1;
    self.token += 1;
    self.loading = true;
    true
  }

  /// Le fondu enchaîné n'a pas eu lieu : il peut encore se déclencher.
  pub fn cancel_crossfade_trigger(&mut self) {
    self.crossfade_triggered = false;
  }

  /// Événements portés par la position (`timeupdate`). `live` : flux direct.
  pub fn position_changed(
    &mut self,
    position: Duration,
    duration: f64,
    live: bool,
  ) -> PositionTriggers {
    let mut triggers = PositionTriggers::default();
    let pos = position.as_secs_f64();
    let fade = f64::from(self.crossfade);
    if self.crossfade > 0
      && !self.crossfade_triggered
      && !self.repeat
      && duration > (fade * 2.0).max(CROSSFADE_MIN_DURATION)
      && pos >= duration - fade
      && !live
    {
      self.crossfade_triggered = true;
      triggers.crossfade = Some(self.crossfade);
    }
    if !self.prepared && position > PREPARE_AFTER {
      self.prepared = true;
      triggers.prepare_next = true;
    }
    triggers
  }

  /// Le titre qui suivra, pour le préchargement. `None` en mélange : le tirage n'est pas encore fait.
  #[must_use]
  pub fn upcoming(&self) -> Option<&Track> {
    if self.shuffle {
      return None;
    }
    self.queue.get(self.index? + 1)
  }

  /// Faut-il préparer la lecture auto (`maybePrepareAutoplay`) ?
  #[must_use]
  pub fn should_prepare_autoplay(&self, autoplay: bool) -> bool {
    autoplay
      && !self.radio
      && !self.shuffle
      && !self.repeat
      && self.current.is_some()
      && !self.queue.is_empty()
      && self.at_end()
  }

  /// `removeQueueTracks`.
  pub fn remove_from_queue(&mut self, indices: &[usize]) {
    let mut removed: Vec<usize> = indices
      .iter()
      .copied()
      .filter(|&index| index < self.queue.len())
      .collect();
    removed.sort_unstable();
    removed.dedup();
    if removed.is_empty() {
      return;
    }
    let before = |index: usize| removed.partition_point(|&at| at < index);
    let remap = |index: usize| (!removed.contains(&index)).then(|| index - before(index));
    self.index = self.index.and_then(|index| {
      if removed.contains(&index) {
        (index - before(index)).checked_sub(1)
      } else {
        Some(index - before(index))
      }
    });
    self.queue_history = self
      .queue_history
      .iter()
      .filter_map(|&index| remap(index))
      .collect();
    let mut position = 0;
    self.queue.retain(|_| {
      let keep = !removed.contains(&position);
      position += 1;
      keep
    });
  }

  /// Glisser-déposer dans la file (`renderQueueView`).
  pub fn move_in_queue(&mut self, from: usize, to: usize) {
    if crate::track_sort::move_item(&mut self.queue, from, to) {
      let remap = |index: usize| crate::track_sort::remap_after_move(index, from, to);
      self.index = self.index.map(remap);
      for index in &mut self.queue_history {
        *index = remap(*index);
      }
    }
  }

  /// « Vider la file ».
  pub fn clear_queue(&mut self) {
    self.radio = false;
    self.queue.clear();
    self.index = None;
    self.queue_history.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const P: &str = "ytmusic";

  fn track(id: &str) -> Track {
    Track {
      id: Some(id.to_string()),
      title: Some(id.to_uppercase()),
      ..Track::default()
    }
  }

  fn tracks(ids: &[&str]) -> Vec<Track> {
    ids.iter().map(|id| track(id)).collect()
  }

  fn none(_: &Track) -> bool {
    false
  }

  fn playing(ids: &[&str], at: usize) -> PlayerState {
    let mut player = PlayerState::new(42);
    let list = tracks(ids);
    player.play_list(list[at].clone(), list, at, P);
    player
  }

  fn played(step: &NextStep) -> &str {
    let NextStep::Play(track) = step else {
      panic!("{step:?}")
    };
    track.id.as_deref().unwrap()
  }

  #[test]
  fn next_plays_the_following_track() {
    let mut player = playing(&["a", "b", "c"], 0);
    assert_eq!(played(&player.next(&none, true)), "b");
    assert_eq!(player.index, Some(1));
    assert_eq!(player.current.as_ref().unwrap().id.as_deref(), Some("b"));
  }

  #[test]
  fn next_at_the_end_asks_for_autoplay_when_it_is_on() {
    let mut player = playing(&["a", "b"], 1);
    assert_eq!(player.next(&none, true), NextStep::Autoplay);
  }

  #[test]
  fn next_at_the_end_wraps_around_without_autoplay() {
    let mut player = playing(&["a", "b"], 1);
    assert_eq!(played(&player.next(&none, false)), "a");
  }

  #[test]
  fn next_at_the_end_with_repeat_on_still_wraps_only_the_natural_end_restarts() {
    let mut player = playing(&["a", "b"], 1);
    player.repeat = true;
    assert_eq!(played(&player.next(&none, false)), "a");
    assert_eq!(
      player.ended(Duration::from_secs(10), 10.0),
      EndStep::Restart
    );
  }

  #[test]
  fn the_end_of_a_radio_asks_for_more_tracks() {
    let mut player = playing(&["a", "b"], 1);
    player.radio = true;
    assert_eq!(player.next(&none, true), NextStep::RefillRadio);
    player.queue.extend(tracks(&["c"]));
    assert_eq!(played(&player.play_after_refill(&none)), "c");
  }

  #[test]
  fn a_my_wave_without_more_tracks_stops_instead_of_looping() {
    let mut player = playing(&["a"], 0);
    player.radio = true;
    player.my_wave = true;
    assert_eq!(player.advance(&none), NextStep::Nothing);
  }

  #[test]
  fn the_radio_is_refilled_with_three_tracks_left() {
    let mut player = playing(&["a", "b", "c", "d", "e"], 0);
    player.radio = true;
    assert!(!player.radio_needs_refill());
    player.next(&none, true);
    assert!(player.radio_needs_refill());
  }

  #[test]
  fn autoplay_appends_its_tracks_and_becomes_a_radio() {
    let mut player = playing(&["a"], 0);
    assert_eq!(player.next(&none, true), NextStep::Autoplay);
    assert_eq!(played(&player.append_autoplay(tracks(&["x", "y"]))), "x");
    assert!(player.radio);
    assert_eq!(player.queue.len(), 3);
    assert_eq!(player.index, Some(1));
  }

  #[test]
  fn autoplay_without_a_result_falls_back_to_the_loop() {
    let mut player = playing(&["a", "b"], 1);
    assert_eq!(player.append_autoplay(Vec::new()), NextStep::Nothing);
    assert!(!player.radio);
    assert_eq!(played(&player.advance(&none)), "a");
  }

  #[test]
  fn blocked_tracks_are_skipped() {
    let mut player = playing(&["a", "b", "c"], 0);
    let blocked = |t: &Track| t.id.as_deref() == Some("b");
    assert_eq!(played(&player.next(&blocked, false)), "c");
  }

  #[test]
  fn shuffle_never_repeats_immediately() {
    let mut player = playing(&["a", "b", "c", "d", "e", "f"], 0);
    player.shuffle = true;
    let mut last = 0;
    for _ in 0..500 {
      player.next(&none, true);
      let index = player.index.unwrap();
      assert_ne!(index, last);
      last = index;
    }
  }

  #[test]
  fn shuffle_avoids_the_recent_half_of_the_queue() {
    let mut player = playing(&["a", "b", "c", "d"], 0);
    player.shuffle = true;
    for _ in 0..200 {
      let recent: Vec<usize> = player.queue_history.iter().rev().take(2).copied().collect();
      player.next(&none, true);
      assert!(
        !recent.contains(&player.index.unwrap()),
        "{recent:?} {:?}",
        player.index
      );
    }
  }

  #[test]
  fn shuffle_does_not_ask_for_autoplay_and_skips_blocked_tracks() {
    let mut player = playing(&["a", "b", "c"], 2);
    player.shuffle = true;
    let blocked = |t: &Track| t.id.as_deref() == Some("a");
    assert_eq!(played(&player.next(&blocked, true)), "b");
  }

  #[test]
  fn shuffle_with_only_blocked_tracks_does_nothing() {
    let mut player = playing(&["a"], 0);
    player.shuffle = true;
    assert_eq!(player.next(&|_| true, true), NextStep::Nothing);
  }

  #[test]
  fn shuffle_with_a_single_track_plays_it_again() {
    let mut player = playing(&["a"], 0);
    player.shuffle = true;
    assert_eq!(played(&player.next(&none, true)), "a");
  }

  #[test]
  fn shuffling_a_list_keeps_every_element_and_changes_the_order() {
    let mut player = PlayerState::new(42);
    let mut list: Vec<u32> = (0..50).collect();
    player.shuffle_list(&mut list);
    assert_ne!(list, (0..50).collect::<Vec<u32>>());
    list.sort_unstable();
    assert_eq!(list, (0..50).collect::<Vec<u32>>());
  }

  #[test]
  fn previous_after_three_seconds_restarts_the_track() {
    let mut player = playing(&["a", "b"], 1);
    assert_eq!(
      player.prev(Duration::from_millis(3001), P),
      PrevStep::Restart
    );
    assert_eq!(player.index, Some(1));
  }

  #[test]
  fn previous_at_three_seconds_exactly_goes_back() {
    let mut player = playing(&["a", "b"], 0);
    player.next(&none, false);
    let PrevStep::Play(track) = player.prev(PREVIOUS_RESTART_AFTER, P) else {
      panic!()
    };
    assert_eq!(track.id.as_deref(), Some("a"));
    assert_eq!(player.index, Some(0));
  }

  #[test]
  fn previous_walks_back_through_what_was_played() {
    let mut player = playing(&["a", "b", "c"], 0);
    player.next(&none, false);
    player.next(&none, false);
    let back = |player: &mut PlayerState| {
      let PrevStep::Play(track) = player.prev(Duration::ZERO, P) else {
        panic!()
      };
      track.id.unwrap()
    };
    assert_eq!(back(&mut player), "b");
    assert_eq!(back(&mut player), "a");
  }

  #[test]
  fn previous_does_not_add_to_the_play_history() {
    let mut player = playing(&["a", "b"], 0);
    player.next(&none, false);
    player.prev(Duration::ZERO, P);
    assert!(player.play_history.is_empty());
  }

  #[test]
  fn previous_at_the_start_wraps_to_the_last_track() {
    let mut player = playing(&["a", "b", "c"], 0);
    let PrevStep::Play(track) = player.prev(Duration::ZERO, P) else {
      panic!()
    };
    assert_eq!(track.id.as_deref(), Some("c"));
    assert_eq!(player.queue_history, [2]);
  }

  #[test]
  fn previous_without_a_queue_does_nothing() {
    assert_eq!(
      PlayerState::new(1).prev(Duration::ZERO, P),
      PrevStep::Nothing
    );
  }

  #[test]
  fn a_list_becomes_the_queue_and_a_missing_track_goes_first() {
    let mut player = PlayerState::new(1);
    player.play_list(track("z"), tracks(&["a", "b"]), 0, P);
    assert_eq!(player.queue[0].id.as_deref(), Some("z"));
    assert_eq!(player.index, Some(0));
    player.play_list(track("b"), tracks(&["a", "b"]), 0, P);
    assert_eq!(player.index, Some(1));
    assert_eq!(player.queue_history, [1]);
  }

  #[test]
  fn a_single_track_joins_the_queue_once() {
    let mut player = playing(&["a"], 0);
    player.play_single(track("b"), P);
    assert_eq!(player.queue.len(), 2);
    assert_eq!(player.index, Some(1));
    player.play_single(track("a"), P);
    assert_eq!(player.queue.len(), 2);
    assert_eq!(player.index, Some(0));
  }

  #[test]
  fn each_start_gets_a_new_token_so_late_answers_are_ignored() {
    let mut player = playing(&["a", "b"], 0);
    let first = player.token;
    player.next(&none, false);
    assert!(player.token > first);
    assert!(player.loading);
  }

  #[test]
  fn a_stream_cut_before_the_end_is_recovered_twice_then_moves_on() {
    let mut player = playing(&["a"], 0);
    let cut = Duration::from_secs(60);
    assert_eq!(player.ended(cut, 200.0), EndStep::Recover(cut));
    assert_eq!(player.ended(cut, 200.0), EndStep::Recover(cut));
    assert_eq!(player.ended(cut, 200.0), EndStep::Next);
  }

  #[test]
  fn a_real_end_moves_on() {
    let mut player = playing(&["a"], 0);
    assert_eq!(player.ended(Duration::from_secs(195), 200.0), EndStep::Next);
    assert_eq!(player.ended(Duration::from_secs(3), 20.0), EndStep::Next);
  }

  #[test]
  fn a_local_file_is_never_recovered() {
    let mut player = PlayerState::new(1);
    let local = Track {
      platform: Some("local".into()),
      ..track("l")
    };
    player.play_list(local.clone(), vec![local], 0, P);
    assert_eq!(player.ended(Duration::from_secs(60), 200.0), EndStep::Next);
    assert!(!player.retry_stream());
  }

  #[test]
  fn a_stream_error_is_retried_twice() {
    let mut player = playing(&["a"], 0);
    let token = player.token;
    assert!(player.retry_stream());
    assert!(player.token > token);
    assert!(player.retry_stream());
    assert!(!player.retry_stream());
  }

  fn crossfade_at(
    player: &mut PlayerState,
    position: f64,
    duration: f64,
    live: bool,
  ) -> Option<u32> {
    player
      .position_changed(Duration::from_secs_f64(position), duration, live)
      .crossfade
  }

  #[test]
  fn the_crossfade_starts_x_seconds_before_the_end_and_once() {
    let mut player = playing(&["a", "b"], 0);
    player.crossfade = 6;
    assert_eq!(crossfade_at(&mut player, 193.9, 200.0, false), None);
    assert_eq!(crossfade_at(&mut player, 194.0, 200.0, false), Some(6));
    assert_eq!(crossfade_at(&mut player, 195.0, 200.0, false), None);
  }

  #[test]
  fn the_crossfade_rule_matches_riff() {
    let mut off = playing(&["a"], 0);
    assert_eq!(crossfade_at(&mut off, 199.0, 200.0, false), None);
    let mut repeat = playing(&["a"], 0);
    repeat.crossfade = 6;
    repeat.repeat = true;
    assert_eq!(crossfade_at(&mut repeat, 199.0, 200.0, false), None);
    let mut short = playing(&["a"], 0);
    short.crossfade = 6;
    assert_eq!(crossfade_at(&mut short, 19.0, 20.0, false), None);
    let mut long_fade = playing(&["a"], 0);
    long_fade.crossfade = 12;
    assert_eq!(crossfade_at(&mut long_fade, 23.0, 24.0, false), None);
    assert_eq!(crossfade_at(&mut long_fade, 24.0, 24.1, false), Some(12));
    let mut live = playing(&["a"], 0);
    live.crossfade = 6;
    assert_eq!(crossfade_at(&mut live, 199.0, 200.0, true), None);
  }

  #[test]
  fn a_new_track_rearms_the_crossfade() {
    let mut player = playing(&["a", "b"], 0);
    player.crossfade = 6;
    assert_eq!(crossfade_at(&mut player, 199.0, 200.0, false), Some(6));
    player.next(&none, false);
    assert_eq!(crossfade_at(&mut player, 199.0, 200.0, false), Some(6));
  }

  #[test]
  fn the_next_track_is_prepared_once_after_five_seconds() {
    let mut player = playing(&["a", "b"], 0);
    let at = |player: &mut PlayerState, secs: u64| {
      player
        .position_changed(Duration::from_secs(secs), 200.0, false)
        .prepare_next
    };
    assert!(!at(&mut player, 5));
    assert!(at(&mut player, 6));
    assert!(!at(&mut player, 7));
    assert_eq!(player.upcoming().unwrap().id.as_deref(), Some("b"));
  }

  #[test]
  fn autoplay_is_prepared_only_on_the_last_track_of_a_plain_queue() {
    let last = playing(&["a"], 0);
    assert!(last.should_prepare_autoplay(true));
    assert!(!last.should_prepare_autoplay(false));
    assert!(!playing(&["a", "b"], 0).should_prepare_autoplay(true));
    let mut radio = playing(&["a"], 0);
    radio.radio = true;
    assert!(!radio.should_prepare_autoplay(true));
  }

  #[test]
  fn removing_from_the_queue_keeps_the_current_track() {
    let mut player = playing(&["a", "b", "c", "d"], 2);
    player.queue_history = vec![0, 2];
    player.remove_from_queue(&[0, 3, 99]);
    assert_eq!(player.queue, tracks(&["b", "c"]));
    assert_eq!(player.index, Some(1));
    assert_eq!(player.queue_history, [1]);
  }

  #[test]
  fn removing_the_current_track_keeps_the_next_one_next() {
    let mut player = playing(&["a", "b", "c"], 1);
    player.remove_from_queue(&[1]);
    assert_eq!(player.index, Some(0));
    assert_eq!(played(&player.next(&none, false)), "c");
  }

  #[test]
  fn removing_the_first_current_track_restarts_from_the_top() {
    let mut player = playing(&["a", "b"], 0);
    player.remove_from_queue(&[0]);
    assert_eq!(player.index, None);
    assert_eq!(played(&player.next(&none, false)), "b");
  }

  #[test]
  fn moving_in_the_queue_follows_the_current_track() {
    let mut player = playing(&["a", "b", "c"], 0);
    player.move_in_queue(0, 2);
    assert_eq!(player.index, Some(2));
    assert_eq!(player.queue_history, [2]);
    assert_eq!(player.queue[2].id.as_deref(), Some("a"));
  }

  #[test]
  fn clearing_the_queue_keeps_the_current_track_playing() {
    let mut player = playing(&["a", "b"], 0);
    player.radio = true;
    player.clear_queue();
    assert!(player.queue.is_empty());
    assert_eq!(player.index, None);
    assert!(!player.radio);
    assert!(player.current.is_some());
  }
}
