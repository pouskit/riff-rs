# Plugins de Riff (Rhai)

Les plugins de cette version de Riff sont écrits en [Rhai](https://rhai.rs), un petit langage de script embarqué, à la syntaxe proche de Rust et de JavaScript. **Les plugins JavaScript de Riff ne sont pas compatibles** : voir [PORTAGE.md](PORTAGE.md) pour les réécrire.

Un plugin ne peut rien faire d'autre que ce que cette page décrit : aucun accès aux fichiers, au réseau, aux processus ou à l'interface. Il ne voit que les fonctions de l'API ci-dessous, et seulement celles des permissions qu'il déclare.

## Installer, activer, retirer
- Les plugins sont dans `<dossier de données>/plugins/`. Réglages, catégorie **Plugins** : *Importer un plugin* (copie d'un fichier `.rhai` après vérification), *Ouvrir le dossier*, *Nouveau plugin*, *Modifier*, interrupteur d'activation, croix de suppression.
- Les permissions d'un plugin sont affichées dans la liste **avant** son activation. Un plugin importé n'est jamais activé d'office.
- L'activation est liée au contenu exact du fichier (empreinte SHA-256). Si le fichier change (mise à jour, modification dans l'éditeur), le plugin s'arrête et attend une nouvelle activation, après relecture de ses permissions.
- Au premier lancement, l'exemple `hello.rhai` est copié dans le dossier, désactivé.
- Les fichiers `.js` du dossier sont listés « incompatibles » et ne sont jamais exécutés.

## Forme d'un plugin
Un fichier `.rhai` d'au plus 1 Mo, au nom fait de lettres et chiffres ASCII, `.`, `_`, `-` et espaces (`^[A-Za-z0-9._ -]+\.rhai$`). Il commence par un en-tête en commentaires :

```rhai
// @name Hello
// @author Moi
// @version 1.0.0
// @description Notification au changement de titre.
// @permissions notify
```

`@name` est obligatoire. `@permissions` liste, séparées par des virgules, des permissions parmi `queue`, `library`, `search`, `shuffle`, `notify`. Sans `@permissions` : aucune permission. Une permission inconnue rend le fichier invalide.

Le plugin déclare ensuite des fonctions **par leur nom** ; Riff les appelle quand il le faut :

| Fonction | Appelée | Budget |
|---|---|---|
| `on_ready()` | à l'activation | 500 ms |
| `on_play()`, `on_pause()` | au début et à l'arrêt de la lecture | 500 ms |
| `on_trackchange(track)` | à chaque nouveau titre (`track` est une carte, ou `()`) | 500 ms |
| `shuffle_pick(ctx)` | en lecture aléatoire, pour choisir le titre suivant (permission `shuffle`) | 3 s |
| `stop()` | à la désactivation | 500 ms |

Une fonction absente n'est simplement pas appelée.

### État : `this`
Les fonctions d'un script Rhai ne voient pas les variables globales. L'état du plugin est donc la carte `this`, disponible dans toutes les fonctions ci-dessus et conservée entre les appels :

```rhai
fn on_play() {
  if this.plays == () { this.plays = 0; }
  this.plays += 1;
}
```

`this` est enregistré dans `plugins/state/<fichier>.json` (au plus 256 Ko, écriture atomique) et relu à l'activation suivante. Les clés qui commencent par `_` restent en mémoire et ne sont pas enregistrées (pratique pour un cache). Une fonction d'aide peut lire et modifier `this` si on l'appelle comme une méthode : `this.aide(x)`.

Les instructions placées hors des fonctions sont réévaluées avant chaque appel : réservez-les aux constantes, ou mettez vos réglages dans une fonction (`fn cfg() { #{ ... } }`).

## API
Une fonction protégée par une permission **n'existe pas** dans le moteur sans cette permission ; l'appeler donne l'erreur « permission refusée : <permission> ».

| Fonction | Permission | Rôle |
|---|---|---|
| `track()` | aucune | titre en cours (carte) ou `()` |
| `is_playing()` | aucune | lecture en cours |
| `same_track(a, b)` | aucune | même titre (plateforme et identifiant, sinon URL, sinon titre, artiste et durée) |
| `random()` | aucune | nombre de 0 (inclus) à 1 (exclu) |
| `log(message)` | aucune | ligne dans le journal de Riff, masquée des jetons (20 lignes par 10 s au plus) |
| `queue()` | `queue` | file de lecture (2 000 titres au plus) |
| `queue_index()` | `queue` | position du titre en cours dans la file, ou `()` |
| `queue_context()` | `queue` | `"queue"`, `"radio"` ou `"wave"` |
| `queue_append(pistes)` | `queue` | ajoute les titres absents de la file (50 par appel), renvoie ceux ajoutés |
| `favorites()`, `history()` | `library` | titres aimés, historique (lecture seule) |
| `search(q)`, `search(q, #{ limit, platform })` | `search` | recherche par Riff, `limit` de 1 à 30 (10 par défaut) |
| `vibe(corps)` | `search` | recommandations, même corps que `/api/vibe` |
| `shuffle_on()` | `shuffle` | lecture aléatoire activée |
| `notify(message)`, `notify(message, type)` | `notify` | notification (`info`, `success`, `warning`, `error`), 200 caractères, 5 par 10 s ; renvoie `false` si refusée |

Un titre est une carte aux seuls champs `id`, `title`, `artist`, `album`, `duration`, `platform`, `url` et `thumbnail` (adresses en `https://` seulement). Aucun chemin local, cookie ou identifiant de compte n'est transmis. Un titre ajouté par `queue_append` doit avoir un `title` et un `id` ou une `url`.

`ctx` de `shuffle_pick` : `#{ queue, queue_index, context, current }`. La fonction renvoie un titre de la file, ou `()` pour laisser Riff tirer au hasard. Riff attend 3,5 s au plus, puis tire au hasard ; il le fait aussi si le titre renvoyé n'est pas dans la file.

`search` et `vibe` passent par Riff (mêmes serveurs et mêmes contrôles que la recherche de l'application), 30 appels par minute au plus ; au-delà, elles renvoient une liste vide. L'attente d'une réponse (10 s au plus) ne compte pas dans le budget du plugin.

Langage disponible : opérateurs, chaînes, tableaux, cartes, boucles, fonctions et fermetures de Rhai, fonctions mathématiques. Absents : `eval`, `import` de fichiers, `sleep`, `exit`, heure, E/S. `print` et `debug` n'affichent rien : utilisez `log`.

## Limites et sécurité
- Chaque plugin a son propre moteur Rhai et son propre fil d'exécution (aucun fil, aucun coût sans plugin actif). Il n'est jamais exécuté sur le fil de l'interface ni sur celui de l'audio.
- Limites : temps (tableau ci-dessus), opérations (40 000 par milliseconde de budget), 32 niveaux d'appels, expressions de profondeur 64 (32 dans une fonction), chaînes de 256 Ko, tableaux et cartes de 10 000 éléments, 1 024 variables, 512 fonctions, aucun module.
- Un appel qui dépasse son budget désactive le plugin aussitôt ; trois erreurs de suite aussi. La raison est affichée et retenue : le plugin reste désactivé au lancement suivant.
- La désactivation interrompt l'appel en cours, appelle `stop()`, puis détruit le moteur : aucun redémarrage de Riff n'est nécessaire.
- La mémoire d'un plugin n'a pas de plafond global (Rhai n'en a pas) : elle est bornée indirectement par les tailles ci-dessus.
- Un plugin Rhai partage le processus de Riff : son isolement repose sur la surface de l'API et sur ces limites, pas sur une séparation matérielle.

## Éditeur
*Modifier* ou *Nouveau plugin* ouvre l'éditeur de code : coloration (grammaire Rust, la plus proche de Rhai), ligne et colonne du curseur, recherche (`Ctrl+F`, suivant, précédent, remplacer, tout remplacer, casse), enregistrement (`Ctrl+S`) avec vérification de la syntaxe. Un fichier modifié affiche `•` ; fermer sans enregistrer demande confirmation. L'éditeur ouvre les fichiers jusqu'à **128 Ko** : au-delà, l'ouverture bloquerait l'interface plus de 100 ms (mesure : 77 ms à 128 Ko, 616 ms à 1 Mo, en release). Un plugin plus gros fonctionne, mais se modifie dans un autre éditeur. Pas de colonne de numéros de ligne : la position du curseur est dans la barre d'état.

## Exemples
- [`examples/hello.rhai`](examples/hello.rhai) : notification à chaque nouveau titre.
- [`examples/better-shuffle.rhai`](examples/better-shuffle.rhai) : portage de Better Shuffle (modes `classic`, `spotify`, `smart`).
