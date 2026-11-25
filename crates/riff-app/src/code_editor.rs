//! Éditeur de code (remplace Monaco) : plugins `.rhai` et thèmes natifs `.json`.
//! Recherche, remplacement, enregistrement (Ctrl+S), avertissement avant de fermer un fichier modifié.

use crate::app::{App, Effect, Message};
use crate::lyrics::state::EditorContent;
use crate::widgets::toast::ToastKind;
use iced::widget::text_editor::{self, Action, Cursor, Position};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Plus gros fichier enregistré : le plafond d'un plugin (1 Mo).
pub const MAX_EDITOR_BYTES: u64 = riff_plugins::manifest::MAX_SOURCE_BYTES;
/// Plus gros fichier ouvert dans l'éditeur : au-delà, l'ouverture bloquerait l'interface plus de 100 ms
/// (mesure de `tests/editor_open.rs` en release). Un fichier plus gros reste exécutable, pas éditable ici.
pub const MAX_EDITOR_OPEN_BYTES: u64 = 128 * 1024;
/// Modèle d'un nouveau plugin.
pub const PLUGIN_TEMPLATE: &str = "// @name My plugin\n// @author\n// @version 1.0.0\n// @description\n// @permissions notify\n\nfn on_trackchange(track) {\n  if track != () {\n    notify(\"Now playing: \" + track.title);\n  }\n}\n";

/// Fichier ouvert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorTarget {
  /// Fichier `.rhai` du dossier des plugins.
  Plugin(String),
  /// Thème natif `<dossier des thèmes>/native/<id>.json`.
  Theme(String),
}

impl EditorTarget {
  /// Chemin du fichier : le nom est validé, il ne sort jamais de son dossier.
  ///
  /// # Errors
  /// Nom refusé.
  pub fn path(&self) -> Result<PathBuf, String> {
    match self {
      Self::Plugin(file) => {
        riff_plugins::manifest::validate_file_name(file).map_err(|err| err.to_string())?;
        Ok(crate::plugins::plugins_dir().join(file))
      }
      Self::Theme(id) => {
        crate::theme::file::validate_id(id)?;
        Ok(
          riff_core::paths::themes_dir()
            .join(crate::theme::file::NATIVE_DIR)
            .join(format!("{id}.json")),
        )
      }
    }
  }

  /// Nom affiché.
  #[must_use]
  pub fn label(&self) -> String {
    match self {
      Self::Plugin(file) => file.clone(),
      Self::Theme(id) => format!("{id}.json"),
    }
  }

  /// Jeton de coloration : Rhai n'a pas de syntaxe dans le jeu de `two-face`, Rust est la plus proche.
  #[must_use]
  pub const fn syntax(&self) -> &'static str {
    match self {
      Self::Plugin(_) => "rs",
      Self::Theme(_) => "json",
    }
  }
}

/// État de l'éditeur ouvert.
#[derive(Debug, Clone)]
pub struct CodeEditor {
  pub target: EditorTarget,
  pub content: EditorContent,
  pub modified: bool,
  pub find: String,
  pub replace: String,
  pub case_sensitive: bool,
  pub find_open: bool,
  /// Fermeture demandée avec des modifications : avertissement affiché.
  pub confirm_close: bool,
  /// Dernier message : enregistré, erreur de syntaxe, introuvable.
  pub status: Option<String>,
}

/// Messages de l'éditeur.
#[derive(Debug, Clone)]
pub enum EditorMessage {
  Open(EditorTarget),
  /// Nouveau plugin depuis le modèle.
  NewPlugin,
  Opened(EditorTarget, Result<String, String>),
  Action(Action),
  ToggleFind,
  Find(String),
  Replace(String),
  ToggleCase,
  Next,
  Previous,
  ReplaceOne,
  ReplaceAll,
  Save,
  Saved(EditorTarget, Result<Option<String>, String>),
  Close,
  CloseAnyway,
  KeepEditing,
}

/// Lit un fichier à ouvrir : taille vérifiée avant lecture, plafond [`MAX_EDITOR_OPEN_BYTES`].
///
/// # Errors
/// Fichier absent, trop gros ou pas en UTF-8.
pub fn read_capped(path: &Path) -> Result<String, String> {
  let meta = fs::symlink_metadata(path).map_err(|err| err.to_string())?;
  if meta.file_type().is_symlink() {
    return Err("lien symbolique refusé".to_string());
  }
  if meta.len() > MAX_EDITOR_OPEN_BYTES {
    return Err(format!(
      "fichier trop gros pour l'éditeur : {} octets (maximum {MAX_EDITOR_OPEN_BYTES})",
      meta.len()
    ));
  }
  fs::read_to_string(path).map_err(|err| err.to_string())
}

/// Écriture atomique, plafonnée.
///
/// # Errors
/// Texte trop gros ou écriture impossible.
pub fn write_capped(path: &Path, text: &str) -> Result<(), String> {
  if text.len() as u64 > MAX_EDITOR_BYTES {
    return Err(format!(
      "fichier trop gros : {} octets (maximum {MAX_EDITOR_BYTES})",
      text.len()
    ));
  }
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
  }
  let tmp = path.with_extension("tmp");
  let mut file = fs::File::create(&tmp).map_err(|err| err.to_string())?;
  file
    .write_all(text.as_bytes())
    .and_then(|()| file.sync_all())
    .map_err(|err| err.to_string())?;
  drop(file);
  fs::rename(&tmp, path).map_err(|err| err.to_string())
}

/// Écrit un nouveau thème sous `base`, ou `base-2`, `base-3`… : le premier fichier absent.
///
/// La création exclusive évite d'écraser un thème existant, même créé entre-temps.
///
/// # Errors
/// Id invalide, texte trop gros ou écriture impossible.
pub fn write_new_theme(base: &str, text: &str) -> Result<String, String> {
  if text.len() as u64 > MAX_EDITOR_BYTES {
    return Err(format!(
      "fichier trop gros : {} octets (maximum {MAX_EDITOR_BYTES})",
      text.len()
    ));
  }
  for n in 1..=10_000 {
    let id = if n == 1 {
      base.to_string()
    } else {
      format!("{base}-{n}")
    };
    let path = EditorTarget::Theme(id.clone()).path()?;
    if let Some(dir) = path.parent() {
      fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    match fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&path)
    {
      Ok(mut file) => {
        let written = file
          .write_all(text.as_bytes())
          .and_then(|()| file.sync_all());
        return match written {
          Ok(()) => Ok(id),
          Err(err) => {
            drop(file);
            let _ = fs::remove_file(&path);
            Err(err.to_string())
          }
        };
      }
      Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
      Err(err) => return Err(err.to_string()),
    }
  }
  Err(format!("aucun identifiant libre pour {base}"))
}

/// Vérifie le texte enregistré : `None` s'il est valide, sinon le problème (syntaxe, contraste).
#[must_use]
pub fn check(target: &EditorTarget, text: &str) -> Option<String> {
  match target {
    EditorTarget::Plugin(file) => {
      if let Err(err) = riff_plugins::manifest::from_bytes(file, text.as_bytes()) {
        return Some(err.to_string());
      }
      riff_plugins::compile(text).err().map(|err| err.to_string())
    }
    EditorTarget::Theme(_) => crate::theme::file::parse(text)
      .err()
      .map(|err| err.to_string()),
  }
}

/// Positions (ligne, octet de début, octet de fin) des occurrences de `needle` dans les lignes.
#[must_use]
pub fn find_all(
  lines: &[String],
  needle: &str,
  case_sensitive: bool,
) -> Vec<(usize, usize, usize)> {
  if needle.is_empty() {
    return Vec::new();
  }
  let needle_cmp = if case_sensitive {
    needle.to_string()
  } else {
    needle.to_lowercase()
  };
  let mut found = Vec::new();
  for (index, line) in lines.iter().enumerate() {
    // Comparaison insensible à la casse : seulement si la mise en minuscules garde les longueurs.
    let haystack = if case_sensitive {
      line.clone()
    } else {
      let lower = line.to_lowercase();
      if lower.len() != line.len() {
        continue;
      }
      lower
    };
    let mut start = 0;
    while let Some(offset) = haystack[start..].find(&needle_cmp) {
      let begin = start + offset;
      found.push((index, begin, begin + needle_cmp.len()));
      start = begin + needle_cmp.len().max(1);
    }
  }
  found
}

impl CodeEditor {
  #[must_use]
  pub fn new(target: EditorTarget, text: &str) -> Self {
    Self {
      target,
      content: EditorContent(text_editor::Content::with_text(text)),
      modified: false,
      find: String::new(),
      replace: String::new(),
      case_sensitive: false,
      find_open: false,
      confirm_close: false,
      status: None,
    }
  }

  fn lines(&self) -> Vec<String> {
    self
      .content
      .0
      .lines()
      .map(|line| line.text.into_owned())
      .collect()
  }

  /// Ligne et colonne du curseur, à partir de 1, colonne en caractères.
  #[must_use]
  pub fn cursor_label(&self) -> (usize, usize) {
    let position = self.content.0.cursor().position;
    let column = self
      .content
      .0
      .line(position.line)
      .map_or(position.column, |line| {
        line
          .text
          .get(..position.column)
          .map_or(position.column, |before| before.chars().count())
      });
    (position.line + 1, column + 1)
  }

  fn select(&mut self, (line, start, end): (usize, usize, usize)) {
    self.content.0.move_to(Cursor {
      position: Position { line, column: end },
      selection: Some(Position {
        line,
        column: start,
      }),
    });
  }

  /// Occurrence suivante (ou précédente) depuis le curseur, en boucle.
  pub fn find_step(&mut self, forward: bool) {
    let matches = find_all(&self.lines(), &self.find, self.case_sensitive);
    if matches.is_empty() {
      self.status = Some("Not found".to_string());
      return;
    }
    let cursor = self.content.0.cursor();
    let here = cursor.position;
    let anchor = cursor.selection.unwrap_or(here);
    let (from_line, from_col) = if forward {
      (here.line, here.column.max(anchor.column))
    } else {
      (here.line, here.column.min(anchor.column))
    };
    let pick = if forward {
      matches
        .iter()
        .find(|(line, start, _)| (*line, *start) >= (from_line, from_col))
        .or_else(|| matches.first())
    } else {
      matches
        .iter()
        .rev()
        .find(|(line, start, _)| (*line, *start) < (from_line, from_col))
        .or_else(|| matches.last())
    };
    if let Some(found) = pick.copied() {
      self.select(found);
      self.status = Some(format!("{} matches", matches.len()));
    }
  }

  /// Remplace la sélection si elle correspond à la recherche, puis passe à la suivante.
  pub fn replace_one(&mut self) {
    let selected = self.content.0.selection().unwrap_or_default();
    let matches = if self.case_sensitive {
      selected == self.find
    } else {
      selected.to_lowercase() == self.find.to_lowercase()
    };
    if !self.find.is_empty() && matches {
      self
        .content
        .0
        .perform(Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(
          self.replace.clone(),
        ))));
      self.modified = true;
    }
    self.find_step(true);
  }

  /// Remplace toutes les occurrences. Renvoie leur nombre.
  pub fn replace_all(&mut self) -> usize {
    let matches = find_all(&self.lines(), &self.find, self.case_sensitive);
    if matches.is_empty() {
      self.status = Some("Not found".to_string());
      return 0;
    }
    let mut lines = self.lines();
    for (line, start, end) in matches.iter().rev() {
      lines[*line].replace_range(*start..*end, &self.replace);
    }
    let cursor = self.content.0.cursor();
    self.content = EditorContent(text_editor::Content::with_text(&lines.join("\n")));
    self.content.0.move_to(Cursor {
      position: cursor.position,
      selection: None,
    });
    self.modified = true;
    self.status = Some(format!("{} replaced", matches.len()));
    matches.len()
  }
}

impl App {
  /// Messages de l'éditeur de code.
  pub fn update_editor(&mut self, message: EditorMessage) -> Vec<Effect> {
    match message {
      EditorMessage::Open(target) => vec![Effect::OpenEditorFile(target)],
      EditorMessage::NewPlugin => {
        let file = (1..)
          .map(|n| format!("plugin-{n}.rhai"))
          .find(|file| !self.plugins.entries.iter().any(|entry| entry.file == *file))
          .unwrap_or_else(|| "plugin.rhai".to_string());
        let mut editor = CodeEditor::new(EditorTarget::Plugin(file), PLUGIN_TEMPLATE);
        editor.modified = true;
        self.ui.editor = Some(editor);
        Vec::new()
      }
      EditorMessage::Opened(target, Ok(text)) => {
        self.ui.editor = Some(CodeEditor::new(target, &text));
        Vec::new()
      }
      EditorMessage::Opened(_, Err(reason)) => vec![Effect::Toast(reason, ToastKind::Error)],
      EditorMessage::Close => {
        let Some(editor) = &mut self.ui.editor else {
          return Vec::new();
        };
        if editor.modified {
          editor.confirm_close = true;
        } else {
          self.ui.editor = None;
        }
        Vec::new()
      }
      EditorMessage::CloseAnyway => {
        self.ui.editor = None;
        Vec::new()
      }
      EditorMessage::Saved(target, result) => self.editor_saved(target, result),
      other => {
        let Some(editor) = &mut self.ui.editor else {
          return Vec::new();
        };
        match other {
          EditorMessage::Action(action) => {
            if action.is_edit() {
              editor.modified = true;
            }
            editor.content.0.perform(action);
          }
          EditorMessage::ToggleFind => editor.find_open = !editor.find_open,
          EditorMessage::Find(text) => editor.find = text,
          EditorMessage::Replace(text) => editor.replace = text,
          EditorMessage::ToggleCase => editor.case_sensitive = !editor.case_sensitive,
          EditorMessage::Next => editor.find_step(true),
          EditorMessage::Previous => editor.find_step(false),
          EditorMessage::ReplaceOne => editor.replace_one(),
          EditorMessage::ReplaceAll => {
            editor.replace_all();
          }
          EditorMessage::KeepEditing => editor.confirm_close = false,
          EditorMessage::Save => {
            return vec![Effect::SaveEditorFile(
              editor.target.clone(),
              editor.content.0.text(),
            )];
          }
          EditorMessage::Open(_)
          | EditorMessage::NewPlugin
          | EditorMessage::Opened(..)
          | EditorMessage::Close
          | EditorMessage::CloseAnyway
          | EditorMessage::Saved(..) => {}
        }
        Vec::new()
      }
    }
  }

  fn editor_saved(
    &mut self,
    target: EditorTarget,
    result: Result<Option<String>, String>,
  ) -> Vec<Effect> {
    let problem = match result {
      Err(reason) => return vec![Effect::Toast(reason, ToastKind::Error)],
      Ok(problem) => problem,
    };
    if let Some(editor) = &mut self.ui.editor
      && editor.target == target
    {
      editor.modified = false;
      editor.confirm_close = false;
      editor.status = Some(problem.clone().unwrap_or_else(|| "Saved".to_string()));
    }
    let mut effects = Vec::new();
    match &target {
      // Le code actif n'est plus celui du fichier : le plugin s'arrête ; son consentement (lié au
      // hachage) sera redonné à la réactivation, après lecture des permissions.
      EditorTarget::Plugin(file) => {
        if self.plugins.names.remove(file).is_some() {
          effects.push(Effect::DeactivatePlugin(file.clone()));
        }
        effects.push(Effect::ScanPlugins);
      }
      EditorTarget::Theme(id) => {
        effects.push(Effect::ListSettingsThemes);
        if problem.is_none() && self.settings.native_theme.as_deref() == Some(id.as_str()) {
          effects.extend(self.update(Message::SelectThemeFile(id.clone())));
        }
      }
    }
    if let Some(problem) = problem {
      effects.push(Effect::Toast(problem, ToastKind::Warning));
    }
    effects
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
  }

  #[test]
  fn find_all_respects_the_case_option() {
    let text = lines("let Track = track;\nTRACK");
    assert_eq!(find_all(&text, "track", true), vec![(0, 12, 17)]);
    assert_eq!(
      find_all(&text, "track", false),
      vec![(0, 4, 9), (0, 12, 17), (1, 0, 5)]
    );
    assert!(find_all(&text, "", false).is_empty());
  }

  #[test]
  fn next_and_previous_select_matches_in_a_loop() {
    let mut editor = CodeEditor::new(EditorTarget::Plugin("a.rhai".into()), "ab\nxab\nab");
    editor.find = "ab".into();
    editor.find_step(true);
    assert_eq!(editor.content.0.selection().as_deref(), Some("ab"));
    assert_eq!(editor.cursor_label(), (1, 3));
    editor.find_step(true);
    assert_eq!(editor.cursor_label(), (2, 4));
    editor.find_step(true);
    assert_eq!(editor.cursor_label(), (3, 3));
    editor.find_step(true);
    assert_eq!(editor.cursor_label(), (1, 3), "retour au début");
    editor.find_step(false);
    assert_eq!(editor.cursor_label(), (3, 3));
  }

  #[test]
  fn replace_one_and_replace_all_mark_the_file_modified() {
    let mut editor = CodeEditor::new(EditorTarget::Plugin("a.rhai".into()), "one two one\nOne");
    editor.find = "one".into();
    editor.replace = "1".into();
    editor.find_step(true);
    editor.replace_one();
    assert!(editor.modified);
    assert_eq!(editor.content.0.text(), "1 two one\nOne");
    assert_eq!(editor.replace_all(), 2);
    assert_eq!(editor.content.0.text(), "1 two 1\n1");
  }

  #[test]
  fn the_column_counts_characters_not_bytes() {
    let mut editor = CodeEditor::new(EditorTarget::Plugin("a.rhai".into()), "é é x");
    editor.find = "x".into();
    editor.find_step(true);
    assert_eq!(editor.cursor_label(), (1, 6));
  }

  #[test]
  fn plugin_and_theme_files_are_checked_on_save() {
    let plugin = EditorTarget::Plugin("a.rhai".into());
    assert_eq!(check(&plugin, "// @name A\nfn on_ready() {}\n"), None);
    let err = check(&plugin, "// @name A\nfn on_ready( {\n").unwrap();
    assert!(err.contains("ligne 2"), "{err}");
    assert!(check(&plugin, "fn x() {}").unwrap().contains("@name"));
    let theme = EditorTarget::Theme("nuit".into());
    assert_eq!(
      check(
        &theme,
        r##"{ "name": "Nuit", "mode": "dark", "source": "#5247D1" }"##
      ),
      None
    );
    assert!(check(&theme, "{").is_some());
  }

  #[test]
  fn targets_never_leave_their_folder() {
    assert!(EditorTarget::Plugin("../x.rhai".into()).path().is_err());
    assert!(EditorTarget::Plugin("x.js".into()).path().is_err());
    assert!(EditorTarget::Theme("../x".into()).path().is_err());
    assert!(EditorTarget::Plugin("ok.rhai".into()).path().is_ok());
  }

  #[test]
  fn read_and_write_refuse_files_over_one_megabyte() {
    let dir = crate::test_temp::TempDir::new("app-editor");
    let path = dir.join("big.rhai");
    let big = fs::File::create(&path).unwrap();
    big.set_len(MAX_EDITOR_BYTES + 1).unwrap();
    drop(big);
    assert!(read_capped(&path).unwrap_err().contains("trop gros"));
    let text = "x".repeat(MAX_EDITOR_BYTES as usize + 1);
    assert!(
      write_capped(&dir.join("w.rhai"), &text)
        .unwrap_err()
        .contains("trop gros")
    );
    write_capped(&dir.join("w.rhai"), "ok").unwrap();
    assert_eq!(read_capped(&dir.join("w.rhai")).unwrap(), "ok");
  }

  fn app() -> App {
    App::new(
      crate::window::PersistedWindowState::default(),
      crate::settings::NativeSettings::default(),
      crate::i18n::I18n::default(),
    )
  }

  #[test]
  fn closing_a_modified_file_asks_first() {
    let mut app = app();
    app.update_editor(EditorMessage::Opened(
      EditorTarget::Plugin("a.rhai".into()),
      Ok("x".into()),
    ));
    app.update_editor(EditorMessage::Action(Action::Edit(
      text_editor::Edit::Insert('y'),
    )));
    assert!(app.ui.editor.as_ref().unwrap().modified);
    app.update_editor(EditorMessage::Close);
    assert!(app.ui.editor.as_ref().unwrap().confirm_close);
    app.update_editor(EditorMessage::KeepEditing);
    assert!(!app.ui.editor.as_ref().unwrap().confirm_close);
    app.update_editor(EditorMessage::CloseAnyway);
    assert!(app.ui.editor.is_none());
  }

  #[test]
  fn ctrl_s_saves_the_current_text_then_restarts_an_edited_plugin() {
    let mut app = app();
    app.update_editor(EditorMessage::Opened(
      EditorTarget::Plugin("a.rhai".into()),
      Ok("// @name A\n".into()),
    ));
    let effects = app.update_editor(EditorMessage::Save);
    assert_eq!(
      effects,
      vec![Effect::SaveEditorFile(
        EditorTarget::Plugin("a.rhai".into()),
        "// @name A\n".into()
      )]
    );
    app.plugins.names.insert("a.rhai".into(), "A".into());
    let effects = app.update_editor(EditorMessage::Saved(
      EditorTarget::Plugin("a.rhai".into()),
      Ok(None),
    ));
    assert_eq!(effects[0], Effect::DeactivatePlugin("a.rhai".into()));
    let editor = app.ui.editor.as_ref().unwrap();
    assert!(!editor.modified);
    assert_eq!(editor.status.as_deref(), Some("Saved"));
  }

  #[test]
  fn a_new_plugin_starts_from_the_template_with_a_free_name() {
    let mut app = app();
    app.update_editor(EditorMessage::NewPlugin);
    let editor = app.ui.editor.as_ref().unwrap();
    assert_eq!(editor.target, EditorTarget::Plugin("plugin-1.rhai".into()));
    assert!(editor.modified);
    assert_eq!(check(&editor.target, &editor.content.0.text()), None);
  }
}
