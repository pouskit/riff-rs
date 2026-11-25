//! Fonctions utilitaires communes.

use crate::net::Http;
use regex::Regex;
use reqwest::header;
use serde_json::{Map, Number, Value, json};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn clean_track_id(id: &str) -> String {
  id.chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
        c
      } else {
        '_'
      }
    })
    .collect()
}

pub fn is_truthy(val: Option<&Value>) -> bool {
  match val {
    None | Some(Value::Null) => false,
    Some(Value::Bool(b)) => *b,
    Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
    Some(Value::String(s)) => !s.is_empty(),
    Some(Value::Array(_)) | Some(Value::Object(_)) => true,
  }
}

pub fn value_to_string(val: Option<&Value>) -> String {
  match val {
    None | Some(Value::Null) => String::new(),
    Some(Value::String(s)) => s.clone(),
    Some(Value::Bool(b)) => b.to_string(),
    Some(Value::Number(n)) => n.to_string(),
    Some(other) => other.to_string(),
  }
}

pub fn normalize_number_value(val: Option<&Value>) -> Value {
  let Some(v) = val else {
    return json!(0);
  };
  if let Some(i) = v.as_i64() {
    return json!(i);
  }
  if let Some(u) = v.as_u64() {
    return json!(u);
  }
  if let Some(f) = v.as_f64() {
    if !f.is_finite() || f == 0.0 {
      return json!(0);
    }
    if f.fract() == 0.0 && f >= (i64::MIN as f64) && f <= (i64::MAX as f64) {
      return json!(f as i64);
    }
    if let Some(num) = Number::from_f64(f) {
      return Value::Number(num);
    }
  }
  if let Some(s) = v.as_str()
    && let Ok(f) = s.trim().parse::<f64>()
    && f.is_finite()
    && f != 0.0
  {
    if f.fract() == 0.0 && f >= (i64::MIN as f64) && f <= (i64::MAX as f64) {
      return json!(f as i64);
    }
    if let Some(num) = Number::from_f64(f) {
      return Value::Number(num);
    }
  }
  json!(0)
}

pub fn now_millis() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0)
}

pub fn encode_uri_component(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  for b in s.as_bytes() {
    match *b {
      b'A'..=b'Z'
      | b'a'..=b'z'
      | b'0'..=b'9'
      | b'-'
      | b'_'
      | b'.'
      | b'!'
      | b'~'
      | b'*'
      | b'\''
      | b'('
      | b')' => out.push(*b as char),
      other => out.push_str(&format!("%{other:02X}")),
    }
  }
  out
}

pub fn sha1_hex(input: &str) -> String {
  let mut hasher = Sha1::new();
  hasher.update(input.as_bytes());
  let digest = hasher.finalize();
  let mut hex = String::with_capacity(40);
  for b in digest {
    hex.push_str(&format!("{b:02x}"));
  }
  hex
}

pub fn random_f64() -> f64 {
  let mut buf = [0u8; 8];
  let _ = getrandom::fill(&mut buf);
  let u = u64::from_le_bytes(buf) >> 11;
  (u as f64) * (1.0 / ((1u64 << 53) as f64))
}

pub fn random_cand_id() -> String {
  const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
  let mut buf = [0u8; 8];
  let _ = getrandom::fill(&mut buf);
  let mut out = String::with_capacity(13);
  out.push_str("cand_");
  for b in buf {
    out.push(CHARS[(b as usize) % CHARS.len()] as char);
  }
  out
}

pub fn shuffle_slice<T>(slice: &mut [T]) {
  let len = slice.len();
  if len <= 1 {
    return;
  }
  for i in (1..len).rev() {
    let j = (random_f64() * ((i + 1) as f64)).floor() as usize;
    slice.swap(i, j.min(i));
  }
}

pub fn read_json_map(path: &Path) -> Map<String, Value> {
  if let Ok(raw) = fs::read_to_string(path)
    && let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&raw)
  {
    return map;
  }
  Map::new()
}

pub fn write_json_map(path: &Path, map: &Map<String, Value>) {
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  if let Ok(text) = serde_json::to_string_pretty(&Value::Object(map.clone())) {
    let _ = fs::write(path, text);
  }
}

pub fn strip_youtube_topic(artist: &str) -> String {
  let trimmed_end = artist.strip_suffix(" - Topic").unwrap_or(artist);
  if trimmed_end.len() != artist.len() {
    trimmed_end.trim().to_string()
  } else {
    artist.to_string()
  }
}

pub fn is_text_explicit(text: &str) -> bool {
  if text.is_empty() {
    return false;
  }
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| {
    Regex::new(r"(?i)\bexplicit\b|\[e\]|\(e\)|\[explicit\]|\(explicit\)|\b18\+|\[18\+\]|\(18\+\)|\+18|\[\+18\]|\(\+18\)|\buncensored\b|\[uncensored\]|\(uncensored\)|\bdirty\b|\[dirty\]|\(dirty\)|без\s+цензуры|нецензурн|\bмат\b|\b(fuck|fucking|fucked|fucker|fucks|shit|bitch|bitches|bitching|asshole|cunt|nigger|nigga|dick|pussy)\b|ху[йеяёюи]|пизд|залуп|муда[кч]|мудил|г[ао]ндон|дерьм|дроч").expect("valid regex")
  });
  re.is_match(text)
}

pub async fn http_get_json(client: &Http, url: &str, timeout_ms: u64, ua: &str) -> Option<Value> {
  http_get_json_reached(client, url, timeout_ms, ua).await.0
}

// Comme `http_get_json`, avec en plus « une réponse HTTP est arrivée » (faux = échec de transport).
pub async fn http_get_json_reached(
  client: &Http,
  url: &str,
  timeout_ms: u64,
  ua: &str,
) -> (Option<Value>, bool) {
  let Ok(resp) = client
    .get(url)
    .header(header::USER_AGENT, ua)
    .timeout(Duration::from_millis(timeout_ms))
    .send()
    .await
  else {
    return (None, false);
  };
  if !resp.status().is_success() {
    return (None, true);
  }
  let Ok(bytes) = resp.bytes().await else {
    return (None, true);
  };
  if bytes.len() > 2 * 1024 * 1024 {
    return (None, true);
  }
  (serde_json::from_slice::<Value>(&bytes).ok(), true)
}

pub async fn fetch_itunes_json(client: &Http, url: &str) -> Option<Value> {
  http_get_json(client, url, 10_000, "Mozilla/5.0").await
}

pub fn collapse_alnum_spaces(s: &str) -> String {
  let mapped: String = s
    .chars()
    .map(|c| {
      if c.is_alphanumeric() || c.is_whitespace() {
        c
      } else {
        ' '
      }
    })
    .collect();
  mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn get_track_normalized_key(artist: &str, title: &str) -> String {
  static RE_T1: OnceLock<Regex> = OnceLock::new();
  static RE_T2: OnceLock<Regex> = OnceLock::new();
  static RE_T3: OnceLock<Regex> = OnceLock::new();
  static RE_T4: OnceLock<Regex> = OnceLock::new();
  static RE_T5: OnceLock<Regex> = OnceLock::new();
  static RE_A1: OnceLock<Regex> = OnceLock::new();

  let r_t1 = RE_T1.get_or_init(|| Regex::new(r"(?i)\s*\([^)]*official[^)]*\)").expect("regex"));
  let r_t2 = RE_T2.get_or_init(|| Regex::new(r"\s*\[[^\]]*\]").expect("regex"));
  let r_t3 = RE_T3.get_or_init(|| {
    Regex::new(r"(?i)\s*\([^)]*(video|audio|remaster(ed)?|lyrics?|visualizer)[^)]*\)")
      .expect("regex")
  });
  let r_t4 = RE_T4.get_or_init(|| Regex::new(r"(?i)\s*(feat\.?|ft\.?)\s+[^(\[-]+").expect("regex"));
  let r_t5 = RE_T5.get_or_init(|| Regex::new(r"(?i)\s*\((feat\.?|ft\.?)[^)]*\)").expect("regex"));
  let r_a1 = RE_A1.get_or_init(|| Regex::new(r"(?i)\s*(feat\.?|ft\.?)\s+.*$").expect("regex"));

  let a_lower = artist.to_lowercase();
  let t_lower = title.to_lowercase();

  let t1 = r_t1.replace_all(&t_lower, "");
  let t2 = r_t2.replace_all(&t1, "");
  let t3 = r_t3.replace_all(&t2, "");
  let t4 = r_t4.replace_all(&t3, "");
  let t5 = r_t5.replace_all(&t4, "");
  let a1 = r_a1.replace_all(&a_lower, "");

  let clean_a = collapse_alnum_spaces(&a1);
  let clean_t = collapse_alnum_spaces(&t5);
  format!("{clean_a}::{clean_t}")
}

pub fn parse_duration_seconds(s: &str) -> i64 {
  if s.is_empty() {
    return 0;
  }
  let parts: Vec<i64> = s
    .trim()
    .split(':')
    .map(|p| p.parse::<i64>().unwrap_or(0))
    .collect();
  if parts.len() == 2 {
    parts[0] * 60 + parts[1]
  } else if parts.len() == 3 {
    parts[0] * 3600 + parts[1] * 60 + parts[2]
  } else {
    0
  }
}

pub fn is_spam_or_unofficial(title: &str, duration: i64, channel_name: &str) -> bool {
  if duration != 0 && !(90..=420).contains(&duration) {
    return true;
  }
  let combined = format!("{} {}", title.to_lowercase(), channel_name.to_lowercase());
  static RE_NON_MUSIC: OnceLock<Regex> = OnceLock::new();
  let re = RE_NON_MUSIC.get_or_init(|| {
    Regex::new(r"(?i)\b(lecture|talk|conversation|in conversation|academy|masterclass|documentary|behind the scenes|making of|explained|history of|how to|news|live stream|livestream|full concert|full set|live at|official video essay|series|trailer|teaser|full movie|short film|podcast|gameplay|walkthrough|playthrough|review|reaction|interview|audiobook|vlog|unboxing|tutorial|parody)\b|\b(episode|season)\s*\d+|\bs\d+e\d+\b|\bep\s*\.?\s*\d+\b").expect("valid regex")
  });
  if re.is_match(&combined) {
    return true;
  }
  const BAD_KEYWORDS: &[&str] = &[
    "1 hour",
    "2 hours",
    "10 hours",
    "hour loop",
    "hours loop",
    "full album",
    "discography",
    "ost compilation",
    "soundtrack compilation",
    "playlist mix",
    "dj set",
    "type beat",
    "slowed + reverb",
    "slowed down",
    "sped up",
    "bass boosted",
    "nightcore",
    "reaction to",
    "reacting to",
  ];
  BAD_KEYWORDS.iter().any(|bad| combined.contains(bad))
}

pub fn lyrics_key(title: &str, artist: &str) -> String {
  let input = format!(
    "{}\0{}",
    artist.trim().to_lowercase(),
    title.trim().to_lowercase()
  );
  let mut hasher = Sha1::new();
  hasher.update(input.as_bytes());
  let digest = hasher.finalize();
  let mut hex = String::with_capacity(40);
  for b in digest {
    hex.push_str(&format!("{b:02x}"));
  }
  hex.chars().take(16).collect()
}

pub fn upgrade_artwork_url(u: &str) -> String {
  if u.is_empty() {
    return String::new();
  }
  static RE1: OnceLock<Regex> = OnceLock::new();
  static RE2: OnceLock<Regex> = OnceLock::new();
  static RE3: OnceLock<Regex> = OnceLock::new();
  static RE4: OnceLock<Regex> = OnceLock::new();
  let r1 = RE1.get_or_init(|| {
    Regex::new(r"(?i)/(\d{2,3})x(\d{2,3})(bb|cc|sr)?\.(jpg|png|webp)").expect("valid regex")
  });
  let r2 = RE2.get_or_init(|| Regex::new(r"(?i)/picture/([^/]+)/\d+x\d+-").expect("valid regex"));
  let r3 = RE3.get_or_init(|| Regex::new(r"(?i)=w\d+-h\d+[^&?]*").expect("valid regex"));
  let r4 = RE4
    .get_or_init(|| Regex::new(r"(?i)-(?:large|t500x500|crop)\.(jpg|png)").expect("valid regex"));

  let s1 = r1.replace(u, "/1200x1200bb.$4");
  let s2 = r2.replace(&s1, "/picture/$1/1000x1000-");
  let s3 = r3.replace(&s2, "=w1400-h1400-l90-rj");
  r4.replace(&s3, "-t1080x1080.$1").into_owned()
}
