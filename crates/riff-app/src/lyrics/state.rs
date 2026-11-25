//! État des paroles : titre chargé, surface affichée, défilement, éditeur et synchro manuelle.

use super::layout::{self, LineBox};
use super::model::{Lyrics, WordState};
use super::scroll::ScrollAnimation;
use super::tvk::LineTransition;
use crate::theme::{fonts, tokens};
use crate::user_data::Track;
use crate::visuals::settings::WordStyle;
use iced::Font;
use iced::advanced::graphics::text::Paragraph;
use iced::advanced::text::{self, Paragraph as _};
use iced::font::Weight;
use iced::widget::text_editor;
use iced::{Pixels, Size};
use std::collections::HashMap;
use std::ops::Range;

/// Identifiant du `scrollable` des paroles.
pub const SCROLL_ID: &str = "lyrics-scroll";
/// `.lyric-line` : 18 px par `--font-scale` 1,1875, interligne 1,45, marges 8 et 12.
pub const PANEL_FONT: f32 = 21.375;
/// `.tv-karaoke-content .lyric-line` : `clamp(1.8rem, 3.2vw, 2.8rem)`, en pixels écran (le `vw` est celui de la page CSS).
#[must_use]
pub fn tvk_font(window_width: f32) -> f32 {
  (window_width / tokens::ZOOM * 0.032).clamp(28.8, 44.8) * tokens::ZOOM
}
pub const LINE_HEIGHT: f32 = 1.45;
/// `.tv-karaoke-content .lyric-line` : `line-height: 1.5` (`styles.css:6554-6560`), plus précis que `1.45` de `styles.css:8348`.
pub const TVK_LINE_HEIGHT: f32 = 1.5;
pub const LINE_PADDING_Y: f32 = 8.0;
pub const LINE_PADDING_X: f32 = 12.0;
/// `.lyrics-container` : `gap: 16px`.
pub const LINE_SPACING: f32 = 16.0;
/// Parenthèses du mode gradient : `0.66em`.
pub const GRADIENT_PAREN_SCALE: f32 = 0.66;
pub const WORD_GAP: f32 = 6.0;
pub const PAREN_SCALE: f32 = 0.72;
const GAP_HEIGHT: f32 = 38.0;
const TVK_GAP_HEIGHT: f32 = 62.0;
const TRANSLATION_HEIGHT: f32 = 24.0;
/// `updateLyricsPadding` : marge haute et basse, en part de la zone.
const PADDING_SHARE: f32 = 0.42;

/// Police des paroles : graisse 600 de `.lyric-line` (`styles.css:2546`), qui n'a pas de `font-family` et hérite donc de
/// `--font-family` (`styles.css:76`), Google Sans.
#[must_use]
pub fn lyric_font(bold: bool) -> Font {
  Font {
    weight: if bold { Weight::Bold } else { Weight::Semibold },
    ..Font::with_name(fonts::UI_FAMILY)
  }
}

/// Mise en page d'une ligne.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineLayout {
  /// Rangées de mots : plages d'indices dans les mots de la ligne.
  pub rows: Vec<Range<usize>>,
  pub words: Vec<LineWord>,
  pub rtl: bool,
}

/// Un mot affiché.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineWord {
  pub text: String,
  pub paren: bool,
}

/// Découpe une ligne en mots : texte des mots chronométrés, sinon les mots du texte. Port du marquage `inParen`.
#[must_use]
pub fn line_words(line: &super::model::Line) -> Vec<LineWord> {
  let tokens: Vec<&str> = if line.words.is_empty() {
    line.text.split_whitespace().collect()
  } else {
    line.words.iter().map(|word| word.text.as_str()).collect()
  };
  let mut in_paren = false;
  tokens
    .into_iter()
    .map(|token| {
      if token.starts_with(['(', '[']) {
        in_paren = true;
      }
      let word = LineWord {
        text: token.to_string(),
        paren: in_paren,
      };
      if token.ends_with([')', ']']) {
        in_paren = false;
      }
      word
    })
    .collect()
}

/// Mesure du texte, remplacée dans les tests par une largeur par caractère.
#[expect(
  clippy::disallowed_methods,
  reason = "hors de `view` : seul appelant `LyricsLayout::compute` (state.rs:151), appelé par `relayout` (lyrics/update.rs:294) sur message, avec cache de largeur par mot dans `compute_with`"
)]
fn measure_text(content: &str, size: f32, face: Font, width: f32) -> Size {
  let paragraph = Paragraph::with_text(text::Text {
    content,
    bounds: Size::new(width, f32::INFINITY),
    size: Pixels(size),
    line_height: text::LineHeight::Relative(LINE_HEIGHT),
    font: face,
    align_x: text::Alignment::Default,
    align_y: iced::alignment::Vertical::Top,
    shaping: text::Shaping::Advanced,
    wrapping: text::Wrapping::Word,
  });
  paragraph.min_bounds()
}

/// Paramètres d'une mise en page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutParams {
  pub width: f32,
  pub viewport: f32,
  pub font: f32,
  pub face: Font,
  /// Espace entre deux lignes : `gap` de `.lyrics-container`, ou `--tvk-gap` en plein écran.
  pub spacing: f32,
  pub style: WordStyle,
  pub word_mode: bool,
  pub tvk: bool,
  /// Pixels écran par pixel CSS des marges fixes : le zoom de Riff dans le panneau, 1 en plein écran.
  pub scale: f32,
}

/// Résultat d'une mise en page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LyricsLayout {
  pub params: Option<LayoutParams>,
  pub lines: Vec<LineLayout>,
  pub boxes: Vec<LineBox>,
  pub content_height: f32,
}

impl LyricsLayout {
  /// Calcule la mise en page. Une largeur par texte déjà mesuré évite de remesurer le même mot.
  #[must_use]
  pub fn compute(lyrics: &Lyrics, params: LayoutParams, translations: Option<&[String]>) -> Self {
    Self::compute_with(
      lyrics,
      params,
      translations,
      &mut |content, size, face, width| measure_text(content, size, face, width),
    )
  }

  fn compute_with(
    lyrics: &Lyrics,
    params: LayoutParams,
    translations: Option<&[String]>,
    measure: &mut dyn FnMut(&str, f32, Font, f32) -> Size,
  ) -> Self {
    let mut widths: HashMap<(String, bool), f32> = HashMap::new();
    let row_height = params.font
      * if params.tvk {
        TVK_LINE_HEIGHT
      } else {
        LINE_HEIGHT
      };
    let mut lines = Vec::with_capacity(lyrics.lines.len());
    let mut heights = Vec::with_capacity(lyrics.lines.len());
    let gradient = params.word_mode && params.style == WordStyle::Gradient;
    for (index, line) in lyrics.lines.iter().enumerate() {
      if line.is_gap {
        lines.push(LineLayout::default());
        heights.push(if params.tvk {
          TVK_GAP_HEIGHT * params.scale
        } else {
          GAP_HEIGHT * params.scale
        });
        continue;
      }
      let rtl = super::rtl::is_rtl_text(&line.text);
      let words = line_words(line);
      let (rows, body) = if gradient {
        let split = super::model::split_parens(&line.text);
        let height = if split.paren.is_empty() {
          measure(&split.main, params.font, params.face, params.width).height
        } else {
          // Parenthèses plus petites, à la suite du texte : `margin-left: 0.28em`.
          let main = measure(&split.main, params.font, params.face, f32::INFINITY).width;
          let paren = measure(
            &split.paren,
            params.font * GRADIENT_PAREN_SCALE,
            params.face,
            f32::INFINITY,
          )
          .width;
          ((main + params.font * 0.28 + paren) / params.width)
            .ceil()
            .max(1.0)
            * row_height
        };
        (Vec::new(), height.max(row_height))
      } else {
        let word_widths: Vec<f32> = words
          .iter()
          .map(|word| {
            *widths
              .entry((word.text.clone(), word.paren))
              .or_insert_with(|| {
                let size = if word.paren {
                  params.font * PAREN_SCALE
                } else {
                  params.font
                };
                measure(&word.text, size, params.face, f32::INFINITY).width
              })
          })
          .collect();
        let rows = layout::wrap_words(&word_widths, params.width, WORD_GAP * params.scale);
        let height = rows.len().max(1) as f32 * row_height;
        (rows, height)
      };
      let translation = translations
        .and_then(|items| items.get(index))
        .filter(|text| !text.is_empty())
        .map_or(0.0, |_| TRANSLATION_HEIGHT * params.scale);
      heights.push(body + LINE_PADDING_Y * 2.0 * params.scale + translation);
      lines.push(LineLayout { rows, words, rtl });
    }
    let padding = (params.viewport * PADDING_SHARE).round();
    let boxes = layout::stack_lines(&heights, padding, params.spacing);
    let content_height = boxes
      .last()
      .map_or(padding * 2.0, |line| line.top + line.height + padding);
    Self {
      params: Some(params),
      lines,
      boxes,
      content_height,
    }
  }
}

/// Ligne en cours de synchro manuelle (`syncState.lines`).
#[derive(Debug, Clone, PartialEq)]
pub struct SyncLine {
  pub text: String,
  pub is_gap: bool,
  pub time: Option<f64>,
}

/// `#sync-mode-overlay`.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncState {
  pub track: Track,
  pub lines: Vec<SyncLine>,
  pub current: usize,
}

impl SyncState {
  /// `handleSyncLineClick` : note le temps, puis cherche la prochaine ligne sans temps.
  pub fn mark(&mut self, index: usize, time: f64) {
    let Some(line) = self.lines.get_mut(index) else {
      return;
    };
    line.time = Some(time);
    self.current = self
      .lines
      .iter()
      .enumerate()
      .skip(index + 1)
      .find(|(_, line)| line.time.is_none())
      .map_or(self.lines.len(), |(next, _)| next);
  }

  /// `restartSync`.
  pub fn restart(&mut self) {
    self.lines.iter_mut().for_each(|line| line.time = None);
    self.current = 0;
  }

  /// Nombre de lignes chronométrées.
  #[must_use]
  pub fn synced(&self) -> usize {
    self.lines.iter().filter(|line| line.time.is_some()).count()
  }

  /// `saveSyncAndExit` : une ligne `[m:ss.xx]texte`, ou le texte seul.
  #[must_use]
  pub fn to_lrc(&self) -> String {
    self
      .lines
      .iter()
      .map(|line| match line.time {
        Some(time) => {
          let minutes = (time / 60.0).floor();
          format!(
            "[{}:{:05.2}]{}",
            minutes as u64,
            time - minutes * 60.0,
            line.text
          )
        }
        None => line.text.clone(),
      })
      .collect::<Vec<_>>()
      .join("\n")
  }
}

/// Contenu de l'éditeur : `text_editor::Content` n'est pas clonable.
#[derive(Debug)]
pub struct EditorContent(pub text_editor::Content);

impl Clone for EditorContent {
  fn clone(&self) -> Self {
    Self(text_editor::Content::with_text(&self.0.text()))
  }
}

/// Paroles et surfaces qui les montrent.
#[derive(Debug, Clone)]
pub struct LyricsState {
  pub lyrics: Option<Lyrics>,
  pub loading: bool,
  /// Le titre courant quand la demande est partie : une réponse tardive est ignorée.
  pub token: u64,
  pub panel_open: bool,
  /// `state.lyricsMode === 'word'`.
  pub word_mode: bool,
  pub tvk_open: bool,
  /// Panneau de personnalisation de la vue plein écran (`#tvk-panel`).
  pub tvk_panel: bool,
  /// `.tv-karaoke.tv-idle` : 3 s sans mouvement de souris, boutons de la vue masqués (`app.js:6540-6552`).
  pub tvk_idle: bool,
  /// Dernier mouvement de souris dans la vue plein écran.
  pub tvk_active: Option<std::time::Instant>,
  /// Entrée, sortie et horloge des orbes de la vue plein écran.
  pub tvk_motion: crate::motion::TvkMotion,
  /// Transitions d'ouverture et de fermeture de `#tvk-panel`.
  pub panel_motion: crate::motion::TvkPanelMotion,
  pub mini_open: bool,
  pub layout: LyricsLayout,
  pub active: Option<usize>,
  pub offset: f32,
  pub viewport: f32,
  /// Hauteur annoncée par le `scrollable` de la surface affichée. Sans elle, une estimation.
  pub reported_viewport: Option<f32>,
  pub animation: Option<ScrollAnimation>,
  /// Changement de ligne active en cours dans la vue plein écran.
  pub transition: Option<LineTransition>,
  pub last_set_offset: f32,
  /// Position lissée entre deux événements du moteur.
  pub position: f64,
  pub clock_base: f64,
  pub clock_at: Option<std::time::Instant>,
  pub translation_enabled: bool,
  pub translate_on: bool,
  pub translations: Option<Vec<String>>,
  pub editor: Option<EditorContent>,
  pub sync: Option<SyncState>,
}

impl Default for LyricsState {
  fn default() -> Self {
    Self {
      lyrics: None,
      loading: false,
      token: 0,
      panel_open: true,
      word_mode: true,
      tvk_open: false,
      tvk_panel: false,
      tvk_idle: false,
      tvk_active: None,
      tvk_motion: crate::motion::TvkMotion::default(),
      panel_motion: crate::motion::TvkPanelMotion::default(),
      mini_open: false,
      layout: LyricsLayout::default(),
      active: None,
      offset: 0.0,
      viewport: 300.0,
      reported_viewport: None,
      animation: None,
      transition: None,
      last_set_offset: 0.0,
      position: 0.0,
      clock_base: 0.0,
      clock_at: None,
      translation_enabled: false,
      translate_on: false,
      translations: None,
      editor: None,
      sync: None,
    }
  }
}

impl LyricsState {
  /// Les paroles ont des temps.
  #[must_use]
  pub fn is_synced(&self) -> bool {
    self.lyrics.as_ref().is_some_and(Lyrics::is_synced)
  }

  /// La ligne active est un blanc : ses barres suivent l'analyseur.
  #[must_use]
  pub fn active_gap(&self) -> bool {
    match (self.lyrics.as_ref(), self.active) {
      (Some(lyrics), Some(active)) => lyrics.lines.get(active).is_some_and(|line| line.is_gap),
      _ => false,
    }
  }

  /// États des mots d'une ligne, pour le mode `words`. Les autres lignes n'ont ni mot actif, ni couleur propre.
  #[must_use]
  pub fn word_states(&self, index: usize) -> Vec<WordState> {
    match self.lyrics.as_ref() {
      Some(lyrics) if self.active == Some(index) => lyrics.word_states(index, self.position),
      _ => Vec::new(),
    }
  }
}

/// Points de mesure du dégradé par caractère.
const SAMPLES: usize = 16;

/// `riff_lyrics_gradient_tightness` : portion de la ligne couverte par le front du dégradé, et position de chaque caractère.
#[must_use]
pub fn gradient_runs(text: &str, fill: f32, tightness: u32) -> Vec<(String, f32)> {
  let chars: Vec<char> = text.chars().collect();
  let count = chars.len();
  if count == 0 {
    return Vec::new();
  }
  let front = tightness as f32 / 100.0;
  // `calc(k * (100% + fw) - fw)` et `calc(k * (100% + fw))`.
  let start = fill * (1.0 + front) - front;
  let end = fill * (1.0 + front);
  // Part remplie d'un caractère : moyenne du dégradé sur sa largeur, le texte n'étant colorable que caractère par caractère.
  let amount = |rank: usize| -> f32 {
    let at = |x: f32| {
      if x <= start {
        1.0
      } else if x >= end {
        0.0
      } else {
        (end - x) / (end - start)
      }
    };
    let total: f32 = (0..SAMPLES)
      .map(|sample| at((rank as f32 + (sample as f32 + 0.5) / SAMPLES as f32) / count as f32))
      .sum();
    (total / SAMPLES as f32 * SAMPLES as f32).round() / SAMPLES as f32
  };
  let mut runs: Vec<(String, f32)> = Vec::new();
  for (rank, ch) in chars.iter().enumerate() {
    let value = amount(rank);
    match runs.last_mut() {
      Some((run, last)) if (*last - value).abs() < f32::EPSILON => run.push(*ch),
      _ => runs.push((ch.to_string(), value)),
    }
  }
  runs
}

#[cfg(test)]
mod tests {
  use super::*;

  fn params(style: WordStyle, word_mode: bool) -> LayoutParams {
    LayoutParams {
      width: 100.0,
      viewport: 200.0,
      font: 10.0,
      face: lyric_font(false),
      spacing: LINE_SPACING,
      style,
      word_mode,
      tvk: false,
      scale: 1.0,
    }
  }

  /// Un caractère aussi large que la police, retour à la ligne par largeur.
  fn fake_measure(content: &str, size: f32, _face: Font, width: f32) -> Size {
    let chars = content.chars().count() as f32 * size;
    if width.is_finite() {
      let rows = (chars / width).ceil().max(1.0);
      Size::new(chars.min(width), rows * 14.0)
    } else {
      Size::new(chars, 14.0)
    }
  }

  fn layout(raw: &str, style: WordStyle, word_mode: bool) -> LyricsLayout {
    LyricsLayout::compute_with(
      &Lyrics::parse(raw, false),
      params(style, word_mode),
      None,
      &mut fake_measure,
    )
  }

  #[test]
  fn words_wrap_in_rows_and_the_line_grows() {
    let result = layout("[00:01.00]aaaa bbbb cccc", WordStyle::Words, true);
    assert_eq!(result.lines[0].rows, vec![0..2, 2..3]);
    assert_eq!(result.boxes[0].height, 2.0 * 14.5 + 16.0);
    assert_eq!(result.boxes[0].top, 84.0);
  }

  #[test]
  fn the_gradient_mode_measures_the_whole_line() {
    let result = layout("[00:01.00]aaaa bbbb cccc", WordStyle::Gradient, true);
    assert!(result.lines[0].rows.is_empty());
    assert_eq!(result.boxes[0].height, 2.0 * 14.0 + 16.0);
  }

  #[test]
  fn the_line_mode_uses_words_even_with_the_gradient_style() {
    let result = layout("[00:01.00]aaaa", WordStyle::Gradient, false);
    assert_eq!(result.lines[0].rows, vec![0..1]);
  }

  #[test]
  fn rtl_lines_are_marked_and_gaps_have_their_own_height() {
    let result = layout(
      "[00:00.00]مرحبا\n[00:01.00]hello\n[00:02.00]♪",
      WordStyle::Words,
      true,
    );
    assert!(result.lines[0].rtl && !result.lines[1].rtl);
    assert_eq!(result.boxes[2].height, GAP_HEIGHT);
  }

  #[test]
  fn a_translation_adds_its_height() {
    let lyrics = Lyrics::parse("[00:01.00]aaaa", false);
    let plain = LyricsLayout::compute_with(
      &lyrics,
      params(WordStyle::Words, true),
      None,
      &mut fake_measure,
    );
    let translated = LyricsLayout::compute_with(
      &lyrics,
      params(WordStyle::Words, true),
      Some(&["bonjour".to_string()]),
      &mut fake_measure,
    );
    assert_eq!(
      translated.boxes[0].height,
      plain.boxes[0].height + TRANSLATION_HEIGHT
    );
  }

  #[test]
  fn the_content_has_a_padding_on_both_ends() {
    let result = layout("[00:01.00]aaaa", WordStyle::Words, true);
    assert_eq!(result.content_height, 84.0 + result.boxes[0].height + 84.0);
  }

  #[test]
  fn the_full_screen_font_follows_the_width_of_the_window() {
    assert!((tvk_font(500.0) - 28.8 * tokens::ZOOM).abs() < 1e-3);
    assert!((tvk_font(1280.0) - 40.96).abs() < 1e-3);
    assert!((tvk_font(3000.0) - 44.8 * tokens::ZOOM).abs() < 1e-3);
  }

  #[test]
  fn small_parentheses_do_not_add_a_row_when_they_fit() {
    // « aaaa » 40 px, marge 2,8 px, « (bb) » 26,4 px en petit : 69,2 px dans une zone de 100.
    let result = layout("[00:01.00]aaaa (bb)", WordStyle::Gradient, true);
    assert_eq!(result.boxes[0].height, 14.5 + 16.0);
    // 70 + 2,8 + 26,4 px dépassent 80 px : deux rangées.
    let lyrics = Lyrics::parse("[00:01.00]aaaaaaa (bb)", false);
    let narrow = LayoutParams {
      width: 80.0,
      ..params(WordStyle::Gradient, true)
    };
    let wrapped = LyricsLayout::compute_with(&lyrics, narrow, None, &mut fake_measure);
    assert_eq!(wrapped.boxes[0].height, 2.0 * 14.5 + 16.0);
  }

  #[test]
  fn a_real_measure_gives_a_positive_width() {
    assert!(measure_text("hello", 20.0, lyric_font(false), f32::INFINITY).width > 0.0);
  }

  #[test]
  fn parentheses_are_marked_until_they_close() {
    let lyrics = Lyrics::parse("[00:01.00]one (two three) four", false);
    let parens: Vec<bool> = line_words(&lyrics.lines[0])
      .iter()
      .map(|word| word.paren)
      .collect();
    assert_eq!(parens, vec![false, true, true, false]);
  }

  fn sync_state() -> SyncState {
    let line = |text: &str| SyncLine {
      text: text.into(),
      is_gap: false,
      time: None,
    };
    SyncState {
      track: Track::default(),
      lines: vec![line("a"), line("b"), line("c")],
      current: 0,
    }
  }

  #[test]
  fn marking_a_line_moves_to_the_next_without_time() {
    let mut sync = sync_state();
    sync.mark(0, 1.0);
    assert_eq!(sync.current, 1);
    sync.lines[2].time = Some(9.0);
    sync.mark(1, 2.0);
    assert_eq!(sync.current, 3, "plus aucune ligne après");
    assert_eq!(sync.synced(), 3);
    sync.mark(99, 2.0);
    assert_eq!(sync.synced(), 3);
  }

  #[test]
  fn restarting_clears_every_time() {
    let mut sync = sync_state();
    sync.mark(0, 1.0);
    sync.restart();
    assert_eq!((sync.synced(), sync.current), (0, 0));
  }

  #[test]
  fn the_lrc_is_written_like_riff() {
    let mut sync = sync_state();
    sync.mark(0, 5.5);
    sync.mark(1, 65.123);
    assert_eq!(sync.to_lrc(), "[0:05.50]a\n[1:05.12]b\nc");
  }

  #[test]
  fn the_gradient_fills_from_the_start_of_the_text() {
    assert_eq!(gradient_runs("", 0.5, 2), Vec::new());
    assert_eq!(
      gradient_runs("abcd", 0.0, 2),
      vec![("abcd".to_string(), 0.0)]
    );
    assert_eq!(
      gradient_runs("abcd", 1.0, 2),
      vec![("abcd".to_string(), 1.0)]
    );
    let middle = gradient_runs("abcd", 0.5, 2);
    assert_eq!(
      middle.first().map(|run| (run.0.as_str(), run.1)),
      Some(("ab", 1.0))
    );
    assert_eq!(middle.last().map(|run| run.1), Some(0.0));
  }

  #[test]
  fn a_wider_front_softens_more_characters() {
    let soft = |runs: Vec<(String, f32)>| {
      runs
        .iter()
        .filter(|run| run.1 > 0.0 && run.1 < 1.0)
        .map(|run| run.0.chars().count())
        .sum::<usize>()
    };
    let text = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    assert!(soft(gradient_runs(text, 0.5, 40)) > soft(gradient_runs(text, 0.5, 2)));
  }

  #[test]
  fn the_filled_part_grows_with_the_fill() {
    let filled = |fill: f32| {
      gradient_runs("abcdefghij", fill, 2)
        .iter()
        .map(|run| run.0.chars().count() as f32 * run.1)
        .sum::<f32>()
    };
    let mut previous = -1.0;
    for step in 0..=20 {
      let value = filled(step as f32 / 20.0);
      assert!(value >= previous, "{value} {previous}");
      previous = value;
    }
    assert!((filled(1.0) - 10.0).abs() < 1e-3);
  }
}
