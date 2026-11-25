//! Dossier temporaire de test, supprimé à la fin du test (même en cas de panique).

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub struct TempDir(PathBuf);

impl TempDir {
  /// Crée un dossier vide et unique `riff-{tag}-{pid}-{n}` sous le dossier temporaire.
  pub fn new(tag: &str) -> Self {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
      "riff-{tag}-{}-{}",
      std::process::id(),
      NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Self(dir)
  }
}

impl Deref for TempDir {
  type Target = Path;

  fn deref(&self) -> &Path {
    &self.0
  }
}

impl AsRef<Path> for TempDir {
  fn as_ref(&self) -> &Path {
    &self.0
  }
}

impl Drop for TempDir {
  fn drop(&mut self) {
    // Sous Windows un fichier encore ouvert fait échouer la suppression : ignoré.
    let _ = std::fs::remove_dir_all(&self.0);
  }
}
