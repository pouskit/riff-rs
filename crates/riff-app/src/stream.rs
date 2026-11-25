//! Source audio d'un titre : fichier, ou flux lu à travers le core sans serveur HTTP.

use crate::backend::{Backend, BackendError, local_url};
use crate::hls::{self, HlsError};
use crate::user_data::{Track, is_youtube_id};
use bytes::Bytes;
use iced::futures::StreamExt;
use iced::futures::stream::BoxStream;
use riff_audio::{RangeFetcher, Source};
use riff_core::core::Core;
use riff_core::dispatch;
use riff_core::error::CoreError;
use riff_core::logging::redact_urls;
use riff_core::reply::{HeaderLite, Method, Params, Reply};
use riff_core::util::clean_track_id;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use url::Url;

const PROXY_PATH: &str = "/api/audio-proxy";
/// Délai de résolution d'un flux (`playTrack`).
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(22);

fn header_of<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
  headers
    .iter()
    .find(|(key, _)| key.eq_ignore_ascii_case(name))
    .map(|(_, value)| value.as_str())
}

/// Taille totale annoncée par une réponse : `Content-Range`, sinon `Content-Length` d'une réponse entière.
fn total_len(status: u16, headers: &[(String, String)]) -> Option<u64> {
  let from_range = header_of(headers, "content-range")
    .and_then(|range| range.rsplit('/').next())
    .and_then(|total| total.trim().parse().ok());
  from_range
    .or_else(|| {
      (status == 200)
        .then(|| header_of(headers, "content-length"))
        .flatten()
        .and_then(|length| length.trim().parse().ok())
    })
    .filter(|len| *len > 0)
}

/// Lit un corps en flux depuis un fil bloquant.
struct StreamReader {
  body: BoxStream<'static, Result<Bytes, CoreError>>,
  chunk: Bytes,
  skip: u64,
  runtime: Handle,
  /// Octets reçus du corps et taille annoncée par `Content-Length`, pour le journal.
  received: u64,
  expected: Option<u64>,
}

impl Read for StreamReader {
  fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
    while self.chunk.is_empty() {
      match self.runtime.block_on(self.body.next()) {
        None => {
          if self
            .expected
            .is_some_and(|expected| self.received < expected)
          {
            tracing::warn!(
              received = self.received,
              expected = self.expected,
              "flux terminé avant la taille annoncée"
            );
          }
          return Ok(0);
        }
        Some(Err(err)) => {
          tracing::warn!(
            error = %redact_urls(&err.to_string()),
            received = self.received,
            "lecture du flux interrompue"
          );
          return Err(io::Error::other(err.to_string()));
        }
        Some(Ok(mut chunk)) => {
          self.received += chunk.len() as u64;
          let dropped = self.skip.min(chunk.len() as u64);
          self.skip -= dropped;
          self.chunk = chunk.split_off(dropped as usize);
        }
      }
    }
    let count = buffer.len().min(self.chunk.len());
    buffer[..count].copy_from_slice(&self.chunk.split_to(count));
    Ok(count)
  }
}

/// Lecteur d'une réponse du core à partir de l'octet `start`.
pub(crate) fn reply_reader(
  reply: Reply,
  start: u64,
  runtime: &Handle,
) -> io::Result<Box<dyn Read + Send>> {
  let status = reply.status();
  if !(200..300).contains(&status) {
    return Err(io::Error::other(format!("réponse {status}")));
  }
  // Un serveur qui ignore la plage renvoie 200 et le fichier depuis le début.
  let skip = if status == 206 { 0 } else { start };
  match reply {
    Reply::Stream { body, headers, .. } => Ok(Box::new(StreamReader {
      body,
      chunk: Bytes::new(),
      skip,
      runtime: runtime.clone(),
      received: 0,
      expected: header_of(&headers, "content-length").and_then(|length| length.trim().parse().ok()),
    })),
    Reply::Bytes { body, .. } => {
      let mut cursor = Cursor::new(body);
      cursor.set_position(skip);
      Ok(Box::new(cursor))
    }
    Reply::File { path, range, .. } => {
      let mut file = File::open(path)?;
      match range {
        Some((first, last)) => {
          file.seek(SeekFrom::Start(first))?;
          Ok(Box::new(file.take(last - first + 1)))
        }
        None => {
          file.seek(SeekFrom::Start(skip))?;
          Ok(Box::new(file))
        }
      }
    }
    Reply::Json { .. } | Reply::Text { .. } => Err(io::Error::other("réponse sans octets")),
  }
}

/// Lecture par plages à travers `audio-proxy` du core.
pub struct CoreRangeFetcher {
  core: Arc<Core>,
  url: String,
  len: Option<u64>,
  runtime: Handle,
}

fn proxy_params(url: &str, range: Option<String>) -> Params {
  let headers = range.map_or_else(Vec::new, |range| vec![("range".to_string(), range)]);
  Params {
    query: HashMap::from([("url".to_string(), url.to_string())]),
    headers: HeaderLite::new(headers),
    ..Params::default()
  }
}

pub(crate) async fn proxy_call(
  core: &Arc<Core>,
  method: Method,
  url: &str,
  range: Option<String>,
) -> Result<Reply, String> {
  match dispatch::call(core, method, PROXY_PATH, proxy_params(url, range)).await {
    Some(Ok(reply)) => Ok(reply),
    Some(Err(err)) => Err(err.to_string()),
    None => Err("route audio-proxy absente".to_string()),
  }
}

/// Taille annoncée par l'URL de YouTube : évite un aller-retour.
fn len_from_url(url: &str) -> Option<u64> {
  Url::parse(url)
    .ok()?
    .query_pairs()
    .find(|(key, _)| key == "clen")
    .and_then(|(_, value)| value.parse().ok())
}

impl CoreRangeFetcher {
  /// Prépare la lecture de `url` et cherche sa taille, nécessaire aux déplacements.
  pub async fn open(core: Arc<Core>, url: String) -> Self {
    let mut len = len_from_url(&url);
    if len.is_none()
      && let Ok(Reply::Stream {
        status, headers, ..
      }) = proxy_call(&core, Method::Head, &url, None).await
    {
      len = total_len(status, &headers);
    }
    if len.is_none()
      && let Ok(Reply::Stream {
        status, headers, ..
      }) = proxy_call(&core, Method::Get, &url, Some("bytes=0-0".to_string())).await
    {
      len = total_len(status, &headers);
    }
    Self {
      core,
      url,
      len,
      runtime: Handle::current(),
    }
  }
}

impl RangeFetcher for CoreRangeFetcher {
  fn fetch(&self, start: u64, end: Option<u64>) -> io::Result<Box<dyn Read + Send>> {
    let range = match end {
      Some(end) => format!("bytes={start}-{}", end.saturating_sub(1)),
      None => format!("bytes={start}-"),
    };
    let reply = self
      .runtime
      .block_on(proxy_call(&self.core, Method::Get, &self.url, Some(range)))
      .map_err(|err| {
        tracing::warn!(error = %redact_urls(&err), "ouverture du flux refusée");
        io::Error::other(err)
      })?;
    tracing::info!(start, status = reply.status(), "flux : plage ouverte");
    reply_reader(reply, start, &self.runtime).inspect_err(|err| {
      tracing::warn!(error = %redact_urls(&err.to_string()), "réponse du flux inutilisable");
    })
  }

  fn len(&self) -> Option<u64> {
    self.len
  }
}

/// Chemin du fichier d'un titre local : chemin simple ou URL `file:`.
#[must_use]
pub fn local_path(track: &Track) -> Option<PathBuf> {
  let raw = track
    .path
    .as_deref()
    .or_else(|| track.extra.get("filePath").and_then(Value::as_str))
    .or(track.url.as_deref())
    .filter(|raw| !raw.is_empty())?;
  if raw.starts_with("file:") {
    return Url::parse(raw).ok()?.to_file_path().ok();
  }
  (!raw.contains("://")).then(|| PathBuf::from(raw))
}

/// Réglages de la résolution d'un flux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveOptions {
  pub platform: String,
  pub quality: String,
  pub force_yt_dlp: bool,
  /// Ignore le cache et toute résolution en cours, sans écarter la résolution directe (`fresh=1`).
  pub fresh: bool,
}

/// Plateforme qui sert à résoudre un titre. La plateforme du titre prime : une playlist mêle YouTube et
/// SoundCloud, et la session SoundCloud ne doit pas faire passer pour SoundCloud un titre YouTube Music,
/// Deezer ou iTunes (URL vide, lien `youtu.be`).
#[must_use]
pub fn effective_platform(track: &Track, session_platform: &str) -> String {
  match track
    .platform
    .as_deref()
    .filter(|platform| !platform.is_empty())
  {
    Some(platform) => platform.to_string(),
    None
      if session_platform == "soundcloud"
        && !track
          .url
          .as_deref()
          .unwrap_or_default()
          .contains("youtube.com") =>
    {
      "soundcloud".to_string()
    }
    None => session_platform.to_string(),
  }
}

/// Paramètres de `/api/stream-info`.
#[must_use]
pub fn stream_info_query(track: &Track, options: &ResolveOptions) -> HashMap<String, String> {
  let text = |value: &Option<String>| value.clone().unwrap_or_default();
  let url = text(&track.url);
  let platform = effective_platform(track, &options.platform);
  let search = track
    .search_query
    .clone()
    .filter(|query| !query.is_empty())
    .unwrap_or_else(
      || match track.artist.as_deref().filter(|artist| !artist.is_empty()) {
        Some(artist) => format!("{artist} - {}", track.title.as_deref().unwrap_or("")),
        None => track.title.clone().unwrap_or_default(),
      },
    );
  let mut query = HashMap::from([
    ("url".to_string(), url),
    ("id".to_string(), text(&track.id)),
    ("platform".to_string(), platform),
    ("title".to_string(), text(&track.title)),
    ("artist".to_string(), text(&track.artist)),
    (
      "duration".to_string(),
      track.duration.unwrap_or(0.0).to_string(),
    ),
    ("quality".to_string(), options.quality.clone()),
    ("q".to_string(), search),
  ]);
  if options.force_yt_dlp {
    query.insert("forceYtDlp".to_string(), "1".to_string());
  }
  if options.fresh {
    query.insert("fresh".to_string(), "1".to_string());
  }
  query
}

fn text_error(err: BackendError) -> String {
  err.to_string()
}

async fn source_of_local_url(backend: &Backend, url: &str) -> Result<Source, String> {
  match backend.resolve_url(url).await.map_err(text_error)? {
    Reply::File { path, .. } => Ok(Source::File(path)),
    other => Err(format!("réponse {}", other.status())),
  }
}

/// Titre sans identifiant YouTube (album, mix, tendance) : le core ferait la course avec SoundCloud et
/// pourrait rendre un flux SoundCloud pour un titre YouTube. On cherche d'abord l'identifiant YouTube.
#[must_use]
pub fn needs_youtube_lookup(track: &Track, platform: &str) -> bool {
  let soundcloud = track.platform.as_deref().unwrap_or(platform) == "soundcloud"
    || track
      .url
      .as_deref()
      .is_some_and(|url| url.contains("soundcloud.com"));
  let known = track.id.as_deref().is_some_and(is_youtube_id)
    || track
      .url
      .as_deref()
      .is_some_and(|url| url.contains("youtube.com/watch"));
  !soundcloud
    && !known
    && track
      .title
      .as_deref()
      .is_some_and(|title| !title.is_empty())
}

/// Premier résultat YouTube Music du titre (`preloadNextTrackStream`).
async fn youtube_id_of(backend: &Backend, track: &Track) -> Option<String> {
  let query = match track.artist.as_deref().filter(|artist| !artist.is_empty()) {
    Some(artist) => format!("{artist} - {}", track.title.as_deref().unwrap_or("")),
    None => track.title.clone().unwrap_or_default(),
  };
  let params = HashMap::from([
    ("q".to_string(), query),
    ("platform".to_string(), "ytmusic".to_string()),
    ("limit".to_string(), "1".to_string()),
  ]);
  let Reply::Json { body, .. } = backend.call("/api/search", params, None).await.ok()? else {
    return None;
  };
  body["tracks"][0]["id"]
    .as_str()
    .filter(|id| is_youtube_id(id))
    .map(str::to_string)
}

async fn stream_info(
  backend: &Backend,
  track: &Track,
  options: &ResolveOptions,
) -> Result<Value, String> {
  match backend
    .call("/api/stream-info", stream_info_query(track, options), None)
    .await
    .map_err(text_error)?
  {
    Reply::Json { body, .. } => Ok(body),
    _ => {
      tracing::warn!("résolution : réponse de stream-info sans JSON");
      Err("Could not load audio stream".to_string())
    }
  }
}

/// Résout la source d'un titre (`playTrack`).
///
/// # Errors
/// Message montré à l'utilisateur.
pub async fn resolve_source(
  backend: Arc<Backend>,
  track: Track,
  options: ResolveOptions,
) -> Result<Source, String> {
  if track.is_local() {
    return local_path(&track)
      .map(Source::File)
      .ok_or_else(|| "File missing from library".to_string());
  }
  if track.is_cached
    && let Some(id) = track.id.as_deref().filter(|id| !id.is_empty())
  {
    let query = HashMap::from([("id".to_string(), clean_track_id(id))]);
    if let Ok(Reply::File { path, .. }) = backend.call("/api/local-track", query, None).await {
      return Ok(Source::File(path));
    }
  }
  let mut lookup = track.clone();
  if !options.force_yt_dlp
    && needs_youtube_lookup(&track, &options.platform)
    && let Some(id) = youtube_id_of(&backend, &track).await
  {
    lookup.url = Some(format!("https://www.youtube.com/watch?v={id}"));
    lookup.id = Some(id);
  }
  let mut options = options;
  loop {
    let body = stream_info(&backend, &lookup, &options).await?;
    match source_of_body(&backend, &body, options.fresh).await {
      // L'URL signée du cache a expiré : une seule résolution neuve.
      Err(Failure::Expired) if !options.fresh => options.fresh = true,
      Err(Failure::Expired) => return Err("Could not load audio stream".to_string()),
      Err(Failure::Message(message)) => return Err(message),
      Ok(source) => return Ok(source),
    }
  }
}

enum Failure {
  Expired,
  Message(String),
}

impl From<String> for Failure {
  fn from(message: String) -> Self {
    Self::Message(message)
  }
}

fn hls_failure(err: HlsError) -> Failure {
  match err {
    HlsError::Expired => Failure::Expired,
    HlsError::Encrypted => Failure::Message(err.to_string()),
    other => {
      tracing::warn!(error = %redact_urls(&other.to_string()), "lecture HLS impossible");
      Failure::Message("Could not load audio stream".to_string())
    }
  }
}

/// Source décrite par une réponse de `stream-info`.
async fn source_of_body(backend: &Backend, body: &Value, fresh: bool) -> Result<Source, Failure> {
  if body["success"].as_bool() != Some(true) {
    return Err(Failure::Message(
      body["error"]
        .as_str()
        .unwrap_or("Failed to extract audio stream")
        .to_string(),
    ));
  }
  let stream_url = body["streamUrl"].as_str().unwrap_or("");
  if body["isLocal"].as_bool() == Some(true) {
    return Ok(source_of_local_url(backend, &local_url(stream_url)).await?);
  }
  let raw = body["rawStreamUrl"]
    .as_str()
    .filter(|raw| !raw.is_empty())
    .ok_or_else(|| {
      tracing::warn!("résolution : rawStreamUrl absent de la réponse");
      Failure::Message("Could not load audio stream".to_string())
    })?;
  if body["isHls"].as_bool() == Some(true) {
    return hls::open(&backend.core, raw, fresh)
      .await
      .map_err(hls_failure);
  }
  let fetcher = CoreRangeFetcher::open(Arc::clone(&backend.core), raw.to_string()).await;
  Ok(Source::Http(Arc::new(fetcher)))
}

#[cfg(test)]
mod tests {
  use super::*;
  use iced::futures::stream;
  use std::io::Write as _;

  fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .unwrap()
  }

  fn read_all(mut reader: Box<dyn Read + Send>) -> Vec<u8> {
    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();
    out
  }

  fn stream_reply(status: u16, chunks: Vec<&'static [u8]>) -> Reply {
    Reply::Stream {
      status,
      headers: Vec::new(),
      body: stream::iter(
        chunks
          .into_iter()
          .map(|chunk| Ok(Bytes::from_static(chunk))),
      )
      .boxed(),
    }
  }

  #[test]
  fn a_stream_reply_reads_across_chunks() {
    let runtime = runtime();
    let reader = reply_reader(
      stream_reply(206, vec![b"ab", b"", b"cd"]),
      10,
      runtime.handle(),
    )
    .unwrap();
    assert_eq!(read_all(reader), b"abcd");
  }

  #[test]
  fn a_server_that_ignores_the_range_is_skipped_to_the_offset() {
    let runtime = runtime();
    let reader =
      reply_reader(stream_reply(200, vec![b"ab", b"cdef"]), 3, runtime.handle()).unwrap();
    assert_eq!(read_all(reader), b"def");
  }

  #[test]
  fn an_error_inside_the_stream_reaches_the_reader() {
    let runtime = runtime();
    let reply = Reply::Stream {
      status: 200,
      headers: Vec::new(),
      body: stream::iter(vec![Err(CoreError::Upstream {
        host: "h".to_string(),
        status: None,
      })])
      .boxed(),
    };
    let mut reader = reply_reader(reply, 0, runtime.handle()).unwrap();
    assert!(reader.read(&mut [0; 4]).is_err());
  }

  #[test]
  fn a_file_reply_honours_its_range() {
    let dir = crate::test_temp::TempDir::new("app-stream");
    let path = dir.join("a.bin");
    File::create(&path)
      .unwrap()
      .write_all(b"0123456789")
      .unwrap();
    let runtime = runtime();
    let ranged = Reply::File {
      path: path.clone(),
      mime: String::new(),
      range: Some((2, 4)),
    };
    assert_eq!(
      read_all(reply_reader(ranged, 2, runtime.handle()).unwrap()),
      b"234"
    );
    let whole = Reply::File {
      path,
      mime: String::new(),
      range: None,
    };
    assert_eq!(
      read_all(reply_reader(whole, 7, runtime.handle()).unwrap()),
      b"789"
    );
  }

  #[test]
  fn an_error_status_or_a_text_reply_gives_an_error() {
    let runtime = runtime();
    assert!(reply_reader(stream_reply(502, vec![]), 0, runtime.handle()).is_err());
    let text = Reply::Text {
      status: 200,
      text: String::new(),
    };
    assert!(reply_reader(text, 0, runtime.handle()).is_err());
  }

  #[test]
  fn the_total_size_comes_from_the_range_then_the_length() {
    let headers = |list: &[(&str, &str)]| -> Vec<(String, String)> {
      list
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
    };
    assert_eq!(
      total_len(
        206,
        &headers(&[("Content-Range", "bytes 0-0/1234"), ("content-length", "1")])
      ),
      Some(1234)
    );
    assert_eq!(
      total_len(200, &headers(&[("content-length", "99")])),
      Some(99)
    );
    assert_eq!(total_len(206, &headers(&[("content-length", "1")])), None);
    assert_eq!(total_len(200, &headers(&[("content-length", "0")])), None);
    assert_eq!(
      total_len(206, &headers(&[("content-range", "bytes 0-0/*")])),
      None
    );
  }

  #[test]
  fn the_youtube_url_gives_the_size_without_a_request() {
    assert_eq!(
      len_from_url("https://x.googlevideo.com/videoplayback?a=1&clen=4242&b=2"),
      Some(4242)
    );
    assert_eq!(len_from_url("https://x/y"), None);
  }

  #[test]
  fn a_local_track_opens_a_path_or_a_file_url() {
    let path = Track {
      path: Some("D:\\m\\a.mp3".into()),
      ..Track::default()
    };
    assert_eq!(local_path(&path), Some(PathBuf::from("D:\\m\\a.mp3")));
    // `to_file_path` dépend de l'OS : un chemin de lecteur n'a de sens que sous Windows.
    let (file_url, file_path) = if cfg!(windows) {
      ("file:///D:/m/a%20b.mp3", "D:\\m\\a b.mp3")
    } else {
      ("file:///m/a%20b.mp3", "/m/a b.mp3")
    };
    let url = Track {
      url: Some(file_url.into()),
      ..Track::default()
    };
    assert_eq!(local_path(&url), Some(PathBuf::from(file_path)));
    let web = Track {
      url: Some("https://x/a.mp3".into()),
      ..Track::default()
    };
    assert_eq!(local_path(&web), None);
    assert_eq!(local_path(&Track::default()), None);
  }

  fn options(platform: &str, force: bool) -> ResolveOptions {
    ResolveOptions {
      platform: platform.to_string(),
      quality: "medium".to_string(),
      force_yt_dlp: force,
      fresh: false,
    }
  }

  #[test]
  fn the_stream_query_matches_riff() {
    let track = Track {
      id: Some("a".into()),
      url: Some("https://www.youtube.com/watch?v=a".into()),
      title: Some("T".into()),
      artist: Some("A".into()),
      duration: Some(12.0),
      platform: Some("ytmusic".into()),
      ..Track::default()
    };
    let query = stream_info_query(&track, &options("soundcloud", false));
    assert_eq!(query["platform"], "ytmusic");
    assert_eq!(query["q"], "A - T");
    assert_eq!(query["duration"], "12");
    assert_eq!(query["quality"], "medium");
    assert!(!query.contains_key("forceYtDlp"));
    assert_eq!(
      stream_info_query(&track, &options("ytmusic", true))["forceYtDlp"],
      "1"
    );
  }

  #[test]
  fn a_fresh_resolution_writes_fresh_and_not_force_yt_dlp() {
    let track = Track {
      id: Some("a".into()),
      ..Track::default()
    };
    let fresh = ResolveOptions {
      fresh: true,
      ..options("ytmusic", false)
    };
    let query = stream_info_query(&track, &fresh);
    assert_eq!(query["fresh"], "1");
    assert!(!query.contains_key("forceYtDlp"));
    let plain = stream_info_query(&track, &options("ytmusic", false));
    assert!(!plain.contains_key("fresh"));
  }

  #[test]
  fn a_soundcloud_session_keeps_soundcloud_for_non_youtube_tracks() {
    let track = Track {
      id: Some("a".into()),
      search_query: Some("cherche".into()),
      ..Track::default()
    };
    let query = stream_info_query(&track, &options("soundcloud", false));
    assert_eq!(query["platform"], "soundcloud");
    assert_eq!(query["q"], "cherche");
  }

  #[test]
  fn a_track_keeps_its_own_platform_in_a_soundcloud_session() {
    let ytmusic = Track {
      id: Some("alb_1".into()),
      platform: Some("ytmusic".into()),
      ..Track::default()
    };
    let query = stream_info_query(&ytmusic, &options("soundcloud", false));
    assert_eq!(query["platform"], "ytmusic");
    let short_link = Track {
      id: Some("dQw4w9WgXcQ".into()),
      url: Some("https://youtu.be/dQw4w9WgXcQ".into()),
      platform: Some("youtube".into()),
      ..Track::default()
    };
    let query = stream_info_query(&short_link, &options("soundcloud", false));
    assert_eq!(query["platform"], "youtube");
    let sc = Track {
      id: Some("123456789".into()),
      url: Some("https://soundcloud.com/a/b".into()),
      platform: Some("soundcloud".into()),
      ..Track::default()
    };
    let query = stream_info_query(&sc, &options("ytmusic", false));
    assert_eq!(query["platform"], "soundcloud");
    assert_eq!(query["id"], "123456789");
    assert_eq!(query["url"], "https://soundcloud.com/a/b");
  }

  #[test]
  fn tracks_without_a_youtube_id_are_looked_up_first() {
    let titled = |id: &str| Track {
      id: Some(id.to_string()),
      title: Some("T".to_string()),
      ..Track::default()
    };
    assert!(needs_youtube_lookup(&titled("alb_123_0"), "ytmusic"));
    assert!(needs_youtube_lookup(&titled("cand_abc"), "ytmusic"));
    assert!(!needs_youtube_lookup(&titled("dQw4w9WgXcQ"), "ytmusic"));
    assert!(
      !needs_youtube_lookup(&titled("alb_123_0"), "soundcloud"),
      "session SoundCloud"
    );
    let on_soundcloud = Track {
      platform: Some("soundcloud".to_string()),
      ..titled("42")
    };
    assert!(!needs_youtube_lookup(&on_soundcloud, "ytmusic"));
    let by_url = Track {
      url: Some("https://www.youtube.com/watch?v=abc".to_string()),
      ..titled("x")
    };
    assert!(!needs_youtube_lookup(&by_url, "ytmusic"));
    assert!(!needs_youtube_lookup(
      &Track {
        title: None,
        ..titled("x")
      },
      "ytmusic"
    ));
  }

  #[test]
  fn a_local_url_rewrites_the_old_server_address() {
    assert_eq!(
      local_url("http://127.0.0.1:38472/api/x?a=1"),
      "riff://local/api/x?a=1"
    );
    assert_eq!(local_url("http://localhost:1/api/x"), "riff://local/api/x");
    assert_eq!(local_url("riff://local/api/x"), "riff://local/api/x");
    assert_eq!(local_url("https://exemple.org/a"), "https://exemple.org/a");
  }
}
