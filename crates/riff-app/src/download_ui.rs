//! Téléchargement et découpe d'un titre. La vue est dans `screens/download.rs`.

use crate::app::{App, Effect};
use crate::ui::{ListId, Modal};
use crate::user_data::Track;
use riff_audio::{AudioSettings, HqSettings};
use std::path::PathBuf;
use std::time::Duration;

/// Formats de `#format-chips-grid`.
pub const FORMATS: [(&str, &str); 5] = [
  ("mp3", "Universal"),
  ("flac", "Lossless"),
  ("m4a", "M4A"),
  ("opus", "Modern"),
  ("wav", "WAV"),
];
/// Qualités de `#quality-chips-grid`.
pub const QUALITIES: [(&str, &str); 4] = [
  ("0", "Studio"),
  ("2", "High quality"),
  ("4", "Standard"),
  ("5", "Compact"),
];
/// Écart minimal entre les poignées (`minRatio`).
pub const MIN_TRIM: f64 = 1.0;
/// Colonnes de l'onde.
pub const WAVE_COLUMNS: usize = 420;
/// Durée supposée d'un titre sans durée connue (`openDownloadModal`).
const UNKNOWN_DURATION: f64 = 180.0;
/// `saveTrackTrim` : une découpe qui couvre tout le titre est retirée.
const FULL_START: f64 = 0.15;
const FULL_END: f64 = 0.25;

/// Onglets de la fenêtre.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DownloadTab {
  #[default]
  File,
  Trim,
}

/// Poignée de la découpe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
  Start,
  End,
}

/// Sélection de la découpe, en secondes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimRange {
  pub start: f64,
  pub end: f64,
  pub duration: f64,
}

fn tenth(value: f64) -> f64 {
  (value * 10.0).round() / 10.0
}

impl TrimRange {
  /// Toute la durée.
  #[must_use]
  pub fn whole(duration: f64) -> Self {
    let duration = if duration.is_finite() && duration > 0.0 {
      duration
    } else {
      UNKNOWN_DURATION
    };
    Self {
      start: 0.0,
      end: duration,
      duration,
    }
  }

  /// Déplace une poignée : arrondi au dixième, au moins [`MIN_TRIM`] entre les deux, jamais hors du titre.
  #[must_use]
  pub fn moved(self, handle: Handle, seconds: f64) -> Self {
    let seconds = if seconds.is_finite() {
      tenth(seconds)
    } else {
      return self;
    };
    match handle {
      Handle::Start => Self {
        start: seconds.min(self.end - MIN_TRIM).max(0.0),
        ..self
      },
      Handle::End => Self {
        end: seconds.max(self.start + MIN_TRIM).min(self.duration),
        ..self
      },
    }
  }

  /// Toute la durée est sélectionnée.
  #[must_use]
  pub fn is_whole(self) -> bool {
    self.start <= FULL_START && self.end >= self.duration - FULL_END
  }

  /// Durée sélectionnée.
  #[must_use]
  pub fn length(self) -> f64 {
    self.end - self.start
  }
}

/// Ce qu'il faut faire pour produire le fichier.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadPlan {
  /// Titre en ligne, sans effets : `/api/download`.
  Remote,
  /// Fichier local : découpe et encodage par `encode_file`, sans réseau.
  Local(PathBuf),
  /// Avec effets : audio brut, `offline::render`, puis `encode_file`.
  Rendered {
    local: Option<PathBuf>,
    settings: Box<AudioSettings>,
    hq: Option<HqSettings>,
  },
}

/// Téléchargement demandé.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadJob {
  pub token: u64,
  pub track: Box<Track>,
  pub format: String,
  pub quality: String,
  pub trim: Option<(f64, f64)>,
  pub plan: DownloadPlan,
}

/// Paramètres de `/api/download`, comme `downloadTrack` de Riff.
#[must_use]
pub fn download_query(job: &DownloadJob, raw: bool) -> Vec<(String, String)> {
  let track = &job.track;
  let mut query = vec![
    ("title".to_string(), track.title.clone().unwrap_or_default()),
    (
      "artist".to_string(),
      track.artist.clone().unwrap_or_default(),
    ),
    ("id".to_string(), track.id.clone().unwrap_or_default()),
    ("url".to_string(), track.url.clone().unwrap_or_default()),
    (
      "thumbnail".to_string(),
      track.thumb_url().unwrap_or_default(),
    ),
    ("format".to_string(), job.format.clone()),
    ("quality".to_string(), job.quality.clone()),
  ];
  if raw {
    query.push(("raw".to_string(), "1".to_string()));
  } else if let Some((start, end)) = job.trim {
    query.push(("start".to_string(), start.to_string()));
    query.push(("end".to_string(), end.to_string()));
  }
  query
}

/// État du téléchargement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DownloadState {
  #[default]
  Idle,
  Running,
  Done(String),
  Failed(String),
}

/// Onde de la découpe : calculée à la demande.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum WaveState {
  #[default]
  Empty,
  Loading,
  Ready(Vec<f32>),
  Failed(String),
}

/// Résultat du calcul de l'onde.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveData {
  pub peaks: Vec<f32>,
  pub duration: f64,
}

/// « Download all » : titres distants d'une playlist, téléchargés l'un après l'autre.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadBatch {
  pub name: String,
  pub tracks: Vec<Track>,
  /// Rang du titre en cours, ou du prochain.
  pub current: usize,
  pub done: usize,
  pub failed: usize,
  /// Cancel demandé : le lot s'arrête à la fin du titre en cours.
  pub cancelling: bool,
}

/// État de la fenêtre.
#[derive(Debug, Clone, Default)]
pub struct DownloadUi {
  pub batch: Option<DownloadBatch>,
  pub track: Option<Box<Track>>,
  pub tab: DownloadTab,
  pub quality: String,
  pub with_effects: bool,
  pub state: DownloadState,
  pub token: u64,
  pub trim: Option<TrimRange>,
  pub wave: WaveState,
  pub wave_token: u64,
  /// Écoute de l'extrait : la lecture s'arrête à cette position.
  pub preview_end: Option<f64>,
}

/// Messages de la fenêtre.
#[derive(Debug, Clone)]
pub enum DownloadMessage {
  Open(Box<Track>),
  /// « Download all » de la playlist ouverte.
  OpenAll,
  /// « Download all » de « Titres aimés ».
  OpenAllFavorites,
  Tab(DownloadTab),
  Format(String),
  Quality(String),
  ToggleEffects(bool),
  Start,
  Cancel,
  Finished(u64, Result<String, String>),
  WaveReady(u64, Result<WaveData, String>),
  MoveHandle(Handle, f64),
  SaveTrim,
  ResetTrim,
  Preview,
}

impl App {
  /// Découpe enregistrée d'un titre (`getTrackTrim`) : ignorée si elle fait moins de 1,5 s, comme Riff.
  #[must_use]
  pub fn saved_trim(&self, track: &Track) -> Option<(f64, f64)> {
    let trim = self
      .data
      .track_trims
      .get(&track.canonical_id(&self.data.platform))?;
    let start = trim
      .get("start")
      .and_then(serde_json::Value::as_f64)
      .unwrap_or(0.0)
      .max(0.0);
    let end = trim.get("end").and_then(serde_json::Value::as_f64)?;
    (end > start + 1.5).then_some((start, end))
  }

  fn open_download(&mut self, track: Box<Track>) -> Vec<Effect> {
    if self.batch_running() {
      return Vec::new();
    }
    self.ui.download.batch = None;
    let duration = track.duration.unwrap_or(0.0);
    let mut range = TrimRange::whole(duration);
    if let Some((start, end)) = self.saved_trim(&track) {
      range.start = start.min(range.duration);
      range.end = end.min(range.duration);
    }
    let download = &mut self.ui.download;
    download.track = Some(track);
    download.tab = DownloadTab::File;
    download.state = DownloadState::Idle;
    download.wave = WaveState::Empty;
    download.trim = Some(range);
    download.preview_end = None;
    if download.quality.is_empty() {
      QUALITIES[0].0.clone_into(&mut download.quality);
    }
    self.open_modal(Modal::Download)
  }

  fn batch_running(&self) -> bool {
    self.ui.download.batch.is_some() && self.ui.download.state == DownloadState::Running
  }

  /// « Downloading i/N » pendant un lot.
  #[must_use]
  pub fn batch_progress(&self) -> Option<String> {
    let batch = self.ui.download.batch.as_ref()?;
    (self.ui.download.state == DownloadState::Running).then(|| {
      format!(
        "{} {}/{}",
        self.i18n.t("Downloading"),
        batch.current + 1,
        batch.tracks.len()
      )
    })
  }

  /// `openDownloadAllModal` : titres locaux exclus, toast s'il n'en reste aucun.
  fn open_download_all(&mut self) -> Vec<Effect> {
    let Some(playlist) = self
      .ui
      .playlist_id
      .as_deref()
      .and_then(|id| self.data.playlist(id))
    else {
      return Vec::new();
    };
    let tracks = playlist.tracks.clone();
    let name = playlist
      .name
      .clone()
      .unwrap_or_else(|| self.i18n.t("Playlist"));
    self.open_download_batch(
      name,
      tracks,
      "No remote tracks to download in this playlist",
    )
  }

  /// Même fenêtre pour les titres aimés, dans l'ordre affiché.
  fn open_download_favorites(&mut self) -> Vec<Effect> {
    let tracks = self.list_as_queue(ListId::Favorites);
    let name = self.liked_title();
    self.open_download_batch(name, tracks, "No remote tracks to download in liked songs")
  }

  fn open_download_batch(
    &mut self,
    name: String,
    tracks: Vec<Track>,
    empty_toast: &str,
  ) -> Vec<Effect> {
    if self.ui.download.state == DownloadState::Running {
      return Vec::new();
    }
    let tracks: Vec<Track> = tracks
      .into_iter()
      .filter(|track| !track.is_local())
      .collect();
    if tracks.is_empty() {
      return vec![Effect::Toast(
        empty_toast.to_string(),
        crate::widgets::toast::ToastKind::Info,
      )];
    }
    let download = &mut self.ui.download;
    download.batch = Some(DownloadBatch {
      name,
      tracks,
      current: 0,
      done: 0,
      failed: 0,
      cancelling: false,
    });
    download.tab = DownloadTab::File;
    download.state = DownloadState::Idle;
    if download.quality.is_empty() {
      QUALITIES[0].0.clone_into(&mut download.quality);
    }
    self.open_modal(Modal::Download)
  }

  /// Lance le titre en cours du lot, par la voie de `DownloadPlan::Remote`.
  fn run_batch_track(&mut self) -> Vec<Effect> {
    let download = &mut self.ui.download;
    let Some(track) = download
      .batch
      .as_ref()
      .and_then(|batch| batch.tracks.get(batch.current))
      .cloned()
    else {
      return Vec::new();
    };
    download.token += 1;
    download.state = DownloadState::Running;
    vec![Effect::RunDownload(Box::new(DownloadJob {
      token: download.token,
      track: Box::new(track),
      format: self.settings.download_format.clone(),
      quality: download.quality.clone(),
      trim: None,
      plan: DownloadPlan::Remote,
    }))]
  }

  /// Un titre du lot est terminé : suivant, ou fin avec le toast de Riff.
  fn batch_finished(&mut self, succeeded: bool) -> Vec<Effect> {
    use crate::widgets::toast::ToastKind;
    let Some(batch) = &mut self.ui.download.batch else {
      return Vec::new();
    };
    if succeeded {
      batch.done += 1;
    } else {
      batch.failed += 1;
    }
    batch.current += 1;
    if !batch.cancelling && batch.current < batch.tracks.len() {
      return self.run_batch_track();
    }
    let (done, failed, total) = (batch.done, batch.failed, batch.tracks.len());
    self.ui.download.batch = None;
    self.ui.download.state = DownloadState::Idle;
    let downloaded = self.i18n.t("Downloaded");
    let (text, kind) = if failed > 0 || done < total {
      (
        format!(
          "{downloaded} {done}/{total} ({failed} {})",
          self.i18n.t("failed")
        ),
        if failed > 0 {
          ToastKind::Error
        } else {
          ToastKind::Info
        },
      )
    } else {
      (
        format!("{downloaded} {total} {}", self.i18n.t("tracks")),
        ToastKind::Success,
      )
    };
    let mut effects = if self.ui.modal == Some(Modal::Download) {
      self.dismiss_modal().unwrap_or_default()
    } else {
      Vec::new()
    };
    effects.push(Effect::Toast(text, kind));
    effects
  }

  fn current_effects(&self) -> (Box<AudioSettings>, Option<HqSettings>) {
    let sound = &self.settings.sound;
    (
      Box::new(self.ui.audio.live.to_engine()),
      sound.hq_enabled.then(|| sound.hq.to_engine()),
    )
  }

  /// Prépare le travail à lancer.
  #[must_use]
  pub fn download_job(&self) -> Option<DownloadJob> {
    let download = &self.ui.download;
    let track = download.track.clone()?;
    let local = track
      .is_local()
      .then(|| track.path.as_deref().map(PathBuf::from))
      .flatten();
    let plan = if download.with_effects {
      let (settings, hq) = self.current_effects();
      DownloadPlan::Rendered {
        local,
        settings,
        hq,
      }
    } else if let Some(path) = local {
      DownloadPlan::Local(path)
    } else {
      DownloadPlan::Remote
    };
    Some(DownloadJob {
      token: download.token + 1,
      track,
      format: self.settings.download_format.clone(),
      quality: download.quality.clone(),
      trim: download
        .trim
        .filter(|range| !range.is_whole())
        .map(|range| (range.start, range.end)),
      plan,
    })
  }

  fn load_wave(&mut self) -> Vec<Effect> {
    let Some(track) = self.ui.download.track.clone() else {
      return Vec::new();
    };
    if matches!(
      self.ui.download.wave,
      WaveState::Loading | WaveState::Ready(_)
    ) {
      return Vec::new();
    }
    self.ui.download.wave = WaveState::Loading;
    self.ui.download.wave_token += 1;
    vec![Effect::LoadWave {
      token: self.ui.download.wave_token,
      track,
    }]
  }

  fn save_trim(&mut self) -> Vec<Effect> {
    let (Some(track), Some(range)) = (self.ui.download.track.clone(), self.ui.download.trim) else {
      return Vec::new();
    };
    let id = track.canonical_id(&self.data.platform);
    if range.is_whole() {
      self.data.track_trims.remove(&id);
    } else {
      let saved = serde_json::json!({ "start": tenth(range.start), "end": tenth(range.end), "updatedAt": riff_core::util::now_millis() });
      self.data.track_trims.insert(id, saved);
    }
    vec![
      self.data_changed(),
      Effect::Toast(
        "Track trim saved".to_string(),
        crate::widgets::toast::ToastKind::Info,
      ),
    ]
  }

  /// Messages de la fenêtre.
  pub fn update_download(&mut self, message: DownloadMessage) -> Vec<Effect> {
    match message {
      DownloadMessage::Open(track) => self.open_download(track),
      DownloadMessage::OpenAll => self.open_download_all(),
      DownloadMessage::OpenAllFavorites => self.open_download_favorites(),
      DownloadMessage::Tab(_) | DownloadMessage::ToggleEffects(_)
        if self.ui.download.batch.is_some() =>
      {
        Vec::new()
      }
      DownloadMessage::Tab(tab) => {
        self.ui.download.tab = tab;
        if tab == DownloadTab::Trim {
          self.load_wave()
        } else {
          Vec::new()
        }
      }
      DownloadMessage::Format(format) => {
        self.settings.download_format = format;
        vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
      }
      DownloadMessage::Quality(quality) => {
        self.ui.download.quality = quality;
        Vec::new()
      }
      DownloadMessage::ToggleEffects(enabled) => {
        self.ui.download.with_effects = enabled;
        Vec::new()
      }
      DownloadMessage::Start => {
        if self.ui.download.state == DownloadState::Running {
          return Vec::new();
        }
        if self.ui.download.batch.is_some() {
          return self.run_batch_track();
        }
        let Some(job) = self.download_job() else {
          return Vec::new();
        };
        self.ui.download.token = job.token;
        self.ui.download.state = DownloadState::Running;
        vec![Effect::RunDownload(Box::new(job))]
      }
      DownloadMessage::Cancel => {
        if self.batch_running() {
          // Riff n'a pas d'annulation : le titre en cours va à son terme.
          if let Some(batch) = &mut self.ui.download.batch {
            batch.cancelling = true;
          }
          return Vec::new();
        }
        if self.ui.download.state == DownloadState::Running {
          self.ui.download.state = DownloadState::Idle;
          self.ui.download.token += 1;
          return vec![Effect::AbortDownload];
        }
        Vec::new()
      }
      DownloadMessage::Finished(token, result) => {
        if token != self.ui.download.token {
          return Vec::new();
        }
        if self.ui.download.batch.is_some() {
          return self.batch_finished(result.is_ok());
        }
        match result {
          Ok(name) => {
            self.ui.download.state = DownloadState::Done(name);
            vec![Effect::Toast(
              "Download complete".to_string(),
              crate::widgets::toast::ToastKind::Info,
            )]
          }
          Err(error) => {
            self.ui.download.state = DownloadState::Failed(error);
            Vec::new()
          }
        }
      }
      DownloadMessage::WaveReady(token, result) => {
        if token != self.ui.download.wave_token {
          return Vec::new();
        }
        match result {
          Ok(wave) => {
            // La durée réelle remplace celle des métadonnées.
            if let Some(range) = &mut self.ui.download.trim {
              let full = range.is_whole();
              range.duration = wave.duration;
              range.end = if full {
                wave.duration
              } else {
                range.end.min(wave.duration)
              };
              range.start = range.start.min((range.end - MIN_TRIM).max(0.0));
            }
            self.ui.download.wave = WaveState::Ready(wave.peaks);
          }
          Err(error) => self.ui.download.wave = WaveState::Failed(error),
        }
        Vec::new()
      }
      DownloadMessage::MoveHandle(handle, seconds) => {
        self.ui.download.trim = self
          .ui
          .download
          .trim
          .map(|range| range.moved(handle, seconds));
        Vec::new()
      }
      DownloadMessage::SaveTrim => self.save_trim(),
      DownloadMessage::ResetTrim => {
        if let Some(range) = &mut self.ui.download.trim {
          *range = TrimRange::whole(range.duration);
        }
        self.save_trim()
      }
      DownloadMessage::Preview => self.preview_trim(),
    }
  }

  fn preview_trim(&mut self) -> Vec<Effect> {
    if let Some(blocked) = self.jam_unavailable() {
      return blocked;
    }
    let (Some(track), Some(range)) = (self.ui.download.track.clone(), self.ui.download.trim) else {
      return Vec::new();
    };
    self.ui.download.preview_end = Some(range.end);
    let start = Duration::from_secs_f64(range.start);
    let platform = self.data.platform.clone();
    if self
      .player
      .current
      .as_ref()
      .is_some_and(|current| current.is_same(&track, &platform))
    {
      return vec![Effect::AudioSeek(start), Effect::AudioPlay];
    }
    self.player.play_single(*track, &platform);
    self.start_current_at(start)
  }

  /// Arrête l'écoute de l'extrait à sa fin. Vrai si la lecture doit s'arrêter.
  pub(crate) fn preview_reached(&mut self, position: f64) -> bool {
    match self.ui.download.preview_end {
      Some(end) if position >= end => {
        self.ui.download.preview_end = None;
        true
      }
      _ => false,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::Message;
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::window::PersistedWindowState;

  fn app() -> App {
    App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
  }

  fn track() -> Box<Track> {
    Box::new(Track {
      id: Some("abc".to_string()),
      title: Some("T".to_string()),
      artist: Some("A".to_string()),
      duration: Some(100.0),
      url: Some("https://www.youtube.com/watch?v=abc".to_string()),
      ..Track::default()
    })
  }

  fn send(app: &mut App, message: DownloadMessage) -> Vec<Effect> {
    app.update(Message::Download(message))
  }

  fn job_of(effects: &[Effect]) -> &DownloadJob {
    effects
      .iter()
      .find_map(|effect| match effect {
        Effect::RunDownload(job) => Some(&**job),
        _ => None,
      })
      .expect("téléchargement lancé")
  }

  #[test]
  fn a_handle_never_leaves_the_track() {
    let range = TrimRange::whole(100.0);
    assert_eq!(range.moved(Handle::Start, -5.0).start, 0.0);
    assert_eq!(range.moved(Handle::End, 500.0).end, 100.0);
  }

  #[test]
  fn the_handles_keep_one_second_between_them() {
    let range = TrimRange {
      start: 10.0,
      end: 20.0,
      duration: 100.0,
    };
    assert_eq!(range.moved(Handle::Start, 19.9).start, 19.0);
    assert_eq!(range.moved(Handle::End, 10.2).end, 11.0);
  }

  #[test]
  fn a_position_is_rounded_to_a_tenth() {
    assert_eq!(
      TrimRange::whole(100.0).moved(Handle::Start, 12.34).start,
      12.3
    );
  }

  #[test]
  fn an_invalid_position_changes_nothing() {
    let range = TrimRange::whole(100.0);
    assert_eq!(range.moved(Handle::Start, f64::NAN), range);
  }

  #[test]
  fn an_unknown_duration_uses_three_minutes() {
    assert_eq!(TrimRange::whole(0.0).duration, 180.0);
    assert_eq!(TrimRange::whole(f64::NAN).duration, 180.0);
  }

  #[test]
  fn the_rendered_length_is_ceil_of_length_over_speed() {
    let frames = |speed: f32, speed_pitch: f32| {
      let settings = AudioSettings {
        speed,
        speed_pitch,
        ..AudioSettings::default()
      };
      riff_audio::offline::render(&vec![0.25; 2000], 2, 8000, &settings, None).len() / 2
    };
    assert_eq!(frames(1.0, 1.0), 1000);
    assert_eq!(frames(2.0, 1.0), 500);
    assert_eq!(frames(1.0, 2.0), 500);
    assert_eq!(frames(0.5, 1.0), 2000);
    assert_eq!(frames(1.5, 1.0), 667);
  }

  #[test]
  fn opening_keeps_the_format_and_takes_the_duration() {
    let mut app = app();
    app.settings.download_format = "flac".to_string();
    send(&mut app, DownloadMessage::Open(track()));
    assert_eq!(app.ui.modal, Some(Modal::Download));
    assert_eq!(app.ui.download.trim, Some(TrimRange::whole(100.0)));
    assert_eq!(app.ui.download.quality, "0");
    assert_eq!(app.download_job().unwrap().format, "flac");
  }

  #[test]
  fn choosing_a_format_is_remembered() {
    let mut app = app();
    let effects = send(&mut app, DownloadMessage::Format("opus".to_string()));
    assert_eq!(app.settings.download_format, "opus");
    assert!(matches!(effects[0], Effect::SaveSettings(_)));
  }

  #[test]
  fn a_remote_download_sends_the_fields_of_riff() {
    let mut app = app();
    send(&mut app, DownloadMessage::Open(track()));
    send(&mut app, DownloadMessage::Quality("4".to_string()));
    let effects = send(&mut app, DownloadMessage::Start);
    let job = job_of(&effects);
    assert_eq!(job.plan, DownloadPlan::Remote);
    let query = download_query(job, false);
    let value = |key: &str| {
      query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
    };
    assert_eq!(value("id"), Some("abc"));
    assert_eq!(value("format"), Some("mp3"));
    assert_eq!(value("quality"), Some("4"));
    assert_eq!(value("start"), None);
    assert_eq!(value("raw"), None);
    assert_eq!(app.ui.download.state, DownloadState::Running);
  }

  #[test]
  fn a_trimmed_download_sends_its_bounds() {
    let mut app = app();
    send(&mut app, DownloadMessage::Open(track()));
    send(&mut app, DownloadMessage::MoveHandle(Handle::Start, 10.0));
    send(&mut app, DownloadMessage::MoveHandle(Handle::End, 30.5));
    let effects = send(&mut app, DownloadMessage::Start);
    let job = job_of(&effects);
    assert_eq!(job.trim, Some((10.0, 30.5)));
    let query = download_query(job, false);
    assert!(query.contains(&("start".to_string(), "10".to_string())));
    assert!(query.contains(&("end".to_string(), "30.5".to_string())));
    assert!(download_query(job, true).contains(&("raw".to_string(), "1".to_string())));
    assert!(
      !download_query(job, true)
        .iter()
        .any(|(key, _)| key == "start")
    );
  }

  #[test]
  fn a_local_file_needs_no_network() {
    let mut app = app();
    let local = Track {
      platform: Some("local".to_string()),
      path: Some("D:/a.mp3".to_string()),
      ..*track()
    };
    assert!(local.is_local());
    send(&mut app, DownloadMessage::Open(Box::new(local)));
    assert_eq!(
      app.download_job().unwrap().plan,
      DownloadPlan::Local(PathBuf::from("D:/a.mp3"))
    );
  }

  #[test]
  fn effects_use_the_current_sound() {
    let mut app = app();
    app.ui.audio.live.speed = 1.5;
    app.settings.sound.hq_enabled = true;
    send(&mut app, DownloadMessage::Open(track()));
    send(&mut app, DownloadMessage::ToggleEffects(true));
    let job = app.download_job().unwrap();
    let DownloadPlan::Rendered {
      local,
      settings,
      hq,
    } = job.plan
    else {
      panic!("rendu attendu")
    };
    assert_eq!(local, None);
    assert!((settings.speed - 1.5).abs() < 1e-6);
    assert!(hq.is_some());
  }

  #[test]
  fn a_second_start_does_nothing_while_running() {
    let mut app = app();
    send(&mut app, DownloadMessage::Open(track()));
    send(&mut app, DownloadMessage::Start);
    assert!(send(&mut app, DownloadMessage::Start).is_empty());
  }

  #[test]
  fn cancelling_aborts_and_ignores_the_late_answer() {
    let mut app = app();
    send(&mut app, DownloadMessage::Open(track()));
    let token = job_of(&send(&mut app, DownloadMessage::Start)).token;
    assert_eq!(
      send(&mut app, DownloadMessage::Cancel),
      [Effect::AbortDownload]
    );
    assert_eq!(app.ui.download.state, DownloadState::Idle);
    send(
      &mut app,
      DownloadMessage::Finished(token, Ok("a.mp3".to_string())),
    );
    assert_eq!(app.ui.download.state, DownloadState::Idle);
  }

  #[test]
  fn the_end_and_the_failure_are_shown() {
    let mut app = app();
    send(&mut app, DownloadMessage::Open(track()));
    let token = job_of(&send(&mut app, DownloadMessage::Start)).token;
    send(
      &mut app,
      DownloadMessage::Finished(token, Err("réseau".to_string())),
    );
    assert_eq!(
      app.ui.download.state,
      DownloadState::Failed("réseau".to_string())
    );
    let token = job_of(&send(&mut app, DownloadMessage::Start)).token;
    send(
      &mut app,
      DownloadMessage::Finished(token, Ok("a.mp3".to_string())),
    );
    assert_eq!(
      app.ui.download.state,
      DownloadState::Done("a.mp3".to_string())
    );
  }

  #[test]
  fn the_wave_is_computed_once_when_the_tab_opens() {
    let mut app = app();
    send(&mut app, DownloadMessage::Open(track()));
    let effects = send(&mut app, DownloadMessage::Tab(DownloadTab::Trim));
    assert!(matches!(effects[0], Effect::LoadWave { .. }));
    assert_eq!(app.ui.download.wave, WaveState::Loading);
    assert!(send(&mut app, DownloadMessage::Tab(DownloadTab::Trim)).is_empty());
  }

  #[test]
  fn the_real_duration_replaces_the_metadata() {
    let mut app = app();
    send(&mut app, DownloadMessage::Open(track()));
    send(&mut app, DownloadMessage::Tab(DownloadTab::Trim));
    let token = app.ui.download.wave_token;
    send(
      &mut app,
      DownloadMessage::WaveReady(
        token,
        Ok(WaveData {
          peaks: vec![0.5],
          duration: 90.0,
        }),
      ),
    );
    let range = app.ui.download.trim.unwrap();
    assert_eq!((range.start, range.end, range.duration), (0.0, 90.0, 90.0));
    assert_eq!(app.ui.download.wave, WaveState::Ready(vec![0.5]));
  }

  #[test]
  fn a_late_wave_is_ignored() {
    let mut app = app();
    send(&mut app, DownloadMessage::Open(track()));
    send(&mut app, DownloadMessage::Tab(DownloadTab::Trim));
    send(
      &mut app,
      DownloadMessage::WaveReady(
        99,
        Ok(WaveData {
          peaks: vec![0.5],
          duration: 90.0,
        }),
      ),
    );
    assert_eq!(app.ui.download.wave, WaveState::Loading);
  }

  #[test]
  fn a_saved_trim_is_read_back_and_a_full_one_is_removed() {
    let mut app = app();
    let track = track();
    send(&mut app, DownloadMessage::Open(track.clone()));
    send(&mut app, DownloadMessage::MoveHandle(Handle::Start, 10.0));
    send(&mut app, DownloadMessage::MoveHandle(Handle::End, 20.0));
    send(&mut app, DownloadMessage::SaveTrim);
    assert_eq!(app.saved_trim(&track), Some((10.0, 20.0)));
    send(&mut app, DownloadMessage::ResetTrim);
    assert_eq!(app.saved_trim(&track), None);
  }

  #[test]
  fn opening_again_takes_the_saved_trim() {
    let mut app = app();
    let track = track();
    app.data.track_trims.insert(
      track.canonical_id(&app.data.platform),
      serde_json::json!({ "start": 5.0, "end": 50.0 }),
    );
    send(&mut app, DownloadMessage::Open(track));
    let range = app.ui.download.trim.unwrap();
    assert_eq!((range.start, range.end), (5.0, 50.0));
  }

  #[test]
  fn a_trim_of_less_than_a_second_and_a_half_is_ignored_like_riff() {
    let mut app = app();
    let track = track();
    app.data.track_trims.insert(
      track.canonical_id(&app.data.platform),
      serde_json::json!({ "start": 5.0, "end": 6.0 }),
    );
    assert_eq!(app.saved_trim(&track), None);
  }

  #[test]
  fn the_preview_of_the_current_track_seeks_and_stops_at_the_end() {
    let mut app = app();
    let track = track();
    app.player.current = Some(*track.clone());
    send(&mut app, DownloadMessage::Open(track));
    send(&mut app, DownloadMessage::MoveHandle(Handle::Start, 10.0));
    send(&mut app, DownloadMessage::MoveHandle(Handle::End, 20.0));
    let effects = send(&mut app, DownloadMessage::Preview);
    assert_eq!(
      effects,
      [
        Effect::AudioSeek(Duration::from_secs(10)),
        Effect::AudioPlay
      ]
    );
    assert!(!app.preview_reached(15.0));
    assert!(app.preview_reached(20.0));
    assert!(!app.preview_reached(21.0), "une seule fois");
  }

  fn named(id: &str, platform: Option<&str>) -> Track {
    Track {
      id: Some(id.to_string()),
      title: Some(id.to_string()),
      platform: platform.map(str::to_string),
      path: platform.map(|_| format!("D:/{id}.mp3")),
      url: Some(format!("https://www.youtube.com/watch?v={id}")),
      ..Track::default()
    }
  }

  /// Playlist ouverte dans le détail, puis « Download all ».
  fn open_all(app: &mut App, tracks: Vec<Track>) -> Vec<Effect> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let id = app.data.create_playlist("P", tracks, 1, unique).unwrap();
    app.ui.playlist_id = Some(id);
    send(app, DownloadMessage::OpenAll)
  }

  fn remote(ids: &[&str]) -> Vec<Track> {
    ids.iter().map(|id| named(id, None)).collect()
  }

  fn runs(effects: &[Effect]) -> Vec<&DownloadJob> {
    effects
      .iter()
      .filter_map(|effect| match effect {
        Effect::RunDownload(job) => Some(&**job),
        _ => None,
      })
      .collect()
  }

  fn toast_of(effects: &[Effect]) -> Option<(&str, crate::widgets::toast::ToastKind)> {
    effects.iter().find_map(|effect| match effect {
      Effect::Toast(text, kind) => Some((text.as_str(), *kind)),
      _ => None,
    })
  }

  /// Termine le titre en cours et rend les effets de la suite.
  fn finish(app: &mut App, result: Result<String, String>) -> Vec<Effect> {
    let token = app.ui.download.token;
    send(app, DownloadMessage::Finished(token, result))
  }

  #[test]
  fn the_local_titles_are_left_out_of_the_batch() {
    let mut app = app();
    let mut tracks = remote(&["a", "b"]);
    tracks.insert(1, named("c", Some("local")));
    open_all(&mut app, tracks);
    let batch = app.ui.download.batch.as_ref().unwrap();
    let ids: Vec<_> = batch.tracks.iter().map(|t| t.id.as_deref()).collect();
    assert_eq!(ids, [Some("a"), Some("b")]);
    assert_eq!(app.ui.modal, Some(Modal::Download));
  }

  #[test]
  fn download_all_of_the_favorites_skips_local_titles() {
    let mut app = app();
    let mut tracks = remote(&["a", "b"]);
    tracks.insert(1, named("c", Some("local")));
    app.data.favorites = tracks;
    send(&mut app, DownloadMessage::OpenAllFavorites);
    let batch = app.ui.download.batch.as_ref().unwrap();
    let ids: Vec<_> = batch.tracks.iter().map(|t| t.id.as_deref()).collect();
    assert_eq!(ids, [Some("a"), Some("b")]);
    assert_eq!(batch.name, "Liked songs");
    assert_eq!(app.ui.modal, Some(Modal::Download));
  }

  #[test]
  fn download_all_of_local_only_favorites_shows_a_toast() {
    let mut app = app();
    app.data.favorites = vec![named("c", Some("local"))];
    let effects = send(&mut app, DownloadMessage::OpenAllFavorites);
    assert_eq!(
      toast_of(&effects).map(|(text, _)| text),
      Some("No remote tracks to download in liked songs")
    );
    assert!(app.ui.download.batch.is_none());
  }

  #[test]
  fn a_playlist_without_a_remote_title_only_shows_the_toast_of_riff() {
    let mut app = app();
    let effects = open_all(&mut app, vec![named("c", Some("local"))]);
    assert_eq!(
      effects,
      [Effect::Toast(
        "No remote tracks to download in this playlist".to_string(),
        crate::widgets::toast::ToastKind::Info
      )]
    );
    assert_eq!(app.ui.modal, None);
    assert!(app.ui.download.batch.is_none());
  }

  #[test]
  fn the_batch_downloads_one_title_at_a_time_in_order() {
    let mut app = app();
    open_all(&mut app, remote(&["a", "b", "c"]));
    assert_eq!(app.ui.download.state, DownloadState::Idle);
    let mut seen = Vec::new();
    let mut effects = send(&mut app, DownloadMessage::Start);
    loop {
      let jobs = runs(&effects);
      assert_eq!(jobs.len(), 1, "jamais deux téléchargements en même temps");
      assert_eq!(app.ui.download.state, DownloadState::Running);
      seen.push(jobs[0].track.id.clone().unwrap());
      assert!(send(&mut app, DownloadMessage::Start).is_empty());
      effects = finish(&mut app, Ok("x.mp3".to_string()));
      if runs(&effects).is_empty() {
        break;
      }
    }
    assert_eq!(seen, ["a", "b", "c"]);
    assert_eq!(app.ui.download.state, DownloadState::Idle);
  }

  #[test]
  fn a_failure_does_not_stop_the_batch() {
    let mut app = app();
    open_all(&mut app, remote(&["a", "b", "c"]));
    send(&mut app, DownloadMessage::Start);
    let effects = finish(&mut app, Err("réseau".to_string()));
    assert_eq!(runs(&effects)[0].track.id.as_deref(), Some("b"));
    let effects = finish(&mut app, Ok("b.mp3".to_string()));
    assert_eq!(runs(&effects)[0].track.id.as_deref(), Some("c"));
  }

  #[test]
  fn cancel_stops_after_the_current_title_without_aborting_it() {
    let mut app = app();
    open_all(&mut app, remote(&["a", "b", "c"]));
    send(&mut app, DownloadMessage::Start);
    assert!(send(&mut app, DownloadMessage::Cancel).is_empty());
    assert_eq!(app.ui.download.state, DownloadState::Running);
    let effects = finish(&mut app, Ok("a.mp3".to_string()));
    assert!(runs(&effects).is_empty());
    assert_eq!(
      toast_of(&effects),
      Some((
        "Downloaded 1/3 (0 failed)",
        crate::widgets::toast::ToastKind::Info
      ))
    );
    assert!(app.ui.download.batch.is_none());
    assert_eq!(app.ui.modal, None);
  }

  #[test]
  fn the_final_message_counts_the_titles_and_the_failures() {
    use crate::widgets::toast::ToastKind;
    let mut app = app();
    open_all(&mut app, remote(&["a", "b", "c"]));
    send(&mut app, DownloadMessage::Start);
    finish(&mut app, Ok("a.mp3".to_string()));
    finish(&mut app, Err("réseau".to_string()));
    let effects = finish(&mut app, Ok("c.mp3".to_string()));
    assert_eq!(
      toast_of(&effects),
      Some(("Downloaded 2/3 (1 failed)", ToastKind::Error))
    );
    assert_eq!(app.ui.modal, None);
    open_all(&mut app, remote(&["a", "b"]));
    send(&mut app, DownloadMessage::Start);
    finish(&mut app, Ok("a.mp3".to_string()));
    let effects = finish(&mut app, Ok("b.mp3".to_string()));
    assert_eq!(
      toast_of(&effects),
      Some(("Downloaded 2 tracks", ToastKind::Success))
    );
  }

  #[test]
  fn the_batch_mode_has_no_trim_and_no_effects() {
    let mut app = app();
    let first = named("a", None);
    app.data.track_trims.insert(
      first.canonical_id(&app.data.platform),
      serde_json::json!({ "start": 5.0, "end": 50.0 }),
    );
    app.settings.download_format = "opus".to_string();
    app.ui.download.with_effects = true;
    open_all(&mut app, vec![first, named("b", None)]);
    send(&mut app, DownloadMessage::Quality("4".to_string()));
    assert!(send(&mut app, DownloadMessage::Tab(DownloadTab::Trim)).is_empty());
    assert_eq!(app.ui.download.tab, DownloadTab::File);
    let effects = send(&mut app, DownloadMessage::Start);
    let job = runs(&effects)[0];
    assert_eq!(job.plan, DownloadPlan::Remote);
    assert_eq!(job.trim, None);
    assert_eq!((job.format.as_str(), job.quality.as_str()), ("opus", "4"));
  }

  #[test]
  fn the_progress_shows_the_rank_of_the_running_title() {
    let mut app = app();
    assert_eq!(app.batch_progress(), None);
    open_all(&mut app, remote(&["a", "b"]));
    send(&mut app, DownloadMessage::Start);
    assert_eq!(app.batch_progress().as_deref(), Some("Downloading 1/2"));
    finish(&mut app, Ok("a.mp3".to_string()));
    assert_eq!(app.batch_progress().as_deref(), Some("Downloading 2/2"));
  }

  #[test]
  fn a_single_download_cannot_start_over_a_running_batch() {
    let mut app = app();
    open_all(&mut app, remote(&["a", "b"]));
    send(&mut app, DownloadMessage::Start);
    assert!(send(&mut app, DownloadMessage::Open(track())).is_empty());
    assert!(app.ui.download.batch.is_some());
    assert!(open_all(&mut app, remote(&["z"])).is_empty());
  }

  #[test]
  fn opening_a_single_download_forgets_an_unstarted_batch() {
    let mut app = app();
    open_all(&mut app, remote(&["a"]));
    send(&mut app, DownloadMessage::Open(track()));
    assert!(app.ui.download.batch.is_none());
  }
}
