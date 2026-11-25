//! Réglages de l'intégration système : raccourcis globaux et raccourcis du Menu Démarrer.

use crate::app::{App, Message};
use crate::system::update_install::{self, InstallState};
use crate::system::updates::{Outcome, UpdateState};
use crate::system::{launcher, startup};
use crate::widgets::button::text_button;
use iced::Element;
use iced::widget::{column, text};

/// Raccourcis globaux et ceux que le système a refusés (déjà pris par un autre programme).
#[must_use]
pub fn shortcuts_panel(app: &App) -> Element<'_, Message> {
  let mut list = column![].spacing(4);
  #[cfg(any(windows, target_os = "linux"))]
  {
    let failed = startup::report()
      .map(|report| report.failed())
      .unwrap_or_default();
    for (hotkey, command) in crate::system::hotkeys::bindings() {
      let name = crate::system::hotkeys::describe(&hotkey);
      let line = if failed.contains(&name) {
        format!(
          "{name} : {command:?} ({})",
          app.i18n.t("already used by another program")
        )
      } else {
        format!("{name} : {command:?}")
      };
      list = list.push(text(line).size(13));
    }
  }
  #[cfg(not(any(windows, target_os = "linux")))]
  {
    let _ = startup::report();
    list = list.push(text(app.i18n.t("Not available on this system.")).size(13));
  }
  list.into()
}

/// Raccourcis du Menu Démarrer et du Bureau : état.
#[must_use]
pub fn launcher_panel(app: &App) -> Element<'_, Message> {
  let info = launcher::info();
  let status = if !info.can_install {
    app.i18n.t("Not supported on this system")
  } else if info.installed {
    app.i18n.t("Installed")
  } else {
    app.i18n.t("Not installed")
  };
  let mut content = column![text(status).size(13)].spacing(8);
  if info.can_install {
    content = content.push(text_button(
      app.i18n.t(if info.installed {
        "Reinstall shortcuts"
      } else {
        "Create shortcuts"
      }),
      Some(Message::LauncherInstall),
    ));
  }
  content.into()
}

/// Version du programme et vérification de mise à jour, sur clic seulement.
#[must_use]
pub fn version_panel(app: &App) -> Element<'_, Message> {
  let mut content = column![text(env!("CARGO_PKG_VERSION")).size(13)].spacing(8);
  if !app.settings.update_check {
    return content.into();
  }
  let checking = app.update == UpdateState::Checking;
  content = content.push(text_button(
    app.i18n.t(if checking {
      "Checking…"
    } else {
      "Check for updates"
    }),
    (!checking).then_some(Message::UpdateCheck),
  ));
  match &app.update {
    UpdateState::Idle | UpdateState::Checking => {}
    UpdateState::Done(Outcome::UpToDate) => {
      content = content.push(text(app.i18n.t("You are up to date.")).size(13))
    }
    UpdateState::Done(Outcome::NoRelease) => {
      content = content.push(text(app.i18n.t("No public release found.")).size(13))
    }
    UpdateState::Done(Outcome::Available(release)) => {
      content = content
        .push(
          text(format!(
            "{} {}",
            app.i18n.t("Version available:"),
            release.version
          ))
          .size(13),
        )
        .push(text(release.notes.clone()).size(12))
        .push(text_button(
          app.i18n.t("Open release page"),
          Some(Message::OpenReleasePage),
        ));
      if update_install::can_install() {
        let idle = matches!(app.install, InstallState::Idle | InstallState::Failed(_));
        content = content.push(text_button(
          app.i18n.t("Download and install"),
          idle.then_some(Message::UpdateInstall),
        ));
      }
      match &app.install {
        InstallState::Idle => {}
        InstallState::Downloading(percent) => {
          content =
            content.push(text(format!("{} {percent} %", app.i18n.t("Downloading…"))).size(13))
        }
        InstallState::Verifying => content = content.push(text(app.i18n.t("Verifying…")).size(13)),
        InstallState::Launched => {
          content = content.push(text(app.i18n.t("Installing: Riff closes and restarts.")).size(13))
        }
        InstallState::Failed(error) => {
          content = content.push(text(format!("{} {error}", app.i18n.t("Update failed:"))).size(13))
        }
      }
    }
    UpdateState::Failed(error) => {
      content =
        content.push(text(format!("{} {error}", app.i18n.t("Update check failed:"))).size(13))
    }
  }
  content.into()
}
