//! Icônes de Riff : les SVG inline de `index.html` et `app.js`, copiés tels quels dans
//! `assets/icons/` (style Feather, `currentColor`), dessinés avec le rendu SVG d'iced.
//!
//! Une seule icône de Riff est un glyphe Material Symbols : `discover_tune` (préréglages audio,
//! `index.html:504`), servie par une police réduite à ce glyphe.
//!
//! Chaque constante est un point de code de la zone d'usage privé qui sert de clé : [`glyph`],
//! [`glyph_px`] et [`is_icon`] la reconnaissent. La couleur suit celle du texte du parent (un
//! bouton la change au survol et à la désactivation), comme `currentColor` dans Riff.

use crate::theme::{fonts, tokens};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::svg::{self, Svg};
use iced::advanced::widget::{Tree, Widget};
use iced::widget::text;
use iced::{Element, Length, Rectangle, Size, mouse};

pub const PLAY: &str = "\u{f000}";
pub const PAUSE: &str = "\u{f001}";
pub const PREVIOUS: &str = "\u{f002}";
pub const NEXT: &str = "\u{f003}";
pub const SHUFFLE: &str = "\u{f004}";
pub const REPEAT: &str = "\u{f005}";
pub const FAVORITE: &str = "\u{f006}";
/// Cœur plein : `fill="currentColor"` posé sur `#fav-icon` (`app.js:12284`).
pub const FAVORITE_FILLED: &str = "\u{f007}";
pub const EDIT: &str = "\u{f008}";
pub const DOWNLOAD: &str = "\u{f009}";
pub const SHARE: &str = "\u{f00a}";
/// Bouton de la barre de lecture (`btn-toggle-queue-panel`).
pub const QUEUE_LIST: &str = "\u{f00b}";
/// Entrée « File d'attente » de la barre latérale (`nav-queue`).
pub const NAV_QUEUE: &str = "\u{f00c}";
pub const MUSIC_NOTE: &str = "\u{f00d}";
/// Mini-paroles de la barre de titre (`btn-mini-lyrics`), même dessin en 14 px.
pub const MINI_LYRICS: &str = "\u{f00e}";
/// Curseurs de la barre de lecture (`btn-open-settings`).
pub const TUNE: &str = "\u{f00f}";
pub const VOLUME_UP: &str = "\u{f010}";
pub const VOLUME_OFF: &str = "\u{f011}";
pub const SETTINGS: &str = "\u{f012}";
/// Logo de l'application, icône par défaut `02-editor.svg` (`app.js:16652`), couleurs d'origine.
pub const HOME: &str = "\u{f013}";
pub const HISTORY: &str = "\u{f014}";
pub const ALBUM: &str = "\u{f015}";
pub const FOLDER: &str = "\u{f016}";
/// Tiroir des playlists (`btn-toggle-playlists-drawer`).
pub const QUEUE_MUSIC: &str = "\u{f017}";
pub const PLAYLIST_ADD: &str = "\u{f018}";
/// Croix des fenêtres modales (`btn-close-settings`), 16 px.
pub const CLOSE: &str = "\u{f019}";
/// Croix de la barre de titre (`btn-close`), 14 px.
pub const WIN_CLOSE: &str = "\u{f01a}";
pub const MINIMIZE: &str = "\u{f01b}";
pub const MAXIMIZE: &str = "\u{f01c}";
/// Riff n'a pas d'icône de restauration : `btn-maximize` garde le même dessin.
pub const RESTORE: &str = "\u{f01d}";
pub const MINI_PLAYER: &str = "\u{f01e}";
pub const SEARCH: &str = "\u{f01f}";
pub const SEARCH_CLEAR: &str = "\u{f020}";
/// Plus des lignes de piste (`btn-row-add-playlist`).
pub const ADD: &str = "\u{f021}";
/// Croix des lignes de piste (`btn-row-remove-playlist`, `btn-remove-from-queue`).
pub const REMOVE: &str = "\u{f022}";
/// Corbeille des lignes de piste locales (`btn-row-remove-local`).
pub const DELETE: &str = "\u{f023}";
/// Trois points des lignes de piste (`btn-row-more`).
pub const MORE: &str = "\u{f024}";
/// Image des lignes de piste (`btn-row-art`).
pub const IMAGE: &str = "\u{f025}";
pub const SECTION_FOR_YOU: &str = "\u{f026}";
pub const SECTION_HOME: &str = "\u{f027}";
pub const SECTION_TRENDING: &str = "\u{f028}";
pub const SECTION_ARTISTS: &str = "\u{f029}";
/// Pastille « Radio » de la bannière de la barre latérale (`index.html:403`).
pub const RADIO: &str = "\u{f02a}";
/// Épingle des lignes de la bibliothèque (`app.js:27262`).
pub const PIN: &str = "\u{f02b}";
/// Cœur plein de la pochette « Liked songs » de la barre latérale (`app.js:27322`).
pub const LIKED_HEART: &str = "\u{f02c}";
/// Logo `riff.svg` des motifs flottants de la bannière (`styles.css:11304`).
pub const BANNER_LOGO: &str = "\u{f02d}";
/// Boutons de plateforme du haut de fenêtre (`platform-yt`, `platform-ytm`, `platform-sc`, `index.html:369-387`).
pub const PLATFORM_YT: &str = "\u{f02e}";
pub const PLATFORM_YTM: &str = "\u{f02f}";
pub const PLATFORM_SC: &str = "\u{f030}";
/// Boutons de `.right-panel-header-actions` (`index.html:2724-2740`) et pochette vide (`index.html:2750`).
pub const LYRICS_EDIT: &str = "\u{f031}";
pub const LYRICS_SYNC: &str = "\u{f032}";
pub const LYRICS_TRANSLATE: &str = "\u{f033}";
pub const LYRICS_FULLSCREEN: &str = "\u{f034}";
pub const COVER_FALLBACK: &str = "\u{f035}";
/// Actualisation du bouton `#btn-regen-mixes` de « Pour toi » (`index.html:617`).
pub const SP_REFRESH: &str = "\u{f036}";
/// Pochette sans image des cartes de l'accueil (`showCoverPlaceholder`, `app.js:25036`).
pub const SP_COVER_EMPTY: &str = "\u{f037}";
/// Triangle de la pastille d'écoutes des artistes favoris (`app.js:11774`).
pub const SP_PLAYS: &str = "\u{f038}";
/// Pastilles de l'encart d'import (`.sp-promo-ico`, `index.html:603-607`), couleurs de marque d'origine.
pub const PROMO_SPOTIFY: &str = "\u{f039}";
pub const PROMO_YTMUSIC: &str = "\u{f03a}";
pub const PROMO_SOUNDCLOUD: &str = "\u{f03b}";
pub const PROMO_APPLE: &str = "\u{f03c}";
pub const PROMO_YANDEX: &str = "\u{f03d}";
/// Croix d'une ligne de l'historique de recherche (`.sh-remove`, `app.js:10211`).
pub const SH_REMOVE: &str = "\u{f03e}";
/// Silhouette d'une ligne « artiste » sans image (`fallbackSvg`, `app.js:10170`).
pub const SH_ARTIST: &str = "\u{f03f}";
/// Pastille `verified-check` (`app.js:10023`) : disque, puis coche de la couleur `on-primary`.
pub const VERIFIED_DISC: &str = "\u{f040}";
pub const VERIFIED_TICK: &str = "\u{f041}";
/// Note de musique d'une vignette ou d'une pochette de recherche absente (`track-row-fallback-icon`, `app.js:11206`, `9876`).
pub const ROW_FALLBACK: &str = "\u{f042}";
/// Icône d'erreur du chargeur de recherche (`.m3-search-error-icon`, `app.js:9923`).
pub const SEARCH_ERROR: &str = "\u{f043}";
/// Dossier de la pochette « My files » de la barre latérale (`app.js:27370`) : sans bouts arrondis, contrairement à `nav-local`.
pub const SB_FOLDER: &str = "\u{f044}";
/// Dossier avec « + » du bouton « Add folder » de « My files » (`index.html:673`, `app.js:12799`).
pub const LOCAL_ADD_FOLDER: &str = "\u{f045}";
/// Flèche de `#btn-back-album` (`index.html:772`).
pub const ALBUM_BACK: &str = "\u{f080}";
/// Pouces de `#btn-dm-like` et `#btn-dm-dislike` (`index.html:779`).
pub const MIX_LIKE: &str = "\u{f081}";
pub const MIX_DISLIKE: &str = "\u{f082}";
/// Détail de playlist (`index.html:717-760`) : flèche de retour, flèches du menu de tri, lecture, téléchargement, export et corbeille.
pub const PL_BACK: &str = "\u{f070}";
pub const PL_CHEVRON_DOWN: &str = "\u{f071}";
pub const PL_CHEVRON_UP: &str = "\u{f072}";
pub const PL_PLAY: &str = "\u{f073}";
pub const PL_DOWNLOAD: &str = "\u{f074}";
pub const PL_EXPORT: &str = "\u{f075}";
pub const PL_DELETE: &str = "\u{f076}";
/// Flèche du bouton retour de la page artiste (`#btn-back-artist`, `index.html:793`).
pub const ARTIST_BACK: &str = "\u{f090}";
/// Quatre carrés de l'onglet « Albums » de la page artiste (`index.html:831`).
pub const ARTIST_TAB_ALBUMS: &str = "\u{f091}";
/// Entrées du menu contextuel d'un titre (`app.js:10710-10850`), SVG de 20 px (`.riff-ctx-item svg`, `styles.css:10058-10064`).
pub const CTX_FAV: &str = "\u{f0c0}";
pub const CTX_FAV_FILLED: &str = "\u{f0c1}";
pub const CTX_PLAYLIST: &str = "\u{f0c2}";
pub const CTX_QUEUE: &str = "\u{f0c3}";
pub const CTX_PIN: &str = "\u{f0c4}";
pub const CTX_PIN_FILLED: &str = "\u{f0c5}";
pub const CTX_SHARE: &str = "\u{f0c6}";
pub const CTX_DOWNLOAD: &str = "\u{f0c7}";
pub const CTX_BANNER: &str = "\u{f0c8}";
pub const CTX_RENAME: &str = "\u{f0c9}";
pub const CTX_TRASH: &str = "\u{f0ca}";
pub const CTX_X: &str = "\u{f0cb}";
pub const CTX_BLOCK: &str = "\u{f0cc}";
pub const CTX_BLOCK_ARTIST: &str = "\u{f0cd}";
/// Réglages : catégories de `.settings-nav-btn` (`index.html:854-936`), recherche, fermeture et flèche des listes.
pub const SET_INTERFACE: &str = "\u{f0e0}";
pub const SET_THEME: &str = "\u{f0e1}";
pub const SET_BANNERS: &str = "\u{f0e2}";
pub const SET_TRACKS: &str = "\u{f0e3}";
pub const SET_INTEGRATIONS: &str = "\u{f0e4}";
pub const SET_STORAGE: &str = "\u{f0e5}";
pub const SET_ADVANCED: &str = "\u{f0e6}";
pub const SET_ABOUT: &str = "\u{f0e7}";
pub const SET_PLUGINS: &str = "\u{f0e8}";
pub const SET_SEARCH: &str = "\u{f0e9}";
pub const SET_CLOSE: &str = "\u{f0ea}";
pub const SET_ARROW: &str = "\u{f0eb}";
/// Bouton « Jam » de la barre de lecture.
pub const JAM: &str = "\u{f0ec}";
/// Seul glyphe Material Symbols : préréglages audio du bas de la barre latérale.
pub const DISCOVER_TUNE: &str = "\u{e018}";
/// Taille du glyphe `discover_tune` dans Riff (`index.html:504`), en pixels CSS.
const DISCOVER_TUNE_PX: f32 = 22.0;

/// Une icône SVG : octets du fichier, taille CSS d'origine, couleur du thème ou couleurs propres.
struct Spec {
  key: &'static str,
  svg: &'static [u8],
  px: f32,
  tinted: bool,
}

macro_rules! spec {
  ($key:expr, $file:literal, $px:expr) => {
    Spec {
      key: $key,
      svg: include_bytes!(concat!("../../assets/icons/", $file, ".svg")),
      px: $px,
      tinted: true,
    }
  };
}

/// Tailles d'après l'attribut `width` du SVG d'origine.
const SPECS: &[Spec] = &[
  spec!(PLAY, "play", 22.0),
  spec!(PAUSE, "pause", 22.0),
  spec!(PREVIOUS, "prev", 20.0),
  spec!(NEXT, "next", 20.0),
  spec!(SHUFFLE, "shuffle", 18.0),
  spec!(REPEAT, "repeat", 18.0),
  spec!(FAVORITE, "fav", 18.0),
  spec!(FAVORITE_FILLED, "fav-filled", 18.0),
  spec!(EDIT, "edit-track", 15.0),
  spec!(DOWNLOAD, "download", 18.0),
  spec!(SHARE, "share", 17.0),
  spec!(QUEUE_LIST, "queue-panel", 18.0),
  spec!(NAV_QUEUE, "nav-queue", 20.0),
  spec!(MUSIC_NOTE, "lyrics-panel", 18.0),
  spec!(MINI_LYRICS, "win-mini-lyrics", 14.0),
  spec!(TUNE, "audio", 18.0),
  spec!(VOLUME_UP, "volume", 18.0),
  spec!(VOLUME_OFF, "volume-muted", 18.0),
  spec!(SETTINGS, "nav-settings", 20.0),
  Spec {
    key: HOME,
    svg: include_bytes!("../../assets/icons/nav-home.svg"),
    px: 48.0,
    tinted: false,
  },
  spec!(HISTORY, "nav-history", 20.0),
  spec!(ALBUM, "nav-albums", 20.0),
  spec!(FOLDER, "nav-local", 20.0),
  spec!(QUEUE_MUSIC, "rail-playlists", 20.0),
  spec!(PLAYLIST_ADD, "sb-new-playlist", 20.0),
  spec!(CLOSE, "close", 16.0),
  spec!(WIN_CLOSE, "win-close", 14.0),
  spec!(MINIMIZE, "win-minimize", 14.0),
  spec!(MAXIMIZE, "win-maximize", 14.0),
  spec!(RESTORE, "win-maximize", 14.0),
  spec!(MINI_PLAYER, "win-mini-player", 14.0),
  spec!(SEARCH, "search", 18.0),
  spec!(SEARCH_CLEAR, "search-clear", 14.0),
  spec!(ADD, "row-add", 16.0),
  spec!(REMOVE, "row-remove", 16.0),
  spec!(DELETE, "row-delete", 16.0),
  spec!(MORE, "row-more", 16.0),
  spec!(IMAGE, "row-art", 16.0),
  spec!(SECTION_FOR_YOU, "sp-made", 20.0),
  spec!(SECTION_HOME, "sp-quick", 20.0),
  spec!(SECTION_TRENDING, "sp-trend", 20.0),
  spec!(SECTION_ARTISTS, "sp-artists", 20.0),
  spec!(RADIO, "radio", 14.0),
  spec!(PIN, "sb-pin", 12.0),
  spec!(LIKED_HEART, "sb-liked", 22.0),
  spec!(BANNER_LOGO, "riff-logo", 28.0),
  spec!(PLATFORM_YT, "platform-yt", 16.0),
  spec!(PLATFORM_YTM, "platform-ytm", 16.0),
  spec!(PLATFORM_SC, "platform-sc", 16.0),
  spec!(LYRICS_EDIT, "rp-edit", 16.0),
  spec!(LYRICS_SYNC, "rp-sync", 16.0),
  spec!(LYRICS_TRANSLATE, "rp-translate", 16.0),
  spec!(LYRICS_FULLSCREEN, "rp-fullscreen", 16.0),
  spec!(COVER_FALLBACK, "rp-cover-fallback", 36.0),
  spec!(SP_REFRESH, "sp-refresh", 16.0),
  spec!(SP_COVER_EMPTY, "sp-cover-empty", 24.0),
  spec!(SP_PLAYS, "sp-plays", 10.0),
  spec!(SH_REMOVE, "sh-remove", 14.0),
  spec!(SH_ARTIST, "sh-artist", 18.0),
  spec!(VERIFIED_DISC, "verified-disc", 14.0),
  spec!(VERIFIED_TICK, "verified-tick", 14.0),
  spec!(ROW_FALLBACK, "row-fallback", 20.0),
  spec!(SEARCH_ERROR, "search-error", 34.0),
  spec!(SB_FOLDER, "sb-folder", 22.0),
  spec!(LOCAL_ADD_FOLDER, "local-add-folder", 16.0),
  spec!(ALBUM_BACK, "album-back", 18.0),
  spec!(MIX_LIKE, "mix-like", 16.0),
  spec!(MIX_DISLIKE, "mix-dislike", 16.0),
  spec!(PL_BACK, "pl-back", 18.0),
  spec!(PL_CHEVRON_DOWN, "pl-chevron-down", 18.0),
  spec!(PL_CHEVRON_UP, "pl-chevron-up", 18.0),
  spec!(PL_PLAY, "pl-play", 16.0),
  spec!(PL_DOWNLOAD, "pl-download", 18.0),
  spec!(PL_EXPORT, "pl-export", 18.0),
  spec!(PL_DELETE, "pl-delete", 18.0),
  spec!(ARTIST_BACK, "artist-back", 18.0),
  spec!(ARTIST_TAB_ALBUMS, "artist-tab-albums", 20.0),
  spec!(CTX_FAV, "ctx-fav", 20.0),
  spec!(CTX_FAV_FILLED, "ctx-fav-filled", 20.0),
  spec!(CTX_PLAYLIST, "ctx-playlist", 20.0),
  spec!(CTX_QUEUE, "ctx-queue", 20.0),
  spec!(CTX_PIN, "ctx-pin", 20.0),
  spec!(CTX_PIN_FILLED, "ctx-pin-filled", 20.0),
  spec!(CTX_SHARE, "ctx-share", 20.0),
  spec!(CTX_DOWNLOAD, "ctx-download", 20.0),
  spec!(CTX_BANNER, "ctx-banner", 20.0),
  spec!(CTX_RENAME, "ctx-rename", 20.0),
  spec!(CTX_TRASH, "ctx-trash", 20.0),
  spec!(CTX_X, "ctx-x", 20.0),
  spec!(CTX_BLOCK, "ctx-block", 20.0),
  spec!(CTX_BLOCK_ARTIST, "ctx-block-artist", 20.0),
  spec!(SET_INTERFACE, "set-interface", 20.0),
  spec!(SET_THEME, "set-theme", 20.0),
  spec!(SET_BANNERS, "set-banners", 20.0),
  spec!(SET_TRACKS, "set-tracks", 20.0),
  spec!(SET_INTEGRATIONS, "set-integrations", 20.0),
  spec!(SET_STORAGE, "set-storage", 20.0),
  spec!(SET_ADVANCED, "set-advanced", 20.0),
  spec!(SET_ABOUT, "set-about", 20.0),
  spec!(SET_PLUGINS, "set-plugins", 20.0),
  spec!(SET_SEARCH, "set-search", 18.0),
  spec!(SET_CLOSE, "set-close", 18.0),
  spec!(SET_ARROW, "set-arrow", 18.0),
  spec!(JAM, "jam", 18.0),
  Spec {
    key: PROMO_SPOTIFY,
    svg: include_bytes!("../../assets/icons/promo-spotify.svg"),
    px: 20.0,
    tinted: false,
  },
  Spec {
    key: PROMO_YTMUSIC,
    svg: include_bytes!("../../assets/icons/promo-ytmusic.svg"),
    px: 20.0,
    tinted: false,
  },
  Spec {
    key: PROMO_SOUNDCLOUD,
    svg: include_bytes!("../../assets/icons/promo-soundcloud.svg"),
    px: 20.0,
    tinted: false,
  },
  Spec {
    key: PROMO_APPLE,
    svg: include_bytes!("../../assets/icons/promo-apple.svg"),
    px: 18.0,
    tinted: false,
  },
  Spec {
    key: PROMO_YANDEX,
    svg: include_bytes!("../../assets/icons/promo-yandex.svg"),
    px: 18.0,
    tinted: false,
  },
];

fn spec(glyph: &str) -> Option<&'static Spec> {
  SPECS.iter().find(|spec| spec.key == glyph)
}

/// Vrai si `glyph` est une icône de ce module (zone d'usage privé).
#[must_use]
pub fn is_icon(glyph: &str) -> bool {
  let mut chars = glyph.chars();
  matches!(
    (chars.next(), chars.next()),
    (Some('\u{e000}'..='\u{f8ff}'), None)
  )
}

/// Clés de toutes les icônes SVG.
pub fn all() -> impl Iterator<Item = &'static str> {
  SPECS.iter().map(|spec| spec.key)
}

/// Taille CSS d'origine de l'icône dans Riff, ou `None` pour un libellé ordinaire.
#[must_use]
pub fn css_size(glyph: &str) -> Option<f32> {
  if glyph == DISCOVER_TUNE {
    return Some(DISCOVER_TUNE_PX);
  }
  spec(glyph).map(|spec| spec.px)
}

/// Dessin SVG teinté par la couleur du texte du parent.
struct SvgIcon {
  handle: svg::Handle,
  size: f32,
  tinted: bool,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for SvgIcon
where
  Renderer: svg::Renderer,
{
  fn size(&self) -> Size<Length> {
    Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
  }

  fn layout(
    &mut self,
    _tree: &mut Tree,
    _renderer: &Renderer,
    _limits: &layout::Limits,
  ) -> layout::Node {
    layout::Node::new(Size::new(self.size, self.size))
  }

  fn draw(
    &self,
    _tree: &Tree,
    renderer: &mut Renderer,
    _theme: &Theme,
    style: &renderer::Style,
    layout: Layout<'_>,
    _cursor: mouse::Cursor,
    _viewport: &Rectangle,
  ) {
    let bounds = layout.bounds();
    let mut image = Svg::new(self.handle.clone());
    if self.tinted {
      // Le rendu SVG ne garde que l'alpha du dessin : l'opacité de la couleur passe à part.
      image = image
        .color(iced::Color {
          a: 1.0,
          ..style.text_color
        })
        .opacity(style.text_color.a);
    }
    renderer.draw_svg(image, bounds, bounds);
  }
}

/// Icône ou libellé à `css_px` pixels CSS de Riff (converti en pixels écran), `text_size` pour un
/// libellé ordinaire.
fn render<'a, Message: 'a>(
  content: &'a str,
  css_px: Option<f32>,
  text_size: f32,
) -> Element<'a, Message> {
  if content == DISCOVER_TUNE {
    return text(content)
      .font(fonts::ICONS)
      .size(tokens::css(css_px.unwrap_or(DISCOVER_TUNE_PX)))
      .into();
  }
  match spec(content) {
    Some(spec) => Element::new(SvgIcon {
      handle: svg::Handle::from_memory(spec.svg),
      size: tokens::css(css_px.unwrap_or(spec.px)),
      tinted: spec.tinted,
    }),
    None => text(content).size(text_size).into(),
  }
}

/// Icône à sa taille d'origine dans Riff, ou libellé ordinaire (`text_size`).
#[must_use]
pub fn glyph<'a, Message: 'a>(content: &'a str, text_size: f32) -> Element<'a, Message> {
  render(content, None, text_size)
}

/// Icône à `css_px` pixels CSS (taille du SVG d'origine selon l'endroit où Riff l'affiche).
#[must_use]
pub fn glyph_px<'a, Message: 'a>(content: &'a str, css_px: f32) -> Element<'a, Message> {
  render(content, Some(css_px), css_px)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_private_use_single_characters_are_icons() {
    assert!(is_icon(PLAY) && is_icon(IMAGE));
    assert!(!is_icon("+") && !is_icon("") && !is_icon("ab") && !is_icon("\u{2665}"));
  }

  #[test]
  fn every_key_is_unique_and_every_svg_parses_to_its_riff_size() {
    for (index, spec) in SPECS.iter().enumerate() {
      assert!(
        SPECS[index + 1..].iter().all(|other| other.key != spec.key),
        "clé en double : {:?}",
        spec.key
      );
      let text = std::str::from_utf8(spec.svg).expect("SVG en UTF-8");
      assert!(text.starts_with("<svg"), "{:?}", spec.key);
      assert!(is_icon(spec.key));
    }
    assert_eq!(DISCOVER_TUNE, "\u{e018}");
  }
}
