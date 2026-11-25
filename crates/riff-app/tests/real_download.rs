//! Vérification réelle : réseau, yt-dlp et ffmpeg, résultats lus par ffprobe.
//!
//! Ignoré par défaut. À lancer sur un dossier de données temporaire :
//! `cargo test -p riff-app --test real_download -- --ignored --nocapture --test-threads=1`

use bytes::Bytes;
use riff_app::backend::Backend;
use riff_audio::offline::{encode_wav_16, render};
use riff_audio::waveform::decode_all;
use riff_audio::{AudioSettings, Source};
use riff_core::reply::Reply;
use riff_core::services::download::{EncodeFile, encode_file};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "support/temp.rs"]
mod temp;

/// « Me at the zoo », 19 s : court et stable.
const VIDEO_ID: &str = "jNQXAC9IVRw";
const TITLE: &str = "Essai partie 8";
const ARTIST: &str = "Riff natif";

fn out_dir(name: &str) -> temp::TempDir {
  temp::TempDir::new(&format!("p8-real-{name}"))
}

fn probe(file: &Path) -> Value {
  let ffprobe = riff_core::paths::resolve_binary("ffprobe").expect("ffprobe");
  let output = Command::new(ffprobe)
    .args([
      "-v",
      "error",
      "-print_format",
      "json",
      "-show_format",
      "-show_streams",
    ])
    .arg(file)
    .output()
    .unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  serde_json::from_slice(&output.stdout).unwrap()
}

fn stream<'a>(probe: &'a Value, kind: &str) -> Option<&'a Value> {
  probe["streams"]
    .as_array()
    .unwrap()
    .iter()
    .find(|stream| stream["codec_type"] == kind)
}

fn tag(probe: &Value, key: &str) -> String {
  let tags = probe["format"]["tags"]
    .as_object()
    .cloned()
    .unwrap_or_default();
  tags
    .iter()
    .find(|(name, _)| name.eq_ignore_ascii_case(key))
    .and_then(|(_, value)| value.as_str())
    .unwrap_or("")
    .to_string()
}

fn duration(probe: &Value) -> f64 {
  probe["format"]["duration"]
    .as_str()
    .unwrap()
    .parse()
    .unwrap()
}

fn report(label: &str, file: &Path) -> Value {
  let probe = probe(file);
  let audio = stream(&probe, "audio").expect("flux audio");
  let cover = stream(&probe, "video");
  println!(
    "{label} : codec {}, débit {}, durée {:.2} s, titre {:?}, artiste {:?}, pochette {}",
    audio["codec_name"].as_str().unwrap_or("?"),
    probe["format"]["bit_rate"].as_str().unwrap_or("?"),
    duration(&probe),
    tag(&probe, "title"),
    tag(&probe, "artist"),
    cover.map_or("absente".to_string(), |video| format!(
      "{}x{}",
      video["width"], video["height"]
    )),
  );
  probe
}

async fn call(backend: &Backend, path: &str, query: &[(&str, String)]) -> Value {
  let query: HashMap<String, String> = query
    .iter()
    .map(|(key, value)| ((*key).to_string(), value.clone()))
    .collect();
  match backend.call(path, query, None::<Bytes>).await.unwrap() {
    Reply::Json { body, .. } => body,
    other => panic!("réponse {}", other.status()),
  }
}

async fn download(
  backend: &Backend,
  dir: &Path,
  format: &str,
  quality: &str,
  trim: Option<(f64, f64)>,
) -> PathBuf {
  let mut query = vec![
    ("id", VIDEO_ID.to_string()),
    ("url", format!("https://www.youtube.com/watch?v={VIDEO_ID}")),
    ("title", TITLE.to_string()),
    ("artist", ARTIST.to_string()),
    (
      "thumbnail",
      format!("https://i.ytimg.com/vi/{VIDEO_ID}/hqdefault.jpg"),
    ),
    ("format", format.to_string()),
    ("quality", quality.to_string()),
    ("customDir", dir.to_string_lossy().into_owned()),
  ];
  if let Some((start, end)) = trim {
    query.push(("start", start.to_string()));
    query.push(("end", end.to_string()));
  }
  let body = call(backend, "/api/download", &query).await;
  assert_eq!(body["success"], true, "{body}");
  dir.join(body["filename"].as_str().unwrap())
}

fn backend() -> Backend {
  Backend::new().expect("moteur").0
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "réseau, yt-dlp et ffmpeg réels"]
async fn a_real_mp3_download_has_its_tags_and_cover_without_320_kbits() {
  let dir = out_dir("mp3");
  let file = download(&backend(), &dir, "mp3", "0", None).await;
  let probe = report("mp3", &file);
  assert_eq!(stream(&probe, "audio").unwrap()["codec_name"], "mp3");
  assert_eq!(tag(&probe, "title"), TITLE);
  assert_eq!(tag(&probe, "artist"), ARTIST);
  assert!(stream(&probe, "video").is_some(), "pochette");
  let rate: u64 = probe["format"]["bit_rate"]
    .as_str()
    .unwrap()
    .parse()
    .unwrap();
  assert!(rate < 315_000, "pas de ré-encodage à 320 kbit/s : {rate}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "réseau, yt-dlp et ffmpeg réels"]
async fn a_real_flac_download_has_its_tags_and_cover() {
  let dir = out_dir("flac");
  let file = download(&backend(), &dir, "flac", "0", None).await;
  let probe = report("flac", &file);
  assert_eq!(stream(&probe, "audio").unwrap()["codec_name"], "flac");
  assert_eq!(tag(&probe, "title"), TITLE);
  assert_eq!(tag(&probe, "artist"), ARTIST);
  assert!(stream(&probe, "video").is_some(), "pochette");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "réseau, yt-dlp et ffmpeg réels"]
async fn a_real_trimmed_download_has_the_expected_length_and_cover() {
  let dir = out_dir("trim");
  let file = download(&backend(), &dir, "mp3", "0", Some((2.0, 7.0))).await;
  let probe = report("découpe mp3", &file);
  assert!((duration(&probe) - 5.0).abs() < 0.3, "{}", duration(&probe));
  assert!(
    stream(&probe, "video").is_some(),
    "pochette avec une découpe"
  );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "réseau, yt-dlp et ffmpeg réels"]
async fn a_real_download_with_effects_has_the_rendered_length() {
  let dir = out_dir("effects");
  let backend = backend();
  let query = [
    ("id", VIDEO_ID.to_string()),
    ("url", format!("https://www.youtube.com/watch?v={VIDEO_ID}")),
    ("title", TITLE.to_string()),
    ("artist", ARTIST.to_string()),
    ("raw", "1".to_string()),
    ("customDir", dir.to_string_lossy().into_owned()),
  ];
  let body = call(&backend, "/api/download", &query).await;
  assert_eq!(body["success"], true, "{body}");
  let raw = dir.join(".tmp").join(body["tempName"].as_str().unwrap());
  let pcm = decode_all(&Source::File(raw)).unwrap();
  let settings = AudioSettings {
    speed: 1.5,
    ..AudioSettings::default()
  };
  let rendered = render(&pcm.samples, 2, pcm.rate, &settings, None);
  let wav = dir.join("rendu.wav");
  std::fs::write(&wav, encode_wav_16(&rendered, 2, pcm.rate)).unwrap();
  let done = encode_file(
    &backend.core,
    &EncodeFile {
      input: wav,
      title: TITLE.to_string(),
      artist: ARTIST.to_string(),
      format: "mp3".to_string(),
      quality: "0".to_string(),
      thumbnail: format!("https://i.ytimg.com/vi/{VIDEO_ID}/hqdefault.jpg"),
      custom_dir: dir.to_string_lossy().into_owned(),
      ..EncodeFile::default()
    },
  )
  .await
  .unwrap();
  let probe = report("avec effets x1,5", &done.directory.join(&done.filename));
  let frames = pcm.samples.len() / 2;
  let expected = (frames as f64 / 1.5).ceil() / f64::from(pcm.rate);
  println!(
    "attendu {expected:.3} s, source {:.3} s",
    pcm.duration_secs()
  );
  assert!(
    (duration(&probe) - expected).abs() < 0.1,
    "{} {expected}",
    duration(&probe)
  );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "réseau, yt-dlp et ffmpeg réels"]
async fn a_real_cover_uses_the_best_size_available() {
  let dir = out_dir("cover");
  let body = call(
    &backend(),
    "/api/download",
    &[
      ("id", "dQw4w9WgXcQ".to_string()),
      (
        "url",
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
      ),
      ("title", TITLE.to_string()),
      ("artist", ARTIST.to_string()),
      (
        "thumbnail",
        "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_string(),
      ),
      ("format", "mp3".to_string()),
      ("quality", "5".to_string()),
      ("customDir", dir.to_string_lossy().into_owned()),
    ],
  )
  .await;
  assert_eq!(body["success"], true, "{body}");
  let probe = report(
    "meilleure pochette",
    &dir.join(body["filename"].as_str().unwrap()),
  );
  let video = stream(&probe, "video").expect("pochette");
  assert!(video["width"].as_u64().unwrap() > 480, "{video}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "réseau réel"]
async fn a_real_playlist_import_returns_tracks() {
  let backend = backend();
  for url in [
    "https://www.deezer.com/playlist/908622995",
    "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
  ] {
    let body = call(
      &backend,
      "/api/playlist-import",
      &[("url", url.to_string())],
    )
    .await;
    println!(
      "{url} : succès {}, {} titres, nom {:?}, erreur {:?}",
      body["success"],
      body["tracks"].as_array().map_or(0, Vec::len),
      body["name"],
      body["error"]
    );
    assert_eq!(body["success"], true, "{body}");
    assert!(!body["tracks"].as_array().unwrap().is_empty());
  }
}

fn isolated_data_dir() -> PathBuf {
  let dir = std::env::var_os("RIFF_DATA_DIR")
    .expect("à lancer avec RIFF_DATA_DIR sur un dossier temporaire");
  let dir = PathBuf::from(dir);
  std::fs::create_dir_all(&dir).unwrap();
  dir
}

#[test]
#[ignore = "écrit dans RIFF_DATA_DIR"]
fn cookies_are_imported_then_deleted_by_the_log_out_button() {
  use riff_core::services::ytdlp::{
    delete_cookies, has_yt_login_cookies, import_cookies, yt_cookies_path,
  };
  isolated_data_dir();
  let scratch = temp::TempDir::new("p8-cookies");
  let source = scratch.join("cookies.txt");
  std::fs::write(
    &source,
    "#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1893456000\tLOGIN_INFO\tvaleur\n",
  )
  .unwrap();
  import_cookies(&source).unwrap();
  assert!(
    yt_cookies_path().starts_with(isolated_data_dir().canonicalize().unwrap())
      || yt_cookies_path().starts_with(isolated_data_dir())
  );
  assert!(yt_cookies_path().exists());
  assert!(has_yt_login_cookies());
  assert!(delete_cookies());
  assert!(!yt_cookies_path().exists());
  assert!(!has_yt_login_cookies());
  assert!(delete_cookies(), "déjà supprimé : pas une erreur");
}

#[test]
#[ignore = "écrit dans RIFF_DATA_DIR"]
fn the_data_folder_changes_with_and_without_its_content() {
  let current = isolated_data_dir();
  std::fs::write(current.join("user-data.json"), "{}").unwrap();
  std::fs::write(current.join("native-settings.json"), "{}").unwrap();
  let scratch = temp::TempDir::new("p8-moved");
  let moved = scratch.join("moved");
  let empty = scratch.join("empty");
  assert!(riff_app::manage::change_data_dir(&moved, true).is_ok());
  assert!(moved.join("user-data.json").exists(), "données déplacées");
  assert!(
    moved.join("native-settings.json").exists(),
    "réglages déplacés"
  );
  assert!(
    !current.join("user-data.json").exists(),
    "plus rien à l'ancien endroit"
  );
  std::fs::write(current.join("user-data.json"), "{}").unwrap();
  assert!(riff_app::manage::change_data_dir(&empty, false).is_ok());
  assert!(!empty.join("user-data.json").exists(), "sans déplacement");
  assert!(
    current.join("user-data.json").exists(),
    "sans déplacement, rien n'est retiré"
  );
  let _ = riff_core::config::set_data_dir_config(None);
}
