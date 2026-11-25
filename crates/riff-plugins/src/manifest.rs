//! En-tête d'un plugin (`// @name ...`), nom de fichier et lecture bornée de la source.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::Path;

/// Taille maximale d'un fichier `.rhai` (décision 3).
pub const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
/// Longueur maximale d'un nom de fichier de plugin.
pub const MAX_FILE_NAME: usize = 128;
/// L'en-tête est lu dans les premières lignes seulement.
const HEADER_LINES: usize = 64;
/// Longueur maximale d'une valeur de l'en-tête, en caractères.
const MAX_FIELD_CHARS: usize = 200;
/// Extension des plugins natifs.
pub const EXTENSION: &str = ".rhai";

const RESERVED_WINDOWS: [&str; 22] = [
  "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
  "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Permission déclarée par `@permissions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
  Queue,
  Library,
  Search,
  Shuffle,
  Notify,
}

impl Permission {
  pub const ALL: [Self; 5] = [
    Self::Queue,
    Self::Library,
    Self::Search,
    Self::Shuffle,
    Self::Notify,
  ];

  #[must_use]
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Queue => "queue",
      Self::Library => "library",
      Self::Search => "search",
      Self::Shuffle => "shuffle",
      Self::Notify => "notify",
    }
  }

  /// Nom exact, en minuscules.
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|perm| perm.as_str() == value)
  }
}

/// Erreurs de l'en-tête, du nom ou de la lecture d'un plugin.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
  #[error("nom de fichier invalide : {0}")]
  InvalidName(String),
  #[error("fichier trop gros : {size} octets (maximum {MAX_SOURCE_BYTES})")]
  TooLarge { size: u64 },
  #[error("lien symbolique refusé : {0}")]
  Symlink(String),
  #[error("métadonnée absente : @{0}")]
  Missing(&'static str),
  #[error("permission inconnue : {0}")]
  UnknownPermission(String),
  #[error("fichier illisible : {0}")]
  Read(String),
  #[error("le fichier n'est pas en UTF-8")]
  NotUtf8,
}

/// Métadonnées d'un plugin.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
  pub name: String,
  pub author: String,
  pub version: String,
  pub description: String,
  /// Triées, sans doublon. Vide par défaut : aucune permission.
  pub permissions: Vec<Permission>,
}

impl Manifest {
  #[must_use]
  pub fn allows(&self, permission: Permission) -> bool {
    self.permissions.contains(&permission)
  }
}

/// Source lue, validée et hachée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSource {
  pub file: String,
  pub manifest: Manifest,
  pub code: String,
  /// SHA-256 hexadécimal de la source : le consentement y est lié.
  pub hash: String,
}

/// Nom de fichier accepté : `^[A-Za-z0-9._ -]+\.rhai$`, sans `..`, sans nom réservé de Windows.
///
/// # Errors
/// [`ManifestError::InvalidName`].
pub fn validate_file_name(name: &str) -> Result<(), ManifestError> {
  let invalid = || ManifestError::InvalidName(name.chars().take(MAX_FILE_NAME).collect());
  let stem = name.strip_suffix(EXTENSION).ok_or_else(invalid)?;
  let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ' ' | '-');
  if stem.is_empty()
    || name.len() > MAX_FILE_NAME
    || !stem.chars().all(allowed)
    || name.contains("..")
    || stem.starts_with([' ', '.'])
    || stem.ends_with([' ', '.'])
  {
    return Err(invalid());
  }
  let base = stem.split('.').next().unwrap_or(stem).to_ascii_lowercase();
  if RESERVED_WINDOWS.contains(&base.as_str()) {
    return Err(invalid());
  }
  Ok(())
}

/// Texte affichable : caractères de contrôle retirés, longueur bornée.
#[must_use]
pub fn clean_text(value: &str, max_chars: usize) -> String {
  value
    .chars()
    .filter(|c| !c.is_control())
    .take(max_chars)
    .collect::<String>()
    .trim()
    .to_string()
}

/// Lit l'en-tête : lignes `//` du début du fichier.
///
/// # Errors
/// `@name` absent, ou permission inconnue.
pub fn parse_header(code: &str) -> Result<Manifest, ManifestError> {
  let mut manifest = Manifest::default();
  for line in code.lines().take(HEADER_LINES) {
    let line = line.trim();
    if line.is_empty() {
      continue;
    }
    let Some(comment) = line.strip_prefix("//") else {
      break;
    };
    let Some(tag) = comment.trim().strip_prefix('@') else {
      continue;
    };
    let (key, value) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
    let value = clean_text(value, MAX_FIELD_CHARS);
    match key.to_ascii_lowercase().as_str() {
      "name" => manifest.name = value,
      "author" => manifest.author = value,
      "version" => manifest.version = value,
      "description" => manifest.description = value,
      "permissions" => {
        for item in value
          .split(',')
          .map(str::trim)
          .filter(|item| !item.is_empty())
        {
          let perm = Permission::parse(&item.to_ascii_lowercase())
            .ok_or_else(|| ManifestError::UnknownPermission(item.to_string()))?;
          manifest.permissions.push(perm);
        }
      }
      _ => {}
    }
  }
  if manifest.name.is_empty() {
    return Err(ManifestError::Missing("name"));
  }
  manifest.permissions.sort_unstable();
  manifest.permissions.dedup();
  Ok(manifest)
}

/// SHA-256 hexadécimal.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
  Sha256::digest(bytes)
    .iter()
    .fold(String::with_capacity(64), |mut out, byte| {
      let _ = write!(out, "{byte:02x}");
      out
    })
}

/// Valide une source déjà en mémoire.
///
/// # Errors
/// Nom, taille, encodage ou en-tête refusés.
pub fn from_bytes(file: &str, bytes: &[u8]) -> Result<PluginSource, ManifestError> {
  validate_file_name(file)?;
  let size = bytes.len() as u64;
  if size > MAX_SOURCE_BYTES {
    return Err(ManifestError::TooLarge { size });
  }
  let code = std::str::from_utf8(bytes).map_err(|_| ManifestError::NotUtf8)?;
  let manifest = parse_header(code)?;
  Ok(PluginSource {
    file: file.to_string(),
    manifest,
    code: code.to_string(),
    hash: sha256_hex(bytes),
  })
}

/// Lit `dir/file` : nom validé avant tout accès, lien symbolique refusé, taille vérifiée avant lecture.
///
/// # Errors
/// Voir [`ManifestError`].
pub fn load(dir: &Path, file: &str) -> Result<PluginSource, ManifestError> {
  validate_file_name(file)?;
  let path = dir.join(file);
  let meta = fs::symlink_metadata(&path).map_err(|err| ManifestError::Read(err.to_string()))?;
  if meta.file_type().is_symlink() {
    return Err(ManifestError::Symlink(file.to_string()));
  }
  if !meta.is_file() {
    return Err(ManifestError::Read(format!("{file} n'est pas un fichier")));
  }
  if meta.len() > MAX_SOURCE_BYTES {
    return Err(ManifestError::TooLarge { size: meta.len() });
  }
  let mut bytes = Vec::with_capacity(meta.len() as usize);
  fs::File::open(&path)
    .and_then(|handle| handle.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes))
    .map_err(|err| ManifestError::Read(err.to_string()))?;
  from_bytes(file, &bytes)
}

#[cfg(test)]
mod tests {
  use super::*;

  const HEADER: &str = "// @name Hello\n// @author Riff\n// @version 1.0.0\n// @description Dit bonjour\n// @permissions notify, queue\n\nfn on_ready() {}\n";

  #[test]
  fn a_full_header_is_read() {
    let manifest = parse_header(HEADER).unwrap();
    assert_eq!(manifest.name, "Hello");
    assert_eq!(manifest.author, "Riff");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.description, "Dit bonjour");
    assert_eq!(
      manifest.permissions,
      vec![Permission::Queue, Permission::Notify]
    );
  }

  #[test]
  fn missing_metadata_is_refused() {
    assert_eq!(
      parse_header("fn on_ready() {}"),
      Err(ManifestError::Missing("name"))
    );
    assert_eq!(
      parse_header("// @author Riff\n"),
      Err(ManifestError::Missing("name"))
    );
  }

  #[test]
  fn the_header_stops_at_the_first_code_line() {
    let code = "// @name A\nlet x = 1;\n// @permissions queue\n";
    assert!(parse_header(code).unwrap().permissions.is_empty());
  }

  #[test]
  fn no_permission_is_the_default() {
    assert!(parse_header("// @name A").unwrap().permissions.is_empty());
  }

  #[test]
  fn an_unknown_permission_is_refused_with_its_name() {
    assert_eq!(
      parse_header("// @name A\n// @permissions queue, files\n"),
      Err(ManifestError::UnknownPermission("files".to_string()))
    );
  }

  #[test]
  fn duplicate_permissions_are_merged() {
    let manifest = parse_header("// @name A\n// @permissions notify,notify, NOTIFY\n").unwrap();
    assert_eq!(manifest.permissions, vec![Permission::Notify]);
  }

  #[test]
  fn control_characters_are_removed_from_metadata() {
    let manifest = parse_header("// @name A\u{1b}[31mB\u{7}\n").unwrap();
    assert_eq!(manifest.name, "A[31mB");
  }

  #[test]
  fn non_ascii_metadata_is_kept() {
    assert_eq!(
      parse_header("// @name Café à l'été\n").unwrap().name,
      "Café à l'été"
    );
  }

  #[test]
  fn valid_names_are_accepted() {
    for name in ["hello.rhai", "better-shuffle.rhai", "My plugin_2.v1.rhai"] {
      assert_eq!(validate_file_name(name), Ok(()), "{name}");
    }
  }

  #[test]
  fn hostile_names_are_refused() {
    for name in [
      "",
      ".rhai",
      "hello.js",
      "hello.RHAI",
      "../hello.rhai",
      "a/b.rhai",
      "a\\b.rhai",
      "C:hello.rhai",
      "a..b.rhai",
      "café.rhai",
      "héllo.rhai",
      "plugin\u{0}.rhai",
      "CON.rhai",
      "nul.rhai",
      "com1.backup.rhai",
      " hello.rhai",
      "hello .rhai",
      ".hidden.rhai",
      &format!("{}.rhai", "a".repeat(200)),
    ] {
      assert!(
        matches!(validate_file_name(name), Err(ManifestError::InvalidName(_))),
        "{name:?}"
      );
    }
  }

  #[test]
  fn a_file_over_one_megabyte_is_refused() {
    let big = vec![b' '; MAX_SOURCE_BYTES as usize + 1];
    assert_eq!(
      from_bytes("big.rhai", &big),
      Err(ManifestError::TooLarge {
        size: MAX_SOURCE_BYTES + 1
      })
    );
  }

  #[test]
  fn invalid_utf8_is_refused() {
    assert_eq!(
      from_bytes("a.rhai", b"// @name A\n\xff\xfe"),
      Err(ManifestError::NotUtf8)
    );
  }

  #[test]
  fn the_hash_is_the_sha256_of_the_source() {
    let source = from_bytes("a.rhai", b"// @name A\n").unwrap();
    assert_eq!(source.hash.len(), 64);
    assert_eq!(
      sha256_hex(b""),
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
  }

  use crate::test_support::temp_dir;

  #[test]
  fn load_reads_a_valid_file_and_checks_the_size_first() {
    let dir = temp_dir("load");
    fs::write(dir.join("ok.rhai"), HEADER).unwrap();
    let source = load(&dir, "ok.rhai").unwrap();
    assert_eq!(source.manifest.name, "Hello");
    assert_eq!(source.hash, sha256_hex(HEADER.as_bytes()));

    let big = fs::File::create(dir.join("big.rhai")).unwrap();
    big.set_len(MAX_SOURCE_BYTES + 10).unwrap();
    assert_eq!(
      load(&dir, "big.rhai"),
      Err(ManifestError::TooLarge {
        size: MAX_SOURCE_BYTES + 10
      })
    );
    assert!(matches!(
      load(&dir, "../ok.rhai"),
      Err(ManifestError::InvalidName(_))
    ));
  }

  #[cfg(unix)]
  #[test]
  fn load_refuses_a_symlink() {
    let dir = temp_dir("link");
    fs::write(dir.join("real.rhai"), HEADER).unwrap();
    std::os::unix::fs::symlink(dir.join("real.rhai"), dir.join("link.rhai")).unwrap();
    assert!(matches!(
      load(&dir, "link.rhai"),
      Err(ManifestError::Symlink(_))
    ));
  }

  #[cfg(windows)]
  #[test]
  fn load_refuses_a_symlink() {
    let dir = temp_dir("link");
    fs::write(dir.join("real.rhai"), HEADER).unwrap();
    // Sans le mode développeur, Windows refuse de créer un lien : le test n'a alors rien à vérifier.
    if std::os::windows::fs::symlink_file(dir.join("real.rhai"), dir.join("link.rhai")).is_ok() {
      assert!(matches!(
        load(&dir, "link.rhai"),
        Err(ManifestError::Symlink(_))
      ));
    }
  }
}
