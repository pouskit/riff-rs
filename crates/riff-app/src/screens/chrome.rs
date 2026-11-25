//! Autour des écrans : barre du haut, historique de recherche, barre latérale, barre de lecture, fenêtres.

use super::Screen;
use super::common::{cover, placeholder_style, plain_column, playlist_cover_url};
pub use super::jam_panel::jam_panel;
pub use super::queue_panel::{queue_panel, queue_panel_cover_urls, queue_panel_empty};
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_ROW};
use crate::library::SIDEBAR_COLLAPSED_WIDTH;
use crate::lyrics::update::LyricsMessage;
use crate::prefs::SettingsMessage;
use crate::prefs_visual::{ArtSlider, MediaTarget, VisualMessage};
use crate::theme::{icons, tokens};
use crate::ui::{Modal, SidebarTab, UiMessage};
use crate::user_data::Track;
use crate::widgets::OVERLAY_ID;
use crate::widgets::button::{filled, icon, text_button, text_style, tonal_style};
use crate::widgets::focus::focusable;
use crate::widgets::text_field::text_field;
use crate::widgets::view_transition::view_transition;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
  Column, button, column, container, image, mouse_area, opaque, row, rule, scrollable, slider,
  space, stack, text, toggler, tooltip,
};
use iced::{Color, Element, Fill, Length, Theme, border, mouse};

/// Champ de recherche : Ctrl+K lui donne le focus.
pub const SEARCH_ID: &str = "riff-search";
pub(super) const SIDEBAR_EDGE_WIDTH: f32 = 6.0;
const SIDEBAR_EDGE_ACTIVE: f32 = 0.75;
const RAIL_ENTRY: f32 = 48.0;
/// Taille des SVG de la barre latérale (`nav-*`, `index.html:425-516`), en pixels CSS.
const RAIL_ICON_PX: f32 = 20.0;
/// Hauteur de la barre de lecture : `.playbar` de Riff, 96 px CSS.
pub const PLAYER_HEIGHT: f32 = tokens::playbar::HEIGHT;
const FEED_COVER: f32 = 36.0;
const MODAL_WIDTH: f32 = 420.0;
const DOWNLOAD_WIDTH: f32 = 560.0;
const ART_WIDTH: f32 = 460.0;
const SCRIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.5);

fn ui(message: UiMessage) -> Message {
  Message::Ui(message)
}

/// Boutons mini-paroles et mini lecteur (`btn-mini-lyrics`, `btn-mini-player`) : glyphe, libellé, message.
pub fn mini_button_specs() -> [(&'static str, &'static str, Message); 2] {
  [
    (
      icons::MINI_LYRICS,
      "Lyrics",
      Message::Lyrics(LyricsMessage::ToggleMini),
    ),
    (
      icons::MINI_PLAYER,
      "Compact mini player",
      Message::ToggleMiniMode,
    ),
  ]
}

/// Infobulle sous un bouton.
pub fn tip<'a>(content: Element<'a, Message>, label: String) -> Element<'a, Message> {
  tip_at(content, label, tooltip::Position::Bottom)
}

/// Infobulle du côté donné : au-dessus pour la barre de lecture, collée au bas de la fenêtre.
pub fn tip_at<'a>(
  content: Element<'a, Message>,
  label: String,
  position: tooltip::Position,
) -> Element<'a, Message> {
  tooltip(
    content,
    container(text(label).size(12))
      .padding([4, 8])
      .style(card_style),
    position,
  )
  .into()
}

/// Clés i18n des infobulles des boutons à icône seule : celles qui suivent l'état du bouton prennent cet état.
pub mod tips {
  pub const fn play(playing: bool) -> &'static str {
    if playing { "Pause" } else { "Play" }
  }

  pub const fn shuffle(on: bool) -> &'static str {
    if on {
      "Turn off shuffle"
    } else {
      "Turn on shuffle"
    }
  }

  pub const fn repeat(on: bool) -> &'static str {
    if on {
      "Turn off repeat"
    } else {
      "Turn on repeat"
    }
  }

  pub const fn mute(muted: bool) -> &'static str {
    if muted { "Unmute" } else { "Mute" }
  }

  pub const fn favorite(favorite: bool) -> &'static str {
    if favorite {
      "Remove from Favorites"
    } else {
      "Add to favorites"
    }
  }

  pub const fn dislike(blocked: bool) -> &'static str {
    if blocked { "Remove dislike" } else { "Dislike" }
  }

  pub const fn save_album(saved: bool) -> &'static str {
    if saved { "Remove album" } else { "Save album" }
  }

  pub const fn maximize(maximized: bool) -> &'static str {
    if maximized { "Restore" } else { "Maximize" }
  }

  pub const PREVIOUS: &str = "Previous";
  pub const NEXT: &str = "Next";
  pub const LYRICS: &str = "Lyrics";
  pub const AUDIO: &str = "Audio settings";
  pub const RENAME: &str = "Rename track";
  pub const DOWNLOAD: &str = "Download track";
  pub const SHARE: &str = "Share link";
  pub const BACK: &str = "Back";
  pub const CLOSE: &str = "Close";
  pub const MINIMIZE: &str = "Minimize";
  pub const REFRESH: &str = "Refresh";
  pub const REMOVE_FROM_QUEUE: &str = "Remove from queue";
  pub const DELETE_PLAYLIST: &str = "Delete playlist";
  pub const JAM: &str = "Jam: listen together";
  pub const JAM_LIVE: &str = "Jam: {count} participants";

  /// Toutes les clés, états compris.
  pub const ALL: &[&str] = &[
    "Play",
    "Pause",
    "Turn on shuffle",
    "Turn off shuffle",
    "Turn on repeat",
    "Turn off repeat",
    "Mute",
    "Unmute",
    "Add to favorites",
    "Remove from Favorites",
    "Dislike",
    "Remove dislike",
    "Maximize",
    "Restore",
    "Save album",
    "Remove album",
    PREVIOUS,
    NEXT,
    LYRICS,
    AUDIO,
    RENAME,
    DOWNLOAD,
    SHARE,
    BACK,
    CLOSE,
    MINIMIZE,
    REFRESH,
    REMOVE_FROM_QUEUE,
    DELETE_PLAYLIST,
    JAM,
    JAM_LIVE,
  ];
}

fn card_style(theme: &Theme) -> container::Style {
  let palette = theme.extended_palette();
  container::Style::default()
    .background(palette.background.neutral.color)
    .color(palette.background.base.text)
    .border(border::rounded(12))
}

fn counted_label(app: &App, screen: Screen) -> String {
  let title = if screen == Screen::Liked {
    app.liked_title()
  } else {
    app.i18n.t(screen.title_key())
  };
  let count = match screen {
    Screen::Liked => app.data.favorites.len(),
    Screen::Albums => app.data.saved_albums.len(),
    Screen::Library => app.data.local_files.len(),
    Screen::History => app.data.history.len(),
    Screen::Queue => app.player.queue.len(),
    _ => 0,
  };
  if count > 0 {
    format!("{title}  {count}")
  } else {
    title
  }
}

fn feed_line<'a>(
  app: &App,
  url: Option<String>,
  title: String,
  sub: String,
) -> iced::widget::Row<'a, Message> {
  row![
    cover(app, url.as_deref(), FEED_COVER, SIZE_ROW, false),
    column![
      text(title).size(13).wrapping(text::Wrapping::None),
      text(sub).size(11).wrapping(text::Wrapping::None)
    ]
    .width(Fill)
    .clip(true),
  ]
  .spacing(8)
  .align_y(Vertical::Center)
}

fn feed_row<'a>(
  app: &App,
  url: Option<String>,
  title: String,
  sub: String,
  active: bool,
  message: Message,
) -> iced::widget::Button<'a, Message> {
  let line = feed_line(app, url, title, sub);
  button(line)
    .padding([4, 8])
    .width(Fill)
    .style(if active { tonal_style } else { text_style })
    .on_press(message)
}

/// URL des pochettes de la barre latérale.
pub fn feed_cover_urls(app: &App, out: &mut Vec<String>) {
  match app.ui.sidebar_tab {
    SidebarTab::Playlists => {
      out.extend(app.data.liked_custom_cover().map(str::to_string));
      out.extend(
        app
          .data
          .sidebar_pinned_tracks
          .iter()
          .filter_map(|track| app.cover_of(track)),
      );
      out.extend(app.data.playlists.iter().filter_map(playlist_cover_url));
    }
    SidebarTab::Albums => out.extend(
      app
        .data
        .saved_albums
        .iter()
        .filter_map(|album| album.cover.clone()),
    ),
    SidebarTab::Queue => {
      out.extend(
        app
          .player
          .current
          .iter()
          .filter_map(|track| app.cover_of(track)),
      );
      out.extend(
        app
          .queue_panel_upcoming()
          .into_iter()
          .filter_map(|(_, track)| app.cover_of(track)),
      );
      out.extend(
        app
          .queue_panel_recent()
          .iter()
          .filter_map(|track| app.cover_of(track)),
      );
    }
  }
}
/// Bouton de la barre de lecture : libellé de `btn-toggle-queue-panel` et message envoyé.
pub(super) fn queue_button_spec() -> (&'static str, Message) {
  ("Now playing and queue", ui(UiMessage::ToggleQueuePanel))
}

/// Bord de la barre latérale : glisser pour redimensionner, double clic pour replier ou déplier.
pub fn sidebar_edge(app: &App) -> Element<'_, Message> {
  let opacity = if app.ui.sidebar_drag.is_some() || app.ui.sidebar_edge_hovered {
    SIDEBAR_EDGE_ACTIVE
  } else {
    0.0
  };
  let line = container(space())
    .width(2)
    .height(Fill)
    .style(move |theme: &Theme| {
      container::Style::default().background(Color {
        a: opacity,
        ..theme.palette().primary
      })
    });
  mouse_area(container(line).center_x(SIDEBAR_EDGE_WIDTH).height(Fill))
    .on_press(ui(UiMessage::SidebarDragStart))
    .on_double_click(ui(UiMessage::SidebarToggle))
    .on_enter(ui(UiMessage::SidebarEdgeHover(true)))
    .on_exit(ui(UiMessage::SidebarEdgeHover(false)))
    .interaction(mouse::Interaction::ResizingHorizontally)
    .into()
}

fn rail_glyph(screen: Screen) -> &'static str {
  match screen {
    Screen::Home => icons::HOME,
    Screen::Liked => icons::FAVORITE,
    Screen::Albums => icons::ALBUM,
    Screen::Library => icons::FOLDER,
    Screen::History => icons::HISTORY,
    Screen::Queue => icons::NAV_QUEUE,
    Screen::Playlists => icons::QUEUE_MUSIC,
    Screen::Settings => icons::SETTINGS,
    _ => "•",
  }
}

fn rail_entry(app: &App, id: String, screen: Screen) -> Element<'_, Message> {
  let message = Message::Navigate(screen);
  let glyph = rail_glyph(screen);
  let px = if screen == Screen::Home {
    icons::css_size(glyph).unwrap_or(RAIL_ICON_PX)
  } else {
    RAIL_ICON_PX
  };
  let entry = button(container(icons::glyph_px(glyph, px)).center(Fill))
    .padding(0)
    .width(RAIL_ENTRY)
    .height(RAIL_ENTRY)
    .style(if screen == app.screen {
      tonal_style
    } else {
      text_style
    })
    .on_press(message.clone());
  let label = container(text(counted_label(app, screen)).size(12))
    .padding([4, 8])
    .style(card_style);
  tooltip(
    focusable(entry)
      .on_activate(Some(message))
      .on_context(move |position| Message::Ui(UiMessage::SidebarMenu(id.clone(), position)))
      .radius(RAIL_ENTRY / 2.0),
    label,
    tooltip::Position::Right,
  )
  .into()
}

/// `btn-sb-presets` : ouvre la fenêtre audio, où se trouvent les préréglages (pas de tiroir).
pub fn presets_message() -> Message {
  Message::AudioPrefs(crate::audio_prefs::AudioMessage::Open)
}

fn presets_button(app: &App) -> Element<'_, Message> {
  tip(
    icon(icons::DISCOVER_TUNE, Some(presets_message()), 32, 32),
    app.i18n.t("Audio presets"),
  )
}

/// Barre repliée : icônes seules, comme `body:not(.sidebar-expanded)`.
fn rail(app: &App) -> Element<'_, Message> {
  let entries = app
    .sidebar_screens()
    .into_iter()
    .map(|(id, screen)| rail_entry(app, id, screen));
  let mut nav = Column::with_children(entries)
    .spacing(4)
    .align_x(Horizontal::Center)
    .width(Fill);
  nav = nav.push(rail_entry(app, "playlists".to_string(), Screen::Playlists));
  if app.dev.is_some() {
    nav = nav.push(rail_entry(app, "dev".to_string(), Screen::Dev));
  }
  let bottom = column![
    rule::horizontal(1),
    rail_entry(app, "settings".to_string(), Screen::Settings),
    presets_button(app),
    tip_at(
      icon(
        icons::PLAYLIST_ADD,
        Some(ui(UiMessage::NewPlaylist)),
        32,
        32
      ),
      app.i18n.t("New playlist"),
      tooltip::Position::Right,
    ),
  ]
  .spacing(6)
  .align_x(Horizontal::Center)
  .width(Fill);
  column![scrollable(nav).height(Fill), bottom]
    .spacing(6)
    .padding([8, 0])
    .into()
}

/// Barre latérale : repliée en rail d'icônes, dépliée dans `sidebar.rs`.
pub fn sidebar(app: &App) -> Element<'_, Message> {
  let width = tokens::css(app.data.sidebar_width as f32);
  if app.data.sidebar_width <= SIDEBAR_COLLAPSED_WIDTH {
    return container(rail(app))
      .width(width)
      .height(Fill)
      .style(|theme: &Theme| {
        container::Style::default().background(theme.extended_palette().background.weaker.color)
      })
      .into();
  }
  super::sidebar::expanded(app, width)
}

fn modal_frame<'a>(title: String, body: Element<'a, Message>) -> Element<'a, Message> {
  sized_modal(title, body, MODAL_WIDTH)
}

fn sized_modal<'a>(title: String, body: Element<'a, Message>, width: f32) -> Element<'a, Message> {
  let card = container(column![text(title).size(18), body].spacing(12))
    .id(OVERLAY_ID)
    .padding(20)
    .width(width)
    .style(card_style);
  let scrim = mouse_area(
    container(space())
      .width(Fill)
      .height(Fill)
      .style(|_: &Theme| container::Style::default().background(SCRIM)),
  )
  .on_press(ui(UiMessage::ModalClose));
  opaque(
    stack![scrim, container(opaque(card)).center(Fill)]
      .width(Fill)
      .height(Fill),
  )
}

/// Fenêtre ouverte.
pub fn modal(app: &App) -> Option<Element<'_, Message>> {
  let modal = app.ui.modal.as_ref()?;
  let cancel = || text_button(app.i18n.t("Cancel"), Some(ui(UiMessage::ModalClose)));
  Some(match modal {
    Modal::Download => sized_modal(
      app.i18n.t(if app.ui.download.batch.is_some() {
        "Download playlist"
      } else {
        "Download track"
      }),
      super::download::view(app),
      DOWNLOAD_WIDTH,
    ),
    Modal::Import(kind) => modal_frame(
      app.i18n.t(kind.title()),
      super::manage::import_window(app, *kind),
    ),
    Modal::TrackArt(track) => track_art_window(app, track),
    Modal::ImportReport(report) => sized_modal(
      app.i18n.t("Import report"),
      super::import::report_view(app, report),
      super::import::REPORT_WIDTH,
    ),
    Modal::Tools => modal_frame(
      app.i18n.t("Required tools"),
      super::manage::tools_window(app),
    ),
    Modal::Audio => super::audio_window::overlay(app),
    Modal::CreatePlaylist { name, .. } => modal_frame(
      app.i18n.t("Create playlist"),
      column![
        text_field(
          "modal-name",
          &app.i18n.t("Playlist name"),
          name,
          |value| ui(UiMessage::ModalName(value)),
          Some(ui(UiMessage::ModalConfirm))
        ),
        row![
          space().width(Fill),
          cancel(),
          filled(
            app.i18n.t("Create"),
            (!name.trim().is_empty()).then_some(ui(UiMessage::ModalConfirm))
          )
        ]
        .spacing(8),
      ]
      .spacing(12)
      .into(),
    ),
    Modal::AddToPlaylist { tracks } => {
      let subtitle = match tracks.as_slice() {
        [one] => format!("{} • {}", one.title_text(), one.artist_text()),
        many => format!("{} tracks selected", many.len()),
      };
      let card_color = app.theme.theme.extended_palette().background.neutral.color;
      let lists = app
        .data
        .playlists
        .iter()
        .enumerate()
        .filter_map(|(rank, playlist)| {
          let id = playlist.id.clone()?;
          let count = playlist.tracks.len();
          let choose = ui(UiMessage::ModalChoose(id));
          let entry = feed_row(
            app,
            playlist_cover_url(playlist),
            playlist.name.clone().unwrap_or_default(),
            format!("{count} tracks"),
            false,
            choose.clone(),
          );
          // `m3FadeUp 200ms backwards`, décalage `idx * 25ms` (`app.js:13543`).
          let entry: Element<'_, Message> = focusable(entry).on_activate(Some(choose)).into();
          Some(
            view_transition(
              entry,
              app.entry_motion.pick_pose(rank, app.now),
              Some(card_color),
            )
            .pop(0.0)
            .spill(tokens::fade_up::OFFSET)
            .into(),
          )
        });
      let items: Vec<Element<'_, Message>> = lists.collect();
      let empty = items.is_empty();
      let list: Element<'_, Message> = if empty {
        text(app.i18n.t("No playlists yet")).size(13).into()
      } else {
        scrollable(plain_column(items))
          .height(Length::Fixed(280.0))
          .into()
      };
      modal_frame(
        app.i18n.t("Add to playlist"),
        column![
          text(subtitle).size(13),
          list,
          row![
            space().width(Fill),
            cancel(),
            text_button(
              app.i18n.t("Create playlist"),
              Some(ui(UiMessage::NewPlaylist))
            )
          ]
          .spacing(8),
        ]
        .spacing(12)
        .into(),
      )
    }
    Modal::Rename { title, artist, .. } => modal_frame(
      app.i18n.t("Rename track"),
      column![
        text_field(
          "modal-title",
          &app.i18n.t("Track title"),
          title,
          |value| ui(UiMessage::ModalTitle(value)),
          Some(ui(UiMessage::ModalConfirm))
        ),
        text_field(
          "modal-artist",
          &app.i18n.t("Artist"),
          artist,
          |value| ui(UiMessage::ModalArtist(value)),
          Some(ui(UiMessage::ModalConfirm))
        ),
        row![
          space().width(Fill),
          cancel(),
          filled(app.i18n.t("Save"), Some(ui(UiMessage::ModalConfirm)))
        ]
        .spacing(8),
      ]
      .spacing(12)
      .into(),
    ),
    Modal::Biography { title, body } => modal_frame(
      title.clone(),
      column![
        scrollable(text(body.as_str()).size(14)).height(Length::Fixed(320.0)),
        row![
          space().width(Fill),
          text_button(app.i18n.t("Close"), Some(ui(UiMessage::ModalClose)))
        ],
      ]
      .spacing(12)
      .into(),
    ),
  })
}

/// Message d'un contrôle visuel.
fn visual(message: VisualMessage) -> Message {
  Message::Settings(SettingsMessage::Visual(message))
}

/// Image de prévisualisation : l'image chargée, sinon un aplat.
fn art_preview<'a>(app: &App, url: Option<&str>, width: f32, height: f32) -> Element<'a, Message> {
  match url.and_then(|url| app.images.get(&ImageKey::new(url, SIZE_ROW))) {
    Some(handle) => image(handle.clone())
      .width(width)
      .height(height)
      .content_fit(iced::ContentFit::Cover)
      .border_radius(8)
      .into(),
    None => container(space())
      .width(width)
      .height(height)
      .style(|theme: &Theme| placeholder_style(theme).border(border::rounded(8)))
      .into(),
  }
}

/// Ligne « Cover » ou « Banner » : aperçu, nom, état, choisir, réinitialiser.
fn art_row<'a>(
  app: &App,
  preview: Element<'a, Message>,
  name: &str,
  status: &str,
  target: MediaTarget,
) -> Element<'a, Message> {
  row![
    preview,
    column![
      text(app.i18n.t(name)).size(14),
      text(app.i18n.t(status)).size(12)
    ]
    .spacing(2)
    .width(Fill),
    text_button(
      app.i18n.t("Choose file"),
      Some(visual(VisualMessage::PickMedia(target)))
    ),
    text_button(
      app.i18n.t("Reset"),
      Some(visual(VisualMessage::ClearMedia(target)))
    ),
  ]
  .spacing(10)
  .align_y(Vertical::Center)
  .into()
}

/// Fenêtre « Customize cover & banner » (`track-art-modal`).
fn track_art_window<'a>(app: &'a App, track: &'a Track) -> Element<'a, Message> {
  let visual_settings = &app.settings.visual;
  let id = track.canonical_id(&app.data.platform);
  let entry = visual_settings.track_art.get(&id);
  let has = |key: &str| {
    entry
      .and_then(|entry| entry.get(key))
      .and_then(serde_json::Value::as_str)
      .is_some_and(|value| !value.is_empty())
  };
  let cover_url = app.cover_of(track);
  let own_banner = visual_settings.track_banner(&id);
  let global_banner =
    (!visual_settings.banner_media.is_empty()).then_some(visual_settings.banner_media.as_str());
  let banner_url = app.banner_preview(track);
  let banner_status = if own_banner.is_some() {
    "Custom banner"
  } else if global_banner.is_some() {
    "Global banner"
  } else {
    "Default art"
  };
  let fx = visual_settings.track_fx(&id);
  let mut effects = column![
    row![
      column![
        text(app.i18n.t("Custom banner effects")).size(14),
        text(app.i18n.t("Per-track blur, opacity and fade")).size(12),
      ]
      .spacing(2)
      .width(Fill),
      focusable(toggler(fx.enabled).on_toggle(|next| visual(VisualMessage::TrackArtFxOn(next))))
        .on_activate(Some(visual(VisualMessage::TrackArtFxOn(!fx.enabled)))),
    ]
    .align_y(Vertical::Center),
  ]
  .spacing(10);
  if fx.enabled {
    for key in ArtSlider::ALL {
      let value = key.value(fx);
      effects = effects.push(
        column![
          row![
            text(app.i18n.t(key.label())).size(13).width(Fill),
            text(key.format(value)).size(13)
          ],
          slider(key.range().0..=key.range().1, value, move |next| visual(
            VisualMessage::TrackArtFx(key, next)
          ))
          .step(key.step())
          .on_release(Message::Settings(SettingsMessage::CommitInt)),
        ]
        .spacing(4),
      );
    }
  }
  sized_modal(
    format!("{} {}", app.i18n.t("Art:"), track.title_text()),
    column![
      art_row(
        app,
        art_preview(app, cover_url.as_deref(), 48.0, 48.0),
        "Cover",
        if has("cover") {
          "Custom cover"
        } else {
          "Default art"
        },
        MediaTarget::TrackCover,
      ),
      art_row(
        app,
        art_preview(app, banner_url.as_deref(), 80.0, 48.0),
        "Banner",
        banner_status,
        MediaTarget::TrackBanner,
      ),
      effects,
      row![
        space().width(Fill),
        text_button(app.i18n.t("Close"), Some(ui(UiMessage::ModalClose)))
      ],
    ]
    .spacing(14)
    .into(),
    ART_WIDTH,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_title_bar_buttons_send_the_mini_lyrics_and_the_compact_mode() {
    let [lyrics, mini] = mini_button_specs();
    assert_eq!(lyrics.1, "Lyrics");
    assert!(matches!(
      lyrics.2,
      Message::Lyrics(LyricsMessage::ToggleMini)
    ));
    assert_eq!(mini.1, "Compact mini player");
    assert!(matches!(mini.2, Message::ToggleMiniMode));
  }

  #[test]
  fn the_presets_button_opens_the_audio_window() {
    let mut app = App::new(
      crate::window::PersistedWindowState::default(),
      crate::settings::NativeSettings::default(),
      crate::i18n::I18n::default(),
    );
    assert!(app.ui.modal.is_none());
    app.update(presets_message());
    assert_eq!(app.ui.modal, Some(Modal::Audio));
  }

  #[test]
  fn the_queue_button_has_the_tooltip_of_riff_and_toggles_the_panel() {
    let (label, message) = queue_button_spec();
    assert_eq!(label, "Now playing and queue");
    assert!(matches!(message, Message::Ui(UiMessage::ToggleQueuePanel)));
  }

  #[test]
  fn toggle_tooltips_follow_the_button_state() {
    assert_eq!(tips::play(false), "Play");
    assert_eq!(tips::play(true), "Pause");
    assert_ne!(tips::shuffle(true), tips::shuffle(false));
    assert_ne!(tips::repeat(true), tips::repeat(false));
    assert_eq!(tips::mute(true), "Unmute");
    assert_eq!(tips::favorite(false), "Add to favorites");
    assert_eq!(tips::favorite(true), "Remove from Favorites");
    assert_eq!(tips::dislike(false), "Dislike");
    assert_eq!(tips::dislike(true), "Remove dislike");
    assert_eq!(tips::maximize(true), "Restore");
  }

  #[test]
  fn every_icon_tooltip_key_is_a_ui_key() {
    let inline = [
      "Edit",
      "Delete",
      "Remove",
      "New playlist",
      "Always on top",
      "Toggle cover",
      "Unblock artist",
      "Unblock track",
    ];
    for key in tips::ALL.iter().chain(&inline) {
      assert!(
        crate::i18n::UI_KEYS.contains(key),
        "{key} absent de UI_KEYS"
      );
    }
  }

  #[test]
  fn the_new_tooltip_keys_are_translated_in_ru_and_uk() {
    let keys = [
      "Pause",
      "Turn on shuffle",
      "Turn off shuffle",
      "Turn on repeat",
      "Turn off repeat",
      "Mute",
      "Unmute",
      "Remove from Favorites",
      "Remove dislike",
      "Previous",
      "Next",
      "Back",
      "Maximize",
      "Restore",
      "Minimize",
      "Save album",
      "Remove album",
      "Remove from queue",
      "Share link",
      "Unblock artist",
      "Unblock track",
      "Always on top",
      "Toggle cover",
    ];
    for lang in ["ru", "uk"] {
      let i18n = crate::i18n::I18n::load(lang, &std::env::temp_dir().join("riff-app-tips-absent"));
      for key in keys {
        assert_ne!(i18n.t(key), key, "{key} non traduit en {lang}");
      }
    }
  }
}
