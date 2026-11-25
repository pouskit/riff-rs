//! Transport en mémoire pour les tests : aucun réseau, liens coupables à la demande.

use super::transport::{Link, Room, TransportError};
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

const TICKET_PREFIX: &str = "riffmem";
const PIPE_BYTES: usize = 64 * 1024;

#[derive(Debug, Default)]
struct HubInner {
  next: u64,
  listeners: HashMap<u64, mpsc::Sender<Link>>,
  switches: Vec<Arc<Switch>>,
}

/// Annuaire partagé des écoutes en mémoire.
#[derive(Debug, Clone, Default)]
pub struct Hub(Arc<Mutex<HubInner>>);

impl Hub {
  #[must_use]
  pub fn new() -> Self {
    Self::default()
  }

  fn lock(&self) -> std::sync::MutexGuard<'_, HubInner> {
    self
      .0
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner)
  }

  pub(super) fn listen(&self, room: Room) -> (String, MemoryListener) {
    let (sender, receiver) = mpsc::channel(16);
    let mut inner = self.lock();
    inner.next += 1;
    let id = inner.next;
    inner.listeners.insert(id, sender);
    let ticket = format!("{TICKET_PREFIX}{id}-{}", room.to_hex());
    (
      ticket,
      MemoryListener {
        id,
        hub: self.clone(),
        receiver,
      },
    )
  }

  pub(super) async fn connect(&self, listener: u64) -> Result<Link, TransportError> {
    let sender = self
      .lock()
      .listeners
      .get(&listener)
      .cloned()
      .ok_or(TransportError::Unreachable)?;
    let (guest, host) = tokio::io::duplex(PIPE_BYTES);
    let switch = Arc::new(Switch::default());
    self.lock().switches.push(switch.clone());
    let wrap = |stream, peer: &str| {
      let (reader, writer) = tokio::io::split(stream);
      Link {
        reader: Box::new(Cut::new(reader, switch.clone())),
        writer: Box::new(Cut::new(writer, switch.clone())),
        peer: peer.to_string(),
      }
    };
    let host_link = wrap(host, "mem-guest");
    let guest_link = wrap(guest, "mem-host");
    sender
      .send(host_link)
      .await
      .map_err(|_| TransportError::Unreachable)?;
    Ok(guest_link)
  }

  /// Coupe toutes les connexions ouvertes ; les écoutes restent.
  pub fn cut_all(&self) {
    let switches = std::mem::take(&mut self.lock().switches);
    for switch in switches {
      switch.cut();
    }
  }
}

#[derive(Debug)]
pub struct MemoryListener {
  id: u64,
  hub: Hub,
  receiver: mpsc::Receiver<Link>,
}

impl MemoryListener {
  pub(super) async fn accept(&mut self) -> Option<Result<Link, TransportError>> {
    self.receiver.recv().await.map(Ok)
  }

  pub(super) fn close(self) {}
}

impl Drop for MemoryListener {
  fn drop(&mut self) {
    self.hub.lock().listeners.remove(&self.id);
  }
}

/// `riffmem<écoute>-<salle en hexadécimal>`.
pub(super) fn parse_ticket(ticket: &str) -> Result<(u64, Room), TransportError> {
  let rest = ticket
    .strip_prefix(TICKET_PREFIX)
    .ok_or(TransportError::InvalidTicket)?;
  let (id, room) = rest.split_once('-').ok_or(TransportError::InvalidTicket)?;
  Ok((
    id.parse().map_err(|_| TransportError::InvalidTicket)?,
    Room::from_hex(room).ok_or(TransportError::InvalidTicket)?,
  ))
}

/// Interrupteur d'un lien : une fois coupé, toute lecture ou écriture échoue.
#[derive(Debug, Default)]
struct Switch {
  cut: AtomicBool,
  wakers: Mutex<Vec<Waker>>,
}

impl Switch {
  fn cut(&self) {
    self.cut.store(true, Ordering::SeqCst);
    let wakers = std::mem::take(
      &mut *self
        .wakers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for waker in wakers {
      waker.wake();
    }
  }

  /// `Err` si le lien est coupé ; sinon mémorise le réveil.
  fn check(&self, cx: &Context<'_>) -> io::Result<()> {
    if self.cut.load(Ordering::SeqCst) {
      return Err(io::ErrorKind::ConnectionReset.into());
    }
    let mut wakers = self
      .wakers
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !wakers.iter().any(|waker| waker.will_wake(cx.waker())) {
      wakers.push(cx.waker().clone());
    }
    drop(wakers);
    // Coupure survenue entre le test et l'enregistrement du réveil.
    if self.cut.load(Ordering::SeqCst) {
      return Err(io::ErrorKind::ConnectionReset.into());
    }
    Ok(())
  }
}

struct Cut<T> {
  inner: T,
  switch: Arc<Switch>,
}

impl<T> Cut<T> {
  const fn new(inner: T, switch: Arc<Switch>) -> Self {
    Self { inner, switch }
  }
}

impl<T: AsyncRead + Unpin> AsyncRead for Cut<T> {
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    if let Err(err) = self.switch.check(cx) {
      return Poll::Ready(Err(err));
    }
    Pin::new(&mut self.inner).poll_read(cx, buf)
  }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Cut<T> {
  fn poll_write(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &[u8],
  ) -> Poll<io::Result<usize>> {
    if let Err(err) = self.switch.check(cx) {
      return Poll::Ready(Err(err));
    }
    Pin::new(&mut self.inner).poll_write(cx, buf)
  }

  fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    if let Err(err) = self.switch.check(cx) {
      return Poll::Ready(Err(err));
    }
    Pin::new(&mut self.inner).poll_flush(cx)
  }

  fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    Pin::new(&mut self.inner).poll_shutdown(cx)
  }
}
