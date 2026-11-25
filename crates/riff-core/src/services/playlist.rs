use super::soundcloud::sc_client_id;
use super::stream::YT_EXTRACTOR_ARGS;
use super::ytdlp::execute_yt_dlp;
use super::*;

const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0.0.0 Safari/537.36";
const YT_BROWSE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

// ---------------------------------------------------------------------------
// Utilitaires
// ---------------------------------------------------------------------------

// Équivalent de `a || b || ''` pour des chaînes JSON.
fn first_str(vals: &[Option<&Value>]) -> String {
  vals
    .iter()
    .find(|v| is_truthy(**v))
    .map(|v| value_to_string(*v))
    .unwrap_or_default()
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
  v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn num_at(v: &Value, key: &str) -> f64 {
  v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

pub fn parse_smart_artist_and_title(raw_title: &str, raw_artist: &str) -> (String, String) {
  static RE_TOPIC: OnceLock<Regex> = OnceLock::new();
  static RE_VEVO: OnceLock<Regex> = OnceLock::new();
  static RE_NOISE: OnceLock<Regex> = OnceLock::new();
  static RE_SEP: OnceLock<Regex> = OnceLock::new();
  static RE_GENERIC: OnceLock<Regex> = OnceLock::new();
  let re_topic = RE_TOPIC.get_or_init(|| Regex::new(r"(?i)\s*-\s*Topic$").expect("regex"));
  let re_vevo = RE_VEVO.get_or_init(|| Regex::new(r"(?i)\bVEVO$").expect("regex"));
  let re_noise = RE_NOISE.get_or_init(|| {
    Regex::new(
      r"(?i)\s*[(\[]\s*(?:official\s*(?:music\s*)?(?:video|audio|lyric\s*video|visualizer)|lyrics?\s*video|lyrics?|visualizer|hd|hq|4k|remastered\s*\d*|mv|clip\s*officiel|официальный\s*клип|премьера\s*клипа|премьера\s*трека)\s*[)\]]",
    )
    .expect("regex")
  });
  let re_sep = RE_SEP.get_or_init(|| Regex::new(r"^(.+?)\s+(?:[-–—]|//)\s+(.+)$").expect("regex"));
  let re_generic = RE_GENERIC.get_or_init(|| {
    Regex::new(r"(?i)unknown|various|music|records|channel|archive|vibes|lyrics|topic|sound|audio|prod|beats|ost|ch\.")
      .expect("regex")
  });

  let mut title = raw_title.trim().to_string();
  let artist0 = re_topic.replace(raw_artist, "");
  let mut artist = re_vevo.replace(&artist0, "").trim().to_string();

  title = re_noise.replace_all(&title, "").trim().to_string();

  if let Some(c) = re_sep.captures(&title) {
    let left = c[1].trim().to_string();
    let right = c[2].trim().to_string();
    let norm_left = left.to_lowercase();
    let norm_uploader = artist.to_lowercase();
    let is_generic = norm_uploader.is_empty() || re_generic.is_match(&norm_uploader);
    let left_len = left.chars().count();
    if (2..=60).contains(&left_len)
      && right.chars().count() >= 2
      && (is_generic
        || norm_uploader == norm_left
        || norm_uploader.contains(&norm_left)
        || !right.to_lowercase().contains(&norm_uploader))
    {
      artist = left;
      title = right;
    }
  }

  (
    if title.is_empty() {
      "untitled".to_string()
    } else {
      title
    },
    if artist.is_empty() {
      "unknown artist".to_string()
    } else {
      artist
    },
  )
}

fn track_json(
  id: &str,
  title: String,
  artist: String,
  duration: i64,
  thumbnail: String,
  url: String,
  platform: &str,
) -> Value {
  json!({
    "id": id,
    "title": title,
    "artist": artist,
    "duration": duration,
    "thumbnail": thumbnail,
    "url": clean_url(&url),
    "platform": platform,
    "isCached": false
  })
}

fn fail(status: u16, error: &str) -> CoreResult {
  json_response(status, &json!({ "success": false, "error": error }))
}

// ---------------------------------------------------------------------------
// SoundCloud
// ---------------------------------------------------------------------------

// GET JSON avec un nouveau client_id sur 401/403 (un seul nouvel essai). Err = statut HTTP (0 : réseau/JSON).
async fn sc_json(
  core: &Core,
  cid: &mut String,
  url_for: &(dyn Fn(&str) -> String + Sync),
) -> Result<Value, u16> {
  for attempt in 0..2 {
    let resp = core
      .net
      .http_client
      .get(url_for(cid))
      .header(header::USER_AGENT, "Mozilla/5.0")
      .timeout(Duration::from_secs(20))
      .send()
      .await
      .map_err(|_| 0u16)?;
    let status = resp.status().as_u16();
    if matches!(status, 401 | 403) && attempt == 0 {
      *cid = sc_client_id(core, Some(cid.as_str())).await;
      continue;
    }
    if !resp.status().is_success() {
      return Err(status);
    }
    return resp.json::<Value>().await.map_err(|_| 0u16);
  }
  Err(0)
}

// Données `window.__sc_hydration` de la page publique.
async fn sc_hydration(core: &Core, page_url: &str) -> Option<Vec<Value>> {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| {
    Regex::new(r"window\.__sc_hydration\s*=\s*(\[[\s\S]*?\]);\s*</script>").expect("regex")
  });
  let resp = core
    .net
    .http_client
    .get(page_url)
    .header(header::USER_AGENT, CHROME_UA)
    .timeout(Duration::from_secs(20))
    .send()
    .await
    .ok()?;
  if !resp.status().is_success() {
    return None;
  }
  let html = resp.text().await.ok()?;
  let raw = re.captures(&html)?.get(1)?.as_str().to_string();
  match serde_json::from_str::<Value>(&raw).ok()? {
    Value::Array(list) => Some(list),
    _ => None,
  }
}

fn sc_track_to_track(t: &Value) -> Value {
  let user = t.get("user");
  let artist_raw = match user {
    Some(u) => first_str(&[u.get("username"), u.get("name")]),
    None => String::new(),
  };
  let (title, artist) = parse_smart_artist_and_title(
    &first_str(&[t.get("title")]).if_empty("untitled"),
    &artist_raw.if_empty("unknown artist"),
  );
  let duration_ms = if is_truthy(t.get("duration")) {
    num_at(t, "duration")
  } else {
    num_at(t, "full_duration")
  };
  let thumb = first_str(&[t.get("artwork_url"), user.and_then(|u| u.get("avatar_url"))]);
  track_json(
    &value_to_string(t.get("id")),
    title,
    artist,
    (duration_ms / 1000.0).round() as i64,
    thumb,
    str_at(t, "permalink_url").to_string(),
    "soundcloud",
  )
}

trait IfEmpty {
  fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
  fn if_empty(self, fallback: &str) -> String {
    if self.is_empty() {
      fallback.to_string()
    } else {
      self
    }
  }
}

async fn soundcloud_likes(core: &Core, input_url: &str) -> Option<CoreResult> {
  static RE_LIKES: OnceLock<Regex> = OnceLock::new();
  let re_likes = RE_LIKES.get_or_init(|| Regex::new(r"(?i)/likes/?(?:[?#].*)?$").expect("regex"));
  let mut cid = sc_client_id(core, None).await;

  let mut user: Option<Value> = None;
  if let Some(list) = sc_hydration(core, input_url).await {
    user = list
      .iter()
      .find(|x| str_at(x, "hydratable") == "user")
      .and_then(|x| x.get("data"))
      .filter(|d| !d.is_null())
      .cloned();
  }
  if user.is_none() {
    let clean_user_url = re_likes.replace(input_url, "").into_owned();
    let enc = encode_uri_component(&clean_user_url);
    user = sc_json(core, &mut cid, &|c| {
      format!("https://api-v2.soundcloud.com/resolve?url={enc}&client_id={c}")
    })
    .await
    .ok();
  }
  let user = user?;
  let user_id = if is_truthy(user.get("id")) {
    value_to_string(user.get("id"))
  } else {
    str_at(&user, "urn")
      .rsplit(':')
      .next()
      .unwrap_or("")
      .to_string()
  };
  if user_id.is_empty() {
    return None;
  }
  let user_name = first_str(&[user.get("username"), user.get("name")]).if_empty("User");
  let cover = first_str(&[
    user.get("avatar_url"),
    user.pointer("/visuals/visuals/0/visual_url"),
  ]);

  let mut next_url = Some(format!(
    "https://api-v2.soundcloud.com/users/{user_id}/likes?limit=200&client_id={cid}"
  ));
  let mut all_items: Vec<Value> = Vec::new();
  let mut pages = 0;
  static RE_CID: OnceLock<Regex> = OnceLock::new();
  let re_cid = RE_CID.get_or_init(|| Regex::new(r"client_id=[^&]+").expect("regex"));
  while let Some(next) = next_url.take() {
    if pages >= 25 {
      break;
    }
    pages += 1;
    let url_for = |c: &str| {
      if next.contains("client_id=") {
        re_cid
          .replace(&next, format!("client_id={c}").as_str())
          .into_owned()
      } else {
        format!("{next}&client_id={c}")
      }
    };
    let Ok(data) = sc_json(core, &mut cid, &url_for).await else {
      break;
    };
    if let Some(col) = data.get("collection").and_then(Value::as_array) {
      all_items.extend(col.iter().cloned());
    }
    next_url = data
      .get("next_href")
      .and_then(Value::as_str)
      .filter(|s| !s.is_empty())
      .map(str::to_string);
  }

  let tracks: Vec<Value> = all_items
    .iter()
    .filter_map(|item| {
      let t = match item.get("track") {
        Some(t) if is_truthy(Some(t)) => Some(t),
        _ => is_truthy(item.get("title")).then_some(item),
      }?;
      (is_truthy(t.get("id")) && is_truthy(t.get("title"))).then(|| sc_track_to_track(t))
    })
    .collect();

  Some(json_response(
    200,
    &json!({ "success": true, "name": format!("{user_name} - Likes"), "cover": cover, "tracks": tracks }),
  ))
}

async fn soundcloud_set(core: &Core, input_url: &str) -> Option<CoreResult> {
  static RE_SECRET: OnceLock<Regex> = OnceLock::new();
  let re_secret = RE_SECRET.get_or_init(|| Regex::new(r"/(s-[a-zA-Z0-9_-]+)").expect("regex"));
  let secret = re_secret
    .captures(input_url)
    .map(|c| c[1].to_string())
    .unwrap_or_default();
  let secret_q = if secret.is_empty() {
    String::new()
  } else {
    format!("&secret_token={secret}")
  };
  let mut cid = sc_client_id(core, None).await;

  let mut pl: Option<Value> = None;
  if let Some(list) = sc_hydration(core, input_url).await {
    pl = list
      .iter()
      .find(|x| str_at(x, "hydratable") == "playlist")
      .and_then(|x| x.get("data"))
      .filter(|d| !d.is_null())
      .cloned();
    if pl.is_none() {
      pl = list
        .iter()
        .filter_map(|x| x.get("data"))
        .find(|d| str_at(d, "kind") == "playlist" || d.get("tracks").is_some_and(Value::is_array))
        .cloned();
    }
  }
  if pl.is_none() {
    let enc = encode_uri_component(input_url);
    pl = sc_json(core, &mut cid, &|c| {
      format!("https://api-v2.soundcloud.com/resolve?url={enc}&client_id={c}{secret_q}")
    })
    .await
    .ok();
  }
  let pl = pl?;
  let pl_tracks = pl.get("tracks").and_then(Value::as_array)?;

  let mut by_id: HashMap<String, Value> = HashMap::new();
  let mut missing: Vec<String> = Vec::new();
  for t in pl_tracks {
    if !t.is_object() {
      continue;
    }
    if is_truthy(t.get("title")) {
      by_id.insert(value_to_string(t.get("id")), t.clone());
    } else {
      missing.push(value_to_string(t.get("id")));
    }
  }
  for chunk in missing.chunks(50) {
    let ids = chunk.join(",");
    let Ok(Value::Array(items)) = sc_json(core, &mut cid, &|c| {
      format!("https://api-v2.soundcloud.com/tracks?ids={ids}&client_id={c}{secret_q}")
    })
    .await
    else {
      continue;
    };
    for it in items {
      if is_truthy(it.get("id")) {
        by_id.insert(value_to_string(it.get("id")), it);
      }
    }
  }

  let tracks: Vec<Value> = pl_tracks
    .iter()
    .filter(|t| t.is_object())
    .map(|t| {
      if is_truthy(t.get("id")) {
        by_id.get(&value_to_string(t.get("id"))).unwrap_or(t)
      } else {
        t
      }
    })
    .filter(|t| is_truthy(t.get("title")))
    .map(sc_track_to_track)
    .collect();
  if tracks.is_empty() {
    return None;
  }
  Some(json_response(
    200,
    &json!({
      "success": true,
      "name": first_str(&[pl.get("title")]).if_empty("SoundCloud Playlist"),
      "cover": str_at(&pl, "artwork_url"),
      "tracks": tracks
    }),
  ))
}

// ---------------------------------------------------------------------------
// YouTube : repli direct sur l'API innertube `browse`
// ---------------------------------------------------------------------------

struct DirectPlaylist {
  name: String,
  cover: String,
  tracks: Vec<Value>,
}

fn yt_text(v: Option<&Value>) -> String {
  match v {
    Some(Value::String(s)) => s.clone(),
    Some(o) => {
      if let Some(s) = o
        .get("simpleText")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
      {
        return s.to_string();
      }
      o.get("runs")
        .and_then(Value::as_array)
        .map(|runs| runs.iter().map(|r| str_at(r, "text")).collect::<String>())
        .unwrap_or_default()
    }
    None => String::new(),
  }
}

fn last_thumb_url(thumbs: Option<&Value>) -> Option<String> {
  thumbs
    .and_then(Value::as_array)
    .and_then(|a| a.last())
    .map(|t| str_at(t, "url").to_string())
    .filter(|s| !s.is_empty())
}

fn parse_duration_str(s: &str) -> i64 {
  let parts: Vec<f64> = s
    .trim()
    .split(':')
    .map(|p| p.trim().parse::<f64>().unwrap_or(f64::NAN))
    .collect();
  if s.trim().is_empty() || parts.iter().any(|p| p.is_nan()) {
    return 0;
  }
  let v = match parts.len() {
    3 => parts[0] * 3600.0 + parts[1] * 60.0 + parts[2],
    2 => parts[0] * 60.0 + parts[1],
    _ => parts[0],
  };
  v as i64
}

struct Walker<'a> {
  platform: &'a str,
  seen: HashSet<String>,
  tracks: Vec<Value>,
}

impl Walker<'_> {
  fn push(
    &mut self,
    id: &str,
    raw_title: &str,
    raw_artist: &str,
    duration: i64,
    thumb: Option<String>,
  ) {
    let (title, artist) = parse_smart_artist_and_title(raw_title, raw_artist);
    let thumbnail = thumb.unwrap_or_else(|| format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"));
    let platform = if self.platform == "ytmusic" {
      "ytmusic"
    } else {
      "youtube"
    };
    self.tracks.push(track_json(
      id,
      title,
      artist,
      duration,
      thumbnail,
      format!("https://www.youtube.com/watch?v={id}"),
      platform,
    ));
  }

  fn walk(&mut self, node: &Value) {
    match node {
      Value::Object(map) => {
        if let Some(item) = map.get("playlistVideoRenderer") {
          let id = value_to_string(item.get("videoId"));
          if !id.is_empty() && self.seen.insert(id.clone()) {
            let raw_t = yt_text(item.get("title")).if_empty("untitled");
            let raw_a = yt_text(item.get("shortBylineText")).if_empty("unknown artist");
            let duration = match item.get("lengthSeconds") {
              Some(v) if is_truthy(Some(v)) => {
                value_to_string(Some(v)).trim().parse::<i64>().unwrap_or(0)
              }
              _ => parse_duration_str(&yt_text(item.get("lengthText"))),
            };
            self.push(
              &id,
              &raw_t,
              &raw_a,
              duration,
              last_thumb_url(item.pointer("/thumbnail/thumbnails")),
            );
          }
        }
        if let Some(item) = map.get("musicResponsiveListItemRenderer") {
          let id = first_str(&[
            item.pointer("/playlistItemData/videoId"),
            item.pointer("/doubleTapCommand/watchEndpoint/videoId"),
          ]);
          if !id.is_empty() && self.seen.insert(id.clone()) {
            let flex = item.get("flexColumns").and_then(Value::as_array);
            let col = |i: usize| {
              flex
                .and_then(|f| f.get(i))
                .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
            };
            let raw_t = yt_text(col(0));
            let raw_a = col(1)
              .and_then(|t| t.pointer("/runs/0/text"))
              .and_then(Value::as_str)
              .unwrap_or("")
              .to_string();
            let duration = item
              .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text")
              .map(|t| parse_duration_str(&yt_text(Some(t))))
              .unwrap_or(0);
            self.push(
              &id,
              &raw_t,
              &raw_a,
              duration,
              last_thumb_url(
                item.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"),
              ),
            );
          }
        }
        for v in map.values() {
          self.walk(v);
        }
      }
      Value::Array(arr) => {
        for v in arr {
          self.walk(v);
        }
      }
      _ => {}
    }
  }
}

async fn fetch_youtube_playlist_direct(
  core: &Core,
  list_id: &str,
  platform_name: &str,
) -> DirectPlaylist {
  let mut result = DirectPlaylist {
    name: "YouTube Playlist".into(),
    cover: String::new(),
    tracks: Vec::new(),
  };
  let browse_id = if list_id.starts_with("VL") {
    list_id.to_string()
  } else {
    format!("VL{list_id}")
  };
  let body = json!({
    "context": { "client": { "clientName": "WEB", "clientVersion": "2.20250101.00.00", "hl": "en", "gl": "US" } },
    "browseId": browse_id
  });
  let resp = core
    .net
    .http_client
    .post("https://www.youtube.com/youtubei/v1/browse?prettyPrint=false")
    .header(header::USER_AGENT, YT_BROWSE_UA)
    .json(&body)
    .timeout(Duration::from_secs(30))
    .send()
    .await;
  let data = match resp {
    Ok(r) if r.status().is_success() => match r.json::<Value>().await {
      Ok(v) => v,
      Err(e) => {
        logging::log_entry("ERROR", &format!("fetchYoutubePlaylistDirect failed: {e}"));
        return result;
      }
    },
    Ok(_) => return result,
    Err(e) => {
      logging::log_entry("ERROR", &format!("fetchYoutubePlaylistDirect failed: {e}"));
      return result;
    }
  };

  if let Some(header_v) = data.get("header") {
    if let Some(phr) = header_v.get("playlistHeaderRenderer") {
      let n = yt_text(phr.get("title"));
      if !n.is_empty() {
        result.name = n;
      }
      if let Some(u) =
        last_thumb_url(phr.pointer("/playlistHeaderBannerRenderer/heroImage/thumbnails"))
      {
        result.cover = u;
      }
    }
    if let Some(mdhr) = header_v.get("musicDetailHeaderRenderer") {
      let n = yt_text(mdhr.get("title"));
      if !n.is_empty() {
        result.name = n;
      }
      if let Some(u) = last_thumb_url(
        mdhr.pointer("/thumbnail/croppedSquareThumbnailRenderer/thumbnail/thumbnails"),
      ) {
        result.cover = u;
      }
    }
  }
  if result.cover.is_empty()
    && let Some(u) =
      last_thumb_url(data.pointer("/microformat/microformatDataRenderer/thumbnail/thumbnails"))
  {
    result.cover = u;
  }
  let micro_title = yt_text(data.pointer("/microformat/microformatDataRenderer/title"));
  if !micro_title.is_empty() {
    result.name = micro_title;
  }

  let mut walker = Walker {
    platform: platform_name,
    seen: HashSet::new(),
    tracks: Vec::new(),
  };
  walker.walk(&data);
  result.tracks = walker.tracks;
  if result.cover.is_empty()
    && let Some(first) = result.tracks.first()
  {
    result.cover = str_at(first, "thumbnail").to_string();
  }
  result
}

fn direct_response(fallback: DirectPlaylist) -> CoreResult {
  json_response(
    200,
    &json!({ "success": true, "name": fallback.name, "cover": fallback.cover, "tracks": fallback.tracks }),
  )
}

// ---------------------------------------------------------------------------
// Deezer
// ---------------------------------------------------------------------------

async fn deezer_playlist(core: &Core, playlist_id: &str) -> Result<Value, String> {
  let playlist = fetch_itunes_json(
    &core.net.http_client,
    &format!("https://api.deezer.com/playlist/{playlist_id}"),
  )
  .await
  .filter(|p| !is_truthy(p.get("error")) && is_truthy(p.get("id")))
  .ok_or_else(|| "Deezer playlist was not found".to_string())?;

  let mut items: Vec<Value> = Vec::new();
  let mut next = playlist
    .pointer("/tracks/next")
    .and_then(Value::as_str)
    .map(str::to_string);
  if let Some(data) = playlist.pointer("/tracks/data").and_then(Value::as_array) {
    items.extend(data.iter().cloned());
  }
  let mut pages = 0;
  while let Some(url) = next.take().filter(|u| !u.is_empty()) {
    if pages >= 100 {
      break;
    }
    let page = fetch_itunes_json(&core.net.http_client, &url)
      .await
      .ok_or_else(|| "Deezer playlist page request failed".to_string())?;
    if let Some(data) = page.get("data").and_then(Value::as_array) {
      items.extend(data.iter().cloned());
    }
    next = page.get("next").and_then(Value::as_str).map(str::to_string);
    pages += 1;
  }

  let tracks: Vec<Value> = items
    .iter()
    .filter(|t| {
      is_truthy(t.get("id")) && is_truthy(t.get("title")) && is_truthy(t.pointer("/artist/name"))
    })
    .map(|t| {
      let album = t.get("album");
      let thumb = album
        .map(|a| first_str(&[a.get("cover_xl"), a.get("cover_big"), a.get("cover")]))
        .unwrap_or_default();
      json!({
        "id": value_to_string(t.get("id")),
        "title": value_to_string(t.get("title")),
        "artist": value_to_string(t.pointer("/artist/name")),
        "duration": t.get("duration").and_then(Value::as_f64).filter(|n| *n != 0.0).unwrap_or(0.0) as i64,
        "thumbnail": thumb,
        "url": clean_url(str_at(t, "link")),
        "platform": "deezer",
        "isExplicit": is_truthy(t.get("explicit_lyrics"))
      })
    })
    .collect();
  if tracks.is_empty() {
    return Err("No tracks found in this Deezer playlist".into());
  }
  Ok(json!({
    "success": true,
    "name": first_str(&[playlist.get("title")]).if_empty("Deezer Playlist"),
    "cover": first_str(&[playlist.get("picture_xl"), playlist.get("picture_big"), playlist.get("picture")]),
    "tracks": tracks
  }))
}

// ---------------------------------------------------------------------------
// Route
// ---------------------------------------------------------------------------

pub async fn playlist_import(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let mut input_url = clean_url(query.get("url").map_or("", String::as_str));
  if [
    "on.soundcloud.com",
    "soundcloud.app.goo.gl",
    "link.deezer.com",
  ]
  .iter()
  .any(|h| input_url.contains(h))
    && let Ok(resp) = core
      .net
      .http_client
      .get(&input_url)
      .header(header::USER_AGENT, "Mozilla/5.0")
      .timeout(Duration::from_secs(15))
      .send()
      .await
  {
    input_url = clean_url(resp.url().as_str());
  }

  let Ok(parsed) = Url::parse(&input_url) else {
    return fail(400, "Unsupported link");
  };
  let host = parsed.host_str().unwrap_or("").to_lowercase();
  let is_soundcloud = host == "soundcloud.com" || host.ends_with(".soundcloud.com");
  let is_youtube = host == "youtube.com" || host == "youtu.be" || host.ends_with(".youtube.com");
  let is_deezer = host == "deezer.com" || host == "www.deezer.com";

  if is_deezer {
    static RE_PL: OnceLock<Regex> = OnceLock::new();
    let re = RE_PL.get_or_init(|| Regex::new(r"(?i)^/playlist/(\d+)(?:/|$)").expect("regex"));
    let Some(c) = re.captures(parsed.path()) else {
      return fail(400, "Unsupported Deezer link");
    };
    return match deezer_playlist(core, &c[1]).await {
      Ok(body) => json_response(200, &body),
      Err(e) => {
        logging::log_entry("ERROR", &format!("Deezer playlist import failed: {e}"));
        fail(502, &e)
      }
    };
  }

  if !is_soundcloud && !is_youtube {
    return fail(400, "Unsupported link");
  }

  let platform_name = if is_soundcloud {
    "soundcloud"
  } else if host == "music.youtube.com" {
    "ytmusic"
  } else {
    "youtube"
  };
  let list_id = parsed
    .query_pairs()
    .find(|(k, _)| k == "list")
    .map(|(_, v)| v.into_owned())
    .unwrap_or_default();

  static RE_LIKES_URL: OnceLock<Regex> = OnceLock::new();
  let re_likes_url =
    RE_LIKES_URL.get_or_init(|| Regex::new(r"(?i)/likes/?(?:[?#]|$)").expect("regex"));
  if is_soundcloud
    && re_likes_url.is_match(&input_url)
    && let Some(resp) = soundcloud_likes(core, &input_url).await
  {
    return resp;
  }
  if is_soundcloud
    && input_url.contains("/sets/")
    && let Some(resp) = soundcloud_set(core, &input_url).await
  {
    return resp;
  }

  let target_url = if is_youtube && !list_id.is_empty() {
    format!(
      "https://www.youtube.com/playlist?list={}",
      encode_uri_component(&list_id)
    )
  } else {
    input_url.clone()
  };
  let mut args: Vec<String> = [
    "--dump-single-json",
    "--ignore-errors",
    "--no-warnings",
    "--playlist-end",
    "5000",
  ]
  .iter()
  .map(|s| s.to_string())
  .collect();
  if is_youtube {
    args.insert(0, "--flat-playlist".into());
    args.push("--extractor-args".into());
    args.push(YT_EXTRACTOR_ARGS.into());
  }
  args.push("--".into());
  args.push(target_url);

  let outcome: Result<CoreResult, String> = async {
    let raw = execute_yt_dlp(core, &args, 60_000, false).await?;
    let mut data: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let entries_of = |d: &Value| -> Vec<Value> {
      match d.get("entries") {
        Some(Value::Array(a)) => a.clone(),
        _ => vec![d.clone()],
      }
    };
    let mut entries = entries_of(&data);
    let first_has_no_title = entries.first().is_some_and(|e| !is_truthy(e.get("title")));
    if first_has_no_title && is_youtube {
      let deep: Vec<String> = args
        .iter()
        .filter(|a| *a != "--flat-playlist")
        .cloned()
        .collect();
      let raw = execute_yt_dlp(core, &deep, 60_000, false).await?;
      data = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
      entries = entries_of(&data);
    }

    let mut tracks: Vec<Value> = Vec::new();
    for item in &entries {
      if !is_truthy(item.get("id")) {
        continue;
      }
      let id = value_to_string(item.get("id"));
      let title_field = str_at(item, "title");
      if title_field == "[Private video]" || title_field == "[Deleted video]" {
        continue;
      }
      let track_url = if is_youtube {
        format!("https://www.youtube.com/watch?v={id}")
      } else {
        clean_url(&first_str(&[item.get("webpage_url"), item.get("url")]))
      };
      if track_url.is_empty() {
        continue;
      }
      let raw_title = first_str(&[
        item.get("title"),
        item.get("track"),
        item.get("fulltitle"),
        item.get("name"),
      ]);
      let artists_joined = item
        .get("artists")
        .and_then(Value::as_array)
        .map(|a| {
          a.iter()
            .map(|v| value_to_string(Some(v)))
            .collect::<Vec<_>>()
            .join(", ")
        })
        .unwrap_or_default();
      let artists_val = Value::String(artists_joined);
      let raw_artist = first_str(&[
        item.get("artist"),
        Some(&artists_val),
        item.get("creator"),
        item.get("channel"),
        item.get("uploader"),
        item.get("album_artist"),
      ]);
      let (title, artist) = parse_smart_artist_and_title(raw_title.trim(), raw_artist.trim());

      let thumbnail = if is_youtube {
        format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")
      } else {
        let mut t = String::new();
        if let Some(list) = item
          .get("thumbnails")
          .and_then(Value::as_array)
          .filter(|l| !l.is_empty())
        {
          let pick = list
            .iter()
            .find(|x| matches!(str_at(x, "id"), "t500x500" | "large" | "crop" | "original"))
            .or(list.last());
          t = pick
            .map(|x| str_at(x, "url").to_string())
            .unwrap_or_default();
        }
        if t.is_empty() {
          t = first_str(&[
            item.get("thumbnail"),
            item.get("artwork_url"),
            item.get("avatar_url"),
          ]);
        }
        t
      };
      tracks.push(track_json(
        &id,
        title,
        artist,
        num_at(item, "duration").round() as i64,
        thumbnail,
        track_url,
        platform_name,
      ));
    }

    if tracks.is_empty() {
      if is_youtube && !list_id.is_empty() {
        let fb = fetch_youtube_playlist_direct(core, &list_id, platform_name).await;
        if !fb.tracks.is_empty() {
          return Ok(direct_response(fb));
        }
      }
      return Ok(fail(200, "No tracks found"));
    }

    let mut cover = first_str(&[data.get("coverUrl"), data.get("ogImage")]);
    if cover.is_empty()
      && let Some(last) = data
        .get("thumbnails")
        .and_then(Value::as_array)
        .and_then(|a| a.last())
    {
      cover = match last {
        Value::String(s) => s.clone(),
        o => str_at(o, "url").to_string(),
      };
    }
    if cover.is_empty() {
      cover = match data.get("thumbnail") {
        Some(Value::String(s)) => s.clone(),
        Some(o) => str_at(o, "url").to_string(),
        None => String::new(),
      };
    }
    if cover.is_empty() && tracks.len() == 1 {
      cover = str_at(&tracks[0], "thumbnail").to_string();
    }
    Ok(json_response(
      200,
      &json!({
        "success": true,
        "name": first_str(&[data.get("title")]).if_empty("Imported playlist"),
        "cover": cover,
        "tracks": tracks
      }),
    ))
  }
  .await;

  match outcome {
    Ok(resp) => resp,
    Err(err) => {
      if is_youtube && !list_id.is_empty() {
        let fb = fetch_youtube_playlist_direct(core, &list_id, platform_name).await;
        if !fb.tracks.is_empty() {
          return direct_response(fb);
        }
      }
      logging::log_entry(
        "ERROR",
        &format!("Playlist import failed: {input_url}: {err}"),
      );
      fail(500, &err)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn smart_title_splits_artist_from_generic_channel() {
    let (t, a) = parse_smart_artist_and_title(
      "Daft Punk - One More Time (Official Video)",
      "Some Music Channel",
    );
    assert_eq!((t.as_str(), a.as_str()), ("One More Time", "Daft Punk"));
  }

  #[test]
  fn smart_title_keeps_uploader_when_title_has_it() {
    let (t, a) = parse_smart_artist_and_title("Song - Daft Punk", "Daft Punk - Topic");
    assert_eq!((t.as_str(), a.as_str()), ("Song - Daft Punk", "Daft Punk"));
  }

  #[test]
  fn smart_title_defaults() {
    let (t, a) = parse_smart_artist_and_title("", "");
    assert_eq!((t.as_str(), a.as_str()), ("untitled", "unknown artist"));
  }

  #[test]
  fn duration_strings() {
    assert_eq!(parse_duration_str("3:25"), 205);
    assert_eq!(parse_duration_str("1:02:03"), 3723);
    assert_eq!(parse_duration_str("x"), 0);
  }
}
