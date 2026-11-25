# Porter un plugin JavaScript de Riff vers Rhai

Les plugins JavaScript de Riff ne fonctionnent pas dans cette version : Riff n'embarque plus de moteur JavaScript ni de page web. Dans l'original, un plugin était exécuté par `new Function('Riff', 'plugin', code)` dans la fenêtre de l'application, avec un accès complet au DOM, à `localStorage`, à `fetch` et à l'élément audio, sans permission. Un plugin Rhai déclare ses permissions et ne voit que l'API décrite dans [README.md](README.md).

## Correspondance de l'API

| JavaScript (Riff) | Rhai | Remarque |
|---|---|---|
| en-tête `@name`, `@author`, `@version`, `@description` | identique, en commentaires `//` | ajouter `@permissions` |
| `Riff.on('ready', fn)` | `fn on_ready()` | fonction nommée, pas de rappel |
| `Riff.on('play', fn)` | `fn on_play()` | |
| `Riff.on('pause', fn)` | `fn on_pause()` | |
| `Riff.on('trackchange', fn)` | `fn on_trackchange(track)` | |
| objet renvoyé `{ stop() {...} }` | `fn stop()` | |
| `Riff.toast(message, type)` | `notify(message, type)` | permission `notify`, 5 par 10 s |
| `Riff.getTrack()` | `track()` | |
| `Riff.isPlaying()` | `is_playing()` | |
| `Riff.sameTrack(a, b)` | `same_track(a, b)` | |
| `Riff.shuffle.setProvider(fn)` | `fn shuffle_pick(ctx)` | permission `shuffle` ; renvoyer `()` au lieu de `null` |
| `Riff.shuffle.isOn()` | `shuffle_on()` | permission `shuffle` |
| `Riff.queue.get()` | `queue()` | permission `queue` |
| `Riff.queue.getIndex()` | `queue_index()` | permission `queue` |
| `Riff.queue.getContext()` | `queue_context()` | `"queue"`, `"radio"` ou `"wave"` : l'origine de la file (recherche, playlist) n'est pas connue |
| `Riff.queue.append(pistes)` | `queue_append(pistes)` | permission `queue`, 50 titres par appel |
| `Riff.library.favorites()` | `favorites()` | permission `library` |
| `Riff.library.history()` | `history()` | permission `library` |
| `await Riff.api.search(q, { limit, platform, signal })` | `search(q, #{ limit, platform })` | permission `search` ; appel bloquant (10 s au plus), pas de `signal` |
| `await Riff.api.vibe(corps)` | `vibe(corps)` | permission `search` |
| `Riff.storage.get(clé, défaut)` / `set(clé, valeur)` | `this.clé` | état enregistré automatiquement ; clés `_…` non enregistrées |
| `Math.random()` | `random()` | |
| `console.log(...)` | `log(message)` | |
| `Riff.audio()` | aucun | pas d'élément audio : le moteur audio est natif |
| `Riff.query(sélecteur)` | aucun | pas de DOM |
| `Riff.addStyle(css)`, `style[data-plugin]` | aucun | pas de CSS : les thèmes natifs (Réglages, Thèmes personnalisés) les remplacent |
| `fetch`, `localStorage`, `setTimeout`, `Promise` | aucun | pas de réseau direct, pas de minuteur ; l'état passe par `this` |
| CSS personnalisé `riff_custom_css`, `riff_custom_css_backup` | non repris | remplacés par les thèmes natifs |

## Ce qui change dans l'écriture
- **Pas de rappels enregistrés** : les fermetures de Rhai ne partagent pas l'état comme celles de JavaScript. Riff appelle des fonctions par leur nom.
- **Pas d'asynchrone** : `search` et `vibe` attendent leur réponse. Le budget de temps du plugin ne compte pas cette attente. Un travail réseau lancé « sans attendre » en JavaScript se fait dans un événement suivant (voir Better Shuffle).
- **Pas de variables globales modifiables** : l'état vit dans `this`. Une fonction d'aide qui doit le modifier s'appelle `this.aide(...)`.
- **Valeurs** : `null`/`undefined` deviennent `()`. Un tableau ou une carte passé à une fonction est copié ; pour le modifier sur place, appeler la fonction comme une méthode (`tableau.echanger(i, j)`, `this` y désigne le tableau).
- **Syntaxe** : `let`, `fn`, `if`/`else` sans parenthèses obligatoires, cartes `#{ clé: valeur }`, chaînes avec interpolation entre accents graves `` `titre : ${t.title}` ``, `for x in tableau`, `for i in 0..n`.

## Exemple : Better Shuffle
`better-shuffle.js` (317 lignes) devient [`examples/better-shuffle.rhai`](examples/better-shuffle.rhai). Mêmes modes (`classic`, `spotify`, `smart`) et mêmes réglages (`artistsToExpand` 4, `perArtist` 10, `maxExpand` 50, `smartEvery` 3, `smartBatch` 10). Différences :
- le mode se choisit dans la fonction `default_mode()` du fichier (le clic droit sur le bouton de mélange demandait le DOM) ; changer le fichier demande de réactiver le plugin ;
- l'apprentissage des titres passés avant 30 s (`skipSec`) n'est pas porté : il lisait la position de l'élément audio ; le goût (`taste`) reste lu et enregistré, les favoris comptent dans le score du mode `smart` ;
- l'élargissement de la recherche ne se déclenche que si `ctx.context == "search"`, ce que Riff natif ne signale pas encore (`queue_context()` ne connaît que `queue`, `radio`, `wave`) ;
- la recherche et les recommandations, lancées sans attendre en JavaScript, sont faites au changement de titre qui suit la création de la session ;
- les marques visuelles sur le bouton de mélange (`addStyle`) ne sont pas portées.
