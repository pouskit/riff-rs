//! Transport `iroh` : QUIC chiffré, hôte authentifié par son `EndpointId`, code d'invitation `riffjam`.

use super::transport::{Link, ROOM_LEN, Room, TransportError};
use super::{AbortOnDrop, CLOSE_TIMEOUT, HANDSHAKE_TIMEOUT, MAX_PENDING};
use iroh::endpoint::{Connection, Incoming, VarInt, WeakConnectionHandle, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, TransportAddr};
use iroh_tickets::endpoint::EndpointTicket;
use iroh_tickets::{ParseError, Ticket};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

pub const ALPN: &[u8] = b"riff/jam/1";
/// Attente de la connexion au relais avant de publier le code d'invitation quand même.
pub const RELAY_WAIT: Duration = Duration::from_secs(10);
/// Délai d'une connexion sortante.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

const TICKET_VERSION: u8 = 1;
/// Version, puis salle.
const TICKET_HEADER: usize = 1 + ROOM_LEN;
const LOCAL_ADDRS_VAR: &str = "RIFF_JAM_LOCAL_ADDRS";

#[derive(Debug, Clone)]
pub struct IrohConfig {
  /// Relais choisi par l'utilisateur ; `None` : relais par défaut de n0.
  pub relay: Option<RelayUrl>,
  /// Garde les adresses locales dans le code d'invitation.
  pub local_addrs: bool,
  /// Tests : endpoint lié au bouclage, sans relais ni accès réseau.
  #[cfg(test)]
  pub(super) loopback: bool,
}

impl IrohConfig {
  /// `local_addrs` est lu de `RIFF_JAM_LOCAL_ADDRS=1`.
  #[must_use]
  pub fn new(relay: Option<RelayUrl>) -> Self {
    Self {
      relay,
      local_addrs: std::env::var(LOCAL_ADDRS_VAR).is_ok_and(|value| value == "1"),
      #[cfg(test)]
      loopback: false,
    }
  }

  #[cfg(test)]
  pub(super) fn loopback() -> Self {
    Self {
      relay: None,
      local_addrs: true,
      loopback: true,
    }
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
  relay_reached: bool,
  endpoint: Endpoint,
  links: mpsc::Receiver<Link>,
  connections: Connections,
  _accept: AbortOnDrop,
}

type Connections = Arc<Mutex<Vec<WeakConnectionHandle>>>;

impl IrohListener {
  pub(super) async fn bind(config: &IrohConfig, room: Room) -> Result<Self, TransportError> {
    let (endpoint, relay_expected) = config.bind().await?;
    let relay_reached = !relay_expected
      || tokio::time::timeout(RELAY_WAIT, endpoint.online())
        .await
        .is_ok();
    let addr = filter_addr(endpoint.addr(), config.local_addrs);
    let ticket = JamTicket { room, addr }.encode_string();
    let (sender, links) = mpsc::channel(MAX_PENDING);
    let connections = Connections::default();
    let accept = tokio::spawn(accept_loop(
      endpoint.clone(),
      sender,
      Arc::clone(&connections),
    ));
    Ok(Self {
      ticket,
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
      _accept,
      ..
    } = self;
    drop(_accept);
    let open = std::mem::take(&mut *connections.lock().unwrap_or_else(PoisonError::into_inner));
    for connection in open.iter().filter_map(WeakConnectionHandle::upgrade) {
      close_connection(&connection);
    }
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, endpoint.close()).await;
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
}

impl IrohDialer {
  pub(super) async fn bind(config: &IrohConfig) -> Result<Self, TransportError> {
    let (endpoint, _) = config.bind().await?;
    Ok(Self { endpoint })
  }

  pub(super) async fn connect(&self, addr: &EndpointAddr) -> Result<Link, TransportError> {
    let open = async {
      let connection = self.endpoint.connect(addr.clone(), ALPN).await.ok()?;
      let (send, recv) = connection.open_bi().await.ok()?;
      Some((connection, send, recv))
    };
    let Ok(Some((connection, send, recv))) = tokio::time::timeout(CONNECT_TIMEOUT, open).await
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
