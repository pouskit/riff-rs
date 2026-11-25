//! Écrans de gestion : outils, imports, comptes, stockage. Les vues sont dans `screens/manage.rs`.

use crate::app::{App, Effect};
use crate::import_report::ImportReport;
use crate::settings::{LegacyStorage, NativeSettings};
use crate::ui::{Modal, ReplyTag};
use crate::user_data::{Playlist, Track, UserData};
use crate::widgets::toast::ToastKind;
use riff_core::import_files::{ExportSource, FilesRead, ImportError, Library};
use riff_core::util::now_millis;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// Intervalle de lecture de l'état de l'installation.
pub const TOOLS_POLL: Duration = Duration::from_millis(500);
/// Intervalle de lecture de la session Last.fm pendant l'autorisation.
pub const LASTFM_POLL: Duration = Duration::from_secs(3);
/// Essais de lecture de la session : 2 minutes.
const LASTFM_POLL_MAX: u32 = 40;
/// `onLastFmTimeUpdate` : seuils du scrobble.
const SCROBBLE_MIN: f64 = 25.0;
const SCROBBLE_MAX: f64 = 240.0;
const SCROBBLE_UNKNOWN: f64 = 180.0;
const NOW_PLAYING_AFTER: f64 = 1.0;
/// Écart maximal entre deux positions comptées comme une lecture continue.
const MAX_TICK: f64 = 2.5;
/// Textes de toast, clés du dictionnaire de traduction.
const LASTFM_SECRET_NOT_SAVED: &str =
  "Last.fm API secret could not be saved securely; enter it again after restarting Riff.";
const LASTFM_SESSION_NOT_SAVED: &str =
  "Last.fm login could not be saved securely; reconnect after restarting Riff.";
const NEW_TARGET: &str = "__new__";
const FAVORITES_TARGET: &str = "__favorites__";

/// État lu dans `/api/tools-status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolsStatus {
  pub ytdlp: bool,
  pub ffmpeg: bool,
  pub ffmpeg_optional: bool,
  /// `idle`, `downloading`, `done` ou `error`.
  pub status: String,
  /// Outil en cours d'installation.
  pub tool: String,
  pub percent: u32,
  pub error: String,
  pub ffmpeg_error: String,
}

impl ToolsStatus {
  /// Lit la réponse du core.
  #[must_use]
  pub fn parse(value: &Value) -> Self {
    let text = |source: &Value, key: &str| {
      source
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
    };
    let progress = value.get("progress").cloned().unwrap_or(Value::Null);
    Self {
      ytdlp: value.get("ytdlp").and_then(Value::as_bool).unwrap_or(false),
      ffmpeg: value
        .get("ffmpeg")
        .and_then(Value::as_bool)
        .unwrap_or(false),
      ffmpeg_optional: value
        .get("ffmpegOptional")
        .and_then(Value::as_bool)
        .unwrap_or(false),
      status: text(&progress, "status"),
      tool: text(&progress, "tool"),
      percent: progress
        .get("percent")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(100) as u32,
      error: text(&progress, "error"),
      ffmpeg_error: text(&progress, "ffmpegError"),
    }
  }

  /// Tout ce qu'il faut est installé.
  #[must_use]
  pub const fn is_ready(&self) -> bool {
    self.ytdlp && (self.ffmpeg || self.ffmpeg_optional)
  }

  /// Message d'erreur à afficher.
  #[must_use]
  pub fn failure(&self) -> Option<&str> {
    [self.error.as_str(), self.ffmpeg_error.as_str()]
      .into_iter()
      .find(|text| !text.is_empty())
  }

  /// Avancement d'un outil, de 0 à 100.
  #[must_use]
  pub fn progress_of(&self, tool: &str, installed: bool) -> u32 {
    if installed {
      100
    } else if self.tool == tool {
      self.percent
    } else {
      0
    }
  }
}

/// État de la fenêtre des outils.
#[derive(Debug, Clone, Default)]
pub struct ToolsUi {
  pub status: ToolsStatus,
  /// L'installation est lancée : l'état est relu jusqu'à sa fin.
  pub installing: bool,
}

/// Source d'un import de playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
  Spotify,
  Apple,
  Link,
  Yandex,
}

impl ImportKind {
  /// Route du core.
  #[must_use]
  pub const fn route(self) -> &'static str {
    match self {
      Self::Spotify => "/api/spotify-import",
      Self::Apple => "/api/apple-import",
      Self::Link => "/api/playlist-import",
      Self::Yandex => "/api/yandex-import",
    }
  }

  /// Titre de la fenêtre.
  #[must_use]
  pub const fn title(self) -> &'static str {
    match self {
      Self::Spotify => "Import from Spotify",
      Self::Apple => "Import from Apple Music",
      Self::Link => "Import from a link",
      Self::Yandex => "Import from Yandex Music",
    }
  }

  /// Format des fichiers d'export, pour les sources qui en ont.
  #[must_use]
  pub const fn export_source(self) -> Option<ExportSource> {
    match self {
      Self::Spotify => Some(ExportSource::Spotify),
      Self::Apple => Some(ExportSource::Apple),
      Self::Link | Self::Yandex => None,
    }
  }

  /// Consigne de l'onglet « Export files » (sources qui en ont).
  #[must_use]
  pub const fn file_hint(self) -> &'static str {
    match self {
      Self::Spotify => {
        "Spotify → Account → Privacy → Download your data. When the archive arrives, pick Playlist1.json and YourLibrary.json here. No login needed."
      }
      Self::Apple => {
        "Go to privacy.apple.com > Request a copy of your data > Apple Music activity, unzip the archive, then pick the playlist/library files here. No login needed."
      }
      Self::Link | Self::Yandex => "",
    }
  }

  /// Texte d'aide du champ.
  #[must_use]
  pub const fn hint(self) -> &'static str {
    match self {
      Self::Spotify => "Spotify playlist link",
      Self::Apple => "Apple Music playlist or album link",
      Self::Link => "Deezer, SoundCloud, YouTube or YouTube Music link",
      Self::Yandex => "Yandex Music playlist or album link",
    }
  }
}

/// État d'une action longue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Progress {
  #[default]
  Idle,
  Running,
  Done(String),
  Failed(String),
}

/// Onglet de la fenêtre d'import : « Playlist link » ou « Export files ».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImportMode {
  #[default]
  Link,
  File,
}

/// Fenêtre d'import.
#[derive(Debug, Clone, Default)]
pub struct ImportUi {
  pub url: String,
  /// `__new__`, `__favorites__` ou identifiant de playlist.
  pub target: String,
  pub progress: Progress,
  pub mode: ImportMode,
  /// Noms des fichiers choisis.
  pub files: Vec<String>,
  /// Contenu des fichiers choisis (`parsedSpotifyExport`, `parsedAppleExport`).
  pub export: Library,
  /// Cases « Liked songs » et « Playlists ».
  pub likes: bool,
  pub playlists: bool,
}

/// Compte Spotify.
#[derive(Debug, Clone, Default)]
pub struct SpotifyUi {
  pub connected: bool,
  pub progress: Progress,
  /// Adresse à enregistrer côté Spotify, sans le port.
  pub redirect: String,
}

/// Compte Last.fm.
#[derive(Debug, Clone, Default)]
pub struct LastfmUi {
  pub api_key: String,
  pub api_secret: String,
  pub progress: Progress,
  pub token: Option<String>,
  pub polls: u32,
  pub connected: bool,
  pub scrobble: ScrobbleState,
}

/// Compte Google.
#[derive(Debug, Clone, Default)]
pub struct GoogleUi {
  pub enabled: bool,
  pub connected: bool,
  pub progress: Progress,
  pub ytmusic: bool,
}

/// Session YouTube de lecture.
#[derive(Debug, Clone, Default)]
pub struct YoutubeUi {
  pub signed_in: bool,
  pub progress: Progress,
}

/// Stockage et avancé.
#[derive(Debug, Clone, Default)]
pub struct StorageUi {
  pub cache_bytes: Option<u64>,
  pub data_dir: String,
  pub download_dir: String,
  pub progress: Progress,
  pub proxy_input: String,
  pub proxy_test: Progress,
  pub yt_dlp_auto_update: bool,
  pub multi_instance: bool,
  /// Le dossier de données a changé : il faut relancer l'application.
  pub restart_needed: bool,
}

/// État des écrans de gestion.
#[derive(Debug, Clone, Default)]
pub struct ManageUi {
  pub tools: ToolsUi,
  pub import: ImportUi,
  pub spotify: SpotifyUi,
  pub lastfm: LastfmUi,
  pub google: GoogleUi,
  pub youtube: YoutubeUi,
  pub storage: StorageUi,
}

/// Réponses du core attendues par ces écrans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManageReply {
  ToolsStatus,
  ToolsStarted,
  Import(ImportKind),
  SpotifyStatus,
  SpotifyLogout,
  SpotifyLibrary,
  GoogleStatus,
  GoogleLogout,
  GoogleLibrary,
  LastfmToken,
  LastfmSession,
  LastfmSent,
  ProxyConfig,
  ProxyTest,
}

/// Résultat d'une action hors du fil d'interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalInfo {
  pub cache_bytes: u64,
  pub data_dir: String,
  pub download_dir: String,
  pub yt_signed_in: bool,
  pub yt_dlp_auto_update: bool,
  pub multi_instance: bool,
  pub lastfm_connected: bool,
  pub lastfm_has_secret: bool,
}

/// Dossier à changer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
  /// Dossier de données : `move_data` copie les données existantes.
  Data {
    move_data: bool,
  },
  Download,
}

/// Valeur Last.fm que le coffre n'a pas pu enregistrer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastfmVaultItem {
  ApiSecret,
  Session,
}

/// Messages de ces écrans.
#[derive(Debug, Clone)]
pub enum ManageMessage {
  OpenTools,
  InstallTools,
  PollTools,
  // Imports.
  OpenImport(ImportKind),
  ImportUrl(String),
  ImportTarget(String),
  ImportStart(ImportKind),
  ImportMode(ImportMode),
  ImportFilesPick(ImportKind),
  /// Fichiers lus ; `None` si le choix est annulé.
  ImportFilesRead(Option<Box<FilesRead>>),
  ImportLikes(bool),
  ImportPlaylists(bool),
  ImportJsonPick,
  ImportJsonRead(Result<Option<String>, String>),
  ExportPlaylists,
  // Spotify.
  SpotifyClientId(String),
  SpotifyConnect,
  SpotifyStarted(Result<(String, String), String>),
  SpotifyLogout,
  SpotifyImportLibrary,
  // Last.fm.
  LastfmKey(String),
  LastfmSecret(String),
  LastfmConnect,
  LastfmPoll,
  LastfmLogout,
  LastfmToggle(bool),
  /// Le coffre a refusé une valeur Last.fm.
  LastfmVaultFailed(LastfmVaultItem),
  // Google.
  GoogleConnect,
  GoogleStarted(Result<String, String>),
  GoogleLogout,
  GoogleImportLibrary,
  GoogleMusic(bool),
  // YouTube.
  CookiesPick,
  CookiesPicked(Result<(), String>),
  CookiesBrowser(String),
  CookiesDelete,
  // Stockage.
  RefreshLocal,
  LocalLoaded(Box<LocalInfo>),
  ClearCache,
  ChangeFolder(FolderKind),
  ResetDownloadFolder,
  FolderChanged(FolderKind, Result<String, String>),
  OpenLogs,
  CopyDiagnostics,
  ExportAccount,
  ExportedAccount(Result<String, String>),
  ImportAccount,
  AccountRead(Result<Option<Value>, String>),
  YtDlpAutoUpdate(bool),
  /// Interrupteur « Allow multiple instances » ; la réponse de l'écriture revient par `MultiInstanceSaved`.
  MultiInstance(bool),
  /// Échec : la valeur relue dans `startup.json`.
  MultiInstanceSaved(Result<(), bool>),
  ProxyInput(String),
  ProxyAdd,
  ProxyRemove(String),
  ProxySelect(String),
  ProxyToggle(bool),
  ProxyTest,
}

fn get(reply: ManageReply, path: &'static str, query: Vec<(String, String)>) -> Effect {
  Effect::CoreJson {
    tag: ReplyTag::Manage(reply),
    path,
    query,
    body: None,
    search: false,
  }
}

fn post(reply: ManageReply, path: &'static str, body: &Value) -> Effect {
  Effect::CoreJson {
    tag: ReplyTag::Manage(reply),
    path,
    query: Vec::new(),
    body: Some(body.to_string()),
    search: false,
  }
}

fn toast(text: &str) -> Effect {
  Effect::Toast(text.to_string(), ToastKind::Info)
}

fn failure_of(value: &Value, fallback: &str) -> String {
  value["error"]
    .as_str()
    .filter(|error| !error.is_empty())
    .unwrap_or(fallback)
    .to_string()
}

fn tracks_of(value: &Value) -> Vec<Track> {
  value
    .as_array()
    .map(|items| {
      items
        .iter()
        .filter_map(|item| serde_json::from_value(item.clone()).ok())
        .collect()
    })
    .unwrap_or_default()
}

/// Avancement du scrobble du titre en cours (`onLastFmTimeUpdate`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrobbleState {
  pub key: String,
  pub started_at: u64,
  pub played: f64,
  pub last_position: f64,
  pub now_playing_sent: bool,
  pub scrobbled: bool,
}

/// Ce que le passage d'une position demande.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrobbleStep {
  pub now_playing: bool,
  pub scrobble: bool,
}

impl ScrobbleState {
  /// Avance avec une position de lecture. `duration` à 0 : inconnue, 180 s supposées.
  pub fn tick(&mut self, key: &str, position: f64, duration: f64, now_secs: u64) -> ScrobbleStep {
    if self.key != key {
      *self = Self {
        key: key.to_string(),
        started_at: now_secs,
        last_position: position,
        ..Self::default()
      };
    }
    let mut step = ScrobbleStep::default();
    if !self.now_playing_sent && position > NOW_PLAYING_AFTER {
      self.now_playing_sent = true;
      step.now_playing = true;
    }
    let delta = position - self.last_position;
    self.last_position = position;
    if delta > 0.0 && delta < MAX_TICK {
      self.played += delta;
    }
    if self.scrobbled {
      // Le titre recommence : `onLastFmTrackStart`.
      if position < 3.0 && self.played > SCROBBLE_MIN {
        *self = Self {
          key: key.to_string(),
          started_at: now_secs,
          last_position: position,
          ..Self::default()
        };
      }
      return step;
    }
    let effective = if duration > 0.0 {
      duration
    } else {
      SCROBBLE_UNKNOWN
    };
    if effective < SCROBBLE_MIN {
      return step;
    }
    if self.played >= (effective * 0.5).clamp(SCROBBLE_MIN, SCROBBLE_MAX) {
      self.scrobbled = true;
      step.scrobble = true;
    }
    step
  }
}

/// Playlists d'un fichier JSON : `{ "playlists": [...] }`, ou un tableau.
#[must_use]
pub fn playlists_from_json(raw: &str) -> Vec<Playlist> {
  let Ok(value) = serde_json::from_str::<Value>(raw) else {
    return Vec::new();
  };
  let items = value.get("playlists").cloned().unwrap_or(value);
  items
    .as_array()
    .map(|items| {
      items
        .iter()
        .filter_map(|item| serde_json::from_value(item.clone()).ok())
        .collect()
    })
    .unwrap_or_default()
}

/// Taille lisible.
#[must_use]
pub fn human_size(bytes: u64) -> String {
  const MB: f64 = 1024.0 * 1024.0;
  if bytes as f64 >= 1024.0 * MB {
    format!("{:.1} GB", bytes as f64 / (1024.0 * MB))
  } else {
    format!("{:.1} MB", bytes as f64 / MB)
  }
}

impl App {
  /// Au lancement : la fenêtre s'ouvre si un outil manque. Lecture locale, sans réseau.
  pub(crate) fn startup_tools(&self) -> Vec<Effect> {
    vec![
      get(ManageReply::ToolsStatus, "/api/tools-status", Vec::new()),
      get(ManageReply::GoogleStatus, "/api/google-status", Vec::new()),
      Effect::LoadLocalInfo,
    ]
  }

  /// Lectures locales à l'ouverture de la page de réglages. `spotify-status` peut rafraîchir un jeton : seulement ici.
  pub(crate) fn settings_opened(&self) -> Vec<Effect> {
    vec![
      Effect::LoadLocalInfo,
      get(ManageReply::GoogleStatus, "/api/google-status", Vec::new()),
      get(
        ManageReply::SpotifyStatus,
        "/api/spotify-status",
        Vec::new(),
      ),
    ]
  }

  fn add_imported(
    &mut self,
    name: &str,
    cover: Option<&str>,
    tracks: Vec<Track>,
    target: &str,
  ) -> usize {
    let count = tracks.len();
    match target {
      FAVORITES_TARGET => {
        let platform = self.data.platform.clone();
        let mut known: std::collections::HashSet<String> = self
          .data
          .favorites
          .iter()
          .map(|track| track.canonical_id(&platform))
          .collect();
        // Comme `some(isSameTrack)` de Riff, un doublon à l'intérieur du lot compte aussi.
        let fresh = tracks
          .into_iter()
          .filter(|track| known.insert(track.canonical_id(&platform)));
        self.data.favorites.extend(fresh);
      }
      NEW_TARGET | "" => {
        let unique = self.next_unique();
        let id = self
          .data
          .create_playlist(name, tracks, now_millis() as i64, unique);
        if let Some(cover) = cover.filter(|cover| !cover.is_empty())
          && id.is_some()
          && let Some(playlist) = self
            .data
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id == id)
        {
          playlist.cover = Some(cover.to_string());
        }
      }
      id => {
        let platform = self.data.platform.clone();
        if let Some(playlist) = self
          .data
          .playlists
          .iter_mut()
          .find(|playlist| playlist.id.as_deref() == Some(id))
        {
          let known: Vec<String> = playlist
            .tracks
            .iter()
            .map(|track| track.canonical_id(&platform))
            .collect();
          playlist.tracks.extend(
            tracks
              .into_iter()
              .filter(|track| !known.contains(&track.canonical_id(&platform))),
          );
        }
      }
    }
    count
  }

  /// `handleSpotifyFileImport` et `handleAppleImportAction` (mode fichier). Pas de rapprochement
  /// par recherche : un titre sans identifiant est cherché à la lecture.
  fn import_exported_files(&mut self, kind: ImportKind) -> Vec<Effect> {
    let import = &self.ui.manage.import;
    let export = &import.export;
    let (likes, playlists, missing) = if kind == ImportKind::Apple {
      let (has_likes, has_playlists) = (!export.likes.is_empty(), !export.playlists.is_empty());
      if has_likes && has_playlists {
        (
          import.likes,
          import.playlists,
          "No tracks selected or found to import",
        )
      } else {
        (
          has_likes,
          has_playlists,
          "No tracks selected or found to import",
        )
      }
    } else if !import.likes && !import.playlists {
      let message = self.i18n.t("Select at least Liked songs or Playlists");
      self.ui.manage.import.progress = Progress::Failed(message);
      return Vec::new();
    } else {
      (
        import.likes,
        import.playlists,
        "No tracks found in the selected files",
      )
    };
    if export.track_count(likes, playlists) == 0 {
      let message = self.i18n.t(missing);
      self.ui.manage.import.progress = Progress::Failed(message);
      return Vec::new();
    }
    let value = export.to_value(likes, playlists);
    let (liked, lists) = self.apply_library(&value);
    let message = format!(
      "{liked} {}, {lists} {}",
      self.i18n.t("liked tracks"),
      self.i18n.t("playlists")
    );
    let import = &mut self.ui.manage.import;
    import.export = Library::default();
    import.files.clear();
    import.progress = Progress::Done(message);
    vec![self.data_changed()]
  }

  /// `apple-file-summary` : ce que contiennent les fichiers choisis.
  #[must_use]
  pub fn export_summary(&self) -> Option<String> {
    let import = &self.ui.manage.import;
    if import.files.is_empty() {
      return None;
    }
    let t = |key: &str| self.i18n.t(key);
    let (likes, lists) = (import.export.likes.len(), import.export.playlists.len());
    Some(match (likes > 0, lists > 0) {
      (true, true) => format!(
        "{} {likes} {} {} {lists} {}",
        t("Found"),
        t("liked songs"),
        t("and"),
        t("playlists")
      ),
      (false, true) => format!(
        "{} {lists} {} ({} {})",
        t("Found"),
        t("playlists"),
        import.export.track_count(false, true),
        t("tracks")
      ),
      (true, false) => format!("{} {likes} {}", t("Found"), t("liked tracks")),
      (false, false) => t("No tracks found in selected files"),
    })
  }

  fn apply_library(&mut self, value: &Value) -> (usize, usize) {
    let likes = tracks_of(&value["likes"]);
    let liked = likes.len();
    self.add_imported("", None, likes, FAVORITES_TARGET);
    let mut lists = 0;
    for playlist in value["playlists"].as_array().into_iter().flatten() {
      let tracks = tracks_of(&playlist["tracks"]);
      if tracks.is_empty() {
        continue;
      }
      let name = playlist["name"].as_str().unwrap_or("Playlist");
      self.add_imported(name, playlist["cover"].as_str(), tracks, NEW_TARGET);
      lists += 1;
    }
    (liked, lists)
  }

  fn lastfm_payload(&self) -> Value {
    json!({})
  }

  fn lastfm_track_payload(&self, track: &Track, timestamp: Option<u64>, duration: f64) -> Value {
    let mut payload = self.lastfm_payload();
    payload["artist"] = json!(track.artist.as_deref().unwrap_or(""));
    payload["track"] = json!(track.title.as_deref().unwrap_or(""));
    payload["album"] = json!(track.album.as_deref().unwrap_or(""));
    payload["duration"] = json!(duration);
    if let Some(timestamp) = timestamp {
      payload["timestamp"] = json!(timestamp);
    }
    payload
  }

  /// Scrobble : appelé à chaque position de lecture.
  pub(crate) fn lastfm_tick(&mut self, position: f64, duration: f64) -> Vec<Effect> {
    if !self.settings.lastfm_enabled || !self.ui.manage.lastfm.connected {
      return Vec::new();
    }
    let Some(track) = self.player.current.clone() else {
      return Vec::new();
    };
    if track.title.as_deref().is_none_or(str::is_empty)
      || track.artist.as_deref().is_none_or(str::is_empty)
    {
      return Vec::new();
    }
    let key = track.canonical_id(&self.data.platform);
    let step = self
      .ui
      .manage
      .lastfm
      .scrobble
      .tick(&key, position, duration, now_millis() / 1000);
    let mut effects = Vec::new();
    if step.now_playing {
      effects.push(post(
        ManageReply::LastfmSent,
        "/api/lastfm/now-playing",
        &self.lastfm_track_payload(&track, None, duration),
      ));
    }
    if step.scrobble {
      let started = self.ui.manage.lastfm.scrobble.started_at;
      effects.push(post(
        ManageReply::LastfmSent,
        "/api/lastfm/scrobble",
        &self.lastfm_track_payload(&track, Some(started), duration),
      ));
    }
    effects
  }

  /// Fin d'une connexion annoncée par le core.
  pub(crate) fn login_event(&mut self, event: &riff_core::events::CoreEvent) -> Vec<Effect> {
    match event {
      riff_core::events::CoreEvent::SpotifyLogin { connected, error } => {
        let spotify = &mut self.ui.manage.spotify;
        spotify.connected = *connected;
        spotify.progress = if error.is_empty() {
          Progress::Idle
        } else {
          Progress::Failed(error.clone())
        };
      }
      riff_core::events::CoreEvent::GoogleLogin { connected, error } => {
        let google = &mut self.ui.manage.google;
        google.connected = *connected;
        google.progress = if error.is_empty() {
          Progress::Idle
        } else {
          Progress::Failed(error.clone())
        };
      }
      riff_core::events::CoreEvent::AppState(_) => {}
    }
    Vec::new()
  }

  /// Remplace les données et les réglages par ceux d'un fichier de compte. Rend le rapport d'import.
  fn apply_account(&mut self, value: &Value) -> Result<ImportReport, String> {
    let store = value.get("localStorage").unwrap_or(value);
    let entries = store.as_object().ok_or("Failed to read account file")?;
    let storage = LegacyStorage::from_entries(entries);
    let keep_window = self.settings.screen;
    self.settings = NativeSettings::imported_from(&storage);
    self.settings.screen = keep_window;
    crate::motion::apply_settings(self.settings.reduce_motion, self.settings.perf_mode);
    self.data = UserData::imported_from(&storage, now_millis() as i64);
    self.player.shuffle = self.data.shuffle;
    self.player.scope = None;
    self.player.repeat = self.data.repeat;
    self.player.crossfade = self.data.crossfade;
    self.refresh_theme();
    self.refresh_orders();
    // Un compte n'apporte aucun jeton : ce que `storage` en contient n'est écrit nulle part.
    let mut report = ImportReport::new(&storage);
    report.mark_secrets_to_reenter();
    Ok(report)
  }

  /// Fichier de compte : `app`, `version`, `exportedAt`, `localStorage`, comme Riff. Aucun jeton.
  #[must_use]
  pub fn account_payload(&self) -> Value {
    // `legacy` d'abord : une clé connue l'emporte.
    let mut entries = self.settings.legacy.clone();
    entries.extend(self.data.to_legacy_entries());
    entries.extend(self.settings.to_legacy_entries());
    json!({ "app": "Riff", "version": "1.0", "exportedAt": now_millis(), "localStorage": entries })
  }

  fn proxy_effect(&self) -> Effect {
    let url = self.settings.proxy_active.clone().unwrap_or_default();
    post(
      ManageReply::ProxyConfig,
      "/api/proxy-config",
      &json!({ "enabled": self.settings.proxy_enabled && !url.is_empty(), "proxyUrl": url }),
    )
  }

  /// Le proxy enregistré est réappliqué au lancement.
  pub(crate) fn startup_proxy(&self) -> Vec<Effect> {
    if self.settings.proxy_enabled && self.settings.proxy_active.is_some() {
      vec![self.proxy_effect()]
    } else {
      Vec::new()
    }
  }

  fn save(&self) -> Effect {
    Effect::SaveSettings(Box::new(self.settings.clone()))
  }

  /// Messages des écrans de gestion.
  pub fn update_manage(&mut self, message: ManageMessage) -> Vec<Effect> {
    match message {
      ManageMessage::OpenTools => {
        let mut effects = self.open_modal(Modal::Tools);
        effects.push(get(
          ManageReply::ToolsStatus,
          "/api/tools-status",
          Vec::new(),
        ));
        effects
      }
      ManageMessage::InstallTools => {
        self.ui.manage.tools.installing = true;
        self.ui.manage.tools.status.error.clear();
        self.ui.manage.tools.status.ffmpeg_error.clear();
        vec![get(
          ManageReply::ToolsStarted,
          "/api/tools-install",
          Vec::new(),
        )]
      }
      ManageMessage::PollTools => vec![get(
        ManageReply::ToolsStatus,
        "/api/tools-status",
        Vec::new(),
      )],
      ManageMessage::OpenImport(kind) => {
        self.ui.manage.import = ImportUi {
          target: NEW_TARGET.to_string(),
          likes: true,
          playlists: true,
          ..ImportUi::default()
        };
        self.open_modal(Modal::Import(kind))
      }
      ManageMessage::ImportMode(mode) => {
        self.ui.manage.import.mode = mode;
        Vec::new()
      }
      ManageMessage::ImportLikes(checked) => {
        self.ui.manage.import.likes = checked;
        Vec::new()
      }
      ManageMessage::ImportPlaylists(checked) => {
        self.ui.manage.import.playlists = checked;
        Vec::new()
      }
      ManageMessage::ImportFilesPick(kind) => kind
        .export_source()
        .map(Effect::PickExportFiles)
        .into_iter()
        .collect(),
      ManageMessage::ImportFilesRead(None) => Vec::new(),
      ManageMessage::ImportFilesRead(Some(read)) => {
        let FilesRead {
          names,
          library,
          errors,
        } = *read;
        let import = &mut self.ui.manage.import;
        import.files = names;
        import.export = library;
        // Riff recoche les deux cases à chaque choix de fichiers Apple, pas Spotify.
        if matches!(self.ui.modal, Some(Modal::Import(ImportKind::Apple))) {
          import.likes = true;
          import.playlists = true;
        }
        import.progress = if errors
          .iter()
          .any(|(_, error)| *error == ImportError::TooLarge)
        {
          Progress::Failed(self.i18n.t("File too large (20 MB max)"))
        } else {
          Progress::Idle
        };
        Vec::new()
      }
      ManageMessage::ImportUrl(url) => {
        self.ui.manage.import.url = url;
        Vec::new()
      }
      ManageMessage::ImportTarget(target) => {
        self.ui.manage.import.target = target;
        Vec::new()
      }
      ManageMessage::ImportStart(kind)
        if kind.export_source().is_some() && self.ui.manage.import.mode == ImportMode::File =>
      {
        self.import_exported_files(kind)
      }
      ManageMessage::ImportStart(kind) => {
        let url = self.ui.manage.import.url.trim().to_string();
        if url.is_empty() || self.ui.manage.import.progress == Progress::Running {
          return Vec::new();
        }
        self.ui.manage.import.progress = Progress::Running;
        vec![get(
          ManageReply::Import(kind),
          kind.route(),
          vec![("url".to_string(), url)],
        )]
      }
      ManageMessage::ImportJsonPick => vec![Effect::PickPlaylistsJson],
      ManageMessage::ImportJsonRead(Ok(None)) => Vec::new(),
      ManageMessage::ImportJsonRead(Ok(Some(raw))) => {
        let playlists = playlists_from_json(&raw);
        if playlists.is_empty() {
          return vec![Effect::Toast(
            "No playlists found in this file".to_string(),
            ToastKind::Error,
          )];
        }
        for playlist in &playlists {
          let name = playlist.name.as_deref().unwrap_or("Playlist");
          self.add_imported(
            name,
            playlist.cover.as_deref(),
            playlist.tracks.clone(),
            NEW_TARGET,
          );
        }
        vec![self.data_changed(), toast("Playlists imported")]
      }
      ManageMessage::ImportJsonRead(Err(error)) => vec![Effect::Toast(error, ToastKind::Error)],
      ManageMessage::ExportPlaylists => {
        let payload =
          json!({ "playlists": self.data.playlists, "defaultName": "riff-playlists.json" });
        let (name, content) = riff_core::local_media::playlists_export(&payload);
        vec![Effect::ExportPlaylist { name, content }]
      }
      ManageMessage::SpotifyClientId(id) => {
        self.settings.spotify_client_id = id;
        vec![self.save()]
      }
      ManageMessage::SpotifyConnect => {
        let id = self.settings.spotify_client_id.trim().to_string();
        self.ui.manage.spotify.progress = Progress::Running;
        vec![Effect::SpotifyLogin(id)]
      }
      ManageMessage::SpotifyStarted(Ok((url, redirect))) => {
        self.ui.manage.spotify.redirect = redirect;
        vec![Effect::OpenUrl(url)]
      }
      ManageMessage::SpotifyStarted(Err(error)) => {
        self.ui.manage.spotify.progress = Progress::Failed(error);
        Vec::new()
      }
      ManageMessage::SpotifyLogout => vec![get(
        ManageReply::SpotifyLogout,
        "/api/spotify-logout",
        Vec::new(),
      )],
      ManageMessage::SpotifyImportLibrary => {
        self.ui.manage.spotify.progress = Progress::Running;
        vec![get(
          ManageReply::SpotifyLibrary,
          "/api/spotify-library",
          vec![
            ("likes".to_string(), "1".to_string()),
            ("playlists".to_string(), "1".to_string()),
          ],
        )]
      }
      ManageMessage::LastfmKey(key) => {
        self.ui.manage.lastfm.api_key = key;
        Vec::new()
      }
      ManageMessage::LastfmSecret(secret) => {
        self.ui.manage.lastfm.api_secret = secret;
        Vec::new()
      }
      ManageMessage::LastfmConnect => {
        let lastfm = &mut self.ui.manage.lastfm;
        lastfm.progress = Progress::Running;
        lastfm.polls = 0;
        lastfm.token = None;
        let mut effects = Vec::new();
        if !lastfm.api_key.trim().is_empty() || !lastfm.api_secret.trim().is_empty() {
          effects.push(Effect::SaveLastfmKeys {
            key: lastfm.api_key.trim().to_string(),
            secret: std::mem::take(&mut lastfm.api_secret).trim().to_string(),
          });
        }
        effects.push(post(
          ManageReply::LastfmToken,
          "/api/lastfm/token",
          &json!({}),
        ));
        effects
      }
      ManageMessage::LastfmPoll => {
        let Some(token) = self.ui.manage.lastfm.token.clone() else {
          return Vec::new();
        };
        self.ui.manage.lastfm.polls += 1;
        if self.ui.manage.lastfm.polls > LASTFM_POLL_MAX {
          self.ui.manage.lastfm.token = None;
          self.ui.manage.lastfm.progress =
            Progress::Failed("Last.fm authorization timed out".to_string());
          return Vec::new();
        }
        vec![post(
          ManageReply::LastfmSession,
          "/api/lastfm/session",
          &json!({ "token": token }),
        )]
      }
      ManageMessage::LastfmLogout => {
        self.ui.manage.lastfm.connected = false;
        self.ui.manage.lastfm.token = None;
        self.ui.manage.lastfm.progress = Progress::Idle;
        self.settings.lastfm_user.clear();
        vec![Effect::SaveLastfmSession(None), self.save()]
      }
      ManageMessage::LastfmToggle(enabled) => {
        self.settings.lastfm_enabled = enabled;
        vec![self.save()]
      }
      ManageMessage::GoogleConnect => {
        if !self.ui.manage.google.enabled {
          return Vec::new();
        }
        self.ui.manage.google.progress = Progress::Running;
        vec![Effect::GoogleLogin]
      }
      ManageMessage::LastfmVaultFailed(item) => vec![Effect::Toast(
        self.i18n.t(match item {
          LastfmVaultItem::ApiSecret => LASTFM_SECRET_NOT_SAVED,
          LastfmVaultItem::Session => LASTFM_SESSION_NOT_SAVED,
        }),
        ToastKind::Error,
      )],
      ManageMessage::GoogleStarted(Ok(url)) => vec![Effect::OpenUrl(url)],
      ManageMessage::GoogleStarted(Err(error)) => {
        self.ui.manage.google.progress = Progress::Failed(error);
        Vec::new()
      }
      ManageMessage::GoogleLogout => vec![get(
        ManageReply::GoogleLogout,
        "/api/google-logout",
        Vec::new(),
      )],
      ManageMessage::GoogleImportLibrary => {
        self.ui.manage.google.progress = Progress::Running;
        let platform = if self.ui.manage.google.ytmusic {
          "ytmusic"
        } else {
          "youtube"
        };
        vec![get(
          ManageReply::GoogleLibrary,
          "/api/google-library",
          vec![("platform".to_string(), platform.to_string())],
        )]
      }
      ManageMessage::GoogleMusic(ytmusic) => {
        self.ui.manage.google.ytmusic = ytmusic;
        Vec::new()
      }
      ManageMessage::CookiesPick => vec![Effect::PickCookies],
      ManageMessage::CookiesPicked(result) => {
        match result {
          Ok(()) => {
            self.ui.manage.youtube.signed_in = true;
            self.ui.manage.youtube.progress = Progress::Done("Signed in".to_string());
          }
          Err(error) => self.ui.manage.youtube.progress = Progress::Failed(error),
        }
        Vec::new()
      }
      ManageMessage::CookiesBrowser(browser) => {
        self.ui.manage.youtube.progress = Progress::Running;
        vec![Effect::BrowserCookies(browser)]
      }
      ManageMessage::CookiesDelete => {
        self.ui.manage.youtube.signed_in = false;
        self.ui.manage.youtube.progress = Progress::Idle;
        vec![Effect::DeleteCookies]
      }
      ManageMessage::RefreshLocal => vec![Effect::LoadLocalInfo],
      ManageMessage::LocalLoaded(info) => {
        let storage = &mut self.ui.manage.storage;
        storage.cache_bytes = Some(info.cache_bytes);
        storage.data_dir = info.data_dir;
        storage.download_dir = info.download_dir;
        storage.yt_dlp_auto_update = info.yt_dlp_auto_update;
        storage.multi_instance = info.multi_instance;
        self.ui.manage.youtube.signed_in = info.yt_signed_in;
        self.ui.manage.lastfm.connected = info.lastfm_connected;
        Vec::new()
      }
      ManageMessage::ClearCache => vec![Effect::ClearCache],
      ManageMessage::ChangeFolder(kind) => vec![Effect::PickDataFolder(kind)],
      ManageMessage::ResetDownloadFolder => vec![Effect::ResetDownloadFolder],
      ManageMessage::FolderChanged(kind, result) => {
        match result {
          Ok(path) if path.is_empty() => {}
          Ok(path) => match kind {
            FolderKind::Data { .. } => {
              self.ui.manage.storage.data_dir = path;
              self.ui.manage.storage.restart_needed = true;
            }
            FolderKind::Download => self.ui.manage.storage.download_dir = path,
          },
          Err(error) => self.ui.manage.storage.progress = Progress::Failed(error),
        }
        Vec::new()
      }
      ManageMessage::OpenLogs => vec![Effect::OpenLogs],
      ManageMessage::CopyDiagnostics => vec![Effect::CopyDiagnostics, toast("Diagnostics copied")],
      ManageMessage::ExportAccount => vec![Effect::ExportAccount(Box::new(self.account_payload()))],
      ManageMessage::ExportedAccount(Ok(path)) => {
        if path.is_empty() {
          return Vec::new();
        }
        vec![toast("Account exported successfully")]
      }
      ManageMessage::ExportedAccount(Err(error)) => vec![Effect::Toast(error, ToastKind::Error)],
      ManageMessage::ImportAccount => vec![Effect::PickAccount],
      ManageMessage::AccountRead(Ok(None)) => Vec::new(),
      ManageMessage::AccountRead(Ok(Some(value))) => match self.apply_account(&value) {
        Ok(report) => {
          let mut effects = vec![self.data_changed(), self.save(), toast("Account imported")];
          effects.extend(self.open_modal(Modal::ImportReport(Box::new(report))));
          effects
        }
        Err(error) => vec![Effect::Toast(error, ToastKind::Error)],
      },
      ManageMessage::AccountRead(Err(error)) => vec![Effect::Toast(error, ToastKind::Error)],
      ManageMessage::YtDlpAutoUpdate(enabled) => {
        self.ui.manage.storage.yt_dlp_auto_update = enabled;
        vec![Effect::SetYtDlpAutoUpdate(enabled)]
      }
      ManageMessage::MultiInstance(enabled) => {
        self.ui.manage.storage.multi_instance = enabled;
        vec![Effect::SetMultiInstance(enabled)]
      }
      ManageMessage::MultiInstanceSaved(Ok(())) => Vec::new(),
      ManageMessage::MultiInstanceSaved(Err(read)) => {
        self.ui.manage.storage.multi_instance = read;
        vec![Effect::Toast(
          "Could not save the setting.".to_string(),
          ToastKind::Error,
        )]
      }
      ManageMessage::ProxyInput(value) => {
        self.ui.manage.storage.proxy_input = value;
        Vec::new()
      }
      ManageMessage::ProxyAdd => {
        let value = std::mem::take(&mut self.ui.manage.storage.proxy_input)
          .trim()
          .to_string();
        if value.is_empty() || self.settings.proxy_list.contains(&value) {
          return Vec::new();
        }
        self.settings.proxy_list.push(value.clone());
        if self.settings.proxy_active.is_none() {
          self.settings.proxy_active = Some(value);
        }
        vec![self.save()]
      }
      ManageMessage::ProxyRemove(value) => {
        self.settings.proxy_list.retain(|known| known != &value);
        if self.settings.proxy_active.as_deref() == Some(value.as_str()) {
          self.settings.proxy_active = self.settings.proxy_list.first().cloned();
        }
        vec![self.save(), self.proxy_effect()]
      }
      ManageMessage::ProxySelect(value) => {
        self.settings.proxy_active = Some(value);
        vec![self.save(), self.proxy_effect()]
      }
      ManageMessage::ProxyToggle(enabled) => {
        self.settings.proxy_enabled = enabled;
        vec![self.save(), self.proxy_effect()]
      }
      ManageMessage::ProxyTest => {
        self.ui.manage.storage.proxy_test = Progress::Running;
        let url = self.settings.proxy_active.clone().unwrap_or_default();
        vec![post(
          ManageReply::ProxyTest,
          "/api/proxy-test",
          &json!({ "proxyUrl": url }),
        )]
      }
    }
  }

  fn import_reply(&mut self, result: Result<Value, String>) -> Vec<Effect> {
    let value = match result {
      Ok(value) => value,
      Err(error) => {
        self.ui.manage.import.progress = Progress::Failed(error);
        return Vec::new();
      }
    };
    let tracks = tracks_of(&value["tracks"]);
    if value["success"].as_bool() != Some(true) || tracks.is_empty() {
      self.ui.manage.import.progress =
        Progress::Failed(failure_of(&value, "Failed to import playlist"));
      return Vec::new();
    }
    let target = self.ui.manage.import.target.clone();
    let name = value["name"].as_str().unwrap_or("Playlist").to_string();
    let count = self.add_imported(&name, value["cover"].as_str(), tracks, &target);
    self.ui.manage.import.progress = Progress::Done(format!("{count} tracks"));
    vec![self.data_changed(), toast("Playlist imported")]
  }

  /// Réponses du core pour ces écrans.
  pub(crate) fn manage_reply(
    &mut self,
    reply: ManageReply,
    result: Result<Value, String>,
  ) -> Vec<Effect> {
    match reply {
      ManageReply::ToolsStatus => match result {
        Ok(value) => {
          let status = ToolsStatus::parse(&value);
          let tools = &mut self.ui.manage.tools;
          if tools.installing && (status.is_ready() || status.failure().is_some()) {
            tools.installing = false;
          }
          let missing = !status.is_ready();
          tools.status = status;
          if missing && self.ui.modal.is_none() && !self.ui.manage.tools.installing {
            return self.open_modal(Modal::Tools);
          }
          Vec::new()
        }
        Err(err) => {
          self.ui.manage.tools.installing = false;
          self.ui.manage.tools.status.error = err;
          Vec::new()
        }
      },
      ManageReply::ToolsStarted => {
        if let Err(err) = result {
          self.ui.manage.tools.installing = false;
          self.ui.manage.tools.status.error = err;
        }
        Vec::new()
      }
      ManageReply::Import(_) => self.import_reply(result),
      ManageReply::SpotifyStatus => {
        if let Ok(value) = result {
          self.ui.manage.spotify.connected = value["connected"].as_bool().unwrap_or(false);
        }
        Vec::new()
      }
      ManageReply::SpotifyLogout => {
        self.ui.manage.spotify.connected = false;
        self.ui.manage.spotify.progress = Progress::Idle;
        Vec::new()
      }
      ManageReply::SpotifyLibrary => self.library_reply(result, true),
      ManageReply::GoogleStatus => {
        if let Ok(value) = result {
          self.ui.manage.google.enabled = value["enabled"].as_bool().unwrap_or(false);
          self.ui.manage.google.connected = value["connected"].as_bool().unwrap_or(false);
          if let Some(error) = value["error"].as_str().filter(|error| !error.is_empty()) {
            self.ui.manage.google.progress = Progress::Failed(error.to_string());
          }
        }
        Vec::new()
      }
      ManageReply::GoogleLogout => {
        self.ui.manage.google.connected = false;
        self.ui.manage.google.progress = Progress::Idle;
        Vec::new()
      }
      ManageReply::GoogleLibrary => self.library_reply(result, false),
      ManageReply::LastfmToken => {
        let lastfm = &mut self.ui.manage.lastfm;
        match result {
          Ok(value) if value["success"].as_bool() == Some(true) => {
            lastfm.token = value["token"].as_str().map(str::to_string);
            vec![Effect::OpenUrl(
              value["authUrl"].as_str().unwrap_or("").to_string(),
            )]
          }
          Ok(value) => {
            lastfm.progress = Progress::Failed(failure_of(&value, "Failed to get auth token"));
            Vec::new()
          }
          Err(error) => {
            lastfm.progress = Progress::Failed(error);
            Vec::new()
          }
        }
      }
      ManageReply::LastfmSession => {
        let Ok(value) = result else {
          return Vec::new();
        };
        if value["success"].as_bool() != Some(true) {
          return Vec::new();
        }
        self.ui.manage.lastfm.token = None;
        self.ui.manage.lastfm.connected = true;
        self.ui.manage.lastfm.progress = Progress::Idle;
        self.settings.lastfm_user = value["username"].as_str().unwrap_or("").to_string();
        vec![
          Effect::SaveLastfmSession(value["sessionKey"].as_str().map(str::to_string)),
          self.save(),
        ]
      }
      ManageReply::LastfmSent | ManageReply::ProxyConfig => Vec::new(),
      ManageReply::ProxyTest => {
        self.ui.manage.storage.proxy_test = match result {
          Ok(value) if value["success"].as_bool() == Some(true) => {
            Progress::Done("Proxy works".to_string())
          }
          Ok(value) => Progress::Failed(failure_of(&value, "Proxy test failed")),
          Err(error) => Progress::Failed(error),
        };
        Vec::new()
      }
    }
  }

  fn library_reply(&mut self, result: Result<Value, String>, spotify: bool) -> Vec<Effect> {
    let (progress, effects) = match result {
      Ok(value) if value["success"].as_bool() == Some(true) || value.get("likes").is_some() => {
        let (likes, lists) = self.apply_library(&value);
        let warning = value["warning"]
          .as_str()
          .filter(|warning| !warning.is_empty())
          .map(str::to_string);
        let text = warning.unwrap_or_else(|| format!("{likes} liked tracks, {lists} playlists"));
        (Progress::Done(text), vec![self.data_changed()])
      }
      Ok(value) => (
        Progress::Failed(failure_of(&value, "Import failed")),
        Vec::new(),
      ),
      Err(error) => (Progress::Failed(error), Vec::new()),
    };
    if spotify {
      self.ui.manage.spotify.progress = progress;
    } else {
      self.ui.manage.google.progress = progress;
    }
    effects
  }
}

/// Chemin affiché : sans le préfixe `\\?\` de Windows.
#[must_use]
pub fn display_path(path: &std::path::Path) -> String {
  let text = path.display().to_string();
  text
    .strip_prefix(r"\\?\")
    .map_or(text.clone(), str::to_string)
}

/// Écrit `multiInstance` dans `startup.json` (les autres clés sont conservées). Échec : la valeur relue.
pub fn write_multi_instance(enabled: bool) -> Result<(), bool> {
  let mut patch = serde_json::Map::new();
  patch.insert("multiInstance".to_string(), Value::Bool(enabled));
  if riff_core::config::write_startup_config(&patch) {
    Ok(())
  } else {
    Err(multi_instance_enabled())
  }
}

/// Valeur de `multiInstance` dans `startup.json`, faux par défaut.
#[must_use]
pub fn multi_instance_enabled() -> bool {
  crate::system::startup::multi_instance()
}

/// Lecture locale pour la page de réglages. À appeler hors du fil d'interface.
#[must_use]
pub fn read_local_info(core: &riff_core::core::Core) -> LocalInfo {
  use riff_core::secrets::SecretName;
  LocalInfo {
    cache_bytes: riff_core::maintenance::cache_size(),
    data_dir: display_path(&riff_core::paths::resolve_data_dir()),
    download_dir: display_path(&riff_core::services::playback::downloads_dir()),
    yt_signed_in: riff_core::services::ytdlp::has_yt_login_cookies(),
    yt_dlp_auto_update: riff_core::config::yt_dlp_auto_update(),
    multi_instance: multi_instance_enabled(),
    lastfm_connected: core
      .auth
      .secrets
      .get(SecretName::LastfmSession)
      .ok()
      .flatten()
      .is_some(),
    lastfm_has_secret: core
      .auth
      .secrets
      .get(SecretName::LastfmApiSecret)
      .ok()
      .flatten()
      .is_some(),
  }
}

/// Change le dossier de données : refuse un dossier imbriqué, vérifie l'écriture, déplace si demandé (`set_data_dir`).
///
/// # Errors
/// Dossier imbriqué, non inscriptible, ou copie impossible.
pub fn change_data_dir(target: &std::path::Path, move_data: bool) -> Result<String, String> {
  let current = riff_core::paths::resolve_data_dir();
  if target == current {
    return Ok(target.display().to_string());
  }
  if target.starts_with(&current) || current.starts_with(target) {
    return Err("Choose a folder outside the current music folder".to_string());
  }
  std::fs::create_dir_all(target).map_err(|_| "That folder is not writable".to_string())?;
  let probe = target.join(".riff_write_test");
  std::fs::write(&probe, b"ok").map_err(|_| "That folder is not writable".to_string())?;
  let _ = std::fs::remove_file(&probe);
  if move_data {
    riff_core::paths::copy_data_entries(&current, target).map_err(|err| err.to_string())?;
  }
  riff_core::config::set_data_dir_config(Some(&target.to_string_lossy()))
    .map_err(|_| "Could not save the new folder".to_string())?;
  if move_data {
    riff_core::paths::remove_data_entries(&current);
  }
  Ok(display_path(target))
}

/// Lit un fichier de compte.
///
/// # Errors
/// Fichier illisible ou corrompu.
pub fn read_account(path: &std::path::Path) -> Result<Value, String> {
  let read = riff_core::local_media::read_account_file(path);
  if read["success"].as_bool() == Some(true) {
    Ok(read["data"].clone())
  } else {
    Err(failure_of(&read, "Failed to read account file"))
  }
}

/// Dossier des journaux.
#[must_use]
pub fn logs_dir() -> PathBuf {
  riff_core::paths::log_dir()
}

#[cfg(test)]
#[path = "manage_tests.rs"]
mod tests;
