//! État des calques animés et des fonds calculés.

use super::blur::{BlurKey, BlurRequest, SHADE_PART};
use super::viz::{BarBackground, CoverShape, FREQ_BINS, LiquidWave, TvkBars};
use crate::app::{App, DEFAULT_WINDOW};
use crate::images::{self, Decoded, ImageKey};
use crate::screens::Screen;
use crate::theme::tokens::playlist_detail as hero;
use crate::user_data::Track;
use iced::widget::image::Handle;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// `.cover-bg-layer` : `blur(22px)` dans un calque au quart de la taille puis agrandi quatre fois, `inset: -15%`, `saturate(1.3)`.
const COVER_BG_SIGMA_TENTHS: u32 = 880;
const COVER_BG_ZOOM: u32 = 130;
const COVER_BG_SATURATE: u32 = 130;
/// `.banner-image` : `scale(1.06)`.
const BANNER_ZOOM: u32 = 106;
/// Un redimensionnement ne relance le calcul qu'après un écart visible.
const WINDOW_BUCKET: u32 = 320;
const ASPECT_BUCKET: f32 = 2.0;

/// Délai au bout duquel un visuel qui n'arrive pas fait fondre l'ancien vers le vide : 3 s couvrent le chargement d'une
/// pochette lente et le calcul du flou, sans garder l'image d'un autre titre quand le chargement échoue.
pub const PENDING_LIMIT: Duration = Duration::from_secs(3);

/// Images présentées au démarrage. Sans elles, un fil temps critique du pilote AMD (`amdvlk64.dll`) occupe un cœur à 100 %
/// tant que l'interface reste immobile, au repos comme en lecture. Mesuré sur une RX 7800 XT : 60 images l'arrêtent une fois
/// sur deux, 240 images sur 7 lancements sur 7, et il reste arrêté ensuite (120 s de repos, redimensionnement).
pub const DRIVER_WARMUP_FRAMES: u32 = 240;

/// Rôle d'un visuel qui fond au changement d'image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerRole {
  CoverBackground,
  FullScreenBackground,
  Banner,
  CustomBackground,
  /// `.hero-fx` des pages de détail : `opacity 500ms ease` et `fxFadeIn 500ms ease` (`styles.css:9188-9210`).
  Hero,
}

impl LayerRole {
  /// Durée et courbe `cubic-bezier` du fondu, celles de Riff :
  /// `.cover-bg-layer` `opacity 700ms ease`, `.tvk-ambient-blob` `600ms ease`, `.right-panel-banner`
  /// `opacity 300ms cubic-bezier(0.2, 0, 0, 1)`. Le fond personnalisé ne dépend pas du titre : pas de fondu.
  fn fade(self) -> (Duration, [f32; 4]) {
    const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
    match self {
      Self::CoverBackground => (Duration::from_millis(700), EASE),
      Self::FullScreenBackground => (Duration::from_millis(600), EASE),
      Self::Banner => (Duration::from_millis(300), [0.2, 0.0, 0.0, 1.0]),
      Self::CustomBackground => (Duration::ZERO, EASE),
      Self::Hero => (Duration::from_millis(500), EASE),
    }
  }

  /// Règles qui coupent le fondu : la réduction pour tous (déclarations sans `!important`) ; `body.perf` retire la transition de
  /// `.hero-fx` (`styles.css:9211`) et masque `#cover-bg` (`styles.css:8393`).
  fn gate(self) -> crate::motion::Gate {
    match self {
      Self::Hero => crate::motion::Gate::BOTH,
      _ => crate::motion::Gate::REDUCED,
    }
  }
}

/// Ce que le rôle doit montrer à cet instant.
#[derive(Debug, Clone)]
pub enum Target {
  /// Rôle non demandé : retrait immédiat.
  Off,
  /// Demandé, sans image à montrer (titre sans pochette) : fondu vers le vide.
  Empty,
  /// Image demandée mais pas encore prête.
  Pending,
  Ready(Handle),
}

/// Fondu en cours : `from` s'efface, `current` apparaît. `None` d'un côté : fondu vers ou depuis le vide.
#[derive(Debug, Clone)]
pub struct LayerFade {
  pub from: Option<Handle>,
  pub started: Instant,
  /// Avancement après courbe, de 0 à 1.
  pub progress: f32,
}

/// Visuel d'un rôle : l'image montrée, et au plus une image précédente pendant un fondu.
#[derive(Debug, Clone, Default)]
pub struct Layer {
  pub current: Option<Handle>,
  pub fade: Option<LayerFade>,
  /// Le rôle a déjà montré une image : la suivante apparaît en fondu au lieu de surgir.
  seen: bool,
  /// Depuis quand l'image attendue n'arrive pas, alors que l'ancienne reste affichée.
  pending_since: Option<Instant>,
}

impl Layer {
  /// Un fondu ou une attente borne encore demande des images.
  #[must_use]
  pub fn active(&self) -> bool {
    self.fade.is_some() || self.pending_since.is_some()
  }

  pub fn update(&mut self, role: LayerRole, target: Target, now: Instant) {
    match target {
      Target::Off => *self = Self::default(),
      Target::Pending => {
        if self.current.is_some() {
          let since = *self.pending_since.get_or_insert(now);
          if now.saturating_duration_since(since) >= PENDING_LIMIT {
            self.replace(role, None, now);
          }
        }
      }
      Target::Empty => {
        if self.current.is_some() {
          self.replace(role, None, now);
        }
      }
      Target::Ready(handle) => {
        self.pending_since = None;
        if self.current.as_ref().map(Handle::id) != Some(handle.id()) {
          self.replace(role, Some(handle), now);
        }
      }
    }
    self.advance(role, now);
  }

  fn replace(&mut self, role: LayerRole, next: Option<Handle>, now: Instant) {
    self.pending_since = None;
    let previous = self.current.take();
    // `transition: opacity` des calques (`styles.css:6395`, `8392`) : durée nulle sous la réduction et sous `body.perf`.
    let fades = !role.fade().0.is_zero()
      && !role.gate().instant()
      && (previous.is_some() || (self.seen && next.is_some()));
    self.fade = fades.then(|| LayerFade {
      from: previous,
      started: now,
      progress: 0.0,
    });
    self.seen |= next.is_some();
    self.current = next;
  }

  fn advance(&mut self, role: LayerRole, now: Instant) {
    let Some(fade) = &mut self.fade else {
      return;
    };
    let (duration, curve) = role.fade();
    let elapsed = now.saturating_duration_since(fade.started);
    if elapsed >= duration || role.gate().instant() {
      self.fade = None;
    } else {
      fade.progress = bezier_ease(curve, elapsed.as_secs_f32() / duration.as_secs_f32());
    }
  }
}

/// `cubic-bezier(x1, y1, x2, y2)` : dichotomie sur x, comme `custom_bezier` des paroles.
pub(crate) fn bezier_ease([x1, y1, x2, y2]: [f32; 4], t: f32) -> f32 {
  if t <= 0.0 {
    return 0.0;
  }
  if t >= 1.0 {
    return 1.0;
  }
  let point = |s: f32, a: f32, b: f32| {
    let inv = 1.0 - s;
    3.0 * inv * inv * s * a + 3.0 * inv * s * s * b + s * s * s
  };
  let (mut low, mut high, mut s) = (0.0_f32, 1.0_f32, t);
  for _ in 0..14 {
    s = f32::midpoint(low, high);
    if point(s, x1, x2) < t {
      low = s;
    } else {
      high = s;
    }
  }
  point(s, y1, y2)
}

/// Opacités de l'ancienne image (dessous) et de la nouvelle (dessus) pour un fondu croisé exact sous l'opacité `opacity`
/// du visuel : `(1 - p) * opacity * ancienne + p * opacity * nouvelle + (1 - opacity) * fond`.
#[must_use]
pub fn crossfade_opacities(opacity: f32, progress: f32) -> (f32, f32) {
  let top = opacity * progress;
  let rest = 1.0 - top;
  let bottom = if rest <= f32::EPSILON {
    0.0
  } else {
    opacity * (1.0 - progress) / rest
  };
  (bottom, top)
}

/// Calques animés.
#[derive(Debug, Clone)]
pub struct VisualState {
  pub bar: BarBackground,
  pub tvk: TvkBars,
  pub shape: CoverShape,
  pub liquid: LiquidWave,
  pub liquid_points: Vec<(f32, f32)>,
  pub bar_width: f32,
  pub freq: [u8; FREQ_BINS],
  pub last_tick: Option<Instant>,
  /// Instant où le calque `.bg-mesh-layer` est apparu : `meshFloat` repart de zéro à chaque affichage (un élément en `display: none`
  /// n'a pas d'animation, `styles.css:11750`). `None` tant que le maillage est masqué.
  pub mesh_started: Option<Instant>,
  /// `lastMeshKey` : la clé du calque affiché (`None` : maillage masqué).
  pub mesh_key: Option<crate::visuals::mesh::MeshKey>,
  pub blurred: HashMap<BlurKey, Handle>,
  pub blur_in_flight: HashSet<BlurKey>,
  pub cover_bg: Layer,
  /// Fond de la vue plein écran : les quatre orbes `.tvk-ambient-blob`, puis la vignette ([`super::blur::SHADE_PART`]).
  pub tvk_orbs: [Layer; 5],
  pub banner: Layer,
  pub custom_bg: Layer,
  /// Halo flouté de la pochette d'une page de détail (`.hero-fx`).
  pub hero_bg: Layer,
  /// Mesure de capacité : les images suivent l'écran, sans minuteur à 60 Hz.
  pub unthrottled: bool,
  /// Images restantes du démarrage, voir [`DRIVER_WARMUP_FRAMES`].
  pub warmup_frames: u32,
}

impl Default for VisualState {
  fn default() -> Self {
    Self {
      bar: BarBackground::default(),
      tvk: TvkBars::default(),
      shape: CoverShape::default(),
      liquid: LiquidWave::default(),
      liquid_points: Vec::new(),
      bar_width: 1248.0,
      freq: [0; FREQ_BINS],
      last_tick: None,
      mesh_started: None,
      mesh_key: None,
      blurred: HashMap::new(),
      blur_in_flight: HashSet::new(),
      cover_bg: Layer::default(),
      tvk_orbs: Default::default(),
      banner: Layer::default(),
      custom_bg: Layer::default(),
      hero_bg: Layer::default(),
      unthrottled: false,
      warmup_frames: 0,
    }
  }
}

impl VisualState {
  /// Un fondu ou une attente demande la prochaine image.
  #[must_use]
  pub fn layers_active(&self) -> bool {
    [&self.cover_bg, &self.banner, &self.custom_bg, &self.hero_bg]
      .into_iter()
      .chain(&self.tvk_orbs)
      .any(Layer::active)
  }
}

/// Fond à calculer : l'image source et son flou.
#[derive(Debug, Clone, PartialEq)]
pub struct WantedBlur {
  pub source: ImageKey,
  pub key: BlurKey,
}

/// Calcul de flou à lancer.
#[derive(Debug, Clone)]
pub struct BlurJob {
  pub key: BlurKey,
  pub width: u32,
  pub height: u32,
  pub pixels: bytes::Bytes,
}

impl App {
  fn blur_request(
    &self,
    sigma_tenths: u32,
    zoom_percent: u32,
    saturate_percent: u32,
  ) -> BlurRequest {
    let window = self.window.size.unwrap_or(DEFAULT_WINDOW);
    let bucket =
      |value: f32| ((value / WINDOW_BUCKET as f32).round() as u32).max(1) * WINDOW_BUCKET;
    let aspect = (window.width / window.height.max(1.0) * ASPECT_BUCKET).round() / ASPECT_BUCKET;
    BlurRequest {
      sigma_tenths,
      aspect_tenths: (aspect * 10.0) as u32,
      zoom_percent,
      saturate_percent,
      window_width: bucket(window.width),
    }
  }

  /// Pochette d'un titre : `cover` de `riff_track_art`, sinon la sienne. Les pochettes animées ne sont pas portées.
  #[must_use]
  pub fn cover_of(&self, track: &Track) -> Option<String> {
    let art = &self.settings.visual.track_art;
    if art.as_object().is_some_and(serde_json::Map::is_empty) {
      return track.thumb_url();
    }
    art
      .get(track.canonical_id(&self.data.platform))
      .and_then(|entry| entry.get("cover"))
      .and_then(serde_json::Value::as_str)
      .filter(|url| !url.is_empty() && !is_animated(url))
      .map(str::to_string)
      .or_else(|| track.thumb_url())
  }

  /// Bannière du titre courant : `riff_track_art`, `riff_banner_media`, puis sa pochette.
  #[must_use]
  pub fn banner_url(&self) -> Option<String> {
    let visual = &self.settings.visual;
    if !visual.banners || !visual.track_banners {
      return None;
    }
    let track = self.player.current.as_ref()?;
    let own = visual
      .track_banner(&track.canonical_id(&self.data.platform))
      .map(str::to_string);
    let chosen = own
      .or_else(|| (!visual.banner_media.is_empty()).then(|| visual.banner_media.clone()))
      .or_else(|| self.cover_of(track))?;
    (!is_video(&chosen)).then_some(chosen)
  }

  /// Flou, opacité et fondu de la bannière du titre courant : ceux du titre quand `bannerCustomFx` est actif, sinon les globaux.
  #[must_use]
  pub fn banner_effects(&self) -> (u32, u32, u32) {
    let visual = &self.settings.visual;
    let global = (
      visual.banner_blur,
      visual.banner_opacity,
      visual.banner_fade,
    );
    let Some(track) = self.player.current.as_ref() else {
      return global;
    };
    let fx = visual.track_fx(&track.canonical_id(&self.data.platform));
    if fx.enabled {
      (fx.blur, fx.opacity, fx.fade)
    } else {
      global
    }
  }

  /// Image montrée pour la bannière d'un titre dans sa fenêtre : la sienne, la globale, puis sa pochette. Les vidéos sont omises.
  #[must_use]
  pub fn banner_preview(&self, track: &Track) -> Option<String> {
    let visual = &self.settings.visual;
    let own = visual.track_banner(&track.canonical_id(&self.data.platform));
    let global = (!visual.banner_media.is_empty()).then_some(visual.banner_media.as_str());
    own
      .or(global)
      .map(str::to_string)
      .or_else(|| self.cover_of(track))
      .filter(|url| !is_video(url))
  }

  /// Fond personnalisé : les vidéos ne sont pas portées.
  #[must_use]
  pub fn background_url(&self) -> Option<&str> {
    let url = self.settings.visual.bg_url.as_str();
    (!url.is_empty() && !is_video(url)).then_some(url)
  }

  /// Clé du fond de pochette du titre courant.
  #[must_use]
  pub fn cover_bg_key(&self) -> Option<BlurKey> {
    if !self.settings.visual.cover_bg {
      return None;
    }
    let url = self.cover_of(self.player.current.as_ref()?)?;
    Some(BlurKey::new(
      &url,
      &self.blur_request(COVER_BG_SIGMA_TENTHS, COVER_BG_ZOOM, COVER_BG_SATURATE),
    ))
  }

  /// Clé d'une pièce du fond de la vue plein écran : un orbe (0 à 3) ou la vignette ([`SHADE_PART`]). La vue qui s'efface garde son fond.
  #[must_use]
  pub fn tvk_bg_key(&self, part: u8) -> Option<BlurKey> {
    if !self.tvk_shown() {
      return None;
    }
    let url = self.cover_of(self.player.current.as_ref()?)?;
    let mut key = BlurKey::new(
      &url,
      &self.blur_request(self.settings.visual.tvk_bg_blur * 10, 100, 100),
    );
    key.ambient = Some(part);
    Some(key)
  }

  /// Clé du fond personnalisé flouté.
  #[must_use]
  pub fn custom_bg_key(&self) -> Option<BlurKey> {
    let blur = self.settings.visual.bg_blur;
    let url = self.background_url().filter(|_| blur > 0)?;
    Some(BlurKey::new(
      url,
      &self.blur_request(blur * 10, BANNER_ZOOM, 100),
    ))
  }

  /// Pochette du halo `.hero-fx` de l'écran : celle de la playlist ouverte (`app.js:13417-13424`) ou de l'album ouvert
  /// (`app.js:9642-9644`, `backgroundImage` de `#album-hero-fx`, sans mosaïque ni aplat).
  #[must_use]
  pub fn hero_url(&self) -> Option<String> {
    self.hero_url_on(self.screen)
  }

  /// Pochette du halo qu'aurait `screen` : décide du voile des entrées avant que l'écran soit affiché.
  #[must_use]
  pub fn hero_url_on(&self, screen: Screen) -> Option<String> {
    match screen {
      Screen::Liked => self.data.liked_custom_cover().map(str::to_string),
      Screen::PlaylistDetail => {
        let playlist = self.data.playlist(self.ui.playlist_id.as_deref()?)?;
        crate::screens::common::playlist_cover_url(playlist)
      }
      Screen::Album => self
        .ui
        .album
        .album
        .cover
        .clone()
        .filter(|url| !url.is_empty()),
      _ => None,
    }
  }

  /// Clé du halo : fenêtre entière sur 70 % de sa hauteur, agrandi de 1,3 (`blur(70px)` après `scale(1.3)`).
  #[must_use]
  pub fn hero_key(&self) -> Option<BlurKey> {
    let url = self.hero_url()?;
    let window = self.window.size.unwrap_or(DEFAULT_WINDOW);
    let mut request = self.blur_request(
      (hero::HERO_BLUR * 10.0).round() as u32,
      (hero::HERO_SCALE * 100.0).round() as u32,
      hero::HERO_SATURATE_PERCENT,
    );
    let aspect = window.width / (window.height * hero::HERO_HEIGHT_SHARE).max(1.0);
    request.aspect_tenths = ((aspect * ASPECT_BUCKET).round() / ASPECT_BUCKET * 10.0) as u32;
    Some(BlurKey::new(&url, &request))
  }

  /// Clé de la bannière floutée.
  #[must_use]
  pub fn banner_key(&self) -> Option<BlurKey> {
    let url = self.banner_url().filter(|_| self.panel_visible())?;
    Some(BlurKey::new(
      &url,
      &self.blur_request(self.banner_effects().0 * 10, BANNER_ZOOM, 100),
    ))
  }

  /// Fonds nécessaires à cet instant.
  #[must_use]
  pub fn wanted_blurs(&self) -> Vec<WantedBlur> {
    let thumb = |key: BlurKey| WantedBlur {
      source: ImageKey::new(&key.url, images::SIZE_CARD),
      key,
    };
    let hero = |key: BlurKey| WantedBlur {
      source: ImageKey::new(&key.url, images::SIZE_HERO),
      key,
    };
    self
      .cover_bg_key()
      .map(thumb)
      .into_iter()
      .chain(
        (0..=SHADE_PART)
          .filter_map(|part| self.tvk_bg_key(part))
          .map(thumb),
      )
      .chain(self.banner_key().map(thumb))
      .chain(self.custom_bg_key().map(hero))
      .chain(self.hero_key().map(hero))
      .collect()
  }

  /// Image du flou demandé : prête, ou en attente. Sans clé, le rôle n'a rien à montrer.
  fn blur_target(&self, key: Option<BlurKey>) -> Target {
    key.map_or(Target::Empty, |key| {
      self
        .visuals
        .blurred
        .get(&key)
        .cloned()
        .map_or(Target::Pending, Target::Ready)
    })
  }

  fn raw_target(&self, url: &str, size: u32) -> Target {
    self
      .images
      .get(&ImageKey::new(url, size))
      .cloned()
      .map_or(Target::Pending, Target::Ready)
  }

  /// Ce que chaque rôle doit montrer : `Off` quand le réglage, la vue ou le titre manque (retrait immédiat), `Empty` quand
  /// le titre n'a pas d'image.
  fn layer_targets(&self) -> ([Target; 4], [Target; 5]) {
    let visual = &self.settings.visual;
    let playing = self.player.current.is_some();
    let cover = if visual.cover_bg && playing {
      self.blur_target(self.cover_bg_key())
    } else {
      Target::Off
    };
    let tvk = if self.tvk_shown() && playing {
      std::array::from_fn(|part| self.blur_target(self.tvk_bg_key(part as u8)))
    } else {
      std::array::from_fn(|_| Target::Off)
    };
    let banner = if visual.banners && visual.track_banners && playing && self.panel_visible() {
      match self.banner_url() {
        None => Target::Empty,
        Some(_) if self.banner_effects().0 > 0 => self.blur_target(self.banner_key()),
        Some(url) => self.raw_target(&url, images::SIZE_CARD),
      }
    } else {
      Target::Off
    };
    let custom = match self.background_url() {
      None => Target::Off,
      Some(url) => match self.custom_bg_key() {
        Some(key) => self.blur_target(Some(key)),
        None => self.raw_target(url, images::SIZE_HERO),
      },
    };
    let halo = match self.hero_key() {
      Some(key) => self.blur_target(Some(key)),
      None => Target::Off,
    };
    ([cover, banner, custom, halo], tvk)
  }

  /// Met les visuels à jour : nouvelle image prête, fondus qui avancent, attente trop longue.
  pub fn sync_layers(&mut self, now: Instant) {
    let ([cover, banner, custom, halo], tvk) = self.layer_targets();
    let visuals = &mut self.visuals;
    visuals
      .cover_bg
      .update(LayerRole::CoverBackground, cover, now);
    for (layer, target) in visuals.tvk_orbs.iter_mut().zip(tvk) {
      layer.update(LayerRole::FullScreenBackground, target, now);
    }
    visuals.banner.update(LayerRole::Banner, banner, now);
    visuals
      .custom_bg
      .update(LayerRole::CustomBackground, custom, now);
    visuals.hero_bg.update(LayerRole::Hero, halo, now);
  }

  /// Fonds à calculer maintenant : leur image source est chargée. Les fonds devenus inutiles sont oubliés ; l'image montrée
  /// reste dans son [`Layer`] jusqu'à l'arrivée de la suivante.
  pub fn take_blur_jobs(&mut self) -> Vec<BlurJob> {
    self.sync_layers(Instant::now());
    let wanted = self.wanted_blurs();
    let visuals = &mut self.visuals;
    visuals
      .blurred
      .retain(|key, _| wanted.iter().any(|item| item.key == *key));
    let mut jobs = Vec::new();
    for item in wanted {
      if visuals.blurred.contains_key(&item.key) || visuals.blur_in_flight.contains(&item.key) {
        continue;
      }
      if let Some(Handle::Rgba {
        width,
        height,
        pixels,
        ..
      }) = self.images.get(&item.source)
      {
        visuals.blur_in_flight.insert(item.key.clone());
        jobs.push(BlurJob {
          key: item.key,
          width: *width,
          height: *height,
          pixels: pixels.clone(),
        });
      }
    }
    jobs
  }

  /// Résultat d'un calcul de flou.
  pub fn blur_ready(&mut self, key: &BlurKey, decoded: Option<Decoded>) {
    self.visuals.blur_in_flight.remove(key);
    if let Some(decoded) = decoded {
      self.visuals.blurred.insert(
        key.clone(),
        Handle::from_rgba(decoded.width, decoded.height, decoded.rgba),
      );
    }
  }
}

/// Vidéo, ou GIF : le décodeur d'images de l'application ne lit que le JPEG et le PNG.
fn is_animated(url: &str) -> bool {
  is_video(url)
    || url
      .to_ascii_lowercase()
      .split('?')
      .next()
      .unwrap_or("")
      .ends_with(".gif")
}

/// `/\.(mp4|webm)($|\?)/i`.
#[must_use]
pub fn is_video(url: &str) -> bool {
  let lower = url.to_ascii_lowercase();
  let path = lower.split('?').next().unwrap_or("");
  path.ends_with(".mp4") || path.ends_with(".webm")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::i18n::I18n;
  use crate::settings::NativeSettings;
  use crate::user_data::UserData;
  use crate::window::PersistedWindowState;
  use iced::Size;

  fn track() -> Track {
    Track {
      id: Some("id1".into()),
      thumbnail: Some("https://exemple.org/p.jpg".into()),
      platform: Some("ytmusic".into()),
      ..Track::default()
    }
  }

  fn app() -> App {
    let mut app = App::new(
      PersistedWindowState::default(),
      NativeSettings::default(),
      I18n::default(),
    )
    .with_data(UserData::default());
    app.window.size = Some(Size::new(1280.0, 800.0));
    app.player.current = Some(track());
    app
  }

  fn decoded() -> Decoded {
    Decoded {
      width: 2,
      height: 2,
      rgba: vec![0; 16],
    }
  }

  #[test]
  fn videos_are_recognized_with_or_without_a_query() {
    assert!(is_video("a.MP4") && is_video("b.webm?x=1"));
    assert!(!is_video("c.png") && !is_video("d.mp4.png"));
  }

  #[test]
  fn no_blur_is_wanted_by_default() {
    assert!(app().wanted_blurs().is_empty());
  }

  #[test]
  fn the_cover_background_waits_for_the_cover_then_runs_once() {
    let mut app = app();
    app.settings.visual.cover_bg = true;
    assert!(
      app.take_blur_jobs().is_empty(),
      "pochette pas encore chargée"
    );
    app.images.insert(
      ImageKey::new("https://exemple.org/p.jpg", images::SIZE_CARD),
      decoded(),
    );
    let jobs = app.take_blur_jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].key.sigma_tenths, 880);
    assert_eq!((jobs[0].width, jobs[0].height), (2, 2));
    assert!(app.take_blur_jobs().is_empty(), "calcul déjà lancé");
    let key = jobs[0].key.clone();
    app.blur_ready(&key, Some(decoded()));
    assert!(app.visuals.blurred.contains_key(&key));
    assert!(app.take_blur_jobs().is_empty(), "déjà calculé");
  }

  #[test]
  fn a_failed_blur_can_be_asked_again_and_a_stale_one_is_forgotten() {
    let mut app = app();
    app.settings.visual.cover_bg = true;
    app.images.insert(
      ImageKey::new("https://exemple.org/p.jpg", images::SIZE_CARD),
      decoded(),
    );
    let key = app.take_blur_jobs()[0].key.clone();
    app.blur_ready(&key, None);
    assert_eq!(app.take_blur_jobs().len(), 1);
    app.blur_ready(&key, Some(decoded()));
    app.settings.visual.cover_bg = false;
    assert!(app.take_blur_jobs().is_empty());
    assert!(app.visuals.blurred.is_empty());
  }

  fn banner_app() -> App {
    let mut app = app();
    app.settings.visual.banners = true;
    app.settings.visual.track_banners = true;
    app.lyrics.panel_open = true;
    app.images.insert(
      ImageKey::new("https://exemple.org/p.jpg", images::SIZE_CARD),
      decoded(),
    );
    app
  }

  fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
  }

  fn id(handle: &Option<Handle>) -> Option<String> {
    handle.as_ref().map(|handle| format!("{:?}", handle.id()))
  }

  /// Lance puis termine le calcul de flou demandé, comme `BlurReady`. Le temps réel ne sert qu'ici : les fondus se
  /// vérifient avec `sync_layers` et des instants choisis.
  fn compute_blurs(app: &mut App) {
    for job in app.take_blur_jobs() {
      app.blur_ready(&job.key, Some(decoded()));
    }
  }

  /// Passe au titre suivant, de pochette `cover`, ou sans pochette.
  fn next_title(app: &mut App, cover: Option<&str>) {
    app.player.current = Some(Track {
      id: Some("id2".into()),
      thumbnail: cover.map(str::to_string),
      platform: Some("ytmusic".into()),
      ..Track::default()
    });
  }

  fn load(app: &mut App, url: &str) {
    app
      .images
      .insert(ImageKey::new(url, images::SIZE_CARD), decoded());
  }

  #[test]
  fn the_previous_blur_stays_until_the_new_one_is_ready_then_is_forgotten() {
    let mut app = banner_app();
    let t0 = Instant::now();
    compute_blurs(&mut app);
    app.sync_layers(t0);
    let old = id(&app.visuals.banner.current);
    assert!(
      old.is_some() && app.visuals.banner.fade.is_none(),
      "premier visuel : sans fondu"
    );
    app.settings.visual.banner_blur = 9;
    let jobs = app.take_blur_jobs();
    assert_eq!(jobs.len(), 1, "le nouveau est lancé");
    assert_eq!(
      id(&app.visuals.banner.current),
      old,
      "l'ancien reste affiché"
    );
    app.blur_ready(&jobs[0].key, Some(decoded()));
    app.sync_layers(t0 + ms(50));
    let fade = app.visuals.banner.fade.as_ref().expect("fondu croisé");
    assert_eq!(id(&fade.from), old);
    assert_ne!(id(&app.visuals.banner.current), old);
    app.take_blur_jobs();
    assert_eq!(app.visuals.blurred.len(), 1, "l'ancien flou est oublié");
  }

  #[test]
  fn the_previous_title_stays_until_the_next_one_is_ready_then_fades_and_stops() {
    let mut app = banner_app();
    let t0 = Instant::now();
    compute_blurs(&mut app);
    app.sync_layers(t0);
    let old = id(&app.visuals.banner.current);
    next_title(&mut app, Some("https://exemple.org/q.jpg"));
    app.sync_layers(t0 + ms(100));
    assert_eq!(id(&app.visuals.banner.current), old, "ancien gardé");
    assert!(app.visuals.banner.fade.is_none());
    assert!(app.animation_needed(), "l'attente est bornée par un délai");
    load(&mut app, "https://exemple.org/q.jpg");
    compute_blurs(&mut app);
    app.sync_layers(t0 + ms(200));
    let fade = app.visuals.banner.fade.as_ref().expect("fondu");
    assert_eq!(id(&fade.from), old);
    app.sync_layers(t0 + ms(350));
    let mid = app.visuals.banner.fade.as_ref().unwrap().progress;
    assert!(
      mid > 0.5 && mid < 1.0,
      "courbe (0.2, 0, 0, 1) : en avance à mi-durée"
    );
    app.sync_layers(t0 + ms(500));
    assert!(app.visuals.banner.fade.is_none());
    assert!(app.visuals.banner.current.is_some());
    assert!(!app.animation_needed(), "rien à redessiner ensuite");
    assert_eq!(
      app.visuals.blurred.len(),
      1,
      "une seule image précédente au plus"
    );
  }

  #[test]
  fn a_slow_image_fades_to_empty_after_the_limit_then_fades_in() {
    let mut app = banner_app();
    let t0 = Instant::now();
    compute_blurs(&mut app);
    app.sync_layers(t0);
    next_title(&mut app, Some("https://exemple.org/lent.jpg"));
    app.sync_layers(t0 + ms(100));
    app.sync_layers(t0 + PENDING_LIMIT + ms(99));
    assert!(app.visuals.banner.current.is_some(), "avant le délai");
    app.sync_layers(t0 + PENDING_LIMIT + ms(100));
    assert!(app.visuals.banner.current.is_none());
    assert!(
      app.visuals.banner.fade.as_ref().unwrap().from.is_some(),
      "fondu vers le vide"
    );
    assert!(app.animation_needed());
    app.sync_layers(t0 + PENDING_LIMIT + ms(500));
    assert!(app.visuals.banner.fade.is_none() && !app.animation_needed());
    load(&mut app, "https://exemple.org/lent.jpg");
    compute_blurs(&mut app);
    app.sync_layers(t0 + PENDING_LIMIT + ms(600));
    let fade = app.visuals.banner.fade.as_ref().expect("fondu d'entrée");
    assert!(fade.from.is_none() && app.visuals.banner.current.is_some());
  }

  #[test]
  fn a_title_without_a_cover_fades_to_empty_at_once() {
    let mut app = banner_app();
    let t0 = Instant::now();
    compute_blurs(&mut app);
    app.sync_layers(t0);
    next_title(&mut app, None);
    app.sync_layers(t0 + ms(10));
    assert!(app.visuals.banner.current.is_none());
    assert!(
      app
        .visuals
        .banner
        .fade
        .as_ref()
        .is_some_and(|fade| fade.from.is_some())
    );
    app.sync_layers(t0 + ms(400));
    assert!(app.visuals.banner.fade.is_none() && !app.animation_needed());
  }

  #[test]
  fn the_raw_banner_without_blur_fades_too() {
    let mut app = banner_app();
    app.settings.visual.banner_blur = 0;
    let t0 = Instant::now();
    app.sync_layers(t0);
    let old = id(&app.visuals.banner.current);
    assert!(old.is_some(), "image brute montrée sans passer par blurred");
    next_title(&mut app, Some("https://exemple.org/q.jpg"));
    app.sync_layers(t0 + ms(10));
    assert_eq!(id(&app.visuals.banner.current), old);
    load(&mut app, "https://exemple.org/q.jpg");
    app.sync_layers(t0 + ms(20));
    assert_eq!(id(&app.visuals.banner.fade.as_ref().unwrap().from), old);
    app.sync_layers(t0 + ms(400));
    assert!(app.visuals.banner.fade.is_none() && !app.animation_needed());
  }

  #[test]
  fn the_cover_and_full_screen_backgrounds_fade_on_a_new_title() {
    let mut app = app();
    app.settings.visual.cover_bg = true;
    app.lyrics.tvk_open = true;
    load(&mut app, "https://exemple.org/p.jpg");
    let t0 = Instant::now();
    compute_blurs(&mut app);
    app.sync_layers(t0);
    let before = (
      id(&app.visuals.cover_bg.current),
      id(&app.visuals.tvk_orbs[0].current),
    );
    assert!(before.0.is_some() && before.1.is_some());
    next_title(&mut app, Some("https://exemple.org/q.jpg"));
    load(&mut app, "https://exemple.org/q.jpg");
    compute_blurs(&mut app);
    app.sync_layers(t0 + ms(50));
    assert_eq!(
      id(&app.visuals.cover_bg.fade.as_ref().unwrap().from),
      before.0
    );
    assert_eq!(
      id(&app.visuals.tvk_orbs[0].fade.as_ref().unwrap().from),
      before.1
    );
    app.sync_layers(t0 + ms(50) + ms(699));
    assert!(app.visuals.cover_bg.fade.is_some(), "700 ms");
    assert!(app.visuals.tvk_orbs[0].fade.is_none(), "600 ms");
    app.sync_layers(t0 + ms(50) + ms(700));
    assert!(!app.visuals.layers_active());
  }

  #[test]
  fn a_custom_background_keeps_the_old_blur_without_fading() {
    let mut app = app();
    app.settings.visual.bg_url = "fond.png".into();
    app.settings.visual.bg_blur = 8;
    app
      .images
      .insert(ImageKey::new("fond.png", images::SIZE_HERO), decoded());
    let t0 = Instant::now();
    compute_blurs(&mut app);
    app.sync_layers(t0);
    let old = id(&app.visuals.custom_bg.current);
    assert!(old.is_some());
    app.settings.visual.bg_blur = 12;
    let jobs = app.take_blur_jobs();
    assert_eq!(id(&app.visuals.custom_bg.current), old);
    app.blur_ready(&jobs[0].key, Some(decoded()));
    app.sync_layers(t0 + ms(10));
    assert_ne!(id(&app.visuals.custom_bg.current), old);
    assert!(app.visuals.custom_bg.fade.is_none() && !app.animation_needed());
  }

  #[test]
  fn turning_a_setting_off_removes_the_visual_at_once() {
    let mut app = banner_app();
    let t0 = Instant::now();
    compute_blurs(&mut app);
    app.sync_layers(t0);
    next_title(&mut app, Some("https://exemple.org/q.jpg"));
    app.sync_layers(t0 + ms(10));
    app.settings.visual.banners = false;
    app.sync_layers(t0 + ms(20));
    assert!(app.visuals.banner.current.is_none() && app.visuals.banner.fade.is_none());
    assert!(!app.animation_needed(), "attente abandonnée");
    app.settings.visual.banners = true;
    app.lyrics.panel_open = false;
    app.sync_layers(t0 + ms(30));
    assert!(app.visuals.banner.current.is_none(), "panneau fermé");
    app.settings.visual.cover_bg = true;
    load(&mut app, "https://exemple.org/q.jpg");
    compute_blurs(&mut app);
    app.sync_layers(t0 + ms(40));
    assert!(app.visuals.cover_bg.current.is_some());
    app.player.current = None;
    app.sync_layers(t0 + ms(50));
    assert!(app.visuals.cover_bg.current.is_none(), "plus de titre");
  }

  #[test]
  fn the_crossfade_keeps_the_opacity_at_both_ends_and_the_curves_are_those_of_riff() {
    assert_eq!(crossfade_opacities(0.6, 0.0), (0.6, 0.0));
    assert_eq!(crossfade_opacities(0.6, 1.0), (0.0, 0.6));
    assert_eq!(crossfade_opacities(1.0, 1.0), (0.0, 1.0));
    let (below, top) = crossfade_opacities(1.0, 0.25);
    assert!((below - 1.0).abs() < 1e-6 && (top - 0.25).abs() < 1e-6);
    let ease = LayerRole::CoverBackground.fade().1;
    assert!(
      (bezier_ease(ease, 0.5) - 0.8).abs() < 0.05,
      "ease : environ 0,8 à mi-course"
    );
    assert_eq!((bezier_ease(ease, 0.0), bezier_ease(ease, 1.0)), (0.0, 1.0));
    assert_eq!(LayerRole::Banner.fade().0, ms(300));
    assert_eq!(LayerRole::FullScreenBackground.fade().0, ms(600));
  }

  #[test]
  fn bezier_ease_keeps_the_overshooting_curves_of_the_toast() {
    // `dropletToastIn` : `cubic-bezier(0.22, 1.35, 0.36, 1)` dépasse 1 avant de revenir (y1 > 1).
    let droplet = [0.22, 1.35, 0.36, 1.0];
    let peak = (1..100)
      .map(|step| bezier_ease(droplet, step as f32 / 100.0))
      .fold(0.0_f32, f32::max);
    assert!(peak > 1.03 && peak < 1.1, "{peak}");
    // Point calculé à la main : s = 0,235 donne x = 0,15 et y = 0,697.
    assert!((bezier_ease(droplet, 0.15) - 0.697).abs() < 0.01);
    assert_eq!(
      (bezier_ease(droplet, 0.0), bezier_ease(droplet, 1.0)),
      (0.0, 1.0)
    );
    // `ease-in-out` de `g-dash` vaut 0,5 au milieu.
    assert!((bezier_ease([0.42, 0.0, 0.58, 1.0], 0.5) - 0.5).abs() < 0.01);
  }

  #[test]
  fn bezier_ease_keeps_the_karaoke_panel_curve_below_one() {
    // `tvk-panel` : `cubic-bezier(0.22, 1, 0.36, 1)` (y1 = y2 = 1) monte sans jamais dépasser 1 : y(s) = 1 - (1 - s)^3.
    let panel = [0.22, 1.0, 0.36, 1.0];
    let peak = (1..100)
      .map(|step| bezier_ease(panel, step as f32 / 100.0))
      .fold(0.0_f32, f32::max);
    assert!(peak <= 1.0, "{peak}");
    // s = 0,5 : x = 3 * 0,25 * 0,5 * 0,22 + 3 * 0,5 * 0,25 * 0,36 + 0,125 = 0,3425 et y = 1 - 0,125 = 0,875.
    assert!((bezier_ease(panel, 0.3425) - 0.875).abs() < 0.002);
    // Même point à la main pour la courbe à dépassement `(0.22, 1.35, 0.36, 1)` : y = 1,00625.
    assert!((bezier_ease([0.22, 1.35, 0.36, 1.0], 0.3425) - 1.00625).abs() < 0.002);
  }

  #[test]
  fn the_full_screen_view_uses_its_own_blur() {
    let mut app = app();
    app.settings.visual.tvk_bg_blur = 30;
    app.lyrics.tvk_open = true;
    assert_eq!(app.wanted_blurs()[0].key.sigma_tenths, 300);
  }

  #[test]
  fn a_custom_background_is_only_computed_when_blurred() {
    let mut app = app();
    app.settings.visual.bg_url = "fond.png".into();
    assert!(app.wanted_blurs().is_empty());
    assert_eq!(app.background_url(), Some("fond.png"));
    app.settings.visual.bg_blur = 8;
    assert_eq!(app.wanted_blurs()[0].source.size, images::SIZE_HERO);
    app.settings.visual.bg_url = "fond.webm".into();
    assert_eq!(app.background_url(), None, "vidéo non portée");
    assert!(app.wanted_blurs().is_empty());
  }

  #[test]
  fn the_banner_follows_the_priorities_of_riff() {
    let mut app = app();
    assert_eq!(app.banner_url(), None, "désactivée par défaut");
    app.settings.visual.track_banners = true;
    assert_eq!(
      app.banner_url().as_deref(),
      Some("https://exemple.org/p.jpg")
    );
    app.settings.visual.banner_media = "global.png".into();
    assert_eq!(app.banner_url().as_deref(), Some("global.png"));
    app.settings.visual.track_art =
      serde_json::json!({ "ytmusic:id1": { "banner": "propre.png" } });
    assert_eq!(app.banner_url().as_deref(), Some("propre.png"));
    app.settings.visual.track_art =
      serde_json::json!({ "ytmusic:id1": { "banner": "propre.webm" } });
    assert_eq!(app.banner_url(), None, "bannière animée non portée");
    app.settings.visual.banners = false;
    app.settings.visual.track_art = serde_json::json!({});
    assert_eq!(app.banner_url(), None);
  }

  #[test]
  fn a_custom_cover_replaces_the_cover_of_its_track_only() {
    let mut app = app();
    let current = track();
    assert_eq!(
      app.cover_of(&current).as_deref(),
      Some("https://exemple.org/p.jpg")
    );
    app.settings.visual.track_art = serde_json::json!({ "ytmusic:id1": { "cover": "propre.png" } });
    assert_eq!(app.cover_of(&current).as_deref(), Some("propre.png"));
    let other = Track {
      id: Some("id2".into()),
      thumbnail: Some("autre.png".into()),
      platform: Some("ytmusic".into()),
      ..Track::default()
    };
    assert_eq!(app.cover_of(&other).as_deref(), Some("autre.png"));
  }

  #[test]
  fn an_animated_cover_keeps_the_cover_of_the_track() {
    let mut app = app();
    for url in ["a.mp4", "a.webm", "a.GIF"] {
      app.settings.visual.track_art = serde_json::json!({ "ytmusic:id1": { "cover": url } });
      assert_eq!(
        app.cover_of(&track()).as_deref(),
        Some("https://exemple.org/p.jpg"),
        "{url}"
      );
    }
  }

  #[test]
  fn a_small_resize_keeps_the_computed_background() {
    let mut app = app();
    app.settings.visual.cover_bg = true;
    let before = app.cover_bg_key();
    app.window.size = Some(Size::new(1300.0, 815.0));
    assert_eq!(app.cover_bg_key(), before);
  }
}
