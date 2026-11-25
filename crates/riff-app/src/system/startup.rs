//! Démarrage : instance unique, seconde instance, canal local partagé avec le reste de l'application.

use super::SystemEvent;
use super::channel::{self, Claim, Server};
use super::host::Report;
use super::protocol::LyricsState;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static SERVER: OnceLock<Server> = OnceLock::new();
static SENDER: OnceLock<mpsc::UnboundedSender<SystemEvent>> = OnceLock::new();
static REPORT: OnceLock<Arc<Report>> = OnceLock::new();
static RECEIVER: Mutex<Option<mpsc::UnboundedReceiver<SystemEvent>>> = Mutex::new(None);

/// Suite du démarrage.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
  /// Lancer l'application.
  Run,
  /// Quitter avec ce code : la fenêtre d'une autre instance a été montrée, ou l'essai a échoué.
  Exit(i32),
}

/// Runtime du canal local et des tâches système : un seul fil, l'activité est faible.
pub fn runtime() -> &'static tokio::runtime::Runtime {
  RUNTIME.get_or_init(|| {
    tokio::runtime::Builder::new_multi_thread()
      .worker_threads(1)
      .thread_name("riff-system")
      .enable_all()
      .build()
      .expect("runtime du canal local")
  })
}

/// Émetteur des événements système, une fois [`acquire`] appelé.
pub fn sender() -> Option<mpsc::UnboundedSender<SystemEvent>> {
  SENDER.get().cloned()
}

/// Récepteur des événements système, remis une seule fois à l'abonnement d'`iced`.
pub fn take_events() -> Option<mpsc::UnboundedReceiver<SystemEvent>> {
  RECEIVER.lock().ok()?.take()
}

/// État du fil système, lisible par la page des réglages.
pub fn report() -> Option<Arc<Report>> {
  REPORT.get().cloned()
}

/// Enregistre l'état du fil système (une seule fois).
pub fn set_report(report: Arc<Report>) {
  let _ = REPORT.set(report);
}

/// Cette instance sert le canal local.
#[must_use]
pub fn serving() -> bool {
  SERVER.get().is_some()
}

/// Publie l'état des paroles aux clients `riff lyrics`. Sans effet si cette instance ne sert pas le canal.
pub fn publish_lyrics(state: LyricsState) {
  if let Some(server) = SERVER.get() {
    server.publish(state);
  }
}

/// Arguments qui désignent des fichiers : tout ce qui ne commence pas par un tiret, en chemins absolus.
/// La seconde instance part d'un autre dossier courant, d'où le chemin absolu.
#[must_use]
pub fn files_from_args(args: &[String], cwd: &Path) -> Vec<PathBuf> {
  args
    .iter()
    .filter(|arg| !arg.starts_with('-') && !arg.is_empty())
    .map(|arg| {
      let path = Path::new(arg);
      if path.is_absolute() {
        path.to_path_buf()
      } else {
        cwd.join(path)
      }
    })
    .collect()
}

/// `multiInstance` de `startup.json` : faux par défaut.
pub fn multi_instance() -> bool {
  riff_core::config::read_startup_config()
    .get("multiInstance")
    .and_then(serde_json::Value::as_bool)
    == Some(true)
}

/// Prend le canal local. Seconde instance : demande à la première de se montrer puis rend [`Outcome::Exit`].
pub fn acquire(args: &[String]) -> Outcome {
  let label = channel::channel_name();
  let (shows, mut show_events) = mpsc::unbounded_channel();
  let claim = runtime().block_on(channel::claim(&label, shows));
  match claim {
    Ok(Claim::Primary(server)) => {
      let (tx, rx) = mpsc::unbounded_channel();
      let forward = tx.clone();
      runtime().spawn(async move {
        while let Some(paths) = show_events.recv().await {
          if forward.send(SystemEvent::Show(paths)).is_err() {
            break;
          }
        }
      });
      let _ = SENDER.set(tx);
      if let Ok(mut slot) = RECEIVER.lock() {
        *slot = Some(rx);
      }
      let _ = SERVER.set(server);
      Outcome::Run
    }
    Ok(Claim::Existing) if multi_instance() => {
      separate_data_dir();
      Outcome::Run
    }
    Ok(Claim::Existing) => {
      let cwd = std::env::current_dir().unwrap_or_default();
      let files = files_from_args(args, &cwd)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
      match runtime().block_on(channel::notify_show(&label, files)) {
        Ok(()) => Outcome::Exit(0),
        Err(error) => {
          tracing::error!(%error, "une autre instance de Riff répond mal sur le canal local");
          Outcome::Exit(1)
        }
      }
    }
    Err(error) => {
      // Pas de canal : l'application démarre quand même, sans instance unique ni `riff lyrics`.
      tracing::warn!(%error, "canal local indisponible");
      Outcome::Run
    }
  }
}

/// Mode multi-instance : un dossier de données par processus, comme `userData-instance-<pid>` de l'original.
fn separate_data_dir() {
  let mut dir = riff_core::paths::resolve_data_dir().into_os_string();
  dir.push(format!("-instance-{}", std::process::id()));
  // SAFETY: appelé au début de `main`, avant l'application ; les fils du runtime système sont au repos.
  unsafe { std::env::set_var("RIFF_DATA_DIR", dir) };
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn flags_are_not_files_and_relative_paths_become_absolute() {
    let cwd = Path::new(if cfg!(windows) { r"C:\music" } else { "/music" });
    let args = [
      "--dev".to_string(),
      "a.mp3".to_string(),
      String::new(),
      "-x".to_string(),
    ];
    assert_eq!(files_from_args(&args, cwd), vec![cwd.join("a.mp3")]);
  }

  #[test]
  fn absolute_paths_are_kept() {
    let absolute = if cfg!(windows) {
      r"D:\x\b.flac"
    } else {
      "/x/b.flac"
    };
    assert_eq!(
      files_from_args(&[absolute.to_string()], Path::new("/ignored")),
      vec![PathBuf::from(absolute)]
    );
  }
}
