//! Fil du jam : trames, messages, pistes diffusées et assainissement des données reçues.

use super::{MAX_ID_CHARS, MAX_NAME_CHARS, MAX_PARTICIPANTS, MAX_TEXT_CHARS, MAX_URL_CHARS};
use crate::audio_prefs::{Sound, is_default_preset};
use crate::stream::effective_platform;
use crate::user_data::Track;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 2;
/// Borne d'une trame avant admission.
pub const MAX_HELLO_FRAME: usize = 4 * 1024;
/// Borne d'une trame après admission.
pub const MAX_FRAME: usize = 1 << 20;
/// Plateforme : longueur maximale.
const MAX_PLATFORM_CHARS: usize = 32;
const MAX_DURATION_SECS: f64 = 86_400.0;
/// Nom d'un participant qui n'en a pas.
pub const DEFAULT_GUEST_NAME: &str = "Guest";

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
  #[error("empty frame")]
  Empty,
  #[error("frame of {0} bytes is over the limit")]
  TooLarge(usize),
  #[error("truncated frame")]
  Truncated,
  #[error("{0}")]
  Io(#[from] std::io::Error),
}

/// Lit une trame : longueur `u32` gros-boutiste, puis le corps. `Ok(None)` sur fin de flux propre entre deux
/// trames. Une longueur hors borne est refusée avant toute lecture du corps.
pub async fn read_frame<R>(reader: &mut R, max: usize) -> Result<Option<Vec<u8>>, FrameError>
where
  R: AsyncRead + Unpin + ?Sized,
{
  let mut header = [0u8; 4];
  let mut filled = 0;
  while filled < header.len() {
    let read = reader.read(&mut header[filled..]).await?;
    if read == 0 {
      return if filled == 0 {
        Ok(None)
      } else {
        Err(FrameError::Truncated)
      };
    }
    filled += read;
  }
  let len = u32::from_be_bytes(header) as usize;
  if len == 0 {
    return Err(FrameError::Empty);
  }
  if len > max {
    return Err(FrameError::TooLarge(len));
  }
  let mut body = Vec::new();
  let read = reader.take(len as u64).read_to_end(&mut body).await?;
  if read < len {
    return Err(FrameError::Truncated);
  }
  Ok(Some(body))
}

pub async fn write_frame<W>(writer: &mut W, body: &[u8]) -> Result<(), FrameError>
where
  W: AsyncWrite + Unpin + ?Sized,
{
  let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(body.len()))?;
  writer.write_all(&len.to_be_bytes()).await?;
  writer.write_all(body).await?;
  writer.flush().await?;
  Ok(())
}

/// Enveloppe d'un message : `{"v": 1, "t": "...", ...}`.
#[derive(Serialize)]
struct Envelope<'a, T> {
  v: u32,
  #[serde(flatten)]
  body: &'a T,
}

/// Corps JSON d'un message, prêt à être cloné vers chaque destinataire.
pub fn encode<T: Serialize>(message: &T) -> Result<Arc<[u8]>, serde_json::Error> {
  encode_versioned(message, PROTOCOL_VERSION)
}

/// Comme [`encode`], avec une version d'enveloppe choisie : le refus de version doit rester lisible par le pair.
pub fn encode_versioned<T: Serialize>(
  message: &T,
  version: u32,
) -> Result<Arc<[u8]>, serde_json::Error> {
  serde_json::to_vec(&Envelope {
    v: version,
    body: message,
  })
  .map(Arc::from)
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
  #[error("invalid json: {0}")]
  Json(#[from] serde_json::Error),
  #[error("protocol version {0}")]
  Version(u64),
}

/// Décode un message ; la version est contrôlée avant le corps.
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, DecodeError> {
  let value: serde_json::Value = serde_json::from_slice(body)?;
  let version = value
    .get("v")
    .and_then(serde_json::Value::as_u64)
    .unwrap_or(0);
  if version != u64::from(PROTOCOL_VERSION) {
    return Err(DecodeError::Version(version));
  }
  Ok(serde_json::from_value(value)?)
}

/// Limite le débit des trames reçues : plus de `max` dans une fenêtre d'une seconde.
#[derive(Debug)]
pub struct RateLimit {
  max: u32,
  window_start: Option<Instant>,
  count: u32,
}

impl RateLimit {
  #[must_use]
  pub const fn new(max: u32) -> Self {
    Self {
      max,
      window_start: None,
      count: 0,
    }
  }

  /// Compte une trame ; `false` si la limite est dépassée.
  pub fn allow(&mut self, now: Instant) -> bool {
    match self.window_start {
      Some(start) if now.saturating_duration_since(start) < Duration::from_secs(1) => {
        self.count = self.count.saturating_add(1);
      }
      _ => {
        self.window_start = Some(now);
        self.count = 1;
      }
    }
    self.count <= self.max
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
  Room,
  Full,
  Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
  Joined,
  Left,
  Paused,
  Resumed,
  Added,
  ResolveFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
  LocalFile,
  QueueFull,
  TooMany,
  Invalid,
  UnknownEntry,
  Rate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
  ResolveFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
  Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
  pub id: u32,
  pub name: String,
  pub host: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
  pub kind: NoticeKind,
  pub by: u32,
  pub name: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub count: Option<u32>,
}

/// Piste telle qu'elle voyage : jamais de chemin local.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WireTrack {
  pub platform: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub artist: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub album: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub duration: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub thumbnail: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cover: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub search_query: Option<String>,
}

fn strip_controls(text: &str) -> String {
  text.chars().filter(|c| !c.is_control()).collect()
}

fn clean_text(text: Option<String>, max_chars: usize) -> Option<String> {
  let text: String = strip_controls(&text?).chars().take(max_chars).collect();
  (!text.is_empty()).then_some(text)
}

/// Hôte joignable seulement depuis la machine ou le réseau local : refusé, car l'URL serait résolue chez
/// chaque participant (SSRF, et routes du core via `backend::local_url`).
fn is_internal_host(host: url::Host<&str>) -> bool {
  match host {
    url::Host::Domain(domain) => {
      let domain = domain.trim_end_matches('.').to_ascii_lowercase();
      domain == "localhost" || domain.ends_with(".localhost")
    }
    url::Host::Ipv4(ip) => is_internal_v4(ip),
    url::Host::Ipv6(ip) => {
      let segments = ip.segments();
      ip.is_loopback()
        || ip.is_unspecified()
        || segments[0] & 0xfe00 == 0xfc00
        || segments[0] & 0xffc0 == 0xfe80
        || ip.to_ipv4_mapped().is_some_and(is_internal_v4)
    }
  }
}

fn is_internal_v4(ip: std::net::Ipv4Addr) -> bool {
  ip.is_loopback() || ip.is_unspecified() || ip.is_private() || ip.is_link_local()
}

fn clean_url(url: Option<String>) -> Option<String> {
  url.filter(|url| {
    url.chars().count() <= MAX_URL_CHARS
      && !url.chars().any(char::is_control)
      && crate::backend::local_url(url) == *url
      && url::Url::parse(url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https")
          && parsed.host().is_some_and(|host| !is_internal_host(host))
      })
  })
}

impl WireTrack {
  /// `None` pour un fichier local. La plateforme est calculée avec celle de la session de l'émetteur, comme
  /// `stream_info_query` : chaque participant résout le titre depuis la plateforme du titre. Une plateforme
  /// vide (ni sur le titre ni en session) donne aussi `None` : la règle du jam exige une plateforme explicite.
  #[must_use]
  pub fn from_track(track: &Track, session_platform: &str) -> Option<Self> {
    if track.is_local() {
      return None;
    }
    let platform = effective_platform(track, session_platform);
    if platform.is_empty() {
      return None;
    }
    Some(Self {
      platform,
      id: track.id.clone(),
      title: track.title.clone(),
      artist: track.artist.clone(),
      album: track.album.clone(),
      duration: track.duration,
      thumbnail: track.thumbnail.clone(),
      cover: track.cover.clone(),
      url: track.url.clone(),
      search_query: track.search_query.clone(),
    })
  }

  /// Piste reçue d'un pair, jamais crue : `None` si elle est inutilisable.
  #[must_use]
  pub fn sanitized(self) -> Option<Self> {
    let platform = strip_controls(&self.platform);
    if platform.is_empty() || platform.chars().count() > MAX_PLATFORM_CHARS || platform == "local" {
      return None;
    }
    let id = self
      .id
      .map(|id| strip_controls(&id))
      .filter(|id| !id.is_empty() && id.chars().count() <= MAX_ID_CHARS);
    let url = clean_url(self.url);
    let title = clean_text(self.title, MAX_TEXT_CHARS);
    if id.is_none() && url.is_none() && title.is_none() {
      return None;
    }
    Some(Self {
      platform,
      id,
      title,
      artist: clean_text(self.artist, MAX_TEXT_CHARS),
      album: clean_text(self.album, MAX_TEXT_CHARS),
      duration: self
        .duration
        .filter(|secs| secs.is_finite() && (0.0..=MAX_DURATION_SECS).contains(secs)),
      thumbnail: clean_url(self.thumbnail),
      cover: clean_url(self.cover),
      url,
      search_query: clean_text(self.search_query, MAX_TEXT_CHARS),
    })
  }

  #[must_use]
  pub fn into_track(self) -> Track {
    Track {
      id: self.id,
      title: self.title,
      artist: self.artist,
      album: self.album,
      duration: self.duration,
      thumbnail: self.thumbnail,
      cover: self.cover,
      url: self.url,
      search_query: self.search_query,
      platform: Some(self.platform),
      ..Track::default()
    }
  }
}

/// Nom affiché : sans caractère de contrôle, `MAX_NAME_CHARS` au plus, `Guest` si vide.
#[must_use]
pub fn sanitize_name(name: &str) -> String {
  let name: String = strip_controls(name.trim())
    .chars()
    .take(MAX_NAME_CHARS)
    .collect();
  let name = name.trim();
  if name.is_empty() {
    DEFAULT_GUEST_NAME.to_string()
  } else {
    name.to_string()
  }
}

/// Nom unique parmi `taken` : suffixe ` (2)`, ` (3)`… sans dépasser `MAX_NAME_CHARS`.
#[must_use]
pub fn unique_name<'a>(name: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
  let is_taken = |candidate: &str| taken.clone().any(|other| other == candidate);
  if !is_taken(name) {
    return name.to_string();
  }
  (2u32..)
    .map(|n| {
      let suffix = format!(" ({n})");
      let keep = MAX_NAME_CHARS.saturating_sub(suffix.chars().count());
      let base: String = name.chars().take(keep).collect();
      format!("{base}{suffix}")
    })
    .find(|candidate| !is_taken(candidate))
    .unwrap_or_else(|| name.to_string())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
  pub uid: u64,
  pub by: u32,
  pub by_name: String,
  pub track: WireTrack,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QueueState {
  pub qv: u64,
  pub entries: Vec<Entry>,
}

impl QueueState {
  /// La file tient dans une trame, avec une marge pour l'enveloppe et les autres champs.
  #[must_use]
  pub fn fits_in_frame(&self) -> bool {
    encode(self).is_ok_and(|body| body.len() <= MAX_FRAME - 4096)
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Current {
  pub uid: Option<u64>,
  pub track: WireTrack,
}

/// Effets partagés par tout le jam : ceux qui changent le son ou le défilement. Volume, égaliseur, HQ et
/// normalisation restent personnels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JamSound {
  pub speed: f32,
  pub speed_pitch: f32,
  pub pitch: f32,
  pub reverb: f32,
  pub distortion: f32,
  pub echo: f32,
}

impl Default for JamSound {
  fn default() -> Self {
    Self::from_sound(&Sound::default())
  }
}

impl JamSound {
  #[must_use]
  pub const fn from_sound(sound: &Sound) -> Self {
    Self {
      speed: sound.speed,
      speed_pitch: sound.speed_pitch,
      pitch: sound.pitch,
      reverb: sound.reverb,
      distortion: sound.distortion,
      echo: sound.echo,
    }
  }

  /// `base` dont les champs partagés sont remplacés ; le reste (volume, égaliseur) est gardé.
  #[must_use]
  pub const fn apply_to(self, mut base: Sound) -> Sound {
    base.speed = self.speed;
    base.speed_pitch = self.speed_pitch;
    base.pitch = self.pitch;
    base.reverb = self.reverb;
    base.distortion = self.distortion;
    base.echo = self.echo;
    base
  }

  /// Valeurs reçues d'un pair : bornées comme les curseurs, une valeur non finie reprend le défaut.
  #[must_use]
  pub fn sanitized(self) -> Self {
    Self::from_sound(&self.apply_to(Sound::default()).clamped())
  }

  /// Facteur d'écoulement de la position, celui du moteur : le pitch change aussi la vitesse.
  #[must_use]
  pub fn rate(self) -> f64 {
    let rate = self
      .apply_to(Sound::default())
      .to_engine()
      .tempo()
      .speed_factor();
    if rate.is_finite() {
      rate.max(MIN_RATE)
    } else {
      1.0
    }
  }
}

/// Plus petit facteur de vitesse admis dans les calculs de position (garde-fou, les curseurs vont à 0,25).
const MIN_RATE: f64 = 0.05;
/// Acquittements portés par un état : un par participant, hôte exclu.
const MAX_ACKS: usize = MAX_PARTICIPANTS;

/// Effets du jam tels que l'hôte les a appliqués.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SharedSound {
  /// Compteur de l'hôte, +1 à chaque changement.
  pub sv: u64,
  /// Participant qui a réglé les effets.
  pub by: u32,
  /// Transition douce demandée par l'auteur.
  pub smooth: bool,
  /// Préréglage par défaut qui a produit ces valeurs.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub preset: Option<String>,
  pub sound: JamSound,
  /// Dernier identifiant de commande `Sound` appliqué pour chaque participant.
  pub acks: Vec<(u32, u64)>,
}

impl SharedSound {
  /// État reçu de l'hôte, jamais cru : valeurs bornées, préréglage connu, acquittements en nombre borné.
  #[must_use]
  pub fn sanitized(mut self) -> Self {
    self.sound = self.sound.sanitized();
    self.preset = self.preset.filter(|id| is_default_preset(id));
    self.acks.truncate(MAX_ACKS);
    self
  }

  /// Dernier acquittement de `participant`.
  #[must_use]
  pub fn ack_of(&self, participant: u32) -> Option<u64> {
    self
      .acks
      .iter()
      .find(|(id, _)| *id == participant)
      .map(|(_, ack)| *ack)
  }

  /// Note que la commande `id` de `participant` est appliquée.
  pub fn acknowledge(&mut self, participant: u32, id: u64) {
    self.acks.retain(|(who, _)| *who != participant);
    self.acks.push((participant, id));
    if self.acks.len() > MAX_ACKS {
      self.acks.remove(0);
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaybackState {
  pub pv: u64,
  pub current: Option<Current>,
  pub start: u64,
  pub seek: u64,
  pub playing: bool,
  pub pos_ms: u64,
  pub at_ms: u64,
  pub shuffle: bool,
  pub repeat: bool,
  pub fade: bool,
  #[serde(default)]
  pub sound: SharedSound,
}

/// Commande d'un participant, appliquée par l'hôte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
  Play,
  Pause,
  Seek {
    pos_ms: u64,
  },
  Next {
    from: Option<u64>,
  },
  Prev {
    from: Option<u64>,
  },
  Add {
    tracks: Vec<WireTrack>,
    next: bool,
  },
  PlayNow {
    tracks: Vec<WireTrack>,
  },
  PlayEntry {
    uid: u64,
  },
  Remove {
    uids: Vec<u64>,
  },
  Move {
    uid: u64,
    to: u32,
  },
  Clear,
  Shuffle {
    on: bool,
  },
  Repeat {
    on: bool,
  },
  /// Effets partagés. `preset` n'est retenu que s'il est un préréglage par défaut.
  Sound {
    sound: JamSound,
    #[serde(default)]
    smooth: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preset: Option<String>,
  },
}

/// Messages de l'invité vers l'hôte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
  Hello {
    room: String,
    name: String,
    app: String,
  },
  Cmd {
    id: u64,
    op: Op,
  },
  Report {
    kind: ReportKind,
    uid: u64,
  },
  Ping {
    t0: u64,
  },
  Bye,
  /// Type inconnu : ignoré (compatibilité ascendante).
  #[serde(other)]
  Unknown,
}

/// Messages de l'hôte vers l'invité.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum HostMsg {
  Welcome {
    you: u32,
    host_ms: u64,
    participants: Vec<Participant>,
    queue: QueueState,
    playback: PlaybackState,
  },
  Refused {
    reason: Refusal,
  },
  Queue(QueueState),
  Playback(PlaybackState),
  Participants {
    list: Vec<Participant>,
  },
  Notice(Notice),
  Rejected {
    id: u64,
    reason: RejectReason,
  },
  Pong {
    t0: u64,
    t1: u64,
  },
  End {
    reason: EndReason,
  },
  /// Type inconnu : ignoré (compatibilité ascendante).
  #[serde(other)]
  Unknown,
}
