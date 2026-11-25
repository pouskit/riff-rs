//! Réglages des paroles, visualiseurs et fonds, lus depuis `native-settings.json`. Les contrôles sont dans `prefs_visual.rs`.

use crate::lyrics::scroll::clamp_speed;
use crate::settings::LegacyStorage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `riff_lyrics_word_style`. Dans Riff, toute autre valeur que `words` donne `gradient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WordStyle {
  #[default]
  Gradient,
  Words,
}

/// Point de `riff_mesh_cfg`, en pourcentage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MeshPoint {
  pub x: f32,
  pub y: f32,
}

/// `riff_mesh_cfg`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MeshConfig {
  pub p1: MeshPoint,
  pub p2: MeshPoint,
  pub speed: f32,
  pub blur: f32,
}

impl Default for MeshConfig {
  fn default() -> Self {
    Self {
      p1: MeshPoint { x: 20.0, y: 0.0 },
      p2: MeshPoint { x: 85.0, y: 5.0 },
      speed: 0.0,
      blur: 0.0,
    }
  }
}

impl MeshConfig {
  fn parse(raw: &str) -> Option<Self> {
    let parsed: Value = serde_json::from_str(raw).ok()?;
    let point = |key: &str, x: f32, y: f32| -> Option<MeshPoint> {
      let point = parsed.get(key)?;
      let axis = |name: &str, default: f32| {
        point
          .get(name)
          .and_then(Value::as_f64)
          .map_or(default, |value| value as f32)
          .clamp(0.0, 100.0)
      };
      Some(MeshPoint {
        x: axis("x", x),
        y: axis("y", y),
      })
    };
    let number = |key: &str| parsed.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Some(Self {
      p1: point("p1", 20.0, 0.0)?,
      p2: point("p2", 85.0, 5.0)?,
      speed: number("speed").clamp(0.0, 100.0),
      blur: number("blur").clamp(0.0, 60.0),
    })
  }

  /// `--mesh-speed` : une demi-période de l'animation, en secondes. Zéro : image fixe.
  #[must_use]
  pub fn period_secs(&self) -> Option<f32> {
    (self.speed > 0.0).then(|| 120.0 / (self.speed * 0.2 + 1.0))
  }
}

/// Alignement des paroles de la vue plein écran (`data-tvk-align`). Toute autre valeur donne `center`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TvkAlign {
  Left,
  #[default]
  Center,
  Right,
}

impl TvkAlign {
  pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];

  #[must_use]
  pub fn from_key(key: &str) -> Self {
    Self::ALL
      .into_iter()
      .find(|align| align.key() == key)
      .unwrap_or_default()
  }

  #[must_use]
  pub const fn key(self) -> &'static str {
    match self {
      Self::Left => "left",
      Self::Center => "center",
      Self::Right => "right",
    }
  }
}

impl<'de> Deserialize<'de> for TvkAlign {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    Ok(Self::from_key(&String::deserialize(deserializer)?))
  }
}

/// Animation des lignes de la vue plein écran (`data-tvk-anim`). Toute autre valeur donne `slide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TvkAnim {
  Smooth,
  #[default]
  Slide,
  Fade,
  Pop,
  None,
}

impl TvkAnim {
  pub const ALL: [Self; 5] = [Self::Smooth, Self::Slide, Self::Fade, Self::Pop, Self::None];

  #[must_use]
  pub fn from_key(key: &str) -> Self {
    Self::ALL
      .into_iter()
      .find(|anim| anim.key() == key)
      .unwrap_or_default()
  }

  #[must_use]
  pub const fn key(self) -> &'static str {
    match self {
      Self::Smooth => "smooth",
      Self::Slide => "slide",
      Self::Fade => "fade",
      Self::Pop => "pop",
      Self::None => "none",
    }
  }
}

impl<'de> Deserialize<'de> for TvkAnim {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    Ok(Self::from_key(&String::deserialize(deserializer)?))
  }
}

/// `riff_tvk` : style des paroles en plein écran. Valeurs par défaut de `TVK_DEFAULTS` (`app.js`), pas celles du HTML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TvkStyle {
  /// Valeur de Riff : `inherit`, un alias (`serif`) ou une pile CSS. Gardée telle quelle.
  pub font: String,
  pub align: TvkAlign,
  pub size: u32,
  pub weight: u32,
  pub gap: u32,
  pub anim: TvkAnim,
  pub speed: u32,
  /// Clés inconnues de `riff_tvk` : conservées à l'écriture.
  #[serde(flatten)]
  pub extra: serde_json::Map<String, Value>,
}

impl Default for TvkStyle {
  fn default() -> Self {
    Self {
      font: "inherit".to_string(),
      align: TvkAlign::Center,
      size: 100,
      weight: 700,
      gap: 16,
      anim: TvkAnim::Slide,
      speed: 900,
      extra: serde_json::Map::new(),
    }
  }
}

impl TvkStyle {
  /// Bornes des curseurs de `index.html`.
  pub const SIZE: (u32, u32) = (60, 160);
  pub const WEIGHT: (u32, u32) = (400, 900);
  pub const GAP: (u32, u32) = (4, 40);
  pub const SPEED: (u32, u32) = (150, 900);

  /// Les valeurs ramenées dans les bornes des curseurs.
  #[must_use]
  pub fn bounded(&self) -> Self {
    let fit = |value: u32, (low, high): (u32, u32)| value.clamp(low, high);
    Self {
      size: fit(self.size, Self::SIZE),
      weight: fit(self.weight, Self::WEIGHT),
      gap: fit(self.gap, Self::GAP),
      speed: fit(self.speed, Self::SPEED),
      ..self.clone()
    }
  }

  /// `getTvk` : une valeur absente ou invalide garde le défaut. Un nombre est accepté en texte, comme `Number(...)`.
  fn parse(raw: &str) -> Self {
    let defaults = Self::default();
    let Ok(Value::Object(parsed)) = serde_json::from_str::<Value>(raw) else {
      return defaults;
    };
    let number = |key: &str| {
      let value = match parsed.get(key)? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
      };
      value.is_finite().then(|| value.round().max(0.0) as u32)
    };
    // `Number(x) || défaut` : zéro vaut le défaut, sauf pour l'interligne.
    let nonzero =
      |key: &str, default: u32| number(key).filter(|value| *value > 0).unwrap_or(default);
    let text = |key: &str| parsed.get(key).and_then(Value::as_str);
    let known: [&str; 7] = ["font", "align", "size", "weight", "gap", "anim", "speed"];
    Self {
      font: text("font")
        .filter(|font| !font.is_empty())
        .map_or(defaults.font, str::to_string),
      align: text("align").map_or(defaults.align, TvkAlign::from_key),
      size: nonzero("size", defaults.size),
      weight: nonzero("weight", defaults.weight),
      gap: number("gap").unwrap_or(defaults.gap),
      anim: text("anim").map_or(defaults.anim, TvkAnim::from_key),
      speed: nonzero("speed", defaults.speed),
      extra: parsed
        .into_iter()
        .filter(|(key, _)| !known.contains(&key.as_str()))
        .collect(),
    }
    .bounded()
  }
}

/// Fenêtre des mini-paroles (décision 6), propre à l'application native.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MiniWindow {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub always_on_top: bool,
}

impl Default for MiniWindow {
  fn default() -> Self {
    Self {
      x: 80.0,
      y: 80.0,
      width: 420.0,
      height: 240.0,
      always_on_top: false,
    }
  }
}

/// Réglages visuels. Les noms rappellent les clés `riff_*`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VisualSettings {
  pub lyrics_source: String,
  pub word_source: String,
  pub online_lyrics: bool,
  pub karaoke_words: bool,
  pub lyrics_autoscroll: bool,
  pub lyrics_speed: f32,
  pub lyrics_ease: String,
  pub lyrics_word_style: WordStyle,
  pub lyrics_gradient_tightness: u32,
  /// `riff_tr_enabled` : le bouton de traduction est proposé. Ne vaut pas consentement : `translationEnabled` reste prioritaire.
  pub tr_enabled: bool,
  /// `riff_tr_auto` : traduit dès l'arrivée des paroles.
  pub tr_auto: bool,
  pub right_panel_width: f32,
  pub mini_lyrics: bool,
  pub mini_window: MiniWindow,
  /// `riff_tvk_visualizer` : visualiseur quand le titre n'a pas de paroles.
  pub tvk_visualizer: bool,
  /// `riff_tvk_visualizer_lyrics` : visualiseur quand le titre a des paroles.
  pub tvk_visualizer_lyrics: bool,
  pub tvk_visualizer_opacity: u32,
  pub tvk_bg_blur: u32,
  /// `riff_tvk_cover_mode` : pochette à la place des paroles.
  pub tvk_cover_mode: bool,
  /// `riff_tvk`.
  pub tvk: TvkStyle,
  pub bar_wave: bool,
  pub bar_viz_behind: bool,
  pub bar_viz_opacity: u32,
  pub viz_sens: u32,
  pub viz_smooth: u32,
  pub viz_connected: bool,
  pub m3_cover: bool,
  pub cover_bg: bool,
  pub cover_bg_brightness: u32,
  pub bg_url: String,
  pub bg_blur: u32,
  pub bg_opacity: u32,
  pub mesh_gradient: bool,
  pub mesh_brightness: u32,
  /// `riff_adv_mesh_follow_intensity`.
  pub mesh_follow_intensity: bool,
  pub mesh: MeshConfig,
  pub banners: bool,
  pub track_banners: bool,
  pub banner_media: String,
  pub banner_blur: u32,
  pub banner_opacity: u32,
  pub banner_fade: u32,
  pub banner_fit: String,
  /// `riff_track_art` : bannière et pochette par titre.
  pub track_art: Value,
  /// Clés écrites par une autre version : conservées telles quelles à l'écriture.
  #[serde(flatten)]
  pub extra: serde_json::Map<String, Value>,
}

impl Default for VisualSettings {
  fn default() -> Self {
    Self {
      lyrics_source: "lrclib".to_string(),
      word_source: "auto".to_string(),
      online_lyrics: true,
      karaoke_words: true,
      lyrics_autoscroll: true,
      lyrics_speed: 1.0,
      lyrics_ease: "inout".to_string(),
      lyrics_word_style: WordStyle::Gradient,
      lyrics_gradient_tightness: 2,
      tr_enabled: false,
      tr_auto: false,
      right_panel_width: 340.0,
      mini_lyrics: false,
      mini_window: MiniWindow::default(),
      tvk_visualizer: true,
      tvk_visualizer_lyrics: true,
      tvk_visualizer_opacity: 85,
      tvk_bg_blur: 22,
      tvk_cover_mode: false,
      tvk: TvkStyle::default(),
      bar_wave: false,
      bar_viz_behind: true,
      bar_viz_opacity: 22,
      viz_sens: 100,
      viz_smooth: 55,
      viz_connected: false,
      m3_cover: true,
      cover_bg: false,
      cover_bg_brightness: 45,
      bg_url: String::new(),
      bg_blur: 0,
      bg_opacity: 100,
      mesh_gradient: false,
      mesh_brightness: 100,
      mesh_follow_intensity: true,
      mesh: MeshConfig::default(),
      banners: true,
      track_banners: false,
      banner_media: String::new(),
      banner_blur: 3,
      banner_opacity: 100,
      banner_fade: 100,
      banner_fit: "zoom".to_string(),
      track_art: Value::Object(serde_json::Map::new()),
      extra: serde_json::Map::new(),
    }
  }
}

/// `parseInt` : les chiffres en tête, sinon `None`.
fn parse_int(raw: &str) -> Option<u32> {
  let trimmed = raw.trim();
  let end = trimmed
    .find(|ch: char| !ch.is_ascii_digit())
    .unwrap_or(trimmed.len());
  trimmed[..end].parse().ok()
}

/// Effets de bannière d'un titre (`bannerCustomFx`, `bannerBlur`, `bannerOpacity`, `bannerFade`, puis `bannerFx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackFx {
  pub enabled: bool,
  pub blur: u32,
  pub opacity: u32,
  pub fade: u32,
}

impl TrackFx {
  /// Valeurs de Riff quand une clé manque.
  pub const DEFAULT: Self = Self {
    enabled: false,
    blur: 3,
    opacity: 100,
    fade: 100,
  };
}
impl VisualSettings {
  /// Reprend les valeurs de Riff. Une valeur absente ou invalide garde le défaut de Riff.
  #[must_use]
  pub fn imported_from(storage: &LegacyStorage) -> Self {
    let defaults = Self::default();
    let text = |key: &str| storage.text(key);
    let owned = |key: &str, default: &str| text(key).unwrap_or(default).to_string();
    let int =
      |key: &str, default: u32, max: u32| text(key).and_then(parse_int).unwrap_or(default).min(max);
    let adv = |name: &str, default: bool| {
      text(&format!("riff_adv_{name}")).map_or(default, |value| value == "true")
    };
    let tightness = text("riff_lyrics_gradient_tightness")
      .or_else(|| text("riff_gradient_tight"))
      .and_then(parse_int)
      .filter(|value| *value <= 40);
    Self {
      lyrics_source: owned("riff_lyrics_source", &defaults.lyrics_source),
      word_source: owned("riff_word_source", &defaults.word_source),
      online_lyrics: adv("online_lyrics", true),
      karaoke_words: adv("karaoke_words", true),
      lyrics_autoscroll: adv("lyrics_autoscroll", true),
      lyrics_speed: clamp_speed(
        text("riff_lyrics_speed")
          .and_then(|value| value.trim().parse().ok())
          .unwrap_or(f32::NAN),
      ),
      lyrics_ease: owned("riff_lyrics_ease", &defaults.lyrics_ease),
      lyrics_word_style: if text("riff_lyrics_word_style") == Some("words") {
        WordStyle::Words
      } else {
        WordStyle::Gradient
      },
      lyrics_gradient_tightness: tightness.unwrap_or(defaults.lyrics_gradient_tightness),
      tr_enabled: text("riff_tr_enabled") == Some("true"),
      tr_auto: text("riff_tr_auto") == Some("true"),
      right_panel_width: text("riff_right_panel_width")
        .and_then(parse_int)
        .map_or(defaults.right_panel_width, |width| width as f32),
      mini_lyrics: text("riff_mini_lyrics") == Some("1"),
      mini_window: MiniWindow::default(),
      tvk_visualizer: text("riff_tvk_visualizer") != Some("false"),
      tvk_visualizer_lyrics: text("riff_tvk_visualizer_lyrics") != Some("false"),
      tvk_visualizer_opacity: int("riff_tvk_visualizer_opacity", 85, 100).max(10),
      tvk_bg_blur: int("riff_tvk_bg_blur", 22, 60),
      tvk_cover_mode: text("riff_tvk_cover_mode") == Some("1"),
      tvk: text("riff_tvk").map_or_else(TvkStyle::default, TvkStyle::parse),
      bar_wave: text("riff_bar_wave") == Some("1"),
      bar_viz_behind: text("riff_bar_viz_pos").is_none_or(|pos| pos == "behind"),
      bar_viz_opacity: int("riff_bar_viz_opacity", 22, 100),
      viz_sens: text("riff_viz_sens").and_then(parse_int).unwrap_or(100),
      viz_smooth: text("riff_viz_smooth").and_then(parse_int).unwrap_or(55),
      viz_connected: text("riff_viz_connected") == Some("1"),
      m3_cover: text("riff_m3_cover") != Some("0"),
      cover_bg: adv("cover_bg", false),
      cover_bg_brightness: int("riff_cover_bg_brightness", 45, 100),
      bg_url: owned("riff_bg_url", ""),
      bg_blur: int("riff_bg_blur", 0, 100),
      bg_opacity: int("riff_bg_opacity", 100, 100),
      mesh_gradient: adv("mesh_gradient", false),
      mesh_brightness: int("riff_mesh_brightness", 100, 100),
      mesh_follow_intensity: adv("mesh_follow_intensity", true),
      mesh: text("riff_mesh_cfg")
        .and_then(MeshConfig::parse)
        .unwrap_or_default(),
      banners: adv("banners", true),
      track_banners: adv("track_banners", false),
      banner_media: owned("riff_banner_media", ""),
      banner_blur: int("riff_banner_blur", 3, 100),
      banner_opacity: int("riff_banner_opacity", 100, 100),
      banner_fade: int("riff_banner_fade", 100, 100),
      banner_fit: owned("riff_banner_fit", &defaults.banner_fit),
      track_art: text("riff_track_art")
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(defaults.track_art),
      extra: serde_json::Map::new(),
    }
  }

  /// Le titre a une bannière ou une pochette personnalisée dans `riff_track_art`.
  #[must_use]
  pub fn track_banner(&self, canonical_id: &str) -> Option<&str> {
    self
      .track_art
      .get(canonical_id)?
      .get("banner")?
      .as_str()
      .filter(|url| !url.is_empty())
  }

  /// Effets de bannière du titre, lus comme `updateRightPanelBanner` : clés à plat d'abord, puis `bannerFx`.
  #[must_use]
  pub fn track_fx(&self, canonical_id: &str) -> TrackFx {
    let Some(entry) = self.track_art.get(canonical_id) else {
      return TrackFx::DEFAULT;
    };
    let nested = entry.get("bannerFx");
    let number = |flat: &str, inner: &str, default: u32, max: u32| {
      entry
        .get(flat)
        .or_else(|| nested.and_then(|fx| fx.get(inner)))
        .and_then(Value::as_f64)
        .map_or(default, |value| {
          value.round().clamp(0.0, f64::from(max)) as u32
        })
    };
    let flag = |value: Option<&Value>| value.and_then(Value::as_bool).unwrap_or(false);
    TrackFx {
      enabled: flag(entry.get("bannerCustomFx")) || flag(nested.and_then(|fx| fx.get("enabled"))),
      blur: number("bannerBlur", "blur", 3, 20),
      opacity: number("bannerOpacity", "opacity", 100, 100),
      fade: number("bannerFade", "fade", 100, 100),
    }
  }

  /// Un fond remplace la surface unie.
  #[must_use]
  pub fn has_background(&self) -> bool {
    self.cover_bg || !self.bg_url.is_empty() || (self.mesh_gradient && self.mesh_brightness > 0)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;
  use std::fs;

  fn storage(content: &str) -> LegacyStorage {
    let dir = crate::test_temp::TempDir::new("app-visual");
    let path = dir.join("storage.json");
    fs::write(&path, content).unwrap();
    LegacyStorage::open(&path)
  }

  #[test]
  fn an_empty_storage_gives_the_defaults_of_riff() {
    assert_eq!(
      VisualSettings::imported_from(&storage("{}")),
      VisualSettings::default()
    );
    let defaults = VisualSettings::default();
    assert_eq!(defaults.lyrics_word_style, WordStyle::Gradient);
    assert!(!defaults.bar_wave && !defaults.cover_bg && defaults.m3_cover);
  }

  #[test]
  fn present_keys_are_imported() {
    let settings = VisualSettings::imported_from(&storage(
      r#"{
        "riff_lyrics_source": "kugou", "riff_lyrics_speed": "1.5", "riff_lyrics_ease": "sine",
        "riff_lyrics_word_style": "words", "riff_lyrics_gradient_tightness": "12", "riff_mini_lyrics": "1",
        "riff_tvk_visualizer_lyrics": "false", "riff_tvk_bg_blur": "40", "riff_bar_viz_pos": "above",
        "riff_bar_viz_opacity": "60", "riff_viz_sens": "150", "riff_viz_smooth": "20", "riff_viz_connected": "1",
        "riff_bar_wave": "1", "riff_m3_cover": "0", "riff_adv_cover_bg": "true", "riff_cover_bg_brightness": "70",
        "riff_bg_url": "file:///a.png", "riff_bg_blur": "8", "riff_bg_opacity": "50", "riff_adv_track_banners": "true",
        "riff_banner_blur": "9", "riff_banner_fade": "40", "riff_banner_fit": "fit", "riff_banner_media": "b.png",
        "riff_banner_opacity": "30", "riff_right_panel_width": "400px", "riff_adv_mesh_gradient": "true",
        "riff_mesh_cfg": "{\"p1\":{\"x\":1,\"y\":2},\"p2\":{\"x\":3,\"y\":4},\"speed\":5,\"blur\":6}",
        "riff_track_art": "{\"id1\":{\"banner\":\"x.png\"}}"
      }"#,
    ));
    assert_eq!(settings.lyrics_source, "kugou");
    assert_eq!(settings.lyrics_speed, 1.5);
    assert_eq!(settings.lyrics_ease, "sine");
    assert_eq!(settings.lyrics_word_style, WordStyle::Words);
    assert_eq!(settings.lyrics_gradient_tightness, 12);
    assert!(settings.mini_lyrics && !settings.tvk_visualizer_lyrics && !settings.bar_viz_behind);
    assert_eq!(
      (
        settings.tvk_bg_blur,
        settings.bar_viz_opacity,
        settings.viz_sens,
        settings.viz_smooth
      ),
      (40, 60, 150, 20)
    );
    assert!(settings.viz_connected && settings.bar_wave && !settings.m3_cover && settings.cover_bg);
    assert_eq!(
      (
        settings.cover_bg_brightness,
        settings.bg_blur,
        settings.bg_opacity
      ),
      (70, 8, 50)
    );
    assert_eq!(settings.bg_url, "file:///a.png");
    assert!(settings.track_banners && settings.mesh_gradient);
    assert_eq!(
      (
        settings.banner_blur,
        settings.banner_fade,
        settings.banner_opacity
      ),
      (9, 40, 30)
    );
    assert_eq!(
      (settings.banner_fit.as_str(), settings.banner_media.as_str()),
      ("fit", "b.png")
    );
    assert_eq!(settings.right_panel_width, 400.0);
    assert_eq!(settings.mesh.p2, MeshPoint { x: 3.0, y: 4.0 });
    assert_eq!((settings.mesh.speed, settings.mesh.blur), (5.0, 6.0));
    assert_eq!(settings.track_banner("id1"), Some("x.png"));
    assert_eq!(settings.track_banner("autre"), None);
  }

  #[test]
  fn the_translation_options_are_off_by_default_and_imported() {
    let defaults = VisualSettings::default();
    assert!(!defaults.tr_enabled && !defaults.tr_auto && defaults.mesh_follow_intensity);
    let settings = VisualSettings::imported_from(&storage(
      r#"{ "riff_tr_enabled": "true", "riff_tr_auto": "true", "riff_adv_mesh_follow_intensity": "false" }"#,
    ));
    assert!(settings.tr_enabled && settings.tr_auto && !settings.mesh_follow_intensity);
    let other = VisualSettings::imported_from(&storage(
      r#"{ "riff_tr_enabled": "1", "riff_tr_auto": "oui" }"#,
    ));
    assert!(!other.tr_enabled && !other.tr_auto);
  }

  #[test]
  fn a_key_written_by_another_version_survives_a_round_trip() {
    let mut value = serde_json::to_value(VisualSettings::default()).unwrap();
    value["futureKey"] = serde_json::json!({ "a": [1, 2] });
    let settings: VisualSettings = serde_json::from_value(value).unwrap();
    assert_eq!(
      settings.extra.get("futureKey"),
      Some(&serde_json::json!({ "a": [1, 2] }))
    );
    let written = serde_json::to_value(&settings).unwrap();
    assert_eq!(written["futureKey"], serde_json::json!({ "a": [1, 2] }));
    assert_eq!(written["barWave"], serde_json::json!(false));
  }

  #[test]
  fn invalid_values_keep_the_defaults() {
    let settings = VisualSettings::imported_from(&storage(
      r#"{ "riff_lyrics_speed": "9", "riff_lyrics_gradient_tightness": "41", "riff_viz_sens": "beaucoup", "riff_mesh_cfg": "{ pas du json", "riff_track_art": "[" }"#,
    ));
    assert_eq!(settings, VisualSettings::default());
  }

  fn tvk(raw: &str) -> TvkStyle {
    let content = serde_json::to_string(&json!({ "riff_tvk": raw })).unwrap();
    VisualSettings::imported_from(&storage(&content)).tvk
  }

  #[test]
  fn the_karaoke_style_starts_with_the_defaults_of_the_js() {
    let settings = VisualSettings::imported_from(&storage("{}"));
    assert_eq!(settings.tvk, TvkStyle::default());
    let style = &settings.tvk;
    assert_eq!(style.font, "inherit");
    assert_eq!(
      (style.align, style.anim),
      (TvkAlign::Center, TvkAnim::Slide)
    );
    assert_eq!(
      (style.size, style.weight, style.gap, style.speed),
      (100, 700, 16, 900)
    );
    assert!(settings.tvk_visualizer && !settings.tvk_cover_mode);
  }

  #[test]
  fn a_complete_riff_tvk_is_imported() {
    let style = tvk(
      r#"{"font":"serif","align":"left","size":130,"weight":500,"gap":24,"anim":"pop","speed":300}"#,
    );
    assert_eq!(style.font, "serif");
    assert_eq!((style.align, style.anim), (TvkAlign::Left, TvkAnim::Pop));
    assert_eq!(
      (style.size, style.weight, style.gap, style.speed),
      (130, 500, 24, 300)
    );
  }

  #[test]
  fn a_partial_riff_tvk_keeps_the_defaults_for_the_rest() {
    let style = tvk(r#"{"size":"120","anim":"fade"}"#);
    assert_eq!((style.size, style.anim), (120, TvkAnim::Fade));
    assert_eq!(
      TvkStyle {
        size: 100,
        anim: TvkAnim::Slide,
        ..style
      },
      TvkStyle::default()
    );
  }

  #[test]
  fn an_invalid_riff_tvk_gives_the_defaults() {
    for raw in ["pas du json", "[1,2]", "null", "42", "\"texte\"", "{}"] {
      assert_eq!(tvk(raw), TvkStyle::default(), "{raw}");
    }
    let style = tvk(
      r#"{"font":"","align":"haut","size":"abc","weight":0,"gap":"x","anim":"zoom","speed":null}"#,
    );
    assert_eq!(style, TvkStyle::default());
  }

  #[test]
  fn the_values_of_riff_tvk_stay_inside_the_bounds_of_the_sliders() {
    let high = tvk(r#"{"size":999,"weight":1000,"gap":500,"speed":5000}"#);
    assert_eq!(
      (high.size, high.weight, high.gap, high.speed),
      (160, 900, 40, 900)
    );
    let low = tvk(r#"{"size":1,"weight":100,"gap":0,"speed":1}"#);
    assert_eq!(
      (low.size, low.weight, low.gap, low.speed),
      (60, 400, 4, 150)
    );
  }

  #[test]
  fn the_flags_of_the_karaoke_are_imported() {
    let settings = VisualSettings::imported_from(&storage(
      r#"{ "riff_tvk_visualizer": "false", "riff_tvk_cover_mode": "1" }"#,
    ));
    assert!(!settings.tvk_visualizer && settings.tvk_cover_mode);
    let other = VisualSettings::imported_from(&storage(
      r#"{ "riff_tvk_visualizer": "oui", "riff_tvk_cover_mode": "0" }"#,
    ));
    assert!(other.tvk_visualizer && !other.tvk_cover_mode);
  }

  #[test]
  fn an_unknown_key_of_riff_tvk_survives_the_import_and_a_round_trip() {
    let style = tvk(r#"{"size":110,"future":{"a":[1]}}"#);
    assert_eq!(style.extra.get("future"), Some(&json!({ "a": [1] })));
    let written = serde_json::to_value(VisualSettings {
      tvk: style,
      ..VisualSettings::default()
    })
    .unwrap();
    assert_eq!(written["tvk"]["future"], json!({ "a": [1] }));
    assert_eq!(written["tvk"]["size"], json!(110));
    assert_eq!(written["tvk"]["anim"], json!("slide"));
  }

  #[test]
  fn the_karaoke_style_is_written_in_the_visual_section() {
    let settings = VisualSettings {
      tvk: TvkStyle {
        align: TvkAlign::Right,
        anim: TvkAnim::None,
        size: 80,
        ..TvkStyle::default()
      },
      tvk_visualizer: false,
      tvk_cover_mode: true,
      ..VisualSettings::default()
    };
    let written = serde_json::to_value(&settings).unwrap();
    assert_eq!(written["tvk"]["align"], json!("right"));
    assert_eq!(written["tvk"]["anim"], json!("none"));
    assert_eq!(written["tvkVisualizer"], json!(false));
    assert_eq!(written["tvkCoverMode"], json!(true));
    let read: VisualSettings = serde_json::from_value(written).unwrap();
    assert_eq!(read, settings);
  }

  #[test]
  fn an_unknown_align_or_animation_in_the_file_does_not_break_the_section() {
    let read: VisualSettings =
      serde_json::from_value(json!({ "tvk": { "align": "haut", "anim": "zoom", "size": 90 } }))
        .unwrap();
    assert_eq!(
      (read.tvk.align, read.tvk.anim, read.tvk.size),
      (TvkAlign::Center, TvkAnim::Slide, 90)
    );
  }

  #[test]
  fn the_style_is_read_bounded_whatever_the_file_says() {
    let style = TvkStyle {
      size: 5000,
      weight: 1,
      gap: 0,
      speed: 0,
      ..TvkStyle::default()
    };
    let bounded = style.bounded();
    assert_eq!(
      (bounded.size, bounded.weight, bounded.gap, bounded.speed),
      (160, 400, 4, 150)
    );
  }

  #[test]
  fn the_mesh_period_follows_its_speed() {
    assert_eq!(MeshConfig::default().period_secs(), None);
    let mesh = MeshConfig {
      speed: 20.0,
      ..MeshConfig::default()
    };
    assert_eq!(mesh.period_secs(), Some(24.0));
  }

  #[test]
  fn any_background_replaces_the_plain_surface() {
    assert!(!VisualSettings::default().has_background());
    assert!(
      VisualSettings {
        cover_bg: true,
        ..VisualSettings::default()
      }
      .has_background()
    );
    assert!(
      VisualSettings {
        bg_url: "a.png".into(),
        ..VisualSettings::default()
      }
      .has_background()
    );
    assert!(
      VisualSettings {
        mesh_gradient: true,
        ..VisualSettings::default()
      }
      .has_background()
    );
    assert!(
      !VisualSettings {
        mesh_gradient: true,
        mesh_brightness: 0,
        ..VisualSettings::default()
      }
      .has_background()
    );
  }
}
