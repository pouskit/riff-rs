# Vie privée

Ce document décrit ce que Riff garde sur ta machine et ce qui en sort. La liste des services contactés, avec leurs hôtes, est dans [SERVICES.md](SERVICES.md).

## Principes
- **Zéro télémétrie.** Aucune statistique d'usage, aucun rapport de plantage, aucun identifiant d'installation n'est envoyé. Les crates de télémétrie courantes sont interdites par `deny.toml`, et un test (`no_telemetry`) échoue si la liste d'interdiction est affaiblie ou si l'une d'elles apparaît dans `Cargo.lock`.
- **Aucune connexion au démarrage vers un serveur de Riff** : il n'y en a pas. Les requêtes réseau partent de tes actions (recherche, lecture, import, paroles) et des réglages que tu actives.
- Deux requêtes partent sans clic : la vérification de `yt-dlp` (voir « Outils et mises à jour ») et le chargement des icônes de marque (tableau ci-dessous).

## Ce qui est envoyé, à qui, quand
| Action | Données envoyées | Destinataire |
|---|---|---|
| Recherche, lecture d'un titre, pochette, biographie | Texte recherché, titre ou artiste, identifiant du titre ; l'hôte d'une image voit ton adresse IP | YouTube, Deezer, iTunes, SoundCloud, Last.fm, Wikipédia |
| Paroles en ligne | Titre et artiste (plus la durée pour Kugou) | lrclib, Genius, Kugou, NetEase, Musixmatch |
| Import d'une playlist | Le lien de la playlist | Spotify, Apple Music, Yandex Music, Deezer, SoundCloud, YouTube |
| Compte connecté, si tu le connectes | Jeton du compte ; bibliothèque (Spotify), écoutes (Last.fm, si activé), lecture seule (Google, `youtube.readonly`) | Spotify, Last.fm, Google |
| Traduction, si tu l'actives | Paroles et textes de l'interface | Google, ou DeepL si une clé est renseignée |
| Icônes de marque de l'interface | Le nom de la marque seulement (résultat gardé sur le disque) | `cdn.simpleicons.org` |
| Téléchargement de `yt-dlp` et ffmpeg, sur ton clic | Aucune donnée personnelle, une requête de téléchargement | GitHub |
| Vérification de mise à jour, sur ton clic | Une requête GET sans corps, sans jeton, sans version ni identifiant | `api.github.com` |
| Discord Rich Presence, si Discord tourne | Titre, artiste, album (réglage « Album on cover hover »), état de pause (réglage « Show when paused »), minutage, adresse de la miniature distante (`http` ou `https`) ; sans miniature distante, l'adresse de l'icône de Riff, que Discord charge depuis son propre CDN (aucun tiers) ; jamais l'adresse du flux audio ni un chemin local | Ton client Discord local (tube nommé, pas de réseau) |
| Jam, seulement pendant un jam que tu lances ou rejoins | Titres de la file (identifiant, titre, artiste, album, durée, plateforme, adresses de miniature), état de lecture, nom affiché ; chiffrés de bout en bout entre participants | Les autres participants du jam ; un relais de n0 (`*.relay.n0.iroh.link`) ou le relais que tu configures, seulement si la connexion directe échoue |
| Code Jam à 6 caractères, seulement pendant un jam que tu lances ou rejoins | Adresse de l'hôte (identifiant public, relais, adresses IP publiques) et jeton de salle, chiffrés avec une clé dérivée du code ; clé publique de rendez-vous dérivée du code | Le serveur de rendez-vous : `dns.iroh.link` (n0) ou celui que tu configures (« Jam rendezvous URL ») |

## Kugou et NetEase en clair
Les paroles de Kugou et de NetEase sont demandées en `http://`, sans chiffrement, comme dans Riff d'origine : toute personne qui voit ton trafic réseau (fournisseur d'accès, réseau public) peut lire le titre et l'artiste cherchés. Elles ne sont interrogées que lorsque la recherche de paroles en ligne est active.

## Traduction
Coupée par défaut. Une fois activée (Réglages, Interface, Translation), les paroles et les textes de l'interface sont envoyés à Google, ou à DeepL si une clé est renseignée. Tant que tu ne l'actives pas, aucun texte ne part vers ces services.

## Jam
Aucune connexion tant que tu ne lances ni ne rejoins un jam. Le code d'invitation est un code de 6 caractères (par exemple `K7Q-M2X`, 30 bits tirés au hasard). Il ne circule jamais : Riff en dérive deux clés (Argon2id, 64 MiB), une clé de signature, dont la partie publique sert d'adresse de rendez-vous, et une clé de chiffrement. L'hôte publie, toutes les 60 s tant que le jam dure, un paquet signé sur le serveur de rendez-vous de n0 (`dns.iroh.link`, protocole pkarr) : l'identifiant public de la session, l'adresse du relais, tes adresses IP publiques et un jeton de salle, chiffrés (AES-256-GCM) avec la clé dérivée du code. L'invité dérive les mêmes clés, lit le paquet et le déchiffre. À la fin du jam, l'hôte publie un paquet vide qui annule le code. Le serveur de rendez-vous voit les adresses IP de l'hôte et des invités, les heures, la clé publique de rendez-vous et un contenu chiffré illisible sans le code. Quiconque a le code peut rejoindre tant que le jam dure, et peut aussi publier sur la même clé : l'hôte le détecte à la republication suivante, prévient et change de code. Un code de 30 bits ne résiste pas à une recherche systématique en ligne, et l'hôte n'approuve pas chaque invité : ne partage le code qu'avec des personnes de confiance. Un code est tiré à chaque jam. Réglages, Avancé, « Jam rendezvous URL » remplace le serveur de n0 par le tien (hôte et invités doivent utiliser le même) ; il est indépendant de « Jam relay URL ». Si le serveur de rendez-vous est injoignable, l'hôte garde l'ancien code d'invitation long (`riffjam…`), qui contient l'adresse en clair et ne passe par aucun serveur de rendez-vous ; il reste accepté en saisie. En connexion directe, les participants voient ton adresse IP. Via le relais, le relais voit les adresses IP, les heures et le volume des échanges, pas leur contenu (chiffré de bout en bout). Pendant le jam seulement, Riff demande à ton routeur local une redirection de port temporaire (UPnP, NAT-PMP, PCP) pour faciliter la connexion directe ; elle disparaît à la fin du jam, et aucun autre service n'est contacté pour cela. Les relais publics de n0 sont annoncés pour un usage de développement et de loisir, sans garantie de service. Réglages, Avancé, « Jam relay URL » remplace ces relais par le tien. Les chemins de fichiers locaux ne sont jamais envoyés : les fichiers locaux sont refusés dans un jam. À la fin du jam, l'identifiant de session est jeté.

## Secrets
- Les jetons de Spotify, Last.fm et Google, la clé DeepL et le secret d'API Last.fm sont rangés dans le coffre du système (Gestionnaire d'identifiants de Windows, Trousseau de macOS, Secret Service sous Linux), jamais dans un fichier en clair ni dans l'export de compte (`.rfacc`).
- Sans coffre disponible (Linux sans Secret Service), Riff refuse de les stocker : la connexion vaut jusqu'à la fermeture, les clés sont à ressaisir.
- Les journaux masquent les en-têtes `Authorization` et `Cookie` et les champs de type jeton, secret, clé d'API ou mot de passe.

## Ce qui reste en clair sur le disque
Dans le dossier de données de Riff (natif) (`%LOCALAPPDATA%\Riff (natif)` sous Windows, `~/Library/Application Support/Riff (natif)` sous macOS, `$XDG_DATA_HOME/riff-native` sous Linux ; `config.json` sous Linux dans `$XDG_CONFIG_HOME/riff-native`), sans chiffrement. Au premier lancement, l'application copie ces fichiers depuis l'ancien dossier `Riff` s'il contient des données de Riff (natif) : `yt-cookies.txt` est alors présent aux deux endroits, l'ancien n'est pas effacé. Elle lit aussi, en lecture seule et sans le copier, le `storage.json` de Riff d'origine (`%APPDATA%\Riff`) ou de Riff Tauri (ancien dossier).
- `yt-cookies.txt`, si tu connectes YouTube : cookies de ta session Google. Quiconque lit ce fichier accède à ton compte Google. Hors Windows, il est créé en mode 0600 ; sous Windows, il suit les droits du dossier de données. Il n'est jamais inclus dans l'export de compte. Il arrive de deux façons, toutes deux à ta demande : un fichier `cookies.txt` que tu choisis, ou une lecture des cookies de Firefox par yt-dlp (Chrome et Edge ne sont pas proposés sous Windows : leur chiffrement bloque cette lecture). Réglages, Avancé, YouTube Account, « Log out » le supprime.
- La clé d'API Last.fm (sans son secret) dans `config.json`.
- L'adresse du proxy dans `native-settings.json`, mot de passe compris si elle en contient un.
- Ta bibliothèque, tes favoris, ton historique et tes réglages (fichiers JSON du dossier de données), et les titres téléchargés.

## Outils et mises à jour
- `yt-dlp -U` : activé par défaut (`ytdlpAutoUpdate`), une vérification au plus toutes les 24 h auprès de GitHub, seulement si `yt-dlp` est celui que Riff a installé dans son dossier `bin`. Désactivable dans Réglages, Avancé.
- `yt-dlp` et ffmpeg ne sont téléchargés que si tu cliques sur « Download » (Réglages, « Required tools »).
- Mise à jour de l'application : vérification sur clic uniquement, jamais automatique, sans téléchargement. Le bouton peut être masqué par le réglage « Update check ».
- Plugins : un plugin Rhai n'a aucun accès au réseau ni aux fichiers ; ses recherches passent par les mêmes serveurs et contrôles que celles de l'application.
