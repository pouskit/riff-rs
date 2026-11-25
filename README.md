# riff-rs

Riff (natif) est un lecteur de musique de bureau écrit en Rust, avec une interface native [iced](https://github.com/iced-rs/iced) et un moteur audio en Rust. C'est un portage de [Riff](https://github.com/rootscripts/riff) 44.1.1 (Electron), qui garde ses fonctions et son apparence sans Electron, sans navigateur embarqué ni JavaScript.

## État : préversion
Le projet prépare une version `1.0.0-rc.1`. Aucune release n'est publiée à ce jour. Tableau final mesuré le 2026-11-23 sur le paquet installé (commit `40ed624`, Ryzen 9 5900X, RX 7800 XT, Windows 11) : RAM au repos 110,4 Mo, RAM en lecture 101,8 Mo, démarrage jusqu'à la deuxième image 347 ms, CPU en lecture de 8,25 % (rien d'affiché) à 12,15 % (scène c), défilement de 5 000 pistes p95 9,35 ms, début de lecture d'un fichier local 70 ms, exécutable 47,93 Mo, dossier d'installation 48,62 Mo, installeur 13,50 Mo. La recherche n'est pas mesurée. Méthode, valeurs brutes, comparaison avec Riff 44.1.1 et limites : [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

| Système | Ce qui a été fait |
|---|---|
| Windows | Développé et essayé à l'écran. |
| Linux, macOS | Compilés, vérifiés par Clippy et testés par la CI (Ubuntu, macOS), jamais lancés à l'écran. Tout ce qui touche à l'affichage, au son, à la zone de notification, aux raccourcis globaux, au coffre des secrets et à la session média y est non vérifié. |

## Installation
Il n'existe pas encore de paquet. Quand les releases seront publiées, elles viendront de la page « Releases » du dépôt : installeur Windows, AppImage Linux, dmg et zip macOS, avec une empreinte SHA-256 par fichier.

**Les paquets ne seront pas signés** (aucun certificat de signature de code Windows ni compte développeur Apple). Vérifie l'empreinte avant de lancer un fichier téléchargé :

- Windows (PowerShell) : `Get-FileHash .\<fichier> -Algorithm SHA256`
- macOS : `shasum -a 256 <fichier>`
- Linux : `sha256sum <fichier>`

Compare le résultat à l'empreinte publiée avec la release.

- **Windows, SmartScreen.** Un exécutable non signé déclenche « Windows a protégé votre ordinateur ». Après avoir vérifié l'empreinte, clique sur « Informations complémentaires », puis « Exécuter quand même ».
- **macOS, Gatekeeper.** Les applications sont signées « ad hoc » (sans identité de développeur) et non notarisées : macOS refuse de les ouvrir au premier lancement. Après avoir vérifié l'empreinte, autorise l'ouverture dans Réglages Système, Confidentialité et sécurité (bouton « Ouvrir quand même »). Le libellé et le chemin exacts dépendent de la version de macOS : non vérifié.

Riff ne se met pas à jour tout seul. Le bouton de vérification de « À propos » interroge l'API des releases du dépôt, sur ton clic seulement, et n'installe rien. Tant que le dépôt n'a pas de release publique, il répond « aucune release publique ».

## Compiler depuis les sources
Prérequis : Rust 1.97 ou plus récent (`rust-version` du workspace, édition 2024).

```
cargo build --release -p riff-app
```

Le binaire est `target/release/riff-app` (`riff-app.exe` sous Windows). Sous Linux, installe d'abord les bibliothèques de développement utilisées par la CI : `pkg-config libasound2-dev libdbus-1-dev libwayland-dev libxkbcommon-dev` (paquets Debian et Ubuntu).

Vérifications du dépôt :

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
cargo deny --workspace --locked check
```

La CI installe `ffmpeg` et `ffprobe` avant de lancer les tests.

## Fonctions
- Recherche et lecture depuis YouTube, YouTube Music et SoundCloud, avec Deezer, iTunes, Last.fm et Wikipédia pour les pochettes, artistes et albums. Les flux passent par `yt-dlp` et ffmpeg, téléchargés sur ton clic (voir [docs/SERVICES.md](docs/SERVICES.md)).
- Moteur audio en Rust : décodage, égaliseur 10 bandes, réverbération, distorsion, compresseur, normalisation.
- Paroles synchronisées (lrclib, Genius, Kugou, NetEase, Musixmatch), mode karaoké, textes de droite à gauche, traduction optionnelle.
- Visuels : barres, forme, fond de pochette flou, bannières par titre.
- Import de playlists Spotify, Apple Music, Yandex Music, Deezer, SoundCloud et YouTube, et de fichiers d'export.
- Bibliothèque, favoris, historique, file de lecture, titres téléchargés, minuterie de veille.
- Discord Rich Presence, scrobbling Last.fm, contrôles du système (session média), zone de notification, raccourcis globaux, fenêtre compacte.
- Thèmes et éditeur de thème, plugins Rhai, commande `riff-app lyrics` qui affiche la ligne de paroles courante dans un terminal.
- Interface en anglais, avec les dictionnaires russe et ukrainien de Riff.
- Import, au premier lancement, des données d'un Riff d'origine ou de Riff Tauri (lecture seule de leur `storage.json`, là où il est : jamais copié, jamais écrit).
- L'écran d'import n'apparaît qu'au premier lancement : tant que `native-settings.json` ou `user-data.json` existe dans le dossier de données, il n'est plus proposé, même après « Start fresh ». Pour le relancer, ferme l'application, déplace ces deux fichiers hors du dossier de données (par exemple `%LOCALAPPDATA%\Riff (natif)` sous Windows ; le dossier réel suit `RIFF_DATA_DIR` et le réglage `dataDir`), puis relance : le `storage.json` de Riff doit toujours exister et contenir des clés. Les nouveaux réglages et données sont perdus si tu ne les gardes pas ailleurs.
- Dossier propre, distinct de Riff : `%LOCALAPPDATA%\Riff (natif)` (Windows), `~/Library/Application Support/Riff (natif)` (macOS), `$XDG_DATA_HOME/riff-native` et `$XDG_CONFIG_HOME/riff-native` (Linux, défauts `~/.local/share` et `~/.config`). Les deux applications peuvent coexister. `RIFF_DATA_DIR` et le réglage `dataDir` gardent la priorité.
- Au premier lancement, si l'ancien dossier par défaut de Riff (natif) (`%LOCALAPPDATA%\Riff`, `~/Library/Application Support/Riff`, `~/.cache/devsize` et `~/.config/devsize`) contient `native-settings.json` ou `user-data.json`, ses données sont copiées dans le nouveau dossier (réglages, données, bibliothèque, paroles, thèmes, plugins, `bin`). L'ancien dossier n'est jamais modifié. Le cache audio (`tracks`), les miniatures et les journaux ne sont pas repris.
- Aucune télémétrie. Voir [docs/PRIVACY.md](docs/PRIVACY.md).

## Plugins
**Les plugins JavaScript de Riff ne sont pas compatibles.** Cette version n'embarque ni moteur JavaScript ni page web : un plugin `.js` est listé « incompatible » et n'est jamais exécuté. Les plugins s'écrivent en [Rhai](https://rhai.rs), dans un bac à sable sans accès aux fichiers ni au réseau, avec des permissions déclarées et affichées avant l'activation. Voir [docs/plugins/README.md](docs/plugins/README.md) pour l'API et [docs/plugins/PORTAGE.md](docs/plugins/PORTAGE.md) pour réécrire un plugin JavaScript.

## Limites connues
- **Accessibilité.** L'application n'expose pas d'arbre d'accessibilité (limite d'iced 0.14.0) : un lecteur d'écran ne peut pas l'utiliser.
- **SoundCloud.** Les flux chiffrés (FairPlay) ne sont pas lisibles : un titre qui n'a que ce flux affiche une erreur.
- **Linux et macOS.** Jamais lancés à l'écran (voir « État »). Sous Linux, l'icône de la zone de notification, les raccourcis globaux sous Wayland, la police de l'interface et le coffre des secrets (Secret Service requis) sont des points de risque non vérifiés.
- **`riff-app lyrics`** affiche la ligne de paroles entière, pas mot à mot comme `riff lyrics` de Riff.
- **Traduction.** Une fois activée, les paroles et les textes de l'interface sont envoyés à Google, ou à DeepL si tu renseignes une clé. Elle est coupée par défaut.
- **Pilote graphique AMD.** Sur la machine de développement (Radeon RX 7800 XT, DX12), sans le contournement `DRIVER_WARMUP_FRAMES` (240 images invisibles au démarrage), un cœur de processeur reste occupé au repos. Le contournement reste en place ; il n'a été essayé sur aucun autre GPU.
- **Traductions `ru` et `uk`.** Elles n'ont pas été relues par un locuteur, y compris les textes ajoutés par riff-rs (écran d'import, rapport, deux messages d'erreur) ; les clés absentes retombent sur l'anglais.
- **Plugins.** Voir [le CHANGELOG](CHANGELOG.md) pour les limites de l'éditeur et des exemples.
- **Kugou et NetEase** sont interrogés en `http://` clair, comme dans Riff d'origine.

## Licence
MIT, Copyright (c) 2026 rootscripts et Copyright (c) 2026 Pouskit. Voir [LICENSE](LICENSE) et [NOTICE.md](NOTICE.md) (dépendance à Riff, polices, icônes, licences des bibliothèques). La licence MIT couvre le code de ce dépôt, pas les conditions d'utilisation des services tiers : voir [docs/SERVICES.md](docs/SERVICES.md).
