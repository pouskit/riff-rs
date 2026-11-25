//! Reprise de la lecture au lancement : file et titre dans `playback-state.json`, position dans `playback-position.json`.
//!
//! Les deux fichiers vivent dans le dossier de données (`RIFF_DATA_DIR`, réglage `dataDir`). La file ne s'écrit que
//! quand elle change ; la position, petite, au plus toutes les [`POSITION_EVERY`] pendant la lecture, à la pause, au
//! déplacement, à la fermeture et à la fin de session de Windows. Jamais pendant un jam : la file du jam n'est pas
//! celle de l'utilisateur.

use crate::app::{App, Effect, Message};
use crate::player::{NextStep, PlayerState};
use crate::ui::PlayerMessage;
use crate::user_data::Track;
use riff_audio::Event as AudioEvent;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

/// Version des deux fichiers : un autre numéro n'est pas lu.
pub const FORMAT: u32 = 1;
pub const STATE_FILE: &str = "playback-state.json";
pub const POSITION_FILE: &str = "playback-position.json";
/// Titres gardés autour du titre en cours.
pub const MAX_QUEUE: usize = 1000;
/// Écart de position qui déclenche une écriture pendant la lecture.
pub const POSITION_EVERY: Duration = Duration::from_secs(10);
/// Attentes entre les essais de résolution du titre repris (réseau pas encore prêt) : environ deux minutes au total.
pub const RETRY_DELAYS: [Duration; 6] = [
  Duration::from_secs(3),
  Duration::from_secs(5),
  Duration::from_secs(10),
  Duration::from_secs(15),
  Duration::from_secs(30),
  Duration::from_secs(60),
];
/// Relance sans fenêtre : si l'ouverture de la fenêtre n'a pas lancé la reprise après ce délai, elle part seule.
pub const START_FALLBACK: Duration = Duration::from_secs(3);
/// Clés libres de [`Track`] utiles à la relecture et à l'affichage ; les autres ne sont pas gardées.
const KEPT_EXTRA: [&str; 11] = [
  "filePath",
  "localThumbnail",
  "image",
  "artworkUrl",
  "isExplicit",
  "explicit",
  "explicit_lyrics",
  "badges",
  "no",
  "durationMs",
  "subtitle",
];

/// Contenu de `playback-state.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeState {
  pub version: u32,
  #[serde(default)]
  pub queue: Vec<Track>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub index: Option<usize>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub current: Option<Track>,
  #[serde(default)]
  pub shuffle: bool,
  /// [`PlayerState::scope`].
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  #[serde(default)]
  pub radio: bool,
  #[serde(default)]
  pub my_wave: bool,
}

/// Contenu de `playback-position.json` : la position ne vaut que pour ce titre (identifiant canonique).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePosition {
  pub version: u32,
  pub track: String,
  pub position_ms: u64,
}

impl ResumePosition {
  #[must_use]
  pub fn new(track: String, at: Duration) -> Self {
    Self {
      version: FORMAT,
      track,
      position_ms: u64::try_from(at.as_millis()).unwrap_or(u64::MAX),
    }
  }
}

/// Titre réduit aux champs utiles à la relecture : ni dates, ni clés libres inconnues.
fn slim(track: &Track) -> Track {
  Track {
    id: track.id.clone(),
    title: track.title.clone(),
    artist: track.artist.clone(),
    album: track.album.clone(),
    duration: track.duration,
    thumbnail: track.thumbnail.clone(),
    cover: track.cover.clone(),
    url: track.url.clone(),
    platform: track.platform.clone(),
    path: track.path.clone(),
    search_query: track.search_query.clone(),
    added_at: None,
    played_at: None,
    is_cached: track.is_cached,
    extra: track
      .extra
      .iter()
      .filter(|(key, _)| KEPT_EXTRA.contains(&key.as_str()))
      .map(|(key, value)| (key.clone(), value.clone()))
      .collect::<Map<String, Value>>(),
  }
}

/// Bornes de la partie de la file enregistrée : au plus [`MAX_QUEUE`] titres autour du titre en cours.
fn window(player: &PlayerState) -> (usize, usize) {
  let len = player.queue.len();
  let start = player
    .index
    .map_or(0, |index| index.saturating_sub(MAX_QUEUE / 4))
    .min(len.saturating_sub(MAX_QUEUE));
  (start, (start + MAX_QUEUE).min(len))
}

/// État à enregistrer : au plus [`MAX_QUEUE`] titres, pris autour du titre en cours.
#[must_use]
pub fn capture(player: &PlayerState) -> ResumeState {
  let (start, end) = window(player);
  ResumeState {
    version: FORMAT,
    queue: player.queue[start..end].iter().map(slim).collect(),
    index: player
      .index
      .filter(|index| (start..end).contains(index))
      .map(|index| index - start),
    current: player.current.as_ref().map(slim),
    shuffle: player.shuffle,
    scope: player.scope.clone(),
    radio: player.radio,
    my_wave: player.my_wave,
  }
}

fn hash_track(track: &Track, hasher: &mut impl Hasher) {
  track.id.hash(hasher);
  track.title.hash(hasher);
  track.artist.hash(hasher);
  track.url.hash(hasher);
  track.platform.hash(hasher);
  track.path.hash(hasher);
}

/// Empreinte de ce que [`capture`] enregistre (même fenêtre, donc coût borné) : une écriture seulement quand elle
/// change. Rien à reprendre : zéro.
fn signature(player: &PlayerState) -> u64 {
  if player.queue.is_empty() && player.current.is_none() {
    return 0;
  }
  let mut hasher = std::collections::hash_map::DefaultHasher::new();
  player.index.hash(&mut hasher);
  player.shuffle.hash(&mut hasher);
  player.scope.hash(&mut hasher);
  player.radio.hash(&mut hasher);
  player.my_wave.hash(&mut hasher);
  player.current.is_some().hash(&mut hasher);
  if let Some(current) = &player.current {
    hash_track(current, &mut hasher);
  }
  let (start, end) = window(player);
  start.hash(&mut hasher);
  player.queue[start..end].len().hash(&mut hasher);
  for track in &player.queue[start..end] {
    hash_track(track, &mut hasher);
  }
  hasher.finish()
}

/// État lu au lancement.
#[derive(Debug, Clone, PartialEq)]
pub struct Restore {
  pub state: ResumeState,
  /// Position enregistrée pour le titre en cours ; zéro si elle est absente ou d'un autre titre.
  pub position: Duration,
  /// Le titre en cours est un fichier local absent.
  pub missing: bool,
}

/// Fichier de version [`FORMAT`]. Absent, illisible ou d'un autre format : `None`, journalisé sauf s'il est absent.
fn read_versioned<T: DeserializeOwned>(path: &Path) -> Option<T> {
  let name = path
    .file_name()
    .and_then(|name| name.to_str())
    .unwrap_or("");
  let raw = match fs::read(path) {
    Ok(raw) => raw,
    Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
    Err(err) => {
      tracing::warn!(error = %err, fichier = name, "lecture de l'état de lecture");
      return None;
    }
  };
  let value: Value = serde_json::from_slice(&raw)
    .inspect_err(|err| tracing::warn!(error = %err, fichier = name, "état de lecture illisible"))
    .ok()?;
  let version = value.get("version").and_then(Value::as_u64);
  if version != Some(u64::from(FORMAT)) {
    tracing::warn!(
      ?version,
      fichier = name,
      "état de lecture d'un format inconnu"
    );
    return None;
  }
  serde_json::from_value(value)
    .inspect_err(|err| tracing::warn!(error = %err, fichier = name, "état de lecture illisible"))
    .ok()
}

/// Lit l'état enregistré dans `dir`. `platform` : plateforme par défaut des identifiants canoniques.
#[must_use]
pub fn load(dir: &Path, platform: &str) -> Option<Restore> {
  let mut state: ResumeState = read_versioned(&dir.join(STATE_FILE))?;
  state.queue.truncate(MAX_QUEUE);
  state.index = state.index.filter(|&index| index < state.queue.len());
  let position = state
    .current
    .as_ref()
    .and_then(|current| {
      let saved: ResumePosition = read_versioned(&dir.join(POSITION_FILE))?;
      (saved.track == current.canonical_id(platform))
        .then(|| Duration::from_millis(saved.position_ms))
    })
    .unwrap_or_default();
  let missing = state.current.as_ref().is_some_and(|track| {
    track.is_local() && crate::stream::local_path(track).is_none_or(|path| !path.is_file())
  });
  Some(Restore {
    state,
    position,
    missing,
  })
}

/// Écrit `value` sans jamais laisser un fichier à moitié écrit.
fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
  static SEQ: AtomicU64 = AtomicU64::new(0);
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)?;
  }
  let seq = SEQ.fetch_add(1, Ordering::Relaxed);
  let tmp = path.with_extension(format!("json.{seq}.tmp"));
  let bytes = serde_json::to_vec(value)?;
  let mut file = fs::File::create(&tmp)?;
  file.write_all(&bytes)?;
  file.sync_all()?;
  drop(file);
  let res = fs::rename(&tmp, path);
  if res.is_err() {
    let _ = fs::remove_file(&tmp);
  }
  res
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Default)]
struct Latest {
  state: Option<Arc<ResumeState>>,
  state_generation: u64,
  state_written: u64,
  position: Option<ResumePosition>,
  position_written: Option<ResumePosition>,
}

/// Dernier état voulu et dernier état écrit : l'écriture de fin de session de Windows et celle de la fermeture
/// n'écrivent que ce qui a changé.
#[derive(Debug)]
pub struct Keeper {
  /// `None` : le dossier de données courant, relu à chaque écriture.
  dir: Option<PathBuf>,
  latest: Mutex<Latest>,
  /// Une écriture à la fois : la fin de session peut croiser une écriture en cours.
  io: Mutex<()>,
}

impl Keeper {
  #[must_use]
  pub fn new(dir: Option<PathBuf>) -> Self {
    Self {
      dir,
      latest: Mutex::default(),
      io: Mutex::new(()),
    }
  }

  pub fn set_state(&self, state: ResumeState) {
    let mut latest = lock(&self.latest);
    latest.state = Some(Arc::new(state));
    latest.state_generation += 1;
  }

  pub fn set_position(&self, position: ResumePosition) {
    lock(&self.latest).position = Some(position);
  }

  /// Position de lecture du titre déjà connu, sans écriture.
  pub fn note_position(&self, at: Duration) {
    if let Some(position) = lock(&self.latest).position.as_mut() {
      position.position_ms = u64::try_from(at.as_millis()).unwrap_or(u64::MAX);
    }
  }

  /// Écrit ce qui a changé depuis la dernière écriture.
  ///
  /// # Errors
  /// Erreur du système de fichiers.
  pub fn write(&self) -> io::Result<()> {
    let _io = lock(&self.io);
    let (state, generation, position) = {
      let latest = lock(&self.latest);
      let state = latest
        .state
        .clone()
        .filter(|_| latest.state_generation != latest.state_written);
      let position = latest
        .position
        .clone()
        .filter(|position| latest.position_written.as_ref() != Some(position));
      (state, latest.state_generation, position)
    };
    if state.is_none() && position.is_none() {
      return Ok(());
    }
    let dir = self
      .dir
      .clone()
      .unwrap_or_else(riff_core::paths::resolve_data_dir);
    if let Some(state) = state {
      write_json(&dir.join(STATE_FILE), &*state)?;
      lock(&self.latest).state_written = generation;
    }
    if let Some(position) = position {
      write_json(&dir.join(POSITION_FILE), &position)?;
      lock(&self.latest).position_written = Some(position);
    }
    Ok(())
  }
}

static KEEPER: OnceLock<Keeper> = OnceLock::new();

/// Active l'enregistrement pour ce processus (jamais pendant une mesure).
pub fn install() {
  let _ = KEEPER.set(Keeper::new(None));
}

#[must_use]
pub fn installed() -> bool {
  KEEPER.get().is_some()
}

pub fn set_state(state: ResumeState) {
  if let Some(keeper) = KEEPER.get() {
    keeper.set_state(state);
  }
}

pub fn set_position(position: ResumePosition) {
  if let Some(keeper) = KEEPER.get() {
    keeper.set_position(position);
  }
}

pub fn note_position(at: Duration) {
  if let Some(keeper) = KEEPER.get() {
    keeper.note_position(at);
  }
}

/// Écriture synchrone : hors du fil d'interface, sauf à la fermeture et à la fin de session.
pub fn flush() {
  if let Some(keeper) = KEEPER.get()
    && let Err(err) = keeper.write()
  {
    tracing::warn!(error = %err, "écriture de l'état de lecture");
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
  at: Duration,
  attempts: usize,
}

/// Suivi de l'état enregistré, côté [`App`].
#[derive(Debug, Clone)]
pub struct ResumeTracker {
  /// Empreinte de la dernière file enregistrée ; au départ, celle d'un lecteur vide : rien n'est écrit sans lecture.
  saved: Option<u64>,
  /// Jeton du lecteur au dernier calcul de l'empreinte.
  token: u64,
  /// Jeton et position de la dernière position enregistrée.
  position: Option<(u64, Duration)>,
  was_playing: bool,
  /// Position où relancer le titre restauré, tant que la reprise n'est pas partie.
  armed: Option<Duration>,
  missing: bool,
  /// Reprise partie, en attente du chargement du flux.
  pending: Option<Pending>,
  /// Le démarrage retenu jusqu'à l'ouverture de la fenêtre (proxy, périphérique, reprise) est parti.
  pub startup_done: bool,
}

impl Default for ResumeTracker {
  fn default() -> Self {
    Self {
      saved: Some(0),
      token: 0,
      position: None,
      was_playing: false,
      armed: None,
      missing: false,
      pending: None,
      startup_done: false,
    }
  }
}

/// Ce qu'un message demande au suivi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watch {
  /// Message fréquent qui ne change la file qu'avec un nouveau jeton : l'empreinte n'est recalculée qu'alors.
  light: bool,
  /// Position du moteur.
  tick: bool,
}

impl Watch {
  #[must_use]
  pub fn of(message: &Message) -> Self {
    Self {
      light: matches!(
        message,
        Message::Audio(_) | Message::Player(PlayerMessage::Level(_) | PlayerMessage::NormalizeTick)
      ),
      tick: matches!(message, Message::Audio(AudioEvent::Position(_))),
    }
  }
}

impl App {
  /// Rétablit la file, la place, l'aléatoire et la portée enregistrés. `start` arme la relance du titre.
  pub fn restore_playback(&mut self, saved: Restore, start: bool) {
    let Restore {
      state,
      position,
      missing,
    } = saved;
    self.player.restore(state.queue, state.index, state.current);
    self.player.shuffle = state.shuffle;
    self.player.scope = state.scope;
    self.player.radio = state.radio;
    self.player.my_wave = state.my_wave;
    self.resume.saved = Some(signature(&self.player));
    self.resume.token = self.player.token;
    self.resume.missing = missing;
    self.resume.armed = (start && self.player.current.is_some()).then_some(position);
  }

  /// Prend l'état du lecteur comme déjà enregistré (lecteur préparé sans passer par `update`).
  pub fn mark_resume_saved(&mut self) {
    self.resume.saved = Some(signature(&self.player));
    self.resume.token = self.player.token;
  }

  /// L'état peut suivre la lecture : hors jam, hors écran d'import, titre chargé.
  #[must_use]
  pub fn resume_live(&self) -> bool {
    self.jam.is_none()
      && !self.import_waiting()
      && !self.player.loading
      && self.player.current.is_some()
  }

  /// Relance le titre restauré, une seule fois : à l'ouverture de la fenêtre, ou seule si la fenêtre ne s'ouvre pas.
  pub(crate) fn resume_start(&mut self) -> Vec<Effect> {
    let Some(at) = self.resume.armed.take() else {
      return Vec::new();
    };
    let Some(track) = self.player.current.clone() else {
      return Vec::new();
    };
    if self.jam.is_some() || self.import_waiting() {
      return Vec::new();
    }
    let mut effects = vec![Effect::AudioMix(self.data.volume, self.data.muted)];
    let missing = self.resume.missing;
    if missing || self.data.is_blocked(&track) {
      // Comme « suivant » : le titre bloqué ou le fichier absent est sauté.
      tracing::info!(missing, "reprise : titre indisponible, passage au suivant");
      let platform = self.platform();
      let data = &self.data;
      let unusable = |candidate: &Track| {
        data.is_blocked(candidate) || (missing && candidate.is_same(&track, &platform))
      };
      match self.player.advance(&unusable) {
        NextStep::Play(next) if !unusable(&next) => effects.extend(self.start_current(None)),
        _ => {
          self.player.loading = false;
          self.playback.is_playing = false;
        }
      }
      return effects;
    }
    self.player.restart_current();
    // Résolution normale du flux (jamais une URL de flux enregistrée), puis lecture à la position enregistrée.
    effects.extend(self.start_current_at(at));
    if !track.is_local() {
      self.resume.pending = Some(Pending { at, attempts: 0 });
    }
    effects
  }

  /// Toute autre lecture remplace la reprise en attente.
  pub(crate) fn resume_cancel(&mut self) {
    self.resume.armed = None;
    self.resume.pending = None;
  }

  pub(crate) fn resume_loaded(&mut self) {
    self.resume.pending = None;
  }

  /// La résolution du titre repris a échoué : nouvel essai plus tard, tant qu'il en reste.
  pub(crate) fn resume_failed(&mut self) -> Option<Vec<Effect>> {
    let pending = self.resume.pending.as_mut()?;
    let Some(&delay) = RETRY_DELAYS.get(pending.attempts) else {
      tracing::warn!(attempts = pending.attempts, "reprise : abandon");
      self.resume.pending = None;
      return None;
    };
    pending.attempts += 1;
    // Le titre reste en chargement pendant l'attente : la reprise n'a pas échoué, elle est en cours.
    self.player.loading = true;
    tracing::warn!(
      attempt = pending.attempts,
      delay_s = delay.as_secs(),
      "reprise : nouvel essai plus tard"
    );
    Some(vec![Effect::ResumeRetry(self.player.token, delay)])
  }

  /// Nouvel essai de la reprise, si rien d'autre n'a été lu entre-temps.
  pub(crate) fn resume_retry(&mut self, token: u64) -> Vec<Effect> {
    let Some(pending) = self.resume.pending.filter(|_| token == self.player.token) else {
      return Vec::new();
    };
    let Some(track) = self.player.current.clone() else {
      return Vec::new();
    };
    // Nouveau jeton, mais ni historique, ni paroles, ni pochette : seule la résolution du flux repart.
    self.player.restart_current();
    vec![self.resolve_effect(&track, pending.at, None, false)]
  }

  /// Écritures demandées par le message qui vient d'être appliqué.
  pub(crate) fn resume_after(&mut self, watch: Watch, effects: &[Effect]) -> Vec<Effect> {
    if self.jam.is_some() || self.import_waiting() {
      return Vec::new();
    }
    let mut saves = Vec::new();
    let token = self.player.token;
    if !watch.light || token != self.resume.token {
      self.resume.token = token;
      let signature = signature(&self.player);
      if self.resume.saved != Some(signature) {
        self.resume.saved = Some(signature);
        saves.push(Effect::SaveResume(Box::new(capture(&self.player))));
      }
    }
    if !self.resume_live() {
      return saves;
    }
    // Un déplacement vaut pour sa cible : le moteur n'a pas encore rendu la nouvelle position.
    let seek = effects.iter().rev().find_map(|effect| match effect {
      Effect::AudioSeek(to) => Some(*to),
      _ => None,
    });
    let seeked = seek.is_some();
    let at = seek.unwrap_or(self.playback.position);
    let paused = std::mem::replace(&mut self.resume.was_playing, self.playback.is_playing)
      && !self.playback.is_playing;
    // Pendant la lecture, un déplacement n'écrit pas : le tic suivant le fait au rythme de `POSITION_EVERY`
    // (flèche maintenue, boucle d'un extrait). En pause, la position exacte part tout de suite.
    let due = match self.resume.position {
      Some((saved, last)) if saved == token => {
        (watch.tick && at.abs_diff(last) >= POSITION_EVERY)
          || ((paused || (seeked && !self.playback.is_playing)) && at != last)
      }
      _ => watch.tick,
    };
    if due && let Some(track) = &self.player.current {
      self.resume.position = Some((token, at));
      saves.push(Effect::SaveResumePosition(ResumePosition::new(
        track.canonical_id(&self.data.platform),
        at,
      )));
    }
    saves
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::test_temp::TempDir;
  use crate::ui::PlayerMessage;
  use crate::user_data::UserData;
  use crate::window::PersistedWindowState;
  use riff_audio::Source;
  use serde_json::json;

  fn track(id: &str) -> Track {
    Track {
      id: Some(id.to_string()),
      title: Some(format!("Titre {id}")),
      artist: Some(format!("Artiste {id}")),
      duration: Some(200.0),
      url: Some(format!("https://exemple.org/{id}")),
      platform: Some("ytmusic".to_string()),
      ..Track::default()
    }
  }

  fn tracks(ids: &[&str]) -> Vec<Track> {
    ids.iter().map(|id| track(id)).collect()
  }

  fn app() -> App {
    App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
    .with_data(UserData {
      autoplay: false,
      ..UserData::default()
    })
  }

  fn state(ids: &[&str], index: usize) -> ResumeState {
    let queue = tracks(ids);
    ResumeState {
      version: FORMAT,
      current: queue.get(index).cloned(),
      queue,
      index: Some(index),
      shuffle: true,
      scope: Some("pl:abc".to_string()),
      radio: false,
      my_wave: false,
    }
  }

  fn restore(state: ResumeState, position: Duration) -> Restore {
    Restore {
      state,
      position,
      missing: false,
    }
  }

  fn resolve_of(effects: &[Effect]) -> Option<(u64, Duration, Box<Track>)> {
    effects.iter().find_map(|effect| match effect {
      Effect::ResolveTrack {
        token,
        start,
        track,
        options,
        ..
      } => {
        assert!(!options.force_yt_dlp);
        Some((*token, *start, track.clone()))
      }
      _ => None,
    })
  }

  fn saves(effects: &[Effect]) -> (usize, usize) {
    let state = effects
      .iter()
      .filter(|effect| matches!(effect, Effect::SaveResume(_)))
      .count();
    let position = effects
      .iter()
      .filter(|effect| matches!(effect, Effect::SaveResumePosition(_)))
      .count();
    (state, position)
  }

  fn position(app: &mut App, secs: u64) -> Vec<Effect> {
    app.update(Message::Audio(AudioEvent::Position(Duration::from_secs(
      secs,
    ))))
  }

  /// Titre `ids[at]` lancé puis chargé : la file part à l'écriture une fois.
  fn playing(ids: &[&str], at: usize) -> App {
    let mut app = app();
    let list = tracks(ids);
    app.player.play_list(list[at].clone(), list, at, "ytmusic");
    app.start_current(None);
    let effects = app.update(Message::Audio(AudioEvent::Loaded {
      duration: Some(Duration::from_secs(200)),
      sample_rate: 44_100,
    }));
    assert_eq!(saves(&effects), (1, 0));
    app
  }

  #[test]
  fn the_state_survives_a_round_trip_through_the_files() {
    let dir = TempDir::new("resume-round-trip");
    let keeper = Keeper::new(Some(dir.to_path_buf()));
    let mut saved = state(&["a", "b", "c"], 1);
    saved.queue[0]
      .extra
      .insert("localThumbnail".into(), json!("x.jpg"));
    keeper.set_state(saved.clone());
    keeper.set_position(ResumePosition::new(
      track("b").canonical_id("ytmusic"),
      Duration::from_millis(42_500),
    ));
    keeper.write().unwrap();
    let loaded = load(&dir, "ytmusic").unwrap();
    assert_eq!(loaded.state, saved);
    assert_eq!(loaded.position, Duration::from_millis(42_500));
    assert!(!loaded.missing);
    let raw: Value = serde_json::from_slice(&fs::read(dir.join(STATE_FILE)).unwrap()).unwrap();
    assert_eq!(raw["version"], 1);
    assert_eq!(raw["scope"], "pl:abc");
    assert!(!dir.join(format!("{STATE_FILE}.0.tmp")).exists());
  }

  #[test]
  fn a_position_of_another_track_is_not_used() {
    let dir = TempDir::new("resume-other-track");
    let keeper = Keeper::new(Some(dir.to_path_buf()));
    keeper.set_state(state(&["a", "b"], 0));
    keeper.set_position(ResumePosition::new(
      track("b").canonical_id("ytmusic"),
      Duration::from_secs(30),
    ));
    keeper.write().unwrap();
    assert_eq!(load(&dir, "ytmusic").unwrap().position, Duration::ZERO);
  }

  #[test]
  fn a_missing_corrupt_or_unknown_file_restores_nothing() {
    let dir = TempDir::new("resume-corrupt");
    assert!(load(&dir, "ytmusic").is_none());
    fs::write(dir.join(STATE_FILE), "{\"version\":1,\"queue\":").unwrap();
    assert!(load(&dir, "ytmusic").is_none());
    fs::write(dir.join(STATE_FILE), "{\"version\":2,\"queue\":[]}").unwrap();
    assert!(load(&dir, "ytmusic").is_none());
    fs::write(dir.join(STATE_FILE), "{\"queue\":[]}").unwrap();
    assert!(load(&dir, "ytmusic").is_none());
    fs::write(dir.join(STATE_FILE), "[1, 2]").unwrap();
    assert!(load(&dir, "ytmusic").is_none());
    fs::write(dir.join(STATE_FILE), "{\"version\":1,\"index\":9}").unwrap();
    fs::write(dir.join(POSITION_FILE), "garbage").unwrap();
    let loaded = load(&dir, "ytmusic").unwrap();
    assert_eq!(loaded.state.index, None);
    assert_eq!(loaded.position, Duration::ZERO);
  }

  #[test]
  fn an_absent_local_file_is_flagged() {
    let dir = TempDir::new("resume-local");
    let local = Track {
      platform: Some("local".into()),
      path: Some(dir.join("absent.mp3").to_string_lossy().into_owned()),
      ..track("l")
    };
    let keeper = Keeper::new(Some(dir.to_path_buf()));
    keeper.set_state(ResumeState {
      current: Some(local.clone()),
      queue: vec![local],
      ..state(&[], 0)
    });
    keeper.write().unwrap();
    assert!(load(&dir, "ytmusic").unwrap().missing);
  }

  #[test]
  fn the_saved_queue_is_bounded_around_the_current_track_and_tracks_are_slimmed() {
    let mut player = PlayerState::new(1);
    let ids: Vec<String> = (0..5000).map(|n| n.to_string()).collect();
    let mut list: Vec<Track> = ids.iter().map(|id| track(id)).collect();
    list[4000]
      .extra
      .insert("lyricsCache".into(), json!([{ "label": "x" }]));
    list[4000]
      .extra
      .insert("filePath".into(), json!("C:/a.mp3"));
    list[4000].added_at = Some(1);
    player.play_list(list[4000].clone(), list, 4000, "ytmusic");
    let saved = capture(&player);
    assert_eq!(saved.queue.len(), MAX_QUEUE);
    let index = saved.index.unwrap();
    assert_eq!(saved.queue[index].id.as_deref(), Some("4000"));
    assert_eq!(index, MAX_QUEUE / 4);
    let kept = &saved.queue[index];
    assert_eq!(kept.added_at, None);
    assert!(kept.extra.contains_key("filePath"));
    assert!(!kept.extra.contains_key("lyricsCache"));
    player.index = Some(4990);
    let saved = capture(&player);
    assert_eq!(saved.queue.last().unwrap().id.as_deref(), Some("4999"));
    assert_eq!(
      saved.queue[saved.index.unwrap()].id.as_deref(),
      Some("4990")
    );
    let bytes = serde_json::to_vec(&saved).unwrap().len();
    assert!(bytes < 300 * 1024, "{bytes}");
  }

  #[test]
  fn the_keeper_writes_nothing_without_a_change() {
    let dir = TempDir::new("resume-unchanged");
    let keeper = Keeper::new(Some(dir.to_path_buf()));
    keeper.write().unwrap();
    assert!(!dir.join(STATE_FILE).exists());
    keeper.set_state(state(&["a"], 0));
    keeper.set_position(ResumePosition::new(
      "ytmusic:a".into(),
      Duration::from_secs(5),
    ));
    keeper.write().unwrap();
    fs::remove_file(dir.join(STATE_FILE)).unwrap();
    fs::remove_file(dir.join(POSITION_FILE)).unwrap();
    keeper.write().unwrap();
    assert!(!dir.join(STATE_FILE).exists());
    assert!(!dir.join(POSITION_FILE).exists());
    keeper.note_position(Duration::from_secs(7));
    keeper.write().unwrap();
    assert!(!dir.join(STATE_FILE).exists());
    let written: ResumePosition =
      serde_json::from_slice(&fs::read(dir.join(POSITION_FILE)).unwrap()).unwrap();
    assert_eq!(written.position_ms, 7000);
  }

  #[test]
  fn the_restore_brings_back_queue_index_shuffle_scope_and_plays_at_the_position() {
    let mut app = app();
    app.restore_playback(
      restore(state(&["a", "b", "c"], 1), Duration::from_secs(73)),
      true,
    );
    assert_eq!(app.player.queue, tracks(&["a", "b", "c"]));
    assert_eq!(app.player.index, Some(1));
    assert!(app.player.shuffle);
    assert_eq!(app.player.scope.as_deref(), Some("pl:abc"));
    assert!(!app.player.loading);
    let effects = app.update(Message::Player(PlayerMessage::Resume));
    let (token, start, resolved) = resolve_of(&effects).expect("résolution du titre");
    assert_eq!(token, app.player.token);
    assert_eq!(start, Duration::from_secs(73));
    assert_eq!(resolved.id.as_deref(), Some("b"));
    assert!(app.player.loading);
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::AudioMix(..)))
    );
    // Le flux résolu se charge à la position, en lecture.
    let effects = app.update(Message::Player(PlayerMessage::Resolved {
      token,
      start,
      crossfade: None,
      result: Ok(Source::File("b.mp3".into())),
    }));
    assert!(matches!(
      effects.as_slice(),
      [Effect::AudioLoadSource(_, at)] if *at == Duration::from_secs(73)
    ));
    // Une seule relance.
    assert!(resolve_of(&app.update(Message::Player(PlayerMessage::Resume))).is_none());
  }

  #[test]
  fn the_restore_does_not_rewrite_the_state_it_just_read() {
    let mut app = app();
    app.restore_playback(restore(state(&["a", "b"], 0), Duration::ZERO), false);
    let effects = app.update(Message::Player(PlayerMessage::ToggleRepeat));
    assert_eq!(saves(&effects), (0, 0));
  }

  #[test]
  fn without_start_the_restored_track_waits() {
    let mut app = app();
    app.restore_playback(restore(state(&["a"], 0), Duration::from_secs(9)), false);
    let effects = app.update(Message::Player(PlayerMessage::Resume));
    assert!(resolve_of(&effects).is_none());
    assert!(!app.player.loading);
  }

  #[test]
  fn a_blocked_track_is_skipped_like_next() {
    let mut app = app();
    assert!(app.data.toggle_block_track(&track("b")));
    let mut saved = state(&["a", "b", "c"], 1);
    saved.shuffle = false;
    app.restore_playback(restore(saved, Duration::from_secs(50)), true);
    let effects = app.update(Message::Player(PlayerMessage::Resume));
    let (_, start, resolved) = resolve_of(&effects).unwrap();
    assert_eq!(resolved.id.as_deref(), Some("c"));
    assert_eq!(start, Duration::ZERO);
    assert_eq!(app.player.index, Some(2));
  }

  #[test]
  fn a_missing_local_file_moves_on_and_nothing_playable_plays_nothing() {
    let mut moved = app();
    let mut saved = state(&["a", "b"], 0);
    saved.shuffle = false;
    moved.restore_playback(
      Restore {
        state: saved,
        position: Duration::from_secs(5),
        missing: true,
      },
      true,
    );
    let (_, _, resolved) =
      resolve_of(&moved.update(Message::Player(PlayerMessage::Resume))).unwrap();
    assert_eq!(resolved.id.as_deref(), Some("b"));

    let mut alone = app();
    alone.restore_playback(
      Restore {
        state: state(&["a"], 0),
        position: Duration::ZERO,
        missing: true,
      },
      true,
    );
    let effects = alone.update(Message::Player(PlayerMessage::Resume));
    assert!(resolve_of(&effects).is_none());
    assert!(!alone.player.loading);
    assert!(!alone.playback.is_playing);
  }

  #[test]
  fn a_failed_resolution_is_retried_a_bounded_number_of_times_then_shown() {
    let mut app = app();
    app.restore_playback(restore(state(&["a"], 0), Duration::from_secs(20)), true);
    let effects = app.update(Message::Player(PlayerMessage::Resume));
    let (mut token, ..) = resolve_of(&effects).unwrap();
    for delay in RETRY_DELAYS {
      let effects = app.update(Message::Player(PlayerMessage::Resolved {
        token,
        start: Duration::from_secs(20),
        crossfade: None,
        result: Err("offline".into()),
      }));
      assert_eq!(effects, vec![Effect::ResumeRetry(token, delay)]);
      let effects = app.update(Message::Player(PlayerMessage::ResumeRetry(token)));
      let (next, start, _) = resolve_of(&effects).unwrap();
      assert!(next > token);
      assert_eq!(start, Duration::from_secs(20));
      token = next;
    }
    let effects = app.update(Message::Player(PlayerMessage::Resolved {
      token,
      start: Duration::from_secs(20),
      crossfade: None,
      result: Err("offline".into()),
    }));
    assert!(
      matches!(effects.as_slice(), [Effect::Toast(..)]),
      "{effects:?}"
    );
  }

  #[test]
  fn the_fallback_starts_like_the_window_and_the_window_then_starts_nothing_twice() {
    let mut app = app();
    app.settings.proxy_enabled = true;
    app.settings.proxy_active = Some("http://proxy.invalid:8080".to_string());
    app.settings.sound.persist = true;
    app.settings.sound.output_device = Some("device".to_string());
    app.restore_playback(restore(state(&["a"], 0), Duration::from_secs(20)), true);
    let effects = app.update(Message::Player(PlayerMessage::Resume));
    let at = |wanted: fn(&Effect) -> bool| effects.iter().position(wanted);
    let proxy = at(|effect| {
      matches!(
        effect,
        Effect::CoreJson {
          path: "/api/proxy-config",
          ..
        }
      )
    });
    let device = at(|effect| matches!(effect, Effect::AudioDevice(_)));
    let resolve = at(|effect| matches!(effect, Effect::ResolveTrack { .. }));
    assert!(
      proxy.is_some() && device.is_some() && resolve.is_some(),
      "{effects:?}"
    );
    assert!(proxy < resolve && device < resolve);
    let opened = app.update(Message::WindowOpened {
      position: None,
      size: iced::Size::new(800.0, 600.0),
    });
    assert!(resolve_of(&opened).is_none());
    assert!(
      !opened
        .iter()
        .any(|effect| matches!(effect, Effect::AudioDevice(_)))
    );
    // Et dans l'autre sens : la fenêtre d'abord, le secours ensuite ne relance rien.
    let mut app = self::app();
    app.restore_playback(restore(state(&["a"], 0), Duration::from_secs(20)), true);
    let opened = app.update(Message::WindowOpened {
      position: None,
      size: iced::Size::new(800.0, 600.0),
    });
    assert!(resolve_of(&opened).is_some());
    let later = app.update(Message::Player(PlayerMessage::Resume));
    assert!(resolve_of(&later).is_none());
  }

  #[test]
  fn a_retry_only_resolves_again_and_keeps_the_track_loading() {
    let mut app = app();
    app.restore_playback(restore(state(&["a"], 0), Duration::from_secs(20)), true);
    let (token, ..) = resolve_of(&app.update(Message::Player(PlayerMessage::Resume))).unwrap();
    app.update(Message::Player(PlayerMessage::Resolved {
      token,
      start: Duration::from_secs(20),
      crossfade: None,
      result: Err("offline".into()),
    }));
    assert!(app.player.loading);
    let effects = app.update(Message::Player(PlayerMessage::ResumeRetry(token)));
    assert!(
      matches!(effects.as_slice(), [Effect::ResolveTrack { .. }]),
      "{effects:?}"
    );
    assert!(app.player.loading);
  }

  #[test]
  fn a_seek_while_playing_waits_for_the_next_interval_and_a_paused_seek_is_saved() {
    let mut app = playing(&["a", "b"], 0);
    assert_eq!(saves(&position(&mut app, 0)), (0, 1));
    for _ in 0..5 {
      let effects = app.update(Message::Player(PlayerMessage::SeekBy(5.0)));
      assert_eq!(saves(&effects), (0, 0));
    }
    app.update(Message::Player(PlayerMessage::TogglePlay));
    let effects = app.update(Message::Player(PlayerMessage::SeekBy(5.0)));
    assert_eq!(saves(&effects), (0, 1));
  }

  #[test]
  fn the_extra_keys_shown_in_the_queue_survive_slimming() {
    let mut full = track("a");
    for key in ["explicit_lyrics", "badges", "no", "durationMs"] {
      full.extra.insert(key.to_string(), json!(1));
    }
    full.extra.insert("heavy".to_string(), json!("x"));
    let slim = slim(&full);
    assert_eq!(slim.extra.len(), 4);
    assert!(!slim.extra.contains_key("heavy"));
  }
  #[test]
  fn another_track_cancels_a_pending_retry() {
    let mut app = app();
    app.data.favorites = tracks(&["x"]);
    app.restore_playback(restore(state(&["a"], 0), Duration::from_secs(20)), true);
    let (token, ..) = resolve_of(&app.update(Message::Player(PlayerMessage::Resume))).unwrap();
    app.update(Message::Player(PlayerMessage::Resolved {
      token,
      start: Duration::ZERO,
      crossfade: None,
      result: Err("offline".into()),
    }));
    app.player.play_single(track("x"), "ytmusic");
    app.start_current(None);
    assert!(resolve_of(&app.update(Message::Player(PlayerMessage::ResumeRetry(token)))).is_none());
  }

  #[test]
  fn track_changes_pause_and_seek_are_saved_and_positions_at_most_every_ten_seconds() {
    let mut app = playing(&["a", "b"], 0);
    // La file a été enregistrée au changement de titre ; la position l'est au premier tic du titre.
    assert_eq!(saves(&position(&mut app, 0)), (0, 1));
    assert_eq!(saves(&position(&mut app, 4)), (0, 0));
    assert_eq!(saves(&position(&mut app, 9)), (0, 0));
    assert_eq!(saves(&position(&mut app, 10)), (0, 1));
    assert_eq!(saves(&position(&mut app, 15)), (0, 0));
    let effects = app.update(Message::Player(PlayerMessage::TogglePlay));
    assert_eq!(saves(&effects), (0, 1));
    // En pause, rien ne change : rien n'est écrit.
    assert_eq!(
      saves(&app.update(Message::Player(PlayerMessage::ToggleRepeat))),
      (0, 0)
    );
    let effects = app.update(Message::Player(PlayerMessage::SeekBy(5.0)));
    assert_eq!(saves(&effects), (0, 1));
    let effects = app.update(Message::Player(PlayerMessage::ToggleShuffle));
    assert_eq!(saves(&effects), (1, 0));
    let effects = app.update(Message::Player(PlayerMessage::Next));
    let (state_saves, _) = saves(&effects);
    assert_eq!(state_saves, 1);
    let Some(Effect::SaveResume(saved)) = effects
      .iter()
      .find(|effect| matches!(effect, Effect::SaveResume(_)))
    else {
      panic!()
    };
    assert!(saved.current.is_some());
  }

  #[test]
  fn a_queue_change_without_a_new_track_is_saved() {
    let mut app = playing(&["a", "b", "c"], 0);
    position(&mut app, 0);
    let effects = app.update(Message::Ui(crate::ui::UiMessage::ClearQueue));
    assert_eq!(saves(&effects).0, 1, "{effects:?}");
  }

  #[test]
  fn a_jam_queue_is_never_saved_and_the_personal_one_stays() {
    let mut app = playing(&["a", "b"], 0);
    position(&mut app, 0);
    app.jam = Some(Box::new(crate::jam::state::JamState::new(
      crate::jam::Role::Guest,
      "moi",
    )));
    app.player.queue = tracks(&["j1", "j2"]);
    app.player.play_external(track("j1"), Some(0));
    app.player.loading = false;
    assert_eq!(saves(&position(&mut app, 30)), (0, 0));
    assert!(!app.resume_live());
    assert_eq!(
      saves(&app.update(Message::Player(PlayerMessage::ToggleRepeat))),
      (0, 0)
    );
  }

  #[test]
  fn the_watch_is_light_for_engine_events() {
    let tick = Watch::of(&Message::Audio(AudioEvent::Position(Duration::ZERO)));
    assert!(tick.light && tick.tick);
    let ended = Watch::of(&Message::Audio(AudioEvent::Ended));
    assert!(ended.light && !ended.tick);
    let next = Watch::of(&Message::Player(PlayerMessage::Next));
    assert!(!next.light && !next.tick);
  }
}
