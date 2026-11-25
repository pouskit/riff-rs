//! Abonnement d'iced aux événements système (seconde instance, touches média, menu de la zone de notification, raccourcis).

use super::startup;
use crate::app::Message;
use iced::futures::{Stream, stream};

/// Flux des événements système. Le récepteur n'est remis qu'une fois : un second abonnement rend un flux vide.
pub fn subscription() -> iced::Subscription<Message> {
  iced::Subscription::run(events)
}

fn events() -> impl Stream<Item = Message> {
  stream::unfold(startup::take_events(), |receiver| async move {
    let mut receiver = receiver?;
    let event = receiver.recv().await?;
    Some((Message::System(event), Some(receiver)))
  })
}
