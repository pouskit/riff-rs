//! Lecture HLS en AAC fMP4 (SoundCloud) : analyse des playlists et récupération des segments par le core.
//!
//! Seuls les flux non chiffrés, à segments fMP4 avec `#EXT-X-MAP`, terminés par `#EXT-X-ENDLIST` sont lus.
//! Le décodage et le déplacement par segment sont dans `riff_audio::decode`.

use crate::stream::{proxy_call, reply_reader};
use bytes::Bytes;
use iced::futures::StreamExt;
use riff_audio::{SegmentFetcher, Segmented, Source};
use riff_core::core::Core;
use riff_core::reply::{Method, Reply};
use std::io::{self, Read};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use url::Url;

/// Taille maximale d'une playlist : 371 segments d'un titre de une heure font environ 305 Ko.
const PLAYLIST_MAX: usize = 4 << 20;
/// Taille maximale de l'en-tête d'initialisation (761 octets observés).
const INIT_MAX: usize = 1 << 20;
/// Marge avant l'expiration d'une URL signée sous laquelle elle n'est plus utilisée.
const EXPIRY_MARGIN_SECS: u64 = 60;
/// Durée maximale annoncée pour un segment : au-delà, la playlist est aberrante.
const SEGMENT_SECS_MAX: f64 = 86_400.0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HlsError {
  #[error("This stream is encrypted and cannot be played")]
  Encrypted,
  #[error("flux HLS non pris en charge : {0}")]
  Unsupported(&'static str),
  #[error("playlist HLS invalide : {0}")]
  Invalid(&'static str),
  #[error("URL signée expirée")]
  Expired,
  #[error("réseau : {0}")]
  Network(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
  pub bandwidth: u64,
  pub codecs: Option<String>,
  pub uri: Url,
}

/// URLs des segments : préfixe et suffixe communs mémorisés une fois (les URLs signées d'un titre ne
/// diffèrent que par le numéro), ce qui ramène environ 305 Ko à environ 5 Ko.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SegmentUrls {
  prefix: String,
  suffix: String,
  middles: Vec<Box<str>>,
}

impl SegmentUrls {
  #[must_use]
  pub fn new(urls: &[String]) -> Self {
    let Some(first) = urls.first() else {
      return Self::default();
    };
    let mut prefix_len = first.len();
    for url in &urls[1..] {
      let common = first
        .bytes()
        .zip(url.bytes())
        .take_while(|(a, b)| a == b)
        .count();
      prefix_len = prefix_len.min(common);
    }
    while !first.is_char_boundary(prefix_len) {
      prefix_len -= 1;
    }
    let rest = &first[prefix_len..];
    let mut suffix_len = rest.len();
    for url in &urls[1..] {
      let common = rest
        .bytes()
        .rev()
        .zip(url[prefix_len..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
      suffix_len = suffix_len.min(common);
    }
    while !rest.is_char_boundary(rest.len() - suffix_len) {
      suffix_len -= 1;
    }
    Self {
      prefix: first[..prefix_len].to_string(),
      suffix: rest[rest.len() - suffix_len..].to_string(),
      middles: urls
        .iter()
        .map(|url| url[prefix_len..url.len() - suffix_len].into())
        .collect(),
    }
  }

  #[must_use]
  pub fn len(&self) -> usize {
    self.middles.len()
  }

  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.middles.is_empty()
  }

  #[must_use]
  pub fn get(&self, index: usize) -> Option<String> {
    self
      .middles
      .get(index)
      .map(|middle| format!("{}{middle}{}", self.prefix, self.suffix))
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaPlaylist {
  pub init: Url,
  /// Durée de chaque segment (`#EXTINF`), en secondes.
  pub durations: Vec<f64>,
  pub segments: SegmentUrls,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Playlist {
  Master(Vec<Variant>),
  Media(MediaPlaylist),
}

/// Attributs `CLE=valeur,CLE="valeur, avec virgule"` d'une ligne de balise.
fn attributes(list: &str) -> Vec<(&str, &str)> {
  let mut out = Vec::new();
  let (mut start, mut quoted) = (0, false);
  for (at, ch) in list.char_indices().chain([(list.len(), ',')]) {
    match ch {
      '"' => quoted = !quoted,
      ',' if !quoted => {
        if let Some((key, value)) = list[start..at].split_once('=') {
          out.push((key.trim(), value.trim().trim_matches('"')));
        }
        start = at + 1;
      }
      _ => {}
    }
  }
  out
}

fn attribute<'a>(list: &[(&'a str, &'a str)], key: &str) -> Option<&'a str> {
  list.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Analyse une playlist maîtresse ou média ; les URI relatives sont résolues sur `base`.
///
/// # Errors
/// Texte qui n'est pas une playlist, flux chiffré ou forme non prise en charge.
pub fn parse(text: &str, base: &Url) -> Result<Playlist, HlsError> {
  let mut lines = text
    .trim_start_matches('\u{feff}')
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty());
  if lines.next() != Some("#EXTM3U") {
    return Err(HlsError::Invalid("#EXTM3U absent"));
  }
  let resolve = |uri: &str| {
    base
      .join(uri)
      .map_err(|_| HlsError::Invalid("URI illisible"))
  };
  let mut variants = Vec::new();
  let mut pending_variant: Option<(u64, Option<String>)> = None;
  let mut pending_duration: Option<f64> = None;
  let mut init = None;
  let mut durations = Vec::new();
  let mut urls = Vec::new();
  let mut ended = false;
  for line in lines {
    if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
      let attrs = attributes(attrs);
      pending_variant = Some((
        attribute(&attrs, "BANDWIDTH")
          .and_then(|v| v.parse().ok())
          .unwrap_or(0),
        attribute(&attrs, "CODECS").map(str::to_string),
      ));
    } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
      if attribute(&attributes(attrs), "METHOD") != Some("NONE") {
        return Err(HlsError::Encrypted);
      }
    } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
      let attrs = attributes(attrs);
      if attribute(&attrs, "BYTERANGE").is_some() {
        return Err(HlsError::Unsupported("BYTERANGE"));
      }
      let uri = attribute(&attrs, "URI").ok_or(HlsError::Invalid("MAP sans URI"))?;
      init = Some(resolve(uri)?);
    } else if line.starts_with("#EXT-X-BYTERANGE") {
      return Err(HlsError::Unsupported("BYTERANGE"));
    } else if let Some(value) = line.strip_prefix("#EXTINF:") {
      let seconds = value.split(',').next().unwrap_or("").trim();
      let seconds: f64 = seconds
        .parse()
        .map_err(|_| HlsError::Invalid("EXTINF illisible"))?;
      if !(0.0..=SEGMENT_SECS_MAX).contains(&seconds) {
        return Err(HlsError::Invalid("EXTINF hors limites"));
      }
      pending_duration = Some(seconds);
    } else if line == "#EXT-X-ENDLIST" {
      ended = true;
    } else if line.starts_with('#') {
      continue;
    } else if let Some((bandwidth, codecs)) = pending_variant.take() {
      variants.push(Variant {
        bandwidth,
        codecs,
        uri: resolve(line)?,
      });
    } else if let Some(duration) = pending_duration.take() {
      durations.push(duration);
      urls.push(resolve(line)?.into());
    }
  }
  if !variants.is_empty() {
    return Ok(Playlist::Master(variants));
  }
  let init = init.ok_or(HlsError::Unsupported("segments sans #EXT-X-MAP"))?;
  if !ended {
    return Err(HlsError::Unsupported("flux sans #EXT-X-ENDLIST"));
  }
  if urls.is_empty() {
    return Err(HlsError::Invalid("aucun segment"));
  }
  Ok(Playlist::Media(MediaPlaylist {
    init,
    durations,
    segments: SegmentUrls::new(&urls),
  }))
}

/// Variante à lire dans une playlist maîtresse : le plus haut débit en AAC (ou sans codec indiqué).
#[must_use]
pub fn pick_variant(variants: &[Variant]) -> Option<&Variant> {
  variants
    .iter()
    .filter(|variant| {
      variant
        .codecs
        .as_deref()
        .is_none_or(|codecs| codecs.contains("mp4a"))
    })
    .max_by_key(|variant| variant.bandwidth)
}

/// Instant d'expiration (secondes Unix) d'une URL signée.
#[must_use]
pub fn expires_of(url: &str) -> Option<u64> {
  Url::parse(url)
    .ok()?
    .query_pairs()
    .find(|(key, _)| key == "expires")
    .and_then(|(_, value)| value.parse().ok())
}

/// Segments lus à travers `audio-proxy` du core.
pub struct HlsFetcher {
  core: Arc<Core>,
  urls: SegmentUrls,
  runtime: Handle,
}

impl SegmentFetcher for HlsFetcher {
  fn fetch(&self, index: usize) -> io::Result<Box<dyn Read + Send>> {
    let url = self
      .urls
      .get(index)
      .ok_or_else(|| io::Error::other("segment hors playlist"))?;
    // Un seul nouvel essai : une connexion réutilisée peut être morte, ou le CDN répondre 5xx.
    let mut last = io::Error::other("segment indisponible");
    for _ in 0..2 {
      match self
        .runtime
        .block_on(proxy_call(&self.core, Method::Get, &url, None))
      {
        Ok(reply) if reply.status() < 500 => {
          let expected = match &reply {
            Reply::Stream { headers, .. } => headers
              .iter()
              .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
              .and_then(|(_, value)| value.trim().parse::<u64>().ok()),
            _ => None,
          };
          return reply_reader(reply, 0, &self.runtime)
            .map(|reader| match expected {
              Some(expected) => Box::new(ExactLength {
                inner: reader,
                expected,
                received: 0,
              }) as Box<dyn Read + Send>,
              None => reader,
            })
            .inspect_err(|err| {
              tracing::warn!(index, error = %err, "segment HLS inutilisable");
            });
        }
        Ok(reply) => last = io::Error::other(format!("réponse {}", reply.status())),
        Err(err) => last = io::Error::other(err),
      }
    }
    tracing::warn!(index, error = %last, "segment HLS refusé");
    Err(last)
  }
}

/// Signale un segment tronqué : une fin de corps avant `Content-Length` est une erreur, pas une fin
/// de segment, sans quoi le décodeur lirait des octets coupés comme un segment complet.
struct ExactLength {
  inner: Box<dyn Read + Send>,
  expected: u64,
  received: u64,
}

impl Read for ExactLength {
  fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
    let count = self.inner.read(buffer)?;
    self.received += count as u64;
    if count == 0 && !buffer.is_empty() && self.received < self.expected {
      return Err(io::Error::other(format!(
        "segment tronqué : {} octets sur {}",
        self.received, self.expected
      )));
    }
    Ok(count)
  }
}

/// Lit le corps d'une réponse en mémoire, au plus `max` octets.
async fn read_body(reply: Reply, max: usize) -> Result<Bytes, HlsError> {
  let status = reply.status();
  if !(200..300).contains(&status) {
    return Err(HlsError::Network(format!("réponse {status}")));
  }
  match reply {
    Reply::Bytes { body, .. } if body.len() <= max => Ok(body),
    Reply::Stream { mut body, .. } => {
      let mut out = Vec::new();
      while let Some(chunk) = body.next().await {
        out.extend_from_slice(&chunk.map_err(|err| HlsError::Network(err.to_string()))?);
        if out.len() > max {
          return Err(HlsError::Invalid("réponse trop volumineuse"));
        }
      }
      Ok(out.into())
    }
    _ => Err(HlsError::Invalid("réponse sans octets")),
  }
}

async fn fetch_bytes(core: &Arc<Core>, url: &Url, max: usize) -> Result<Bytes, HlsError> {
  let reply = proxy_call(core, Method::Get, url.as_str(), None)
    .await
    .map_err(HlsError::Network)?;
  read_body(reply, max).await
}

async fn fetch_playlist(core: &Arc<Core>, url: &Url) -> Result<Playlist, HlsError> {
  let bytes = fetch_bytes(core, url, PLAYLIST_MAX).await?;
  let text = std::str::from_utf8(&bytes).map_err(|_| HlsError::Invalid("playlist non UTF-8"))?;
  parse(text, url)
}

fn now_secs() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |elapsed| elapsed.as_secs())
}

/// Un refus ou une erreur réseau sur la playlist ou l'en-tête, sans résolution neuve : l'URL signée
/// du cache est sans doute périmée (le proxy rend 502 pour tout refus en amont).
fn stale_if_cached(err: HlsError, fresh: bool) -> HlsError {
  match err {
    HlsError::Network(_) if !fresh => HlsError::Expired,
    other => other,
  }
}

/// Ouvre un titre HLS : lit la playlist (et la variante choisie d'une playlist maîtresse) et l'en-tête.
/// Aucun segment n'est demandé avant la lecture.
///
/// Sans `fresh`, l'URL peut venir du cache : elle doit rester valable pendant tout le titre, et un
/// échec réseau sur la playlist ou l'en-tête rend `Expired` pour provoquer une résolution neuve.
///
/// # Errors
/// Flux chiffré ou non pris en charge, URL signée expirée, erreur réseau.
pub async fn open(core: &Arc<Core>, url: &str, fresh: bool) -> Result<Source, HlsError> {
  let base = Url::parse(url).map_err(|_| HlsError::Invalid("URL illisible"))?;
  let media = match fetch_playlist(core, &base)
    .await
    .map_err(|err| stale_if_cached(err, fresh))?
  {
    Playlist::Media(media) => media,
    Playlist::Master(variants) => {
      let variant = pick_variant(&variants).ok_or(HlsError::Unsupported("aucune variante AAC"))?;
      match fetch_playlist(core, &variant.uri)
        .await
        .map_err(|err| stale_if_cached(err, fresh))?
      {
        Playlist::Media(media) => media,
        Playlist::Master(_) => return Err(HlsError::Unsupported("playlists imbriquées")),
      }
    }
  };
  let mut starts = Vec::with_capacity(media.durations.len());
  let mut total = 0.0;
  for seconds in &media.durations {
    starts.push(Duration::try_from_secs_f64(total).map_err(|_| HlsError::Invalid("durée"))?);
    total += seconds;
  }
  let duration = Duration::try_from_secs_f64(total).map_err(|_| HlsError::Invalid("durée"))?;
  let first = media.segments.get(0).unwrap_or_default();
  let needed = if fresh { 0 } else { duration.as_secs() };
  let limit = now_secs() + EXPIRY_MARGIN_SECS + needed;
  if [url, media.init.as_str(), first.as_str()]
    .into_iter()
    .filter_map(expires_of)
    .any(|expires| expires < limit)
  {
    return Err(HlsError::Expired);
  }
  let init = fetch_bytes(core, &media.init, INIT_MAX)
    .await
    .map_err(|err| stale_if_cached(err, fresh))?;
  Ok(Source::Segmented(Segmented {
    init: init.to_vec().into(),
    starts: starts.into(),
    duration,
    fetcher: Arc::new(HlsFetcher {
      core: Arc::clone(core),
      urls: media.segments,
      runtime: Handle::current(),
    }),
  }))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn base() -> Url {
    Url::parse("https://cdn.example/a/b/playlist.m3u8?x=1").unwrap()
  }

  const MEDIA: &str = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-PLAYLIST-TYPE:VOD\n\
    #EXT-X-MAP:URI=\"init.mp4?expires=1\"\n#EXTINF:10.007800,\nhttps://other.example/d0.m4s?expires=1\n\
    #EXTINF:2.5,\nd1.m4s?expires=1\n#EXT-X-ENDLIST\n";

  fn media(text: &str) -> MediaPlaylist {
    match parse(text, &base()).unwrap() {
      Playlist::Media(media) => media,
      Playlist::Master(_) => panic!("playlist maîtresse"),
    }
  }

  #[test]
  fn media_playlist_resolves_relative_and_absolute_uris() {
    let media = media(MEDIA);
    assert_eq!(
      media.init.as_str(),
      "https://cdn.example/a/b/init.mp4?expires=1"
    );
    assert_eq!(media.durations, vec![10.0078, 2.5]);
    assert_eq!(media.segments.len(), 2);
    assert_eq!(
      media.segments.get(0).as_deref(),
      Some("https://other.example/d0.m4s?expires=1")
    );
    assert_eq!(
      media.segments.get(1).as_deref(),
      Some("https://cdn.example/a/b/d1.m4s?expires=1")
    );
    assert_eq!(media.segments.get(2), None);
  }

  #[test]
  fn encrypted_streams_are_refused_but_method_none_passes() {
    let keyed = |method: &str| {
      MEDIA.replace(
        "#EXT-X-MAP",
        &format!("#EXT-X-KEY:METHOD={method}\n#EXT-X-MAP"),
      )
    };
    assert_eq!(parse(&keyed("AES-128"), &base()), Err(HlsError::Encrypted));
    assert_eq!(
      parse(&keyed("SAMPLE-AES"), &base()),
      Err(HlsError::Encrypted)
    );
    assert!(parse(&keyed("NONE"), &base()).is_ok());
  }

  #[test]
  fn unsupported_shapes_are_reported() {
    let no_map = MEDIA.replace("#EXT-X-MAP:URI=\"init.mp4?expires=1\"\n", "");
    assert!(matches!(
      parse(&no_map, &base()),
      Err(HlsError::Unsupported(_))
    ));
    let live = MEDIA.replace("#EXT-X-ENDLIST\n", "");
    assert!(matches!(
      parse(&live, &base()),
      Err(HlsError::Unsupported(_))
    ));
    let ranged = MEDIA.replace("#EXTINF:2.5,", "#EXT-X-BYTERANGE:100@0\n#EXTINF:2.5,");
    assert!(matches!(
      parse(&ranged, &base()),
      Err(HlsError::Unsupported(_))
    ));
  }

  #[test]
  fn text_without_the_header_is_invalid() {
    assert!(matches!(
      parse("<html></html>", &base()),
      Err(HlsError::Invalid(_))
    ));
    assert!(matches!(parse("", &base()), Err(HlsError::Invalid(_))));
  }

  #[test]
  fn master_playlist_picks_the_best_aac_variant() {
    let master = "#EXTM3U\n\
      #EXT-X-STREAM-INF:BANDWIDTH=96000,CODECS=\"mp4a.40.2\"\nlow/p.m3u8\n\
      #EXT-X-STREAM-INF:BANDWIDTH=160000,CODECS=\"mp4a.40.2\"\nhigh/p.m3u8\n\
      #EXT-X-STREAM-INF:BANDWIDTH=320000,CODECS=\"opus\"\nopus/p.m3u8\n";
    let Playlist::Master(variants) = parse(master, &base()).unwrap() else {
      panic!("playlist média");
    };
    assert_eq!(variants.len(), 3);
    let picked = pick_variant(&variants).unwrap();
    assert_eq!(picked.uri.as_str(), "https://cdn.example/a/b/high/p.m3u8");
    assert_eq!(pick_variant(&[]), None);
  }

  #[test]
  fn variant_without_codecs_is_a_candidate() {
    let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000\nv.m3u8\n";
    let Playlist::Master(variants) = parse(master, &base()).unwrap() else {
      panic!("playlist média");
    };
    assert!(pick_variant(&variants).is_some());
  }

  #[test]
  fn segment_urls_round_trip_signed_urls() {
    let urls: Vec<String> = (0..371)
      .map(|i| {
        format!("https://cdn.example/t/{i:03}/data{i:03}.m4s?expires=1795580766&sig=AbC-1_2")
      })
      .collect();
    let packed = SegmentUrls::new(&urls);
    assert_eq!(packed.len(), 371);
    for (i, url) in urls.iter().enumerate() {
      assert_eq!(packed.get(i).as_deref(), Some(url.as_str()));
    }
  }

  #[test]
  fn segment_urls_handle_one_url_identical_urls_and_none() {
    let one = SegmentUrls::new(&["https://a.example/x.m4s".to_string()]);
    assert_eq!(one.get(0).as_deref(), Some("https://a.example/x.m4s"));
    let same = SegmentUrls::new(&["https://a/x".to_string(), "https://a/x".to_string()]);
    assert_eq!(same.get(1).as_deref(), Some("https://a/x"));
    let prefix_of_other =
      SegmentUrls::new(&["https://a/x".to_string(), "https://a/x1".to_string()]);
    assert_eq!(prefix_of_other.get(0).as_deref(), Some("https://a/x"));
    assert_eq!(prefix_of_other.get(1).as_deref(), Some("https://a/x1"));
    assert!(SegmentUrls::new(&[]).is_empty());
  }

  #[test]
  fn expires_is_read_from_the_query() {
    assert_eq!(
      expires_of("https://cdn.example/p.m3u8?a=1&expires=1795580766&b=2"),
      Some(1_795_580_766)
    );
    assert_eq!(expires_of("https://cdn.example/p.m3u8?a=1"), None);
    assert_eq!(expires_of("pas une url"), None);
  }

  #[test]
  fn absurd_segment_durations_are_rejected() {
    for value in ["inf", "NaN", "1e400", "1e20", "-1", "86401"] {
      let text = format!(
        "#EXTM3U
#EXT-X-MAP:URI=\"i.mp4\"
#EXTINF:{value},
s.m4s
#EXT-X-ENDLIST
"
      );
      assert_eq!(
        parse(&text, &base()),
        Err(HlsError::Invalid("EXTINF hors limites")),
        "{value}"
      );
    }
  }

  #[test]
  fn a_truncated_body_is_an_error_not_an_end_of_segment() {
    let mut full = ExactLength {
      inner: Box::new(io::Cursor::new(vec![7u8; 10])),
      expected: 10,
      received: 0,
    };
    let mut out = Vec::new();
    full.read_to_end(&mut out).expect("corps complet");
    assert_eq!(out.len(), 10);
    let mut cut = ExactLength {
      inner: Box::new(io::Cursor::new(vec![7u8; 6])),
      expected: 10,
      received: 0,
    };
    let err = cut.read_to_end(&mut Vec::new()).expect_err("corps tronqué");
    assert_eq!(err.kind(), io::ErrorKind::Other);
  }

  #[test]
  fn a_network_failure_on_a_cached_url_asks_for_a_fresh_resolution() {
    let net = || HlsError::Network("réponse 502".into());
    assert_eq!(stale_if_cached(net(), false), HlsError::Expired);
    assert_eq!(stale_if_cached(net(), true), net());
    assert_eq!(
      stale_if_cached(HlsError::Encrypted, false),
      HlsError::Encrypted
    );
  }
}
