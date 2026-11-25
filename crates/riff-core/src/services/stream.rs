use super::playback::{AUDIO_EXTENSIONS, downloads_dir};
use super::search::direct_youtube_search;
use super::*;
use futures_util::future::{join, join_all, select_ok};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::watch;

pub type InflightValue = Option<Option<String>>;

const STREAM_CACHE_TTL_MS: u64 = 7_200_000;
const STREAM_CACHE_MAX: usize = 35;
const SC_AUDIO_FORMAT: &str = "http_mp3_128/bestaudio[ext=mp3][protocol^=http][protocol!=http_dash_segments]/bestaudio[ext=m4a][protocol^=http]/bestaudio[acodec^=mp4a]/bestaudio[protocol=https]/bestaudio/18/best";
// Écart voulu avec Riff (server.js:3021, `bestaudio/best`) : le moteur ne décode pas Opus, or `bestaudio` choisit
// le format 251 (WebM, Opus) pour YouTube. On préfère le m4a (AAC) progressif, sans HLS ni DASH, le reste en dernier.
const YT_DLP_AUDIO_FORMAT: &str = "bestaudio[ext=m4a][protocol^=http][protocol!=http_dash_segments]/bestaudio[ext=m4a]/18/bestaudio/best";
pub const YT_EXTRACTOR_ARGS: &str = "youtube:player_client=default,android_vr";

// ---------------------------------------------------------------------------
// Cache des URLs de flux, résolutions en cours, préchargement
// ---------------------------------------------------------------------------

fn stream_cache_get(core: &Core, key: &str) -> Option<(u64, Value)> {
  let guard = core.caches.stream_url_cache.lock().ok()?;
  guard.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

// Les URLs signées de SoundCloud portent `expires` (secondes Unix) : une entrée de cache qui expire dans
// moins de `EXPIRY_MARGIN_MS` ne doit plus être servie.
const EXPIRY_MARGIN_MS: u64 = 600_000;

fn expires_soon(data: &Value, now_ms: u64) -> bool {
  let Some(raw) = data.get("rawStreamUrl").and_then(Value::as_str) else {
    return false;
  };
  let Some(query) = raw.split_once('?').map(|(_, query)| query) else {
    return false;
  };
  query
    .split('&')
    .find_map(|pair| pair.strip_prefix("expires="))
    .and_then(|value| value.parse::<u64>().ok())
    .is_some_and(|secs| secs.saturating_mul(1000) < now_ms.saturating_add(EXPIRY_MARGIN_MS))
}

fn stream_cache_set(core: &Core, key: &str, data: &Value) {
  if let Ok(mut guard) = core.caches.stream_url_cache.lock() {
    guard.retain(|(k, _)| k != key);
    if guard.len() >= STREAM_CACHE_MAX {
      guard.remove(0);
    }
    guard.push((key.to_string(), (now_millis(), data.clone())));
  }
}

fn stream_cache_delete(core: &Core, key: &str) {
  if let Ok(mut guard) = core.caches.stream_url_cache.lock() {
    guard.retain(|(k, _)| k != key);
  }
}

// Une résolution par clé à la fois : les appels concurrents attendent le résultat du premier.
struct InflightGuard<'a> {
  core: &'a Core,
  key: String,
  tx: watch::Sender<InflightValue>,
  rx: watch::Receiver<InflightValue>,
}

impl InflightGuard<'_> {
  fn resolve(&self, url: Option<String>) {
    self.tx.send_replace(Some(url));
  }
}

impl Drop for InflightGuard<'_> {
  fn drop(&mut self) {
    if let Ok(mut map) = self.core.playback.stream_inflight.lock()
      && map.get(&self.key).is_some_and(|r| r.same_channel(&self.rx))
    {
      map.remove(&self.key);
    }
  }
}

fn register_inflight<'a>(core: &'a Core, key: &str) -> InflightGuard<'a> {
  let (tx, rx) = watch::channel(None);
  if let Ok(mut map) = core.playback.stream_inflight.lock() {
    map.insert(key.to_string(), rx.clone());
  }
  InflightGuard {
    core,
    key: key.to_string(),
    tx,
    rx,
  }
}

// None : rien en cours pour cette clé. Some(None) : la résolution en cours a échoué.
async fn wait_inflight(core: &Core, key: &str) -> Option<Option<String>> {
  let mut rx = core
    .playback
    .stream_inflight
    .lock()
    .ok()?
    .get(key)
    .cloned()?;
  let result = match rx.wait_for(|v| v.is_some()).await {
    Ok(g) => (*g).clone().flatten(),
    Err(_) => None,
  };
  Some(result)
}

fn local_track_url(base: &str, id: &str) -> String {
  format!("{base}/api/local-track?id={}", encode_uri_component(id))
}

// Écart voulu avec Riff : pour un titre qui n'est pas SoundCloud (YouTube, Deezer, iTunes), un flux SoundCloud
// `.m3u8` trouvé par recherche est écarté afin que la résolution continue (yt-dlp, recherche). riff-rs sait lire
// le HLS AAC fMP4 de SoundCloud, mais ne substitue pas SoundCloud à un titre YouTube.
fn drop_hls(url: Option<String>) -> Option<String> {
  url.filter(|u| !u.contains(".m3u8"))
}

/// `keep_hls` : le titre demandé est SoundCloud, dont le flux n'est plus que du HLS ; sinon un HLS trouvé
/// par recherche est écarté.
async fn fetch_sc_stream_by_query(core: &Core, query: &str, keep_hls: bool) -> Option<String> {
  let url = soundcloud::fetch_direct_sc_stream(core, "", query).await;
  if keep_hls { url } else { drop_hls(url) }
}

fn proxied_url(base: &str, raw: &str) -> String {
  format!("{base}/api/audio-proxy?url={}", encode_uri_component(raw))
}

fn stream_data(base: &str, raw: &str, resolved_id: Option<&str>) -> Value {
  let is_hls = raw.contains(".m3u8");
  let proxied = proxied_url(base, raw);
  let mut data = json!({
    "success": true,
    "isLocal": false,
    "streamUrl": if is_hls { raw.to_string() } else { proxied.clone() },
    "rawStreamUrl": raw,
    "proxyUrl": proxied,
    "isHls": is_hls
  });
  if let (Some(id), Some(obj)) = (resolved_id, data.as_object_mut()) {
    obj.insert("resolvedId".to_string(), json!(id));
  }
  data
}

// Préchargement : résout l'URL de flux (sans yt-dlp) pour que le clic sur lecture soit instantané.
pub async fn prefetch_track(core: &Core, track_url: &str, track_id: &str, platform: &str) {
  let track_url = soundcloud::normalize_soundcloud_url(track_url);
  let mut clean_id = clean_track_id(track_id);
  if clean_id.is_empty()
    && let Some((_, after)) = track_url.split_once("v=")
  {
    clean_id = clean_track_id(after.split('&').next().unwrap_or(""));
  }
  let key_src = if clean_id.is_empty() {
    track_url.as_str()
  } else {
    clean_id.as_str()
  };
  if key_src.is_empty() {
    return;
  }
  let cache_key = format!("{}_medium", key_src.to_lowercase());
  {
    let Ok(mut set) = core.playback.prefetching.lock() else {
      return;
    };
    if set.len() > 2 || set.contains(&cache_key) {
      return;
    }
    let in_flight = core
      .playback
      .stream_inflight
      .lock()
      .map(|m| m.contains_key(&cache_key))
      .unwrap_or(true);
    if in_flight || stream_cache_get(core, &cache_key).is_some() {
      return;
    }
    set.insert(cache_key.clone());
  }
  let guard = register_inflight(core, &cache_key);
  let stream = if platform == "soundcloud" || track_url.contains("soundcloud.com") {
    let src = soundcloud::sc_stream_source(&track_url, &clean_id);
    soundcloud::fetch_direct_sc_stream(core, src, "").await
  } else if !clean_id.is_empty() && !youtube::extract_yt_id_from_any(&clean_id).is_empty() {
    youtube::fetch_direct_yt_stream(core, &clean_id, "medium").await
  } else {
    None
  };
  if let Some(url) = &stream {
    let data = stream_data(core.local_base_url(), url, None);
    stream_cache_set(core, &cache_key, &data);
    if !clean_id.is_empty() {
      stream_cache_set(core, &format!("{clean_id}_medium"), &data);
    }
  }
  guard.resolve(stream);
  drop(guard);
  if let Ok(mut set) = core.playback.prefetching.lock() {
    set.remove(&cache_key);
  }
}

// ---------------------------------------------------------------------------
// Fichiers locaux : métadonnées et dossier de téléchargements
// ---------------------------------------------------------------------------

pub fn sanitize_filename(s: &str) -> String {
  let base = if s.is_empty() { "track" } else { s };
  let cleaned: String = base
    .chars()
    .filter(|c| !"<>:\"/\\|?*".contains(*c))
    .collect();
  cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_cached_file_by_meta(core: &Core, title: &str, artist: &str) -> Option<PathBuf> {
  if title.is_empty() {
    return None;
  }
  let target_title = title.to_lowercase().trim().to_string();
  let target_artist = artist.to_lowercase().trim().to_string();
  let index = core.library.track_metadata_index.lock().ok()?;
  for (cid, meta) in index.iter() {
    let m_title = meta.get("title").and_then(Value::as_str).unwrap_or("");
    if m_title.is_empty() || m_title.to_lowercase().trim() != target_title {
      continue;
    }
    let m_artist = meta.get("artist").and_then(Value::as_str).unwrap_or("");
    let m_artist_l = m_artist.to_lowercase().trim().to_string();
    if artist.is_empty()
      || m_artist.is_empty()
      || m_artist_l == target_artist
      || m_artist_l.contains(&target_artist)
    {
      for ext in [".opus", ".webm", ".m4a", ".mp3"] {
        let fp = paths::tracks_dir().join(format!("{cid}{ext}"));
        if fp.exists() {
          return Some(fp);
        }
      }
    }
  }
  None
}

static DOWNLOADS_INDEX: Mutex<(u64, Option<HashMap<String, String>>)> = Mutex::new((0, None));

// Un téléchargement ou un encodage vient d'ajouter un fichier : l'index est reconstruit à la prochaine recherche.
pub fn invalidate_downloads_index() {
  if let Ok(mut g) = DOWNLOADS_INDEX.lock() {
    *g = (0, None);
  }
}

fn find_downloaded_file(title: &str, artist: &str) -> Option<String> {
  if title.is_empty() && artist.is_empty() {
    return None;
  }
  let mut guard = DOWNLOADS_INDEX.lock().ok()?;
  if guard.1.is_none() || now_millis().saturating_sub(guard.0) > 15_000 {
    let mut map = HashMap::new();
    if let Ok(entries) = fs::read_dir(downloads_dir()) {
      for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
          continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let p = Path::new(&name);
        let ext = p
          .extension()
          .and_then(|e| e.to_str())
          .unwrap_or("")
          .to_lowercase();
        if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
          continue;
        }
        let stem = p
          .file_stem()
          .and_then(|s| s.to_str())
          .unwrap_or("")
          .to_lowercase();
        if !stem.is_empty() {
          map.insert(stem, name);
        }
      }
    }
    *guard = (now_millis(), Some(map));
  }
  let key = format!(
    "{} - {}",
    sanitize_filename(if artist.is_empty() {
      "unknown artist"
    } else {
      artist
    }),
    sanitize_filename(if title.is_empty() { "untitled" } else { title })
  )
  .to_lowercase();
  guard.1.as_ref()?.get(&key).cloned()
}

// ---------------------------------------------------------------------------
// Classement des candidats et requêtes de recherche
// ---------------------------------------------------------------------------

fn strip_symbols(s: &str) -> String {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| Regex::new(r"[^\p{L}\p{N}\s]").expect("regex"));
  re.replace_all(s, " ")
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

fn norm_text(s: &str) -> String {
  strip_symbols(&s.to_lowercase())
}

// Premier artiste d'une liste. Différence voulue avec Riff : feat/ft ne coupent que comme mots entiers
// (Riff coupait "Daft Punk" en "Da" et "Taylor Swift" en "Taylor Swi").
fn first_artist_part(artist: &str) -> String {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| Regex::new(r"(?i)[,;&/]|\bfeat\b\.?|\bft\b\.?").expect("regex"));
  re.split(artist).next().unwrap_or(artist).to_string()
}

fn make_clean_search_query(artist: &str, title: &str) -> String {
  static RE_TOPIC: OnceLock<Regex> = OnceLock::new();
  static RE_VEVO: OnceLock<Regex> = OnceLock::new();
  static RE_FEAT: OnceLock<Regex> = OnceLock::new();
  static RE_NOISE: OnceLock<Regex> = OnceLock::new();
  let re_topic = RE_TOPIC.get_or_init(|| Regex::new(r"(?i)\s*-\s*Topic$").expect("regex"));
  let re_vevo = RE_VEVO.get_or_init(|| Regex::new(r"(?i)\bVEVO$").expect("regex"));
  let re_feat = RE_FEAT.get_or_init(|| {
    Regex::new(r"(?i)\s*[(\[]\s*(?:feat\.?|ft\.?|prod\.?|featuring)[^)]*[)\]]").expect("regex")
  });
  let re_noise = RE_NOISE.get_or_init(|| {
    Regex::new(r"(?i)\s*[(\[]\s*(?:official\s*(?:music\s*)?(?:video|audio|lyric\s*video|visualizer)|lyrics?\s*video|lyrics?|visualizer|hd|hq|4k|mv|clip)\s*[)\]]")
      .expect("regex")
  });
  let a = re_topic.replace(artist, "");
  let a = re_vevo.replace(&a, "");
  let clean_a = strip_symbols(&first_artist_part(&a));
  let t = re_feat.replace_all(title, "");
  let t = re_noise.replace_all(&t, "");
  let clean_t = strip_symbols(&t);
  format!("{clean_a} {clean_t}").trim().to_string()
}

fn score_imported_candidate(
  target_title: &str,
  target_artist: &str,
  target_dur: f64,
  cand: &Value,
) -> f64 {
  let cand_title = cand.get("title").and_then(Value::as_str).unwrap_or("");
  if cand_title.is_empty() {
    return -999.0;
  }
  let t_title = norm_text(target_title);
  let c_title = norm_text(cand_title);
  let mut t_artist = norm_text(&first_artist_part(target_artist));
  if t_artist.is_empty() {
    t_artist = norm_text(target_artist);
  }
  let c_artist = norm_text(cand.get("artist").and_then(Value::as_str).unwrap_or(""));
  let c_full = format!("{c_artist} {c_title}");
  if t_title.is_empty() || c_title.is_empty() {
    return -999.0;
  }

  let mut score = 0.0;
  if c_title == t_title {
    score += 55.0;
  } else if c_title.contains(&t_title) || t_title.contains(&c_title) {
    score += 35.0;
  } else {
    let words: Vec<&str> = t_title
      .split(' ')
      .filter(|w| w.chars().count() > 1)
      .collect();
    if !words.is_empty() {
      let hits = words.iter().filter(|w| c_title.contains(*w)).count();
      let ratio = hits as f64 / words.len() as f64;
      if ratio < 0.45 {
        return -500.0;
      }
      score += ratio * 28.0;
    }
  }

  if !t_artist.is_empty() && t_artist != "unknown artist" {
    if c_artist == t_artist || c_artist.contains(&t_artist) || t_artist.contains(&c_artist) {
      score += 45.0;
    } else if c_full.contains(&t_artist) {
      score += 32.0;
    } else {
      score -= 35.0;
    }
  }

  const MODIFIERS: [&str; 18] = [
    "slowed",
    "reverb",
    "sped up",
    "speed up",
    "nightcore",
    "hardstyle",
    "remix",
    "bootleg",
    "mashup",
    "8d",
    "karaoke",
    "instrumental",
    "cover",
    "live",
    "tribute",
    "bass boosted",
    "phonk remix",
    "edit",
  ];
  let norm_target_artist = norm_text(target_artist);
  for m in MODIFIERS {
    let in_target = t_title.contains(m) || norm_target_artist.contains(m);
    let in_cand = c_full.contains(m);
    if in_cand && !in_target {
      score -= 95.0;
    } else if in_cand && in_target {
      score += 22.0;
    }
  }

  let c_dur = cand.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
  if target_dur > 30.0 && c_dur > 30.0 {
    let diff = (target_dur - c_dur).abs();
    if diff <= 3.0 {
      score += 35.0;
    } else if diff <= 8.0 {
      score += 20.0;
    } else if diff <= 15.0 {
      score += 8.0;
    } else if diff > 45.0 {
      score -= 35.0;
    }
  }
  if target_dur > 45.0 && c_dur > 0.0 && c_dur < 35.0 {
    return -500.0;
  }
  score
}

// ---------------------------------------------------------------------------
// /api/stream-info
// ---------------------------------------------------------------------------

struct StreamReq {
  track_url: String,
  track_id: String,
  platform: String,
  title: String,
  artist: String,
  duration: f64,
  for_share: bool,
  force_yt_dlp: bool,
  fresh: bool,
  quality: String,
  query_q: String,
}

fn first_http_lines(raw: &str) -> Vec<String> {
  raw
    .trim()
    .lines()
    .map(|l| l.trim().to_string())
    .filter(|l| l.starts_with("http"))
    .collect()
}

// Comme `first_http_lines`, sans les flux `.m3u8` : un titre YouTube ne bascule pas sur un HLS.
fn non_hls_http_lines(raw: &str) -> Vec<String> {
  let mut lines = first_http_lines(raw);
  lines.retain(|l| !l.contains(".m3u8"));
  lines
}

// Clients yt-dlp essayés dans l'ordre pour la relance `forceYtDlp`. Mesuré le 2026-11-16 : `tv_simply` rend une URL
// qui répond 206 aux plages ouvertes, `web_embedded` aussi mais pas toujours ; le client par défaut (`TVHTML5`) répond
// 403. Volatil : la sonde `youtube::accept_stream_url` décide, cet ordre ne fait que fixer les essais.
const YT_DLP_PLAYER_CLIENTS: [Option<&str>; 3] = [Some("tv_simply"), Some("web_embedded"), None];
// Budget total des essais, sous le délai de 20 s de `stream_info`. Un essai de yt-dlp dure environ 4 s.
const YT_DLP_BUDGET_MS: u64 = 17_000;
const YT_DLP_ATTEMPT_MAX_MS: u64 = 15_000;
const YT_DLP_ATTEMPT_MIN_MS: u64 = 3_000;

fn yt_dlp_stream_args(v_url: &str, player_client: Option<&str>) -> Vec<String> {
  let mut args = args_of(&[
    "--no-config",
    "-g",
    "--no-warnings",
    "--no-playlist",
    "--socket-timeout",
    "10",
    "--retries",
    "2",
  ]);
  if let Some(client) = player_client {
    args.push("--extractor-args".to_string());
    args.push(format!("youtube:player_client={client}"));
  }
  args.extend(args_of(&["-f", YT_DLP_AUDIO_FORMAT, "--", v_url]));
  args
}

fn yt_dlp_stream_attempts(v_url: &str) -> Vec<Vec<String>> {
  YT_DLP_PLAYER_CLIENTS
    .iter()
    .map(|client| yt_dlp_stream_args(v_url, *client))
    .collect()
}

fn needs_yt_auth(msg: &str) -> bool {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re = RE.get_or_init(|| {
    Regex::new(r"(?i)confirm your age|not a bot|Sign in to confirm|age-restricted|LOGIN_REQUIRED")
      .expect("regex")
  });
  re.is_match(msg)
}

fn args_of(list: &[&str]) -> Vec<String> {
  list.iter().map(|s| s.to_string()).collect()
}

// État partagé par les résolutions concurrentes d'une même requête.
struct Resolver<'a> {
  core: &'a Core,
  quality: &'a str,
  force_yt_dlp: bool,
  audio_format: &'a str,
  q: &'a str,
  search_q: &'a str,
  need_yt_auth: AtomicBool,
  failed_ids: Mutex<HashSet<String>>,
}

impl Resolver<'_> {
  fn sc_query(&self) -> &str {
    if self.q.is_empty() {
      self.search_q
    } else {
      self.q
    }
  }

  async fn yt_dlp_stream(&self, cand_id: &str) -> Option<String> {
    let vid = youtube::extract_yt_id_from_any(cand_id);
    if vid.is_empty() {
      return None;
    }
    let v_url = format!("https://www.youtube.com/watch?v={vid}");
    let started = std::time::Instant::now();
    for args in yt_dlp_stream_attempts(&v_url) {
      let left = YT_DLP_BUDGET_MS.saturating_sub(started.elapsed().as_millis() as u64);
      if left < YT_DLP_ATTEMPT_MIN_MS {
        break;
      }
      match ytdlp::execute_yt_dlp(self.core, &args, left.min(YT_DLP_ATTEMPT_MAX_MS), true).await {
        Ok(raw) => {
          if let Some(url) = non_hls_http_lines(&raw).into_iter().next()
            && youtube::accept_stream_url(self.core, &url, "yt-dlp").await
          {
            return Some(url);
          }
        }
        Err(msg) => {
          if needs_yt_auth(&msg) {
            self.need_yt_auth.store(true, Ordering::Relaxed);
          }
        }
      }
    }
    None
  }

  async fn yt_candidate_stream(&self, cand_id: &str) -> Option<String> {
    let vid = youtube::extract_yt_id_from_any(cand_id);
    if vid.is_empty() {
      return None;
    }
    if !self.force_yt_dlp
      && let Some(s) = youtube::fetch_direct_yt_stream(self.core, &vid, self.quality).await
    {
      return Some(s);
    }
    if !self.sc_query().is_empty()
      && let Some(s) = fetch_sc_stream_by_query(self.core, self.sc_query(), false).await
    {
      return Some(s);
    }
    self.yt_dlp_stream(cand_id).await
  }

  async fn sc_candidate_stream(&self, cand_url: &str, cand_id: &str) -> Option<String> {
    let src = soundcloud::sc_stream_source(cand_url, cand_id);
    if let Some(s) = soundcloud::fetch_direct_sc_stream(self.core, src, "").await {
      return Some(s);
    }
    if !ytdlp::is_allowed_target(&self.core.net.http_client, cand_url) {
      return None;
    }
    let args = args_of(&[
      "--no-config",
      "-g",
      "--no-warnings",
      "--no-playlist",
      "--socket-timeout",
      "8",
      "--retries",
      "1",
      "-f",
      self.audio_format,
      "--",
      cand_url,
    ]);
    let raw = ytdlp::execute_yt_dlp(self.core, &args, 10_000, false)
      .await
      .ok()?;
    first_http_lines(&raw).into_iter().find(|l| {
      let low = l.to_lowercase();
      !(low.contains("preview") || low.contains("drm") || low.contains("cf-preview-media"))
    })
  }

  // Recherche YouTube Music + YouTube, classe les candidats et résout les meilleurs en parallèle.
  async fn search_candidates_stream(
    &self,
    title: &str,
    artist: &str,
    duration: f64,
  ) -> Result<(String, Option<String>), ()> {
    let max_cand = if self.need_yt_auth.load(Ordering::Relaxed) {
      1
    } else {
      3
    };
    let (ytm, yt) = join(
      search::ytmusic_search(self.core, self.search_q, 3500, 0, false),
      direct_youtube_search(&self.core.net.http_client, self.search_q, false, 12),
    )
    .await;
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<Value> = Vec::new();
    for cand in ytm.into_iter().chain(yt) {
      let Some(id) = cand
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
      else {
        continue;
      };
      let failed = self
        .failed_ids
        .lock()
        .map(|f| f.contains(&id))
        .unwrap_or(false);
      if !failed && seen.insert(id) {
        candidates.push(cand);
      }
    }
    let target_title = if title.is_empty() {
      self.search_q
    } else {
      title
    };
    let mut ranked: Vec<(f64, Value)> = candidates
      .into_iter()
      .map(|c| {
        (
          score_imported_candidate(target_title, artist, duration, &c),
          c,
        )
      })
      .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(max_cand);
    if ranked.is_empty() {
      return Err(());
    }
    let ids: Vec<String> = ranked
      .iter()
      .map(|(_, c)| {
        c.get("id")
          .and_then(Value::as_str)
          .unwrap_or("")
          .to_string()
      })
      .collect();
    if let Ok(mut failed) = self.failed_ids.lock() {
      failed.extend(ids.iter().cloned());
    }
    let results = join_all(ids.iter().map(|id| async move {
      self
        .yt_candidate_stream(id)
        .await
        .map(|url| (url, Some(id.clone())))
    }))
    .await;
    results.into_iter().flatten().next().ok_or(())
  }
}

pub async fn stream_info(core: &Arc<Core>, params: Params) -> CoreResult {
  let query = &params.query;
  let get = |k: &str| query.get(k).map(String::as_str).unwrap_or("").to_string();
  let timeout_sec = get("timeout")
    .trim()
    .parse::<i64>()
    .ok()
    .map(|n| n.clamp(5, 30))
    .unwrap_or(20) as u64;
  let req = StreamReq {
    track_url: clean_url(&get("url")),
    track_id: get("id"),
    platform: if get("platform").is_empty() {
      "ytmusic".to_string()
    } else {
      get("platform")
    },
    title: get("title"),
    artist: get("artist"),
    duration: get("duration")
      .trim()
      .parse::<f64>()
      .ok()
      .filter(|d| d.is_finite())
      .unwrap_or(0.0),
    for_share: get("forShare") == "1",
    force_yt_dlp: get("forceYtDlp") == "1",
    fresh: get("fresh") == "1",
    quality: if get("quality").is_empty() {
      "medium".to_string()
    } else {
      get("quality")
    },
    query_q: get("q").trim().to_string(),
  };
  match tokio::time::timeout(
    Duration::from_secs(timeout_sec),
    resolve_stream_info(core, req),
  )
  .await
  {
    Ok(resp) => resp,
    // Le futur abandonné tue les processus yt-dlp en cours (kill_on_drop).
    Err(_) => json_response(
      504,
      &json!({ "success": false, "error": "Stream resolution timed out" }),
    ),
  }
}

async fn resolve_stream_info(core: &Core, req: StreamReq) -> CoreResult {
  let base = core.local_base_url();
  let StreamReq {
    mut track_url,
    track_id,
    platform,
    mut title,
    artist,
    duration,
    for_share,
    force_yt_dlp,
    fresh,
    quality,
    query_q,
  } = req;
  let artist = strip_youtube_topic(&artist);
  track_url = soundcloud::normalize_soundcloud_url(&track_url);

  let mut clean_id = clean_track_id(&track_id);
  if clean_id.is_empty()
    && let Some((_, after)) = track_url.split_once("v=")
  {
    clean_id = clean_track_id(after.split('&').next().unwrap_or(""));
  }
  let is_real_yt_id = !youtube::extract_yt_id_from_any(&clean_id).is_empty();
  static RE_SYNTH: OnceLock<Regex> = OnceLock::new();
  let re_synth = RE_SYNTH.get_or_init(|| Regex::new(r"(?i)cand_|dz_|it_|sp_|ym_").expect("regex"));
  if !is_real_yt_id && !track_url.is_empty() && re_synth.is_match(&track_url) {
    track_url.clear();
  }

  let is_sc = platform == "soundcloud" || track_url.contains("soundcloud.com");
  if is_sc && title.is_empty() && artist.is_empty() && !track_url.is_empty() {
    static RE_SLUG: OnceLock<Regex> = OnceLock::new();
    static RE_SEP: OnceLock<Regex> = OnceLock::new();
    let re_slug =
      RE_SLUG.get_or_init(|| Regex::new(r"soundcloud\.com/[^/]+/([^/?#]+)").expect("regex"));
    let re_sep = RE_SEP.get_or_init(|| Regex::new(r"[-_]+").expect("regex"));
    if let Some(c) = re_slug.captures(&track_url)
      && &c[1] != "sets"
    {
      title = re_sep.replace_all(&c[1], " ").trim().to_string();
    }
  }

  let mut cached_file = if clean_id.is_empty() {
    None
  } else {
    find_cached_file(&clean_id)
  };
  if cached_file.is_none()
    && (!title.is_empty() || !artist.is_empty())
    && find_cached_file_by_meta(core, &title, &artist).is_some()
  {
    let id = if clean_id.is_empty() {
      clean_track_id(&title)
    } else {
      clean_id.clone()
    };
    return json_response(
      200,
      &json!({ "success": true, "isLocal": true, "streamUrl": local_track_url(base, &id) }),
    );
  }
  let downloaded_name = if title.is_empty() && artist.is_empty() {
    None
  } else {
    find_downloaded_file(&title, &artist)
  };

  if !for_share && let Some(file) = cached_file.take() {
    let stem = file
      .file_stem()
      .and_then(|s| s.to_str())
      .unwrap_or("")
      .to_string();
    let id = if stem.is_empty() {
      clean_id.clone()
    } else {
      stem
    };
    return json_response(
      200,
      &json!({ "success": true, "isLocal": true, "streamUrl": local_track_url(base, &id) }),
    );
  }
  if !for_share && let Some(name) = downloaded_name {
    return json_response(
      200,
      &json!({
        "success": true,
        "isLocal": true,
        "isDownload": true,
        "streamUrl": format!("{base}/api/downloaded-track?name={}", encode_uri_component(&name))
      }),
    );
  }

  let clean_artist = artist.to_lowercase().trim().to_string();
  let clean_title = title.to_lowercase().trim().to_string();
  let meta_key = (!clean_artist.is_empty() && !clean_title.is_empty())
    .then(|| format!("{clean_artist}::{clean_title}_{quality}"));
  let key_src = [clean_id.as_str(), track_url.as_str(), query_q.as_str()]
    .into_iter()
    .find(|s| !s.is_empty())
    .map(str::to_string)
    .unwrap_or_else(|| format!("{artist}_{title}"));
  let cache_key = format!("{}_{quality}", key_src.to_lowercase());
  let id_key = (!clean_id.is_empty()).then(|| format!("{clean_id}_{quality}"));

  // Écart voulu avec Riff : `fresh` vide le cache et n'attend pas une résolution en cours (son URL peut être la morte), comme `forceYtDlp`, mais garde la résolution directe en premier.
  let uncached = force_yt_dlp || fresh;
  if uncached {
    stream_cache_delete(core, &cache_key);
    if let Some(k) = &id_key {
      stream_cache_delete(core, k);
    }
    if let Some(k) = &meta_key {
      stream_cache_delete(core, k);
    }
  } else if !for_share {
    let hit = [Some(&cache_key), id_key.as_ref(), meta_key.as_ref()]
      .into_iter()
      .flatten()
      .find_map(|k| stream_cache_get(core, k));
    if let Some((ts, data)) = hit
      && now_millis().saturating_sub(ts) < STREAM_CACHE_TTL_MS
      && !expires_soon(&data, now_millis())
    {
      return json_response(200, &data);
    }
  }

  if !uncached && let Some(Some(url)) = wait_inflight(core, &cache_key).await {
    return json_response(200, &stream_data(base, &url, None));
  }

  if platform == "youtube"
    && !track_url.starts_with("http")
    && !clean_id.is_empty()
    && is_real_yt_id
  {
    track_url = format!("https://www.youtube.com/watch?v={clean_id}");
  }

  let guard = register_inflight(core, &cache_key);
  let is_yt = platform == "youtube"
    || platform == "ytmusic"
    || track_url.contains("youtube.com")
    || track_url.contains("youtu.be");
  let audio_format = if for_share || is_sc {
    SC_AUDIO_FORMAT
  } else {
    match quality.as_str() {
      "low" => "wa[ext=m4a]/ba[ext=m4a]/18/b/best",
      "high" | "max" => "ba[ext=m4a]/18/b/best",
      _ => "ba[ext=m4a]/ba[ext=webm]/ba/18/b/best",
    }
  };
  let artist_prefix = if artist.is_empty() {
    String::new()
  } else {
    format!("{artist} - ")
  };
  let q = {
    let joined = format!("{artist_prefix}{title}").trim().to_string();
    if joined.is_empty() {
      query_q.clone()
    } else {
      joined
    }
  };
  let search_q = {
    let cleaned = make_clean_search_query(&artist, &title);
    if cleaned.is_empty() {
      q.clone()
    } else {
      cleaned
    }
  };
  let resolver = Resolver {
    core,
    quality: &quality,
    force_yt_dlp,
    audio_format,
    q: &q,
    search_q: &search_q,
    need_yt_auth: AtomicBool::new(false),
    failed_ids: Mutex::new(HashSet::new()),
  };

  // Réponse de succès : met en cache, libère les appels en attente.
  let respond = |url: &str, resolved_id: Option<&str>| -> CoreResult {
    guard.resolve(Some(url.to_string()));
    let final_id = resolved_id
      .map(str::to_string)
      .filter(|s| !s.is_empty())
      .unwrap_or_else(|| clean_id.clone());
    if for_share {
      let share_web_url = if final_id.is_empty() {
        track_url.clone()
      } else {
        format!("https://www.youtube.com/watch?v={final_id}")
      };
      return json_response(
        200,
        &json!({ "success": true, "streamUrl": url, "shareWebUrl": share_web_url }),
      );
    }
    let data = stream_data(base, url, resolved_id);
    stream_cache_set(core, &cache_key, &data);
    if !final_id.is_empty() && final_id != cache_key {
      stream_cache_set(core, &format!("{final_id}_{quality}"), &data);
    }
    if let Some(k) = &meta_key {
      stream_cache_set(core, k, &data);
    }
    json_response(200, &data)
  };

  let mut stream: Option<String> = None;
  if is_sc {
    stream = resolver.sc_candidate_stream(&track_url, &clean_id).await;
    if let Some(s) = &stream {
      return respond(s, None);
    }
  } else if is_real_yt_id && !clean_id.is_empty() {
    stream = resolver.yt_candidate_stream(&clean_id).await;
    if let Some(s) = &stream {
      return respond(s, Some(&clean_id));
    }
    if let Ok(mut f) = resolver.failed_ids.lock() {
      f.insert(clean_id.clone());
    }
  }

  if !search_q.is_empty() {
    let failed_any = resolver
      .failed_ids
      .lock()
      .map(|f| !f.is_empty())
      .unwrap_or(false);
    let sc_raced = failed_any || clean_id.is_empty() || !is_real_yt_id;
    let yt_fut = Box::pin(resolver.search_candidates_stream(&title, &artist, duration));
    let best = if sc_raced {
      let sc_fut = Box::pin(async {
        fetch_sc_stream_by_query(core, resolver.sc_query(), is_sc)
          .await
          .map(|u| (u, None))
          .ok_or(())
      });
      select_ok(vec![
        yt_fut
          as std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(String, Option<String>), ()>> + Send + '_>,
          >,
        sc_fut,
      ])
      .await
      .ok()
      .map(|(m, _)| m)
    } else {
      yt_fut.await.ok()
    };
    if let Some((url, id)) = best {
      return respond(&url, id.as_deref());
    }

    if !sc_raced
      && !resolver.sc_query().is_empty()
      && let Some(u) = fetch_sc_stream_by_query(core, resolver.sc_query(), is_sc).await
    {
      return respond(&u, None);
    }

    if !resolver.need_yt_auth.load(Ordering::Relaxed) {
      let target = format!("ytsearch1:{} audio", resolver.sc_query());
      let args = args_of(&[
        "--no-config",
        "-g",
        "--no-warnings",
        "--no-playlist",
        "--socket-timeout",
        "8",
        "--retries",
        "1",
        "--extractor-args",
        "youtube:player_client=default,android_vr",
        "-f",
        YT_DLP_AUDIO_FORMAT,
        &target,
      ]);
      if let Ok(raw) = ytdlp::execute_yt_dlp(core, &args, 10_000, false).await {
        for url in non_hls_http_lines(&raw) {
          if youtube::verify_stream_playable(core, &url).await {
            return respond(&url, None);
          }
        }
      }
    }
  }

  let mut share_web_url = String::new();
  if for_share && (!title.is_empty() || !artist.is_empty()) {
    let target = format!("ytsearch1:{artist_prefix}{title} audio");
    let args = args_of(&[
      "--no-config",
      "--no-warnings",
      "--no-playlist",
      "--extractor-args",
      YT_EXTRACTOR_ARGS,
      "--print",
      "%(webpage_url)s\n%(url)s",
      "-f",
      "bestaudio[ext=m4a]/bestaudio",
      &target,
    ]);
    if let Ok(raw) = ytdlp::execute_yt_dlp(core, &args, 10_000, false).await {
      let lines: Vec<&str> = raw
        .trim()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
      match lines.as_slice() {
        [web, direct, ..] => {
          share_web_url = web.to_string();
          stream = Some(direct.to_string());
        }
        [one] if one.starts_with("http") => {
          if one.contains("youtube.com") || one.contains("youtu.be") {
            share_web_url = one.to_string();
          } else {
            stream = Some(one.to_string());
          }
        }
        _ => {}
      }
    }
  }
  if for_share && share_web_url.is_empty() {
    if track_url.contains("youtube.com") || track_url.contains("youtu.be") {
      share_web_url = track_url.clone();
    } else if !clean_id.is_empty() && is_yt {
      share_web_url = format!("https://www.youtube.com/watch?v={clean_id}");
    }
  }
  guard.resolve(stream.clone());

  if for_share {
    return json_response(
      200,
      &json!({ "success": true, "streamUrl": stream.unwrap_or_default(), "shareWebUrl": share_web_url }),
    );
  }
  let need_auth = resolver.need_yt_auth.load(Ordering::Relaxed);
  logging::log_entry(
    "ERROR",
    &format!(
      "Stream resolve failed: {}",
      if title.is_empty() { &track_url } else { &title }
    ),
  );
  json_response(
    500,
    &json!({
      "success": false,
      "needAuth": need_auth,
      "error": if need_auth { "YouTube sign-in required for this track" } else { "could not extract audio stream" }
    }),
  )
}

pub async fn prefetch(core: &Arc<Core>, params: Params) -> CoreResult {
  let query = &params.query;
  let get = |k: &str| query.get(k).cloned().unwrap_or_default();
  let (url, id) = (clean_url(&get("url")), get("id"));
  let platform = if get("platform").is_empty() {
    "ytmusic".to_string()
  } else {
    get("platform")
  };
  spawn_prefetch(core, url, id, platform);
  json_response(200, &json!({ "success": true }))
}

pub fn spawn_prefetch(core: &Arc<Core>, url: String, id: String, platform: String) {
  let core = Arc::clone(core);
  tokio::spawn(async move {
    prefetch_track(&core, &url, &id, &platform).await;
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn drop_hls_discards_m3u8_and_keeps_progressive_streams() {
    assert_eq!(drop_hls(None), None);
    assert_eq!(
      drop_hls(Some(
        "https://cf-hls.sndcdn.com/x/playlist.m3u8?sig=1".into()
      )),
      None
    );
    assert_eq!(
      drop_hls(Some("https://cf-media.sndcdn.com/x.mp3".into())).as_deref(),
      Some("https://cf-media.sndcdn.com/x.mp3")
    );
  }

  #[test]
  fn yt_dlp_stream_args_use_the_m4a_selector() {
    let url = "https://www.youtube.com/watch?v=BV4hG61P8FU";
    let args = yt_dlp_stream_args(url, None);
    let at = args.iter().position(|a| a == "-f").expect("-f");
    assert_eq!(args[at + 1], YT_DLP_AUDIO_FORMAT);
    assert!(YT_DLP_AUDIO_FORMAT.starts_with("bestaudio[ext=m4a][protocol^=http]"));
    assert_ne!(args[at + 1], "bestaudio/best");
    assert_eq!(args[at + 2], "--");
    assert_eq!(args[at + 3], url);
    assert!(!args.iter().any(|a| a == "--extractor-args"));
  }

  #[test]
  fn yt_dlp_stream_attempts_try_the_measured_clients_before_the_default_arguments() {
    let url = "https://www.youtube.com/watch?v=BV4hG61P8FU";
    let attempts = yt_dlp_stream_attempts(url);
    let clients: Vec<Option<String>> = attempts
      .iter()
      .map(|args| {
        args
          .iter()
          .position(|a| a == "--extractor-args")
          .map(|at| args[at + 1].clone())
      })
      .collect();
    assert_eq!(
      clients,
      [
        Some("youtube:player_client=tv_simply".to_string()),
        Some("youtube:player_client=web_embedded".to_string()),
        None,
      ]
    );
    for args in &attempts {
      let at = args.iter().position(|a| a == "-f").expect("-f");
      assert_eq!(args[at + 1], YT_DLP_AUDIO_FORMAT);
      assert_eq!(&args[args.len() - 2..], ["--", url]);
    }
    assert_eq!(attempts[2], yt_dlp_stream_args(url, None));
  }

  #[test]
  fn expires_soon_reads_the_signed_url_expiry() {
    let data = |url: &str| json!({ "rawStreamUrl": url });
    let now = 1_000_000_000_000u64;
    let near = format!(
      "https://cdn.example/p.m3u8?a=1&expires={}&b=2",
      now / 1000 + 300
    );
    let far = format!("https://cdn.example/p.m3u8?expires={}", now / 1000 + 3600);
    assert!(expires_soon(&data(&near), now));
    assert!(!expires_soon(&data(&far), now));
    assert!(!expires_soon(&data("https://cdn.example/a.m4a"), now));
    assert!(!expires_soon(&json!({}), now));
  }

  #[test]
  fn non_hls_http_lines_rejects_m3u8() {
    let raw = "https://a.example/hls/playlist.m3u8?x=1\nnoise\nhttps://rr.example/audio.m4a\n";
    assert_eq!(
      non_hls_http_lines(raw),
      vec!["https://rr.example/audio.m4a".to_string()]
    );
    assert!(non_hls_http_lines("https://a.example/p.m3u8").is_empty());
  }

  #[test]
  fn clean_query_keeps_artist_names_with_ft() {
    assert_eq!(
      make_clean_search_query("Daft Punk - Topic", "Get Lucky (feat. Pharrell Williams)"),
      "Daft Punk Get Lucky"
    );
    assert_eq!(
      make_clean_search_query("Taylor Swift", "Style [Official Video]"),
      "Taylor Swift Style"
    );
    assert_eq!(
      make_clean_search_query("Drake ft. Rihanna", "Take Care"),
      "Drake Take Care"
    );
    assert_eq!(
      make_clean_search_query("Artist VEVO", "Song (HD)"),
      "Artist Song"
    );
  }

  #[test]
  fn candidate_score_prefers_exact_match_and_penalizes_remix() {
    let exact = json!({ "title": "Get Lucky", "artist": "Daft Punk", "duration": 369 });
    let remix =
      json!({ "title": "Get Lucky (Nightcore remix)", "artist": "Some Channel", "duration": 200 });
    let a = score_imported_candidate("Get Lucky", "Daft Punk", 369.0, &exact);
    let b = score_imported_candidate("Get Lucky", "Daft Punk", 369.0, &remix);
    assert!(a > 100.0 && b < a);
    assert_eq!(
      score_imported_candidate(
        "Get Lucky",
        "Daft Punk",
        369.0,
        &json!({ "title": "Unrelated words here", "artist": "x" })
      ),
      -500.0
    );
  }

  #[test]
  fn youtube_id_extraction() {
    assert_eq!(
      youtube::extract_yt_id_from_any("4D7u5KF7SP8"),
      "4D7u5KF7SP8"
    );
    assert_eq!(
      youtube::extract_yt_id_from_any("https://www.youtube.com/watch?v=4D7u5KF7SP8&t=3"),
      "4D7u5KF7SP8"
    );
    assert_eq!(
      youtube::extract_yt_id_from_any("https://youtu.be/4D7u5KF7SP8"),
      "4D7u5KF7SP8"
    );
    assert_eq!(youtube::extract_yt_id_from_any("dz_1234567890123"), "");
    assert_eq!(youtube::extract_yt_id_from_any("254111945"), "");
  }
}
