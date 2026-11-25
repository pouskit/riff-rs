//! Éditeur de thème natif (`nom`, `mode`, `source`, `overrides`) : aperçu en direct et contrôle des 13 paires
//! de contraste (>= 4,5:1) avant l'enregistrement.

use crate::app::{App, Effect, Message};
use crate::code_editor::{CodeEditor, EditorTarget};
use crate::theme::ActiveTheme;
use crate::theme::contrast::REQUIRED_RATIO;
use crate::theme::file::{self, ROLES, ThemeError};
use crate::widgets::toast::ToastKind;
use serde_json::{Map, Value, json};

/// Couleurs proposées d'un clic.
pub const PRESETS: [&str; 12] = [
  "#1DB954", "#5247D1", "#E91E63", "#F44336", "#FF9800", "#FFC107", "#8BC34A", "#009688",
  "#03A9F4", "#3F51B5", "#9C27B0", "#607D8B",
];

/// Thème en cours d'édition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeDraft {
  /// Nom du fichier, sans `.json`.
  pub id: String,
  /// Brouillon jamais enregistré : l'id est tiré du nom à l'enregistrement.
  pub is_new: bool,
  pub name: String,
  pub dark: bool,
  pub source: String,
  pub overrides: Vec<(String, String)>,
  /// Rôle choisi pour un nouvel override.
  pub role: Option<String>,
  /// Résultat du dernier contrôle : `None` si le thème est valide.
  pub problem: Option<String>,
}

/// Messages de l'éditeur de thème.
#[derive(Debug, Clone)]
pub enum ThemeEditorMessage {
  New,
  Edit(String),
  Loaded(String, Result<String, String>),
  Name(String),
  Dark(bool),
  Source(String),
  Role(String),
  AddOverride,
  OverrideColor(usize, String),
  RemoveOverride(usize),
  Save,
  Saved(String, Result<(), String>),
  Cancel,
  EditAsText,
}

/// Premier id libre : `base`, puis `base-2`, `base-3`…
#[must_use]
pub fn free_id(base: &str, taken: impl Fn(&str) -> bool) -> String {
  if !taken(base) {
    return base.to_string();
  }
  (2..)
    .map(|n| format!("{base}-{n}"))
    .find(|id| !taken(id))
    .unwrap_or_else(|| base.to_string())
}

/// Identifiant de fichier tiré du nom : lettres et chiffres ASCII, tirets.
#[must_use]
pub fn slug(name: &str) -> String {
  let mut out = String::new();
  for c in name.chars() {
    if c.is_ascii_alphanumeric() {
      out.push(c.to_ascii_lowercase());
    } else if !out.ends_with('-') && !out.is_empty() {
      out.push('-');
    }
  }
  let out = out.trim_end_matches('-').to_string();
  if out.is_empty() {
    "theme".to_string()
  } else {
    out
  }
}

/// Message clair pour un thème refusé.
#[must_use]
pub fn explain(error: &ThemeError) -> String {
  match error {
    ThemeError::Contrast(err) => format!(
      "Contrast too low: {} is {:.2}:1, it needs at least {REQUIRED_RATIO}:1. Change one of these two colors.",
      err.pair, err.ratio
    ),
    ThemeError::BadColor { field, value } => {
      format!("{field}: \"{value}\" is not a #RRGGBB color.")
    }
    other => other.to_string(),
  }
}

impl ThemeDraft {
  #[must_use]
  pub fn new(name: &str, dark: bool, source: &str) -> Self {
    Self {
      id: slug(name),
      is_new: true,
      name: name.to_string(),
      dark,
      source: source.to_string(),
      overrides: Vec::new(),
      role: None,
      problem: None,
    }
  }

  /// Lit un fichier de thème existant, overrides compris.
  ///
  /// # Errors
  /// JSON invalide.
  pub fn from_json(id: &str, raw: &str) -> Result<Self, String> {
    let file: file::ThemeFile = serde_json::from_str(raw).map_err(|err| err.to_string())?;
    let mut overrides: Vec<(String, String)> = file.overrides.into_iter().collect();
    overrides.sort();
    Ok(Self {
      id: id.to_string(),
      is_new: false,
      name: file.name,
      dark: file.mode == file::FileMode::Dark,
      source: file.source,
      overrides,
      role: None,
      problem: None,
    })
  }

  /// Fichier JSON du thème.
  #[must_use]
  pub fn to_json(&self) -> String {
    let overrides: Map<String, Value> = self
      .overrides
      .iter()
      .map(|(role, color)| (role.clone(), Value::from(color.clone())))
      .collect();
    let value = json!({
      "name": self.name,
      "mode": if self.dark { "dark" } else { "light" },
      "source": self.source,
      "overrides": overrides,
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
  }

  /// Valide le brouillon : thème prêt pour l'aperçu, ou le problème à corriger.
  pub fn check(&mut self) -> Option<file::LoadedTheme> {
    match file::parse(&self.to_json()) {
      Ok(loaded) => {
        self.problem = None;
        Some(loaded)
      }
      Err(err) => {
        self.problem = Some(explain(&err));
        None
      }
    }
  }
}

impl App {
  /// Applique le brouillon à toute l'application s'il est valide (aperçu en direct).
  fn preview_draft(&mut self) {
    let Some(draft) = &mut self.ui.theme_editor else {
      return;
    };
    if let Some(loaded) = draft.check() {
      self.theme = ActiveTheme::from_file(draft.id.clone(), loaded);
    }
  }

  /// Retour au thème enregistré.
  fn restore_theme(&mut self) -> Vec<Effect> {
    match self.settings.native_theme.clone() {
      Some(id) => vec![Effect::LoadThemeFile(id)],
      None => {
        self.theme = ActiveTheme::from_accent(&self.settings);
        self.refresh_theme();
        Vec::new()
      }
    }
  }

  /// Messages de l'éditeur de thème.
  pub fn update_theme_editor(&mut self, message: ThemeEditorMessage) -> Vec<Effect> {
    match message {
      ThemeEditorMessage::New => {
        let dark = self.theme.dark;
        self.ui.theme_editor = Some(ThemeDraft::new("My theme", dark, PRESETS[1]));
        self.preview_draft();
        Vec::new()
      }
      ThemeEditorMessage::Edit(id) => vec![Effect::ReadThemeDraft(id)],
      ThemeEditorMessage::Loaded(id, Ok(raw)) => match ThemeDraft::from_json(&id, &raw) {
        Ok(draft) => {
          self.ui.theme_editor = Some(draft);
          self.preview_draft();
          Vec::new()
        }
        Err(reason) => vec![Effect::Toast(reason, ToastKind::Error)],
      },
      ThemeEditorMessage::Loaded(_, Err(reason)) => vec![Effect::Toast(reason, ToastKind::Error)],
      ThemeEditorMessage::Cancel => {
        self.ui.theme_editor = None;
        self.restore_theme()
      }
      ThemeEditorMessage::Saved(id, Ok(())) => {
        self.ui.theme_editor = None;
        let mut effects = vec![Effect::ListSettingsThemes];
        effects.extend(self.update(Message::SelectThemeFile(id)));
        effects
      }
      ThemeEditorMessage::Saved(_, Err(reason)) => vec![Effect::Toast(reason, ToastKind::Error)],
      other => {
        let Some(draft) = &mut self.ui.theme_editor else {
          return Vec::new();
        };
        match other {
          ThemeEditorMessage::Name(name) => {
            draft.name = name;
          }
          ThemeEditorMessage::Dark(dark) => draft.dark = dark,
          ThemeEditorMessage::Source(source) => draft.source = source.trim().to_uppercase(),
          ThemeEditorMessage::Role(role) => draft.role = Some(role),
          ThemeEditorMessage::AddOverride => {
            if let Some(role) = draft.role.take()
              && !draft.overrides.iter().any(|(known, _)| *known == role)
            {
              draft.overrides.push((role, draft.source.clone()));
            }
          }
          ThemeEditorMessage::OverrideColor(index, color) => {
            if let Some(entry) = draft.overrides.get_mut(index) {
              entry.1 = color.trim().to_uppercase();
            }
          }
          ThemeEditorMessage::RemoveOverride(index) => {
            if index < draft.overrides.len() {
              draft.overrides.remove(index);
            }
          }
          ThemeEditorMessage::Save => {
            // Le contrôle de contraste bloque l'enregistrement d'un thème illisible.
            // Un nouveau thème prend l'id de son nom courant ; l'effet choisit un id libre sur disque.
            let id = if draft.is_new {
              slug(&draft.name)
            } else {
              draft.id.clone()
            };
            if draft.check().is_none() || file::validate_id(&id).is_err() {
              return Vec::new();
            }
            return vec![Effect::SaveThemeDraft(id, draft.to_json(), draft.is_new)];
          }
          ThemeEditorMessage::EditAsText => {
            // Le brouillon part dans l'éditeur de code : un thème jamais enregistré n'a pas de fichier.
            // Un nouveau thème vise un id libre d'après les thèmes listés, jamais un fichier existant.
            let id = if draft.is_new {
              let listed = &self.ui.prefs.themes;
              free_id(&slug(&draft.name), |id| listed.iter().any(|t| t.id == id))
            } else {
              draft.id.clone()
            };
            let mut editor = CodeEditor::new(EditorTarget::Theme(id), &draft.to_json());
            editor.modified = true;
            self.ui.theme_editor = None;
            self.ui.editor = Some(editor);
            return self.restore_theme();
          }
          ThemeEditorMessage::New
          | ThemeEditorMessage::Edit(_)
          | ThemeEditorMessage::Loaded(..)
          | ThemeEditorMessage::Cancel
          | ThemeEditorMessage::Saved(..) => {}
        }
        self.preview_draft();
        Vec::new()
      }
    }
  }
}

/// Rôles proposés pour un override, sans ceux déjà modifiés.
#[must_use]
pub fn free_roles(draft: &ThemeDraft) -> Vec<String> {
  ROLES
    .iter()
    .filter(|role| !draft.overrides.iter().any(|(known, _)| known == *role))
    .map(|role| (*role).to_string())
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::theme::ThemeSource;

  fn app() -> App {
    App::new(
      crate::window::PersistedWindowState::default(),
      crate::settings::NativeSettings::default(),
      crate::i18n::I18n::default(),
    )
  }

  fn editor(app: &mut App, message: ThemeEditorMessage) -> Vec<Effect> {
    app.update_theme_editor(message)
  }

  #[test]
  fn a_draft_round_trips_through_the_native_theme_file() {
    let mut draft = ThemeDraft::new("Nuit bleue", true, "#5247D1");
    draft.overrides.push(("surface".into(), "#101010".into()));
    assert_eq!(draft.id, "nuit-bleue");
    let back = ThemeDraft::from_json(&draft.id, &draft.to_json()).unwrap();
    assert_eq!(back.name, "Nuit bleue");
    assert_eq!(back.overrides, draft.overrides);
    assert!(file::parse(&draft.to_json()).is_ok());
  }

  #[test]
  fn the_preview_applies_to_the_whole_app_while_editing() {
    let mut app = app();
    editor(&mut app, ThemeEditorMessage::New);
    editor(&mut app, ThemeEditorMessage::Source("#E91E63".into()));
    assert!(matches!(&app.theme.source, ThemeSource::File(id) if id == "my-theme"));
    editor(&mut app, ThemeEditorMessage::Cancel);
    assert!(!matches!(app.theme.source, ThemeSource::File(_)));
    assert!(app.ui.theme_editor.is_none());
  }

  #[test]
  fn a_pair_under_4_5_is_explained_and_blocks_saving() {
    let mut app = app();
    editor(&mut app, ThemeEditorMessage::New);
    for (role, color) in [("primary", "#101010"), ("on_primary", "#202020")] {
      editor(&mut app, ThemeEditorMessage::Role(role.into()));
      editor(&mut app, ThemeEditorMessage::AddOverride);
      let index = app.ui.theme_editor.as_ref().unwrap().overrides.len() - 1;
      editor(
        &mut app,
        ThemeEditorMessage::OverrideColor(index, color.into()),
      );
    }
    let problem = app
      .ui
      .theme_editor
      .as_ref()
      .unwrap()
      .problem
      .clone()
      .unwrap();
    assert!(problem.contains("on_primary/primary"), "{problem}");
    assert!(problem.contains("4.5"), "{problem}");
    assert!(editor(&mut app, ThemeEditorMessage::Save).is_empty());
    editor(&mut app, ThemeEditorMessage::RemoveOverride(0));
    editor(&mut app, ThemeEditorMessage::RemoveOverride(0));
    assert!(app.ui.theme_editor.as_ref().unwrap().problem.is_none());
    let effects = editor(&mut app, ThemeEditorMessage::Save);
    assert!(matches!(&effects[0], Effect::SaveThemeDraft(id, _, true) if id == "my-theme"));
  }

  #[test]
  fn a_bad_color_is_explained() {
    let mut draft = ThemeDraft::new("A", true, "rouge");
    assert!(draft.check().is_none());
    assert!(draft.problem.unwrap().contains("#RRGGBB"));
  }

  #[test]
  fn saving_selects_the_theme_and_text_mode_opens_the_code_editor() {
    let mut app = app();
    editor(&mut app, ThemeEditorMessage::New);
    editor(&mut app, ThemeEditorMessage::Source("#E91E63".into()));
    let effects = editor(&mut app, ThemeEditorMessage::EditAsText);
    assert!(
      !effects
        .iter()
        .any(|e| matches!(e, Effect::OpenEditorFile(_)))
    );
    assert!(app.ui.theme_editor.is_none());
    let opened = app.ui.editor.as_ref().unwrap();
    assert_eq!(opened.target, EditorTarget::Theme("my-theme".into()));
    assert!(opened.modified);
    assert!(opened.content.0.text().contains("#E91E63"));
    let effects = editor(&mut app, ThemeEditorMessage::Saved("nuit".into(), Ok(())));
    assert!(effects.contains(&Effect::LoadThemeFile("nuit".into())));
  }

  fn saved_id(app: &mut App) -> String {
    match editor(app, ThemeEditorMessage::Save).first() {
      Some(Effect::SaveThemeDraft(id, _, _)) => id.clone(),
      other => panic!("pas d'enregistrement : {other:?}"),
    }
  }

  #[test]
  fn a_renamed_new_theme_takes_the_id_of_its_name() {
    let mut app = app();
    editor(&mut app, ThemeEditorMessage::New);
    editor(&mut app, ThemeEditorMessage::Name("Nuit bleue".into()));
    assert_eq!(saved_id(&mut app), "nuit-bleue");
    let effects = editor(&mut app, ThemeEditorMessage::Save);
    assert!(matches!(&effects[0], Effect::SaveThemeDraft(_, _, true)));
  }

  #[test]
  fn free_ids_skip_taken_ones() {
    let taken = ["my-theme", "my-theme-2"];
    assert_eq!(free_id("autre", |id| taken.contains(&id)), "autre");
    assert_eq!(free_id("my-theme", |id| taken.contains(&id)), "my-theme-3");
    assert_eq!(free_id("my-theme", |id| id == "my-theme"), "my-theme-2");
  }

  #[test]
  fn text_mode_of_a_new_theme_avoids_listed_ids() {
    let mut app = app();
    app.ui.prefs.themes = vec![file::ThemeEntry {
      id: "my-theme".into(),
      name: "My theme".into(),
      status: file::ThemeStatus::Incompatible,
    }];
    editor(&mut app, ThemeEditorMessage::New);
    editor(&mut app, ThemeEditorMessage::EditAsText);
    let opened = app.ui.editor.as_ref().unwrap();
    assert_eq!(opened.target, EditorTarget::Theme("my-theme-2".into()));
  }

  #[test]
  fn an_edited_theme_keeps_its_id_when_renamed() {
    let mut app = app();
    let raw = ThemeDraft::new("Nuit", true, "#5247D1").to_json();
    editor(&mut app, ThemeEditorMessage::Loaded("nuit".into(), Ok(raw)));
    editor(&mut app, ThemeEditorMessage::Name("Jour".into()));
    let effects = editor(&mut app, ThemeEditorMessage::Save);
    assert!(matches!(&effects[0], Effect::SaveThemeDraft(id, _, false) if id == "nuit"));
  }

  #[test]
  fn slugs_stay_inside_the_folder() {
    assert_eq!(slug("../../Été 2026!"), "t-2026");
    assert_eq!(slug("***"), "theme");
    assert!(file::validate_id(&slug("a/b\\c..d")).is_ok());
  }
}
