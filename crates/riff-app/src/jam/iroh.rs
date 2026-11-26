//! Transport `iroh` : QUIC chiffré, hôte authentifié par son `EndpointId`. Le code d'invitation est un code court
//! (`K7Q-M2X`) résolu par un paquet de rendez-vous chiffré (voir `rendezvous.rs`) ; l'ancien code long `riffjam`
//! reste accepté, et sert quand le rendez-vous est indisponible.

use super::code::{Invite, JamCode};
use super::rendezvous::{self, LookupError, REPUBLISH_EVERY, RelayClient};
use super::transport::{Link, ROOM_LEN, Room, TransportError};
use super::{AbortOnDrop, CLOSE_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_PENDING};
use iroh::address_lookup::{N0_DNS_PKARR_RELAY_PROD, PkarrRelayClient};
use iroh::endpoint::{Connection, Incoming, VarInt, WeakConnectionHandle, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, TransportAddr};
use iroh_tickets::endpoint::EndpointTicket;
use iroh_tickets::{ParseError, Ticket};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use url::Url;

pub const ALPN: &[u8] = b"riff/jam/1";
/// Attente de la connexion au relais avant de publier le code d'invitation quand même.
pub const RELAY_WAIT: Duration = Duration::from_secs(10);
/// Délai d'une connexion sortante.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Délai pour tirer un code et le publier : au-delà, l'hôte garde l'ancien code long.
pub const CLAIM_TIMEOUT: Duration = Duration::from_secs(15);
/// Délai de lecture d'un code court par l'invité.
pub const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);
/// Délai accordé à la pierre tombale à la fermeture de l'écoute.
pub const TOMBSTONE_TIMEOUT: Duration = Duration::from_secs(1);

const TICKET_VERSION: u8 = 1;
/// Version, puis salle.
const TICKET_HEADER: usize = 1 + ROOM_LEN;
const LOCAL_ADDRS_VAR: &str = "RIFF_JAM_LOCAL_ADDRS";

#[derive(Debug, Clone)]
pub struct IrohConfig {
  /// Relais choisi par l'utilisateur ; `None` : relais par défaut de n0.
  pub relay: Option<RelayUrl>,
  /// Relais pkarr du rendez-vous, indépendant du relais de transport ; `None` : celui de n0.
  pub rendezvous: Option<Url>,
  /// Garde les adresses locales dans le code d'invitation.
  pub local_addrs: bool,
  /// L'URL de rendez-vous réglée est invalide : pas de rendez-vous du tout, jamais celui de n0 à sa place.
  pub no_rendezvous: bool,
  /// Tests : endpoint lié au bouclage, sans relais ni accès réseau.
  #[cfg(test)]
  pub(super) loopback: bool,
  /// Tests : relais de rendez-vous simulé (sans lui, un endpoint de bouclage n'a pas de rendez-vous).
  #[cfg(test)]
  pub(super) mock: Option<rendezvous::mock::MockRelay>,
  /// Tests : période de republication du rendez-vous.
  #[cfg(test)]
  pub(super) republish: Duration,
  /// Tests : délai d'une connexion sortante.
  #[cfg(test)]
  pub(super) connect: Duration,
  /// Tests : nombre de connexions sortantes à faire échouer d'emblée.
  #[cfg(test)]
  pub(super) connect_failures: Arc<std::sync::atomic::AtomicUsize>,
}

impl IrohConfig {
  /// `local_addrs` est lu de `RIFF_JAM_LOCAL_ADDRS=1`.
  #[must_use]
  pub fn new(relay: Option<RelayUrl>, rendezvous: Option<Url>) -> Self {
    Self {
      relay,
      rendezvous,
      local_addrs: std::env::var(LOCAL_ADDRS_VAR).is_ok_and(|value| value == "1"),
      no_rendezvous: false,
      #[cfg(test)]
      loopback: false,
      #[cfg(test)]
      mock: None,
      #[cfg(test)]
      republish: REPUBLISH_EVERY,
      #[cfg(test)]
      connect: CONNECT_TIMEOUT,
      #[cfg(test)]
      connect_failures: Arc::default(),
    }
  }

  #[cfg(test)]
  pub(super) fn loopback() -> Self {
    Self {
      relay: None,
      rendezvous: None,
      local_addrs: true,
      no_rendezvous: false,
      loopback: true,
      mock: None,
      republish: REPUBLISH_EVERY,
      connect: CONNECT_TIMEOUT,
      connect_failures: Arc::default(),
    }
  }

  fn connect_timeout(&self) -> Duration {
    #[cfg(test)]
    return self.connect;
    #[cfg(not(test))]
    CONNECT_TIMEOUT
  }

  fn republish_every(&self) -> Duration {
    #[cfg(test)]
    return self.republish;
    #[cfg(not(test))]
    REPUBLISH_EVERY
  }

  /// Bouclage avec un relais de rendez-vous simulé : le code court est publié et résolu pour de vrai.
  #[cfg(test)]
  pub(super) fn loopback_with(mock: rendezvous::mock::MockRelay) -> Self {
    Self {
      mock: Some(mock),
      ..Self::loopback()
    }
  }

  /// Client du relais de rendez-vous, qui prête le résolveur DNS et la configuration TLS de l'endpoint.
  fn relay_client(&self, endpoint: &Endpoint) -> Option<RelayClient> {
    if self.no_rendezvous {
      return None;
    }
    #[cfg(test)]
    if self.loopback {
      return self.mock.clone().map(RelayClient::Mock);
    }
    let url = match self.rendezvous.clone() {
      Some(url) => url,
      None => N0_DNS_PKARR_RELAY_PROD.parse().ok()?,
    };
    let resolver = endpoint.dns_resolver().ok()?.clone();
    Some(RelayClient::Pkarr(PkarrRelayClient::new(
      url,
      endpoint.tls_config().clone(),
      resolver,
    )))
  }

  /// Endpoint prêt à accepter ; le booléen dit si un relais est attendu.
  async fn bind(&self) -> Result<(Endpoint, bool), TransportError> {
    #[cfg(test)]
    if self.loopback {
      use iroh::endpoint::{NetReportConfig, PortmapperConfig};
      let endpoint = Endpoint::builder(presets::Minimal)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .map_err(|_| bind_failed())?
        .portmapper_config(PortmapperConfig::Disabled)
        .net_report_config(NetReportConfig::minimal())
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .map_err(|_| bind_failed())?;
      return Ok((endpoint, false));
    }
    // Le portmapper d'iroh (UPnP, NAT-PMP, PCP) reste actif : il améliore la connexion directe. Documenté dans PRIVACY.md et SERVICES.md.
    let mut builder = Endpoint::builder(presets::N0)
      .clear_address_lookup()
      .alpns(vec![ALPN.to_vec()]);
    if let Some(relay) = self.relay.as_ref() {
      builder = builder.relay_mode(RelayMode::custom([relay.clone()]));
    }
    let endpoint = builder.bind().await.map_err(|_| bind_failed())?;
    Ok((endpoint, true))
  }
}

fn bind_failed() -> TransportError {
  TransportError::Other("endpoint bind failed".to_string())
}

/// Code d'invitation : version, salle, adresse de l'hôte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JamTicket {
  pub room: Room,
  pub addr: EndpointAddr,
}

impl Ticket for JamTicket {
  const KIND: &'static str = "riffjam";

  fn encode_bytes(&self) -> Vec<u8> {
    let mut bytes = vec![TICKET_VERSION];
    bytes.extend_from_slice(&self.room.0);
    bytes.extend(EndpointTicket::new(self.addr.clone()).encode_bytes());
    bytes
  }

  fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
    if bytes.len() < TICKET_HEADER || bytes[0] != TICKET_VERSION {
      return Err(ParseError::verification_failed("riffjam header"));
    }
    let mut room = [0u8; ROOM_LEN];
    room.copy_from_slice(&bytes[1..TICKET_HEADER]);
    let ticket = EndpointTicket::decode_bytes(&bytes[TICKET_HEADER..])?;
    Ok(Self {
      room: Room(room),
      addr: ticket.endpoint_addr().clone(),
    })
  }
}

/// Salle et adresse de l'hôte.
pub(super) fn parse_ticket(ticket: &str) -> Result<(Room, EndpointAddr), TransportError> {
  let JamTicket { room, addr } =
    JamTicket::decode_string(ticket.trim()).map_err(|_| TransportError::InvalidTicket)?;
  Ok((room, addr))
}

fn is_shared_v4(ip: Ipv4Addr) -> bool {
  let [a, b, ..] = ip.octets();
  a == 100 && b & 0xC0 == 0x40
}

fn is_global(ip: IpAddr) -> bool {
  match ip {
    IpAddr::V4(ip) => {
      !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || is_shared_v4(ip))
    }
    IpAddr::V6(ip) => !(is_local_v6(&ip)),
  }
}

fn is_local_v6(ip: &Ipv6Addr) -> bool {
  ip.is_unspecified() || ip.is_loopback() || ip.is_unicast_link_local() || ip.is_unique_local()
}

/// Garde les relais et les adresses IP globales, sauf si `keep_local`.
pub(super) fn filter_addr(addr: EndpointAddr, keep_local: bool) -> EndpointAddr {
  let kept = addr.addrs.into_iter().filter(|transport| match transport {
    TransportAddr::Relay(_) => true,
    TransportAddr::Ip(socket) => keep_local || is_global(socket.ip()),
    _ => false,
  });
  EndpointAddr::from_parts(addr.id, kept)
}

fn close_connection(connection: &Connection) {
  connection.close(VarInt::from_u32(0), b"bye");
}

/// Écoute de l'hôte : un endpoint et une tâche d'acceptation, rien d'autre.
#[derive(Debug)]
pub(super) struct IrohListener {
  ticket: String,
  codes: Option<watch::Receiver<String>>,
  publisher: Option<Publisher>,
  relay_reached: bool,
  endpoint: Endpoint,
  links: mpsc::Receiver<Link>,
  connections: Connections,
  _accept: AbortOnDrop,
}

type Connections = Arc<Mutex<Vec<WeakConnectionHandle>>>;

/// Tâche qui republie le paquet de rendez-vous ; l'ordre d'arrêt lui fait publier la pierre tombale.
#[derive(Debug)]
struct Publisher {
  stop: oneshot::Sender<()>,
  task: JoinHandle<()>,
}

impl Publisher {
  async fn stop(self) {
    let Self { stop, mut task } = self;
    let _ = stop.send(());
    if tokio::time::timeout(TOMBSTONE_TIMEOUT, &mut task)
      .await
      .is_err()
    {
      task.abort();
    }
  }
}

impl IrohListener {
  pub(super) async fn bind(config: &IrohConfig, room: Room) -> Result<Self, TransportError> {
    let (endpoint, relay_expected) = config.bind().await?;
    let relay_reached = !relay_expected
      || tokio::time::timeout(RELAY_WAIT, endpoint.online())
        .await
        .is_ok();
    let local_addrs = config.local_addrs;
    let source = {
      let endpoint = endpoint.clone();
      move || Invite {
        room,
        addr: filter_addr(endpoint.addr(), local_addrs),
      }
    };
    let (ticket, codes, publisher) = match publish_code(config, &endpoint, &source).await {
      Some((client, published)) => {
        let ticket = published.code.display();
        let (codes_tx, codes_rx) = watch::channel(ticket.clone());
        let (stop, stop_rx) = oneshot::channel();
        let task = tokio::spawn(rendezvous::publish_loop(
          client,
          source,
          published,
          config.republish_every(),
          codes_tx,
          stop_rx,
        ));
        (ticket, Some(codes_rx), Some(Publisher { stop, task }))
      }
      None => {
        let Invite { room, addr } = source();
        (JamTicket { room, addr }.encode_string(), None, None)
      }
    };
    let (sender, links) = mpsc::channel(MAX_PENDING);
    let connections = Connections::default();
    let accept = tokio::spawn(accept_loop(
      endpoint.clone(),
      sender,
      Arc::clone(&connections),
    ));
    Ok(Self {
      ticket,
      codes,
      publisher,
      relay_reached,
      endpoint,
      links,
      connections,
      _accept: AbortOnDrop(accept),
    })
  }

  pub(super) fn ticket(&self) -> &str {
    &self.ticket
  }

  pub(super) fn codes(&self) -> Option<watch::Receiver<String>> {
    self.codes.clone()
  }

  pub(super) const fn relay_reached(&self) -> bool {
    self.relay_reached
  }

  pub(super) async fn accept(&mut self) -> Option<Result<Link, TransportError>> {
    self.links.recv().await.map(Ok)
  }

  pub(super) async fn close(self) {
    let Self {
      endpoint,
      connections,
      publisher,
      _accept,
      ..
    } = self;
    drop(_accept);
    if let Some(publisher) = publisher {
      publisher.stop().await;
    }
    let open = std::mem::take(&mut *connections.lock().unwrap_or_else(PoisonError::into_inner));
    for connection in open.iter().filter_map(WeakConnectionHandle::upgrade) {
      close_connection(&connection);
    }
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, endpoint.close()).await;
  }
}

/// Tire un code libre et publie le paquet de rendez-vous. `None` : rendez-vous indisponible, l'hôte garde
/// l'ancien code long.
async fn publish_code(
  config: &IrohConfig,
  endpoint: &Endpoint,
  source: &(impl Fn() -> Invite + Send + Sync),
) -> Option<(Arc<RelayClient>, rendezvous::Published)> {
  let client = Arc::new(config.relay_client(endpoint)?);
  let invite = source();
  let claim = rendezvous::claim(&*client, &invite, || JamCode::random().ok());
  match tokio::time::timeout(CLAIM_TIMEOUT, claim).await {
    Ok(Ok(published)) => Some((client, published)),
    Ok(Err(err)) => {
      tracing::debug!(%err, "jam: rendezvous unavailable, using the long invite code");
      None
    }
    Err(_) => {
      tracing::debug!("jam: rendezvous timed out, using the long invite code");
      None
    }
  }
}

/// Accepte les connexions entrantes ; au plus `MAX_PENDING` poignées de main en cours.
async fn accept_loop(endpoint: Endpoint, links: mpsc::Sender<Link>, connections: Connections) {
  let mut pending = JoinSet::new();
  loop {
    tokio::select! {
      incoming = endpoint.accept() => {
        let Some(incoming) = incoming else { break };
        if pending.len() >= MAX_PENDING {
          incoming.refuse();
        } else {
          pending.spawn(admit(incoming, links.clone(), Arc::clone(&connections)));
        }
      },
      Some(_) = pending.join_next(), if !pending.is_empty() => {},
    }
  }
}

/// Poignée de main QUIC, puis premier flux ouvert par l'invité.
async fn admit(incoming: Incoming, links: mpsc::Sender<Link>, connections: Connections) {
  let handshake = async {
    let connection = incoming.await.ok()?;
    let (send, recv) = connection.accept_bi().await.ok()?;
    Some((connection, send, recv))
  };
  let Ok(Some((connection, send, recv))) = tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake).await
  else {
    tracing::debug!("jam: incoming handshake failed");
    return;
  };
  let peer = connection.remote_id().fmt_short().to_string();
  {
    let mut open = connections.lock().unwrap_or_else(PoisonError::into_inner);
    open.retain(|handle| handle.upgrade().is_some());
    open.push(connection.weak_handle());
  }
  let link = Link {
    reader: Box::new(recv),
    writer: Box::new(send),
    peer,
  };
  if links.send(link).await.is_err() {
    close_connection(&connection);
  }
}

/// Endpoint de l'invité, gardé entre deux tentatives.
#[derive(Debug)]
pub(super) struct IrohDialer {
  endpoint: Endpoint,
  rendezvous: Option<RelayClient>,
  connect_timeout: Duration,
  #[cfg(test)]
  connect_failures: Arc<std::sync::atomic::AtomicUsize>,
}

impl IrohDialer {
  pub(super) async fn bind(config: &IrohConfig) -> Result<Self, TransportError> {
    let (endpoint, _) = config.bind().await?;
    let rendezvous = config.relay_client(&endpoint);
    Ok(Self {
      endpoint,
      rendezvous,
      connect_timeout: config.connect_timeout(),
      #[cfg(test)]
      connect_failures: Arc::clone(&config.connect_failures),
    })
  }

  /// Salle et adresse de l'hôte d'un code court.
  pub(super) async fn resolve(
    &self,
    code: &JamCode,
  ) -> Result<(Room, EndpointAddr), TransportError> {
    let Some(client) = self.rendezvous.as_ref() else {
      return Err(TransportError::Unreachable);
    };
    let found = tokio::time::timeout(LOOKUP_TIMEOUT, rendezvous::lookup(client, code))
      .await
      .map_err(|_| TransportError::Unreachable)?;
    match found {
      Ok(Invite { room, addr }) => Ok((room, addr)),
      Err(LookupError::NotFound) => Err(TransportError::NotFound),
      Err(LookupError::Stale) => Err(TransportError::Stale),
      Err(LookupError::Unreachable) => Err(TransportError::Unreachable),
    }
  }

  pub(super) async fn connect(&self, addr: &EndpointAddr) -> Result<Link, TransportError> {
    #[cfg(test)]
    if self
      .connect_failures
      .try_update(
        std::sync::atomic::Ordering::SeqCst,
        std::sync::atomic::Ordering::SeqCst,
        |left| left.checked_sub(1),
      )
      .is_ok()
    {
      return Err(TransportError::Unreachable);
    }
    let open = async {
      let connection = self.endpoint.connect(addr.clone(), ALPN).await.ok()?;
      let (send, recv) = connection.open_bi().await.ok()?;
      Some((connection, send, recv))
    };
    let Ok(Some((connection, send, recv))) = tokio::time::timeout(self.connect_timeout, open).await
    else {
      return Err(TransportError::Unreachable);
    };
    Ok(Link {
      reader: Box::new(recv),
      writer: Box::new(send),
      peer: connection.remote_id().fmt_short().to_string(),
    })
  }

  pub(super) async fn close(self) {
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, self.endpoint.close()).await;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use iroh::SecretKey;
  use std::net::SocketAddr;

  fn sample_addr(ips: &[&str]) -> EndpointAddr {
    let id = SecretKey::from_bytes(&[7u8; 32]).public();
    let mut addr = EndpointAddr::from_parts(
      id,
      ips
        .iter()
        .map(|ip| TransportAddr::Ip(ip.parse::<SocketAddr>().expect("socket"))),
    );
    addr = addr.with_relay_url("https://relay.example./".parse().expect("relay"));
    addr
  }

  fn sample() -> JamTicket {
    JamTicket {
      room: Room([9; ROOM_LEN]),
      addr: sample_addr(&["203.0.113.5:4000"]),
    }
  }

  #[test]
  fn ticket_round_trips_with_prefix() {
    let ticket = sample();
    let text = ticket.encode_string();
    assert!(text.starts_with("riffjam"));
    assert_eq!(JamTicket::decode_string(&text).expect("decode"), ticket);
    let (room, addr) = parse_ticket(&format!("  {text}\n")).expect("parse");
    assert_eq!(room, ticket.room);
    assert_eq!(addr, ticket.addr);
  }

  #[test]
  fn ticket_rejects_malformed_input() {
    let ticket = sample();
    let bytes = ticket.encode_bytes();
    let encode = |bytes: &[u8]| {
      let mut text = "riffjam".to_string();
      data_encoding_lower(bytes, &mut text);
      text
    };
    let mut other_version = bytes.clone();
    other_version[0] = 2;
    let text = ticket.encode_string();
    let wrong_prefix = text.replacen("riffjam", "riffxxx", 1);
    let bad_base32 = format!("riffjam{}", "1!8");
    for bad in [
      encode(&other_version),
      wrong_prefix,
      bad_base32,
      encode(&bytes[..TICKET_HEADER - 1]),
      encode(&[]),
      String::new(),
    ] {
      assert!(
        matches!(parse_ticket(&bad), Err(TransportError::InvalidTicket)),
        "{bad}"
      );
    }
  }

  #[test]
  fn the_rendezvous_url_is_read_and_a_bad_one_is_flagged() {
    use super::super::InvalidUrls;
    use super::super::transport::Transport;
    let none = InvalidUrls::default();
    let rendezvous = |relay: Option<&str>, url: Option<&str>| {
      let (transport, invalid) = super::super::transport(relay, url);
      let Transport::Iroh(config) = transport else {
        panic!("iroh transport expected");
      };
      (
        config.rendezvous.map(|url| url.to_string()),
        config.no_rendezvous,
        invalid,
      )
    };
    assert_eq!(rendezvous(None, None), (None, false, none));
    assert_eq!(
      rendezvous(None, Some("https://pkarr.example/pkarr")),
      (Some("https://pkarr.example/pkarr".to_string()), false, none)
    );
    assert_eq!(
      rendezvous(None, Some("http://192.168.1.2:8080/pkarr")),
      (
        Some("http://192.168.1.2:8080/pkarr".to_string()),
        false,
        none
      )
    );
    // Une URL de rendez-vous invalide coupe le rendez-vous au lieu de retomber sur le serveur de n0.
    let bad_rendezvous = InvalidUrls {
      relay: false,
      rendezvous: true,
    };
    for bad in [
      "not a url",
      "pkarr.example",
      "ftp://pkarr.example/",
      "file:///tmp/x",
      "",
    ] {
      assert_eq!(
        rendezvous(None, Some(bad)),
        (None, true, bad_rendezvous),
        "{bad}"
      );
    }
    // Un relais de transport invalide ne touche pas au rendez-vous, et inversement.
    assert_eq!(
      rendezvous(Some("%%"), Some("https://pkarr.example/")),
      (
        Some("https://pkarr.example/".to_string()),
        false,
        InvalidUrls {
          relay: true,
          rendezvous: false
        }
      )
    );
    assert_eq!(
      rendezvous(Some("%%"), None),
      (
        None,
        false,
        InvalidUrls {
          relay: true,
          rendezvous: false
        }
      )
    );
  }

  #[tokio::test]
  async fn an_invalid_rendezvous_url_never_falls_back_to_the_default_server() {
    let (endpoint, _) = IrohConfig::loopback().bind().await.expect("endpoint");
    let real = |no_rendezvous: bool| IrohConfig {
      loopback: false,
      no_rendezvous,
      ..IrohConfig::loopback()
    };
    assert!(real(false).relay_client(&endpoint).is_some());
    assert!(real(true).relay_client(&endpoint).is_none());
  }

  #[test]
  fn the_input_is_told_apart_as_a_short_code_or_an_old_ticket() {
    use super::super::transport::{Lookup, Transport};
    let transport = Transport::Iroh(IrohConfig::loopback());
    let ticket = sample().encode_string();
    for typed in [
      ticket.clone(),
      format!("  {}\n", ticket.to_ascii_uppercase()),
    ] {
      match transport.classify(&typed) {
        Ok(Lookup::Direct(target)) => assert_eq!(target.room, sample().room),
        other => panic!("{typed}: {other:?}"),
      }
    }
    for typed in ["K7Q-M2X", " k7q m2x ", "K7QM2X"] {
      match transport.classify(typed) {
        Ok(Lookup::Code(code)) => assert_eq!(code.display(), "K7Q-M2X"),
        other => panic!("{typed}: {other:?}"),
      }
    }
    for bad in [
      "",
      "K7Q-M2",
      "K7Q-M2XX",
      "I7Q-M2X",
      "0O1-IIX",
      "riffjam",
      "riffjam123",
      "hello world",
    ] {
      assert!(
        matches!(transport.classify(bad), Err(TransportError::InvalidTicket)),
        "{bad}"
      );
    }
  }

  /// Base32 sans remplissage, minuscule (même forme que `Ticket::encode_string`).
  fn data_encoding_lower(bytes: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in bytes {
      buffer = (buffer << 8) | u32::from(*byte);
      bits += 8;
      while bits >= 5 {
        bits -= 5;
        out.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
      }
    }
    if bits > 0 {
      out.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }
  }

  #[test]
  fn filter_drops_non_global_addresses() {
    let all = [
      "10.0.0.1:1",
      "172.16.5.4:2",
      "192.168.1.9:3",
      "127.0.0.1:4",
      "169.254.1.1:5",
      "100.64.0.1:6",
      "100.127.255.254:7",
      "0.0.0.0:8",
      "[fe80::1]:9",
      "[fc00::1]:10",
      "[fd12::1]:11",
      "[::1]:12",
      "[::]:13",
      "8.8.8.8:14",
      "100.128.0.1:15",
      "[2606:4700::1]:16",
    ];
    let addr = sample_addr(&all);
    let kept = filter_addr(addr.clone(), false);
    let ips: Vec<String> = kept.ip_addrs().map(ToString::to_string).collect();
    assert_eq!(ips.len(), 3, "{ips:?}");
    for expected in ["8.8.8.8:14", "100.128.0.1:15", "[2606:4700::1]:16"] {
      assert!(ips.iter().any(|ip| ip == expected), "{expected}");
    }
    assert_eq!(kept.relay_urls().count(), 1);
    assert_eq!(kept.id, addr.id);
    assert_eq!(filter_addr(addr.clone(), true), addr);
  }

  #[tokio::test]
  async fn loopback_endpoints_exchange_a_stream() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let config = IrohConfig::loopback();
    let room = Room([3; ROOM_LEN]);
    let mut listener = IrohListener::bind(&config, room).await.expect("listen");
    assert!(listener.relay_reached());
    let (parsed_room, addr) = parse_ticket(listener.ticket()).expect("ticket");
    assert_eq!(parsed_room, room);
    let dialer = IrohDialer::bind(&config).await.expect("dialer");
    let mut guest = dialer.connect(&addr).await.expect("connect");
    guest.writer.write_all(b"hello").await.expect("write");
    let mut host = tokio::time::timeout(Duration::from_secs(5), listener.accept())
      .await
      .expect("accept in time")
      .expect("open")
      .expect("link");
    let mut buffer = [0u8; 5];
    host.reader.read_exact(&mut buffer).await.expect("read");
    assert_eq!(&buffer, b"hello");
    host.writer.write_all(b"welcome").await.expect("write");
    let mut reply = [0u8; 7];
    guest.reader.read_exact(&mut reply).await.expect("read");
    assert_eq!(&reply, b"welcome");
    drop((guest, host));
    dialer.close().await;
    listener.close().await;
  }

  #[tokio::test]
  async fn a_short_code_is_published_resolved_and_withdrawn_on_close() {
    use super::super::rendezvous::mock::MockRelay;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let relay = MockRelay::default();
    let config = IrohConfig::loopback_with(relay.clone());
    let room = Room([4; ROOM_LEN]);
    let mut listener = IrohListener::bind(&config, room).await.expect("listen");
    let code = JamCode::parse(listener.ticket()).expect("short code");
    assert_eq!(listener.ticket(), code.display());
    assert_eq!(listener.ticket().len(), 7);
    assert!(listener.codes().is_some());
    let dialer = IrohDialer::bind(&config).await.expect("dialer");
    let (found_room, addr) = dialer.resolve(&code).await.expect("resolve");
    assert_eq!(found_room, room);
    let mut guest = dialer.connect(&addr).await.expect("connect");
    guest.writer.write_all(b"hi").await.expect("write");
    let mut host = tokio::time::timeout(Duration::from_secs(5), listener.accept())
      .await
      .expect("accept in time")
      .expect("open")
      .expect("link");
    let mut buffer = [0u8; 2];
    host.reader.read_exact(&mut buffer).await.expect("read");
    assert_eq!(&buffer, b"hi");
    drop((guest, host));
    listener.close().await;
    assert!(matches!(
      dialer.resolve(&code).await,
      Err(TransportError::NotFound)
    ));
    dialer.close().await;
  }

  /// Vrai réseau : relais pkarr de n0 (`dns.iroh.link`) et relais iroh. Lancer avec `--ignored`.
  #[tokio::test]
  #[ignore = "réseau"]
  async fn the_n0_rendezvous_resolves_a_real_short_code() {
    let config = IrohConfig::new(None, None);
    let room = Room([6; ROOM_LEN]);
    let mut listener = IrohListener::bind(&config, room).await.expect("listen");
    let code = JamCode::parse(listener.ticket()).expect("short code, not the long ticket");
    let dialer = IrohDialer::bind(&config).await.expect("dialer");
    let (found_room, addr) = dialer.resolve(&code).await.expect("resolve");
    assert_eq!(found_room, room);
    let _guest = dialer.connect(&addr).await.expect("connect");
    let accepted = tokio::time::timeout(Duration::from_secs(10), listener.accept())
      .await
      .expect("accept in time");
    assert!(accepted.is_some());
    listener.close().await;
    assert!(matches!(
      dialer.resolve(&code).await,
      Err(TransportError::NotFound)
    ));
    dialer.close().await;
  }

  #[tokio::test]
  async fn an_unknown_code_is_not_found_and_a_dead_rendezvous_is_unreachable() {
    use super::super::rendezvous::mock::MockRelay;
    let relay = MockRelay::default();
    let config = IrohConfig::loopback_with(relay.clone());
    let dialer = IrohDialer::bind(&config).await.expect("dialer");
    let code = JamCode::parse("K7QM2X").expect("code");
    assert!(matches!(
      dialer.resolve(&code).await,
      Err(TransportError::NotFound)
    ));
    relay.set_down(true);
    assert!(matches!(
      dialer.resolve(&code).await,
      Err(TransportError::Unreachable)
    ));
    dialer.close().await;
    // Sans relais de rendez-vous (bouclage seul) : aucun code court ne se résout.
    let bare = IrohDialer::bind(&IrohConfig::loopback())
      .await
      .expect("dialer");
    assert!(matches!(
      bare.resolve(&code).await,
      Err(TransportError::Unreachable)
    ));
    bare.close().await;
  }

  #[tokio::test]
  async fn the_host_keeps_the_long_code_when_the_rendezvous_is_down() {
    use super::super::rendezvous::mock::MockRelay;
    let relay = MockRelay::default();
    relay.set_down(true);
    let config = IrohConfig::loopback_with(relay);
    let listener = IrohListener::bind(&config, Room([5; ROOM_LEN]))
      .await
      .expect("listen");
    assert!(listener.ticket().starts_with("riffjam"));
    assert!(listener.codes().is_none());
    let (room, _) = parse_ticket(listener.ticket()).expect("old ticket");
    assert_eq!(room, Room([5; ROOM_LEN]));
    listener.close().await;
  }

  async fn next_event(
    events: &mut mpsc::UnboundedReceiver<super::super::JamEvent>,
  ) -> super::super::JamEvent {
    tokio::time::timeout(Duration::from_secs(10), events.recv())
      .await
      .expect("event in time")
      .expect("channel open")
  }

  #[tokio::test]
  async fn a_session_joins_with_a_short_code_typed_loosely() {
    use super::super::rendezvous::mock::MockRelay;
    use super::super::transport::Transport;
    use super::super::{EndCause, JamEvent, JamOut, start_guest, start_host};
    let relay = MockRelay::default();
    let transport = Transport::Iroh(IrohConfig::loopback_with(relay.clone()));
    let (host, host_task) = start_host(transport.clone(), "Host".to_string(), 1);
    let mut host_events = host.inbox.take().expect("inbox");
    let host_task = tokio::spawn(host_task);
    let ticket = loop {
      if let JamEvent::Hosting { ticket } = next_event(&mut host_events).await {
        break ticket;
      }
    };
    assert_eq!(ticket.len(), 7);
    let typed = format!("  {} \n", ticket.to_lowercase().replace('-', " "));
    let (guest, guest_task) = start_guest(transport.clone(), typed, "Guest".to_string(), 1);
    let mut guest_events = guest.inbox.take().expect("inbox");
    let guest_task = tokio::spawn(guest_task);
    loop {
      if let JamEvent::Joined { participants, .. } = next_event(&mut guest_events).await {
        assert_eq!(participants.len(), 2);
        break;
      }
    }
    let _ = host.commands.send(JamOut::Stop);
    assert_eq!(host_task.await.expect("host task"), EndCause::Stopped);
    loop {
      if matches!(next_event(&mut guest_events).await, JamEvent::Ended(_)) {
        break;
      }
    }
    let _ = guest.commands.send(JamOut::Stop);
    guest_task.await.expect("guest task");
    // Le jam fini, le code ne mène plus nulle part.
    let (late, late_task) = start_guest(transport, ticket, "Late".to_string(), 2);
    let mut late_events = late.inbox.take().expect("inbox");
    assert_eq!(late_task.await, EndCause::CodeNotFound);
    assert!(matches!(
      next_event(&mut late_events).await,
      JamEvent::Ended(EndCause::CodeNotFound)
    ));
  }

  /// Après un essai de reconnexion raté, le code est relu ; un jam disparu donne `CodeNotFound` quand tous les
  /// essais ont échoué.
  #[tokio::test]
  async fn a_guest_rereads_the_code_after_a_failed_reconnection() {
    use super::super::rendezvous::mock::MockRelay;
    use super::super::transport::Transport;
    use super::super::{ClientConfig, EndCause, JamEvent, start_guest_with, start_host};
    let relay = MockRelay::default();
    let mut iroh = IrohConfig::loopback_with(relay);
    iroh.connect = Duration::from_millis(500);
    let transport = Transport::Iroh(iroh);
    let (host, host_task) = start_host(transport.clone(), "Host".to_string(), 1);
    let mut host_events = host.inbox.take().expect("inbox");
    let host_task = tokio::spawn(host_task);
    let ticket = loop {
      if let JamEvent::Hosting { ticket } = next_event(&mut host_events).await {
        break ticket;
      }
    };
    let config = ClientConfig {
      peer_silence: Duration::from_millis(500),
      reconnect_delays: [Duration::from_millis(10); 3],
      ..ClientConfig::default()
    };
    let (guest, guest_task) = start_guest_with(transport, ticket, "Guest".to_string(), 1, config);
    let mut guest_events = guest.inbox.take().expect("inbox");
    let guest_task = tokio::spawn(guest_task);
    loop {
      if matches!(next_event(&mut guest_events).await, JamEvent::Joined { .. }) {
        break;
      }
    }
    // L'hôte disparaît sans prévenir ; son écoute publie la pierre tombale en tombant.
    host_task.abort();
    drop(host);
    let cause = tokio::time::timeout(Duration::from_secs(60), guest_task)
      .await
      .expect("guest ends in time")
      .expect("guest task");
    assert_eq!(cause, EndCause::CodeNotFound);
  }

  /// Le code d'un hôte vivant cesse de se résoudre (pierre tombale d'un tiers) : la reconnexion tente quand même
  /// la cible déjà connue au lieu de finir par `CodeNotFound`.
  #[tokio::test]
  async fn a_guest_reconnects_to_a_live_host_whose_code_no_longer_resolves() {
    use super::super::code;
    use super::super::rendezvous::{build_packet, mock::MockRelay};
    use super::super::transport::Transport;
    use super::super::{ClientConfig, JamEvent, JamOut, start_guest_with, start_host};
    let relay = MockRelay::default();
    let iroh = IrohConfig::loopback_with(relay.clone());
    let failures = Arc::clone(&iroh.connect_failures);
    let transport = Transport::Iroh(iroh);
    let (host, host_task) = start_host(transport.clone(), "Host".to_string(), 1);
    let mut host_events = host.inbox.take().expect("inbox");
    let host_task = tokio::spawn(host_task);
    let ticket = loop {
      if let JamEvent::Hosting { ticket } = next_event(&mut host_events).await {
        break ticket;
      }
    };
    let config = ClientConfig {
      peer_silence: Duration::from_millis(800),
      ping_burst: [
        Duration::ZERO,
        Duration::from_millis(50),
        Duration::from_millis(100),
      ],
      ping_every: Duration::from_secs(3600),
      reconnect_delays: [Duration::from_millis(10); 3],
      ..ClientConfig::default()
    };
    let keys = code::derive(&JamCode::parse(&ticket).expect("code"))
      .await
      .expect("kdf");
    let (guest, guest_task) = start_guest_with(transport, ticket, "Guest".to_string(), 1, config);
    let mut guest_events = guest.inbox.take().expect("inbox");
    let guest_task = tokio::spawn(guest_task);
    loop {
      if matches!(next_event(&mut guest_events).await, JamEvent::Joined { .. }) {
        break;
      }
    }
    // Le code ne mène plus à rien, l'hôte est toujours là ; le premier essai de reconnexion échoue.
    relay.put(build_packet(&keys, None).expect("tombstone"));
    failures.store(1, std::sync::atomic::Ordering::SeqCst);
    loop {
      match next_event(&mut guest_events).await {
        JamEvent::Joined { .. } => break,
        JamEvent::Ended(cause) => panic!("the guest gave up: {cause:?}"),
        _ => {}
      }
    }
    let _ = host.commands.send(JamOut::Stop);
    let _ = guest.commands.send(JamOut::Stop);
    host_task.await.expect("host task");
    guest_task.await.expect("guest task");
  }

  #[tokio::test]
  async fn a_guest_with_a_wrong_or_malformed_code_ends_at_once() {
    use super::super::rendezvous::mock::MockRelay;
    use super::super::transport::Transport;
    use super::super::{EndCause, start_guest};
    let transport = Transport::Iroh(IrohConfig::loopback_with(MockRelay::default()));
    for (typed, expected) in [
      ("K7Q-M2X", EndCause::CodeNotFound),
      ("not a code", EndCause::InvalidTicket),
      ("", EndCause::InvalidTicket),
    ] {
      let (_guest, task) = start_guest(transport.clone(), typed.to_string(), "G".to_string(), 1);
      assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), task)
          .await
          .expect("ends in time"),
        expected,
        "{typed}"
      );
    }
  }

  #[tokio::test]
  async fn a_session_still_joins_with_the_old_long_ticket() {
    use super::super::rendezvous::mock::MockRelay;
    use super::super::transport::Transport;
    use super::super::{JamEvent, JamOut, start_guest, start_host};
    let relay = MockRelay::default();
    relay.set_down(true);
    let transport = Transport::Iroh(IrohConfig::loopback_with(relay));
    let (host, host_task) = start_host(transport.clone(), "Host".to_string(), 1);
    let mut host_events = host.inbox.take().expect("inbox");
    let host_task = tokio::spawn(host_task);
    let ticket = loop {
      if let JamEvent::Hosting { ticket } = next_event(&mut host_events).await {
        break ticket;
      }
    };
    assert!(ticket.starts_with("riffjam"));
    let (guest, guest_task) = start_guest(transport, ticket, "Guest".to_string(), 1);
    let mut guest_events = guest.inbox.take().expect("inbox");
    let guest_task = tokio::spawn(guest_task);
    loop {
      if matches!(next_event(&mut guest_events).await, JamEvent::Joined { .. }) {
        break;
      }
    }
    let _ = host.commands.send(JamOut::Stop);
    let _ = guest.commands.send(JamOut::Stop);
    host_task.await.expect("host task");
    guest_task.await.expect("guest task");
  }

  #[tokio::test]
  async fn the_host_is_told_when_its_code_is_taken_over() {
    use super::super::code::{self, Invite};
    use super::super::rendezvous::{build_packet, mock::MockRelay};
    use super::super::transport::Transport;
    use super::super::{JamEvent, JamOut, start_guest, start_host};
    let relay = MockRelay::default();
    let mut config = IrohConfig::loopback_with(relay.clone());
    config.republish = Duration::from_millis(100);
    let transport = Transport::Iroh(config);
    let (host, host_task) = start_host(transport.clone(), "Host".to_string(), 1);
    let mut host_events = host.inbox.take().expect("inbox");
    let host_task = tokio::spawn(host_task);
    let old = loop {
      if let JamEvent::Hosting { ticket } = next_event(&mut host_events).await {
        break ticket;
      }
    };
    // Un tiers qui connaît le code publie un paquet plus récent sur la clé de rendez-vous.
    let keys = code::derive(&JamCode::parse(&old).expect("code"))
      .await
      .expect("kdf");
    let foreign = Invite {
      room: Room([1; ROOM_LEN]),
      addr: EndpointAddr::new(SecretKey::from_bytes(&[3u8; 32]).public()),
    };
    relay.put(build_packet(&keys, Some(&foreign)).expect("foreign packet"));
    let fresh = loop {
      if let JamEvent::CodeChanged { ticket } = next_event(&mut host_events).await {
        break ticket;
      }
    };
    assert_ne!(fresh, old);
    assert_eq!(fresh.len(), 7);
    let (guest, guest_task) = start_guest(transport, fresh, "Guest".to_string(), 1);
    let mut guest_events = guest.inbox.take().expect("inbox");
    let guest_task = tokio::spawn(guest_task);
    loop {
      if matches!(next_event(&mut guest_events).await, JamEvent::Joined { .. }) {
        break;
      }
    }
    let _ = host.commands.send(JamOut::Stop);
    let _ = guest.commands.send(JamOut::Stop);
    host_task.await.expect("host task");
    guest_task.await.expect("guest task");
  }

  #[tokio::test]
  async fn loopback_session_reaches_welcome() {
    use super::super::transport::Transport;
    use super::super::{JamEvent, JamOut, start_guest, start_host};
    async fn next(events: &mut mpsc::UnboundedReceiver<JamEvent>) -> JamEvent {
      tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .expect("event in time")
        .expect("channel open")
    }
    let transport = Transport::Iroh(IrohConfig::loopback());
    let (host, host_task) = start_host(transport.clone(), "Host".to_string(), 1);
    let mut host_events = host.inbox.take().expect("inbox");
    let host_task = tokio::spawn(host_task);
    let ticket = loop {
      if let JamEvent::Hosting { ticket } = next(&mut host_events).await {
        break ticket;
      }
    };
    assert!(ticket.starts_with("riffjam"));
    let (guest, guest_task) = start_guest(transport, ticket, "Guest".to_string(), 1);
    let mut guest_events = guest.inbox.take().expect("inbox");
    let guest_task = tokio::spawn(guest_task);
    loop {
      if let JamEvent::Joined { participants, .. } = next(&mut guest_events).await {
        assert_eq!(participants.len(), 2);
        break;
      }
    }
    let _ = host.commands.send(JamOut::Stop);
    loop {
      if matches!(next(&mut guest_events).await, JamEvent::Ended(_)) {
        break;
      }
    }
    let _ = guest.commands.send(JamOut::Stop);
    host_task.await.expect("host task");
    guest_task.await.expect("guest task");
  }
}
