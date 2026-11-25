use super::*;
use crate::core::LyricsCacheEntry;
use base64::Engine;
use std::sync::LazyLock;

const UA_MOZ: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";
const MM_BASE: &str = "https://apic-desktop.musixmatch.com/ws/1.1";

macro_rules! re {
  ($name:ident, $pat:expr) => {
    static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("regex lyrics"));
  };
}

re!(RE_BRACKETS, r"\[.*?\]|\(.*?\)");
re!(
  RE_TITLE_NOISE,
  r"(?i)official\s*(?:music)?\s*video|official\s*audio|lyrics|hd|4k|remastered|visualizer|clip\s*officiel|video\s*clip"
);
re!(RE_FEAT, r"(?i)ft\.?|feat\.?");
re!(RE_SYMBOLS, r"[^A-Za-z0-9_\s\x{0400}-\x{04FF}]");
re!(RE_SPACES, r"\s+");
re!(RE_ARTIST_NOISE, r"(?i)- topic|vevo|records|official");
re!(RE_SYNCED, r"\[\d{2}:\d{2}");
re!(
  RE_META_BRACKET,
  r"(?i)\s*[\(\[][^)\]]*(?:official|audio|video|lyrics?|prod|feat|ft|remaster|version|edit)[^)\]]*[\)\]]"
);
re!(RE_NON_ALNUM, r"[^\p{L}\p{N}]+");
re!(
  RE_MM_TITLE,
  r"(?i)\s*[\(\[][^\)\]]*(?:feat|ft|remix|official|audio)[^\)\]]*[\)\]]"
);
re!(RE_MM_ARTIST, r"(?i) - Topic|VEVO");
re!(
  RE_KUGOU_TAGS,
  r"(?im)^\[(?:id|ti|ar|al|by|hash|sign|qq|total|offset):[^\]]*\]\r?\n?"
);
re!(RE_FEAT_PAREN, r"(?i)\s*\(feat\.[^)]*\)");
re!(RE_FEAT_BRACKET, r"(?i)\s*\[feat\.[^\]]*\]");
re!(RE_FEATURING, r"(?i)\s*\(featuring[^)]*\)");
re!(RE_FEAT_BARE, r"(?i)\s*feat\.\S+");
re!(RE_REMASTER, r"(?i)\s*-\s*remaster(?:ed)?(?:\s*\d+)?");
re!(RE_LIVE_PAREN, r"(?i)\s*\(live[^)]*\)");
re!(RE_LIVE_BRACKET, r"(?i)\s*\[live[^\]]*\]");
re!(
  RE_INSTRUMENTAL_TITLE,
  r"(?i)(?-u:\b)(?:instrumental|type\s+beat)(?-u:\b)"
);
re!(
  RE_EXPLICIT,
  r"(?i)(?-u:\b)(?:fuck|fucking|fucked|fucker|fucks|shit|bitch|bitches|bitching|asshole|cunt|nigger|nigga|dick|pussy)(?-u:\b)|ху[йеяёюи]|пизд|залуп|муда[кч]|мудил|г[ао]ндон|дерьм|дроч|(?:^|[^а-яё])(?:бля[а-яё]*|сук[аиуео]|суч[екьяа][а-яё]*)|(?:^|[^а-яё])(?:[её]б[а-яё]+|[её]б(?-u:\b)|(?:вы|за|на|до|по|пере|про|разъ|раз|съ|въ|под|от|у)[её]б[а-яё]*)"
);
re!(RE_YRC_WORD, r"\((\d+),(\d+)(?:,\d+)?\)([^(]*)");
re!(RE_YRC_LINE, r"^\[(\d+),(\d+)\](.*)$");
re!(RE_CJK, r"[\x{4e00}-\x{9fa5}\x{3040}-\x{30ff}]");
re!(RE_BR, r"(?i)<br\s*/?>");
re!(RE_TAGS, r"<[^>]+>");
re!(RE_ENT_DEC, r"&#(\d+);");
re!(RE_ENT_HEX, r"(?i)&#x([0-9a-f]+);");
re!(RE_GENIUS_SECTION, r"(?i)\[[^\]]*\]");
re!(RE_NEWLINES3, r"\n{3,}");
re!(RE_CONTRIBUTORS, r"(?i)^\d*\s*Contributors");
re!(RE_LYRICS_LINE, r"(?i)^\[?.*Lyrics\]?$");

fn s(v: Option<&Value>) -> &str {
  v.and_then(Value::as_str).unwrap_or("")
}

fn first_text(a: Option<&Value>, b: Option<&Value>) -> String {
  let a = s(a);
  if a.is_empty() {
    s(b).to_string()
  } else {
    a.to_string()
  }
}

fn clean_query(title: &str, artist: &str) -> String {
  let mut q = format!("{title} {artist}");
  for re in [
    &RE_FEAT_PAREN,
    &RE_FEAT_BRACKET,
    &RE_FEATURING,
    &RE_FEAT_BARE,
    &RE_REMASTER,
    &RE_LIVE_PAREN,
    &RE_LIVE_BRACKET,
  ] {
    q = re.replace_all(&q, "").into_owned();
  }
  RE_SPACES.replace_all(&q, " ").trim().to_string()
}

fn sanitize_for_lyrics(title: &str, artist: &str) -> (String, String) {
  let mut t = RE_BRACKETS.replace_all(title, "").into_owned();
  t = RE_TITLE_NOISE.replace_all(&t, "").into_owned();
  t = RE_FEAT.replace_all(&t, "").into_owned();
  t = RE_SYMBOLS.replace_all(&t, " ").into_owned();
  t = RE_SPACES.replace_all(&t, " ").trim().to_string();

  let mut a = RE_BRACKETS.replace_all(artist, "").into_owned();
  a = RE_ARTIST_NOISE.replace_all(&a, "").into_owned();
  a = RE_SYMBOLS.replace_all(&a, " ").into_owned();
  a = RE_SPACES.replace_all(&a, " ").trim().to_string();
  (t, a)
}

fn is_synced(text: &str) -> bool {
  RE_SYNCED.is_match(text)
}

pub fn is_lyrics_explicit(lyrics: &str) -> bool {
  !lyrics.is_empty() && RE_EXPLICIT.is_match(lyrics)
}

fn normalize_meta(text: &str) -> String {
  let low = text.to_lowercase();
  let no_brackets = RE_META_BRACKET.replace_all(&low, "");
  RE_NON_ALNUM
    .replace_all(&no_brackets, " ")
    .trim()
    .to_string()
}

fn is_strict_match(req_title: &str, req_artist: &str, cand_title: &str, cand_artist: &str) -> bool {
  let t1 = normalize_meta(req_title);
  let t2 = normalize_meta(cand_title);
  if t1.is_empty() || t2.is_empty() || t1 != t2 {
    return false;
  }
  let a1 = normalize_meta(req_artist);
  let a2 = normalize_meta(cand_artist);
  if a1.is_empty() || a2.is_empty() {
    return false;
  }
  a1 == a2
}

async fn get_text(
  client: &Http,
  url: &str,
  ua: &str,
  referer: Option<&str>,
  timeout_ms: u64,
) -> Option<(u16, String)> {
  let mut req = client
    .get(url)
    .header(header::USER_AGENT, ua)
    .timeout(Duration::from_millis(timeout_ms));
  if let Some(r) = referer {
    req = req.header(header::REFERER, r);
  }
  let resp = req.send().await.ok()?;
  let status = resp.status().as_u16();
  let bytes = resp.bytes().await.ok()?;
  Some((status, String::from_utf8_lossy(&bytes).into_owned()))
}

async fn get_json_2xx(
  client: &Http,
  url: &str,
  ua: &str,
  referer: Option<&str>,
  timeout_ms: u64,
) -> Option<Value> {
  let (status, body) = get_text(client, url, ua, referer, timeout_ms).await?;
  if !(200..300).contains(&status) {
    return None;
  }
  serde_json::from_str(&body).ok()
}

enum Lrc {
  Instrumental,
  Text(String),
}

struct Lyr {
  synced: String,
  plain: String,
}

async fn fetch_lrclib(client: &Http, title: &str, artist: &str) -> Option<Lrc> {
  let (ct, ca) = sanitize_for_lyrics(title, artist);
  let url = format!(
    "https://lrclib.net/api/get?track_name={}&artist_name={}",
    encode_uri_component(&ct),
    encode_uri_component(&ca)
  );
  if let Some((200, body)) = get_text(client, &url, "devsize/2.0", None, 8000).await
    && let Ok(p) = serde_json::from_str::<Value>(&body)
  {
    if is_truthy(p.get("instrumental")) {
      return Some(Lrc::Instrumental);
    }
    let t = first_text(p.get("syncedLyrics"), p.get("plainLyrics"));
    if !t.is_empty() {
      return Some(Lrc::Text(t));
    }
  }
  let query = format!("{ca} {ct}");
  let search = format!(
    "https://lrclib.net/api/search?q={}",
    encode_uri_component(query.trim())
  );
  let (_, body) = get_text(client, &search, "devsize/2.0", None, 8000).await?;
  let list = serde_json::from_str::<Value>(&body).ok()?;
  let list = list.as_array()?;
  let first = list.first()?;
  if is_truthy(first.get("instrumental")) {
    return Some(Lrc::Instrumental);
  }
  if let Some(item) = list.iter().find(|i| is_truthy(i.get("syncedLyrics"))) {
    return Some(Lrc::Text(s(item.get("syncedLyrics")).to_string()));
  }
  let t = first_text(first.get("syncedLyrics"), first.get("plainLyrics"));
  if t.is_empty() {
    None
  } else {
    Some(Lrc::Text(t))
  }
}

async fn musixmatch_token(core: &Core) -> String {
  if let Ok(g) = core.auth.musixmatch_token.lock()
    && !g.0.is_empty()
    && now_millis() < g.1
  {
    return g.0.clone();
  }
  let url = format!("{MM_BASE}/token.get?app_id=web-desktop-app-v1.0");
  if let Some(data) = get_json_2xx(&core.net.http_client, &url, UA_MOZ, None, 15_000).await {
    let token = s(data.pointer("/message/body/user_token")).to_string();
    if !token.is_empty() {
      if let Ok(mut g) = core.auth.musixmatch_token.lock() {
        *g = (token.clone(), now_millis() + 24 * 3600 * 1000);
      }
      return token;
    }
  }
  core
    .auth
    .musixmatch_token
    .lock()
    .map(|g| g.0.clone())
    .unwrap_or_default()
}

async fn fetch_musixmatch(core: &Core, title: &str, artist: &str) -> Option<Lyr> {
  let client = &core.net.http_client;
  let token = musixmatch_token(core).await;
  let clean_title = RE_MM_TITLE.replace_all(title, "").trim().to_string();
  let clean_artist = RE_MM_ARTIST.replace_all(artist, "").trim().to_string();
  let q_track = encode_uri_component(if clean_title.is_empty() {
    title
  } else {
    &clean_title
  });
  let q_artist = encode_uri_component(if clean_artist.is_empty() {
    artist
  } else {
    &clean_artist
  });
  let tok = if token.is_empty() {
    String::new()
  } else {
    format!("&usertoken={}", encode_uri_component(&token))
  };
  let call = |name: &str| {
    format!(
      "{MM_BASE}/{name}?app_id=web-desktop-app-v1.0{tok}&q_track={q_track}&q_artist={q_artist}"
    )
  };

  if let Some(data) = get_json_2xx(client, &call("macro.subtitles.get"), UA_MOZ, None, 15_000).await
    && let Some(macro_calls) = data.pointer("/message/body/macro_calls")
    && is_truthy(Some(macro_calls))
  {
    let track = macro_calls
      .pointer("/matcher.track.get/message/body/track")
      .or_else(|| macro_calls.pointer("/track.get/message/body/track"))?;
    if !is_strict_match(
      title,
      artist,
      s(track.get("track_name")),
      s(track.get("artist_name")),
    ) {
      return None;
    }
    let sub = s(macro_calls
      .pointer("/track.subtitles.get/message/body/subtitle_list/0/subtitle/subtitle_body"));
    let lyr = s(macro_calls.pointer("/track.lyrics.get/message/body/lyrics/lyrics_body"));
    if !sub.is_empty() || !lyr.is_empty() {
      return Some(Lyr {
        synced: sub.to_string(),
        plain: lyr.to_string(),
      });
    }
  }

  let track_data = get_json_2xx(client, &call("matcher.track.get"), UA_MOZ, None, 15_000).await?;
  let track = track_data.pointer("/message/body/track")?;
  if !is_strict_match(
    title,
    artist,
    s(track.get("track_name")),
    s(track.get("artist_name")),
  ) {
    return None;
  }
  let synced = get_json_2xx(client, &call("matcher.subtitle.get"), UA_MOZ, None, 15_000)
    .await
    .map(|d| s(d.pointer("/message/body/subtitle/subtitle_body")).to_string())
    .unwrap_or_default();
  let plain = get_json_2xx(client, &call("matcher.lyrics.get"), UA_MOZ, None, 15_000)
    .await
    .map(|d| s(d.pointer("/message/body/lyrics/lyrics_body")).to_string())
    .unwrap_or_default();
  if synced.is_empty() && plain.is_empty() {
    None
  } else {
    Some(Lyr { synced, plain })
  }
}

async fn fetch_kugou(client: &Http, title: &str, artist: &str, duration: f64) -> Option<Lyr> {
  let dur_ms = (duration * 1000.0).round() as i64;
  let search = format!(
    "http://lyrics.kugou.com/search?ver=1&man=yes&client=pc&keyword={}&duration={}&hash=",
    encode_uri_component(&format!("{artist} - {title}")),
    dur_ms
  );
  let json = get_json_2xx(client, &search, UA_MOZ, None, 15_000).await?;
  let candidates = json.get("candidates").and_then(Value::as_array)?;
  let cand = candidates.iter().find(|c| {
    if !c.is_object() {
      return false;
    }
    let filename = s(c.get("filename"));
    let mut parts = filename.split('-');
    let f_artist = parts.next().unwrap_or("");
    let f_title = parts.next().unwrap_or("");
    let song = if s(c.get("songname")).is_empty() {
      f_title
    } else {
      s(c.get("songname"))
    };
    let singer = if s(c.get("singername")).is_empty() {
      f_artist
    } else {
      s(c.get("singername"))
    };
    is_strict_match(title, artist, song, singer)
  })?;
  let id = value_to_string(cand.get("id"));
  let access = value_to_string(cand.get("accesskey"));
  if id.is_empty() || id == "0" || access.is_empty() {
    return None;
  }
  let dl = format!(
    "http://lyrics.kugou.com/download?ver=1&client=pc&id={id}&accesskey={access}&fmt=lrc&charset=utf8"
  );
  let dl_json = get_json_2xx(client, &dl, UA_MOZ, None, 15_000).await?;
  let content = s(dl_json.get("content"));
  if content.is_empty() {
    return None;
  }
  let decoded = base64::engine::general_purpose::STANDARD
    .decode(content)
    .ok()?;
  let lrc = String::from_utf8_lossy(&decoded).into_owned();
  let lrc = RE_KUGOU_TAGS.replace_all(&lrc, "").trim().to_string();
  if lrc.is_empty() {
    None
  } else {
    Some(Lyr {
      synced: lrc,
      plain: String::new(),
    })
  }
}

fn netease_artist_names(song: &Value) -> Vec<String> {
  let list = song
    .get("artists")
    .filter(|v| is_truthy(Some(v)))
    .or_else(|| song.get("ar"))
    .and_then(Value::as_array);
  list
    .map(|arr| arr.iter().map(|a| s(a.get("name")).to_string()).collect())
    .unwrap_or_default()
}

fn netease_duration_secs(song: &Value) -> f64 {
  let dt = song.get("dt").and_then(Value::as_f64).filter(|d| *d != 0.0);
  let d = dt
    .or_else(|| song.get("duration").and_then(Value::as_f64))
    .unwrap_or(0.0);
  d / 1000.0
}

async fn netease_get(client: &Http, url: &str) -> Option<Value> {
  let (status, body) = get_text(client, url, UA_MOZ, Some("https://music.163.com"), 8000).await?;
  if status != 200 {
    return None;
  }
  serde_json::from_str(&body).ok()
}

async fn fetch_netease(client: &Http, title: &str, artist: &str, target: f64) -> Option<Lyr> {
  let q = clean_query(title, artist);
  let url = format!(
    "http://music.163.com/api/search/get/web?s={}&type=1&limit=5",
    encode_uri_component(&q)
  );
  let j = netease_get(client, &url).await?;
  let songs = j.pointer("/result/songs").and_then(Value::as_array)?;
  let valid: Vec<&Value> = songs
    .iter()
    .filter(|song| {
      song.is_object()
        && is_strict_match(
          title,
          artist,
          s(song.get("name")),
          &netease_artist_names(song).join(" "),
        )
    })
    .collect();
  let matched = valid
    .iter()
    .find(|song| !(target > 0.0 && (netease_duration_secs(song) - target).abs() > 4.0))
    .or_else(|| valid.first())?;
  let id = value_to_string(matched.get("id"));
  if id.is_empty() || id == "0" {
    return None;
  }
  let lyric_url = format!("http://music.163.com/api/song/lyric?id={id}&lv=1&kv=1&tv=-1");
  let lj = netease_get(client, &lyric_url).await?;
  let lrc = s(lj.pointer("/lrc/lyric"));
  if lrc.is_empty() {
    None
  } else if is_synced(lrc) {
    Some(Lyr {
      synced: lrc.to_string(),
      plain: String::new(),
    })
  } else {
    Some(Lyr {
      synced: String::new(),
      plain: lrc.to_string(),
    })
  }
}

fn norm_simple_cjk(text: &str) -> String {
  text
    .to_lowercase()
    .chars()
    .filter(|c| {
      c.is_ascii_digit()
        || c.is_ascii_lowercase()
        || ('\u{0400}'..='\u{04ff}').contains(c)
        || ('\u{4e00}'..='\u{9fa5}').contains(c)
    })
    .collect()
}

fn parse_yrc_words(rest: &str) -> Vec<Value> {
  let mut words: Vec<Value> = Vec::new();
  let mut cur: Option<(String, f64, f64)> = None;
  let push = |words: &mut Vec<Value>, w: (String, f64, f64)| {
    let text = w.0.trim().to_string();
    if !text.is_empty() {
      words.push(json!({ "text": text, "start": w.1, "end": w.2 }));
    }
  };
  for cap in RE_YRC_WORD.captures_iter(rest) {
    let start_ms: f64 = cap[1].parse().unwrap_or(0.0);
    let dur_ms: f64 = cap[2].parse().unwrap_or(0.0);
    let raw = cap[3].to_string();
    let start = start_ms / 1000.0;
    let end = (start_ms + dur_ms) / 1000.0;
    let is_cjk = RE_CJK.is_match(&raw);

    if let Some(w) = cur.take_if(|w| is_cjk || RE_CJK.is_match(&w.0)) {
      push(&mut words, w);
    }
    match cur.as_mut() {
      None => cur = Some((raw.clone(), start, end)),
      Some(w) => {
        w.0.push_str(&raw);
        w.2 = w.2.max(end);
      }
    }
    if (raw.chars().any(char::is_whitespace) || is_cjk)
      && let Some(w) = cur.take()
    {
      push(&mut words, w);
    }
  }
  if let Some(w) = cur {
    push(&mut words, w);
  }
  words
}

fn parse_yrc(yrc: &str) -> Vec<Value> {
  let mut lines = Vec::new();
  for raw in yrc.split('\n') {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
      continue;
    }
    let Some(m) = RE_YRC_LINE.captures(trimmed) else {
      continue;
    };
    let start_ms: f64 = m[1].parse().unwrap_or(0.0);
    let dur_ms: f64 = m[2].parse().unwrap_or(0.0);
    let words = parse_yrc_words(&m[3]);
    if words.is_empty() {
      continue;
    }
    let full = words
      .iter()
      .map(|w| s(w.get("text")))
      .collect::<Vec<_>>()
      .join(" ")
      .trim()
      .to_string();
    if !full.is_empty()
      && !full.starts_with("作词")
      && !full.starts_with("作曲")
      && !full.starts_with("制作人")
    {
      lines.push(json!({
        "start": start_ms / 1000.0,
        "end": (start_ms + dur_ms) / 1000.0,
        "text": full,
        "words": words
      }));
    }
  }
  lines
}

async fn fetch_netease_words(
  client: &Http,
  title: &str,
  artist: &str,
  target: f64,
) -> Option<Vec<Value>> {
  let q = encode_uri_component(&clean_query(title, artist));
  let urls = [
    format!("http://music.163.com/api/search/get/web?s={q}&type=1&limit=5"),
    format!("http://music.163.com/api/search/get?s={q}&type=1&limit=5"),
  ];
  let target_artists: Vec<String> = artist
    .split([',', '&', '/', '+'])
    .map(norm_simple_cjk)
    .filter(|a| !a.is_empty())
    .collect();
  let norm_title = norm_simple_cjk(title);
  for url in &urls {
    let Some(j) = netease_get(client, url).await else {
      continue;
    };
    let Some(songs) = j.pointer("/result/songs").and_then(Value::as_array) else {
      continue;
    };
    let matched = songs.iter().find(|song| {
      if target > 0.0 && (netease_duration_secs(song) - target).abs() > 3.0 {
        return false;
      }
      let cand_title = norm_simple_cjk(s(song.get("name")));
      if norm_title.is_empty()
        || cand_title.is_empty()
        || (!norm_title.contains(&cand_title) && !cand_title.contains(&norm_title))
      {
        return false;
      }
      let cand_artists: Vec<String> = song
        .get("artists")
        .and_then(Value::as_array)
        .map(|arr| {
          arr
            .iter()
            .map(|a| norm_simple_cjk(s(a.get("name"))))
            .filter(|a| !a.is_empty())
            .collect()
        })
        .unwrap_or_default();
      target_artists.is_empty()
        || cand_artists.iter().any(|ca| {
          target_artists
            .iter()
            .any(|ta| ca.contains(ta.as_str()) || ta.contains(ca.as_str()))
        })
    });
    if let Some(song) = matched {
      let id = value_to_string(song.get("id"));
      let lyric_url = format!("http://music.163.com/api/song/lyric?id={id}&lv=1&kv=1&tv=-1&yv=1");
      let lj = netease_get(client, &lyric_url).await?;
      let lines = parse_yrc(s(lj.pointer("/yrc/lyric")));
      return if lines.is_empty() { None } else { Some(lines) };
    }
  }
  None
}

fn decode_entities(text: &str) -> String {
  let mut t = text
    .replace("&amp;", "&")
    .replace("&lt;", "<")
    .replace("&gt;", ">")
    .replace("&quot;", "\"")
    .replace("&#039;", "'")
    .replace("&#39;", "'")
    .replace("&apos;", "'")
    .replace("&nbsp;", " ");
  t = RE_ENT_DEC
    .replace_all(&t, |c: &regex::Captures| {
      c[1]
        .parse::<u32>()
        .ok()
        .and_then(char::from_u32)
        .map(String::from)
        .unwrap_or_default()
    })
    .into_owned();
  RE_ENT_HEX
    .replace_all(&t, |c: &regex::Captures| {
      u32::from_str_radix(&c[1], 16)
        .ok()
        .and_then(char::from_u32)
        .map(String::from)
        .unwrap_or_default()
    })
    .into_owned()
}

fn extract_genius_lyrics(html: &str) -> String {
  const MARKER: &str = "data-lyrics-container=\"true\"";
  let mut lyrics = String::new();
  let mut pos = 0usize;
  while let Some(rel) = html[pos..].find(MARKER) {
    let idx = pos + rel;
    let Some(gt) = html[idx..].find('>') else {
      break;
    };
    let content_start = idx + gt + 1;
    let mut depth = 1i32;
    let mut cur = content_start;
    while depth > 0 && cur < html.len() {
      let next_open = html[cur..].find("<div").map(|p| p + cur);
      let Some(next_close) = html[cur..].find("</div>").map(|p| p + cur) else {
        break;
      };
      if let Some(open) = next_open.filter(|o| *o < next_close) {
        depth += 1;
        cur = open + 4;
      } else {
        depth -= 1;
        if depth == 0 {
          if !lyrics.is_empty() {
            lyrics.push('\n');
          }
          lyrics.push_str(&html[content_start..next_close]);
          pos = next_close + 6;
          break;
        }
        cur = next_close + 6;
      }
    }
    if depth > 0 {
      break;
    }
  }
  let text = RE_BR.replace_all(&lyrics, "\n");
  let text = RE_TAGS.replace_all(&text, "");
  let text = decode_entities(&text);
  let text = RE_GENIUS_SECTION
    .replace_all(&text, |c: &regex::Captures| {
      let inner = c[0][1..].to_lowercase();
      let keep = ["instrumental", "music", "intro", "outro", "solo"]
        .iter()
        .any(|w| {
          inner.strip_prefix(w).is_some_and(|rest| {
            rest
              .chars()
              .next()
              .is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'))
          })
        });
      if keep {
        c[0].to_string()
      } else {
        String::new()
      }
    })
    .into_owned();
  let text = RE_NEWLINES3.replace_all(&text, "\n\n");
  let kept: Vec<&str> = text
    .split('\n')
    .map(str::trim)
    .filter(|l| {
      if RE_CONTRIBUTORS.is_match(l) {
        return false;
      }
      let low = l.to_lowercase();
      if low.ends_with("lyrics") && !l.contains(' ') && l.encode_utf16().count() < 30 {
        return false;
      }
      !(RE_LYRICS_LINE.is_match(l) && low.contains("lyrics") && !l.contains('['))
    })
    .collect();
  kept.join("\n").trim().to_string()
}

fn norm_simple_ascii(text: &str) -> String {
  text
    .to_lowercase()
    .chars()
    .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    .collect()
}

async fn fetch_genius(client: &Http, title: &str, artist: &str) -> Option<String> {
  let query = clean_query(title, artist);
  let search = format!(
    "https://genius.com/api/search/song?q={}",
    encode_uri_component(&query)
  );
  let (status, body) = get_text(client, &search, UA_MOZ, None, 8000).await?;
  if status != 200 {
    return None;
  }
  let json: Value = serde_json::from_str(&body).ok()?;
  let hits = json
    .pointer("/response/sections/0/hits")
    .and_then(Value::as_array)?;
  let n_title = norm_simple_ascii(title);
  let n_artist = norm_simple_ascii(artist);
  let overlaps =
    |a: &str, b: &str| !a.is_empty() && !b.is_empty() && (a == b || a.contains(b) || b.contains(a));
  let mut best = hits.first()?;
  let mut best_score = -1;
  for h in hits {
    let Some(result) = h.get("result").filter(|r| is_truthy(r.get("url"))) else {
      continue;
    };
    let mut score = 0;
    if overlaps(&norm_simple_ascii(s(result.get("title"))), &n_title) {
      score += 2;
    }
    if overlaps(&norm_simple_ascii(s(result.get("artist_names"))), &n_artist) {
      score += 1;
    }
    if score > best_score {
      best_score = score;
      best = h;
    }
  }
  let song_url = s(best.pointer("/result/url"));
  if song_url.is_empty() {
    return None;
  }
  let (page_status, html) = get_text(client, song_url, UA_MOZ, None, 8000).await?;
  if page_status != 200 {
    return None;
  }
  let lyrics = extract_genius_lyrics(&html);
  if lyrics.is_empty() {
    None
  } else {
    Some(lyrics)
  }
}

fn legacy_custom_path(title: &str, artist: &str) -> PathBuf {
  let sanitize = |v: &str, fallback: &str| {
    let base = if v.is_empty() { fallback } else { v };
    base
      .chars()
      .map(|c| {
        if c.is_ascii_alphanumeric() || c == '_' || c.is_whitespace() || c == '-' {
          c
        } else {
          '_'
        }
      })
      .collect::<String>()
  };
  let name = format!(
    "{}_{}.lrc",
    sanitize(artist, "unknown"),
    sanitize(title, "untitled")
  )
  .to_lowercase();
  let name = RE_SPACES.replace_all(&name, "_").into_owned();
  paths::custom_lyrics_dir().join(name)
}

fn legacy_fetched_path(title: &str, artist: &str) -> PathBuf {
  let clean = |v: &str| {
    v.chars()
      .map(|c| {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
          c
        } else {
          '_'
        }
      })
      .collect::<String>()
      .to_lowercase()
  };
  paths::resolve_data_dir()
    .join("lyrics")
    .join(format!("{}___{}.lrc", clean(artist), clean(title)))
}

fn lyrics_json(lyrics: &str, is_custom: bool) -> CoreResult {
  json_response(
    200,
    &json!({
      "success": true,
      "lyrics": lyrics,
      "isCustom": is_custom,
      "isExplicit": is_lyrics_explicit(lyrics)
    }),
  )
}

fn instrumental_json() -> CoreResult {
  json_response(
    200,
    &json!({ "success": true, "instrumental": true, "syncedLyrics": "", "plainLyrics": "" }),
  )
}

fn cache_lyrics(core: &Core, key: &str, entry: LyricsCacheEntry) {
  if let Ok(mut g) = core.caches.lyrics_cache.lock() {
    g.insert(key.to_string(), entry);
  }
}

pub async fn lyrics(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let title = query.get("title").map(String::as_str).unwrap_or("");
  let artist = query.get("artist").map(String::as_str).unwrap_or("");
  let allow_online = query.get("online").map(String::as_str) != Some("0");
  let source = query
    .get("source")
    .filter(|v| !v.is_empty())
    .map(String::as_str)
    .unwrap_or("lrclib");
  let duration = query
    .get("duration")
    .and_then(|d| d.trim().parse::<f64>().ok())
    .unwrap_or(0.0);
  let client = &core.net.http_client;
  let cache_key = format!("{artist}:{title}:{source}").to_lowercase();

  let cached = core
    .caches
    .lyrics_cache
    .lock()
    .ok()
    .and_then(|g| g.get(&cache_key).cloned());
  if let Some(c) = cached {
    if c.instrumental {
      return instrumental_json();
    }
    if !c.text.contains('\u{FFFD}') {
      return lyrics_json(&c.text, c.is_custom);
    }
    if let Ok(mut g) = core.caches.lyrics_cache.lock() {
      g.remove(&cache_key);
    }
  }

  let key = lyrics_key(title, artist);
  let ascii_only = title.is_ascii() && artist.is_ascii();

  let mut custom_path = paths::custom_lyrics_dir().join(format!("{key}.lrc"));
  if !custom_path.exists() && ascii_only {
    let legacy = legacy_custom_path(title, artist);
    if legacy.exists() {
      custom_path = legacy;
    }
  }
  if custom_path.exists()
    && let Ok(text) = fs::read_to_string(&custom_path)
  {
    cache_lyrics(
      core,
      &cache_key,
      LyricsCacheEntry {
        text: text.clone(),
        is_custom: true,
        instrumental: false,
      },
    );
    return lyrics_json(&text, true);
  }

  if RE_INSTRUMENTAL_TITLE.is_match(title) {
    cache_lyrics(
      core,
      &cache_key,
      LyricsCacheEntry {
        text: String::new(),
        is_custom: false,
        instrumental: true,
      },
    );
    return instrumental_json();
  }

  let lyrics_dir = paths::resolve_data_dir().join("lyrics");
  let suffix = if source == "genius" { "_genius" } else { "" };
  let mut fetched_path = lyrics_dir.join(format!("{key}{suffix}.lrc"));
  if !fetched_path.exists() && ascii_only {
    let legacy = legacy_fetched_path(title, artist);
    if legacy.exists() {
      fetched_path = legacy;
    }
  }
  if source != "genius"
    && fetched_path.exists()
    && let Ok(disk) = fs::read_to_string(&fetched_path)
  {
    if disk.contains('\u{FFFD}') {
      let _ = fs::remove_file(&fetched_path);
    } else {
      cache_lyrics(
        core,
        &cache_key,
        LyricsCacheEntry {
          text: disk.clone(),
          is_custom: false,
          instrumental: false,
        },
      );
      return lyrics_json(&disk, false);
    }
  }

  if !allow_online {
    return lyrics_json("", false);
  }

  let pick = |l: Option<Lyr>| {
    l.map(|l| {
      if l.synced.is_empty() {
        l.plain
      } else {
        l.synced
      }
    })
    .unwrap_or_default()
  };
  let mut raw = String::new();
  let mut backend = "";
  match source {
    "musixmatch" => {
      raw = pick(fetch_musixmatch(core, title, artist).await);
      backend = "musixmatch";
    }
    "kugou" => {
      raw = pick(fetch_kugou(client, title, artist, duration).await);
      backend = "kugou";
    }
    "netease" => {
      raw = pick(fetch_netease(client, title, artist, duration).await);
      backend = "netease";
    }
    "genius" => {
      raw = fetch_genius(client, title, artist)
        .await
        .unwrap_or_default();
      backend = "genius";
    }
    _ => {
      let mut plain_fallback = String::new();
      match fetch_lrclib(client, title, artist).await {
        Some(Lrc::Instrumental) => {
          cache_lyrics(
            core,
            &cache_key,
            LyricsCacheEntry {
              text: String::new(),
              is_custom: false,
              instrumental: true,
            },
          );
          return instrumental_json();
        }
        Some(Lrc::Text(t)) => {
          if is_synced(&t) {
            raw = t;
            backend = "lrclib";
          } else {
            plain_fallback = t;
          }
        }
        None => {}
      }
      if raw.is_empty()
        && let Some(mm) = fetch_musixmatch(core, title, artist).await
      {
        if is_synced(&mm.synced) {
          raw = mm.synced;
          backend = "musixmatch";
        } else if plain_fallback.is_empty() {
          plain_fallback = mm.plain;
        }
      }
      if raw.is_empty()
        && let Some(kg) = fetch_kugou(client, title, artist, duration).await
      {
        if is_synced(&kg.synced) {
          raw = kg.synced;
          backend = "kugou";
        } else if plain_fallback.is_empty() {
          plain_fallback = kg.plain;
        }
      }
      if raw.is_empty()
        && let Some(ne) = fetch_netease(client, title, artist, duration).await
      {
        if is_synced(&ne.synced) {
          raw = ne.synced;
          backend = "netease";
        } else if plain_fallback.is_empty() {
          plain_fallback = ne.plain;
        }
      }
      if raw.is_empty() {
        if !plain_fallback.is_empty() {
          raw = plain_fallback;
        } else if let Some(gn) = fetch_genius(client, title, artist).await {
          raw = gn;
          backend = "genius";
        }
      }
    }
  }

  if raw.contains('\u{FFFD}') {
    raw.clear();
  }
  if !raw.trim().is_empty() {
    if !backend.is_empty() && backend != "genius" {
      let _ = fs::create_dir_all(&lyrics_dir);
      let _ = fs::write(&fetched_path, raw.as_bytes());
    }
    cache_lyrics(
      core,
      &cache_key,
      LyricsCacheEntry {
        text: raw.clone(),
        is_custom: false,
        instrumental: false,
      },
    );
    return lyrics_json(&raw, false);
  }
  cache_lyrics(
    core,
    &cache_key,
    LyricsCacheEntry {
      text: String::new(),
      is_custom: false,
      instrumental: false,
    },
  );
  lyrics_json("", false)
}

pub async fn lyrics_words(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let title = query.get("title").map(String::as_str).unwrap_or("");
  let artist = query.get("artist").map(String::as_str).unwrap_or("");
  let duration = query
    .get("duration")
    .and_then(|d| d.trim().parse::<f64>().ok())
    .unwrap_or(0.0);
  match fetch_netease_words(&core.net.http_client, title, artist, duration).await {
    Some(lines) if !lines.is_empty() => {
      json_response(200, &json!({ "success": true, "lines": lines }))
    }
    _ => json_response(200, &json!({ "success": false, "lines": [] })),
  }
}
