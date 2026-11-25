//! État du jam côté `App` : file parallèle, participants, compteurs de l'hôte, miroir de l'invité.

use super::protocol::{Participant, PlaybackState};
use super::{Role, protocol};
use crate::player::PlayerState;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
  Starting,
  Live,
  /// Numéro de l'essai, de 1 à 3.
  Reconnecting(u8),
}

/// Qui a ajouté une entrée de la file ; `player.queue[i]` correspond à `entries[i]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMeta {
  pub uid: u64,
  pub by: u32,
  pub by_name: String,
}

/// Dernier état de lecture diffusé par l'hôte, sans la position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sent {
  pub uid: Option<u64>,
  pub start: u64,
  pub seek: u64,
  pub playing: bool,
  pub shuffle: bool,
  pub repeat: bool,
}

/// Dernier état de l'hôte reçu par l'invité, avec l'instant local de `at_ms`.
#[derive(Debug, Clone)]
pub struct HostView {
  pub state: PlaybackState,
  pub anchor: Instant,
}

/// Lecteur de l'invité avant le jam, rendu à la sortie.
#[derive(Debug, Clone)]
pub struct Saved {
  pub player: PlayerState,
}

#[derive(Debug, Clone)]
pub struct JamState {
  pub role: Role,
  pub phase: Phase,
  pub ticket: Option<String>,
  pub me: u32,
  pub participants: Vec<Participant>,
  pub entries: Vec<EntryMeta>,
  /// L'utilisateur a demandé la fin : une fin de tâche sans événement n'est pas une perte.
  pub stopping: bool,
  // Hôte.
  pub next_uid: u64,
  pub qv: u64,
  pub pv: u64,
  pub start: u64,
  pub seek: u64,
  pub sent: Option<Sent>,
  /// Suite d'`uid` de la dernière file diffusée.
  pub sent_queue: Option<Vec<u64>>,
  pub position_at: Option<Instant>,
  pub failures: u8,
  pub pending_fade: bool,
  /// Battement : diffuse l'état même sans changement.
  pub force: bool,
  // Invité.
  pub host: Option<HostView>,
  pub saved: Option<Box<Saved>>,
  pub last_correction: Option<Instant>,
  pub next_cmd: u64,
}

impl JamState {
  #[must_use]
  pub fn new(role: Role, name: &str) -> Self {
    let participants = match role {
      Role::Host => vec![Participant {
        id: 0,
        name: name.to_string(),
        host: true,
      }],
      Role::Guest => Vec::new(),
    };
    Self {
      role,
      phase: Phase::Starting,
      ticket: None,
      me: 0,
      participants,
      entries: Vec::new(),
      stopping: false,
      next_uid: 1,
      qv: 0,
      pv: 0,
      start: 0,
      seek: 0,
      sent: None,
      sent_queue: None,
      position_at: None,
      failures: 0,
      pending_fade: false,
      force: false,
      host: None,
      saved: None,
      last_correction: None,
      next_cmd: 0,
    }
  }

  #[must_use]
  pub fn is_host(&self) -> bool {
    self.role == Role::Host
  }

  #[must_use]
  pub fn is_guest(&self) -> bool {
    self.role == Role::Guest
  }

  pub const fn fresh_uid(&mut self) -> u64 {
    let uid = self.next_uid;
    self.next_uid += 1;
    uid
  }

  #[must_use]
  pub fn index_of(&self, uid: u64) -> Option<usize> {
    self.entries.iter().position(|entry| entry.uid == uid)
  }

  /// Nom du participant `id` ; `Guest` s'il est parti.
  #[must_use]
  pub fn name_of(&self, id: u32) -> String {
    self
      .participants
      .iter()
      .find(|participant| participant.id == id)
      .map_or_else(
        || protocol::DEFAULT_GUEST_NAME.to_string(),
        |participant| participant.name.clone(),
      )
  }

  /// Nom de l'hôte du jam.
  #[must_use]
  pub fn host_name(&self) -> String {
    self
      .participants
      .iter()
      .find(|participant| participant.host)
      .map_or_else(
        || protocol::DEFAULT_GUEST_NAME.to_string(),
        |participant| participant.name.clone(),
      )
  }

  /// `uid` du titre courant connu de l'hôte (invité).
  #[must_use]
  pub fn host_uid(&self) -> Option<u64> {
    self
      .host
      .as_ref()
      .and_then(|view| view.state.current.as_ref())
      .and_then(|current| current.uid)
  }
}
