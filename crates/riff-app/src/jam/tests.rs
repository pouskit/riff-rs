//! Tests de session : transport en mémoire, aucun réseau, délais raccourcis par `HostConfig` et `ClientConfig`.

use super::memory::Hub;
use super::protocol::*;
use super::sync::{self, ClockSync};
use super::transport::{Link, Room, Security, Transport};
use super::*;
use crate::user_data::Track;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

const WAIT: Duration = Duration::from_secs(5);

fn host_config() -> HostConfig {
  HostConfig {
    handshake_timeout: Duration::from_millis(300),
    peer_silence: WAIT,
    refuse_delay: Duration::from_millis(20),
    close_timeout: Duration::from_millis(500),
    ..HostConfig::default()
  }
}

fn client_config() -> ClientConfig {
  ClientConfig {
    handshake_timeout: Duration::from_secs(2),
    peer_silence: WAIT,
    close_timeout: Duration::from_millis(500),
    ping_every: Duration::from_millis(50),
    ping_burst: [
      Duration::ZERO,
      Duration::from_millis(10),
      Duration::from_millis(20),
    ],
    reconnect_delays: [Duration::from_millis(20); 3],
    ..ClientConfig::default()
  }
}

struct Peer {
  link: JamLink,
  events: UnboundedReceiver<JamEvent>,
  task: JoinHandle<EndCause>,
}

impl Peer {
  async fn next(&mut self) -> JamEvent {
    tokio::time::timeout(WAIT, self.events.recv())
      .await
      .expect("event in time")
      .expect("event channel open")
  }

  /// Jette les événements jusqu'à celui que `pick` retient.
  async fn until<T>(&mut self, mut pick: impl FnMut(&JamEvent) -> Option<T>) -> T {
    loop {
      let event = self.next().await;
      if let Some(found) = pick(&event) {
        return found;
      }
    }
  }

  fn send(&self, out: JamOut) {
    self.link.commands.send(out).expect("session running");
  }

  async fn joined(&mut self) -> (u32, Vec<Participant>, QueueState, PlaybackState, Instant) {
    self
      .until(|event| match event {
        JamEvent::Joined {
          you,
          participants,
          queue,
          playback,
          anchor,
        } => Some((
          *you,
          participants.clone(),
          queue.clone(),
          playback.clone(),
          *anchor,
        )),
        _ => None,
      })
      .await
  }

  async fn ended(&mut self) -> EndCause {
    self
      .until(|event| match event {
        JamEvent::Ended(cause) => Some(cause.clone()),
        _ => None,
      })
      .await
  }
}

async fn host(hub: &Hub) -> (Peer, String) {
  let (link, task) = start_host_with(
    Transport::Memory(hub.clone()),
    "Hote".to_string(),
    1,
    host_config(),
  );
  let events = link.inbox.take().expect("inbox");
  let mut peer = Peer {
    link,
    events,
    task: tokio::spawn(task),
  };
  let ticket = peer
    .until(|event| match event {
      JamEvent::Hosting { ticket } => Some(ticket.clone()),
      _ => None,
    })
    .await;
  (peer, ticket)
}

fn guest(hub: &Hub, ticket: &str, name: &str) -> Peer {
  let (link, task) = start_guest_with(
    Transport::Memory(hub.clone()),
    ticket.to_string(),
    name.to_string(),
    1,
    client_config(),
  );
  let events = link.inbox.take().expect("inbox");
  Peer {
    link,
    events,
    task: tokio::spawn(task),
  }
}

fn wire(id: &str) -> WireTrack {
  WireTrack {
    platform: "youtube".to_string(),
    id: Some(id.to_string()),
    title: Some(format!("Titre {id}")),
    ..WireTrack::default()
  }
}

fn queue_of(ids: &[&str]) -> QueueState {
  QueueState {
    qv: 1,
    entries: ids
      .iter()
      .zip(1u64..)
      .map(|(id, uid)| Entry {
        uid,
        by: 0,
        by_name: "Hote".to_string(),
        track: wire(id),
      })
      .collect(),
  }
}

fn playback(playing: bool) -> PlaybackState {
  PlaybackState {
    pv: 1,
    current: Some(Current {
      uid: Some(1),
      track: wire("a"),
    }),
    start: 1,
    playing,
    pos_ms: 10_000,
    ..PlaybackState::default()
  }
}

/// Invité brut : parle le protocole à la main.
struct Raw {
  link: Link,
}

impl Raw {
  async fn connect(hub: &Hub, ticket: &str) -> Self {
    let transport = Transport::Memory(hub.clone());
    let target = transport.parse(ticket).expect("ticket");
    let link = transport
      .dialer()
      .await
      .expect("dialer")
      .connect(&target)
      .await
      .expect("connect");
    Self { link }
  }

  async fn write_json(&mut self, json: &str) {
    protocol::write_frame(&mut self.link.writer, json.as_bytes())
      .await
      .expect("write");
  }

  async fn write(&mut self, message: &ClientMsg) {
    let body = protocol::encode(message).expect("encode");
    protocol::write_frame(&mut self.link.writer, &body)
      .await
      .expect("write");
  }

  async fn hello(&mut self, room: &str) {
    self
      .write(&ClientMsg::Hello {
        room: room.to_string(),
        name: "Brut".to_string(),
        app: "test".to_string(),
      })
      .await;
  }

  /// Prochain message, `None` à la fermeture du lien.
  async fn read(&mut self) -> Option<HostMsg> {
    let frame = tokio::time::timeout(WAIT, protocol::read_frame(&mut self.link.reader, MAX_FRAME))
      .await
      .expect("frame in time");
    match frame {
      Ok(Some(body)) => Some(protocol::decode(&body).expect("host message")),
      Ok(None) | Err(_) => None,
    }
  }

  async fn joined(hub: &Hub, ticket: &str) -> Self {
    let mut raw = Self::connect(hub, ticket).await;
    let room = room_of(hub, ticket);
    raw.hello(&room).await;
    assert!(matches!(raw.read().await, Some(HostMsg::Welcome { .. })));
    raw
  }
}

fn room_of(hub: &Hub, ticket: &str) -> String {
  Transport::Memory(hub.clone())
    .parse(ticket)
    .expect("ticket")
    .room
    .to_hex()
}

#[test]
fn every_message_survives_a_json_round_trip() {
  let participants = vec![
    Participant {
      id: 0,
      name: "Hote".into(),
      host: true,
    },
    Participant {
      id: 1,
      name: "Invité".into(),
      host: false,
    },
  ];
  let ops = vec![
    Op::Play,
    Op::Pause,
    Op::Seek { pos_ms: 42 },
    Op::Next { from: Some(3) },
    Op::Prev { from: None },
    Op::Add {
      tracks: vec![wire("a")],
      next: true,
    },
    Op::PlayNow {
      tracks: vec![wire("b")],
    },
    Op::PlayEntry { uid: 9 },
    Op::Remove { uids: vec![1, 2] },
    Op::Move { uid: 4, to: 0 },
    Op::Clear,
    Op::Shuffle { on: true },
    Op::Repeat { on: false },
  ];
  let mut client = vec![
    ClientMsg::Hello {
      room: "00".repeat(16),
      name: "x".into(),
      app: "riff-rs/test".into(),
    },
    ClientMsg::Report {
      kind: ReportKind::ResolveFailed,
      uid: 5,
    },
    ClientMsg::Ping { t0: 77 },
    ClientMsg::Bye,
  ];
  client.extend(
    ops
      .into_iter()
      .zip(1u64..)
      .map(|(op, id)| ClientMsg::Cmd { id, op }),
  );
  for message in client {
    let body = protocol::encode(&message).expect("encode");
    assert_eq!(
      protocol::decode::<ClientMsg>(&body).expect("decode"),
      message
    );
  }
  let host = vec![
    HostMsg::Welcome {
      you: 1,
      host_ms: 1234,
      participants: participants.clone(),
      queue: queue_of(&["a", "b"]),
      playback: playback(true),
    },
    HostMsg::Refused {
      reason: Refusal::Full,
    },
    HostMsg::Queue(queue_of(&["a"])),
    HostMsg::Playback(playback(false)),
    HostMsg::Participants { list: participants },
    HostMsg::Notice(Notice {
      kind: NoticeKind::Added,
      by: 1,
      name: "Invité".into(),
      title: Some("Titre".into()),
      count: None,
    }),
    HostMsg::Notice(Notice {
      kind: NoticeKind::Added,
      by: 1,
      name: "Invité".into(),
      title: None,
      count: Some(3),
    }),
    HostMsg::Rejected {
      id: 8,
      reason: RejectReason::QueueFull,
    },
    HostMsg::Pong { t0: 1, t1: 2 },
    HostMsg::End {
      reason: EndReason::Ended,
    },
  ];
  for message in host {
    let body = protocol::encode(&message).expect("encode");
    assert_eq!(protocol::decode::<HostMsg>(&body).expect("decode"), message);
  }
}

#[test]
fn the_envelope_carries_the_version_and_the_type_tag() {
  let body = protocol::encode(&ClientMsg::Ping { t0: 5 }).expect("encode");
  let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
  assert_eq!(json["v"], 1);
  assert_eq!(json["t"], "ping");
  assert_eq!(json["t0"], 5);
}

#[test]
fn another_version_is_refused_and_an_unknown_type_is_ignored() {
  assert!(matches!(
    protocol::decode::<ClientMsg>(br#"{"v":2,"t":"bye"}"#),
    Err(DecodeError::Version(2))
  ));
  assert!(matches!(
    protocol::decode::<ClientMsg>(br#"{"t":"bye"}"#),
    Err(DecodeError::Version(0))
  ));
  assert_eq!(
    protocol::decode::<ClientMsg>(br#"{"v":1,"t":"from_the_future","x":1}"#).expect("decode"),
    ClientMsg::Unknown
  );
  assert_eq!(
    protocol::decode::<HostMsg>(br#"{"v":1,"t":"from_the_future"}"#).expect("decode"),
    HostMsg::Unknown
  );
  assert!(matches!(
    protocol::decode::<ClientMsg>(b"not json"),
    Err(DecodeError::Json(_))
  ));
}

#[tokio::test]
async fn frames_are_bounded_and_checked() {
  let mut empty: &[u8] = &[0, 0, 0, 0];
  assert!(matches!(
    protocol::read_frame(&mut empty, 100).await,
    Err(FrameError::Empty)
  ));
  // Seul l'en-tête est présent : une lecture du corps échouerait en `Truncated`.
  let mut huge: &[u8] = &101u32.to_be_bytes();
  assert!(matches!(
    protocol::read_frame(&mut huge, 100).await,
    Err(FrameError::TooLarge(101))
  ));
  let mut cut: &[u8] = &[0, 0, 0, 10, b'a', b'b', b'c'];
  assert!(matches!(
    protocol::read_frame(&mut cut, 100).await,
    Err(FrameError::Truncated)
  ));
  let mut half_header: &[u8] = &[0, 0];
  assert!(matches!(
    protocol::read_frame(&mut half_header, 100).await,
    Err(FrameError::Truncated)
  ));
  let mut end: &[u8] = &[];
  assert!(matches!(
    protocol::read_frame(&mut end, 100).await,
    Ok(None)
  ));
  let mut wire = Vec::new();
  protocol::write_frame(&mut wire, b"{}")
    .await
    .expect("write");
  protocol::write_frame(&mut wire, b"[1]")
    .await
    .expect("write");
  let mut reader: &[u8] = &wire;
  assert_eq!(
    protocol::read_frame(&mut reader, 100).await.expect("read"),
    Some(b"{}".to_vec())
  );
  assert_eq!(
    protocol::read_frame(&mut reader, 100).await.expect("read"),
    Some(b"[1]".to_vec())
  );
  assert!(matches!(
    protocol::read_frame(&mut reader, 100).await,
    Ok(None)
  ));
}

#[test]
fn the_rate_limit_counts_frames_per_second() {
  let start = Instant::now();
  let mut limit = RateLimit::new(3);
  assert!((0..3).all(|_| limit.allow(start)));
  assert!(!limit.allow(start));
  assert!(limit.allow(start + Duration::from_millis(1100)));
}

#[test]
fn received_tracks_are_sanitized() {
  let base = wire("abc");
  let long = "é".repeat(MAX_TEXT_CHARS + 50);
  let track = WireTrack {
    title: Some(long),
    artist: Some("Art\u{0}iste\u{7}".to_string()),
    thumbnail: Some("file:///etc/passwd".to_string()),
    cover: Some("https://example.com/c.jpg".to_string()),
    url: Some(format!("https://example.com/{}", "a".repeat(MAX_URL_CHARS))),
    duration: Some(f64::NAN),
    ..base.clone()
  }
  .sanitized()
  .expect("usable");
  assert_eq!(
    track.title.as_deref().map(|t| t.chars().count()),
    Some(MAX_TEXT_CHARS)
  );
  assert_eq!(track.artist.as_deref(), Some("Artiste"));
  assert_eq!(track.thumbnail, None);
  assert_eq!(track.cover.as_deref(), Some("https://example.com/c.jpg"));
  assert_eq!(track.url, None);
  assert_eq!(track.duration, None);

  let http = WireTrack {
    thumbnail: Some("http://example.com/t.png".to_string()),
    duration: Some(215.5),
    ..base.clone()
  }
  .sanitized()
  .expect("usable");
  assert_eq!(http.thumbnail.as_deref(), Some("http://example.com/t.png"));
  assert_eq!(http.duration, Some(215.5));
  let too_long = WireTrack {
    duration: Some(86_401.0),
    ..base.clone()
  }
  .sanitized()
  .expect("usable");
  assert_eq!(too_long.duration, None);

  assert!(
    WireTrack {
      platform: "local".to_string(),
      ..base.clone()
    }
    .sanitized()
    .is_none()
  );
  assert!(
    WireTrack {
      platform: String::new(),
      ..base.clone()
    }
    .sanitized()
    .is_none()
  );
  assert!(
    WireTrack {
      platform: "p".repeat(33),
      ..base.clone()
    }
    .sanitized()
    .is_none()
  );
  let nothing = WireTrack {
    platform: "youtube".to_string(),
    artist: Some("Seul".to_string()),
    ..WireTrack::default()
  };
  assert!(nothing.sanitized().is_none());
  let oversized_id = WireTrack {
    id: Some("i".repeat(MAX_ID_CHARS + 1)),
    ..WireTrack::default()
  };
  assert!(
    WireTrack {
      platform: "youtube".to_string(),
      ..oversized_id
    }
    .sanitized()
    .is_none()
  );
}

#[test]
fn received_urls_never_target_the_local_machine() {
  for bad in [
    "http://127.0.0.1:9/api/x",
    "http://localhost:9/api/google-logout",
    "http://LOCALHOST./api/x",
    "http://a.localhost/x",
    "http://0.0.0.0/x",
    "http://10.0.0.5/x",
    "http://192.168.1.1/x",
    "http://169.254.169.254/x",
    "http://2130706433/x",
    "http://[::1]/x",
    "http://[::]/x",
    "http://[fe80::1]/x",
    "http://[fd00::1]/x",
    "http://[::ffff:127.0.0.1]/x",
    "ftp://example.com/x",
    "https://",
  ] {
    let track = WireTrack {
      thumbnail: Some(bad.to_string()),
      cover: Some(bad.to_string()),
      url: Some(bad.to_string()),
      ..wire("abc")
    }
    .sanitized()
    .expect("usable");
    assert_eq!(track.thumbnail, None, "{bad}");
    assert_eq!(track.cover, None, "{bad}");
    assert_eq!(track.url, None, "{bad}");
  }
  let ok = WireTrack {
    thumbnail: Some("https://[2606:4700::1]/t.png".to_string()),
    ..wire("abc")
  }
  .sanitized()
  .expect("usable");
  assert!(ok.thumbnail.is_some());
}

#[test]
fn a_wire_track_converts_back_to_a_track_with_its_platform() {
  let track = WireTrack {
    artist: Some("Art".to_string()),
    duration: Some(12.0),
    ..wire("abc")
  }
  .into_track();
  assert_eq!(track.platform.as_deref(), Some("youtube"));
  assert_eq!(track.id.as_deref(), Some("abc"));
  assert_eq!(track.artist.as_deref(), Some("Art"));
  assert_eq!(track.path, None);
}

#[test]
fn names_are_cleaned_and_made_unique() {
  assert_eq!(sanitize_name("  \u{0}  "), "Guest");
  assert_eq!(
    sanitize_name(&"n".repeat(50)).chars().count(),
    MAX_NAME_CHARS
  );
  assert_eq!(sanitize_name(" Bob\n"), "Bob");
  let taken = ["Bob", "Bob (2)"];
  assert_eq!(unique_name("Alice", taken.iter().copied()), "Alice");
  assert_eq!(unique_name("Bob", taken.iter().copied()), "Bob (3)");
  let long = "n".repeat(MAX_NAME_CHARS);
  let renamed = unique_name(&long, std::iter::once(long.as_str()));
  assert_eq!(renamed.chars().count(), MAX_NAME_CHARS);
  assert!(renamed.ends_with(" (2)"));
}

#[test]
fn from_track_picks_the_platform_like_the_resolver() {
  let local = Track {
    platform: Some("local".to_string()),
    path: Some("C:/secret/song.mp3".to_string()),
    ..Track::default()
  };
  assert_eq!(WireTrack::from_track(&local, "youtube"), None);

  let bare = Track {
    id: Some("123456789".to_string()),
    url: Some("https://soundcloud.com/a/b".to_string()),
    ..Track::default()
  };
  let wire = WireTrack::from_track(&bare, "soundcloud").expect("remote");
  assert_eq!(wire.platform, "soundcloud");

  let ytmusic = Track {
    id: Some("alb_1".to_string()),
    platform: Some("ytmusic".to_string()),
    ..Track::default()
  };
  assert_eq!(
    WireTrack::from_track(&ytmusic, "soundcloud")
      .expect("remote")
      .platform,
    "ytmusic"
  );

  let youtube_in_soundcloud = Track {
    id: Some("dQw4w9WgXcQ".to_string()),
    url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string()),
    ..Track::default()
  };
  assert_eq!(
    WireTrack::from_track(&youtube_in_soundcloud, "soundcloud")
      .expect("remote")
      .platform,
    "soundcloud"
  );
  assert_eq!(
    WireTrack::from_track(&bare, "youtube").map(|w| w.platform),
    Some("youtube".to_string())
  );
  assert_eq!(WireTrack::from_track(&Track::default(), ""), None);
}

#[test]
fn a_local_path_never_reaches_the_wire() {
  let track = Track {
    id: Some("x".to_string()),
    title: Some("T".to_string()),
    platform: Some("youtube".to_string()),
    path: Some("C:/secret/song.mp3".to_string()),
    ..Track::default()
  };
  let wire = WireTrack::from_track(&track, "youtube").expect("remote");
  let json = serde_json::to_string(&wire).expect("json");
  assert!(!json.contains("secret"));
}

#[test]
fn the_memory_transport_is_confidential_and_parses_its_tickets() {
  let transport = Transport::Memory(Hub::new());
  assert_eq!(transport.security(), Security::Confidential);
  assert!(transport.parse("riffjam123").is_err());
  assert!(transport.parse("riffmem1-zz").is_err());
  assert!(transport.parse("riffmem-00").is_err());
  let room = Room([7; 16]);
  assert_eq!(Room::from_hex(&room.to_hex()), Some(room));
  assert_eq!(Room::from_hex(&"A".repeat(32)), None);
  assert!(room.ct_eq(&Room([7; 16])));
  assert!(!room.ct_eq(&Room([8; 16])));
}

#[tokio::test]
async fn a_guest_with_the_right_room_gets_the_cached_state() {
  let hub = Hub::new();
  let (host, ticket) = host(&hub).await;
  let mut first = guest(&hub, &ticket, "Alice");
  first.joined().await;
  host.send(JamOut::Queue(queue_of(&["a", "b"])));
  host.send(JamOut::Playback {
    state: Box::new(playback(true)),
    at: Instant::now(),
  });
  // Le premier invité a reçu les deux états : la tâche de l'hôte les a en cache.
  first
    .until(|event| matches!(event, JamEvent::Playback { .. }).then_some(()))
    .await;
  let mut second = guest(&hub, &ticket, "Bob");
  let (you, participants, queue, state, _) = second.joined().await;
  assert_eq!(you, 2);
  assert_eq!(participants.len(), 3);
  assert!(participants[0].host);
  assert_eq!(queue.entries.len(), 2);
  assert_eq!(state.pos_ms, 10_000);
  assert!(state.playing);
}

#[tokio::test]
async fn a_wrong_room_is_refused_then_closed() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut raw = Raw::connect(&hub, &ticket).await;
  raw.hello(&"0".repeat(32)).await;
  assert_eq!(
    raw.read().await,
    Some(HostMsg::Refused {
      reason: Refusal::Room
    })
  );
  assert_eq!(raw.read().await, None);
  let mut malformed = Raw::connect(&hub, &ticket).await;
  malformed.hello("not-a-room").await;
  assert_eq!(
    malformed.read().await,
    Some(HostMsg::Refused {
      reason: Refusal::Room
    })
  );
}

#[tokio::test]
async fn another_protocol_version_is_refused_at_the_handshake() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut raw = Raw::connect(&hub, &ticket).await;
  raw
    .write_json(r#"{"v":2,"t":"hello","room":"","name":"x","app":"y"}"#)
    .await;
  assert_eq!(
    raw.read().await,
    Some(HostMsg::Refused {
      reason: Refusal::Version
    })
  );
}

#[tokio::test]
async fn a_silent_link_is_closed_after_the_handshake_delay() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut raw = Raw::connect(&hub, &ticket).await;
  let started = Instant::now();
  assert_eq!(raw.read().await, None);
  assert!(started.elapsed() >= Duration::from_millis(250));
}

#[tokio::test]
async fn a_first_message_other_than_hello_closes_the_link() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut raw = Raw::connect(&hub, &ticket).await;
  raw
    .write(&ClientMsg::Cmd {
      id: 1,
      op: Op::Play,
    })
    .await;
  assert_eq!(raw.read().await, None);
  let mut oversized = Raw::connect(&hub, &ticket).await;
  let junk = format!(
    r#"{{"v":1,"t":"hello","room":"{}"}}"#,
    "a".repeat(MAX_HELLO_FRAME)
  );
  oversized.write_json(&junk).await;
  assert_eq!(oversized.read().await, None);
}

#[tokio::test]
async fn the_ninth_participant_is_refused_as_full() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut guests = Vec::new();
  for index in 0..MAX_PARTICIPANTS - 1 {
    let mut peer = guest(&hub, &ticket, &format!("g{index}"));
    peer.joined().await;
    guests.push(peer);
  }
  let mut raw = Raw::connect(&hub, &ticket).await;
  raw.hello(&room_of(&hub, &ticket)).await;
  assert_eq!(
    raw.read().await,
    Some(HostMsg::Refused {
      reason: Refusal::Full
    })
  );
  let mut late = guest(&hub, &ticket, "late");
  assert_eq!(late.ended().await, EndCause::Refused(Refusal::Full));
}

#[tokio::test]
async fn an_unknown_message_after_admission_is_ignored() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut raw = Raw::joined(&hub, &ticket).await;
  raw
    .write_json(r#"{"v":1,"t":"from_the_future","x":1}"#)
    .await;
  raw.write(&ClientMsg::Ping { t0: 3 }).await;
  match raw.read().await {
    Some(HostMsg::Pong { t0, .. }) => assert_eq!(t0, 3),
    other => panic!("pong attendu, reçu {other:?}"),
  }
}

#[tokio::test]
async fn a_guest_command_reaches_the_host_and_state_reaches_every_guest() {
  let hub = Hub::new();
  let (mut host, ticket) = host(&hub).await;
  let mut alice = guest(&hub, &ticket, "Alice");
  let (alice_id, ..) = alice.joined().await;
  let mut bob = guest(&hub, &ticket, "Bob");
  bob.joined().await;

  let op = Op::Add {
    tracks: vec![wire("x")],
    next: false,
  };
  alice.send(JamOut::Command {
    id: 11,
    op: op.clone(),
  });
  let (from, id, got) = host
    .until(|event| match event {
      JamEvent::GuestOp { from, id, op } => Some((*from, *id, op.clone())),
      _ => None,
    })
    .await;
  assert_eq!((from, id, got), (alice_id, 11, op));

  alice.send(JamOut::Report { uid: 6 });
  let reported = host
    .until(|event| match event {
      JamEvent::GuestReport { from, uid } => Some((*from, *uid)),
      _ => None,
    })
    .await;
  assert_eq!(reported, (alice_id, 6));

  host.send(JamOut::Queue(queue_of(&["a", "b", "c"])));
  for peer in [&mut alice, &mut bob] {
    let queue = peer
      .until(|event| match event {
        JamEvent::Queue(queue) => Some(queue.clone()),
        _ => None,
      })
      .await;
    assert_eq!(queue.entries.len(), 3);
  }

  let at = Instant::now();
  host.send(JamOut::Playback {
    state: Box::new(playback(false)),
    at,
  });
  for peer in [&mut alice, &mut bob] {
    let (state, anchor) = peer
      .until(|event| match event {
        JamEvent::Playback { state, anchor } => Some((state.clone(), *anchor)),
        _ => None,
      })
      .await;
    assert!(!state.playing);
    assert_eq!(state.pos_ms, 10_000);
    let gap = anchor.max(at) - anchor.min(at);
    assert!(gap < Duration::from_millis(250), "ancre décalée de {gap:?}");
  }

  host.send(JamOut::Reject {
    to: alice_id,
    id: 11,
    reason: RejectReason::QueueFull,
  });
  let rejected = alice
    .until(|event| match event {
      JamEvent::Rejected { id, reason } => Some((*id, *reason)),
      _ => None,
    })
    .await;
  assert_eq!(rejected, (11, RejectReason::QueueFull));
}

#[tokio::test]
async fn a_leaving_guest_is_announced_to_the_others() {
  let hub = Hub::new();
  let (mut host, ticket) = host(&hub).await;
  let mut alice = guest(&hub, &ticket, "Alice");
  alice.joined().await;
  let mut bob = guest(&hub, &ticket, "Bob");
  bob.joined().await;
  host
    .until(|event| match event {
      JamEvent::Participants(list) if list.len() == 3 => Some(()),
      _ => None,
    })
    .await;

  alice.send(JamOut::Stop);
  assert_eq!(alice.ended().await, EndCause::Stopped);
  let list = bob
    .until(|event| match event {
      JamEvent::Participants(list) if list.len() == 2 => Some(list.clone()),
      _ => None,
    })
    .await;
  assert_eq!(list[1].name, "Bob");
  let notice = bob
    .until(|event| match event {
      JamEvent::Notice(notice) if notice.kind == NoticeKind::Left => Some(notice.clone()),
      _ => None,
    })
    .await;
  assert_eq!(notice.name, "Alice");
  let notice = host
    .until(|event| match event {
      JamEvent::Notice(notice) if notice.kind == NoticeKind::Left => Some(notice.clone()),
      _ => None,
    })
    .await;
  assert_eq!(notice.name, "Alice");
}

#[tokio::test]
async fn duplicate_names_get_a_suffix() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut first = guest(&hub, &ticket, "Alice");
  first.joined().await;
  let mut second = guest(&hub, &ticket, "Alice");
  let (_, participants, ..) = second.joined().await;
  let names: Vec<_> = participants.iter().map(|p| p.name.as_str()).collect();
  assert_eq!(names, ["Hote", "Alice", "Alice (2)"]);
}

#[tokio::test]
async fn the_host_ending_the_jam_ends_every_guest_without_reconnecting() {
  let hub = Hub::new();
  let (mut host, ticket) = host(&hub).await;
  let mut alice = guest(&hub, &ticket, "Alice");
  alice.joined().await;
  let mut bob = guest(&hub, &ticket, "Bob");
  bob.joined().await;

  host.send(JamOut::Stop);
  assert_eq!(host.ended().await, EndCause::Stopped);
  for peer in [&mut alice, &mut bob] {
    loop {
      match peer.next().await {
        JamEvent::Ended(cause) => {
          assert_eq!(cause, EndCause::HostEnded);
          break;
        }
        JamEvent::Reconnecting { .. } => panic!("pas de reconnexion après `end`"),
        _ => {}
      }
    }
  }
  assert!(
    tokio::time::timeout(WAIT, host.task)
      .await
      .expect("host task ends")
      .is_ok()
  );
}

#[tokio::test]
async fn a_cut_link_reconnects_and_joins_again() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut alice = guest(&hub, &ticket, "Alice");
  alice.joined().await;

  hub.cut_all();
  let attempt = alice
    .until(|event| match event {
      JamEvent::Reconnecting { attempt } => Some(*attempt),
      _ => None,
    })
    .await;
  assert_eq!(attempt, 1);
  let (_, participants, ..) = alice.joined().await;
  assert_eq!(participants.len(), 2);
  assert_eq!(participants[1].name, "Alice");
}

#[tokio::test]
async fn a_vanished_host_ends_the_guest_after_three_attempts() {
  let hub = Hub::new();
  let (host, ticket) = host(&hub).await;
  let mut alice = guest(&hub, &ticket, "Alice");
  alice.joined().await;

  host.task.abort();
  let mut attempts = Vec::new();
  let cause = loop {
    match alice.next().await {
      JamEvent::Reconnecting { attempt } => attempts.push(attempt),
      JamEvent::Ended(cause) => break cause,
      _ => {}
    }
  };
  assert_eq!(attempts, [1, 2, 3]);
  assert_eq!(cause, EndCause::Lost);
}

#[tokio::test]
async fn a_malformed_ticket_ends_the_guest_at_once() {
  let hub = Hub::new();
  let mut alice = guest(&hub, "riffjam-nothing", "Alice");
  assert_eq!(alice.ended().await, EndCause::InvalidTicket);
}

#[tokio::test]
async fn a_closed_room_is_unreachable_for_a_new_guest() {
  let hub = Hub::new();
  let (mut host, ticket) = host(&hub).await;
  host.send(JamOut::Stop);
  host.ended().await;
  let mut late = guest(&hub, &ticket, "Late");
  assert!(matches!(late.ended().await, EndCause::Transport(_)));
}

#[tokio::test]
async fn a_guest_over_the_frame_rate_is_cut() {
  let hub = Hub::new();
  let (mut host, ticket) = host(&hub).await;
  let mut raw = Raw::joined(&hub, &ticket).await;
  for _ in 0..=MAX_FRAMES_PER_SEC + 10 {
    raw.write(&ClientMsg::Ping { t0: 1 }).await;
  }
  // Les `pong` arrivent, puis le lien se ferme.
  while raw.read().await.is_some() {}
  host
    .until(|event| match event {
      JamEvent::Notice(notice) if notice.kind == NoticeKind::Left => Some(()),
      _ => None,
    })
    .await;
}

#[tokio::test]
async fn ping_is_answered_by_pong_and_pong_makes_no_event() {
  let hub = Hub::new();
  let (_host, ticket) = host(&hub).await;
  let mut raw = Raw::joined(&hub, &ticket).await;
  raw.write(&ClientMsg::Ping { t0: 41 }).await;
  match raw.read().await {
    Some(HostMsg::Pong { t0, .. }) => assert_eq!(t0, 41),
    other => panic!("pong attendu, reçu {other:?}"),
  }

  let mut alice = guest(&hub, &ticket, "Alice");
  alice.joined().await;
  // Plusieurs `ping` partent pendant ce délai ; les `pong` ne produisent aucun événement.
  tokio::time::sleep(Duration::from_millis(300)).await;
  assert!(alice.events.try_recv().is_err());
}

#[test]
fn the_inbox_is_taken_once() {
  let (link, _task) = start_host(Transport::Memory(Hub::new()), "Hote".to_string(), 3);
  assert_eq!(link.inbox.generation, 3);
  assert!(link.inbox.take().is_some());
  assert!(link.inbox.take().is_none());
}

#[test]
fn the_clock_keeps_the_offset_of_the_smallest_round_trip() {
  let mut clock = ClockSync::default();
  assert_eq!(clock.offset(), None);
  // t0 = 1000, t2 = 1100 : rtt 100 ; t1 = 1550 : offset = 1550 - (1000 + 50) = 500.
  clock.add(1000, 1550, 1100);
  assert_eq!(clock.offset(), Some(500));
  // rtt 20, offset 480 : plus précis.
  clock.add(2000, 2490, 2020);
  assert_eq!(clock.offset(), Some(480));
  // rtt plus grand : ne remplace pas.
  clock.add(3000, 3700, 3400);
  assert_eq!(clock.offset(), Some(480));
  // t2 < t0 : ignoré.
  clock.add(5000, 0, 4000);
  assert_eq!(clock.offset(), Some(480));
}

#[test]
fn the_clock_seed_is_replaced_by_a_real_measure() {
  let mut clock = ClockSync::default();
  clock.seed(10_000, 4_000);
  assert_eq!(clock.offset(), Some(6_000));
  clock.add(100, 6_150, 120);
  assert_eq!(clock.offset(), Some(6_040));
  let mut negative = ClockSync::default();
  negative.seed(1_000, 4_000);
  assert_eq!(negative.offset(), Some(-3_000));
}

#[test]
fn the_clock_keeps_eight_samples() {
  let mut clock = ClockSync::default();
  clock.add(0, 1000, 10);
  for index in 1..=8 {
    clock.add(index * 100, index * 100 + 50, index * 100 + 50);
  }
  // L'échantillon le plus précis est sorti de la fenêtre.
  assert_eq!(clock.offset(), Some(25));
}

#[test]
fn host_time_maps_to_a_local_instant_with_either_offset_sign() {
  let epoch = Instant::now();
  let now = epoch + Duration::from_secs(60);
  // L'hôte est en avance de 500 ms : 2 000 ms chez l'hôte = 1 500 ms chez l'invité.
  assert_eq!(
    sync::host_to_local(2_000, 500, epoch, now),
    epoch + Duration::from_millis(1_500)
  );
  // L'hôte est en retard de 500 ms.
  assert_eq!(
    sync::host_to_local(2_000, -500, epoch, now),
    epoch + Duration::from_millis(2_500)
  );
  // Avant l'origine de l'invité : repli sur `now`.
  assert_eq!(sync::host_to_local(100, 500, epoch, now), now);
}

#[test]
fn the_expected_position_follows_the_anchor_and_is_clamped() {
  let anchor = Instant::now();
  let now = anchor + Duration::from_secs(2);
  assert_eq!(
    sync::expected_position(10_000, anchor, true, now, None),
    Duration::from_secs(12)
  );
  assert_eq!(
    sync::expected_position(10_000, anchor, false, now, None),
    Duration::from_secs(10)
  );
  assert_eq!(
    sync::expected_position(10_000, anchor, true, now, Some(Duration::from_secs(11))),
    Duration::from_secs(11)
  );
  // Ancre dans le futur : la position n'a pas encore atteint `pos_ms`.
  assert_eq!(
    sync::expected_position(10_000, now, true, anchor, None),
    Duration::from_secs(8)
  );
}

#[test]
fn a_correction_needs_a_gap_and_an_expired_cooldown() {
  let now = Instant::now();
  let at = Duration::from_secs;
  assert!(sync::needs_correction(at(10), at(12), None, now));
  assert!(sync::needs_correction(at(12), at(10), None, now));
  assert!(!sync::needs_correction(
    at(10),
    Duration::from_millis(11_400),
    None,
    now
  ));
  assert!(!sync::needs_correction(
    at(10),
    Duration::from_millis(11_500),
    None,
    now
  ));
  assert!(sync::needs_correction(
    at(10),
    Duration::from_millis(11_501),
    None,
    now
  ));
  let recent = now.checked_sub(Duration::from_secs(2));
  assert!(!sync::needs_correction(at(10), at(20), recent, now));
  let old = now.checked_sub(CORRECTION_COOLDOWN + Duration::from_millis(1));
  assert!(sync::needs_correction(at(10), at(20), old, now));
}

#[test]
fn the_host_position_extrapolates_for_one_second_at_most() {
  let now = Instant::now();
  let position = Duration::from_secs(30);
  let at = |ms| now.checked_sub(Duration::from_millis(ms));
  assert_eq!(
    sync::host_position(position, at(400), true, now),
    Duration::from_millis(30_400)
  );
  assert_eq!(
    sync::host_position(position, at(5_000), true, now),
    Duration::from_secs(31)
  );
  assert_eq!(sync::host_position(position, at(400), false, now), position);
  assert_eq!(sync::host_position(position, None, true, now), position);
}

#[tokio::test]
async fn host_states_are_coalesced_and_notices_are_budgeted() {
  let hub = Hub::new();
  let (host, ticket) = host(&hub).await;
  let mut raw = Raw::joined(&hub, &ticket).await;
  for pv in 1..=40 {
    host.send(JamOut::Playback {
      state: Box::new(PlaybackState {
        pv,
        ..playback(true)
      }),
      at: Instant::now(),
    });
    host.send(JamOut::Notice(Notice {
      kind: NoticeKind::Paused,
      by: 0,
      name: "Hote".to_string(),
      title: None,
      count: None,
    }));
  }
  let (mut states, mut notices) = (0, 0);
  loop {
    match raw.read().await {
      Some(HostMsg::Playback(state)) => {
        states += 1;
        if state.pv == 40 {
          break;
        }
      }
      Some(HostMsg::Notice(_)) => notices += 1,
      other => assert!(other.is_some(), "lien fermé avant le dernier état"),
    }
  }
  assert!(states < 40, "{states} trames pour 40 états");
  assert!(notices <= MAX_NOTICES_PER_SEC, "{notices} notices");
}
