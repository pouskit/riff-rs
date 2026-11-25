//! Contrôles des paroles, visualiseurs et fonds. Le catalogue est dans `prefs.rs`, la vue dans `screens/settings.rs`.

use crate::app::{App, Effect};
use crate::images::{ImageOrigin, origin_of};
use crate::lyrics::update::LyricsMessage;
use crate::ui::Modal;
use crate::visuals::settings::{MeshConfig, TrackFx, WordStyle};
use crate::widgets::toast::ToastKind;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use url::Url;

/// Réglages à deux états.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualBool {
  BarWave,
  VizConnected,
  M3Cover,
  TvkVisualizerLyrics,
  Banners,
  TrackBanners,
  CoverBg,
  MeshGradient,
  MeshFollowIntensity,
  OnlineLyrics,
  KaraokeWords,
  LyricsAutoscroll,
  TrEnabled,
  TrAuto,
  MiniLyrics,
  MiniOnTop,
}

impl VisualBool {
  pub const ALL: [Self; 16] = [
    Self::BarWave,
    Self::VizConnected,
    Self::M3Cover,
    Self::TvkVisualizerLyrics,
    Self::Banners,
    Self::TrackBanners,
    Self::CoverBg,
    Self::MeshGradient,
    Self::MeshFollowIntensity,
    Self::OnlineLyrics,
    Self::KaraokeWords,
    Self::LyricsAutoscroll,
    Self::TrEnabled,
    Self::TrAuto,
    Self::MiniLyrics,
    Self::MiniOnTop,
  ];
}

/// Réglages numériques, avec les bornes et le pas du curseur de Riff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualInt {
  BarVizOpacity,
  VizSens,
  VizSmooth,
  TvkVisualizerOpacity,
  TvkBgBlur,
  MeshBrightness,
  MeshSpeed,
  MeshP1X,
  MeshP1Y,
  MeshP2X,
  MeshP2Y,
  BannerBlur,
  BannerOpacity,
  BannerFade,
  BgOpacity,
  BgBlur,
  CoverBgBrightness,
  LyricsSpeed,
  LyricsTightness,
}

impl VisualInt {
  pub const ALL: [Self; 19] = [
    Self::BarVizOpacity,
    Self::VizSens,
    Self::VizSmooth,
    Self::TvkVisualizerOpacity,
    Self::TvkBgBlur,
    Self::MeshBrightness,
    Self::MeshSpeed,
    Self::MeshP1X,
    Self::MeshP1Y,
    Self::MeshP2X,
    Self::MeshP2Y,
    Self::BannerBlur,
    Self::BannerOpacity,
    Self::BannerFade,
    Self::BgOpacity,
    Self::BgBlur,
    Self::CoverBgBrightness,
    Self::LyricsSpeed,
    Self::LyricsTightness,
  ];

  /// Bornes du curseur de `index.html`. `riff_viz_sens` et `riff_viz_smooth` ont deux curseurs dans Riff : la plage la plus large.
  #[must_use]
  pub const fn range(self) -> (i32, i32) {
    match self {
      Self::BarVizOpacity => (5, 85),
      Self::VizSens => (0, 300),
      Self::VizSmooth => (0, 95),
      Self::TvkVisualizerOpacity => (10, 100),
      Self::TvkBgBlur => (0, 60),
      Self::MeshBrightness
      | Self::MeshSpeed
      | Self::MeshP1X
      | Self::MeshP1Y
      | Self::MeshP2X
      | Self::MeshP2Y => (0, 100),
      Self::BannerBlur => (0, 12),
      Self::BannerOpacity | Self::BannerFade => (20, 100),
      Self::BgOpacity | Self::CoverBgBrightness => (5, 100),
      Self::BgBlur => (0, 40),
      Self::LyricsSpeed => (5, 25),
      Self::LyricsTightness => (2, 24),
    }
  }

  /// Pas du curseur de `index.html`.
  #[must_use]
  pub const fn step(self) -> i32 {
    match self {
      Self::VizSens
      | Self::TvkVisualizerOpacity
      | Self::MeshBrightness
      | Self::BannerOpacity
      | Self::BgOpacity
      | Self::CoverBgBrightness => 5,
      Self::TvkBgBlur | Self::BannerFade | Self::LyricsTightness => 2,
      _ => 1,
    }
  }

  /// Valeur affichée à côté du curseur.
  #[must_use]
  pub fn format(self, value: i32) -> String {
    match self {
      Self::LyricsSpeed => format!("{:.1}x", value as f32 / 10.0),
      Self::TvkBgBlur | Self::BannerBlur | Self::BgBlur => format!("{value}px"),
      Self::MeshSpeed => value.to_string(),
      _ => format!("{value}%"),
    }
  }
}

/// Réglages à choix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualChoice {
  LyricsSource,
  WordSource,
  LyricsEase,
  WordStyle,
  BarVizPos,
  BannerFit,
}

impl VisualChoice {
  pub const ALL: [Self; 6] = [
    Self::LyricsSource,
    Self::WordSource,
    Self::LyricsEase,
    Self::WordStyle,
    Self::BarVizPos,
    Self::BannerFit,
  ];

  /// Valeur enregistrée et libellé de Riff.
  #[must_use]
  pub const fn options(self) -> &'static [(&'static str, &'static str)] {
    match self {
      Self::LyricsSource => &[
        ("lrclib", "LRCLIB"),
        ("musixmatch", "Musixmatch"),
        ("kugou", "Kugou LRC"),
        ("netease", "NetEase"),
        ("genius", "Genius"),
      ],
      Self::WordSource => &[
        ("auto", "Auto"),
        ("netease", "NetEase"),
        ("estimate", "Estimate only (current)"),
      ],
      Self::LyricsEase => &[
        ("inout", "Ease in-out (Cubic)"),
        ("quad", "Smooth (Quad)"),
        ("sine", "Gentle (Sine)"),
        ("out", "Ease out (Quart)"),
        ("outsine", "Soft ease out"),
        ("expo", "Snappy (Expo)"),
        ("linear", "Linear"),
      ],
      Self::WordStyle => &[("gradient", "Gradient"), ("words", "Words")],
      Self::BarVizPos => &[("mini", "Compact"), ("behind", "Behind bar")],
      Self::BannerFit => &[("fit", "Fit"), ("zoom", "Zoom"), ("stretch", "Stretch")],
    }
  }
}

/// Curseurs des effets de bannière d'un titre (`index.html`, fenêtre `track-art-modal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtSlider {
  Blur,
  Opacity,
  Fade,
}

impl ArtSlider {
  pub const ALL: [Self; 3] = [Self::Blur, Self::Opacity, Self::Fade];

  #[must_use]
  pub const fn range(self) -> (i32, i32) {
    match self {
      Self::Blur => (0, 20),
      Self::Opacity | Self::Fade => (10, 100),
    }
  }

  #[must_use]
  pub const fn step(self) -> i32 {
    match self {
      Self::Blur => 1,
      Self::Opacity => 5,
      Self::Fade => 2,
    }
  }

  #[must_use]
  pub const fn label(self) -> &'static str {
    match self {
      Self::Blur => "Banner blur",
      Self::Opacity => "Banner opacity",
      Self::Fade => "Banner fade",
    }
  }

  #[must_use]
  pub fn format(self, value: i32) -> String {
    match self {
      Self::Blur => format!("{value}px"),
      Self::Opacity | Self::Fade => format!("{value}%"),
    }
  }

  #[must_use]
  pub const fn value(self, fx: TrackFx) -> i32 {
    (match self {
      Self::Blur => fx.blur,
      Self::Opacity => fx.opacity,
      Self::Fade => fx.fade,
    }) as i32
  }
}

/// Fichier choisi pour une bannière ou un fond, ou pour la pochette ou la bannière du titre de la fenêtre ouverte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaTarget {
  Banner,
  Background,
  TrackCover,
  TrackBanner,
}

/// Messages des contrôles visuels.
#[derive(Debug, Clone)]
pub enum VisualMessage {
  Bool(VisualBool, bool),
  /// Déplacement d'un curseur : appliqué sans écriture. L'écriture suit au relâchement (`SettingsMessage::CommitInt`).
  Int(VisualInt, i32),
  Choice(VisualChoice, String),
  PickMedia(MediaTarget),
  MediaPicked(MediaTarget, Option<PathBuf>),
  ClearMedia(MediaTarget),
  /// Interrupteur « Custom banner effects » du titre de la fenêtre ouverte : écrit aussitôt.
  TrackArtFxOn(bool),
  /// Curseur d'effet du titre, comme `Int` : l'écriture suit au relâchement.
  TrackArtFx(ArtSlider, i32),
  ResetMesh,
}

/// Fichiers que le décodeur d'images de l'application sait lire.
fn is_supported_image(path: &Path) -> bool {
  path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      ["png", "jpg", "jpeg"]
        .iter()
        .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

/// Adresse relue par `images::origin_of`.
fn file_url(path: &Path) -> Option<String> {
  is_supported_image(path)
    .then(|| Url::from_file_path(path).ok())
    .flatten()
    .map(String::from)
}

/// Nom affiché pour le fichier choisi : le nom du fichier local, sinon l'adresse telle quelle.
#[must_use]
pub fn media_label(url: &str) -> String {
  match origin_of(url) {
    Some(ImageOrigin::File(path)) if url.starts_with("file:") => path.file_name().map_or_else(
      || url.to_string(),
      |name| name.to_string_lossy().into_owned(),
    ),
    _ => url.to_string(),
  }
}

fn signed(value: u32) -> i32 {
  i32::try_from(value).unwrap_or(i32::MAX)
}

impl App {
  /// Valeur d'un interrupteur.
  #[must_use]
  pub fn visual_bool(&self, key: VisualBool) -> bool {
    let visual = &self.settings.visual;
    match key {
      VisualBool::BarWave => visual.bar_wave,
      VisualBool::VizConnected => visual.viz_connected,
      VisualBool::M3Cover => visual.m3_cover,
      VisualBool::TvkVisualizerLyrics => visual.tvk_visualizer_lyrics,
      VisualBool::Banners => visual.banners,
      VisualBool::TrackBanners => visual.track_banners,
      VisualBool::CoverBg => visual.cover_bg,
      VisualBool::MeshGradient => visual.mesh_gradient,
      VisualBool::MeshFollowIntensity => visual.mesh_follow_intensity,
      VisualBool::OnlineLyrics => visual.online_lyrics,
      VisualBool::KaraokeWords => visual.karaoke_words,
      VisualBool::LyricsAutoscroll => visual.lyrics_autoscroll,
      VisualBool::TrEnabled => visual.tr_enabled,
      VisualBool::TrAuto => visual.tr_auto,
      VisualBool::MiniLyrics => self.lyrics.mini_open,
      VisualBool::MiniOnTop => visual.mini_window.always_on_top,
    }
  }

  /// Valeur d'un curseur, ramenée dans ses bornes.
  #[must_use]
  pub fn visual_int(&self, key: VisualInt) -> i32 {
    let visual = &self.settings.visual;
    let value = match key {
      VisualInt::BarVizOpacity => signed(visual.bar_viz_opacity),
      VisualInt::VizSens => signed(visual.viz_sens),
      VisualInt::VizSmooth => signed(visual.viz_smooth),
      VisualInt::TvkVisualizerOpacity => signed(visual.tvk_visualizer_opacity),
      VisualInt::TvkBgBlur => signed(visual.tvk_bg_blur),
      VisualInt::MeshBrightness => signed(visual.mesh_brightness),
      VisualInt::MeshSpeed => visual.mesh.speed.round() as i32,
      VisualInt::MeshP1X => visual.mesh.p1.x.round() as i32,
      VisualInt::MeshP1Y => visual.mesh.p1.y.round() as i32,
      VisualInt::MeshP2X => visual.mesh.p2.x.round() as i32,
      VisualInt::MeshP2Y => visual.mesh.p2.y.round() as i32,
      VisualInt::BannerBlur => signed(visual.banner_blur),
      VisualInt::BannerOpacity => signed(visual.banner_opacity),
      VisualInt::BannerFade => signed(visual.banner_fade),
      VisualInt::BgOpacity => signed(visual.bg_opacity),
      VisualInt::BgBlur => signed(visual.bg_blur),
      VisualInt::CoverBgBrightness => signed(visual.cover_bg_brightness),
      VisualInt::LyricsSpeed => (visual.lyrics_speed * 10.0).round() as i32,
      VisualInt::LyricsTightness => signed(visual.lyrics_gradient_tightness),
    };
    let (low, high) = key.range();
    value.clamp(low, high)
  }

  /// Valeur enregistrée d'un choix.
  #[must_use]
  pub fn visual_choice(&self, key: VisualChoice) -> &str {
    let visual = &self.settings.visual;
    match key {
      VisualChoice::LyricsSource => &visual.lyrics_source,
      VisualChoice::WordSource => &visual.word_source,
      VisualChoice::LyricsEase => &visual.lyrics_ease,
      VisualChoice::WordStyle => match visual.lyrics_word_style {
        WordStyle::Gradient => "gradient",
        WordStyle::Words => "words",
      },
      VisualChoice::BarVizPos => {
        if visual.bar_viz_behind {
          "behind"
        } else {
          "mini"
        }
      }
      VisualChoice::BannerFit => &visual.banner_fit,
    }
  }

  fn set_visual_bool(&mut self, key: VisualBool, value: bool) -> Vec<Effect> {
    match key {
      VisualBool::MiniLyrics => {
        return if value == self.lyrics.mini_open {
          Vec::new()
        } else {
          self.update_lyrics(LyricsMessage::ToggleMini)
        };
      }
      VisualBool::MiniOnTop => {
        return if value == self.settings.visual.mini_window.always_on_top {
          Vec::new()
        } else {
          self.update_lyrics(LyricsMessage::MiniOnTop)
        };
      }
      // `app.js:18463` et `:18681` : couper masque les lignes traduites, allumer avec l'autre interrupteur lance la traduction.
      VisualBool::TrEnabled | VisualBool::TrAuto => {
        let visual = &mut self.settings.visual;
        if key == VisualBool::TrEnabled {
          visual.tr_enabled = value;
        } else {
          visual.tr_auto = value;
        }
        let mut effects = self.save_settings();
        if key == VisualBool::TrEnabled && !value {
          if self.lyrics.translate_on {
            self.stop_translation();
          }
        } else if value && self.settings.visual.tr_auto && !self.lyrics.translate_on {
          effects.extend(self.start_translation());
        }
        return effects;
      }
      _ => {}
    }
    let visual = &mut self.settings.visual;
    match key {
      VisualBool::BarWave => visual.bar_wave = value,
      VisualBool::VizConnected => visual.viz_connected = value,
      VisualBool::M3Cover => visual.m3_cover = value,
      VisualBool::TvkVisualizerLyrics => visual.tvk_visualizer_lyrics = value,
      VisualBool::Banners => visual.banners = value,
      VisualBool::TrackBanners => visual.track_banners = value,
      VisualBool::CoverBg => visual.cover_bg = value,
      VisualBool::MeshGradient => visual.mesh_gradient = value,
      VisualBool::MeshFollowIntensity => visual.mesh_follow_intensity = value,
      VisualBool::OnlineLyrics => visual.online_lyrics = value,
      VisualBool::KaraokeWords => {
        visual.karaoke_words = value;
        self.relayout();
      }
      VisualBool::LyricsAutoscroll => visual.lyrics_autoscroll = value,
      VisualBool::TrEnabled
      | VisualBool::TrAuto
      | VisualBool::MiniLyrics
      | VisualBool::MiniOnTop => {}
    }
    self.save_settings()
  }

  fn set_visual_int(&mut self, key: VisualInt, value: i32) {
    let (low, high) = key.range();
    let value = value.clamp(low, high);
    let (whole, real) = (value as u32, value as f32);
    let visual = &mut self.settings.visual;
    match key {
      VisualInt::BarVizOpacity => visual.bar_viz_opacity = whole,
      VisualInt::VizSens => visual.viz_sens = whole,
      VisualInt::VizSmooth => visual.viz_smooth = whole,
      VisualInt::TvkVisualizerOpacity => visual.tvk_visualizer_opacity = whole,
      VisualInt::TvkBgBlur => visual.tvk_bg_blur = whole,
      VisualInt::MeshBrightness => visual.mesh_brightness = whole,
      VisualInt::MeshSpeed => visual.mesh.speed = real,
      VisualInt::MeshP1X => visual.mesh.p1.x = real,
      VisualInt::MeshP1Y => visual.mesh.p1.y = real,
      VisualInt::MeshP2X => visual.mesh.p2.x = real,
      VisualInt::MeshP2Y => visual.mesh.p2.y = real,
      VisualInt::BannerBlur => visual.banner_blur = whole,
      VisualInt::BannerOpacity => visual.banner_opacity = whole,
      VisualInt::BannerFade => visual.banner_fade = whole,
      VisualInt::BgOpacity => visual.bg_opacity = whole,
      VisualInt::BgBlur => visual.bg_blur = whole,
      VisualInt::CoverBgBrightness => visual.cover_bg_brightness = whole,
      VisualInt::LyricsSpeed => visual.lyrics_speed = real / 10.0,
      VisualInt::LyricsTightness => visual.lyrics_gradient_tightness = whole,
    }
  }

  fn set_visual_choice(&mut self, key: VisualChoice, value: &str) -> Vec<Effect> {
    if !key.options().iter().any(|(listed, _)| *listed == value) {
      return Vec::new();
    }
    let visual = &mut self.settings.visual;
    match key {
      VisualChoice::LyricsSource => visual.lyrics_source = value.to_string(),
      VisualChoice::WordSource => visual.word_source = value.to_string(),
      VisualChoice::LyricsEase => visual.lyrics_ease = value.to_string(),
      VisualChoice::WordStyle => {
        visual.lyrics_word_style = if value == "words" {
          WordStyle::Words
        } else {
          WordStyle::Gradient
        };
        self.relayout();
      }
      VisualChoice::BarVizPos => visual.bar_viz_behind = value == "behind",
      VisualChoice::BannerFit => visual.banner_fit = value.to_string(),
    }
    self.save_settings()
  }

  /// Identifiant du titre de la fenêtre « Customize cover & banner », si elle est ouverte.
  fn art_track_id(&self) -> Option<String> {
    match &self.ui.modal {
      Some(Modal::TrackArt(track)) => Some(track.canonical_id(&self.data.platform)),
      _ => None,
    }
  }

  /// `setTrackArt` : fusionne `patch` dans l'entrée du titre. Une clé nulle ou vide est retirée, une entrée vide aussi.
  /// `false` si aucune fenêtre n'est ouverte.
  fn write_track_art(&mut self, patch: &[(&str, Value)]) -> bool {
    let Some(id) = self.art_track_id() else {
      return false;
    };
    let all = &mut self.settings.visual.track_art;
    if !all.is_object() {
      *all = json!({});
    }
    let Some(all) = all.as_object_mut() else {
      return false;
    };
    let mut entry = all
      .get(&id)
      .and_then(Value::as_object)
      .cloned()
      .unwrap_or_default();
    for (key, value) in patch {
      if value.is_null() || value.as_str() == Some("") {
        entry.remove(*key);
      } else {
        entry.insert((*key).to_string(), value.clone());
      }
    }
    if entry.is_empty() {
      all.remove(&id);
    } else {
      all.insert(id, Value::Object(entry));
    }
    true
  }

  /// Écrit les effets du titre avec toutes les clés de Riff, `bannerFx` compris.
  fn write_track_fx(&mut self, change: impl FnOnce(&mut TrackFx)) -> bool {
    let Some(id) = self.art_track_id() else {
      return false;
    };
    let mut fx = self.settings.visual.track_fx(&id);
    change(&mut fx);
    self.write_track_art(&[
      ("bannerCustomFx", json!(fx.enabled)),
      ("bannerBlur", json!(fx.blur)),
      ("bannerOpacity", json!(fx.opacity)),
      ("bannerFade", json!(fx.fade)),
      (
        "bannerFx",
        json!({ "enabled": fx.enabled, "blur": fx.blur, "opacity": fx.opacity, "fade": fx.fade }),
      ),
    ])
  }

  /// Applique un fichier choisi, ou le retrait de celui-ci quand `url` est vide.
  fn apply_media(&mut self, target: MediaTarget, url: String) -> Vec<Effect> {
    match target {
      MediaTarget::Banner => self.settings.visual.banner_media = url,
      MediaTarget::Background => self.settings.visual.bg_url = url,
      MediaTarget::TrackCover | MediaTarget::TrackBanner => {
        let key = if target == MediaTarget::TrackCover {
          "cover"
        } else {
          "banner"
        };
        return if self.write_track_art(&[(key, Value::String(url))]) {
          self.save_settings()
        } else {
          Vec::new()
        };
      }
    }
    self.save_settings()
  }

  /// Messages des contrôles visuels.
  pub fn update_visual(&mut self, message: VisualMessage) -> Vec<Effect> {
    match message {
      VisualMessage::Bool(key, value) => self.set_visual_bool(key, value),
      VisualMessage::Int(key, value) => {
        self.set_visual_int(key, value);
        Vec::new()
      }
      VisualMessage::Choice(key, value) => self.set_visual_choice(key, &value),
      VisualMessage::PickMedia(target) => vec![Effect::PickMedia(target)],
      VisualMessage::MediaPicked(_, None) => Vec::new(),
      VisualMessage::MediaPicked(target, Some(path)) => match file_url(&path) {
        Some(url) => self.apply_media(target, url),
        None => vec![Effect::Toast(
          "Unsupported file: choose a PNG or JPEG image.".to_string(),
          ToastKind::Error,
        )],
      },
      VisualMessage::ClearMedia(target) => self.apply_media(target, String::new()),
      VisualMessage::TrackArtFxOn(enabled) => {
        if self.write_track_fx(|fx| fx.enabled = enabled) {
          self.save_settings()
        } else {
          Vec::new()
        }
      }
      VisualMessage::TrackArtFx(slider, value) => {
        let (low, high) = slider.range();
        let value = value.clamp(low, high) as u32;
        self.write_track_fx(|fx| match slider {
          ArtSlider::Blur => fx.blur = value,
          ArtSlider::Opacity => fx.opacity = value,
          ArtSlider::Fade => fx.fade = value,
        });
        Vec::new()
      }
      VisualMessage::ResetMesh => {
        self.settings.visual.mesh = MeshConfig::default();
        self.save_settings()
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::Message;
  use crate::i18n::I18n;
  use crate::lyrics::model::Lyrics;
  use crate::prefs::SettingsMessage;
  use crate::settings::{LegacyStorage, NativeSettings};
  use crate::user_data::{Track, UserData};
  use crate::visuals::settings::WordStyle;
  use crate::visuals::viz::FREQ_BINS;
  use crate::window::PersistedWindowState;
  use iced::Size;
  use std::time::{Duration, Instant};

  fn track() -> Track {
    Track {
      id: Some("a".into()),
      title: Some("Titre".into()),
      artist: Some("Artiste".into()),
      duration: Some(200.0),
      url: Some("https://exemple.org/a".into()),
      thumbnail: Some("https://exemple.org/p.jpg".into()),
      platform: Some("ytmusic".into()),
      ..Track::default()
    }
  }

  fn app() -> App {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
    .with_data(UserData::default());
    app.window.size = Some(Size::new(1280.0, 800.0));
    app
  }

  fn playing() -> App {
    let mut app = app();
    app.player.current = Some(track());
    app.playback.is_playing = true;
    app.mark_resume_saved();
    app
  }

  fn with_lyrics() -> App {
    let mut app = playing();
    app.lyrics.lyrics = Some(Lyrics::parse(
      "[00:01.00]one\n[00:03.00]two\n[00:05.00]three\n[00:07.00]four",
      false,
    ));
    app.relayout();
    app
  }

  fn send(app: &mut App, message: VisualMessage) -> Vec<Effect> {
    app.update(Message::Settings(SettingsMessage::Visual(message)))
  }

  fn saved(effects: &[Effect]) -> bool {
    effects
      .iter()
      .any(|effect| matches!(effect, Effect::SaveSettings(_)))
  }

  fn scratch_file(name: &str) -> (crate::test_temp::TempDir, PathBuf) {
    let dir = crate::test_temp::TempDir::new(&format!("app-prefs-visual-{name}"));
    let path = dir.join("native-settings.json");
    (dir, path)
  }

  const BOOL_DEFAULTS: [(VisualBool, bool); 16] = [
    (VisualBool::BarWave, false),
    (VisualBool::VizConnected, false),
    (VisualBool::M3Cover, true),
    (VisualBool::TvkVisualizerLyrics, true),
    (VisualBool::Banners, true),
    (VisualBool::TrackBanners, false),
    (VisualBool::CoverBg, false),
    (VisualBool::MeshGradient, false),
    (VisualBool::MeshFollowIntensity, true),
    (VisualBool::OnlineLyrics, true),
    (VisualBool::KaraokeWords, true),
    (VisualBool::LyricsAutoscroll, true),
    (VisualBool::TrEnabled, false),
    (VisualBool::TrAuto, false),
    (VisualBool::MiniLyrics, false),
    (VisualBool::MiniOnTop, false),
  ];

  /// Clé, bornes, pas et défaut de `index.html` et `app.js` (`getItem(...) || 'x'`).
  const INT_SPECS: [(VisualInt, i32, i32, i32, i32); 19] = [
    (VisualInt::BarVizOpacity, 5, 85, 1, 22),
    (VisualInt::VizSens, 0, 300, 5, 100),
    (VisualInt::VizSmooth, 0, 95, 1, 55),
    (VisualInt::TvkVisualizerOpacity, 10, 100, 5, 85),
    (VisualInt::TvkBgBlur, 0, 60, 2, 22),
    (VisualInt::MeshBrightness, 0, 100, 5, 100),
    (VisualInt::MeshSpeed, 0, 100, 1, 0),
    (VisualInt::MeshP1X, 0, 100, 1, 20),
    (VisualInt::MeshP1Y, 0, 100, 1, 0),
    (VisualInt::MeshP2X, 0, 100, 1, 85),
    (VisualInt::MeshP2Y, 0, 100, 1, 5),
    (VisualInt::BannerBlur, 0, 12, 1, 3),
    (VisualInt::BannerOpacity, 20, 100, 5, 100),
    (VisualInt::BannerFade, 20, 100, 2, 100),
    (VisualInt::BgOpacity, 5, 100, 5, 100),
    (VisualInt::BgBlur, 0, 40, 1, 0),
    (VisualInt::CoverBgBrightness, 5, 100, 5, 45),
    (VisualInt::LyricsSpeed, 5, 25, 1, 10),
    (VisualInt::LyricsTightness, 2, 24, 2, 2),
  ];

  const CHOICE_DEFAULTS: [(VisualChoice, &str); 6] = [
    (VisualChoice::LyricsSource, "lrclib"),
    (VisualChoice::WordSource, "auto"),
    (VisualChoice::LyricsEase, "inout"),
    (VisualChoice::WordStyle, "gradient"),
    (VisualChoice::BarVizPos, "behind"),
    (VisualChoice::BannerFit, "zoom"),
  ];

  #[test]
  fn every_control_is_listed_once() {
    assert_eq!(VisualBool::ALL.len(), BOOL_DEFAULTS.len());
    assert!(
      VisualBool::ALL
        .iter()
        .all(|key| BOOL_DEFAULTS.iter().any(|(listed, _)| listed == key))
    );
    assert!(
      VisualInt::ALL
        .iter()
        .all(|key| INT_SPECS.iter().any(|(listed, ..)| listed == key))
    );
    assert!(
      VisualChoice::ALL
        .iter()
        .all(|key| CHOICE_DEFAULTS.iter().any(|(listed, _)| listed == key))
    );
  }

  #[test]
  fn the_defaults_are_the_defaults_of_riff() {
    let app = app();
    for (key, expected) in BOOL_DEFAULTS {
      assert_eq!(app.visual_bool(key), expected, "{key:?}");
    }
    for (key, _, _, _, expected) in INT_SPECS {
      assert_eq!(app.visual_int(key), expected, "{key:?}");
    }
    for (key, expected) in CHOICE_DEFAULTS {
      assert_eq!(app.visual_choice(key), expected, "{key:?}");
    }
  }

  #[test]
  fn bounds_and_steps_are_the_ones_of_the_page_of_riff() {
    for (key, low, high, step, default) in INT_SPECS {
      assert_eq!(key.range(), (low, high), "{key:?}");
      assert_eq!(key.step(), step, "{key:?}");
      assert!(
        (default - low) % step == 0,
        "{key:?} : le défaut est hors de la grille du curseur"
      );
    }
  }

  #[test]
  fn sliders_stay_inside_their_bounds() {
    let mut app = app();
    for (key, low, high, ..) in INT_SPECS {
      send(&mut app, VisualMessage::Int(key, high + 500));
      assert_eq!(app.visual_int(key), high, "{key:?}");
      send(&mut app, VisualMessage::Int(key, low - 500));
      assert_eq!(app.visual_int(key), low, "{key:?}");
    }
  }

  #[test]
  fn a_slider_applies_at_once_and_writes_on_release() {
    let mut app = app();
    assert!(send(&mut app, VisualMessage::Int(VisualInt::BgBlur, 12)).is_empty());
    assert_eq!(app.settings.visual.bg_blur, 12);
    assert!(saved(
      &app.update(Message::Settings(SettingsMessage::CommitInt))
    ));
  }

  #[test]
  fn the_values_are_shown_with_their_unit() {
    assert_eq!(VisualInt::LyricsSpeed.format(10), "1.0x");
    assert_eq!(VisualInt::LyricsSpeed.format(25), "2.5x");
    assert_eq!(VisualInt::BannerBlur.format(3), "3px");
    assert_eq!(VisualInt::BgOpacity.format(60), "60%");
    assert_eq!(VisualInt::MeshSpeed.format(0), "0");
  }

  #[test]
  fn every_switch_reads_back_and_persists() {
    for key in VisualBool::ALL {
      let mut app = app();
      let next = !app.visual_bool(key);
      let effects = send(&mut app, VisualMessage::Bool(key, next));
      assert_eq!(app.visual_bool(key), next, "{key:?}");
      assert!(saved(&effects), "{key:?} : {effects:?}");
      send(&mut app, VisualMessage::Bool(key, !next));
      assert_eq!(app.visual_bool(key), !next, "{key:?}");
    }
  }

  #[test]
  fn every_choice_offers_its_default_and_refuses_other_values() {
    for (key, default) in CHOICE_DEFAULTS {
      assert!(
        key.options().iter().any(|(value, _)| *value == default),
        "{key:?}"
      );
      let mut app = app();
      let effects = send(&mut app, VisualMessage::Choice(key, "inconnu".into()));
      assert!(effects.is_empty(), "{key:?}");
      assert_eq!(app.visual_choice(key), default, "{key:?}");
      for (value, _) in key.options() {
        assert!(
          saved(&send(&mut app, VisualMessage::Choice(key, (*value).into()))),
          "{key:?} {value}"
        );
        assert_eq!(app.visual_choice(key), *value);
      }
    }
  }

  #[test]
  fn the_choices_of_riff_are_all_there() {
    let values = |key: VisualChoice| {
      key
        .options()
        .iter()
        .map(|(value, _)| *value)
        .collect::<Vec<_>>()
    };
    assert_eq!(
      values(VisualChoice::LyricsSource),
      ["lrclib", "musixmatch", "kugou", "netease", "genius"]
    );
    assert_eq!(
      values(VisualChoice::WordSource),
      ["auto", "netease", "estimate"]
    );
    assert_eq!(
      values(VisualChoice::LyricsEase),
      ["inout", "quad", "sine", "out", "outsine", "expo", "linear"]
    );
    assert_eq!(values(VisualChoice::WordStyle), ["gradient", "words"]);
    assert_eq!(values(VisualChoice::BarVizPos), ["mini", "behind"]);
    assert_eq!(values(VisualChoice::BannerFit), ["fit", "zoom", "stretch"]);
  }

  fn change_everything(app: &mut App) {
    for key in VisualBool::ALL {
      let next = !app.visual_bool(key);
      send(app, VisualMessage::Bool(key, next));
    }
    for (key, _, high, ..) in INT_SPECS {
      send(app, VisualMessage::Int(key, high));
    }
    for (key, default) in CHOICE_DEFAULTS {
      let other = key
        .options()
        .iter()
        .map(|(value, _)| *value)
        .find(|value| *value != default)
        .unwrap();
      send(app, VisualMessage::Choice(key, other.into()));
    }
  }

  #[test]
  fn every_control_survives_a_save_and_a_reload() {
    let mut app = app();
    let before = app.settings.visual.clone();
    change_everything(&mut app);
    assert_ne!(*app.settings.visual, *before);
    for key in VisualBool::ALL {
      assert_ne!(
        app.visual_bool(key),
        BOOL_DEFAULTS
          .iter()
          .find(|(listed, _)| *listed == key)
          .unwrap()
          .1,
        "{key:?}"
      );
    }
    let (_dir, path) = scratch_file("reload");
    app.settings.save(&path).unwrap();
    let loaded = NativeSettings::load_or_import(
      &path,
      &LegacyStorage::open(&path.with_file_name("absent.json")),
    );
    assert_eq!(*loaded.visual, *app.settings.visual);
  }

  #[test]
  fn an_unknown_key_of_the_visual_section_is_kept_on_save() {
    let (_dir, path) = scratch_file("unknown");
    std::fs::write(
      &path,
      r#"{ "visual": { "barWave": true, "futureKey": [1, 2] }, "otherFutureKey": 7 }"#,
    )
    .unwrap();
    let mut settings = NativeSettings::load_or_import(
      &path,
      &LegacyStorage::open(&path.with_file_name("absent.json")),
    );
    assert!(settings.visual.bar_wave);
    settings.visual.bg_blur = 9;
    settings.save(&path).unwrap();
    let written: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["visual"]["futureKey"], serde_json::json!([1, 2]));
    assert_eq!(written["visual"]["bgBlur"], 9);
    assert_eq!(written["otherFutureKey"], 7);
  }

  #[test]
  fn the_word_style_and_the_karaoke_switch_lay_the_lyrics_out_again() {
    let mut app = with_lyrics();
    app.lyrics.active = Some(1);
    send(
      &mut app,
      VisualMessage::Choice(VisualChoice::WordStyle, "words".into()),
    );
    assert_eq!(app.settings.visual.lyrics_word_style, WordStyle::Words);
    assert_eq!(
      app.lyrics.active, None,
      "la mise en page n'a pas été refaite"
    );
    app.lyrics.active = Some(1);
    send(
      &mut app,
      VisualMessage::Bool(VisualBool::KaraokeWords, false),
    );
    assert_eq!(app.lyrics.active, None);
  }

  #[test]
  fn the_next_lyrics_request_follows_the_source_and_the_online_switch() {
    let mut app = playing();
    send(
      &mut app,
      VisualMessage::Choice(VisualChoice::LyricsSource, "kugou".into()),
    );
    send(
      &mut app,
      VisualMessage::Bool(VisualBool::OnlineLyrics, false),
    );
    let effects = app.load_lyrics();
    let Some(Effect::CoreJson { query, .. }) = effects.first() else {
      panic!("attendu : demande de paroles");
    };
    let value = |name: &str| {
      query
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
    };
    assert_eq!(
      (value("source"), value("online")),
      (Some("kugou"), Some("0"))
    );
  }

  fn first_scroll(app: &mut App) -> Option<crate::lyrics::scroll::ScrollAnimation> {
    app.playback.position = Duration::from_secs(5);
    app.animation_tick(Instant::now(), &[0; FREQ_BINS]);
    app.lyrics.animation
  }

  #[test]
  fn auto_scroll_off_stops_following_the_active_line() {
    let mut on = with_lyrics();
    assert!(first_scroll(&mut on).is_some());
    let mut off = with_lyrics();
    send(
      &mut off,
      VisualMessage::Bool(VisualBool::LyricsAutoscroll, false),
    );
    assert!(first_scroll(&mut off).is_none());
  }

  #[test]
  fn the_animation_speed_and_curve_shape_the_scroll() {
    let mut slow = with_lyrics();
    send(&mut slow, VisualMessage::Int(VisualInt::LyricsSpeed, 5));
    let mut fast = with_lyrics();
    send(&mut fast, VisualMessage::Int(VisualInt::LyricsSpeed, 25));
    send(
      &mut fast,
      VisualMessage::Choice(VisualChoice::LyricsEase, "linear".into()),
    );
    let (slow, fast) = (
      first_scroll(&mut slow).unwrap(),
      first_scroll(&mut fast).unwrap(),
    );
    assert!(fast.duration < slow.duration);
    assert_eq!(fast.easing, crate::lyrics::scroll::Easing::Linear);
    assert_eq!(slow.easing, crate::lyrics::scroll::Easing::InOut);
  }

  #[test]
  fn the_gradient_tightness_is_the_one_drawn() {
    let mut app = app();
    send(&mut app, VisualMessage::Int(VisualInt::LyricsTightness, 12));
    assert_eq!(app.settings.visual.lyrics_gradient_tightness, 12);
  }

  #[test]
  fn the_bar_visualizer_asks_for_the_analyser_and_the_frames() {
    let mut app = playing();
    assert!(!app.visualizer_visible());
    send(&mut app, VisualMessage::Bool(VisualBool::BarWave, true));
    assert!(app.visualizer_visible());
    app.settings.visual.m3_cover = false;
    assert!(app.animation_needed());
    send(&mut app, VisualMessage::Bool(VisualBool::BarWave, false));
    assert!(!app.layers_need_frames());
  }

  #[test]
  fn the_bar_position_and_the_tone_follow_their_controls() {
    let mut app = app();
    send(
      &mut app,
      VisualMessage::Choice(VisualChoice::BarVizPos, "mini".into()),
    );
    assert!(!app.settings.visual.bar_viz_behind);
    send(
      &mut app,
      VisualMessage::Choice(VisualChoice::BarVizPos, "behind".into()),
    );
    assert!(app.settings.visual.bar_viz_behind);
    send(&mut app, VisualMessage::Int(VisualInt::BarVizOpacity, 40));
    send(&mut app, VisualMessage::Int(VisualInt::VizSens, 250));
    send(&mut app, VisualMessage::Int(VisualInt::VizSmooth, 80));
    let visual = &app.settings.visual;
    assert_eq!(
      (visual.bar_viz_opacity, visual.viz_sens, visual.viz_smooth),
      (40, 250, 80)
    );
  }

  #[test]
  fn the_blur_sliders_change_the_blurs_that_are_asked_for() {
    let mut app = playing();
    app.settings.visual.bg_url = "file:///D:/fond.png".into();
    send(&mut app, VisualMessage::Int(VisualInt::BgBlur, 0));
    assert!(app.custom_bg_key().is_none());
    send(&mut app, VisualMessage::Int(VisualInt::BgBlur, 10));
    let ten = app.custom_bg_key().expect("fond flouté demandé");
    send(&mut app, VisualMessage::Int(VisualInt::BgBlur, 20));
    assert_ne!(app.custom_bg_key().unwrap(), ten);

    app.lyrics.tvk_open = true;
    let before = app.tvk_bg_key(0).unwrap();
    send(&mut app, VisualMessage::Int(VisualInt::TvkBgBlur, 40));
    assert_ne!(app.tvk_bg_key(0).unwrap(), before);

    app.lyrics.tvk_open = false;
    app.lyrics.panel_open = true;
    send(
      &mut app,
      VisualMessage::Bool(VisualBool::TrackBanners, true),
    );
    let banner = app.banner_key().expect("bannière demandée");
    send(&mut app, VisualMessage::Int(VisualInt::BannerBlur, 9));
    assert_ne!(app.banner_key().unwrap(), banner);
    send(&mut app, VisualMessage::Bool(VisualBool::Banners, false));
    assert!(app.banner_key().is_none());
  }

  #[test]
  fn the_cover_background_asks_for_a_blur_and_replaces_the_other_backgrounds() {
    let mut app = playing();
    assert!(app.cover_bg_key().is_none());
    send(&mut app, VisualMessage::Bool(VisualBool::CoverBg, true));
    assert!(app.cover_bg_key().is_some());
    assert!(app.settings.visual.has_background());
    send(
      &mut app,
      VisualMessage::Int(VisualInt::CoverBgBrightness, 70),
    );
    assert_eq!(app.settings.visual.cover_bg_brightness, 70);
  }

  #[test]
  fn the_mesh_controls_drive_the_animation_and_the_background() {
    let mut app = playing();
    app.settings.visual.m3_cover = false;
    assert!(!app.settings.visual.has_background());
    send(
      &mut app,
      VisualMessage::Bool(VisualBool::MeshGradient, true),
    );
    assert!(app.settings.visual.has_background());
    assert!(!app.layers_need_frames(), "vitesse 0 : image fixe");
    send(&mut app, VisualMessage::Int(VisualInt::MeshSpeed, 20));
    assert!(app.animation_needed());
    assert_eq!(app.settings.visual.mesh.period_secs(), Some(24.0));
    send(&mut app, VisualMessage::Int(VisualInt::MeshBrightness, 0));
    assert!(!app.layers_need_frames());
    assert!(!app.settings.visual.has_background());
  }

  #[test]
  fn the_mesh_points_are_written_and_can_be_reset() {
    let mut app = app();
    send(&mut app, VisualMessage::Int(VisualInt::MeshP1X, 55));
    send(&mut app, VisualMessage::Int(VisualInt::MeshP1Y, 44));
    send(&mut app, VisualMessage::Int(VisualInt::MeshP2X, 33));
    send(&mut app, VisualMessage::Int(VisualInt::MeshP2Y, 22));
    send(&mut app, VisualMessage::Int(VisualInt::MeshSpeed, 60));
    let mesh = app.settings.visual.mesh;
    assert_eq!(
      ((mesh.p1.x, mesh.p1.y), (mesh.p2.x, mesh.p2.y), mesh.speed),
      ((55.0, 44.0), (33.0, 22.0), 60.0)
    );
    let effects = send(&mut app, VisualMessage::ResetMesh);
    assert!(saved(&effects));
    assert_eq!(
      app.settings.visual.mesh,
      crate::visuals::settings::MeshConfig::default()
    );
  }

  #[test]
  fn a_picked_image_becomes_a_file_url_that_the_loader_reads_back() {
    let mut app = app();
    let path = std::env::temp_dir().join("fond de test.PNG");
    let effects = send(
      &mut app,
      VisualMessage::MediaPicked(MediaTarget::Background, Some(path.clone())),
    );
    assert!(saved(&effects));
    let url = app.settings.visual.bg_url.clone();
    assert!(url.starts_with("file:///"), "{url}");
    assert_eq!(origin_of(&url), Some(ImageOrigin::File(path)));
    assert_eq!(app.background_url(), Some(url.as_str()));
    assert!(saved(&send(
      &mut app,
      VisualMessage::ClearMedia(MediaTarget::Background)
    )));
    assert!(app.settings.visual.bg_url.is_empty());
  }

  #[test]
  fn a_banner_image_replaces_the_track_art_until_it_is_reset() {
    let mut app = playing();
    send(
      &mut app,
      VisualMessage::Bool(VisualBool::TrackBanners, true),
    );
    assert_eq!(
      app.banner_url().as_deref(),
      Some("https://exemple.org/p.jpg")
    );
    let path = std::env::temp_dir().join("bannière.jpg");
    send(
      &mut app,
      VisualMessage::MediaPicked(MediaTarget::Banner, Some(path)),
    );
    let url = app.settings.visual.banner_media.clone();
    assert!(url.starts_with("file:///"));
    assert_eq!(app.banner_url().as_deref(), Some(url.as_str()));
    send(&mut app, VisualMessage::ClearMedia(MediaTarget::Banner));
    assert_eq!(
      app.banner_url().as_deref(),
      Some("https://exemple.org/p.jpg")
    );
  }

  #[test]
  fn a_video_or_a_non_image_is_refused_with_a_message() {
    for name in [
      "clip.mp4",
      "clip.webm",
      "notes.txt",
      "sans-extension",
      "animation.gif",
    ] {
      let mut app = app();
      let effects = send(
        &mut app,
        VisualMessage::MediaPicked(MediaTarget::Background, Some(PathBuf::from(name))),
      );
      assert!(app.settings.visual.bg_url.is_empty(), "{name}");
      assert!(
        effects
          .iter()
          .any(|effect| matches!(effect, Effect::Toast(..))),
        "{name} : {effects:?}"
      );
      assert!(!saved(&effects), "{name}");
    }
  }

  #[test]
  fn cancelling_the_file_dialog_changes_nothing() {
    let mut app = app();
    assert!(
      send(
        &mut app,
        VisualMessage::MediaPicked(MediaTarget::Banner, None)
      )
      .is_empty()
    );
    assert_eq!(
      *app.settings.visual,
      NativeSettings::default().visual.as_ref().clone()
    );
  }

  #[test]
  fn the_chosen_file_is_shown_by_its_name() {
    assert_eq!(media_label(""), "");
    let path = std::env::temp_dir().join("mon fond.jpg");
    let url = file_url(&path).unwrap();
    assert_eq!(media_label(&url), "mon fond.jpg");
    assert_eq!(
      media_label("https://exemple.org/a/b.png"),
      "https://exemple.org/a/b.png"
    );
  }

  #[test]
  fn choosing_a_file_opens_the_dialog() {
    let mut app = app();
    assert_eq!(
      send(&mut app, VisualMessage::PickMedia(MediaTarget::Banner)),
      [Effect::PickMedia(MediaTarget::Banner)]
    );
  }

  #[test]
  fn the_mini_lyrics_switch_opens_and_closes_the_window_and_remembers_it() {
    let mut app = app();
    let effects = send(&mut app, VisualMessage::Bool(VisualBool::MiniLyrics, true));
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::OpenMini(_)))
    );
    assert!(app.lyrics.mini_open && app.settings.visual.mini_lyrics);
    assert!(
      send(&mut app, VisualMessage::Bool(VisualBool::MiniLyrics, true)).is_empty(),
      "déjà ouverte"
    );
    let effects = send(&mut app, VisualMessage::Bool(VisualBool::MiniLyrics, false));
    assert!(effects.contains(&Effect::CloseMini));
    assert!(!app.lyrics.mini_open && !app.settings.visual.mini_lyrics);
  }

  fn translation_requests(effects: &[Effect]) -> Vec<serde_json::Value> {
    effects
      .iter()
      .filter_map(|effect| match effect {
        Effect::CoreJson {
          path: "/api/translate",
          body: Some(body),
          ..
        } => serde_json::from_str(body).ok(),
        _ => None,
      })
      .collect()
  }

  fn lyrics_arrive(app: &mut App) -> Vec<Effect> {
    app.lyrics.token = app.player.token;
    let tag = crate::ui::ReplyTag::Lyrics {
      token: app.lyrics.token,
    };
    app.lyrics_reply(
      &tag,
      Ok(serde_json::json!({ "success": true, "lyrics": "[00:01.00]one\n[00:03.00]two" })),
    )
  }

  fn consenting(mut app: App) -> App {
    app.lyrics.translation_enabled = true;
    app
  }

  #[test]
  fn the_translation_button_needs_the_consent_of_the_core_and_the_switch() {
    let mut app = app();
    assert!(!app.translation_available());
    app.settings.visual.tr_enabled = true;
    assert!(!app.translation_available(), "le consentement manque");
    app.lyrics.translation_enabled = true;
    assert!(app.translation_available());
    app.settings.visual.tr_enabled = false;
    assert!(
      !app.translation_available(),
      "l'interrupteur de Riff manque"
    );
  }

  #[test]
  fn the_translation_switches_never_grant_the_consent() {
    let mut app = app();
    send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    send(&mut app, VisualMessage::Bool(VisualBool::TrAuto, true));
    assert!(!app.lyrics.translation_enabled);
    assert!(!app.bool_pref(crate::prefs::BoolKey::Translation));
  }

  #[test]
  fn without_the_consent_nothing_goes_to_the_translator() {
    let mut app = playing();
    let mut all = Vec::new();
    all.extend(send(
      &mut app,
      VisualMessage::Bool(VisualBool::TrEnabled, true),
    ));
    all.extend(send(
      &mut app,
      VisualMessage::Bool(VisualBool::TrAuto, true),
    ));
    all.extend(lyrics_arrive(&mut app));
    all.extend(app.update(Message::Settings(SettingsMessage::SetText(
      crate::prefs::TextKey::TrLang,
      "de".into(),
    ))));
    all.extend(app.update_lyrics(LyricsMessage::ToggleTranslate));
    assert!(translation_requests(&all).is_empty(), "{all:?}");
    assert!(!app.lyrics.translate_on);
  }

  #[test]
  fn auto_translate_starts_when_the_lyrics_arrive() {
    let mut app = consenting(playing());
    app.settings.tr_lang = "fr".into();
    send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    let effects = lyrics_arrive(&mut app);
    assert!(
      translation_requests(&effects).is_empty(),
      "Auto-translate est coupé"
    );
    send(&mut app, VisualMessage::Bool(VisualBool::TrAuto, true));
    app.lyrics.translate_on = false;
    let effects = lyrics_arrive(&mut app);
    assert_eq!(
      translation_requests(&effects),
      [serde_json::json!({ "texts": ["one", "two"], "targetLang": "fr" })]
    );
    assert!(app.lyrics.translate_on);
  }

  #[test]
  fn switching_auto_translate_on_translates_the_current_lyrics_at_once() {
    let mut app = consenting(playing());
    send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    lyrics_arrive(&mut app);
    assert!(!app.lyrics.translate_on);
    let effects = send(&mut app, VisualMessage::Bool(VisualBool::TrAuto, true));
    assert_eq!(translation_requests(&effects).len(), 1);
    assert!(saved(&effects));
  }

  #[test]
  fn switching_the_translation_off_hides_the_translated_lines() {
    let mut app = consenting(playing());
    send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    lyrics_arrive(&mut app);
    app.update_lyrics(LyricsMessage::ToggleTranslate);
    app.lyrics.translations = Some(vec!["un".into(), "deux".into()]);
    let effects = send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, false));
    assert!(translation_requests(&effects).is_empty());
    assert!(!app.lyrics.translate_on && app.lyrics.translations.is_none());
    assert!(!app.translation_available());
  }

  #[test]
  fn switching_the_translation_on_with_auto_starts_it() {
    let mut app = consenting(playing());
    send(&mut app, VisualMessage::Bool(VisualBool::TrAuto, true));
    lyrics_arrive(&mut app);
    let effects = send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    assert_eq!(translation_requests(&effects).len(), 1);
  }

  fn set_consent(app: &mut App, value: bool) -> Vec<Effect> {
    app.update(Message::Settings(SettingsMessage::SetBool(
      crate::prefs::BoolKey::Translation,
      value,
    )))
  }

  #[test]
  fn the_consent_given_in_the_session_makes_the_translation_available() {
    let mut app = playing();
    send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    assert!(!app.translation_available());
    let effects = set_consent(&mut app, true);
    assert!(app.lyrics.translation_enabled && app.translation_available());
    assert!(effects.contains(&Effect::SetTranslation(true)));
    assert!(translation_requests(&effects).is_empty(), "pas de paroles");
  }

  #[test]
  fn giving_the_consent_with_auto_and_lyrics_requests_the_translation() {
    let mut app = playing();
    app.settings.tr_lang = "fr".into();
    send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    send(&mut app, VisualMessage::Bool(VisualBool::TrAuto, true));
    lyrics_arrive(&mut app);
    let effects = set_consent(&mut app, true);
    assert_eq!(
      translation_requests(&effects),
      [serde_json::json!({ "texts": ["one", "two"], "targetLang": "fr" })]
    );
    assert!(app.lyrics.translate_on);
    assert_eq!(effects.first(), Some(&Effect::SetTranslation(true)));
  }

  #[test]
  fn withdrawing_the_consent_stops_the_translation_and_the_requests() {
    let mut app = consenting(playing());
    send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    send(&mut app, VisualMessage::Bool(VisualBool::TrAuto, true));
    lyrics_arrive(&mut app);
    app.lyrics.translations = Some(vec!["un".into(), "deux".into()]);
    let mut effects = set_consent(&mut app, false);
    assert!(effects.contains(&Effect::SetTranslation(false)));
    assert!(!app.lyrics.translate_on && app.lyrics.translations.is_none());
    assert!(!app.translation_available());
    effects.extend(lyrics_arrive(&mut app));
    effects.extend(app.update_lyrics(LyricsMessage::ToggleTranslate));
    assert!(translation_requests(&effects).is_empty(), "{effects:?}");
  }

  #[test]
  fn changing_the_language_while_translating_asks_again_in_the_new_language() {
    let mut app = consenting(playing());
    send(&mut app, VisualMessage::Bool(VisualBool::TrEnabled, true));
    lyrics_arrive(&mut app);
    app.update_lyrics(LyricsMessage::ToggleTranslate);
    let effects = app.update(Message::Settings(SettingsMessage::SetText(
      crate::prefs::TextKey::TrLang,
      "uk".into(),
    )));
    assert_eq!(
      translation_requests(&effects),
      [serde_json::json!({ "texts": ["one", "two"], "targetLang": "uk" })]
    );
    assert!(saved(&effects));
    app.lyrics.translate_on = false;
    let effects = app.update(Message::Settings(SettingsMessage::SetText(
      crate::prefs::TextKey::TrLang,
      "de".into(),
    )));
    assert!(
      translation_requests(&effects).is_empty(),
      "rien à relancer quand la traduction est coupée"
    );
  }

  #[test]
  fn the_translation_entries_say_what_is_sent_and_where() {
    let entry = |label: &str| {
      crate::prefs::ENTRIES
        .iter()
        .find(|entry| entry.label == label)
        .unwrap()
    };
    for label in ["Lyrics translation", "Auto-translate"] {
      let entry = entry(label);
      assert_eq!(entry.category, crate::prefs::Category::Tracks);
      assert!(
        entry.hint.contains("Google")
          && entry.hint.contains("DeepL")
          && entry.hint.contains("Translation"),
        "{label} : {}",
        entry.hint
      );
    }
  }

  #[test]
  fn the_mini_window_can_stay_on_top() {
    let mut app = app();
    let effects = send(&mut app, VisualMessage::Bool(VisualBool::MiniOnTop, true));
    assert!(effects.contains(&Effect::MiniLevel(true)));
    assert!(app.settings.visual.mini_window.always_on_top);
    assert!(send(&mut app, VisualMessage::Bool(VisualBool::MiniOnTop, true)).is_empty());
    assert!(
      send(&mut app, VisualMessage::Bool(VisualBool::MiniOnTop, false))
        .contains(&Effect::MiniLevel(false))
    );
  }

  fn open_art(app: &mut App) {
    app.update(Message::Ui(crate::ui::UiMessage::Track(
      Box::new(track()),
      crate::ui::TrackCommand::Art,
    )));
    assert!(matches!(app.ui.modal, Some(crate::ui::Modal::TrackArt(_))));
  }

  fn entry(app: &App) -> serde_json::Value {
    app.settings.visual.track_art["ytmusic:a"].clone()
  }

  fn picked(app: &mut App, target: MediaTarget, name: &str) -> Vec<Effect> {
    let path = std::env::temp_dir().join(name);
    send(app, VisualMessage::MediaPicked(target, Some(path)))
  }

  #[test]
  fn a_track_cover_is_chosen_read_back_and_reset() {
    let mut app = playing();
    open_art(&mut app);
    assert!(saved(&picked(
      &mut app,
      MediaTarget::TrackCover,
      "pochette.PNG"
    )));
    let url = entry(&app)["cover"].as_str().unwrap().to_string();
    assert!(url.starts_with("file:///"), "{url}");
    assert_eq!(app.cover_of(&track()).as_deref(), Some(url.as_str()));
    assert!(saved(&send(
      &mut app,
      VisualMessage::ClearMedia(MediaTarget::TrackCover)
    )));
    assert_eq!(app.settings.visual.track_art, serde_json::json!({}));
    assert_eq!(
      app.cover_of(&track()).as_deref(),
      Some("https://exemple.org/p.jpg")
    );
  }

  #[test]
  fn a_track_banner_is_chosen_read_back_and_reset() {
    let mut app = playing();
    app.settings.visual.track_banners = true;
    open_art(&mut app);
    picked(&mut app, MediaTarget::TrackBanner, "bannière.jpeg");
    let url = entry(&app)["banner"].as_str().unwrap().to_string();
    assert_eq!(app.banner_url().as_deref(), Some(url.as_str()));
    send(
      &mut app,
      VisualMessage::ClearMedia(MediaTarget::TrackBanner),
    );
    assert_eq!(app.settings.visual.track_art, serde_json::json!({}));
    assert_eq!(
      app.banner_url().as_deref(),
      Some("https://exemple.org/p.jpg")
    );
  }

  #[test]
  fn resetting_one_image_keeps_the_other_and_an_empty_entry_is_removed() {
    let mut app = playing();
    open_art(&mut app);
    picked(&mut app, MediaTarget::TrackCover, "a.png");
    picked(&mut app, MediaTarget::TrackBanner, "b.png");
    send(&mut app, VisualMessage::ClearMedia(MediaTarget::TrackCover));
    let kept = entry(&app);
    assert!(kept.get("cover").is_none() && kept.get("banner").is_some());
    send(
      &mut app,
      VisualMessage::ClearMedia(MediaTarget::TrackBanner),
    );
    assert!(app.settings.visual.track_art.get("ytmusic:a").is_none());
    assert_eq!(app.settings.visual.track_art, serde_json::json!({}));
  }

  #[test]
  fn a_file_that_is_not_png_or_jpeg_is_refused_for_the_track_art() {
    for target in [MediaTarget::TrackCover, MediaTarget::TrackBanner] {
      for name in ["a.gif", "a.webp", "clip.mp4", "notes.txt", "sans-extension"] {
        let mut app = playing();
        open_art(&mut app);
        let effects = picked(&mut app, target, name);
        assert_eq!(
          app.settings.visual.track_art,
          serde_json::json!({}),
          "{name}"
        );
        assert!(
          effects
            .iter()
            .any(|effect| matches!(effect, Effect::Toast(..)))
        );
        assert!(!saved(&effects), "{name}");
      }
    }
  }

  #[test]
  fn a_picked_file_without_the_window_open_is_ignored() {
    let mut app = playing();
    let effects = picked(&mut app, MediaTarget::TrackCover, "a.png");
    assert!(effects.is_empty());
    assert_eq!(app.settings.visual.track_art, serde_json::json!({}));
  }

  #[test]
  fn choosing_a_track_file_opens_the_dialog() {
    let mut app = playing();
    open_art(&mut app);
    assert_eq!(
      send(&mut app, VisualMessage::PickMedia(MediaTarget::TrackBanner)),
      [Effect::PickMedia(MediaTarget::TrackBanner)]
    );
  }

  #[test]
  fn the_effects_of_the_track_replace_the_global_ones() {
    let mut app = playing();
    app.settings.visual.banner_blur = 9;
    app.settings.visual.banner_opacity = 80;
    app.settings.visual.banner_fade = 70;
    assert_eq!(app.banner_effects(), (9, 80, 70));
    open_art(&mut app);
    send(&mut app, VisualMessage::TrackArtFxOn(true));
    assert_eq!(app.banner_effects(), (3, 100, 100), "défauts de Riff");
    send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Blur, 7));
    send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Opacity, 60));
    send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Fade, 40));
    assert_eq!(app.banner_effects(), (7, 60, 40));
    send(&mut app, VisualMessage::TrackArtFxOn(false));
    assert_eq!(app.banner_effects(), (9, 80, 70));
  }

  #[test]
  fn the_sliders_keep_the_bounds_and_steps_of_riff() {
    assert_eq!(ArtSlider::Blur.range(), (0, 20));
    assert_eq!(ArtSlider::Opacity.range(), (10, 100));
    assert_eq!(ArtSlider::Fade.range(), (10, 100));
    assert_eq!(
      [
        ArtSlider::Blur.step(),
        ArtSlider::Opacity.step(),
        ArtSlider::Fade.step()
      ],
      [1, 5, 2]
    );
    let mut app = playing();
    open_art(&mut app);
    send(&mut app, VisualMessage::TrackArtFxOn(true));
    send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Blur, 99));
    send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Opacity, 0));
    assert_eq!(app.banner_effects().0, 20);
    assert_eq!(app.banner_effects().1, 10);
  }

  #[test]
  fn the_effects_are_written_with_the_keys_of_riff_and_the_fade_keeps_its_own_value() {
    let mut app = playing();
    open_art(&mut app);
    send(&mut app, VisualMessage::TrackArtFxOn(true));
    send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Blur, 7));
    send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Opacity, 60));
    let effects = send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Fade, 40));
    assert!(!saved(&effects), "écriture au relâchement");
    assert_eq!(
      entry(&app),
      serde_json::json!({
        "bannerCustomFx": true, "bannerBlur": 7, "bannerOpacity": 60, "bannerFade": 40,
        "bannerFx": { "enabled": true, "blur": 7, "opacity": 60, "fade": 40 }
      })
    );
  }

  #[test]
  fn the_keys_of_riff_survive_a_write_and_a_read() {
    let mut app = playing();
    open_art(&mut app);
    picked(&mut app, MediaTarget::TrackCover, "c.png");
    send(&mut app, VisualMessage::TrackArtFxOn(true));
    send(&mut app, VisualMessage::TrackArtFx(ArtSlider::Fade, 30));
    let written = serde_json::to_value(app.settings.visual.as_ref()).unwrap();
    let read: crate::visuals::settings::VisualSettings = serde_json::from_value(written).unwrap();
    assert_eq!(read.track_art, app.settings.visual.track_art);
    let fx = read.track_fx("ytmusic:a");
    assert_eq!(
      (fx.enabled, fx.blur, fx.opacity, fx.fade),
      (true, 3, 100, 30)
    );
  }

  #[test]
  fn the_effects_of_an_old_entry_with_only_banner_fx_are_read() {
    let mut app = playing();
    app.settings.visual.track_art = serde_json::json!({
      "ytmusic:a": { "bannerFx": { "enabled": true, "blur": 5, "opacity": 50, "fade": 30 } }
    });
    assert_eq!(app.banner_effects(), (5, 50, 30));
    app.settings.visual.track_art = serde_json::json!({
      "ytmusic:a": { "bannerCustomFx": false, "bannerBlur": 1 }
    });
    assert_eq!(app.banner_effects(), (3, 100, 100));
  }
}
