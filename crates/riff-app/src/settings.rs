//! Réglages de l'application native, import depuis `storage.json`, et lecture seule de ce fichier.

use crate::audio_prefs::SoundPrefs;
use crate::import_report;
use crate::screens::Screen;
use crate::user_data::UserData;
use crate::visuals::settings::VisualSettings;
use material_colors::color::Rgb;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

const FILE_NAME: &str = "native-settings.json";
const DEFAULT_LANG: &str = "en";
/// Accent d'un Riff vierge : la migration `riff_defaults_v44_hue246` (`app.js:592-597`) le pose, comme
/// la valeur initiale de `state.themeAccent` (`app.js:817`).
const DEFAULT_ACCENT: &str = "#5247D1";
/// Luminosité d'un Riff vierge (`app.js:595` et `:819`).
const DEFAULT_BRIGHTNESS: u8 = 20;
/// Intensité d'un Riff vierge (`app.js:596` et `:818`).
const DEFAULT_INTENSITY: u8 = 85;
const DEFAULT_PALETTE_STYLE: &str = "classic";
/// Bornes de `settings-slider-scale` (`index.html:990`).
pub const UI_SCALE_RANGE: (u32, u32) = (80, 130);
/// Valeur de `settings-slider-scale` dans `index.html`.
const DEFAULT_UI_SCALE: u32 = 105;
const DEFAULT_NICKNAME: &str = "User";
const DEFAULT_CATEGORY: &str = "interface";
const DEFAULT_HOME_LAYOUT: &str = "spotify";
const DEFAULT_DOWNLOAD_FORMAT: &str = "mp3";
/// Sections de l'accueil, dans l'ordre de `DEFAULT_HOME_ORDER`.
pub const DEFAULT_HOME_ORDER: [&str; 4] = [
  "sp-made-sec",
  "sp-quick-sec",
  "sp-trend-sec",
  "sp-artists-sec",
];

/// Mode de thème. `Auto` (« Auto (album) » dans Riff) suit la pochette du titre en cours, sur une base sombre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
  #[default]
  Dark,
  Light,
  Auto,
}

/// Origine des réglages au premier lancement : `Fresh` interdit tout complément depuis `storage.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
  Fresh,
  #[default]
  #[serde(other)]
  Riff,
}

impl ImportSource {
  fn is_default(&self) -> bool {
    *self == Self::Riff
  }
}

/// Réglages de `native-settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NativeSettings {
  pub ui_lang: String,
  pub tr_lang: String,
  pub theme_mode: ThemeMode,
  pub theme_accent: String,
  pub theme_brightness: u8,
  pub theme_intensity: u8,
  pub palette_style: String,
  /// Identifiant du thème de fichier choisi.
  pub native_theme: Option<String>,
  /// Dernier écran affiché.
  pub screen: Screen,
  /// Échelle de l'interface en pourcentage, sur [`UI_SCALE_RANGE`].
  pub ui_scale: u32,
  pub compact_engine_btns: bool,
  pub hide_engine_btns: bool,
  pub play_btn_bg: bool,
  pub show_explicit: bool,
  pub home_order: Vec<String>,
  pub intensity_bg: bool,
  pub highlight_buttons: bool,
  pub search_albums: bool,
  pub classic_track_rows: bool,
  pub show_local: bool,
  pub show_albums: bool,
  pub hide_titlebar: bool,
  pub home_wave: bool,
  pub home_recent: bool,
  pub home_artists: bool,
  pub home_layout: String,
  pub nickname: String,
  /// Nom affiché dans un jam ; vide : nom par défaut.
  pub jam_name: String,
  /// Relais du jam choisi par l'utilisateur ; vide : relais par défaut.
  pub jam_relay: String,
  pub avatar: Option<String>,
  /// Catégorie ouverte de la page de réglages.
  pub settings_cat: String,
  /// Section Avancé dépliée (`riff_adv_open`, absente = repliée).
  pub adv_open: bool,
  /// Format de la fenêtre de téléchargement.
  pub download_format: String,
  pub proxy_list: Vec<String>,
  pub proxy_enabled: bool,
  pub proxy_active: Option<String>,
  pub spotify_client_id: String,
  pub spotify_promo_dismissed: bool,
  pub lastfm_enabled: bool,
  pub lastfm_user: String,
  /// Fermer la fenêtre la garde dans la zone de notification (`riff_tray_on_close`, vrai hors Linux).
  pub tray_on_close: bool,
  /// Fenêtre principale au premier plan, bascule du menu de la zone de notification (`riff_main_always_on_top`, faux par défaut).
  pub main_always_on_top: bool,
  /// Session média du système : touches média et superposition.
  pub media_session: bool,
  /// Présence Discord (`devsize_discord_rpc`).
  pub discord_rpc: bool,
  /// L'artiste dans le statut Discord (`riff_discord_artist_status`).
  pub discord_artist_status: bool,
  /// Réduire le mouvement (`riff_adv_reduce_motion`, `app.js:1094`). Absent : la préférence du système, relue à chaque lancement
  /// (`systemPrefersReducedMotion`), comme un Riff vierge.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub reduce_motion: Option<bool>,
  /// Mode performance (`riff_perf_mode`, `app.js:17167`) : absent ou autre que `1` = désactivé.
  pub perf_mode: bool,
  /// Bouton « Vérifier les mises à jour » dans « à propos ». Aucune requête automatique, quel que soit ce réglage.
  pub update_check: bool,
  /// Fenêtre audio.
  pub sound: SoundPrefs,
  /// Paroles, visualiseurs et fonds.
  pub visual: Box<VisualSettings>,
  /// Clés de Riff sans équivalent natif, conservées telles quelles et réécrites par l'export de compte.
  /// Jamais de secret ni de cache (voir `import_report`).
  #[serde(skip_serializing_if = "Map::is_empty")]
  pub legacy: Map<String, Value>,
  /// `Fresh` après « Start fresh » : `load_or_import` ne complète plus rien depuis `storage.json`.
  #[serde(skip_serializing_if = "ImportSource::is_default")]
  pub import_source: ImportSource,
  /// Clés écrites par une autre version : conservées telles quelles à l'écriture.
  #[serde(flatten)]
  pub extra: Map<String, Value>,
}

impl Default for NativeSettings {
  fn default() -> Self {
    Self {
      ui_lang: DEFAULT_LANG.to_string(),
      tr_lang: DEFAULT_LANG.to_string(),
      theme_mode: ThemeMode::default(),
      theme_accent: DEFAULT_ACCENT.to_string(),
      theme_brightness: DEFAULT_BRIGHTNESS,
      theme_intensity: DEFAULT_INTENSITY,
      palette_style: DEFAULT_PALETTE_STYLE.to_string(),
      native_theme: None,
      screen: Screen::default(),
      ui_scale: DEFAULT_UI_SCALE,
      compact_engine_btns: false,
      hide_engine_btns: false,
      play_btn_bg: true,
      show_explicit: true,
      home_order: DEFAULT_HOME_ORDER.iter().map(ToString::to_string).collect(),
      intensity_bg: false,
      highlight_buttons: true,
      search_albums: true,
      classic_track_rows: false,
      show_local: true,
      show_albums: false,
      hide_titlebar: false,
      home_wave: true,
      home_recent: true,
      home_artists: true,
      home_layout: DEFAULT_HOME_LAYOUT.to_string(),
      nickname: DEFAULT_NICKNAME.to_string(),
      jam_name: String::new(),
      jam_relay: String::new(),
      avatar: None,
      settings_cat: DEFAULT_CATEGORY.to_string(),
      adv_open: false,
      download_format: DEFAULT_DOWNLOAD_FORMAT.to_string(),
      proxy_list: Vec::new(),
      proxy_enabled: false,
      proxy_active: None,
      spotify_client_id: String::new(),
      spotify_promo_dismissed: false,
      lastfm_enabled: true,
      lastfm_user: String::new(),
      tray_on_close: !cfg!(target_os = "linux"),
      main_always_on_top: false,
      media_session: true,
      discord_rpc: true,
      discord_artist_status: true,
      update_check: true,
      reduce_motion: None,
      perf_mode: false,
      sound: SoundPrefs::default(),
      visual: Box::default(),
      legacy: Map::new(),
      import_source: ImportSource::default(),
      extra: Map::new(),
    }
  }
}

impl NativeSettings {
  /// Défauts de « Start fresh » : rien n'est repris de `storage.json`, ce choix est noté.
  #[must_use]
  pub fn fresh() -> Self {
    Self {
      import_source: ImportSource::Fresh,
      ..Self::default()
    }
  }

  /// Reprend les réglages de Riff : uniquement les clés connues, une valeur invalide garde le défaut.
  #[must_use]
  pub fn imported_from(storage: &LegacyStorage) -> Self {
    let defaults = Self::default();
    let text = |key: &str, default: String| storage.text(key).map_or(default, str::to_string);
    let percent = |key: &str, default: u8| {
      storage
        .text(key)
        .and_then(|v| v.trim().parse::<u8>().ok())
        .filter(|v| *v <= 100)
        .unwrap_or(default)
    };
    let mut settings = Self {
      ui_lang: text("riff_ui_lang", defaults.ui_lang),
      tr_lang: text("riff_tr_lang", defaults.tr_lang),
      theme_mode: match storage.text("riff_theme_mode") {
        Some("light") => ThemeMode::Light,
        Some("auto") => ThemeMode::Auto,
        _ => ThemeMode::Dark,
      },
      theme_accent: storage
        .text("riff_theme_accent")
        .filter(|v| v.parse::<Rgb>().is_ok())
        .map_or(defaults.theme_accent, str::to_string),
      theme_brightness: percent("riff_theme_brightness", defaults.theme_brightness),
      theme_intensity: percent("riff_theme_intensity", defaults.theme_intensity),
      palette_style: text("riff_palette_style", defaults.palette_style),
      native_theme: None,
      screen: Screen::default(),
      ui_scale: storage
        .text("devsize_ui_scale")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|v| (UI_SCALE_RANGE.0..=UI_SCALE_RANGE.1).contains(v))
        .unwrap_or(defaults.ui_scale),
      compact_engine_btns: storage.text("riff_compact_engine_btns") == Some("1"),
      hide_engine_btns: storage.text("riff_hide_engine_btns") == Some("1"),
      play_btn_bg: storage.text("riff_play_btn_bg") != Some("false"),
      show_explicit: storage.text("riff_show_explicit") != Some("0"),
      home_order: home_order_from(storage.text("riff_home_order")),
      intensity_bg: storage.text("riff_adv_intensity_bg") == Some("true"),
      highlight_buttons: storage.text("riff_highlight_buttons") != Some("0"),
      search_albums: storage.text("riff_adv_albums") != Some("false"),
      classic_track_rows: storage.text("riff_classic_track_rows") == Some("1"),
      show_local: storage.text("riff_show_local") != Some("false"),
      show_albums: storage.text("riff_show_albums") == Some("1"),
      hide_titlebar: storage.text("riff_adv_hide_titlebar") == Some("true"),
      home_wave: storage.text("riff_adv_my_wave") != Some("false"),
      home_recent: storage.text("riff_adv_jump_back") != Some("false"),
      home_artists: storage.text("riff_adv_fav_artists") != Some("false"),
      home_layout: match storage.text("riff_home_layout") {
        Some("riff") => "riff".to_string(),
        _ => defaults.home_layout,
      },
      nickname: storage
        .text("riff_user_nickname")
        .filter(|v| !v.trim().is_empty())
        .map_or(defaults.nickname, str::to_string),
      jam_name: defaults.jam_name,
      jam_relay: defaults.jam_relay,
      avatar: storage.text("riff_user_avatar").map(str::to_string),
      settings_cat: text("riff_settings_cat", defaults.settings_cat),
      adv_open: storage.text("riff_adv_open") == Some("true"),
      download_format: text("riff_download_format", defaults.download_format),
      proxy_list: storage
        .text("riff_proxy_list")
        .and_then(|raw| serde_json::from_str::<Vec<Value>>(raw).ok())
        .map_or_else(Vec::new, |items| {
          items
            .iter()
            .filter_map(|item| {
              item
                .as_str()
                .or_else(|| item.get("url").and_then(Value::as_str))
            })
            .map(str::to_string)
            .collect()
        }),
      proxy_enabled: storage.text("riff_proxy_enabled") == Some("true"),
      proxy_active: storage.text("riff_proxy_active_id").map(str::to_string),
      spotify_client_id: text("riff_spotify_client_id", defaults.spotify_client_id),
      spotify_promo_dismissed: storage.text("riff_spotify_promo_dismissed") == Some("1"),
      lastfm_enabled: storage.text("riff_lastfm_enabled") != Some("false"),
      lastfm_user: text("riff_lastfm_user", defaults.lastfm_user),
      tray_on_close: storage
        .text("riff_tray_on_close")
        .map_or(defaults.tray_on_close, |value| value == "true"),
      main_always_on_top: storage.text("riff_main_always_on_top") == Some("true"),
      media_session: storage
        .text("riff_win_global_media_keys")
        .map_or(defaults.media_session, |value| value == "true"),
      discord_rpc: storage.text("devsize_discord_rpc") != Some("false"),
      discord_artist_status: storage.text("riff_discord_artist_status") != Some("0"),
      update_check: defaults.update_check,
      reduce_motion: storage
        .text("riff_adv_reduce_motion")
        .map(|value| value == "true"),
      perf_mode: storage.text("riff_perf_mode") == Some("1"),
      sound: SoundPrefs::imported_from(storage),
      visual: Box::new(VisualSettings::imported_from(storage)),
      legacy: Map::new(),
      import_source: ImportSource::Riff,
      extra: Map::new(),
    };
    // Les clés de données sont lues ici pour qu'elles ne passent pas pour des clés sans équivalent.
    let _ = UserData::imported_from(storage, 0);
    settings.legacy = import_report::legacy_entries(storage);
    settings
  }

  /// Lit le fichier de réglages (un BOM UTF-8 en tête est ignoré). Absent ou corrompu : import depuis `storage`, puis écriture du fichier.
  /// Un champ absent du fichier, ajouté par une version récente, vient lui aussi de `storage`.
  #[must_use]
  pub fn load_or_import(settings_path: &Path, storage: &LegacyStorage) -> Self {
    let imported = Self::imported_from(storage);
    let mut had_corrupt_file = false;
    match fs::read_to_string(settings_path) {
      Ok(raw) => match serde_json::from_str::<Map<String, Value>>(
        raw.strip_prefix('\u{feff}').unwrap_or(&raw),
      ) {
        Ok(saved) => {
          // « Start fresh » : aucun champ absent n'est complété depuis `storage.json`.
          let fresh_base;
          let base = if saved.get("importSource") == Some(&Value::String("fresh".to_string())) {
            fresh_base = Self::imported_from(&LegacyStorage::default());
            &fresh_base
          } else {
            &imported
          };
          if let Ok(Value::Object(mut merged)) = serde_json::to_value(base) {
            merged.extend(saved);
            // Un fichier sans section `visual` (version précédente) reprend celle de Riff, ajoutée par `imported`.
            if let Ok(settings) = serde_json::from_value(Value::Object(merged)) {
              return settings;
            }
          }
          had_corrupt_file = true;
        }
        Err(err) => {
          tracing::warn!(error = %err, "native-settings.json illisible, mis de côté");
          had_corrupt_file = true;
        }
      },
      Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
      Err(err) => tracing::warn!(error = %err, "lecture de native-settings.json"),
    }
    if had_corrupt_file
      && let Err(err) = fs::rename(settings_path, settings_path.with_extension("json.corrupt"))
    {
      tracing::warn!(error = %err, "mise de côté de native-settings.json");
    }
    let settings = imported;
    if let Err(err) = settings.save(settings_path) {
      tracing::warn!(error = %err, "écriture de native-settings.json");
    }
    settings
  }

  /// Clés de Riff, pour l'export de compte.
  #[must_use]
  pub fn to_legacy_entries(&self) -> Map<String, Value> {
    let mut out = Map::new();
    let mut put = |key: &str, value: String| {
      out.insert(key.to_string(), Value::String(value));
    };
    let bit = |value: bool| {
      if value {
        "1".to_string()
      } else {
        "0".to_string()
      }
    };
    let word = |value: bool| value.to_string();
    put("riff_ui_lang", self.ui_lang.clone());
    put("riff_tr_lang", self.tr_lang.clone());
    put(
      "riff_theme_mode",
      match self.theme_mode {
        ThemeMode::Dark => "dark",
        ThemeMode::Light => "light",
        ThemeMode::Auto => "auto",
      }
      .to_string(),
    );
    put("riff_theme_accent", self.theme_accent.clone());
    put("riff_theme_brightness", self.theme_brightness.to_string());
    put("riff_theme_intensity", self.theme_intensity.to_string());
    put("riff_palette_style", self.palette_style.clone());
    put("devsize_ui_scale", self.ui_scale.to_string());
    put("riff_compact_engine_btns", bit(self.compact_engine_btns));
    put("riff_hide_engine_btns", bit(self.hide_engine_btns));
    put("riff_play_btn_bg", word(self.play_btn_bg));
    put("riff_show_explicit", bit(self.show_explicit));
    put(
      "riff_home_order",
      serde_json::to_string(&self.home_order).unwrap_or_default(),
    );
    put("riff_adv_intensity_bg", word(self.intensity_bg));
    put("riff_highlight_buttons", bit(self.highlight_buttons));
    put("riff_adv_albums", word(self.search_albums));
    put("riff_classic_track_rows", bit(self.classic_track_rows));
    put("riff_show_local", word(self.show_local));
    put("riff_show_albums", bit(self.show_albums));
    put("riff_adv_hide_titlebar", word(self.hide_titlebar));
    put("riff_adv_my_wave", word(self.home_wave));
    put("riff_adv_jump_back", word(self.home_recent));
    put("riff_adv_fav_artists", word(self.home_artists));
    put("riff_home_layout", self.home_layout.clone());
    put("riff_user_nickname", self.nickname.clone());
    if let Some(avatar) = &self.avatar {
      put("riff_user_avatar", avatar.clone());
    }
    put("riff_settings_cat", self.settings_cat.clone());
    put("riff_adv_open", word(self.adv_open));
    put("riff_download_format", self.download_format.clone());
    put(
      "riff_proxy_list",
      serde_json::to_string(&self.proxy_list).unwrap_or_default(),
    );
    put("riff_proxy_enabled", word(self.proxy_enabled));
    if let Some(active) = &self.proxy_active {
      put("riff_proxy_active_id", active.clone());
    }
    put("riff_spotify_client_id", self.spotify_client_id.clone());
    put(
      "riff_spotify_promo_dismissed",
      bit(self.spotify_promo_dismissed),
    );
    put("riff_lastfm_enabled", word(self.lastfm_enabled));
    put("riff_lastfm_user", self.lastfm_user.clone());
    put("riff_tray_on_close", word(self.tray_on_close));
    put("riff_main_always_on_top", word(self.main_always_on_top));
    put("riff_win_global_media_keys", word(self.media_session));
    put("devsize_discord_rpc", word(self.discord_rpc));
    put(
      "riff_discord_artist_status",
      bit(self.discord_artist_status),
    );
    if let Some(reduced) = self.reduce_motion {
      put("riff_adv_reduce_motion", word(reduced));
    }
    put("riff_perf_mode", bit(self.perf_mode));
    put("riff_persist_sound_settings", bit(self.sound.persist));
    put("riff_smooth_presets", word(self.sound.smooth_presets));
    put(
      "riff_audio_settings",
      serde_json::to_string(&self.sound.audio).unwrap_or_default(),
    );
    put(
      "riff_hq_settings",
      serde_json::to_string(&self.sound.hq).unwrap_or_default(),
    );
    put("riff_hq_enabled", bit(self.sound.hq_enabled));
    put("riff_active_preset", self.sound.active_preset.clone());
    put(
      "devsize_custom_presets",
      serde_json::to_string(&self.sound.custom_presets).unwrap_or_default(),
    );
    if let Some(device) = &self.sound.output_device {
      put("riff_audio_output_device", device.clone());
    }
    put("riff_adv_connected_audio", word(self.sound.connected_audio));
    out
  }

  /// Écrit le fichier sans jamais laisser un fichier à moitié écrit.
  ///
  /// # Errors
  /// Erreur du système de fichiers.
  pub fn save(&self, path: &Path) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{seq}.tmp"));
    let bytes = serde_json::to_string_pretty(self)?;
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes.as_bytes())?;
    file.sync_all()?;
    drop(file);
    let res = fs::rename(&tmp, path);
    if res.is_err() {
      let _ = fs::remove_file(&tmp);
    }
    res
  }
}

/// `getHomeSectionsOrder` : les sections inconnues sont retirées, les manquantes ajoutées à la fin.
fn home_order_from(raw: Option<&str>) -> Vec<String> {
  let parsed: Vec<String> = raw
    .and_then(|raw| serde_json::from_str(raw).ok())
    .unwrap_or_default();
  let mut order: Vec<String> = parsed
    .into_iter()
    .filter(|id| DEFAULT_HOME_ORDER.contains(&id.as_str()))
    .collect();
  for id in DEFAULT_HOME_ORDER {
    if !order.iter().any(|known| known == id) {
      order.push(id.to_string());
    }
  }
  order
}

/// Chemin de `native-settings.json`.
#[must_use]
pub fn settings_path() -> PathBuf {
  riff_core::paths::resolve_data_dir().join(FILE_NAME)
}

/// Clés demandées à un [`LegacyStorage`]. Toutes les copies sont égales : ce suivi n'entre pas dans `PartialEq`.
#[derive(Debug, Default)]
struct ReadKeys(Mutex<BTreeSet<String>>);

impl ReadKeys {
  fn lock(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
    self.0.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

impl Clone for ReadKeys {
  fn clone(&self) -> Self {
    Self(Mutex::new(self.lock().clone()))
  }
}

impl PartialEq for ReadKeys {
  fn eq(&self, _: &Self) -> bool {
    true
  }
}

/// `storage.json` de Riff, en lecture seule. Lu directement : `riff_core::storage::read_storage_disk` peut réécrire
/// le fichier et lire le coffre, et ce fichier appartient à Riff (Tauri), qui reste utilisable en parallèle.
/// Chaque clé demandée par `get` et `text` est enregistrée : c'est la base du rapport d'import.
#[derive(Clone, Default, PartialEq)]
pub struct LegacyStorage {
  entries: Map<String, Value>,
  read: ReadKeys,
}

/// Les noms de clés seulement : un `Debug` ne doit jamais montrer une valeur (elles peuvent être des secrets).
impl std::fmt::Debug for LegacyStorage {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("LegacyStorage")
      .field("keys", &self.entries.keys().collect::<Vec<_>>())
      .finish()
  }
}

impl LegacyStorage {
  fn new(entries: Map<String, Value>) -> Self {
    Self {
      entries,
      read: ReadKeys::default(),
    }
  }

  /// Stockage venu d'un fichier de compte : une valeur autre qu'un texte est écrite en JSON, comme `localStorage`.
  #[must_use]
  pub fn from_entries(entries: &serde_json::Map<String, Value>) -> Self {
    Self::new(
      entries
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| {
          let text = value
            .as_str()
            .map_or_else(|| value.to_string(), str::to_string);
          (key.clone(), Value::String(text))
        })
        .collect(),
    )
  }

  /// Lit le fichier. Absent ou corrompu : stockage vide.
  #[must_use]
  pub fn open(path: &Path) -> Self {
    let parsed = fs::read_to_string(path)
      .ok()
      .and_then(|raw| serde_json::from_str(&raw).ok());
    match parsed {
      Some(Value::Object(fields)) => Self::new(fields),
      _ => Self::default(),
    }
  }

  /// Valeur d'une clé.
  #[must_use]
  pub fn get(&self, key: &str) -> Option<&Value> {
    let mut read = self.read.lock();
    if !read.contains(key) {
      read.insert(key.to_string());
    }
    drop(read);
    self.entries.get(key)
  }

  pub(crate) fn text(&self, key: &str) -> Option<&str> {
    self.get(key).and_then(Value::as_str)
  }

  /// Toutes les entrées, sans les marquer comme lues.
  #[must_use]
  pub fn entries(&self) -> &Map<String, Value> {
    &self.entries
  }

  /// Aucune clé.
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.entries.is_empty()
  }

  /// Clés demandées jusqu'ici (présentes ou non), triées.
  #[must_use]
  pub fn read_keys(&self) -> BTreeSet<String> {
    self.read.lock().clone()
  }

  /// Mêmes entrées, suivi des lectures remis à zéro.
  #[must_use]
  pub fn untracked_copy(&self) -> Self {
    Self::new(self.entries.clone())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;
  use serde_json::json;

  fn temp_dir() -> TempDir {
    TempDir::new("app-settings")
  }

  fn storage_with(content: &str) -> (TempDir, LegacyStorage) {
    let dir = temp_dir();
    let path = dir.join("storage.json");
    fs::write(&path, content).unwrap();
    let storage = LegacyStorage::open(&path);
    (dir, storage)
  }

  const FIXTURE: &str = r##"{
    "riff_ui_lang": "ru",
    "riff_tr_lang": "uk",
    "riff_theme_mode": "light",
    "riff_theme_accent": "#5247d1",
    "riff_theme_brightness": "35",
    "riff_theme_intensity": "60",
    "riff_palette_style": "vibrant",
    "devsize_playlists": "[{\"name\":\"ne pas importer\"}]",
    "riff_deepl_api_key": "jeton-de-test"
  }"##;

  #[test]
  fn present_keys_are_imported() {
    let (_, storage) = storage_with(FIXTURE);
    assert_eq!(
      NativeSettings::imported_from(&storage),
      NativeSettings {
        ui_lang: "ru".into(),
        tr_lang: "uk".into(),
        theme_mode: ThemeMode::Light,
        theme_accent: "#5247d1".into(),
        theme_brightness: 35,
        theme_intensity: 60,
        palette_style: "vibrant".into(),
        native_theme: None,
        screen: Screen::default(),
        ..NativeSettings::default()
      }
    );
  }

  #[test]
  fn absent_keys_keep_the_defaults() {
    let (_, storage) = storage_with(r#"{ "riff_ui_lang": "uk" }"#);
    let settings = NativeSettings::imported_from(&storage);
    assert_eq!(
      settings,
      NativeSettings {
        ui_lang: "uk".into(),
        ..NativeSettings::default()
      }
    );
  }

  #[test]
  fn values_of_the_wrong_type_keep_the_defaults() {
    let (_, storage) = storage_with(
      r#"{
        "riff_ui_lang": 12,
        "riff_tr_lang": null,
        "riff_theme_mode": "rose",
        "riff_theme_accent": "pas une couleur",
        "riff_theme_brightness": "beaucoup",
        "riff_theme_intensity": "900",
        "riff_palette_style": ["a"]
      }"#,
    );
    assert_eq!(
      NativeSettings::imported_from(&storage),
      NativeSettings::default()
    );
  }

  #[test]
  fn a_corrupt_storage_gives_the_defaults() {
    let (_, storage) = storage_with("{ pas du json");
    assert_eq!(storage, LegacyStorage::default());
    assert_eq!(
      NativeSettings::imported_from(&storage),
      NativeSettings::default()
    );
  }

  #[test]
  fn a_storage_that_is_not_an_object_is_empty() {
    let (_, storage) = storage_with("[1, 2]");
    assert_eq!(storage, LegacyStorage::default());
  }

  #[test]
  fn a_missing_storage_is_empty() {
    assert_eq!(
      LegacyStorage::open(&temp_dir().join("absent.json")),
      LegacyStorage::default()
    );
  }

  #[test]
  fn first_launch_imports_only_the_listed_keys_and_writes_the_file() {
    let (dir, storage) = storage_with(FIXTURE);
    let path = dir.join(FILE_NAME);
    let settings = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(settings.ui_lang, "ru");
    let written = fs::read_to_string(&path).unwrap();
    assert!(!written.contains("ne pas importer"));
    assert!(!written.contains("jeton-de-test"));
    assert!(!dir.join("native-settings.json.tmp").exists());
  }

  #[test]
  fn the_storage_file_is_never_written() {
    let (dir, storage) = storage_with(FIXTURE);
    let storage_path = dir.join("storage.json");
    let before = fs::metadata(&storage_path).unwrap().modified().unwrap();
    let settings = NativeSettings::load_or_import(&dir.join(FILE_NAME), &storage);
    settings.save(&dir.join(FILE_NAME)).unwrap();
    assert_eq!(LegacyStorage::open(&storage_path), storage);
    assert_eq!(fs::read_to_string(&storage_path).unwrap(), FIXTURE);
    assert_eq!(
      fs::metadata(&storage_path).unwrap().modified().unwrap(),
      before
    );
  }

  #[test]
  fn the_next_launch_keeps_the_saved_settings() {
    let (dir, storage) = storage_with(FIXTURE);
    let path = dir.join(FILE_NAME);
    let mut settings = NativeSettings::load_or_import(&path, &storage);
    settings.ui_lang = "uk".into();
    settings.save(&path).unwrap();
    assert_eq!(
      NativeSettings::load_or_import(&path, &storage).ui_lang,
      "uk"
    );
  }

  #[test]
  fn a_corrupt_settings_file_imports_again_without_panic() {
    let (dir, storage) = storage_with(FIXTURE);
    let path = dir.join(FILE_NAME);
    fs::write(&path, "{ pas du json").unwrap();
    assert_eq!(
      NativeSettings::load_or_import(&path, &storage).ui_lang,
      "ru"
    );
  }

  #[test]
  fn a_field_missing_from_the_settings_file_keeps_its_default() {
    let (dir, storage) = storage_with("{}");
    let path = dir.join(FILE_NAME);
    fs::write(&path, r#"{ "uiLang": "uk" }"#).unwrap();
    let settings = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(
      settings,
      NativeSettings {
        ui_lang: "uk".into(),
        ..NativeSettings::default()
      }
    );
  }

  #[test]
  fn an_unknown_key_of_the_settings_file_is_kept_on_save() {
    let (dir, storage) = storage_with("{}");
    let path = dir.join(FILE_NAME);
    fs::write(&path, r#"{ "uiLang": "uk", "cleFuture": { "a": 1 } }"#).unwrap();
    let settings = NativeSettings::load_or_import(&path, &storage);
    settings.save(&path).unwrap();
    let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["cleFuture"], json!({ "a": 1 }));
    assert_eq!(written["uiLang"], "uk");
  }

  #[test]
  fn a_field_added_later_comes_from_riff_and_a_saved_field_wins() {
    let (dir, storage) = storage_with(r#"{ "riff_user_nickname": "Hugo", "riff_ui_lang": "ru" }"#);
    let path = dir.join(FILE_NAME);
    fs::write(&path, r#"{ "uiLang": "uk" }"#).unwrap();
    let settings = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(settings.nickname, "Hugo");
    assert_eq!(settings.ui_lang, "uk");
  }

  #[test]
  fn the_defaults_match_the_page_of_riff() {
    let settings = NativeSettings::default();
    assert_eq!(settings.ui_scale, 105);
    assert!(settings.play_btn_bg);
    assert!(settings.show_explicit);
    assert!(settings.highlight_buttons);
    assert!(settings.search_albums);
    assert!(settings.show_local);
    assert!(!settings.show_albums);
    assert_eq!(settings.home_layout, "spotify");
    assert_eq!(settings.nickname, "User");
    assert_eq!(settings.settings_cat, "interface");
  }

  #[test]
  fn each_page_setting_is_imported_from_its_key() {
    let (_, storage) = storage_with(
      r#"{
        "devsize_ui_scale": "115",
        "riff_compact_engine_btns": "1",
        "riff_hide_engine_btns": "1",
        "riff_play_btn_bg": "false",
        "riff_show_explicit": "0",
        "riff_home_order": "[\"sp-trend-sec\",\"inconnue\"]",
        "riff_adv_intensity_bg": "true",
        "riff_highlight_buttons": "0",
        "riff_adv_albums": "false",
        "riff_classic_track_rows": "1",
        "riff_show_local": "false",
        "riff_show_albums": "1",
        "riff_adv_hide_titlebar": "true",
        "riff_adv_my_wave": "false",
        "riff_adv_jump_back": "false",
        "riff_adv_fav_artists": "false",
        "riff_home_layout": "riff",
        "riff_user_nickname": "Hugo",
        "riff_user_avatar": "a.png",
        "riff_settings_cat": "theme"
      }"#,
    );
    let settings = NativeSettings::imported_from(&storage);
    assert_eq!(settings.ui_scale, 115);
    assert!(settings.compact_engine_btns);
    assert!(settings.hide_engine_btns);
    assert!(!settings.play_btn_bg);
    assert!(!settings.show_explicit);
    assert_eq!(
      settings.home_order,
      [
        "sp-trend-sec",
        "sp-made-sec",
        "sp-quick-sec",
        "sp-artists-sec"
      ]
    );
    assert!(settings.intensity_bg);
    assert!(!settings.highlight_buttons);
    assert!(!settings.search_albums);
    assert!(settings.classic_track_rows);
    assert!(!settings.show_local);
    assert!(settings.show_albums);
    assert!(settings.hide_titlebar);
    assert!(!settings.home_wave);
    assert!(!settings.home_recent);
    assert!(!settings.home_artists);
    assert_eq!(settings.home_layout, "riff");
    assert_eq!(settings.nickname, "Hugo");
    assert_eq!(settings.avatar.as_deref(), Some("a.png"));
    assert_eq!(settings.settings_cat, "theme");
  }

  #[test]
  fn the_settings_survive_an_account_round_trip() {
    let settings = NativeSettings {
      ui_lang: "uk".into(),
      theme_mode: ThemeMode::Light,
      theme_brightness: 33,
      ui_scale: 115,
      show_albums: true,
      play_btn_bg: false,
      home_layout: "riff".into(),
      nickname: "Hugo".into(),
      download_format: "flac".into(),
      proxy_list: vec!["http://p:1".into()],
      proxy_enabled: true,
      spotify_client_id: "abc".into(),
      lastfm_user: "moi".into(),
      ..NativeSettings::default()
    };
    let read =
      NativeSettings::imported_from(&LegacyStorage::from_entries(&settings.to_legacy_entries()));
    assert_eq!(read, settings);
  }

  #[test]
  fn an_account_value_that_is_not_a_text_is_written_as_json() {
    let entries = json!({ "riff_x": [1, 2], "riff_y": "a", "riff_z": null });
    let storage = LegacyStorage::from_entries(entries.as_object().unwrap());
    assert_eq!(storage.get("riff_x"), Some(&json!("[1,2]")));
    assert_eq!(storage.get("riff_y"), Some(&json!("a")));
    assert_eq!(storage.get("riff_z"), None);
  }

  #[test]
  fn an_out_of_range_scale_keeps_the_default() {
    let (_, storage) = storage_with(r#"{ "devsize_ui_scale": "400" }"#);
    assert_eq!(NativeSettings::imported_from(&storage).ui_scale, 105);
  }

  #[test]
  fn a_settings_file_without_visual_section_imports_it_from_riff() {
    let (dir, storage) = storage_with(r#"{ "riff_bar_wave": "1" }"#);
    let path = dir.join(FILE_NAME);
    fs::write(&path, r#"{ "uiLang": "uk" }"#).unwrap();
    let settings = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(settings.ui_lang, "uk");
    assert!(settings.visual.bar_wave);
  }

  #[test]
  fn a_saved_visual_section_wins_over_riff() {
    let (dir, storage) = storage_with(r#"{ "riff_bar_wave": "1" }"#);
    let path = dir.join(FILE_NAME);
    fs::write(&path, r#"{ "visual": { "barWave": false } }"#).unwrap();
    assert!(
      !NativeSettings::load_or_import(&path, &storage)
        .visual
        .bar_wave
    );
  }

  #[test]
  fn the_karaoke_style_is_saved_in_the_visual_section_and_its_unknown_key_kept() {
    let (dir, storage) = storage_with(r#"{ "riff_tvk": "{\"size\":120,\"future\":7}" }"#);
    let path = dir.join(FILE_NAME);
    let mut settings = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(settings.visual.tvk.size, 120);
    settings.visual.tvk.anim = crate::visuals::settings::TvkAnim::Fade;
    settings.visual.tvk_cover_mode = true;
    settings.save(&path).unwrap();
    let file: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(file["visual"]["tvk"]["anim"], json!("fade"));
    assert_eq!(file["visual"]["tvk"]["future"], json!(7));
    assert_eq!(file["visual"]["tvkCoverMode"], json!(true));
    let read = NativeSettings::load_or_import(&path, &LegacyStorage::from_entries(&Map::new()));
    assert_eq!(read.visual.tvk, settings.visual.tvk);
    assert!(read.visual.tvk_cover_mode);
  }

  #[test]
  fn legacy_storage_reads_any_key() {
    let (_, storage) = storage_with(FIXTURE);
    assert_eq!(storage.get("riff_ui_lang"), Some(&json!("ru")));
    assert_eq!(storage.get("absente"), None);
  }

  #[test]
  fn a_corrupt_settings_file_is_kept_aside() {
    let (dir, storage) = storage_with(r#"{ "riff_ui_lang": "uk" }"#);
    let path = dir.join(FILE_NAME);
    fs::write(&path, b"{ broken json").unwrap();
    let settings = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(settings.ui_lang, "uk");
    assert_eq!(
      fs::read_to_string(path.with_extension("json.corrupt")).unwrap(),
      "{ broken json"
    );
  }

  #[test]
  fn a_utf8_bom_in_front_of_the_settings_file_is_ignored() {
    let (dir, storage) = storage_with(r#"{ "riff_ui_lang": "uk" }"#);
    let path = dir.join(FILE_NAME);
    fs::write(&path, "\u{feff}{ \"uiLang\": \"ru\" }").unwrap();
    let settings = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(settings.ui_lang, "ru");
    assert!(!path.with_extension("json.corrupt").exists());
    settings.save(&path).unwrap();
    assert!(!fs::read(&path).unwrap().starts_with(&[0xEF, 0xBB, 0xBF]));
  }

  #[test]
  fn the_keys_read_are_tracked_but_not_compared() {
    let (_, storage) = storage_with(FIXTURE);
    let untouched = storage.untracked_copy();
    assert!(storage.read_keys().is_empty());
    let _ = storage.text("riff_ui_lang");
    let _ = storage.get("absente");
    assert_eq!(
      storage.read_keys().into_iter().collect::<Vec<_>>(),
      ["absente", "riff_ui_lang"]
    );
    assert_eq!(storage, untouched);
    assert!(untouched.read_keys().is_empty());
  }

  #[test]
  fn the_debug_of_a_storage_never_shows_a_value() {
    let (_, storage) = storage_with(FIXTURE);
    let shown = format!("{storage:?}");
    assert!(!shown.contains("jeton-de-test"));
    assert!(shown.contains("riff_deepl_api_key"));
  }

  #[test]
  fn the_global_media_keys_map_to_the_media_session() {
    let from = |content: &str| NativeSettings::imported_from(&storage_with(content).1);
    assert!(!from(r#"{ "riff_win_global_media_keys": "false" }"#).media_session);
    assert!(from(r#"{ "riff_win_global_media_keys": "true" }"#).media_session);
    assert!(from(r#"{ "riff_ui_lang": "uk" }"#).media_session);
  }

  #[test]
  fn the_media_session_survives_an_account_round_trip() {
    let settings = NativeSettings {
      media_session: false,
      ..NativeSettings::default()
    };
    let read =
      NativeSettings::imported_from(&LegacyStorage::from_entries(&settings.to_legacy_entries()));
    assert!(!read.media_session);
  }

  #[test]
  fn the_tray_on_close_import_follows_the_key_or_the_native_default() {
    let from = |content: &str| NativeSettings::imported_from(&storage_with(content).1);
    assert!(from(r#"{ "riff_tray_on_close": "true" }"#).tray_on_close);
    assert!(!from(r#"{ "riff_tray_on_close": "false" }"#).tray_on_close);
    assert_eq!(
      from(r#"{ "riff_ui_lang": "uk" }"#).tray_on_close,
      NativeSettings::default().tray_on_close
    );
  }

  #[test]
  fn the_main_always_on_top_setting_is_off_by_default_and_survives_a_round_trip() {
    assert!(!NativeSettings::default().main_always_on_top);
    let from = |content: &str| NativeSettings::imported_from(&storage_with(content).1);
    assert!(from(r#"{ "riff_main_always_on_top": "true" }"#).main_always_on_top);
    assert!(!from(r#"{ "riff_main_always_on_top": "false" }"#).main_always_on_top);
    assert!(!from(r#"{ "riff_ui_lang": "uk" }"#).main_always_on_top);
    let settings = NativeSettings {
      main_always_on_top: true,
      ..NativeSettings::default()
    };
    let read =
      NativeSettings::imported_from(&LegacyStorage::from_entries(&settings.to_legacy_entries()));
    assert!(read.main_always_on_top);
  }

  #[test]
  fn the_discord_presence_import_is_off_only_for_false() {
    let from = |content: &str| NativeSettings::imported_from(&storage_with(content).1);
    assert!(!from(r#"{ "devsize_discord_rpc": "false" }"#).discord_rpc);
    assert!(from(r#"{ "devsize_discord_rpc": "true" }"#).discord_rpc);
    assert!(from(r#"{ "riff_ui_lang": "uk" }"#).discord_rpc);
  }

  #[test]
  fn the_discord_artist_status_import_is_off_only_for_zero() {
    let from = |content: &str| NativeSettings::imported_from(&storage_with(content).1);
    assert!(!from(r#"{ "riff_discord_artist_status": "0" }"#).discord_artist_status);
    assert!(from(r#"{ "riff_discord_artist_status": "1" }"#).discord_artist_status);
    assert!(from(r#"{ "riff_ui_lang": "uk" }"#).discord_artist_status);
  }

  #[test]
  fn the_tray_and_discord_settings_survive_an_account_round_trip() {
    let round_trip = |tray_on_close, discord_rpc, discord_artist_status| {
      let settings = NativeSettings {
        tray_on_close,
        discord_rpc,
        discord_artist_status,
        ..NativeSettings::default()
      };
      NativeSettings::imported_from(&LegacyStorage::from_entries(&settings.to_legacy_entries()))
    };
    let off = round_trip(false, false, false);
    assert!(!off.tray_on_close && !off.discord_rpc && !off.discord_artist_status);
    let on = round_trip(true, true, true);
    assert!(on.tray_on_close && on.discord_rpc && on.discord_artist_status);
  }

  #[test]
  fn the_motion_settings_come_from_their_keys_and_follow_the_system_when_absent() {
    let from = |content: &str| NativeSettings::imported_from(&storage_with(content).1);
    let blank = from("{}");
    assert_eq!(
      blank.reduce_motion, None,
      "absent : la préférence du système"
    );
    assert!(!blank.perf_mode, "absent : mode performance désactivé");
    assert_eq!(
      from(r#"{ "riff_adv_reduce_motion": "true" }"#).reduce_motion,
      Some(true)
    );
    assert_eq!(
      from(r#"{ "riff_adv_reduce_motion": "false" }"#).reduce_motion,
      Some(false)
    );
    assert!(from(r#"{ "riff_perf_mode": "1" }"#).perf_mode);
    assert!(!from(r#"{ "riff_perf_mode": "0" }"#).perf_mode);
    assert!(
      !from(r#"{ "riff_perf_mode": "true" }"#).perf_mode,
      "`=== '1'`, `app.js:17167`"
    );
    assert_eq!(NativeSettings::fresh().reduce_motion, None);
  }

  #[test]
  fn the_motion_settings_are_read_not_kept_in_legacy_and_survive_an_account_round_trip() {
    let (_, storage) =
      storage_with(r#"{ "riff_adv_reduce_motion": "true", "riff_perf_mode": "1" }"#);
    let settings = NativeSettings::imported_from(&storage);
    assert!(settings.legacy.is_empty());
    let report = import_report::ImportReport::new(&storage);
    assert!(report.not_ported.is_empty(), "{:?}", report.not_ported);
    assert_eq!(
      report.imported,
      ["riff_adv_reduce_motion", "riff_perf_mode"]
    );
    let entries = settings.to_legacy_entries();
    assert_eq!(entries["riff_adv_reduce_motion"], json!("true"));
    assert_eq!(entries["riff_perf_mode"], json!("1"));
    let again = NativeSettings::imported_from(&LegacyStorage::from_entries(&entries));
    assert_eq!((again.reduce_motion, again.perf_mode), (Some(true), true));
    // Choix jamais fait : rien n'est écrit pour la réduction, le système décide encore au prochain lancement.
    let untouched = NativeSettings::default().to_legacy_entries();
    assert!(!untouched.contains_key("riff_adv_reduce_motion"));
    assert_eq!(untouched["riff_perf_mode"], json!("0"));
  }

  #[test]
  fn an_unset_reduction_is_not_written_to_the_settings_file() {
    let value = serde_json::to_value(NativeSettings::default()).unwrap();
    assert!(value.get("reduceMotion").is_none());
    assert_eq!(value["perfMode"], json!(false));
    let chosen = serde_json::to_value(NativeSettings {
      reduce_motion: Some(false),
      ..NativeSettings::default()
    })
    .unwrap();
    assert_eq!(chosen["reduceMotion"], json!(false));
  }

  const DECISION_4_KEYS: [&str; 15] = [
    "riff_ignored_update_ver",
    "riff_font",
    "riff_font_scale",
    "riff_corner_radius",
    "riff_ui_alpha",
    "riff_panel_blur",
    "riff_icon_pack",
    "riff_app_icon",
    "riff_wavy_progress",
    "riff_progress_knob",
    "riff_progress_bg",
    "riff_no_hand_cursor",
    "riff_custom_css",
    "riff_m3_hero_covers",
    "riff_home_pane",
  ];

  #[test]
  fn legacy_keeps_the_keys_without_native_equivalent() {
    let mut entries = Map::new();
    for key in DECISION_4_KEYS {
      entries.insert(key.to_string(), json!(format!("valeur de {key}")));
    }
    entries.insert("riff_une_cle_future".to_string(), json!("x"));
    entries.insert("riff_ui_lang".to_string(), json!("uk"));
    let legacy = NativeSettings::imported_from(&LegacyStorage::from_entries(&entries)).legacy;
    for key in DECISION_4_KEYS {
      assert_eq!(legacy[key], json!(format!("valeur de {key}")), "{key}");
    }
    assert!(legacy.contains_key("riff_une_cle_future"));
    assert!(!legacy.contains_key("riff_ui_lang"), "clé déjà reprise");
  }

  #[test]
  fn legacy_leaves_out_secrets_caches_markers_and_the_queue_panel() {
    let (_, storage) = storage_with(
      r#"{
        "riff_lastfm_apisecret": "S-1", "riff_lastfm_sk": "S-2", "riff_deepl_api_key": "S-3",
        "riff_lastfm_apikey": "K-4",
        "riff_artist_meta_cache": "{}", "riff_i18n_fr": "{}", "riff_foo_cache": "{}",
        "riff_scale_rebased_v2": "1", "riff_defaults_v44_applied": "1", "riff_defaults_v44_hue246": "1",
        "riff_tvk_blur_reset_v1": "1", "riff_stats": "{}", "riff_keyboard_shortcuts": "{}",
        "riff_queue_panel_open": "true",
        "riff_font": "mono"
      }"#,
    );
    let settings = NativeSettings::imported_from(&storage);
    assert_eq!(settings.legacy.keys().collect::<Vec<_>>(), ["riff_font"]);
    let serialized = serde_json::to_string(&settings).unwrap();
    for secret in ["S-1", "S-2", "S-3", "K-4"] {
      assert!(!serialized.contains(secret), "{secret}");
    }
  }

  #[test]
  fn data_keys_never_reach_legacy() {
    let (_, storage) = storage_with(
      r#"{ "devsize_favorites": "[]", "riff_volume": "0.3", "riff_playlist_sort": "added-desc" }"#,
    );
    assert!(NativeSettings::imported_from(&storage).legacy.is_empty());
  }

  #[test]
  fn legacy_survives_a_save_and_a_load() {
    let (dir, storage) = storage_with(r#"{ "riff_font": "mono" }"#);
    let path = dir.join(FILE_NAME);
    let first = NativeSettings::load_or_import(&path, &storage);
    let again = NativeSettings::load_or_import(&path, &LegacyStorage::default());
    assert_eq!(again.legacy, first.legacy);
    assert_eq!(again.legacy["riff_font"], json!("mono"));
  }

  #[test]
  fn a_file_still_holding_the_removed_autohide_key_loads_and_keeps_the_other_settings() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    fs::write(
      &path,
      r#"{ "uiLang": "uk", "uiScale": 120, "autohideTimestamps": true }"#,
    )
    .unwrap();
    let settings = NativeSettings::load_or_import(&path, &LegacyStorage::default());
    assert_eq!(settings.ui_lang, "uk");
    assert_eq!(settings.ui_scale, 120);
    assert!(!path.with_extension("json.corrupt").exists());
  }

  #[test]
  fn an_old_settings_file_without_the_new_keys_round_trips_unchanged() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    fs::write(&path, r#"{ "uiLang": "uk", "futureKey": 3 }"#).unwrap();
    let settings = NativeSettings::load_or_import(&path, &LegacyStorage::default());
    assert!(settings.legacy.is_empty());
    assert_eq!(settings.import_source, ImportSource::Riff);
    let out = dir.join("out.json");
    settings.save(&out).unwrap();
    let file: Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(file["uiLang"], json!("uk"));
    assert_eq!(file["futureKey"], json!(3));
    assert!(file.get("legacy").is_none());
    assert!(file.get("importSource").is_none());
  }

  #[test]
  fn the_import_source_is_saved_only_when_fresh() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    NativeSettings::fresh().save(&path).unwrap();
    let file: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(file["importSource"], json!("fresh"));
    let read: NativeSettings = serde_json::from_str(r#"{ "importSource": "autre" }"#).unwrap();
    assert_eq!(read.import_source, ImportSource::Riff);
  }

  #[test]
  fn a_fresh_start_never_completes_a_missing_field_from_riff() {
    let (dir, storage) = storage_with(
      r#"{ "riff_ui_lang": "ru", "riff_font": "mono", "riff_win_global_media_keys": "false" }"#,
    );
    let path = dir.join(FILE_NAME);
    fs::write(&path, r#"{ "importSource": "fresh", "trLang": "uk" }"#).unwrap();
    let fresh = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(fresh.ui_lang, "en");
    assert!(fresh.media_session);
    assert!(fresh.legacy.is_empty());
    assert_eq!(fresh.tr_lang, "uk");
    assert_eq!(fresh.import_source, ImportSource::Fresh);
    fs::write(&path, r#"{ "importSource": "riff" }"#).unwrap();
    let riff = NativeSettings::load_or_import(&path, &storage);
    assert_eq!(riff.ui_lang, "ru");
    assert!(!riff.media_session);
    assert!(riff.legacy.contains_key("riff_font"));
  }
}
