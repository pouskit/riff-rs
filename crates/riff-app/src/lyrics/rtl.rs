//! Détection du sens d'écriture : port de `lyrics-rtl.js`.

use regex::Regex;
use std::sync::LazyLock;

static MARKUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").expect("motif valide"));
static LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{L}").expect("motif valide"));
// `-u` : `\b` et l'insensibilité à la casse restent en ASCII, comme une regex JS sans drapeau `u`.
static RTL_LANGUAGE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i-u)^(ar|fa|per|fas|prs|pes|he|iw|ur|ps|ckb|sd|ug|yi)\b").expect("motif valide")
});

/// `RTL_CHAR_RE`.
#[must_use]
pub const fn is_rtl_char(ch: char) -> bool {
  matches!(ch, '\u{0590}'..='\u{05FF}' | '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// `isRtlText` : la première lettre décide, le balisage est ignoré.
#[must_use]
pub fn is_rtl_text(text: &str) -> bool {
  let clean = MARKUP.replace_all(text, "");
  match LETTER
    .find(&clean)
    .and_then(|letter| letter.as_str().chars().next())
  {
    Some(first) => is_rtl_char(first),
    None => clean.chars().any(is_rtl_char),
  }
}

/// `isRtlLanguageCode`.
#[must_use]
pub fn is_rtl_language_code(lang: &str) -> bool {
  RTL_LANGUAGE.is_match(lang.trim())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn detects_arabic_lyrics_as_right_to_left() {
    assert!(is_rtl_text("أحبك يا حياتي"));
  }

  #[test]
  fn detects_hebrew_lyrics_as_right_to_left() {
    assert!(is_rtl_text("אני אוהב אותך"));
  }

  #[test]
  fn treats_english_lyrics_as_left_to_right() {
    assert!(!is_rtl_text("I love you tonight"));
  }

  #[test]
  fn ignores_leading_numbers_and_punctuation_before_the_first_strong_character() {
    assert!(is_rtl_text("123 - أحبك"));
    assert!(!is_rtl_text("123 - I love you"));
  }

  #[test]
  fn uses_the_first_strong_character_for_mixed_lines() {
    assert!(!is_rtl_text("Hello مرحبا"));
    assert!(is_rtl_text("مرحبا Hello"));
  }

  #[test]
  fn ignores_markup_and_blank_lyrics() {
    assert!(is_rtl_text("<span>مرحبا</span>"));
    assert!(!is_rtl_text(""));
    assert!(!is_rtl_text("   "));
  }

  #[test]
  fn a_line_without_letters_falls_back_on_any_rtl_character() {
    assert!(is_rtl_text("١٢٣"));
    assert!(!is_rtl_text("123"));
  }

  #[test]
  fn rtl_languages_match_on_a_word_boundary() {
    for lang in ["ar", "AR", " he ", "ar-SA", "ckb", "yi", "per"] {
      assert!(is_rtl_language_code(lang), "{lang}");
    }
    for lang in ["", "en", "arabic", "ar_SA", "fr", "de-AR"] {
      assert!(!is_rtl_language_code(lang), "{lang}");
    }
  }
}
