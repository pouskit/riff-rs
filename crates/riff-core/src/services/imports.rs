use super::spotify::{get_with_retry, load_auth, lock};
use super::*;
use std::sync::LazyLock;

const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const FIREFOX_UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:125.0) Gecko/20100101 Firefox/125.0";
const PATHFINDER_URL: &str = "https://api-partner.spotify.com/pathfinder/v1/query";
// Identifiant public de la requête persistée "fetchPlaylist" (API non documentée de Spotify).
const PATHFINDER_HASH: &str = "8964e8eafb21aa992a7d951d256d83285c04be2105d209262901de70cb97584a";

macro_rules! re {
  ($name:ident, $pat:expr) => {
    static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("regex imports"));
  };
}

re!(
  RE_SPOTIFY_URL,
  r"^(?:(?:https://)?open\.spotify\.com/(?:embed/)?playlist/|spotify:playlist:)([a-zA-Z0-9]{22})(?:[/?#]|$)"
);
re!(
  RE_NEXT_DATA,
  r#"(?s)<script id="__NEXT_DATA__"[^>]*>(.*?)</script>"#
);
re!(
  RE_OG_IMAGE,
  r#"(?i)<meta\s+(?:property|name)=["'](?:og:image|twitter:image)["']\s+content=["']([^"']+)["']"#
);
re!(RE_PRIVATE, r"(?i)private");
re!(
  RE_APPLE_ALBUM,
  r"(?i)music\.apple\.com/(?:[a-z]{2}/)?album/(?:[^/]+/)?(\d+)"
);
re!(
  RE_APPLE_PLAYLIST,
  r"(?i)music\.apple\.com/.*?(pl\.(?:u-)?[a-zA-Z0-9_-]+)"
);
re!(
  RE_APPLE_SERVER_DATA,
  r#"(?is)<script\s+type=["']application/json["']\s+id=["']serialized-server-data["'][^>]*>(.*?)</script>"#
);
re!(
  RE_APPLE_LD,
  r#"(?is)<script\s+id=["']schema:music-playlist["']\s+type=["']application/ld\+json["'][^>]*>(.*?)</script>"#
);
re!(RE_APPLE_PRIV, r"(?i)private|sign\s*in|login");
re!(
  RE_APPLE_OG_TITLE,
  r#"(?i)<meta\s+property=["']og:title["']\s+content=["']([^"']*)["']"#
);
re!(RE_APPLE_ON_AM, r"(?i)\s*on Apple Music\s*$");
re!(RE_HTML_TITLE, r"(?i)<title>([^<]*)</title>");
re!(
  RE_APPLE_TITLE_SUFFIX,
  r"(?i)\s*-\s*Playlist\s*-\s*Apple Music\s*$"
);
re!(RE_LEADING_SYMBOLS, r"^[^A-Za-z0-9_\s]+");
re!(RE_ISO_DURATION, r"PT(?:(\d+)M)?(?:(\d+)S)?");
re!(RE_YANDEX_USER_PL, r"(?i)users/([^/?#]+)/playlists/(\d+)");
re!(RE_YANDEX_PL, r"(?i)playlists/((?:lk\.)?[a-f0-9-]{8,})");
re!(RE_YANDEX_ALBUM_TRACK, r"(?i)album/(\d+)/track/(\d+)");
re!(RE_YANDEX_TRACK, r"(?i)track/(\d+)");
re!(RE_YANDEX_ALBUM, r"(?i)album/(\d+)");
re!(RE_HTTP, r"(?i)^https?://");

const PAGE_ENTITY: &str = "/state/data/entity";
const PAGE_ACCESS_TOKEN: &str = "/state/settings/session/accessToken";

fn err_json(status: u16, body: Value) -> CoreResult {
  json_response(status, &body)
}

fn st(v: Option<&Value>) -> &str {
  v.and_then(Value::as_str).unwrap_or("")
}

fn pick<'a>(vals: &[Option<&'a Value>]) -> Option<&'a Value> {
  vals.iter().flatten().find(|v| is_truthy(Some(v))).copied()
}

fn pick_str(vals: &[Option<&Value>]) -> String {
  pick(vals).and_then(Value::as_str).unwrap_or("").to_string()
}

fn round_secs(ms: f64) -> i64 {
  (ms / 1000.0).round() as i64
}

fn set_opt(map: &mut Value, key: &str, val: Option<i64>) {
  if let Some(v) = val {
    map[key] = json!(v);
  }
}

// ---------------------------------------------------------------- Spotify

async fn pathfinder_request(client: &Http, token: &str, variables: Value) -> Result<Value, String> {
  let resp = client
    .post(PATHFINDER_URL)
    .bearer_auth(token)
    .header(header::ACCEPT, "text/html,application/json")
    .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
    .header(header::REFERER, "https://open.spotify.com/")
    .header(header::USER_AGENT, CHROME_UA)
    .json(&json!({
      "variables": variables,
      "operationName": "fetchPlaylist",
      "extensions": { "persistedQuery": { "version": 1, "sha256Hash": PATHFINDER_HASH } }
    }))
    .timeout(Duration::from_secs(15))
    .send()
    .await
    .map_err(|e| {
      if e.is_timeout() {
        "Pathfinder request timed out".to_string()
      } else {
        e.to_string()
      }
    })?;
  let status = resp.status().as_u16();
  if status != 200 {
    return Err(format!("Pathfinder status {status}"));
  }
  resp
    .json::<Value>()
    .await
    .map_err(|_| "Invalid pathfinder response".to_string())
}

async fn fetch_full_playlist(
  client: &Http,
  playlist_id: &str,
  token: &str,
) -> Result<Option<(Vec<Value>, u64)>, String> {
  let uri = format!("spotify:playlist:{playlist_id}");
  let vars = |offset: usize| json!({ "uri": uri, "offset": offset, "limit": 100, "enableWatchFeedEntrypoint": true });
  let first = pathfinder_request(client, token, vars(0)).await?;
  let Some(content) = first
    .pointer("/data/playlistV2/content")
    .filter(|c| c.get("items").is_some_and(Value::is_array))
  else {
    return Ok(None);
  };
  let mut items = content["items"].as_array().cloned().unwrap_or_default();
  let total = content
    .get("totalCount")
    .and_then(Value::as_u64)
    .filter(|t| *t != 0)
    .unwrap_or(items.len() as u64);
  let mut guard = 0;
  while (items.len() as u64) < total && guard < 100 {
    guard += 1;
    let page = pathfinder_request(client, token, vars(items.len())).await?;
    let page_items = page
      .pointer("/data/playlistV2/content/items")
      .and_then(Value::as_array)
      .filter(|a| !a.is_empty());
    let Some(page_items) = page_items else {
      return Err("Incomplete Pathfinder playlist".to_string());
    };
    items.extend(page_items.iter().cloned());
  }
  if (items.len() as u64) < total {
    return Err("Incomplete Pathfinder playlist".to_string());
  }
  let tracks = items
    .iter()
    .filter_map(|it| {
      let t = pick(&[it.pointer("/itemV2/data"), it.pointer("/item/data")]).cloned().unwrap_or(json!({}));
      let artists: Vec<&str> = t
        .pointer("/albumOfTrack/artists/items")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.pointer("/profile/name").and_then(Value::as_str)).filter(|n| !n.is_empty()).collect())
        .unwrap_or_default();
      let dur_ms = pick(&[t.pointer("/trackDuration/totalMilliseconds"), t.pointer("/duration/totalMilliseconds"), t.get("duration_ms")]);
      let duration = dur_ms.and_then(Value::as_f64).map(round_secs).unwrap_or(0);
      let cover = t
        .pointer("/albumOfTrack/coverArt/sources")
        .and_then(Value::as_array)
        .and_then(|s| s.last())
        .map(|s| st(s.get("url")).to_string())
        .unwrap_or_default();
      let title = st(t.get("name")).trim().to_string();
      (!title.is_empty()).then(|| json!({ "title": title, "artist": artists.join(", "), "duration": duration, "cover": cover, "thumbnail": cover }))
    })
    .collect();
  Ok(Some((tracks, total)))
}

async fn try_spotify_web_api(core: &Core, playlist_id: &str) -> Option<Value> {
  {
    let mut g = lock(core);
    load_auth(core, &mut g);
    if g.auth.token.is_empty() && g.auth.refresh_token.is_empty() {
      return None;
    }
  }
  let url = format!(
    "https://api.spotify.com/v1/playlists/{}",
    encode_uri_component(playlist_id)
  );
  let resp = get_with_retry(core, &url).await.ok()?;
  if !(200..300).contains(&resp.status) {
    return None;
  }
  let d: Value = serde_json::from_str(&resp.body).ok()?;
  let name = Some(st(d.get("name")))
    .filter(|n| !n.is_empty())
    .unwrap_or("Spotify Playlist")
    .to_string();
  let cover = d
    .pointer("/images/0/url")
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();
  let paging = pick(&[d.get("items"), d.get("tracks")])?;
  let mut items = paging.get("items")?.as_array()?.clone();
  let mut next = paging
    .get("next")
    .and_then(Value::as_str)
    .filter(|n| !n.is_empty())
    .map(str::to_string);
  let mut guard = 0;
  while let Some(url) = next.take() {
    if guard >= 100 {
      next = Some(url);
      break;
    }
    guard += 1;
    let r = get_with_retry(core, &url).await.ok()?;
    if !(200..300).contains(&r.status) {
      return None;
    }
    let nd: Value = serde_json::from_str(&r.body).ok()?;
    let arr = nd.get("items")?.as_array().filter(|a| !a.is_empty())?;
    items.extend(arr.iter().cloned());
    next = nd
      .get("next")
      .and_then(Value::as_str)
      .filter(|n| !n.is_empty())
      .map(str::to_string);
  }
  if next.is_some()
    || paging
      .get("total")
      .and_then(Value::as_u64)
      .is_some_and(|t| (items.len() as u64) < t)
  {
    return None;
  }
  let tracks: Vec<Value> = items
    .iter()
    .filter_map(|it| {
      let t = pick(&[it.get("item"), it.get("track")]).unwrap_or(it);
      let title = st(t.get("name"));
      if title.is_empty() {
        return None;
      }
      let cover = t
        .pointer("/album/images/0/url")
        .and_then(Value::as_str)
        .unwrap_or("");
      let artist = t
        .get("artists")
        .and_then(Value::as_array)
        .map(|a| {
          a.iter()
            .map(|x| st(x.get("name")))
            .collect::<Vec<_>>()
            .join(", ")
        })
        .unwrap_or_default();
      let mut entry =
        json!({ "title": title, "artist": artist, "cover": cover, "thumbnail": cover });
      set_opt(
        &mut entry,
        "duration",
        t.get("duration_ms")
          .and_then(Value::as_f64)
          .filter(|m| *m != 0.0)
          .map(round_secs),
      );
      Some(entry)
    })
    .collect();
  let total = tracks.len();
  Some(json!({ "success": true, "name": name, "cover": cover, "tracks": tracks, "total": total }))
}

async fn fetch_embed(client: &Http, playlist_id: &str, ua: &str) -> Result<(u16, String), String> {
  let resp = client
    .get(format!(
      "https://open.spotify.com/embed/playlist/{playlist_id}"
    ))
    .header(header::USER_AGENT, ua)
    .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
    .header(header::ACCEPT, "text/html,application/json")
    .header(header::REFERER, "https://open.spotify.com/")
    .timeout(Duration::from_secs(10))
    .send()
    .await
    .map_err(|e| e.to_string())?;
  let status = resp.status().as_u16();
  Ok((status, resp.text().await.map_err(|e| e.to_string())?))
}

fn spotify_walk(node: &Value, seen: &mut HashSet<String>, out: &mut Vec<Value>) {
  match node {
    Value::Array(arr) => arr.iter().for_each(|n| spotify_walk(n, seen, out)),
    Value::Object(map) => {
      let uri = st(node.get("uri"));
      let title = pick(&[node.get("title"), node.get("name")]);
      let artists_joined = || -> String {
        let names: Vec<String> = match node.get("artists").and_then(Value::as_array) {
          Some(arr) => arr
            .iter()
            .filter_map(|a| {
              a.as_str().map(str::to_string).or_else(|| {
                pick(&[a.get("name"), a.pointer("/profile/name")])
                  .and_then(Value::as_str)
                  .map(str::to_string)
              })
            })
            .collect(),
          None => node
            .pointer("/albumOfTrack/artists/items")
            .and_then(Value::as_array)
            .map(|a| {
              a.iter()
                .filter_map(|x| x.pointer("/profile/name").and_then(Value::as_str))
                .filter(|n| !n.is_empty())
                .map(str::to_string)
                .collect()
            })
            .unwrap_or_default(),
        };
        names.join(", ")
      };
      let subtitle: Option<Value> = pick(&[node.get("subtitle"), node.get("artist")])
        .cloned()
        .or_else(|| Some(json!(artists_joined())));
      let subtitle_truthy = subtitle.as_ref().is_some_and(|v| is_truthy(Some(v)));
      let duration = pick(&[
        node.pointer("/duration/totalMilliseconds"),
        node.pointer("/trackDuration/totalMilliseconds"),
        node.get("duration_ms"),
        node.get("duration").filter(|d| d.is_number()),
      ]);
      let is_track_uri = uri.starts_with("spotify:track:");
      if let Some(title) = title.and_then(Value::as_str)
        && (subtitle_truthy || is_track_uri)
        && (is_track_uri || st(node.get("entityType")) == "track" || duration.is_some())
      {
        let sub = subtitle.as_ref().and_then(Value::as_str).unwrap_or("");
        if seen.insert(format!("{title}__{sub}")) {
          let dur = duration
            .and_then(Value::as_f64)
            .map(|d| {
              if d > 1000.0 {
                round_secs(d)
              } else {
                d.round() as i64
              }
            })
            .unwrap_or(0);
          let cover = pick(&[node.get("image"), node.pointer("/coverArt/sources/0/url")])
            .and_then(Value::as_str)
            .unwrap_or("");
          out.push(json!({ "title": title.trim(), "artist": sub.trim(), "duration": dur, "thumbnail": cover, "cover": cover }));
        }
        return;
      }
      map.values().for_each(|n| spotify_walk(n, seen, out));
    }
    _ => {}
  }
}

pub async fn spotify_import(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let input = query.get("url").map(String::as_str).unwrap_or("");
  let Some(playlist_id) = RE_SPOTIFY_URL.captures(input).map(|c| c[1].to_string()) else {
    return err_json(
      400,
      json!({ "success": false, "code": "spotify_error", "error": "Invalid Spotify playlist URL" }),
    );
  };
  if let Some(result) = try_spotify_web_api(core, &playlist_id).await
    && result["tracks"].as_array().is_some_and(|t| !t.is_empty())
  {
    return json_response(200, &result);
  }
  let client = &core.net.http_client;
  let embed = match fetch_embed(client, &playlist_id, CHROME_UA).await {
    Ok((403 | 429, _)) => {
      tokio::time::sleep(Duration::from_millis(800)).await;
      fetch_embed(client, &playlist_id, FIREFOX_UA).await
    }
    other => other,
  };
  let Ok((status, body)) = embed else {
    return err_json(
      500,
      json!({ "success": false, "code": "spotify_error", "error": "Network error connecting to Spotify" }),
    );
  };
  match status {
    404 => {
      return err_json(
        404,
        json!({ "success": false, "code": "spotify_not_found", "error": "Playlist not found" }),
      );
    }
    403 => {
      return err_json(
        403,
        json!({ "success": false, "code": "spotify_forbidden", "error": "Spotify refused request (403)" }),
      );
    }
    429 => {
      return err_json(
        429,
        json!({ "success": false, "code": "spotify_rate_limited", "error": "Spotify rate limited (429)" }),
      );
    }
    200 => {}
    other => {
      return err_json(
        other,
        json!({ "success": false, "code": "spotify_error", "error": format!("Spotify returned status {other}") }),
      );
    }
  }

  let internal = || {
    err_json(
      500,
      json!({ "success": false, "code": "spotify_error", "error": "Internal server error" }),
    )
  };
  let Some(next_data) = RE_NEXT_DATA.captures(&body) else {
    let priv_ = RE_PRIVATE.is_match(&body);
    return err_json(
      403,
      json!({
        "success": false,
        "code": if priv_ { "spotify_forbidden" } else { "spotify_error" },
        "error": if priv_ { "Private playlist" } else { "Could not parse playlist" }
      }),
    );
  };
  let Ok(data) = serde_json::from_str::<Value>(&next_data[1]) else {
    return internal();
  };
  let page_props = data
    .pointer("/props/pageProps")
    .cloned()
    .unwrap_or(Value::Null);
  if page_props.get("status").and_then(Value::as_i64) == Some(404) {
    return err_json(
      404,
      json!({ "success": false, "code": "spotify_not_found", "error": "Playlist not found" }),
    );
  }
  let entity = page_props
    .pointer(PAGE_ENTITY)
    .cloned()
    .unwrap_or(Value::Null);
  let name = pick_str(&[entity.get("name"), page_props.get("title")]);
  let name = if name.is_empty() {
    "Spotify Playlist".to_string()
  } else {
    name
  };
  let og = RE_OG_IMAGE
    .captures(&body)
    .map(|c| c[1].to_string())
    .unwrap_or_default();
  let mut cover = pick_str(&[
    entity.pointer("/images/0/url"),
    entity.pointer("/coverArt/sources/0/url"),
    entity.get("image"),
    page_props.pointer("/metadata/image"),
  ]);
  if cover.is_empty() {
    cover = og;
  }

  let mut tracks: Vec<Value> = Vec::new();
  let mut limited = false;
  let mut total = 0u64;
  let access_token = page_props
    .pointer(PAGE_ACCESS_TOKEN)
    .and_then(Value::as_str)
    .filter(|t| !t.is_empty());
  match access_token {
    Some(token) => match fetch_full_playlist(client, &playlist_id, token).await {
      Ok(Some((t, tot))) if !t.is_empty() => {
        total = if tot != 0 { tot } else { t.len() as u64 };
        tracks = t;
      }
      _ => limited = true,
    },
    None => limited = true,
  }
  let track_list = entity.get("trackList").and_then(Value::as_array);
  if tracks.is_empty() {
    if let Some(list) = track_list.filter(|l| !l.is_empty()) {
      tracks = list
        .iter()
        .take(100)
        .filter_map(|t| {
          let dur = pick(&[t.get("duration"), t.get("duration_ms")]).and_then(Value::as_f64).map(round_secs).unwrap_or(0);
          let tc = pick_str(&[t.get("image"), t.pointer("/coverArt/sources/0/url")]);
          let title = pick_str(&[t.get("title"), t.get("name")]).trim().to_string();
          let artist = pick_str(&[t.get("subtitle"), t.get("artist")]).trim().to_string();
          (!title.is_empty()).then(|| json!({ "title": title, "artist": artist, "duration": dur, "thumbnail": tc, "cover": tc }))
        })
        .collect();
    }
    if tracks.is_empty() {
      let mut seen = HashSet::new();
      spotify_walk(&data, &mut seen, &mut tracks);
      tracks.truncate(100);
    }
  }
  if total == 0 {
    total = entity
      .get("totalCount")
      .and_then(Value::as_u64)
      .filter(|t| *t != 0)
      .unwrap_or_else(|| {
        let l = track_list.map(|l| l.len()).unwrap_or(0);
        if l != 0 {
          l as u64
        } else {
          tracks.len() as u64
        }
      });
  }
  if tracks.is_empty() {
    let priv_ = RE_PRIVATE.is_match(&body) || RE_PRIVATE.is_match(&page_props.to_string());
    return err_json(
      403,
      json!({
        "success": false,
        "code": if priv_ { "spotify_forbidden" } else { "spotify_error" },
        "error": if priv_ { "Private playlist" } else { "Could not read tracks" }
      }),
    );
  }
  let complete_embed = track_list.is_some_and(|l| {
    entity
      .get("totalCount")
      .and_then(Value::as_u64)
      .is_some_and(|tc| tc as usize <= l.len())
      && l.len() <= 100
  });
  if limited && !complete_embed {
    return err_json(
      409,
      json!({
        "success": false,
        "code": "spotify_incomplete",
        "error": "Spotify exposed only part of this playlist. Connect your account or use Export files."
      }),
    );
  }
  json_response(
    200,
    &json!({
      "success": true, "name": name, "cover": cover, "tracks": tracks, "hasMore": false, "total": total, "limited": limited
    }),
  )
}

// ------------------------------------------------------------------ Apple

fn apple_walk(node: &Value, ctx: &mut AppleCtx) {
  match node {
    Value::Array(arr) => arr.iter().for_each(|n| apple_walk(n, ctx)),
    Value::Object(map) => {
      let kind = st(node.pointer("/contentDescriptor/kind"));
      let ntype = st(node.get("type"));
      let at_type = st(node.get("@type"));
      if ctx.title.is_empty() {
        if kind == "playlist" || ntype == "playlists" || ntype == "playlist" {
          ctx.title = pick_str(&[
            node.get("title"),
            node.get("name"),
            node.pointer("/attributes/name"),
          ]);
          if let Some(u) = pick(&[
            node.pointer("/artwork/dictionary/url"),
            node.pointer("/artwork/url"),
          ])
          .and_then(Value::as_str)
          {
            ctx.cover = u.to_string();
          }
        } else if at_type == "MusicPlaylist" {
          ctx.title = st(node.get("name")).to_string();
        }
      }
      if ctx.total == 0
        && let Some(n) = node.get("trackCount").and_then(Value::as_u64)
      {
        ctx.total = n;
      }
      let title = match node.get("title").and_then(Value::as_str) {
        Some(t) => t,
        None if at_type == "MusicRecording" || ntype == "songs" => st(node.get("name")),
        None => "",
      }
      .trim();
      let artist = node
        .get("artistName")
        .and_then(Value::as_str)
        .or_else(|| node.get("artist").and_then(Value::as_str))
        .or_else(|| node.pointer("/byArtist/name").and_then(Value::as_str))
        .unwrap_or("")
        .trim();
      let dur_raw = pick(&[
        node.get("durationInMillis"),
        node.get("duration"),
        node.get("duration_ms"),
      ]);
      let duration: Option<i64> = match dur_raw {
        Some(Value::Number(n)) => n.as_f64().map(|d| {
          if d > 1000.0 {
            round_secs(d)
          } else {
            d.round() as i64
          }
        }),
        Some(Value::String(s)) if s.starts_with("PT") => RE_ISO_DURATION.captures(s).map(|c| {
          let g = |i: usize| {
            c.get(i)
              .and_then(|m| m.as_str().parse::<i64>().ok())
              .unwrap_or(0)
          };
          g(1) * 60 + g(2)
        }),
        _ => None,
      };
      let is_song = matches!(kind, "song" | "music-video")
        || matches!(ntype, "songs" | "song" | "library-songs")
        || at_type == "MusicRecording"
        || duration.is_some_and(|d| d != 0)
        || is_truthy(node.get("audioTraits"));
      if !title.is_empty() && !artist.is_empty() && is_song {
        if ctx.seen.insert(format!(
          "{}___{}",
          title.to_lowercase(),
          artist.to_lowercase()
        )) {
          let art = pick_str(&[
            node.pointer("/artwork/dictionary/url"),
            node.pointer("/artwork/url"),
          ])
          .replace("{w}x{h}bb.{f}", "600x600bb.jpg")
          .replace("{w}x{h}", "600x600");
          let mut entry =
            json!({ "title": title, "artist": artist, "thumbnail": art, "cover": art });
          set_opt(&mut entry, "duration", duration);
          ctx.tracks.push(entry);
        }
        return;
      }
      map.values().for_each(|n| apple_walk(n, ctx));
    }
    _ => {}
  }
}

#[derive(Default)]
struct AppleCtx {
  title: String,
  cover: String,
  total: u64,
  seen: HashSet<String>,
  tracks: Vec<Value>,
}

async fn apple_album(core: &Core, album_id: &str) -> Option<Value> {
  let data = fetch_itunes_json(
    &core.net.http_client,
    &format!("https://itunes.apple.com/lookup?id={album_id}&entity=song"),
  )
  .await?;
  let results = data.get("results")?.as_array()?;
  let album = results
    .iter()
    .find(|r| st(r.get("wrapperType")) == "collection")?;
  let art = |v: &str| v.replace("100x100", "600x600");
  let tracks: Vec<Value> = results
    .iter()
    .filter(|r| st(r.get("wrapperType")) == "track")
    .map(|item| {
      let art_url = art(&pick_str(&[
        item.get("artworkUrl100"),
        album.get("artworkUrl100"),
      ]));
      let dur = item
        .get("trackTimeMillis")
        .and_then(Value::as_f64)
        .filter(|m| *m != 0.0)
        .map(round_secs)
        .unwrap_or(0);
      json!({
        "title": st(item.get("trackName")),
        "artist": pick_str(&[item.get("artistName"), album.get("artistName")]),
        "duration": dur,
        "thumbnail": art_url,
        "cover": art_url
      })
    })
    .collect();
  if tracks.is_empty() {
    return None;
  }
  let name = Some(st(album.get("collectionName")))
    .filter(|n| !n.is_empty())
    .unwrap_or("Album");
  Some(
    json!({ "success": true, "name": name, "cover": art(st(album.get("artworkUrl100"))), "tracks": tracks }),
  )
}

pub async fn apple_import(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let cleaned = clean_url(query.get("url").map_or("", String::as_str));
  let input = cleaned.as_str();
  if let Some(album_id) = RE_APPLE_ALBUM.captures(input).map(|c| c[1].to_string())
    && let Some(result) = apple_album(core, &album_id).await
  {
    return json_response(200, &result);
  }
  if !RE_APPLE_PLAYLIST.is_match(input) {
    return err_json(
      400,
      json!({ "success": false, "error": "Invalid Apple Music playlist URL" }),
    );
  }
  let resp = core
    .net
    .http_client
    .get(input)
    .header(header::USER_AGENT, CHROME_UA)
    .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
    .timeout(Duration::from_secs(15))
    .send()
    .await;
  let (status, body) = match resp {
    Ok(r) => {
      let status = r.status().as_u16();
      match r.text().await {
        Ok(b) => (status, b),
        Err(e) => return apple_net_error(&e),
      }
    }
    Err(e) => return apple_net_error(&e),
  };
  match status {
    404 => {
      return err_json(
        404,
        json!({ "success": false, "error": "not_found", "status": 404 }),
      );
    }
    429 => {
      return err_json(
        429,
        json!({ "success": false, "error": "rate_limit", "status": 429 }),
      );
    }
    401 | 403 => {
      return err_json(
        403,
        json!({ "success": false, "error": "private", "status": 403 }),
      );
    }
    _ => {}
  }
  let parsed = RE_APPLE_SERVER_DATA
    .captures(&body)
    .and_then(|c| serde_json::from_str::<Value>(&c[1]).ok())
    .or_else(|| {
      RE_APPLE_LD
        .captures(&body)
        .and_then(|c| serde_json::from_str::<Value>(&c[1]).ok())
    });
  let Some(parsed) = parsed else {
    let priv_ = RE_APPLE_PRIV.is_match(&body);
    return err_json(
      403,
      json!({ "success": false, "error": if priv_ { "private" } else { "restricted" }, "status": 403 }),
    );
  };
  let mut ctx = AppleCtx::default();
  apple_walk(&parsed, &mut ctx);
  if ctx.title.is_empty() {
    if let Some(c) = RE_APPLE_OG_TITLE.captures(&body) {
      ctx.title = RE_APPLE_ON_AM.replace(&c[1], "").trim().to_string();
    } else if let Some(c) = RE_HTML_TITLE.captures(&body) {
      let t = RE_APPLE_TITLE_SUFFIX.replace(&c[1], "");
      ctx.title = RE_LEADING_SYMBOLS.replace(&t, "").trim().to_string();
    }
  }
  if ctx.cover.contains("{w}x{h}bb.{f}") {
    ctx.cover = ctx.cover.replace("{w}x{h}bb.{f}", "600x600bb.jpg");
  }
  if ctx.tracks.is_empty() {
    return err_json(
      403,
      json!({ "success": false, "error": "private", "status": 403 }),
    );
  }
  let total = if ctx.total != 0 {
    ctx.total
  } else {
    ctx.tracks.len() as u64
  };
  let name = if ctx.title.is_empty() {
    "Apple Music Playlist".to_string()
  } else {
    ctx.title
  };
  json_response(
    200,
    &json!({
      "success": true, "name": name, "cover": ctx.cover, "tracks": ctx.tracks,
      "hasMore": total > ctx.tracks.len() as u64, "total": total
    }),
  )
}

fn apple_net_error(e: &crate::net::Error) -> CoreResult {
  let net = e.is_timeout() || e.is_connect() || e.is_request();
  if net {
    err_json(
      504,
      json!({ "success": false, "error": "network", "status": 504 }),
    )
  } else {
    err_json(
      500,
      json!({ "success": false, "error": "unknown", "status": 500 }),
    )
  }
}

// ----------------------------------------------------------------- Yandex

const YANDEX_API: &str = "https://api.music.yandex.net";

enum YandexLink {
  Playlist(String),
  Track(Vec<String>),
  Album(String),
}

fn parse_yandex_link(input: &str) -> Option<YandexLink> {
  let text = input.trim();
  if let Some(m) = RE_YANDEX_USER_PL.captures(text) {
    return Some(YandexLink::Playlist(format!(
      "/users/{}/playlists/{}",
      &m[1], &m[2]
    )));
  }
  if let Some(m) = RE_YANDEX_PL.captures(text) {
    return Some(YandexLink::Playlist(format!("/playlist/{}", &m[1])));
  }
  if let Some(m) = RE_YANDEX_ALBUM_TRACK.captures(text) {
    return Some(YandexLink::Track(vec![format!("{}:{}", &m[2], &m[1])]));
  }
  if let Some(m) = RE_YANDEX_TRACK.captures(text) {
    return Some(YandexLink::Track(vec![m[1].to_string()]));
  }
  RE_YANDEX_ALBUM
    .captures(text)
    .map(|m| YandexLink::Album(m[1].to_string()))
}

async fn yandex_request(
  client: &Http,
  path: &str,
  form: Option<String>,
) -> Result<(u16, Option<Value>), crate::net::Error> {
  let mut last_err = None;
  for attempt in 0..2 {
    let mut req = client
      .request(
        if form.is_some() {
          reqwest::Method::POST
        } else {
          reqwest::Method::GET
        },
        format!("{YANDEX_API}{path}"),
      )
      .header(header::USER_AGENT, "Yandex-Music-API")
      .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023231")
      .header(header::ACCEPT, "application/json")
      .timeout(Duration::from_secs(15));
    if let Some(body) = &form {
      req = req
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body.clone());
    }
    match req.send().await {
      Ok(resp) => {
        let status = resp.status().as_u16();
        if status >= 500 && attempt == 0 {
          tokio::time::sleep(Duration::from_millis(400)).await;
          continue;
        }
        return Ok((status, resp.json::<Value>().await.ok()));
      }
      Err(e) => last_err = Some(e),
    }
  }
  Err(last_err.expect("deux tentatives sans réponse"))
}

async fn fetch_yandex_tracks_by_ids(
  client: &Http,
  ids: &[String],
) -> Result<Vec<Value>, crate::net::Error> {
  let mut found: Vec<(String, Value)> = Vec::new();
  for chunk in ids.chunks(100) {
    let body = url::form_urlencoded::Serializer::new(String::new())
      .append_pair("track-ids", &chunk.join(","))
      .finish();
    let (status, data) = yandex_request(client, "/tracks", Some(body)).await?;
    if !(200..300).contains(&status) {
      continue;
    }
    for t in data
      .as_ref()
      .and_then(|d| d.get("result"))
      .and_then(Value::as_array)
      .into_iter()
      .flatten()
    {
      let id = value_to_string(t.get("id"));
      if t.get("id").is_some_and(|v| !v.is_null()) {
        found.retain(|(k, _)| *k != id);
        found.push((id, t.clone()));
      }
    }
  }
  Ok(found.into_iter().map(|(_, v)| v).collect())
}

fn yandex_cover_url(uri: &str) -> String {
  if uri.is_empty() {
    return String::new();
  }
  let value = uri.replacen("%%", "400x400", 1);
  if RE_HTTP.is_match(&value) {
    value
  } else {
    format!("https://{value}")
  }
}

fn normalize_yandex_track(t: &Value, fallback_album: &str) -> Option<Value> {
  if !is_truthy(t.get("title")) {
    return None;
  }
  let mut title = value_to_string(t.get("title")).trim().to_string();
  let version = st(t.get("version")).trim();
  if !version.is_empty() && !title.to_lowercase().contains(&version.to_lowercase()) {
    title.push_str(&format!(" ({version})"));
  }
  let names: Vec<&str> = t
    .get("artists")
    .and_then(Value::as_array)
    .map(|a| {
      a.iter()
        .map(|x| st(x.get("name")))
        .filter(|n| !n.is_empty())
        .collect()
    })
    .unwrap_or_default();
  let artist = if names.is_empty() {
    st(t.get("artist")).to_string()
  } else {
    names.join(", ")
  };
  let album = t.pointer("/albums/0").filter(|a| is_truthy(Some(a)));
  let cover_src = pick_str(&[
    t.get("ogImage"),
    t.get("coverUri"),
    album.and_then(|a| a.get("coverUri")),
    t.get("cover").filter(|c| c.is_string()),
  ]);
  let mut cover = yandex_cover_url(&cover_src);
  if cover.is_empty() {
    cover = pick_str(&[t.get("thumbnail"), t.get("image")]);
  }
  let duration = match (
    t.get("durationMs")
      .and_then(Value::as_f64)
      .filter(|d| *d != 0.0),
    t.get("duration")
      .and_then(Value::as_f64)
      .filter(|d| *d != 0.0),
  ) {
    (Some(ms), _) => round_secs(ms),
    (None, Some(d)) => d.round() as i64,
    _ => 0,
  };
  let album_title = album
    .map(|a| st(a.get("title")).to_string())
    .filter(|a| !a.is_empty())
    .unwrap_or_else(|| fallback_album.to_string());
  Some(json!({
    "title": title, "artist": artist, "album": album_title, "duration": duration,
    "cover": cover, "thumbnail": cover, "image": cover
  }))
}

fn yandex_fail(status: u16) -> CoreResult {
  match status {
    401 | 403 => err_json(
      403,
      json!({ "success": false, "error": "private", "status": 403 }),
    ),
    404 => err_json(
      404,
      json!({ "success": false, "error": "not found", "status": 404 }),
    ),
    429 => err_json(
      429,
      json!({ "success": false, "error": "too many requests", "status": 429 }),
    ),
    s => {
      let s = if s == 0 { 502 } else { s };
      err_json(
        s,
        json!({ "success": false, "error": "Failed to fetch from Yandex Music", "status": s }),
      )
    }
  }
}

async fn yandex_import_inner(
  client: &Http,
  link: YandexLink,
) -> Result<CoreResult, crate::net::Error> {
  let mut name = "Yandex Music Playlist".to_string();
  let cover;
  let raw_tracks: Vec<Value>;
  match link {
    YandexLink::Playlist(path) => {
      let (status, data) = yandex_request(client, &path, None).await?;
      if !(200..300).contains(&status) {
        return Ok(yandex_fail(status));
      }
      let data = data.unwrap_or(Value::Null);
      let r = data
        .get("result")
        .filter(|r| is_truthy(Some(r)))
        .unwrap_or(&data);
      let n = pick_str(&[r.get("title"), r.get("name")]);
      if !n.is_empty() {
        name = n;
      }
      cover = yandex_cover_url(&pick_str(&[
        r.get("ogImage"),
        r.get("coverUrl"),
        r.pointer("/cover/uri"),
        r.get("cover").filter(|c| c.is_string()),
      ]));
      let items = r
        .get("tracks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
      let mut resolved: Vec<Option<Value>> = vec![None; items.len()];
      let mut missing: Vec<(usize, String, String)> = Vec::new();
      for (idx, item) in items.iter().enumerate() {
        let t = item
          .get("track")
          .filter(|t| is_truthy(Some(t)))
          .or_else(|| Some(item).filter(|i| is_truthy(i.get("title"))));
        if let Some(t) = t.filter(|t| is_truthy(t.get("title"))) {
          resolved[idx] = Some(t.clone());
        } else if item.get("id").is_some_and(|v| !v.is_null()) {
          let id = value_to_string(item.get("id"));
          let key = if item.get("albumId").is_some_and(|v| !v.is_null()) {
            format!("{id}:{}", value_to_string(item.get("albumId")))
          } else {
            id.clone()
          };
          missing.push((idx, id, key));
        }
      }
      if !missing.is_empty() {
        let keys: Vec<String> = missing.iter().map(|m| m.2.clone()).collect();
        let found = fetch_yandex_tracks_by_ids(client, &keys).await?;
        for (idx, id, _) in missing {
          resolved[idx] = found
            .iter()
            .find(|t| value_to_string(t.get("id")) == id)
            .cloned();
        }
      }
      raw_tracks = resolved
        .into_iter()
        .map(|t| t.unwrap_or(Value::Null))
        .collect();
    }
    YandexLink::Album(id) => {
      let (status, data) =
        yandex_request(client, &format!("/albums/{id}/with-tracks"), None).await?;
      if !(200..300).contains(&status) {
        return Ok(yandex_fail(status));
      }
      let data = data.unwrap_or(Value::Null);
      let r = data.get("result").cloned().unwrap_or(json!({}));
      name = Some(st(r.get("title")))
        .filter(|t| !t.is_empty())
        .unwrap_or("Yandex Music Album")
        .to_string();
      cover = yandex_cover_url(&pick_str(&[r.get("ogImage"), r.get("coverUri")]));
      let album_stub = json!([{ "title": r.get("title"), "coverUri": r.get("coverUri") }]);
      raw_tracks = r
        .get("volumes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|v| v.as_array().cloned().unwrap_or_default())
        .map(|mut t| {
          if t.is_object() && t.get("albums").is_none() {
            t["albums"] = album_stub.clone();
          }
          t
        })
        .collect();
    }
    YandexLink::Track(ids) => {
      let found = fetch_yandex_tracks_by_ids(client, &ids).await?;
      if found.is_empty() {
        return Ok(yandex_fail(404));
      }
      let first = normalize_yandex_track(&found[0], "");
      name = first
        .as_ref()
        .map(|f| st(f.get("title")).to_string())
        .unwrap_or_else(|| "Yandex Music Track".to_string());
      cover = first
        .as_ref()
        .map(|f| st(f.get("cover")).to_string())
        .unwrap_or_default();
      raw_tracks = found;
    }
  }
  let tracks: Vec<Value> = raw_tracks
    .iter()
    .filter_map(|t| normalize_yandex_track(t, ""))
    .collect();
  if tracks.is_empty() {
    return Ok(yandex_fail(404));
  }
  Ok(json_response(
    200,
    &json!({
      "success": true, "name": name, "cover": cover, "tracks": tracks,
      "total": raw_tracks.len(), "skipped": raw_tracks.len() - tracks.len()
    }),
  ))
}

pub async fn yandex_import(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let input = query.get("url").map(String::as_str).unwrap_or("");
  let Some(link) = parse_yandex_link(input) else {
    return err_json(
      400,
      json!({ "success": false, "error": "unsupported link", "status": 400 }),
    );
  };
  match yandex_import_inner(&core.net.http_client, link).await {
    Ok(resp) => resp,
    Err(e) => {
      let net = e.is_timeout() || e.is_connect() || e.is_request();
      if net {
        err_json(
          504,
          json!({ "success": false, "error": "network", "status": 504 }),
        )
      } else {
        err_json(
          500,
          json!({ "success": false, "error": e.to_string(), "status": 500 }),
        )
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn spotify_page_pointers_match_the_page_structure() {
    let page = json!({ "state": { "data": { "entity": { "name": "Mix" } }, "settings": { "session": { "accessToken": "t" } } } });
    assert_eq!(page.pointer(PAGE_ENTITY), Some(&json!({ "name": "Mix" })));
    assert_eq!(page.pointer(PAGE_ACCESS_TOKEN), Some(&json!("t")));
  }
}
