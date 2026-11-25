//! Composants de base : focus clavier visible, Tab, Entrée, Espace et Échap sur chacun.

pub mod button;
pub mod dialog;
pub mod focus;
pub mod hover;
pub mod inert;
pub mod list;
pub mod menu;
pub mod reveal;
pub mod spinner;
pub mod text_field;
pub mod ticker;
pub mod toast;
pub mod touch;
pub mod view_transition;
pub mod wheel;

/// Conteneur de la superposition ouverte (menu ou dialogue) : la tabulation y reste enfermée.
pub const OVERLAY_ID: &str = "riff-overlay";
