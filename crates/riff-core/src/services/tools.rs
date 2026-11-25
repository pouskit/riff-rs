use super::ytdlp::execute_yt_dlp;
use super::*;
use sha2::Sha256;
use std::io::Write;
use tokio::net::TcpStream;
use tokio::process::Command;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Riff/43.1";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const UPDATE_STAMP: &str = ".ytdlp-update-stamp";

#[derive(Clone)]
pub struct ToolsState {
  status: String,
  tool: String,
  percent: u64,
  error: String,
  ffmpeg_error: String,
}

impl Default for ToolsState {
  fn default() -> Self {
    Self::new()
  }
}

impl ToolsState {
  pub fn new() -> Self {
    Self {
      status: "idle".into(),
      tool: String::new(),
      percent: 0,
      error: String::new(),
      ffmpeg_error: String::new(),
    }
  }

  fn progress(&self) -> Value {
    json!({
      "status": self.status,
      "tool": self.tool,
      "percent": self.percent,
      "error": self.error,
      "ffmpegError": self.ffmpeg_error
    })
  }
}

fn set_state(core: &Core, f: impl FnOnce(&mut ToolsState)) {
  if let Ok(mut g) = core.tools.lock() {
    f(&mut g);
  }
}

fn bin_dir() -> PathBuf {
  paths::resolve_data_dir().join("bin")
}

fn exe_name(name: &str) -> String {
  if cfg!(windows) {
    format!("{name}.exe")
  } else {
    name.to_string()
  }
}

pub async fn tools_status(core: &Core) -> CoreResult {
  let st = core
    .tools
    .lock()
    .map(|g| g.clone())
    .unwrap_or_else(|_| ToolsState::new());
  let ytdlp = paths::resolve_binary("yt-dlp").is_some();
  let ffmpeg = paths::resolve_binary("ffmpeg").is_some();
  let step = if st.tool.is_empty() {
    String::new()
  } else {
    format!("downloading {}", st.tool)
  };
  json_response(
    200,
    &json!({
      "ytDlp": ytdlp,
      "ytdlp": ytdlp,
      "ffmpeg": ffmpeg,
      "percent": st.percent,
      "step": step,
      "error": st.error,
      "progress": st.progress(),
      "ffmpegOptional": cfg!(target_os = "macos"),
      "ffmpegError": st.ffmpeg_error
    }),
  )
}

pub async fn tools_install(core: &Arc<Core>) -> CoreResult {
  tokio::spawn(ensure_tools(core.clone()));
  json_response(200, &json!({ "started": true }))
}

pub async fn proxy_test(core: &Core, method: Method, params: Params) -> CoreResult {
  let body = &params.body;
  if method != Method::Post {
    return text_response(404, "Not found");
  }
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let payload = serde_json::from_slice::<Value>(body).unwrap_or_else(|_| json!({}));
  let proxy_url = value_to_string(payload.get("proxyUrl")).trim().to_string();
  let t0 = now_millis();
  let args: Vec<String> = [
    "--no-config",
    "--proxy",
    proxy_url.as_str(),
    "--simulate",
    "--no-warnings",
    "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
  ]
  .iter()
  .map(|s| s.to_string())
  .collect();
  match execute_yt_dlp(core, &args, 8000, true).await {
    Ok(_) => json_response(
      200,
      &json!({ "success": true, "pingMs": now_millis().saturating_sub(t0) }),
    ),
    Err(e) => json_response(200, &json!({ "success": false, "error": e })),
  }
}

// ---------------------------------------------------------------------------
// Installation de yt-dlp et ffmpeg dans <données>\bin
// ---------------------------------------------------------------------------

fn yt_dlp_asset() -> &'static str {
  let arch = std::env::consts::ARCH;
  if cfg!(windows) {
    match arch {
      "aarch64" => "yt-dlp_arm64.exe",
      "x86" => "yt-dlp_x86.exe",
      _ => "yt-dlp.exe",
    }
  } else if cfg!(target_os = "macos") {
    "yt-dlp_macos"
  } else if arch == "aarch64" {
    "yt-dlp_linux_aarch64"
  } else {
    "yt-dlp_linux"
  }
}

fn ffmpeg_asset() -> Option<String> {
  let a = if std::env::consts::ARCH == "aarch64" {
    "arm64"
  } else {
    "64"
  };
  if cfg!(windows) {
    Some(format!("ffmpeg-master-latest-win{a}-gpl.zip"))
  } else if cfg!(target_os = "linux") {
    Some(format!("ffmpeg-master-latest-linux{a}-gpl.tar.xz"))
  } else {
    None
  }
}

fn expected_hash(sums: &str, file_name: &str) -> Option<String> {
  static RE: OnceLock<Regex> = OnceLock::new();
  let re =
    RE.get_or_init(|| Regex::new(r"(?i)^([a-f0-9]{64})\s+\*?(?:\./)?(.+)$").expect("regex sha"));
  sums.lines().find_map(|line| {
    let c = re.captures(line.trim())?;
    (c[2].trim() == file_name).then(|| c[1].to_lowercase())
  })
}

fn download_client() -> Http {
  Http::tools(UA)
}

fn make_command(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Command {
  let mut c = Command::new(program);
  c.args(args)
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true);
  #[cfg(windows)]
  c.creation_flags(CREATE_NO_WINDOW);
  c
}

async fn run_command(mut cmd: Command, label: &str, timeout_ms: u64) -> Result<(), String> {
  cmd
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped());
  let child = cmd.spawn().map_err(|e| {
    logging::log_entry("ERROR", &format!("{label} spawn error: {e}"));
    e.to_string()
  })?;
  let out = tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output())
    .await
    .map_err(|_| format!("{label} timed out"))?
    .map_err(|e| e.to_string())?;
  if out.status.success() {
    return Ok(());
  }
  let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
  let tail: Vec<&str> = stderr.lines().collect();
  let tail = tail[tail.len().saturating_sub(20)..].join("\n");
  logging::log_entry(
    "ERROR",
    &format!("{label} exited with {:?}: {tail}", out.status.code()),
  );
  if stderr.is_empty() {
    Err(format!(
      "{label} exited with {}",
      out.status.code().unwrap_or(-1)
    ))
  } else {
    Err(stderr)
  }
}

// Le 2026-11-02 : 39 Mo pour le plus gros yt-dlp, 192 Mo pour la plus grosse archive ffmpeg.
const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CHECKSUMS_BYTES: u64 = 1024 * 1024;
const YT_DLP_CHECK_INTERVAL_MS: u64 = 24 * 3600 * 1000;

fn yt_dlp_check_due(enabled: bool, last_check: Option<u64>, now: u64) -> bool {
  enabled && last_check.is_none_or(|last| last > now || now - last >= YT_DLP_CHECK_INTERVAL_MS)
}

// `:` : lecteur Windows (`C:`) ou flux NTFS caché (`a.exe:b`). Les deux séparateurs sont testés quel que soit l'OS.
fn is_safe_archive_path(name: &str) -> bool {
  !name.starts_with(['/', '\\'])
    && !name.contains([':', '\0'])
    && !name.split(['/', '\\']).any(|part| part == "..")
}

async fn command_stdout(mut cmd: Command, label: &str, timeout_ms: u64) -> Result<String, String> {
  cmd
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null());
  let child = cmd.spawn().map_err(|e| e.to_string())?;
  let out = tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait_with_output())
    .await
    .map_err(|_| format!("{label} timed out"))?
    .map_err(|e| e.to_string())?;
  if !out.status.success() {
    return Err(format!(
      "{label} exited with {}",
      out.status.code().unwrap_or(-1)
    ));
  }
  Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn get_text(client: &Http, url: &str) -> Result<String, String> {
  let resp = client
    .get(url)
    .timeout(Duration::from_secs(25))
    .max_bytes(MAX_CHECKSUMS_BYTES)
    .send()
    .await
    .map_err(|e| e.to_string())?;
  if !resp.status().is_success() {
    return Err(format!("HTTP {} for {url}", resp.status().as_u16()));
  }
  resp.text().await.map_err(|e| e.to_string())
}

fn hash_file(path: &Path) -> Result<String, String> {
  hash_reader(File::open(path).map_err(|e| e.to_string())?)
}

fn hash_reader(mut reader: impl std::io::Read) -> Result<String, String> {
  let mut hasher = Sha256::new();
  let mut buf = [0u8; 64 * 1024];
  loop {
    let n = match reader.read(&mut buf) {
      Ok(0) => break,
      Ok(n) => n,
      Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
      Err(e) => return Err(e.to_string()),
    };
    hasher.update(&buf[..n]);
  }
  let digest = hasher.finalize();
  Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

enum DownloadFailure {
  /// Inutile de réessayer : origine refusée ou fichier trop gros.
  Fatal(String),
  Retry(String),
}

fn network_failure(err: &net::Error) -> DownloadFailure {
  if err.is_blocked() || matches!(err, net::Error::TooLarge { .. }) {
    DownloadFailure::Fatal(err.to_string())
  } else {
    DownloadFailure::Retry(err.to_string())
  }
}

async fn try_download(
  core: &Core,
  client: &Http,
  url: &str,
  dest: &Path,
  existing: u64,
  max_bytes: u64,
) -> Result<(), DownloadFailure> {
  let retry = |e: std::io::Error| DownloadFailure::Retry(e.to_string());
  let mut req = client.get(url).streaming().max_bytes(max_bytes);
  if existing > 0 {
    req = req.header(header::RANGE, format!("bytes={existing}-"));
  }
  let mut resp = req.send().await.map_err(|e| network_failure(&e))?;
  let status = resp.status().as_u16();
  if status != 200 && status != 206 {
    return Err(DownloadFailure::Retry(format!("HTTP {status} for {url}")));
  }
  let partial = status == 206;
  let mut done = if partial { existing } else { 0 };
  let total = resp.content_length().filter(|n| *n > 0).map(|n| n + done);
  if total.is_some_and(|t| t > max_bytes) {
    return Err(DownloadFailure::Fatal(format!(
      "download larger than {max_bytes} bytes"
    )));
  }
  let mut opts = fs::OpenOptions::new();
  opts.create(true);
  if partial {
    opts.append(true);
  } else {
    opts.write(true).truncate(true);
  }
  let mut file = opts.open(dest).map_err(retry)?;
  while let Some(chunk) = resp.chunk().await.map_err(|e| network_failure(&e))? {
    file.write_all(&chunk).map_err(retry)?;
    done += chunk.len() as u64;
    if done > max_bytes {
      return Err(DownloadFailure::Fatal(format!(
        "download larger than {max_bytes} bytes"
      )));
    }
    if let Some(t) = total {
      set_state(core, |s| s.percent = (done * 100 / t).min(99));
    }
  }
  set_state(core, |s| s.percent = 100);
  Ok(())
}

// Reprise par Range (5 essais). Renvoie le SHA-256 du fichier.
async fn download_file(
  core: &Core,
  client: &Http,
  url: &str,
  dest: &Path,
  max_bytes: u64,
) -> Result<String, String> {
  let _ = fs::remove_file(dest);
  let mut last_err = "Download failed".to_string();
  for attempt in 0..5u64 {
    let existing = fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    match try_download(core, client, url, dest, existing, max_bytes).await {
      Ok(()) => return hash_file(dest),
      Err(DownloadFailure::Fatal(e)) => {
        let _ = fs::remove_file(dest);
        return Err(e);
      }
      Err(DownloadFailure::Retry(e)) => {
        last_err = e;
        tokio::time::sleep(Duration::from_millis(1200 * (attempt + 1))).await;
      }
    }
  }
  Err(last_err)
}

#[cfg(unix)]
fn make_executable(path: &Path) {
  use std::os::unix::fs::PermissionsExt;
  let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

async fn install_yt_dlp(core: &Core, client: &Http) -> Result<(), String> {
  let asset = yt_dlp_asset();
  let base = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
  let sums = get_text(client, &format!("{base}/SHA2-256SUMS")).await?;
  let want =
    expected_hash(&sums, asset).ok_or_else(|| format!("No checksum published for {asset}"))?;
  let target = bin_dir().join(exe_name("yt-dlp"));
  let tmp = PathBuf::from(format!("{}.download", target.display()));
  let got = download_file(
    core,
    client,
    &format!("{base}/{asset}"),
    &tmp,
    MAX_BINARY_BYTES,
  )
  .await?;
  if got != want {
    let _ = fs::remove_file(&tmp);
    return Err("yt-dlp checksum mismatch".into());
  }
  make_executable(&tmp);
  fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

fn find_file(dir: &Path, file_name: &str) -> Option<PathBuf> {
  for entry in fs::read_dir(dir).ok()?.flatten() {
    let path = entry.path();
    if entry.file_type().is_ok_and(|t| t.is_dir()) {
      if let Some(hit) = find_file(&path, file_name) {
        return Some(hit);
      }
    } else if entry.file_name().to_string_lossy() == file_name
      && entry.file_type().is_ok_and(|t| t.is_file())
    {
      // Un lien symbolique de l'archive pourrait viser un fichier hors du dossier d'extraction.
      return Some(path);
    }
  }
  None
}

// bsdtar de Windows (System32) lit aussi les zip et va bien plus vite qu'Expand-Archive.
// Lancé depuis le dossier de l'archive avec un nom relatif (un "C:" absolu est pris pour un hôte par tar GNU).
fn windows_tar_command(archive: &Path, args: &[&str]) -> Command {
  let sys_tar = Path::new(r"C:\Windows\System32\tar.exe");
  let tar = if sys_tar.exists() {
    sys_tar
  } else {
    Path::new("tar.exe")
  };
  let file_name = archive
    .file_name()
    .map(|n| n.to_string_lossy().into_owned())
    .unwrap_or_default();
  let mut full_args = vec!["-f", file_name.as_str()];
  full_args.extend_from_slice(args);
  let mut cmd = make_command(tar, &full_args);
  if let Some(parent) = archive.parent() {
    cmd.current_dir(parent);
  }
  cmd
}

async fn list_archive(archive: &Path) -> Result<String, String> {
  let archive_s = archive.to_string_lossy().into_owned();
  if !cfg!(windows) {
    return command_stdout(make_command("tar", &["-tf", &archive_s]), "tar", 60_000).await;
  }
  if let Ok(listing) =
    command_stdout(windows_tar_command(archive, &["-t"]), "tar.exe", 60_000).await
  {
    return Ok(listing);
  }
  let script = format!(
    "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Add-Type -AssemblyName System.IO.Compression.FileSystem; $z=[IO.Compression.ZipFile]::OpenRead('{}'); $z.Entries.FullName; $z.Dispose()",
    archive_s.replace('\'', "''")
  );
  command_stdout(
    make_command(
      "powershell.exe",
      &["-NoProfile", "-NonInteractive", "-Command", &script],
    ),
    "powershell.exe",
    60_000,
  )
  .await
}

// Rien n'est extrait si une seule entrée sort du dossier cible (zip slip).
async fn extract_archive(archive: &Path, work: &Path) -> Result<(), String> {
  let listing = list_archive(archive)
    .await
    .map_err(|e| format!("Could not read archive: {e}"))?;
  if let Some(bad) = listing.lines().find(|name| !is_safe_archive_path(name)) {
    return Err(format!("Archive refused, unsafe path: {bad}"));
  }
  let archive_s = archive.to_string_lossy().into_owned();
  let work_s = work.to_string_lossy().into_owned();
  if !cfg!(windows) {
    return run_command(
      make_command("tar", &["-xf", &archive_s, "-C", &work_s]),
      "tar",
      600_000,
    )
    .await;
  }
  if let Err(first) = run_command(
    windows_tar_command(archive, &["-x", "-C", &work_s]),
    "tar.exe",
    600_000,
  )
  .await
  {
    let script = format!(
      "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
      archive_s.replace('\'', "''"),
      work_s.replace('\'', "''")
    );
    run_command(
      make_command(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
      ),
      "powershell.exe",
      600_000,
    )
    .await
    .map_err(|e| format!("Failed to extract archive: {first}; {e}"))?;
  }
  Ok(())
}

async fn install_ffmpeg(core: &Core, client: &Http) -> Result<(), String> {
  let Some(asset) = ffmpeg_asset() else {
    return Err("No ffmpeg build for this system. Install ffmpeg with your package manager (for macOS: brew install ffmpeg).".into());
  };
  let base = "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download";
  let sums = get_text(client, &format!("{base}/checksums.sha256")).await?;
  let want =
    expected_hash(&sums, &asset).ok_or_else(|| format!("No checksum published for {asset}"))?;
  let archive = std::env::temp_dir().join(format!("riff-{asset}"));
  let got = download_file(
    core,
    client,
    &format!("{base}/{asset}"),
    &archive,
    MAX_ARCHIVE_BYTES,
  )
  .await?;
  if got != want {
    let _ = fs::remove_file(&archive);
    return Err("ffmpeg checksum mismatch".into());
  }
  let work = std::env::temp_dir().join(format!(
    "riff-ffmpeg-{}-{}",
    std::process::id(),
    now_millis()
  ));
  let result = async {
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    extract_archive(&archive, &work).await?;
    for name in ["ffmpeg", "ffprobe"] {
      let exe = exe_name(name);
      let src =
        find_file(&work, &exe).ok_or_else(|| format!("{exe} not found inside the archive"))?;
      let dest = bin_dir().join(&exe);
      fs::copy(&src, &dest).map_err(|e| e.to_string())?;
      make_executable(&dest);
    }
    Ok::<(), String>(())
  }
  .await;
  let _ = fs::remove_dir_all(&work);
  let _ = fs::remove_file(&archive);
  result
}

pub async fn ensure_tools(core: Arc<Core>) {
  let already = core
    .tools
    .lock()
    .map(|mut g| {
      if g.status == "downloading" {
        return true;
      }
      g.status = "downloading".into();
      g.error.clear();
      false
    })
    .unwrap_or(true);
  if already {
    return;
  }
  let _ = fs::create_dir_all(bin_dir());
  let client = download_client();
  let result: Result<(), String> = async {
    if paths::resolve_binary("yt-dlp").is_none() {
      set_state(&core, |s| {
        s.tool = "yt-dlp".into();
        s.percent = 0;
      });
      install_yt_dlp(&core, &client).await?;
    }
    if paths::resolve_binary("ffmpeg").is_none() {
      set_state(&core, |s| {
        s.tool = "ffmpeg".into();
        s.percent = 0;
      });
      match install_ffmpeg(&core, &client).await {
        Ok(()) => set_state(&core, |s| s.ffmpeg_error.clear()),
        Err(e) if cfg!(target_os = "macos") => {
          logging::log_entry("ERROR", &format!("ffmpeg install failed: {e}"));
          set_state(&core, |s| s.ffmpeg_error = e);
        }
        Err(e) => return Err(e),
      }
    }
    Ok(())
  }
  .await;
  match result {
    Ok(()) => set_state(&core, |s| s.status = "done".into()),
    Err(e) => {
      let tool = core
        .tools
        .lock()
        .map(|g| g.tool.clone())
        .unwrap_or_default();
      logging::log_entry("ERROR", &format!("Tool download failed: {tool}: {e}"));
      set_state(&core, |s| {
        s.status = "error".into();
        s.error = e;
      });
    }
  }
}

/// Vérifie la présence d'une mise à jour de yt-dlp installé dans `<données>/bin` : seule requête automatique de Riff.
/// Au plus une fois par 24 h, même en cas d'échec, désactivable (`ytdlpAutoUpdate` dans `config.json`).
/// À appeler après le premier écran, jamais au démarrage du core.
pub async fn update_yt_dlp_if_managed(core: Arc<Core>) {
  let Some(bin) = paths::resolve_binary("yt-dlp") else {
    return;
  };
  let dir = bin_dir();
  if bin.parent() != Some(dir.as_path()) {
    return;
  }
  let stamp = dir.join(UPDATE_STAMP);
  let last_check = fs::read_to_string(&stamp)
    .ok()
    .and_then(|text| text.trim().parse::<u64>().ok());
  if !yt_dlp_check_due(config::yt_dlp_auto_update(), last_check, now_millis()) {
    return;
  }
  let _ = fs::write(&stamp, now_millis().to_string());
  net::record_external("github.com");
  let bin_s = bin.to_string_lossy().into_owned();
  if let Err(e) = run_command(make_command(&bin, &["-U"]), &bin_s, 120_000).await {
    logging::log_entry("ERROR", &format!("yt-dlp update failed: {e}"));
    return;
  }
  if !bin.exists() {
    logging::log_entry("WARN", "yt-dlp disappeared after self-update, reinstalling");
    if let Err(e) = install_yt_dlp(&core, &download_client()).await {
      logging::log_entry("ERROR", &format!("yt-dlp reinstall failed: {e}"));
    }
  }
}

pub async fn check_proxy_alive(proxy_url: &str, timeout: Duration) -> bool {
  if proxy_url.is_empty() {
    return false;
  }
  let formatted = if proxy_url.contains("://") {
    proxy_url.to_string()
  } else {
    format!("socks5://{proxy_url}")
  };
  let Ok(parsed) = Url::parse(&formatted) else {
    return false;
  };
  let Some(port) = parsed.port() else {
    return false;
  };
  if port == 0 {
    return false;
  }
  let host = parsed.host_str().unwrap_or("127.0.0.1");
  let addr = format!("{host}:{port}");
  tokio::time::timeout(timeout, TcpStream::connect(addr))
    .await
    .map(|res| res.is_ok())
    .unwrap_or(false)
}

pub async fn proxy_config(core: &Core, params: Params) -> CoreResult {
  let body = &params.body;
  if body.len() > MAX_BODY_BYTES {
    return json_response(
      413,
      &json!({ "success": false, "error": "payload too large" }),
    );
  }
  let text = std::str::from_utf8(body).unwrap_or("");
  let payload = serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({}));
  let enabled = is_truthy(payload.get("enabled"));
  let candidate = if enabled {
    value_to_string(payload.get("proxyUrl")).trim().to_string()
  } else {
    String::new()
  };

  let active = if !candidate.is_empty() {
    if check_proxy_alive(&candidate, Duration::from_millis(1500)).await {
      candidate
    } else {
      String::new()
    }
  } else {
    String::new()
  };

  if let Ok(mut guard) = core.net.server_active_proxy.lock() {
    *guard = active;
  }

  json_response(200, &json!({ "success": true }))
}

#[cfg(test)]
mod tests {
  use super::*;

  /// SHA-256 de « abc » (FIPS 180-4, annexe B.1).
  const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

  #[test]
  fn hash_file_matches_the_known_digest() {
    let dir = crate::test_temp::TempDir::new("core-hash-file");
    let path = dir.join("abc.bin");
    std::fs::write(&path, b"abc").unwrap();
    assert_eq!(hash_file(&path).as_deref(), Ok(ABC_SHA256));
    assert!(hash_file(&dir.join("absent.bin")).is_err());
  }

  #[test]
  fn hash_reader_retries_after_an_interruption() {
    struct Flaky<'a> {
      interrupted: bool,
      rest: &'a [u8],
    }
    impl std::io::Read for Flaky<'_> {
      fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !std::mem::replace(&mut self.interrupted, true) {
          return Err(std::io::ErrorKind::Interrupted.into());
        }
        self.rest.read(buf)
      }
    }
    let reader = Flaky {
      interrupted: false,
      rest: b"abc",
    };
    assert_eq!(hash_reader(reader).as_deref(), Ok(ABC_SHA256));
  }

  #[test]
  fn the_yt_dlp_check_is_due_at_most_once_per_day_and_can_be_disabled() {
    let day = 24 * 3600 * 1000;
    let now = 10 * day;
    assert!(yt_dlp_check_due(true, None, now));
    assert!(!yt_dlp_check_due(false, None, now));
    assert!(!yt_dlp_check_due(true, Some(now - day + 1), now));
    assert!(yt_dlp_check_due(true, Some(now - day), now));
    assert!(!yt_dlp_check_due(false, Some(now - day), now));
    // Horloge reculée : le fichier est dans le futur, la vérification ne reste pas bloquée.
    assert!(yt_dlp_check_due(true, Some(now + day), now));
  }

  #[test]
  fn unsafe_archive_paths_are_detected() {
    for name in [
      "../evil.exe",
      "..\\evil.exe",
      "a/../../evil.exe",
      "a\\..\\..\\evil.exe",
      "/etc/passwd",
      "\\evil.exe",
      "C:\\evil.exe",
      "C:/evil.exe",
      "..",
      "a/..",
    ] {
      assert!(!is_safe_archive_path(name), "{name} devrait être refusé");
    }
    for name in [
      "ffmpeg-master/bin/ffmpeg.exe",
      "bin\\ffprobe.exe",
      "a..b/c",
      "./bin/ffmpeg",
      "dir/",
    ] {
      assert!(is_safe_archive_path(name), "{name} devrait être accepté");
    }
  }

  /// Écrit un zip non compressé minimal contenant un fichier par nom.
  #[cfg(windows)]
  fn write_zip(path: &Path, names: &[&str]) {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let content = b"contenu";
    let mut crc = flate2::Crc::new();
    crc.update(content);
    let crc = crc.sum();
    let size = content.len() as u32;
    for name in names {
      let offset = local.len() as u32;
      let name_len = name.len() as u16;
      local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
      for v in [20u16, 0, 0, 0, 0x21] {
        local.extend_from_slice(&v.to_le_bytes());
      }
      for v in [crc, size, size] {
        local.extend_from_slice(&v.to_le_bytes());
      }
      local.extend_from_slice(&name_len.to_le_bytes());
      local.extend_from_slice(&0u16.to_le_bytes());
      local.extend_from_slice(name.as_bytes());
      local.extend_from_slice(content);

      central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
      for v in [20u16, 20, 0, 0, 0, 0x21] {
        central.extend_from_slice(&v.to_le_bytes());
      }
      for v in [crc, size, size] {
        central.extend_from_slice(&v.to_le_bytes());
      }
      central.extend_from_slice(&name_len.to_le_bytes());
      for v in [0u16, 0, 0, 0] {
        central.extend_from_slice(&v.to_le_bytes());
      }
      central.extend_from_slice(&0u32.to_le_bytes());
      central.extend_from_slice(&offset.to_le_bytes());
      central.extend_from_slice(name.as_bytes());
    }
    let (central_offset, central_size) = (local.len() as u32, central.len() as u32);
    let count = names.len() as u16;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, count, count] {
      out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    fs::write(path, out).unwrap();
  }

  #[cfg(windows)]
  fn test_dir(name: &str) -> crate::test_temp::TempDir {
    let dir = crate::test_temp::TempDir::new(&format!("tools-test-{name}"));
    fs::create_dir_all(dir.join("work")).unwrap();
    dir
  }

  #[cfg(windows)]
  #[tokio::test]
  async fn a_trapped_archive_is_refused_and_extracts_nothing() {
    for (case, entry) in [
      ("slash", "../evil.exe"),
      ("backslash", "..\\evil.exe"),
      ("absolute", "C:/evil.exe"),
    ] {
      let dir = test_dir(case);
      let archive = dir.join("trap.zip");
      write_zip(&archive, &["bin/ffmpeg.exe", entry]);
      let err = extract_archive(&archive, &dir.join("work"))
        .await
        .expect_err(entry);
      assert!(err.contains("unsafe path"), "{entry} : {err}");
      assert!(
        !dir.join("evil.exe").exists(),
        "{entry} : fichier écrit hors du dossier"
      );
      assert_eq!(
        fs::read_dir(dir.join("work")).unwrap().count(),
        0,
        "{entry} : rien ne doit être extrait"
      );
    }
  }

  #[cfg(windows)]
  #[tokio::test]
  async fn a_clean_archive_is_extracted() {
    let dir = test_dir("clean");
    let archive = dir.join("ok.zip");
    write_zip(&archive, &["ffmpeg-master/bin/ffmpeg.exe"]);
    extract_archive(&archive, &dir.join("work")).await.unwrap();
    let found = find_file(&dir.join("work"), "ffmpeg.exe").expect("fichier extrait");
    assert_eq!(fs::read(found).unwrap(), b"contenu");
  }

  #[test]
  fn tool_downloads_refuse_any_other_origin() {
    let client = download_client();
    let parse = |s: &str| Url::parse(s).unwrap();
    assert!(
      client
        .check(&parse(
          "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
        ))
        .is_ok()
    );
    assert!(
      client
        .check(&parse("https://release-assets.githubusercontent.com/x"))
        .is_ok()
    );
    assert!(
      client
        .check(&parse("https://objects.githubusercontent.com/x"))
        .is_ok()
    );
    assert!(client.check(&parse("https://api.deezer.com/x")).is_err());
    assert!(
      client
        .check(&parse("https://evil.example/yt-dlp.exe"))
        .is_err()
    );
    assert!(client.check(&parse("http://127.0.0.1/yt-dlp.exe")).is_err());
  }

  #[test]
  fn expected_hash_matches_star_and_dot_prefix() {
    let h = "a".repeat(64);
    let sums = format!("{} *yt-dlp.exe\n{} ./yt-dlp_x86.exe\n", h, "B".repeat(64));
    assert_eq!(expected_hash(&sums, "yt-dlp.exe"), Some(h));
    assert_eq!(expected_hash(&sums, "yt-dlp_x86.exe"), Some("b".repeat(64)));
    assert_eq!(expected_hash(&sums, "absent"), None);
  }
}
