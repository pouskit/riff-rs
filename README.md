# Riff

[![CI](https://github.com/pouskit/riff-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/pouskit/riff-rs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pouskit/riff-rs)](https://github.com/pouskit/riff-rs/releases/latest)
[![Licence MIT](https://img.shields.io/badge/licence-MIT-blue)](LICENSE)

**Lecteur de musique de bureau, 100 % Rust, léger, rapide et sans télémétrie.**

Riff réunit YouTube, YouTube Music, SoundCloud et tes fichiers locaux dans une seule application native : interface [iced](https://github.com/iced-rs/iced), moteur audio en Rust, aucun navigateur embarqué, aucun JavaScript.

## Points forts
- **Léger.** Environ 110 Mo de RAM au repos, démarrage en 350 ms, installeur de 13,5 Mo ([mesures](docs/BENCHMARKS.md)).
- **Un seul lecteur pour tout.** Recherche et lecture YouTube, YouTube Music, SoundCloud et fichiers locaux, mélangés librement dans une même playlist.
- **Jam.** Écoute partagée par Internet avec jusqu'à 7 amis : file commune, lecture, pause et déplacement synchronisés, connexion directe chiffrée de bout en bout (`iroh`), code d'invitation à copier.
- **Son.** Égaliseur 10 bandes, réverbération, distorsion, compresseur, normalisation, fondu enchaîné.
- **Paroles.** Synchronisées (lrclib, Genius, Kugou, NetEase, Musixmatch), mode karaoké, textes de droite à gauche, traduction optionnelle.
- **Respect de la vie privée.** Zéro télémétrie, paramètres de suivi retirés des adresses, secrets dans le coffre du système ([détails](docs/PRIVACY.md)).

## Fonctions
- Bibliothèque, favoris, titres bloqués, historique, file de lecture, téléchargements hors ligne, minuterie de veille.
- Reprise au lancement : file, titre et position retrouvés.
- Import de playlists Spotify, Apple Music, Yandex Music, Deezer, SoundCloud, YouTube et de fichiers d'export.
- Visuels, fond de pochette flou, bannières et pochettes par titre, thèmes et éditeur de thème, fenêtre compacte.
- Discord Rich Presence personnalisable, scrobbling Last.fm, contrôles multimédias du système, zone de notification, raccourcis globaux, lancement avec Windows.
- Plugins en [Rhai](https://rhai.rs), dans un bac à sable sans accès aux fichiers ni au réseau ([API](docs/plugins/README.md)).
- Interface en anglais, français, russe et ukrainien.
- Commande `riff-app lyrics` : la ligne de paroles courante dans un terminal.

## Installation
Télécharge l'installeur depuis la page [Releases](https://github.com/pouskit/riff-rs/releases/latest).

| Système | Paquet | État |
|---|---|---|
| Windows 10/11 x64 | `riff-app_<version>_x64-setup.exe` | Pris en charge |
| Linux x64 | AppImage | Expérimental (jamais testé à l'écran) |
| macOS | dmg | Expérimental (jamais testé à l'écran) |

Les paquets ne sont pas signés par un certificat de code. Vérifie l'empreinte SHA-256 avant de les lancer et compare-la au fichier `SHA256SUMS` de la release :

- Windows (PowerShell) : `Get-FileHash .\<fichier> -Algorithm SHA256`
- macOS : `shasum -a 256 <fichier>`
- Linux : `sha256sum <fichier>`

Windows affiche alors « Windows a protégé votre ordinateur » : clique sur « Informations complémentaires », puis « Exécuter quand même ». Sous macOS, autorise l'ouverture dans Réglages Système, Confidentialité et sécurité.

**Mises à jour.** Riff ne contacte GitHub que sur ton clic (« À propos », désactivable). Sous Windows, l'installation d'une mise à jour vérifie sa signature minisign et son SHA-256 avant de la lancer ; un fichier non signé est refusé.

## Compiler depuis les sources
Prérequis : Rust 1.97 ou plus récent.

```
cargo build --release -p riff-app
```

Le binaire est `target/release/riff-app` (`riff-app.exe` sous Windows). Sous Linux (Debian, Ubuntu) : `pkg-config libasound2-dev libdbus-1-dev libwayland-dev libxkbcommon-dev`.

Vérifications :

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
cargo deny --workspace --locked check
```

Architecture : `riff-app` (interface iced), `riff-audio` (décodage `symphonia`, sortie `cpal`, effets), `riff-core` (services, bibliothèque, réseau), `riff-plugins` (moteur Rhai).

## Données
Dossier de données : `%LOCALAPPDATA%\Riff (natif)` (Windows), `~/Library/Application Support/Riff (natif)` (macOS), `~/.local/share/riff-native` et `~/.config/riff-native` (Linux). La variable `RIFF_DATA_DIR` le remplace. Au premier lancement, Riff propose d'importer les données d'une installation de Riff (Electron) existante, en lecture seule.

## Prochaines étapes
- Corriger la navigation dans les réglages et ajouter la lecture directe depuis les cartes de l'accueil.
- Améliorer la file automatique : moins de répétition de l'album ou des favoris, mode découverte de titres similaires hors favoris.
- Notifications de sortie pour les artistes suivis.
- Connexion à un compte YouTube Music : favoris synchronisés et suggestions sur l'accueil (selon les API disponibles).
- Audits complets de sécurité et de performance.
- Validation à l'écran sous Linux et macOS.

Suivi détaillé : [CHANGELOG](CHANGELOG.md).

## Limites connues
- Pas d'arbre d'accessibilité (limite d'iced 0.14.0) : un lecteur d'écran ne peut pas utiliser l'application.
- Flux SoundCloud chiffrés (FairPlay) non lisibles.
- Plugins JavaScript de Riff (Electron) non compatibles : ils se réécrivent en Rhai ([guide](docs/plugins/PORTAGE.md)).
- Traductions russe et ukrainienne non relues par un locuteur.
- Kugou et NetEase sont interrogés en `http://` clair.

## Licence
MIT. Riff est dérivé de [Riff](https://github.com/rootscripts/riff) (Electron, MIT) : Copyright (c) 2026 rootscripts et Copyright (c) 2026 Pouskit. Voir [LICENSE](LICENSE) et [NOTICE.md](NOTICE.md). La licence couvre ce code, pas les conditions d'utilisation des services tiers ([docs/SERVICES.md](docs/SERVICES.md)).
