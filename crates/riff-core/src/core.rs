//! État du core.

use crate::config;
use crate::events::CoreEvent;
use crate::net::Http;
use crate::paths;
use crate::privacy::clean_url;
use crate::secrets::{KeyringStore, SecretStore};
use crate::services::{spotify, stream, tools, translate};
use crate::util::{now_millis, read_json_map, write_json_map};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, broadcast, watch};

pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct LyricsCacheEntry {
  pub text: String,
  pub is_custom: bool,
  pub instrumental: bool,
}

pub type SearchCacheEntry = (String, (u64, Vec<Value>));

pub type DownloadOutcome = Option<(u16, Value)>;

/// Configuration de [`Core::new`].
#[derive(Debug, Clone, Default)]
pub struct CoreConfig {
  /// Préfixe des URL locales renvoyées dans les réponses, par exemple `http://127.0.0.1:38472`.
  pub local_base_url: String,
  /// Coffre des jetons. Par défaut : le coffre du système.
  pub secret_store: Option<Arc<dyn SecretStore>>,
}

fn ensure_server_dirs() {
  translate::cleanup_i18n_dir();
  let _ = fs::create_dir_all(paths::tracks_dir());
  let _ = fs::create_dir_all(paths::custom_lyrics_dir());
  let _ = fs::create_dir_all(paths::resolve_data_dir().join("lyrics"));
  let _ = fs::create_dir_all(paths::thumbnails_dir());
  let _ = fs::create_dir_all(paths::library_dir());
}

/// Clients HTTP et proxy actif.
pub struct NetState {
  pub http_client: Http,
  pub yt_client: Http,
  pub image_client: Http,
  pub server_active_proxy: Mutex<String>,
}

/// Caches en mémoire.
pub struct CacheState {
  pub album_cache: Mutex<Vec<(String, (u64, Value))>>,
  pub hot_picks_cache: Mutex<Option<(u64, Value)>>,
  pub trending_cache: Mutex<(u64, Vec<Value>)>,
  pub artist_cache: Mutex<Vec<(String, Value)>>,
  pub vibe_cache: Mutex<HashMap<String, (u64, Vec<Value>)>>,
  pub search_cache: Mutex<Vec<SearchCacheEntry>>,
  pub dz_artist_id_cache: Mutex<HashMap<String, (u64, Option<String>)>>,
  pub yt_mix_cache: Mutex<HashMap<String, (u64, Vec<Value>)>>,
  pub stream_url_cache: Mutex<Vec<(String, (u64, Value))>>,
  pub thumb_host_fails: Mutex<HashMap<String, u64>>,
  pub lyrics_cache: Mutex<HashMap<String, LyricsCacheEntry>>,
}

/// Index de la bibliothèque et des métadonnées.
pub struct LibraryState {
  pub library_index: Mutex<Map<String, Value>>,
  pub track_metadata_index: Mutex<Map<String, Value>>,
}

/// Lecture et téléchargements en cours.
pub struct PlaybackState {
  pub stream_inflight: Mutex<HashMap<String, watch::Receiver<stream::InflightValue>>>,
  pub prefetching: Mutex<HashSet<String>>,
  pub download_inflight: Mutex<HashMap<String, watch::Receiver<DownloadOutcome>>>,
  pub current_app_state: Mutex<Value>,
}

/// Sessions et jetons des services.
pub struct AuthState {
  pub secrets: Arc<dyn SecretStore>,
  pub spotify: Mutex<spotify::SpotifyShared>,
  pub spotify_refresh: AsyncMutex<()>,
  pub musixmatch_token: Mutex<(String, u64)>,
  pub sc_client_id: Mutex<(String, u64)>,
  pub sc_refresh: AsyncMutex<()>,
}

/// Titres déjà recommandés.
pub struct DiscoveryState {
  pub recently_recommended_track_ids: Mutex<Vec<String>>,
  pub recently_recommended_keys: Mutex<Vec<String>>,
}

/// État partagé de toutes les opérations du core.
pub struct Core {
  pub net: NetState,
  pub caches: CacheState,
  pub library: LibraryState,
  pub playback: PlaybackState,
  pub auth: AuthState,
  pub discovery: DiscoveryState,
  pub tools: Mutex<tools::ToolsState>,
  events: broadcast::Sender<CoreEvent>,
  local_base_url: String,
}

impl Core {
  #[must_use]
  pub fn new(config: CoreConfig) -> Arc<Self> {
    ensure_server_dirs();
    Arc::new(Self {
      net: NetState {
        http_client: Http::service(),
        yt_client: Http::stream(),
        image_client: Http::public_image(),
        server_active_proxy: Mutex::new(String::new()),
      },
      caches: CacheState {
        album_cache: Mutex::new(Vec::new()),
        hot_picks_cache: Mutex::new(None),
        trending_cache: Mutex::new((0, Vec::new())),
        artist_cache: Mutex::new(Vec::new()),
        vibe_cache: Mutex::new(HashMap::new()),
        search_cache: Mutex::new(Vec::new()),
        dz_artist_id_cache: Mutex::new(HashMap::new()),
        yt_mix_cache: Mutex::new(HashMap::new()),
        stream_url_cache: Mutex::new(Vec::new()),
        thumb_host_fails: Mutex::new(HashMap::new()),
        lyrics_cache: Mutex::new(HashMap::new()),
      },
      library: LibraryState {
        library_index: Mutex::new(read_json_map(&paths::library_path())),
        track_metadata_index: Mutex::new(read_json_map(&paths::metadata_path())),
      },
      playback: PlaybackState {
        stream_inflight: Mutex::new(HashMap::new()),
        prefetching: Mutex::new(HashSet::new()),
        download_inflight: Mutex::new(HashMap::new()),
        current_app_state: Mutex::new(json!({})),
      },
      auth: AuthState {
        secrets: config
          .secret_store
          .unwrap_or_else(|| Arc::new(KeyringStore::default())),
        spotify: Mutex::new(spotify::SpotifyShared::default()),
        spotify_refresh: AsyncMutex::new(()),
        musixmatch_token: Mutex::new((String::new(), 0)),
        sc_client_id: Mutex::new((String::new(), 0)),
        sc_refresh: AsyncMutex::new(()),
      },
      discovery: DiscoveryState {
        recently_recommended_track_ids: Mutex::new(Vec::new()),
        recently_recommended_keys: Mutex::new(Vec::new()),
      },
      tools: Mutex::new(tools::ToolsState::new()),
      events: broadcast::channel(64).0,
      local_base_url: config.local_base_url,
    })
  }

  /// Préfixe des URL locales.
  #[must_use]
  pub fn local_base_url(&self) -> &str {
    &self.local_base_url
  }

  /// URL locale du chemin donné.
  #[must_use]
  pub fn local_url(&self, path: &str) -> String {
    format!("{}{path}", self.local_base_url)
  }

  /// Vrai si l'utilisateur a accepté d'envoyer des textes à un service de traduction (DeepL, Google). Faux par défaut.
  #[must_use]
  pub fn translation_enabled(&self) -> bool {
    config::translation_enabled()
  }

  /// Active ou coupe l'envoi de textes aux services de traduction, et retient le choix.
  pub fn set_translation_enabled(&self, enabled: bool) {
    config::set_translation_enabled(enabled);
  }

  /// Abonnement aux événements du core.
  #[must_use]
  pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
    self.events.subscribe()
  }

  pub(crate) fn publish(&self, event: CoreEvent) {
    let _ = self.events.send(event);
  }

  /// Dernier état de l'application reçu.
  #[must_use]
  pub fn current_app_state(&self) -> Value {
    self
      .playback
      .current_app_state
      .lock()
      .map(|g| g.clone())
      .unwrap_or_else(|_| json!({}))
  }

  pub fn get_album_cache(&self, key: &str) -> Option<Value> {
    let now = now_millis();
    let mut guard = self.caches.album_cache.lock().ok()?;
    if let Some(pos) = guard.iter().position(|(k, _)| k == key) {
      let (_, (expires, val)) = &guard[pos];
      if now > *expires {
        guard.remove(pos);
        return None;
      }
      return Some(val.clone());
    }
    None
  }

  pub fn set_album_cache(&self, key: &str, data: Value) {
    let expires = now_millis() + 15 * 60 * 1000;
    if let Ok(mut guard) = self.caches.album_cache.lock() {
      if let Some(pos) = guard.iter().position(|(k, _)| k == key) {
        guard.remove(pos);
      } else if guard.len() >= 100 {
        guard.remove(0);
      }
      guard.push((key.to_string(), (expires, data)));
    }
  }

  pub fn save_track_metadata_entry(&self, clean_id: &str, extra: Map<String, Value>) {
    if clean_id.is_empty() {
      return;
    }
    let mut meta_map = self.sync_metadata_from_disk();
    let mut entry = meta_map
      .get(clean_id)
      .and_then(Value::as_object)
      .cloned()
      .unwrap_or_default();
    for (k, v) in extra {
      let v = match (k.as_str(), v.as_str()) {
        ("url", Some(url)) => Value::String(clean_url(url)),
        _ => v,
      };
      entry.insert(k, v);
    }
    entry.insert("lastPlayed".to_string(), json!(now_millis()));
    meta_map.insert(clean_id.to_string(), Value::Object(entry));
    self.save_metadata(&meta_map);
  }

  pub fn remember_recommended(&self, track_id: &str, norm_key: &str) {
    if !track_id.is_empty()
      && let Ok(mut ids) = self.discovery.recently_recommended_track_ids.lock()
    {
      if !ids.iter().any(|x| x == track_id) {
        ids.push(track_id.to_string());
      }
      while ids.len() > 100 {
        ids.remove(0);
      }
    }
    if !norm_key.is_empty()
      && let Ok(mut keys) = self.discovery.recently_recommended_keys.lock()
    {
      if !keys.iter().any(|x| x == norm_key) {
        keys.push(norm_key.to_string());
      }
      while keys.len() > 100 {
        keys.remove(0);
      }
    }
  }

  pub fn is_recently_recommended(&self, track_id: &str, norm_key: &str) -> bool {
    self.is_recently_recommended_id(track_id) || self.is_recently_recommended_key(norm_key)
  }

  pub fn is_recently_recommended_id(&self, track_id: &str) -> bool {
    if !track_id.is_empty()
      && let Ok(ids) = self.discovery.recently_recommended_track_ids.lock()
    {
      return ids.iter().any(|x| x == track_id);
    }
    false
  }

  pub fn is_recently_recommended_key(&self, norm_key: &str) -> bool {
    if !norm_key.is_empty()
      && let Ok(keys) = self.discovery.recently_recommended_keys.lock()
    {
      return keys.iter().any(|x| x == norm_key);
    }
    false
  }

  pub fn sync_library_from_disk(&self) -> Map<String, Value> {
    let path = paths::library_path();
    let disk_map = read_json_map(&path);
    if let Ok(mut guard) = self.library.library_index.lock() {
      if path.exists() {
        *guard = disk_map.clone();
      }
      return guard.clone();
    }
    disk_map
  }

  pub fn save_library(&self, map: &Map<String, Value>) {
    if let Ok(mut guard) = self.library.library_index.lock() {
      *guard = map.clone();
    }
    write_json_map(&paths::library_path(), map);
  }

  pub fn sync_metadata_from_disk(&self) -> Map<String, Value> {
    let path = paths::metadata_path();
    let disk_map = read_json_map(&path);
    if let Ok(mut guard) = self.library.track_metadata_index.lock() {
      if path.exists() {
        *guard = disk_map.clone();
      }
      return guard.clone();
    }
    disk_map
  }

  pub fn save_metadata(&self, map: &Map<String, Value>) {
    if let Ok(mut guard) = self.library.track_metadata_index.lock() {
      *guard = map.clone();
    }
    write_json_map(&paths::metadata_path(), map);
  }
}
