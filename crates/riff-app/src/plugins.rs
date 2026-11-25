//! Pont entre les plugins Rhai (`riff-plugins`) et le lecteur : événements, mélange, réponses aux demandes.

use crate::app::{App, Effect};
use crate::user_data::Track;
use crate::widgets::toast::ToastKind;
use iced::futures::stream::{self, BoxStream, StreamExt};
use riff_plugins::api::PICK_BUDGET;
use riff_plugins::{
  Consent, HostRequest, HostResponse, Lifecycle, NotifyKind, Outgoing, PickResult, PluginEntry,
  PluginEvent, PluginSource, Scan, track as plugin_track,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Attente d'un tirage de plugin avant de revenir au mélange de Riff.
pub const PICK_WAIT: Duration = PICK_BUDGET.saturating_add(Duration::from_millis(500));

/// Tirage demandé à un plugin, en attente de sa réponse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingPick {
  pub id: u64,
  pub crossfade: Option<u32>,
}

/// État des plugins vu par l'application.
#[derive(Debug, Clone, Default)]
pub struct PluginsState {
  /// Un plugin au moins est actif : sans lui, aucun événement n'est construit.
  pub active: bool,
  /// Un plugin actif propose le prochain titre en mélange.
  pub picks: bool,
  pub pending_pick: Option<PendingPick>,
  /// Nom affiché de chaque plugin actif, pour ses notifications.
  pub names: HashMap<String, String>,
  /// Dernière erreur de chaque plugin.
  pub errors: HashMap<String, String>,
  pub consent: Consent,
  /// Contenu du dossier des plugins, lu à l'ouverture de la catégorie.
  pub entries: Vec<PluginEntry>,
  last_token: u64,
  last_playing: bool,
}

/// Messages des plugins.
#[derive(Debug, Clone)]
pub enum PluginMessage {
  /// Demande ou changement d'état venu d'un fil de plugin.
  Outgoing(Outgoing),
  /// Réponse de `shuffle_pick`, ou délai dépassé.
  Picked(u64, PickResult),
  /// Relit le dossier des plugins.
  Scan,
  Scanned(Box<Scan>),
  /// Activation (après lecture des permissions affichées) ou désactivation.
  Toggle(String, bool),
  Loaded(Result<Box<PluginSource>, String>),
  Remove(String),
  Removed(String, Result<(), String>),
  Import,
  Imported(Vec<Result<String, String>>),
  OpenFolder,
}

/// Réponse de l'application à une demande de plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginAnswer {
  Now(HostResponse, Vec<Effect>),
  /// Rien à répondre (notification, journal), seulement des effets.
  Silent(Vec<Effect>),
  /// Recherche par le core, hors du fil d'interface.
  Search {
    query: String,
    limit: u32,
    platform: Option<String>,
  },
  Vibe(Value),
}

fn toast_kind(kind: NotifyKind) -> ToastKind {
  match kind {
    NotifyKind::Info => ToastKind::Info,
    NotifyKind::Success => ToastKind::Success,
    NotifyKind::Warning => ToastKind::Warning,
    NotifyKind::Error => ToastKind::Error,
  }
}

impl App {
  /// Piste vue par un plugin : champs autorisés seulement, plateforme renseignée.
  #[must_use]
  pub fn plugin_track(&self, track: &Track) -> Value {
    let mut value = serde_json::to_value(track).unwrap_or(Value::Null);
    if let Some(map) = value.as_object_mut()
      && !map.contains_key("platform")
    {
      map.insert("platform".into(), Value::from(self.data.platform.clone()));
    }
    plugin_track::sanitize(&value).unwrap_or(Value::Null)
  }

  fn plugin_tracks<'a>(&self, tracks: impl Iterator<Item = &'a Track>) -> Value {
    Value::Array(tracks.map(|track| self.plugin_track(track)).collect())
  }

  fn plugin_context(&self) -> &'static str {
    if self.player.my_wave {
      "wave"
    } else if self.player.radio {
      "radio"
    } else {
      "queue"
    }
  }

  /// Événements à transmettre depuis le dernier appel : changement de titre, lecture, pause.
  /// Sans plugin actif, ne construit rien.
  pub fn plugin_events(&mut self) -> Vec<PluginEvent> {
    if !self.plugins.active {
      return Vec::new();
    }
    let mut events = Vec::new();
    if self.player.token != self.plugins.last_token {
      self.plugins.last_token = self.player.token;
      let track = self
        .player
        .current
        .as_ref()
        .map_or(Value::Null, |track| self.plugin_track(track));
      events.push(PluginEvent::TrackChange(track));
    }
    let playing = self.playback.is_playing;
    if playing != self.plugins.last_playing {
      self.plugins.last_playing = playing;
      events.push(if playing {
        PluginEvent::Play
      } else {
        PluginEvent::Pause
      });
    }
    events
  }

  /// `shuffle.setProvider` : en mélange, le prochain titre est demandé au plugin.
  pub(crate) fn plugin_pick(&mut self, crossfade: Option<u32>) -> Option<Vec<Effect>> {
    if !self.plugins.picks || !self.player.shuffle_applies() {
      return None;
    }
    let id = self.next_unique();
    self.plugins.pending_pick = Some(PendingPick { id, crossfade });
    let ctx = json!({
      "queue": self.plugin_tracks(self.player.queue.iter()),
      "queue_index": self.player.index,
      "context": self.plugin_context(),
      "current": self.player.current.as_ref().map(|track| self.plugin_track(track)),
    });
    Some(vec![Effect::PluginPick { id, ctx }])
  }

  /// Réponse d'un tirage. Piste absente de la file, `()`, erreur ou délai : mélange de Riff.
  pub(crate) fn plugin_picked(&mut self, id: u64, result: PickResult) -> Vec<Effect> {
    if self.plugins.pending_pick.map(|pending| pending.id) != Some(id) {
      return Vec::new();
    }
    let Some(pending) = self.plugins.pending_pick.take() else {
      return Vec::new();
    };
    if let Err(reason) = &result {
      tracing::warn!(reason = %riff_core::logging::redact(reason), "tirage du plugin refusé");
    }
    let platform = self.data.platform.clone();
    // Dans un jam, la file est commune : les titres bloqués y sont joués.
    let in_jam = self.jam.is_some();
    let index = result
      .ok()
      .flatten()
      .and_then(|value| serde_json::from_value::<Track>(value).ok())
      .and_then(|pick| {
        self.player.queue.iter().position(|track| {
          track.is_same(&pick, &platform) && (in_jam || !self.data.is_blocked(track))
        })
      });
    let step = match index {
      Some(index) => self.player.play_at(index),
      None => {
        let data = &self.data;
        self
          .player
          .advance(&|track| !in_jam && data.is_blocked(track))
      }
    };
    self.apply_next(step, pending.crossfade)
  }

  /// `queue.append` : ajoute les pistes absentes de la file, renvoie celles ajoutées.
  fn plugin_append(&mut self, tracks: &[Value]) -> Value {
    // Pendant un jam, la file est celle de l'hôte : un plugin n'y ajoute rien.
    if self.jam.is_some() {
      return Value::Array(Vec::new());
    }
    let platform = self.data.platform.clone();
    let mut added = Vec::new();
    for value in tracks {
      let Ok(track) = serde_json::from_value::<Track>(value.clone()) else {
        continue;
      };
      if self
        .player
        .queue
        .iter()
        .any(|queued| queued.is_same(&track, &platform))
      {
        continue;
      }
      added.push(self.plugin_track(&track));
      self.player.queue.push(track);
    }
    Value::Array(added)
  }

  fn plugin_name(&self, file: &str) -> String {
    self
      .plugins
      .names
      .get(file)
      .cloned()
      .unwrap_or_else(|| file.to_string())
  }

  /// Répond à une demande d'un plugin.
  pub fn answer_plugin(&mut self, plugin: &str, request: &HostRequest) -> PluginAnswer {
    let value = match request {
      HostRequest::Track => self
        .player
        .current
        .as_ref()
        .map_or(Value::Null, |track| self.plugin_track(track)),
      HostRequest::IsPlaying => Value::Bool(self.playback.is_playing),
      HostRequest::ShuffleOn => Value::Bool(self.player.shuffle),
      HostRequest::Queue => self.plugin_tracks(self.player.queue.iter()),
      HostRequest::QueueIndex => self.player.index.map_or(Value::Null, Value::from),
      HostRequest::QueueContext => Value::from(self.plugin_context()),
      HostRequest::QueueAppend(tracks) => self.plugin_append(tracks),
      HostRequest::Favorites => self.plugin_tracks(self.data.favorites.iter()),
      HostRequest::History => self.plugin_tracks(self.data.history.iter()),
      HostRequest::Search {
        query,
        limit,
        platform,
      } => {
        return PluginAnswer::Search {
          query: query.clone(),
          limit: *limit,
          platform: platform.clone(),
        };
      }
      HostRequest::Vibe(body) => return PluginAnswer::Vibe(body.clone()),
      HostRequest::Notify { message, kind } => {
        let text = format!("{} : {message}", self.plugin_name(plugin));
        let effects = self.update(crate::app::Message::ShowToast(
          text,
          toast_kind(*kind),
          None,
        ));
        return PluginAnswer::Silent(effects);
      }
      HostRequest::Log(message) => {
        tracing::info!(plugin = %plugin, "{}", riff_core::logging::redact(message));
        return PluginAnswer::Silent(Vec::new());
      }
    };
    PluginAnswer::Now(HostResponse::Value(value), Vec::new())
  }

  /// Changement d'état d'un plugin.
  pub(crate) fn plugin_lifecycle(&mut self, event: &Lifecycle) -> Vec<Effect> {
    match event {
      Lifecycle::Started { file } => {
        self.plugins.errors.remove(file);
        Vec::new()
      }
      Lifecycle::Stopped { file } => {
        self.plugins.names.remove(file);
        Vec::new()
      }
      Lifecycle::Error { file, message } => {
        tracing::warn!(plugin = %file, "{}", riff_core::logging::redact(message));
        self.plugins.errors.insert(file.clone(), message.clone());
        Vec::new()
      }
      Lifecycle::Failed { file, reason } => {
        let text = format!("{} : {reason}", self.plugin_name(file));
        self.plugins.errors.insert(file.clone(), reason.clone());
        self.plugins.names.remove(file);
        self.update(crate::app::Message::ShowToast(text, ToastKind::Error, None))
      }
      Lifecycle::Disabled { file, reason } => {
        let text = format!("{} disabled: {reason}", self.plugin_name(file));
        self.plugins.errors.insert(file.clone(), reason.clone());
        self.plugins.names.remove(file);
        self.plugins.consent.disable(file, reason);
        let mut effects = vec![Effect::SavePluginConsent(Box::new(
          self.plugins.consent.clone(),
        ))];
        effects.extend(self.update(crate::app::Message::ShowToast(
          text,
          ToastKind::Warning,
          None,
        )));
        effects
      }
    }
  }

  fn save_consent(&self) -> Effect {
    Effect::SavePluginConsent(Box::new(self.plugins.consent.clone()))
  }

  fn plugin_toast(&mut self, text: String, kind: ToastKind) -> Vec<Effect> {
    self.update(crate::app::Message::ShowToast(text, kind, None))
  }

  /// Plugins consentis et pas encore actifs : activés (lancement, ou après une relecture).
  fn activate_granted(&mut self, granted: Vec<PluginSource>) -> Vec<Effect> {
    let names = &mut self.plugins.names;
    granted
      .into_iter()
      .filter_map(|source| {
        if names.contains_key(&source.file) {
          return None;
        }
        names.insert(source.file.clone(), source.manifest.name.clone());
        Some(Effect::ActivatePlugin(Box::new(source)))
      })
      .collect()
  }

  /// Messages des plugins (les demandes sont traitées par `Program`, qui a l'hôte).
  pub fn update_plugins(&mut self, message: PluginMessage) -> Vec<Effect> {
    match message {
      PluginMessage::Picked(id, result) => self.plugin_picked(id, result),
      PluginMessage::Scan => vec![Effect::ScanPlugins],
      PluginMessage::Scanned(scan) => {
        let Scan {
          entries,
          consent,
          granted,
        } = *scan;
        self.plugins.entries = entries;
        self.plugins.consent = consent;
        self.activate_granted(granted)
      }
      PluginMessage::Toggle(file, true) => vec![Effect::LoadPlugin(file)],
      PluginMessage::Toggle(file, false) => {
        self.plugins.consent.revoke(&file);
        self.plugins.names.remove(&file);
        vec![
          Effect::DeactivatePlugin(file),
          self.save_consent(),
          Effect::ScanPlugins,
        ]
      }
      PluginMessage::Loaded(Ok(source)) => {
        // Le consentement porte sur la source lue maintenant : celle qui sera exécutée.
        self.plugins.consent.grant(&source);
        self.plugins.errors.remove(&source.file);
        let mut effects = vec![self.save_consent()];
        effects.extend(self.activate_granted(vec![*source]));
        effects.push(Effect::ScanPlugins);
        effects
      }
      PluginMessage::Loaded(Err(reason)) => self.plugin_toast(reason, ToastKind::Error),
      PluginMessage::Remove(file) => {
        self.plugins.names.remove(&file);
        self.plugins.consent.forget(&file);
        vec![
          Effect::DeactivatePlugin(file.clone()),
          self.save_consent(),
          Effect::RemovePlugin(file),
        ]
      }
      PluginMessage::Removed(file, result) => {
        let mut effects = vec![Effect::ScanPlugins];
        if let Err(reason) = result {
          effects.extend(self.plugin_toast(format!("{file} : {reason}"), ToastKind::Error));
        }
        effects
      }
      PluginMessage::Import => vec![Effect::PickPlugins],
      PluginMessage::Imported(results) => {
        let mut effects = vec![Effect::ScanPlugins];
        for result in results {
          let (text, kind) = match result {
            Ok(file) => (format!("Plugin imported: {file}"), ToastKind::Success),
            Err(reason) => (reason, ToastKind::Error),
          };
          effects.extend(self.plugin_toast(text, kind));
        }
        effects
      }
      PluginMessage::OpenFolder => vec![Effect::OpenPluginsFolder],
      PluginMessage::Outgoing(Outgoing::Lifecycle(event)) => self.plugin_lifecycle(&event),
      PluginMessage::Outgoing(Outgoing::Call(call)) => {
        match self.answer_plugin(&call.plugin.clone(), &call.request) {
          PluginAnswer::Now(response, effects) => {
            call.reply(response);
            effects
          }
          PluginAnswer::Silent(effects) => effects,
          // Sans `Program` (tests), la recherche n'a pas de core : réponse vide.
          PluginAnswer::Search { .. } | PluginAnswer::Vibe(_) => {
            call.reply(HostResponse::Value(Value::Array(Vec::new())));
            Vec::new()
          }
        }
      }
    }
  }
}

/// Dossier des plugins natifs (`<données>/plugins`).
#[must_use]
pub fn plugins_dir() -> std::path::PathBuf {
  riff_core::paths::plugins_dir()
}

/// États des plugins : jamais exposés aux scripts.
#[must_use]
pub fn state_dir() -> std::path::PathBuf {
  plugins_dir().join("state")
}

#[must_use]
pub fn consent_path() -> std::path::PathBuf {
  plugins_dir().join(riff_plugins::consent::CONSENT_FILE)
}

/// Messages des fils de plugin, consommés une seule fois par l'abonnement. Aucun fil de relais :
/// le canal est asynchrone, lu par l'exécuteur d'iced.
#[derive(Clone)]
pub struct PluginInbox(Arc<Mutex<Option<mpsc::UnboundedReceiver<Outgoing>>>>);

impl PluginInbox {
  /// Destination à donner à [`riff_plugins::PluginHost::with_sink`], et la boîte qui la lit.
  #[must_use]
  pub fn new() -> (riff_plugins::api::Sink, Self) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let sink: riff_plugins::api::Sink = Arc::new(move |message| sender.send(message).is_ok());
    (sink, Self(Arc::new(Mutex::new(Some(receiver)))))
  }
}

impl Hash for PluginInbox {
  fn hash<H: Hasher>(&self, state: &mut H) {
    Arc::as_ptr(&self.0).hash(state);
  }
}

fn inbox_stream(inbox: &PluginInbox) -> BoxStream<'static, crate::app::Message> {
  let taken = inbox.0.lock().ok().and_then(|mut slot| slot.take());
  match taken {
    Some(receiver) => stream::unfold(receiver, |mut receiver| async move {
      receiver.recv().await.map(|outgoing| {
        (
          crate::app::Message::Plugin(PluginMessage::Outgoing(outgoing)),
          receiver,
        )
      })
    })
    .boxed(),
    None => stream::empty().boxed(),
  }
}

/// Abonnement aux messages des plugins.
pub fn subscription(inbox: PluginInbox) -> iced::Subscription<crate::app::Message> {
  iced::Subscription::run_with(inbox, inbox_stream)
}

/// Pistes d'une réponse du core (`{ success, tracks }`).
#[must_use]
pub fn tracks_of_reply(body: &Value) -> Value {
  match body.get("tracks") {
    Some(tracks @ Value::Array(_)) => tracks.clone(),
    _ if body.is_array() => body.clone(),
    _ => Value::Array(Vec::new()),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::Message;
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::ui::PlayerMessage;
  use crate::user_data::UserData;
  use crate::window::PersistedWindowState;

  fn track(id: &str, artist: &str) -> Track {
    Track {
      id: Some(id.to_string()),
      title: Some(format!("Titre {id}")),
      artist: Some(artist.to_string()),
      duration: Some(200.0),
      platform: Some("youtube".to_string()),
      ..Track::default()
    }
  }

  fn app() -> App {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
    .with_data(UserData {
      autoplay: false,
      ..UserData::default()
    });
    let queue = vec![
      track("a", "A"),
      track("b", "B"),
      track("c", "C"),
      track("d", "D"),
    ];
    app.player.play_list(queue[0].clone(), queue, 0, "youtube");
    app.player.shuffle = true;
    app.plugins.active = true;
    app.plugins.picks = true;
    app.mark_resume_saved();
    app
  }

  fn current(app: &App) -> &str {
    app
      .player
      .current
      .as_ref()
      .and_then(|track| track.id.as_deref())
      .unwrap_or("")
  }

  fn ask_pick(app: &mut App) -> u64 {
    let effects = app.update(Message::Player(PlayerMessage::Next));
    let Some(Effect::PluginPick { id, ctx }) = effects.first() else {
      panic!("attendu : tirage du plugin, {effects:?}");
    };
    assert_eq!(ctx["queue"].as_array().unwrap().len(), 4);
    assert_eq!(ctx["current"]["id"], "a");
    assert_eq!(ctx["queue_index"], 0);
    *id
  }

  #[test]
  fn next_in_shuffle_asks_the_plugin_and_plays_its_pick() {
    let mut app = app();
    let id = ask_pick(&mut app);
    assert_eq!(current(&app), "a", "rien ne change avant la réponse");
    let effects = app.update(Message::Plugin(PluginMessage::Picked(
      id,
      Ok(Some(
        json!({ "id": "c", "platform": "youtube", "title": "Titre c" }),
      )),
    )));
    assert_eq!(current(&app), "c");
    assert_eq!(app.player.index, Some(2));
    assert!(
      effects
        .iter()
        .any(|effect| matches!(effect, Effect::ResolveTrack { .. }))
    );
  }

  #[test]
  fn an_empty_pick_falls_back_to_the_random_shuffle() {
    let mut app = app();
    let id = ask_pick(&mut app);
    app.update(Message::Plugin(PluginMessage::Picked(id, Ok(None))));
    assert_ne!(current(&app), "a");
    assert!(app.plugins.pending_pick.is_none());
  }

  #[test]
  fn an_error_or_a_timeout_falls_back_to_the_random_shuffle() {
    for reason in ["shuffle_pick : raté", "délai dépassé"] {
      let mut app = app();
      let id = ask_pick(&mut app);
      app.update(Message::Plugin(PluginMessage::Picked(
        id,
        Err(reason.to_string()),
      )));
      assert_ne!(current(&app), "a", "{reason}");
    }
  }

  #[test]
  fn a_pick_outside_the_queue_falls_back_and_a_stale_pick_is_ignored() {
    let mut app = app();
    let id = ask_pick(&mut app);
    app.update(Message::Plugin(PluginMessage::Picked(
      id,
      Ok(Some(json!({ "id": "zzz", "platform": "youtube" }))),
    )));
    let after = current(&app).to_string();
    assert_ne!(after, "a");
    let effects = app.update(Message::Plugin(PluginMessage::Picked(
      id,
      Ok(Some(json!({ "id": "b", "platform": "youtube" }))),
    )));
    assert!(effects.is_empty());
    assert_eq!(current(&app), after);
  }

  #[test]
  fn without_a_picking_plugin_shuffle_stays_synchronous() {
    let mut app = app();
    app.plugins.picks = false;
    let effects = app.update(Message::Player(PlayerMessage::Next));
    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::PluginPick { .. }))
    );
    assert_ne!(current(&app), "a");
  }

  #[test]
  fn events_follow_the_track_and_the_play_state_only_when_a_plugin_is_active() {
    let mut app = app();
    app.plugins.active = false;
    assert!(app.plugin_events().is_empty());
    app.plugins.active = true;
    let events = app.plugin_events();
    assert!(matches!(&events[..], [PluginEvent::TrackChange(track)] if track["id"] == "a"));
    assert!(app.plugin_events().is_empty());
    app.playback.is_playing = true;
    assert_eq!(app.plugin_events(), vec![PluginEvent::Play]);
    app.playback.is_playing = false;
    assert_eq!(app.plugin_events(), vec![PluginEvent::Pause]);
  }

  #[test]
  fn queue_append_skips_tracks_already_queued() {
    let mut app = app();
    let answer = app.answer_plugin(
      "p.rhai",
      &HostRequest::QueueAppend(vec![
        json!({ "id": "b", "title": "Déjà là", "platform": "youtube" }),
        json!({ "id": "e", "title": "Nouveau", "platform": "youtube" }),
        json!({ "id": "e", "title": "Nouveau bis", "platform": "youtube" }),
      ]),
    );
    let PluginAnswer::Now(HostResponse::Value(added), _) = answer else {
      panic!("{answer:?}");
    };
    assert_eq!(added.as_array().unwrap().len(), 1);
    assert_eq!(added[0]["id"], "e");
    assert_eq!(app.player.queue.len(), 5);
  }

  #[test]
  fn tracks_given_to_plugins_carry_no_local_path() {
    let mut app = app();
    let mut local = track("x", "L");
    local.platform = Some("local".to_string());
    local.path = Some("C:\\Music\\x.mp3".to_string());
    app.player.queue.push(local);
    let PluginAnswer::Now(HostResponse::Value(queue), _) =
      app.answer_plugin("p.rhai", &HostRequest::Queue)
    else {
      panic!();
    };
    let text = queue.to_string();
    assert!(!text.contains("Music"), "{text}");
  }

  #[test]
  fn a_notification_becomes_a_toast_with_the_plugin_name() {
    let mut app = app();
    app
      .plugins
      .names
      .insert("hello.rhai".into(), "Hello".into());
    let answer = app.answer_plugin(
      "hello.rhai",
      &HostRequest::Notify {
        message: "Now playing: X".into(),
        kind: NotifyKind::Info,
      },
    );
    assert!(matches!(answer, PluginAnswer::Silent(_)));
    assert!(
      app
        .toasts
        .active
        .as_ref()
        .is_some_and(|toast| toast.message == "Hello : Now playing: X")
    );
  }

  fn source(file: &str, code: &str) -> PluginSource {
    riff_plugins::manifest::from_bytes(file, code.as_bytes()).unwrap()
  }

  fn plugin(app: &mut App, message: PluginMessage) -> Vec<Effect> {
    app.update(Message::Plugin(message))
  }

  #[test]
  fn enabling_reads_the_file_again_then_grants_the_consent_and_activates() {
    let mut app = app();
    assert_eq!(
      plugin(&mut app, PluginMessage::Toggle("a.rhai".into(), true)),
      vec![Effect::LoadPlugin("a.rhai".into())]
    );
    let loaded = source("a.rhai", "// @name A\n// @permissions notify\n");
    let effects = plugin(
      &mut app,
      PluginMessage::Loaded(Ok(Box::new(loaded.clone()))),
    );
    assert!(
      matches!(&effects[0], Effect::SavePluginConsent(consent) if consent.enabled["a.rhai"] == loaded.hash)
    );
    assert!(effects.contains(&Effect::ActivatePlugin(Box::new(loaded))));
    assert_eq!(app.plugins.names["a.rhai"], "A");
  }

  #[test]
  fn disabling_revokes_the_consent_and_stops_the_plugin() {
    let mut app = app();
    let loaded = source("a.rhai", "// @name A\n");
    plugin(&mut app, PluginMessage::Loaded(Ok(Box::new(loaded))));
    let effects = plugin(&mut app, PluginMessage::Toggle("a.rhai".into(), false));
    assert_eq!(effects[0], Effect::DeactivatePlugin("a.rhai".into()));
    assert!(
      matches!(&effects[1], Effect::SavePluginConsent(consent) if consent.enabled.is_empty())
    );
    assert!(!app.plugins.names.contains_key("a.rhai"));
  }

  #[test]
  fn a_scan_activates_granted_plugins_once() {
    let mut app = app();
    let granted = source("a.rhai", "// @name A\n");
    let scan = Scan {
      granted: vec![granted.clone()],
      ..Scan::default()
    };
    let effects = plugin(&mut app, PluginMessage::Scanned(Box::new(scan.clone())));
    assert_eq!(effects, vec![Effect::ActivatePlugin(Box::new(granted))]);
    assert!(plugin(&mut app, PluginMessage::Scanned(Box::new(scan))).is_empty());
  }

  #[test]
  fn removing_stops_forgets_and_deletes() {
    let mut app = app();
    plugin(
      &mut app,
      PluginMessage::Loaded(Ok(Box::new(source("a.rhai", "// @name A\n")))),
    );
    let effects = plugin(&mut app, PluginMessage::Remove("a.rhai".into()));
    assert_eq!(effects[0], Effect::DeactivatePlugin("a.rhai".into()));
    assert_eq!(effects[2], Effect::RemovePlugin("a.rhai".into()));
    assert!(app.plugins.consent.enabled.is_empty());
  }

  #[test]
  fn an_import_refusal_is_shown_and_nothing_is_enabled() {
    let mut app = app();
    let effects = plugin(
      &mut app,
      PluginMessage::Imported(vec![
        Ok("good.rhai".into()),
        Err("nom de fichier invalide : évil.rhai".into()),
      ]),
    );
    assert_eq!(effects[0], Effect::ScanPlugins);
    assert!(app.plugins.consent.enabled.is_empty());
    assert!(app.plugins.names.is_empty());
  }

  #[test]
  fn an_automatic_disable_is_remembered_and_shown() {
    let mut app = app();
    let source = riff_plugins::manifest::from_bytes("p.rhai", b"// @name P\n").unwrap();
    app.plugins.consent.grant(&source);
    let effects = app.plugin_lifecycle(&Lifecycle::Disabled {
      file: "p.rhai".into(),
      reason: "3 échecs".into(),
    });
    assert!(
      matches!(&effects[0], Effect::SavePluginConsent(consent) if consent.enabled.is_empty())
    );
    assert_eq!(app.plugins.errors["p.rhai"], "3 échecs");
  }
}
