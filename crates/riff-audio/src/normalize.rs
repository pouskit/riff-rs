//! Contrôleur de normalisation de volume (`app.js`, lignes 2715-2731).

const INITIAL_AVERAGE_DB: f32 = -120.0;
const MIN_LEVEL_DB: f32 = -50.0;
const COEFFICIENT: f32 = 0.05;
const TARGET_DB: f32 = -17.0;
const MAX_GAIN_DB: f32 = 9.0;

/// Moyenne glissante du niveau. La fréquence d'appel de l'original est de 100 ms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizeState {
  average_db: f32,
}

impl Default for NormalizeState {
  fn default() -> Self {
    Self {
      average_db: INITIAL_AVERAGE_DB,
    }
  }
}

/// Renvoie le gain à appliquer, ou `None` si le niveau est trop faible pour être pris en compte.
pub fn next_gain_db(state: &mut NormalizeState, level_db: f32) -> Option<f32> {
  if level_db <= MIN_LEVEL_DB {
    return None;
  }
  state.average_db = if state.average_db <= INITIAL_AVERAGE_DB {
    level_db
  } else {
    state.average_db + (level_db - state.average_db) * COEFFICIENT
  };
  Some((TARGET_DB - state.average_db).clamp(-MAX_GAIN_DB, MAX_GAIN_DB))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn first_value_is_taken_directly() {
    let mut state = NormalizeState::default();
    assert_eq!(next_gain_db(&mut state, -20.0), Some(3.0));
  }

  #[test]
  fn low_levels_are_ignored_and_keep_the_average() {
    let mut state = NormalizeState::default();
    assert_eq!(next_gain_db(&mut state, -50.0), None);
    assert_eq!(next_gain_db(&mut state, -120.0), None);
    assert_eq!(state, NormalizeState::default());
    next_gain_db(&mut state, -20.0);
    let before = state;
    assert_eq!(next_gain_db(&mut state, -60.0), None);
    assert_eq!(state, before);
  }

  #[test]
  fn average_moves_by_five_percent() {
    let mut state = NormalizeState::default();
    next_gain_db(&mut state, -20.0);
    let gain = next_gain_db(&mut state, -10.0);
    let average = -20.0 + (-10.0 - -20.0) * 0.05;
    assert_eq!(gain, Some(-17.0 - average));
  }

  #[test]
  fn sequence_converges_towards_the_target() {
    let mut state = NormalizeState::default();
    let mut last = None;
    for _ in 0..1000 {
      last = next_gain_db(&mut state, -12.0);
    }
    let gain = last.expect("niveau pris en compte");
    assert!((gain - -5.0).abs() < 1e-3, "gain {gain}");
  }

  #[test]
  fn gain_is_bounded_to_nine_db() {
    let mut quiet = NormalizeState::default();
    assert_eq!(next_gain_db(&mut quiet, -40.0), Some(9.0));
    let mut loud = NormalizeState::default();
    assert_eq!(next_gain_db(&mut loud, 0.0), Some(-9.0));
  }
}
