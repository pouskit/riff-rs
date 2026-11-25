//! Une implémentation par plateforme. Seul Windows est compilé et testé sur la machine de développement.

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
mod windows_host;
#[cfg(windows)]
pub mod windows_shell;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub use windows_host::spawn;

#[cfg(target_os = "linux")]
pub use linux::spawn;

#[cfg(target_os = "macos")]
pub use macos::spawn;

/// Identifiant stable de l'utilisateur courant, utilisé dans le nom du canal local.
pub fn user_id() -> String {
  #[cfg(windows)]
  let id = windows::user_id();
  #[cfg(unix)]
  let id = unix::user_id();
  id.unwrap_or_else(|error| {
    tracing::warn!(%error, "identifiant d'utilisateur introuvable, nom de repli");
    "user".to_string()
  })
}

/// Préférence « réduire les animations » du système. Windows : `SPI_GETCLIENTAREAANIMATION` inversé ; ailleurs, animations actives.
pub fn prefers_reduced_motion() -> bool {
  #[cfg(windows)]
  {
    !windows::client_area_animations()
  }
  #[cfg(not(windows))]
  {
    false
  }
}
