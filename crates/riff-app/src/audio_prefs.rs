//! Réglages de son : effets, égaliseur, HQ, préréglages et sortie audio. La vue est dans `screens/audio_window.rs`.

use crate::app::{App, Effect};
use crate::settings::LegacyStorage;
use crate::widgets::toast::ToastKind;
use riff_audio::output::DeviceInfo;
use riff_audio::{AudioSettings, EqGains, HqEngine, HqSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

/// Durée de la transition entre deux préréglages (`applyPreset`).
pub const TWEEN: Duration = Duration::from_millis(400);
/// Pas de la transition.
pub const TWEEN_TICK: Duration = Duration::from_millis(16);
const DEFAULT_PRESET: &str = "p_default";

/// Bornes des curseurs de `#audio-settings-modal`.
pub const SPEED_RANGE: (f32, f32) = (0.5, 2.0);
pub const PITCH_RANGE: (f32, f32) = (-12.0, 12.0);
pub const VOLUME_RANGE: (f32, f32) = (0.0, 2.0);
pub const EFFECT_RANGE: (f32, f32) = (0.0, 100.0);
pub const EQ_RANGE: (f32, f32) = (-12.0, 12.0);
pub const HQ_RANGE: (f32, f32) = (-50.0, 50.0);

/// `eq` de `audioSettings` : les trois bandes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Eq3 {
  pub low: f32,
  pub mid: f32,
  pub high: f32,
}

/// `audioSettings` de Riff, mêmes noms de champs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sound {
  pub speed: f32,
  pub speed_pitch: f32,
  pub pitch: f32,
  pub reverb: f32,
  pub distortion: f32,
  pub volume: f32,
  pub echo: f32,
  pub eq: Eq3,
  pub eq_bands: [f32; 10],
}

impl Default for Sound {
  fn default() -> Self {
    Self {
      speed: 1.0,
      speed_pitch: 1.0,
      pitch: 0.0,
      reverb: 0.0,
      distortion: 0.0,
      volume: 1.0,
      echo: 0.0,
      eq: Eq3::default(),
      eq_bands: [0.0; 10],
    }
  }
}

fn bounded(value: f32, (low, high): (f32, f32), default: f32) -> f32 {
  if value.is_finite() {
    value.clamp(low, high)
  } else {
    default
  }
}

impl Sound {
  /// Valeurs ramenées aux bornes des curseurs. Une valeur non finie reprend le défaut.
  #[must_use]
  pub fn clamped(mut self) -> Self {
    self.speed = bounded(self.speed, SPEED_RANGE, 1.0);
    self.speed_pitch = bounded(self.speed_pitch, SPEED_RANGE, 1.0);
    self.pitch = bounded(self.pitch, PITCH_RANGE, 0.0);
    self.volume = bounded(self.volume, VOLUME_RANGE, 1.0);
    self.reverb = bounded(self.reverb, EFFECT_RANGE, 0.0);
    self.distortion = bounded(self.distortion, EFFECT_RANGE, 0.0);
    self.echo = bounded(self.echo, EFFECT_RANGE, 0.0);
    self.eq.low = bounded(self.eq.low, EQ_RANGE, 0.0);
    self.eq.mid = bounded(self.eq.mid, EQ_RANGE, 0.0);
    self.eq.high = bounded(self.eq.high, EQ_RANGE, 0.0);
    self.eq_bands = self.eq_bands.map(|band| bounded(band, EQ_RANGE, 0.0));
    self
  }

  /// Réglages du moteur.
  #[must_use]
  pub const fn to_engine(&self) -> AudioSettings {
    AudioSettings {
      speed: self.speed,
      speed_pitch: self.speed_pitch,
      pitch: self.pitch,
      reverb: self.reverb,
      distortion: self.distortion,
      volume: self.volume,
      echo: self.echo,
      eq: EqGains {
        low: self.eq.low,
        mid: self.eq.mid,
        high: self.eq.high,
        bands: self.eq_bands,
      },
    }
  }

  /// `normalizePreset` : `settings` ou `values`, `pitchSemitones` pour `pitch`, `gain` pour `volume`.
  fn from_legacy(value: &Value) -> Self {
    let source = value
      .get("settings")
      .or_else(|| value.get("values"))
      .unwrap_or(value);
    let number = |key: &str, default: f32| {
      source
        .get(key)
        .and_then(Value::as_f64)
        .map_or(default, |n| n as f32)
    };
    let eq = source.get("eq").cloned().unwrap_or(Value::Null);
    let band = |key: &str| {
      eq.get(key)
        .and_then(Value::as_f64)
        .map_or(0.0, |n| n as f32)
    };
    let mut bands = [0.0; 10];
    if let Some(list) = source.get("eqBands").and_then(Value::as_array)
      && list.len() == 10
    {
      for (slot, item) in bands.iter_mut().zip(list) {
        *slot = item.as_f64().map_or(0.0, |n| n as f32);
      }
    }
    Self {
      speed: number("speed", 1.0),
      speed_pitch: number("speedPitch", 1.0),
      pitch: source
        .get("pitch")
        .and_then(Value::as_f64)
        .map_or_else(|| number("pitchSemitones", 0.0), |n| n as f32),
      reverb: number("reverb", 0.0),
      distortion: number("distortion", 0.0),
      volume: source
        .get("volume")
        .and_then(Value::as_f64)
        .map_or_else(|| number("gain", 1.0), |n| n as f32),
      echo: number("echo", 0.0),
      eq: Eq3 {
        low: band("low"),
        mid: band("mid"),
        high: band("high"),
      },
      eq_bands: bands,
    }
    .clamped()
  }

  /// `tweenSettings` : `progress` va de 0 à 1, courbe `k * k * (3 - 2 * k)`.
  #[must_use]
  pub fn tweened(&self, to: &Self, progress: f32) -> Self {
    let k = progress.clamp(0.0, 1.0);
    let e = k * k * (3.0 - 2.0 * k);
    let mix = |a: f32, b: f32| a + (b - a) * e;
    let mut bands = self.eq_bands;
    for (band, target) in bands.iter_mut().zip(to.eq_bands) {
      *band = mix(*band, target);
    }
    Self {
      speed: mix(self.speed, to.speed),
      speed_pitch: mix(self.speed_pitch, to.speed_pitch),
      pitch: mix(self.pitch, to.pitch),
      reverb: mix(self.reverb, to.reverb),
      distortion: mix(self.distortion, to.distortion),
      volume: mix(self.volume, to.volume),
      echo: mix(self.echo, to.echo),
      eq: Eq3 {
        low: mix(self.eq.low, to.eq.low),
        mid: mix(self.eq.mid, to.eq.mid),
        high: mix(self.eq.high, to.eq.high),
      },
      eq_bands: bands,
    }
  }
}

/// `riff_hq_settings` : le champ `preset` de Riff n'a aucun effet sur le son, il est conservé.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hq {
  pub engine: String,
  pub preset: String,
  pub vocal: f32,
  pub air: f32,
  pub bass: f32,
}

impl Default for Hq {
  fn default() -> Self {
    Self {
      engine: HQ_ENGINES[0].0.to_string(),
      preset: "studio".to_string(),
      vocal: 0.0,
      air: 0.0,
      bass: 0.0,
    }
  }
}

/// Moteurs HQ : valeur enregistrée et libellé.
pub const HQ_ENGINES: [(&str, &str); 3] = [
  ("hqmusic-3", "hqmusic-3 (Harmonic Restorer)"),
  ("glyphs-7B", "glyphs-7B (High-End Upscaler)"),
  ("fyro1.1", "fyro1.1 (Dynamic Depth)"),
];

impl Hq {
  /// Réglages du moteur.
  #[must_use]
  pub fn to_engine(&self) -> HqSettings {
    HqSettings {
      engine: match self.engine.as_str() {
        "glyphs-7B" => HqEngine::Glyphs7B,
        "fyro1.1" => HqEngine::Fyro11,
        _ => HqEngine::HqMusic3,
      },
      bass: self.bass.clamp(HQ_RANGE.0, HQ_RANGE.1),
      vocal: self.vocal.clamp(HQ_RANGE.0, HQ_RANGE.1),
      air: self.air.clamp(HQ_RANGE.0, HQ_RANGE.1),
    }
  }
}

/// Préréglage de son.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
  pub id: String,
  pub name: String,
  pub settings: Sound,
}

impl Preset {
  fn from_legacy(value: &Value) -> Option<Self> {
    let id = value.get("id")?.as_str()?.to_string();
    let name = value
      .get("name")
      .and_then(Value::as_str)
      .filter(|name| !name.is_empty())
      .unwrap_or("preset")
      .to_string();
    Some(Self {
      id,
      name,
      settings: Sound::from_legacy(value),
    })
  }
}

fn preset(id: &str, name: &str, settings: Sound) -> Preset {
  Preset {
    id: id.to_string(),
    name: name.to_string(),
    settings,
  }
}

/// Six préréglages de Riff (`defaultPresets`, `app.js:651`).
#[must_use]
pub fn default_presets() -> Vec<Preset> {
  let base = Sound::default();
  vec![
    preset(DEFAULT_PRESET, "Default", base),
    preset(
      "p_speedup",
      "Speed up (orig pitch)",
      Sound {
        speed: 1.35,
        ..base
      },
    ),
    preset(
      "p_speedup_pitch",
      "Speed up",
      Sound {
        speed_pitch: 1.30,
        ..base
      },
    ),
    preset(
      "p_slowed",
      "Slowed + reverb",
      Sound {
        speed_pitch: 0.82,
        reverb: 65.0,
        echo: 15.0,
        ..base
      },
    ),
    preset(
      "p_nightcore",
      "Nightcore",
      Sound {
        speed_pitch: 1.25,
        reverb: 20.0,
        ..base
      },
    ),
    preset(
      "p_distortion",
      "Distortion",
      Sound {
        distortion: 75.0,
        volume: 1.25,
        ..base
      },
    ),
  ]
}

/// Réglages de son de `native-settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SoundPrefs {
  pub persist: bool,
  pub smooth_presets: bool,
  pub audio: Sound,
  pub hq: Hq,
  pub hq_enabled: bool,
  pub active_preset: String,
  pub custom_presets: Vec<Preset>,
  pub output_device: Option<String>,
  pub connected_audio: bool,
}

impl Default for SoundPrefs {
  fn default() -> Self {
    Self {
      persist: false,
      smooth_presets: true,
      audio: Sound::default(),
      hq: Hq::default(),
      hq_enabled: false,
      active_preset: DEFAULT_PRESET.to_string(),
      custom_presets: Vec::new(),
      output_device: None,
      connected_audio: true,
    }
  }
}

impl SoundPrefs {
  /// Clés de Riff : `riff_audio_settings`, `riff_hq_settings`, `riff_hq_enabled`, `riff_active_preset`,
  /// `devsize_custom_presets`, `riff_persist_sound_settings`, `riff_smooth_presets`, `riff_audio_output_device`,
  /// `riff_adv_connected_audio`.
  #[must_use]
  pub fn imported_from(storage: &LegacyStorage) -> Self {
    let text = |key: &str| storage.get(key).and_then(Value::as_str);
    let json = |key: &str| text(key).and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    let defaults = Self::default();
    let presets = json("devsize_custom_presets").and_then(|value| {
      value
        .as_array()
        .map(|items| items.iter().filter_map(Preset::from_legacy).collect())
    });
    let hq = json("riff_hq_settings").and_then(|value| serde_json::from_value::<Hq>(value).ok());
    Self {
      persist: text("riff_persist_sound_settings") == Some("1"),
      smooth_presets: text("riff_smooth_presets") != Some("false"),
      audio: json("riff_audio_settings").map_or(defaults.audio, |value| Sound::from_legacy(&value)),
      hq: hq.unwrap_or(defaults.hq),
      hq_enabled: text("riff_hq_enabled") == Some("1"),
      active_preset: text("riff_active_preset").map_or(defaults.active_preset, str::to_string),
      custom_presets: presets.unwrap_or_default(),
      output_device: text("riff_audio_output_device")
        .filter(|id| *id != "default")
        .map(str::to_string),
      connected_audio: text("riff_adv_connected_audio") != Some("false"),
    }
  }
}

/// Onglets de la fenêtre audio (`data-audio-tab`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AudioTab {
  Enhance,
  #[default]
  Tempo,
  Effects,
  Equalizer,
  Sleep,
  /// Absent de Riff : sortie audio, fondu enchaîné et préréglages, après les cinq onglets de Riff.
  Output,
}

impl AudioTab {
  pub const ALL: [Self; 6] = [
    Self::Enhance,
    Self::Tempo,
    Self::Effects,
    Self::Equalizer,
    Self::Sleep,
    Self::Output,
  ];

  #[must_use]
  pub const fn label(self) -> &'static str {
    match self {
      Self::Enhance => "Enhancer",
      Self::Tempo => "Speed & Pitch",
      Self::Effects => "Audio FX",
      Self::Equalizer => "Equalizer",
      Self::Output => "Output",
      Self::Sleep => "Sleep",
    }
  }
}

/// Durée du fondu de la minuterie de sommeil (`fadeOutAndPause`).
pub const SLEEP_FADE: Duration = Duration::from_secs(4);
/// Montée qui rétablit le gain maître après la pause : `set_volume` serait ignoré tant que le fondu du moteur dure.
const SLEEP_RESTORE: f32 = 0.05;
/// Durées proposées, en minutes.
pub const SLEEP_MINUTES: [u32; 4] = [15, 30, 45, 60];

/// `state.sleepTimer` : en mémoire seulement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SleepTimer {
  #[default]
  Off,
  Minutes {
    minutes: u32,
    ends_at: Instant,
  },
  EndOfTrack,
}

/// Bouton de l'onglet Sleep (`data-sleep-minutes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepChoice {
  Minutes(u32),
  EndOfTrack,
  Off,
}

impl SleepTimer {
  /// Texte du statut : « Off », « m:ss » ou « End of track » (`updateSleepTimerUI`).
  #[must_use]
  pub fn status(self, now: Instant, translate: impl Fn(&str) -> String) -> String {
    match self {
      Self::Off => translate("Off"),
      Self::EndOfTrack => translate("End of track"),
      Self::Minutes { ends_at, .. } => {
        let left = ends_at.saturating_duration_since(now).as_secs();
        format!("{}:{:02}", left / 60, left % 60)
      }
    }
  }

  /// Le bouton `choice` est celui de la minuterie en cours.
  #[must_use]
  pub fn selects(self, choice: SleepChoice) -> bool {
    match (self, choice) {
      (Self::Off, SleepChoice::Off) | (Self::EndOfTrack, SleepChoice::EndOfTrack) => true,
      (Self::Minutes { minutes, .. }, SleepChoice::Minutes(chosen)) => minutes == chosen,
      _ => false,
    }
  }
}

/// Champ d'effet réglé par un curseur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundField {
  Speed,
  SpeedPitch,
  Pitch,
  Volume,
  Reverb,
  Distortion,
  Echo,
  Low,
  Mid,
  High,
  Band(usize),
  HqVocal,
  HqAir,
  HqBass,
}

impl SoundField {
  /// Bornes et pas du curseur.
  #[must_use]
  pub const fn range(self) -> (f32, f32, f32) {
    match self {
      Self::Speed | Self::SpeedPitch => (SPEED_RANGE.0, SPEED_RANGE.1, 0.05),
      Self::Pitch => (PITCH_RANGE.0, PITCH_RANGE.1, 1.0),
      Self::Volume => (VOLUME_RANGE.0, VOLUME_RANGE.1, 0.01),
      Self::Reverb | Self::Distortion | Self::Echo => (EFFECT_RANGE.0, EFFECT_RANGE.1, 1.0),
      Self::Low | Self::Mid | Self::High | Self::Band(_) => (EQ_RANGE.0, EQ_RANGE.1, 1.0),
      Self::HqVocal | Self::HqAir | Self::HqBass => (HQ_RANGE.0, HQ_RANGE.1, 1.0),
    }
  }
}

/// Saisie d'un nom de préréglage : nouveau, ou renommage de `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetEdit {
  pub id: Option<String>,
  pub name: String,
}

/// Transition en cours entre deux préréglages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
  pub from: Sound,
  pub to: Sound,
  pub elapsed: Duration,
}

/// État de la fenêtre audio et son courant.
#[derive(Debug, Clone, Default)]
pub struct AudioUi {
  pub tab: AudioTab,
  pub live: Sound,
  pub tween: Option<Tween>,
  pub edit: Option<PresetEdit>,
  pub devices: Vec<DeviceInfo>,
  pub sleep: SleepTimer,
  /// Numéro de l'échéance en cours : une échéance plus ancienne est ignorée.
  pub sleep_generation: u64,
  /// Ouverture, fermeture, changement d'onglet et point du minuteur (`styles.css:7585-7592`, `7679`, `9887`).
  pub motion: crate::motion::AudioMotion,
}

/// Messages de la fenêtre audio.
#[derive(Debug, Clone)]
pub enum AudioMessage {
  Open,
  Tab(AudioTab),
  Set(SoundField, f32),
  Commit,
  ToggleHq(bool),
  HqEngine(String),
  ApplyPreset(String),
  TweenTick,
  NewPreset,
  RenamePreset(String),
  PresetName(String),
  PresetSave,
  PresetCancel,
  DeletePreset(String),
  ResetBands,
  /// `#btn-reset-audio` : réglages, HQ, préréglage actif et normalisation remis aux défauts.
  ResetAll,
  TogglePersist(bool),
  ToggleSmooth(bool),
  Devices(Vec<DeviceInfo>),
  Device(Option<String>),
  Sleep(SleepChoice),
  /// Échéance de la génération donnée.
  SleepDue(u64),
  /// Fin du fondu : pause puis gain rétabli.
  SleepFaded,
  /// Rafraîchit le compte à rebours affiché.
  SleepTick,
}

impl App {
  /// Préréglages par défaut puis préréglages de l'utilisateur.
  #[must_use]
  pub fn all_presets(&self) -> Vec<Preset> {
    default_presets()
      .into_iter()
      .chain(self.settings.sound.custom_presets.iter().cloned())
      .collect()
  }

  /// Valeur d'un curseur de son.
  #[must_use]
  pub fn sound_value(&self, field: SoundField) -> f32 {
    let live = &self.ui.audio.live;
    let hq = &self.settings.sound.hq;
    match field {
      SoundField::Speed => live.speed,
      SoundField::SpeedPitch => live.speed_pitch,
      SoundField::Pitch => live.pitch,
      SoundField::Volume => live.volume,
      SoundField::Reverb => live.reverb,
      SoundField::Distortion => live.distortion,
      SoundField::Echo => live.echo,
      SoundField::Low => live.eq.low,
      SoundField::Mid => live.eq.mid,
      SoundField::High => live.eq.high,
      SoundField::Band(index) => live.eq_bands.get(index).copied().unwrap_or(0.0),
      SoundField::HqVocal => hq.vocal,
      SoundField::HqAir => hq.air,
      SoundField::HqBass => hq.bass,
    }
  }

  /// `loadSoundSettings` : sans « Save sound settings », chaque lancement repart des défauts.
  pub(crate) fn startup_sound(&mut self) -> Vec<Effect> {
    let sound = &mut self.settings.sound;
    if !sound.persist {
      sound.audio = Sound::default();
      sound.hq_enabled = false;
      sound.hq = Hq::default();
      DEFAULT_PRESET.clone_into(&mut sound.active_preset);
    }
    self.ui.audio.live = self.settings.sound.audio;
    let mut effects = Vec::new();
    if self.ui.audio.live != Sound::default() {
      effects.push(Effect::AudioSound(Box::new(self.ui.audio.live.to_engine())));
    }
    if self.settings.sound.hq_enabled {
      effects.push(self.hq_effect());
    }
    if let Some(id) = self.settings.sound.output_device.clone() {
      effects.push(Effect::AudioDevice(Some(id)));
    }
    effects
  }

  fn hq_effect(&self) -> Effect {
    Effect::AudioHq(
      self.settings.sound.hq_enabled,
      self.settings.sound.hq.to_engine(),
    )
  }

  fn persist_sound(&mut self) -> Vec<Effect> {
    self.settings.sound.audio = self.ui.audio.live;
    vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
  }

  fn set_sound(&mut self, field: SoundField, value: f32) -> Vec<Effect> {
    let (low, high, _) = field.range();
    let value = if value.is_finite() {
      value.clamp(low, high)
    } else {
      return Vec::new();
    };
    self.ui.audio.tween = None;
    let live = &mut self.ui.audio.live;
    match field {
      SoundField::Speed => live.speed = value,
      SoundField::SpeedPitch => live.speed_pitch = value,
      SoundField::Pitch => live.pitch = value,
      SoundField::Volume => live.volume = value,
      SoundField::Reverb => live.reverb = value,
      SoundField::Distortion => live.distortion = value,
      SoundField::Echo => live.echo = value,
      SoundField::Low => live.eq.low = value,
      SoundField::Mid => live.eq.mid = value,
      SoundField::High => live.eq.high = value,
      SoundField::Band(index) => {
        if let Some(band) = live.eq_bands.get_mut(index) {
          *band = value;
        }
      }
      SoundField::HqVocal => self.settings.sound.hq.vocal = value,
      SoundField::HqAir => self.settings.sound.hq.air = value,
      SoundField::HqBass => self.settings.sound.hq.bass = value,
    }
    if matches!(
      field,
      SoundField::HqVocal | SoundField::HqAir | SoundField::HqBass
    ) {
      return vec![self.hq_effect()];
    }
    vec![Effect::AudioSound(Box::new(self.ui.audio.live.to_engine()))]
  }

  fn apply_preset(&mut self, id: &str) -> Vec<Effect> {
    let Some(preset) = self
      .all_presets()
      .into_iter()
      .find(|preset| preset.id == id)
    else {
      return Vec::new();
    };
    id.clone_into(&mut self.settings.sound.active_preset);
    if self.settings.sound.smooth_presets {
      self.ui.audio.tween = Some(Tween {
        from: self.ui.audio.live,
        to: preset.settings,
        elapsed: Duration::ZERO,
      });
      return vec![Effect::SaveSettings(Box::new(self.settings.clone()))];
    }
    self.ui.audio.tween = None;
    self.ui.audio.live = preset.settings;
    let mut effects = vec![Effect::AudioSound(Box::new(preset.settings.to_engine()))];
    effects.extend(self.persist_sound());
    effects
  }

  fn tween_tick(&mut self) -> Vec<Effect> {
    let Some(tween) = &mut self.ui.audio.tween else {
      return Vec::new();
    };
    tween.elapsed += TWEEN_TICK;
    let progress = tween.elapsed.as_secs_f32() / TWEEN.as_secs_f32();
    let current = tween.from.tweened(&tween.to, progress);
    let done = progress >= 1.0;
    self.ui.audio.live = if done { tween.to } else { current };
    let mut effects = vec![Effect::AudioSound(Box::new(self.ui.audio.live.to_engine()))];
    if done {
      self.ui.audio.tween = None;
      effects.extend(self.persist_sound());
    }
    effects
  }

  fn save_preset_edit(&mut self) -> Vec<Effect> {
    let Some(edit) = self.ui.audio.edit.take() else {
      return Vec::new();
    };
    let name = edit.name.trim().to_string();
    if name.is_empty() {
      return Vec::new();
    }
    match edit.id {
      Some(id) => {
        if let Some(preset) = self
          .settings
          .sound
          .custom_presets
          .iter_mut()
          .find(|preset| preset.id == id)
        {
          preset.name = name;
        }
      }
      None => {
        // Identifiant unique : `custom_` puis un compteur, comme le suffixe aléatoire de Riff.
        let id = format!(
          "custom_{:x}{:x}",
          riff_core::util::now_millis(),
          self.next_unique()
        );
        self.settings.sound.custom_presets.push(Preset {
          id: id.clone(),
          name,
          settings: self.ui.audio.live,
        });
        self.settings.sound.active_preset = id;
      }
    }
    self.persist_sound()
  }

  /// `btn-reset-audio` (`app.js:18980`).
  fn reset_all(&mut self) -> Vec<Effect> {
    let sound = &mut self.settings.sound;
    sound.hq_enabled = false;
    sound.hq = Hq::default();
    DEFAULT_PRESET.clone_into(&mut sound.active_preset);
    self.ui.audio.tween = None;
    self.ui.audio.live = Sound::default();
    let mut effects = vec![
      Effect::AudioSound(Box::new(self.ui.audio.live.to_engine())),
      self.hq_effect(),
    ];
    effects.extend(self.persist_sound());
    effects.extend(self.update_settings(crate::prefs::SettingsMessage::SetBool(
      crate::prefs::BoolKey::Normalize,
      false,
    )));
    effects.push(Effect::Toast(
      self.i18n.t("Audio settings reset"),
      ToastKind::Info,
    ));
    effects
  }

  /// Messages de la fenêtre audio.
  pub fn update_audio(&mut self, message: AudioMessage) -> Vec<Effect> {
    match message {
      AudioMessage::Open => {
        let mut effects = self.open_modal(crate::ui::Modal::Audio);
        if self.overlay_motion.enabled {
          self.now = Instant::now();
          self.ui.audio.motion.open(self.now);
        }
        effects.push(Effect::ListSettingsDevices);
        effects
      }
      AudioMessage::Tab(tab) => {
        // Cliquer l'onglet déjà actif ne change aucune classe : `audioPanelIn` ne rejoue pas.
        if self.overlay_motion.enabled && tab != self.ui.audio.tab {
          self.now = Instant::now();
          self
            .ui
            .audio
            .motion
            .switch_panel(self.now, tab == AudioTab::Sleep);
        }
        self.ui.audio.tab = tab;
        Vec::new()
      }
      AudioMessage::Set(field, value) => self.set_sound(field, value),
      AudioMessage::Commit => self.persist_sound(),
      AudioMessage::ToggleHq(enabled) => {
        self.settings.sound.hq_enabled = enabled;
        let mut effects = vec![self.hq_effect()];
        effects.extend(self.persist_sound());
        effects
      }
      AudioMessage::HqEngine(engine) => {
        self.settings.sound.hq.engine = engine;
        let mut effects = vec![self.hq_effect()];
        effects.extend(self.persist_sound());
        effects
      }
      AudioMessage::ApplyPreset(id) => self.apply_preset(&id),
      AudioMessage::TweenTick => self.tween_tick(),
      AudioMessage::NewPreset => {
        self.ui.audio.edit = Some(PresetEdit {
          id: None,
          name: String::new(),
        });
        Vec::new()
      }
      AudioMessage::RenamePreset(id) => {
        let name = self
          .settings
          .sound
          .custom_presets
          .iter()
          .find(|preset| preset.id == id)
          .map(|preset| preset.name.clone());
        if let Some(name) = name {
          self.ui.audio.edit = Some(PresetEdit { id: Some(id), name });
        }
        Vec::new()
      }
      AudioMessage::PresetName(name) => {
        if let Some(edit) = &mut self.ui.audio.edit {
          edit.name = name;
        }
        Vec::new()
      }
      AudioMessage::PresetSave => self.save_preset_edit(),
      AudioMessage::PresetCancel => {
        self.ui.audio.edit = None;
        Vec::new()
      }
      AudioMessage::DeletePreset(id) => {
        self
          .settings
          .sound
          .custom_presets
          .retain(|preset| preset.id != id);
        if self.settings.sound.active_preset == id {
          DEFAULT_PRESET.clone_into(&mut self.settings.sound.active_preset);
        }
        self.persist_sound()
      }
      AudioMessage::ResetBands => {
        self.ui.audio.tween = None;
        self.ui.audio.live.eq_bands = [0.0; 10];
        let mut effects = vec![Effect::AudioSound(Box::new(self.ui.audio.live.to_engine()))];
        effects.extend(self.persist_sound());
        effects
      }
      AudioMessage::ResetAll => self.reset_all(),
      AudioMessage::TogglePersist(persist) => {
        self.settings.sound.persist = persist;
        self.persist_sound()
      }
      AudioMessage::ToggleSmooth(smooth) => {
        self.settings.sound.smooth_presets = smooth;
        self.persist_sound()
      }
      AudioMessage::Devices(devices) => {
        self.ui.audio.devices = devices;
        Vec::new()
      }
      AudioMessage::Device(id) => {
        self.settings.sound.output_device.clone_from(&id);
        let mut effects = vec![Effect::AudioDevice(id)];
        effects.extend(self.persist_sound());
        effects
      }
      AudioMessage::Sleep(choice) => self.set_sleep(choice),
      AudioMessage::SleepDue(generation) => self.sleep_due(generation),
      AudioMessage::SleepFaded => {
        let mut effects = self.sleep_pause();
        effects.push(Effect::AudioFadeIn(SLEEP_RESTORE));
        effects
      }
      AudioMessage::SleepTick => Vec::new(),
    }
  }

  /// `setSleepTimer`.
  fn set_sleep(&mut self, choice: SleepChoice) -> Vec<Effect> {
    // `.active` arrive sur `#sleep-timer-status` quand la minuterie passe de coupée à posée ; elle reste posée d'un choix à l'autre.
    if self.overlay_motion.enabled
      && self.ui.audio.sleep == SleepTimer::Off
      && choice != SleepChoice::Off
    {
      self.now = Instant::now();
      self.ui.audio.motion.start_dot(self.now);
    }
    let audio = &mut self.ui.audio;
    audio.sleep_generation += 1;
    match choice {
      SleepChoice::Off => {
        audio.sleep = SleepTimer::Off;
        Vec::new()
      }
      SleepChoice::EndOfTrack => {
        audio.sleep = SleepTimer::EndOfTrack;
        vec![Effect::Toast(
          self.i18n.t("Sleep timer: end of track"),
          ToastKind::Info,
        )]
      }
      SleepChoice::Minutes(minutes) => {
        let length = Duration::from_secs(u64::from(minutes) * 60);
        audio.sleep = SleepTimer::Minutes {
          minutes,
          ends_at: Instant::now() + length,
        };
        let text = format!(
          "{}: {minutes} {}",
          self.i18n.t("Sleep timer"),
          self.i18n.t("min")
        );
        vec![
          Effect::SleepAlarm(audio.sleep_generation, length),
          Effect::Toast(text, ToastKind::Info),
        ]
      }
    }
  }

  /// Échéance en minutes : `fadeOutAndPause`. Sans son à couper, on met en pause tout de suite.
  fn sleep_due(&mut self, generation: u64) -> Vec<Effect> {
    let audio = &mut self.ui.audio;
    if generation != audio.sleep_generation || !matches!(audio.sleep, SleepTimer::Minutes { .. }) {
      return Vec::new();
    }
    audio.sleep = SleepTimer::Off;
    if self.data.muted || self.data.volume <= 0.0 {
      return self.sleep_pause();
    }
    vec![
      Effect::AudioFadeOut(SLEEP_FADE.as_secs_f32()),
      Effect::SleepFadeWait(SLEEP_FADE),
    ]
  }

  fn sleep_pause(&mut self) -> Vec<Effect> {
    self.playback.is_playing = false;
    vec![Effect::AudioPause, self.state_effect(false)]
  }

  /// Minuterie « fin de titre » armée.
  pub(crate) fn sleep_at_end(&self) -> bool {
    self.ui.audio.sleep == SleepTimer::EndOfTrack
  }

  /// Fin de titre atteinte avec la minuterie « fin de titre » : pause, minuterie levée.
  pub(crate) fn sleep_stop(&mut self) -> Vec<Effect> {
    self.ui.audio.sleep = SleepTimer::Off;
    self.sleep_pause()
  }

  /// Une animation de la fenêtre audio demande l'image suivante : carte, panneau, ou point du minuteur (infini tant qu'il est affiché).
  #[must_use]
  pub fn audio_animating(&self) -> bool {
    let audio = &self.ui.audio;
    audio.motion.running(self.now)
      || (self.ui.modal == Some(crate::ui::Modal::Audio)
        && audio.tab == AudioTab::Sleep
        && audio.sleep != SleepTimer::Off
        && audio.motion.dot_started())
  }

  /// Le compte à rebours n'est affiché que dans l'onglet Sleep de la fenêtre audio.
  #[must_use]
  pub fn sleep_countdown_visible(&self) -> bool {
    self.ui.modal == Some(crate::ui::Modal::Audio)
      && self.ui.audio.tab == AudioTab::Sleep
      && matches!(self.ui.audio.sleep, SleepTimer::Minutes { .. })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::Message;
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::window::PersistedWindowState;
  use std::fs;

  fn app() -> App {
    App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
  }

  fn send(app: &mut App, message: AudioMessage) -> Vec<Effect> {
    app.update(Message::AudioPrefs(message))
  }

  fn storage(content: &str) -> LegacyStorage {
    let dir = crate::test_temp::TempDir::new("audio-prefs");
    let path = dir.join(format!("{:x}.json", content.len()));
    fs::write(&path, content).unwrap();
    LegacyStorage::open(&path)
  }

  fn assert_close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
  }

  fn engine_of(effects: &[Effect]) -> AudioSettings {
    effects
      .iter()
      .find_map(|effect| match effect {
        Effect::AudioSound(settings) => Some(**settings),
        _ => None,
      })
      .expect("réglages envoyés au moteur")
  }

  #[test]
  fn the_six_default_presets_match_riff_field_by_field() {
    let presets = default_presets();
    let names: Vec<&str> = presets.iter().map(|preset| preset.name.as_str()).collect();
    assert_eq!(
      names,
      [
        "Default",
        "Speed up (orig pitch)",
        "Speed up",
        "Slowed + reverb",
        "Nightcore",
        "Distortion"
      ]
    );
    assert_eq!(presets[0].settings, Sound::default());
    assert_close(presets[1].settings.speed, 1.35);
    assert_close(presets[2].settings.speed_pitch, 1.30);
    assert_close(presets[3].settings.speed_pitch, 0.82);
    assert_close(presets[3].settings.reverb, 65.0);
    assert_close(presets[3].settings.echo, 15.0);
    assert_close(presets[4].settings.speed_pitch, 1.25);
    assert_close(presets[4].settings.reverb, 20.0);
    assert_close(presets[5].settings.distortion, 75.0);
    assert_close(presets[5].settings.volume, 1.25);
  }

  #[test]
  fn applying_a_preset_without_transition_sends_each_field_to_the_engine() {
    let mut app = app();
    app.settings.sound.smooth_presets = false;
    let custom = Sound {
      speed: 1.1,
      speed_pitch: 1.2,
      pitch: 3.0,
      reverb: 40.0,
      distortion: 10.0,
      volume: 0.9,
      echo: 5.0,
      eq: Eq3 {
        low: 1.0,
        mid: 2.0,
        high: 3.0,
      },
      eq_bands: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
    };
    app.settings.sound.custom_presets.push(Preset {
      id: "c".to_string(),
      name: "c".to_string(),
      settings: custom,
    });
    let engine = engine_of(&send(&mut app, AudioMessage::ApplyPreset("c".to_string())));
    assert_close(engine.speed, 1.1);
    assert_close(engine.speed_pitch, 1.2);
    assert_close(engine.pitch, 3.0);
    assert_close(engine.reverb, 40.0);
    assert_close(engine.distortion, 10.0);
    assert_close(engine.volume, 0.9);
    assert_close(engine.echo, 5.0);
    assert_close(engine.eq.low, 1.0);
    assert_close(engine.eq.mid, 2.0);
    assert_close(engine.eq.high, 3.0);
    assert_eq!(engine.eq.bands, custom.eq_bands);
    assert_eq!(app.settings.sound.active_preset, "c");
  }

  #[test]
  fn a_smooth_transition_reaches_the_preset_in_400_ms() {
    let mut app = app();
    assert!(app.settings.sound.smooth_presets);
    send(
      &mut app,
      AudioMessage::ApplyPreset("p_distortion".to_string()),
    );
    assert!(app.ui.audio.tween.is_some());
    assert_close(app.ui.audio.live.distortion, 0.0);
    let mut ticks = 0;
    let mut middle = None;
    while app.ui.audio.tween.is_some() {
      send(&mut app, AudioMessage::TweenTick);
      ticks += 1;
      if ticks == 12 {
        middle = Some(app.ui.audio.live.distortion);
      }
    }
    assert_eq!(ticks, 25);
    let middle = middle.unwrap();
    assert!(middle > 10.0 && middle < 65.0, "{middle}");
    assert_close(app.ui.audio.live.distortion, 75.0);
    assert_close(app.ui.audio.live.volume, 1.25);
    assert_close(app.settings.sound.audio.distortion, 75.0);
  }

  #[test]
  fn the_curve_of_the_transition_is_the_one_of_riff() {
    let (from, to) = (
      Sound::default(),
      Sound {
        reverb: 100.0,
        ..Sound::default()
      },
    );
    assert_close(from.tweened(&to, 0.0).reverb, 0.0);
    assert_close(from.tweened(&to, 0.25).reverb, 15.625);
    assert_close(from.tweened(&to, 0.5).reverb, 50.0);
    assert_close(from.tweened(&to, 1.0).reverb, 100.0);
    assert_close(from.tweened(&to, 7.0).reverb, 100.0);
  }

  #[test]
  fn moving_a_slider_cancels_the_transition() {
    let mut app = app();
    send(
      &mut app,
      AudioMessage::ApplyPreset("p_distortion".to_string()),
    );
    send(&mut app, AudioMessage::Set(SoundField::Echo, 5.0));
    assert!(app.ui.audio.tween.is_none());
  }

  #[test]
  fn every_slider_stays_inside_its_bounds_and_reads_back() {
    let fields = [
      SoundField::Speed,
      SoundField::SpeedPitch,
      SoundField::Pitch,
      SoundField::Volume,
      SoundField::Reverb,
      SoundField::Distortion,
      SoundField::Echo,
      SoundField::Low,
      SoundField::Mid,
      SoundField::High,
      SoundField::Band(0),
      SoundField::Band(9),
      SoundField::HqVocal,
      SoundField::HqAir,
      SoundField::HqBass,
    ];
    let mut app = app();
    for field in fields {
      let (low, high, _) = field.range();
      send(&mut app, AudioMessage::Set(field, high + 100.0));
      assert_close(app.sound_value(field), high);
      send(&mut app, AudioMessage::Set(field, low - 100.0));
      assert_close(app.sound_value(field), low);
      let before = app.sound_value(field);
      send(&mut app, AudioMessage::Set(field, f32::NAN));
      assert_close(app.sound_value(field), before);
    }
  }

  #[test]
  fn the_equalizer_sends_its_gains_to_the_engine() {
    let mut app = app();
    let engine = engine_of(&send(&mut app, AudioMessage::Set(SoundField::Band(5), 6.0)));
    assert_close(engine.eq.bands[5], 6.0);
    let engine = engine_of(&send(&mut app, AudioMessage::Set(SoundField::Low, -3.0)));
    assert_close(engine.eq.low, -3.0);
    let engine = engine_of(&send(&mut app, AudioMessage::ResetBands));
    assert_eq!(engine.eq.bands, [0.0; 10]);
    assert_close(engine.eq.low, -3.0);
  }

  #[test]
  fn resetting_the_bands_cancels_a_running_transition() {
    let mut app = app();
    app.settings.sound.custom_presets.push(Preset {
      id: "c".to_string(),
      name: "c".to_string(),
      settings: Sound {
        eq_bands: [6.0; 10],
        ..Sound::default()
      },
    });
    send(&mut app, AudioMessage::ApplyPreset("c".to_string()));
    send(&mut app, AudioMessage::TweenTick);
    assert!(app.ui.audio.tween.is_some());
    assert!(app.ui.audio.live.eq_bands[0] > 0.0);
    send(&mut app, AudioMessage::ResetBands);
    assert!(app.ui.audio.tween.is_none());
    for _ in 0..30 {
      send(&mut app, AudioMessage::TweenTick);
      assert_eq!(app.ui.audio.live.eq_bands, [0.0; 10]);
    }
  }

  #[test]
  fn hq_sends_the_engine_and_its_settings() {
    let mut app = app();
    assert_eq!(
      send(&mut app, AudioMessage::ToggleHq(true))[0],
      Effect::AudioHq(true, HqSettings::default())
    );
    let effects = send(&mut app, AudioMessage::HqEngine("glyphs-7B".to_string()));
    assert!(matches!(
      effects[0],
      Effect::AudioHq(
        true,
        HqSettings {
          engine: HqEngine::Glyphs7B,
          ..
        }
      )
    ));
    let effects = send(&mut app, AudioMessage::Set(SoundField::HqBass, 20.0));
    assert!(
      matches!(effects[0], Effect::AudioHq(true, settings) if (settings.bass - 20.0).abs() < 1e-5)
    );
    assert_eq!(
      Hq {
        engine: "fyro1.1".to_string(),
        ..Hq::default()
      }
      .to_engine()
      .engine,
      HqEngine::Fyro11
    );
    assert_eq!(
      Hq {
        engine: "inconnu".to_string(),
        ..Hq::default()
      }
      .to_engine()
      .engine,
      HqEngine::HqMusic3
    );
  }

  #[test]
  fn a_preset_can_be_created_renamed_and_deleted() {
    let mut app = app();
    send(&mut app, AudioMessage::Set(SoundField::Reverb, 33.0));
    send(&mut app, AudioMessage::NewPreset);
    send(
      &mut app,
      AudioMessage::PresetName("  Mon son  ".to_string()),
    );
    let effects = send(&mut app, AudioMessage::PresetSave);
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::SaveSettings(_)))
    );
    let created = app.settings.sound.custom_presets[0].clone();
    assert_eq!(created.name, "Mon son");
    assert_close(created.settings.reverb, 33.0);
    assert_eq!(app.settings.sound.active_preset, created.id);

    send(&mut app, AudioMessage::RenamePreset(created.id.clone()));
    assert_eq!(app.ui.audio.edit.as_ref().unwrap().name, "Mon son");
    send(&mut app, AudioMessage::PresetName("Autre".to_string()));
    send(&mut app, AudioMessage::PresetSave);
    assert_eq!(app.settings.sound.custom_presets[0].name, "Autre");

    send(&mut app, AudioMessage::DeletePreset(created.id));
    assert!(app.settings.sound.custom_presets.is_empty());
    assert_eq!(app.settings.sound.active_preset, DEFAULT_PRESET);
  }

  #[test]
  fn an_empty_name_creates_nothing() {
    let mut app = app();
    send(&mut app, AudioMessage::NewPreset);
    send(&mut app, AudioMessage::PresetName("   ".to_string()));
    send(&mut app, AudioMessage::PresetSave);
    assert!(app.settings.sound.custom_presets.is_empty());
    assert!(app.ui.audio.edit.is_none());
  }

  #[test]
  fn default_presets_cannot_be_renamed() {
    let mut app = app();
    send(
      &mut app,
      AudioMessage::RenamePreset(DEFAULT_PRESET.to_string()),
    );
    assert!(app.ui.audio.edit.is_none());
  }

  #[test]
  fn the_custom_presets_of_riff_are_imported() {
    let storage = storage(
      r#"{ "devsize_custom_presets": "[{\"id\":\"custom_a\",\"name\":\"Ma basse\",\"settings\":{\"speed\":1.1,\"speedPitch\":0.9,\"pitch\":2,\"reverb\":12,\"distortion\":3,\"volume\":1.5,\"echo\":4,\"eq\":{\"low\":5,\"mid\":-1,\"high\":2},\"eqBands\":[1,2,3,4,5,6,7,8,9,10]}},{\"id\":\"old\",\"values\":{\"pitchSemitones\":-3,\"gain\":0.5,\"eqBands\":[1,2]}},{\"name\":\"sans id\"}]" }"#,
    );
    let presets = SoundPrefs::imported_from(&storage).custom_presets;
    assert_eq!(presets.len(), 2);
    assert_eq!(presets[0].name, "Ma basse");
    let sound = presets[0].settings;
    assert_close(sound.speed, 1.1);
    assert_close(sound.speed_pitch, 0.9);
    assert_close(sound.pitch, 2.0);
    assert_close(sound.reverb, 12.0);
    assert_close(sound.distortion, 3.0);
    assert_close(sound.volume, 1.5);
    assert_close(sound.echo, 4.0);
    assert_close(sound.eq.low, 5.0);
    assert_close(sound.eq.mid, -1.0);
    assert_close(sound.eq.high, 2.0);
    assert_close(sound.eq_bands[9], 10.0);
    assert_eq!(presets[1].name, "preset");
    assert_close(presets[1].settings.pitch, -3.0);
    assert_close(presets[1].settings.volume, 0.5);
    assert_eq!(
      presets[1].settings.eq_bands, [0.0; 10],
      "eqBands de 2 valeurs ignoré, comme normalizePreset"
    );
  }

  #[test]
  fn the_sound_keys_of_riff_are_imported() {
    let storage = storage(
      r#"{
        "riff_persist_sound_settings": "1",
        "riff_smooth_presets": "false",
        "riff_audio_settings": "{\"reverb\":30,\"distortion\":900}",
        "riff_hq_settings": "{\"engine\":\"glyphs-7B\",\"bass\":12}",
        "riff_hq_enabled": "1",
        "riff_active_preset": "p_nightcore",
        "riff_audio_output_device": "sortie",
        "riff_adv_connected_audio": "false"
      }"#,
    );
    let sound = SoundPrefs::imported_from(&storage);
    assert!(sound.persist);
    assert!(!sound.smooth_presets);
    assert_close(sound.audio.reverb, 30.0);
    assert_close(sound.audio.distortion, 100.0);
    assert_eq!(sound.hq.engine, "glyphs-7B");
    assert_close(sound.hq.bass, 12.0);
    assert!(sound.hq_enabled);
    assert_eq!(sound.active_preset, "p_nightcore");
    assert_eq!(sound.output_device.as_deref(), Some("sortie"));
    assert!(!sound.connected_audio);
  }

  #[test]
  fn without_persistence_the_sound_starts_from_the_defaults() {
    let mut app = app();
    app.settings.sound.audio.reverb = 50.0;
    app.settings.sound.hq_enabled = true;
    app.settings.sound.active_preset = "p_slowed".to_string();
    assert!(app.startup_sound().is_empty());
    assert_eq!(app.ui.audio.live, Sound::default());
    assert!(!app.settings.sound.hq_enabled);
    assert_eq!(app.settings.sound.active_preset, DEFAULT_PRESET);
  }

  #[test]
  fn with_persistence_the_sound_is_restored_at_startup() {
    let mut app = app();
    app.settings.sound.persist = true;
    app.settings.sound.audio.reverb = 50.0;
    app.settings.sound.hq_enabled = true;
    app.settings.sound.output_device = Some("sortie".to_string());
    let effects = app.startup_sound();
    assert_close(engine_of(&effects).reverb, 50.0);
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::AudioHq(true, _)))
    );
    assert!(effects.contains(&Effect::AudioDevice(Some("sortie".to_string()))));
  }

  #[test]
  fn choosing_an_output_device_applies_and_remembers_it() {
    let mut app = app();
    let effects = send(&mut app, AudioMessage::Device(Some("b".to_string())));
    assert_eq!(effects[0], Effect::AudioDevice(Some("b".to_string())));
    assert_eq!(app.settings.sound.output_device.as_deref(), Some("b"));
  }

  #[test]
  fn opening_the_window_lists_the_devices() {
    let mut app = app();
    let effects = send(&mut app, AudioMessage::Open);
    assert!(effects.contains(&Effect::ListSettingsDevices));
    assert_eq!(app.ui.modal, Some(crate::ui::Modal::Audio));
    app.update(Message::Escape);
    assert_eq!(app.ui.modal, None);
  }

  fn toast_text(effects: &[Effect]) -> Option<&str> {
    effects.iter().find_map(|effect| match effect {
      Effect::Toast(text, _) => Some(text.as_str()),
      _ => None,
    })
  }

  fn alarm_of(effects: &[Effect]) -> Option<(u64, Duration)> {
    effects.iter().find_map(|effect| match effect {
      Effect::SleepAlarm(generation, delay) => Some((*generation, *delay)),
      _ => None,
    })
  }

  #[test]
  fn the_five_tabs_of_riff_come_first_then_the_output_tab_of_riff_rs() {
    assert_eq!(
      AudioTab::ALL[..5],
      [
        AudioTab::Enhance,
        AudioTab::Tempo,
        AudioTab::Effects,
        AudioTab::Equalizer,
        AudioTab::Sleep
      ]
    );
    assert_eq!(AudioTab::ALL.last(), Some(&AudioTab::Output));
    assert_eq!(AudioTab::Sleep.label(), "Sleep");
    assert_eq!(AudioTab::Effects.label(), "Audio FX");
    assert_eq!(AudioTab::ALL.len(), 6);
  }

  #[test]
  fn reset_all_restores_the_defaults_like_btn_reset_audio() {
    let mut app = app();
    app.ui.audio.live.reverb = 50.0;
    app.ui.audio.live.eq_bands[3] = 4.0;
    app.settings.sound.hq_enabled = true;
    app.settings.sound.hq.vocal = 12.0;
    app.settings.sound.active_preset = "custom_1".to_string();
    app.data.normalize = true;
    let effects = send(&mut app, AudioMessage::ResetAll);
    assert_eq!(app.ui.audio.live, Sound::default());
    assert!(!app.settings.sound.hq_enabled);
    assert_eq!(app.settings.sound.hq, Hq::default());
    assert_eq!(app.settings.sound.active_preset, DEFAULT_PRESET);
    assert!(!app.data.normalize);
    assert_eq!(engine_of(&effects), Sound::default().to_engine());
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::AudioHq(false, _)))
    );
    assert!(effects.contains(&Effect::NormalizeGain(0.0)));
    assert_eq!(toast_text(&effects), Some("Audio settings reset"));
  }

  #[test]
  fn fifteen_minutes_arms_one_deadline_and_a_toast() {
    let mut app = app();
    let effects = send(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(15)));
    let (generation, delay) = alarm_of(&effects).expect("échéance");
    assert_eq!(delay, Duration::from_secs(900));
    assert_eq!(generation, app.ui.audio.sleep_generation);
    assert_eq!(toast_text(&effects), Some("Sleep timer: 15 min"));
    assert!(matches!(
      app.ui.audio.sleep,
      SleepTimer::Minutes { minutes: 15, .. }
    ));
  }

  #[test]
  fn end_of_track_arms_no_deadline() {
    let mut app = app();
    let effects = send(&mut app, AudioMessage::Sleep(SleepChoice::EndOfTrack));
    assert_eq!(alarm_of(&effects), None);
    assert_eq!(toast_text(&effects), Some("Sleep timer: end of track"));
    assert_eq!(app.ui.audio.sleep, SleepTimer::EndOfTrack);
  }

  #[test]
  fn turn_off_cancels_the_deadline() {
    let mut app = app();
    let effects = send(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(30)));
    let (generation, _) = alarm_of(&effects).unwrap();
    let off = send(&mut app, AudioMessage::Sleep(SleepChoice::Off));
    assert!(off.is_empty());
    assert_eq!(app.ui.audio.sleep, SleepTimer::Off);
    assert!(send(&mut app, AudioMessage::SleepDue(generation)).is_empty());
  }

  #[test]
  fn a_stale_deadline_does_nothing() {
    let mut app = app();
    let first = send(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(15)));
    let second = send(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(60)));
    let (old, _) = alarm_of(&first).unwrap();
    let (new, _) = alarm_of(&second).unwrap();
    assert_ne!(old, new);
    assert!(send(&mut app, AudioMessage::SleepDue(old)).is_empty());
    assert!(matches!(
      app.ui.audio.sleep,
      SleepTimer::Minutes { minutes: 60, .. }
    ));
    assert!(!send(&mut app, AudioMessage::SleepDue(new)).is_empty());
  }

  #[test]
  fn the_deadline_fades_out_then_pauses_then_restores_the_gain() {
    let mut app = app();
    app.playback.is_playing = true;
    let effects = send(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(15)));
    let (generation, _) = alarm_of(&effects).unwrap();
    let due = send(&mut app, AudioMessage::SleepDue(generation));
    assert_eq!(
      due,
      vec![Effect::AudioFadeOut(4.0), Effect::SleepFadeWait(SLEEP_FADE)]
    );
    assert_eq!(app.ui.audio.sleep, SleepTimer::Off);
    assert!(
      app.playback.is_playing,
      "pas de pause avant la fin du fondu"
    );
    let faded = send(&mut app, AudioMessage::SleepFaded);
    assert_eq!(faded.first(), Some(&Effect::AudioPause));
    assert_eq!(faded.last(), Some(&Effect::AudioFadeIn(SLEEP_RESTORE)));
    assert!(!app.playback.is_playing);
  }

  #[test]
  fn a_muted_player_pauses_at_the_deadline_without_a_fade() {
    let mut app = app();
    app.data.muted = true;
    let effects = send(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(15)));
    let (generation, _) = alarm_of(&effects).unwrap();
    let due = send(&mut app, AudioMessage::SleepDue(generation));
    assert_eq!(due.first(), Some(&Effect::AudioPause));
    assert!(
      !due
        .iter()
        .any(|effect| matches!(effect, Effect::AudioFadeOut(_)))
    );
  }

  #[test]
  fn the_status_shows_off_minutes_and_seconds_or_end_of_track() {
    let now = Instant::now();
    let same = |key: &str| key.to_string();
    assert_eq!(SleepTimer::Off.status(now, same), "Off");
    assert_eq!(SleepTimer::EndOfTrack.status(now, same), "End of track");
    let timer = SleepTimer::Minutes {
      minutes: 15,
      ends_at: now + Duration::from_millis(75_500),
    };
    assert_eq!(timer.status(now, same), "1:15");
    assert_eq!(timer.status(now + Duration::from_secs(80), same), "0:00");
    assert!(timer.selects(SleepChoice::Minutes(15)));
    assert!(!timer.selects(SleepChoice::Minutes(30)));
    assert!(SleepTimer::Off.selects(SleepChoice::Off));
  }

  #[test]
  fn only_a_running_countdown_on_the_open_sleep_tab_needs_a_tick() {
    let mut app = app();
    assert!(!app.sleep_countdown_visible(), "minuterie coupée");
    send(&mut app, AudioMessage::Open);
    send(&mut app, AudioMessage::Tab(AudioTab::Sleep));
    assert!(!app.sleep_countdown_visible(), "minuterie coupée");
    send(&mut app, AudioMessage::Sleep(SleepChoice::EndOfTrack));
    assert!(
      !app.sleep_countdown_visible(),
      "fin de titre : rien à compter"
    );
    send(&mut app, AudioMessage::Sleep(SleepChoice::Minutes(15)));
    assert!(app.sleep_countdown_visible());
    send(&mut app, AudioMessage::Tab(AudioTab::Output));
    assert!(!app.sleep_countdown_visible(), "autre onglet");
    send(&mut app, AudioMessage::Tab(AudioTab::Sleep));
    app.update(Message::Escape);
    assert!(!app.sleep_countdown_visible(), "fenêtre fermée");
    send(&mut app, AudioMessage::Sleep(SleepChoice::Off));
    assert!(!app.sleep_countdown_visible());
  }
}
