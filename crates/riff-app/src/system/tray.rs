//! Zone de notification par `tray-icon` : menu « Show/Hide », « Play/Pause », « Always on top », « Start with Windows », « Quit », comme l'original (`main.js:2228`).

use super::autostart;
use super::host::TrayLabels;
use super::{Command, SystemEvent};
use tokio::sync::mpsc::UnboundedSender;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const ICON_PNG: &[u8] = include_bytes!("../../assets/icon-512.png");
const ICON_SIZE: u32 = 32;
const ID_SHOW_HIDE: &str = "show-hide";
const ID_PLAY_PAUSE: &str = "play-pause";
const ID_ALWAYS_ON_TOP: &str = "always-on-top";
const ID_AUTOSTART: &str = "start-with-windows";
const ID_QUIT: &str = "quit";
const TOOLTIP: &str = "Riff";

/// Icône décodée et réduite à 32 px, en RGBA.
pub fn icon_rgba() -> Result<(Vec<u8>, u32, u32), image::ImageError> {
  let full = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)?;
  let small = image::imageops::resize(
    &full.to_rgba8(),
    ICON_SIZE,
    ICON_SIZE,
    image::imageops::FilterType::Lanczos3,
  );
  Ok((small.into_raw(), ICON_SIZE, ICON_SIZE))
}

/// Événement du menu, d'après l'identifiant de l'entrée choisie. Le menu envoie les mêmes commandes que les touches média.
#[must_use]
pub fn event_for_menu(id: &str) -> Option<SystemEvent> {
  match id {
    ID_SHOW_HIDE => Some(SystemEvent::ToggleWindow),
    ID_PLAY_PAUSE => Some(SystemEvent::Command(Command::Toggle)),
    ID_ALWAYS_ON_TOP => Some(SystemEvent::ToggleAlwaysOnTop),
    ID_AUTOSTART => Some(SystemEvent::ToggleAutostart),
    ID_QUIT => Some(SystemEvent::Quit),
    _ => None,
  }
}

/// Clic sur l'icône : un clic gauche relâché affiche ou masque la fenêtre.
#[must_use]
pub fn event_for_click(event: &TrayIconEvent) -> Option<SystemEvent> {
  match event {
    TrayIconEvent::Click {
      button: MouseButton::Left,
      button_state: MouseButtonState::Up,
      ..
    } => Some(SystemEvent::ToggleWindow),
    _ => None,
  }
}

/// Icône visible et ses entrées de menu.
pub struct Tray {
  _icon: TrayIcon,
  show_hide: MenuItem,
  play_pause: MenuItem,
  always_on_top: CheckMenuItem,
  autostart: CheckMenuItem,
  quit: MenuItem,
}

impl Tray {
  /// Crée l'icône sur le fil appelant, qui doit faire tourner une boucle de messages.
  pub fn create(
    labels: &TrayLabels,
    always_on_top: bool,
    autostart: autostart::State,
    events: UnboundedSender<SystemEvent>,
  ) -> Result<Self, String> {
    let (rgba, width, height) = icon_rgba().map_err(|error| error.to_string())?;
    let icon =
      tray_icon::Icon::from_rgba(rgba, width, height).map_err(|error| error.to_string())?;
    let show_hide = MenuItem::with_id(ID_SHOW_HIDE, &labels.show_hide, true, None);
    let play_pause = MenuItem::with_id(ID_PLAY_PAUSE, &labels.play_pause, true, None);
    let always_on_top = CheckMenuItem::with_id(
      ID_ALWAYS_ON_TOP,
      &labels.always_on_top,
      true,
      always_on_top,
      None,
    );
    let start_with_windows = CheckMenuItem::with_id(
      ID_AUTOSTART,
      &labels.start_with_windows,
      autostart != autostart::State::Unavailable,
      autostart == autostart::State::On,
      None,
    );
    let quit = MenuItem::with_id(ID_QUIT, &labels.quit, true, None);
    let menu = Menu::new();
    menu
      .append_items(&[
        &show_hide,
        &play_pause,
        &always_on_top,
        &start_with_windows,
        &quit,
      ])
      .map_err(|error| error.to_string())?;
    let sender = events.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
      if let Some(event) = event_for_menu(&event.id.0) {
        let _ = sender.send(event);
      }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
      if let Some(event) = event_for_click(&event) {
        let _ = events.send(event);
      }
    }));
    let tray = TrayIconBuilder::new()
      .with_menu(Box::new(menu))
      .with_menu_on_left_click(false)
      .with_tooltip(TOOLTIP)
      .with_icon(icon)
      .build()
      .map_err(|error| error.to_string())?;
    Ok(Self {
      _icon: tray,
      show_hide,
      play_pause,
      always_on_top,
      autostart: start_with_windows,
      quit,
    })
  }

  pub fn set_labels(&self, labels: &TrayLabels) {
    self.show_hide.set_text(&labels.show_hide);
    self.play_pause.set_text(&labels.play_pause);
    self.always_on_top.set_text(&labels.always_on_top);
    self.autostart.set_text(&labels.start_with_windows);
    self.quit.set_text(&labels.quit);
  }

  /// L'entrée se coche d'elle-même au clic : l'application la remet d'aplomb si le réglage diffère.
  pub fn set_always_on_top(&self, on: bool) {
    self.always_on_top.set_checked(on);
  }

  /// La case suit le registre, relu après chaque bascule.
  pub fn set_autostart(&self, state: autostart::State) {
    self
      .autostart
      .set_enabled(state != autostart::State::Unavailable);
    self.autostart.set_checked(state == autostart::State::On);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn icon_is_32_by_32_rgba_and_not_blank() {
    let (rgba, width, height) = icon_rgba().expect("icon");
    assert_eq!((width, height), (32, 32));
    assert_eq!(rgba.len(), 32 * 32 * 4);
    assert!(
      rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0),
      "image entièrement transparente"
    );
  }

  #[test]
  fn menu_entries_send_the_media_commands() {
    assert_eq!(event_for_menu("show-hide"), Some(SystemEvent::ToggleWindow));
    assert_eq!(
      event_for_menu("play-pause"),
      Some(SystemEvent::Command(Command::Toggle))
    );
    assert_eq!(
      event_for_menu("always-on-top"),
      Some(SystemEvent::ToggleAlwaysOnTop)
    );
    assert_eq!(
      event_for_menu("start-with-windows"),
      Some(SystemEvent::ToggleAutostart)
    );
    assert_eq!(event_for_menu("quit"), Some(SystemEvent::Quit));
    assert_eq!(event_for_menu("other"), None);
  }
}
