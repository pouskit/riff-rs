//! Fil système de Windows : boucle de messages Win32 pour `tray-icon` et `global-hotkey`, fenêtre invisible pour la veille.

use crate::system::host::{Core, Host, HostInit, HostRequest, Report};
use crate::system::{SystemEvent, hotkeys, tray::Tray};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use tokio::sync::mpsc::UnboundedSender;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
  NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
  CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
  PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PM_NOREMOVE, PeekMessageW, PostThreadMessageW,
  RegisterClassW, TranslateMessage, WM_APP, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
  WS_OVERLAPPED, WTS_SESSION_UNLOCK,
};
use windows::core::w;

/// Posé par la fenêtre de veille, lu par la boucle après chaque message.
static RESUMED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn power_proc(
  window: HWND,
  message: u32,
  wparam: WPARAM,
  lparam: LPARAM,
) -> LRESULT {
  let resumed = match message {
    WM_POWERBROADCAST => matches!(
      wparam.0 as u32,
      PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND
    ),
    WM_WTSSESSION_CHANGE => wparam.0 as u32 == WTS_SESSION_UNLOCK,
    _ => false,
  };
  if resumed {
    RESUMED.store(true, Ordering::SeqCst);
  }
  // SAFETY: appel direct de la procédure par défaut avec les arguments reçus.
  unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Fenêtre sans affichage qui reçoit les messages de veille et de déverrouillage.
struct PowerWatcher(HWND);

impl PowerWatcher {
  fn create() -> io::Result<Self> {
    // SAFETY: classe et fenêtre locales au fil appelant, détruites dans `Drop`.
    unsafe {
      let instance = HINSTANCE(GetModuleHandleW(None).map_err(io::Error::other)?.0);
      let class = WNDCLASSW {
        lpfnWndProc: Some(power_proc),
        hInstance: instance,
        lpszClassName: w!("RiffPowerWatcher"),
        ..Default::default()
      };
      RegisterClassW(&class);
      let window = CreateWindowExW(
        Default::default(),
        class.lpszClassName,
        w!("Riff"),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some(instance),
        None,
      )
      .map_err(io::Error::other)?;
      let _ = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION);
      Ok(Self(window))
    }
  }
}

impl Drop for PowerWatcher {
  fn drop(&mut self) {
    // SAFETY: la fenêtre a été créée par `create` sur ce fil.
    unsafe {
      let _ = WTSUnRegisterSessionNotification(self.0);
      let _ = DestroyWindow(self.0);
    }
  }
}

/// Lance le fil système. Rend la poignée une fois la boucle de messages prête.
pub fn spawn(
  events: UnboundedSender<SystemEvent>,
  init: HostInit,
  report: Arc<Report>,
) -> io::Result<Host> {
  let (requests, receiver) = mpsc::channel();
  let (ready, ready_receiver) = mpsc::channel();
  let thread_report = Arc::clone(&report);
  let join = std::thread::Builder::new()
    .name("riff-system-host".into())
    .spawn(move || run(&events, &receiver, init, &thread_report, &ready))?;
  let thread_id = ready_receiver
    .recv()
    .map_err(|_| io::Error::other("system host thread did not start"))?;
  let wake = Box::new(move || {
    // SAFETY: `thread_id` est celui du fil système ; un message perdu après sa fin est sans effet.
    let _ = unsafe { PostThreadMessageW(thread_id, WM_APP, WPARAM(0), LPARAM(0)) };
  });
  Ok(Host::new(requests, wake, join, report))
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

fn unregister_hotkeys(manager: &GlobalHotKeyManager) {
  for (hotkey, _) in hotkeys::bindings() {
    let _ = manager.unregister(hotkey);
  }
}

fn run(
  events: &UnboundedSender<SystemEvent>,
  requests: &Receiver<HostRequest>,
  init: HostInit,
  report: &Arc<Report>,
  ready: &mpsc::Sender<u32>,
) {
  let mut message = MSG::default();
  // SAFETY: un premier `PeekMessageW` crée la file de messages du fil avant d'annoncer son identifiant ; `message` est une sortie valide.
  let thread_id = unsafe {
    let _ = PeekMessageW(&mut message, None, WM_APP, WM_APP, PM_NOREMOVE);
    GetCurrentThreadId()
  };
  let _ = ready.send(thread_id);

  let hotkey_events = events.clone();
  GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
    if event.state == HotKeyState::Pressed
      && let Some(command) = hotkeys::command_for(event.id)
    {
      let _ = hotkey_events.send(SystemEvent::Command(command));
    }
  }));
  let manager = match GlobalHotKeyManager::new() {
    Ok(manager) => {
      register_hotkeys(&manager, report);
      Some(manager)
    }
    Err(error) => {
      tracing::warn!(%error, "raccourcis globaux indisponibles");
      None
    }
  };
  let tray = match Tray::create(&init.labels, events.clone()) {
    Ok(tray) => {
      report
        .tray_active
        .store(true, std::sync::atomic::Ordering::Relaxed);
      Some(tray)
    }
    Err(error) => {
      tracing::warn!(%error, "zone de notification indisponible");
      None
    }
  };
  let _watcher = PowerWatcher::create()
    .inspect_err(|error| tracing::warn!(%error, "veille non surveillée"))
    .ok();
  let mut core = Core::new(events.clone(), Arc::clone(report), init.media_session);

  'messages: loop {
    // SAFETY: `message` est une sortie valide ; la boucle s'arrête sur `WM_QUIT` ou une erreur.
    let status = unsafe { GetMessageW(&mut message, None, 0, 0) };
    if status.0 <= 0 {
      break;
    }
    if message.message == WM_APP {
      while let Ok(request) = requests.try_recv() {
        match request {
          HostRequest::Labels(labels) => {
            if let Some(tray) = &tray {
              tray.set_labels(&labels);
            }
          }
          HostRequest::Resume => resume(manager.as_ref(), &mut core, report),
          other => {
            if !core.apply(other) {
              break 'messages;
            }
          }
        }
      }
    } else {
      // SAFETY: `message` vient de `GetMessageW`.
      unsafe {
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
      }
    }
    if RESUMED.swap(false, Ordering::SeqCst) {
      resume(manager.as_ref(), &mut core, report);
    }
  }
  if let Some(manager) = &manager {
    unregister_hotkeys(manager);
  }
}

/// Retour de veille ou déverrouillage : raccourcis ré-enregistrés, session recréée (`main.js:1225`).
fn resume(manager: Option<&GlobalHotKeyManager>, core: &mut Core, report: &Report) {
  if let Some(manager) = manager {
    unregister_hotkeys(manager);
    register_hotkeys(manager, report);
  }
  core.recreate_session();
}
