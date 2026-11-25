//! macOS : NON COMPILÉ sur la machine de développement (Windows seul), à vérifier par la CI.
//! Repli documenté : seule la session média (`souvlaki`, centre de commandes) est ouverte. La zone de notification
//! (`tray-icon`) et les raccourcis globaux (`global-hotkey`) exigent d'être créés sur le fil principal, que la boucle
//! d'iced occupe : ils sont désactivés sur ce système, sans erreur, et `riff_tray_on_close` n'y a pas d'effet.

use crate::system::SystemEvent;
use crate::system::host::{Core, Host, HostInit, HostRequest, Report};
use std::io;
use std::sync::Arc;
use std::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;

pub fn spawn(
  events: UnboundedSender<SystemEvent>,
  init: HostInit,
  report: Arc<Report>,
) -> io::Result<Host> {
  let (requests, receiver) = mpsc::channel::<HostRequest>();
  let thread_report = Arc::clone(&report);
  let join = std::thread::Builder::new()
    .name("riff-system-host".into())
    .spawn(move || {
      let mut core = Core::new(events, thread_report, init.media_session);
      core.apply(HostRequest::MediaSession {
        enabled: init.media_session,
        hwnd: None,
      });
      while let Ok(request) = receiver.recv() {
        match request {
          HostRequest::Resume => core.recreate_session(),
          other => {
            if !core.apply(other) {
              break;
            }
          }
        }
      }
    })?;
  Ok(Host::new(requests, Box::new(|| {}), join, report))
}
