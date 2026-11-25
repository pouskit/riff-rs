//! État des écrans et messages de l'interface. La logique se trouve dans `update.rs`, les vues dans `screens/`.

use crate::user_data::{SavedAlbum, SortMode, Track};
use crate::widgets::list::VirtualList;
use iced::Point;
use iced::keyboard::Modifiers;
use riff_audio::Source;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Glisser du bord de la barre latérale. `start_x` est connu au premier déplacement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarDrag {
  pub start_width: u32,
  pub start_x: Option<f32>,
}

/// Pas d'une ligne de piste : boîte et écart de `.tracks-list`.
pub const ROW_HEIGHT: f32 = crate::theme::tokens::track_row::PITCH;
const INITIAL_LIST_HEIGHT: f32 = 600.0;

/// Liste de pistes affichée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListId {
  Favorites,
  History,
  Queue,
  Local,
  Playlist,
  Search,
  Album,
  Artist,
  Trending,
  Recent,
  Pinned,
}

impl ListId {
  /// Identifiant du `scrollable`.
  #[must_use]
  pub const fn scroll_id(self) -> &'static str {
    match self {
      Self::Favorites => "list-favorites",
      Self::History => "list-history",
      Self::Queue => "list-queue",
      Self::Local => "list-local",
      Self::Playlist => "list-playlist",
      Self::Search => "list-search",
      Self::Album => "list-album",
      Self::Artist => "list-artist",
      Self::Trending => "list-trending",
      Self::Recent => "list-recent",
      Self::Pinned => "list-pinned",
    }
  }

  /// Le glisser-déposer réordonne cette liste.
  #[must_use]
  pub const fn can_reorder(self) -> bool {
    matches!(self, Self::Favorites | Self::Playlist | Self::Queue)
  }
}

/// Filtre des résultats de recherche.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchFilter {
  #[default]
  All,
  Songs,
  Artists,
  Albums,
}

/// Carte d'artiste en tête des résultats.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArtistCard {
  pub name: String,
  pub avatar: Option<String>,
  pub fans: f64,
  pub verified: bool,
}

/// Résultats de recherche.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
  pub input: String,
  pub query: String,
  pub token: u64,
  pub tracks: Vec<Track>,
  pub albums: Vec<SavedAlbum>,
  pub artist: Option<ArtistCard>,
  pub filter: SearchFilter,
  pub has_more: bool,
  pub loading: bool,
  pub loading_more: bool,
  pub error: Option<String>,
  pub history_open: bool,
  /// Ligne de l'historique sous le pointeur (`.sh-row:hover`).
  pub hover: Option<usize>,
  /// Pochette du premier résultat à la place de la loupe (`.search-icon-cover`, `setSearchIconCover`).
  pub icon_cover: Option<String>,
  /// Carte d'album sous le pointeur (`.search-album-card:hover`).
  pub album_hover: Option<usize>,
  /// Défilement horizontal de la rangée d'albums (`updateRowFade`).
  pub albums_scroll: f32,
}

/// Rangées de l'accueil (`#sp-made`, `#sp-quick`, `#sp-trend`, `#sp-artists`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeRow {
  Made,
  Quick,
  Trending,
  Artists,
}

impl HomeRow {
  /// Identifiant du `scrollable` horizontal.
  #[must_use]
  pub const fn scroll_id(self) -> &'static str {
    match self {
      Self::Made => "home-made",
      Self::Quick => "home-quick",
      Self::Trending => "home-trending",
      Self::Artists => "home-artists",
    }
  }

  /// Index dans `HomeState::scroll` ; `#sp-quick` est une grille qui ne défile pas.
  #[must_use]
  pub const fn index(self) -> Option<usize> {
    match self {
      Self::Made => Some(0),
      Self::Quick => None,
      Self::Trending => Some(1),
      Self::Artists => Some(2),
    }
  }
}

/// Accueil.
#[derive(Debug, Clone, Default)]
pub struct HomeState {
  pub wave: Vec<Track>,
  pub trending: Vec<Track>,
  pub wave_loading: bool,
  pub wave_failed: bool,
  pub mixes_busy: bool,
  /// Nombre de mix annoncés par `planMixes` tant que `mixes_busy` (`state._plannedMixCount`).
  pub mixes_planned: usize,
  /// `/api/trending` en cours : `renderTrending` montre alors un indicateur de chargement.
  pub trending_loading: bool,
  pub started: bool,
  /// Carte sous le pointeur (`.sp-card:hover`).
  pub hover: Option<(HomeRow, usize)>,
  /// Levée de la pochette au survol : `transform 180ms ease` de `.sp-card-cover`, une piste par carte.
  pub lift: crate::motion::Tracks<(HomeRow, usize)>,
  /// Décalage horizontal des rangées `#sp-made`, `#sp-trend`, `#sp-artists` : fondus de bord (`updateRowFade`).
  pub scroll: [f32; 3],
}

/// Page d'album.
#[derive(Debug, Clone, Default)]
pub struct AlbumView {
  pub album: SavedAlbum,
  pub tracks: Vec<Track>,
  pub grid_covers: Vec<String>,
  pub loading: bool,
  pub error: Option<String>,
  pub run: u64,
}

impl AlbumView {
  /// Mix quotidien : ni enregistrement ni artiste cliquable.
  #[must_use]
  pub fn is_mix(&self) -> bool {
    self.album.kind.as_deref() == Some("Daily Mix")
  }
}

/// Onglet de la page d'artiste.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ArtistTab {
  #[default]
  Tracks,
  Albums,
}

/// Page d'artiste.
#[derive(Debug, Clone, Default)]
pub struct ArtistView {
  pub name: String,
  pub tracks: Vec<Track>,
  pub albums: Vec<SavedAlbum>,
  pub bio: String,
  pub banner: Option<String>,
  pub avatar: Option<String>,
  pub fans_line: String,
  pub fans: f64,
  pub verified: bool,
  pub expanded: bool,
  pub tab: ArtistTab,
  pub loading: bool,
  pub run: u64,
  pub failed: bool,
}

impl ArtistView {
  /// Nombre de titres affichés : 5, ou tous.
  #[must_use]
  pub fn visible_tracks(&self) -> usize {
    if self.expanded {
      self.tracks.len()
    } else {
      self.tracks.len().min(5)
    }
  }
}

/// Grille de cartes d'album hors recherche : vue « Albums », rangée d'albums de « Mes fichiers » ou onglet « Albums » d'un artiste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardGrid {
  Saved,
  Local,
  Artist,
}

/// Onglet de la bibliothèque de la barre latérale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SidebarTab {
  #[default]
  Playlists,
  Albums,
  Queue,
}

/// Onglet du panneau « Now playing and queue ».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QueueTab {
  #[default]
  Queue,
  Recent,
}

/// Panneau « Now playing and queue » : fermé à chaque lancement, jamais écrit sur disque.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueuePanel {
  pub open: bool,
  pub tab: QueueTab,
  /// Fondu et glissement de `.open` (`styles.css:10927-10940`).
  pub motion: crate::motion::PanelMotion,
}

/// Panneau du jam : fermé à chaque lancement, jamais écrit sur disque.
#[derive(Debug, Clone, Default)]
pub struct JamPanel {
  pub open: bool,
  pub motion: crate::motion::PanelMotion,
  /// Code d'invitation à rejoindre.
  pub code: String,
  /// Nom saisi ; vide : nom enregistré ou par défaut.
  pub name: String,
}

/// Fenêtre de saisie ou d'information.
#[derive(Debug, Clone, PartialEq)]
pub enum Modal {
  CreatePlaylist {
    name: String,
    tracks: Vec<Track>,
  },
  AddToPlaylist {
    tracks: Vec<Track>,
  },
  Rename {
    track: Box<Track>,
    title: String,
    artist: String,
  },
  TrackArt(Box<Track>),
  Biography {
    title: String,
    body: String,
  },
  Audio,
  Tools,
  Download,
  Import(crate::manage::ImportKind),
  /// Rapport d'un import depuis Riff ou depuis un compte `.rfacc` : des noms de clés, jamais de valeur.
  ImportReport(Box<crate::import_report::ImportReport>),
}

/// Pression sur une ligne, en attente de son relâchement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowPress {
  pub list: ListId,
  pub from: usize,
  pub over: usize,
}

/// Commandes sur une piste, depuis une ligne, un menu ou la barre de lecture.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackCommand {
  ToggleFavorite,
  AddToPlaylist,
  AddToQueue,
  /// Seulement dans un jam : juste après le titre en cours.
  PlayNext,
  PinToSidebar,
  PinToTop(String),
  Share,
  Rename,
  Art,
  Download,
  RemoveFromPlaylist,
  RemoveFromQueue(usize),
  RemoveLocal,
  BlockTrack,
  BlockArtist,
  OpenArtist(String),
}

/// Messages du lecteur.
#[derive(Debug, Clone)]
pub enum PlayerMessage {
  TogglePlay,
  Next,
  Prev,
  ToggleShuffle,
  ToggleRepeat,
  SeekPreview(f32),
  SeekRelease,
  SeekBy(f32),
  Volume(f32),
  VolumeBy(f32),
  ToggleMute,
  Resolved {
    token: u64,
    start: Duration,
    crossfade: Option<u32>,
    result: Result<Source, String>,
  },
  NormalizeTick,
  Level(f32),
  AutoplayReady(Vec<Track>),
  PlayMyWave,
}

/// Réponses JSON du core, par demande.
#[derive(Debug, Clone, PartialEq)]
pub enum ReplyTag {
  Vibe { refresh: bool },
  Hot,
  Trending,
  Search { token: u64 },
  SearchMore { token: u64 },
  SearchAlbums { token: u64 },
  SearchArtist { token: u64, via_track: bool },
  AlbumTracks { run: u64 },
  Artist { run: u64 },
  ArtistMeta { name: String },
  TrackCover { token: u64 },
  Manage(crate::manage::ManageReply),
  RadioRefill,
  Lyrics { token: u64 },
  LyricsWords { token: u64 },
  LyricsTranslated { token: u64 },
  LyricsSaved { success_message: &'static str },
  Ignored,
}

/// Messages des écrans.
#[derive(Debug, Clone)]
pub enum UiMessage {
  Reply(ReplyTag, Result<Value, String>),
  SaveDue(u64),
  // Lignes de piste.
  RowPressed(ListId, usize),
  RowEntered(ListId, usize),
  RowLeft(ListId, usize),
  RowReleased,
  MouseReleased,
  PressedElsewhere,
  RowActivated(ListId, usize),
  RowMenu(ListId, usize, Point),
  RowCommand(ListId, usize, TrackCommand),
  CurrentCommand(TrackCommand),
  CurrentMenu(Point),
  Track(Box<Track>, TrackCommand),
  Modifiers(Modifiers),
  ListScrolled(ListId, f32, f32),
  ClearSelection,
  SelectAll,
  SelectionToPlaylist,
  SelectionToFavorites,
  SelectionRemove,
  // Recherche.
  SearchInput(String),
  SearchSubmit,
  SearchClear,
  SearchFocus,
  HistoryHover(Option<usize>),
  SetPlatform(String),
  SetSearchFilter(SearchFilter),
  /// Page des résultats défilée : décalage et hauteur visible (`#main-content` scroll, `app.js:10417`).
  SearchScrolled(f32, f32),
  /// Pointeur entré (`true`) ou sorti (`false`) d'une carte d'album.
  SearchAlbumHover(usize, bool),
  SearchAlbumsScrolled(f32),
  /// Pointeur entré (`true`) ou sorti (`false`) d'une carte des vues « Albums » et « Mes fichiers ».
  CardHover(CardGrid, usize, bool),
  /// Page « Mes fichiers » défilée : décalage et hauteur visible.
  LocalScrolled(f32, f32),
  /// Page « Titres aimés » défilée : décalage et hauteur visible.
  FavoritesScrolled(f32, f32),
  /// Page « Détail d'album » défilée : décalage et hauteur visible.
  AlbumScrolled(f32, f32),
  /// Page du détail d'une playlist défilée : décalage et hauteur visible.
  PlaylistScrolled(f32, f32),
  /// Menu de tri de la playlist ouvert ou fermé (`#playlist-sort-dropdown.open`, `app.js:12419-12439`).
  PlaylistSortOpen(bool),
  /// Page « Historique » défilée : décalage et hauteur visible.
  HistoryScrolled(f32, f32),
  /// Page « File d'attente » défilée : décalage et hauteur visible.
  QueueScrolled(f32, f32),
  /// Rangée d'albums de « Mes fichiers » défilée (`updateRowFade`).
  LocalAlbumsScrolled(f32),
  /// Bouton « Copy error & diagnostics » (`#btn-copy-search-err`, `app.js:10004`).
  CopySearchError,
  HistoryOpen(usize),
  HistoryRemove(usize),
  HistoryClear,
  OpenSearchArtist,
  FocusSearch,
  // Accueil.
  HomeRefresh,
  /// Pointeur entré (`true`) ou sorti (`false`) d'une carte de l'accueil.
  HomeHover(HomeRow, usize, bool),
  /// Rangée de cartes défilée (`updateRowFade`).
  HomeRowScrolled(HomeRow, f32),
  /// Encart d'import Spotify : bouton « Open Integrations » et croix.
  SpotifyPromoOpen,
  SpotifyPromoClose,
  OpenMix(usize),
  RateMix(bool),
  MixReady {
    day: String,
    id: String,
    title: String,
    artists: Vec<String>,
    tracks: Vec<Track>,
  },
  MixesDone,
  // Pages.
  OpenAlbum(Box<SavedAlbum>),
  OpenSavedAlbum(usize),
  OpenSearchAlbum(usize),
  OpenArtistAlbum(usize),
  OpenLocalAlbum(String),
  ToggleSaveAlbum,
  OpenArtist(String),
  ArtistTab(ArtistTab),
  ArtistToggleExpanded,
  ArtistFollow,
  ArtistBlock,
  ArtistBio,
  OpenFollowedArtist(usize),
  // Filtres et tris.
  FavoritesQuery(String),
  LocalQuery(String),
  PlaylistQuery(String),
  SetFavoritesSort(SortMode),
  SetPlaylistSort(SortMode),
  // Fichiers locaux.
  PickFiles,
  PickFolder,
  LocalPicked(Vec<std::path::PathBuf>, bool),
  LocalFound(Vec<Track>),
  LocalRescanned(Vec<Track>),
  ClearHistory,
  ClearQueue,
  // Playlists.
  OpenPlaylist(String),
  NewPlaylist,
  ModalName(String),
  ModalTitle(String),
  ModalArtist(String),
  ModalConfirm,
  ModalChoose(String),
  ModalClose,
  PlaylistRename(String),
  PlaylistDescription(String),
  PlaylistPlay,
  /// « Play » de « Titres aimés » : dans l'ordre affiché.
  FavoritesPlay,
  /// Bascule de l'aléatoire de la liste ouverte (titres aimés ou playlist), mémorisée pour elle seule.
  ListShuffleToggle,
  FavoritesExport,
  /// Nom, description et affiche des titres aimés, éditables comme ceux d'une playlist.
  FavoritesRename(String),
  FavoritesDescription(String),
  FavoritesPickCover,
  FavoritesCoverPicked(Option<std::path::PathBuf>),
  PlaylistDelete,
  PlaylistDeleted(String),
  PlaylistPickCover,
  PlaylistCoverPicked(String, Option<std::path::PathBuf>),
  /// Clic droit sur l'affiche de la page (playlist ouverte ou titres aimés) : choisir une image, revenir à l'affiche par défaut.
  CoverMenu(Point),
  /// Revient à l'affiche par défaut de la page affichée.
  CoverReset,
  PlaylistExport(String),
  PlaylistExported(Result<String, String>),
  PlaylistMenu(String, Point),
  // Barre latérale.
  SidebarTab(SidebarTab),
  /// Titre de la bibliothèque joué seul (`playTrack(track, [track], 0, 'sidebar-pinned')`, `app.js:27309`).
  SidebarPlayTrack(Box<Track>),
  /// Rangée `.sb-lib-row` de la bibliothèque sous le pointeur (indice dans le flux).
  SidebarRowEntered(usize),
  SidebarRowLeft(usize),
  /// Défilement du flux de la bibliothèque (`fade-top`, `app.js:27454`).
  SidebarFeedScrolled(f32),
  SidebarMenu(String, Point),
  SidebarMove(String, i32),
  SidebarReset,
  SidebarDragStart,
  SidebarDragMoved(f32),
  SidebarDragEnd,
  SidebarToggle,
  SidebarEdgeHover(bool),
  // Panneau « Now playing and queue ».
  ToggleQueuePanel,
  QueuePanelTab(QueueTab),
  /// Index dans la file d'un titre « Next up ».
  QueuePanelPlay(usize),
  /// Rang dans les récents affichés.
  QueuePanelPlayRecent(usize),
  // Fenêtre.
  TitlebarHover(bool),
  /// Pointeur entré ou sorti de la pochette de la barre de lecture (`#playbar-artwork-wrapper`, `app.js:16830-16841`).
  CoverHover(bool),
  /// Pointeur entré (`true`) ou sorti d'un bouton rond de la barre de lecture, désigné par son glyphe.
  ControlHover(crate::motion::ControlKey, bool),
  /// L'élément de cette clé a quitté la vue sous le pointeur (liste rechargée, catégorie changée, instance remplacée à sa place) : aucune
  /// sortie ne viendra, son survol, son appui et ses ondes sont oubliés sans transition (`motion::ControlHover::forget`).
  ControlGone(crate::motion::ControlKey),
  /// Appui sur un bouton (`pointerdown`, avec le point du clic relatif au bouton et sa taille) ou relâchement (`pointerup`) : pose
  /// la classe `.pressed` et l'onde (`app.js:2607-2638`).
  Touch(crate::motion::ControlKey, Option<(iced::Point, iced::Size)>),
  /// Pointeur dans `#right-panel` : montre les commandes (`showRpControls`, `app.js:14243`).
  PanelMoved,
  PanelLeft,
  /// Échéance du cache des commandes, de cette génération.
  PanelControlsDue(u64),
  FileDropped(std::path::PathBuf),
  DropFlush(u64),
  FocusSaved(Option<usize>),
}

/// Commandes du panneau droit (`.rp-controls-visible`) : visibles tant que le pointeur bouge dans le panneau.
#[derive(Debug, Clone, Default)]
pub struct PanelControls {
  pub visible: bool,
  pub hovered: bool,
  pub last_move: Option<Instant>,
  /// Seule l'échéance de la dernière génération agit.
  pub generation: u64,
}

/// État des écrans, hors données enregistrées.
#[derive(Debug, Clone, Default)]
pub struct Ui {
  pub search: SearchState,
  pub home: HomeState,
  pub album: AlbumView,
  pub artist: ArtistView,
  pub playlist_id: Option<String>,
  pub favorites_query: String,
  pub local_query: String,
  pub playlist_query: String,
  /// Menu de tri du détail de playlist ouvert (ou de la page des titres aimés, qui a le même en-tête).
  pub playlist_sort_open: bool,
  /// Ordre d'affichage des listes triées et filtrées : indices dans les données.
  pub favorites_order: Vec<usize>,
  pub local_order: Vec<usize>,
  pub playlist_order: Vec<usize>,
  pub lists: HashMap<ListId, VirtualList>,
  /// Ligne de piste sous le pointeur (`.track-row:hover`).
  pub row_hover: Option<(ListId, usize)>,
  /// Fond de survol animé des lignes de titres : une piste par ligne, 0 au repos et 1 survolée.
  pub row_fade: crate::motion::Tracks<(ListId, usize)>,
  pub press: Option<RowPress>,
  pub modifiers: Modifiers,
  pub selection: HashSet<String>,
  pub selection_anchor: Option<usize>,
  pub selection_list: Option<ListId>,
  pub modal: Option<Modal>,
  pub sidebar_tab: SidebarTab,
  /// Rangée de la bibliothèque sous le pointeur (`.sb-lib-row:hover`).
  pub sidebar_hover: Option<usize>,
  /// Défilement vertical du flux de la bibliothèque, en pixels écran.
  pub sidebar_scroll: f32,
  pub queue_panel: QueuePanel,
  pub jam_panel: JamPanel,
  pub sidebar_drag: Option<SidebarDrag>,
  pub sidebar_edge_hovered: bool,
  pub seek_preview: Option<f32>,
  pub autoplay_found: Option<Vec<Track>>,
  pub autoplay_waiting: bool,
  pub autoplay_loading: bool,
  pub radio_loading: bool,
  pub radio_waiting: bool,
  /// Fondu à appliquer quand la lecture auto ou la radio auront trouvé le titre suivant.
  pub waiting_crossfade: Option<u32>,
  pub local_rescanned: bool,
  /// Carte sous le pointeur (`.search-album-card:hover`) dans « Albums » ou « Mes fichiers ».
  pub card_hover: Option<(CardGrid, usize)>,
  /// Défilement horizontal de la rangée d'albums de « Mes fichiers ».
  pub local_albums_scroll: f32,
  pub dropped: Vec<std::path::PathBuf>,
  pub drop_generation: u64,
  /// Ordinal du widget qui avait le focus avant la superposition.
  pub saved_focus: Option<usize>,
  /// Page de réglages.
  pub prefs: crate::prefs::SettingsUi,
  /// Éditeur de code ouvert (plugin ou thème).
  pub editor: Option<crate::code_editor::CodeEditor>,
  /// Éditeur de thème ouvert.
  pub theme_editor: Option<crate::theme_editor::ThemeDraft>,
  /// Fenêtre audio et son courant.
  pub audio: crate::audio_prefs::AudioUi,
  /// Écrans de gestion.
  pub manage: crate::manage::ManageUi,
  /// Fenêtre de téléchargement.
  pub download: crate::download_ui::DownloadUi,
  pub titlebar_hovered: bool,
  /// Survol animé des boutons ronds de la barre de lecture (`.btn-icon-pill`, `.control-btn`).
  pub controls: crate::motion::ControlHover,
  pub panel_controls: PanelControls,
}

/// Pas d'une ligne : celui de `track_row`, plus bas dans un album (sans vignette).
fn row_height(id: ListId) -> f32 {
  if id == ListId::Album {
    crate::theme::tokens::album_detail::ROW_HEIGHT + crate::theme::tokens::track_row::LIST_GAP
  } else {
    ROW_HEIGHT
  }
}

impl Ui {
  /// Défilement d'une liste.
  #[must_use]
  pub fn list(&self, id: ListId) -> VirtualList {
    self
      .lists
      .get(&id)
      .copied()
      .unwrap_or_else(|| VirtualList::new(row_height(id), INITIAL_LIST_HEIGHT))
  }
}
