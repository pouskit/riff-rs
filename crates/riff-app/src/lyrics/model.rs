//! Modèle de paroles : réponses de `/api/lyrics` et `/api/lyrics-words`, ligne et mot actifs selon la position.
//! Port de `loadSyncedLyrics`, `parseEnhancedLrcWords`, `getLineStartTime`, `updateSyncedLyricsHighlight`,
//! `computeEstimatedWordIndex` et `updateActiveGradient` (`app.js`).

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;

/// Une ligne sans temps a un temps négatif, comme dans Riff.
const NO_TIME: f64 = -1.0;
/// Paroles personnalisées : avance de lecture, et avance de la première ligne (`CUSTOM_SHIFT_LEAD_SECONDS`).
const CUSTOM_LOOK_AHEAD: f64 = 0.25;
const CUSTOM_SHIFT_LEAD: f64 = 4.0;
const DEFAULT_LINE_SECONDS: f64 = 4.0;
const GAP_MIN_SECONDS: f64 = 6.0;

static LRC_LINE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\[(\d+):(\d+(?:\.\d+)?)\](.*)").expect("motif valide"));
static BRACKETS: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\[[^\]]*\]").expect("motif valide"));
static KEPT_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i-u)^(?:\d+:\d+|instrumental|music|intro|outro|solo)\b").expect("motif valide")
});
static GAP_SYMBOLS: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^[\s♪♫\-–—.]*$").expect("motif valide"));
static GAP_WORD: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)^[\(\[]?\s*(?:instrumental|music|intro|outro|solo|interlude|break)\s*[\)\]]?$")
    .expect("motif valide")
});
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("motif valide"));
static PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(([^)]+)\)").expect("motif valide"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").expect("motif valide"));
static WORD_TAG: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"<(\d+):(\d+(?:\.\d+)?)>").expect("motif valide"));

/// Un mot chronométré.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Word {
  #[serde(alias = "word")]
  pub text: String,
  #[serde(alias = "time")]
  pub start: f64,
  #[serde(default = "no_end", alias = "endTime")]
  pub end: f64,
}

const fn no_end() -> f64 {
  f64::NAN
}

/// Une ligne de paroles.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Line {
  pub time: f64,
  pub text: String,
  pub words: Vec<Word>,
  pub is_gap: bool,
}

impl Line {
  fn gap(time: f64) -> Self {
    Self {
      time,
      is_gap: true,
      ..Self::default()
    }
  }
}

/// État d'un mot : classes `past-word`, `active-word` et aucune de Riff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordState {
  Past,
  Active,
  Future,
}

/// Paroles d'un titre.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lyrics {
  pub lines: Vec<Line>,
  pub is_custom: bool,
}

fn remove_headers(text: &str) -> String {
  BRACKETS
    .replace_all(text, |caps: &regex::Captures<'_>| {
      let whole = &caps[0];
      let inner = &whole[1..whole.len() - 1];
      if KEPT_BRACKET.is_match(inner) {
        whole.to_string()
      } else {
        String::new()
      }
    })
    .into_owned()
}

fn is_gap_text(text: &str) -> bool {
  GAP_SYMBOLS.is_match(text) || GAP_WORD.is_match(text)
}

/// `parseEnhancedLrcWords` : un mot par balise `<mm:ss.xx>`, le suivant ou la fin de ligne le termine.
#[must_use]
pub fn parse_enhanced_words(line: &str, line_end: f64) -> Vec<Word> {
  let tags: Vec<(f64, usize, usize)> = WORD_TAG
    .captures_iter(line)
    .filter_map(|caps| {
      let whole = caps.get(0)?;
      let minutes: f64 = caps[1].parse().ok()?;
      let seconds: f64 = caps[2].parse().ok()?;
      Some((minutes * 60.0 + seconds, whole.start(), whole.end()))
    })
    .collect();
  let mut words = Vec::with_capacity(tags.len());
  for (index, &(start, _, text_start)) in tags.iter().enumerate() {
    let next = tags.get(index + 1);
    let text = line[text_start..next.map_or(line.len(), |tag| tag.1)].trim();
    if text.is_empty() {
      continue;
    }
    let end = next.map_or(
      if line_end > start {
        line_end
      } else {
        start + 0.5
      },
      |tag| tag.0,
    );
    words.push(Word {
      text: text.to_string(),
      start,
      end,
    });
  }
  words
}

fn parse_raw_lines(raw: &str) -> Vec<Line> {
  let mut parsed = Vec::new();
  for source in raw.split('\n') {
    if let Some(caps) = LRC_LINE.captures(source) {
      let minutes: f64 = caps[1].parse().unwrap_or(0.0);
      let seconds: f64 = caps[2].parse().unwrap_or(0.0);
      let time = minutes * 60.0 + seconds;
      let without_headers = remove_headers(&caps[3]);
      let text = without_headers.trim();
      if text.is_empty() || is_gap_text(text) {
        parsed.push(Line::gap(time));
        continue;
      }
      parsed.push(Line {
        time,
        text: TAG.replace_all(text, "").trim().to_string(),
        words: parse_enhanced_words(text, time + DEFAULT_LINE_SECONDS),
        is_gap: false,
      });
    } else {
      let without_headers = remove_headers(source);
      let plain = without_headers.trim();
      if plain.is_empty() {
        continue;
      }
      parsed.push(if is_gap_text(plain) {
        Line::gap(NO_TIME)
      } else {
        Line {
          time: NO_TIME,
          text: plain.to_string(),
          ..Line::default()
        }
      });
    }
  }
  parsed
}

/// Fin estimée d'une ligne pour détecter un blanc : fin de son dernier mot, sinon quatre secondes.
fn estimated_end(line: &Line) -> f64 {
  line
    .words
    .last()
    .map(|word| word.end)
    .filter(|end| end.is_finite() && *end != 0.0)
    .unwrap_or(line.time + DEFAULT_LINE_SECONDS)
}

fn insert_gaps(parsed: Vec<Line>) -> Vec<Line> {
  let mut with_gaps = Vec::with_capacity(parsed.len() + 2);
  if parsed
    .first()
    .is_some_and(|line| line.time >= GAP_MIN_SECONDS)
  {
    with_gaps.push(Line::gap(0.0));
  }
  for index in 0..parsed.len() {
    let line = &parsed[index];
    with_gaps.push(line.clone());
    if let Some(next) = parsed.get(index + 1)
      && line.time >= 0.0
      && next.time >= 0.0
      && !line.is_gap
      && !next.is_gap
    {
      let end = estimated_end(line);
      if next.time - end >= GAP_MIN_SECONDS {
        with_gaps.push(Line::gap(end));
      }
    }
  }
  with_gaps
}

impl Lyrics {
  /// Analyse le texte de `/api/lyrics`.
  #[must_use]
  pub fn parse(raw: &str, is_custom: bool) -> Self {
    let parsed = parse_raw_lines(raw);
    let lines = if parsed.iter().any(|line| line.time >= 0.0) {
      insert_gaps(parsed)
    } else {
      parsed
    };
    Self { lines, is_custom }
  }

  /// Analyse la réponse JSON de `/api/lyrics`. `None` : pas de paroles (échec, instrumental ou texte vide).
  #[must_use]
  pub fn from_reply(reply: &Value) -> Option<Self> {
    if reply.get("success").and_then(Value::as_bool) != Some(true) {
      return None;
    }
    let raw = reply
      .get("lyrics")
      .and_then(Value::as_str)
      .filter(|raw| !raw.is_empty())?;
    let is_custom = reply
      .get("isCustom")
      .and_then(Value::as_bool)
      .unwrap_or(false);
    let lyrics = Self::parse(raw, is_custom);
    (!lyrics.lines.is_empty()).then_some(lyrics)
  }

  /// Au moins une ligne a un temps.
  #[must_use]
  pub fn is_synced(&self) -> bool {
    self.lines.iter().any(|line| line.time >= 0.0)
  }

  /// `getLineStartTime` : les lignes personnalisées commencent au temps de la ligne précédente.
  #[must_use]
  pub fn line_start(&self, index: usize) -> f64 {
    let Some(line) = self.lines.get(index) else {
      return NO_TIME;
    };
    if line.time < 0.0 {
      return NO_TIME;
    }
    if !self.is_custom {
      return line.time;
    }
    self.lines[..index]
      .iter()
      .rev()
      .find(|previous| previous.time >= 0.0)
      .map_or((line.time - CUSTOM_SHIFT_LEAD).max(0.0), |previous| {
        previous.time
      })
  }

  /// `getLineEndTime`.
  #[must_use]
  pub fn line_end(&self, index: usize) -> f64 {
    let start = self.line_start(index);
    (index + 1..self.lines.len())
      .map(|next| self.line_start(next))
      .find(|time| *time >= 0.0)
      .unwrap_or(start + DEFAULT_LINE_SECONDS)
  }

  /// Position utilisée pour chercher la ligne.
  #[must_use]
  pub fn match_time(&self, position: f64) -> f64 {
    if self.is_custom {
      position + CUSTOM_LOOK_AHEAD
    } else {
      position
    }
  }

  /// Ligne active. Riff parcourt les lignes chronométrées dans leur ordre et prend la première qui contient la position.
  #[must_use]
  pub fn active_line(&self, position: f64) -> Option<usize> {
    let time = self.match_time(position);
    let timed: Vec<(f64, usize)> = (0..self.lines.len())
      .filter_map(|index| {
        let start = self.line_start(index);
        (start >= 0.0).then_some((start, index))
      })
      .collect();
    timed
      .iter()
      .enumerate()
      .find_map(|(rank, &(start, index))| {
        let next = timed.get(rank + 1).map_or(f64::INFINITY, |item| item.0);
        (time >= start && time < next).then_some(index)
      })
  }

  /// `getEffectiveLineDuration`.
  fn effective_duration(&self, index: usize) -> f64 {
    let raw = (self.line_end(index) - self.line_start(index)).max(0.45);
    let breath = if raw > 1.2 {
      (raw * 0.16).min(0.45)
    } else {
      raw * 0.08
    };
    (raw - breath).max(0.38)
  }

  /// `computeEstimatedWordIndex` : mot actif d'une ligne sans mots chronométrés, ou le nombre de mots en fin de ligne.
  #[must_use]
  pub fn estimated_word(&self, tokens: &[&str], match_time: f64, index: usize) -> usize {
    if tokens.is_empty() {
      return 0;
    }
    let elapsed = match_time - self.line_start(index);
    if elapsed <= 0.0 {
      return 0;
    }
    let duration = self.effective_duration(index);
    if elapsed >= duration {
      return tokens.len();
    }
    let last = tokens.len() - 1;
    let weights: Vec<f64> = tokens
      .iter()
      .enumerate()
      .map(|(rank, token)| {
        let text = token.trim();
        // `.length` compte les unités UTF-16 en JS.
        let mut weight = (text.encode_utf16().count() as f64 * 0.45).max(1.2);
        if text.ends_with([',', ';', ':', '、', '，']) {
          weight += 1.65;
        } else if text.ends_with(['.', '!', '?', '—', '–', '…']) {
          weight += 2.1;
        }
        if rank == last {
          weight += 0.65;
        }
        weight
      })
      .collect();
    let target = elapsed / duration * weights.iter().sum::<f64>();
    let mut sum = 0.0;
    for (rank, weight) in weights.iter().enumerate() {
      sum += weight;
      if target <= sum {
        return rank;
      }
    }
    last
  }

  /// États des mots de la ligne active : mode « words ». Le temps des mots chronométrés n'a pas l'avance des paroles personnalisées.
  #[must_use]
  pub fn word_states(&self, index: usize, position: f64) -> Vec<WordState> {
    let Some(line) = self.lines.get(index) else {
      return Vec::new();
    };
    if !line.words.is_empty() {
      let line_end = self.line_end(index);
      return line
        .words
        .iter()
        .map(|word| {
          let end = if word.end.is_finite() {
            word.end
          } else {
            line_end
          };
          if position >= end {
            WordState::Past
          } else if position >= word.start {
            WordState::Active
          } else {
            WordState::Future
          }
        })
        .collect();
    }
    let tokens: Vec<&str> = line.text.split_whitespace().collect();
    let current = self.estimated_word(&tokens, self.match_time(position), index);
    (0..tokens.len())
      .map(|rank| match rank.cmp(&current) {
        std::cmp::Ordering::Less => WordState::Past,
        std::cmp::Ordering::Equal => WordState::Active,
        std::cmp::Ordering::Greater => WordState::Future,
      })
      .collect()
  }

  /// `updateActiveGradient` : part remplie de la ligne, de 0 à 1, mesurée en caractères.
  #[must_use]
  pub fn gradient_fill(&self, index: usize, position: f64) -> f32 {
    let Some(line) = self.lines.get(index) else {
      return 0.0;
    };
    let duration = self.effective_duration(index);
    let tau = ((self.match_time(position) - self.line_start(index)) / duration).clamp(0.0, 1.0);
    if tau <= 0.0 {
      return 0.0;
    }
    if tau >= 1.0 {
      return 1.0;
    }
    let split = split_parens(&line.text);
    let full = if split.paren.is_empty() {
      split.main
    } else {
      format!("{} {}", split.main, split.paren)
    };
    let chars: Vec<char> = full.chars().collect();
    let count = chars.len();
    if count == 0 {
      return tau as f32;
    }
    let mut cumulative = Vec::with_capacity(count + 1);
    cumulative.push(0.0_f64);
    let mut total = 0.0;
    for (rank, ch) in chars.iter().enumerate() {
      let mut weight = char_weight(*ch);
      if rank + 2 >= count {
        weight *= 1.18;
      }
      total += weight;
      cumulative.push(total);
    }
    let target = tau * total;
    let rank = cumulative[1..]
      .partition_point(|sum| *sum <= target)
      .min(count - 1);
    let width = cumulative[rank + 1] - cumulative[rank];
    let inside = if width > 0.0 {
      (target - cumulative[rank]) / width
    } else {
      0.0
    };
    (((rank as f64 + inside) / count as f64).clamp(0.0, 1.0)) as f32
  }

  /// Fusionne les mots de `/api/lyrics-words`. Port de `fetchAndUpgradeWords` : `true` si les paroles ont changé.
  pub fn merge_words(&mut self, reply: &Value) -> bool {
    let Some(lines) = word_lines(reply) else {
      return false;
    };
    let has_sync = self.is_synced();
    if self.is_custom && !has_sync {
      return false;
    }
    if !has_sync {
      self.lines = lines
        .into_iter()
        .map(|line| Line {
          time: line.start,
          text: line.text,
          words: line.words,
          is_gap: false,
        })
        .collect();
      return true;
    }
    let count_gap =
      self.lines.len().abs_diff(lines.len()) as f64 / self.lines.len().max(lines.len()) as f64;
    if count_gap > 0.15 {
      return false;
    }
    let mut mapped: Vec<Option<usize>> = vec![None; self.lines.len()];
    let mut matched = 0_usize;
    let mut cursor = 0_usize;
    for (index, line) in self.lines.iter().enumerate() {
      let mut best = 0.0;
      let mut best_line = None;
      for (candidate, other) in lines
        .iter()
        .enumerate()
        .take(cursor + 4)
        .skip(cursor.saturating_sub(3))
      {
        let similarity = text_similarity(&line.text, &other.text);
        if similarity > best {
          best = similarity;
          best_line = Some(candidate);
        }
      }
      if let Some(found) = best_line.filter(|_| best >= 0.6) {
        matched += 1;
        cursor = found + 1;
        mapped[index] = Some(found);
      }
    }
    if (matched as f64) / (self.lines.len() as f64) < 0.70 {
      return false;
    }
    for (line, found) in self.lines.iter_mut().zip(mapped) {
      if let Some(found) = found {
        line.words.clone_from(&lines[found].words);
      }
    }
    true
  }
}

#[derive(Debug, Deserialize)]
struct WordLine {
  #[serde(default)]
  start: f64,
  #[serde(default)]
  text: String,
  #[serde(default)]
  words: Vec<Word>,
}

fn word_lines(reply: &Value) -> Option<Vec<WordLine>> {
  if reply.get("success").and_then(Value::as_bool) != Some(true) {
    return None;
  }
  let lines: Vec<WordLine> = serde_json::from_value(reply.get("lines")?.clone()).ok()?;
  (!lines.is_empty()).then_some(lines)
}

/// `stringNormalizedSimilarity`.
fn text_similarity(a: &str, b: &str) -> f64 {
  let normalize = |text: &str| -> Vec<char> {
    text
      .chars()
      .flat_map(char::to_lowercase)
      .filter(|ch| {
        ch.is_ascii_alphanumeric()
          || ('\u{0400}'..='\u{04ff}').contains(ch)
          || ('\u{4e00}'..='\u{9fa5}').contains(ch)
      })
      .collect()
  };
  let (first, second) = (normalize(a), normalize(b));
  if first == second {
    return 1.0;
  }
  let (short, long) = if first.len() <= second.len() {
    (&first, &second)
  } else {
    (&second, &first)
  };
  if long
    .windows(short.len().max(1))
    .any(|window| window == short.as_slice())
    || short.is_empty()
  {
    return short.len() as f64 / long.len() as f64;
  }
  let common = first.iter().zip(&second).filter(|(x, y)| x == y).count();
  common as f64 / long.len() as f64
}

/// `getCharBaseWeight`.
fn char_weight(ch: char) -> f64 {
  match ch {
    ',' | ';' | ':' | '、' | '，' => return 4.6,
    '.' | '!' | '?' | '—' | '–' | '。' => return 5.8,
    _ => {}
  }
  if ch.is_whitespace() || is_punctuation(ch) {
    return 0.55;
  }
  if matches!(ch, '\u{1100}'..='\u{11FF}' | '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}' | '\u{F900}'..='\u{FAFF}')
  {
    return 2.8;
  }
  if is_mark(ch) {
    return 2.6;
  }
  let lower = ch.to_lowercase().next().unwrap_or(ch);
  if "аеёиоуыэюяєіїўѣѵѫѧѩѭ".contains(lower)
    || matches!(lower, 'æ' | 'ø' | 'œ' | 'ı')
    || "aeiouyàáâãäåèéêëìíîïòóôõöùúûüýÿ".contains(lower)
  {
    return 2.5;
  }
  1.0
}

static PUNCTUATION: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^\p{P}$").expect("motif valide"));
static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{M}$").expect("motif valide"));

fn is_punctuation(ch: char) -> bool {
  let mut buffer = [0; 4];
  PUNCTUATION.is_match(ch.encode_utf8(&mut buffer))
}

fn is_mark(ch: char) -> bool {
  let mut buffer = [0; 4];
  MARK.is_match(ch.encode_utf8(&mut buffer))
}

/// `splitLyricParensToEnd` : le texte, les parenthèses reportées à la fin et leur place dans la ligne.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientText {
  pub main: String,
  pub paren: String,
  pub start_ratio: f32,
  pub end_ratio: f32,
}

/// Port de `splitLyricParensToEnd`.
#[must_use]
pub fn split_parens(text: &str) -> GradientText {
  let without_tags = TAG.replace_all(text, "");
  let line = without_tags.trim();
  let utf16 = |part: &str| part.encode_utf16().count();
  let total = utf16(line).max(1) as f32;
  let mut parens = Vec::new();
  let (mut first, mut last) = (None, 0_usize);
  let mut joined = String::with_capacity(line.len());
  let mut cursor = 0;
  for caps in PAREN.captures_iter(line) {
    let whole = caps.get(0).expect("groupe 0");
    joined.push_str(&line[cursor..whole.start()]);
    joined.push(' ');
    cursor = whole.end();
    let inner = caps[1].trim();
    if !inner.is_empty() {
      parens.push(format!("({inner})"));
      first.get_or_insert(utf16(&line[..whole.start()]));
      last = utf16(&line[..whole.end()]);
    }
  }
  joined.push_str(&line[cursor..]);
  let main = SPACES.replace_all(&joined, " ").trim().to_string();
  if main.is_empty() {
    return GradientText {
      main: line.to_string(),
      paren: String::new(),
      start_ratio: 0.7,
      end_ratio: 1.0,
    };
  }
  let start_ratio = first.map_or(0.7, |offset| (offset as f32 / total).clamp(0.05, 0.85));
  let end_ratio = match first {
    Some(offset) if last > offset => (start_ratio + 0.12).max((last as f32 / total).min(1.0)),
    _ => 1.0,
  };
  GradientText {
    main,
    paren: parens.join(" "),
    start_ratio,
    end_ratio,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn parentheses_move_to_the_end_with_their_place_in_the_line() {
    let split = split_parens("one (two) three");
    assert_eq!(split.main, "one three");
    assert_eq!(split.paren, "(two)");
    assert!(
      (split.start_ratio - 4.0 / 15.0).abs() < 1e-5,
      "{}",
      split.start_ratio
    );
    assert!(
      (split.end_ratio - 9.0 / 15.0).abs() < 1e-5,
      "{}",
      split.end_ratio
    );
  }

  #[test]
  fn several_parentheses_are_joined() {
    let split = split_parens("a (b) c (d)");
    assert_eq!(split.main, "a c");
    assert_eq!(split.paren, "(b) (d)");
  }

  #[test]
  fn a_line_only_in_parentheses_stays_as_it_is() {
    let split = split_parens("(tout)");
    assert_eq!(split.main, "(tout)");
    assert_eq!(split.paren, "");
    assert_eq!((split.start_ratio, split.end_ratio), (0.7, 1.0));
  }

  #[test]
  fn a_line_without_parentheses_has_the_default_ratios() {
    let split = split_parens("  plain <b>text</b> ");
    assert_eq!(split.main, "plain text");
    assert_eq!(split.paren, "");
    assert_eq!((split.start_ratio, split.end_ratio), (0.7, 1.0));
  }

  #[test]
  fn the_ratios_stay_in_the_bounds_of_riff() {
    let split = split_parens("(aaaaaaaaaaaaaaaaaaaa) b");
    assert_eq!(split.start_ratio, 0.05, "début borné");
    let split = split_parens("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa (b)");
    assert_eq!(split.start_ratio, 0.85, "début borné");
    assert_eq!(split.end_ratio, 1.0);
  }

  #[test]
  fn empty_parentheses_are_removed_without_a_paren_text() {
    let split = split_parens("a ( ) b");
    assert_eq!(split.main, "a b");
    assert_eq!(split.paren, "");
  }

  fn times(lyrics: &Lyrics) -> Vec<f64> {
    lyrics.lines.iter().map(|line| line.time).collect()
  }

  #[test]
  fn timed_lines_are_read_with_their_text() {
    let lyrics = Lyrics::parse("[00:01.50]one\n[01:02]two three", false);
    assert_eq!(times(&lyrics), vec![1.5, 5.5, 62.0]);
    assert!(lyrics.lines[1].is_gap);
    assert_eq!(lyrics.lines[2].text, "two three");
    assert!(lyrics.is_synced());
  }

  #[test]
  fn plain_lines_have_no_time_and_empty_lines_are_dropped() {
    let lyrics = Lyrics::parse("one\n\n  \ntwo", false);
    assert_eq!(times(&lyrics), vec![NO_TIME, NO_TIME]);
    assert!(!lyrics.is_synced());
  }

  #[test]
  fn metadata_brackets_are_removed_and_section_names_become_gaps() {
    let lyrics = Lyrics::parse(
      "[ar:Someone]\n[00:01.00][Verse 1] hello\n[00:02.00][instrumental]\n[00:03.00]♪\n[00:04.00]Break",
      false,
    );
    assert_eq!(
      lyrics
        .lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>(),
      vec!["hello", "", "", ""]
    );
    assert_eq!(
      lyrics
        .lines
        .iter()
        .map(|line| line.is_gap)
        .collect::<Vec<_>>(),
      vec![false, true, true, true]
    );
  }

  #[test]
  fn a_late_first_line_gets_a_gap_and_a_long_silence_too() {
    let lyrics = Lyrics::parse("[00:06.00]one\n[00:16.00]two\n[00:25.00]three", false);
    assert_eq!(times(&lyrics), vec![0.0, 6.0, 10.0, 16.0, 25.0]);
    assert!(lyrics.lines[0].is_gap && lyrics.lines[2].is_gap);
    assert!(!lyrics.lines[4].is_gap);
  }

  #[test]
  fn a_first_line_before_six_seconds_gets_no_gap() {
    assert_eq!(times(&Lyrics::parse("[00:05.99]one", false)), vec![5.99]);
  }

  #[test]
  fn enhanced_lines_keep_their_word_times_without_the_tags() {
    let lyrics = Lyrics::parse("[00:01.00]<00:01.00>one <00:01.50>two", false);
    let line = &lyrics.lines[0];
    assert_eq!(line.text, "one two");
    assert_eq!(
      line.words,
      vec![
        Word {
          text: "one".into(),
          start: 1.0,
          end: 1.5
        },
        Word {
          text: "two".into(),
          start: 1.5,
          end: 5.0
        },
      ]
    );
  }

  #[test]
  fn a_last_word_after_the_line_end_lasts_half_a_second() {
    assert_eq!(parse_enhanced_words("<00:10.00>one", 5.0)[0].end, 10.5);
  }

  #[test]
  fn the_reply_gives_lyrics_only_when_it_has_text() {
    assert!(
      Lyrics::from_reply(&json!({ "success": true, "lyrics": "[00:01.00]a", "isCustom": true }))
        .is_some_and(|lyrics| lyrics.is_custom)
    );
    assert_eq!(
      Lyrics::from_reply(&json!({ "success": true, "lyrics": "" })),
      None
    );
    assert_eq!(
      Lyrics::from_reply(&json!({ "success": true, "instrumental": true })),
      None
    );
    assert_eq!(
      Lyrics::from_reply(&json!({ "success": false, "lyrics": "a" })),
      None
    );
  }

  fn three_lines() -> Lyrics {
    Lyrics::parse("[00:01.00]one\n[00:02.00]two\n[00:03.00]three", false)
  }

  #[test]
  fn the_active_line_has_inclusive_start_and_exclusive_end() {
    let lyrics = three_lines();
    assert_eq!(lyrics.active_line(0.999), None);
    assert_eq!(lyrics.active_line(1.0), Some(0));
    assert_eq!(lyrics.active_line(1.999), Some(0));
    assert_eq!(lyrics.active_line(2.0), Some(1));
    assert_eq!(lyrics.active_line(5000.0), Some(2));
  }

  #[test]
  fn a_single_line_stays_active_until_the_end() {
    let lyrics = Lyrics::parse("[00:01.00]one", false);
    assert_eq!(lyrics.active_line(1000.0), Some(0));
    assert_eq!(lyrics.line_end(0), 5.0);
  }

  #[test]
  fn lines_without_time_are_never_active() {
    let lyrics = Lyrics::parse("one\ntwo", false);
    assert_eq!(lyrics.active_line(10.0), None);
    assert_eq!(lyrics.line_start(0), NO_TIME);
    assert_eq!(lyrics.line_start(99), NO_TIME);
  }

  #[test]
  fn a_line_without_time_between_two_timed_lines_is_skipped() {
    let lyrics = Lyrics::parse("[00:01.00]one\nplain\n[00:03.00]three", false);
    assert_eq!(lyrics.active_line(2.5), Some(0));
    assert_eq!(lyrics.line_end(0), 3.0);
    assert_eq!(lyrics.active_line(3.0), Some(2));
  }

  #[test]
  fn custom_lines_start_at_the_previous_time_with_a_look_ahead() {
    let lyrics = Lyrics::parse("[00:05.00]one\n[00:09.00]two", true);
    assert_eq!(lyrics.line_start(0), 1.0);
    assert_eq!(lyrics.line_start(1), 5.0);
    assert_eq!(lyrics.active_line(0.74), None);
    assert_eq!(lyrics.active_line(0.75), Some(0));
    assert_eq!(lyrics.active_line(4.75), Some(1));
  }

  #[test]
  fn the_custom_shift_stops_at_zero() {
    assert_eq!(Lyrics::parse("[00:02.00]one", true).line_start(0), 0.0);
  }

  #[test]
  fn timed_words_follow_the_position() {
    let lyrics = Lyrics::parse("[00:02.00]<00:02.00>a <00:03.00>b <00:04.00>c", false);
    use WordState::{Active, Future, Past};
    assert_eq!(lyrics.word_states(0, 1.0), vec![Future, Future, Future]);
    assert_eq!(lyrics.word_states(0, 2.0), vec![Active, Future, Future]);
    assert_eq!(lyrics.word_states(0, 3.5), vec![Past, Active, Future]);
    assert_eq!(lyrics.word_states(0, 20.0), vec![Past, Past, Past]);
  }

  #[test]
  fn a_word_without_end_lasts_until_the_end_of_the_line() {
    let mut lyrics = Lyrics::parse("[00:01.00]a\n[00:03.00]b", false);
    lyrics.lines[0].words = vec![Word {
      text: "a".into(),
      start: 1.0,
      end: f64::NAN,
    }];
    assert_eq!(lyrics.word_states(0, 2.9), vec![WordState::Active]);
    assert_eq!(lyrics.word_states(0, 3.0), vec![WordState::Past]);
  }

  #[test]
  fn estimated_words_are_weighted_by_length_and_punctuation() {
    let lyrics = Lyrics::parse("[00:00.00]a bb cc\n[00:09.00]next", false);
    // Durée 9 - 0,45 = 8,55 ; poids 1,2 / 1,2 / 1,2 + 0,65, soit des fins de mot à 2,41 et 4,83.
    let tokens = ["a", "bb", "cc"];
    assert_eq!(lyrics.estimated_word(&tokens, 0.0, 0), 0);
    assert_eq!(lyrics.estimated_word(&tokens, 2.4, 0), 0);
    assert_eq!(lyrics.estimated_word(&tokens, 2.5, 0), 1);
    assert_eq!(lyrics.estimated_word(&tokens, 4.9, 0), 2);
    assert_eq!(lyrics.estimated_word(&tokens, 8.55, 0), 3);
    assert_eq!(lyrics.estimated_word(&[], 2.0, 0), 0);
  }

  #[test]
  fn estimated_words_end_with_all_words_past() {
    let lyrics = Lyrics::parse("[00:00.00]a b\n[00:09.00]next", false);
    assert_eq!(
      lyrics.word_states(0, 8.9),
      vec![WordState::Past, WordState::Past]
    );
  }

  #[test]
  fn combining_characters_weigh_on_the_gradient_without_being_split() {
    let lyrics = Lyrics::parse("[00:00.00]e\u{0301}a\n[00:10.00]next", false);
    assert_eq!(char_weight('\u{0301}'), 2.6);
    let middle = lyrics.gradient_fill(0, 2.0);
    assert!(middle > 0.0 && middle < 1.0, "{middle}");
  }

  #[test]
  fn the_gradient_fill_is_monotonic_and_bounded() {
    let lyrics = Lyrics::parse("[00:10.00]Hello, world\n[00:14.00]next", false);
    assert_eq!(lyrics.gradient_fill(1, 9.0), 0.0);
    assert_eq!(lyrics.gradient_fill(1, 14.0), 1.0);
    let mut previous = 0.0;
    for step in 0..40 {
      let fill = lyrics.gradient_fill(1, 10.0 + f64::from(step) * 0.1);
      assert!(fill >= previous, "pas {step} : {fill} < {previous}");
      previous = fill;
    }
  }

  #[test]
  fn word_lines_upgrade_matching_lyrics() {
    let mut lyrics = Lyrics::parse("[00:01.00]hello world\n[00:05.00]other line", false);
    let reply = json!({ "success": true, "lines": [
      { "start": 1.0, "text": "hello world", "words": [{ "text": "hello", "start": 1.0, "end": 1.5 }, { "text": "world", "start": 1.5, "end": 2.0 }] },
      { "start": 5.0, "text": "other line", "words": [{ "text": "other", "start": 5.0, "end": 5.5 }] },
    ] });
    assert!(lyrics.merge_words(&reply));
    assert_eq!(lyrics.lines[0].words.len(), 2);
    assert_eq!(lyrics.lines[1].words[0].end, 5.5);
  }

  #[test]
  fn word_lines_are_refused_when_too_few_lines_match() {
    let mut lyrics = Lyrics::parse("[00:01.00]hello world\n[00:05.00]other line", false);
    let reply = json!({ "success": true, "lines": [
      { "start": 1.0, "text": "completely different", "words": [] },
      { "start": 5.0, "text": "nothing alike here", "words": [] },
    ] });
    assert!(!lyrics.merge_words(&reply));
    assert!(lyrics.lines[0].words.is_empty());
  }

  #[test]
  fn word_lines_are_refused_when_the_counts_differ_even_if_most_lines_match() {
    let raw: String = (0..10)
      .map(|rank| format!("[00:{rank:02}.00]line {rank}\n"))
      .collect();
    let mut lyrics = Lyrics::parse(&raw, false);
    let lines: Vec<Value> = (0..8).map(|rank| json!({ "start": rank, "text": format!("line {rank}"), "words": [{ "text": "line", "start": rank, "end": rank }] })).collect();
    assert!(
      !lyrics.merge_words(&json!({ "success": true, "lines": lines })),
      "écart de 20 %, 80 % de lignes reconnues"
    );
    assert!(lyrics.lines[0].words.is_empty());
  }

  #[test]
  fn word_lines_are_refused_when_the_line_counts_differ() {
    let mut lyrics = three_lines();
    let reply = json!({ "success": true, "lines": [{ "start": 1.0, "text": "one", "words": [] }] });
    assert!(!lyrics.merge_words(&reply));
  }

  #[test]
  fn word_lines_replace_plain_lyrics() {
    let mut lyrics = Lyrics::parse("hello", false);
    let reply = json!({ "success": true, "lines": [{ "start": 3.0, "text": "hello", "words": [{ "text": "hello", "start": 3.0, "end": 4.0 }] }] });
    assert!(lyrics.merge_words(&reply));
    assert_eq!(lyrics.lines[0].time, 3.0);
    assert!(lyrics.is_synced());
  }

  #[test]
  fn plain_custom_lyrics_are_never_replaced() {
    let mut lyrics = Lyrics::parse("hello", true);
    let reply =
      json!({ "success": true, "lines": [{ "start": 3.0, "text": "hello", "words": [] }] });
    assert!(!lyrics.merge_words(&reply));
    assert!(!lyrics.is_synced());
  }

  #[test]
  fn a_failed_word_reply_changes_nothing() {
    let mut lyrics = three_lines();
    let before = lyrics.clone();
    assert!(!lyrics.merge_words(&json!({ "success": false, "lines": [] })));
    assert_eq!(lyrics, before);
  }
}
