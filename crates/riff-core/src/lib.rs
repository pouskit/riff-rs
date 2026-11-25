//! Logique métier de Riff, sans HTTP.

pub mod adopt;
pub mod config;
pub mod core;
pub mod covers;
pub mod discord;
pub mod discovery;
pub mod dispatch;
pub mod error;
pub mod events;
pub mod import_files;
pub mod library;
pub mod local_media;
pub mod logging;
pub mod maintenance;
pub mod models;
pub mod net;
pub mod os;
pub mod paths;
pub mod privacy;
pub mod reply;
pub mod secrets;
pub mod services;
pub mod storage;
pub mod themes;
pub mod util;

#[cfg(test)]
#[path = "../tests/support/temp.rs"]
mod test_temp;
