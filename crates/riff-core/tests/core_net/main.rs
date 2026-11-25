//! Tests d'intégration réseau du core : `discovery` et `services`, sans réseau réel.
//!
//! Chaque `Core` reçoit un transport de rejeu. Trois régimes : fixtures enregistrées (`tests/fixtures/http`), aucune
//! fixture (réseau indisponible simulé) et réponses écrites à la main par le test (`common::core_http_500`,
//! `common::core_hand_written`).

#[path = "../support/temp.rs"]
mod temp;

mod common;
mod discovery;
mod lastfm;
mod lyrics;
mod others;
mod playback;
mod playlist;
mod search;
mod soundcloud;
mod translate;
