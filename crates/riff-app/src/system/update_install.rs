//! Téléchargement, vérification et installation d'une mise à jour. Sur clic seulement, après
//! la vérification de `updates`. L'installeur n'est lancé que si sa signature minisign (clé publique embarquée) ET
//! son SHA-256 publié sont bons. Procédure de signature : `docs/RELEASING.md`.

use super::updates::{REPOSITORY, Release};
use minisign_verify::{PublicKey, Signature};
use riff_core::net::Http;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Clé publique minisign de la release, ligne en base64 de `minisign.pub` (la seconde ligne du fichier).
///
/// PROVISOIRE : aucune clé de production n'existe encore. Vide, elle fait refuser toute mise à jour
/// (`VerifyError::KeyNotConfigured`). À remplacer par le mainteneur selon `docs/RELEASING.md`.
pub const UPDATE_PUBLIC_KEY: &str = "";

/// Fichier de sommes joint à la release : lignes `<sha256 hexadécimal>  <nom du fichier>` (sortie de `sha256sum`).
pub const CHECKSUMS_NAME: &str = "SHA256SUMS";
/// Suffixe du fichier de signature joint à la release : `<installeur>.minisig`.
pub const SIGNATURE_SUFFIX: &str = ".minisig";

const MAX_INSTALLER_BYTES: u64 = 300 * 1024 * 1024;
const MAX_SMALL_BYTES: u64 = 64 * 1024;
const SMALL_TIMEOUT: Duration = Duration::from_secs(20);
const INSTALLER_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const USER_AGENT: &str = "riff-rs";
/// Options de l'installeur NSIS (gabarit `packaging/installer.nsi`) : `/S` silencieux (NSIS), `/P` sans page
/// (le gabarit saute alors la page de réinstallation), `/NS` sans raccourci du Bureau recréé, `/R` relance après
/// installation. Aucune donnée utilisateur n'est touchée : `appdata_paths` n'est pas configuré.
#[cfg(windows)]
const INSTALLER_ARGS: [&str; 4] = ["/S", "/P", "/NS", "/R"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
  #[error("this build has no update signing key, so no update can be trusted")]
  KeyNotConfigured,
  #[error("the release has no signature file for this installer")]
  SignatureMissing,
  #[error("the signature file is unreadable")]
  SignatureMalformed,
  #[error("the signature does not match this installer, or was made by another key")]
  SignatureRejected,
  #[error("the signature was made for another file")]
  SignatureWrongFile,
  #[error("the release publishes no SHA-256 for this installer")]
  ChecksumMissing,
  #[error("the SHA-256 of the installer differs from the published one")]
  ChecksumMismatch,
  #[error("could not read the installer: {0}")]
  Io(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
  #[error("automatic installation is only available on Windows, open the release page instead")]
  Unsupported,
  #[error("download failed: {0}")]
  Network(String),
  #[error("the download is larger than expected")]
  TooLarge,
  #[error("the download took too long")]
  Timeout,
  #[error("the release has no file {0}")]
  MissingAsset(String),
  #[error("update refused, nothing was run: {0}")]
  Verify(#[from] VerifyError),
  #[error("could not write the installer: {0}")]
  Io(String),
  #[error("could not start the installer: {0}")]
  Launch(String),
}

/// Avancement de l'installation, montré dans « à propos ».
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum InstallState {
  #[default]
  Idle,
  /// Pourcentage de l'installeur reçu.
  Downloading(u8),
  Verifying,
  /// L'installeur tourne : l'application se ferme pour qu'il remplace le binaire.
  Launched,
  Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallEvent {
  Progress(u8),
  Verifying,
  Launched,
  Failed(InstallError),
}

/// L'installation automatique n'existe que sous Windows (NSIS). Linux (AppImage) et macOS (dmg) : page de la release.
#[must_use]
pub const fn can_install() -> bool {
  cfg!(windows)
}

/// Nom de l'installeur publié par `cargo packager --formats nsis` (x64).
#[must_use]
pub fn installer_name(version: &str) -> String {
  format!("riff-app_{version}_x64-setup.exe")
}

fn asset_url(base: &str, version: &str, name: &str) -> String {
  format!("{base}/v{version}/{name}")
}

fn release_base() -> String {
  format!("https://github.com/{REPOSITORY}/releases/download")
}

/// SHA-256 hexadécimal (minuscules) publié pour `file_name` dans un fichier de sommes (`sha256sum`, `*` binaire permis).
fn published_hash(sums: &str, file_name: &str) -> Option<String> {
  sums.lines().find_map(|line| {
    let (hash, rest) = line.trim().split_once(char::is_whitespace)?;
    let name = rest.trim_start().trim_start_matches('*');
    (name == file_name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
      .then(|| hash.to_ascii_lowercase())
  })
}

/// Vérifie `path` : signature minisign de `public_key` (ligne base64) ET SHA-256 publié. Les deux sont exigés.
///
/// # Errors
/// [`VerifyError`] au premier refus : clé absente ou invalide, signature absente, illisible, d'une autre clé ou
/// d'un autre fichier, somme absente ou différente.
pub fn verify_file(
  path: &Path,
  public_key: &str,
  signature: Option<&str>,
  sums: Option<&str>,
  file_name: &str,
) -> Result<(), VerifyError> {
  let key = PublicKey::from_base64(public_key.trim()).map_err(|_| VerifyError::KeyNotConfigured)?;
  let signature = Signature::decode(signature.ok_or(VerifyError::SignatureMissing)?)
    .map_err(|_| VerifyError::SignatureMalformed)?;
  // Le commentaire de confiance est signé : il lie la signature au nom (donc à la version) du fichier. Sans cela,
  // un ancien installeur légitimement signé passerait pour le nouveau.
  let wanted = format!("file:{file_name}");
  if !signature
    .trusted_comment()
    .split_whitespace()
    .any(|token| token == wanted)
  {
    return Err(VerifyError::SignatureWrongFile);
  }
  let want = sums
    .and_then(|sums| published_hash(sums, file_name))
    .ok_or(VerifyError::ChecksumMissing)?;
  let mut stream = key
    .verify_stream(&signature)
    .map_err(|_| VerifyError::SignatureRejected)?;
  let mut file = fs::File::open(path).map_err(|e| VerifyError::Io(e.to_string()))?;
  let mut sha = Sha256::new();
  let mut buffer = vec![0u8; 64 * 1024];
  loop {
    let read = file
      .read(&mut buffer)
      .map_err(|e| VerifyError::Io(e.to_string()))?;
    if read == 0 {
      break;
    }
    stream.update(&buffer[..read]);
    sha.update(&buffer[..read]);
  }
  stream
    .finalize()
    .map_err(|_| VerifyError::SignatureRejected)?;
  let got: String = sha.finalize().iter().map(|b| format!("{b:02x}")).collect();
  if got != want {
    return Err(VerifyError::ChecksumMismatch);
  }
  Ok(())
}

/// Petit fichier (somme, signature) : `None` si l'hôte répond 404.
async fn fetch_small(http: &Http, url: &str) -> Result<Option<String>, InstallError> {
  let response = http
    .get(url)
    .timeout(SMALL_TIMEOUT)
    .max_bytes(MAX_SMALL_BYTES)
    .send()
    .await
    .map_err(network_error)?;
  match response.status().as_u16() {
    200 => {}
    404 => return Ok(None),
    other => return Err(InstallError::Network(format!("HTTP {other}"))),
  }
  let bytes = response.bytes().await.map_err(network_error)?;
  Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn network_error(error: riff_core::net::Error) -> InstallError {
  if matches!(error, riff_core::net::Error::TooLarge { .. }) {
    InstallError::TooLarge
  } else if error.is_timeout() {
    InstallError::Timeout
  } else {
    InstallError::Network(error.to_string())
  }
}

async fn download_installer(
  http: &Http,
  url: &str,
  dest: &Path,
  progress: &mut impl FnMut(u8),
) -> Result<(), InstallError> {
  use std::io::Write;
  let mut response = http
    .get(url)
    .streaming()
    .max_bytes(MAX_INSTALLER_BYTES)
    .send()
    .await
    .map_err(network_error)?;
  match response.status().as_u16() {
    200 => {}
    404 => return Err(InstallError::MissingAsset(file_name_of(url))),
    other => return Err(InstallError::Network(format!("HTTP {other}"))),
  }
  let total = response.content_length().filter(|n| *n > 0);
  if total.is_some_and(|n| n > MAX_INSTALLER_BYTES) {
    return Err(InstallError::TooLarge);
  }
  let mut file = fs::File::create(dest).map_err(|e| InstallError::Io(e.to_string()))?;
  let mut done = 0u64;
  while let Some(chunk) = response.chunk().await.map_err(network_error)? {
    file
      .write_all(&chunk)
      .map_err(|e| InstallError::Io(e.to_string()))?;
    done += chunk.len() as u64;
    if let Some(total) = total {
      progress(u8::try_from((done * 100 / total).min(100)).unwrap_or(100));
    }
  }
  file.flush().map_err(|e| InstallError::Io(e.to_string()))
}

fn file_name_of(url: &str) -> String {
  url.rsplit('/').next().unwrap_or(url).to_string()
}

/// Télécharge l'installeur de `version` depuis `base` (`<base>/v<version>/<fichier>`) dans `dir`, puis le vérifie.
/// Au moindre refus ou échec, `dir` est vidé : aucun fichier non vérifié ne survit.
///
/// # Errors
/// [`InstallError`] : réseau, taille, délai, fichier absent de la release ou vérification refusée.
pub async fn fetch_verified(
  http: &Http,
  base: &str,
  version: &str,
  dir: &Path,
  public_key: &str,
  mut event: impl FnMut(InstallEvent),
) -> Result<PathBuf, InstallError> {
  let name = installer_name(version);
  let result = async {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).map_err(|e| InstallError::Io(e.to_string()))?;
    let sums = fetch_small(http, &asset_url(base, version, CHECKSUMS_NAME)).await?;
    let signature = fetch_small(
      http,
      &asset_url(base, version, &format!("{name}{SIGNATURE_SUFFIX}")),
    )
    .await?;
    // Refus avant de télécharger quoi que ce soit si la release n'est pas signée.
    if signature.is_none() {
      return Err(VerifyError::SignatureMissing.into());
    }
    if sums.is_none() {
      return Err(VerifyError::ChecksumMissing.into());
    }
    let path = dir.join(&name);
    event(InstallEvent::Progress(0));
    let url = asset_url(base, version, &name);
    let mut last = 0u8;
    let mut on_progress = |p: u8| {
      if p != last {
        last = p;
        event(InstallEvent::Progress(p));
      }
    };
    tokio::time::timeout(
      INSTALLER_TIMEOUT,
      download_installer(http, &url, &path, &mut on_progress),
    )
    .await
    .map_err(|_| InstallError::Timeout)??;
    event(InstallEvent::Verifying);
    let (check_path, key) = (path.clone(), public_key.to_owned());
    let check_name = name.clone();
    tokio::task::spawn_blocking(move || {
      verify_file(
        &check_path,
        &key,
        signature.as_deref(),
        sums.as_deref(),
        &check_name,
      )
    })
    .await
    .map_err(|e| InstallError::Io(e.to_string()))??;
    Ok(path)
  }
  .await;
  if result.is_err() {
    let _ = fs::remove_dir_all(dir);
  }
  result
}

/// Lance l'installeur NSIS en mode silencieux par utilisateur. Il tue lui-même une instance encore ouverte
/// (`CheckIfAppIsRunning`) et relance l'application (`/R`) ; l'appelant doit donc fermer l'application aussitôt.
///
/// # Errors
/// [`InstallError::Launch`] si le processus ne démarre pas, [`InstallError::Unsupported`] hors Windows.
#[cfg(windows)]
pub fn launch_installer(path: &Path) -> Result<(), InstallError> {
  use std::os::windows::process::CommandExt;
  const CREATE_NO_WINDOW: u32 = 0x0800_0000;
  std::process::Command::new(path)
    .args(INSTALLER_ARGS)
    .creation_flags(CREATE_NO_WINDOW)
    .spawn()
    .map(drop)
    .map_err(|e| InstallError::Launch(e.to_string()))
}

#[cfg(not(windows))]
pub fn launch_installer(_path: &Path) -> Result<(), InstallError> {
  Err(InstallError::Unsupported)
}

fn work_dir() -> PathBuf {
  std::env::temp_dir().join("riff-rs-update")
}

/// Flux complet pour l'interface : téléchargement, vérification, lancement. Le dernier événement est
/// `Launched` ou `Failed`.
pub fn run(release: Release) -> impl iced::futures::Stream<Item = InstallEvent> {
  iced::stream::channel(
    32,
    async move |mut out: iced::futures::channel::mpsc::Sender<InstallEvent>| {
      use iced::futures::SinkExt;
      let last = if can_install() {
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded();
        let http = Http::tools(USER_AGENT);
        let version = release.version;
        let work = tokio::spawn(async move {
          let dir = work_dir();
          let path = fetch_verified(
            &http,
            &release_base(),
            &version,
            &dir,
            UPDATE_PUBLIC_KEY,
            |e| {
              let _ = tx.unbounded_send(e);
            },
          )
          .await?;
          launch_installer(&path).inspect_err(|_| {
            let _ = fs::remove_dir_all(&dir);
          })
        });
        while let Some(event) = iced::futures::StreamExt::next(&mut rx).await {
          let _ = out.send(event).await;
        }
        match work.await {
          Ok(Ok(())) => InstallEvent::Launched,
          Ok(Err(error)) => InstallEvent::Failed(error),
          Err(error) => InstallEvent::Failed(InstallError::Io(error.to_string())),
        }
      } else {
        InstallEvent::Failed(InstallError::Unsupported)
      };
      let _ = out.send(last).await;
    },
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;
  use std::sync::atomic::{AtomicU32, Ordering};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  use tokio::net::TcpListener;

  // Fixtures : paire de clés A et B de TEST (jamais la clé de production), générées une fois avec la crate
  // `minisign` 0.9.1 ; les clés secrètes ont été jetées. `payload.a.minisig` est la signature de `payload.bin` par A,
  // commentaire de confiance `timestamp:0 file:riff-app_9.9.9_x64-setup.exe`.
  const PAYLOAD: &[u8] = include_bytes!("../../tests/data/update/payload.bin");
  const SIG_A: &str = include_str!("../../tests/data/update/payload.a.minisig");
  const SIG_B: &str = include_str!("../../tests/data/update/payload.b.minisig");
  const KEY_A: &str = include_str!("../../tests/data/update/key-a.pub");
  const NAME: &str = "riff-app_9.9.9_x64-setup.exe";

  fn key(pub_file: &str) -> String {
    pub_file.lines().nth(1).unwrap().to_string()
  }

  fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
      .iter()
      .map(|b| format!("{b:02x}"))
      .collect()
  }

  fn sums(bytes: &[u8], name: &str) -> String {
    format!("{}  {name}\n", sha(bytes))
  }

  struct Scratch(PathBuf);
  impl Scratch {
    fn new() -> Self {
      static NEXT: AtomicU32 = AtomicU32::new(0);
      let dir = std::env::temp_dir().join(format!(
        "riff-update-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
      ));
      fs::create_dir_all(&dir).unwrap();
      Self(dir)
    }
    fn file(&self, bytes: &[u8]) -> PathBuf {
      let path = self.0.join("installer.bin");
      fs::write(&path, bytes).unwrap();
      path
    }
  }
  impl Drop for Scratch {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.0);
    }
  }

  fn check(
    bytes: &[u8],
    signature: Option<&str>,
    sums_text: Option<&str>,
    public_key: &str,
  ) -> Result<(), VerifyError> {
    let scratch = Scratch::new();
    verify_file(&scratch.file(bytes), public_key, signature, sums_text, NAME)
  }

  #[test]
  fn a_file_signed_by_the_embedded_key_with_the_right_sha256_is_accepted() {
    let good = sums(PAYLOAD, NAME);
    assert_eq!(
      check(PAYLOAD, Some(SIG_A), Some(&good), &key(KEY_A)),
      Ok(())
    );
  }

  #[test]
  fn a_file_modified_by_one_byte_is_refused() {
    let mut changed = PAYLOAD.to_vec();
    changed[1000] ^= 1;
    // Somme recalculée sur le fichier modifié : seule la signature peut le refuser.
    let good_sums = sums(&changed, NAME);
    assert_eq!(
      check(&changed, Some(SIG_A), Some(&good_sums), &key(KEY_A)),
      Err(VerifyError::SignatureRejected)
    );
  }

  #[test]
  fn a_signature_made_by_another_key_is_refused() {
    let good = sums(PAYLOAD, NAME);
    assert_eq!(
      check(PAYLOAD, Some(SIG_B), Some(&good), &key(KEY_A)),
      Err(VerifyError::SignatureRejected)
    );
  }

  #[test]
  fn a_missing_signature_is_refused() {
    let good = sums(PAYLOAD, NAME);
    assert_eq!(
      check(PAYLOAD, None, Some(&good), &key(KEY_A)),
      Err(VerifyError::SignatureMissing)
    );
    assert_eq!(
      check(PAYLOAD, Some("pas une signature"), Some(&good), &key(KEY_A)),
      Err(VerifyError::SignatureMalformed)
    );
  }

  #[test]
  fn a_wrong_or_missing_sha256_is_refused_even_with_a_good_signature() {
    let wrong = sums(b"autre contenu", NAME);
    assert_eq!(
      check(PAYLOAD, Some(SIG_A), Some(&wrong), &key(KEY_A)),
      Err(VerifyError::ChecksumMismatch)
    );
    assert_eq!(
      check(PAYLOAD, Some(SIG_A), None, &key(KEY_A)),
      Err(VerifyError::ChecksumMissing)
    );
    let other_file = sums(PAYLOAD, "autre.exe");
    assert_eq!(
      check(PAYLOAD, Some(SIG_A), Some(&other_file), &key(KEY_A)),
      Err(VerifyError::ChecksumMissing)
    );
  }

  #[test]
  fn a_signature_made_for_another_file_name_is_refused() {
    let good = sums(PAYLOAD, "riff-app_9.9.8_x64-setup.exe");
    let scratch = Scratch::new();
    assert_eq!(
      verify_file(
        &scratch.file(PAYLOAD),
        &key(KEY_A),
        Some(SIG_A),
        Some(&good),
        "riff-app_9.9.8_x64-setup.exe"
      ),
      Err(VerifyError::SignatureWrongFile)
    );
  }

  #[test]
  fn the_provisional_production_key_refuses_everything() {
    assert_eq!(UPDATE_PUBLIC_KEY, "", "clé de production à remplacer");
    let good = sums(PAYLOAD, NAME);
    // Même une signature et une somme parfaites, d'une clé de test, sont refusées.
    assert_eq!(
      check(PAYLOAD, Some(SIG_A), Some(&good), UPDATE_PUBLIC_KEY),
      Err(VerifyError::KeyNotConfigured)
    );
    assert_eq!(
      check(PAYLOAD, Some(SIG_A), Some(&good), "pas une clé"),
      Err(VerifyError::KeyNotConfigured)
    );
  }

  #[test]
  fn published_hashes_are_read_like_sha256sum_writes_them() {
    let h = "ab".repeat(32);
    let text = format!(
      "{h}  a.exe\n{} *b.exe\n{h} c.exe\nnot-a-hash  d.exe\n",
      "CD".repeat(32)
    );
    assert_eq!(published_hash(&text, "a.exe"), Some(h.clone()));
    assert_eq!(published_hash(&text, "b.exe"), Some("cd".repeat(32)));
    assert_eq!(published_hash(&text, "c.exe"), Some(h));
    assert_eq!(published_hash(&text, "d.exe"), None);
    assert_eq!(published_hash(&text, "absent.exe"), None);
  }

  #[test]
  fn the_installer_is_named_like_the_packager_names_it() {
    assert_eq!(
      installer_name("1.0.0-rc.1"),
      "riff-app_1.0.0-rc.1_x64-setup.exe"
    );
    assert_eq!(
      asset_url(&release_base(), "1.0.0", "SHA256SUMS"),
      "https://github.com/pouskit/riff-rs/releases/download/v1.0.0/SHA256SUMS"
    );
  }

  #[test]
  fn downloads_only_reach_hosts_the_service_profile_already_allows() {
    let http = Http::tools(USER_AGENT);
    for url in [
      "https://github.com/x/y/releases/download/v1/a",
      "https://objects.githubusercontent.com/a",
      "https://release-assets.githubusercontent.com/a",
    ] {
      assert!(http.check(&url.parse().unwrap()).is_ok(), "{url}");
    }
    assert!(
      http
        .check(&"https://evil.example/a".parse().unwrap())
        .is_err()
    );
    assert!(release_base().starts_with("https://github.com/"));
  }

  #[test]
  fn installation_is_windows_only() {
    assert_eq!(can_install(), cfg!(windows));
    #[cfg(not(windows))]
    assert_eq!(
      launch_installer(Path::new("x")),
      Err(InstallError::Unsupported)
    );
  }

  /// Serveur HTTP local minimal : `chemin -> corps`, 404 sinon.
  async fn serve(files: HashMap<String, Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
      while let Ok((mut socket, _)) = listener.accept().await {
        let files = files.clone();
        tokio::spawn(async move {
          let mut request = Vec::new();
          let mut buffer = [0u8; 1024];
          while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match socket.read(&mut buffer).await {
              Ok(0) | Err(_) => return,
              Ok(n) => request.extend_from_slice(&buffer[..n]),
            }
          }
          let line = String::from_utf8_lossy(&request);
          let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
          let (status, body) = match files.get(&path) {
            Some(body) => ("200 OK", body.clone()),
            None => ("404 Not Found", Vec::new()),
          };
          let head = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
          );
          let _ = socket.write_all(head.as_bytes()).await;
          let _ = socket.write_all(&body).await;
        });
      }
    });
    base
  }

  fn release_files(
    version: &str,
    installer: &[u8],
    signature: Option<&str>,
    sums_text: &str,
  ) -> HashMap<String, Vec<u8>> {
    let name = installer_name(version);
    let mut files = HashMap::new();
    files.insert(format!("/v{version}/{name}"), installer.to_vec());
    files.insert(
      format!("/v{version}/{CHECKSUMS_NAME}"),
      sums_text.as_bytes().to_vec(),
    );
    if let Some(signature) = signature {
      files.insert(
        format!("/v{version}/{name}{SIGNATURE_SUFFIX}"),
        signature.as_bytes().to_vec(),
      );
    }
    files
  }

  #[tokio::test]
  async fn the_full_flow_accepts_a_signed_release_and_reports_progress() {
    let version = "9.9.9";
    let scratch = Scratch::new();
    let dir = scratch.0.join("update");
    let base = serve(release_files(
      version,
      PAYLOAD,
      Some(SIG_A),
      &sums(PAYLOAD, NAME),
    ))
    .await;
    let mut events = Vec::new();
    let path = fetch_verified(&Http::local(), &base, version, &dir, &key(KEY_A), |e| {
      events.push(e)
    })
    .await
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), PAYLOAD);
    assert_eq!(events.first(), Some(&InstallEvent::Progress(0)));
    assert!(events.contains(&InstallEvent::Progress(100)));
    assert_eq!(events.last(), Some(&InstallEvent::Verifying));
  }

  #[tokio::test]
  async fn the_full_flow_refuses_and_leaves_nothing_behind() {
    let version = "9.9.9";
    let scratch = Scratch::new();
    let dir = scratch.0.join("update");
    let good_sums = sums(PAYLOAD, NAME);

    // Octet modifié sur le serveur : signature refusée, dossier vidé.
    let mut changed = PAYLOAD.to_vec();
    changed[7] ^= 1;
    let base = serve(release_files(
      version,
      &changed,
      Some(SIG_A),
      &sums(&changed, NAME),
    ))
    .await;
    let result = fetch_verified(&Http::local(), &base, version, &dir, &key(KEY_A), |_| {}).await;
    assert_eq!(result, Err(VerifyError::SignatureRejected.into()));
    assert!(!dir.exists(), "fichier non vérifié supprimé");

    // Somme fausse : refus, dossier vidé.
    let base = serve(release_files(
      version,
      PAYLOAD,
      Some(SIG_A),
      &sums(b"autre", NAME),
    ))
    .await;
    let result = fetch_verified(&Http::local(), &base, version, &dir, &key(KEY_A), |_| {}).await;
    assert_eq!(result, Err(VerifyError::ChecksumMismatch.into()));
    assert!(!dir.exists());

    // Signature d'une autre clé.
    let base = serve(release_files(version, PAYLOAD, Some(SIG_B), &good_sums)).await;
    let result = fetch_verified(&Http::local(), &base, version, &dir, &key(KEY_A), |_| {}).await;
    assert_eq!(result, Err(VerifyError::SignatureRejected.into()));
    assert!(!dir.exists());

    // Signature absente de la release : refus avant tout téléchargement.
    let base = serve(release_files(version, PAYLOAD, None, &good_sums)).await;
    let mut events = Vec::new();
    let result = fetch_verified(&Http::local(), &base, version, &dir, &key(KEY_A), |e| {
      events.push(e)
    })
    .await;
    assert_eq!(result, Err(VerifyError::SignatureMissing.into()));
    assert!(events.is_empty(), "rien n'est téléchargé sans signature");
    assert!(!dir.exists());

    // Fichier de sommes absent.
    let mut files = release_files(version, PAYLOAD, Some(SIG_A), &good_sums);
    files.remove(&format!("/v{version}/{CHECKSUMS_NAME}"));
    let base = serve(files).await;
    let result = fetch_verified(&Http::local(), &base, version, &dir, &key(KEY_A), |_| {}).await;
    assert_eq!(result, Err(VerifyError::ChecksumMissing.into()));

    // Clé provisoire : tout est refusé, même une release parfaite.
    let base = serve(release_files(version, PAYLOAD, Some(SIG_A), &good_sums)).await;
    let result = fetch_verified(
      &Http::local(),
      &base,
      version,
      &dir,
      UPDATE_PUBLIC_KEY,
      |_| {},
    )
    .await;
    assert_eq!(result, Err(VerifyError::KeyNotConfigured.into()));
    assert!(!dir.exists());
  }

  #[tokio::test]
  async fn a_missing_installer_is_reported() {
    let version = "9.9.9";
    let name = installer_name(version);
    let scratch = Scratch::new();
    let dir = scratch.0.join("update");
    let mut files = release_files(version, PAYLOAD, Some(SIG_A), &sums(PAYLOAD, &name));
    files.remove(&format!("/v{version}/{name}"));
    let base = serve(files).await;
    let result = fetch_verified(&Http::local(), &base, version, &dir, &key(KEY_A), |_| {}).await;
    assert_eq!(result, Err(InstallError::MissingAsset(name)));
    assert!(!dir.exists());
  }

  #[test]
  fn the_state_starts_idle_and_errors_read_clearly() {
    assert_eq!(InstallState::default(), InstallState::Idle);
    let refused = InstallError::from(VerifyError::SignatureMissing).to_string();
    assert!(refused.starts_with("update refused, nothing was run"));
  }
}
