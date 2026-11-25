use super::*;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::Command;

/// Page publique lue par yt-dlp pour fermer proprement et écrire les cookies.
const COOKIE_PROBE_URL: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
static COOKIE_SEQ: AtomicU64 = AtomicU64::new(0);
static COOKIE_CACHE: Mutex<(Option<SystemTime>, bool)> = Mutex::new((None, false));

pub fn yt_cookies_path() -> PathBuf {
  paths::resolve_data_dir().join("yt-cookies.txt")
}

pub fn cookie_jar_has_yt_login(text: &str) -> bool {
  let (mut login_info, mut sapisid, mut sid) = (false, false, false);
  for line in text.lines() {
    // `#HttpOnly_` n'est pas un commentaire : c'est le préfixe des cookies HttpOnly du format Netscape.
    let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 7 {
      continue;
    }
    let domain = parts[0].to_lowercase();
    let domain = domain.trim_start_matches('.');
    if domain != "youtube.com" && !domain.ends_with(".youtube.com") {
      continue;
    }
    match parts[5] {
      "LOGIN_INFO" => login_info = true,
      "SAPISID" => sapisid = true,
      "SID" => sid = true,
      _ => {}
    }
  }
  login_info || (sapisid && sid)
}

/// Importe un `cookies.txt` choisi par l'utilisateur vers `yt-cookies.txt` : seulement une session YouTube valide.
///
/// # Errors
/// Fichier illisible, sans session YouTube, ou écriture impossible.
pub fn import_cookies(source: &Path) -> Result<(), String> {
  let text = fs::read_to_string(source).map_err(|_| "Could not read that file".to_string())?;
  if !cookie_jar_has_yt_login(&text) {
    return Err("This file has no YouTube sign-in cookies".to_string());
  }
  write_cookies(&yt_cookies_path(), &text)
}

/// Écrit le fichier de cookies, en 0600 hors Windows, sans jamais laisser un fichier à moitié écrit.
fn write_cookies(target: &Path, text: &str) -> Result<(), String> {
  let failure = |_: std::io::Error| "Could not write the cookies file".to_string();
  if let Some(parent) = target.parent() {
    fs::create_dir_all(parent).map_err(failure)?;
  }
  let temp = target.with_extension(format!("txt.{}.tmp", std::process::id()));
  fs::write(&temp, text).map_err(failure)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o600)).map_err(failure)?;
  }
  fs::rename(&temp, target).map_err(failure)
}

/// Déconnexion YouTube : supprime `yt-cookies.txt`. Vrai si le fichier a disparu.
#[must_use]
pub fn delete_cookies() -> bool {
  let path = yt_cookies_path();
  match fs::remove_file(&path) {
    Ok(()) => true,
    Err(err) => err.kind() == std::io::ErrorKind::NotFound,
  }
}

/// Navigateurs proposés pour `--cookies-from-browser`. Sous Windows, Firefox seul : Chrome et Edge chiffrent leurs
/// cookies (chiffrement App-Bound, yt-dlp n° 10927).
#[must_use]
pub const fn cookie_browsers() -> &'static [&'static str] {
  if cfg!(windows) {
    &["firefox"]
  } else {
    &[
      "firefox", "chrome", "chromium", "brave", "edge", "opera", "vivaldi",
    ]
  }
}

/// Arguments de yt-dlp qui exportent les cookies d'un navigateur vers `target`. yt-dlp écrit le fichier en se fermant.
#[must_use]
pub fn browser_cookies_args(browser: &str, target: &Path) -> Vec<String> {
  vec![
    "--cookies-from-browser".to_string(),
    browser.to_string(),
    "--cookies".to_string(),
    target.to_string_lossy().into_owned(),
    "--skip-download".to_string(),
    "--no-warnings".to_string(),
    "--no-playlist".to_string(),
    COOKIE_PROBE_URL.to_string(),
  ]
}

/// Exporte les cookies YouTube d'un navigateur vers `yt-cookies.txt`. Lance yt-dlp, qui contacte YouTube.
///
/// # Errors
/// Navigateur non proposé, yt-dlp absent ou en échec, ou pas de session YouTube dans ce navigateur.
pub async fn import_browser_cookies(browser: &str) -> Result<(), String> {
  if !cookie_browsers().contains(&browser) {
    return Err("Unsupported browser".to_string());
  }
  let Some(bin) = paths::resolve_binary("yt-dlp") else {
    return Err("yt-dlp not found".to_string());
  };
  let target = yt_cookies_path();
  let temp = target.with_extension(format!("browser.{}.tmp", std::process::id()));
  let mut command = Command::new(bin);
  command
    .args(browser_cookies_args(browser, &temp))
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true);
  #[cfg(windows)]
  command.creation_flags(0x0800_0000);
  let ran = command.status().await;
  let text = fs::read_to_string(&temp).unwrap_or_default();
  let _ = fs::remove_file(&temp);
  if ran.is_err() || !cookie_jar_has_yt_login(&text) {
    return Err("No YouTube sign-in found in this browser".to_string());
  }
  write_cookies(&target, &text)
}

pub fn has_yt_login_cookies() -> bool {
  let path = yt_cookies_path();
  let Ok(mtime) = fs::metadata(&path).and_then(|m| m.modified()) else {
    if let Ok(mut c) = COOKIE_CACHE.lock() {
      *c = (None, false);
    }
    return false;
  };
  if let Ok(c) = COOKIE_CACHE.lock()
    && c.0 == Some(mtime)
  {
    return c.1;
  }
  let valid = fs::read_to_string(&path)
    .map(|t| cookie_jar_has_yt_login(&t))
    .unwrap_or(false);
  if let Ok(mut c) = COOKIE_CACHE.lock() {
    *c = (Some(mtime), valid);
  }
  valid
}

// En-têtes d'authentification YouTube (SAPISIDHASH), None si pas de session.
pub fn yt_auth_headers(origin: &str) -> Option<Vec<(&'static str, String)>> {
  let raw = fs::read_to_string(yt_cookies_path()).ok()?;
  let mut pairs: Vec<String> = Vec::new();
  let mut sapisid = String::new();
  for line in raw.lines() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
      continue;
    }
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() >= 7 && parts[0].contains("youtube.com") {
      pairs.push(format!("{}={}", parts[5], parts[6]));
      if parts[5] == "SAPISID" || (sapisid.is_empty() && parts[5] == "__Secure-3PAPISID") {
        sapisid = parts[6].to_string();
      }
    }
  }
  if sapisid.is_empty() || pairs.is_empty() {
    return None;
  }
  let ts = now_millis() / 1000;
  let hash = sha1_hex(&format!("{ts} {sapisid} {origin}"));
  Some(vec![
    ("Cookie", pairs.join("; ")),
    ("Authorization", format!("SAPISIDHASH {ts}_{hash}")),
    ("Origin", origin.to_string()),
    ("X-Origin", origin.to_string()),
    ("X-Goog-AuthUser", "0".to_string()),
  ])
}

struct CookieCopy(Option<PathBuf>);

impl Drop for CookieCopy {
  fn drop(&mut self) {
    if let Some(p) = self.0.take() {
      let _ = fs::remove_file(p);
    }
  }
}

// yt-dlp.exe (PyInstaller) lance un processus enfant : tuer le parent ne suffit pas, on tue l'arbre
// de façon synchrone (le parent doit encore exister pour que /T retrouve l'enfant).
struct TreeKill(Option<u32>);

impl TreeKill {
  fn disarm(&mut self) {
    self.0 = None;
  }
}

impl Drop for TreeKill {
  fn drop(&mut self) {
    #[cfg(windows)]
    if let Some(pid) = self.0.take() {
      use std::os::windows::process::CommandExt;
      let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    }
  }
}

fn has_flag(args: &[String], flag: &str) -> bool {
  args.iter().any(|a| a == flag)
}

// Lance yt-dlp avec les mêmes options que Riff. Le processus est tué à l'expiration
// du délai ou si le futur est abandonné (kill_on_drop).
pub async fn execute_yt_dlp(
  core: &Core,
  args: &[String],
  timeout_ms: u64,
  quiet: bool,
) -> Result<String, String> {
  let bin = paths::resolve_binary("yt-dlp").ok_or_else(|| {
    "yt-dlp not found. Put yt-dlp and ffmpeg into the bin folder next to the app, or add them to PATH."
      .to_string()
  })?;

  let mut final_args: Vec<String> = if has_flag(args, "--no-config") {
    Vec::new()
  } else {
    vec!["--no-config".into()]
  };
  let proxy = core
    .net
    .server_active_proxy
    .lock()
    .map(|g| g.trim().to_string())
    .unwrap_or_default();
  let has_proxy = args
    .iter()
    .any(|a| a == "--proxy" || a.starts_with("--proxy="));
  if !has_proxy && !proxy.is_empty() {
    final_args.push("--proxy".into());
    final_args.push(proxy);
  }
  if !has_flag(args, "--ffmpeg-location")
    && let Some(ff) = paths::resolve_binary("ffmpeg")
    && let Some(dir) = ff.parent()
  {
    final_args.push("--ffmpeg-location".into());
    final_args.push(dir.to_string_lossy().into_owned());
  }
  if !has_flag(args, "--force-ipv4") {
    final_args.push("--force-ipv4".into());
  }
  if !has_flag(args, "--geo-bypass") {
    final_args.push("--geo-bypass".into());
  }
  // yt-dlp réécrit le fichier de cookies : on lui donne une copie jetable.
  let mut cookie_copy = CookieCopy(None);
  if !has_flag(args, "--cookies") && has_yt_login_cookies() {
    let src = yt_cookies_path();
    let tmp = paths::resolve_data_dir().join(format!(
      ".yt-cookies-{}-{}-{}.tmp",
      std::process::id(),
      now_millis(),
      COOKIE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let used = if fs::copy(&src, &tmp).is_ok() {
      cookie_copy.0 = Some(tmp.clone());
      tmp
    } else {
      src
    };
    final_args.push("--cookies".into());
    final_args.push(used.to_string_lossy().into_owned());
  }
  final_args.extend(args.iter().cloned());

  let mut cmd = Command::new(&bin);
  cmd.args(&final_args).stdin(std::process::Stdio::null());
  // Sous Windows c'est TreeKill qui tue l'arbre complet (parent d'abord = enfant orphelin).
  #[cfg(not(windows))]
  cmd.kill_on_drop(true);
  if !has_proxy && final_args.iter().all(|a| a != "--proxy") {
    for k in [
      "HTTP_PROXY",
      "HTTPS_PROXY",
      "ALL_PROXY",
      "http_proxy",
      "https_proxy",
      "all_proxy",
    ] {
      cmd.env_remove(k);
    }
  }
  #[cfg(windows)]
  cmd.creation_flags(0x0800_0000);

  cmd
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
  let child = cmd.spawn().map_err(|e| {
    logging::log_entry("ERROR", &format!("yt-dlp spawn error: {e}"));
    e.to_string()
  })?;
  let mut tree_kill = TreeKill(child.id());
  let fut = child.wait_with_output();
  let result = if timeout_ms > 0 {
    match tokio::time::timeout(Duration::from_millis(timeout_ms), fut).await {
      Ok(r) => r,
      Err(_) => return Err(format!("yt-dlp timed out after {timeout_ms}ms")),
    }
  } else {
    fut.await
  };
  tree_kill.disarm();
  let output = result.map_err(|e| e.to_string())?;

  if output.status.success() {
    return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
  }
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  let snippet: String = stderr.chars().take(300).collect();
  let first = args.iter().take(3).cloned().collect::<Vec<_>>().join(" ");
  if !quiet {
    logging::log_entry("WARN", &format!("yt-dlp failed: {first} - {snippet}"));
  }
  if stderr.is_empty() {
    Err(format!(
      "yt-dlp exited with code {}",
      output.status.code().unwrap_or(-1)
    ))
  } else {
    Err(stderr)
  }
}

/// Vrai si `url` peut être donnée à yt-dlp : il fait son propre trafic, hors du client réseau, donc seules les URL que
/// le profil `Service` accepterait lui parviennent.
pub fn is_allowed_target(client: &Http, url: &str) -> bool {
  Url::parse(url).is_ok_and(|parsed| client.check(&parsed).is_ok())
}

#[cfg(test)]
mod target_tests {
  use super::*;

  #[test]
  fn only_service_urls_reach_yt_dlp() {
    let client = Http::service();
    assert!(is_allowed_target(
      &client,
      "https://www.youtube.com/watch?v=abc"
    ));
    assert!(is_allowed_target(&client, "https://soundcloud.com/a/b"));
    for url in [
      "http://192.168.1.1/",
      "http://127.0.0.1:9/x",
      "https://evil.example/a",
      "file:///C:/a",
      "--exec=calc",
      "",
      "pas une url",
    ] {
      assert!(!is_allowed_target(&client, url), "{url}");
    }
  }
}

#[cfg(test)]
mod cookie_tests {
  use super::*;

  const JAR: &str = "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t1893456000\tSAPISID\tvaleur\n.youtube.com\tTRUE\t/\tTRUE\t1893456000\tSID\tvaleur\n";

  #[test]
  fn an_httponly_cookie_counts_as_a_login() {
    assert!(cookie_jar_has_yt_login(
      "#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1893456000\tLOGIN_INFO\tvaleur\n"
    ));
  }

  #[test]
  fn a_comment_or_another_site_is_not_a_login() {
    assert!(!cookie_jar_has_yt_login(
      "# .youtube.com\tTRUE\t/\tTRUE\t1\tLOGIN_INFO\tv\n"
    ));
    assert!(!cookie_jar_has_yt_login(
      ".example.com\tTRUE\t/\tTRUE\t1\tLOGIN_INFO\tv\n"
    ));
    assert!(cookie_jar_has_yt_login(JAR));
  }

  #[test]
  fn writing_leaves_the_exact_file_and_no_temporary() {
    let dir = crate::test_temp::TempDir::new("cookies");
    let target = dir.join("yt-cookies.txt");
    write_cookies(&target, JAR).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), JAR);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
      );
    }
  }

  #[test]
  fn a_file_without_a_login_is_refused_and_nothing_is_written() {
    let dir = crate::test_temp::TempDir::new("cookies-source");
    let source = dir.join("source.txt");
    fs::write(&source, ".example.com\tTRUE\t/\tTRUE\t1\tSID\tv\n").unwrap();
    assert_eq!(
      import_cookies(&source),
      Err("This file has no YouTube sign-in cookies".to_string())
    );
    assert_eq!(
      import_cookies(Path::new("Z:/absent/rien.txt")),
      Err("Could not read that file".to_string())
    );
  }

  #[test]
  fn the_browser_export_writes_to_the_given_file() {
    let args = browser_cookies_args("firefox", Path::new("out.txt"));
    assert_eq!(
      args[..4],
      ["--cookies-from-browser", "firefox", "--cookies", "out.txt"]
    );
    assert!(args.contains(&"--skip-download".to_string()));
  }

  #[test]
  fn windows_only_offers_firefox() {
    assert!(cookie_browsers().contains(&"firefox"));
    if cfg!(windows) {
      assert_eq!(cookie_browsers(), ["firefox"]);
    }
  }

  #[tokio::test]
  async fn an_unlisted_browser_is_refused_before_starting_anything() {
    assert_eq!(
      import_browser_cookies("netscape").await,
      Err("Unsupported browser".to_string())
    );
  }
}
