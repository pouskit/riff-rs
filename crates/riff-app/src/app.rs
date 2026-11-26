//! État de l'application, messages, mise à jour et vue (architecture Elm).
//!
//! [`App::update`] ne fait aucune E/S : il change l'état et renvoie des [`Effect`], que [`Program`] convertit en tâches.

use crate::audio_prefs::{self, AudioMessage};
use crate::audio_sub::{self, AudioEvents};
use crate::backend::{Backend, LOCAL_BASE_URL};
use crate::bench::{
  BLUR_MAX_MICROS, LatencyLog, SceneArgs, SceneFrames, ScrollBench, VIEW_COUNT, finish_scene_after,
};
use crate::core_sub;
use crate::download_ui::{self, DownloadMessage};
use crate::i18n::{self, I18n};
use crate::images::{self, ImageCache, ImageKey};
use crate::import_flow::{self, ImportChoice, ImportMessage, ImportState};
use crate::jam::JamMessage;
use crate::lyrics::state::{LyricsState, SCROLL_ID};
use crate::lyrics::update::{LyricsMessage, MIN_TICK, PERF_TICK};
use crate::manage::{self, LastfmVaultItem, ManageMessage};
use crate::player::PlayerState;
use crate::plugins::{PluginAnswer, PluginMessage};
use crate::prefs::SettingsMessage;
use crate::prefs_visual::VisualMessage;
use crate::screens::dev::{self, DevMessage, DevState, TrackRow};
use crate::screens::import as import_screen;
use crate::screens::{
  Screen, backdrop, chrome as screen_chrome, common, compact, home as home_screen,
  lyrics as lyrics_screen, right_panel, window_frame,
};
use crate::settings::{self, LegacyStorage, NativeSettings, ThemeMode};
use crate::stream::{self, ResolveOptions};
use crate::system::side::{SystemSide, tray_labels};
use crate::system::{SystemEvent, autostart, update_install, updates};
use crate::theme::file::{self, LoadedTheme};
use crate::theme::{ActiveTheme, cover};
use crate::ui::{ListId, PlayerMessage, ReplyTag, Ui, UiMessage};
use crate::user_data::{self, Track, UserData};
use crate::visuals::blur::{self, BlurKey};
use crate::visuals::settings::{MiniWindow, WordStyle};
use crate::visuals::state::{DRIVER_WARMUP_FRAMES, VisualState};
use crate::visuals::viz::FREQ_BINS;
use crate::widgets::OVERLAY_ID;
use crate::widgets::dialog::{self, Dialog};
use crate::widgets::menu::{self, Menu};
use crate::widgets::ticker::Ticker;
use crate::widgets::toast::{self, ToastKind, Toasts};
use crate::window::{
  self as chrome, MIN_HEIGHT, MIN_WIDTH, MINI_MIN, MINI_SIZE, PersistedWindowState, WindowTracker,
};
use iced::advanced::widget::operation::{self, focusable};
use iced::advanced::widget::{Operation, operate};
use iced::alignment::{Horizontal, Vertical};
use iced::event::{self, Event};
use iced::futures::StreamExt;
use iced::keyboard::{self, key};
use iced::widget::{Stack, canvas, column, container, scrollable, space};
use iced::window;
use iced::{Color, Element, Fill, Point, Size, Subscription, Task, Theme};
use material_colors::color::Rgb;
use riff_audio::normalize::NormalizeState;
use riff_audio::waveform;
use riff_audio::{Event as AudioEvent, Source};
use riff_core::events::CoreEvent;
use riff_core::reply::Reply;
use riff_core::{config, paths};
use riff_plugins::{Lifecycle, Outgoing};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::form_urlencoded;

/// Taille maximale de l'historique de navigation.
pub const MAX_BACK_STACK: usize = 50;
const NORMALIZE_TICK: Duration = Duration::from_millis(100);
const SAVE_DELAY: Duration = Duration::from_millis(500);
const DROP_DELAY: Duration = Duration::from_millis(150);
const AUTOPLAY_ARTIST_TIMEOUT: Duration = Duration::from_secs(8);
const AUTOPLAY_VIBE_TIMEOUT: Duration = Duration::from_secs(9);
const AUTOPLAY_PER_ARTIST: usize = 3;
pub(crate) const DEFAULT_WINDOW: Size = Size::new(1280.0, 800.0);

impl From<UiMessage> for Message {
  fn from(message: UiMessage) -> Self {
    Self::Ui(message)
  }
}

/// Messages de l'application.
#[derive(Debug, Clone)]
pub enum Message {
  WindowOpened {
    position: Option<Point>,
    size: Size,
  },
  WindowMoved(Point),
  WindowResized(Size),
  WindowRescaled(f32),
  ScaleKnown(f32),
  MaximizedState(bool),
  CloseRequested,
  CloseStateKnown {
    is_maximized: bool,
    is_full_screen: bool,
  },
  Chrome(chrome::Action),
  SetThemeMode(ThemeMode),
  CoverBytes(Arc<[u8]>),
  CoverColor(Rgb),
  SelectThemeFile(String),
  ThemeFileLoaded(String, Result<Box<LoadedTheme>, String>),
  ClearThemeFile,
  SetLanguage(String),
  LanguageLoaded(Arc<I18n>),
  Navigate(Screen),
  Back,
  Audio(AudioEvent),
  Core(CoreEvent),
  LoadFile(PathBuf),
  OpenMenu(Box<Menu<Message>>),
  CloseMenu,
  MenuChoose(Box<Message>),
  OpenDialog(Dialog, Option<Box<Message>>),
  CloseDialog,
  ConfirmDialog,
  ShowToast(String, ToastKind, Option<Duration>),
  ToastExpired(u64),
  /// Fin de la minuterie `(id, génération)` arrivée à l'instant donné ; sans effet si un survol l'a annulée.
  ToastTimeout(u64, u32, Instant),
  /// Le pointeur entre dans la notification `id` (vrai) ou en sort (faux) à l'instant donné (`mouseenter`, `mouseleave`, `app.js:534-544`).
  ToastHover(u64, bool, Instant),
  ImageLoaded(ImageKey, Result<Arc<images::Decoded>, String>),
  Escape,
  FocusMove {
    back: bool,
  },
  Dev(DevMessage),
  Ui(UiMessage),
  Player(PlayerMessage),
  Settings(SettingsMessage),
  AudioPrefs(AudioMessage),
  Manage(ManageMessage),
  Import(ImportMessage),
  Download(DownloadMessage),
  ProbeFrame,
  BenchFrame(Instant),
  Lyrics(LyricsMessage),
  /// Une image du minuteur d'animation (décision 4).
  AnimationFrame(Instant),
  BlurReady(BlurKey, Option<Arc<images::Decoded>>),
  /// Événement d'une fenêtre : `Program` l'adresse à la fenêtre principale ou aux mini-paroles.
  ForWindow(window::Id, Box<Message>),
  ScenePause,
  /// Raccourci lettre : sans effet sous une boîte de dialogue.
  Shortcut(Box<Message>),
  /// Événement du système : seconde instance, touche média, menu de la zone de notification, raccourci global.
  System(SystemEvent),
  /// Bascule « Always on top » de la fenêtre principale (menu de la zone de notification).
  ToggleMainAlwaysOnTop,
  /// Identifiant natif de la fenêtre principale (HWND sous Windows), pour la session média.
  MainHandle(u64),
  /// Pochette du titre en cours, chargée pour la session média.
  SystemCover(String, Option<Arc<images::Decoded>>),
  /// « Vérifier les mises à jour » : une requête, sur clic seulement.
  UpdateCheck,
  UpdateChecked(Result<updates::Outcome, updates::CheckError>),
  /// « Download and install » : accord explicite, sur clic seulement.
  UpdateInstall,
  UpdateInstallEvent(update_install::InstallEvent),
  /// Ouvre la page de la release proposée dans le navigateur.
  OpenReleasePage,
  /// Écrit les raccourcis du Menu Démarrer et du Bureau, sur clic seulement.
  LauncherInstall,
  /// Bascule la fenêtre principale vers la disposition compacte, ou la rétablit.
  ToggleMiniMode,
  /// Bouton `#btn-mini-lyrics` du mode compact : affiche ou masque les paroles dans la fenêtre.
  ToggleMiniLyrics,
  /// Ouvre le dialogue de vérification des mises à jour (« à propos »).
  NoOp,
  /// Jam : écoute partagée.
  Jam(crate::jam::JamMessage),
  /// Plugins Rhai.
  Plugin(crate::plugins::PluginMessage),
  /// Éditeur de code.
  Editor(crate::code_editor::EditorMessage),
  /// Éditeur de thème.
  ThemeEditor(crate::theme_editor::ThemeEditorMessage),
}

/// Source audio dans un effet : deux sources se comparent par leur description.
#[derive(Debug, Clone)]
pub struct SourceEffect(pub Source);

impl PartialEq for SourceEffect {
  fn eq(&self, other: &Self) -> bool {
    format!("{:?}", self.0) == format!("{:?}", other.0)
  }
}

/// Requête d'un mix quotidien.
#[derive(Debug, Clone, PartialEq)]
pub struct MixJob {
  pub id: String,
  pub title: String,
  pub artists: Vec<String>,
  pub body: String,
}

/// Effets demandés par [`App::update`].
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
  QueryScale,
  QueryMaximized,
  ApplyBounds(Point, Size),
  QueryCloseState,
  /// Les données ne sont écrites que si elles sont présentes : un écran d'import sans choix les retient.
  SaveWindowAndExit(PersistedWindowState, Option<Box<UserData>>),
  /// Choix de l'écran d'import, avec le `storage.json` lu au lancement.
  ImportChoice(ImportChoice, Box<LegacyStorage>),
  Chrome(chrome::Action),
  SaveSettings(Box<NativeSettings>),
  ExtractCover(Arc<[u8]>),
  LoadThemeFile(String),
  LoadLanguage(String),
  AudioLoad(PathBuf),
  FocusOverlay,
  FocusNext {
    in_overlay: bool,
  },
  FocusPrevious {
    in_overlay: bool,
  },
  Unfocus,
  /// Minuterie de la notification : identifiant, génération, durée.
  ToastTimer(u64, u32, Duration),
  AudioPlay,
  AudioPause,
  AudioSeek(Duration),
  AudioVolume(f32),
  AudioFadeOut(f32),
  AudioFadeIn(f32),
  /// Minuterie de sommeil : message `SleepDue` de cette génération après la durée.
  SleepAlarm(u64, Duration),
  /// Fin du fondu de la minuterie de sommeil : message `SleepFaded` après la durée.
  SleepFadeWait(Duration),
  AudioMix(f32, bool),
  NormalizeGain(f32),
  AudioLoadSource(SourceEffect, Duration),
  AudioCrossfade(SourceEffect, u32),
  AudioDevice(Option<String>),
  DevSearch(String, &'static str),
  ListDevices,
  ListThemes,
  FetchCover(String),
  ReadCoverFile(PathBuf),
  PublishState,
  PublishAppState(Value),
  ResolveTrack {
    token: u64,
    track: Box<Track>,
    options: ResolveOptions,
    start: Duration,
    crossfade: Option<u32>,
  },
  CoreJson {
    tag: ReplyTag,
    path: &'static str,
    query: Vec<(String, String)>,
    body: Option<String>,
    /// Requête de recherche : annulée par la recherche suivante.
    search: bool,
  },
  AbortSearch,
  ScheduleSave(u64),
  WriteUserData(Box<UserData>),
  /// File et titre à reprendre au prochain lancement.
  SaveResume(Box<crate::resume::ResumeState>),
  /// Position à reprendre au prochain lancement.
  SaveResumePosition(crate::resume::ResumePosition),
  /// Message `ResumeRetry` de ce jeton après la durée.
  ResumeRetry(u64, Duration),
  Toast(String, ToastKind),
  PrepareAutoplay {
    artists: Vec<String>,
    payload: Value,
  },
  GenerateMixes {
    day: String,
    jobs: Vec<MixJob>,
  },
  FocusId(&'static str),
  CopyText(String),
  PickFiles,
  PickFolder,
  PickPlaylistCover(String),
  PickFavoritesCover,
  /// Image d'une bannière ou d'un fond.
  PickMedia(crate::prefs_visual::MediaTarget),
  ExportPlaylist {
    name: String,
    content: String,
  },
  ScanLocal(Vec<PathBuf>, bool),
  ScanDropped(Vec<PathBuf>),
  RescanLocal(Vec<String>),
  UpdateYtDlp,
  SetTranslation(bool),
  ListSettingsThemes,
  AudioSound(Box<riff_audio::AudioSettings>),
  AudioHq(bool, riff_audio::HqSettings),
  ListSettingsDevices,
  LoadLocalInfo,
  OpenUrl(String),
  SpotifyLogin(String),
  GoogleLogin,
  SaveLastfmKeys {
    key: String,
    secret: String,
  },
  SaveLastfmSession(Option<String>),
  PickCookies,
  BrowserCookies(String),
  DeleteCookies,
  PickPlaylistsJson,
  /// Fichiers d'export Spotify ou Apple Music, lus puis analysés hors du fil d'interface.
  PickExportFiles(riff_core::import_files::ExportSource),
  ClearCache,
  PickDataFolder(manage::FolderKind),
  ResetDownloadFolder,
  OpenLogs,
  CopyDiagnostics,
  ExportAccount(Box<Value>),
  PickAccount,
  SetYtDlpAutoUpdate(bool),
  SetMultiInstance(bool),
  RunDownload(Box<download_ui::DownloadJob>),
  CheckUpdate,
  /// Télécharge, vérifie puis lance l'installeur de cette release.
  InstallUpdate(updates::Release),
  InstallLauncher,
  EnterMiniWindow,
  /// Taille minimale puis taille de la fenêtre compacte quand les paroles s'ouvrent ou se ferment.
  ResizeMini(Size, Size),
  LeaveMiniWindow(Point, Size, bool),
  AbortDownload,
  LoadWave {
    token: u64,
    track: Box<Track>,
  },
  DropTimer(u64),
  /// Message `PanelControlsDue` de cette génération après la durée.
  PanelControlsTimer(u64, Duration),
  RestoreFocus(Option<usize>),
  LyricsScroll(f32),
  /// Remet le flux de la bibliothèque de la barre latérale en haut (changement d'onglet).
  SidebarFeedTop,
  OpenMini(MiniWindow),
  CloseMini,
  MiniLevel(bool),
  /// Niveau de la fenêtre principale : au premier plan ou normal.
  MainLevel(bool),
  MiniDrag,
  /// Demande `shuffle_pick(ctx)` au plugin de mélange.
  PluginPick {
    id: u64,
    ctx: Value,
  },
  SavePluginConsent(Box<riff_plugins::Consent>),
  ScanPlugins,
  /// Relit la source avant de l'activer : le consentement porte sur ce qui sera exécuté.
  LoadPlugin(String),
  ActivatePlugin(Box<riff_plugins::PluginSource>),
  DeactivatePlugin(String),
  RemovePlugin(String),
  PickPlugins,
  OpenPluginsFolder,
  OpenEditorFile(crate::code_editor::EditorTarget),
  SaveEditorFile(crate::code_editor::EditorTarget, String),
  ReadThemeDraft(String),
  SaveThemeDraft(String, String, bool),
  /// Lance un jam d'hôte.
  JamHost {
    name: String,
    relay: Option<String>,
    rendezvous: Option<String>,
  },
  /// Rejoint le jam du code d'invitation.
  JamJoin {
    ticket: String,
    name: String,
    relay: Option<String>,
    rendezvous: Option<String>,
  },
  /// Message vers la session réseau en cours.
  JamSend(crate::jam::JamOut),
  /// Lit le presse-papiers pour le champ du code.
  JamPaste,
}

/// État de lecture vu par l'interface.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playback {
  pub position: Duration,
  pub duration: Option<Duration>,
  pub sample_rate: Option<u32>,
  pub is_playing: bool,
  pub buffering: bool,
  pub device: Option<String>,
  pub error: Option<String>,
  /// La lecture jouait à la perte du périphérique : le moteur reprend au retour.
  pub resume_on_device: bool,
}

/// État de l'application, testable sans fenêtre.
#[derive(Debug, Clone)]
pub struct App {
  pub window: WindowTracker,
  pub settings: NativeSettings,
  pub theme: ActiveTheme,
  pub last_theme_error: Option<String>,
  pub i18n: Arc<I18n>,
  pub screen: Screen,
  pub back_stack: Vec<Screen>,
  pub playback: Playback,
  pub last_core_event: Option<CoreEvent>,
  pub menu: Option<Menu<Message>>,
  /// Menu fermé qui finit de s'effacer (`.riff-ctx-menu` reste dans le DOM sans `.open`, `app.js:10659`).
  pub menu_leaving: Option<Menu<Message>>,
  /// Ouverture et fermeture du menu contextuel et de la modale des réglages : actives dans l'application lancée seulement.
  pub overlay_motion: crate::motion::OverlayMotion,
  pub dialog: Option<Dialog>,
  pub dialog_action: Option<Box<Message>>,
  pub toasts: Toasts,
  /// État de l'écran de développement, seulement avec `--dev`.
  pub dev: Option<DevState>,
  /// Données de l'utilisateur, enregistrées dans `user-data.json`.
  pub data: UserData,
  pub data_generation: u64,
  pub player: PlayerState,
  pub normalize: NormalizeState,
  pub ui: Ui,
  pub images: ImageCache,
  pub lyrics: LyricsState,
  pub visuals: VisualState,
  /// Horloge des animations : les vues la lisent au lieu de `Instant::now()`. Mise à jour par `Message::AnimationFrame`, réglable par le banc de rendu.
  pub now: Instant,
  /// Instant de création : origine de phase des animations sans départ propre (`.g-spinner` de l'album).
  pub epoch: Instant,
  /// Attente de la lecture : anti-rebond de 500 ms avant le spinner du bouton lecture (`app.js:14030-14065`).
  pub play_wait: crate::visuals::spinner::PlayWait,
  /// Boucle partagée des `.m3-btn-shape-spinner` (`startButtonShapeSpinners`, `app.js:9944-9980`) : loader de recherche, spinner de lecture.
  pub shape_loop: crate::visuals::spinner::ShapeSpinnerLoop,
  /// Animations de la barre de lecture : bouton lecture/pause, phase de l'onde, suivi de la progression (`app.js:3377-3520`, `14030`).
  pub playbar_motion: crate::visuals::playbar::BarMotion,
  /// Fond de la fenêtre et du bouton lecture qui fondent quand le thème change (`13-theme-titre`, `styles.css:112-122`, `3059`).
  pub theme_fade: crate::motion::ThemeFade,
  /// Graine du tirage des formes des spinners (`Math.random` de `nextSpinnerShape`) : fixée par le banc de rendu.
  pub spinner_seed: u64,
  /// Splash de démarrage : `None` hors du démarrage réel (`Program::boot` le pose), donc absent du banc et des tests.
  pub splash: Option<crate::splash::Splash>,
  /// Transition de vue (`.content-view.active`) : posée par la navigation, active seulement dans l'application lancée.
  pub view_motion: crate::motion::ViewMotion,
  /// Entrées `m3FadeUp` des listes, de la carte d'artiste et des choix de playlist (`styles.css:1016`).
  pub entry_motion: crate::motion::EntryMotion,
  /// Horloge des spinners circulaires des cartes réservées de l'accueil (`m3Rotate`, `m3Dash`), recréés par `renderMadeForYou` (`app.js:25391`).
  pub home_spinner: crate::motion::SpinnerClock,
  /// Horloge du spinner des tendances, inséré par `renderTrending` (`app.js:25172`) quand le chargement commence.
  pub trending_spinner: crate::motion::SpinnerClock,
  /// Barres d'égaliseur de la ligne en cours (`spBarEq`, `styles.css:1711`).
  pub bars_motion: crate::motion::BarsMotion,
  /// Badge « vérifié », chargeur et entrée des onglets de la page artiste (`styles.css:1899`, `2083`, `app.js:11846`).
  pub artist_motion: crate::motion::ArtistMotion,
  /// Dernière empreinte de la file vue sur l'écran Queue : évite de la hacher deux fois par message (`None` hors de cet écran).
  pub(crate) queue_seen: Option<u64>,
  /// Décalage du fuseau, lu au lancement : le jour des mixes quotidiens est local.
  pub utc_offset_minutes: i64,
  /// Vérification de mise à jour (« à propos »).
  pub update: updates::UpdateState,
  pub install: update_install::InstallState,
  /// Plugins Rhai : état vu par l'application.
  pub plugins: crate::plugins::PluginsState,
  /// Écran d'import du premier lancement.
  pub import: ImportState,
  /// Jam en cours : `None` hors jam, aucune tâche ni abonnement.
  pub jam: Option<Box<crate::jam::state::JamState>>,
  /// Reprise de la lecture au lancement : ce qui est enregistré, relance en attente.
  pub resume: crate::resume::ResumeTracker,
  unique: u64,
}

impl App {
  /// Effet de niveau de la fenêtre principale pour le réglage courant et le mode compact.
  pub(crate) fn main_level_effect(&self) -> Effect {
    Effect::MainLevel(main_on_top(
      self.settings.main_always_on_top,
      self.window.mini.is_some(),
    ))
  }

  #[must_use]
  pub fn new(window_state: PersistedWindowState, settings: NativeSettings, i18n: I18n) -> Self {
    let now = Instant::now();
    let theme = ActiveTheme::from_accent(&settings);
    let (body, play) = theme_fade_targets(&theme);
    crate::motion::apply_settings(settings.reduce_motion, settings.perf_mode);
    Self {
      window: WindowTracker::new(window_state),
      theme,
      screen: settings.screen,
      back_stack: Vec::new(),
      playback: Playback::default(),
      last_core_event: None,
      menu: None,
      menu_leaving: None,
      overlay_motion: crate::motion::OverlayMotion::default(),
      dialog: None,
      dialog_action: None,
      toasts: Toasts::default(),
      dev: None,
      settings,
      last_theme_error: None,
      i18n: Arc::new(i18n),
      data: UserData::default(),
      data_generation: 0,
      player: PlayerState::new(riff_core::util::now_millis()),
      normalize: NormalizeState::default(),
      ui: Ui::default(),
      images: ImageCache::default(),
      lyrics: LyricsState::default(),
      visuals: VisualState::default(),
      now,
      epoch: now,
      play_wait: crate::visuals::spinner::PlayWait::default(),
      shape_loop: crate::visuals::spinner::ShapeSpinnerLoop::default(),
      playbar_motion: crate::visuals::playbar::BarMotion::default(),
      theme_fade: crate::motion::ThemeFade::at(body, play),
      spinner_seed: riff_core::util::now_millis(),
      splash: None,
      view_motion: crate::motion::ViewMotion::default(),
      entry_motion: crate::motion::EntryMotion::default(),
      home_spinner: crate::motion::SpinnerClock::default(),
      trending_spinner: crate::motion::SpinnerClock::default(),
      bars_motion: crate::motion::BarsMotion::default(),
      artist_motion: crate::motion::ArtistMotion::default(),
      queue_seen: None,
      utc_offset_minutes: 0,
      update: updates::UpdateState::default(),
      install: update_install::InstallState::default(),
      plugins: crate::plugins::PluginsState::default(),
      import: ImportState::default(),
      jam: None,
      resume: crate::resume::ResumeTracker::default(),
      unique: 0,
    }
  }

  /// Donne les données enregistrées à l'application.
  #[must_use]
  pub fn with_data(mut self, data: UserData) -> Self {
    self.player.shuffle = data.shuffle;
    self.player.repeat = data.repeat;
    self.player.crossfade = data.crossfade;
    self.data = data;
    self.refresh_orders();
    self
  }

  pub(crate) const fn next_unique(&mut self) -> u64 {
    self.unique += 1;
    self.unique
  }

  /// Active l'écran de développement et l'ouvre.
  #[must_use]
  pub fn with_dev(mut self) -> Self {
    self.dev = Some(DevState::default());
    self.screen = Screen::Dev;
    self
  }

  /// Les contrôles survolés qui ont quitté la vue n'enverront aucune sortie (`motion::take_gone`) : leur survol part avant tout
  /// nouveau message.
  pub fn forget_gone_controls(&mut self) {
    for key in crate::motion::take_gone() {
      self.ui.controls.forget(key);
    }
  }

  /// Applique un message. Aucune E/S ici. Sans choix de l'écran d'import, les écritures sont retenues.
  pub fn update(&mut self, message: Message) -> Vec<Effect> {
    self.forget_gone_controls();
    let watch_queue =
      self.entry_motion.enabled && self.screen == Screen::Queue && Self::may_change_queue(&message);
    let queue_before =
      watch_queue.then(|| self.queue_seen.unwrap_or_else(|| self.queue_signature()));
    let resume_watch = Self::may_change_queue(&message).then(|| crate::resume::Watch::of(&message));
    let effects = self.dispatch(message);
    let mut effects = self.jam_after_update(effects);
    if let Some(watch) = resume_watch {
      let saves = self.resume_after(watch, &effects);
      effects.extend(saves);
    }
    // `renderQueueView` recrée les lignes à chaque changement de la file ou du titre en cours (`app.js:4564`, `12503`, `14689`).
    if let Some(before) = queue_before {
      let after = self.queue_signature();
      self.queue_seen = (self.screen == Screen::Queue).then_some(after);
      if after != before {
        self.entered_rows(Screen::Queue);
      }
    }
    self.track_play_wait();
    self.track_shape_spinners();
    self.track_home_spinner();
    self.track_bars();
    self.track_playbar();
    self.track_theme();
    self.track_mesh();
    self.suspend_writes(effects)
  }

  fn dispatch(&mut self, message: Message) -> Vec<Effect> {
    match message {
      Message::WindowOpened { position, size } => {
        if let Some(position) = position {
          self.window.moved(position);
        }
        self.window.resized(size);
        let mut effects = vec![Effect::QueryScale];
        effects.extend(self.startup_tools());
        effects.extend(self.on_show(self.screen));
        if self.dev.is_some() {
          effects.extend([Effect::ListDevices, Effect::ListThemes]);
        }
        // Mixage, yt-dlp, son, proxy, accueil et langue : après le choix de l'écran d'import s'il y en a un.
        effects.extend(self.start_or_hold());
        effects
      }
      Message::WindowMoved(position) => {
        self.window.moved(position);
        vec![Effect::QueryMaximized]
      }
      Message::WindowResized(size) => {
        self.window.resized(size);
        self.lyrics.reported_viewport = None;
        self.relayout();
        vec![Effect::QueryMaximized]
      }
      Message::WindowRescaled(scale) => {
        self.window.scale = Some(scale);
        Vec::new()
      }
      Message::ScaleKnown(scale) => match self.window.rescale_for_restore(scale) {
        Some((position, size)) => vec![Effect::ApplyBounds(position, size)],
        None => vec![Effect::QueryMaximized],
      },
      Message::MaximizedState(is_maximized) => {
        self.window.maximized_state(is_maximized);
        Vec::new()
      }
      Message::CloseRequested | Message::Chrome(chrome::Action::Close) => {
        vec![Effect::QueryCloseState]
      }
      Message::CloseStateKnown {
        is_maximized,
        is_full_screen,
      } => {
        let mut effects = Vec::new();
        // Au mieux : le processus peut finir avant l'envoi, les invités sortent alors après trois essais.
        if self.jam.is_some() {
          effects.push(Effect::JamSend(crate::jam::JamOut::Stop));
        }
        effects.push(Effect::SaveWindowAndExit(
          self.window.closing_state(is_maximized, is_full_screen),
          Some(Box::new(self.data.clone())),
        ));
        effects
      }
      Message::Jam(message) => self.update_jam(message),
      Message::Settings(message) => self.update_settings(message),
      Message::Plugin(message) => self.update_plugins(message),
      Message::Editor(message) => self.update_editor(message),
      Message::ThemeEditor(message) => self.update_theme_editor(message),
      Message::AudioPrefs(message) => self.update_audio(message),
      Message::Manage(message) => self.update_manage(message),
      Message::Import(message) => self.update_import(message),
      Message::Download(message) => self.update_download(message),
      Message::Chrome(action) => vec![Effect::Chrome(action)],
      Message::ToggleMainAlwaysOnTop => {
        self.settings.main_always_on_top = !self.settings.main_always_on_top;
        vec![
          self.main_level_effect(),
          Effect::SaveSettings(Box::new(self.settings.clone())),
        ]
      }
      Message::SetThemeMode(mode) => {
        self.settings.theme_mode = mode;
        if self.settings.native_theme.is_none() {
          self.theme = ActiveTheme::from_accent(&self.settings);
        }
        vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
      }
      Message::CoverBytes(bytes) if self.follows_cover() => vec![Effect::ExtractCover(bytes)],
      Message::CoverBytes(_) => Vec::new(),
      Message::CoverColor(source) => {
        if self.follows_cover() {
          self.theme = ActiveTheme::from_cover(source, &self.settings);
        }
        Vec::new()
      }
      Message::SelectThemeFile(id) => vec![Effect::LoadThemeFile(id)],
      Message::ThemeFileLoaded(id, Ok(loaded)) => {
        self.last_theme_error = None;
        self.settings.native_theme = Some(id.clone());
        self.theme = ActiveTheme::from_file(id, *loaded);
        vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
      }
      Message::ThemeFileLoaded(_, Err(reason)) => {
        self.last_theme_error = Some(reason);
        Vec::new()
      }
      Message::ClearThemeFile => {
        self.settings.native_theme = None;
        self.theme = ActiveTheme::from_accent(&self.settings);
        vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
      }
      Message::SetLanguage(lang) => {
        self.settings.ui_lang.clone_from(&lang);
        vec![
          Effect::LoadLanguage(lang),
          Effect::SaveSettings(Box::new(self.settings.clone())),
        ]
      }
      Message::LanguageLoaded(i18n) => {
        if i18n.lang() == self.settings.ui_lang {
          self.i18n = i18n;
        }
        Vec::new()
      }
      Message::Navigate(screen) => {
        if screen == self.screen {
          return Vec::new();
        }
        if self.back_stack.len() == MAX_BACK_STACK {
          self.back_stack.remove(0);
        }
        self.back_stack.push(self.screen);
        let mut effects = self.show(screen);
        effects.extend(self.on_show(screen));
        if screen == Screen::Settings {
          effects.extend(self.settings_opened());
        }
        effects
      }
      Message::Back => match self.back_stack.pop() {
        Some(screen) => {
          let mut effects = self.show(screen);
          effects.extend(self.on_show(screen));
          effects
        }
        None => Vec::new(),
      },
      Message::Audio(event) => self.audio_event(event),
      Message::Core(event @ (CoreEvent::SpotifyLogin { .. } | CoreEvent::GoogleLogin { .. })) => {
        self.last_core_event = Some(event.clone());
        self.login_event(&event)
      }
      Message::Core(event) => {
        self.last_core_event = Some(event);
        Vec::new()
      }
      Message::LoadFile(path) => {
        self.playback = Playback {
          is_playing: true,
          device: self.playback.device.take(),
          ..Playback::default()
        };
        vec![Effect::AudioLoad(path)]
      }
      Message::OpenMenu(menu) => {
        self.menu = Some(*menu);
        self.menu_leaving = None;
        if self.overlay_motion.enabled {
          // Hors animation, `now` date de la dernière image : le fondu part de l'instant réel.
          self.now = Instant::now();
          self.overlay_motion.open_menu(self.now);
        }
        vec![Effect::FocusOverlay]
      }
      Message::CloseMenu => {
        self.close_menu();
        self.restore_focus_effects()
      }
      Message::MenuChoose(message) => {
        self.close_menu();
        let mut effects = self.restore_focus_effects();
        effects.extend(self.update(*message));
        effects
      }
      Message::OpenDialog(dialog, action) => {
        self.dialog = Some(dialog);
        self.dialog_action = action;
        vec![Effect::FocusOverlay]
      }
      Message::CloseDialog => {
        self.dialog = None;
        self.dialog_action = None;
        self.restore_focus_effects()
      }
      Message::ConfirmDialog => {
        self.dialog = None;
        let mut effects = self.restore_focus_effects();
        if let Some(action) = self.dialog_action.take() {
          effects.extend(self.update(*action));
        }
        effects
      }
      Message::ShowToast(message, kind, duration) => {
        let translated = self.i18n.t(&message);
        // Hors animation, `now` date de la dernière image : l'apparition part de l'instant réel.
        self.now = Instant::now();
        let effects = toast_timer(self.toasts.push(&translated, kind, duration));
        self.toasts.stamp(self.now);
        effects
      }
      // Fin de durée ou clic : la sortie `.leaving` part, le retrait suit à une image d'animation (`toast_frame`).
      Message::ToastExpired(id) => {
        self.now = Instant::now();
        self.toasts.begin_leave(id, self.now);
        Vec::new()
      }
      Message::ToastTimeout(id, generation, at) => {
        self.toasts.timeout(id, generation, at);
        Vec::new()
      }
      Message::ToastHover(id, true, at) => {
        self.toasts.hover_enter(id, at);
        Vec::new()
      }
      Message::ToastHover(id, false, at) => toast_timer(self.toasts.hover_leave(id, at)),
      Message::Escape => self.escape(),
      Message::FocusMove { back } => {
        let in_overlay = self.menu.is_some() || self.dialog.is_some();
        vec![if back {
          Effect::FocusPrevious { in_overlay }
        } else {
          Effect::FocusNext { in_overlay }
        }]
      }
      // Écran d'import sans choix : ni lecture au clavier, ni recherche (donc aucune requête réseau).
      Message::Shortcut(_)
      | Message::Ui(UiMessage::FocusSearch)
      | Message::Player(
        PlayerMessage::TogglePlay
        | PlayerMessage::Next
        | PlayerMessage::Prev
        | PlayerMessage::SeekBy(_)
        | PlayerMessage::VolumeBy(_),
      ) if self.import_waiting() => Vec::new(),
      Message::Dev(message) => dev::update(self, message),
      Message::Ui(message) => self.update_ui(message),
      Message::Player(message) => self.update_player(message),
      Message::ImageLoaded(key, result) => {
        match result {
          Ok(decoded) => self.images.insert(key, Arc::unwrap_or_clone(decoded)),
          Err(reason) => {
            tracing::debug!(%reason, "pochette");
            self.images.failed(key);
          }
        }
        Vec::new()
      }
      Message::Lyrics(message) => self.update_lyrics(message),
      Message::BlurReady(key, decoded) => {
        self.blur_ready(&key, decoded.map(Arc::unwrap_or_clone));
        Vec::new()
      }
      Message::ForWindow(_, inner) => self.update(*inner),
      Message::Shortcut(_) if self.dialog.is_some() || self.ui.modal.is_some() => Vec::new(),
      Message::Shortcut(message) => self.update(*message),
      Message::ScenePause => self.update(Message::Player(PlayerMessage::TogglePlay)),
      Message::UpdateCheck
        if self.settings.update_check && self.update != updates::UpdateState::Checking =>
      {
        self.update = updates::UpdateState::Checking;
        vec![Effect::CheckUpdate]
      }
      Message::UpdateCheck => Vec::new(),
      Message::UpdateChecked(result) => {
        self.update = match result {
          Ok(outcome) => updates::UpdateState::Done(outcome),
          Err(error) => updates::UpdateState::Failed(error),
        };
        Vec::new()
      }
      Message::UpdateInstall
        if self.settings.update_check
          && update_install::can_install()
          && matches!(
            self.install,
            update_install::InstallState::Idle | update_install::InstallState::Failed(_)
          ) =>
      {
        match &self.update {
          updates::UpdateState::Done(updates::Outcome::Available(release)) => {
            self.install = update_install::InstallState::Downloading(0);
            vec![Effect::InstallUpdate(release.clone())]
          }
          _ => Vec::new(),
        }
      }
      Message::UpdateInstall => Vec::new(),
      Message::UpdateInstallEvent(event) => {
        self.install = match event {
          update_install::InstallEvent::Progress(percent) => {
            update_install::InstallState::Downloading(percent)
          }
          update_install::InstallEvent::Verifying => update_install::InstallState::Verifying,
          update_install::InstallEvent::Launched => update_install::InstallState::Launched,
          update_install::InstallEvent::Failed(error) => {
            update_install::InstallState::Failed(error.to_string())
          }
        };
        Vec::new()
      }
      Message::LauncherInstall => vec![Effect::InstallLauncher],
      Message::ToggleMiniMode => {
        if self.window.enter_mini() {
          let mut effects = vec![Effect::EnterMiniWindow];
          // `toggleMiniWindow` rouvre les paroles si elles étaient affichées (`app.js:18265-18268`).
          if self.window.mini_lyrics {
            let (min, size) = self.window.mini_lyrics_bounds(MINI_SIZE);
            effects.push(Effect::ResizeMini(min, size));
          }
          effects
        } else {
          self
            .window
            .leave_mini()
            .map_or_else(Vec::new, |(position, size, maximized)| {
              vec![Effect::LeaveMiniWindow(position, size, maximized)]
            })
        }
      }
      Message::ToggleMiniLyrics => self
        .window
        .toggle_mini_lyrics()
        .map_or_else(Vec::new, |(min, size)| vec![Effect::ResizeMini(min, size)]),
      Message::OpenReleasePage => match &self.update {
        updates::UpdateState::Done(updates::Outcome::Available(release)) => {
          vec![Effect::OpenUrl(release.url.clone())]
        }
        _ => Vec::new(),
      },
      Message::ProbeFrame
      | Message::BenchFrame(_)
      | Message::AnimationFrame(_)
      | Message::System(_)
      | Message::MainHandle(_)
      | Message::SystemCover(..)
      | Message::NoOp => Vec::new(),
    }
  }

  /// `.bg-mesh-layer` est affiché : maillage activé, luminosité non nulle (`app.js:1673`) et `body.cover-bg-on` absent
  /// (`styles.css:8390`).
  #[must_use]
  pub fn mesh_shown(&self) -> bool {
    let visual = &self.settings.visual;
    visual.mesh_gradient && visual.mesh_brightness > 0 && !visual.cover_bg
  }

  /// Clé du calque affiché, `None` s'il est masqué.
  fn mesh_key_now(&self) -> Option<crate::visuals::mesh::MeshKey> {
    self
      .mesh_shown()
      .then(|| crate::screens::backdrop::mesh_key(self))
  }

  /// Date chaque changement de clé (`meshKey`, `app.js:1695`) et chaque apparition ou disparition du calque : l'animation
  /// `meshFloat` repart de zéro, car l'autre calque sort de `display: none`.
  pub fn sync_mesh(&mut self, now: Instant) {
    let key = self.mesh_key_now();
    if key != self.visuals.mesh_key {
      self.visuals.mesh_key = key;
      self.visuals.mesh_started = key.map(|_| now);
    }
  }

  fn track_mesh(&mut self) {
    if self.mesh_key_now() != self.visuals.mesh_key {
      self.now = Instant::now();
      self.sync_mesh(self.now);
    }
  }

  /// Date chaque changement de thème : Riff pose les variables d'un coup et seuls le fond de `body` et celui du bouton lecture
  /// fondent (voir `ThemeFade`). Le départ est l'instant du message, sans attente simulée.
  fn track_theme(&mut self) {
    let (body, play) = theme_fade_targets(&self.theme);
    if self.theme_fade.differs(body, play) {
      self.now = Instant::now();
      self.theme_fade.set(body, play, self.now);
    }
  }

  /// Date le début et la fin de l'attente de la lecture, pour l'anti-rebond du spinner.
  fn track_play_wait(&mut self) {
    let waiting = self.player.loading || self.playback.buffering;
    if waiting != self.play_wait.since.is_some() {
      self.now = Instant::now();
      self.play_wait.track(waiting, self.now);
    }
  }

  /// Au moins un `.m3-btn-shape-spinner:not(.hidden)` existe à `now` (`app.js:9953`) : le loader de la recherche, qui reste en place sur
  /// l'erreur et quelle que soit la vue, ou `#playbar-spinner` une fois l'anti-rebond de 500 ms passé. Les équivalents des boutons
  /// « Checking... » et « Signing in... », du `#tvk-play-spinner` et du spinner de téléchargement n'existent pas dans riff-rs.
  #[must_use]
  pub fn shape_spinners_shown(&self, now: Instant) -> bool {
    self.ui.search.loading || self.ui.search.error.is_some() || self.play_wait.spinner_shown(now)
  }

  /// Date l'apparition et la disparition de la boucle des spinners à formes, à l'horloge d'`App`.
  fn track_shape_spinners(&mut self) {
    if self.shape_spinners_shown(self.now) != self.shape_loop.shown() {
      self.now = Instant::now();
      self.sync_shape_loop(self.now);
    }
  }

  /// Aligne la boucle des spinners à formes sur ce qui est affiché à `at`.
  pub fn sync_shape_loop(&mut self, at: Instant) {
    if self.shape_spinners_shown(at) {
      self.shape_loop.show(at);
    } else {
      self.shape_loop.hide(at);
    }
  }

  /// Pose commune à tous les spinners à formes à l'horloge d'`App`.
  #[must_use]
  pub fn shape_pose(&self) -> crate::visuals::spinner::Pose {
    self.shape_loop.pose(
      self.spinner_seed,
      crate::visuals::spinner::PROFILES.len(),
      self.epoch,
      self.now,
    )
  }

  /// Date l'apparition, la disparition et la recréation des spinners circulaires de l'accueil. Les deux horloges lisent le même
  /// instant : des spinners nés du même rendu (`renderSpotifyHome`) partagent leur départ.
  fn track_home_spinner(&mut self) {
    if !self.home_spinner.enabled {
      return;
    }
    let made = crate::screens::home::made_spinner_shown(self);
    let trending = crate::screens::home::trending_spinner_shown(self);
    let changed =
      |clock: &crate::motion::SpinnerClock, shown| shown != clock.running() || clock.recreated;
    if changed(&self.home_spinner, made) || changed(&self.trending_spinner, trending) {
      self.now = Instant::now();
      self.home_spinner.track(made, self.now);
      self.trending_spinner.track(trending, self.now);
    }
  }

  /// `renderMadeForYou` vient de recréer les cartes réservées (`row.innerHTML = ''`, `app.js:25423`) : plan des mix, mix arrivé,
  /// fin de la génération. Les spinners de ces cartes repartent de 0 ; celui des tendances n'est pas touché.
  pub(crate) fn recreate_made_spinners(&mut self) {
    self.home_spinner.recreate();
  }

  /// Date le départ des barres d'égaliseur (titre lancé) et leurs pauses : `body.is-playing` suit la lecture, `body.app-paused` la fenêtre masquée.
  fn track_bars(&mut self) {
    if !self.bars_motion.enabled {
      return;
    }
    let running = self.playback.is_playing && !self.is_minimized();
    let at = Instant::now();
    if self.bars_motion.track(self.player.token, running, at) {
      self.now = at;
    }
  }

  /// Liste de titres affichée par l'écran, la seule qui porte des `.track-row`.
  fn track_list_of(&self) -> Option<ListId> {
    match self.screen {
      Screen::Album => Some(ListId::Album),
      Screen::Artist => Some(ListId::Artist),
      Screen::History => Some(ListId::History),
      Screen::Library => Some(ListId::Local),
      Screen::Liked => Some(ListId::Favorites),
      Screen::PlaylistDetail => Some(ListId::Playlist),
      Screen::Queue => Some(ListId::Queue),
      Screen::Search => Some(ListId::Search),
      _ => None,
    }
  }

  /// Une ligne construite de l'écran porte `.playing` : ses barres d'égaliseur sont à l'écran.
  fn playing_row_shown(&self) -> bool {
    let (Some(current), Some(list)) = (self.player.current.as_ref(), self.track_list_of()) else {
      return false;
    };
    let len = self.list_len(list);
    let mut rows = if list == ListId::Artist {
      0..len
    } else {
      self.ui.list(list).visible(len)
    };
    rows.any(|index| {
      self
        .list_track(list, index)
        .is_some_and(|track| current.is_same(track, &self.data.platform))
    })
  }

  /// Hauteurs des quatre barres de la ligne en cours à `App::now` (fractions de `.sp-playing-bars`).
  #[must_use]
  pub fn bar_heights(&self) -> [f32; 4] {
    self
      .bars_motion
      .heights(self.now, self.playback.is_playing && !self.is_minimized())
  }

  /// Les barres d'égaliseur bougent et sont à l'écran : l'abonnement aux images doit tourner.
  #[must_use]
  pub fn bars_animating(&self) -> bool {
    !crate::motion::Gate::BOTH.instant()
      && self.bars_motion.running(self.now, self.playback.is_playing)
      && self.playing_row_shown()
  }

  /// Lignes de titres recréées : leurs barres repartent de zéro (`highlightPlayingRow` après `renderTracks`, `app.js:11485`).
  fn restart_bars(&mut self) {
    if self.bars_motion.enabled {
      let running = self.playback.is_playing && !self.is_minimized();
      self.bars_motion.restart(self.now, running);
    }
  }

  /// Notification affichée, en haut au centre de la fenêtre.
  pub(crate) fn toast_layer(&self) -> Option<Element<'_, Message>> {
    let toast = self.toasts.active.as_ref()?;
    let scheme = &self.theme.scheme;
    let rgb = |color: Rgb| Color::from_rgb8(color.red, color.green, color.blue);
    let colors = toast::Colors {
      background: rgb(scheme.surface_container_highest),
      text: rgb(scheme.on_surface),
      primary: rgb(scheme.primary),
      error: rgb(scheme.error),
    };
    Some(
      container(toast::view(
        toast,
        self.now,
        colors,
        Message::ToastExpired(toast.id),
        Message::ToastHover,
      ))
      .width(Fill)
      .height(Fill)
      .align_x(Horizontal::Center)
      .align_y(Vertical::Top)
      .into(),
    )
  }

  /// Image d'animation : retire la notification dont la sortie est finie et lance la minuterie de la suivante.
  pub(crate) fn toast_frame(&mut self, at: Instant) -> Vec<Effect> {
    toast_timer(self.toasts.finish_leaving(at))
  }

  /// Échap ferme la couche du dessus, puis la notification, puis retire le focus.
  fn escape(&mut self) -> Vec<Effect> {
    if self.close_menu() {
      return self.restore_focus_effects();
    }
    if self.dialog.take().is_some() {
      self.dialog_action = None;
      return self.restore_focus_effects();
    }
    if let Some(effects) = self.dismiss_modal() {
      return effects;
    }
    if let Some(effects) = self.escape_lyrics() {
      return effects;
    }
    if let Some(id) = self.toasts.active.as_ref().map(|toast| toast.id) {
      let effects = toast_timer(self.toasts.dismiss(id));
      self.now = Instant::now();
      self.toasts.stamp(self.now);
      return effects;
    }
    if self.clear_selection() {
      return Vec::new();
    }
    if let Some(effects) = self.escape_search() {
      return effects;
    }
    if matches!(
      self.screen,
      Screen::Album | Screen::Artist | Screen::Settings
    ) {
      return self.update(Message::Back);
    }
    vec![Effect::Unfocus]
  }

  /// Ferme le menu : il s'efface sur place (180 ms) dans l'application lancée. Faux s'il n'y en avait pas.
  fn close_menu(&mut self) -> bool {
    let Some(menu) = self.menu.take() else {
      return false;
    };
    if self.overlay_motion.enabled {
      self.now = Instant::now();
      self.overlay_motion.close_menu(self.now);
      self.menu_leaving = Some(menu);
    }
    true
  }

  /// Rend le focus au widget qui l'avait avant la superposition.
  pub(crate) fn restore_focus_effects(&mut self) -> Vec<Effect> {
    vec![Effect::RestoreFocus(self.ui.saved_focus.take())]
  }

  /// Lignes recréées par `renderTracks` ou `createTrackRow` : `m3FadeUp` repart de l'instant réel (voir `show`).
  /// Sans effet si `screen` n'est pas l'écran affiché : une réponse tardive ne rejoue pas la liste d'un autre écran.
  pub(crate) fn entered_rows(&mut self, screen: Screen) {
    if self.entry_motion.enabled && self.screen == screen {
      self.now = Instant::now();
      self.entry_motion.plain = Some(self.entry_plain(screen));
      self.entry_motion.start_rows(self.now);
      self.restart_bars();
    }
  }

  /// Lignes ajoutées en fin de liste par « charger plus » (`app.js:10420`) : seules les nouvelles s'animent, à partir du rang `first`.
  pub(crate) fn entered_more_rows(&mut self, screen: Screen, first: usize) {
    if self.entry_motion.enabled && self.screen == screen {
      self.now = Instant::now();
      self.entry_motion.plain = Some(self.entry_plain(screen));
      self.entry_motion.start_more(self.now, first);
    }
  }

  /// Faux pour les messages qui ne touchent jamais la file (images d'animation, souris, défilement, fenêtre) : on ne hache pas la file pour eux.
  fn may_change_queue(message: &Message) -> bool {
    !matches!(
      message,
      Message::AnimationFrame(_)
        | Message::ProbeFrame
        | Message::BenchFrame(_)
        | Message::WindowMoved(_)
        | Message::WindowResized(_)
        | Message::BlurReady(..)
        | Message::ImageLoaded(..)
        | Message::ToastExpired(_)
        | Message::ToastTimeout(..)
        | Message::ToastHover(..)
        | Message::Core(_)
        | Message::Ui(
          UiMessage::PanelMoved
            | UiMessage::SidebarDragMoved(_)
            | UiMessage::CardHover(..)
            | UiMessage::HomeHover(..)
            | UiMessage::ControlHover(..)
            | UiMessage::SearchScrolled(..)
            | UiMessage::HomeRowScrolled(..)
            | UiMessage::RowEntered(..)
            | UiMessage::RowLeft(..)
            | UiMessage::HistoryHover(_)
            | UiMessage::ListScrolled(..)
            | UiMessage::QueueScrolled(..)
        )
    )
  }

  /// Empreinte de la file et du titre en cours : elle change quand `renderQueueView` aurait recréé les lignes.
  fn queue_signature(&self) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    self.player.index.hash(&mut hasher);
    for track in &self.player.queue {
      track.id.hash(&mut hasher);
      track.title.hash(&mut hasher);
    }
    hasher.finish()
  }

  /// Voile des entrées : le fond uni de la page seulement, décidé d'après le halo de l'écran d'arrivée (`hero_url_on`) et non
  /// d'après `visuals.hero_bg`, qui garde le halo de l'écran quitté jusqu'au prochain `sync_layers`.
  pub(crate) fn entry_plain(&self, screen: Screen) -> bool {
    !self.settings.visual.has_background() && self.hero_url_on(screen).is_none()
  }

  /// Carte d'artiste des résultats réécrite par `fetchArtistCardForSearch`.
  pub(crate) fn entered_artist(&mut self) {
    if self.entry_motion.enabled && self.screen == Screen::Search {
      self.now = Instant::now();
      self.entry_motion.plain = Some(self.entry_plain(Screen::Search));
      self.entry_motion.start_artist(self.now);
    }
  }

  /// `showSettingsCategory` (`app.js:17555`) : la section de la catégorie perd `cat-hidden` et rejoue `settingsIn`, sauf si elle était déjà
  /// affichée (la classe ne change pas, l'animation ne repart pas : ouverture de la fenêtre sur la dernière catégorie, `04-reglages`).
  pub(crate) fn show_settings_category(&mut self, key: &str) {
    if self.settings.settings_cat == key {
      return;
    }
    if self.entry_motion.enabled {
      self.now = Instant::now();
      self.entry_motion.start_section(self.now);
    }
    key.clone_into(&mut self.settings.settings_cat);
  }

  /// Boîte d'erreur de la recherche recréée par `stopM3SearchLoaderWithError` (`app.js:9982`).
  pub(crate) fn entered_error(&mut self) {
    if self.entry_motion.enabled && self.screen == Screen::Search {
      self.now = Instant::now();
      self.entry_motion.plain = Some(self.entry_plain(Screen::Search));
      self.entry_motion.start_error(self.now);
    }
  }

  /// Choix de playlist recréés à l'ouverture de la fenêtre (`app.js:13543`).
  pub(crate) fn entered_picks(&mut self) {
    if self.entry_motion.enabled {
      self.now = Instant::now();
      self.entry_motion.start_picks(self.now);
    }
  }

  /// Clic sur un onglet de la bibliothèque latérale : `renderSidebarLibraryFeed(true)` recrée les rangées sans `no-sb-anim` (`app.js:27577`).
  pub(crate) fn entered_sidebar(&mut self) {
    if self.entry_motion.enabled {
      self.now = Instant::now();
      self.entry_motion.start_sidebar(self.now);
    }
  }

  fn show(&mut self, screen: Screen) -> Vec<Effect> {
    if self.view_motion.enabled || self.overlay_motion.enabled || self.artist_motion.enabled {
      // Hors animation, `now` date de la dernière image : la vue part de l'instant réel.
      self.now = Instant::now();
    }
    self.view_motion.navigate(self.screen, screen, self.now);
    self.queue_seen = None;
    self.ui.playlist_sort_open = false;
    // `renderFavoritesView`, `renderHistoryView`, `renderLocalView`, `renderQueueView` et `renderTracks` des playlists recréent les lignes à chaque ouverture ;
    // album et artiste les créent à l'arrivée des données (`entered_rows`).
    if self.entry_motion.enabled
      && matches!(
        screen,
        Screen::Liked | Screen::Library | Screen::History | Screen::Queue | Screen::PlaylistDetail
      )
    {
      self.entry_motion.plain = Some(self.entry_plain(screen));
      self.entry_motion.start_rows(self.now);
      self.restart_bars();
    }
    self.overlay_motion.navigate(self.screen, screen, self.now);
    if screen == Screen::Artist && self.screen != Screen::Artist {
      // `#view-artist` sort de `display: none` : le panneau affiché et le badge visible rejouent leur animation CSS.
      if self.entry_motion.enabled {
        self.entry_motion.plain = Some(self.entry_plain(screen));
      }
      self.artist_motion.start_pane(self.now);
      if self.ui.artist.verified && !self.ui.artist.loading {
        self.artist_motion.start_badge(self.now);
      }
    }
    self.screen = screen;
    self.ui.search.history_open = false;
    if !screen.is_remembered() {
      return Vec::new();
    }
    self.settings.screen = screen;
    vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
  }

  pub(crate) fn follows_cover(&self) -> bool {
    self.settings.theme_mode == ThemeMode::Auto && self.settings.native_theme.is_none()
  }

  /// Images à afficher sur l'écran courant, les plus importantes d'abord.
  #[must_use]
  pub fn wanted_images(&self) -> Vec<ImageKey> {
    // Données de Riff en mémoire mais pas encore consenties : aucune requête d'image avant le choix.
    if self.import_waiting() {
      return Vec::new();
    }
    let mut wanted = Vec::new();
    let mut add = |url: Option<String>, size: u32| {
      if let Some(url) = url {
        wanted.push(ImageKey { url, size });
      }
    };
    add(
      self
        .player
        .current
        .as_ref()
        .and_then(|track| self.cover_of(track)),
      images::SIZE_CARD,
    );
    add(self.banner_url(), images::SIZE_CARD);
    if self.lyrics.tvk_open && self.settings.visual.tvk_cover_mode {
      add(
        self
          .player
          .current
          .as_ref()
          .and_then(|track| self.cover_of(track)),
        images::SIZE_HERO,
      );
    }
    if let Some(crate::ui::Modal::TrackArt(track)) = &self.ui.modal {
      add(self.cover_of(track), images::SIZE_ROW);
      add(self.banner_preview(track), images::SIZE_ROW);
    }
    add(self.background_url().map(str::to_string), images::SIZE_HERO);
    let list = match self.screen {
      Screen::Liked => Some(ListId::Favorites),
      Screen::History => Some(ListId::History),
      Screen::Queue => Some(ListId::Queue),
      Screen::Library => Some(ListId::Local),
      Screen::PlaylistDetail => Some(ListId::Playlist),
      Screen::Search => Some(ListId::Search),
      Screen::Album => Some(ListId::Album),
      Screen::Artist => Some(ListId::Artist),
      _ => None,
    };
    if let Some(list) = list {
      for index in self.ui.list(list).visible(self.list_len(list)) {
        add(
          self
            .list_track(list, index)
            .and_then(|track| self.cover_of(track)),
          images::SIZE_ROW,
        );
      }
    }
    add(self.ui.search.icon_cover.clone(), images::SIZE_ROW);
    match self.screen {
      Screen::Home => {
        let mut urls = Vec::new();
        home_screen::cover_urls(self, &mut urls);
        urls
          .into_iter()
          .for_each(|url| add(Some(url), images::SIZE_CARD));
      }
      Screen::Albums => self
        .data
        .saved_albums
        .iter()
        .for_each(|album| add(album.cover.clone(), images::SIZE_CARD)),
      Screen::Playlists => self
        .data
        .playlists
        .iter()
        .for_each(|playlist| add(common::playlist_cover_url(playlist), images::SIZE_CARD)),
      Screen::Search => {
        self
          .ui
          .search
          .albums
          .iter()
          .for_each(|album| add(album.cover.clone(), images::SIZE_CARD));
        add(
          self
            .ui
            .search
            .artist
            .as_ref()
            .and_then(|artist| artist.avatar.clone()),
          images::SIZE_CARD,
        );
      }
      Screen::Library => crate::screens::library::local_albums(&self.data.local_files)
        .into_iter()
        .for_each(|(_, _, cover, _)| add(cover, images::SIZE_CARD)),
      Screen::Album => {
        add(self.ui.album.album.cover.clone(), images::SIZE_HERO);
        self
          .ui
          .album
          .grid_covers
          .iter()
          .for_each(|url| add(Some(url.clone()), images::SIZE_CARD));
      }
      Screen::Artist => {
        add(self.ui.artist.banner.clone(), images::SIZE_HERO);
        self
          .ui
          .artist
          .albums
          .iter()
          .for_each(|album| add(album.cover.clone(), images::SIZE_CARD));
      }
      Screen::Liked => add(
        self.data.liked_custom_cover().map(str::to_string),
        images::SIZE_HERO,
      ),
      Screen::PlaylistDetail => {
        let playlist = self
          .ui
          .playlist_id
          .as_deref()
          .and_then(|id| self.data.playlist(id));
        add(
          playlist.and_then(common::playlist_cover_url),
          images::SIZE_HERO,
        );
      }
      _ => {}
    }
    let mut feed = Vec::new();
    screen_chrome::feed_cover_urls(self, &mut feed);
    screen_chrome::queue_panel_cover_urls(self, &mut feed);
    feed
      .into_iter()
      .for_each(|url| add(Some(url), images::SIZE_ROW));
    if self.ui.search.history_open {
      self
        .data
        .search_history_matching(&self.ui.search.input)
        .iter()
        .for_each(|entry| add(entry.thumb_url(), images::SIZE_ROW));
    }
    wanted
  }

  /// `checkCompactLayout` (`app.js:18246-18250`) : `body.mini` suit la largeur de la fenêtre (<= 520 px CSS), pas l'ordre
  /// de passage en mode compact. Tant que la taille est inconnue, le mode compact de la fenêtre décide.
  #[must_use]
  pub fn compact_layout(&self) -> bool {
    self.window.layout_size().map_or_else(
      || self.window.mini.is_some(),
      |size| size.width <= crate::theme::tokens::compact::LAYOUT_MAX_WIDTH,
    )
  }

  /// Vue de l'application. Les boutons lisent leur survol, leur appui et leurs ondes dans l'état de `ui.controls` à `self.now`.
  #[must_use]
  pub fn view(&self) -> Element<'_, Message> {
    crate::motion::with_view_scope(&self.ui.controls, self.now, || self.view_content())
  }

  fn view_content(&self) -> Element<'_, Message> {
    if self.compact_layout() {
      return compact::view(self);
    }
    // Écran d'import sans choix : seul cet écran existe, aucun contrôle de l'interface n'est atteignable.
    if let Some(pending) = &self.import.pending {
      let frame = chrome::frame(
        import_screen::view(self, pending),
        self.window.is_maximized,
        Message::Chrome,
      );
      return match self.splash_layer() {
        Some(splash) => Stack::with_children([frame, splash])
          .width(Fill)
          .height(Fill)
          .into(),
        None => frame,
      };
    }
    let screen = if self.screen == Screen::Dev {
      dev::view(self)
    } else {
      self.screen.view(self)
    };
    let side = if self.panel_shown() {
      right_panel::panel(self)
    } else {
      right_panel::hidden()
    };
    let body = window_frame::main_area(
      self,
      screen_chrome::sidebar(self),
      screen_chrome::sidebar_edge(self),
      screen,
      Some(side),
    );
    let mut page = column![body].width(Fill).height(Fill);
    if let Some(selection) = common::selection_bar(self) {
      page = page.push(selection);
    }
    page = page.push(crate::screens::playbar::player_bar(self));
    let has_background = self.settings.visual.has_background();
    let body = self.theme_fade.body.value(self.now);
    let content = container(page).style(move |theme: &Theme| {
      if has_background {
        transparent_surface_style(theme)
      } else {
        surface_style(theme).background(body)
      }
    });
    let mut layers = Stack::new().width(Fill).height(Fill);
    if has_background {
      layers = layers.push(
        container(space())
          .width(Fill)
          .height(Fill)
          .style(move |theme: &Theme| surface_style(theme).background(body)),
      );
      layers = layers.extend(backdrop::layers(self));
    }
    layers = layers.push(content);
    if self.main_ticker() {
      layers = layers.push(ticker());
    }
    if let Some(visualizer) = backdrop::bar_visualizer(self, screen_chrome::PLAYER_HEIGHT) {
      layers = layers.push(visualizer);
    }
    if self.tvk_shown() {
      layers = layers.push(lyrics_screen::tvk(self));
    }
    if let Some(editor) = lyrics_screen::editor(self) {
      layers = layers.push(editor);
    }
    if let Some(editor) = crate::screens::code_editor::view(self) {
      layers = layers.push(editor);
    }
    if let Some(sync) = lyrics_screen::sync(self) {
      layers = layers.push(sync);
    }
    if let Some(queue) = screen_chrome::queue_panel(self) {
      layers = layers.push(queue);
    }
    if let Some(jam) = screen_chrome::jam_panel(self) {
      layers = layers.push(jam);
    }
    if let Some(history) = crate::screens::search_history::view(self) {
      layers = layers.push(history);
    }
    if self.screen == Screen::Settings || self.overlay_motion.settings_leaving(self.now) {
      layers = layers.push(crate::screens::settings::modal(self));
    }
    if let Some(modal) = screen_chrome::modal(self) {
      layers = layers.push(modal);
    }
    if let Some(closing) = crate::screens::audio_window::closing(self) {
      layers = layers.push(closing);
    }
    if let Some(dialog) = &self.dialog {
      layers = layers.push(dialog::view(
        dialog,
        Message::CloseDialog,
        Message::ConfirmDialog,
      ));
    }
    let window = self.window.size.unwrap_or(DEFAULT_WINDOW);
    let menu_colors = menu::Colors::of(&self.theme.scheme);
    let choose = |message| Message::MenuChoose(Box::new(message));
    if let Some(menu) = &self.menu {
      layers = layers.push(menu::view(
        menu,
        window,
        menu_colors,
        choose,
        Message::CloseMenu,
        self.overlay_motion.menu_opacity(self.now),
      ));
    } else if let Some(menu) = &self.menu_leaving
      && self.overlay_motion.menu_fading(self.now)
    {
      layers = layers.push(menu::fading(
        menu,
        window,
        menu_colors,
        choose,
        self.overlay_motion.menu_opacity(self.now),
      ));
    }
    if let Some(toast) = self.toast_layer() {
      layers = layers.push(toast);
    }
    if let Some(splash) = self.splash_layer() {
      layers = layers.push(splash);
    }
    chrome::frame(layers.into(), self.window.is_maximized, Message::Chrome)
  }

  /// Calque du splash de démarrage, au-dessus de tout (`z-index: 999999`, `styles.css:9695`). Couleur : l'accent des réglages.
  fn splash_layer(&self) -> Option<Element<'_, Message>> {
    let splash = self.splash.as_ref()?;
    let accent = self
      .settings
      .theme_accent
      .parse()
      .unwrap_or(crate::theme::scheme::DEFAULT_SOURCE);
    let accent = iced::Color::from_rgb8(accent.red, accent.green, accent.blue);
    crate::splash::view(splash, self.now, accent)
  }

  /// Image d'animation du splash : la première image lui donne son départ, la dernière le retire.
  pub fn splash_frame(&mut self, at: Instant) {
    let Some(splash) = &mut self.splash else {
      return;
    };
    splash.start.get_or_insert(at);
    if splash.finished(at) {
      self.splash = None;
    }
  }
}

/// Minuterie de la notification montrée ; le toast spinner n'en a pas (`app.js:10474-10487`).
fn toast_timer(shown: Option<&toast::Toast>) -> Vec<Effect> {
  shown
    .filter(|toast| toast.kind != ToastKind::Spinner)
    .map_or_else(Vec::new, |toast| {
      vec![Effect::ToastTimer(
        toast.id,
        toast.timer_gen,
        toast.remaining,
      )]
    })
}

/// Minuteur d'animation, absent de la vue quand rien n'est animé. Un pixel : `Stack` et `Column` retirent un enfant de taille nulle.
pub(crate) fn ticker<'a>() -> Element<'a, Message> {
  canvas(Ticker {
    on_frame: Message::AnimationFrame,
  })
  .width(1)
  .height(1)
  .into()
}

/// Fond de `body` (`--md-sys-color-background`) et fond du bouton lecture (`--md-sys-color-primary`) du thème.
#[must_use]
pub fn theme_fade_targets(theme: &ActiveTheme) -> (Color, Color) {
  let primary = theme.scheme.primary;
  (
    theme.theme.extended_palette().background.base.color,
    Color::from_rgb8(primary.red, primary.green, primary.blue),
  )
}

fn transparent_surface_style(theme: &Theme) -> container::Style {
  container::Style::default().color(theme.extended_palette().background.base.text)
}

fn surface_style(theme: &Theme) -> container::Style {
  container::Style::default()
    .background(theme.extended_palette().background.base.color)
    .color(theme.extended_palette().background.base.text)
}

/// Position et durée remises à zéro pour un nouveau titre. Le périphérique reste.
#[must_use]
pub fn playback_reset(previous: &Playback) -> Playback {
  Playback {
    device: previous.device.clone(),
    ..Playback::default()
  }
}

impl Playback {
  /// Applique un événement du moteur. `Loaded` est réémis à chaque changement de périphérique : il ne touche pas à la position.
  pub(crate) fn apply(&mut self, event: AudioEvent) {
    match event {
      AudioEvent::Loaded {
        duration,
        sample_rate,
      } => {
        self.duration = duration;
        self.sample_rate = Some(sample_rate);
        self.error = None;
      }
      AudioEvent::Position(position) => self.position = position,
      AudioEvent::Ended => self.is_playing = false,
      AudioEvent::Buffering(buffering) => self.buffering = buffering,
      AudioEvent::DeviceChanged(device) => self.device = Some(device),
      AudioEvent::Error(err) => self.error = Some(err.to_string()),
    }
  }
}

/// Raccourcis internes (`runKeyboardShortcut`). Les touches d'un champ de saisie n'arrivent pas jusqu'ici.
fn shortcut(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Message> {
  let player = |message| Some(Message::Player(message));
  let command = modifiers.control() || modifiers.command();
  match key {
    keyboard::Key::Named(key::Named::Tab) => Some(Message::FocusMove {
      back: modifiers.shift(),
    }),
    keyboard::Key::Named(key::Named::Escape) => Some(Message::Escape),
    keyboard::Key::Named(key::Named::ArrowLeft) if modifiers.alt() => Some(Message::Back),
    keyboard::Key::Named(key::Named::Space) => player(PlayerMessage::TogglePlay),
    keyboard::Key::Named(key::Named::ArrowRight) if command => player(PlayerMessage::Next),
    keyboard::Key::Named(key::Named::ArrowLeft) if command => player(PlayerMessage::Prev),
    keyboard::Key::Named(key::Named::ArrowRight) if modifiers.shift() => {
      player(PlayerMessage::SeekBy(5.0))
    }
    keyboard::Key::Named(key::Named::ArrowLeft) if modifiers.shift() => {
      player(PlayerMessage::SeekBy(-5.0))
    }
    keyboard::Key::Named(key::Named::ArrowUp) if command => player(PlayerMessage::VolumeBy(1.0)),
    keyboard::Key::Named(key::Named::ArrowDown) if command => player(PlayerMessage::VolumeBy(-1.0)),
    keyboard::Key::Character(text) => {
      let letter = |message| Some(Message::Shortcut(Box::new(message)));
      match (text.to_lowercase().as_str(), command) {
        ("k", true) => Some(Message::Ui(UiMessage::FocusSearch)),
        ("m", false) => letter(Message::Player(PlayerMessage::ToggleMute)),
        ("h", false) => letter(Message::Player(PlayerMessage::ToggleShuffle)),
        ("r", false) => letter(Message::Player(PlayerMessage::ToggleRepeat)),
        ("l", false) => letter(Message::Lyrics(LyricsMessage::TogglePanel)),
        ("f", false) => letter(Message::Lyrics(LyricsMessage::OpenTvk)),
        ("p", false) => letter(Message::ToggleMiniMode),
        _ => None,
      }
    }
    _ => None,
  }
}

/// Tant que le bord est tenu : le curseur quitte le bord, le mouvement reste suivi.
fn sidebar_drag_event(
  event: Event,
  _status: event::Status,
  _window: window::Id,
) -> Option<Message> {
  match event {
    Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
      Some(Message::Ui(UiMessage::SidebarDragMoved(position.x)))
    }
    Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
      Some(Message::Ui(UiMessage::SidebarDragEnd))
    }
    _ => None,
  }
}

fn window_event(event: Event, status: event::Status, id: window::Id) -> Option<Message> {
  let for_window = |message| Some(Message::ForWindow(id, Box::new(message)));
  match event {
    // La répétition de l'espace ne bascule pas la lecture à chaque image du clavier.
    Event::Keyboard(keyboard::Event::KeyPressed {
      key: keyboard::Key::Named(key::Named::Space),
      repeat: true,
      ..
    }) => None,
    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
      if status == event::Status::Ignored =>
    {
      shortcut(&key, modifiers)
    }
    Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
      Some(Message::Ui(UiMessage::Modifiers(modifiers)))
    }
    Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
      if status == event::Status::Ignored =>
    {
      Some(Message::Ui(UiMessage::MouseReleased))
    }
    Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
      if status == event::Status::Ignored =>
    {
      Some(Message::Ui(UiMessage::PressedElsewhere))
    }
    Event::Window(window::Event::FileDropped(path)) => {
      Some(Message::Ui(UiMessage::FileDropped(path)))
    }
    Event::Window(window::Event::Opened { position, size }) => {
      for_window(Message::WindowOpened { position, size })
    }
    Event::Window(window::Event::Moved(position)) => for_window(Message::WindowMoved(position)),
    Event::Window(window::Event::Resized(size)) => for_window(Message::WindowResized(size)),
    Event::Window(window::Event::Rescaled(scale)) => for_window(Message::WindowRescaled(scale)),
    Event::Window(window::Event::CloseRequested) => for_window(Message::CloseRequested),
    _ => None,
  }
}

/// Arguments de lancement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartArgs {
  /// `--dev` : écran de développement.
  pub dev: bool,
  /// `--tray` : lancement de la session Windows, la fenêtre principale reste cachée dans la zone de notification.
  pub tray: bool,
  /// `--t0 <ms> --startup-log <fichier>` : mesure le démarrage depuis l'instant donné (ms Unix), puis quitte.
  pub startup_probe: Option<(u128, PathBuf)>,
  /// `--bench-scroll <fichier>` : défilement automatique de « Titres aimés », mesuré, puis quitte.
  pub bench_scroll: Option<PathBuf>,
  /// `--latency-log <fichier>` : délai entre la demande d'un titre et sa première position.
  pub latency_log: Option<PathBuf>,
  /// Fichiers audio passés en argument : ouverts comme un dépôt de fichiers.
  pub files: Vec<PathBuf>,
  /// `--scene` : mesure d'une scène, puis quitte.
  pub scene: Option<SceneArgs>,
}

impl StartArgs {
  /// Lit les arguments. Un argument inconnu est ignoré.
  #[must_use]
  pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
    let mut parsed = Self::default();
    let (mut t0, mut log) = (None, None);
    let mut scene = SceneArgs::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
      match arg.as_str() {
        "--dev" => parsed.dev = true,
        "--tray" => parsed.tray = true,
        "--t0" => t0 = args.next().and_then(|value| value.parse().ok()),
        "--startup-log" => log = args.next().map(PathBuf::from),
        "--bench-scroll" => parsed.bench_scroll = args.next().map(PathBuf::from),
        "--latency-log" => parsed.latency_log = args.next().map(PathBuf::from),
        "--scene" => scene.name = args.next().unwrap_or_default(),
        "--scene-file" => scene.file = args.next().map(PathBuf::from).unwrap_or_default(),
        "--scene-log" => scene.log = args.next().map(PathBuf::from).unwrap_or_default(),
        "--scene-seconds" => {
          scene.seconds = args
            .next()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
        }
        "--scene-pause" => scene.pause_after = args.next().and_then(|value| value.parse().ok()),
        "--scene-unthrottled" => scene.unthrottled = true,
        other if !other.starts_with('-') && !other.is_empty() => {
          parsed.files.push(PathBuf::from(other))
        }
        _ => {}
      }
    }
    parsed.startup_probe = t0.zip(log);
    parsed.scene = (!scene.name.is_empty()).then_some(scene);
    parsed
  }
}

struct StartupProbe {
  t0_ms: u128,
  log: PathBuf,
  frames: u32,
}

impl StartupProbe {
  fn record(&self) -> std::io::Result<()> {
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map_err(std::io::Error::other)?
      .as_millis();
    let mut file = OpenOptions::new()
      .create(true)
      .append(true)
      .open(&self.log)?;
    writeln!(file, "{}", now.saturating_sub(self.t0_ms))
  }
}

/// Application lancée : état, core et moteur.
pub struct Program {
  probe: Option<StartupProbe>,
  pub app: App,
  pub backend: Arc<Backend>,
  audio_events: AudioEvents,
  search_tasks: Vec<iced::task::Handle>,
  download_tasks: Vec<iced::task::Handle>,
  scroll_bench: Option<ScrollBench>,
  latency: Option<LatencyLog>,
  main_window: window::Id,
  mini_window: Option<window::Id>,
  /// Agrandie ou plein écran d'une fenêtre née cachée (`--tray`), rendus à sa première apparition.
  deferred_mode: crate::window::DeferredMode,
  analyser_hidden: bool,
  scene_frames: Option<Arc<Mutex<SceneFrames>>>,
  scene_play_pending: bool,
  system: SystemSide,
  /// Plugins Rhai actifs : aucun fil tant qu'aucun plugin n'est activé.
  plugin_host: riff_plugins::PluginHost,
  plugin_inbox: crate::plugins::PluginInbox,
  /// Session réseau du jam : `None` hors jam.
  jam: Option<ProgramJam>,
  /// Génération de la dernière session lancée : les événements d'une session finie sont ignorés.
  jam_generation: u64,
}

/// Session réseau du jam : commandes, boîte de réception, poignée de la tâche.
struct ProgramJam {
  generation: u64,
  commands: tokio::sync::mpsc::UnboundedSender<crate::jam::JamOut>,
  inbox: crate::jam::JamInbox,
  handle: iced::task::Handle,
}

/// Actions d'une scène, en secondes après le lancement : éditeur et synchro de bout en bout.
fn scene_steps(name: &str) -> Vec<(u64, LyricsMessage)> {
  let mut steps = Vec::new();
  if name.starts_with("editor") {
    steps.push((6, LyricsMessage::OpenEditor));
  }
  if name.starts_with("sync") {
    steps.extend([
      (6, LyricsMessage::OpenSync),
      (8, LyricsMessage::SyncLine(0)),
      (10, LyricsMessage::SyncLine(1)),
    ]);
  }
  match name {
    "editor-save" => steps.push((9, LyricsMessage::EditorSave)),
    "sync-save" => steps.push((12, LyricsMessage::SyncSave)),
    _ => {}
  }
  steps
}

/// Réglages d'une scène : mêmes calques que `scripts/scenes/measure-scene.ps1` pour Riff.
fn apply_scene(app: &mut App, name: &str, file: &std::path::Path) {
  let visual = &mut app.settings.visual;
  visual.m3_cover = false;
  visual.bar_wave = false;
  visual.cover_bg = false;
  app.lyrics.panel_open = false;
  match name {
    "b" => {
      app.lyrics.panel_open = true;
      visual.lyrics_word_style = WordStyle::Words;
    }
    "b2" => {
      app.lyrics.panel_open = true;
      visual.lyrics_word_style = WordStyle::Gradient;
    }
    "c" => {
      visual.m3_cover = true;
      visual.bar_wave = true;
      visual.bar_viz_behind = true;
    }
    "d" => visual.cover_bg = true,
    "tvk" => {
      visual.lyrics_word_style = WordStyle::Words;
      app.lyrics.tvk_open = true;
    }
    "tvk-gradient" => app.lyrics.tvk_open = true,
    "mesh" => {
      visual.mesh_gradient = true;
      visual.mesh.speed = 20.0;
    }
    "bg" | "bg-blur" => {
      app.lyrics.panel_open = true;
      visual.bg_url = file
        .with_file_name("cover.jpg")
        .to_string_lossy()
        .into_owned();
      visual.bg_opacity = 60;
      visual.bg_blur = if name == "bg-blur" { 12 } else { 0 };
    }
    "banner" => {
      app.lyrics.panel_open = true;
      visual.track_banners = true;
    }
    "liquid" => {
      visual.bar_wave = true;
      visual.viz_connected = true;
      visual.bar_viz_opacity = 60;
    }
    "bars-right" => {
      visual.bar_wave = true;
      visual.bar_viz_behind = false;
    }
    "editor" | "editor-save" | "sync" | "sync-save" => app.lyrics.panel_open = true,
    // Page de réglages, ouverte sur une catégorie (`settings-banners`) ou sur une recherche (`settings-find-lyrics`).
    _ if name.starts_with("settings") => {
      app.screen = Screen::Settings;
      if let Some(query) = name.strip_prefix("settings-find-") {
        app.ui.prefs.query = query.replace('_', " ");
      } else if let Some(category) = name.strip_prefix("settings-") {
        category.clone_into(&mut app.settings.settings_cat);
      }
    }
    "e" => {
      app.lyrics.panel_open = true;
      visual.lyrics_word_style = WordStyle::Words;
      visual.m3_cover = true;
      visual.bar_wave = true;
      visual.bar_viz_behind = true;
      visual.cover_bg = true;
    }
    _ => {}
  }
}

impl Program {
  /// Crée le core et le moteur, lit l'état de la fenêtre.
  ///
  /// # Panics
  /// Le fil de contrôle du moteur audio n'a pas démarré.
  pub fn boot(args: &StartArgs, mut main_settings: window::Settings) -> (Self, Task<Message>) {
    let window_state = PersistedWindowState::read(&paths::window_state_path()).unwrap_or_default();
    let storage = LegacyStorage::open(&riff_core::adopt::import_storage_path());
    // Premier lancement : l'offre d'import se décide AVANT tout `load_or_import`, qui écrirait les fichiers.
    let startup = import_flow::startup_state(
      &settings::settings_path(),
      &user_data::user_data_path(),
      storage,
      riff_core::util::now_millis() as i64,
    );
    let i18n = I18n::load(&startup.settings.ui_lang, &i18n_cache_dir());
    let mut app = App::new(window_state, startup.settings, i18n)
      .with_data(startup.data)
      .with_import(startup.pending);
    app.ui.prefs.translation = config::translation_enabled();
    if args.bench_scroll.is_some() {
      app.screen = Screen::Liked;
    }
    app.utc_offset_minutes = i64::from(jiff::ZonedDateTime::now().offset().seconds()) / 60;
    if args.dev {
      app = app.with_dev();
    }
    if let Some(id) = app.settings.native_theme.clone() {
      // Lu avant la première image : pas de passage visible par le thème par défaut.
      let loaded = file::load(&paths::themes_dir(), &id).map(Box::new);
      app.update(Message::ThemeFileLoaded(id, loaded));
    }
    let (backend, audio_events) = Backend::new().expect("fil de contrôle audio");
    app.lyrics.translation_enabled = backend.core.translation_enabled();
    // Pas de lecture de l'analyseur tant qu'aucun visualiseur n'est visible : petite FFT (décision 5).
    backend.engine.set_analyser_hidden(true);
    let mut system = SystemSide::start(&app);
    let plan = boot_plan(
      args.tray,
      system.expects_tray(),
      app.settings.visual.mini_lyrics && !app.import_waiting(),
      &mut main_settings,
    );
    let start_hidden = plan.start_hidden;
    main_settings.level = mini_level(app.settings.main_always_on_top);
    if start_hidden {
      system.main_hidden = true;
      app.window.main_hidden = true;
    }
    let deferred_mode = plan.deferred;
    let (main_window, open_main) = window::open(main_settings);
    let mut tasks = vec![
      open_main
        .then(window::raw_id::<Message>)
        .map(Message::MainHandle),
    ];
    let mut mini_window = None;
    if plan.open_mini {
      let (id, open) = window::open(mini_settings(app.settings.visual.mini_window));
      mini_window = Some(id);
      app.lyrics.mini_open = true;
      tasks.push(open.discard());
    }
    if args.bench_scroll.is_none() && args.startup_probe.is_none() {
      app.visuals.warmup_frames = DRIVER_WARMUP_FRAMES;
    }
    // Démarrage réel seulement : ni les scènes ni le banc de défilement, que le calque noir masquerait.
    if args.scene.is_none() && args.bench_scroll.is_none() {
      app.splash = plan.splash.then(crate::splash::Splash::default);
      app.view_motion.enabled = true;
      app.entry_motion.enabled = true;
      app.home_spinner.enabled = true;
      app.trending_spinner.enabled = true;
      app.bars_motion.enabled = true;
      app.artist_motion.enabled = true;
      app.overlay_motion.enabled = true;
      app.lyrics.tvk_motion.enabled = true;
      app.lyrics.panel_motion.enabled = true;
    }
    let mut scene_frames = None;
    if let Some(scene) = &args.scene {
      app.screen = Screen::Library;
      apply_scene(&mut app, &scene.name, &scene.file);
      app.visuals.unthrottled = scene.unthrottled;
      let frames = Arc::new(Mutex::new(SceneFrames::default()));
      finish_scene_after(scene.clone(), Arc::clone(&frames));
      scene_frames = Some(frames);
      tasks.push(Task::done(Message::Ui(UiMessage::LocalPicked(
        vec![scene.file.clone()],
        false,
      ))));
      if scene.name == "mini" {
        tasks.push(Task::done(Message::Lyrics(LyricsMessage::ToggleMini)));
      }
      for (seconds, step) in scene_steps(&scene.name) {
        tasks.push(Task::future(async move {
          tokio::time::sleep(Duration::from_secs(seconds)).await;
          Message::Lyrics(step)
        }));
      }
      if let Some(pause) = scene.pause_after {
        tasks.push(Task::future(async move {
          tokio::time::sleep(Duration::from_secs(pause)).await;
          Message::ScenePause
        }));
      }
    }
    // Reprise de la lecture : jamais pendant une mesure, qui n'écrit pas non plus l'état.
    // Une seconde instance (mode multi-instance) démarre vide : une seule instance joue et écrit.
    if args.scene.is_none()
      && args.bench_scroll.is_none()
      && args.startup_probe.is_none()
      && !crate::system::startup::secondary()
    {
      crate::resume::install();
      if !app.import_waiting()
        && let Some(saved) =
          crate::resume::load(&riff_core::paths::resolve_data_dir(), &app.data.platform)
      {
        // Des fichiers ouverts avec l'application remplacent la relance du titre.
        app.restore_playback(saved, args.files.is_empty());
        tasks.push(Task::future(async {
          tokio::time::sleep(crate::resume::START_FALLBACK).await;
          Message::Player(PlayerMessage::Resume)
        }));
      }
    }
    let (plugin_sink, plugin_inbox) = crate::plugins::PluginInbox::new();
    let plugin_host = riff_plugins::PluginHost::with_sink(crate::plugins::state_dir(), plugin_sink);
    // Plugins consentis : activés au lancement, hors du fil d'interface pour la lecture des fichiers.
    tasks.push(Task::done(Message::Plugin(PluginMessage::Scan)));
    for path in &args.files {
      tasks.push(Task::done(Message::Ui(UiMessage::FileDropped(
        path.clone(),
      ))));
    }
    (
      Self {
        probe: args.startup_probe.clone().map(|(t0_ms, log)| StartupProbe {
          t0_ms,
          log,
          frames: 0,
        }),
        app,
        backend: Arc::new(backend),
        audio_events: AudioEvents::new(audio_events),
        search_tasks: Vec::new(),
        download_tasks: Vec::new(),
        scroll_bench: args.bench_scroll.clone().map(ScrollBench::new),
        latency: args.latency_log.clone().map(LatencyLog::new),
        main_window,
        mini_window,
        deferred_mode,
        analyser_hidden: true,
        scene_play_pending: scene_frames.is_some(),
        scene_frames,
        system,
        plugin_host,
        plugin_inbox,
        jam: None,
        jam_generation: 0,
      },
      Task::batch(tasks),
    )
  }

  /// Applique un message et lance ses effets.
  pub fn update(&mut self, message: Message) -> Task<Message> {
    self.app.forget_gone_controls();
    if matches!(message, Message::ProbeFrame) {
      return self.probe_frame();
    }
    if let Message::BenchFrame(at) = message {
      return self.bench_frame(at);
    }
    if let Message::AnimationFrame(at) = message {
      return self.animation_frame(at);
    }
    if let Message::Jam(JamMessage::Event(generation, _) | JamMessage::TaskEnded(generation, _)) =
      &message
    {
      if self.jam.as_ref().map(|jam| jam.generation) != Some(*generation) {
        return Task::none();
      }
      if matches!(message, Message::Jam(JamMessage::TaskEnded(..))) {
        self.jam = None;
      }
    }
    let message = match message {
      Message::System(event) => return self.system_event(event),
      Message::MainHandle(handle) => {
        self.system.main_window_handle(handle);
        return self.sync_system();
      }
      Message::SystemCover(url, decoded) => {
        if let Some(decoded) = decoded {
          self.system.cover_loaded(&url, &decoded);
        }
        return self.sync_system();
      }
      other if self.closes_to_tray(&other) => return self.hide_main(),
      other => with_deferred_close_state(self.deferred_mode, other),
    };
    let message = match message {
      Message::ForWindow(id, inner) if Some(id) == self.mini_window => match mini_message(*inner) {
        Some(mini) => mini,
        None => return Task::none(),
      },
      other => other,
    };
    self.observe_latency(&message);
    let message = match message {
      Message::Player(PlayerMessage::NormalizeTick) => {
        Message::Player(PlayerMessage::Level(self.backend.engine.level_db()))
      }
      other => other,
    };
    let message = match message {
      Message::Plugin(PluginMessage::Outgoing(Outgoing::Call(call))) => {
        return self.plugin_call(call);
      }
      other => other,
    };
    let plugin_finished = match &message {
      Message::Plugin(PluginMessage::Outgoing(Outgoing::Lifecycle(
        Lifecycle::Disabled { file, .. } | Lifecycle::Failed { file, .. },
      ))) => Some(file.clone()),
      _ => None,
    };
    let plugin_message = matches!(message, Message::Plugin(_));
    let language_changed = matches!(message, Message::LanguageLoaded(_));
    let tick = match &message {
      Message::Audio(AudioEvent::Position(at)) => Some(*at),
      _ => None,
    };
    let effects = self.app.update(message);
    if language_changed {
      self.system.set_labels(tray_labels(&self.app));
    }
    if let Some(file) = plugin_finished {
      self.plugin_host.forget(&file);
    }
    if plugin_message {
      self.sync_plugin_flags();
    }
    // Sans plugin actif, `plugin_events` ne construit rien.
    for event in self.app.plugin_events() {
      self.plugin_host.dispatch(&event);
    }
    let mut tasks = Vec::with_capacity(effects.len() + 1);
    for effect in effects {
      tasks.push(self.run_effect(effect));
    }
    // Position exacte pour l'écriture de fin de session, sans écriture ici.
    if let Some(at) = tick
      && self.app.resume_live()
    {
      crate::resume::note_position(at);
    }
    let wanted = self.app.wanted_images();
    for key in self.app.images.request(&wanted) {
      let backend = Arc::clone(&self.backend);
      tasks.push(Task::future(async move {
        let result = images::load(backend, key.clone()).await;
        Message::ImageLoaded(key, result.map(Arc::new))
      }));
    }
    for job in self.app.take_blur_jobs() {
      tasks.push(Task::future(async move {
        let key = job.key.clone();
        let computed = tokio::task::spawn_blocking(move || {
          let started = Instant::now();
          let result = blur::compute(job.width, job.height, &job.pixels, &job.key);
          BLUR_MAX_MICROS.fetch_max(started.elapsed().as_micros() as u64, Ordering::Relaxed);
          result
        })
        .await;
        Message::BlurReady(key, computed.ok().flatten().map(Arc::new))
      }));
    }
    self.sync_analyser();
    tasks.push(self.sync_system());
    if self.scene_play_pending && !self.app.data.local_files.is_empty() {
      self.scene_play_pending = false;
      tasks.push(Task::done(Message::Ui(UiMessage::RowActivated(
        ListId::Local,
        0,
      ))));
    }
    Task::batch(tasks)
  }

  /// Pousse l'état au système (session média, Discord, paroles) et charge la pochette d'un nouveau titre.
  fn sync_system(&mut self) -> Task<Message> {
    let Some(url) = self.system.sync(&self.app) else {
      return Task::none();
    };
    let backend = Arc::clone(&self.backend);
    Task::future(async move {
      let decoded = images::load(backend, ImageKey::new(&url, images::SIZE_CARD))
        .await
        .ok()
        .map(Arc::new);
      Message::SystemCover(url, decoded)
    })
  }

  /// Fermer la fenêtre principale la masque quand la zone de notification est active.
  fn closes_to_tray(&self, message: &Message) -> bool {
    let main_close = match message {
      Message::ForWindow(id, inner) => {
        *id == self.main_window && matches!(**inner, Message::CloseRequested)
      }
      Message::CloseRequested | Message::Chrome(chrome::Action::Close) => true,
      _ => false,
    };
    main_close && self.system.hides_on_close(self.app.settings.tray_on_close)
  }

  fn hide_main(&mut self) -> Task<Message> {
    self.system.main_hidden = true;
    self.app.window.main_hidden = true;
    window::set_mode(self.main_window, window::Mode::Hidden)
  }

  fn show_main(&mut self) -> Task<Message> {
    self.system.main_hidden = false;
    self.app.window.main_hidden = false;
    let deferred = std::mem::take(&mut self.deferred_mode);
    let mut tasks = vec![
      window::set_mode(self.main_window, window::Mode::Windowed),
      window::minimize(self.main_window, false),
      window::gain_focus(self.main_window),
    ];
    if deferred.fullscreen {
      tasks.push(window::set_mode(self.main_window, window::Mode::Fullscreen));
    } else if deferred.maximized {
      tasks.push(window::maximize(self.main_window, true));
    }
    if deferred.mini_lyrics && self.mini_window.is_none() {
      let (id, open) = window::open(mini_settings(self.app.settings.visual.mini_window));
      self.mini_window = Some(id);
      self.app.lyrics.mini_open = true;
      tasks.push(open.discard());
    }
    Task::batch(tasks)
  }

  fn system_event(&mut self, event: SystemEvent) -> Task<Message> {
    match event {
      SystemEvent::Show(paths) => {
        let mut tasks = vec![self.show_main()];
        tasks.extend(
          paths
            .into_iter()
            .filter(|path| riff_core::local_media::is_audio_file(path))
            .map(|path| Task::done(Message::Ui(UiMessage::FileDropped(path)))),
        );
        Task::batch(tasks)
      }
      SystemEvent::ToggleWindow if self.system.main_hidden => self.show_main(),
      SystemEvent::ToggleWindow => self.hide_main(),
      SystemEvent::Command(command) => {
        let mut tasks = Vec::new();
        for message in self.system.messages_for(command, &self.app, Instant::now()) {
          tasks.push(self.update(message));
        }
        Task::batch(tasks)
      }
      SystemEvent::ToggleAlwaysOnTop => self.update(Message::ToggleMainAlwaysOnTop),
      SystemEvent::ToggleAutostart => {
        self.system.toggle_autostart();
        Task::none()
      }
      SystemEvent::TrayUnavailable if self.system.main_hidden => self.show_main(),
      SystemEvent::TrayUnavailable => Task::none(),
      SystemEvent::Quit => {
        self.system.quitting = true;
        self.update(Message::CloseRequested)
      }
    }
  }

  /// Petite FFT quand aucun visualiseur n'est visible (décision 5).
  fn sync_analyser(&mut self) {
    let hidden = !self.app.visualizer_visible();
    if hidden != self.analyser_hidden {
      self.analyser_hidden = hidden;
      self.backend.engine.set_analyser_hidden(hidden);
    }
  }

  /// Une image du minuteur d'animation. Fenêtre réduite ou aucun visualiseur : l'analyseur n'est pas lu.
  fn animation_frame(&mut self, at: Instant) -> Task<Message> {
    self.app.now = at;
    self.app.sync_shape_loop(at);
    self.app.ui.controls.prune(at);
    self.app.splash_frame(at);
    if let Some(frames) = &self.scene_frames
      && let Ok(mut frames) = frames.lock()
    {
      frames.frame(at);
    }
    if self.app.visuals.last_tick.is_some_and(|last| {
      at.saturating_duration_since(last)
        < if crate::motion::perf() {
          PERF_TICK
        } else {
          MIN_TICK
        }
    }) {
      return Task::none();
    }
    let mut freq = [0_u8; FREQ_BINS];
    self.sync_analyser();
    if self.app.visualizer_visible() {
      self.backend.engine.frequency_bytes(&mut freq);
    }
    let effects = self.app.animation_tick(at, &freq);
    Task::batch(
      effects
        .into_iter()
        .map(|effect| self.run_effect(effect))
        .collect::<Vec<_>>(),
    )
  }

  /// Une image du défilement mesuré.
  fn bench_frame(&mut self, at: Instant) -> Task<Message> {
    let Some(bench) = &mut self.scroll_bench else {
      return Task::none();
    };
    let list = self.app.ui.list(ListId::Favorites);
    let end = (self.app.list_len(ListId::Favorites) as f32 * crate::ui::ROW_HEIGHT
      - list.viewport_height)
      .max(0.0);
    match bench.frame(at, list.offset, end) {
      Some(step) => iced::widget::operation::scroll_by(
        ListId::Favorites.scroll_id(),
        iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: step },
      ),
      None => {
        if let Err(err) = bench.write() {
          tracing::warn!(error = %err, "écriture de la mesure de défilement");
        }
        iced::exit()
      }
    }
  }

  fn observe_latency(&mut self, message: &Message) {
    let Some(log) = &mut self.latency else {
      return;
    };
    match message {
      Message::Player(PlayerMessage::Resolved { result: Ok(_), .. }) => {
        log.source_ready(Instant::now())
      }
      Message::Audio(AudioEvent::Position(position))
        if !self.app.player.loading && !position.is_zero() =>
      {
        log.first_position(Instant::now())
      }
      Message::Ui(UiMessage::RowActivated(..) | UiMessage::RowReleased)
      | Message::Player(PlayerMessage::Next) => log.track_requested(Instant::now()),
      _ => {}
    }
  }

  /// Drapeaux des plugins vus par `App` (pur) : actifs, tirage de mélange.
  fn sync_plugin_flags(&mut self) {
    self.app.plugins.active = self.plugin_host.is_active();
    self.app.plugins.picks = self.plugin_host.picks();
  }

  /// Répond à la demande d'un plugin. La recherche passe par le core (`Backend::call`), avec échéance.
  fn plugin_call(&mut self, call: riff_plugins::HostCall) -> Task<Message> {
    let answer = self.app.answer_plugin(&call.plugin, &call.request);
    let (effects, search) = match answer {
      PluginAnswer::Now(response, effects) => {
        call.reply(response);
        (effects, None)
      }
      PluginAnswer::Silent(effects) => (effects, None),
      PluginAnswer::Search {
        query,
        limit,
        platform,
      } => {
        let mut params = HashMap::from([
          ("q".to_string(), query),
          ("limit".to_string(), limit.to_string()),
        ]);
        if let Some(platform) = platform {
          params.insert("platform".to_string(), platform);
        }
        (Vec::new(), Some(("/api/search", params, None)))
      }
      PluginAnswer::Vibe(body) => (
        Vec::new(),
        Some(("/api/vibe", HashMap::new(), Some(body.to_string()))),
      ),
    };
    let mut tasks: Vec<Task<Message>> = effects
      .into_iter()
      .map(|effect| self.run_effect(effect))
      .collect();
    if let Some((path, params, body)) = search {
      let backend = Arc::clone(&self.backend);
      tasks.push(Task::future(async move {
        let reply = tokio::time::timeout(
          riff_plugins::api::SEARCH_WAIT,
          backend.call(path, params, body.map(Into::into)),
        )
        .await;
        let response = match reply {
          Ok(Ok(Reply::Json { body, .. })) => {
            riff_plugins::HostResponse::Value(crate::plugins::tracks_of_reply(&body))
          }
          Ok(Ok(other)) => riff_plugins::HostResponse::Error(format!("réponse {}", other.status())),
          Ok(Err(err)) => riff_plugins::HostResponse::Error(err.to_string()),
          Err(_) => riff_plugins::HostResponse::Error("délai dépassé".to_string()),
        };
        call.reply(response);
        Message::NoOp
      }));
    }
    Task::batch(tasks)
  }

  /// Lance la tâche réseau d'un jam et range son lien. Une session précédente encore présente est annulée.
  fn launch_jam(
    &mut self,
    link: crate::jam::JamLink,
    session: impl std::future::Future<Output = crate::jam::EndCause> + Send + 'static,
    invalid: crate::jam::InvalidUrls,
  ) -> Task<Message> {
    let generation = self.jam_generation;
    let (task, handle) = Task::future(async move {
      let cause = session.await;
      Message::Jam(JamMessage::TaskEnded(generation, cause))
    })
    .abortable();
    if let Some(old) = self.jam.replace(ProgramJam {
      generation,
      commands: link.commands,
      inbox: link.inbox,
      handle,
    }) {
      old.handle.abort();
    }
    let toasts: Vec<_> = [
      (invalid.relay, "Invalid relay URL, using the default relays"),
      (
        invalid.rendezvous,
        "Invalid rendezvous URL, short codes are unavailable",
      ),
    ]
    .into_iter()
    .filter(|(flagged, _)| *flagged)
    .map(|(_, key)| {
      Task::done(Message::ShowToast(
        self.app.i18n.t(key),
        ToastKind::Error,
        None,
      ))
    })
    .collect();
    if toasts.is_empty() {
      return task;
    }
    Task::batch(std::iter::once(task).chain(toasts))
  }

  /// Effets qui touchent à l'état de `Program`, puis les autres.
  fn run_effect(&mut self, effect: Effect) -> Task<Message> {
    match effect {
      Effect::ActivatePlugin(source) => {
        let file = source.file.clone();
        if let Err(reason) = self.plugin_host.activate(*source) {
          return Task::done(Message::Plugin(PluginMessage::Outgoing(
            Outgoing::Lifecycle(Lifecycle::Failed { file, reason }),
          )));
        }
        self.sync_plugin_flags();
        Task::none()
      }
      Effect::DeactivatePlugin(file) => {
        self.plugin_host.deactivate(&file);
        self.sync_plugin_flags();
        Task::none()
      }
      Effect::PluginPick { id, ctx } => match self.plugin_host.shuffle_pick(ctx) {
        Some(answer) => Task::future(async move {
          let result = tokio::task::spawn_blocking(move || {
            answer
              .recv_timeout(crate::plugins::PICK_WAIT)
              .unwrap_or_else(|_| Err("délai dépassé".to_string()))
          })
          .await
          .unwrap_or_else(|err| Err(err.to_string()));
          Message::Plugin(PluginMessage::Picked(id, result))
        }),
        None => Task::done(Message::Plugin(PluginMessage::Picked(
          id,
          Err("aucun plugin disponible".to_string()),
        ))),
      },
      Effect::JamHost {
        name,
        relay,
        rendezvous,
      } => {
        let (transport, invalid) = crate::jam::transport(relay.as_deref(), rendezvous.as_deref());
        self.jam_generation += 1;
        let (link, session) = crate::jam::start_host(transport, name, self.jam_generation);
        self.launch_jam(link, session, invalid)
      }
      Effect::JamJoin {
        ticket,
        name,
        relay,
        rendezvous,
      } => {
        let (transport, invalid) = crate::jam::transport(relay.as_deref(), rendezvous.as_deref());
        self.jam_generation += 1;
        let (link, session) = crate::jam::start_guest(transport, ticket, name, self.jam_generation);
        self.launch_jam(link, session, invalid)
      }
      Effect::JamSend(out) => {
        if let Some(jam) = &self.jam {
          // Tâche déjà finie : la fin arrive par `TaskEnded`.
          let _ = jam.commands.send(out);
        }
        Task::none()
      }
      Effect::JamPaste => {
        iced::clipboard::read().map(|text| Message::Jam(JamMessage::Pasted(text)))
      }
      Effect::AbortSearch => {
        self
          .search_tasks
          .drain(..)
          .for_each(|handle| handle.abort());
        Task::none()
      }
      Effect::AbortDownload => {
        self
          .download_tasks
          .drain(..)
          .for_each(|handle| handle.abort());
        Task::none()
      }
      Effect::RunDownload(job) => {
        let (task, handle) = run_download(Arc::clone(&self.backend), *job).abortable();
        self.download_tasks.push(handle);
        task
      }
      Effect::OpenMini(settings) => {
        let (id, open) = window::open(mini_settings(settings));
        self.mini_window = Some(id);
        open.discard()
      }
      Effect::CloseMini => self
        .mini_window
        .take()
        .map_or_else(Task::none, window::close),
      Effect::MiniLevel(on_top) => self
        .mini_window
        .map_or_else(Task::none, |id| window::set_level(id, mini_level(on_top))),
      Effect::MainLevel(on_top) => window::set_level(self.main_window, mini_level(on_top)),
      Effect::MiniDrag => self.mini_window.map_or_else(Task::none, window::drag),
      Effect::CoreJson {
        tag,
        path,
        query,
        body,
        search,
      } => {
        let task = core_json(Arc::clone(&self.backend), tag, path, query, body);
        if search {
          let (task, handle) = task.abortable();
          self.search_tasks.push(handle);
          task
        } else {
          task
        }
      }
      // Quitter le mode mini remet la fenêtre au niveau normal : le réglage « Always on top » reprend ensuite la main.
      Effect::LeaveMiniWindow(..) => run(effect, &self.backend).chain(window::set_level(
        self.main_window,
        mini_level(main_on_top(self.app.settings.main_always_on_top, false)),
      )),
      other => run(other, &self.backend),
    }
  }

  #[must_use]
  pub fn view(&self, id: window::Id) -> Element<'_, Message> {
    VIEW_COUNT.fetch_add(1, Ordering::Relaxed);
    if Some(id) == self.mini_window {
      return lyrics_screen::mini(&self.app);
    }
    debug_assert_eq!(id, self.main_window);
    self.app.view()
  }

  /// La deuxième image part après la première image présentée : borne haute du premier contenu.
  fn probe_frame(&mut self) -> Task<Message> {
    let Some(probe) = &mut self.probe else {
      return Task::none();
    };
    probe.frames += 1;
    if probe.frames < 2 {
      return Task::none();
    }
    if let Err(err) = probe.record() {
      tracing::warn!(error = %err, "écriture de la mesure de démarrage");
    }
    iced::exit()
  }

  #[must_use]
  pub fn theme(&self) -> Theme {
    self.app.theme.theme.clone()
  }

  /// Échelle de l'interface (`devsize_ui_scale`).
  #[must_use]
  pub fn scale_factor(&self) -> f32 {
    self.app.scale_factor()
  }

  pub fn subscription(&self) -> Subscription<Message> {
    let probe = if self.probe.is_some() {
      window::frames().map(|_| Message::ProbeFrame)
    } else {
      Subscription::none()
    };
    let normalize = if self.app.data.normalize && self.app.playback.is_playing {
      iced::time::every(NORMALIZE_TICK).map(|_| Message::Player(PlayerMessage::NormalizeTick))
    } else {
      Subscription::none()
    };
    let preset_tween = if self.app.ui.audio.tween.is_some() {
      iced::time::every(audio_prefs::TWEEN_TICK)
        .map(|_| Message::AudioPrefs(AudioMessage::TweenTick))
    } else {
      Subscription::none()
    };
    let sleep_countdown = if self.app.sleep_countdown_visible() {
      iced::time::every(Duration::from_secs(1))
        .map(|_| Message::AudioPrefs(AudioMessage::SleepTick))
    } else {
      Subscription::none()
    };
    let lastfm_poll = if self.app.ui.manage.lastfm.token.is_some() {
      iced::time::every(manage::LASTFM_POLL).map(|_| Message::Manage(ManageMessage::LastfmPoll))
    } else {
      Subscription::none()
    };
    let tools_poll = if self.app.ui.manage.tools.installing {
      iced::time::every(manage::TOOLS_POLL).map(|_| Message::Manage(ManageMessage::PollTools))
    } else {
      Subscription::none()
    };
    let bench = if self.scroll_bench.is_some() {
      window::frames().map(Message::BenchFrame)
    } else {
      Subscription::none()
    };
    // Le minuteur à 60 Hz est un widget (`widgets/ticker.rs`). Ici, seule la mesure de capacité suit l'écran.
    let animation = if self.app.visuals.unthrottled && self.app.animation_needed() {
      window::frames().map(Message::AnimationFrame)
    } else {
      Subscription::none()
    };
    // `resetTvIdleTimer` : une vérification par seconde tant que la vue plein écran est active.
    let tvk_idle = if self.app.lyrics.tvk_open && !self.app.lyrics.tvk_idle {
      iced::time::every(Duration::from_secs(1)).map(|_| {
        Message::Lyrics(LyricsMessage::Tvk(
          crate::lyrics::tvk::TvkMessage::IdleCheck,
        ))
      })
    } else {
      Subscription::none()
    };
    let sidebar_drag = if self.app.ui.sidebar_drag.is_some() {
      event::listen_with(sidebar_drag_event)
    } else {
      Subscription::none()
    };
    let jam = self.jam.as_ref().map_or_else(Subscription::none, |jam| {
      crate::jam::subscription(jam.inbox.clone(), |generation, event| {
        Message::Jam(JamMessage::Event(generation, Box::new(event)))
      })
    });
    let jam_heartbeat = if self.app.jam_heartbeat_needed() {
      iced::time::every(crate::jam::HEARTBEAT).map(|_| Message::Jam(JamMessage::Heartbeat))
    } else {
      Subscription::none()
    };
    Subscription::batch([
      jam,
      jam_heartbeat,
      lastfm_poll,
      tvk_idle,
      tools_poll,
      preset_tween,
      sleep_countdown,
      animation,
      bench,
      sidebar_drag,
      normalize,
      probe,
      event::listen_with(window_event),
      audio_sub::subscription(self.audio_events.clone()),
      crate::system::events::subscription(),
      core_sub::subscription(Arc::clone(&self.backend.core)),
      crate::plugins::subscription(self.plugin_inbox.clone()),
    ])
  }
}

/// Événement de la fenêtre des mini-paroles.
fn mini_message(event: Message) -> Option<Message> {
  let message = match event {
    Message::WindowMoved(position) if position.x > chrome::MINIMIZED_POSITION => {
      LyricsMessage::MiniMoved(position)
    }
    Message::WindowResized(size) => LyricsMessage::MiniResized(size),
    Message::CloseRequested => LyricsMessage::MiniClosed,
    _ => return None,
  };
  Some(Message::Lyrics(message))
}

/// Niveau voulu de la fenêtre principale : le mode compact la garde au premier plan quel que soit le réglage.
const fn main_on_top(setting: bool, compact: bool) -> bool {
  setting || compact
}

/// Décisions de `Program::boot` qui dépendent du lancement `--tray`.
struct BootPlan {
  start_hidden: bool,
  deferred: crate::window::DeferredMode,
  /// Ouvrir la fenêtre de mini-paroles dès le lancement.
  open_mini: bool,
  /// Jouer le splash : caché, il se jouerait sans personne et réveillerait l'animation.
  splash: bool,
}

/// Sans choix de l'écran d'import, pas de mini-paroles : elles viendraient de réglages pas encore acceptés (`mini_wanted`).
fn boot_plan(
  tray_arg: bool,
  tray_expected: bool,
  mini_wanted: bool,
  main_settings: &mut window::Settings,
) -> BootPlan {
  let start_hidden = autostart::starts_hidden(tray_arg, tray_expected);
  let mut deferred = crate::window::DeferredMode::default();
  if start_hidden {
    deferred = crate::window::hide_at_creation(main_settings);
    deferred.mini_lyrics = mini_wanted;
  }
  BootPlan {
    start_hidden,
    deferred,
    open_mini: mini_wanted && !start_hidden,
    splash: !start_hidden,
  }
}

/// Une fenêtre née cachée n'est ni agrandie ni en plein écran : à la fermeture, les états mis de côté comptent.
fn with_deferred_close_state(deferred: crate::window::DeferredMode, message: Message) -> Message {
  match message {
    Message::CloseStateKnown {
      is_maximized,
      is_full_screen,
    } => Message::CloseStateKnown {
      is_maximized: is_maximized || deferred.maximized,
      is_full_screen: is_full_screen || deferred.fullscreen,
    },
    other => other,
  }
}

/// Niveau de fenêtre d'après un réglage « au premier plan » (fenêtre principale ou paroles mini).
const fn mini_level(on_top: bool) -> window::Level {
  if on_top {
    window::Level::AlwaysOnTop
  } else {
    window::Level::Normal
  }
}

/// Fenêtre des mini-paroles (décision 6) : sans bordure, position et taille retrouvées.
fn mini_settings(saved: MiniWindow) -> window::Settings {
  window::Settings {
    size: Size::new(saved.width, saved.height),
    position: window::Position::Specific(Point::new(saved.x, saved.y)),
    min_size: Some(Size::new(240.0, 120.0)),
    decorations: false,
    exit_on_close_request: false,
    level: mini_level(saved.always_on_top),
    ..window::Settings::default()
  }
}

fn i18n_cache_dir() -> PathBuf {
  paths::resolve_data_dir().join(i18n::CACHE_DIR)
}

/// Charge une langue. La traduction réseau ne part que si `translationEnabled` vaut `true` dans `config.json`.
async fn load_language(backend: Arc<Backend>, lang: String) -> Arc<I18n> {
  let cache = i18n_cache_dir();
  let loaded = I18n::load(&lang, &cache);
  if !i18n::needs_network(&config::read_config(), &loaded, i18n::UI_KEYS) {
    return Arc::new(loaded);
  }
  let body = json!({ "target": lang, "texts": loaded.missing(i18n::UI_KEYS) }).to_string();
  match backend
    .call("/api/i18n-batch", HashMap::new(), Some(body.into()))
    .await
  {
    Ok(_) => Arc::new(I18n::load(&lang, &cache)),
    Err(err) => {
      tracing::warn!(error = %err, "traduction de l'interface");
      Arc::new(loaded)
    }
  }
}

async fn dev_search(
  backend: &Backend,
  query: String,
  limit: &str,
) -> Result<Vec<TrackRow>, String> {
  let params = HashMap::from([
    ("q".to_string(), query),
    ("limit".to_string(), limit.to_string()),
  ]);
  match backend
    .call("/api/search", params, None)
    .await
    .map_err(|err| err.to_string())?
  {
    Reply::Json { status: 200, body } => Ok(dev::rows_from_search(&body)),
    other => Err(format!("réponse {}", other.status())),
  }
}

/// Fichier source d'un téléchargement avec rendu : fichier local, ou audio brut demandé au core.
async fn raw_source(
  backend: &Backend,
  track: &Track,
  job: Option<&download_ui::DownloadJob>,
) -> Result<(PathBuf, bool), String> {
  if let (true, Some(path)) = (track.is_local(), track.path.as_deref()) {
    return Ok((PathBuf::from(path), false));
  }
  let query = match job {
    Some(job) => download_ui::download_query(job, true),
    None => vec![
      ("title".to_string(), track.title.clone().unwrap_or_default()),
      (
        "artist".to_string(),
        track.artist.clone().unwrap_or_default(),
      ),
      ("id".to_string(), track.id.clone().unwrap_or_default()),
      ("url".to_string(), track.url.clone().unwrap_or_default()),
      ("raw".to_string(), "1".to_string()),
    ],
  };
  let reply = backend
    .call("/api/download", query.into_iter().collect(), None)
    .await
    .map_err(|err| err.to_string())?;
  let Reply::Json { body, .. } = reply else {
    return Err("réponse inattendue".to_string());
  };
  if body["success"].as_bool() != Some(true) {
    return Err(
      body["error"]
        .as_str()
        .unwrap_or("download failed")
        .to_string(),
    );
  }
  let name = body["tempName"].as_str().ok_or("tempName absent")?;
  Ok((
    riff_core::services::playback::downloads_dir()
      .join(".tmp")
      .join(name),
    true,
  ))
}

/// Onde d'un titre pour la découpe, calculée hors du fil d'interface.
async fn load_wave(backend: Arc<Backend>, track: Track) -> Result<download_ui::WaveData, String> {
  let (path, temporary) = raw_source(&backend, &track, None).await?;
  let decoded = tokio::task::spawn_blocking(move || {
    let result = waveform::decode_all(&riff_audio::Source::File(path.clone()));
    if temporary {
      let _ = std::fs::remove_file(&path);
    }
    result
  })
  .await
  .map_err(|err| err.to_string())?
  .map_err(|err| err.to_string())?;
  Ok(download_ui::WaveData {
    peaks: waveform::peaks(&decoded.samples, download_ui::WAVE_COLUMNS),
    duration: decoded.duration_secs(),
  })
}

/// Rendu hors ligne de l'audio avec effets, écrit dans un WAV temporaire.
fn render_to_wav(
  source: PathBuf,
  temporary: bool,
  trim: Option<(f64, f64)>,
  settings: riff_audio::AudioSettings,
  hq: Option<riff_audio::HqSettings>,
) -> Result<PathBuf, String> {
  let decoded =
    waveform::decode_all(&riff_audio::Source::File(source.clone())).map_err(|err| err.to_string());
  if temporary {
    let _ = std::fs::remove_file(&source);
  }
  let decoded = decoded?;
  let slice = match trim {
    Some((start, end)) => decoded.slice(start, end),
    None => decoded.samples.as_slice(),
  };
  let rendered = riff_audio::offline::render(slice, 2, decoded.rate, &settings, hq.as_ref());
  let wav = riff_audio::offline::encode_wav_16(&rendered, 2, decoded.rate);
  let out = riff_core::services::playback::downloads_dir().join(".tmp");
  std::fs::create_dir_all(&out).map_err(|err| err.to_string())?;
  let out = out.join(format!("render_{}.wav", riff_core::util::now_millis()));
  std::fs::write(&out, wav).map_err(|err| err.to_string())?;
  Ok(out)
}

async fn perform_download(
  backend: Arc<Backend>,
  job: download_ui::DownloadJob,
) -> Result<String, String> {
  let encode =
    |input: PathBuf, trim: Option<(f64, f64)>| riff_core::services::download::EncodeFile {
      input,
      title: job.track.title.clone().unwrap_or_default(),
      artist: job.track.artist.clone().unwrap_or_default(),
      format: job.format.clone(),
      quality: job.quality.clone(),
      thumbnail: job.track.thumb_url().unwrap_or_default(),
      custom_dir: String::new(),
      start: trim.map(|(start, _)| start),
      end: trim.map(|(_, end)| end),
    };
  match &job.plan {
    download_ui::DownloadPlan::Remote => {
      let query = download_ui::download_query(&job, false)
        .into_iter()
        .collect();
      let reply = backend
        .call("/api/download", query, None)
        .await
        .map_err(|err| err.to_string())?;
      let Reply::Json { body, .. } = reply else {
        return Err("réponse inattendue".to_string());
      };
      if body["success"].as_bool() == Some(true) {
        Ok(body["filename"].as_str().unwrap_or("").to_string())
      } else {
        Err(
          body["error"]
            .as_str()
            .unwrap_or("download failed")
            .to_string(),
        )
      }
    }
    download_ui::DownloadPlan::Local(path) => {
      riff_core::services::download::encode_file(&backend.core, &encode(path.clone(), job.trim))
        .await
        .map(|done| done.filename)
    }
    download_ui::DownloadPlan::Rendered { settings, hq, .. } => {
      let (source, temporary) = raw_source(&backend, &job.track, Some(&job)).await?;
      let (trim, settings, hq) = (job.trim, **settings, *hq);
      let rendered =
        tokio::task::spawn_blocking(move || render_to_wav(source, temporary, trim, settings, hq))
          .await
          .map_err(|err| err.to_string())??;
      let result =
        riff_core::services::download::encode_file(&backend.core, &encode(rendered.clone(), None))
          .await
          .map(|done| done.filename);
      let _ = std::fs::remove_file(&rendered);
      result
    }
  }
}

fn run_download(backend: Arc<Backend>, job: download_ui::DownloadJob) -> Task<Message> {
  let token = job.token;
  Task::future(async move {
    Message::Download(DownloadMessage::Finished(
      token,
      perform_download(backend, job).await,
    ))
  })
}

/// Lit une pochette par le core : URL locale telle quelle, URL distante par `/api/image-proxy`.
async fn fetch_cover(backend: &Backend, url: &str) -> Result<Vec<u8>, String> {
  let local = if url.starts_with(LOCAL_BASE_URL) {
    url.to_string()
  } else {
    let query: String = form_urlencoded::Serializer::new(String::new())
      .append_pair("url", url)
      .finish();
    format!("{LOCAL_BASE_URL}/api/image-proxy?{query}")
  };
  let reply = backend
    .resolve_url(&local)
    .await
    .map_err(|err| err.to_string())?;
  Backend::reply_bytes(reply)
    .await
    .map(|bytes| bytes.to_vec())
    .map_err(|err| err.to_string())
}

fn tracks_from(value: &Value) -> Vec<Track> {
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

/// Appelle le core et renvoie sa réponse JSON.
fn core_json(
  backend: Arc<Backend>,
  tag: ReplyTag,
  path: &'static str,
  query: Vec<(String, String)>,
  body: Option<String>,
) -> Task<Message> {
  Task::future(async move {
    let result = match backend
      .call(path, query.into_iter().collect(), body.map(Into::into))
      .await
    {
      Ok(Reply::Json { body, .. }) => Ok(body),
      Ok(other) => Err(format!("réponse {}", other.status())),
      Err(err) => Err(err.to_string()),
    };
    Message::Ui(UiMessage::Reply(tag, result))
  })
}

/// `buildAutoplayCandidates` : titres des artistes de la file, puis recommandations.
async fn autoplay_candidates(
  backend: Arc<Backend>,
  artists: Vec<String>,
  payload: Value,
) -> Vec<Track> {
  let artist_calls = artists.into_iter().map(|name| {
    let backend = Arc::clone(&backend);
    async move {
      let query = HashMap::from([("name".to_string(), name)]);
      match tokio::time::timeout(
        AUTOPLAY_ARTIST_TIMEOUT,
        backend.call("/api/artist", query, None),
      )
      .await
      {
        Ok(Ok(Reply::Json { body, .. })) => tracks_from(&body["artist"]["tracks"]),
        _ => Vec::new(),
      }
    }
  });
  let by_artist = iced::futures::future::join_all(artist_calls).await;
  let same_artist: Vec<Track> = by_artist
    .into_iter()
    .flat_map(|tracks| tracks.into_iter().take(AUTOPLAY_PER_ARTIST))
    .collect();
  let wave = match tokio::time::timeout(
    AUTOPLAY_VIBE_TIMEOUT,
    backend.call(
      "/api/vibe",
      HashMap::new(),
      Some(payload.to_string().into()),
    ),
  )
  .await
  {
    Ok(Ok(Reply::Json { body, .. })) => tracks_from(&body["tracks"]),
    _ => Vec::new(),
  };
  let split = same_artist.len().min(2);
  same_artist[..split]
    .iter()
    .cloned()
    .chain(wave)
    .chain(same_artist[split..].iter().cloned())
    .collect()
}

const AUDIO_FILTER: &[&str] = &["mp3", "m4a", "flac", "wav", "ogg", "opus", "aac", "webm"];

/// Sélecteur natif attaché à la fenêtre.
fn pick_dialog() -> Task<rfd::AsyncFileDialog> {
  window::oldest()
    .and_then(|id| window::run(id, |window| rfd::AsyncFileDialog::new().set_parent(&window)))
}

/// Retient quel widget avait le focus avant d'ouvrir une superposition.
fn save_focus_task() -> Task<Message> {
  let (operation, slot) = crate::widgets::focus::focused_ordinal();
  operate(operation).discard().chain(Task::future(async move {
    Message::Ui(UiMessage::FocusSaved(
      slot.lock().ok().and_then(|value| *value),
    ))
  }))
}

fn cover_message(bytes: Result<Vec<u8>, String>) -> Message {
  match bytes {
    Ok(bytes) => Message::CoverBytes(bytes.into()),
    Err(reason) => Message::ShowToast(reason, ToastKind::Error, None),
  }
}

/// Une opération de widget ne demande pas d'image à `iced_winit` : le message qui suit fait apparaître l'anneau de focus.
fn focus_task(focus: impl Operation<()> + 'static) -> Task<Message> {
  operate(focus).discard().chain(Task::done(Message::NoOp))
}

/// Opération de focus limitée à la superposition ouverte.
fn scoped_focus(focus: impl Operation<()> + 'static) -> Task<Message> {
  focus_task(operation::scope(OVERLAY_ID.into(), focus))
}

fn run(effect: Effect, backend: &Arc<Backend>) -> Task<Message> {
  // La fenêtre principale est toujours la plus ancienne : les mini-paroles ne reçoivent pas ces actions.
  let latest = window::oldest();
  match effect {
    Effect::LyricsScroll(y) => iced::widget::operation::scroll_to(
      SCROLL_ID,
      scrollable::AbsoluteOffset {
        x: None,
        y: Some(y),
      },
    ),
    Effect::SidebarFeedTop => iced::widget::operation::scroll_to(
      crate::screens::sidebar::FEED_ID,
      scrollable::AbsoluteOffset {
        x: None,
        y: Some(0.0),
      },
    ),
    Effect::OpenMini(_)
    | Effect::CloseMini
    | Effect::MiniLevel(_)
    | Effect::MainLevel(_)
    | Effect::MiniDrag
    | Effect::JamHost { .. }
    | Effect::JamJoin { .. }
    | Effect::JamSend(_)
    | Effect::JamPaste
    | Effect::PluginPick { .. }
    | Effect::ActivatePlugin(_)
    | Effect::DeactivatePlugin(_) => Task::none(),
    Effect::ScanPlugins => Task::future(tokio::task::spawn_blocking(|| {
      riff_plugins::store::scan(&crate::plugins::plugins_dir())
    }))
    .map(|scan| Message::Plugin(PluginMessage::Scanned(Box::new(scan.unwrap_or_default())))),
    Effect::LoadPlugin(file) => Task::future(tokio::task::spawn_blocking(move || {
      riff_plugins::manifest::load(&crate::plugins::plugins_dir(), &file)
        .map(Box::new)
        .map_err(|err| format!("{file} : {err}"))
    }))
    .map(|loaded| {
      Message::Plugin(PluginMessage::Loaded(
        loaded.unwrap_or_else(|err| Err(err.to_string())),
      ))
    }),
    Effect::RemovePlugin(file) => {
      let name = file.clone();
      Task::future(tokio::task::spawn_blocking(move || {
        riff_plugins::store::remove(
          &crate::plugins::plugins_dir(),
          &crate::plugins::state_dir(),
          &file,
        )
      }))
      .map(move |result| {
        Message::Plugin(PluginMessage::Removed(
          name.clone(),
          result.unwrap_or_else(|err| Err(err.to_string())),
        ))
      })
    }
    Effect::PickPlugins => pick_dialog().then(|dialog| {
      Task::future(async move {
        let files = dialog
          .add_filter("Rhai", &["rhai"])
          .pick_files()
          .await
          .unwrap_or_default();
        let paths: Vec<PathBuf> = files.iter().map(|file| file.path().to_path_buf()).collect();
        let results = tokio::task::spawn_blocking(move || {
          let dir = crate::plugins::plugins_dir();
          paths
            .iter()
            .map(|path| riff_plugins::store::import(&dir, path))
            .collect()
        })
        .await
        .unwrap_or_default();
        Message::Plugin(PluginMessage::Imported(results))
      })
    }),
    Effect::OpenEditorFile(target) => {
      let opened = target.clone();
      Task::future(tokio::task::spawn_blocking(move || {
        target
          .path()
          .and_then(|path| crate::code_editor::read_capped(&path))
      }))
      .map(move |result| {
        Message::Editor(crate::code_editor::EditorMessage::Opened(
          opened.clone(),
          result.unwrap_or_else(|err| Err(err.to_string())),
        ))
      })
    }
    Effect::SaveEditorFile(target, text) => {
      let saved = target.clone();
      Task::future(tokio::task::spawn_blocking(move || {
        let path = target.path()?;
        crate::code_editor::write_capped(&path, &text)?;
        Ok(crate::code_editor::check(&target, &text))
      }))
      .map(move |result| {
        Message::Editor(crate::code_editor::EditorMessage::Saved(
          saved.clone(),
          result.unwrap_or_else(|err| Err(err.to_string())),
        ))
      })
    }
    Effect::ReadThemeDraft(id) => {
      let read = id.clone();
      Task::future(tokio::task::spawn_blocking(move || {
        crate::code_editor::EditorTarget::Theme(id)
          .path()
          .and_then(|path| crate::code_editor::read_capped(&path))
      }))
      .map(move |result| {
        Message::ThemeEditor(crate::theme_editor::ThemeEditorMessage::Loaded(
          read.clone(),
          result.unwrap_or_else(|err| Err(err.to_string())),
        ))
      })
    }
    Effect::SaveThemeDraft(id, json, is_new) => {
      Task::future(tokio::task::spawn_blocking(move || {
        if is_new {
          return crate::code_editor::write_new_theme(&id, &json).map(|free| (free, Ok(())));
        }
        let path = crate::code_editor::EditorTarget::Theme(id.clone()).path()?;
        Ok((id, crate::code_editor::write_capped(&path, &json)))
      }))
      .map(|result| {
        let (id, saved) = match result {
          Ok(Ok(done)) => done,
          Ok(Err(err)) => (String::new(), Err(err)),
          Err(err) => (String::new(), Err(err.to_string())),
        };
        Message::ThemeEditor(crate::theme_editor::ThemeEditorMessage::Saved(id, saved))
      })
    }
    Effect::OpenPluginsFolder => Task::future(tokio::task::spawn_blocking(|| {
      let dir = crate::plugins::plugins_dir();
      let _ = std::fs::create_dir_all(&dir);
      riff_core::os::open_folder(&dir);
    }))
    .discard(),
    Effect::QueryScale => latest
      .and_then(window::scale_factor)
      .map(Message::ScaleKnown),
    Effect::QueryMaximized => latest
      .and_then(window::is_maximized)
      .map(Message::MaximizedState),
    Effect::ApplyBounds(position, size) => latest
      .and_then(move |id| Task::batch([window::resize(id, size), window::move_to(id, position)])),
    Effect::QueryCloseState => latest.and_then(|id| {
      window::is_maximized(id).then(move |is_maximized| {
        window::mode(id).map(move |mode| Message::CloseStateKnown {
          is_maximized,
          is_full_screen: mode == window::Mode::Fullscreen,
        })
      })
    }),
    Effect::SaveWindowAndExit(state, data) => {
      if let Err(err) = state.write(&paths::window_state_path()) {
        tracing::warn!(error = %err, "écriture de window-state.json");
      }
      if let Some(data) = data {
        if let Err(err) = data.save(&user_data::user_data_path()) {
          tracing::warn!(error = %err, "écriture de user-data.json");
        }
        crate::resume::flush();
      }
      iced::exit()
    }
    Effect::ImportChoice(choice, storage) => {
      let core = Arc::clone(&backend.core);
      Task::future(tokio::task::spawn_blocking(move || {
        let outcome = import_flow::run_choice(
          choice,
          &storage,
          &settings::settings_path(),
          &user_data::user_data_path(),
          core.auth.secrets.as_ref(),
          riff_core::util::now_millis() as i64,
        );
        // Clé d'API Last.fm : reprise seulement si la config du core n'en a pas.
        if let Some(key) = &outcome.lastfm_api_key
          && let Some(merged) = import_flow::with_lastfm_key(&config::read_config(), key)
          && let Err(err) = config::write_config(&merged)
        {
          tracing::warn!(error = %err, "écriture de config.json");
        }
        outcome
      }))
      .map(|result| match result {
        Ok(outcome) => Message::Import(ImportMessage::Done(Box::new(outcome))),
        Err(err) => {
          tracing::warn!(error = %err, "import depuis Riff");
          Message::Import(ImportMessage::Failed)
        }
      })
    }
    Effect::Chrome(action) => chrome_task(action),
    Effect::SaveSettings(settings) => {
      static SETTINGS_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
      let generation = SETTINGS_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
      Task::future(async move {
        tokio::time::sleep(Duration::from_millis(120)).await;
        if SETTINGS_GEN.load(std::sync::atomic::Ordering::Relaxed) != generation {
          return;
        }
        let _ = tokio::task::spawn_blocking(move || {
          if SETTINGS_GEN.load(std::sync::atomic::Ordering::Relaxed) == generation
            && let Err(err) = settings.save(&settings::settings_path())
          {
            tracing::warn!(error = %err, "écriture de native-settings.json");
          }
        })
        .await;
      })
      .discard()
    }
    Effect::ExtractCover(bytes) => Task::future(tokio::task::spawn_blocking(move || {
      cover::source_from_image(&bytes)
    }))
    .map(|result| match result {
      Ok(Ok(source)) => Message::CoverColor(source),
      Ok(Err(err)) => {
        tracing::warn!(error = %err, "pochette illisible");
        Message::NoOp
      }
      Err(err) => {
        tracing::warn!(error = %err, "extraction de la couleur");
        Message::NoOp
      }
    }),
    Effect::FocusOverlay => save_focus_task()
      .chain(focus_task(focusable::unfocus()))
      .chain(scoped_focus(focusable::focus_next())),
    Effect::FocusNext { in_overlay: false } => focus_task(crate::widgets::focus::cycle_next()),
    Effect::FocusPrevious { in_overlay: false } => {
      focus_task(crate::widgets::focus::cycle_previous())
    }
    Effect::FocusNext { in_overlay: true } => scoped_focus(crate::widgets::focus::cycle_next()),
    Effect::FocusPrevious { in_overlay: true } => {
      scoped_focus(crate::widgets::focus::cycle_previous())
    }
    Effect::Unfocus => focus_task(focusable::unfocus()),
    Effect::ToastTimer(id, generation, duration) => Task::future(async move {
      tokio::time::sleep(duration).await;
      Message::ToastTimeout(id, generation, Instant::now())
    }),
    Effect::AudioPlay => {
      backend.engine.play();
      Task::none()
    }
    Effect::AudioPause => {
      backend.engine.pause();
      Task::none()
    }
    Effect::AudioSeek(target) => {
      backend.engine.seek(target);
      Task::none()
    }
    Effect::AudioVolume(volume) => {
      backend.engine.set_volume(volume, false);
      Task::none()
    }
    Effect::AudioFadeOut(seconds) => {
      backend.engine.fade_out(seconds);
      Task::none()
    }
    Effect::AudioFadeIn(seconds) => {
      backend.engine.fade_in(seconds);
      Task::none()
    }
    Effect::SleepAlarm(generation, delay) => Task::future(async move {
      tokio::time::sleep(delay).await;
      Message::AudioPrefs(AudioMessage::SleepDue(generation))
    }),
    Effect::SleepFadeWait(delay) => Task::future(async move {
      tokio::time::sleep(delay).await;
      Message::AudioPrefs(AudioMessage::SleepFaded)
    }),
    Effect::AudioDevice(id) => {
      backend.engine.set_output_device(id);
      Task::none()
    }
    Effect::DevSearch(query, limit) => {
      let backend = Arc::clone(backend);
      Task::future(async move {
        Message::Dev(DevMessage::SearchDone(
          dev_search(&backend, query, limit).await,
        ))
      })
    }
    Effect::ListDevices => Task::future(tokio::task::spawn_blocking(
      riff_audio::Engine::list_devices,
    ))
    .map(|devices| Message::Dev(DevMessage::DevicesLoaded(devices.unwrap_or_default()))),
    Effect::ListThemes => Task::future(tokio::task::spawn_blocking(|| {
      let css: Vec<String> = riff_core::themes::list_themes()
        .iter()
        .filter_map(|theme| {
          theme
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
        })
        .collect();
      file::list_themes(&paths::themes_dir(), &css)
    }))
    .map(|themes| Message::Dev(DevMessage::ThemesLoaded(themes.unwrap_or_default()))),
    Effect::FetchCover(url) => {
      let backend = Arc::clone(backend);
      Task::future(async move { cover_message(fetch_cover(&backend, &url).await) })
    }
    Effect::ReadCoverFile(path) => {
      Task::future(tokio::task::spawn_blocking(move || std::fs::read(path))).map(
        |read| match read {
          Ok(result) => cover_message(result.map_err(|err| err.to_string())),
          Err(err) => cover_message(Err(err.to_string())),
        },
      )
    }
    Effect::PublishState => {
      let backend = Arc::clone(backend);
      Task::future(async move {
        let body = json!({ "source": "--dev", "at": riff_core::util::now_millis() }).to_string();
        if let Err(err) = backend
          .call("/api/state", HashMap::new(), Some(body.into()))
          .await
        {
          tracing::warn!(error = %err, "publication de l'état");
        }
        Message::NoOp
      })
    }
    Effect::RestoreFocus(Some(ordinal)) => focus_task(crate::widgets::focus::focus_nth(ordinal)),
    Effect::RestoreFocus(None) => focus_task(focusable::unfocus()),
    Effect::AudioMix(volume, muted) => {
      backend.engine.set_volume(volume, muted);
      Task::none()
    }
    Effect::NormalizeGain(gain) => {
      backend.engine.set_normalize_gain_db(gain);
      Task::none()
    }
    Effect::AudioLoadSource(source, start) => {
      backend.engine.load(source.0, start, true);
      Task::none()
    }
    Effect::AudioCrossfade(source, seconds) => {
      backend.engine.crossfade_to(source.0, seconds as f32);
      Task::none()
    }
    Effect::AudioSound(settings) => {
      backend.engine.set_settings(*settings);
      Task::none()
    }
    Effect::AudioHq(enabled, settings) => {
      backend.engine.set_hq(enabled, settings);
      Task::none()
    }
    Effect::ListSettingsDevices => Task::future(tokio::task::spawn_blocking(
      riff_audio::Engine::list_devices,
    ))
    .map(|devices| Message::AudioPrefs(AudioMessage::Devices(devices.unwrap_or_default()))),
    Effect::LoadWave { token, track } => {
      let backend = Arc::clone(backend);
      Task::future(async move {
        let result = load_wave(backend, *track).await;
        Message::Download(DownloadMessage::WaveReady(token, result))
      })
    }
    Effect::LoadLocalInfo => {
      let backend = Arc::clone(backend);
      Task::future(tokio::task::spawn_blocking(move || {
        manage::read_local_info(&backend.core)
      }))
      .then(|info| match info {
        Ok(info) => Task::done(Message::Manage(ManageMessage::LocalLoaded(Box::new(info)))),
        Err(_) => Task::none(),
      })
    }
    Effect::OpenUrl(url) => {
      riff_core::os::open_external_url(&url);
      Task::none()
    }
    Effect::SpotifyLogin(client_id) => {
      let core = Arc::clone(&backend.core);
      Task::future(async move {
        let started = riff_core::services::spotify::start_login(&core, &client_id).await;
        Message::Manage(ManageMessage::SpotifyStarted(
          started
            .map(|login| (login.url, login.redirect_uri))
            .map_err(|err| err.to_string()),
        ))
      })
    }
    Effect::GoogleLogin => {
      let core = Arc::clone(&backend.core);
      Task::future(async move {
        let result = match riff_core::services::google::client_id() {
          Some(client_id) => riff_core::services::google::start_login(&core, &client_id)
            .await
            .map(|login| login.url)
            .map_err(|err| err.to_string()),
          None => Err("Google sign-in is not configured in this build: no client ID.".to_string()),
        };
        Message::Manage(ManageMessage::GoogleStarted(result))
      })
    }
    Effect::SaveLastfmKeys { key, secret } => {
      let core = Arc::clone(&backend.core);
      Task::future(tokio::task::spawn_blocking(move || {
        if !key.is_empty() {
          let mut config = riff_core::config::read_config();
          config["lastfmApiKey"] = Value::String(key);
          if let Err(err) = riff_core::config::write_config(&config) {
            tracing::warn!(error = %err, "écriture de config.json");
          }
        }
        if !secret.is_empty()
          && let Err(err) = core
            .auth
            .secrets
            .set(riff_core::secrets::SecretName::LastfmApiSecret, &secret)
        {
          tracing::warn!(error = %err, "enregistrement du secret Last.fm");
          return Message::Manage(ManageMessage::LastfmVaultFailed(LastfmVaultItem::ApiSecret));
        }
        Message::NoOp
      }))
      .map(|result| result.unwrap_or(Message::NoOp))
    }
    Effect::SaveLastfmSession(session) => {
      use riff_core::secrets::SecretName::LastfmSession;
      let saving = session.is_some();
      let result = match session {
        Some(key) => backend.core.auth.secrets.set(LastfmSession, &key),
        None => backend.core.auth.secrets.delete(LastfmSession),
      };
      if let Err(err) = result {
        tracing::warn!(error = %err, "session Last.fm dans le coffre");
        if saving {
          return Task::done(Message::Manage(ManageMessage::LastfmVaultFailed(
            LastfmVaultItem::Session,
          )));
        }
      }
      Task::none()
    }
    Effect::PickCookies => pick_dialog().then(|dialog| {
      Task::future(async move {
        let Some(file) = dialog.add_filter("cookies.txt", &["txt"]).pick_file().await else {
          return Message::NoOp;
        };
        Message::Manage(ManageMessage::CookiesPicked(
          riff_core::services::ytdlp::import_cookies(file.path()),
        ))
      })
    }),
    Effect::BrowserCookies(browser) => Task::future(async move {
      Message::Manage(ManageMessage::CookiesPicked(
        riff_core::services::ytdlp::import_browser_cookies(&browser).await,
      ))
    }),
    Effect::DeleteCookies => {
      if !riff_core::services::ytdlp::delete_cookies() {
        tracing::warn!("suppression de yt-cookies.txt impossible");
      }
      Task::none()
    }
    Effect::PickPlaylistsJson => pick_dialog().then(|dialog| {
      Task::future(async move {
        let Some(file) = dialog.add_filter("JSON", &["json"]).pick_file().await else {
          return Message::Manage(ManageMessage::ImportJsonRead(Ok(None)));
        };
        Message::Manage(ManageMessage::ImportJsonRead(
          std::fs::read_to_string(file.path())
            .map(Some)
            .map_err(|err| err.to_string()),
        ))
      })
    }),
    Effect::PickExportFiles(source) => pick_dialog().then(move |dialog| {
      Task::future(async move {
        let extensions: &[&str] = match source {
          riff_core::import_files::ExportSource::Spotify => &["json"],
          riff_core::import_files::ExportSource::Apple => &["csv", "json"],
        };
        let Some(files) = dialog.add_filter("Export", extensions).pick_files().await else {
          return Message::Manage(ManageMessage::ImportFilesRead(None));
        };
        let paths: Vec<std::path::PathBuf> =
          files.iter().map(|file| file.path().to_path_buf()).collect();
        let read =
          tokio::task::spawn_blocking(move || riff_core::import_files::read_files(source, &paths))
            .await
            .ok();
        Message::Manage(ManageMessage::ImportFilesRead(read.map(Box::new)))
      })
    }),
    Effect::ClearCache => Task::future(tokio::task::spawn_blocking(
      riff_core::maintenance::clear_cache,
    ))
    .map(|_| Message::Manage(ManageMessage::RefreshLocal)),
    Effect::PickDataFolder(kind) => pick_dialog().then(move |dialog| {
      Task::future(async move {
        let Some(folder) = dialog.pick_folder().await else {
          return Message::Manage(ManageMessage::FolderChanged(kind, Ok(String::new())));
        };
        let path = folder.path().to_path_buf();
        let result = tokio::task::spawn_blocking(move || match kind {
          manage::FolderKind::Data { move_data } => manage::change_data_dir(&path, move_data),
          manage::FolderKind::Download => Ok(
            riff_core::config::set_download_dir_config(Some(&path.to_string_lossy()))
              .map_or_else(String::new, |dir| dir.display().to_string()),
          ),
        })
        .await
        .unwrap_or_else(|err| Err(err.to_string()));
        Message::Manage(ManageMessage::FolderChanged(kind, result))
      })
    }),
    Effect::ResetDownloadFolder => {
      let _ = config::set_download_dir_config(None);
      Task::done(Message::Manage(ManageMessage::RefreshLocal))
    }
    Effect::OpenLogs => {
      riff_core::os::open_folder(&manage::logs_dir());
      Task::none()
    }
    Effect::CopyDiagnostics => iced::clipboard::write(riff_core::maintenance::diagnostics(env!(
      "CARGO_PKG_VERSION"
    ))),
    Effect::ExportAccount(payload) => pick_dialog().then(move |dialog| {
      let payload = payload.clone();
      Task::future(async move {
        let name = riff_core::local_media::account_export_name(&payload);
        let Some(file) = dialog
          .set_file_name(name)
          .add_filter("Riff account", &["rfacc"])
          .save_file()
          .await
        else {
          return Message::Manage(ManageMessage::ExportedAccount(Ok(String::new())));
        };
        let written = riff_core::local_media::write_account_export(file.path(), &payload);
        let result = if written["success"] == true {
          Ok(file.path().display().to_string())
        } else {
          Err(
            written["error"]
              .as_str()
              .unwrap_or("Account export failed")
              .to_string(),
          )
        };
        Message::Manage(ManageMessage::ExportedAccount(result))
      })
    }),
    Effect::PickAccount => pick_dialog().then(|dialog| {
      Task::future(async move {
        let Some(file) = dialog
          .add_filter("Riff account", &["rfacc", "json"])
          .pick_file()
          .await
        else {
          return Message::Manage(ManageMessage::AccountRead(Ok(None)));
        };
        Message::Manage(ManageMessage::AccountRead(
          manage::read_account(file.path()).map(Some),
        ))
      })
    }),
    Effect::SetYtDlpAutoUpdate(enabled) => {
      config::set_yt_dlp_auto_update(enabled);
      Task::none()
    }
    Effect::SetMultiInstance(enabled) => Task::future(tokio::task::spawn_blocking(move || {
      manage::write_multi_instance(enabled)
    }))
    .map(|result| {
      Message::Manage(ManageMessage::MultiInstanceSaved(
        result.unwrap_or_else(|_| Err(manage::multi_instance_enabled())),
      ))
    }),
    Effect::SetTranslation(enabled) => {
      backend.core.set_translation_enabled(enabled);
      Task::none()
    }
    Effect::SavePluginConsent(consent) => Task::future(tokio::task::spawn_blocking(move || {
      consent.save(&crate::plugins::consent_path())
    }))
    .then(|result| match result {
      Ok(Ok(())) => Task::none(),
      Ok(Err(err)) => Task::done(Message::ShowToast(
        format!("Plugins: {err}"),
        ToastKind::Error,
        None,
      )),
      Err(err) => Task::done(Message::ShowToast(err.to_string(), ToastKind::Error, None)),
    }),
    Effect::ListSettingsThemes => Task::future(tokio::task::spawn_blocking(|| {
      let css: Vec<String> = riff_core::themes::list_themes()
        .iter()
        .filter_map(|theme| {
          theme
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
        })
        .collect();
      file::list_themes(&paths::themes_dir(), &css)
    }))
    .map(|themes| Message::Settings(SettingsMessage::ThemesListed(themes.unwrap_or_default()))),
    Effect::PublishAppState(state) => {
      let backend = Arc::clone(backend);
      Task::future(async move {
        if let Err(err) = backend
          .call("/api/state", HashMap::new(), Some(state.to_string().into()))
          .await
        {
          tracing::warn!(error = %err, "publication de l'état");
        }
        Message::NoOp
      })
    }
    Effect::ResolveTrack {
      token,
      track,
      options,
      start,
      crossfade,
    } => {
      let backend = Arc::clone(backend);
      Task::future(async move {
        let result = tokio::time::timeout(
          stream::RESOLVE_TIMEOUT,
          stream::resolve_source(backend, *track, options),
        )
        .await
        .unwrap_or_else(|_| Err("Stream request timed out".to_string()));
        Message::Player(PlayerMessage::Resolved {
          token,
          start,
          crossfade,
          result,
        })
      })
    }
    Effect::AbortSearch
    | Effect::AbortDownload
    | Effect::RunDownload(_)
    | Effect::CoreJson { .. } => Task::none(),
    Effect::ScheduleSave(generation) => Task::future(async move {
      tokio::time::sleep(SAVE_DELAY).await;
      Message::Ui(UiMessage::SaveDue(generation))
    }),
    Effect::WriteUserData(data) => Task::future(tokio::task::spawn_blocking(move || {
      if let Err(err) = data.save(&user_data::user_data_path()) {
        tracing::warn!(error = %err, "écriture de user-data.json");
      }
    }))
    .discard(),
    Effect::SaveResume(_) | Effect::SaveResumePosition(_) if !crate::resume::installed() => {
      Task::none()
    }
    Effect::SaveResume(state) => {
      crate::resume::set_state(*state);
      Task::future(tokio::task::spawn_blocking(crate::resume::flush)).discard()
    }
    Effect::SaveResumePosition(position) => {
      crate::resume::set_position(position);
      Task::future(tokio::task::spawn_blocking(crate::resume::flush)).discard()
    }
    Effect::ResumeRetry(token, delay) => Task::future(async move {
      tokio::time::sleep(delay).await;
      Message::Player(PlayerMessage::ResumeRetry(token))
    }),
    Effect::Toast(text, kind) => Task::done(Message::ShowToast(text, kind, None)),
    Effect::PrepareAutoplay { artists, payload } => {
      let backend = Arc::clone(backend);
      Task::future(async move {
        Message::Player(PlayerMessage::AutoplayReady(
          autoplay_candidates(backend, artists, payload).await,
        ))
      })
    }
    Effect::GenerateMixes { day, jobs } => {
      let backend = Arc::clone(backend);
      let ready = iced::futures::stream::iter(jobs).then(move |job| {
        let (backend, day) = (Arc::clone(&backend), day.clone());
        async move {
          let tracks = match backend
            .call("/api/vibe", HashMap::new(), Some(job.body.into()))
            .await
          {
            Ok(Reply::Json { body, .. }) => tracks_from(&body["tracks"]),
            _ => Vec::new(),
          };
          Message::Ui(UiMessage::MixReady {
            day,
            id: job.id,
            title: job.title,
            artists: job.artists,
            tracks,
          })
        }
      });
      Task::stream(ready.chain(iced::futures::stream::once(async {
        Message::Ui(UiMessage::MixesDone)
      })))
    }
    Effect::FocusId(id) => iced::widget::operation::focus(id),
    Effect::CopyText(text) => iced::clipboard::write(text),
    Effect::PickFiles => pick_dialog().then(|dialog| {
      Task::future(async move {
        let files = dialog
          .add_filter("Audio", AUDIO_FILTER)
          .pick_files()
          .await
          .unwrap_or_default();
        Message::Ui(UiMessage::LocalPicked(
          files.iter().map(|file| file.path().to_path_buf()).collect(),
          false,
        ))
      })
    }),
    Effect::PickFolder => pick_dialog().then(|dialog| {
      Task::future(async move {
        let folder = dialog.pick_folder().await;
        Message::Ui(UiMessage::LocalPicked(
          folder
            .iter()
            .map(|folder| folder.path().to_path_buf())
            .collect(),
          true,
        ))
      })
    }),
    Effect::PickPlaylistCover(id) => pick_dialog().then(move |dialog| {
      let id = id.clone();
      Task::future(async move {
        let file = dialog
          .add_filter("Image", &["png", "jpg", "jpeg"])
          .pick_file()
          .await;
        Message::Ui(UiMessage::PlaylistCoverPicked(
          id,
          file.map(|file| file.path().to_path_buf()),
        ))
      })
    }),
    Effect::PickFavoritesCover => pick_dialog().then(|dialog| {
      Task::future(async move {
        let file = dialog
          .add_filter("Image", &["png", "jpg", "jpeg"])
          .pick_file()
          .await;
        Message::Ui(UiMessage::FavoritesCoverPicked(
          file.map(|file| file.path().to_path_buf()),
        ))
      })
    }),
    Effect::PickMedia(target) => pick_dialog().then(move |dialog| {
      Task::future(async move {
        let file = dialog
          .add_filter("Image", &["png", "jpg", "jpeg"])
          .pick_file()
          .await;
        Message::Settings(SettingsMessage::Visual(VisualMessage::MediaPicked(
          target,
          file.map(|file| file.path().to_path_buf()),
        )))
      })
    }),
    Effect::ExportPlaylist { name, content } => pick_dialog().then(move |dialog| {
      let (name, content) = (name.clone(), content.clone());
      Task::future(async move {
        let Some(file) = dialog
          .set_file_name(name)
          .add_filter("JSON", &["json"])
          .save_file()
          .await
        else {
          return Message::NoOp;
        };
        let written = riff_core::local_media::write_playlists_export(file.path(), &content);
        let result = if written["success"] == true {
          Ok(file.path().display().to_string())
        } else {
          Err(
            written["error"]
              .as_str()
              .unwrap_or("Export failed")
              .to_string(),
          )
        };
        Message::Ui(UiMessage::PlaylistExported(result))
      })
    }),
    Effect::ScanLocal(paths, folders) => Task::future(tokio::task::spawn_blocking(move || {
      if folders {
        riff_core::local_media::local_folders_metadata(paths)
      } else {
        riff_core::local_media::local_files_metadata(paths)
      }
    }))
    .map(|found| {
      Message::Ui(UiMessage::LocalFound(tracks_from(&Value::Array(
        found.unwrap_or_default(),
      ))))
    }),
    Effect::ScanDropped(paths) => Task::future(tokio::task::spawn_blocking(move || {
      let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) =
        paths.into_iter().partition(|path| path.is_dir());
      let files = files
        .into_iter()
        .filter(|path| riff_core::local_media::is_audio_file(path))
        .collect();
      let mut found = riff_core::local_media::local_files_metadata(files);
      found.extend(riff_core::local_media::local_folders_metadata(folders));
      found
    }))
    .map(|found| {
      Message::Ui(UiMessage::LocalFound(tracks_from(&Value::Array(
        found.unwrap_or_default(),
      ))))
    }),
    Effect::RescanLocal(paths) => Task::future(tokio::task::spawn_blocking(move || {
      riff_core::local_media::rescan_local_metadata(paths)
    }))
    .map(|found| {
      Message::Ui(UiMessage::LocalRescanned(tracks_from(&Value::Array(
        found.unwrap_or_default(),
      ))))
    }),
    Effect::CheckUpdate => Task::future(updates::check_latest()).map(Message::UpdateChecked),
    // Après `Launched`, l'application se ferme (chemin de `Quitter`) pour que l'installeur remplace le binaire.
    Effect::InstallUpdate(release) => {
      Task::stream(update_install::run(release).flat_map(|event| {
        let launched = event == update_install::InstallEvent::Launched;
        iced::futures::stream::iter(
          std::iter::once(Message::UpdateInstallEvent(event))
            .chain(launched.then_some(Message::System(SystemEvent::Quit))),
        )
      }))
    }
    Effect::EnterMiniWindow => latest.and_then(|id| {
      Task::batch([
        window::maximize(id, false),
        window::set_min_size(id, Some(MINI_MIN)),
        window::resize(id, MINI_SIZE),
        window::set_level(id, window::Level::AlwaysOnTop),
      ])
    }),
    Effect::ResizeMini(min, size) => latest.and_then(move |id| {
      Task::batch([
        window::set_min_size(id, Some(min)),
        window::resize(id, size),
      ])
    }),
    Effect::LeaveMiniWindow(position, size, maximized) => latest.and_then(move |id| {
      Task::batch([
        window::set_level(id, window::Level::Normal),
        window::set_min_size(id, Some(Size::new(MIN_WIDTH as f32, MIN_HEIGHT as f32))),
        window::resize(id, size),
        window::move_to(id, position),
        window::maximize(id, maximized),
      ])
    }),
    Effect::InstallLauncher => Task::future(tokio::task::spawn_blocking(
      crate::system::launcher::install,
    ))
    .map(|result| {
      match result
        .map_err(|error| error.to_string())
        .and_then(|inner| inner)
      {
        Ok(path) => Message::ShowToast(
          path.display().to_string(),
          ToastKind::Success,
          Some(Duration::from_secs(4)),
        ),
        Err(reason) => Message::ShowToast(reason, ToastKind::Error, None),
      }
    }),
    Effect::UpdateYtDlp => {
      let core = Arc::clone(&backend.core);
      Task::future(riff_core::services::tools::update_yt_dlp_if_managed(core)).discard()
    }
    Effect::PanelControlsTimer(generation, delay) => Task::future(async move {
      tokio::time::sleep(delay).await;
      Message::Ui(UiMessage::PanelControlsDue(generation))
    }),
    Effect::DropTimer(generation) => Task::future(async move {
      tokio::time::sleep(DROP_DELAY).await;
      Message::Ui(UiMessage::DropFlush(generation))
    }),
    Effect::AudioLoad(path) => {
      backend
        .engine
        .load(Source::File(path), Duration::ZERO, true);
      Task::none()
    }
    Effect::LoadLanguage(lang) => {
      Task::future(load_language(Arc::clone(backend), lang)).map(Message::LanguageLoaded)
    }
    Effect::LoadThemeFile(id) => Task::future(async move {
      let loaded = file::load(&paths::themes_dir(), &id).map(Box::new);
      Message::ThemeFileLoaded(id, loaded)
    }),
  }
}

fn chrome_task(action: chrome::Action) -> Task<Message> {
  let latest = window::latest();
  match action {
    chrome::Action::Drag => latest.and_then(window::drag),
    chrome::Action::Resize(direction) => {
      latest.and_then(move |id| window::drag_resize(id, direction))
    }
    chrome::Action::ToggleMaximize => latest.and_then(window::toggle_maximize),
    chrome::Action::Minimize => latest.and_then(|id| window::minimize(id, true)),
    chrome::Action::Close => Task::none(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::theme::{ThemeSource, contrast};
  use crate::widgets::menu::MenuItem;

  /// Message d'un raccourci, sans l'enveloppe `Shortcut` des lettres.
  fn pressed(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Option<Message> {
    match shortcut(&key, modifiers) {
      Some(Message::Shortcut(message)) => Some(*message),
      other => other,
    }
  }

  /// État touché par les six lettres.
  fn letter_state(app: &App) -> String {
    format!(
      "{} {} {} {} {} {}",
      app.data.muted,
      app.player.shuffle,
      app.player.repeat,
      app.lyrics.panel_open,
      app.lyrics.tvk_open,
      app.window.mini.is_some()
    )
  }

  #[test]
  fn the_compact_mode_keeps_the_main_window_on_top() {
    assert!(main_on_top(true, false));
    assert!(main_on_top(false, true));
    assert!(!main_on_top(false, false));
    let mut app = app();
    app.window.mini = Some(crate::window::PreMini {
      bounds: crate::window::WindowBounds::default(),
      is_maximized: false,
    });
    for _ in 0..2 {
      let effects = app.update(Message::ToggleMainAlwaysOnTop);
      assert!(!effects.contains(&Effect::MainLevel(false)));
      assert!(effects.contains(&Effect::MainLevel(true)));
    }
  }

  #[test]
  fn the_tray_toggle_flips_the_main_window_level_and_saves() {
    let mut app = app();
    assert!(!app.settings.main_always_on_top);
    let on = app.update(Message::ToggleMainAlwaysOnTop);
    assert!(app.settings.main_always_on_top);
    assert!(on.contains(&Effect::MainLevel(true)));
    assert!(on.iter().any(|effect| matches!(
      effect,
      Effect::SaveSettings(settings) if settings.main_always_on_top
    )));
    let off = app.update(Message::ToggleMainAlwaysOnTop);
    assert!(!app.settings.main_always_on_top);
    assert!(off.contains(&Effect::MainLevel(false)));
  }

  #[test]
  fn the_always_on_top_setting_maps_to_the_window_level() {
    assert_eq!(mini_level(true), window::Level::AlwaysOnTop);
    assert_eq!(mini_level(false), window::Level::Normal);
  }

  fn app_playing() -> App {
    let mut app = app();
    app.player.current = Some(crate::user_data::Track {
      id: Some("a".to_string()),
      title: Some("Titre".to_string()),
      ..crate::user_data::Track::default()
    });
    app.mark_resume_saved();
    app
  }

  #[test]
  fn the_six_letters_act_without_a_dialog_box() {
    use keyboard::Modifiers as M;
    for key in ["m", "h", "r", "l", "f", "p"] {
      let mut app = app_playing();
      let before = letter_state(&app);
      let message = shortcut(&letter(key), M::empty()).expect(key);
      let effects = app.update(message);
      assert!(
        letter_state(&app) != before || !effects.is_empty(),
        "{key} sans effet"
      );
    }
  }

  #[test]
  fn the_six_letters_do_nothing_under_a_dialog_box() {
    use keyboard::Modifiers as M;
    let openers: [fn(&mut App); 2] = [
      |app| {
        app.update(Message::OpenDialog(sample_dialog(), None));
      },
      |app| {
        app.open_modal(crate::ui::Modal::Audio);
      },
    ];
    for open in openers {
      for key in ["m", "h", "r", "l", "f", "p"] {
        let mut app = app_playing();
        open(&mut app);
        let before = letter_state(&app);
        let message = shortcut(&letter(key), M::empty()).expect(key);
        let effects = app.update(message);
        assert_eq!(letter_state(&app), before, "{key} sous une boîte");
        assert!(effects.is_empty(), "{key} sous une boîte : {effects:?}");
      }
    }
  }

  #[test]
  fn the_other_shortcuts_are_unchanged_under_a_dialog_box() {
    use keyboard::Modifiers as M;
    assert!(matches!(
      shortcut(&named(key::Named::Space), M::empty()),
      Some(Message::Player(PlayerMessage::TogglePlay))
    ));
    assert!(matches!(
      shortcut(&named(key::Named::Tab), M::empty()),
      Some(Message::FocusMove { back: false })
    ));
    assert!(matches!(
      shortcut(&letter("k"), M::CTRL),
      Some(Message::Ui(UiMessage::FocusSearch))
    ));
    let mut app = app_playing();
    app.update(Message::OpenDialog(sample_dialog(), None));
    app.update(Message::Escape);
    assert!(app.dialog.is_none(), "Échap ferme toujours la boîte");
  }

  fn named(key: key::Named) -> keyboard::Key {
    keyboard::Key::Named(key)
  }

  fn letter(text: &str) -> keyboard::Key {
    keyboard::Key::Character(text.into())
  }

  #[test]
  fn a_settings_scene_opens_the_page_on_its_category() {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    apply_scene(
      &mut app,
      "settings-banners",
      std::path::Path::new("scene.mp3"),
    );
    assert_eq!(app.screen, Screen::Settings);
    assert_eq!(app.settings.settings_cat, "banners");
    apply_scene(&mut app, "settings", std::path::Path::new("scene.mp3"));
    assert_eq!(
      app.settings.settings_cat, "banners",
      "sans catégorie, celle enregistrée reste"
    );
    apply_scene(
      &mut app,
      "settings-find-lyrics",
      std::path::Path::new("scene.mp3"),
    );
    assert_eq!(app.ui.prefs.query, "lyrics");
    assert_eq!(app.settings.settings_cat, "banners");
  }

  #[test]
  fn the_shortcuts_of_riff_are_mapped() {
    use keyboard::Modifiers as M;
    let player = |key, modifiers| match pressed(key, modifiers) {
      Some(Message::Player(message)) => format!("{message:?}"),
      other => format!("{other:?}"),
    };
    assert_eq!(player(named(key::Named::Space), M::empty()), "TogglePlay");
    assert_eq!(player(named(key::Named::ArrowRight), M::CTRL), "Next");
    assert_eq!(player(named(key::Named::ArrowLeft), M::CTRL), "Prev");
    assert_eq!(
      player(named(key::Named::ArrowRight), M::SHIFT),
      "SeekBy(5.0)"
    );
    assert_eq!(
      player(named(key::Named::ArrowLeft), M::SHIFT),
      "SeekBy(-5.0)"
    );
    assert_eq!(player(named(key::Named::ArrowUp), M::CTRL), "VolumeBy(1.0)");
    assert_eq!(
      player(named(key::Named::ArrowDown), M::CTRL),
      "VolumeBy(-1.0)"
    );
    assert_eq!(player(letter("m"), M::empty()), "ToggleMute");
    assert_eq!(player(letter("H"), M::empty()), "ToggleShuffle");
    assert_eq!(player(letter("r"), M::empty()), "ToggleRepeat");
    assert!(matches!(
      pressed(letter("k"), M::CTRL),
      Some(Message::Ui(UiMessage::FocusSearch))
    ));
    assert!(pressed(letter("k"), M::empty()).is_none());
    assert!(pressed(letter("m"), M::CTRL).is_none());
    assert!(pressed(named(key::Named::ArrowRight), M::empty()).is_none());
    assert!(matches!(
      pressed(named(key::Named::Escape), M::empty()),
      Some(Message::Escape)
    ));
    assert!(matches!(
      pressed(named(key::Named::ArrowLeft), M::ALT),
      Some(Message::Back)
    ));
  }

  #[test]
  fn l_f_and_p_open_the_lyrics_the_karaoke_and_the_compact_mode() {
    use keyboard::Modifiers as M;
    assert!(matches!(
      pressed(letter("l"), M::empty()),
      Some(Message::Lyrics(LyricsMessage::TogglePanel))
    ));
    assert!(matches!(
      pressed(letter("F"), M::empty()),
      Some(Message::Lyrics(LyricsMessage::OpenTvk))
    ));
    assert!(matches!(
      pressed(letter("p"), M::empty()),
      Some(Message::ToggleMiniMode)
    ));
    for key in ["l", "f", "p"] {
      assert!(pressed(letter(key), M::CTRL).is_none(), "Ctrl+{key}");
    }
  }

  #[test]
  fn l_f_and_p_do_nothing_when_a_text_field_has_taken_the_key() {
    let key_event = |key: &str| {
      Event::Keyboard(keyboard::Event::KeyPressed {
        key: letter(key),
        modified_key: letter(key),
        physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
      })
    };
    for key in ["l", "f", "p"] {
      assert!(
        window_event(
          key_event(key),
          event::Status::Captured,
          window::Id::unique()
        )
        .is_none(),
        "{key} dans un champ"
      );
      assert!(
        window_event(key_event(key), event::Status::Ignored, window::Id::unique()).is_some(),
        "{key} hors champ"
      );
    }
  }

  #[test]
  fn p_leaves_an_open_mini_lyrics_window_as_it_is() {
    let mut app = app();
    app.settings.visual.mini_lyrics = true;
    app.lyrics.mini_open = true;
    let effects = app.update(Message::ToggleMiniMode);
    assert_eq!(effects, vec![Effect::EnterMiniWindow]);
    assert!(app.lyrics.mini_open && app.settings.visual.mini_lyrics);
    let effects = app.update(Message::ToggleMiniMode);
    assert!(
      matches!(effects.as_slice(), [Effect::LeaveMiniWindow(..)]),
      "{effects:?}"
    );
    assert!(app.lyrics.mini_open && app.settings.visual.mini_lyrics);
  }

  #[test]
  fn the_ticker_has_a_size_or_iced_drops_it() {
    assert!(!ticker().as_widget().size_hint().is_void());
  }

  #[test]
  fn start_args_read_a_scene() {
    let args = StartArgs::parse(
      [
        "--scene",
        "b",
        "--scene-file",
        "f.mp3",
        "--scene-log",
        "l.csv",
        "--scene-seconds",
        "72",
        "--scene-pause",
        "20",
      ]
      .map(String::from),
    );
    assert_eq!(
      args.scene,
      Some(SceneArgs {
        name: "b".into(),
        file: "f.mp3".into(),
        log: "l.csv".into(),
        seconds: 72,
        pause_after: Some(20),
        unthrottled: false
      })
    );
    assert_eq!(StartArgs::parse(["--dev".to_string()]).scene, None);
  }

  #[test]
  fn window_events_are_tagged_with_their_window() {
    let id = window::Id::unique();
    let message = window_event(
      Event::Window(window::Event::CloseRequested),
      event::Status::Ignored,
      id,
    );
    assert!(
      matches!(message, Some(Message::ForWindow(from, inner)) if from == id && matches!(*inner, Message::CloseRequested))
    );
  }

  #[test]
  fn the_mini_window_only_listens_to_its_own_bounds_and_close() {
    assert!(matches!(
      mini_message(Message::CloseRequested),
      Some(Message::Lyrics(LyricsMessage::MiniClosed))
    ));
    assert!(matches!(
      mini_message(Message::WindowResized(Size::new(1.0, 2.0))),
      Some(Message::Lyrics(LyricsMessage::MiniResized(_)))
    ));
    assert!(
      mini_message(Message::WindowMoved(Point::new(-32000.0, -32000.0))).is_none(),
      "fenêtre réduite"
    );
    assert!(mini_message(Message::WindowRescaled(2.0)).is_none());
  }

  fn saved_window_settings() -> window::Settings {
    PersistedWindowState {
      is_maximized: true,
      ..PersistedWindowState::default()
    }
    .settings()
  }

  #[test]
  fn a_tray_launch_hides_the_window_and_holds_back_the_mini_window_and_the_splash() {
    let mut settings = saved_window_settings();
    let plan = boot_plan(true, true, true, &mut settings);
    assert!(plan.start_hidden && !settings.visible && !settings.maximized);
    assert!(!plan.open_mini && !plan.splash);
    assert_eq!(
      plan.deferred,
      crate::window::DeferredMode {
        maximized: true,
        fullscreen: false,
        mini_lyrics: true
      }
    );
  }

  #[test]
  fn a_normal_launch_or_a_missing_tray_shows_the_window() {
    for (tray_arg, tray_expected) in [(false, true), (true, false), (false, false)] {
      let mut settings = saved_window_settings();
      let plan = boot_plan(tray_arg, tray_expected, true, &mut settings);
      assert!(!plan.start_hidden && settings.visible && settings.maximized);
      assert!(plan.open_mini && plan.splash);
      assert_eq!(plan.deferred, crate::window::DeferredMode::default());
    }
    let mut settings = saved_window_settings();
    assert!(!boot_plan(false, true, false, &mut settings).open_mini);
  }

  #[test]
  fn quitting_a_tray_session_keeps_the_saved_maximized_and_fullscreen_states() {
    let deferred = crate::window::DeferredMode {
      maximized: true,
      fullscreen: false,
      mini_lyrics: false,
    };
    let known = |is_maximized, is_full_screen| Message::CloseStateKnown {
      is_maximized,
      is_full_screen,
    };
    let flags = |message: Message| match message {
      Message::CloseStateKnown {
        is_maximized,
        is_full_screen,
      } => Some((is_maximized, is_full_screen)),
      _ => None,
    };
    assert_eq!(
      flags(with_deferred_close_state(deferred, known(false, false))),
      Some((true, false))
    );
    assert_eq!(
      flags(with_deferred_close_state(
        crate::window::DeferredMode::default(),
        known(false, false)
      )),
      Some((false, false))
    );
    assert!(flags(with_deferred_close_state(deferred, Message::CloseRequested)).is_none());
    let mut app = app();
    let effects = app.update(with_deferred_close_state(deferred, known(false, false)));
    let [Effect::SaveWindowAndExit(state, _)] = effects.as_slice() else {
      panic!("{effects:?}")
    };
    assert!(state.is_maximized && !state.is_full_screen);
  }

  #[test]
  fn start_args_read_the_tray_launch_and_keep_the_other_arguments() {
    let args = |list: &[&str]| StartArgs::parse(list.iter().map(|arg| (*arg).to_string()));
    assert!(!args(&[]).tray);
    assert!(args(&["--tray"]).tray);
    let both = args(&["--tray", "--dev", "a.mp3"]);
    assert!(both.tray && both.dev);
    assert_eq!(both.files, vec![PathBuf::from("a.mp3")]);
  }

  #[test]
  fn start_args_read_the_measures() {
    let args =
      StartArgs::parse(["--bench-scroll", "a.csv", "--latency-log", "b.csv"].map(str::to_string));
    assert_eq!(args.bench_scroll, Some(PathBuf::from("a.csv")));
    assert_eq!(args.latency_log, Some(PathBuf::from("b.csv")));
  }

  #[test]
  fn start_args_read_dev_and_the_startup_probe() {
    let args = |list: &[&str]| StartArgs::parse(list.iter().map(|arg| (*arg).to_string()));
    assert_eq!(args(&[]), StartArgs::default());
    assert!(args(&["--dev"]).dev);
    assert_eq!(
      args(&["--t0", "12", "--startup-log", "a.csv"]).startup_probe,
      Some((12, PathBuf::from("a.csv")))
    );
    assert_eq!(
      args(&["--t0", "pas un nombre", "--startup-log", "a.csv"]).startup_probe,
      None
    );
    assert_eq!(args(&["--t0", "12"]).startup_probe, None);
  }

  fn app() -> App {
    App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
  }

  fn sample_menu() -> Box<Menu<Message>> {
    Box::new(Menu {
      position: Point::new(10.0, 20.0),
      items: vec![MenuItem::new(
        "Aller aux albums",
        Message::Navigate(Screen::Albums),
      )],
    })
  }

  fn sample_dialog() -> Dialog {
    Dialog {
      title: "Titre".to_string(),
      body: "Texte".to_string(),
      cancel: "Non".to_string(),
      confirm: Some("Oui".to_string()),
    }
  }

  #[test]
  fn opening_a_menu_moves_the_focus_into_it() {
    let mut app = app();
    assert_eq!(
      app.update(Message::OpenMenu(sample_menu())),
      vec![Effect::FocusOverlay]
    );
    assert!(app.menu.is_some());
  }

  #[test]
  fn escape_closes_the_menu_first() {
    let mut app = app();
    app.update(Message::OpenDialog(sample_dialog(), None));
    app.update(Message::OpenMenu(sample_menu()));
    assert_eq!(
      app.update(Message::Escape),
      vec![Effect::RestoreFocus(None)]
    );
    assert!(app.menu.is_none());
    assert!(app.dialog.is_some());
  }

  #[test]
  fn a_click_outside_closes_the_menu() {
    let mut app = app();
    app.update(Message::OpenMenu(sample_menu()));
    app.update(Message::CloseMenu);
    assert!(app.menu.is_none());
  }

  #[test]
  fn choosing_an_entry_closes_the_menu_and_runs_its_message() {
    let mut app = app();
    app.update(Message::OpenMenu(sample_menu()));
    let effects = app.update(Message::MenuChoose(Box::new(Message::Navigate(
      Screen::Albums,
    ))));
    assert!(app.menu.is_none());
    assert_eq!(app.screen, Screen::Albums);
    assert_eq!(
      effects,
      vec![
        Effect::RestoreFocus(None),
        Effect::SaveSettings(Box::new(app.settings.clone()))
      ]
    );
  }

  #[test]
  fn opening_a_dialog_moves_the_focus_into_it() {
    let mut app = app();
    assert_eq!(
      app.update(Message::OpenDialog(sample_dialog(), None)),
      vec![Effect::FocusOverlay]
    );
    assert!(app.dialog.is_some());
  }

  #[test]
  fn escape_closes_the_dialog_without_confirming() {
    let mut app = app();
    app.update(Message::OpenDialog(
      sample_dialog(),
      Some(Box::new(Message::Navigate(Screen::Albums))),
    ));
    assert_eq!(
      app.update(Message::Escape),
      vec![Effect::RestoreFocus(None)]
    );
    assert!(app.dialog.is_none());
    assert_eq!(app.screen, Screen::Home);
  }

  #[test]
  fn confirming_a_dialog_closes_it_and_runs_its_message() {
    let mut app = app();
    app.update(Message::OpenDialog(
      sample_dialog(),
      Some(Box::new(Message::Navigate(Screen::Albums))),
    ));
    app.update(Message::ConfirmDialog);
    assert!(app.dialog.is_none());
    assert_eq!(app.screen, Screen::Albums);
  }

  #[test]
  fn closing_a_dialog_does_not_run_its_message() {
    let mut app = app();
    app.update(Message::OpenDialog(
      sample_dialog(),
      Some(Box::new(Message::Navigate(Screen::Albums))),
    ));
    app.update(Message::CloseDialog);
    app.update(Message::ConfirmDialog);
    assert_eq!(app.screen, Screen::Home);
  }

  #[test]
  fn tab_stays_inside_an_open_overlay() {
    let mut app = app();
    let forward = Message::FocusMove { back: false };
    let backward = Message::FocusMove { back: true };
    assert_eq!(
      app.update(forward.clone()),
      vec![Effect::FocusNext { in_overlay: false }]
    );
    assert_eq!(
      app.update(backward.clone()),
      vec![Effect::FocusPrevious { in_overlay: false }]
    );
    app.update(Message::OpenDialog(sample_dialog(), None));
    assert_eq!(
      app.update(forward),
      vec![Effect::FocusNext { in_overlay: true }]
    );
    assert_eq!(
      app.update(backward),
      vec![Effect::FocusPrevious { in_overlay: true }]
    );
  }

  #[test]
  fn escape_dismisses_the_toast_after_the_overlays() {
    let mut app = app();
    app.update(Message::ShowToast(
      "note".to_string(),
      ToastKind::Info,
      None,
    ));
    assert!(app.toasts.active.is_some());
    app.update(Message::Escape);
    assert!(app.toasts.active.is_none());
  }

  #[test]
  fn escape_without_anything_open_removes_the_focus() {
    assert_eq!(app().update(Message::Escape), vec![Effect::Unfocus]);
  }

  #[test]
  fn a_toast_starts_its_timer_and_the_next_one_follows() {
    let mut app = app();
    let effects = app.update(Message::ShowToast("un".to_string(), ToastKind::Info, None));
    let first = app.toasts.active.clone().unwrap();
    assert_eq!(
      effects,
      vec![Effect::ToastTimer(
        first.id,
        first.timer_gen,
        first.duration
      )]
    );
    assert_eq!(
      app.update(Message::ShowToast(
        "deux".to_string(),
        ToastKind::Error,
        None
      )),
      Vec::new()
    );
    // La fin de durée lance la sortie ; la suivante n'apparaît qu'une fois la sortie finie (200 ms).
    assert_eq!(app.update(Message::ToastExpired(first.id)), Vec::new());
    assert_eq!(app.toasts.active.as_ref().map(|t| t.id), Some(first.id));
    let left = app.now;
    assert_eq!(
      app.toast_frame(left + Duration::from_millis(199)),
      Vec::new()
    );
    let effects = app.toast_frame(left + Duration::from_millis(200));
    let second = app.toasts.active.clone().unwrap();
    assert_eq!(second.message, "Deux");
    assert_eq!(
      effects,
      vec![Effect::ToastTimer(
        second.id,
        second.timer_gen,
        second.duration
      )]
    );
    assert_eq!(second.shown_at, Some(left + Duration::from_millis(200)));
  }

  #[test]
  fn hovering_a_toast_pauses_its_timer_and_leaving_resumes_it() {
    let mut app = app();
    let effects = app.update(Message::ShowToast(
      "un".to_string(),
      ToastKind::Info,
      Some(Duration::from_secs(3)),
    ));
    let toast = app.toasts.active.clone().unwrap();
    let start = toast.shown_at.unwrap();
    assert_eq!(
      effects,
      vec![Effect::ToastTimer(
        toast.id,
        toast.timer_gen,
        Duration::from_secs(3)
      )]
    );
    let at = |ms: u64| start + Duration::from_millis(ms);
    assert_eq!(
      app.update(Message::ToastHover(toast.id, true, at(1000))),
      Vec::new()
    );
    // La minuterie de départ arrive pendant le survol : la notification reste.
    app.update(Message::ToastTimeout(toast.id, toast.timer_gen, at(3000)));
    assert_eq!(app.toasts.active.as_ref().unwrap().leaving_at, None);
    let effects = app.update(Message::ToastHover(toast.id, false, at(10_000)));
    let resumed = app.toasts.active.clone().unwrap();
    assert_eq!(
      effects,
      vec![Effect::ToastTimer(
        resumed.id,
        resumed.timer_gen,
        Duration::from_secs(2)
      )]
    );
    app.update(Message::ToastTimeout(
      resumed.id,
      resumed.timer_gen,
      at(12_000),
    ));
    assert_eq!(
      app.toasts.active.as_ref().unwrap().leaving_at,
      Some(at(12_000))
    );
  }

  #[test]
  fn a_hover_during_the_exit_changes_nothing_and_a_hovered_toast_asks_for_no_frames() {
    let mut app = app();
    app.update(Message::ShowToast("un".to_string(), ToastKind::Info, None));
    let toast = app.toasts.active.clone().unwrap();
    let start = toast.shown_at.unwrap();
    app.now = start + Duration::from_secs(1);
    app.update(Message::ToastHover(toast.id, true, app.now));
    assert!(
      !app.animation_needed(),
      "survolée et posée : le banc d'images s'arrête"
    );
    app.update(Message::ToastHover(toast.id, false, app.now));
    let generation = app.toasts.active.as_ref().unwrap().timer_gen;
    let leaving = start + Duration::from_secs(4);
    app.update(Message::ToastTimeout(toast.id, generation, leaving));
    let before = app.toasts.clone();
    assert_eq!(
      app.update(Message::ToastHover(toast.id, true, leaving)),
      Vec::new()
    );
    assert_eq!(
      app.update(Message::ToastHover(toast.id, false, leaving)),
      Vec::new()
    );
    assert_eq!(app.toasts, before);
    app.now = leaving;
    assert!(app.animation_needed());
  }

  #[test]
  fn a_toast_asks_for_frames_until_it_has_settled_and_again_while_it_leaves() {
    let mut app = app();
    assert!(!app.animation_needed());
    app.update(Message::ShowToast("un".to_string(), ToastKind::Info, None));
    let shown = app.toasts.active.as_ref().and_then(|t| t.shown_at).unwrap();
    assert!(app.animation_needed());
    app.now = shown + Duration::from_millis(340);
    assert!(!app.animation_needed(), "posée : l'abonnement s'arrête");
    let id = app.toasts.active.as_ref().map(|t| t.id).unwrap();
    app.update(Message::ToastExpired(id));
    assert!(app.animation_needed(), "sortie en cours");
    app.toast_frame(app.now + Duration::from_millis(200));
    assert!(app.toasts.active.is_none() && !app.animation_needed());
  }

  #[test]
  fn the_spinner_toast_has_no_timer_and_keeps_asking_for_frames() {
    let mut app = app();
    let effects = app.update(Message::ShowToast(String::new(), ToastKind::Spinner, None));
    assert_eq!(effects, Vec::new());
    app.now += Duration::from_secs(60);
    assert!(app.animation_needed());
  }

  #[test]
  fn the_toast_queue_overflows_like_show_toast() {
    let mut app = app();
    for index in 0..15 {
      app.update(Message::ShowToast(
        format!("n{index}"),
        ToastKind::Info,
        None,
      ));
    }
    assert_eq!(app.toasts.queue.len(), toast::MAX_QUEUED);
    assert_eq!(
      app.toasts.queue.front().map(|t| t.message.as_str()),
      Some("N5")
    );
  }

  #[test]
  fn toasts_are_translated() {
    let cache = std::env::temp_dir().join("riff-app-i18n-absent");
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::load("ru", &cache),
    );
    app.update(Message::ShowToast(
      "Settings".to_string(),
      ToastKind::Info,
      None,
    ));
    assert_eq!(
      app.toasts.active.map(|t| t.message),
      Some("Настройки".to_string())
    );
  }

  #[test]
  fn each_navigation_changes_the_screen_and_remembers_it() {
    for screen in Screen::NAV.into_iter().chain(Screen::SECONDARY) {
      let mut app = app();
      app.update(Message::Navigate(Screen::Queue));
      let effects = app.update(Message::Navigate(screen));
      assert_eq!(app.screen, screen);
      assert_eq!(app.settings.screen, screen);
      if screen == Screen::Settings {
        assert_eq!(
          effects[0],
          Effect::SaveSettings(Box::new(app.settings.clone()))
        );
      } else if screen != Screen::Queue {
        assert_eq!(
          effects,
          vec![Effect::SaveSettings(Box::new(app.settings.clone()))]
        );
      }
    }
  }

  #[test]
  fn navigating_to_the_current_screen_keeps_the_history() {
    let mut app = app();
    assert_eq!(app.update(Message::Navigate(Screen::Home)), Vec::new());
    assert!(app.back_stack.is_empty());
  }

  #[test]
  fn back_returns_to_the_previous_screens() {
    let mut app = app();
    app.update(Message::Navigate(Screen::Albums));
    app.update(Message::Navigate(Screen::Queue));
    let effects = app.update(Message::Back);
    assert_eq!(app.screen, Screen::Albums);
    assert_eq!(app.settings.screen, Screen::Albums);
    assert_eq!(
      effects,
      vec![Effect::SaveSettings(Box::new(app.settings.clone()))]
    );
    app.update(Message::Back);
    assert_eq!(app.screen, Screen::Home);
  }

  #[test]
  fn back_without_history_does_nothing() {
    let mut app = app();
    assert_eq!(app.update(Message::Back), Vec::new());
    assert_eq!(app.screen, Screen::Home);
  }

  #[test]
  fn the_history_stays_bounded() {
    let mut app = app();
    for _ in 0..200 {
      app.update(Message::Navigate(Screen::Albums));
      app.update(Message::Navigate(Screen::Queue));
    }
    assert!(app.back_stack.len() <= MAX_BACK_STACK);
  }

  #[test]
  fn the_app_opens_on_the_remembered_screen() {
    let settings = NativeSettings {
      screen: Screen::Queue,
      ..NativeSettings::default()
    };
    let app = App::new(PersistedWindowState::default(), settings, I18n::default());
    assert_eq!(app.screen, Screen::Queue);
  }

  #[test]
  fn loaded_sets_the_duration_and_keeps_the_position_after_a_device_change() {
    let mut app = app();
    app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(
      42,
    ))));
    app.update(Message::Audio(AudioEvent::Loaded {
      duration: Some(Duration::from_secs(200)),
      sample_rate: 48_000,
    }));
    assert_eq!(app.playback.position, Duration::from_secs(42));
    assert_eq!(app.playback.duration, Some(Duration::from_secs(200)));
  }

  #[test]
  fn loading_a_new_file_resets_the_position() {
    let mut app = app();
    app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(
      42,
    ))));
    let path = std::path::PathBuf::from("titre.mp3");
    let effects = app.update(Message::LoadFile(path.clone()));
    assert_eq!(app.playback.position, Duration::ZERO);
    assert_eq!(app.playback.duration, None);
    assert_eq!(effects, vec![Effect::AudioLoad(path)]);
  }

  #[test]
  fn changing_the_language_loads_it_outside_of_update_and_saves() {
    let mut app = app();
    let effects = app.update(Message::SetLanguage("ru".into()));
    assert_eq!(app.settings.ui_lang, "ru");
    assert_eq!(
      effects,
      vec![
        Effect::LoadLanguage("ru".into()),
        Effect::SaveSettings(Box::new(app.settings.clone()))
      ]
    );
  }

  #[test]
  fn a_loaded_language_applies_without_restart() {
    let mut app = app();
    assert_eq!(app.i18n.t("Settings"), "Settings");
    let cache = std::env::temp_dir().join("riff-app-i18n-absent");
    app.update(Message::SetLanguage("ru".into()));
    app.update(Message::LanguageLoaded(Arc::new(I18n::load("ru", &cache))));
    assert_eq!(app.i18n.t("Settings"), "Настройки");
    assert_eq!(
      app.i18n.t("Pas dans le dictionnaire"),
      "Pas dans le dictionnaire"
    );
  }

  #[test]
  fn a_late_language_does_not_replace_the_current_one() {
    let mut app = app();
    let cache = std::env::temp_dir().join("riff-app-i18n-absent");
    app.update(Message::SetLanguage("uk".into()));
    app.update(Message::SetLanguage("ru".into()));
    app.update(Message::LanguageLoaded(Arc::new(I18n::load("uk", &cache))));
    assert_eq!(app.i18n.lang(), "");
    app.update(Message::LanguageLoaded(Arc::new(I18n::load("ru", &cache))));
    assert_eq!(app.i18n.lang(), "ru");
  }

  fn dark_theme_file(name: &str) -> LoadedTheme {
    file::parse(&format!(
      r##"{{ "name": "{name}", "mode": "dark", "source": "#5247D1" }}"##
    ))
    .unwrap()
  }

  #[test]
  fn default_theme_comes_from_the_accent() {
    let app = app();
    assert_eq!(app.theme.source, ThemeSource::Accent);
    // Riff vierge : accent `#5247d1`, `hslToHex(245, 51, 75)` (`app.js:1261`), calculé et non mesuré.
    assert_eq!(app.theme.scheme.primary, Rgb::new(164, 159, 224));
  }

  #[test]
  fn switching_to_light_changes_the_theme_and_saves() {
    let mut app = app();
    let effects = app.update(Message::SetThemeMode(ThemeMode::Light));
    assert!(!app.theme.dark);
    assert!(!app.theme.theme.extended_palette().is_dark);
    assert_eq!(
      effects,
      vec![Effect::SaveSettings(Box::new(app.settings.clone()))]
    );
    assert_eq!(app.settings.theme_mode, ThemeMode::Light);
  }

  #[test]
  fn a_cover_only_changes_the_theme_in_auto_mode() {
    let mut app = app();
    let source = Rgb::new(0x80, 0x20, 0x20);
    let before = app.theme.scheme;
    app.update(Message::CoverColor(source));
    assert_eq!(app.theme.scheme, before);
    app.update(Message::SetThemeMode(ThemeMode::Auto));
    app.update(Message::CoverColor(source));
    assert_eq!(app.theme.source, ThemeSource::Cover(source));
    assert_ne!(app.theme.scheme, before);
    assert_eq!(contrast::check(&app.theme.scheme), Ok(()));
  }

  #[test]
  fn a_new_cover_gives_a_new_scheme() {
    let mut app = app();
    app.update(Message::SetThemeMode(ThemeMode::Auto));
    app.update(Message::CoverColor(Rgb::new(0x80, 0x20, 0x20)));
    let first = app.theme.scheme;
    app.update(Message::CoverColor(Rgb::new(0x20, 0x20, 0x80)));
    assert_ne!(app.theme.scheme, first);
  }

  #[test]
  fn a_new_cover_fades_the_play_button_and_the_background() {
    let mut app = app();
    // Le fond ne suit la couleur que sous `intensity_bg` ; sinon il reste le gris du thème sombre.
    app.settings.intensity_bg = true;
    app.update(Message::SetThemeMode(ThemeMode::Auto));
    let (body, play) = theme_fade_targets(&app.theme);
    app.theme_fade = crate::motion::ThemeFade::at(body, play);
    assert!(!app.theme_fade.running(app.now), "repos");
    let (body_before, play_before) = theme_fade_targets(&app.theme);
    app.update(Message::CoverColor(Rgb::new(0x80, 0x20, 0x20)));
    let (body, play) = theme_fade_targets(&app.theme);
    assert_ne!((body, play), (body_before, play_before));
    let start = app.now;
    assert_eq!(app.theme_fade.play.value(start), play_before, "départ");
    assert_eq!(app.theme_fade.body.value(start), body_before);
    assert!(app.animation_needed());
    // 150 ms `ease` pour le bouton, 400 ms pour le fond.
    app.now = start + Duration::from_millis(150);
    assert_eq!(app.theme_fade.play.value(app.now), play);
    assert_ne!(app.theme_fade.body.value(app.now), body);
    app.now = start + Duration::from_millis(400);
    assert_eq!(app.theme_fade.body.value(app.now), body);
    assert!(!app.animation_needed(), "posée : l'abonnement s'arrête");
    // Un message sans changement de thème ne relance rien.
    app.update(Message::CoverColor(Rgb::new(0x80, 0x20, 0x20)));
    assert!(!app.theme_fade.running(app.now));
  }

  #[test]
  fn a_cover_change_during_a_fade_starts_from_the_displayed_color() {
    let mut app = app();
    app.update(Message::SetThemeMode(ThemeMode::Auto));
    app.update(Message::CoverColor(Rgb::new(0x80, 0x20, 0x20)));
    let start = app.now;
    app.now = start + Duration::from_millis(60);
    let shown = app.theme_fade.play.value(app.now);
    assert_ne!(shown, app.theme_fade.play.target());
    app.theme_fade.set(
      app.theme_fade.body.target(),
      Color::from_rgb8(0, 0, 0xff),
      app.now,
    );
    assert_eq!(app.theme_fade.play.value(app.now), shown);
  }

  #[test]
  fn cover_bytes_are_decoded_outside_of_update() {
    let mut app = app();
    assert_eq!(
      app.update(Message::CoverBytes(vec![1, 2].into())),
      Vec::new()
    );
    app.update(Message::SetThemeMode(ThemeMode::Auto));
    assert_eq!(
      app.update(Message::CoverBytes(vec![1, 2].into())),
      vec![Effect::ExtractCover(vec![1, 2].into())]
    );
  }

  #[test]
  fn a_theme_file_applies_without_restart_and_is_remembered() {
    let mut app = app();
    assert_eq!(
      app.update(Message::SelectThemeFile("nuit".into())),
      vec![Effect::LoadThemeFile("nuit".into())]
    );
    let effects = app.update(Message::ThemeFileLoaded(
      "nuit".into(),
      Ok(Box::new(dark_theme_file("Nuit"))),
    ));
    assert_eq!(app.theme.source, ThemeSource::File("nuit".into()));
    assert_eq!(app.settings.native_theme.as_deref(), Some("nuit"));
    assert_eq!(
      effects,
      vec![Effect::SaveSettings(Box::new(app.settings.clone()))]
    );
  }

  #[test]
  fn a_refused_theme_file_keeps_the_current_theme() {
    let mut app = app();
    app.update(Message::ThemeFileLoaded(
      "casse".into(),
      Err("contraste".to_string()),
    ));
    assert_eq!(app.theme.source, ThemeSource::Accent);
    assert_eq!(app.settings.native_theme, None);
    assert_eq!(app.last_theme_error.as_deref(), Some("contraste"));
  }

  #[test]
  fn a_cover_does_not_replace_a_theme_file() {
    let mut app = app();
    app.update(Message::SetThemeMode(ThemeMode::Auto));
    app.update(Message::ThemeFileLoaded(
      "nuit".into(),
      Ok(Box::new(dark_theme_file("Nuit"))),
    ));
    app.update(Message::CoverColor(Rgb::new(0x80, 0x20, 0x20)));
    assert_eq!(app.theme.source, ThemeSource::File("nuit".into()));
  }

  #[test]
  fn clearing_the_theme_file_returns_to_the_accent() {
    let mut app = app();
    app.update(Message::ThemeFileLoaded(
      "nuit".into(),
      Ok(Box::new(dark_theme_file("Nuit"))),
    ));
    let effects = app.update(Message::ClearThemeFile);
    assert_eq!(app.theme.source, ThemeSource::Accent);
    assert_eq!(
      effects,
      vec![Effect::SaveSettings(Box::new(app.settings.clone()))]
    );
  }

  #[test]
  fn opening_asks_for_the_scale() {
    let mut app = app();
    let effects = app.update(Message::WindowOpened {
      position: None,
      size: Size::new(1280.0, 800.0),
    });
    assert_eq!(effects[0], Effect::QueryScale);
    assert!(effects.contains(&Effect::UpdateYtDlp));
    assert!(effects.contains(&Effect::AudioMix(app.data.volume, app.data.muted)));
  }

  fn available() -> updates::Outcome {
    updates::Outcome::Available(updates::Release {
      version: "9.9.9".into(),
      notes: String::new(),
      url: "https://github.com/pouskit/riff-rs/releases/tag/v9.9.9".into(),
    })
  }

  #[test]
  fn the_update_check_runs_once_per_click_and_never_by_itself() {
    let mut app = app();
    assert_eq!(app.update, updates::UpdateState::Idle);
    assert_eq!(app.update(Message::UpdateCheck), vec![Effect::CheckUpdate]);
    assert_eq!(app.update, updates::UpdateState::Checking);
    assert!(
      app.update(Message::UpdateCheck).is_empty(),
      "pas de seconde requête pendant la première"
    );
  }

  #[test]
  fn the_compact_mode_toggles_between_two_window_effects() {
    let mut app = app();
    assert_eq!(
      app.update(Message::ToggleMiniMode),
      vec![Effect::EnterMiniWindow]
    );
    let effects = app.update(Message::ToggleMiniMode);
    assert!(
      matches!(effects.as_slice(), [Effect::LeaveMiniWindow(_, _, false)]),
      "{effects:?}"
    );
    assert_eq!(
      app.update(Message::ToggleMiniMode),
      vec![Effect::EnterMiniWindow]
    );
  }

  /// `checkCompactLayout` (`app.js:18246-18259`) : la mise en page suit la largeur reçue (<= 520 px CSS), pas l'ordre de passage.
  #[test]
  fn the_compact_layout_follows_the_window_width_like_riff() {
    let mut app = app();
    let width = |app: &mut App, w: f32| app.update(Message::WindowResized(Size::new(w, 160.0)));
    width(&mut app, 1920.0);
    app.update(Message::ToggleMiniMode);
    assert!(
      !app.compact_layout(),
      "la fenêtre garde 1920 px jusqu'à la nouvelle taille"
    );
    width(&mut app, 380.0);
    assert!(app.compact_layout());
    // Fenêtre compacte élargie : 520 px CSS = 491,4 px logiques.
    width(&mut app, 600.0);
    assert!(app.window.mini.is_some() && !app.compact_layout());
    width(&mut app, 491.0);
    assert!(app.compact_layout());
    width(&mut app, 492.0);
    assert!(!app.compact_layout());
    // Sortie : la mise en page compacte reste tant que la fenêtre n'a pas repris sa taille.
    width(&mut app, 380.0);
    app.update(Message::ToggleMiniMode);
    assert!(app.window.mini.is_none() && app.compact_layout());
    width(&mut app, 1920.0);
    assert!(!app.compact_layout());
  }

  /// Windows envoie `Resized` en 0x0 à la réduction (WM_SIZE sans filtre) : la mise en page ne change pas, comme `innerWidth` de Chromium.
  #[test]
  fn the_compact_layout_ignores_a_zero_sized_window_while_minimized() {
    let mut app = app();
    let resize =
      |app: &mut App, w: f32, h: f32| app.update(Message::WindowResized(Size::new(w, h)));
    resize(&mut app, 1920.0, 1080.0);
    assert!(!app.compact_layout());
    resize(&mut app, 0.0, 0.0);
    assert!(!app.compact_layout());
    resize(&mut app, 1920.0, 1080.0);
    assert!(!app.compact_layout());
    app.update(Message::ToggleMiniMode);
    resize(&mut app, 380.0, 160.0);
    assert!(app.compact_layout());
    resize(&mut app, 0.0, 0.0);
    assert!(app.compact_layout());
    resize(&mut app, 380.0, 160.0);
    assert!(app.compact_layout());
  }
  #[test]
  fn the_compact_layout_keeps_the_window_state_while_the_size_is_unknown() {
    let mut app = app();
    app.window.size = None;
    assert!(!app.compact_layout());
    app.update(Message::ToggleMiniMode);
    assert!(app.compact_layout());
  }

  #[test]
  fn the_compact_lyrics_button_only_works_in_the_compact_mode_and_resizes_the_window() {
    let mut app = app();
    assert!(app.update(Message::ToggleMiniLyrics).is_empty());
    app.update(Message::ToggleMiniMode);
    app.update(Message::WindowOpened {
      position: None,
      size: MINI_SIZE,
    });
    let effects = app.update(Message::ToggleMiniLyrics);
    assert_eq!(
      effects,
      vec![Effect::ResizeMini(
        crate::window::MINI_LYRICS_MIN,
        Size::new(380.0, 360.0)
      )]
    );
    assert!(app.window.mini_lyrics);
    // Les paroles restent demandées à la sortie, et la fenêtre les retrouve à la rentrée (`app.js:18265`).
    app.update(Message::ToggleMiniMode);
    let effects = app.update(Message::ToggleMiniMode);
    assert_eq!(
      effects,
      vec![
        Effect::EnterMiniWindow,
        Effect::ResizeMini(crate::window::MINI_LYRICS_MIN, Size::new(380.0, 360.0))
      ]
    );
  }

  #[test]
  fn the_launcher_is_written_only_on_click() {
    let mut app = app();
    assert_eq!(
      app.update(Message::LauncherInstall),
      vec![Effect::InstallLauncher]
    );
  }

  #[test]
  fn a_disabled_update_check_sends_nothing() {
    let mut app = app();
    app.settings.update_check = false;
    assert!(app.update(Message::UpdateCheck).is_empty());
    assert_eq!(app.update, updates::UpdateState::Idle);
  }

  #[test]
  fn the_update_result_is_kept_and_the_release_page_opens_only_for_an_offered_release() {
    let mut app = app();
    assert!(app.update(Message::OpenReleasePage).is_empty());
    app.update(Message::UpdateChecked(Ok(updates::Outcome::UpToDate)));
    assert!(app.update(Message::OpenReleasePage).is_empty());
    app.update(Message::UpdateChecked(Err(
      updates::CheckError::RateLimited,
    )));
    assert_eq!(
      app.update,
      updates::UpdateState::Failed(updates::CheckError::RateLimited)
    );
    app.update(Message::UpdateChecked(Ok(available())));
    assert_eq!(
      app.update(Message::OpenReleasePage),
      vec![Effect::OpenUrl(
        "https://github.com/pouskit/riff-rs/releases/tag/v9.9.9".into()
      )]
    );
  }

  #[test]
  fn the_install_starts_only_for_an_offered_release_and_only_on_click() {
    let mut app = app();
    assert!(
      app.update(Message::UpdateInstall).is_empty(),
      "rien d'offert"
    );
    assert_eq!(app.install, update_install::InstallState::Idle);
    app.update(Message::UpdateChecked(Ok(available())));
    let effects = app.update(Message::UpdateInstall);
    if update_install::can_install() {
      assert!(matches!(effects.as_slice(), [Effect::InstallUpdate(r)] if r.version == "9.9.9"));
      assert_eq!(app.install, update_install::InstallState::Downloading(0));
      assert!(
        app.update(Message::UpdateInstall).is_empty(),
        "pas de second téléchargement pendant le premier"
      );
    } else {
      assert!(effects.is_empty(), "Linux et macOS : page de la release");
      assert_eq!(app.install, update_install::InstallState::Idle);
    }
    app.settings.update_check = false;
    app.install = update_install::InstallState::Idle;
    assert!(app.update(Message::UpdateInstall).is_empty());
  }

  #[test]
  fn install_events_drive_the_state_and_a_failure_can_be_retried() {
    use update_install::{InstallError, InstallEvent, InstallState, VerifyError};
    let mut app = app();
    app.update(Message::UpdateInstallEvent(InstallEvent::Progress(40)));
    assert_eq!(app.install, InstallState::Downloading(40));
    app.update(Message::UpdateInstallEvent(InstallEvent::Verifying));
    assert_eq!(app.install, InstallState::Verifying);
    app.update(Message::UpdateInstallEvent(InstallEvent::Failed(
      InstallError::Verify(VerifyError::SignatureMissing),
    )));
    let InstallState::Failed(message) = &app.install else {
      panic!("échec attendu")
    };
    assert!(message.contains("nothing was run"));
    app.update(Message::UpdateInstallEvent(InstallEvent::Launched));
    assert_eq!(app.install, InstallState::Launched);
  }

  #[test]
  fn close_button_and_system_close_save_the_window_then_exit() {
    let mut app = app();
    assert_eq!(
      app.update(Message::CloseRequested),
      vec![Effect::QueryCloseState]
    );
    assert_eq!(
      app.update(Message::Chrome(chrome::Action::Close)),
      vec![Effect::QueryCloseState]
    );
    let effects = app.update(Message::CloseStateKnown {
      is_maximized: true,
      is_full_screen: false,
    });
    let [Effect::SaveWindowAndExit(state, _)] = effects.as_slice() else {
      panic!("{effects:?}")
    };
    assert!(state.is_maximized);
  }

  #[test]
  fn a_scale_other_than_one_restores_the_physical_bounds() {
    let mut app = app();
    let effects = app.update(Message::ScaleKnown(2.0));
    assert_eq!(
      effects,
      vec![Effect::ApplyBounds(
        Point::new(50.0, 40.0),
        Size::new(640.0, 400.0)
      )]
    );
  }
}
