//! Sources audio : fichier, mémoire, HTTP par plages via un `RangeFetcher` fourni par l'appelant.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSource;

use crate::AudioError;

/// Lecture par plages d'octets, implémentée par l'appelant (`riff_core::net`).
pub trait RangeFetcher: Send + Sync {
  /// Ouvre un flux à partir de `start`, jusqu'à `end` exclu si indiqué.
  ///
  /// # Errors
  /// Erreur réseau ou plage refusée.
  fn fetch(&self, start: u64, end: Option<u64>) -> io::Result<Box<dyn Read + Send>>;

  /// Taille totale, si elle est connue.
  fn len(&self) -> Option<u64>;

  fn is_empty(&self) -> bool {
    self.len() == Some(0)
  }
}

/// Récupération des segments d'un titre découpé (HLS), implémentée par l'appelant.
pub trait SegmentFetcher: Send + Sync {
  /// Ouvre le flux du segment `index`.
  ///
  /// # Errors
  /// Erreur réseau ou segment refusé.
  fn fetch(&self, index: usize) -> io::Result<Box<dyn Read + Send>>;
}

/// Titre en segments fMP4 : un en-tête d'initialisation puis des segments de durée connue.
#[derive(Clone)]
pub struct Segmented {
  pub init: Arc<[u8]>,
  /// Début de chaque segment (cumul des durées annoncées), `starts[0]` = 0.
  pub starts: Arc<[Duration]>,
  /// Durée totale : somme des durées annoncées.
  pub duration: Duration,
  pub fetcher: Arc<dyn SegmentFetcher>,
}

impl Segmented {
  /// Segment qui contient l'instant `at`.
  pub(crate) fn segment_at(&self, at: Duration) -> usize {
    self
      .starts
      .partition_point(|start| *start <= at)
      .saturating_sub(1)
  }
}

/// Origine des données d'un titre.
#[derive(Clone)]
pub enum Source {
  File(PathBuf),
  Memory(Arc<[u8]>),
  Http(Arc<dyn RangeFetcher>),
  Segmented(Segmented),
}

impl From<Vec<u8>> for Source {
  fn from(bytes: Vec<u8>) -> Self {
    Self::Memory(bytes.into())
  }
}

impl std::fmt::Debug for Source {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::File(path) => f.debug_tuple("File").field(path).finish(),
      Self::Memory(bytes) => write!(f, "Memory({} octets)", bytes.len()),
      Self::Http(_) => f.write_str("Http"),
      Self::Segmented(segmented) => write!(f, "Segmented({} segments)", segmented.starts.len()),
    }
  }
}

/// Octets gardés derrière la position de lecture : couvre l'entrelacement de deux pistes
/// d'un mp4 (zones distantes d'une dizaine de Ko).
const WINDOW_KEEP: usize = 1 << 20;
/// Taille à partir de laquelle la fenêtre est ramenée à `WINDOW_KEEP` (copie amortie).
const WINDOW_TRIM_AT: usize = 2 * WINDOW_KEEP;
/// Saut vers l'avant lu et gardé plutôt que de rouvrir une connexion : à quelques Mbit/s,
/// 256 Kio se lisent en moins qu'une ouverture de requête HTTPS suivie de sa montée en débit.
const SKIP_AHEAD_MAX: u64 = 256 << 10;
const SKIP_CHUNK: usize = 64 << 10;

/// Flux HTTP : une connexion ouverte à partir de `stream_pos`, et une fenêtre des derniers
/// octets reçus. Seul un retour hors fenêtre ou un grand saut vers l'avant rouvre une requête.
struct RangeSource {
  fetcher: Arc<dyn RangeFetcher>,
  /// Position logique de lecture.
  position: u64,
  stream: Mutex<Option<Box<dyn Read + Send>>>,
  /// Position du prochain octet à sortir de `stream`.
  stream_pos: u64,
  /// Derniers octets reçus : `window[0]` est à l'offset `window_start`, la fin à `stream_pos`.
  window: Vec<u8>,
  window_start: u64,
}

impl RangeSource {
  fn new(fetcher: Arc<dyn RangeFetcher>) -> Self {
    Self {
      fetcher,
      position: 0,
      stream: Mutex::new(None),
      stream_pos: 0,
      window: Vec::new(),
      window_start: 0,
    }
  }

  fn push_window(&mut self, bytes: &[u8]) {
    self.window.extend_from_slice(bytes);
    self.stream_pos += bytes.len() as u64;
    if self.window.len() > WINDOW_TRIM_AT {
      let cut = self.window.len() - WINDOW_KEEP;
      self.window.drain(..cut);
      self.window_start += cut as u64;
    }
  }

  fn slot(&mut self) -> io::Result<&mut Option<Box<dyn Read + Send>>> {
    self
      .stream
      .get_mut()
      .map_err(|_| io::Error::other("verrou empoisonné"))
  }

  /// Lit au plus `buffer.len()` octets de la connexion ouverte ; 0 si aucune.
  fn read_stream(&mut self, len: usize, buffer: &mut [u8]) -> io::Result<usize> {
    match self.slot()?.as_mut() {
      Some(stream) => stream.read(&mut buffer[..len]),
      None => Ok(0),
    }
  }

  /// Repart d'une nouvelle connexion à `position`.
  fn reopen(&mut self) -> io::Result<()> {
    *self.slot()? = None;
    self.window.clear();
    self.window_start = self.position;
    self.stream_pos = self.position;
    let stream = self.fetcher.fetch(self.position, None)?;
    *self.slot()? = Some(stream);
    Ok(())
  }
}

impl Read for RangeSource {
  fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
    if buffer.is_empty() {
      return Ok(0);
    }
    if self.position >= self.window_start && self.position < self.stream_pos {
      let from = (self.position - self.window_start) as usize;
      let count = buffer.len().min(self.window.len() - from);
      buffer[..count].copy_from_slice(&self.window[from..from + count]);
      self.position += count as u64;
      return Ok(count);
    }
    let open = self.slot()?.is_some();
    let reusable =
      open && self.position >= self.stream_pos && self.position - self.stream_pos <= SKIP_AHEAD_MAX;
    if !reusable {
      self.reopen()?;
    }
    let mut skip = Vec::new();
    while self.position > self.stream_pos {
      skip.resize(SKIP_CHUNK, 0);
      let want = skip.len().min((self.position - self.stream_pos) as usize);
      let count = self.read_stream(want, &mut skip)?;
      if count == 0 {
        return Ok(0);
      }
      self.push_window(&skip[..count]);
    }
    let count = self.read_stream(buffer.len(), buffer)?;
    self.push_window(&buffer[..count]);
    self.position += count as u64;
    Ok(count)
  }
}

impl Seek for RangeSource {
  fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "position invalide");
    self.position = match target {
      SeekFrom::Start(offset) => offset,
      SeekFrom::Current(delta) => self
        .position
        .checked_add_signed(delta)
        .ok_or_else(invalid)?,
      SeekFrom::End(delta) => {
        let len = self
          .fetcher
          .len()
          .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "taille inconnue"))?;
        len.checked_add_signed(delta).ok_or_else(invalid)?
      }
    };
    Ok(self.position)
  }
}

impl MediaSource for RangeSource {
  fn is_seekable(&self) -> bool {
    self.fetcher.len().is_some()
  }

  fn byte_len(&self) -> Option<u64> {
    self.fetcher.len()
  }
}

/// Taille maximale d'un segment préchargé : au-delà, le serveur n'envoie pas un segment audio.
const SEGMENT_MAX: u64 = 8 << 20;

/// Lecture continue de l'en-tête puis des segments à partir de `next`. Le segment suivant est
/// téléchargé en avance par un thread : le tampon de sortie est trop court pour attendre la
/// première réponse du serveur à chaque segment. Au plus deux segments en mémoire.
/// Les erreurs ne sont jamais `UnexpectedEof`, que `symphonia` prendrait pour une fin de flux.
pub(crate) struct SegmentReader {
  segmented: Segmented,
  /// Derrière un verrou pour que le lecteur soit `Sync` (exigé par `MediaSource`).
  current: Mutex<Option<Box<dyn Read + Send>>>,
  next: usize,
  prefetch: Option<(usize, JoinHandle<io::Result<Vec<u8>>>)>,
  /// Levé au `Drop` : le préchargement en cours cesse de télécharger.
  cancel: Arc<AtomicBool>,
  position: u64,
}

impl Drop for SegmentReader {
  fn drop(&mut self) {
    self.cancel.store(true, Ordering::Relaxed);
  }
}

impl SegmentReader {
  pub(crate) fn new(segmented: Segmented, first: usize) -> Self {
    let init = Cursor::new(segmented.init.clone());
    Self {
      segmented,
      current: Mutex::new(Some(Box::new(init))),
      next: first,
      prefetch: None,
      cancel: Arc::new(AtomicBool::new(false)),
      position: 0,
    }
  }

  fn start_prefetch(&mut self, index: usize) {
    if index >= self.segmented.starts.len() {
      return;
    }
    let fetcher = self.segmented.fetcher.clone();
    let cancel = Arc::clone(&self.cancel);
    let handle = std::thread::Builder::new()
      .name("riff-segment".into())
      .spawn(move || {
        let mut stream = fetcher.fetch(index)?.take(SEGMENT_MAX + 1);
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 64 << 10];
        loop {
          if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::other("préchargement annulé"));
          }
          let count = match stream.read(&mut chunk) {
            Ok(count) => count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
          };
          if count == 0 {
            break;
          }
          bytes.extend_from_slice(&chunk[..count]);
        }
        if bytes.len() as u64 > SEGMENT_MAX {
          return Err(io::Error::other("segment trop volumineux"));
        }
        Ok(bytes)
      });
    // Sans thread, le segment sera simplement lu à la demande.
    self.prefetch = handle.ok().map(|handle| (index, handle));
  }

  fn current(&mut self) -> io::Result<&mut Option<Box<dyn Read + Send>>> {
    self
      .current
      .get_mut()
      .map_err(|_| io::Error::other("verrou empoisonné"))
  }

  /// Passe au segment suivant ; `false` après le dernier.
  fn advance(&mut self) -> io::Result<bool> {
    let index = self.next;
    if index >= self.segmented.starts.len() {
      *self.current()? = None;
      return Ok(false);
    }
    let stream: Box<dyn Read + Send> = match self.prefetch.take() {
      Some((at, handle)) if at == index => {
        let bytes = handle
          .join()
          .map_err(|_| io::Error::other("préchargement interrompu"))??;
        Box::new(Cursor::new(bytes))
      }
      _ => self.segmented.fetcher.fetch(index)?,
    };
    *self.current()? = Some(stream);
    self.next = index + 1;
    self.start_prefetch(index + 1);
    Ok(true)
  }
}

impl Read for SegmentReader {
  fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
    if buffer.is_empty() {
      return Ok(0);
    }
    loop {
      let Some(current) = self.current()?.as_mut() else {
        return Ok(0);
      };
      let count = current.read(buffer)?;
      if count > 0 {
        self.position += count as u64;
        return Ok(count);
      }
      if !self.advance()? {
        return Ok(0);
      }
    }
  }
}

impl Seek for SegmentReader {
  fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
    match target {
      SeekFrom::Current(0) => Ok(self.position),
      SeekFrom::Start(offset) if offset == self.position => Ok(self.position),
      _ => Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "déplacement impossible dans un flux segmenté",
      )),
    }
  }
}

impl MediaSource for SegmentReader {
  fn is_seekable(&self) -> bool {
    false
  }

  fn byte_len(&self) -> Option<u64> {
    None
  }
}

/// Ouvre la source pour `symphonia`, avec l'extension du fichier comme indication.
pub(crate) fn open(source: &Source) -> Result<(Box<dyn MediaSource>, Hint), AudioError> {
  let mut hint = Hint::new();
  let media: Box<dyn MediaSource> = match source {
    Source::File(path) => {
      if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
      }
      let file = File::open(path)
        .map_err(|err| AudioError::Source(format!("{} : {err}", path.display())))?;
      Box::new(file)
    }
    Source::Memory(bytes) => Box::new(Cursor::new(bytes.clone())),
    Source::Http(fetcher) => Box::new(RangeSource::new(fetcher.clone())),
    // Sonde de l'en-tête seul, en mémoire : aucune requête avant la première lecture.
    Source::Segmented(segmented) => {
      hint.with_extension("mp4");
      Box::new(Cursor::new(segmented.init.clone()))
    }
  };
  Ok((media, hint))
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicUsize, Ordering};

  struct CountingFetcher {
    bytes: Vec<u8>,
    calls: AtomicUsize,
  }

  impl CountingFetcher {
    fn new(len: usize) -> Arc<Self> {
      // Suite pseudo-aléatoire : un décalage d'octets se verrait.
      let mut state = 0x2545_F491u32;
      let bytes = (0..len)
        .map(|_| {
          state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
          (state >> 24) as u8
        })
        .collect();
      Arc::new(Self {
        bytes,
        calls: AtomicUsize::new(0),
      })
    }

    fn slice(&self, offset: u64, len: usize) -> &[u8] {
      let start = offset as usize;
      &self.bytes[start..start + len]
    }

    fn calls(&self) -> usize {
      self.calls.load(Ordering::Relaxed)
    }
  }

  impl RangeFetcher for CountingFetcher {
    fn fetch(&self, start: u64, end: Option<u64>) -> io::Result<Box<dyn Read + Send>> {
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

  fn read_at(source: &mut RangeSource, offset: u64, len: usize) -> Vec<u8> {
    source.seek(SeekFrom::Start(offset)).expect("seek");
    let mut out = vec![0u8; len];
    source.read_exact(&mut out).expect("lecture");
    out
  }

  #[test]
  fn two_close_cursors_share_one_connection() {
    let fetcher = CountingFetcher::new(3 << 20);
    let mut source = RangeSource::new(fetcher.clone());
    let (mut a, mut b) = (100_000u64, 112_000u64);
    for _ in 0..2000 {
      let got = read_at(&mut source, a, 20);
      assert_eq!(got, fetcher.slice(a, 20));
      a += 20;
      let got = read_at(&mut source, b, 600);
      assert_eq!(got, fetcher.slice(b, 600));
      b += 600;
    }
    assert_eq!(fetcher.calls(), 1);
  }

  #[test]
  fn forward_jump_below_threshold_keeps_the_connection() {
    let fetcher = CountingFetcher::new(2 << 20);
    let mut source = RangeSource::new(fetcher.clone());
    read_at(&mut source, 1000, 100);
    let near = 1100 + SKIP_AHEAD_MAX;
    assert_eq!(read_at(&mut source, near, 50), fetcher.slice(near, 50));
    assert_eq!(fetcher.calls(), 1);
    let far = near + 50 + SKIP_AHEAD_MAX + 1;
    assert_eq!(read_at(&mut source, far, 50), fetcher.slice(far, 50));
    assert_eq!(fetcher.calls(), 2);
  }

  #[test]
  fn backward_inside_window_is_free_and_outside_reopens() {
    let fetcher = CountingFetcher::new(6 << 20);
    let mut source = RangeSource::new(fetcher.clone());
    let mut out = vec![0u8; 5 << 20];
    source.read_exact(&mut out).expect("lecture");
    assert_eq!(out, fetcher.bytes[..5 << 20]);
    assert_eq!(fetcher.calls(), 1);
    assert!(source.window.len() <= WINDOW_TRIM_AT, "fenêtre non bornée");
    let inside = (5u64 << 20) - 1000;
    assert_eq!(
      read_at(&mut source, inside, 500),
      fetcher.slice(inside, 500)
    );
    assert_eq!(fetcher.calls(), 1);
    assert_eq!(read_at(&mut source, 10, 500), fetcher.bytes[10..510]);
    assert_eq!(fetcher.calls(), 2);
  }

  #[test]
  fn seek_variants_and_end_of_file() {
    let fetcher = CountingFetcher::new(10_000);
    let mut source = RangeSource::new(fetcher.clone());
    assert!(source.is_seekable());
    assert_eq!(source.byte_len(), Some(10_000));
    assert_eq!(source.seek(SeekFrom::End(-10)).expect("fin"), 9990);
    let mut rest = Vec::new();
    source.read_to_end(&mut rest).expect("lecture");
    assert_eq!(rest, fetcher.bytes[9990..]);
    assert_eq!(source.seek(SeekFrom::Current(-20)).expect("relatif"), 9980);
    assert!(source.seek(SeekFrom::Current(-20_000)).is_err());
    assert_eq!(read_at(&mut source, 9980, 20), fetcher.bytes[9980..]);
    // La fin du fichier a ouvert une connexion à 9990 ; 9980 est hors fenêtre : une seconde.
    assert_eq!(fetcher.calls(), 2);
  }

  struct FailingFetcher {
    calls: AtomicUsize,
  }

  struct FailAfter(usize);

  impl Read for FailAfter {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
      if self.0 == 0 {
        return Err(io::Error::other("connexion coupée"));
      }
      let count = buffer.len().min(self.0);
      buffer[..count].fill(7);
      self.0 -= count;
      Ok(count)
    }
  }

  impl RangeFetcher for FailingFetcher {
    fn fetch(&self, _: u64, _: Option<u64>) -> io::Result<Box<dyn Read + Send>> {
      self.calls.fetch_add(1, Ordering::Relaxed);
      Ok(Box::new(FailAfter(100)))
    }

    fn len(&self) -> Option<u64> {
      Some(1000)
    }
  }

  #[test]
  fn read_error_is_reported_without_reopening() {
    let fetcher = Arc::new(FailingFetcher {
      calls: AtomicUsize::new(0),
    });
    let mut source = RangeSource::new(fetcher.clone());
    let mut buffer = [0u8; 64];
    assert_eq!(source.read(&mut buffer).expect("début"), 64);
    assert_eq!(source.read(&mut buffer).expect("fin du reste"), 36);
    assert!(source.read(&mut buffer).is_err());
    assert!(source.read(&mut buffer).is_err());
    assert_eq!(fetcher.calls.load(Ordering::Relaxed), 1);
  }

  /// Segments factices : le segment `i` contient `i + 1` répété 1000 fois. Journalise les requêtes.
  struct FakeSegments {
    calls: Mutex<Vec<usize>>,
    fail_at: Option<usize>,
  }

  impl SegmentFetcher for FakeSegments {
    fn fetch(&self, index: usize) -> io::Result<Box<dyn Read + Send>> {
      self.calls.lock().expect("verrou").push(index);
      if self.fail_at == Some(index) {
        return Err(io::Error::other("segment indisponible"));
      }
      Ok(Box::new(Cursor::new(vec![index as u8 + 1; 1000])))
    }
  }

  fn segmented(count: usize, fail_at: Option<usize>) -> (Segmented, Arc<FakeSegments>) {
    let fetcher = Arc::new(FakeSegments {
      calls: Mutex::new(Vec::new()),
      fail_at,
    });
    let starts: Vec<Duration> = (0..count as u64).map(Duration::from_secs).collect();
    let segmented = Segmented {
      init: Arc::from(&b"INIT"[..]),
      starts: starts.into(),
      duration: Duration::from_secs(count as u64),
      fetcher: fetcher.clone(),
    };
    (segmented, fetcher)
  }

  fn calls(fetcher: &FakeSegments) -> Vec<usize> {
    let mut calls = fetcher.calls.lock().expect("verrou").clone();
    calls.sort_unstable();
    calls
  }

  #[test]
  fn segments_are_read_after_the_init_in_order_without_early_requests() {
    let (segmented, fetcher) = segmented(3, None);
    let mut reader = SegmentReader::new(segmented, 1);
    assert!(calls(&fetcher).is_empty(), "requête avant lecture");
    let mut out = Vec::new();
    reader.read_to_end(&mut out).expect("lecture");
    assert_eq!(&out[..4], b"INIT");
    assert!(out[4..1004].iter().all(|b| *b == 2));
    assert!(out[1004..].iter().all(|b| *b == 3));
    assert_eq!(out.len(), 2004);
    assert_eq!(calls(&fetcher), vec![1, 2], "chaque segment une seule fois");
  }

  #[test]
  fn segment_at_finds_the_segment_containing_the_instant() {
    let (segmented, _) = segmented(4, None);
    assert_eq!(segmented.segment_at(Duration::ZERO), 0);
    assert_eq!(segmented.segment_at(Duration::from_millis(999)), 0);
    assert_eq!(segmented.segment_at(Duration::from_secs(1)), 1);
    assert_eq!(segmented.segment_at(Duration::from_secs(99)), 3);
  }

  #[test]
  fn segment_failure_is_an_other_error_never_an_end_of_file() {
    let (segmented, _) = segmented(3, Some(1));
    let mut reader = SegmentReader::new(segmented, 0);
    let mut out = Vec::new();
    let err = reader.read_to_end(&mut out).expect_err("échec attendu");
    assert_eq!(err.kind(), io::ErrorKind::Other);
  }

  #[test]
  fn segment_reader_is_not_seekable_beyond_its_position() {
    let (segmented, _) = segmented(2, None);
    let mut reader = SegmentReader::new(segmented, 0);
    assert!(!reader.is_seekable());
    assert_eq!(reader.byte_len(), None);
    let mut head = [0u8; 4];
    reader.read_exact(&mut head).expect("lecture");
    assert_eq!(reader.stream_position().expect("position"), 4);
    assert_eq!(reader.seek(SeekFrom::Start(4)).expect("position"), 4);
    let err = reader.seek(SeekFrom::Start(0)).expect_err("retour");
    assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    assert!(reader.seek(SeekFrom::End(0)).is_err());
  }
}
