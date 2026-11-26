//! Configuration, render parameters, and track model for Windows wallpaper sync.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Target cover size requested when upgrading YouTube/Google thumbnail URLs.
pub const COVER_FETCH_SIZE: u32 = 544;

/// How renders are mapped across monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
  #[default]
  Primary,
  Span,
}

impl OutputMode {
  #[must_use]
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Primary => "primary",
      Self::Span => "span",
    }
  }

  #[must_use]
  pub fn parse(value: &str) -> Self {
    match value {
      "span" => Self::Span,
      _ => Self::Primary,
    }
  }
}

/// Alpha easing curve for desktop wallpaper crossfade transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransitionEasing {
  #[default]
  Smoothstep,
  Linear,
}

impl TransitionEasing {
  #[must_use]
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Smoothstep => "smoothstep",
      Self::Linear => "linear",
    }
  }

  #[must_use]
  pub fn parse(value: &str) -> Self {
    match value {
      "linear" => Self::Linear,
      _ => Self::Smoothstep,
    }
  }
}

/// Alignment of a scheduled crossfade relative to the track boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransitionAnchor {
  #[default]
  Center,
  End,
  Start,
}

impl TransitionAnchor {
  #[must_use]
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Center => "center",
      Self::End => "end",
      Self::Start => "start",
    }
  }

  #[must_use]
  pub fn parse(value: &str) -> Self {
    match value {
      "end" => Self::End,
      "start" => Self::Start,
      _ => Self::Center,
    }
  }
}

/// Persisted settings for Windows wallpaper synchronization (1:1 defaults from `wallpaper-sync-mod`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WallpaperSyncConfig {
  pub enabled: bool,
  pub output_mode: OutputMode,
  pub force_fill_style: bool,
  pub quality_px: u32,
  pub blur_px: u32,
  pub blur_ref_width: u32,
  pub glow_size_percent: u32,
  pub glow_opacity: f32,
  pub color_mix_ratio: u8,
  pub luminosity_cap: f32,
  pub darken_step: f32,
  pub brightness: f32,
  pub gradient_height: u32,
  pub dither: f32,
  pub transition_enabled: bool,
  pub transition_duration: f32,
  pub transition_fps: u32,
  pub transition_easing: TransitionEasing,
  pub transition_anchor: TransitionAnchor,
  pub transition_arm_lead: f32,
  pub transition_settle: f32,
  pub skip_when_obscured: bool,
  pub prefetch_enabled: bool,
  pub prefetch_eager: bool,
  pub prefetch_lead_seconds: f32,
  pub queue_interval: f32,
  pub cache_enabled: bool,
  pub cache_max_entries: usize,
  pub cache_max_bytes: u64,
  pub restore_on_stop: bool,
  pub verbose_logging: bool,
}

impl Default for WallpaperSyncConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      output_mode: OutputMode::Span,
      force_fill_style: true,
      quality_px: 50,
      blur_px: 100,
      blur_ref_width: 1920,
      glow_size_percent: 100,
      glow_opacity: 1.0,
      color_mix_ratio: 50,
      luminosity_cap: 0.5,
      darken_step: 0.9,
      brightness: 0.6,
      gradient_height: 0,
      dither: 0.0,
      transition_enabled: true,
      transition_duration: 1.0,
      transition_fps: 120,
      transition_easing: TransitionEasing::Smoothstep,
      transition_anchor: TransitionAnchor::Center,
      transition_arm_lead: 0.35,
      transition_settle: 0.08,
      skip_when_obscured: false,
      prefetch_enabled: true,
      prefetch_eager: true,
      prefetch_lead_seconds: 20.0,
      queue_interval: 3.0,
      cache_enabled: true,
      cache_max_entries: 80,
      cache_max_bytes: 134_217_728,
      restore_on_stop: true,
      verbose_logging: false,
    }
  }
}

impl WallpaperSyncConfig {
  /// Resets visual/transition/advanced options to defaults while preserving `enabled`.
  pub fn reset_options(&mut self) {
    let enabled = self.enabled;
    *self = Self {
      enabled,
      ..Self::default()
    };
  }

  /// Extracts parameters that affect the rendered wallpaper bitmap.
  #[must_use]
  pub fn render_params(&self) -> RenderParams {
    RenderParams {
      quality_px: self.quality_px.max(1),
      blur_px: self.blur_px,
      blur_ref_width: self.blur_ref_width.max(1),
      glow_size_percent: self.glow_size_percent.clamp(1, 100),
      glow_opacity: self.glow_opacity.clamp(0.0, 1.0),
      color_mix_ratio: self.color_mix_ratio.min(100),
      luminosity_cap: self.luminosity_cap.clamp(0.05, 1.0),
      darken_step: self.darken_step.clamp(0.1, 0.99),
      brightness: self.brightness.clamp(0.05, 2.0),
      gradient_height: self.gradient_height,
      dither: self.dither.max(0.0),
    }
  }
}

/// Parameters controlling the ambient wallpaper render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderParams {
  pub quality_px: u32,
  pub blur_px: u32,
  pub blur_ref_width: u32,
  pub glow_size_percent: u32,
  pub glow_opacity: f32,
  pub color_mix_ratio: u8,
  pub luminosity_cap: f32,
  pub darken_step: f32,
  pub brightness: f32,
  pub gradient_height: u32,
  pub dither: f32,
}

/// One monitor sub-rectangle within a multi-monitor span canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorTile {
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

/// Lightweight track snapshot passed from the UI/player state to the wallpaper worker.
#[derive(Debug, Clone, PartialEq)]
pub struct WallpaperTrack {
  pub id: String,
  pub title: String,
  pub artist: String,
  pub cover_url: String,
  pub remaining_secs: Option<f64>,
  pub paused: bool,
}

/// Replaces `=wNNN-hNNN` in YouTube/Google artwork URLs with `=w{size}-h{size}`.
#[must_use]
pub fn upgrade_cover_url(url: &str, size: u32) -> String {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| Regex::new(r"=w\d+-h\d+").expect("valid cover size regex"));
  re.replace(url, format!("=w{size}-h{size}")).into_owned()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn defaults_match_wallpaper_sync_mod() {
    let cfg = WallpaperSyncConfig::default();
    assert!(!cfg.enabled);
    assert_eq!(cfg.output_mode, OutputMode::Span);
    assert!(cfg.force_fill_style);
    assert_eq!(cfg.quality_px, 50);
    assert_eq!(cfg.blur_px, 100);
    assert_eq!(cfg.blur_ref_width, 1920);
    assert_eq!(cfg.glow_size_percent, 100);
    assert_eq!(cfg.color_mix_ratio, 50);
    assert!((cfg.brightness - 0.6).abs() < f32::EPSILON);
    assert_eq!(cfg.gradient_height, 0);
    assert!(cfg.dither.abs() < f32::EPSILON);
    assert!(cfg.transition_enabled);
    assert!((cfg.transition_duration - 1.0).abs() < f32::EPSILON);
    assert_eq!(cfg.transition_fps, 120);
    assert_eq!(cfg.transition_easing, TransitionEasing::Smoothstep);
    assert_eq!(cfg.transition_anchor, TransitionAnchor::Center);
    assert!(!cfg.skip_when_obscured);
    assert!(cfg.prefetch_enabled);
    assert!(cfg.prefetch_eager);
    assert!(cfg.cache_enabled);
    assert_eq!(cfg.cache_max_entries, 80);
    assert_eq!(cfg.cache_max_bytes, 134_217_728);
    assert!(cfg.restore_on_stop);
  }

  #[test]
  fn serde_roundtrip_and_partial_deserialization_preserve_defaults() {
    let mut cfg = WallpaperSyncConfig {
      enabled: true,
      output_mode: OutputMode::Primary,
      color_mix_ratio: 40,
      ..WallpaperSyncConfig::default()
    };
    let json = serde_json::to_string(&cfg).expect("serialize");
    let back: WallpaperSyncConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(cfg, back);

    let partial: WallpaperSyncConfig =
      serde_json::from_str(r#"{"enabled":true}"#).expect("partial");
    assert!(partial.enabled);
    assert_eq!(partial.blur_ref_width, 1920);
    assert!(partial.restore_on_stop);

    cfg.reset_options();
    assert!(cfg.enabled);
    assert_eq!(cfg.output_mode, OutputMode::Span);
    assert_eq!(cfg.color_mix_ratio, 50);
  }

  #[test]
  fn upgrade_cover_url_rewrites_google_size_suffix() {
    assert_eq!(
      upgrade_cover_url(
        "https://lh3.googleusercontent.com/abc=w120-h120-l90-rj",
        544
      ),
      "https://lh3.googleusercontent.com/abc=w544-h544-l90-rj"
    );
    assert_eq!(
      upgrade_cover_url("https://example.com/cover.jpg", 544),
      "https://example.com/cover.jpg"
    );
  }
}
