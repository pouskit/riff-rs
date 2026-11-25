//! Vues des écrans de gestion : outils, imports, comptes, stockage.

use super::chrome::tip;
use super::common::chip;
use crate::app::{App, Message};
use crate::manage::{FolderKind, ImportKind, ImportMode, ManageMessage, Progress, human_size};
use crate::theme::icons;
use crate::ui::UiMessage;
use crate::widgets::button::{filled, icon, text_button, tonal};
use crate::widgets::dialog::Dialog;
use crate::widgets::focus::focusable;
use crate::widgets::text_field::{secret_field, text_field};
use iced::alignment::Vertical;
use iced::widget::{Column, Row, column, pick_list, progress_bar, row, space, text, toggler};
use iced::{Element, Fill};

const IMPORT_URL_ID: &str = "import-url";
const SPOTIFY_ID: &str = "spotify-client-id";
const LASTFM_KEY_ID: &str = "lastfm-key";
const LASTFM_SECRET_ID: &str = "lastfm-secret";
const PROXY_ID: &str = "proxy-url";

/// Choix d'une liste déroulante.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
  value: String,
  label: String,
}

impl std::fmt::Display for Choice {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(&self.label)
  }
}

fn send(message: ManageMessage) -> Message {
  Message::Manage(message)
}

fn tool_row<'a>(app: &App, name: &'static str, installed: bool) -> Element<'a, Message> {
  let status = &app.ui.manage.tools.status;
  let percent = status.progress_of(name, installed);
  let state = if installed {
    app.i18n.t("Installed")
  } else {
    format!("{percent} %")
  };
  column![
    row![text(name).size(14).width(Fill), text(state).size(12)].align_y(Vertical::Center),
    progress_bar(0.0..=100.0, percent as f32).girth(6),
  ]
  .spacing(4)
  .into()
}

/// Fenêtre d'installation de yt-dlp et ffmpeg.
#[must_use]
pub fn tools_window(app: &App) -> Element<'_, Message> {
  let tools = &app.ui.manage.tools;
  let status = &tools.status;
  let mut body = column![
    text(
      app
        .i18n
        .t("Riff needs yt-dlp and ffmpeg to play and save tracks (~400 MB).")
    )
    .size(13),
    tool_row(app, "yt-dlp", status.ytdlp),
    tool_row(app, "ffmpeg", status.ffmpeg),
  ]
  .spacing(12);
  if let Some(failure) = status.failure() {
    body = body.push(text(failure.to_string()).size(13).style(text::danger));
  }
  let label = if status.failure().is_some() {
    "Retry"
  } else {
    "Install"
  };
  let mut actions = row![text_button(
    app.i18n.t("Close"),
    Some(Message::Ui(UiMessage::ModalClose))
  )]
  .spacing(8);
  if !status.is_ready() {
    actions = actions.push(filled(
      app.i18n.t(label),
      (!tools.installing).then_some(send(ManageMessage::InstallTools)),
    ));
  }
  body.push(actions).into()
}

fn progress_line<'a>(app: &App, progress: &Progress) -> Option<Element<'a, Message>> {
  match progress {
    Progress::Idle => None,
    Progress::Running => Some(text(app.i18n.t("Please wait...")).size(13).into()),
    Progress::Done(message) => Some(text(message.clone()).size(13).into()),
    Progress::Failed(message) => Some(text(message.clone()).size(13).style(text::danger).into()),
  }
}

fn with_progress<'a>(
  app: &App,
  body: Column<'a, Message>,
  progress: &Progress,
) -> Element<'a, Message> {
  match progress_line(app, progress) {
    Some(line) => body.push(line).into(),
    None => body.into(),
  }
}

fn switch<'a>(
  label: String,
  value: bool,
  on_toggle: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
  let activate = on_toggle(!value);
  row![
    text(label).size(13).width(Fill),
    focusable(toggler(value).on_toggle(on_toggle)).on_activate(Some(activate))
  ]
  .align_y(Vertical::Center)
  .into()
}

/// Bloc « outils » de la page de réglages.
#[must_use]
pub fn tools_panel(app: &App) -> Element<'_, Message> {
  let status = &app.ui.manage.tools.status;
  let line = format!(
    "yt-dlp : {} / ffmpeg : {}",
    app
      .i18n
      .t(if status.ytdlp { "Installed" } else { "Missing" }),
    app.i18n.t(if status.ffmpeg {
      "Installed"
    } else {
      "Missing"
    })
  );
  row![
    text(line).size(13).width(Fill),
    tonal(app.i18n.t("Manage"), Some(send(ManageMessage::OpenTools)))
  ]
  .align_y(Vertical::Center)
  .into()
}

/// Fenêtre d'import d'une playlist.
#[must_use]
pub fn import_window(app: &App, kind: ImportKind) -> Element<'_, Message> {
  let import = &app.ui.manage.import;
  let running = import.progress == Progress::Running;
  let mut targets = vec![
    Choice {
      value: "__new__".to_string(),
      label: app.i18n.t("New playlist"),
    },
    Choice {
      value: "__favorites__".to_string(),
      label: app.i18n.t("Liked songs"),
    },
  ];
  targets.extend(app.data.playlists.iter().filter_map(|playlist| {
    Some(Choice {
      value: playlist.id.clone()?,
      label: playlist.name.clone().unwrap_or_default(),
    })
  }));
  let selected = targets
    .iter()
    .find(|choice| choice.value == import.target)
    .cloned();
  let link = column![
    text(app.i18n.t(kind.hint())).size(13),
    focusable(text_field(
      IMPORT_URL_ID,
      "https://",
      &import.url,
      |value| send(ManageMessage::ImportUrl(value)),
      Some(send(ManageMessage::ImportStart(kind)))
    )),
    text(app.i18n.t("Import into")).size(13),
    pick_list(targets, selected, |choice: Choice| send(
      ManageMessage::ImportTarget(choice.value)
    ))
    .text_size(13)
    .width(Fill),
  ]
  .spacing(10);
  let body = if kind.export_source().is_none() {
    link
  } else {
    let tab = |label: &str, mode: ImportMode| {
      let message = Some(send(ManageMessage::ImportMode(mode)));
      if import.mode == mode {
        filled(app.i18n.t(label), message)
      } else {
        tonal(app.i18n.t(label), message)
      }
    };
    let pane = match import.mode {
      ImportMode::Link => link,
      ImportMode::File => export_pane(app, kind),
    };
    column![
      row![
        tab("Playlist link", ImportMode::Link),
        tab("Export files", ImportMode::File)
      ]
      .spacing(6),
      pane
    ]
    .spacing(10)
  };
  let actions = row![
    text_button(
      app.i18n.t("Close"),
      Some(Message::Ui(UiMessage::ModalClose))
    ),
    filled(
      app.i18n.t(if running { "Importing..." } else { "Import" }),
      (!running).then_some(send(ManageMessage::ImportStart(kind)))
    ),
  ]
  .spacing(8);
  let body = match progress_line(app, &import.progress) {
    Some(line) => body.push(line),
    None => body,
  };
  body.push(actions).into()
}

/// Onglet « Export files » : choix des fichiers, cases et résumé.
fn export_pane(app: &App, kind: ImportKind) -> Column<'_, Message> {
  let import = &app.ui.manage.import;
  let names = if import.files.is_empty() {
    app.i18n.t("No files selected")
  } else {
    import.files.join(", ")
  };
  let mut pane = column![
    text(app.i18n.t(kind.file_hint())).size(13),
    row![
      tonal(
        app.i18n.t("Choose files"),
        Some(send(ManageMessage::ImportFilesPick(kind)))
      ),
      text(names).size(13)
    ]
    .spacing(10)
    .align_y(Vertical::Center),
  ]
  .spacing(10);
  // Riff n'affiche les cases d'Apple que si les fichiers ont les deux sortes de contenu.
  let both = !import.export.likes.is_empty() && !import.export.playlists.is_empty();
  if kind == ImportKind::Spotify || both {
    pane = pane
      .push(switch(app.i18n.t("Liked songs"), import.likes, |checked| {
        send(ManageMessage::ImportLikes(checked))
      }))
      .push(switch(
        app.i18n.t("Playlists"),
        import.playlists,
        |checked| send(ManageMessage::ImportPlaylists(checked)),
      ));
  }
  if kind == ImportKind::Apple
    && let Some(summary) = app.export_summary()
  {
    pane = pane.push(text(summary).size(13));
  }
  pane
}

/// Boutons d'import.
#[must_use]
pub fn imports_panel(app: &App) -> Element<'_, Message> {
  let button = |label: &str, kind: ImportKind| {
    tonal(
      app.i18n.t(label),
      Some(send(ManageMessage::OpenImport(kind))),
    )
  };
  row![
    button("Spotify", ImportKind::Spotify),
    button("Apple Music", ImportKind::Apple),
    button("Deezer, SoundCloud, YouTube", ImportKind::Link),
    button("Yandex Music", ImportKind::Yandex)
  ]
  .spacing(6)
  .wrap()
  .into()
}

/// Compte Spotify.
#[must_use]
pub fn spotify_panel(app: &App) -> Element<'_, Message> {
  let spotify = &app.ui.manage.spotify;
  let mut body = Column::new().spacing(8);
  if spotify.connected {
    body = body.push(
      row![
        text(app.i18n.t("Connected")).size(13).width(Fill),
        tonal(
          app.i18n.t("Import library"),
          (spotify.progress != Progress::Running)
            .then_some(send(ManageMessage::SpotifyImportLibrary))
        ),
        text_button(
          app.i18n.t("Log out"),
          Some(send(ManageMessage::SpotifyLogout))
        ),
      ]
      .spacing(8)
      .align_y(Vertical::Center),
    );
  } else {
    let id = &app.settings.spotify_client_id;
    body = body
      .push(text(app.i18n.t("Client ID of your Spotify application. Allowed address: http://127.0.0.1/api/spotify-callback")).size(12))
      .push(row![
        focusable(text_field(SPOTIFY_ID, "Client ID", id, |value| send(ManageMessage::SpotifyClientId(value)), None)),
        filled(app.i18n.t("Log in"), (!id.trim().is_empty() && spotify.progress != Progress::Running).then_some(send(ManageMessage::SpotifyConnect))),
      ].spacing(8).align_y(Vertical::Center));
  }
  with_progress(app, body, &spotify.progress)
}

/// Compte Last.fm.
#[must_use]
pub fn lastfm_panel(app: &App) -> Element<'_, Message> {
  let lastfm = &app.ui.manage.lastfm;
  let mut body = Column::new().spacing(8);
  if lastfm.connected {
    body = body
      .push(
        row![
          text(format!(
            "{} {}",
            app.i18n.t("Connected account"),
            app.settings.lastfm_user
          ))
          .size(13)
          .width(Fill),
          text_button(
            app.i18n.t("Log out"),
            Some(send(ManageMessage::LastfmLogout))
          ),
        ]
        .align_y(Vertical::Center),
      )
      .push(switch(
        app.i18n.t("Scrobble what I listen to"),
        app.settings.lastfm_enabled,
        |value| send(ManageMessage::LastfmToggle(value)),
      ));
  } else {
    body = body
      .push(
        text(
          app
            .i18n
            .t("Your Last.fm API key and secret. The secret goes in the system vault."),
        )
        .size(12),
      )
      .push(focusable(text_field(
        LASTFM_KEY_ID,
        "API key",
        &lastfm.api_key,
        |value| send(ManageMessage::LastfmKey(value)),
        None,
      )))
      .push(focusable(secret_field(
        LASTFM_SECRET_ID,
        "API secret",
        &lastfm.api_secret,
        |value| send(ManageMessage::LastfmSecret(value)),
      )))
      .push(filled(
        app.i18n.t("Connect in browser"),
        (lastfm.progress != Progress::Running).then_some(send(ManageMessage::LastfmConnect)),
      ));
  }
  with_progress(app, body, &lastfm.progress)
}

/// Compte Google : désactivé sans identifiant client.
#[must_use]
pub fn google_panel(app: &App) -> Element<'_, Message> {
  let google = &app.ui.manage.google;
  if !google.enabled {
    return text(
      app
        .i18n
        .t("Not available: this build has no Google client ID."),
    )
    .size(13)
    .into();
  }
  let mut body = Column::new().spacing(8);
  if google.connected {
    body = body
      .push(switch(
        app.i18n.t("Import as YouTube Music tracks"),
        google.ytmusic,
        |value| send(ManageMessage::GoogleMusic(value)),
      ))
      .push(
        row![
          tonal(
            app.i18n.t("Import library"),
            (google.progress != Progress::Running)
              .then_some(send(ManageMessage::GoogleImportLibrary))
          ),
          text_button(
            app.i18n.t("Log out"),
            Some(send(ManageMessage::GoogleLogout))
          ),
        ]
        .spacing(8),
      );
  } else {
    body = body.push(filled(
      app.i18n.t("Log in"),
      (google.progress != Progress::Running).then_some(send(ManageMessage::GoogleConnect)),
    ));
  }
  with_progress(app, body, &google.progress)
}

/// Connexion YouTube de lecture.
#[must_use]
pub fn youtube_panel(app: &App) -> Element<'_, Message> {
  let youtube = &app.ui.manage.youtube;
  let mut body = Column::new().spacing(8);
  if youtube.signed_in {
    body = body.push(
      row![
        text(app.i18n.t("Signed in")).size(13).width(Fill),
        text_button(
          app.i18n.t("Log out"),
          Some(send(ManageMessage::CookiesDelete))
        ),
      ]
      .align_y(Vertical::Center),
    );
  } else {
    let mut actions = Row::new().spacing(6).push(tonal(
      app.i18n.t("Import cookies.txt"),
      Some(send(ManageMessage::CookiesPick)),
    ));
    for browser in riff_core::services::ytdlp::cookie_browsers() {
      actions = actions.push(tonal(
        format!("{} {browser}", app.i18n.t("Read from")),
        (youtube.progress != Progress::Running)
          .then_some(send(ManageMessage::CookiesBrowser((*browser).to_string()))),
      ));
    }
    body = body.push(actions.wrap());
  }
  with_progress(app, body, &youtube.progress)
}

/// Dossiers de données et de téléchargement.
#[must_use]
pub fn folders_panel(app: &App) -> Element<'_, Message> {
  let storage = &app.ui.manage.storage;
  let data = |move_data: bool| send(ManageMessage::ChangeFolder(FolderKind::Data { move_data }));
  let mut body = column![
    text(storage.data_dir.clone()).size(13),
    row![
      tonal(app.i18n.t("Change and move my data"), Some(data(true))),
      text_button(app.i18n.t("Change without moving"), Some(data(false)))
    ]
    .spacing(8)
    .wrap(),
    text(app.i18n.t("Download folder")).size(14),
    text(storage.download_dir.clone()).size(13),
    row![
      tonal(
        app.i18n.t("Change"),
        Some(send(ManageMessage::ChangeFolder(FolderKind::Download)))
      ),
      text_button(
        app.i18n.t("Reset"),
        Some(send(ManageMessage::ResetDownloadFolder))
      ),
    ]
    .spacing(8),
  ]
  .spacing(8);
  if storage.restart_needed {
    body = body.push(text(app.i18n.t("Restart Riff to use the new folder.")).size(13));
  }
  with_progress(app, body, &storage.progress)
}

/// Taille du cache et vidage.
#[must_use]
pub fn cache_panel(app: &App) -> Element<'_, Message> {
  let size = app
    .ui
    .manage
    .storage
    .cache_bytes
    .map_or_else(String::new, human_size);
  let clear = Message::OpenDialog(
    Dialog {
      title: app.i18n.t("Clear cache"),
      body: app
        .i18n
        .t("Cached audio and thumbnails will be downloaded again when needed."),
      cancel: app.i18n.t("Cancel"),
      confirm: Some(app.i18n.t("Clear")),
    },
    Some(Box::new(send(ManageMessage::ClearCache))),
  );
  row![
    text(size).size(13).width(Fill),
    tonal(app.i18n.t("Clear"), Some(clear)),
  ]
  .spacing(8)
  .align_y(Vertical::Center)
  .into()
}

/// Export et import de playlists.
#[must_use]
pub fn backups_panel(app: &App) -> Element<'_, Message> {
  row![
    tonal(
      app.i18n.t("Export all playlists (JSON)"),
      Some(send(ManageMessage::ExportPlaylists))
    ),
    tonal(
      app.i18n.t("Import playlists (JSON)"),
      Some(send(ManageMessage::ImportJsonPick))
    ),
  ]
  .spacing(8)
  .wrap()
  .into()
}

/// Export et import du compte.
#[must_use]
pub fn account_panel(app: &App) -> Element<'_, Message> {
  let import = Message::OpenDialog(
    Dialog {
      title: app.i18n.t("Import Riff account"),
      body: app
        .i18n
        .t("Importing an account will replace your current data."),
      cancel: app.i18n.t("Cancel"),
      confirm: Some(app.i18n.t("Import")),
    },
    Some(Box::new(send(ManageMessage::ImportAccount))),
  );
  row![
    tonal(
      app.i18n.t("Export"),
      Some(send(ManageMessage::ExportAccount))
    ),
    tonal(app.i18n.t("Import"), Some(import))
  ]
  .spacing(8)
  .into()
}

/// Proxy de l'application.
#[must_use]
pub fn proxy_panel(app: &App) -> Element<'_, Message> {
  let storage = &app.ui.manage.storage;
  let settings = &app.settings;
  let mut body = column![
    switch(app.i18n.t("Use a proxy"), settings.proxy_enabled, |value| {
      send(ManageMessage::ProxyToggle(value))
    }),
    row![
      focusable(text_field(
        PROXY_ID,
        "socks5://127.0.0.1:1080",
        &storage.proxy_input,
        |value| send(ManageMessage::ProxyInput(value)),
        Some(send(ManageMessage::ProxyAdd))
      )),
      tonal(app.i18n.t("Add"), Some(send(ManageMessage::ProxyAdd))),
      text_button(
        app.i18n.t("Test"),
        settings
          .proxy_active
          .is_some()
          .then_some(send(ManageMessage::ProxyTest))
      ),
    ]
    .spacing(8)
    .align_y(Vertical::Center),
  ]
  .spacing(8);
  for proxy in &settings.proxy_list {
    let selected = settings.proxy_active.as_deref() == Some(proxy.as_str());
    body = body.push(
      row![
        chip(
          proxy.clone(),
          selected,
          send(ManageMessage::ProxySelect(proxy.clone()))
        ),
        space().width(Fill),
        tip(
          icon(
            icons::CLOSE,
            Some(send(ManageMessage::ProxyRemove(proxy.clone()))),
            28,
            28
          ),
          app.i18n.t("Remove")
        ),
      ]
      .align_y(Vertical::Center),
    );
  }
  with_progress(app, body, &storage.proxy_test)
}

/// Mise à jour automatique de yt-dlp.
#[must_use]
pub fn yt_dlp_panel(app: &App) -> Element<'_, Message> {
  switch(
    app.i18n.t("Enabled"),
    app.ui.manage.storage.yt_dlp_auto_update,
    |value| send(ManageMessage::YtDlpAutoUpdate(value)),
  )
}

/// Plusieurs instances (`multiInstance` de `startup.json`), appliqué au prochain démarrage.
#[must_use]
pub fn multi_instance_panel(app: &App) -> Element<'_, Message> {
  switch(
    app.i18n.t("Enabled"),
    app.ui.manage.storage.multi_instance,
    |value| send(ManageMessage::MultiInstance(value)),
  )
}

/// Diagnostic et journaux.
#[must_use]
pub fn diagnostics_panel(app: &App) -> Element<'_, Message> {
  row![
    tonal(
      app.i18n.t("Copy diagnostics"),
      Some(send(ManageMessage::CopyDiagnostics))
    ),
    text_button(app.i18n.t("Open logs"), Some(send(ManageMessage::OpenLogs))),
  ]
  .spacing(8)
  .into()
}
