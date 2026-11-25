use super::*;
use crate::app::Message;
use crate::i18n::I18n;
use crate::ui::UiMessage;
use crate::user_data::Playlist;
use crate::window::PersistedWindowState;
use riff_core::events::CoreEvent;
use riff_core::import_files::{ImportError, ImportedPlaylist, ImportedTrack};

fn app() -> App {
  App::new(
    PersistedWindowState::default(),
    NativeSettings::default(),
    I18n::default(),
  )
}

fn send(app: &mut App, message: ManageMessage) -> Vec<Effect> {
  app.update(Message::Manage(message))
}

fn reply(app: &mut App, reply: ManageReply, value: Value) -> Vec<Effect> {
  app.update(Message::Ui(UiMessage::Reply(
    ReplyTag::Manage(reply),
    Ok(value),
  )))
}

fn failed(app: &mut App, reply: ManageReply, error: &str) -> Vec<Effect> {
  app.update(Message::Ui(UiMessage::Reply(
    ReplyTag::Manage(reply),
    Err(error.to_string()),
  )))
}

fn status(ytdlp: bool, ffmpeg: bool, state: &str, tool: &str, percent: u32, error: &str) -> Value {
  json!({
    "ytdlp": ytdlp,
    "ffmpeg": ffmpeg,
    "ffmpegOptional": false,
    "progress": { "status": state, "tool": tool, "percent": percent, "error": error, "ffmpegError": "" }
  })
}

fn import_body(names: &[&str]) -> Value {
  let tracks: Vec<Value> = names
    .iter()
    .map(|name| json!({ "id": name, "title": name, "artist": "A" }))
    .collect();
  json!({ "success": true, "name": "Ma liste", "cover": "https://i.ytimg.com/a.jpg", "tracks": tracks })
}

fn core_path(effects: &[Effect]) -> Option<&'static str> {
  effects.iter().find_map(|effect| match effect {
    Effect::CoreJson { path, .. } => Some(*path),
    _ => None,
  })
}

#[test]
fn the_status_of_the_core_is_read() {
  let read = ToolsStatus::parse(&status(true, false, "downloading", "ffmpeg", 42, ""));
  assert!(read.ytdlp);
  assert!(!read.ffmpeg);
  assert_eq!(read.tool, "ffmpeg");
  assert_eq!(read.percent, 42);
  assert!(!read.is_ready());
  assert_eq!(read.progress_of("ffmpeg", read.ffmpeg), 42);
  assert_eq!(read.progress_of("yt-dlp", read.ytdlp), 100);
}

#[test]
fn ffmpeg_is_optional_when_the_core_says_so() {
  let read = ToolsStatus {
    ytdlp: true,
    ffmpeg_optional: true,
    ..ToolsStatus::default()
  };
  assert!(read.is_ready());
}

#[test]
fn a_missing_tool_opens_the_window_at_startup() {
  let mut app = app();
  assert_eq!(core_path(&app.startup_tools()), Some("/api/tools-status"));
  reply(
    &mut app,
    ManageReply::ToolsStatus,
    status(false, true, "idle", "", 0, ""),
  );
  assert_eq!(app.ui.modal, Some(Modal::Tools));
}

#[test]
fn no_window_when_every_tool_is_there() {
  let mut app = app();
  reply(
    &mut app,
    ManageReply::ToolsStatus,
    status(true, true, "idle", "", 0, ""),
  );
  assert_eq!(app.ui.modal, None);
}

#[test]
fn installing_polls_until_the_end() {
  let mut app = app();
  let effects = send(&mut app, ManageMessage::InstallTools);
  assert_eq!(core_path(&effects), Some("/api/tools-install"));
  assert!(app.ui.manage.tools.installing);
  reply(
    &mut app,
    ManageReply::ToolsStatus,
    status(false, false, "downloading", "yt-dlp", 10, ""),
  );
  assert!(app.ui.manage.tools.installing);
  assert_eq!(app.ui.manage.tools.status.percent, 10);
  reply(
    &mut app,
    ManageReply::ToolsStatus,
    status(true, true, "done", "", 100, ""),
  );
  assert!(!app.ui.manage.tools.installing);
}

#[test]
fn an_error_stops_the_polling_and_a_retry_clears_it() {
  let mut app = app();
  send(&mut app, ManageMessage::InstallTools);
  reply(
    &mut app,
    ManageReply::ToolsStatus,
    status(false, false, "error", "yt-dlp", 0, "réseau coupé"),
  );
  assert!(!app.ui.manage.tools.installing);
  assert_eq!(app.ui.manage.tools.status.failure(), Some("réseau coupé"));
  send(&mut app, ManageMessage::InstallTools);
  assert!(app.ui.manage.tools.installing);
  assert_eq!(app.ui.manage.tools.status.failure(), None);
}

#[test]
fn a_failed_install_call_is_reported() {
  let mut app = app();
  send(&mut app, ManageMessage::InstallTools);
  failed(&mut app, ManageReply::ToolsStarted, "réponse 500");
  assert!(!app.ui.manage.tools.installing);
  assert_eq!(app.ui.manage.tools.status.failure(), Some("réponse 500"));
}

#[test]
fn each_import_uses_its_route() {
  for (kind, route) in [
    (ImportKind::Spotify, "/api/spotify-import"),
    (ImportKind::Apple, "/api/apple-import"),
    (ImportKind::Link, "/api/playlist-import"),
    (ImportKind::Yandex, "/api/yandex-import"),
  ] {
    let mut app = app();
    send(&mut app, ManageMessage::OpenImport(kind));
    assert_eq!(app.ui.modal, Some(Modal::Import(kind)));
    send(
      &mut app,
      ManageMessage::ImportUrl(" https://a ".to_string()),
    );
    let effects = send(&mut app, ManageMessage::ImportStart(kind));
    assert_eq!(core_path(&effects), Some(route));
    assert!(
      matches!(&effects[0], Effect::CoreJson { query, .. } if query[0] == ("url".to_string(), "https://a".to_string()))
    );
    assert_eq!(app.ui.manage.import.progress, Progress::Running);
  }
}

#[test]
fn an_empty_link_starts_nothing() {
  let mut app = app();
  send(&mut app, ManageMessage::OpenImport(ImportKind::Link));
  assert!(send(&mut app, ManageMessage::ImportStart(ImportKind::Link)).is_empty());
}

#[test]
fn an_import_creates_a_playlist_by_default() {
  let mut app = app();
  send(&mut app, ManageMessage::OpenImport(ImportKind::Link));
  send(&mut app, ManageMessage::ImportUrl("https://a".to_string()));
  send(&mut app, ManageMessage::ImportStart(ImportKind::Link));
  reply(
    &mut app,
    ManageReply::Import(ImportKind::Link),
    import_body(&["a", "b"]),
  );
  assert_eq!(app.data.playlists.len(), 1);
  assert_eq!(app.data.playlists[0].name.as_deref(), Some("Ma liste"));
  assert_eq!(app.data.playlists[0].tracks.len(), 2);
  assert_eq!(
    app.data.playlists[0].cover.as_deref(),
    Some("https://i.ytimg.com/a.jpg")
  );
  assert_eq!(
    app.ui.manage.import.progress,
    Progress::Done("2 tracks".to_string())
  );
}

#[test]
fn an_import_into_favorites_skips_what_is_already_there() {
  let mut app = app();
  send(&mut app, ManageMessage::OpenImport(ImportKind::Link));
  send(
    &mut app,
    ManageMessage::ImportTarget(FAVORITES_TARGET.to_string()),
  );
  reply(
    &mut app,
    ManageReply::Import(ImportKind::Link),
    import_body(&["a"]),
  );
  reply(
    &mut app,
    ManageReply::Import(ImportKind::Link),
    import_body(&["a", "b"]),
  );
  assert_eq!(app.data.favorites.len(), 2);
  assert!(app.data.playlists.is_empty());
}

#[test]
fn an_import_into_an_existing_playlist_adds_only_new_tracks() {
  let mut app = app();
  app.data.playlists.push(Playlist {
    id: Some("pl".to_string()),
    name: Some("P".to_string()),
    ..Playlist::default()
  });
  send(&mut app, ManageMessage::OpenImport(ImportKind::Link));
  send(&mut app, ManageMessage::ImportTarget("pl".to_string()));
  reply(
    &mut app,
    ManageReply::Import(ImportKind::Link),
    import_body(&["a"]),
  );
  reply(
    &mut app,
    ManageReply::Import(ImportKind::Link),
    import_body(&["a", "b"]),
  );
  assert_eq!(app.data.playlists.len(), 1);
  assert_eq!(app.data.playlists[0].tracks.len(), 2);
}

#[test]
fn a_refused_import_shows_the_error_of_the_core_without_correcting_it() {
  let mut app = app();
  send(&mut app, ManageMessage::OpenImport(ImportKind::Yandex));
  reply(
    &mut app,
    ManageReply::Import(ImportKind::Yandex),
    json!({ "success": false, "error": "Yandex answered 451", "status": 451 }),
  );
  assert_eq!(
    app.ui.manage.import.progress,
    Progress::Failed("Yandex answered 451".to_string())
  );
  assert!(app.data.playlists.is_empty());
}

#[test]
fn an_import_without_tracks_is_a_failure() {
  let mut app = app();
  send(&mut app, ManageMessage::OpenImport(ImportKind::Link));
  reply(
    &mut app,
    ManageReply::Import(ImportKind::Link),
    json!({ "success": true, "tracks": [] }),
  );
  assert_eq!(
    app.ui.manage.import.progress,
    Progress::Failed("Failed to import playlist".to_string())
  );
  failed(&mut app, ManageReply::Import(ImportKind::Link), "réseau");
  assert_eq!(
    app.ui.manage.import.progress,
    Progress::Failed("réseau".to_string())
  );
}

#[test]
fn a_json_file_of_playlists_is_read_in_both_forms() {
  let wrapped =
    r#"{ "playlists": [{ "id": "a", "name": "A", "tracks": [{ "id": "t", "title": "T" }] }] }"#;
  assert_eq!(playlists_from_json(wrapped).len(), 1);
  assert_eq!(
    playlists_from_json(r#"[{ "id": "a", "name": "A" }]"#).len(),
    1
  );
  assert!(playlists_from_json("pas du json").is_empty());
  assert!(playlists_from_json("{}").is_empty());
}

#[test]
fn importing_a_json_file_creates_its_playlists() {
  let mut app = app();
  let raw = r#"[{ "id": "a", "name": "A", "tracks": [{ "id": "t", "title": "T" }] }]"#.to_string();
  send(&mut app, ManageMessage::ImportJsonRead(Ok(Some(raw))));
  assert_eq!(app.data.playlists.len(), 1);
  assert_eq!(app.data.playlists[0].name.as_deref(), Some("A"));
  let effects = send(
    &mut app,
    ManageMessage::ImportJsonRead(Ok(Some("{}".to_string()))),
  );
  assert!(matches!(effects[0], Effect::Toast(_, ToastKind::Error)));
  assert!(send(&mut app, ManageMessage::ImportJsonRead(Ok(None))).is_empty());
}

#[test]
fn the_spotify_login_is_started_with_the_client_id_and_opens_the_browser() {
  let mut app = app();
  send(
    &mut app,
    ManageMessage::SpotifyClientId(" 0123456789abcdef ".to_string()),
  );
  let effects = send(&mut app, ManageMessage::SpotifyConnect);
  assert_eq!(
    effects,
    [Effect::SpotifyLogin("0123456789abcdef".to_string())]
  );
  let effects = send(
    &mut app,
    ManageMessage::SpotifyStarted(Ok((
      "https://accounts.spotify.com/x".to_string(),
      "http://127.0.0.1:5/api/spotify-callback".to_string(),
    ))),
  );
  assert_eq!(
    effects,
    [Effect::OpenUrl(
      "https://accounts.spotify.com/x".to_string()
    )]
  );
  assert_eq!(app.ui.manage.spotify.progress, Progress::Running);
}

#[test]
fn a_refused_client_id_is_shown() {
  let mut app = app();
  send(
    &mut app,
    ManageMessage::SpotifyStarted(Err("Invalid Client ID".to_string())),
  );
  assert_eq!(
    app.ui.manage.spotify.progress,
    Progress::Failed("Invalid Client ID".to_string())
  );
}

#[test]
fn the_end_of_the_spotify_login_arrives_by_the_core_event() {
  let mut app = app();
  send(&mut app, ManageMessage::SpotifyConnect);
  app.update(Message::Core(CoreEvent::SpotifyLogin {
    connected: true,
    error: String::new(),
  }));
  assert!(app.ui.manage.spotify.connected);
  assert_eq!(app.ui.manage.spotify.progress, Progress::Idle);
  app.update(Message::Core(CoreEvent::SpotifyLogin {
    connected: false,
    error: "refusé".to_string(),
  }));
  assert!(!app.ui.manage.spotify.connected);
  assert_eq!(
    app.ui.manage.spotify.progress,
    Progress::Failed("refusé".to_string())
  );
}

#[test]
fn the_spotify_logout_uses_the_core() {
  let mut app = app();
  app.ui.manage.spotify.connected = true;
  assert_eq!(
    core_path(&send(&mut app, ManageMessage::SpotifyLogout)),
    Some("/api/spotify-logout")
  );
  reply(
    &mut app,
    ManageReply::SpotifyLogout,
    json!({ "success": true }),
  );
  assert!(!app.ui.manage.spotify.connected);
}

#[test]
fn a_library_adds_likes_and_playlists() {
  let mut app = app();
  send(&mut app, ManageMessage::SpotifyImportLibrary);
  let body = json!({
    "likes": [{ "id": "a", "title": "A" }],
    "playlists": [{ "name": "P", "tracks": [{ "id": "b", "title": "B" }] }, { "name": "Vide", "tracks": [] }]
  });
  reply(&mut app, ManageReply::SpotifyLibrary, body);
  assert_eq!(app.data.favorites.len(), 1);
  assert_eq!(app.data.playlists.len(), 1);
  assert_eq!(
    app.ui.manage.spotify.progress,
    Progress::Done("1 liked tracks, 1 playlists".to_string())
  );
}

#[test]
fn google_stays_disabled_without_a_client_id() {
  let mut app = app();
  reply(
    &mut app,
    ManageReply::GoogleStatus,
    json!({ "enabled": false, "connected": false }),
  );
  assert!(!app.ui.manage.google.enabled);
  assert!(send(&mut app, ManageMessage::GoogleConnect).is_empty());
}

#[test]
fn a_vault_error_in_the_google_status_is_shown() {
  let mut app = app();
  reply(
    &mut app,
    ManageReply::GoogleStatus,
    json!({ "enabled": true, "connected": false, "error": "Secure storage is unavailable. Google login could not be loaded." }),
  );
  assert_eq!(
    app.ui.manage.google.progress,
    Progress::Failed(
      "Secure storage is unavailable. Google login could not be loaded.".to_string()
    )
  );
}

#[test]
fn a_lastfm_vault_failure_raises_an_error_toast() {
  let mut app = app();
  for (item, text) in [
    (
      LastfmVaultItem::ApiSecret,
      "Last.fm API secret could not be saved securely; enter it again after restarting Riff.",
    ),
    (
      LastfmVaultItem::Session,
      "Last.fm login could not be saved securely; reconnect after restarting Riff.",
    ),
  ] {
    assert_eq!(
      send(&mut app, ManageMessage::LastfmVaultFailed(item)),
      [Effect::Toast(text.to_string(), ToastKind::Error)]
    );
  }
}

#[test]
fn the_lastfm_vault_messages_are_translated() {
  for lang in ["ru", "uk"] {
    let i18n = I18n::load(lang, &std::env::temp_dir().join("riff-app-i18n-absent"));
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      i18n,
    );
    for item in [LastfmVaultItem::ApiSecret, LastfmVaultItem::Session] {
      let effects = send(&mut app, ManageMessage::LastfmVaultFailed(item));
      let [Effect::Toast(text, _)] = &effects[..] else {
        panic!("un toast attendu");
      };
      assert!(!text.starts_with("Last.fm"), "{lang}: {text}");
    }
  }
}

#[test]
fn google_connects_when_a_client_id_is_there() {
  let mut app = app();
  reply(
    &mut app,
    ManageReply::GoogleStatus,
    json!({ "enabled": true, "connected": false }),
  );
  assert_eq!(
    send(&mut app, ManageMessage::GoogleConnect),
    [Effect::GoogleLogin]
  );
  assert_eq!(
    send(
      &mut app,
      ManageMessage::GoogleStarted(Ok("https://accounts.google.com/x".to_string()))
    ),
    [Effect::OpenUrl("https://accounts.google.com/x".to_string())]
  );
  app.update(Message::Core(CoreEvent::GoogleLogin {
    connected: true,
    error: String::new(),
  }));
  assert!(app.ui.manage.google.connected);
}

#[test]
fn the_google_library_asks_for_the_chosen_platform_and_shows_the_warning() {
  let mut app = app();
  send(&mut app, ManageMessage::GoogleMusic(true));
  let effects = send(&mut app, ManageMessage::GoogleImportLibrary);
  assert!(
    matches!(&effects[0], Effect::CoreJson { path: "/api/google-library", query, .. } if query[0] == ("platform".to_string(), "ytmusic".to_string()))
  );
  reply(
    &mut app,
    ManageReply::GoogleLibrary,
    json!({ "success": true, "likes": [], "playlists": [], "warning": "1,000" }),
  );
  assert_eq!(
    app.ui.manage.google.progress,
    Progress::Done("1,000".to_string())
  );
  failed(&mut app, ManageReply::GoogleLibrary, "quota");
  assert_eq!(
    app.ui.manage.google.progress,
    Progress::Failed("quota".to_string())
  );
}

#[test]
fn the_lastfm_login_opens_the_browser_then_reads_the_session() {
  let mut app = app();
  send(&mut app, ManageMessage::LastfmKey(" cle ".to_string()));
  send(
    &mut app,
    ManageMessage::LastfmSecret(" secret ".to_string()),
  );
  let effects = send(&mut app, ManageMessage::LastfmConnect);
  assert_eq!(
    effects[0],
    Effect::SaveLastfmKeys {
      key: "cle".to_string(),
      secret: "secret".to_string()
    }
  );
  assert_eq!(core_path(&effects), Some("/api/lastfm/token"));
  assert!(
    app.ui.manage.lastfm.api_secret.is_empty(),
    "le secret ne reste pas en mémoire d'interface"
  );
  let effects = reply(
    &mut app,
    ManageReply::LastfmToken,
    json!({ "success": true, "token": "T", "authUrl": "https://www.last.fm/api/auth/?t=T" }),
  );
  assert_eq!(
    effects,
    [Effect::OpenUrl(
      "https://www.last.fm/api/auth/?t=T".to_string()
    )]
  );
  assert_eq!(
    core_path(&send(&mut app, ManageMessage::LastfmPoll)),
    Some("/api/lastfm/session")
  );
  assert!(
    reply(
      &mut app,
      ManageReply::LastfmSession,
      json!({ "success": false })
    )
    .is_empty(),
    "pas encore autorisé"
  );
  let effects = reply(
    &mut app,
    ManageReply::LastfmSession,
    json!({ "success": true, "sessionKey": "S", "username": "moi" }),
  );
  assert_eq!(effects[0], Effect::SaveLastfmSession(Some("S".to_string())));
  assert!(app.ui.manage.lastfm.connected);
  assert_eq!(app.settings.lastfm_user, "moi");
  assert!(app.ui.manage.lastfm.token.is_none());
}

#[test]
fn the_session_is_never_written_in_the_settings() {
  let mut app = app();
  send(&mut app, ManageMessage::LastfmConnect);
  reply(
    &mut app,
    ManageReply::LastfmSession,
    json!({ "success": true, "sessionKey": "SESSION-SECRETE", "username": "moi" }),
  );
  let written = serde_json::to_string(&app.settings).unwrap();
  assert!(!written.contains("SESSION-SECRETE"));
}

#[test]
fn the_lastfm_authorization_gives_up_after_two_minutes() {
  let mut app = app();
  app.ui.manage.lastfm.token = Some("T".to_string());
  for _ in 0..LASTFM_POLL_MAX {
    assert!(!send(&mut app, ManageMessage::LastfmPoll).is_empty());
  }
  assert!(send(&mut app, ManageMessage::LastfmPoll).is_empty());
  assert_eq!(
    app.ui.manage.lastfm.progress,
    Progress::Failed("Last.fm authorization timed out".to_string())
  );
}

#[test]
fn logging_out_of_lastfm_clears_the_session() {
  let mut app = app();
  app.ui.manage.lastfm.connected = true;
  app.settings.lastfm_user = "moi".to_string();
  let effects = send(&mut app, ManageMessage::LastfmLogout);
  assert_eq!(effects[0], Effect::SaveLastfmSession(None));
  assert!(!app.ui.manage.lastfm.connected);
  assert!(app.settings.lastfm_user.is_empty());
}

#[test]
fn now_playing_is_sent_once_after_a_second() {
  let mut state = ScrobbleState::default();
  assert!(!state.tick("a", 0.5, 200.0, 100).now_playing);
  assert!(state.tick("a", 1.5, 200.0, 100).now_playing);
  assert!(!state.tick("a", 2.5, 200.0, 100).now_playing);
}

fn play(state: &mut ScrobbleState, key: &str, seconds: u32, duration: f64) -> u32 {
  let mut scrobbles = 0;
  for second in 0..=seconds {
    if state.tick(key, f64::from(second), duration, 1000).scrobble {
      scrobbles += 1;
    }
  }
  scrobbles
}

#[test]
fn a_track_is_scrobbled_at_half_its_length() {
  let mut state = ScrobbleState::default();
  assert_eq!(play(&mut state, "a", 99, 200.0), 0);
  assert!(state.tick("a", 100.0, 200.0, 1000).scrobble);
  assert!(
    !state.tick("a", 101.0, 200.0, 1000).scrobble,
    "une seule fois"
  );
}

#[test]
fn the_scrobble_threshold_stays_between_25_and_240_seconds() {
  let mut short = ScrobbleState::default();
  assert_eq!(play(&mut short, "a", 24, 30.0), 0);
  assert!(short.tick("a", 25.0, 30.0, 1000).scrobble);
  let mut long = ScrobbleState::default();
  assert_eq!(play(&mut long, "b", 239, 1000.0), 0);
  assert!(long.tick("b", 240.0, 1000.0, 1000).scrobble);
}

#[test]
fn a_track_shorter_than_25_seconds_is_never_scrobbled() {
  let mut state = ScrobbleState::default();
  assert_eq!(play(&mut state, "a", 24, 24.0), 0);
}

#[test]
fn an_unknown_duration_counts_as_three_minutes() {
  let mut state = ScrobbleState::default();
  assert_eq!(play(&mut state, "a", 89, 0.0), 0);
  assert!(state.tick("a", 90.0, 0.0, 1000).scrobble);
}

#[test]
fn a_jump_in_the_track_does_not_count_as_listening() {
  let mut state = ScrobbleState::default();
  state.tick("a", 0.0, 100.0, 1000);
  assert!(!state.tick("a", 90.0, 100.0, 1000).scrobble);
  assert!(state.played < 1.0);
}

#[test]
fn a_new_track_starts_again_and_a_replay_can_scrobble_again() {
  let mut state = ScrobbleState::default();
  assert_eq!(play(&mut state, "a", 60, 100.0), 1);
  assert_eq!(play(&mut state, "b", 60, 100.0), 1);
  assert_eq!(state.key, "b");
  assert_eq!(play(&mut state, "b", 60, 100.0), 1, "le titre a recommencé");
}

#[test]
fn scrobbles_are_sent_only_when_connected_and_enabled() {
  let mut app = app();
  app.player.current = Some(Track {
    id: Some("a".to_string()),
    title: Some("T".to_string()),
    artist: Some("A".to_string()),
    ..Track::default()
  });
  assert!(app.lastfm_tick(5.0, 100.0).is_empty());
  app.ui.manage.lastfm.connected = true;
  assert_eq!(
    core_path(&app.lastfm_tick(5.0, 100.0)),
    Some("/api/lastfm/now-playing")
  );
  app.settings.lastfm_enabled = false;
  assert!(app.lastfm_tick(6.0, 100.0).is_empty());
}

#[test]
fn the_scrobble_sends_the_start_time_of_the_track() {
  let mut app = app();
  app.player.current = Some(Track {
    id: Some("a".to_string()),
    title: Some("T".to_string()),
    artist: Some("A".to_string()),
    ..Track::default()
  });
  app.ui.manage.lastfm.connected = true;
  let mut sent = None;
  for second in 0..=30 {
    for effect in app.lastfm_tick(f64::from(second), 40.0) {
      if let Effect::CoreJson {
        path: "/api/lastfm/scrobble",
        body,
        ..
      } = effect
      {
        sent = body;
      }
    }
  }
  let body: Value = serde_json::from_str(&sent.expect("scrobble envoyé")).unwrap();
  assert_eq!(body["track"], "T");
  assert_eq!(body["artist"], "A");
  assert!(body["timestamp"].as_u64().unwrap() > 0);
}

#[test]
fn deleting_the_cookies_signs_out() {
  let mut app = app();
  app.ui.manage.youtube.signed_in = true;
  assert_eq!(
    send(&mut app, ManageMessage::CookiesDelete),
    [Effect::DeleteCookies]
  );
  assert!(!app.ui.manage.youtube.signed_in);
}

#[test]
fn importing_cookies_reports_the_result() {
  let mut app = app();
  assert_eq!(
    send(&mut app, ManageMessage::CookiesPick),
    [Effect::PickCookies]
  );
  send(
    &mut app,
    ManageMessage::CookiesPicked(Err("sans session".to_string())),
  );
  assert!(!app.ui.manage.youtube.signed_in);
  assert_eq!(
    app.ui.manage.youtube.progress,
    Progress::Failed("sans session".to_string())
  );
  send(&mut app, ManageMessage::CookiesPicked(Ok(())));
  assert!(app.ui.manage.youtube.signed_in);
}

#[test]
fn the_browser_cookies_are_asked_outside_of_update() {
  let mut app = app();
  assert_eq!(
    send(
      &mut app,
      ManageMessage::CookiesBrowser("firefox".to_string())
    ),
    [Effect::BrowserCookies("firefox".to_string())]
  );
  assert_eq!(app.ui.manage.youtube.progress, Progress::Running);
}

#[test]
fn the_local_information_is_shown() {
  let mut app = app();
  send(
    &mut app,
    ManageMessage::LocalLoaded(Box::new(LocalInfo {
      cache_bytes: 5,
      data_dir: "D".to_string(),
      download_dir: "T".to_string(),
      yt_signed_in: true,
      yt_dlp_auto_update: true,
      multi_instance: true,
      lastfm_connected: true,
      lastfm_has_secret: false,
    })),
  );
  assert_eq!(app.ui.manage.storage.cache_bytes, Some(5));
  assert_eq!(app.ui.manage.storage.data_dir, "D");
  assert!(app.ui.manage.youtube.signed_in);
  assert!(app.ui.manage.lastfm.connected);
  assert!(app.ui.manage.storage.yt_dlp_auto_update);
  assert!(app.ui.manage.storage.multi_instance);
}

#[test]
fn a_size_is_readable() {
  assert_eq!(human_size(1024 * 1024), "1.0 MB");
  assert_eq!(human_size(1536 * 1024 * 1024), "1.5 GB");
  assert_eq!(human_size(0), "0.0 MB");
}

#[test]
fn changing_the_data_folder_asks_for_a_restart() {
  let mut app = app();
  let kind = FolderKind::Data { move_data: true };
  assert_eq!(
    send(&mut app, ManageMessage::ChangeFolder(kind)),
    [Effect::PickDataFolder(kind)]
  );
  send(
    &mut app,
    ManageMessage::FolderChanged(kind, Ok("E:/riff".to_string())),
  );
  assert_eq!(app.ui.manage.storage.data_dir, "E:/riff");
  assert!(app.ui.manage.storage.restart_needed);
}

#[test]
fn cancelling_a_folder_changes_nothing_and_an_error_is_shown() {
  let mut app = app();
  send(
    &mut app,
    ManageMessage::FolderChanged(FolderKind::Download, Ok(String::new())),
  );
  assert!(app.ui.manage.storage.download_dir.is_empty());
  send(
    &mut app,
    ManageMessage::FolderChanged(FolderKind::Download, Err("non inscriptible".to_string())),
  );
  assert_eq!(
    app.ui.manage.storage.progress,
    Progress::Failed("non inscriptible".to_string())
  );
}

#[test]
fn a_windows_extended_path_is_shown_without_its_prefix() {
  assert_eq!(
    display_path(std::path::Path::new(r"\\?\C:\Riff\data")),
    r"C:\Riff\data"
  );
  assert_eq!(
    display_path(std::path::Path::new(r"D:\Downloads")),
    r"D:\Downloads"
  );
}

#[test]
fn navigating_to_the_settings_refreshes_the_accounts() {
  let mut app = app();
  let effects = app.update(Message::Navigate(crate::screens::Screen::Settings));
  assert_eq!(
    effects
      .iter()
      .filter(|effect| matches!(
        effect,
        Effect::CoreJson {
          path: "/api/spotify-status",
          ..
        }
      ))
      .count(),
    1
  );
}

#[test]
fn a_folder_inside_the_current_one_is_refused() {
  let inside = riff_core::paths::resolve_data_dir().join("dedans");
  assert_eq!(
    change_data_dir(&inside, false),
    Err("Choose a folder outside the current music folder".to_string())
  );
}

#[test]
fn an_account_round_trip_keeps_the_data_and_contains_no_secret() {
  let mut app = app();
  app.data.favorites.push(Track {
    id: Some("a".to_string()),
    title: Some("A".to_string()),
    ..Track::default()
  });
  app.data.playlists.push(Playlist {
    id: Some("pl".to_string()),
    name: Some("P".to_string()),
    ..Playlist::default()
  });
  app.data.volume = 0.25;
  app.data.crossfade = 5;
  app.settings.nickname = "Hugo".to_string();
  app
    .settings
    .sound
    .custom_presets
    .push(crate::audio_prefs::Preset {
      id: "c".to_string(),
      name: "C".to_string(),
      settings: crate::audio_prefs::Sound::default(),
    });
  app.settings.lastfm_user = "moi".to_string();
  let payload = app.account_payload();
  assert_eq!(payload["app"], "Riff");
  assert_eq!(payload["version"], "1.0");
  let text = payload.to_string();
  for secret in [
    "riff_lastfm_sk",
    "riff_lastfm_apisecret",
    "riff_deepl_api_key",
    "yt-cookies",
    "refreshToken",
    "accessToken",
  ] {
    assert!(!text.contains(secret), "{secret}");
  }

  let dir = crate::test_temp::TempDir::new("account");
  let path = dir.join("compte.rfacc");
  assert_eq!(
    riff_core::local_media::write_account_export(&path, &payload)["success"],
    true
  );
  let raw = std::fs::read(&path).unwrap();
  assert_eq!(&raw[0..4], b"RFAC", "fichier chiffré comme Riff");
  let read = read_account(&path).unwrap();

  let mut other = self::app();
  send(&mut other, ManageMessage::AccountRead(Ok(Some(read))));
  assert_eq!(other.data.favorites.len(), 1);
  assert_eq!(other.data.playlists[0].name.as_deref(), Some("P"));
  assert!((other.data.volume - 0.25).abs() < 1e-6);
  assert_eq!(other.data.crossfade, 5);
  assert_eq!(other.player.crossfade, 5);
  assert_eq!(other.settings.nickname, "Hugo");
  assert_eq!(other.settings.sound.custom_presets[0].name, "C");
  assert_eq!(other.settings.lastfm_user, "moi");
}

#[test]
fn the_account_export_rewrites_legacy_and_the_known_keys_win() {
  let mut app = app();
  app
    .settings
    .legacy
    .insert("riff_font".to_string(), json!("mono"));
  app
    .settings
    .legacy
    .insert("riff_ui_lang".to_string(), json!("perdu"));
  app.settings.ui_lang = "uk".to_string();
  let payload = app.account_payload();
  assert_eq!(payload["localStorage"]["riff_font"], "mono");
  assert_eq!(payload["localStorage"]["riff_ui_lang"], "uk");

  let mut other = self::app();
  send(&mut other, ManageMessage::AccountRead(Ok(Some(payload))));
  assert_eq!(other.settings.legacy["riff_font"], "mono");
  assert!(!other.settings.legacy.contains_key("riff_ui_lang"));
  assert_eq!(other.settings.ui_lang, "uk");
}

#[test]
fn an_account_of_riff_without_the_wrapper_is_read() {
  let mut app = app();
  send(
    &mut app,
    ManageMessage::AccountRead(Ok(Some(
      json!({ "riff_user_nickname": "Riff", "devsize_favorites": [{ "id": "a" }] }),
    ))),
  );
  assert_eq!(app.settings.nickname, "Riff");
  assert_eq!(app.data.favorites.len(), 1);
}

#[test]
fn a_bad_account_file_changes_nothing() {
  let mut app = app();
  app.settings.nickname = "Hugo".to_string();
  let effects = send(
    &mut app,
    ManageMessage::AccountRead(Ok(Some(json!([1, 2])))),
  );
  assert!(matches!(effects[0], Effect::Toast(_, ToastKind::Error)));
  assert_eq!(app.settings.nickname, "Hugo");
  assert!(read_account(std::path::Path::new("Z:/absent/compte.rfacc")).is_err());
}

#[test]
fn a_proxy_can_be_added_chosen_tested_and_removed() {
  let mut app = app();
  send(
    &mut app,
    ManageMessage::ProxyInput(" http://p:1 ".to_string()),
  );
  send(&mut app, ManageMessage::ProxyAdd);
  assert_eq!(app.settings.proxy_list, ["http://p:1"]);
  assert_eq!(app.settings.proxy_active.as_deref(), Some("http://p:1"));
  assert!(app.ui.manage.storage.proxy_input.is_empty());

  send(
    &mut app,
    ManageMessage::ProxyInput("http://p:1".to_string()),
  );
  send(&mut app, ManageMessage::ProxyAdd);
  assert_eq!(app.settings.proxy_list.len(), 1, "pas de doublon");

  let effects = send(&mut app, ManageMessage::ProxyToggle(true));
  assert!(effects.iter().any(|effect| matches!(effect, Effect::CoreJson { path: "/api/proxy-config", body: Some(body), .. } if body.contains("http://p:1") && body.contains("\"enabled\":true"))));
  assert_eq!(app.startup_proxy().len(), 1);

  assert_eq!(
    core_path(&send(&mut app, ManageMessage::ProxyTest)),
    Some("/api/proxy-test")
  );
  reply(
    &mut app,
    ManageReply::ProxyTest,
    json!({ "success": false, "error": "mort" }),
  );
  assert_eq!(
    app.ui.manage.storage.proxy_test,
    Progress::Failed("mort".to_string())
  );

  send(
    &mut app,
    ManageMessage::ProxyRemove("http://p:1".to_string()),
  );
  assert!(app.settings.proxy_list.is_empty());
  assert_eq!(app.settings.proxy_active, None);
  assert!(app.startup_proxy().is_empty());
}

#[test]
fn the_update_check_of_yt_dlp_can_be_turned_off() {
  let mut app = app();
  assert_eq!(
    send(&mut app, ManageMessage::YtDlpAutoUpdate(false)),
    [Effect::SetYtDlpAutoUpdate(false)]
  );
  assert!(!app.ui.manage.storage.yt_dlp_auto_update);
}

#[test]
fn the_multi_instance_switch_writes_and_is_kept_when_the_write_succeeds() {
  let mut app = app();
  assert!(!app.ui.manage.storage.multi_instance);
  assert_eq!(
    send(&mut app, ManageMessage::MultiInstance(true)),
    [Effect::SetMultiInstance(true)]
  );
  assert!(app.ui.manage.storage.multi_instance);
  assert!(send(&mut app, ManageMessage::MultiInstanceSaved(Ok(()))).is_empty());
  assert!(app.ui.manage.storage.multi_instance);
}

#[test]
fn a_failed_multi_instance_write_shows_an_error_and_restores_the_value_read() {
  let mut app = app();
  send(&mut app, ManageMessage::MultiInstance(true));
  let effects = send(&mut app, ManageMessage::MultiInstanceSaved(Err(false)));
  assert!(!app.ui.manage.storage.multi_instance);
  assert!(
    matches!(effects.as_slice(), [Effect::Toast(_, ToastKind::Error)]),
    "{effects:?}"
  );
}

#[test]
fn opening_the_settings_reads_the_accounts_only_then() {
  let app = app();
  let startup = app.startup_tools();
  assert!(
    !startup.iter().any(|effect| matches!(
      effect,
      Effect::CoreJson {
        path: "/api/spotify-status",
        ..
      }
    )),
    "aucune requête réseau au lancement"
  );
  assert!(app.settings_opened().iter().any(|effect| matches!(
    effect,
    Effect::CoreJson {
      path: "/api/spotify-status",
      ..
    }
  )));
}

// Import par fichier (exports Spotify et Apple Music)

fn exported(title: &str, artist: &str, duration: Option<f64>) -> ImportedTrack {
  ImportedTrack {
    title: title.to_string(),
    artist: artist.to_string(),
    duration,
  }
}

fn exported_list(name: &str, titles: &[&str]) -> ImportedPlaylist {
  ImportedPlaylist {
    name: name.to_string(),
    tracks: titles
      .iter()
      .map(|title| exported(title, "Les Lunes", None))
      .collect(),
  }
}

/// Fenêtre ouverte en mode fichier, avec une sélection déjà lue.
fn file_import(app: &mut App, kind: ImportKind, library: Library) {
  send(app, ManageMessage::OpenImport(kind));
  send(app, ManageMessage::ImportMode(ImportMode::File));
  let read = FilesRead {
    names: vec!["export.json".to_string()],
    library,
    errors: Vec::new(),
  };
  send(app, ManageMessage::ImportFilesRead(Some(Box::new(read))));
}

fn sample_library() -> Library {
  Library {
    likes: vec![
      exported("Aube", "Les Lunes", Some(215.0)),
      exported("Brume", "Nord", None),
    ],
    playlists: vec![
      exported_list("Route", &["Aube", "Ciel"]),
      exported_list("Soir", &["Orage"]),
    ],
  }
}

fn failure(app: &App) -> Option<&str> {
  match &app.ui.manage.import.progress {
    Progress::Failed(message) => Some(message),
    _ => None,
  }
}

#[test]
fn only_spotify_and_apple_pick_export_files() {
  let mut app = app();
  assert_eq!(
    send(
      &mut app,
      ManageMessage::ImportFilesPick(ImportKind::Spotify)
    ),
    [Effect::PickExportFiles(ExportSource::Spotify)]
  );
  assert_eq!(
    send(&mut app, ManageMessage::ImportFilesPick(ImportKind::Apple)),
    [Effect::PickExportFiles(ExportSource::Apple)]
  );
  for kind in [ImportKind::Link, ImportKind::Yandex] {
    assert!(send(&mut app, ManageMessage::ImportFilesPick(kind)).is_empty());
  }
}

#[test]
fn the_picked_files_replace_the_previous_ones_and_a_cancel_keeps_them() {
  let mut app = app();
  file_import(&mut app, ImportKind::Spotify, sample_library());
  assert_eq!(app.ui.manage.import.files, ["export.json"]);
  assert_eq!(app.ui.manage.import.export, sample_library());
  assert!(send(&mut app, ManageMessage::ImportFilesRead(None)).is_empty());
  assert_eq!(app.ui.manage.import.export, sample_library());
  let second = FilesRead {
    names: vec!["a.json".to_string(), "b.json".to_string()],
    library: Library::default(),
    errors: Vec::new(),
  };
  send(
    &mut app,
    ManageMessage::ImportFilesRead(Some(Box::new(second))),
  );
  assert_eq!(app.ui.manage.import.files, ["a.json", "b.json"]);
  assert!(app.ui.manage.import.export.is_empty());
}

#[test]
fn exported_likes_reach_the_favorites_without_duplicates() {
  let mut app = app();
  app.data.favorites.push(Track {
    title: Some("Aube".to_string()),
    artist: Some("Les Lunes".to_string()),
    duration: Some(215.0),
    ..Track::default()
  });
  let mut library = sample_library();
  library.likes.push(exported("Brume", "Nord", None));
  library.playlists.clear();
  file_import(&mut app, ImportKind::Spotify, library);
  let effects = send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify));
  let titles: Vec<_> = app
    .data
    .favorites
    .iter()
    .map(|track| track.title.as_deref().unwrap_or(""))
    .collect();
  assert_eq!(titles, ["Aube", "Brume"]);
  assert!(app.data.playlists.is_empty());
  assert!(!effects.is_empty(), "les données sont sauvegardées");
  assert!(
    effects
      .iter()
      .all(|effect| !matches!(effect, Effect::CoreJson { .. })),
    "aucune requête : les titres sont cherchés à la lecture"
  );
  assert!(matches!(app.ui.manage.import.progress, Progress::Done(_)));
}

#[test]
fn an_exported_track_has_no_id_and_keeps_its_duration() {
  let mut app = app();
  file_import(&mut app, ImportKind::Apple, sample_library());
  send(&mut app, ManageMessage::ImportStart(ImportKind::Apple));
  let aube = &app.data.favorites[0];
  assert_eq!(aube.id, None);
  assert_eq!(aube.artist.as_deref(), Some("Les Lunes"));
  assert_eq!(aube.duration, Some(215.0));
}

#[test]
fn exported_playlists_are_created_with_their_names_and_tracks() {
  let mut app = app();
  file_import(&mut app, ImportKind::Spotify, sample_library());
  send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify));
  let names: Vec<_> = app
    .data
    .playlists
    .iter()
    .map(|playlist| playlist.name.as_deref().unwrap_or(""))
    .collect();
  assert_eq!(names, ["Route", "Soir"]);
  assert_eq!(app.data.playlists[0].tracks.len(), 2);
  assert_eq!(app.data.playlists[1].tracks.len(), 1);
  assert_ne!(app.data.playlists[0].id, app.data.playlists[1].id);
  assert_eq!(app.data.favorites.len(), 2);
  assert_eq!(
    app.ui.manage.import.progress,
    Progress::Done("2 liked tracks, 2 playlists".to_string())
  );
}

#[test]
fn the_liked_songs_box_is_respected() {
  let mut app = app();
  file_import(&mut app, ImportKind::Spotify, sample_library());
  send(&mut app, ManageMessage::ImportLikes(false));
  send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify));
  assert!(app.data.favorites.is_empty());
  assert_eq!(app.data.playlists.len(), 2);
}

#[test]
fn the_playlists_box_is_respected() {
  let mut app = app();
  file_import(&mut app, ImportKind::Spotify, sample_library());
  send(&mut app, ManageMessage::ImportPlaylists(false));
  send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify));
  assert_eq!(app.data.favorites.len(), 2);
  assert!(app.data.playlists.is_empty());
}

#[test]
fn both_boxes_are_checked_when_the_window_opens() {
  let mut app = app();
  send(&mut app, ManageMessage::OpenImport(ImportKind::Apple));
  assert!(app.ui.manage.import.likes);
  assert!(app.ui.manage.import.playlists);
  assert_eq!(app.ui.manage.import.mode, ImportMode::Link);
}

#[test]
fn spotify_needs_at_least_one_box() {
  let mut app = app();
  file_import(&mut app, ImportKind::Spotify, sample_library());
  send(&mut app, ManageMessage::ImportLikes(false));
  send(&mut app, ManageMessage::ImportPlaylists(false));
  let effects = send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify));
  assert!(effects.is_empty());
  assert_eq!(
    failure(&app),
    Some("Select at least Liked songs or Playlists")
  );
  assert!(app.data.favorites.is_empty());
  assert!(app.data.playlists.is_empty());
}

#[test]
fn nothing_found_gives_the_message_of_riff_for_each_source() {
  let mut app = app();
  file_import(&mut app, ImportKind::Spotify, Library::default());
  assert!(send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify)).is_empty());
  assert_eq!(failure(&app), Some("No tracks found in the selected files"));
  file_import(&mut app, ImportKind::Apple, Library::default());
  assert!(send(&mut app, ManageMessage::ImportStart(ImportKind::Apple)).is_empty());
  assert_eq!(failure(&app), Some("No tracks selected or found to import"));
}

#[test]
fn spotify_boxes_hide_what_the_files_hold() {
  let mut app = app();
  let mut only_likes = sample_library();
  only_likes.playlists.clear();
  file_import(&mut app, ImportKind::Spotify, only_likes);
  send(&mut app, ManageMessage::ImportLikes(false));
  assert!(send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify)).is_empty());
  assert_eq!(failure(&app), Some("No tracks found in the selected files"));
  assert!(app.data.favorites.is_empty());
}

#[test]
fn apple_ignores_the_boxes_when_only_one_kind_is_found() {
  let mut app = app();
  let mut library = sample_library();
  library.likes.clear();
  file_import(&mut app, ImportKind::Apple, library);
  send(&mut app, ManageMessage::ImportPlaylists(false));
  send(&mut app, ManageMessage::ImportLikes(false));
  send(&mut app, ManageMessage::ImportStart(ImportKind::Apple));
  assert_eq!(app.data.playlists.len(), 2);
  assert!(app.data.favorites.is_empty());
}

#[test]
fn apple_follows_the_boxes_when_both_kinds_are_found() {
  let mut app = app();
  file_import(&mut app, ImportKind::Apple, sample_library());
  send(&mut app, ManageMessage::ImportLikes(false));
  send(&mut app, ManageMessage::ImportStart(ImportKind::Apple));
  assert!(app.data.favorites.is_empty());
  assert_eq!(app.data.playlists.len(), 2);
}

#[test]
fn a_new_selection_checks_the_apple_boxes_again() {
  let mut app = app();
  file_import(&mut app, ImportKind::Apple, sample_library());
  send(&mut app, ManageMessage::ImportLikes(false));
  send(&mut app, ManageMessage::ImportPlaylists(false));
  let read = FilesRead {
    names: vec!["b.csv".to_string()],
    library: sample_library(),
    errors: Vec::new(),
  };
  send(
    &mut app,
    ManageMessage::ImportFilesRead(Some(Box::new(read))),
  );
  assert!(app.ui.manage.import.likes && app.ui.manage.import.playlists);
}

#[test]
fn a_finished_file_import_is_not_added_twice() {
  let mut app = app();
  file_import(&mut app, ImportKind::Spotify, sample_library());
  send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify));
  assert!(app.ui.manage.import.export.is_empty());
  assert!(send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify)).is_empty());
  assert_eq!(app.data.playlists.len(), 2);
  assert_eq!(app.data.favorites.len(), 2);
}

#[test]
fn a_refused_file_is_reported() {
  let mut app = app();
  send(&mut app, ManageMessage::OpenImport(ImportKind::Spotify));
  let read = FilesRead {
    names: vec!["enorme.json".to_string()],
    library: Library::default(),
    errors: vec![("enorme.json".to_string(), ImportError::TooLarge)],
  };
  send(
    &mut app,
    ManageMessage::ImportFilesRead(Some(Box::new(read))),
  );
  assert_eq!(failure(&app), Some("File too large (20 MB max)"));
}

#[test]
fn the_mode_decides_between_the_url_and_the_files() {
  let mut app = app();
  send(&mut app, ManageMessage::OpenImport(ImportKind::Spotify));
  send(
    &mut app,
    ManageMessage::ImportUrl("https://open.spotify.com/playlist/x".to_string()),
  );
  send(&mut app, ManageMessage::ImportMode(ImportMode::File));
  let effects = send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify));
  assert!(effects.is_empty(), "mode fichier : le lien n'est pas lu");
  send(&mut app, ManageMessage::ImportMode(ImportMode::Link));
  let effects = send(&mut app, ManageMessage::ImportStart(ImportKind::Spotify));
  assert_eq!(core_path(&effects), Some("/api/spotify-import"));
}

#[test]
fn opening_the_window_again_forgets_the_files() {
  let mut app = app();
  file_import(&mut app, ImportKind::Apple, sample_library());
  send(&mut app, ManageMessage::OpenImport(ImportKind::Apple));
  assert!(app.ui.manage.import.files.is_empty());
  assert!(app.ui.manage.import.export.is_empty());
}

#[test]
fn the_apple_summary_has_the_texts_of_riff() {
  let mut app = app();
  assert_eq!(app.export_summary(), None);
  file_import(&mut app, ImportKind::Apple, sample_library());
  assert_eq!(
    app.export_summary().as_deref(),
    Some("Found 2 liked songs and 2 playlists")
  );
  let mut playlists = sample_library();
  playlists.likes.clear();
  file_import(&mut app, ImportKind::Apple, playlists);
  assert_eq!(
    app.export_summary().as_deref(),
    Some("Found 2 playlists (3 tracks)")
  );
  let mut likes = sample_library();
  likes.playlists.clear();
  file_import(&mut app, ImportKind::Apple, likes);
  assert_eq!(
    app.export_summary().as_deref(),
    Some("Found 2 liked tracks")
  );
  file_import(&mut app, ImportKind::Apple, Library::default());
  assert_eq!(
    app.export_summary().as_deref(),
    Some("No tracks found in selected files")
  );
}
