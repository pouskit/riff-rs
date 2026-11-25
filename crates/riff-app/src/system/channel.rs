//! Canal local de l'utilisateur : verrou d'instance unique, « montre-toi » et flux des paroles.
//! Tube nommé sous Windows (limité à l'utilisateur, clients distants refusés), socket Unix ailleurs. Jamais de TCP.

use super::platform;
use super::protocol::{
  LyricsState, ProtocolError, READ_TIMEOUT, Reply, Request, encode_line, read_request,
};
use interprocess::local_socket::tokio::{Stream, prelude::*};
use interprocess::local_socket::{ListenerOptions, Name};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::task::JoinHandle;

/// Variable d'environnement qui remplace le nom du canal (essais : jamais le canal d'une vraie instance).
pub const NAME_OVERRIDE_ENV: &str = "RIFF_IPC_NAME";
/// Clients servis en même temps. Les suivants sont déconnectés.
const MAX_CLIENTS: usize = 16;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Nom du canal : `RIFF_IPC_NAME` s'il est défini, sinon `riff-rs-<identifiant de l'utilisateur>`.
#[must_use]
pub fn channel_name() -> String {
  let overridden = std::env::var(NAME_OVERRIDE_ENV).ok();
  name_for(&platform::user_id(), overridden.as_deref())
}

/// Nom construit à partir de l'utilisateur et d'un remplacement éventuel. Seuls `[A-Za-z0-9_-]` sont gardés.
#[must_use]
pub fn name_for(user: &str, overridden: Option<&str>) -> String {
  let clean = |text: &str| -> String {
    text
      .chars()
      .map(|c| {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
          c
        } else {
          '_'
        }
      })
      .take(96)
      .collect()
  };
  match overridden.map(str::trim).filter(|name| !name.is_empty()) {
    Some(name) => clean(name),
    None => format!("riff-rs-{}", clean(user)),
  }
}

fn local_name(label: &str) -> io::Result<Name<'static>> {
  #[cfg(windows)]
  {
    use interprocess::local_socket::GenericNamespaced;
    label.to_owned().to_ns_name::<GenericNamespaced>()
  }
  #[cfg(unix)]
  {
    use interprocess::local_socket::GenericFilePath;
    platform::unix::socket_path(label)?.to_fs_name::<GenericFilePath>()
  }
}

/// Moitié d'un flux partagé : `&Stream` lit et écrit, ce qui remplace `split`, déprécié.
pub struct Half(Arc<Stream>);

fn halves(stream: Stream) -> (Half, Half) {
  let shared = Arc::new(stream);
  (Half(Arc::clone(&shared)), Half(shared))
}

impl tokio::io::AsyncRead for Half {
  fn poll_read(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
    buf: &mut tokio::io::ReadBuf<'_>,
  ) -> std::task::Poll<io::Result<()>> {
    std::pin::Pin::new(&mut &*self.0).poll_read(cx, buf)
  }
}

impl tokio::io::AsyncWrite for Half {
  fn poll_write(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
    buf: &[u8],
  ) -> std::task::Poll<io::Result<usize>> {
    std::pin::Pin::new(&mut &*self.0).poll_write(cx, buf)
  }

  fn poll_flush(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<io::Result<()>> {
    std::pin::Pin::new(&mut &*self.0).poll_flush(cx)
  }

  fn poll_shutdown(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<io::Result<()>> {
    std::pin::Pin::new(&mut &*self.0).poll_shutdown(cx)
  }
}

/// Résultat de la tentative de prise du canal.
pub enum Claim {
  /// Cette instance est la première : elle sert le canal.
  Primary(Server),
  /// Une instance répond déjà sur le canal.
  Existing,
}

/// Prend le canal : se connecte d'abord, crée ensuite. Sous Windows, une création qui échoue pour cause de
/// premier tube déjà pris ressemble à un refus d'accès, d'où cette séquence et la seconde connexion.
pub async fn claim(label: &str, shows: mpsc::UnboundedSender<Vec<PathBuf>>) -> io::Result<Claim> {
  claim_with(label, shows, READ_TIMEOUT).await
}

async fn claim_with(
  label: &str,
  shows: mpsc::UnboundedSender<Vec<PathBuf>>,
  deadline: Duration,
) -> io::Result<Claim> {
  if connect(label).await.is_ok() {
    return Ok(Claim::Existing);
  }
  match create_listener(label) {
    Ok(listener) => Ok(Claim::Primary(Server::start(listener, shows, deadline))),
    Err(created) => {
      if connect(label).await.is_ok() {
        Ok(Claim::Existing)
      } else {
        Err(created)
      }
    }
  }
}

async fn connect(label: &str) -> io::Result<Stream> {
  let name = local_name(label)?;
  tokio::time::timeout(CONNECT_TIMEOUT, Stream::connect(name))
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "channel connection timed out"))?
}

fn create_listener(label: &str) -> io::Result<interprocess::local_socket::tokio::Listener> {
  let options = ListenerOptions::new().name(local_name(label)?);
  #[cfg(windows)]
  let options = {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    let sddl = platform::windows::owner_only_sddl(&platform::windows::user_id()?);
    let wide = widestring::U16CString::from_str(sddl).map_err(io::Error::other)?;
    options.security_descriptor(SecurityDescriptor::deserialize(&wide)?)
  };
  #[cfg(unix)]
  let options = options.try_overwrite(true);
  options.create_tokio()
}

/// Serveur du canal : ses tâches s'arrêtent quand il est détruit.
pub struct Server {
  lyrics: watch::Sender<LyricsState>,
  task: JoinHandle<()>,
}

struct Shared {
  shows: mpsc::UnboundedSender<Vec<PathBuf>>,
  lyrics: watch::Receiver<LyricsState>,
  deadline: Duration,
}

impl Server {
  fn start(
    listener: interprocess::local_socket::tokio::Listener,
    shows: mpsc::UnboundedSender<Vec<PathBuf>>,
    deadline: Duration,
  ) -> Self {
    let (lyrics, receiver) = watch::channel(LyricsState::default());
    let shared = Arc::new(Shared {
      shows,
      lyrics: receiver,
      deadline,
    });
    Self {
      lyrics,
      task: tokio::spawn(accept_loop(listener, shared)),
    }
  }

  /// Publie l'état des paroles ; les clients connectés le reçoivent s'il a changé.
  pub fn publish(&self, state: LyricsState) {
    self.lyrics.send_if_modified(|current| {
      let changed = *current != state;
      if changed {
        *current = state;
      }
      changed
    });
  }
}

impl Drop for Server {
  fn drop(&mut self) {
    self.task.abort();
  }
}

async fn accept_loop(listener: interprocess::local_socket::tokio::Listener, shared: Arc<Shared>) {
  let slots = Arc::new(Semaphore::new(MAX_CLIENTS));
  loop {
    let stream = match listener.accept().await {
      Ok(stream) => stream,
      Err(error) => {
        tracing::warn!(%error, "connexion refusée par le canal local");
        tokio::time::sleep(Duration::from_millis(100)).await;
        continue;
      }
    };
    let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
      tracing::warn!("trop de clients sur le canal local, connexion fermée");
      continue;
    };
    let shared = Arc::clone(&shared);
    tokio::spawn(async move {
      handle(stream, &shared).await;
      drop(slot);
    });
  }
}

async fn handle(stream: Stream, shared: &Shared) {
  let (mut reader, mut writer) = halves(stream);
  match read_request(&mut reader, shared.deadline).await {
    Ok(Request::Show { paths }) => {
      let reply = if shared
        .shows
        .send(paths.into_iter().map(PathBuf::from).collect())
        .is_ok()
      {
        Reply::ok()
      } else {
        Reply {
          ok: false,
          error: Some("application closing".into()),
        }
      };
      write_line(&mut writer, &reply).await;
    }
    Ok(Request::Lyrics) => stream_lyrics(&mut reader, &mut writer, shared.lyrics.clone()).await,
    Err(error) => {
      tracing::debug!(%error, "requête du canal local refusée");
      write_line(&mut writer, &Reply::refused(&error)).await;
    }
  }
}

async fn write_line<W, T>(writer: &mut W, value: &T) -> bool
where
  W: tokio::io::AsyncWrite + Unpin,
  T: serde::Serialize,
{
  let line = encode_line(value);
  let write = async {
    writer.write_all(&line).await?;
    writer.flush().await
  };
  matches!(tokio::time::timeout(WRITE_TIMEOUT, write).await, Ok(Ok(())))
}

/// Envoie l'état courant, puis chaque changement, jusqu'à la déconnexion du client ou la fin du serveur.
async fn stream_lyrics<R, W>(
  reader: &mut R,
  writer: &mut W,
  mut updates: watch::Receiver<LyricsState>,
) where
  R: tokio::io::AsyncRead + Unpin,
  W: tokio::io::AsyncWrite + Unpin,
{
  let first = updates.borrow_and_update().clone();
  if !write_line(writer, &first).await {
    return;
  }
  let mut probe = [0u8; 64];
  loop {
    tokio::select! {
      changed = updates.changed() => {
        if changed.is_err() {
          return;
        }
        let state = updates.borrow_and_update().clone();
        if !write_line(writer, &state).await {
          return;
        }
      }
      // Le client n'envoie plus rien après sa requête : toute lecture qui rend la main signifie qu'il est parti.
      read = reader.read(&mut probe) => {
        if !matches!(read, Ok(n) if n > 0) {
          return;
        }
      }
    }
  }
}

/// Erreur d'un client du canal.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
  #[error("no running Riff on the local channel: {0}")]
  Unreachable(io::Error),
  #[error("refused by Riff: {0}")]
  Refused(String),
  #[error("protocol error: {0}")]
  Protocol(#[from] ProtocolError),
  #[error("io error: {0}")]
  Io(io::Error),
}

/// Seconde instance : demande à la première de se montrer et d'ouvrir les fichiers donnés.
pub async fn notify_show(label: &str, paths: Vec<String>) -> Result<(), ClientError> {
  let stream = connect(label).await.map_err(ClientError::Unreachable)?;
  let (reader, mut writer) = halves(stream);
  if !write_line(&mut writer, &Request::Show { paths }).await {
    return Err(ClientError::Io(io::Error::other("write failed")));
  }
  let mut lines = BufReader::new(reader.take(super::protocol::MAX_LINE as u64 + 1));
  let mut reply = String::new();
  tokio::time::timeout(READ_TIMEOUT, lines.read_line(&mut reply))
    .await
    .map_err(|_| ClientError::Protocol(ProtocolError::Timeout))?
    .map_err(ClientError::Io)?;
  let reply: Reply = serde_json::from_str(reply.trim_end())
    .map_err(|error| ProtocolError::Invalid(error.to_string()))?;
  if reply.ok {
    Ok(())
  } else {
    Err(ClientError::Refused(reply.error.unwrap_or_default()))
  }
}

/// Abonné au flux des paroles (`riff lyrics`).
pub struct LyricsClient {
  lines: BufReader<Half>,
  _writer: Half,
}

impl LyricsClient {
  /// Se connecte et s'abonne. Le premier état arrive aussitôt.
  pub async fn connect(label: &str) -> Result<Self, ClientError> {
    let stream = connect(label).await.map_err(ClientError::Unreachable)?;
    let (reader, mut writer) = halves(stream);
    if !write_line(&mut writer, &Request::Lyrics).await {
      return Err(ClientError::Io(io::Error::other("write failed")));
    }
    Ok(Self {
      lines: BufReader::new(reader),
      _writer: writer,
    })
  }

  /// Prochain état, `None` quand le serveur ferme le canal.
  pub async fn next(&mut self) -> Result<Option<LyricsState>, ClientError> {
    let mut line = Vec::new();
    let mut limited = (&mut self.lines).take(super::protocol::MAX_LINE as u64 + 1);
    let read = limited
      .read_until(b'\n', &mut line)
      .await
      .map_err(ClientError::Io)?;
    if read == 0 {
      return Ok(None);
    }
    if line.last() != Some(&b'\n') {
      return Err(
        if line.len() > super::protocol::MAX_LINE {
          ProtocolError::TooLarge
        } else {
          ProtocolError::Closed
        }
        .into(),
      );
    }
    serde_json::from_slice(&line)
      .map(Some)
      .map_err(|error| ProtocolError::Invalid(error.to_string()).into())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicU32, Ordering};

  const SHORT: Duration = Duration::from_millis(150);

  fn unique() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
      "riff-test-{}-{}",
      std::process::id(),
      NEXT.fetch_add(1, Ordering::Relaxed)
    )
  }

  async fn primary(
    label: &str,
    deadline: Duration,
  ) -> (Server, mpsc::UnboundedReceiver<Vec<PathBuf>>) {
    let (tx, rx) = mpsc::unbounded_channel();
    match claim_with(label, tx, deadline).await.expect("claim") {
      Claim::Primary(server) => (server, rx),
      Claim::Existing => panic!("le nom {label} était déjà pris"),
    }
  }

  #[test]
  fn name_keeps_the_user_and_cleans_odd_characters() {
    assert_eq!(name_for("S-1-5-21-1", None), "riff-rs-S-1-5-21-1");
    assert_eq!(name_for("a b\\c/é", None), "riff-rs-a_b_c__");
  }

  #[test]
  fn name_override_replaces_the_default_and_ignores_blanks() {
    assert_eq!(name_for("u", Some(" test.1 ")), "test_1");
    assert_eq!(name_for("u", Some("  ")), "riff-rs-u");
  }

  #[tokio::test]
  async fn first_claim_is_primary_and_second_sees_it() {
    let label = unique();
    let (_server, _rx) = primary(&label, SHORT).await;
    let (tx, _rx2) = mpsc::unbounded_channel();
    assert!(matches!(
      claim_with(&label, tx, SHORT).await,
      Ok(Claim::Existing)
    ));
  }

  #[tokio::test]
  async fn the_channel_is_free_again_after_the_server_is_dropped() {
    let label = unique();
    let (server, _rx) = primary(&label, SHORT).await;
    drop(server);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (tx, _rx2) = mpsc::unbounded_channel();
    assert!(matches!(
      claim_with(&label, tx, SHORT).await,
      Ok(Claim::Primary(_))
    ));
  }

  #[tokio::test]
  async fn show_reaches_the_primary_with_the_paths() {
    let label = unique();
    let (_server, mut rx) = primary(&label, SHORT).await;
    notify_show(&label, vec!["a.mp3".into(), "b.flac".into()])
      .await
      .expect("show");
    assert_eq!(
      rx.recv().await,
      Some(vec![PathBuf::from("a.mp3"), PathBuf::from("b.flac")])
    );
  }

  async fn raw_exchange(label: &str, bytes: &[u8]) -> Option<Reply> {
    let stream = connect(label).await.expect("connect");
    let (reader, mut writer) = halves(stream);
    let _ = writer.write_all(bytes).await;
    let _ = writer.flush().await;
    let mut lines = BufReader::new(reader);
    let mut reply = String::new();
    let read = tokio::time::timeout(Duration::from_secs(3), lines.read_line(&mut reply))
      .await
      .ok()?
      .ok()?;
    if read == 0 {
      None
    } else {
      serde_json::from_str(reply.trim_end()).ok()
    }
  }

  #[tokio::test]
  async fn a_too_large_message_is_refused_and_the_server_keeps_working() {
    let label = unique();
    let (_server, mut rx) = primary(&label, SHORT).await;
    let mut bytes = vec![b'a'; 70 * 1024];
    bytes.push(b'\n');
    let reply = raw_exchange(&label, &bytes).await.expect("reply");
    assert!(!reply.ok);
    assert!(rx.try_recv().is_err());
    notify_show(&label, vec![]).await.expect("still serving");
  }

  #[tokio::test]
  async fn a_line_that_is_not_json_is_refused_and_the_server_keeps_working() {
    let label = unique();
    let (_server, mut rx) = primary(&label, SHORT).await;
    let reply = raw_exchange(&label, b"not json\n").await.expect("reply");
    assert!(!reply.ok);
    assert!(rx.try_recv().is_err());
    notify_show(&label, vec![]).await.expect("still serving");
  }

  #[tokio::test]
  async fn a_silent_client_is_disconnected_after_the_deadline() {
    let label = unique();
    let (_server, _rx) = primary(&label, SHORT).await;
    let stream = connect(&label).await.expect("connect");
    let (reader, _writer) = halves(stream);
    let mut lines = BufReader::new(reader);
    let mut reply = String::new();
    let started = std::time::Instant::now();
    let read = tokio::time::timeout(Duration::from_secs(3), lines.read_line(&mut reply))
      .await
      .expect("server must answer")
      .expect("read");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(read == 0 || reply.contains("\"ok\":false"), "{reply:?}");
  }

  #[tokio::test]
  async fn lyrics_client_gets_the_current_state_then_updates_then_the_end() {
    let label = unique();
    let (server, _rx) = primary(&label, SHORT).await;
    server.publish(LyricsState {
      title: "T".into(),
      artist: "A".into(),
      line: "one".into(),
      playing: true,
    });
    let mut client = LyricsClient::connect(&label).await.expect("connect");
    let first = client.next().await.expect("read").expect("state");
    assert_eq!(first.line, "one");
    server.publish(LyricsState {
      line: "two".into(),
      ..first
    });
    assert_eq!(
      client.next().await.expect("read").expect("state").line,
      "two"
    );
    drop(server);
    assert!(client.next().await.expect("clean end").is_none());
  }

  #[tokio::test]
  async fn notify_without_a_server_reports_unreachable() {
    let error = notify_show(&unique(), vec![])
      .await
      .expect_err("nobody listens");
    assert!(matches!(error, ClientError::Unreachable(_)), "{error}");
  }

  #[cfg(windows)]
  #[tokio::test]
  async fn the_pipe_grants_access_to_the_current_user_only() {
    let label = unique();
    let (_server, _rx) = primary(&label, SHORT).await;
    let dacl = platform::windows::dacl_of_pipe(&label).expect("security descriptor");
    // Comparaison binaire : la conversion SDDL réécrit certains SID en alias (`LA`, ...).
    // GENERIC_ALL est traduit par le noyau en FILE_ALL_ACCESS.
    assert!(dacl.protected, "{dacl:?}");
    assert_eq!(dacl.ace_count, 1, "{dacl:?}");
    assert_eq!(dacl.ace_type, 0, "{dacl:?}");
    assert_eq!(dacl.ace_flags, 0, "{dacl:?}");
    assert_eq!(
      dacl.mask,
      windows::Win32::Storage::FileSystem::FILE_ALL_ACCESS.0,
      "{dacl:?}"
    );
    assert!(dacl.sid_is_current_user, "{dacl:?}");
  }
}
