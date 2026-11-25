//! Plugins Rhai de Riff : en-tête, bac à sable, API hôte et cycle de vie.
//!
//! Aucune dépendance à l'interface : `riff-app` relie [`PluginHost`] au lecteur.

pub mod api;
pub mod consent;
pub mod host;
pub mod manifest;
pub mod runtime;
pub mod sandbox;
pub mod store;
pub mod track;

pub use api::{
  HostCall, HostRequest, HostResponse, Lifecycle, NotifyKind, Outgoing, PickResult, PluginEvent,
};
pub use consent::{Consent, ConsentState};
pub use host::PluginHost;
pub use manifest::{Manifest, ManifestError, Permission, PluginSource};
pub use runtime::threads_spawned;
pub use sandbox::{CompileError, compile};
pub use store::{PluginEntry, PluginKind, Scan};

#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod attack_tests;
#[cfg(test)]
mod examples_tests;
#[cfg(test)]
mod test_support;

#[cfg(test)]
#[path = "../tests/support/temp.rs"]
mod test_temp;
