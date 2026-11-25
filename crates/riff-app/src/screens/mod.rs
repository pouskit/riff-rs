//! Écrans de l'application.

pub mod album;
pub mod albums;
pub mod artist;
pub mod audio_window;
pub mod backdrop;
pub mod chrome;
pub mod code_editor;
pub mod common;
pub mod compact;
pub mod dev;
pub mod download;
pub mod history;
pub mod home;
pub mod import;
pub mod jam_panel;
pub mod library;
pub mod liked;
pub mod lyrics;
pub mod manage;
pub mod playbar;
pub mod playlist_detail;
pub mod playlists;
pub mod plugins;
pub mod queue;
pub mod queue_panel;
pub mod right_panel;
pub mod search;
pub mod search_history;
pub mod settings;
pub mod settings_system;
pub mod sidebar;
pub mod system;
pub mod theme_editor;
pub mod window_frame;

use crate::app::{App, Message};
use crate::i18n::I18n;
use iced::widget::{container, text};
use iced::{Element, Fill};
use serde::{Deserialize, Serialize};

/// Écran affiché.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Screen {
  #[default]
  Home,
  Search,
  Liked,
  Albums,
  Library,
  History,
  Queue,
  Playlists,
  Settings,
  /// Détail d'une playlist, jamais mémorisé.
  PlaylistDetail,
  /// Page d'album, jamais mémorisée.
  Album,
  /// Page d'artiste, jamais mémorisée.
  Artist,
  /// Écran de développement, derrière `--dev`. Jamais mémorisé.
  Dev,
}

impl Screen {
  /// Entrées `nav-item` de la barre latérale de Riff, dans l'ordre de `index.html`.
  pub const NAV: [Self; 7] = [
    Self::Home,
    Self::Liked,
    Self::Albums,
    Self::Library,
    Self::History,
    Self::Queue,
    Self::Settings,
  ];

  /// Écrans atteints hors de la barre : barre de recherche et tiroir des playlists dans Riff.
  pub const SECONDARY: [Self; 2] = [Self::Search, Self::Playlists];

  /// Titre de l'écran : chaîne anglaise, clé de traduction.
  #[must_use]
  pub const fn title_key(self) -> &'static str {
    match self {
      Self::Home => "Home",
      Self::Search => "Search",
      Self::Liked => "Liked songs",
      Self::Albums => "Albums",
      Self::Library => "My files",
      Self::History => "History",
      Self::Queue => "Queue",
      Self::Playlists => "Playlists",
      Self::Settings => "Settings",
      Self::PlaylistDetail => "Playlist",
      Self::Album => "Album",
      Self::Artist => "Artist",
      Self::Dev => "Dev",
    }
  }

  /// Les pages de détail dépendent de ce qui les a ouvertes : on ne les rouvre pas au lancement.
  #[must_use]
  pub const fn is_remembered(self) -> bool {
    !matches!(
      self,
      Self::Dev | Self::PlaylistDetail | Self::Album | Self::Artist
    )
  }

  /// Vue d'un écran. `Dev` a sa propre vue : `dev::view`.
  #[must_use]
  pub fn view(self, app: &App) -> Element<'_, Message> {
    match self {
      Self::Dev => shell(&app.i18n, self),
      Self::Home => home::view(app),
      Self::Search => search::view(app),
      Self::Liked => liked::view(app),
      Self::Albums => albums::view(app),
      Self::Library => library::view(app),
      Self::History => history::view(app),
      Self::Queue => queue::view(app),
      Self::Playlists => playlists::view(app),
      Self::Settings => settings::view(app),
      Self::PlaylistDetail => playlist_detail::view(app),
      Self::Album => album::view(app),
      Self::Artist => artist::view(app),
    }
  }
}

/// Coquille vide : le titre traduit de l'écran.
pub(crate) fn shell<'a>(i18n: &I18n, screen: Screen) -> Element<'a, Message> {
  container(text(i18n.t(screen.title_key())).size(28))
    .padding(32)
    .width(Fill)
    .height(Fill)
    .into()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::UI_KEYS;

  #[test]
  fn every_title_is_a_translation_key_of_the_shell() {
    for screen in Screen::NAV.into_iter().chain(Screen::SECONDARY) {
      assert!(
        UI_KEYS.contains(&screen.title_key()),
        "{}",
        screen.title_key()
      );
    }
  }

  #[test]
  fn the_sidebar_follows_the_order_of_riff() {
    let titles = Screen::NAV.map(Screen::title_key);
    assert_eq!(
      titles,
      [
        "Home",
        "Liked songs",
        "Albums",
        "My files",
        "History",
        "Queue",
        "Settings"
      ]
    );
  }
}
