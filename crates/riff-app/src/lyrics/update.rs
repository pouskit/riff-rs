//! Messages des paroles et image d'animation. Port de `loadSyncedLyrics`, `updateSyncedLyricsHighlight`,
//! `openCustomLyricsEditor`, `openSyncMode` et de la décision de lancer ou non `startVisualizers`.

use super::model::Lyrics;
use super::scroll::{Easing, ScrollAnimation, clamp_speed};
use super::state::{
  EditorContent, LINE_PADDING_X, LINE_SPACING, LayoutParams, LyricsLayout, PANEL_FONT, SyncLine,
  SyncState, lyric_font, tvk_font,
};
use super::tvk::{self, TvkMessage};
use crate::app::{App, Effect};
use crate::jam::protocol::Op;
use crate::theme::tokens;
use crate::ui::ReplyTag;
use crate::visuals::settings::WordStyle;
use crate::visuals::viz::{self, FREQ_BINS};
use crate::widgets::toast::ToastKind;
use iced::widget::text_editor;
use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// Les images plus rapprochées sont ignorées : deux fenêtres ou un écran rapide gardent le pas de 60 Hz.
pub const MIN_TICK: Duration = Duration::from_millis(14);
/// `body.perf` : `timestamp - lastFrameTime < 33` saute l'image (`app.js:3080`).
pub const PERF_TICK: Duration = Duration::from_millis(33);
/// Écart maximal de la position lissée par rapport au dernier événement du moteur.
const MAX_INTERPOLATION: f64 = 1.0;

static GAP_LINE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)^\[?\s*(?:instrumental|music|intro|outro|solo)\s*\]?$").expect("motif valide")
});

/// Messages des paroles.
#[derive(Debug, Clone)]
pub enum LyricsMessage {
  TogglePanel,
  ToggleMode,
  ToggleTranslate,
  Scrolled { offset: f32, viewport: f32 },
  LineClicked(usize),
  OpenEditor,
  EditorAction(text_editor::Action),
  EditorSave,
  EditorClose,
  OpenSync,
  SyncLine(usize),
  SyncRestart,
  SyncSave,
  SyncClose,
  OpenTvk,
  CloseTvk,
  Tvk(TvkMessage),
  ToggleMini,
  MiniOnTop,
  MiniClosed,
  MiniMoved(iced::Point),
  MiniResized(iced::Size),
  MiniDrag,
}

impl App {
  /// Demande les paroles du titre courant.
  pub(crate) fn load_lyrics(&mut self) -> Vec<Effect> {
    let token = self.player.token;
    let state = &mut self.lyrics;
    state.lyrics = None;
    state.layout = LyricsLayout::default();
    state.active = None;
    state.transition = None;
    state.animation = None;
    state.offset = 0.0;
    state.position = 0.0;
    state.clock_at = None;
    state.clock_base = 0.0;
    state.translations = None;
    state.translate_on = false;
    state.loading = true;
    state.token = token;
    let Some(track) = self.player.current.as_ref() else {
      state.loading = false;
      return Vec::new();
    };
    let mut query = vec![
      ("title".to_string(), track.title.clone().unwrap_or_default()),
      (
        "artist".to_string(),
        track.artist.clone().unwrap_or_default(),
      ),
      (
        "online".to_string(),
        if self.settings.visual.online_lyrics {
          "1"
        } else {
          "0"
        }
        .to_string(),
      ),
      (
        "source".to_string(),
        self.settings.visual.lyrics_source.clone(),
      ),
    ];
    if let Some(duration) = track.duration {
      query.push(("duration".to_string(), duration.to_string()));
    }
    vec![Effect::CoreJson {
      tag: ReplyTag::Lyrics { token },
      path: "/api/lyrics",
      query,
      body: None,
      search: false,
    }]
  }

  fn lyrics_words_request(&self, token: u64) -> Vec<Effect> {
    if self.settings.visual.word_source == "estimate" {
      return Vec::new();
    }
    let Some(track) = self.player.current.as_ref() else {
      return Vec::new();
    };
    let duration = track
      .duration
      .map(|value| value.to_string())
      .or_else(|| {
        self
          .playback
          .duration
          .map(|value| value.as_secs_f64().to_string())
      })
      .unwrap_or_default();
    let query = vec![
      ("title".to_string(), track.title.clone().unwrap_or_default()),
      (
        "artist".to_string(),
        track.artist.clone().unwrap_or_default(),
      ),
      ("duration".to_string(), duration),
    ];
    vec![Effect::CoreJson {
      tag: ReplyTag::LyricsWords { token },
      path: "/api/lyrics-words",
      query,
      body: None,
      search: false,
    }]
  }

  /// Réponses du core pour les paroles.
  pub(crate) fn lyrics_reply(
    &mut self,
    tag: &ReplyTag,
    result: Result<Value, String>,
  ) -> Vec<Effect> {
    match *tag {
      ReplyTag::Lyrics { token } => {
        if token != self.lyrics.token {
          return Vec::new();
        }
        self.lyrics.loading = false;
        let Ok(reply) = result else {
          return Vec::new();
        };
        let Some(lyrics) = Lyrics::from_reply(&reply) else {
          self.lyrics.lyrics = None;
          self.relayout();
          return Vec::new();
        };
        self.lyrics.lyrics = Some(lyrics);
        self.relayout();
        let mut effects = self.lyrics_words_request(token);
        // `app.js:5241` : `trEnabled && trAuto` lance la traduction à l'arrivée des paroles.
        if self.settings.visual.tr_auto && !self.lyrics.translate_on {
          effects.extend(self.start_translation());
        }
        effects
      }
      ReplyTag::LyricsWords { token } => {
        if token != self.lyrics.token {
          return Vec::new();
        }
        if let (Ok(reply), Some(lyrics)) = (result, self.lyrics.lyrics.as_mut())
          && lyrics.merge_words(&reply)
        {
          self.relayout();
        }
        Vec::new()
      }
      ReplyTag::LyricsTranslated { token } => {
        if token != self.lyrics.token || !self.lyrics.translate_on {
          return Vec::new();
        }
        match result {
          Ok(reply) if reply.get("success").and_then(Value::as_bool) == Some(true) => {
            if let Some(items) = reply.get("translations").and_then(Value::as_array) {
              self.lyrics.translations = Some(
                items
                  .iter()
                  .map(|item| item.as_str().unwrap_or("").to_string())
                  .collect(),
              );
              self.relayout();
            }
          }
          Ok(reply) => {
            let error = reply.get("error").and_then(Value::as_str).unwrap_or("");
            tracing::warn!(
              error = %riff_core::logging::redact_urls(error),
              "traduction des paroles refusée"
            );
          }
          Err(error) => tracing::warn!(
            error = %riff_core::logging::redact_urls(&error),
            "traduction des paroles en échec"
          ),
        }
        Vec::new()
      }
      ReplyTag::LyricsSaved { success_message } => match result {
        Ok(reply) if reply.get("success").and_then(Value::as_bool) == Some(true) => {
          self.lyrics.editor = None;
          self.lyrics.sync = None;
          let mut effects = vec![Effect::Toast(success_message.to_string(), ToastKind::Info)];
          effects.extend(self.load_lyrics());
          effects
        }
        Ok(reply) => vec![save_failed(
          reply
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Save failed"),
        )],
        Err(reason) => vec![save_failed(&reason)],
      },
      _ => Vec::new(),
    }
  }

  /// Largeur et hauteur de la zone des paroles.
  fn lyric_zone(&self) -> (f32, f32) {
    let window = self.window.size.unwrap_or(crate::app::DEFAULT_WINDOW);
    if self.lyrics.tvk_open {
      let geometry = tvk::geometry(window);
      return (
        (geometry.line - tokens::css(LINE_PADDING_X) * 2.0).max(40.0),
        (window.height - geometry.pad_y * 2.0).max(100.0),
      );
    }
    let panel = self.settings.visual.right_panel_width;
    // Panneau, zone des paroles puis marge de chaque ligne : tout en pixels CSS convertis.
    let width = tokens::css(panel)
      - tokens::right_panel::PADDING_SIDE * 2.0
      - tokens::right_panel::LYRICS_PADDING_X * 2.0
      - tokens::css(LINE_PADDING_X) * 2.0;
    (
      width.max(40.0),
      (window.height - tokens::right_panel::chrome_height(panel)).max(60.0),
    )
  }

  /// Recalcule la mise en page après un changement de paroles, de surface ou de taille.
  pub(crate) fn relayout(&mut self) {
    let (width, estimated) = self.lyric_zone();
    let viewport = self.lyrics.reported_viewport.unwrap_or(estimated);
    let window_width = self.window.size.unwrap_or(crate::app::DEFAULT_WINDOW).width;
    let state = &mut self.lyrics;
    state.viewport = viewport;
    let tvk = state.tvk_open;
    let params = LayoutParams {
      width,
      viewport,
      font: if tvk {
        tvk_font(window_width) * tvk::metrics(&self.settings.visual.tvk).scale
      } else {
        tokens::css(PANEL_FONT)
      },
      face: if tvk {
        tvk::face(&self.settings.visual.tvk)
      } else {
        lyric_font(false)
      },
      spacing: if tvk {
        tvk::line_spacing(&self.settings.visual.tvk)
      } else {
        tokens::css(LINE_SPACING)
      },
      style: self.settings.visual.lyrics_word_style,
      word_mode: state.word_mode && self.settings.visual.karaoke_words,
      tvk,
      scale: tokens::ZOOM,
    };
    state.layout = match state.lyrics.as_ref() {
      Some(lyrics) => LyricsLayout::compute(
        lyrics,
        params,
        state.translations.as_deref().filter(|_| state.translate_on),
      ),
      None => LyricsLayout::default(),
    };
    state.active = None;
    state.transition = None;
    state.animation = None;
  }

  /// Le mode gradient est actif (`isGradientMode`).
  #[must_use]
  pub fn is_gradient_mode(&self) -> bool {
    self.lyrics.word_mode
      && self.settings.visual.karaoke_words
      && self.settings.visual.lyrics_word_style == WordStyle::Gradient
  }

  /// La traduction est proposée : décision 8. Il faut le consentement du core (`translationEnabled`, coupé par défaut)
  /// et `riff_tr_enabled`, qui masque le bouton dans Riff (`app.js:6096`). L'interrupteur ne vaut jamais consentement.
  #[must_use]
  pub const fn translation_available(&self) -> bool {
    self.lyrics.translation_enabled && self.settings.visual.tr_enabled
  }

  /// `#right-panel` prend sa place : Riff l'ouvre au lancement, avec ou sans titre en cours. Il reste affiché sous `.tv-karaoke`.
  #[must_use]
  pub const fn panel_shown(&self) -> bool {
    self.lyrics.panel_open
  }

  /// `showRpControls` : montre les commandes du panneau, puis les cache 1,8 s après le dernier mouvement.
  pub(crate) fn show_panel_controls(&mut self) -> Vec<Effect> {
    let controls = &mut self.ui.panel_controls;
    controls.visible = true;
    controls.last_move = Some(Instant::now());
    controls.generation += 1;
    vec![Effect::PanelControlsTimer(
      controls.generation,
      tokens::right_panel::CONTROLS_AFTER_MOVE,
    )]
  }

  /// Le panneau de paroles est affiché.
  #[must_use]
  pub fn panel_visible(&self) -> bool {
    self.lyrics.panel_open && self.player.current.is_some()
  }

  /// Fenêtre principale réduite ou masquée dans la zone de notification.
  #[must_use]
  pub fn is_minimized(&self) -> bool {
    self.window.main_hidden
      || self
        .window
        .position
        .is_some_and(|position| position.x <= crate::window::MINIMIZED_POSITION)
  }

  /// Une surface montre les paroles : le mode pochette de la vue plein écran les masque.
  fn lyrics_visible(&self) -> bool {
    // Sous le plein écran, `openTvKaraoke` a déplacé les paroles hors du panneau : il reste affiché, sans paroles.
    (self.panel_visible() && !self.lyrics.tvk_open)
      || (self.lyrics.tvk_open && self.tvk_lyrics_shown())
      || self.lyrics.mini_open
      || (self.compact_layout() && self.window.mini_lyrics)
  }

  /// Pochette façonnée de la barre de lecture : `m3cEnabled` est faux sous `body.perf` (`app.js:16812`).
  #[must_use]
  pub fn m3_cover_on(&self) -> bool {
    self.settings.visual.m3_cover && !crate::motion::perf()
  }

  /// Un visualiseur lit l'analyseur : sinon il passe à sa petite FFT (décision 5).
  #[must_use]
  pub fn visualizer_visible(&self) -> bool {
    // `body.perf` : l'analyseur n'est plus lu et les visualiseurs ne sont plus dessinés (`app.js:3089`).
    if self.is_minimized() || crate::motion::perf() {
      return false;
    }
    let bars = self.settings.visual.bar_wave && !self.lyrics.tvk_open;
    let tvk = self.tvk_visualizer_shown();
    let gap = self.lyrics.active_gap() && self.lyrics_visible();
    self.player.current.is_some() && (bars || tvk || gap)
  }

  /// Un calque ou la barre de lecture a besoin de la prochaine image (décision 4). Faux : aucun rendu demandé.
  #[must_use]
  pub fn animation_needed(&self) -> bool {
    self.layers_need_frames() || self.playbar_needs_frames() || self.theme_fade.running(self.now)
  }

  /// La barre de lecture demande des images : l'onde ondule avec l'horloge pendant la lecture (`app.js:3453`), et le bouton
  /// lecture/pause ou le suivi de la position ont une transition en cours.
  fn playbar_needs_frames(&self) -> bool {
    if self.is_minimized() {
      return false;
    }
    // `body.no-wavy` (posé par `body.perf`) : trait droit, la phase n'est plus dessinée.
    let wave = !crate::motion::perf()
      && self.player.current.is_some()
      && self.playback.is_playing
      && !self.lyrics.tvk_open;
    wave || self.playbar_motion.animating(self.now)
  }

  /// Un calque a besoin de la prochaine image, sans la barre de lecture.
  #[must_use]
  pub fn layers_need_frames(&self) -> bool {
    if self.is_minimized() {
      return self.lyrics.mini_open && self.playback.is_playing && self.lyrics.is_synced();
    }
    if self.visuals.warmup_frames > 0 {
      return true;
    }
    let visual = &self.settings.visual;
    let playing = self.playback.is_playing;
    let scrolling = (self.lyrics.animation.is_some() || self.lyrics.transition.is_some())
      && self.lyrics_visible();
    let lyrics = playing && self.lyrics.is_synced() && self.lyrics_visible();
    let current = self.player.current.is_some();
    let drawn = !crate::motion::perf();
    let bars = drawn && current && visual.bar_wave && (playing || self.visuals.bar.active);
    let tvk = drawn && self.tvk_visualizer_shown() && (playing || self.visuals.tvk.active);
    let shape = current
      && self.m3_cover_on()
      && !self.lyrics.tvk_open
      && (playing || self.visuals.shape.is_settling(playing));
    // Le calque du fond et l'aperçu des réglages ont chacun leur `meshFloat` ; speed 0 : `animation-duration: 0s`, rien ne bouge.
    let mesh = visual.mesh.period_secs().is_some()
      && !crate::motion::mesh_frozen()
      && (self.mesh_shown()
        || (self.ui.prefs.mesh_open
          && self.screen == crate::screens::Screen::Settings
          && visual.mesh_gradient
          && visual.mesh_brightness > 0));
    // Les `.g-spinner` (toast spinner, chargement d'un album) et les `.m3-circular-progress` de l'accueil tournent avec l'horloge de `App`.
    let spinners = self.toasts.animating(self.now)
      || self.play_wait.animating(self.now)
      || (self.screen == crate::screens::Screen::Search
        && (self.ui.search.loading || self.ui.search.error.is_some()))
      || (self.screen == crate::screens::Screen::Album && self.ui.album.loading)
      || self.home_spinner.animating()
      || self.trending_spinner.animating();
    // Page artiste : le badge tourne tant qu'il est affiché (`infinite`), le chargeur tant que la réponse n'est pas là.
    let artist = self.screen == crate::screens::Screen::Artist && {
      let view = &self.ui.artist;
      let motion = &self.artist_motion;
      (motion.badge.is_some()
        && view.verified
        && !view.loading
        && !crate::motion::Gate::BOTH.instant())
        || (motion.loader.is_some() && view.loading)
        || motion.pane_running(self.now)
    };
    let motion = artist
      || self.view_motion.running(self.now)
      || self.ui.home.lift.animating(self.now)
      || self.ui.controls.animating(self.now)
      || self.ui.row_fade.animating(self.now)
      || self.bars_animating()
      || self.overlay_motion.animating(self.now)
      || self.ui.queue_panel.motion.running(self.now)
      || self.ui.jam_panel.motion.running(self.now)
      || self.audio_animating()
      || self.lyrics.tvk_motion.running(self.now)
      || self.lyrics.panel_motion.running(self.now)
      || self
        .entry_motion
        .running(self.now, self.data.playlists.len());
    // Les orbes du fond tournent tant que la vue plein écran est ouverte (`tvkOrbit1-4`, `infinite`).
    let orbits = self.lyrics.tvk_open && self.lyrics.tvk_motion.orbiting();
    scrolling
      || motion
      || orbits
      || lyrics
      || bars
      || tvk
      || shape
      || mesh
      || spinners
      || self.visuals.layers_active()
      || self.splash.is_some()
  }

  /// Le minuteur vit dans la fenêtre principale tant qu'elle est dessinée.
  #[must_use]
  pub fn main_ticker(&self) -> bool {
    !self.visuals.unthrottled && !self.is_minimized() && self.animation_needed()
  }

  /// Fenêtre principale réduite : le minuteur passe dans les mini-paroles.
  #[must_use]
  pub fn mini_ticker(&self) -> bool {
    !self.visuals.unthrottled && self.is_minimized() && self.animation_needed()
  }

  /// Une image d'animation : `freq` vaut les octets de l'analyseur si un visualiseur est visible.
  pub fn animation_tick(&mut self, at: Instant, freq: &[u8; FREQ_BINS]) -> Vec<Effect> {
    let mut effects = self.toast_frame(at);
    if self.menu_leaving.is_some() && !self.overlay_motion.menu_fading(at) {
      self.menu_leaving = None;
    }
    self.visuals.warmup_frames = self.visuals.warmup_frames.saturating_sub(1);
    self.sync_mesh(at);
    self.sync_layers(at);
    let tvk_bars = self.tvk_visualizer_shown();
    let playing = self.playback.is_playing;
    self.visuals.bar_width =
      (self.window.size.unwrap_or(crate::app::DEFAULT_WINDOW).width - 32.0).max(200.0);
    let delta_ms = self.visuals.last_tick.map_or(0.0, |last| {
      at.saturating_duration_since(last).as_secs_f32() * 1000.0
    });
    self.visuals.last_tick = Some(at);
    self.visuals.freq = *freq;
    let visual = &self.settings.visual;
    let (smooth, sens) = viz::viz_settings(visual.viz_smooth, visual.viz_sens);
    // `body.perf` : ni lecture de l'analyseur ni dessin des visualiseurs, les barres gardent leur dernier état (`app.js:3089`).
    let drawn = !crate::motion::perf();
    if drawn && visual.bar_wave {
      if playing {
        self.visuals.bar.update(freq, smooth, sens);
      } else {
        self.visuals.bar.decay();
      }
      if visual.viz_connected {
        let heights = self.visuals.bar.symmetric();
        self.visuals.liquid_points =
          self
            .visuals
            .liquid
            .points(&heights, self.visuals.bar_width, viz::BAR_HEIGHT);
      }
    }
    if drawn && tvk_bars {
      if playing {
        self.visuals.tvk.update(freq, smooth, sens, TVK_BAR_ZONE);
      } else {
        self.visuals.tvk.decay();
      }
    }
    if self.m3_cover_on() {
      self.visuals.shape.advance(f64::from(delta_ms), playing);
    }
    self.update_position(at);
    effects.extend(self.follow_active_line(at));
    effects
  }

  /// Lisse la position entre deux événements du moteur.
  fn update_position(&mut self, at: Instant) {
    let engine = self.playback.position.as_secs_f64();
    let state = &mut self.lyrics;
    if state.clock_at.is_none() || (engine - state.clock_base).abs() > f64::EPSILON {
      state.clock_base = engine;
      state.clock_at = Some(at);
    }
    state.position = if self.playback.is_playing {
      let elapsed = state
        .clock_at
        .map_or(0.0, |base| at.saturating_duration_since(base).as_secs_f64());
      engine + elapsed.min(MAX_INTERPOLATION)
    } else {
      engine
    };
  }

  /// `updateSyncedLyricsHighlight` : ligne active, puis défilement.
  fn follow_active_line(&mut self, at: Instant) -> Vec<Effect> {
    let mut effects = Vec::new();
    let mut changed_from = None;
    let visual = &self.settings.visual;
    let state = &mut self.lyrics;
    if let Some(lyrics) = state.lyrics.as_ref() {
      let active = lyrics.active_line(state.position);
      if active != state.active {
        changed_from = Some(state.active);
        state.active = active;
        if visual.lyrics_autoscroll {
          let target = active
            .and_then(|index| state.layout.boxes.get(index))
            .map_or(0.0, |line| {
              super::layout::target_scroll(*line, state.viewport)
            });
          state.animation = ScrollAnimation::new(
            state.offset,
            target,
            at,
            clamp_speed(visual.lyrics_speed),
            Easing::from_key(&visual.lyrics_ease),
          );
        }
      }
    }
    if let Some(animation) = state.animation {
      let (value, done) = animation.at(at);
      state.offset = value;
      state.last_set_offset = value;
      effects.push(Effect::LyricsScroll(value));
      if done {
        state.animation = None;
      }
    }
    if let Some(previous) = changed_from {
      self.lyrics.transition = self.line_transition(previous, at);
    }
    self.line_advance(at);
    effects
  }

  /// Messages des paroles.
  pub(crate) fn update_lyrics(&mut self, message: LyricsMessage) -> Vec<Effect> {
    match message {
      LyricsMessage::TogglePanel => {
        self.lyrics.panel_open = !self.lyrics.panel_open;
        if self.lyrics.panel_open {
          self.relayout();
          return self.show_panel_controls();
        }
        Vec::new()
      }
      LyricsMessage::ToggleMode => {
        self.lyrics.word_mode = !self.lyrics.word_mode && self.settings.visual.karaoke_words;
        self.relayout();
        Vec::new()
      }
      LyricsMessage::ToggleTranslate => self.toggle_translate(),
      LyricsMessage::Tvk(message) => self.update_tvk(message),
      LyricsMessage::Scrolled { offset, viewport } => {
        let state = &mut self.lyrics;
        state.offset = offset;
        // `wheel` annule l'animation dans Riff.
        if state.animation.is_some() && (offset - state.last_set_offset).abs() > 2.0 {
          state.animation = None;
        }
        if viewport > 0.0 && (viewport - state.viewport).abs() > 0.5 {
          state.reported_viewport = Some(viewport);
          self.relayout();
        }
        Vec::new()
      }
      LyricsMessage::LineClicked(index) => {
        let start = self
          .lyrics
          .lyrics
          .as_ref()
          .map_or(-1.0, |lyrics| lyrics.line_start(index));
        if start < 0.0 {
          return Vec::new();
        }
        let position = Duration::from_secs_f64(start);
        if let Some(effects) = self.jam_seek(position) {
          return effects;
        }
        self.playback.position = position;
        self.lyrics.clock_at = None;
        vec![Effect::AudioSeek(position)]
      }
      LyricsMessage::OpenEditor => {
        if self.player.current.is_none() {
          return Vec::new();
        }
        let text = self
          .lyrics
          .lyrics
          .as_ref()
          .map_or_else(String::new, |lyrics| {
            lyrics
              .lines
              .iter()
              .map(|line| {
                if line.is_gap {
                  "[Instrumental]"
                } else {
                  line.text.as_str()
                }
              })
              .collect::<Vec<_>>()
              .join("\n")
          });
        self.lyrics.editor = Some(EditorContent(text_editor::Content::with_text(&text)));
        Vec::new()
      }
      LyricsMessage::EditorAction(action) => {
        if let Some(editor) = &mut self.lyrics.editor {
          editor.0.perform(action);
        }
        Vec::new()
      }
      LyricsMessage::EditorSave => {
        let Some(editor) = &self.lyrics.editor else {
          return Vec::new();
        };
        let text = editor.0.text().trim().to_string();
        let message = if text.is_empty() {
          "Lyrics removed"
        } else {
          "Lyrics saved successfully"
        };
        self.save_custom_lyrics(&text, message)
      }
      LyricsMessage::EditorClose => {
        self.lyrics.editor = None;
        Vec::new()
      }
      LyricsMessage::OpenSync => self.open_sync(),
      LyricsMessage::SyncLine(index) => {
        let position = self.lyrics_position();
        if let Some(sync) = &mut self.lyrics.sync {
          sync.mark(index, position);
        }
        Vec::new()
      }
      LyricsMessage::SyncRestart => {
        if let Some(sync) = &mut self.lyrics.sync {
          sync.restart();
          let toast = Effect::Toast("Sync restarted".to_string(), ToastKind::Info);
          if let Some(mut effects) = self.jam_seek(Duration::ZERO) {
            effects.push(toast);
            return effects;
          }
          self.playback.position = Duration::ZERO;
          return vec![Effect::AudioSeek(Duration::ZERO), toast];
        }
        Vec::new()
      }
      LyricsMessage::SyncSave => {
        let Some(sync) = &self.lyrics.sync else {
          return Vec::new();
        };
        let track = sync.track.clone();
        let text = sync.to_lrc();
        self.save_lyrics_for(&track, &text, "Synced lyrics saved")
      }
      LyricsMessage::SyncClose => {
        self.lyrics.sync = None;
        Vec::new()
      }
      LyricsMessage::OpenTvk => {
        if self.player.current.is_none() {
          return Vec::new();
        }
        if !self.lyrics.tvk_open {
          if self.lyrics.tvk_motion.enabled {
            // Hors animation, `now` date de la dernière image : `tvk-in` part de l'instant réel.
            self.now = Instant::now();
          }
          self.lyrics.tvk_motion.open(self.now);
        }
        self.lyrics.tvk_open = true;
        self.lyrics.tvk_idle = false;
        self.lyrics.tvk_active = Some(Instant::now());
        self.lyrics.reported_viewport = None;
        self.relayout();
        Vec::new()
      }
      LyricsMessage::CloseTvk => {
        if self.lyrics.tvk_open {
          if self.lyrics.tvk_motion.enabled {
            self.now = Instant::now();
          }
          self.lyrics.tvk_motion.close(self.now);
        }
        self.lyrics.tvk_open = false;
        self.set_tvk_panel(false);
        self.lyrics.tvk_idle = false;
        self.lyrics.reported_viewport = None;
        self.relayout();
        Vec::new()
      }
      LyricsMessage::ToggleMini => {
        self.settings.visual.mini_lyrics = !self.lyrics.mini_open;
        let effect = if self.lyrics.mini_open {
          Effect::CloseMini
        } else {
          Effect::OpenMini(self.settings.visual.mini_window)
        };
        self.lyrics.mini_open = !self.lyrics.mini_open;
        vec![
          effect,
          Effect::SaveSettings(Box::new(self.settings.clone())),
        ]
      }
      LyricsMessage::MiniOnTop => {
        self.settings.visual.mini_window.always_on_top =
          !self.settings.visual.mini_window.always_on_top;
        vec![
          Effect::MiniLevel(self.settings.visual.mini_window.always_on_top),
          Effect::SaveSettings(Box::new(self.settings.clone())),
        ]
      }
      LyricsMessage::MiniClosed => {
        self.lyrics.mini_open = false;
        self.settings.visual.mini_lyrics = false;
        vec![
          Effect::CloseMini,
          Effect::SaveSettings(Box::new(self.settings.clone())),
        ]
      }
      LyricsMessage::MiniMoved(position) => {
        self.settings.visual.mini_window.x = position.x;
        self.settings.visual.mini_window.y = position.y;
        vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
      }
      LyricsMessage::MiniResized(size) => {
        self.settings.visual.mini_window.width = size.width;
        self.settings.visual.mini_window.height = size.height;
        vec![Effect::SaveSettings(Box::new(self.settings.clone()))]
      }
      LyricsMessage::MiniDrag => vec![Effect::MiniDrag],
    }
  }

  fn lyrics_position(&self) -> f64 {
    if self.lyrics.clock_at.is_some() {
      self.lyrics.position
    } else {
      self.playback.position.as_secs_f64()
    }
  }

  fn toggle_translate(&mut self) -> Vec<Effect> {
    if !self.translation_available() {
      return Vec::new();
    }
    if self.lyrics.translate_on {
      self.stop_translation();
      return Vec::new();
    }
    self.start_translation()
  }

  /// Cache les lignes traduites (`state.trActive = false`).
  pub(crate) fn stop_translation(&mut self) {
    self.lyrics.translate_on = false;
    self.lyrics.translations = None;
    self.relayout();
  }

  /// Active la traduction et la demande si des paroles sont chargées.
  pub(crate) fn start_translation(&mut self) -> Vec<Effect> {
    if !self.translation_available() {
      return Vec::new();
    }
    self.lyrics.translate_on = true;
    self.request_translation()
  }

  /// Demande les lignes traduites dans la langue cible courante (`fetchAndApplyTranslation`). Rien ne part sans consentement.
  pub(crate) fn request_translation(&self) -> Vec<Effect> {
    if !self.translation_available() {
      return Vec::new();
    }
    let Some(lyrics) = self.lyrics.lyrics.as_ref() else {
      return Vec::new();
    };
    let texts: Vec<&str> = lyrics.lines.iter().map(|line| line.text.as_str()).collect();
    let body = json!({ "texts": texts, "targetLang": self.translation_target() }).to_string();
    vec![Effect::CoreJson {
      tag: ReplyTag::LyricsTranslated {
        token: self.lyrics.token,
      },
      path: "/api/translate",
      query: Vec::new(),
      body: Some(body),
      search: false,
    }]
  }

  fn save_custom_lyrics(&self, text: &str, message: &'static str) -> Vec<Effect> {
    match self.player.current.clone() {
      Some(track) => self.save_lyrics_for(&track, text, message),
      None => Vec::new(),
    }
  }

  fn save_lyrics_for(
    &self,
    track: &crate::user_data::Track,
    text: &str,
    message: &'static str,
  ) -> Vec<Effect> {
    let query = vec![
      ("title".to_string(), track.title.clone().unwrap_or_default()),
      (
        "artist".to_string(),
        track.artist.clone().unwrap_or_default(),
      ),
    ];
    vec![Effect::CoreJson {
      tag: ReplyTag::LyricsSaved {
        success_message: message,
      },
      path: "/api/save-custom-lyrics",
      query,
      body: Some(json!({ "lyrics": text }).to_string()),
      search: false,
    }]
  }

  /// `openSyncMode`.
  fn open_sync(&mut self) -> Vec<Effect> {
    let Some(track) = self.player.current.clone() else {
      return Vec::new();
    };
    let lines: Vec<SyncLine> = self
      .lyrics
      .lyrics
      .as_ref()
      .map(|lyrics| {
        lyrics
          .lines
          .iter()
          .map(|line| {
            let is_gap = line.is_gap || GAP_LINE.is_match(&line.text);
            SyncLine {
              text: if is_gap {
                "[Instrumental]".to_string()
              } else {
                line.text.trim().to_string()
              },
              is_gap,
              time: None,
            }
          })
          .filter(|line| !line.text.is_empty())
          .collect()
      })
      .unwrap_or_default();
    if lines.is_empty() {
      return vec![Effect::Toast(
        "Add lyrics first using the edit button, then sync them".to_string(),
        ToastKind::Info,
      )];
    }
    self.lyrics.sync = Some(SyncState {
      track,
      lines,
      current: 0,
    });
    let toast = Effect::Toast(
      "Click or tap each line when you hear it".to_string(),
      ToastKind::Info,
    );
    if let Some(mut effects) = self.jam_seek(Duration::ZERO) {
      effects.extend(self.jam_divert(Op::Play).unwrap_or_default());
      effects.push(toast);
      return effects;
    }
    self.playback.position = Duration::ZERO;
    self.playback.is_playing = true;
    vec![Effect::AudioSeek(Duration::ZERO), Effect::AudioPlay, toast]
  }

  /// Espace pendant la synchro : note la ligne attendue. `None` si la synchro est fermée.
  pub(crate) fn sync_space(&mut self) -> Option<Vec<Effect>> {
    let current = self.lyrics.sync.as_ref()?.current;
    Some(self.update_lyrics(LyricsMessage::SyncLine(current)))
  }

  /// Échap : d'abord la synchro, l'éditeur, le panneau de personnalisation, puis la vue plein écran.
  pub(crate) fn escape_lyrics(&mut self) -> Option<Vec<Effect>> {
    if self.lyrics.sync.take().is_some() || self.lyrics.editor.take().is_some() {
      return Some(Vec::new());
    }
    if self.lyrics.tvk_open && self.lyrics.tvk_panel {
      self.set_tvk_panel(false);
      return Some(Vec::new());
    }
    self
      .lyrics
      .tvk_open
      .then(|| self.update_lyrics(LyricsMessage::CloseTvk))
  }
}

fn save_failed(reason: &str) -> Effect {
  Effect::Toast(format!("Failed to save lyrics: {reason}"), ToastKind::Error)
}

/// Hauteur maximale de la zone des barres de la vue plein écran, en pixels CSS : `clamp(52px, 8.5vh, 84px)` (`app.js:2850`).
pub const TVK_BAR_ZONE: f32 = 84.0;

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::{Message, Playback};
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::ui::{PlayerMessage, UiMessage};
  use crate::user_data::{Track, UserData};
  use crate::window::PersistedWindowState;
  use iced::Size;

  fn track(id: &str) -> Track {
    Track {
      id: Some(id.to_string()),
      title: Some("Titre".to_string()),
      artist: Some("Artiste".to_string()),
      duration: Some(200.0),
      url: Some(format!("https://exemple.org/{id}")),
      platform: Some("ytmusic".to_string()),
      ..Track::default()
    }
  }

  fn app() -> App {
    let data = UserData {
      favorites: vec![track("a"), track("b")],
      autoplay: false,
      ..UserData::default()
    };
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
    .with_data(data);
    app.window.size = Some(Size::new(1280.0, 800.0));
    app.settings.visual.m3_cover = false;
    app
  }

  type LyricRequest<'a> = (&'a ReplyTag, &'static str, &'a [(String, String)]);

  fn lyric_request(effects: &[Effect]) -> Option<LyricRequest<'_>> {
    effects.iter().find_map(|effect| match effect {
      Effect::CoreJson {
        tag:
          tag @ (ReplyTag::Lyrics { .. }
          | ReplyTag::LyricsWords { .. }
          | ReplyTag::LyricsSaved { .. }
          | ReplyTag::LyricsTranslated { .. }),
        path,
        query,
        ..
      } => Some((tag, *path, query.as_slice())),
      _ => None,
    })
  }

  fn query_value<'a>(query: &'a [(String, String)], key: &str) -> Option<&'a str> {
    query
      .iter()
      .find(|(name, _)| name == key)
      .map(|(_, value)| value.as_str())
  }

  fn playing(mut app: App) -> App {
    app.player.current = Some(track("a"));
    app.playback.is_playing = true;
    app
  }

  fn with_lyrics(raw: &str) -> App {
    let mut app = playing(app());
    app.lyrics.lyrics = Some(Lyrics::parse(raw, false));
    app.relayout();
    app
  }

  const SYNCED: &str = "[00:01.00]one\n[00:03.00]two\n[00:05.00]three\n[00:07.00]four";

  /// Une image : `AnimationFrame` règle l'horloge de `App` avant le tick.
  fn tick(app: &mut App, at: Instant) -> Vec<Effect> {
    app.now = at;
    app.animation_tick(at, &[0; FREQ_BINS])
  }

  fn at_position(app: &mut App, seconds: f64) {
    app.playback.position = Duration::from_secs_f64(seconds);
  }

  #[test]
  fn starting_a_track_asks_for_its_lyrics() {
    let mut app = app();
    let effects = app.update(Message::Ui(UiMessage::RowActivated(
      crate::ui::ListId::Favorites,
      0,
    )));
    let (tag, path, query) = lyric_request(&effects).expect("demande de paroles");
    assert_eq!(
      *tag,
      ReplyTag::Lyrics {
        token: app.player.token
      }
    );
    assert_eq!(path, "/api/lyrics");
    assert_eq!(query_value(query, "title"), Some("Titre"));
    assert_eq!(query_value(query, "artist"), Some("Artiste"));
    assert_eq!(query_value(query, "source"), Some("lrclib"));
    assert_eq!(query_value(query, "online"), Some("1"));
    assert_eq!(query_value(query, "duration"), Some("200"));
  }

  #[test]
  fn the_source_and_online_settings_reach_the_request() {
    let mut app = playing(app());
    app.settings.visual.lyrics_source = "kugou".to_string();
    app.settings.visual.online_lyrics = false;
    let effects = app.load_lyrics();
    let (_, _, query) = lyric_request(&effects).unwrap();
    assert_eq!(query_value(query, "source"), Some("kugou"));
    assert_eq!(query_value(query, "online"), Some("0"));
  }

  #[test]
  fn a_reply_for_another_track_is_ignored() {
    let mut app = playing(app());
    app.load_lyrics();
    let stale = ReplyTag::Lyrics {
      token: app.lyrics.token + 1,
    };
    let effects = app.lyrics_reply(
      &stale,
      Ok(json!({ "success": true, "lyrics": "[00:01.00]a" })),
    );
    assert!(effects.is_empty());
    assert_eq!(app.lyrics.lyrics, None);
    assert!(app.lyrics.loading);
  }

  #[test]
  fn a_reply_sets_the_lyrics_then_asks_for_the_words() {
    let mut app = playing(app());
    app.load_lyrics();
    let tag = ReplyTag::Lyrics {
      token: app.lyrics.token,
    };
    let effects = app.lyrics_reply(
      &tag,
      Ok(json!({ "success": true, "lyrics": "[00:01.00]a" })),
    );
    assert!(!app.lyrics.loading);
    assert!(app.lyrics.is_synced());
    assert_eq!(app.lyrics.layout.lines.len(), 1);
    let (_, path, _) = lyric_request(&effects).unwrap();
    assert_eq!(path, "/api/lyrics-words");
  }

  #[test]
  fn the_estimate_source_never_asks_for_the_words() {
    let mut app = playing(app());
    app.settings.visual.word_source = "estimate".to_string();
    app.load_lyrics();
    let tag = ReplyTag::Lyrics {
      token: app.lyrics.token,
    };
    assert!(
      app
        .lyrics_reply(
          &tag,
          Ok(json!({ "success": true, "lyrics": "[00:01.00]a" }))
        )
        .is_empty()
    );
  }

  #[test]
  fn no_lyrics_leaves_the_panel_empty_without_asking_for_words() {
    let mut app = playing(app());
    app.load_lyrics();
    let tag = ReplyTag::Lyrics {
      token: app.lyrics.token,
    };
    assert!(
      app
        .lyrics_reply(&tag, Ok(json!({ "success": true, "lyrics": "" })))
        .is_empty()
    );
    assert_eq!(app.lyrics.lyrics, None);
    assert!(!app.lyrics.loading);
  }

  #[test]
  fn the_words_upgrade_the_loaded_lyrics() {
    let mut app = with_lyrics("[00:01.00]one two");
    let tag = ReplyTag::LyricsWords {
      token: app.lyrics.token,
    };
    app.lyrics_reply(&tag, Ok(json!({ "success": true, "lines": [{ "start": 1.0, "text": "one two", "words": [{ "text": "one", "start": 1.0, "end": 1.5 }, { "text": "two", "start": 1.5, "end": 2.0 }] }] })));
    assert_eq!(app.lyrics.lyrics.as_ref().unwrap().lines[0].words.len(), 2);
  }

  #[test]
  fn a_tick_finds_the_line_and_scrolls_towards_it() {
    let mut app = with_lyrics(SYNCED);
    app.settings.visual.lyrics_speed = 2.5;
    let start = Instant::now();
    at_position(&mut app, 5.0);
    let effects = tick(&mut app, start);
    assert_eq!(app.lyrics.active, Some(2));
    assert!(app.lyrics.animation.is_some());
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::LyricsScroll(_)))
    );
    let target =
      crate::lyrics::layout::target_scroll(app.lyrics.layout.boxes[2], app.lyrics.viewport);
    let effects = tick(&mut app, start + Duration::from_secs(3));
    assert!(app.lyrics.animation.is_none());
    assert_eq!(app.lyrics.offset, target);
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::LyricsScroll(value) if *value == target))
    );
    assert!(
      tick(&mut app, start + Duration::from_secs(3)).is_empty(),
      "rien tant que la ligne ne change pas"
    );
  }

  #[test]
  fn autoscroll_can_be_turned_off() {
    let mut app = with_lyrics(SYNCED);
    app.settings.visual.lyrics_autoscroll = false;
    at_position(&mut app, 5.0);
    assert!(tick(&mut app, Instant::now()).is_empty());
    assert_eq!(app.lyrics.active, Some(2));
  }

  #[test]
  fn the_position_is_smoothed_between_engine_events() {
    let mut app = with_lyrics(SYNCED);
    let start = Instant::now();
    at_position(&mut app, 2.0);
    tick(&mut app, start);
    tick(&mut app, start + Duration::from_millis(100));
    assert!(
      (app.lyrics.position - 2.1).abs() < 1e-6,
      "{}",
      app.lyrics.position
    );
    at_position(&mut app, 2.25);
    tick(&mut app, start + Duration::from_millis(200));
    assert_eq!(app.lyrics.position, 2.25);
    tick(&mut app, start + Duration::from_secs(60));
    assert_eq!(app.lyrics.position, 3.25, "l'écart reste borné");
    app.playback.is_playing = false;
    tick(&mut app, start + Duration::from_secs(61));
    assert_eq!(
      app.lyrics.position, 2.25,
      "à l'arrêt, la position du moteur"
    );
  }

  #[test]
  fn clicking_a_line_seeks_to_its_start() {
    let mut app = with_lyrics(SYNCED);
    let effects = app.update_lyrics(LyricsMessage::LineClicked(1));
    assert!(
      matches!(effects.as_slice(), [Effect::AudioSeek(position)] if *position == Duration::from_secs(3))
    );
    assert!(app.update_lyrics(LyricsMessage::LineClicked(99)).is_empty());
  }

  #[test]
  fn clicking_a_line_without_time_does_nothing() {
    let mut app = with_lyrics("one\ntwo");
    assert!(app.update_lyrics(LyricsMessage::LineClicked(0)).is_empty());
  }

  #[test]
  fn playing_with_lyrics_needs_frames_and_stopping_does_not() {
    let mut app = with_lyrics(SYNCED);
    assert!(app.animation_needed());
    app.playback.is_playing = false;
    assert!(
      !app.animation_needed(),
      "à l'arrêt, sans effet continu : aucun rendu"
    );
  }

  #[test]
  fn lyrics_without_times_need_no_frames() {
    let app = with_lyrics("one\ntwo");
    assert!(!app.layers_need_frames());
  }

  #[test]
  fn a_closed_panel_needs_no_frames() {
    let mut app = with_lyrics(SYNCED);
    app.lyrics.panel_open = false;
    assert!(!app.layers_need_frames());
  }

  #[test]
  fn a_minimized_window_needs_no_frames_and_no_analyser() {
    let mut app = with_lyrics(SYNCED);
    app.settings.visual.bar_wave = true;
    app.settings.visual.mesh_gradient = true;
    app.settings.visual.mesh.speed = 10.0;
    assert!(app.animation_needed() && app.visualizer_visible());
    app.window.position = Some(iced::Point::new(-32000.0, -32000.0));
    assert!(!app.animation_needed());
    assert!(!app.visualizer_visible());
  }

  #[test]
  fn the_mini_window_keeps_its_lyrics_alive_when_the_main_window_is_minimized() {
    let mut app = with_lyrics(SYNCED);
    app.window.position = Some(iced::Point::new(-32000.0, -32000.0));
    app.lyrics.mini_open = true;
    assert!(app.animation_needed());
    assert!(
      !app.main_ticker() && app.mini_ticker(),
      "le minuteur vit dans la fenêtre encore dessinée"
    );
    app.playback.is_playing = false;
    assert!(!app.animation_needed());
  }

  #[test]
  fn the_ticker_lives_in_the_main_window_and_stops_without_animation() {
    let mut app = with_lyrics(SYNCED);
    assert!(app.main_ticker() && !app.mini_ticker());
    app.visuals.unthrottled = true;
    assert!(!app.main_ticker(), "la mesure de capacité suit l'écran");
    app.visuals.unthrottled = false;
    app.playback.is_playing = false;
    assert!(!app.main_ticker() && !app.mini_ticker());
  }

  #[test]
  fn the_startup_frames_run_out_then_nothing_is_rendered() {
    let mut app = app();
    assert!(!app.animation_needed());
    app.visuals.warmup_frames = 3;
    assert!(app.animation_needed() && app.main_ticker());
    let start = Instant::now();
    tick(&mut app, start);
    tick(&mut app, start);
    assert!(app.animation_needed());
    tick(&mut app, start);
    assert!(!app.animation_needed() && !app.main_ticker());
  }

  #[test]
  fn a_minimized_window_skips_the_startup_frames() {
    let mut app = app();
    app.visuals.warmup_frames = 3;
    app.window.position = Some(iced::Point::new(-32000.0, -32000.0));
    assert!(!app.animation_needed());
  }

  #[test]
  fn a_running_scroll_keeps_frames_during_a_pause() {
    let mut app = with_lyrics(SYNCED);
    at_position(&mut app, 5.0);
    tick(&mut app, Instant::now());
    app.playback.is_playing = false;
    assert!(app.lyrics.animation.is_some());
    assert!(app.animation_needed());
  }

  #[test]
  fn the_bars_keep_frames_until_they_rest_after_a_pause() {
    let mut app = playing(app());
    app.settings.visual.bar_wave = true;
    let start = Instant::now();
    app.animation_tick(start, &[255; FREQ_BINS]);
    app.playback.is_playing = false;
    assert!(app.animation_needed());
    let mut frames = 0;
    while app.animation_needed() {
      tick(&mut app, start);
      frames += 1;
      assert!(frames < 200, "les barres ne s'arrêtent pas");
    }
  }

  #[test]
  fn the_shape_keeps_frames_until_its_rotation_settles() {
    let mut app = playing(app());
    app.settings.visual.m3_cover = true;
    assert!(app.animation_needed());
    // Les approches de la vitesse se calculent sur le temps écoulé : des images à 60 Hz.
    let start = Instant::now();
    let mut frames = 0_u32;
    let mut next = |app: &mut App| {
      frames += 1;
      tick(app, start + Duration::from_millis(u64::from(frames) * 16))
    };
    for _ in 0..100 {
      next(&mut app);
    }
    app.playback.is_playing = false;
    assert!(app.animation_needed());
    let mut settling = 0;
    while app.animation_needed() {
      next(&mut app);
      settling += 1;
      assert!(settling < 200);
    }
    assert!(settling > 5);
  }

  #[test]
  fn only_an_animated_mesh_needs_frames_during_a_pause() {
    let mut app = app();
    app.settings.visual.mesh_gradient = true;
    assert!(!app.animation_needed());
    app.settings.visual.mesh.speed = 10.0;
    assert!(app.animation_needed());
    app.settings.visual.mesh_brightness = 0;
    assert!(!app.animation_needed());
  }

  /// Remet les interrupteurs de mouvement à zéro quand un test finit.
  struct MotionGuard;

  impl MotionGuard {
    fn new(reduced: bool, perf: bool) -> Self {
      crate::motion::set_reduced(reduced);
      crate::motion::set_perf(perf);
      Self
    }
  }

  impl Drop for MotionGuard {
    fn drop(&mut self) {
      crate::motion::set_reduced(false);
      crate::motion::set_perf(false);
    }
  }

  #[test]
  fn a_frozen_mesh_asks_for_no_frames_and_stays_on_its_first_key() {
    let mut app = app();
    app.settings.visual.mesh_gradient = true;
    app.settings.visual.mesh.speed = 10.0;
    let start = Instant::now();
    app.sync_mesh(start);
    app.now = start + Duration::from_secs(4);
    let phase = |app: &App| {
      crate::screens::backdrop::mesh_program(app, app.visuals.mesh_started, 0.0, 0.0, 0).phase
    };
    assert!(app.animation_needed());
    assert!(phase(&app) > 0.0);
    for (reduced, perf) in [(true, false), (false, true)] {
      let _guard = MotionGuard::new(reduced, perf);
      assert!(
        !app.animation_needed(),
        "réduit {reduced}, performance {perf}"
      );
      assert_eq!(phase(&app), 0.0, "`meshFloat` : `transform: none`");
      assert!(app.mesh_shown(), "le calque reste affiché");
    }
  }

  #[test]
  fn the_orbits_keep_frames_when_reduced_and_stop_in_performance_mode() {
    let mut app = app();
    app.lyrics.tvk_open = true;
    app.lyrics.tvk_motion = crate::motion::TvkMotion::launched();
    // L'entrée `tvk-in` est finie : seules les orbes tiennent encore le rythme (`body.perf` ne touche pas `tvk-in`).
    app
      .lyrics
      .tvk_motion
      .open(Instant::now() - Duration::from_secs(5));
    assert!(app.animation_needed());
    {
      let _guard = MotionGuard::new(true, false);
      assert!(
        app.animation_needed(),
        "`!important` plus spécifique que `.force-reduced-motion *`"
      );
    }
    let _guard = MotionGuard::new(false, true);
    assert!(!app.animation_needed());
  }

  #[test]
  fn the_performance_mode_stops_the_visualizers_the_wave_and_the_shaped_cover() {
    let mut app = playing(app());
    app.settings.visual.bar_wave = true;
    app.settings.visual.m3_cover = true;
    assert!(app.visualizer_visible() && app.m3_cover_on());
    let start = Instant::now();
    app.animation_tick(start, &[255; FREQ_BINS]);
    let bars = app.visuals.bar.clone();
    let _guard = MotionGuard::new(false, true);
    assert!(!app.visualizer_visible(), "l'analyseur n'est plus lu");
    assert!(!app.m3_cover_on());
    app.animation_tick(start + Duration::from_millis(40), &[255; FREQ_BINS]);
    assert_eq!(
      app.visuals.bar, bars,
      "les barres gardent leur dernier état"
    );
    assert!(
      !app.animation_needed(),
      "ni barres, ni onde, ni pochette façonnée à animer"
    );
  }

  #[test]
  fn the_performance_mode_ticks_every_33_ms_at_most() {
    assert_eq!(PERF_TICK, Duration::from_millis(33));
    assert!(PERF_TICK > MIN_TICK);
  }

  #[test]
  fn the_mesh_layer_restarts_its_float_each_time_it_appears() {
    let mut app = app();
    let at = Instant::now();
    app.sync_mesh(at);
    assert!(
      app.visuals.mesh_started.is_none(),
      "maillage coupé : calque en display none"
    );
    app.settings.visual.mesh_gradient = true;
    app.sync_mesh(at);
    assert_eq!(app.visuals.mesh_started, Some(at));
    let later = at + Duration::from_secs(5);
    app.sync_mesh(later);
    assert_eq!(
      app.visuals.mesh_started,
      Some(at),
      "un calque déjà affiché garde son départ"
    );
    app.settings.visual.cover_bg = true;
    app.sync_mesh(later);
    assert!(
      app.visuals.mesh_started.is_none(),
      "`body.cover-bg-on` masque le calque"
    );
    app.settings.visual.cover_bg = false;
    app.sync_mesh(later);
    assert_eq!(app.visuals.mesh_started, Some(later));
  }

  #[test]
  fn the_mesh_preview_animates_only_while_its_panel_is_open() {
    let mut app = app();
    app.settings.visual.mesh_gradient = false;
    app.settings.visual.mesh.speed = 20.0;
    app.screen = crate::screens::Screen::Settings;
    app.ui.prefs.mesh_open = true;
    assert!(!app.animation_needed(), "maillage coupé : aperçu vide");
    app.settings.visual.mesh_gradient = true;
    assert!(app.animation_needed());
    app.ui.prefs.mesh_open = false;
    app.settings.visual.cover_bg = true;
    assert!(!app.animation_needed());
  }

  #[test]
  fn the_analyser_is_only_needed_by_a_visible_visualizer() {
    let mut app = with_lyrics(SYNCED);
    assert!(!app.visualizer_visible(), "paroles seules");
    app.settings.visual.bar_wave = true;
    assert!(app.visualizer_visible());
    app.settings.visual.bar_wave = false;
    app.lyrics.tvk_open = true;
    assert!(app.visualizer_visible());
    app.settings.visual.tvk_visualizer_lyrics = false;
    assert!(!app.visualizer_visible());
  }

  #[test]
  fn an_active_gap_reads_the_analyser() {
    let mut app = with_lyrics("[00:00.00]♪\n[00:02.00]one");
    at_position(&mut app, 0.5);
    tick(&mut app, Instant::now());
    assert!(app.lyrics.active_gap());
    assert!(app.visualizer_visible());
    app.lyrics.panel_open = false;
    assert!(!app.visualizer_visible());
  }

  #[test]
  fn nothing_is_read_without_a_track() {
    let mut app = app();
    app.settings.visual.bar_wave = true;
    assert!(!app.visualizer_visible());
    assert!(!app.animation_needed());
  }

  #[test]
  fn translation_is_hidden_and_silent_unless_enabled() {
    let mut app = with_lyrics(SYNCED);
    assert!(!app.translation_available());
    assert!(app.update_lyrics(LyricsMessage::ToggleTranslate).is_empty());
    assert!(!app.lyrics.translate_on);
  }

  #[test]
  fn translation_asks_the_core_and_shows_the_lines() {
    let mut app = with_lyrics("[00:01.00]one");
    app.lyrics.translation_enabled = true;
    app.settings.visual.tr_enabled = true;
    app.settings.tr_lang = "fr".to_string();
    let effects = app.update_lyrics(LyricsMessage::ToggleTranslate);
    let Some(Effect::CoreJson {
      path,
      body: Some(body),
      ..
    }) = effects.first()
    else {
      panic!("attendu : demande de traduction");
    };
    assert_eq!(*path, "/api/translate");
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body, json!({ "texts": ["one"], "targetLang": "fr" }));
    let height = app.lyrics.layout.boxes[0].height;
    let tag = ReplyTag::LyricsTranslated {
      token: app.lyrics.token,
    };
    app.lyrics_reply(&tag, Ok(json!({ "success": true, "translations": ["un"] })));
    assert_eq!(
      app.lyrics.translations.as_deref(),
      Some(&["un".to_string()][..])
    );
    assert!(app.lyrics.layout.boxes[0].height > height);
    app.update_lyrics(LyricsMessage::ToggleTranslate);
    assert_eq!(app.lyrics.translations, None);
  }

  #[test]
  fn the_editor_opens_with_the_lines_and_saves_them() {
    let mut app = with_lyrics("[00:00.00]♪\n[00:01.00]one");
    app.update_lyrics(LyricsMessage::OpenEditor);
    assert_eq!(
      app.lyrics.editor.as_ref().unwrap().0.text().trim_end(),
      "[Instrumental]\none"
    );
    let effects = app.update_lyrics(LyricsMessage::EditorSave);
    let Some(Effect::CoreJson {
      path,
      query,
      body: Some(body),
      ..
    }) = effects.first()
    else {
      panic!("attendu : enregistrement");
    };
    assert_eq!(*path, "/api/save-custom-lyrics");
    assert_eq!(query_value(query, "title"), Some("Titre"));
    assert_eq!(
      serde_json::from_str::<Value>(body).unwrap(),
      json!({ "lyrics": "[Instrumental]\none" })
    );
  }

  #[test]
  fn saved_lyrics_close_the_editor_and_reload() {
    let mut app = with_lyrics("[00:01.00]one");
    app.update_lyrics(LyricsMessage::OpenEditor);
    let effects = app.lyrics_reply(
      &ReplyTag::LyricsSaved {
        success_message: "Lyrics saved successfully",
      },
      Ok(json!({ "success": true })),
    );
    assert!(app.lyrics.editor.is_none());
    assert!(
      matches!(&effects[0], Effect::Toast(message, ToastKind::Info) if message == "Lyrics saved successfully")
    );
    assert_eq!(
      lyric_request(&effects).map(|request| request.1),
      Some("/api/lyrics")
    );
  }

  #[test]
  fn a_failed_save_keeps_the_editor_open() {
    let mut app = with_lyrics("[00:01.00]one");
    app.update_lyrics(LyricsMessage::OpenEditor);
    let effects = app.lyrics_reply(
      &ReplyTag::LyricsSaved {
        success_message: "x",
      },
      Ok(json!({ "success": false, "error": "disque plein" })),
    );
    assert!(app.lyrics.editor.is_some());
    assert!(
      matches!(&effects[0], Effect::Toast(message, ToastKind::Error) if message.contains("disque plein"))
    );
  }

  #[test]
  fn sync_needs_lyrics() {
    let mut app = playing(app());
    assert!(matches!(
      app.update_lyrics(LyricsMessage::OpenSync).as_slice(),
      [Effect::Toast(..)]
    ));
    assert!(app.lyrics.sync.is_none());
  }

  #[test]
  fn sync_restarts_the_track_then_marks_lines_and_saves_an_lrc() {
    let mut app = with_lyrics("[00:01.00]one\n[00:02.00]two");
    let effects = app.update_lyrics(LyricsMessage::OpenSync);
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::AudioSeek(position) if position.is_zero()))
    );
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::AudioPlay))
    );
    at_position(&mut app, 1.5);
    app.update_lyrics(LyricsMessage::SyncLine(0));
    at_position(&mut app, 3.25);
    assert!(app.sync_space().is_some());
    let effects = app.update_lyrics(LyricsMessage::SyncSave);
    let Some(Effect::CoreJson {
      path,
      body: Some(body),
      ..
    }) = effects.first()
    else {
      panic!("attendu : enregistrement");
    };
    assert_eq!(*path, "/api/save-custom-lyrics");
    assert_eq!(
      serde_json::from_str::<Value>(body).unwrap(),
      json!({ "lyrics": "[0:01.50]one\n[0:03.25]two" })
    );
  }

  #[test]
  fn space_toggles_playback_outside_of_sync_and_marks_inside() {
    let mut app = with_lyrics("[00:01.00]one");
    assert!(app.sync_space().is_none());
    app.update_lyrics(LyricsMessage::OpenSync);
    let was_playing = app.playback.is_playing;
    app.update(Message::Player(PlayerMessage::TogglePlay));
    assert_eq!(app.playback.is_playing, was_playing, "Espace note la ligne");
    assert_eq!(app.lyrics.sync.as_ref().unwrap().synced(), 1);
  }

  #[test]
  fn escape_closes_sync_then_editor_then_full_screen() {
    let mut app = with_lyrics("[00:01.00]one");
    app.update_lyrics(LyricsMessage::OpenTvk);
    app.update_lyrics(LyricsMessage::OpenEditor);
    app.update_lyrics(LyricsMessage::OpenSync);
    app.update(Message::Escape);
    assert!(app.lyrics.sync.is_none() && app.lyrics.editor.is_some());
    app.update(Message::Escape);
    assert!(app.lyrics.editor.is_none() && app.lyrics.tvk_open);
    app.update(Message::Escape);
    assert!(!app.lyrics.tvk_open);
  }

  #[test]
  fn the_full_screen_view_has_its_own_layout_and_gives_back_the_panel() {
    let mut app = with_lyrics(SYNCED);
    let panel = app.lyrics.layout.boxes[0].height;
    app.update_lyrics(LyricsMessage::OpenTvk);
    assert!(app.lyrics.layout.boxes[0].height > panel);
    assert!(
      app.panel_visible(),
      "Riff garde #right-panel sous .tv-karaoke"
    );
    app.update_lyrics(LyricsMessage::CloseTvk);
    assert_eq!(app.lyrics.layout.boxes[0].height, panel);
    assert!(app.panel_visible());
  }

  #[test]
  fn the_mini_window_is_remembered_and_closes_cleanly() {
    let mut app = with_lyrics(SYNCED);
    let effects = app.update_lyrics(LyricsMessage::ToggleMini);
    assert!(matches!(effects[0], Effect::OpenMini(_)));
    assert!(app.lyrics.mini_open && app.settings.visual.mini_lyrics);
    app.update_lyrics(LyricsMessage::MiniMoved(iced::Point::new(5.0, 6.0)));
    app.update_lyrics(LyricsMessage::MiniResized(Size::new(300.0, 200.0)));
    let window = app.settings.visual.mini_window;
    assert_eq!(
      (window.x, window.y, window.width, window.height),
      (5.0, 6.0, 300.0, 200.0)
    );
    let effects = app.update_lyrics(LyricsMessage::MiniOnTop);
    assert!(matches!(effects[0], Effect::MiniLevel(true)));
    let effects = app.update_lyrics(LyricsMessage::MiniClosed);
    assert!(matches!(effects[0], Effect::CloseMini));
    assert!(!app.lyrics.mini_open && !app.settings.visual.mini_lyrics);
  }

  #[test]
  fn the_mini_window_alone_needs_frames_for_the_lyrics() {
    let mut app = with_lyrics(SYNCED);
    app.lyrics.panel_open = false;
    assert!(!app.layers_need_frames());
    app.lyrics.mini_open = true;
    assert!(app.animation_needed());
  }

  #[test]
  fn the_mode_button_goes_between_word_and_line() {
    let mut app = with_lyrics(SYNCED);
    assert!(app.is_gradient_mode());
    assert!(app.lyrics.layout.lines[0].rows.is_empty());
    app.update_lyrics(LyricsMessage::ToggleMode);
    assert!(!app.is_gradient_mode());
    assert!(!app.lyrics.layout.lines[0].rows.is_empty());
    app.settings.visual.karaoke_words = false;
    app.update_lyrics(LyricsMessage::ToggleMode);
    assert!(
      !app.lyrics.word_mode,
      "sans karaoke_words, le mode reste en lignes"
    );
  }

  #[test]
  fn a_user_scroll_cancels_the_animation() {
    let mut app = with_lyrics(SYNCED);
    at_position(&mut app, 5.0);
    tick(&mut app, Instant::now());
    let viewport = app.lyrics.viewport;
    app.update_lyrics(LyricsMessage::Scrolled {
      offset: app.lyrics.last_set_offset + 1.0,
      viewport,
    });
    assert!(app.lyrics.animation.is_some(), "notre propre défilement");
    app.update_lyrics(LyricsMessage::Scrolled {
      offset: 900.0,
      viewport,
    });
    assert!(app.lyrics.animation.is_none());
  }

  #[test]
  fn the_reported_zone_is_kept_and_does_not_reset_the_active_line() {
    let mut app = with_lyrics(SYNCED);
    at_position(&mut app, 5.0);
    let start = Instant::now();
    tick(&mut app, start);
    app.update_lyrics(LyricsMessage::Scrolled {
      offset: 0.0,
      viewport: 512.0,
    });
    assert_eq!(app.lyrics.viewport, 512.0);
    tick(&mut app, start + Duration::from_millis(16));
    assert_eq!(app.lyrics.active, Some(2));
    app.update_lyrics(LyricsMessage::Scrolled {
      offset: 10.0,
      viewport: 512.0,
    });
    assert_eq!(app.lyrics.active, Some(2), "la même zone ne relance rien");
  }

  #[test]
  fn another_surface_or_window_size_forgets_the_reported_zone() {
    let mut app = with_lyrics(SYNCED);
    app.update_lyrics(LyricsMessage::Scrolled {
      offset: 0.0,
      viewport: 512.0,
    });
    app.update_lyrics(LyricsMessage::OpenTvk);
    assert_ne!(app.lyrics.viewport, 512.0);
    app.update_lyrics(LyricsMessage::Scrolled {
      offset: 0.0,
      viewport: 400.0,
    });
    assert_eq!(app.lyrics.viewport, 400.0);
    app.update(Message::WindowResized(Size::new(1600.0, 1000.0)));
    assert_ne!(app.lyrics.viewport, 400.0);
  }

  #[test]
  fn the_panel_needs_a_current_track() {
    let app = app();
    assert_eq!(app.playback, Playback::default());
    assert!(!app.panel_visible());
  }

  #[test]
  fn the_panel_takes_its_place_without_a_track_like_a_fresh_riff() {
    let mut app = app();
    assert!(app.panel_shown() && !app.panel_visible());
    app.update_lyrics(LyricsMessage::TogglePanel);
    assert!(!app.panel_shown());
    app.update_lyrics(LyricsMessage::TogglePanel);
    assert!(app.panel_shown());
    app.lyrics.tvk_open = true;
    assert!(
      app.panel_shown(),
      "le panneau reste affiché sous le plein écran"
    );
  }

  #[test]
  fn the_panel_controls_follow_the_pointer_like_riff() {
    use crate::theme::tokens::right_panel::{CONTROLS_AFTER_LEAVE, CONTROLS_AFTER_MOVE};
    let mut app = app();
    assert!(!app.ui.panel_controls.visible);
    let effects = app.update_ui(UiMessage::PanelMoved);
    let [Effect::PanelControlsTimer(first, delay)] = effects.as_slice() else {
      panic!("une échéance attendue : {effects:?}");
    };
    assert!(app.ui.panel_controls.visible);
    assert_eq!(*delay, CONTROLS_AFTER_MOVE);
    // Un mouvement de plus ne lance pas d'autre minuteur.
    assert!(app.update_ui(UiMessage::PanelMoved).is_empty());
    // Échéance atteinte pendant un mouvement récent : le panneau attend le reste du délai.
    let effects = app.update_ui(UiMessage::PanelControlsDue(*first));
    assert!(matches!(
      effects.as_slice(),
      [Effect::PanelControlsTimer(generation, rest)] if generation == first && *rest <= CONTROLS_AFTER_MOVE
    ));
    assert!(app.ui.panel_controls.visible);
    // Sortie : 350 ms, et l'ancienne échéance ne compte plus.
    let effects = app.update_ui(UiMessage::PanelLeft);
    let [Effect::PanelControlsTimer(second, delay)] = effects.as_slice() else {
      panic!("une échéance attendue : {effects:?}");
    };
    assert_eq!(*delay, CONTROLS_AFTER_LEAVE);
    assert!(second > first);
    assert!(
      app
        .update_ui(UiMessage::PanelControlsDue(*first))
        .is_empty()
    );
    assert!(app.ui.panel_controls.visible);
    assert!(
      app
        .update_ui(UiMessage::PanelControlsDue(*second))
        .is_empty()
    );
    assert!(!app.ui.panel_controls.visible);
  }

  #[test]
  fn opening_the_panel_shows_its_controls_for_a_moment() {
    let mut app = app();
    app.update_lyrics(LyricsMessage::TogglePanel);
    let effects = app.update_lyrics(LyricsMessage::TogglePanel);
    assert!(app.ui.panel_controls.visible);
    assert!(matches!(
      effects.as_slice(),
      [Effect::PanelControlsTimer(..)]
    ));
  }
}
