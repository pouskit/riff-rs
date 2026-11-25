//! Table des chemins vers les opérations du core, sans serveur HTTP.

use crate::core::Core;
use crate::reply::{CoreResult, Method, Params, text_response};
use crate::services::{
  download, google, imports, lastfm, lyrics, playback, playlist, search, spotify, stream, tools,
  translate,
};
use crate::{covers, discovery, events, library};
use std::sync::Arc;

/// Routes réservées à une méthode : avec une autre, la réponse est un 404.
const GET_ONLY: [&str; 1] = ["/api/audio-output-info"];
const POST_ONLY: [&str; 11] = [
  "/api/state",
  "/api/proxy-config",
  "/api/vibe",
  "/api/library-add",
  "/api/library-update",
  "/api/library-remove",
  "/api/offline-delete",
  "/api/clear-offline-library",
  "/api/translate",
  "/api/i18n-batch",
  "/api/save-custom-lyrics",
];

/// Appelle l'opération du chemin donné. `None` si le chemin n'existe pas.
pub async fn call(
  core: &Arc<Core>,
  method: Method,
  path: &str,
  params: Params,
) -> Option<CoreResult> {
  if (GET_ONLY.contains(&path) && method != Method::Get)
    || (POST_ONLY.contains(&path) && method != Method::Post)
  {
    return Some(text_response(404, "Not found"));
  }
  Some(match path {
    "/api/audio-output-info" => playback::audio_output_info().await,
    "/api/state" => events::app_state(core, params).await,
    "/api/proxy-config" => tools::proxy_config(core, params).await,
    "/api/tmp-file" => download::tmp_file(params).await,
    "/api/brand-icon" => covers::brand_icon(core, params).await,
    "/api/albums/search" => discovery::album_search(core, params).await,
    "/api/albums/tracks" | "/api/album" | "/api/album-tracks" => {
      discovery::album_tracks(core, params).await
    }
    "/api/hot" => discovery::hot_picks(core, params).await,
    "/api/vibe" => discovery::vibe_recommendations(core, params).await,
    "/api/trending" => discovery::trending(core).await,
    "/api/artist" => discovery::artist(core, params).await,
    "/api/check-cache" => library::check_cache(core, params).await,
    "/api/download" => download::download(core, params).await,
    "/api/playlist-import" => playlist::playlist_import(core, params).await,
    "/api/tools-status" | "/api/tools/status" => tools::tools_status(core).await,
    "/api/tools-install" | "/api/tools/install" => tools::tools_install(core).await,
    "/api/proxy-test" => tools::proxy_test(core, method, params).await,
    "/api/local-track" => library::local_track(core, params).await,
    "/api/library-add" => library::library_add(core, params).await,
    "/api/library-update" => library::library_update(core, params).await,
    "/api/library" => library::library(core).await,
    "/api/library-file" => library::library_file(core, params).await,
    "/api/library-remove" => library::library_remove(core, params).await,
    "/api/image-proxy" => covers::image_proxy(core, params).await,
    "/api/local-thumbnail" => covers::local_thumbnail(params).await,
    "/api/offline-library" => library::offline_library(core).await,
    "/api/track-analysis" => library::track_analysis(core, method, params).await,
    "/api/offline-delete" => library::offline_delete(core, params).await,
    "/api/clear-offline-library" => library::clear_offline_library(core).await,
    "/api/translate" => translate::translate(core, params).await,
    "/api/i18n-batch" => translate::i18n_batch(core, params).await,
    "/api/spotify-import" => imports::spotify_import(core, params).await,
    "/api/apple-import" => imports::apple_import(core, params).await,
    "/api/yandex-import" => imports::yandex_import(core, params).await,
    "/api/spotify-auth-start" => spotify::auth_start(core, params).await,
    "/api/spotify-callback" => spotify::callback(core, params).await,
    "/api/spotify-status" => spotify::status(core).await,
    "/api/spotify-logout" => spotify::logout(core).await,
    "/api/spotify-library" => spotify::library(core, params).await,
    "/api/google-status" => google::status(core).await,
    "/api/google-logout" => google::logout(core).await,
    "/api/google-library" => google::library(core, params).await,
    "/api/lyrics" => lyrics::lyrics(core, params).await,
    "/api/lyrics-words" => lyrics::lyrics_words(core, params).await,
    "/api/track-cover" => covers::track_cover(core, params).await,
    "/api/search" => search::search(core, params).await,
    "/api/downloaded-track" => playback::downloaded_track(params).await,
    "/api/stream-info" => stream::stream_info(core, params).await,
    "/api/prefetch" => stream::prefetch(core, params).await,
    "/api/audio-proxy" => playback::audio_proxy(core, method, params).await,
    p if p.starts_with("/api/lastfm/") => lastfm::lastfm(core, method, p, params).await,
    "/api/save-custom-lyrics" => library::save_custom_lyrics(core, params).await,
    _ => return None,
  })
}
