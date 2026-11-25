//! Le jam vu par `App` : commandes, application chez l'hôte, miroir chez l'invité, diffusion.
//! Aucune E/S : les échanges avec le réseau passent par `Effect::JamSend` et `JamMessage::Event`.

use super::protocol::{
  self, Current, Entry, Notice, NoticeKind, Op, PlaybackState, QueueState, Refusal, RejectReason,
  WireTrack,
};
use super::state::{EntryMeta, HostView, JamState, Phase, Saved, Sent};
use super::{
  EndCause, JamEvent, JamMessage, JamOut, MAX_QUEUE, MAX_TRACKS_PER_OP, Role, SEEK_THRESHOLD, sync,
};
use crate::app::{App, Effect, playback_reset};
use crate::stream::effective_platform;
use crate::track_sort;
use crate::update::{toast_effect, toast_effect_kind};
use crate::user_data::Track;
use crate::widgets::toast::ToastKind;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const BLOCKED: &str = "Not available during a Jam";
const RECONNECTING: &str = "Reconnecting to the Jam…";
const LOCAL_REFUSED: &str = "Local files can't be played in a Jam";

fn millis(duration: Duration) -> u64 {
  u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl App {
  /// Texte traduit dont chaque `{clé}` est remplacée.
  fn jam_text(&self, key: &str, fills: &[(&str, &str)]) -> String {
    fills.iter().fold(self.i18n.t(key), |text, (name, value)| {
      text.replace(&format!("{{{name}}}"), value)
    })
  }

  fn jam_toast(&self, key: &str, fills: &[(&str, &str)]) -> Effect {
    toast_effect(&self.jam_text(key, fills))
  }

  #[must_use]
  pub fn jam_is_guest(&self) -> bool {
    self.jam.as_ref().is_some_and(|jam| jam.is_guest())
  }

  #[must_use]
  pub fn jam_is_host(&self) -> bool {
    self.jam.as_ref().is_some_and(|jam| jam.is_host())
  }

  /// Le battement de 5 s ne tourne que pour un hôte qui joue devant au moins un invité.
  #[must_use]
  pub fn jam_heartbeat_needed(&self) -> bool {
    self.playback.is_playing
      && self
        .jam
        .as_ref()
        .is_some_and(|jam| jam.is_host() && jam.phase == Phase::Live && jam.participants.len() >= 2)
  }

  fn jam_default_name(&self) -> String {
    let nickname = self.settings.nickname.trim();
    if !nickname.is_empty() {
      return nickname.to_string();
    }
    std::env::var("USERNAME")
      .or_else(|_| std::env::var("USER"))
      .ok()
      .map(|name| name.trim().to_string())
      .filter(|name| !name.is_empty())
      .unwrap_or_else(|| protocol::DEFAULT_GUEST_NAME.to_string())
  }

  /// Nom saisi, sinon nom enregistré, sinon nom par défaut.
  fn jam_pick_name(&self) -> String {
    let typed = self.ui.jam_panel.name.trim();
    let saved = self.settings.jam_name.trim();
    let name = if typed.is_empty() { saved } else { typed };
    protocol::sanitize_name(&if name.is_empty() {
      self.jam_default_name()
    } else {
      name.to_string()
    })
  }

  fn jam_relay(&self) -> Option<String> {
    let relay = self.settings.jam_relay.trim();
    (!relay.is_empty()).then(|| relay.to_string())
  }

  fn jam_wire(&self, track: &Track) -> Option<WireTrack> {
    WireTrack::from_track(track, &self.data.platform).and_then(WireTrack::sanitized)
  }

  /// Hors jam : rien. En jam : toast, la fonction appelante s'arrête.
  pub(crate) fn jam_unavailable(&self) -> Option<Vec<Effect>> {
    self
      .jam
      .as_ref()
      .map(|_| vec![self.jam_toast(BLOCKED, &[])])
  }

  /// `uid` du titre courant connu : celui de l'entrée jouée chez l'hôte, celui de l'état reçu chez l'invité.
  pub(crate) fn jam_known_uid(&self) -> Option<u64> {
    let jam = self.jam.as_ref()?;
    if jam.is_guest() {
      return jam.host_uid();
    }
    let index = self.player.index?;
    let current = self.player.current.as_ref()?;
    self
      .player
      .queue
      .get(index)?
      .is_same(current, &self.data.platform)
      .then(|| jam.entries.get(index).map(|entry| entry.uid))?
  }

  pub(crate) fn jam_queue_uid(&self, index: usize) -> Option<u64> {
    self.jam.as_ref()?.entries.get(index).map(|entry| entry.uid)
  }

  /// Hors jam : `None`. Hôte : applique la commande. Invité : l'envoie à l'hôte.
  pub(crate) fn jam_divert(&mut self, op: Op) -> Option<Vec<Effect>> {
    let jam = self.jam.as_mut()?;
    if jam.is_host() {
      return Some(self.jam_apply(0, 0, op));
    }
    if jam.phase != Phase::Live {
      return Some(vec![self.jam_toast(RECONNECTING, &[])]);
    }
    jam.next_cmd += 1;
    let id = jam.next_cmd;
    Some(vec![Effect::JamSend(super::JamOut::Command { id, op })])
  }

  /// Commande portant des titres : les locaux sont retirés avec un toast, rien n'est envoyé sans titre restant.
  fn jam_tracks_op(
    &mut self,
    tracks: Vec<Track>,
    make: impl FnOnce(Vec<WireTrack>) -> Op,
  ) -> Vec<Effect> {
    let total = tracks.len();
    let mut wire: Vec<WireTrack> = tracks
      .iter()
      .filter_map(|track| self.jam_wire(track))
      .collect();
    let skipped = total - wire.len();
    let cut = wire.len().saturating_sub(MAX_TRACKS_PER_OP);
    wire.truncate(MAX_TRACKS_PER_OP);
    let mut effects = Vec::new();
    if skipped == 1 && total == 1 {
      effects.push(self.jam_toast(LOCAL_REFUSED, &[]));
    } else if skipped > 0 {
      effects.push(self.jam_toast(
        "{count} local files skipped",
        &[("count", &skipped.to_string())],
      ));
    }
    if cut > 0 {
      effects.push(self.jam_toast(
        "{count} tracks over the limit were skipped",
        &[("count", &cut.to_string())],
      ));
    }
    if !wire.is_empty() {
      effects.extend(self.jam_divert(make(wire)).unwrap_or_default());
    }
    effects
  }

  pub(crate) fn jam_add(&mut self, tracks: Vec<Track>, next: bool) -> Vec<Effect> {
    self.jam_tracks_op(tracks, |tracks| Op::Add { tracks, next })
  }

  pub(crate) fn jam_play_now(&mut self, tracks: Vec<Track>) -> Vec<Effect> {
    self.jam_tracks_op(tracks, |tracks| Op::PlayNow { tracks })
  }

  /// Retrait d'entrées de la file, par index.
  pub(crate) fn jam_remove_indices(&mut self, indices: &[usize]) -> Vec<Effect> {
    let uids: Vec<u64> = indices
      .iter()
      .filter_map(|&index| self.jam_queue_uid(index))
      .collect();
    if uids.is_empty() {
      return Vec::new();
    }
    self.jam_divert(Op::Remove { uids }).unwrap_or_default()
  }

  pub(crate) fn jam_toggle_play(&mut self) -> Option<Vec<Effect>> {
    let jam = self.jam.as_ref()?;
    let playing = if jam.is_host() {
      self.playback.is_playing
    } else {
      jam.host.as_ref().is_some_and(|view| view.state.playing)
    };
    self.jam_divert(if playing { Op::Pause } else { Op::Play })
  }

  pub(crate) fn jam_step(&mut self, back: bool) -> Option<Vec<Effect>> {
    self.jam.as_ref()?;
    let from = self.jam_known_uid();
    self.jam_divert(if back {
      Op::Prev { from }
    } else {
      Op::Next { from }
    })
  }

  pub(crate) fn jam_seek(&mut self, target: Duration) -> Option<Vec<Effect>> {
    self.jam.as_ref()?;
    self.jam_divert(Op::Seek {
      pos_ms: millis(target),
    })
  }

  // ---- Hôte : application des commandes ----

  /// Applique la commande de `by` (0 : l'hôte). Un refus part à l'invité, ou devient un toast chez l'hôte.
  pub(crate) fn jam_apply(&mut self, by: u32, id: u64, op: Op) -> Vec<Effect> {
    match self.jam_exec(by, op) {
      Ok(effects) => effects,
      Err(reason) if by == 0 => self.jam_reject_toast(reason).into_iter().collect(),
      Err(reason) => vec![Effect::JamSend(JamOut::Reject { to: by, id, reason })],
    }
  }

  fn jam_reject_toast(&self, reason: RejectReason) -> Option<Effect> {
    match reason {
      RejectReason::LocalFile => Some(self.jam_toast(LOCAL_REFUSED, &[])),
      RejectReason::QueueFull | RejectReason::TooMany => {
        Some(self.jam_toast("The Jam queue is full", &[]))
      }
      other => {
        tracing::debug!(?other, "commande du jam refusée");
        None
      }
    }
  }

  fn jam_notice(
    &self,
    kind: NoticeKind,
    by: u32,
    name: &str,
    title: Option<String>,
    count: Option<u32>,
  ) -> Vec<Effect> {
    let notice = Notice {
      kind,
      by,
      name: name.to_string(),
      title,
      count,
    };
    let mut effects = Vec::new();
    if by != 0 {
      effects.push(toast_effect(&self.jam_notice_text(&notice)));
    }
    effects.push(Effect::JamSend(JamOut::Notice(notice)));
    effects
  }

  fn jam_notice_text(&self, notice: &Notice) -> String {
    let name = notice.name.as_str();
    match notice.kind {
      NoticeKind::Joined => self.jam_text("{name} joined the Jam", &[("name", name)]),
      NoticeKind::Left => self.jam_text("{name} left the Jam", &[("name", name)]),
      NoticeKind::Paused => self.jam_text("{name} paused", &[("name", name)]),
      NoticeKind::Resumed => self.jam_text("{name} resumed playback", &[("name", name)]),
      NoticeKind::ResolveFailed => {
        self.jam_text("{name} couldn't play this track", &[("name", name)])
      }
      NoticeKind::Added => match (&notice.title, notice.count) {
        (Some(title), _) => {
          self.jam_text("{name} added {title}", &[("name", name), ("title", title)])
        }
        (None, count) => self.jam_text(
          "{name} added {count} tracks",
          &[("name", name), ("count", &count.unwrap_or(0).to_string())],
        ),
      },
    }
  }

  /// Pistes d'une commande, assainies ; refus avant toute modification.
  fn jam_validate(&self, tracks: Vec<WireTrack>) -> Result<Vec<WireTrack>, RejectReason> {
    if tracks.is_empty() {
      return Err(RejectReason::Invalid);
    }
    if tracks.len() > MAX_TRACKS_PER_OP {
      return Err(RejectReason::TooMany);
    }
    let mut valid = Vec::with_capacity(tracks.len());
    for track in tracks {
      if track.platform == "local" {
        return Err(RejectReason::LocalFile);
      }
      valid.push(track.sanitized().ok_or(RejectReason::Invalid)?);
    }
    if self.player.queue.len() + valid.len() > MAX_QUEUE {
      return Err(RejectReason::QueueFull);
    }
    let mut queue = self.jam_wire_queue();
    queue
      .entries
      .extend(valid.iter().cloned().map(|track| Entry {
        uid: u64::MAX,
        by: 0,
        by_name: protocol::DEFAULT_GUEST_NAME.to_string(),
        track,
      }));
    if !queue.fits_in_frame() {
      return Err(RejectReason::QueueFull);
    }
    Ok(valid)
  }

  /// Insère à `at` dans la file et dans les méta, en gardant l'index et l'historique cohérents.
  fn jam_insert(&mut self, at: usize, by: u32, name: &str, tracks: Vec<WireTrack>) -> usize {
    let at = at.min(self.player.queue.len());
    let count = tracks.len();
    if let Some(jam) = self.jam.as_mut() {
      let metas: Vec<EntryMeta> = (0..count)
        .map(|_| EntryMeta {
          uid: jam.fresh_uid(),
          by,
          by_name: name.to_string(),
        })
        .collect();
      jam.entries.splice(at..at, metas);
    }
    self
      .player
      .queue
      .splice(at..at, tracks.into_iter().map(WireTrack::into_track));
    for index in &mut self.player.queue_history {
      if *index >= at {
        *index += count;
      }
    }
    if let Some(index) = self.player.index.as_mut()
      && *index >= at
    {
      *index += count;
    }
    at
  }

  fn jam_remove(&mut self, indices: &[usize]) {
    let removed: HashSet<usize> = indices.iter().copied().collect();
    self.player.remove_from_queue(indices);
    if let Some(jam) = self.jam.as_mut() {
      let mut position = 0;
      jam.entries.retain(|_| {
        let keep = !removed.contains(&position);
        position += 1;
        keep
      });
    }
  }

  fn jam_play_at(&mut self, index: usize) -> Vec<Effect> {
    let step = self.player.play_at(index);
    self.apply_next(step, None)
  }

  fn jam_added(&self, by: u32, name: &str, tracks: &[WireTrack]) -> Vec<Effect> {
    let title = (tracks.len() == 1)
      .then(|| tracks[0].title.clone())
      .flatten();
    let count = u32::try_from(tracks.len()).ok();
    self.jam_notice(NoticeKind::Added, by, name, title, count)
  }

  fn jam_exec(&mut self, by: u32, op: Op) -> Result<Vec<Effect>, RejectReason> {
    let Some(jam) = self.jam.as_ref() else {
      return Ok(Vec::new());
    };
    let name = jam.name_of(by);
    match op {
      Op::Play => {
        let mut effects = if self.player.current.is_none() {
          if self.player.queue.is_empty() {
            return Ok(Vec::new());
          }
          self.jam_play_at(0)
        } else if self.playback.is_playing || self.player.loading {
          return Ok(Vec::new());
        } else {
          self.toggle_play()
        };
        effects.extend(self.jam_notice(NoticeKind::Resumed, by, &name, None, None));
        Ok(effects)
      }
      Op::Pause => {
        if !self.playback.is_playing || self.player.loading {
          return Ok(Vec::new());
        }
        let mut effects = self.toggle_play();
        effects.extend(self.jam_notice(NoticeKind::Paused, by, &name, None, None));
        Ok(effects)
      }
      Op::Seek { pos_ms } => {
        if self.player.current.is_none() || self.player.loading {
          return Ok(Vec::new());
        }
        let mut target = Duration::from_millis(pos_ms);
        let duration = self.duration_secs();
        if duration > 0.0 {
          target = target.min(Duration::from_secs_f64(duration));
        }
        self.playback.position = target;
        Ok(vec![Effect::AudioSeek(target)])
      }
      Op::Next { from } => {
        if from.is_some() && from != self.jam_known_uid() {
          return Ok(Vec::new());
        }
        Ok(self.go_next(false, None))
      }
      Op::Prev { from } => {
        if from.is_some() && from != self.jam_known_uid() {
          return Ok(Vec::new());
        }
        Ok(self.go_prev())
      }
      Op::Add { tracks, next } => {
        let tracks = self.jam_validate(tracks)?;
        let at = if next {
          self.player.index.map_or(0, |index| index + 1)
        } else {
          self.player.queue.len()
        };
        let mut effects = self.jam_added(by, &name, &tracks);
        let at = self.jam_insert(at, by, &name, tracks);
        if self.player.current.is_none() {
          effects.extend(self.jam_play_at(at));
        }
        Ok(effects)
      }
      Op::PlayNow { tracks } => {
        let tracks = self.jam_validate(tracks)?;
        let at = self.player.index.map_or(0, |index| index + 1);
        let mut effects = self.jam_added(by, &name, &tracks);
        let at = self.jam_insert(at, by, &name, tracks);
        effects.extend(self.jam_play_at(at));
        Ok(effects)
      }
      Op::PlayEntry { uid } => {
        let index = jam.index_of(uid).ok_or(RejectReason::UnknownEntry)?;
        Ok(self.jam_play_at(index))
      }
      Op::Remove { uids } => {
        if uids.is_empty() {
          return Ok(Vec::new());
        }
        let indices: Vec<usize> = uids.iter().filter_map(|&uid| jam.index_of(uid)).collect();
        if indices.is_empty() {
          return Err(RejectReason::UnknownEntry);
        }
        self.jam_remove(&indices);
        Ok(Vec::new())
      }
      Op::Move { uid, to } => {
        let from = jam.index_of(uid).ok_or(RejectReason::UnknownEntry)?;
        let to = usize::try_from(to)
          .unwrap_or(usize::MAX)
          .min(self.player.queue.len().saturating_sub(1));
        self.player.move_in_queue(from, to);
        if let Some(jam) = self.jam.as_mut() {
          track_sort::move_item(&mut jam.entries, from, to);
        }
        Ok(Vec::new())
      }
      Op::Clear => {
        let keep = self.player.index;
        let indices: Vec<usize> = (0..self.player.queue.len())
          .filter(|&index| Some(index) != keep)
          .collect();
        self.jam_remove(&indices);
        Ok(Vec::new())
      }
      Op::Shuffle { on } => {
        self.player.shuffle = on;
        Ok(Vec::new())
      }
      Op::Repeat { on } => {
        self.player.repeat = on;
        Ok(Vec::new())
      }
    }
  }

  // ---- Hôte : compteurs et diffusion ----

  /// `start_current` : un titre démarre chez l'hôte.
  pub(crate) fn jam_on_start(&mut self, crossfade: bool) {
    if let Some(jam) = self.jam.as_mut().filter(|jam| jam.is_host()) {
      jam.start += 1;
      jam.pending_fade = crossfade;
      jam.position_at = None;
    }
  }

  /// Répétition : le même titre repart chez l'hôte.
  pub(crate) fn jam_on_restart(&mut self) {
    if let Some(jam) = self.jam.as_mut().filter(|jam| jam.is_host()) {
      jam.start += 1;
      jam.position_at = None;
    }
  }

  /// Position publiée par le moteur.
  pub(crate) fn jam_on_position(&mut self) {
    if let Some(jam) = self.jam.as_mut().filter(|jam| jam.is_host()) {
      jam.position_at = Some(Instant::now());
    }
  }

  /// Le titre est chargé chez l'hôte.
  pub(crate) fn jam_on_loaded(&mut self) {
    if let Some(jam) = self.jam.as_mut().filter(|jam| jam.is_host()) {
      jam.failures = 0;
    }
  }

  /// Échec de résolution. Hôte : tente le titre suivant, trois fois au plus de suite. Invité : le dit à l'hôte.
  pub(crate) fn jam_resolve_failed(&mut self) -> Vec<Effect> {
    let uid = self.jam_known_uid();
    let Some(jam) = self.jam.as_mut() else {
      return Vec::new();
    };
    if jam.is_host() {
      if jam.failures >= 3 {
        return Vec::new();
      }
      jam.failures += 1;
      return self.go_next(false, None);
    }
    let mut effects =
      vec![self.jam_toast("Couldn't play this track, waiting for the next one", &[])];
    if let Some(uid) = uid {
      effects.push(Effect::JamSend(JamOut::Report { uid }));
    }
    effects
  }

  fn jam_wire_queue(&self) -> QueueState {
    let Some(jam) = self.jam.as_ref() else {
      return QueueState::default();
    };
    let entries = jam
      .entries
      .iter()
      .zip(&self.player.queue)
      .filter_map(|(meta, track)| {
        WireTrack::from_track(track, &self.data.platform).map(|track| Entry {
          uid: meta.uid,
          by: meta.by,
          by_name: meta.by_name.clone(),
          track,
        })
      })
      .collect();
    QueueState {
      qv: jam.qv,
      entries,
    }
  }

  /// Après chaque message : l'hôte diffuse ce qui a changé.
  pub(crate) fn jam_after_update(&mut self, mut effects: Vec<Effect>) -> Vec<Effect> {
    let Some(jam) = self.jam.as_ref() else {
      return effects;
    };
    if !jam.is_host() || jam.phase != Phase::Live {
      return effects;
    }
    debug_assert_eq!(jam.entries.len(), self.player.queue.len());
    self.jam_repair_entries();
    let now = Instant::now();
    let seeks = effects
      .iter()
      .filter(|effect| matches!(effect, Effect::AudioSeek(_)))
      .count() as u64;
    let uid = self.jam_known_uid();
    let playing = self.playback.is_playing && !self.player.loading;
    let (shuffle, repeat) = (self.player.shuffle, self.player.repeat);
    let position = self.playback.position;
    let current = self
      .player
      .current
      .as_ref()
      .and_then(|track| WireTrack::from_track(track, &self.data.platform))
      .map(|track| Current { uid, track });
    let Some(jam) = self.jam.as_ref() else {
      return effects;
    };
    let uids: Vec<u64> = jam.entries.iter().map(|entry| entry.uid).collect();
    let queue_changed = jam.sent_queue.as_deref() != Some(uids.as_slice());
    let mut queue = queue_changed.then(|| self.jam_wire_queue());
    let Some(jam) = self.jam.as_mut() else {
      return effects;
    };
    jam.seek += seeks;
    if seeks > 0 {
      jam.position_at = Some(now);
    }
    if playing && !jam.sent.is_some_and(|sent| sent.playing) {
      jam.position_at = Some(now);
    }
    let sent = Sent {
      uid,
      start: jam.start,
      seek: jam.seek,
      playing,
      shuffle,
      repeat,
    };
    if let Some(queue) = queue.as_mut() {
      jam.qv += 1;
      queue.qv = jam.qv;
      jam.sent_queue = Some(uids);
      effects.push(Effect::JamSend(JamOut::Queue(queue.clone())));
    }
    if jam.sent != Some(sent) || jam.force {
      jam.pv += 1;
      let state = PlaybackState {
        pv: jam.pv,
        current,
        start: sent.start,
        seek: sent.seek,
        playing,
        pos_ms: millis(sync::host_position(position, jam.position_at, playing, now)),
        at_ms: 0,
        shuffle,
        repeat,
        fade: jam.pending_fade,
      };
      jam.sent = Some(sent);
      jam.force = false;
      jam.pending_fade = false;
      effects.push(Effect::JamSend(JamOut::Playback {
        state: Box::new(state),
        at: now,
      }));
    }
    effects
  }

  /// Filet de sécurité en production : `entries` suit toujours `player.queue`.
  fn jam_repair_entries(&mut self) {
    let len = self.player.queue.len();
    let name = self
      .jam
      .as_ref()
      .map(|jam| jam.name_of(0))
      .unwrap_or_default();
    let Some(jam) = self.jam.as_mut() else {
      return;
    };
    jam.entries.truncate(len);
    while jam.entries.len() < len {
      let uid = jam.fresh_uid();
      jam.entries.push(EntryMeta {
        uid,
        by: 0,
        by_name: name.clone(),
      });
    }
  }

  // ---- Messages ----

  pub(crate) fn update_jam(&mut self, message: JamMessage) -> Vec<Effect> {
    match message {
      JamMessage::Event(_, event) => self.jam_event(*event),
      JamMessage::TaskEnded(_, cause) => {
        let Some(jam) = self.jam.as_ref() else {
          return Vec::new();
        };
        let cause = if jam.stopping {
          EndCause::Stopped
        } else {
          cause
        };
        self.jam_ended(cause)
      }
      JamMessage::Heartbeat => {
        if let Some(jam) = self.jam.as_mut() {
          jam.force = true;
        }
        Vec::new()
      }
      JamMessage::TogglePanel => {
        let was_open = self.ui.jam_panel.open;
        self.ui.jam_panel.open = !was_open;
        if self.view_motion.enabled {
          self
            .ui
            .jam_panel
            .motion
            .set_open(was_open, !was_open, self.now);
        }
        let queue_open = self.ui.queue_panel.open;
        if !was_open && queue_open {
          self.ui.queue_panel.open = false;
          if self.view_motion.enabled {
            self.ui.queue_panel.motion.set_open(true, false, self.now);
          }
        }
        Vec::new()
      }
      JamMessage::Start => self.jam_start(),
      JamMessage::CodeInput(code) => {
        self.ui.jam_panel.code = code;
        Vec::new()
      }
      JamMessage::NameInput(name) => {
        self.ui.jam_panel.name = name;
        Vec::new()
      }
      JamMessage::Paste => vec![Effect::JamPaste],
      JamMessage::Pasted(text) => {
        if let Some(text) = text {
          self.ui.jam_panel.code = text.trim().to_string();
        }
        Vec::new()
      }
      JamMessage::Join => self.jam_join(),
      JamMessage::CopyCode => {
        let Some(ticket) = self.jam.as_ref().and_then(|jam| jam.ticket.clone()) else {
          return Vec::new();
        };
        vec![
          Effect::CopyText(ticket),
          toast_effect_kind(&self.i18n.t("Code copied"), ToastKind::Success),
        ]
      }
      JamMessage::Leave | JamMessage::End => {
        let Some(jam) = self.jam.as_mut() else {
          return Vec::new();
        };
        jam.stopping = true;
        vec![Effect::JamSend(JamOut::Stop)]
      }
    }
  }

  /// Coupe radio et autoplay en attente : rien ne doit allonger la file pendant un jam.
  fn jam_reset_auto(&mut self) {
    self.ui.autoplay_waiting = false;
    self.ui.autoplay_loading = false;
    self.ui.autoplay_found = None;
    self.ui.radio_waiting = false;
    self.ui.radio_loading = false;
  }

  fn jam_start(&mut self) -> Vec<Effect> {
    if self.jam.is_some() {
      return Vec::new();
    }
    let name = self.jam_pick_name();
    self.player.radio = false;
    self.player.my_wave = false;
    self.jam_reset_auto();
    let mut effects = Vec::new();
    let unusable: Vec<usize> = self
      .player
      .queue
      .iter()
      .enumerate()
      .filter(|(_, track)| self.jam_wire(track).is_none())
      .map(|(index, _)| index)
      .collect();
    self.player.remove_from_queue(&unusable);
    if !unusable.is_empty() {
      effects.push(self.jam_toast(
        "{count} local files skipped",
        &[("count", &unusable.len().to_string())],
      ));
    }
    let overflow: Vec<usize> = (MAX_QUEUE..self.player.queue.len()).collect();
    self.player.remove_from_queue(&overflow);
    if self
      .player
      .current
      .as_ref()
      .is_some_and(|track| track.is_local())
    {
      effects.push(Effect::AudioPause);
      self.player.current = None;
      self.player.index = None;
      self.playback.is_playing = false;
    }
    // Plateforme figée : changer la plateforme de recherche pendant le jam ne touche pas les titres déjà en file.
    let session = self.data.platform.clone();
    for track in self
      .player
      .queue
      .iter_mut()
      .chain(self.player.current.as_mut())
    {
      if track.platform.as_deref().is_none_or(str::is_empty) {
        let platform = effective_platform(track, &session);
        track.platform = (!platform.is_empty()).then_some(platform);
      }
    }
    let mut jam = JamState::new(Role::Host, &name);
    for _ in 0..self.player.queue.len() {
      let uid = jam.fresh_uid();
      jam.entries.push(EntryMeta {
        uid,
        by: 0,
        by_name: name.clone(),
      });
    }
    self.jam = Some(Box::new(jam));
    if !self.ui.jam_panel.name.trim().is_empty() && self.settings.jam_name != name {
      self.settings.jam_name.clone_from(&name);
      effects.extend(self.save_settings());
    }
    effects.push(Effect::JamHost {
      name,
      relay: self.jam_relay(),
    });
    effects
  }

  fn jam_join(&mut self) -> Vec<Effect> {
    let ticket = self.ui.jam_panel.code.trim().to_string();
    if self.jam.is_some() || ticket.is_empty() {
      return Vec::new();
    }
    let name = self.jam_pick_name();
    self.jam = Some(Box::new(JamState::new(Role::Guest, &name)));
    let mut effects = Vec::new();
    if !self.ui.jam_panel.name.trim().is_empty() && self.settings.jam_name != name {
      self.settings.jam_name.clone_from(&name);
      effects.extend(self.save_settings());
    }
    effects.push(Effect::JamJoin {
      ticket,
      name,
      relay: self.jam_relay(),
    });
    effects
  }

  fn jam_event(&mut self, event: JamEvent) -> Vec<Effect> {
    if self.jam.is_none() {
      return Vec::new();
    }
    match event {
      JamEvent::Hosting { ticket } => {
        if let Some(jam) = self.jam.as_mut() {
          jam.phase = Phase::Live;
          jam.ticket = Some(ticket);
          jam.sent = None;
          jam.sent_queue = None;
        }
        Vec::new()
      }
      JamEvent::RelayUnreachable => {
        vec![toast_effect_kind(
          &self
            .i18n
            .t("Relay unreachable: only direct connections will work"),
          ToastKind::Error,
        )]
      }
      JamEvent::Joined {
        you,
        participants,
        queue,
        playback,
        anchor,
      } => self.jam_joined(you, participants, queue, playback, anchor),
      JamEvent::Participants(participants) => {
        if let Some(jam) = self.jam.as_mut() {
          jam.participants = participants;
        }
        Vec::new()
      }
      JamEvent::GuestOp { from, id, op } if self.jam_is_host() => self.jam_apply(from, id, op),
      JamEvent::GuestReport { from, uid: _ } if self.jam_is_host() => {
        let name = self
          .jam
          .as_ref()
          .map(|jam| jam.name_of(from))
          .unwrap_or_default();
        self.jam_notice(NoticeKind::ResolveFailed, from, &name, None, None)
      }
      JamEvent::Queue(queue) if self.jam_is_guest() => {
        let uid = self.jam.as_ref().and_then(|jam| jam.host_uid());
        self.jam_mirror_queue(queue, uid);
        Vec::new()
      }
      JamEvent::Playback { state, anchor } if self.jam_is_guest() => {
        self.jam_guest_playback(state, anchor)
      }
      JamEvent::Notice(notice) => {
        let me = self.jam.as_ref().map_or(0, |jam| jam.me);
        if notice.by == me {
          Vec::new()
        } else {
          vec![toast_effect(&self.jam_notice_text(&notice))]
        }
      }
      JamEvent::Rejected { reason, .. } => self.jam_reject_toast(reason).into_iter().collect(),
      JamEvent::Reconnecting { attempt } => {
        if let Some(jam) = self.jam.as_mut() {
          jam.phase = Phase::Reconnecting(attempt);
        }
        Vec::new()
      }
      JamEvent::Ended(cause) => self.jam_ended(cause),
      JamEvent::GuestOp { .. }
      | JamEvent::GuestReport { .. }
      | JamEvent::Queue(_)
      | JamEvent::Playback { .. } => Vec::new(),
    }
  }

  /// Fin du jam, quelle qu'en soit la cause. L'invité retrouve sa file d'avant, en pause.
  fn jam_ended(&mut self, cause: EndCause) -> Vec<Effect> {
    let Some(mut jam) = self.jam.take() else {
      return Vec::new();
    };
    let mut effects = Vec::new();
    if let Some(saved) = jam.saved.take() {
      let token = self.player.token.max(saved.player.token) + 1;
      let crossfade = self.player.crossfade;
      self.player = saved.player;
      self.player.token = token;
      self.player.loading = false;
      self.player.crossfade = crossfade;
      self.playback = playback_reset(&self.playback);
      effects.push(Effect::AudioPause);
      effects.push(self.state_effect(false));
    }
    // Hôte : le titre en cours, joué malgré son blocage pendant le jam, cède la place au suivant.
    let current_blocked = self
      .player
      .current
      .as_ref()
      .is_some_and(|track| self.data.is_blocked(track));
    if current_blocked && self.playback.is_playing {
      effects.extend(self.go_next(false, None));
    }
    let text = match cause {
      EndCause::Stopped => None,
      EndCause::HostEnded => Some("The host ended the Jam"),
      EndCause::Lost => Some("Connection to the Jam was lost"),
      EndCause::InvalidTicket => Some("Invalid invite code"),
      EndCause::Refused(Refusal::Room) => Some("Wrong or expired invite code"),
      EndCause::Refused(Refusal::Full) => Some("This Jam is full"),
      EndCause::Refused(Refusal::Version) => Some("This Jam uses another version of Riff"),
      EndCause::Transport(_) if jam.is_host() && jam.phase == Phase::Starting => {
        Some("Could not start the Jam")
      }
      EndCause::Transport(_) => Some("Connection to the Jam was lost"),
    };
    if let Some(text) = text {
      let kind = if matches!(cause, EndCause::HostEnded) {
        ToastKind::Info
      } else {
        ToastKind::Error
      };
      effects.push(toast_effect_kind(&self.i18n.t(text), kind));
    }
    effects
  }

  // ---- Invité : miroir ----

  fn jam_joined(
    &mut self,
    you: u32,
    participants: Vec<protocol::Participant>,
    queue: QueueState,
    playback: PlaybackState,
    anchor: Instant,
  ) -> Vec<Effect> {
    let Some(jam) = self.jam.as_mut() else {
      return Vec::new();
    };
    let first = jam.phase == Phase::Starting;
    jam.me = you;
    jam.participants = participants;
    jam.phase = Phase::Live;
    if jam.saved.is_none() {
      jam.saved = Some(Box::new(Saved {
        player: self.player.clone(),
      }));
    }
    let host = jam.host_name();
    let mut effects = Vec::new();
    if first {
      self.player.radio = false;
      self.player.my_wave = false;
      self.jam_reset_auto();
      effects.push(self.jam_toast("You joined {name}'s Jam", &[("name", &host)]));
    }
    self.jam_mirror_queue(queue, playback.current.as_ref().and_then(|c| c.uid));
    effects.extend(self.jam_guest_playback(playback, anchor));
    effects
  }

  /// La file de l'hôte devient la file locale ; le titre en cours n'est pas touché.
  fn jam_mirror_queue(&mut self, queue: QueueState, current_uid: Option<u64>) {
    let mut tracks = Vec::new();
    let mut entries = Vec::new();
    for entry in queue.entries.into_iter().take(MAX_QUEUE) {
      if let Some(track) = entry.track.sanitized() {
        tracks.push(track.into_track());
        entries.push(EntryMeta {
          uid: entry.uid,
          by: entry.by,
          by_name: protocol::sanitize_name(&entry.by_name),
        });
      }
    }
    let Some(jam) = self.jam.as_mut() else {
      return;
    };
    jam.entries = entries;
    let index = current_uid.and_then(|uid| jam.index_of(uid));
    self.player.queue = tracks;
    self.player.index = index;
    self.player.queue_history = index.into_iter().collect();
  }

  fn jam_expected(&self, state: &PlaybackState, anchor: Instant, now: Instant) -> Duration {
    let duration = self.duration_secs();
    sync::expected_position(
      state.pos_ms,
      anchor,
      state.playing,
      now,
      (duration > 0.0).then(|| Duration::from_secs_f64(duration)),
    )
  }

  fn jam_guest_playback(&mut self, state: PlaybackState, anchor: Instant) -> Vec<Effect> {
    let now = Instant::now();
    let Some(jam) = self.jam.as_mut() else {
      return Vec::new();
    };
    let prev = jam.host.as_ref().map(|view| view.state.clone());
    if prev.as_ref().is_some_and(|prev| state.pv < prev.pv) {
      return Vec::new();
    }
    jam.host = Some(HostView {
      state: state.clone(),
      anchor,
    });
    self.player.shuffle = state.shuffle;
    self.player.repeat = state.repeat;
    let uid = |state: &PlaybackState| state.current.as_ref().and_then(|current| current.uid);
    let Some(prev) = prev.filter(|prev| prev.start == state.start && uid(prev) == uid(&state))
    else {
      return self.jam_guest_start(&state, anchor, now);
    };
    if self.player.loading {
      return Vec::new();
    }
    let expected = self.jam_expected(&state, anchor, now);
    let mut effects = Vec::new();
    let mut sought = false;
    let Some(jam) = self.jam.as_mut() else {
      return effects;
    };
    if state.seek != prev.seek {
      effects.push(Effect::AudioSeek(expected));
      self.playback.position = expected;
      jam.last_correction = Some(now);
      sought = true;
    }
    let local = self.playback.position;
    match (prev.playing, state.playing) {
      (false, true) => {
        if !sought && sync::needs_correction(local, expected, jam.last_correction, now) {
          effects.push(Effect::AudioSeek(expected));
          jam.last_correction = Some(now);
        }
        effects.push(Effect::AudioPlay);
        self.playback.is_playing = true;
        effects.push(self.state_effect(true));
      }
      (true, false) => {
        effects.push(Effect::AudioPause);
        self.playback.is_playing = false;
        let position = Duration::from_millis(state.pos_ms);
        if !sought && local.abs_diff(position) > SEEK_THRESHOLD {
          effects.push(Effect::AudioSeek(position));
          self.playback.position = position;
        }
        effects.push(self.state_effect(false));
      }
      _ => {
        if state.playing
          && self.playback.is_playing
          && !sought
          && sync::needs_correction(local, expected, jam.last_correction, now)
        {
          effects.push(Effect::AudioSeek(expected));
          jam.last_correction = Some(now);
        }
      }
    }
    effects
  }

  /// L'hôte a changé de titre : l'invité le démarre à la position attendue.
  fn jam_guest_start(
    &mut self,
    state: &PlaybackState,
    anchor: Instant,
    now: Instant,
  ) -> Vec<Effect> {
    let current = state.current.as_ref().and_then(|current| {
      current
        .track
        .clone()
        .sanitized()
        .map(|track| (current.uid, track.into_track()))
    });
    let Some((uid, track)) = current else {
      self.player.token += 1;
      self.player.loading = false;
      self.player.current = None;
      self.player.index = None;
      self.playback.is_playing = false;
      return vec![Effect::AudioPause, self.state_effect(false)];
    };
    let index = uid.and_then(|uid| self.jam.as_ref().and_then(|jam| jam.index_of(uid)));
    let duration = track
      .duration
      .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
      .map(Duration::from_secs_f64);
    let expected = sync::expected_position(state.pos_ms, anchor, state.playing, now, duration);
    self.player.play_external(track, index);
    if let Some(jam) = self.jam.as_mut() {
      jam.last_correction = None;
    }
    if state.fade && self.player.crossfade > 0 && self.playback.is_playing {
      self.start_current(Some(self.player.crossfade))
    } else {
      self.start_current_at(expected)
    }
  }

  /// `Position` du moteur chez l'invité : recale si l'écart dépasse le seuil.
  pub(crate) fn jam_guest_position(&mut self, position: Duration) -> Option<Effect> {
    let now = Instant::now();
    let jam = self
      .jam
      .as_ref()
      .filter(|jam| jam.is_guest() && jam.phase == Phase::Live)?;
    let view = jam.host.as_ref()?;
    if !view.state.playing || !self.playback.is_playing {
      return None;
    }
    let expected = self.jam_expected(&view.state, view.anchor, now);
    if !sync::needs_correction(position, expected, jam.last_correction, now) {
      return None;
    }
    self.jam.as_mut()?.last_correction = Some(now);
    Some(Effect::AudioSeek(expected))
  }

  /// `Loaded` chez l'invité : se cale sur l'état de l'hôte.
  pub(crate) fn jam_guest_loaded(&mut self) -> Vec<Effect> {
    let now = Instant::now();
    let Some(view) = self
      .jam
      .as_ref()
      .filter(|jam| jam.is_guest())
      .and_then(|jam| jam.host.clone())
    else {
      return Vec::new();
    };
    if !view.state.playing {
      self.playback.is_playing = false;
      return vec![Effect::AudioPause, self.state_effect(false)];
    }
    let expected = self.jam_expected(&view.state, view.anchor, now);
    if sync::needs_correction(self.playback.position, expected, None, now) {
      vec![Effect::AudioSeek(expected)]
    } else {
      Vec::new()
    }
  }
}
