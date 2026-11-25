//! Linux : NON COMPILÉ sur la machine de développement (Windows seul), à vérifier par la CI.
//! Zone de notification par le protocole StatusNotifierItem (`tray-icon`, option `ksni`), session MPRIS par `souvlaki`
//! (zbus, sans libdbus), raccourcis globaux par `global-hotkey` (X11 seulement, rien sous Wayland pur).
//! Aucun de ces trois ne demande de boucle de messages : le fil se contente d'attendre les demandes.

use crate::system::host::{Core, Host, HostInit, HostRequest, Report};
use crate::system::{SystemEvent, hotkeys, tray::Tray};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use tokio::sync::mpsc::UnboundedSender;

pub fn spawn(
  events: UnboundedSender<SystemEvent>,
  init: HostInit,
  report: Arc<Report>,
) -> io::Result<Host> {
  let (requests, receiver) = mpsc::channel();
  let thread_report = Arc::clone(&report);
  let join = std::thread::Builder::new()
    .name("riff-system-host".into())
    .spawn(move || run(&events, &receiver, init, &thread_report))?;
  Ok(Host::new(requests, Box::new(|| {}), join, report))
}

fn register_hotkeys(manager: &GlobalHotKeyManager, report: &Report) {
  let mut failed = Vec::new();
  for (hotkey, _) in hotkeys::bindings() {
    if let Err(error) = manager.register(hotkey) {
      tracing::warn!(%error, shortcut = %hotkeys::describe(&hotkey), "raccourci global refusé");
      failed.push(hotkeys::describe(&hotkey));
    }
  }
  if let Ok(mut list) = report.failed_hotkeys.lock() {
    *list = failed;
  }
}

fn run(
  events: &UnboundedSender<SystemEvent>,
  requests: &Receiver<HostRequest>,
  init: HostInit,
  report: &Arc<Report>,
) {
  let hotkey_events = events.clone();
  GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
    if event.state == HotKeyState::Pressed
      && let Some(command) = hotkeys::command_for(event.id)
    {
      let _ = hotkey_events.send(SystemEvent::Command(command));
    }
  }));
  let manager = GlobalHotKeyManager::new()
    .inspect_err(|error| tracing::warn!(%error, "raccourcis globaux indisponibles"))
    .ok();
  if let Some(manager) = &manager {
    register_hotkeys(manager, report);
  }
  let tray = Tray::create(&init.labels, events.clone())
    .inspect_err(|error| tracing::warn!(%error, "zone de notification indisponible"))
    .ok();
  report
    .tray_active
    .store(tray.is_some(), std::sync::atomic::Ordering::Relaxed);
  let mut core = Core::new(events.clone(), Arc::clone(report), init.media_session);
  // MPRIS n'a pas besoin de fenêtre : la session s'ouvre dès le départ si elle est voulue.
  core.apply(HostRequest::MediaSession {
    enabled: init.media_session,
    hwnd: None,
  });
  while let Ok(request) = requests.recv() {
    match request {
      HostRequest::Labels(labels) => {
        if let Some(tray) = &tray {
          tray.set_labels(&labels);
        }
      }
      HostRequest::Resume => {
        if let Some(manager) = &manager {
          for (hotkey, _) in hotkeys::bindings() {
            let _ = manager.unregister(hotkey);
          }
          register_hotkeys(manager, report);
        }
        core.recreate_session();
      }
      other => {
        if !core.apply(other) {
          break;
        }
      }
    }
  }
}
