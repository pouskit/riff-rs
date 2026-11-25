//! Nettoyage des URL : retire les paramètres de suivi. Liste écrite pour ce projet, pas de règles tierces.

use url::Url;

const GLOBAL: &[&str] = &[
  "utm_source",
  "utm_medium",
  "utm_campaign",
  "utm_term",
  "utm_content",
  "utm_id",
  "fbclid",
  "gclid",
  "dclid",
  "msclkid",
  "mc_cid",
  "mc_eid",
  "igshid",
  "igsh",
  "yclid",
  "twclid",
  "_ga",
  "ref_src",
];
const YOUTUBE: &[&str] = &["si", "feature", "pp"];
const SPOTIFY: &[&str] = &["si", "context", "nd"];
const SOUNDCLOUD: &[&str] = &["si", "ref"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Platform {
  YouTube,
  Spotify,
  SoundCloud,
  Other,
}

fn platform_of(host: &str) -> Platform {
  let host = host.trim_end_matches('.');
  match host {
    "youtube.com" | "youtu.be" => Platform::YouTube,
    h if h.ends_with(".youtube.com") => Platform::YouTube,
    "open.spotify.com" | "play.spotify.com" | "spotify.com" | "www.spotify.com" => {
      Platform::Spotify
    }
    "soundcloud.com" | "www.soundcloud.com" | "m.soundcloud.com" | "on.soundcloud.com" => {
      Platform::SoundCloud
    }
    _ => Platform::Other,
  }
}

fn is_tracking(platform: Platform, key: &str) -> bool {
  let key = key.to_ascii_lowercase();
  let key = key.as_str();
  GLOBAL.contains(&key)
    || match platform {
      Platform::YouTube => YOUTUBE.contains(&key),
      Platform::Spotify => SPOTIFY.contains(&key) || key.starts_with("utm_"),
      Platform::SoundCloud => SOUNDCLOUD.contains(&key) || key.starts_with("utm_"),
      Platform::Other => false,
    }
}

// Les paires gardées restent celles de l'entrée, octet pour octet : ni réordonnées, ni réencodées.
fn is_tracking_pair(platform: Platform, pair: &str) -> bool {
  url::form_urlencoded::parse(pair.as_bytes())
    .next()
    .is_some_and(|(key, _)| is_tracking(platform, &key))
}

/// Retire les paramètres de suivi connus. Une URL invalide ou non `http(s)` est rendue telle quelle, comme un lien
/// SoundCloud privé (`/s-…`), dont le jeton de partage ne doit pas être altéré.
#[must_use]
pub fn clean_url(url: &str) -> String {
  let Ok(parsed) = Url::parse(url) else {
    return url.to_string();
  };
  if !matches!(parsed.scheme(), "http" | "https") {
    return url.to_string();
  }
  let platform = platform_of(parsed.host_str().unwrap_or(""));
  if platform == Platform::SoundCloud && parsed.path().contains("/s-") {
    return url.to_string();
  }
  let (before_fragment, fragment) = url
    .split_once('#')
    .map_or((url, None), |(b, f)| (b, Some(f)));
  let Some((base, query)) = before_fragment.split_once('?') else {
    return url.to_string();
  };
  let kept: Vec<&str> = query
    .split('&')
    .filter(|pair| !is_tracking_pair(platform, pair))
    .collect();
  if kept.len() == query.split('&').count() {
    return url.to_string();
  }
  let mut out = String::with_capacity(url.len());
  out.push_str(base);
  if !kept.is_empty() {
    out.push('?');
    out.push_str(&kept.join("&"));
  }
  if let Some(f) = fragment {
    out.push('#');
    out.push_str(f);
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn table() {
    let cases: &[(&str, &str)] = &[
      // Globaux, sur n'importe quel hôte
      ("https://example.com/?utm_source=a", "https://example.com/"),
      (
        "https://example.com/?utm_medium=a&x=1",
        "https://example.com/?x=1",
      ),
      (
        "https://example.com/?x=1&utm_campaign=a",
        "https://example.com/?x=1",
      ),
      (
        "https://example.com/?utm_term=a&x=1&utm_content=b&y=2",
        "https://example.com/?x=1&y=2",
      ),
      (
        "https://example.com/?utm_id=a&fbclid=b&gclid=c&dclid=d&msclkid=e",
        "https://example.com/",
      ),
      (
        "https://example.com/?mc_cid=a&mc_eid=b&igshid=c&igsh=d",
        "https://example.com/",
      ),
      (
        "https://example.com/?yclid=a&twclid=b&_ga=c&ref_src=d",
        "https://example.com/",
      ),
      (
        "https://example.com/a?UTM_SOURCE=x",
        "https://example.com/a",
      ),
      (
        "https://example.com/?utm%5Fsource=x&y=1",
        "https://example.com/?y=1",
      ),
      // URL inconnues et paramètres inconnus : jamais touchés
      (
        "https://example.com/?a=1&b=2",
        "https://example.com/?a=1&b=2",
      ),
      (
        "https://example.com/?si=1&feature=2&pp=3&ref=4&context=5&nd=6",
        "https://example.com/?si=1&feature=2&pp=3&ref=4&context=5&nd=6",
      ),
      (
        "https://example.com/?utm_foo=1",
        "https://example.com/?utm_foo=1",
      ),
      (
        "HTTPS://Example.COM/Path?A=%20b+c",
        "HTTPS://Example.COM/Path?A=%20b+c",
      ),
      (
        "https://example.com/?b=2&a=1&utm_source=x&c=3",
        "https://example.com/?b=2&a=1&c=3",
      ),
      (
        "https://example.com/?q=a%26utm_source%3Db",
        "https://example.com/?q=a%26utm_source%3Db",
      ),
      ("https://example.com/?", "https://example.com/?"),
      (
        "https://example.com/?a=1&&b=2",
        "https://example.com/?a=1&&b=2",
      ),
      // YouTube
      (
        "https://www.youtube.com/watch?v=abc&si=x",
        "https://www.youtube.com/watch?v=abc",
      ),
      (
        "https://youtube.com/watch?si=x&v=abc&feature=share&pp=y",
        "https://youtube.com/watch?v=abc",
      ),
      (
        "https://m.youtube.com/watch?v=abc&feature=youtu.be",
        "https://m.youtube.com/watch?v=abc",
      ),
      (
        "https://music.youtube.com/watch?v=abc&list=PL1&si=x&index=3",
        "https://music.youtube.com/watch?v=abc&list=PL1&index=3",
      ),
      (
        "https://youtu.be/abc?si=x&t=30",
        "https://youtu.be/abc?t=30",
      ),
      (
        "https://www.youtube.com/watch?v=abc&t=10s&list=L&index=2",
        "https://www.youtube.com/watch?v=abc&t=10s&list=L&index=2",
      ),
      // Spotify
      (
        "https://open.spotify.com/track/abc?si=x",
        "https://open.spotify.com/track/abc",
      ),
      (
        "https://open.spotify.com/album/abc?si=x&context=y&nd=1&utm_foo=z",
        "https://open.spotify.com/album/abc",
      ),
      (
        "https://open.spotify.com/playlist/abc?go=1&si=x",
        "https://open.spotify.com/playlist/abc?go=1",
      ),
      // SoundCloud
      (
        "https://soundcloud.com/a/b?si=x&utm_foo=y&ref=z",
        "https://soundcloud.com/a/b",
      ),
      (
        "https://m.soundcloud.com/a/sets/b?si=x&keep=1",
        "https://m.soundcloud.com/a/sets/b?keep=1",
      ),
      (
        "https://soundcloud.com/a/b/s-AbCdE?si=x&utm_source=y",
        "https://soundcloud.com/a/b/s-AbCdE?si=x&utm_source=y",
      ),
      (
        "https://soundcloud.com/a/sets/b/s-AbCdE?ref=z",
        "https://soundcloud.com/a/sets/b/s-AbCdE?ref=z",
      ),
      (
        "https://soundcloud.com/a/s-mix?si=x",
        "https://soundcloud.com/a/s-mix?si=x",
      ),
      (
        "https://soundcloud.com/a/news-b?si=x",
        "https://soundcloud.com/a/news-b",
      ),
      // Doublons
      (
        "https://www.youtube.com/watch?si=a&v=1&si=b&v=2&si=c",
        "https://www.youtube.com/watch?v=1&v=2",
      ),
      // Fragments conservés, et jamais lus comme une requête
      (
        "https://example.com/?utm_source=a#top",
        "https://example.com/#top",
      ),
      (
        "https://www.youtube.com/watch?v=abc&si=x#t=30",
        "https://www.youtube.com/watch?v=abc#t=30",
      ),
      (
        "https://example.com/#utm_source=a",
        "https://example.com/#utm_source=a",
      ),
      (
        "https://example.com/#a?utm_source=b",
        "https://example.com/#a?utm_source=b",
      ),
      // URL invalides ou d'un autre schéma : rendues telles quelles
      ("", ""),
      ("pas une url", "pas une url"),
      ("not a url?utm_source=a", "not a url?utm_source=a"),
      (
        "www.youtube.com/watch?v=a&si=b",
        "www.youtube.com/watch?v=a&si=b",
      ),
      ("http://", "http://"),
      (
        "ftp://example.com/?utm_source=a",
        "ftp://example.com/?utm_source=a",
      ),
      (
        "file:///C:/a.mp3?utm_source=a",
        "file:///C:/a.mp3?utm_source=a",
      ),
      // Hôtes voisins : pas de règle de plateforme
      (
        "https://notyoutube.com/watch?v=a&si=b",
        "https://notyoutube.com/watch?v=a&si=b",
      ),
      (
        "https://youtube.com.evil.example/watch?si=b",
        "https://youtube.com.evil.example/watch?si=b",
      ),
      (
        "https://api.spotify.com/v1/x?context=1",
        "https://api.spotify.com/v1/x?context=1",
      ),
      // URL signées de flux : intactes
      (
        "https://r1.googlevideo.com/videoplayback?expire=1&si=2&sig=3",
        "https://r1.googlevideo.com/videoplayback?expire=1&si=2&sig=3",
      ),
    ];
    assert!(cases.len() >= 30);
    for (input, expected) in cases {
      assert_eq!(&clean_url(input), expected, "entrée : {input}");
    }
  }

  #[test]
  fn cleaning_twice_changes_nothing() {
    let once = clean_url("https://www.youtube.com/watch?si=a&v=1&utm_source=b#c");
    assert_eq!(clean_url(&once), once);
  }
}
