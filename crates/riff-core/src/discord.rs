//! Portage de `src/main/discord.js` : Rich Presence Discord sur l'IPC local (tube nommé `discord-ipc-N`).
//! Client écrit à la main plutôt qu'avec la crate `discord-rich-presence` : celle-ci lit en bloquant sans délai
//! possible (le thread restait coincé quand Discord ne répondait pas au handshake) et ignore les réponses.

use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout};

/// Identifiant public de l'application Discord de Riff (surchargeable par `RIFF_DISCORD_CLIENT_ID`).
const CLIENT_ID: &str = "1520100510877749358";
const RECONNECT_DELAY: Duration = Duration::from_secs(15);
const RESTART_DELAY: Duration = Duration::from_millis(600);
/// Discord met un instant à libérer le tube après une fermeture (`isDestroying` de l'original : 500 ms).
const SETTLE_DELAY: Duration = Duration::from_millis(500);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FRAME: usize = 1 << 20;

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

#[cfg(windows)]
type Conn = tokio::net::windows::named_pipe::NamedPipeClient;
#[cfg(not(windows))]
type Conn = tokio::net::UnixStream;

enum Msg {
  Update(Value),
  Clear,
  SetEnabled(bool),
  Reconnect,
}

struct Handle {
  tx: mpsc::UnboundedSender<Msg>,
  enabled: Arc<AtomicBool>,
  connected: Arc<AtomicBool>,
}

static HANDLE: OnceLock<Handle> = OnceLock::new();

/// Démarre la tâche et tente la première connexion (équivalent de `initRPC()` dans `createWindow`).
/// Démarre le client. `spawn` exécute la tâche de fond sur le runtime de l'application.
pub fn init(spawn: impl FnOnce(BoxFuture<'static, ()>)) {
  HANDLE.get_or_init(|| {
    let (tx, rx) = mpsc::unbounded_channel();
    let enabled = Arc::new(AtomicBool::new(true));
    let connected = Arc::new(AtomicBool::new(false));
    let worker = Worker {
      enabled: enabled.clone(),
      connected: connected.clone(),
      conn: None,
      last_track: None,
      retry_at: None,
      destroyed_at: None,
    };
    spawn(worker.run(rx).boxed());
    Handle {
      tx,
      enabled,
      connected,
    }
  });
}

fn send(msg: Msg) {
  if let Some(h) = HANDLE.get() {
    let _ = h.tx.send(msg);
  }
}

pub fn update(track_info: Value) {
  if track_info.is_object() {
    send(Msg::Update(track_info));
  }
}

pub fn clear() {
  send(Msg::Clear);
}

pub fn set_enabled(enabled: bool) {
  if let Some(h) = HANDLE.get() {
    h.enabled.store(enabled, Ordering::SeqCst);
  }
  send(Msg::SetEnabled(enabled));
}

pub fn get_enabled() -> bool {
  HANDLE
    .get()
    .map(|h| h.enabled.load(Ordering::SeqCst))
    .unwrap_or(true)
}

pub fn get_connected() -> bool {
  HANDLE
    .get()
    .map(|h| h.connected.load(Ordering::SeqCst))
    .unwrap_or(false)
}

pub fn reconnect() {
  send(Msg::Reconnect);
}

#[cfg(windows)]
async fn open_pipe() -> Option<Conn> {
  use tokio::net::windows::named_pipe::ClientOptions;
  for i in 0..10 {
    if let Ok(c) = ClientOptions::new().open(format!(r"\\.\pipe\discord-ipc-{i}")) {
      return Some(c);
    }
  }
  None
}

/// Non testé hors Windows.
#[cfg(not(windows))]
async fn open_pipe() -> Option<Conn> {
  let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
  for i in 0..10 {
    if let Ok(c) = Conn::connect(format!("{base}/discord-ipc-{i}")).await {
      return Some(c);
    }
  }
  None
}

async fn write_frame(c: &mut Conn, op: u32, data: &Value) -> io::Result<()> {
  let body = data.to_string();
  let mut buf = Vec::with_capacity(8 + body.len());
  buf.extend_from_slice(&op.to_le_bytes());
  buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
  buf.extend_from_slice(body.as_bytes());
  timeout(IO_TIMEOUT, c.write_all(&buf))
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

async fn read_frame(c: &mut Conn) -> io::Result<(u32, Value)> {
  timeout(IO_TIMEOUT, async {
    let mut head = [0u8; 8];
    c.read_exact(&mut head).await?;
    let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if len > MAX_FRAME {
      return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut body = vec![0u8; len];
    c.read_exact(&mut body).await?;
    let data =
      serde_json::from_slice(&body).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    Ok((op, data))
  })
  .await
  .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

/// Prochaine réponse en répondant aux PING. `None` si Discord ferme ou si l'E/S échoue.
async fn read_reply(c: &mut Conn) -> Option<Value> {
  loop {
    let (op, data) = read_frame(c).await.ok()?;
    match op {
      OP_CLOSE => return None,
      OP_PING => write_frame(c, OP_PONG, &data).await.ok()?,
      _ => return Some(data),
    }
  }
}

/// Traite les trames reçues hors requête (complète une trame coupée). `false` si la connexion est à fermer.
async fn handle_idle_bytes(c: &mut Conn, mut data: Vec<u8>) -> bool {
  while !data.is_empty() {
    let Ok(missing) = missing_for_header(&data, c).await else {
      return false;
    };
    data.extend_from_slice(&missing);
    let len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if len > MAX_FRAME {
      return false;
    }
    if data.len() < 8 + len {
      let mut rest = vec![0u8; 8 + len - data.len()];
      match timeout(IO_TIMEOUT, c.read_exact(&mut rest)).await {
        Ok(Ok(_)) => data.extend_from_slice(&rest),
        _ => return false,
      }
    }
    let op = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let body: Vec<u8> = data.drain(..8 + len).skip(8).collect();
    match op {
      OP_CLOSE => return false,
      OP_PING => {
        let Ok(v) = serde_json::from_slice::<Value>(&body) else {
          return false;
        };
        if write_frame(c, OP_PONG, &v).await.is_err() {
          return false;
        }
      }
      _ => {}
    }
  }
  true
}

/// Octets manquants pour avoir un en-tête de 8 octets complet.
async fn missing_for_header(data: &[u8], c: &mut Conn) -> io::Result<Vec<u8>> {
  if data.len() >= 8 {
    return Ok(Vec::new());
  }
  let mut rest = vec![0u8; 8 - data.len()];
  timeout(IO_TIMEOUT, c.read_exact(&mut rest))
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
  Ok(rest)
}

/// Ouvre le tube, fait le handshake et attend `READY`.
async fn handshake(client_id: &str) -> Option<(Conn, String)> {
  let mut c = open_pipe().await?;
  let hello = json!({ "v": 1, "client_id": client_id });
  write_frame(&mut c, OP_HANDSHAKE, &hello).await.ok()?;
  let reply = read_reply(&mut c).await?;
  if reply.get("evt").and_then(Value::as_str) != Some("READY") {
    return None;
  }
  let name = reply
    .pointer("/data/user/username")
    .and_then(Value::as_str)
    .unwrap_or("unknown")
    .to_string();
  Some((c, name))
}

/// `SET_ACTIVITY` (activité `null` = effacement). `None` si la connexion est perdue, `Some(accepté)` sinon.
async fn set_activity(c: &mut Conn, activity: Value) -> Option<bool> {
  let nonce = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let payload = json!({
    "cmd": "SET_ACTIVITY",
    "args": { "pid": std::process::id(), "activity": activity },
    "nonce": format!("{nonce:x}"),
  });
  write_frame(c, OP_FRAME, &payload).await.ok()?;
  let reply = read_reply(c).await?;
  Some(reply.get("evt").and_then(Value::as_str) != Some("ERROR"))
}

/// Envoie l'activité avec `status_display_type`, puis sans si Discord la refuse. `false` si la connexion est perdue.
async fn push_track(c: &mut Conn, info: &Value) -> bool {
  let p = Params::from(info);
  match set_activity(c, p.activity(true)).await {
    None => false,
    Some(true) => true,
    Some(false) => match set_activity(c, p.activity(false)).await {
      None => false,
      Some(true) => true,
      Some(false) => {
        tracing::warn!("[Discord RPC] SET_ACTIVITY refusé par Discord");
        true
      }
    },
  }
}

struct Worker {
  enabled: Arc<AtomicBool>,
  connected: Arc<AtomicBool>,
  conn: Option<Conn>,
  last_track: Option<Value>,
  retry_at: Option<Instant>,
  destroyed_at: Option<Instant>,
}

impl Worker {
  async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
    self.connect().await;
    loop {
      let retry = self.retry_at;
      tokio::select! {
        msg = rx.recv() => match msg {
          Some(m) => self.handle(m).await,
          None => break,
        },
        _ = sleep_until(retry.unwrap_or_else(Instant::now)), if retry.is_some() => {
          self.retry_at = None;
          if self.is_enabled() && self.conn.is_none() {
            self.connect().await;
          }
        }
        _ = async {
          match self.conn.as_ref() {
            Some(c) => { let _ = c.readable().await; }
            None => std::future::pending::<()>().await,
          }
        } => self.on_incoming().await,
      }
    }
  }

  fn is_enabled(&self) -> bool {
    self.enabled.load(Ordering::SeqCst)
  }

  async fn handle(&mut self, msg: Msg) {
    match msg {
      Msg::Update(info) => {
        self.last_track = Some(info);
        if !self.is_enabled() {
          return;
        }
        if self.conn.is_some() {
          self.apply_last_track().await;
        } else if self.retry_at.is_none() {
          self.connect().await;
        }
      }
      Msg::Clear => {
        self.last_track = None;
        self.clear_activity().await;
      }
      Msg::SetEnabled(true) => {
        if self.conn.is_none() {
          self.retry_at = None;
          self.connect().await;
        } else {
          self.apply_last_track().await;
        }
      }
      Msg::SetEnabled(false) => {
        self.clear_activity().await;
        self.destroy_conn().await;
      }
      Msg::Reconnect => {
        self.destroy_conn().await;
        tokio::time::sleep(RESTART_DELAY).await;
        self.connect().await;
      }
    }
  }

  /// Données ou fermeture reçues hors requête : PING, fermeture (événement `disconnected` de l'original).
  async fn on_incoming(&mut self) {
    let Some(c) = self.conn.as_mut() else {
      return;
    };
    // `readable()` peut se réveiller à tort : `try_read` tranche (WouldBlock = fausse alerte).
    let mut buf = [0u8; 4096];
    let alive = match c.try_read(&mut buf) {
      Err(e) if e.kind() == io::ErrorKind::WouldBlock => true,
      Ok(0) | Err(_) => false,
      Ok(n) => handle_idle_bytes(c, buf[..n].to_vec()).await,
    };
    if !alive {
      tracing::info!("[Discord RPC] Disconnected");
      self.connection_lost();
    }
  }

  async fn connect(&mut self) {
    if let Some(t) = self.destroyed_at.take() {
      tokio::time::sleep(SETTLE_DELAY.saturating_sub(t.elapsed())).await;
    }
    let id = std::env::var("RIFF_DISCORD_CLIENT_ID").unwrap_or_else(|_| CLIENT_ID.to_string());
    match timeout(IO_TIMEOUT * 2, handshake(&id)).await {
      Ok(Some((conn, user))) => {
        tracing::info!("[Discord RPC] Connected as {user}");
        self.conn = Some(conn);
        self.connected.store(true, Ordering::SeqCst);
        if self.is_enabled() {
          self.apply_last_track().await;
        }
      }
      _ => {
        tracing::info!("[Discord RPC] Connexion impossible");
        self.connection_lost();
      }
    }
  }

  /// Équivalent de l'événement `disconnected` / d'un échec de login : nouvel essai dans 15 s.
  fn connection_lost(&mut self) {
    self.conn = None;
    self.connected.store(false, Ordering::SeqCst);
    if self.is_enabled() && self.retry_at.is_none() {
      self.retry_at = Some(Instant::now() + RECONNECT_DELAY);
    }
  }

  /// Fermeture propre (trame `CLOSE`) : sans elle Discord ne répond plus au handshake suivant.
  async fn destroy_conn(&mut self) {
    self.retry_at = None;
    if let Some(mut c) = self.conn.take() {
      let _ = write_frame(&mut c, OP_CLOSE, &json!({})).await;
      self.destroyed_at = Some(Instant::now());
    }
    self.connected.store(false, Ordering::SeqCst);
  }

  async fn clear_activity(&mut self) {
    let Some(c) = self.conn.as_mut() else {
      return;
    };
    if set_activity(c, Value::Null).await.is_none() {
      self.connection_lost();
    }
  }

  async fn apply_last_track(&mut self) {
    let (Some(info), Some(c)) = (self.last_track.as_ref(), self.conn.as_mut()) else {
      return;
    };
    if !push_track(c, info).await {
      self.connection_lost();
    }
  }
}

struct Params {
  details: String,
  state: String,
  thumbnail: String,
  url: Option<String>,
  start: i64,
  duration: i64,
  artist_in_status: bool,
}

impl Params {
  fn from(info: &Value) -> Self {
    let text = |key: &str, default: &str| -> String {
      info
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
    };
    let artist_in_status = info.get("artistInStatus").and_then(Value::as_bool) != Some(false);
    let artist = text("artist", "Unknown Artist");
    let state = if artist_in_status {
      clean(&artist)
    } else {
      clean(&format!("by {artist}"))
    };
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_secs() as i64)
      .unwrap_or(0);
    let start = info
      .get("startTimestamp")
      .and_then(Value::as_f64)
      .filter(|v| *v != 0.0)
      .map(|v| v as i64)
      .unwrap_or(now);
    let duration = info
      .get("duration")
      .and_then(Value::as_f64)
      .filter(|v| *v > 0.0)
      .map(|v| v.floor() as i64)
      .unwrap_or(0);
    let url = info
      .get("url")
      .and_then(Value::as_str)
      .filter(|u| u.starts_with("https://") && u.len() <= 512)
      .map(str::to_string);
    Params {
      details: clean(&text("title", "Unknown Track")),
      state,
      thumbnail: text("thumbnail", ""),
      url,
      start,
      duration,
      artist_in_status,
    }
  }

  /// Objet `activity` du protocole (type 2 = « Listening »). `status_display_type: 1` = afficher l'artiste.
  fn activity(&self, with_display: bool) -> Value {
    let mut timestamps = json!({ "start": self.start });
    if self.duration > 0 {
      timestamps["end"] = json!(self.start + self.duration);
    }
    let mut a = json!({
      "type": 2,
      "details": self.details,
      "state": self.state,
      "timestamps": timestamps,
      "instance": false,
    });
    if !self.thumbnail.is_empty() {
      a["assets"] = json!({ "large_image": self.thumbnail });
    }
    if let Some(url) = &self.url {
      a["buttons"] = json!([{ "label": "Listen", "url": url }]);
    }
    if with_display && self.artist_in_status {
      a["status_display_type"] = json!(1);
    }
    a
  }
}

/// `substring(0, 128).padEnd(2, ' ')` : Discord refuse les champs de moins de 2 caractères.
fn clean(s: &str) -> String {
  let mut out: String = s.chars().take(128).collect();
  while out.chars().count() < 2 {
    out.push(' ');
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn clean_pads_and_truncates() {
    assert_eq!(clean("a"), "a ");
    assert_eq!(clean(""), "  ");
    assert_eq!(clean(&"x".repeat(200)).chars().count(), 128);
  }

  #[test]
  fn params_defaults_and_url_rules() {
    let p = Params::from(&json!({ "url": "http://x", "artistInStatus": false, "artist": "A" }));
    assert_eq!(p.details, "Unknown Track");
    assert_eq!(p.state, "by A");
    assert!(p.url.is_none());
    let p = Params::from(&json!({ "url": "https://x", "duration": 215.9, "startTimestamp": 1000 }));
    assert_eq!(p.url.as_deref(), Some("https://x"));
    assert_eq!((p.start, p.duration), (1000, 215));
    assert_eq!(p.state, "Unknown Artist");
  }

  #[test]
  fn activity_shape_matches_original() {
    let p = Params::from(&json!({
      "title": "T", "artist": "Art", "duration": 100, "startTimestamp": 50,
      "thumbnail": "https://i/x.jpg", "url": "https://u"
    }));
    let a = p.activity(true);
    assert_eq!(a["type"], 2);
    assert_eq!(a["timestamps"], json!({ "start": 50, "end": 150 }));
    assert_eq!(a["status_display_type"], 1);
    assert_eq!(a["buttons"][0]["label"], "Listen");
    assert_eq!(a["assets"]["large_image"], "https://i/x.jpg");
    assert!(p.activity(false).get("status_display_type").is_none());
  }

  /// Nécessite un Discord ouvert ; modifie brièvement la présence de l'utilisateur.
  #[tokio::test]
  #[ignore]
  async fn live_presence() {
    let (mut c, user) = handshake(CLIENT_ID).await.expect("handshake");
    println!("READY user={user}");
    let info = json!({
      "title": "riff-rs test", "artist": "test", "artistInStatus": true, "duration": 200,
      "startTimestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() - 30,
      "url": "https://example.com/x", "thumbnail": ""
    });
    println!("accepted={}", push_track(&mut c, &info).await);
    println!("clear={:?}", set_activity(&mut c, Value::Null).await);
  }

  /// Nécessite un Discord ouvert ; teste la tâche (connexion, désactivation, réactivation, reconnexion).
  #[tokio::test]
  #[ignore]
  async fn live_worker() {
    // Discord ignore les handshakes pendant ~30 s après plusieurs connexions rapprochées.
    async fn wait_for(expected: bool) -> bool {
      for _ in 0..120 {
        if get_connected() == expected {
          return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
      }
      false
    }
    init(|task| {
      tokio::spawn(task);
    });
    assert!(wait_for(true).await, "connecté au démarrage");
    update(json!({ "title": "riff-rs test", "artist": "test", "duration": 200 }));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    set_enabled(false);
    assert!(
      wait_for(false).await && !get_enabled(),
      "déconnecté une fois désactivé"
    );
    set_enabled(true);
    assert!(
      wait_for(true).await && get_enabled(),
      "reconnecté une fois réactivé"
    );
    reconnect();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(wait_for(true).await, "reconnecté après reconnect()");
    clear();
    tokio::time::sleep(Duration::from_millis(1500)).await;
  }
}
