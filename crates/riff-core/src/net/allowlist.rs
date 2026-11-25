//! Hôtes autorisés du profil `Service`. Motifs : nom exact, ou `*.domaine` (sous-domaines seulement, pas le domaine nu).

const SERVICE_HOSTS: &[&str] = &[
  // YouTube : recherche et lecture innertube, playlists, pages de repli.
  "www.youtube.com",
  "youtube.com",
  "m.youtube.com",
  "music.youtube.com",
  // Lien court YouTube, résolu pendant un import.
  "youtu.be",
  // Flux audio de YouTube (audio-proxy, vérification de flux).
  "*.googlevideo.com",
  // Miniatures YouTube (`i.ytimg.com`, `i9.ytimg.com`...) ; aussi joignables par `PublicImage`.
  "*.ytimg.com",
  // Deezer : recherche, artistes, albums, import de playlists.
  "api.deezer.com",
  // Pages Deezer : cible des liens courts et des imports.
  "deezer.com",
  "www.deezer.com",
  // Lien court Deezer, résolu pendant un import.
  "link.deezer.com",
  // Pochettes Deezer (`cdn-images.dzcdn.net`, `e-cdns-images.dzcdn.net`) ; aussi joignables par `PublicImage`.
  "*.dzcdn.net",
  // iTunes : recherche, classements, repli de pochette et d'album.
  "itunes.apple.com",
  // Apple Music : import de playlists.
  "music.apple.com",
  // Pochettes iTunes (`is1-ssl.mzstatic.com`...) ; aussi joignables par `PublicImage`.
  "*.mzstatic.com",
  // SoundCloud : pages publiques, client_id, API, import, résolution de flux.
  "soundcloud.com",
  "www.soundcloud.com",
  "m.soundcloud.com",
  "api.soundcloud.com",
  "api-v2.soundcloud.com",
  // Lien court SoundCloud, résolu pendant un import.
  "on.soundcloud.com",
  "soundcloud.app.goo.gl",
  // SoundCloud : flux audio (`playback.media-streaming.soundcloud.cloud`, constaté le 2026-11-02).
  "*.soundcloud.cloud",
  // SoundCloud : scripts (`a-v2`) et CDN média.
  "*.sndcdn.com",
  // Spotify : API officielle (connexion utilisateur).
  "api.spotify.com",
  // Spotify : jeton OAuth et page d'autorisation.
  "accounts.spotify.com",
  // Spotify : page intégrée pour les imports publics.
  "open.spotify.com",
  // Spotify : API Pathfinder pour les imports publics.
  "api-partner.spotify.com",
  // Google : API YouTube Data et jetons de la connexion, seulement avec un identifiant client.
  "www.googleapis.com",
  "oauth2.googleapis.com",
  // Google : page d'autorisation, ouverte dans le navigateur.
  "accounts.google.com",
  // Last.fm : API (biographie, auditeurs, scrobble).
  "ws.audioscrobbler.com",
  // Last.fm : page d'autorisation.
  "www.last.fm",
  "last.fm",
  // Wikipédia : biographie d'artiste de repli.
  "en.wikipedia.org",
  "ru.wikipedia.org",
  // Paroles.
  "lrclib.net",
  "genius.com",
  "lyrics.kugou.com",
  "music.163.com",
  "apic-desktop.musixmatch.com",
  // Yandex Music : import de playlists.
  "api.music.yandex.net",
  // Traduction, seulement si l'utilisateur l'active. Reçoit les paroles et textes à traduire.
  "translate.googleapis.com",
  "api.deepl.com",
  "api-free.deepl.com",
  // Icônes de marque de l'UI, mises en cache disque. Reçoit seulement le nom de la marque.
  "cdn.simpleicons.org",
  // Téléchargement de yt-dlp et ffmpeg (voir `TOOL_ORIGINS`).
  "github.com",
  "objects.githubusercontent.com",
  "release-assets.githubusercontent.com",
  // Vérification de mise à jour de riff-rs, sur demande. Ne sert pas à ouvrir des URL (`os::is_allowed_external_url`).
  "api.github.com",
];

// `release-assets` : cible réelle des releases GitHub, constatée le 2026-11-02 sur yt-dlp.
const TOOL_ORIGINS: &[&str] = &[
  "github.com",
  "objects.githubusercontent.com",
  "release-assets.githubusercontent.com",
];

fn matches(pattern: &str, host: &str) -> bool {
  match pattern.strip_prefix("*.") {
    Some(domain) => host
      .strip_suffix(domain)
      .is_some_and(|sub| sub.len() > 1 && sub.ends_with('.')),
    None => pattern == host,
  }
}

fn listed(patterns: &[&str], host: &str) -> bool {
  let host = host.trim_end_matches('.').to_ascii_lowercase();
  patterns.iter().any(|p| matches(p, &host))
}

/// Vrai si `host` figure dans la liste du profil `Service`.
#[must_use]
pub fn is_allowed(host: &str) -> bool {
  listed(SERVICE_HOSTS, host)
}

/// Vrai si `host` est une origine acceptée pour télécharger yt-dlp et ffmpeg.
#[must_use]
pub fn is_tool_origin(host: &str) -> bool {
  listed(TOOL_ORIGINS, host)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn listed_hosts_are_allowed_and_wildcards_match_subdomains_only() {
    assert!(is_allowed("api.deezer.com"));
    assert!(is_allowed("rr3---sn-abc.googlevideo.com"));
    assert!(!is_allowed("googlevideo.com"));
    assert!(!is_allowed("evilgooglevideo.com"));
    assert!(!is_allowed("googlevideo.com.evil.example"));
    assert!(!is_allowed("evil.example"));
  }

  #[test]
  fn real_stream_hosts_seen_in_production_are_listed() {
    assert!(is_allowed("rr4---sn-4g5e6nzl.googlevideo.com"));
    assert!(is_allowed("playback.media-streaming.soundcloud.cloud"));
    assert!(is_allowed("cf-hls-media.sndcdn.com"));
  }

  #[test]
  fn matching_ignores_case_and_the_trailing_dot() {
    assert!(is_allowed("API.Deezer.COM"));
    assert!(is_allowed("api.deezer.com."));
  }

  #[test]
  fn the_release_api_is_listed_for_the_update_check_only() {
    assert!(is_allowed("api.github.com"));
    assert!(!is_tool_origin("api.github.com"));
    assert!(!is_allowed("evil.api.github.com"));
  }

  #[test]
  fn tool_origins_are_limited_to_github() {
    assert!(is_tool_origin("github.com"));
    assert!(is_tool_origin("objects.githubusercontent.com"));
    assert!(is_tool_origin("release-assets.githubusercontent.com"));
    assert!(!is_tool_origin("api.deezer.com"));
    assert!(!is_tool_origin("evil.githubusercontent.com"));
  }

  /// Parcourt les sources du core : tout hôte constant dans le code de production doit être listé.
  #[test]
  fn every_host_written_in_the_core_sources_is_listed() {
    let re = regex::Regex::new(r"https?://([A-Za-z0-9.-]+)").expect("regex");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src];
    let mut missing = std::collections::BTreeSet::new();
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
      for entry in std::fs::read_dir(&dir).expect("dossier source").flatten() {
        let path = entry.path();
        if path.is_dir() {
          stack.push(path);
          continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
          continue;
        }
        let text = std::fs::read_to_string(&path).expect("source lisible");
        // Le code de test est toujours en fin de fichier.
        let production = text.split("#[cfg(test)]").next().unwrap_or("");
        for line in production
          .lines()
          .filter(|l| !l.trim_start().starts_with("//"))
        {
          // Les motifs de regex écrivent `a-v2\.sndcdn\.com`.
          let line = line.replace("\\.", ".");
          for caps in re.captures_iter(&line) {
            let host = &caps[1];
            // Boucle locale : adaptateur HTTP hérité (`local_base_url`), hors du profil `Service`.
            if host == "127.0.0.1" {
              continue;
            }
            seen += 1;
            if !is_allowed(host) {
              missing.insert(format!("{host} ({})", path.display()));
            }
          }
        }
      }
    }
    assert!(
      seen > 30,
      "le parcours n'a trouvé que {seen} URL : regex ou dossier faux"
    );
    assert!(
      missing.is_empty(),
      "hôtes absents de la liste : {missing:#?}"
    );
  }
}
