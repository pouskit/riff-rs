# Décision 001 : framework UI (iced 0.14)

**Décision : GO CONDITIONNEL pour iced 0.14.0.**

Condition : refaire le test de composition IME sur une machine où un IME de composition fonctionne, avant la fin du socle UI. Si la composition échoue dans iced alors qu'elle marche dans une zone de texte Windows, le critère i passe en échec, ce qui vaut NO-GO selon les critères du test.

Écart avec les critères du test : leur règle réserve GO CONDITIONNEL à un échec parmi b, c, e, g, et ne prévoit pas un critère impossible à vérifier. Aucun test n'a échoué, mais une partie de i n'a pas pu être mesurée : GO n'est donc pas démontré, et aucun échec ne justifie NO-GO. Aucun seuil n'a été modifié.

## Machine
Ryzen 9 5900X (12 cœurs, 24 threads), Radeon RX 7800 XT (pilote 32.0.32015.7029), 32 Go. Écran principal 2560x1440 à 240 Hz, secondaire 1920x1080. Échelle Windows 100 % (96 ppp, facteur 1 relevé par iced). Fenêtre de test 1920x1080 sur l'écran principal, toujours au premier plan pour b, c, e. Build `--release` sans LTO.

Conditions réelles : Discord, Firefox, YouTube Music, VS Code ouverts, et jusqu'à 3 processus `cargo`/`rustc` d'autres sessions pendant b, c, e (charge CPU relevée de 14 à 88 %). Backend wgpu exact non déterminé : les bibliothèques Vulkan et D3D12 sont toutes deux chargées.

## Résultats
| # | Test | Valeur mesurée | Seuil GO | Verdict |
|---|---|---|---|---|
| a | RTL | Arabe lié, ordre et surlignage couleur identiques à Edge 156, sur 3 lignes et un paragraphe à la ligne | Comparaison Chromium | GO, 2 contournements |
| b | Viz 64 barres + forme, 60 s | p50 4,18, p95 4,58, p99 4,81, max 9,14 ms ; 0 frame manquée sur 14 395 | p95 <= 16,6 ms, 0 manquée | GO |
| c | Flou | Direct : p50 4,17, p95 4,55, max 18,26 ms. Précalculé : p95 4,51, max 10,27 ms, calcul médian 3,50 ms, max 3,69 ms | p95 <= 16,6 ms ou calcul <= 100 ms | GO |
| d | Couleurs M3 | 3 pochettes, 78 paires `on-X/X`, 0 échec, ratio minimum 5,47 | >= 4,5:1 | GO |
| e | Liste 50 000, 2 400 px/s | p50 4,17, p95 4,80, max 6,60 ms, 0 manquée sur 14 400. RAM 148,7 Mo contre 128,1 à vide, soit +20,6 Mo | p95 <= 16,6 ms, <= 50 Mo | GO |
| f | RAM repos (liste 50 000 + flou direct + viz arrêtée), 5 lancements | Min 145,9, **médiane 148,8**, max 150,0 Mo, dont `conhost` 1,6 Mo | <= 158 Mo | GO |
| g | Démarrage, 5 lancements | 326, 329, **343**, 363, 382 ms (cache disque chaud, démarrage à froid non mesuré) | Médiane <= 500 ms | GO |
| h | Fenêtre sans bordure | 23 étapes sur 23 : déplacer, 8 poignées, taille minimale, agrandir, double-clic, Win+flèches, glisser vers les bords, réduire, fermer | Tout fonctionne ou limite documentée | GO, 1 limite |
| i | Saisie | Accents et touches mortes, collage, copie, coupe, panneau emoji corrects. Composition IME non vérifiable | Saisie correcte | PARTIEL |
| j | Accessibilité | Aucune occurrence d'`accesskit` dans `Cargo.lock` : pas d'arbre d'accessibilité (déduit des dépendances, lecteur d'écran non essayé) | Information | Non |

Définitions, écrites avant les mesures : frame manquée = intervalle > 16,67 ms entre deux `RedrawRequested` (vsync à 240 Hz, donc plus exigeant qu'un écran 60 Hz) ; 2 s de préchauffage exclues ; démarrage = deuxième tick, donc borne haute du premier contenu présenté. Le seul dépassement est sur c en direct (18,26 ms), ce test n'exige pas 0 frame manquée.

Validation du banc : `scripts/measure.ps1` sur `notepad.exe` : 54,6 / 54,7 / 54,8 Mo, 1 processus (le lanceur de Windows 11 se termine, l'enfant orphelin est bien suivi). Lancé en arrière-plan par l'outil, Notepad disparaissait, cause non identifiée : mesures faites au premier plan.

Captures : `prototypes/iced-spike/out/rtl-*.png` (a), `blur-*.png` (c), `palette.png` (d), plus `viz.png`, `list.png`, `idle.png`. Résultats bruts dans le même dossier (`*-frames.csv`, `ram-*.csv`, `palette.csv`, `chrome-results.csv`, `input-results.csv`). Les captures de h et i ont été supprimées : prises à l'écran, elles montraient des bouts du bureau sur les bords.

## Test i, composition IME
Fr-FR et en-GB seuls installés, Microsoft Pinyin ajouté temporairement (liste de langues restaurée et vérifiée à chaque fois). Essais scriptés et un essai manuel par l'utilisateur, icône 拼 sélectionnée : **la zone de texte Windows témoin reçoit `nihao` en latin, comme iced**, sans candidats. Le mode de conversion reste à 0 même forcé par `WM_IME_CONTROL`. La composition ne fonctionne donc sur aucune fenêtre de cette installation : le test ne dit rien sur iced.

Indices, non équivalents à un test (lecture du code) : `winit` traite `WM_IME_STARTCOMPOSITION` et `WM_IME_COMPOSITION`, `iced_winit` active l'IME et affiche le texte en cours de composition, `text_input` déclare son curseur à l'IME. Le premier essai scripté n'avait rien écrit dans le champ, sans témoin ni journal : ce résultat reste inexpliqué.

Autre écart : un emoji envoyé en deux demi-caractères par `SendInput`/`VK_PACKET` est perdu (15 unités reçues sur 17), non essayé sur le témoin, cause non établie. Le reste du même texte (accents, japonais, arabe) arrive correctement.

## Limites et bugs relevés dans iced 0.14.0
- `Span::background` : rectangles mal placés en RTL (`span_bounds` de `iced_graphics` suppose des x croissants). Contournement vérifié : mots en tuiles dans un `Row` inversé. Le surlignage de Riff par la couleur (`.active-word`) est correct.
- `text::Alignment::Right` sur une seule ligne RTL : texte hors de la fenêtre (`text::align` perd le relayout). Contournement vérifié : alignement par défaut et `container.align_right`.
- Pas de menu Snap Layouts au survol du bouton agrandir : `winit` ne traite pas `HTMAXBUTTON` (lecture du code, non essayé). Win+flèches et le glisser vers les bords fonctionnent. Contournement possible par sous-classement de la fenêtre, non essayé.
- Mode de paroles « gradient » de Riff (`background-clip: text`) : non testé.

## Risques
- Maintenance : dépôt actif (dernier commit 2026-11-01, 516 issues ouvertes, 31 887 étoiles), mais `iced` 0.14.0 date du 2025-12-07 (`iced_widget` 0.14.2 du 2025-12-11). iced 0.14 utilise wgpu 27, wgpu 30 est disponible.
- RAM : 9 Mo de marge seulement sous la cible, sans `riff-core`, ni audio, ni vraie bibliothèque. Une fenêtre vide coûte déjà 128 Mo (système de polices, wgpu, pilote) : à réduire en priorité dans le socle UI.
- Accessibilité absente : à traiter si la parité avec Riff l'exige.
- Mesures sur une machine puissante à 240 Hz : aucune mesure sur une machine modeste.

## Chiffre de RAM retenu
Cible inchangée : **158 Mo** (50 % des 317 Mo d'Electron). Mesure de référence du prototype : 148,8 Mo. La cible n'est donc pas modifiée.

## Incident pendant le test h
Une première version de `drive-chrome.ps1` a pris une fenêtre cachée de 16x16 pour la fenêtre de test et a envoyé de vrais clics, glissés et touches sur les fenêtres de l'utilisateur, sans l'avoir prévenu. Une fenêtre Firefox a probablement été fermée. Scripts durcis : fenêtre identifiée par PID, titre et taille, clic refusé si la fenêtre sous le curseur n'est pas la bonne, touche refusée sans focus. Après durcissement, ils se sont arrêtés d'eux-mêmes deux fois sans rien envoyer ailleurs.
