//! Temps d'ouverture d'un fichier dans l'éditeur de code, par taille.
//! Mesure en release : `cargo test --release -p riff-app --test editor_open -- --ignored --nocapture`.

use riff_app::code_editor::{CodeEditor, EditorTarget, MAX_EDITOR_OPEN_BYTES};
use std::time::Instant;

#[test]
#[ignore = "mesure, à lancer en release"]
fn measure_open_time_by_size() {
  let line = "let value = track.title + \" - \" + track.artist; // commentaire\n";
  for size in [16_384, 65_536, 131_072, 262_144, 524_288, 1_048_576] {
    let text = line.repeat(size / line.len());
    let started = Instant::now();
    let editor = CodeEditor::new(EditorTarget::Plugin("big.rhai".into()), &text);
    let elapsed = started.elapsed();
    eprintln!(
      "ouverture de {} octets ({} lignes) : {elapsed:?}",
      text.len(),
      editor.content.0.line_count()
    );
  }
  eprintln!("plafond d'ouverture : {MAX_EDITOR_OPEN_BYTES} octets");
}
