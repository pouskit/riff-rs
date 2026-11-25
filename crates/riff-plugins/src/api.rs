//! Messages entre les plugins et l'application.

use serde_json::Value;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

/// Budget d'un événement (`on_ready`, `on_play`, `on_pause`, `on_trackchange`, `stop`).
pub const EVENT_BUDGET: Duration = Duration::from_millis(500);
/// Budget de `shuffle_pick`.
pub const PICK_BUDGET: Duration = Duration::from_secs(3);
/// Attente maximale d'une réponse de l'application pour `search` et `vibe`.
pub const SEARCH_WAIT: Duration = Duration::from_secs(10);
/// Attente maximale d'une réponse de l'application pour une lecture d'état (file, favoris).
pub const READ_WAIT: Duration = Duration::from_secs(1);
/// Échecs de suite avant désactivation.
pub const MAX_FAILURES: u32 = 3;

/// Événement de lecture transmis aux plugins actifs.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginEvent {
  Ready,
  Play,
  Pause,
  /// Piste nettoyée par [`crate::track::sanitize`], ou `null`.
  TrackChange(Value),
}

impl PluginEvent {
  /// Fonction appelée dans le script.
  #[must_use]
  pub const fn function(&self) -> &'static str {
    match self {
      Self::Ready => "on_ready",
      Self::Play => "on_play",
      Self::Pause => "on_pause",
      Self::TrackChange(_) => "on_trackchange",
    }
  }
}

/// Type d'une notification, comme `toast(message, type)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
  Info,
  Success,
  Warning,
  Error,
}

impl NotifyKind {
  /// Valeur inconnue : `Info`.
  #[must_use]
  pub fn parse(value: &str) -> Self {
    match value {
      "success" => Self::Success,
      "warning" | "warn" => Self::Warning,
      "error" => Self::Error,
      _ => Self::Info,
    }
  }
}

/// Demande d'un plugin à l'application.
#[derive(Debug, Clone, PartialEq)]
pub enum HostRequest {
  Track,
  IsPlaying,
  ShuffleOn,
  Queue,
  QueueIndex,
  QueueContext,
  /// Pistes déjà nettoyées par [`crate::track::incoming`]. Réponse : les pistes ajoutées.
  QueueAppend(Vec<Value>),
  Favorites,
  History,
  Search {
    query: String,
    limit: u32,
    platform: Option<String>,
  },
  Vibe(Value),
  Notify {
    message: String,
    kind: NotifyKind,
  },
  /// Texte nettoyé ; l'application le masque (`logging::redact`) avant de l'écrire.
  Log(String),
}

/// Réponse de l'application.
#[derive(Debug, Clone, PartialEq)]
pub enum HostResponse {
  Value(Value),
  Error(String),
}

/// Demande d'un plugin, avec le canal de réponse quand elle en attend une.
#[derive(Debug, Clone)]
pub struct HostCall {
  pub plugin: String,
  pub request: HostRequest,
  reply: Option<SyncSender<HostResponse>>,
}

impl HostCall {
  pub(crate) const fn new(
    plugin: String,
    request: HostRequest,
    reply: Option<SyncSender<HostResponse>>,
  ) -> Self {
    Self {
      plugin,
      request,
      reply,
    }
  }

  /// Le plugin attend-il une réponse ?
  #[must_use]
  pub const fn expects_reply(&self) -> bool {
    self.reply.is_some()
  }

  /// Répond. Un plugin arrêté ou qui n'attend plus ne bloque rien.
  pub fn reply(&self, response: HostResponse) {
    if let Some(reply) = &self.reply {
      let _ = reply.try_send(response);
    }
  }
}

/// Changement d'état d'un plugin, vu par l'application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lifecycle {
  /// Compilé, `on_ready` appelé.
  Started { file: String },
  /// Refusé à l'activation (syntaxe, état illisible).
  Failed { file: String, reason: String },
  /// Erreur d'un appel ; le plugin reste actif.
  Error { file: String, message: String },
  /// Budget dépassé, 3 échecs de suite ou panique : le moteur est détruit.
  Disabled { file: String, reason: String },
  /// Arrêt demandé : `stop()` appelé, moteur détruit.
  Stopped { file: String },
}

impl Lifecycle {
  #[must_use]
  pub fn file(&self) -> &str {
    match self {
      Self::Started { file }
      | Self::Failed { file, .. }
      | Self::Error { file, .. }
      | Self::Disabled { file, .. }
      | Self::Stopped { file } => file,
    }
  }
}

/// Ce que les plugins envoient à l'application, sur un seul canal.
#[derive(Debug, Clone)]
pub enum Outgoing {
  Call(HostCall),
  Lifecycle(Lifecycle),
}

/// Destination des messages des plugins. Renvoie faux si l'application ne reçoit plus rien.
/// Appelée depuis les fils des plugins : elle ne doit pas bloquer.
pub type Sink = std::sync::Arc<dyn Fn(Outgoing) -> bool + Send + Sync>;

/// Résultat de `shuffle_pick` : une piste, `()` (aucune proposition), ou une erreur.
pub type PickResult = Result<Option<Value>, String>;
