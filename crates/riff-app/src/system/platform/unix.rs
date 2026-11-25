//! Linux et macOS : NON COMPILÉ sur la machine de développement (Windows seul), à vérifier par la CI.

use std::io;
use std::os::unix::fs::MetadataExt;

/// Numéro d'utilisateur, lu sur le dossier personnel (évite une dépendance à `libc`).
pub fn user_id() -> io::Result<String> {
  let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
  Ok(std::fs::metadata(home)?.uid().to_string())
}

/// Chemin du socket : dans `XDG_RUNTIME_DIR` (réservé à l'utilisateur) ou, à défaut, dans un dossier `0700` du dossier temporaire.
pub fn socket_path(label: &str) -> io::Result<std::path::PathBuf> {
  use std::os::unix::fs::DirBuilderExt;
  let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
    Some(runtime) => std::path::PathBuf::from(runtime),
    None => {
      let dir = std::env::temp_dir().join(format!("riff-rs-{}", user_id()?));
      std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
      dir
    }
  };
  Ok(dir.join(format!("{label}.sock")))
}
