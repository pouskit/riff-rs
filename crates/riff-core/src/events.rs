//! Événements diffusés par le core.

use crate::core::{Core, MAX_BODY_BYTES};
use crate::reply::{CoreResult, Params, json_response, text_response};
use serde_json::{Value, json};

/// Événement du core. `/api/lyrics-stream` ne diffuse que l'état de l'application : pas de variante pour les paroles.
#[derive(Debug, Clone, PartialEq)]
pub enum CoreEvent {
  AppState(Value),
  /// Fin d'une connexion lancée par `spotify::start_login` : `error` vide si tout s'est bien passé.
  SpotifyLogin {
    connected: bool,
    error: String,
  },
  /// Fin d'une connexion lancée par `google::start_login`.
  GoogleLogin {
    connected: bool,
    error: String,
  },
}

/// Enregistre l'état de l'application et le diffuse aux abonnés.
pub async fn app_state(core: &Core, params: Params) -> CoreResult {
  let body = &params.body;
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let Ok(text) = std::str::from_utf8(body) else {
    return text_response(400, "");
  };
  let Ok(parsed) = serde_json::from_str::<Value>(text) else {
    return text_response(400, "");
  };
  core.publish(CoreEvent::AppState(parsed.clone()));
  if let Ok(mut guard) = core.playback.current_app_state.lock() {
    *guard = parsed;
  }
  json_response(200, &json!({ "success": true }))
}
