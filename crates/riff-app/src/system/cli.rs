//! `riff lyrics` : affiche la ligne de paroles en cours dans le terminal (`cli.js`). Même exécutable, détecté avant
//! toute fenêtre. L'état vient du canal local d'une instance déjà lancée ; la connexion est reprise toutes les 1,5 s.
//! Rendu : une ligne centrée, majuscules grasses cyan pour l'ASCII (repli de l'original quand `cfonts` manque).
//! Écart avec l'original : une ligne entière, pas mot à mot, parce que le flux du canal local porte la ligne courante.

use super::channel::{self, ClientError, LyricsClient};
use super::protocol::LyricsState;
use std::future::Future;
use std::io::{self, Write};
use std::time::Duration;

const RETRY_DELAY: Duration = Duration::from_millis(1500);
const CLEAR: &str = "\x1b[2J\x1b[H";
const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";
const GREY: &str = "\x1b[90m";
const CYAN_BOLD: &str = "\x1b[1;36m";
const WHITE_BOLD: &str = "\x1b[1;97m";
const RESET: &str = "\x1b[0m";

/// Premier argument `lyrics` ou `--lyrics`.
#[must_use]
pub fn is_lyrics_command(args: &[String]) -> bool {
  matches!(
    args.first().map(String::as_str),
    Some("lyrics" | "--lyrics")
  )
}

/// Largeur d'affichage d'un caractère : 0 pour les accents combinants, 2 pour les écritures larges et les emoji.
fn char_width(c: char) -> usize {
  match u32::from(c) {
    0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F => 0,
    0x1100..=0x115F
    | 0x2E80..=0xA4CF
    | 0xAC00..=0xD7A3
    | 0xF900..=0xFAFF
    | 0xFE30..=0xFE6F
    | 0xFF00..=0xFF60
    | 0xFFE0..=0xFFE6
    | 0x1F300..=0x1F64F
    | 0x1F900..=0x1F9FF
    | 0x20000..=0x3FFFD => 2,
    _ => 1,
  }
}

#[must_use]
pub fn display_width(text: &str) -> usize {
  text.chars().map(char_width).sum()
}

/// Le texte vient d'un fichier ou du réseau : aucune séquence d'échappement ne doit atteindre le terminal.
fn clean(text: &str) -> String {
  text
    .chars()
    .filter(|c| !c.is_control())
    .collect::<String>()
    .trim()
    .to_string()
}

/// Coupe à `max` colonnes, avec « … » si le texte est coupé.
fn fit(text: &str, max: usize) -> String {
  if display_width(text) <= max {
    return text.to_string();
  }
  let mut out = String::new();
  let mut used = 0;
  for c in text.chars() {
    let width = char_width(c);
    if used + width + 1 > max {
      break;
    }
    used += width;
    out.push(c);
  }
  out.push('…');
  out
}

fn centred(text: &str, cols: usize) -> String {
  format!(
    "{}{text}",
    " ".repeat(cols.saturating_sub(display_width(text)) / 2)
  )
}

/// Image complète du terminal pour cet état. Pure, pour les essais.
#[must_use]
pub fn frame(state: &LyricsState, cols: usize, rows: usize) -> String {
  let cols = cols.max(10);
  let rows = rows.max(3);
  let header = [clean(&state.artist), clean(&state.title)]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" - ");
  let line = clean(&state.line);
  let mut out = String::from(CLEAR);
  if !header.is_empty() {
    out.push_str(&format!(
      "{GREY}{}{RESET}",
      centred(&fit(&header, cols), cols)
    ));
  }
  out.push('\n');
  if line.is_empty() {
    return out;
  }
  out.push_str(&"\n".repeat((rows / 2).saturating_sub(2)));
  let styled = if line.is_ascii() {
    format!(
      "{CYAN_BOLD}{}{RESET}",
      centred(&fit(&line.to_uppercase(), cols), cols)
    )
  } else {
    format!("{WHITE_BOLD}{}{RESET}", centred(&fit(&line, cols), cols))
  };
  out.push_str(&styled);
  out.push('\n');
  out
}

fn waiting(text: &str) -> String {
  format!("{CLEAR}{GREY}[Riff CLI] {text}{RESET}\n")
}

/// Fin de [`follow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
  /// Au moins une connexion a réussi.
  Connected,
  /// Aucune instance de Riff n'a répondu avant l'arrêt.
  NeverConnected,
}

#[derive(Default)]
struct Progress {
  connected: bool,
  waiting_shown: bool,
}

async fn session<W: Write>(
  label: &str,
  out: &mut W,
  size: &impl Fn() -> (usize, usize),
  progress: &mut Progress,
) -> io::Result<()> {
  let mut client = match LyricsClient::connect(label).await {
    Ok(client) => client,
    Err(error) => {
      if !progress.waiting_shown {
        progress.waiting_shown = true;
        let text = match error {
          ClientError::Unreachable(_) => {
            "No running Riff found on the local channel. Waiting for it to start...".to_string()
          }
          other => format!("{other}. Retrying..."),
        };
        out.write_all(waiting(&text).as_bytes())?;
        out.flush()?;
      }
      return Ok(());
    }
  };
  progress.connected = true;
  progress.waiting_shown = false;
  let mut last: Option<LyricsState> = None;
  loop {
    match client.next().await {
      Ok(Some(state)) => {
        if last.as_ref() == Some(&state) {
          continue;
        }
        let (cols, rows) = size();
        out.write_all(frame(&state, cols, rows).as_bytes())?;
        out.flush()?;
        last = Some(state);
      }
      Ok(None) | Err(_) => {
        progress.waiting_shown = true;
        out.write_all(waiting("Connection lost. Waiting for Riff...").as_bytes())?;
        return out.flush();
      }
    }
  }
}

/// Suit l'instance de Riff jusqu'à `stop`, en se reconnectant tous les `retry`. Une erreur d'écriture (sortie fermée) l'arrête.
pub async fn follow<W: Write>(
  label: &str,
  out: &mut W,
  size: impl Fn() -> (usize, usize),
  retry: Duration,
  stop: impl Future<Output = ()>,
) -> io::Result<Ended> {
  tokio::pin!(stop);
  let mut progress = Progress::default();
  loop {
    tokio::select! {
      () = &mut stop => break,
      result = session(label, out, &size, &mut progress) => result?,
    }
    tokio::select! {
      () = &mut stop => break,
      () = tokio::time::sleep(retry) => {}
    }
  }
  Ok(if progress.connected {
    Ended::Connected
  } else {
    Ended::NeverConnected
  })
}

/// Sortie terminal. Sous Windows, l'exécutable release est du sous-système GUI et n'a pas de sortie standard sauf
/// redirection : on s'attache à la console du parent et on écrit dans `CONOUT$`.
struct Term {
  #[cfg(windows)]
  console: Option<std::fs::File>,
}

/// Mesure de la fenêtre du terminal, indépendante de la sortie pour que les deux se prêtent à `follow`.
struct Probe {
  #[cfg(windows)]
  handle: Option<windows::Win32::Foundation::HANDLE>,
}

impl Term {
  fn open() -> (Self, Probe) {
    #[cfg(windows)]
    {
      use std::os::windows::io::AsRawHandle;
      use windows::Win32::Foundation::HANDLE;
      use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode,
        GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
      };
      // SAFETY: appels Win32 sans pointeur de l'appelant ; les handles ne sont pas fermés ici.
      let standard = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }
        .ok()
        .filter(|handle| !handle.is_invalid() && !handle.0.is_null());
      let (console, handle) = match standard {
        Some(handle) => (None, Some(handle)),
        None => {
          // SAFETY: voir plus haut. L'échec (pas de console parente) laisse `CONOUT$` introuvable, géré ci-dessous.
          let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
          let console = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("CONOUT$")
            .ok();
          let handle = console.as_ref().map(|file| HANDLE(file.as_raw_handle()));
          (console, handle)
        }
      };
      if let Some(handle) = handle {
        let mut mode = Default::default();
        // SAFETY: `mode` est une variable locale valide pour l'appel.
        if unsafe { GetConsoleMode(handle, &mut mode) }.is_ok() {
          // SAFETY: handle de console valide, vérifié par `GetConsoleMode`.
          let _ = unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) };
        }
      }
      (Self { console }, Probe { handle })
    }
    #[cfg(not(windows))]
    (Self {}, Probe {})
  }
}

impl Probe {
  /// (colonnes, lignes) ; 80x24 si la taille est inconnue ou la sortie redirigée.
  fn size(&self) -> (usize, usize) {
    #[cfg(windows)]
    if let Some(handle) = self.handle {
      use windows::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo,
      };
      let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
      // SAFETY: `info` est une structure locale valide pour l'appel.
      if unsafe { GetConsoleScreenBufferInfo(handle, &mut info) }.is_ok() {
        let cols =
          usize::try_from(i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1)
            .unwrap_or(0);
        let rows =
          usize::try_from(i32::from(info.srWindow.Bottom) - i32::from(info.srWindow.Top) + 1)
            .unwrap_or(0);
        if cols > 0 && rows > 0 {
          return (cols, rows);
        }
      }
    }
    #[cfg(not(windows))]
    {
      let read = |name: &str| {
        std::env::var(name)
          .ok()
          .and_then(|value| value.trim().parse::<usize>().ok())
          .filter(|value| *value > 0)
      };
      if let (Some(cols), Some(rows)) = (read("COLUMNS"), read("LINES")) {
        return (cols, rows);
      }
    }
    (80, 24)
  }
}

impl Write for Term {
  fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
    #[cfg(windows)]
    if let Some(console) = self.console.as_mut() {
      return console.write(bytes);
    }
    io::stdout().write(bytes)
  }

  fn flush(&mut self) -> io::Result<()> {
    #[cfg(windows)]
    if let Some(console) = self.console.as_mut() {
      return console.flush();
    }
    io::stdout().flush()
  }
}

/// Lance le mode terminal. Rend le code de sortie : 0 après une session, 1 si Riff n'a jamais répondu ou si la sortie est fermée.
pub fn run() -> i32 {
  let (mut term, probe) = Term::open();
  let label = channel::channel_name();
  let _ = term.write_all(HIDE_CURSOR.as_bytes());
  let size = move || probe.size();
  let stop = async {
    let _ = tokio::signal::ctrl_c().await;
  };
  let result =
    super::startup::runtime().block_on(follow(&label, &mut term, size, RETRY_DELAY, stop));
  let _ = term.write_all(format!("{SHOW_CURSOR}{CLEAR}").as_bytes());
  let _ = term.flush();
  match result {
    Ok(Ended::Connected) => 0,
    Ok(Ended::NeverConnected) => {
      eprintln!("riff lyrics: no running Riff found on the local channel.");
      1
    }
    Err(_) => 1,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::system::channel::{Claim, claim};
  use std::sync::atomic::{AtomicU32, Ordering};
  use tokio::sync::mpsc;

  fn unique() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
      "riff-cli-test-{}-{}",
      std::process::id(),
      NEXT.fetch_add(1, Ordering::Relaxed)
    )
  }

  fn state(line: &str) -> LyricsState {
    LyricsState {
      title: "Titre".into(),
      artist: "Artiste".into(),
      line: line.into(),
      playing: true,
    }
  }

  #[test]
  fn only_the_first_argument_selects_the_command() {
    let args = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert!(is_lyrics_command(&args(&["lyrics"])));
    assert!(is_lyrics_command(&args(&["--lyrics", "x"])));
    assert!(!is_lyrics_command(&args(&["a.mp3", "lyrics"])));
    assert!(!is_lyrics_command(&[]));
  }

  #[test]
  fn wide_characters_count_double_and_accents_count_zero() {
    assert_eq!(display_width("abc"), 3);
    assert_eq!(display_width("日本語"), 6);
    assert_eq!(display_width("e\u{301}"), 1);
    assert_eq!(display_width("한국어"), 6);
  }

  #[test]
  fn an_ascii_line_is_upper_case_bold_cyan_and_centred() {
    let out = frame(&state("hello"), 21, 10);
    assert!(out.starts_with(CLEAR));
    assert!(
      out.contains(&format!("{CYAN_BOLD}{}HELLO{RESET}", " ".repeat(8))),
      "{out:?}"
    );
  }

  #[test]
  fn a_non_ascii_line_keeps_its_case_and_is_centred_by_display_width() {
    let out = frame(&state("日本語"), 20, 10);
    assert!(
      out.contains(&format!("{WHITE_BOLD}{}日本語{RESET}", " ".repeat(7))),
      "{out:?}"
    );
    assert!(frame(&state("Été"), 20, 10).contains("Été"));
  }

  #[test]
  fn a_line_wider_than_the_terminal_is_cut_with_an_ellipsis() {
    let out = frame(&state("日本語日本語日本語日本語"), 12, 10);
    assert!(out.contains('…'));
    let drawn = out.lines().find(|line| line.contains('日')).unwrap();
    let visible: String = drawn.replace(WHITE_BOLD, "").replace(RESET, "");
    assert!(display_width(&visible) <= 12, "{visible:?}");
  }

  #[test]
  fn escape_sequences_in_the_lyrics_never_reach_the_terminal() {
    let hostile = LyricsState {
      title: "t\x1b]0;pwned\x07".into(),
      artist: String::new(),
      line: "a\x1b[2Jb\r\nc".into(),
      playing: true,
    };
    let out = frame(&hostile, 40, 10);
    let allowed = [CLEAR, GREY, CYAN_BOLD, WHITE_BOLD, RESET];
    let mut rest = out.clone();
    for sequence in allowed {
      rest = rest.replace(sequence, "");
    }
    assert!(
      !rest.contains('\x1b') && !rest.contains('\x07') && !rest.contains('\r'),
      "{out:?}"
    );
  }

  #[test]
  fn an_empty_line_shows_only_the_header() {
    let out = frame(&state(""), 40, 10);
    assert!(out.contains("Artiste - Titre") && !out.contains(CYAN_BOLD));
  }

  #[tokio::test]
  async fn without_a_running_riff_it_says_so_once_and_ends_as_never_connected() {
    let mut out = Vec::new();
    let stop = tokio::time::sleep(Duration::from_millis(400));
    let ended = follow(
      &unique(),
      &mut out,
      || (40, 10),
      Duration::from_millis(50),
      stop,
    )
    .await
    .unwrap();
    assert_eq!(ended, Ended::NeverConnected);
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.matches("No running Riff found").count(), 1, "{text:?}");
  }

  #[tokio::test]
  async fn it_draws_the_published_lines_and_reconnects_after_a_restart() {
    let label = unique();
    let (tx, _rx) = mpsc::unbounded_channel();
    let Claim::Primary(server) = claim(&label, tx).await.expect("claim") else {
      panic!("canal déjà pris")
    };
    server.publish(state("first line"));
    let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Shared {
      fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
      }
      fn flush(&mut self) -> io::Result<()> {
        Ok(())
      }
    }
    let watcher = std::sync::Arc::clone(&buffer);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let label_task = label.clone();
    let task = tokio::spawn(async move {
      let mut out = Shared(watcher);
      follow(
        &label_task,
        &mut out,
        || (40, 10),
        Duration::from_millis(50),
        async {
          let _ = stop_rx.await;
        },
      )
      .await
    });
    let wait_for = |needle: &'static str| {
      let buffer = std::sync::Arc::clone(&buffer);
      async move {
        for _ in 0..80 {
          if String::from_utf8_lossy(&buffer.lock().unwrap()).contains(needle) {
            return true;
          }
          tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
      }
    };
    assert!(wait_for("FIRST LINE").await, "première ligne non dessinée");
    drop(server);
    assert!(
      wait_for("Connection lost").await,
      "perte de connexion non signalée"
    );
    let (tx2, _rx2) = mpsc::unbounded_channel();
    let mut second = None;
    for _ in 0..40 {
      if let Ok(Claim::Primary(server)) = claim(&label, tx2.clone()).await {
        second = Some(server);
        break;
      }
      tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let second = second.expect("canal libéré après l'arrêt du serveur");
    second.publish(state("second line"));
    assert!(wait_for("SECOND LINE").await, "reconnexion absente");
    let _ = stop_tx.send(());
    assert_eq!(task.await.unwrap().unwrap(), Ended::Connected);
  }

  #[tokio::test]
  async fn a_closed_output_ends_the_loop_with_an_error() {
    struct Closed;
    impl Write for Closed {
      fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
      }
      fn flush(&mut self) -> io::Result<()> {
        Ok(())
      }
    }
    let result = follow(
      &unique(),
      &mut Closed,
      || (40, 10),
      Duration::from_millis(20),
      std::future::pending(),
    )
    .await;
    assert!(result.is_err());
  }
}
