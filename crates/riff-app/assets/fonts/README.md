# Polices embarquées : régénérer les fichiers

Les cinq `.ttf` de ce dossier sont des sous-ensembles de deux polices variables de Google. Aucun outil de découpe n'est dans le dépôt : la procédure ci-dessous le reconstruit. Les licences sont dans `OFL-GoogleSans.txt` et `LICENSE-MaterialSymbols-Apache-2.0.txt`, l'attribution dans `NOTICE.md`.

## Sources

Les tailles et le contenu ont été revérifiés sur GitHub le 17 novembre 2026 (tailles identiques, empreintes non recalculées en amont). Les sources ne portent pas de numéro de version : l'empreinte SHA-256 est la seule référence exacte.

| Fichier source | URL | Taille | SHA-256 |
|---|---|---|---|
| `GS.ttf` (Google Sans, axes `GRAD` -50..200, `opsz` 17..18, `wght` 400..700) | `https://raw.githubusercontent.com/google/fonts/main/ofl/googlesans/GoogleSans%5BGRAD,opsz,wght%5D.ttf` | 4 974 940 | `d0a87d835a944b8b40d0e82a5651bb59ab97b936a2aeed5946eb57e7b2a3a90a` |
| `MS.ttf` (Material Symbols Outlined, variable `FILL`, `GRAD`, `opsz`, `wght`) | `https://raw.githubusercontent.com/google/material-design-icons/master/variablefont/MaterialSymbolsOutlined%5BFILL,GRAD,opsz,wght%5D.ttf` | 10 744 832 | `f7837ba8573d9a7a74931618ced8307bc81f96d49af296bf6c8052c772ed5282` |
| `MS.codepoints` (nom d'icône, point de code) | même dossier, extension `.codepoints` | 79 421 | `a7404d9e5a1b63f496cf05ace45d386075886b5a7f123fba9f29527cb6006fde` |

## Outil

Node.js 26.12.0 et le paquet npm `subset-font` 2.9.0 (WebAssembly de HarfBuzz via `harfbuzzjs` 1.6.4, licences permissives). À installer dans un dossier de travail hors du dépôt :

```
npm init -y
npm install subset-font@2.9.0
```

## Procédure

Placer `GS.ttf`, `MS.ttf` et `MS.codepoints` dans `../dl/` (relatif au script), enregistrer le script sous `build.mjs` à côté de `node_modules`, puis `node build.mjs <dossier de sortie>`.

```js
import subsetFont from 'subset-font';
import fs from 'fs';
const dl = '../dl/', out = process.argv[2];
fs.mkdirSync(out, { recursive: true });

// Google Sans : une instance statique par graisse, sous-ensemble latin, grec, cyrillique.
const ranges = [
  [0x20, 0x7e], [0xa0, 0x24f], [0x300, 0x36f], [0x370, 0x3ff], [0x400, 0x52f], [0x1e00, 0x1eff],
  [0x2000, 0x206f], [0x20a0, 0x20bf], [0x2190, 0x2193], [0x2212, 0x2212], [0x2022, 0x2022],
  [0x2026, 0x2026], [0x2116, 0x2116], [0x2122, 0x2122],
];
const chars = [];
for (const [a, b] of ranges) for (let c = a; c <= b; c++) chars.push(String.fromCodePoint(c));
const latin = chars.join('');
const gs = fs.readFileSync(dl + 'GS.ttf');
for (const [w, n] of [[400, 'Regular'], [500, 'Medium'], [600, 'SemiBold'], [700, 'Bold']]) {
  const b = await subsetFont(gs, latin, { targetFormat: 'truetype', variationAxes: { wght: w, opsz: 18, GRAD: 0 } });
  fs.writeFileSync(`${out}/GoogleSans-${n}.ttf`, b);
}

// Material Symbols : le seul glyphe que Riff affiche, `discover_tune` (index.html:504), FILL 0.
const codepoints = new Map(fs.readFileSync(dl + 'MS.codepoints', 'utf8').split('\n').filter(Boolean).map((l) => l.split(' ')));
const icons = ['discover_tune'].map((name) => String.fromCodePoint(parseInt(codepoints.get(name), 16))).join('');
const ms = fs.readFileSync(dl + 'MS.ttf');
const outlined = await subsetFont(ms, icons, { targetFormat: 'truetype', variationAxes: { FILL: 0, wght: 400, opsz: 24, GRAD: 0 } });
fs.writeFileSync(`${out}/MaterialSymbols-Outlined.ttf`, outlined);
```

Plages Unicode de Google Sans : U+0020-007E (latin de base), U+00A0-024F (latin-1 et latin étendu A et B), U+0300-036F (diacritiques combinants), U+0370-03FF (grec), U+0400-052F (cyrillique et supplément), U+1E00-1EFF (latin étendu additionnel), U+2000-206F (ponctuation générale), U+20A0-20BF (symboles monétaires), U+2190-2193 (flèches), U+2212 (moins), U+2022 (puce), U+2026 (points de suspension), U+2116 (numéro), U+2122 (marque). Axes figés : `opsz` 18, `GRAD` 0, `wght` 400, 500, 600 ou 700 selon le fichier.

## Résultat attendu

Avec les sources ci-dessus, la sortie est identique octet pour octet aux fichiers du dépôt (vérifié par SHA-256, y compris avec le script de la section précédente pour `MaterialSymbols-Outlined.ttf`).

| Fichier | SHA-256 |
|---|---|
| `GoogleSans-Regular.ttf` | `be41a1023de20cee8aa6ab51eed9b61890aa14cf72f6edd3d5ad584bb2e66c9f` |
| `GoogleSans-Medium.ttf` | `bc8bf0b97e4de2d7704cf5c4f229bd47d58762328ccf83caaeb32d34d653bfea` |
| `GoogleSans-SemiBold.ttf` | `497d1cdecc2537544ca8f76792b07dc6a33f0244eed2d37f972a4ef46cf8079e` |
| `GoogleSans-Bold.ttf` | `617211156219457dba672e044230804a7b9846af336a55a5ce6e760c53fc615b` |
| `MaterialSymbols-Outlined.ttf` | `f22c507e2158c107fd21a7a4030f0e53f99da15fd9b67a268691970260984814` |

Pour ajouter une icône Material Symbols, ajouter son nom dans le tableau `['discover_tune']` (les noms sont dans `MS.codepoints`) et ajouter son point de code à `theme::icons`. Riff n'en utilise qu'une : toutes les autres icônes sont des SVG (`../icons/`).
