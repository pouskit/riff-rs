//! Deux vrais processus sur le canal local : le premier le prend, le second le trouve occupé, demande « montre-toi »
//! avec ses fichiers, puis s'arrête. Le test se relance lui-même (aucune fenêtre, aucune icône, aucun raccourci).
//! Le canal porte un nom unique : l'instance réelle de l'utilisateur n'est jamais touchée.

use riff_app::system::channel::{Claim, claim, notify_show};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tokio::sync::mpsc;

const CHILD_LABEL: &str = "RIFF_TEST_CHILD_LABEL";

/// Rôle du second processus : ne fait rien quand le test tourne normalement.
#[tokio::test]
async fn second_instance_role() {
  let Ok(label) = std::env::var(CHILD_LABEL) else {
    return;
  };
  let (tx, _rx) = mpsc::unbounded_channel();
  match claim(&label, tx).await.expect("claim") {
    Claim::Existing => {}
    Claim::Primary(_) => std::process::exit(3),
  }
  if notify_show(&label, vec!["a.mp3".into(), "b.flac".into()])
    .await
    .is_err()
  {
    std::process::exit(4);
  }
}

#[tokio::test]
async fn a_second_process_finds_the_first_asks_it_to_show_and_exits() {
  let label = format!("riff-two-process-{}", std::process::id());
  let (tx, mut rx) = mpsc::unbounded_channel();
  let Claim::Primary(_server) = claim(&label, tx).await.expect("claim") else {
    panic!("canal déjà pris")
  };

  let exe = std::env::current_exe().expect("exe du test");
  let mut child = Command::new(exe)
    .args([
      "second_instance_role",
      "--exact",
      "--nocapture",
      "--test-threads=1",
    ])
    .env(CHILD_LABEL, &label)
    .spawn()
    .expect("second processus");
  let status = tokio::task::spawn_blocking(move || child.wait())
    .await
    .unwrap()
    .expect("attente");
  assert!(
    status.success(),
    "second processus : {status:?} (3 = a pris le canal, 4 = envoi refusé)"
  );

  let paths = tokio::time::timeout(Duration::from_secs(5), rx.recv())
    .await
    .expect("délai")
    .expect("montre-toi reçu");
  assert_eq!(paths, vec![PathBuf::from("a.mp3"), PathBuf::from("b.flac")]);
}
