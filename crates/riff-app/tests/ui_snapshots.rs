//! Banc de rendu hors écran : écrit un PNG par écran de l'lication dans `target/ui-snapshots/`.
//!
//! Aucune fenêtre, aucun réseau, aucun fichier de l'utilisateur : l'état est construit à la main avec
//! des données factices, les pochettes sont des dégradés générés en mémoire, et les effets émis par
//! `App::update` ne sont jamais exécutés. Le rendu passe par `iced_test::Simulator` (moteur wgpu,
//! rendu sur une texture, sans surface ni fenêtre).
//!
//! Commande unique, depuis la racine du dépôt :
//!
//! ```text
//! cargo test -p riff-app --test ui_snapshots --locked -- --ignored --nocapture
//! ```
//!
//! Variables d'environnement facultatives :
//! - `RIFF_SNAPSHOT_QHD=1` : produit aussi chaque écran en 2560x1440 logique (`*-2560x1440.png`).
//! - `RIFF_SNAPSHOT_DIR=<dossier>` : dossier de sortie (défaut : `target/ui-snapshots`).
//! - `RIFF_SNAPSHOT_ONLY=<préfixe>` : ne rend que les scènes dont le nom commence par ce préfixe (ex. `04`).
//! - `ICED_TEST_BACKEND=<nom>` : moteur choisi par `iced_test` (lu par la crate).
//!
//! Scènes d'animation (`motion_scenes()`, numéros 41 et plus) : chaque instant `t` (en ms après le déclencheur) donne un `<scène>-tNNN.png`
//! (`t000`, `t170`, `t1380`), rendu depuis un `App` neuf dont l'horloge `App::now` vaut `t0 + t`. Si la scène nomme une capture
//! `reference` et que le dossier `ui-reference/motion/<reference>` existe (recherché en remontant les ancêtres de `CARGO_MANIFEST_DIR`,
//! sans erreur s'il manque), le banc écrit aussi `<scène>-tNNN-cote.png` (image de référence la plus proche de `t` à gauche, même zone du
//! rendu à droite) et, une fois par scène, `<scène>-reference.txt` (instant de chaque image de référence).
//!
//! Le simulateur rend à l'échelle 2 (fixe dans `iced_test` 0.14.0) : chaque image est réduite à la
//! taille logique demandée avant l'écriture, d'où des PNG de 1920x1080 pixels.

use iced::{Point, Settings, Size};
use iced_test::Simulator;
use image::imageops::FilterType;
use material_colors::color::Rgb;
use riff_app::app::{App, Message, Playback, theme_fade_targets};
use riff_app::audio_prefs::{AudioTab, SleepTimer};
use riff_app::i18n::I18n;
use riff_app::images::{Decoded, ImageKey, SIZE_CARD, SIZE_HERO, SIZE_ROW};
use riff_app::motion::{self, ThemeFade};
use riff_app::theme::{ActiveTheme, fonts, icons};
use riff_app::visuals::playbar::Wave;

use riff_app::prefs::{BoolKey, Category, SettingsMessage, TextKey};
use riff_app::screens::Screen;
use riff_app::settings::{NativeSettings, ThemeMode};
use riff_app::ui::{
  ArtistCard, ArtistTab, ArtistView, CardGrid, HomeRow, ListId, Modal, ReplyTag, SearchFilter,
  SidebarTab, TrackCommand, UiMessage,
};
use riff_app::user_data::{
  DailyMix, DailyMixes, Playlist, SavedAlbum, SearchEntry, Track, TrackCache, UserData,
};
use riff_app::widgets::toast::ToastKind;
use riff_app::window::PersistedWindowState;
use riff_audio::Event as AudioEvent;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "support/temp.rs"]
mod temp;

const TITLES: [&str; 12] = [
  "Midnight City",
  "Blinding Lights",
  "Nuvole Bianche",
  "Take Five",
  "Clair de Lune",
  "Digital Love",
  "Resonance",
  "Bohemian Rhapsody",
  "Windowlicker",
  "Say My Name",
  "Genesis",
  "Intro",
];
const ARTISTS: [&str; 8] = [
  "M83",
  "The Weeknd",
  "Ludovico Einaudi",
  "Dave Brubeck",
  "Debussy",
  "Daft Punk",
  "HOME",
  "Queen",
];
const ALBUMS: [&str; 6] = [
  "Hurry Up, We're Dreaming",
  "After Hours",
  "Una Mattina",
  "Time Out",
  "Suite bergamasque",
  "Discovery",
];

fn track(n: usize) -> Track {
  Track {
    id: Some(format!("track-{n}")),
    title: Some(format!(
      "{} {}",
      TITLES[n % TITLES.len()],
      n / TITLES.len() + 1
    )),
    artist: Some(ARTISTS[n % ARTISTS.len()].to_string()),
    album: Some(ALBUMS[n % ALBUMS.len()].to_string()),
    duration: Some(150.0 + (n * 17 % 120) as f64),
    cover: Some(cover_url(n % 20)),
    thumbnail: Some(cover_url(n % 20)),
    url: Some(format!("https://exemple.invalid/track-{n}")),
    platform: Some("ytmusic".to_string()),
    ..Track::default()
  }
}

fn tracks(range: std::ops::Range<usize>) -> Vec<Track> {
  range.map(track).collect()
}

fn cover_url(n: usize) -> String {
  format!("https://covers.invalid/{n}.jpg")
}

fn playlist(id: &str, name: &str, range: std::ops::Range<usize>) -> Playlist {
  Playlist {
    id: Some(id.to_string()),
    name: Some(name.to_string()),
    tracks: tracks(range),
    ..Playlist::default()
  }
}

fn album(n: usize) -> SavedAlbum {
  SavedAlbum {
    id: Some(format!("album-{n}")),
    title: Some(ALBUMS[n % ALBUMS.len()].to_string()),
    artist: Some(ARTISTS[n % ARTISTS.len()].to_string()),
    cover: Some(cover_url(n)),
    year: Some((2008 + n).to_string()),
    ..SavedAlbum::default()
  }
}

/// Paroles synchronisées au format LRC, une ligne toutes les 6 secondes.
fn lrc() -> String {
  [
    "Waiting in a car, waiting for a ride in the dark",
    "The night city grows, look at the horizon glow",
    "Drive until we feel alive, the lights are in our eyes",
    "Streets are empty, the radio is low",
    "Take me where the neon rivers flow",
    "Hold on to the moment, never let it go",
    "Every shadow sings a song we used to know",
    "We are the sound of the midnight city",
  ]
  .iter()
  .enumerate()
  .map(|(i, text)| {
    let seconds = 90 + i * 6;
    format!("[{:02}:{:02}.00]{text}", seconds / 60, seconds % 60)
  })
  .collect::<Vec<_>>()
  .join("\n")
}

/// Pochette sombre de la capture 15 : fond noir, une tache bleutée à gauche et une tache dorée en bas à droite (valeurs
/// relevées sur le fond de la capture, pas la vraie image).
fn dark_cover(side: u32) -> Decoded {
  let mut rgba = Vec::with_capacity((side * side * 4) as usize);
  for y in 0..side {
    for x in 0..side {
      let (u, v) = (x as f32 / side as f32, y as f32 / side as f32);
      let blob = |cx: f32, cy: f32| (-((u - cx).powi(2) + (v - cy).powi(2)) / 0.03).exp();
      let (blue, gold) = (blob(0.3, 0.4), blob(0.72, 0.72));
      let channel = |base: f32, with_blue: f32, with_gold: f32| {
        (base + with_blue * blue + with_gold * gold).min(255.0) as u8
      };
      rgba.extend_from_slice(&[
        channel(6.0, 110.0, 190.0),
        channel(6.0, 130.0, 130.0),
        channel(8.0, 180.0, 30.0),
        255,
      ]);
    }
  }
  Decoded {
    width: side,
    height: side,
    rgba,
  }
}

/// Pochette de la capture motion 12 (`12-karaoke`) : bandes noires en haut et en bas, flancs rouge sombre, fond blanc et deux drapeaux
/// (étoiles et rayures en haut à droite, Union Jack en bas à gauche). Approximation de l'image, pour que les orbes aient ses couleurs.
fn flag_cover(side: u32) -> Decoded {
  let mut rgba = Vec::with_capacity((side * side * 4) as usize);
  for y in 0..side {
    for x in 0..side {
      let (u, v) = (x as f32 / side as f32, y as f32 / side as f32);
      let rgb = if !(0.13..=0.87).contains(&v) {
        [0, 0, 0]
      } else if !(0.12..=0.88).contains(&u) {
        [100, 14, 10]
      } else if (0.5..0.88).contains(&u) && (0.17..0.55).contains(&v) {
        // Drapeau américain : cantonnement bleu, rayures rouges et blanches.
        if u < 0.7 && v < 0.36 {
          [40, 50, 120]
        } else if (((v - 0.17) / 0.38 * 13.0) as u32).is_multiple_of(2) {
          [190, 30, 40]
        } else {
          [255, 255, 255]
        }
      } else if (0.12..0.62).contains(&u) && (0.5..0.88).contains(&v) {
        // Union Jack : fond bleu, croix blanche bordée de rouge et diagonales.
        let (a, b) = ((u - 0.12) / 0.5, (v - 0.5) / 0.38);
        let on_cross = (a - 0.5).abs() < 0.12 || (b - 0.5).abs() < 0.12;
        let on_diagonal = (a - b).abs() < 0.1 || (a + b - 1.0).abs() < 0.1;
        if on_cross {
          [210, 25, 35]
        } else if on_diagonal {
          [245, 245, 245]
        } else {
          [30, 45, 115]
        }
      } else {
        [255, 255, 255]
      };
      rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
  }
  Decoded {
    width: side,
    height: side,
    rgba,
  }
}

/// Pochette factice : dégradé diagonal dont la teinte dépend de l'URL.
fn gradient(key: &ImageKey) -> Decoded {
  if key.url.ends_with("/karaoke-sombre.jpg") {
    return dark_cover(key.size.max(8));
  }
  if key.url.ends_with("/karaoke-drapeaux.jpg") {
    return flag_cover(key.size.max(8));
  }
  let seed = key.url.bytes().fold(7_u32, |hash, byte| {
    hash.wrapping_mul(31).wrapping_add(u32::from(byte))
  });
  let (r0, g0, b0) = (seed & 0xff, (seed >> 8) & 0xff, (seed >> 16) & 0xff);
  let side = key.size.max(8);
  let mut rgba = Vec::with_capacity((side * side * 4) as usize);
  for y in 0..side {
    for x in 0..side {
      let t = (x + y) * 255 / (2 * side);
      let mix = |base: u32, other: u32| ((base * (255 - t) + other * t) / 255) as u8;
      rgba.extend_from_slice(&[mix(r0, 255 - r0), mix(g0, 255 - g0), mix(b0, 255 - b0), 255]);
    }
  }
  Decoded {
    width: side,
    height: side,
    rgba,
  }
}

/// Ligne de l'historique de recherche : `kind` vaut `query`, `track` ou `artist`.
fn entry(kind: &str, title: &str, subtitle: &str, thumb: Option<usize>) -> SearchEntry {
  SearchEntry {
    kind: Some(kind.to_string()),
    key: Some(format!("{kind}:{title}")),
    title: Some(title.to_string()),
    subtitle: (!subtitle.is_empty()).then(|| subtitle.to_string()),
    thumb: thumb.map(cover_url),
    query: (kind == "query").then(|| title.to_string()),
    ..SearchEntry::default()
  }
}

/// Historique comparable à celui de `02-recherche-historique` de `ui-reference`.
fn search_history() -> Vec<SearchEntry> {
  vec![
    entry("query", "rushy", "", Some(31)),
    entry("query", "wallace", "", Some(32)),
    entry("track", "Sacrifice de masse", "NoeCLL - uploads", Some(33)),
    entry("query", "sacrifice de masse", "", Some(33)),
    entry("track", "Lost Dreams", "noir heart", Some(34)),
  ]
}

/// Titres de la recherche « daft punk » de `03-resultats-recherche` : titre, artiste, durée, explicite.
const RESULT_TRACKS: [(&str, &str, f64, bool); 12] = [
  ("DAFT PUNK (feat. Yway)", "NAY", 283.0, true),
  (
    "Instant Crush (feat. Julian Casablancas)",
    "Daft Punk & Julian Casablancas",
    338.0,
    false,
  ),
  ("Fresh", "Daft Punk", 245.0, false),
  (
    "Give Life Back to Music (Drumless Edition)",
    "Daft Punk",
    276.0,
    false,
  ),
  ("Da Funk", "Daft Punk", 329.0, false),
  (
    "The Son of Flynn (From \"TRON: Legacy\"/Score)",
    "Daft Punk",
    96.0,
    false,
  ),
  (
    "End of Line (From \"TRON: Legacy\"/Score)",
    "Daft Punk",
    157.0,
    false,
  ),
  ("Starboy (feat. Daft Punk)", "The Weeknd", 231.0, false),
  (
    "The Game Has Changed (From \"TRON: Legacy\"/Score)",
    "Daft Punk",
    206.0,
    false,
  ),
  ("Around the World", "Daft Punk", 429.0, false),
  ("One More Time", "Daft Punk", 320.0, false),
  (
    "Get Lucky (feat. Pharrell Williams)",
    "Daft Punk",
    369.0,
    false,
  ),
];

/// Albums de `03-resultats-recherche` : titre, artiste, année.
const RESULT_ALBUMS: [(&str, &str, &str); 8] = [
  (
    "I Gotta Try You Girl (feat. Daft Punk)",
    "Junior Kimbrough",
    "2021",
  ),
  ("Random Access Memories", "Daft Punk", "2013"),
  ("Discovery", "Daft Punk", "2001"),
  ("TRON: Legacy - The Complete Edition", "Daft Punk", "2010"),
  ("Homework (25th Anniversary Edition)", "Daft Punk", "1997"),
  ("Homework", "Daft Punk", "1997"),
  ("Random Access Memories", "Daft Punk", "2013"),
  ("Human After All", "Daft Punk", "2005"),
];

fn result_tracks() -> Vec<Track> {
  RESULT_TRACKS
    .iter()
    .enumerate()
    .map(|(n, (title, artist, duration, explicit))| {
      let mut track = Track {
        id: Some(format!("result-{n}")),
        title: Some((*title).to_string()),
        artist: Some((*artist).to_string()),
        duration: Some(*duration),
        cover: Some(cover_url(60 + n)),
        thumbnail: Some(cover_url(60 + n)),
        url: Some(format!("https://exemple.invalid/result-{n}")),
        platform: Some("ytmusic".to_string()),
        ..Track::default()
      };
      if *explicit {
        track.extra.insert("explicit".to_string(), true.into());
      }
      track
    })
    .collect()
}

fn result_albums() -> Vec<SavedAlbum> {
  RESULT_ALBUMS
    .iter()
    .enumerate()
    .map(|(n, (title, artist, year))| SavedAlbum {
      id: Some(format!("result-album-{n}")),
      title: Some((*title).to_string()),
      artist: Some((*artist).to_string()),
      cover: Some(cover_url(80 + n)),
      year: Some((*year).to_string()),
      ..SavedAlbum::default()
    })
    .collect()
}

/// Libellés français des pastilles, comme dans le cache de traduction du profil capturé (« Tout », « Chansons »,
/// « Artistes ») : les captures de `ui-reference` sont en français partiel.
fn french_chips() -> I18n {
  let dir = temp::TempDir::new("ui-snapshots-i18n");
  fs::write(
    dir.join("fr.json"),
    r#"{"All":"Tout","Songs":"Chansons","Artists":"Artistes"}"#,
  )
  .expect("traduction");
  I18n::load("fr", &dir)
}

/// Recherche « daft punk » affichée : rien en lecture, panneau droit ouvert, comme les captures 03.
fn search_results(app: &mut App, filter: SearchFilter) {
  nothing_playing(app);
  app.lyrics.panel_open = true;
  app.i18n = std::sync::Arc::new(french_chips());
  app.ui.search.input = "daft punk".to_string();
  app.ui.search.query = "daft punk".to_string();
  app.ui.search.tracks = result_tracks();
  app.ui.search.albums = result_albums();
  app.ui.search.has_more = true;
  app.ui.search.icon_cover = Some(cover_url(60));
  app.update(Message::Ui(UiMessage::SetSearchFilter(filter)));
  go(app, Screen::Search);
}

/// Scènes des résultats de recherche : captures 03 de `ui-reference`, puis variantes non capturées.
fn result_scenes() -> Vec<Scene> {
  vec![
    // Capture `03-resultats-recherche` : filtre Tout, sans carte d'artiste (rendu observé tel quel).
    ("03-resultats-recherche", |app| {
      search_results(app, SearchFilter::All);
    }),
    // Capture `03b-resultats-artistes` : filtre Artistes, page vide faute de carte.
    ("03b-resultats-artistes", |app| {
      search_results(app, SearchFilter::Artists);
    }),
    // Capture `03c-resultats-albums` : filtre Albums.
    ("03c-resultats-albums", |app| {
      search_results(app, SearchFilter::Albums);
    }),
    // Réglage `riff_search_artists` actif et artiste trouvé : carte d'artiste vérifié. Non capturé.
    ("03d-resultats-carte-artiste", |app| {
      search_results(app, SearchFilter::All);
      app.ui.search.artist = Some(ArtistCard {
        name: "Daft Punk".to_string(),
        avatar: Some(cover_url(70)),
        fans: 4_300_000.0,
        verified: true,
      });
    }),
    // Pointeur sur le deuxième titre : fond `surface-container` et bouton favori. Non capturé.
    ("03e-resultats-survol-titre", |app| {
      search_results(app, SearchFilter::All);
      app.ui.row_hover = Some((ListId::Search, 1));
    }),
    // Pointeur sur la deuxième carte d'album : levée de 2 px. Non capturé.
    ("03f-resultats-survol-album", |app| {
      search_results(app, SearchFilter::All);
      app.ui.search.album_hover = Some(1);
    }),
    // Recherche en cours : albums déjà là, forme qui tourne à la place des titres. Non capturé.
    ("03g-resultats-chargement", |app| {
      search_results(app, SearchFilter::All);
      app.ui.search.tracks.clear();
      app.ui.search.loading = true;
    }),
    // Aucun titre, aucun album : `.empty-hint`. Non capturé.
    ("03h-resultats-aucun", |app| {
      search_results(app, SearchFilter::All);
      app.ui.search.tracks.clear();
      app.ui.search.albums.clear();
    }),
    // Erreur de recherche : `stopM3SearchLoaderWithError`. Non capturé.
    ("03i-resultats-erreur", |app| {
      search_results(app, SearchFilter::All);
      app.ui.search.tracks.clear();
      app.ui.search.albums.clear();
      app.ui.search.error = Some("Offline: no internet connection".to_string());
    }),
    // Un résultat en cours de lecture : barres à la place du numéro, titre `primary`. Non capturé.
    ("03j-resultats-lecture", |app| {
      search_results(app, SearchFilter::All);
      app.player.current = Some(result_tracks().remove(2));
    }),
  ]
}

/// Cinq « Daily Mix » de six titres aux pochettes distinctes : mosaïques 2 x 2 de l'accueil.
fn daily_mixes() -> DailyMixes {
  DailyMixes {
    day: Some("2026-11-17".to_string()),
    mixes: (0..5)
      .map(|n| DailyMix {
        id: Some(format!("daily-mix-{}", n + 1)),
        title: None,
        artists: (0..4)
          .map(|k| ARTISTS[(n + k) % ARTISTS.len()].to_string())
          .collect(),
        tracks: tracks(n * 4..n * 4 + 6),
        ..DailyMix::default()
      })
      .collect(),
  }
}

/// État de base : données déterministes, un titre à mi-parcours en lecture.
fn base_app() -> App {
  // Le banc ne dépend pas de la machine : le système laisse les animations actives (`SPI_GETCLIENTAREAANIMATION` vrai).
  motion::inject_system_preference(Some(false));
  let favorites = tracks(0..32);
  let data = UserData {
    favorites,
    playlists: vec![
      playlist("pl-1", "Route de nuit", 0..14),
      playlist("pl-2", "Concentration", 8..20),
      playlist("pl-3", "Classiques", 20..30),
      playlist("pl-4", "Sport", 3..11),
    ],
    history: tracks(10..26),
    local_files: tracks(30..48),
    saved_albums: (0..6).map(album).collect(),
    wave_cache: Some(TrackCache {
      ts: Some(0),
      tracks: tracks(4..16),
    }),
    trending_cache: Some(TrackCache {
      ts: Some(0),
      tracks: tracks(16..28),
    }),
    daily_mixes: Some(daily_mixes()),
    search_history: search_history(),
    volume: 0.7,
    ..UserData::default()
  };
  let mut app = App::new(
    PersistedWindowState::default(),
    captured_settings(),
    I18n::default(),
  )
  .with_data(data);
  // Tirage des formes des spinners fixé : sans lui la graine vient de l'horloge et 03g, 03i changent à chaque rendu.
  app.spinner_seed = 1;
  app.ui.home.wave = tracks(4..16);
  app.ui.home.trending = tracks(16..28);
  app.ui.home.started = true;
  let current = track(0);
  app.player.queue = tracks(0..12);
  app.player.index = Some(0);
  app.player.current = Some(current);
  app.playback = Playback {
    position: Duration::from_secs(105),
    duration: Some(Duration::from_secs(210)),
    sample_rate: Some(48_000),
    is_playing: true,
    ..Playback::default()
  };
  app.lyrics.token = app.player.token;
  let token = app.lyrics.token;
  app.update(Message::Ui(UiMessage::Reply(
    ReplyTag::Lyrics { token },
    Ok(serde_json::json!({ "success": true, "lyrics": lrc() })),
  )));
  app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(
    105,
  ))));
  app.lyrics.panel_open = false;
  app
}

/// Rien en lecture : état du lancement des captures 01, 12 et 13.
fn nothing_playing(app: &mut App) {
  app.player.current = None;
  app.player.index = None;
  app.player.queue.clear();
  app.playback = Playback::default();
  app.lyrics.lyrics = None;
}

fn go(app: &mut App, screen: Screen) {
  app.update(Message::Navigate(screen));
}

/// Clics sur `#btn-toggle-fav` aux instants `flips_ms` (axe des images) déjà passés à `at_ms` ; rend leur nombre.
fn toggle_like_at(app: &mut App, at_ms: u64, flips_ms: &[f64]) -> usize {
  let done = flips_ms
    .iter()
    .filter(|&&flip| at_ms as f64 >= flip)
    .count();
  for _ in 0..done {
    app.update(Message::Ui(UiMessage::CurrentCommand(
      TrackCommand::ToggleFavorite,
    )));
  }
  done
}

fn resize(app: &mut App, size: Size) {
  app.update(Message::WindowOpened {
    position: None,
    size,
  });
}

/// Calcule le halo `.hero-fx` de la page ouverte, comme le fait l'application en tâche de fond : pochette chargée, flou, fondu terminé.
fn settle_hero(app: &mut App) {
  let Some(wanted) = app.hero_key() else {
    return;
  };
  for job in app.take_blur_jobs() {
    if job.key == wanted {
      let done = riff_app::visuals::blur::compute(job.width, job.height, &job.pixels, &job.key);
      app.blur_ready(&job.key, done);
    }
  }
  app.sync_layers(std::time::Instant::now());
}

/// Lignes inventées du banc de la lecture plein écran : une toutes les 4 s, la lecture est sur la quatrième (sans silence assez long pour créer une ligne vide).
fn karaoke_lrc() -> String {
  [
    "Première ligne du banc",
    "Deuxième ligne du banc",
    "Troisième ligne du banc, assez longue pour deux rangées",
    "Quatrième ligne du banc, aussi longue pour deux rangées",
    "Cinquième ligne du banc",
    "Sixième ligne du banc",
    "Septième ligne du banc",
  ]
  .iter()
  .enumerate()
  .map(|(i, text)| {
    let seconds = i * 4;
    format!("[{:02}:{:02}.00]{text}", seconds / 60, seconds % 60)
  })
  .collect::<Vec<_>>()
  .join(
    "
",
  )
}

/// Plein écran du profil capturé : repos (`tv-idle`), taille 120 %, position à 32 % (13,6 s sur 42,5 s, quatrième ligne remplie à 40 %), titre et artiste inventés.
fn karaoke_capture(app: &mut App, lrc: Option<String>) {
  go(app, Screen::Home);
  let mut current = track(5);
  current.title = Some("Titre long du banc (feat. Invité du banc)".to_string());
  current.artist = Some("Artiste du banc".to_string());
  current.duration = Some(42.5);
  current.cover = Some("https://covers.invalid/karaoke-sombre.jpg".to_string());
  current.thumbnail = current.cover.clone();
  app.player.current = Some(current);
  app.playback.position = Duration::from_millis(13_600);
  app.playback.duration = Some(Duration::from_millis(42_500));
  app.settings.visual.tvk.size = 120;
  app.lyrics.lyrics = None;
  if let Some(lrc) = lrc {
    app.lyrics.token = app.player.token;
    let token = app.lyrics.token;
    app.update(Message::Ui(UiMessage::Reply(
      ReplyTag::Lyrics { token },
      Ok(serde_json::json!({ "success": true, "lyrics": lrc })),
    )));
  }
  app.update(Message::Audio(AudioEvent::Position(Duration::from_millis(
    13_600,
  ))));
  app.update(Message::Lyrics(
    riff_app::lyrics::update::LyricsMessage::OpenTvk,
  ));
  app.animation_tick(
    std::time::Instant::now(),
    &[0; riff_app::visuals::viz::FREQ_BINS],
  );
  // La capture de Riff est à l'état établi : la transition de la ligne active (`transition: opacity` de
  // `.tv-karaoke[data-tvk-anim="slide"] .lyric-line`, `styles.css:9130-9141`) est finie.
  app.lyrics.transition = None;
  app.lyrics.tvk_idle = true;
}

/// Karaoké de la capture motion 12 : « the hood dream », pochette de drapeaux, souris bougée par le clic (`tv-idle` absent).
fn karaoke_motion(app: &mut App) {
  // Le panneau de paroles de droite est ouvert sous le karaoké (f001 et f021 de la capture).
  app.lyrics.panel_open = true;
  karaoke_capture(app, Some(karaoke_lrc()));
  if let Some(track) = app.player.current.as_mut() {
    track.title = Some("the hood dream".to_string());
    track.artist = Some("808mystic".to_string());
    track.cover = Some("https://covers.invalid/karaoke-drapeaux.jpg".to_string());
    track.thumbnail = track.cover.clone();
  }
  app.lyrics.tvk_idle = false;
}

/// Karaoké qui vient de s'ouvrir : `tvk-in` et les orbes partent à `opened`.
fn karaoke_opened(app: &mut App, opened: Instant) {
  karaoke_motion(app);
  app.lyrics.tvk_motion = motion::TvkMotion::launched();
  app.lyrics.tvk_motion.open(opened);
}

/// Karaoké fermé à `closed` : l'écran reste affiché pendant `tvk-out`, les orbes figés à l'instant de la fermeture.
fn karaoke_closed(app: &mut App, opened: Instant, closed: Instant) {
  karaoke_motion(app);
  // Mouvement désactivé : le message ferme la vue sans dater `tvk-out` à l'instant réel.
  app.update(Message::Lyrics(
    riff_app::lyrics::update::LyricsMessage::CloseTvk,
  ));
  app.lyrics.tvk_idle = false;
  app.lyrics.tvk_motion = motion::TvkMotion::launched();
  app.lyrics.tvk_motion.open(opened);
  app.lyrics.tvk_motion.close(closed);
}

/// Animation `pop` du karaoké (réglage « Pop », vitesse de 900 ms d'un Riff vierge) : la quatrième ligne devient active à
/// `t0 + start_ms`. L'état d'avant est celui d'une lecture qui a déjà éclairé la troisième ligne ; le vrai chemin (`animation_tick`)
/// lance ensuite la transition.
fn karaoke_pop(app: &mut App, t0: Instant, start_ms: u64) {
  karaoke_motion(app);
  app.settings.visual.tvk.anim = riff_app::visuals::settings::TvkAnim::Pop;
  app.settings.visual.tvk.speed = 900;
  let start = t0 + Duration::from_millis(start_ms);
  let freq = [0; riff_app::visuals::viz::FREQ_BINS];
  app.playback.is_playing = false;
  app.playback.position = Duration::from_secs_f64(8.3);
  app.now = start - Duration::from_secs(3);
  app.animation_tick(app.now, &freq);
  app.now = start - Duration::from_secs(1);
  app.animation_tick(app.now, &freq);
  app.playback.is_playing = true;
  app.playback.position = Duration::from_secs_f64(12.0);
  app.now = start;
  app.animation_tick(start, &freq);
}

/// Calcule le fond ambiant de la vue plein écran, comme le fait l'application en tâche de fond.
fn settle_tvk(app: &mut App) {
  if app.tvk_bg_key(0).is_none() {
    return;
  }
  for job in app.take_blur_jobs() {
    // Les quatre orbes et la vignette du fond plein écran.
    if job.key.ambient.is_some() {
      let done = riff_app::visuals::blur::compute(job.width, job.height, &job.pixels, &job.key);
      app.blur_ready(&job.key, done);
    }
  }
  app.sync_layers(std::time::Instant::now());
}

/// Remplit le cache de pochettes avec les images que l'écran demande.
fn fill_covers(app: &mut App) {
  let mut keys = app.wanted_images();
  if let Some(url) = app.player.current.as_ref().and_then(|t| t.cover.clone()) {
    for size in [SIZE_ROW, SIZE_CARD, SIZE_HERO] {
      keys.push(ImageKey {
        url: url.clone(),
        size,
      });
    }
  }
  for key in keys {
    let decoded = gradient(&key);
    app.images.insert(key, decoded);
  }
}

fn out_dir() -> PathBuf {
  let dir = std::env::var_os("RIFF_SNAPSHOT_DIR").map_or_else(
    || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots"),
    PathBuf::from,
  );
  fs::create_dir_all(&dir).expect("dossier de sortie");
  dir.canonicalize().expect("chemin du dossier de sortie")
}

/// Rend une scène du plateau. Le banc n'exécute pas l'effet de défilement des paroles : en plein écran, la ligne active est
/// amenée au centre par un coup de molette sur la zone des paroles, comme le ferait l'application.
fn render_scene(app: &mut App, name: &str, size: Size) {
  // Capture de la navigation défilée jusqu'au bouton du compte (`#settings-nav-account`).
  if name.ends_with("-reglages-compte-nav") {
    let scale = size.width / 1920.0;
    return render_with(app, name, size, move |sim| {
      sim.point_at(Point::new(570.0 * scale, 500.0 * scale));
      sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -2000.0 },
      })]);
    });
  }
  // 60e : la page Thème défilée d'un écran pour montrer le panneau du maillage sous « Mesh gradient brightness ».
  if name.starts_with("60e") {
    let scale = size.width / 1920.0;
    return render_with(app, name, size, move |sim| {
      sim.point_at(Point::new(1070.0 * scale, 540.0 * scale));
      sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Pixels {
          x: 0.0,
          y: -riff_app::theme::tokens::settings::PAGE_SCROLL,
        },
      })]);
    });
  }
  if let Some(pages) = settings_page(name) {
    let scale = size.width / 1920.0;
    return render_with(app, name, size, move |sim| {
      sim.point_at(Point::new(1070.0 * scale, 540.0 * scale));
      sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Pixels {
          x: 0.0,
          y: -(pages as f32) * riff_app::theme::tokens::settings::PAGE_SCROLL,
        },
      })]);
    });
  }
  let state = &app.lyrics;
  let target = (state.tvk_open && state.active.is_some())
    .then(|| {
      let line = state.layout.boxes[state.active.unwrap_or(0)];
      riff_app::lyrics::layout::target_scroll(line, state.viewport)
    })
    .filter(|target| *target > 0.0);
  let Some(target) = target else {
    return render(app, name, size);
  };
  app.lyrics.offset = target;
  let scale = size.width / 1920.0;
  render_with(app, name, size, move |sim| {
    sim.point_at(Point::new(1192.0 * scale, 540.0 * scale));
    sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
      delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -target },
    })]);
  });
}

/// Nombre de défilements d'une page d'une scène des réglages `NN<lettre>-reglages-<catégorie>-pK` (K >= 2).
fn settings_page(name: &str) -> Option<u32> {
  // Scènes d'animation : le banc ajoute `-tNNN` au nom.
  let name = name
    .rsplit_once("-t")
    .filter(|(_, instant)| !instant.is_empty() && instant.bytes().all(|b| b.is_ascii_digit()))
    .map_or(name, |(head, _)| head);
  let rest = name.split_once('-')?.1.strip_prefix("reglages-")?;
  let page = rest.rsplit_once("-p")?.1.parse::<u32>().ok()?;
  page.checked_sub(1).filter(|pages| *pages > 0)
}

/// Rend la vue de `app` et écrit `<nom>.png` à la taille logique `size`.
fn render(app: &mut App, name: &str, size: Size) {
  render_with(app, name, size, |_| {});
}

/// Comme `render`, après avoir fait jouer `interact` sur le simulateur (clic, saisie).
fn render_with(
  app: &mut App,
  name: &str,
  size: Size,
  interact: impl FnOnce(&mut Simulator<'_, Message>),
) {
  if std::env::var("RIFF_SNAPSHOT_ONLY").is_ok_and(|only| !name.starts_with(&only)) {
    return;
  }
  // Un écran fixe n'est pas le premier instant d'un changement : l'état de la barre de lecture construit par `update` (titre qui
  // vient de jouer ou de s'arrêter) y animerait ses transitions à t = 0. Les scènes d'animation posent cet état elles-mêmes.
  if !MOTION_RENDER.with(std::cell::Cell::get) {
    app.playbar_motion = Default::default();
  }
  fill_covers(app);
  settle_hero(app);
  settle_tvk(app);
  let scratch = temp::TempDir::new("ui-snapshots");
  let settings = Settings {
    default_font: fonts::UI,
    fonts: fonts::bytes(),
    ..Settings::default()
  };
  let theme = app.theme.theme.clone();
  let snapshot = {
    let mut simulator = Simulator::with_size(settings, size, app.view());
    interact(&mut simulator);
    simulator.snapshot(&theme).expect("capture")
  };
  // `matches_image` écrit le PNG quand le fichier n'existe pas ; le nom reçoit le suffixe du moteur.
  assert!(
    snapshot
      .matches_image(scratch.join("shot.png"))
      .expect("PNG")
  );
  let written = fs::read_dir(&scratch)
    .expect("lecture du dossier temporaire")
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .find(|path| path.extension().is_some_and(|ext| ext == "png"))
    .expect("PNG produit");
  let full = image::open(&written).expect("lecture du PNG").into_rgba8();
  let reduced = image::imageops::resize(
    &full,
    size.width as u32,
    size.height as u32,
    FilterType::Lanczos3,
  );
  let target = out_dir().join(format!("{name}.png"));
  reduced.save(&target).expect("écriture du PNG");
  println!("{}", target.display());
  // Barre de lecture : recadrage des 91 derniers pixels de 1920x1080, comme `12b` de `ui-reference`.
  if name.contains("-barre-") && size.height as u32 == 1080 {
    let bar = image::imageops::crop_imm(&reduced, 0, 989, reduced.width(), 91).to_image();
    let target = out_dir().join(format!("{name}-recadree.png"));
    bar.save(&target).expect("écriture du recadrage");
    println!("{}", target.display());
  }
}

type Scene = (&'static str, fn(&mut App));

/// Accueil au lancement, champ de recherche sélectionné par un clic, historique ouvert.
fn search_open(app: &mut App) {
  go(app, Screen::Home);
  nothing_playing(app);
  app.lyrics.panel_open = true;
  app.ui.search.history_open = true;
}

/// Scènes rendues après un clic sur la pastille de recherche : le focus du champ se voit au rendu.
fn search_scenes() -> Vec<Scene> {
  vec![
    // Capture `02-recherche-historique` : cinq lignes sous le champ vide.
    ("02c-recherche-historique", search_open),
    // Pointeur sur la deuxième ligne : croix de suppression visible.
    ("02d-recherche-historique-survol", |app| {
      search_open(app);
      app.ui.search.hover = Some(1);
    }),
    // Historique vide : `render` de `initSearchHistory` masque le panneau. Non capturé par `ui-reference`.
    ("02e-recherche-historique-vide", |app| {
      search_open(app);
      app.data.search_history.clear();
    }),
    // Texte saisi : l'historique est filtré. Non capturé par `ui-reference`.
    ("02f-recherche-historique-filtre", |app| {
      search_open(app);
      app.ui.search.input = "sacrifice".to_string();
    }),
    // Douze lignes affichées au plus (défilement), artiste vérifié, titre explicite, vignettes manquantes.
    ("02g-recherche-historique-long", |app| {
      search_open(app);
      let mut list = search_history();
      let mut artist = entry("artist", "Daft Punk", "", Some(40));
      artist.extra.insert("verified".to_string(), true.into());
      let mut explicit = entry("track", "Say My Name", "Odesza", None);
      explicit.extra.insert("explicit".to_string(), true.into());
      list.extend([artist, explicit, entry("artist", "HOME", "", None)]);
      list.extend((0..14).map(|n| entry("query", &format!("recherche {n}"), "", Some(41 + n))));
      app.data.search_history = list;
    }),
  ]
}

/// Titres « Recently played » de `04c-bibliotheque-file` de `ui-reference` : titre, artiste.
const SIDEBAR_RECENT: [(&str, &str); 12] = [
  ("Fentanyl", "Freeze Corleone"),
  ("température", "Mairo"),
  ("cherry blossoms", "mxlu"),
  ("Hooligan", "Rushy"),
  ("SAD", "Glizz"),
  ("J'SAVAIS", "Ikaz Boi"),
  ("Star Shopping", "Lil Peep"),
  ("Wallace Cleaver & Kingcsix - Intro", "Kingcsix"),
  ("xénon", "Wallace Cleaver"),
  ("JOURNAL DE QUÊTE (Interlude)", "Vald"),
  ("Daddy Issues", "The Neighbourhood"),
  ("Nuit blanche", "Laylow"),
];

fn sidebar_track(n: usize, title: &str, artist: &str) -> Track {
  Track {
    id: Some(format!("sidebar-{n}")),
    title: Some(title.to_string()),
    artist: Some(artist.to_string()),
    duration: Some(200.0),
    cover: Some(cover_url(100 + n)),
    thumbnail: Some(cover_url(100 + n)),
    url: Some(format!("https://exemple.invalid/sidebar-{n}")),
    platform: Some("ytmusic".to_string()),
    ..Track::default()
  }
}

fn sidebar_history(count: usize) -> Vec<Track> {
  SIDEBAR_RECENT
    .iter()
    .take(count)
    .enumerate()
    .map(|(n, (title, artist))| sidebar_track(n, title, artist))
    .collect()
}

/// Profil des captures 04 : rien en lecture, accueil au centre, 568 titres aimés, une playlist « like »
/// de 567 titres, aucun épinglé ni album, historique de douze titres, « My files » masqué
/// (`riff_show_local` à `false` : la rangée manque sur `04b-bibliotheque-albums`).
fn sidebar_capture(app: &mut App, tab: SidebarTab) {
  go(app, Screen::Home);
  nothing_playing(app);
  app.lyrics.panel_open = true;
  app.data.favorites = tracks(0..568);
  app.data.playlists = vec![playlist("pl-like", "like", 0..567)];
  app.data.saved_albums.clear();
  app.data.history = sidebar_history(SIDEBAR_RECENT.len());
  app.settings.show_local = false;
  app.update(Message::Ui(UiMessage::SidebarTab(tab)));
}

/// Scène de la bibliothèque : `x`, `y` donnent la position du pointeur à l'échelle 1920 x 1080.
type SidebarScene = (&'static str, fn(&mut App), Option<(f32, f32)>);

/// Captures 04a, 04b et 04c de `ui-reference`, puis variantes non capturées (survol, épinglés, vide, titre long).
fn sidebar_scenes() -> Vec<SidebarScene> {
  vec![
    (
      "04a-bibliotheque-playlists",
      |app| {
        sidebar_capture(app, SidebarTab::Playlists);
      },
      None,
    ),
    (
      "04b-bibliotheque-albums",
      |app| {
        sidebar_capture(app, SidebarTab::Albums);
      },
      None,
    ),
    (
      "04c-bibliotheque-file",
      |app| {
        sidebar_capture(app, SidebarTab::Queue);
      },
      None,
    ),
    // Pointeur sur « like » : fond à 8 %.
    (
      "04a2-bibliotheque-playlists-survol",
      |app| {
        sidebar_capture(app, SidebarTab::Playlists);
      },
      Some((130.0, 281.0)),
    ),
    // Deux titres épinglés (épingle devant l'artiste, croix au survol du premier), playlist ouverte.
    (
      "04a3-bibliotheque-playlists-epingles",
      |app| {
        sidebar_capture(app, SidebarTab::Playlists);
        app.data.sidebar_pinned_tracks = vec![
          sidebar_track(1, "température", "Mairo"),
          sidebar_track(
            2,
            "Un titre très long qui dépasse la largeur de la barre latérale",
            "Un artiste au nom tout aussi long que son titre",
          ),
        ];
        app
          .data
          .playlists
          .push(playlist("pl-2", "Route de nuit", 0..14));
        app.update(Message::Ui(UiMessage::OpenPlaylist("pl-2".to_string())));
        app.ui.sidebar_hover = Some(1);
      },
      Some((130.0, 281.0)),
    ),
    // Playlist sans nom ni titre : « Untitled », beaucoup de rangées, fondu du bas.
    (
      "04a4-bibliotheque-playlists-longue",
      |app| {
        sidebar_capture(app, SidebarTab::Playlists);
        app.data.playlists = (0..14)
          .map(|n| {
            playlist(
              &format!("pl-{n}"),
              &format!("Playlist {}", n + 1),
              0..(n + 3),
            )
          })
          .collect();
      },
      None,
    ),
    // Flux défilé de plus de 6 px : fondu du haut en plus de celui du bas.
    (
      "04a5-bibliotheque-playlists-defilee",
      |app| {
        sidebar_capture(app, SidebarTab::Playlists);
        app.data.playlists = (0..14)
          .map(|n| {
            playlist(
              &format!("pl-{n}"),
              &format!("Playlist {}", n + 1),
              0..(n + 3),
            )
          })
          .collect();
        app.ui.sidebar_scroll = 120.0;
      },
      None,
    ),
    // Albums enregistrés et « My files » (Riff vierge : `riff_show_local` absent).
    (
      "04b2-bibliotheque-albums-enregistres",
      |app| {
        sidebar_capture(app, SidebarTab::Albums);
        app.settings.show_local = true;
        app.data.local_files = tracks(30..48);
        app.data.saved_albums = (0..6).map(album).collect();
      },
      None,
    ),
    // « My files » seul, albums vides.
    (
      "04b3-bibliotheque-albums-fichiers",
      |app| {
        sidebar_capture(app, SidebarTab::Albums);
        app.settings.show_local = true;
        app.data.local_files = tracks(30..48);
      },
      None,
    ),
    // Pointeur sur un album.
    (
      "04b4-bibliotheque-albums-survol",
      |app| {
        sidebar_capture(app, SidebarTab::Albums);
        app.settings.show_local = true;
        app.data.saved_albums = (0..6).map(album).collect();
      },
      Some((130.0, 281.0)),
    ),
    // Titre en cours : « Now playing », « Next up » (croix au survol de la première) et « Recently played ».
    (
      "04c2-bibliotheque-file-lecture",
      |app| {
        sidebar_capture(app, SidebarTab::Queue);
        app.player.queue = tracks(0..8);
        app.player.index = Some(0);
        app.player.current = Some(track(0));
        app.ui.sidebar_hover = Some(3);
      },
      Some((130.0, 340.0)),
    ),
    // File et historique vides.
    (
      "04c3-bibliotheque-file-vide",
      |app| {
        sidebar_capture(app, SidebarTab::Queue);
        app.data.history.clear();
      },
      None,
    ),
    // Pointeur sur un titre récent.
    (
      "04c4-bibliotheque-file-survol",
      |app| {
        sidebar_capture(app, SidebarTab::Queue);
      },
      Some((130.0, 237.0)),
    ),
  ]
}

/// Libellés français des deux boutons d'en-tête de « Mes fichiers » (04e : le profil capturé est en français
/// partiel ; les textes de la liste vide, créés après la traduction, y restent en anglais).
fn french_local() -> I18n {
  let dir = temp::TempDir::new("ui-snapshots-i18n-local");
  fs::write(
    dir.join("fr.json"),
    r#"{"Add files":"Ajouter des fichiers","Add folder":"Ajouter un dossier"}"#,
  )
  .expect("traduction");
  I18n::load("fr", &dir)
}

/// Profil des captures 04d et 04e : celui de 04a (sidebar_capture), puis l'écran demandé.
fn view_capture(app: &mut App, screen: Screen) {
  sidebar_capture(app, SidebarTab::Playlists);
  app.data.local_files.clear();
  app.i18n = std::sync::Arc::new(french_local());
  go(app, screen);
}

/// Titres locaux de « Mes fichiers » : la moitié sans album, pour la rangée d'albums et les lignes seules.
fn local_files(count: usize) -> Vec<Track> {
  tracks(30..30 + count)
}

/// Captures 04d et 04e de `ui-reference` (vides), puis variantes remplies, survols et bords.
fn view_scenes() -> Vec<SidebarScene> {
  vec![
    (
      "04d-vue-albums",
      |app| view_capture(app, Screen::Albums),
      None,
    ),
    (
      "04e-mes-fichiers",
      |app| view_capture(app, Screen::Library),
      None,
    ),
    // Six albums enregistrés : la grille tient sur une seule ligne de six colonnes.
    (
      "04d2-vue-albums-remplie",
      |app| {
        view_capture(app, Screen::Albums);
        app.data.saved_albums = (0..6).map(album).collect();
      },
      None,
    ),
    // Quatorze albums : trois lignes, pochettes absentes pour le dernier.
    (
      "04d3-vue-albums-plusieurs-lignes",
      |app| {
        view_capture(app, Screen::Albums);
        app.data.saved_albums = (0..14).map(album).collect();
        app.data.saved_albums[13].cover = None;
        app.data.saved_albums[12].title =
          Some("Un titre d'album vraiment très long qui dépasse la carte".to_string());
      },
      None,
    ),
    // Survol de la deuxième carte : levée de 2 px.
    (
      "04d4-vue-albums-survol",
      |app| {
        view_capture(app, Screen::Albums);
        app.data.saved_albums = (0..14).map(album).collect();
        app.ui.card_hover = Some((riff_app::ui::CardGrid::Saved, 1));
      },
      None,
    ),
    // Quarante albums : la page déborde, la barre de défilement prend sa place et les colonnes se resserrent.
    (
      "04d5-vue-albums-debordement",
      |app| {
        view_capture(app, Screen::Albums);
        app.data.saved_albums = (0..40).map(album).collect();
      },
      None,
    ),
    // Dix-huit titres locaux : rangée d'albums, puis les lignes.
    (
      "04e2-mes-fichiers-remplie",
      |app| {
        view_capture(app, Screen::Library);
        app.data.local_files = local_files(18);
        app.update(Message::Ui(UiMessage::LocalQuery(String::new())));
      },
      None,
    ),
    // Aucun titre n'a d'album : pas de rangée d'albums.
    (
      "04e3-mes-fichiers-sans-album",
      |app| {
        view_capture(app, Screen::Library);
        app.data.local_files = local_files(18)
          .into_iter()
          .map(|track| Track {
            album: None,
            ..track
          })
          .collect();
        app.update(Message::Ui(UiMessage::LocalQuery(String::new())));
      },
      None,
    ),
    // Recherche sans résultat : « Your library is empty... » de renderTracks.
    (
      "04e4-mes-fichiers-recherche-vide",
      |app| {
        view_capture(app, Screen::Library);
        app.data.local_files = local_files(18);
        app.update(Message::Ui(UiMessage::LocalQuery("zzz".to_string())));
      },
      None,
    ),
    // Pointeur sur « Ajouter un dossier » : rayon de 18 px, fond à 8 %.
    (
      "04e5-mes-fichiers-survol-bouton",
      |app| view_capture(app, Screen::Library),
      Some((1480.0, 138.0)),
    ),
    // Pointeur sur « Add files » de la liste vide.
    (
      "04e6-mes-fichiers-survol-vide",
      |app| view_capture(app, Screen::Library),
      Some((858.0, 299.0)),
    ),
    // Survol de la première carte d'album de la rangée.
    (
      "04e7-mes-fichiers-survol-album",
      |app| {
        view_capture(app, Screen::Library);
        app.data.local_files = local_files(18);
        app.update(Message::Ui(UiMessage::LocalQuery(String::new())));
        app.ui.card_hover = Some((riff_app::ui::CardGrid::Local, 0));
      },
      None,
    ),
  ]
}

/// Premiers titres aimés de `05-titres-aimes` de `ui-reference` : titre, artiste, durée en secondes.
const LIKED_CAPTURE: [(&str, &str, f64); 13] = [
  (
    "SCOTT x MOHA LA SQUALE ( remix @elsanta315 )",
    "Albatrose",
    122.0,
  ),
  ("S5-E36 (Return Of The Mack)", "Next Up", 191.0),
  ("the hood dream", "808mystic", 225.0),
  ("Freestyle 2021", "Favé", 122.0),
  (
    "Backyard ) - remix",
    "LETO X TRAVIS SCOTT (Mozart capitaine Jackson ep1",
    112.0,
  ),
  ("Jrk19 (prod: has to be you)", "Positif", 104.0),
  ("GARE DU NORD", "YVNNIS", 192.0),
  ("JOURNAL DE QUÊTES", "LUTHER", 172.0),
  ("Céleste", "LuvResval", 277.0),
  ("Hyrule", "LuvResval", 189.0),
  ("Un Viano à Venise", "LuvResval", 189.0),
  ("Ballade nocturne III", "LuvResval", 205.0),
  ("AZNVR (une minute de musique)", "LuvResval", 240.0),
];

/// 568 titres aimés dont les treize premiers sont ceux de la capture.
fn liked_tracks() -> Vec<Track> {
  let mut all = tracks(0..568);
  for (n, (title, artist, duration)) in LIKED_CAPTURE.iter().enumerate() {
    all[n] = Track {
      id: Some(format!("liked-{n}")),
      title: Some((*title).to_string()),
      artist: Some((*artist).to_string()),
      album: None,
      duration: Some(*duration),
      cover: Some(cover_url(200 + n)),
      thumbnail: Some(cover_url(200 + n)),
      url: Some(format!("https://exemple.invalid/liked-{n}")),
      ..all[n].clone()
    };
  }
  all
}

/// Profil de la capture 05 : celui de 04a, puis « Titres aimés ».
fn liked_capture(app: &mut App) {
  view_capture(app, Screen::Liked);
  app.data.favorites = liked_tracks();
  app.update(Message::Ui(UiMessage::FavoritesQuery(String::new())));
}

/// Capture 05 de `ui-reference`, puis variantes non capturées (survol, vide, lecture, recherche, épinglé).
fn liked_scenes() -> Vec<SidebarScene> {
  vec![
    ("05-titres-aimes", liked_capture, None),
    // Survol de la troisième ligne : fond de survol, actions visibles.
    (
      "05b-titres-aimes-survol",
      |app| {
        liked_capture(app);
        app.ui.row_hover = Some((riff_app::ui::ListId::Favorites, 2));
      },
      None,
    ),
    // Aucun favori : « No favourites yet... ».
    (
      "05c-titres-aimes-vide",
      |app| {
        liked_capture(app);
        app.data.favorites.clear();
        app.update(Message::Ui(UiMessage::FavoritesQuery(String::new())));
      },
      None,
    ),
    // La troisième ligne est en lecture.
    (
      "05d-titres-aimes-en-lecture",
      |app| {
        liked_capture(app);
        app.player.current = Some(app.data.favorites[2].clone());
        app.player.queue = app.data.favorites[..13].to_vec();
        app.player.index = Some(2);
        app.playback = Playback {
          position: Duration::from_secs(60),
          duration: Some(Duration::from_secs(225)),
          is_playing: true,
          ..Playback::default()
        };
      },
      None,
    ),
    // Recherche « luv » : les quatre titres de LuvResval.
    (
      "05e-titres-aimes-recherche",
      |app| {
        liked_capture(app);
        app.update(Message::Ui(UiMessage::FavoritesQuery("luv".to_string())));
      },
      None,
    ),
    // Recherche sans résultat : le même message vide que la liste vide.
    (
      "05f-titres-aimes-recherche-vide",
      |app| {
        liked_capture(app);
        app.update(Message::Ui(UiMessage::FavoritesQuery("zzz".to_string())));
      },
      None,
    ),
    // Quatrième titre épinglé : il passe en tête de liste.
    (
      "05g-titres-aimes-epingle",
      |app| {
        liked_capture(app);
        let id = app.data.favorites[3].canonical_id(&app.data.platform);
        app
          .data
          .pinned_tracks
          .insert("favorites".to_string(), serde_json::json!([id]));
        app.update(Message::Ui(UiMessage::FavoritesQuery(String::new())));
      },
      None,
    ),
  ]
}

/// Titres de « Random Access Memories » de `08-album` : titre, artiste, durée en secondes.
const RAM_TRACKS: [(&str, &str, f64); 13] = [
  ("Give Life Back to Music", "Daft Punk", 274.0),
  ("The Game of Love", "Daft Punk", 322.0),
  ("Giorgio by Moroder", "Daft Punk", 545.0),
  ("Within", "Daft Punk", 229.0),
  ("Instant Crush", "Daft Punk & Julian Casablancas", 338.0),
  (
    "Lose Yourself to Dance",
    "Daft Punk & Pharrell Williams",
    354.0,
  ),
  ("Touch", "Daft Punk & Paul Williams", 499.0),
  (
    "Get Lucky",
    "Daft Punk, Pharrell Williams & Nile Rodgers",
    370.0,
  ),
  ("Beyond", "Daft Punk", 290.0),
  ("Motherboard", "Daft Punk", 342.0),
  ("Fragments of Time", "Daft Punk & Todd Edwards", 280.0),
  ("Doin' it Right", "Daft Punk & Panda Bear", 251.0),
  ("Contact", "Daft Punk", 381.0),
];

/// Réponse de `/api/albums/tracks` pour l'album de `08-album` (`no` : numéro du titre, `app.js:9707`).
fn ram_reply() -> serde_json::Value {
  let tracks: Vec<_> = RAM_TRACKS
    .iter()
    .enumerate()
    .map(|(n, (title, artist, duration))| {
      serde_json::json!({ "title": title, "artist": artist, "duration": duration, "no": n + 1 })
    })
    .collect();
  serde_json::json!({
    "success": true,
    "album": { "artist": "Daft Punk", "year": "2013" },
    "tracks": tracks,
  })
}

/// Album ouvert depuis une carte de la recherche « daft punk » (carte « Random Access Memories » de `03c`), rien en
/// lecture, panneau droit ouvert : l'état des captures 08. `reply` à `false` laisse le chargement en cours.
fn open_album(app: &mut App, reply: bool) {
  nothing_playing(app);
  app.lyrics.panel_open = true;
  app.ui.search.input = "daft punk".to_string();
  app.ui.search.query = "daft punk".to_string();
  let album = result_albums().remove(1);
  app.update(Message::Ui(UiMessage::OpenAlbum(Box::new(album))));
  if reply {
    let run = app.ui.album.run;
    app.update(Message::Ui(UiMessage::Reply(
      ReplyTag::AlbumTracks { run },
      Ok(ram_reply()),
    )));
  }
}

/// Album ouvert, cinquième titre en cours de lecture (le titre dont les barres remplacent le numéro).
fn open_album_playing(app: &mut App) {
  open_album(app, true);
  app.player.current = Some(app.ui.album.tracks[4].clone());
  app.player.queue = app.ui.album.tracks.clone();
  app.player.index = Some(4);
}

/// Fond des captures de la fenêtre Audio : album ouvert, cinquième titre en lecture, panneau de droite ouvert.
/// Dialogue « Rename track » (`button::filled` « Save » et `text_button` « Cancel ») ; la vue est construite une fois pour que les
/// boutons génériques existent (`motion::button_key`).
fn rename_dialog(app: &mut App) {
  open_album_playing(app);
  app.ui.modal = Some(Modal::Rename {
    track: Box::new(track(0)),
    title: "bipolar".to_string(),
    artist: "diedlonely".to_string(),
  });
  drop(app.view());
}

fn audio_open(app: &mut App, tab: AudioTab) {
  open_album_playing(app);
  app.playback.is_playing = true;
  app.ui.modal = Some(Modal::Audio);
  app.ui.audio.tab = tab;
}

/// Premiers titres de la playlist « like » de `07-playlist-detail` de `ui-reference` : titre, artiste, durée en secondes.
const PLAYLIST_CAPTURE: [(&str, &str, f64); 8] = [
  ("bipolar", "diedlonely", 133.0),
  ("Fentanyl", "Freeze Corleone", 225.0),
  (
    "Asleep in space (SLOWED + REVERB + RAIN) Zachariehs",
    "dexghxst",
    278.0,
  ),
  (
    "All For Nothing (Slowed to Perfection)",
    "zachariehs",
    252.0,
  ),
  ("Don't Like.1 (Guitar Remix)", "dinoa1", 282.0),
  ("Cotton Club Freestyle (Remix)", "Josman", 71.0),
  ("LV Dans Le BOLT", "Hologram Lo'", 182.0),
  ("Goth Bitch", "1nonly", 137.0),
];

/// 567 titres dont les huit premiers sont ceux de la capture ; la durée totale vaut 1683:14 comme sur la capture.
fn playlist_capture_tracks() -> Vec<Track> {
  let mut all: Vec<Track> = (0..567)
    .map(|n| Track {
      id: Some(format!("like-{n}")),
      duration: Some(123.0 + (n * 17 % 110) as f64),
      cover: Some(cover_url(300 + n)),
      thumbnail: Some(cover_url(300 + n)),
      url: Some(format!("https://exemple.invalid/like-{n}")),
      ..track(n)
    })
    .collect();
  for (n, (title, artist, duration)) in PLAYLIST_CAPTURE.iter().enumerate() {
    all[n].title = Some((*title).to_string());
    all[n].artist = Some((*artist).to_string());
    all[n].album = None;
    all[n].duration = Some(*duration);
  }
  let others: f64 = all[..566].iter().filter_map(|t| t.duration).sum();
  all[566].duration = Some(1683.0 * 60.0 + 14.0 - others);
  all
}

/// Libellés français du profil capturé : `Play`, `Download all` et `Export as JSON` sont traduits sur 07, pas le reste du détail.
fn french_playlist() -> I18n {
  let dir = temp::TempDir::new("ui-snapshots-i18n-playlist");
  fs::write(
    dir.join("fr.json"),
    r#"{"Play":"Jouer","Download all":"Téléchargez tout","Export as JSON":"Exporter au format JSON","Playlists":"Listes de lecture","Queue":"File d'attente"}"#,
  )
  .expect("traduction");
  I18n::load("fr", &dir)
}

/// Profil de la capture 07 : celui de 04a, puis la playlist « like » de 567 titres ouverte.
fn playlist_capture(app: &mut App) {
  sidebar_capture(app, SidebarTab::Playlists);
  app.i18n = std::sync::Arc::new(french_playlist());
  app.data.playlists = vec![Playlist {
    tracks: playlist_capture_tracks(),
    ..playlist("pl-like", "like", 0..0)
  }];
  app.update(Message::Ui(UiMessage::OpenPlaylist("pl-like".to_string())));
}

/// Scènes du détail de playlist 07b à 07i : survol, vide, recherche, lecture, menu de tri, description, sans pochette.
fn playlist_scenes() -> Vec<Scene> {
  vec![
    // Pointeur sur la troisième ligne : fond de survol, bouton favori visible.
    ("07b-playlist-detail-survol", |app| {
      playlist_capture(app);
      app.ui.row_hover = Some((ListId::Playlist, 2));
    }),
    // Playlist sans titre : « This playlist is empty », nombre à 0, pochette de repli.
    ("07c-playlist-detail-vide", |app| {
      playlist_capture(app);
      app.data.playlists[0].tracks.clear();
      app.update(Message::Ui(UiMessage::PlaylistQuery(String::new())));
    }),
    // Recherche « zach » : les titres dont le titre ou l'artiste la contient, et « 3 of 567 tracks ».
    ("07d-playlist-detail-recherche", |app| {
      playlist_capture(app);
      app.update(Message::Ui(UiMessage::PlaylistQuery("zach".to_string())));
    }),
    // Recherche sans résultat : le même message vide.
    ("07e-playlist-detail-recherche-vide", |app| {
      playlist_capture(app);
      app.update(Message::Ui(UiMessage::PlaylistQuery("zzz".to_string())));
    }),
    // La troisième ligne est en lecture (barres animées, titre en `primary`).
    ("07f-playlist-detail-en-lecture", |app| {
      playlist_capture(app);
      let tracks = app.data.playlists[0].tracks.clone();
      app.player.current = Some(tracks[2].clone());
      app.player.queue = tracks[..13].to_vec();
      app.player.index = Some(2);
      app.playback = Playback {
        position: Duration::from_secs(60),
        duration: Some(Duration::from_secs(278)),
        is_playing: true,
        ..Playback::default()
      };
    }),
    // Menu de tri ouvert : trois options, la courante en `primary`.
    ("07g-playlist-detail-menu-tri", |app| {
      playlist_capture(app);
      app.ui.playlist_sort_open = true;
    }),
    // Description renseignée et tri par date d'ajout : libellé du déclencheur et option choisie.
    ("07h-playlist-detail-description", |app| {
      playlist_capture(app);
      app.data.playlists[0].description = Some("Mes sons de nuit, à écouter en boucle".to_string());
      app.data.playlist_sort = riff_app::user_data::SortMode::AddedDesc;
      app.ui.playlist_sort_open = true;
    }),
    // Aucune pochette : icône de note de musique sur `surface-container-high`, pas de halo.
    ("07i-playlist-detail-sans-pochette", |app| {
      playlist_capture(app);
      for track in &mut app.data.playlists[0].tracks {
        track.cover = None;
        track.thumbnail = None;
      }
      app.update(Message::Ui(UiMessage::PlaylistQuery(String::new())));
    }),
  ]
}

/// Titres populaires de `09-artiste` de `ui-reference` : titre, durée en secondes. Les quatre premiers sont visibles
/// sur la capture, les suivants complètent « Show all ».
const ARTIST_CAPTURE: [(&str, f64); 8] = [
  ("Instant Crush (feat. Julian Casablancas)", 337.0),
  (
    "Get Lucky (Radio Edit - feat. Pharrell Williams and Nile Rodgers)",
    248.0,
  ),
  ("Veridis Quo", 345.0),
  ("One More Time", 320.0),
  ("Around the World", 429.0),
  ("Harder, Better, Faster, Stronger", 224.0),
  ("Digital Love", 301.0),
  ("Lose Yourself to Dance", 353.0),
];

fn artist_tracks(count: usize) -> Vec<Track> {
  ARTIST_CAPTURE
    .iter()
    .take(count)
    .enumerate()
    .map(|(n, (title, duration))| Track {
      id: Some(format!("artist-{n}")),
      title: Some((*title).to_string()),
      artist: Some("Daft Punk".to_string()),
      album: None,
      duration: Some(*duration),
      cover: Some(cover_url(300 + n)),
      thumbnail: Some(cover_url(300 + n)),
      url: Some(format!("https://exemple.invalid/artist-{n}")),
      platform: Some("ytmusic".to_string()),
      ..Track::default()
    })
    .collect()
}

fn artist_albums() -> Vec<SavedAlbum> {
  [
    ("Random Access Memories", "2013"),
    ("Discovery", "2001"),
    ("Homework", "1997"),
    ("Human After All", "2005"),
    ("Alive 2007", "2007"),
    ("TRON: Legacy", "2010"),
    ("Musique Vol. 1", "2006"),
    ("Alive 1997", "2001"),
  ]
  .iter()
  .enumerate()
  .map(|(n, (title, year))| SavedAlbum {
    id: Some(format!("artist-album-{n}")),
    title: Some((*title).to_string()),
    artist: Some("Daft Punk".to_string()),
    cover: Some(cover_url(320 + n)),
    year: Some((*year).to_string()),
    ..SavedAlbum::default()
  })
  .collect()
}

/// Libellés français de la capture 09 : « Tracks » devient « Pistes » et « Bio » « Biographie » (cache de
/// traduction du profil capturé) ; « Follow », « Block », « Popular Songs » et « Show all » restent en anglais.
fn french_artist() -> I18n {
  let dir = temp::TempDir::new("ui-snapshots-i18n-artist");
  fs::write(
    dir.join("fr.json"),
    r#"{"Tracks":"Pistes","Bio":"Biographie"}"#,
  )
  .expect("traduction");
  I18n::load("fr", &dir)
}

/// Capture 09 de `ui-reference` : profil de 04a, recherche « daft punk » dans la barre, page de Daft Punk ouverte
/// depuis l'album (rien en lecture, bannière, cinq titres populaires dont huit au total, huit albums).
fn artist_capture(app: &mut App) {
  sidebar_capture(app, SidebarTab::Playlists);
  app.i18n = std::sync::Arc::new(french_artist());
  app.ui.search.input = "daft punk".to_string();
  app.ui.search.query = "daft punk".to_string();
  app.ui.search.icon_cover = Some(cover_url(60));
  app.ui.artist = ArtistView {
    name: "Daft Punk".to_string(),
    tracks: artist_tracks(8),
    albums: artist_albums(),
    bio: "Daft Punk est un duo de musique électronique français.".to_string(),
    banner: Some(cover_url(299)),
    avatar: Some(cover_url(299)),
    fans_line: "6,501,975 monthly listeners".to_string(),
    fans: 6_501_975.0,
    verified: true,
    ..ArtistView::default()
  };
  go(app, Screen::Artist);
}

/// Capture 09 avec la troisième ligne en lecture.
fn artist_playing(app: &mut App) {
  artist_capture(app);
  app.player.current = Some(app.ui.artist.tracks[2].clone());
  app.player.queue = app.ui.artist.tracks[..5].to_vec();
  app.player.index = Some(2);
  app.playback = Playback {
    position: Duration::from_secs(60),
    duration: Some(Duration::from_secs(345)),
    is_playing: true,
    ..Playback::default()
  };
}

/// Capture 14 : page artiste, « Instant Crush » en lecture, menu ouvert au point où la capture le place.
fn context_menu_artist(app: &mut App) {
  artist_capture(app);
  app.player.current = Some(app.ui.artist.tracks[0].clone());
  app.player.queue = app.ui.artist.tracks[..5].to_vec();
  app.player.index = Some(0);
  app.playback = Playback {
    position: Duration::from_secs(60),
    duration: Some(Duration::from_secs(337)),
    is_playing: true,
    ..Playback::default()
  };
  app.ui.row_hover = Some((ListId::Artist, 0));
  app.update(Message::Ui(UiMessage::RowMenu(
    ListId::Artist,
    0,
    Point::new(475.0, 386.0),
  )));
}

/// Capture 14b : Titres aimés, rien en lecture, menu sur la troisième ligne.
fn context_menu_liked(app: &mut App) {
  liked_capture(app);
  nothing_playing(app);
  app.update(Message::Ui(UiMessage::RowMenu(
    ListId::Favorites,
    2,
    Point::new(574.0, 330.0),
  )));
}

/// Centre de l'entrée `index` du menu de 14b (pas de 42 px CSS, séparateur de 9 px CSS et 2 px d'écart avant les
/// entrées de blocage), en pixels écran.
fn context_menu_item_center(index: usize, after_divider: bool) -> (f32, f32) {
  use riff_app::theme::tokens::{context_menu as m, css};
  let divider = if after_divider {
    m::DIVIDER_HEIGHT + 2.0 * m::DIVIDER_MARGIN_Y + m::GAP
  } else {
    0.0
  };
  (
    574.0 + 100.0,
    330.0 + m::PADDING + index as f32 * (m::ITEM_HEIGHT + m::GAP) + divider + css(20.0),
  )
}

/// Variantes de 14 qui placent le pointeur : survol d'une entrée, bords de la fenêtre.
fn context_menu_scenes() -> Vec<SidebarScene> {
  vec![
    // Pointeur sur « Share link » (sixième entrée).
    (
      "14c-menu-contextuel-titre-survol",
      context_menu_liked,
      Some(context_menu_item_center(5, false)),
    ),
    // Pointeur sur « Block track » : survol rouge des entrées `danger`.
    (
      "14d-menu-contextuel-titre-survol-danger",
      context_menu_liked,
      Some(context_menu_item_center(9, true)),
    ),
    // Menu ouvert près du bord droit : il s'ouvre à gauche du pointeur.
    (
      "14e-menu-contextuel-titre-bord-droit",
      |app| {
        liked_capture(app);
        app.update(Message::Ui(UiMessage::RowMenu(
          ListId::Favorites,
          2,
          Point::new(1850.0, 330.0),
        )));
      },
      None,
    ),
    // Menu ouvert près du bord bas : il s'ouvre au-dessus du pointeur.
    (
      "14f-menu-contextuel-titre-bord-bas",
      |app| {
        liked_capture(app);
        app.update(Message::Ui(UiMessage::RowMenu(
          ListId::Favorites,
          2,
          Point::new(574.0, 900.0),
        )));
      },
      None,
    ),
    // Détail de playlist : séparateur avant « Remove from playlist », qui est `danger`.
    (
      "14g-menu-contextuel-titre-playlist",
      |app| {
        playlist_capture(app);
        app.update(Message::Ui(UiMessage::RowMenu(
          ListId::Playlist,
          2,
          Point::new(574.0, 330.0),
        )));
      },
      None,
    ),
    // Clic droit sur le titre de la barre de lecture (`CurrentMenu`) : le menu s'ouvre au-dessus du pointeur.
    (
      "14h-menu-contextuel-titre-barre",
      |app| {
        app.update(Message::Ui(UiMessage::CurrentMenu(Point::new(
          110.0, 1030.0,
        ))));
      },
      None,
    ),
  ]
}

/// Premiers titres de l'historique de `10-historique` de `ui-reference` : titre, artiste, durée en secondes,
/// pastille explicite.
const HISTORY_CAPTURE: [(&str, &str, f64, bool); 13] = [
  ("Fentanyl", "Freeze Corleone", 225.0, false),
  ("température", "Mairo", 208.0, false),
  ("cherry blossoms", "mxlu", 129.0, false),
  ("Hooligan", "Rushy", 162.0, false),
  ("SAD", "Glizz", 140.0, true),
  ("J'SAVAIS", "Ikaz Boi", 137.0, false),
  ("Star Shopping", "Lil Peep", 141.0, true),
  (
    "Wallace Cleaver & 1PLIKÉ140 - Aidez-moi/Aimez-moi [kingcsix remix]",
    "Kingcsix",
    171.0,
    false,
  ),
  ("xénon", "Wallace Cleaver", 218.0, false),
  ("JOURNAL DE QUÊTES", "LUTHER", 172.0, false),
  ("8am in London", "DC3", 183.0, false),
  ("S5-E36 (Return Of The Mack)", "Next Up", 191.0, true),
  ("the hood dream", "808mystic", 225.0, false),
];

/// Quarante titres écoutés dont les treize premiers sont ceux de la capture.
fn history_tracks() -> Vec<Track> {
  let mut all = tracks(0..40);
  for (n, (title, artist, duration, explicit)) in HISTORY_CAPTURE.iter().enumerate() {
    let mut track = Track {
      id: Some(format!("history-{n}")),
      title: Some((*title).to_string()),
      artist: Some((*artist).to_string()),
      album: None,
      duration: Some(*duration),
      cover: Some(cover_url(300 + n)),
      thumbnail: Some(cover_url(300 + n)),
      url: Some(format!("https://exemple.invalid/history-{n}")),
      ..all[n].clone()
    };
    if *explicit {
      track
        .extra
        .insert("isExplicit".to_string(), serde_json::json!(true));
    }
    all[n] = track;
  }
  all
}

/// Profil de la capture 10 : celui de 04a, puis « Historique ».
fn history_capture(app: &mut App) {
  view_capture(app, Screen::History);
  app.data.history = history_tracks();
}

/// Profil de la capture 11a : celui de 04a (rien en lecture), « Playback queue » ouverte, file vide.
fn queue_capture(app: &mut App) {
  view_capture(app, Screen::Queue);
  nothing_playing(app);
}

/// File de 13 titres (ceux de l'historique de 10) pour les états de la vue non capturés.
fn queue_filled(app: &mut App) {
  queue_capture(app);
  app.player.queue = history_tracks()[..13].to_vec();
}

/// Profil de la capture 11b : page de Daft Punk, premier titre en lecture, panneau ouvert sur « Queue ».
fn queue_panel_capture(app: &mut App) {
  artist_capture(app);
  app.lyrics.panel_open = false;
  let queue = artist_tracks(8);
  app.player.current = Some(queue[0].clone());
  app.player.queue = queue;
  app.player.index = Some(0);
  app.playback = Playback {
    position: Duration::from_secs(60),
    duration: Some(Duration::from_secs(320)),
    is_playing: true,
    ..Playback::default()
  };
  app.ui.queue_panel.open = true;
  app.ui.queue_panel.tab = riff_app::ui::QueueTab::Queue;
}

fn scenes() -> Vec<Scene> {
  vec![
    // Accueil au lancement, rien en lecture (captures 01).
    ("01-accueil", |app| {
      go(app, Screen::Home);
      nothing_playing(app);
      app.lyrics.panel_open = true;
    }),
    // Accueil pendant la lecture d'un titre (captures 01b).
    ("01b-accueil-lecture", |app| {
      go(app, Screen::Home);
      app.lyrics.lyrics = None;
      app.lyrics.panel_open = true;
    }),
    // Premier chargement : ni vague, ni mix, ni tendances en cache. Non capturé par `ui-reference`.
    ("01c-accueil-chargement", |app| {
      go(app, Screen::Home);
      nothing_playing(app);
      app.lyrics.panel_open = true;
      app.ui.home.wave.clear();
      app.ui.home.trending.clear();
      app.ui.home.wave_loading = true;
      app.ui.home.trending_loading = true;
      app.ui.home.mixes_busy = true;
      app.ui.home.mixes_planned = 5;
      app.data.daily_mixes = Some(DailyMixes::default());
    }),
    // Riff vierge : aucune donnée, encart d'import visible. Non capturé par `ui-reference`.
    ("01d-accueil-vierge", |app| {
      go(app, Screen::Home);
      nothing_playing(app);
      app.lyrics.panel_open = true;
      app.data.favorites.clear();
      app.data.history.clear();
      app.data.playlists.clear();
      app.data.daily_mixes = None;
      app.ui.home.wave.clear();
      app.ui.home.trending.clear();
    }),
    // Pointeur sur une carte de « Pour toi » (levée de 2 px) : `08a-survol-carte` n'existe pas pour l'accueil.
    ("01e-accueil-survol-carte", |app| {
      go(app, Screen::Home);
      nothing_playing(app);
      app.lyrics.panel_open = true;
      app.ui.home.hover = Some((HomeRow::Made, 2));
    }),
    // Pointeur sur une tuile de « Accueil ».
    ("01f-accueil-survol-tuile", |app| {
      go(app, Screen::Home);
      nothing_playing(app);
      app.lyrics.panel_open = true;
      app.ui.home.hover = Some((HomeRow::Quick, 1));
    }),
    // Rangées qui débordent : fondu de droite de `fade_strip` (angle du dégradé).
    ("01g-accueil-fondu-rangee", |app| {
      go(app, Screen::Home);
      nothing_playing(app);
      app.lyrics.panel_open = true;
    }),
    // « Mes fichiers » en lecture, données du banc (les captures vides sont 04e).
    ("04e8-mes-fichiers-en-lecture", |app| {
      go(app, Screen::Library)
    }),
    ("04-titres-aimes", |app| go(app, Screen::Liked)),
    ("05-playlists", |app| go(app, Screen::Playlists)),
    // Ancre 07 : scènes du détail de playlist (07*) à insérer juste sous cette ligne.
    ("07-playlist-detail", playlist_capture),
    // Ancre 11 : scènes de la file d'attente, vue et panneau (11*) à insérer juste sous cette ligne.
    ("11a-file-attente", queue_capture),
    // Panneau flottant ouvert sur la page de Daft Punk (capture 11b).
    ("11b-file-attente-panneau", queue_panel_capture),
    // File de 13 titres, rien en lecture. Non capturé par `ui-reference`.
    ("11c-file-attente-liste", queue_filled),
    // Survol de la troisième ligne : fond de survol et bouton de retrait. Non capturé.
    ("11d-file-attente-survol", |app| {
      queue_filled(app);
      app.ui.row_hover = Some((riff_app::ui::ListId::Queue, 2));
    }),
    // La troisième ligne est en lecture. Non capturé.
    ("11e-file-attente-en-lecture", |app| {
      queue_filled(app);
      app.player.current = Some(app.player.queue[2].clone());
      app.player.index = Some(2);
      app.playback = Playback {
        position: Duration::from_secs(60),
        duration: Some(Duration::from_secs(129)),
        is_playing: true,
        ..Playback::default()
      };
    }),
    // Panneau ouvert, rien en lecture : « Nothing is playing ». Non capturé.
    ("11f-panneau-sans-lecture", |app| {
      queue_capture(app);
      app.ui.queue_panel.open = true;
    }),
    // Dernier titre de la file en lecture : « Nothing queued next ». Non capturé.
    ("11g-panneau-fin-de-file", |app| {
      queue_panel_capture(app);
      app.player.index = Some(7);
      app.player.current = Some(app.player.queue[7].clone());
    }),
    // Onglet « Recently played » avec l'historique. Non capturé (`ui-reference`, section « Non capturé »).
    ("11h-panneau-recemment-joues", |app| {
      queue_panel_capture(app);
      app.data.history = history_tracks();
      app.ui.queue_panel.tab = riff_app::ui::QueueTab::Recent;
    }),
    // Onglet « Recently played » sans historique : « Nothing played yet ». Non capturé.
    ("11i-panneau-recemment-vide", |app| {
      queue_panel_capture(app);
      app.data.history.clear();
      app.ui.queue_panel.tab = riff_app::ui::QueueTab::Recent;
    }),
    // Ancre 08 : scènes de l'album (08*) à insérer juste sous cette ligne.
    // Capture `08-album` : Random Access Memories, 13 titres, bouton d'enregistrement masqué (`riff_show_albums` à 0).
    ("08-album", |app| open_album(app, true)),
    // Pointeur sur la troisième ligne : fond `surface-container-high` et bouton favori. Non capturé.
    ("08b-album-survol-ligne", |app| {
      open_album(app, true);
      app.ui.row_hover = Some((ListId::Album, 2));
    }),
    // `riff_show_albums` à 1, album pas encore enregistré : cœur vide à la suite des détails. Non capturé.
    ("08c-album-enregistrer", |app| {
      open_album(app, true);
      app.settings.show_albums = true;
    }),
    // Album enregistré : cœur plein. Non capturé.
    ("08d-album-enregistre", |app| {
      open_album(app, true);
      app.settings.show_albums = true;
      let album = app.ui.album.album.clone();
      app.data.saved_albums.insert(0, album);
    }),
    // Un titre de l'album en cours de lecture : barres et titre `primary`. Non capturé.
    ("08e-album-lecture", open_album_playing),
    // Titres demandés au serveur, pas encore là : `.g-spinner` sous la pochette. Non capturé.
    ("08f-album-chargement", |app| open_album(app, false)),
    // Le serveur répond sans titre : `Failed to load album` en couleur d'erreur. Non capturé.
    ("08g-album-erreur", |app| {
      open_album(app, false);
      let run = app.ui.album.run;
      app.update(Message::Ui(UiMessage::Reply(
        ReplyTag::AlbumTracks { run },
        Ok(serde_json::json!({ "success": false, "error": "Failed to load album tracks" })),
      )));
    }),
    // Album local sans pochette : aplat `surface-container` à la place de l'image, aucun fond flou. Non capturé.
    ("08h-album-local-sans-pochette", |app| {
      nothing_playing(app);
      app.lyrics.panel_open = true;
      app.data.local_files = (0..5)
        .map(|n| Track {
          album: Some("Démos 2024".to_string()),
          cover: None,
          thumbnail: None,
          ..track(30 + n)
        })
        .collect();
      app.update(Message::Ui(UiMessage::OpenLocalAlbum(
        "Démos 2024".to_string(),
      )));
    }),
    // Mix quotidien non noté : mosaïque 2 x 2, artistes en texte simple, pastille « Do you like these tracks? ». Non capturé.
    ("08i-album-mix-quotidien", |app| {
      nothing_playing(app);
      app.lyrics.panel_open = true;
      app.update(Message::Ui(UiMessage::OpenMix(0)));
    }),
    // Mix déjà noté : la pastille disparaît. Non capturé.
    ("08j-album-mix-note", |app| {
      nothing_playing(app);
      app.lyrics.panel_open = true;
      app.update(Message::Ui(UiMessage::OpenMix(0)));
      app.update(Message::Ui(UiMessage::RateMix(true)));
    }),
    // Titre et artistes longs : le titre passe sur trois lignes, les artistes à la ligne. Non capturé.
    ("08k-album-titre-long", |app| {
      open_album(app, true);
      app.ui.album.album.title = Some(
        "Random Access Memories (Drumless Edition) (Deluxe Anniversary Collector Box Set)"
          .to_string(),
      );
      app.ui.album.album.artist =
        Some("Daft Punk, Pharrell Williams, Nile Rodgers & Julian Casablancas".to_string());
    }),
    ("13a-paroles-panneau", |app| {
      go(app, Screen::Home);
      app.lyrics.panel_open = true;
    }),
    // Ancre 09 : scènes de la page artiste (09*) à insérer juste sous cette ligne.
    ("09-artiste", artist_capture),
    // Pointeur sur la deuxième ligne : fond de survol, bouton « ... » inchangé.
    ("09b-artiste-survol-titre", |app| {
      artist_capture(app);
      app.ui.row_hover = Some((ListId::Artist, 1));
    }),
    // Onglet Albums : non capturé par `ui-reference` (INDEX.md, « Non capturé »), rendu d'après styles.css.
    ("09c-artiste-onglet-albums", |app| {
      artist_capture(app);
      app.ui.artist.tab = ArtistTab::Albums;
    }),
    // Onglet Albums, pointeur sur la troisième carte : levée de 2 px.
    ("09d-artiste-albums-survol-carte", |app| {
      artist_capture(app);
      app.ui.artist.tab = ArtistTab::Albums;
      app.ui.card_hover = Some((CardGrid::Artist, 2));
    }),
    // Troisième titre en lecture : titre en `primary`, fond `surface-container-high`.
    ("09e-artiste-en-lecture", artist_playing),
    // Réponse du serveur attendue : chargeur, onglets masqués, aucun auditeur.
    ("09f-artiste-chargement", |app| {
      artist_capture(app);
      let view = &mut app.ui.artist;
      *view = ArtistView {
        name: view.name.clone(),
        loading: true,
        ..ArtistView::default()
      };
    }),
    // Échec de `/api/artist` : « No tracks found for this artist. », sans bannière.
    ("09g-artiste-erreur", |app| {
      artist_capture(app);
      let view = &mut app.ui.artist;
      *view = ArtistView {
        name: view.name.clone(),
        failed: true,
        ..ArtistView::default()
      };
    }),
    // Aucune bannière ni avatar : fond `primary-container` vers `surface`.
    ("09h-artiste-sans-image", |app| {
      artist_capture(app);
      app.ui.artist.banner = None;
      app.ui.artist.avatar = None;
    }),
    // « Show all » ouvert : les huit titres et « Show less ».
    ("09i-artiste-tout-afficher", |app| {
      artist_capture(app);
      app.ui.artist.expanded = true;
    }),
    // Artiste suivi et bloqué : « Following » en `primary`, « Blocked » sur fond rouge à 15 %.
    ("09j-artiste-suivi-bloque", |app| {
      artist_capture(app);
      let name = app.ui.artist.name.clone();
      app.data.toggle_follow(&name, None, None);
      app.data.toggle_block_artist(&name);
    }),
    // Artiste sans album ni biographie ni coche, quatre titres : pas d'onglets, pas de « Show all ».
    ("09k-artiste-sans-albums", |app| {
      artist_capture(app);
      let view = &mut app.ui.artist;
      view.albums.clear();
      view.bio.clear();
      view.verified = false;
      view.tracks.truncate(4);
    }),
    // Boutons Follow et Block sans « riff_highlight_buttons » : Follow prend le style des autres pastilles.
    ("09l-artiste-sans-bouton-colore", |app| {
      artist_capture(app);
      app.settings.highlight_buttons = false;
    }),
    // Ancre 15 : scènes de la lecture plein écran karaoké (15*) à insérer juste sous cette ligne.
    ("15-paroles-plein-ecran", |app| {
      karaoke_capture(app, Some(karaoke_lrc()))
    }),
    // Souris bougée : les boutons du coin haut droit sont visibles (`tv-idle` absent), titre à pleine opacité.
    ("15b-paroles-plein-ecran-commandes", |app| {
      karaoke_capture(app, Some(karaoke_lrc()));
      app.lyrics.tvk_idle = false;
    }),
    // Lecture en pause : bouton de 56 px, rayon 28.
    ("15c-paroles-plein-ecran-pause", |app| {
      karaoke_capture(app, Some(karaoke_lrc()));
      app.playback.is_playing = false;
    }),
    // Paroles sans temps : aucune ligne active, toutes à l'opacité de repos.
    ("15d-paroles-plein-ecran-non-synchronisees", |app| {
      let plain = (1..=6)
        .map(|n| format!("Ligne du banc numéro {n}"))
        .collect::<Vec<_>>()
        .join("\n");
      karaoke_capture(app, Some(plain));
    }),
    // Sans paroles : `tv-nolyrics`, colonne centrée, pochette de 36vh, barres.
    ("15e-plein-ecran-sans-paroles", |app| {
      karaoke_capture(app, None)
    }),
    // Paroles en cours de chargement : même vue que sans paroles.
    ("15f-plein-ecran-chargement", |app| {
      karaoke_capture(app, None);
      app.lyrics.loading = true;
    }),
    // Mode pochette (`#btn-tvk-cover-mode`) : pochette de 48vh.
    ("15g-plein-ecran-mode-pochette", |app| {
      karaoke_capture(app, Some(karaoke_lrc()));
      app.settings.visual.tvk_cover_mode = true;
    }),
    // Ancre 14 : scènes du menu contextuel d'un titre (14*) à insérer juste sous cette ligne.
    // Menu contextuel ouvert depuis la page artiste, un titre en lecture (capture 14).
    ("14-menu-contextuel-titre", context_menu_artist),
    // Menu contextuel ouvert depuis Titres aimés, rien en lecture (capture 14b).
    ("14b-menu-contextuel-titre-aimes", context_menu_liked),
    // Ancre 10 : scènes de l'historique (10*) à insérer juste sous cette ligne.
    ("10-historique", history_capture),
    // Survol de la troisième ligne : fond de survol, actions visibles.
    ("10b-historique-survol", |app| {
      history_capture(app);
      app.ui.row_hover = Some((riff_app::ui::ListId::History, 2));
    }),
    // Historique vide : « No tracks to show » (`renderTracks`, contexte `history`).
    ("10c-historique-vide", |app| {
      history_capture(app);
      app.data.history.clear();
    }),
    // La troisième ligne est en lecture.
    ("10d-historique-en-lecture", |app| {
      history_capture(app);
      app.player.current = Some(app.data.history[2].clone());
      app.player.queue = app.data.history[..13].to_vec();
      app.player.index = Some(2);
      app.playback = Playback {
        position: Duration::from_secs(60),
        duration: Some(Duration::from_secs(129)),
        is_playing: true,
        ..Playback::default()
      };
    }),
    // Ancre 21 : scènes des réglages, cadre et catégories (21* à 31*) à insérer juste sous cette ligne.
    ("21-reglages-interface", |app| {
      settings(app, Category::Interface)
    }),
    // Défilements de `.settings-pane` (captures 21-p2 à 21-p6) : 638 px par page.
    ("21b-reglages-interface-p2", |app| {
      settings(app, Category::Interface)
    }),
    ("21c-reglages-interface-p3", |app| {
      settings(app, Category::Interface)
    }),
    ("21d-reglages-interface-p4", |app| {
      settings(app, Category::Interface)
    }),
    ("21e-reglages-interface-p5", |app| {
      settings(app, Category::Interface)
    }),
    ("21f-reglages-interface-p6", |app| {
      settings(app, Category::Interface)
    }),
    ("22-reglages-theme", |app| settings(app, Category::Theme)),
    ("22b-reglages-theme-p2", |app| {
      settings(app, Category::Theme)
    }),
    ("22c-reglages-theme-p3", |app| {
      settings(app, Category::Theme)
    }),
    // Mode automatique : « Intensity » et « Brightness » sont grisées (cible 22-p1).
    ("22d-reglages-theme-auto", |app| {
      settings(app, Category::Theme);
      app.update(Message::Settings(SettingsMessage::SetText(
        TextKey::ThemeMode,
        "auto".to_string(),
      )));
    }),
    ("23-reglages-bandeaux", |app| {
      settings(app, Category::Banners)
    }),
    ("23b-reglages-bandeaux-p2", |app| {
      settings(app, Category::Banners)
    }),
    ("24-reglages-titres", |app| settings(app, Category::Tracks)),
    ("24b-reglages-titres-p2", |app| {
      settings(app, Category::Tracks)
    }),
    ("24c-reglages-titres-p3", |app| {
      settings(app, Category::Tracks)
    }),
    ("24d-reglages-titres-p4", |app| {
      settings(app, Category::Tracks)
    }),
    ("25-reglages-integrations", |app| {
      settings(app, Category::Integrations)
    }),
    ("25b-reglages-integrations-p2", |app| {
      settings(app, Category::Integrations)
    }),
    ("26-reglages-stockage", |app| {
      settings(app, Category::Storage);
      storage_state(app);
    }),
    // Capture 26-p2 : le panneau ne défile que de 16 px, il reste calé en bas.
    ("26b-reglages-stockage-p2", |app| {
      settings(app, Category::Storage);
      storage_state(app);
    }),
    // Section Avancé dépliée (`riff_adv_open`) : captures 27-p1 à 27-p4.
    ("27-reglages-avance", |app| {
      settings(app, Category::Advanced);
      app.settings.adv_open = true;
    }),
    ("27b-reglages-avance-p2", |app| {
      settings(app, Category::Advanced);
      app.settings.adv_open = true;
    }),
    ("27c-reglages-avance-p3", |app| {
      settings(app, Category::Advanced);
      app.settings.adv_open = true;
    }),
    ("27d-reglages-avance-p4", |app| {
      settings(app, Category::Advanced);
      app.settings.adv_open = true;
    }),
    // Section repliée : l'état d'un Riff vierge.
    ("27e-reglages-avance-replie", |app| {
      settings(app, Category::Advanced)
    }),
    ("28-reglages-apropos", |app| settings(app, Category::About)),
    ("29-reglages-themes-perso", |app| {
      settings(app, Category::CustomThemes)
    }),
    // Capture 30 : un plugin d'exemple, désactivé.
    ("30-reglages-plugins", |app| {
      settings(app, Category::Plugins);
      app.plugins.entries = vec![riff_plugins::PluginEntry {
        file: "hello.rhai".to_string(),
        kind: riff_plugins::PluginKind::Native {
          manifest: riff_plugins::Manifest {
            name: "Hello plugin".to_string(),
            author: "Riff".to_string(),
            version: "1.0.0".to_string(),
            description: "Shows a message when the track changes. Edit or delete this file."
              .to_string(),
            permissions: Vec::new(),
          },
          consent: riff_plugins::ConsentState::Off,
        },
      }];
    }),
    ("30b-reglages-plugins-vide", |app| {
      settings(app, Category::Plugins)
    }),
    ("31-reglages-compte", |app| settings(app, Category::Account)),
    ("31b-reglages-compte-p2", |app| {
      settings(app, Category::Account)
    }),
    ("31c-reglages-compte-nav", |app| {
      settings(app, Category::Account)
    }),
    // Ancre 32 : scènes de la fenêtre Audio (32*) à insérer juste sous cette ligne.
    // Captures `32a` à `32e` de `ui-reference` : album en lecture, panneau de droite ouvert, fenêtre Audio sur l'onglet voulu.
    ("32a-audio-ameliorateur", |app| {
      audio_open(app, AudioTab::Enhance);
    }),
    ("32b-audio-vitesse", |app| {
      audio_open(app, AudioTab::Tempo);
    }),
    ("32c-audio-effets", |app| {
      audio_open(app, AudioTab::Effects);
    }),
    ("32d-audio-egaliseur", |app| {
      audio_open(app, AudioTab::Equalizer);
    }),
    ("32e-audio-dormir", |app| {
      audio_open(app, AudioTab::Sleep);
    }),
    // Haute qualité allumée : moteur et trois curseurs. Non capturé.
    ("32f-audio-ameliorateur-actif", |app| {
      audio_open(app, AudioTab::Enhance);
      app.settings.sound.hq_enabled = true;
      app.settings.sound.hq.vocal = 12.0;
      app.settings.sound.hq.bass = -20.0;
      app.data.normalize = true;
    }),
    // Réglages touchés : vitesse, hauteur et gain. Non capturé.
    ("32g-audio-vitesse-reglee", |app| {
      audio_open(app, AudioTab::Tempo);
      let live = &mut app.ui.audio.live;
      live.speed = 1.35;
      live.speed_pitch = 0.82;
      live.pitch = 3.0;
      live.volume = 1.25;
    }),
    // Effets et égaliseur touchés. Non capturé.
    ("32h-audio-effets-regles", |app| {
      audio_open(app, AudioTab::Effects);
      let live = &mut app.ui.audio.live;
      live.reverb = 65.0;
      live.distortion = 30.0;
      live.echo = 15.0;
    }),
    ("32i-audio-egaliseur-regle", |app| {
      audio_open(app, AudioTab::Equalizer);
      let live = &mut app.ui.audio.live;
      live.eq.low = 4.0;
      live.eq.high = -3.0;
      live.eq_bands = [4.0, 3.0, 1.0, 0.0, -2.0, -4.0, 0.0, 2.0, 5.0, 8.0];
    }),
    // Minuterie de 15 minutes en cours : compte à rebours en couleur primaire, bouton actif. Non capturé.
    ("32j-audio-dormir-actif", |app| {
      audio_open(app, AudioTab::Sleep);
      app.ui.audio.sleep = SleepTimer::Minutes {
        minutes: 15,
        ends_at: std::time::Instant::now() + Duration::from_secs(14 * 60 + 30),
      };
    }),
    // Onglet Output de riff-rs, absent de Riff : préréglages, sortie, fondu enchaîné. Non capturé.
    ("32k-audio-sortie", |app| {
      audio_open(app, AudioTab::Output);
    }),
    ("12-barre-lecture-pleine", |app| go(app, Screen::Home)),
    // Barre à l'arrêt, mélange actif, survol de la barre : temps affichés.
    ("12g-barre-arret-survol", |app| {
      go(app, Screen::Home);
      app.playback.is_playing = false;
      app.player.shuffle = true;
    }),
    // Lecture en cours, répétition active, pochette carrée (réglage `m3_cover` coupé).
    ("12h-barre-pochette-carree", |app| {
      go(app, Screen::Home);
      app.player.repeat = true;
      app.settings.visual.m3_cover = false;
    }),
    // Titre trop long : fondu de droite de `fade_right` (24 derniers pixels).
    ("12i-barre-titre-long", |app| {
      go(app, Screen::Home);
      if let Some(track) = app.player.current.as_mut() {
        track.title = Some(
          "Un titre vraiment beaucoup trop long pour tenir dans la barre de lecture de Riff"
            .to_string(),
        );
      }
    }),
    // Panneau droit sans titre en cours : `#right-panel.lyrics-empty` au lancement (captures 01).
    ("12-panneau-sans-lecture", |app| {
      go(app, Screen::Home);
      nothing_playing(app);
      app.lyrics.panel_open = true;
    }),
    // Titre en cours sans paroles : pochette centrée (captures 01b).
    ("12b-panneau-lecture", |app| {
      go(app, Screen::Home);
      app.lyrics.lyrics = None;
      app.lyrics.panel_open = true;
    }),
    // Paroles synchronisées : pochette en haut, lignes dessous (captures 13a).
    ("12c-panneau-paroles", |app| {
      go(app, Screen::Home);
      app.lyrics.panel_open = true;
    }),
    // Panneau masqué par `#btn-toggle-lyrics-panel` : l'écart de la grille reste (captures 13b).
    ("12d-panneau-masque", |app| {
      go(app, Screen::Home);
      app.lyrics.panel_open = false;
    }),
    // Commandes du pointeur (`.rp-controls-visible`) : non capturées par `ui-reference`.
    ("12e-panneau-commandes", |app| {
      go(app, Screen::Home);
      app.lyrics.panel_open = true;
      app.ui.panel_controls.visible = true;
    }),
    // Bannière activée (`riff_adv_track_banners`, coupée par défaut) : non capturée par `ui-reference`.
    ("12f-panneau-banniere", |app| {
      go(app, Screen::Home);
      app.lyrics.panel_open = true;
      app.settings.visual.track_banners = true;
      let decoded = gradient(&ImageKey::new("https://covers.invalid/banner.jpg", 256));
      app.visuals.banner.current = Some(iced::widget::image::Handle::from_rgba(
        decoded.width,
        decoded.height,
        decoded.rgba,
      ));
    }),
  ]
}

/// Réglages du profil des captures de `ui-reference` (`storage.json` : accent `#7342d7`, luminosité 66,
/// intensité 85, mode `auto`), pas ceux d'un Riff vierge.
fn captured_settings() -> NativeSettings {
  NativeSettings {
    theme_accent: "#7342D7".to_string(),
    theme_brightness: 66,
    ..NativeSettings::default()
  }
}

fn settings(app: &mut App, category: Category) {
  // La fenêtre se pose sur la page d'où l'on vient : l'album de la capture (`ui-reference/screens/21*.png`).
  open_album(app, true);
  go(app, Screen::Settings);
  app.update(Message::Settings(SettingsMessage::OpenCategory(category)));
}

/// Stockage de la capture 26 : outils installés, dossiers par défaut du profil capturé.
fn storage_state(app: &mut App) {
  let manage = &mut app.ui.manage;
  manage.tools.status.ytdlp = true;
  manage.tools.status.ffmpeg = true;
  manage.storage.data_dir = r"D:DownloadsRiff".to_string();
  manage.storage.download_dir = r"D:Downloads".to_string();
}

fn sizes() -> Vec<(Size, &'static str)> {
  let mut sizes = vec![(Size::new(1920.0, 1080.0), "")];
  if std::env::var_os("RIFF_SNAPSHOT_QHD").is_some_and(|v| v == "1") {
    sizes.push((Size::new(2560.0, 1440.0), "-2560x1440"));
  }
  sizes
}

#[test]
#[ignore = "banc de rendu : voir l'en-tête du fichier"]
fn write_screen_snapshots() {
  for (size, suffix) in sizes() {
    for (name, prepare) in scenes() {
      let mut app = base_app();
      resize(&mut app, size);
      prepare(&mut app);
      render_scene(&mut app, &format!("{name}{suffix}"), size);
    }
    // Champ de recherche avec le focus : clic au centre de la pastille (960 px, 68 px à l'échelle 1).
    let (x, y) = (size.width / 2.0, size.height * 68.0 / 1080.0);
    let focus = |sim: &mut Simulator<'_, Message>| {
      sim.point_at(Point::new(x, y));
      sim.simulate(iced_test::simulator::click());
    };
    let mut app = base_app();
    resize(&mut app, size);
    go(&mut app, Screen::Home);
    render_with(
      &mut app,
      &format!("02b-recherche-focus{suffix}"),
      size,
      focus,
    );
    for (name, prepare) in result_scenes() {
      let mut app = base_app();
      resize(&mut app, size);
      prepare(&mut app);
      render(&mut app, &format!("{name}{suffix}"), size);
    }
    for (name, prepare, pointer) in sidebar_scenes() {
      let mut app = base_app();
      resize(&mut app, size);
      prepare(&mut app);
      let scale = size.width / 1920.0;
      render_with(&mut app, &format!("{name}{suffix}"), size, move |sim| {
        if let Some((x, y)) = pointer {
          sim.point_at(Point::new(x * scale, y * scale));
        }
      });
    }
    for (name, prepare, pointer) in liked_scenes() {
      let mut app = base_app();
      resize(&mut app, size);
      prepare(&mut app);
      let scale = size.width / 1920.0;
      render_with(&mut app, &format!("{name}{suffix}"), size, move |sim| {
        if let Some((x, y)) = pointer {
          sim.point_at(Point::new(x * scale, y * scale));
        }
      });
    }
    for (name, prepare, pointer) in context_menu_scenes() {
      let mut app = base_app();
      resize(&mut app, size);
      prepare(&mut app);
      let scale = size.width / 1920.0;
      render_with(&mut app, &format!("{name}{suffix}"), size, move |sim| {
        if let Some((x, y)) = pointer {
          sim.point_at(Point::new(x * scale, y * scale));
        }
      });
    }
    for (name, prepare) in playlist_scenes() {
      let mut app = base_app();
      resize(&mut app, size);
      prepare(&mut app);
      render(&mut app, &format!("{name}{suffix}"), size);
    }
    for (name, prepare, pointer) in view_scenes() {
      let mut app = base_app();
      resize(&mut app, size);
      prepare(&mut app);
      let scale = size.width / 1920.0;
      render_with(&mut app, &format!("{name}{suffix}"), size, move |sim| {
        if let Some((x, y)) = pointer {
          sim.point_at(Point::new(x * scale, y * scale));
        }
      });
    }
    for (name, prepare) in search_scenes() {
      let mut app = base_app();
      resize(&mut app, size);
      prepare(&mut app);
      render_with(&mut app, &format!("{name}{suffix}"), size, focus);
    }
    // Ancre 17 : scènes du mode compact (17*) à insérer juste sous cette ligne.
    // Mode compact : fenêtre réduite (380x160), puis agrandie à 380x360 avec les mini-paroles.
    for (name, size, prepare, focus) in compact_scenes() {
      let mut app = base_app();
      app.window.enter_mini();
      resize(&mut app, riff_app::window::MINI_SIZE);
      prepare(&mut app);
      resize(&mut app, size);
      render_with(&mut app, &format!("{name}{suffix}"), size, focus);
    }
    if suffix.is_empty() {
      write_motion_snapshots(size);
    }
  }
}

/// Type d'une scène du mode compact : nom, taille de fenêtre, préparation de l'état, interaction.
type CompactScene = (
  &'static str,
  Size,
  fn(&mut App),
  fn(&mut Simulator<'_, Message>),
);

/// Paroles inventées du banc du mode compact : une ligne toutes les 4 secondes, à partir de 5 s (pas de ligne de pause avant la première).
fn compact_lrc() -> String {
  [
    "Première ligne du banc",
    "Deuxième ligne du banc",
    "Troisième ligne du banc",
    "Quatrième ligne du banc",
    "Cinquième ligne du banc",
    "Sixième ligne du banc",
    "Septième ligne du banc",
    "Huitième ligne du banc",
    "Neuvième ligne du banc",
    "Dixième ligne du banc (écho)",
  ]
  .iter()
  .enumerate()
  .map(|(i, text)| {
    let seconds = 5 + i * 4;
    format!("[{:02}:{:02}.00]{text}", seconds / 60, seconds % 60)
  })
  .collect::<Vec<_>>()
  .join(
    "
",
  )
}

/// Mode compact avec les mini-paroles ouvertes, lecture à `seconds` secondes.
fn compact_lyrics(app: &mut App, seconds: u64) {
  app.update(Message::ToggleMiniLyrics);
  let token = app.lyrics.token;
  app.update(Message::Ui(UiMessage::Reply(
    ReplyTag::Lyrics { token },
    Ok(serde_json::json!({ "success": true, "lyrics": compact_lrc() })),
  )));
  app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(
    seconds,
  ))));
  // La ligne active est choisie par l'image d'animation, pas par l'événement du moteur.
  app.animation_tick(std::time::Instant::now(), &[0; 64]);
}

/// Position de lecture à `seconds` secondes, ligne active recalculée.
fn compact_position(app: &mut App, seconds: f64) {
  app.update(Message::Audio(AudioEvent::Position(
    Duration::from_secs_f64(seconds),
  )));
  app.animation_tick(std::time::Instant::now(), &[0; 64]);
}

fn compact_scenes() -> Vec<CompactScene> {
  let mini = riff_app::window::MINI_SIZE;
  let tall = Size::new(mini.width, 360.0);
  let none: fn(&mut Simulator<'_, Message>) = |_| {};
  vec![
    // Cibles 17a et 17b : lecture avancée à 91 % de la piste (vague lavande de x 14 à 335, piste grise jusqu'à 365).
    (
      "17a-mode-compact-mini",
      mini,
      |app| compact_position(app, 191.0),
      none,
    ),
    (
      "17b-mode-compact-mini-paroles",
      tall,
      |app| {
        compact_lyrics(app, 0);
        // Piste de 5 s à 4,55 s : 91 % de progression, et avant la première ligne (5 s) donc aucune ligne active.
        app.playback.duration = Some(Duration::from_secs(5));
        compact_position(app, 4.55);
      },
      none,
    ),
    (
      "17c-mode-compact-mini-paroles-ligne-active",
      tall,
      |app| compact_lyrics(app, 21),
      none,
    ),
    (
      "17d-mode-compact-mini-paroles-fin",
      tall,
      |app| compact_lyrics(app, 42),
      none,
    ),
    (
      "17e-mode-compact-mini-paroles-absentes",
      tall,
      |app| {
        compact_lyrics(app, 0);
        app.lyrics.lyrics = None;
      },
      none,
    ),
    (
      "17f-mode-compact-mini-rien-en-lecture",
      mini,
      nothing_playing,
      none,
    ),
    (
      "17g-mode-compact-mini-pause",
      mini,
      |app| app.playback.is_playing = false,
      none,
    ),
    (
      "17h-mode-compact-mini-survol-precedent",
      mini,
      |_| {},
      |simulator| simulator.point_at(Point::new(238.0, 79.0)),
    ),
    (
      "17i-mode-compact-mini-survol-fermer",
      mini,
      |_| {},
      |simulator| simulator.point_at(Point::new(352.0, 13.0)),
    ),
  ]
}

/// Splash de démarrage dont l'animation part `offset_ms` après le déclencheur `t0` (latence d'injection de la capture de Riff).
fn splash_at(app: &mut App, t0: Instant, offset_ms: i64) {
  go(app, Screen::Home);
  nothing_playing(app);
  let shift = Duration::from_millis(offset_ms.unsigned_abs());
  app.splash = Some(riff_app::splash::Splash {
    start: Some(if offset_ms < 0 {
      t0 - shift
    } else {
      t0 + shift
    }),
  });
}

/// Paroles de la capture `15-paroles` (« the hood dream », 808mystic) : début de chaque ligne en secondes. Les écarts de la quatrième à la
/// septième ligne sont ceux mesurés entre les sauts de `samples.json` (1733, 1505 et 1225 ms).
const HOOD_DREAM: [(f64, &str); 7] = [
  (10.0, "My headphones goin' into my blazer sleeve"),
  (12.0, "You think i'm resting my head on my hand"),
  (14.0, "Nah fam, I'm listening to music"),
  (16.0, "What? You didn't know what i was doing?"),
  (17.73, "I'm on the ball like Patrick Ewing"),
  (19.23, "You better not get out your chair"),
  (20.45, "Cause when you come back"),
];

/// Panneau droit au moment où la ligne `line` de `HOOD_DREAM` devient active, à `t0 + start_ms`. L'état d'avant est celui d'une lecture
/// qui a déjà centré la ligne précédente ; le vrai chemin (`animation_tick`) lance ensuite le défilement et la transition d'opacité.
fn lyrics_jump(app: &mut App, t0: Instant, line: usize, start_ms: u64) {
  go(app, Screen::Home);
  let lrc = HOOD_DREAM
    .iter()
    .map(|(seconds, text)| {
      format!(
        "[{:02}:{:05.2}]{text}",
        (seconds / 60.0) as u32,
        seconds % 60.0
      )
    })
    .collect::<Vec<_>>()
    .join(
      "
",
    );
  if let Some(track) = app.player.current.as_mut() {
    track.title = Some("the hood dream".to_string());
    track.artist = Some("808mystic".to_string());
  }
  let token = app.lyrics.token;
  app.update(Message::Ui(UiMessage::Reply(
    ReplyTag::Lyrics { token },
    Ok(serde_json::json!({ "success": true, "lyrics": lrc })),
  )));
  app.lyrics.panel_open = true;
  let start = t0 + Duration::from_millis(start_ms);
  let freq = [0; riff_app::visuals::viz::FREQ_BINS];
  app.playback.is_playing = false;
  app.playback.position = Duration::from_secs_f64(HOOD_DREAM[line - 1].0 + 0.3);
  app.now = start - Duration::from_secs(3);
  app.animation_tick(app.now, &freq);
  app.now = start - Duration::from_secs(1);
  app.animation_tick(app.now, &freq);
  app.playback.is_playing = true;
  app.playback.position = Duration::from_secs_f64(HOOD_DREAM[line].0);
  app.now = start;
  app.animation_tick(start, &freq);
}

/// Image d'une scène de paroles à `app.now` : la trame `animation_tick` de l'application, puis le défilement qu'elle demande
/// (`Effect::LyricsScroll`, appliqué dans l'application par `scroll_to`) rejoué par la molette sur le panneau, seul geste que le
/// simulateur transmet au `scrollable`.
fn render_lyrics_frame(app: &mut App, name: &str, size: Size) {
  app.animation_tick(app.now, &[0; riff_app::visuals::viz::FREQ_BINS]);
  let offset = app.lyrics.offset;
  let scale = size.width / 1920.0;
  render_with(app, name, size, move |sim| {
    sim.point_at(Point::new(1650.0 * scale, 700.0 * scale));
    sim.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
      delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -offset },
    })]);
  });
}

/// Scène d'animation : `setup` prépare un `App` neuf à l'instant du déclencheur `t0` ; le banc règle ensuite `app.now` sur
/// `t0 + t` pour chaque `t` de `times_ms` et rend `<name>-tNNN.png`. `reference` : dossier de `ui-reference/motion` à comparer.
struct MotionScene {
  name: &'static str,
  reference: Option<&'static str>,
  times_ms: &'static [u64],
  setup: fn(&mut App, Instant),
}

fn motion_scenes() -> Vec<MotionScene> {
  vec![
    // Ancre 41 : splash de démarrage (scènes 41*, captures motion 01-splash-demarrage, 01b-splash-rejoue).
    // 41a : l'animation part à +60 ms du déclencheur de `01b` (latence d'injection de 55 à 90 ms : à 62 ms la capture en est à ses premiers
    // pour cent de tracé, au fondu elle retarde de 70 à 100 ms ; `samples.json` : départ de page ~220 ms pour un déclencheur à 191,5 ms).
    MotionScene {
      name: "41a-splash-rejoue",
      reference: Some("01b-splash-rejoue"),
      times_ms: &[
        62, 110, 160, 215, 267, 317, 421, 535, 637, 736, 840, 941, 997, 1070, 1165, 1261, 1356,
        1448, 1898,
      ],
      setup: |app, t0| splash_at(app, t0, 60),
    },
    // 41b : rechargement réel. Le déclencheur (première image où le logo porte une transformation) suit le vrai départ d'une image au plus,
    // mais les captures d'après rechargement sont lentes (f000 dure 781 ms) : le fondu de `f001` (1106 ms) n'est qu'entamé, d'où +100 ms.
    MotionScene {
      name: "41b-splash-demarrage",
      reference: Some("01-splash-demarrage"),
      times_ms: &[623, 1106, 1252, 1359, 1464],
      setup: |app, t0| splash_at(app, t0, 100),
    },
    // 41c : accent par défaut d'un Riff vierge (`app.js:183`), sans capture.
    MotionScene {
      name: "41c-splash-defaut",
      reference: None,
      times_ms: &[650],
      setup: |app, t0| {
        splash_at(app, t0, 0);
        app.settings.theme_accent = "#5247d1".to_string();
      },
    },
    // Ancre 43 : toast gouttelette et son spinner (scènes 43*, captures motion 03-toast, 03b-toast-spinner).
    // Le temps des scènes est celui des captures (0 = déclencheur). Décalage d'injection retenu : l'animation CSS part vers
    // 100 ms (opacité 0,70 à 133,5 ms = courbe à p = 0,15, `samples.json` de 03-toast), vers 60 ms pour 03b-toast-spinner
    // (première rotation mesurée à 84 ms pour 0,11 rad). `.leaving` de 03-toast : vers 1630 ms (opacité 0,505 à 1758,7 ms).
    MotionScene {
      name: "43a-toast-entree",
      reference: Some("03-toast"),
      times_ms: &[0, 30, 117, 201, 289, 372, 449],
      setup: |app, t0| {
        go(app, Screen::Home);
        nothing_playing(app);
        show_toast(app, ToastKind::Info, "Message de test", t0 + TOAST_LAG);
      },
    },
    MotionScene {
      name: "43b-toast-sortie",
      reference: Some("03-toast"),
      times_ms: &[1580, 1662, 1742, 1817, 1895, 1967],
      setup: |app, t0| {
        go(app, Screen::Home);
        nothing_playing(app);
        show_toast(app, ToastKind::Info, "Message de test", t0 + TOAST_LAG);
        if let Some(toast) = &mut app.toasts.active {
          toast.leaving_at = Some(t0 + TOAST_LEAVING);
        }
      },
    },
    MotionScene {
      name: "43c-toast-spinner",
      reference: Some("03b-toast-spinner"),
      times_ms: &[
        14, 129, 245, 333, 579, 751, 1079, 1435, 1514, 1831, 2032, 2860,
      ],
      setup: |app, t0| {
        go(app, Screen::Home);
        nothing_playing(app);
        show_toast(app, ToastKind::Spinner, "", t0 + TOAST_SPINNER_LAG);
      },
    },
    // Ancre 44 : modale des réglages, ouverture (scènes 44*, capture motion 04-reglages).
    // 04-reglages : `samples.json` donne le départ de l'ouverture à 255,5 ms de l'horloge de la page (déclencheur à 162,7 ms : +92,8 ms)
    // et celui de la fermeture à 1659,7 ms (+1497 ms), moindres carrés sur les trois transitions (`styles.css:7377-7405`, écart
    // 1e-6). Les images de la capture n'aident pas à recaler : l'image à 152 ms montre encore la page seule (l'ouverture part à
    // 93 ms) et celle à 1666 ms la page seule aussi (la fermeture finit à 1697 ms), soit des retards de signes opposés. Les départs
    // sont donc ceux de `samples.json` sans retard, et les planches ne comparent de façon sûre que l'état ouvert (748 ms) et
    // l'état fermé (1895 ms). La page derrière est Titres aimés avec un titre en lecture et le panneau de paroles, catégorie Thème, comme la capture (sans sa recherche).
    MotionScene {
      name: "44a-reglages-ouverture",
      reference: Some("04-reglages"),
      times_ms: &[93, 125, 152, 190, 240, 300, 748],
      setup: |app, t0| {
        settings_behind(app);
        go(app, Screen::Settings);
        app.update(Message::Settings(SettingsMessage::OpenCategory(
          Category::Theme,
        )));
        app.overlay_motion = motion::OverlayMotion::launched();
        app.overlay_motion.navigate(
          Screen::Liked,
          Screen::Settings,
          t0 + Duration::from_millis(SETTINGS_OPEN_MS),
        );
      },
    },
    MotionScene {
      name: "44b-reglages-fermeture",
      reference: Some("04-reglages"),
      times_ms: &[1497, 1540, 1580, 1620, 1666, 1700, 1895],
      setup: |app, t0| {
        settings_behind(app);
        app.update(Message::Settings(SettingsMessage::OpenCategory(
          Category::Theme,
        )));
        app.overlay_motion = motion::OverlayMotion::launched();
        app
          .overlay_motion
          .navigate(Screen::Liked, Screen::Settings, t0 - Duration::from_secs(1));
        app.overlay_motion.navigate(
          Screen::Settings,
          Screen::Liked,
          t0 + Duration::from_millis(SETTINGS_CLOSE_MS),
        );
      },
    },
    // Ancre 45 : menu contextuel, ouverture (scènes 45*, capture motion 05-menu-contextuel).
    // 05-menu-contextuel : `opacity 180ms ease`, départ à 277,7 ms de l'horloge de la page (déclencheur à 169,9 ms : +107,8 ms) et
    // fermeture à 1402,8 ms (+1232,9 ms), moindres carrés sur `samples.json` (écart 1e-4). Retard des images : opacité d'un pixel du
    // menu sur l'image à 129,5 ms (gris 30 entre 15 et 33, soit 0,83) et sur celle à 1199,5 ms (gris 21, soit 0,33 à la fermeture) :
    // 74 ms puis 103 ms (quantification : ±10 ms). Départs posés : 107,8 - 74 = 34 ms et 1232,9 - 103 = 1130 ms. Un seul instant de
    // chaque sens tombe au milieu du fondu : les autres instants n'ont pas d'image de référence à leur date.
    MotionScene {
      name: "45a-menu-contextuel-ouverture",
      reference: Some("05-menu-contextuel"),
      times_ms: &[19, 60, 80, 100, 129, 160, 229, 334],
      setup: |app, t0| {
        context_menu_capture(app);
        app.overlay_motion = motion::OverlayMotion::launched();
        app
          .overlay_motion
          .open_menu(t0 + Duration::from_millis(MENU_OPEN_MS));
      },
    },
    MotionScene {
      name: "45b-menu-contextuel-fermeture",
      reference: Some("05-menu-contextuel"),
      times_ms: &[1100, 1150, 1199, 1230, 1260, 1328, 1433],
      setup: |app, t0| {
        context_menu_capture(app);
        app.menu_leaving = app.menu.take();
        app.overlay_motion = motion::OverlayMotion::launched();
        app.overlay_motion.open_menu(t0 - Duration::from_secs(1));
        app
          .overlay_motion
          .close_menu(t0 + Duration::from_millis(MENU_CLOSE_MS));
      },
    },
    // Ancre 46 : panneau de file d'attente, ouverture (scènes 46*, capture motion 06-file-attente).
    // 06-file-attente : `.open` posé à +111,8 ms de l'horloge du déclencheur (`samples.json` : `opacity` quitte 0 à 278 pour un `mark`
    // à 166,2 ; ajustement sur 220 ms `ease` et 260 ms `(0.2,0,0,1)`). Les images portent un retard de 29 ms (bord gauche du panneau sur `f002` à 180 ms : 3 px d'écart au repos, soit s = 0,74 donc 90 ms de
    // transition ; rendu à 82 ms de départ) : départ posé à 111,8 - 29,8 = 82 ms.
    MotionScene {
      name: "46a-file-attente",
      reference: Some("06-file-attente"),
      times_ms: &[32, 180, 306, 411, 511],
      setup: |app, t0| {
        queue_panel_capture(app);
        // La capture de Riff garde le panneau de paroles ouvert derrière (visible à travers le fondu).
        app.lyrics.panel_open = true;
        app
          .ui
          .queue_panel
          .motion
          .set_open(false, true, t0 + Duration::from_millis(PANEL_START_MS));
      },
    },
    // Fermeture sans capture de Riff : mêmes durées et courbes dans le sens inverse (`styles.css:10927`, `10934`), départ à t0.
    MotionScene {
      name: "46b-file-attente-fermeture-defaut",
      reference: None,
      times_ms: &[0, 50, 100, 150, 220, 255, 270],
      setup: |app, t0| {
        queue_panel_capture(app);
        app.ui.queue_panel.open = false;
        app.ui.queue_panel.motion.set_open(true, false, t0);
      },
    },
    // Ancre 47 : transition de vue (scènes 47*, capture motion 07-changement-vue).
    // 07-changement-vue : Accueil vers Titres aimés. Départ de `m3ContainerOpen` à +102,5 ms sur l'horloge de `samples.json` (première
    // ligne où `opacity` quitte 0, ajustement par moindres carrés sur 400 ms `(0.2,0,0,1)`, écart 1e-4). Les images portent un retard
    // de 35 ms (opacité d'un pixel de la liste, moindres carrés sur les 6 premières images) : départ à 102,5 - 35 = 67 ms. Avant le
    // départ Riff montre encore l'Accueil (retiré à +98 ms) : le banc n'a que la vue entrante, vide à t < 67 ms (l'image à 49 ms de
    // Riff est vide aussi : le retard la place avant le fondu). Le départ est posé ici : la navigation du banc reste au repos.
    MotionScene {
      name: "47a-changement-vue",
      reference: Some("07-changement-vue"),
      times_ms: &[49, 185, 321, 446, 585, 718, 1027],
      setup: |app, t0| {
        go(app, Screen::Home);
        app.lyrics.panel_open = true;
        app.view_motion.enabled = true;
        go(app, Screen::Liked);
        app.view_motion.start = Some(t0 + Duration::from_millis(VIEW_START_MS));
      },
    },
    // 17-recherche-loader : le loader mesure 96 px à 80 ms puis 92,16 px à 113 ms, qui regrandit jusqu'à 96 : c'est `scale(0.96)` de
    // `m3ContainerOpen` (la vue Accueil rejouée par la recherche), départ à +113 ms. Même scène que 57a, avec la transition de vue.
    MotionScene {
      name: "47b-vue-recherche-loader",
      reference: Some("17-recherche-loader"),
      times_ms: &[134, 250, 350, 450, 500, 1300],
      setup: |app, t0| {
        app.view_motion.enabled = true;
        search_results(app, SearchFilter::All);
        app.ui.search.tracks.clear();
        app.ui.search.loading = true;
        app.shape_loop.show(t0 + Duration::from_millis(80));
        app.epoch = t0 + Duration::from_millis(80) - Duration::from_millis(2608);
        app.spinner_seed = seed_for_shapes(&[COOKIE12, HEXAGON]);
        app.view_motion.start = Some(t0 + Duration::from_millis(113));
      },
    },
    // Ancre 48 : survols de carte et de bouton (scènes 48*, captures motion 08a-survol-carte, 08b-survol-bouton).
    // 08a-survol-carte : cartes de « Tendance maintenant » (`.sp-card-cover`). `transform 180ms ease` : entrée à +59,5 ms, sortie à
    // +976,25 ms (moindres carrés sur `samples.json`, écart 1e-3 px), valeurs -2 px puis 0. Retard des images supposé égal à celui de
    // 08b (42 ms : les pochettes ne bougent pas dans les PNG de Riff, 2 px à 0,945 tombent sous le pixel, rien à ajuster dessus).
    // Départs posés ici : entrée à 59,5 - 42 = 17 ms, sortie à 976 - 42 = 934 ms.
    MotionScene {
      name: "48a-survol-carte",
      reference: Some("08a-survol-carte"),
      times_ms: &[58, 134, 214, 296, 962, 1036, 1110, 1185],
      setup: |app, t0| {
        go(app, Screen::Home);
        app.lyrics.panel_open = true;
        let key = (HomeRow::Trending, 0);
        app.ui.home.hover = Some(key);
        app
          .ui
          .home
          .lift
          .set(key, 1.0, t0 + Duration::from_millis(17), motion::CARD);
        app
          .ui
          .home
          .lift
          .set(key, 0.0, t0 + Duration::from_millis(934), motion::CARD);
      },
    },
    // 08b-survol-bouton : `.btn-icon-pill` de la file d'attente. `--hl` et fond en `.18s ease`, `border-radius` en `.22s (.2,0,0,1)`
    // (`styles.css:7509`, plus long que les 70/100 ms de `styles.css:131`) : entrée à +63,25 ms, sortie à +788,5 ms (moindres carrés,
    // écart 0,008 % sur le rayon). Retard des images : 42 ms (niveau de gris d'un pixel du disque : 28 -> 43 à l'entrée, 57 -> 28 à la
    // sortie, moindres carrés). Départs posés ici : 63 - 42 = 21 ms et 789 - 42 = 747 ms.
    MotionScene {
      name: "48b-survol-bouton",
      reference: Some("08b-survol-bouton"),
      times_ms: &[
        15, 60, 104, 144, 199, 305, 756, 794, 833, 876, 921, 1003, 1055,
      ],
      setup: |app, t0| {
        go(app, Screen::Home);
        app.lyrics.panel_open = true;
        app
          .ui
          .controls
          .set(icons::QUEUE_LIST, true, t0 + Duration::from_millis(21));
        app
          .ui
          .controls
          .set(icons::QUEUE_LIST, false, t0 + Duration::from_millis(747));
      },
    },
    // Ancre 49 : like du bouton favori de la barre de lecture, bascule instantanée du remplissage (scènes 49*, capture motion 09-like).
    // 09-like : titre aimé au départ, clic sur `#btn-toggle-fav` (`element.click()`, ni survol ni :active), second clic 800 ms plus tard.
    // `samples.json` (horloge de la page, déclencheur à 170,9) : `svg.fill` passe de `currentColor` à `none` à 224,4 (+53,5 ms, image rAF
    // suivante) puis revient à 1053,6 (+882,7 ms) ; rien d'autre ne change (fond, rayon 50 %, `--hl` 0 %, transform identité, 35,9954 px).
    // Recalage : le contenu d'une image date de son milieu plus la latence d'injection. Les images de `-reference.txt` donnent la 1re
    // bascule entre f001 (-67,5, pleine) et f002 (13,0, vide), la 2e entre f017 (811,5, vide) et f018 (883,5, pleine) : seule une latence
    // de 40,5 à 71,2 ms satisfait les deux. Retenu 45 ms, soit des bascules à 53,5 - 45 = 8,5 ms et 882,7 - 45 = 837,7 ms sur l'axe des
    // images. À t = 0 (instant minimal du banc) le titre est encore aimé mais f002, la plus proche, est déjà vide (f001 est à -67,5).
    // 49a : barre de lecture sur l'accueil, le titre courant est dans les favoris.
    MotionScene {
      name: "49a-like-barre",
      reference: Some("09-like"),
      times_ms: &[0, 13, 89, 253, 436, 811, 884, 1043, 1661],
      setup: |app, _| {
        go(app, Screen::Home);
        toggle_like_at(app, MOTION_AT.with(std::cell::Cell::get), &[8.5, 837.7]);
      },
    },
    // 49b (sans capture) : même clic sur l'écran Titres aimés. `toggleFavoriteTrack` appelle `renderFavoritesView` (`app.js:12274`) :
    // toutes les lignes sont recréées et rejouent `m3FadeUp` (350 ms, 30 ms par rang) depuis le clic. Délike à t = 0, relike à 1000 ms
    // (la liste retrouve le titre en tête, `unshift`). `entry_motion` est désactivé dans le banc : le départ du fondu est posé ici.
    MotionScene {
      name: "49b-like-titres-aimes-defaut",
      reference: None,
      times_ms: &[0, 45, 175, 560, 999, 1000, 1045, 1175, 1700],
      setup: |app, t0| {
        liked_capture(app);
        app.player.current = Some(liked_tracks()[2].clone());
        let at = MOTION_AT.with(std::cell::Cell::get);
        let flips = toggle_like_at(app, at, &[0.0, 1000.0]);
        let start = [0u64, 1000][flips.saturating_sub(1)];
        if flips > 0 {
          app.entry_motion.rows = Some(t0 + Duration::from_millis(start));
        }
      },
    },
    // Ancre 50 : bouton lecture/pause et morphing de l'icône (scènes 50*, capture motion 10-lecture-pause).
    // 10-lecture-pause : clic pause à t = 0, second clic 1200 ms plus tard. `samples.json` (horloge de la page) : la transition CSS part
    // 55 ms après le déclencheur et la seconde à 1292,5 ms (ajustement de la largeur et de `d` : écart cumulé de 0,3 px sur 5 points
    // pour 55 ms, 5,5 px pour 50). Le contenu d'une image date du milieu de son intervalle d'envoi plus ~72 ms (f001, milieu -19 ms, est
    // encore à 70 px ; f002, milieu 60 ms, est à 46,6 px, ce qui demande 76 ms de transition ; f025, milieu 1280 ms, est à 67,7 px
    // soit 58 ms de transition). Sur l'axe des images, les transitions partent donc à -17 ms et à 1220,5 ms. 50a : première
    // transition, lecture vers pause ; 50b : la seconde, pause vers lecture, après 1237,5 ms de pause.
    MotionScene {
      name: "50a-lecture-pause-vers-pause",
      reference: Some("10-lecture-pause"),
      times_ms: &[60, 128, 184, 252, 319, 376, 446, 553, 800],
      setup: |app, t0| {
        go(app, Screen::Home);
        app.playback.is_playing = false;
        let button = &mut app.playbar_motion.button;
        button.track(true, t0 - Duration::from_secs(10));
        button.track(false, t0 - PLAY_CLICK_EARLY);
        // `base_app` date l'onde de l'instant réel : sans cette phase figée, sa phase aux premiers instants en dépend.
        app.playbar_motion.wave =
          Wave::frozen(Wave::default().phase(t0, app.playback.position.as_secs_f32()));
      },
    },
    MotionScene {
      name: "50b-lecture-pause-vers-lecture",
      reference: Some("10-lecture-pause"),
      times_ms: &[1333, 1393, 1454, 1519, 1595, 1736, 2000],
      setup: |app, t0| {
        go(app, Screen::Home);
        let button = &mut app.playbar_motion.button;
        button.track(true, t0 - Duration::from_secs(10));
        button.track(false, t0 - PLAY_CLICK_EARLY);
        button.track(true, t0 + PLAY_SECOND_CLICK);
        app.playbar_motion.wave =
          Wave::frozen(Wave::default().phase(t0, app.playback.position.as_secs_f32()));
      },
    },
    // Ancre 51 : barre de progression ondulée (scènes 51*, capture motion 11-barre-ondulee).
    // 11-barre-ondulee : lecture, clic pause à 1200 ms, clic lecture 1400 ms plus tard (la capture montre bien la pause, que MOTION.md
    // disait non capturée). Phase de l'onde ajustée sur chaque image (moindres carrés de `y = c + a sin(k x) + b cos(k x)` sur le
    // centre du trait) puis recalée en comparant la même mesure sur le rendu de riff-rs (écart constant de 0,10 rad sur 29 images, ce
    // qui absorbe l'incertitude sur l'origine du canvas) : 2,08 + 3,5 t rad en lecture (3,5 rad/s tenus à 0,03 rad près), figée à
    // 0 de 1200 à ~2565 ms, puis elle repart de 0. Durée du titre (390 s) déduite de l'avance de la fin de l'onde (4 px en 3,5 s), position
    // de chaque scène recalée sur la fin de l'onde mesurée (écart de 40 px au premier essai).
    // 51a : lecture avant la pause ; 51b : pause ; 51c : reprise.
    MotionScene {
      name: "51a-barre-ondulee",
      reference: Some("11-barre-ondulee"),
      times_ms: &[105, 226, 335, 448, 568, 679, 793, 911, 1019, 1132],
      setup: |app, t0| {
        go(app, Screen::Home);
        scrubber_at(app, 203.0);
        app.playbar_motion.wave = Wave::running(WAVE_PHASE_AT_TRIGGER, t0);
      },
    },
    MotionScene {
      name: "51b-barre-ondulee-pause",
      reference: Some("11-barre-ondulee"),
      times_ms: &[
        1243, 1346, 1457, 1564, 1670, 1781, 1891, 1994, 2102, 2211, 2326, 2444, 2555,
      ],
      setup: |app, _| {
        go(app, Screen::Home);
        scrubber_at(app, 205.2);
        app.playback.is_playing = false;
        app.playbar_motion.wave = Wave::frozen(WAVE_PHASE_PAUSED);
      },
    },
    MotionScene {
      name: "51c-barre-ondulee-reprise",
      reference: Some("11-barre-ondulee"),
      times_ms: &[2672, 2796, 2918, 3028, 3139, 3248, 3357],
      setup: |app, t0| {
        go(app, Screen::Home);
        scrubber_at(app, 205.4);
        app.playbar_motion.wave = Wave::running(WAVE_PHASE_PAUSED, t0 + WAVE_RESUME);
      },
    },
    // Ancre 52 : karaoké plein écran, entrée et sortie, tvk-pop, orbes (scènes 52*, capture motion 12-karaoke).
    // 12-karaoke (`samples.json`, déclencheur à 161,7 ms de l'horloge de la page) : `display` passe à `flex` à 253,7 ms mais `tvk-in` ne
    // part qu'à 316,2 ms (image longue de 62 ms, comme dans 13-theme-titre) : ajustement de la courbe `(0.2, 0, 0, 1)` sur l'opacité de 316
    // à 716 ms, erreur quadratique 9e-8. Soit 154 ms après le déclencheur du banc (`TVK_ENTER_MS`). Décalage d'injection des images de
    // référence : les images de Riff ont 32 à 48 ms d'avance sur leur étiquette de `-reference.txt`. Mesuré par l'opacité de groupe de
    // l'image (écart d'alpha médian contre f005 et f001, relié à `samples.json`) : f002, étiquetée 255,5 ms, est la page à 466 ms
    // (opacité 0,775), soit 303 ms du banc ; f003, étiquetée 427 ms, a l'opacité 0,978 de la page à 620 ms, soit 459 ms ; f004 (opacité
    // 1) est établie, son instant exact n'est pas mesurable (au moins 555 ms). Les orbes partent avec l'animation d'entrée. Instants :
    // avant le départ (écran absent), début, f002 recalée, +74, f003 recalée, f004, état établi.
    // Lecture de la planche `-cote` : elle montre l'image de Riff choisie sur l'étiquette brute de `-reference.txt` (`nearest_frame`), et le tableau
    // ci-dessous donne l'instant réel de chaque image (décalé de 32 à 48 ms). t082 : f001 ; t190 : f002 en partie (opacité entre f001 et f002) ; t303 : f002 ;
    // t377 et t459 : f003 ; t581 : f004 ; t868 : f006.
    MotionScene {
      name: "52a-karaoke-entree",
      reference: Some("12-karaoke"),
      times_ms: &[82, 190, 303, 377, 459, 581, 868],
      setup: |app, t0| karaoke_opened(app, t0 + Duration::from_millis(TVK_ENTER_MS)),
    },
    // `tvk-out` : même ajustement sur `1 - opacité` de 2570 à 2870 ms, départ à 2587,3 ms de la page (2426 ms après le déclencheur,
    // `TVK_EXIT_MS`), 300 ms `(0.4, 0, 1, 1)` ; `.hidden` vient à 2874,8 ms (320 ms après le clic à 2554,8 ms, `app.js:7109`), soit
    // 2713 ms du banc : à 2760 ms l'écran est retiré des deux côtés. Riff retire l'écran à une opacité d'environ 0,07 (l'animation n'est pas
    // finie) ; riff-rs date la fermeture au départ de l'animation, donc l'animation finit à 2726 ms (opacité 0) et le retrait arrive à
    // 2746 ms, 33 ms après Riff : dans la capture, `tvk-out` ne part que 32,5 ms après le clic (image longue), riff-rs les confond.
    // Dans l'application (déduit, non mesuré), le décalage est celui d'une image. Aucun écart visible : l'écran est invisible de 2726 à 2746 ms.
    // Décalage d'injection : les images de Riff ont 37 à 40 ms d'avance sur leur étiquette. f017, étiquetée 2442,5 ms, a l'opacité 0,942
    // de la page à 2641 ms (soit 2481 ms du banc, 0,945 sur la courbe) ; f018, étiquetée 2606 ms, a l'opacité 0,394 (2645 ms du banc) ;
    // f019 (opacité 0) est retirée, au moins 2713 ms. Les orbes sont figés à la fermeture (`.tvk-bg-paused`) et les paroles déjà rendues
    // à leur panneau (`closeTvKaraoke` les déplace tout de suite).
    // Lecture de la planche `-cote` : t2400 à t2481 : f017 ; t2560 et t2645 : f018 ; t2670 à t2760 : f019, déjà retirée (écran absent de
    // Riff). La planche montre l'image choisie sur l'étiquette brute de `-reference.txt` ; l'instant réel de chaque image est décalé de 32 à 48 ms.
    MotionScene {
      name: "52b-karaoke-sortie",
      reference: Some("12-karaoke"),
      times_ms: &[2400, 2440, 2481, 2560, 2645, 2670, 2700, 2760],
      setup: |app, t0| {
        karaoke_closed(
          app,
          t0 + Duration::from_millis(TVK_ENTER_MS),
          t0 + Duration::from_millis(TVK_EXIT_MS),
        );
      },
    },
    // Orbes `tvkOrbit1-4` pendant les 2,2 premières secondes de la capture : l'horloge des orbes part avec `tvk-in`. Les couleurs viennent
    // de la pochette du banc (`flag_cover`, approximation de celle de la capture) : on compare les positions et les formes des taches,
    // pas la teinte exacte.
    MotionScene {
      name: "52c-karaoke-orbes",
      reference: Some("12-karaoke"),
      times_ms: &[330, 600, 900, 1200, 1500, 1800, 2100, 2390],
      setup: |app, t0| karaoke_opened(app, t0 + Duration::from_millis(TVK_ENTER_MS)),
    },
    // Animation `pop` (`tvk-pop`, `styles.css:9110-9114`) : la courbe `(.2,0,0,1)` s'applique à chaque segment. Ce n'est pas le défaut d'un
    // Riff vierge (`slide`, 900 ms, `app.js:7331-7332`) : sans capture de référence. Instants à 0, 11, 25, 50, 75, 89 et 100 % de 900 ms.
    MotionScene {
      name: "52d-karaoke-pop-defaut",
      reference: None,
      times_ms: &[0, 100, 225, 450, 675, 800, 900],
      setup: |app, t0| karaoke_pop(app, t0, 0),
    },
    // Orbes sur plusieurs cycles, sans référence : l'horloge vaut t + 0,4 s (l'écran s'est ouvert 400 ms avant le déclencheur). `.b2`
    // fait 4 s (retour au départ à 3,6 s), `.b3` 6 s, `.b1` 17 s, `.b4` 22 s ; `alternate` ramène chaque orbe sur ses pas au cycle suivant.
    MotionScene {
      name: "52e-karaoke-orbes-cycles",
      reference: None,
      times_ms: &[0, 1600, 3600, 5600, 7600, 9600, 11600, 17000],
      setup: |app, t0| karaoke_opened(app, t0 - Duration::from_millis(400)),
    },
    // Ancre 53 : couleur dynamique de la pochette (scènes 53*, capture motion 13-theme-titre).
    // 13-theme-titre (`samples.json`, déclencheur à 156,9 ms) : les variables `--md-sys-color-*` changent d'un coup à 237,7 ms (cover chargée
    // et analysée, ~81 ms), mais la transition du bouton lecture (150 ms `ease`) et celle du fond de `body` (400 ms, fin à 708,6 ms) ne
    // partent qu'à ~305 ms à cause d'une image longue de 62 ms entre 237,7 et 300,2 ms. riff-rs lance la transition à l'arrivée de
    // `CoverColor` sans attente simulée : le message est donc daté `THEME_START` = 150 ms après le déclencheur (305 - 156,9 = 148 ms,
    // ajustement des deux courbes sur `samples.json`). Décalage d'injection des images de référence : aucun, leurs instants sont
    // ceux de `<scène>-reference.txt`. Sources de couleur : `(0, 128, 152)` à intensité 25 donne le primary de départ rgb(182, 198, 201),
    // `(224, 16, 0)` à intensité 56 celui d'arrivée rgb(213, 172, 170) (mesuré : rgb(212, 172, 170)). Riff dérive l'intensité de la
    // saturation de la pochette (`app.js:1849`), riff-rs lit le réglage : d'où deux intensités. Le fond de `body` ne change pas dans
    // riff-rs (gris du thème sombre, `scheme.surface`) alors que Riff passe de #0f0f0f à #0d0d0d : seule la transition du bouton se voit.
    MotionScene {
      name: "53a-theme-pochette",
      reference: Some("13-theme-titre"),
      times_ms: &[0, 168, 195, 230, 270, 330, 421, 560, 659],
      setup: |app, t0| cover_theme_change(app, t0, THEME_START, false),
    },
    // Même transition avec `intensity_bg` : le fond suit la pochette et fond en 400 ms (`app.js:1167`). Sans capture de référence.
    MotionScene {
      name: "53b-theme-pochette-fond",
      reference: None,
      times_ms: &[0, 60, 150, 250, 400],
      setup: |app, t0| cover_theme_change(app, t0, 0, true),
    },
    // Thème par défaut d'un Riff vierge : accent `#5247d1` (`app.js:817`), mode `dark` (`app.js:814`), au repos. Sans capture.
    MotionScene {
      name: "53c-theme-defaut",
      reference: None,
      times_ms: &[0],
      setup: |app, _| go(app, Screen::Liked),
    },
    // Ancre 54 : passage en mode mini, fenêtre et mise en page compacte en une image (scènes 54*, capture motion 14-mini).
    // Taille de la fenêtre par instant : `motion_window`. Aucune transition CSS (`MOTION.md:77`) : `samples.json` donne body 2031,75x1142,86 à
    // 54,2 ms puis 402,12x169,31 à 58,4 ms (déclencheur à 2,3 ms, soit 51,9 et 56,1 ms après lui) ; le passage est posé à 54 ms. Aucune
    // latence d'injection à retrancher : `samples.json` et `mark` sont sur la même horloge de page. Les images de référence (f000 à 63 ms)
    // sont toutes après le passage : avant 54 ms il n'y a rien à comparer, la planche montre alors l'image la plus proche.
    // Les instants de référence : 63 (f000), 146 (f001), 700 (f013), 2000 (f038).
    MotionScene {
      name: "54a-mini-entree",
      reference: Some("14-mini"),
      times_ms: &[0, 30, 50, 54, 63, 146, 700, 2000],
      setup: |app, _t0| {
        app.update(Message::ToggleMiniMode);
      },
    },
    // 54b : sortie, sans capture (le 2e clic de 14-mini n'a pas restauré la fenêtre). `LeaveMiniWindow` rend la fenêtre à 1920x1080 ;
    // tant que la nouvelle taille n'est pas reçue (avant 50 ms), la largeur de 380 px garde la mise en page compacte (`checkCompactLayout`).
    MotionScene {
      name: "54b-mini-sortie-defaut",
      reference: None,
      times_ms: &[0, 40, 50, 100],
      setup: |app, _t0| {
        app.update(Message::ToggleMiniMode);
        app.update(Message::ToggleMiniMode);
      },
    },
    // 54c : fenêtre compacte élargie, sans capture. 600 px > 491,4 px logiques (520 px CSS) : mise en page complète ; 480 px : compacte.
    MotionScene {
      name: "54c-mini-elargie-defaut",
      reference: None,
      times_ms: &[0, 100],
      setup: |app, _t0| {
        app.update(Message::ToggleMiniMode);
      },
    },
    // Ancre 55 : défilement des paroles et ligne active (scènes 55*, capture motion 15-paroles).
    // 15-paroles : déclencheur à 165,8 ms sur l'horloge de la page. Le départ des transitions est celui que donne l'ajustement des
    // courbes de `samples.json` (opacité : 480 ms `inout`, défilement : 340 ms pour 90 px, soit 345 ms attendus) : 1504 ms page pour
    // le premier saut (le premier échantillon à 1479,2 ms montre déjà la nouvelle ligne à 0,48 avant que la transition ne parte), 3212,7 ms
    // pour le deuxième ; moins 165,8 ms, 1338 et 3047 ms après le déclencheur. Décalage de latence : aucun (l'image f009, à 1480 ms, est à
    // +62 px de défilement comme la page à 1645 ms). Instants : début, +50, +100, image de référence, +247, image, +437, image.
    MotionScene {
      name: "55a-paroles-saut-1",
      reference: Some("15-paroles"),
      times_ms: &[1338, 1388, 1438, 1480, 1585, 1657, 1775, 1839],
      setup: |app, t0| lyrics_jump(app, t0, 3, 1338),
    },
    MotionScene {
      name: "55b-paroles-saut-2",
      reference: Some("15-paroles"),
      times_ms: &[3047, 3097, 3147, 3224, 3300, 3398, 3490, 3583],
      setup: |app, t0| lyrics_jump(app, t0, 4, 3047),
    },
    // Ancre 56 : spinner de lecture (scènes 56*, capture motion 16-spinner-lecture).
    // 16-spinner-lecture : `.buffering` forcé à t = 0, donc le spinner part à 14 ms (premier échantillon de `path.d`, icône à 0,99
    // d'opacité) ; l'anti-rebond de 500 ms est avant. Formes de la capture (comparées aux profils par `samples.json`) :
    // hexagone, arche, fantôme, d'où la graine cherchée. Rotation : au premier échantillon le 1er point est à 2,727 rad, soit
    // 1302 ms sur l'horloge de la page (`now % 3000`).
    MotionScene {
      name: "56a-spinner-lecture",
      reference: Some("16-spinner-lecture"),
      times_ms: &[
        14, 100, 306, 527, 722, 952, 1148, 1410, 1735, 2018, 2373, 2856,
      ],
      setup: |app, t0| {
        go(app, Screen::Home);
        waiting_play(app);
        app.play_wait.since = Some(t0 + Duration::from_millis(14) - Duration::from_millis(500));
        app.shape_loop.show(t0 + Duration::from_millis(14));
        app.epoch = t0 + Duration::from_millis(14) - Duration::from_millis(1302);
        app.spinner_seed = seed_for_shapes(&[HEXAGON, ARCH, GHOST]);
      },
    },
    // Anti-rebond réel de `updatePlayPauseButton` : l'attente part à t = 0, le spinner à 500 ms. Sans capture de référence.
    MotionScene {
      name: "56b-spinner-lecture-antirebond",
      reference: None,
      times_ms: &[0, 250, 499, 500, 541, 640, 760, 1200],
      setup: |app, t0| {
        go(app, Screen::Home);
        waiting_play(app);
        app.play_wait.since = Some(t0);
        app.shape_loop.show(t0 + Duration::from_millis(500));
        app.epoch = t0;
        app.spinner_seed = seed_for_shapes(&[HEXAGON, ARCH, GHOST]);
      },
    },
    // Ancre 57 : loader de recherche (scènes 57*, capture motion 17-recherche-loader).
    // 17-recherche-loader : le loader existe de 80 ms (premier `path.d`, boucle de formes lancée) à 500 ms, puis les résultats le
    // remplacent. Formes mesurées sur `samples.json` (erreur 0,02 sur les rayons) : dodécagone à bords ronds (`Cookie12Sided`) qui se
    // fond vers l'hexagone depuis 80 ms. Rotation : le 1er point est à 5,463 rad à 80 ms, soit 2608 ms sur l'horloge de la page.
    // Au-delà de 500 ms, pas de référence : 1300 et 2500 ms montrent le pas suivant et le tour de rotation.
    // Taille : le loader mesure 92,28 px à 134 ms puis 95,99 px : c'est `m3ContainerOpen` (`scale(0.96)` vers `none`, 400 ms, vue
    // qui devient active, `styles.css:1009`), pas l'animation du loader. Non reproduit ici (transition de vue).
    MotionScene {
      name: "57a-recherche-loader",
      reference: Some("17-recherche-loader"),
      times_ms: &[80, 150, 250, 350, 450, 500, 1300, 2500],
      setup: |app, t0| {
        search_results(app, SearchFilter::All);
        app.ui.search.tracks.clear();
        app.ui.search.loading = true;
        app.shape_loop.show(t0 + Duration::from_millis(80));
        app.epoch = t0 + Duration::from_millis(80) - Duration::from_millis(2608);
        app.spinner_seed = seed_for_shapes(&[COOKIE12, HEXAGON]);
      },
    },
    // Ancre 58 : entrées de listes et cartes m3FadeUp (scènes 58*-defaut, sans capture motion).
    // `m3FadeUp` (`styles.css:1016`) : opacité 0 et translateY(10px) vers le repos, `cubic-bezier(0.2, 0, 0, 1)`. Le départ de chaque scène est
    // posé ici (`entry_motion`, désactivé dans le banc) : l'instant t = 0 est l'insertion du nœud, `backwards`/`both` le laissent à l'image de départ.
    // 58a (`.mix-card`) et 58d (`.track-loading-overlay`) n'ont pas de scène : aucun nœud `.mix-card` n'est créé par `app.js` (la classe n'y
    // est que dans deux sélecteurs de survol et de ripple) et `#track-loading-overlay { display: none !important }` (`styles.css:9598`).
    // Carte d'artiste des résultats : 260 ms sans décalage. À 260 ms la carte est au repos (pas de pose).
    MotionScene {
      name: "58b-carte-artiste-defaut",
      reference: None,
      times_ms: &[0, 65, 130, 195, 259, 260, 600],
      setup: |app, t0| {
        search_results(app, SearchFilter::All);
        app.ui.search.artist = Some(ArtistCard {
          name: "Daft Punk".to_string(),
          avatar: Some(cover_url(70)),
          fans: 4_300_000.0,
          verified: true,
        });
        app.entry_motion.artist = Some(t0);
      },
    },
    // Lignes de titres : opacité seule (`transform: none !important`, `styles.css:1588`), 350 ms, décalage 30 ms par rang jusqu'au rang 8.
    // La 8e ligne part à 210 ms et finit à 560 ms ; à 45 ms les lignes 1 et 2 ont commencé, les suivantes attendent à l'opacité 0.
    MotionScene {
      name: "58c-lignes-defaut",
      reference: None,
      times_ms: &[0, 45, 120, 175, 350, 559, 560, 800],
      setup: |app, t0| {
        liked_capture(app);
        app.entry_motion.rows = Some(t0);
      },
    },
    // Choix de playlist : 200 ms, décalage `idx * 25 ms` (`app.js:13543`). Six playlists : le dernier choix part à 125 ms et finit à 325 ms.
    MotionScene {
      name: "58e-choix-playlist-defaut",
      reference: None,
      times_ms: &[0, 60, 125, 225, 324, 325, 600],
      setup: |app, t0| {
        liked_capture(app);
        app.data.playlists = (0..6)
          .map(|n| {
            playlist(
              &format!("pl-{n}"),
              ["Nuit", "Route", "Focus", "Sport", "Cuisine", "Calme"][n],
              (n * 20)..(n * 20 + 12 + n),
            )
          })
          .collect();
        app.ui.modal = Some(Modal::AddToPlaylist {
          tracks: tracks(0..1),
        });
        app.entry_motion.picks = Some(t0);
      },
    },
    // Ancre 59 : halo de pochette de la barre de lecture, anneau et forme morphante (scènes 59*, sans capture motion dédiée).
    // Horloge : le halo de Riff avance image par image (`m3cLoop`, `app.js:16953`). Le banc ne peut pas rendre l'instant t par une
    // formule : `setup` rejoue donc, depuis t0, des images à pas fixe de 1000/240 ms (base 240 Hz de la planification) sur
    // `app.visuals.shape`, avec la même `CoverShape::advance` que l'application. Les événements (pause, survol) sont posés aux images
    // qui suivent leur instant. Le nombre d'images est `floor(t * 240 / 1000)` (`halo_frames`) : l'état rendu est celui de la dernière
    // image avant t (4,2 ms de retard au plus). `t` est lu dans `MOTION_AT`, posé par le banc avant chaque `setup`.
    // Valeurs de Riff (recalculées en rejouant la boucle `app.js:16953-16986` à 240 Hz, voir le rapport) : rotation = 2π/14000 *
    // somme(vitesse * 4,17 ms), vitesse 0,25 puis * 0,9 d'écart par image vers 1 ; phase = 0,0075 * 4,17 ms par image en lecture.
    // 59a : lecture depuis un halo neuf (vitesse 0,25 vers 1, phase 0, forme 0 tenue 2000 ms puis fondue en 1250 ms, `shapes.js:305-306`).
    MotionScene {
      name: "59a-barre-halo-lecture-defaut",
      reference: None,
      times_ms: &[0, 250, 500, 1000, 2000, 3250],
      setup: |app, _t0| halo_replay(app, 0, true, |_| false),
    },
    // 59b : pause à t = 0 après une lecture établie (2500 ms : vitesse 1, phase avancée, forme à 40 % de sa transition). Riff :
    // `targetRate` passe à 0,25 (`app.js:16791`), la phase ne bouge plus (`app.js:16980`), la boucle s'arrête quand `rotRate` est à
    // 0,25 à 0,01 près (`app.js:16983`, environ 130 ms à 240 Hz) : la forme reste figée au dernier dessin.
    MotionScene {
      name: "59b-barre-halo-pause-defaut",
      reference: None,
      times_ms: &[0, 17, 50, 100, 200, 400, 1000],
      setup: |app, _t0| halo_replay(app, 600, false, |_| false),
    },
    // 59c : souris sur la pochette entrée à t = 0 et sortie à 300 ms, lecture établie (vitesse 1). Riff : `hoverProgress += diff * 0,2`
    // par image, arrêt sous 0,002 (`app.js:16958-16964`) ; pochette * (1 + 0,16 h), centre levé de 4 h px (`app.js:16863-16866`).
    MotionScene {
      name: "59c-barre-halo-survol-defaut",
      reference: None,
      times_ms: &[0, 8, 17, 33, 67, 150, 300, 317, 350, 450],
      setup: |app, _t0| halo_replay(app, 120, true, |frame| (1..=72).contains(&frame)),
    },
    // 59d : un changement de forme complet, graine fixe 49_734_321 (`CoverShape`) : t = 0 est l'image où `now - start >= 2000` (tirage de
    // la forme suivante, `app.js:16858`), la transition `easeInOutCubic` dure 1250 ms (`shapes.js:280`, `306`), la forme est ensuite tenue
    // jusqu'à 2000 ms (prochain tirage).
    MotionScene {
      name: "59d-barre-halo-forme-defaut",
      reference: None,
      times_ms: &[0, 300, 625, 950, 1250, 2000],
      setup: |app, _t0| halo_replay(app, 480, true, |_| false),
    },
    // 59e : fenêtre compacte en lecture depuis un halo neuf (`body.mini`, canvas de 120 px ramené à 24vmin, `styles.css:7031-7050`).
    MotionScene {
      name: "59e-halo-compact-defaut",
      reference: None,
      times_ms: &[0, 1000, 2500],
      setup: |app, _t0| {
        app.window.enter_mini();
        resize(app, riff_app::window::MINI_SIZE);
        halo_replay(app, 0, true, |_| false);
      },
    },
    // 59f : survol plein avec une pointe de la forme vers le haut (479 images de lecture : rotation 0,88 rad, la pointe de la forme 0 à
    // -144 degrés passe à -93 degrés). `.playbar` et `.playbar-left` ont `overflow: visible` (`styles.css:2862`, `5387`) : l'anneau agrandi
    // (x 1,16) et levé de 4 px dépasse le bord haut de la barre (y 989,3), seuls l'image et son masque sont coupés à la barre.
    MotionScene {
      name: "59f-barre-halo-survol-pointe-defaut",
      reference: None,
      times_ms: &[0, 150, 300],
      setup: |app, _t0| halo_replay(app, 479, true, |_| true),
    },
    // Ancre 60 : maillage de fond meshFloat (scènes 60*, sans capture motion).
    // Riff vierge (`#5247d1`, luminosité 20, intensité 85, mode sombre) avec `mesh_gradient` activé (`getAdv('mesh_gradient', false)`, `app.js:1673`) et
    // `riff_mesh_cfg` par défaut (`app.js:16133` : points (20, 0) et (85, 5), vitesse 0, flou 0). Layer : `inset: -40px` (`styles.css:160`), soit
    // 1960 x 1160 px CSS pour la fenêtre de 1920 x 1080 ; un pixel CSS vaut 0,945 px écran. Valeurs attendues, `p` = progrès de `meshFloat` :
    // à 0, `scale(1) translate(0, 0)` ; à 50 % de l'aller, `scale(1.06) translate(2%, 1.5%)` ; à 100 %, `scale(1.03) translate(-1.5%, 1%)`.
    // Alphas en sombre : 0,55 et 0,45 (`app.js:1703-1704`) à luminosité 100 %.
    // 60a : vitesse 0 (« 0 (static) », `app.js:16163`), `--mesh-speed: 0s` : aucune animation, `scale(1) translate(0, 0)`.
    MotionScene {
      name: "60a-maillage-statique-defaut",
      reference: None,
      times_ms: &[0],
      setup: |app, t0| mesh_scene(app, t0, 0.0, 0.0),
    },
    // 60b : vitesse 20, `120 / (20 * 0.2 + 1)` = 24 s par aller (`app.js:16149`), `alternate` : le retour dure aussi 24 s. Instants et
    // transformation attendue (chaque segment a sa courbe `cubic-bezier(0.42, 0, 0.58, 1)` : 0,5 à mi-segment) :
    //   0 ms (p 0)       scale 1,000  translate (0 %, 0 %)
    //   6000 ms (p 0,25) scale 1,030  translate (1,0 %, 0,75 %)  : mi-segment 0 -> 50 %
    //   12000 ms (p 0,5) scale 1,060  translate (2,0 %, 1,5 %)   : clé 50 %
    //   18000 ms (p 0,75) scale 1,045 translate (0,25 %, 1,25 %) : mi-segment 50 % -> 100 %
    //   24000 ms (p 1)   scale 1,030  translate (-1,5 %, 1,0 %)  : clé 100 %
    //   36000 ms (p 1,5) scale 1,060  translate (2,0 %, 1,5 %)   : retour, `alternate` rejoue l'aller à l'envers
    //   48000 ms (p 2)   scale 1,000  translate (0 %, 0 %)       : de retour au repos
    MotionScene {
      name: "60b-maillage-flottant-defaut",
      reference: None,
      times_ms: &[0, 6000, 12_000, 18_000, 24_000, 36_000, 48_000],
      setup: |app, t0| mesh_scene(app, t0, 20.0, 0.0),
    },
    // 60c : `filter: blur(30px)` (`--mesh-blur`, `styles.css:165`) à vitesse 0, `scale(1) translate(0, 0)`. Le flou s'applique dans le repère du
    // calque (avant `transform`), `blur(30px)` = écart type de 30 px CSS ; le bord du calque, à 40 px de la fenêtre, laisse passer le fond
    // de `body` : au milieu du bord haut de la fenêtre le calque garde phi(40 / 30) = 91 % de son poids. Sans flou : voir 60a. 60f : 60 px (maximum).
    MotionScene {
      name: "60c-maillage-flou-defaut",
      reference: None,
      times_ms: &[0],
      setup: |app, t0| mesh_scene(app, t0, 0.0, 30.0),
    },
    MotionScene {
      name: "60f-maillage-flou-max-defaut",
      reference: None,
      times_ms: &[0],
      setup: |app, t0| mesh_scene(app, t0, 0.0, 60.0),
    },
    // 60g : maillage actif sous le menu contextuel d'une ligne (capture 14b), rien en lecture, un instant. Le menu doit s'afficher entier avec
    // son fond : le voile et les fonds des calques posés après le maillage ne partagent pas ses bornes (`iced_graphics::layer::Stack::merge`).
    MotionScene {
      name: "60g-maillage-menu-defaut",
      reference: None,
      times_ms: &[0],
      setup: |app, t0| {
        mesh_scene(app, t0, 0.0, 0.0);
        context_menu_liked(app);
      },
    },
    // 60d : changement de couleur à t = 0 (accent `#5247d1` vers `#d14752`), vitesse 20 (période 24 s), calque parti depuis 12 s (échelle 1,06
    // juste avant le changement). `updateMeshGradient` pose `opacity: 1` sur l'autre calque et `opacity: 0` sur l'ancien (`app.js:1721-1722`) ;
    // `.bg-mesh-layer[style*="opacity: 0"] { display: none !important }` (`styles.css:11750`) masque l'ancien à l'instant même, et le nouveau, qui
    // sortait de `display: none`, n'a ni transition ni animation en cours : coupure franche vers la nouvelle couleur (0,55 et 0,45 d'alpha, aucune
    // image intermédiaire) et `meshFloat` repart de 0 % (`meshKey` change, `app.js:1695`). Attendu : t 0 `scale(1)`, 125 ms p 0,005 (échelle
    // 1,0000), 600 ms p 0,025 (échelle 1,0007), 6000 ms p 0,25 `scale(1.03) translate(1 %, 0,75 %)`, soit loin de 1,06 d'avant le changement.
    // Le fondu de 500 ms `ease` (`styles.css:9186-9193`) ne joue que pour le tout premier `updateMeshGradient` activé après le chargement, quand
    // aucun calque n'a de style en ligne (`index.html:309-310`) : il n'a pas de scène ici. Sa courbe est dans `visuals::mesh::layer_fade`
    // (0, 0,80 à 250 ms, 1 à 500 ms), non branchée.
    MotionScene {
      name: "60d-maillage-fondu-defaut",
      reference: None,
      times_ms: &[0, 125, 250, 375, 500, 600, 6000],
      setup: |app, t0| {
        mesh_scene(app, t0 - Duration::from_secs(12), 20.0, 0.0);
        app.update(Message::Settings(SettingsMessage::AccentInput(
          "#d14752".to_string(),
        )));
        let (body, play) = theme_fade_targets(&app.theme);
        app.theme_fade = ThemeFade::at(body, play);
        // `update` date le changement à l'instant réel du banc : on le ramène à `t0`.
        assert!(
          app
            .visuals
            .mesh_started
            .is_some_and(|started| started > t0 - Duration::from_secs(12))
        );
        app.visuals.mesh_started = Some(t0);
      },
    },
    // 60e : panneau « Configure » du maillage dans les réglages du thème, vitesse 20 (période 24 s), `.mesh-preview-bg` : `inset: -20px`
    // (`styles.css:6891`), même `meshFloat` que le fond depuis l'ouverture du panneau, même flou. Transformation attendue comme en 60b :
    // 0 ms `scale(1)`, 12000 ms `scale(1.06) translate(2 %, 1,5 %)`, 24000 ms `scale(1.03) translate(-1,5 %, 1 %)`.
    MotionScene {
      name: "60e-maillage-apercu-defaut",
      reference: None,
      times_ms: &[0, 12_000, 24_000],
      setup: |app, t0| {
        mesh_scene(app, t0, 20.0, 0.0);
        settings(app, Category::Theme);
        app.ui.prefs.mesh_open = true;
        app.ui.prefs.mesh_opened = Some(t0);
      },
    },
    // Ancre 61 : page artiste, badge cookie et loader sur l'horloge de l'app, entrée des onglets artistPaneSlideIn (scènes 61*-defaut, sans capture motion).
    // 61a : badge `cookieBadgeRotate 14s linear infinite` (`styles.css:1899-1918`), parti à t0 (`hidden` retiré, `app.js:12060`). Angle = t / 14000 * 360 deg,
    // sens horaire. La forme `m3-cookie-8` a huit lobes (`app.js:366`) : elle se superpose à elle-même tous les 45 deg, seuls les instants hors
    // multiples de 45 deg la distinguent du repos. Attendu : 0 ms 0 deg (repos) ; 875 ms 22,5 deg (demi-lobe, lobes et creux échangés) ;
    // 3500 ms 90 deg (identique au repos) ; 6125 ms 157,5 deg (demi-lobe) ; 10500 ms 270 deg (identique au repos) ; 14000 ms 360 deg (tour fini, repos).
    // La coche ne tourne pas (`.m3-cookie-check-icon` hors de `.m3-cookie-shape`).
    MotionScene {
      name: "61a-badge-defaut",
      reference: None,
      times_ms: &[0, 875, 3500, 6125, 10_500, 14_000],
      setup: |app, t0| {
        artist_capture(app);
        app.artist_motion.badge = Some(t0);
      },
    },
    // 61b : onglet Albums, `artistPaneSlideIn 260ms cubic-bezier(0.2, 0, 0, 1) both` (`styles.css:2083-2086`, `2127-2129`) parti à t0 (clic sur l'onglet,
    // `app.js:11911-11918`). Origine : centre du panneau. Valeurs attendues (x = 0,6 s (1-s)^2 + s^3, y = 3 s^2 - 2 s^3, `translateY` = 9,45 px écran
    // x (1 - y), échelle = 0,99 + 0,01 y) : 0 ms y = 0 (opacité 0, 9,45 px, 0,99) ; 26 ms y = 0,156 (7,97 px, 0,9916) ; 65 ms y = 0,607 (3,71 px, 0,9961) ;
    // 130 ms y = 0,878 (1,15 px, 0,9988) ; 259 ms y = 1,000 ; 260 ms et 600 ms : repos (aucune transformation).
    MotionScene {
      name: "61b-onglet-albums-defaut",
      reference: None,
      times_ms: &[0, 26, 65, 130, 259, 260, 600],
      setup: |app, t0| {
        artist_capture(app);
        app.ui.artist.tab = ArtistTab::Albums;
        app.artist_motion.pane = Some(t0);
      },
    },
    // 61c : chargeur de la page (`renderM3Loader`, `app.js:11846-11880`) parti à t0 : une forme par 1200 ms (rang = t / 1200), tour en 4 s (angle = t / 4000 * 360 deg,
    // phase de Riff `now % 4000` sur l'horloge de la page, non reproductible : ici 0 au départ). Attendu : 0 ms forme 0, 0 deg ; 600 ms forme 0, 54 deg ;
    // 1200 ms forme 1, 108 deg ; 2400 ms forme 2, 216 deg ; 3600 ms forme 3, 324 deg ; 4000 ms forme 3, 360 deg.
    MotionScene {
      name: "61c-loader-defaut",
      reference: None,
      times_ms: &[0, 600, 1200, 2400, 3600, 4000],
      setup: |app, t0| {
        artist_capture(app);
        let view = &mut app.ui.artist;
        *view = ArtistView {
          name: view.name.clone(),
          loading: true,
          ..ArtistView::default()
        };
        app.artist_motion.loader = Some(t0);
      },
    },
    // 61d : onglet Titres (« Popular Songs »), mêmes valeurs que 61b.
    MotionScene {
      name: "61d-onglet-titres-defaut",
      reference: None,
      times_ms: &[0, 26, 65, 130, 259, 260, 600],
      setup: |app, t0| {
        artist_capture(app);
        app.artist_motion.pane = Some(t0);
      },
    },
    // Ancre 62 : listes, barres d'égaliseur spBarEq, survol de ligne, entrée sbRowFadeIn de la bibliothèque latérale (scènes 62*-defaut, sans capture motion).
    // Titres aimés de la capture 05 avec la 2e ligne en lecture : ses barres d'égaliseur et le survol de la 4e ligne sont visibles.
    // 62a : barres en lecture (`spBarEq`, `styles.css:1711-1721`). L'animation part de t = 0 (`.playing` posé par `highlightPlayingRow`) ; chaque barre
    // reste à 100 % pendant son délai (`fill-mode: none`), puis va de 20 % à 100 % en `ease-in-out`, en alternance. Hauteurs attendues en % de 14 px
    // (b1 délai 100 ms/750 ms, b2 300/550, b3 0/850, b4 200/650) : t=0 [100,100,20,100] ; 150 [21,100,25,100] ; 300 [32,20,41,24] ;
    // 500 [65,42,72,55] ; 800 [99,99,99,99] ; 1100 [81,66,86,75] (b1 et b3 redescendent) ; 1400 [32,20,41,24] ; 1700 [23,66,20,36].
    MotionScene {
      name: "62a-barres-lecture-defaut",
      reference: None,
      times_ms: &[0, 150, 300, 500, 800, 1100, 1400, 1700],
      setup: |app, t0| {
        liked_capture(app);
        app.player.current = Some(app.data.favorites[1].clone());
        app.playback.is_playing = true;
        app.bars_motion.restart(t0, true);
      },
    },
    // 62b : pause à 600 ms (`body.is-playing` retiré : `animation-play-state: paused` et `height: 35% !important`), reprise à 1200 ms là où l'animation
    // s'était arrêtée (temps écoulé 600 ms). Attendu : t=300 [32,20,41,24] ; 590 [80,64,84,73] ; 1000 et 1190 [35,35,35,35] ; 1300 (700 ms
    // écoulées) [93,88,95,91] ; 1500 (900 ms) [99,99,99,99] ; 1800 (1200 ms) [65,42,72,55].
    MotionScene {
      name: "62b-barres-pause-reprise-defaut",
      reference: None,
      times_ms: &[300, 590, 700, 1000, 1190, 1300, 1500, 1800],
      setup: |app, t0| {
        liked_capture(app);
        app.player.current = Some(app.data.favorites[1].clone());
        app.playback.is_playing = true;
        app.bars_motion.restart(t0, true);
        app
          .bars_motion
          .set_running(false, t0 + Duration::from_millis(600));
        app
          .bars_motion
          .set_running(true, t0 + Duration::from_millis(1200));
      },
    },
    // 62c : survol de la 4e ligne. `.track-row { transition: background-color 140ms ease !important }` (`styles.css:1589`) vers `surface-container`
    // (`styles.css:1613`), dans les deux sens. Opacité du fond attendue (ease à p = t/140) : t=0 0 ; 35 0,409 ; 70 0,802 ; 105 0,961 ; 140 1.
    // Entrée : pointeur arrivé à t = 0. Sortie (scène suivante) : pointeur parti à t = 0, fond à 1 avant, donc `1 - ease(p)` :
    // t=0 1 ; 35 0,591 ; 70 0,198 ; 105 0,040 ; 140 0.
    MotionScene {
      name: "62c-survol-ligne-entree-defaut",
      reference: None,
      times_ms: &[0, 35, 70, 105, 140, 300],
      setup: |app, t0| {
        liked_capture(app);
        app.player.current = Some(app.data.favorites[1].clone());
        app.playback.is_playing = false;
        let key = (ListId::Favorites, 3);
        app.ui.row_hover = Some(key);
        app.ui.row_fade.set(key, 1.0, t0, motion::ROW_HOVER);
      },
    },
    MotionScene {
      name: "62c-survol-ligne-sortie-defaut",
      reference: None,
      times_ms: &[0, 35, 70, 105, 140, 300],
      setup: |app, t0| {
        liked_capture(app);
        app.player.current = Some(app.data.favorites[1].clone());
        app.playback.is_playing = false;
        let key = (ListId::Favorites, 3);
        app
          .ui
          .row_fade
          .set(key, 1.0, t0 - Duration::from_secs(1), motion::ROW_HOVER);
        app.ui.row_fade.set(key, 0.0, t0, motion::ROW_HOVER);
      },
    },
    // 62d : clic sur l'onglet « Queue » de la bibliothèque latérale. `sbRowFadeIn 240ms cubic-bezier(0.2, 0, 0, 1) both` (`styles.css:11595`), décalage
    // `min(i, 12) * 20ms`, `i` = rang parmi les `.sb-lib-row` (les trois étiquettes de section ne comptent pas : « Now playing » = rang 0, les titres
    // suivants 1, 2, ...). Opacité, translateY (px CSS), échelle attendus du rang 0 : t=0 (0, 8, 0,985) ; 20 (0,10, 7,2, 0,9865) ; 60 (0,607, 3,14, 0,9941) ;
    // 100 (0,817, 1,46, 0,9973) ; 160 (0,953, 0,37, 0,9993) ; 240 au repos. Rang 2 : t=0 et 20 (0, 8, 0,985) ; 60 (0,10, 7,2, 0,9865) ; 100 (0,607,
    // 3,14, 0,9941) ; 160 (0,878, 0,98, 0,9982) ; 240 (0,99, 0,08, 0,9998) ; 300 au repos. Rang 12 et plus : part à 240 ms, repos à 480 ms.
    MotionScene {
      name: "62d-bibliotheque-onglet-defaut",
      reference: None,
      times_ms: &[0, 20, 60, 100, 160, 240, 300, 480],
      setup: |app, t0| {
        go(app, Screen::Home);
        app.update(Message::Ui(UiMessage::SidebarTab(SidebarTab::Queue)));
        app.entry_motion.sidebar = Some(t0);
      },
    },
    // Ancre 63 : entrées de panneaux, sections des réglages settingsIn, panneau du karaoké tvk-panel-in, erreur de recherche m3ErrFadeIn (scènes 63*-defaut, sans capture motion).
    // 63a : changement de catégorie des réglages (Interface vers Thème, `app.js:17657`), départ à t = 0. `settingsIn` : opacité 0 et
    // `translateY(6px)` vers le repos, 200 ms `ease` sur l'unique segment (`styles.css:7349-7351`). Valeurs attendues (opacité, décalage en
    // pixels CSS) : 0 ms 0 et 6 ; 50 ms 0,41 et 3,55 ; 100 ms 0,80 et 1,19 ; 150 ms 0,96 et 0,24 ; 199 ms et 200 ms 1 et 0 ; 500 ms repos.
    // Le décalage reste à 6 px tant que `ease` n'a pas quitté 0. La section seule bouge : le menu de gauche et le titre de la page non.
    // L'ouverture de la fenêtre sur la dernière catégorie (44*) ne rejoue rien : la capture 04-reglages garde la section à l'opacité 1.
    MotionScene {
      name: "63a-reglages-categorie-defaut",
      reference: None,
      times_ms: &[0, 50, 100, 150, 199, 200, 500],
      setup: |app, t0| {
        settings_behind(app);
        go(app, Screen::Settings);
        app.update(Message::Settings(SettingsMessage::OpenCategory(
          Category::Theme,
        )));
        app.entry_motion.section = Some(t0);
      },
    },
    // 63b : le panneau du karaoké s'ouvre au survol de `#tvk-top-hover-zone` (`app.js:7476`, `7495`), départ à t = 0. Les valeurs de
    // `tvk-panel-in` ne se voient pas : `.open` pose `transform` et `opacity` en `!important`, qui l'emportent sur l'animation. Ce qui joue
    // est la transition de 380 ms `cubic-bezier(0.22, 1, 0.36, 1)` sur `clip-path` et `transform`, de 180 ms `ease` sur `opacity`. Aucun
    // dépassement : y1 = y2 = 1, la courbe monte sans passer 1. Valeurs attendues (découpe visible, décalage en pixels CSS, échelle,
    // opacité) : 0 ms 0, -18, 0,98, 0 ; 40 ms 0,42, -10,5, 0,988, 0,35 ; 90 ms 0,74, -4,6, 0,995, 0,80 ; 180 ms 0,95, -0,9, 0,999, 1 ;
    // 260 ms 0,99, -0,1, 1, 1 ; 340 ms 1, 0, 1, 1 ; 380 ms et 600 ms repos. La découpe est une part de `hauteur + 20 px` depuis le haut.
    MotionScene {
      name: "63b-karaoke-panneau-ouverture-defaut",
      reference: None,
      times_ms: &[0, 40, 90, 180, 260, 340, 380, 600],
      setup: |app, t0| {
        karaoke_motion(app);
        app.lyrics.panel_motion = motion::TvkPanelMotion::launched();
        app.lyrics.panel_motion.set_open(true, t0);
        app.lyrics.tvk_panel = true;
      },
    },
    // 63b, fermeture : `.open` est retiré, mêmes transitions en sens inverse, `opacity 260ms ease 80ms` (le panneau reste opaque 80 ms) et
    // `visibility` tient 380 ms (`styles.css:8863-8866`). Valeurs attendues : 0 ms repos ; 40 ms découpe 0,58, -7,5, 0,992, opacité 1 ;
    // 80 ms 0,30, -12,5, 0,986, 1 ; 150 ms 0,09, -16,5, 0,982, 0,55 ; 210 ms 0,02, -17,6, 0,981, 0,20 ; 340 ms 0, -18, 0,98, 0 ; 380 ms absent.
    MotionScene {
      name: "63b-karaoke-panneau-fermeture-defaut",
      reference: None,
      times_ms: &[0, 40, 80, 150, 210, 340, 379, 380],
      setup: |app, t0| {
        karaoke_motion(app);
        app.lyrics.panel_motion = motion::TvkPanelMotion::launched();
        app
          .lyrics
          .panel_motion
          .set_open(true, t0 - Duration::from_secs(1));
        app.lyrics.panel_motion.set_open(false, t0);
        app.lyrics.tvk_panel = false;
      },
    },
    // 63c : erreur de recherche, la boîte est créée à t = 0 par `stopM3SearchLoaderWithError` (`app.js:9982`). `m3ErrFadeIn` : 320 ms `ease`
    // après 220 ms de délai, la boîte est invisible avant (`opacity: 0` de base, `styles.css:10441`). Valeurs attendues (opacité, décalage
    // en pixels CSS) : 0, 219 et 220 ms 0 et 8 ; 300 ms 0,41 et 4,73 ; 380 ms 0,80 et 1,58 ; 460 ms 0,96 et 0,32 ; 540 ms 1 et 0 ; 800 ms repos.
    // L'icône d'erreur et la forme du chargeur ne font pas partie de cette scène (`m3-search-error-icon`, autre transition).
    MotionScene {
      name: "63c-recherche-erreur-defaut",
      reference: None,
      times_ms: &[0, 219, 220, 300, 380, 460, 540, 800],
      setup: |app, t0| {
        search_results(app, SearchFilter::All);
        app.ui.search.tracks.clear();
        app.ui.search.albums.clear();
        app.ui.search.error = Some("Offline: no internet connection".to_string());
        app.entry_motion.error = Some(t0);
      },
    },
    // Ancre 64 : éditeur audio connecté connectedExpand et connectedCollapse, onglets audioPanelIn, point du minuteur sleepDot (scènes 64*-defaut, sans capture motion).
    // 64a : `connectedExpand 320ms (0.2,0,0,1)` (`styles.css:7585`, `7589`) : le haut de la carte monte du bas, `connectedContentIn` (`styles.css:7587`,
    // `7591`) tient l'en-tête et `.audio-modal-layout` à l'opacité 0 et `translateY(8px)` jusqu'à 30 % (96 ms), puis la courbe court de 30 % à 100 %.
    // `audioPanelIn 220ms` rejoue aussi (la carte quitte `display: none`, `app.js:14983-14987`). Départ à t0, onglet Speed & Pitch (`app.js:14991`).
    // Valeurs attendues (part de la carte visible, opacité du contenu, `translateY` en px CSS) :
    //   0 ms : 0 %, 0, 8 ; 48 ms : 35,1 %, 0, 8 ; 96 ms : 68,8 %, 0, 8 ; 144 ms : 84,4 %, 0,534, 3,73 ; 192 ms : 92,9 %, 0,827, 1,38 ;
    //   240 ms : 97,5 %, 0,946, 0,44 ; 288 ms : 99,6 %, 0,993, 0,06 ; 320 ms : carte entière au repos (identique à 32b).
    MotionScene {
      name: "64a-audio-ouverture-defaut",
      reference: None,
      times_ms: &[0, 48, 96, 144, 192, 240, 288, 320, 400],
      setup: |app, t0| {
        audio_open(app, AudioTab::Tempo);
        app.ui.audio.motion.open(t0);
      },
    },
    // 64b : `.connected-closing` : `connectedCollapse 200ms (0.2,0,0,1)` (`styles.css:7586`, `7590`) et `connectedContentOut 120ms (0.2,0,0,1)`
    // (`styles.css:7588`, `7592`), carte retirée à 200 ms (`app.js:14963-14967`). Départ à t0.
    // Valeurs attendues (part visible, opacité du contenu, `translateY` en px CSS) :
    //   0 ms : 100 %, 1, 0 ; 60 ms : 31,2 %, 0,122, 3,51 ; 120 ms : 7,1 %, 0, 4 ; 200 ms et après : carte retirée.
    MotionScene {
      name: "64b-audio-fermeture-defaut",
      reference: None,
      times_ms: &[0, 60, 120, 199, 200, 260],
      setup: |app, t0| {
        audio_open(app, AudioTab::Tempo);
        app.ui.modal = None;
        app.ui.audio.motion.close(t0);
      },
    },
    // 64c : `.audio-panel.active` : `audioPanelIn 0.22s (0.2,0,0,1)` (`styles.css:7679-7680`), de `opacity: 0; translateY(6px)` à l'identité, départ au clic
    // de l'onglet (`app.js:14785-14790`) : Speed & Pitch vers Audio FX. La carte est au repos.
    // Valeurs attendues (opacité du panneau, `translateY` en px CSS) :
    //   0 ms : 0, 6 ; 55 ms : 0,607, 2,36 ; 110 ms : 0,878, 0,73 ; 165 ms : 0,975, 0,15 ; 220 ms et après : 1, 0 (identique à 32c).
    MotionScene {
      name: "64c-audio-onglet-defaut",
      reference: None,
      times_ms: &[0, 55, 110, 165, 220, 300],
      setup: |app, t0| {
        audio_open(app, AudioTab::Effects);
        app.ui.audio.motion.switch_panel(t0, false);
      },
    },
    // 64d : `#sleep-timer-status.active::before` : point de 6 px, `sleepDot 2s ease-in-out infinite` (`styles.css:9887-9898`), `0%, 100%` opacité 0,35 et
    // échelle 0,75, `50%` opacité 1 et échelle 1, `ease-in-out` par segment. Départ quand le minuteur devient actif (`app.js:15335-15341`) : t0. Minuterie
    // « End of track » (texte fixe, le compte à rebours lit l'horloge réelle). Valeurs attendues (opacité, échelle, diamètre en px CSS) :
    //   0 ms : 0,35, 0,75, 4,5 ; 500 ms : 0,675, 0,875, 5,25 ; 1000 ms : 1, 1, 6 ; 1500 ms : 0,675, 0,875, 5,25 ; 2000 ms : 0,35, 0,75, 4,5.
    MotionScene {
      name: "64d-audio-minuteur-point-defaut",
      reference: None,
      times_ms: &[0, 500, 1000, 1500, 2000],
      setup: |app, t0| {
        audio_open(app, AudioTab::Sleep);
        app.ui.audio.sleep = SleepTimer::EndOfTrack;
        app.ui.audio.motion.start_dot(t0);
      },
    },
    // Ancre 65 : boutons, onde rippleOut au clic, état .pressed tenu 180 ms, survol --hl et rayon des boutons génériques (scènes 65*-defaut, sans capture motion).
    // 65a : onde `rippleOut` sur `#btn-toggle-queue-panel` (`.btn-icon-pill`, 36 px CSS = 34,02 px écran, accueil, panneau ouvert). Clic gauche à
    // t0 au point (6, 9) du bouton (`app.js:2622-2638`) : diamètre = 2 x distance au coin le plus éloigné (coin bas droit : dx 28,02, dy 25,02 =
    // 37,57 px de rayon). `rippleOut .55s cubic-bezier(.2, 0, 0, 1) forwards` (`styles.css:7503-7504`) : échelle 0 vers 1 et opacité .16 vers 0 sur la
    // même courbe (une seule paire d'images clés), retrait à `animationend` (550 ms), `filter: blur(2px)`, coupe `overflow: hidden` au rayon du bouton.
    // Valeurs attendues (rayon en px écran, opacité au centre, avancement de la courbe) :
    //   0 ms : 0, 0,160, 0 ; 70 ms : 9,94, 0,118, 0,2646 ; 140 ms : 23,12, 0,062, 0,6154 ; 275 ms : 32,98, 0,020, 0,8778 ;
    //   410 ms : 36,61, 0,004, 0,9745 ; 550 ms et après : aucune onde (le bouton n'a plus que son survol).
    MotionScene {
      name: "65a-ripple-defaut",
      reference: None,
      times_ms: &[0, 70, 140, 275, 410, 550, 700],
      setup: |app, t0| {
        go(app, Screen::Home);
        app.lyrics.panel_open = true;
        app.ui.controls.ripple(
          icons::QUEUE_LIST,
          Point::new(6.0, 9.0),
          Size::new(34.02, 34.02),
          t0,
        );
      },
    },
    // 65b : `.pressed` tenu après un clic bref sur le même bouton : `pointerdown` à t0 (`app.js:2610`), `pointerup` à 60 ms, la classe part à
    // `max(0, PRESS_MIN - 60) = 120 ms` du relâchement, soit à 180 ms de t0 (`app.js:2615-2616`). Fond `on-surface` 12 % sur 180 ms `ease` : la règle
    // `:is(...)` de `styles.css:7508-7509` (`background-color .18s ease !important`) bat les 150 ms de `.pressed` (`styles.css:6239-6246`). Montée de 0 à 180 ms, départ de la classe à 180 ms, descente de 180 à 360 ms. L'onde du même clic est posée aussi.
    // Avancement du fond de `.pressed` attendu (0 = fond de repos, 1 = `on-surface` à 22,56 %) :
    //   0 ms : 0 ; 90 ms : 0,802 ; 179 ms : 1 (encore tenu) ; 270 ms : 0,198 ; 359 ms : 0 (0,0003) ; 360 ms et après : 0.
    MotionScene {
      name: "65b-pressed-defaut",
      reference: None,
      times_ms: &[0, 90, 179, 270, 359, 360, 400],
      setup: |app, t0| {
        go(app, Screen::Home);
        app.lyrics.panel_open = true;
        app.ui.controls.press(icons::QUEUE_LIST, t0);
        app
          .ui
          .controls
          .release(icons::QUEUE_LIST, t0 + Duration::from_millis(60));
      },
    },
    // 65c : survol d'une pilule générique (`button::filled` « Save » du dialogue de renommage ; `.btn-pill-primary` : `border-radius 28px` borné à
    // 20 px vers `18px`, `styles.css:7511`, `7519`). Entrée à t0 : fond en `.18s ease`, rayon en `.22s (.2, 0, 0, 1)` (`styles.css:7509`).
    // riff-rs garde le rayon de repos de `tokens::radius::BUTTON` (13,23 px) et lui retire les 2 px CSS (1,89 px) que Riff perd.
    // Valeurs attendues (avancement de `--hl` : `on-surface` 8 % sur le fond de repos `primary`, soit (180,159,224) vers (183,164,225) pour `on-surface` #e2e2e9 ; rayon en px écran) :
    //   0 ms : 0, 13,23 ; 45 ms : 0,4085 (181,161,224), 12,26 ; 90 ms : 0,8023 (183,163,225), 11,70 ; 180 ms : 1 (184,164,225), 11,36 ; 220  ms et après : 1, 11,34.
    MotionScene {
      name: "65c-survol-pilule-defaut",
      reference: None,
      times_ms: &[0, 45, 90, 180, 220, 400],
      setup: |app, t0| {
        rename_dialog(app);
        let key = motion::button_key("chrome.rs", "Save", 0);
        app.ui.controls.set(key, true, t0);
      },
    },
    // 65d : survol d'un bouton rond générique (`button::icon` « presets » de la barre latérale repliée, 32 px ; `.rail-icon-btn` : `50%` vers `40%`,
    // `styles.css:7514`, `7520`). Mêmes transitions que 65c ; le rayon perd 10 % du côté (3,2 px) sur le rayon de repos de riff-rs (13,23 px).
    // Valeurs attendues (avancement de `--hl` : `on-surface` 8 % sur le fond de repos transparent, soit un fond de rail de 26 qui monte vers 42 ; rayon en px écran) :
    //   0 ms : 0, 26, 13,23 ; 45 ms : 0,4085, 32,5, 11,59 ; 90 ms : 0,8023, 38,8, 10,64 ; 180 ms : 1, 42, 10,07 ; 220 ms et après : 1, 42, 10,03.
    MotionScene {
      name: "65d-survol-rond-defaut",
      reference: None,
      times_ms: &[0, 45, 90, 180, 220, 400],
      setup: |app, t0| {
        app.data.sidebar_width = riff_app::library::SIDEBAR_COLLAPSED_WIDTH;
        go(app, Screen::Home);
        drop(app.view());
        let key = motion::button_key("chrome.rs", icons::DISCOVER_TUNE, 0);
        app.ui.controls.set(key, true, t0);
      },
    },
    // 65e : onde sur la pilule « Save » (70 x 36 px mesurés sur 65c), clic à 3 px du bord gauche et 5 px du haut : l'onde est plus grande que le bouton et doit suivre
    // les coins arrondis (coupe `overflow: hidden` au rayon courant). Mêmes instants et courbe que 65a ; rayon à 0 / 70 / 140 / 275 ms, `reach` = hypot(67, 31) = 73,82 px :
    //   0, 19,53 (73,82 x 0,2646), 45,43, 64,80.
    MotionScene {
      name: "65e-ripple-pilule-defaut",
      reference: None,
      times_ms: &[0, 70, 140, 275, 550],
      setup: |app, t0| {
        rename_dialog(app);
        let key = motion::button_key("chrome.rs", "Save", 0);
        app
          .ui
          .controls
          .ripple(key, Point::new(3.0, 5.0), Size::new(70.0, 36.0), t0);
      },
    },
    // Ancre 66 : panneau du karaoké centré, transition tvk-panel, décalage de .tvk-panel-inner, icône d'erreur de recherche (scènes 66*-defaut, sans capture motion).
    // 66a : ouverture du panneau du karaoké (`.open` posé, `app.js:7476`, `7495`), départ à t = 0. Placement `styles.css:8839-8847` : haut de la
    // fenêtre, centré, largeur `min(380px, 100vw - 32px)`, coins du bas de 28 px, raccords concaves de 26 px en haut. Transitions `styles.css:8879-8882`
    // sur la découpe `clip-path` (part de `hauteur + 20 px` visible depuis le haut), le `transform` (`translate3d(-50%, -18px, 0) scale(0.98)` vers
    // `scale(1)`, origine haut centre) et l'opacité (180 ms `ease`) ; courbe de forme `cubic-bezier(0.22, 1, 0.36, 1)` sur 380 ms. Valeurs attendues
    // (découpe, décalage en pixels CSS, échelle, opacité) : 0 ms 0, -18, 0,980, 0 ; 95 ms 0,765, -4,23, 0,9953, 0,83 ; 180 ms 0,952, -0,86, 0,9990, 1 ;
    // 190 ms 0,961, -0,70, 0,9992, 1 ; 285 ms 0,997, -0,06, 0,9999, 1 ; 380 ms et 600 ms repos. Les raccords suivent la découpe et l'opacité.
    MotionScene {
      name: "66a-karaoke-panneau-ouverture-defaut",
      reference: None,
      times_ms: &[0, 95, 180, 190, 285, 380, 600],
      setup: |app, t0| {
        karaoke_motion(app);
        app.lyrics.panel_motion = motion::TvkPanelMotion::launched();
        app.lyrics.panel_motion.set_open(true, t0);
        app.lyrics.tvk_panel = true;
      },
    },
    // 66a, fermeture : `.open` retiré, `opacity 260ms ease 80ms` (opaque pendant 80 ms), `visibility` tient 380 ms (`styles.css:8863-8866`). Valeurs
    // attendues (découpe, décalage, échelle, opacité) : 0 ms 1, 0, 1, 1 ; 80 ms 0,305, -12,52, 0,9861, 1 ; 95 ms 0,235, -13,77, 0,9847, 0,96 ;
    // 190 ms 0,039, -17,30, 0,9808, 0,29 ; 285 ms 0,003, -17,94, 0,9801, 0,03 ; 340 ms 0, -18, 0,980, 0 ; 379 ms idem (encore dessiné, invisible) ;
    // 380 ms absent.
    MotionScene {
      name: "66a-karaoke-panneau-fermeture-defaut",
      reference: None,
      times_ms: &[0, 80, 95, 190, 285, 340, 379, 380],
      setup: |app, t0| {
        karaoke_motion(app);
        app.lyrics.panel_motion = motion::TvkPanelMotion::launched();
        app
          .lyrics
          .panel_motion
          .set_open(true, t0 - Duration::from_secs(1));
        app.lyrics.panel_motion.set_open(false, t0);
        app.lyrics.tvk_panel = false;
      },
    },
    // 66b : `.tvk-panel-inner` seul (`styles.css:8904-8925`) : `translateY(-6px)` vers 0 en 220 ms `cubic-bezier(0.16, 1, 0.3, 1)`, dans les deux sens,
    // sans délai. Le panneau est déjà ouvert et au repos (découpe, échelle, opacité à 1) ; seul le contenu glisse. Valeurs attendues du décalage
    // en pixels CSS : 0 ms -6,00 ; 55 ms -1,05 ; 110 ms -0,17 ; 220 ms 0 ; 600 ms repos. Remplissage 18/22/24 px, espace de 14 px, hauteur
    // `min(76vh, 580px)`, fondu de 36 px en bas par un dégradé du fond du panneau (le masque est impossible en iced).
    MotionScene {
      name: "66b-karaoke-panneau-interne-defaut",
      reference: None,
      times_ms: &[0, 55, 110, 220, 600],
      setup: |app, t0| {
        karaoke_motion(app);
        app.lyrics.panel_motion = motion::TvkPanelMotion::launched();
        app
          .lyrics
          .panel_motion
          .set_open(true, t0 - Duration::from_secs(3));
        app
          .lyrics
          .panel_motion
          .set_inner(false, t0 - Duration::from_secs(2));
        app.lyrics.panel_motion.set_inner(true, t0);
        app.lyrics.tvk_panel = true;
      },
    },
    // 66c : icône d'erreur de la recherche (`.m3-search-error-icon`, `styles.css:10417-10432`), `.is-error` posé à t = 0 avec la boîte (`app.js:9940`,
    // `9982`). Transitions de 320 ms après 180 ms : opacité 0 vers 0,85 en `ease`, échelle 0,75 vers 1 en `cubic-bezier(0.2, 0.9, 0.2, 1)`, origine au centre.
    // Valeurs attendues (opacité, échelle) : 0 et 180 ms 0 et 0,750 ; 260 ms 0,347 et 0,9518 ; 340 ms 0,682 et 0,9906 ; 420 ms 0,816 et 0,9987 ;
    // 500 ms 0,85 et 1 ; 600 ms repos. La boîte d'erreur suit 63c (délai de 220 ms).
    MotionScene {
      name: "66c-recherche-erreur-icone-defaut",
      reference: None,
      times_ms: &[0, 180, 260, 340, 420, 500, 600],
      setup: |app, t0| {
        search_results(app, SearchFilter::All);
        app.ui.search.tracks.clear();
        app.ui.search.albums.clear();
        app.ui.search.error = Some("Offline: no internet connection".to_string());
        app.entry_motion.error = Some(t0);
      },
    },
    // Ancre 67 : accueil, entrée m3FadeUp des cartes mix, spinner circulaire M3 sur l'horloge de l'app (scènes 67*-defaut, sans capture motion).
    // 67 : aucune scène de cartes mix. `.mix-card` (`styles.css:1426-1431`, `m3FadeUp 400ms` et délais 0, 80, 160 ms) n'est créé nulle part : `app.js` ne
    // contient la classe que dans deux sélecteurs (`2609`, `2622`) et `index.html` pas du tout ; les cartes de l'accueil sont des `.sp-card` (`app.js:25101`,
    // `25437`, `25495`) sans `animation`. Rien à animer, donc rien à porter (même constat que 58a).
    // 67a : `.m3-circular-progress` (`styles.css:3353-3391`) des cartes « Pour toi » et des tendances. Le départ est posé ici (`home_spinner`, désactivé dans le banc).
    // `m3Rotate` : 1,4 s linéaire, 360 deg. `m3Dash` : 1,4 s, `ease-in-out` (`cubic-bezier(.42,0,.58,1)`) sur chaque segment 0-50 % et 50-100 % ;
    // dasharray 1 -> 90 puis 90, dashoffset 0 -> -35 puis -35 -> -125. Cercle r = 18 (circonférence 113,1), départ de l'arc à 3 h, sens horaire.
    // Valeurs attendues (rotation du SVG, début de l'arc depuis 3 h en tenant compte de la rotation, longueur d'arc, en unités SVG) :
    //    0 ms : rotation 0 deg, dash 1, offset 0 : point à 0 deg.
    //  175 ms : rotation 45 deg, x = 0,25, ease 0,129 : dash 12,5, offset -4,5 : arc de 59 deg à 99 deg.
    //  350 ms : rotation 90 deg, ease 0,5 : dash 45,5, offset -17,5 : arc de 146 deg à 290 deg.
    //  700 ms : rotation 180 deg, ease 1 : dash 90, offset -35 : le tiret couvre de 35 à 113,1 unités (il ne repasse pas le début du cercle) : arc de 291 deg à 180 deg, soit 249 deg, quasi tout le tour.
    // 1050 ms : rotation 270 deg, ease 0,5 : dash 90, offset -80, coupé au bout du cercle (113,1) : arc de 33 unités, de 165 deg à 270 deg.
    // 1399 ms : offset -125 : arc hors du cercle, seul l'anneau de fond reste. 1400 ms : l'animation boucle, image identique à 0 ms.
    MotionScene {
      name: "67a-spinner-defaut",
      reference: None,
      times_ms: &[0, 175, 350, 700, 1050, 1399, 1400],
      setup: |app, t0| {
        go(app, Screen::Home);
        nothing_playing(app);
        app.lyrics.panel_open = true;
        app.ui.home.wave.clear();
        app.ui.home.trending.clear();
        app.ui.home.wave_loading = true;
        app.ui.home.trending_loading = true;
        app.ui.home.mixes_busy = true;
        app.ui.home.mixes_planned = 5;
        app.data.daily_mixes = Some(DailyMixes::default());
        app.home_spinner.start = Some(t0);
        app.trending_spinner.start = Some(t0);
      },
    },
    // Ancre 68 : réduction de mouvement force-reduced-motion et mode performance body.perf (scènes 68*-defaut, sans capture motion).
    // Réglages « Performance mode » (Interface, `index.html:1308-1314`, entre « Visualizer sensitivity » et « Customize sidebar & home layout ») et
    // « Reduce motion » (Advanced, `index.html:2616-2624`, sous « Home layout »), au repos d'un Riff vierge : interrupteurs éteints
    // (`localStorage` absent : mode performance `=== '1'` faux ; réduction = préférence du système, que le banc impose à « animations actives »).
    // Descriptions : « Disables visualizers, animated gradient and blur effects, limits animations to 30 FPS » et
    // « Replace spring animations with simple fades ». Page 4 de l'Interface (638 px par page), page 3 d'Advanced (section dépliée).
    MotionScene {
      name: "68a-reglages-performance-defaut-p4",
      reference: None,
      times_ms: &[0],
      setup: |app, _t0| settings(app, Category::Interface),
    },
    MotionScene {
      name: "68b-reglages-reduction-defaut-p3",
      reference: None,
      times_ms: &[0],
      setup: |app, _t0| {
        settings(app, Category::Advanced);
        app.settings.adv_open = true;
      },
    },
    // Mêmes rangées, interrupteurs allumés : `.effect-switch-btn.active`.
    MotionScene {
      name: "68c-reglages-performance-actif-defaut-p4",
      reference: None,
      times_ms: &[0],
      setup: |app, _t0| {
        settings(app, Category::Interface);
        app.update(Message::Settings(SettingsMessage::SetBool(
          BoolKey::PerfMode,
          true,
        )));
      },
    },
    MotionScene {
      name: "68d-reglages-reduction-actif-defaut-p3",
      reference: None,
      times_ms: &[0],
      setup: |app, _t0| {
        settings(app, Category::Advanced);
        app.settings.adv_open = true;
        app.update(Message::Settings(SettingsMessage::SetBool(
          BoolKey::ReduceMotion,
          true,
        )));
      },
    },
    // 68e : transition de vue (Accueil vers Titres aimés, comme 47a) avec `force-reduced-motion` : `m3ContainerOpen` dure 0,001 ms
    // (`styles.css:185`), la vue est à l'état final dès le départ. Attendu à t = 0, 50 et 400 ms : opacité 1, `scale(1)`, `translateY(0)`,
    // identiques à l'état stable (sans réduction, 47a : opacité 0 et `scale(0.96)` à t = 0).
    MotionScene {
      name: "68e-vue-reduite-defaut",
      reference: None,
      times_ms: &[0, 50, 400],
      setup: |app, t0| {
        go(app, Screen::Home);
        app.lyrics.panel_open = true;
        app.view_motion.enabled = true;
        motion::set_reduced(true);
        go(app, Screen::Liked);
        app.view_motion.start = Some(t0);
      },
    },
    // 68f : maillage animé (vitesse 20, 24 s par aller, comme 60b) sous `body.perf` : `[class*="mesh"], .bg-mesh-layer { animation: none }`
    // (`styles.css:7542`). Le calque reste affiché et immobile : `scale(1) translate(0, 0)` à tout instant (60b : 1,03 à 6 s, 1,06 à 12 s).
    MotionScene {
      name: "68f-maillage-performance-defaut",
      reference: None,
      times_ms: &[0, 6000, 12_000],
      setup: |app, t0| {
        mesh_scene(app, t0, 20.0, 0.0);
        motion::set_perf(true);
      },
    },
    // 68g : barres d'égaliseur en lecture (comme 62a) sous `body.perf` : `animation-play-state: paused; height: 35% !important`
    // (`styles.css:1725-1728`) : [35, 35, 35, 35] % à tout instant (62a : [100, 100, 20, 100] à t = 0, [65, 42, 72, 55] à 500 ms).
    MotionScene {
      name: "68g-barres-performance-defaut",
      reference: None,
      times_ms: &[0, 500, 1100],
      setup: |app, t0| {
        liked_capture(app);
        app.player.current = Some(app.data.favorites[1].clone());
        app.playback.is_playing = true;
        app.bars_motion.restart(t0, true);
        motion::set_perf(true);
      },
    },
    // 68h : mêmes barres sous `force-reduced-motion` : `spBarEq` (`animation-iteration-count 1`, 0,001 ms) s'achève sans remplissage et la
    // hauteur revient à `height: 100%` (`styles.css:1703`), les délais de 0 à 300 ms ne changent rien : [100, 100, 100, 100] % à tout instant.
    MotionScene {
      name: "68h-barres-reduit-defaut",
      reference: None,
      times_ms: &[0, 500, 1100],
      setup: |app, t0| {
        liked_capture(app);
        app.player.current = Some(app.data.favorites[1].clone());
        app.playback.is_playing = true;
        app.bars_motion.restart(t0, true);
        motion::set_reduced(true);
      },
    },
    // 68i : orbes du karaoké (comme 52e, instants 0 et 5,6 s) sous `body.perf` : `animation-play-state: paused` (`styles.css:6359`), donc sur
    // la première image clé : b1 `scale(1.05)`, b2 `scale(1.1)`, b3 `scale(1)`, b4 `scale(1.15)`, translations et rotations nulles, à tout instant.
    MotionScene {
      name: "68i-orbes-performance-defaut",
      reference: None,
      times_ms: &[0, 5600],
      setup: |app, t0| {
        karaoke_opened(app, t0 - Duration::from_millis(400));
        motion::set_perf(true);
      },
    },
    // 68j : barre de progression sous `body.perf` : `no-wavy` (`app.js:17013`) trace un trait droit de la même largeur (`app.js:3493`) au lieu
    // de l'onde (amplitude 5 px, longueur d'onde 42 px). Écran d'accueil, lecture en cours à mi-titre.
    MotionScene {
      name: "68j-onde-performance-defaut",
      reference: None,
      times_ms: &[0],
      setup: |app, _t0| {
        go(app, Screen::Home);
        motion::set_perf(true);
      },
    },
    // Ancre 69 : survol et onde des boutons, clé de survol donnée par l'appelant, onde au-dessus du texte, survol .setting-item --hl 4 % (scènes 69*-defaut, sans capture motion).
    // 69a : survol d'une rangée de réglage (`.setting-item` « Hide top titlebar », Réglages, Interface ; fond `surface-container`). Entrée du
    // pointeur à t0 : `.setting-item:hover { --hl: 4% }` (`styles.css:7520`, dans `@media (hover: hover)`, `styles.css:7517`), `--hl` en calque
    // `on-surface` (`styles.css:7507`) et `transition: --hl .18s ease` (`styles.css:7509`, `!important`, bat les 150 ms de `styles.css:3480`).
    // Avancement de `--hl` : `ease` (.25, .1, .25, 1) sur t / 180 ms, soit 0 à 0 ms, 0,4085 à 45 ms, 0,8023 à 90 ms, 1 à 180 ms et après ;
    // couleur de la rangée = fond + (on-surface - fond) x 4 % x avancement. Valeurs attendues en RGB (fond de la rangée (33, 33, 33), `on-surface` (226, 226, 226)) : 0 ms (33, 33, 33) ; 45 ms (36, 36, 36) ; 90 ms (39, 39, 39) ; 180 ms et 400 ms (41, 41, 41).
    MotionScene {
      name: "69a-reglage-survol-defaut",
      reference: None,
      times_ms: &[0, 45, 90, 180, 400],
      setup: |app, t0| {
        settings(app, Category::Interface);
        app.ui.controls.set(
          motion::ControlKey::new(("setting-item", "Hide top titlebar")),
          true,
          t0,
        );
      },
    },
    // 69b : même survol sur une rangée de la fenêtre audio (`.setting-item` « Speed », onglet Tempo, 1re rangée : `border-radius: 20px 20px 4px 4px`,
    // `styles.css:7682`), fond `surface-container-high` (`styles.css:7681`). Mêmes instants et même courbe. Valeurs attendues en RGB (fond (41, 41, 41)) : 0 ms (41, 41, 41) ; 45 ms (44, 44, 44) ; 90 ms (47, 47, 47) ; 180 ms et 400 ms (48, 48, 48).
    MotionScene {
      name: "69b-audio-reglage-survol-defaut",
      reference: None,
      times_ms: &[0, 45, 90, 180, 400],
      setup: |app, t0| {
        audio_open(app, AudioTab::Tempo);
        app.ui.controls.set(
          motion::ControlKey::new(("audio-item", "tempo", 0_usize)),
          true,
          t0,
        );
      },
    },
    // 69c : `body.perf` : `transition: none !important` (`styles.css:7541`), la rangée prend `--hl: 4 %` dès l'entrée : avancement 1 à 0 ms.
    // Valeurs attendues en RGB : (41, 41, 41) à 0 ms et à 90 ms (comme 69a à 180 ms).
    MotionScene {
      name: "69c-reglage-survol-performance-defaut",
      reference: None,
      times_ms: &[0, 90],
      setup: |app, t0| {
        settings(app, Category::Interface);
        motion::set_perf(true);
        app.ui.controls.set(
          motion::ControlKey::new(("setting-item", "Hide top titlebar")),
          true,
          t0,
        );
      },
    },
    // 69d : `.setting-item.is-disabled { pointer-events: none }` (`styles.css:3502-3506`) : « Intensity » (Réglages, Thème, mode automatique, opacité
    // 0,45) ne réagit pas au survol. Même si la clé est posée, la couleur reste celle de repos à tout instant : (29, 29, 29) à 0 ms et à 180 ms
    MotionScene {
      name: "69d-reglage-desactive-defaut",
      reference: None,
      times_ms: &[0, 180],
      setup: |app, t0| {
        settings(app, Category::Theme);
        app.update(Message::Settings(SettingsMessage::SetText(
          TextKey::ThemeMode,
          "auto".to_string(),
        )));
        app.ui.controls.set(
          motion::ControlKey::new(("setting-item", "Intensity")),
          true,
          t0,
        );
      },
    },
    // 69e : onde `rippleOut` au-dessus du libellé d'un bouton plein (`button::filled` « Save », 70 x 36 px mesurés sur 65c). `.ripple-host` est
    // ajouté en dernier enfant du bouton (`app.js:2630`), `position: absolute; inset: 0; overflow: hidden; border-radius: inherit`
    // (`styles.css:7502`) : il est peint par-dessus le texte. Clic au centre du libellé (35, 18) : `reach` = hypot(35, 18) = 39,36 px. Mêmes
    // courbe et durée que 65a. Valeurs attendues (rayon en px écran, opacité au centre) : 70 ms : 10,41, 0,118 ; 140 ms : 24,22, 0,062 ;
    // 275 ms : 34,55, 0,020 ; l'onde (couleur `on-primary`, `currentColor`) s'étend sur le libellé et le fond : au centre du libellé le fond
    // monte à 70 ms vers (164, 145, 208) à 8 px au-dessus du centre (`primary` (180, 159, 224), `on-primary` ~ (44, 40, 88), 0,118 d'opacité).
    MotionScene {
      name: "69e-ripple-sur-libelle-defaut",
      reference: None,
      times_ms: &[70, 140, 275],
      setup: |app, t0| {
        rename_dialog(app);
        let key = motion::button_key("chrome.rs", "Save", 0);
        app
          .ui
          .controls
          .ripple(key, Point::new(35.0, 18.0), Size::new(70.0, 36.0), t0);
      },
    },
    // 69f : deux boutons du même lieu d'appel et du même libellé : les flèches « monter » (`button::icon_px` `PL_CHEVRON_UP`, `icon_px_button` de
    // `settings.rs`) des rangées du personnalisateur « Customize sidebar & home layout » (Réglages, Interface, page 6 de défilement). Seule celle
    // de la 2e rangée est survolée à t0 : fond `on-surface` 8 % sur 180 ms (comme 65d) ; celle de la 1re rangée reste au repos à tout instant.
    // Avancement du survolé : 0 à 0 ms, 0,4085 à 45 ms, 0,8023 à 90 ms, 1 à 180 ms et après ; l'autre : 0.
    MotionScene {
      name: "69f-reglages-deux-boutons-meme-libelle-defaut-p6",
      reference: None,
      times_ms: &[0, 45, 90, 180, 400],
      setup: |app, t0| {
        settings(app, Category::Interface);
        let key = motion::button_key("settings.rs", icons::PL_CHEVRON_UP, 1);
        app.ui.controls.set(key, true, t0);
      },
    },
    // Ancre 70 : panneau du karaoké, voile d'opacité, masque bas et raccord droit de tvk-panel-in (scènes 70*-defaut, sans capture motion).
    // 70a : voile d'opacité de groupe du panneau pendant l'ouverture (`.open` posé à t = 0, mêmes transitions que 66a). `tvk-panel-in`
    // (`styles.css:8868-8873`) ne joue jamais : `.open` pose `opacity` et `transform` en `!important` (`styles.css:8875-8877`), qui
    // l'emportent sur une animation ; l'opacité vient de la transition de 180 ms `ease` (`styles.css:8881`) et la forme de celle de 380 ms
    // `cubic-bezier(0.22, 1, 0.36, 1)`. Opacité de groupe `o` : 0 ms 0 ; 50 ms 0,4682 ; 95 ms 0,8286 ; 170 ms 0,9983 ; 340 ms et 600 ms 1.
    // Découpe `r` : 0, 0,5016, 0,7649, 0,9418, 0,9998, repos. Riff compose le groupe (panneau, ombre, `::before` et `::after`) à `o` sur
    // ce qui est dessous : en un pixel de la boîte du panneau, RGB = `o * fond du panneau + (1 - o) * pixel du fond sans panneau`.
    // Fond du panneau `--drawer-bg` = `surface_container_high` = (41, 41, 41). Pixel du fond : celui de `70c` aux mêmes coordonnées.
    // Points A (790, 200) et B (790, 150), dans la marge de remplissage gauche du panneau (aucun contenu) : dans la boîte à partir de 50 ms
    // (bas de la boîte à 273 px). Fond de `70c` : A (128, 106, 113), B (118, 84, 91). Valeurs attendues (A ; B) : 0 ms hors boîte, le fond ;
    // 50 ms (87, 76, 79) ; (82, 64, 68) ; 95 ms (56, 52, 53) ; (54, 48, 50) ; 170 ms (41, 41, 41) ; 340 ms et 600 ms (41, 41, 41).
    // Le fond (aplat, orbes, vignette) est composé en une image unique (`Orbs::flatten`) dessinée à `1 - o` : l'écart hors des lignes de
    // paroles est de 0,5 en moyenne (max 1,2) à 50 ms. Sous une ligne de paroles (absente du fond rejoué) il monte à 16,5 à 50 ms et 5,4 à 95 ms.
    MotionScene {
      name: "70a-karaoke-panneau-voile-defaut",
      reference: None,
      times_ms: &[0, 50, 95, 170, 340, 600],
      setup: |app, t0| {
        karaoke_motion(app);
        app.lyrics.panel_motion = motion::TvkPanelMotion::launched();
        app.lyrics.panel_motion.set_open(true, t0);
        app.lyrics.tvk_panel = true;
      },
    },
    // 70b : panneau ouvert et au repos. Masque bas (`mask-image` de `.tvk-panel-inner`, `styles.css:8916-8917`) : de l'ordonnée
    // 548,1 - 34,02 = 514,1 à 548,1 (bas du panneau, `max-height: min(76vh, 580px)` = 580 px CSS = 548,1 px écran), l'alpha du contenu
    // passe de 1 à 0 ; le fond `--drawer-bg` du panneau, opaque, apparaît à sa place. Abscisses (`left: 50%`, largeur 380 px CSS = 359,1 px) :
    // bord gauche 780,45, bord droit 1139,55, bord droit du contenu 1139,55 - 22 px CSS (20,79) = 1118,76, pointe des raccords
    // 780,45 - 24,57 = 755,88 et 1139,55 + 24,57 = 1164,12 (rangée 0), rayon de 28 px CSS (26,46) en bas.
    MotionScene {
      name: "70b-karaoke-panneau-masque-raccord-defaut",
      reference: None,
      times_ms: &[600],
      setup: |app, t0| {
        karaoke_motion(app);
        app.lyrics.panel_motion = motion::TvkPanelMotion::launched();
        app
          .lyrics
          .panel_motion
          .set_open(true, t0 - Duration::from_secs(3));
        app.lyrics.tvk_panel = true;
      },
    },
    // 70c : même fond sans panneau (fermé depuis 1 s) : le pixel de fond de chaque point de 70a.
    MotionScene {
      name: "70c-karaoke-fond-sans-panneau-defaut",
      reference: None,
      times_ms: &[600],
      setup: |app, t0| {
        karaoke_motion(app);
        app.lyrics.panel_motion = motion::TvkPanelMotion::launched();
        app
          .lyrics
          .panel_motion
          .set_open(true, t0 - Duration::from_secs(3));
        app
          .lyrics
          .panel_motion
          .set_open(false, t0 - Duration::from_secs(1));
        app.lyrics.tvk_panel = false;
      },
    },
    // Ancre 71 : toast, pause du minuteur au survol et reprise à la sortie (scènes 71*-defaut, sans capture motion).
    // Toast `info` de 3500 ms posé à t0 (entrée finie à 340 ms), survol et sortie datés par l'appelant. Sortie `dropletToastOut` : 200 ms,
    // `cubic-bezier(0.4, 0, 1, 1)` (`styles.css:4668`, `4673`). Opacité attendue à 0, 50, 100, 150 et 200 ms de `.leaving` :
    // 1,000, 0,901, 0,675, 0,370 et 0 (translateY -9,9 %, -32,5 %, -63,0 % et -100 % de la hauteur ; scaleX 0,980, 0,935, 0,874, 0,800).
    // 71a : survol à 1000 ms, jamais quitté. La fermeture sans survol partirait à 3500 ms (fin à 3700 ms) : à 3400, 3600, 4000 et
    // 9000 ms le toast reste posé, opacité 1,000, sans transformation.
    MotionScene {
      name: "71a-toast-survol-defaut",
      reference: None,
      times_ms: &[1000, 3400, 3600, 4000, 9000],
      setup: |app, t0| hovered_toast(app, t0, 1000, None),
    },
    // 71b : survol à 1000 ms (`remaining` = 3500 - 1000 = 2500 ms), sortie du pointeur à 5000 ms : `.leaving` à 5000 + 2500 = 7500 ms.
    // 5000 et 7400 ms : posé, opacité 1,000. 7500 : début de la sortie (1,000). 7550 : 0,901. 7600 : 0,675. 7650 : 0,370.
    // 7700 et 7800 ms : 0 (animation finie, `fill-mode: forwards`).
    MotionScene {
      name: "71b-toast-reprise-defaut",
      reference: None,
      times_ms: &[5000, 7400, 7500, 7550, 7600, 7650, 7700, 7800],
      setup: |app, t0| hovered_toast(app, t0, 1000, Some(5000)),
    },
    // 71c : plancher de 500 ms. Survol à 3300 ms (3500 - 3300 = 200 ms < 500 : `remaining` = 500 ms), sortie à 3400 ms : `.leaving` à
    // 3400 + 500 = 3900 ms. 3800 ms : posé, opacité 1,000. 3900 : 1,000. 3950 : 0,901. 4000 : 0,675. 4050 : 0,370. 4100 : 0.
    MotionScene {
      name: "71c-toast-plancher-defaut",
      reference: None,
      times_ms: &[3800, 3900, 3950, 4000, 4050, 4100],
      setup: |app, t0| hovered_toast(app, t0, 3300, Some(3400)),
    },
    // Ancre 72 : accueil, départ propre à chaque spinner circulaire M3 selon son insertion (scènes 72*-defaut, sans capture motion).
    // 72 : chaque spinner a le départ de son insertion (`styles.css:3353-3391`, mêmes courbes que 67a). Les tendances partent quand le chargement commence
    // (`row.innerHTML = <spinner>`, `app.js:25172`, `trending_spinner`) ; les cartes réservées partent du dernier `renderMadeForYou` (`app.js:25391`, mix reçu `25368`,
    // plan `25330`, fin `25386`, `home_spinner`) : « For you » (`25440-25450`) et les « Daily Mix n » (`25500-25510`) naissent du même rendu et partagent leur départ.
    // Arc par temps écoulé `e` du spinner (rotation du SVG = 360 deg * e / 1400, 3,183 deg par unité de cercle, départ de l'arc à 3 h, début -> fin en degrés) :
    //    0 ms : 0 -> 3.   175 ms : 59 -> 99.   350 ms : 146 -> 290.   525 ms : 232 -> 122 (ease 0,871, dash 78,5, offset -30,5).
    //  700 ms : 291 -> 180.   875 ms : 13 -> 225 (dash 90, offset -46,6).   1050 ms : 165 -> 270.   1225 ms et 1399 ms : arc hors du cercle (anneau seul).   1400 ms : comme 0.
    // 72a : cartes recréées 525 ms avant `t0`, tendances parties à `t0`. Valeurs attendues (tendances `e = t`, cartes `e = t + 525`, modulo 1400) :
    //    0 ms : tendances 0 -> 3 ; cartes 232 -> 122.     175 ms : tendances 59 -> 99 ; cartes 291 -> 180.    350 ms : tendances 146 -> 290 ; cartes 13 -> 225.
    //  525 ms : tendances 232 -> 122 ; cartes 165 -> 270.  700 ms : tendances 291 -> 180 ; cartes anneau seul (e = 1225).
    //  875 ms : tendances 13 -> 225 ; cartes 0 -> 3 (la boucle repart).  1050 ms : tendances 165 -> 270 ; cartes 59 -> 99.  1400 ms : tendances 0 -> 3 ; cartes 232 -> 122.
    MotionScene {
      name: "72a-spinners-departs-distincts-defaut",
      reference: None,
      times_ms: &[0, 175, 350, 525, 700, 875, 1050, 1400],
      setup: |app, t0| {
        home_spinners(app);
        app.home_spinner.start = Some(t0 - Duration::from_millis(525));
        app.trending_spinner.start = Some(t0);
      },
    },
    // 72b : cartes recréées à `t0` (mix reçu), tendances parties 700 ms avant. Cartes `e = t`, tendances `e = t + 700`, modulo 1400 :
    //    0 ms : cartes 0 -> 3 ; tendances 291 -> 180.     350 ms : cartes 146 -> 290 ; tendances 165 -> 270.    700 ms : cartes 291 -> 180 ; tendances 0 -> 3.
    // 1050 ms : cartes 165 -> 270 ; tendances 146 -> 290.  1400 ms : cartes 0 -> 3 ; tendances 291 -> 180.
    MotionScene {
      name: "72b-spinners-departs-inverses-defaut",
      reference: None,
      times_ms: &[0, 350, 700, 1050, 1400],
      setup: |app, t0| {
        home_spinners(app);
        app.home_spinner.start = Some(t0);
        app.trending_spinner.start = Some(t0 - Duration::from_millis(700));
      },
    },
    // Ancre 73 : survol des contrôles purgé quand l'élément disparaît, rang de survol stable, toast survolé dès son apparition sous un pointeur immobile (scènes 73*-defaut, sans capture motion).
    // 73a : un survol ne reste pas posé quand l'élément disparaît sous le pointeur. Rangée `.setting-item` « Hide top titlebar » (Réglages,
    // Interface) survolée à t0 (`--hl: 4 %`, `styles.css:7520`) ; la catégorie change sous le pointeur (Thème : la rangée est retirée du DOM, elle
    // perd son état, aucune sortie n'arrive) ; le pointeur sort de la zone ; retour à Interface : la rangée est recréée, le pointeur n'est pas
    // dessus, aucune transition (une `transition` ne part pas d'un élément neuf). Valeurs attendues à chaque instant après le retour (0, 45, 90, 180,
    // 400 ms) : `--hl` 0, fond de repos de la rangée (33, 33, 33) ; sans correctif la rangée restait à `--hl` 4 % : (41, 41, 41).
    MotionScene {
      name: "73a-reglage-survol-purge-defaut",
      reference: None,
      times_ms: &[0, 45, 90, 180, 400],
      setup: |app, t0| {
        settings(app, Category::Interface);
        let row = motion::ControlKey::new(("setting-item", "Hide top titlebar"));
        let messages = still_pointer_on(app, "Hide top titlebar", t0);
        assert_eq!(apply_hover(app, messages, t0), 1, "entrée de la rangée");
        assert!(app.ui.controls.mix(row, t0).hovered);
        app.update(Message::Settings(SettingsMessage::OpenCategory(
          Category::Theme,
        )));
        app.update(Message::Settings(SettingsMessage::OpenCategory(
          Category::Interface,
        )));
        assert!(!app.ui.controls.mix(row, t0).hovered, "survol purgé");
        assert!(!app.ui.controls.mix(row, t0).tracked);
      },
    },
    // 73b : bouton qui apparaît sous un pointeur immobile. Le dialogue de renommage s'ouvre sous le pointeur, posé sur « Save » (`button::filled`,
    // `.btn-pill-primary`) : Chromium recalcule le survol après la mise en page et envoie l'entrée à l'image d'apparition. Départ à t0 : `--hl` 8 %
    // en `.18s ease` (`styles.css:7509`, `!important` : bat les 70 ms de `--transition-hover`, `styles.css:82` et `146`) et rayon en `.22s`. Valeurs
    // attendues comme 65c : avancement de `--hl` 0 à 0 ms, 0,4085 à 45 ms (181, 161, 224), 0,8023 à 90 ms (183, 163, 225), 1 à 180 ms et après (184, 164, 225).
    MotionScene {
      name: "73b-bouton-apparait-survol-defaut",
      reference: None,
      times_ms: &[0, 45, 90, 180, 400],
      setup: |app, t0| {
        rename_dialog(app);
        let messages = still_pointer_on(app, "Save", t0);
        assert_eq!(apply_hover(app, messages, t0), 1, "entrée du bouton");
        let key = motion::button_key("chrome.rs", "Save", 0);
        assert!(app.ui.controls.mix(key, t0).hovered);
      },
    },
    // 73e : rangée qui apparaît sous un pointeur immobile (Réglages, Interface, « Hide top titlebar ») : même départ que 73b, mêmes valeurs que 69a
    // (`--hl` 4 %) : (33, 33, 33) à 0 ms, (36, 36, 36) à 45 ms, (39, 39, 39) à 90 ms, (41, 41, 41) à 180 et 400 ms.
    MotionScene {
      name: "73e-reglage-apparait-survol-defaut",
      reference: None,
      times_ms: &[0, 45, 90, 180, 400],
      setup: |app, t0| {
        settings(app, Category::Interface);
        let messages = still_pointer_on(app, "Hide top titlebar", t0);
        assert_eq!(apply_hover(app, messages, t0), 1, "entrée de la rangée");
      },
    },
    // 73c : toast qui apparaît sous un pointeur immobile (en haut au centre, `app.js:534-545`) : `mouseenter` à la première image où la boîte, en
    // `dropletToastIn`, couvre le pointeur ; la minuterie est en pause. Toast de 500 ms : sans survol `.leaving` partirait à 500 ms (fin à 700 ms) ;
    // le pointeur part à 2000 ms, il reste `max(500 - entrée, 500)` = 500 ms (plancher `app.js:537`) : `.leaving` à 2500 ms. Opacité attendue
    // (sortie `dropletToastOut` 200 ms, `cubic-bezier(0.4, 0, 1, 1)`, `styles.css:4668`, `4673`) : 700, 1500 et 2400 ms : 1,000 (posé, sans transformation) ;
    // 2500 ms : 1,000 ; 2550 : 0,901 ; 2600 : 0,675 ; 2650 : 0,370 ; 2700 et 2800 : 0.
    MotionScene {
      name: "73c-toast-sous-pointeur-defaut",
      reference: None,
      times_ms: &[700, 1500, 2400, 2500, 2550, 2600, 2650, 2700, 2800],
      setup: |app, t0| {
        toast_under_still_pointer(app, t0, 500, Some(2000));
      },
    },
    // 73d : rang de survol stable. Réglages, Intégrations : « Log out » de Spotify et de Google (`manage.rs`, même fichier et même libellé), celui de
    // Last.fm apparaît avant eux quand le compte se connecte (bouton conditionnel). Les panneaux de compte portent leur sel (`motion::keyed`) : la clé de
    // « Log out » de Google ne dépend plus du nombre de « Log out » placés avant. Survolé à t0 : celui de Google ; ceux de Spotify et de Last.fm restent au
    // repos à tout instant. Survol d'un bouton texte (`.btn-pill-primary`-like `--hl` 8 %, `styles.css:7521`) sur le fond de la rangée en `.18s ease`
    // (`styles.css:7509`) : avancement 0 à 0 ms, 0,4085 à 45 ms, 0,8023 à 90 ms, 1 à 180 ms et après.
    MotionScene {
      name: "73d-reglages-rang-stable-defaut-p2",
      reference: None,
      times_ms: &[0, 45, 90, 180, 400],
      setup: |app, t0| {
        settings(app, Category::Integrations);
        app.ui.manage.spotify.connected = true;
        app.ui.manage.google.enabled = true;
        app.ui.manage.google.connected = true;
        app.ui.manage.lastfm.connected = true;
        let key = motion::button_key_with("google", "manage.rs", "Log out", 0);
        app.ui.controls.set(key, true, t0);
      },
    },
    // Ancre 74 : spinners à formes .m3-btn-shape-spinner sur une boucle partagée par l'horloge d'App, reprise au dernier toIdx après un temps mort (scènes 74*-defaut, sans capture motion).
    // Graine du banc : 1. `sequence(1, 11, n)` = [0, 7, 6, 4, 8, 3, 7, 9, ...] : arche (7), capsule (6), fantôme (4), pentagone (8),
    // `Clover4Leaf` (3), arche (7). Le premier pas de la boucle (`app.js:9952`, page inactive) va de la 2e forme du tirage (`toIdx` initial)
    // à la 3e : à l'apparition à `s`, arche vers capsule, puis un pas tous les 1200 ms. Rotation : `epoch = t0`, soit `(t % 3000) / 3000 * 360` degrés.
    // 74a : loader de recherche visible de 0 à 3000 ms, caché de 3000 à 4500 ms (résultats affichés, aucun spinner), réaffiché à 4500 ms.
    // Pas à 0, 1200, 2400 ms ; la boucle s'arrête à 3000 ms sur `fromIdx` = fantôme, `toIdx` = pentagone, `stepStart` = 2400 ms. À 4500 ms,
    // 4500 - 2400 >= 1200 : un pas, `fromIdx` = pentagone (le dernier `toIdx`, pas une reprise depuis la graine), `toIdx` = `Clover4Leaf`.
    // Valeurs attendues (forme de départ vers forme d'arrivée, progression `easeInOutCubic`, rotation) :
    //   0 ms : arche vers capsule, 0, 0 deg | 600 : 0,5, 72 deg | 1200 : capsule vers fantôme, 0, 144 deg
    //   2400 : fantôme vers pentagone, 0, 288 deg | 2999 : 0,50, 360 deg (fin de tour) | 3000 et 4000 : aucun loader
    //   4500 : pentagone vers `Clover4Leaf`, 0, 180 deg | 5100 : 0,5, 252 deg | 5700 : `Clover4Leaf` vers arche, 0, 324 deg
    MotionScene {
      name: "74a-loader-temps-mort-defaut",
      reference: None,
      times_ms: &[0, 600, 1200, 2400, 2999, 3000, 4000, 4500, 5100, 5700],
      setup: |app, t0| {
        let t = MOTION_AT.with(std::cell::Cell::get);
        search_results(app, SearchFilter::All);
        app.epoch = t0;
        app.spinner_seed = 1;
        // Les événements déjà arrivés à t, dans l'ordre : le rendu de t ne dépend pas du chemin.
        app.shape_loop.show(t0);
        if t >= 3000 {
          app.shape_loop.hide(t0 + Duration::from_millis(3000));
        }
        if t >= 4500 {
          app.shape_loop.show(t0 + Duration::from_millis(4500));
        }
        if !(3000..4500).contains(&t) {
          app.ui.search.tracks.clear();
          app.ui.search.loading = true;
        }
      },
    },
    // 74b : loader de recherche affiché dès 0 ms, spinner de lecture du bouton lecture après son anti-rebond (attente à 300 ms, spinner à
    // 800 ms). Une seule boucle : à 800 ms le spinner de lecture montre la même forme et la même rotation que le loader (pas de départ propre).
    //   0 ms : arche vers capsule, 0, 0 deg (loader seul) | 600 : 0,5, 72 deg (loader seul) | 800 : 0,85, 96 deg (les deux)
    //   1200 : capsule vers fantôme, 0, 144 deg | 1800 : 0,5, 216 deg | 2400 : fantôme vers pentagone, 0, 288 deg | 3000 : 0,5, 0 deg
    MotionScene {
      name: "74b-loader-et-lecture-defaut",
      reference: None,
      times_ms: &[0, 600, 800, 1200, 1800, 2400, 3000],
      setup: |app, t0| {
        search_results(app, SearchFilter::All);
        app.ui.search.tracks.clear();
        app.ui.search.loading = true;
        waiting_play(app);
        app.play_wait.since = Some(t0 + Duration::from_millis(300));
        app.epoch = t0;
        app.spinner_seed = 1;
        app.shape_loop.show(t0);
      },
    },
    // 74c : temps mort court. Loader visible de 0 à 3000 ms, caché de 3000 à 3500 ms, réaffiché à 3500 ms. `stepStart` = 2400 ms : à 3500 ms
    // 3500 - 2400 = 1100 < 1200, aucun pas, le pas fantôme vers pentagone continue sans saut et finit à 3600 ms comme sans temps mort.
    //   2700 ms : fantôme vers pentagone, 0,06, 324 deg | 2999 : 0,50, 360 deg | 3000 et 3250 : aucun loader
    //   3500 : 1,00 (0,998), 60 deg | 3599 : 1,00, 72 deg | 3600 : pentagone vers `Clover4Leaf`, 0, 72 deg | 4200 : 0,5, 144 deg
    // (Riff franchit 1200 ms à la première image qui suit 3600 ms : écart d'une image au plus.)
    MotionScene {
      name: "74c-loader-temps-mort-court-defaut",
      reference: None,
      times_ms: &[0, 2700, 2999, 3000, 3250, 3500, 3599, 3600, 4200],
      setup: |app, t0| {
        let t = MOTION_AT.with(std::cell::Cell::get);
        search_results(app, SearchFilter::All);
        app.epoch = t0;
        app.spinner_seed = 1;
        app.shape_loop.show(t0);
        if t >= 3000 {
          app.shape_loop.hide(t0 + Duration::from_millis(3000));
        }
        if t >= 3500 {
          app.shape_loop.show(t0 + Duration::from_millis(3500));
        }
        if !(3000..3500).contains(&t) {
          app.ui.search.tracks.clear();
          app.ui.search.loading = true;
        }
      },
    },
  ]
}

/// Accueil en attente partout : « For you » et les « Daily Mix n » réservés, tendances en chargement ; les départs sont posés par la scène.
fn home_spinners(app: &mut App) {
  go(app, Screen::Home);
  nothing_playing(app);
  app.lyrics.panel_open = true;
  app.ui.home.wave.clear();
  app.ui.home.trending.clear();
  app.ui.home.wave_loading = true;
  app.ui.home.trending_loading = true;
  app.ui.home.mixes_busy = true;
  app.ui.home.mixes_planned = 5;
  app.data.daily_mixes = Some(DailyMixes::default());
}

/// Riff vierge avec le maillage activé : vitesse et flou de `riff_mesh_cfg` posés, `meshFloat` parti à `t0` (apparition du calque).
fn mesh_scene(app: &mut App, t0: Instant, speed: f32, blur: f32) {
  app.settings.theme_accent = "#5247D1".to_string();
  app.settings.theme_brightness = 20;
  app.settings.theme_intensity = 85;
  app.theme = ActiveTheme::from_accent(&app.settings);
  let (body, play) = theme_fade_targets(&app.theme);
  app.theme_fade = ThemeFade::at(body, play);
  nothing_playing(app);
  go(app, Screen::Home);
  let visual = &mut app.settings.visual;
  visual.mesh_gradient = true;
  visual.mesh_brightness = 100;
  visual.mesh.speed = speed;
  visual.mesh.blur = blur;
  // Le calque apparaît à `t0` : `meshFloat` part de 0 % (clé posée sans passer par `update`, qui daterait l'instant réel du banc).
  app.visuals.mesh_key = None;
  app.sync_mesh(t0);
}

/// 12-karaoke : départ de `tvk-in` et de `tvk-out` sur l'axe du banc (ms après le déclencheur), voir les scènes 52.
const TVK_ENTER_MS: u64 = 154;
const TVK_EXIT_MS: u64 = 2426;
/// Départ de `m3ContainerOpen` dans 07-changement-vue : voir le commentaire de la scène 47a.
const VIEW_START_MS: u64 = 67;
/// Départ de la transition du panneau de file d'attente, ms après le déclencheur du banc (scène 46a, recalé sur `f002`).
const PANEL_START_MS: u64 = 82;

/// 10-lecture-pause : départ des deux transitions du bouton sur l'axe des images (voir les commentaires des scènes 50).
const PLAY_CLICK_EARLY: Duration = Duration::from_millis(17);
const PLAY_SECOND_CLICK: Duration = Duration::from_micros(1_220_500);
/// 11-barre-ondulee : phase de l'onde à t = 0, figée par la pause (de 1200 ms), et instant de la reprise (voir les scènes 51).
const WAVE_PHASE_AT_TRIGGER: f32 = 2.08;
const WAVE_PHASE_PAUSED: f32 = 0.0;
const WAVE_RESUME: Duration = Duration::from_millis(2565);

/// Titre de 390 s (durée déduite de l'avance de l'onde dans 11-barre-ondulee) à la position `seconds`.
fn scrubber_at(app: &mut App, seconds: f64) {
  app.playback.duration = Some(Duration::from_secs(390));
  app.playback.position = Duration::from_secs_f64(seconds);
}

/// Départs des transitions de 04-reglages et 05-menu-contextuel : voir les commentaires des scènes 44 et 45.
const SETTINGS_OPEN_MS: u64 = 93;
const SETTINGS_CLOSE_MS: u64 = 1497;
const MENU_OPEN_MS: u64 = 34;
const MENU_CLOSE_MS: u64 = 1130;

/// Page derrière les réglages de 04-reglages : Titres aimés avec le titre en lecture et le panneau de paroles.
fn settings_behind(app: &mut App) {
  liked_capture(app);
  app.lyrics.panel_open = true;
  app.player.current = app.data.favorites.first().cloned();
  app.player.index = None;
  app.playback = Playback {
    position: Duration::from_secs(60),
    duration: Some(Duration::from_secs(337)),
    is_playing: true,
    ..Playback::default()
  };
}

/// Menu de la capture 05-menu-contextuel : Titres aimés, rien en lecture, clic droit sur la première ligne (menu à 606,99 et
/// 221,99 px CSS, soit 573,62 et 209,78 px).
fn context_menu_capture(app: &mut App) {
  liked_capture(app);
  nothing_playing(app);
  app.update(Message::Ui(UiMessage::RowMenu(
    ListId::Favorites,
    0,
    Point::new(573.62, 209.78),
  )));
}

/// Départ de `dropletToastIn` dans 03-toast, 03b : voir les commentaires des scènes 43.
const TOAST_LAG: Duration = Duration::from_millis(100);
const TOAST_SPINNER_LAG: Duration = Duration::from_millis(60);
/// Instant de `.leaving` dans 03-toast.
const TOAST_LEAVING: Duration = Duration::from_millis(1630);

/// Toast `info` de 3500 ms posé à `t0`, survolé à `enter_ms` et, si `leave_ms` est donné, quitté à cet instant. La minuterie de départ
/// arrive à 3500 ms, celle de la reprise à la sortie plus ce qu'il reste : les messages passent par `App::update`, comme en production.
fn hovered_toast(app: &mut App, t0: Instant, enter_ms: u64, leave_ms: Option<u64>) {
  go(app, Screen::Home);
  nothing_playing(app);
  show_toast(app, ToastKind::Info, "Message de test", t0);
  let toast = app.toasts.active.clone().expect("toast affiché");
  let at = |ms: u64| t0 + Duration::from_millis(ms);
  app.update(Message::ToastHover(toast.id, true, at(enter_ms)));
  app.update(Message::ToastTimeout(
    toast.id,
    toast.timer_gen,
    t0 + toast.duration,
  ));
  if let Some(leave_ms) = leave_ms {
    app.update(Message::ToastHover(toast.id, false, at(leave_ms)));
    let resumed = app.toasts.active.clone().expect("toast affiché");
    app.update(Message::ToastTimeout(
      resumed.id,
      resumed.timer_gen,
      at(leave_ms) + resumed.remaining,
    ));
  }
}

/// Taille logique des scènes d'animation (`sizes()` sans `RIFF_SNAPSHOT_QHD`).
const MOTION_SIZE: Size = Size::new(1920.0, 1080.0);

/// Simulateur de la vue de `app`, avec les polices du banc.
fn pointer_simulator(app: &App) -> Simulator<'_, Message> {
  let settings = Settings {
    default_font: fonts::UI,
    fonts: fonts::bytes(),
    ..Settings::default()
  };
  Simulator::with_size(settings, MOTION_SIZE, app.view())
}

/// Messages que les widgets de la vue de `app` publient à l'image `at` quand le pointeur, immobile (aucun `CursorMoved`), est posé en
/// `point` : le survol y est recalculé par la mise en page, comme le fait Chromium. Le simulateur est détruit au retour : la vue est
/// retirée, ses widgets survolés signalent leur disparition (`motion::take_gone`).
fn still_pointer(app: &App, point: Point, at: Instant) -> Vec<Message> {
  let mut sim = pointer_simulator(app);
  sim.point_at(point);
  let _ = sim.simulate([iced::Event::Window(iced::window::Event::RedrawRequested(
    at,
  ))]);
  sim.into_messages().collect()
}

/// Comme [`still_pointer`], le pointeur au centre du texte `label` de la vue.
fn still_pointer_on(app: &App, label: &str, at: Instant) -> Vec<Message> {
  let mut sim = pointer_simulator(app);
  let center = sim
    .find(label)
    .expect("texte dans la vue")
    .visible_bounds()
    .expect("texte visible")
    .center();
  sim.point_at(center);
  let _ = sim.simulate([iced::Event::Window(iced::window::Event::RedrawRequested(
    at,
  ))]);
  sim.into_messages().collect()
}

/// Rejoue sur `app.ui.controls` les messages de survol publiés par la vue, datés `at` (`update` lit l'horloge réelle, le banc non).
/// Rend le nombre d'entrées, de sorties et d'oublis appliqués.
fn apply_hover(app: &mut App, messages: Vec<Message>, at: Instant) -> usize {
  let mut applied = 0;
  for message in messages {
    match message {
      Message::Ui(UiMessage::ControlHover(key, hovered)) => {
        app.ui.controls.set(key, hovered, at);
        applied += 1;
      }
      Message::Ui(UiMessage::ControlGone(key)) => {
        app.ui.controls.forget(key);
        applied += 1;
      }
      _ => {}
    }
  }
  applied
}

/// Notification `info` de `duration_ms` posée à `t0` sous un pointeur immobile en haut au centre. Les images du navigateur sont rejouées
/// toutes les 25 ms depuis `t0` : le simulateur donne le message de survol de la première image où la boîte, transformée par l'entrée,
/// couvre le pointeur. Si `leave_ms` est donné, le pointeur part à cet instant (`mouseleave`). Rend l'instant d'entrée, en ms après `t0`.
fn toast_under_still_pointer(
  app: &mut App,
  t0: Instant,
  duration_ms: u64,
  leave_ms: Option<u64>,
) -> u64 {
  go(app, Screen::Home);
  nothing_playing(app);
  show_toast(app, ToastKind::Info, "Message de test", t0);
  // `showToast(message, type, duration)` : durée donnée par l'appelant.
  if let Some(toast) = &mut app.toasts.active {
    toast.duration = Duration::from_millis(duration_ms);
    toast.remaining = toast.duration;
  }
  let toast = app.toasts.active.clone().expect("toast affiché");
  let at = |ms: u64| t0 + Duration::from_millis(ms);
  let entered = (0..=400_u64)
    .step_by(25)
    .find(|&ms| {
      app.now = at(ms);
      let messages = still_pointer(app, Point::new(960.0, 40.0), at(ms));
      let hovered = messages
        .iter()
        .any(|message| matches!(message, Message::ToastHover(id, true, _) if *id == toast.id));
      for message in messages {
        if matches!(message, Message::ToastHover(..)) {
          app.update(message);
        }
      }
      hovered
    })
    .expect("la boîte couvre le pointeur pendant l'entrée");
  app.update(Message::ToastTimeout(
    toast.id,
    toast.timer_gen,
    at(duration_ms),
  ));
  if let Some(leave_ms) = leave_ms {
    app.update(Message::ToastHover(toast.id, false, at(leave_ms)));
    let resumed = app.toasts.active.clone().expect("toast affiché");
    app.update(Message::ToastTimeout(
      resumed.id,
      resumed.timer_gen,
      at(leave_ms) + resumed.remaining,
    ));
  }
  entered
}

/// Affiche une notification dont l'animation d'entrée part à `start`.
fn show_toast(app: &mut App, kind: ToastKind, message: &str, start: Instant) {
  app.update(Message::ShowToast(message.to_string(), kind, None));
  if let Some(toast) = &mut app.toasts.active {
    toast.shown_at = Some(start);
  }
}

/// Indices des formes de `RiffSpinnerShapes` dans les 11 profils : 4 de `shapes.js`, puis `ghost`, `dumbbell`, `capsule`, `arch`,
/// `pentagon`, `hexagon`, `triangle`.
const COOKIE12: usize = 2;
const GHOST: usize = 4;
const ARCH: usize = 7;
const HEXAGON: usize = 9;

/// Première graine dont le tirage montre ces formes, dans l'ordre, après la forme de départ.
fn seed_for_shapes(wanted: &[usize]) -> u64 {
  (0..)
    .find(|seed| riff_app::visuals::spinner::sequence(*seed, 11, wanted.len() + 1)[1..] == *wanted)
    .expect("une graine existe")
}

/// Chargement du titre en cours : la lecture n'a pas commencé, le bouton prend l'aspect `.playing`.
/// Délai entre le déclencheur de `13-theme-titre` et le départ des transitions de thème (voir l'ancre 53).
const THEME_START: u64 = 150;

/// Thème « Auto (album) » : une première pochette est déjà appliquée au repos, puis une seconde arrive à `t0 + lag_ms`.
fn cover_theme_change(app: &mut App, t0: Instant, lag_ms: u64, tinted_background: bool) {
  go(app, Screen::Liked);
  app.settings.theme_mode = ThemeMode::Auto;
  app.settings.intensity_bg = tinted_background;
  app.settings.theme_intensity = 25;
  app.theme = ActiveTheme::from_cover(Rgb::new(0, 128, 152), &app.settings);
  let (body, play) = theme_fade_targets(&app.theme);
  app.theme_fade = ThemeFade::at(body, play);
  app.settings.theme_intensity = 56;
  app.theme = ActiveTheme::from_cover(Rgb::new(224, 16, 0), &app.settings);
  let (body, play) = theme_fade_targets(&app.theme);
  app
    .theme_fade
    .set(body, play, t0 + Duration::from_millis(lag_ms));
}

fn waiting_play(app: &mut App) {
  app.playback.is_playing = false;
  app.player.loading = true;
}

/// Facteur appliqué aux `clipCssPx` de `timing.json` pour obtenir des pixels écran. Mesuré : le toast de `03-toast` est centré sur
/// x = 960 et la boîte de recherche y mesure 534 px, soit les 529 px écran de `layout.json` (pas 529 / 0,945). Ces clips sont donc
/// déjà en pixels écran (1920x1080) malgré leur nom. Les `clipScreenPx` valent 1 par définition.
const CSS_CLIP_ZOOM: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Clip {
  x: f64,
  y: f64,
  w: f64,
  h: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct RefFrame {
  file: String,
  send_ms: f64,
  recv_ms: f64,
}

/// Une capture de `ui-reference/motion` : dossier, zone, images et instant du déclencheur (horloge Node de `sendMs`/`recvMs`).
#[derive(Debug, Clone, PartialEq)]
struct Reference {
  dir: PathBuf,
  clip: Clip,
  zoom: f64,
  /// `clipCssPx.scale` : réduction des images de référence (14-mini : 0,5) ; 1 sans cette clé.
  scale: f64,
  frames: Vec<RefFrame>,
  anchor_ms: f64,
}

/// Instant d'une image de référence relativement au déclencheur : milieu de `[sendMs, recvMs]` (la capture est prise entre
/// l'envoi et la réception de `Page.captureScreenshot`) moins l'instant du déclencheur sur la même horloge Node.
fn frame_time_ms(frame: &RefFrame, anchor_ms: f64) -> f64 {
  f64::midpoint(frame.send_ms, frame.recv_ms) - anchor_ms
}

/// Indice de l'image dont l'instant est le plus proche de `t_ms` (égalité : la plus ancienne).
fn nearest_frame(frames: &[RefFrame], anchor_ms: f64, t_ms: f64) -> Option<usize> {
  let mut best: Option<(usize, f64)> = None;
  for (i, frame) in frames.iter().enumerate() {
    let gap = (frame_time_ms(frame, anchor_ms) - t_ms).abs();
    if best.is_none_or(|(_, g)| gap < g) {
      best = Some((i, gap));
    }
  }
  best.map(|(i, _)| i)
}

/// Zone en pixels écran `(x, y, largeur, hauteur)` d'un clip multiplié par `zoom`.
fn clip_to_screen(clip: Clip, zoom: f64) -> (u32, u32, u32, u32) {
  let px = |v: f64| (v * zoom).round().max(0.0) as u32;
  (px(clip.x), px(clip.y), px(clip.w).max(1), px(clip.h).max(1))
}

/// Instant du déclencheur : `actionNodeMs` s'il existe. Sinon (`01-splash-demarrage`, page rechargée, aucun clic) : première
/// ligne de `samples.json` où le logo porte une transformation (`logoTransform` différent de `none`, début du zoom 1,22 -> 1),
/// convertie sur l'horloge Node par `dt - T0epochMs`. Elle est postérieure d'au plus une image (~130 ms ici) au vrai départ.
fn trigger_ms(timing: &Value, samples: Option<&Value>) -> Option<f64> {
  if let Some(action) = timing["actionNodeMs"].as_f64() {
    return Some(action);
  }
  let epoch = timing["T0epochMs"].as_f64()?;
  samples?["rows"]
    .as_array()?
    .iter()
    .find(|row| row["logoTransform"].as_str().is_some_and(|t| t != "none"))
    .and_then(|row| row["dt"].as_f64())
    .map(|dt| dt - epoch)
}

fn parse_reference(dir: &Path, timing: &Value, samples: Option<&Value>) -> Option<Reference> {
  let (key, zoom) = if timing["clipScreenPx"].is_object() {
    ("clipScreenPx", 1.0)
  } else {
    ("clipCssPx", CSS_CLIP_ZOOM)
  };
  let clip = &timing[key];
  let scale = clip["scale"].as_f64().unwrap_or(1.0);
  // `"viewport"` (07-changement-vue) : fenêtre entière, les images de référence sont réduites de moitié.
  let clip = if clip.as_str() == Some("viewport") {
    Clip {
      x: 0.0,
      y: 0.0,
      w: 1920.0,
      h: 1080.0,
    }
  } else {
    Clip {
      x: clip["x"].as_f64()?,
      y: clip["y"].as_f64()?,
      w: clip["w"].as_f64()?,
      h: clip["h"].as_f64()?,
    }
  };
  let frames = timing["frames"]
    .as_array()?
    .iter()
    .map(|frame| {
      Some(RefFrame {
        file: frame["file"].as_str()?.to_string(),
        send_ms: frame["sendMs"].as_f64()?,
        recv_ms: frame["recvMs"].as_f64()?,
      })
    })
    .collect::<Option<Vec<_>>>()?;
  Some(Reference {
    dir: dir.to_path_buf(),
    clip,
    zoom,
    scale,
    frames,
    anchor_ms: trigger_ms(timing, samples)?,
  })
}

/// Dossier `ui-reference/motion` : premier ancêtre de `CARGO_MANIFEST_DIR` qui le contient.
fn find_motion_root() -> Option<PathBuf> {
  Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .map(|dir| dir.join("ui-reference").join("motion"))
    .find(|dir| dir.is_dir())
}

fn read_json(path: &Path) -> Option<Value> {
  serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn load_reference(name: &str) -> Option<Reference> {
  let dir = find_motion_root()?.join(name);
  let timing = read_json(&dir.join("timing.json"))?;
  let samples = read_json(&dir.join("samples.json"));
  parse_reference(&dir, &timing, samples.as_ref())
}

fn only_matches(name: &str) -> bool {
  std::env::var("RIFF_SNAPSHOT_ONLY").map_or(true, |only| name.starts_with(&only))
}

/// `<scène>-reference.txt` : instant et fichier de chaque image de référence.
fn reference_listing(reference: &Reference) -> String {
  let mut out = String::from("t (ms après le déclencheur) -> image\n");
  for frame in &reference.frames {
    let t = frame_time_ms(frame, reference.anchor_ms);
    out += &format!("{t:8.1} -> {}\n", frame.file);
  }
  out
}

/// `<nom>-cote.png` : image de référence la plus proche de `t_ms` à gauche, même zone du rendu `<nom>.png` à droite.
fn write_side_by_side(reference: &Reference, name: &str, t_ms: u64) {
  let Some(index) = nearest_frame(&reference.frames, reference.anchor_ms, t_ms as f64) else {
    return;
  };
  let frame = &reference.frames[index];
  let left = image::open(reference.dir.join(&frame.file))
    .expect("image de référence")
    .into_rgba8();
  let rendered = image::open(out_dir().join(format!("{name}.png")))
    .expect("rendu riff-rs")
    .into_rgba8();
  let (x, y, w, h) = clip_to_screen(reference.clip, reference.zoom);
  let x = x.min(rendered.width().saturating_sub(1));
  let y = y.min(rendered.height().saturating_sub(1));
  let w = w.min(rendered.width() - x);
  let h = h.min(rendered.height() - y);
  let mut right = image::imageops::crop_imm(&rendered, x, y, w, h).to_image();
  // Référence réduite (`clipCssPx.scale`, 14-mini) : l'écran entier est capturé, la fenêtre compacte n'en occupe que le coin haut gauche.
  // Le reste de la zone prend le fond uni de la référence (son pixel en bas à droite), puis la zone entière est réduite par `scale`.
  if reference.scale != 1.0 {
    let (_, _, zone_w, zone_h) = clip_to_screen(reference.clip, reference.zoom);
    if (zone_w, zone_h) != right.dimensions() {
      let fill = *left.get_pixel(left.width() - 1, left.height() - 1);
      let mut zone = image::RgbaImage::from_pixel(zone_w, zone_h, fill);
      image::imageops::replace(&mut zone, &right, 0, 0);
      right = zone;
    }
    let (target_w, target_h) = (
      (f64::from(zone_w) * reference.scale).round() as u32,
      (f64::from(zone_h) * reference.scale).round() as u32,
    );
    if right.dimensions() != (target_w, target_h) {
      right = image::imageops::resize(&right, target_w, target_h, FilterType::Lanczos3);
    }
  }
  if right.dimensions() != left.dimensions() {
    right = image::imageops::resize(&right, left.width(), left.height(), FilterType::Lanczos3);
  }
  let mut board = image::RgbaImage::new(left.width() * 2, left.height());
  image::imageops::replace(&mut board, &left, 0, 0);
  image::imageops::replace(&mut board, &right, i64::from(left.width()), 0);
  let target = out_dir().join(format!("{name}-cote.png"));
  board.save(&target).expect("écriture de la planche");
  println!("{}", target.display());
}

thread_local! {
  /// Les rendus en cours sont ceux des scènes d'animation (`motion_scenes`).
  static MOTION_RENDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
  /// Instant `t` (ms après le déclencheur) de la scène d'animation en cours de préparation, pour les scènes qui rejouent des images.
  static MOTION_AT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Images de 1000/240 ms écoulées à `t_ms` (les scènes 59 rejouent la boucle du halo à 240 Hz).
fn halo_frames(t_ms: u64) -> u64 {
  t_ms * 240 / 1000
}

/// Rejoue le halo : `warmup` images de lecture avant t0, puis `halo_frames(t)` images à partir de t0, en lecture (`playing`) ou en pause
/// (état posé aussi pour la barre). `hovered(frame)` : souris sur la pochette à l'image `frame` (1 = première image après t0).
/// Écrit les valeurs obtenues sur la sortie standard, pour les comparer à la boucle de Riff.
fn halo_replay(app: &mut App, warmup: u64, playing: bool, hovered: impl Fn(u64) -> bool) {
  const FRAME_MS: f64 = 1000.0 / 240.0;
  let t = MOTION_AT.with(std::cell::Cell::get);
  for _ in 0..warmup {
    app.visuals.shape.advance(FRAME_MS, true);
  }
  let frames = halo_frames(t);
  // Les événements de t0 (pause, entrée de la souris) précèdent la première image.
  for frame in 1..=frames {
    app.visuals.shape.hovered = hovered(frame);
    app.visuals.shape.advance(FRAME_MS, playing);
  }
  app.playback.is_playing = playing;
  let shape = &app.visuals.shape;
  println!(
    "halo t={t} images={frames} survol={:.5} vitesse={:.5} rotation={:.5} phase={:.5} forme={}>{} fondu={:.1}",
    shape.hover_progress,
    shape.rotation_rate,
    shape.rotation,
    shape.phase,
    shape.from,
    shape.to,
    shape.morph_ms
  );
}

/// Taille de la fenêtre à l'instant `t_ms` des scènes qui la font changer (54*, mode mini) ; `None` : celle de la scène (1920x1080 par défaut).
/// `LeaveMiniWindow` et `EnterMiniWindow` ne sont pas exécutés par le banc : la nouvelle taille arrive ici, comme `Event::Resized`.
fn motion_window(name: &str, t_ms: u64) -> Option<Size> {
  let full = Size::new(1920.0, 1080.0);
  let mini = riff_app::window::MINI_SIZE;
  match name {
    // `samples.json` de 14-mini : le passage tombe entre 54,2 et 58,4 ms de la page.
    "54a-mini-entree" => Some(if t_ms < 54 { full } else { mini }),
    "54b-mini-sortie-defaut" => Some(if t_ms < 50 { mini } else { full }),
    "54c-mini-elargie-defaut" => Some(Size::new(
      if t_ms == 0 { 600.0 } else { 480.0 },
      mini.height,
    )),
    _ => None,
  }
}

fn write_motion_snapshots(size: Size) {
  MOTION_RENDER.with(|motion| motion.set(true));
  for scene in motion_scenes() {
    if !only_matches(scene.name) {
      continue;
    }
    let reference = scene.reference.and_then(load_reference);
    if let Some(reference) = &reference {
      let target = out_dir().join(format!("{}-reference.txt", scene.name));
      fs::write(&target, reference_listing(reference)).expect("écriture de la liste");
    }
    let t0 = Instant::now();
    // Fenêtre compacte (59e) : la capture est celle de la fenêtre de 380x160.
    let default_size = if scene.name.starts_with("59e") {
      riff_app::window::MINI_SIZE
    } else {
      size
    };
    for &t in scene.times_ms {
      // Taille de la fenêtre à l'instant t (scènes 54*), sinon celle de la scène.
      let size = motion_window(scene.name, t).unwrap_or(default_size);
      let mut app = base_app();
      resize(&mut app, size);
      MOTION_AT.with(|at| at.set(t));
      // Widgets survolés de la scène précédente, détruits avec leur simulateur : leurs clés n'appartiennent pas à cet `App`.
      let _ = motion::take_gone();
      (scene.setup)(&mut app, t0);
      // Seules les scènes 50 et 51 posent l'état de la barre de lecture ; pour les autres, il vient d'`update` et daterait un
      // changement à l'instant réel du banc (phase de l'onde non reproductible).
      if !scene.name.starts_with("50") && !scene.name.starts_with("51") {
        app.playbar_motion = Default::default();
      }
      app.now = t0 + Duration::from_millis(t);
      let name = format!("{}-t{t:03}", scene.name);
      if scene.name.starts_with("55") {
        render_lyrics_frame(&mut app, &name, size);
      } else {
        render_scene(&mut app, &name, size);
      }
      if let Some(reference) = &reference {
        write_side_by_side(reference, &name, t);
      }
    }
  }
  MOTION_RENDER.with(|motion| motion.set(false));
  // Les scènes 68* posent la réduction et le mode performance : retour aux réglages d'un Riff vierge.
  motion::apply_settings(Some(false), false);
}

#[cfg(test)]
mod motion_tests {
  use super::*;

  fn frame(send_ms: f64, recv_ms: f64) -> RefFrame {
    RefFrame {
      file: "f.png".to_string(),
      send_ms,
      recv_ms,
    }
  }

  #[test]
  fn frame_time_is_the_midpoint_minus_the_trigger() {
    assert!((frame_time_ms(&frame(313.0, 401.0), 156.0) - 201.0).abs() < 1e-9);
    assert!((frame_time_ms(&frame(0.0, 63.0), 156.0) + 124.5).abs() < 1e-9);
  }

  #[test]
  fn nearest_frame_picks_the_closest_and_the_older_on_a_tie() {
    // Instants relatifs à un déclencheur à 100 : -50, 50, 150.
    let frames = [frame(0.0, 100.0), frame(100.0, 200.0), frame(200.0, 300.0)];
    assert_eq!(nearest_frame(&frames, 100.0, 0.0), Some(0));
    assert_eq!(nearest_frame(&frames, 100.0, 60.0), Some(1));
    assert_eq!(nearest_frame(&frames, 100.0, 100.0), Some(1));
    assert_eq!(nearest_frame(&frames, 100.0, 5000.0), Some(2));
    assert_eq!(nearest_frame(&[], 0.0, 0.0), None);
  }

  #[test]
  fn clip_is_converted_to_screen_pixels() {
    let clip = Clip {
      x: 560.0,
      y: 0.0,
      w: 800.0,
      h: 140.0,
    };
    assert_eq!(clip_to_screen(clip, 1.0), (560, 0, 800, 140));
    assert_eq!(clip_to_screen(clip, 0.945), (529, 0, 756, 132));
    assert_eq!(clip_to_screen(clip, CSS_CLIP_ZOOM), (560, 0, 800, 140));
  }

  #[test]
  fn the_reference_scale_defaults_to_one() {
    let timing = serde_json::json!({
      "clipCssPx": { "x": 0, "y": 0, "w": 1920, "h": 1080, "scale": 0.5 },
      "actionNodeMs": 0,
      "frames": [{ "file": "f000.png", "sendMs": 6, "recvMs": 120 }],
    });
    let dir = Path::new("x");
    assert!((parse_reference(dir, &timing, None).expect("14-mini").scale - 0.5).abs() < 1e-9);
    let timing = serde_json::json!({
      "clipCssPx": { "x": 0, "y": 0, "w": 800, "h": 140 },
      "actionNodeMs": 0,
      "frames": [],
    });
    assert!(
      (parse_reference(dir, &timing, None)
        .expect("sans scale")
        .scale
        - 1.0)
        .abs()
        < 1e-9
    );
  }

  #[test]
  fn the_mini_scenes_change_the_window_size_at_their_instants() {
    let size = |name, t| motion_window(name, t).map(|s| (s.width, s.height));
    assert_eq!(size("54a-mini-entree", 50), Some((1920.0, 1080.0)));
    assert_eq!(size("54a-mini-entree", 54), Some((380.0, 160.0)));
    assert_eq!(size("54b-mini-sortie-defaut", 40), Some((380.0, 160.0)));
    assert_eq!(size("54b-mini-sortie-defaut", 100), Some((1920.0, 1080.0)));
    assert_eq!(size("54c-mini-elargie-defaut", 0), Some((600.0, 160.0)));
    assert_eq!(size("54c-mini-elargie-defaut", 100), Some((480.0, 160.0)));
    assert_eq!(size("53c-theme-defaut", 0), None);
  }

  #[test]
  fn trigger_comes_from_the_action_or_from_the_first_animated_sample() {
    let timing = serde_json::json!({ "actionNodeMs": 156, "T0epochMs": 1000 });
    assert_eq!(trigger_ms(&timing, None), Some(156.0));
    let timing = serde_json::json!({ "T0epochMs": 1000 });
    let samples = serde_json::json!({ "rows": [
      { "dt": 1100, "logoTransform": "none" },
      { "dt": 1576, "logoTransform": "matrix(1.22, 0, 0, 1.22, 0, 0)" },
    ] });
    assert_eq!(trigger_ms(&timing, Some(&samples)), Some(576.0));
    assert_eq!(trigger_ms(&timing, None), None);
  }

  #[test]
  fn the_six_anchor_captures_load_when_the_reference_exists() {
    let Some(root) = find_motion_root() else {
      return;
    };
    for name in [
      "01-splash-demarrage",
      "01b-splash-rejoue",
      "03-toast",
      "03b-toast-spinner",
      "16-spinner-lecture",
      "17-recherche-loader",
    ] {
      let reference = load_reference(name).unwrap_or_else(|| panic!("{name} se charge"));
      assert!(!reference.frames.is_empty(), "{name}");
      assert!(reference.anchor_ms.is_finite(), "{name}");
      let first = image::open(root.join(name).join(&reference.frames[0].file)).expect("image");
      assert_eq!(
        (f64::from(first.width()), f64::from(first.height())),
        (reference.clip.w, reference.clip.h),
        "{name} : taille de l'image et du clip"
      );
      println!(
        "{name}: {} images {}x{}, déclencheur {:.1} ms, première image à {:.1} ms",
        reference.frames.len(),
        first.width(),
        first.height(),
        reference.anchor_ms,
        frame_time_ms(&reference.frames[0], reference.anchor_ms)
      );
    }
  }
}
