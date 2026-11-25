//! Dossier temporaire de test, supprimé à la fin du test (même en cas de panique).
// Inclus par plusieurs binaires de test : chacun n'en utilise qu'une partie.
#![allow(dead_code)]

use std::cell::RefCell;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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

/// Dossier commun à tous les tests d'un binaire, au chemin stable (des variables d'environnement le visent).
/// Il est (re)créé quand aucun fil de test ne l'utilise et supprimé quand le dernier de ces fils se termine.
pub struct SharedDir {
  tag: &'static str,
  users: Mutex<usize>,
}

struct Lease(&'static SharedDir);

thread_local! {
  static LEASES: RefCell<Vec<Lease>> = const { RefCell::new(Vec::new()) };
}

impl SharedDir {
  pub const fn new(tag: &'static str) -> Self {
    Self {
      tag,
      users: Mutex::new(0),
    }
  }

  pub fn path(&self) -> PathBuf {
    std::env::temp_dir().join(format!("riff-{}-{}", self.tag, std::process::id()))
  }

  /// Garde le dossier pour le fil courant jusqu'à sa fin. `fill` remplit un dossier neuf, il peut donc être rappelé.
  pub fn lease(&'static self, fill: impl FnOnce(&Path)) -> PathBuf {
    let path = self.path();
    LEASES.with(|leases| {
      let mut leases = leases.borrow_mut();
      if leases.iter().any(|lease| std::ptr::eq(lease.0, self)) {
        return;
      }
      let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
      if *users == 0 {
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        fill(&path);
      }
      *users += 1;
      leases.push(Lease(self));
    });
    path
  }
}

impl Drop for Lease {
  fn drop(&mut self) {
    let mut users = self.0.users.lock().unwrap_or_else(|e| e.into_inner());
    *users -= 1;
    if *users == 0 {
      let _ = std::fs::remove_dir_all(self.0.path());
    }
  }
}
