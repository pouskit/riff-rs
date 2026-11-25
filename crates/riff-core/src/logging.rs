//! Journal de l'application. Tout message est masqué avant d'être écrit.

use crate::paths::{log_dir, log_path};
use regex::Regex;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::OnceLock;

const TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

pub const LOG_MAX_BYTES: u64 = 1024 * 1024;

/// Remplace la valeur des en-têtes et paramètres sensibles par `***`, dans une URL, un corps JSON ou un formulaire.
#[must_use]
pub fn redact(message: &str) -> String {
  static HEADERS: OnceLock<Regex> = OnceLock::new();
  static FIELDS: OnceLock<Regex> = OnceLock::new();
  // La valeur d'un en-tête va jusqu'à la fin de la ligne ou de la chaîne JSON : `Bearer x`, `SID=a; HSID=b`.
  let headers = HEADERS.get_or_init(|| {
    Regex::new(r#"(?i)\b(authorization|cookie)(["']?\s*(?:[:=]|%3D)\s*["']?)[^"\r\n]+"#)
      .expect("regex")
  });
  // Tout nom qui finit par token, secret ou key d'API, plus `code`, `sk`, `password`, `api_sig`, `deeplKey`.
  // `%3F`, `%26`, `%3D` : `?`, `&`, `=` encodés, quand une URL est elle-même passée en paramètre.
  let fields = FIELDS.get_or_init(|| {
    Regex::new(r#"(?i)(\b|%3F|%26)([\w-]*(?:token|secret|api_?key)|deepl_?key|code|sk|password|api_sig)(["']?\s*(?:[:=]|%3D)\s*["']?)[^&\s"',;})]+"#)
      .expect("regex")
  });
  let masked = headers.replace_all(message, "$1$2***");
  fields.replace_all(&masked, "$1$2$3***").into_owned()
}

/// Réduit toute URL `http(s)://` du texte à `schéma://hôte/…` : ni identifiant, ni chemin, ni paramètres.
#[must_use]
pub fn redact_urls(message: &str) -> String {
  static URLS: OnceLock<Regex> = OnceLock::new();
  let urls = URLS.get_or_init(|| {
    Regex::new(r#"(?i)\b(https?)://([^/?#\s"'<>)\]]*)[^\s"'<>)\]]*"#).expect("regex")
  });
  urls
    .replace_all(message, |caps: &regex::Captures| {
      // Ce qui précède `@` est un identifiant : jamais écrit.
      let authority = &caps[2];
      let host = authority.rsplit('@').next().unwrap_or(authority);
      format!("{}://{host}/…", caps[1].to_ascii_lowercase())
    })
    .into_owned()
}

pub fn log_entry(level: &str, message: &str) {
  let dir = log_dir();
  if fs::create_dir_all(&dir).is_err() {
    return;
  }
  let path = log_path();
  if let Ok(meta) = fs::metadata(&path)
    && meta.len() > LOG_MAX_BYTES
  {
    let rotated = dir.join("riff.log.1");
    let _ = fs::rename(&path, rotated);
  }
  let ts = jiff::Timestamp::now().strftime(TS_FORMAT);
  let clean = redact(message);
  let clipped: String = clean.chars().take(800).collect();
  if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
    let _ = writeln!(file, "{ts} [{level}] {clipped}");
  }
}

pub fn tail_log(lines: usize) -> Vec<String> {
  let Ok(content) = fs::read_to_string(log_path()) else {
    return Vec::new();
  };
  let all: Vec<String> = content
    .lines()
    .filter(|l| !l.is_empty())
    .map(String::from)
    .collect();
  let start = all.len().saturating_sub(lines);
  all[start..].to_vec()
}

#[cfg(test)]
mod tests {
  use super::*;

  const SECRET: &str = "SENTINELLE123";

  #[test]
  fn timestamp_is_utc_iso_with_milliseconds() {
    let ts = jiff::Timestamp::new(1_795_000_000, 789_999_999).unwrap();
    assert_eq!(
      ts.strftime(TS_FORMAT).to_string(),
      "2026-11-18T11:06:40.789Z"
    );
  }

  #[test]
  fn every_sensitive_field_is_masked_in_every_form() {
    let keys = [
      "access_token",
      "refresh_token",
      "api_key",
      "client_secret",
      "code",
      "token",
      "apiKey",
      "deeplKey",
      "apiSecret",
      "refreshToken",
      "sk",
      "password",
      "api_sig",
      "secret_token",
      "id_token",
    ];
    for key in keys {
      for message in [
        format!("https://h.example/p?a=1&{key}={SECRET}&b=2"),
        format!("https://h.example/p?{key}={SECRET}"),
        format!("{key}={SECRET}"),
        format!("{{\"{key}\":\"{SECRET}\",\"x\":1}}"),
        format!("{{\"{key}\": \"{SECRET}\"}}"),
        format!("{key}: {SECRET}"),
        format!("url=https%3A%2F%2Fh.example%2F%3F{key}%3D{SECRET}"),
      ] {
        let out = redact(&message);
        assert!(!out.contains(SECRET), "fuite : {message} -> {out}");
      }
    }
  }

  #[test]
  fn authorization_and_cookie_values_are_masked() {
    for message in [
      format!("Authorization: Bearer {SECRET}"),
      format!("authorization: DeepL-Auth-Key {SECRET}:fx"),
      format!("AUTHORIZATION={SECRET}"),
      format!("{{\"Authorization\":\"Bearer {SECRET}\"}}"),
      format!("Authorization: SAPISIDHASH 123_{SECRET}"),
      format!("Cookie: SID={SECRET}; HSID={SECRET}"),
    ] {
      let out = redact(&message);
      assert!(!out.contains(SECRET), "fuite : {message} -> {out}");
      assert!(out.contains("***"), "{out}");
    }
  }

  #[test]
  fn ordinary_messages_are_unchanged() {
    for message in [
      "server.rs listening on 127.0.0.1:38472",
      "yt-dlp exited with code 1",
      "https://api.deezer.com/search?q=daft&limit=1",
      "Tool download failed: ffmpeg: HTTP 404",
      "error_code=5",
      "task: done",
    ] {
      assert_eq!(redact(message), message);
    }
  }

  #[test]
  fn urls_are_reduced_to_scheme_and_host() {
    assert_eq!(
      redact_urls("GET https://rr3---sn-x.googlevideo.com/videoplayback?ip=1.2.3.4&sig=abc a"),
      "GET https://rr3---sn-x.googlevideo.com/… a"
    );
    assert_eq!(
      redact_urls("(http://user:pw@h.example:8080/p#f) et HTTPS://H.example"),
      "(http://h.example:8080/…) et https://H.example/…"
    );
    assert_eq!(
      redact_urls("réponse 403, sans URL"),
      "réponse 403, sans URL"
    );
  }

  #[test]
  fn fields_around_a_secret_are_kept() {
    assert_eq!(
      redact("https://h.example/p?a=1&token=abc&b=2"),
      "https://h.example/p?a=1&token=***&b=2"
    );
  }
}
