//! Plugins actifs : un fil par plugin, événements sans blocage, `shuffle_pick`.

use crate::api::{Outgoing, PickResult, PluginEvent, Sink};
use crate::manifest::PluginSource;
use crate::runtime::{self, Command, Spawned};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

struct Running {
  file: String,
  spawned: Spawned,
}

/// Hôte des plugins. Sans plugin actif : aucun fil, aucune allocation par événement.
pub struct PluginHost {
  plugins: Vec<Running>,
  out: Sink,
  state_dir: PathBuf,
  seed: Option<u64>,
}

impl std::fmt::Debug for PluginHost {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("PluginHost")
      .field(
        "plugins",
        &self.plugins.iter().map(|p| &p.file).collect::<Vec<_>>(),
      )
      .field("state_dir", &self.state_dir)
      .finish_non_exhaustive()
  }
}

/// Mélange de 64 bits (splitmix64) : graines distinctes par plugin.
const fn mix(mut x: u64) -> u64 {
  x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
  x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
  x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
  x ^ (x >> 31)
}

fn file_seed(file: &str) -> u64 {
  file.bytes().fold(0xCBF2_9CE4_8422_2325_u64, |hash, byte| {
    (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
  })
}

impl PluginHost {
  /// Crée l'hôte et le canal par lequel les plugins parlent à l'application.
  /// `state_dir` reçoit les états (`<fichier>.json`) ; les plugins n'en voient jamais le chemin.
  #[must_use]
  pub fn new(state_dir: PathBuf) -> (Self, Receiver<Outgoing>) {
    let (out, incoming) = mpsc::channel();
    let sink: Sink = Arc::new(move |message| out.send(message).is_ok());
    (Self::with_sink(state_dir, sink), incoming)
  }

  /// Comme [`PluginHost::new`], avec une destination fournie par l'application (canal asynchrone).
  #[must_use]
  pub fn with_sink(state_dir: PathBuf, out: Sink) -> Self {
    Self {
      plugins: Vec::new(),
      out,
      state_dir,
      seed: None,
    }
  }

  /// Graine fixe de `random()`, pour des tests reproductibles.
  #[must_use]
  pub const fn with_seed(mut self, seed: u64) -> Self {
    self.seed = Some(seed);
    self
  }

  /// Un plugin au moins est actif.
  #[inline]
  #[must_use]
  pub const fn is_active(&self) -> bool {
    !self.plugins.is_empty()
  }

  #[must_use]
  pub fn is_running(&self, file: &str) -> bool {
    self.plugins.iter().any(|plugin| plugin.file == file)
  }

  /// Fichiers des plugins actifs.
  pub fn running(&self) -> impl Iterator<Item = &str> {
    self.plugins.iter().map(|plugin| plugin.file.as_str())
  }

  /// Active un plugin : son fil compile la source puis appelle `on_ready`. Le résultat arrive en
  /// [`crate::Lifecycle`]. Un plugin du même nom déjà actif est d'abord arrêté.
  ///
  /// # Errors
  /// Le fil n'a pas pu être créé.
  pub fn activate(&mut self, source: PluginSource) -> Result<(), String> {
    self.deactivate(&source.file);
    let seed = mix(
      self.seed.unwrap_or_else(|| {
        SystemTime::now()
          .duration_since(UNIX_EPOCH)
          .map_or(0, |elapsed| elapsed.as_nanos() as u64)
      }) ^ file_seed(&source.file),
    );
    let file = source.file.clone();
    let spawned = runtime::spawn(source, self.state_dir.clone(), self.out.clone(), seed)
      .map_err(|err| format!("fil du plugin non créé : {err}"))?;
    self.plugins.push(Running { file, spawned });
    Ok(())
  }

  /// Désactive un plugin : l'appel en cours est interrompu, `stop()` est appelé puis le moteur est détruit,
  /// sur son fil. Ne bloque pas.
  pub fn deactivate(&mut self, file: &str) {
    if let Some(index) = self.plugins.iter().position(|plugin| plugin.file == file) {
      let running = self.plugins.swap_remove(index);
      running.spawned.cancel.store(true, Ordering::Release);
      // Le canal se ferme ici : le fil appelle `stop()` et s'arrête.
      drop(running);
    }
  }

  /// Oublie un plugin dont le fil s'est déjà arrêté (désactivation automatique, échec de compilation).
  pub fn forget(&mut self, file: &str) {
    self.plugins.retain(|plugin| plugin.file != file);
  }

  /// Transmet un événement aux plugins actifs, sans attendre. Un plugin occupé perd l'événement.
  #[inline]
  pub fn dispatch(&self, event: &PluginEvent) {
    for plugin in &self.plugins {
      let _ = plugin
        .spawned
        .commands
        .try_send(Command::Event(event.clone()));
    }
  }

  /// Un plugin actif propose le prochain titre en mélange.
  #[must_use]
  pub fn picks(&self) -> bool {
    self
      .plugins
      .iter()
      .any(|plugin| plugin.spawned.picks.load(Ordering::Acquire))
  }

  /// Demande `shuffle_pick(ctx)` au premier plugin qui le propose. `None` : aucun, ou plugin occupé.
  #[must_use]
  pub fn shuffle_pick(&self, ctx: Value) -> Option<Receiver<PickResult>> {
    let plugin = self
      .plugins
      .iter()
      .find(|plugin| plugin.spawned.picks.load(Ordering::Acquire))?;
    let (reply, answer) = mpsc::sync_channel(1);
    plugin
      .spawned
      .commands
      .try_send(Command::Pick { ctx, reply })
      .ok()?;
    Some(answer)
  }
}

impl Drop for PluginHost {
  fn drop(&mut self) {
    for plugin in &self.plugins {
      plugin.spawned.cancel.store(true, Ordering::Release);
    }
  }
}
