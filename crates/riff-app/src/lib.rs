//! Application native de Riff, avec iced.

pub mod app;
pub mod audio_prefs;
pub mod audio_sub;
pub mod backend;
pub mod bench;
pub mod code_editor;
pub mod core_sub;
pub mod download_ui;
pub mod hls;
pub mod i18n;
pub mod images;
pub mod import_flow;
pub mod import_report;
pub mod jam;
pub mod languages;
pub mod library;
pub mod lyrics;
pub mod manage;
pub mod motion;
pub mod player;
pub mod plugins;
pub mod prefs;
pub mod prefs_visual;
pub mod resume;
pub mod screens;
pub mod settings;
pub mod splash;
pub mod stream;
pub mod system;
pub mod theme;
pub mod theme_editor;
pub mod track_sort;
pub mod ui;
pub mod update;
pub mod user_data;
pub mod visuals;
pub mod widgets;
pub mod window;

#[cfg(test)]
#[path = "../tests/support/temp.rs"]
mod test_temp;
