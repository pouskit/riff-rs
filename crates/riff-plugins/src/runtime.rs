//! Fil d'un plugin : moteur, API hôte selon les permissions, budgets, état persistant.

use crate::api::{
  EVENT_BUDGET, HostCall, HostRequest, HostResponse, Lifecycle, MAX_FAILURES, NotifyKind, Outgoing,
  PICK_BUDGET, PickResult, PluginEvent, READ_WAIT, SEARCH_WAIT, Sink,
};
use crate::manifest::{Permission, PluginSource, clean_text};
use crate::sandbox::{base_engine, compile_with};
use crate::track;
use rhai::{AST, Array, CallFnOptions, Dynamic, Engine, EvalAltResult, Map, Scope};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{self, Write as _};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

/// Taille maximale de l'état enregistré d'un plugin.
pub const STATE_MAX_BYTES: usize = 256 * 1024;
/// Pile du fil d'un plugin : Rhai en debug consomme beaucoup de pile par niveau d'appel.
const STACK_SIZE: usize = 16 * 1024 * 1024;
/// Plafond absolu d'un appel, attentes de l'application comprises.
const HOST_WAIT_CAP: Duration = Duration::from_secs(30);
/// Listes renvoyées au plugin (file, favoris, historique), en nombre de pistes.
const HOST_LIST_MAX: usize = 2_000;
const APPEND_MAX: usize = 50;
const NOTIFY_CHARS: usize = 200;
const LOG_CHARS: usize = 500;
const QUERY_CHARS: usize = 200;
/// Opérations Rhai autorisées par milliseconde de budget (500 ms : 2 milliards). Environ 4 fois le débit
/// mesuré d'une boucle vide en release (465 millions par seconde) : le temps arrête le script le premier.
const OPS_PER_MS: u64 = 4_000_000;
/// Commandes en attente par plugin : au-delà, un événement est abandonné.
pub(crate) const COMMAND_QUEUE: usize = 32;

static THREADS_SPAWNED: AtomicUsize = AtomicUsize::new(0);

/// Nombre de fils de plugin créés depuis le lancement : aucun plugin, aucun fil.
#[must_use]
pub fn threads_spawned() -> usize {
  THREADS_SPAWNED.load(Ordering::Relaxed)
}

/// Fonctions de l'API soumises à une permission. Absentes du moteur sans elle.
const GUARDED: [(&str, Permission); 10] = [
  ("queue", Permission::Queue),
  ("queue_index", Permission::Queue),
  ("queue_context", Permission::Queue),
  ("queue_append", Permission::Queue),
  ("favorites", Permission::Library),
  ("history", Permission::Library),
  ("search", Permission::Search),
  ("vibe", Permission::Search),
  ("notify", Permission::Notify),
  ("shuffle_on", Permission::Shuffle),
];

pub(crate) enum Command {
  Event(PluginEvent),
  Pick {
    ctx: Value,
    reply: SyncSender<PickResult>,
  },
}

/// Côté hôte d'un fil de plugin.
pub(crate) struct Spawned {
  pub commands: SyncSender<Command>,
  pub cancel: Arc<AtomicBool>,
  /// Vrai quand le script est compilé, a `shuffle_pick` et la permission `shuffle`.
  pub picks: Arc<AtomicBool>,
}

/// Fenêtre glissante de débit.
struct Rate {
  max: usize,
  span: Duration,
  times: VecDeque<Instant>,
}

impl Rate {
  const fn new(max: usize, span: Duration) -> Self {
    Self {
      max,
      span,
      times: VecDeque::new(),
    }
  }

  fn allow(&mut self, now: Instant) -> bool {
    while self
      .times
      .front()
      .is_some_and(|at| now.duration_since(*at) >= self.span)
    {
      self.times.pop_front();
    }
    if self.times.len() >= self.max {
      return false;
    }
    self.times.push_back(now);
    true
  }
}

/// État d'un appel en cours, partagé avec les fonctions de l'hôte et le rappel de progression.
struct Ctx {
  file: String,
  out: Sink,
  cancel: Arc<AtomicBool>,
  deadline: Cell<Instant>,
  hard_deadline: Cell<Instant>,
  over_budget: Cell<bool>,
  rng: Cell<u64>,
  notify: RefCell<Rate>,
  search: RefCell<Rate>,
  log: RefCell<Rate>,
}

impl Ctx {
  fn begin(&self, budget: Duration) {
    let now = Instant::now();
    self.deadline.set(now + budget);
    self.hard_deadline.set(now + budget + HOST_WAIT_CAP);
    self.over_budget.set(false);
  }

  fn send(&self, request: HostRequest) {
    let _ = (self.out)(Outgoing::Call(HostCall::new(
      self.file.clone(),
      request,
      None,
    )));
  }

  /// Demande à l'application et attend sa réponse. L'attente ne compte pas dans le budget du plugin,
  /// dans la limite du plafond absolu.
  fn ask(&self, request: HostRequest, wait: Duration) -> Result<Value, Box<EvalAltResult>> {
    let started = Instant::now();
    let (reply, answer) = mpsc::sync_channel(1);
    let delivered = (self.out)(Outgoing::Call(HostCall::new(
      self.file.clone(),
      request,
      Some(reply),
    )));
    if !delivered {
      return Err(runtime_error("l'application est arrêtée"));
    }
    let result = answer.recv_timeout(wait);
    let credited = (self.deadline.get() + started.elapsed()).min(self.hard_deadline.get());
    self.deadline.set(credited);
    if self.cancel.load(Ordering::Acquire) {
      return Err(runtime_error("plugin arrêté"));
    }
    match result {
      Ok(HostResponse::Value(value)) => Ok(value),
      Ok(HostResponse::Error(message)) => Err(runtime_error(&message)),
      Err(RecvTimeoutError::Timeout) => Err(runtime_error(&format!(
        "l'application n'a pas répondu en {} s",
        wait.as_secs()
      ))),
      Err(RecvTimeoutError::Disconnected) => Err(runtime_error("l'application est arrêtée")),
    }
  }

  /// `Math.random()` : xorshift64*, reproductible avec une graine fixe.
  fn random(&self) -> f64 {
    let mut x = self.rng.get();
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    self.rng.set(x);
    let mixed = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
    (mixed >> 11) as f64 / (1_u64 << 53) as f64
  }
}

fn runtime_error(message: &str) -> Box<EvalAltResult> {
  message.to_string().into()
}

fn to_dynamic(value: &Value) -> Dynamic {
  rhai::serde::to_dynamic(value).unwrap_or(Dynamic::UNIT)
}

fn from_dynamic(value: &Dynamic) -> Value {
  rhai::serde::from_dynamic(value).unwrap_or(Value::Null)
}

/// Liste de pistes de l'application, bornée, nettoyée.
fn track_list(value: &Value) -> Dynamic {
  let tracks: Vec<Value> = value
    .as_array()
    .map(|items| {
      items
        .iter()
        .take(HOST_LIST_MAX)
        .filter_map(track::sanitize)
        .collect()
    })
    .unwrap_or_default();
  to_dynamic(&Value::Array(tracks))
}

fn register_api(engine: &mut Engine, ctx: &Rc<Ctx>, permissions: &[Permission]) {
  let allows = |perm: Permission| permissions.contains(&perm);
  // Simule un défaut de l'hôte : le test vérifie qu'une panique désactive le plugin sans tuer l'application.
  #[cfg(test)]
  engine.register_fn("__test_panic", || -> bool { panic!("panique de test") });

  let c = Rc::clone(ctx);
  engine.register_fn("track", move || -> Result<Dynamic, Box<EvalAltResult>> {
    let value = c.ask(HostRequest::Track, READ_WAIT)?;
    Ok(track::sanitize(&value).map_or(Dynamic::UNIT, |track| to_dynamic(&track)))
  });
  let c = Rc::clone(ctx);
  engine.register_fn("is_playing", move || -> Result<bool, Box<EvalAltResult>> {
    Ok(c.ask(HostRequest::IsPlaying, READ_WAIT)?.as_bool() == Some(true))
  });
  engine.register_fn("same_track", |a: Dynamic, b: Dynamic| {
    track::same_track(&from_dynamic(&a), &from_dynamic(&b))
  });
  let c = Rc::clone(ctx);
  engine.register_fn("random", move || c.random());
  let c = Rc::clone(ctx);
  engine.register_fn("log", move |message: Dynamic| {
    if c.log.borrow_mut().allow(Instant::now()) {
      c.send(HostRequest::Log(clean_text(
        &message.to_string(),
        LOG_CHARS,
      )));
    }
  });

  if allows(Permission::Queue) {
    let c = Rc::clone(ctx);
    engine.register_fn("queue", move || -> Result<Dynamic, Box<EvalAltResult>> {
      Ok(track_list(&c.ask(HostRequest::Queue, READ_WAIT)?))
    });
    let c = Rc::clone(ctx);
    engine.register_fn(
      "queue_index",
      move || -> Result<Dynamic, Box<EvalAltResult>> {
        Ok(
          c.ask(HostRequest::QueueIndex, READ_WAIT)?
            .as_i64()
            .map_or(Dynamic::UNIT, Dynamic::from),
        )
      },
    );
    let c = Rc::clone(ctx);
    engine.register_fn(
      "queue_context",
      move || -> Result<String, Box<EvalAltResult>> {
        let value = c.ask(HostRequest::QueueContext, READ_WAIT)?;
        Ok(clean_text(value.as_str().unwrap_or(""), 64))
      },
    );
    let c = Rc::clone(ctx);
    engine.register_fn(
      "queue_append",
      move |tracks: Array| -> Result<Dynamic, Box<EvalAltResult>> {
        let cleaned: Vec<Value> = tracks
          .iter()
          .take(APPEND_MAX)
          .filter_map(|item| track::incoming(&from_dynamic(item)))
          .collect();
        if cleaned.is_empty() {
          return Ok(Dynamic::from_array(Array::new()));
        }
        Ok(track_list(
          &c.ask(HostRequest::QueueAppend(cleaned), READ_WAIT)?,
        ))
      },
    );
  }

  if allows(Permission::Library) {
    let c = Rc::clone(ctx);
    engine.register_fn(
      "favorites",
      move || -> Result<Dynamic, Box<EvalAltResult>> {
        Ok(track_list(&c.ask(HostRequest::Favorites, READ_WAIT)?))
      },
    );
    let c = Rc::clone(ctx);
    engine.register_fn("history", move || -> Result<Dynamic, Box<EvalAltResult>> {
      Ok(track_list(&c.ask(HostRequest::History, READ_WAIT)?))
    });
  }

  if allows(Permission::Search) {
    let search = {
      let c = Rc::clone(ctx);
      move |query: &str, options: &Map| -> Result<Dynamic, Box<EvalAltResult>> {
        if !c.search.borrow_mut().allow(Instant::now()) {
          return Ok(Dynamic::from_array(Array::new()));
        }
        let limit = options
          .get("limit")
          .and_then(|limit| limit.as_int().ok())
          .unwrap_or(10)
          .clamp(1, 30) as u32;
        let platform = options
          .get("platform")
          .map(|platform| clean_text(&platform.to_string(), 32))
          .filter(|platform| {
            !platform.is_empty()
              && platform
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
          });
        let value = c.ask(
          HostRequest::Search {
            query: clean_text(query, QUERY_CHARS),
            limit,
            platform,
          },
          SEARCH_WAIT,
        )?;
        Ok(track_list(&value))
      }
    };
    let with_options = search.clone();
    engine.register_fn("search", move |query: &str, options: Map| {
      with_options(query, &options)
    });
    engine.register_fn("search", move |query: &str| search(query, &Map::new()));
    let c = Rc::clone(ctx);
    engine.register_fn(
      "vibe",
      move |body: Map| -> Result<Dynamic, Box<EvalAltResult>> {
        if !c.search.borrow_mut().allow(Instant::now()) {
          return Ok(Dynamic::from_array(Array::new()));
        }
        let body = from_dynamic(&Dynamic::from_map(body));
        Ok(track_list(&c.ask(HostRequest::Vibe(body), SEARCH_WAIT)?))
      },
    );
  }

  if allows(Permission::Notify) {
    let notify = {
      let c = Rc::clone(ctx);
      move |message: &Dynamic, kind: &str| -> bool {
        if !c.notify.borrow_mut().allow(Instant::now()) {
          return false;
        }
        let message = clean_text(&message.to_string(), NOTIFY_CHARS);
        if message.is_empty() {
          return false;
        }
        c.send(HostRequest::Notify {
          message,
          kind: NotifyKind::parse(kind),
        });
        true
      }
    };
    let with_kind = notify.clone();
    engine.register_fn("notify", move |message: Dynamic, kind: &str| {
      with_kind(&message, kind)
    });
    engine.register_fn("notify", move |message: Dynamic| notify(&message, "info"));
  }

  if allows(Permission::Shuffle) {
    let c = Rc::clone(ctx);
    engine.register_fn("shuffle_on", move || -> Result<bool, Box<EvalAltResult>> {
      Ok(c.ask(HostRequest::ShuffleOn, READ_WAIT)?.as_bool() == Some(true))
    });
  }
}

/// Message d'erreur affichable, avec « permission refusée » pour une fonction gardée absente.
fn describe(err: &EvalAltResult, permissions: &[Permission]) -> String {
  if let EvalAltResult::ErrorFunctionNotFound(signature, _) = err.unwrap_inner() {
    let name = signature
      .split([' ', '('])
      .next()
      .unwrap_or(signature.as_str());
    if let Some((_, perm)) = GUARDED
      .iter()
      .find(|(guarded, perm)| *guarded == name && !permissions.contains(perm))
    {
      return format!("permission refusée : {} ({name})", perm.as_str());
    }
  }
  err.to_string()
}

/// Issue d'un appel de fonction du script.
enum Outcome {
  Done(Dynamic),
  /// Fonction absente : rien à faire.
  Missing,
  Failed(String),
  /// Budget de temps ou d'opérations dépassé : désactivation immédiate.
  OverBudget(String),
  Cancelled,
}

struct Plugin {
  file: String,
  permissions: Vec<Permission>,
  engine: Engine,
  ast: AST,
  ctx: Rc<Ctx>,
  functions: HashSet<(String, usize)>,
  state: Dynamic,
  saved_state: String,
  state_path: PathBuf,
  failures: u32,
}

impl Plugin {
  fn has(&self, name: &str, arity: usize) -> bool {
    self.functions.contains(&(name.to_string(), arity))
  }

  fn call(&mut self, name: &str, args: Vec<Dynamic>, budget: Duration) -> Outcome {
    if !self.has(name, args.len()) {
      return Outcome::Missing;
    }
    self.ctx.begin(budget);
    // Plafond d'opérations proportionnel au budget : seconde barrière derrière le temps.
    self
      .engine
      .set_max_operations(OPS_PER_MS.saturating_mul(budget.as_millis() as u64));
    let mut scope = Scope::new();
    let options = CallFnOptions::new().bind_this_ptr(&mut self.state);
    let engine = &self.engine;
    let ast = &self.ast;
    let result = catch_unwind(AssertUnwindSafe(|| {
      engine.call_fn_with_options::<Dynamic>(options, &mut scope, ast, name, args)
    }));
    if self.ctx.cancel.load(Ordering::Acquire) {
      return Outcome::Cancelled;
    }
    let outcome = match result {
      Err(_) => return Outcome::OverBudget(format!("panique pendant {name}")),
      Ok(Ok(value)) => Outcome::Done(value),
      Ok(Err(err)) => {
        let over = self.ctx.over_budget.get()
          || matches!(err.unwrap_inner(), EvalAltResult::ErrorTooManyOperations(_));
        if over {
          return Outcome::OverBudget(format!(
            "{name} a dépassé son budget ({} ms)",
            budget.as_millis()
          ));
        }
        Outcome::Failed(format!("{name} : {}", describe(&err, &self.permissions)))
      }
    };
    match self.save_state() {
      Ok(()) => outcome,
      Err(reason) => Outcome::Failed(reason),
    }
  }

  /// Écrit l'état s'il a changé. Les clés qui commencent par `_` ne sont pas enregistrées.
  fn save_state(&mut self) -> Result<(), String> {
    if !self.state.is_map() {
      self.state = Dynamic::from_map(Map::new());
      return Err("l'état (this) doit rester une carte".to_string());
    }
    let mut value = from_dynamic(&self.state);
    if let Some(map) = value.as_object_mut() {
      map.retain(|key, _| !key.starts_with('_'));
    }
    let text = serde_json::to_string(&value).unwrap_or_default();
    if text.len() > STATE_MAX_BYTES {
      return Err(format!(
        "état trop gros : {} octets (maximum {STATE_MAX_BYTES})",
        text.len()
      ));
    }
    if text == self.saved_state {
      return Ok(());
    }
    write_atomic(&self.state_path, text.as_bytes())
      .map_err(|err| format!("état non enregistré : {err}"))?;
    self.saved_state = text;
    Ok(())
  }
}

/// Écriture complète dans un fichier temporaire, puis renommage.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir)?;
  }
  let tmp = path.with_extension("json.tmp");
  let mut file = fs::File::create(&tmp)?;
  file.write_all(bytes)?;
  file.sync_all()?;
  drop(file);
  fs::rename(&tmp, path)
}

/// Lit l'état enregistré : absent ou illisible, une carte vide.
fn read_state(path: &Path) -> (Dynamic, String) {
  let text = fs::metadata(path)
    .ok()
    .filter(|meta| meta.len() <= STATE_MAX_BYTES as u64)
    .and_then(|_| fs::read_to_string(path).ok())
    .unwrap_or_default();
  match serde_json::from_str::<Value>(&text) {
    Ok(value @ Value::Object(_)) => (to_dynamic(&value), text),
    _ => (Dynamic::from_map(Map::new()), String::new()),
  }
}

/// Chemin de l'état d'un plugin : `<dossier>/<fichier>.json`, nom déjà validé.
#[must_use]
pub fn state_path(state_dir: &Path, file: &str) -> PathBuf {
  state_dir.join(format!("{file}.json"))
}

struct Thread {
  source: PluginSource,
  state_dir: PathBuf,
  out: Sink,
  commands: Receiver<Command>,
  cancel: Arc<AtomicBool>,
  picks: Arc<AtomicBool>,
  seed: u64,
}

impl Thread {
  fn lifecycle(&self, event: Lifecycle) {
    let _ = (self.out)(Outgoing::Lifecycle(event));
  }

  fn load(&self) -> Result<Plugin, String> {
    let ctx = Rc::new(Ctx {
      file: self.source.file.clone(),
      out: self.out.clone(),
      cancel: Arc::clone(&self.cancel),
      deadline: Cell::new(Instant::now()),
      hard_deadline: Cell::new(Instant::now()),
      over_budget: Cell::new(false),
      rng: Cell::new(self.seed | 1),
      notify: RefCell::new(Rate::new(5, Duration::from_secs(10))),
      search: RefCell::new(Rate::new(30, Duration::from_secs(60))),
      log: RefCell::new(Rate::new(20, Duration::from_secs(10))),
    });
    let mut engine = base_engine();
    let progress = Rc::clone(&ctx);
    engine.on_progress(move |operations| {
      if operations % 128 != 0 {
        return None;
      }
      if progress.cancel.load(Ordering::Acquire) {
        return Some("plugin arrêté".into());
      }
      if Instant::now() > progress.deadline.get() {
        progress.over_budget.set(true);
        return Some("budget dépassé".into());
      }
      None
    });
    let permissions = self.source.manifest.permissions.clone();
    register_api(&mut engine, &ctx, &permissions);
    let ast = compile_with(&engine, &self.source.code).map_err(|err| err.to_string())?;
    let functions = ast
      .iter_functions()
      .map(|f| (f.name.to_string(), f.params.len()))
      .collect();
    let state_path = state_path(&self.state_dir, &self.source.file);
    let (state, saved_state) = read_state(&state_path);
    Ok(Plugin {
      file: self.source.file.clone(),
      permissions,
      engine,
      ast,
      ctx,
      functions,
      state,
      saved_state,
      state_path,
      failures: 0,
    })
  }

  /// Applique la règle d'échec. `Some(raison)` : le plugin doit être désactivé.
  fn settle(&self, plugin: &mut Plugin, outcome: &Outcome) -> Option<String> {
    match outcome {
      Outcome::Done(_) => {
        plugin.failures = 0;
        None
      }
      Outcome::Missing | Outcome::Cancelled => None,
      Outcome::OverBudget(reason) => Some(reason.clone()),
      Outcome::Failed(message) => {
        plugin.failures += 1;
        self.lifecycle(Lifecycle::Error {
          file: plugin.file.clone(),
          message: message.clone(),
        });
        (plugin.failures >= MAX_FAILURES)
          .then(|| format!("{MAX_FAILURES} échecs de suite, dernier : {message}"))
      }
    }
  }

  fn run(self) {
    let file = self.source.file.clone();
    let mut plugin = match self.load() {
      Ok(plugin) => plugin,
      Err(reason) => {
        self.lifecycle(Lifecycle::Failed { file, reason });
        return;
      }
    };
    let picks = plugin.has("shuffle_pick", 1) && plugin.permissions.contains(&Permission::Shuffle);
    self.picks.store(picks, Ordering::Release);
    self.lifecycle(Lifecycle::Started { file: file.clone() });

    let ready = plugin.call("on_ready", Vec::new(), EVENT_BUDGET);
    if let Some(reason) = self.settle(&mut plugin, &ready) {
      self.disable(&file, reason);
      return;
    }
    // Le canal se ferme quand l'hôte désactive le plugin : `stop()`, puis fin du fil.
    while let Ok(command) = self.commands.recv() {
      if self.cancel.load(Ordering::Acquire) {
        if let Command::Pick { reply, .. } = command {
          let _ = reply.try_send(Err("plugin arrêté".to_string()));
        }
        continue;
      }
      let outcome = match command {
        Command::Event(event) => {
          let args = match &event {
            PluginEvent::TrackChange(track) => vec![to_dynamic(track)],
            _ => Vec::new(),
          };
          plugin.call(event.function(), args, EVENT_BUDGET)
        }
        Command::Pick { ctx, reply } => {
          let outcome = if picks {
            plugin.call("shuffle_pick", vec![to_dynamic(&ctx)], PICK_BUDGET)
          } else {
            Outcome::Missing
          };
          let _ = reply.try_send(match &outcome {
            Outcome::Done(value) if value.is_unit() => Ok(None),
            Outcome::Done(value) => Ok(track::sanitize(&from_dynamic(value))),
            Outcome::Missing => Ok(None),
            Outcome::Failed(reason) | Outcome::OverBudget(reason) => Err(reason.clone()),
            Outcome::Cancelled => Err("plugin arrêté".to_string()),
          });
          outcome
        }
      };
      if let Some(reason) = self.settle(&mut plugin, &outcome) {
        self.disable(&file, reason);
        return;
      }
    }
    self.picks.store(false, Ordering::Release);
    // `stop()` s'exécute même si l'arrêt a interrompu l'appel précédent.
    self.cancel.store(false, Ordering::Release);
    let _ = plugin.call("stop", Vec::new(), EVENT_BUDGET);
    self.lifecycle(Lifecycle::Stopped { file });
  }

  fn disable(&self, file: &str, reason: String) {
    self.picks.store(false, Ordering::Release);
    self.lifecycle(Lifecycle::Disabled {
      file: file.to_string(),
      reason,
    });
  }
}

/// Lance le fil d'un plugin. Il compile, appelle `on_ready`, puis attend ses commandes.
///
/// # Errors
/// Le système refuse de créer le fil.
pub(crate) fn spawn(
  source: PluginSource,
  state_dir: PathBuf,
  out: Sink,
  seed: u64,
) -> io::Result<Spawned> {
  let (commands, receiver) = mpsc::sync_channel(COMMAND_QUEUE);
  let cancel = Arc::new(AtomicBool::new(false));
  let picks = Arc::new(AtomicBool::new(false));
  let thread = Thread {
    source,
    state_dir,
    out,
    commands: receiver,
    cancel: Arc::clone(&cancel),
    picks: Arc::clone(&picks),
    seed,
  };
  let name = format!("plugin {}", thread.source.file);
  let panic_out = thread.out.clone();
  let panic_file = thread.source.file.clone();
  thread::Builder::new()
    .name(name)
    .stack_size(STACK_SIZE)
    .spawn(move || {
      if catch_unwind(AssertUnwindSafe(|| thread.run())).is_err() {
        let _ = panic_out(Outgoing::Lifecycle(Lifecycle::Disabled {
          file: panic_file,
          reason: "panique du fil du plugin".to_string(),
        }));
      }
    })?;
  THREADS_SPAWNED.fetch_add(1, Ordering::Relaxed);
  Ok(Spawned {
    commands,
    cancel,
    picks,
  })
}
