use crate::paths;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use url::Url;

pub const AUDIO_EXT_FILTER: &str = "*.mp3;*.m4a;*.flac;*.wav;*.ogg;*.aac";
pub const MEDIA_EXT_FILTER: &str = "*.jpg;*.jpeg;*.png;*.gif;*.webp;*.svg;*.mp4;*.webm;*.mov";
const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "flac", "wav", "ogg", "aac"];

pub fn path_to_file_url(p: &Path) -> String {
  Url::from_file_path(p)
    .map(|u| u.to_string())
    .unwrap_or_else(|_| format!("file:///{}", p.to_string_lossy().replace('\\', "/")))
}

pub fn is_audio_file(p: &Path) -> bool {
  p.extension()
    .and_then(|e| e.to_str())
    .map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
    .unwrap_or(false)
}

/// Copie les fichiers donnés dans la bibliothèque et renvoie leurs entrées.
pub fn import_audio_files_from(files: Vec<PathBuf>) -> Vec<Value> {
  let lib_dir = paths::library_dir();
  let _ = fs::create_dir_all(&lib_dir);
  let mut out = Vec::new();
  for src in files {
    let Ok(meta) = fs::metadata(&src) else {
      continue;
    };
    if !meta.is_file() {
      continue;
    }
    let ext = src
      .extension()
      .and_then(|e| e.to_str())
      .unwrap_or("mp3")
      .to_ascii_lowercase();
    let base_name = src.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let stem = src
      .file_stem()
      .and_then(|s| s.to_str())
      .unwrap_or_default()
      .to_string();

    let mut hasher = Sha1::new();
    hasher.update(format!("{base_name}{}", meta.len()).as_bytes());
    let hex_part: String = hasher
      .finalize()
      .iter()
      .take(6)
      .map(|b| format!("{b:02x}"))
      .collect();
    let id = format!("local_{hex_part}");
    let file = format!("{id}.{ext}");
    let dest = lib_dir.join(&file);
    if !dest.exists() {
      let _ = fs::copy(&src, &dest);
    }
    out.push(json!({
      "id": id,
      "file": file,
      "ext": ext,
      "name": stem,
    }));
  }
  out
}

fn decode_synchsafe_u32(b: &[u8]) -> usize {
  if b.len() < 4 {
    return 0;
  }
  (((b[0] as usize) & 0x7f) << 21)
    | (((b[1] as usize) & 0x7f) << 14)
    | (((b[2] as usize) & 0x7f) << 7)
    | ((b[3] as usize) & 0x7f)
}

fn decode_id3_text(payload: &[u8]) -> String {
  if payload.is_empty() {
    return String::new();
  }
  let enc = payload[0];
  let raw = &payload[1..];
  let text = match enc {
    0 => raw.iter().map(|&c| c as char).collect::<String>(),
    1 => {
      if raw.len() >= 2 {
        let (le, slice) = if raw[0] == 0xff && raw[1] == 0xfe {
          (true, &raw[2..])
        } else if raw[0] == 0xfe && raw[1] == 0xff {
          (false, &raw[2..])
        } else {
          (true, raw)
        };
        let u16s: Vec<u16> = slice
          .as_chunks::<2>()
          .0
          .iter()
          .map(|&[b0, b1]| {
            if le {
              u16::from_le_bytes([b0, b1])
            } else {
              u16::from_be_bytes([b0, b1])
            }
          })
          .collect();
        String::from_utf16_lossy(&u16s)
      } else {
        String::new()
      }
    }
    2 => {
      let u16s: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[b0, b1]| u16::from_be_bytes([b0, b1]))
        .collect();
      String::from_utf16_lossy(&u16s)
    }
    _ => String::from_utf8_lossy(raw).into_owned(),
  };
  text.trim_matches('\0').trim().to_string()
}

fn extract_apic(payload: &[u8], is_v22: bool) -> Option<(&'static str, Vec<u8>)> {
  if payload.len() < 8 {
    return None;
  }
  let enc = payload[0];
  let mut pos = 1usize;
  let mime: String;
  if is_v22 {
    if pos + 3 > payload.len() {
      return None;
    }
    mime = String::from_utf8_lossy(&payload[pos..pos + 3]).to_ascii_lowercase();
    pos += 3;
  } else {
    let end = payload[pos..].iter().position(|&b| b == 0)? + pos;
    mime = String::from_utf8_lossy(&payload[pos..end]).to_ascii_lowercase();
    pos = end + 1;
  }
  if pos >= payload.len() {
    return None;
  }
  pos += 1; // picture type
  if enc == 1 || enc == 2 {
    while pos + 1 < payload.len() {
      if payload[pos] == 0 && payload[pos + 1] == 0 {
        pos += 2;
        break;
      }
      pos += 2;
    }
  } else if let Some(rel) = payload[pos..].iter().position(|&b| b == 0) {
    pos += rel + 1;
  }
  if pos >= payload.len() {
    return None;
  }
  let img = &payload[pos..];
  if img.len() <= 32 {
    return None;
  }
  let ext = if mime.contains("png") {
    "png"
  } else if mime.contains("webp") {
    "webp"
  } else if mime.contains("gif") {
    "gif"
  } else {
    "jpg"
  };
  Some((ext, img.to_vec()))
}

#[derive(Default)]
struct ParsedAudioTags {
  title: String,
  artist: String,
  album: String,
  year: String,
  track_num: u32,
  cover: Option<(&'static str, Vec<u8>)>,
}

fn parse_id3v2(buf: &[u8]) -> Option<ParsedAudioTags> {
  if buf.len() < 10 || &buf[0..3] != b"ID3" {
    return None;
  }
  let ver = buf[3];
  let flags = buf[5];
  let tag_size = decode_synchsafe_u32(&buf[6..10]);
  let end = (10 + tag_size).min(buf.len());
  let mut offset = 10usize;
  if (flags & 0x40) != 0 && offset + 4 <= end {
    let ext_size = if ver == 4 {
      decode_synchsafe_u32(&buf[offset..offset + 4])
    } else {
      u32::from_be_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
      ]) as usize
    };
    offset += if ver == 4 { ext_size } else { 4 + ext_size };
  }

  let mut out = ParsedAudioTags::default();
  let header_len = if ver == 2 { 6 } else { 10 };

  while offset + header_len <= end {
    let id_slice = if ver == 2 {
      &buf[offset..offset + 3]
    } else {
      &buf[offset..offset + 4]
    };
    if id_slice.iter().all(|&b| b == 0)
      || !id_slice
        .iter()
        .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
      break;
    }
    let id = std::str::from_utf8(id_slice).unwrap_or("");
    let size = if ver == 2 {
      ((buf[offset + 3] as usize) << 16)
        | ((buf[offset + 4] as usize) << 8)
        | (buf[offset + 5] as usize)
    } else if ver == 4 {
      decode_synchsafe_u32(&buf[offset + 4..offset + 8])
    } else {
      u32::from_be_bytes([
        buf[offset + 4],
        buf[offset + 5],
        buf[offset + 6],
        buf[offset + 7],
      ]) as usize
    };
    if size == 0 || offset + header_len + size > end {
      break;
    }
    let payload = &buf[offset + header_len..offset + header_len + size];
    offset += header_len + size;

    match id {
      "TIT2" | "TT2" if out.title.is_empty() => out.title = decode_id3_text(payload),
      "TPE1" | "TP1" if out.artist.is_empty() => out.artist = decode_id3_text(payload),
      "TALB" | "TAL" if out.album.is_empty() => out.album = decode_id3_text(payload),
      "TDRC" | "TYER" | "TYE" if out.year.is_empty() => {
        let y = decode_id3_text(payload);
        out.year = y.chars().take(4).collect();
      }
      "TRCK" | "TRK" if out.track_num == 0 => {
        let trk = decode_id3_text(payload);
        if let Some(first) = trk.split('/').next()
          && let Ok(n) = first.trim().parse::<u32>()
        {
          out.track_num = n;
        }
      }
      "APIC" | "PIC" if out.cover.is_none() => {
        out.cover = extract_apic(payload, ver == 2);
      }
      _ => {}
    }
  }
  Some(out)
}

fn parse_flac(buf: &[u8]) -> Option<ParsedAudioTags> {
  if buf.len() < 8 || &buf[0..4] != b"fLaC" {
    return None;
  }
  let mut out = ParsedAudioTags::default();
  let mut offset = 4usize;
  while offset + 4 <= buf.len() {
    let header = buf[offset];
    let is_last = (header & 0x80) != 0;
    let block_type = header & 0x7f;
    let block_len = ((buf[offset + 1] as usize) << 16)
      | ((buf[offset + 2] as usize) << 8)
      | (buf[offset + 3] as usize);
    offset += 4;
    if offset + block_len > buf.len() {
      break;
    }
    let block = &buf[offset..offset + block_len];
    offset += block_len;

    if block_type == 4 && block.len() >= 8 {
      let vendor_len = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as usize;
      let mut p = 4 + vendor_len;
      if p + 4 <= block.len() {
        let count =
          u32::from_le_bytes([block[p], block[p + 1], block[p + 2], block[p + 3]]) as usize;
        p += 4;
        for _ in 0..count {
          if p + 4 > block.len() {
            break;
          }
          let clen =
            u32::from_le_bytes([block[p], block[p + 1], block[p + 2], block[p + 3]]) as usize;
          p += 4;
          if p + clen > block.len() {
            break;
          }
          let entry = String::from_utf8_lossy(&block[p..p + clen]);
          p += clen;
          if let Some((k, v)) = entry.split_once('=') {
            let key = k.trim().to_ascii_uppercase();
            let val = v.trim().to_string();
            match key.as_str() {
              "TITLE" if out.title.is_empty() => out.title = val,
              "ARTIST" if out.artist.is_empty() => out.artist = val,
              "ALBUMARTIST" if out.artist.is_empty() => out.artist = val,
              "ALBUM" if out.album.is_empty() => out.album = val,
              "DATE" | "YEAR" if out.year.is_empty() => {
                out.year = val.chars().take(4).collect();
              }
              "TRACKNUMBER" if out.track_num == 0 => {
                if let Some(first) = val.split('/').next()
                  && let Ok(n) = first.trim().parse::<u32>()
                {
                  out.track_num = n;
                }
              }
              _ => {}
            }
          }
        }
      }
    } else if block_type == 6 && out.cover.is_none() && block.len() > 32 {
      let mut p = 4usize;
      if p + 4 <= block.len() {
        let mime_len =
          u32::from_be_bytes([block[p], block[p + 1], block[p + 2], block[p + 3]]) as usize;
        p += 4;
        if p + mime_len + 4 <= block.len() {
          let mime = String::from_utf8_lossy(&block[p..p + mime_len]).to_ascii_lowercase();
          p += mime_len;
          let desc_len =
            u32::from_be_bytes([block[p], block[p + 1], block[p + 2], block[p + 3]]) as usize;
          p += 4 + desc_len + 16;
          if p + 4 <= block.len() {
            let data_len =
              u32::from_be_bytes([block[p], block[p + 1], block[p + 2], block[p + 3]]) as usize;
            p += 4;
            if data_len > 32 && p + data_len <= block.len() {
              let ext = if mime.contains("png") {
                "png"
              } else if mime.contains("webp") {
                "webp"
              } else {
                "jpg"
              };
              out.cover = Some((ext, block[p..p + data_len].to_vec()));
            }
          }
        }
      }
    }

    if is_last {
      break;
    }
  }
  Some(out)
}

fn find_folder_cover(track_path: &Path) -> Option<PathBuf> {
  let dir = track_path.parent()?;
  let stem = track_path.file_stem()?.to_str()?;
  let candidates = [
    format!("{stem}.jpg"),
    format!("{stem}.png"),
    format!("{stem}.webp"),
    "cover.jpg".to_string(),
    "cover.jpeg".to_string(),
    "cover.png".to_string(),
    "cover.webp".to_string(),
    "folder.jpg".to_string(),
    "folder.png".to_string(),
    "front.jpg".to_string(),
    "front.png".to_string(),
    "album.jpg".to_string(),
    "album.png".to_string(),
    "artwork.jpg".to_string(),
    "artwork.png".to_string(),
  ];
  for name in candidates {
    let p = dir.join(name);
    if p.is_file() {
      return Some(p);
    }
  }
  None
}

pub fn extract_local_track_metadata(file_path: &Path) -> Option<Value> {
  let meta = fs::metadata(file_path).ok()?;
  if !meta.is_file() {
    return None;
  }
  let size = meta.len();
  let mtime_ms = meta
    .modified()
    .ok()
    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0);

  let read_len = size.min(4 * 1024 * 1024) as usize;
  let mut buf = vec![0u8; read_len];
  if let Ok(mut f) = File::open(file_path) {
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
  } else {
    buf.clear();
  }

  let tags = parse_id3v2(&buf)
    .or_else(|| parse_flac(&buf))
    .unwrap_or_default();

  let mut title = tags.title;
  let mut artist = tags.artist;
  let album = tags.album;
  let year = tags.year;
  let track_num = tags.track_num;

  let mut cover_url = String::new();
  if let Some((ext, bytes)) = tags.cover {
    let covers_dir = paths::local_covers_dir();
    if fs::create_dir_all(&covers_dir).is_ok() {
      let mut hasher = Sha1::new();
      hasher.update(format!("{}:{}:{}", file_path.to_string_lossy(), mtime_ms, size).as_bytes());
      let hash_hex: String = hasher
        .finalize()
        .iter()
        .take(10)
        .map(|b| format!("{b:02x}"))
        .collect();
      let cover_path = covers_dir.join(format!("{hash_hex}.{ext}"));
      if !cover_path.exists() {
        let _ = fs::write(&cover_path, bytes);
      }
      if cover_path.is_file() {
        cover_url = path_to_file_url(&cover_path);
      }
    }
  }

  if cover_url.is_empty()
    && let Some(folder_cover) = find_folder_cover(file_path)
  {
    cover_url = path_to_file_url(&folder_cover);
  }

  let stem = file_path
    .file_stem()
    .and_then(|s| s.to_str())
    .unwrap_or("Unknown")
    .trim();
  if title.is_empty() {
    if let Some((lhs, rhs)) = stem.split_once(" - ") {
      if artist.is_empty() {
        artist = lhs.trim().to_string();
      }
      title = rhs.trim().to_string();
    } else {
      title = stem.to_string();
    }
  }
  if artist.is_empty() {
    artist = "Unknown Artist".to_string();
  }

  let mut id_hasher = Sha1::new();
  id_hasher.update(file_path.to_string_lossy().as_bytes());
  let file_hash: String = id_hasher
    .finalize()
    .iter()
    .take(16)
    .map(|b| format!("{b:02x}"))
    .collect();

  let file_name = file_path
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or_default()
    .to_string();
  let file_url = path_to_file_url(file_path);

  Some(json!({
    "id": format!("local_{file_hash}"),
    "path": file_path.to_string_lossy().to_string(),
    "url": file_url,
    "fileUrl": file_url,
    "name": file_name,
    "size": size,
    "mtimeMs": mtime_ms,
    "title": title,
    "artist": artist,
    "album": album,
    "year": year,
    "trackNum": track_num,
    "trackNumber": track_num,
    "duration": 0,
    "thumbnail": cover_url,
    "cover": cover_url,
    "coverUrl": cover_url,
    "platform": "local",
  }))
}

/// Lit les tags des fichiers donnés.
pub fn local_files_metadata(files: Vec<PathBuf>) -> Vec<Value> {
  files
    .into_iter()
    .filter_map(|p| extract_local_track_metadata(&p))
    .collect()
}

fn collect_audio_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
  if depth > 5 || out.len() >= 2000 {
    return;
  }
  let Ok(entries) = fs::read_dir(dir) else {
    return;
  };
  for entry in entries.filter_map(|e| e.ok()) {
    if out.len() >= 2000 {
      break;
    }
    let name = entry.file_name();
    if name.to_string_lossy().starts_with('.') {
      continue;
    }
    let full = entry.path();
    let Ok(ft) = entry.file_type() else {
      continue;
    };
    if ft.is_dir() {
      collect_audio_files(&full, depth + 1, out);
    } else if ft.is_file() && is_audio_file(&full) {
      out.push(full);
    }
  }
}

/// Parcourt les dossiers donnés et lit les tags des fichiers audio trouvés.
pub fn local_folders_metadata(dirs: Vec<PathBuf>) -> Vec<Value> {
  if dirs.is_empty() {
    return Vec::new();
  }
  let mut all_files = Vec::new();
  for d in dirs {
    collect_audio_files(&d, 0, &mut all_files);
  }
  let mut seen = HashSet::new();
  let mut out = Vec::new();
  for f in all_files {
    if seen.insert(f.clone())
      && let Some(meta) = extract_local_track_metadata(&f)
    {
      out.push(meta);
    }
  }
  out
}

pub fn rescan_local_metadata(paths: Vec<String>) -> Vec<Value> {
  let mut out = Vec::new();
  for p in paths.into_iter().take(3000) {
    let pb = PathBuf::from(&p);
    if pb.is_file()
      && let Some(meta) = extract_local_track_metadata(&pb)
    {
      out.push(meta);
    }
  }
  out
}

/// Nom de fichier proposé et contenu de l'export des playlists.
#[must_use]
pub fn playlists_export(payload: &Value) -> (String, String) {
  let default_name = payload
    .get("defaultName")
    .and_then(Value::as_str)
    .filter(|s| !s.trim().is_empty())
    .unwrap_or("playlists.json");
  let content = if let Some(c) = payload.get("content").and_then(Value::as_str) {
    c.to_string()
  } else if let Some(d) = payload.get("data") {
    serde_json::to_string_pretty(d).unwrap_or_default()
  } else {
    serde_json::to_string_pretty(payload).unwrap_or_default()
  };
  (default_name.to_string(), content)
}

/// Écrit l'export des playlists à l'emplacement choisi.
#[must_use]
pub fn write_playlists_export(file_path: &Path, content: &str) -> Value {
  match fs::write(file_path, content) {
    Ok(()) => json!({
      "success": true,
      "canceled": false,
      "filePath": file_path.to_string_lossy().to_string(),
    }),
    Err(err) => json!({
      "success": false,
      "canceled": false,
      "error": err.to_string(),
    }),
  }
}

fn rfacc_key() -> [u8; 32] {
  let mut hasher = Sha256::new();
  hasher.update(b"riff-account-key-v1");
  let digest = hasher.finalize();
  let mut key = [0u8; 32];
  key.copy_from_slice(&digest);
  key
}

fn encrypt_account(data_obj: &Value) -> Result<Vec<u8>, String> {
  let json_str = if let Some(s) = data_obj.as_str() {
    s.to_string()
  } else {
    serde_json::to_string(data_obj).map_err(|e| e.to_string())?
  };

  let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
  encoder
    .write_all(json_str.as_bytes())
    .map_err(|e| e.to_string())?;
  let compressed = encoder.finish().map_err(|e| e.to_string())?;

  let mut iv = [0u8; 12];
  getrandom::fill(&mut iv).map_err(|e| e.to_string())?;

  let key = rfacc_key();
  let cipher = Aes256Gcm::new((&key).into());
  let nonce = Nonce::from(iv);
  let ct_with_tag = cipher
    .encrypt(&nonce, compressed.as_slice())
    .map_err(|e| e.to_string())?;
  if ct_with_tag.len() < 16 {
    return Err("Encryption failed".to_string());
  }
  let split_at = ct_with_tag.len() - 16;
  let (encrypted, tag) = ct_with_tag.split_at(split_at);

  let mut out = Vec::with_capacity(4 + 12 + 16 + encrypted.len());
  out.extend_from_slice(b"RFAC");
  out.extend_from_slice(&iv);
  out.extend_from_slice(tag);
  out.extend_from_slice(encrypted);
  Ok(out)
}

fn decrypt_account(buffer: &[u8]) -> Result<Value, String> {
  if buffer.len() < 32 {
    return Err("Invalid account file".to_string());
  }
  if &buffer[0..4] != b"RFAC" {
    return Err("Invalid file header".to_string());
  }
  let iv = &buffer[4..16];
  let tag = &buffer[16..32];
  let encrypted = &buffer[32..];

  let mut ct_with_tag = Vec::with_capacity(encrypted.len() + 16);
  ct_with_tag.extend_from_slice(encrypted);
  ct_with_tag.extend_from_slice(tag);

  let key = rfacc_key();
  let cipher = Aes256Gcm::new((&key).into());
  let nonce = Nonce::try_from(iv).map_err(|_| "Invalid or corrupted account file".to_string())?;
  let decrypted = cipher
    .decrypt(&nonce, ct_with_tag.as_slice())
    .map_err(|_| "Invalid or corrupted account file".to_string())?;

  let mut decoder = DeflateDecoder::new(decrypted.as_slice());
  let mut decompressed = String::new();
  decoder
    .read_to_string(&mut decompressed)
    .map_err(|e| e.to_string())?;
  serde_json::from_str::<Value>(&decompressed).map_err(|e| e.to_string())
}

/// Nom de fichier proposé pour l'export du compte.
#[must_use]
pub fn account_export_name(payload: &Value) -> String {
  payload
    .get("defaultName")
    .and_then(Value::as_str)
    .filter(|s| !s.trim().is_empty())
    .unwrap_or("riff-account.rfacc")
    .to_string()
}

/// Chiffre le compte et l'écrit à l'emplacement choisi.
#[must_use]
pub fn write_account_export(file_path: &Path, payload: &Value) -> Value {
  match encrypt_account(payload)
    .and_then(|bytes| fs::write(file_path, bytes).map_err(|e| e.to_string()))
  {
    Ok(()) => json!({
      "success": true,
      "canceled": false,
      "filePath": file_path.to_string_lossy().to_string(),
    }),
    Err(err) => json!({
      "success": false,
      "canceled": false,
      "error": err,
    }),
  }
}

/// Lit un fichier de compte (`.rfacc` chiffré ou ancien JSON).
#[must_use]
pub fn read_account_file(file_path: &Path) -> Value {
  let raw = match fs::read(file_path) {
    Ok(b) => b,
    Err(err) => {
      return json!({
        "success": false,
        "canceled": false,
        "error": err.to_string()
      });
    }
  };

  let parsed = if raw.len() >= 4 && &raw[0..4] == b"RFAC" {
    decrypt_account(&raw)
  } else {
    String::from_utf8(raw)
      .map_err(|e| e.to_string())
      .and_then(|s| serde_json::from_str::<Value>(&s).map_err(|e| e.to_string()))
  };

  match parsed {
    Ok(data) => json!({
      "success": true,
      "canceled": false,
      "data": data,
      "filePath": file_path.to_string_lossy().to_string(),
    }),
    Err(err) => json!({
      "success": false,
      "canceled": false,
      "error": err,
    }),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn unsupported_containers_are_rejected() {
    for name in ["a.opus", "b.webm", "c.wma"] {
      assert!(!is_audio_file(Path::new(name)), "{name}");
    }
    for name in ["a.mp3", "b.m4a", "c.flac", "d.wav", "e.ogg", "f.aac"] {
      assert!(is_audio_file(Path::new(name)), "{name}");
    }
  }

  /// Fichier `.rfacc` produit avec aes-gcm 0.10.3 : le format disque ne doit pas changer.
  const ACCOUNT_REFERENCE_HEX: &str = "524641432402777f9ce342f31c49b2cecb8e5abb74b3bd1ac085d2d3644eca7859b2a427e9333e3e1e9ab99b945f667ec350b14d94b8957255749a74a4e751bf2e43";

  fn hex_decode(hex: &str) -> Vec<u8> {
    (0..hex.len())
      .step_by(2)
      .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
      .collect()
  }

  #[test]
  fn account_file_written_by_previous_cipher_version_still_decrypts() {
    let buffer = hex_decode(ACCOUNT_REFERENCE_HEX);
    let value = decrypt_account(&buffer).unwrap();
    assert_eq!(value, json!({"name": "reference", "n": [1, 2, 3]}));
  }
}
