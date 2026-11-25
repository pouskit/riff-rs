#![cfg_attr(not(test), windows_subsystem = "windows")]
//! Lancement de l'application native.

use riff_app::app::{Program, StartArgs};
use riff_app::system::cli;
use riff_app::system::startup::{self, Outcome};
use riff_app::theme::fonts;
use riff_app::window::PersistedWindowState;
use riff_core::paths;

const WORKER_THREADS: usize = 2;

/// Exécuteur d'iced : tokio (requis par `reqwest`) avec peu de fils, le travail bloquant du core a son propre groupe.
struct Executor(tokio::runtime::Runtime);

impl iced::executor::Executor for Executor {
  fn new() -> Result<Self, iced::futures::io::Error> {
    tokio::runtime::Builder::new_multi_thread()
      .worker_threads(WORKER_THREADS)
      .enable_all()
      .build()
      .map(Self)
  }

  fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
    drop(self.0.spawn(future));
  }

  fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
    self.0.block_on(future)
  }

  fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
    let _guard = self.0.enter();
    f()
  }
}

fn init_tracing() {
  let dir = paths::log_dir();
  let _ = std::fs::create_dir_all(&dir);
  let path = dir.join("riff-app.log");
  let Ok(file) = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(path)
  else {
    return;
  };
  let _ = tracing_subscriber::fmt()
    .with_ansi(false)
    .with_max_level(tracing::Level::INFO)
    .with_writer(std::sync::Mutex::new(file))
    .try_init();
}

fn main() -> iced::Result {
  #[cfg(windows)]
  if std::env::var_os("WGPU_BACKEND").is_none() {
    // SAFETY: aucun autre fil n'est encore démarré au tout début de `main`.
    unsafe {
      std::env::set_var("WGPU_BACKEND", "dx12");
    }
  }
  // Avant tout accès au dossier de données (journal, `riff lyrics`) : reprise de l'ancien dossier par défaut.
  let adoption = riff_core::adopt::adopt_previous_data();
  let raw_args: Vec<String> = std::env::args().skip(1).collect();
  // `riff lyrics` : jamais de fenêtre ni de canal pris, détecté en premier.
  if cli::is_lyrics_command(&raw_args) {
    std::process::exit(cli::run());
  }
  init_tracing();
  match &adoption {
    riff_core::adopt::Adoption::Skipped => {}
    riff_core::adopt::Adoption::Done {
      from,
      entries,
      skipped,
    } => {
      tracing::info!(from = %from.display(), entries, ?skipped, "données de l'ancien dossier reprises")
    }
    riff_core::adopt::Adoption::Failed { from, error } => {
      tracing::warn!(from = %from.display(), %error, "reprise de l'ancien dossier impossible");
    }
  }
  // Mesures et essais (`--t0`, `--scene`, `--bench-scroll`...) ne prennent jamais le canal d'une vraie instance.
  if !raw_args.iter().any(|arg| {
    arg == "--t0" || arg == "--scene" || arg == "--bench-scroll" || arg == "--latency-log"
  }) && let Outcome::Exit(code) = startup::acquire(&raw_args)
  {
    std::process::exit(code);
  }
  // Identité de la barre des tâches : les fenêtres se regroupent avec le raccourci « Riff (natif) ». Avant la première fenêtre.
  #[cfg(windows)]
  riff_app::system::platform::windows_shell::set_process_app_id(riff_app::system::launcher::APP_ID);
  let window_state = PersistedWindowState::read(&paths::window_state_path()).unwrap_or_default();
  let args = StartArgs::parse(raw_args);
  let app = iced::daemon(
    move || Program::boot(&args, window_state.settings()),
    Program::update,
    Program::view,
  )
  .title("Riff")
  .subscription(Program::subscription)
  .theme(|program: &Program, _window: iced::window::Id| program.theme())
  .scale_factor(|program: &Program, _window: iced::window::Id| program.scale_factor())
  .antialiasing(true)
  .default_font(fonts::UI)
  .executor::<Executor>();
  fonts::bytes()
    .into_iter()
    .fold(app, |app, font| app.font(font))
    .run()
}
