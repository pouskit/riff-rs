//! Les 5 raccourcis globaux de l'original (`GLOBAL_SHORTCUTS`, `main.js`), par `global-hotkey`.

use super::Command;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};

/// Combinaisons et commandes. Les touches média physiques passent par la session système, pas par ici.
#[must_use]
pub fn bindings() -> [(HotKey, Command); 5] {
  let ctrl_shift = Modifiers::CONTROL | Modifiers::SHIFT;
  let ctrl_alt_shift = Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT;
  [
    (HotKey::new(Some(ctrl_shift), Code::Space), Command::Toggle),
    (
      HotKey::new(Some(ctrl_alt_shift), Code::ArrowRight),
      Command::Next,
    ),
    (
      HotKey::new(Some(ctrl_alt_shift), Code::ArrowLeft),
      Command::Previous,
    ),
    (
      HotKey::new(Some(ctrl_alt_shift), Code::ArrowUp),
      Command::VolumeUp,
    ),
    (
      HotKey::new(Some(ctrl_alt_shift), Code::ArrowDown),
      Command::VolumeDown,
    ),
  ]
}

/// Commande liée à l'identifiant d'un raccourci.
#[must_use]
pub fn command_for(id: u32) -> Option<Command> {
  bindings()
    .into_iter()
    .find(|(hotkey, _)| hotkey.id() == id)
    .map(|(_, command)| command)
}

/// Texte lisible d'une combinaison, pour le journal et les réglages.
#[must_use]
pub fn describe(hotkey: &HotKey) -> String {
  hotkey.into_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashSet;

  #[test]
  fn five_distinct_bindings() {
    let ids: HashSet<u32> = bindings().iter().map(|(hotkey, _)| hotkey.id()).collect();
    assert_eq!(ids.len(), 5);
  }

  #[test]
  fn play_pause_is_ctrl_shift_space() {
    let (hotkey, command) = &bindings()[0];
    assert_eq!(*command, Command::Toggle);
    assert_eq!(hotkey.mods, Modifiers::CONTROL | Modifiers::SHIFT);
    assert_eq!(hotkey.key, Code::Space);
  }

  #[test]
  fn arrows_use_ctrl_alt_shift_and_map_like_the_original() {
    let expected = [
      (Code::ArrowRight, Command::Next),
      (Code::ArrowLeft, Command::Previous),
      (Code::ArrowUp, Command::VolumeUp),
      (Code::ArrowDown, Command::VolumeDown),
    ];
    for ((hotkey, command), (key, wanted)) in bindings().iter().skip(1).zip(expected) {
      assert_eq!(hotkey.key, key);
      assert_eq!(*command, wanted);
      assert_eq!(
        hotkey.mods,
        Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT
      );
    }
  }

  #[test]
  fn ids_resolve_back_to_commands() {
    for (hotkey, command) in bindings() {
      assert_eq!(command_for(hotkey.id()), Some(command));
    }
    assert_eq!(command_for(0), None);
  }
}
