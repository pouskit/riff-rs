//! Fichiers de thème natifs : `<dossier thèmes>/native/*.json`.

use crate::theme::contrast::{self, ContrastError};
use crate::theme::scheme;
use material_colors::color::Rgb;
use material_colors::scheme::Scheme;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Sous-dossier des thèmes natifs dans le dossier des thèmes.
pub const NATIVE_DIR: &str = "native";

/// Erreurs d'un fichier de thème.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ThemeError {
  #[error("JSON invalide : {0}")]
  Json(String),
  #[error("couleur mal formée pour {field} : {value}")]
  BadColor { field: String, value: String },
  #[error("rôle M3 inconnu : {0}")]
  UnknownRole(String),
  #[error(transparent)]
  Contrast(#[from] ContrastError),
}

/// Mode d'un thème.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileMode {
  Dark,
  Light,
}

/// Contenu d'un fichier de thème.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ThemeFile {
  pub name: String,
  pub mode: FileMode,
  pub source: String,
  #[serde(default)]
  pub overrides: HashMap<String, String>,
}

/// Thème de fichier validé.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedTheme {
  pub name: String,
  pub dark: bool,
  pub scheme: Scheme,
}

/// Lit et valide un thème : JSON, couleurs, rôles, puis contraste des 13 paires.
///
/// # Errors
/// Voir [`ThemeError`].
pub fn parse(raw: &str) -> Result<LoadedTheme, ThemeError> {
  let file: ThemeFile =
    serde_json::from_str(raw).map_err(|err| ThemeError::Json(err.to_string()))?;
  let dark = file.mode == FileMode::Dark;
  let mut scheme = scheme::from_source(parse_color("source", &file.source)?, dark);
  for (role, value) in &file.overrides {
    let slot = role_mut(&mut scheme, role).ok_or_else(|| ThemeError::UnknownRole(role.clone()))?;
    *slot = parse_color(role, value)?;
  }
  contrast::check(&scheme)?;
  Ok(LoadedTheme {
    name: file.name,
    dark,
    scheme,
  })
}

/// Couleur `#RRGGBB` stricte.
fn parse_color(field: &str, value: &str) -> Result<Rgb, ThemeError> {
  let bad = || ThemeError::BadColor {
    field: field.to_string(),
    value: value.to_string(),
  };
  let hex = value
    .strip_prefix('#')
    .filter(|hex| hex.len() == 6)
    .ok_or_else(bad)?;
  hex.parse().map_err(|_| bad())
}

macro_rules! roles {
  ($($role:ident),+ $(,)?) => {
    /// Rôles M3 modifiables par `overrides`.
    pub const ROLES: &[&str] = &[$(stringify!($role)),+];

    /// Rôle M3 d'après son nom, `on_primary` ou `on-primary`.
    fn role_mut<'a>(scheme: &'a mut Scheme, role: &str) -> Option<&'a mut Rgb> {
      match role.replace('-', "_").as_str() {
        $(stringify!($role) => Some(&mut scheme.$role),)+
        _ => None,
      }
    }
  };
}

roles!(
  primary,
  on_primary,
  primary_container,
  on_primary_container,
  inverse_primary,
  primary_fixed,
  primary_fixed_dim,
  on_primary_fixed,
  on_primary_fixed_variant,
  secondary,
  on_secondary,
  secondary_container,
  on_secondary_container,
  secondary_fixed,
  secondary_fixed_dim,
  on_secondary_fixed,
  on_secondary_fixed_variant,
  tertiary,
  on_tertiary,
  tertiary_container,
  on_tertiary_container,
  tertiary_fixed,
  tertiary_fixed_dim,
  on_tertiary_fixed,
  on_tertiary_fixed_variant,
  error,
  on_error,
  error_container,
  on_error_container,
  surface_dim,
  surface,
  surface_tint,
  surface_bright,
  surface_container_lowest,
  surface_container_low,
  surface_container,
  surface_container_high,
  surface_container_highest,
  on_surface,
  on_surface_variant,
  outline,
  outline_variant,
  inverse_surface,
  inverse_on_surface,
  surface_variant,
  background,
  on_background,
  shadow,
  scrim,
);

/// Taille maximale d'un fichier de thème, vérifiée avant lecture.
pub const MAX_THEME_BYTES: u64 = 64 * 1024;

/// Identifiant de thème : ne sort jamais du dossier.
///
/// # Errors
/// Identifiant vide ou avec un séparateur ou `..`.
pub fn validate_id(id: &str) -> Result<(), String> {
  if id.is_empty() || id.contains(['/', '\\', ':']) || id.contains("..") {
    return Err(format!("identifiant de thème invalide : {id}"));
  }
  Ok(())
}

/// Lit un fichier de thème après avoir vérifié sa taille.
fn read_capped(path: &Path) -> Result<String, String> {
  let size = fs::metadata(path).map_err(|err| err.to_string())?.len();
  if size > MAX_THEME_BYTES {
    return Err(format!(
      "fichier de thème trop gros : {size} octets (maximum {MAX_THEME_BYTES})"
    ));
  }
  fs::read_to_string(path).map_err(|err| err.to_string())
}

/// Lit le thème natif d'identifiant `id`. L'identifiant vient des réglages : il ne sort jamais du dossier.
///
/// # Errors
/// Identifiant invalide, fichier illisible ou thème refusé, avec la raison.
pub fn load(themes_dir: &Path, id: &str) -> Result<LoadedTheme, String> {
  validate_id(id)?;
  let raw = read_capped(&themes_dir.join(NATIVE_DIR).join(format!("{id}.json")))?;
  parse(&raw).map_err(|err| err.to_string())
}

/// État d'un thème de la liste.
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeStatus {
  Valid(Box<LoadedTheme>),
  Refused(String),
  /// Ancien thème CSS de Riff : pas importable.
  Incompatible,
}

/// Entrée de la liste des thèmes.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeEntry {
  pub id: String,
  pub name: String,
  pub status: ThemeStatus,
}

/// Liste les thèmes natifs de `themes_dir/native`, et les thèmes CSS donnés comme incompatibles.
#[must_use]
pub fn list_themes(themes_dir: &Path, css_names: &[String]) -> Vec<ThemeEntry> {
  let mut paths: Vec<PathBuf> = fs::read_dir(themes_dir.join(NATIVE_DIR))
    .map(|dir| dir.flatten().map(|entry| entry.path()).collect())
    .unwrap_or_default();
  paths.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
  paths.sort();
  let native = paths.iter().filter_map(|path| {
    let id = path.file_stem()?.to_string_lossy().into_owned();
    let parsed = read_capped(path).and_then(|raw| parse(&raw).map_err(|err| err.to_string()));
    Some(match parsed {
      Ok(theme) => ThemeEntry {
        id,
        name: theme.name.clone(),
        status: ThemeStatus::Valid(Box::new(theme)),
      },
      Err(reason) => ThemeEntry {
        name: id.clone(),
        id,
        status: ThemeStatus::Refused(reason),
      },
    })
  });
  let css = css_names.iter().map(|name| ThemeEntry {
    id: name.clone(),
    name: name.clone(),
    status: ThemeStatus::Incompatible,
  });
  native.chain(css).collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  const VALID: &str = r##"{ "name": "Nuit", "mode": "dark", "source": "#5247D1" }"##;

  #[test]
  fn a_valid_file_gives_the_scheme_of_its_source() {
    let theme = parse(VALID).unwrap();
    assert_eq!(theme.name, "Nuit");
    assert!(theme.dark);
    assert_eq!(
      theme.scheme,
      scheme::from_source(Rgb::new(0x52, 0x47, 0xD1), true)
    );
  }

  #[test]
  fn light_mode_uses_the_light_scheme() {
    let theme = parse(r##"{ "name": "Jour", "mode": "light", "source": "#5247D1" }"##).unwrap();
    assert!(!theme.dark);
    assert_eq!(
      theme.scheme,
      scheme::from_source(Rgb::new(0x52, 0x47, 0xD1), false)
    );
  }

  #[test]
  fn overrides_replace_a_role_in_both_spellings() {
    let raw = r##"{ "name": "A", "mode": "dark", "source": "#5247D1",
      "overrides": { "surface": "#101010", "on-surface": "#FAFAFA" } }"##;
    let theme = parse(raw).unwrap();
    assert_eq!(theme.scheme.surface, Rgb::new(0x10, 0x10, 0x10));
    assert_eq!(theme.scheme.on_surface, Rgb::new(0xFA, 0xFA, 0xFA));
  }

  #[test]
  fn invalid_json_is_refused() {
    assert!(matches!(parse("{ pas du json"), Err(ThemeError::Json(_))));
  }

  #[test]
  fn an_unknown_mode_is_refused() {
    assert!(matches!(
      parse(r##"{ "name": "A", "mode": "gris", "source": "#5247D1" }"##),
      Err(ThemeError::Json(_))
    ));
  }

  #[test]
  fn malformed_colors_are_refused() {
    for source in [
      "5247D1",
      "#5247D",
      "#FFF",
      "#GGGGGG",
      "#5247D1FF",
      "bleu",
      "#ÉÉÉ",
    ] {
      let raw = format!(r##"{{ "name": "A", "mode": "dark", "source": "{source}" }}"##);
      assert_eq!(
        parse(&raw),
        Err(ThemeError::BadColor {
          field: "source".to_string(),
          value: source.to_string()
        }),
        "{source}"
      );
    }
    let raw = r##"{ "name": "A", "mode": "dark", "source": "#5247D1", "overrides": { "primary": "rouge" } }"##;
    assert_eq!(
      parse(raw),
      Err(ThemeError::BadColor {
        field: "primary".to_string(),
        value: "rouge".to_string()
      })
    );
  }

  #[test]
  fn an_unknown_role_is_refused() {
    let raw = r##"{ "name": "A", "mode": "dark", "source": "#5247D1", "overrides": { "rouge": "#FF0000" } }"##;
    assert_eq!(
      parse(raw),
      Err(ThemeError::UnknownRole("rouge".to_string()))
    );
  }

  #[test]
  fn a_pair_under_the_contrast_is_refused_with_its_name() {
    let raw = r##"{ "name": "A", "mode": "dark", "source": "#5247D1",
      "overrides": { "primary": "#101010", "on_primary": "#202020" } }"##;
    let Err(ThemeError::Contrast(err)) = parse(raw) else {
      panic!("attendu : contraste")
    };
    assert_eq!(err.pair, "on_primary/primary");
    assert!(err.ratio < 4.5);
  }

  #[test]
  fn the_list_gives_valid_refused_and_incompatible_themes() {
    let dir = crate::test_temp::TempDir::new("app-themes");
    let native = dir.join(NATIVE_DIR);
    std::fs::create_dir_all(&native).unwrap();
    std::fs::write(native.join("nuit.json"), VALID).unwrap();
    std::fs::write(native.join("casse.json"), "{").unwrap();
    std::fs::write(native.join("ignore.txt"), "pas un thème").unwrap();

    let entries = list_themes(&dir, &["Ancien".to_string()]);
    assert_eq!(entries.len(), 3);
    assert!(
      matches!(entries[0].status, ThemeStatus::Refused(_)),
      "{:?}",
      entries[0]
    );
    assert_eq!(entries[0].id, "casse");
    assert!(
      matches!(entries[1].status, ThemeStatus::Valid(_)),
      "{:?}",
      entries[1]
    );
    assert_eq!(entries[1].name, "Nuit");
    assert_eq!(entries[2].name, "Ancien");
    assert_eq!(entries[2].status, ThemeStatus::Incompatible);
  }

  #[test]
  fn load_refuses_an_identifier_that_leaves_the_folder() {
    let dir = std::env::temp_dir();
    for id in ["../secret", "a/b", "a\\b", ".."] {
      assert!(
        load(&dir, id).unwrap_err().starts_with("identifiant"),
        "{id}"
      );
    }
  }

  #[test]
  fn a_theme_file_over_64_kb_is_refused_before_reading() {
    let dir = crate::test_temp::TempDir::new("app-big-theme");
    let native = dir.join(NATIVE_DIR);
    std::fs::create_dir_all(&native).unwrap();
    let file = std::fs::File::create(native.join("gros.json")).unwrap();
    file.set_len(MAX_THEME_BYTES + 1).unwrap();
    drop(file);
    assert!(load(&dir, "gros").unwrap_err().contains("trop gros"));
    let entries = list_themes(&dir, &[]);
    assert!(
      matches!(&entries[0].status, ThemeStatus::Refused(reason) if reason.contains("trop gros"))
    );
  }

  #[test]
  fn a_missing_folder_gives_an_empty_list() {
    assert_eq!(
      list_themes(&std::env::temp_dir().join("riff-app-sans-themes"), &[]),
      Vec::new()
    );
  }
}
