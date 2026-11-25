//! Vérification de mise à jour, sur demande seulement. Une requête GET sans corps ni jeton vers
//! l'API des releases, par le client `Service` du core. Aucune donnée de l'utilisateur ni de l'installation n'est
//! envoyée : pas de version dans l'agent, pas d'identifiant. Le téléchargement et l'installation sont dans `update_install`.

use riff_core::net::Http;
use serde::Deserialize;
use std::cmp::Ordering;
use std::time::Duration;

/// Dépôt des releases. Constante unique. Une réponse 404 de l'API (sans jeton) est affichée par l'interface comme
/// « aucune release publique ».
pub const REPOSITORY: &str = "pouskit/riff-rs";
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY: u64 = 1024 * 1024;
const MAX_NOTES_CHARS: usize = 2000;
/// La page de release ne peut être qu'ici : l'adresse vient du réseau, elle n'est jamais ouverte telle quelle sinon.
const RELEASE_PAGE_PREFIX: &str = "https://github.com/";

#[must_use]
pub fn releases_url() -> String {
  format!("https://api.github.com/repos/{REPOSITORY}/releases/latest")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
  pub version: String,
  pub notes: String,
  pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
  UpToDate,
  Available(Release),
  /// Le dépôt ne publie aucune release lisible sans jeton (404).
  NoRelease,
}

/// Avancement de la vérification, montré dans « à propos ».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum UpdateState {
  #[default]
  Idle,
  Checking,
  Done(Outcome),
  Failed(CheckError),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
  #[error("network error: {0}")]
  Network(String),
  #[error("rate limited by the release server, try again later")]
  RateLimited,
  #[error("unexpected reply from the release server ({0})")]
  Status(u16),
  #[error("unreadable reply from the release server")]
  Malformed,
}

#[derive(Deserialize)]
struct RawRelease {
  tag_name: String,
  #[serde(default)]
  body: Option<String>,
  html_url: String,
  #[serde(default)]
  draft: bool,
  #[serde(default)]
  prerelease: bool,
}

/// Composantes numériques d'une version `v1.2.3-rc1+meta` et présence d'un suffixe de préversion.
fn parse_version(text: &str) -> Option<(Vec<u64>, bool)> {
  let text = text.trim().trim_start_matches(['v', 'V']);
  let without_build = text.split('+').next()?;
  let (core, pre) = match without_build.split_once('-') {
    Some((core, _)) => (core, true),
    None => (without_build, false),
  };
  let parts = core
    .split('.')
    .map(|part| part.parse::<u64>().ok())
    .collect::<Option<Vec<_>>>()?;
  (!parts.is_empty()).then_some((parts, pre))
}

/// `candidate` est strictement plus récente que `current`. `None` : l'une des deux n'est pas une version.
fn compare(candidate: &str, current: &str) -> Option<Ordering> {
  let (mut a, a_pre) = parse_version(candidate)?;
  let (mut b, b_pre) = parse_version(current)?;
  let len = a.len().max(b.len());
  a.resize(len, 0);
  b.resize(len, 0);
  Some(a.cmp(&b).then_with(|| b_pre.cmp(&a_pre)))
}

fn truncate(notes: &str) -> String {
  match notes.char_indices().nth(MAX_NOTES_CHARS) {
    Some((end, _)) => format!("{}…", notes[..end].trim_end()),
    None => notes.trim().to_string(),
  }
}

/// Lit la réponse de l'API. Pure : les essais lui passent des réponses enregistrées.
pub fn interpret(status: u16, body: &[u8], current: &str) -> Result<Outcome, CheckError> {
  match status {
    200 => {}
    404 => return Ok(Outcome::NoRelease),
    403 | 429 => return Err(CheckError::RateLimited),
    other => return Err(CheckError::Status(other)),
  }
  let raw: RawRelease = serde_json::from_slice(body).map_err(|_| CheckError::Malformed)?;
  if raw.draft || raw.prerelease {
    return Ok(Outcome::UpToDate);
  }
  if !raw.html_url.starts_with(RELEASE_PAGE_PREFIX) {
    return Err(CheckError::Malformed);
  }
  match compare(&raw.tag_name, current).ok_or(CheckError::Malformed)? {
    Ordering::Greater => Ok(Outcome::Available(Release {
      version: raw
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string(),
      notes: truncate(raw.body.as_deref().unwrap_or_default()),
      url: raw.html_url,
    })),
    Ordering::Equal | Ordering::Less => Ok(Outcome::UpToDate),
  }
}

/// Une requête GET vers `url`. `http` est le client `Service` : l'hôte doit figurer dans sa liste d'autorisation.
pub async fn check(http: &Http, url: &str, current: &str) -> Result<Outcome, CheckError> {
  let response = http
    .get(url)
    .header("Accept", "application/vnd.github+json")
    // GitHub refuse une requête sans agent. Valeur fixe : ni version ni identifiant.
    .header("User-Agent", "riff-rs")
    .timeout(TIMEOUT)
    .max_bytes(MAX_BODY)
    .send()
    .await
    .map_err(|error| CheckError::Network(error.to_string()))?;
  let status = response.status().as_u16();
  let body = response
    .bytes()
    .await
    .map_err(|error| CheckError::Network(error.to_string()))?;
  interpret(status, &body, current)
}

/// Vérifie la dernière release du dépôt par rapport à la version de ce programme.
pub async fn check_latest() -> Result<Outcome, CheckError> {
  check(&Http::service(), &releases_url(), env!("CARGO_PKG_VERSION")).await
}

#[cfg(test)]
mod tests {
  use super::*;

  fn release(tag: &str) -> String {
    format!(
      r#"{{"tag_name":"{tag}","name":"Riff","draft":false,"prerelease":false,"html_url":"https://github.com/pouskit/riff-rs/releases/tag/{tag}","body":"- correctif\n- nouveauté"}}"#
    )
  }

  #[test]
  fn a_newer_release_is_offered_with_its_notes_and_page() {
    let outcome = interpret(200, release("v0.2.0").as_bytes(), "0.1.0").unwrap();
    assert_eq!(
      outcome,
      Outcome::Available(Release {
        version: "0.2.0".into(),
        notes: "- correctif\n- nouveauté".into(),
        url: "https://github.com/pouskit/riff-rs/releases/tag/v0.2.0".into(),
      })
    );
  }

  #[test]
  fn the_same_or_an_older_release_is_up_to_date() {
    assert_eq!(
      interpret(200, release("v0.1.0").as_bytes(), "0.1.0").unwrap(),
      Outcome::UpToDate
    );
    assert_eq!(
      interpret(200, release("0.0.9").as_bytes(), "0.1.0").unwrap(),
      Outcome::UpToDate
    );
  }

  #[test]
  fn versions_compare_numerically_not_as_text() {
    assert_eq!(compare("0.10.0", "0.9.0"), Some(Ordering::Greater));
    assert_eq!(compare("1.0", "1.0.0"), Some(Ordering::Equal));
    assert_eq!(compare("1.0.0", "1.0.0-rc1"), Some(Ordering::Greater));
    assert_eq!(compare("1.0.0-rc1", "1.0.0"), Some(Ordering::Less));
    assert_eq!(compare("1.2.3+build5", "1.2.3"), Some(Ordering::Equal));
    assert_eq!(compare("nightly", "1.0.0"), None);
  }

  #[test]
  fn a_missing_repository_is_not_an_error() {
    assert_eq!(
      interpret(404, br#"{"message":"Not Found"}"#, "0.1.0").unwrap(),
      Outcome::NoRelease
    );
  }

  #[test]
  fn a_rate_limit_is_reported_as_such() {
    let body = br#"{"message":"API rate limit exceeded"}"#;
    assert_eq!(interpret(403, body, "0.1.0"), Err(CheckError::RateLimited));
    assert_eq!(interpret(429, body, "0.1.0"), Err(CheckError::RateLimited));
  }

  #[test]
  fn other_statuses_and_garbage_are_errors() {
    assert_eq!(interpret(500, b"", "0.1.0"), Err(CheckError::Status(500)));
    assert_eq!(
      interpret(200, b"<html>", "0.1.0"),
      Err(CheckError::Malformed)
    );
    assert_eq!(
      interpret(200, release("nightly").as_bytes(), "0.1.0"),
      Err(CheckError::Malformed)
    );
  }

  #[test]
  fn a_release_page_outside_github_is_refused() {
    let body = release("v0.2.0").replace("https://github.com/", "https://evil.example/");
    assert_eq!(
      interpret(200, body.as_bytes(), "0.1.0"),
      Err(CheckError::Malformed)
    );
  }

  #[test]
  fn drafts_and_prereleases_are_ignored() {
    let body = release("v0.2.0").replace(r#""prerelease":false"#, r#""prerelease":true"#);
    assert_eq!(
      interpret(200, body.as_bytes(), "0.1.0").unwrap(),
      Outcome::UpToDate
    );
  }

  #[test]
  fn long_notes_are_cut_on_a_character_boundary() {
    let body = release("v0.2.0").replace("- correctif", &"é".repeat(MAX_NOTES_CHARS + 50));
    let Outcome::Available(found) = interpret(200, body.as_bytes(), "0.1.0").unwrap() else {
      panic!("release attendue")
    };
    assert_eq!(
      found.notes.chars().filter(|c| *c == 'é').count(),
      MAX_NOTES_CHARS
    );
    assert!(found.notes.ends_with('…'));
  }

  #[test]
  fn the_request_goes_through_the_service_allowlist_only() {
    assert!(releases_url().starts_with("https://api.github.com/repos/"));
    assert!(riff_core::net::allowlist::is_allowed("api.github.com"));
    assert!(!riff_core::os::is_allowed_external_url(
      "https://api.github.com/repos/x/y"
    ));
  }
}
