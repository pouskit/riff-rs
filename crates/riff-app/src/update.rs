//! Mise à jour des écrans et du lecteur, sans E/S : tout passe par des [`Effect`].

use crate::app::{App, Effect, Message, MixJob, SourceEffect, playback_reset};
use crate::jam::protocol::Op;
use crate::library::{self, FAVORITES_SCOPE};
use crate::player::{EndStep, MAX_STREAM_RETRIES, NextStep, PrevStep};
use crate::prefs::Category;
use crate::screens::Screen;
use crate::stream::ResolveOptions;
use crate::theme::{icons, tokens};
use crate::track_sort;
use crate::ui::{
  ArtistCard, ArtistTab, ListId, Modal, PlayerMessage, ReplyTag, RowPress, SearchFilter,
  SidebarDrag, SidebarTab, TrackCommand, UiMessage,
};
use crate::user_data::{DailyMix, DailyMixes, SavedAlbum, SortMode, Track, TrackCache};
use crate::widgets::menu::{Menu, MenuItem};
use crate::widgets::toast::ToastKind;
use riff_audio::{AudioError, Event as AudioEvent};
use riff_core::util::{get_track_normalized_key, now_millis};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// `initQueuePanel` : titres à venir et récents affichés.
const QUEUE_PANEL_UPCOMING: usize = 50;
const QUEUE_PANEL_RECENT: usize = 40;
const SEARCH_PAGE: usize = 12;
const SEARCH_MIN_FETCH: usize = 3;
const INFINITE_SCROLL_MARGIN: f32 = 280.0;
const WAVE_FRESH_MS: i64 = 4 * 3600 * 1000;
const TRENDING_FRESH_MS: i64 = 12 * 3600 * 1000;
const WAVE_MIN_TRACKS: usize = 5;
const WAVE_SKIP_BEFORE: Duration = Duration::from_secs(12);
const VOLUME_STEP: f32 = 0.05;
const MIX_LIMIT: u32 = 20;
const MIX_MIN_TRACKS: usize = 8;
const QUICK_RECENT: usize = 5;
/// Nom de la playlist et du fichier à l'export des titres aimés.
const FAVORITES_EXPORT_NAME: &str = "Liked Songs";
const ARTIST_META_MAX: usize = 8;
pub const DEFAULT_SIDEBAR_ORDER: [&str; 6] = [
  "discover",
  "favorites",
  "albums",
  "local",
  "history",
  "queue",
];

fn now_ms() -> i64 {
  now_millis() as i64
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
  list
    .iter()
    .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
    .collect()
}

fn get_json(tag: ReplyTag, path: &'static str, query: Vec<(String, String)>) -> Effect {
  Effect::CoreJson {
    tag,
    path,
    query,
    body: None,
    search: false,
  }
}

fn post_json(tag: ReplyTag, path: &'static str, body: &Value) -> Effect {
  Effect::CoreJson {
    tag,
    path,
    query: Vec::new(),
    body: Some(body.to_string()),
    search: false,
  }
}

fn tracks_of(value: &Value) -> Vec<Track> {
  value
    .as_array()
    .map(|items| {
      items
        .iter()
        .filter_map(|item| serde_json::from_value(item.clone()).ok())
        .collect()
    })
    .unwrap_or_default()
}

fn success(value: &Value) -> bool {
  value.get("success").and_then(Value::as_bool) == Some(true)
}

impl App {
  pub(crate) fn platform(&self) -> String {
    self.data.platform.clone()
  }

  /// Sauvegarde différée : seul le dernier changement écrit le fichier.
  pub(crate) fn data_changed(&mut self) -> Effect {
    self.refresh_orders();
    self.data_generation += 1;
    Effect::ScheduleSave(self.data_generation)
  }

  /// Recalcule l'ordre d'affichage des listes triées et filtrées.
  pub(crate) fn refresh_orders(&mut self) {
    let text_of = |value: &Option<String>| value.as_deref().unwrap_or("").to_lowercase();
    let matches = |track: &Track, query: &str, with_album: bool| {
      text_of(&track.title).contains(query)
        || text_of(&track.artist).contains(query)
        || (with_album && text_of(&track.album).contains(query))
    };
    let query = self.ui.favorites_query.trim().to_lowercase();
    let mut favorites = self.data.display_order(
      &self.data.favorites,
      self.data.favorites_sort,
      FAVORITES_SCOPE,
    );
    if !query.is_empty() {
      favorites.retain(|&index| matches(&self.data.favorites[index], &query, false));
    }
    self.ui.favorites_order = favorites;

    let query = self.ui.local_query.trim().to_lowercase();
    self.ui.local_order = (0..self.data.local_files.len())
      .filter(|&index| query.is_empty() || matches(&self.data.local_files[index], &query, true))
      .collect();

    self.ui.playlist_order = match self
      .ui
      .playlist_id
      .as_deref()
      .and_then(|id| self.data.playlist(id))
    {
      Some(playlist) => {
        let scope = format!("pl:{}", playlist.id.as_deref().unwrap_or(""));
        let query = self.ui.playlist_query.trim().to_lowercase();
        let mut order = self
          .data
          .display_order(&playlist.tracks, self.data.playlist_sort, &scope);
        if !query.is_empty() {
          order.retain(|&index| matches(&playlist.tracks[index], &query, false));
        }
        order
      }
      None => Vec::new(),
    };
  }

  /// Nombre de lignes d'une liste.
  #[must_use]
  pub fn list_len(&self, list: ListId) -> usize {
    match list {
      ListId::Favorites => self.ui.favorites_order.len(),
      ListId::History => self.data.history.len(),
      ListId::Queue => self.player.queue.len(),
      ListId::Local => self.ui.local_order.len(),
      ListId::Playlist => self.ui.playlist_order.len(),
      ListId::Search => self.ui.search.tracks.len(),
      ListId::Album => self.ui.album.tracks.len(),
      ListId::Artist => self.ui.artist.visible_tracks(),
      ListId::Trending => self.ui.home.trending.len(),
      ListId::Recent => self.recent_tracks().len(),
      ListId::Pinned => self.data.sidebar_pinned_tracks.len(),
    }
  }

  /// Ligne affichée d'une liste.
  #[must_use]
  pub fn list_track(&self, list: ListId, index: usize) -> Option<&Track> {
    match list {
      ListId::Favorites => self
        .data
        .favorites
        .get(*self.ui.favorites_order.get(index)?),
      ListId::History => self.data.history.get(index),
      ListId::Queue => self.player.queue.get(index),
      ListId::Local => self.data.local_files.get(*self.ui.local_order.get(index)?),
      ListId::Playlist => {
        let playlist = self.data.playlist(self.ui.playlist_id.as_deref()?)?;
        playlist.tracks.get(*self.ui.playlist_order.get(index)?)
      }
      ListId::Search => self.ui.search.tracks.get(index),
      ListId::Album => self.ui.album.tracks.get(index),
      ListId::Artist => self.ui.artist.tracks.get(index),
      ListId::Trending => self.ui.home.trending.get(index),
      ListId::Recent => self.recent_tracks().get(index).copied(),
      ListId::Pinned => self.data.sidebar_pinned_tracks.get(index),
    }
  }

  /// Pistes récentes de l'accès rapide, sans doublon.
  #[must_use]
  pub fn recent_tracks(&self) -> Vec<&Track> {
    let mut seen = HashSet::new();
    self
      .data
      .history
      .iter()
      .filter(|track| seen.insert(track.canonical_id(&self.data.platform)))
      .take(QUICK_RECENT)
      .collect()
  }

  /// Toute la liste, dans l'ordre affiché : elle devient la file de lecture.
  pub(crate) fn list_as_queue(&self, list: ListId) -> Vec<Track> {
    match list {
      // Comme `sortTracksFor` : la file suit le tri mais pas le filtre.
      ListId::Favorites => {
        let order = self.data.display_order(
          &self.data.favorites,
          self.data.favorites_sort,
          FAVORITES_SCOPE,
        );
        order
          .into_iter()
          .map(|index| self.data.favorites[index].clone())
          .collect()
      }
      ListId::Playlist => self
        .ui
        .playlist_id
        .as_deref()
        .and_then(|id| self.data.playlist(id))
        .map_or_else(Vec::new, |playlist| {
          let scope = format!("pl:{}", playlist.id.as_deref().unwrap_or(""));
          let order = self
            .data
            .display_order(&playlist.tracks, self.data.playlist_sort, &scope);
          order
            .into_iter()
            .map(|index| playlist.tracks[index].clone())
            .collect()
        }),
      ListId::Artist => self.ui.artist.tracks.clone(),
      _ => (0..self.list_len(list))
        .filter_map(|index| self.list_track(list, index).cloned())
        .collect(),
    }
  }

  /// Portée d'aléatoire d'une liste : `favorites` ou `pl:<id>` ; `None` pour les autres.
  fn list_scope(&self, list: ListId) -> Option<String> {
    match list {
      ListId::Favorites => Some(FAVORITES_SCOPE.to_string()),
      ListId::Playlist => self.ui.playlist_id.as_ref().map(|id| format!("pl:{id}")),
      _ => None,
    }
  }

  /// La liste devient la file. Avec une portée, l'aléatoire du lecteur prend celui de la portée ; sans, le réglage
  /// global.
  fn replace_queue(&mut self, track: Track, list: Vec<Track>, start: usize, scope: Option<String>) {
    let platform = self.platform();
    self.player.play_list(track, list, start, &platform);
    self.player.shuffle = scope
      .as_deref()
      .map_or(self.data.shuffle, |scope| self.data.shuffle_on(scope));
    self.player.scope = scope;
  }

  /// Bouton « Play » d'une playlist ou des titres aimés : la liste devient la file, mélangée si l'aléatoire de sa
  /// portée est actif.
  fn play_all(&mut self, mut list: Vec<Track>, scope: String) -> Vec<Effect> {
    if list.is_empty() {
      return Vec::new();
    }
    if self.data.shuffle_on(&scope) {
      self.player.shuffle_list(&mut list);
    }
    if self.jam.is_some() {
      return self.jam_play_now(list);
    }
    self.player.radio = false;
    self.player.my_wave = false;
    let first = list[0].clone();
    self.replace_queue(first, list, 0, Some(scope));
    self.start_current(None)
  }

  /// Nom affiché des titres aimés : celui choisi, sinon « Liked songs » traduit.
  #[must_use]
  pub fn liked_title(&self) -> String {
    self
      .data
      .liked_custom_name()
      .map_or_else(|| self.i18n.t("Liked songs"), str::to_string)
  }

  /// Portée d'épingle de l'écran courant (`currentPinScope`).
  #[must_use]
  pub fn pin_scope(&self) -> Option<String> {
    match self.screen {
      Screen::Liked => Some(FAVORITES_SCOPE.to_string()),
      Screen::PlaylistDetail => self.ui.playlist_id.as_ref().map(|id| format!("pl:{id}")),
      _ => None,
    }
  }

  /// `playTrack`, depuis une ligne.
  fn play_row(&mut self, list: ListId, index: usize) -> Vec<Effect> {
    let Some(track) = self.list_track(list, index).cloned() else {
      return Vec::new();
    };
    if self.jam.is_some() {
      let op = match (list, self.jam_queue_uid(index)) {
        (ListId::Queue, Some(uid)) => Op::PlayEntry { uid },
        (ListId::Queue, None) => return Vec::new(),
        _ => return self.jam_play_now(vec![track]),
      };
      return self.jam_divert(op).unwrap_or_default();
    }
    if list == ListId::Search {
      self.data.record_search_track(&track, now_ms());
    }
    if list != ListId::Queue {
      self.player.radio = false;
      self.player.my_wave = false;
    }
    self.ui.search.history_open = false;
    let platform = self.platform();
    if list == ListId::Pinned {
      self.replace_queue(track.clone(), vec![track], 0, None);
    } else {
      let queue = self.list_as_queue(list);
      let start = queue
        .iter()
        .position(|at| at.is_same(&track, &platform))
        .unwrap_or(index);
      if list == ListId::Queue {
        // La file ne change pas d'origine : contexte et aléatoire restent ceux en cours.
        let scope = self.player.scope.clone();
        self.player.play_list(track, queue, index, &platform);
        self.player.scope = scope;
      } else {
        let scope = self.list_scope(list);
        self.replace_queue(track, queue, start, scope);
      }
    }
    self.start_current(None)
  }

  /// Titres « Next up » avec leur index dans la file : 50 au plus, après le titre en cours.
  #[must_use]
  pub fn queue_panel_upcoming(&self) -> Vec<(usize, &Track)> {
    let start = self.player.index.map_or(0, |index| index + 1);
    (start..)
      .zip(self.player.queue.iter().skip(start))
      .take(QUEUE_PANEL_UPCOMING)
      .collect()
  }

  /// Titres « Recently played » : 40 au plus.
  #[must_use]
  pub fn queue_panel_recent(&self) -> &[Track] {
    &self.data.history[..self.data.history.len().min(QUEUE_PANEL_RECENT)]
  }

  /// Rangée de la bibliothèque latérale : `playTrack(fullTrack, [fullTrack], 0, 'sidebar-pinned')` (`app.js:27306-27309`).
  /// `fullTrack` est la version des favoris ou de l'historique si elle existe : elle est plus complète.
  fn play_sidebar_track(&mut self, track: Track) -> Vec<Effect> {
    let platform = self.platform();
    let full = self
      .data
      .favorites
      .iter()
      .chain(self.data.history.iter())
      .find(|saved| saved.is_same(&track, &platform))
      .cloned()
      .unwrap_or(track);
    if self.jam.is_some() {
      return self.jam_play_now(vec![full]);
    }
    self.player.radio = false;
    self.player.my_wave = false;
    self.ui.search.history_open = false;
    self.replace_queue(full.clone(), vec![full], 0, None);
    self.start_current(None)
  }

  /// `playTrack(tr, recent, recent.indexOf(tr))` : les récents affichés deviennent la file.
  fn play_recent(&mut self, index: usize) -> Vec<Effect> {
    let recent = self.queue_panel_recent().to_vec();
    let Some(track) = recent.get(index).cloned() else {
      return Vec::new();
    };
    if self.jam.is_some() {
      return self.jam_play_now(vec![track]);
    }
    self.player.radio = false;
    self.player.my_wave = false;
    self.replace_queue(track, recent, index, None);
    self.start_current(None)
  }

  /// Démarre le titre courant à une position donnée.
  pub(crate) fn start_current_at(&mut self, start: Duration) -> Vec<Effect> {
    let mut effects = self.start_current(None);
    for effect in &mut effects {
      if let Effect::ResolveTrack {
        start: position, ..
      } = effect
      {
        *position = start;
      }
    }
    effects
  }

  /// Démarre le titre courant du lecteur : historique, source, état diffusé.
  pub(crate) fn start_current(&mut self, crossfade: Option<u32>) -> Vec<Effect> {
    let Some(track) = self.player.current.clone() else {
      return Vec::new();
    };
    self.resume_cancel();
    self.normalize = riff_audio::normalize::NormalizeState::default();
    self.playback = playback_reset(&self.playback);
    self.jam_on_start(crossfade.is_some());
    self.player.crossfade_pending = crossfade.is_some();
    self.ui.waiting_crossfade = None;
    self.ui.autoplay_found = None;
    self.ui.autoplay_loading = false;
    self.ui.autoplay_waiting = false;
    self.ui.radio_waiting = false;
    self.data.record_played(&track, now_ms());
    // `playTrack` rappelle `renderHistoryView` quand l'historique est affiché (`app.js:4572`).
    self.entered_rows(Screen::History);
    let mut effects = vec![
      self.data_changed(),
      Effect::NormalizeGain(0.0),
      self.state_effect(false),
    ];
    if crossfade.is_none() {
      effects.push(Effect::AudioPause);
    }
    let start = self
      .saved_trim(&track)
      .map_or(Duration::ZERO, |(start, _)| Duration::from_secs_f64(start));
    effects.push(self.resolve_effect(&track, start, crossfade, false));
    effects.extend(self.load_lyrics());
    match track.thumb_url() {
      Some(url) if self.follows_cover() => effects.push(Effect::FetchCover(url)),
      None if !track.is_local() => effects.push(get_json(
        ReplyTag::TrackCover {
          token: self.player.token,
        },
        "/api/track-cover",
        pairs(&[
          ("artist", track.artist.as_deref().unwrap_or("")),
          ("title", track.title.as_deref().unwrap_or("")),
        ]),
      )),
      _ => {}
    }
    if self.player.radio_needs_refill() {
      effects.extend(self.request_radio_refill());
    }
    effects
  }

  pub(crate) fn resolve_effect(
    &self,
    track: &Track,
    start: Duration,
    crossfade: Option<u32>,
    force_yt_dlp: bool,
  ) -> Effect {
    Effect::ResolveTrack {
      token: self.player.token,
      track: Box::new(track.clone()),
      options: ResolveOptions {
        platform: self.data.platform.clone(),
        quality: self.data.audio_quality.clone(),
        force_yt_dlp,
        fresh: false,
      },
      start,
      crossfade,
    }
  }

  /// Relance d'un flux interrompu. Écart voulu avec Riff (qui force `yt-dlp` dès la première relance) :
  /// la première relance repart d'une résolution directe fraîche, `yt-dlp` reste le dernier recours.
  fn retry_effect(&self, track: &Track, position: Duration) -> Effect {
    let force_yt_dlp = self.player.stream_retries >= MAX_STREAM_RETRIES;
    let mut effect = self.resolve_effect(track, position, None, force_yt_dlp);
    if let Effect::ResolveTrack { options, .. } = &mut effect {
      options.fresh = !force_yt_dlp;
    }
    effect
  }

  pub(crate) fn state_effect(&self, playing: bool) -> Effect {
    let current = self.player.current.as_ref();
    Effect::PublishAppState(json!({
      "title": current.and_then(|track| track.title.as_deref()),
      "artist": current.and_then(|track| track.artist.as_deref()),
      "isPlaying": playing,
      "word": "",
      "wordIndex": -1,
      "lineIndex": -1,
    }))
  }

  pub(crate) fn duration_secs(&self) -> f64 {
    self
      .playback
      .duration
      .map(|duration| duration.as_secs_f64())
      .filter(|duration| *duration > 1.0)
      .or_else(|| {
        self
          .player
          .current
          .as_ref()
          .and_then(|track| track.duration)
      })
      .unwrap_or(0.0)
  }

  pub(crate) fn go_next(&mut self, natural: bool, crossfade: Option<u32>) -> Vec<Effect> {
    if !natural
      && self.player.my_wave
      && self.playback.position < WAVE_SKIP_BEFORE
      && let Some(track) = self.player.current.clone()
    {
      self.data.record_wave_skip(&track, now_ms());
    }
    if let Some(effects) = self.plugin_pick(crossfade) {
      return effects;
    }
    let data = &self.data;
    // Dans un jam, la file est commune : les titres bloqués y sont joués, sans être débloqués.
    let in_jam = self.jam.is_some();
    let step = self.player.next(
      &|track| !in_jam && data.is_blocked(track),
      data.autoplay && !in_jam,
    );
    self.apply_next(step, crossfade)
  }

  pub(crate) fn apply_next(&mut self, step: NextStep, crossfade: Option<u32>) -> Vec<Effect> {
    if crossfade.is_some() {
      self.ui.waiting_crossfade = crossfade;
    }
    match step {
      NextStep::Nothing => {
        self.playback.is_playing = false;
        Vec::new()
      }
      NextStep::Play(_) => {
        let crossfade = self.ui.waiting_crossfade.take();
        self.start_current(crossfade)
      }
      NextStep::RefillRadio => {
        self.ui.radio_waiting = true;
        self.request_radio_refill()
      }
      NextStep::Autoplay => {
        self.ui.autoplay_waiting = true;
        if let Some(found) = self.ui.autoplay_found.take() {
          return self.autoplay_ready(found);
        }
        self.prepare_autoplay()
      }
    }
  }

  fn prepare_autoplay(&mut self) -> Vec<Effect> {
    if self.ui.autoplay_loading {
      return Vec::new();
    }
    self.ui.autoplay_loading = true;
    let current = self.player.current.as_ref();
    let mut artists: Vec<String> = current
      .and_then(|track| track.artist.as_deref())
      .map(library::autoplay_artists)
      .unwrap_or_default();
    let mut counts: Vec<(String, u32)> = Vec::new();
    for artist in self
      .player
      .queue
      .iter()
      .flat_map(|track| library::autoplay_artists(track.artist.as_deref().unwrap_or("")))
    {
      match counts.iter_mut().find(|(name, _)| *name == artist) {
        Some(entry) => entry.1 += 1,
        None => counts.push((artist, 1)),
      }
    }
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (name, _) in counts {
      if !artists
        .iter()
        .any(|known| known.eq_ignore_ascii_case(&name))
      {
        artists.push(name);
      }
    }
    artists.truncate(4);
    let profile = self.data.taste_profile(8, now_ms());
    let payload = json!({
      "platform": "youtube",
      "limit": 12,
      "clientProfile": profile,
      "seeds": profile["seeds"],
      "avoid": self.data.wave_avoid(),
    });
    vec![Effect::PrepareAutoplay { artists, payload }]
  }

  /// `buildAutoplayCandidates` : la lecture auto ne repropose ni la file, ni l'historique récent, ni un titre bloqué.
  fn usable_autoplay(&self, found: Vec<Track>) -> Vec<Track> {
    let platform = &self.data.platform;
    let key_of = |track: &Track| {
      get_track_normalized_key(
        track.artist.as_deref().unwrap_or(""),
        track.title.as_deref().unwrap_or(""),
      )
    };
    let known = self
      .player
      .queue
      .iter()
      .chain(self.data.history.iter().take(60));
    let mut ids: HashSet<String> = HashSet::new();
    let mut keys: HashSet<String> = HashSet::new();
    for track in known {
      ids.insert(track.canonical_id(platform));
      keys.insert(key_of(track));
    }
    found
      .into_iter()
      .filter(|track| {
        track
          .title
          .as_deref()
          .is_some_and(|title| !title.is_empty())
          && !self.data.is_blocked(track)
          && ids.insert(track.canonical_id(platform))
          && keys.insert(key_of(track))
      })
      .collect()
  }

  fn autoplay_ready(&mut self, found: Vec<Track>) -> Vec<Effect> {
    self.ui.autoplay_loading = false;
    if self.jam.is_some() {
      // La file appartient au jam : une réponse tardive ne l'allonge pas.
      self.ui.autoplay_waiting = false;
      return Vec::new();
    }
    let usable = self.usable_autoplay(found);
    if !self.ui.autoplay_waiting {
      self.ui.autoplay_found = Some(usable);
      return Vec::new();
    }
    self.ui.autoplay_waiting = false;
    if usable.is_empty() {
      let data = &self.data;
      let in_jam = self.jam.is_some();
      let step = self
        .player
        .advance(&|track| !in_jam && data.is_blocked(track));
      return self.apply_next(step, None);
    }
    let step = self.player.append_autoplay(usable);
    let mut effects = vec![toast_effect("Autoplay: similar tracks")];
    effects.extend(self.apply_next(step, None));
    effects
  }

  fn request_radio_refill(&mut self) -> Vec<Effect> {
    if self.ui.radio_loading {
      return Vec::new();
    }
    self.ui.radio_loading = true;
    let taste = self.data.taste_profile(8, now_ms());
    let avoid = self.data.wave_avoid();
    let tail = self.player.queue.len().saturating_sub(60);
    let queue = &self.player.queue[tail..];
    let radio_key = |track: &Track| {
      format!(
        "{}::{}",
        track.artist.as_deref().unwrap_or("").trim().to_lowercase(),
        track.title.as_deref().unwrap_or("").trim().to_lowercase()
      )
    };
    let mut ids: Vec<Value> = avoid["ids"].as_array().cloned().unwrap_or_default();
    ids.extend(
      queue
        .iter()
        .filter_map(|track| track.id.clone())
        .map(Value::from),
    );
    ids.truncate(120);
    let mut keys: Vec<Value> = avoid["keys"].as_array().cloned().unwrap_or_default();
    keys.extend(queue.iter().map(radio_key).map(Value::from));
    keys.truncate(120);
    let artists: Vec<Value> = avoid["artists"]
      .as_array()
      .cloned()
      .unwrap_or_default()
      .into_iter()
      .take(40)
      .collect();
    let body = json!({
      "platform": self.data.platform,
      "seeds": taste["seeds"],
      "topArtists": taste["topArtists"],
      "avoid": { "ids": ids, "keys": keys, "artists": artists },
      "count": 12,
    });
    vec![post_json(ReplyTag::RadioRefill, "/api/vibe", &body)]
  }

  fn radio_reply(&mut self, result: Result<Value, String>) -> Vec<Effect> {
    self.ui.radio_loading = false;
    if self.jam.is_some() {
      self.ui.radio_waiting = false;
      return Vec::new();
    }
    let incoming = result
      .map(|value| tracks_of(&value["tracks"]))
      .unwrap_or_default();
    let key_of = |track: &Track| {
      format!(
        "{}::{}",
        track.artist.as_deref().unwrap_or("").trim().to_lowercase(),
        track.title.as_deref().unwrap_or("").trim().to_lowercase()
      )
    };
    let tail = self.player.queue.len().saturating_sub(60);
    let mut ids: HashSet<String> = self.player.queue[tail..]
      .iter()
      .filter_map(|track| track.id.clone())
      .collect();
    let mut keys: HashSet<String> = self.player.queue[tail..].iter().map(key_of).collect();
    let fresh: Vec<Track> = incoming
      .into_iter()
      .filter(|track| {
        track
          .title
          .as_deref()
          .is_some_and(|title| !title.is_empty())
          && (track.id.is_some() || track.url.is_some())
          && track.id.as_ref().is_none_or(|id| ids.insert(id.clone()))
          && keys.insert(key_of(track))
      })
      .collect();
    let added = !fresh.is_empty();
    self.player.queue.extend(fresh);
    if !self.ui.radio_waiting {
      return Vec::new();
    }
    self.ui.radio_waiting = false;
    let data = &self.data;
    let in_jam = self.jam.is_some();
    let step = if added {
      self
        .player
        .play_after_refill(&|track| !in_jam && data.is_blocked(track))
    } else {
      self
        .player
        .advance(&|track| !in_jam && data.is_blocked(track))
    };
    self.apply_next(step, None)
  }

  /// Fin de la découpe du titre : on reboucle en mode répétition, sinon on passe au suivant (`app.js:8808`).
  fn trim_end_reached(&mut self, position: Duration) -> Option<Vec<Effect>> {
    let track = self.player.current.as_ref()?;
    let (start, end) = self.saved_trim(track)?;
    if position.as_secs_f64() < end {
      return None;
    }
    if self.sleep_at_end() {
      return Some(self.sleep_stop());
    }
    if self.player.repeat {
      return Some(vec![Effect::AudioSeek(Duration::from_secs_f64(start))]);
    }
    Some(self.go_next(true, None))
  }

  /// Événements du moteur.
  pub(crate) fn audio_event(&mut self, event: AudioEvent) -> Vec<Effect> {
    let was_loading = self.player.loading;
    match event {
      // Le moteur peut encore signaler l'ancien titre pendant que le suivant se résout.
      AudioEvent::Position(_) | AudioEvent::Ended if was_loading => Vec::new(),
      AudioEvent::Position(position) => {
        self.playback.apply(AudioEvent::Position(position));
        let duration = self.duration_secs();
        let live = self.playback.duration.is_none();
        let mut scrobbles = self.lastfm_tick(position.as_secs_f64(), duration);
        if self.preview_reached(position.as_secs_f64()) {
          return vec![Effect::AudioPause];
        }
        if !self.jam_is_guest()
          && let Some(effects) = self.trim_end_reached(position)
        {
          return effects;
        }
        self.jam_on_position();
        let triggers = self.player.position_changed(position, duration, live);
        let mut effects = std::mem::take(&mut scrobbles);
        effects.extend(self.jam_guest_position(position));
        if triggers.prepare_next {
          if let Some(next) = self
            .player
            .upcoming()
            .filter(|next| !next.is_local() && !next.is_cached)
          {
            effects.push(prefetch_effect(next, &self.data.platform));
          }
          if self
            .player
            .should_prepare_autoplay(self.data.autoplay && self.jam.is_none())
          {
            effects.extend(self.prepare_autoplay());
          }
        }
        if let Some(seconds) = triggers.crossfade {
          if self.sleep_at_end() || self.jam_is_guest() {
            // Écart voulu avec Riff : le fondu ne doit pas enchaîner avant la pause. Chez l'invité d'un jam,
            // l'hôte décide du changement de titre.
            self.player.cancel_crossfade_trigger();
          } else {
            effects.extend(self.go_next(true, Some(seconds)));
          }
        }
        effects
      }
      AudioEvent::Loaded {
        duration,
        sample_rate,
      } => {
        self.playback.apply(AudioEvent::Loaded {
          duration,
          sample_rate,
        });
        if !was_loading {
          return Vec::new();
        }
        self.player.loading = false;
        self.player.crossfade_pending = false;
        self.playback.is_playing = true;
        self.resume_loaded();
        self.jam_on_loaded();
        let mut effects = vec![self.state_effect(true)];
        effects.extend(self.jam_guest_loaded());
        if let Some(duration) = duration.filter(|duration| duration.as_secs() > 1)
          && self.fill_duration(duration.as_secs_f64().round())
        {
          effects.push(self.data_changed());
        }
        effects
      }
      AudioEvent::Ended => self.track_ended(),
      AudioEvent::Error(AudioError::NoDevice) => self.device_lost(),
      AudioEvent::DeviceChanged(device) => {
        self.playback.apply(AudioEvent::DeviceChanged(device));
        self.device_back()
      }
      AudioEvent::Error(err) => {
        let (origin, platform) = self.track_origin();
        tracing::warn!(
          error = %audio_error_text(&err),
          position_s = self.playback.position.as_secs(),
          origin,
          platform,
          "erreur du moteur audio"
        );
        self.playback.apply(AudioEvent::Error(err));
        self.stream_failed()
      }
      AudioEvent::Buffering(buffering) => {
        if buffering != self.playback.buffering {
          tracing::warn!(
            buffering,
            position_s = self.playback.position.as_secs(),
            "mise en mémoire tampon"
          );
        }
        self.playback.apply(AudioEvent::Buffering(buffering));
        Vec::new()
      }
    }
  }

  /// Durée lue dans le flux pour un titre qui n'en a pas (`loadedmetadata`). Renvoie `true` si des données ont changé.
  fn fill_duration(&mut self, duration: f64) -> bool {
    let Some(current) = self.player.current.clone() else {
      return false;
    };
    let platform = self.platform();
    let mut changed = false;
    let fill = |track: &mut Track| {
      if track.duration.unwrap_or(0.0) <= 0.0 && track.is_same(&current, &platform) {
        track.duration = Some(duration);
        true
      } else {
        false
      }
    };
    for list in [
      &mut self.data.local_files,
      &mut self.data.favorites,
      &mut self.data.history,
    ]
    .into_iter()
    .chain(
      self
        .data
        .playlists
        .iter_mut()
        .map(|playlist| &mut playlist.tracks),
    ) {
      for track in list.iter_mut() {
        changed |= fill(track);
      }
    }
    self.player.queue.iter_mut().for_each(|track| {
      fill(track);
    });
    if let Some(track) = &mut self.player.current {
      fill(track);
    }
    changed
  }

  fn track_ended(&mut self) -> Vec<Effect> {
    let Some(track) = self.player.current.clone() else {
      self.playback.is_playing = false;
      return Vec::new();
    };
    let duration = self.duration_secs();
    match self.player.ended(self.playback.position, duration) {
      EndStep::Restart | EndStep::Next if self.sleep_at_end() => self.sleep_stop(),
      // Chez l'invité d'un jam, l'hôte décide de la suite.
      EndStep::Restart | EndStep::Next if self.jam_is_guest() => {
        self.playback.is_playing = false;
        Vec::new()
      }
      EndStep::Restart => {
        self.jam_on_restart();
        self.player.token += 1;
        self.player.loading = true;
        self.playback = playback_reset(&self.playback);
        vec![self.resolve_effect(&track, Duration::ZERO, None, false)]
      }
      EndStep::Recover(position) => {
        tracing::warn!(
          position_s = position.as_secs(),
          "fin prématurée : nouvelle résolution"
        );
        self.player.token += 1;
        self.player.loading = true;
        vec![self.retry_effect(&track, position)]
      }
      EndStep::Next => {
        self.playback.is_playing = false;
        self.go_next(true, None)
      }
    }
  }

  /// Aucun périphérique de sortie : pause, sans nouvelle résolution du flux (Riff : `pauseSmooth`
  /// au `devicechange`). Le moteur signale l'erreur une fois par panne et reprendra seul au retour.
  fn device_lost(&mut self) -> Vec<Effect> {
    self.playback.apply(AudioEvent::Error(AudioError::NoDevice));
    self.playback.resume_on_device = self.playback.is_playing || self.player.loading;
    self.player.loading = false;
    self.playback.is_playing = false;
    vec![
      toast_effect_kind("No audio output device", ToastKind::Error),
      self.state_effect(false),
    ]
  }

  /// Retour d'un périphérique : le moteur reprend la lecture s'il jouait à la perte
  /// (`retry_output`, `resume_playing`). L'état de l'application suit.
  fn device_back(&mut self) -> Vec<Effect> {
    let resume = std::mem::take(&mut self.playback.resume_on_device);
    if !resume || self.playback.is_playing || self.player.loading {
      return Vec::new();
    }
    self.playback.is_playing = true;
    vec![self.state_effect(true)]
  }

  /// Origine du titre courant pour le journal : jamais le titre ni l'artiste.
  fn track_origin(&self) -> (&'static str, &str) {
    match self.player.current.as_ref() {
      None => ("aucun", "-"),
      Some(track) if track.is_local() => ("local", "local"),
      Some(track) => ("en ligne", track.platform.as_deref().unwrap_or("inconnue")),
    }
  }

  /// `recoverCurrentStream`.
  fn stream_failed(&mut self) -> Vec<Effect> {
    let position = self.playback.position;
    if self.player.retry_stream()
      && let Some(track) = self.player.current.clone()
    {
      tracing::warn!(
        attempt = self.player.stream_retries,
        position_s = position.as_secs(),
        "flux interrompu : nouvelle résolution"
      );
      return vec![self.retry_effect(&track, position)];
    }
    self.player.loading = false;
    self.playback.is_playing = false;
    let local = self.player.current.as_ref().is_some_and(Track::is_local);
    tracing::warn!(
      retries = self.player.stream_retries,
      local,
      "flux interrompu : abandon"
    );
    let message = if local {
      "File missing from library"
    } else {
      "Could not load audio stream"
    };
    vec![toast_effect_kind(message, ToastKind::Error)]
  }

  fn volume_effect(&mut self) -> Vec<Effect> {
    vec![
      Effect::AudioMix(self.data.volume, self.data.muted),
      self.data_changed(),
    ]
  }

  /// Messages du lecteur.
  pub fn update_player(&mut self, message: PlayerMessage) -> Vec<Effect> {
    match message {
      PlayerMessage::TogglePlay => {
        if let Some(effects) = self.sync_space() {
          effects
        } else if let Some(effects) = self.jam_toggle_play() {
          effects
        } else {
          self.toggle_play()
        }
      }
      PlayerMessage::Next => self
        .jam_step(false)
        .unwrap_or_else(|| self.go_next(false, None)),
      PlayerMessage::Prev => self.jam_step(true).unwrap_or_else(|| self.go_prev()),
      PlayerMessage::ToggleShuffle => {
        if self.jam.is_some() {
          let on = !self.player.shuffle;
          return self.jam_divert(Op::Shuffle { on }).unwrap_or_default();
        }
        self.player.shuffle = !self.player.shuffle;
        // Une file issue d'une liste porte l'aléatoire de cette liste ; sinon le réglage global.
        let known = self.player.scope.clone().filter(|scope| {
          scope == FAVORITES_SCOPE
            || scope.strip_prefix("pl:").is_some_and(|id| {
              self
                .data
                .playlists
                .iter()
                .any(|p| p.id.as_deref() == Some(id))
            })
        });
        match known {
          Some(scope) => self.data.set_shuffle_scope(&scope, self.player.shuffle),
          None => self.data.shuffle = self.player.shuffle,
        }
        vec![self.data_changed()]
      }
      PlayerMessage::ToggleRepeat => {
        if self.jam.is_some() {
          let on = !self.player.repeat;
          return self.jam_divert(Op::Repeat { on }).unwrap_or_default();
        }
        self.player.repeat = !self.player.repeat;
        self.data.repeat = self.player.repeat;
        vec![self.data_changed()]
      }
      PlayerMessage::SeekPreview(seconds) => {
        self.ui.seek_preview = Some(seconds);
        Vec::new()
      }
      PlayerMessage::SeekRelease => self
        .ui
        .seek_preview
        .take()
        .map_or_else(Vec::new, |seconds| {
          let target = Duration::from_secs_f32(seconds.max(0.0));
          if let Some(effects) = self.jam_seek(target) {
            return effects;
          }
          self.playback.position = target;
          vec![Effect::AudioSeek(target)]
        }),
      PlayerMessage::SeekBy(delta) => {
        if self.player.current.is_none() {
          return Vec::new();
        }
        let target = (self.playback.position.as_secs_f64() + f64::from(delta))
          .clamp(0.0, self.duration_secs().max(0.0));
        let target = Duration::from_secs_f64(target);
        if let Some(effects) = self.jam_seek(target) {
          return effects;
        }
        self.playback.position = target;
        vec![Effect::AudioSeek(target)]
      }
      PlayerMessage::Volume(volume) => {
        self.data.volume = volume.clamp(0.0, 1.0);
        if self.data.volume > 0.0 {
          self.data.muted = false;
        }
        self.volume_effect()
      }
      PlayerMessage::VolumeBy(delta) => {
        let step = if delta > 0.0 {
          VOLUME_STEP
        } else {
          -VOLUME_STEP
        };
        self.data.volume = ((self.data.volume + step) * 100.0)
          .round()
          .clamp(0.0, 100.0)
          / 100.0;
        if self.data.volume > 0.0 {
          self.data.muted = false;
        }
        self.volume_effect()
      }
      PlayerMessage::ToggleMute => {
        self.data.muted = !self.data.muted;
        self.volume_effect()
      }
      PlayerMessage::Resolved {
        token,
        start,
        crossfade,
        result,
      } => {
        if token != self.player.token {
          return Vec::new();
        }
        match result {
          Ok(source) => match crossfade {
            Some(seconds) => vec![Effect::AudioCrossfade(SourceEffect(source), seconds)],
            None => vec![Effect::AudioLoadSource(SourceEffect(source), start)],
          },
          Err(reason) => {
            let (origin, platform) = self.track_origin();
            tracing::warn!(
              reason = %riff_core::logging::redact_urls(&reason),
              origin,
              platform,
              "résolution du flux échouée"
            );
            self.player.loading = false;
            self.player.crossfade_pending = false;
            self.playback.is_playing = false;
            if self.jam_is_guest() {
              return self.jam_resolve_failed();
            }
            if let Some(effects) = self.resume_failed() {
              return effects;
            }
            let mut effects = vec![toast_effect_kind(&reason, ToastKind::Error)];
            effects.extend(self.jam_resolve_failed());
            effects
          }
        }
      }
      PlayerMessage::NormalizeTick => Vec::new(),
      PlayerMessage::Level(level) => {
        if !self.data.normalize || !self.playback.is_playing {
          return Vec::new();
        }
        riff_audio::normalize::next_gain_db(&mut self.normalize, level)
          .map_or_else(Vec::new, |gain| vec![Effect::NormalizeGain(gain)])
      }
      PlayerMessage::AutoplayReady(found) => self.autoplay_ready(found),
      PlayerMessage::PlayMyWave => self.play_my_wave(),
      // Secours : même démarrage que l'ouverture de la fenêtre (proxy et périphérique avant tout flux).
      PlayerMessage::Resume => self.start_or_hold(),
      PlayerMessage::ResumeRetry(token) => self.resume_retry(token),
    }
  }

  /// Corps de « précédent » : recommence le titre ou revient au précédent.
  pub(crate) fn go_prev(&mut self) -> Vec<Effect> {
    let platform = self.platform();
    match self.player.prev(self.playback.position, &platform) {
      PrevStep::Nothing => Vec::new(),
      PrevStep::Restart => {
        self.playback.position = Duration::ZERO;
        vec![Effect::AudioSeek(Duration::ZERO)]
      }
      PrevStep::Play(_) => self.start_current(None),
    }
  }

  pub(crate) fn toggle_play(&mut self) -> Vec<Effect> {
    if self.player.current.is_none() {
      if self.ui.home.wave.is_empty() {
        return Vec::new();
      }
      return self.start_radio(0, false);
    }
    if self.player.loading {
      return Vec::new();
    }
    // Un choix explicite pendant la panne remplace la reprise automatique.
    self.playback.resume_on_device = false;
    if self.playback.is_playing {
      self.playback.is_playing = false;
      return vec![Effect::AudioPause, self.state_effect(false)];
    }
    // Après la fin d'un titre, il n'y a plus de voix : on recharge.
    if self.playback.duration.is_none()
      || self.duration_secs() - self.playback.position.as_secs_f64() < 0.5
    {
      return self.start_current(None);
    }
    self.playback.is_playing = true;
    vec![Effect::AudioPlay, self.state_effect(true)]
  }

  fn start_radio(&mut self, start: usize, my_wave: bool) -> Vec<Effect> {
    if let Some(blocked) = self.jam_unavailable() {
      return blocked;
    }
    let Some(track) = self.ui.home.wave.get(start).cloned() else {
      return Vec::new();
    };
    let queue = self.ui.home.wave.clone();
    self.replace_queue(track, queue, start, None);
    self.player.radio = true;
    self.player.my_wave = my_wave;
    self.start_current(None)
  }

  /// `playMyWave`.
  fn play_my_wave(&mut self) -> Vec<Effect> {
    if let Some(blocked) = self.jam_unavailable() {
      return blocked;
    }
    if self.ui.home.wave.is_empty() {
      if self.data.taste_profile(8, now_ms())["trackCount"]
        .as_u64()
        .unwrap_or(0)
        < 5
      {
        return vec![toast_effect("Save at least 5 tracks to tune For you")];
      }
      return self.load_home(false);
    }
    let mut effects = self.start_radio(0, true);
    effects.push(toast_effect("Playing For you"));
    effects
  }

  /// Après le premier écran : accueil, sans réseau avant.
  pub(crate) fn start_home(&mut self) -> Vec<Effect> {
    if self.ui.home.started {
      return Vec::new();
    }
    self.ui.home.started = true;
    let now = now_ms();
    let mut effects = Vec::new();
    let usable = |cache: &Option<TrackCache>| {
      cache
        .as_ref()
        .map_or_else(Vec::new, |cache| cache.tracks.clone())
    };
    self.ui.home.wave = usable(&self.data.wave_cache)
      .into_iter()
      .filter(|track| !self.data.is_blocked(track))
      .collect();
    self.ui.home.trending = usable(&self.data.trending_cache)
      .into_iter()
      .filter(|track| !self.data.is_blocked(track))
      .collect();
    let fresh = |cache: &Option<TrackCache>, age: i64| {
      cache
        .as_ref()
        .and_then(|cache| cache.ts)
        .is_some_and(|ts| now - ts < age)
    };
    if !fresh(&self.data.wave_cache, WAVE_FRESH_MS) || self.ui.home.wave.is_empty() {
      effects.extend(self.load_home(false));
    }
    if !fresh(&self.data.trending_cache, TRENDING_FRESH_MS) || self.ui.home.trending.is_empty() {
      self.ui.home.trending_loading = true;
      effects.push(get_json(ReplyTag::Trending, "/api/trending", Vec::new()));
    }
    effects.extend(self.generate_mixes());
    // Artistes favoris sans image (`renderFavArtistsHome`).
    let missing: Vec<String> = self
      .data
      .followed_artists()
      .iter()
      .filter(|artist| artist.avatar.is_none())
      .map(|artist| artist.name.clone())
      .collect();
    effects.extend(missing.into_iter().take(ARTIST_META_MAX).map(|name| {
      let query = pairs(&[("name", &name)]);
      get_json(ReplyTag::ArtistMeta { name }, "/api/artist", query)
    }));
    effects
  }

  /// L'encart d'import se montre tant qu'il n'est pas fermé et qu'aucune playlist n'existe (`initSpotifyPromo`).
  pub(crate) fn spotify_promo_visible(&self) -> bool {
    !self.settings.spotify_promo_dismissed && self.data.playlists.is_empty()
  }

  fn dismiss_spotify_promo(&mut self) -> Vec<Effect> {
    self.settings.spotify_promo_dismissed = true;
    self.save_settings()
  }

  fn load_home(&mut self, refresh: bool) -> Vec<Effect> {
    self.ui.home.wave_loading = true;
    self.ui.home.wave_failed = false;
    let payload = self.data.vibe_payload(10, refresh, now_ms());
    vec![post_json(ReplyTag::Vibe { refresh }, "/api/vibe", &payload)]
  }

  fn today(&self) -> String {
    library::local_day(now_ms(), self.utc_offset_minutes)
  }

  /// `generateDailyMixes`.
  fn generate_mixes(&mut self) -> Vec<Effect> {
    if !self.wants_daily_mixes() {
      return Vec::new();
    }
    let today = self.today();
    let cached =
      self.data.daily_mixes.as_ref().is_some_and(|mixes| {
        mixes.day.as_deref() == Some(today.as_str()) && !mixes.mixes.is_empty()
      });
    if cached || self.ui.home.mixes_busy {
      return Vec::new();
    }
    let profile = self.data.taste_profile(40, now_ms());
    let buckets = library::plan_mixes(&profile);
    if buckets.is_empty() {
      return Vec::new();
    }
    self.ui.home.mixes_busy = true;
    self.ui.home.mixes_planned = buckets.len();
    // `renderMadeForYou` après `planMixes` (`app.js:25330`) : les cartes réservées sont recréées.
    self.recreate_made_spinners();
    self.data.daily_mixes = Some(DailyMixes {
      day: Some(today.clone()),
      mixes: Vec::new(),
    });
    let jobs = buckets
      .into_iter()
      .map(|bucket| MixJob {
        id: bucket.id,
        title: bucket.title,
        artists: bucket.artists,
        body: json!({
          "history": self.data.history.iter().take(30).collect::<Vec<_>>(),
          "favorites": self.data.favorites.iter().take(25).collect::<Vec<_>>(),
          "platform": "youtube",
          "limit": MIX_LIMIT,
          "clientProfile": profile,
          "seeds": bucket.seeds,
          "avoid": self.data.wave_avoid(),
        })
        .to_string(),
      })
      .collect();
    vec![Effect::GenerateMixes { day: today, jobs }]
  }

  fn open_search(&mut self, query: String) -> Vec<Effect> {
    let query = query.trim().to_string();
    if query.is_empty() {
      return Vec::new();
    }
    let search = &mut self.ui.search;
    search.token += 1;
    let token = search.token;
    search.query.clone_from(&query);
    search.tracks.clear();
    search.albums.clear();
    search.artist = None;
    search.has_more = false;
    // `renderM3SearchLoader` recrée le nœud, mais `startButtonShapeSpinners` (`app.js:9944-9980`) garde `fromIdx`, `toIdx` et
    // `stepStart` tant qu'un `.m3-btn-shape-spinner` reste affiché : `App::track_shape_spinners` date l'apparition et la disparition.
    search.loading = true;
    search.loading_more = false;
    search.error = None;
    search.album_hover = None;
    search.albums_scroll = 0.0;
    search.history_open = false;
    self.ui.lists.remove(&ListId::Search);
    let mut effects = vec![Effect::AbortSearch];
    effects.extend(self.update(crate::app::Message::Navigate(Screen::Search)));
    let platform = self.platform();
    effects.push(Effect::CoreJson {
      tag: ReplyTag::Search { token },
      path: "/api/search",
      query: vec![
        ("q".to_string(), query.clone()),
        ("platform".to_string(), platform),
        ("limit".to_string(), SEARCH_PAGE.to_string()),
        ("offset".to_string(), "0".to_string()),
      ],
      body: None,
      search: true,
    });
    if self.settings.search_albums && query.chars().count() >= SEARCH_MIN_FETCH {
      effects.push(Effect::CoreJson {
        tag: ReplyTag::SearchAlbums { token },
        path: "/api/albums/search",
        query: vec![("q".to_string(), query)],
        body: None,
        search: true,
      });
    }
    effects
  }

  fn search_reply(&mut self, token: u64, result: Result<Value, String>) -> Vec<Effect> {
    if token != self.ui.search.token {
      return Vec::new();
    }
    self.ui.search.loading = false;
    let value = match result {
      Ok(value) if success(&value) && value["tracks"].is_array() => value,
      Ok(value) => {
        self.ui.search.error = Some(
          value["error"]
            .as_str()
            .unwrap_or("Failed to load search results")
            .to_string(),
        );
        self.entered_error();
        let offline = value["offline"].as_bool() == Some(true);
        return if self.data.platform == "ytmusic" && !offline {
          vec![toast_effect("YouTube Music is unavailable")]
        } else {
          Vec::new()
        };
      }
      Err(reason) => {
        self.ui.search.error = Some(reason);
        self.entered_error();
        return Vec::new();
      }
    };
    let query = self.ui.search.query.clone();
    let mut tracks = library::rank_search_tracks(&query, tracks_of(&value["tracks"]));
    for track in &mut tracks {
      self.data.apply_rename(track);
    }
    self.ui.search.has_more = value["hasMore"].as_bool().unwrap_or(false);
    let first_thumb = tracks.first().and_then(Track::thumb_url);
    // `setSearchIconCover(firstThumb)` (`app.js:10377`) : seule une recherche réussie change la vignette.
    self.ui.search.icon_cover.clone_from(&first_thumb);
    self.data.record_search_query(&query, first_thumb, now_ms());
    let mut effects = vec![self.data_changed()];
    if self.data.search_artists && query.chars().count() >= SEARCH_MIN_FETCH {
      let (name, via_track) = artist_lookup(&query, tracks.first());
      effects.push(Effect::CoreJson {
        tag: ReplyTag::SearchArtist { token, via_track },
        path: "/api/artist",
        query: vec![("name".to_string(), name)],
        body: None,
        search: true,
      });
    }
    self.ui.search.tracks = tracks;
    self.entered_rows(Screen::Search);
    effects
  }

  fn search_artist_reply(&mut self, token: u64, via_track: bool, value: &Value) {
    if token != self.ui.search.token || !success(value) {
      return;
    }
    let artist = &value["artist"];
    let Some(name) = artist["name"].as_str().filter(|name| !name.is_empty()) else {
      return;
    };
    if artist["isKnownArtist"].as_bool() != Some(true) {
      return;
    }
    let wanted = library::normalize_match_text(&self.ui.search.query);
    let found = library::normalize_match_text(name);
    if !via_track {
      let exact = found == wanted;
      let close = (found.starts_with(&wanted) || wanted.starts_with(&found))
        && found.chars().count().abs_diff(wanted.chars().count()) <= 3;
      if !exact && !close {
        return;
      }
      let top = self
        .ui
        .search
        .tracks
        .first()
        .map(|track| library::normalize_match_text(track.title.as_deref().unwrap_or("")));
      if top.as_deref() == Some(wanted.as_str()) && found != wanted {
        return;
      }
    }
    let avatar = artist["avatar"]
      .as_str()
      .filter(|avatar| !avatar.is_empty())
      .map(str::to_string)
      .or_else(|| {
        self
          .ui
          .search
          .tracks
          .first()
          .and_then(|track| track.thumbnail.clone())
      });
    self.ui.search.artist = Some(ArtistCard {
      name: name.to_string(),
      avatar,
      fans: artist["fans"]
        .as_f64()
        .or_else(|| artist["nb_fan"].as_f64())
        .unwrap_or(0.0),
      verified: artist["isVerified"].as_bool().unwrap_or(false),
    });
    self.entered_artist();
  }

  fn search_more(&mut self) -> Vec<Effect> {
    let search = &mut self.ui.search;
    if !search.has_more || search.loading_more || search.query.is_empty() {
      return Vec::new();
    }
    search.loading_more = true;
    let (token, offset, query) = (search.token, search.tracks.len(), search.query.clone());
    let platform = self.data.platform.clone();
    vec![Effect::CoreJson {
      tag: ReplyTag::SearchMore { token },
      path: "/api/search",
      query: vec![
        ("q".to_string(), query),
        ("platform".to_string(), platform),
        ("limit".to_string(), SEARCH_PAGE.to_string()),
        ("offset".to_string(), offset.to_string()),
      ],
      body: None,
      search: true,
    }]
  }

  /// Ouvre un album : titres fournis (fichiers locaux, mix), sinon demandés au core.
  pub(crate) fn open_album(
    &mut self,
    album: SavedAlbum,
    tracks: Option<Vec<Track>>,
    grid: Vec<String>,
  ) -> Vec<Effect> {
    self.ui.album.run += 1;
    let run = self.ui.album.run;
    self.ui.lists.remove(&ListId::Album);
    self.ui.album.grid_covers = grid;
    self.ui.album.error = None;
    self.ui.search.history_open = false;
    let mut effects = self.update(crate::app::Message::Navigate(Screen::Album));
    let (id, title, artist) = (
      album.id.clone().unwrap_or_default(),
      album.title.clone().unwrap_or_default(),
      album.artist.clone().unwrap_or_default(),
    );
    self.ui.album.album = album;
    match tracks {
      Some(tracks) => {
        self.ui.album.tracks = tracks;
        self.ui.album.loading = false;
        self.entered_rows(Screen::Album);
      }
      None => {
        self.ui.album.tracks = Vec::new();
        self.ui.album.loading = true;
        effects.push(get_json(
          ReplyTag::AlbumTracks { run },
          "/api/albums/tracks",
          pairs(&[("id", &id), ("title", &title), ("artist", &artist)]),
        ));
      }
    }
    effects
  }

  fn album_reply(&mut self, run: u64, result: Result<Value, String>) {
    if run != self.ui.album.run {
      return;
    }
    self.ui.album.loading = false;
    let value = match result {
      Ok(value)
        if success(&value)
          && value["tracks"]
            .as_array()
            .is_some_and(|tracks| !tracks.is_empty()) =>
      {
        value
      }
      Ok(value) => {
        self.ui.album.error = Some(
          value["error"]
            .as_str()
            .unwrap_or("Failed to load album tracks")
            .to_string(),
        );
        return;
      }
      Err(reason) => {
        self.ui.album.error = Some(reason);
        return;
      }
    };
    if let Some(artist) = value["album"]["artist"]
      .as_str()
      .filter(|artist| !artist.is_empty())
    {
      self.ui.album.album.artist = Some(artist.to_string());
    }
    if let Some(year) = value["album"]["year"]
      .as_str()
      .filter(|year| !year.is_empty())
    {
      self.ui.album.album.year = Some(year.to_string());
    }
    let album = self.ui.album.album.clone();
    let album_id = album.id.clone().unwrap_or_default();
    let album_artist = album.artist.clone().unwrap_or_default();
    let mut tracks = tracks_of(&value["tracks"]);
    for (index, track) in tracks.iter_mut().enumerate() {
      if track.url.as_deref().unwrap_or("").is_empty() && !track.is_local() {
        track.id = Some(format!("alb_{album_id}_{index}"));
      }
      if track.artist.as_deref().unwrap_or("").is_empty() {
        track.artist = Some(if album_artist.is_empty() {
          "Unknown artist".to_string()
        } else {
          album_artist.clone()
        });
      }
      track.album.clone_from(&album.title);
      let ms = track
        .extra
        .get("durationMs")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
      if track.duration.unwrap_or(0.0) <= 0.0 && ms > 0.0 {
        track.duration = Some((ms / 1000.0).round());
      }
      let cover = track
        .cover
        .clone()
        .or_else(|| track.thumbnail.clone())
        .or_else(|| album.cover.clone());
      track.cover.clone_from(&cover);
      track.thumbnail = track.thumbnail.clone().or(cover);
      if track.platform.as_deref().unwrap_or("").is_empty() {
        track.platform = Some("ytmusic".to_string());
      }
      let title = library::strip_featuring(track.title.as_deref().unwrap_or(""));
      let main = library::split_artists(track.artist.as_deref().unwrap_or(""))
        .into_iter()
        .next()
        .unwrap_or_default();
      track.search_query = Some(format!("{main} {title}").trim().to_string());
      self.data.apply_rename(track);
    }
    self.ui.album.tracks = tracks;
    self.entered_rows(Screen::Album);
  }

  /// `openArtistProfile`.
  pub(crate) fn open_artist(&mut self, name: &str) -> Vec<Effect> {
    let clean = library::clean_artist_name(name);
    if clean.is_empty() || clean.eq_ignore_ascii_case("unknown artist") {
      return Vec::new();
    }
    let run = self.ui.artist.run + 1;
    // Page déjà affichée sur l'onglet Albums : `paneTracks` perd `hidden`, son animation repart (`app.js:11911-11918`).
    let pane_returns = self.screen == Screen::Artist && self.ui.artist.tab == ArtistTab::Albums;
    self.ui.artist = crate::ui::ArtistView {
      name: clean.clone(),
      loading: true,
      run,
      ..Default::default()
    };
    self.ui.search.history_open = false;
    self.ui.lists.remove(&ListId::Artist);
    let hint = self
      .player
      .current
      .as_ref()
      .and_then(|track| track.title.clone())
      .unwrap_or_default();
    let mut effects = self.update(crate::app::Message::Navigate(Screen::Artist));
    if self.artist_motion.enabled {
      // `renderM3Loader(el.artistTracksList)` (`app.js:11993`) : le chargeur repart à chaque ouverture.
      self.now = Instant::now();
      self.artist_motion.start_loader(self.now);
      if pane_returns {
        self.artist_motion.start_pane(self.now);
      }
    }
    effects.push(get_json(
      ReplyTag::Artist { run },
      "/api/artist",
      pairs(&[("name", &clean), ("hintTrack", &hint)]),
    ));
    effects
  }

  fn artist_reply(&mut self, run: u64, result: Result<Value, String>) -> Vec<Effect> {
    if run != self.ui.artist.run {
      return Vec::new();
    }
    let view = &mut self.ui.artist;
    view.loading = false;
    let value = result.unwrap_or_default();
    if !success(&value) || !value["artist"].is_object() {
      view.failed = true;
      return Vec::new();
    }
    let artist = &value["artist"];
    view.verified = artist["isVerified"].as_bool().unwrap_or(false);
    let badge_shown = view.verified && self.screen == Screen::Artist;
    view.fans = artist["fans"]
      .as_f64()
      .or_else(|| artist["nb_fan"].as_f64())
      .unwrap_or(0.0);
    view.fans_line = artist["monthlyListeners"]
      .as_str()
      .map(str::to_string)
      .filter(|line| !line.is_empty())
      .unwrap_or_else(|| {
        if view.fans > 0.0 {
          format!(
            "{} listeners",
            riff_core::discovery::format_thousands(view.fans as u64)
          )
        } else {
          String::new()
        }
      });
    let text = |key: &str| {
      artist[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    };
    view.avatar = text("avatar");
    view.banner = text("banner").or_else(|| view.avatar.clone());
    view.bio = strip_tags(artist["bio"].as_str().unwrap_or(""))
      .trim()
      .to_string();
    view.tracks = tracks_of(&artist["tracks"]);
    let wanted = view.name.trim().to_lowercase();
    view.albums = artist["albums"]
      .as_array()
      .into_iter()
      .flatten()
      .filter(|album| {
        let count = album["trackCount"].as_f64();
        let by_artist = album["artist"]
          .as_str()
          .is_some_and(|name| name.trim().to_lowercase() == wanted);
        album["title"]
          .as_str()
          .is_some_and(|title| !title.is_empty())
          && by_artist
          && count.is_none_or(|count| count == 0.0 || count > 1.0)
          && album["recordType"].as_str() != Some("single")
      })
      .filter_map(|album| serde_json::from_value(album.clone()).ok())
      .collect();
    if view.albums.is_empty() {
      view.tab = ArtistTab::Tracks;
    }
    let name = view.name.clone();
    let (avatar, fans) = (
      view.avatar.clone(),
      Some(view.fans).filter(|fans| *fans > 0.0),
    );
    // `renderTracks(el.artistTracksList, ...)` : les lignes sont créées à l'arrivée de la page.
    self.entered_rows(Screen::Artist);
    if badge_shown && self.artist_motion.enabled {
      // `classList.toggle('hidden', !isVerified)` (`app.js:12060`) : le badge devient visible, son tour part de zéro.
      self.now = Instant::now();
      self.artist_motion.start_badge(self.now);
    }
    if self.data.is_followed(&name) {
      self.data.update_artist_meta(&name, avatar, fans);
      return vec![self.data_changed()];
    }
    Vec::new()
  }

  fn track_command(&mut self, track: Track, command: TrackCommand) -> Vec<Effect> {
    let now = now_ms();
    match command {
      TrackCommand::ToggleFavorite => {
        self.data.toggle_favorite(&track, now);
        self.entered_rows(Screen::Liked);
        vec![self.data_changed()]
      }
      TrackCommand::AddToPlaylist => self.open_modal(Modal::AddToPlaylist {
        tracks: vec![track],
      }),
      TrackCommand::AddToQueue if self.jam.is_some() => self.jam_add(vec![track], false),
      TrackCommand::PlayNext => {
        if self.jam.is_some() {
          self.jam_add(vec![track], true)
        } else {
          Vec::new()
        }
      }
      TrackCommand::AddToQueue => {
        if self.player.current.is_none() || self.player.queue.is_empty() {
          self.replace_queue(track.clone(), vec![track], 0, None);
          let mut effects = self.start_current(None);
          effects.push(toast_effect_kind("Added to queue", ToastKind::Success));
          return effects;
        }
        self.player.queue.push(track);
        vec![toast_effect_kind("Added to queue", ToastKind::Success)]
      }
      TrackCommand::PinToSidebar => {
        let pinned = self.data.toggle_sidebar_pin(&track);
        vec![
          self.data_changed(),
          toast_effect(if pinned {
            "Pinned to sidebar"
          } else {
            "Unpinned from sidebar"
          }),
        ]
      }
      TrackCommand::PinToTop(scope) => {
        let pinned = self.data.toggle_pin(&scope, &track);
        self.entered_rows(Screen::Liked);
        self.entered_rows(Screen::PlaylistDetail);
        vec![
          self.data_changed(),
          toast_effect(if pinned {
            "Track pinned"
          } else {
            "Track unpinned"
          }),
        ]
      }
      TrackCommand::Download => {
        self.update_download(crate::download_ui::DownloadMessage::Open(Box::new(track)))
      }
      TrackCommand::Share => match library::share_url(&track) {
        Some(url) => vec![
          Effect::CopyText(url),
          toast_effect_kind("Share link copied to clipboard!", ToastKind::Success),
        ],
        None => vec![toast_effect("No link to share for this track")],
      },
      TrackCommand::Rename => {
        let title = track.title.clone().unwrap_or_default();
        let artist = track.artist.clone().unwrap_or_default();
        self.open_modal(Modal::Rename {
          track: Box::new(track),
          title,
          artist,
        })
      }
      TrackCommand::Art => self.open_modal(Modal::TrackArt(Box::new(track))),
      TrackCommand::RemoveFromPlaylist => {
        if let Some(id) = self.ui.playlist_id.clone() {
          self.data.remove_from_playlist(&id, &track);
        }
        vec![self.data_changed()]
      }
      TrackCommand::RemoveFromQueue(index) => {
        if self.jam.is_some() {
          return self.jam_remove_indices(&[index]);
        }
        self.player.remove_from_queue(&[index]);
        Vec::new()
      }
      TrackCommand::RemoveLocal => self.remove_local(std::slice::from_ref(&track)),
      TrackCommand::BlockTrack => {
        let was_current_blocked = self
          .player
          .current
          .as_ref()
          .is_some_and(|current| self.data.is_track_blocked(current));
        let was_favorite = self.data.is_favorite(&track);
        let blocked = self.data.toggle_block_track(&track);
        // Le blocage retire le titre des favoris : même rafraîchissement qu'un retrait normal.
        if blocked && was_favorite {
          self.entered_rows(Screen::Liked);
        }
        let mut effects = vec![
          self.data_changed(),
          toast_effect(if blocked {
            "Blocked track"
          } else {
            "Unblocked track"
          }),
        ];
        // Dislike = skip du titre en cours, sauf dans un jam (la file est commune).
        let is_current_now = self
          .player
          .current
          .as_ref()
          .is_some_and(|current| self.data.is_track_blocked(current));
        if blocked && is_current_now && !was_current_blocked && self.jam.is_none() {
          let was_playing = self.playback.is_playing;
          effects.extend(self.go_next(false, None));
          // Plus rien à jouer (tout est bloqué) : le moteur audio doit s'arrêter.
          if was_playing && !self.playback.is_playing {
            effects.push(Effect::AudioPause);
            effects.push(self.state_effect(false));
          }
        }
        effects
      }
      TrackCommand::BlockArtist => {
        let name = track.artist.clone().unwrap_or_default();
        let blocked = self.data.toggle_block_artist(&name);
        let text = if blocked {
          format!("Blocked artist \"{}\"", name.trim())
        } else {
          format!("Unblocked artist \"{}\"", name.trim())
        };
        vec![self.data_changed(), toast_effect(&text)]
      }
      TrackCommand::OpenArtist(name) => self.open_artist(&name),
    }
  }

  fn remove_local(&mut self, tracks: &[Track]) -> Vec<Effect> {
    let ids = self.data.remove_local_files(tracks);
    let mut effects = vec![self.data_changed()];
    if !ids.is_empty() {
      effects.push(post_json(
        ReplyTag::Ignored,
        "/api/library-remove",
        &json!({ "ids": ids, "clearAll": self.data.local_files.is_empty() }),
      ));
    }
    effects
  }

  /// Entrées du menu contextuel d'une piste (`openTrackContextMenu`, `app.js:10670-10880`).
  fn track_menu(
    &self,
    track: &Track,
    list: Option<(ListId, usize)>,
  ) -> Vec<MenuItem<crate::app::Message>> {
    let command = |label: &str, icon: &'static str, command: TrackCommand| {
      MenuItem::new(
        self.i18n.t(label),
        crate::app::Message::Ui(UiMessage::Track(Box::new(track.clone()), command)),
      )
      .icon(icon)
    };
    let favorite = self.data.is_favorite(track);
    let pinned_to_sidebar = self.data.is_pinned_to_sidebar(track);
    let mut items = vec![
      command(
        if favorite {
          "Remove from Favorites"
        } else {
          "Favorite"
        },
        if favorite {
          icons::CTX_FAV_FILLED
        } else {
          icons::CTX_FAV
        },
        TrackCommand::ToggleFavorite,
      )
      .active(favorite),
      command(
        "Add to playlist",
        icons::CTX_PLAYLIST,
        TrackCommand::AddToPlaylist,
      ),
      command("Add to queue", icons::CTX_QUEUE, TrackCommand::AddToQueue),
      command(
        if pinned_to_sidebar {
          "Unpin from sidebar"
        } else {
          "Pin to sidebar"
        },
        if pinned_to_sidebar {
          icons::CTX_PIN_FILLED
        } else {
          icons::CTX_PIN
        },
        TrackCommand::PinToSidebar,
      )
      .active(pinned_to_sidebar),
    ];
    if self.jam.is_some() {
      items.insert(
        3,
        command("Play next", icons::CTX_QUEUE, TrackCommand::PlayNext),
      );
    }
    if let Some(scope) = self.pin_scope() {
      let pinned = self.data.is_pinned(&scope, track);
      items.push(
        command(
          if pinned { "Unpin" } else { "Pin to top" },
          if pinned {
            icons::CTX_PIN_FILLED
          } else {
            icons::CTX_PIN
          },
          TrackCommand::PinToTop(scope),
        )
        .active(pinned),
      );
    }
    items.push(command("Share link", icons::CTX_SHARE, TrackCommand::Share));
    items.push(command(
      "Download",
      icons::CTX_DOWNLOAD,
      TrackCommand::Download,
    ));
    items.push(command(
      "Change banner",
      icons::CTX_BANNER,
      TrackCommand::Art,
    ));
    items.push(command(
      "Rename track",
      icons::CTX_RENAME,
      TrackCommand::Rename,
    ));
    let removal = match list {
      Some((ListId::Playlist, _)) => Some(command(
        "Remove from playlist",
        icons::CTX_TRASH,
        TrackCommand::RemoveFromPlaylist,
      )),
      Some((ListId::Queue, index)) => Some(command(
        "Remove from queue",
        icons::CTX_X,
        TrackCommand::RemoveFromQueue(index),
      )),
      Some((ListId::Local, _)) => Some(command(
        "Remove from Riff",
        icons::CTX_TRASH,
        TrackCommand::RemoveLocal,
      )),
      _ => None,
    };
    items.extend(removal.map(|item| item.danger().divider_before()));
    items.push(
      command(
        if self.data.is_track_blocked(track) {
          "Unblock track"
        } else {
          "Block track"
        },
        icons::CTX_BLOCK,
        TrackCommand::BlockTrack,
      )
      .danger()
      .divider_before(),
    );
    if let Some(artist) = track.artist.as_deref().filter(|artist| !artist.is_empty()) {
      items.push(
        command(
          if self.data.is_artist_blocked(artist) {
            "Unblock artist"
          } else {
            "Block artist"
          },
          icons::CTX_BLOCK_ARTIST,
          TrackCommand::BlockArtist,
        )
        .danger(),
      );
    }
    items
  }

  pub(crate) fn open_modal(&mut self, modal: Modal) -> Vec<Effect> {
    if matches!(modal, Modal::AddToPlaylist { .. }) {
      self.entered_picks();
    }
    self.ui.modal = Some(modal);
    vec![Effect::FocusOverlay]
  }

  fn close_modal(&mut self) -> Vec<Effect> {
    // `closeConnected` : la carte audio reste affichée 200 ms (`connectedCollapse`).
    if self.overlay_motion.enabled && self.ui.modal == Some(Modal::Audio) {
      self.now = std::time::Instant::now();
      self.ui.audio.motion.close(self.now);
    }
    self.ui.modal = None;
    self.restore_focus_effects()
  }

  /// Ferme la fenêtre ouverte. `true` si elle existait (Échap).
  pub(crate) fn dismiss_modal(&mut self) -> Option<Vec<Effect>> {
    self.ui.modal.is_some().then(|| self.close_modal())
  }

  fn selected_tracks(&self) -> Vec<Track> {
    let Some(list) = self.ui.selection_list else {
      return Vec::new();
    };
    (0..self.list_len(list))
      .filter_map(|index| self.list_track(list, index))
      .filter(|track| {
        self
          .ui
          .selection
          .contains(&track.canonical_id(&self.data.platform))
      })
      .cloned()
      .collect()
  }

  /// Échap efface la sélection (`clearTrackSelection`).
  pub(crate) fn clear_selection(&mut self) -> bool {
    let had = !self.ui.selection.is_empty();
    self.ui.selection.clear();
    self.ui.selection_anchor = None;
    self.ui.selection_list = None;
    had
  }

  fn select_row(&mut self, list: ListId, index: usize) {
    if self.ui.selection_list != Some(list) {
      self.clear_selection();
      self.ui.selection_list = Some(list);
    }
    let platform = self.platform();
    let anchor = self
      .ui
      .selection_anchor
      .filter(|_| self.ui.modifiers.shift());
    if let Some(anchor) = anchor {
      for at in anchor.min(index)..=anchor.max(index) {
        if let Some(track) = self.list_track(list, at) {
          self.ui.selection.insert(track.canonical_id(&platform));
        }
      }
    } else if let Some(key) = self
      .list_track(list, index)
      .map(|track| track.canonical_id(&platform))
      && !self.ui.selection.remove(&key)
    {
      self.ui.selection.insert(key);
    }
    self.ui.selection_anchor = Some(index);
    if self.ui.selection.is_empty() {
      self.ui.selection_list = None;
    }
  }

  fn reorder(&mut self, list: ListId, from: usize, to: usize) -> Vec<Effect> {
    fn apply(tracks: &mut Vec<Track>, order: Vec<usize>, from: usize, to: usize) {
      let mut items: Vec<Option<Track>> = std::mem::take(tracks).into_iter().map(Some).collect();
      let mut sorted: Vec<Track> = order
        .into_iter()
        .filter_map(|index| items[index].take())
        .collect();
      track_sort::move_item(&mut sorted, from, to);
      *tracks = sorted;
    }
    match list {
      ListId::Queue => {
        if self.jam.is_some() {
          let Some(uid) = self.jam_queue_uid(from) else {
            return Vec::new();
          };
          let to = u32::try_from(to).unwrap_or(u32::MAX);
          return self.jam_divert(Op::Move { uid, to }).unwrap_or_default();
        }
        self.player.move_in_queue(from, to);
        Vec::new()
      }
      ListId::Favorites if self.ui.favorites_query.trim().is_empty() => {
        let order = self.ui.favorites_order.clone();
        self.data.favorites_sort = SortMode::Manual;
        apply(&mut self.data.favorites, order, from, to);
        vec![self.data_changed()]
      }
      ListId::Playlist if self.ui.playlist_query.trim().is_empty() => {
        let order = self.ui.playlist_order.clone();
        let Some(id) = self.ui.playlist_id.clone() else {
          return Vec::new();
        };
        self.data.playlist_sort = SortMode::Manual;
        if let Some(playlist) = self.data.playlist_mut(&id) {
          apply(&mut playlist.tracks, order, from, to);
        }
        vec![self.data_changed()]
      }
      _ => Vec::new(),
    }
  }

  /// Entrée de menu de la barre latérale : déplacer.
  pub(crate) fn sidebar_order(&self) -> Vec<String> {
    let mut order: Vec<String> = self
      .data
      .sidebar_order
      .iter()
      .filter(|id| DEFAULT_SIDEBAR_ORDER.contains(&id.as_str()))
      .cloned()
      .collect();
    for id in DEFAULT_SIDEBAR_ORDER {
      if !order.iter().any(|known| known == id) {
        order.push(id.to_string());
      }
    }
    order
  }

  /// Écrans de la barre latérale, dans l'ordre choisi.
  #[must_use]
  pub fn sidebar_screens(&self) -> Vec<(String, Screen)> {
    self
      .sidebar_order()
      .into_iter()
      .filter(|id| match id.as_str() {
        "local" => self.settings.show_local,
        "albums" => self.settings.show_albums,
        _ => true,
      })
      .map(|id| {
        let screen = match id.as_str() {
          "favorites" => Screen::Liked,
          "albums" => Screen::Albums,
          "local" => Screen::Library,
          "history" => Screen::History,
          "queue" => Screen::Queue,
          _ => Screen::Home,
        };
        (id, screen)
      })
      .collect()
  }

  /// Quand on ouvre « Mes fichiers » : liste noire, relecture des métadonnées manquantes.
  pub(crate) fn on_show(&mut self, screen: Screen) -> Vec<Effect> {
    self.ui.card_hover = None;
    match screen {
      Screen::Library => {
        self.data.drop_removed_local_files();
        self.refresh_orders();
        if self.ui.local_rescanned {
          return Vec::new();
        }
        self.ui.local_rescanned = true;
        let paths = self.data.local_files_to_rescan();
        if paths.is_empty() {
          Vec::new()
        } else {
          vec![Effect::RescanLocal(paths)]
        }
      }
      _ => Vec::new(),
    }
  }

  /// Fond de survol d'une ligne de titre : `background-color 140ms ease` repart de la valeur courante (`styles.css:1589`).
  fn fade_row(&mut self, list: ListId, index: usize, hovered: bool) {
    self.now = std::time::Instant::now();
    self.ui.row_fade.set(
      (list, index),
      if hovered { 1.0 } else { 0.0 },
      self.now,
      crate::motion::ROW_HOVER,
    );
  }

  /// Messages des écrans.
  pub fn update_ui(&mut self, message: UiMessage) -> Vec<Effect> {
    match message {
      UiMessage::Reply(tag, result) => self.reply(tag, result),
      UiMessage::SaveDue(generation) => {
        if generation == self.data_generation {
          vec![Effect::WriteUserData(Box::new(self.data.clone()))]
        } else {
          Vec::new()
        }
      }
      UiMessage::RowPressed(list, index) => {
        self.ui.press = Some(RowPress {
          list,
          from: index,
          over: index,
        });
        Vec::new()
      }
      UiMessage::RowEntered(list, index) => {
        self.ui.row_hover = Some((list, index));
        self.fade_row(list, index, true);
        if let Some(press) = &mut self.ui.press
          && press.list == list
        {
          press.over = index;
          return Vec::new();
        }
        self
          .list_track(list, index)
          .filter(|track| track.url.is_some() && !track.is_local())
          .map_or_else(Vec::new, |track| {
            vec![prefetch_effect(track, &self.data.platform)]
          })
      }
      UiMessage::RowLeft(list, index) => {
        // Une ligne plus tôt dans l'arbre a pu recevoir l'entrée avant cette sortie.
        if self.ui.row_hover == Some((list, index)) {
          self.ui.row_hover = None;
        }
        self.fade_row(list, index, false);
        Vec::new()
      }
      UiMessage::RowReleased => {
        let Some(press) = self.ui.press.take() else {
          return Vec::new();
        };
        if press.over != press.from && press.list.can_reorder() {
          return self.reorder(press.list, press.from, press.over);
        }
        self.row_activated(press.list, press.over)
      }
      UiMessage::MouseReleased => {
        self.ui.press = None;
        self.ui.prefs.order_drag = None;
        Vec::new()
      }
      UiMessage::TitlebarHover(hovered) => {
        self.ui.titlebar_hovered = hovered;
        Vec::new()
      }
      UiMessage::PanelMoved => {
        let controls = &mut self.ui.panel_controls;
        controls.hovered = true;
        controls.last_move = Some(std::time::Instant::now());
        if controls.visible {
          Vec::new()
        } else {
          self.show_panel_controls()
        }
      }
      UiMessage::PanelLeft => {
        let controls = &mut self.ui.panel_controls;
        controls.hovered = false;
        if !controls.visible {
          return Vec::new();
        }
        controls.generation += 1;
        vec![Effect::PanelControlsTimer(
          controls.generation,
          tokens::right_panel::CONTROLS_AFTER_LEAVE,
        )]
      }
      UiMessage::PanelControlsDue(generation) => {
        let controls = &mut self.ui.panel_controls;
        if generation != controls.generation {
          return Vec::new();
        }
        let after_move = tokens::right_panel::CONTROLS_AFTER_MOVE;
        if controls.hovered {
          let elapsed = controls.last_move.map_or(after_move, |at| at.elapsed());
          if elapsed < after_move {
            return vec![Effect::PanelControlsTimer(generation, after_move - elapsed)];
          }
        }
        controls.visible = false;
        Vec::new()
      }
      UiMessage::CoverHover(hovered) => {
        self.visuals.shape.hovered = hovered;
        Vec::new()
      }
      UiMessage::ControlHover(key, hovered) => {
        self.now = std::time::Instant::now();
        self.ui.controls.set(key, hovered, self.now);
        Vec::new()
      }
      UiMessage::ControlGone(key) => {
        self.ui.controls.forget(key);
        Vec::new()
      }
      UiMessage::Touch(key, press) => {
        self.now = std::time::Instant::now();
        match press {
          Some((origin, size)) => {
            self.ui.controls.press(key, self.now);
            self.ui.controls.ripple(key, origin, size, self.now);
          }
          None => self.ui.controls.release(key, self.now),
        }
        Vec::new()
      }
      UiMessage::PressedElsewhere => {
        self.ui.search.history_open = false;
        self.ui.search.hover = None;
        Vec::new()
      }
      UiMessage::RowActivated(list, index) => self.row_activated(list, index),
      UiMessage::RowMenu(list, index, position) => {
        let Some(track) = self.list_track(list, index).cloned() else {
          return Vec::new();
        };
        let items = self.track_menu(&track, Some((list, index)));
        self.update(crate::app::Message::OpenMenu(Box::new(Menu {
          position,
          items,
        })))
      }
      UiMessage::CurrentMenu(position) => {
        let Some(track) = self.player.current.clone() else {
          return Vec::new();
        };
        let items = self.track_menu(&track, None);
        self.update(crate::app::Message::OpenMenu(Box::new(Menu {
          position,
          items,
        })))
      }
      UiMessage::RowCommand(list, index, command) => self
        .list_track(list, index)
        .cloned()
        .map_or_else(Vec::new, |track| self.track_command(track, command)),
      UiMessage::CurrentCommand(command) => self
        .player
        .current
        .clone()
        .map_or_else(Vec::new, |track| self.track_command(track, command)),
      UiMessage::Track(track, command) => self.track_command(*track, command),
      UiMessage::Modifiers(modifiers) => {
        self.ui.modifiers = modifiers;
        Vec::new()
      }
      UiMessage::ListScrolled(list, offset, height) => {
        let mut state = self.ui.list(list);
        state.scrolled(offset, height);
        self.ui.lists.insert(list, state);
        let total = self.list_len(list) as f32 * crate::ui::ROW_HEIGHT;
        if list == ListId::Search && offset + height >= total - INFINITE_SCROLL_MARGIN {
          return self.search_more();
        }
        Vec::new()
      }
      UiMessage::ClearSelection => {
        self.clear_selection();
        Vec::new()
      }
      UiMessage::SelectAll => {
        if let Some(list) = self.ui.selection_list {
          let platform = self.platform();
          let all: Vec<String> = (0..self.list_len(list))
            .filter_map(|index| {
              self
                .list_track(list, index)
                .map(|track| track.canonical_id(&platform))
            })
            .collect();
          if all.iter().all(|key| self.ui.selection.contains(key)) {
            self.clear_selection();
          } else {
            self.ui.selection.extend(all);
          }
        }
        Vec::new()
      }
      UiMessage::SelectionToPlaylist => {
        let tracks = self.selected_tracks();
        self.open_modal(Modal::AddToPlaylist { tracks })
      }
      UiMessage::SelectionToFavorites => {
        let tracks = self.selected_tracks();
        let added = self.data.add_favorites(&tracks, now_ms());
        self.clear_selection();
        vec![
          self.data_changed(),
          toast_effect(&format!("Added {added} tracks to favorites")),
        ]
      }
      UiMessage::SelectionRemove => self.selection_remove(),
      UiMessage::SearchInput(text) => {
        let empty = text.trim().is_empty();
        self.ui.search.history_open = true;
        self.ui.search.hover = None;
        self.ui.search.input = text;
        if empty && self.screen == Screen::Search {
          return self.leave_search();
        }
        Vec::new()
      }
      UiMessage::SearchSubmit => self.open_search(self.ui.search.input.clone()),
      UiMessage::SearchClear => {
        self.ui.search.input.clear();
        self.ui.search.history_open = false;
        self.leave_search()
      }
      UiMessage::SearchFocus => {
        self.ui.search.history_open = true;
        Vec::new()
      }
      UiMessage::HistoryHover(index) => {
        self.ui.search.hover = index;
        Vec::new()
      }
      UiMessage::SetPlatform(platform) => {
        self.data.platform = platform;
        let mut effects = vec![self.data_changed()];
        if !self.ui.search.input.trim().is_empty() {
          effects.extend(self.open_search(self.ui.search.input.clone()));
        }
        effects
      }
      UiMessage::SetSearchFilter(filter) => {
        self.ui.search.filter = filter;
        Vec::new()
      }
      UiMessage::SearchScrolled(offset, height) => {
        let top = crate::screens::search::list_top(&self.ui.search);
        let mut state = self.ui.list(ListId::Search);
        state.scrolled((offset - top).max(0.0), height);
        self.ui.lists.insert(ListId::Search, state);
        let total = top + self.list_len(ListId::Search) as f32 * crate::ui::ROW_HEIGHT;
        if offset + height >= total - INFINITE_SCROLL_MARGIN {
          return self.search_more();
        }
        Vec::new()
      }
      UiMessage::SearchAlbumHover(index, entered) => {
        let search = &mut self.ui.search;
        if entered {
          search.album_hover = Some(index);
        } else if search.album_hover == Some(index) {
          search.album_hover = None;
        }
        Vec::new()
      }
      UiMessage::SearchAlbumsScrolled(offset) => {
        self.ui.search.albums_scroll = offset;
        Vec::new()
      }
      UiMessage::CardHover(grid, index, entered) => {
        if entered {
          self.ui.card_hover = Some((grid, index));
        } else if self.ui.card_hover == Some((grid, index)) {
          self.ui.card_hover = None;
        }
        Vec::new()
      }
      UiMessage::LocalAlbumsScrolled(offset) => {
        self.ui.local_albums_scroll = offset;
        Vec::new()
      }
      UiMessage::LocalScrolled(offset, height) => {
        let has_albums = crate::screens::library::has_albums(&self.data.local_files);
        let top = crate::screens::library::list_top(has_albums);
        let mut state = self.ui.list(ListId::Local);
        state.scrolled((offset - top).max(0.0), height);
        self.ui.lists.insert(ListId::Local, state);
        Vec::new()
      }
      UiMessage::FavoritesScrolled(offset, height) => {
        let top = crate::screens::liked::list_top();
        let mut state = self.ui.list(ListId::Favorites);
        state.scrolled((offset - top).max(0.0), height);
        self.ui.lists.insert(ListId::Favorites, state);
        Vec::new()
      }
      UiMessage::AlbumScrolled(offset, height) => {
        let top = crate::screens::album::list_top();
        let mut state = self.ui.list(ListId::Album);
        state.scrolled((offset - top).max(0.0), height);
        self.ui.lists.insert(ListId::Album, state);
        Vec::new()
      }
      UiMessage::PlaylistScrolled(offset, height) => {
        let top = crate::screens::playlist_detail::list_top();
        let mut state = self.ui.list(ListId::Playlist);
        state.scrolled((offset - top).max(0.0), height);
        self.ui.lists.insert(ListId::Playlist, state);
        Vec::new()
      }
      UiMessage::PlaylistSortOpen(open) => {
        self.ui.playlist_sort_open = open;
        Vec::new()
      }
      UiMessage::HistoryScrolled(offset, height) => {
        let top = crate::screens::history::list_top();
        let mut state = self.ui.list(ListId::History);
        state.scrolled((offset - top).max(0.0), height);
        self.ui.lists.insert(ListId::History, state);
        Vec::new()
      }
      UiMessage::QueueScrolled(offset, height) => {
        let top = crate::screens::queue::list_top();
        let mut state = self.ui.list(ListId::Queue);
        state.scrolled((offset - top).max(0.0), height);
        self.ui.lists.insert(ListId::Queue, state);
        Vec::new()
      }
      UiMessage::CopySearchError => {
        let search = &self.ui.search;
        let report = format!(
          "Search Error: {}
Query: {}
Platform: {}

{}",
          search.error.as_deref().unwrap_or_default(),
          search.query,
          self.data.platform,
          riff_core::maintenance::diagnostics(env!("CARGO_PKG_VERSION")),
        );
        vec![
          Effect::CopyText(report.trim().to_string()),
          toast_effect_kind(
            "Copied error & diagnostics to clipboard",
            ToastKind::Success,
          ),
        ]
      }
      UiMessage::HistoryOpen(index) => self.open_history_entry(index),
      UiMessage::HistoryRemove(index) => {
        let key = self
          .data
          .search_history_matching(&self.ui.search.input)
          .get(index)
          .and_then(|entry| entry.key.clone());
        self.data.search_history.retain(|entry| entry.key != key);
        vec![self.data_changed()]
      }
      UiMessage::HistoryClear => {
        self.data.search_history.clear();
        self.ui.search.history_open = false;
        vec![self.data_changed()]
      }
      UiMessage::OpenSearchArtist => {
        let Some(card) = self.ui.search.artist.clone() else {
          return Vec::new();
        };
        self
          .data
          .record_search_artist(&card.name, card.avatar, card.verified, now_ms());
        let mut effects = vec![self.data_changed()];
        effects.extend(self.open_artist(&card.name));
        effects
      }
      UiMessage::FocusSearch => {
        self.ui.search.history_open = true;
        self.ui.search.hover = None;
        vec![Effect::FocusId(crate::screens::chrome::SEARCH_ID)]
      }
      // `#btn-regen-mixes` : nouvelle vague, puis nouveaux mix si aucun n'est en cours.
      UiMessage::HomeRefresh => {
        let mut effects = if self.ui.home.wave_loading {
          Vec::new()
        } else {
          self.load_home(true)
        };
        if !self.ui.home.mixes_busy {
          self.data.daily_mixes = None;
          effects.extend(self.generate_mixes());
        }
        effects
      }
      UiMessage::HomeHover(row, index, entered) => {
        self.now = std::time::Instant::now();
        self.ui.home.lift.set(
          (row, index),
          if entered { 1.0 } else { 0.0 },
          self.now,
          crate::motion::CARD,
        );
        let key = Some((row, index));
        if entered {
          self.ui.home.hover = key;
        } else if self.ui.home.hover == key {
          // Une carte plus tôt dans l'arbre a pu recevoir l'entrée avant cette sortie.
          self.ui.home.hover = None;
        }
        Vec::new()
      }
      UiMessage::HomeRowScrolled(row, offset) => {
        if let Some(slot) = row.index() {
          self.ui.home.scroll[slot] = offset;
        }
        Vec::new()
      }
      UiMessage::SpotifyPromoClose => self.dismiss_spotify_promo(),
      UiMessage::SpotifyPromoOpen => {
        let mut effects = self.dismiss_spotify_promo();
        self.show_settings_category(Category::Integrations.key());
        effects.extend(self.update(Message::Navigate(Screen::Settings)));
        effects
      }
      UiMessage::OpenMix(index) => self.open_mix(index),
      UiMessage::RateMix(like) => self.rate_mix(like),
      UiMessage::MixReady {
        day,
        id,
        title,
        artists,
        tracks,
      } => {
        if tracks.len() < MIX_MIN_TRACKS {
          return Vec::new();
        }
        let mixes = self
          .data
          .daily_mixes
          .get_or_insert_with(DailyMixes::default);
        if mixes.day.as_deref() != Some(day.as_str()) {
          *mixes = DailyMixes {
            day: Some(day),
            mixes: Vec::new(),
          };
        }
        mixes.mixes.push(DailyMix {
          id: Some(id),
          title: Some(title),
          artists,
          tracks,
          ..DailyMix::default()
        });
        // `renderMadeForYou` à chaque mix reçu (`app.js:25368`) : toutes les cartes réservées sont recréées.
        self.recreate_made_spinners();
        vec![self.data_changed()]
      }
      UiMessage::MixesDone => {
        self.ui.home.mixes_busy = false;
        self.ui.home.mixes_planned = 0;
        // `renderMadeForYou` dans le `finally` de `generateDailyMixes` (`app.js:25386`).
        self.recreate_made_spinners();
        Vec::new()
      }
      UiMessage::OpenAlbum(album) => self.open_album(*album, None, Vec::new()),
      UiMessage::OpenSavedAlbum(index) => self
        .data
        .saved_albums
        .get(index)
        .cloned()
        .map_or_else(Vec::new, |album| self.open_album(album, None, Vec::new())),
      UiMessage::OpenSearchAlbum(index) => self
        .ui
        .search
        .albums
        .get(index)
        .cloned()
        .map_or_else(Vec::new, |album| self.open_album(album, None, Vec::new())),
      UiMessage::OpenArtistAlbum(index) => self
        .ui
        .artist
        .albums
        .get(index)
        .cloned()
        .map_or_else(Vec::new, |album| self.open_album(album, None, Vec::new())),
      UiMessage::OpenLocalAlbum(name) => self.open_local_album(&name),
      UiMessage::ToggleSaveAlbum => {
        let album = self.ui.album.album.clone();
        let saved = self.data.toggle_saved_album(&album);
        if saved && self.data.like_album_tracks {
          let tracks = self.ui.album.tracks.clone();
          self.data.add_favorites(&tracks, now_ms());
        }
        vec![self.data_changed()]
      }
      UiMessage::OpenArtist(name) => self.open_artist(&name),
      UiMessage::ArtistTab(tab) => {
        self.ui.artist.tab = tab;
        if self.artist_motion.enabled {
          // Le clic rejoue `artistPaneSlideIn` sur le panneau choisi, même si l'onglet était déjà actif.
          self.now = Instant::now();
          self.artist_motion.start_pane(self.now);
        }
        Vec::new()
      }
      UiMessage::ArtistToggleExpanded => {
        self.ui.artist.expanded = !self.ui.artist.expanded;
        self.entered_rows(Screen::Artist);
        Vec::new()
      }
      UiMessage::ArtistFollow => {
        let view = self.ui.artist.clone();
        let followed = self.data.toggle_follow(
          &view.name,
          view.avatar.as_deref(),
          Some(view.fans).filter(|fans| *fans > 0.0),
        );
        vec![
          self.data_changed(),
          toast_effect(&format!(
            "{} {}",
            if followed { "Following" } else { "Unfollowed" },
            view.name
          )),
        ]
      }
      UiMessage::ArtistBlock => {
        let name = self.ui.artist.name.clone();
        let blocked = self.data.toggle_block_artist(&name);
        let text = if blocked {
          format!("Blocked artist \"{name}\"")
        } else {
          format!("Unblocked artist \"{name}\"")
        };
        vec![self.data_changed(), toast_effect(&text)]
      }
      UiMessage::ArtistBio => {
        let (title, body) = (self.ui.artist.name.clone(), self.ui.artist.bio.clone());
        self.open_modal(Modal::Biography { title, body })
      }
      UiMessage::OpenFollowedArtist(index) => {
        let name = self
          .data
          .followed_artists()
          .get(index)
          .map(|artist| artist.name.clone());
        name.map_or_else(Vec::new, |name| self.open_artist(&name))
      }
      UiMessage::FavoritesQuery(query) => {
        self.ui.favorites_query = query;
        self.refresh_orders();
        self.entered_rows(Screen::Liked);
        Vec::new()
      }
      UiMessage::LocalQuery(query) => {
        self.ui.local_query = query;
        self.refresh_orders();
        self.entered_rows(Screen::Library);
        Vec::new()
      }
      UiMessage::PlaylistQuery(query) => {
        self.ui.playlist_query = query;
        self.refresh_orders();
        self.entered_rows(Screen::PlaylistDetail);
        Vec::new()
      }
      UiMessage::SetFavoritesSort(mode) => {
        self.ui.playlist_sort_open = false;
        self.data.favorites_sort = mode;
        self.entered_rows(Screen::Liked);
        vec![self.data_changed()]
      }
      UiMessage::SetPlaylistSort(mode) => {
        self.ui.playlist_sort_open = false;
        self.data.playlist_sort = mode;
        self.entered_rows(Screen::PlaylistDetail);
        vec![self.data_changed()]
      }
      UiMessage::PickFiles => vec![Effect::PickFiles],
      UiMessage::PickFolder => vec![Effect::PickFolder],
      UiMessage::LocalPicked(paths, folders) => vec![Effect::ScanLocal(paths, folders)],
      UiMessage::LocalFound(tracks) => {
        let added = self.data.merge_local_files(tracks);
        let mut effects = vec![self.data_changed()];
        if added > 0 {
          effects.push(toast_effect(&format!(
            "Added {added} track{}",
            if added == 1 { "" } else { "s" }
          )));
        }
        effects
      }
      UiMessage::LocalRescanned(tracks) => {
        self.data.apply_rescan(tracks);
        vec![self.data_changed()]
      }
      UiMessage::ClearHistory => {
        self.data.history.clear();
        self.entered_rows(Screen::History);
        vec![self.data_changed()]
      }
      UiMessage::ClearQueue => {
        if self.jam.is_some() {
          return self.jam_divert(Op::Clear).unwrap_or_default();
        }
        self.player.clear_queue();
        Vec::new()
      }
      UiMessage::OpenPlaylist(id) => self.open_playlist(id),
      UiMessage::NewPlaylist => self.open_modal(Modal::CreatePlaylist {
        name: String::new(),
        tracks: Vec::new(),
      }),
      UiMessage::ModalName(text) => {
        if let Some(Modal::CreatePlaylist { name, .. }) = &mut self.ui.modal {
          *name = text;
        }
        Vec::new()
      }
      UiMessage::ModalTitle(text) => {
        if let Some(Modal::Rename { title, .. }) = &mut self.ui.modal {
          *title = text;
        }
        Vec::new()
      }
      UiMessage::ModalArtist(text) => {
        if let Some(Modal::Rename { artist, .. }) = &mut self.ui.modal {
          *artist = text;
        }
        Vec::new()
      }
      UiMessage::ModalConfirm => self.modal_confirm(),
      UiMessage::ModalChoose(id) => self.modal_choose(&id),
      UiMessage::ModalClose => self.close_modal(),
      UiMessage::PlaylistRename(name) => {
        if let Some(id) = self.ui.playlist_id.clone()
          && let Some(playlist) = self.data.playlist_mut(&id)
        {
          playlist.name = Some(name);
          return vec![self.data_changed()];
        }
        Vec::new()
      }
      UiMessage::PlaylistDescription(text) => {
        if let Some(id) = self.ui.playlist_id.clone()
          && let Some(playlist) = self.data.playlist_mut(&id)
        {
          playlist.description = Some(text);
          return vec![self.data_changed()];
        }
        Vec::new()
      }
      UiMessage::PlaylistPlay => {
        let Some(scope) = self.list_scope(ListId::Playlist) else {
          return Vec::new();
        };
        let list = self.list_as_queue(ListId::Playlist);
        self.play_all(list, scope)
      }
      UiMessage::FavoritesPlay => {
        let list = self.list_as_queue(ListId::Favorites);
        self.play_all(list, FAVORITES_SCOPE.to_string())
      }
      UiMessage::ListShuffleToggle => {
        let Some(scope) = self.pin_scope() else {
          return Vec::new();
        };
        let on = self.data.toggle_shuffle_scope(&scope);
        if self.player.scope.as_deref() == Some(scope.as_str()) {
          self.player.shuffle = on;
        }
        vec![self.data_changed()]
      }
      UiMessage::FavoritesExport => {
        let exported_at = jiff::Timestamp::now()
          .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
          .to_string();
        let name = self
          .data
          .liked_custom_name()
          .unwrap_or(FAVORITES_EXPORT_NAME);
        let content = serde_json::to_string_pretty(&library::tracks_export(
          name,
          self.data.liked_description.as_deref().unwrap_or(""),
          &self.data.favorites,
          &exported_at,
        ))
        .unwrap_or_default();
        let name = format!("{}.json", library::safe_file_name(name));
        vec![Effect::ExportPlaylist { name, content }]
      }
      UiMessage::FavoritesRename(name) => {
        self.data.liked_name = Some(name);
        vec![self.data_changed()]
      }
      UiMessage::FavoritesDescription(text) => {
        self.data.liked_description = Some(text);
        vec![self.data_changed()]
      }
      UiMessage::FavoritesPickCover => vec![Effect::PickFavoritesCover],
      UiMessage::FavoritesCoverPicked(path) => {
        if let Some(path) = path {
          self.data.liked_cover = Some(path.to_string_lossy().to_string());
          return vec![self.data_changed()];
        }
        Vec::new()
      }
      UiMessage::CoverMenu(position) => {
        let liked = self.screen == Screen::Liked;
        let custom = if liked {
          self.data.liked_custom_cover().is_some()
        } else {
          self
            .ui
            .playlist_id
            .as_deref()
            .and_then(|id| self.data.playlist(id))
            .is_some_and(|playlist| {
              playlist
                .cover
                .as_deref()
                .is_some_and(|cover| !cover.trim().is_empty())
            })
        };
        let item = |label: &str, message: UiMessage| {
          MenuItem::new(self.i18n.t(label), crate::app::Message::Ui(message))
        };
        let mut items = vec![item(
          "Change cover",
          if liked {
            UiMessage::FavoritesPickCover
          } else {
            UiMessage::PlaylistPickCover
          },
        )];
        if custom {
          items.push(item("Reset cover", UiMessage::CoverReset));
        }
        self.update(crate::app::Message::OpenMenu(Box::new(Menu {
          position,
          items,
        })))
      }
      UiMessage::CoverReset => {
        if self.screen == Screen::Liked {
          self.data.liked_cover = None;
        } else if let Some(id) = self.ui.playlist_id.clone()
          && let Some(playlist) = self.data.playlist_mut(&id)
        {
          playlist.cover = None;
        } else {
          return Vec::new();
        }
        vec![self.data_changed()]
      }
      UiMessage::PlaylistDelete => self.confirm_playlist_delete(),
      UiMessage::PlaylistDeleted(id) => {
        self.data.delete_playlist(&id);
        if self.player.scope.as_deref() == Some(format!("pl:{id}").as_str()) {
          self.player.scope = None;
        }
        self.ui.playlist_id = None;
        let mut effects = vec![self.data_changed(), toast_effect("Playlist deleted")];
        effects.extend(self.update(crate::app::Message::Navigate(Screen::Home)));
        effects
      }
      UiMessage::PlaylistPickCover => self
        .ui
        .playlist_id
        .clone()
        .map_or_else(Vec::new, |id| vec![Effect::PickPlaylistCover(id)]),
      UiMessage::PlaylistCoverPicked(id, path) => {
        if let Some(path) = path
          && let Some(playlist) = self.data.playlist_mut(&id)
        {
          playlist.cover = Some(path.to_string_lossy().to_string());
          return vec![self.data_changed()];
        }
        Vec::new()
      }
      UiMessage::PlaylistExport(id) => {
        let Some(playlist) = self.data.playlist(&id) else {
          return Vec::new();
        };
        let exported_at = jiff::Timestamp::now()
          .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
          .to_string();
        let content =
          serde_json::to_string_pretty(&library::playlist_export(playlist, &exported_at))
            .unwrap_or_default();
        let name = format!(
          "{}.json",
          library::safe_file_name(playlist.name.as_deref().unwrap_or("playlist"))
        );
        vec![Effect::ExportPlaylist { name, content }]
      }
      UiMessage::PlaylistExported(result) => match result {
        Ok(_) => vec![toast_effect_kind("Exported 1 playlist", ToastKind::Success)],
        Err(reason) => vec![toast_effect_kind(&reason, ToastKind::Error)],
      },
      UiMessage::PlaylistMenu(id, position) => {
        let item = |label: &str, message: UiMessage| {
          MenuItem::new(self.i18n.t(label), crate::app::Message::Ui(message))
        };
        let items = vec![item("Export as JSON", UiMessage::PlaylistExport(id))];
        self.update(crate::app::Message::OpenMenu(Box::new(Menu {
          position,
          items,
        })))
      }
      UiMessage::SidebarTab(tab) => {
        self.entered_sidebar();
        self.ui.sidebar_tab = tab;
        self.ui.sidebar_hover = None;
        self.ui.sidebar_scroll = 0.0;
        vec![Effect::SidebarFeedTop]
      }
      UiMessage::SidebarPlayTrack(track) => self.play_sidebar_track(*track),
      UiMessage::SidebarRowEntered(index) => {
        self.ui.sidebar_hover = Some(index);
        Vec::new()
      }
      UiMessage::SidebarRowLeft(index) => {
        // Une rangée plus tôt dans l'arbre a pu recevoir l'entrée avant cette sortie.
        if self.ui.sidebar_hover == Some(index) {
          self.ui.sidebar_hover = None;
        }
        Vec::new()
      }
      UiMessage::SidebarFeedScrolled(offset) => {
        self.ui.sidebar_scroll = offset;
        Vec::new()
      }
      UiMessage::QueuePanelPlay(index) => self.play_row(ListId::Queue, index),
      UiMessage::ToggleQueuePanel => {
        let was_open = self.ui.queue_panel.open;
        self.ui.queue_panel.open = !was_open;
        if self.view_motion.enabled {
          self
            .ui
            .queue_panel
            .motion
            .set_open(was_open, !was_open, self.now);
        }
        if !was_open && self.ui.jam_panel.open {
          self.ui.jam_panel.open = false;
          if self.view_motion.enabled {
            self.ui.jam_panel.motion.set_open(true, false, self.now);
          }
        }
        Vec::new()
      }
      UiMessage::QueuePanelTab(tab) => {
        self.ui.queue_panel.tab = tab;
        Vec::new()
      }
      UiMessage::QueuePanelPlayRecent(index) => self.play_recent(index),
      UiMessage::SidebarMenu(id, position) => {
        let item = |label: &str, message: UiMessage| {
          MenuItem::new(self.i18n.t(label), crate::app::Message::Ui(message))
        };
        let items = vec![
          item("Move up", UiMessage::SidebarMove(id.clone(), -1)),
          item("Move down", UiMessage::SidebarMove(id, 1)),
          item("Reset", UiMessage::SidebarReset),
        ];
        self.update(crate::app::Message::OpenMenu(Box::new(Menu {
          position,
          items,
        })))
      }
      UiMessage::SidebarMove(id, delta) => {
        let mut order = self.sidebar_order();
        if let Some(from) = order.iter().position(|known| *known == id) {
          let to = from.saturating_add_signed(delta as isize);
          track_sort::move_item(&mut order, from, to);
        }
        self.data.sidebar_order = order;
        vec![self.data_changed()]
      }
      UiMessage::SidebarReset => {
        self.data.sidebar_order.clear();
        vec![
          self.data_changed(),
          toast_effect("Sidebar & home layout reset to default"),
        ]
      }
      UiMessage::SidebarDragStart => {
        self.ui.sidebar_drag = Some(SidebarDrag {
          start_width: self.data.sidebar_width,
          start_x: None,
        });
        Vec::new()
      }
      UiMessage::SidebarDragMoved(x) => {
        if let Some(drag) = &mut self.ui.sidebar_drag {
          match drag.start_x {
            None => drag.start_x = Some(x),
            Some(start) => {
              self.data.sidebar_width = library::snap_sidebar_width(
                drag.start_width as f32 + (x - start) / crate::theme::tokens::ZOOM,
              )
            }
          }
        }
        Vec::new()
      }
      UiMessage::SidebarDragEnd => {
        if self.ui.sidebar_drag.take().is_some() {
          vec![self.data_changed()]
        } else {
          Vec::new()
        }
      }
      UiMessage::SidebarToggle => {
        self.ui.sidebar_drag = None;
        self.data.sidebar_width = if self.data.sidebar_width > library::SIDEBAR_COLLAPSED_WIDTH {
          library::SIDEBAR_COLLAPSED_WIDTH
        } else {
          library::SIDEBAR_DEFAULT_WIDTH
        };
        vec![self.data_changed()]
      }
      UiMessage::SidebarEdgeHover(hovered) => {
        self.ui.sidebar_edge_hovered = hovered;
        Vec::new()
      }
      UiMessage::FileDropped(path) => {
        self.ui.dropped.push(path);
        self.ui.drop_generation += 1;
        vec![Effect::DropTimer(self.ui.drop_generation)]
      }
      UiMessage::DropFlush(generation) => {
        if generation != self.ui.drop_generation {
          return Vec::new();
        }
        let paths = std::mem::take(&mut self.ui.dropped);
        let mut effects = self.update(crate::app::Message::Navigate(Screen::Library));
        effects.push(Effect::ScanDropped(paths));
        effects
      }
      UiMessage::FocusSaved(ordinal) => {
        self.ui.saved_focus = ordinal;
        Vec::new()
      }
    }
  }

  fn row_activated(&mut self, list: ListId, index: usize) -> Vec<Effect> {
    let modifiers = self.ui.modifiers;
    if modifiers.control() || modifiers.shift() || modifiers.command() {
      self.select_row(list, index);
      return Vec::new();
    }
    self.play_row(list, index)
  }

  fn selection_remove(&mut self) -> Vec<Effect> {
    let tracks = self.selected_tracks();
    let list = self.ui.selection_list;
    let platform = self.platform();
    let selected = std::mem::take(&mut self.ui.selection);
    self.clear_selection();
    match list {
      Some(ListId::Local) => {
        let mut effects = self.remove_local(&tracks);
        effects.push(toast_effect("Removed from Local Files"));
        effects
      }
      Some(ListId::Playlist) => {
        if let Some(id) = self.ui.playlist_id.clone() {
          for track in &tracks {
            self.data.remove_from_playlist(&id, track);
          }
        }
        vec![
          self.data_changed(),
          toast_effect(&format!("Removed {} tracks", tracks.len())),
        ]
      }
      Some(ListId::Queue) => {
        let indices: Vec<usize> = (0..self.player.queue.len())
          .filter(|&index| selected.contains(&self.player.queue[index].canonical_id(&platform)))
          .collect();
        if self.jam.is_some() {
          return self.jam_remove_indices(&indices);
        }
        self.player.remove_from_queue(&indices);
        vec![toast_effect("Removed tracks from queue")]
      }
      _ => Vec::new(),
    }
  }

  fn leave_search(&mut self) -> Vec<Effect> {
    self.ui.search.query.clear();
    self.ui.search.has_more = false;
    self.ui.search.token += 1;
    let mut effects = vec![Effect::AbortSearch];
    if self.screen == Screen::Search {
      effects.extend(self.update(crate::app::Message::Navigate(Screen::Home)));
    }
    effects
  }

  /// Échap dans le champ de recherche, ou sur les résultats.
  pub(crate) fn escape_search(&mut self) -> Option<Vec<Effect>> {
    if self.ui.search.history_open {
      self.ui.search.history_open = false;
      return Some(Vec::new());
    }
    if self.screen == Screen::Search {
      self.ui.search.input.clear();
      return Some(self.leave_search());
    }
    None
  }

  fn open_history_entry(&mut self, index: usize) -> Vec<Effect> {
    let Some(entry) = self
      .data
      .search_history_matching(&self.ui.search.input)
      .get(index)
      .map(|entry| (*entry).clone())
    else {
      return Vec::new();
    };
    self.ui.search.history_open = false;
    match entry.kind.as_deref() {
      Some("track") if entry.track.is_some() => {
        let Some(track) = entry.track else {
          return Vec::new();
        };
        self.data.record_search_track(&track, now_ms());
        if self.jam.is_some() {
          return self.jam_play_now(vec![track]);
        }
        self.replace_queue(track.clone(), vec![track], 0, None);
        self.start_current(None)
      }
      Some("artist") => {
        let name = entry.title.unwrap_or_default();
        self.ui.search.input.clear();
        let verified = entry
          .extra
          .get("verified")
          .and_then(Value::as_bool)
          .unwrap_or(false);
        self
          .data
          .record_search_artist(&name, entry.thumb, verified, now_ms());
        let mut effects = vec![self.data_changed()];
        effects.extend(self.open_artist(&name));
        effects
      }
      _ => {
        let query = entry.query.or(entry.title).unwrap_or_default();
        self.ui.search.input.clone_from(&query);
        self.open_search(query)
      }
    }
  }

  fn open_mix(&mut self, index: usize) -> Vec<Effect> {
    let Some(mix) = self
      .data
      .daily_mixes
      .as_ref()
      .and_then(|mixes| mixes.mixes.get(index))
      .cloned()
    else {
      return Vec::new();
    };
    let title = mix
      .title
      .clone()
      .unwrap_or_else(|| format!("Daily Mix {}", index + 1));
    let mut seen = HashSet::new();
    let grid: Vec<String> = mix
      .tracks
      .iter()
      .filter_map(Track::thumb_url)
      .filter(|url| seen.insert(url.clone()))
      .take(4)
      .collect();
    let album = SavedAlbum {
      id: Some(
        mix
          .id
          .clone()
          .unwrap_or_else(|| format!("daily-mix-{}", index + 1)),
      ),
      title: Some(title),
      kind: Some("Daily Mix".to_string()),
      artist: Some(if mix.artists.is_empty() {
        "Daily Mix".to_string()
      } else {
        mix.artists.join(", ")
      }),
      track_count: Some(mix.tracks.len() as f64),
      cover: grid.first().cloned(),
      ..SavedAlbum::default()
    };
    let grid = if grid.len() >= 4 { grid } else { Vec::new() };
    self.open_album(album, Some(mix.tracks), grid)
  }

  fn rate_mix(&mut self, like: bool) -> Vec<Effect> {
    let album = self.ui.album.album.clone();
    let tracks = self.ui.album.tracks.clone();
    self.data.rate_mix(
      album.id.as_deref().unwrap_or(""),
      album.artist.as_deref(),
      &tracks,
      like,
    );
    let (text, kind) = if like {
      ("Glad you like this mix!", ToastKind::Success)
    } else {
      ("Thanks, we will play this less", ToastKind::Info)
    };
    vec![self.data_changed(), toast_effect_kind(text, kind)]
  }

  fn open_local_album(&mut self, name: &str) -> Vec<Effect> {
    let mut tracks: Vec<Track> = self
      .data
      .local_files
      .iter()
      .filter(|track| track.album.as_deref().map(str::trim) == Some(name))
      .cloned()
      .collect();
    let number = |track: &Track| {
      track
        .extra
        .get("trackNumber")
        .and_then(Value::as_i64)
        .unwrap_or(0)
    };
    tracks.sort_by_key(number);
    let album = SavedAlbum {
      id: Some(format!("local_alb_{name}")),
      title: Some(name.to_string()),
      artist: tracks.first().and_then(|track| track.artist.clone()),
      cover: tracks.iter().find_map(Track::thumb_url),
      kind: Some("Local Album".to_string()),
      track_count: Some(tracks.len() as f64),
      ..SavedAlbum::default()
    };
    self.open_album(album, Some(tracks), Vec::new())
  }

  fn open_playlist(&mut self, id: String) -> Vec<Effect> {
    if self.data.playlist(&id).is_none() {
      return Vec::new();
    }
    self.ui.playlist_id = Some(id);
    self.ui.playlist_query.clear();
    self.ui.playlist_sort_open = false;
    self.ui.lists.remove(&ListId::Playlist);
    self.refresh_orders();
    let effects = self.update(crate::app::Message::Navigate(Screen::PlaylistDetail));
    // Déjà sur le détail d'une playlist : `Navigate` sort tôt, `openPlaylistDetail` rappelle `renderTracks` (`app.js:13440`).
    self.entered_rows(Screen::PlaylistDetail);
    effects
  }

  fn modal_confirm(&mut self) -> Vec<Effect> {
    let Some(modal) = self.ui.modal.clone() else {
      return Vec::new();
    };
    match modal {
      Modal::CreatePlaylist { name, tracks } => {
        let now = now_ms();
        let unique = self.next_unique();
        let Some(id) = self.data.create_playlist(&name, tracks, now, unique) else {
          return Vec::new();
        };
        let mut effects = self.close_modal();
        effects.push(self.data_changed());
        effects.extend(self.open_playlist(id));
        effects
      }
      Modal::Rename {
        track,
        title,
        artist,
      } => {
        self.data.rename_track(&track, &title, &artist, now_ms());
        let platform = self.platform();
        let (new_title, new_artist) = (title.trim().to_string(), artist.trim().to_string());
        let apply = |entry: &mut Track| {
          if entry.is_same(&track, &platform) {
            entry.title = Some(if new_title.is_empty() {
              "Untitled track".to_string()
            } else {
              new_title.clone()
            });
            entry.artist = Some(if new_artist.is_empty() {
              "Unknown artist".to_string()
            } else {
              new_artist.clone()
            });
          }
        };
        self.player.queue.iter_mut().for_each(&apply);
        self.ui.search.tracks.iter_mut().for_each(&apply);
        self.ui.album.tracks.iter_mut().for_each(&apply);
        self.ui.artist.tracks.iter_mut().for_each(&apply);
        if let Some(current) = &mut self.player.current {
          apply(current);
        }
        let mut effects = self.close_modal();
        effects.push(self.data_changed());
        effects.push(self.state_effect(self.playback.is_playing));
        effects.push(toast_effect("Track info updated"));
        effects
      }
      Modal::AddToPlaylist { .. }
      | Modal::Biography { .. }
      | Modal::Audio
      | Modal::Tools
      | Modal::Download
      | Modal::TrackArt(_)
      | Modal::Import(_)
      | Modal::ImportReport(_) => self.close_modal(),
    }
  }

  fn modal_choose(&mut self, id: &str) -> Vec<Effect> {
    let Some(Modal::AddToPlaylist { tracks }) = self.ui.modal.clone() else {
      return Vec::new();
    };
    self.data.add_to_playlist(id, &tracks, now_ms());
    let name = self
      .data
      .playlist(id)
      .and_then(|playlist| playlist.name.clone())
      .unwrap_or_default();
    self.clear_selection();
    let mut effects = self.close_modal();
    effects.push(self.data_changed());
    let text = if tracks.len() == 1 {
      format!("Added to \"{name}\"")
    } else {
      format!("Added {} tracks to \"{name}\"", tracks.len())
    };
    effects.push(toast_effect_kind(&text, ToastKind::Success));
    effects
  }

  fn confirm_playlist_delete(&mut self) -> Vec<Effect> {
    let Some(id) = self.ui.playlist_id.clone() else {
      return Vec::new();
    };
    let name = self
      .data
      .playlist(&id)
      .and_then(|playlist| playlist.name.clone())
      .unwrap_or_default();
    let dialog = crate::widgets::dialog::Dialog {
      title: self.i18n.t("Delete playlist"),
      body: format!("Delete playlist \"{name}\"? This action cannot be undone."),
      cancel: self.i18n.t("Cancel"),
      confirm: Some(self.i18n.t("Delete")),
    };
    self.update(crate::app::Message::OpenDialog(
      dialog,
      Some(Box::new(crate::app::Message::Ui(
        UiMessage::PlaylistDeleted(id),
      ))),
    ))
  }

  fn reply(&mut self, tag: ReplyTag, result: Result<Value, String>) -> Vec<Effect> {
    match tag {
      ReplyTag::Lyrics { .. }
      | ReplyTag::LyricsWords { .. }
      | ReplyTag::LyricsTranslated { .. }
      | ReplyTag::LyricsSaved { .. } => self.lyrics_reply(&tag, result),
      ReplyTag::Vibe { refresh } => self.vibe_reply(refresh, result),
      ReplyTag::Hot => {
        self.ui.home.wave_loading = false;
        let tracks: Vec<Track> = result
          .map(|value| tracks_of(&value["tracks"]))
          .unwrap_or_default()
          .into_iter()
          .filter(|track| !self.data.is_blocked(track))
          .collect();
        self.ui.home.wave_failed = tracks.is_empty() && self.ui.home.wave.is_empty();
        if !tracks.is_empty() {
          self.ui.home.wave = tracks;
        }
        Vec::new()
      }
      ReplyTag::Trending => {
        self.ui.home.trending_loading = false;
        let tracks = result
          .map(|value| tracks_of(&value["tracks"]))
          .unwrap_or_default();
        if tracks.is_empty() {
          return Vec::new();
        }
        self.data.trending_cache = Some(TrackCache {
          ts: Some(now_ms()),
          tracks: tracks.clone(),
        });
        self.ui.home.trending = tracks
          .into_iter()
          .filter(|track| !self.data.is_blocked(track))
          .collect();
        vec![self.data_changed()]
      }
      ReplyTag::Search { token } => self.search_reply(token, result),
      ReplyTag::SearchMore { token } => {
        if token != self.ui.search.token {
          return Vec::new();
        }
        self.ui.search.loading_more = false;
        let value = result.unwrap_or_default();
        let more = tracks_of(&value["tracks"]);
        self.ui.search.has_more = !more.is_empty() && value["hasMore"].as_bool().unwrap_or(false);
        let first = self.ui.search.tracks.len();
        self.ui.search.tracks.extend(more);
        self.entered_more_rows(Screen::Search, first);
        Vec::new()
      }
      ReplyTag::SearchAlbums { token } => {
        if token == self.ui.search.token {
          let value = result.unwrap_or_default();
          self.ui.search.albums = value["albums"]
            .as_array()
            .map(|albums| {
              albums
                .iter()
                .filter_map(|album| serde_json::from_value(album.clone()).ok())
                .collect()
            })
            .unwrap_or_default();
        }
        Vec::new()
      }
      ReplyTag::SearchArtist { token, via_track } => {
        self.search_artist_reply(token, via_track, &result.unwrap_or_default());
        Vec::new()
      }
      ReplyTag::AlbumTracks { run } => {
        self.album_reply(run, result);
        Vec::new()
      }
      ReplyTag::Artist { run } => self.artist_reply(run, result),
      ReplyTag::ArtistMeta { name } => {
        let value = result.unwrap_or_default();
        let artist = &value["artist"];
        let avatar = [&artist["avatar"], &artist["banner"]]
          .iter()
          .find_map(|value| value.as_str().filter(|avatar| !avatar.is_empty()))
          .map(str::to_string);
        let fans = artist["fans"].as_f64().filter(|fans| *fans > 0.0);
        self.data.update_artist_meta(&name, avatar, fans);
        vec![self.data_changed()]
      }
      ReplyTag::TrackCover { token } => {
        let cover = result.unwrap_or_default()["cover"]
          .as_str()
          .filter(|cover| !cover.is_empty())
          .map(str::to_string);
        if token == self.player.token
          && let (Some(cover), Some(current)) = (cover, &mut self.player.current)
        {
          current.thumbnail = Some(cover.clone());
          if self.follows_cover() {
            return vec![Effect::FetchCover(cover)];
          }
        }
        Vec::new()
      }
      ReplyTag::RadioRefill => self.radio_reply(result),
      ReplyTag::Manage(reply) => self.manage_reply(reply, result),
      ReplyTag::Ignored => Vec::new(),
    }
  }

  fn vibe_reply(&mut self, refresh: bool, result: Result<Value, String>) -> Vec<Effect> {
    let platform = self.platform();
    let mut seen_ids = HashSet::new();
    let mut seen_keys = HashSet::new();
    let unique: Vec<Track> = result
      .ok()
      .filter(success)
      .map(|value| tracks_of(&value["tracks"]))
      .unwrap_or_default()
      .into_iter()
      .filter(|track| {
        !self.data.is_blocked(track)
          && seen_ids.insert(track.canonical_id(&platform))
          && seen_keys.insert(get_track_normalized_key(
            track.artist.as_deref().unwrap_or(""),
            track.title.as_deref().unwrap_or(""),
          ))
      })
      .collect();
    if unique.len() < WAVE_MIN_TRACKS {
      if !refresh && !unique.is_empty() {
        self.ui.home.wave.clone_from(&unique);
      }
      return vec![get_json(
        ReplyTag::Hot,
        "/api/hot",
        pairs(&[("limit", "12")]),
      )];
    }
    self.ui.home.wave_loading = false;
    self.ui.home.wave_failed = false;
    self.data.wave_cache = Some(TrackCache {
      ts: Some(now_ms()),
      tracks: unique.clone(),
    });
    let seed = now_millis();
    self.ui.home.wave = blend_favorites(&self.data, unique, seed);
    vec![self.data_changed()]
  }
}

/// Texte d'une erreur audio pour le journal : URL réduites à l'hôte, chemin d'un fichier local omis.
fn audio_error_text(err: &AudioError) -> String {
  match err {
    AudioError::Source(_) => "source : ouverture impossible (détail omis)".to_string(),
    other => riff_core::logging::redact_urls(&other.to_string()),
  }
}

pub(crate) fn toast_effect(text: &str) -> Effect {
  toast_effect_kind(text, ToastKind::Info)
}

pub(crate) fn toast_effect_kind(text: &str, kind: ToastKind) -> Effect {
  Effect::Toast(text.to_string(), kind)
}

fn prefetch_effect(track: &Track, platform: &str) -> Effect {
  get_json(
    ReplyTag::Ignored,
    "/api/prefetch",
    pairs(&[
      ("url", track.url.as_deref().unwrap_or("")),
      ("id", track.id.as_deref().unwrap_or("")),
      ("platform", track.platform.as_deref().unwrap_or(platform)),
    ]),
  )
}

fn strip_tags(html: &str) -> String {
  let mut out = String::with_capacity(html.len());
  let mut inside = false;
  for character in html.chars() {
    match character {
      '<' => inside = true,
      '>' if inside => inside = false,
      _ if !inside => out.push(character),
      _ => {}
    }
  }
  out
}

/// `fetchArtistCardForSearch` : si la requête contient l'artiste du premier titre, on cherche cet artiste.
fn artist_lookup(query: &str, first: Option<&Track>) -> (String, bool) {
  let wanted = library::normalize_match_text(query);
  let top = first
    .and_then(|track| track.artist.as_deref())
    .and_then(|artist| library::autoplay_artists(artist).into_iter().next());
  if let Some(top) = top {
    let normalized = library::normalize_match_text(&top);
    let contained = format!(" {wanted} ").contains(&format!(" {normalized} "));
    if normalized.chars().count() >= 2 && normalized != wanted && contained {
      return (top, true);
    }
  }
  (query.to_string(), false)
}

/// `blendFavoritesIntoWave` : quelques favoris d'artistes récents, un tous les 3 titres.
fn blend_favorites(data: &crate::user_data::UserData, wave: Vec<Track>, seed: u64) -> Vec<Track> {
  let platform = &data.platform;
  let recent: Vec<&Track> = data.history.iter().take(20).collect();
  if recent.is_empty() || data.favorites.is_empty() {
    return wave;
  }
  let artist_keys = |track: &Track| -> Vec<String> {
    library::autoplay_artists(track.artist.as_deref().unwrap_or(""))
      .iter()
      .map(|name| library::normalize_artist_name(name))
      .collect()
  };
  let recent_artists: HashSet<String> =
    recent.iter().flat_map(|track| artist_keys(track)).collect();
  let just_played: HashSet<String> = recent
    .iter()
    .take(5)
    .map(|track| track.canonical_id(platform))
    .collect();
  let in_wave: HashSet<String> = wave
    .iter()
    .map(|track| track.canonical_id(platform))
    .collect();
  let mut pool: Vec<&Track> = data
    .favorites
    .iter()
    .filter(|track| {
      let id = track.canonical_id(platform);
      !in_wave.contains(&id)
        && !just_played.contains(&id)
        && !data.is_blocked(track)
        && artist_keys(track)
          .iter()
          .any(|artist| recent_artists.contains(artist))
    })
    .collect();
  if pool.is_empty() {
    return wave;
  }
  let mut state = seed | 1;
  for index in (1..pool.len()).rev() {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    pool.swap(index, (state % (index as u64 + 1)) as usize);
  }
  let count = (wave.len() / 4).max(2).min(pool.len());
  let mut picks = pool.into_iter().take(count);
  let mut out = Vec::with_capacity(wave.len() + count);
  for (index, track) in wave.into_iter().enumerate() {
    out.push(track);
    if (index + 1) % 3 == 0
      && let Some(pick) = picks.next()
    {
      out.push(pick.clone());
    }
  }
  out.extend(picks.cloned());
  out
}

#[must_use]
pub(crate) fn search_filter_shows(filter: SearchFilter, section: SearchFilter) -> bool {
  filter == SearchFilter::All || filter == section
}

#[must_use]
pub(crate) fn tab_title(tab: SidebarTab) -> &'static str {
  match tab {
    SidebarTab::Playlists => "Playlists",
    SidebarTab::Albums => "Albums",
    SidebarTab::Queue => "Queue",
  }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
