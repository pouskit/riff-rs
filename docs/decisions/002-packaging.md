# Décision 002 : outil d'empaquetage

**Statut : décidé le 2026-11-22 : `cargo-packager`.**

Recommandation : **`cargo-packager` 0.11.8** pour les trois plateformes (NSIS, AppImage, dmg et `.app`), complété par deux commandes manuelles sur macOS (zip et signature ad hoc). Repli : NSIS, `linuxdeploy`/`appimagetool` et scripts macOS appelés directement.

Document rédigé le 2026-11-22, sans rien installer ni empaqueter. Aucun outil n'a été exécuté : toutes les cases viennent de la lecture du code source ou de la documentation de la version exacte (liens en fin de document).

Légende : **V** = vérifié dans la source ou la documentation citée (référence entre crochets). **D** = déduit, non confirmé par une exécution ni par un texte explicite. **NT** = non trouvé dans la documentation lue.

## Cible de l'original (faits vérifiés)
Riff 44.1.1, `electron-builder` 26 :
- Windows : NSIS x64, `oneClick: false`, `perMachine: false`, dossier d'installation au choix, `deleteAppDataOnUninstall: false`.
- Linux : AppImage x64, catégorie `AudioVideo`.
- macOS : dmg et zip, x64 et arm64, `identity: null`, `hardenedRuntime: false`, signature ad hoc par script.
- Nom d'artefact `${name}-${version}-${os}-${arch}.${ext}`, identifiant `io.github.rootscripts.riff`, mises à jour par `electron-updater`.

Contraintes de riff-rs : mise à jour à la demande avec vérification minisign faite par l'application, release en brouillon seulement, actions fixées par SHA, `contents: write` limité au travail qui publie, aucune télémétrie.

## État du dépôt qui touche l'empaquetage (vérifié, `7984840`)
- Binaire : `[[bin]] riff-app` dans `crates/riff-app/Cargo.toml`. Version `0.1.0` dans `[workspace.package]`, licence `MIT`. Pas de champ `description`, `authors` ni `repository` dans le workspace (D : certains outils les exigent, voir cargo-dist).
- Icônes : un seul fichier, `crates/riff-app/assets/icon-512.png` (PNG 512x512, RGBA, 48 345 octets). Aucun `.ico`, aucun `.icns`, aucun 1024 px. L'original utilise `icon-1024.png` pour macOS (spec).
- Aucun `build.rs` et aucune crate de ressources Windows (`winresource`, `embed-resource`) : D, le `.exe` n'a pas d'icône intégrée. Aucun des outils ci-dessous ne règle ce point, c'est un travail à part (build.rs et `.ico`).
- CI : `.github/workflows/ci.yml`, `permissions: contents: read`, actions fixées par SHA (`actions/checkout`, `Swatinem/rust-cache`, `taiki-e/install-action`, `actions/upload-artifact`), matrice `ubuntu-latest`, `windows-latest`, `macos-latest`. Pas de cible macOS x64 ni arm64 explicite : `macos-latest` seul (D : arm64 aujourd'hui, x64 à ajouter).

## Candidats et dates (crates.io et GitHub, relevés le 2026-11-22)
| Outil | Dernière version | Date | Licence | Dépôt : dernière activité |
|---|---|---|---|---|
| `cargo-packager` | 0.11.8 [C1] | 2025-11-27 | Apache-2.0 OR MIT [C1] | commits jusqu'au 2026-11-19, dépôt non archivé, 36 tickets ouverts (PR comprises) [G1] |
| `cargo-wix` | 0.3.9 [C2] | 2025-03-13 | MIT/Apache-2.0 [C2] | derniers commits : montées de version de dépendances, 2026-07-17 [G2] |
| `cargo-dist` (commande `dist`) | 0.34.0 [C3] | 2026-11-02 | MIT OR Apache-2.0 [C3] | commits jusqu'au 2026-11-21, 339 tickets ouverts (PR comprises) [G3] |
| `cargo-bundle` | 0.12.0 [C4] | 2026-09-20 | MIT OR Apache-2.0 [C4] | commits jusqu'au 2026-11-14, 78 tickets ouverts ; README : « Very early alpha » [G4, B1] |
| NSIS direct | 3.13 [N1] | 2026-09-27 | zlib/libpng (modules de compression : licences à part) [N2] | maintenu (sorties régulières) |
| Inno Setup direct | 7.1.1 et 6.7.4 [I1] | 2026-10-28 | « Inno Setup License », permissive, mentions à conserver [I2] | dépôt actif |
| WiX Toolset direct | 7.0.1 [X1] | 2026-10-26 | MS-RL [X1] ; **Open Source Maintenance Fee exigée si le projet génère des revenus** [X2] | dépôt actif |
| `linuxdeploy` | 1-alpha-20251107-1 (étiquette), `continuous` 2026-11-01 [L1] | 2025-11-07 | MIT [L1] | dernier commit 2026-08-01 |
| `appimagetool` | 1.9.1 [L2] | 2025-11-18 | MIT [L2] | `continuous` 2026-11-06 |
| `create-dmg` | 1.3.0 [M1] | 2026-07-02 | MIT [M1] | actif |

Écartés sans comparaison détaillée : `cargo-deb`, `cargo-generate-rpm` (paquets système, hors périmètre : la spec exclut les gestionnaires de paquets), `cargo-appimage` 2.4.0 (GPL-3.0 [C5], AppImage seul), AppImageKit (dernière version 2020 [L3]).

## Tableau de comparaison (versions exactes)
`cargo-packager` 0.11.8 [P*], `cargo-wix` 0.3.9 [W*], `cargo-dist` 0.34.0 [D*], `cargo-bundle` 0.12.0 [B*].

| Critère | cargo-packager 0.11.8 | cargo-wix 0.3.9 | cargo-dist 0.34.0 | cargo-bundle 0.12.0 |
|---|---|---|---|---|
| Windows : formats | NSIS (`.exe`) et MSI via WiX 3.11 [P1] | MSI seul, Windows seul [W1] | MSI (WiX v3), zip. Pas de NSIS [D1, D2] | MSI « expérimental » (`msi`, `wxsmsi`) [B1] |
| Linux : formats | AppImage, deb, pacman [P1] | aucun | tar.xz de l'exécutable seul [D3] | AppImage, deb, rpm [B1] |
| macOS : formats | dmg et `.app`. **Pas de zip** [P1] | aucun | archive de l'exécutable seul, pas de `.app` ni de dmg (« future installers », ticket 24) [D1] | `.app` ; dmg cité dans le README (option `dmg_background`) mais absent des valeurs de `--format` : NT pour un format dmg autonome [B1] |
| macOS x64 et arm64 | une exécution par `target_triple` (champ de config) [P2] ; D : CI avec un runner par architecture | aucun | cibles `x86_64-apple-darwin` et `aarch64-apple-darwin` en CI [D4] | plusieurs `--target` : binaire universel via `lipo` [B1] (écart : l'original livre deux paquets séparés) |
| Installation par utilisateur, sans administrateur | **Oui**, `install_mode` vaut `CurrentUser` par défaut : `RequestExecutionLevel user`, dossier `$LOCALAPPDATA\<produit>`, clés dans `HKCU` [P2, P3]. MSI : `InstallScope="perMachine"` en dur [P5] | Modèle par défaut `perMachine` [W2]. Per-user : D (modèle personnalisable), NT | MSI par défaut dans `%ProgramFiles%` [D2]. Per-user : NT | NT |
| Choix du dossier | Oui : page `MUI_PAGE_DIRECTORY`, sautée seulement en mode passif [P3]. MSI : `WixUI_InstallDir` [P5] | NT (modèle : `WixUI_FeatureTree` [W2] ; D : page de personnalisation, pas confirmé) | Oui : menu « choose where to install » du MSI [D2] | NT |
| Données conservées à la désinstallation | Oui par défaut : l'option `appdata_paths` (vide par défaut) ajoute une case, décochée par défaut, qui supprime ces chemins [P2, P3]. Sans `appdata_paths`, aucune suppression [P3] | D : un MSI ne retire que ses propres fichiers | D : idem MSI | NT |
| Raccourci Menu Démarrer | Oui : page `MUI_PAGE_STARTMENU` et `CreateShortcut "$SMPROGRAMS\…"` ; en silencieux, créé sauf option `/NS` [P3]. Raccourci bureau en option sur la page finale [P3] | Absent du modèle par défaut (aucune balise `Shortcut`) [W2] | NT | NT |
| Entrée de désinstallation | Oui : clé `Software\Microsoft\Windows\CurrentVersion\Uninstall\<produit>` avec `DisplayName`, `DisplayIcon`, `DisplayVersion`, `UninstallString`, dans `HKCU` en mode utilisateur [P3] | Oui (MSI : Ajout/Suppression de programmes, `ARPPRODUCTICON`) [W2] | Oui, dite explicitement dans la doc [D2] | NT |
| Icônes : formats attendus | NSIS : `installer_icon` (chemin, format non précisé : NT) [P2, P4]. MSI : un `.ico` cherché dans `icons` [P5]. macOS : un `.icns` copié tel quel, sinon construit depuis les PNG de tailles valides ICNS [P6]. Linux : PNG de `icons` [P7]. **`.ico` à créer** : le dépôt n'a que `icon-512.png` | `.ico` pour `ARPPRODUCTICON` (`product-icon`) [W2]. **`.ico` à créer** | NT | « SVG (Linux), ICNS, ICO, PNG, tout format lisible par la crate `image` », conversion automatique [B1] |
| Signature | Windows : `sign_command`, `certificate_thumbprint`, `tsp`, `digest_algorithm` [P2]. macOS : `signing_identity` (Developer ID) et notarisation par variables d'environnement [P2]. **Pas d'ad hoc** : la signature n'est lancée que si `signing_identity` est défini [P8]. D : `signing_identity = "-"` pourrait donner une signature ad hoc via `codesign`, non essayé | `cargo wix sign` via `signtool` du SDK Windows 10 [W1]. Rien pour macOS | Windows : SSL.com eSigner ou Azure Artifact Signing, **x86_64 seulement** [D5]. macOS : NT | Apple : p12 et bibliothèque `apple-codesign`, sans notarisation [B1]. Windows : Authenticode via `osslsigncode` embarqué, **GPL-3.0-or-later**, derrière la feature `windows-signing` [B1]. Ad hoc : NT |
| Sans certificat | Livre non signé (D : aucun champ requis) | idem (D) | idem (D) | `windows-signing` donnée comme erreur si configurée sans la feature [B1] ; sans configuration : D, non signé |
| GitHub Actions | Aucun workflow généré. **Aucun binaire précompilé** : la page de la version 0.11.8 n'a aucun fichier [G5] et `taiki-e/install-action` n'a pas de manifeste `cargo-packager` (404 sur `main`) [G6]. Donc `cargo install --locked cargo-packager` en CI (compilation, D : plusieurs minutes), ou action tierce à fixer par SHA. D : n'appelle pas l'API GitHub, donc compatible avec `contents: write` limité au travail de publication | Aucun workflow. Aucun manifeste `taiki-e` [G6]. WiX 3 préinstallé sur les runners GitHub (dit par la doc de cargo-dist [D2]) | **Génère** `.github/workflows/release.yml` (plan, build, host, publish, announce) [D6]. Épinglage par SHA possible : `github-action-commits` [D7]. **Écart avec la spec** : la release finale est « non brouillon » à l'étape announce [D6] ; avec `create-release = false`, il suppose un brouillon existant et **le publie à la fin** [D8]. Binaires précompilés disponibles [G7]. Permissions du workflow généré : non relues (NT) | Aucun workflow. Aucun manifeste `taiki-e` [G6] |
| Téléchargements pendant l'empaquetage | NSIS 3.09 et plugins depuis `github.com/tauri-apps/binary-releases`, vérifiés en **SHA-1** [P4]. AppImage : `AppRun` et `linuxdeploy` depuis le même dépôt, et `linuxdeploy-plugin-appimage` depuis l'étiquette mobile `continuous` ; aucune vérification de hash lue (D) [P7]. dmg : script `create-dmg` à un commit fixe [P9]. WiX 3.11.2, vérifié en SHA-256 [P5]. Tous sur `github.com` / `raw.githubusercontent.com` | WiX : installation préalable à la main [W1] | Binaire de `dist` récupéré par le workflow généré (détail NT) | Runtime AppImage « continuous » téléchargé et mis en cache [B1] |
| Compatibilité avec minisign côté application | N'impose rien à l'application : l'updater `cargo-packager-updater` 0.2.3 est une crate séparée, qu'il suffit de ne pas utiliser [P10]. Signature d'artefacts minisign facultative (`--private-key`) [P11] : on peut l'ignorer et signer à part avec `minisign`. Options d'installation silencieuse : `/S` (NSIS), `/P`, `/R`, `/NS` [P3] | Aucun updater (NT) | `install-updater` (axoupdater) : uniquement pour les installeurs shell et PowerShell, expérimental, hors cible [D9]. Les installeurs shell/PowerShell récupèrent depuis GitHub (D) | Aucun updater (NT) |
| Télémétrie | Aucune crate de télémétrie dans `crates/packager/Cargo.toml` ; réseau : `ureq` pour les téléchargements d'outils [P10]. Lecture partielle du code : D | NT | NT dans la config et la CLI | NT |
| Nom d'artefact | `<nom>_<version>_<arch>-setup.exe`, `<nom>_<version>_<arch>.AppImage` [P4, P7] : écart avec `${name}-${version}-${os}-${arch}.${ext}` ; renommage à faire dans le workflow | `target/wix/` (NT sur le nom) | `<nom>-<cible>.zip` ou `.tar.xz` (D) | NT |
| Version préliminaire (`1.0.0-rc.1`) | NSIS : D, la version complète est reprise telle quelle et un `.0` est ajouté pour `VIProductVersion` (fonction `add_build_number_if_needed`) [P4]. MSI : `convert_version` exige du numérique [P5]. À essayer | WiX : numérique requis (D) | Gère les préversions (`publish-prereleases`) [D10] | NT |
| Dépendances système requises | NSIS téléchargé : aucune sur Windows. Linux : `linuxdeploy` s'exécute en FUSE ou extrait (D) | WiX 3 + SDK pour signer [W1] | WiX 3 pour MSI [D2] | Bibliothèques partagées **non** collectées dans l'AppImage [B1] : Riff lie `alsa`, `dbus`, `wayland`, `xkbcommon` (CI.yml, paquets installés) : AppImage fragile |

Outils directs (une ligne par outil, mêmes critères quand la doc lue les couvre) :

| Outil | Ce qu'il apporte | Vérifié | NT ou déduit |
|---|---|---|---|
| NSIS 3.13 | Même moteur que `electron-builder` pour l'original. Contrôle total du script `.nsi` (per-user, dossier, données, raccourcis). Le modèle `installer.nsi` de cargo-packager [P3] peut servir de base (Apache-2.0 OR MIT) | Version, date, licence [N1, N2] | Aucune doc NSIS lue au-delà de la page de licence et de téléchargement. Tout le script serait à écrire et à maintenir. Signature : D, `signtool` en étape séparée |
| Inno Setup 7.1.1 | `PrivilegesRequired=lowest` : jamais d'élévation, valeurs `admin` ou `lowest`, défaut `admin` [I3]. `DisableDirPage` : valeurs `auto`, `yes`, `no` [I4]. `SetupIconFile` pour l'icône [I5] | Directives citées | Écart d'apparence avec l'original (NSIS). Windows seul. Conservation des données et raccourcis : D, NT lu |
| WiX 7.0.1 | MSI complet | Licence MS-RL et redevance de maintenance si revenus [X1, X2] | Per-user, dossier, raccourcis : non lus. Redevance à écarter par principe de dépendance. MSI plus lourd que NSIS pour ce besoin |
| `linuxdeploy` + `appimagetool` | Collecte des bibliothèques (README de cargo-bundle : l'outil « wrap linuxdeploy / appimagetool » collecte [B1]) ; ce sont les briques qu'appelle cargo-packager pour l'AppImage [P7] | Versions, licences MIT [L1, L2] | `linuxdeploy` : dernière étiquette en novembre 2025, le reste en `continuous` (mobile). Doc de ces outils non lue |
| macOS à la main | `ditto -c -k --keepParent` pour le zip, `codesign --sign -` pour l'ad hoc, `create-dmg` 1.3.0 ou `hdiutil` pour le dmg | `create-dmg` : MIT, 2026-07-02 [M1] | Commandes `ditto` et `codesign` : D (connaissance générale, pages Apple non lues). Exigent un runner macOS, donc CI |

## Écarts avec l'original, par outil
| Point de l'original | cargo-packager | cargo-wix | cargo-dist | cargo-bundle |
|---|---|---|---|---|
| NSIS x64, par utilisateur, dossier au choix, données gardées | Conforme (V : modèle NSIS) | Écart : MSI, `perMachine` | Écart : MSI, Program Files | Écart : MSI expérimental |
| AppImage x64, catégorie `AudioVideo` | Format présent ; catégorie : `category` du champ de config [P2], valeur `AudioVideo` : D | Aucun | Aucun | Format présent, sans collecte des bibliothèques |
| macOS dmg x64 et arm64 | dmg présent, 2 exécutions (D) | Aucun | Aucun dmg | dmg : D, ambigu |
| macOS zip x64 et arm64 | **Absent** : `ditto` manuel | Aucun | zip d'un exécutable nu, pas un `.app` | Absent |
| Signature ad hoc macOS | Étape manuelle (`codesign --sign -`), ou `signing_identity` à essayer | Aucun | Aucun | NT |
| `${name}-${version}-${os}-${arch}.${ext}` | Renommage dans le workflow | idem | Nommage propre à l'outil | idem |
| Mise à jour (electron-updater) | Remplacée par la mise à jour à la demande de l'application, indépendante de l'outil | idem | idem, `axoupdater` non utilisé | idem |
| Release brouillon seulement | Conforme (l'outil ne publie rien) | Conforme | **Non conforme par défaut** (publie à la fin) | Conforme |

## Recommandation
**`cargo-packager` 0.11.8** : c'est le seul outil lu qui produit à la fois le NSIS par utilisateur (dossier au choix, raccourci du Menu Démarrer, entrée de désinstallation, données conservées, tous vérifiés dans son modèle `installer.nsi`), l'AppImage et le dmg, sans générer de workflow ni publier quoi que ce soit : le workflow de release reste écrit à la main avec actions fixées par SHA et `contents: write` limité au travail de publication. Il n'impose ni updater ni télémétrie (D, voir lignes correspondantes). `cargo-dist` est écarté : pas de NSIS, d'AppImage ni de dmg, et sa publication finale de la release contredit la règle du brouillon. `cargo-wix` et `cargo-bundle` sont écartés : MSI seul, ou outil « early alpha » sans collecte de bibliothèques dans l'AppImage.

Risques de `cargo-packager`, à ne pas masquer :
1. Dernière version publiée il y a 12 mois (2025-11-27), alors que le dépôt reçoit encore des commits (2026-11-19). Les commits récents ne sont pas dans 0.11.8.
2. Téléchargements non maîtrisés pendant l'empaquetage : NSIS 3.09 (la version 3.13 existe depuis 2026-09-27), plugins vérifiés en SHA-1 seulement, `linuxdeploy-plugin-appimage` en `continuous` (mobile) pour l'AppImage. Risque de chaîne d'approvisionnement à documenter dans `docs/RELEASING.md`, à mesurer par un build en CI avant la première release.
3. Pas de binaire précompilé : `cargo install --locked` dans la CI (temps, et nouvelle dépendance de build à signaler). Alternative : fixer un téléchargement par SHA-256.
4. Pas de zip macOS ni de signature ad hoc : deux commandes manuelles à écrire dans le workflow, non essayées ici.
5. `.ico` et `.icns` à produire à partir de `icon-512.png` (512 px : D, pas de variante 1024 px pour le `.icns` Retina). L'icône du `.exe` lui-même exige un `build.rs`, hors outil.
6. Linux et macOS non buildables sur la machine de développement : validation par la CI uniquement. L'AppImage dépend du comportement de `linuxdeploy` sur `libasound`, `libdbus`, `libwayland`, `libxkbcommon` : D, non essayé.
7. Nom du produit : le modèle NSIS dérive la clé de désinstallation et le dossier par défaut de `PRODUCTNAME` (V) : le nom final doit différer de « Riff » pour la coexistence (lien avec la décision 5 de la spec).
8. Versions préliminaires (`1.0.0-rc.1`) : comportement de NSIS et WiX à essayer, D.

Repli si `cargo-packager` est refusé : NSIS 3.13 direct (script `.nsi` inspiré de [P3]), `linuxdeploy` et `appimagetool` pour l'AppImage, `ditto`, `codesign` et `create-dmg` sur macOS. Plus de contrôle, plus de maintenance, mêmes contraintes de CI.

## Sources
Relevées le 2026-11-22. Les fichiers sont lus à l'étiquette de la version exacte.

Crates.io (API publique `crates.io/api/v1/crates/<nom>`) :
- [C1] https://crates.io/crates/cargo-packager/0.11.8
- [C2] https://crates.io/crates/cargo-wix/0.3.9
- [C3] https://crates.io/crates/cargo-dist/0.34.0
- [C4] https://crates.io/crates/cargo-bundle/0.12.0
- [C5] https://crates.io/crates/cargo-appimage/2.4.0

cargo-packager, étiquette `cargo-packager-v0.11.8` (base : https://github.com/crabnebula-dev/cargo-packager/blob/cargo-packager-v0.11.8) :
- [P1] `/crates/utils/src/lib.rs` (enum `PackageFormat`) et `/README.md`
- [P2] `/crates/packager/src/config/mod.rs` (`NsisConfig`, `NSISInstallerMode`, `MacOsConfig`, `WindowsConfig`, `icons`, `target_triple`, `category`)
- [P3] `/crates/packager/src/package/nsis/installer.nsi`
- [P4] `/crates/packager/src/package/nsis/mod.rs`
- [P5] `/crates/packager/src/package/wix/main.wxs` et `/mod.rs`
- [P6] `/crates/packager/src/util.rs` (`create_icns_file`) et `/package/app/mod.rs`
- [P7] `/crates/packager/src/package/appimage/mod.rs`
- [P8] `/crates/packager/src/package/app/mod.rs` (signature conditionnée à `signing_identity`)
- [P9] `/crates/packager/src/package/dmg/mod.rs`
- [P10] `/crates/packager/Cargo.toml`, `/crates/updater/Cargo.toml`
- [P11] `/crates/packager/src/cli/mod.rs`
- [G1] https://github.com/crabnebula-dev/cargo-packager (API GitHub : dernier commit, tickets)
- [G5] https://github.com/crabnebula-dev/cargo-packager/releases/tag/cargo-packager-v0.11.8 (liste des fichiers : vide)

cargo-wix, étiquette `0.3.9` (base : https://github.com/volks73/cargo-wix/blob/0.3.9) :
- [W1] `/README.md`
- [W2] `/src/templates/main.wxs.mustache`
- [G2] https://github.com/volks73/cargo-wix (API GitHub). La page https://volks73.github.io/cargo-wix/ n'a pas pu être lue (« Not Found »).

cargo-dist, étiquette `v0.34.0` (base : https://github.com/axodotdev/cargo-dist/blob/v0.34.0/book/src) :
- [D1] `/installers/index.md`
- [D2] `/installers/msi.md`
- [D3] `/artifacts/archives.md`
- [D4] `/reference/config.md` (`targets`)
- [D5] `/supplychain-security/signing/windows.md`
- [D6] `/ci/index.md`
- [D7] `/reference/config.md` (`github-action-commits`)
- [D8] `/ci/customizing.md` (`create-release = false`)
- [D9] `/installers/updater.md`
- [D10] `/reference/config.md` (`publish-prereleases`)
- [G3] https://github.com/axodotdev/cargo-dist ; [G7] https://github.com/axodotdev/cargo-dist/releases/tag/v0.34.0

cargo-bundle, étiquette `v0.12.0` :
- [B1] https://github.com/burtonageo/cargo-bundle/blob/v0.12.0/Readme.md (le README de `master` renvoie 404 : lu à l'étiquette)
- [G4] https://github.com/burtonageo/cargo-bundle

Outils directs :
- [N1] https://nsis.sourceforge.io/Download ; [N2] https://nsis.sourceforge.io/License
- [I1] https://github.com/jrsoftware/issrc/releases ; [I2] https://github.com/jrsoftware/issrc/blob/is-7_1_1/license.txt ; [I3] https://jrsoftware.org/ishelp/topic_setup_privilegesrequired.htm ; [I4] https://jrsoftware.org/ishelp/topic_setup_disabledirpage.htm ; [I5] https://jrsoftware.org/ishelp/topic_setup_setupiconfile.htm
- [X1] https://github.com/wixtoolset/wix/blob/v7.0.1/LICENSE.TXT ; [X2] https://github.com/wixtoolset/wix/blob/v7.0.1/README.md (section « Open Source Maintenance Fee »)
- [L1] https://github.com/linuxdeploy/linuxdeploy/releases ; [L2] https://github.com/AppImage/appimagetool/releases ; [L3] https://github.com/AppImage/AppImageKit/releases
- [M1] https://github.com/create-dmg/create-dmg/releases
- [G6] https://github.com/taiki-e/install-action/tree/main/manifests (aucun manifeste pour `cargo-packager`, `cargo-wix`, `cargo-dist`, `cargo-bundle`)

Pages non lues : documentation docs.rs (le code source de l'étiquette a été lu à la place), documentation HTML de cargo-wix, pages shell et PowerShell de cargo-dist, workflow `release.yml` généré par cargo-dist (jamais produit), documentation de NSIS hors licence et téléchargement, documentation de WiX 7, de `linuxdeploy`, d'`appimagetool` et de `create-dmg`, pages Apple sur `codesign` et `ditto`.
