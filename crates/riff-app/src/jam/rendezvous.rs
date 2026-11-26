//! Rendez-vous du code court : un paquet pkarr signé par la clé dérivée du code, dont le contenu (adresse de
//! l'hôte) est chiffré avec l'autre clé dérivée. Il vit sur un relais pkarr (celui de n0 par défaut, réglable).
//!
//! Le relais voit l'adresse IP de chacun, les heures, la clé publique de rendez-vous et un texte chiffré. Jamais de
//! code, de clé ni de charge utile dans un journal.

use super::code::{self, CryptoError, Invite, JamCode, Keys};
use iroh::PublicKey;
use iroh::address_lookup::PkarrRelayClient;
use iroh_dns::pkarr::SignedPacket;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, watch};

/// Nom DNS des enregistrements TXT du paquet.
pub const TXT_NAME: &str = "_riffjam";
const TTL: u32 = 30;
/// Période de republication : le paquet reste frais et l'adresse de l'hôte à jour.
pub const REPUBLISH_EVERY: Duration = Duration::from_secs(60);
/// Âge au-delà duquel un invité ignore un paquet, et au-delà duquel un code n'est plus tenu pour pris.
pub const MAX_AGE: Duration = Duration::from_secs(180);
const CLAIM_ATTEMPTS: usize = 8;
/// Délai d'une lecture ou d'une écriture de la boucle de publication : le client HTTP d'iroh n'en pose aucun.
const TICK_TIMEOUT: Duration = Duration::from_secs(10);
/// Délai du tirage d'un nouveau code en cours de jam (jusqu'à 8 dérivations plus le réseau).
const ROTATE_TIMEOUT: Duration = Duration::from_secs(15);
/// Délai total des pierres tombales à l'arrêt, juste sous celui que l'écoute accorde à la tâche avant de l'avorter.
pub const TOMBSTONE_BUDGET: Duration = Duration::from_millis(800);
/// Un caractère d'ordre, puis 254 de données : 255 octets au plus par chaîne TXT.
const CHUNK_DATA: usize = 254;

/// Le relais n'a pas répondu comme prévu : réseau, serveur ou réponse invalide.
#[derive(Debug, thiserror::Error)]
#[error("rendezvous relay unreachable")]
pub struct RelayError;

/// Relais pkarr : lire et écrire le paquet d'une clé.
pub trait Relay: Send + Sync + 'static {
  /// `Ok(None)` : rien n'est publié pour cette clé.
  fn resolve(
    &self,
    key: PublicKey,
  ) -> impl Future<Output = Result<Option<SignedPacket>, RelayError>> + Send;

  fn publish(&self, packet: &SignedPacket) -> impl Future<Output = Result<(), RelayError>> + Send;
}

/// Relais réel, ou simulé dans les tests.
#[derive(Debug, Clone)]
pub enum RelayClient {
  Pkarr(PkarrRelayClient),
  #[cfg(test)]
  Mock(mock::MockRelay),
}

/// Statut HTTP d'une erreur de relais. Les erreurs d'iroh sont opaques et leur texte court ne le porte pas : seul
/// le `Debug` alterné le montre (`HttpRequest { status: 404, .. }`).
fn http_status(err: &(dyn std::error::Error + 'static)) -> Option<u16> {
  parse_http_status(&format!("{err:#?}"))
}

fn parse_http_status(text: &str) -> Option<u16> {
  let after = &text[text.find("HttpRequest")? + "HttpRequest".len()..];
  let digits = after.split_once("status:")?.1.trim_start();
  let end = digits
    .find(|c: char| !c.is_ascii_digit())
    .unwrap_or(digits.len());
  digits[..end].parse().ok()
}

impl Relay for RelayClient {
  async fn resolve(&self, key: PublicKey) -> Result<Option<SignedPacket>, RelayError> {
    match self {
      Self::Pkarr(client) => match client.resolve(key).await {
        Ok(packet) => Ok(Some(packet)),
        // Seul le 404 dit « rien de publié » ; 429, 5xx et le reste sont des pannes du relais.
        Err(err) if http_status(&err) == Some(404) => Ok(None),
        Err(_) => Err(RelayError),
      },
      #[cfg(test)]
      Self::Mock(mock) => mock.resolve(key).await,
    }
  }

  async fn publish(&self, packet: &SignedPacket) -> Result<(), RelayError> {
    match self {
      Self::Pkarr(client) => client.publish(packet).await.map_err(|_| RelayError),
      #[cfg(test)]
      Self::Mock(mock) => mock.publish(packet).await,
    }
  }
}

/// Microsecondes depuis l'époque Unix.
#[must_use]
pub fn now_micros() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |elapsed| {
      u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
    })
}

fn age_micros(packet: &SignedPacket, now: u64) -> u64 {
  now.saturating_sub(packet.timestamp().as_micros())
}

#[derive(Debug, thiserror::Error)]
#[error("rendezvous packet could not be built")]
pub struct PacketError;

fn txt_strings(keys: &Keys, invite: &Invite) -> Result<Vec<String>, PacketError> {
  let sealed = keys.seal(&invite.encode()).map_err(|_| PacketError)?;
  let text = code::base32_encode(&sealed);
  text
    .as_bytes()
    .chunks(CHUNK_DATA)
    .enumerate()
    .map(|(index, chunk)| {
      let order = u8::try_from(index).ok().filter(|order| *order < 10);
      let order = char::from(b'0' + order.ok_or(PacketError)?);
      let data = std::str::from_utf8(chunk).map_err(|_| PacketError)?;
      Ok(format!("{order}{data}"))
    })
    .collect()
}

/// Paquet signé qui porte l'invitation chiffrée ; `None` : paquet vide (pierre tombale, plus récent que tout
/// paquet publié avant). Trop gros avec les adresses IP : seul le relais est gardé.
///
/// # Errors
/// Chiffrement ou encodage en échec.
pub fn build_packet(keys: &Keys, invite: Option<&Invite>) -> Result<SignedPacket, PacketError> {
  let build = |invite: Option<&Invite>| {
    let strings = invite.map(|invite| txt_strings(keys, invite)).transpose()?;
    SignedPacket::from_txt_strings(keys.secret(), TXT_NAME, strings.unwrap_or_default(), TTL)
      .map_err(|_| PacketError)
  };
  build(invite).or_else(|err| match invite {
    Some(invite) => build(Some(&invite.relay_only())),
    None => Err(err),
  })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
  /// Pierre tombale : le jam est fini.
  Gone,
  /// Paquet publié il y a plus de [`MAX_AGE`].
  Stale,
  /// Autre clé, texte altéré ou charge utile illisible.
  Invalid,
}

/// Lit un paquet : fraîcheur, déchiffrement, charge utile.
///
/// # Errors
/// [`OpenError`] : pierre tombale, paquet périmé ou invalide.
pub fn open(keys: &Keys, packet: &SignedPacket, now: u64) -> Result<Invite, OpenError> {
  if packet.public_key() != keys.public() {
    return Err(OpenError::Invalid);
  }
  let mut parts = packet.txt_records(TXT_NAME);
  if parts.is_empty() {
    return Err(OpenError::Gone);
  }
  if age_micros(packet, now) > u64::try_from(MAX_AGE.as_micros()).unwrap_or(u64::MAX) {
    return Err(OpenError::Stale);
  }
  parts.sort();
  let text: String = parts
    .iter()
    .map(|part| part.get(1..).unwrap_or_default())
    .collect();
  let sealed = code::base32_decode(&text).ok_or(OpenError::Invalid)?;
  let plain = keys.open(&sealed).ok_or(OpenError::Invalid)?;
  Invite::decode(&plain).ok_or(OpenError::Invalid)
}

/// Un paquet non vide et récent : le code est pris.
fn is_live(packet: &SignedPacket, now: u64) -> bool {
  !packet.txt_records(TXT_NAME).is_empty()
    && age_micros(packet, now) <= u64::try_from(MAX_AGE.as_micros()).unwrap_or(u64::MAX)
}

#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
  #[error("rendezvous relay unreachable")]
  Unreachable,
  #[error("no free code found")]
  Crowded,
  #[error("random generator failed")]
  Random,
  #[error(transparent)]
  Crypto(#[from] CryptoError),
  #[error(transparent)]
  Packet(#[from] PacketError),
}

/// Code publié : le code, ses clés et le dernier paquet accepté par le relais.
#[derive(Debug)]
pub struct Published {
  pub code: JamCode,
  keys: Keys,
  last: SignedPacket,
}

/// Tire un code libre et publie l'invitation. Un paquet récent et non vide sur la clé dérivée est une collision :
/// nouveau tirage. `draw` rend `None` si le générateur aléatoire échoue.
///
/// # Errors
/// Relais injoignable, aucun code libre, générateur aléatoire, chiffrement ou encodage en échec.
pub async fn claim<R: Relay>(
  relay: &R,
  invite: &Invite,
  mut draw: impl FnMut() -> Option<JamCode> + Send,
) -> Result<Published, ClaimError> {
  for _ in 0..CLAIM_ATTEMPTS {
    let code = draw().ok_or(ClaimError::Random)?;
    let keys = code::derive(&code).await?;
    match relay.resolve(keys.public()).await {
      Ok(Some(found)) if is_live(&found, now_micros()) => continue,
      Ok(_) => {}
      // Les erreurs du relais sont opaques : une lecture en échec ne bloque pas la publication, qui échoue
      // d'elle-même si le relais est hors service.
      Err(_) => tracing::debug!("jam: rendezvous collision check failed"),
    }
    let packet = build_packet(&keys, Some(invite))?;
    relay
      .publish(&packet)
      .await
      .map_err(|_| ClaimError::Unreachable)?;
    return Ok(Published {
      code,
      keys,
      last: packet,
    });
  }
  Err(ClaimError::Crowded)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LookupError {
  #[error("code not found")]
  NotFound,
  #[error("code expired")]
  Stale,
  #[error("rendezvous relay unreachable")]
  Unreachable,
}

/// Invité : dérive les clés du code, lit le paquet et le déchiffre.
///
/// # Errors
/// [`LookupError::NotFound`] : rien de publié, pierre tombale ou paquet illisible.
/// [`LookupError::Stale`] : paquet publié il y a plus de [`MAX_AGE`] à l'horloge de l'invité, jam fini ou horloge
/// qui diverge de celle de l'hôte de plus de 2 min environ.
/// [`LookupError::Unreachable`] : relais injoignable.
pub async fn lookup<R: Relay>(relay: &R, code: &JamCode) -> Result<Invite, LookupError> {
  let keys = code::derive(code)
    .await
    .map_err(|_| LookupError::Unreachable)?;
  match relay.resolve(keys.public()).await {
    Err(_) => Err(LookupError::Unreachable),
    Ok(None) => Err(LookupError::NotFound),
    Ok(Some(packet)) => open(&keys, &packet, now_micros()).map_err(|err| match err {
      OpenError::Stale => LookupError::Stale,
      OpenError::Gone | OpenError::Invalid => LookupError::NotFound,
    }),
  }
}

/// Borne une opération du relais.
async fn bounded<T>(
  limit: Duration,
  op: impl Future<Output = Result<T, RelayError>>,
) -> Result<T, RelayError> {
  tokio::time::timeout(limit, op)
    .await
    .unwrap_or(Err(RelayError))
}

/// Publie un paquet vide : le code ne mène plus à rien.
async fn tombstone<R: Relay>(relay: &R, published: &Published) -> Result<(), RelayError> {
  match build_packet(&published.keys, None) {
    Ok(packet) => bounded(TICK_TIMEOUT, relay.publish(&packet)).await,
    // Rien à publier : inutile de réessayer.
    Err(_) => Ok(()),
  }
}

/// Publie la pierre tombale des codes abandonnés, le plus récent d'abord ; un échec garde le reste pour plus tard.
async fn bury<R: Relay>(relay: &R, retired: &mut Vec<Published>) {
  while let Some(old) = retired.last() {
    if tombstone(relay, old).await.is_err() {
      tracing::debug!("jam: tombstone not published yet");
      return;
    }
    retired.pop();
  }
}

/// Une période : détecte un paquet étranger plus récent (code compromis : nouveau code), sinon republie. Chaque
/// requête est bornée ; l'état n'est changé qu'entre deux attentes, pour que l'arrêt puisse l'interrompre.
async fn tick<R: Relay>(
  relay: &R,
  source: &(impl Fn() -> Invite + Send + Sync),
  published: &mut Published,
  retired: &mut Vec<Published>,
  codes: &watch::Sender<String>,
) {
  bury(relay, retired).await;
  match bounded(TICK_TIMEOUT, relay.resolve(published.keys.public())).await {
    Ok(Some(found)) if found != published.last && found.more_recent_than(&published.last) => {
      tracing::warn!("jam: another party published on the rendezvous key, picking a new code");
      let claimed = tokio::time::timeout(
        ROTATE_TIMEOUT,
        claim(relay, &source(), || JamCode::random().ok()),
      )
      .await;
      match claimed {
        Ok(Ok(new)) => {
          retired.push(std::mem::replace(published, new));
          codes.send_replace(published.code.display());
          bury(relay, retired).await;
        }
        Ok(Err(err)) => tracing::debug!(%err, "jam: new code not published yet"),
        Err(_) => tracing::debug!("jam: new code timed out"),
      }
      return;
    }
    Ok(_) => {}
    Err(_) => tracing::debug!("jam: rendezvous check failed"),
  }
  let Ok(packet) = build_packet(&published.keys, Some(&source())) else {
    return;
  };
  match bounded(TICK_TIMEOUT, relay.publish(&packet)).await {
    Ok(()) => published.last = packet,
    Err(_) => tracing::debug!("jam: rendezvous republish failed"),
  }
}

/// Republie toutes les `every` ; à l'arrêt (ordre reçu ou émetteur détruit), interrompt la période en cours et
/// publie la pierre tombale, bornée par [`TOMBSTONE_BUDGET`]. `codes` reçoit le nouveau code quand il change.
pub async fn publish_loop<R: Relay>(
  relay: Arc<R>,
  source: impl Fn() -> Invite + Send + Sync,
  mut published: Published,
  every: Duration,
  codes: watch::Sender<String>,
  mut stop: oneshot::Receiver<()>,
) {
  let mut retired = Vec::new();
  loop {
    tokio::select! {
      _ = &mut stop => break,
      () = tokio::time::sleep(every) => {}
    }
    tokio::select! {
      _ = &mut stop => break,
      () = tick(&*relay, &source, &mut published, &mut retired, &codes) => {}
    }
  }
  retired.push(published);
  let _ = tokio::time::timeout(TOMBSTONE_BUDGET, bury(&*relay, &mut retired)).await;
}

#[cfg(test)]
pub mod mock {
  use super::{Relay, RelayError};
  use iroh::PublicKey;
  use iroh_dns::pkarr::SignedPacket;
  use std::collections::HashMap;
  use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

  #[derive(Debug, Default)]
  struct State {
    packets: HashMap<PublicKey, SignedPacket>,
    down: bool,
    publishes: usize,
  }

  /// Relais pkarr en mémoire : garde le paquet le plus récent de chaque clé.
  #[derive(Debug, Clone, Default)]
  pub struct MockRelay(Arc<Mutex<State>>);

  impl MockRelay {
    fn state(&self) -> MutexGuard<'_, State> {
      self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Relais injoignable (vrai) ou de nouveau joignable (faux).
    pub fn set_down(&self, down: bool) {
      self.state().down = down;
    }

    pub fn packet(&self, key: &PublicKey) -> Option<SignedPacket> {
      self.state().packets.get(key).cloned()
    }

    /// Publication faite par un tiers : passe par le même chemin que l'hôte.
    pub fn put(&self, packet: SignedPacket) {
      self.state().packets.insert(packet.public_key(), packet);
    }

    pub fn publishes(&self) -> usize {
      self.state().publishes
    }
  }

  impl Relay for MockRelay {
    async fn resolve(&self, key: PublicKey) -> Result<Option<SignedPacket>, RelayError> {
      let state = self.state();
      if state.down {
        return Err(RelayError);
      }
      Ok(state.packets.get(&key).cloned())
    }

    async fn publish(&self, packet: &SignedPacket) -> Result<(), RelayError> {
      let mut state = self.state();
      if state.down {
        return Err(RelayError);
      }
      state.publishes += 1;
      let key = packet.public_key();
      let newer = state
        .packets
        .get(&key)
        .is_none_or(|current| packet.more_recent_than(current));
      if newer {
        state.packets.insert(key, packet.clone());
      }
      Ok(())
    }
  }
}

#[cfg(test)]
mod tests {
  use super::mock::MockRelay;
  use super::*;
  use crate::jam::transport::{ROOM_LEN, Room};
  use iroh::{EndpointAddr, SecretKey, TransportAddr};
  use iroh_dns::pkarr::Timestamp;
  use std::collections::VecDeque;
  use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};

  fn code(text: &str) -> JamCode {
    JamCode::parse(text).expect("code")
  }

  fn invite(port: u16) -> Invite {
    let id = SecretKey::from_bytes(&[7u8; 32]).public();
    let addr = EndpointAddr::from_parts(
      id,
      [TransportAddr::Ip(
        format!("203.0.113.5:{port}").parse().expect("ip"),
      )],
    )
    .with_relay_url("https://relay.example./".parse().expect("relay"));
    Invite {
      room: Room([9; ROOM_LEN]),
      addr,
    }
  }

  async fn keys(text: &str) -> Keys {
    code::derive(&code(text)).await.expect("kdf")
  }

  fn draws(codes: &[&str]) -> impl FnMut() -> Option<JamCode> + Send + use<> {
    let mut queue: VecDeque<JamCode> = codes.iter().map(|text| code(text)).collect();
    move || queue.pop_front()
  }

  /// Même paquet, publié `age` plus tôt : la signature n'est plus valable, le relais simulé ne la vérifie pas.
  fn aged(packet: &SignedPacket, age: Duration) -> SignedPacket {
    let micros = packet
      .timestamp()
      .as_micros()
      .saturating_sub(u64::try_from(age.as_micros()).expect("micros"));
    SignedPacket::from_parts_unchecked(
      packet.public_key().as_bytes(),
      &packet.signature().to_bytes(),
      Timestamp::from_micros(micros),
      packet.encoded_packet(),
    )
    .expect("packet")
  }

  #[tokio::test]
  async fn a_packet_carries_the_invite_and_opens_with_the_same_code() {
    let keys = keys("K7QM2X").await;
    let packet = build_packet(&keys, Some(&invite(4000))).expect("packet");
    assert_eq!(packet.public_key(), keys.public());
    assert_eq!(
      open(&keys, &packet, now_micros()),
      Ok(invite(4000)),
      "aller-retour"
    );
  }

  #[tokio::test]
  async fn a_packet_stays_inside_the_pkarr_limits() {
    let keys = keys("K7QM2X").await;
    let mut big = invite(1);
    for port in 2..=10u16 {
      big.addr = big
        .addr
        .with_ip_addr(format!("[2606:4700::1]:{port}").parse().expect("ip"));
    }
    for invite in [invite(4000), big] {
      let packet = build_packet(&keys, Some(&invite)).expect("packet");
      assert!(packet.encoded_packet().len() <= 1000);
      let parts = packet.txt_records(TXT_NAME);
      assert!(!parts.is_empty());
      assert!(parts.iter().all(|part| part.len() <= 255));
      assert_eq!(
        open(&keys, &packet, now_micros()).map(|i| i.room),
        Ok(invite.room)
      );
    }
  }

  #[tokio::test]
  async fn a_packet_that_is_too_big_falls_back_to_the_relay_only() {
    let keys = keys("K7QM2X").await;
    let mut huge = invite(1);
    // Chaque adresse IPv6 occupe 19 octets : 6 au plus partent dans la charge utile, le paquet reste valide.
    for port in 2..=40u16 {
      huge.addr = huge
        .addr
        .with_ip_addr(format!("[2606:4700::{port:x}]:{port}").parse().expect("ip"));
    }
    let packet = build_packet(&keys, Some(&huge)).expect("packet");
    let opened = open(&keys, &packet, now_micros()).expect("open");
    assert_eq!(opened.addr.relay_urls().count(), 1);
    assert!(opened.addr.ip_addrs().count() <= 6);
  }

  #[tokio::test]
  async fn a_tombstone_is_empty_newer_and_reads_as_gone() {
    let keys = keys("K7QM2X").await;
    let live = build_packet(&keys, Some(&invite(4000))).expect("packet");
    let gone = build_packet(&keys, None).expect("tombstone");
    assert!(gone.txt_records(TXT_NAME).is_empty());
    assert!(gone.more_recent_than(&live));
    assert_eq!(open(&keys, &gone, now_micros()), Err(OpenError::Gone));
  }

  #[tokio::test]
  async fn a_stale_packet_is_rejected_after_three_minutes() {
    let keys = keys("K7QM2X").await;
    let packet = build_packet(&keys, Some(&invite(4000))).expect("packet");
    let now = now_micros();
    let three_minutes = 180_000_000;
    assert!(open(&keys, &packet, now + three_minutes - 60_000_000).is_ok());
    assert_eq!(
      open(&keys, &packet, now + three_minutes + 5_000_000),
      Err(OpenError::Stale)
    );
    // Une horloge locale en retard ne rend pas le paquet périmé.
    assert!(open(&keys, &packet, now.saturating_sub(10_000_000)).is_ok());
  }

  #[tokio::test]
  async fn a_packet_of_another_code_or_with_garbage_is_invalid() {
    let keys = keys("K7QM2X").await;
    let other = self::keys("K7QM2Y").await;
    let foreign = build_packet(&other, Some(&invite(4000))).expect("packet");
    assert_eq!(open(&keys, &foreign, now_micros()), Err(OpenError::Invalid));
    // Mêmes clés de rendez-vous, charge utile qui n'est pas du texte chiffré.
    let junk =
      SignedPacket::from_txt_strings(keys.secret(), TXT_NAME, ["0abc!"], TTL).expect("junk");
    assert_eq!(open(&keys, &junk, now_micros()), Err(OpenError::Invalid));
    let not_sealed =
      SignedPacket::from_txt_strings(keys.secret(), TXT_NAME, ["0abcdefgh"], TTL).expect("junk");
    assert_eq!(
      open(&keys, &not_sealed, now_micros()),
      Err(OpenError::Invalid)
    );
  }

  #[tokio::test]
  async fn the_records_are_read_in_order_whatever_their_arrival_order() {
    let keys = keys("K7QM2X").await;
    let sealed = keys.seal(&invite(4000).encode()).expect("seal");
    let text = code::base32_encode(&sealed);
    let (first, second) = text.split_at(text.len() / 2);
    let reversed = SignedPacket::from_txt_strings(
      keys.secret(),
      TXT_NAME,
      [format!("1{second}"), format!("0{first}")],
      TTL,
    )
    .expect("packet");
    assert_eq!(open(&keys, &reversed, now_micros()), Ok(invite(4000)));
  }

  #[tokio::test]
  async fn claim_publishes_and_lookup_finds_the_invite() {
    let relay = MockRelay::default();
    let published = claim(&relay, &invite(4000), draws(&["K7QM2X"]))
      .await
      .expect("claim");
    assert_eq!(published.code.display(), "K7Q-M2X");
    assert_eq!(relay.publishes(), 1);
    assert_eq!(lookup(&relay, &code("k7q-m2x")).await, Ok(invite(4000)));
  }

  #[tokio::test]
  async fn lookup_of_an_unknown_code_is_not_found() {
    let relay = MockRelay::default();
    assert_eq!(
      lookup(&relay, &code("K7QM2X")).await,
      Err(LookupError::NotFound)
    );
  }

  #[tokio::test]
  async fn lookup_reports_a_relay_that_is_down_apart_from_a_missing_code() {
    let relay = MockRelay::default();
    relay.set_down(true);
    assert_eq!(
      lookup(&relay, &code("K7QM2X")).await,
      Err(LookupError::Unreachable)
    );
    assert!(matches!(
      claim(&relay, &invite(1), draws(&["K7QM2X"])).await,
      Err(ClaimError::Unreachable)
    ));
  }

  #[tokio::test]
  async fn lookup_tells_a_stale_packet_from_a_tombstone() {
    let relay = MockRelay::default();
    let published = claim(&relay, &invite(4000), draws(&["K7QM2X"]))
      .await
      .expect("claim");
    let key = published.keys.public();
    relay.put(aged(&published.last, Duration::from_secs(240)));
    // Périmé : un paquet qui existe mais trop ancien se dit à part, l'horloge de l'invité peut être en cause.
    assert_eq!(
      lookup(&relay, &code("K7QM2X")).await,
      Err(LookupError::Stale)
    );
    relay.put(build_packet(&published.keys, None).expect("tombstone"));
    assert_eq!(
      lookup(&relay, &code("K7QM2X")).await,
      Err(LookupError::NotFound)
    );
    assert!(relay.packet(&key).is_some());
  }

  #[tokio::test]
  async fn a_live_packet_on_the_derived_key_is_a_collision_and_draws_again() {
    let relay = MockRelay::default();
    claim(&relay, &invite(1111), draws(&["K7QM2X"]))
      .await
      .expect("first");
    let second = claim(
      &relay,
      &invite(2222),
      draws(&["K7QM2X", "K7QM2X", "ABCDEF"]),
    )
    .await
    .expect("second");
    assert_eq!(second.code.display(), "ABC-DEF");
    assert_eq!(lookup(&relay, &code("K7QM2X")).await, Ok(invite(1111)));
    assert_eq!(lookup(&relay, &code("ABCDEF")).await, Ok(invite(2222)));
  }

  #[tokio::test]
  async fn a_stale_or_empty_packet_does_not_count_as_a_collision() {
    let relay = MockRelay::default();
    let first = claim(&relay, &invite(1111), draws(&["K7QM2X"]))
      .await
      .expect("first");
    relay.put(aged(&first.last, Duration::from_secs(600)));
    let reused = claim(&relay, &invite(2222), draws(&["K7QM2X"]))
      .await
      .expect("reuse after expiry");
    assert_eq!(reused.code.display(), "K7Q-M2X");
    relay.put(build_packet(&reused.keys, None).expect("tombstone"));
    claim(&relay, &invite(3333), draws(&["K7QM2X"]))
      .await
      .expect("reuse after a tombstone");
    assert_eq!(lookup(&relay, &code("K7QM2X")).await, Ok(invite(3333)));
  }

  #[tokio::test]
  async fn a_failed_collision_check_does_not_block_the_publication() {
    use std::sync::atomic::AtomicBool;
    /// Lecture toujours en échec, écriture qui passe.
    struct BlindRelay(MockRelay, AtomicBool);
    impl Relay for BlindRelay {
      async fn resolve(&self, _: PublicKey) -> Result<Option<SignedPacket>, RelayError> {
        self.1.store(true, Ordering::SeqCst);
        Err(RelayError)
      }
      async fn publish(&self, packet: &SignedPacket) -> Result<(), RelayError> {
        self.0.publish(packet).await
      }
    }
    let relay = BlindRelay(MockRelay::default(), AtomicBool::new(false));
    claim(&relay, &invite(4000), draws(&["K7QM2X"]))
      .await
      .expect("claim");
    assert!(relay.1.load(Ordering::SeqCst));
    assert_eq!(lookup(&relay.0, &code("K7QM2X")).await, Ok(invite(4000)));
  }

  #[tokio::test]
  async fn claim_gives_up_when_every_draw_collides() {
    let relay = MockRelay::default();
    claim(&relay, &invite(1), draws(&["K7QM2X"]))
      .await
      .expect("first");
    let tries = AtomicUsize::new(0);
    let result = claim(&relay, &invite(2), || {
      tries.fetch_add(1, Ordering::SeqCst);
      Some(code("K7QM2X"))
    })
    .await;
    assert!(matches!(result, Err(ClaimError::Crowded)));
    assert_eq!(tries.load(Ordering::SeqCst), CLAIM_ATTEMPTS);
  }

  #[tokio::test]
  async fn claim_fails_when_the_generator_does() {
    let relay = MockRelay::default();
    let result = claim(&relay, &invite(1), || None).await;
    assert!(matches!(result, Err(ClaimError::Random)));
  }

  /// Boucle de publication de test : période courte, adresse qui change à chaque lecture.
  struct Loop {
    relay: MockRelay,
    codes: watch::Receiver<String>,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
    key: PublicKey,
  }

  async fn start_loop(every: Duration) -> Loop {
    let relay = MockRelay::default();
    let published = claim(&relay, &invite(4000), draws(&["K7QM2X"]))
      .await
      .expect("claim");
    let key = published.keys.public();
    let (codes_tx, codes) = watch::channel(published.code.display());
    let (stop, stop_rx) = oneshot::channel();
    let counter = AtomicU16::new(4000);
    let source = move || invite(counter.fetch_add(1, Ordering::SeqCst));
    let task = tokio::spawn(publish_loop(
      Arc::new(relay.clone()),
      source,
      published,
      every,
      codes_tx,
      stop_rx,
    ));
    Loop {
      relay,
      codes,
      stop,
      task,
      key,
    }
  }

  async fn wait_for(mut condition: impl FnMut() -> bool) {
    for _ in 0..200 {
      if condition() {
        return;
      }
      tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("condition not met in time");
  }

  /// Relais dont la lecture ou l'écriture peut se bloquer sans jamais répondre.
  #[derive(Clone, Default)]
  struct Gated {
    inner: MockRelay,
    hang_resolve: Arc<std::sync::atomic::AtomicBool>,
    hang_publish: Arc<std::sync::atomic::AtomicBool>,
  }

  impl Relay for Gated {
    async fn resolve(&self, key: PublicKey) -> Result<Option<SignedPacket>, RelayError> {
      if self.hang_resolve.load(Ordering::SeqCst) {
        std::future::pending::<()>().await;
      }
      self.inner.resolve(key).await
    }

    async fn publish(&self, packet: &SignedPacket) -> Result<(), RelayError> {
      if self.hang_publish.load(Ordering::SeqCst) {
        std::future::pending::<()>().await;
      }
      self.inner.publish(packet).await
    }
  }

  /// Boucle de publication sur un relais qui se bloque, avec une période de 20 ms.
  async fn gated_loop() -> (
    Gated,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
    PublicKey,
  ) {
    let relay = Gated::default();
    let published = claim(&relay, &invite(4000), draws(&["K7QM2X"]))
      .await
      .expect("claim");
    let key = published.keys.public();
    let (codes_tx, _codes) = watch::channel(published.code.display());
    let (stop, stop_rx) = oneshot::channel();
    let task = tokio::spawn(publish_loop(
      Arc::new(relay.clone()),
      || invite(4000),
      published,
      Duration::from_millis(20),
      codes_tx,
      stop_rx,
    ));
    (relay, stop, task, key)
  }

  #[tokio::test]
  async fn a_tick_stuck_on_the_relay_does_not_delay_the_tombstone() {
    let (relay, stop, task, key) = gated_loop().await;
    relay.hang_resolve.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(3), task)
      .await
      .expect("loop ends without waiting for the stuck tick")
      .expect("loop");
    let packet = relay.inner.packet(&key).expect("packet");
    assert!(packet.txt_records(TXT_NAME).is_empty(), "tombstone");
  }

  #[tokio::test]
  async fn a_relay_that_never_answers_cannot_hold_the_loop_after_the_stop() {
    let (relay, stop, task, _key) = gated_loop().await;
    relay.hang_resolve.store(true, Ordering::SeqCst);
    relay.hang_publish.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(3), task)
      .await
      .expect("the tombstone is given up on")
      .expect("loop");
  }

  #[tokio::test]
  async fn the_packet_is_republished_with_the_current_address() {
    let running = start_loop(Duration::from_millis(40)).await;
    wait_for(|| running.relay.publishes() >= 4).await;
    let invite = lookup(&running.relay, &code("K7QM2X"))
      .await
      .expect("found");
    let port = invite.addr.ip_addrs().next().expect("ip").port();
    assert!(port > 4000, "adresse relue à chaque période : {port}");
    let _ = running.stop.send(());
    running.task.await.expect("loop");
  }

  #[tokio::test]
  async fn stopping_the_loop_publishes_a_tombstone() {
    let running = start_loop(Duration::from_secs(3600)).await;
    assert!(lookup(&running.relay, &code("K7QM2X")).await.is_ok());
    let _ = running.stop.send(());
    running.task.await.expect("loop");
    assert_eq!(
      lookup(&running.relay, &code("K7QM2X")).await,
      Err(LookupError::NotFound)
    );
    let packet = running.relay.packet(&running.key).expect("packet");
    assert!(packet.txt_records(TXT_NAME).is_empty());
  }

  #[tokio::test]
  async fn dropping_the_stop_sender_also_ends_the_loop_with_a_tombstone() {
    let running = start_loop(Duration::from_secs(3600)).await;
    drop(running.stop);
    running.task.await.expect("loop");
    assert_eq!(
      lookup(&running.relay, &code("K7QM2X")).await,
      Err(LookupError::NotFound)
    );
  }

  #[tokio::test]
  async fn a_foreign_newer_packet_makes_the_host_pick_a_new_code() {
    let mut running = start_loop(Duration::from_millis(40)).await;
    let keys = keys("K7QM2X").await;
    running
      .relay
      .put(build_packet(&keys, Some(&invite(9999))).expect("foreign"));
    tokio::time::timeout(Duration::from_secs(10), running.codes.changed())
      .await
      .expect("code changed in time")
      .expect("sender alive");
    let fresh = running.codes.borrow().clone();
    assert_ne!(fresh, "K7Q-M2X");
    let found = lookup(&running.relay, &code(&fresh))
      .await
      .expect("new code");
    assert_ne!(found.addr.ip_addrs().next().expect("ip").port(), 9999);
    // L'ancien code ne mène plus à l'invité : la pierre tombale est plus récente que le paquet étranger.
    assert_eq!(
      lookup(&running.relay, &code("K7QM2X")).await,
      Err(LookupError::NotFound)
    );
    let _ = running.stop.send(());
    running.task.await.expect("loop");
    assert_eq!(
      lookup(&running.relay, &code(&fresh)).await,
      Err(LookupError::NotFound)
    );
  }

  #[tokio::test]
  async fn an_outage_keeps_the_loop_alive_and_it_recovers() {
    let running = start_loop(Duration::from_millis(30)).await;
    wait_for(|| running.relay.publishes() >= 2).await;
    running.relay.set_down(true);
    let before = running.relay.publishes();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(running.relay.publishes(), before);
    running.relay.set_down(false);
    wait_for(|| running.relay.publishes() > before).await;
    assert!(lookup(&running.relay, &code("K7QM2X")).await.is_ok());
    let _ = running.stop.send(());
    running.task.await.expect("loop");
  }

  /// Relais HTTP local qui répond toujours `status` ; le point d'accès prête sa TLS et son résolveur au client.
  async fn relay_answering(status: &'static str) -> (RelayClient, iroh::Endpoint) {
    use iroh::endpoint::{NetReportConfig, PortmapperConfig, presets};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let endpoint = iroh::Endpoint::builder(presets::Minimal)
      .clear_ip_transports()
      .bind_addr("127.0.0.1:0")
      .expect("addr")
      .portmapper_config(PortmapperConfig::Disabled)
      .net_report_config(NetReportConfig::minimal())
      .bind()
      .await
      .expect("endpoint");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
      .await
      .expect("listener");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
      while let Ok((mut socket, _)) = listener.accept().await {
        let mut request = [0u8; 1024];
        let _ = socket.read(&mut request).await;
        let reply = format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        let _ = socket.write_all(reply.as_bytes()).await;
      }
    });
    let client = PkarrRelayClient::new(
      format!("http://127.0.0.1:{port}/").parse().expect("url"),
      endpoint.tls_config().clone(),
      endpoint.dns_resolver().expect("resolver").clone(),
    );
    (RelayClient::Pkarr(client), endpoint)
  }

  #[tokio::test]
  async fn only_a_404_from_the_relay_means_nothing_is_published() {
    let key = SecretKey::from_bytes(&[7u8; 32]).public();
    let (relay, _endpoint) = relay_answering("404 Not Found").await;
    assert!(matches!(relay.resolve(key).await, Ok(None)));
    for status in [
      "429 Too Many Requests",
      "500 Internal Server Error",
      "502 Bad Gateway",
      "503 Service Unavailable",
    ] {
      let (relay, _endpoint) = relay_answering(status).await;
      assert!(relay.resolve(key).await.is_err(), "{status}");
    }
  }

  #[tokio::test]
  async fn a_throttled_or_failing_relay_reads_as_unreachable_not_as_a_missing_code() {
    for status in ["429 Too Many Requests", "503 Service Unavailable"] {
      let (relay, _endpoint) = relay_answering(status).await;
      assert_eq!(
        lookup(&relay, &code("K7QM2X")).await,
        Err(LookupError::Unreachable),
        "{status}"
      );
    }
    let (relay, _endpoint) = relay_answering("404 Not Found").await;
    assert_eq!(
      lookup(&relay, &code("K7QM2X")).await,
      Err(LookupError::NotFound)
    );
  }

  #[test]
  fn the_http_status_is_read_from_the_error_text() {
    assert_eq!(
      parse_http_status("Error { source: Stack(HttpRequest {\n status: 404,\n meta: Meta }) }"),
      Some(404)
    );
    assert_eq!(
      parse_http_status("Error { source: Stack(HttpRequest { status: 503, meta: Meta }) }"),
      Some(503)
    );
    assert_eq!(
      parse_http_status("Stack(HttpSend { source: connection refused })"),
      None
    );
    assert_eq!(parse_http_status("HttpRequest { status: oops }"), None);
  }
}
