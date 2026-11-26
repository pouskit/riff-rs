//! Jam : écoute partagée. Ce module porte le protocole, les sessions réseau et la synchronisation ; il ne
//! connaît ni le lecteur ni l'interface (voir `docs/specs/21-jam.md`).

pub mod app;
#[cfg(test)]
mod app_tests;
pub mod client;
pub mod code;
pub mod host;
pub mod iroh;
#[cfg(test)]
pub mod memory;
pub mod protocol;
pub mod rendezvous;
pub mod state;
pub mod sync;
#[cfg(test)]
mod tests;
pub mod transport;

use iced::futures::StreamExt;
use iced::futures::stream::{self, BoxStream};
use protocol::{Notice, Op, Participant, PlaybackState, QueueState, Refusal, RejectReason};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use transport::Transport;

pub use client::ClientConfig;
pub use host::HostConfig;

/// Messages de l'application pour le jam.
#[derive(Debug, Clone)]
pub enum JamMessage {
  /// Événement du réseau, avec la génération de la session qui l'a produit.
  Event(u64, Box<JamEvent>),
  /// La tâche réseau de cette génération est finie. La cause vient de la tâche elle-même : l'événement
  /// `Ended` du canal d'abonnement arrive après, quand la génération est déjà retirée.
  TaskEnded(u64, EndCause),
  Heartbeat,
  TogglePanel,
  Start,
  CodeInput(String),
  NameInput(String),
  Paste,
  Pasted(Option<String>),
  Join,
  CopyCode,
  /// Invité.
  Leave,
  /// Hôte.
  End,
}

/// Réglages d'URL qui ne s'analysent pas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InvalidUrls {
  pub relay: bool,
  pub rendezvous: bool,
}

/// Transport réel. Un relais invalide donne ceux par défaut. Une URL de rendez-vous invalide coupe le rendez-vous
/// (l'hôte garde le code long, l'invité n'a pas de code court) : elle ne retombe jamais sur le serveur de n0, qui
/// verrait l'IP et les heures de quelqu'un qui en a choisi un autre. `InvalidUrls` le signale.
#[must_use]
pub fn transport(relay: Option<&str>, rendezvous: Option<&str>) -> (Transport, InvalidUrls) {
  let parsed = relay.map(str::parse::<::iroh::RelayUrl>);
  let rendezvous = rendezvous.map(|url| {
    url::Url::parse(url)
      .ok()
      .filter(|url| matches!(url.scheme(), "http" | "https"))
  });
  let invalid = InvalidUrls {
    relay: matches!(parsed, Some(Err(_))),
    rendezvous: matches!(rendezvous, Some(None)),
  };
  let relay = parsed.and_then(Result::ok);
  let mut config = iroh::IrohConfig::new(relay, rendezvous.flatten());
  config.no_rendezvous = invalid.rendezvous;
  (Transport::Iroh(config), invalid)
}

/// Hôte compris.
pub const MAX_PARTICIPANTS: usize = 8;
/// Poignées de main simultanées.
pub const MAX_PENDING: usize = 4;
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_QUEUE: usize = 500;
pub const MAX_TRACKS_PER_OP: usize = 200;
pub const MAX_NAME_CHARS: usize = 32;
/// Titre, artiste, album, requête.
pub const MAX_TEXT_CHARS: usize = 200;
/// `url`, `thumbnail`, `cover`.
pub const MAX_URL_CHARS: usize = 1024;
pub const MAX_ID_CHARS: usize = 256;
/// Trames par seconde acceptées d'un invité.
pub const MAX_FRAMES_PER_SEC: u32 = 50;
/// Trames par seconde acceptées de l'hôte : au-dessus du débit maximal qu'il produit (états regroupés,
/// notices bornées, pongs), pour qu'un hôte qui s'emballe ne coupe pas les invités.
pub const MAX_HOST_FRAMES_PER_SEC: u32 = 200;
/// Fenêtre de regroupement de `Playback` : seul le dernier état est diffusé.
pub const PLAYBACK_WINDOW: Duration = Duration::from_millis(50);
/// Fenêtre de regroupement de `Queue`, dont les trames sont lourdes.
pub const QUEUE_WINDOW: Duration = Duration::from_millis(250);
/// Notices par seconde diffusées par l'hôte, au-delà jetées.
pub const MAX_NOTICES_PER_SEC: u32 = 5;
pub const PING_EVERY: Duration = Duration::from_secs(5);
/// Pings à l'admission, pour mesurer l'horloge vite.
pub const PING_BURST: [Duration; 3] = [
  Duration::ZERO,
  Duration::from_millis(250),
  Duration::from_millis(500),
];
pub const PEER_SILENCE: Duration = Duration::from_secs(20);
pub const RECONNECT_DELAYS: [Duration; 3] = [
  Duration::from_secs(1),
  Duration::from_secs(3),
  Duration::from_secs(6),
];
pub const HEARTBEAT: Duration = Duration::from_secs(5);
pub const SEEK_THRESHOLD: Duration = Duration::from_millis(1500);
pub const CORRECTION_COOLDOWN: Duration = Duration::from_secs(4);
/// Trames en attente par invité.
pub const GUEST_CHANNEL: usize = 64;
/// Délai accordé à la fermeture d'une écoute ou d'un lien.
pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
/// Attente avant de fermer un lien dont la salle est fausse.
pub const REFUSE_DELAY: Duration = Duration::from_secs(1);
/// Attente de l'hôte resté seul (après au moins un invité) avant de fermer le jam : couvre la reconnexion d'un
/// invité (1 + 3 + 6 s de délais plus l'établissement).
pub const ALONE_GRACE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
  Host,
  Guest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndCause {
  Stopped,
  HostEnded,
  /// Hôte : il a eu au moins un invité et n'en a plus depuis `ALONE_GRACE`.
  Alone,
  Lost,
  Refused(Refusal),
  InvalidTicket,
  /// Code court sans paquet de rendez-vous valable : faux, périmé, ou jam fini.
  CodeNotFound,
  /// Paquet de rendez-vous trop ancien : jam fini depuis plus de 3 min, ou horloge qui diverge de celle de l'hôte.
  CodeStale,
  Transport(String),
}

/// Réseau vers application.
#[derive(Debug, Clone, PartialEq)]
pub enum JamEvent {
  /// Hôte prêt.
  Hosting {
    ticket: String,
  },
  /// Hôte : relais non joint dans le délai.
  RelayUnreachable,
  /// Hôte : un tiers a publié sur la clé de rendez-vous, le code a changé.
  CodeChanged {
    ticket: String,
  },
  /// Invité admis, et réadmis après une reconnexion.
  Joined {
    you: u32,
    participants: Vec<Participant>,
    queue: QueueState,
    playback: PlaybackState,
    anchor: Instant,
  },
  Participants(Vec<Participant>),
  /// Hôte : commande d'un invité. `id` est celui de la commande, pour le refus.
  GuestOp {
    from: u32,
    id: u64,
    op: Op,
  },
  /// Hôte : échec de résolution chez un invité.
  GuestReport {
    from: u32,
    uid: u64,
  },
  /// Invité.
  Queue(QueueState),
  /// Invité. `anchor` : instant local qui correspond à `at_ms`.
  Playback {
    state: PlaybackState,
    anchor: Instant,
  },
  Notice(Notice),
  /// Invité.
  Rejected {
    id: u64,
    reason: RejectReason,
  },
  /// Invité : numéro de l'essai, de 1 à 3.
  Reconnecting {
    attempt: u8,
  },
  Ended(EndCause),
}

/// Application vers réseau.
#[derive(Debug, Clone, PartialEq)]
pub enum JamOut {
  /// Hôte : diffuse et garde en cache pour `welcome`.
  Queue(QueueState),
  /// Hôte : `at_ms` est calculé par la tâche à partir de `at`.
  Playback {
    state: Box<PlaybackState>,
    at: Instant,
  },
  /// Hôte : diffuse.
  Notice(Notice),
  /// Hôte.
  Reject {
    to: u32,
    id: u64,
    reason: RejectReason,
  },
  /// Invité.
  Command { id: u64, op: Op },
  /// Invité.
  Report { uid: u64 },
  /// Les deux : fin propre.
  Stop,
}

/// Boîte de réception des événements, reprise une seule fois par l'abonnement.
#[derive(Debug, Clone)]
pub struct JamInbox {
  pub generation: u64,
  receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<JamEvent>>>>,
}

impl JamInbox {
  fn new(generation: u64, receiver: mpsc::UnboundedReceiver<JamEvent>) -> Self {
    Self {
      generation,
      receiver: Arc::new(Mutex::new(Some(receiver))),
    }
  }

  /// Récepteur, une seule fois ; ensuite `None`.
  #[must_use]
  pub fn take(&self) -> Option<mpsc::UnboundedReceiver<JamEvent>> {
    self
      .receiver
      .lock()
      .ok()
      .and_then(|mut receiver| receiver.take())
  }

  /// Événements avec leur génération ; vide si la boîte a déjà été reprise.
  pub fn events(&self) -> BoxStream<'static, (u64, JamEvent)> {
    let generation = self.generation;
    match self.take() {
      Some(receiver) => stream::unfold(receiver, move |mut receiver| async move {
        receiver
          .recv()
          .await
          .map(|event| ((generation, event), receiver))
      })
      .boxed(),
      None => stream::empty().boxed(),
    }
  }
}

impl Hash for JamInbox {
  fn hash<H: Hasher>(&self, state: &mut H) {
    Arc::as_ptr(&self.receiver).hash(state);
  }
}

/// Abonnement aux événements du jam ; `wrap` fabrique le message de l'application.
pub fn subscription<M: Send + 'static>(
  inbox: JamInbox,
  wrap: fn(u64, JamEvent) -> M,
) -> iced::Subscription<M> {
  iced::Subscription::run_with((inbox, wrap), |(inbox, wrap)| {
    let wrap = *wrap;
    inbox
      .events()
      .map(move |(generation, event)| wrap(generation, event))
      .boxed()
  })
}

/// Lien de l'application vers une session.
#[derive(Debug, Clone)]
pub struct JamLink {
  pub commands: mpsc::UnboundedSender<JamOut>,
  pub inbox: JamInbox,
}

type Channels = (
  JamLink,
  mpsc::UnboundedSender<JamEvent>,
  mpsc::UnboundedReceiver<JamOut>,
);

fn channels(generation: u64) -> Channels {
  let (event_tx, event_rx) = mpsc::unbounded_channel();
  let (command_tx, command_rx) = mpsc::unbounded_channel();
  (
    JamLink {
      commands: command_tx,
      inbox: JamInbox::new(generation, event_rx),
    },
    event_tx,
    command_rx,
  )
}

/// Session de l'hôte : rien ne tourne avant que la tâche rendue soit exécutée.
pub fn start_host(
  transport: Transport,
  name: String,
  generation: u64,
) -> (JamLink, impl Future<Output = EndCause> + Send + 'static) {
  start_host_with(transport, name, generation, HostConfig::default())
}

pub fn start_host_with(
  transport: Transport,
  name: String,
  generation: u64,
  config: HostConfig,
) -> (JamLink, impl Future<Output = EndCause> + Send + 'static) {
  let (link, events, commands) = channels(generation);
  (link, host::run(transport, name, config, events, commands))
}

/// Session de l'invité : rien ne tourne avant que la tâche rendue soit exécutée.
pub fn start_guest(
  transport: Transport,
  ticket: String,
  name: String,
  generation: u64,
) -> (JamLink, impl Future<Output = EndCause> + Send + 'static) {
  start_guest_with(transport, ticket, name, generation, ClientConfig::default())
}

pub fn start_guest_with(
  transport: Transport,
  ticket: String,
  name: String,
  generation: u64,
  config: ClientConfig,
) -> (JamLink, impl Future<Output = EndCause> + Send + 'static) {
  let (link, events, commands) = channels(generation);
  (
    link,
    client::run(transport, ticket, name, config, events, commands),
  )
}

/// Tâche annulée à la destruction de sa poignée.
#[derive(Debug)]
pub(crate) struct AbortOnDrop(pub tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
  fn drop(&mut self) {
    self.0.abort();
  }
}

/// Millisecondes écoulées entre `epoch` et `now`.
pub(crate) fn millis_since(epoch: Instant, now: Instant) -> u64 {
  u64::try_from(now.saturating_duration_since(epoch).as_millis()).unwrap_or(u64::MAX)
}
