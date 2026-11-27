//! Dedicated background OS thread for Windows wallpaper sync, caching, prefetch, and crossfade.

use super::cache::{RenderCache, StateStore, WallpaperRing, cache_key};
use super::config::{
  COVER_FETCH_SIZE, MonitorTile, WallpaperSyncConfig, WallpaperTrack, upgrade_cover_url,
};
use super::render::{RgbImage, decode_rgb, encode_bmp_24, render_single, render_span};
use super::scheduler::FadeScheduler;
use super::win32::{
  FadeRunParams, FadeTransition, Rect, compute_layout, get_wallpaper, get_wallpaper_style,
  is_desktop_obscured, probe_desktop_capture, set_wallpaper_raw, set_wallpaper_style,
};
use crate::backend::Backend;
use crate::images;
use crate::system::startup;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TICK_INTERVAL: Duration = Duration::from_millis(50);
const SUPERSEDE_GRACE_SECS: f64 = 5.0;

#[derive(Debug)]
enum WorkerMsg {
  State(
    Box<(
      WallpaperSyncConfig,
      Option<WallpaperTrack>,
      Option<WallpaperTrack>,
    )>,
  ),
  Disable,
  Stop,
}

#[derive(Debug)]
struct BuiltWallpaper {
  key: String,
  track: WallpaperTrack,
  wallpaper_path: PathBuf,
}

#[derive(Debug)]
struct BuildDone {
  generation: u64,
  is_prefetch: bool,
  result: Option<BuiltWallpaper>,
}

pub struct WorkerHandle {
  tx: Sender<WorkerMsg>,
  abort_fade: Arc<AtomicBool>,
  join: Option<JoinHandle<()>>,
}

impl WorkerHandle {
  #[must_use]
  pub fn spawn(config: WallpaperSyncConfig, backend: Arc<Backend>) -> Option<Self> {
    let (tx, rx) = mpsc::channel();
    let abort_fade = Arc::new(AtomicBool::new(false));
    let thread_abort = Arc::clone(&abort_fade);
    let data_dir = riff_core::paths::resolve_data_dir().join("wallpaper-sync");
    let runtime_handle = startup::runtime().handle().clone();

    let join = std::thread::Builder::new()
      .name("wallpaper-sync".into())
      .spawn(move || {
        let mut service =
          WallpaperService::new(config, data_dir, backend, runtime_handle, thread_abort);
        service.run(rx);
      })
      .inspect_err(|err| tracing::warn!(%err, "failed to spawn wallpaper-sync thread"))
      .ok()?;

    Some(Self {
      tx,
      abort_fade,
      join: Some(join),
    })
  }

  pub fn send_state(
    &self,
    config: WallpaperSyncConfig,
    current: Option<WallpaperTrack>,
    next: Option<WallpaperTrack>,
  ) {
    self.abort_fade.store(false, Ordering::Relaxed);
    let _ = self
      .tx
      .send(WorkerMsg::State(Box::new((config, current, next))));
  }

  pub fn disable(&self) {
    self.abort_fade.store(true, Ordering::Relaxed);
    let _ = self.tx.send(WorkerMsg::Disable);
  }

  pub fn request_stop(&self) {
    self.abort_fade.store(true, Ordering::Relaxed);
    let _ = self.tx.send(WorkerMsg::Stop);
  }

  pub fn stop(mut self) {
    self.request_stop();
    if let Some(join) = self.join.take() {
      let _ = join.join();
    }
  }
}

struct WallpaperService {
  config: WallpaperSyncConfig,
  backend: Arc<Backend>,
  runtime: tokio::runtime::Handle,
  abort_fade: Arc<AtomicBool>,
  enabled: bool,
  start_instant: Instant,
  rect: Rect,
  tiles: Option<Vec<MonitorTile>>,
  output_width: u32,
  output_height: u32,
  applied_style: Option<String>,
  cache: RenderCache,
  state_store: StateStore,
  ring: WallpaperRing,
  scheduler: FadeScheduler,
  current: Option<WallpaperTrack>,
  next_candidate: Option<WallpaperTrack>,
  applied_key: Option<String>,
  prepared: Option<BuiltWallpaper>,
  preparing_key: Option<String>,
  building_key: Option<String>,
  generation: u64,
  armed: Option<FadeTransition>,
  superseded_key: Option<String>,
  superseded_at: f64,
  original_wallpaper: Option<String>,
  original_style: Option<String>,
  done_tx: Sender<BuildDone>,
  done_rx: Receiver<BuildDone>,
}

impl WallpaperService {
  fn new(
    config: WallpaperSyncConfig,
    data_dir: PathBuf,
    backend: Arc<Backend>,
    runtime: tokio::runtime::Handle,
    abort_fade: Arc<AtomicBool>,
  ) -> Self {
    let _ = fs::create_dir_all(&data_dir);
    let original_wallpaper = get_wallpaper();
    let original_style = get_wallpaper_style();
    let cache = RenderCache::new(
      data_dir.join("cache"),
      config.cache_enabled,
      config.cache_max_entries,
      config.cache_max_bytes,
    );
    let state_store = StateStore::new(&data_dir);
    let active_is_ours = original_wallpaper
      .as_deref()
      .is_some_and(|p| std::path::Path::new(p).starts_with(&data_dir));
    let applied_key = if active_is_ours {
      state_store.load_key()
    } else {
      None
    };
    let ring = WallpaperRing::new(data_dir.clone());
    let (done_tx, done_rx) = mpsc::channel();
    let enabled = config.enabled;

    let mut svc = Self {
      config,
      backend,
      runtime,
      abort_fade,
      enabled,
      start_instant: Instant::now(),
      rect: Rect {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
      },
      tiles: None,
      output_width: 1,
      output_height: 1,
      applied_style: None,
      cache,
      state_store,
      ring,
      scheduler: FadeScheduler::default(),
      current: None,
      next_candidate: None,
      applied_key,
      prepared: None,
      preparing_key: None,
      building_key: None,
      generation: 1,
      armed: None,
      superseded_key: None,
      superseded_at: 0.0,
      original_wallpaper,
      original_style,
      done_tx,
      done_rx,
    };
    svc.configure_output();
    if svc.config.transition_enabled {
      let _ = probe_desktop_capture();
    }
    svc
  }

  fn clock(&self) -> f64 {
    self.start_instant.elapsed().as_secs_f64()
  }

  fn configure_output(&mut self) {
    let span = matches!(self.config.output_mode, super::config::OutputMode::Span);
    let (rect, tiles) = compute_layout(span);
    self.rect = rect;
    self.tiles = tiles;
    self.output_width = rect.width.max(1) as u32;
    self.output_height = rect.height.max(1) as u32;
    let target_style = if self.tiles.is_some() { "22" } else { "10" };
    if self.config.force_fill_style && self.applied_style.as_deref() != Some(target_style) {
      set_wallpaper_style(target_style);
      self.applied_style = Some(target_style.to_string());
    }
  }

  fn signature(&self) -> String {
    let params = self.config.render_params();
    serde_json::to_string(&(&params, &self.tiles, self.output_width, self.output_height))
      .unwrap_or_default()
  }

  fn key_of(&self, track: &WallpaperTrack) -> String {
    let id = if track.id.is_empty() {
      &track.cover_url
    } else {
      &track.id
    };
    format!("{id}|{}", self.signature())
  }

  fn cache_key_of(&self, cover_url: &str) -> String {
    cache_key(
      cover_url,
      self.output_width,
      self.output_height,
      &self.config.render_params(),
      self.tiles.as_deref(),
    )
  }

  fn fade_lead(&self) -> f64 {
    f64::from(self.config.transition_arm_lead) + f64::from(self.config.transition_settle)
  }

  fn can_fade(&self) -> bool {
    if !self.config.transition_enabled {
      return false;
    }
    if !probe_desktop_capture() {
      return false;
    }
    if self.config.skip_when_obscured && is_desktop_obscured(self.rect) {
      return false;
    }
    true
  }

  fn cancel_schedule(&mut self, reason: Option<&str>) {
    if let Some(reason) = reason
      && (self.scheduler.fade_at.is_some() || self.armed.is_some())
    {
      if self.config.verbose_logging {
        tracing::debug!(reason, "wallpaper-sync scheduled fade cancelled");
      }
      self.scheduler.cancel();
      self.armed = None;
    }
  }

  fn refresh_deadline(&mut self, track: &WallpaperTrack) {
    let now = self.clock();
    let reason = self
      .scheduler
      .note_progress(track.remaining_secs, track.paused, now);
    self.cancel_schedule(reason);
  }

  fn spawn_build(&mut self, track: WallpaperTrack, is_prefetch: bool) {
    let key = self.key_of(&track);
    let cache_key_str = self.cache_key_of(&upgrade_cover_url(&track.cover_url, COVER_FETCH_SIZE));
    let bmp_path = self.ring.next_path();
    let cache = self.cache.clone();
    let backend = Arc::clone(&self.backend);
    let params = self.config.render_params();
    let width = self.output_width;
    let height = self.output_height;
    let tiles = self.tiles.clone();
    let done_tx = self.done_tx.clone();
    let generation = self.generation;

    if is_prefetch {
      self.preparing_key = Some(key.clone());
    } else {
      self.building_key = Some(key.clone());
    }

    self.runtime.spawn(async move {
      let upgraded = upgrade_cover_url(&track.cover_url, COVER_FETCH_SIZE);
      let cache_read = cache.clone();
      let cache_read_key = cache_key_str.clone();
      let mut img_opt = tokio::task::spawn_blocking(move || cache_read.get(&cache_read_key))
        .await
        .ok()
        .flatten();

      if img_opt.is_none() {
        let fetched = match images::fetch_bytes(&backend, &upgraded).await {
          Ok(bytes) => Ok(bytes),
          Err(err) if upgraded != track.cover_url => {
            tracing::debug!(%err, "upgraded cover fetch failed, falling back to raw URL");
            images::fetch_bytes(&backend, &track.cover_url).await
          }
          Err(err) => Err(err),
        };
        if let Ok(bytes) = fetched {
          let rendered = tokio::task::spawn_blocking(move || -> Option<RgbImage> {
            let cover = decode_rgb(&bytes).ok()?;
            let rendered = match &tiles {
              Some(t) => render_span(&cover, width, height, t, &params),
              None => render_single(&cover, width, height, &params),
            };
            cache.put(&cache_key_str, &rendered);
            Some(rendered)
          })
          .await
          .ok()
          .flatten();
          img_opt = rendered;
        }
      }

      let result = if let Some(img) = img_opt {
        let bmp_ok = tokio::task::spawn_blocking(move || {
          let bmp = encode_bmp_24(&img);
          fs::write(&bmp_path, bmp).map(|()| bmp_path)
        })
        .await
        .ok()
        .and_then(Result::ok);

        bmp_ok.map(|wallpaper_path| BuiltWallpaper {
          key,
          track,
          wallpaper_path,
        })
      } else {
        None
      };

      let _ = done_tx.send(BuildDone {
        generation,
        is_prefetch,
        result,
      });
    });
  }

  fn apply_built(&mut self, built: BuiltWallpaper, start_at: Option<Instant>) {
    self
      .state_store
      .save(Some(&built.key), &built.track.title, &built.track.artist);
    if !self.can_fade() {
      self.armed = None;
      set_wallpaper_raw(&built.wallpaper_path.to_string_lossy(), true);
      return;
    }

    let transition = match self.armed.take() {
      Some(t) => Some(t),
      None => FadeTransition::new(self.rect).ok(),
    };
    let Some(mut transition) = transition else {
      set_wallpaper_raw(&built.wallpaper_path.to_string_lossy(), true);
      return;
    };

    transition.begin(&built.wallpaper_path);
    let abort_fade = &self.abort_fade;
    transition.run(
      FadeRunParams {
        duration: f64::from(self.config.transition_duration),
        fps: self.config.transition_fps,
        easing: self.config.transition_easing,
        start_at,
        settle: f64::from(self.config.transition_settle),
      },
      || abort_fade.load(Ordering::Relaxed),
    );
  }

  fn handle_current(&mut self, track: WallpaperTrack, from_player: bool) {
    let key = self.key_of(&track);
    if self.applied_key.as_deref() == Some(key.as_str()) {
      self.superseded_key = None;
      self.current = Some(track.clone());
      self.refresh_deadline(&track);
      return;
    }
    if self.superseded_key.as_deref() == Some(key.as_str())
      && self.clock() - self.superseded_at < SUPERSEDE_GRACE_SECS
    {
      return;
    }
    if self.building_key.as_deref() == Some(key.as_str()) {
      self.current = Some(track.clone());
      self.refresh_deadline(&track);
      return;
    }

    if from_player {
      let _ = self.scheduler.learn_bias(self.clock());
    }
    self.cancel_schedule(Some("song changed"));

    if let Some(prepared) = self.prepared.take()
      && prepared.key == key
    {
      self.current = Some(track.clone());
      self.applied_key = Some(prepared.key.clone());
      self.refresh_deadline(&track);
      self.apply_built(prepared, None);
      return;
    }

    self.current = Some(track.clone());
    self.refresh_deadline(&track);
    self.spawn_build(track, false);
  }

  fn handle_state(
    &mut self,
    config: WallpaperSyncConfig,
    current: Option<WallpaperTrack>,
    next: Option<WallpaperTrack>,
  ) {
    if !config.enabled {
      self.config = config;
      self.handle_disable();
      return;
    }
    if !self.enabled {
      // Re-enabling from idle: refresh baseline original wallpaper & style if needed.
      self.enabled = true;
      self.original_wallpaper = get_wallpaper();
      self.original_style = get_wallpaper_style();
      self.applied_style = None;
      self.generation += 1;
    }

    let prev_sig = self.signature();
    self.config = config;
    self.cache.enabled = self.config.cache_enabled;
    self.cache.max_entries = self.config.cache_max_entries;
    self.cache.max_bytes = self.config.cache_max_bytes;
    self.configure_output();
    self.next_candidate = next;

    let sig_changed = self.signature() != prev_sig;
    if sig_changed {
      self.generation += 1;
      self.cancel_schedule(Some("settings changed"));
      self.prepared = None;
      self.preparing_key = None;
      self.building_key = None;
      self.applied_key = None;
    } else if self.armed.is_none()
      && let Some(prepared) = &self.prepared
      && self
        .next_candidate
        .as_ref()
        .map(|n| self.key_of(n))
        .as_deref()
        != Some(prepared.key.as_str())
    {
      self.prepared = None;
      self.cancel_schedule(Some("next track changed"));
    }

    if let Some(track) = current {
      self.handle_current(track, !sig_changed);
    } else {
      self.current = None;
      self.building_key = None;
      self.prepared = None;
      self.cancel_schedule(Some("no active track"));
    }
  }

  fn handle_disable(&mut self) {
    if !self.enabled {
      return;
    }
    self.enabled = false;
    self.generation += 1;
    self.cancel_schedule(Some("disabled"));
    self.current = None;
    self.next_candidate = None;
    self.prepared = None;
    self.preparing_key = None;
    self.building_key = None;
    self.applied_key = None;
    self.superseded_key = None;
    self.applied_style = None;
    self.restore_original();
  }

  fn handle_build_done(&mut self, done: BuildDone) {
    if done.generation != self.generation {
      return;
    }
    if done.is_prefetch {
      self.preparing_key = None;
      if let Some(built) = done.result
        && self.applied_key.as_deref() != Some(built.key.as_str())
      {
        self.prepared = Some(built);
      }
      return;
    }

    self.building_key = None;
    let Some(built) = done.result else {
      return;
    };
    let still_current = self
      .current
      .as_ref()
      .is_some_and(|t| self.key_of(t) == built.key);
    if !still_current {
      return;
    }
    self.applied_key = Some(built.key.clone());
    self.apply_built(built, None);
  }

  fn tick(&mut self) {
    // 1. Prefetch next track when ready.
    if self.config.prefetch_enabled
      && self.current.is_some()
      && self.prepared.is_none()
      && self.preparing_key.is_none()
      && self.building_key.is_none()
      && self.armed.is_none()
      && let Some(next) = self.next_candidate.clone()
    {
      let should_prefetch = if self.config.prefetch_eager {
        true
      } else {
        self
          .scheduler
          .remaining(self.clock())
          .is_some_and(|rem| rem <= f64::from(self.config.prefetch_lead_seconds))
      };
      if should_prefetch && self.applied_key.as_deref() != Some(self.key_of(&next).as_str()) {
        self.spawn_build(next, true);
      }
    }

    // 2. Schedule fade if a prepared next track exists.
    if self.prepared.is_some() {
      let now = self.clock();
      let (_, cancel_reason) = self.scheduler.plan(
        f64::from(self.config.transition_duration),
        self.config.transition_anchor,
        now,
      );
      self.cancel_schedule(cancel_reason);
    }

    // 3. Arm overlay slightly ahead of deadline.
    if self.armed.is_none()
      && self.prepared.is_some()
      && let Some(due) = self.scheduler.due_in(self.clock())
      && due <= self.fade_lead()
      && self.can_fade()
    {
      self.armed = FadeTransition::new(self.rect).ok();
    }

    // 4. Fire fade when deadline enters lead window.
    if self.prepared.is_some()
      && let Some(due) = self.scheduler.due_in(self.clock())
      && due <= self.fade_lead()
    {
      let Some(built) = self.prepared.take() else {
        return;
      };
      let now = self.clock();
      let deadline = self.scheduler.fire(now);
      let start_at = deadline.map(|d| {
        let delta = d - now;
        if delta >= 0.0 {
          Instant::now() + Duration::from_secs_f64(delta)
        } else {
          Instant::now()
        }
      });
      self.superseded_key = self.applied_key.clone();
      self.superseded_at = now;
      self.current = Some(built.track.clone());
      self.applied_key = Some(built.key.clone());
      self.apply_built(built, start_at);
    }
  }

  fn restore_original(&mut self) {
    self.armed = None;
    if !self.config.restore_on_stop {
      return;
    }
    if let Some(style) = &self.original_style {
      set_wallpaper_style(style);
    }
    if let Some(path) = &self.original_wallpaper {
      set_wallpaper_raw(path, true);
      self.state_store.save(None, "", "");
      tracing::info!("wallpaper-sync restored previous Windows wallpaper");
    }
  }

  fn run(&mut self, rx: Receiver<WorkerMsg>) {
    loop {
      let msg = if self.enabled {
        match rx.recv_timeout(TICK_INTERVAL) {
          Ok(m) => Some(m),
          Err(RecvTimeoutError::Timeout) => None,
          Err(RecvTimeoutError::Disconnected) => break,
        }
      } else {
        match rx.recv() {
          Ok(m) => Some(m),
          Err(_) => break,
        }
      };

      if let Some(msg) = msg {
        match msg {
          WorkerMsg::State(state) => {
            let (config, current, next) = *state;
            self.handle_state(config, current, next);
          }
          WorkerMsg::Disable => self.handle_disable(),
          WorkerMsg::Stop => break,
        }
      }

      // Coalesce any queued messages before ticking.
      while let Ok(msg) = rx.try_recv() {
        match msg {
          WorkerMsg::State(state) => {
            let (config, current, next) = *state;
            self.handle_state(config, current, next);
          }
          WorkerMsg::Disable => self.handle_disable(),
          WorkerMsg::Stop => {
            if self.enabled {
              self.restore_original();
            }
            return;
          }
        }
      }

      while let Ok(done) = self.done_rx.try_recv() {
        if self.enabled {
          self.handle_build_done(done);
        }
      }

      if self.enabled {
        self.tick();
      }
    }
    if self.enabled {
      self.restore_original();
    }
  }
}
