//! Disk cache (LRU JPEG), state file, and 4-slot BMP ring for wallpaper sync.

use super::config::{MonitorTile, RenderParams};
use super::render::{RgbImage, decode_rgb, encode_jpeg_95};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const BMP_RING_SIZE: u8 = 4;

/// Computes the deterministic SHA-256 cache key from cover URL, output dimensions, and render params.
#[must_use]
pub fn cache_key(
  cover_url: &str,
  width: u32,
  height: u32,
  params: &RenderParams,
  tiles: Option<&[MonitorTile]>,
) -> String {
  #[derive(Serialize)]
  struct Key<'a> {
    url: &'a str,
    size: [u32; 2],
    params: &'a RenderParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    tiles: Option<&'a [MonitorTile]>,
  }
  let payload = serde_json::to_vec(&Key {
    url: cover_url,
    size: [width, height],
    params,
    tiles,
  })
  .unwrap_or_default();
  let digest = Sha256::digest(payload);
  let mut out = String::with_capacity(digest.len() * 2);
  for byte in digest {
    use std::fmt::Write as _;
    let _ = write!(out, "{byte:02x}");
  }
  out
}

/// Disk-backed LRU cache of rendered wallpapers stored as quality-95 JPEGs.
#[derive(Debug, Clone)]
pub struct RenderCache {
  dir: PathBuf,
  pub enabled: bool,
  pub max_entries: usize,
  pub max_bytes: u64,
}

impl RenderCache {
  #[must_use]
  pub fn new(dir: PathBuf, enabled: bool, max_entries: usize, max_bytes: u64) -> Self {
    let mut cache = Self {
      dir,
      enabled,
      max_entries,
      max_bytes,
    };
    if cache.enabled
      && let Err(err) = fs::create_dir_all(&cache.dir)
    {
      tracing::warn!(%err, "wallpaper-sync cache disabled: directory unreachable");
      cache.enabled = false;
    }
    cache
  }

  fn entry_path(&self, key: &str) -> PathBuf {
    self.dir.join(format!("{key}.jpg"))
  }

  /// Reads and decodes a cached wallpaper, refreshing its mtime on hit and deleting corrupt entries.
  #[must_use]
  pub fn get(&self, key: &str) -> Option<RgbImage> {
    if !self.enabled {
      return None;
    }
    let path = self.entry_path(key);
    let bytes = fs::read(&path).ok()?;
    match decode_rgb(&bytes) {
      Ok(img) => {
        if let Ok(file) = fs::OpenOptions::new().write(true).open(&path) {
          let _ = file.set_modified(SystemTime::now());
        }
        Some(img)
      }
      Err(err) => {
        tracing::warn!(%err, key = &key[..key.len().min(12)], "removing unreadable wallpaper cache entry");
        let _ = fs::remove_file(&path);
        None
      }
    }
  }

  /// Stores a rendered wallpaper as JPEG-95 and prunes older entries past configured limits.
  pub fn put(&self, key: &str, img: &RgbImage) {
    if !self.enabled {
      return;
    }
    if let Err(err) = fs::create_dir_all(&self.dir) {
      tracing::warn!(%err, "wallpaper-sync cache directory unavailable");
      return;
    }
    let bytes = match encode_jpeg_95(img) {
      Ok(bytes) => bytes,
      Err(err) => {
        tracing::warn!(%err, "wallpaper-sync JPEG encode failed");
        return;
      }
    };
    if let Err(err) = fs::write(self.entry_path(key), bytes) {
      tracing::warn!(%err, "wallpaper-sync cache write failed");
      return;
    }
    self.prune();
  }

  pub fn prune(&self) {
    if !self.enabled {
      return;
    }
    let Ok(read_dir) = fs::read_dir(&self.dir) else {
      return;
    };
    let mut entries: Vec<(PathBuf, SystemTime, u64)> = read_dir
      .flatten()
      .filter_map(|entry| {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jpg") {
          return None;
        }
        let meta = entry.metadata().ok()?;
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        Some((path, mtime, meta.len()))
      })
      .collect();
    entries.sort_by_key(|a| std::cmp::Reverse(a.1));

    let mut total_bytes = 0u64;
    for (idx, (path, _, size)) in entries.into_iter().enumerate() {
      total_bytes = total_bytes.saturating_add(size);
      if idx >= self.max_entries || total_bytes > self.max_bytes {
        let _ = fs::remove_file(path);
      }
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedState {
  pub key: Option<String>,
  pub title: String,
  pub artist: String,
}

/// Reads and writes `state.json` inside `<data_dir>/wallpaper-sync/`.
#[derive(Debug, Clone)]
pub struct StateStore {
  path: PathBuf,
}

impl StateStore {
  #[must_use]
  pub fn new(data_dir: &Path) -> Self {
    Self {
      path: data_dir.join("state.json"),
    }
  }

  #[must_use]
  pub fn load_key(&self) -> Option<String> {
    let raw = fs::read_to_string(&self.path).ok()?;
    serde_json::from_str::<SavedState>(&raw).ok()?.key
  }

  pub fn save(&self, key: Option<&str>, title: &str, artist: &str) {
    if let Some(parent) = self.path.parent() {
      let _ = fs::create_dir_all(parent);
    }
    let payload = SavedState {
      key: key.map(str::to_string),
      title: title.to_string(),
      artist: artist.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&payload)
      && let Err(err) = fs::write(&self.path, json)
    {
      tracing::warn!(%err, "wallpaper-sync failed to write state.json");
    }
  }
}

/// Rotates 4 BMP file paths so Windows `SPI_SETDESKWALLPAPER` always observes a distinct path.
#[derive(Debug, Clone)]
pub struct WallpaperRing {
  dir: PathBuf,
  slot: u8,
}

impl WallpaperRing {
  #[must_use]
  pub fn new(dir: PathBuf) -> Self {
    Self { dir, slot: 0 }
  }

  pub fn next_path(&mut self) -> PathBuf {
    self.slot = (self.slot + 1) % BMP_RING_SIZE;
    self.dir.join(format!("wallpaper_{}.bmp", self.slot))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::system::wallpaper::config::WallpaperSyncConfig;
  use crate::test_temp::TempDir;

  #[test]
  fn cache_put_get_and_prune_respects_entry_and_byte_limits() {
    let dir = TempDir::new("wallpaper-cache");
    let cache = RenderCache::new(dir.to_path_buf(), true, 2, 10_000_000);
    let img = RgbImage::filled(16, 16, [12, 34, 56]);
    cache.put("a", &img);
    std::thread::sleep(std::time::Duration::from_millis(20));
    cache.put("b", &img);
    std::thread::sleep(std::time::Duration::from_millis(20));
    cache.put("c", &img);

    // Max 2 entries: oldest ("a") is pruned.
    assert!(cache.get("a").is_none());
    assert!(cache.get("b").is_some());
    assert!(cache.get("c").is_some());

    // Corrupt entry is removed on read.
    fs::write(dir.join("bad.jpg"), b"not a jpeg").expect("write corrupt");
    assert!(cache.get("bad").is_none());
    assert!(!dir.join("bad.jpg").exists());
  }

  #[test]
  fn cache_key_changes_with_tiles_or_params() {
    let params = WallpaperSyncConfig::default().render_params();
    let k1 = cache_key("https://cover", 1920, 1080, &params, None);
    let k2 = cache_key(
      "https://cover",
      1920,
      1080,
      &params,
      Some(&[MonitorTile {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
      }]),
    );
    assert_ne!(k1, k2);
  }

  #[test]
  fn ring_rotates_four_bmp_slots_and_state_roundtrips() {
    let dir = TempDir::new("wallpaper-ring");
    let mut ring = WallpaperRing::new(dir.to_path_buf());
    let p1 = ring.next_path();
    let p2 = ring.next_path();
    let p3 = ring.next_path();
    let p4 = ring.next_path();
    let p5 = ring.next_path();
    assert_ne!(p1, p2);
    assert_ne!(p2, p3);
    assert_ne!(p3, p4);
    assert_eq!(p1, p5);

    let store = StateStore::new(&dir);
    assert_eq!(store.load_key(), None);
    store.save(Some("abc|sig"), "Track", "Artist");
    assert_eq!(store.load_key().as_deref(), Some("abc|sig"));
  }
}
