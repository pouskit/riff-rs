//! Traductions : dictionnaire intégré, cache du core, puis chaîne anglaise.

use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Sous-dossier du cache de traduction dans le dossier de données.
pub const CACHE_DIR: &str = "i18n";

/// Chaînes affichées par le socle, envoyées à `/api/i18n-batch` si la traduction réseau est activée.
pub const UI_KEYS: &[&str] = &[
  "Edit",
  "Configure",
  "No blocked artists or tracks",
  "Hide the window titlebar and reveal it on top edge hover",
  "Not recommended: regular YouTube includes non-music videos and loads slower than YouTube Music",
  "Show only icons for YouTube, YouTube Music, and SoundCloud in the top bar",
  "Hide search engine buttons from the top bar",
  "Show or hide the rounded background behind the main play/pause button",
  "Morphing Material 3 shape with a wavy progress ring around the cover",
  "Displays [E] badge next to tracks with explicit content",
  "Show wave visualizer at the bottom even when track has lyrics",
  "Adjust transparency of the TV karaoke visualizer",
  "Blur of the ambient background in fullscreen karaoke",
  "Higher = calmer, slower bars",
  "Higher = taller bars",
  "Performance mode",
  "Disables visualizers, animated gradient and blur effects, limits animations to 30 FPS",
  "Reduce motion",
  "Replace spring animations with simple fades",
  "Tint background and surface panels with the accent color and intensity",
  "Fill the main button of each screen with the accent color",
  "Use rounder 20px corners and press animation on track rows",
  "Display saved albums in the sidebar and album page",
  "Automatically add all tracks of an album to favorites when saving it",
  "Blocked items are hidden from recommendations and auto-skipped",
  "All these tracks are blocked",
  "Automatically save High Quality, effects and the active preset",
  "How the active line moves",
  "Preferred lyrics provider with automatic fallback",
  "Source for karaoke word timings",
  "How the active line is highlighted in Word mode",
  "Shows albums in search results",
  "YouTube",
  "YouTube Music",
  "SoundCloud",
  "Apple Music",
  "Deezer",
  "Yandex Music",
  "Sync",
  "Word",
  "Line",
  "No lyrics",
  "Loading...",
  "Cancel",
  "Save",
  "Recording",
  "Restart",
  "Save and exit",
  "lines synced",
  "Tap a line or press Space on beat",
  "Add lyrics first using the edit button, then sync them",
  "Click or tap each line when you hear it",
  "Sync restarted",
  "Synced lyrics saved",
  "Lyrics saved successfully",
  "Lyrics removed",
  "Home",
  "Search",
  "Search tracks, artists, or albums...",
  "Liked songs",
  "Albums",
  "All",
  "Songs",
  "Artists",
  "My files",
  "History",
  "Queue",
  "Playlists",
  "Settings",
  "About",
  "Accent color",
  "Account",
  "Add a description...",
  "Add files",
  "Add folder",
  "Add to favorites",
  "Add to playlist",
  "Added to queue",
  "Advanced",
  "Apply intensity to background",
  "Artist",
  "Auto (album)",
  "Autoplay similar tracks",
  "Autoplay: similar tracks",
  "Banners",
  "Based on your taste",
  "Bio",
  "Blocked artists & tracks",
  "Brightness",
  "Cancel",
  "Classic",
  "Classic rounded track cards",
  "Clear",
  "Clear history",
  "Clear queue",
  "Close",
  "Compact engine buttons",
  "Couldn't load recommendations",
  "Create",
  "Create playlist",
  "Crossfade",
  "Custom themes",
  "Customize sidebar & home layout",
  "Dark",
  "Delete",
  "Delete playlist",
  "Dislike",
  "Do you like these tracks?",
  "Drag and drop to reorder sidebar navigation items and home page sections",
  "Export as JSON",
  "Exported 1 playlist",
  "Expressive",
  "Favorite artists",
  "Favorites",
  "For you",
  "Hide engine buttons",
  "Hide top titlebar",
  "High",
  "Highlight buttons",
  "Home / Quick picks",
  "Home sections",
  "Home layout",
  "Hue",
  "Integrations",
  "Intensity",
  "Interface",
  "Interface language",
  "Jump back in",
  "Left",
  "Light",
  "Like",
  "Like all album tracks when saving album",
  "Loading...",
  "Low",
  "Made for you",
  "Medium",
  "Middle",
  "My Wave recommendations",
  "Native themes",
  "Neutral",
  "Nickname",
  "No favourites yet. Use the heart on a track to save it",
  "No local tracks in library yet.",
  "No matching tracks. Try a different search",
  "No native theme found.",
  "No playlists yet",
  "No recently played tracks.",
  "No saved albums yet",
  "No saved albums yet. Save albums from the album view.",
  "Your library is empty. Add some music to get started",
  "No settings found.",
  "No tracks found for this artist.",
  "Off by default. When on, lyrics and interface texts are sent to Google, or to DeepL if a key is set.",
  "Palette style",
  "Play",
  "Play/Pause button background",
  "Playback queue",
  "Playing For you",
  "Playlist",
  "Playlist deleted",
  "Playlist name",
  "Playlist title",
  "Plugins",
  "Plugins are not available in this version.",
  "Popular Songs",
  "Position of searchbar",
  "Queue is empty. Play a track to start.",
  "Radio",
  "Refresh",
  "Remove",
  "Removed from Local Files",
  "Removed tracks from queue",
  "Rename track",
  "Reset order",
  "Riff",
  "Right",
  "Save",
  "Save at least 5 tracks to tune For you",
  "Search favorites...",
  "Search history",
  "Search in playlist...",
  "Search my files...",
  "Search settings…",
  "Searching...",
  "Select all",
  "Share link copied to clipboard!",
  "Show Albums tab",
  "Show My files tab",
  "Show YouTube platform",
  "Show artists in search",
  "Show explicit tag [E] on tracks",
  "Sidebar & home layout reset to default",
  "Sidebar navigation",
  "Song quality",
  "Storage",
  "Theme",
  "Theme mode",
  "This playlist is empty",
  "Tonal spot",
  "Track info updated",
  "Track title",
  "Tracks",
  "Translation language",
  "Trending now",
  "UI scale",
  "Vibrant",
  "Volume normalization",
  "YouTube Music is unavailable",
  "about",
  "account",
  "advanced",
  "auto",
  "banners",
  "custom-themes",
  "dark",
  "integrations",
  "interface",
  "light",
  "plugins",
  "storage",
  "theme",
  "tracks",
  "Translation",
  "10-band Equalizer",
  "Audio",
  "Audio FX",
  "Audio output device",
  "Audio settings",
  "Audio settings reset",
  "Last.fm API secret could not be saved securely; enter it again after restarting Riff.",
  "Last.fm login could not be saved securely; reconnect after restarting Riff.",
  "Bass Depth",
  "Default device",
  "Distortion",
  "Echo",
  "Enhancement Engine",
  "Enhancer",
  "Equalizer",
  "fyro1.1 (Dynamic Depth)",
  "glyphs-7B (High-End Upscaler)",
  "High Quality",
  "High-End Air",
  "hqmusic-3 (Harmonic Restorer)",
  "Keeps loudness similar between tracks (adjusts live, up to 9 dB)",
  "Mid",
  "Muffled",
  "Normalize volume",
  "Party bathroom",
  "Pitch",
  "Reset audio settings",
  "Reverb",
  "Speed",
  "Speed (pitch)",
  "Vocal Clarity",
  "Volume gain",
  "Output",
  "Preset name...",
  "Presets",
  "Reset",
  "Save preset",
  "Save sound settings",
  "Discord RPC",
  "Reconnect",
  "Discord status",
  "Discord time",
  "Discord cover",
  "Show when paused",
  "Album on cover hover",
  "Track",
  "Track - Artist",
  "Progress bar",
  "Elapsed",
  "Hidden",
  "Paused",
  "Show dynamic ambient color gradients behind interface panels",
  "Update check",
  "Adds a button that asks GitHub for the latest release. Nothing is sent and nothing is checked on its own.",
  "Check for updates",
  "Checking…",
  "You are up to date.",
  "No public release found.",
  "Version available:",
  "Open release page",
  "Download and install",
  "Downloading…",
  "Verifying…",
  "Installing: Riff closes and restarts.",
  "Update failed:",
  "Update check failed:",
  "Temporary web and partial data",
  "yt-dlp and ffmpeg",
  "Download failed",
  "Backup all playlists to a JSON file",
  "Restore playlists from exported JSON file",
  "Export Riff account (.rfacc)",
  "Backup full library, playlists, settings and history",
  "Import Riff account (.rfacc)",
  "Restore entire account from encrypted binary backup",
  "Create desktop & app menu shortcut",
  "Add Riff shortcut to your desktop and system applications menu",
  "Create shortcut",
  "Sign in",
  "Sign out",
  "Sign in to import your YouTube library",
  "Route Riff audio and stream extraction through a proxy without slowing down your whole system",
  "No proxies configured",
  "Audio bitrate and streaming format selection",
  "Minimize to tray on close",
  "Keep running in background when window is closed",
  "Legacy media key capture (Windows)",
  "Leave off to use the Windows media overlay and lock screen controls. Turning this on takes media keys away from the overlay",
  "Fetch synced and plain lyrics from network providers",
  "Highlight individual words in synchronized lyrics",
  "Smoothly follow the active lyric line during playback",
  "Low intensity makes the gradient colors grayer",
  "When the queue ends, keep playing more from the same artists, then your wave",
  "Show personal recommendations on the home page",
  "Recently played tracks on the home page",
  "Favorite artists on home page",
  "Spotify: rows of mixes and quick access. Riff: the carousel.",
  "Keyboard shortcuts",
  "Global shortcuts",
  "Logs",
  "Open logs folder",
  "Version",
  "Profile avatar",
  "Click your avatar above to pick a custom photo or reset to default",
  "Save your profile, playlists, favorites, history, and settings to a file",
  "Restore your entire Riff account from a .rfacc backup file",
  "Connected services",
  "Manage your accounts, import playlists and stream from linked platforms",
  "Reset avatar",
  "Manage integrations",
  "Add a .css file to restyle Riff.",
  "Tip: Riff sets its color variables from JavaScript, so a theme must use !important to override them (for example --md-sys-color-primary: #ff4081 !important).",
  "Ctrl+Shift+R turns every custom theme and CSS off if something breaks.",
  "Plugins are JavaScript files with full access to Riff.",
  "Only add plugins you trust. They live in the plugins folder: delete a file and the plugin is gone. Ctrl+Shift+R turns every plugin off.",
  "Open folder",
  "Reload list",
  "No plugins. Put .js files into the plugins folder or click Add.",
  "No themes yet. Click Add and choose a .css file.",
  "Old CSS theme of Riff: not importable.",
  "by",
  "Show/Hide",
  "Play/Pause",
  "Start with Windows",
  "Quit",
  "already used by another program",
  "Not available on this system.",
  "Not supported on this system",
  "Not installed",
  "Nothing playing",
  "Compact window",
  "Switch to compact window",
  "Create shortcuts",
  "Reinstall shortcuts",
  "Smooth preset transitions",
  "Speed & Pitch",
  "Sleep",
  "Sleep timer",
  "Sleep timer: end of track",
  "Off",
  "min",
  "End of track",
  "Turn off",
  "Fades out and pauses when the timer runs out.",
  "Open audio settings",
  "Installed",
  "Required tools",
  "Riff needs yt-dlp and ffmpeg to play and save tracks (~400 MB).",
  "Apply current audio effects (Preset/High Quality)",
  "Audio format",
  "Bitrate quality",
  "Could not read the audio",
  "Download file",
  "Download track",
  "Copy share link",
  "download & options",
  "Audio presets",
  "Compact mini player",
  "Lyrics",
  "Downloading...",
  "Drag the handles. The selection is also used by the download.",
  "Duration",
  "End",
  "Play selection",
  "Save trim",
  "Saved:",
  "Start",
  "Trim best part",
  "Add",
  "At most one check per day, contacts github.com.",
  "Cached audio and thumbnails will be downloaded again when needed.",
  "Change",
  "Change and move my data",
  "Change without moving",
  "Clear cache",
  "Client ID of your Spotify application. Allowed address: http://127.0.0.1/api/spotify-callback",
  "Connect in browser",
  "Connected",
  "Connected account",
  "Copy diagnostics",
  "Diagnostics",
  "Download folder",
  "Enabled",
  "Export",
  "Export all playlists (JSON)",
  "Google and YouTube",
  "Import",
  "Import Riff account",
  "Import as YouTube Music tracks",
  "Import cookies.txt",
  "Import into",
  "Import library",
  "Import playlists (JSON)",
  "Importing an account will replace your current data.",
  "Imports your liked videos and playlists. Read-only access.",
  "In-app proxy (YouTube / SoundCloud bypass)",
  "Last.fm Scrobbling",
  "Log in",
  "Log out",
  "Manage",
  "Music folder",
  "New playlist",
  "Not available: this build has no Google client ID.",
  "Open logs",
  "Please wait...",
  "Read from",
  "Restart Riff to use the new folder.",
  "Scrobble what I listen to",
  "Signed in",
  "Spotify",
  "Stored as yt-cookies.txt in your data folder, unencrypted. These cookies give access to your Google account.",
  "Test",
  "Update yt-dlp automatically",
  "Use a proxy",
  "YouTube Account",
  "Your Last.fm API key and secret. The secret goes in the system vault.",
  // Paroles, visualiseurs et fonds.
  "Lyrics translation",
  "Auto-translate",
  "Lyrics are sent to Google, or to DeepL if a key is set. Needs the Translation switch in Interface.",
  "Show wave visualizer in bottom bar",
  "Playbar visualizer position",
  "Show visualizer as a small icon on the right or wide behind the playbar",
  "Playbar visualizer opacity",
  "Visualizer sensitivity",
  "Visualizer smoothness",
  "Connected liquid visualizer",
  "Draw a smooth connected liquid wave instead of separate bars",
  "Shaped cover in the bottom bar",
  "Visualizer with lyrics",
  "Visualizer opacity",
  "Background blur",
  "Mesh gradient brightness",
  "Mesh gradient",
  "Animation speed",
  "First point, horizontal",
  "First point, vertical",
  "Second point, horizontal",
  "Second point, vertical",
  "Enable banners",
  "Show artwork backdrop banners in Now playing and playlists",
  "Track banners",
  "Show artwork backdrop banner for currently playing track",
  "Banner media",
  "Banner fit",
  "Banner blur",
  "Banner opacity",
  "Banner fade",
  "Background media",
  "Opacity",
  "Blur",
  "Cover background",
  "Uses a blurred version of the current cover as the background",
  "Cover background brightness",
  "Choose file",
  "Reset to track art",
  "Lyrics animation",
  "Lyrics animation speed",
  "Lyrics source",
  "Word-by-word source",
  "Word mode style",
  "Gradient tightness",
  "Mini lyrics window",
  "Mini lyrics always on top",
  "Online lyrics lookup",
  "Karaoke word highlighting",
  "Auto-scroll synced lyrics",
  "Background mesh gradient",
  "Mesh follows theme intensity",
  "Unsupported file: choose a PNG or JPEG image.",
  "Compact",
  "Behind bar",
  "Auto",
  "Estimate only (current)",
  "Ease in-out (Cubic)",
  "Smooth (Quad)",
  "Gentle (Sine)",
  "Ease out (Quart)",
  "Soft ease out",
  "Snappy (Expo)",
  "Linear",
  "Gradient",
  "Words",
  "Fit",
  "Zoom",
  "Stretch",
  "Change banner",
  "Customize cover & banner",
  "Change cover/banner for this track",
  "Art:",
  "Cover",
  "Banner",
  "Custom cover",
  "Custom banner",
  "Global banner",
  "Default art",
  "Custom banner effects",
  "Per-track blur, opacity and fade",
  "Download",
  "Download all",
  "Download all songs",
  "Download playlist",
  "Downloading",
  "Downloaded",
  "failed",
  "No remote tracks to download in this playlist",
  "Playlist link",
  "Export files",
  "Choose files",
  "No files selected",
  "Spotify → Account → Privacy → Download your data. When the archive arrives, pick Playlist1.json and YourLibrary.json here. No login needed.",
  "Go to privacy.apple.com > Request a copy of your data > Apple Music activity, unzip the archive, then pick the playlist/library files here. No login needed.",
  "Select at least Liked songs or Playlists",
  "No tracks found in the selected files",
  "No tracks selected or found to import",
  "No tracks found in selected files",
  "File too large (20 MB max)",
  "Found",
  "liked songs",
  "liked tracks",
  "playlists",
  "and",
  "Customize lyrics",
  "Toggle album cover",
  "Font",
  "Default",
  "SF Pro",
  "Serif",
  "Rounded",
  "Mono",
  "Condensed",
  "Alignment",
  "Align left",
  "Align center",
  "Align right",
  "Size",
  "Weight",
  "Line spacing",
  "Animation",
  "Smooth",
  "Slide",
  "Fade",
  "Pop",
  "None",
  "No-lyrics visualizer",
  "Connected wave style",
  "Now playing and queue",
  "Nothing is playing",
  "Nothing queued next",
  "Nothing played yet",
  "Allow multiple instances",
  "For testing: lets you start a second Riff window. Applies after restart. The second window uses its own clean profile.",
  "Could not save the setting.",
  "Import your music from Spotify, YouTube Music, SoundCloud, Apple & Yandex Music!",
  "Transfer your playlists and liked songs in a few clicks in Settings → Integrations",
  "Open Integrations",
  "Import your Riff data",
  "Riff data was found on this computer. Import it into this app, or start with a clean slate.",
  "Nothing is written until you choose. Your Riff data is never modified.",
  "Start fresh",
  "Import from Riff",
  "Local files",
  "Importing...",
  "Import report",
  "Imported",
  "Invalid value, default kept",
  "No native equivalent",
  "Unknown keys",
  "Secrets",
  "Stored in the system vault",
  "Not imported, enter it again",
  "The native font is fixed at build time",
  "The native interface has no global text scale",
  "Corner radii are set per widget",
  "The native window is opaque",
  "Blurring what is behind a panel is not available",
  "Icons are Unicode characters",
  "No SVG rendering for the home button icon",
  "The native progress bar has no wavy shape",
  "The native progress bar has no knob",
  "The native progress bar has no canvas background",
  "Cursors are set per widget, not globally",
  "The Advanced section is always visible natively",
  "There is a single collapsible sidebar natively",
  "Custom CSS is not supported natively (deliberate difference)",
  "Shaped covers in Now Playing and Karaoke are not ported",
  "The native home shows every section, without panes",
  "Updates are checked on click only, nothing to ignore",
  "No CSS effects to clip natively",
  "CSS page decoration has no native equivalent",
  "CSS ambient light has no native equivalent",
  "Extracting the cover colors for the mesh is not ported",
  "Visual effects are forced off in Riff and absent from the product",
  "Tip counter of the web interface",
  "Old name of the gradient tightness, read only when the new key is absent",
  "Cleared by Riff at every launch",
  "Cache replaced by an in-memory cache",
  "Riff internal migration marker",
  "Dead key, removed by Riff at launch",
  "Translation cache, rebuilt natively",
  "Cache, rebuilt on demand",
  "No link to share for this track",
  "New plugin",
  "Import plugin",
  "Open plugins folder",
  "No plugin found.",
  "No permission",
  "Error",
  "Changed since you enabled it. Check its permissions, then enable it again.",
  "JavaScript plugin from Riff, incompatible. See docs/plugins/PORTAGE.md to port it to Rhai.",
  "queue: read and add to the play queue",
  "library: read favorites and history",
  "search: search and get recommendations through Riff",
  "shuffle: choose the next track in shuffle",
  "notify: show notifications",
  "Plugins run in a sandbox: no file, network or system access, only the permissions listed below.",
  "Offline: no internet connection",
  "Failed to load search results",
  "Error while fetching tracks. Please report this to GitHub WITH logs / diagnostics.",
  "Copy error & diagnostics",
  "Shuffle",
  "No remote tracks to download in liked songs",
  "Pause",
  "Turn on shuffle",
  "Turn off shuffle",
  "Turn on repeat",
  "Turn off repeat",
  "Mute",
  "Unmute",
  "Remove from Favorites",
  "Remove dislike",
  "Previous",
  "Next",
  "Back",
  "Maximize",
  "Restore",
  "Minimize",
  "Save album",
  "Remove album",
  "Remove from queue",
  "Share link",
  "Unblock artist",
  "Unblock track",
  "Always on top",
  "Toggle cover",
  "Change cover",
  "Reset cover",
  "Jam",
  "Jam: listen together",
  "Jam: {count} participants",
  "Listen to the same music with friends. Everyone can add tracks, pause and skip.",
  "Your name",
  "Start a Jam",
  "Join a Jam",
  "Paste an invite code",
  "Paste",
  "Join",
  "Starting the Jam…",
  "Connecting…",
  "Reconnecting ({n}/3)…",
  "Reconnecting to the Jam…",
  "Invite code",
  "Copy",
  "Code copied",
  "Share this code with your friends. The connection is direct when possible, otherwise through an encrypted relay.",
  "Windows may ask you to allow Riff on the network the first time.",
  "Participants",
  "(host)",
  "Jam hosted by {name}",
  "End Jam",
  "Leave Jam",
  "Added by {name}",
  "Play next",
  "{name} joined the Jam",
  "{name} left the Jam",
  "{name} paused",
  "{name} resumed playback",
  "{name} added {title}",
  "{name} added {count} tracks",
  "{name} couldn't play this track",
  "Couldn't play this track, waiting for the next one",
  "You joined {name}'s Jam",
  "The host ended the Jam",
  "The Jam ended because everyone else left",
  "Connection to the Jam was lost",
  "Invalid invite code",
  "Wrong or expired invite code",
  "Code not found or the Jam ended",
  "Code expired, or check your system clock",
  "Jam code compromised, a new code was generated",
  "This Jam is full",
  "This Jam uses another version of Riff",
  "Could not start the Jam",
  "Relay unreachable: only direct connections will work",
  "Local files can't be played in a Jam",
  "{count} local files skipped",
  "{count} tracks over the limit were skipped",
  "The Jam queue is full",
  "Not available during a Jam",
  "Effects are shared in the Jam",
  "Set by {name}",
  "Only for you",
  "Jam relay URL",
  "Leave empty to use the default encrypted relays",
  "Jam rendezvous URL",
  "Leave empty to use the default server that resolves Jam codes",
  "Invalid relay URL, using the default relays",
  "Invalid rendezvous URL, short codes are unavailable",
  "Sync Windows wallpaper",
  "Syncs the Windows desktop wallpaper with the album art of the song playing",
  "Monitors",
  "Primary monitor only",
  "One render per monitor (span)",
  "Blur reference width",
  "Album colour mix",
  "Glow size",
  "Dithering",
  "Gradient resolution",
  "Full resolution",
  "Frame rate",
  "Easing",
  "Centred on the change",
  "Ends on the change",
  "Starts on the change",
  "Skip when the desktop is hidden",
  "Pre-render the next song",
  "Pre-render as soon as the next song is known",
  "Cache renders on disk",
  "Force the Windows wallpaper style",
  "Restore the previous wallpaper when disabled",
];

const ENGLISH: &str = "en";
const FR: &str = include_str!("../i18n/fr.json");
const RU: &str = include_str!("../i18n/ru.json");
const UK: &str = include_str!("../i18n/uk.json");

/// Dictionnaires d'une langue. Les clés sont les chaînes anglaises.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct I18n {
  lang: String,
  builtin: HashMap<String, String>,
  cache: HashMap<String, String>,
}

impl I18n {
  /// Charge la langue : dictionnaire intégré et cache `<cache_dir>/<lang>.json`, absent ou corrompu sans erreur.
  #[must_use]
  pub fn load(lang: &str, cache_dir: &Path) -> Self {
    let builtin = match lang {
      "fr" => parse_builtin(FR),
      "ru" => parse_builtin(RU),
      "uk" => parse_builtin(UK),
      _ => HashMap::new(),
    };
    Self {
      lang: lang.to_string(),
      builtin,
      cache: read_cache(lang, cache_dir),
    }
  }

  /// Langue active.
  #[must_use]
  pub fn lang(&self) -> &str {
    &self.lang
  }

  /// Traduit une chaîne anglaise. Absente de la langue active : l'anglais.
  #[must_use]
  pub fn t(&self, key: &str) -> String {
    self.translated(key).unwrap_or(key).to_string()
  }

  fn translated(&self, key: &str) -> Option<&str> {
    self
      .builtin
      .get(key)
      .or_else(|| self.cache.get(key))
      .map(String::as_str)
  }

  /// Clés sans traduction dans la langue active.
  #[must_use]
  pub fn missing<'a>(&self, keys: &[&'a str]) -> Vec<&'a str> {
    if self.lang == ENGLISH {
      return Vec::new();
    }
    keys
      .iter()
      .copied()
      .filter(|key| self.translated(key).is_none())
      .collect()
  }
}

/// La traduction réseau ne part que si l'utilisateur l'a activée et qu'il manque des chaînes.
#[must_use]
pub fn needs_network(config: &Value, i18n: &I18n, keys: &[&str]) -> bool {
  config.get("translationEnabled").and_then(Value::as_bool) == Some(true)
    && !i18n.missing(keys).is_empty()
}

fn parse_builtin(raw: &str) -> HashMap<String, String> {
  serde_json::from_str(raw).expect("dictionnaire intégré généré par scripts/extract-i18n.mjs")
}

fn read_cache(lang: &str, cache_dir: &Path) -> HashMap<String, String> {
  if lang.is_empty()
    || !lang
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
  {
    return HashMap::new();
  }
  let parsed = fs::read_to_string(cache_dir.join(format!("{lang}.json")))
    .ok()
    .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
  let Some(Value::Object(entries)) = parsed else {
    return HashMap::new();
  };
  entries
    .into_iter()
    .filter_map(|(key, value)| Some((key, value.as_str()?.to_string())))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;
  use serde_json::json;

  fn empty_cache() -> std::path::PathBuf {
    std::env::temp_dir().join("riff-app-i18n-absent")
  }

  fn cache_with(lang: &str, content: &str) -> TempDir {
    let dir = TempDir::new("app-i18n");
    std::fs::write(dir.join(format!("{lang}.json")), content).unwrap();
    dir
  }

  #[test]
  fn known_key_is_translated_in_russian() {
    let i18n = I18n::load("ru", &empty_cache());
    assert_eq!(i18n.t("Search settings..."), "Поиск настроек...");
    assert_eq!(i18n.lang(), "ru");
  }

  #[test]
  fn known_key_is_translated_in_ukrainian() {
    let i18n = I18n::load("uk", &empty_cache());
    assert_ne!(i18n.t("Search settings..."), "Search settings...");
  }

  #[test]
  fn unknown_key_gives_the_english_string() {
    let i18n = I18n::load("ru", &empty_cache());
    assert_eq!(
      i18n.t("Pas dans le dictionnaire"),
      "Pas dans le dictionnaire"
    );
  }

  #[test]
  fn english_gives_the_key() {
    let i18n = I18n::load("en", &empty_cache());
    assert_eq!(i18n.t("Search settings..."), "Search settings...");
  }

  #[test]
  fn missing_cache_file_is_not_an_error() {
    let i18n = I18n::load("de", &empty_cache());
    assert_eq!(i18n.t("Settings"), "Settings");
  }

  #[test]
  fn the_cache_translates_another_language() {
    let i18n = I18n::load(
      "de",
      &cache_with("de", r#"{ "Settings": "Einstellungen", "Bad": 12 }"#),
    );
    assert_eq!(i18n.t("Settings"), "Einstellungen");
    assert_eq!(i18n.t("Bad"), "Bad");
  }

  #[test]
  fn the_builtin_dictionary_comes_before_the_cache() {
    let i18n = I18n::load(
      "ru",
      &cache_with(
        "ru",
        r#"{ "Search settings...": "du cache", "Extra": "Экстра" }"#,
      ),
    );
    assert_eq!(i18n.t("Search settings..."), "Поиск настроек...");
    assert_eq!(i18n.t("Extra"), "Экстра");
  }

  #[test]
  fn a_corrupt_cache_is_ignored() {
    let i18n = I18n::load("fr", &cache_with("fr", "{ pas du json"));
    assert_eq!(i18n.t("Settings"), "Settings");
  }

  #[test]
  fn a_language_cannot_leave_the_cache_folder() {
    let dir = cache_with("de", r#"{ "Settings": "Einstellungen" }"#);
    let child = dir.join("enfant");
    std::fs::create_dir_all(&child).unwrap();
    let i18n = I18n::load("../de", &child);
    assert_eq!(i18n.t("Settings"), "Settings");
  }

  #[test]
  fn missing_lists_the_keys_without_translation() {
    let i18n = I18n::load("ru", &empty_cache());
    assert_eq!(
      i18n.missing(&["Search settings...", "Absente"]),
      vec!["Absente"]
    );
    assert!(
      I18n::load("en", &empty_cache())
        .missing(&["Absente"])
        .is_empty()
    );
  }

  #[test]
  fn network_translation_needs_the_user_consent() {
    let i18n = I18n::load("ru", &empty_cache());
    assert!(!needs_network(&json!({}), &i18n, &["Absente"]));
    assert!(!needs_network(
      &json!({ "translationEnabled": false }),
      &i18n,
      &["Absente"]
    ));
    assert!(needs_network(
      &json!({ "translationEnabled": true }),
      &i18n,
      &["Absente"]
    ));
  }

  #[test]
  fn network_translation_does_not_run_without_missing_keys() {
    let enabled = json!({ "translationEnabled": true });
    assert!(!needs_network(
      &enabled,
      &I18n::load("en", &empty_cache()),
      &["Absente"]
    ));
    assert!(!needs_network(
      &enabled,
      &I18n::load("ru", &empty_cache()),
      &["Search settings..."]
    ));
  }

  #[test]
  fn plugin_and_search_error_labels_are_translated() {
    let keys = [
      "New plugin",
      "Import plugin",
      "Open plugins folder",
      "No plugin found.",
      "No permission",
      "Error",
      "Offline: no internet connection",
      "Failed to load search results",
      "Copy error & diagnostics",
    ];
    for lang in ["ru", "uk"] {
      let i18n = I18n::load(lang, &empty_cache());
      for key in keys {
        assert!(UI_KEYS.contains(&key), "{key} absent de UI_KEYS");
        assert_ne!(i18n.t(key), key, "{key} non traduit en {lang}");
      }
    }
  }

  #[test]
  fn blocking_labels_are_translated() {
    let keys = ["All these tracks are blocked"];
    for lang in ["fr", "ru", "uk"] {
      let i18n = I18n::load(lang, &empty_cache());
      for key in keys {
        assert!(UI_KEYS.contains(&key), "{key} absent de UI_KEYS");
        assert!(
          i18n.translated(key).is_some(),
          "{key} non traduit en {lang}"
        );
      }
    }
  }

  #[test]
  fn jam_labels_are_translated() {
    let keys = [
      "Jam",
      "Jam: listen together",
      "Jam: {count} participants",
      "Start a Jam",
      "Join a Jam",
      "Paste an invite code",
      "Invite code",
      "Participants",
      "End Jam",
      "Leave Jam",
      "Added by {name}",
      "Play next",
      "{name} joined the Jam",
      "{name} added {count} tracks",
      "You joined {name}'s Jam",
      "The Jam ended because everyone else left",
      "Wrong or expired invite code",
      "Code not found or the Jam ended",
      "Code expired, or check your system clock",
      "Invalid relay URL, using the default relays",
      "Invalid rendezvous URL, short codes are unavailable",
      "Jam code compromised, a new code was generated",
      "Local files can't be played in a Jam",
      "Effects are shared in the Jam",
      "Set by {name}",
      "Only for you",
      "Jam relay URL",
      "Leave empty to use the default encrypted relays",
      "Jam rendezvous URL",
      "Leave empty to use the default server that resolves Jam codes",
    ];
    for lang in ["fr", "ru", "uk"] {
      let i18n = I18n::load(lang, &empty_cache());
      for key in keys {
        assert!(UI_KEYS.contains(&key), "{key} absent de UI_KEYS");
        assert!(
          i18n.translated(key).is_some(),
          "{key} non traduit en {lang}"
        );
      }
    }
  }

  #[test]
  fn discord_labels_are_translated() {
    let keys = [
      "Discord status",
      "Discord time",
      "Discord cover",
      "Show when paused",
      "Album on cover hover",
      "Track",
      "Track - Artist",
      "Progress bar",
      "Elapsed",
      "Hidden",
      "Paused",
    ];
    for lang in ["fr", "ru", "uk"] {
      let i18n = I18n::load(lang, &empty_cache());
      for key in keys {
        assert!(UI_KEYS.contains(&key), "{key} absent de UI_KEYS");
        assert!(
          i18n.translated(key).is_some(),
          "{key} non traduit en {lang}"
        );
      }
    }
  }

  #[test]
  fn every_jam_key_is_in_every_builtin_dictionary_with_its_placeholders() {
    let english = I18n::load(ENGLISH, &empty_cache());
    let fr = I18n::load("fr", &empty_cache());
    let ru = I18n::load("ru", &empty_cache());
    let uk = I18n::load("uk", &empty_cache());
    for key in fr.builtin.keys() {
      for (lang, dict) in [("ru", &ru), ("uk", &uk)] {
        assert!(dict.builtin.contains_key(key), "{key} absent de {lang}");
      }
      for placeholder in ["{name}", "{title}", "{count}", "{n}"] {
        for (lang, dict) in [("fr", &fr), ("ru", &ru), ("uk", &uk)] {
          assert_eq!(
            key.contains(placeholder),
            dict.t(key).contains(placeholder),
            "{placeholder} de {key} en {lang}"
          );
        }
      }
      assert_eq!(english.t(key), *key);
    }
    assert_eq!(fr.t("Copy"), "Copier");
  }

  #[test]
  fn wallpaper_sync_labels_are_translated() {
    let keys = [
      "Sync Windows wallpaper",
      "Syncs the Windows desktop wallpaper with the album art of the song playing",
      "Monitors",
      "Primary monitor only",
      "One render per monitor (span)",
      "Blur reference width",
      "Album colour mix",
      "Brightness",
      "Glow size",
      "Dithering",
      "Off",
      "Gradient resolution",
      "Full resolution",
      "Crossfade",
      "Duration",
      "Frame rate",
      "Easing",
      "Smooth",
      "Linear",
      "Alignment",
      "Centred on the change",
      "Ends on the change",
      "Starts on the change",
      "Skip when the desktop is hidden",
      "Pre-render the next song",
      "Pre-render as soon as the next song is known",
      "Cache renders on disk",
      "Force the Windows wallpaper style",
      "Restore the previous wallpaper when disabled",
      "Reset",
    ];
    for lang in ["fr", "ru", "uk"] {
      let i18n = I18n::load(lang, &empty_cache());
      for key in keys {
        assert!(UI_KEYS.contains(&key), "{key} absent de UI_KEYS");
        assert!(
          i18n.translated(key).is_some(),
          "{key} non traduit en {lang}"
        );
      }
    }
  }
}
