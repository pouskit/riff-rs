# Services tiers

Riff n'a aucun serveur à lui. Il contacte les services ci-dessous, uniquement dans les cas décrits dans [PRIVACY.md](PRIVACY.md). Les hôtes autorisés sont fixés dans le code (`crates/riff-core/src/net/allowlist.rs`) : une requête vers un autre hôte est refusée. Exception : le Jam ne passe pas par ce filtre. Ses connexions (QUIC vers les autres participants et vers le relais, HTTPS vers le serveur de rendez-vous) sont faites par la crate `iroh`, seulement pendant un jam.

**La licence MIT couvre le code de ce dépôt, pas les conditions d'utilisation de ces services : l'utilisateur reste responsable de son usage.** Les noms de services et de marques appartiennent à leurs propriétaires ; Riff n'est affilié à aucun d'eux. Plusieurs services (YouTube, SoundCloud, Spotify, Kugou, NetEase, Musixmatch) sont interrogés par des interfaces publiques non documentées ou celles de leurs pages web : elles peuvent changer ou cesser de fonctionner sans préavis.

## Liste

| Service | Usage dans Riff | Hôtes |
|---|---|---|
| YouTube, YouTube Music | Recherche, lecture, playlists, miniatures | `www.youtube.com`, `youtube.com`, `m.youtube.com`, `music.youtube.com`, `youtu.be`, `*.googlevideo.com`, `*.ytimg.com` |
| Deezer | Recherche, artistes, albums, pochettes, import de playlists | `api.deezer.com`, `deezer.com`, `www.deezer.com`, `link.deezer.com`, `*.dzcdn.net` |
| iTunes, Apple Music | Recherche, classements, pochettes, import de playlists | `itunes.apple.com`, `music.apple.com`, `*.mzstatic.com` |
| SoundCloud | Recherche, lecture, import | `soundcloud.com`, `www.soundcloud.com`, `m.soundcloud.com`, `api.soundcloud.com`, `api-v2.soundcloud.com`, `on.soundcloud.com`, `soundcloud.app.goo.gl`, `*.soundcloud.cloud`, `*.sndcdn.com` |
| Spotify | Connexion (bibliothèque), import de playlists publiques | `api.spotify.com`, `accounts.spotify.com`, `open.spotify.com`, `api-partner.spotify.com` |
| Yandex Music | Import de playlists | `api.music.yandex.net` |
| Last.fm | Biographies, auditeurs, scrobbling (si activé) | `ws.audioscrobbler.com`, `www.last.fm`, `last.fm` |
| Wikipédia | Biographie d'artiste de repli | `en.wikipedia.org`, `ru.wikipedia.org` |
| lrclib | Paroles | `lrclib.net` |
| Genius | Paroles | `genius.com` |
| Kugou | Paroles, en `http://` clair | `lyrics.kugou.com` |
| NetEase Cloud Music | Paroles, en `http://` clair | `music.163.com` |
| Musixmatch | Paroles | `apic-desktop.musixmatch.com` |
| Google | Connexion YouTube Data (lecture seule, seulement avec un identifiant client), traduction (coupée par défaut) | `accounts.google.com`, `www.googleapis.com`, `oauth2.googleapis.com`, `translate.googleapis.com` |
| DeepL | Traduction, si tu renseignes une clé (coupée par défaut) | `api.deepl.com`, `api-free.deepl.com` |
| Simple Icons | Icônes de marque, nom de la marque seulement | `cdn.simpleicons.org` |
| GitHub | Téléchargement de `yt-dlp` et ffmpeg, vérification de mise à jour de l'application | `github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`, `api.github.com` |
| Discord | Rich Presence, via le client Discord local (tube nommé, pas de réseau) | aucun |
| n0 (relais iroh) | Jam : relais chiffré quand la connexion directe entre participants échoue, et découverte de l'adresse publique, seulement pendant un jam | `use1-1.relay.n0.iroh.link`, `usw1-1.relay.n0.iroh.link`, `euc1-1.relay.n0.iroh.link`, `aps1-1.relay.n0.iroh.link`, ou le relais configuré |
| n0 (rendez-vous pkarr) | Jam : le code à 6 caractères mène à un paquet signé et chiffré (adresse de l'hôte) publié sur ce serveur, lu par les invités ; l'hôte le republie toutes les 60 s et l'annule à la fin du jam. Il voit les adresses IP, les heures, une clé publique dérivée du code et un contenu chiffré. Sans garantie de service ; le réglage « Jam rendezvous URL » le remplace par un serveur pkarr auto-hébergé | `dns.iroh.link`, ou l'URL configurée |
| Routeur local | Jam : demande de redirection de port temporaire (UPnP, NAT-PMP, PCP) pour faciliter la connexion directe, seulement pendant un jam, sans trafic hors du réseau local | aucun (passerelle du réseau local) |

`yt-dlp` et ffmpeg font aussi leur propre trafic réseau, hors de cette liste : `yt-dlp` contacte les sites de lecture (YouTube, SoundCloud, etc.) pour résoudre et télécharger les flux. Riff ne lui transmet que des adresses acceptées par son filtre de services ; une redirection décidée par le site n'est pas filtrée.

## Outils téléchargés à l'exécution
`yt-dlp` et ffmpeg ne sont pas distribués avec Riff. Ils sont téléchargés dans le dossier `bin` du dossier de données quand tu cliques sur « Download » (Réglages, « Required tools »), seulement si Riff ne les trouve pas déjà (variable `RIFF_YTDLP` ou `RIFF_FFMPEG`, dossier `bin` des données, dossier de l'exécutable, ou le `PATH`).

| Outil | Origine | Fichier | Vérification |
|---|---|---|---|
| `yt-dlp` | `https://github.com/yt-dlp/yt-dlp/releases/latest/download/` | `yt-dlp.exe` (Windows x64), `yt-dlp_arm64.exe`, `yt-dlp_x86.exe`, `yt-dlp_macos`, `yt-dlp_linux`, `yt-dlp_linux_aarch64` selon le système | SHA-256 du fichier comparé à `SHA2-256SUMS`, publié au même endroit |
| ffmpeg et ffprobe | `https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/` | `ffmpeg-master-latest-win64-gpl.zip`, `ffmpeg-master-latest-winarm64-gpl.zip`, `ffmpeg-master-latest-linux64-gpl.tar.xz`, `ffmpeg-master-latest-linuxarm64-gpl.tar.xz` (variante GPL) | SHA-256 de l'archive comparé à `checksums.sha256`, publié au même endroit |

- Un fichier dont l'empreinte diffère est supprimé, rien n'est installé et l'erreur est affichée.
- Le fichier d'empreintes vient du même hôte que le fichier : la vérification protège d'un téléchargement corrompu ou tronqué, pas d'une release compromise chez GitHub.
- Pas de build ffmpeg pour macOS : installe ffmpeg toi-même (`brew install ffmpeg`).
- Une fois par 24 h au plus, si `yt-dlp` est celui que Riff a installé, il est mis à jour par `yt-dlp -U` (réglage `ytdlpAutoUpdate`, activé par défaut, désactivable dans Réglages, Avancé). La vérification d'intégrité de cette mise à jour est faite par `yt-dlp` lui-même : non examinée ici.

### Licences relevées le 2026-11-22
- **yt-dlp** : Unlicense (domaine public) pour le code du dépôt, [LICENSE](https://github.com/yt-dlp/yt-dlp/blob/master/LICENSE). Les exécutables Windows et macOS construits avec PyInstaller embarquent du code sous d'autres licences et forment un ensemble sous GPLv3+ ; l'exécutable Unix `yt-dlp` embarque du code ISC et MIT ([section « Licensing » du README](https://github.com/yt-dlp/yt-dlp#licensing), [THIRD_PARTY_LICENSES.txt](https://github.com/yt-dlp/yt-dlp/blob/master/THIRD_PARTY_LICENSES.txt)). Les exécutables autonomes que Riff télécharge sont probablement ceux construits avec PyInstaller, donc sous GPLv3+ : déduit, non vérifié fichier par fichier.
- **ffmpeg (builds de `yt-dlp/FFmpeg-Builds`, variante GPL)** : construits avec `--enable-gpl --enable-version3` (`variants/defaults-gpl.sh`, fichier de licence `COPYING.GPLv3`), donc sous GPL version 3 ([defaults-gpl.sh](https://github.com/yt-dlp/FFmpeg-Builds/blob/master/variants/defaults-gpl.sh), [légal FFmpeg](https://www.ffmpeg.org/legal.html)). Les scripts de construction de `FFmpeg-Builds` sont sous licence MIT, Copyright BtbN ([LICENSE](https://github.com/yt-dlp/FFmpeg-Builds/blob/master/LICENSE)). Riff les télécharge et les lance comme programmes séparés : il ne les lie pas et ne les redistribue pas.
