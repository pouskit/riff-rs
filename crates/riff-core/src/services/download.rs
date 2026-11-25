use super::playback::downloads_dir;
use super::soundcloud::normalize_soundcloud_url;
use super::stream::{YT_EXTRACTOR_ARGS, invalidate_downloads_index, sanitize_filename};
use super::ytdlp::execute_yt_dlp;
use super::*;
use crate::core::DownloadOutcome;
use crate::reply::Reply;
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::Command;
use tokio::sync::watch;

const VALID_FORMATS: [&str; 6] = ["mp3", "m4a", "flac", "opus", "wav", "ogg"];
const MAX_ENCODE_BYTES: u64 = 500 * 1024 * 1024;
const YT_DLP_DOWNLOAD_TIMEOUT_MS: u64 = 60_000;
const TRASH_EXTS: [&str; 13] = [
  ".webp", ".webm", ".part", ".ytdl", ".temp", ".jpg", ".jpeg", ".png", ".json", ".vtt", ".m4a",
  ".opus", ".mp4",
];

static SEQ: AtomicU64 = AtomicU64::new(0);

fn unique_suffix() -> String {
  format!("{}_{}", now_millis(), SEQ.fetch_add(1, Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// Garde-fous : fichiers temporaires supprimés même si le client coupe la connexion
// ---------------------------------------------------------------------------

struct TempFile(Option<PathBuf>);

impl TempFile {
  fn path(&self) -> Option<&Path> {
    self.0.as_deref()
  }

  fn keep(mut self) {
    self.0 = None;
  }
}

impl Drop for TempFile {
  fn drop(&mut self) {
    if let Some(p) = self.0.take() {
      let _ = fs::remove_file(p);
    }
  }
}

fn list_names(dir: &Path) -> HashSet<String> {
  fs::read_dir(dir)
    .map(|rd| {
      rd.flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
    })
    .unwrap_or_default()
}

fn ext_with_dot(name: &str) -> String {
  Path::new(name)
    .extension()
    .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
    .unwrap_or_default()
}

// Équivalent de cleanupDownloadArtifacts : `keep` conserve le fichier final et supprime les restes de yt-dlp.
fn cleanup_artifacts(
  dir: &Path,
  before: &HashSet<String>,
  base_name: &str,
  video_id: &str,
  ext: &str,
  keep: bool,
) {
  let chosen = format!(".{}", ext.to_lowercase());
  for f in list_names(dir) {
    if before.contains(&f) {
      continue;
    }
    if !(f.starts_with(base_name) || (!video_id.is_empty() && f.contains(video_id))) {
      continue;
    }
    if keep && f == format!("{base_name}{chosen}") {
      continue;
    }
    let file_ext = ext_with_dot(&f);
    if (TRASH_EXTS.contains(&file_ext.as_str()) || !keep) && !(keep && file_ext == chosen) {
      let _ = fs::remove_file(dir.join(&f));
    }
  }
}

fn cleanup_raw_partials(dir: &Path, before: &HashSet<String>, prefix: &str) {
  for f in list_names(dir) {
    if !before.contains(&f) && f.starts_with(prefix) {
      let _ = fs::remove_file(dir.join(&f));
    }
  }
}

// Supprime les restes d'un téléchargement interrompu ou raté, sauf si on l'a désarmé (succès).
struct DownloadCleanup {
  dir: PathBuf,
  before: HashSet<String>,
  base_name: String,
  video_id: String,
  ext: String,
  raw: bool,
  armed: bool,
}

impl Drop for DownloadCleanup {
  fn drop(&mut self) {
    if !self.armed {
      return;
    }
    if self.raw {
      cleanup_raw_partials(&self.dir, &self.before, &self.base_name);
    } else {
      cleanup_artifacts(
        &self.dir,
        &self.before,
        &self.base_name,
        &self.video_id,
        &self.ext,
        false,
      );
    }
  }
}

// Un seul téléchargement par clé : les appels identiques attendent le résultat du premier.
struct DownloadGuard<'a> {
  core: &'a Core,
  key: String,
  tx: watch::Sender<DownloadOutcome>,
  rx: watch::Receiver<DownloadOutcome>,
}

impl DownloadGuard<'_> {
  fn finish(&self, status: u16, body: &Value) {
    self.tx.send_replace(Some((status, body.clone())));
  }
}

impl Drop for DownloadGuard<'_> {
  fn drop(&mut self) {
    if self.tx.borrow().is_none() {
      self.tx.send_replace(Some((
        500,
        json!({ "success": false, "error": "Download ended without a result" }),
      )));
    }
    if let Ok(mut map) = self.core.playback.download_inflight.lock()
      && map.get(&self.key).is_some_and(|r| r.same_channel(&self.rx))
    {
      map.remove(&self.key);
    }
  }
}

enum Registration<'a> {
  Leader(DownloadGuard<'a>),
  Waiter(watch::Receiver<DownloadOutcome>),
}

fn register<'a>(core: &'a Core, key: &str) -> Registration<'a> {
  let Ok(mut map) = core.playback.download_inflight.lock() else {
    let (tx, rx) = watch::channel(None);
    return Registration::Leader(DownloadGuard {
      core,
      key: key.to_string(),
      tx,
      rx,
    });
  };
  if let Some(rx) = map.get(key) {
    return Registration::Waiter(rx.clone());
  }
  let (tx, rx) = watch::channel(None);
  map.insert(key.to_string(), rx.clone());
  Registration::Leader(DownloadGuard {
    core,
    key: key.to_string(),
    tx,
    rx,
  })
}

// ---------------------------------------------------------------------------
// Paramètres, dossiers, pochette
// ---------------------------------------------------------------------------

// parseFloat de JavaScript : lit le préfixe numérique.
fn parse_float_prefix(s: &str) -> Option<f64> {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re =
    RE.get_or_init(|| Regex::new(r"^[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?").expect("regex"));
  re.find(s.trim_start())?.as_str().parse::<f64>().ok()
}

fn parse_time(query: &HashMap<String, String>, key: &str) -> Option<f64> {
  let raw = query.get(key)?;
  if raw.is_empty() {
    return None;
  }
  parse_float_prefix(raw).map(|n| n.max(0.0))
}

fn bitrate_for(quality: &str) -> &'static str {
  match quality {
    "2" => "256k",
    "4" => "192k",
    "5" => "128k",
    _ => "320k",
  }
}

fn pick_format(format: &str) -> String {
  let f = format.to_lowercase();
  if VALID_FORMATS.contains(&f.as_str()) {
    f
  } else {
    "mp3".to_string()
  }
}

// (dossier de sortie, dossier .tmp). `customDir` remplace le dossier de téléchargements s'il existe ou peut être créé.
fn resolve_dirs(custom_dir: &str) -> (PathBuf, PathBuf) {
  let mut dir = downloads_dir();
  if !custom_dir.is_empty() {
    let custom = PathBuf::from(custom_dir);
    if !custom.exists() {
      let _ = fs::create_dir_all(&custom);
    }
    if custom.exists() {
      dir = custom;
    }
  }
  let tmp = dir.join(".tmp");
  if !tmp.exists() {
    let _ = fs::create_dir_all(&tmp);
  }
  (dir, tmp)
}

/// Adresses à essayer, de la meilleure taille à la moins bonne. Écart volontaire avec Riff : `hqdefault.jpg` fait 480 x 360.
fn cover_candidates(thumb: &str) -> Vec<String> {
  let upgraded = upgrade_artwork_url(thumb);
  let target = if upgraded.is_empty() {
    thumb.to_string()
  } else {
    upgraded
  };
  match target
    .strip_suffix("/hqdefault.jpg")
    .filter(|_| target.contains("ytimg.com/"))
  {
    Some(base) => vec![
      format!("{base}/maxresdefault.jpg"),
      format!("{base}/sddefault.jpg"),
      target.clone(),
    ],
    None => vec![target],
  }
}

async fn fetch_cover(core: &Core, url: &str, cover: &Path) -> bool {
  let Ok(resp) = core
    .net
    .image_client
    .get(url)
    .timeout(Duration::from_secs(10))
    .send()
    .await
  else {
    return false;
  };
  if !resp.status().is_success() {
    return false;
  }
  let Ok(bytes) = resp.bytes().await else {
    return false;
  };
  if bytes.len() < 32 || fs::write(cover, &bytes).is_err() {
    let _ = fs::remove_file(cover);
    return false;
  }
  true
}

async fn download_cover_temp(core: &Core, thumb: &str, tmp_dir: &Path) -> TempFile {
  let cover = tmp_dir.join(format!("cover_{}.jpg", unique_suffix()));
  for target in cover_candidates(thumb) {
    let lower = target.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
      if fetch_cover(core, &target, &cover).await {
        return TempFile(Some(cover));
      }
    } else if Path::new(&target).exists() && fs::copy(&target, &cover).is_ok() {
      return TempFile(Some(cover));
    }
  }
  TempFile(None)
}

// ---------------------------------------------------------------------------
// ffmpeg
// ---------------------------------------------------------------------------

struct FfmpegJob<'a> {
  input: &'a Path,
  cover: Option<&'a Path>,
  ext: &'a str,
  bitrate: &'a str,
  title: &'a str,
  artist: &'a str,
  start: Option<f64>,
  end: Option<f64>,
  /// L'audio est déjà au format demandé : pas de ré-encodage. Écart volontaire avec Riff.
  copy_audio: bool,
  out: &'a Path,
}

/// L'entrée est déjà au format `ext`.
fn same_format(input: &Path, ext: &str) -> bool {
  input
    .extension()
    .and_then(|found| found.to_str())
    .is_some_and(|found| found.eq_ignore_ascii_case(ext))
}

fn ffmpeg_args(j: &FfmpegJob) -> Vec<String> {
  let s = |v: &str| v.to_string();
  let mut a = vec![s("-y")];
  if let Some(start) = j.start {
    a.extend([s("-ss"), start.to_string()]);
  }
  if let Some(end) = j.end {
    a.extend([s("-to"), end.to_string()]);
  }
  a.extend([s("-i"), j.input.to_string_lossy().into_owned()]);
  if let Some(cover) = j.cover
    && matches!(j.ext, "mp3" | "m4a" | "flac")
  {
    a.extend([
      s("-i"),
      cover.to_string_lossy().into_owned(),
      s("-map"),
      s("0:a:0"),
      s("-map"),
      s("1:v:0"),
      s("-c:v"),
      s("mjpeg"),
    ]);
    if j.ext == "mp3" {
      a.extend([
        s("-id3v2_version"),
        s("3"),
        s("-metadata:s:v"),
        s("title=Album cover"),
        s("-metadata:s:v"),
        s("comment=Cover (front)"),
      ]);
    } else {
      a.extend([s("-disposition:v:0"), s("attached_pic")]);
    }
  }
  if j.copy_audio {
    a.extend([s("-c:a"), s("copy")]);
  } else if matches!(j.ext, "mp3" | "m4a" | "opus" | "ogg") {
    a.extend([s("-b:a"), s(j.bitrate)]);
  }
  a.extend([
    s("-metadata"),
    format!("title={}", j.title),
    s("-metadata"),
    format!("artist={}", j.artist),
    j.out.to_string_lossy().into_owned(),
  ]);
  a
}

/// Réintègre pochette et tags dans un fichier déjà au format demandé.
fn cover_embed_args(
  input: &Path,
  cover: &Path,
  ext: &str,
  bitrate: &str,
  title: &str,
  artist: &str,
  out: &Path,
) -> Vec<String> {
  ffmpeg_args(&FfmpegJob {
    input,
    cover: Some(cover),
    ext,
    bitrate,
    title,
    artist,
    start: None,
    end: None,
    copy_audio: true,
    out,
  })
}

// Le processus est tué si le futur est abandonné (client parti).
async fn run_ffmpeg(bin: &Path, args: &[String]) -> Result<(), String> {
  let mut cmd = Command::new(bin);
  cmd
    .args(args)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped())
    .kill_on_drop(true);
  #[cfg(windows)]
  cmd.creation_flags(0x0800_0000);
  let child = cmd.spawn().map_err(|e| e.to_string())?;
  let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
  if out.status.success() {
    return Ok(());
  }
  let stderr = String::from_utf8_lossy(&out.stderr);
  let lines: Vec<&str> = stderr.lines().collect();
  logging::log_entry(
    "WARN",
    &format!(
      "ffmpeg failed: {}",
      lines[lines.len().saturating_sub(5)..].join(" | ")
    ),
  );
  Err(format!(
    "ffmpeg exited with code {}",
    out.status.code().unwrap_or(-1)
  ))
}

fn success_body(raw: bool, temp_name: &str, filename: &str, dir: &Path) -> Value {
  if raw {
    json!({ "success": true, "tempName": temp_name })
  } else {
    json!({ "success": true, "filename": filename, "directory": dir.to_string_lossy() })
  }
}

// ---------------------------------------------------------------------------
// /api/download
// ---------------------------------------------------------------------------

struct DownloadReq {
  title: String,
  artist: String,
  ext: String,
  quality: String,
  start: Option<f64>,
  end: Option<f64>,
  raw: bool,
  clean_id: String,
  target_url: String,
  thumb: String,
  custom_dir: String,
}

fn yt_dlp_args(r: &DownloadReq, template: &str, target: &str) -> Vec<String> {
  let s = |v: &str| v.to_string();
  let mut a = vec![
    s("--no-playlist"),
    s("--no-warnings"),
    s("-x"),
    s("--audio-format"),
    r.ext.clone(),
    s("--audio-quality"),
    r.quality.clone(),
    s("--embed-metadata"),
    s("-o"),
    s(template),
    s("--extractor-args"),
    s(YT_EXTRACTOR_ARGS),
  ];
  if !r.raw && matches!(r.ext.as_str(), "mp3" | "m4a" | "flac" | "opus" | "ogg") {
    a.extend([s("--embed-thumbnail"), s("--convert-thumbnails"), s("jpg")]);
  }
  if r.start.is_some() || r.end.is_some() {
    let start = r.start.unwrap_or(0.0);
    let to = match r.end {
      Some(e) if e != 0.0 => format!(" -to {e}"),
      _ => String::new(),
    };
    // Écart avec Riff, qui donne `ffmpeg:` : la miniature y échoue et la passe des tags recoupe l'audio (5 s donnaient 2,97 s).
    a.extend([
      s("--postprocessor-args"),
      format!("ExtractAudio:-ss {start}{to}"),
    ]);
  }
  a.extend([s("--"), s(target)]);
  a
}

async fn perform_download(core: &Core, r: &DownloadReq) -> (u16, Value) {
  let (downloads, tmp_dir) = resolve_dirs(&r.custom_dir);
  let cover = if !r.thumb.is_empty() && !r.raw {
    download_cover_temp(core, &r.thumb, &tmp_dir).await
  } else {
    TempFile(None)
  };

  let clean_title = sanitize_filename(if r.title.is_empty() {
    "untitled"
  } else {
    &r.title
  });
  let clean_artist = sanitize_filename(if r.artist.is_empty() {
    "unknown artist"
  } else {
    &r.artist
  });
  let temp_name = format!("raw_{}.wav", unique_suffix());
  let out_filename = if r.raw {
    temp_name.clone()
  } else {
    format!("{clean_artist} - {clean_title}.{}", r.ext)
  };
  let out_path = if r.raw {
    tmp_dir.join(&temp_name)
  } else {
    downloads.join(&out_filename)
  };
  let base_name = if r.raw {
    temp_name.trim_end_matches(".wav").to_string()
  } else {
    format!("{clean_artist} - {clean_title}")
  };
  let template = if r.raw {
    tmp_dir.join(format!("{base_name}.%(ext)s"))
  } else {
    downloads.join(format!("{clean_artist} - {clean_title}.%(ext)s"))
  }
  .to_string_lossy()
  .into_owned();
  let ok_body = || success_body(r.raw, &temp_name, &out_filename, &downloads);
  let meta_title = if r.title.is_empty() {
    clean_title.as_str()
  } else {
    r.title.as_str()
  };
  let meta_artist = if r.artist.is_empty() {
    clean_artist.as_str()
  } else {
    r.artist.as_str()
  };
  let bitrate = bitrate_for(&r.quality);

  // Fichier déjà en cache : simple transcodage ffmpeg, sans réseau.
  if !r.clean_id.is_empty()
    && let Some(cached) = find_cached_file(&r.clean_id)
    && let Some(ffmpeg) = paths::resolve_binary("ffmpeg")
  {
    let out_guard = TempFile(Some(out_path.clone()));
    let args = ffmpeg_args(&FfmpegJob {
      input: &cached,
      cover: cover.path(),
      ext: &r.ext,
      bitrate,
      title: meta_title,
      artist: meta_artist,
      start: r.start,
      end: r.end,
      copy_audio: same_format(&cached, &r.ext),
      out: &out_path,
    });
    if run_ffmpeg(&ffmpeg, &args).await.is_ok() {
      out_guard.keep();
      invalidate_downloads_index();
      return (200, ok_body());
    }
  }

  let target_dir = if r.raw {
    tmp_dir.clone()
  } else {
    downloads.clone()
  };
  let mut cleanup = DownloadCleanup {
    before: list_names(&target_dir),
    dir: target_dir,
    base_name: base_name.clone(),
    video_id: r.clean_id.clone(),
    ext: r.ext.clone(),
    raw: r.raw,
    armed: true,
  };

  let primary = yt_dlp_args(r, &template, &r.target_url);
  let mut result = execute_yt_dlp(core, &primary, YT_DLP_DOWNLOAD_TIMEOUT_MS, false).await;
  let mut first_err = String::new();

  if let Err(err) = &result {
    first_err = err.clone();
    logging::log_entry(
      "ERROR",
      &format!(
        "Download primary failed: {}: {err}",
        if r.title.is_empty() {
          &r.target_url
        } else {
          &r.title
        }
      ),
    );
    static RE_RETRY: OnceLock<Regex> = OnceLock::new();
    let re_retry = RE_RETRY.get_or_init(|| {
      Regex::new(r"(?i)403|not a bot|sign in to confirm|drm protected").expect("regex")
    });
    static RE_SC: OnceLock<Regex> = OnceLock::new();
    let re_sc = RE_SC.get_or_init(|| Regex::new(r"(?i)soundcloud\.com|scsearch").expect("regex"));
    if re_retry.is_match(err) && (!r.title.is_empty() || !r.artist.is_empty()) {
      let q = if !r.artist.is_empty() && !r.title.is_empty() {
        format!("{} - {}", r.artist, r.title)
      } else if r.title.is_empty() {
        r.artist.clone()
      } else {
        r.title.clone()
      };
      let fallback = if re_sc.is_match(&r.target_url) {
        format!("ytsearch1:{q} audio")
      } else {
        format!("scsearch1:{q}")
      };
      result = execute_yt_dlp(
        core,
        &yt_dlp_args(r, &template, &fallback),
        YT_DLP_DOWNLOAD_TIMEOUT_MS,
        false,
      )
      .await;
      if let Err(e) = &result {
        logging::log_entry(
          "ERROR",
          &format!("Download fallback retry failed: {fallback}: {e}"),
        );
      }
    }
    if result.is_err() {
      result = execute_yt_dlp(core, &primary, YT_DLP_DOWNLOAD_TIMEOUT_MS, false).await;
    }
  }

  if let Err(e) = result {
    let msg = if e.is_empty() { first_err } else { e };
    logging::log_entry(
      "ERROR",
      &format!(
        "Download failure: {} ({msg})",
        if r.title.is_empty() {
          &r.target_url
        } else {
          &r.title
        }
      ),
    );
    return (500, json!({ "success": false, "error": msg }));
  }

  // Succès : on garde le fichier final, on retire les restes, puis on réintègre la pochette si besoin.
  cleanup.armed = false;
  if !r.raw {
    cleanup_artifacts(
      &cleanup.dir,
      &cleanup.before,
      &base_name,
      &r.clean_id,
      &r.ext,
      true,
    );
    invalidate_downloads_index();
    if let Some(cover_path) = cover.path()
      && matches!(r.ext.as_str(), "mp3" | "m4a" | "flac")
      && out_path.exists()
      && let Some(ffmpeg) = paths::resolve_binary("ffmpeg")
    {
      let embed_tmp = PathBuf::from(format!("{}.cover.{}", out_path.display(), r.ext));
      let embed_guard = TempFile(Some(embed_tmp.clone()));
      let args = cover_embed_args(
        &out_path,
        cover_path,
        &r.ext,
        bitrate,
        meta_title,
        meta_artist,
        &embed_tmp,
      );
      if run_ffmpeg(&ffmpeg, &args).await.is_ok() && fs::rename(&embed_tmp, &out_path).is_ok() {
        embed_guard.keep();
      }
    }
  }
  (200, ok_body())
}

// Une URL que le profil `Service` refuserait n'atteint jamais yt-dlp : le titre est cherché à la place.
fn download_target(client: &Http, track_url: &str, artist: &str, title: &str) -> String {
  if !ytdlp::is_allowed_target(client, track_url) || track_url.contains("deezer.com") {
    format!("ytsearch1:{artist} {title} official audio")
  } else {
    track_url.to_string()
  }
}

pub async fn download(core: &Core, params: Params) -> CoreResult {
  let query = &params.query;
  let get = |k: &str| query.get(k).cloned().unwrap_or_default();
  let track_url = normalize_soundcloud_url(&clean_url(&get("url")));
  let title = get("title");
  let artist = get("artist");
  if track_url.is_empty() && (title.is_empty() || artist.is_empty()) {
    return json_response(
      400,
      &json!({ "success": false, "error": "missing track information" }),
    );
  }

  let raw = matches!(get("raw").as_str(), "1" | "true");
  let ext = if raw {
    "wav".to_string()
  } else {
    pick_format(&get("format").if_empty_str("mp3"))
  };
  let quality = {
    let q = get("quality").if_empty_str("0");
    if q.chars().all(|c| c.is_ascii_digit()) {
      q
    } else {
      "0".to_string()
    }
  };
  let (start, end) = (parse_time(query, "start"), parse_time(query, "end"));

  let clean_title = sanitize_filename(if title.is_empty() { "untitled" } else { &title });
  let clean_artist = sanitize_filename(if artist.is_empty() {
    "unknown artist"
  } else {
    &artist
  });
  let target_url = download_target(
    &core.net.http_client,
    &track_url,
    &clean_artist,
    &clean_title,
  );

  let mut clean_id = clean_track_id(&get("id"));
  if clean_id.is_empty() && !target_url.is_empty() {
    static RE_VID: OnceLock<Regex> = OnceLock::new();
    let re = RE_VID.get_or_init(|| {
      Regex::new(r"(?:v=|/embed/|/watch/|youtu\.be/|/v/)([a-zA-Z0-9_-]{11})").expect("regex")
    });
    if let Some(c) = re.captures(&target_url) {
      clean_id = clean_track_id(&c[1]);
    }
  }

  let req = DownloadReq {
    title,
    artist,
    ext,
    quality,
    start,
    end,
    raw,
    clean_id,
    target_url,
    thumb: get("thumbnail"),
    custom_dir: get("customDir"),
  };

  let respond = |status: u16, body: &Value| json_response(status, body);

  if req.raw {
    let (status, body) = perform_download(core, &req).await;
    return respond(status, &body);
  }

  let key = json!([
    if req.clean_id.is_empty() {
      req.target_url.as_str()
    } else {
      req.clean_id.as_str()
    },
    clean_artist,
    clean_title,
    req.ext,
    req.quality,
    req.start,
    req.end
  ])
  .to_string();

  match register(core, &key) {
    Registration::Waiter(mut rx) => {
      let outcome = match rx.wait_for(|v| v.is_some()).await {
        Ok(g) => (*g).clone(),
        Err(_) => None,
      }
      .unwrap_or_else(|| {
        (
          500,
          json!({ "success": false, "error": "Download ended without a result" }),
        )
      });
      respond(outcome.0, &outcome.1)
    }
    Registration::Leader(guard) => {
      let (status, body) = perform_download(core, &req).await;
      guard.finish(status, &body);
      respond(status, &body)
    }
  }
}

trait IfEmptyStr {
  fn if_empty_str(self, fallback: &str) -> String;
}

impl IfEmptyStr for String {
  fn if_empty_str(self, fallback: &str) -> String {
    if self.is_empty() {
      fallback.to_string()
    } else {
      self
    }
  }
}

// ---------------------------------------------------------------------------
// /api/encode : reçoit un WAV (jusqu'à 500 Mo) en flux, sans le charger en mémoire
// ---------------------------------------------------------------------------

fn fail(status: u16, error: &str) -> CoreResult {
  json_response(status, &json!({ "success": false, "error": error }))
}

/// Le WAV arrive en flux pour ne pas le charger en mémoire : d'où ce paramètre à part plutôt que `Params`.
pub async fn encode(
  core: &Core,
  method: Method,
  query: HashMap<String, String>,
  mut stream: BoxStream<'static, Result<Bytes, String>>,
) -> CoreResult {
  if method != Method::Post {
    return text_response(404, "Not found");
  }
  let get = |k: &str| query.get(k).cloned().unwrap_or_default();
  let source_name = get("name");
  let (_, tmp_dir) = resolve_dirs(&get("customDir"));
  let render_path = tmp_dir.join(format!("render_{}.wav", unique_suffix()));
  let render_guard = TempFile(Some(render_path.clone()));

  let mut file = match File::create(&render_path) {
    Ok(f) => f,
    Err(e) => return fail(400, &e.to_string()),
  };
  let mut received: u64 = 0;
  while let Some(chunk) = stream.next().await {
    let chunk = match chunk {
      Ok(c) => c,
      Err(e) => return fail(400, &e),
    };
    received += chunk.len() as u64;
    if received > MAX_ENCODE_BYTES {
      return fail(413, "WAV exceeds 300MB limit");
    }
    if let Err(e) = file.write_all(&chunk) {
      return fail(400, &e.to_string());
    }
  }
  drop(file);

  // Le WAV source (rendu par /api/download?raw=1) est supprimé dans tous les cas une fois ffmpeg lancé.
  static RE_WAV: OnceLock<Regex> = OnceLock::new();
  let re_wav = RE_WAV.get_or_init(|| Regex::new(r"^[A-Za-z0-9_ .\-]+\.wav$").expect("regex"));
  let _source_guard = TempFile(
    re_wav
      .is_match(&source_name)
      .then(|| tmp_dir.join(&source_name)),
  );
  let request = EncodeFile {
    input: render_path.clone(),
    title: get("title"),
    artist: get("artist"),
    format: get("format"),
    quality: get("quality"),
    thumbnail: get("thumbnail"),
    custom_dir: get("customDir"),
    start: None,
    end: None,
  };
  let _ = &render_guard;
  match encode_file(core, &request).await {
    Ok(done) => json_response(
      200,
      &json!({ "success": true, "filename": done.filename, "directory": done.directory.to_string_lossy() }),
    ),
    Err(e) => fail(500, &e),
  }
}

/// Encodage d'un fichier déjà sur le disque.
#[derive(Debug, Clone, Default)]
pub struct EncodeFile {
  pub input: PathBuf,
  pub title: String,
  pub artist: String,
  /// Format demandé. Vide ou inconnu : `mp3`.
  pub format: String,
  /// Qualité de Riff : `0`, `2`, `4` ou `5`. Vide : `0`.
  pub quality: String,
  /// Pochette : adresse ou chemin local. Vide : aucune.
  pub thumbnail: String,
  /// Dossier de sortie. Vide : dossier de téléchargement.
  pub custom_dir: String,
  /// Découpe, en secondes.
  pub start: Option<f64>,
  pub end: Option<f64>,
}

/// Fichier produit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
  pub filename: String,
  pub directory: PathBuf,
}

/// Encode `input` avec ffmpeg, tags et pochette. `input` reste en place : il appartient à l'appelant.
///
/// # Errors
/// ffmpeg introuvable ou en échec.
pub async fn encode_file(core: &Core, request: &EncodeFile) -> Result<Encoded, String> {
  let title = request.title.clone().if_empty_str("untitled");
  let artist = request.artist.clone().if_empty_str("unknown artist");
  let ext = pick_format(&request.format.clone().if_empty_str("mp3"));
  let bitrate = bitrate_for(&request.quality.clone().if_empty_str("0"));
  let (downloads, tmp_dir) = resolve_dirs(&request.custom_dir);
  let cover = if request.thumbnail.is_empty() {
    TempFile(None)
  } else {
    download_cover_temp(core, &request.thumbnail, &tmp_dir).await
  };
  let filename = format!(
    "{} - {}.{ext}",
    sanitize_filename(&artist),
    sanitize_filename(&title)
  );
  let out_path = downloads.join(&filename);
  let Some(ffmpeg) = paths::resolve_binary("ffmpeg") else {
    return Err("ffmpeg not found".to_string());
  };
  let out_guard = TempFile(Some(out_path.clone()));
  let trimmed = request.start.is_some() || request.end.is_some();
  let args = ffmpeg_args(&FfmpegJob {
    input: &request.input,
    cover: cover.path(),
    ext: &ext,
    bitrate,
    title: &title,
    artist: &artist,
    start: request.start,
    end: request.end,
    // Un format sans perte se découpe à l'échantillon près en le réécrivant, sans rien perdre.
    copy_audio: same_format(&request.input, &ext)
      && !(trimmed && matches!(ext.as_str(), "wav" | "flac")),
    out: &out_path,
  });
  match run_ffmpeg(&ffmpeg, &args).await {
    Ok(()) => {
      out_guard.keep();
      invalidate_downloads_index();
      Ok(Encoded {
        filename,
        directory: downloads,
      })
    }
    Err(e) => {
      logging::log_entry("ERROR", &format!("Encode failure: {title} ({e})"));
      Err(e)
    }
  }
}

pub fn is_valid_tmp_wav_name(name: &str) -> bool {
  if name.contains('/') || name.contains('\\') || name.contains("..") {
    return false;
  }
  let Some(stem) = name.strip_suffix(".wav") else {
    return false;
  };
  if stem.is_empty() {
    return false;
  }
  stem
    .chars()
    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ' || c == '.' || c == '-')
}

pub async fn tmp_file(params: Params) -> CoreResult {
  let query = &params.query;
  let file_name = query.get("name").map(String::as_str).unwrap_or("");
  if !is_valid_tmp_wav_name(file_name) {
    return json_response(
      400,
      &json!({ "success": false, "error": "Invalid file name" }),
    );
  }
  let downloads_dir = config::resolve_download_dir().unwrap_or_else(paths::default_download_dir);
  let _ = fs::create_dir_all(&downloads_dir);
  let file_path = downloads_dir.join(".tmp").join(file_name);
  let Ok(meta) = fs::metadata(&file_path) else {
    return json_response(404, &json!({ "success": false, "error": "File not found" }));
  };
  if !meta.is_file() {
    return json_response(404, &json!({ "success": false, "error": "File not found" }));
  }
  let Ok(bytes) = fs::read(&file_path) else {
    return json_response(404, &json!({ "success": false, "error": "File not found" }));
  };
  Ok(Reply::Bytes {
    status: 200,
    content_type: "audio/wav".to_string(),
    body: Bytes::from(bytes),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_float_like_js() {
    assert_eq!(parse_float_prefix("12.5abc"), Some(12.5));
    assert_eq!(parse_float_prefix("  7"), Some(7.0));
    assert_eq!(parse_float_prefix(".5"), Some(0.5));
    assert_eq!(parse_float_prefix("abc"), None);
  }

  #[test]
  fn mp3_cover_args_keep_original_order() {
    let out = PathBuf::from("o.mp3");
    let input = PathBuf::from("i.opus");
    let cover = PathBuf::from("c.jpg");
    let a = ffmpeg_args(&FfmpegJob {
      input: &input,
      cover: Some(&cover),
      ext: "mp3",
      bitrate: "320k",
      title: "T",
      artist: "A",
      start: Some(1.5),
      end: Some(9.0),
      copy_audio: false,
      out: &out,
    });
    assert_eq!(
      a.join(" "),
      "-y -ss 1.5 -to 9 -i i.opus -i c.jpg -map 0:a:0 -map 1:v:0 -c:v mjpeg -id3v2_version 3 -metadata:s:v title=Album cover -metadata:s:v comment=Cover (front) -b:a 320k -metadata title=T -metadata artist=A o.mp3"
    );
  }

  #[test]
  fn yt_dlp_args_trim_and_cover() {
    let r = DownloadReq {
      title: "T".into(),
      artist: "A".into(),
      ext: "mp3".into(),
      quality: "0".into(),
      start: Some(5.0),
      end: Some(0.0),
      raw: false,
      clean_id: String::new(),
      target_url: "https://x".into(),
      thumb: String::new(),
      custom_dir: String::new(),
    };
    let a = yt_dlp_args(&r, "tpl", "https://x");
    assert_eq!(a[0], "--no-playlist");
    assert!(
      a.windows(2)
        .any(|w| w == ["--postprocessor-args", "ExtractAudio:-ss 5"])
    );
    assert!(a.contains(&"--embed-thumbnail".to_string()));
    assert_eq!(a[a.len() - 2..], ["--", "https://x"]);
  }
}

#[cfg(test)]
#[path = "download_fixture_tests.rs"]
mod fixture_tests;

#[cfg(test)]
mod target_tests {
  use super::*;

  #[test]
  fn an_unlisted_url_falls_back_to_a_search() {
    let client = Http::service();
    let target = |url: &str| download_target(&client, url, "Artist", "Song");
    assert_eq!(
      target("https://www.youtube.com/watch?v=abc"),
      "https://www.youtube.com/watch?v=abc"
    );
    let search = "ytsearch1:Artist Song official audio";
    for url in [
      "http://192.168.1.1/x",
      "https://evil.example/x",
      "--exec=calc",
      "",
      "https://www.deezer.com/track/1",
    ] {
      assert_eq!(target(url), search, "{url}");
    }
  }
}
