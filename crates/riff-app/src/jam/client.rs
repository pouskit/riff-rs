//! Tâche réseau de l'invité : admission, réception de l'état, reconnexion.

use super::protocol::{self, ClientMsg, DecodeError, HostMsg, MAX_FRAME, RateLimit, Refusal};
use super::sync::{self, ClockSync};
use super::transport::{Dialer, Target, Transport, TransportError};
use super::{
  AbortOnDrop, CLOSE_TIMEOUT, EndCause, HANDSHAKE_TIMEOUT, JamEvent, JamOut,
  MAX_HOST_FRAMES_PER_SEC, PEER_SILENCE, PING_BURST, PING_EVERY, RECONNECT_DELAYS, millis_since,
};
use std::future::Future;
use std::pin::pin;
use std::time::{Duration, Instant};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// Délais et bornes de la tâche de l'invité ; les tests les raccourcissent.
#[derive(Debug, Clone)]
pub struct ClientConfig {
  pub handshake_timeout: Duration,
  pub peer_silence: Duration,
  pub close_timeout: Duration,
  pub max_frames_per_sec: u32,
  pub ping_every: Duration,
  pub ping_burst: [Duration; 3],
  pub reconnect_delays: [Duration; 3],
}

impl Default for ClientConfig {
  fn default() -> Self {
    Self {
      handshake_timeout: HANDSHAKE_TIMEOUT,
      peer_silence: PEER_SILENCE,
      close_timeout: CLOSE_TIMEOUT,
      max_frames_per_sec: MAX_HOST_FRAMES_PER_SEC,
      ping_every: PING_EVERY,
      ping_burst: PING_BURST,
      reconnect_delays: RECONNECT_DELAYS,
    }
  }
}

type Writer = Box<dyn AsyncWrite + Send + Unpin>;

/// Connexion admise : écriture directe, lecture par une tâche qui rend les messages décodés.
struct Session {
  writer: Writer,
  frames: mpsc::UnboundedReceiver<HostMsg>,
  _reader: AbortOnDrop,
}

/// Réponse `welcome`, avec l'instant local de réception.
struct Welcome {
  you: u32,
  participants: Vec<protocol::Participant>,
  queue: protocol::QueueState,
  playback: protocol::PlaybackState,
}

enum Failure {
  Refused(Refusal),
  Transport(String),
  Lost,
}

enum Outcome {
  Stopped,
  HostEnded,
  Lost,
}

pub(super) async fn run(
  transport: Transport,
  ticket: String,
  name: String,
  cfg: ClientConfig,
  events: mpsc::UnboundedSender<JamEvent>,
  mut commands: mpsc::UnboundedReceiver<JamOut>,
) -> EndCause {
  let cause = match transport.parse(&ticket) {
    Err(_) => EndCause::InvalidTicket,
    Ok(target) => match transport.dialer().await {
      Err(err) => EndCause::Transport(err.to_string()),
      Ok(dialer) => {
        let mut guest = Guest {
          dialer,
          target,
          name: protocol::sanitize_name(&name),
          cfg,
          epoch: Instant::now(),
          clock: ClockSync::default(),
          events: events.clone(),
        };
        let cause = guest.session(&mut commands).await;
        guest.dialer.close().await;
        cause
      }
    },
  };
  let _ = events.send(JamEvent::Ended(cause.clone()));
  cause
}

struct Guest {
  dialer: Dialer,
  target: Target,
  name: String,
  cfg: ClientConfig,
  epoch: Instant,
  clock: ClockSync,
  events: mpsc::UnboundedSender<JamEvent>,
}

/// Attend `fut` ; `None` si l'application demande l'arrêt (ou disparaît) entre-temps. Les autres
/// commandes sont jetées : l'application les bloque déjà hors connexion.
async fn interruptible<F: Future>(
  commands: &mut mpsc::UnboundedReceiver<JamOut>,
  fut: F,
) -> Option<F::Output> {
  let mut fut = pin!(fut);
  loop {
    tokio::select! {
      output = &mut fut => return Some(output),
      command = commands.recv() => match command {
        Some(JamOut::Stop) | None => return None,
        Some(_) => {}
      },
    }
  }
}

async fn send(writer: &mut Writer, message: &ClientMsg, timeout: Duration) -> bool {
  let Ok(body) = protocol::encode(message) else {
    return false;
  };
  matches!(
    tokio::time::timeout(timeout, protocol::write_frame(writer, &body)).await,
    Ok(Ok(()))
  )
}

/// Lit les messages de l'hôte : débit, silence, version ; la fin du canal signale la perte du lien.
async fn read_loop(
  mut reader: Box<dyn tokio::io::AsyncRead + Send + Unpin>,
  frames: mpsc::UnboundedSender<HostMsg>,
  cfg: ClientConfig,
) {
  let mut limit = RateLimit::new(cfg.max_frames_per_sec);
  loop {
    let frame = tokio::time::timeout(
      cfg.peer_silence,
      protocol::read_frame(&mut reader, MAX_FRAME),
    )
    .await;
    let Ok(Ok(Some(body))) = frame else {
      return;
    };
    if !limit.allow(Instant::now()) {
      tracing::debug!("jam: host over the frame rate limit");
      return;
    }
    match protocol::decode::<HostMsg>(&body) {
      Ok(HostMsg::Unknown) => tracing::debug!("jam: unknown message ignored"),
      Ok(message) => {
        if frames.send(message).is_err() {
          return;
        }
      }
      Err(DecodeError::Version(version)) => {
        tracing::debug!(version, "jam: unsupported protocol version");
        return;
      }
      Err(DecodeError::Json(err)) => {
        tracing::debug!(%err, "jam: invalid frame");
        return;
      }
    }
  }
}

impl Guest {
  fn emit(&self, event: JamEvent) {
    let _ = self.events.send(event);
  }

  fn guest_ms(&self) -> u64 {
    millis_since(self.epoch, Instant::now())
  }

  /// Connexion, `hello`, puis `welcome` ou `refused`.
  async fn establish(&mut self) -> Result<(Session, Welcome), Failure> {
    let link = self
      .dialer
      .connect(&self.target)
      .await
      .map_err(|err| match err {
        TransportError::Closed => Failure::Lost,
        other => Failure::Transport(other.to_string()),
      })?;
    let mut writer = link.writer;
    let (frames_tx, mut frames) = mpsc::unbounded_channel();
    let reader = AbortOnDrop(tokio::spawn(read_loop(
      link.reader,
      frames_tx,
      self.cfg.clone(),
    )));
    let hello = ClientMsg::Hello {
      room: self.target.room.to_hex(),
      name: self.name.clone(),
      app: concat!("riff-rs/", env!("CARGO_PKG_VERSION")).to_string(),
    };
    if !send(&mut writer, &hello, self.cfg.handshake_timeout).await {
      return Err(Failure::Lost);
    }
    let reply = tokio::time::timeout(self.cfg.handshake_timeout, frames.recv()).await;
    match reply {
      Ok(Some(HostMsg::Welcome {
        you,
        host_ms,
        participants,
        queue,
        playback,
      })) => {
        self.clock.seed(host_ms, self.guest_ms());
        Ok((
          Session {
            writer,
            frames,
            _reader: reader,
          },
          Welcome {
            you,
            participants,
            queue,
            playback,
          },
        ))
      }
      Ok(Some(HostMsg::Refused { reason })) => Err(Failure::Refused(reason)),
      _ => Err(Failure::Lost),
    }
  }

  fn anchor(&self, at_ms: u64) -> Instant {
    sync::host_to_local(
      at_ms,
      self.clock.offset().unwrap_or(0),
      self.epoch,
      Instant::now(),
    )
  }

  /// Première connexion, puis sessions successives tant que la reconnexion réussit.
  async fn session(&mut self, commands: &mut mpsc::UnboundedReceiver<JamOut>) -> EndCause {
    let first = match interruptible(commands, self.establish()).await {
      None => return EndCause::Stopped,
      Some(Ok(established)) => established,
      Some(Err(failure)) => return failure.into_cause(),
    };
    let mut current = first;
    loop {
      let (session, welcome) = current;
      let anchor = self.anchor(welcome.playback.at_ms);
      self.emit(JamEvent::Joined {
        you: welcome.you,
        participants: welcome.participants,
        queue: welcome.queue,
        playback: welcome.playback,
        anchor,
      });
      match self.run_session(session, commands).await {
        Outcome::Stopped => return EndCause::Stopped,
        Outcome::HostEnded => return EndCause::HostEnded,
        Outcome::Lost => {}
      }
      current = match self.reconnect(commands).await {
        Ok(established) => established,
        Err(cause) => return cause,
      };
    }
  }

  /// Trois essais espacés par `reconnect_delays` ; `Err` porte la cause de fin.
  async fn reconnect(
    &mut self,
    commands: &mut mpsc::UnboundedReceiver<JamOut>,
  ) -> Result<(Session, Welcome), EndCause> {
    for (index, delay) in self.cfg.reconnect_delays.into_iter().enumerate() {
      let attempt = u8::try_from(index + 1).unwrap_or(u8::MAX);
      self.emit(JamEvent::Reconnecting { attempt });
      if interruptible(commands, tokio::time::sleep(delay))
        .await
        .is_none()
      {
        return Err(EndCause::Stopped);
      }
      match interruptible(commands, self.establish()).await {
        None => return Err(EndCause::Stopped),
        Some(Ok(established)) => return Ok(established),
        Some(Err(Failure::Refused(reason))) => return Err(EndCause::Refused(reason)),
        Some(Err(_)) => {}
      }
    }
    Err(EndCause::Lost)
  }

  async fn run_session(
    &mut self,
    mut session: Session,
    commands: &mut mpsc::UnboundedReceiver<JamOut>,
  ) -> Outcome {
    let started = tokio::time::Instant::now();
    let mut pings_sent = 0usize;
    let mut next_ping = started + self.cfg.ping_burst[0];
    let timeout = self.cfg.handshake_timeout;
    loop {
      tokio::select! {
        message = session.frames.recv() => match message {
          None => return Outcome::Lost,
          Some(HostMsg::End { .. }) => return Outcome::HostEnded,
          Some(message) => self.on_host_message(message),
        },
        command = commands.recv() => {
          let message = match command {
            Some(JamOut::Command { id, op }) => ClientMsg::Cmd { id, op },
            Some(JamOut::Report { uid }) => ClientMsg::Report {
              kind: protocol::ReportKind::ResolveFailed,
              uid,
            },
            Some(JamOut::Stop) | None => {
              let close = async {
                send(&mut session.writer, &ClientMsg::Bye, timeout).await;
                let _ = session.writer.shutdown().await;
              };
              let _ = tokio::time::timeout(self.cfg.close_timeout, close).await;
              return Outcome::Stopped;
            }
            Some(_) => continue,
          };
          if !send(&mut session.writer, &message, timeout).await {
            return Outcome::Lost;
          }
        },
        () = tokio::time::sleep_until(next_ping) => {
          let ping = ClientMsg::Ping { t0: self.guest_ms() };
          if !send(&mut session.writer, &ping, timeout).await {
            return Outcome::Lost;
          }
          pings_sent += 1;
          next_ping = match self.cfg.ping_burst.get(pings_sent) {
            Some(offset) => started + *offset,
            None => tokio::time::Instant::now() + self.cfg.ping_every,
          };
        },
      }
    }
  }

  fn on_host_message(&mut self, message: HostMsg) {
    match message {
      HostMsg::Queue(queue) => self.emit(JamEvent::Queue(queue)),
      HostMsg::Playback(state) => {
        let anchor = self.anchor(state.at_ms);
        self.emit(JamEvent::Playback { state, anchor });
      }
      HostMsg::Participants { list } => self.emit(JamEvent::Participants(list)),
      HostMsg::Notice(notice) => self.emit(JamEvent::Notice(notice)),
      HostMsg::Rejected { id, reason } => self.emit(JamEvent::Rejected { id, reason }),
      HostMsg::Pong { t0, t1 } => {
        let t2 = self.guest_ms();
        self.clock.add(t0, t1, t2);
      }
      HostMsg::Welcome { .. }
      | HostMsg::Refused { .. }
      | HostMsg::End { .. }
      | HostMsg::Unknown => {}
    }
  }
}

impl Failure {
  fn into_cause(self) -> EndCause {
    match self {
      Self::Refused(reason) => EndCause::Refused(reason),
      Self::Transport(err) => EndCause::Transport(err),
      Self::Lost => EndCause::Lost,
    }
  }
}
