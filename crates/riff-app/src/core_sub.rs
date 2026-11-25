//! Événements du core vers un flux de messages.

use crate::app::Message;
use iced::Subscription;
use iced::futures::{Stream, stream};
use riff_core::core::Core;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

/// Flux des événements du core. Un retard de l'interface saute les événements perdus sans couper le flux.
pub fn stream(core: &Core) -> impl Stream<Item = Message> + use<> {
  stream::unfold(core.subscribe(), |mut events| async move {
    loop {
      match events.recv().await {
        Ok(event) => return Some((Message::Core(event), events)),
        Err(RecvError::Lagged(skipped)) => tracing::debug!(skipped, "événements du core sautés"),
        Err(RecvError::Closed) => return None,
      }
    }
  })
}

struct CoreHandle(Arc<Core>);

impl Hash for CoreHandle {
  fn hash<H: Hasher>(&self, state: &mut H) {
    Arc::as_ptr(&self.0).hash(state);
  }
}

/// Abonnement aux événements du core.
pub fn subscription(core: Arc<Core>) -> Subscription<Message> {
  Subscription::run_with(CoreHandle(core), |handle| stream(&handle.0))
}
