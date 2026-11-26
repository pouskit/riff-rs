//! Interface de transport du jam : un flux bidirectionnel, fiable et ordonné, par participant.
use super::code::{self, JamCode};
use std::fmt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::watch;

pub const ROOM_LEN: usize = 16;

/// Jeton de salle : 128 bits aléatoires. Jamais écrit dans un journal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Room(pub [u8; ROOM_LEN]);

impl fmt::Debug for Room {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("Room(..)")
  }
}

impl Room {
  /// # Errors
  /// Échec du générateur aléatoire du système.
  pub fn random() -> Result<Self, getrandom::Error> {
    let mut bytes = [0u8; ROOM_LEN];
    getrandom::fill(&mut bytes)?;
    Ok(Self(bytes))
  }

  /// 32 caractères hexadécimaux minuscules.
  #[must_use]
  pub fn to_hex(&self) -> String {
    use std::fmt::Write;
    self
      .0
      .iter()
      .fold(String::with_capacity(2 * ROOM_LEN), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
      })
  }

  /// Exactement 32 caractères hexadécimaux minuscules.
  #[must_use]
  pub fn from_hex(hex: &str) -> Option<Self> {
    if hex.len() != 2 * ROOM_LEN || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
      return None;
    }
    let nibble = |digit: u8| char::from(digit).to_digit(16);
    let digits = hex.as_bytes();
    let mut bytes = [0u8; ROOM_LEN];
    for (index, byte) in bytes.iter_mut().enumerate() {
      let high = nibble(digits[2 * index])?;
      let low = nibble(digits[2 * index + 1])?;
      *byte = u8::try_from((high << 4) | low).ok()?;
    }
    Some(Self(bytes))
  }

  /// Comparaison à temps constant.
  #[must_use]
  pub fn ct_eq(&self, other: &Self) -> bool {
    self
      .0
      .iter()
      .zip(&other.0)
      .fold(0u8, |acc, (a, b)| acc | (a ^ b))
      == 0
  }
}

/// Flux bidirectionnel vers un pair.
pub struct Link {
  pub reader: Box<dyn AsyncRead + Send + Unpin>,
  pub writer: Box<dyn AsyncWrite + Send + Unpin>,
  /// Étiquette de journal : identifiant court du pair, jamais une adresse IP.
  pub peer: String,
}

impl fmt::Debug for Link {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Link")
      .field("peer", &self.peer)
      .finish_non_exhaustive()
  }
}

/// Sécurité garantie par le canal. Un transport `Plain` exige une authentification HMAC avant tout usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
  Confidential,
  Plain,
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
  #[error("invalid ticket")]
  InvalidTicket,
  /// Code court sans paquet de rendez-vous valable : jam fini, code faux ou périmé.
  #[error("code not found")]
  NotFound,
  /// Paquet de rendez-vous trop ancien : jam fini, ou horloge de l'invité qui diverge de celle de l'hôte.
  #[error("code expired")]
  Stale,
  #[error("unreachable")]
  Unreachable,
  #[error("closed")]
  Closed,
  #[error("{0}")]
  Other(String),
}

#[derive(Debug, Clone)]
pub enum Transport {
  Iroh(super::iroh::IrohConfig),
  #[cfg(test)]
  Memory(super::memory::Hub),
}

#[derive(Debug)]
enum ListenerKind {
  Iroh(super::iroh::IrohListener),
  #[cfg(test)]
  Memory(super::memory::MemoryListener),
}

#[derive(Debug)]
pub struct Listener {
  ticket: String,
  kind: ListenerKind,
}

impl Listener {
  /// Code d'invitation copiable : code court (`K7Q-M2X`), ou ancien code long si le rendez-vous est indisponible.
  #[must_use]
  pub fn ticket(&self) -> &str {
    &self.ticket
  }

  /// Prochaine connexion entrante ; `None` quand l'écoute est fermée.
  pub async fn accept(&mut self) -> Option<Result<Link, TransportError>> {
    match self.kind {
      ListenerKind::Iroh(ref mut listener) => listener.accept().await,
      #[cfg(test)]
      ListenerKind::Memory(ref mut listener) => listener.accept().await,
    }
  }

  /// Faux si l'hôte n'a pas joint de relais dans le délai : le code d'invitation ne porte que des adresses directes.
  #[must_use]
  pub const fn relay_reached(&self) -> bool {
    match self.kind {
      ListenerKind::Iroh(ref listener) => listener.relay_reached(),
      #[cfg(test)]
      ListenerKind::Memory(_) => true,
    }
  }

  /// Suit les changements de code : l'hôte en change quand un tiers publie sur sa clé de rendez-vous.
  #[must_use]
  pub fn codes(&self) -> CodeWatch {
    match self.kind {
      ListenerKind::Iroh(ref listener) => CodeWatch(listener.codes()),
      #[cfg(test)]
      ListenerKind::Memory(_) => CodeWatch(None),
    }
  }

  /// Ferme l'écoute et toutes les connexions.
  pub async fn close(self) {
    match self.kind {
      ListenerKind::Iroh(listener) => listener.close().await,
      #[cfg(test)]
      ListenerKind::Memory(listener) => listener.close(),
    }
  }
}

/// Changements du code d'invitation d'un hôte.
#[derive(Debug)]
pub struct CodeWatch(Option<watch::Receiver<String>>);

impl CodeWatch {
  /// Prochain code ; n'aboutit jamais si le code ne change pas.
  pub async fn changed(&mut self) -> String {
    if let Some(receiver) = self.0.as_mut()
      && receiver.changed().await.is_ok()
    {
      return receiver.borrow_and_update().clone();
    }
    std::future::pending().await
  }
}

#[derive(Debug, Clone)]
enum Address {
  Iroh(iroh::EndpointAddr),
  #[cfg(test)]
  Memory(u64),
}

/// Destination d'un invité, lue dans un code d'invitation.
#[derive(Debug, Clone)]
pub struct Target {
  pub room: Room,
  address: Address,
}

/// Saisie d'un invité reconnue, avant toute résolution.
#[derive(Debug, Clone)]
pub enum Lookup {
  /// Ancien code d'invitation : l'adresse y est déjà.
  Direct(Target),
  /// Code court : l'adresse se lit sur le relais de rendez-vous.
  Code(JamCode),
}

#[derive(Debug)]
enum DialerKind {
  Iroh(super::iroh::IrohDialer),
  #[cfg(test)]
  Memory(super::memory::Hub),
}

/// Point de départ des connexions d'un invité, gardé entre deux tentatives.
#[derive(Debug)]
pub struct Dialer {
  kind: DialerKind,
}

impl Dialer {
  /// # Errors
  /// Pair injoignable, ou transport fermé.
  pub async fn connect(&self, target: &Target) -> Result<Link, TransportError> {
    match (&self.kind, &target.address) {
      (DialerKind::Iroh(dialer), Address::Iroh(addr)) => dialer.connect(addr).await,
      #[cfg(test)]
      (DialerKind::Memory(hub), Address::Memory(listener)) => hub.connect(*listener).await,
      #[cfg(test)]
      _ => Err(TransportError::InvalidTicket),
    }
  }

  /// Cible de la saisie ; un code court passe par le relais de rendez-vous.
  ///
  /// # Errors
  /// [`TransportError::NotFound`] : code inconnu ou jam fini. [`TransportError::Stale`] : paquet périmé.
  /// [`TransportError::Unreachable`] : relais
  /// de rendez-vous injoignable.
  pub async fn resolve(&self, lookup: &Lookup) -> Result<Target, TransportError> {
    match (lookup, &self.kind) {
      (Lookup::Direct(target), _) => Ok(target.clone()),
      (Lookup::Code(code), DialerKind::Iroh(dialer)) => {
        let (room, addr) = dialer.resolve(code).await?;
        Ok(Target {
          room,
          address: Address::Iroh(addr),
        })
      }
      #[cfg(test)]
      (Lookup::Code(_), DialerKind::Memory(_)) => Err(TransportError::InvalidTicket),
    }
  }

  pub async fn close(self) {
    match self.kind {
      DialerKind::Iroh(dialer) => dialer.close().await,
      #[cfg(test)]
      DialerKind::Memory(_) => {}
    }
  }
}

impl Transport {
  /// # Errors
  /// Le transport ne peut pas ouvrir l'écoute.
  pub async fn listen(&self, room: Room) -> Result<Listener, TransportError> {
    match *self {
      Self::Iroh(ref config) => {
        let listener = super::iroh::IrohListener::bind(config, room).await?;
        Ok(Listener {
          ticket: listener.ticket().to_string(),
          kind: ListenerKind::Iroh(listener),
        })
      }
      #[cfg(test)]
      Self::Memory(ref hub) => {
        let (ticket, listener) = hub.listen(room);
        Ok(Listener {
          ticket,
          kind: ListenerKind::Memory(listener),
        })
      }
    }
  }

  /// Reconnaît la saisie d'un invité : ancien code d'invitation ou code court (saisie tolérante).
  ///
  /// # Errors
  /// [`TransportError::InvalidTicket`] si la saisie n'est pas un code de ce transport.
  pub fn classify(&self, input: &str) -> Result<Lookup, TransportError> {
    match *self {
      Self::Iroh(_) => {
        if code::is_legacy(input) {
          self
            .parse(&input.trim().to_ascii_lowercase())
            .map(Lookup::Direct)
        } else {
          JamCode::parse(input)
            .map(Lookup::Code)
            .ok_or(TransportError::InvalidTicket)
        }
      }
      #[cfg(test)]
      Self::Memory(_) => self.parse(input).map(Lookup::Direct),
    }
  }

  /// Cible d'un code d'invitation qui porte déjà l'adresse (ancien code, ou transport en mémoire).
  ///
  /// # Errors
  /// [`TransportError::InvalidTicket`] si le code n'est pas celui de ce transport.
  pub fn parse(&self, ticket: &str) -> Result<Target, TransportError> {
    match *self {
      Self::Iroh(_) => {
        let (room, addr) = super::iroh::parse_ticket(ticket)?;
        Ok(Target {
          room,
          address: Address::Iroh(addr),
        })
      }
      #[cfg(test)]
      Self::Memory(_) => {
        let (listener, room) = super::memory::parse_ticket(ticket)?;
        Ok(Target {
          room,
          address: Address::Memory(listener),
        })
      }
    }
  }

  /// # Errors
  /// Le transport ne peut pas préparer les connexions sortantes.
  pub async fn dialer(&self) -> Result<Dialer, TransportError> {
    match *self {
      Self::Iroh(ref config) => Ok(Dialer {
        kind: DialerKind::Iroh(super::iroh::IrohDialer::bind(config).await?),
      }),
      #[cfg(test)]
      Self::Memory(ref hub) => Ok(Dialer {
        kind: DialerKind::Memory(hub.clone()),
      }),
    }
  }

  /// Sécurité garantie par le canal.
  #[must_use]
  pub const fn security(&self) -> Security {
    match *self {
      Self::Iroh(_) => Security::Confidential,
      #[cfg(test)]
      Self::Memory(_) => Security::Confidential,
    }
  }
}
