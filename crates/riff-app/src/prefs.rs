//! Page de réglages : catalogue des réglages, lecture, écriture et effets. La vue est dans `screens/settings.rs`.

use crate::app::{App, Effect, Message};
use crate::manage::ImportKind;
use crate::prefs_visual::{VisualBool, VisualChoice, VisualInt};
use crate::screens::Screen;
use crate::settings::{DEFAULT_HOME_ORDER, ThemeMode, UI_SCALE_RANGE};
use crate::system::wallpaper::{OutputMode, TransitionAnchor, TransitionEasing};
use crate::theme::{ActiveTheme, ThemeSource};
use crate::update::DEFAULT_SIDEBAR_ORDER;
use material_colors::color::Rgb;
use material_colors::hct::Hct;

const TONE_RANGE: (i32, i32) = (0, 100);
const CROSSFADE_RANGE: (i32, i32) = (0, 12);
const HUE_RANGE: (i32, i32) = (0, 360);
/// Échelle pour laquelle les tailles de l'interface native ont été écrites : défaut de Riff.
const REFERENCE_SCALE: f32 = 105.0;

/// Catégories de `#view-settings`, dans l'ordre de `index.html`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
  Interface,
  Theme,
  Banners,
  Tracks,
  Integrations,
  Storage,
  Advanced,
  About,
  CustomThemes,
  Plugins,
  Account,
}

impl Category {
  pub const ALL: [Self; 11] = [
    Self::Interface,
    Self::Theme,
    Self::Banners,
    Self::Tracks,
    Self::Integrations,
    Self::Storage,
    Self::Advanced,
    Self::About,
    Self::CustomThemes,
    Self::Plugins,
    Self::Account,
  ];

  /// Valeur de `data-cat`, gardée dans `settings_cat`.
  #[must_use]
  pub const fn key(self) -> &'static str {
    match self {
      Self::Interface => "interface",
      Self::Theme => "theme",
      Self::Banners => "banners",
      Self::Tracks => "tracks",
      Self::Integrations => "integrations",
      Self::Storage => "storage",
      Self::Advanced => "advanced",
      Self::About => "about",
      Self::CustomThemes => "custom-themes",
      Self::Plugins => "plugins",
      Self::Account => "account",
    }
  }

  #[must_use]
  pub const fn label(self) -> &'static str {
    match self {
      Self::Interface => "Interface",
      Self::Theme => "Theme",
      Self::Banners => "Banners",
      Self::Tracks => "Tracks",
      Self::Integrations => "Integrations",
      Self::Storage => "Storage",
      Self::Advanced => "Advanced",
      Self::About => "About",
      Self::CustomThemes => "Custom themes",
      Self::Plugins => "Plugins",
      Self::Account => "Account",
    }
  }

  /// Valeur inconnue : `interface`.
  #[must_use]
  pub fn parse(value: &str) -> Self {
    Self::ALL
      .into_iter()
      .find(|category| category.key() == value)
      .unwrap_or(Self::Interface)
  }
}

/// Réglages à deux états.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolKey {
  CompactEngine,
  HideEngine,
  PlayBtnBg,
  ShowExplicit,
  IntensityBg,
  HighlightButtons,
  SearchAlbums,
  ClassicRows,
  ShowLocal,
  ShowAlbums,
  HideTitlebar,
  PerfMode,
  ReduceMotion,
  HomeWave,
  HomeRecent,
  HomeArtists,
  ShowYtPlatform,
  SearchArtists,
  LikeAlbumTracks,
  Autoplay,
  Normalize,
  Translation,
  SmoothPresets,
  PersistSound,
  TrayOnClose,
  MediaSession,
  DiscordRpc,
  DiscordArtist,
  UpdateCheck,
  WallpaperSync,
}

/// Réglages numériques, avec leurs bornes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntKey {
  UiScale,
  Brightness,
  Intensity,
  Hue,
  Crossfade,
}

impl IntKey {
  /// Bornes du curseur de `index.html`.
  #[must_use]
  pub const fn range(self) -> (i32, i32) {
    match self {
      Self::UiScale => (UI_SCALE_RANGE.0 as i32, UI_SCALE_RANGE.1 as i32),
      Self::Brightness | Self::Intensity => TONE_RANGE,
      Self::Hue => HUE_RANGE,
      Self::Crossfade => CROSSFADE_RANGE,
    }
  }
}

/// Réglages à choix ou à saisie libre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKey {
  UiLang,
  TrLang,
  ThemeMode,
  PaletteStyle,
  SearchPos,
  HomeLayout,
  AudioQuality,
  Nickname,
  JamRelay,
}

/// Blocs de la page qui ne sont pas de simples contrôles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
  SidebarOrder,
  HomeOrder,
  Blocked,
  NativeThemes,
  BannerMedia,
  BackgroundMedia,
  Mesh,
  Plugins,
  Version,
  AudioWindow,
  Tools,
  Imports,
  Spotify,
  Lastfm,
  Google,
  YoutubeSignIn,
  Folders,
  Cache,
  Backups,
  Proxy,
  YtDlp,
  MultiInstance,
  Diagnostics,
  AccountBackup,
  Shortcuts,
  Launcher,
  MiniWindow,
}

/// Contrôle d'un réglage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
  Toggle(BoolKey),
  Slider(IntKey),
  Choice(TextKey, &'static [(&'static str, &'static str)]),
  Languages(TextKey),
  Accent,
  Input(TextKey),
  Panel(Panel),
  /// Paroles, visualiseurs et fonds.
  VisualToggle(VisualBool),
  VisualSlider(VisualInt),
  VisualChoice(VisualChoice),
  /// Choix à boutons côte à côte (`.theme-mode-toggle`) plutôt qu'en liste.
  Segmented(TextKey, &'static [(&'static str, &'static str)]),
  /// Ligne de plateforme avec son bouton « Import » (`#card-*-import`).
  Import(ImportKind),
}

/// Entrée de la page. `label` et `hint` sont des clés de traduction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
  pub category: Category,
  pub label: &'static str,
  pub hint: &'static str,
  pub control: Control,
}

const fn entry(
  category: Category,
  label: &'static str,
  hint: &'static str,
  control: Control,
) -> Entry {
  Entry {
    category,
    label,
    hint,
    control,
  }
}

/// Connexion YouTube de lecture.
pub const YOUTUBE_HINT: &str = "Stored as yt-cookies.txt in your data folder, unencrypted. These cookies give access to your Google account.";

/// Texte affiché pour la traduction : décision 5 de la spec.
pub const TRANSLATION_HINT: &str = "Off by default. When on, lyrics and interface texts are sent to Google, or to DeepL if a key is set.";

/// Texte affiché pour la traduction des paroles : rien ne part sans le réglage « Translation » de l'interface.
pub const LYRICS_TRANSLATION_HINT: &str = "Lyrics are sent to Google, or to DeepL if a key is set. Needs the Translation switch in Interface.";

const THEME_MODES: &[(&str, &str)] = &[
  ("light", "Light"),
  ("dark", "Dark"),
  ("auto", "Auto (album)"),
];
const PALETTES: &[(&str, &str)] = &[
  ("classic", "Classic"),
  ("tonal-spot", "Tonal spot"),
  ("expressive", "Expressive"),
  ("vibrant", "Vibrant"),
  ("neutral", "Neutral"),
];
const SEARCH_POSITIONS: &[(&str, &str)] =
  &[("left", "Left"), ("middle", "Middle"), ("right", "Right")];
const HOME_LAYOUTS: &[(&str, &str)] = &[("spotify", "Spotify"), ("riff", "Riff")];
const QUALITIES: &[(&str, &str)] = &[("low", "Low"), ("medium", "Medium"), ("high", "High")];

/// Tout le catalogue, dans l'ordre de la page.
pub const ENTRIES: &[Entry] = &[
  // Interface : ordre et textes de `index.html:946-1337`. Sans réglage dans riff-rs, donc non inventées :
  // échelle de police, police, style d'icône, moteur de recherche, rayon d'angle, opacité de l'interface,
  // flou des panneaux, curseur, sensibilité et douceur de la barre, couvertures façonnées, karaoké sans paroles.
  entry(
    Category::Interface,
    "Interface language",
    "",
    Control::Languages(TextKey::UiLang),
  ),
  entry(
    Category::Interface,
    "Hide top titlebar",
    "Hide the window titlebar and reveal it on top edge hover",
    Control::Toggle(BoolKey::HideTitlebar),
  ),
  entry(
    Category::Interface,
    "UI scale",
    "",
    Control::Slider(IntKey::UiScale),
  ),
  entry(
    Category::Interface,
    "Position of searchbar",
    "",
    Control::Choice(TextKey::SearchPos, SEARCH_POSITIONS),
  ),
  entry(
    Category::Interface,
    "Show YouTube platform",
    "Not recommended: regular YouTube includes non-music videos and loads slower than YouTube Music",
    Control::Toggle(BoolKey::ShowYtPlatform),
  ),
  entry(
    Category::Interface,
    "Compact engine buttons",
    "Show only icons for YouTube, YouTube Music, and SoundCloud in the top bar",
    Control::Toggle(BoolKey::CompactEngine),
  ),
  entry(
    Category::Interface,
    "Hide engine buttons",
    "Hide search engine buttons from the top bar",
    Control::Toggle(BoolKey::HideEngine),
  ),
  entry(
    Category::Interface,
    "Play/Pause button background",
    "Show or hide the rounded background behind the main play/pause button",
    Control::Toggle(BoolKey::PlayBtnBg),
  ),
  entry(
    Category::Interface,
    "Show wave visualizer in bottom bar",
    "",
    Control::VisualToggle(VisualBool::BarWave),
  ),
  entry(
    Category::Interface,
    "Playbar visualizer position",
    "Show visualizer as a small icon on the right or wide behind the playbar",
    Control::VisualChoice(VisualChoice::BarVizPos),
  ),
  entry(
    Category::Interface,
    "Playbar visualizer opacity",
    "",
    Control::VisualSlider(VisualInt::BarVizOpacity),
  ),
  entry(
    Category::Interface,
    "Connected liquid visualizer",
    "Draw a smooth connected liquid wave instead of separate bars",
    Control::VisualToggle(VisualBool::VizConnected),
  ),
  entry(
    Category::Interface,
    "Shaped cover in the bottom bar",
    "Morphing Material 3 shape with a wavy progress ring around the cover",
    Control::VisualToggle(VisualBool::M3Cover),
  ),
  entry(
    Category::Interface,
    "Show explicit tag [E] on tracks",
    "Displays [E] badge next to tracks with explicit content",
    Control::Toggle(BoolKey::ShowExplicit),
  ),
  entry(
    Category::Interface,
    "Visualizer with lyrics",
    "Show wave visualizer at the bottom even when track has lyrics",
    Control::VisualToggle(VisualBool::TvkVisualizerLyrics),
  ),
  entry(
    Category::Interface,
    "Visualizer opacity",
    "Adjust transparency of the TV karaoke visualizer",
    Control::VisualSlider(VisualInt::TvkVisualizerOpacity),
  ),
  entry(
    Category::Interface,
    "Background blur",
    "Blur of the ambient background in fullscreen karaoke",
    Control::VisualSlider(VisualInt::TvkBgBlur),
  ),
  entry(
    Category::Interface,
    "Visualizer smoothness",
    "Higher = calmer, slower bars",
    Control::VisualSlider(VisualInt::VizSmooth),
  ),
  entry(
    Category::Interface,
    "Visualizer sensitivity",
    "Higher = taller bars",
    Control::VisualSlider(VisualInt::VizSens),
  ),
  // `index.html:1308-1314`, entre « Visualizer sensitivity » et la rangée de la barre latérale.
  entry(
    Category::Interface,
    "Performance mode",
    "Disables visualizers, animated gradient and blur effects, limits animations to 30 FPS",
    Control::Toggle(BoolKey::PerfMode),
  ),
  // Une seule rangée comme `#setting-sidebar-home-customizer` (`index.html:1317-1337`) : les deux listes y sont côte à côte.
  entry(
    Category::Interface,
    "Customize sidebar & home layout",
    "Drag and drop to reorder sidebar navigation items and home page sections",
    Control::Panel(Panel::SidebarOrder),
  ),
  // Riff n'a aucun réglage de traduction de l'interface (pas de « Translation » dans `index.html`) :
  // garde-fou propre à riff-rs, laissé en fin de catégorie.
  entry(
    Category::Interface,
    "Translation",
    TRANSLATION_HINT,
    Control::Toggle(BoolKey::Translation),
  ),
  // Thème : `index.html:1495-1626`. « Home button icon » (icônes d'application) n'existe pas dans riff-rs.
  entry(
    Category::Theme,
    "Theme mode",
    "",
    Control::Segmented(TextKey::ThemeMode, THEME_MODES),
  ),
  entry(
    Category::Theme,
    "Sync Windows wallpaper",
    "Syncs the Windows desktop wallpaper with the album art of the song playing",
    Control::Toggle(BoolKey::WallpaperSync),
  ),
  entry(
    Category::Theme,
    "Palette style",
    "",
    Control::Choice(TextKey::PaletteStyle, PALETTES),
  ),
  entry(Category::Theme, "Accent color", "", Control::Accent),
  entry(Category::Theme, "Hue", "", Control::Slider(IntKey::Hue)),
  entry(
    Category::Theme,
    "Intensity",
    "",
    Control::Slider(IntKey::Intensity),
  ),
  entry(
    Category::Theme,
    "Apply intensity to background",
    "Tint background and surface panels with the accent color and intensity",
    Control::Toggle(BoolKey::IntensityBg),
  ),
  entry(
    Category::Theme,
    "Brightness",
    "",
    Control::Slider(IntKey::Brightness),
  ),
  // Écart voulu avec Riff (demandé par Hugo) : l'interrupteur est sous Theme, chez Riff il est sous Advanced (`index.html:2481`).
  entry(
    Category::Theme,
    "Background mesh gradient",
    "Show dynamic ambient color gradients behind interface panels",
    Control::VisualToggle(VisualBool::MeshGradient),
  ),
  entry(
    Category::Theme,
    "Mesh gradient brightness",
    "",
    Control::VisualSlider(VisualInt::MeshBrightness),
  ),
  entry(
    Category::Theme,
    "Highlight buttons",
    "Fill the main button of each screen with the accent color",
    Control::Toggle(BoolKey::HighlightButtons),
  ),
  // Bannières : `index.html:1629-1756`.
  entry(
    Category::Banners,
    "Enable banners",
    "Show artwork backdrop banners in Now playing and playlists",
    Control::VisualToggle(VisualBool::Banners),
  ),
  entry(
    Category::Banners,
    "Track banners",
    "Show artwork backdrop banner for currently playing track",
    Control::VisualToggle(VisualBool::TrackBanners),
  ),
  entry(
    Category::Banners,
    "Banner media",
    "",
    Control::Panel(Panel::BannerMedia),
  ),
  entry(
    Category::Banners,
    "Banner fit",
    "",
    Control::VisualChoice(VisualChoice::BannerFit),
  ),
  entry(
    Category::Banners,
    "Banner blur",
    "",
    Control::VisualSlider(VisualInt::BannerBlur),
  ),
  entry(
    Category::Banners,
    "Banner opacity",
    "",
    Control::VisualSlider(VisualInt::BannerOpacity),
  ),
  entry(
    Category::Banners,
    "Banner fade",
    "",
    Control::VisualSlider(VisualInt::BannerFade),
  ),
  entry(
    Category::Banners,
    "Background media",
    "",
    Control::Panel(Panel::BackgroundMedia),
  ),
  entry(
    Category::Banners,
    "Opacity",
    "",
    Control::VisualSlider(VisualInt::BgOpacity),
  ),
  entry(
    Category::Banners,
    "Blur",
    "",
    Control::VisualSlider(VisualInt::BgBlur),
  ),
  entry(
    Category::Banners,
    "Cover background",
    "Uses a blurred version of the current cover as the background",
    Control::VisualToggle(VisualBool::CoverBg),
  ),
  entry(
    Category::Banners,
    "Cover background brightness",
    "",
    Control::VisualSlider(VisualInt::CoverBgBrightness),
  ),
  // Pistes : `index.html:1759-2035`. Sans réglage dans riff-rs : barre ondulée, bouton et fond de la barre de progression,
  // sortie audio, délai du flux.
  entry(
    Category::Tracks,
    "Show artists in search",
    "",
    Control::Toggle(BoolKey::SearchArtists),
  ),
  entry(
    Category::Tracks,
    "Classic rounded track cards",
    "Use rounder 20px corners and press animation on track rows",
    Control::Toggle(BoolKey::ClassicRows),
  ),
  entry(
    Category::Tracks,
    "Show My files tab",
    "",
    Control::Toggle(BoolKey::ShowLocal),
  ),
  entry(
    Category::Tracks,
    "Show Albums tab",
    "Display saved albums in the sidebar and album page",
    Control::Toggle(BoolKey::ShowAlbums),
  ),
  entry(
    Category::Tracks,
    "Like all album tracks when saving album",
    "Automatically add all tracks of an album to favorites when saving it",
    Control::Toggle(BoolKey::LikeAlbumTracks),
  ),
  entry(
    Category::Tracks,
    "Blocked artists & tracks",
    "Blocked items are hidden from recommendations and auto-skipped",
    Control::Panel(Panel::Blocked),
  ),
  entry(
    Category::Tracks,
    "Smooth preset transitions",
    "",
    Control::Toggle(BoolKey::SmoothPresets),
  ),
  entry(
    Category::Tracks,
    "Save sound settings",
    "Automatically save High Quality, effects and the active preset",
    Control::Toggle(BoolKey::PersistSound),
  ),
  entry(
    Category::Tracks,
    "Crossfade",
    "",
    Control::Slider(IntKey::Crossfade),
  ),
  // Les deux textes de confidentialité de la traduction sont conservés (décision 5 de la spec) : Riff n'en a pas.
  entry(
    Category::Tracks,
    "Lyrics translation",
    LYRICS_TRANSLATION_HINT,
    Control::VisualToggle(VisualBool::TrEnabled),
  ),
  entry(
    Category::Tracks,
    "Translation language",
    "",
    Control::Languages(TextKey::TrLang),
  ),
  entry(
    Category::Tracks,
    "Auto-translate",
    LYRICS_TRANSLATION_HINT,
    Control::VisualToggle(VisualBool::TrAuto),
  ),
  entry(
    Category::Tracks,
    "Lyrics animation",
    "How the active line moves",
    Control::VisualChoice(VisualChoice::LyricsEase),
  ),
  entry(
    Category::Tracks,
    "Lyrics animation speed",
    "",
    Control::VisualSlider(VisualInt::LyricsSpeed),
  ),
  entry(
    Category::Tracks,
    "Lyrics source",
    "Preferred lyrics provider with automatic fallback",
    Control::VisualChoice(VisualChoice::LyricsSource),
  ),
  entry(
    Category::Tracks,
    "Word-by-word source",
    "Source for karaoke word timings",
    Control::VisualChoice(VisualChoice::WordSource),
  ),
  entry(
    Category::Tracks,
    "Word mode style",
    "How the active line is highlighted in Word mode",
    Control::VisualChoice(VisualChoice::WordStyle),
  ),
  entry(
    Category::Tracks,
    "Gradient tightness",
    "",
    Control::VisualSlider(VisualInt::LyricsTightness),
  ),
  // Rangées propres à riff-rs, laissées en fin de catégorie : « Albums » est dans « Advanced » chez Riff (`index.html:2531`).
  entry(
    Category::Tracks,
    "Albums",
    "Shows albums in search results",
    Control::Toggle(BoolKey::SearchAlbums),
  ),
  entry(
    Category::Tracks,
    "Audio settings",
    "",
    Control::Panel(Panel::AudioWindow),
  ),
  entry(
    Category::Tracks,
    "Mini lyrics window",
    "",
    Control::VisualToggle(VisualBool::MiniLyrics),
  ),
  entry(
    Category::Tracks,
    "Mini lyrics always on top",
    "",
    Control::VisualToggle(VisualBool::MiniOnTop),
  ),
  entry(
    Category::Tracks,
    "Volume normalization",
    "",
    Control::Toggle(BoolKey::Normalize),
  ),
  // Intégrations : `index.html:2039-2170`. « JSON Playlist » n'a pas d'import.
  entry(
    Category::Integrations,
    "Discord RPC",
    "",
    Control::Toggle(BoolKey::DiscordRpc),
  ),
  entry(
    Category::Integrations,
    "Last.fm Scrobbling",
    "",
    Control::Panel(Panel::Lastfm),
  ),
  entry(
    Category::Integrations,
    "Show artist in Discord status",
    "Display \"Listening to Artist\" instead of \"Listening to Riff\" in compact profile status",
    Control::Toggle(BoolKey::DiscordArtist),
  ),
  entry(
    Category::Integrations,
    "Spotify",
    "",
    Control::Panel(Panel::Spotify),
  ),
  entry(
    Category::Integrations,
    "YouTube",
    "",
    Control::Import(ImportKind::Link),
  ),
  entry(
    Category::Integrations,
    "YouTube Music",
    "",
    Control::Import(ImportKind::Link),
  ),
  entry(
    Category::Integrations,
    "SoundCloud",
    "",
    Control::Import(ImportKind::Link),
  ),
  entry(
    Category::Integrations,
    "Apple Music",
    "",
    Control::Import(ImportKind::Apple),
  ),
  entry(
    Category::Integrations,
    "Deezer",
    "",
    Control::Import(ImportKind::Link),
  ),
  entry(
    Category::Integrations,
    "Yandex Music",
    "",
    Control::Import(ImportKind::Yandex),
  ),
  entry(
    Category::Integrations,
    "Google and YouTube",
    "Imports your liked videos and playlists. Read-only access.",
    Control::Panel(Panel::Google),
  ),
  entry(
    Category::Storage,
    "Clear cache",
    "Temporary web and partial data",
    Control::Panel(Panel::Cache),
  ),
  entry(
    Category::Storage,
    "Required tools",
    "yt-dlp and ffmpeg",
    Control::Panel(Panel::Tools),
  ),
  entry(
    Category::Storage,
    "Music folder",
    "",
    Control::Panel(Panel::Folders),
  ),
  entry(
    Category::Storage,
    "Download folder",
    "",
    Control::Panel(Panel::Folders),
  ),
  entry(
    Category::Storage,
    "Export all playlists (JSON)",
    "Backup all playlists to a JSON file",
    Control::Panel(Panel::Backups),
  ),
  entry(
    Category::Storage,
    "Import playlists (JSON)",
    "Restore playlists from exported JSON file",
    Control::Panel(Panel::Backups),
  ),
  entry(
    Category::Storage,
    "Export Riff account (.rfacc)",
    "Backup full library, playlists, settings and history",
    Control::Panel(Panel::AccountBackup),
  ),
  entry(
    Category::Storage,
    "Import Riff account (.rfacc)",
    "Restore entire account from encrypted binary backup",
    Control::Panel(Panel::AccountBackup),
  ),
  entry(
    Category::Advanced,
    "Create desktop & app menu shortcut",
    "Add Riff shortcut to your desktop and system applications menu",
    Control::Panel(Panel::Launcher),
  ),
  entry(
    Category::Advanced,
    "YouTube Account",
    YOUTUBE_HINT,
    Control::Panel(Panel::YoutubeSignIn),
  ),
  entry(
    Category::Advanced,
    "In-app proxy (YouTube / SoundCloud bypass)",
    "Route Riff audio and stream extraction through a proxy without slowing down your whole system",
    Control::Panel(Panel::Proxy),
  ),
  entry(
    Category::Advanced,
    "Song quality",
    "Audio bitrate and streaming format selection",
    Control::Choice(TextKey::AudioQuality, QUALITIES),
  ),
  entry(
    Category::Advanced,
    "Minimize to tray on close",
    "Keep running in background when window is closed",
    Control::Toggle(BoolKey::TrayOnClose),
  ),
  entry(
    Category::Advanced,
    "Legacy media key capture (Windows)",
    "Leave off to use the Windows media overlay and lock screen controls. Turning this on takes media keys away from the overlay",
    Control::Toggle(BoolKey::MediaSession),
  ),
  entry(
    Category::Advanced,
    "Allow multiple instances",
    "For testing: lets you start a second Riff window. Applies after restart. The second window uses its own clean profile.",
    Control::Panel(Panel::MultiInstance),
  ),
  entry(
    Category::Advanced,
    "Online lyrics lookup",
    "Fetch synced and plain lyrics from network providers",
    Control::VisualToggle(VisualBool::OnlineLyrics),
  ),
  entry(
    Category::Advanced,
    "Karaoke word highlighting",
    "Highlight individual words in synchronized lyrics",
    Control::VisualToggle(VisualBool::KaraokeWords),
  ),
  entry(
    Category::Advanced,
    "Auto-scroll synced lyrics",
    "Smoothly follow the active lyric line during playback",
    Control::VisualToggle(VisualBool::LyricsAutoscroll),
  ),
  entry(
    Category::Advanced,
    "Mesh follows theme intensity",
    "Low intensity makes the gradient colors grayer",
    Control::VisualToggle(VisualBool::MeshFollowIntensity),
  ),
  entry(
    Category::Advanced,
    "Autoplay similar tracks",
    "When the queue ends, keep playing more from the same artists, then your wave",
    Control::Toggle(BoolKey::Autoplay),
  ),
  entry(
    Category::Advanced,
    "My Wave recommendations",
    "Show personal recommendations on the home page",
    Control::Toggle(BoolKey::HomeWave),
  ),
  entry(
    Category::Advanced,
    "Jump back in",
    "Recently played tracks on the home page",
    Control::Toggle(BoolKey::HomeRecent),
  ),
  entry(
    Category::Advanced,
    "Favorite artists",
    "Favorite artists on home page",
    Control::Toggle(BoolKey::HomeArtists),
  ),
  entry(
    Category::Advanced,
    "Home layout",
    "Spotify: rows of mixes and quick access. Riff: the carousel.",
    Control::Choice(TextKey::HomeLayout, HOME_LAYOUTS),
  ),
  // `index.html:2616-2624`, sous « Home layout ».
  entry(
    Category::Advanced,
    "Reduce motion",
    "Replace spring animations with simple fades",
    Control::Toggle(BoolKey::ReduceMotion),
  ),
  // Réglages de riff-rs que Riff n'a pas dans cette section : gardés après les rangées de Riff.
  entry(
    Category::Advanced,
    "Global shortcuts",
    "",
    Control::Panel(Panel::Shortcuts),
  ),
  entry(
    Category::Advanced,
    "Update yt-dlp automatically",
    "At most one check per day, contacts github.com.",
    Control::Panel(Panel::YtDlp),
  ),
  entry(
    Category::Advanced,
    "Compact window",
    "",
    Control::Panel(Panel::MiniWindow),
  ),
  entry(
    Category::Advanced,
    "Jam relay URL",
    "Leave empty to use the default encrypted relays",
    Control::Input(TextKey::JamRelay),
  ),
  entry(Category::About, "Riff", "", Control::Panel(Panel::Version)),
  entry(
    Category::About,
    "Keyboard shortcuts",
    "",
    Control::Panel(Panel::Shortcuts),
  ),
  entry(
    Category::About,
    "Diagnostics",
    "",
    Control::Panel(Panel::Diagnostics),
  ),
  entry(
    Category::About,
    "Logs",
    "",
    Control::Panel(Panel::Diagnostics),
  ),
  entry(
    Category::About,
    "Update check",
    "Adds a button that asks GitHub for the latest release. Nothing is sent and nothing is checked on its own.",
    Control::Toggle(BoolKey::UpdateCheck),
  ),
  entry(
    Category::CustomThemes,
    "Native themes",
    "",
    Control::Panel(Panel::NativeThemes),
  ),
  entry(
    Category::Plugins,
    "Plugins",
    "",
    Control::Panel(Panel::Plugins),
  ),
  entry(
    Category::Account,
    "Nickname",
    "",
    Control::Input(TextKey::Nickname),
  ),
  entry(
    Category::Account,
    "Profile avatar",
    "Click your avatar above to pick a custom photo or reset to default",
    Control::Panel(Panel::AccountBackup),
  ),
  entry(
    Category::Account,
    "Export Riff account (.rfacc)",
    "Save your profile, playlists, favorites, history, and settings to a file",
    Control::Panel(Panel::AccountBackup),
  ),
  entry(
    Category::Account,
    "Import Riff account (.rfacc)",
    "Restore your entire Riff account from a .rfacc backup file",
    Control::Panel(Panel::AccountBackup),
  ),
  entry(
    Category::Account,
    "Connected services",
    "Manage your accounts, import playlists and stream from linked platforms",
    Control::Panel(Panel::AccountBackup),
  ),
];

/// Entrées d'une catégorie.
pub fn in_category(category: Category) -> impl Iterator<Item = &'static Entry> {
  ENTRIES
    .iter()
    .filter(move |entry| entry.category == category)
}

/// Recherche de réglage : libellé anglais ou traduit, sans casse. `translate` donne le libellé affiché.
pub fn matching(query: &str, translate: impl Fn(&str) -> String) -> Vec<&'static Entry> {
  let query = query.trim().to_lowercase();
  if query.is_empty() {
    return Vec::new();
  }
  ENTRIES
    .iter()
    .filter(|entry| {
      entry.label.to_lowercase().contains(&query)
        || translate(entry.label).to_lowercase().contains(&query)
    })
    .collect()
}

/// Liste réordonnée par glisser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderList {
  Sidebar,
  Home,
}

/// État de la page, hors réglages enregistrés.
#[derive(Debug, Clone, Default)]
pub struct SettingsUi {
  pub query: String,
  pub accent_input: Option<String>,
  /// Champ hexadécimal de l'accent déplié par le bouton « + » (Riff ouvre un sélecteur de couleur natif à la place).
  pub accent_open: bool,
  /// Panneau du maillage déplié par « Configure » (Riff ouvre `#mesh-customizer-modal`).
  pub mesh_open: bool,
  /// Instant d'ouverture du panneau : `.mesh-preview-bg` n'existe à l'écran qu'avec la fenêtre, son `meshFloat` repart de zéro.
  pub mesh_opened: Option<std::time::Instant>,
  /// Sub-panel toggled by "Configure" on the "Sync Windows wallpaper" row.
  pub wallpaper_sync_open: bool,
  pub order_drag: Option<(OrderList, usize)>,
  /// `translationEnabled` de `config.json`, lu au lancement.
  pub translation: bool,
  pub themes: Vec<crate::theme::file::ThemeEntry>,
}

/// Configuration updates for Windows wallpaper synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperSyncMessage {
  OutputMode(OutputMode),
  BlurRefWidth(u32),
  ColorMixRatio(u8),
  Brightness(u8),
  GlowSizePercent(u32),
  Dither(u8),
  GradientHeight(u32),
  TransitionEnabled(bool),
  TransitionDuration(u8),
  TransitionFps(u32),
  TransitionEasing(TransitionEasing),
  TransitionAnchor(TransitionAnchor),
  SkipWhenObscured(bool),
  PrefetchEnabled(bool),
  PrefetchEager(bool),
  CacheEnabled(bool),
  ForceFillStyle(bool),
  RestoreOnStop(bool),
  Reset,
}

/// Messages de la page.
#[derive(Debug, Clone)]
pub enum SettingsMessage {
  OpenCategory(Category),
  Search(String),
  SetBool(BoolKey, bool),
  /// Déplacement d'un curseur : appliqué sans écriture.
  SetInt(IntKey, i32),
  /// Relâchement d'un curseur : écriture.
  CommitInt,
  SetText(TextKey, String),
  AccentInput(String),
  /// Bouton « + » des pastilles d'accent.
  AccentToggle,
  /// Bouton « Configure » de « Mesh gradient brightness ».
  MeshToggle,
  /// Bouton « Configure » de « Sync Windows wallpaper ».
  WallpaperSyncTogglePanel,
  WallpaperSync(WallpaperSyncMessage),
  OrderPress(OrderList, usize),
  OrderEnter(OrderList, usize),
  OrderRelease,
  OrderReset(OrderList),
  /// Boutons « haut » et « bas » d'une rangée : de la position `from` à la position `to`.
  OrderMove(OrderList, usize, usize),
  /// Bouton « Reset » unique de `#btn-reset-sidebar-order` : remet la barre et l'accueil dans l'ordre d'origine.
  OrderResetAll,
  /// `#btn-toggle-adv-section` : replie ou déplie la section Avancé (`riff_adv_open`).
  ToggleAdvanced,
  /// Bouton « Reconnect » de « Discord RPC » : relance la connexion locale.
  DiscordReconnect,
  /// « Reset avatar » du compte : revient à l'icône par défaut.
  ResetAvatar,
  UnblockArtist(usize),
  UnblockTrack(usize),
  ThemesListed(Vec<crate::theme::file::ThemeEntry>),
  /// Paroles, visualiseurs et fonds.
  Visual(crate::prefs_visual::VisualMessage),
}

fn clamped(key: IntKey, value: i32) -> i32 {
  let (low, high) = key.range();
  value.clamp(low, high)
}

fn hex(color: Rgb) -> String {
  format!("#{:02X}{:02X}{:02X}", color.red, color.green, color.blue)
}

fn moved<T>(items: &mut Vec<T>, from: usize, to: usize) -> bool {
  if from == to || from >= items.len() || to >= items.len() {
    return false;
  }
  let item = items.remove(from);
  items.insert(to, item);
  true
}

impl App {
  /// Échelle de l'interface. 105 %, le défaut de Riff, correspond à 1.
  #[must_use]
  pub fn scale_factor(&self) -> f32 {
    self.settings.ui_scale as f32 / REFERENCE_SCALE
  }

  /// Valeur d'un réglage à deux états.
  #[must_use]
  pub fn bool_pref(&self, key: BoolKey) -> bool {
    let settings = &self.settings;
    match key {
      BoolKey::CompactEngine => settings.compact_engine_btns,
      BoolKey::HideEngine => settings.hide_engine_btns,
      BoolKey::PlayBtnBg => settings.play_btn_bg,
      BoolKey::ShowExplicit => settings.show_explicit,
      BoolKey::IntensityBg => settings.intensity_bg,
      BoolKey::HighlightButtons => settings.highlight_buttons,
      BoolKey::SearchAlbums => settings.search_albums,
      BoolKey::ClassicRows => settings.classic_track_rows,
      BoolKey::ShowLocal => settings.show_local,
      BoolKey::ShowAlbums => settings.show_albums,
      BoolKey::HideTitlebar => settings.hide_titlebar,
      BoolKey::PerfMode => settings.perf_mode,
      BoolKey::ReduceMotion => crate::motion::reduced(),
      BoolKey::HomeWave => settings.home_wave,
      BoolKey::HomeRecent => settings.home_recent,
      BoolKey::HomeArtists => settings.home_artists,
      BoolKey::ShowYtPlatform => self.data.show_yt_platform,
      BoolKey::SearchArtists => self.data.search_artists,
      BoolKey::LikeAlbumTracks => self.data.like_album_tracks,
      BoolKey::Autoplay => self.data.autoplay,
      BoolKey::Normalize => self.data.normalize,
      BoolKey::Translation => self.ui.prefs.translation,
      BoolKey::SmoothPresets => settings.sound.smooth_presets,
      BoolKey::PersistSound => settings.sound.persist,
      BoolKey::TrayOnClose => settings.tray_on_close,
      BoolKey::MediaSession => settings.media_session,
      BoolKey::DiscordRpc => settings.discord_rpc,
      BoolKey::DiscordArtist => settings.discord_artist_status,
      BoolKey::UpdateCheck => settings.update_check,
      BoolKey::WallpaperSync => settings.wallpaper_sync.enabled,
    }
  }

  /// Valeur d'un réglage numérique.
  #[must_use]
  pub fn int_pref(&self, key: IntKey) -> i32 {
    match key {
      IntKey::UiScale => self.settings.ui_scale as i32,
      IntKey::Brightness => i32::from(self.settings.theme_brightness),
      IntKey::Intensity => i32::from(self.settings.theme_intensity),
      IntKey::Hue => self
        .settings
        .theme_accent
        .parse::<Rgb>()
        .map_or(0, |accent| Hct::new(accent).get_hue().round() as i32),
      IntKey::Crossfade => self.data.crossfade as i32,
    }
  }

  /// Valeur d'un réglage texte.
  #[must_use]
  pub fn text_pref(&self, key: TextKey) -> &str {
    match key {
      TextKey::UiLang => &self.settings.ui_lang,
      TextKey::TrLang => &self.settings.tr_lang,
      TextKey::ThemeMode => match self.settings.theme_mode {
        ThemeMode::Dark => "dark",
        ThemeMode::Light => "light",
        ThemeMode::Auto => "auto",
      },
      TextKey::PaletteStyle => &self.settings.palette_style,
      TextKey::SearchPos => &self.data.search_pos,
      TextKey::HomeLayout => &self.settings.home_layout,
      TextKey::AudioQuality => &self.data.audio_quality,
      TextKey::Nickname => &self.settings.nickname,
      TextKey::JamRelay => &self.settings.jam_relay,
    }
  }

  /// Langue cible de la traduction des paroles (`trLang`).
  #[must_use]
  pub fn translation_target(&self) -> &str {
    &self.settings.tr_lang
  }

  /// Recalcule le thème après un changement de réglage. Un thème de fichier reste tel quel.
  pub(crate) fn refresh_theme(&mut self) {
    if self.settings.native_theme.is_some() {
      return;
    }
    self.theme = match (&self.theme.source, self.follows_cover()) {
      (ThemeSource::Cover(source), true) => ActiveTheme::from_cover(*source, &self.settings),
      _ => ActiveTheme::from_accent(&self.settings),
    };
  }

  pub(crate) fn save_settings(&self) -> Vec<Effect> {
    vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
  }

  fn set_bool(&mut self, key: BoolKey, value: bool) -> Vec<Effect> {
    let settings = &mut self.settings;
    match key {
      BoolKey::SmoothPresets => settings.sound.smooth_presets = value,
      BoolKey::PersistSound => settings.sound.persist = value,
      BoolKey::TrayOnClose => settings.tray_on_close = value,
      BoolKey::MediaSession => settings.media_session = value,
      BoolKey::DiscordRpc => settings.discord_rpc = value,
      BoolKey::DiscordArtist => settings.discord_artist_status = value,
      BoolKey::UpdateCheck => settings.update_check = value,
      BoolKey::WallpaperSync => settings.wallpaper_sync.enabled = value,
      BoolKey::CompactEngine => settings.compact_engine_btns = value,
      BoolKey::HideEngine => settings.hide_engine_btns = value,
      BoolKey::PlayBtnBg => settings.play_btn_bg = value,
      BoolKey::ShowExplicit => settings.show_explicit = value,
      BoolKey::IntensityBg => {
        settings.intensity_bg = value;
        self.refresh_theme();
      }
      BoolKey::HighlightButtons => {
        settings.highlight_buttons = value;
        self.refresh_theme();
      }
      BoolKey::SearchAlbums => settings.search_albums = value,
      BoolKey::ClassicRows => settings.classic_track_rows = value,
      BoolKey::ShowLocal => {
        settings.show_local = value;
        return self.leave_hidden_tab(value, Screen::Library);
      }
      BoolKey::ShowAlbums => {
        settings.show_albums = value;
        return self.leave_hidden_tab(value, Screen::Albums);
      }
      BoolKey::HideTitlebar => settings.hide_titlebar = value,
      BoolKey::PerfMode => {
        settings.perf_mode = value;
        crate::motion::set_perf(value);
      }
      BoolKey::ReduceMotion => {
        settings.reduce_motion = Some(value);
        crate::motion::set_reduced(value);
      }
      BoolKey::HomeWave => settings.home_wave = value,
      BoolKey::HomeRecent => settings.home_recent = value,
      BoolKey::HomeArtists => settings.home_artists = value,
      BoolKey::ShowYtPlatform => {
        self.data.show_yt_platform = value;
        return vec![self.data_changed()];
      }
      BoolKey::SearchArtists => {
        self.data.search_artists = value;
        return vec![self.data_changed()];
      }
      BoolKey::LikeAlbumTracks => {
        self.data.like_album_tracks = value;
        return vec![self.data_changed()];
      }
      BoolKey::Autoplay => {
        self.data.autoplay = value;
        return vec![self.data_changed()];
      }
      BoolKey::Normalize => {
        self.data.normalize = value;
        let mut effects = vec![self.data_changed()];
        if !value {
          self.normalize = riff_audio::normalize::NormalizeState::default();
          effects.push(Effect::NormalizeGain(0.0));
        }
        return effects;
      }
      BoolKey::Translation => {
        self.ui.prefs.translation = value;
        self.lyrics.translation_enabled = value;
        // L'écriture du consentement passe avant la demande : le core relit `translationEnabled` à chaque requête.
        let mut effects = vec![Effect::SetTranslation(value)];
        if value && self.settings.ui_lang != "en" {
          effects.push(Effect::LoadLanguage(self.settings.ui_lang.clone()));
        }
        if !value {
          if self.lyrics.translate_on {
            self.stop_translation();
          }
        } else if self.settings.visual.tr_enabled
          && self.settings.visual.tr_auto
          && !self.lyrics.translate_on
        {
          effects.extend(self.start_translation());
        }
        return effects;
      }
    }
    self.save_settings()
  }

  /// `switchView('discover')` : on quitte un onglet qu'on vient de masquer.
  fn leave_hidden_tab(&mut self, shown: bool, screen: Screen) -> Vec<Effect> {
    let mut effects = self.save_settings();
    if !shown && self.screen == screen {
      effects.extend(self.update(Message::Navigate(Screen::Home)));
    }
    effects
  }

  fn set_int(&mut self, key: IntKey, value: i32) -> Vec<Effect> {
    let value = clamped(key, value);
    match key {
      IntKey::UiScale => self.settings.ui_scale = value as u32,
      IntKey::Brightness => {
        self.settings.theme_brightness = value as u8;
        self.refresh_theme();
      }
      IntKey::Intensity => {
        self.settings.theme_intensity = value as u8;
        self.refresh_theme();
      }
      IntKey::Hue => {
        if let Ok(accent) = self.settings.theme_accent.parse::<Rgb>() {
          let mut color = Hct::new(accent);
          color.set_hue(f64::from(value));
          self.settings.theme_accent = hex(color.into());
          self.ui.prefs.accent_input = None;
          self.refresh_theme();
        }
      }
      IntKey::Crossfade => {
        self.data.crossfade = value as u32;
        self.player.crossfade = value as u32;
      }
    }
    Vec::new()
  }

  fn set_text(&mut self, key: TextKey, value: String) -> Vec<Effect> {
    match key {
      TextKey::UiLang => return self.update(Message::SetLanguage(value)),
      TextKey::ThemeMode => {
        let mode = match value.as_str() {
          "light" => ThemeMode::Light,
          "auto" => ThemeMode::Auto,
          _ => ThemeMode::Dark,
        };
        return self.update(Message::SetThemeMode(mode));
      }
      // `app.js:18512` : une traduction en cours repart dans la nouvelle langue.
      TextKey::TrLang => {
        self.settings.tr_lang = value;
        let mut effects = self.save_settings();
        if self.lyrics.translate_on {
          effects.extend(self.request_translation());
        }
        return effects;
      }
      TextKey::PaletteStyle => {
        self.settings.palette_style = value;
        self.refresh_theme();
      }
      TextKey::SearchPos => {
        self.data.search_pos = value;
        return vec![self.data_changed()];
      }
      TextKey::HomeLayout => self.settings.home_layout = value,
      TextKey::AudioQuality => {
        self.data.audio_quality = value;
        return vec![self.data_changed()];
      }
      TextKey::Nickname => self.settings.nickname = value,
      TextKey::JamRelay => self.settings.jam_relay = value,
    }
    self.save_settings()
  }

  fn order_entries(&self, list: OrderList) -> Vec<String> {
    match list {
      OrderList::Sidebar => self.sidebar_order(),
      OrderList::Home => self.settings.home_order.clone(),
    }
  }

  fn move_order(&mut self, list: OrderList, from: usize, to: usize) -> Vec<Effect> {
    let mut order = self.order_entries(list);
    if !moved(&mut order, from, to) {
      return Vec::new();
    }
    match list {
      OrderList::Sidebar => {
        self.data.sidebar_order = order;
        vec![self.data_changed()]
      }
      OrderList::Home => {
        self.settings.home_order = order;
        self.save_settings()
      }
    }
  }

  /// Messages de la page.
  pub fn update_settings(&mut self, message: SettingsMessage) -> Vec<Effect> {
    match message {
      SettingsMessage::OpenCategory(category) => {
        self.show_settings_category(category.key());
        self.ui.prefs.query.clear();
        let mut effects = self.save_settings();
        if category == Category::CustomThemes {
          effects.push(Effect::ListSettingsThemes);
        }
        if category == Category::Plugins {
          effects.push(Effect::ScanPlugins);
        }
        if matches!(
          category,
          Category::Integrations | Category::Storage | Category::Advanced | Category::Account
        ) {
          effects.extend(self.settings_opened());
        }
        effects
      }
      SettingsMessage::Search(query) => {
        self.ui.prefs.query = query;
        Vec::new()
      }
      SettingsMessage::SetBool(key, value) => self.set_bool(key, value),
      SettingsMessage::SetInt(key, value) => self.set_int(key, value),
      SettingsMessage::CommitInt => {
        let mut effects = self.save_settings();
        effects.push(self.data_changed());
        effects
      }
      SettingsMessage::SetText(key, value) => self.set_text(key, value),
      SettingsMessage::AccentInput(value) => {
        let parsed = value.trim().parse::<Rgb>().ok();
        self.ui.prefs.accent_input = Some(value);
        match parsed {
          Some(color) => {
            self.settings.theme_accent = hex(color);
            self.refresh_theme();
            self.save_settings()
          }
          None => Vec::new(),
        }
      }
      SettingsMessage::AccentToggle => {
        self.ui.prefs.accent_open = !self.ui.prefs.accent_open;
        Vec::new()
      }
      SettingsMessage::MeshToggle => {
        self.ui.prefs.mesh_open = !self.ui.prefs.mesh_open;
        self.ui.prefs.mesh_opened = self.ui.prefs.mesh_open.then(|| {
          self.now = std::time::Instant::now();
          self.now
        });
        Vec::new()
      }
      SettingsMessage::WallpaperSyncTogglePanel => {
        self.ui.prefs.wallpaper_sync_open = !self.ui.prefs.wallpaper_sync_open;
        Vec::new()
      }
      SettingsMessage::WallpaperSync(msg) => {
        let ws = &mut self.settings.wallpaper_sync;
        match msg {
          WallpaperSyncMessage::OutputMode(mode) => ws.output_mode = mode,
          WallpaperSyncMessage::BlurRefWidth(px) => ws.blur_ref_width = px.max(1),
          WallpaperSyncMessage::ColorMixRatio(pct) => ws.color_mix_ratio = pct.min(100),
          WallpaperSyncMessage::Brightness(pct) => {
            ws.brightness = f32::from(pct.clamp(20, 100)) / 100.0;
          }
          WallpaperSyncMessage::GlowSizePercent(pct) => ws.glow_size_percent = pct.clamp(40, 100),
          WallpaperSyncMessage::Dither(tenths) => ws.dither = f32::from(tenths.min(20)) / 10.0,
          WallpaperSyncMessage::GradientHeight(px) => ws.gradient_height = px,
          WallpaperSyncMessage::TransitionEnabled(v) => ws.transition_enabled = v,
          WallpaperSyncMessage::TransitionDuration(tenths) => {
            ws.transition_duration = f32::from(tenths.clamp(5, 50)) / 10.0;
          }
          WallpaperSyncMessage::TransitionFps(fps) => ws.transition_fps = fps.clamp(1, 240),
          WallpaperSyncMessage::TransitionEasing(easing) => ws.transition_easing = easing,
          WallpaperSyncMessage::TransitionAnchor(anchor) => ws.transition_anchor = anchor,
          WallpaperSyncMessage::SkipWhenObscured(v) => ws.skip_when_obscured = v,
          WallpaperSyncMessage::PrefetchEnabled(v) => ws.prefetch_enabled = v,
          WallpaperSyncMessage::PrefetchEager(v) => ws.prefetch_eager = v,
          WallpaperSyncMessage::CacheEnabled(v) => ws.cache_enabled = v,
          WallpaperSyncMessage::ForceFillStyle(v) => ws.force_fill_style = v,
          WallpaperSyncMessage::RestoreOnStop(v) => ws.restore_on_stop = v,
          WallpaperSyncMessage::Reset => ws.reset_options(),
        }
        self.save_settings()
      }
      SettingsMessage::OrderPress(list, index) => {
        self.ui.prefs.order_drag = Some((list, index));
        Vec::new()
      }
      SettingsMessage::OrderEnter(list, index) => match self.ui.prefs.order_drag {
        Some((dragged, from)) if dragged == list && from != index => {
          self.ui.prefs.order_drag = Some((list, index));
          self.move_order(list, from, index)
        }
        _ => Vec::new(),
      },
      SettingsMessage::OrderRelease => {
        self.ui.prefs.order_drag = None;
        Vec::new()
      }
      SettingsMessage::OrderReset(OrderList::Sidebar) => {
        self.data.sidebar_order.clear();
        vec![self.data_changed()]
      }
      SettingsMessage::OrderReset(OrderList::Home) => {
        self.settings.home_order = DEFAULT_HOME_ORDER.iter().map(ToString::to_string).collect();
        self.save_settings()
      }
      SettingsMessage::OrderMove(list, from, to) => self.move_order(list, from, to),
      SettingsMessage::OrderResetAll => {
        let mut effects = self.update_settings(SettingsMessage::OrderReset(OrderList::Sidebar));
        effects.extend(self.update_settings(SettingsMessage::OrderReset(OrderList::Home)));
        effects
      }
      SettingsMessage::DiscordReconnect => {
        riff_core::discord::reconnect();
        Vec::new()
      }
      SettingsMessage::ToggleAdvanced => {
        self.settings.adv_open = !self.settings.adv_open;
        self.save_settings()
      }
      SettingsMessage::ResetAvatar => {
        self.settings.avatar = None;
        self.save_settings()
      }
      SettingsMessage::UnblockArtist(index) => {
        if index < self.data.blocked_artists.len() {
          self.data.blocked_artists.remove(index);
        }
        vec![self.data_changed()]
      }
      SettingsMessage::UnblockTrack(index) => {
        if index < self.data.blocked_tracks.len() {
          self.data.blocked_tracks.remove(index);
        }
        vec![self.data_changed()]
      }
      SettingsMessage::ThemesListed(themes) => {
        self.ui.prefs.themes = themes;
        Vec::new()
      }
      SettingsMessage::Visual(message) => self.update_visual(message),
    }
  }

  /// Sections de l'accueil à afficher, dans l'ordre choisi.
  #[must_use]
  pub fn home_sections(&self) -> Vec<&str> {
    self
      .settings
      .home_order
      .iter()
      .map(String::as_str)
      .filter(|id| match *id {
        "sp-made-sec" | "sp-trend-sec" => self.settings.home_wave,
        "sp-quick-sec" => self.settings.home_recent,
        "sp-artists-sec" => self.settings.home_artists,
        _ => true,
      })
      .collect()
  }

  /// `generateDailyMixes` : ni mix sans « My Wave », ni mix dans la disposition « riff ».
  #[must_use]
  pub fn wants_daily_mixes(&self) -> bool {
    self.settings.home_wave && self.settings.home_layout != "riff"
  }
}

/// Nom affiché d'une entrée de la barre.
#[must_use]
pub fn sidebar_entry_label(id: &str) -> &'static str {
  match id {
    "favorites" => "Liked songs",
    "albums" => "Albums",
    "local" => "My files",
    "history" => "History",
    "queue" => "Queue",
    _ => "Home",
  }
}

/// Nom affiché d'une section de l'accueil.
#[must_use]
pub fn home_section_label(id: &str) -> &'static str {
  match id {
    "sp-made-sec" => "Made for you",
    "sp-quick-sec" => "Home / Quick picks",
    "sp-trend-sec" => "Trending now",
    _ => "Favorite artists",
  }
}

/// Ordre par défaut de la barre, pour les tests et la réinitialisation.
#[must_use]
pub const fn default_sidebar() -> [&'static str; 6] {
  DEFAULT_SIDEBAR_ORDER
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::{I18n, UI_KEYS};
  use crate::settings::NativeSettings;
  use crate::window::PersistedWindowState;

  fn app() -> App {
    App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
  }

  /// Remet les interrupteurs de mouvement à zéro quand le test finit.
  fn motion_reset() -> impl Drop {
    struct Reset;
    impl Drop for Reset {
      fn drop(&mut self) {
        crate::motion::set_reduced(false);
        crate::motion::set_perf(false);
        crate::motion::inject_system_preference(None);
      }
    }
    Reset
  }

  fn send(app: &mut App, message: SettingsMessage) -> Vec<Effect> {
    app.update(Message::Settings(message))
  }

  fn saved(effects: &[Effect]) -> bool {
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::SaveSettings(_)))
  }

  const ALL_BOOLS: [BoolKey; 30] = [
    BoolKey::TrayOnClose,
    BoolKey::MediaSession,
    BoolKey::DiscordRpc,
    BoolKey::DiscordArtist,
    BoolKey::UpdateCheck,
    BoolKey::WallpaperSync,
    BoolKey::SmoothPresets,
    BoolKey::PersistSound,
    BoolKey::CompactEngine,
    BoolKey::HideEngine,
    BoolKey::PlayBtnBg,
    BoolKey::ShowExplicit,
    BoolKey::IntensityBg,
    BoolKey::HighlightButtons,
    BoolKey::SearchAlbums,
    BoolKey::ClassicRows,
    BoolKey::ShowLocal,
    BoolKey::ShowAlbums,
    BoolKey::HideTitlebar,
    BoolKey::PerfMode,
    BoolKey::ReduceMotion,
    BoolKey::HomeWave,
    BoolKey::HomeRecent,
    BoolKey::HomeArtists,
    BoolKey::ShowYtPlatform,
    BoolKey::SearchArtists,
    BoolKey::LikeAlbumTracks,
    BoolKey::Autoplay,
    BoolKey::Normalize,
    BoolKey::Translation,
  ];

  #[test]
  fn every_bool_setting_reads_back_what_was_written() {
    let mut app = app();
    for key in ALL_BOOLS {
      for value in [true, false] {
        send(&mut app, SettingsMessage::SetBool(key, value));
        assert_eq!(app.bool_pref(key), value, "{key:?}");
      }
    }
  }

  #[test]
  fn every_bool_setting_persists() {
    for key in ALL_BOOLS {
      let mut app = app();
      let next = !app.bool_pref(key);
      let effects = send(&mut app, SettingsMessage::SetBool(key, next));
      assert!(
        effects.iter().any(|effect| matches!(
          effect,
          Effect::SaveSettings(_) | Effect::ScheduleSave(_) | Effect::SetTranslation(_)
        )),
        "{key:?}: {effects:?}"
      );
    }
  }

  #[test]
  fn sliders_stay_inside_their_bounds() {
    let mut app = app();
    for key in [
      IntKey::UiScale,
      IntKey::Brightness,
      IntKey::Intensity,
      IntKey::Crossfade,
    ] {
      let (low, high) = key.range();
      send(&mut app, SettingsMessage::SetInt(key, high + 500));
      assert_eq!(app.int_pref(key), high, "{key:?}");
      send(&mut app, SettingsMessage::SetInt(key, low - 500));
      assert_eq!(app.int_pref(key), low, "{key:?}");
    }
  }

  #[test]
  fn a_slider_writes_only_on_release() {
    let mut app = app();
    assert!(send(&mut app, SettingsMessage::SetInt(IntKey::Brightness, 40)).is_empty());
    assert!(saved(&send(&mut app, SettingsMessage::CommitInt)));
  }

  #[test]
  fn brightness_changes_the_applied_theme() {
    let mut app = app();
    let before = app.theme.scheme.surface;
    send(&mut app, SettingsMessage::SetInt(IntKey::Brightness, 100));
    assert_ne!(app.theme.scheme.surface, before);
  }

  #[test]
  fn intensity_changes_the_applied_theme() {
    let mut app = app();
    let before = app.theme.scheme.primary;
    send(&mut app, SettingsMessage::SetInt(IntKey::Intensity, 20));
    assert_ne!(app.theme.scheme.primary, before);
  }

  #[test]
  fn the_palette_style_changes_the_applied_theme() {
    let mut app = app();
    let before = app.theme.scheme.secondary;
    let effects = send(
      &mut app,
      SettingsMessage::SetText(TextKey::PaletteStyle, "vibrant".to_string()),
    );
    assert_ne!(app.theme.scheme.secondary, before);
    assert!(saved(&effects));
  }

  #[test]
  fn a_theme_file_is_not_replaced_by_the_sliders() {
    let mut app = app();
    app.settings.native_theme = Some("fichier".to_string());
    let before = app.theme.scheme.surface;
    send(&mut app, SettingsMessage::SetInt(IntKey::Brightness, 100));
    assert_eq!(app.theme.scheme.surface, before);
  }

  #[test]
  fn the_translation_language_is_the_target() {
    let mut app = app();
    send(
      &mut app,
      SettingsMessage::SetText(TextKey::TrLang, "uk".to_string()),
    );
    assert_eq!(app.translation_target(), "uk");
  }

  #[test]
  fn the_translation_box_writes_the_core_config_and_loads_the_language() {
    let mut app = app();
    app.settings.ui_lang = "fr".to_string();
    let effects = send(
      &mut app,
      SettingsMessage::SetBool(BoolKey::Translation, true),
    );
    assert_eq!(
      effects,
      [
        Effect::SetTranslation(true),
        Effect::LoadLanguage("fr".to_string())
      ]
    );
    let effects = send(
      &mut app,
      SettingsMessage::SetBool(BoolKey::Translation, false),
    );
    assert_eq!(effects, [Effect::SetTranslation(false)]);
  }

  #[test]
  fn translation_is_off_by_default_and_explains_what_is_sent() {
    assert!(!app().bool_pref(BoolKey::Translation));
    assert!(TRANSLATION_HINT.contains("Google"));
    assert!(TRANSLATION_HINT.contains("DeepL"));
    assert!(TRANSLATION_HINT.contains("lyrics"));
  }

  #[test]
  fn turning_normalization_off_resets_the_gain() {
    let mut app = app();
    send(&mut app, SettingsMessage::SetBool(BoolKey::Normalize, true));
    let effects = send(
      &mut app,
      SettingsMessage::SetBool(BoolKey::Normalize, false),
    );
    assert!(effects.contains(&Effect::NormalizeGain(0.0)));
  }

  #[test]
  fn the_scale_factor_is_one_at_the_default_of_riff() {
    let mut app = app();
    assert!((app.scale_factor() - 1.0).abs() < f32::EPSILON);
    send(&mut app, SettingsMessage::SetInt(IntKey::UiScale, 126));
    assert!((app.scale_factor() - 1.2).abs() < 1e-5);
  }

  #[test]
  fn the_search_finds_a_setting_of_any_category() {
    let found = matching("crossf", |label| label.to_string());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].category, Category::Tracks);
    assert!(matching("   ", |label| label.to_string()).is_empty());
  }

  #[test]
  fn the_search_also_reads_the_translated_label() {
    let found = matching("Яркость", |label| {
      if label == "Brightness" {
        "Яркость".to_string()
      } else {
        label.to_string()
      }
    });
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].label, "Brightness");
  }

  #[test]
  fn each_category_has_at_least_one_entry() {
    for category in Category::ALL {
      assert!(in_category(category).next().is_some(), "{category:?}");
    }
  }

  #[test]
  fn the_mesh_switch_sits_under_theme_right_before_the_mesh_brightness() {
    let labels: Vec<&str> = in_category(Category::Theme)
      .map(|entry| entry.label)
      .collect();
    let brightness = labels.iter().position(|l| *l == "Brightness").unwrap();
    let switch = labels
      .iter()
      .position(|l| *l == "Background mesh gradient")
      .expect("sous Theme");
    assert!(switch > brightness, "après Brightness");
    assert_eq!(labels[switch + 1], "Mesh gradient brightness");
    assert!(
      in_category(Category::Advanced).all(|entry| entry.label != "Background mesh gradient"),
      "plus sous Advanced"
    );
  }

  #[test]
  fn sync_windows_wallpaper_sits_right_below_theme_mode() {
    let labels: Vec<&str> = in_category(Category::Theme)
      .map(|entry| entry.label)
      .collect();
    let mode_idx = labels.iter().position(|l| *l == "Theme mode").unwrap();
    assert_eq!(labels[mode_idx + 1], "Sync Windows wallpaper");
  }

  #[test]
  fn wallpaper_sync_panel_toggles_and_options_update_and_reset() {
    let mut app = app();
    assert!(!app.ui.prefs.wallpaper_sync_open);
    send(&mut app, SettingsMessage::WallpaperSyncTogglePanel);
    assert!(app.ui.prefs.wallpaper_sync_open);
    send(&mut app, SettingsMessage::WallpaperSyncTogglePanel);
    assert!(!app.ui.prefs.wallpaper_sync_open);

    send(
      &mut app,
      SettingsMessage::SetBool(BoolKey::WallpaperSync, true),
    );
    let effects = send(
      &mut app,
      SettingsMessage::WallpaperSync(WallpaperSyncMessage::OutputMode(OutputMode::Primary)),
    );
    assert!(saved(&effects));
    assert_eq!(app.settings.wallpaper_sync.output_mode, OutputMode::Primary);

    send(
      &mut app,
      SettingsMessage::WallpaperSync(WallpaperSyncMessage::Brightness(80)),
    );
    assert!((app.settings.wallpaper_sync.brightness - 0.8).abs() < 1e-6);

    send(
      &mut app,
      SettingsMessage::WallpaperSync(WallpaperSyncMessage::Reset),
    );
    assert!(app.settings.wallpaper_sync.enabled);
    assert_eq!(app.settings.wallpaper_sync.output_mode, OutputMode::Span);
    assert!((app.settings.wallpaper_sync.brightness - 0.6).abs() < 1e-6);
  }

  #[test]
  fn the_discord_rows_are_under_integrations_in_riff_order() {
    let labels: Vec<&str> = in_category(Category::Integrations)
      .map(|entry| entry.label)
      .collect();
    assert_eq!(
      labels[..4],
      [
        "Discord RPC",
        "Last.fm Scrobbling",
        "Show artist in Discord status",
        "Spotify"
      ]
    );
    assert!(in_category(Category::Advanced).all(|entry| !entry.label.contains("Discord")));
  }

  #[test]
  fn the_motion_rows_sit_where_riff_puts_them() {
    let interface: Vec<&str> = in_category(Category::Interface)
      .map(|entry| entry.label)
      .collect();
    let at = interface
      .iter()
      .position(|label| *label == "Performance mode")
      .expect("rangée du mode performance");
    assert_eq!(interface[at - 1], "Visualizer sensitivity");
    assert_eq!(interface[at + 1], "Customize sidebar & home layout");
    let advanced: Vec<&str> = in_category(Category::Advanced)
      .map(|entry| entry.label)
      .collect();
    let at = advanced
      .iter()
      .position(|label| *label == "Reduce motion")
      .expect("rangée de la réduction de mouvement");
    assert_eq!(advanced[at - 1], "Home layout");
  }

  #[test]
  fn the_motion_switches_apply_at_once_and_are_saved() {
    let mut app = app();
    let _reset = motion_reset();
    assert!(!app.bool_pref(BoolKey::ReduceMotion) && !app.bool_pref(BoolKey::PerfMode));
    let effects = send(
      &mut app,
      SettingsMessage::SetBool(BoolKey::ReduceMotion, true),
    );
    assert!(saved(&effects));
    assert!(crate::motion::reduced() && app.settings.reduce_motion == Some(true));
    let effects = send(&mut app, SettingsMessage::SetBool(BoolKey::PerfMode, true));
    assert!(saved(&effects));
    assert!(crate::motion::perf() && app.settings.perf_mode);
    send(
      &mut app,
      SettingsMessage::SetBool(BoolKey::ReduceMotion, false),
    );
    assert!(!crate::motion::reduced());
    assert_eq!(
      app.settings.reduce_motion,
      Some(false),
      "choix explicite gardé"
    );
  }

  #[test]
  fn a_new_app_applies_the_saved_settings_or_the_system_preference() {
    let _reset = motion_reset();
    let build = |settings: NativeSettings| {
      App::new(PersistedWindowState::default(), settings, I18n::default())
    };
    crate::motion::inject_system_preference(Some(true));
    let _app = build(NativeSettings::default());
    assert!(crate::motion::reduced(), "réglage absent : le système");
    let _app = build(NativeSettings {
      reduce_motion: Some(false),
      perf_mode: true,
      ..NativeSettings::default()
    });
    assert!(!crate::motion::reduced() && crate::motion::perf());
  }

  #[test]
  fn an_unknown_category_opens_the_first_one() {
    assert_eq!(Category::parse("rose"), Category::Interface);
    assert_eq!(Category::parse("custom-themes"), Category::CustomThemes);
  }

  #[test]
  fn opening_a_category_is_remembered() {
    let mut app = app();
    let effects = send(&mut app, SettingsMessage::OpenCategory(Category::Theme));
    assert_eq!(app.settings.settings_cat, "theme");
    assert!(saved(&effects));
  }

  #[test]
  fn the_plus_button_opens_and_closes_the_hex_field_without_changing_the_accent() {
    let mut app = app();
    let before = app.settings.theme_accent.clone();
    assert!(!app.ui.prefs.accent_open);
    assert!(send(&mut app, SettingsMessage::AccentToggle).is_empty());
    assert!(app.ui.prefs.accent_open);
    send(&mut app, SettingsMessage::AccentToggle);
    assert!(!app.ui.prefs.accent_open);
    assert_eq!(app.settings.theme_accent, before);
  }

  #[test]
  fn configure_opens_and_closes_the_mesh_panel() {
    let mut app = app();
    assert!(!app.ui.prefs.mesh_open);
    send(&mut app, SettingsMessage::MeshToggle);
    assert!(app.ui.prefs.mesh_open);
    send(&mut app, SettingsMessage::MeshToggle);
    assert!(!app.ui.prefs.mesh_open);
  }

  #[test]
  fn a_valid_accent_applies_and_an_invalid_one_waits() {
    let mut app = app();
    let before = app.settings.theme_accent.clone();
    assert!(send(&mut app, SettingsMessage::AccentInput("#12".to_string())).is_empty());
    assert_eq!(app.settings.theme_accent, before);
    assert!(saved(&send(
      &mut app,
      SettingsMessage::AccentInput("#5247d1".to_string())
    )));
    assert_eq!(app.settings.theme_accent, "#5247D1");
  }

  #[test]
  fn the_hue_slider_moves_the_accent() {
    let mut app = app();
    let before = app.settings.theme_accent.clone();
    send(&mut app, SettingsMessage::SetInt(IntKey::Hue, 300));
    assert_ne!(app.settings.theme_accent, before);
    assert!(
      (app.int_pref(IntKey::Hue) - 300).abs() <= 3,
      "{}",
      app.int_pref(IntKey::Hue)
    );
  }

  #[test]
  fn dragging_reorders_the_sidebar_and_stops_on_release() {
    let mut app = app();
    send(&mut app, SettingsMessage::OrderPress(OrderList::Sidebar, 0));
    send(&mut app, SettingsMessage::OrderEnter(OrderList::Sidebar, 2));
    assert_eq!(
      app.data.sidebar_order[..3],
      ["favorites", "albums", "discover"]
    );
    send(&mut app, SettingsMessage::OrderRelease);
    let before = app.data.sidebar_order.clone();
    send(&mut app, SettingsMessage::OrderEnter(OrderList::Sidebar, 0));
    assert_eq!(app.data.sidebar_order, before);
  }

  #[test]
  fn resetting_the_sidebar_gives_the_order_of_riff() {
    let mut app = app();
    app.data.sidebar_order = vec!["queue".to_string()];
    send(&mut app, SettingsMessage::OrderReset(OrderList::Sidebar));
    assert_eq!(app.sidebar_order(), default_sidebar());
  }

  #[test]
  fn the_arrows_move_a_row_and_the_single_reset_restores_both_lists() {
    let mut app = app();
    send(
      &mut app,
      SettingsMessage::OrderMove(OrderList::Sidebar, 0, 1),
    );
    assert_eq!(app.sidebar_order()[..2], ["favorites", "discover"]);
    send(&mut app, SettingsMessage::OrderMove(OrderList::Home, 3, 2));
    assert_ne!(app.settings.home_order, DEFAULT_HOME_ORDER);
    send(&mut app, SettingsMessage::OrderResetAll);
    assert_eq!(app.sidebar_order(), default_sidebar());
    assert_eq!(app.settings.home_order, DEFAULT_HOME_ORDER);
  }

  #[test]
  fn an_arrow_on_the_edge_row_changes_nothing() {
    let mut app = app();
    let before = app.sidebar_order();
    let effects = send(
      &mut app,
      SettingsMessage::OrderMove(OrderList::Sidebar, 0, 0),
    );
    assert_eq!(app.sidebar_order(), before);
    assert!(effects.is_empty());
  }

  #[test]
  fn dragging_reorders_the_home_sections_and_saves() {
    let mut app = app();
    send(&mut app, SettingsMessage::OrderPress(OrderList::Home, 3));
    let effects = send(&mut app, SettingsMessage::OrderEnter(OrderList::Home, 0));
    assert_eq!(app.settings.home_order[0], "sp-artists-sec");
    assert!(saved(&effects));
    send(&mut app, SettingsMessage::OrderRelease);
    send(&mut app, SettingsMessage::OrderReset(OrderList::Home));
    assert_eq!(app.settings.home_order, DEFAULT_HOME_ORDER);
  }

  #[test]
  fn a_section_can_be_turned_off() {
    let mut app = app();
    assert_eq!(app.home_sections().len(), 4);
    send(
      &mut app,
      SettingsMessage::SetBool(BoolKey::HomeRecent, false),
    );
    assert!(!app.home_sections().contains(&"sp-quick-sec"));
    send(
      &mut app,
      SettingsMessage::SetBool(BoolKey::HomeArtists, false),
    );
    assert!(!app.home_sections().contains(&"sp-artists-sec"));
    send(&mut app, SettingsMessage::SetBool(BoolKey::HomeWave, false));
    assert!(!app.home_sections().contains(&"sp-made-sec"));
  }

  #[test]
  fn daily_mixes_need_the_sections_layout_and_my_wave() {
    let mut app = app();
    assert!(app.wants_daily_mixes());
    send(
      &mut app,
      SettingsMessage::SetText(TextKey::HomeLayout, "riff".to_string()),
    );
    assert!(!app.wants_daily_mixes());
    send(
      &mut app,
      SettingsMessage::SetText(TextKey::HomeLayout, "spotify".to_string()),
    );
    send(&mut app, SettingsMessage::SetBool(BoolKey::HomeWave, false));
    assert!(!app.wants_daily_mixes());
  }

  #[test]
  fn unblocking_removes_the_entry() {
    let mut app = app();
    app.data.blocked_artists = vec!["a".to_string(), "b".to_string()];
    send(&mut app, SettingsMessage::UnblockArtist(0));
    assert_eq!(app.data.blocked_artists, ["b"]);
    send(&mut app, SettingsMessage::UnblockArtist(9));
    assert_eq!(app.data.blocked_artists, ["b"]);
  }

  #[test]
  fn every_label_is_a_translation_key() {
    for entry in ENTRIES {
      assert!(UI_KEYS.contains(&entry.label), "{}", entry.label);
      assert!(
        entry.hint.is_empty() || UI_KEYS.contains(&entry.hint),
        "{}",
        entry.hint
      );
    }
    for category in Category::ALL {
      assert!(UI_KEYS.contains(&category.label()), "{}", category.label());
    }
  }
}
