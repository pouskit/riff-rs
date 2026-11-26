//! Code d'invitation court : 6 caractères, dérivation de clés (Argon2id) et chiffrement de la charge utile.
//!
//! Le code ne circule jamais : il sert seulement à dériver une clé de rendez-vous (Ed25519, qui signe le paquet
//! publié) et une clé AES-256-GCM (qui chiffre l'adresse de l'hôte). Jamais de code, de clé ni de charge utile
//! dans un journal.

use super::transport::{ROOM_LEN, Room};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use iroh::{EndpointAddr, PublicKey, RelayUrl, SecretKey, TransportAddr};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// 32 symboles, sans I, O, 0 ni 1 : 5 bits par caractère.
pub const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const CODE_LEN: usize = 6;
/// Préfixe de l'ancien code d'invitation, gardé en secours.
pub const LEGACY_PREFIX: &str = "riffjam";

const SALT: &[u8] = b"riff/jam/rdv/v1";
const AAD: &[u8] = b"riff/jam/v1";
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const KDF_OUT: usize = 2 * KEY_LEN;
/// Argon2id : 64 MiB, 3 passes, 1 voie (environ 200 ms en release, mesuré).
const PROD_MEMORY_KIB: u32 = 64 * 1024;
const PROD_PASSES: u32 = 3;
#[cfg(not(test))]
const MEMORY_KIB: u32 = PROD_MEMORY_KIB;
#[cfg(not(test))]
const PASSES: u32 = PROD_PASSES;
/// Tests : paramètres réduits, la dérivation reste déterministe.
#[cfg(test)]
const MEMORY_KIB: u32 = 32;
#[cfg(test)]
const PASSES: u32 = 1;

const INVITE_VERSION: u8 = 1;
const MAX_IPS: usize = 6;

/// Code de 6 caractères tiré dans [`ALPHABET`] : 30 bits.
#[derive(Clone, PartialEq, Eq)]
pub struct JamCode([u8; CODE_LEN]);

impl fmt::Debug for JamCode {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("JamCode(..)")
  }
}

impl JamCode {
  /// # Errors
  /// Échec du générateur aléatoire du système.
  pub fn random() -> Result<Self, getrandom::Error> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes)?;
    Ok(Self::from_bits(u32::from_be_bytes(bytes)))
  }

  /// Les 30 bits de poids faible, 5 par caractère : 32 = 2^5, aucun rejet.
  fn from_bits(bits: u32) -> Self {
    let mut symbols = [0u8; CODE_LEN];
    for (index, symbol) in symbols.iter_mut().enumerate() {
      let shift = 5 * (CODE_LEN - 1 - index);
      *symbol = ALPHABET[((bits >> shift) & 31) as usize];
    }
    Self(symbols)
  }

  /// Saisie tolérante : espaces autour, minuscules, tirets et espaces retirés. `None` si ce n'est pas un code.
  #[must_use]
  pub fn parse(input: &str) -> Option<Self> {
    let mut symbols = [0u8; CODE_LEN];
    let mut count = 0;
    for ch in input.trim().chars() {
      if ch == '-' || ch.is_whitespace() {
        continue;
      }
      let upper = u8::try_from(ch.to_ascii_uppercase()).ok()?;
      if !ALPHABET.contains(&upper) || count == CODE_LEN {
        return None;
      }
      symbols[count] = upper;
      count += 1;
    }
    (count == CODE_LEN).then_some(Self(symbols))
  }

  /// Forme normalisée : 6 majuscules et chiffres, sans tiret.
  #[must_use]
  pub fn normalized(&self) -> &str {
    std::str::from_utf8(&self.0).unwrap_or_default()
  }

  /// Forme affichée : `K7Q-M2X`.
  #[must_use]
  pub fn display(&self) -> String {
    let text = self.normalized();
    let (head, tail) = text.split_at(CODE_LEN / 2);
    format!("{head}-{tail}")
  }
}

/// `true` pour une saisie qui commence par `riffjam` (sans tenir compte de la casse) : ancien code d'invitation.
#[must_use]
pub fn is_legacy(input: &str) -> bool {
  input
    .trim_start()
    .get(..LEGACY_PREFIX.len())
    .is_some_and(|head| head.eq_ignore_ascii_case(LEGACY_PREFIX))
}

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
  #[error("key derivation failed")]
  Kdf,
  #[error("random generator failed")]
  Random,
  #[error("encryption failed")]
  Seal,
}

/// Clés dérivées du code.
pub struct Keys {
  rendezvous: SecretKey,
  cipher: [u8; KEY_LEN],
}

impl fmt::Debug for Keys {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("Keys(..)")
  }
}

impl Keys {
  /// Clé publique de rendez-vous : adresse du paquet sur le relais.
  #[must_use]
  pub fn public(&self) -> PublicKey {
    self.rendezvous.public()
  }

  pub(super) const fn secret(&self) -> &SecretKey {
    &self.rendezvous
  }

  /// Chiffre avec la clé AES du code ; nonce aléatoire devant le texte chiffré.
  ///
  /// # Errors
  /// Générateur aléatoire ou chiffrement en échec.
  pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| CryptoError::Random)?;
    let cipher = Aes256Gcm::new((&self.cipher).into());
    let sealed = cipher
      .encrypt(
        &Nonce::from(nonce),
        Payload {
          msg: plain,
          aad: AAD,
        },
      )
      .map_err(|_| CryptoError::Seal)?;
    let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
  }

  /// `None` si le texte est trop court, altéré, ou chiffré avec une autre clé.
  #[must_use]
  pub fn open(&self, sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < NONCE_LEN {
      return None;
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new((&self.cipher).into());
    cipher
      .decrypt(
        &Nonce::try_from(nonce).ok()?,
        Payload {
          msg: body,
          aad: AAD,
        },
      )
      .ok()
  }
}

/// Argon2id sur le code normalisé, sel fixe. Bloquant : environ 200 ms et 64 MiB, à lancer dans `spawn_blocking`.
fn derive_with(code: &JamCode, memory_kib: u32, passes: u32) -> Result<Keys, CryptoError> {
  let params = Params::new(memory_kib, passes, 1, Some(KDF_OUT)).map_err(|_| CryptoError::Kdf)?;
  let mut out = [0u8; KDF_OUT];
  Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
    .hash_password_into(code.normalized().as_bytes(), SALT, &mut out)
    .map_err(|_| CryptoError::Kdf)?;
  let mut rendezvous = [0u8; KEY_LEN];
  let mut cipher = [0u8; KEY_LEN];
  rendezvous.copy_from_slice(&out[..KEY_LEN]);
  cipher.copy_from_slice(&out[KEY_LEN..]);
  Ok(Keys {
    rendezvous: SecretKey::from_bytes(&rendezvous),
    cipher,
  })
}

/// Dérivation hors du fil asynchrone.
///
/// # Errors
/// Paramètres refusés par Argon2, ou tâche annulée.
pub async fn derive(code: &JamCode) -> Result<Keys, CryptoError> {
  let code = code.clone();
  tokio::task::spawn_blocking(move || derive_with(&code, MEMORY_KIB, PASSES))
    .await
    .map_err(|_| CryptoError::Kdf)?
}

/// Ce que le paquet de rendez-vous porte : la salle et l'adresse de l'hôte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
  pub room: Room,
  pub addr: EndpointAddr,
}

impl Invite {
  /// Le même sans adresses IP : plus petit, pour un paquet qui dépasse la limite.
  #[must_use]
  pub fn relay_only(&self) -> Self {
    let kept = self
      .addr
      .addrs
      .iter()
      .filter(|addr| matches!(addr, TransportAddr::Relay(_)))
      .cloned();
    Self {
      room: self.room,
      addr: EndpointAddr::from_parts(self.addr.id, kept),
    }
  }

  /// `version | EndpointId 32 | salle 16 | longueur du relais (0 : aucun) | relais | n | adresses IP`.
  #[must_use]
  pub fn encode(&self) -> Vec<u8> {
    let mut out = vec![INVITE_VERSION];
    out.extend_from_slice(self.addr.id.as_bytes());
    out.extend_from_slice(&self.room.0);
    let relay = self
      .addr
      .relay_urls()
      .next()
      .map(ToString::to_string)
      .and_then(|url| u8::try_from(url.len()).ok().map(|len| (len, url)));
    match relay {
      Some((len, url)) => {
        out.push(len);
        out.extend_from_slice(url.as_bytes());
      }
      None => out.push(0),
    }
    let ips: Vec<&SocketAddr> = self.addr.ip_addrs().take(MAX_IPS).collect();
    out.push(u8::try_from(ips.len()).unwrap_or(0));
    for ip in ips {
      match ip {
        SocketAddr::V4(v4) => {
          out.push(4);
          out.extend_from_slice(&v4.ip().octets());
        }
        SocketAddr::V6(v6) => {
          out.push(6);
          out.extend_from_slice(&v6.ip().octets());
        }
      }
      out.extend_from_slice(&ip.port().to_be_bytes());
    }
    out
  }

  /// `None` pour toute forme inattendue : version, longueurs, octets en trop.
  #[must_use]
  pub fn decode(bytes: &[u8]) -> Option<Self> {
    let mut reader = Reader(bytes);
    if reader.byte()? != INVITE_VERSION {
      return None;
    }
    let id = PublicKey::from_bytes(&reader.array::<KEY_LEN>()?).ok()?;
    let room = Room(reader.array::<ROOM_LEN>()?);
    let relay_len = usize::from(reader.byte()?);
    let mut addrs = Vec::new();
    if relay_len > 0 {
      let url = std::str::from_utf8(reader.take(relay_len)?).ok()?;
      addrs.push(TransportAddr::Relay(url.parse::<RelayUrl>().ok()?));
    }
    let count = usize::from(reader.byte()?);
    if count > MAX_IPS {
      return None;
    }
    for _ in 0..count {
      let family = reader.byte()?;
      let ip: IpAddr = match family {
        4 => Ipv4Addr::from(reader.array::<4>()?).into(),
        6 => Ipv6Addr::from(reader.array::<16>()?).into(),
        _ => return None,
      };
      let port = u16::from_be_bytes(reader.array::<2>()?);
      addrs.push(TransportAddr::Ip(SocketAddr::new(ip, port)));
    }
    reader.0.is_empty().then(|| Self {
      room,
      addr: EndpointAddr::from_parts(id, addrs),
    })
  }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
  fn take(&mut self, len: usize) -> Option<&'a [u8]> {
    let (head, tail) = self.0.split_at_checked(len)?;
    self.0 = tail;
    Some(head)
  }

  fn byte(&mut self) -> Option<u8> {
    self.take(1).map(|bytes| bytes[0])
  }

  fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
    self.take(N)?.try_into().ok()
  }
}

const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Base32 minuscule sans remplissage.
#[must_use]
pub fn base32_encode(bytes: &[u8]) -> String {
  let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
  let mut buffer = 0u32;
  let mut bits = 0;
  for byte in bytes {
    buffer = (buffer << 8) | u32::from(*byte);
    bits += 8;
    while bits >= 5 {
      bits -= 5;
      out.push(char::from(BASE32[((buffer >> bits) & 31) as usize]));
    }
    buffer &= (1 << bits) - 1;
  }
  if bits > 0 {
    out.push(char::from(BASE32[((buffer << (5 - bits)) & 31) as usize]));
  }
  out
}

#[must_use]
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
  let mut out = Vec::with_capacity(text.len() * 5 / 8);
  let mut buffer = 0u32;
  let mut bits = 0;
  for ch in text.bytes() {
    let value = match ch {
      b'a'..=b'z' => ch - b'a',
      b'2'..=b'7' => ch - b'2' + 26,
      _ => return None,
    };
    buffer = (buffer << 5) | u32::from(value);
    bits += 5;
    if bits >= 8 {
      bits -= 8;
      out.push(u8::try_from((buffer >> bits) & 0xFF).ok()?);
      buffer &= (1 << bits) - 1;
    }
  }
  Some(out)
}

#[cfg(test)]
mod tests {
  use super::*;
  use iroh::SecretKey as Key;

  fn code(text: &str) -> JamCode {
    JamCode::parse(text).expect("code")
  }

  fn invite() -> Invite {
    let id = Key::from_bytes(&[7u8; 32]).public();
    let addr = EndpointAddr::from_parts(
      id,
      [
        TransportAddr::Ip("203.0.113.5:4000".parse().expect("ip")),
        TransportAddr::Ip("[2606:4700::1]:5000".parse().expect("ip")),
      ],
    )
    .with_relay_url("https://relay.example./".parse().expect("relay"));
    Invite {
      room: Room([9; ROOM_LEN]),
      addr,
    }
  }

  #[test]
  fn the_alphabet_has_32_symbols_without_look_alikes() {
    assert_eq!(ALPHABET.len(), 32);
    for banned in *b"IO01" {
      assert!(!ALPHABET.contains(&banned));
    }
    let mut sorted = ALPHABET.to_vec();
    sorted.dedup();
    assert_eq!(sorted.len(), 32);
  }

  #[test]
  fn every_bit_pattern_maps_to_six_symbols_of_the_alphabet() {
    assert_eq!(JamCode::from_bits(0).normalized(), "AAAAAA");
    assert_eq!(JamCode::from_bits((1 << 30) - 1).normalized(), "999999");
    // Les 2 bits de poids fort du mot sont ignorés.
    assert_eq!(
      JamCode::from_bits(u32::MAX),
      JamCode::from_bits((1 << 30) - 1)
    );
    for _ in 0..200 {
      let random = JamCode::random().expect("random");
      assert_eq!(random.normalized().len(), CODE_LEN);
      assert!(random.normalized().bytes().all(|b| ALPHABET.contains(&b)));
    }
  }

  #[test]
  fn a_code_is_shown_with_a_dash_and_round_trips() {
    let code = code("K7QM2X");
    assert_eq!(code.display(), "K7Q-M2X");
    assert_eq!(JamCode::parse(&code.display()), Some(code.clone()));
    assert_eq!(JamCode::parse(code.normalized()), Some(code));
  }

  #[test]
  fn the_input_is_trimmed_uppercased_and_stripped_of_dashes_and_spaces() {
    let expected = code("K7QM2X");
    for typed in [
      "k7qm2x",
      "  K7Q-M2X \n",
      "k7q m2x",
      "k-7-q-m-2-x",
      "K7Q\u{a0}M2X",
      "\tk7q-m2x",
    ] {
      assert_eq!(JamCode::parse(typed), Some(expected.clone()), "{typed:?}");
    }
  }

  #[test]
  fn the_look_alike_characters_and_wrong_lengths_are_rejected() {
    for bad in [
      "I7QM2X", "O7QM2X", "07QM2X", "17QM2X", "K7QM2", "K7QM2XA", "", "---", "K7QM2é", "K7QM2!",
      "riffjam",
    ] {
      assert!(JamCode::parse(bad).is_none(), "{bad:?}");
    }
  }

  #[test]
  fn the_legacy_prefix_is_detected_without_case() {
    assert!(is_legacy("riffjamabc"));
    assert!(is_legacy("  RIFFJAMabc"));
    assert!(!is_legacy("K7Q-M2X"));
    assert!(!is_legacy("riff"));
    assert!(!is_legacy(""));
  }

  #[test]
  fn the_kdf_is_deterministic_and_tied_to_the_code() {
    let first = derive_with(&code("K7QM2X"), MEMORY_KIB, PASSES).expect("kdf");
    let again = derive_with(&code("k7q-m2x"), MEMORY_KIB, PASSES).expect("kdf");
    let other = derive_with(&code("K7QM2Y"), MEMORY_KIB, PASSES).expect("kdf");
    assert_eq!(first.public(), again.public());
    assert_eq!(first.cipher, again.cipher);
    assert_ne!(first.public(), other.public());
    assert_ne!(first.cipher, other.cipher);
    assert_ne!(first.rendezvous.to_bytes(), first.cipher);
  }

  /// Vecteur figé : une dérivation qui changerait casserait la compatibilité entre deux versions de Riff.
  #[test]
  fn the_kdf_matches_a_frozen_vector() {
    let keys = derive_with(&code("K7QM2X"), MEMORY_KIB, PASSES).expect("kdf");
    assert_eq!(
      hex(&keys.rendezvous.to_bytes()),
      "71ff61de8b1cd677761c4f3f29701940da28e36f3470fbd05e511bddbf457098",
      "clé de rendez-vous"
    );
    assert_eq!(
      hex(&keys.cipher),
      "dc51d887c307f4c40763802b489b34104a564864bb43d0e90eb31b293c482ec9",
      "clé AES"
    );
  }

  /// Mêmes paramètres que la production (64 MiB, 3 passes).
  #[test]
  fn the_kdf_matches_a_frozen_vector_with_production_parameters() {
    let keys = derive_with(&code("K7QM2X"), PROD_MEMORY_KIB, PROD_PASSES).expect("kdf");
    assert_eq!(
      hex(&keys.rendezvous.to_bytes()),
      "893040fbe5544f3ea783a2ec738b462d34c944e4fc620c6e9e5c543534b29d4f",
      "clé de rendez-vous"
    );
    assert_eq!(
      hex(&keys.cipher),
      "3164514ea7df31e647f054a75c9698ec26aa02a663249a1222e8863d850a14b6",
      "clé AES"
    );
  }

  fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut text, byte| {
      let _ = write!(text, "{byte:02x}");
      text
    })
  }

  #[tokio::test]
  async fn derive_runs_off_the_async_thread_and_matches_the_blocking_form() {
    let async_keys = derive(&code("K7QM2X")).await.expect("kdf");
    let blocking = derive_with(&code("K7QM2X"), MEMORY_KIB, PASSES).expect("kdf");
    assert_eq!(async_keys.public(), blocking.public());
  }

  #[test]
  fn sealing_round_trips_and_uses_a_fresh_nonce() {
    let keys = derive_with(&code("K7QM2X"), MEMORY_KIB, PASSES).expect("kdf");
    let first = keys.seal(b"payload").expect("seal");
    let second = keys.seal(b"payload").expect("seal");
    assert_ne!(first, second);
    assert_eq!(keys.open(&first).as_deref(), Some(&b"payload"[..]));
    assert_eq!(keys.open(&second).as_deref(), Some(&b"payload"[..]));
  }

  #[test]
  fn opening_fails_with_another_key_or_a_tampered_text() {
    let keys = derive_with(&code("K7QM2X"), MEMORY_KIB, PASSES).expect("kdf");
    let other = derive_with(&code("K7QM2Y"), MEMORY_KIB, PASSES).expect("kdf");
    let sealed = keys.seal(b"payload").expect("seal");
    assert!(other.open(&sealed).is_none());
    let mut tampered = sealed.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    assert!(keys.open(&tampered).is_none());
    assert!(keys.open(&sealed[..NONCE_LEN - 1]).is_none());
    assert!(keys.open(&[]).is_none());
  }

  #[test]
  fn the_associated_data_binds_the_ciphertext_to_riff() {
    let keys = derive_with(&code("K7QM2X"), MEMORY_KIB, PASSES).expect("kdf");
    let nonce = [5u8; NONCE_LEN];
    let cipher = Aes256Gcm::new((&keys.cipher).into());
    let mut sealed = nonce.to_vec();
    sealed.extend(
      cipher
        .encrypt(
          &Nonce::from(nonce),
          Payload {
            msg: b"payload",
            aad: b"another/aad",
          },
        )
        .expect("encrypt"),
    );
    assert!(keys.open(&sealed).is_none());
  }

  #[test]
  fn an_invite_round_trips() {
    let invite = invite();
    let bytes = invite.encode();
    assert_eq!(Invite::decode(&bytes), Some(invite.clone()));
    let bare = Invite {
      room: invite.room,
      addr: EndpointAddr::new(invite.addr.id),
    };
    assert_eq!(Invite::decode(&bare.encode()), Some(bare));
  }

  #[test]
  fn a_relay_only_invite_drops_the_ip_addresses() {
    let invite = invite();
    let slim = invite.relay_only();
    assert_eq!(slim.addr.ip_addrs().count(), 0);
    assert_eq!(slim.addr.relay_urls().count(), 1);
    assert_eq!(slim.addr.id, invite.addr.id);
    assert_eq!(Invite::decode(&slim.encode()), Some(slim));
  }

  #[test]
  fn an_invite_with_too_many_addresses_keeps_the_first_ones() {
    let mut invite = invite();
    for port in 1..=10u16 {
      invite.addr = invite
        .addr
        .with_ip_addr(format!("198.51.100.1:{port}").parse().expect("ip"));
    }
    let decoded = Invite::decode(&invite.encode()).expect("decode");
    assert_eq!(decoded.addr.ip_addrs().count(), MAX_IPS);
  }

  #[test]
  fn malformed_invites_are_rejected() {
    let good = invite().encode();
    let mut other_version = good.clone();
    other_version[0] = 2;
    let mut trailing = good.clone();
    trailing.push(0);
    let mut bad_family = good.clone();
    let family_at = 1 + KEY_LEN + ROOM_LEN + 1 + "https://relay.example./".len() + 1;
    bad_family[family_at] = 5;
    for bad in [
      other_version,
      trailing,
      bad_family,
      good[..good.len() - 1].to_vec(),
      good[..10].to_vec(),
      Vec::new(),
    ] {
      assert!(Invite::decode(&bad).is_none());
    }
  }

  #[test]
  fn base32_round_trips_every_length() {
    for len in 0..40usize {
      let bytes: Vec<u8> = (0..len)
        .map(|i| u8::try_from((i * 7 + 3) % 251).expect("byte"))
        .collect();
      let text = base32_encode(&bytes);
      assert!(text.bytes().all(|b| BASE32.contains(&b)));
      assert_eq!(base32_decode(&text), Some(bytes), "{len}");
    }
    assert_eq!(base32_encode(b"foobar"), "mzxw6ytboi");
    assert!(base32_decode("ab1").is_none());
    assert!(base32_decode("AB").is_none());
  }
}
