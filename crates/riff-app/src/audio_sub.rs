//! Événements du moteur audio vers un flux de messages.

use crate::app::Message;
use iced::Subscription;
use iced::futures::Stream;
use iced::futures::stream::{self, BoxStream, StreamExt};
use riff_audio::Event;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::mpsc;

/// Flux des événements d'un récepteur du moteur. Le récepteur est bloquant : il vit dans son propre fil.
pub fn stream(events: Receiver<Event>) -> impl Stream<Item = Message> {
  let (sender, receiver) = mpsc::unbounded_channel();
  let spawned = thread::Builder::new()
    .name("riff-audio-events".into())
    .spawn(move || {
      while let Ok(event) = events.recv() {
        if sender.send(Message::Audio(event)).is_err() {
          break;
        }
      }
    });
  if let Err(err) = spawned {
    tracing::error!(error = %err, "fil des événements audio");
  }
  stream::unfold(receiver, |mut receiver| async move {
    receiver.recv().await.map(|message| (message, receiver))
  })
}

/// Récepteur du moteur, consommé une seule fois par l'abonnement.
#[derive(Clone)]
pub struct AudioEvents(Arc<Mutex<Option<Receiver<Event>>>>);

impl AudioEvents {
  #[must_use]
  pub fn new(events: Receiver<Event>) -> Self {
    Self(Arc::new(Mutex::new(Some(events))))
  }
}

impl Hash for AudioEvents {
  fn hash<H: Hasher>(&self, state: &mut H) {
    Arc::as_ptr(&self.0).hash(state);
  }
}

fn start(events: &AudioEvents) -> BoxStream<'static, Message> {
  let taken = events.0.lock().ok().and_then(|mut slot| slot.take());
  taken.map_or_else(
    || stream::empty().boxed(),
    |receiver| stream(receiver).boxed(),
  )
}

/// Abonnement aux événements du moteur.
pub fn subscription(events: AudioEvents) -> Subscription<Message> {
  Subscription::run_with(events, start)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::mpsc;
  use std::time::Duration;

  #[tokio::test]
  async fn engine_events_arrive_as_messages_in_order() {
    let (sender, receiver) = mpsc::channel();
    let mut messages = Box::pin(stream(receiver));
    sender
      .send(Event::Position(Duration::from_secs(3)))
      .unwrap();
    sender.send(Event::Ended).unwrap();
    let Some(Message::Audio(Event::Position(position))) = messages.next().await else {
      panic!("attendu : position");
    };
    assert_eq!(position, Duration::from_secs(3));
    assert!(matches!(
      messages.next().await,
      Some(Message::Audio(Event::Ended))
    ));
  }

  #[tokio::test]
  async fn the_stream_ends_when_the_engine_is_gone() {
    let (sender, receiver) = mpsc::channel::<Event>();
    let mut messages = Box::pin(stream(receiver));
    drop(sender);
    assert!(messages.next().await.is_none());
  }

  #[tokio::test]
  async fn the_receiver_is_only_taken_once() {
    let (sender, receiver) = mpsc::channel();
    let events = AudioEvents::new(receiver);
    let mut first = start(&events);
    let mut second = start(&events);
    assert!(second.next().await.is_none());
    sender.send(Event::Ended).unwrap();
    assert!(matches!(
      first.next().await,
      Some(Message::Audio(Event::Ended))
    ));
  }
}
