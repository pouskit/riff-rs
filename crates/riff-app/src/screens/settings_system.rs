//! Catégories Stockage, Avancé, À propos, Thèmes personnalisés, Plugins et Compte de `#view-settings` (Riff 44.1.1).
//! Le cadre, la navigation et les rangées communes sont dans `settings.rs` ; ici : rangées à icône, boutons en pastille,
//! en-têtes (`.ct-head`, héros À propos, héros du compte), listes de cartes et section Avancé repliable.

use super::settings::{
  NICKNAME_ID, Palette, VERSION_TAG, dropdown, options_of, row_radius, send, switch, with_alpha,
};
use crate::app::{App, Message};
use crate::code_editor::{EditorMessage, EditorTarget};
use crate::manage::{FolderKind, ManageMessage, Progress};
use crate::plugins::PluginMessage;
use crate::prefs::{Category, Control, Entry, Panel, SettingsMessage, TextKey};
use crate::prefs_visual::VisualMessage;
use crate::system::launcher;
use crate::system::update_install::{self, InstallState};
use crate::system::updates::{Outcome, UpdateState};
use crate::theme::tokens::settings::system as t;
use crate::theme::{fonts, icons, tokens};
use crate::widgets::dialog::Dialog;
use crate::widgets::focus::focusable;
use iced::alignment::{Horizontal, Vertical};
use iced::gradient::Linear;
use iced::widget::{
  Column, Row, button, column, container, mouse_area, responsive, row, space, stack, svg, text,
  text_input,
};
use iced::{
  Background, Border, Color, Element, Fill, Font, Padding, Radians, Shrink, Theme, border,
};
use riff_plugins::{ConsentState, PluginEntry, PluginKind};

const PROXY_ID: &str = "settings-proxy";
const SVG_CACHE: &[u8] = include_bytes!("../../assets/icons/set-cache.svg");
const SVG_TOOL: &[u8] = include_bytes!("../../assets/icons/set-tool.svg");
const SVG_IMPORT: &[u8] = include_bytes!("../../assets/icons/set-import.svg");
const SVG_EXPORT: &[u8] = include_bytes!("../../assets/icons/set-export.svg");
const SVG_FOLDER: &[u8] = include_bytes!("../../assets/icons/set-folder.svg");
const SVG_TRASH: &[u8] = include_bytes!("../../assets/icons/set-trash.svg");
const SVG_YOUTUBE_TILE: &[u8] = include_bytes!("../../assets/icons/set-yt-logo.svg");
const SVG_SPOTIFY: &[u8] = include_bytes!("../../assets/icons/set-spotify.svg");
const SVG_DISCORD: &[u8] = include_bytes!("../../assets/icons/acc-discord.svg");
const SVG_CHEVRON: &[u8] = include_bytes!("../../assets/icons/set-arrow.svg");
const SVG_RIFF: &[u8] = include_bytes!("../../assets/icons/riff-logo.svg");
const SATELLITE_SVG: [&[u8]; 7] = [
  include_bytes!("../../assets/icons/acc-soundcloud.svg"),
  include_bytes!("../../assets/icons/acc-wave.svg"),
  include_bytes!("../../assets/icons/acc-lastfm.svg"),
  include_bytes!("../../assets/icons/acc-discord.svg"),
  include_bytes!("../../assets/icons/acc-ytmusic.svg"),
  include_bytes!("../../assets/icons/acc-spotify.svg"),
  include_bytes!("../../assets/icons/acc-youtube.svg"),
];

fn rgb((r, g, b): t::Rgb8) -> Color {
  Color::from_rgb8(r, g, b)
}

/// Icône SVG de `css_px` pixels CSS ; `color` la teinte (les SVG en `currentColor`), sinon couleurs d'origine.
fn glyph<'a>(bytes: &'static [u8], css_px: f32, color: Option<Color>) -> Element<'a, Message> {
  let size = tokens::css(css_px);
  let widget = svg(svg::Handle::from_memory(bytes))
    .width(size)
    .height(size);
  match color {
    Some(color) => widget
      .opacity(color.a)
      .style(move |_: &Theme, _| svg::Style {
        color: Some(Color { a: 1.0, ..color }),
      })
      .into(),
    None => widget.into(),
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
  /// `.btn-pill-primary`.
  Primary,
  /// `.btn-outline-pill`.
  Outline,
}

/// Dimensions d'un bouton en pastille.
#[derive(Clone, Copy)]
struct Size {
  height: f32,
  padding_x: f32,
  text: f32,
  font: Font,
}

impl Size {
  /// Boutons des rangées du Stockage et de À propos : `padding` en ligne `6px 14px`, 12 px, graisse 500.
  const ROW: Self = Self {
    height: t::BTN_HEIGHT,
    padding_x: t::BTN_PADDING_X,
    text: t::BTN_TEXT,
    font: fonts::MEDIUM,
  };
  /// Boutons sans style en ligne : en-têtes `.ct-head`.
  const HEAD: Self = Self {
    height: t::BTN_HEIGHT,
    padding_x: t::HEAD_BTN_PADDING_X,
    text: t::HEAD_BTN_TEXT,
    font: fonts::MEDIUM,
  };
  /// « Reconnect » de « Discord RPC » : `padding: 4px 10px`, 12 px (`index.html:2052`), hauteur de `.btn-outline-pill`.
  const RECONNECT: Self = Self {
    height: t::BTN_HEIGHT,
    padding_x: tokens::css(10.0),
    text: t::BTN_TEXT,
    font: fonts::MEDIUM,
  };
  const SMALL: Self = Self {
    height: t::SMALL_BTN_HEIGHT,
    padding_x: t::BTN_PADDING_X,
    text: t::SMALL_BTN_TEXT,
    font: fonts::MEDIUM,
  };
  const SIGN: Self = Self {
    height: t::SMALL_BTN_HEIGHT,
    padding_x: t::SIGN_BTN_PADDING_X,
    text: t::SIGN_BTN_TEXT,
    font: fonts::MEDIUM,
  };
  /// `.acc-btn` du compte.
  const ACCOUNT: Self = Self {
    height: t::ACC_BTN_HEIGHT,
    padding_x: t::ACC_BTN_PADDING_X,
    text: t::ACC_BTN_TEXT,
    font: fonts::SEMIBOLD,
  };
}

/// Bouton en pastille, avec une icône facultative avant le libellé. Sans message : désactivé.
fn pill<'a>(
  p: Palette,
  label: String,
  kind: Kind,
  size: Size,
  lead: Option<&'static [u8]>,
  message: Option<Message>,
) -> Element<'a, Message> {
  let foreground = match kind {
    Kind::Primary => p.roles.on_primary,
    Kind::Outline => p.roles.variant,
  };
  let caption = text(label)
    .size(size.text)
    .font(size.font)
    .wrapping(text::Wrapping::None);
  let mut content = Row::new().spacing(t::ACC_BTN_GAP).align_y(Vertical::Center);
  if let Some(bytes) = lead {
    content = content.push(glyph(bytes, t::ACC_BTN_ICON, Some(foreground)));
  }
  let content = content.push(caption);
  let mut widget = button(container(content).height(Fill).align_y(Vertical::Center))
    .padding(Padding::from([0.0, size.padding_x]))
    .height(size.height)
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      let (background, color) = match (kind, hovered) {
        (Kind::Primary, false) => (p.roles.primary, p.roles.on_primary),
        (Kind::Primary, true) => (with_alpha(p.roles.primary, 0.9), p.roles.on_primary),
        (Kind::Outline, false) => (p.roles.high, p.roles.variant),
        (Kind::Outline, true) => (p.roles.highest, p.roles.on_surface),
      };
      let fade = if matches!(status, button::Status::Disabled) {
        0.4
      } else {
        1.0
      };
      button::Style {
        background: Some(Background::Color(with_alpha(
          background,
          background.a * fade,
        ))),
        text_color: with_alpha(color, fade),
        border: border::rounded(9999),
        ..button::Style::default()
      }
    });
  if let Some(message) = message.clone() {
    widget = widget.on_press(message);
  }
  focusable(widget)
    .on_activate(message)
    .radius(size.height / 2.0)
    .into()
}

/// Nom d'une rangée, 13 px graisse 500 (`.setting-name`).
fn name<'a>(p: Palette, label: String) -> Element<'a, Message> {
  text(label)
    .size(tokens::settings::NAME_TEXT)
    .font(fonts::MEDIUM)
    .color(p.roles.on_surface)
    .into()
}

/// Description : `.setting-hint` (78 %, interligne 1,35) ou texte en ligne de 12 px en `outline` plein.
fn hint<'a>(p: Palette, hinted: bool, label: String) -> Element<'a, Message> {
  let line = text(label).size(tokens::settings::HINT_TEXT);
  if hinted {
    line
      .line_height(tokens::settings::HINT_LINE)
      .color(with_alpha(p.roles.outline, tokens::settings::HINT_OPACITY))
      .into()
  } else {
    line.color(p.roles.outline).into()
  }
}

/// Nom et description de l'entrée.
fn name_column<'a>(app: &App, p: Palette, entry: &Entry, hinted: bool) -> Column<'a, Message> {
  let mut column = column![name(p, app.i18n.t(entry.label))].width(Fill);
  if hinted {
    column = column.spacing(tokens::css(2.0));
  }
  if !entry.hint.is_empty() {
    column = column.push(hint(p, hinted, app.i18n.t(entry.hint)));
  }
  column
}

/// Pastille d'icône : 32 px sur `surface-container-high`, icône en `primary`.
fn icon_disc<'a>(p: Palette, bytes: &'static [u8]) -> Element<'a, Message> {
  container(glyph(bytes, t::ICON_GLYPH, Some(p.roles.primary)))
    .center(t::ICON_CIRCLE)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(p.roles.high)
        .border(border::rounded(t::ICON_CIRCLE / 2.0))
    })
    .into()
}

/// Rangée à icône : pastille, nom, textes secondaires, boutons à droite.
fn icon_row<'a>(
  p: Palette,
  lead: Element<'a, Message>,
  texts: Column<'a, Message>,
  actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
  let mut line = row![lead, texts.width(Fill)]
    .spacing(t::ICON_GAP)
    .align_y(Vertical::Center);
  if !actions.is_empty() {
    line = line.push(Row::with_children(actions).spacing(t::HEAD_ACTIONS_GAP));
  }
  let _ = p;
  line.into()
}

fn toggle_row<'a>(
  app: &App,
  p: Palette,
  entry: &Entry,
  hinted: bool,
  on: bool,
  message: Message,
) -> Element<'a, Message> {
  row![name_column(app, p, entry, hinted), switch(p, on, message)]
    .spacing(tokens::settings::ROW_TOP_GAP)
    .align_y(Vertical::Center)
    .into()
}

/// Message de la boîte de confirmation `OpenDialog`.
fn confirm(app: &App, title: &str, body: &str, accept: &str, on_accept: Message) -> Message {
  Message::OpenDialog(
    Dialog {
      title: app.i18n.t(title),
      body: app.i18n.t(body),
      cancel: app.i18n.t("Cancel"),
      confirm: Some(app.i18n.t(accept)),
    },
    Some(Box::new(on_accept)),
  )
}

fn manage(message: ManageMessage) -> Message {
  Message::Manage(message)
}

fn progress_text(app: &App, progress: &Progress) -> Option<(String, bool)> {
  match progress {
    Progress::Idle => None,
    Progress::Running => Some((app.i18n.t("Please wait..."), false)),
    Progress::Done(message) => Some((message.clone(), false)),
    Progress::Failed(message) => Some((message.clone(), true)),
  }
}

// ---------------------------------------------------------------------------------------------
// Stockage
// ---------------------------------------------------------------------------------------------

fn tools_status(app: &App) -> String {
  let tools = &app.ui.manage.tools;
  let status = &tools.status;
  if status.is_ready() {
    app.i18n.t("Installed")
  } else if status.failure().is_some() {
    app.i18n.t("Download failed")
  } else if tools.installing {
    let percent = (status.progress_of("yt-dlp", status.ytdlp)
      + status.progress_of("ffmpeg", status.ffmpeg))
      / 2;
    format!("{} {percent}%", app.i18n.t("Downloading..."))
  } else {
    app.i18n.t("Missing")
  }
}

fn storage_row<'a>(
  app: &'a App,
  p: Palette,
  entry: &'static Entry,
) -> Option<Element<'a, Message>> {
  let storage = &app.ui.manage.storage;
  let hinted = false;
  let two_lines = |extra: Option<String>| {
    let mut column = name_column(app, p, entry, hinted);
    if let Some(line) = extra {
      column = column.push(hint(p, hinted, line));
    }
    column
  };
  let outline = |label: &str, lead: Option<&'static [u8]>, message: Message| {
    pill(
      p,
      app.i18n.t(label),
      Kind::Outline,
      Size::ROW,
      lead,
      Some(message),
    )
  };
  let row = match (entry.label, entry.control) {
    ("Clear cache", _) => {
      let clear = confirm(
        app,
        "Clear cache",
        "Cached audio and thumbnails will be downloaded again when needed.",
        "Clear",
        manage(ManageMessage::ClearCache),
      );
      let line = icon_row(
        p,
        icon_disc(p, SVG_CACHE),
        name_column(app, p, entry, hinted),
        Vec::new(),
      );
      return Some(mouse_area(line).on_press(clear).into());
    }
    ("Required tools", _) => {
      let ready = app.ui.manage.tools.status.is_ready();
      let installing = app.ui.manage.tools.installing;
      let actions = if ready {
        Vec::new()
      } else {
        vec![pill(
          p,
          app.i18n.t("Download"),
          Kind::Primary,
          Size::ROW,
          None,
          (!installing).then_some(manage(ManageMessage::InstallTools)),
        )]
      };
      icon_row(
        p,
        icon_disc(p, SVG_TOOL),
        two_lines(Some(tools_status(app))),
        actions,
      )
    }
    ("Music folder", _) => icon_row(
      p,
      icon_disc(p, SVG_FOLDER),
      two_lines(Some(storage.data_dir.clone())),
      // « Change without moving » n'existe pas dans Riff : fonction de riff-rs gardée à côté de « Change ».
      vec![
        outline(
          "Change without moving",
          None,
          manage(ManageMessage::ChangeFolder(FolderKind::Data {
            move_data: false,
          })),
        ),
        outline(
          "Change",
          None,
          manage(ManageMessage::ChangeFolder(FolderKind::Data {
            move_data: true,
          })),
        ),
      ],
    ),
    ("Download folder", _) => icon_row(
      p,
      icon_disc(p, SVG_IMPORT),
      two_lines(Some(storage.download_dir.clone())),
      vec![
        outline("Reset", None, manage(ManageMessage::ResetDownloadFolder)),
        outline(
          "Change",
          None,
          manage(ManageMessage::ChangeFolder(FolderKind::Download)),
        ),
      ],
    ),
    ("Export all playlists (JSON)", _) => icon_row(
      p,
      icon_disc(p, SVG_EXPORT),
      two_lines(None),
      vec![outline(
        "Export",
        None,
        manage(ManageMessage::ExportPlaylists),
      )],
    ),
    ("Import playlists (JSON)", _) => icon_row(
      p,
      icon_disc(p, SVG_IMPORT),
      two_lines(None),
      vec![outline(
        "Import",
        None,
        manage(ManageMessage::ImportJsonPick),
      )],
    ),
    ("Export Riff account (.rfacc)", _) => icon_row(
      p,
      icon_disc(p, SVG_EXPORT),
      two_lines(None),
      vec![outline(
        "Export",
        None,
        manage(ManageMessage::ExportAccount),
      )],
    ),
    ("Import Riff account (.rfacc)", _) => icon_row(
      p,
      icon_disc(p, SVG_IMPORT),
      two_lines(None),
      vec![outline("Import", None, import_account(app))],
    ),
    _ => return None,
  };
  Some(row)
}

/// Remplacement du compte : confirmation avant de lire le fichier.
fn import_account(app: &App) -> Message {
  confirm(
    app,
    "Import Riff account",
    "Importing an account will replace your current data.",
    "Import",
    manage(ManageMessage::ImportAccount),
  )
}

/// Ligne d'état sous la liste du Stockage : dernier export, import ou changement de dossier.
fn storage_tail<'a>(app: &App, p: Palette) -> Option<Element<'a, Message>> {
  let storage = &app.ui.manage.storage;
  let mut lines = Column::new().spacing(tokens::css(4.0));
  let mut any = false;
  if storage.restart_needed {
    any = true;
    lines = lines.push(hint(
      p,
      false,
      app.i18n.t("Restart Riff to use the new folder."),
    ));
  }
  if let Some((line, failed)) = progress_text(app, &storage.progress) {
    any = true;
    let color = if failed {
      error_color(app)
    } else {
      p.roles.variant
    };
    lines = lines.push(text(line).size(tokens::settings::HINT_TEXT).color(color));
  }
  any.then(|| {
    container(lines)
      .padding(Padding::ZERO.left(tokens::css(14.0)).top(tokens::css(8.0)))
      .into()
  })
}

fn error_color(app: &App) -> Color {
  let error = app.theme.scheme.error;
  Color::from_rgb8(error.red, error.green, error.blue)
}

// ---------------------------------------------------------------------------------------------
// Avancé
// ---------------------------------------------------------------------------------------------

/// `#btn-toggle-adv-section` : titre et chevron ; le chevron tourne quand la section est dépliée.
fn advanced_header<'a>(app: &App, p: Palette) -> Element<'a, Message> {
  let open = app.settings.adv_open;
  let size = tokens::css(18.0);
  let mut chevron = svg(svg::Handle::from_memory(SVG_CHEVRON))
    .width(size)
    .height(size)
    .style(move |_: &Theme, _| svg::Style {
      color: Some(p.roles.outline),
    });
  if open {
    chevron = chevron.rotation(iced::Rotation::Solid(Radians::PI));
  }
  let title = text(app.i18n.t("Advanced"))
    .size(t::SECTION_TITLE_TEXT)
    .font(fonts::MEDIUM)
    .color(with_alpha(p.roles.variant, t::SECTION_TITLE_OPACITY));
  let line = row![title, space().width(Fill), chevron].align_y(Vertical::Center);
  // `.adv-items-list` : `padding-top: 12px`, seulement quand la section est dépliée.
  let below = if open { tokens::css(12.0) } else { 0.0 };
  let widget = button(
    container(line)
      .padding(Padding::from([t::ADV_HEADER_PADDING_Y, 0.0]).bottom(below))
      .width(Fill),
  )
  .padding(0)
  .width(Fill)
  .on_press(send(SettingsMessage::ToggleAdvanced))
  .style(|_, _| button::Style::default());
  focusable(widget)
    .on_activate(Some(send(SettingsMessage::ToggleAdvanced)))
    .into()
}

fn proxy_item<'a>(app: &App, p: Palette, proxy: &str, selected: bool) -> Element<'a, Message> {
  let color = if selected {
    p.roles.on_primary
  } else {
    p.roles.on_surface
  };
  let remove = button(icons::glyph_px(icons::CLOSE, 14.0))
    .padding(tokens::css(4.0))
    .on_press(manage(ManageMessage::ProxyRemove(proxy.to_string())))
    .style(move |_, _| button::Style {
      text_color: color,
      ..button::Style::default()
    });
  let _ = app;
  let line = container(
    row![
      text(proxy.to_string())
        .size(tokens::settings::NAME_TEXT)
        .color(color)
        .width(Fill),
      remove
    ]
    .align_y(Vertical::Center)
    .spacing(tokens::css(10.0)),
  )
  .padding([t::PROXY_ITEM_PADDING_Y, t::PROXY_ITEM_PADDING_X])
  .width(Fill)
  .style(move |_: &Theme| {
    container::Style::default()
      .background(if selected {
        p.roles.primary
      } else {
        p.roles.container
      })
      .border(border::rounded(t::PROXY_ITEM_RADIUS))
  });
  mouse_area(line)
    .on_press(manage(ManageMessage::ProxySelect(proxy.to_string())))
    .into()
}

/// `#setting-proxy-manager` : interrupteur, puis champ, « + », « Test » et liste.
fn proxy_row<'a>(app: &'a App, p: Palette, entry: &'static Entry) -> Element<'a, Message> {
  let settings = &app.settings;
  let storage = &app.ui.manage.storage;
  let top = toggle_row(
    app,
    p,
    entry,
    true,
    settings.proxy_enabled,
    manage(ManageMessage::ProxyToggle(!settings.proxy_enabled)),
  );
  let input = container(focusable(
    text_input(
      "socks5://127.0.0.1:10808 or http://user:pass@ip:port",
      &storage.proxy_input,
    )
    .id(PROXY_ID)
    .on_input(|value| manage(ManageMessage::ProxyInput(value)))
    .on_submit(manage(ManageMessage::ProxyAdd))
    .size(t::PROXY_INPUT_TEXT)
    .padding(Padding::from([0.0, t::PROXY_INPUT_PADDING_X]))
    .style(move |_, _| text_input::Style {
      background: Background::Color(p.roles.container),
      border: border::rounded(t::PROXY_INPUT_RADIUS),
      icon: p.roles.variant,
      placeholder: with_alpha(p.roles.variant, 0.7),
      value: p.roles.on_surface,
      selection: with_alpha(p.roles.primary, 0.4),
    }),
  ))
  .height(t::PROXY_HEIGHT)
  .align_y(Vertical::Center)
  .width(Fill);
  let add = Size {
    height: t::PROXY_HEIGHT,
    padding_x: t::BTN_PADDING_X,
    text: t::PROXY_ADD_TEXT,
    font: fonts::SEMIBOLD,
  };
  let test = Size {
    height: t::PROXY_HEIGHT,
    padding_x: t::BTN_PADDING_X,
    text: t::PROXY_TEST_TEXT,
    font: fonts::MEDIUM,
  };
  let controls = row![
    input,
    container(pill(
      p,
      "+".to_string(),
      Kind::Primary,
      add,
      None,
      Some(manage(ManageMessage::ProxyAdd))
    ))
    .center_x(Shrink)
    .width(Shrink)
    .style(|_: &Theme| container::Style::default()),
    pill(
      p,
      app.i18n.t("Test"),
      Kind::Outline,
      test,
      None,
      settings
        .proxy_active
        .is_some()
        .then_some(manage(ManageMessage::ProxyTest))
    ),
  ]
  .spacing(t::PROXY_INPUT_GAP)
  .align_y(Vertical::Center);
  let list: Element<'a, Message> = if settings.proxy_list.is_empty() {
    container(
      text(app.i18n.t("No proxies configured"))
        .size(t::PROXY_EMPTY_TEXT)
        .color(with_alpha(p.roles.on_surface, t::PROXY_EMPTY_OPACITY)),
    )
    .padding([t::PROXY_EMPTY_PADDING_Y, t::PROXY_EMPTY_PADDING_X])
    .into()
  } else {
    Column::with_children(settings.proxy_list.iter().map(|proxy| {
      let selected = settings.proxy_active.as_deref() == Some(proxy.as_str());
      proxy_item(app, p, proxy, selected)
    }))
    .spacing(t::PROXY_LIST_GAP)
    .width(Fill)
    .into()
  };
  let mut body = column![top, controls, list]
    .spacing(t::PROXY_GAP)
    .width(Fill);
  if let Some((line, failed)) = progress_text(app, &storage.proxy_test) {
    let color = if failed {
      error_color(app)
    } else {
      p.roles.variant
    };
    body = body.push(text(line).size(t::PROXY_EMPTY_TEXT).color(color));
  }
  body.into()
}

/// `#setting-yt-account` : tuile YouTube, état de la connexion, « Sign in » ou « Sign out ».
fn youtube_row<'a>(app: &'a App, p: Palette, entry: &'static Entry) -> Element<'a, Message> {
  let signed_in = app.ui.manage.youtube.signed_in;
  let tile = container(glyph(SVG_YOUTUBE_TILE, 20.0, None))
    .center(t::YT_TILE)
    .style(|_: &Theme| {
      container::Style::default()
        .background(Color::WHITE)
        .border(border::rounded(t::YT_TILE_RADIUS))
    });
  let status = if signed_in {
    app.i18n.t("Signed in")
  } else {
    app.i18n.t("Sign in to import your YouTube library")
  };
  let texts = column![name(p, app.i18n.t(entry.label)), hint(p, true, status)]
    .spacing(tokens::css(2.0))
    .push(hint(p, true, app.i18n.t(entry.hint)));
  // « Read from <navigateur> » n'existe pas dans Riff : connexion par navigateur de riff-rs, gardée sous les textes.
  let texts = if signed_in {
    texts
  } else {
    let reading = app.ui.manage.youtube.progress != Progress::Running;
    let browsers = riff_core::services::ytdlp::cookie_browsers()
      .iter()
      .fold(Row::new().spacing(tokens::css(6.0)), |browsers, browser| {
        browsers.push(pill(
          p,
          format!("{} {browser}", app.i18n.t("Read from")),
          Kind::Outline,
          Size::SMALL,
          None,
          reading.then(|| manage(ManageMessage::CookiesBrowser((*browser).to_string()))),
        ))
      })
      .wrap();
    texts.push(container(browsers).padding(Padding::ZERO.top(tokens::css(6.0))))
  };
  let action = if signed_in {
    pill(
      p,
      app.i18n.t("Sign out"),
      Kind::Outline,
      Size::SIGN,
      None,
      Some(manage(ManageMessage::CookiesDelete)),
    )
  } else {
    pill(
      p,
      app.i18n.t("Sign in"),
      Kind::Primary,
      Size::SIGN,
      None,
      Some(manage(ManageMessage::CookiesPick)),
    )
  };
  icon_row(p, tile.into(), texts, vec![action])
}

/// `#setting-home-layout` : nom, deux choix avec icône, explication dessous.
fn layout_row<'a>(app: &'a App, p: Palette, entry: &'static Entry) -> Element<'a, Message> {
  let current = app.text_pref(TextKey::HomeLayout);
  let choice = |value: &'static str, label: &str, bytes: &'static [u8], tint: bool| {
    let active = current == value;
    let color = if active {
      p.on_secondary_container
    } else {
      p.roles.variant
    };
    let content = row![
      glyph(bytes, t::LAYOUT_ICON, tint.then_some(color)),
      text(label.to_string())
        .size(tokens::settings::SEGMENT_TEXT)
        .font(if active { fonts::BOLD } else { fonts::MEDIUM })
    ]
    .spacing(t::LAYOUT_GAP)
    .align_y(Vertical::Center);
    button(content)
      .padding([
        tokens::settings::SEGMENT_BUTTON_PADDING_Y,
        tokens::settings::SEGMENT_BUTTON_PADDING_X,
      ])
      .on_press(send(SettingsMessage::SetText(
        TextKey::HomeLayout,
        value.to_string(),
      )))
      .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let background = if active {
          Some(Background::Color(p.secondary_container))
        } else if hovered {
          Some(Background::Color(with_alpha(p.roles.on_surface, 0.08)))
        } else {
          None
        };
        button::Style {
          background,
          text_color: color,
          border: border::rounded(9999),
          ..button::Style::default()
        }
      })
  };
  let group = container(
    row![
      choice("spotify", "Spotify", SVG_SPOTIFY, true),
      choice("riff", "Riff", SVG_RIFF, false)
    ]
    .spacing(tokens::settings::SEGMENT_PADDING),
  )
  .padding(tokens::settings::SEGMENT_PADDING)
  .style(move |_: &Theme| {
    container::Style::default()
      .background(p.roles.container)
      .border(border::rounded(9999))
  });
  column![
    name(p, app.i18n.t(entry.label)),
    group,
    container(hint(p, false, app.i18n.t(entry.hint)))
      .padding(Padding::ZERO.top(t::LAYOUT_HINT_TOP - tokens::css(2.0)))
  ]
  .spacing(tokens::settings::ROW_GAP)
  .into()
}

fn advanced_row<'a>(
  app: &'a App,
  p: Palette,
  entry: &'static Entry,
) -> Option<(Element<'a, Message>, bool)> {
  let plain = |element| Some((element, false));
  let hinted = |element| Some((element, true));
  match (entry.label, entry.control) {
    ("Create desktop & app menu shortcut", _) => {
      let info = launcher::info();
      let action = pill(
        p,
        app.i18n.t("Create shortcut"),
        Kind::Outline,
        Size::SMALL,
        None,
        info.can_install.then_some(Message::LauncherInstall),
      );
      hinted(icon_row_plain(
        p,
        name_column(app, p, entry, true),
        vec![action],
      ))
    }
    ("YouTube Account", _) => hinted(youtube_row(app, p, entry)),
    ("In-app proxy (YouTube / SoundCloud bypass)", _) => hinted(proxy_row(app, p, entry)),
    ("Song quality", Control::Choice(key, options)) => {
      let select = dropdown(
        p,
        options_of(app, options),
        app.text_pref(key),
        move |value| send(SettingsMessage::SetText(key, value)),
      );
      hinted(
        row![
          name_column(app, p, entry, true),
          container(select).width(tokens::css(220.0))
        ]
        .spacing(tokens::settings::ROW_TOP_GAP)
        .align_y(Vertical::Center)
        .into(),
      )
    }
    ("Home layout", _) => plain(layout_row(app, p, entry)),
    ("Legacy media key capture (Windows)", Control::Toggle(key)) => {
      // Riff : activé = capture des touches. riff-rs : `MediaSession` activé = superposition du système, sens inverse.
      let capture = !app.bool_pref(key);
      plain(toggle_row(
        app,
        p,
        entry,
        false,
        capture,
        send(SettingsMessage::SetBool(key, capture)),
      ))
    }
    ("Allow multiple instances", Control::Panel(Panel::MultiInstance)) => {
      let on = app.ui.manage.storage.multi_instance;
      plain(toggle_row(
        app,
        p,
        entry,
        false,
        on,
        manage(ManageMessage::MultiInstance(!on)),
      ))
    }
    // Réglages de riff-rs que Riff n'a pas : mêmes rangées que celles de Riff.
    ("Update yt-dlp automatically", _) => {
      let on = app.ui.manage.storage.yt_dlp_auto_update;
      hinted(toggle_row(
        app,
        p,
        entry,
        true,
        on,
        manage(ManageMessage::YtDlpAutoUpdate(!on)),
      ))
    }
    ("Compact window", _) => plain(
      row![
        name_column(app, p, entry, false),
        pill(
          p,
          app.i18n.t("Switch to compact window"),
          Kind::Outline,
          Size::SMALL,
          None,
          Some(Message::ToggleMiniMode),
        )
      ]
      .spacing(tokens::settings::ROW_TOP_GAP)
      .align_y(Vertical::Center)
      .into(),
    ),
    ("Global shortcuts", _) => plain(
      column![
        name(p, app.i18n.t(entry.label)),
        container(hint(p, false, shortcuts_text(app))).padding(Padding::ZERO.top(tokens::css(6.0)))
      ]
      .width(Fill)
      .into(),
    ),
    (_, Control::Toggle(key)) => {
      let on = app.bool_pref(key);
      plain(toggle_row(
        app,
        p,
        entry,
        false,
        on,
        send(SettingsMessage::SetBool(key, !on)),
      ))
    }
    (_, Control::VisualToggle(key)) => {
      let on = app.visual_bool(key);
      plain(toggle_row(
        app,
        p,
        entry,
        false,
        on,
        send(SettingsMessage::Visual(VisualMessage::Bool(key, !on))),
      ))
    }
    _ => None,
  }
}

fn icon_row_plain<'a>(
  p: Palette,
  texts: Column<'a, Message>,
  actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
  let mut line = row![texts.width(Fill)]
    .spacing(tokens::settings::ROW_TOP_GAP)
    .align_y(Vertical::Center);
  if !actions.is_empty() {
    line = line.push(Row::with_children(actions).spacing(t::HEAD_ACTIONS_GAP));
  }
  let _ = p;
  line.into()
}

// ---------------------------------------------------------------------------------------------
// À propos
// ---------------------------------------------------------------------------------------------

/// Texte du dialogue « Keyboard shortcuts » : raccourcis globaux, et ceux que le système a refusés.
fn shortcuts_text(app: &App) -> String {
  #[cfg(any(windows, target_os = "linux"))]
  {
    let failed = crate::system::startup::report()
      .map(|report| report.failed())
      .unwrap_or_default();
    crate::system::hotkeys::bindings()
      .into_iter()
      .map(|(hotkey, command)| {
        let name = crate::system::hotkeys::describe(&hotkey);
        if failed.contains(&name) {
          format!(
            "{name} : {command:?} ({})",
            app.i18n.t("already used by another program")
          )
        } else {
          format!("{name} : {command:?}")
        }
      })
      .collect::<Vec<_>>()
      .join("\n")
  }
  #[cfg(not(any(windows, target_os = "linux")))]
  {
    app.i18n.t("Not available on this system.")
  }
}

fn update_lines<'a>(app: &App, p: Palette) -> Vec<Element<'a, Message>> {
  let line = |content: String| hint(p, false, content);
  match &app.update {
    UpdateState::Idle | UpdateState::Checking => Vec::new(),
    UpdateState::Done(Outcome::UpToDate) => vec![line(app.i18n.t("You are up to date."))],
    UpdateState::Done(Outcome::NoRelease) => vec![line(app.i18n.t("No public release found."))],
    UpdateState::Done(Outcome::Available(release)) => vec![line(format!(
      "{} {}",
      app.i18n.t("Version available:"),
      release.version
    ))],
    UpdateState::Failed(error) => vec![line(format!(
      "{} {error}",
      app.i18n.t("Update check failed:")
    ))],
  }
  .into_iter()
  .chain(install_line(app).map(line))
  .collect()
}

/// État du téléchargement et de l'installation, seulement une fois lancés.
fn install_line(app: &App) -> Option<String> {
  Some(match &app.install {
    InstallState::Idle => return None,
    InstallState::Downloading(percent) => format!("{} {percent} %", app.i18n.t("Downloading…")),
    InstallState::Verifying => app.i18n.t("Verifying…"),
    InstallState::Launched => app.i18n.t("Installing: Riff closes and restarts."),
    InstallState::Failed(error) => format!("{} {error}", app.i18n.t("Update failed:")),
  })
}

/// « Download and install » : seulement quand une version plus récente est offerte, sous Windows, hors installation.
fn can_start_install(app: &App) -> bool {
  update_install::can_install()
    && matches!(app.install, InstallState::Idle | InstallState::Failed(_))
}

fn about_row<'a>(app: &'a App, p: Palette, entry: &'static Entry) -> Option<Element<'a, Message>> {
  let outline = |label: &str, message: Message| {
    pill(
      p,
      app.i18n.t(label),
      Kind::Outline,
      Size::ROW,
      None,
      Some(message),
    )
  };
  match entry.label {
    "Riff" => {
      let logo = container(glyph(SVG_RIFF, 20.0, None))
        .center(t::ICON_CIRCLE)
        .style(move |_: &Theme| {
          container::Style::default()
            .background(p.roles.container)
            .border(border::rounded(t::ICON_CIRCLE / 2.0))
        });
      let mut texts = column![
        name(p, app.i18n.t("Riff")),
        hint(
          p,
          false,
          format!("{} {}", app.i18n.t("Version"), &VERSION_TAG[1..])
        )
      ];
      for line in update_lines(app, p) {
        texts = texts.push(line);
      }
      let mut actions = Vec::new();
      if app.settings.update_check {
        let checking = app.update == UpdateState::Checking;
        actions.push(pill(
          p,
          app.i18n.t(if checking {
            "Checking…"
          } else {
            "Check for updates"
          }),
          Kind::Primary,
          Size::ROW,
          None,
          (!checking).then_some(Message::UpdateCheck),
        ));
        if let UpdateState::Done(Outcome::Available(_)) = &app.update {
          if update_install::can_install() {
            actions.push(pill(
              p,
              app.i18n.t("Download and install"),
              Kind::Primary,
              Size::ROW,
              None,
              can_start_install(app).then_some(Message::UpdateInstall),
            ));
          }
          actions.push(outline("Open release page", Message::OpenReleasePage));
        }
      }
      Some(icon_row(p, logo.into(), texts, actions))
    }
    "Keyboard shortcuts" => {
      let open = Message::OpenDialog(
        Dialog {
          title: app.i18n.t("Keyboard shortcuts"),
          body: shortcuts_text(app),
          cancel: app.i18n.t("Close"),
          confirm: None,
        },
        None,
      );
      Some(icon_row_plain(
        p,
        column![name(p, app.i18n.t("Keyboard shortcuts"))],
        vec![outline("Keyboard shortcuts", open)],
      ))
    }
    "Diagnostics" => Some(icon_row_plain(
      p,
      column![name(p, app.i18n.t("Diagnostics"))],
      vec![outline(
        "Copy diagnostics",
        manage(ManageMessage::CopyDiagnostics),
      )],
    )),
    "Logs" => Some(icon_row_plain(
      p,
      column![name(p, app.i18n.t("Logs"))],
      vec![outline("Open logs folder", manage(ManageMessage::OpenLogs))],
    )),
    _ => None,
  }
}

/// Bannière de la page À propos : fond violet, logos flottants et « cookie » central, titre dessous.
fn about_hero<'a>(p: Palette) -> Element<'a, Message> {
  let radius = border::Radius {
    top_left: t::HERO_RADIUS_TOP,
    top_right: t::HERO_RADIUS_TOP,
    bottom_right: t::HERO_RADIUS_BOTTOM,
    bottom_left: t::HERO_RADIUS_BOTTOM,
  };
  let background = Linear::new(Radians::PI)
    .add_stop(0.0, rgb(t::HERO_TOP))
    .add_stop(0.52, rgb(t::HERO_MIDDLE))
    .add_stop(1.0, rgb(t::HERO_BOTTOM));
  let banner = responsive(move |size| {
    let fill = container(space())
      .width(Fill)
      .height(Fill)
      .style(move |_: &Theme| {
        container::Style::default()
          .background(Background::Gradient(background.into()))
          .border(Border::default().rounded(radius))
      });
    let mut layers = stack![fill].width(Fill).height(Fill);
    for (px, top, left) in t::FLOATS {
      layers = layers.push(
        container(glyph(
          SVG_RIFF,
          px,
          Some(with_alpha(rgb(t::FLOAT_COLOR), t::FLOAT_OPACITY)),
        ))
        .padding(Padding::ZERO.top(size.height * top).left(size.width * left)),
      );
    }
    let cookie = container(glyph(SVG_RIFF, 46.0, Some(rgb(t::COOKIE_LOGO_COLOR))))
      .center(t::COOKIE)
      .style(move |_: &Theme| {
        container::Style::default()
          .background(rgb(t::COOKIE_COLOR))
          .border(border::rounded(t::COOKIE * 0.34))
      });
    layers.push(container(cookie).center(Fill)).into()
  })
  .height(t::HERO_HEIGHT);
  let title = text("Riff")
    .size(t::HERO_TITLE_TEXT)
    .font(fonts::BOLD)
    .color(p.roles.on_surface);
  column![banner, title]
    .spacing(t::HERO_GAP)
    .align_x(Horizontal::Center)
    .width(Fill)
    .padding(Padding::ZERO.bottom(t::HERO_MARGIN_BOTTOM))
    .into()
}

// ---------------------------------------------------------------------------------------------
// Compte
// ---------------------------------------------------------------------------------------------

/// Héros du compte : dégradé teinté par l'avatar, motif de logos, avatar de 96 px et pseudo modifiable.
fn account_hero<'a>(app: &'a App) -> Element<'a, Message> {
  let background = Linear::new(Radians::PI)
    .add_stop(0.0, rgb(t::ACCOUNT_C1))
    .add_stop(1.0, rgb(t::ACCOUNT_C2));
  let radius = t::ACCOUNT_RADIUS;
  let nickname = app.text_pref(TextKey::Nickname).to_string();
  let input = text_input("User", &nickname)
    .id(NICKNAME_ID)
    .on_input(|value| send(SettingsMessage::SetText(TextKey::Nickname, value)))
    .size(t::ACCOUNT_NAME_TEXT)
    .font(fonts::BOLD)
    .align_x(Horizontal::Center)
    .padding(Padding::from([tokens::css(2.0), tokens::css(10.0)]))
    .width(tokens::css(260.0))
    .style(|_, status| {
      let active = matches!(
        status,
        text_input::Status::Hovered | text_input::Status::Focused { .. }
      );
      text_input::Style {
        background: Background::Color(if active {
          Color::from_rgba(1.0, 1.0, 1.0, 0.14)
        } else {
          Color::TRANSPARENT
        }),
        border: border::rounded(tokens::css(10.0)),
        icon: Color::WHITE,
        placeholder: Color::from_rgba(1.0, 1.0, 1.0, 0.6),
        value: Color::WHITE,
        selection: Color::from_rgba(1.0, 1.0, 1.0, 0.3),
      }
    });
  let avatar = container(glyph(SVG_RIFF, 56.0, Some(rgb(t::COOKIE_COLOR))))
    .center(t::ACCOUNT_AVATAR)
    .style(|_: &Theme| {
      container::Style::default()
        .background(Color::from_rgba(0.0, 0.0, 0.0, 0.25))
        .border(border::rounded(t::ACCOUNT_AVATAR / 2.0))
    });
  let pattern = responsive(move |size| {
    let fill = container(space())
      .width(Fill)
      .height(Fill)
      .style(move |_: &Theme| {
        container::Style::default()
          .background(Background::Gradient(background.into()))
          .border(border::rounded(radius))
      });
    let mut layers = stack![fill].width(Fill).height(Fill);
    for (top, left, scale, opacity) in t::PATTERN {
      layers = layers.push(
        container(glyph(
          SVG_RIFF,
          t::PATTERN_SIZE * scale,
          Some(Color::from_rgba(0.0, 0.0, 0.0, t::PATTERN_ALPHA * opacity)),
        ))
        .padding(Padding::ZERO.top(size.height * top).left(size.width * left)),
      );
    }
    layers.into()
  });
  let center = container(
    column![avatar, input]
      .spacing(t::ACCOUNT_CENTER_GAP)
      .align_x(Horizontal::Center),
  )
  .center(Fill);
  container(stack![pattern, center])
    .height(t::ACCOUNT_HEIGHT + t::ACCOUNT_MARGIN_BOTTOM)
    .width(Fill)
    .padding(Padding::ZERO.bottom(t::ACCOUNT_MARGIN_BOTTOM))
    .into()
}

/// Satellites autour du logo : `.acc-orbital-widget`.
fn orbital<'a>() -> Element<'a, Message> {
  let mut layers = stack![space().width(t::ORBIT).height(t::ORBIT)]
    .width(t::ORBIT)
    .height(t::ORBIT);
  let offset = (t::ORBIT - t::ORBIT_CENTER) / 2.0;
  layers = layers.push(
    container(glyph(SVG_RIFF, 66.0, Some(Color::WHITE)))
      .padding(Padding::ZERO.top(offset).left(offset)),
  );
  for (index, (top, left, color, px)) in t::SATELLITES.into_iter().enumerate() {
    let disc = container(glyph(SATELLITE_SVG[index], px, None))
      .center(t::SATELLITE)
      .style(move |_: &Theme| {
        container::Style::default()
          .background(rgb(color))
          .border(border::rounded(t::SATELLITE / 2.0))
      });
    layers = layers
      .push(container(disc).padding(Padding::ZERO.top(tokens::css(top)).left(tokens::css(left))));
  }
  layers.into()
}

fn account_row<'a>(
  app: &'a App,
  p: Palette,
  entry: &'static Entry,
) -> Option<(Element<'a, Message>, Padding)> {
  let acc = |label: &str, kind: Kind, message: Message| {
    pill(
      p,
      app.i18n.t(label),
      kind,
      Size::ACCOUNT,
      None,
      Some(message),
    )
  };
  let row = match entry.label {
    "Profile avatar" => (
      icon_row_plain(
        p,
        name_column(app, p, entry, true),
        vec![acc(
          "Reset avatar",
          Kind::Outline,
          send(SettingsMessage::ResetAvatar),
        )],
      ),
      hint_padding(),
    ),
    "Export Riff account (.rfacc)" => (
      icon_row_plain(
        p,
        name_column(app, p, entry, true),
        vec![acc(
          "Export",
          Kind::Primary,
          manage(ManageMessage::ExportAccount),
        )],
      ),
      hint_padding(),
    ),
    "Import Riff account (.rfacc)" => (
      icon_row_plain(
        p,
        name_column(app, p, entry, true),
        vec![acc("Import", Kind::Outline, import_account(app))],
      ),
      hint_padding(),
    ),
    "Connected services" => {
      let texts = column![
        text(app.i18n.t(entry.label))
          .size(t::SERVICES_NAME_TEXT)
          .font(fonts::SEMIBOLD)
          .color(p.roles.on_surface),
        container(
          text(app.i18n.t(entry.hint))
            .size(tokens::settings::HINT_TEXT)
            .line_height(1.45)
            .color(with_alpha(p.roles.outline, tokens::settings::HINT_OPACITY))
        )
        .max_width(t::SERVICES_HINT_WIDTH),
        container(acc(
          "Manage integrations",
          Kind::Outline,
          send(SettingsMessage::OpenCategory(Category::Integrations)),
        ))
        .padding(Padding::ZERO.top(t::SERVICES_BUTTON_TOP - t::SERVICES_GAP)),
      ]
      .spacing(t::SERVICES_GAP)
      .width(Fill);
      (
        row![texts, orbital()]
          .spacing(tokens::css(20.0))
          .align_y(Vertical::Center)
          .into(),
        Padding::from([t::SERVICES_PADDING_Y, t::SERVICES_PADDING_X]),
      )
    }
    _ => return None,
  };
  Some(row)
}

// ---------------------------------------------------------------------------------------------
// Thèmes personnalisés et Plugins
// ---------------------------------------------------------------------------------------------

/// `.ct-head` : titre et texte à gauche, boutons à droite.
fn ct_head<'a>(
  p: Palette,
  title: String,
  body: Element<'a, Message>,
  actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
  let left = column![
    text(title)
      .size(t::HEAD_TITLE_TEXT)
      .font(fonts::SEMIBOLD)
      .color(p.roles.on_surface),
    body
  ]
  .spacing(t::HEAD_TITLE_MARGIN)
  .width(Fill);
  container(
    row![
      left,
      Row::with_children(actions)
        .spacing(t::HEAD_ACTIONS_GAP)
        .align_y(Vertical::Center)
    ]
    .spacing(t::HEAD_GAP)
    .align_y(Vertical::Center),
  )
  .padding(Padding::ZERO.bottom(t::HEAD_MARGIN_BOTTOM))
  .into()
}

fn head_line<'a>(p: Palette, line: f32, content: String) -> Element<'a, Message> {
  text(content)
    .size(t::HEAD_TEXT)
    .line_height(line)
    .color(p.roles.variant)
    .into()
}

fn themes_head<'a>(app: &App, p: Palette) -> Element<'a, Message> {
  use crate::theme_editor::ThemeEditorMessage;
  let body = column![
    head_line(p, t::HEAD_LINE_THEMES, app.i18n.t("Add a .css file to restyle Riff.")),
    head_line(
      p,
      t::HEAD_LINE_THEMES,
      app.i18n.t(
        "Tip: Riff sets its color variables from JavaScript, so a theme must use !important to override them (for example --md-sys-color-primary: #ff4081 !important)."
      )
    ),
    head_line(
      p,
      t::HEAD_LINE_THEMES,
      app.i18n.t("Ctrl+Shift+R turns every custom theme and CSS off if something breaks.")
    ),
  ];
  ct_head(
    p,
    app.i18n.t("Custom themes"),
    body.into(),
    vec![pill(
      p,
      app.i18n.t("Add"),
      Kind::Primary,
      Size::HEAD,
      None,
      Some(Message::ThemeEditor(ThemeEditorMessage::New)),
    )],
  )
}

fn plugins_head<'a>(app: &App, p: Palette) -> Element<'a, Message> {
  let warning = app.theme.scheme.error;
  let warning = Color::from_rgb8(warning.red, warning.green, warning.blue);
  let body = iced::widget::rich_text::<(), Message, Theme, iced::Renderer>([
    iced::widget::span(app.i18n.t("Plugins are JavaScript files with full access to Riff."))
      .color(warning),
    iced::widget::span(format!(
      " {}",
      app.i18n.t(
        "Only add plugins you trust. They live in the plugins folder: delete a file and the plugin is gone. Ctrl+Shift+R turns every plugin off."
      )
    ))
    .color(p.roles.variant),
  ])
  .size(t::HEAD_TEXT)
  .line_height(t::HEAD_LINE_PLUGINS);
  let action = |label: &str, kind: Kind, message: Message| {
    pill(p, app.i18n.t(label), kind, Size::HEAD, None, Some(message))
  };
  ct_head(
    p,
    app.i18n.t("Plugins"),
    body.into(),
    vec![
      action(
        "Open folder",
        Kind::Outline,
        send_plugin(PluginMessage::OpenFolder),
      ),
      action(
        "Reload list",
        Kind::Outline,
        send_plugin(PluginMessage::Scan),
      ),
      action("Add", Kind::Primary, send_plugin(PluginMessage::Import)),
    ],
  )
}

fn send_plugin(message: PluginMessage) -> Message {
  Message::Plugin(message)
}

/// `.ct-empty` : texte centré sur `surface-container-high`.
fn empty_card<'a>(p: Palette, content: String) -> Element<'a, Message> {
  container(
    text(content)
      .size(t::EMPTY_TEXT)
      .color(with_alpha(p.roles.on_surface, t::EMPTY_OPACITY)),
  )
  .padding([t::EMPTY_PADDING_Y, t::EMPTY_PADDING_X])
  .center_x(Fill)
  .into()
}

/// `.ct-card` : nom, méta, puis boutons.
/// Raccourcit `text` pour tenir dans `width` pixels à `size`, avec « … » (largeur moyenne d'un caractère : 0,5 em).
fn ellipsize(text: &str, width: f32, size: f32) -> String {
  let fit = (width / (size * 0.5)).floor().max(1.0) as usize;
  if text.chars().count() <= fit {
    return text.to_string();
  }
  let kept: String = text.chars().take(fit.saturating_sub(1)).collect();
  format!("{}…", kept.trim_end())
}

fn card<'a>(
  p: Palette,
  title: String,
  meta: String,
  extra: Vec<Element<'a, Message>>,
  actions: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
  let meta_color = with_alpha(p.roles.on_surface, t::CARD_META_OPACITY);
  let mut info = column![
    text(title)
      .size(t::CARD_NAME_TEXT)
      .font(fonts::BOLD)
      .color(p.roles.on_surface)
      .wrapping(text::Wrapping::None),
    container(responsive(move |size| {
      // `text-overflow: ellipsis` : iced ne coupe pas avec « … », la ligne est raccourcie d'après sa largeur moyenne.
      text(ellipsize(&meta, size.width, t::CARD_META_TEXT))
        .size(t::CARD_META_TEXT)
        .color(meta_color)
        .wrapping(text::Wrapping::None)
        .into()
    }))
    .height(t::CARD_META_TEXT * 1.35)
    .width(Fill)
    .padding(Padding::ZERO.top(t::CARD_META_TOP)),
  ]
  .width(Fill);
  for line in extra {
    info = info.push(line);
  }
  row![
    container(info).width(Fill).clip(true),
    Row::with_children(actions)
      .spacing(t::CARD_GAP / 2.0)
      .align_y(Vertical::Center)
  ]
  .spacing(t::CARD_GAP)
  .align_y(Vertical::Center)
  .into()
}

fn delete_button<'a>(p: Palette, message: Message) -> Element<'a, Message> {
  let widget =
    button(container(glyph(SVG_TRASH, 22.0, Some(p.roles.variant))).center(t::CARD_DELETE_SIZE))
      .padding(0)
      .on_press(message.clone())
      .style(move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
          .then(|| Background::Color(with_alpha(p.roles.on_surface, 0.08))),
        border: border::rounded(9999),
        ..button::Style::default()
      });
  focusable(widget)
    .on_activate(Some(message))
    .radius(t::CARD_DELETE_SIZE / 2.0)
    .into()
}

/// Ce que chaque permission autorise, montré avant l'activation.
const fn permission_text(permission: riff_plugins::Permission) -> &'static str {
  use riff_plugins::Permission;
  match permission {
    Permission::Queue => "queue: read and add to the play queue",
    Permission::Library => "library: read favorites and history",
    Permission::Search => "search: search and get recommendations through Riff",
    Permission::Shuffle => "shuffle: choose the next track in shuffle",
    Permission::Notify => "notify: show notifications",
  }
}

fn plugin_card<'a>(app: &'a App, p: Palette, entry: &'a PluginEntry) -> Element<'a, Message> {
  let file = entry.file.clone();
  let remove = delete_button(p, send_plugin(PluginMessage::Remove(file.clone())));
  match &entry.kind {
    PluginKind::Native { manifest, consent } => {
      let on = *consent == ConsentState::Granted && app.plugins.names.contains_key(&file);
      let mut title = manifest.name.clone();
      if !manifest.version.is_empty() {
        title.push_str(&format!(" v{}", manifest.version));
      }
      let mut parts = Vec::new();
      if !manifest.author.is_empty() {
        parts.push(format!("{} {}", app.i18n.t("by"), manifest.author));
      }
      if !manifest.description.is_empty() {
        parts.push(manifest.description.clone());
      }
      parts.push(file.clone());
      let permissions = if manifest.permissions.is_empty() {
        app.i18n.t("No permission")
      } else {
        manifest
          .permissions
          .iter()
          .map(|permission| app.i18n.t(permission_text(*permission)))
          .collect::<Vec<_>>()
          .join(" · ")
      };
      let mut extra: Vec<Element<'a, Message>> = vec![hint(p, false, permissions)];
      if *consent == ConsentState::Changed {
        extra.push(hint(
          p,
          false,
          app.i18n.t("Changed since you enabled it. Check its permissions, then enable it again."),
        ));
      }
      let problem = app
        .plugins
        .errors
        .get(&file)
        .or_else(|| app.plugins.consent.disabled.get(&file));
      if let Some(problem) = problem {
        extra.push(hint(p, false, format!("{}: {problem}", app.i18n.t("Error"))));
      }
      let target = EditorTarget::Plugin(file.clone());
      let edit = target
        .path()
        .is_ok()
        .then(|| Message::Editor(EditorMessage::Open(target)));
      card(
        p,
        title,
        parts.join(" · "),
        extra,
        vec![
          pill(p, app.i18n.t("Edit"), Kind::Outline, Size::SMALL, None, edit),
          remove,
          switch(p, on, send_plugin(PluginMessage::Toggle(file, !on))),
        ],
      )
    }
    PluginKind::Invalid(reason) => card(p, file, reason.clone(), Vec::new(), vec![remove]),
    PluginKind::Incompatible => card(
      p,
      file,
      app.i18n.t(
        "JavaScript plugin from Riff, incompatible. See docs/plugins/PORTAGE.md to port it to Rhai.",
      ),
      Vec::new(),
      vec![remove],
    ),
  }
}

fn theme_card<'a>(
  app: &'a App,
  p: Palette,
  theme: &'a crate::theme::file::ThemeEntry,
) -> Element<'a, Message> {
  use crate::theme::file::ThemeStatus;
  use crate::theme_editor::ThemeEditorMessage;
  let selected = app.settings.native_theme.as_deref() == Some(theme.id.as_str());
  let toggle = if selected {
    Message::ClearThemeFile
  } else {
    Message::SelectThemeFile(theme.id.clone())
  };
  match &theme.status {
    ThemeStatus::Incompatible => card(
      p,
      theme.name.clone(),
      app.i18n.t("Old CSS theme of Riff: not importable."),
      Vec::new(),
      Vec::new(),
    ),
    ThemeStatus::Refused(reason) => card(
      p,
      theme.name.clone(),
      reason.clone(),
      Vec::new(),
      Vec::new(),
    ),
    ThemeStatus::Valid(_) => card(
      p,
      theme.name.clone(),
      theme.id.clone(),
      Vec::new(),
      vec![
        pill(
          p,
          app.i18n.t("Edit"),
          Kind::Outline,
          Size::SMALL,
          None,
          Some(Message::ThemeEditor(ThemeEditorMessage::Edit(
            theme.id.clone(),
          ))),
        ),
        pill(
          p,
          app.i18n.t("Edit as text"),
          Kind::Outline,
          Size::SMALL,
          None,
          Some(Message::Editor(EditorMessage::Open(EditorTarget::Theme(
            theme.id.clone(),
          )))),
        ),
        switch(p, selected, toggle),
      ],
    ),
  }
}

/// `.ct-card` dans `.ct-list.m3-group` : `surface-container-high`, coins du groupe.
fn card_shell<'a>(
  p: Palette,
  content: Element<'a, Message>,
  padding: Padding,
  index: usize,
  count: usize,
) -> Element<'a, Message> {
  container(content)
    .padding(padding)
    .width(Fill)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(p.roles.high)
        .border(Border::default().rounded(row_radius(index, count)))
    })
    .into()
}

/// Liste en groupe de cartes.
fn card_group<'a>(p: Palette, cards: Vec<Element<'a, Message>>) -> Element<'a, Message> {
  let count = cards.len();
  let padding = Padding {
    top: t::CARD_PADDING_Y,
    right: t::CARD_PADDING_RIGHT,
    bottom: t::CARD_PADDING_Y,
    left: t::CARD_PADDING_LEFT,
  };
  Column::with_children(
    cards
      .into_iter()
      .enumerate()
      .map(|(index, content)| card_shell(p, content, padding, index, count)),
  )
  .spacing(tokens::settings::GROUP_GAP)
  .padding(tokens::settings::GROUP_PADDING)
  .width(Fill)
  .into()
}

fn empty_group<'a>(p: Palette, content: String) -> Element<'a, Message> {
  container(card_shell(p, empty_card(p, content), Padding::ZERO, 0, 1))
    .padding(tokens::settings::GROUP_PADDING)
    .width(Fill)
    .into()
}

// ---------------------------------------------------------------------------------------------
// Points d'entrée de `settings.rs`
// ---------------------------------------------------------------------------------------------

fn plain_padding() -> Padding {
  Padding::from([
    tokens::settings::ROW_PADDING_Y,
    tokens::settings::ROW_PADDING_X,
  ])
}

fn hint_padding() -> Padding {
  Padding::from([
    tokens::settings::ROW_HINT_PADDING_Y,
    tokens::settings::ROW_HINT_PADDING_X,
  ])
}

/// Rangées Discord de l'onglet Intégrations (`index.html:2043-2087`) ; les autres restent communes. Les réglages
/// de l'activité reprennent la rangée de « Show artist in Discord status », qu'ils remplacent, sans sa description :
/// marges de `.setting-item` sans `.setting-hint`.
fn integrations_row<'a>(
  app: &'a App,
  p: Palette,
  entry: &'static Entry,
) -> Option<(Element<'a, Message>, Padding)> {
  let hinted = !entry.hint.is_empty();
  let padding = if hinted {
    hint_padding()
  } else {
    plain_padding()
  };
  let key = match entry.control {
    Control::Toggle(key) => key,
    Control::Choice(key @ (TextKey::DiscordStatus | TextKey::DiscordTime), options) => {
      let select = dropdown(
        p,
        options_of(app, options),
        app.text_pref(key),
        move |value| send(SettingsMessage::SetText(key, value)),
      );
      let line = row![
        name_column(app, p, entry, hinted),
        container(select).width(tokens::css(220.0))
      ]
      .spacing(tokens::settings::ROW_TOP_GAP)
      .align_y(Vertical::Center);
      return Some((line.into(), padding));
    }
    _ => return None,
  };
  let on = app.bool_pref(key);
  let message = send(SettingsMessage::SetBool(key, !on));
  match entry.label {
    "Discord RPC" => {
      let reconnect = pill(
        p,
        app.i18n.t("Reconnect"),
        Kind::Outline,
        Size::RECONNECT,
        None,
        Some(send(SettingsMessage::DiscordReconnect)),
      );
      let line = row![
        glyph(SVG_DISCORD, t::ICON_GLYPH, Some(rgb(t::DISCORD_BLURPLE))),
        name(p, app.i18n.t(entry.label)),
        space().width(Fill),
        reconnect,
        switch(p, on, message),
      ]
      .spacing(tokens::css(8.0))
      .align_y(Vertical::Center);
      Some((line.into(), plain_padding()))
    }
    "Discord cover" | "Show when paused" | "Album on cover hover" => {
      Some((toggle_row(app, p, entry, hinted, on, message), padding))
    }
    _ => None,
  }
}

/// Corps et marges d'une rangée de ces catégories ; `None` : rangée commune.
pub(super) fn body<'a>(
  app: &'a App,
  p: Palette,
  entry: &'static Entry,
) -> Option<(Element<'a, Message>, Padding)> {
  match entry.category {
    Category::Storage => storage_row(app, p, entry).map(|row| (row, plain_padding())),
    Category::Advanced => advanced_row(app, p, entry).map(|(row, hinted)| {
      (
        row,
        if hinted {
          hint_padding()
        } else {
          plain_padding()
        },
      )
    }),
    Category::About => about_row(app, p, entry).map(|row| (row, plain_padding())),
    Category::Integrations => integrations_row(app, p, entry),
    Category::Account => account_row(app, p, entry),
    _ => None,
  }
}

/// La rangée fait-elle partie du groupe affiché ? Le pseudo du compte est dans le héros.
pub(super) fn grouped(entry: &Entry) -> bool {
  !(entry.category == Category::Account && entry.label == "Nickname")
}

/// Section Avancé repliée : aucune rangée.
pub(super) fn hides(app: &App, category: Category) -> bool {
  category == Category::Advanced && !app.settings.adv_open
}

/// En-tête au-dessus du groupe.
pub(super) fn head<'a>(
  app: &'a App,
  p: Palette,
  category: Category,
) -> Option<Element<'a, Message>> {
  match category {
    Category::Advanced => Some(advanced_header(app, p)),
    Category::About => Some(about_hero(p)),
    Category::Account => Some(account_hero(app)),
    Category::CustomThemes => Some(themes_head(app, p)),
    Category::Plugins => Some(plugins_head(app, p)),
    _ => None,
  }
}

/// Liste de cartes à la place des rangées (Thèmes personnalisés, Plugins).
pub(super) fn custom_group<'a>(
  app: &'a App,
  p: Palette,
  category: Category,
) -> Option<Element<'a, Message>> {
  match category {
    Category::Plugins => {
      let cards: Vec<Element<'a, Message>> = app
        .plugins
        .entries
        .iter()
        .map(|entry| plugin_card(app, p, entry))
        .collect();
      let list = if cards.is_empty() {
        empty_group(
          p,
          app
            .i18n
            .t("No plugins. Put .js files into the plugins folder or click Add."),
        )
      } else {
        card_group(p, cards)
      };
      // « New plugin » n'existe pas dans Riff : bouton en plus, sous la liste.
      let create = row![
        space().width(Fill),
        pill(
          p,
          app.i18n.t("New plugin"),
          Kind::Outline,
          Size::SMALL,
          None,
          Some(Message::Editor(EditorMessage::NewPlugin)),
        )
      ];
      Some(column![list, create].spacing(tokens::css(6.0)).into())
    }
    Category::CustomThemes => {
      if let Some(draft) = &app.ui.theme_editor {
        return Some(super::theme_editor::panel(app, draft));
      }
      if app.ui.prefs.themes.is_empty() {
        return Some(empty_group(
          p,
          app
            .i18n
            .t("No themes yet. Click Add and choose a .css file."),
        ));
      }
      Some(card_group(
        p,
        app
          .ui
          .prefs
          .themes
          .iter()
          .map(|theme| theme_card(app, p, theme))
          .collect(),
      ))
    }
    _ => None,
  }
}

/// Ligne d'état sous le groupe.
pub(super) fn tail<'a>(app: &App, p: Palette, category: Category) -> Option<Element<'a, Message>> {
  (category == Category::Storage)
    .then(|| storage_tail(app, p))
    .flatten()
}

/// `#settings-nav-account` (`index.html:935-943`, `index.html:14-40`) : avatar de 30 px, pseudo et « Account », 48 px de haut.
pub(super) fn account_nav_button<'a>(
  app: &App,
  p: Palette,
  selected: bool,
) -> Element<'a, Message> {
  let nickname = app.text_pref(TextKey::Nickname).trim().to_string();
  let name = if nickname.is_empty() {
    "User".to_string()
  } else {
    nickname
  };
  let (name_color, sub_color) = if selected {
    (p.roles.on_primary, with_alpha(p.roles.on_primary, 0.72))
  } else {
    (p.roles.on_surface, with_alpha(p.roles.variant, 0.72))
  };
  let high = p.roles.highest;
  let avatar = container(glyph(SVG_RIFF, 20.0, Some(p.roles.primary)))
    .center(t::NAV_ACCOUNT_AVATAR)
    .style(move |_: &Theme| {
      container::Style::default()
        .background(high)
        .border(border::rounded(9999))
    });
  let meta = column![
    text(name)
      .size(t::NAV_ACCOUNT_NAME_TEXT)
      .font(fonts::BOLD)
      .color(name_color)
      .wrapping(text::Wrapping::None),
    text(app.i18n.t("Account"))
      .size(t::NAV_ACCOUNT_SUB_TEXT)
      .color(sub_color)
  ];
  let content = row![avatar, meta]
    .spacing(tokens::css(10.0))
    .align_y(Vertical::Center);
  let radius = t::NAV_ACCOUNT_HEIGHT / 2.0;
  let widget = button(container(content).height(Fill).align_y(Vertical::Center))
    .padding(Padding::from([0.0, tokens::css(12.0)]))
    .width(Fill)
    .height(t::NAV_ACCOUNT_HEIGHT)
    .on_press(send(SettingsMessage::OpenCategory(Category::Account)))
    .style(move |_, status| {
      let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
      let background = if selected {
        Some(p.roles.primary)
      } else if hovered {
        Some(with_alpha(p.roles.on_surface, 0.08))
      } else {
        None
      };
      button::Style {
        background: background.map(Background::Color),
        border: border::rounded(9999),
        ..button::Style::default()
      }
    });
  focusable(widget)
    .on_activate(Some(send(SettingsMessage::OpenCategory(Category::Account))))
    .radius(radius)
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_discord_activity_rows_without_a_hint_take_the_plain_padding() {
    let app = App::new(
      crate::window::PersistedWindowState::default(),
      crate::settings::NativeSettings::default(),
      crate::i18n::I18n::default(),
    );
    let p = Palette::of(&app);
    let rows: Vec<_> = crate::prefs::in_category(Category::Integrations)
      .filter(|entry| entry.label != "Discord RPC")
      .filter_map(|entry| integrations_row(&app, p, entry).map(|(_, padding)| (entry, padding)))
      .collect();
    assert_eq!(rows.len(), 5);
    for (entry, padding) in rows {
      assert!(entry.hint.is_empty(), "{}", entry.label);
      assert_eq!(padding, plain_padding(), "{}", entry.label);
    }
  }

  #[test]
  fn the_nickname_lives_in_the_account_hero_not_in_the_rows() {
    let nickname = crate::prefs::in_category(Category::Account)
      .find(|entry| entry.label == "Nickname")
      .expect("réglage du pseudo");
    assert!(!grouped(nickname));
    assert!(crate::prefs::in_category(Category::Storage).all(grouped));
  }

  #[test]
  fn the_storage_rows_follow_riff() {
    let labels: Vec<&str> = crate::prefs::in_category(Category::Storage)
      .map(|entry| entry.label)
      .collect();
    assert_eq!(
      labels,
      [
        "Clear cache",
        "Required tools",
        "Music folder",
        "Download folder",
        "Export all playlists (JSON)",
        "Import playlists (JSON)",
        "Export Riff account (.rfacc)",
        "Import Riff account (.rfacc)",
      ]
    );
  }

  #[test]
  fn the_advanced_rows_of_riff_come_first_in_their_order() {
    let labels: Vec<&str> = crate::prefs::in_category(Category::Advanced)
      .map(|entry| entry.label)
      .collect();
    let riff = [
      "Create desktop & app menu shortcut",
      "YouTube Account",
      "In-app proxy (YouTube / SoundCloud bypass)",
      "Song quality",
      "Minimize to tray on close",
      "Legacy media key capture (Windows)",
      "Allow multiple instances",
      "Online lyrics lookup",
      "Karaoke word highlighting",
      "Auto-scroll synced lyrics",
      "Mesh follows theme intensity",
      "Autoplay similar tracks",
      "My Wave recommendations",
      "Jump back in",
      "Favorite artists",
      "Home layout",
    ];
    assert_eq!(labels[..riff.len()], riff);
  }
}
