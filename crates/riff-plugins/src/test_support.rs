//! Faux hôte pour les tests : répond aux demandes des plugins et garde tout ce qu'ils envoient.

use crate::api::{HostRequest, HostResponse, Lifecycle, Outgoing};
use crate::host::PluginHost;
use crate::manifest::{PluginSource, from_bytes};
use crate::test_temp::TempDir;
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub type Responder = Arc<dyn Fn(&HostRequest) -> Option<HostResponse> + Send + Sync>;

/// Ce que le faux hôte a reçu.
#[derive(Default)]
pub struct Seen {
  pub requests: Vec<(String, HostRequest)>,
  pub lifecycle: Vec<Lifecycle>,
}

/// Dossier d'état d'un faux hôte. Un fil de plugin peut encore enregistrer son état après la fin du test et
/// recréer le dossier : on le vide plusieurs fois avant de laisser `TempDir` faire la suppression finale.
pub struct StateDir(TempDir);

impl std::ops::Deref for StateDir {
  type Target = std::path::Path;

  fn deref(&self) -> &std::path::Path {
    &self.0
  }
}

impl Drop for StateDir {
  fn drop(&mut self) {
    for _ in 0..3 {
      thread::sleep(Duration::from_millis(20));
      let _ = std::fs::remove_dir_all(&self.0);
    }
  }
}

pub struct FakeHost {
  pub host: PluginHost,
  pub seen: Arc<Mutex<Seen>>,
  pub state_dir: StateDir,
}

/// Réponses par défaut : une file de trois pistes, lecture en cours.
pub fn default_answer(request: &HostRequest) -> Option<HostResponse> {
  let value = match request {
    HostRequest::Track => json!({ "id": "1", "title": "Un", "artist": "A", "platform": "youtube" }),
    HostRequest::IsPlaying | HostRequest::ShuffleOn => json!(true),
    HostRequest::Queue | HostRequest::Favorites | HostRequest::History => sample_queue(),
    HostRequest::QueueIndex => json!(0),
    HostRequest::QueueContext => json!("playlist"),
    HostRequest::QueueAppend(tracks) => Value::Array(tracks.clone()),
    HostRequest::Search { .. } | HostRequest::Vibe(_) => {
      json!([{ "id": "s1", "title": "Trouvé", "artist": "B", "platform": "youtube" }])
    }
    HostRequest::Notify { .. } | HostRequest::Log(_) => return None,
  };
  Some(HostResponse::Value(value))
}

pub fn sample_queue() -> Value {
  json!([
    { "id": "1", "title": "Un", "artist": "A", "platform": "youtube" },
    { "id": "2", "title": "Deux", "artist": "B", "platform": "youtube" },
    { "id": "3", "title": "Trois", "artist": "C", "platform": "youtube" }
  ])
}

pub fn temp_dir(tag: &str) -> TempDir {
  TempDir::new(&format!("plugins-{tag}"))
}

pub fn source(file: &str, code: &str) -> PluginSource {
  from_bytes(file, code.as_bytes()).unwrap()
}

impl FakeHost {
  pub fn new(tag: &str) -> Self {
    Self::with(tag, Arc::new(default_answer), Some(7))
  }

  pub fn with(tag: &str, responder: Responder, seed: Option<u64>) -> Self {
    let state_dir = StateDir(temp_dir(tag));
    let (host, incoming) = PluginHost::new(state_dir.to_path_buf());
    let host = match seed {
      Some(seed) => host.with_seed(seed),
      None => host,
    };
    let seen = Arc::new(Mutex::new(Seen::default()));
    spawn_responder(incoming, responder, Arc::clone(&seen));
    Self {
      host,
      seen,
      state_dir,
    }
  }

  /// Attend une condition sur ce qui a été reçu.
  pub fn wait_for(&self, timeout: Duration, check: impl Fn(&Seen) -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
      if check(&self.seen.lock().unwrap()) {
        return true;
      }
      thread::sleep(Duration::from_millis(5));
    }
    check(&self.seen.lock().unwrap())
  }

  pub fn start(&mut self, file: &str, code: &str) {
    self.host.activate(source(file, code)).unwrap();
    let started = self.wait_for(Duration::from_secs(10), |seen| {
      seen.lifecycle.iter().any(|event| {
        event.file() == file
          && matches!(
            event,
            Lifecycle::Started { .. } | Lifecycle::Failed { .. } | Lifecycle::Disabled { .. }
          )
      })
    });
    assert!(started, "le plugin {file} n'a pas démarré");
  }

  pub fn notifications(&self) -> Vec<String> {
    self
      .seen
      .lock()
      .unwrap()
      .requests
      .iter()
      .filter_map(|(_, request)| match request {
        HostRequest::Notify { message, .. } => Some(message.clone()),
        _ => None,
      })
      .collect()
  }

  pub fn errors(&self) -> Vec<String> {
    self
      .seen
      .lock()
      .unwrap()
      .lifecycle
      .iter()
      .filter_map(|event| match event {
        Lifecycle::Error { message, .. } => Some(message.clone()),
        _ => None,
      })
      .collect()
  }

  pub fn wait_notification(&self, text: &str) -> bool {
    self.wait_for(Duration::from_secs(10), |seen| {
      seen.requests.iter().any(|(_, request)| {
        matches!(request, HostRequest::Notify { message, .. } if message.contains(text))
      })
    })
  }
}

/// Réponse spéciale : la demande est abandonnée sans réponse, comme par un exécuteur arrêté.
pub const DROP: &str = "__drop__";

fn spawn_responder(incoming: Receiver<Outgoing>, responder: Responder, seen: Arc<Mutex<Seen>>) {
  thread::spawn(move || {
    // Demandes sans réponse : gardées, le plugin attend jusqu'à son échéance.
    let mut pending = Vec::new();
    while let Ok(message) = incoming.recv() {
      match message {
        Outgoing::Call(call) => {
          seen
            .lock()
            .unwrap()
            .requests
            .push((call.plugin.clone(), call.request.clone()));
          match responder(&call.request) {
            Some(HostResponse::Error(marker)) if marker == DROP => drop(call),
            Some(response) => call.reply(response),
            None => pending.push(call),
          }
        }
        Outgoing::Lifecycle(event) => seen.lock().unwrap().lifecycle.push(event),
      }
    }
  });
}
