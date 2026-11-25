//! Vue « Titres aimés » `#view-favorites` de Riff 44.1.1 (`index.html:634-663`, `app.js:12376-12399`,
//! `styles.css:1483-1493`, `1543-1570`) : l'en-tête de Riff n'a rien de visible (titre, sous-titre et menu de tri
//! sont masqués par `styles.css:1483-1488`).
//!
//! Écart voulu avec Riff (demandé par Hugo) : la page est celle d'une playlist, `playlist_detail::liked_view` : affiche
//! changeable, nom et description éditables, nombre de titres et durée, menu de tri, « Play », « Shuffle »,
//! « Download all » et « Export as JSON », champ de recherche, puis la liste. La page défile d'un bloc.

use crate::app::{App, Message};
use iced::Element;

/// Décalage de la liste dans la page qui défile : celui du détail d'une playlist.
#[must_use]
pub fn list_top() -> f32 {
  super::playlist_detail::list_top()
}

#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  super::playlist_detail::liked_view(app)
}

#[cfg(test)]
mod tests {
  /// La liste commence sous le champ de recherche de l'en-tête de playlist : 480 px à 1920 x 1080, page à 97 px.
  #[test]
  fn the_list_starts_where_a_playlist_list_does() {
    assert!(
      (super::list_top() + 97.0 - 480.0).abs() < 3.0,
      "{}",
      super::list_top()
    );
  }
}
