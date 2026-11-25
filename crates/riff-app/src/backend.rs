//! Tuyau vers le core et le moteur audio, sans serveur HTTP.

use bytes::Bytes;
use iced::futures::StreamExt;
use riff_audio::{AudioError, Engine, EngineConfig, Event};
use riff_core::core::{Core, CoreConfig};
use riff_core::dispatch;
use riff_core::error::CoreError;
use riff_core::reply::{Method, Params, Reply};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use url::Url;

fn read_file_range(path: &Path, range: Option<(u64, u64)>) -> io::Result<Vec<u8>> {
  let Some((start, end)) = range else {
    return fs::read(path);
  };
  let mut file = File::open(path)?;
  file.seek(SeekFrom::Start(start))?;
  let mut bytes = Vec::new();
  file.take(end - start + 1).read_to_end(&mut bytes)?;
  Ok(bytes)
}

/// Préfixe des URL locales : résolues par [`Backend::resolve_url`], jamais par le réseau.
pub const LOCAL_BASE_URL: &str = "riff://local";
const LOCAL_SCHEME: &str = "riff";
const LOCAL_HOST: &str = "local";

/// Réécrit vers [`LOCAL_BASE_URL`] une URL du serveur local de Riff, qui reste dans les données importées.
#[must_use]
pub fn local_url(url: &str) -> String {
  for host in ["http://127.0.0.1", "http://localhost"] {
    if let Some(rest) = url
      .strip_prefix(host)
      .and_then(|rest| rest.strip_prefix(':'))
      && let Some(path) = rest.find('/').map(|slash| &rest[slash..])
    {
      return format!("{LOCAL_BASE_URL}{path}");
    }
  }
  url.to_string()
}

/// Erreurs du tuyau vers le core.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
  #[error("URL locale invalide : {0}")]
  BadUrl(String),
  #[error("réponse {0}")]
  Status(u16),
  #[error("la réponse ne contient pas d'octets")]
  NotBytes,
  #[error("lecture impossible : {0}")]
  Read(String),
  #[error(transparent)]
  Core(#[from] CoreError),
}

/// Core et moteur audio de l'application.
pub struct Backend {
  pub core: Arc<Core>,
  pub engine: Engine,
}

impl Backend {
  /// Crée le core et le moteur.
  ///
  /// # Errors
  /// Le fil de contrôle du moteur n'a pas démarré.
  pub fn new() -> Result<(Self, Receiver<Event>), AudioError> {
    Self::with_config(CoreConfig::default())
  }

  /// Comme [`Backend::new`], avec une configuration du core. Son préfixe d'URL est toujours [`LOCAL_BASE_URL`].
  ///
  /// # Errors
  /// Le fil de contrôle du moteur n'a pas démarré.
  pub fn with_config(config: CoreConfig) -> Result<(Self, Receiver<Event>), AudioError> {
    let core = Core::new(CoreConfig {
      local_base_url: LOCAL_BASE_URL.to_string(),
      ..config
    });
    let (engine, events) = Engine::new(EngineConfig::default())?;
    Ok((Self { core, engine }, events))
  }

  /// Appelle une opération du core : `POST` si `body` est donné, sinon `GET`. Chemin inconnu : réponse 404.
  ///
  /// # Errors
  /// Erreur renvoyée par l'opération.
  pub async fn call(
    &self,
    path: &str,
    query: HashMap<String, String>,
    body: Option<Bytes>,
  ) -> Result<Reply, BackendError> {
    let method = if body.is_some() {
      Method::Post
    } else {
      Method::Get
    };
    let params = Params {
      query,
      body: body.unwrap_or_default(),
      ..Params::default()
    };
    match dispatch::call(&self.core, method, path, params).await {
      Some(result) => Ok(result?),
      None => Ok(Reply::Text {
        status: 404,
        text: "Not found".to_string(),
      }),
    }
  }

  /// Lit le corps complet d'une réponse : image, fichier ou flux.
  ///
  /// # Errors
  /// Statut autre que 2xx, réponse sans octets, ou lecture impossible.
  pub async fn reply_bytes(reply: Reply) -> Result<Bytes, BackendError> {
    let status = reply.status();
    if !(200..300).contains(&status) {
      return Err(BackendError::Status(status));
    }
    match reply {
      Reply::Bytes { body, .. } => Ok(body),
      Reply::File { path, range, .. } => {
        let bytes = tokio::task::spawn_blocking(move || read_file_range(&path, range))
          .await
          .map_err(|err| BackendError::Read(err.to_string()))?
          .map_err(|err| BackendError::Read(err.to_string()))?;
        Ok(bytes.into())
      }
      Reply::Stream { mut body, .. } => {
        let mut bytes = Vec::new();
        while let Some(chunk) = body.next().await {
          bytes.extend_from_slice(&chunk?);
        }
        Ok(bytes.into())
      }
      Reply::Json { .. } | Reply::Text { .. } => Err(BackendError::NotBytes),
    }
  }

  /// Résout une URL `riff://local/<chemin>?<query>` renvoyée par le core.
  ///
  /// # Errors
  /// URL d'un autre schéma ou d'un autre hôte, ou erreur renvoyée par l'opération.
  pub async fn resolve_url(&self, url: &str) -> Result<Reply, BackendError> {
    let parsed = Url::parse(url).map_err(|_| BackendError::BadUrl(url.to_string()))?;
    if parsed.scheme() != LOCAL_SCHEME || parsed.host_str() != Some(LOCAL_HOST) {
      return Err(BackendError::BadUrl(url.to_string()));
    }
    let query = parsed.query_pairs().into_owned().collect();
    self.call(parsed.path(), query, None).await
  }
}
