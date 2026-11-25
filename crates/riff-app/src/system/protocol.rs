//! Protocole du canal local : une requête JSON par ligne, 64 Ko au plus, lue avec un délai.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

/// Taille maximale d'une ligne, saut de ligne compris dans la limite de lecture mais pas dans la ligne.
pub const MAX_LINE: usize = 64 * 1024;
/// Délai accordé à un client pour envoyer sa ligne entière.
pub const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Nombre maximal de fichiers dans une requête `show`.
pub const MAX_PATHS: usize = 256;
const MAX_PATH_LEN: usize = 4096;

/// Requête d'un client.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
  /// Seconde instance : montrer la fenêtre, avec des fichiers audio à ouvrir.
  Show {
    #[serde(default)]
    paths: Vec<String>,
  },
  /// `riff lyrics` : recevoir l'état des paroles à chaque changement.
  Lyrics,
}

/// Réponse à une requête `show`, ou refus d'une requête invalide.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Reply {
  pub ok: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub error: Option<String>,
}

impl Reply {
  #[must_use]
  pub const fn ok() -> Self {
    Self {
      ok: true,
      error: None,
    }
  }

  #[must_use]
  pub fn refused(error: &ProtocolError) -> Self {
    Self {
      ok: false,
      error: Some(error.to_string()),
    }
  }
}

/// État diffusé aux clients `riff lyrics`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct LyricsState {
  pub title: String,
  pub artist: String,
  /// Ligne de paroles courante, vide entre deux lignes.
  pub line: String,
  pub playing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
  #[error("line over {} bytes", MAX_LINE)]
  TooLarge,
  #[error("no complete line before the deadline")]
  Timeout,
  #[error("connection closed before a complete line")]
  Closed,
  #[error("invalid request: {0}")]
  Invalid(String),
  #[error("io error: {0}")]
  Io(std::io::ErrorKind),
}

impl Request {
  /// Refuse les requêtes dont les chemins sont démesurés ou contiennent un octet nul.
  pub fn validate(self) -> Result<Self, ProtocolError> {
    if let Self::Show { paths } = &self {
      if paths.len() > MAX_PATHS {
        return Err(ProtocolError::Invalid(format!(
          "more than {MAX_PATHS} paths"
        )));
      }
      if paths
        .iter()
        .any(|path| path.len() > MAX_PATH_LEN || path.contains('\0'))
      {
        return Err(ProtocolError::Invalid(
          "path too long or containing NUL".into(),
        ));
      }
    }
    Ok(self)
  }
}

/// Lit une ligne JSON bornée. Le délai couvre la ligne entière : un client qui goutte à goutte est coupé aussi.
pub async fn read_request<R: AsyncRead + Unpin>(
  reader: &mut R,
  deadline: Duration,
) -> Result<Request, ProtocolError> {
  let line = tokio::time::timeout(deadline, read_line(reader))
    .await
    .map_err(|_| ProtocolError::Timeout)??;
  let request: Request =
    serde_json::from_slice(&line).map_err(|error| ProtocolError::Invalid(error.to_string()))?;
  request.validate()
}

async fn read_line<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, ProtocolError> {
  let mut line = Vec::new();
  let mut limited = BufReader::new(reader.take(MAX_LINE as u64 + 1));
  limited
    .read_until(b'\n', &mut line)
    .await
    .map_err(|error| ProtocolError::Io(error.kind()))?;
  if line.last() == Some(&b'\n') {
    line.pop();
    return if line.len() > MAX_LINE {
      Err(ProtocolError::TooLarge)
    } else {
      Ok(line)
    };
  }
  if line.len() > MAX_LINE {
    Err(ProtocolError::TooLarge)
  } else {
    Err(ProtocolError::Closed)
  }
}

/// Sérialise une valeur sur une ligne terminée par un saut de ligne.
#[must_use]
pub fn encode_line<T: Serialize>(value: &T) -> Vec<u8> {
  let mut bytes = serde_json::to_vec(value).unwrap_or_default();
  bytes.push(b'\n');
  bytes
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::io::AsyncWriteExt;

  const SHORT: Duration = Duration::from_millis(80);

  async fn read(bytes: &[u8]) -> Result<Request, ProtocolError> {
    let mut reader = bytes;
    read_request(&mut reader, READ_TIMEOUT).await
  }

  #[tokio::test]
  async fn parses_show_with_paths() {
    let request = read(b"{\"type\":\"show\",\"paths\":[\"C:\\\\a.mp3\",\"b.flac\"]}\n").await;
    assert_eq!(
      request,
      Ok(Request::Show {
        paths: vec!["C:\\a.mp3".into(), "b.flac".into()]
      })
    );
  }

  #[tokio::test]
  async fn show_without_paths_is_empty() {
    assert_eq!(
      read(b"{\"type\":\"show\"}\n").await,
      Ok(Request::Show { paths: vec![] })
    );
  }

  #[tokio::test]
  async fn parses_lyrics() {
    assert_eq!(read(b"{\"type\":\"lyrics\"}\n").await, Ok(Request::Lyrics));
  }

  #[tokio::test]
  async fn rejects_a_line_that_is_not_json() {
    assert!(matches!(
      read(b"hello\n").await,
      Err(ProtocolError::Invalid(_))
    ));
  }

  #[tokio::test]
  async fn rejects_an_unknown_type() {
    assert!(matches!(
      read(b"{\"type\":\"format\"}\n").await,
      Err(ProtocolError::Invalid(_))
    ));
  }

  #[tokio::test]
  async fn rejects_invalid_utf8() {
    assert!(matches!(
      read(b"{\"type\":\"show\",\"paths\":[\"\xff\xfe\"]}\n").await,
      Err(ProtocolError::Invalid(_))
    ));
  }

  #[tokio::test]
  async fn rejects_a_line_without_newline_over_the_limit() {
    let bytes = vec![b'a'; MAX_LINE + 10];
    assert_eq!(read(&bytes).await, Err(ProtocolError::TooLarge));
  }

  #[tokio::test]
  async fn rejects_a_line_with_newline_over_the_limit() {
    let mut bytes = vec![b'a'; MAX_LINE + 1];
    bytes.push(b'\n');
    assert_eq!(read(&bytes).await, Err(ProtocolError::TooLarge));
  }

  #[tokio::test]
  async fn accepts_a_line_of_exactly_the_limit() {
    let head = b"{\"type\":\"lyrics\",\"pad\":\"";
    let tail = b"\"}";
    let mut bytes = head.to_vec();
    bytes.resize(MAX_LINE - tail.len(), b'a');
    bytes.extend_from_slice(tail);
    assert_eq!(bytes.len(), MAX_LINE);
    bytes.push(b'\n');
    assert_eq!(read(&bytes).await, Ok(Request::Lyrics));
  }

  #[tokio::test]
  async fn closed_connection_before_any_byte_is_closed() {
    assert_eq!(read(b"").await, Err(ProtocolError::Closed));
  }

  #[tokio::test]
  async fn line_cut_before_its_newline_is_closed() {
    assert_eq!(
      read(b"{\"type\":\"lyrics\"}").await,
      Err(ProtocolError::Closed)
    );
  }

  #[tokio::test]
  async fn silent_client_times_out() {
    let (mut server, _client) = tokio::io::duplex(64);
    assert_eq!(
      read_request(&mut server, SHORT).await,
      Err(ProtocolError::Timeout)
    );
  }

  #[tokio::test]
  async fn slow_client_times_out_even_if_it_sends_bytes() {
    let (mut server, mut client) = tokio::io::duplex(64);
    client.write_all(b"{\"type\":").await.unwrap();
    assert_eq!(
      read_request(&mut server, SHORT).await,
      Err(ProtocolError::Timeout)
    );
  }

  #[tokio::test]
  async fn rejects_too_many_paths() {
    let paths: Vec<String> = (0..=MAX_PATHS).map(|i| format!("{i}.mp3")).collect();
    let line = encode_line(&Request::Show { paths });
    assert!(matches!(read(&line).await, Err(ProtocolError::Invalid(_))));
  }

  #[tokio::test]
  async fn rejects_a_path_with_nul() {
    let line = encode_line(&Request::Show {
      paths: vec!["a\0b".into()],
    });
    assert!(matches!(read(&line).await, Err(ProtocolError::Invalid(_))));
  }

  #[tokio::test]
  async fn rejects_a_path_over_the_length_limit() {
    let line = encode_line(&Request::Show {
      paths: vec!["a".repeat(MAX_PATH_LEN + 1)],
    });
    assert!(matches!(read(&line).await, Err(ProtocolError::Invalid(_))));
  }

  #[test]
  fn encoded_lines_hold_one_newline_at_the_end() {
    let line = encode_line(&LyricsState {
      title: "a\nb".into(),
      ..LyricsState::default()
    });
    assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
    assert_eq!(line.last(), Some(&b'\n'));
  }

  #[test]
  fn refusal_carries_the_reason() {
    let reply = Reply::refused(&ProtocolError::TooLarge);
    assert!(!reply.ok);
    assert!(reply.error.is_some_and(|error| error.contains("65536")));
  }
}
