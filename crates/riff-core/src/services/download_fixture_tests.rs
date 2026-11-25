//! Commande réelle du core, ffmpeg réel, résultat lu par ffprobe.
//! Fixture : sinus généré par `ffmpeg -f lavfi`, pochette JPEG générée, aucun réseau.

use super::*;
use crate::core::Core;
use crate::test_temp::TempDir;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

const COVER_SIZE: (u64, u64) = (640, 480);
const SOURCE_BITRATE: u64 = 128_000;
const BITRATE_TOLERANCE: f64 = 0.03;

struct Fixture {
  dir: TempDir,
  _data: DataDir,
  ffmpeg: PathBuf,
  ffprobe: PathBuf,
}

/// `Core::new` crée les dossiers de données : on le pointe sur un dossier temporaire, jamais sur le vrai.
/// Partagé par les fixtures vivantes du processus, supprimé avec la dernière.
struct DataDir;

static DATA_USERS: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

impl DataDir {
  fn path() -> PathBuf {
    std::env::temp_dir().join(format!("riff-encode-data-{}", std::process::id()))
  }

  fn acquire() -> Self {
    static DONE: std::sync::Once = std::sync::Once::new();
    let mut users = DATA_USERS.lock().unwrap_or_else(|e| e.into_inner());
    fs::create_dir_all(Self::path()).unwrap();
    // SAFETY : posé une seule fois, avant le premier `Core::new` de ces tests (tous passent par `Fixture::new`).
    DONE.call_once(|| unsafe { std::env::set_var("RIFF_DATA_DIR", Self::path()) });
    *users += 1;
    Self
  }
}

impl Drop for DataDir {
  fn drop(&mut self) {
    let mut users = DATA_USERS.lock().unwrap_or_else(|e| e.into_inner());
    *users -= 1;
    if *users == 0 {
      let _ = fs::remove_dir_all(Self::path());
    }
  }
}

impl Fixture {
  /// `None` si ffmpeg manque : le test le dit au lieu de réussir en silence.
  /// Le harnais n'a pas d'état « ignoré » à l'exécution : `RIFF_REQUIRE_FFMPEG=1` fait échouer le test à la place.
  fn new(name: &str) -> Option<Self> {
    let data = DataDir::acquire();
    let (Some(ffmpeg), Some(ffprobe)) = (
      paths::resolve_binary("ffmpeg"),
      paths::resolve_binary("ffprobe"),
    ) else {
      assert!(
        std::env::var_os("RIFF_REQUIRE_FFMPEG").is_none(),
        "ffmpeg ou ffprobe introuvable et RIFF_REQUIRE_FFMPEG est défini"
      );
      eprintln!("TEST NON EXÉCUTÉ (compté comme réussi) : ffmpeg ou ffprobe introuvable");
      return None;
    };
    Some(Self {
      dir: TempDir::new(&format!("encode-{name}")),
      _data: data,
      ffmpeg,
      ffprobe,
    })
  }

  fn path(&self, name: &str) -> PathBuf {
    self.dir.join(name)
  }

  fn run(&self, args: &[&str]) {
    let output = StdCommand::new(&self.ffmpeg)
      .args(["-hide_banner", "-loglevel", "error", "-y"])
      .args(args)
      .output()
      .unwrap();
    assert!(
      output.status.success(),
      "{}",
      String::from_utf8_lossy(&output.stderr)
    );
  }

  /// Sinus de 4 s au format de `name`.
  fn sine(&self, name: &str, codec_args: &[&str]) -> PathBuf {
    let out = self.path(name);
    let mut args = vec![
      "-f",
      "lavfi",
      "-i",
      "sine=frequency=440:duration=4:sample_rate=44100",
    ];
    args.extend_from_slice(codec_args);
    let out_text = out.to_string_lossy().into_owned();
    args.push(&out_text);
    self.run(&args);
    out
  }

  /// Pochette JPEG de 640 x 480.
  fn cover(&self) -> PathBuf {
    let out = self.path("cover.jpg");
    let size = format!("color=c=red:s={}x{}", COVER_SIZE.0, COVER_SIZE.1);
    let out_text = out.to_string_lossy().into_owned();
    self.run(&["-f", "lavfi", "-i", &size, "-frames:v", "1", &out_text]);
    out
  }

  fn probe(&self, file: &Path) -> Value {
    let output = StdCommand::new(&self.ffprobe)
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

  /// Empreinte des paquets audio : identique seulement si l'audio n'a pas été ré-encodé.
  fn audio_md5(&self, file: &Path) -> String {
    let output = StdCommand::new(&self.ffmpeg)
      .args(["-hide_banner", "-loglevel", "error", "-i"])
      .arg(file)
      .args(["-map", "0:a:0", "-c", "copy", "-f", "md5", "-"])
      .output()
      .unwrap();
    assert!(
      output.status.success(),
      "{}",
      String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
  }
}

fn stream<'a>(probe: &'a Value, kind: &str) -> &'a Value {
  probe["streams"]
    .as_array()
    .unwrap()
    .iter()
    .find(|stream| stream["codec_type"] == kind)
    .unwrap_or_else(|| panic!("flux {kind} absent : {probe}"))
}

fn tag(probe: &Value, key: &str) -> String {
  let tags = &probe["format"]["tags"];
  tags
    .as_object()
    .unwrap()
    .iter()
    .find(|(name, _)| name.eq_ignore_ascii_case(key))
    .map_or(String::new(), |(_, value)| {
      value.as_str().unwrap().to_string()
    })
}

fn bitrate(probe: &Value) -> u64 {
  stream(probe, "audio")["bit_rate"]
    .as_str()
    .and_then(|value| value.parse().ok())
    .or_else(|| {
      probe["format"]["bit_rate"]
        .as_str()
        .and_then(|value| value.parse().ok())
    })
    .unwrap()
}

fn assert_cover(probe: &Value) {
  let video = stream(probe, "video");
  assert_eq!(
    (
      video["width"].as_u64().unwrap(),
      video["height"].as_u64().unwrap()
    ),
    COVER_SIZE,
    "taille de la pochette"
  );
  assert_eq!(video["disposition"]["attached_pic"], 1, "pochette attachée");
}

#[tokio::test]
async fn embedding_a_cover_in_an_mp3_keeps_the_audio_untouched() {
  let Some(fixture) = Fixture::new("copy") else {
    return;
  };
  let source = fixture.sine("source.mp3", &["-c:a", "libmp3lame", "-b:a", "128k"]);
  let cover = fixture.cover();
  let out = fixture.path("out.mp3");

  let args = cover_embed_args(&source, &cover, "mp3", "320k", "Titre é", "Artiste ü", &out);
  run_ffmpeg(&fixture.ffmpeg, &args).await.unwrap();

  let probe = fixture.probe(&out);
  assert_eq!(stream(&probe, "audio")["codec_name"], "mp3");
  let rate = bitrate(&probe) as f64;
  assert!(
    ((rate - SOURCE_BITRATE as f64) / SOURCE_BITRATE as f64).abs() < BITRATE_TOLERANCE,
    "débit {rate}, source {SOURCE_BITRATE}"
  );
  assert_eq!(
    fixture.audio_md5(&out),
    fixture.audio_md5(&source),
    "audio ré-encodé"
  );
  assert_eq!(tag(&probe, "title"), "Titre é");
  assert_eq!(tag(&probe, "artist"), "Artiste ü");
  assert_cover(&probe);
}

#[tokio::test]
async fn embedding_a_cover_in_a_flac_keeps_the_audio_untouched() {
  let Some(fixture) = Fixture::new("flac") else {
    return;
  };
  let source = fixture.sine("source.flac", &["-c:a", "flac"]);
  let cover = fixture.cover();
  let out = fixture.path("out.flac");

  let args = cover_embed_args(&source, &cover, "flac", "320k", "Titre", "Artiste", &out);
  run_ffmpeg(&fixture.ffmpeg, &args).await.unwrap();

  let probe = fixture.probe(&out);
  assert_eq!(stream(&probe, "audio")["codec_name"], "flac");
  assert_eq!(fixture.audio_md5(&out), fixture.audio_md5(&source));
  assert_eq!(tag(&probe, "title"), "Titre");
  assert_eq!(tag(&probe, "artist"), "Artiste");
  assert_cover(&probe);
}

fn encode_request(
  fixture: &Fixture,
  input: PathBuf,
  format: &str,
  quality: &str,
  thumbnail: String,
) -> EncodeFile {
  EncodeFile {
    input,
    title: "Titre".to_string(),
    artist: "Artiste".to_string(),
    format: format.to_string(),
    quality: quality.to_string(),
    thumbnail,
    custom_dir: fixture.path("sortie").to_string_lossy().into_owned(),
    start: None,
    end: None,
  }
}

#[tokio::test]
async fn encode_file_gives_the_requested_bitrate_tags_and_cover() {
  let Some(fixture) = Fixture::new("encode") else {
    return;
  };
  let wav = fixture.sine("render.wav", &[]);
  let cover = fixture.cover().to_string_lossy().into_owned();
  let core = Core::new(crate::core::CoreConfig::default());
  for (quality, expected) in [
    ("0", 320_000.0),
    ("2", 256_000.0),
    ("4", 192_000.0),
    ("5", 128_000.0),
  ] {
    let done = encode_file(
      &core,
      &encode_request(&fixture, wav.clone(), "mp3", quality, cover.clone()),
    )
    .await
    .unwrap();
    assert_eq!(done.filename, "Artiste - Titre.mp3");
    let probe = fixture.probe(&done.directory.join(&done.filename));
    assert_eq!(stream(&probe, "audio")["codec_name"], "mp3");
    let rate = bitrate(&probe) as f64;
    assert!(
      ((rate - expected) / expected).abs() < BITRATE_TOLERANCE,
      "qualité {quality} : {rate}"
    );
    assert_eq!(tag(&probe, "title"), "Titre");
    assert_eq!(tag(&probe, "artist"), "Artiste");
    assert_cover(&probe);
  }
  assert!(wav.exists(), "le fichier source appartient à l'appelant");
}

#[tokio::test]
async fn encode_file_writes_flac_without_loss() {
  let Some(fixture) = Fixture::new("encode-flac") else {
    return;
  };
  let wav = fixture.sine("render.wav", &[]);
  let core = Core::new(crate::core::CoreConfig::default());
  let done = encode_file(
    &core,
    &encode_request(&fixture, wav, "flac", "0", String::new()),
  )
  .await
  .unwrap();
  let probe = fixture.probe(&done.directory.join(&done.filename));
  assert_eq!(stream(&probe, "audio")["codec_name"], "flac");
  assert_eq!(tag(&probe, "title"), "Titre");
}

#[test]
fn only_the_same_format_can_be_copied() {
  assert!(same_format(Path::new("a/b.MP3"), "mp3"));
  assert!(same_format(Path::new("b.m4a"), "m4a"));
  assert!(!same_format(Path::new("b.opus"), "mp3"));
  assert!(!same_format(Path::new("b"), "mp3"));
}

#[test]
fn the_cover_is_asked_at_the_best_size_first() {
  assert_eq!(
    cover_candidates("https://i.ytimg.com/vi/abc/hqdefault.jpg"),
    [
      "https://i.ytimg.com/vi/abc/maxresdefault.jpg",
      "https://i.ytimg.com/vi/abc/sddefault.jpg",
      "https://i.ytimg.com/vi/abc/hqdefault.jpg",
    ]
  );
  assert_eq!(
    cover_candidates("https://is1-ssl.mzstatic.com/a/100x100bb.jpg"),
    ["https://is1-ssl.mzstatic.com/a/1200x1200bb.jpg"]
  );
  assert_eq!(cover_candidates("C:/a.jpg"), ["C:/a.jpg"]);
}

fn duration(probe: &Value) -> f64 {
  probe["format"]["duration"]
    .as_str()
    .unwrap()
    .parse()
    .unwrap()
}

#[tokio::test]
async fn cutting_a_wav_gives_the_expected_length() {
  let Some(fixture) = Fixture::new("cut-wav") else {
    return;
  };
  let wav = fixture.sine("source.wav", &[]);
  let core = Core::new(crate::core::CoreConfig::default());
  let request = EncodeFile {
    start: Some(1.0),
    end: Some(3.0),
    ..encode_request(&fixture, wav, "wav", "0", String::new())
  };
  let done = encode_file(&core, &request).await.unwrap();
  let probe = fixture.probe(&done.directory.join(&done.filename));
  assert!(
    (duration(&probe) - 2.0).abs() < 0.01,
    "durée {}",
    duration(&probe)
  );
}

#[tokio::test]
async fn cutting_an_mp3_keeps_its_codec_and_bitrate() {
  let Some(fixture) = Fixture::new("cut-mp3") else {
    return;
  };
  let source = fixture.sine("source.mp3", &["-c:a", "libmp3lame", "-b:a", "128k"]);
  let core = Core::new(crate::core::CoreConfig::default());
  let request = EncodeFile {
    start: Some(1.0),
    end: Some(3.0),
    ..encode_request(&fixture, source, "mp3", "0", String::new())
  };
  let done = encode_file(&core, &request).await.unwrap();
  let probe = fixture.probe(&done.directory.join(&done.filename));
  assert!(
    (duration(&probe) - 2.0).abs() < 0.1,
    "durée {}",
    duration(&probe)
  );
  assert_eq!(stream(&probe, "audio")["codec_name"], "mp3");
  let rate = bitrate(&probe) as f64;
  assert!(
    ((rate - SOURCE_BITRATE as f64) / SOURCE_BITRATE as f64).abs() < BITRATE_TOLERANCE,
    "débit {rate}"
  );
}

#[tokio::test]
async fn cutting_an_opus_source_to_mp3_encodes_it() {
  let Some(fixture) = Fixture::new("cut-opus") else {
    return;
  };
  let source = fixture.sine("source.opus", &["-c:a", "libopus"]);
  let core = Core::new(crate::core::CoreConfig::default());
  let request = EncodeFile {
    start: Some(1.0),
    end: Some(3.0),
    ..encode_request(&fixture, source, "mp3", "5", String::new())
  };
  let done = encode_file(&core, &request).await.unwrap();
  let probe = fixture.probe(&done.directory.join(&done.filename));
  assert_eq!(stream(&probe, "audio")["codec_name"], "mp3");
  assert!(
    (duration(&probe) - 2.0).abs() < 0.1,
    "durée {}",
    duration(&probe)
  );
}
