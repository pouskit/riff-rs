//! Écran de développement (`--dev`) : prouve le tuyau vers le core et le moteur audio.

use crate::app::{App, Effect, Message};
use crate::settings::ThemeMode;
use crate::theme::ThemeSource;
use crate::theme::file::{ThemeEntry, ThemeStatus};
use crate::widgets::button::{filled, text_button, tonal};
use crate::widgets::dialog::Dialog;
use crate::widgets::focus::focusable;
use crate::widgets::list::VirtualList;
use crate::widgets::menu::{Menu, MenuItem};
use crate::widgets::text_field::text_field;
use crate::widgets::toast::ToastKind;
use iced::widget::{Column, button, column, container, row, scrollable, slider, text};
use iced::{Element, Fill, Point, Theme};
use riff_audio::DeviceInfo;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

const ROW_HEIGHT: f32 = 44.0;
const LIST_HEIGHT: f32 = 280.0;
const LIST_ID: &str = "dev-results";
const PATH_FIELD: &str = "dev-path";
const QUERY_FIELD: &str = "dev-query";
const COVER_FIELD: &str = "dev-cover";
const SEARCH_LIMIT: &str = "50";
/// Volume de départ du moteur (`DEFAULT_VOLUME` de `riff_audio::engine`).
const ENGINE_DEFAULT_VOLUME: f32 = 0.85;
const LANGUAGES: [&str; 3] = ["en", "ru", "uk"];

/// Résultat de recherche affiché brut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRow {
  pub title: String,
  pub artist: String,
  pub thumbnail: String,
}

/// Lit les titres d'une réponse de `/api/search`.
#[must_use]
pub fn rows_from_search(body: &Value) -> Vec<TrackRow> {
  let field = |track: &Value, name: &str| {
    track
      .get(name)
      .and_then(Value::as_str)
      .unwrap_or_default()
      .to_string()
  };
  body
    .get("tracks")
    .and_then(Value::as_array)
    .map(|tracks| {
      tracks
        .iter()
        .map(|track| TrackRow {
          title: field(track, "title"),
          artist: field(track, "artist"),
          thumbnail: field(track, "thumbnail"),
        })
        .collect()
    })
    .unwrap_or_default()
}

/// Messages de l'écran de développement.
#[derive(Debug, Clone)]
pub enum DevMessage {
  PathChanged(String),
  LoadPath,
  Play,
  Pause,
  SeekPreview(f32),
  SeekRelease,
  Volume(f32),
  QueryChanged(String),
  Search,
  SearchDone(Result<Vec<TrackRow>, String>),
  ListScrolled { offset: f32, height: f32 },
  OpenRowMenu(usize, Point),
  CoverPathChanged(String),
  ApplyCoverPath,
  ApplyRowCover(usize),
  DevicesLoaded(Vec<DeviceInfo>),
  SelectDevice(Option<String>),
  ThemesLoaded(Vec<ThemeEntry>),
  PublishState,
}

/// État de l'écran de développement.
#[derive(Debug, Clone)]
pub struct DevState {
  pub path: String,
  pub query: String,
  pub searching: bool,
  pub results: Vec<TrackRow>,
  pub list: VirtualList,
  pub cover_path: String,
  pub devices: Vec<DeviceInfo>,
  pub themes: Vec<ThemeEntry>,
  pub volume: f32,
  pub seek_preview: Option<f32>,
}

impl Default for DevState {
  fn default() -> Self {
    Self {
      path: String::new(),
      query: String::new(),
      searching: false,
      results: Vec::new(),
      list: VirtualList::new(ROW_HEIGHT, LIST_HEIGHT),
      cover_path: String::new(),
      devices: Vec::new(),
      themes: Vec::new(),
      volume: ENGINE_DEFAULT_VOLUME,
      seek_preview: None,
    }
  }
}

/// Mise à jour. Aucune E/S ici.
pub fn update(app: &mut App, message: DevMessage) -> Vec<Effect> {
  let Some(dev) = app.dev.as_mut() else {
    return Vec::new();
  };
  match message {
    DevMessage::PathChanged(path) => dev.path = path,
    DevMessage::LoadPath => {
      let path = dev.path.trim().trim_matches('"');
      if !path.is_empty() {
        let path = PathBuf::from(path);
        return app.update(Message::LoadFile(path));
      }
    }
    DevMessage::Play => {
      app.playback.is_playing = true;
      return vec![Effect::AudioPlay];
    }
    DevMessage::Pause => {
      app.playback.is_playing = false;
      return vec![Effect::AudioPause];
    }
    DevMessage::SeekPreview(seconds) => dev.seek_preview = Some(seconds),
    DevMessage::SeekRelease => {
      if let Some(seconds) = dev.seek_preview.take() {
        let target = Duration::from_secs_f32(seconds.max(0.0));
        app.playback.position = target;
        return vec![Effect::AudioSeek(target)];
      }
    }
    DevMessage::Volume(volume) => {
      dev.volume = volume;
      return vec![Effect::AudioVolume(volume)];
    }
    DevMessage::QueryChanged(query) => dev.query = query,
    DevMessage::Search => {
      let query = dev.query.trim().to_string();
      if !query.is_empty() {
        dev.searching = true;
        return vec![Effect::DevSearch(query, SEARCH_LIMIT)];
      }
    }
    DevMessage::SearchDone(Ok(results)) => {
      dev.searching = false;
      dev.results = results;
      dev.list.offset = 0.0;
    }
    DevMessage::SearchDone(Err(reason)) => {
      dev.searching = false;
      return app.update(Message::ShowToast(reason, ToastKind::Error, None));
    }
    DevMessage::ListScrolled { offset, height } => dev.list.scrolled(offset, height),
    DevMessage::OpenRowMenu(index, position) => {
      let Some(row) = dev.results.get(index) else {
        return Vec::new();
      };
      let items = vec![
        MenuItem::new(
          "Thème depuis la pochette",
          Message::Dev(DevMessage::ApplyRowCover(index)),
        ),
        MenuItem::new(
          "Afficher le titre",
          Message::ShowToast(row.title.clone(), ToastKind::Info, None),
        ),
      ];
      return app.update(Message::OpenMenu(Box::new(Menu { position, items })));
    }
    DevMessage::CoverPathChanged(path) => dev.cover_path = path,
    DevMessage::ApplyCoverPath => {
      let path = dev.cover_path.trim().trim_matches('"');
      if !path.is_empty() {
        return vec![Effect::ReadCoverFile(PathBuf::from(path))];
      }
    }
    DevMessage::ApplyRowCover(index) => {
      if let Some(row) = dev
        .results
        .get(index)
        .filter(|row| !row.thumbnail.is_empty())
      {
        return vec![Effect::FetchCover(row.thumbnail.clone())];
      }
    }
    DevMessage::DevicesLoaded(devices) => dev.devices = devices,
    DevMessage::SelectDevice(id) => return vec![Effect::AudioDevice(id)],
    DevMessage::ThemesLoaded(themes) => dev.themes = themes,
    DevMessage::PublishState => return vec![Effect::PublishState],
  }
  Vec::new()
}

fn dev_message(message: DevMessage) -> Message {
  Message::Dev(message)
}

fn section<'a>(title: &'a str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
  container(column![text(title).size(16), content.into()].spacing(10))
    .padding(16)
    .width(Fill)
    .style(|theme: &Theme| {
      container::Style::default()
        .background(theme.extended_palette().background.weaker.color)
        .border(iced::border::rounded(12))
    })
    .into()
}

fn clock(duration: Duration) -> String {
  let seconds = duration.as_secs();
  format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn playback_section<'a>(app: &'a App, dev: &'a DevState) -> Element<'a, Message> {
  let playback = &app.playback;
  let duration = playback.duration.unwrap_or_default().as_secs_f32();
  let position = dev.seek_preview.unwrap_or(playback.position.as_secs_f32());
  let status = format!(
    "Position {} / {}  |  {} Hz  |  {}{}  |  périphérique : {}",
    clock(playback.position),
    clock(playback.duration.unwrap_or_default()),
    playback.sample_rate.unwrap_or_default(),
    if playback.is_playing {
      "lecture"
    } else {
      "arrêt"
    },
    if playback.buffering {
      ", mémoire tampon"
    } else {
      ""
    },
    playback.device.as_deref().unwrap_or("par défaut"),
  );
  let mut body = column![
    row![
      text_field(
        PATH_FIELD,
        "Chemin d'un fichier audio local",
        &dev.path,
        |path| dev_message(DevMessage::PathChanged(path)),
        Some(dev_message(DevMessage::LoadPath)),
      ),
      filled("Charger", Some(dev_message(DevMessage::LoadPath))),
      tonal("Lecture", Some(dev_message(DevMessage::Play))),
      tonal("Pause", Some(dev_message(DevMessage::Pause))),
    ]
    .spacing(8),
    text(status).size(13),
    row![
      text("Position").size(13),
      slider(
        0.0..=duration.max(0.001),
        position.min(duration.max(0.001)),
        |value| dev_message(DevMessage::SeekPreview(value))
      )
      .on_release(dev_message(DevMessage::SeekRelease)),
    ]
    .spacing(8),
    row![
      text("Volume").size(13),
      slider(0.0..=1.0, dev.volume, |value| dev_message(
        DevMessage::Volume(value)
      ))
      .step(0.01_f32),
    ]
    .spacing(8),
  ]
  .spacing(8);
  if let Some(error) = &playback.error {
    body = body.push(text(error.as_str()).size(13));
  }
  section("Lecture", body)
}

fn result_row<'a>(dev: &'a DevState, index: usize) -> Element<'a, Message> {
  let track = &dev.results[index];
  let line = row![
    text(track.title.as_str()).size(14).width(Fill),
    text(track.artist.as_str()).size(13).width(Fill),
  ]
  .spacing(12);
  let message = dev_message(DevMessage::ApplyRowCover(index));
  let entry = button(line)
    .on_press(message.clone())
    .padding([10, 12])
    .height(ROW_HEIGHT)
    .width(Fill)
    .style(crate::widgets::button::text_style);
  focusable(entry)
    .on_activate(Some(message))
    .on_context(move |position| dev_message(DevMessage::OpenRowMenu(index, position)))
    .into()
}

fn search_section(dev: &DevState) -> Element<'_, Message> {
  let submit = dev_message(DevMessage::Search);
  let results = dev.list.view(
    LIST_ID,
    dev.results.len(),
    |viewport| {
      dev_message(DevMessage::ListScrolled {
        offset: viewport.absolute_offset().y,
        height: viewport.bounds().height,
      })
    },
    |index| result_row(dev, index),
  );
  let label = if dev.searching {
    "Recherche en cours"
  } else {
    "Chercher"
  };
  section(
    "Recherche (core réel)",
    column![
      row![
        text_field(
          QUERY_FIELD,
          "Titre, artiste",
          &dev.query,
          |query| dev_message(DevMessage::QueryChanged(query)),
          Some(submit.clone()),
        ),
        filled(label, Some(submit)),
      ]
      .spacing(8),
      text(format!(
        "{} résultat(s). Clic : thème depuis la pochette, clic droit : menu.",
        dev.results.len()
      ))
      .size(13),
      container(results).height(LIST_HEIGHT),
    ]
    .spacing(8),
  )
}

fn devices_section(dev: &DevState) -> Element<'_, Message> {
  let mut list = Column::new().spacing(4);
  list = list.push(text_button(
    "Périphérique par défaut",
    Some(dev_message(DevMessage::SelectDevice(None))),
  ));
  for device in &dev.devices {
    let label = if device.is_default {
      format!("{} (défaut)", device.name)
    } else {
      device.name.clone()
    };
    list = list.push(text_button(
      label,
      Some(dev_message(DevMessage::SelectDevice(Some(
        device.id.clone(),
      )))),
    ));
  }
  section("Périphériques", list)
}

fn theme_section<'a>(app: &'a App, dev: &'a DevState) -> Element<'a, Message> {
  let mode = |label, mode| {
    if app.settings.theme_mode == mode {
      filled(label, None)
    } else {
      tonal(label, Some(Message::SetThemeMode(mode)))
    }
  };
  let source = match &app.theme.source {
    ThemeSource::Accent => format!("accent {}", app.settings.theme_accent),
    ThemeSource::Cover(color) => format!("pochette {color}"),
    ThemeSource::File(id) => format!("fichier {id}"),
  };
  let mut files = Column::new().spacing(4);
  for entry in &dev.themes {
    files = files.push(match &entry.status {
      ThemeStatus::Valid(_) => Element::from(text_button(
        entry.name.as_str(),
        Some(Message::SelectThemeFile(entry.id.clone())),
      )),
      ThemeStatus::Refused(reason) => text(format!("{} : refusé, {reason}", entry.name))
        .size(13)
        .into(),
      ThemeStatus::Incompatible => text(format!("{} : thème CSS incompatible", entry.name))
        .size(13)
        .into(),
    });
  }
  let mut body = column![
    text(format!(
      "Source : {source}, primaire {}",
      app.theme.scheme.primary
    ))
    .size(13),
    row![
      mode("Sombre", ThemeMode::Dark),
      mode("Clair", ThemeMode::Light),
      mode("Auto (pochette)", ThemeMode::Auto),
      text_button(
        "Quitter le fichier",
        app
          .settings
          .native_theme
          .as_ref()
          .map(|_| Message::ClearThemeFile)
      ),
    ]
    .spacing(8),
    row![
      text_field(
        COVER_FIELD,
        "Chemin d'une pochette PNG ou JPEG",
        &dev.cover_path,
        |path| dev_message(DevMessage::CoverPathChanged(path)),
        Some(dev_message(DevMessage::ApplyCoverPath)),
      ),
      tonal("Appliquer", Some(dev_message(DevMessage::ApplyCoverPath))),
    ]
    .spacing(8),
    files,
  ]
  .spacing(8);
  if let Some(error) = &app.last_theme_error {
    body = body.push(text(format!("Thème refusé : {error}")).size(13));
  }
  section("Thème", body)
}

fn language_section(app: &App) -> Element<'_, Message> {
  let buttons = LANGUAGES.map(|lang| {
    if app.settings.ui_lang == lang {
      filled(lang, None)
    } else {
      tonal(lang, Some(Message::SetLanguage(lang.to_string())))
    }
  });
  let [en, ru, uk] = buttons;
  section("Langue", row![en, ru, uk].spacing(8))
}

fn components_section(app: &App) -> Element<'_, Message> {
  let dialog = Dialog {
    title: "Dialogue de démonstration".to_string(),
    body: "Tab reste dans le dialogue, Échap ou un clic sur le voile le ferme.".to_string(),
    cancel: "Annuler".to_string(),
    confirm: Some("Confirmer".to_string()),
  };
  let confirmed = Message::ShowToast("Dialogue confirmé".to_string(), ToastKind::Success, None);
  let core_event = app
    .last_core_event
    .as_ref()
    .map_or_else(|| "aucun".to_string(), |event| format!("{event:?}"));
  section(
    "Composants et événements",
    column![
      row![
        tonal(
          "Dialogue",
          Some(Message::OpenDialog(dialog, Some(Box::new(confirmed))))
        ),
        tonal(
          "Notification",
          Some(Message::ShowToast(
            "Notification de démonstration".to_string(),
            ToastKind::Info,
            None
          ))
        ),
        tonal(
          "Publier AppState",
          Some(dev_message(DevMessage::PublishState))
        ),
      ]
      .spacing(8),
      text(format!("Dernier CoreEvent : {core_event}")).size(13),
      text(format!(
        "Notifications en attente : {}",
        app.toasts.queue.len()
      ))
      .size(13),
    ]
    .spacing(8),
  )
}

/// Vue de l'écran.
#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let Some(dev) = app.dev.as_ref() else {
    return text("--dev requis").into();
  };
  scrollable(
    column![
      playback_section(app, dev),
      search_section(dev),
      devices_section(dev),
      theme_section(app, dev),
      language_section(app),
      components_section(app),
    ]
    .spacing(12)
    .padding(16),
  )
  .width(Fill)
  .height(Fill)
  .into()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::I18n;
  use crate::screens::Screen;
  use crate::settings::NativeSettings;
  use crate::window::PersistedWindowState;
  use serde_json::json;

  fn app() -> App {
    App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
    .with_dev()
  }

  fn send(app: &mut App, message: DevMessage) -> Vec<Effect> {
    app.update(Message::Dev(message))
  }

  fn row_of(title: &str) -> TrackRow {
    TrackRow {
      title: title.to_string(),
      artist: "A".to_string(),
      thumbnail: "https://x/y.jpg".to_string(),
    }
  }

  #[test]
  fn dev_mode_opens_on_the_dev_screen_and_lists_devices_and_themes() {
    let mut app = app();
    assert_eq!(app.screen, Screen::Dev);
    let effects = app.update(Message::WindowOpened {
      position: None,
      size: iced::Size::new(1280.0, 800.0),
    });
    assert!(effects.contains(&Effect::ListDevices));
    assert!(effects.contains(&Effect::ListThemes));
  }

  #[test]
  fn the_dev_screen_is_not_remembered() {
    let mut app = app();
    app.update(Message::Navigate(Screen::Queue));
    assert_eq!(app.settings.screen, Screen::Queue);
    app.update(Message::Navigate(Screen::Dev));
    assert_eq!(app.screen, Screen::Dev);
    assert_eq!(app.settings.screen, Screen::Queue);
  }

  #[test]
  fn without_dev_mode_the_messages_do_nothing() {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    );
    assert_eq!(app.screen, Screen::Home);
    assert_eq!(send(&mut app, DevMessage::Play), Vec::new());
  }

  #[test]
  fn loading_a_path_resets_the_playback() {
    let mut app = app();
    send(
      &mut app,
      DevMessage::PathChanged(" \"C:\\son.mp3\" ".to_string()),
    );
    assert_eq!(
      send(&mut app, DevMessage::LoadPath),
      vec![Effect::AudioLoad(PathBuf::from("C:\\son.mp3"))]
    );
  }

  #[test]
  fn an_empty_path_loads_nothing() {
    assert_eq!(send(&mut app(), DevMessage::LoadPath), Vec::new());
  }

  #[test]
  fn play_pause_and_volume_reach_the_engine() {
    let mut app = app();
    assert_eq!(send(&mut app, DevMessage::Play), vec![Effect::AudioPlay]);
    assert_eq!(send(&mut app, DevMessage::Pause), vec![Effect::AudioPause]);
    assert_eq!(
      send(&mut app, DevMessage::Volume(0.5)),
      vec![Effect::AudioVolume(0.5)]
    );
  }

  #[test]
  fn a_seek_is_sent_on_release_only() {
    let mut app = app();
    assert_eq!(send(&mut app, DevMessage::SeekPreview(12.0)), Vec::new());
    assert_eq!(
      send(&mut app, DevMessage::SeekRelease),
      vec![Effect::AudioSeek(Duration::from_secs(12))]
    );
    assert_eq!(app.playback.position, Duration::from_secs(12));
    assert_eq!(send(&mut app, DevMessage::SeekRelease), Vec::new());
  }

  #[test]
  fn searching_calls_the_core_and_shows_the_rows() {
    let mut app = app();
    send(&mut app, DevMessage::QueryChanged(" daft ".to_string()));
    assert_eq!(
      send(&mut app, DevMessage::Search),
      vec![Effect::DevSearch("daft".to_string(), SEARCH_LIMIT)]
    );
    assert!(app.dev.as_ref().unwrap().searching);
    send(&mut app, DevMessage::SearchDone(Ok(vec![row_of("Un")])));
    let dev = app.dev.as_ref().unwrap();
    assert!(!dev.searching);
    assert_eq!(dev.results, vec![row_of("Un")]);
  }

  #[test]
  fn a_failed_search_shows_a_toast() {
    let mut app = app();
    send(
      &mut app,
      DevMessage::SearchDone(Err("réseau coupé".to_string())),
    );
    assert_eq!(
      app.toasts.active.map(|toast| toast.kind),
      Some(ToastKind::Error)
    );
  }

  #[test]
  fn an_empty_query_does_not_search() {
    assert_eq!(send(&mut app(), DevMessage::Search), Vec::new());
  }

  #[test]
  fn a_right_click_on_a_row_opens_its_menu() {
    let mut app = app();
    send(&mut app, DevMessage::SearchDone(Ok(vec![row_of("Un")])));
    assert_eq!(
      send(&mut app, DevMessage::OpenRowMenu(0, Point::new(5.0, 6.0))),
      vec![Effect::FocusOverlay]
    );
    assert_eq!(app.menu.as_ref().map(|menu| menu.items.len()), Some(2));
    app.update(Message::Escape);
    assert_eq!(
      send(&mut app, DevMessage::OpenRowMenu(9, Point::ORIGIN)),
      Vec::new()
    );
    assert!(app.menu.is_none());
  }

  #[test]
  fn a_row_cover_is_fetched_through_the_core() {
    let mut app = app();
    send(&mut app, DevMessage::SearchDone(Ok(vec![row_of("Un")])));
    assert_eq!(
      send(&mut app, DevMessage::ApplyRowCover(0)),
      vec![Effect::FetchCover("https://x/y.jpg".to_string())]
    );
  }

  #[test]
  fn rows_are_read_from_the_search_reply() {
    let body = json!({ "tracks": [{ "title": "T", "artist": "A", "thumbnail": "U" }, { "title": "Sans image" }] });
    let rows = rows_from_search(&body);
    assert_eq!(
      rows[0],
      TrackRow {
        title: "T".into(),
        artist: "A".into(),
        thumbnail: "U".into()
      }
    );
    assert_eq!(rows[1].thumbnail, "");
    assert_eq!(rows_from_search(&json!({ "error": "x" })), Vec::new());
  }

  #[test]
  fn clock_formats_minutes_and_seconds() {
    assert_eq!(clock(Duration::from_secs(125)), "2:05");
  }
}
