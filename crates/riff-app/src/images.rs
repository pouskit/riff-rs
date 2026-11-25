//! Pochettes : cache mémoire borné, chargement asynchrone par le core, décodage à la taille d'affichage.

use crate::backend::{Backend, LOCAL_BASE_URL, local_url};
use iced::widget::image::Handle;
use image::imageops::FilterType;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use url::{Url, form_urlencoded};

/// Budget mémoire du cache (décision 5).
pub const BUDGET_BYTES: usize = 64 * 1024 * 1024;
const MAX_IN_FLIGHT: usize = 6;
/// Tailles de décodage, en pixels : deux fois l'affichage pour rester net sur un écran à 200 %.
pub const SIZE_ROW: u32 = 96;
pub const SIZE_CARD: u32 = 320;
pub const SIZE_HERO: u32 = 560;

/// Image à la taille donnée.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageKey {
  pub url: String,
  pub size: u32,
}

impl ImageKey {
  #[must_use]
  pub fn new(url: &str, size: u32) -> Self {
    Self {
      url: url.to_string(),
      size,
    }
  }
}

/// Image décodée.
#[derive(Debug, Clone)]
pub struct Decoded {
  pub width: u32,
  pub height: u32,
  pub rgba: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Entry {
  handle: Handle,
  size: (u32, u32),
  bytes: usize,
  used: u64,
}

/// Cache des pochettes. L'éviction retire les images les moins récemment demandées.
#[derive(Debug, Clone, Default)]
pub struct ImageCache {
  entries: HashMap<ImageKey, Entry>,
  in_flight: HashSet<ImageKey>,
  failed: HashSet<ImageKey>,
  wanted_clock: u64,
  total: usize,
  clock: u64,
  budget: Option<usize>,
}

impl ImageCache {
  /// Cache de budget donné, pour les tests.
  #[must_use]
  pub fn with_budget(budget: usize) -> Self {
    Self {
      budget: Some(budget),
      ..Self::default()
    }
  }

  #[must_use]
  pub fn get(&self, key: &ImageKey) -> Option<&Handle> {
    self.entries.get(key).map(|entry| &entry.handle)
  }

  /// Largeur et hauteur en pixels de l'image décodée.
  #[must_use]
  pub fn dimensions(&self, key: &ImageKey) -> Option<(u32, u32)> {
    self.entries.get(key).map(|entry| entry.size)
  }

  /// Octets gardés en mémoire.
  #[must_use]
  pub const fn total_bytes(&self) -> usize {
    self.total
  }

  /// Marque les images visibles comme récentes et renvoie celles à charger, dans l'ordre demandé.
  pub fn request(&mut self, wanted: &[ImageKey]) -> Vec<ImageKey> {
    self.clock += 1;
    self.wanted_clock = self.clock;
    let mut starts = Vec::new();
    for key in wanted {
      if let Some(entry) = self.entries.get_mut(key) {
        entry.used = self.clock;
      } else if self.in_flight.len() < MAX_IN_FLIGHT
        && !self.in_flight.contains(key)
        && !self.failed.contains(key)
      {
        self.in_flight.insert(key.clone());
        starts.push(key.clone());
      }
    }
    starts
  }

  /// Retient une image décodée, puis évince jusqu'à respecter le budget. L'image la plus récente reste.
  pub fn insert(&mut self, key: ImageKey, decoded: Decoded) {
    self.in_flight.remove(&key);
    self.clock += 1;
    let bytes = decoded.rgba.len();
    let size = (decoded.width, decoded.height);
    let handle = Handle::from_rgba(decoded.width, decoded.height, decoded.rgba);
    if let Some(old) = self.entries.insert(
      key,
      Entry {
        handle,
        size,
        bytes,
        used: self.clock,
      },
    ) {
      self.total -= old.bytes;
    }
    self.total += bytes;
    self.evict();
  }

  /// Une image illisible n'est pas redemandée.
  pub fn failed(&mut self, key: ImageKey) {
    self.in_flight.remove(&key);
    self.failed.insert(key);
  }

  fn evict(&mut self) {
    let budget = self.budget.unwrap_or(BUDGET_BYTES);
    let wanted_clock = self.wanted_clock;
    while self.total > budget && self.entries.len() > 1 {
      let oldest = self
        .entries
        .iter()
        .filter(|(_, entry)| wanted_clock == 0 || entry.used < wanted_clock)
        .min_by_key(|(_, entry)| entry.used)
        .map(|(key, _)| key.clone());
      let Some(entry) = oldest.and_then(|key| self.entries.remove(&key)) else {
        return;
      };
      self.total -= entry.bytes;
    }
  }
}

/// Décode et réduit à `size` pixels au plus. Calcul long : à lancer hors du fil de l'interface.
///
/// # Errors
/// Image illisible.
pub fn decode(bytes: &[u8], size: u32) -> Result<Decoded, image::ImageError> {
  let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
  let mut limits = image::Limits::default();
  limits.max_image_width = Some(4096);
  limits.max_image_height = Some(4096);
  limits.max_alloc = Some(64 * 1024 * 1024);
  reader.limits(limits);
  let image = reader.decode()?;
  let image = if image.width() > size || image.height() > size {
    image.resize(size, size, FilterType::Triangle)
  } else {
    image
  };
  let rgba = image.into_rgba8();
  Ok(Decoded {
    width: rgba.width(),
    height: rgba.height(),
    rgba: rgba.into_raw(),
  })
}

fn proxied(url: &str) -> String {
  let query: String = form_urlencoded::Serializer::new(String::new())
    .append_pair("url", url)
    .finish();
  format!("{LOCAL_BASE_URL}/api/image-proxy?{query}")
}

/// D'où lire les octets d'une pochette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageOrigin {
  /// URL `riff://local/...` résolue par le core : réseau et cache disque passent par lui.
  Core(String),
  File(std::path::PathBuf),
}

/// Classe une URL de pochette : locale, fichier, ou distante par `/api/image-proxy`.
#[must_use]
pub fn origin_of(url: &str) -> Option<ImageOrigin> {
  let rewritten = local_url(url);
  if rewritten.starts_with(LOCAL_BASE_URL) {
    return Some(ImageOrigin::Core(rewritten));
  }
  if url.starts_with("http://") || url.starts_with("https://") {
    return Some(ImageOrigin::Core(proxied(url)));
  }
  if url.starts_with("file:") {
    return Url::parse(url)
      .ok()?
      .to_file_path()
      .ok()
      .map(ImageOrigin::File);
  }
  if url.contains("://") || url.starts_with("data:") || url.is_empty() {
    return None;
  }
  Some(ImageOrigin::File(url.into()))
}

/// Lit les octets d'une pochette.
///
/// # Errors
/// Lecture impossible.
pub async fn fetch_bytes(backend: &Arc<Backend>, url: &str) -> Result<Vec<u8>, String> {
  match origin_of(url).ok_or_else(|| format!("pochette non prise en charge : {url}"))? {
    ImageOrigin::Core(local) => {
      let reply = backend
        .resolve_url(&local)
        .await
        .map_err(|err| err.to_string())?;
      Backend::reply_bytes(reply)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|err| err.to_string())
    }
    ImageOrigin::File(path) => tokio::task::spawn_blocking(move || std::fs::read(path))
      .await
      .map_err(|err| err.to_string())?
      .map_err(|err| err.to_string()),
  }
}

/// Charge et décode une pochette.
///
/// # Errors
/// Lecture ou décodage impossible.
pub async fn load(backend: Arc<Backend>, key: ImageKey) -> Result<Decoded, String> {
  let bytes = fetch_bytes(&backend, &key.url).await?;
  let size = key.size;
  tokio::task::spawn_blocking(move || decode(&bytes, size))
    .await
    .map_err(|err| err.to_string())?
    .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn decoded(width: u32, height: u32) -> Decoded {
    Decoded {
      width,
      height,
      rgba: vec![0; (width * height * 4) as usize],
    }
  }

  fn key(name: &str) -> ImageKey {
    ImageKey::new(name, SIZE_ROW)
  }

  #[test]
  fn the_budget_of_the_decision_is_64_mib() {
    assert_eq!(BUDGET_BYTES, 67_108_864);
  }

  #[test]
  fn the_cache_never_exceeds_64_mib_by_evicting_the_oldest() {
    let mut cache = ImageCache::default();
    let side = 1024;
    let each = (side * side * 4) as usize;
    let count = BUDGET_BYTES / each + 10;
    for index in 0..count {
      cache.insert(key(&format!("{index}")), decoded(side, side));
      assert!(cache.total_bytes() <= BUDGET_BYTES);
    }
    assert!(cache.get(&key("0")).is_none());
    assert!(cache.get(&key(&format!("{}", count - 1))).is_some());
  }

  #[test]
  fn a_recently_requested_image_survives_eviction() {
    let mut cache = ImageCache::with_budget(130);
    cache.insert(key("a"), decoded(4, 4));
    cache.insert(key("b"), decoded(4, 4));
    cache.request(&[key("a")]);
    cache.insert(key("c"), decoded(4, 4));
    assert!(cache.get(&key("a")).is_some());
    assert!(cache.get(&key("b")).is_none());
    assert_eq!(cache.total_bytes(), 128);
  }

  #[test]
  fn an_image_larger_than_the_budget_is_still_shown() {
    let mut cache = ImageCache::with_budget(10);
    cache.insert(key("a"), decoded(4, 4));
    assert!(cache.get(&key("a")).is_some());
  }

  #[test]
  fn replacing_an_image_keeps_the_count_right() {
    let mut cache = ImageCache::default();
    cache.insert(key("a"), decoded(4, 4));
    cache.insert(key("a"), decoded(2, 2));
    assert_eq!(cache.total_bytes(), 16);
  }

  #[test]
  fn requests_are_limited_and_not_repeated() {
    let mut cache = ImageCache::default();
    let wanted: Vec<ImageKey> = (0..10).map(|index| key(&format!("{index}"))).collect();
    let first = cache.request(&wanted);
    assert_eq!(first.len(), MAX_IN_FLIGHT);
    assert_eq!(first[0], wanted[0]);
    assert!(cache.request(&wanted).is_empty());
    cache.insert(first[0].clone(), decoded(1, 1));
    assert_eq!(cache.request(&wanted), [wanted[MAX_IN_FLIGHT].clone()]);
  }

  #[test]
  fn a_failed_image_is_not_asked_again() {
    let mut cache = ImageCache::default();
    let wanted = [key("a")];
    assert_eq!(cache.request(&wanted).len(), 1);
    cache.failed(key("a"));
    assert!(cache.request(&wanted).is_empty());
  }

  #[test]
  fn decoding_is_bounded_by_the_display_size() {
    let mut png = Vec::new();
    image::RgbaImage::new(400, 200)
      .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
      .unwrap();
    let small = decode(&png, SIZE_ROW).unwrap();
    assert_eq!((small.width, small.height), (96, 48));
    assert_eq!(small.rgba.len(), 96 * 48 * 4);
    let kept = decode(&png, SIZE_HERO).unwrap();
    assert_eq!((kept.width, kept.height), (400, 200));
    assert!(decode(b"pas une image", SIZE_ROW).is_err());
  }

  #[test]
  fn each_kind_of_cover_has_its_origin() {
    assert_eq!(
      origin_of("https://a.b/c.jpg?x=1"),
      Some(ImageOrigin::Core(
        "riff://local/api/image-proxy?url=https%3A%2F%2Fa.b%2Fc.jpg%3Fx%3D1".to_string()
      ))
    );
    assert_eq!(
      origin_of("http://127.0.0.1:38472/api/local-thumbnail?p=1"),
      Some(ImageOrigin::Core(
        "riff://local/api/local-thumbnail?p=1".to_string()
      ))
    );
    // `to_file_path` dépend de l'OS : un chemin de lecteur n'a de sens que sous Windows.
    let (file_url, file_path) = if cfg!(windows) {
      ("file:///D:/a.png", "D:\\a.png")
    } else {
      ("file:///a.png", "/a.png")
    };
    assert_eq!(
      origin_of(file_url),
      Some(ImageOrigin::File(file_path.into()))
    );
    assert_eq!(
      origin_of("D:\\a.png"),
      Some(ImageOrigin::File("D:\\a.png".into()))
    );
    assert_eq!(origin_of("data:image/png;base64,AAAA"), None);
    assert_eq!(origin_of(""), None);
  }

  #[test]
  fn actively_wanted_keys_never_enter_a_reload_loop() {
    let mut cache = ImageCache::with_budget(64);
    let wanted: Vec<ImageKey> = (0..200).map(|i| key(&format!("img-{i}"))).collect();
    let mut loads = 0;
    loop {
      let batch = cache.request(&wanted);
      if batch.is_empty() {
        break;
      }
      loads += batch.len();
      assert!(loads <= wanted.len(), "boucle de rechargement détectée");
      for k in batch {
        cache.insert(k, decoded(4, 4));
      }
    }
    assert_eq!(loads, 200);
    assert!(cache.request(&wanted).is_empty());
  }
}
