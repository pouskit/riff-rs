//! Décodage par `symphonia` : formats, déplacement à l'échantillon près, sortie stéréo entrelacée.

use std::time::Duration;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase, Timestamp};

use crate::AudioError;
use crate::source::{self, SegmentReader, Segmented, Source};

fn decode_error(err: impl std::fmt::Display) -> AudioError {
  AudioError::Decode(err.to_string())
}

/// Lecteur de titre : produit des blocs `f32` stéréo entrelacés.
pub struct Decoder {
  format: Box<dyn FormatReader>,
  decoder: Box<dyn AudioDecoder>,
  track_id: u32,
  time_base: Option<TimeBase>,
  sample_rate: u32,
  duration: Option<Duration>,
  /// Après un déplacement : horodatage à partir duquel garder les échantillons.
  keep_from: Option<Timestamp>,
  scratch: Vec<f32>,
  /// Titre en segments : `symphonia` ne voit qu'un segment à la fois, d'où les trois champs.
  segmented: Option<Segmented>,
  /// Début, dans le titre, du segment lu par `format`.
  offset: Duration,
  /// Reconstruction paresseuse du lecteur à cet instant, avant la prochaine lecture.
  reopen_at: Option<Duration>,
  /// Déplacement tombé après le dernier échantillon d'un titre segmenté : plus rien à lire.
  ended: bool,
}

fn probe(media: Box<dyn MediaSource>, hint: &Hint) -> Result<Box<dyn FormatReader>, AudioError> {
  let stream = MediaSourceStream::new(media, Default::default());
  symphonia::default::get_probe()
    .probe(
      hint,
      stream,
      FormatOptions::default(),
      MetadataOptions::default(),
    )
    .map_err(decode_error)
}

impl Decoder {
  /// # Errors
  /// Source illisible, format inconnu ou absence de piste audio.
  pub fn open(source: &Source) -> Result<Self, AudioError> {
    let (media, hint) = source::open(source)?;
    let format = probe(media, &hint)?;
    let track = format
      .default_track(TrackType::Audio)
      .ok_or_else(|| AudioError::Decode("pas de piste audio".into()))?;
    let params = track
      .codec_params
      .as_ref()
      .and_then(|params| params.audio())
      .ok_or_else(|| AudioError::Decode("paramètres audio absents".into()))?;
    let sample_rate = params
      .sample_rate
      .ok_or_else(|| AudioError::Decode("fréquence inconnue".into()))?;
    let decoder = symphonia::default::get_codecs()
      .make_audio_decoder(params, &AudioDecoderOptions::default())
      .map_err(decode_error)?;
    let time_base = track.time_base;
    // La durée d'un segment ne dit rien du titre : `symphonia` ne lit que le premier `sidx`.
    let segmented = match source {
      Source::Segmented(segmented) => Some(segmented.clone()),
      _ => None,
    };
    let duration = match (time_base, track.duration, track.num_frames) {
      (Some(base), Some(ticks), _) => base.calc_duration(ticks).map(time_to_duration),
      (_, _, Some(frames)) => Some(Duration::from_secs_f64(
        frames as f64 / f64::from(sample_rate),
      )),
      _ => None,
    };
    let duration = segmented
      .as_ref()
      .map_or(duration, |seg| Some(seg.duration));
    let track_id = track.id;
    Ok(Self {
      format,
      decoder,
      track_id,
      time_base,
      sample_rate,
      duration,
      keep_from: None,
      scratch: Vec::new(),
      reopen_at: segmented.as_ref().map(|_| Duration::ZERO),
      segmented,
      offset: Duration::ZERO,
      ended: false,
    })
  }

  /// Repart du segment qui contient `at`, sur un lecteur de flux neuf.
  fn reopen_segmented(&mut self, at: Duration) -> Result<(), AudioError> {
    let Some(segmented) = self.segmented.clone() else {
      return Ok(());
    };
    let index = segmented.segment_at(at);
    let mut hint = Hint::new();
    hint.with_extension("mp4");
    self.format = probe(
      Box::new(SegmentReader::new(segmented.clone(), index)),
      &hint,
    )?;
    self.offset = segmented.starts[index];
    self.decoder.reset();
    self.keep_from = None;
    let inside = at.saturating_sub(self.offset);
    if !inside.is_zero()
      && let Err(err) = self.seek_format(inside)
    {
      // La somme des `EXTINF` dépasse l'audio réel d'une trame : un déplacement en toute fin
      // tombe hors du dernier segment, ce qui est la fin du titre et non une erreur.
      if index + 1 < segmented.starts.len() {
        return Err(err);
      }
      tracing::debug!(error = %err, "déplacement après la fin du dernier segment");
      self.ended = true;
    }
    Ok(())
  }

  pub fn sample_rate(&self) -> u32 {
    self.sample_rate
  }

  pub fn duration(&self) -> Option<Duration> {
    self.duration
  }

  /// Ajoute un bloc à `out` et renvoie sa position, ou `None` en fin de titre.
  ///
  /// # Errors
  /// Erreur de lecture non récupérable.
  pub fn next_block(&mut self, out: &mut Vec<f32>) -> Result<Option<Duration>, AudioError> {
    if let Some(at) = self.reopen_at.take() {
      self.reopen_segmented(at)?;
    }
    if self.ended {
      return Ok(None);
    }
    loop {
      let packet = match self.format.next_packet() {
        Ok(Some(packet)) => packet,
        Ok(None) => return Ok(None),
        Err(SymphoniaError::ResetRequired) => return Ok(None),
        Err(err) => return Err(decode_error(err)),
      };
      if packet.track_id != self.track_id {
        continue;
      }
      let buffer = match self.decoder.decode(&packet) {
        Ok(buffer) => buffer,
        Err(SymphoniaError::DecodeError(err)) => {
          tracing::warn!(error = err, "paquet audio ignoré");
          continue;
        }
        Err(SymphoniaError::IoError(err)) => {
          tracing::warn!(error = %err, "paquet audio ignoré");
          continue;
        }
        Err(err) => return Err(decode_error(err)),
      };
      let rate = buffer.spec().rate();
      let ticks_per_frame = ticks_per_frame(self.time_base, rate);
      // Durée utile du paquet : le décodeur AAC ne retire pas le remplissage de fin d'encodeur.
      let valid = (packet.dur.get() as f64 / ticks_per_frame).round() as usize;
      let frames = if valid > 0 {
        buffer.frames().min(valid)
      } else {
        buffer.frames()
      };
      if frames == 0 {
        continue;
      }
      let channels = buffer.spec().channels().count().max(1);
      self.scratch.resize(buffer.samples_interleaved(), 0.0);
      buffer.copy_to_slice_interleaved(self.scratch.as_mut_slice());

      let mut skip = 0usize;
      let mut pts = packet.pts;
      if let Some(keep_from) = self.keep_from {
        let packet_end = packet.pts.get() as f64 + frames as f64 * ticks_per_frame;
        if packet_end <= keep_from.get() as f64 {
          continue;
        }
        let ahead = keep_from.get().saturating_sub(packet.pts.get()).max(0);
        skip = ((ahead as f64 / ticks_per_frame).round() as usize).min(frames);
        pts = Timestamp::new(packet.pts.get().max(keep_from.get()));
        self.keep_from = None;
      }
      append_stereo(
        &self.scratch[skip * channels..frames * channels],
        channels,
        out,
      );
      return Ok(Some(self.position_of(pts, rate)));
    }
  }

  fn position_of(&self, pts: Timestamp, rate: u32) -> Duration {
    let local = match self.time_base {
      Some(base) => base.calc_time(pts).map_or(Duration::ZERO, time_to_duration),
      None => Duration::from_secs_f64(pts.get().max(0) as f64 / f64::from(rate)),
    };
    self.offset + local
  }

  /// # Errors
  /// Format sans déplacement possible ou position hors du titre.
  pub fn seek(&mut self, to: Duration) -> Result<(), AudioError> {
    if let Some(segmented) = &self.segmented {
      if to > segmented.duration {
        return Err(AudioError::Decode("position hors du titre".into()));
      }
      self.reopen_at = Some(to);
      self.ended = false;
      return Ok(());
    }
    self.seek_format(to)
  }

  fn seek_format(&mut self, to: Duration) -> Result<(), AudioError> {
    let time = Time::try_from_secs_f64(to.as_secs_f64())
      .ok_or_else(|| AudioError::Decode("position invalide".into()))?;
    let seeked = self
      .format
      .seek(
        SeekMode::Accurate,
        SeekTo::Time {
          time,
          track_id: Some(self.track_id),
        },
      )
      .map_err(decode_error)?;
    self.decoder.reset();
    self.keep_from = Some(seeked.required_ts);
    Ok(())
  }
}

fn time_to_duration(time: Time) -> Duration {
  Duration::from_secs_f64(time.as_secs_f64().max(0.0))
}

fn ticks_per_frame(time_base: Option<TimeBase>, rate: u32) -> f64 {
  time_base.map_or(1.0, |base| {
    f64::from(base.denom.get()) / (f64::from(base.numer.get()) * f64::from(rate))
  })
}

/// Mono dupliqué, stéréo copié, au-delà : les deux premiers canaux (voir `NOTES.md`).
fn append_stereo(samples: &[f32], channels: usize, out: &mut Vec<f32>) {
  match channels {
    1 => out.extend(samples.iter().flat_map(|v| [*v, *v])),
    2 => out.extend_from_slice(samples),
    _ => out.extend(
      samples
        .chunks_exact(channels)
        .flat_map(|frame| [frame[0], frame[1]]),
    ),
  }
}

#[cfg(test)]
pub(crate) mod test_support {
  use crate::source::Source;
  use std::io::{Cursor, Read};
  use std::path::PathBuf;
  use std::sync::Arc;
  use std::time::Duration;

  /// Fichier WAV 16 bits généré par le test : l'échantillon `i` vaut `i` (rampe), pour vérifier
  /// un déplacement à l'échantillon près.
  pub(crate) fn ramp_wav(rate: u32, channels: u16, frames: u32) -> Vec<u8> {
    let data_size = frames * u32::from(channels) * 2;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for frame in 0..frames {
      for _ in 0..channels {
        bytes.extend_from_slice(&((frame % 32_768) as i16).to_le_bytes());
      }
    }
    bytes
  }

  pub(crate) fn data_bytes(name: &str) -> Vec<u8> {
    std::fs::read(
      PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name),
    )
    .expect("fixture")
  }

  /// Segments HLS de `tests/data/hls`, avec journal des requêtes.
  pub(crate) struct FixtureSegments {
    pub(crate) init: Vec<u8>,
    segments: Vec<Vec<u8>>,
    pub(crate) calls: std::sync::Mutex<Vec<usize>>,
  }

  impl crate::source::SegmentFetcher for FixtureSegments {
    fn fetch(&self, index: usize) -> std::io::Result<Box<dyn Read + Send>> {
      self.calls.lock().expect("verrou").push(index);
      Ok(Box::new(Cursor::new(self.segments[index].clone())))
    }
  }

  /// Durées EXTINF de `tone.m3u8`.
  pub(crate) const EXTINF: [f64; 4] = [1.021678, 0.998458, 0.998458, 0.023220];

  pub(crate) fn hls_fixture() -> (Source, Arc<FixtureSegments>) {
    let fetcher = Arc::new(FixtureSegments {
      init: data_bytes("hls/init.mp4"),
      segments: (0..4)
        .map(|i| data_bytes(&format!("hls/seg{i}.m4s")))
        .collect(),
      calls: std::sync::Mutex::new(Vec::new()),
    });
    let mut starts = Vec::new();
    let mut total = 0.0;
    for duration in EXTINF {
      starts.push(Duration::from_secs_f64(total));
      total += duration;
    }
    let source = Source::Segmented(crate::source::Segmented {
      init: fetcher.init.clone().into(),
      starts: starts.into(),
      duration: Duration::from_secs_f64(total),
      fetcher: fetcher.clone(),
    });
    (source, fetcher)
  }
}

#[cfg(test)]
mod tests {
  use super::test_support::{FixtureSegments, data_bytes, hls_fixture, ramp_wav};
  use super::*;
  use crate::source::RangeFetcher;
  use std::io::{Cursor, Read};
  use std::path::PathBuf;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  const FORMATS: [&str; 4] = ["tone.flac", "tone.mp3", "tone.ogg", "tone.m4a"];

  fn data(name: &str) -> Source {
    Source::File(
      PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name),
    )
  }

  fn decode_all(decoder: &mut Decoder) -> Vec<f32> {
    let mut out = Vec::new();
    while decoder.next_block(&mut out).expect("décodage").is_some() {}
    out
  }

  fn sample_index(value: f32) -> i32 {
    (value * 32_768.0).round() as i32
  }

  #[test]
  fn wav_is_decoded_with_its_properties() {
    let mut decoder = Decoder::open(&Source::from(ramp_wav(44_100, 2, 44_100))).expect("ouverture");
    assert_eq!(decoder.sample_rate(), 44_100);
    assert_eq!(decoder.duration(), Some(Duration::from_secs(1)));
    let out = decode_all(&mut decoder);
    assert_eq!(out.len(), 88_200);
    assert_eq!(sample_index(out[2000]), 1000);
    assert_eq!(sample_index(out[2001]), 1000);
  }

  #[test]
  fn mono_is_duplicated_to_stereo() {
    let mut decoder = Decoder::open(&Source::from(ramp_wav(48_000, 1, 4800))).expect("ouverture");
    let out = decode_all(&mut decoder);
    assert_eq!(out.len(), 9600);
    assert_eq!(out[200], out[201]);
    assert_eq!(sample_index(out[200]), 100);
  }

  #[test]
  fn seek_on_wav_is_sample_accurate() {
    let mut decoder = Decoder::open(&Source::from(ramp_wav(48_000, 2, 48_000))).expect("ouverture");
    for (to_ms, frame) in [(500u64, 24_000), (123, 5904), (900, 43_200), (0, 0)] {
      decoder
        .seek(Duration::from_millis(to_ms))
        .expect("déplacement");
      let mut block = Vec::new();
      let position = decoder
        .next_block(&mut block)
        .expect("décodage")
        .expect("bloc");
      assert_eq!(sample_index(block[0]), frame % 32_768, "à {to_ms} ms");
      let got_ms = position.as_secs_f64() * 1000.0;
      assert!(
        (got_ms - to_ms as f64).abs() < 0.05,
        "position {got_ms} ms à {to_ms} ms"
      );
    }
  }

  #[test]
  fn compressed_formats_decode_to_about_one_second_of_tone() {
    for name in FORMATS {
      let mut decoder = Decoder::open(&data(name)).unwrap_or_else(|e| panic!("{name} : {e}"));
      assert_eq!(decoder.sample_rate(), 44_100, "{name}");
      let out = decode_all(&mut decoder);
      let frames = out.len() / 2;
      assert!(
        (frames as i64 - 44_100).abs() < 1200,
        "{name} : {frames} échantillons"
      );
      let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
      assert!(peak > 0.05, "{name} : crête {peak}");
      let middle: Vec<f32> = out[40_000..60_000].iter().step_by(2).copied().collect();
      let crossings = middle
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
      let freq = crossings as f64 / (middle.len() as f64 / 44_100.0);
      assert!((freq - 1000.0).abs() < 20.0, "{name} : {freq} Hz");
    }
  }

  #[test]
  fn seek_on_compressed_formats_lands_on_the_requested_frame() {
    for name in FORMATS {
      let mut reference = Decoder::open(&data(name)).expect("ouverture");
      let total = decode_all(&mut reference).len() / 2;
      let mut decoder = Decoder::open(&data(name)).expect("ouverture");
      decoder
        .seek(Duration::from_millis(500))
        .expect("déplacement");
      let rest = decode_all(&mut decoder).len() / 2;
      let want = total - 22_050;
      assert!(
        (rest as i64 - want as i64).abs() <= 1,
        "{name} : {rest} au lieu de {want}"
      );
    }
  }

  struct MemoryFetcher {
    bytes: Vec<u8>,
    calls: AtomicUsize,
  }

  impl RangeFetcher for MemoryFetcher {
    fn fetch(&self, start: u64, end: Option<u64>) -> std::io::Result<Box<dyn Read + Send>> {
      self.calls.fetch_add(1, Ordering::Relaxed);
      let end = end.map_or(self.bytes.len(), |e| e as usize);
      Ok(Box::new(Cursor::new(
        self.bytes[start as usize..end].to_vec(),
      )))
    }

    fn len(&self) -> Option<u64> {
      Some(self.bytes.len() as u64)
    }
  }

  #[test]
  fn http_source_reads_and_seeks_through_the_fetcher() {
    let fetcher = Arc::new(MemoryFetcher {
      bytes: ramp_wav(48_000, 2, 48_000),
      calls: AtomicUsize::new(0),
    });
    let mut decoder = Decoder::open(&Source::Http(fetcher.clone())).expect("ouverture");
    decoder
      .seek(Duration::from_millis(500))
      .expect("déplacement");
    let mut block = Vec::new();
    decoder
      .next_block(&mut block)
      .expect("décodage")
      .expect("bloc");
    assert_eq!(sample_index(block[0]), 24_000);
    assert!(
      fetcher.calls.load(Ordering::Relaxed) >= 2,
      "une requête par déplacement"
    );
  }

  /// Enregistre l'octet de départ de chaque requête.
  struct LoggingFetcher {
    bytes: Vec<u8>,
    starts: std::sync::Mutex<Vec<u64>>,
  }

  impl LoggingFetcher {
    fn new(bytes: Vec<u8>) -> Arc<Self> {
      Arc::new(Self {
        bytes,
        starts: std::sync::Mutex::new(Vec::new()),
      })
    }

    fn starts(&self) -> Vec<u64> {
      self.starts.lock().expect("verrou").clone()
    }
  }

  impl RangeFetcher for LoggingFetcher {
    fn fetch(&self, start: u64, end: Option<u64>) -> std::io::Result<Box<dyn Read + Send>> {
      self.starts.lock().expect("verrou").push(start);
      let end = end.map_or(self.bytes.len(), |e| e as usize);
      Ok(Box::new(Cursor::new(
        self.bytes[start as usize..end].to_vec(),
      )))
    }

    fn len(&self) -> Option<u64> {
      Some(self.bytes.len() as u64)
    }
  }

  #[test]
  fn two_track_mp4_after_seek_needs_few_requests() {
    let bytes = data_bytes("tone_video.mp4");
    let reference = {
      let mut decoder = Decoder::open(&Source::from(bytes.clone())).expect("ouverture");
      decoder.seek(Duration::from_secs(2)).expect("déplacement");
      decode_all(&mut decoder)
    };
    assert!(reference.len() > 100_000, "fixture mal lue");
    let fetcher = LoggingFetcher::new(bytes);
    let mut decoder = Decoder::open(&Source::Http(fetcher.clone())).expect("ouverture");
    decoder.seek(Duration::from_secs(2)).expect("déplacement");
    let out = decode_all(&mut decoder);
    assert_eq!(out, reference, "échantillons décodés différents");
    let starts = fetcher.starts();
    eprintln!("mp4 deux pistes : {} requêtes {starts:?}", starts.len());
    assert!(starts.len() <= 3, "{} requêtes : {starts:?}", starts.len());
  }

  fn hls_reference() -> Vec<f32> {
    let mut bytes = data_bytes("hls/init.mp4");
    for i in 0..4 {
      bytes.extend(data_bytes(&format!("hls/seg{i}.m4s")));
    }
    let mut decoder = Decoder::open(&Source::from(bytes)).expect("ouverture");
    decode_all(&mut decoder)
  }

  fn hls_calls(fetcher: &FixtureSegments) -> Vec<usize> {
    let mut calls = fetcher.calls.lock().expect("verrou").clone();
    calls.sort_unstable();
    calls.dedup();
    calls
  }

  #[test]
  fn segmented_open_makes_no_request_and_reports_the_sum_of_durations() {
    let (source, fetcher) = hls_fixture();
    let decoder = Decoder::open(&source).expect("ouverture");
    assert_eq!(decoder.sample_rate(), 44_100);
    let secs = decoder.duration().expect("durée").as_secs_f64();
    assert!((secs - 3.041_814).abs() < 1e-6, "{secs} s");
    assert!(hls_calls(&fetcher).is_empty());
  }

  #[test]
  fn segmented_decodes_like_the_concatenated_file_with_growing_positions() {
    let reference = hls_reference();
    assert!(reference.len() > 100_000, "fixture mal lue");
    let (source, _) = hls_fixture();
    let mut decoder = Decoder::open(&source).expect("ouverture");
    let mut out = Vec::new();
    let mut last = Duration::ZERO;
    while let Some(position) = decoder.next_block(&mut out).expect("décodage") {
      assert!(position >= last, "position décroissante");
      last = position;
    }
    assert_eq!(out, reference);
    assert!(
      last > Duration::from_secs(2),
      "décalage non appliqué : {last:?}"
    );
  }

  #[test]
  fn segmented_seek_lands_on_the_requested_frame_and_fetches_only_needed_segments() {
    let reference = hls_reference();
    let total = reference.len() / 2;
    let (source, fetcher) = hls_fixture();
    let mut decoder = Decoder::open(&source).expect("ouverture");
    decoder
      .seek(Duration::from_millis(1500))
      .expect("déplacement");
    let mut out = Vec::new();
    let position = decoder
      .next_block(&mut out)
      .expect("décodage")
      .expect("bloc");
    assert!(
      (position.as_secs_f64() - 1.5).abs() < 0.001,
      "position {position:?}"
    );
    out.extend(decode_all(&mut decoder));
    let want = total - 66_150;
    assert!(
      (out.len() as i64 / 2 - want as i64).abs() <= 1,
      "{} au lieu de {want}",
      out.len() / 2
    );
    let start = reference.len() - out.len();
    // Le recouvrement MDCT du premier paquet manque après la reconstruction : 2 paquets ignorés.
    let settle = 2 * 2048;
    assert!(
      reference[start + settle..]
        .iter()
        .zip(&out[settle..])
        .all(|(a, b)| (a - b).abs() < 1e-6),
      "échantillons différents de la référence"
    );
    assert_eq!(hls_calls(&fetcher), vec![1, 2, 3]);
  }

  #[test]
  fn segmented_seek_skips_the_leading_segments() {
    let (source, fetcher) = hls_fixture();
    let mut decoder = Decoder::open(&source).expect("ouverture");
    decoder
      .seek(Duration::from_millis(2500))
      .expect("déplacement");
    decode_all(&mut decoder);
    assert_eq!(hls_calls(&fetcher), vec![2, 3]);
  }

  #[test]
  fn segmented_backward_seek_after_reading_restarts_the_stream() {
    let (source, _) = hls_fixture();
    let mut decoder = Decoder::open(&source).expect("ouverture");
    let mut out = Vec::new();
    for _ in 0..200 {
      decoder.next_block(&mut out).expect("décodage");
    }
    decoder
      .seek(Duration::from_millis(500))
      .expect("déplacement");
    let mut block = Vec::new();
    let position = decoder
      .next_block(&mut block)
      .expect("décodage")
      .expect("bloc");
    assert!((position.as_secs_f64() - 0.5).abs() < 0.001, "{position:?}");
  }

  #[test]
  fn segmented_seek_to_the_exact_end_finishes_the_track() {
    let (source, _) = hls_fixture();
    let mut decoder = Decoder::open(&source).expect("ouverture");
    let duration = decoder.duration().expect("durée");
    decoder.seek(duration).expect("déplacement");
    let mut out = Vec::new();
    assert_eq!(decoder.next_block(&mut out).expect("fin de titre"), None);
    assert_eq!(decoder.next_block(&mut out).expect("fin de titre"), None);
    decoder
      .seek(duration - Duration::from_millis(1))
      .expect("déplacement");
    assert_eq!(decoder.next_block(&mut out).expect("fin de titre"), None);
    decoder
      .seek(Duration::from_millis(500))
      .expect("déplacement");
    assert!(decoder.next_block(&mut out).expect("décodage").is_some());
  }

  #[test]
  fn segmented_seek_past_the_end_is_an_error() {
    let (source, _) = hls_fixture();
    let mut decoder = Decoder::open(&source).expect("ouverture");
    assert!(decoder.seek(Duration::from_secs(4)).is_err());
  }

  #[test]
  fn invalid_data_returns_an_error_instead_of_panicking() {
    assert!(Decoder::open(&Source::from(vec![0u8; 2048])).is_err());
    assert!(Decoder::open(&Source::File(PathBuf::from("absent.flac"))).is_err());
  }
}
