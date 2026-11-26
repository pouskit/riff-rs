# Benchmarks : Riff 44.1.1 contre riff-rs

Ce fichier donne des mesures, leur méthode et leurs limites. Il ne tire aucune conclusion générale.

## En-tête

| Élément | Valeur |
|---|---|
| Date de la campagne | 2026-11-14, de 12:30 à 13:12 (heure locale) |
| Générée par | `scripts/bench.ps1` (commit `89cd8bf`) |
| Processeur | AMD Ryzen 9 5900X, 12 cœurs, 24 processeurs logiques |
| Carte graphique | AMD Radeon RX 7800 XT, pilote 32.0.32015.7029 |
| Mémoire | 31,9 Go |
| Système | Windows 11 Pro, build 26300 |
| Écran | 240 Hz (valeur lue par CIM, voir « Limites ») |
| Riff | 44.1.1 (Electron), copie de l'installé hors du dossier d'installation, `resources\app-update.yml` renommé en `app-update.yml.off`, profil isolé |
| riff-rs | commit `386ca31`, `target\release\riff-app.exe` (release) |

Toutes les valeurs du tableau principal et des sections de méthode viennent de cette campagne unique, sur cette machine. Les sections « Mesure plugins » et « Historique » ont leur propre date et leur propre source.

Conventions : le temps et la mémoire sont des médianes. « Essais » compte les lancements ou passes valides. 1 Mo = 1 048 576 octets. Un pourcentage de CPU vaut 100 % pour un cœur logique, donc une valeur au-dessus de 100 % est possible. Le rapport est riff-rs divisé par Riff.

## Tableau final

Mesure sur le paquet installé, riff-rs seul : Riff n'a pas été remesuré.

| Élément | Valeur |
|---|---|
| Date de la campagne | 2026-11-23, de 00:50 à 01:13 (heure locale), en 4 appels de `scripts/bench.ps1` |
| Commit | `40ed624`, version `1.0.0-rc.1` |
| Exécutable mesuré | `riff-app.exe` installé par l'installeur NSIS (`/S`, `%LOCALAPPDATA%\Programs\Riff (natif)\riff-app.exe`), SHA-256 `6683a52d90834f8d7aec56a612c23681e5575b77d1f4c97ae994d08e43d1ac40` |
| Installeur | `riff-app_1.0.0-rc.1_x64-setup.exe`, 14 159 122 octets, SHA-256 `ff37c9431b283c70549562a658736b839ce4759c7a1992ef00585c75c7125842`, construit le 2026-11-23 à 00:49 par `cargo packager --release --formats nsis` |
| Machine | Même processeur (Ryzen 9 5900X, 24 processeurs logiques), même carte (RX 7800 XT), même mémoire (31,9 Go), même système (Windows 11 Pro, build 26300) que l'en-tête. Deux éléments diffèrent : pilote 32.0.32029.2010 (au lieu de 32.0.32015.7029) et fréquence d'écran lue par CIM : 75 Hz (au lieu de 240 Hz) |

Méthodes : celles de « Méthode, mesure par mesure », sans changement. Conventions de l'en-tête (médiane, 1 Mo = 1 048 576 octets). La colonne Riff 44.1.1 reprend le tableau principal : campagne du 2026-11-14, non remesuré. Aucun rapport n'est calculé entre deux campagnes différentes.

| Mesure | Riff 44.1.1 (campagne du 2026-11-14, non remesuré) | riff-rs `40ed624`, installé (2026-11-23) |
|---|---|---|
| RAM au repos, accueil | 285,3 Mo (284,9 à 289,8 ; 5 essais) | 110,4 Mo (108,5 à 111,5 ; 5 essais) |
| RAM en lecture, 60 s, scène a | 329,05 Mo (324,1 à 334 ; 2 essais) | 101,8 Mo (101,7 à 101,9 ; 2 essais) |
| Démarrage jusqu'au premier rendu | 871,9 ms (847,6 à 913 ; 5 essais), méthode différente | 347 ms (343 à 351 ; 5 essais) |
| CPU en lecture, scène a (rien d'affiché) | 90,85 % (90,6 à 91,1 ; 2 essais) | 8,25 % (7,4 à 9,1 ; 2 essais) |
| CPU en lecture, scène b (paroles mot à mot) | 111,1 % (110,5 à 111,7 ; 2 essais) | 8,8 % (7,9 à 9,7 ; 2 essais) |
| CPU en lecture, scène b2 (paroles en dégradé) | 114,1 % (112,3 à 115,9 ; 2 essais) | 8,95 % (8,6 à 9,3 ; 2 essais) |
| CPU en lecture, scène c (barres et forme) | 126,75 % (123,2 à 130,3 ; 2 essais) | 12,15 % (11,2 à 13,1 ; 2 essais) |
| CPU en lecture, scène d (fond de pochette flou) | 101,95 % (101,4 à 102,5 ; 2 essais) | 9,8 % (8,7 à 10,9 ; 2 essais) |
| CPU en lecture, scène e (paroles, barres, forme, fond flou) | 142,9 % (139,2 à 146,6 ; 2 essais) | 11,7 % (11,1 à 12,3 ; 2 essais) |
| Défilement de 5 000 pistes, p95 de l'intervalle entre images | non mesuré | 9,35 ms (9,33 à 9,44 ; 3 essais) |
| Latence de début de lecture, fichier local | non mesurée | 70 ms (69 à 72 ; 5 essais) |
| Taille de l'exécutable | 212,74 Mo (1 mesure), `Riff.exe` seul | 47,93 Mo (50 256 896 octets ; 1 mesure) |
| Taille du dossier d'installation | 395,52 Mo (1 mesure) | 48,62 Mo (50 985 386 octets : `riff-app.exe` et `uninstall.exe` ; 1 mesure) |
| Taille de l'installeur | 102,53 Mo (1 mesure) | 13,50 Mo (14 159 122 octets ; 1 mesure) |
| Recherche | 2,05 ms et 393,25 ms (voir le tableau principal) | non mesurée, voir « Non mesuré » |

Valeurs brutes de riff-rs : RAM au repos 108,5 / 109,1 / 111,5 / 110,4 / 111,2 Mo ; RAM en lecture 101,9 / 101,7 Mo ; démarrage 351 / 343 / 343 / 350 / 347 ms ; CPU, passe 1 puis passe 2, scène a 7,4 / 9,1, b 7,9 / 9,7, b2 9,3 / 8,6, c 11,2 / 13,1, d 10,9 / 8,7, e 12,3 / 11,1 % ; défilement 9,33 / 9,44 / 9,35 ms ; latence 69 / 72 / 69 / 71 / 70 ms.

Seuils (cibles inchangées) :

| Seuil | Mesure | Verdict |
|---|---|---|
| RAM au repos <= 158 Mo | 110,4 Mo | Tenu |
| Démarrage jusqu'au premier contenu < 500 ms | 347 ms (maximum 351 ms) | Tenu. La méthode note la deuxième image présentée, borne haute du premier contenu |

Réserves : la RAM au repos (110,4 Mo) et le CPU de la scène a (8,25 %) sont plus hauts que dans le tableau principal du 2026-11-14 (94,8 Mo ; 2,25 %), sur un autre commit (`40ed624` au lieu de `386ca31`), un autre pilote et une autre fréquence d'écran lue (75 Hz au lieu de 240 Hz). Ces écarts ne sont pas expliqués ici. Le SHA-256 de l'installeur diffère de celui essayé le 2026-11-23 (`4f5989a2…2eae`, construit depuis `6d78d03`, avant le module de mise à jour) : `40ed624` ajoute le module de mise à jour, d'où un exécutable plus gros (50 256 896 octets au lieu de 49 507 840). Installation et désinstallation silencieuses exécutées pour cette campagne : le dossier du programme a été supprimé et `%LOCALAPPDATA%\Riff (natif)` est resté inchangé (17 entrées, date de modification 2026-11-23 00:21:44).

## Build optimisé (2026-11-23)

riff-rs seul, `target\release\riff-app.exe` (pas d'installeur). Deux campagnes de `scripts/bench.ps1` le même jour, même machine, même pilote (32.0.32029.2010). Fréquence d'écran lue par CIM : 75 Hz pour toutes les mesures de `8826f19` ; pour `15ad7e2`, 75 Hz pour la taille, la RAM au repos et le démarrage, 59 Hz pour toutes les autres. Mêmes méthodes que « Méthode, mesure par mesure » ; recherche et latence en ligne non mesurées (réseau).

- `15ad7e2` : avant les optimisations, de 03:53 à 04:22.
- `8826f19` : profil release `lto = "fat"` et `codegen-units = 1`, une tessellation par barre dans les canvas de barres, cache borné des textes raccourcis. SHA-256 `78607e21ab39711cfde1a0adba9759525fe90a43e35517afcc4704d09e37baec`, de 13:26 à 13:52.

| Mesure | `15ad7e2` | `8826f19` | Changement |
|---|---|---|---|
| Taille de l'exécutable | 47,94 Mo | 37,43 Mo | -21,9 % |
| RAM au repos, accueil | 110,8 Mo (108,3 à 111 ; 5 essais) | 110 Mo (109,1 à 110,9 ; 5 essais) | -0,7 % |
| Démarrage jusqu'au premier rendu | 334 ms (328 à 749 ; 5 essais) | 326 ms (324 à 332 ; 5 essais) | -2,4 % |
| RAM en lecture, scène a | 100,45 Mo (99,7 à 101,2 ; 2 essais) | 100,15 Mo (99,7 à 100,6 ; 2 essais) | -0,3 % |
| CPU, scène a | 7,95 % (5,7 à 10,2 ; 2 essais) | 6,5 % (5,9 à 7,1 ; 2 essais) | -18,2 % |
| CPU, scène b | 6,45 % (6,4 à 6,5 ; 2 essais) | 5,75 % (5,6 à 5,9 ; 2 essais) | -10,9 % |
| CPU, scène b2 | 6,45 % (6,4 à 6,5 ; 2 essais) | 7,75 % (7,7 à 7,8 ; 2 essais) | +20,2 % |
| CPU, scène c | 9,25 % (9,2 à 9,3 ; 2 essais) | 9,05 % (7,8 à 10,3 ; 2 essais) | -2,2 % |
| CPU, scène d | 5,75 % (5,5 à 6 ; 2 essais) | 5,95 % (4,5 à 7,4 ; 2 essais) | +3,5 % |
| CPU, scène e | 9,25 % (9,2 à 9,3 ; 2 essais) | 10,65 % (10,5 à 10,8 ; 2 essais) | +15,1 % |
| Défilement de 5 000 pistes, p95 | 20,3 ms (20,25 à 20,34 ; 3 essais) | 8,14 ms (8,06 à 8,15 ; 3 essais) | voir réserves |
| Latence de début de lecture, fichier local | 163 ms (146 à 163 ; 5 essais) | 138 ms (134 à 140 ; 5 essais) | voir réserves |

Valeurs brutes de `8826f19` : RAM au repos 109,8 / 109,1 / 110,9 / 110 / 110,2 Mo ; démarrage 332 / 327 / 326 / 324 / 324 ms ; RAM en lecture 99,7 / 100,6 Mo ; CPU, passe 1 puis passe 2, scène a 5,9 / 7,1, b 5,9 / 5,6, b2 7,8 / 7,7, c 10,3 / 7,8, d 7,4 / 4,5, e 10,5 / 10,8 % ; défilement 8,06 / 8,14 / 8,15 ms ; latence 140 / 136 / 134 / 138 / 138 ms.

Seuils : RAM au repos <= 158 Mo (110 Mo, tenu) ; démarrage < 500 ms (326 ms, maximum 332 ms, tenu).

Réserves :
- Les deux colonnes viennent de deux sessions non alternées, à 10 h d'écart. Le même exécutable `8826f19` a donné 10,65 % sur la scène e entre 13:47 et 13:50 et 9,85 % (médiane de 4 passes) entre 14:09 et 14:38 : l'écart entre sessions atteint 0,8 point. Les scènes de `15ad7e2` ont tourné avec l'écran lu à 59 Hz, celles de `8826f19` à 75 Hz ; l'effet de ce changement sur le CPU n'est pas mesuré. Les hausses des scènes b2 (+1,3 point) et e (+1,4 point) ne sont donc pas établies, pas plus que les baisses des scènes a et b. Mesurée en A/B alterné dans une même session (2 tours, 4 passes par version), la tessellation par barre baisse les médianes de la scène e de 9,2 % à 7,8 % et de la scène c de 8,7 % à 7,9 %.
- Défilement et latence : la baisse vient de l'environnement, pas du code (aucun changement ne touche ces chemins). `15ad7e2` : 1 798 images en 30 s, p50 16,7 ms, soit le rythme de 60 Hz (écran lu à 59 Hz). `8826f19` : 5 601 à 5 672 images en 30 s, p50 4,56 à 4,70 ms, soit environ 190 images par seconde, alors que CIM lit 75 Hz. Le p95 de défilement suit le rythme réel de l'écran.
- Durée du build release propre : 112 s avant, 338 s après (profil LTO), sur la même machine.

## Tableau principal

| Mesure | Riff 44.1.1 | riff-rs `386ca31` | Rapport |
|---|---|---|---|
| RAM au repos, accueil | 285,3 Mo (284,9 à 289,8 ; 5 essais) | 94,8 Mo (94,6 à 98,1 ; 5 essais) | 0,332 |
| RAM en lecture, 60 s, scène a | 329,05 Mo (324,1 à 334 ; 2 essais) | 89,75 Mo (89,5 à 90 ; 2 essais) | 0,273 |
| Démarrage jusqu'au premier rendu | 871,9 ms (847,6 à 913 ; 5 essais) | 318 ms (314 à 323 ; 5 essais) | non calculé : méthodes différentes |
| Taille de l'exécutable | 212,74 Mo (1 mesure), `Riff.exe` seul | 42,12 Mo (1 mesure) | 0,198 |
| Taille du dossier d'installation | 395,52 Mo (1 mesure) | non mesurée | |
| Taille de l'installeur | 102,53 Mo (1 mesure) | non mesurée | |
| CPU en lecture, scène a (rien d'affiché) | 90,85 % (90,6 à 91,1 ; 2 essais) | 2,25 % (1,3 à 3,2 ; 2 essais) | 0,025 |
| CPU en lecture, scène b (paroles mot à mot) | 111,1 % (110,5 à 111,7 ; 2 essais) | 7,05 % (6,1 à 8 ; 2 essais) | 0,063 |
| CPU en lecture, scène b2 (paroles en dégradé) | 114,1 % (112,3 à 115,9 ; 2 essais) | 7,6 % (7,5 à 7,7 ; 2 essais) | 0,067 |
| CPU en lecture, scène c (barres et forme) | 126,75 % (123,2 à 130,3 ; 2 essais) | 10,3 % (8,8 à 11,8 ; 2 essais) | 0,081 |
| CPU en lecture, scène d (fond de pochette flou) | 101,95 % (101,4 à 102,5 ; 2 essais) | 2,05 % (1,3 à 2,8 ; 2 essais) | 0,020 |
| CPU en lecture, scène e (paroles, barres, forme, fond flou) | 142,9 % (139,2 à 146,6 ; 2 essais) | 11,75 % (11,1 à 12,4 ; 2 essais) | 0,082 |
| Recherche, même requête répétée | 2,05 ms (1,8 à 525,3 ; 10 essais) | non mesurée | |
| Recherche, requêtes distinctes | 393,25 ms (336,5 à 464,2 ; 10 essais) | non mesurée | |
| Défilement de 5 000 pistes, p95 de l'intervalle entre images | non mesuré | 9,64 ms (9,48 à 9,94 ; 3 essais) | |
| Latence de début de lecture, fichier local | non mesurée | 69 ms (68 à 70 ; 5 essais) | |
| Latence de début de lecture, titre en ligne | non mesurée | non mesurée | |

Notes de lecture :

- Rapports calculés à partir des médianes ci-dessus et arrondis à 3 décimales. Ils sont donnés seulement quand les deux applications ont la même méthode. Pour la taille de l'exécutable, la méthode est la même mais `Riff.exe` n'est pas l'ensemble de Riff (voir « Limites »).
- Recherche de Riff, série répétée : l'essai 1 vaut 525,3 ms et la médiane des essais 2 à 10 vaut 2 ms. Série distincte : l'essai 1 vaut 436,8 ms et la médiane des essais 2 à 10 vaut 383 ms (arrondi du JSON). Les valeurs brutes de la série répétée sont 525,3 / 2 / 1,8 / 2,2 / 2,6 / 2,1 / 1,9 / 2 / 3,3 / 1,9 ms.
- Défilement de riff-rs, détail par lancement (images, p50, p99, max, images au-delà de 16,67 ms, au-delà de 33,33 ms) : 5087, 5,219, 10,404, 13,637 ms, 0, 0 ; 5197, 5,015, 10,248, 13,532 ms, 0, 0 ; 5028, 5,239, 10,745, 16,749 ms, 1, 0.
- Processus comptés : Riff 6 au repos et 6 en lecture (scène a), riff-rs 1 dans les deux cas.
- Médianes des relevés d'une seconde par passe, scène a : Riff 80,6 et 91,1 % ; riff-rs 1,5 et 2,4 %. Les autres scènes figurent dans le JSON de la campagne, non copié dans le dépôt.

## Méthode, mesure par mesure

Isolation commune à toutes les mesures : un seul des deux programmes tourne à la fois. Chaque lancement utilise un profil neuf, avec `LOCALAPPDATA`, `APPDATA`, `RIFF_DATA_DIR` et `RIFF_IPC_NAME` redirigés vers un dossier de travail (pour Riff, `--user-data-dir` aussi). La fenêtre est écrite à 1280 x 800 (`window-state.json`) pour les deux programmes et n'est jamais réduite. Pour Riff, le fichier `.rifftutorial` portant la version est écrit, sinon Riff ouvre `welcome.html` au lieu de `index.html`. Les processus sont arrêtés par PID, jamais par nom, et le script vérifie en fin de mesure qu'il ne reste aucun processus lancé par lui. Une mesure qui échoue n'arrête pas les autres : elle est écrite « non mesurée » avec sa raison.

### RAM au repos

Somme du Private Working Set de l'arbre de processus (`scripts/measure.ps1`). 5 lancements, 30 s d'attente après le lancement, 3 relevés par lancement dont la médiane est gardée, puis médiane des 5 lancements. Accueil, profil vide. Valeurs brutes par lancement : Riff 289,8 / 285,3 / 284,9 / 287,5 / 285,3 Mo ; riff-rs 98,1 / 94,8 / 94,9 / 94,8 / 94,6 Mo.

### RAM en lecture

Même somme du Private Working Set, en un seul relevé, après 60 s de lecture de la scène a (`scripts/scenes/measure-scene.ps1`). Même titre local de 5 min pour les deux (mp3 généré par `scripts/scenes/fixture.mjs`, pochette et paroles chronométrées mot à mot), son coupé par la session audio Windows, paroles fermées. Médiane des passes valides.

### CPU moyen en lecture

Somme des temps CPU de l'arbre de processus, un relevé par seconde pendant 60 s, via `measure-scene.ps1`. La valeur du tableau est la médiane des moyennes des passes valides. Une passe est valide seulement si elle a produit exactement 60 relevés. Pour Riff, la lecture doit aussi être active : pas en pause et position supérieure à 8 s, sinon la passe est rejetée. Pour riff-rs, le journal de scène doit contenir la ligne `views`, ce qui prouve que la scène est allée au bout. Le script retente jusqu'à 2 fois de plus que le nombre de passes demandé (4 tentatives pour 2 passes). Les scènes sont définies par des réglages de Riff écrits dans le profil (voir `measure-scene.ps1`). Le temps GPU n'est pas compté.

### Démarrage jusqu'au premier rendu

Les deux méthodes diffèrent, et le tableau ne les met pas en rapport.

- riff-rs : lancement avec `--t0` (heure Unix en ms prise juste avant `Start-Process`) et `--startup-log`. L'application note le temps à la deuxième image présentée, borne haute du premier contenu, puis quitte.
- Riff : pas de deuxième image mesurable. Le temps est celui entre le lancement (même horloge Unix) et le First Contentful Paint de `index.html`, lu par le protocole de débogage (`timeOrigin` plus FCP, port ouvert). Il mesure le premier rendu de la coquille, pas des listes ni des pochettes.

Médiane de 5 lancements, profil vide, fenêtre 1280 x 800. Valeurs brutes : Riff 869 / 884,3 / 871,9 / 847,6 / 913 ms ; riff-rs 322 / 318 / 323 / 314 / 315 ms.

### Taille

Taille lue sur le disque, lecture seule. Riff : `Riff.exe` de la copie ; dossier d'installation désigné par `-RiffInstallDir` (défaut `%LOCALAPPDATA%\Programs\Riff`) ; installeur désigné par `-RiffInstaller` (défaut `riff-44.1.1-win-x64.exe` du dossier Téléchargements). Les chemins réellement utilisés ne sont pas dans le JSON, sauf celui de la copie. riff-rs : `riff-app.exe` de la release.

### Temps de recherche de Riff

Requête `daft punk one more time`, YouTube Music, 12 résultats, 10 essais à 0,5 s d'intervalle, requêtes réseau réelles, donc réseau variable. Le script lit la durée du `fetch` de `/api/search` lancé depuis la page, par le protocole de débogage (même requête que l'interface, sans le rendu). Deuxième série : une requête différente par essai (10 requêtes de la liste par défaut du script) pour contourner un cache. Le script attend 8 s après le lancement de Riff avant la première requête. Une série où une recherche échoue ou renvoie 0 piste est écrite « non mesurée ».

### Défilement de riff-rs

`--bench-scroll <fichier>` : défilement automatique de Titres aimés (5 000 pistes, 5 000 PNG distincts de 256 x 256 générés par le banc) à 2 000 px/s pendant 30 s après 3 s de chauffe. Valeur : p95 de l'intervalle entre images, médiane de 3 lancements. Un lancement est refusé s'il mesure moins de 500 images. L'intervalle suit le rythme de l'écran.

### Latence de début de lecture de riff-rs

`--latency-log` en mode `--scene a` avec le mp3 local des scènes : délai entre la demande du titre (activation de la première ligne des fichiers locaux, déclenchée par l'application) et la première position de lecture non nulle. Arrêt après 4 s. Médiane de 5 lancements.

### Reproduire

Depuis la racine du dépôt, Windows PowerShell 5.1, Node 22 ou plus récent pour les sondes de Riff. Valeurs par défaut du script : 5 lancements au repos avec 30 s d'attente, 5 de démarrage, scènes `a b b2 c d e` avec 2 passes de 60 s, 10 essais de recherche, 3 lancements de défilement, 5 de latence. Chemin de l'installation et de l'installeur de Riff modifiables par `-RiffInstallDir` et `-RiffInstaller`.

```powershell
node scripts\scenes\fixture.mjs <dossier fixture>
.\scripts\bench.ps1 -RsExe target\release\riff-app.exe -RiffExe "<copie de Riff>\Riff.exe" `
  -Fixture <dossier fixture> -Work <dossier de travail> -Out <dossier de sortie> -RsCommit 386ca31
```

Essai court : ajouter `-Only size,ram-idle -IdleRuns 1 -IdleWait 10`. Le script écrit `bench-results.json` (valeurs brutes, min, max, méthode, raison des absences) et `bench-table.md` dans le dossier de sortie. La copie de Riff doit être hors de l'installation réelle : le script refuse de lancer l'installation réelle.

### Bench court avant fusion

À lancer avant de fusionner tout changement qui touche au rendu, à la vue, aux animations, au profil de compilation ou aux dépendances. Il compare la branche (« après ») à sa base (« avant »), deux exécutables release copiés hors de `target`. Machine au repos, aucun build pendant la mesure, scripts lancés avec `&` dans la même session PowerShell. Durée : environ 50 min.

Ordre alterné en 2 tours : tour 1 avant puis après, tour 2 après puis avant. Chaque appel donne 2 passes de 60 s par scène, soit 4 passes par version et par scène. Garder les scènes a, b, c et e. Ajouter b2 ou d si le changement touche les paroles en dégradé ou le fond flou.

```powershell
$f = @{ Fixture = '<dossier fixture>'; Work = '<dossier de travail>' }
& .\scripts\bench.ps1 @f -RsExe <avant.exe> -RsCommit <base> -Only cpu -Scenes a,b,c,e -Out <sortie>\t1-avant
& .\scripts\bench.ps1 @f -RsExe <apres.exe> -RsCommit <branche> -Only cpu -Scenes a,b,c,e -Out <sortie>\t1-apres
& .\scripts\bench.ps1 @f -RsExe <apres.exe> -RsCommit <branche> -Only cpu -Scenes a,b,c,e -Out <sortie>\t2-apres
& .\scripts\bench.ps1 @f -RsExe <avant.exe> -RsCommit <base> -Only cpu -Scenes a,b,c,e -Out <sortie>\t2-avant
& .\scripts\bench.ps1 @f -RsExe <apres.exe> -RsCommit <branche> -Only size,ram-idle,startup -IdleRuns 3 -StartupRuns 3 -Out <sortie>\idle-apres
& .\scripts\bench.ps1 @f -RsExe <avant.exe> -RsCommit <base> -Only size,ram-idle,startup -IdleRuns 3 -StartupRuns 3 -Out <sortie>\idle-avant
```

Valeur comparée : médiane des 4 passes (les valeurs brutes sont dans `raw` de chaque `bench-results.json`), jamais un appel isolé.

| Mesure | Seuil de régression (après contre avant) | Bruit observé le 2026-11-23 |
|---|---|---|
| CPU d'une scène | médiane après <= médiane avant + 0,5 point | même exécutable, 4 passes d'une session : jusqu'à 4,1 points entre passes ; écart de médiane entre sessions : jusqu'à 1,45 point |
| RAM au repos | médiane après <= médiane avant + 2 Mo | 107,1 à 110,9 Mo |
| Démarrage | médiane après <= médiane avant + 20 ms | 324 à 332 ms, une valeur isolée à 749 ms |
| Taille de l'exécutable | toute hausse au-dessus de 0,5 Mo est expliquée | mesure exacte |
| Cibles absolues | RAM au repos <= 158 Mo, démarrage < 500 ms | |

Un seuil dépassé : refaire 2 tours (8 passes par version) avant de conclure. Pour le CPU, un dépassement à 8 passes n'est retenu comme régression que si un test de permutation exact le confirme : répartir de toutes les façons possibles les 16 passes (avant et après mêlées) en deux groupes de 8 (12 870 répartitions) ; p = part des répartitions où la médiane du premier groupe dépasse celle du second d'au moins l'écart observé. Régression retenue si p < 0,05 ; sinon l'écart est déclaré dans le bruit et ne bloque pas la fusion. Raison : sur un même exécutable (`8826f19`, 14 passes du 2026-11-23, toutes les répartitions 7 contre 7), le seuil de +0,5 point se déclenche dans 32,5 % des répartitions pour la scène c et 30,2 % pour la scène e. Limite : à 8 passes, une hausse réelle de l'ordre d'un point n'est pas séparable du bruit sur ces scènes (déduit de la même comparaison) ; le bench court n'écarte que les régressions nettes. Un gain annoncé est retenu seulement si le rapport des médianes atteint la cible annoncée et que les plages des passes avant et après sont disjointes.

Hors de ce bench : le CPU au repos en pause (une source d'images qui ne s'éteint pas au repos le fait monter au-dessus de 0) et le défilement, qui suit le rythme réel de l'écran. Les mesurer à part si le changement les touche.

## Non mesuré

Raisons reprises du JSON de la campagne (accents rétablis, sens inchangé), sauf la ligne « Machine modeste ».

| Mesure | Application | Raison |
|---|---|---|
| Temps de recherche, les deux séries | riff-rs | Aucun point d'entrée sans modifier le code : pas d'argument de recherche, pas de serveur HTTP local, `RIFF_NET_LOG` ne journalise que les hôtes (pas de durée). Piloter l'interface ne donne aucune heure de fin observable. |
| Latence de début de lecture, titre en ligne | Riff et riff-rs | Exige un titre en ligne (réseau) et, pour riff-rs, une activation de ligne par entrée souris ou clavier (`--latency-log` ne réagit qu'à `RowActivated`, `RowReleased` et `Next`). Aucune injection fiable sans voler le curseur ou le focus. |
| Latence de début de lecture, fichier local | Riff | Non scriptée : la mesure par le protocole de débogage de Chromium est peu fiable (4 essais sur 9 sans lecture, valeurs à 42 ms sans cause établie), ordre de grandeur seulement. |
| Défilement de 5 000 pistes | Riff | Pas de mesure équivalente dans Riff : aucun argument de défilement mesuré, seul riff-rs expose `--bench-scroll`. |
| Dossier d'installation | riff-rs | Mesuré dans le tableau final : 48,62 Mo. Reste non mesuré dans le tableau principal du 2026-11-14 (exécutable seul à l'époque). |
| Installeur | riff-rs | Mesuré dans le tableau final : 13,50 Mo. Reste non mesuré dans le tableau principal du 2026-11-14 (installeur non produit à l'époque). |
| Machine modeste | les deux | Aucune mesure : la campagne tourne sur une seule machine. Cette mesure est hors périmètre. |

## Limites de la comparaison

Chaque point vient du JSON de la campagne, de `scripts/bench.ps1`, de `scripts/bench-riff.mjs`, de `scripts/scenes/` ou de la section « Historique ».

- Démarrage : deux méthodes. riff-rs note la deuxième image présentée (borne haute du premier contenu). Riff est mesuré au First Contentful Paint de la coquille `index.html`, lu par le protocole de débogage avec un port ouvert, sans les listes ni les pochettes. Les deux valeurs ne se mesurent pas au même événement.
- Paroles de Riff : le pilote de scène clique sur le bouton du panneau de paroles pour le fermer dans les scènes sans paroles (toutes sauf `b` et `e`, d'après `scripts/scenes/riff-driver.mjs`). Déduit : le panneau est donc ouvert par défaut sur l'accueil. La RAM au repos de Riff n'est pas mesurée par ce pilote, mais par `measure.ps1` sur l'accueil. L'état du panneau pendant cette mesure n'est pas relevé.
- Taille de la fenêtre : le script écrit 1280 x 800 dans le fichier d'état des deux applications. La taille réellement prise par Riff n'est pas consignée dans les résultats de la campagne, bien que le pilote sache lire `innerWidth` et `innerHeight`.
- RAM en lecture et CPU : 2 passes seulement par scène et par application. L'écart entre les deux passes est parfois grand : CPU de la scène a de riff-rs 1,3 et 3,2 %, scène c 8,8 et 11,8 %. La série des scènes de l'historique signale déjà une variation de série à série (voir « Historique »).
- RAM en lecture : un seul relevé par passe, après 60 s.
- Recherche : un cache du serveur de Riff rend les essais répétés presque gratuits (essai 1 à 525,3 ms, suivants vers 2 ms). Seule la série à requêtes distinctes évite ce cache. Dans les deux cas le réseau varie, et la valeur mesurée est la durée du `fetch` sans le rendu de l'interface.
- Temps GPU non compté dans le CPU. Riff est une application Electron (Chromium), riff-rs une application `iced` en DX12 : leur travail GPU n'est pas relevé.
- Autres applications ouvertes sur la machine pendant la campagne : non relevées. Les séries des scènes et du contournement du pilote notent la même incertitude.
- Une seule machine, un seul écran, un seul GPU (RX 7800 XT) et un seul pilote. Le contournement du pilote AMD (`DRIVER_WARMUP_FRAMES = 240`, voir « Historique ») est actif dans le binaire mesuré. Son effet sur un autre GPU n'est pas testé.
- Fréquence de l'écran : 240 Hz lu par CIM. Dans la série des scènes, CIM et cadence réelle ont divergé : le relevé CIM annonçait déjà 59 Hz alors que les images arrivaient à 240 Hz, puis la cadence observée est passée à 60 images par seconde, sans cause établie. Le p95 de défilement suit le rythme de l'écran.
- Taille : `Riff.exe` seul (212,74 Mo) ne représente pas Riff. Le programme charge aussi ses DLL, ses `.pak` et ses `resources`. Le dossier d'installation fait 395,52 Mo. riff-rs est un exécutable seul, sans dossier ni installeur mesurés.
- Fonctionnalités inégales : Riff anime aussi sa barre de progression ondulée et ses transitions, absentes de riff-rs. Le CPU des deux programmes ne mesure donc pas exactement le même travail.
- Mêmes fichiers pour les deux (titre, paroles, fenêtre), mais les paroles viennent du cache disque de chaque application, pas du réseau. Les paroles ne sont pas des paroles personnalisées, car Riff décale ces lignes d'une et aucun mot n'y devient actif (commentaire de `measure-scene.ps1`).
- Version : la copie de Riff est la version publiée 44.1.1 (Electron). Les valeurs historiques ci-dessous viennent de `electron .` non empaqueté et d'un autre profil de fenêtre : elles ne sont pas comparables au tableau.

## Mesure plugins

Source : mesure à part du 2026-11-14, même machine et même binaire `386ca31` que la campagne ci-dessus, `scripts\measure.ps1`. Elle vérifie le seuil de 5 Mo de surcoût des plugins.

Méthode : 5 lancements, 30 s d'attente, 3 relevés, accueil. Dossiers isolés par `RIFF_DATA_DIR`, `LOCALAPPDATA` et `APPDATA`. Avec plugins : `hello.rhai` et `better-shuffle.rhai` activés, consentement écrit dans `riff-plugins.json`, dossier `state` créé par l'application pendant la mesure. Les deux séries se suivent, sans être entrelacées.

| Série | Médiane | Min à max | Lancements |
|---|---|---|---|
| Sans plugin actif | 95,2 Mo | 94,7 à 97,2 | 97,2 / 95,2 / 95,8 / 94,8 / 94,7 |
| `hello.rhai` et `better-shuffle.rhai` activés | 96,2 Mo | 95,9 à 99,0 | 99,0 / 95,9 / 96,1 / 96,2 / 96,2 |

Surcoût : 1,0 Mo de médiane (seuil : 5 Mo). La série sans plugin (95,2 Mo) est distincte de la ligne « RAM au repos » du tableau principal (94,8 Mo) : autre série de lancements, même binaire, même jour.

## Historique

Valeurs antérieures du dépôt, avec leur source. Elles portent sur d'autres commits, parfois sur une autre méthode, et ne se mélangent pas au tableau du jour. Machine : un Ryzen 9 5900X et une RX 7800 XT pour la série des scènes, une RX 7800 XT pour la série du contournement du pilote, seulement Windows 11 pour la version Tauri.

### Version Tauri

Date de mesure non indiquée ; dernière modification de la source le 2026-11-02 (date de commit). Il s'agit de la première version de riff-rs, construite avec Tauri et WebView2, non du portage actuel.

| Mesure | Riff 44.1.1 (`electron .`) | riff-rs Tauri avant | riff-rs Tauri après |
|---|---|---|---|
| RAM au repos, Private Working Set, médiane | 317 Mo (298 à 351) | 289 Mo (288 à 344) | 233 Mo (230 à 234) |
| Processus | 6 | 8 | 8 |
| Installeur Windows | 102,5 Mo | 7,2 Mo (NSIS) | idem |
| `app.exe` release | | 23,6 Mo | idem |

Méthode de cette section : 5 lancements, 30 s d'attente, 3 relevés. Fenêtre maximisée 1920 x 1080, accueil. Riff lancé par `electron .` depuis une copie du dépôt de référence de Riff (non empaqueté, Electron 41.7), avec Discord et un navigateur ouverts. Le gain « après » vient de `--enable-low-end-device-mode`, repassé en option (`RIFF_LOW_MEM=1`), donc la valeur par défaut était 289 Mo. Le même texte note que le démarrage n'était pas comparable proprement : 31 ms de fenêtre visible pour riff-rs (contenu vide) contre environ 550 ms pour Electron.

### Défilement, RAM et démarrage (commité le 2026-11-03)

Date exacte de mesure non indiquée. Mesures de la version finale de cette série, fixture de 5 000 pistes et 5 000 PNG, `scripts/measure.ps1`.

| Mesure | Résultat |
|---|---|
| Défilement, 2 000 px/s, 30 s | p50 5,2 ms, p95 11,9 ms, p99 14,6 ms, max 15,9 ms, 0 image manquée |
| RAM au repos, 5 lancements, accueil | médiane 157,0 Mo (156,4 à 157,6) |
| Démarrage, 5 lancements (`--t0`) | médiane 322 ms (314 à 324) |

Délai entre clic et début de lecture, mesuré sur `fb94f17` et non refait : fichier local 78 ms ; titre en ligne 187, 200, 248 et 315 ms ; titre d'album 638 à 832 ms. Pour Riff, par le protocole de débogage de Chromium : 211, 212, 419, 596 ms et 42 ms sur un titre sans cause établie, avec 4 essais sur 9 sans lecture dans les 15 s. Cette comparaison est un ordre de grandeur.

Ces valeurs servent de valeurs de départ (défilement 11,9 ms, RAM 157,0 Mo, démarrage 322 ms, début de lecture en ligne 187 à 315 ms).

### Scènes (commité le 2026-11-03)

Même titre pour les deux programmes, 60 s de relevés, fenêtre 1280 x 800, copie de l'installé de Riff 44.1.1 sans `app-update.yml`. La colonne riff-rs est la version `c93d7d3`. CPU en % d'un cœur.

| Scène | Riff passe 1 | Riff passe 2 | riff-rs `c93d7d3` |
|---|---|---|---|
| a, rien d'affiché | 88,3 | 94,0 | 1,6 |
| b, paroles mot à mot | 144,4 | 103,2 | 3,1 |
| b2, paroles en dégradé | 116,6 | 119,1 | 3,1 |
| c, barres et forme | 151,5 | 127,6 | 7,9 |
| d, fond de pochette flou | 96,2 | 104,4 | 1,5 |
| e, paroles, barres, forme, fond flou | 129,1 | 137,7 | 9,2 |

Réserves notées : une série précédente de riff-rs donnait 0 / 6,3 / 12,6 / 7,6 / 0 / 14,1 pour les mêmes scènes ; une autre session tournait sur la machine pendant une partie des mesures ; Riff anime sa barre de progression ondulée et ses transitions, absentes de riff-rs ; le temps GPU n'est pas compté. RAM de riff-rs : accueil 152,0 Mo, scène a 148,5 Mo, scène e 152,4 Mo, plein écran 150,6 Mo, mesh 160,0 Mo, mini-paroles 156,0 Mo. Cette série note aussi un défaut du pilote AMD, un cœur occupé en continu sans image présentée, déjà présent dans la série précédente.

### Vulkan contre DX12 (2026-11-26)

Commit `4569ce9`, release (`lto = "fat"`), RX 7800 XT. A/B alterné en 2 tours (`methode/prof/ab-wgpu.ps1`, hors dépôt, appelle `bench.ps1 -Only ram-idle,startup`) : DX12 par défaut contre `WGPU_BACKEND=vulkan`. 4 lancements au repos (attente 30 s, CPU sur 15 s) et 6 démarrages par version.

| Mesure | DX12 | Vulkan |
|---|---|---|
| RAM privée au repos | 110,8 Mo (110,2 à 111,1) | 130,6 Mo (127,9 à 131,8) |
| Commit privé au repos | 211,6 Mo (205,2 à 212,8) | 299,1 Mo (295,4 à 300,9) |
| CPU au repos, par lancement | 0 / 0 / 0,1 / 0 % | 0,1 / 99,5 / 0,1 / 99,4 % |
| Démarrage (médiane) | 383 ms | 297 ms |

En Vulkan, `amdvlk64.dll` est chargé et un cœur reste à 100 % dans 2 lancements sur 4 malgré `DRIVER_WARMUP_FRAMES = 240`. DX12 reste le backend forcé (`main.rs`). La RAM au repos en DX12 (110,8 Mo) dépasse le seuil de 105 Mo, contre 95,4 Mo sur `634fd15` : cause non cherchée.

### RAM et CPU au repos (commité le 2026-11-11)

Commit mesuré `634fd15`, release, DX12 par défaut, données isolées. RAM privée au repos, accueil vide, 1 processus : 97,0 / 95,2 / 95,4 / 94,9 / 97,4 Mo, médiane 95,4 Mo (94,9 à 97,4). Références : `2b68a68` 98,7 Mo (93,5 à 100,1), base précédente 98,6 Mo (93,2 à 99,1). CPU au repos : 0,00 / 0,10 / 0,00 % d'un cœur sur 15 s. Seuils : 105 Mo et 2 %.

### Contournement du pilote AMD (commité le 2026-11-10)

Commit `8a663a5`, `riff-app.exe` release de 36 980 224 octets, DX12 par défaut, `DRIVER_WARMUP_FRAMES = 240`, fenêtre 1280 x 800, son coupé, 1 processus par scène, une seule série.

| Scène | RAM privée (Mo) | CPU médian (% d'un cœur) | p95 de l'intervalle entre images (ms) |
|---|---|---|---|
| a | 85,5 | 0,0 | sans image animée |
| e | 96,6 | 12,6 | 17,05 |
| mesh | 95,0 | 12,5 | 17,06 |
| mini | 87,5 | 7,9 | 17,25 |

Cette série donne aussi la RAM de l'accueil par `measure.ps1`, dans un seul dossier isolé pour les 5 lancements : 93,8 / 101,1 / 101,0 / 100,3 / 101,2 Mo, médiane 101,0 Mo. Avec un dossier neuf à chaque lancement : 92,8 à 93,9 Mo. Il note que le p95 d'environ 17 ms correspond à la cadence du minuteur de 60 Hz, non à l'écran, et que la CPU de la scène e est plus haute que dans la série des scènes (12,6 contre 9,2) sans cause attribuée.

Essai du contournement du pilote AMD : `DRIVER_WARMUP_FRAMES = 0` donne 94,8 à 99,96 % d'un cœur au repos sur 5 lancements sur 5 ; à 240, 0,00 à 0,08 %.

### Écarts entre l'historique et la campagne du jour

La RAM au repos de riff-rs passe de 95,4 Mo (série RAM et CPU au repos) et de 101,0 Mo ou 92,8 à 93,9 Mo (série du contournement du pilote, selon le dossier) à 94,8 Mo. La RAM de Riff passe de 317 Mo (version Tauri, méthode différente) à 285,3 Mo. Le CPU de riff-rs varie d'une série à l'autre (scène e : 9,2 dans la série des scènes, 12,6 dans celle du contournement du pilote, 11,75 ici). Ces écarts ne sont pas expliqués ici.
