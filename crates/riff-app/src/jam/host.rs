//! Tâche réseau de l'hôte : admission, diffusion, commandes des invités.

use super::protocol::{
  self, ClientMsg, DecodeError, EndReason, HostMsg, MAX_FRAME, MAX_HELLO_FRAME, Notice, NoticeKind,
  PROTOCOL_VERSION, Participant, PlaybackState, QueueState, RateLimit, Refusal,
};
use super::transport::{Link, Listener, Room, Transport};
use super::{
  ALONE_GRACE, AbortOnDrop, CLOSE_TIMEOUT, EndCause, GUEST_CHANNEL, HANDSHAKE_TIMEOUT, JamEvent,
  JamOut, MAX_FRAMES_PER_SEC, MAX_NOTICES_PER_SEC, MAX_PARTICIPANTS, MAX_PENDING, PEER_SILENCE,
  PLAYBACK_WINDOW, QUEUE_WINDOW, REFUSE_DELAY, millis_since,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

/// Délais et bornes de la tâche de l'hôte ; les tests les raccourcissent.
#[derive(Debug, Clone)]
pub struct HostConfig {
  pub handshake_timeout: Duration,
  pub peer_silence: Duration,
  pub refuse_delay: Duration,
  pub close_timeout: Duration,
  pub max_frames_per_sec: u32,
  pub guest_channel: usize,
  pub max_participants: usize,
  pub max_pending: usize,
  /// Attente avant de fermer un jam qui a eu des invités et n'en a plus.
  pub alone_grace: Duration,
}

impl Default for HostConfig {
  fn default() -> Self {
    Self {
      handshake_timeout: HANDSHAKE_TIMEOUT,
      peer_silence: PEER_SILENCE,
      refuse_delay: REFUSE_DELAY,
      close_timeout: CLOSE_TIMEOUT,
      max_frames_per_sec: MAX_FRAMES_PER_SEC,
      guest_channel: GUEST_CHANNEL,
      max_participants: MAX_PARTICIPANTS,
      max_pending: MAX_PENDING,
      alone_grace: ALONE_GRACE,
    }
  }
}

type Outbox = mpsc::Sender<Arc<[u8]>>;
type Reader = Box<dyn AsyncRead + Send + Unpin>;
type Writer = Box<dyn AsyncWrite + Send + Unpin>;

/// Messages des sous-tâches vers la boucle principale.
enum Inner {
  /// Poignée de main réussie ; la boucle décide de l'admission.
  Admit(Box<Admission>),
  Msg(u32, ClientMsg),
  Gone(u32),
}

struct Admission {
  reader: Reader,
  writer: Writer,
  peer: String,
  name: String,
}

struct Guest {
  name: String,
  outbox: Outbox,
  writer: JoinHandle<()>,
  _reader: AbortOnDrop,
}

struct Host {
  cfg: HostConfig,
  epoch: Instant,
  events: mpsc::UnboundedSender<JamEvent>,
  inner: mpsc::UnboundedSender<Inner>,
  name: String,
  next_id: u32,
  guests: HashMap<u32, Guest>,
  /// Vrai dès qu'un invité a été admis : un jam qui n'en a jamais eu n'est jamais fermé pour solitude.
  ever_guest: bool,
  /// Instant où la liste d'invités est devenue vide (après au moins un invité).
  alone_since: Option<Instant>,
  queue: QueueState,
  playback: PlaybackState,
  /// États en attente de diffusion : seule la dernière valeur compte.
  pending: Pending,
  notices: RateLimit,
}

/// Regroupement des états diffusés : une trame par fenêtre au plus, la première sans attente.
#[derive(Default)]
struct Pending {
  queue: Option<QueueState>,
  playback: Option<PlaybackState>,
  queue_sent: Option<Instant>,
  playback_sent: Option<Instant>,
}

impl Pending {
  /// Instant où une trame en attente peut partir.
  fn deadline(&self) -> Option<Instant> {
    let queue = self.queue.as_ref().map(|_| {
      self
        .queue_sent
        .map_or_else(Instant::now, |at| at + QUEUE_WINDOW)
    });
    let playback = self.playback.as_ref().map(|_| {
      self
        .playback_sent
        .map_or_else(Instant::now, |at| at + PLAYBACK_WINDOW)
    });
    match (queue, playback) {
      (Some(a), Some(b)) => Some(a.min(b)),
      (a, b) => a.or(b),
    }
  }
}

pub(super) async fn run(
  transport: Transport,
  name: String,
  cfg: HostConfig,
  events: mpsc::UnboundedSender<JamEvent>,
  mut commands: mpsc::UnboundedReceiver<JamOut>,
) -> EndCause {
  let end = |cause: EndCause| {
    let _ = events.send(JamEvent::Ended(cause.clone()));
    cause
  };
  let room = match Room::random() {
    Ok(room) => room,
    Err(err) => return end(EndCause::Transport(err.to_string())),
  };
  // `listen` peut attendre le relais plusieurs secondes : un arrêt demandé pendant ce temps coupe tout de suite.
  // Les autres commandes sont rejouées une fois l'hôte prêt.
  let mut early = Vec::new();
  let listening = transport.listen(room);
  tokio::pin!(listening);
  let mut listener = loop {
    tokio::select! {
      result = &mut listening => match result {
        Ok(listener) => break listener,
        Err(err) => return end(EndCause::Transport(err.to_string())),
      },
      out = commands.recv() => match out {
        Some(JamOut::Stop) | None => return end(EndCause::Stopped),
        Some(out) => early.push(out),
      },
    }
  };
  let _ = events.send(JamEvent::Hosting {
    ticket: listener.ticket().to_string(),
  });
  if !listener.relay_reached() {
    let _ = events.send(JamEvent::RelayUnreachable);
  }
  let mut codes = listener.codes();
  let (inner_tx, mut inner_rx) = mpsc::unbounded_channel();
  let mut pending = JoinSet::new();
  let mut host = Host {
    cfg: cfg.clone(),
    epoch: Instant::now(),
    events: events.clone(),
    inner: inner_tx,
    name: protocol::sanitize_name(&name),
    next_id: 1,
    guests: HashMap::new(),
    ever_guest: false,
    alone_since: None,
    queue: QueueState::default(),
    playback: PlaybackState::default(),
    pending: Pending::default(),
    notices: RateLimit::new(MAX_NOTICES_PER_SEC),
  };
  for out in early {
    host.on_out(out);
  }
  let cause = loop {
    let flush_in = host
      .pending
      .deadline()
      .map(|at| at.saturating_duration_since(Instant::now()));
    let alone_in = host
      .alone_deadline()
      .map(|at| at.saturating_duration_since(Instant::now()));
    tokio::select! {
      () = tokio::time::sleep(flush_in.unwrap_or_default()), if flush_in.is_some() => host.flush(),
      () = tokio::time::sleep(alone_in.unwrap_or_default()), if alone_in.is_some() => {
        // Une arrivée déjà postée (ou un départ) passe avant la décision.
        while let Ok(message) = inner_rx.try_recv() {
          host.on_inner(message);
        }
        if host.alone_deadline().is_some_and(|at| at <= Instant::now()) {
          if pending.is_empty() {
            break EndCause::Alone;
          }
          // Poignée de main en cours : l'invité qui arrive n'est pas coupé, on réarme.
          host.alone_since = Some(Instant::now());
        }
      },
      incoming = listener.accept() => match incoming {
        None => break EndCause::Transport("closed".to_string()),
        Some(Ok(link)) => {
          if pending.len() >= cfg.max_pending {
            tracing::debug!(peer = %link.peer, "jam: too many pending handshakes");
          } else {
            pending.spawn(handshake(link, room, cfg.clone(), host.inner.clone()));
          }
        }
        Some(Err(err)) => tracing::debug!(%err, "jam: accept failed"),
      },
      ticket = codes.changed() => {
        let _ = events.send(JamEvent::CodeChanged { ticket });
      },
      Some(message) = inner_rx.recv() => host.on_inner(message),
      Some(_) = pending.join_next(), if !pending.is_empty() => {},
      out = commands.recv() => match out {
        Some(JamOut::Stop) | None => break EndCause::Stopped,
        Some(out) => host.on_out(out),
      },
    }
  };
  host.shutdown().await;
  listener_close(listener, cfg.close_timeout).await;
  end(cause)
}

async fn listener_close(listener: Listener, timeout: Duration) {
  if tokio::time::timeout(timeout, listener.close())
    .await
    .is_err()
  {
    tracing::debug!("jam: listener close timed out");
  }
}

/// Lit le `hello`, contrôle version et salle, puis remet le lien à la boucle principale.
async fn handshake(link: Link, room: Room, cfg: HostConfig, inner: mpsc::UnboundedSender<Inner>) {
  let Link {
    mut reader,
    mut writer,
    peer,
  } = link;
  let Ok(Ok(Some(body))) = tokio::time::timeout(
    cfg.handshake_timeout,
    protocol::read_frame(&mut reader, MAX_HELLO_FRAME),
  )
  .await
  else {
    return;
  };
  let hello = match protocol::decode::<ClientMsg>(&body) {
    Ok(ClientMsg::Hello { room, name, .. }) => (room, name),
    Err(DecodeError::Version(version)) => {
      let version = u32::try_from(version).unwrap_or(PROTOCOL_VERSION);
      return refuse(&mut writer, Refusal::Version, version).await;
    }
    _ => return,
  };
  let valid = Room::from_hex(&hello.0).is_some_and(|given| given.ct_eq(&room));
  if !valid {
    refuse(&mut writer, Refusal::Room, PROTOCOL_VERSION).await;
    tokio::time::sleep(cfg.refuse_delay).await;
    return;
  }
  tracing::debug!(%peer, "jam: handshake done");
  let admission = Admission {
    reader,
    writer,
    peer,
    name: protocol::sanitize_name(&hello.1),
  };
  let _ = inner.send(Inner::Admit(Box::new(admission)));
}

/// `version` est celle de l'enveloppe : pour `Refusal::Version`, celle du `hello`, que le pair sait décoder.
async fn refuse(writer: &mut Writer, reason: Refusal, version: u32) {
  if let Ok(body) = protocol::encode_versioned(&HostMsg::Refused { reason }, version) {
    let _ = protocol::write_frame(writer, &body).await;
  }
  let _ = writer.shutdown().await;
}

/// Écrit les trames de la file jusqu'à sa fermeture, puis ferme le sens d'écriture.
async fn write_loop(mut writer: Writer, mut outbox: mpsc::Receiver<Arc<[u8]>>) {
  while let Some(body) = outbox.recv().await {
    if protocol::write_frame(&mut writer, &body).await.is_err() {
      break;
    }
  }
  let _ = writer.shutdown().await;
}

/// Lit les trames d'un invité : débit, silence, `ping` répondu sur place, le reste vers la boucle principale.
async fn guest_reader(
  id: u32,
  mut reader: Reader,
  outbox: Outbox,
  inner: mpsc::UnboundedSender<Inner>,
  cfg: HostConfig,
  epoch: Instant,
) {
  let mut limit = RateLimit::new(cfg.max_frames_per_sec);
  loop {
    let frame = tokio::time::timeout(
      cfg.peer_silence,
      protocol::read_frame(&mut reader, MAX_FRAME),
    )
    .await;
    let Ok(Ok(Some(body))) = frame else {
      break;
    };
    if !limit.allow(Instant::now()) {
      tracing::debug!(id, "jam: guest over the frame rate limit");
      break;
    }
    match protocol::decode::<ClientMsg>(&body) {
      Ok(ClientMsg::Ping { t0 }) => {
        let pong = HostMsg::Pong {
          t0,
          t1: millis_since(epoch, Instant::now()),
        };
        let sent = protocol::encode(&pong).is_ok_and(|body| outbox.try_send(body).is_ok());
        if !sent {
          break;
        }
      }
      Ok(ClientMsg::Unknown) => tracing::debug!(id, "jam: unknown message ignored"),
      Ok(message) => {
        if inner.send(Inner::Msg(id, message)).is_err() {
          return;
        }
      }
      Err(DecodeError::Version(_)) => {
        if let Ok(body) = protocol::encode(&HostMsg::Refused {
          reason: Refusal::Version,
        }) {
          let _ = outbox.try_send(body);
        }
        break;
      }
      Err(DecodeError::Json(err)) => {
        tracing::debug!(id, %err, "jam: invalid frame");
        break;
      }
    }
  }
  let _ = inner.send(Inner::Gone(id));
}

impl Host {
  fn participants(&self) -> Vec<Participant> {
    let mut list = vec![Participant {
      id: 0,
      name: self.name.clone(),
      host: true,
    }];
    let mut guests: Vec<_> = self.guests.iter().collect();
    guests.sort_by_key(|(id, _)| **id);
    list.extend(guests.into_iter().map(|(id, guest)| Participant {
      id: *id,
      name: guest.name.clone(),
      host: false,
    }));
    list
  }

  fn on_inner(&mut self, message: Inner) {
    match message {
      Inner::Admit(admission) => self.admit(*admission),
      Inner::Msg(from, message) => self.on_message(from, message),
      Inner::Gone(id) => self.remove_guest(id),
    }
  }

  fn on_message(&mut self, from: u32, message: ClientMsg) {
    if !self.guests.contains_key(&from) {
      return;
    }
    match message {
      ClientMsg::Cmd { id, op } => {
        let _ = self.events.send(JamEvent::GuestOp { from, id, op });
      }
      ClientMsg::Report { uid, .. } => {
        let _ = self.events.send(JamEvent::GuestReport { from, uid });
      }
      ClientMsg::Bye => self.remove_guest(from),
      ClientMsg::Hello { .. } | ClientMsg::Ping { .. } | ClientMsg::Unknown => {}
    }
  }

  fn admit(&mut self, admission: Admission) {
    let Admission {
      reader,
      writer,
      peer,
      name,
    } = admission;
    let (outbox, receiver) = mpsc::channel(self.cfg.guest_channel);
    let writer = tokio::spawn(write_loop(writer, receiver));
    if self.guests.len() + 1 >= self.cfg.max_participants {
      if let Ok(body) = protocol::encode(&HostMsg::Refused {
        reason: Refusal::Full,
      }) {
        let _ = outbox.try_send(body);
      }
      return;
    }
    let id = self.next_id;
    self.next_id += 1;
    let name = protocol::unique_name(
      &name,
      self
        .guests
        .values()
        .map(|guest| guest.name.as_str())
        .chain(std::iter::once(self.name.as_str()))
        .collect::<Vec<_>>()
        .into_iter(),
    );
    tracing::debug!(%peer, id, "jam: guest admitted");
    let reader = AbortOnDrop(tokio::spawn(guest_reader(
      id,
      reader,
      outbox.clone(),
      self.inner.clone(),
      self.cfg.clone(),
      self.epoch,
    )));
    self.ever_guest = true;
    self.alone_since = None;
    self.guests.insert(
      id,
      Guest {
        name: name.clone(),
        outbox,
        writer,
        _reader: reader,
      },
    );
    let participants = self.participants();
    let welcome = HostMsg::Welcome {
      you: id,
      host_ms: millis_since(self.epoch, Instant::now()),
      participants: participants.clone(),
      queue: self.queue.clone(),
      playback: self.playback.clone(),
    };
    self.send_to(id, &welcome);
    self.announce(
      Some(id),
      participants,
      Notice {
        kind: NoticeKind::Joined,
        by: id,
        name,
        title: None,
        count: None,
      },
    );
  }

  /// Échéance de fermeture de l'hôte resté seul.
  fn alone_deadline(&self) -> Option<Instant> {
    self.alone_since.map(|at| at + self.cfg.alone_grace)
  }

  fn remove_guest(&mut self, id: u32) {
    let Some(guest) = self.guests.remove(&id) else {
      return;
    };
    tracing::debug!(id, "jam: guest left");
    if self.guests.is_empty() && self.ever_guest && self.alone_since.is_none() {
      self.alone_since = Some(Instant::now());
    }
    // Un pair qui ne lit plus bloquerait l'écriture : on l'annule, ce qui lâche le flux et ferme la connexion.
    guest.writer.abort();
    let participants = self.participants();
    self.announce(
      None,
      participants,
      Notice {
        kind: NoticeKind::Left,
        by: id,
        name: guest.name,
        title: None,
        count: None,
      },
    );
  }

  /// Arrivée ou départ : liste et notice vers les invités (sauf `except`, déjà informé par `welcome`) et vers
  /// l'application.
  fn announce(&mut self, except: Option<u32>, participants: Vec<Participant>, notice: Notice) {
    self.broadcast_except(
      except,
      &HostMsg::Participants {
        list: participants.clone(),
      },
    );
    self.broadcast_except(except, &HostMsg::Notice(notice.clone()));
    let _ = self.events.send(JamEvent::Participants(participants));
    let _ = self.events.send(JamEvent::Notice(notice));
  }

  fn on_out(&mut self, out: JamOut) {
    match out {
      JamOut::Queue(queue) => {
        self.queue = queue.clone();
        self.pending.queue = Some(queue);
        self.flush();
      }
      JamOut::Playback { mut state, at } => {
        state.at_ms = millis_since(self.epoch, at);
        self.playback = (*state).clone();
        self.pending.playback = Some(*state);
        self.flush();
      }
      JamOut::Notice(notice) => {
        if self.notices.allow(Instant::now()) {
          self.broadcast(&HostMsg::Notice(notice));
        } else {
          tracing::debug!("jam: notice over the budget dropped");
        }
      }
      JamOut::Reject { to, id, reason } => {
        self.send_to(to, &HostMsg::Rejected { id, reason });
      }
      JamOut::Command { .. } | JamOut::Report { .. } | JamOut::Stop => {}
    }
  }

  /// Diffuse les états en attente dont la fenêtre est écoulée ; la file passe avant l'état de lecture.
  fn flush(&mut self) {
    let now = Instant::now();
    if self
      .pending
      .queue_sent
      .is_none_or(|at| now >= at + QUEUE_WINDOW)
      && let Some(queue) = self.pending.queue.take()
    {
      self.pending.queue_sent = Some(now);
      self.broadcast(&HostMsg::Queue(queue));
    }
    if self
      .pending
      .playback_sent
      .is_none_or(|at| now >= at + PLAYBACK_WINDOW)
      && let Some(state) = self.pending.playback.take()
    {
      self.pending.playback_sent = Some(now);
      self.broadcast(&HostMsg::Playback(state));
    }
  }

  /// Encode une fois, puis clone vers chaque invité ; un invité dont la file est pleine est coupé.
  fn broadcast(&mut self, message: &HostMsg) {
    self.broadcast_except(None, message);
  }

  fn broadcast_except(&mut self, except: Option<u32>, message: &HostMsg) {
    let Some(body) = self.encode(message) else {
      return;
    };
    let dead: Vec<u32> = self
      .guests
      .iter()
      .filter(|(id, _)| Some(**id) != except)
      .filter(|(_, guest)| guest.outbox.try_send(body.clone()).is_err())
      .map(|(id, _)| *id)
      .collect();
    for id in dead {
      self.remove_guest(id);
    }
  }

  fn send_to(&mut self, id: u32, message: &HostMsg) {
    let Some(body) = self.encode(message) else {
      return;
    };
    let failed = self
      .guests
      .get(&id)
      .is_some_and(|guest| guest.outbox.try_send(body).is_err());
    if failed {
      self.remove_guest(id);
    }
  }

  fn encode(&self, message: &HostMsg) -> Option<Arc<[u8]>> {
    match protocol::encode(message) {
      Ok(body) if body.len() <= MAX_FRAME => Some(body),
      Ok(body) => {
        tracing::warn!(
          len = body.len(),
          "jam: message over the frame limit dropped"
        );
        None
      }
      Err(err) => {
        tracing::warn!(%err, "jam: message not encodable");
        None
      }
    }
  }

  /// Fin du jam : `end` à tous, puis attente bornée de l'écriture des files.
  async fn shutdown(&mut self) {
    if let Some(queue) = self.pending.queue.take() {
      self.broadcast(&HostMsg::Queue(queue));
    }
    if let Some(state) = self.pending.playback.take() {
      self.broadcast(&HostMsg::Playback(state));
    }
    self.broadcast(&HostMsg::End {
      reason: EndReason::Ended,
    });
    let writers: Vec<JoinHandle<()>> = self.guests.drain().map(|(_, guest)| guest.writer).collect();
    let drained = async {
      for writer in writers {
        let _ = writer.await;
      }
    };
    if tokio::time::timeout(self.cfg.close_timeout, drained)
      .await
      .is_err()
    {
      tracing::debug!("jam: guest writers did not drain in time");
    }
  }
}
