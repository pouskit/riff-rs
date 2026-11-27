//! Windows desktop wallpaper synchronization with ambient cover-art rendering and 60 fps crossfade.

pub mod cache;
pub mod config;
pub mod render;
pub mod scheduler;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod win32;

pub use config::{
  OutputMode, RenderParams, TransitionAnchor, TransitionEasing, WallpaperSyncConfig, WallpaperTrack,
};

use crate::backend::Backend;
use std::sync::Arc;

/// Coordinates the background wallpaper-sync worker (no-op off Windows).
pub struct WallpaperController {
  #[cfg(windows)]
  worker: Option<service::WorkerHandle>,
}

impl Default for WallpaperController {
  fn default() -> Self {
    Self::new()
  }
}

impl WallpaperController {
  #[must_use]
  pub fn new() -> Self {
    Self {
      #[cfg(windows)]
      worker: None,
    }
  }

  pub fn sync(
    &mut self,
    config: &WallpaperSyncConfig,
    current: Option<WallpaperTrack>,
    next: Option<WallpaperTrack>,
    backend: &Arc<Backend>,
  ) {
    #[cfg(windows)]
    {
      if !config.enabled {
        self.disable();
        return;
      }
      if self.worker.is_none() {
        self.worker = service::WorkerHandle::spawn(config.clone(), Arc::clone(backend));
      }
      if let Some(worker) = &self.worker {
        worker.send_state(config.clone(), current, next);
      }
    }
    #[cfg(not(windows))]
    {
      let _ = (config, current, next, backend);
    }
  }

  /// Signals the worker to restore the original wallpaper and go idle without blocking the UI thread.
  pub fn disable(&mut self) {
    #[cfg(windows)]
    if let Some(worker) = &self.worker {
      worker.disable();
    }
  }

  /// Signals the worker to stop without blocking (used during exit before `Drop` joins).
  pub fn request_stop(&mut self) {
    #[cfg(windows)]
    if let Some(worker) = &self.worker {
      worker.request_stop();
    }
  }

  pub fn stop(&mut self) {
    #[cfg(windows)]
    if let Some(worker) = self.worker.take() {
      worker.stop();
    }
  }
}

impl Drop for WallpaperController {
  fn drop(&mut self) {
    self.stop();
  }
}
