//! Registre des requêtes et réglages de vie privée. Binaire à part : il change `config.json` et lit le journal entier.

#[cfg(windows)]
use riff_core::config;
use riff_core::core::{Core, CoreConfig};
use riff_core::paths;
use riff_core::secrets::MemoryStore;
use riff_core::services::tools;
use std::sync::Arc;

#[path = "support/temp.rs"]
mod temp;

fn net_lines() -> Vec<String> {
  std::fs::read_to_string(paths::log_path())
    .unwrap_or_default()
    .lines()
    .filter(|l| l.contains("[NET]"))
    .map(str::to_string)
    .collect()
}

/// Un seul test : l'ordre compte, et l'environnement n'est pas partagé avec d'autres fils.
#[tokio::test]
async fn the_only_automatic_request_is_the_yt_dlp_check_at_most_once_per_day() {
  let root = temp::TempDir::new("net-startup");
  std::fs::create_dir_all(root.join("data")).unwrap();
  // SAFETY : aucun autre fil n'existe encore dans ce binaire à un seul test.
  unsafe {
    std::env::set_var("RIFF_DATA_DIR", root.join("data"));
    std::env::set_var("LOCALAPPDATA", root.join("local"));
    std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
    // macOS résout le dossier de config depuis le dossier personnel : jamais le vrai.
    #[cfg(unix)]
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("RIFF_NET_LOG", "1");
    std::env::remove_var("RIFF_YTDLP");
  }

  // Démarrage sans action : aucune requête.
  let core = Core::new(CoreConfig {
    local_base_url: String::new(),
    secret_store: Some(Arc::new(MemoryStore::default())),
  });
  tokio::time::sleep(std::time::Duration::from_millis(300)).await;
  assert_eq!(
    net_lines(),
    Vec::<String>::new(),
    "aucune requête au démarrage"
  );

  // Traduction : coupée par défaut, le choix est retenu.
  assert!(!core.translation_enabled());
  core.set_translation_enabled(true);
  assert!(core.translation_enabled());
  core.set_translation_enabled(false);
  assert!(!core.translation_enabled());

  // Premier écran, yt-dlp non géré par Riff : rien.
  tools::update_yt_dlp_if_managed(core.clone()).await;
  assert_eq!(
    net_lines(),
    Vec::<String>::new(),
    "yt-dlp absent : aucune requête"
  );

  // yt-dlp géré (faux exécutable), réglage coupé : rien.
  #[cfg(windows)]
  {
    let bin = root.join("data").join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(r"C:\Windows\System32\whoami.exe", bin.join("yt-dlp.exe")).unwrap();
    config::set_yt_dlp_auto_update(false);
    tools::update_yt_dlp_if_managed(core.clone()).await;
    assert_eq!(
      net_lines(),
      Vec::<String>::new(),
      "réglage coupé : aucune requête"
    );

    // Réglage actif : une seule vérification, même si le lancement échoue.
    config::set_yt_dlp_auto_update(true);
    tools::update_yt_dlp_if_managed(core.clone()).await;
    let lines = net_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].ends_with("external github.com"), "{lines:?}");
    tools::update_yt_dlp_if_managed(core.clone()).await;
    assert_eq!(net_lines().len(), 1, "au plus une par 24 h");
  }

  // Le journal ne contient que l'hôte : ni chemin ni paramètres.
  let err = core
    .net
    .http_client
    .get("https://evil.example/chemin?token=SENTINELLE")
    .send()
    .await
    .err()
    .unwrap();
  assert!(err.is_blocked());
  let _ = core.net.http_client.get("http://[bad").send().await;
  assert!(
    !std::fs::read_to_string(paths::log_path())
      .unwrap_or_default()
      .contains("SENTINELLE")
  );
}
