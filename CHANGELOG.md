# Changelog

Format : [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/). Numérotation : [SemVer](https://semver.org/lang/fr/). Source unique de la version : `[workspace.package].version`.

## [Unreleased]

## [1.0.0] - 2026-11-26

Première version publique.

### Added
- Application native en Rust (iced 0.14.0) : portage de Riff 44.1.1, sans Electron, sans navigateur embarqué.
- Moteur audio en Rust (décodage `symphonia`, sortie `cpal`) : égaliseur 10 bandes, réverbération, distorsion, compresseur, normalisation.
- Recherche et lecture YouTube, YouTube Music, SoundCloud ; pochettes, artistes et albums par Deezer, iTunes, Last.fm, Wikipédia.
- Lecture SoundCloud en HLS (AAC fMP4, sans chiffrement) : segments lus à la demande, durée et déplacement exacts, titres YouTube et SoundCloud mêlés dans une même playlist.
- Paroles synchronisées (lrclib, Genius, Kugou, NetEase, Musixmatch), karaoké, texte de droite à gauche, traduction optionnelle (Google, DeepL).
- Visuels, fond de pochette flou, bannières et pochettes par titre, fenêtre compacte, minuterie de veille, thèmes et éditeur de thème.
- Import de playlists Spotify, Apple Music, Yandex Music, Deezer, SoundCloud, YouTube, et de fichiers d'export ; écran d'import et rapport au premier lancement (données de Riff d'origine lues en lecture seule).
- Jam : écoute partagée par Internet avec jusqu'à 7 amis (bouton de la barre de lecture, code d'invitation, transport `iroh` chiffré de bout en bout). File commune mêlant YouTube et SoundCloud, lecture, pause, déplacement et ajouts partagés, mention « Added by » dans la file, reconnexion automatique, réglage « Jam relay URL ». Aucune connexion hors jam ; détails dans `docs/PRIVACY.md` et `docs/SERVICES.md`. Interface en anglais, français, russe et ukrainien.
- Discord Rich Presence, scrobbling Last.fm, session média du système, zone de notification, raccourcis globaux, instance unique.
- Plugins en Rhai (bac à sable sans fichiers ni réseau, permissions déclarées) : documentation et exemples `hello.rhai` et `better-shuffle.rhai` dans `docs/plugins/`.
- Commande `riff-app lyrics`.
- Vérification de mise à jour sur demande (bouton de « À propos », désactivable).
- Mise à jour signée (Windows) : « Download and install » télécharge l'installeur sur clic, vérifie sa signature minisign (clé publique embarquée) et son SHA-256 publié, puis le lance en silencieux ; signature absente ou fausse = refus. Procédure du mainteneur : `docs/RELEASING.md`.
- Téléchargement de `yt-dlp` et ffmpeg sur demande, avec vérification SHA-256.
- Documents : README, LICENSE, NOTICE, `docs/PRIVACY.md`, `docs/SERVICES.md`, `docs/BENCHMARKS.md`.
- CI GitHub Actions (format, Clippy, tests sur Windows, Linux et macOS, `cargo deny`, `cargo audit`, absence de télémétrie).

### Changed (écarts voulus avec Riff 44.1.1)
- Les plugins JavaScript ne sont pas compatibles (remplacés par Rhai) ; un fichier `.js` est listé « incompatible » et jamais exécuté.
- Non portés : thèmes CSS et CSS personnalisé (`riff_custom_css`), éditeur Monaco (remplacé par un éditeur de code natif), écran de bienvenue (remplacé par l'écran d'import), carrousel de l'accueil, fonds vidéo et GIF, mode performance, barre ondulée, mise à jour automatique.
- Paramètres de suivi supprimés des adresses (`clean_url`).
- « Share » copie l'adresse directe du titre au lieu d'un lien `riff.hs.vc/share` (qui envoyait titre, artiste et durée au serveur de Riff) ; un titre sans adresse publique (fichier local) n'a pas de lien, un toast l'indique.
- Traduction coupée par défaut, avec un avertissement d'envoi à Google ou DeepL.
- Mise à jour de l'application sur clic seulement, jamais en arrière-plan.
- Fondu enchaîné bloqué quand la minuterie « End of track » est active.
- Pas d'annulation du lot de téléchargement ; le toast « Downloaded N tracks » n'indique pas le dossier.
- Pochette et bannière par titre : PNG et JPEG seulement.
- Karaoké : flou de 1,5 px non porté ; polices limitées à celles présentes sur la machine.
- Import par fichier sans barre de progression.
- L'encart Spotify n'est masqué que lorsqu'une playlist existe ; son bouton ouvre Réglages, Intégrations.
- Boutons en glyphes Unicode, pas les icônes de Riff.
- `riff-app lyrics` affiche la ligne entière, pas mot à mot.
- Discord : « Show artist in Discord status » est remplacé par des réglages de l'activité (Réglages, Intégrations) : « Discord status » (Riff, Artist, Track, Track - Artist, repris de l'ancien interrupteur), « Discord time » (barre de progression, temps écoulé, masqué), « Discord cover », « Show when paused » (coupé par défaut, comme Riff), « Album on cover hover ». Un titre sans pochette publique montre l'icône de Riff hébergée par Discord. Un saut de lecture met le minutage à jour. Libellés traduits en `fr`, `ru` et `uk`, ces deux derniers sans relecture.
- Chaînes ajoutées hors Riff : « File too large (20 MB max) », « Could not save the setting. » et 47 chaînes de l'écran d'import et du rapport, traduites en `ru` et `uk` sans relecture.

### Known issues
- Pas d'arbre d'accessibilité (limite d'iced 0.14.0) : un lecteur d'écran est inutilisable.
- Les flux SoundCloud chiffrés (FairPlay) ne sont pas lisibles.
- Linux et macOS : compilés et testés par la CI, jamais lancés à l'écran.
- Pilote AMD : `DRIVER_WARMUP_FRAMES = 240` nécessaire en DX12 sur la machine de développement ; autre GPU non essayé.
- Éditeur de plugins : pas de numéros de ligne ni de surlignage de la ligne courante ; ouverture limitée à 128 Ko.
- Plugins : l'élargissement par recherche de `better-shuffle` ne se déclenche jamais ; l'apprentissage des titres passés (`skipSec`) n'est pas porté.
- Kugou et NetEase sont interrogés en `http://` clair.
- Les paquets ne seront pas signés (ni Authenticode, ni Developer ID) : voir le README.
- Traductions `ru` et `uk` non relues.

### Security
- Zéro télémétrie, vérifiée par `deny.toml` et un test.
- Secrets (jetons Spotify, Last.fm, Google, clé DeepL) dans le coffre du système.
- Hôtes réseau limités à une liste fixe ; téléchargements d'outils vérifiés par SHA-256.

[Unreleased]: https://github.com/pouskit/riff-rs/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/pouskit/riff-rs/releases/tag/v1.0.0
