//! Client réseau unique du core. Aucune requête ne passe ailleurs : `clippy.toml` interdit de construire un client hors d'ici.
//!
//! Le contrôle se fait à deux niveaux, car aucun ne suffit seul :
//! - sur l'URL, à chaque saut (une IP littérale ne passe jamais par le résolveur DNS) ;
//! - sur l'adresse résolue, dans le résolveur (un nom public peut pointer vers une adresse privée).

pub mod allowlist;
#[cfg(feature = "replay")]
pub mod replay;

use crate::logging;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, StatusCode, redirect};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use url::{Host, Url};

const MAX_REDIRECTS: usize = 5;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const TOOLS_TIMEOUT: Duration = Duration::from_secs(25);
const SERVICE_MAX_BYTES: u64 = 64 * 1024 * 1024;
const IMAGE_MAX_BYTES: u64 = 10 * 1024 * 1024;
const LOCAL_MAX_BYTES: u64 = 16 * 1024 * 1024;
const STREAM_MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Ce qu'un client a le droit de contacter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
  /// Hôtes de [`allowlist`] uniquement, redirections comprises.
  Service,
  /// N'importe quel hôte public : adresses privées refusées, 10 Mo au plus, type `image/*`.
  PublicImage,
  /// Réseau privé et boucle locale uniquement, jamais un hôte public.
  Local,
}

impl Profile {
  const fn label(self) -> &'static str {
    match self {
      Self::Service => "service",
      Self::PublicImage => "public-image",
      Self::Local => "local",
    }
  }
}

/// Raison d'un refus par la politique réseau.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Blocked {
  #[error("unsupported scheme")]
  Scheme,
  #[error("missing host")]
  NoHost,
  #[error("private address refused: {0}")]
  PrivateAddress(String),
  #[error("host not allowed: {0}")]
  HostNotAllowed(String),
  #[error("public host refused by the local profile: {0}")]
  NotLocal(String),
  #[error("too many redirects")]
  TooManyRedirects,
}

/// Erreur d'une requête. Le message ne contient jamais l'URL (chemin et paramètres peuvent porter des jetons).
#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error(transparent)]
  Blocked(#[from] Blocked),
  #[error("invalid url")]
  InvalidUrl,
  #[error("response larger than {limit} bytes")]
  TooLarge { limit: u64 },
  #[error("response is not an image")]
  NotImage,
  #[error("invalid json: {0}")]
  Decode(#[from] serde_json::Error),
  #[error("{0}")]
  Request(reqwest::Error),
  /// Fixture absente, illisible ou refusée à l'enregistrement. Le message porte l'URL : réservé aux tests.
  #[cfg(feature = "replay")]
  #[error("replay: {0}")]
  Replay(String),
}

impl Error {
  /// Vrai si le délai de la requête a expiré.
  #[must_use]
  pub fn is_timeout(&self) -> bool {
    matches!(self, Self::Request(e) if e.is_timeout())
  }

  /// Vrai si la connexion a échoué.
  #[must_use]
  pub fn is_connect(&self) -> bool {
    matches!(self, Self::Request(e) if e.is_connect())
  }

  /// Vrai si l'envoi de la requête a échoué.
  #[must_use]
  pub fn is_request(&self) -> bool {
    matches!(self, Self::Request(e) if e.is_request())
  }

  /// Vrai si la politique réseau a refusé la requête.
  #[must_use]
  pub fn is_blocked(&self) -> bool {
    matches!(self, Self::Blocked(_))
  }
}

impl From<reqwest::Error> for Error {
  // Résolveur et redirections signalent un refus par un `Blocked` enfoui dans la chaîne d'erreurs de reqwest.
  fn from(err: reqwest::Error) -> Self {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&err);
    while let Some(current) = source {
      if let Some(blocked) = current.downcast_ref::<Blocked>() {
        return Self::Blocked(blocked.clone());
      }
      source = current.source();
    }
    Self::Request(err.without_url())
  }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
  let [a, b, c, _] = ip.octets();
  ip.is_loopback()
    || ip.is_private()
    || ip.is_link_local()
    || ip.is_broadcast()
    || ip.is_multicast()
    || a == 0
    || a >= 240
    || (a == 100 && (64..128).contains(&b))
    || (a == 192 && b == 0 && c == 0)
}

// IPv4 embarquée : mappée `::ffff:0:0/96`, compatible `::/96`, NAT64 `64:ff9b::/96`, 6to4 `2002::/16`.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
  let s = ip.segments();
  let from = |hi: u16, lo: u16| Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo));
  match s {
    [0, 0, 0, 0, 0, 0xffff, hi, lo]
    | [0, 0, 0, 0, 0, 0, hi, lo]
    | [0x64, 0xff9b, 0, 0, 0, 0, hi, lo] => Some(from(hi, lo)),
    [0x2002, hi, lo, ..] => Some(from(hi, lo)),
    _ => None,
  }
}

/// Vrai pour toute adresse qui n'est pas publique : boucle locale, réseaux privés, lien local, CGNAT, multidiffusion,
/// non spécifiée, et les formes IPv6 qui embarquent une telle adresse IPv4.
#[must_use]
pub fn is_blocked_ip(ip: IpAddr) -> bool {
  match ip {
    IpAddr::V4(v4) => is_blocked_v4(v4),
    IpAddr::V6(v6) => {
      v6.is_loopback()
        || v6.is_unspecified()
        || v6.is_unique_local()
        || v6.is_unicast_link_local()
        || v6.is_multicast()
        || embedded_v4(v6).is_some_and(is_blocked_v4)
    }
  }
}

fn is_localhost(domain: &str) -> bool {
  let d = domain.trim_end_matches('.').to_ascii_lowercase();
  d == "localhost" || d.ends_with(".localhost")
}

/// Applique la politique d'un profil à une URL (à chaque saut, redirections comprises).
fn check_url(profile: Profile, allowed: fn(&str) -> bool, url: &Url) -> Result<(), Blocked> {
  if !matches!(url.scheme(), "http" | "https") {
    return Err(Blocked::Scheme);
  }
  let host = url.host().ok_or(Blocked::NoHost)?;
  let literal = match &host {
    Host::Ipv4(ip) => Some(IpAddr::V4(*ip)),
    Host::Ipv6(ip) => Some(IpAddr::V6(*ip)),
    Host::Domain(_) => None,
  };
  let name = host.to_string();
  let private = literal.map_or_else(|| is_localhost(&name), is_blocked_ip);
  match profile {
    Profile::Local if private => Ok(()),
    Profile::Local => Err(Blocked::NotLocal(name)),
    _ if private => Err(Blocked::PrivateAddress(name)),
    Profile::PublicImage => Ok(()),
    Profile::Service if literal.is_none() && allowed(&name) => Ok(()),
    Profile::Service => Err(Blocked::HostNotAllowed(name)),
  }
}

/// Journalise un hôte contacté si `RIFF_NET_LOG=1`. Jamais le chemin ni les paramètres.
fn record(label: &str, host: &str) {
  if std::env::var("RIFF_NET_LOG").is_ok_and(|v| v == "1") {
    logging::log_entry("NET", &format!("{label} {host}"));
  }
}

/// Journalise le trafic d'un processus externe (yt-dlp), qui échappe à ce client.
pub fn record_external(host: &str) {
  record("external", host);
}

fn record_url(profile: Profile, url: &Url) {
  record(profile.label(), url.host_str().unwrap_or(""));
}

struct SystemResolver;

impl Resolve for SystemResolver {
  fn resolve(&self, name: Name) -> Resolving {
    Box::pin(async move {
      let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
      Ok(Box::new(addrs.into_iter()) as Addrs)
    })
  }
}

/// Retire les adresses non publiques d'une résolution. Échoue si aucune ne reste.
struct PublicOnlyResolver {
  inner: Arc<dyn Resolve>,
}

impl Resolve for PublicOnlyResolver {
  fn resolve(&self, name: Name) -> Resolving {
    let inner = Arc::clone(&self.inner);
    Box::pin(async move {
      let host = name.as_str().to_string();
      let kept: Vec<SocketAddr> = inner
        .resolve(name)
        .await?
        .filter(|a| !is_blocked_ip(a.ip()))
        .collect();
      if kept.is_empty() {
        return Err(
          Box::new(Blocked::PrivateAddress(host)) as Box<dyn std::error::Error + Send + Sync>
        );
      }
      Ok(Box::new(kept.into_iter()) as Addrs)
    })
  }
}

#[derive(Clone)]
struct Inner {
  client: reqwest::Client,
  profile: Profile,
  allowed: fn(&str) -> bool,
  max_bytes: u64,
  /// Transport de remplacement : si présent, `send` lit des fixtures au lieu d'ouvrir une connexion.
  #[cfg(feature = "replay")]
  replay: Option<replay::Replay>,
}

/// Réglages de construction d'un client.
struct Config {
  profile: Profile,
  allowed: fn(&str) -> bool,
  max_bytes: u64,
  follow_redirects: bool,
  use_proxy: bool,
  ipv4_http1: bool,
  connect_timeout: Duration,
  idle_timeout: Option<Duration>,
  user_agent: Option<&'static str>,
  resolver: Arc<dyn Resolve>,
  #[cfg(test)]
  overrides: Vec<(&'static str, SocketAddr)>,
}

impl Config {
  fn new(profile: Profile) -> Self {
    Self {
      profile,
      allowed: allowlist::is_allowed,
      max_bytes: SERVICE_MAX_BYTES,
      follow_redirects: true,
      use_proxy: false,
      ipv4_http1: false,
      connect_timeout: CONNECT_TIMEOUT,
      idle_timeout: Some(IDLE_TIMEOUT),
      user_agent: None,
      resolver: Arc::new(SystemResolver),
      #[cfg(test)]
      overrides: Vec::new(),
    }
  }
}

/// Client HTTP du core. Se clone sans coût.
#[derive(Clone)]
pub struct Http {
  inner: Arc<Inner>,
}

impl Http {
  // Seul endroit du dépôt qui construit un client reqwest : `clippy.toml` l'interdit partout ailleurs pour que toute
  // requête subisse la politique de ce module. Échec = TLS indisponible, le core ne peut rien faire sans réseau
  // (l'ancien repli sur `Client::new` paniquait dans ce même cas).
  #[allow(clippy::disallowed_methods)]
  fn build(config: Config) -> Self {
    let Config {
      profile,
      allowed,
      max_bytes,
      follow_redirects,
      use_proxy,
      ipv4_http1,
      connect_timeout,
      idle_timeout,
      user_agent,
      resolver,
      ..
    } = config;
    let policy = if follow_redirects {
      redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
          return attempt.error(Blocked::TooManyRedirects);
        }
        match check_url(profile, allowed, attempt.url()) {
          Ok(()) => {
            record_url(profile, attempt.url());
            attempt.follow()
          }
          Err(blocked) => attempt.error(blocked),
        }
      })
    } else {
      redirect::Policy::none()
    };
    let mut builder = reqwest::ClientBuilder::new()
      .redirect(policy)
      .referer(false)
      .connect_timeout(connect_timeout);
    if profile != Profile::Local {
      builder = builder.dns_resolver(Arc::new(PublicOnlyResolver { inner: resolver }));
    }
    if !use_proxy {
      builder = builder.no_proxy();
    }
    if ipv4_http1 {
      builder = builder
        .local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        .http1_only();
    }
    if let Some(idle) = idle_timeout {
      builder = builder.read_timeout(idle);
    }
    if let Some(ua) = user_agent {
      builder = builder.user_agent(ua);
    }
    #[cfg(test)]
    for (domain, addr) in &config.overrides {
      builder = builder.resolve(domain, *addr);
    }
    let client = builder
      .build()
      .expect("TLS indisponible : client réseau impossible à construire");
    Self {
      inner: Arc::new(Inner {
        client,
        profile,
        allowed,
        max_bytes,
        #[cfg(feature = "replay")]
        replay: replay::Replay::from_env(),
      }),
    }
  }

  /// Remplace le transport réseau par un rejeu de fixtures. La politique du profil s'applique toujours avant.
  #[cfg(feature = "replay")]
  #[must_use]
  pub fn with_replay(&self, replay: replay::Replay) -> Self {
    Self {
      inner: Arc::new(Inner {
        replay: Some(replay),
        ..(*self.inner).clone()
      }),
    }
  }

  /// Profil `Service`. Garde le proxy système : le contrôle sur l'hôte suffit à la liste d'autorisation.
  #[must_use]
  pub fn service() -> Self {
    Self::build(Config {
      use_proxy: true,
      ..Config::new(Profile::Service)
    })
  }

  /// Profil `Service` pour les flux YouTube : IPv4 uniquement (les URL googlevideo sont liées à l'IP qui les a
  /// demandées), sans proxy système, sans redirection automatique (l'appelant revalide chaque saut).
  #[must_use]
  pub fn stream() -> Self {
    Self::build(Config {
      follow_redirects: false,
      ipv4_http1: true,
      ..Config::new(Profile::Service)
    })
  }

  /// Profil `PublicImage`. Sans proxy : un proxy résoudrait le nom lui-même, hors du filtre d'adresses.
  #[must_use]
  pub fn public_image() -> Self {
    Self::build(Config {
      max_bytes: IMAGE_MAX_BYTES,
      ..Config::new(Profile::PublicImage)
    })
  }

  /// Profil `Local`. Sans délai d'inactivité : un flux local (SSE) peut rester muet longtemps.
  #[must_use]
  pub fn local() -> Self {
    Self::build(Config {
      max_bytes: LOCAL_MAX_BYTES,
      idle_timeout: None,
      ..Config::new(Profile::Local)
    })
  }

  /// Profil `Service` restreint aux origines GitHub, pour télécharger yt-dlp et ffmpeg.
  #[must_use]
  pub fn tools(user_agent: &'static str) -> Self {
    Self::build(Config {
      allowed: allowlist::is_tool_origin,
      use_proxy: true,
      connect_timeout: TOOLS_TIMEOUT,
      idle_timeout: Some(TOOLS_TIMEOUT),
      user_agent: Some(user_agent),
      ..Config::new(Profile::Service)
    })
  }

  /// Vérifie qu'une URL passe la politique de ce client, sans rien envoyer.
  ///
  /// # Errors
  /// [`Blocked`] si l'URL est refusée.
  pub fn check(&self, url: &Url) -> Result<(), Blocked> {
    check_url(self.inner.profile, self.inner.allowed, url)
  }

  #[must_use]
  pub fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
    self.request(Method::GET, url)
  }

  #[must_use]
  pub fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
    self.request(Method::POST, url)
  }

  #[must_use]
  pub fn request(&self, method: Method, url: impl AsRef<str>) -> RequestBuilder {
    let url = Url::parse(url.as_ref()).map_err(|_| Error::InvalidUrl);
    let inner = url
      .as_ref()
      .ok()
      .map(|u| self.inner.client.request(method, u.clone()));
    RequestBuilder {
      http: self.clone(),
      url,
      inner,
      explicit_timeout: false,
      streaming: false,
      max_bytes: self.inner.max_bytes,
    }
  }
}

/// Requête en construction. La politique s'applique à l'envoi.
pub struct RequestBuilder {
  http: Http,
  url: Result<Url, Error>,
  inner: Option<reqwest::RequestBuilder>,
  explicit_timeout: bool,
  streaming: bool,
  max_bytes: u64,
}

impl RequestBuilder {
  fn map(mut self, f: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder) -> Self {
    self.inner = self.inner.take().map(f);
    self
  }

  #[must_use]
  pub fn header<K, V>(self, key: K, value: V) -> Self
  where
    HeaderName: TryFrom<K>,
    <HeaderName as TryFrom<K>>::Error: Into<http::Error>,
    HeaderValue: TryFrom<V>,
    <HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
  {
    self.map(|r| r.header(key, value))
  }

  #[must_use]
  pub fn body(self, body: impl Into<reqwest::Body>) -> Self {
    self.map(|r| r.body(body))
  }

  #[must_use]
  pub fn json<T: Serialize + ?Sized>(self, json: &T) -> Self {
    self.map(|r| r.json(json))
  }

  #[must_use]
  pub fn bearer_auth(self, token: impl std::fmt::Display) -> Self {
    self.map(|r| r.bearer_auth(token))
  }

  /// Délai total de la requête, corps compris. 30 s si non précisé.
  #[must_use]
  pub fn timeout(mut self, timeout: Duration) -> Self {
    self.explicit_timeout = true;
    self.map(|r| r.timeout(timeout))
  }

  /// Corps long lu au fil de l'eau : pas de délai total, seul le délai d'inactivité du client s'applique.
  #[must_use]
  pub fn streaming(mut self) -> Self {
    self.streaming = true;
    self.max_bytes = STREAM_MAX_BYTES;
    self
  }

  /// Plafond de taille du corps de la réponse.
  #[must_use]
  pub fn max_bytes(mut self, limit: u64) -> Self {
    self.max_bytes = limit;
    self
  }

  /// Copie pour un nouvel essai. `None` si le corps n'est pas copiable.
  #[must_use]
  pub fn try_clone(&self) -> Option<Self> {
    let url = match &self.url {
      Ok(u) => Ok(u.clone()),
      Err(_) => Err(Error::InvalidUrl),
    };
    let inner = match &self.inner {
      Some(r) => Some(r.try_clone()?),
      None => None,
    };
    Some(Self {
      http: self.http.clone(),
      url,
      inner,
      explicit_timeout: self.explicit_timeout,
      streaming: self.streaming,
      max_bytes: self.max_bytes,
    })
  }

  /// Applique la politique réseau puis envoie la requête.
  ///
  /// # Errors
  /// [`Error::Blocked`] si la politique refuse la cible ou un saut de redirection, [`Error::NotImage`] pour une
  /// réponse réussie sans type `image/*` en profil `PublicImage`, [`Error::Request`] pour une erreur de transport.
  pub async fn send(self) -> Result<Response, Error> {
    let url = self.url?;
    self.http.check(&url)?;
    let inner = self.inner.ok_or(Error::InvalidUrl)?;
    let profile = self.http.inner.profile;
    record_url(profile, &url);
    let inner = if self.explicit_timeout || self.streaming {
      inner
    } else {
      inner.timeout(DEFAULT_TIMEOUT)
    };
    let request = inner.build()?;
    #[cfg(feature = "replay")]
    let response = match &self.http.inner.replay {
      Some(replay) => {
        replay
          .execute(&self.http.inner.client, request, self.max_bytes)
          .await?
      }
      None => self.http.inner.client.execute(request).await?,
    };
    #[cfg(not(feature = "replay"))]
    let response = self.http.inner.client.execute(request).await?;
    if profile == Profile::PublicImage
      && response.status().is_success()
      && !is_image(response.headers())
    {
      return Err(Error::NotImage);
    }
    Ok(Response {
      inner: response,
      max_bytes: self.max_bytes,
      read: 0,
    })
  }
}

fn is_image(headers: &HeaderMap) -> bool {
  headers
    .get(reqwest::header::CONTENT_TYPE)
    .and_then(|v| v.to_str().ok())
    .is_some_and(|t| {
      t.trim_start()
        .get(..6)
        .is_some_and(|p| p.eq_ignore_ascii_case("image/"))
    })
}

/// Réponse dont le corps ne peut pas dépasser le plafond de la requête.
pub struct Response {
  inner: reqwest::Response,
  max_bytes: u64,
  read: u64,
}

impl Response {
  #[must_use]
  pub fn status(&self) -> StatusCode {
    self.inner.status()
  }

  #[must_use]
  pub fn headers(&self) -> &HeaderMap {
    self.inner.headers()
  }

  /// URL finale, après redirections.
  #[must_use]
  pub fn url(&self) -> &Url {
    self.inner.url()
  }

  #[must_use]
  pub fn content_length(&self) -> Option<u64> {
    self.inner.content_length()
  }

  /// Morceau suivant du corps.
  ///
  /// # Errors
  /// [`Error::TooLarge`] au-delà du plafond, [`Error::Request`] pour une erreur de transport.
  pub async fn chunk(&mut self) -> Result<Option<Bytes>, Error> {
    let Some(chunk) = self.inner.chunk().await? else {
      return Ok(None);
    };
    self.read = self.read.saturating_add(chunk.len() as u64);
    if self.read > self.max_bytes {
      return Err(Error::TooLarge {
        limit: self.max_bytes,
      });
    }
    Ok(Some(chunk))
  }

  /// Corps entier en mémoire.
  ///
  /// # Errors
  /// Voir [`Response::chunk`].
  pub async fn bytes(mut self) -> Result<Bytes, Error> {
    if self
      .inner
      .content_length()
      .is_some_and(|n| n > self.max_bytes)
    {
      return Err(Error::TooLarge {
        limit: self.max_bytes,
      });
    }
    let mut body = Vec::new();
    while let Some(chunk) = self.chunk().await? {
      body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
  }

  /// Corps en texte, décodé selon le jeu de caractères de `Content-Type` (mêmes règles que reqwest).
  ///
  /// # Errors
  /// Voir [`Response::chunk`].
  pub async fn text(self) -> Result<String, Error> {
    let headers = self.inner.headers().clone();
    let body = self.bytes().await?;
    let mut rebuilt = http::Response::new(body);
    *rebuilt.headers_mut() = headers;
    Ok(reqwest::Response::from(rebuilt).text().await?)
  }

  /// Corps décodé depuis du JSON.
  ///
  /// # Errors
  /// Voir [`Response::chunk`] ; [`Error::Decode`] si le JSON est invalide.
  pub async fn json<T: DeserializeOwned>(self) -> Result<T, Error> {
    Ok(serde_json::from_slice(&self.bytes().await?)?)
  }

  /// Corps au fil de l'eau. S'arrête sur [`Error::TooLarge`] au-delà du plafond.
  pub fn bytes_stream(self) -> impl Stream<Item = Result<Bytes, Error>> {
    let limit = self.max_bytes;
    let mut read = 0u64;
    self.inner.bytes_stream().map(move |chunk| {
      let chunk = chunk?;
      read = read.saturating_add(chunk.len() as u64);
      if read > limit {
        return Err(Error::TooLarge { limit });
      }
      Ok(chunk)
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  use tokio::net::TcpListener;

  fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
  }

  fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
  }

  #[test]
  fn private_and_special_addresses_are_blocked() {
    for s in [
      "127.0.0.1",
      "127.255.0.1",
      "10.0.0.1",
      "172.16.0.1",
      "172.31.255.255",
      "192.168.1.1",
      "169.254.169.254",
      "100.64.0.1",
      "100.127.255.255",
      "0.0.0.0",
      "0.1.2.3",
      "255.255.255.255",
      "224.0.0.1",
      "240.0.0.1",
      "::1",
      "::",
      "fc00::1",
      "fd12::1",
      "fe80::1",
      "ff02::1",
      "::ffff:127.0.0.1",
      "::ffff:10.0.0.1",
      "::ffff:169.254.169.254",
      "::127.0.0.1",
      "64:ff9b::7f00:1",
      "2002:7f00:1::",
    ] {
      assert!(is_blocked_ip(ip(s)), "{s} devrait être bloquée");
    }
  }

  #[test]
  fn public_addresses_are_accepted() {
    for s in [
      "8.8.8.8",
      "93.184.216.34",
      "172.32.0.1",
      "100.128.0.1",
      "2606:4700:4700::1111",
      "::ffff:8.8.8.8",
      "64:ff9b::808:808",
    ] {
      assert!(!is_blocked_ip(ip(s)), "{s} devrait être acceptée");
    }
  }

  #[test]
  fn public_image_refuses_private_literals_whatever_their_spelling() {
    let allow = allowlist::is_allowed;
    for s in [
      "http://127.0.0.1/",
      "http://10.0.0.1/",
      "http://169.254.169.254/latest/meta-data",
      "http://[::1]/",
      "http://[::ffff:127.0.0.1]/",
      "http://localhost/",
      "http://LOCALHOST./",
      "http://a.localhost/",
      "http://2130706433/",
      "http://0x7f.0.0.1/",
      "http://0177.0.0.1/",
    ] {
      assert!(
        check_url(Profile::PublicImage, allow, &url(s)).is_err(),
        "{s} devrait être refusée"
      );
    }
    assert_eq!(
      check_url(
        Profile::PublicImage,
        allow,
        &url("https://93.184.216.34/a.jpg")
      ),
      Ok(())
    );
    assert_eq!(
      check_url(
        Profile::PublicImage,
        allow,
        &url("https://images.example/a.jpg")
      ),
      Ok(())
    );
    assert_eq!(
      check_url(
        Profile::PublicImage,
        allow,
        &url("ftp://images.example/a.jpg")
      ),
      Err(Blocked::Scheme)
    );
  }

  #[test]
  fn service_refuses_unlisted_hosts_and_private_literals() {
    let allow = allowlist::is_allowed;
    assert_eq!(
      check_url(
        Profile::Service,
        allow,
        &url("https://api.deezer.com/search")
      ),
      Ok(())
    );
    assert_eq!(
      check_url(Profile::Service, allow, &url("https://evil.example/")),
      Err(Blocked::HostNotAllowed("evil.example".to_string()))
    );
    assert!(check_url(Profile::Service, allow, &url("http://127.0.0.1:38472/x")).is_err());
    assert!(check_url(Profile::Service, allow, &url("http://93.184.216.34/x")).is_err());
  }

  #[test]
  fn local_accepts_only_private_targets() {
    let allow = allowlist::is_allowed;
    for s in [
      "http://127.0.0.1:9/",
      "http://[::1]:9/",
      "http://localhost:9/",
      "http://192.168.1.2:1080/",
    ] {
      assert_eq!(check_url(Profile::Local, allow, &url(s)), Ok(()), "{s}");
    }
    for s in ["https://8.8.8.8/", "https://api.deezer.com/"] {
      assert!(
        matches!(
          check_url(Profile::Local, allow, &url(s)),
          Err(Blocked::NotLocal(_))
        ),
        "{s}"
      );
    }
  }

  struct StaticResolver(Vec<SocketAddr>);

  impl Resolve for StaticResolver {
    fn resolve(&self, _name: Name) -> Resolving {
      let addrs = self.0.clone();
      Box::pin(async move { Ok(Box::new(addrs.into_iter()) as Addrs) })
    }
  }

  fn addr(s: &str) -> SocketAddr {
    SocketAddr::new(ip(s), 0)
  }

  async fn resolve_with(addrs: &[&str]) -> Result<Vec<SocketAddr>, String> {
    let inner = Arc::new(StaticResolver(addrs.iter().map(|a| addr(a)).collect()));
    let resolved = PublicOnlyResolver { inner }
      .resolve("host.example".parse().unwrap())
      .await;
    resolved.map(Iterator::collect).map_err(|e| e.to_string())
  }

  #[tokio::test]
  async fn a_name_resolving_only_to_private_addresses_is_refused() {
    for private in [
      "127.0.0.1",
      "10.0.0.1",
      "169.254.169.254",
      "::1",
      "::ffff:127.0.0.1",
    ] {
      let err = resolve_with(&[private]).await.unwrap_err();
      assert!(err.contains("private address refused"), "{private} : {err}");
    }
  }

  #[tokio::test]
  async fn a_public_resolution_is_accepted_and_private_entries_are_dropped() {
    assert_eq!(
      resolve_with(&["93.184.216.34"]).await.unwrap(),
      vec![addr("93.184.216.34")]
    );
    assert_eq!(
      resolve_with(&["10.0.0.1", "93.184.216.34", "::1"])
        .await
        .unwrap(),
      vec![addr("93.184.216.34")]
    );
  }

  /// Le résolveur de test renvoie du privé : la requête doit échouer en `Blocked`, sans rien contacter.
  #[tokio::test]
  async fn a_request_to_a_domain_resolving_to_private_is_blocked() {
    let http = Http::build(Config {
      resolver: Arc::new(StaticResolver(vec![addr("127.0.0.1")])),
      ..Config::new(Profile::PublicImage)
    });
    let err = http
      .get("http://rebind.example/a.jpg")
      .send()
      .await
      .err()
      .unwrap();
    assert!(
      matches!(&err, Error::Blocked(Blocked::PrivateAddress(h)) if h == "rebind.example"),
      "{err:?}"
    );
  }

  /// Sert des réponses brutes dans l'ordre, une par connexion. Renvoie le port.
  async fn serve(responses: Vec<String>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
      for response in responses {
        let Ok((mut stream, _)) = listener.accept().await else {
          return;
        };
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).await;
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
      }
    });
    port
  }

  fn redirect_to(location: &str) -> String {
    format!(
      "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
  }

  fn ok(content_type: &str, body: &str) -> String {
    format!(
      "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
      body.len()
    )
  }

  /// `ok.example` pointe vers le serveur local de test ; le filtre ne voit que les autres noms.
  async fn image_client_for(responses: Vec<String>) -> (Http, u16) {
    let port = serve(responses).await;
    let overrides = vec![("ok.example", SocketAddr::new(ip("127.0.0.1"), port))];
    (
      Http::build(Config {
        overrides,
        max_bytes: IMAGE_MAX_BYTES,
        ..Config::new(Profile::PublicImage)
      }),
      port,
    )
  }

  #[tokio::test]
  async fn a_redirect_to_a_private_address_is_blocked() {
    let (http, port) = image_client_for(vec![redirect_to("http://127.0.0.1/secret")]).await;
    let err = http
      .get(format!("http://ok.example:{port}/a.jpg"))
      .send()
      .await
      .err()
      .unwrap();
    assert!(err.is_blocked(), "{err:?}");
  }

  #[tokio::test]
  async fn more_than_five_redirects_are_refused_and_five_are_followed() {
    let hop =
      |n: usize, port_hint: &str| redirect_to(&format!("http://ok.example:{port_hint}/{n}"));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let p = port.to_string();
    let run = |hops: usize| {
      let mut responses: Vec<String> = (0..hops).map(|n| hop(n, &p)).collect();
      responses.push(ok("image/png", "image"));
      responses
    };
    let (five, six) = (run(5), run(6));
    tokio::spawn(async move {
      for response in five.into_iter().chain(six) {
        let Ok((mut stream, _)) = listener.accept().await else {
          return;
        };
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).await;
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
      }
    });
    let overrides = vec![("ok.example", SocketAddr::new(ip("127.0.0.1"), port))];
    let http = Http::build(Config {
      overrides,
      ..Config::new(Profile::PublicImage)
    });
    let start = format!("http://ok.example:{port}/start");
    assert_eq!(http.get(&start).send().await.unwrap().status(), 200);
    let err = http.get(&start).send().await.err().unwrap();
    assert!(
      matches!(err, Error::Blocked(Blocked::TooManyRedirects)),
      "{err:?}"
    );
  }

  #[tokio::test]
  async fn service_refuses_a_redirect_to_an_unlisted_host() {
    let port = serve(vec![redirect_to("https://evil.example/x")]).await;
    let overrides = vec![("api.deezer.com", SocketAddr::new(ip("127.0.0.1"), port))];
    let http = Http::build(Config {
      overrides,
      ..Config::new(Profile::Service)
    });
    let err = http
      .get(format!("http://api.deezer.com:{port}/"))
      .send()
      .await
      .err()
      .unwrap();
    assert!(
      matches!(&err, Error::Blocked(Blocked::HostNotAllowed(h)) if h == "evil.example"),
      "{err:?}"
    );
  }

  #[tokio::test]
  async fn service_sends_nothing_to_an_unlisted_host() {
    let err = Http::service()
      .get("https://evil.example/")
      .send()
      .await
      .err()
      .unwrap();
    assert!(
      matches!(&err, Error::Blocked(Blocked::HostNotAllowed(h)) if h == "evil.example"),
      "{err:?}"
    );
  }

  #[tokio::test]
  async fn public_image_refuses_a_response_that_is_not_an_image() {
    let (http, port) = image_client_for(vec![ok("text/html", "<html>")]).await;
    let err = http
      .get(format!("http://ok.example:{port}/a.jpg"))
      .send()
      .await
      .err()
      .unwrap();
    assert!(matches!(err, Error::NotImage), "{err:?}");
  }

  #[tokio::test]
  async fn public_image_accepts_an_image_and_passes_errors_through() {
    let (http, port) = image_client_for(vec![
      ok("IMAGE/png", "data"),
      "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
    ])
    .await;
    let target = format!("http://ok.example:{port}/a.jpg");
    assert_eq!(
      http
        .get(&target)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap(),
      Bytes::from_static(b"data")
    );
    assert_eq!(http.get(&target).send().await.unwrap().status(), 404);
  }

  #[tokio::test]
  async fn the_body_is_capped_with_and_without_a_declared_length() {
    let body = "x".repeat(2000);
    let unsized_reply =
      format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nConnection: close\r\n\r\n{body}");
    let (http, port) = image_client_for(vec![ok("image/png", &body), unsized_reply]).await;
    let target = format!("http://ok.example:{port}/a.jpg");
    for _ in 0..2 {
      let err = http
        .get(&target)
        .max_bytes(1000)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap_err();
      assert!(matches!(err, Error::TooLarge { limit: 1000 }), "{err:?}");
    }
  }

  #[tokio::test]
  async fn text_decodes_the_declared_charset() {
    let port = serve(vec![
      "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 3\r\nConnection: close\r\n\r\nété".replace("Length: 3", "Length: 5"),
    ])
    .await;
    let text = Http::local()
      .get(format!("http://127.0.0.1:{port}/"))
      .send()
      .await
      .unwrap()
      .text()
      .await
      .unwrap();
    assert_eq!(text, "été");
  }

  #[tokio::test]
  async fn no_referer_is_sent_across_a_redirect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
      let (mut first, _) = listener.accept().await.unwrap();
      let mut buf = [0u8; 4096];
      let _ = first.read(&mut buf).await;
      let reply = redirect_to(&format!("http://127.0.0.1:{port}/next"));
      first.write_all(reply.as_bytes()).await.unwrap();
      first.shutdown().await.unwrap();
      let (mut second, _) = listener.accept().await.unwrap();
      let n = second.read(&mut buf).await.unwrap();
      let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_lowercase());
      second
        .write_all(ok("text/plain", "ok").as_bytes())
        .await
        .unwrap();
    });
    Http::local()
      .get(format!("http://127.0.0.1:{port}/start"))
      .send()
      .await
      .unwrap();
    let second_request = rx.await.unwrap();
    assert!(second_request.starts_with("get /next"), "{second_request}");
    assert!(!second_request.contains("referer"), "{second_request}");
  }

  #[test]
  fn errors_do_not_expose_the_url() {
    let secret = "SENTINELLE";
    let http = Http::local();
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .unwrap();
    // Port 9 fermé : la connexion échoue, l'erreur de reqwest citerait l'URL sans `without_url`.
    let err = runtime
      .block_on(
        http
          .get(format!("http://127.0.0.1:9/?token={secret}"))
          .send(),
      )
      .err()
      .unwrap();
    assert!(!err.to_string().contains(secret), "{err}");
  }
}
