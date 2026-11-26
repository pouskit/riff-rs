# Publier une version signée

Les mises à jour de l'application (bouton « Download and install » de « À propos », Windows seulement) refusent tout installeur dont la signature minisign ou le SHA-256 ne correspond pas. Ce document décrit ce que le mainteneur fait sur sa machine. Les workflows de CI ne signent rien et ne contiennent aucune clé.

## Ce que l'application vérifie

Pour la version `X.Y.Z` (étiquette `vX.Y.Z`), l'application télécharge, depuis `https://github.com/<REPOSITORY>/releases/download/vX.Y.Z/` (`REPOSITORY` : `crates/riff-app/src/system/updates.rs`) :

| Fichier | Rôle |
|---|---|
| `riff-app_X.Y.Z_x64-setup.exe` | installeur NSIS produit par `cargo packager --release --formats nsis` |
| `riff-app_X.Y.Z_x64-setup.exe.minisig` | signature minisign de l'installeur |
| `SHA256SUMS` | sommes au format `sha256sum` : `<64 hexadécimaux>  <nom du fichier>` |

Refus (fichier supprimé, rien d'exécuté) si : clé publique embarquée absente, `.minisig` absent ou illisible, signature d'une autre clé, installeur modifié, commentaire de confiance sans `file:<nom de l'installeur>`, entrée absente de `SHA256SUMS`, SHA-256 différent.

Le commentaire de confiance est signé : il lie la signature au nom (donc à la version) du fichier, ce qui empêche de faire passer un ancien installeur signé pour un nouveau. Ne pas utiliser `-t` à la signature, ou y mettre `file:<nom exact de l'installeur>`.

## 1. Installer minisign

Version vérifiée : 0.12 (publiée le 2025-01-15), binaires Windows, Linux et macOS sur https://github.com/jedisct1/minisign/releases/tag/0.12 (page du projet : https://jedisct1.github.io/minisign/). Vérifier la signature du binaire téléchargé avec la clé publique indiquée sur la page du projet.

## 2. Générer la paire de clés (une seule fois, sur la machine du mainteneur)

```
minisign -G
```

Commande de la documentation du projet : la clé publique est affichée et écrite dans `minisign.pub` ; la clé secrète est chiffrée par un mot de passe et écrite dans `~/.minisign/minisign.key`. Choisir un mot de passe fort (n'utiliser `-W` que pour une clé de test).

## 3. Garder la clé privée

- `~/.minisign/minisign.key` reste sur la machine du mainteneur, avec une sauvegarde chiffrée hors ligne (support amovible). Perdre la clé impose une rotation (section 7).
- Jamais dans le dépôt, jamais dans un secret de GitHub Actions, jamais dans un message ou une capture. `.gitignore` ne protège pas d'un ajout forcé : vérifier `git status` avant chaque commit.

## 4. Embarquer la clé publique

Ouvrir `minisign.pub` : la seconde ligne (base64, commence par `RW`) est la clé. La copier dans la constante unique `UPDATE_PUBLIC_KEY` de `crates/riff-app/src/system/update_install.rs`.

Clé de production embarquée depuis la 1.0.0 (identifiant `4050AF6BFAC3CAB6`), contrôlée par `the_production_key_decodes_and_refuses_test_signatures`. Une constante vide fait refuser toute mise à jour. La clé doit être embarquée dans une version publiée avant de signer les versions suivantes : une version déjà installée ne reconnaît que la clé qu'elle contient.

## 5. Signer l'installeur du brouillon

Le workflow `.github/workflows/release.yml` (étiquette `vX.Y.Z` poussée, égale à `v` + `[workspace.package].version`) construit les paquets en CI et crée un brouillon de release avec les installeurs et `SHA256SUMS`. Il ne signe rien. Depuis un dossier vide, télécharger l'installeur Windows et les sommes du brouillon :

```
gh release download vX.Y.Z --pattern "riff-app_X.Y.Z_x64-setup.exe" --pattern SHA256SUMS
```

Contrôler le SHA-256 de l'installeur contre `SHA256SUMS` (Git Bash, Linux ou macOS ; PowerShell : `Get-FileHash -Algorithm SHA256`, à comparer à la ligne de l'installeur) :

```
sha256sum --ignore-missing -c SHA256SUMS
```

Signer :

```
minisign -Sm riff-app_X.Y.Z_x64-setup.exe
```

Commande de la documentation du projet : la signature est écrite dans `riff-app_X.Y.Z_x64-setup.exe.minisig` (par défaut `<fichier>.minisig`, option `-x` pour changer). Le commentaire de confiance par défaut est `timestamp:<epoch>` puis `file:<nom du fichier>` (source : `src/minisign.c` de la version 0.12).

Contrôle avant publication :

```
minisign -Vm riff-app_X.Y.Z_x64-setup.exe -P <clé publique en base64>
```

`SHA256SUMS` liste tous les fichiers livrés ; seule la ligne de l'installeur Windows est lue par l'application.

## 6. Joindre à la release (brouillon)

Ajouter la signature au brouillon (l'installeur et `SHA256SUMS` y sont déjà) :

```
gh release upload vX.Y.Z riff-app_X.Y.Z_x64-setup.exe.minisig
```

L'étiquette doit être `vX.Y.Z` (avec `v`). Relire les noms des trois fichiers, puis seulement publier le brouillon : le workflow ne publie jamais.

Linux (AppImage) et macOS (dmg) : pas d'installation automatique, le bouton « Open release page » ouvre la page de la release.

## 7. Rotation de clé

1. Générer une nouvelle paire (`minisign -G -p nouvelle.pub -s nouvelle.key`, options `-p` et `-s` de la documentation).
2. Version de transition : embarquer la nouvelle clé publique, la signer avec l'ANCIENNE clé (les installations existantes ne connaissent que l'ancienne), publier.
3. Versions suivantes : signer avec la nouvelle clé seulement.
4. Clé compromise : le mécanisme actuel n'accepte qu'une seule clé embarquée. Les installations existantes ne peuvent alors pas se mettre à jour automatiquement vers une version signée par une nouvelle clé : publier un avis sur la page de la release et faire télécharger l'installeur à la main (README : vérification du SHA-256). Détruire l'ancienne clé secrète.

## Ce qui n'a pas été essayé

L'installation réelle par mise à jour (installeur lancé avec `/S /P /NS /R`, fermeture puis relance) n'a jamais été exécutée : aucune clé de production, aucun dépôt public, aucune release. À valider par une mise à jour d'une version factice vers une autre.
