//! Réponse générique du core et paramètres d'appel, sans dépendance à HTTP.

use crate::error::CoreError;
use bytes::Bytes;
use futures_util::stream::{self, BoxStream, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

/// Résultat de toutes les opérations qui répondent par un [`Reply`].
pub type CoreResult = Result<Reply, CoreError>;

/// En-têtes d'une requête, en paires nom/valeur.
#[derive(Debug, Clone, Default)]
pub struct HeaderLite(Vec<(String, String)>);

impl HeaderLite {
  #[must_use]
  pub fn new(headers: Vec<(String, String)>) -> Self {
    Self(headers)
  }

  /// Première valeur du nom donné, sans tenir compte de la casse.
  #[must_use]
  pub fn get(&self, name: &str) -> Option<&str> {
    self
      .0
      .iter()
      .find(|(k, _)| k.eq_ignore_ascii_case(name))
      .map(|(_, v)| v.as_str())
  }
}

/// Paramètres d'une opération.
#[derive(Debug, Clone, Default)]
pub struct Params {
  pub query: HashMap<String, String>,
  pub body: Bytes,
  pub headers: HeaderLite,
}

/// Méthode d'appel, pour les quelques opérations dont le comportement en dépend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
  Get,
  Head,
  Post,
  Other,
}

/// Réponse d'une opération du core.
pub enum Reply {
  Json {
    status: u16,
    body: Value,
  },
  Text {
    status: u16,
    text: String,
  },
  Bytes {
    status: u16,
    content_type: String,
    body: Bytes,
  },
  /// Fichier à servir, en entier ou sur une plage d'octets (bornes incluses).
  File {
    path: PathBuf,
    mime: String,
    range: Option<(u64, u64)>,
  },
  /// Corps en flux, ou réponse qui porte des en-têtes particuliers.
  Stream {
    status: u16,
    headers: Vec<(String, String)>,
    body: BoxStream<'static, Result<Bytes, CoreError>>,
  },
}

impl Reply {
  #[must_use]
  pub fn status(&self) -> u16 {
    match self {
      Self::Json { status, .. }
      | Self::Text { status, .. }
      | Self::Bytes { status, .. }
      | Self::Stream { status, .. } => *status,
      Self::File { range: Some(_), .. } => 206,
      Self::File { range: None, .. } => 200,
    }
  }

  /// Corps déjà en mémoire avec des en-têtes particuliers : la longueur est déclarée.
  #[must_use]
  pub fn buffered(status: u16, mut headers: Vec<(String, String)>, body: Bytes) -> Self {
    headers.push(("content-length".to_string(), body.len().to_string()));
    Self::Stream {
      status,
      headers,
      body: stream::once(async move { Ok(body) }).boxed(),
    }
  }

  /// Réponse sans corps avec des en-têtes particuliers.
  #[must_use]
  pub fn empty_with_headers(status: u16, mut headers: Vec<(String, String)>) -> Self {
    headers.push(("content-length".to_string(), "0".to_string()));
    Self::Stream {
      status,
      headers,
      body: stream::empty().boxed(),
    }
  }
}

impl fmt::Debug for Reply {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Json { status, body } => f
        .debug_struct("Json")
        .field("status", status)
        .field("body", body)
        .finish(),
      Self::Text { status, text } => f
        .debug_struct("Text")
        .field("status", status)
        .field("text", text)
        .finish(),
      Self::Bytes {
        status,
        content_type,
        body,
      } => f
        .debug_struct("Bytes")
        .field("status", status)
        .field("content_type", content_type)
        .field("len", &body.len())
        .finish(),
      Self::File { path, mime, range } => f
        .debug_struct("File")
        .field("path", path)
        .field("mime", mime)
        .field("range", range)
        .finish(),
      Self::Stream {
        status, headers, ..
      } => f
        .debug_struct("Stream")
        .field("status", status)
        .field("headers", headers)
        .finish_non_exhaustive(),
    }
  }
}

/// Réponse JSON.
pub fn json_response(status: u16, body: &Value) -> CoreResult {
  Ok(Reply::Json {
    status,
    body: body.clone(),
  })
}

/// Réponse texte, sans type de contenu.
pub fn text_response(status: u16, text: &str) -> CoreResult {
  Ok(Reply::Text {
    status,
    text: text.to_string(),
  })
}

pub fn parse_byte_range(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
  if file_size == 0 {
    return None;
  }
  let trimmed = range_header.trim();
  let rest = trimmed.strip_prefix("bytes=")?;
  let (start_raw, end_raw) = rest.split_once('-')?;
  if start_raw.is_empty() && end_raw.is_empty() {
    return None;
  }
  if !start_raw.chars().all(|c| c.is_ascii_digit()) || !end_raw.chars().all(|c| c.is_ascii_digit())
  {
    return None;
  }

  let (start, mut end) = if start_raw.is_empty() {
    let suffix: u64 = end_raw.parse().ok()?;
    if suffix == 0 {
      return None;
    }
    let s = file_size.saturating_sub(suffix);
    (s, file_size - 1)
  } else {
    let s: u64 = start_raw.parse().ok()?;
    let e: u64 = if end_raw.is_empty() {
      file_size - 1
    } else {
      end_raw.parse().ok()?
    };
    (s, e)
  };

  if start > end || start >= file_size {
    return None;
  }
  if end >= file_size {
    end = file_size - 1;
  }
  Some((start, end))
}

/// Sert un fichier en tenant compte de l'en-tête `Range`.
pub fn serve_file_with_range(
  file_path: &Path,
  headers: &HeaderLite,
  content_type: &str,
  not_found_msg: &str,
  read_err_status: u16,
  read_err_msg: &str,
) -> CoreResult {
  let Ok(meta) = fs::metadata(file_path) else {
    return text_response(404, not_found_msg);
  };
  if !meta.is_file() {
    return text_response(404, not_found_msg);
  }
  let range = match headers.get("range") {
    Some(range_val) => {
      let Some(bounds) = parse_byte_range(range_val, meta.len()) else {
        return Ok(Reply::empty_with_headers(
          416,
          vec![(
            "content-range".to_string(),
            format!("bytes */{}", meta.len()),
          )],
        ));
      };
      Some(bounds)
    }
    None => None,
  };
  if File::open(file_path).is_err() {
    return text_response(read_err_status, read_err_msg);
  }
  Ok(Reply::File {
    path: file_path.to_path_buf(),
    mime: content_type.to_string(),
    range,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_temp::TempDir;
  use serde_json::json;

  #[test]
  fn header_lookup_ignores_case() {
    let headers = HeaderLite::new(vec![("Range".to_string(), "bytes=0-1".to_string())]);
    assert_eq!(headers.get("range"), Some("bytes=0-1"));
    assert_eq!(headers.get("RANGE"), Some("bytes=0-1"));
    assert_eq!(headers.get("origin"), None);
  }

  #[test]
  fn json_response_keeps_status_and_body() {
    let reply = json_response(201, &json!({ "a": 1 })).unwrap();
    assert!(matches!(reply, Reply::Json { status: 201, ref body } if body == &json!({ "a": 1 })));
  }

  #[test]
  fn text_response_keeps_status_and_text() {
    let reply = text_response(404, "Not found").unwrap();
    assert!(matches!(reply, Reply::Text { status: 404, ref text } if text == "Not found"));
  }

  #[test]
  fn status_reads_every_variant() {
    assert_eq!(text_response(418, "").unwrap().status(), 418);
    assert_eq!(json_response(200, &json!(null)).unwrap().status(), 200);
    let file = Reply::File {
      path: "a".into(),
      mime: "audio/mpeg".into(),
      range: Some((0, 1)),
    };
    assert_eq!(file.status(), 206);
    let file = Reply::File {
      path: "a".into(),
      mime: "audio/mpeg".into(),
      range: None,
    };
    assert_eq!(file.status(), 200);
    let bytes = Reply::Bytes {
      status: 200,
      content_type: "image/png".into(),
      body: Bytes::new(),
    };
    assert_eq!(bytes.status(), 200);
  }

  #[test]
  fn byte_range_handles_open_ended_and_suffix_forms() {
    assert_eq!(parse_byte_range("bytes=0-15", 100), Some((0, 15)));
    assert_eq!(parse_byte_range("bytes=90-", 100), Some((90, 99)));
    assert_eq!(parse_byte_range("bytes=-10", 100), Some((90, 99)));
    assert_eq!(parse_byte_range("bytes=50-999", 100), Some((50, 99)));
  }

  #[test]
  fn byte_range_rejects_invalid_forms() {
    assert_eq!(parse_byte_range("bytes=100-", 100), None);
    assert_eq!(parse_byte_range("bytes=5-1", 100), None);
    assert_eq!(parse_byte_range("bytes=-0", 100), None);
    assert_eq!(parse_byte_range("bytes=-", 100), None);
    assert_eq!(parse_byte_range("octets=0-1", 100), None);
    assert_eq!(parse_byte_range("bytes=a-b", 100), None);
    assert_eq!(parse_byte_range("bytes=0-1", 0), None);
  }

  fn temp_file(name: &str, content: &[u8]) -> (TempDir, PathBuf) {
    let dir = TempDir::new("reply");
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    (dir, path)
  }

  fn range_headers(value: &str) -> HeaderLite {
    HeaderLite::new(vec![("range".to_string(), value.to_string())])
  }

  #[test]
  fn serve_file_without_range_replies_with_the_whole_file() {
    let (_dir, path) = temp_file("whole", b"0123456789");
    let reply = serve_file_with_range(
      &path,
      &HeaderLite::default(),
      "audio/mpeg",
      "absent",
      500,
      "erreur",
    )
    .unwrap();
    assert!(matches!(reply, Reply::File { range: None, ref mime, .. } if mime == "audio/mpeg"));
  }

  #[test]
  fn serve_file_with_range_replies_with_the_bounds() {
    let (_dir, path) = temp_file("bounds", b"0123456789");
    let reply = serve_file_with_range(
      &path,
      &range_headers("bytes=2-4"),
      "audio/mpeg",
      "absent",
      500,
      "erreur",
    )
    .unwrap();
    assert!(matches!(
      reply,
      Reply::File {
        range: Some((2, 4)),
        ..
      }
    ));
  }

  #[test]
  fn serve_file_out_of_range_replies_416_with_the_size() {
    let (_dir, path) = temp_file("out", b"0123456789");
    let reply = serve_file_with_range(
      &path,
      &range_headers("bytes=99-"),
      "audio/mpeg",
      "absent",
      500,
      "erreur",
    )
    .unwrap();
    let Reply::Stream {
      status, headers, ..
    } = reply
    else {
      panic!("attendu : Reply::Stream");
    };
    assert_eq!(status, 416);
    assert!(headers.contains(&("content-range".to_string(), "bytes */10".to_string())));
  }

  #[test]
  fn serve_missing_file_replies_404_with_the_message() {
    let dir = TempDir::new("reply-absent");
    let path = dir.join("fichier-absent");
    let reply = serve_file_with_range(
      &path,
      &HeaderLite::default(),
      "audio/mpeg",
      "absent",
      500,
      "erreur",
    )
    .unwrap();
    assert!(matches!(reply, Reply::Text { status: 404, ref text } if text == "absent"));
  }

  #[tokio::test]
  async fn buffered_declares_its_length_and_yields_its_bytes() {
    let reply = Reply::buffered(
      200,
      vec![("cache-control".into(), "no-store".into())],
      Bytes::from_static(b"abc"),
    );
    let Reply::Stream {
      status,
      headers,
      mut body,
    } = reply
    else {
      panic!("attendu : Reply::Stream");
    };
    assert_eq!(status, 200);
    assert!(headers.contains(&("cache-control".to_string(), "no-store".to_string())));
    assert!(headers.contains(&("content-length".to_string(), "3".to_string())));
    assert_eq!(
      body.next().await.unwrap().unwrap(),
      Bytes::from_static(b"abc")
    );
    assert!(body.next().await.is_none());
  }

  #[tokio::test]
  async fn empty_with_headers_has_no_body_and_zero_length() {
    let reply = Reply::empty_with_headers(302, vec![("location".into(), "https://x".into())]);
    let Reply::Stream {
      status,
      headers,
      mut body,
    } = reply
    else {
      panic!("attendu : Reply::Stream");
    };
    assert_eq!(status, 302);
    assert!(headers.contains(&("content-length".to_string(), "0".to_string())));
    assert!(body.next().await.is_none());
  }
}
