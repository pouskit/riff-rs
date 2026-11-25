//! Erreurs du core.

/// Erreur d'une opération du core.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
  #[error("requête invalide : {0}")]
  BadRequest(String),
  #[error("introuvable")]
  NotFound,
  #[error("indisponible : {0}")]
  Unavailable(String),
  #[error("erreur du service {host} (statut {status:?})")]
  Upstream { host: String, status: Option<u16> },
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error(transparent)]
  Json(#[from] serde_json::Error),
  #[error("opération annulée")]
  Cancelled,
}

impl CoreError {
  /// Code HTTP de l'adaptateur, choisi parmi ceux que renvoient déjà les routes.
  #[must_use]
  pub fn status(&self) -> u16 {
    match self {
      Self::BadRequest(_) | Self::Json(_) => 400,
      Self::NotFound => 404,
      Self::Unavailable(_) | Self::Io(_) => 500,
      Self::Upstream { .. } => 502,
      Self::Cancelled => 504,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn status_matches_the_codes_of_the_http_routes() {
    assert_eq!(CoreError::BadRequest("x".into()).status(), 400);
    assert_eq!(CoreError::NotFound.status(), 404);
    assert_eq!(CoreError::Unavailable("x".into()).status(), 500);
    assert_eq!(
      CoreError::Upstream {
        host: "h".into(),
        status: Some(500)
      }
      .status(),
      502
    );
    assert_eq!(
      CoreError::Upstream {
        host: "h".into(),
        status: None
      }
      .status(),
      502
    );
    assert_eq!(CoreError::Cancelled.status(), 504);
  }

  #[test]
  fn io_and_json_errors_convert_with_their_status() {
    let io: CoreError = std::io::Error::other("disque").into();
    assert_eq!(io.status(), 500);
    let json: CoreError = serde_json::from_str::<serde_json::Value>("{")
      .unwrap_err()
      .into();
    assert_eq!(json.status(), 400);
  }

  #[test]
  fn upstream_message_names_the_host() {
    let err = CoreError::Upstream {
      host: "api.deezer.com".into(),
      status: Some(503),
    };
    assert!(err.to_string().contains("api.deezer.com"));
  }
}
