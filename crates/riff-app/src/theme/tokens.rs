//! Jetons visuels de Riff 44.1.1, mesurés sur l'application d'origine (`ui-reference/tokens.json`).
//!
//! Les valeurs de Riff sont en pixels CSS. Riff applique un zoom interne de 0,945 : la cible est la
//! taille à l'écran, donc chaque constante ci-dessous est déjà multipliée par [`ZOOM`].

use iced::Color;

/// Zoom interne de Riff (`devsize_ui_scale` 105 % x 0,90) : 1 px CSS = 0,945 px écran.
pub const ZOOM: f32 = 0.945;

/// Convertit des pixels CSS de Riff en pixels écran.
#[must_use]
pub const fn css(px: f32) -> f32 {
  px * ZOOM
}

/// Taille des icônes (SVG de 18 px CSS dans Riff), en pixels écran.
pub const ICON_SIZE: f32 = css(18.0);
/// Icône du bouton lecture (22 px CSS).
pub const PLAY_ICON_SIZE: f32 = css(22.0);

/// Échelle typographique, en pixels écran.
pub mod text {
  use super::css;

  /// Texte courant : 16 px x `--font-scale` 1,1875 = 19 px CSS.
  pub const BODY: f32 = css(19.0);
  /// Titre de carte.
  pub const CARD_TITLE: f32 = css(14.5);
  /// Sous-titre de carte.
  pub const SUBTITLE: f32 = css(12.5);
  /// Titre de section (graisse 700).
  pub const SECTION_TITLE: f32 = css(22.0);
  /// Titre de la barre de lecture (graisse 500).
  pub const PLAYBAR_TITLE: f32 = css(16.6);
}

/// Rayons de coin, en pixels écran (`--md-shape-corner-*`).
pub mod radius {
  use super::css;

  pub const XS: f32 = css(4.0);
  pub const S: f32 = css(8.0);
  pub const M: f32 = css(12.0);
  pub const L: f32 = css(16.0);
  pub const XL: f32 = css(28.0);
  /// `--md-shape-corner-full` : pastille.
  pub const FULL: f32 = 9999.0;
  /// Boutons.
  pub const BUTTON: f32 = css(14.0);
}

/// Haut de fenêtre et panneau central, mesurés dans `ui-reference/layout.json` et `styles.css`.
pub mod shell {
  use super::css;

  /// `#titlebar` : hauteur 40 px CSS (`layout.json`, `titlebar.css.h`).
  pub const TITLEBAR_HEIGHT: f32 = css(40.0);
  /// `.titlebar` : `padding: 0 16px`.
  pub const TITLEBAR_PADDING: f32 = css(16.0);
  /// `.titlebar-controls` : `gap: 8px`.
  pub const WINDOW_BUTTON_GAP: f32 = css(8.0);
  /// `.win-btn` : `min-width: 42px` (`layout.json`, `window-btn-*.css.w`).
  pub const WINDOW_BUTTON_WIDTH: f32 = css(42.0);
  /// `.titlebar-btn` : `height: 28px`.
  pub const WINDOW_BUTTON_HEIGHT: f32 = css(28.0);
  /// Survol : `border-radius: 40%` de la hauteur du bouton (`styles.css:7522`).
  pub const WINDOW_BUTTON_HOVER_RADIUS: f32 = css(28.0 * 0.4);
  /// `.titlebar-riff-icon` : 20 px.
  pub const LOGO_SIZE: f32 = css(20.0);
  /// `.top-nav` : `height: 64px`.
  pub const TOP_NAV_HEIGHT: f32 = css(64.0);
  /// `.top-nav` : `padding: 0 24px`.
  pub const TOP_NAV_PADDING: f32 = css(24.0);
  /// `.search-box-wrapper` : `max-width: 560px`.
  pub const SEARCH_MAX_WIDTH: f32 = css(560.0);
  /// `.search-box-wrapper` : plancher de `max(200px, ...)`.
  pub const SEARCH_MIN_WIDTH: f32 = css(200.0);
  /// `.search-box` : `height: 44px`.
  pub const SEARCH_HEIGHT: f32 = css(44.0);
  /// `.search-box` : `padding: 0 16px`.
  pub const SEARCH_PADDING: f32 = css(16.0);
  /// `.search-icon` : `margin-right: 12px`.
  pub const SEARCH_ICON_GAP: f32 = css(12.0);
  /// `.search-box input` : `calc(16px * var(--font-scale))`, soit 19 px CSS.
  pub const SEARCH_TEXT: f32 = css(19.0);
  /// `--engine-w` mesuré : `.platform-switch-container` à 2 boutons, 306,9 px CSS.
  pub const ENGINE_WIDTH: f32 = css(307.0);
  /// `--engine-w` en mode compact (`app.js:14527`).
  pub const ENGINE_WIDTH_COMPACT: f32 = css(124.0);
  /// `--engine-gap`.
  pub const ENGINE_GAP: f32 = css(16.0);
  /// `.platform-pill-switch` : `padding: 4px; gap: 4px`.
  pub const PLATFORM_PILL_PADDING: f32 = css(4.0);
  /// `.platform-btn` : hauteur mesurée 29,96 px CSS (`layout.json`, `platform-ytm.css.h`).
  pub const PLATFORM_BUTTON_HEIGHT: f32 = css(30.0);
  /// `.platform-btn` : `font-size: calc(12px * var(--font-scale))`.
  pub const PLATFORM_TEXT: f32 = css(14.25);
  /// Rayon du panneau central : `radial-gradient(circle 28px ...)` des coins de la barre latérale
  /// et de la barre de lecture (`styles.css:10656-10687`).
  pub const PANEL_RADIUS: f32 = css(28.0);
  /// `.app-layout` : `padding: 0 10px 0 0`.
  pub const PANEL_PADDING_RIGHT: f32 = css(10.0);
  /// `.app-layout` : `gap: 8px` entre le panneau central et le panneau droit.
  pub const PANEL_GAP: f32 = css(8.0);
  /// `#main-content` : fondu du masque en haut (`styles.css:9292`).
  pub const FADE_TOP: f32 = css(24.0);
  /// `#main-content` : fondu du masque en bas.
  pub const FADE_BOTTOM: f32 = css(28.0);
}

/// Barre de lecture `#playbar`, mesurée dans `ui-reference/layout.json` (clés `playbar`,
/// `#btn-play-pause`, `#scrubber-track`, `#volume-slider`) et `styles.css:2808-3320`, `9599-9625`,
/// `10769-10782`.
pub mod playbar {
  use super::css;

  /// `--font-scale` : les tailles de texte de la barre sont `calc(Npx * var(--font-scale))`.
  pub const FONT_SCALE: f32 = 1.1875;
  /// `body:not(.classic-sidebar) .playbar` : `height: 96px` (90,72 px écran dans `layout.json`).
  pub const HEIGHT: f32 = css(96.0);
  /// Même règle : `padding: 6px 18px 10px`. Le bas est absorbé par le dépassement de la ligne.
  pub const PADDING_TOP: f32 = css(6.0);
  pub const PADDING_X: f32 = css(18.0);
  /// `.playbar` : `gap: 16px` entre les trois colonnes de la grille.
  pub const COLUMN_GAP: f32 = css(16.0);
  /// `.playbar-center` : `width: clamp(340px, 40vw, 580px)`. `40vw` vaut 40 % de la fenêtre, en pixels écran.
  pub const CENTER_MIN: f32 = css(340.0);
  pub const CENTER_MAX: f32 = css(580.0);
  pub const CENTER_VIEWPORT_SHARE: f32 = 0.40;
  /// `.playbar-center` : `gap: 4px; padding: 2px 0 4px`.
  pub const CENTER_GAP: f32 = css(4.0);
  pub const CENTER_PADDING_TOP: f32 = css(2.0);
  pub const CENTER_PADDING_BOTTOM: f32 = css(4.0);
  /// Hauteur de la ligne de la grille : 2 + 48 + 4 + 28 + 4 (la colonne centrale est la plus haute).
  pub const ROW_HEIGHT: f32 = css(86.0);

  /// `.playbar-left` : `padding-left: 14px` et `.artwork-wrapper` : `margin-left: 4px`.
  pub const LEFT_PADDING: f32 = css(18.0);
  /// `.artwork-wrapper` : 72 px.
  pub const COVER: f32 = css(72.0);
  /// `.m3c-canvas` (`CANVAS_PX`, `app.js:16769`) : 120 px, centré sur la pochette.
  pub const COVER_CANVAS: f32 = css(120.0);
  /// `.m3c-wrap` : pochette en forme (`COVER_PX`, `app.js:16768`) : 70 px.
  pub const SHAPE_COVER: f32 = css(70.0);
  /// `.playbar-left` : `gap: 14px`.
  pub const LEFT_GAP: f32 = css(14.0);
  /// `.track-meta` : `gap: 4px; padding-right: 6px`.
  pub const META_GAP: f32 = css(4.0);
  pub const META_PADDING_RIGHT: f32 = css(6.0);
  /// `.track-artist` : `calc(12.5px * var(--font-scale))`.
  pub const ARTIST_TEXT: f32 = css(12.5 * FONT_SCALE);
  /// `.track-title`, `.track-artist` : `line-height: 1.3`.
  pub const LINE_HEIGHT: f32 = 1.3;
  /// Fondu de droite des titres : `mask-image` sur les 24 derniers pixels.
  pub const FADE: f32 = css(24.0);

  /// `.btn-icon-pill`, `#btn-shuffle`, `#btn-repeat` : 36 px.
  pub const PILL: f32 = css(36.0);
  /// `#btn-prev`, `#btn-next` : 40 px.
  pub const SKIP: f32 = css(40.0);
  /// `.playback-controls` : `gap: 4px` ; `#btn-prev` `margin-left: 8px`, `#btn-next` `margin-right: 8px`.
  pub const CONTROL_GAP: f32 = css(4.0);
  pub const SKIP_MARGIN: f32 = css(8.0);
  /// `#btn-shuffle.active` : `box-shadow: 0 0 0 2px primary-container`.
  pub const RING: f32 = css(2.0);
  /// `.play-pause-wrapper` : 70 x 48. `.btn-play-pause` : 48 px au repos, 70 px en lecture.
  pub const PLAY_WRAPPER_WIDTH: f32 = css(70.0);
  pub const PLAY_HEIGHT: f32 = css(48.0);
  pub const PLAY_WIDTH_PAUSED: f32 = css(48.0);
  /// `.btn-play-pause` : `border-radius: 24px`, puis 18 px en lecture (`.playing`).
  pub const PLAY_RADIUS_PAUSED: f32 = css(24.0);
  pub const PLAY_RADIUS_PLAYING: f32 = css(18.0);
  /// `button:active` : `border-radius: 12px`.
  pub const PRESSED_RADIUS: f32 = css(12.0);
  /// `:is(.btn-icon-pill, .control-btn, ...):hover` : `border-radius: 40%`.
  pub const HOVER_RADIUS_SHARE: f32 = 0.4;
  /// `--hl` au survol : `:is(.btn-icon-pill, .control-btn, ...):not(:disabled):hover { --hl: 8% }` (`styles.css:7518`).
  pub const HOVER_LAYER: f32 = 0.08;

  /// `.time-label` : `width: 36px; font-size: calc(11px * var(--font-scale))`.
  pub const TIME_WIDTH: f32 = css(36.0);
  pub const TIME_TEXT: f32 = css(11.0 * FONT_SCALE);
  /// `layout.json` : 12 px entre le temps et la barre (`.scrubber-bar-wrapper`).
  pub const TIME_GAP: f32 = css(12.0);
  /// `.scrubber-track-container` : `height: 28px` ; `.squiggly-canvas` : `height: 40px`.
  pub const SCRUBBER_HEIGHT: f32 = css(28.0);
  pub const SCRUBBER_CANVAS_HEIGHT: f32 = css(40.0);

  /// `.playbar-right` : `gap: 12px; padding-right: 24px` ; `.volume-control-group` : `gap: 8px`.
  pub const RIGHT_GAP: f32 = css(12.0);
  pub const RIGHT_PADDING: f32 = css(24.0);
  pub const VOLUME_GAP: f32 = css(8.0);
  /// `.volume-slider-wrapper` : 80 px ; `.sl` : 28 px de haut.
  pub const VOLUME_WIDTH: f32 = css(80.0);
  pub const VOLUME_HEIGHT: f32 = css(28.0);

  /// `.btn-play-pause` : `transition: width 200ms, border-radius 200ms cubic-bezier(0.34, 1.56, 0.64, 1)` (`styles.css:3059`) ;
  /// la courbe dépasse 1 (`--transition-jelly`, `styles.css:77-83`).
  pub const PLAY_JELLY_MS: u64 = 200;
  pub const PLAY_JELLY_CURVE: [f32; 4] = [0.34, 1.56, 0.64, 1.0];
  /// `.play-pause-morph path` : `transition: d 400ms cubic-bezier(0.2, 0, 0, 1)` (`styles.css:3117`).
  pub const PLAY_MORPH_MS: u64 = 400;
  pub const PLAY_MORPH_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `#play-icon` : SVG de 22 px au `viewBox` de 24 (`index.html:2823`) ; trait de 1,6 (`styles.css:3111`).
  pub const PLAY_ICON_VIEW_BOX: f32 = 24.0;
  pub const PLAY_ICON_STROKE: f32 = 1.6;
  /// `scrubPhase += 3.5 * dt` en lecture seulement (`app.js:3453-3455`), en radians par seconde.
  pub const WAVE_RATE: f64 = 3.5;
  /// `Math.min(1, dt * 45)` au repos, `dt * 32` pendant un glissement (`app.js:3430-3432`), par seconde, à 60 images par seconde.
  pub const SCRUB_FOLLOW_RATE: f32 = 45.0;
  pub const SCRUB_FOLLOW_RATE_DRAG: f32 = 32.0;
  /// Cadence de référence de Riff (écran à 60 Hz) pour convertir `lerp(dt * k)` en approche exponentielle.
  pub const SCRUB_FOLLOW_REFERENCE_HZ: f32 = 60.0;
}

/// Panneau droit `#right-panel`, mesuré dans `ui-reference/layout.json` (clés `right-panel*`),
/// `tokens.json` et `styles.css:2137-2545`, `10761-10766`, `11704-11736`.
pub mod right_panel {
  use super::{css, playbar, shell};

  /// `--right-panel-width` par défaut, en pixels CSS (`styles.css:2139`, `app.js:14413`).
  pub const DEFAULT_WIDTH: f32 = 340.0;
  /// `border-radius: 24px` (`styles.css:10762`).
  pub const RADIUS: f32 = css(24.0);
  /// `padding: 56px 24px 24px` (`styles.css:2140`, `2451`).
  pub const PADDING_TOP: f32 = css(56.0);
  pub const PADDING_SIDE: f32 = css(24.0);
  pub const PADDING_BOTTOM: f32 = css(24.0);
  /// `margin-bottom: 10px` (`styles.css:10765`).
  pub const MARGIN_BOTTOM: f32 = css(10.0);
  /// `gap: 12px` entre la boîte de pochette et les paroles (`styles.css:11710`).
  pub const GAP: f32 = css(12.0);
  /// `.right-panel-artwork-box` : `gap: 8px` ; `.right-panel-meta` : `gap: 4px`.
  pub const ARTWORK_GAP: f32 = css(8.0);
  pub const META_GAP: f32 = css(4.0);
  /// `color-mix(in srgb, var(--page-bg) 98%, white 2%)` : part de blanc.
  pub const WHITE_SHARE: f32 = 0.02;
  /// `.lyrics-container` : `padding: 16px 8px` ; la part verticale est remplacée par
  /// `updateLyricsPadding` (`app.js:5534`).
  pub const LYRICS_PADDING_X: f32 = css(8.0);

  /// Titre et artiste avec des paroles : 16 px et 12 px par `--font-scale` (`styles.css:2350-2368`).
  pub const TITLE_TEXT: f32 = css(16.0 * playbar::FONT_SCALE);
  pub const ARTIST_TEXT: f32 = css(12.0 * playbar::FONT_SCALE);
  /// Titre et artiste sans paroles : 20 px (graisse 700) et 14,5 px (`styles.css:11729-11736`).
  pub const TITLE_TEXT_EMPTY: f32 = css(20.0 * playbar::FONT_SCALE);
  pub const ARTIST_TEXT_EMPTY: f32 = css(14.5 * playbar::FONT_SCALE);
  /// `line-height: normal` mesuré : 29,63 / 23,75 pour le titre en 700, 22,22 / 17,22 pour
  /// l'artiste en 500 (`layout.json`, `right-panel-title`, `right-panel-artist`). Le titre en 500
  /// reprend le rapport de l'artiste, de même graisse (déduit).
  pub const TITLE_LINE_BOLD: f32 = 29.63 / 23.75;
  pub const TEXT_LINE: f32 = 22.22 / 17.2188;

  /// `.right-panel-header` : `top/left/right: 12px` ; `.right-panel-header-actions` :
  /// `gap: 6px; padding: 5px 10px` (`styles.css:2454-2480`).
  pub const HEADER_INSET: f32 = css(12.0);
  pub const HEADER_GAP: f32 = css(6.0);
  pub const HEADER_PADDING_X: f32 = css(10.0);
  pub const HEADER_PADDING_Y: f32 = css(5.0);
  /// Part du `surface-container-highest` du fond des commandes : `color-mix(... 78%, transparent)`.
  pub const HEADER_ALPHA: f32 = 0.78;
  /// `.lyrics-action-btn`, `#btn-lyrics-mode`, `#btn-close-right-panel` : 32 px de côté.
  pub const BUTTON: f32 = css(32.0);
  pub const ICON: f32 = 16.0;
  /// `.lyrics-mode-btn` : `font-size: calc(11px * var(--font-scale))`, `padding: 4px 12px`.
  pub const MODE_TEXT: f32 = css(11.0 * playbar::FONT_SCALE);
  pub const MODE_PADDING_X: f32 = css(12.0);
  /// `opacity: 0.9 !important` des boutons au repos.
  pub const BUTTON_OPACITY: f32 = 0.9;
  /// `background: rgba(255, 255, 255, 0.12)` des boutons ronds.
  pub const BUTTON_FILL: f32 = 0.12;
  /// Cache des commandes : visibles 1,8 s après un mouvement, 350 ms après la sortie (`app.js:14243`).
  pub const CONTROLS_AFTER_MOVE: std::time::Duration = std::time::Duration::from_millis(1800);
  pub const CONTROLS_AFTER_LEAVE: std::time::Duration = std::time::Duration::from_millis(350);

  /// Côté de la pochette, en pixels écran : `clamp(240px, width - 36px, 320px)`, ou
  /// `clamp(268px, width - 28px, 356px)` sans paroles (`styles.css:11717-11727`).
  #[must_use]
  pub fn cover_side(width_css: f32, empty: bool) -> f32 {
    let (min, max, inset) = if empty {
      (268.0, 356.0, 28.0)
    } else {
      (240.0, 320.0, 36.0)
    };
    css((width_css - inset).clamp(min, max))
  }

  /// Hauteur de la boîte de pochette : pochette, titre, artiste et leurs écarts.
  #[must_use]
  pub fn artwork_height(width_css: f32, empty: bool) -> f32 {
    let (title, artist) = if empty {
      (
        TITLE_TEXT_EMPTY * TITLE_LINE_BOLD,
        ARTIST_TEXT_EMPTY * TEXT_LINE,
      )
    } else {
      (TITLE_TEXT * TEXT_LINE, ARTIST_TEXT * TEXT_LINE)
    };
    cover_side(width_css, empty) + ARTWORK_GAP + title + META_GAP + artist
  }

  /// Hauteur fixe autour de la zone des paroles dans la fenêtre : barres, marges, pochette, titre.
  #[must_use]
  pub fn chrome_height(width_css: f32) -> f32 {
    shell::TITLEBAR_HEIGHT
      + shell::TOP_NAV_HEIGHT
      + playbar::HEIGHT
      + MARGIN_BOTTOM
      + PADDING_TOP
      + PADDING_BOTTOM
      + GAP
      + artwork_height(width_css, false)
  }
}

/// Écran d'accueil `#view-discover` (disposition « spotify », celle d'un Riff vierge), mesuré dans
/// `ui-reference/layout.json` (zones `main-content`, `view-discover`, `sp-*-sec`, `cssGrid`) et
/// `styles.css:9281-9420`, `9470-9487`, `10979-10997`.
pub mod home {
  use super::{css, text};

  /// `.main-content` : `padding: 22px 10px 24px 20px` (`layout.json`, `cssGrid.main-content`).
  pub const MAIN_TOP: f32 = css(22.0);
  pub const MAIN_RIGHT: f32 = css(10.0);
  pub const MAIN_BOTTOM: f32 = css(24.0);
  pub const MAIN_LEFT: f32 = css(20.0);
  /// `.sp-home` : `padding: 4px 4px 40px`, `gap: 28px`.
  pub const HOME_SIDE: f32 = css(4.0);
  pub const HOME_BOTTOM: f32 = css(40.0);
  pub const SECTION_GAP: f32 = css(28.0);
  /// Barre de défilement verticale : `::-webkit-scrollbar { width: 5px }` (`styles.css:209`).
  pub const SCROLLBAR: f32 = css(5.0);
  /// Part de `on-surface` du pouce (`styles.css:212`).
  pub const SCROLLER_SHARE: f32 = 0.28;

  /// `.sp-section h2` : `gap: 10px; margin: 0 0 14px; font-size: 22px`, icône 20 px (taille CSS).
  pub const H2_GAP: f32 = css(10.0);
  pub const H2_MARGIN: f32 = css(14.0);
  pub const H2_ICON: f32 = 20.0;
  /// Hauteur mesurée du `h2` sans bouton : 27,46 px CSS pour 22 px (`sp-quick-sec`, 177,46 - 2 x 64 - 8 - 14).
  pub const H2_LINE: f32 = 27.46 / 22.0;
  /// `#btn-regen-mixes` : `width: 32px; height: 32px`, icône de 16 px (taille CSS).
  pub const REFRESH: f32 = css(32.0);
  pub const REFRESH_ICON: f32 = 16.0;

  /// `.sp-row` : `grid-auto-columns: 180px; gap: 16px; padding: 0 0 4px`.
  pub const CARD_WIDTH: f32 = css(180.0);
  pub const CARD_GAP: f32 = css(16.0);
  pub const ROW_PADDING_BOTTOM: f32 = css(4.0);
  /// `.sp-card-cover` : `border-radius: 14px`, relevée de 2 px au survol (`translateY(-2px)`).
  pub const COVER_RADIUS: f32 = css(14.0);
  pub const COVER_LIFT: f32 = css(2.0);
  /// Bouton « Play » d'une carte de mix, au survol : cercle de 44 px à 8 px du coin bas droit de la pochette.
  pub const CARD_PLAY: f32 = css(44.0);
  pub const CARD_PLAY_ICON: f32 = 20.0;
  pub const CARD_PLAY_MARGIN: f32 = css(8.0);
  /// `.sp-grid-cover` : `gap: 2px; padding: 2px`, images à `border-radius: 4px` (`styles.css:9470`).
  pub const GRID_GAP: f32 = css(2.0);
  pub const GRID_RADIUS: f32 = css(4.0);
  /// `.sp-card-title` : `margin-top: 10px`, `.sp-card-sub` : `margin-top: 2px`.
  pub const TITLE_MARGIN: f32 = css(10.0);
  pub const SUB_MARGIN: f32 = css(2.0);
  /// Hauteur de ligne du titre (14,5 px) et du sous-titre (12,5 px) : 180 + 10 + 2 + t + s + 4 =
  /// 228,79 px CSS mesurés pour `sp-trend-sec`, soit t + s = 32,79 px, partagés au même rapport (dérivé).
  pub const TEXT_LINE: f32 = 32.79 / (14.5 + 12.5);
  /// Fondu des textes (`.sp-fade`, 28 px) et des bords d'une rangée (`.fade-left/right`, 36 px).
  pub const TEXT_FADE: f32 = css(28.0);
  pub const ROW_FADE: f32 = css(36.0);
  /// Seuil de `updateRowFade` : 4 px de défilement.
  pub const ROW_FADE_THRESHOLD: f32 = css(4.0);
  /// `.sp-card-cover-empty svg` : 30 % du côté, opacité 0,6.
  pub const EMPTY_ICON_SHARE: f32 = 0.30;
  pub const EMPTY_ICON_OPACITY: f32 = 0.6;

  /// Hauteur d'une carte au repos : levée de survol réservée, pochette, titre, sous-titre, marge basse.
  pub const CARD_HEIGHT: f32 = COVER_LIFT
    + CARD_WIDTH
    + TITLE_MARGIN
    + text::CARD_TITLE * TEXT_LINE
    + SUB_MARGIN
    + text::SUBTITLE * TEXT_LINE
    + ROW_PADDING_BOTTOM;

  /// `.sp-quick` : trois colonnes, `gap: 8px`. `.sp-tile` : `height: 64px; gap: 12px; padding: 0 12px 0 0;
  /// border-radius: 12px`, fond `surface-container-high` à 70 %.
  pub const QUICK_COLUMNS: usize = 3;
  pub const QUICK_GAP: f32 = css(8.0);
  pub const TILE_HEIGHT: f32 = css(64.0);
  pub const TILE_GAP: f32 = css(12.0);
  pub const TILE_PADDING_RIGHT: f32 = css(12.0);
  pub const TILE_RADIUS: f32 = css(12.0);
  pub const TILE_ALPHA: f32 = 0.7;
  /// `.sp-heart svg` : 24 px (taille CSS). `.sp-tile .sp-card-cover-empty svg` : 26 px.
  pub const TILE_HEART_ICON: f32 = 24.0;
  pub const TILE_EMPTY_ICON: f32 = css(26.0);

  /// `.m3-spinner-container` d'une carte en attente : 32 px, SVG de 24 px, fond transparent.
  pub const SPINNER_BOX: f32 = css(32.0);
  pub const SPINNER_SVG: f32 = css(24.0);
  /// Chargement des tendances : conteneur de 48 px à fond `surface-container`, SVG de 36 px, marge de 24 px.
  pub const SPINNER_BIG_BOX: f32 = css(48.0);
  pub const SPINNER_BIG_SVG: f32 = css(36.0);
  pub const SPINNER_BIG_MARGIN: f32 = css(24.0);
  /// `@keyframes m3Rotate` (`linear`) et `m3Dash` : 1,4 s (`styles.css:3360`, `3371`) ; cercle `r = 18` pour un `viewBox` de 48, trait de 4.
  pub const SPINNER_PERIOD_SECS: f32 = 1.4;
  /// `ease-in-out` : `cubic-bezier(0.42, 0, 0.58, 1)`, appliquée à chaque intervalle de `m3Dash` (`styles.css:3371`).
  pub const SPINNER_DASH_CURVE: [f32; 4] = [0.42, 0.0, 0.58, 1.0];
  /// Images clés de `m3Dash` : `stroke-dasharray` 1 puis 90 (`styles.css:3385-3391`).
  pub const SPINNER_DASH_START: f32 = 1.0;
  pub const SPINNER_DASH_END: f32 = 90.0;
  /// `stroke-dashoffset` à 50 % puis à 100 % (`styles.css:3387`, `3389`).
  pub const SPINNER_OFFSET_MID: f32 = -35.0;
  pub const SPINNER_OFFSET_END: f32 = -125.0;
  /// Style de base de `.m3-spinner-head` : `stroke-dasharray: 80, 200`, `stroke-dashoffset: 0` (`styles.css:3381-3382`), celui que revoit
  /// le spinner quand la réduction de mouvement termine `m3Rotate` et `m3Dash` (sans `forwards`).
  pub const SPINNER_BASE_DASH: f32 = 80.0;

  /// `.fav-artists-row` : colonnes de 168 px, `gap: 20px`. `.fav-artist-card` : `gap: 10px`.
  pub const ARTIST_WIDTH: f32 = css(168.0);
  pub const ARTIST_GAP: f32 = css(20.0);
  pub const ARTIST_CIRCLE: f32 = css(156.0);
  pub const ARTIST_NAME_GAP: f32 = css(10.0);
  pub const ARTIST_NAME_TEXT: f32 = css(14.0);
  /// `.fav-artist-plays` : `bottom: 10px; padding: 4px 10px; gap: 5px; font-size: 12px`, icône de 10 px.
  pub const PLAYS_BOTTOM: f32 = css(10.0);
  pub const PLAYS_PADDING_X: f32 = css(10.0);
  pub const PLAYS_PADDING_Y: f32 = css(4.0);
  pub const PLAYS_GAP: f32 = css(5.0);
  pub const PLAYS_TEXT: f32 = css(12.0);

  /// `.spotify-promo` : `gap: 20px; margin: 0 0 24px; padding: 16px 20px; border-radius: 24px`.
  pub const PROMO_GAP: f32 = css(20.0);
  pub const PROMO_MARGIN: f32 = css(24.0);
  pub const PROMO_PADDING_Y: f32 = css(16.0);
  pub const PROMO_PADDING_X: f32 = css(20.0);
  pub const PROMO_RADIUS: f32 = css(24.0);
  /// `.sp-promo-ico` : 36 px, `gap: 6px` ; titre 18 px (700), sous-titre 13 px ; `gap: 2px`.
  pub const PROMO_ICON_BOX: f32 = css(36.0);
  pub const PROMO_ICON_GAP: f32 = css(6.0);
  pub const PROMO_TITLE: f32 = css(18.0);
  pub const PROMO_SUB: f32 = css(13.0);
  pub const PROMO_TEXT_GAP: f32 = css(2.0);
  /// `.btn-pill-primary` : `padding: 10px 24px`, `font-size: calc(14px * var(--font-scale))`, graisse 500.
  pub const PROMO_BUTTON_PADDING_Y: f32 = css(10.0);
  pub const PROMO_BUTTON_PADDING_X: f32 = css(24.0);
  pub const PROMO_BUTTON_TEXT: f32 = css(14.0 * super::playbar::FONT_SCALE);
  /// `#btn-spotify-promo-close` : `.btn-icon-pill` de 36 px, croix de 14 px (taille CSS).
  pub const PROMO_CLOSE: f32 = css(36.0);
  pub const PROMO_CLOSE_ICON: f32 = 14.0;
}

/// Historique de recherche `#search-history-panel`, mesuré dans `ui-reference/screens/02-recherche-historique.png`
/// et `styles.css:11003-11081`, construit par `initSearchHistory` (`app.js:10098-10246`).
pub mod search {
  use super::css;

  /// `.search-history-panel` : `top: calc(100% + 8px)` (`styles.css:11005`).
  pub const PANEL_GAP: f32 = css(8.0);
  /// `max-height: 440px` (`styles.css:11008`), `padding: 8px` (`:11010`), `border-radius: 20px` (`:11012`).
  pub const PANEL_MAX_HEIGHT: f32 = css(440.0);
  pub const PANEL_PADDING: f32 = css(8.0);
  pub const PANEL_RADIUS: f32 = css(20.0);
  /// Hauteur de ligne du texte « normal » de Google Sans : 27,46 px CSS mesurés pour 22 px (`home::H2_LINE`).
  pub const TEXT_LINE: f32 = super::home::H2_LINE;

  /// `.sh-head` : `padding: 8px 10px 6px`, graisse 700, taille héritée (19 px CSS) (`styles.css:11031`).
  pub const HEAD_TOP: f32 = css(8.0);
  pub const HEAD_SIDE: f32 = css(10.0);
  pub const HEAD_BOTTOM: f32 = css(6.0);
  pub const HEAD_TEXT: f32 = super::text::BODY;
  /// `.sh-clear` : `font-size: 13px`, graisse 600 (`styles.css:11038-11039`).
  pub const CLEAR_TEXT: f32 = css(13.0);

  /// `.sh-row` : `gap: 12px; padding: 6px 10px; border-radius: 12px` (`styles.css:11047-11049`).
  pub const ROW_GAP: f32 = css(12.0);
  pub const ROW_PADDING_Y: f32 = css(6.0);
  pub const ROW_PADDING_X: f32 = css(10.0);
  pub const ROW_RADIUS: f32 = css(12.0);
  /// `.sh-thumb` : 48 px, `border-radius: 8px` (`styles.css:11054-11056`) ; `.round` : disque (`:11064`).
  pub const THUMB: f32 = css(48.0);
  pub const THUMB_RADIUS: f32 = css(8.0);
  /// SVG de repli de la vignette : 18 px CSS (`app.js:10170-10172`).
  pub const THUMB_ICON: f32 = 18.0;
  /// `.sh-title` : graisse 600, taille héritée, `gap: 6px` (`styles.css:11067`).
  pub const TITLE_TEXT: f32 = super::text::BODY;
  pub const TITLE_GAP: f32 = css(6.0);
  /// `.sh-sub` : `font-size: 13px; gap: 6px` (`styles.css:11068`).
  pub const SUB_TEXT: f32 = css(13.0);
  pub const SUB_GAP: f32 = css(6.0);
  /// `.sh-remove` : `padding: 6px`, croix de 14 px CSS (`styles.css:11072`, `app.js:10211`).
  pub const REMOVE_PADDING: f32 = css(6.0);
  pub const REMOVE_ICON: f32 = 14.0;
  /// `.verified-check` : 14 px (`app.js:10023`) ; `.explicit-badge` : `margin-right: 6px` (`styles.css:9434`).
  pub const VERIFIED: f32 = 14.0;
  pub const BADGE_GAP: f32 = css(6.0);
}

/// Ligne de piste `.track-row` (`createTrackRow`, `app.js:11157-11245`), mesurée dans
/// `ui-reference/screens/03-resultats-recherche.png` et `styles.css:1572-1780`, `9424`, `10013`, `10113`.
pub mod track_row {
  use super::{css, playbar::FONT_SCALE};

  /// `.track-row` : `padding: 10px 18px; gap: 16px; border-radius: 10px` (`styles.css:1577-1582`).
  pub const PADDING_Y: f32 = css(10.0);
  pub const PADDING_X: f32 = css(18.0);
  pub const GAP: f32 = css(16.0);
  pub const RADIUS: f32 = css(10.0);
  /// `body.classic-track-rows .track-row` : `border-radius: 20px` (`styles.css:1592`).
  pub const CLASSIC_RADIUS: f32 = css(20.0);
  /// `.tracks-list` : `gap: 4px` (`styles.css:1572`).
  pub const LIST_GAP: f32 = css(4.0);
  /// `.track-row-num` : 24 px, `12px x --font-scale`, couleur `outline` (`styles.css:1621`).
  pub const NUM_WIDTH: f32 = css(24.0);
  pub const NUM_TEXT: f32 = css(12.0 * FONT_SCALE);
  /// `.track-row-thumb-wrap` : 44 px, `border-radius: var(--md-shape-corner-m)` (`styles.css:1629`).
  pub const THUMB: f32 = css(44.0);
  pub const THUMB_RADIUS: f32 = css(12.0);
  /// `.track-row-fallback-icon` : 20 px CSS (`styles.css:1641`).
  pub const THUMB_ICON: f32 = 20.0;
  /// `.track-row-info` : `gap: 3px` (`styles.css:1667`).
  pub const INFO_GAP: f32 = css(3.0);
  /// `.track-row-title` : `14px x --font-scale`, graisse 500 (`styles.css:1676`).
  pub const TITLE_TEXT: f32 = css(14.0 * FONT_SCALE);
  /// `.track-row-artist` et `.track-row-duration` : `12px x --font-scale` (`styles.css:1729`, `1747`).
  pub const ARTIST_TEXT: f32 = css(12.0 * FONT_SCALE);
  pub const DURATION_TEXT: f32 = css(12.0 * FONT_SCALE);
  /// `.explicit-badge` dans `.track-row-artist-wrap` : `margin-right: 6px` (`styles.css:9434`).
  pub const BADGE_GAP: f32 = css(6.0);
  /// `.btn-icon-pill` : 36 px, icône de 16 px CSS (`styles.css:3285`, `app.js:11229`).
  pub const ACTION: f32 = css(36.0);
  pub const ACTION_ICON: f32 = 16.0;
  /// `.sp-playing-bars` : 16 x 14 px, quatre barres de 2,5 px, `gap: 2px`, au repos 35 % (`styles.css:1692`).
  pub const BARS_WIDTH: f32 = css(16.0);
  pub const BARS_HEIGHT: f32 = css(14.0);
  pub const BAR_WIDTH: f32 = css(2.5);
  pub const BAR_GAP: f32 = css(2.0);
  pub const BAR_REST: f32 = 0.35;
  /// `@keyframes spBarEq` : `0% { height: 20% }` vers `100% { height: 100% }` (`styles.css:1718-1721`). Avant la fin du délai, `animation-fill-mode`
  /// vaut `none` : la barre garde la hauteur de sa règle, `height: 100%` (`styles.css:1708`).
  pub const BAR_LOW: f32 = 0.20;
  pub const BAR_HIGH: f32 = 1.0;
  /// `animation: spBarEq 0.8s ease-in-out infinite alternate` (`styles.css:1711`) : `ease-in-out` = `cubic-bezier(0.42, 0, 0.58, 1)`, appliquée
  /// à chaque itération ; `alternate` rejoue les itérations impaires à l'envers (progrès dirigé `1 - p`).
  pub const BAR_CURVE: [f32; 4] = [0.42, 0.0, 0.58, 1.0];
  /// `animation-delay` des barres `.b1` à `.b4` (`styles.css:1713-1716`).
  pub const BAR_DELAY_MS: [f32; 4] = [100.0, 300.0, 0.0, 200.0];
  /// `animation-duration` des barres `.b1` à `.b4` (`styles.css:1713-1716`) : la durée de 0,8 s de `animation` est écrasée.
  pub const BAR_DURATION_MS: [f32; 4] = [750.0, 550.0, 850.0, 650.0];
  /// `.track-row { transition: background-color 140ms ease !important }` (`styles.css:1589`) : le `!important` l'emporte sur les
  /// 70 ms de `.track-row:hover` (`styles.css:143-146`) ; `body.classic-track-rows` garde 140 ms pour le fond (`styles.css:1594`).
  /// Le fond de survol est donc fondu dans les deux sens sur 140 ms.
  pub const HOVER_MS: f32 = 140.0;
  /// `ease` : `cubic-bezier(0.25, 0.1, 0.25, 1)`.
  pub const HOVER_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
  /// `.track-row.track-selected` : `primary` à 22 % sur `surface-container-high` (`styles.css:10150`).
  pub const SELECTED_SHARE: f32 = 0.22;
  /// Hauteur de la boîte : 44 + 2 x 10 px CSS.
  pub const HEIGHT: f32 = THUMB + 2.0 * PADDING_Y;
  /// Pas d'une ligne dans la liste (boîte et `gap`), aussi la hauteur d'une ligne virtualisée.
  pub const PITCH: f32 = HEIGHT + LIST_GAP;
}

/// Résultats de recherche `#search-results-container` (`index.html:621-631`), mesurés dans
/// `ui-reference/screens/03*.png` (1920 x 1080 px écran, zoom 0,945) et `styles.css`.
pub mod search_results {
  use super::{css, home, playbar::FONT_SCALE};

  /// `.search-filter-chips` : `gap: 8px; margin-bottom: 14px` (`styles.css:10553`).
  pub const CHIPS_GAP: f32 = css(8.0);
  pub const CHIPS_MARGIN: f32 = css(14.0);
  /// `.search-chip` : `height: 32px; padding: 0 14px; border-radius: 16px; font-size: 13px` (`styles.css:10559`).
  pub const CHIP_HEIGHT: f32 = css(32.0);
  pub const CHIP_PADDING_X: f32 = css(14.0);
  pub const CHIP_RADIUS: f32 = css(16.0);
  pub const CHIP_TEXT: f32 = css(13.0);

  /// `h3` de `.section-sub-header` : `1,17em` de la taille du corps (19 px CSS), sans marge (mesuré, aucune
  /// règle dans `styles.css` ; `app.js:9858`).
  pub const H3_TEXT: f32 = css(19.0 * 1.17);
  /// `.search-albums-section` : `margin-bottom: 24px` (`styles.css:8409`).
  pub const SECTION_MARGIN: f32 = css(24.0);
  /// `.search-albums-row` : `gap: 16px; padding: 4px 0 12px` (`styles.css:8412`).
  pub const ROW_GAP: f32 = css(16.0);
  pub const ROW_PADDING_TOP: f32 = css(4.0);
  pub const ROW_PADDING_BOTTOM: f32 = css(12.0);
  /// `.search-album-card` : 172 px, `gap: 10px`, levée de 2 px au survol (`styles.css:8436-8449`).
  pub const CARD_WIDTH: f32 = css(172.0);
  pub const CARD_GAP: f32 = css(10.0);
  pub const CARD_LIFT: f32 = css(2.0);
  /// `.search-album-cover` : `border-radius: 18px` (`styles.css:8452`).
  pub const COVER_RADIUS: f32 = css(18.0);
  /// SVG de repli d'une pochette absente : 32 px CSS (`app.js:9876`).
  pub const COVER_ICON: f32 = 32.0;
  /// `.search-album-info` : `gap: 2px`. Titre `13px x --font-scale` (600), sous-titre `12px x --font-scale`.
  pub const INFO_GAP: f32 = css(2.0);
  pub const TITLE_TEXT: f32 = css(13.0 * FONT_SCALE);
  pub const SUB_TEXT: f32 = css(12.0 * FONT_SCALE);
  /// Fondu des bords de la rangée : `.fade-left`, `.fade-right` (`styles.css:8386`).
  pub const ROW_FADE: f32 = home::ROW_FADE;
  pub const ROW_FADE_THRESHOLD: f32 = home::ROW_FADE_THRESHOLD;
  /// Hauteur d'une carte d'album : pochette, écart, titre, écart, sous-titre.
  pub const CARD_HEIGHT: f32 =
    CARD_WIDTH + CARD_GAP + TITLE_TEXT * home::H2_LINE + INFO_GAP + SUB_TEXT * home::H2_LINE;
  /// Hauteur de la section : `h3`, rangée avec ses marges, marge basse.
  pub const ALBUMS_HEIGHT: f32 =
    H3_TEXT * home::H2_LINE + ROW_PADDING_TOP + CARD_HEIGHT + ROW_PADDING_BOTTOM + SECTION_MARGIN;

  /// `.search-artist-card` : `gap: 16px; padding: 12px 20px; margin-bottom: 16px; border-radius: 28px` (`styles.css:1496`).
  pub const ARTIST_GAP: f32 = css(16.0);
  pub const ARTIST_PADDING_Y: f32 = css(12.0);
  pub const ARTIST_PADDING_X: f32 = css(20.0);
  pub const ARTIST_MARGIN: f32 = css(16.0);
  pub const ARTIST_RADIUS: f32 = css(28.0);
  /// `.search-artist-avatar-wrap` : 64 px, disque (`styles.css:1514`).
  pub const AVATAR: f32 = css(64.0);
  /// `.search-artist-info` : `gap: 2px`. Nom `16px x --font-scale` (700, `line-height: 1.3`), libellé `12px x --font-scale` (500).
  pub const ARTIST_INFO_GAP: f32 = css(2.0);
  pub const ARTIST_NAME_TEXT: f32 = css(16.0 * FONT_SCALE);
  pub const ARTIST_NAME_LINE: f32 = 1.3;
  pub const ARTIST_LABEL_TEXT: f32 = css(12.0 * FONT_SCALE);
  /// `.verified-check` : 14 px, `margin-left: 6px` (`styles.css:11080`).
  pub const VERIFIED: f32 = 14.0;
  pub const VERIFIED_MARGIN: f32 = css(6.0);
  /// Hauteur de la carte et de sa marge basse.
  pub const ARTIST_HEIGHT: f32 = AVATAR + 2.0 * ARTIST_PADDING_Y + ARTIST_MARGIN;

  /// `.search-icon-cover` : 22 px, `border-radius: 6px`, `margin-right: 12px` (`styles.css:408`) ; loupe de 18 px.
  pub const ICON_COVER: f32 = css(22.0);
  pub const ICON_COVER_RADIUS: f32 = css(6.0);
  pub const ICON_LOUPE: f32 = css(18.0);

  /// `.m3-search-loader-wrap` : `padding: 56px 20px; gap: 18px`, scène de 96 px (`styles.css:10392-10402`).
  pub const LOADER_PADDING_Y: f32 = css(56.0);
  pub const LOADER_PADDING_X: f32 = css(20.0);
  pub const LOADER_GAP: f32 = css(18.0);
  pub const LOADER_STAGE: f32 = css(96.0);
  /// Pas du morphing `STEP_MS` (`app.js:9959`) et tour complet de la forme : `now % 3000` (`app.js:9971`).
  pub const LOADER_STEP_SECS: f32 = 1.2;
  pub const LOADER_TURN_SECS: f32 = 3.0;
  /// Parts de `on-surface` du remplissage : 22 % en chargement, 14 % en erreur (`styles.css:10411`, `10415`).
  pub const LOADER_FILL: f32 = 0.22;
  pub const LOADER_FILL_ERROR: f32 = 0.14;
  /// `.m3-search-error-icon` : SVG de 34 px, opacité 0,85 (`styles.css:10417`, `10428`, `app.js:9922`).
  pub const ERROR_ICON: f32 = 34.0;
  pub const ERROR_ICON_OPACITY: f32 = 0.85;
  /// `.m3-search-error-icon` : `opacity: 0; transform: scale(0.75)` de base, `.is-error` (posé avec la boîte d'erreur, `app.js:9940`) les
  /// mène à `0.85` et `scale(1)` par `transition: opacity 320ms ease 180ms, transform 320ms cubic-bezier(0.2, 0.9, 0.2, 1) 180ms`
  /// (`styles.css:10417-10432`). La courbe d'échelle ne dépasse pas 1 (`y2 = 1`, `y1 = 0.9`).
  pub const ERROR_ICON_DELAY_MS: f32 = 180.0;
  pub const ERROR_ICON_MS: f32 = 320.0;
  pub const ERROR_ICON_FADE_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
  pub const ERROR_ICON_SCALE_CURVE: [f32; 4] = [0.2, 0.9, 0.2, 1.0];
  pub const ERROR_ICON_SCALE_FROM: f32 = 0.75;
  /// `.m3-search-error-box` : `gap: 10px; max-width: 460px` ; titre 14 px (600), détail 12 px monospace,
  /// `padding: 6px 12px; border-radius: 10px`, fond `on-surface` à 8 % (`styles.css:10431-10460`).
  pub const ERROR_GAP: f32 = css(10.0);
  pub const ERROR_MAX_WIDTH: f32 = css(460.0);
  pub const ERROR_TITLE_TEXT: f32 = css(14.0);
  pub const ERROR_DETAIL_TEXT: f32 = css(12.0);
  pub const ERROR_DETAIL_PADDING_Y: f32 = css(6.0);
  pub const ERROR_DETAIL_PADDING_X: f32 = css(12.0);
  pub const ERROR_DETAIL_RADIUS: f32 = css(10.0);
  pub const ERROR_DETAIL_SHARE: f32 = 0.08;
  /// Bouton « Copy error & diagnostics » : `margin-top: 4px; height: 34px; padding: 0 16px; font-size: 12.5px` (`app.js:10001`).
  pub const ERROR_BUTTON_MARGIN: f32 = css(4.0);
  pub const ERROR_BUTTON_HEIGHT: f32 = css(34.0);
  pub const ERROR_BUTTON_PADDING_X: f32 = css(16.0);
  pub const ERROR_BUTTON_TEXT: f32 = css(12.5);
  /// `.m3-search-error-box` : `opacity: 0; transform: translateY(8px); animation: m3ErrFadeIn 320ms ease 220ms forwards`,
  /// `@keyframes m3ErrFadeIn { to { opacity: 1; transform: translateY(0) } }` (`styles.css:10433-10445`). Avant le délai la boîte est
  /// à son style de base (invisible), après la fin `forwards` garde l'arrivée.
  pub const ERROR_FADE_DELAY_MS: f32 = 220.0;
  pub const ERROR_FADE_MS: f32 = 320.0;
  pub const ERROR_FADE_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
  pub const ERROR_FADE_OFFSET: f32 = css(8.0);

  /// `.empty-hint` : `13px x --font-scale`, couleur `outline`, centré, `padding: 32px 0` (`styles.css:2784`).
  pub const HINT_TEXT: f32 = css(13.0 * FONT_SCALE);
  pub const HINT_PADDING_Y: f32 = css(32.0);
}

/// Vue « Albums » `#view-albums` (`index.html:666`, `app.js:9481-9505`), mesurée dans
/// `ui-reference/screens/04d-vue-albums.png`.
pub mod albums {
  use super::{css, search_results};

  /// `#saved-albums-grid` : `repeat(auto-fill, minmax(180px, 1fr))`, `gap: 22px` (`styles.css:8457-8460`).
  pub const GRID_MIN: f32 = css(180.0);
  pub const GRID_GAP: f32 = css(22.0);
  /// `#saved-albums-grid` : `padding: 8px 4px 32px 4px` (`styles.css:8460`).
  pub const GRID_PADDING_TOP: f32 = css(8.0);
  pub const GRID_PADDING_SIDE: f32 = css(4.0);
  pub const GRID_PADDING_BOTTOM: f32 = css(32.0);
  /// `.empty-hint` de la vue vide : `padding: 48px 0` (`app.js:9486`), dans la première colonne de la grille.
  pub const HINT_PADDING_Y: f32 = css(48.0);
  /// Texte du message vide : celui de `.empty-hint` (`styles.css:2784`).
  pub const HINT_TEXT: f32 = search_results::HINT_TEXT;
}

/// Vue « Mes fichiers » `#view-local` (`index.html:668-690`, `app.js:12653-12814`), mesurée dans
/// `ui-reference/screens/04e-mes-fichiers.png`.
pub mod local {
  use super::{css, playbar::FONT_SCALE, search_results};

  /// `.view-header` de `#view-local` : titre masqué, boutons à droite, `margin-bottom: 16px; padding-bottom: 0` (`styles.css:1490-1493`).
  pub const HEADER_MARGIN: f32 = css(16.0);
  /// Écart entre les deux boutons : 4,5 px écran mesurés sur 04e (aucune règle CSS trouvée).
  pub const HEADER_GAP: f32 = css(4.8);
  /// `.btn-pill-primary` : `height: 40px; padding: 10px 24px; gap: 8px; font-size: 14px x --font-scale` (`styles.css:3303-3316`).
  pub const BUTTON_HEIGHT: f32 = css(40.0);
  pub const BUTTON_TEXT: f32 = css(14.0 * FONT_SCALE);
  pub const PRIMARY_PADDING_X: f32 = css(24.0);
  /// `.btn-outline-pill` : `padding: 8px 20px; gap: 8px` (`styles.css:3323-3335`).
  pub const OUTLINE_PADDING_X: f32 = css(20.0);
  pub const BUTTON_GAP: f32 = css(8.0);
  /// SVG des boutons : 16 px (`index.html:672`).
  pub const BUTTON_ICON: f32 = 16.0;
  /// `.view-search-box` : `gap: 10px; padding: 8px 16px; margin-bottom: 16px` (`styles.css:1543-1551`).
  pub const SEARCH_GAP: f32 = css(10.0);
  pub const SEARCH_PADDING_X: f32 = css(16.0);
  pub const SEARCH_MARGIN: f32 = css(16.0);
  /// Hauteur mesurée de la pastille : 33 px écran sur 04e, soit 35 px CSS (2 x 8 px et un champ de 19 px).
  pub const SEARCH_HEIGHT: f32 = css(35.0);
  /// `.view-search-input` : `13px x --font-scale` (`styles.css:1560-1567`) ; loupe de 16 px (`index.html:681`).
  pub const SEARCH_TEXT: f32 = css(13.0 * FONT_SCALE);
  pub const SEARCH_ICON: f32 = 16.0;
  /// `#local-albums-row` : `margin-bottom: 18px` en ligne (`index.html:685`).
  pub const ALBUMS_MARGIN: f32 = css(18.0);
  /// Hauteur de la rangée d'albums : `.search-albums-row` (`padding: 4px 0 12px`) autour d'une carte.
  pub const ALBUMS_HEIGHT: f32 = search_results::ROW_PADDING_TOP
    + search_results::CARD_HEIGHT
    + search_results::ROW_PADDING_BOTTOM;
  /// Liste vide : `.empty-hint` en colonne centrée, `gap: 12px` (`app.js:12796`), boutons séparés de 10 px (`app.js:12798`).
  pub const EMPTY_GAP: f32 = css(12.0);
  pub const EMPTY_BUTTONS_GAP: f32 = css(10.0);
}

/// Bibliothèque de la barre latérale (`#sidebar-library-wrap`) : onglets `.sb-horiz-tab`, flux
/// `#sidebar-library-feed`, rangées `.sb-lib-row` (`styles.css:11379-11600`, `app.js:27252-27456`).
pub mod sidebar {
  use super::{css, playbar::FONT_SCALE};

  /// `.sb-home-banner-title` : `20px x --font-scale`, graisse 800 (rendue en 700 : Google Sans ne déclare que 400, 500 et 700) (`styles.css:11334-11335`).
  pub const BANNER_TITLE_TEXT: f32 = css(20.0 * FONT_SCALE);
  /// `.sb-home-banner-Action` : `12px x --font-scale`, graisse 600 (`styles.css:11348`).
  pub const BANNER_ACTION_TEXT: f32 = css(12.0 * FONT_SCALE);

  /// `.sb-horiz-tabs` : `gap: 6px; padding: 2px 4px 0 4px`.
  pub const TABS_GAP: f32 = css(6.0);
  pub const TABS_PADDING_TOP: f32 = css(2.0);
  pub const TABS_PADDING_X: f32 = css(4.0);
  /// `.sb-horiz-tabs` : `border-bottom` en `outline-variant` à 55 % (`styles.css:11388`).
  pub const TABS_RULE_SHARE: f32 = 0.55;
  /// `.sb-horiz-tab` : `height: 38px; padding: 0 10px; border-radius: 12px 12px 0 0` (`styles.css:11401-11404`).
  pub const TAB_HEIGHT: f32 = css(38.0);
  pub const TAB_PADDING_X: f32 = css(10.0);
  pub const TAB_RADIUS: f32 = css(12.0);
  /// `.sb-horiz-tab` : `font-size: calc(12.5px * var(--font-scale))` (`styles.css:11408`).
  pub const TAB_TEXT: f32 = css(12.5 * FONT_SCALE);
  /// `.sb-horiz-tab:hover:not(.active)` : `on-surface` à 5 % ; `.active` : à 10 % (`styles.css:11416-11421`).
  pub const TAB_HOVER_SHARE: f32 = 0.05;
  pub const TAB_ACTIVE_SHARE: f32 = 0.10;
  /// `.sb-horiz-tab::before` : trait de 32 x 3 px, coins hauts de 3 px (`styles.css:11431-11432`).
  pub const TAB_UNDERLINE_WIDTH: f32 = css(32.0);
  pub const TAB_UNDERLINE_HEIGHT: f32 = css(3.0);

  /// `.sidebar-library-feed` : `gap: 6px; padding: 6px 2px 14px 2px` (`styles.css:11447, 11455`).
  pub const FEED_GAP: f32 = css(6.0);
  pub const FEED_PADDING_TOP: f32 = css(6.0);
  pub const FEED_PADDING_X: f32 = css(2.0);
  pub const FEED_PADDING_BOTTOM: f32 = css(14.0);
  /// Masque `linear-gradient(... transparent 100%)` sur les 22 derniers pixels (`styles.css:11457`).
  pub const FADE_BOTTOM: f32 = css(22.0);
  /// `.fade-top` : transparent jusqu'à 0, plein à 18 px (`styles.css:11464`).
  pub const FADE_TOP: f32 = css(18.0);
  /// `feed.scrollTop > 6` active `.fade-top` (`app.js:27454`).
  pub const FADE_TOP_SCROLL: f32 = css(6.0);
  /// Nombre de titres « Next up » (`slice(startIdx, startIdx + 40)`, `app.js:27418`).
  pub const UPCOMING_SHOWN: usize = 40;
  /// Nombre de titres « Recently played » (`slice(0, 20)`, `app.js:27433`).
  pub const RECENT_SHOWN: usize = 20;

  /// `.sb-lib-section-label` : `11px x --font-scale`, graisse 700, `padding: 10px 10px 4px 10px` (`styles.css:11467-11474`).
  pub const LABEL_TEXT: f32 = css(11.0 * FONT_SCALE);
  pub const LABEL_PADDING_TOP: f32 = css(10.0);
  pub const LABEL_PADDING_X: f32 = css(10.0);
  pub const LABEL_PADDING_BOTTOM: f32 = css(4.0);

  /// `.sb-lib-row` : `min-height: 76px; padding: 8px 10px; gap: 14px; border-radius: 16px` (`styles.css:11480-11483`).
  pub const ROW_HEIGHT: f32 = css(76.0);
  pub const ROW_PADDING_Y: f32 = css(8.0);
  pub const ROW_PADDING_X: f32 = css(10.0);
  pub const ROW_GAP: f32 = css(14.0);
  pub const ROW_RADIUS: f32 = css(16.0);
  /// `.sb-lib-row:hover` : `on-surface` à 8 % (`styles.css:11491`).
  pub const ROW_HOVER_SHARE: f32 = 0.08;
  /// `.sb-lib-cover` : 60 px, `border-radius: 14px` (`styles.css:11498-11504`).
  pub const COVER: f32 = css(60.0);
  pub const COVER_RADIUS: f32 = css(14.0);
  /// Icônes des pochettes sans image : 20 px (titre, album) et 22 px (aimés, fichiers) (`app.js:27288-27370`).
  pub const COVER_ICON: f32 = 20.0;
  pub const COVER_ICON_LARGE: f32 = 22.0;
  /// Rangées de l'onglet Queue. `.sb-lib-row` : `padding: 8px 10px; gap: 14px`, `.sb-lib-cover` : 60 px, `border-radius: 14px`,
  /// `.sb-lib-title` : 15 px, `.sb-lib-sub` : 12,5 px. Écart voulu avec Riff (demandé par Hugo) : mêmes proportions que
  /// le panneau de file (`queue`, pochette x 0,77, marge verticale divisée par 2, textes moins 1 px) : pochette 46 px
  /// et 11 px de rayon, `padding: 4px 10px`, `gap: 12px`, titre 14 px, détail 11,5 px.
  pub const QUEUE_COVER: f32 = css(46.0);
  pub const QUEUE_COVER_RADIUS: f32 = css(11.0);
  pub const QUEUE_ROW_PADDING_Y: f32 = css(4.0);
  pub const QUEUE_ROW_GAP: f32 = css(12.0);
  pub const QUEUE_TITLE_TEXT: f32 = css(14.0 * FONT_SCALE);
  pub const QUEUE_SUB_TEXT: f32 = css(11.5 * FONT_SCALE);
  /// `.sb-lib-cover.liked-grad` : dégradé 135deg de `primary` vers `primary` à 52 % + blanc (`styles.css:11520`).
  pub const LIKED_END_SHARE: f32 = 0.52;
  /// `.sb-lib-cover.local-grad` : `primary` à 22 % dans `surface-container-highest` (`styles.css:11524`).
  pub const LOCAL_SHARE: f32 = 0.22;
  /// `.sb-lib-info` : `gap: 4px` (`styles.css:11528-11536`).
  pub const INFO_GAP: f32 = css(4.0);
  /// `.sb-lib-title` : `15px x --font-scale`, graisse 600, `line-height: 1.25` (`styles.css:11538-11540`).
  pub const TITLE_TEXT: f32 = css(15.0 * FONT_SCALE);
  pub const TITLE_LINE: f32 = 1.25;
  /// `.sb-lib-sub` : `12.5px x --font-scale`, `line-height: 1.2`, `gap: 5px` (`styles.css:11550-11558`).
  pub const SUB_TEXT: f32 = css(12.5 * FONT_SCALE);
  pub const SUB_LINE: f32 = 1.2;
  pub const SUB_GAP: f32 = css(5.0);
  /// `.sb-lib-pin-icon` : 12 px (`styles.css:11561`).
  pub const PIN_ICON: f32 = 12.0;
  /// `.sb-lib-row-action` : 28 px rond, icône de 14 px, visible au survol de la rangée (`styles.css:11567-11585`).
  pub const ACTION: f32 = css(28.0);
  pub const ACTION_ICON: f32 = 14.0;
  /// `.empty-hint` : `13px x --font-scale`, centré, `padding: 28px 12px` en ligne (`app.js:27388`, `27446`, `styles.css:2784`).
  pub const HINT_TEXT: f32 = css(13.0 * FONT_SCALE);
  pub const HINT_PADDING_Y: f32 = css(28.0);
  pub const HINT_PADDING_X: f32 = css(12.0);

  /// `.sb-lib-row { animation: sbRowFadeIn 240ms cubic-bezier(0.2, 0, 0, 1) both }` (`styles.css:11595`).
  pub const ENTER_MS: f32 = 240.0;
  pub const ENTER_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `animation-delay: calc(min(var(--i, 0), 12) * 20ms)` (`styles.css:11596`) : `--i` est le rang de la rangée parmi les `.sb-lib-row` du flux
  /// (`app.js:27452`), les étiquettes de section n'en comptent pas.
  pub const ENTER_STEP_MS: f32 = 20.0;
  pub const ENTER_MAX_RANK: usize = 12;
  /// `@keyframes sbRowFadeIn` : `from { opacity: 0; transform: translateY(8px) scale(0.985) }` vers `translateY(0) scale(1)` (`styles.css:11590-11593`).
  /// Origine par défaut (centre de la rangée), une seule courbe sur les deux images clés.
  pub const ENTER_OFFSET: f32 = css(8.0);
  pub const ENTER_SCALE_FROM: f32 = 0.985;
}

/// Détail de playlist, `#view-playlist-detail` (`index.html:716-762`, `app.js:13352-13450`), mesuré dans
/// `ui-reference/screens/07-playlist-detail.png`. Les boutons et le champ de recherche reprennent `tokens::local`.
pub mod playlist_detail {
  use super::{css, local, playbar::FONT_SCALE};

  /// `#btn-back-playlists` : 40 px, icône de 18 px (`styles.css:9588-9595`), `margin-bottom: 12px` en ligne (`index.html:717`).
  pub const BACK_SIZE: f32 = css(40.0);
  pub const BACK_ICON: f32 = 18.0;
  pub const BACK_MARGIN: f32 = css(12.0);

  /// `.playlist-detail-header` : ligne alignée en bas, `gap: 24px; padding: 28px 24px 20px; margin-bottom: 24px` (`styles.css:5003-5014`).
  pub const HEADER_GAP: f32 = css(24.0);
  pub const HEADER_PADDING_TOP: f32 = css(28.0);
  pub const HEADER_PADDING_X: f32 = css(24.0);
  pub const HEADER_PADDING_BOTTOM: f32 = css(20.0);
  pub const HEADER_MARGIN: f32 = css(24.0);

  /// `.playlist-cover-wrapper` : 160 px, `border-radius: var(--md-shape-corner-medium, 16px)` (variable absente : 16 px) (`styles.css:5059-5073`).
  pub const COVER: f32 = css(160.0);
  pub const COVER_RADIUS: f32 = css(16.0);
  /// `renderPlaylistCover` : icône de repli à `size * 0.45`, soit 72 px, couleur `outline` sur `surface-container-high` (`app.js:12930`, `styles.css:5205-5224`).
  pub const COVER_ICON: f32 = 72.0;

  /// `.playlist-meta` : colonne, `gap: 8px` (`styles.css:5101-5108`).
  pub const META_GAP: f32 = css(8.0);
  /// Hauteur de ligne du texte courant de Google Sans (mesurée : 22,22 px pour 17,2188 px, voir `tokens::home`).
  pub const LINE: f32 = 22.22 / 17.2188;
  /// `.playlist-type-badge` : `11px x --font-scale`, graisse 700, majuscules, `letter-spacing: .08em` (`styles.css:5110-5117`).
  pub const BADGE_TEXT: f32 = css(11.0 * FONT_SCALE);
  pub const BADGE_SPACING: f32 = 0.08;
  pub const BADGE_HEIGHT: f32 = css(16.5);
  /// `#playlist-detail-title` : `clamp(1.8rem, 3vw, 2.8rem)` avec `rem` = 16 px x `--font-scale` (`styles.css:5953`), soit 53,2 px
  /// pour une fenêtre de 2031 px CSS ; `line-height: 1.1`, graisse 700 (`styles.css:5119-5129`).
  pub const TITLE_TEXT: f32 = css(2.8 * 16.0 * FONT_SCALE);
  pub const TITLE_LINE: f32 = 1.1;
  /// Titre et description sont éditables sur place : bordure de 1,5 px, remplissage `4px 10px`, marge négative `-4px -10px`
  /// (`styles.css:9689`). Seuls la bordure et la marge de 8 px au-dessus et au-dessous comptent dans la mise en page.
  pub const EDIT_BORDER: f32 = css(1.5);
  pub const EDIT_PADDING_X: f32 = css(10.0);
  pub const EDIT_PADDING_Y: f32 = css(4.0);
  pub const EDIT_RADIUS: f32 = css(12.0);
  /// `#playlist-detail-desc` : `13px x --font-scale`, `min-height: 24px`, couleur `on-surface-variant` ; texte vide à 45 % (`styles.css:9690-9691`).
  pub const DESC_TEXT: f32 = css(13.0 * FONT_SCALE);
  pub const DESC_MIN: f32 = css(24.0);
  /// Écart mesuré sur 07 entre la mise en page calculée et la capture : la ligne du nombre de titres est 5 px plus haut.
  pub const DESC_TRIM: f32 = css(5.3);
  pub const DESC_EMPTY_SHARE: f32 = 0.45;
  /// Survol d'un champ éditable : `on-surface` à 6 %, bordure `outline` à 35 % (`styles.css:9692`).
  pub const EDIT_HOVER_SHARE: f32 = 0.06;
  pub const EDIT_HOVER_BORDER_SHARE: f32 = 0.35;
  /// `#playlist-detail-count` : `14px x --font-scale` ; hauteur mesurée sur 07 (15,9 px écran).
  pub const COUNT_TEXT: f32 = css(14.0 * FONT_SCALE);
  pub const COUNT_HEIGHT: f32 = css(16.8);

  /// `.playlist-actions` : `gap: 12px; margin-top: 12px` (`styles.css:5135-5142`) ; la ligne mesure 44 px (corbeille).
  pub const ACTIONS_GAP: f32 = css(12.0);
  pub const ACTIONS_MARGIN: f32 = css(12.0);
  pub const ACTIONS_HEIGHT: f32 = css(44.0);
  /// Écart mesuré sur 07 : la ligne de boutons est 2 px plus bas que la mise en page calculée.
  pub const ACTIONS_EXTRA: f32 = css(1.9);
  /// `#btn-play-playlist-shuffle` : `height: 40px; padding: 0 22px` en ligne (`index.html:737`), icône de 16 px.
  pub const PLAY_PADDING_X: f32 = css(22.0);
  pub const PLAY_ICON: f32 = 16.0;
  /// `.btn-outline-pill svg` : 18 px (`index.html:741`).
  pub const PILL_ICON: f32 = 18.0;
  /// `.playlist-actions #btn-delete-playlist` : 44 px, rond (`styles.css:5169-5192`).
  pub const DELETE_SIZE: f32 = css(44.0);
  pub const DELETE_ICON: f32 = 18.0;

  /// `#playlist-sort-dropdown` : 210 px (`index.html:722`) ; `.m3-dropdown-trigger` : `padding: 10px 14px; gap: 8px;
  /// border-radius: 12px; 14px x --font-scale`, graisse 500 ; hauteur mesurée 39,7 px CSS (`styles.css:4810-4851`).
  pub const SORT_WIDTH: f32 = css(210.0);
  pub const SORT_HEIGHT: f32 = css(40.0);
  pub const SORT_PADDING_X: f32 = css(14.0);
  pub const SORT_GAP: f32 = css(8.0);
  pub const SORT_RADIUS: f32 = css(12.0);
  /// `.m3-dropdown.open .m3-dropdown-trigger` : coins du bas à 6 px (`styles.css:7499`).
  pub const SORT_OPEN_RADIUS: f32 = css(6.0);
  pub const SORT_TEXT: f32 = css(14.0 * FONT_SCALE);
  pub const SORT_ARROW: f32 = 18.0;
  /// `.m3-dropdown-menu` : `top: calc(100% + 2px)`, `padding: 6px`, rayon 12 px, fond `surface-container-high` à 76 % (`styles.css:7493-7497`, `11020`).
  pub const MENU_GAP: f32 = css(2.0);
  pub const MENU_PADDING: f32 = css(6.0);
  pub const MENU_RADIUS: f32 = css(12.0);
  pub const MENU_SHARE: f32 = 0.76;
  /// `.m3-dropdown-option` : `min-height: 40px; padding: 9px 12px; border-radius: 12px` ; survol `primary` à 18 % (`styles.css:4916-4945`, `7500`).
  pub const OPTION_HEIGHT: f32 = css(40.0);
  pub const OPTION_PADDING_X: f32 = css(12.0);
  pub const OPTION_RADIUS: f32 = css(12.0);
  pub const OPTION_HOVER_SHARE: f32 = 0.18;

  /// Hauteur de la colonne de texte : badge, titre, description (27 px de mise en page), nombre de titres, puis la ligne d'actions.
  pub const META_HEIGHT: f32 = BADGE_HEIGHT
    + META_GAP
    + (TITLE_TEXT * TITLE_LINE + 2.0 * EDIT_BORDER)
    + META_GAP
    + (DESC_MIN - DESC_TRIM + 2.0 * EDIT_BORDER)
    + META_GAP
    + COUNT_HEIGHT
    + META_GAP
    + ACTIONS_MARGIN
    + ACTIONS_EXTRA
    + ACTIONS_HEIGHT;
  /// Contenu de l'en-tête : la plus haute de la pochette et de la colonne de texte.
  pub const CONTENT_HEIGHT: f32 = if META_HEIGHT > COVER {
    META_HEIGHT
  } else {
    COVER
  };
  pub const HEADER_HEIGHT: f32 = HEADER_PADDING_TOP + CONTENT_HEIGHT + HEADER_PADDING_BOTTOM;
  /// Champ de recherche : `.view-search-box`, comme `tokens::local`.
  pub const SEARCH_HEIGHT: f32 = local::SEARCH_HEIGHT;
  pub const SEARCH_MARGIN: f32 = local::SEARCH_MARGIN;

  /// `.hero-fx` du détail (`#playlist-hero-fx`) : sans `clip_fx` (défaut), l'élément est déplacé dans `#fx-root` en
  /// `position: fixed; left: 0; right: 0; top: 0; height: 70vh` (`app.js:1642-1656`, `16111`, `styles.css:8403`).
  pub const HERO_HEIGHT_SHARE: f32 = 0.70;
  /// `transform: scale(1.3)` depuis le haut au centre, `filter: blur(70px) saturate(1.5) brightness(0.5)` (`styles.css:5017-5031`).
  pub const HERO_SCALE: f32 = 1.3;
  pub const HERO_BLUR: f32 = css(70.0) * HERO_SCALE;
  pub const HERO_SATURATE_PERCENT: u32 = 150;
  pub const HERO_BRIGHTNESS: f32 = 0.5;
  /// `mask-image: linear-gradient(to bottom, #000 0%, #000 45%, transparent 100%)` (`styles.css:5028`).
  pub const HERO_MASK_OPAQUE: f32 = 0.45;
}

/// Détail d'album, `#view-album` de Riff.
pub mod album_detail {
  use super::{css, playbar::FONT_SCALE};

  /// `#btn-back-album` : 40 px rond, icône de 18 px CSS, `margin-bottom: 16px` (`styles.css:9588-9595`, `index.html:772`).
  pub const BACK_SIZE: f32 = css(40.0);
  pub const BACK_ICON: f32 = 18.0;
  pub const BACK_MARGIN: f32 = css(16.0);
  /// `.main-content:has(#view-album.active)` : `padding-bottom: 48px` au lieu de 24 px (`styles.css:8495-8499`).
  pub const PAGE_BOTTOM_EXTRA: f32 = css(48.0 - 24.0);

  /// `.album-layout` : `padding: 8px 24px 32px`, colonnes `minmax(260px, 320px) minmax(0, 1fr)`, `gap: 44px`
  /// (`styles.css:8527-8534`). Mesuré : 302 px écran (320 px CSS) sur `08-album.png`.
  pub const LAYOUT_PADDING_TOP: f32 = css(8.0);
  pub const LAYOUT_PADDING_X: f32 = css(24.0);
  pub const LAYOUT_PADDING_BOTTOM: f32 = css(32.0);
  pub const SIDE_WIDTH: f32 = css(320.0);
  pub const SIDE_MIN_WIDTH: f32 = css(260.0);
  pub const LAYOUT_GAP: f32 = css(44.0);

  /// `.album-side` : `gap: 14px; padding: 20px; border-radius: 24px`, fond `surface-container` à 72 %
  /// (`styles.css:8535-8548`). Le `backdrop-filter: blur(20px)` n'a pas d'équivalent : le fond derrière est déjà flou.
  pub const SIDE_GAP: f32 = css(14.0);
  pub const SIDE_PADDING: f32 = css(20.0);
  pub const SIDE_RADIUS: f32 = css(24.0);
  pub const SIDE_SHARE: f32 = 0.72;

  /// Hauteur mesurée de la colonne (460 px écran sur `08-album.png`, titre sur deux lignes) : sert seulement à savoir si la
  /// page déborde et fait apparaître la barre de défilement.
  pub const SIDE_HEIGHT_ESTIMATE: f32 = css(487.0);

  /// `#album-detail-cover img` : `border-radius: 20px` (`styles.css:8560-8567`), comme l'aplat et la mosaïque
  /// `.sp-grid-cover` (`gap: 3px; padding: 3px`, images de 8 px, `app.js:9594-9604`).
  pub const COVER_RADIUS: f32 = css(20.0);
  pub const COVER_GRID_GAP: f32 = css(3.0);
  pub const COVER_GRID_RADIUS: f32 = css(8.0);

  /// `#album-detail-title` : `clamp(26px, 2.6vw, 36px)`, graisse 700, `line-height: 1.15`, 3 lignes au plus
  /// (`styles.css:8569-8583`). `vw` est la largeur CSS du viewport, soit la largeur de la fenêtre divisée par le zoom.
  pub const TITLE_LINE: f32 = 1.15;
  pub const TITLE_MAX_LINES: usize = 3;
  #[must_use]
  pub fn title_size(window_width: f32) -> f32 {
    css((window_width / super::ZOOM * 0.026).clamp(26.0, 36.0))
  }

  /// `#album-detail-artist` : 18 px, graisse 600, `primary` (`styles.css:8584-8600`).
  pub const ARTIST_TEXT: f32 = css(18.0);
  /// `#album-detail-count` : 14 px, `line-height: 1.4`, `on-surface-variant` (`styles.css:8606-8612`).
  pub const META_TEXT: f32 = css(14.0);
  pub const META_LINE: f32 = 1.4;

  /// `#btn-save-album` : `.btn-icon-pill`, 36 px, icône de 18 px CSS (`index.html:778`, `styles.css:3285`).
  pub const SAVE_SIZE: f32 = css(36.0);
  pub const SAVE_ICON: f32 = 18.0;

  /// `.daily-mix-feedback` : `margin-top: 6px; padding: 8px 14px; gap: 12px`, pastille `surface-container`
  /// (`styles.css:9640`). Libellé de `13px x --font-scale`, graisse 500 ; boutons de 32 px, `gap: 6px`, icônes de 16 px
  /// (`styles.css:9643-9646`, `index.html:779`).
  pub const MIX_MARGIN_TOP: f32 = css(6.0);
  pub const MIX_PADDING_Y: f32 = css(8.0);
  pub const MIX_PADDING_X: f32 = css(14.0);
  pub const MIX_GAP: f32 = css(12.0);
  pub const MIX_LABEL_TEXT: f32 = css(13.0 * FONT_SCALE);
  pub const MIX_BUTTONS_GAP: f32 = css(6.0);
  pub const MIX_BUTTON: f32 = css(32.0);
  pub const MIX_ICON: f32 = 16.0;

  /// `#album-tracks-list .track-row` : `border-radius: 20px`, survol `surface-container-high` (`styles.css:8650-8656`).
  /// Les vignettes sont masquées (`styles.css:8656-8658`).
  pub const ROW_RADIUS: f32 = css(20.0);
  /// Sans vignette de 44 px, la hauteur d'une ligne vient du bloc titre et artiste : 61 px CSS de boîte (pas de 65 px CSS
  /// avec le `gap` de 4 px), mesuré sur `08-album.png` (lignes à 61,4 px d'écart).
  pub const ROW_HEIGHT: f32 = css(61.0);

  /// Chargement : `.m3-loader-container` en ligne, `padding: 64px 0`, `.g-spinner` de 40 px (`app.js:9612`, `styles.css:6248`).
  pub const LOADER_PADDING_Y: f32 = css(64.0);
  pub const LOADER_SIZE: f32 = css(40.0);
  /// Erreur : `.empty-hint` (`13px x --font-scale`, `padding: 32px 0`, `styles.css:2784`) en couleur `error` (`app.js:9694`).
  pub const HINT_TEXT: f32 = css(13.0 * FONT_SCALE);
  pub const HINT_PADDING_Y: f32 = css(32.0);
}

/// Page artiste `#view-artist` (`index.html:790-857`, `app.js:11885-12196`), mesurée dans
/// `ui-reference/screens/09-artiste.png` (1920 x 1080 px écran, zoom 0,945) et `styles.css:1779-2135`,
/// `9588-9596`, `9661`, `10464`.
pub mod artist {
  use super::{css, playbar::FONT_SCALE, track_row};

  /// `.artist-hero` : `height: clamp(340px, 46vh, 480px)` (`styles.css:1803`). `46vh` est en pixels écran de la fenêtre.
  pub const HERO_MIN: f32 = css(340.0);
  pub const HERO_MAX: f32 = css(480.0);
  pub const HERO_VH: f32 = 0.46;
  /// `.artist-hero-img` : `brightness(.8)`, `object-position: center 25%` (`styles.css:1821-1833`).
  pub const HERO_BRIGHTNESS: f32 = 0.8;
  pub const HERO_FOCUS_Y: f32 = 0.25;
  /// Masque de l'image : opaque en haut, `0,85` à 60 %, transparent en bas (`styles.css:1830`).
  pub const HERO_MASK_MID_AT: f32 = 0.6;
  pub const HERO_MASK_MID: f32 = 0.85;
  /// `.artist-hero::after` : noir à 25 % en haut, 45 % à mi-hauteur, `surface-container-low` en bas (`styles.css:1810-1820`).
  pub const HERO_SHADE_TOP: f32 = 0.25;
  pub const HERO_SHADE_MID: f32 = 0.45;

  /// `.btn-back-artist` : `left: 24px; top: 20px`, 40 px, rond, noir à 55 %, icône de 18 px (`styles.css:1834`, `9588-9596`).
  pub const BACK_LEFT: f32 = css(24.0);
  pub const BACK_TOP: f32 = css(20.0);
  pub const BACK_SIZE: f32 = css(40.0);
  pub const BACK_ICON: f32 = 18.0;
  pub const BACK_FILL: f32 = 0.55;
  /// `.artist-hero-info` : `left: 32px; bottom: 32px` (`styles.css:1860`).
  pub const INFO_LEFT: f32 = css(32.0);
  pub const INFO_BOTTOM: f32 = css(32.0);

  /// `.artist-action-buttons` : `gap: 8px; margin-bottom: 12px` ; boutons de 36 px, `padding: 0 16px`, `13px` (`index.html:799`).
  pub const ACTIONS_GAP: f32 = css(8.0);
  pub const ACTIONS_MARGIN: f32 = css(12.0);
  pub const ACTION_HEIGHT: f32 = css(36.0);
  pub const ACTION_PADDING_X: f32 = css(16.0);
  pub const ACTION_TEXT: f32 = css(13.0);
  /// Couleur de « Block » et fond de « Blocked » à 15 % (`index.html:801`, `styles.css:10464`).
  pub const BLOCK_RED: [u8; 3] = [0xff, 0xb4, 0xab];
  pub const BLOCKED_SHARE: f32 = 0.15;

  /// `.artist-title` : `clamp(48px, 6vw, 72px)`, graisse 800 (Google Sans 800 n'est pas embarquée : 700), `line-height: 1`.
  pub const TITLE_MIN: f32 = css(48.0);
  pub const TITLE_MAX: f32 = css(72.0);
  pub const TITLE_VW: f32 = 0.06;
  pub const TITLE_LINE: f32 = 1.0;
  /// `.artist-title-wrap` : `gap: 12px` ; pastille `.artist-m3-cookie-badge` : 32 px, `margin-left: 4px` (`styles.css:1884`).
  pub const TITLE_GAP: f32 = css(12.0);
  pub const BADGE: f32 = css(32.0);
  pub const BADGE_MARGIN: f32 = css(4.0);
  /// Forme `m3-cookie-8` : rayon `42 + 4 cos(8a)` en centièmes du côté (`app.js:366`), tour en 14 s (`styles.css:1899`).
  pub const BADGE_RADIUS: f32 = 0.42;
  pub const BADGE_WAVE: f32 = 0.04;
  pub const BADGE_TURN_SECS: f32 = 14.0;
  /// Coche : SVG de 16 px, trait de 3,5 sur 24 (`index.html:812`).
  pub const BADGE_CHECK: f32 = css(16.0);
  pub const BADGE_CHECK_STROKE: f32 = 3.5 / 24.0;
  /// `.artist-fans-line` : `margin-top: 8px; font-size: 14px` (`styles.css:1919`).
  pub const FANS_MARGIN: f32 = css(8.0);
  pub const FANS_TEXT: f32 = css(14.0);

  /// `.artist-horiz-tabs` : `gap: 16px; padding: 14px 32px 0`, bordure basse de 1 px (`outline-variant` à 55 %), `margin-bottom: 4px` (`styles.css:2083`).
  pub const TABS_GAP: f32 = css(16.0);
  pub const TABS_PADDING_TOP: f32 = css(14.0);
  pub const TABS_BORDER_SHARE: f32 = 0.55;
  pub const TABS_MARGIN: f32 = css(4.0);
  /// `.artist-horiz-tab` : `min-width: 168px; padding: 12px 32px 13px; gap: 6px`, coins hauts de 14 px, icône de 20 px,
  /// texte `13px x --font-scale` (500, 600 actif) (`styles.css:2088-2106`).
  pub const TAB_MIN_WIDTH: f32 = css(168.0);
  pub const TAB_PADDING_TOP: f32 = css(12.0);
  pub const TAB_PADDING_BOTTOM: f32 = css(13.0);
  pub const TAB_PADDING_X: f32 = css(32.0);
  pub const TAB_GAP: f32 = css(6.0);
  pub const TAB_RADIUS: f32 = css(14.0);
  pub const TAB_ICON: f32 = 20.0;
  pub const TAB_TEXT: f32 = css(13.0 * FONT_SCALE);
  /// Survol d'un onglet inactif : `on-surface` à 5 % ; actif : 10 % (`styles.css:2107-2108`).
  pub const TAB_HOVER_SHARE: f32 = 0.05;
  pub const TAB_ACTIVE_SHARE: f32 = 0.10;
  /// Trait de l'onglet actif : 42 x 3 px, `on-surface` (`styles.css:2109-2125`).
  pub const TAB_BAR_WIDTH: f32 = css(42.0);
  pub const TAB_BAR_HEIGHT: f32 = css(3.0);
  /// `.artist-content-section` : `gap: 12px` (`styles.css:1994`).
  pub const CONTENT_GAP: f32 = css(12.0);

  /// `.artist-tracks-header` : `padding: 24px 32px 12px` ; titre 22 px (700), bouton 14 px (600), `padding: 4px 8px` (`styles.css:2001-2025`).
  pub const HEADER_PADDING_TOP: f32 = css(24.0);
  pub const HEADER_PADDING_X: f32 = css(32.0);
  pub const HEADER_PADDING_BOTTOM: f32 = css(12.0);
  pub const TOGGLE_TEXT: f32 = css(14.0);
  pub const TOGGLE_PADDING_Y: f32 = css(4.0);
  pub const TOGGLE_PADDING_X: f32 = css(8.0);

  /// `#view-artist .tracks-list` : `padding: 0 16px` ; `.track-row` : `height: 72px; padding: 8px 16px; gap: 16px`.
  /// Le rayon reste 10 px : `.track-row { border-radius: 10px !important }` l'emporte (`styles.css:1583`, `2030-2040`).
  pub const LIST_PADDING_X: f32 = css(16.0);
  pub const ROW_HEIGHT: f32 = css(72.0);
  pub const ROW_PADDING_Y: f32 = css(8.0);
  pub const ROW_PADDING_X: f32 = css(16.0);
  pub const ROW_GAP: f32 = css(16.0);
  /// Vignette de 56 px à coins de 8 px ; titre 16 px (500) ; artiste et durée 14 px (`styles.css:2048-2068`).
  pub const THUMB: f32 = css(56.0);
  pub const THUMB_RADIUS: f32 = css(8.0);
  pub const TITLE_TEXT_ROW: f32 = css(16.0);
  pub const ARTIST_TEXT_ROW: f32 = css(14.0);
  pub const DURATION_TEXT_ROW: f32 = css(14.0);
  /// Pas d'une ligne : boîte et `gap: 4px` de `.tracks-list`.
  pub const ROW_PITCH: f32 = ROW_HEIGHT + track_row::LIST_GAP;
  /// `#view-artist` : `padding-bottom: 40px`, puis `.main-content` : `padding-bottom: 24px` (`styles.css:1791`, `layout.json`).
  pub const PAGE_BOTTOM: f32 = css(40.0 + 24.0);

  /// `.artist-albums-grid` : `repeat(auto-fill, minmax(156px, 1fr)); gap: 20px; padding: 8px 32px 32px` (`styles.css:2130`).
  pub const GRID_MIN: f32 = css(156.0);
  pub const GRID_GAP: f32 = css(20.0);
  pub const GRID_PADDING_TOP: f32 = css(8.0);
  pub const GRID_PADDING_X: f32 = css(32.0);
  pub const GRID_PADDING_BOTTOM: f32 = css(32.0);

  /// `renderM3Loader` : canvas de 52 px, `padding: 48px 0`, forme à 72 % du côté, une forme toutes les 1,2 s, tour en 4 s
  /// (`app.js:11846-11880`).
  pub const LOADER_SIZE: f32 = css(52.0);
  pub const LOADER_PADDING_Y: f32 = css(48.0);
  pub const LOADER_SHAPE: f32 = 0.72;
  pub const LOADER_STEP_SECS: f32 = 1.2;
  pub const LOADER_TURN_SECS: f32 = 4.0;
  /// Un profil radial de `visuals::shapes` tient dans 270 unités de diamètre (`shapes.js`, `samplePathToRadialProfile`).
  pub const LOADER_PROFILE_UNITS: f32 = 270.0;

  /// `.artist-tab-pane:not(.hidden)` : `artistPaneSlideIn 260ms cubic-bezier(0.2, 0, 0, 1) both` (`styles.css:2127-2129`), de
  /// `opacity: 0; transform: translateY(10px) scale(0.99)` vers l'identité (`styles.css:2083-2086`) ; `transform-origin` par défaut :
  /// le centre du panneau. Une seule paire d'images clés : la courbe vaut pour tout le trajet.
  pub const PANE_MS: f32 = 260.0;
  pub const PANE_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  pub const PANE_OFFSET: f32 = css(10.0);
  pub const PANE_SCALE: f32 = 0.99;

  /// Hauteur de l'image de bannière pour une fenêtre de `window_height` px écran.
  #[must_use]
  pub fn hero_height(window_height: f32) -> f32 {
    (HERO_VH * window_height).clamp(HERO_MIN, HERO_MAX)
  }

  /// Taille du nom pour une fenêtre de `window_width` px écran.
  #[must_use]
  pub fn title_size(window_width: f32) -> f32 {
    (TITLE_VW * window_width).clamp(TITLE_MIN, TITLE_MAX)
  }
}

/// Vue « Historique » `#view-history` (`index.html:696-704`, `app.js:12462-12465`), mesurée dans
/// `ui-reference/screens/10-historique.png`. Le bouton reprend `tokens::local` (`.btn-outline-pill`).
pub mod history_view {
  use super::local;

  /// `.view-header` : le titre `h2` est masqué (`styles.css:1483-1488`), seul `#btn-clear-history`
  /// (`.btn-outline-pill`, `height: 40px`, `styles.css:3323-3335`) reste, aligné à droite (`styles.css:1490-1493`).
  pub const HEADER_HEIGHT: f32 = local::BUTTON_HEIGHT;
  /// `.view-header` : `margin-bottom: 16px; padding-bottom: 0` (`styles.css:1490-1493`).
  pub const HEADER_MARGIN: f32 = local::HEADER_MARGIN;
}

/// File d'attente, vue `#view-queue` (`index.html:706-715`) et panneau `#np-queue-panel` (`app.js:26978-27144`).
/// Valeurs CSS en pixels CSS (via [`css`]) ; mesures de contrôle sur `ui-reference/screens/11a`, `11b`.
pub mod queue {
  use super::{css, local, playbar::FONT_SCALE};

  /// `.view-header` de `#view-queue` : titre masqué, `#btn-clear-queue` (`.btn-outline-pill`, 40 px) à droite,
  /// `margin-bottom: 16px; padding-bottom: 0` (`styles.css:1483-1493`, `3323-3335`).
  pub const HEADER_HEIGHT: f32 = local::BUTTON_HEIGHT;
  pub const HEADER_MARGIN: f32 = local::HEADER_MARGIN;

  /// `.np-queue-panel` : `top: 72px; right: 16px; width: 368px; border-radius: 24px` (`styles.css:10908-10927`).
  pub const PANEL_WIDTH: f32 = css(368.0);
  pub const PANEL_TOP: f32 = css(72.0);
  pub const PANEL_RIGHT: f32 = css(16.0);
  /// `bottom: calc(var(--playbar-space) + 8px)` avec `--playbar-space: 130px` (`styles.css:7224`, `10912`).
  pub const PANEL_BOTTOM: f32 = css(130.0 + 8.0);
  pub const PANEL_RADIUS: f32 = css(24.0);

  /// `.np-queue-head` : `gap: 8px; padding: 12px 14px 0 14px` (`styles.css:10942-10950`).
  pub const HEAD_GAP: f32 = css(8.0);
  pub const HEAD_PADDING_TOP: f32 = css(12.0);
  pub const HEAD_PADDING_X: f32 = css(14.0);
  /// `.np-queue-close` : 32 x 32 px, `margin-bottom: 6px`, SVG de 16 px (`styles.css:10954-10962`, `app.js:26995`).
  pub const CLOSE_SIZE: f32 = css(32.0);
  pub const CLOSE_MARGIN_BOTTOM: f32 = css(6.0);
  pub const CLOSE_ICON: f32 = 16.0;
  /// `.np-queue-close:hover` : `on-surface` à 10 % (`styles.css:10963`).
  pub const CLOSE_HOVER_SHARE: f32 = 0.10;
  /// Deux filets de 1 px (`border-bottom` de `.np-queue-tabs` puis de `.np-queue-head`), `outline-variant`
  /// à 55 % : 2 px mesurés sur 11b (`styles.css:10945`, `11380-11389`).
  pub const RULE_SHARE: f32 = 0.55;
  pub const RULE_COUNT: u16 = 2;

  /// `.np-queue-body` : `padding: 10px 12px 16px 12px; gap: 2px` (`styles.css:10964`).
  pub const BODY_PADDING_TOP: f32 = css(10.0);
  pub const BODY_PADDING_X: f32 = css(12.0);
  pub const BODY_PADDING_BOTTOM: f32 = css(16.0);
  pub const BODY_GAP: f32 = css(2.0);

  /// `.np-queue-heading` : `700`, `12.5px x --font-scale`, `padding: 12px 10px 6px` (4 px en haut pour le premier)
  /// (`styles.css:10965-10966`). Interligne déduite de 11b : 20 px CSS (écart entre « Next up » et la ligne suivante).
  pub const HEADING_TEXT: f32 = css(12.5 * FONT_SCALE);
  pub const HEADING_LINE: f32 = css(20.0);
  pub const HEADING_PADDING_TOP: f32 = css(12.0);
  pub const HEADING_PADDING_TOP_FIRST: f32 = css(4.0);
  pub const HEADING_PADDING_X: f32 = css(10.0);
  pub const HEADING_PADDING_BOTTOM: f32 = css(6.0);

  /// `.np-queue-row` : `gap: 12px; padding: 8px 10px; border-radius: 12px` (`styles.css:10967`). Écart voulu avec Riff
  /// (demandé par Hugo) : rangées plus compactes, `gap: 10px; padding: 4px 10px`.
  pub const ROW_GAP: f32 = css(10.0);
  pub const ROW_PADDING_Y: f32 = css(4.0);
  pub const ROW_PADDING_X: f32 = css(10.0);
  pub const ROW_RADIUS: f32 = css(12.0);
  /// `:hover` : `on-surface` à 7 % ; `.playing` : `primary` à 14 % (`styles.css:10968-10969`).
  pub const ROW_HOVER_SHARE: f32 = 0.07;
  pub const ROW_PLAYING_SHARE: f32 = 0.14;
  /// `.np-queue-art` : 44 px, `border-radius: 10px` (`styles.css:10971`). Écart voulu avec Riff (demandé par Hugo) : 34 px, 8 px.
  pub const ART: f32 = css(34.0);
  pub const ART_RADIUS: f32 = css(8.0);
  /// `.np-queue-info` : `gap: 2px` ; titre `13.5px x --font-scale` 500 (600 en lecture), artiste `12px x --font-scale`
  /// (`styles.css:10973-10975`). Interlignes déduites de 11b (hauteur de rangée 60 px CSS). Écart voulu avec Riff (demandé
  /// par Hugo) : 12,5 px et 11 px.
  pub const INFO_GAP: f32 = css(2.0);
  pub const TITLE_TEXT: f32 = css(12.5 * FONT_SCALE);
  pub const ARTIST_TEXT: f32 = css(11.0 * FONT_SCALE);
  pub const TEXT_LINE: f32 = 1.3;
  /// Largeur du bloc de texte : panneau moins `.np-queue-body` (2 x 12), `.np-queue-row` (2 x 10), pochette et `gap`.
  pub const INFO_WIDTH: f32 = css(368.0 - 24.0 - 20.0 - 34.0 - 10.0);

  /// `.np-queue-empty` : `padding: 36px 16px; font-size: 13px x --font-scale; color: outline` (`styles.css:10976`).
  pub const EMPTY_PADDING_Y: f32 = css(36.0);
  pub const EMPTY_PADDING_X: f32 = css(16.0);
  pub const EMPTY_TEXT: f32 = css(13.0 * FONT_SCALE);

  /// `.np-queue-panel` : `transition: opacity 220ms ease, transform 260ms cubic-bezier(0.2, 0, 0, 1)` dans les deux sens
  /// (`styles.css:10927`, `10934-10940`). Mesuré sur `06-file-attente` : fin de l'opacité à 220 ms, du transform à 260 ms.
  pub const OPACITY_MS: f32 = 220.0;
  /// `ease` : `cubic-bezier(0.25, 0.1, 0.25, 1)`.
  pub const OPACITY_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
  pub const SHAPE_MS: f32 = 260.0;
  pub const SHAPE_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// Repos fermé : `opacity: 0; transform: translateX(16px) scale(0.98)` (`styles.css:10925-10926`), origine au centre
  /// (`transform-origin` non posé ; `rect` de `samples.json` : 1667,45 = 1647,77 + 16 + 368 x 0,01).
  pub const CLOSED_OFFSET_X: f32 = css(16.0);
  pub const CLOSED_SCALE: f32 = 0.98;
}

/// Menu contextuel d'un titre, `.riff-ctx-menu` de Riff.
pub mod context_menu {
  use super::{Color, css};

  /// `.riff-ctx-menu` : `min-width: 216px` (`styles.css:10018`).
  pub const MIN_WIDTH: f32 = css(216.0);
  /// `padding: 8px` (`styles.css:10019`).
  pub const PADDING: f32 = css(8.0);
  /// `border-radius: 20px` (`styles.css:10020`).
  pub const RADIUS: f32 = css(20.0);
  /// `gap: 2px` (`styles.css:10028`).
  pub const GAP: f32 = css(2.0);
  /// Marge de `openTrackContextMenu` avec le bord de la fenêtre : 12 px au bord droit et bas, 8 px au
  /// minimum en haut et à gauche (`app.js:10891-10894`).
  pub const EDGE_FLIP: f32 = css(12.0);
  pub const EDGE_MIN: f32 = css(8.0);
  /// `.riff-ctx-item` : `padding: 10px 14px`, `gap: 13px`, `border-radius: 13px` (`styles.css:10051-10059`).
  /// Hauteur mesurée sur 14b : 40 px CSS (pas de 42 px entre deux entrées, `gap` compris).
  pub const ITEM_HEIGHT: f32 = css(40.0);
  pub const ITEM_PADDING_X: f32 = css(14.0);
  pub const ITEM_GAP: f32 = css(13.0);
  pub const ITEM_RADIUS: f32 = css(13.0);
  /// `font-size: calc(14px * var(--font-scale))` avec `--font-scale` 1,1875 (`styles.css:10061`), graisse 500.
  pub const ITEM_TEXT: f32 = css(14.0 * 1.1875);
  /// `.riff-ctx-item svg` : 20 px (`styles.css:10067-10068`).
  pub const ICON_SIZE: f32 = css(20.0);
  /// `.riff-ctx-divider` : `height: 1px; margin: 4px 10px` (`styles.css:10107-10110`).
  pub const DIVIDER_HEIGHT: f32 = css(1.0);
  pub const DIVIDER_MARGIN_Y: f32 = css(4.0);
  pub const DIVIDER_MARGIN_X: f32 = css(10.0);
  /// Part de `--md-sys-color-outline-variant` dans le séparateur (40 %, `styles.css:10110`).
  pub const DIVIDER_SHARE: f32 = 0.4;
  /// Part de `--md-sys-color-on-surface` dans le fond de survol (10 %, `styles.css:10075`).
  pub const HOVER_SHARE: f32 = 0.10;
  /// `.riff-ctx-item.danger:hover` : fond `rgba(255, 84, 73, 0.16)`, texte et icône `#ffb4ab`
  /// (`styles.css:10088-10094`).
  pub const DANGER_BACKGROUND: Color = Color {
    r: 1.0,
    g: 84.0 / 255.0,
    b: 73.0 / 255.0,
    a: 0.16,
  };
  pub const DANGER_TEXT: Color = Color::from_rgb8(0xff, 0xb4, 0xab);
  /// `opacity` à l'ouverture comme à la fermeture : `transition: opacity 180ms ease !important` (`styles.css:10035`), sans
  /// `transform`. Fermeture animée aussi : `closeTrackContextMenu` retire seulement `.open` (`app.js:10659-10661`). Mesuré sur
  /// `05-menu-contextuel` : opacité 0,0294 à 286 ms (départ à 277,7 ms) puis 0,484 à 1457 ms (départ de la fermeture à 1402,8 ms),
  /// écart quadratique 1e-4 sur 180 ms.
  pub const FADE_MS: f32 = 180.0;
  /// `ease` : `cubic-bezier(0.25, 0.1, 0.25, 1)`.
  pub const FADE_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
}

/// Lecture plein écran karaoké, `#tv-karaoke` de Riff.
pub mod karaoke {
  use super::css;

  /// `.tv-karaoke` : `background: #0a0a0e !important` (`styles.css:6297-6299`), sous le fond ambiant. L'aplat ambiant est opaque et
  /// couvre tout l'écran : cette couleur n'est jamais visible et n'est pas dessinée (un second aplat opaque sous celui-là
  /// multiplierait le fondu de `tvk-in` et `tvk-out`).
  pub const BASE: [u8; 3] = [0x0a, 0x0a, 0x0e];
  /// Part de l'opacité d'un orbe présente en un pixel typique de l'écran (les orbes sont des disques flous qui ne couvrent pas tout) :
  /// règle l'opacité des calques pendant `tvk-in` et `tvk-out`, voir `visuals::orbs::GroupOpacity`. Ajustée sur la scène 52a.
  pub const ORB_PRESENCE: f32 = 0.5;
  /// Opacité typique de la vignette `radial-gradient(circle at 50% 45%, rgba(0,0,0,0.14), rgba(0,0,0,0.48))` (`styles.css:6319`).
  /// Constante ajustée sur la mesure de la scène 52a (comme `ORB_PRESENCE`) : ce n'est pas la valeur à mi-rayon du dégradé (0,31).
  pub const VIGNETTE_TYPICAL: f32 = 0.2;
  /// `.tv-cover` : `border-radius: 28px` (`styles.css:6486`).
  pub const COVER_RADIUS: f32 = css(28.0);
  /// `.tv-cover` : `box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5)` (`styles.css:6487`).
  pub const COVER_SHADOW_OFFSET: f32 = css(20.0);
  pub const COVER_SHADOW_BLUR: f32 = css(60.0);
  pub const COVER_SHADOW_ALPHA: f32 = 0.5;
  /// `.tv-karaoke-meta` : `opacity: 0.6` au repos avec les paroles (`styles.css:6500`, `6835`).
  pub const IDLE_META_OPACITY: f32 = 0.6;
  /// `.tv-karaoke-title` : `clamp(20px, 2.4vw, 30px)`, `font-weight: 700`, `line-height: 1.25` (`styles.css:6503-6511`).
  pub const TITLE_LINE_HEIGHT: f32 = 1.25;
  /// `.tv-karaoke-artist` : `clamp(14px, 1.6vw, 18px)`, `rgba(255,255,255,0.75)`, `line-height: 1.3` (`styles.css:6512-6521`).
  pub const ARTIST_LINE_HEIGHT: f32 = 1.3;
  pub const ARTIST_ALPHA: f32 = 0.75;
  /// `.tv-karaoke-meta` : `gap: 8px` (`styles.css:6495`).
  pub const META_GAP: f32 = css(8.0);
  /// `.tv-side` : `gap: 24px` (`styles.css:6480`) ; `16px` sans paroles ou en mode pochette (`6618`, `6665`).
  pub const SIDE_GAP: f32 = 24.0;
  pub const CENTERED_GAP: f32 = 16.0;
  /// `.tv-nolyrics` et `.tv-cover-mode` : titre `28px`, artiste `18px` à 70 %, largeur `min(600px, 90vw)`
  /// (`styles.css:6640-6648`, `6681-6688`).
  pub const CENTERED_TITLE: f32 = 28.0;
  pub const CENTERED_ARTIST: f32 = 18.0;
  pub const CENTERED_ARTIST_ALPHA: f32 = 0.7;
  pub const CENTERED_META_WIDTH: f32 = css(600.0);
  /// `.tvk-output` : `font-size: 13px`, `gap: 8px`, `opacity: 0.8` (`styles.css:6777-6792`).
  pub const OUTPUT_SIZE: f32 = css(13.0);
  pub const OUTPUT_GAP: f32 = css(8.0);
  pub const OUTPUT_ALPHA: f32 = 0.8;
  /// `#tvk-output-name` : `mask-image` sur les 28 derniers pixels, soit environ quatre caractères (`styles.css:6798-6807`).
  pub const OUTPUT_FADE_CHARS: usize = 4;
  /// `.tvk-controls .control-btn` : 44 px, `rgba(255,255,255,0.16)`, survol 0,28, appui 0,36 (`styles.css:6710-6735`).
  pub const SKIP_SIZE: f32 = css(44.0);
  pub const SKIP_ALPHA: f32 = 0.16;
  pub const SKIP_HOVER_ALPHA: f32 = 0.28;
  pub const SKIP_PRESS_ALPHA: f32 = 0.36;
  /// `.tvk-controls .btn-play-pause` : 56 px de haut, 78 px en lecture (rayon 20) ou 56 px à l'arrêt (rayon 28), dans un
  /// `.play-pause-wrapper` de 70 px (`styles.css:3027-3029`, `6743-6773`) ; `gap: 16px` entre les trois (`6703`).
  pub const PLAY_HEIGHT: f32 = 56.0;
  pub const PLAY_WIDTH_PLAYING: f32 = 78.0;
  pub const PLAY_RADIUS_PLAYING: f32 = 20.0;
  pub const PLAY_RADIUS_PAUSED: f32 = 28.0;
  pub const PLAY_WRAPPER: f32 = 70.0;
  pub const CONTROLS_GAP: f32 = 16.0;
  /// `.tvk-controls` : `margin-top: 8px` ; `.tvk-output` : `margin-bottom: -6px` ; `.tvk-progress-row` : `margin-top: 6px`.
  pub const CONTROLS_MARGIN_TOP: f32 = 8.0;
  pub const OUTPUT_MARGIN_BOTTOM: f32 = -6.0;
  pub const PROGRESS_MARGIN_TOP: f32 = 6.0;
  /// `.tvk-progress-row` : `width: clamp(220px, 28vw, 400px)`, `height: 36px` (`styles.css:8789-8799`).
  pub const PROGRESS_HEIGHT: f32 = 36.0;
  /// `.play-pause-wrapper` : `height: 48px` (`styles.css:3029`) : le bouton de 56 px dépasse de 4 px en haut et en bas.
  pub const CONTROLS_OVERFLOW: f32 = 4.0;
  /// Largeur du canvas de progression mesurée pendant `tvk-in` (`scale(0.96)`, `app.js:2881`).
  pub const PROGRESS_CANVAS_SHARE: f32 = 0.96;
  /// `#tvk-squiggly-canvas` (`app.js:3546-3600`) : marges 10, longueur d'onde 42, amplitude 5, trait de 9,5, rampe de 10 px.
  pub const SCRUB_PAD: f32 = 10.0;
  pub const SCRUB_WAVE_LENGTH: f32 = 42.0;
  pub const SCRUB_AMPLITUDE: f32 = 5.0;
  pub const SCRUB_WIDTH: f32 = 9.5;
  /// `.tv-karaoke-close` : 44 px, sans fond, survol `rgba(255,255,255,0.12)` ; coin haut droit à 28 px (`styles.css:6432-6449`,
  /// `8766-8778`) ; le bouton pochette est à `right: 80px`, soit 8 px avant le bouton de fermeture.
  pub const CORNER_SIZE: f32 = css(44.0);
  pub const CORNER_MARGIN: f32 = css(28.0);
  pub const CORNER_GAP: f32 = css(8.0);
  pub const CORNER_HOVER_ALPHA: f32 = 0.12;
  /// `#tvk-top-hover-zone` : 320 x 70 px en haut au centre (`styles.css:8779-8788`).
  pub const HOVER_ZONE: (f32, f32) = (css(320.0), css(70.0));
  /// `.tvk-visualizer` : `clamp(52px, 8.5vh, 84px)` de haut, `min(760, max(360, 64vw))` de large (`app.js:2847-2857`).
  pub const BARS_MAX_HEIGHT: f32 = 84.0;

  /// `@keyframes tvk-in` : `from { opacity: 0; transform: scale(0.96) translateY(32px) }` (`styles.css:6266-6269`). Le `translateY` est
  /// dans le repère déjà mis à l'échelle : mesuré `matrix(0.96, 0, 0, 0.96, 0, 30.72)` à t = 0 de `12-karaoke` (`samples.json`).
  pub const POSE_SCALE_FROM: f32 = 0.96;
  pub const POSE_OFFSET: f32 = css(32.0);
  /// `.tv-karaoke` : `animation: tvk-in 0.4s cubic-bezier(0.2, 0, 0, 1) both` (`styles.css:6283`). Ajusté sur `samples.json` (erreur
  /// quadratique 9e-8 sur l'opacité de 316 à 716 ms).
  pub const ENTER_MS: f32 = 400.0;
  pub const ENTER_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `.tv-karaoke.closing` : `animation: tvk-out 0.3s cubic-bezier(0.4, 0, 1, 1) both` (`styles.css:6270-6273`, `6288-6292`).
  pub const EXIT_MS: f32 = 300.0;
  pub const EXIT_CURVE: [f32; 4] = [0.4, 0.0, 1.0, 1.0];
  /// `closeTvKaraoke` : `.closing` est retiré et `.hidden` posé au bout de 320 ms (`app.js:7109-7113`).
  pub const HIDE_MS: f32 = 320.0;
  /// `.tv-karaoke .tvk-panel` : `transition` de `clip-path` et de `transform` en `380ms cubic-bezier(0.22, 1, 0.36, 1)`, ouverte comme
  /// fermée (`styles.css:8863-8866`, `8879-8882`). La courbe ne dépasse pas 1 (`y1 = y2 = 1`). `tvk-panel-in` (`styles.css:8868-8871`,
  /// `8873`) ne se voit jamais : `.open` pose `transform` et `opacity` en `!important`, qui l'emportent sur une animation (CSS Cascade 4,
  /// section 6.1) ; seules les transitions, au-dessus des `!important`, jouent.
  pub const PANEL_SHAPE_MS: f32 = 380.0;
  pub const PANEL_SHAPE_CURVE: [f32; 4] = [0.22, 1.0, 0.36, 1.0];
  /// `opacity` : `180ms ease` à l'ouverture (`styles.css:8881`), `260ms ease 80ms` à la fermeture (`styles.css:8865`).
  pub const PANEL_OPEN_FADE_MS: f32 = 180.0;
  pub const PANEL_CLOSE_FADE_MS: f32 = 260.0;
  pub const PANEL_CLOSE_FADE_DELAY_MS: f32 = 80.0;
  pub const PANEL_FADE_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
  /// Fermé, le panneau est à `translate3d(-50%, -18px, 0) scale(0.98)` (`styles.css:8850`), `transform-origin: top center`
  /// (`styles.css:8829`, non redéfini par `.tv-karaoke .tvk-panel`).
  pub const PANEL_SCALE_FROM: f32 = 0.98;
  pub const PANEL_OFFSET: f32 = css(18.0);
  /// `clip-path: inset(0 -35px 100% -35px round 0 0 28px 28px)` fermé, `inset(0 -35px -20px -35px ...)` ouvert (`styles.css:8862`,
  /// `8874`) : le bas de la découpe passe de `100 %` de la hauteur à `-20px`, soit une hauteur visible de `p * (hauteur + 20px)`.
  pub const PANEL_CLIP_SIDE: f32 = css(35.0);
  pub const PANEL_CLIP_BELOW: f32 = css(20.0);
  /// Placement : `position: fixed; top: 0; left: 50%` et `width: min(380px, calc(100vw - 32px))` (`styles.css:8839-8844`), coins du bas
  /// arrondis de 28 px (`border-radius: 0 0 28px 28px`, `8846`), fond `--drawer-bg` = `surface-container-high-solid` (`8833`), ombre
  /// `0 8px 32px rgba(0, 0, 0, 0.45)` (`8851`).
  pub const PANEL_WIDTH: f32 = 380.0;
  pub const PANEL_MARGIN: f32 = 32.0;
  pub const PANEL_RADIUS: f32 = css(28.0);
  pub const PANEL_SHADOW_Y: f32 = css(8.0);
  pub const PANEL_SHADOW_BLUR: f32 = css(32.0);
  pub const PANEL_SHADOW_ALPHA: f32 = 0.45;
  /// Raccords concaves `::before` et `::after` : carrés de `--rb: 26px` hors du panneau, dégradé radial de rayon 26 px centré sur le
  /// coin bas extérieur (`styles.css:8834`, `8885-8902`). Le demi-pixel de transition du dégradé est l'anticrénelage du chemin.
  pub const PANEL_EAR: f32 = css(26.0);
  /// Pas, en pixels écran, de l'image unique du fond rejouée sous le panneau pendant son fondu (`Orbs::flatten`) : le fond est flou
  /// (orbes floutés, vignette en dégradé), sans détail plus fin que quelques pixels. Choix de l'implémentation, sans équivalent dans Riff.
  pub const VEIL_STEP: f32 = 8.0;
  /// `.tvk-panel-inner` : `max-height: min(76vh, 580px)`, `padding: 18px 22px 24px 22px`, `gap: 14px` (`styles.css:8905-8912`).
  pub const INNER_MAX_HEIGHT: f32 = css(580.0);
  pub const INNER_MAX_VH: f32 = 0.76;
  pub const INNER_PAD_TOP: f32 = css(18.0);
  pub const INNER_PAD_X: f32 = css(22.0);
  pub const INNER_PAD_BOTTOM: f32 = css(24.0);
  pub const INNER_GAP: f32 = css(14.0);
  /// `mask-image: linear-gradient(to bottom, #000 0%, #000 calc(100% - 36px), transparent 100%)` (`styles.css:8916-8917`).
  pub const INNER_FADE: f32 = css(36.0);
  /// `.tvk-panel-inner` : `transform: translateY(-6px)` fermé, `translateY(0)` ouvert (`styles.css:8918`, `8923-8925`), `transition:
  /// transform 220ms cubic-bezier(0.16, 1, 0.3, 1)` (`8919`) : dans les deux sens, sans délai. La courbe ne dépasse pas 1.
  pub const INNER_OFFSET: f32 = css(6.0);
  pub const INNER_MS: f32 = 220.0;
  pub const INNER_CURVE: [f32; 4] = [0.16, 1.0, 0.3, 1.0];
  /// `@keyframes tvk-pop` : `animation: tvk-pop var(--tvk-speed) cubic-bezier(.2,0,0,1)` (`styles.css:9110-9114`, `9160-9167`) ; la
  /// courbe s'applique à chaque segment entre images clés.
  pub const POP_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];

  /// `ease-in-out` des orbes : `cubic-bezier(0.42, 0, 0.58, 1)`, appliqué à chaque segment des `@keyframes tvkOrbit*`.
  pub const ORBIT_CURVE: [f32; 4] = [0.42, 0.0, 0.58, 1.0];

  /// Image clé de `tvkOrbit*` : `translate3d(x%, y%, 0) scale(s) rotate(r deg)`, `x` et `y` en parts de la taille de l'orbe.
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct OrbitFrame {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub degrees: f32,
  }

  /// Un orbe `.tvk-ambient-blob.bN` : boîte en parts du conteneur `#tvk-ambient-bg` (largeur, hauteur, bord gauche, bord haut),
  /// opacité, durée de l'animation `ease-in-out infinite alternate` et images clés à 0, 50 et 100 %.
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct Orb {
    pub width: f32,
    pub height: f32,
    pub left: f32,
    pub top: f32,
    pub opacity: f32,
    pub seconds: f32,
    pub frames: [OrbitFrame; 3],
  }

  const fn frame(x: f32, y: f32, scale: f32, degrees: f32) -> OrbitFrame {
    OrbitFrame {
      x: x / 100.0,
      y: y / 100.0,
      scale,
      degrees,
    }
  }

  /// `.tvk-ambient-blob` : `border-radius: 50%`, `background-size: cover`, `opacity: 0.67` (`styles.css:6323-6333`). `right` et `bottom`
  /// de la feuille de style sont convertis en bords gauche et haut. Les images clés viennent de `styles.css:6365-6384`.
  pub const ORBS: [Orb; 4] = [
    // `.b1` : 78 %, `top: -22%`, `left: -18%`, `tvkOrbit1 17s` (`styles.css:6334-6338`).
    Orb {
      width: 0.78,
      height: 0.78,
      left: -0.18,
      top: -0.22,
      opacity: 0.67,
      seconds: 17.0,
      frames: [
        frame(0.0, 0.0, 1.05, 0.0),
        frame(14.0, 10.0, 1.22, 25.0),
        frame(6.0, 18.0, 1.12, -15.0),
      ],
    },
    // `.b2` : 74 %, `bottom: -24%`, `right: -16%`, `tvkOrbit2 4s` (`styles.css:6339-6343`).
    Orb {
      width: 0.74,
      height: 0.74,
      left: 1.16 - 0.74,
      top: 1.24 - 0.74,
      opacity: 0.67,
      seconds: 4.0,
      frames: [
        frame(0.0, 0.0, 1.10, 0.0),
        frame(-15.0, -12.0, 1.25, -28.0),
        frame(-8.0, -20.0, 1.10, 18.0),
      ],
    },
    // `.b3` : 65 %, `top: 12%`, `right: 8%`, `opacity: 0.72`, `tvkOrbit3 6s` (`styles.css:6344-6349`).
    Orb {
      width: 0.65,
      height: 0.65,
      left: 0.92 - 0.65,
      top: 0.12,
      opacity: 0.72,
      seconds: 6.0,
      frames: [
        frame(0.0, 0.0, 1.00, 0.0),
        frame(-18.0, 14.0, 1.18, 32.0),
        frame(10.0, -12.0, 1.26, -22.0),
      ],
    },
    // `.b4` : 68 %, `bottom: 8%`, `left: 12%`, `opacity: 0.68`, `tvkOrbit4 22s` (`styles.css:6350-6355`).
    Orb {
      width: 0.68,
      height: 0.68,
      left: 0.12,
      top: 0.92 - 0.68,
      opacity: 0.68,
      seconds: 22.0,
      frames: [
        frame(0.0, 0.0, 1.15, 0.0),
        frame(16.0, -14.0, 1.02, -24.0),
        frame(-12.0, 10.0, 1.32, 20.0),
      ],
    },
  ];
}

/// Mode compact, `body.mini` de Riff (`styles.css:6952-7222`), mesuré dans `ui-reference/screens/17a` et `17b`.
///
/// Le mode s'active sous 520 px CSS de large (`app.js:18246`). Les tailles du lecteur suivent `vmin`, le plus
/// petit côté de la fenêtre en pixels CSS : [`Metrics::of`] les calcule depuis la taille de fenêtre écran.
pub mod compact {
  use super::{ZOOM, css};

  /// `body.mini` s'applique quand `window.innerWidth <= 520` (`app.js:18247`) ; 1 px CSS = `ZOOM` px logique, soit 491,4.
  pub const LAYOUT_MAX_WIDTH: f32 = css(520.0);

  /// `body.mini .titlebar` : `height: 28px; padding: 0 8px` (`styles.css:6961-6964`).
  pub const TITLEBAR_HEIGHT: f32 = css(28.0);
  pub const TITLEBAR_PADDING: f32 = css(8.0);
  /// `body.mini .titlebar-controls` : `gap: 2px` (`styles.css:6968`).
  pub const BUTTON_GAP: f32 = css(2.0);
  /// `.win-btn` : `min-width: 42px` (`styles.css:278-285`) l'emporte sur `width: 24px` de `body.mini .titlebar-btn` ;
  /// la hauteur est 24 px (`styles.css:6971-6974`).
  pub const BUTTON_WIDTH: f32 = css(42.0);
  pub const BUTTON_HEIGHT: f32 = css(24.0);
  /// SVG de 14 px (`index.html:335-345`).
  pub const BUTTON_ICON: f32 = 14.0;
  /// `.titlebar-btn:hover` : rayon `40 %` (`styles.css:7522`).
  pub const BUTTON_HOVER_RADIUS: f32 = BUTTON_HEIGHT * 0.4;

  /// `body.mini .artwork-wrapper` : `margin: 4px` (`styles.css:7021`).
  pub const COVER_MARGIN: f32 = css(4.0);
  /// `body.mini .track-meta` : `gap: 2px` (`styles.css:7058`).
  pub const META_GAP: f32 = css(2.0);
  /// `body.mini .playback-controls` : `gap: 6px` (`styles.css:7106`).
  pub const CONTROLS_GAP: f32 = css(6.0);
  /// `#btn-prev` `margin-left: 8px`, `#btn-next` `margin-right: 8px` (`styles.css:3024-3025`).
  pub const SKIP_MARGIN: f32 = css(8.0);
  /// `.play-pause-wrapper` : 70 px de large (`styles.css:3029`), le bouton reste centré dedans.
  pub const PLAY_WRAPPER: f32 = css(70.0);
  /// `.btn-play-pause.playing` : `border-radius: 18px` (`styles.css:3083`) ; au repos 24 px (`styles.css:3049`).
  pub const PLAY_RADIUS_PLAYING: f32 = css(18.0);
  pub const PLAY_RADIUS_PAUSED: f32 = css(24.0);
  /// `.m3c-canvas` : 120 px et forme de 70 px (`app.js:16768-16769`) ; en mode compact le canvas est
  /// ramené à la taille de la pochette, la forme garde ce rapport.
  pub const SHAPE_SHARE: f32 = 70.0 / 120.0;
  /// `body.mini .scrubber-track-container` : `height: 18px` (`styles.css:7145`) ; le canvas garde 40 px de bitmap,
  /// la vague est donc écrasée verticalement de 18/40 (`app.js:2828`).
  pub const SCRUBBER_HEIGHT: f32 = css(18.0);
  pub const SCRUBBER_BITMAP_HEIGHT: f32 = 40.0;
  /// `body.mini .scrubber-bar-wrapper` : `padding-bottom: 2px` (`styles.css:7131`).
  pub const SCRUBBER_PADDING: f32 = css(2.0);

  /// `#mini-lyrics-container` : `padding: 8px 0` (`styles.css:7157`).
  pub const LYRICS_PADDING: f32 = css(8.0);
  /// `.mini-lyric-line` : 13 px, interligne 1,5, `padding: 3px 0`, opacité 0,38, graisse 500 (`styles.css:7174-7186`).
  pub const LINE_SIZE: f32 = css(13.0);
  pub const LINE_HEIGHT: f32 = 1.5;
  pub const LINE_PADDING: f32 = css(3.0);
  pub const LINE_OPACITY: f32 = 0.38;
  /// `.mini-lyric-line.active` : 15 px, graisse 700, couleur `primary` (`styles.css:7187-7192`).
  pub const LINE_SIZE_ACTIVE: f32 = css(15.0);
  /// `.mini-lyric-line.active .lyric-word` : opacité 0,45 pour les mots à venir (`styles.css:7197-7199`).
  pub const WORD_OPACITY: f32 = 0.45;
  /// `.mini-lyric-line .lyric-paren` : `font-size: 0.68em`, opacité 0,55 (`styles.css:2623-2631`).
  pub const PAREN_SCALE: f32 = 0.68;
  /// `.mini-lyrics-empty` : 13 px, `outline` à 0,6 (`styles.css:7209-7220`).
  pub const EMPTY_OPACITY: f32 = 0.6;

  /// `clamp(min, per_vmin x vmin, max)` en pixels CSS, rendu en pixels écran.
  fn clamp_vmin(vmin: f32, min: f32, per_vmin: f32, max: f32) -> f32 {
    css((per_vmin * vmin).clamp(min, max))
  }

  /// Dimensions du lecteur qui dépendent de la taille de la fenêtre (`clamp(.., N vmin, ..)`), en pixels écran.
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct Metrics {
    /// `.playbar` : `padding: clamp(8, 2.5vmin, 20) clamp(12, 3.5vmin, 24) clamp(12, 3.5vmin, 22)` (`styles.css:6982`).
    pub pad_top: f32,
    pub pad_x: f32,
    pub pad_bottom: f32,
    /// `column-gap: clamp(10, 3vmin, 22)` et `row-gap: clamp(6, 2vmin, 14)` (`styles.css:6986-6987`).
    pub col_gap: f32,
    pub row_gap: f32,
    /// Pochette : `clamp(48, 24vmin, 136)` (`styles.css:7050`).
    pub cover: f32,
    /// `.track-title` : `clamp(13, 3.6vmin, 20)` ; `.track-artist` : `clamp(11, 2.8vmin, 15)` (`styles.css:7076`, `7089`).
    pub title: f32,
    pub artist: f32,
    /// `.control-btn` : `clamp(32, 8.5vmin, 46)` ; `.btn-play-pause` : `clamp(40, 11vmin, 64)` (`styles.css:7112`, `7118`).
    pub control: f32,
    pub play: f32,
  }

  impl Metrics {
    /// Dimensions pour une fenêtre de `width` x `height` pixels écran. Riff mesure `vmin` sur la zone
    /// de la page, en pixels CSS : la fenêtre divisée par le zoom interne.
    #[must_use]
    pub fn of(width: f32, height: f32) -> Self {
      let vmin = width.min(height) / ZOOM / 100.0;
      Self {
        pad_top: clamp_vmin(vmin, 8.0, 2.5, 20.0),
        pad_x: clamp_vmin(vmin, 12.0, 3.5, 24.0),
        pad_bottom: clamp_vmin(vmin, 12.0, 3.5, 22.0),
        col_gap: clamp_vmin(vmin, 10.0, 3.0, 22.0),
        row_gap: clamp_vmin(vmin, 6.0, 2.0, 14.0),
        cover: clamp_vmin(vmin, 48.0, 24.0, 136.0),
        title: clamp_vmin(vmin, 13.0, 3.6, 20.0),
        artist: clamp_vmin(vmin, 11.0, 2.8, 15.0),
        control: clamp_vmin(vmin, 32.0, 8.5, 46.0),
        play: clamp_vmin(vmin, 40.0, 11.0, 64.0),
      }
    }

    /// Largeur de `.playback-controls` : marges de 8 px, écarts de 6 px, bouton lecture dans son cadre de 70 px.
    #[must_use]
    pub fn controls_width(&self) -> f32 {
      2.0 * (SKIP_MARGIN + self.control + CONTROLS_GAP) + PLAY_WRAPPER
    }
  }
}

/// Réglages, `#view-settings` et `.settings-pane` de Riff, mesurés sur `ui-reference/screens/21*.png`.
/// Les valeurs viennent de `styles.css` (`:7259-7352` et `7356-7480` pour le cadre, `3476-3530` pour les rangées).
pub mod settings {
  use super::{css, playbar::FONT_SCALE};

  /// `#view-settings` : voile `rgba(0, 0, 0, 0.52)` (`styles.css:7361`).
  pub const SCRIM_ALPHA: f32 = 0.52;
  /// `#view-settings .settings-view` : `min(1060px, 94vw)` sur `min(750px, 88vh)`, coins de 28 px (`styles.css:7389-7398`).
  pub const MODAL_WIDTH: f32 = css(1060.0);
  pub const MODAL_HEIGHT: f32 = css(750.0);
  pub const MODAL_RADIUS: f32 = css(28.0);
  /// `#view-settings` : `opacity` du voile (et donc de tout ce qu'il contient) en `190ms cubic-bezier(0.2, 0, 0, 1)`
  /// (`styles.css:7377`, `7385`).
  pub const SCRIM_MS: f32 = 190.0;
  /// `.settings-view` : `opacity 180ms` et `transform 200ms`, même courbe (`styles.css:7399-7402`).
  pub const CARD_OPACITY_MS: f32 = 180.0;
  pub const CARD_POSE_MS: f32 = 200.0;
  pub const FADE_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `.settings-section:not(.cat-hidden)` : `animation: settingsIn .2s ease`, `@keyframes settingsIn { from { opacity: 0;
  /// transform: translateY(6px) } }` (`styles.css:7349-7351`). La seule image clé est `from` : l'arrivée est la valeur courante.
  pub const SECTION_MS: f32 = 200.0;
  pub const SECTION_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
  pub const SECTION_OFFSET: f32 = css(6.0);
  /// Fermée, la carte est `transform: scale(0.95) translateY(8px)` (`styles.css:7399`) autour de son centre (`transform-origin`
  /// par défaut) : mesuré `matrix(0.95, 0, 0, 0.95, 0, 7.6)` sur `04-reglages`, donc `translateY` dans le repère mis à l'échelle.
  pub const CARD_SCALE_FROM: f32 = 0.95;
  pub const CARD_OFFSET: f32 = css(8.0);

  /// `#view-settings .settings-nav` : `flex: 0 0 240px; padding: 20px 12px` (`styles.css:7418-7427`).
  pub const NAV_WIDTH: f32 = css(240.0);
  pub const NAV_PADDING_Y: f32 = css(20.0);
  pub const NAV_PADDING_X: f32 = css(12.0);
  /// `.settings-nav` : `gap: 4px` (`styles.css:7260`).
  pub const NAV_GAP: f32 = css(4.0);
  /// `.settings-nav-btn` : `height: 48px; padding: 0 16px; gap: 12px`, 14 px, graisse 500 (`styles.css:7266-7279`).
  pub const NAV_BUTTON_HEIGHT: f32 = css(48.0);
  pub const NAV_BUTTON_PADDING_X: f32 = css(16.0);
  pub const NAV_BUTTON_GAP: f32 = css(12.0);
  pub const NAV_BUTTON_TEXT: f32 = css(14.0 * FONT_SCALE);
  /// `.settings-nav-btn:hover:not(.active)` : `on-surface` à 8 % (`styles.css:7288`).
  pub const NAV_HOVER_SHARE: f32 = 0.08;
  /// `.settings-search-box` : `margin: 0 8px 12px; height: 42px; padding: 0 16px` (`styles.css:7296-7305`).
  pub const SEARCH_HEIGHT: f32 = css(42.0);
  pub const SEARCH_MARGIN_X: f32 = css(8.0);
  pub const SEARCH_MARGIN_BOTTOM: f32 = css(12.0);
  pub const SEARCH_PADDING_X: f32 = css(16.0);
  /// `#settings-search-input` : 15 px, graisse 500 ; `.settings-search-icon` : 18 px, marge de 10 px (`styles.css:7308-7326`).
  pub const SEARCH_TEXT: f32 = css(15.0);
  pub const SEARCH_ICON_GAP: f32 = css(10.0);

  /// `#view-settings .settings-pane` : `padding: 28px 68px 40px 32px` (`styles.css:7428-7435`).
  pub const PANE_PADDING_TOP: f32 = css(28.0);
  pub const PANE_PADDING_RIGHT: f32 = css(68.0);
  pub const PANE_PADDING_BOTTOM: f32 = css(40.0);
  pub const PANE_PADDING_LEFT: f32 = css(32.0);
  /// Déplacement d'une capture de défilement à la suivante, mesuré sur 21-p1 à p6 (638 px écran).
  pub const PAGE_SCROLL: f32 = 638.0;

  /// `.m3-group` : `padding: 6px; gap: 6px`, coins de 28 px aux extrémités et de 6 px entre rangées (`styles.css:7474-7484`).
  pub const GROUP_PADDING: f32 = css(6.0);
  pub const GROUP_GAP: f32 = css(6.0);
  pub const GROUP_OUTER: f32 = css(28.0);
  pub const GROUP_INNER: f32 = css(6.0);

  /// `.setting-item` sans `.setting-hint` : `min-height: 48px; padding: 10px 16px` ; avec : `min-height: 60px; padding: 14px 18px` (`styles.css:3482-3492`).
  pub const ROW_MIN_HEIGHT: f32 = css(48.0);
  pub const ROW_PADDING_Y: f32 = css(10.0);
  pub const ROW_PADDING_X: f32 = css(16.0);
  pub const ROW_HINT_MIN_HEIGHT: f32 = css(60.0);
  pub const ROW_HINT_PADDING_Y: f32 = css(14.0);
  pub const ROW_HINT_PADDING_X: f32 = css(18.0);
  /// `.setting-item` : `gap: 8px` ; `.setting-item-top` : `gap: 14px`, 13 px (`styles.css:3493-3515`).
  pub const ROW_GAP: f32 = css(8.0);
  pub const ROW_TOP_GAP: f32 = css(14.0);
  pub const NAME_TEXT: f32 = css(13.0 * FONT_SCALE);
  /// Les tailles en `calc(N px * var(--font-scale))` portent le facteur 1,1875 de la capture (`playbar::FONT_SCALE`).
  /// `.setting-hint` : 12 px, `outline` à 78 %, interligne 1,35 (`styles.css:3523-3529`).
  pub const HINT_TEXT: f32 = css(12.0 * FONT_SCALE);
  pub const HINT_OPACITY: f32 = 0.78;
  pub const HINT_LINE: f32 = 1.35;
  /// `.setting-hints` : 11 px, `outline` (`styles.css:3517-3522`).
  pub const SCALE_TEXT: f32 = css(11.0 * FONT_SCALE);

  /// `.effect-switch-btn` : 52 x 32, bordure de 2 px, pouce de 16 px (24 px allumé) (`styles.css:3685-3735`).
  pub const SWITCH_WIDTH: f32 = css(52.0);
  pub const SWITCH_HEIGHT: f32 = css(32.0);
  pub const SWITCH_BORDER: f32 = css(2.0);
  pub const SWITCH_THUMB_OFF: f32 = css(16.0);
  pub const SWITCH_THUMB_ON: f32 = css(24.0);
  /// `left` du pouce, dans la bordure : 6 px éteint, 22 px allumé.
  pub const SWITCH_THUMB_LEFT_OFF: f32 = css(6.0);
  pub const SWITCH_THUMB_LEFT_ON: f32 = css(22.0);

  /// `.sl` : hauteur 28 px, rail de 16 px, pouce de 6 px (`styles.css:7570-7575`).
  pub const SLIDER_HEIGHT: f32 = css(28.0);
  pub const SLIDER_RAIL: f32 = css(16.0);
  pub const SLIDER_THUMB: f32 = css(6.0);
  /// `.sl-inactive` : `on-surface` à 16 % (`styles.css:7571`).
  pub const SLIDER_INACTIVE_SHARE: f32 = 0.16;
  /// `.slider-ticks` : hauteur 24 px, repères de 6 px (10 px majeurs), libellés de 11 px à 14 px (`styles.css:7870-7876`).
  pub const TICKS_HEIGHT: f32 = css(24.0);
  pub const TICK_MINOR: f32 = css(6.0);
  pub const TICK_MAJOR: f32 = css(10.0);
  pub const TICK_WIDTH: f32 = css(2.0);
  pub const TICK_LABEL_TOP: f32 = css(14.0);

  /// `.m3-dropdown-trigger` : `padding: 10px 14px`, coins de 12 px, 14 px, graisse 500 (`styles.css:4810-4828`).
  pub const DROPDOWN_PADDING_Y: f32 = css(10.0);
  pub const DROPDOWN_PADDING_X: f32 = css(14.0);
  pub const DROPDOWN_RADIUS: f32 = css(12.0);
  pub const DROPDOWN_TEXT: f32 = css(14.0 * FONT_SCALE);
  /// `#icon-pack-dropdown` : 236 px (`styles.css:4838`).
  pub const DROPDOWN_INLINE_WIDTH: f32 = css(236.0);

  /// `.theme-mode-toggle` et `.theme-mode-btn` : `padding: 4px`, `gap: 4px`, boutons `6px 16px`, 12 px (`styles.css:4784-4804`).
  pub const SEGMENT_PADDING: f32 = css(4.0);
  pub const SEGMENT_BUTTON_PADDING_Y: f32 = css(6.0);
  pub const SEGMENT_BUTTON_PADDING_X: f32 = css(16.0);
  pub const SEGMENT_TEXT: f32 = css(12.0 * FONT_SCALE);

  /// `.settings-modal-close-btn` : `top: 20px; right: 20px`, cercle de 36 px bordé de 2 px, « ESC » 10 px graisse 700 (`styles.css:7436-7470`).
  pub const CLOSE_OFFSET: f32 = css(20.0);
  pub const CLOSE_CIRCLE: f32 = css(36.0);
  pub const CLOSE_BORDER: f32 = css(2.0);
  pub const CLOSE_GAP: f32 = css(4.0);
  pub const CLOSE_TEXT: f32 = css(10.0);
  /// `.settings-close-circle` : bordure `outline` à 60 % ; `.settings-close-esc` : opacité 0,7.
  pub const CLOSE_BORDER_SHARE: f32 = 0.60;
  pub const CLOSE_ESC_OPACITY: f32 = 0.7;

  /// `.btn-outline-pill` des rangées : `padding: 2px 10px`, 12 px, fond `surface-container-high` (`styles.css:3323-3335`, `index.html:1127`).
  pub const PILL_PADDING_Y: f32 = css(2.0);
  pub const PILL_PADDING_X: f32 = css(10.0);
  pub const PILL_TEXT: f32 = css(12.0 * FONT_SCALE);

  /// Catégories Stockage, Avancé, À propos, Thèmes personnalisés, Plugins et Compte.
  pub mod system {
    use super::{FONT_SCALE, css};

    /// Couleur opaque en octets rouge, vert, bleu.
    pub type Rgb8 = (u8, u8, u8);

    /// `.btn-outline-pill` et `.btn-pill-primary` : `height: 40px`, 14 px graisse 500, `padding` en ligne `6px 14px` (`styles.css:3303-3335`, `index.html:2203`).
    pub const BTN_HEIGHT: f32 = css(40.0);
    pub const BTN_PADDING_X: f32 = css(14.0);
    pub const BTN_TEXT: f32 = css(12.0 * FONT_SCALE);
    /// Bouton de `.ct-head` sans style en ligne : `padding: 8px 20px` (outline) ou `10px 24px` (principal), 14 px (`styles.css:3303-3335`).
    pub const HEAD_BTN_PADDING_X: f32 = css(22.0);
    pub const HEAD_BTN_TEXT: f32 = css(14.0 * FONT_SCALE);
    /// Boutons de l'Avancé : `height: 32px; padding: 0 14px; font-size: 13px` (`index.html:2347`), connexion `0 16px`, 12,5 px (`index.html:2364`).
    pub const SMALL_BTN_HEIGHT: f32 = css(32.0);
    pub const SMALL_BTN_TEXT: f32 = css(13.0);
    pub const SIGN_BTN_PADDING_X: f32 = css(16.0);
    pub const SIGN_BTN_TEXT: f32 = css(12.5);
    /// Boutons du proxy : `height: 38px`, « + » 18 px graisse 600 sur 42 px mini, « Test » 12 px (`index.html:2385-2387`).
    pub const PROXY_HEIGHT: f32 = css(38.0);
    pub const PROXY_ADD_MIN_WIDTH: f32 = css(42.0);
    pub const PROXY_ADD_TEXT: f32 = css(18.0);
    pub const PROXY_TEST_TEXT: f32 = css(12.0);
    /// `.proxy-manager-box` : `margin-top: 12px; gap: 10px` ; `.proxy-list-item` : `padding: 8px 12px`, coins de 14 px, liste en `gap: 6px` (`index.html:2383`, `styles.css:9935-9960`).
    pub const PROXY_TOP: f32 = css(12.0);
    pub const PROXY_GAP: f32 = css(10.0);
    pub const PROXY_INPUT_GAP: f32 = css(8.0);
    pub const PROXY_ITEM_PADDING_Y: f32 = css(8.0);
    pub const PROXY_ITEM_PADDING_X: f32 = css(12.0);
    pub const PROXY_ITEM_RADIUS: f32 = css(14.0);
    pub const PROXY_LIST_GAP: f32 = css(6.0);
    pub const PROXY_EMPTY_TEXT: f32 = css(12.0);
    pub const PROXY_EMPTY_OPACITY: f32 = 0.6;
    pub const PROXY_EMPTY_PADDING_Y: f32 = css(6.0);
    pub const PROXY_EMPTY_PADDING_X: f32 = css(4.0);
    /// Champ du proxy : `.pill-text-input`, 13 px, `padding: 12px 16px`, coins `corner-m` (12 px) (`styles.css:3892-3900`).
    pub const PROXY_INPUT_TEXT: f32 = css(13.0);
    pub const PROXY_INPUT_PADDING_X: f32 = css(16.0);
    pub const PROXY_INPUT_RADIUS: f32 = css(12.0);

    /// Pastille d'icône des rangées du Stockage : 32 px, `surface-container-high`, icône de 20 px en `primary`, `gap: 12px` (`index.html:2178`).
    pub const ICON_CIRCLE: f32 = css(32.0);
    pub const ICON_GLYPH: f32 = 20.0;
    pub const ICON_GAP: f32 = css(12.0);
    /// Tuile YouTube du compte : 34 px, coins de 10 px, fond blanc, logo de 20 x 14 px (`index.html:2357`).
    pub const YT_TILE: f32 = css(34.0);
    pub const YT_TILE_RADIUS: f32 = css(10.0);
    /// Aménagement de la maison : `.theme-mode-btn` avec icône de 15 px et `gap: 6px` (`index.html:2603`).
    pub const LAYOUT_ICON: f32 = 15.0;
    pub const LAYOUT_GAP: f32 = css(6.0);
    pub const LAYOUT_HINT_TOP: f32 = css(6.0);

    /// `.adv-section-header` : `padding: 8px 0`, chevron de 18 px en `outline` (`styles.css:3804-3821`).
    pub const ADV_HEADER_PADDING_Y: f32 = css(8.0);
    /// `.settings-section-title` (`styles.css:4774`) : 0,85 rem (racine de 16 px mise à l'échelle comme `font-scale`), graisse 500, `on-surface-variant` à 70 %.
    pub const SECTION_TITLE_TEXT: f32 = css(13.6 * FONT_SCALE);
    pub const SECTION_TITLE_OPACITY: f32 = 0.7;

    /// `.ct-head` : `gap: 20px; margin-bottom: 16px` ; titre 15 px graisse 600, description 12 px, interligne 1,4 ou 1,45 (`styles.css:7880-7884`, `index.html:1344-1357`).
    pub const HEAD_GAP: f32 = css(20.0);
    pub const HEAD_MARGIN_BOTTOM: f32 = css(16.0);
    pub const HEAD_TITLE_TEXT: f32 = css(15.0 * FONT_SCALE);
    pub const HEAD_TITLE_MARGIN: f32 = css(4.0);
    pub const HEAD_TEXT: f32 = css(12.0 * FONT_SCALE);
    pub const HEAD_LINE_THEMES: f32 = 1.4;
    pub const HEAD_LINE_PLUGINS: f32 = 1.45;
    pub const HEAD_ACTIONS_GAP: f32 = css(8.0);
    /// `.ct-card` : `padding: 12px 16px 12px 12px`, `gap: 16px` ; nom 15 px graisse 700, méta 12,5 px à 75 % (`styles.css:7887-7890`).
    pub const CARD_PADDING_Y: f32 = css(12.0);
    pub const CARD_PADDING_RIGHT: f32 = css(16.0);
    pub const CARD_PADDING_LEFT: f32 = css(12.0);
    pub const CARD_GAP: f32 = css(16.0);
    pub const CARD_NAME_TEXT: f32 = css(15.0 * FONT_SCALE);
    pub const CARD_META_TEXT: f32 = css(12.5 * FONT_SCALE);
    pub const CARD_META_OPACITY: f32 = 0.75;
    pub const CARD_META_TOP: f32 = css(2.0);
    /// Bouton de suppression `.btn-icon-pill`, icône de 22 px.
    pub const CARD_DELETE_SIZE: f32 = css(40.0);
    /// `.ct-empty` : `padding: 28px 16px`, centré, 75 % (`styles.css:7891`).
    pub const EMPTY_PADDING_Y: f32 = css(28.0);
    pub const EMPTY_PADDING_X: f32 = css(16.0);
    pub const EMPTY_TEXT: f32 = css(14.0 * FONT_SCALE);
    pub const EMPTY_OPACITY: f32 = 0.75;

    /// `.riff-hero-banner-wrap` : `gap: 14px; margin-bottom: 20px` ; bannière de 148 px, coins `28 28 8 8` (`styles.css:9488-9505`).
    pub const HERO_GAP: f32 = css(14.0);
    pub const HERO_MARGIN_BOTTOM: f32 = css(20.0);
    pub const HERO_HEIGHT: f32 = css(148.0);
    pub const HERO_RADIUS_TOP: f32 = css(28.0);
    pub const HERO_RADIUS_BOTTOM: f32 = css(8.0);
    /// `radial-gradient(#9E6BFF, #834DF2 52 %, #6336B5)` : rendu en dégradé vertical.
    pub const HERO_TOP: Rgb8 = (0x9E, 0x6B, 0xFF);
    pub const HERO_MIDDLE: Rgb8 = (0x83, 0x4D, 0xF2);
    pub const HERO_BOTTOM: Rgb8 = (0x63, 0x36, 0xB5);
    /// `.riff-hero-cookie` : 92 px tourné de 45° dans un `clip-path` de 4 lobes, soit 70 px mesurés sur 28, `#D7C2FF` ; logo de 46 px `#36284E` (`styles.css:9512-9532`).
    /// `Discord RPC` : `color: #5865F2` sur l'icône (`index.html:2046`).
    pub const DISCORD_BLURPLE: Rgb8 = (0x58, 0x65, 0xF2);
    pub const COOKIE: f32 = css(70.0);
    pub const COOKIE_COLOR: Rgb8 = (0xD7, 0xC2, 0xFF);
    pub const COOKIE_LOGO: f32 = css(46.0);
    pub const COOKIE_LOGO_COLOR: Rgb8 = (0x36, 0x28, 0x4E);
    /// `.riff-float-logo` : `#492E7D` à 85 %, tailles 24, 28, 17 px ; positions en part de la bannière (`styles.css:9539-9569`).
    pub const FLOAT_COLOR: Rgb8 = (0x49, 0x2E, 0x7D);
    pub const FLOAT_OPACITY: f32 = 0.85;
    /// (taille, haut, gauche) ; bord haut et bord gauche en part de la bannière, déduits de `top`/`left`/`right`/`bottom` en pourcentage.
    pub const FLOATS: [(f32, f32, f32); 3] =
      [(24.0, 0.32, 0.35), (28.0, 0.28, 0.69), (17.0, 0.705, 0.656)];
    /// `.riff-hero-title` : 24 px graisse 700.
    pub const HERO_TITLE_TEXT: f32 = css(24.0 * FONT_SCALE);

    /// `.tg-account-hero` (règle des fenêtres étroites, `styles.css:11993`) : 236 px, coins de 26 px, avatar de 96 px, nom 21 px graisse 700.
    pub const ACCOUNT_HEIGHT: f32 = css(236.0);
    pub const ACCOUNT_RADIUS: f32 = css(26.0);
    /// `#settings-nav-account` : 48 px, avatar de 30 px, pseudo 13,5 px et « Account » 11 px (`index.html:14-50`).
    pub const NAV_ACCOUNT_HEIGHT: f32 = css(48.0);
    pub const NAV_ACCOUNT_AVATAR: f32 = css(30.0);
    pub const NAV_ACCOUNT_NAME_TEXT: f32 = css(13.5 * FONT_SCALE);
    pub const NAV_ACCOUNT_SUB_TEXT: f32 = css(11.0 * FONT_SCALE);
    pub const ACCOUNT_MARGIN_BOTTOM: f32 = css(14.0);
    pub const ACCOUNT_AVATAR: f32 = css(96.0);
    pub const ACCOUNT_NAME_TEXT: f32 = css(21.0 * FONT_SCALE);
    pub const ACCOUNT_CENTER_GAP: f32 = css(8.0);
    /// `--acc-c1` et `--acc-c2` pour l'icône par défaut (teinte 268 supposée) : `hsl(h, 30 %, 38 %)` et `hsl(h + 8, 22 %, 18 %)` (`app.js:22288`).
    pub const ACCOUNT_C1: Rgb8 = (0x5F, 0x43, 0x79);
    pub const ACCOUNT_C2: Rgb8 = (0x32, 0x1A, 0x42);
    /// `.tg-pat-icon` : 22 px, noir à 28 % ; (haut, gauche) en part, échelle, opacité (`styles.css:12030-12039`).
    pub const PATTERN_SIZE: f32 = 22.0;
    pub const PATTERN_ALPHA: f32 = 0.28;
    pub const PATTERN: [(f32, f32, f32, f32); 10] = [
      (0.10, 0.21, 0.95, 0.72),
      (0.24, 0.31, 1.06, 0.92),
      (0.53, 0.30, 0.98, 0.86),
      (0.44, 0.09, 0.84, 0.5),
      (0.74, 0.19, 0.90, 0.64),
      (0.05, 0.48, 0.92, 0.75),
      (0.24, 0.62, 1.06, 0.92),
      (0.53, 0.63, 0.98, 0.86),
      (0.10, 0.76, 0.95, 0.72),
      (0.44, 0.89, 0.84, 0.5),
    ];
    /// `.acc-btn` : `height: 38px; padding: 0 16px`, 13 px graisse 600, icône de 15 px (`styles.css:12160-12175`).
    pub const ACC_BTN_HEIGHT: f32 = css(38.0);
    pub const ACC_BTN_PADDING_X: f32 = css(16.0);
    pub const ACC_BTN_TEXT: f32 = css(13.0 * FONT_SCALE);
    pub const ACC_BTN_ICON: f32 = 15.0;
    pub const ACC_BTN_GAP: f32 = css(8.0);
    /// Rangée « Connected services » : `padding: 18px 24px`, nom 16 px graisse 600, texte de 380 px au plus, bouton à 12 px (`index.html:1428-1436`).
    pub const SERVICES_PADDING_Y: f32 = css(18.0);
    pub const SERVICES_PADDING_X: f32 = css(24.0);
    pub const SERVICES_NAME_TEXT: f32 = css(16.0 * FONT_SCALE);
    pub const SERVICES_GAP: f32 = css(6.0);
    pub const SERVICES_HINT_WIDTH: f32 = css(380.0);
    pub const SERVICES_BUTTON_TOP: f32 = css(12.0);
    /// `.acc-orbital-widget` : 190 px ; centre de 66 px ; satellites de 36 px (`styles.css:12189-12260`).
    pub const ORBIT: f32 = css(190.0);
    pub const ORBIT_CENTER: f32 = css(66.0);
    pub const SATELLITE: f32 = css(36.0);
    /// (haut, gauche) en pixels CSS, fond, taille du logo : SoundCloud, Wave, Last.fm, Discord, YouTube Music, Spotify, YouTube.
    pub const SATELLITES: [(f32, f32, Rgb8, f32); 7] = [
      (4.0, 77.0, (0xFF, 0x55, 0x00), 19.0),
      (20.0, 135.0, (0x1A, 0x12, 0x22), 20.0),
      (77.0, 147.0, (0xD5, 0x10, 0x07), 19.0),
      (132.0, 126.0, (0x58, 0x65, 0xF2), 20.0),
      (149.0, 77.0, (0xFF, 0x00, 0x00), 20.0),
      (126.0, 24.0, (0x1E, 0xD7, 0x60), 20.0),
      (60.0, 7.0, (0xFF, 0xFF, 0xFF), 22.0),
    ];
  }

  /// `.customizer-box` : `surface-container-low`, coins de 14 px, `padding: 14px` ; deux colonnes séparées de 16 px (`styles.css:10496-10503`, `index.html:1325`).
  pub const CUSTOMIZER_GAP: f32 = css(16.0);
  pub const CUSTOMIZER_BOX_RADIUS: f32 = css(14.0);
  pub const CUSTOMIZER_BOX_PADDING: f32 = css(14.0);
  /// Titre de colonne : 13 px, graisse 600, marge basse de 10 px (`index.html:1326`).
  pub const CUSTOMIZER_TITLE_TEXT: f32 = css(13.0);
  pub const CUSTOMIZER_TITLE_MARGIN: f32 = css(10.0);
  /// `.customizer-drag-row` : `gap: 10px; padding: 8px 12px`, coins de 10 px, 13 px, liste en `gap: 6px` (`styles.css:10505-10523`).
  pub const CUSTOMIZER_LIST_GAP: f32 = css(6.0);
  pub const CUSTOMIZER_ROW_GAP: f32 = css(10.0);
  pub const CUSTOMIZER_ROW_PADDING_Y: f32 = css(8.0);
  pub const CUSTOMIZER_ROW_PADDING_X: f32 = css(12.0);
  pub const CUSTOMIZER_ROW_RADIUS: f32 = css(10.0);
  pub const CUSTOMIZER_ROW_TEXT: f32 = css(13.0);
  /// Poignée de glissement : six points de 2,5 px.
  pub const CUSTOMIZER_GRIP_DOT: f32 = css(2.5);
  pub const CUSTOMIZER_GRIP_GAP: f32 = css(2.5);
  /// `#btn-reset-sidebar-order` : `height: 30px; padding: 0 12px; font-size: 12px` (`index.html:1323`).
  pub const CUSTOMIZER_RESET_HEIGHT: f32 = css(30.0);
  pub const CUSTOMIZER_RESET_PADDING_X: f32 = css(12.0);
  pub const CUSTOMIZER_RESET_TEXT: f32 = css(12.0);

  /// `.setting-item` dont le texte d'aide est en style en ligne (pas de classe `.setting-hint`) : couleur `outline` pleine, sans l'opacité de 0,78.
  pub const HINT_INLINE_OPACITY: f32 = 1.0;

  /// `.palette-preview-dots` : `gap: 8px; margin-top: 4px; padding: 4px 2px` ; `.palette-dot` : 18 px, bordure de 1 px `outline-variant` (`styles.css:3771-3784`).
  pub const PALETTE_DOT: f32 = css(18.0);
  pub const PALETTE_DOT_GAP: f32 = css(8.0);
  pub const PALETTE_DOTS_PADDING_Y: f32 = css(4.0);
  pub const PALETTE_DOTS_PADDING_X: f32 = css(2.0);
  pub const PALETTE_DOTS_MARGIN_TOP: f32 = css(4.0);
  pub const PALETTE_DOT_BORDER: f32 = css(1.0);

  /// `.theme-palette-picker` : pastilles de 36 px, `gap: 10px; margin-top: 4px` ; la couleur choisie porte un anneau de 3 px à 3 px (`styles.css:3614-3646`).
  pub const SWATCH_SIZE: f32 = css(36.0);
  pub const SWATCH_GAP: f32 = css(10.0);
  pub const SWATCH_MARGIN_TOP: f32 = css(4.0);
  pub const SWATCH_RING: f32 = css(3.0);
  pub const SWATCH_RING_OFFSET: f32 = css(3.0);
  /// `.theme-custom-color-btn svg` : croix de 14 px (`index.html:1549`).
  pub const SWATCH_PLUS: f32 = css(14.0);

  /// `.hue-seg` : 12 segments de 16 px de haut dans une zone de 28 px, `gap: 4px`, trouée de 11 px de chaque côté du pouce (`styles.css:7577-7582`, `app.js:16721-16736`).
  pub const HUE_HEIGHT: f32 = css(28.0);
  pub const HUE_TRACK_HEIGHT: f32 = css(16.0);
  pub const HUE_SEGMENTS: usize = 12;
  pub const HUE_SEGMENT_GAP: f32 = css(4.0);
  pub const HUE_KNOB_CLEARANCE: f32 = css(11.0);
  /// Coins des segments : 8 px aux extrémités de la bande, 4 px ailleurs.
  pub const HUE_SEGMENT_OUTER: f32 = css(8.0);
  pub const HUE_SEGMENT_INNER: f32 = css(4.0);
  /// `hsl(h 75% 55%)` de chaque extrémité de segment (`app.js:16728`).
  pub const HUE_SATURATION: f32 = 0.75;
  pub const HUE_LIGHTNESS: f32 = 0.55;

  /// `.btn-pill-primary` et `.btn-outline-pill` : 40 px de haut, 14 px, graisse 500 ; `padding` de 24 px (plein) ou 20 px (contour) (`styles.css:3303-3345`).
  pub const BUTTON_HEIGHT: f32 = css(40.0);
  pub const BUTTON_TEXT: f32 = css(14.0 * FONT_SCALE);
  pub const BUTTON_PRIMARY_PADDING_X: f32 = css(24.0);
  pub const BUTTON_OUTLINE_PADDING_X: f32 = css(20.0);
  /// Les deux boutons d'une rangée de médias : `gap: 8px` (`index.html:1654`).
  pub const BUTTON_GAP: f32 = css(8.0);

  /// `.setting-hints` en texte seul (curseurs de plus de 20 pas, `app.js:18135-18138`) : 11 px, `outline`, `justify-content: space-between` (`styles.css:3517-3522`).
  pub const HINTS_TEXT: f32 = css(11.0 * FONT_SCALE);
  /// Rangée « Blocked artists & tracks » vide : 12 px, `outline` (`app.js:23547`).
  pub const BLOCKED_EMPTY_TEXT: f32 = css(12.0);

  /// `.setting-item.is-disabled` : opacité 0,45, sans événements de souris (`styles.css:3502-3506`).
  pub const DISABLED_OPACITY: f32 = 0.45;
}

/// Fenêtre Audio, `#audio-settings-modal` de Riff.
pub mod audio {
  use super::css;

  /// `--font-scale` : les tailles de texte de la fenêtre sont `calc(Npx * var(--font-scale))`.
  const FONT_SCALE: f32 = 1.1875;

  /// `.audio-menu-card.connected-editor` : `width: min(540px, calc(100vw - 80px))` (`styles.css:7589`).
  pub const CARD_WIDTH: f32 = css(540.0);
  /// Même règle : `height: 460px`.
  pub const CARD_HEIGHT: f32 = css(460.0);
  /// Même règle : `padding: 16px`.
  pub const CARD_PADDING: f32 = css(16.0);
  /// Même règle : `border-radius: 24px 24px 0 0`.
  pub const CARD_RADIUS: f32 = css(24.0);
  /// Même règle : `right: 64px` (`bottom` : sur la barre de lecture, `styles.css:10831`).
  pub const CARD_RIGHT: f32 = css(64.0);
  /// `.settings-card` : `gap: 16px` (`styles.css:3430`), aussi `gap` de `.audio-modal-layout` (`styles.css:7597`).
  pub const GAP: f32 = css(16.0);

  /// `.audio-menu-card .settings-card-header h3` : `24px x --font-scale`, graisse 500 (`styles.css:4995`).
  pub const TITLE_SIZE: f32 = css(24.0 * FONT_SCALE);
  /// `.settings-title-group` : SVG de 20 px (`index.html:2944`) et `gap: 8px` (`styles.css:3461`).
  pub const TITLE_ICON: f32 = css(20.0);
  pub const TITLE_GAP: f32 = css(8.0);
  /// `.btn-icon-pill` : 36 x 36 (`styles.css:3285`), SVG de 16 px (`index.html:2947`).
  pub const CLOSE_SIZE: f32 = css(36.0);
  pub const CLOSE_ICON: f32 = css(16.0);

  /// `.audio-menu-card.connected-editor .audio-tabs` : `flex: 0 0 150px` (`styles.css:7596`).
  pub const TABS_WIDTH: f32 = css(150.0);
  /// `.audio-tabs` : `gap: 2px` (`styles.css:7598`).
  pub const TABS_GAP: f32 = css(2.0);
  /// `.audio-tab` : `height: 44px; padding: 0 16px; gap: 10px`, `13px x --font-scale` (`styles.css:7599`).
  pub const TAB_HEIGHT: f32 = css(44.0);
  pub const TAB_PADDING: f32 = css(16.0);
  pub const TAB_GAP: f32 = css(10.0);
  pub const TAB_TEXT: f32 = css(13.0 * FONT_SCALE);
  /// `.audio-tab svg` : 18 x 18 (`styles.css:7620`).
  pub const TAB_ICON: f32 = css(18.0);
  /// `.audio-tab:hover:not(.active)` : 8 % de `on-surface` (`styles.css:7625`).
  pub const TAB_HOVER_SHARE: f32 = 0.08;

  /// `.audio-menu-card.connected-editor .settings-body` : `padding: 8px 10px 14px` (`styles.css:7742`).
  pub const BODY_PADDING_TOP: f32 = css(8.0);
  pub const BODY_PADDING_X: f32 = css(10.0);
  pub const BODY_PADDING_BOTTOM: f32 = css(14.0);
  /// `.audio-menu-card.connected-editor .audio-panel` : `padding: 2px 4px 8px` (`styles.css:7743`).
  pub const PANEL_PADDING_TOP: f32 = css(2.0);
  pub const PANEL_PADDING_X: f32 = css(4.0);
  pub const PANEL_PADDING_BOTTOM: f32 = css(8.0);
  /// `.audio-panel` : `gap: 2px` (`styles.css:7678`) ; l'onglet Enhancer : `gap: 8px` (`styles.css:7702`).
  pub const PANEL_GAP: f32 = css(2.0);
  pub const ENHANCE_GAP: f32 = css(8.0);
  /// `.audio-panel .setting-item` : `border-radius: 4px`, et 20 px sur les coins extérieurs de la première et de la dernière rangée
  /// (`styles.css:7681-7685`) ; l'onglet Enhancer : 16 px partout (`styles.css:7736-7739`). Visible seulement au survol (`--hl`).
  pub const ITEM_OUTER_RADIUS: f32 = css(20.0);
  pub const ITEM_INNER_RADIUS: f32 = css(4.0);
  pub const ENHANCE_ITEM_RADIUS: f32 = css(16.0);

  /// `.setting-item` sans indication : `padding: 10px 16px` (`styles.css:3482`, `!important`).
  pub const ITEM_PADDING_Y: f32 = css(10.0);
  pub const ITEM_PADDING_X: f32 = css(16.0);
  /// « Normalize volume » : `padding` haut et bas mesurés à 20 px CSS sur `32a-audio-enhance.png` (bloc de trois lignes).
  pub const ITEM_PADDING_TALL: f32 = css(20.0);
  /// `.audio-panel .setting-item` : `gap: 6px` entre la ligne du haut et le curseur (`styles.css:7681`).
  pub const ITEM_GAP: f32 = css(6.0);
  /// Ligne `.setting-item-top` d'un curseur : 20 px CSS mesurés (`13px x --font-scale`, interligne 1,3).
  pub const ROW_HEIGHT: f32 = css(20.0);
  /// `.setting-item-top` : `font-size: 13px x --font-scale`, `gap: 14px` (`styles.css:3507`, `3514`).
  pub const NAME_SIZE: f32 = css(13.0 * FONT_SCALE);
  pub const ROW_GAP: f32 = css(14.0);
  /// Note sous un réglage : `12px x --font-scale`, `opacity: 0.78` (`index.html:3060`, `styles.css:3523`).
  pub const HINT_SIZE: f32 = css(12.0 * FONT_SCALE);
  pub const HINT_OPACITY: f32 = 0.78;
  /// Interligne mesuré des notes (16,5 px écran sur `32a-audio-enhance.png`).
  pub const HINT_LINE: f32 = 16.5 / (12.0 * FONT_SCALE * super::ZOOM);

  /// `.sl` : `height: 28px` (`styles.css:7568`).
  pub const SLIDER_HEIGHT: f32 = css(28.0);
  /// `.sl-active`, `.sl-inactive` : `top: 6px; height: 16px` (`styles.css:7569`).
  pub const SLIDER_TRACK_TOP: f32 = css(6.0);
  pub const SLIDER_TRACK_HEIGHT: f32 = css(16.0);
  /// Rayons des extrémités et du côté du curseur (`styles.css:7570`, `7571`).
  pub const SLIDER_OUTER_RADIUS: f32 = css(8.0);
  pub const SLIDER_INNER_RADIUS: f32 = css(3.0);
  /// Curseur : `width: 6px` au repos, `4px` pressé (`styles.css:7574`, `7575`) ; iced demande des pixels entiers.
  pub const SLIDER_THUMB: u16 = 6;
  pub const SLIDER_THUMB_PRESSED: u16 = 4;
  /// Le segment s'arrête à `8px` du centre du curseur (`styles.css:7569`, `7570`).
  pub const SLIDER_GAP: f32 = css(8.0);
  /// `.sl-inactive` : 16 % de `on-surface` (`styles.css:7571`).
  pub const SLIDER_INACTIVE_SHARE: f32 = 0.16;

  /// `.beta-badge` : `10px x --font-scale`, graisse 700 (`index.html:2999`), `padding: 2px 6px`, `margin-left: 8px`, rayon XS (`styles.css:3537`).
  pub const BADGE_SIZE: f32 = css(10.0 * FONT_SCALE);
  pub const BADGE_PADDING_Y: f32 = css(2.0);
  pub const BADGE_PADDING_X: f32 = css(6.0);
  pub const BADGE_MARGIN: f32 = css(8.0);
  pub const BADGE_RADIUS: f32 = css(4.0);

  /// Panneau Enhancer ouvert : `.hq-panel-content` `gap: 10px; padding-top: 10px` (`styles.css:7730`).
  pub const HQ_GAP: f32 = css(10.0);
  /// `.hq-label` : `12px x --font-scale`, graisse 600 (`styles.css:7741`).
  pub const HQ_LABEL_SIZE: f32 = css(12.0 * FONT_SCALE);
  /// `.hq-slider-group` : `padding: 8px 12px; border-radius: 12px; gap: 6px` (`styles.css:7765`).
  pub const HQ_GROUP_PADDING_Y: f32 = css(8.0);
  pub const HQ_GROUP_PADDING_X: f32 = css(12.0);
  pub const HQ_GROUP_RADIUS: f32 = css(12.0);
  pub const HQ_GROUP_GAP: f32 = css(6.0);
  /// `#hq-engine-trigger` : `min-height: 44px; padding: 8px 12px; border-radius: 12px` (`styles.css:7763`).
  pub const HQ_ENGINE_HEIGHT: f32 = css(44.0);

  /// `.eq-row` : `grid-template-columns: 44px 1fr 56px; gap: 12px; padding: 12px 16px` (`styles.css:7769`).
  pub const EQ_NAME_WIDTH: f32 = css(44.0);
  pub const EQ_VALUE_WIDTH: f32 = css(56.0);
  pub const EQ_ROW_GAP: f32 = css(12.0);
  pub const EQ_ROW_PADDING_Y: f32 = css(12.0);
  pub const EQ_ROW_PADDING_X: f32 = css(16.0);
  /// `.eq-name` `13px x --font-scale`, `.eq-val` `12px x --font-scale` (`styles.css:6253`, `6254`).
  pub const EQ_VALUE_SIZE: f32 = css(12.0 * FONT_SCALE);
  /// `.eq-section-10band` : `margin-top: 14px; padding-top: 12px`, filet de 1 px `outline-variant` (`styles.css:7778`).
  pub const EQ_SECTION_MARGIN: f32 = css(14.0);
  pub const EQ_SECTION_PADDING: f32 = css(12.0);
  /// `.eq-section-header` : `margin-bottom: 8px` (`styles.css:7779`).
  pub const EQ_HEADER_GAP: f32 = css(8.0);
  /// `.btn-outline-pill` : `height: 40px; padding: 8px 20px`, `14px x --font-scale`, graisse 500 (`styles.css:3323`).
  pub const PILL_HEIGHT: f32 = css(40.0);
  pub const PILL_PADDING_X: f32 = css(20.0);
  pub const PILL_TEXT: f32 = css(14.0 * FONT_SCALE);
  /// `.eq-faders-container` : `gap: 8px` (`styles.css:7780`).
  pub const EQ_FADERS_GAP: f32 = css(8.0);
  /// `.eq-scale` : hauteur 180 px, `padding: 6px 4px 6px 0`, `11px x --font-scale` (`styles.css:7781`) ; largeur mesurée sur `32d-audio-eq.png`.
  pub const EQ_FADER_HEIGHT: f32 = css(180.0);
  pub const EQ_SCALE_WIDTH: f32 = css(27.0);
  pub const EQ_SCALE_PADDING_Y: f32 = css(6.0);
  pub const EQ_SCALE_PADDING_RIGHT: f32 = css(4.0);
  pub const EQ_SCALE_SIZE: f32 = css(11.0 * FONT_SCALE);
  /// `.eq-grid-line` : 1 px, `opacity: 0.5`, aux `14px` du haut et du bas (`styles.css:7784`).
  pub const EQ_GRID_INSET: f32 = css(14.0);
  pub const EQ_GRID_OPACITY: f32 = 0.5;
  /// `.eq-bands` : `gap: 6px` (`styles.css:7789`).
  pub const EQ_BANDS_GAP: f32 = css(6.0);
  /// `.eq-band-range` : piste de 4 px, curseur de 6 x 20 px (`styles.css:7796`, `7797`).
  pub const EQ_RAIL: f32 = css(4.0);
  pub const EQ_THUMB_LENGTH: u16 = 6;
  pub const EQ_THUMB_WIDTH: f32 = css(20.0);
  pub const EQ_THUMB_RADIUS: f32 = css(3.0);
  /// `.eq-band-freq` : `11px x --font-scale`, `margin-top: 4px` (`styles.css:7798`).
  pub const EQ_FREQ_SIZE: f32 = css(11.0 * FONT_SCALE);
  pub const EQ_FREQ_MARGIN: f32 = css(4.0);

  /// `.sleep-timer-options` : `gap: 8px; margin-top: 12px` (`styles.css:9865`).
  pub const SLEEP_GAP: f32 = css(8.0);
  pub const SLEEP_MARGIN: f32 = css(12.0);
  /// `.sleep-timer-hint` : `margin-top: 10px` (`styles.css:9871`).
  pub const SLEEP_HINT_MARGIN: f32 = css(10.0);

  /// `.audio-menu-card .settings-footer` : `padding: 8px 12px 4px`, aligné à droite (`styles.css:7800`).
  pub const FOOTER_PADDING_TOP: f32 = css(8.0);
  pub const FOOTER_PADDING_X: f32 = css(12.0);
  pub const FOOTER_PADDING_BOTTOM: f32 = css(4.0);

  /// `connectedExpand 320ms cubic-bezier(0.2, 0, 0, 1) both` : le haut de la carte monte du bas (`clip-path: inset(100% ...)` vers `inset(0 ...)`,
  /// `styles.css:7585`, `7589`). Deux images clés : la courbe s'applique à tout le trajet.
  pub const EXPAND_MS: f32 = 320.0;
  pub const EXPAND_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `.connected-closing` : `connectedCollapse 200ms cubic-bezier(0.2, 0, 0, 1) both`, inverse du dévoilement (`styles.css:7586`, `7590`).
  /// `closeConnected` retire la carte 200 ms après (`app.js:14963-14967`).
  pub const COLLAPSE_MS: f32 = 200.0;
  pub const COLLAPSE_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `connectedContentIn 320ms cubic-bezier(0.2, 0, 0, 1) both` sur l'en-tête et `.audio-modal-layout` : `0%, 30%` opacité 0 et `translateY(8px)`,
  /// `100%` à l'identité (`styles.css:7587`, `7591`). La courbe s'applique au segment 30 % - 100 %.
  pub const CONTENT_IN_MS: f32 = 320.0;
  pub const CONTENT_HOLD: f32 = 0.3;
  pub const CONTENT_OFFSET: f32 = css(8.0);
  pub const CONTENT_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `connectedContentOut 120ms cubic-bezier(0.2, 0, 0, 1) both` : opacité 1 vers 0 et `translateY(0)` vers `translateY(4px)` (`styles.css:7588`, `7592`).
  pub const CONTENT_OUT_MS: f32 = 120.0;
  pub const CONTENT_OUT_OFFSET: f32 = css(4.0);
  pub const CONTENT_OUT_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `.audio-panel.active` : `audioPanelIn 0.22s cubic-bezier(0.2, 0, 0, 1)`, de `opacity: 0; translateY(6px)` à l'identité (`styles.css:7679-7680`).
  pub const PANEL_MS: f32 = 220.0;
  pub const PANEL_OFFSET: f32 = css(6.0);
  pub const PANEL_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `#sleep-timer-status.active::before` : point de 6 px et `gap: 6px` avec le texte (`styles.css:9881-9893`).
  pub const DOT_SIZE: f32 = css(6.0);
  pub const DOT_GAP: f32 = css(6.0);
  /// `sleepDot 2s ease-in-out infinite` : `0%, 100%` `opacity: 0.35; scale(0.75)`, `50%` `opacity: 1; scale(1)` (`styles.css:9887-9898`).
  /// `ease-in-out` s'applique à chaque segment.
  pub const DOT_PERIOD_MS: f32 = 2000.0;
  pub const DOT_CURVE: [f32; 4] = [0.42, 0.0, 0.58, 1.0];
  pub const DOT_MIN_OPACITY: f32 = 0.35;
  pub const DOT_MIN_SCALE: f32 = 0.75;
}

/// Fonds sombres de Riff au repos (`--md-sys-color-*-solid`, `--page-solid`).
pub mod surface {
  use super::Color;

  /// Page (`--page-solid`, `surface-dim`).
  pub const PAGE: Color = Color::from_rgb8(0x12, 0x12, 0x12);
  /// `surface-container-low`.
  pub const LOW: Color = Color::from_rgb8(0x1a, 0x1a, 0x1a);
  /// `surface-container`.
  pub const CONTAINER: Color = Color::from_rgb8(0x21, 0x21, 0x21);
  /// `surface-container-high`.
  pub const HIGH: Color = Color::from_rgb8(0x29, 0x29, 0x29);
}

/// Accent de repos : lavande `rgb(180, 159, 224)`, texte `#2c1d49`.
pub mod accent {
  use super::Color;

  pub const REST: Color = Color::from_rgb8(180, 159, 224);
  pub const ON_REST: Color = Color::from_rgb8(0x2c, 0x1d, 0x49);
}

/// Splash de démarrage (`runRiffSplash`, `app.js:165-215` ; `#splash`, `styles.css:9695-9700`). Durées en ms, courbes `cubic-bezier`.
pub mod splash {
  /// Durée du tracé et du zoom du logo (`app.js:199` et `app.js:206`, `duration: 920`).
  pub const DRAW_MS: f32 = 920.0;
  /// Courbe du tracé et du zoom, appliquée au progrès global puis interpolation linéaire entre images clés (`app.js:199`, `easing`).
  pub const DRAW_CURVE: [f32; 4] = [0.22, 1.0, 0.36, 1.0];
  /// Position de l'image clé intermédiaire du tracé (`app.js:197`, `offset: 0.65`).
  pub const KEY_MIDDLE: f32 = 0.65;
  /// `fill-opacity` à l'image clé intermédiaire (`app.js:197`).
  pub const FILL_MIDDLE: f32 = 0.15;
  /// `stroke-opacity` à la fin (`app.js:198`).
  pub const STROKE_END: f32 = 0.25;
  /// Échelle initiale de l'hôte du logo (`app.js:204`, `scale(1.22)`).
  pub const SCALE_FROM: f32 = 1.22;
  /// Opacité initiale de l'hôte du logo (`app.js:204`, `opacity: 0.2`).
  pub const OPACITY_FROM: f32 = 0.2;
  /// Épaisseur du trait, en unités du viewBox (`app.js:188`, `strokeWidth = '6px'`).
  pub const STROKE_WIDTH: f32 = 6.0;
  /// Taille du viewBox de `riff.svg` (`assets/riff.svg:1`, `viewBox="0 0 578 555"`).
  pub const VIEW_BOX: [f32; 2] = [578.0, 555.0];
  /// Longueur du chemin retenue par Riff (`app.js:186`, `Math.ceil(getTotalLength())`, mesurée à 3716 : `MOTION.md`).
  pub const PATH_LENGTH: f32 = 3716.0;
  /// Côté du logo, en part du plus petit côté de la fenêtre (`styles.css:9697`, `26vmin`).
  pub const LOGO_VMIN: f32 = 0.26;
  /// Durée du fondu de sortie (`styles.css:9695`, `transition: opacity .45s`).
  pub const FADE_MS: f32 = 450.0;
  /// Courbe du fondu de sortie (`styles.css:9695`).
  pub const FADE_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// Délai entre la classe `hide` et le retrait du calque (`app.js:174`, `setTimeout(() => splash.remove(), 420)`).
  pub const REMOVE_AFTER_HIDE_MS: f32 = 420.0;
}

/// Transition de vue et survols (`07-changement-vue`, `08a-survol-carte`, `08b-survol-bouton`). Durées en ms, courbes `cubic-bezier`.
pub mod motion {
  use super::css;

  /// `.content-view.active` : `m3ContainerOpen 400ms cubic-bezier(0.2, 0.0, 0, 1.0) both` (`styles.css:1001`).
  pub const VIEW_MS: f32 = 400.0;
  pub const VIEW_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `@keyframes m3ContainerOpen` : `from { opacity: 0; transform: translateY(16px) scale(0.96) }` (`styles.css:1009`).
  pub const VIEW_OFFSET: f32 = css(16.0);
  pub const VIEW_SCALE_FROM: f32 = 0.96;

  /// `--hl`, `background-color`, `color` : `.18s ease` (`styles.css:7509`, qui l'emporte par `!important` sur `70ms`/`100ms` de
  /// `styles.css:131-147`). Mesuré sur `08b-survol-bouton` : fin du fondu à 180 ms.
  pub const HOVER_MS: f32 = 180.0;
  /// `ease` : `cubic-bezier(0.25, 0.1, 0.25, 1)`.
  pub const HOVER_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
  /// `border-radius .22s cubic-bezier(.2, 0, 0, 1)` (`styles.css:7509`).
  pub const RADIUS_MS: f32 = 220.0;
  pub const RADIUS_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `.sp-card-cover { transition: transform 180ms ease }` (`styles.css:9371-9379`), `translateY(-2px)` au survol (`9380`).
  pub const CARD_MS: f32 = 180.0;
  pub const CARD_CURVE: [f32; 4] = HOVER_CURVE;
  /// `html` et `body` : `transition: background-color var(--transition-emphasized), color var(--transition-emphasized)` ; la variable
  /// vaut `400ms cubic-bezier(0.2, 0, 0, 1)` (`styles.css:80`, `112-122`). Mesuré sur `13-theme-titre` : fond de `#0f0f0f` à `#0d0d0d`,
  /// départ à 308 ms de l'horloge de `samples.json`, fin à 708,6 ms.
  pub const THEME_BODY_MS: f32 = 400.0;
  pub const THEME_BODY_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `.btn-play-pause` : `background-color 150ms ease !important` (`styles.css:3059`). Mesuré : de `rgb(182, 198, 201)` à
  /// `rgb(212, 172, 170)` entre 308 et 460 ms.
  pub const THEME_PLAY_MS: f32 = 150.0;
  pub const THEME_PLAY_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
}

/// Onde au clic, état `.pressed` et survol des boutons génériques (scènes `65*`). Durées en ms, courbes `cubic-bezier`.
pub mod touch {
  use super::css;

  /// `.ripple { animation: rippleOut .55s cubic-bezier(.2, 0, 0, 1) forwards }` (`styles.css:7503`). Une seule paire d'images clés
  /// (`0%` et `100%`, `styles.css:7504`) : la courbe porte sur tout le trajet, pour l'échelle comme pour l'opacité.
  pub const RIPPLE_MS: f32 = 550.0;
  pub const RIPPLE_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `@keyframes rippleOut` : `0% { transform: scale(0); opacity: .16 }` vers `100% { scale(1); opacity: 0 }` (`styles.css:7504`).
  pub const RIPPLE_ALPHA: f32 = 0.16;
  /// `.ripple { filter: blur(2px) }` (`styles.css:7503`) : écart type du flou gaussien, en pixels CSS.
  pub const RIPPLE_BLUR: f32 = css(2.0);
  /// Décalages (en écarts types) des cinq disques qui approchent le bord flou : quantiles 10, 30, 50, 70 et 90 % d'une loi normale,
  /// chaque disque portant le même cinquième de l'opacité.
  pub const RIPPLE_EDGE: [f32; 5] = [-1.2816, -0.5244, 0.0, 0.5244, 1.2816];
  /// `PRESS_MIN = 180` : durée minimale de la classe `.pressed` (`app.js:2607`).
  pub const PRESS_MIN_MS: f32 = 180.0;
  /// `.pressed { background-color: on-surface 12 %; transition: background-color 150ms ease }` (`styles.css:6239-6246`, `149-156`) est battue
  /// par `:is(.btn-pill-primary, ..., .btn-icon-pill, ...)` : `background-color .18s ease !important` (`styles.css:7508-7509`). Tous les boutons
  /// de riff-rs sont de cette liste, donc 180 ms `ease`, comme `motion::HOVER`.
  pub const PRESS_MS: f32 = 180.0;
  pub const PRESS_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
  /// Fond de `.pressed` : `on-surface` à 12 % (`styles.css:6244`), puis `--hl: 12%` en calque (`styles.css:7523`).
  pub const PRESS_SHARE: f32 = 0.12;
  /// `--hl` au survol des boutons : 8 % (`styles.css:7516`).
  pub const HOVER_SHARE: f32 = 0.08;
  /// `.setting-item:hover { --hl: 4% }` (`styles.css:7520`, dans `@media (hover: hover)`, `styles.css:7517`) : `on-surface` à 4 % sur le fond
  /// de la rangée. `.setting-item` est dans la liste `:is(...)` de `styles.css:7507` : `--hl` suit `.18s ease` (`styles.css:7509`, comme
  /// `motion::HOVER`) et `body.perf` coupe la transition (`styles.css:7541`). Pas de rayon animé ni d'onde : la liste du rayon (`7511-7515`)
  /// ne contient pas `.setting-item`.
  pub const SETTING_HOVER_SHARE: f32 = 0.04;
  /// Rayon des pilules : `28px` au repos, `18px` au survol (`styles.css:7511`, `7519`), en pixels CSS.
  pub const PILL_REST: f32 = css(28.0);
  pub const PILL_HOVER: f32 = css(18.0);
  /// Boutons ronds : `50%` au repos, `40%` au survol (`styles.css:7514`, `7520`).
  pub const ROUND_REST: f32 = 0.5;
  pub const ROUND_HOVER: f32 = 0.4;
  /// `.btn-pill-primary` et `.btn-outline-pill` : `height: 40px` (`styles.css:3314`, `3329`) ; `border-radius: 28px` se borne alors à 20 px.
  pub const PILL_HEIGHT: f32 = css(40.0);
}

/// Entrées `m3FadeUp` (`styles.css:1016-1019`) : `from { opacity: 0; transform: translateY(10px) }` vers l'état de repos, courbe
/// `cubic-bezier(0.2, 0, 0, 1)` pour tous les usages. Durées en ms.
pub mod fade_up {
  use super::css;

  pub const CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `translateY(10px)` de l'image de départ (`styles.css:1017`).
  pub const OFFSET: f32 = css(10.0);
  /// `.track-row` : `animation: m3FadeUp 350ms ... backwards` (`styles.css:1590`).
  pub const ROW_MS: f32 = 350.0;
  /// `.track-row:nth-child(n) { animation-delay }` : 30 ms par rang, rangs 1 à 8 (`styles.css:1605-1612`), 0 au-delà.
  pub const ROW_STEP_MS: f32 = 30.0;
  pub const ROW_STAGGERED: usize = 8;
  /// `.track-row { transform: none !important }` (`styles.css:1588`) : une déclaration `!important` d'auteur l'emporte sur une
  /// animation, la translation n'est jamais appliquée, seule l'opacité s'anime.
  pub const ROW_OFFSET: f32 = 0.0;
  /// `.search-artist-card` : `m3FadeUp 260ms ... both` (`styles.css:1507`), sans décalage.
  pub const ARTIST_MS: f32 = 260.0;
  /// `.playlist-pick-item` : `m3FadeUp 200ms ... backwards` (`styles.css:3995`), décalage `idx * 25ms` (`app.js:13543`).
  pub const PICK_MS: f32 = 200.0;
  pub const PICK_STEP_MS: f32 = 25.0;
}

/// Maillage de fond `.bg-mesh-layer` et aperçu `.mesh-preview-bg` (`meshFloat`). Sans capture motion : valeurs lues dans `styles.css`
/// et `app.js` de Riff.
pub mod mesh {
  use super::css;

  /// `.bg-mesh-layer { inset: -40px }` : le calque dépasse la fenêtre de 40 px de chaque côté (`styles.css:160`).
  pub const INSET: f32 = css(40.0);
  /// `.mesh-preview-bg { inset: -20px }` dans `.mesh-preview-box` (`styles.css:6891`).
  pub const PREVIEW_INSET: f32 = css(20.0);
  /// `.mesh-preview-box` : 280 x 160, `border-radius: var(--md-shape-corner-m)` (12 px), bordure de 1 px (`styles.css:6878-6888`, `71`).
  pub const PREVIEW_WIDTH: f32 = css(280.0);
  pub const PREVIEW_HEIGHT: f32 = css(160.0);
  pub const PREVIEW_RADIUS: f32 = css(12.0);
  pub const PREVIEW_BORDER: f32 = css(1.0);

  /// `@keyframes meshFloat` : `scale(s) translate(x%, y%)` à 0 %, 50 % et 100 % (`styles.css:169-173`). `(échelle, x, y)`, les
  /// décalages en fraction du calque (le pourcentage d'un `translate` se rapporte à la boîte de l'élément), centre de l'échelle au
  /// centre du calque (`transform-origin` par défaut, aucun autre dans `styles.css`).
  pub const FLOAT_KEYS: [(f32, f32, f32); 3] =
    [(1.0, 0.0, 0.0), (1.06, 0.02, 0.015), (1.03, -0.015, 0.01)];
  /// `animation: meshFloat var(--mesh-speed) ease-in-out infinite alternate` (`styles.css:166`) : `ease-in-out` s'applique à chaque
  /// segment entre deux images clés (`cubic-bezier(0.42, 0, 0.58, 1)`).
  pub const FLOAT_CURVE: [f32; 4] = [0.42, 0.0, 0.58, 1.0];

  /// Alphas des deux ellipses `updateMeshGradient`, thème sombre puis clair, avant la luminosité (`app.js:1703-1704`).
  pub const ALPHA_DARK: (f32, f32) = (0.55, 0.45);
  pub const ALPHA_LIGHT: (f32, f32) = (0.22, 0.18);
  /// `transparent 70 %` et `transparent 65 %` des deux `radial-gradient` (`app.js:1712-1713`).
  pub const STOP_FIRST: f32 = 0.70;
  pub const STOP_SECOND: f32 = 0.65;
  /// `linear-gradient(to bottom, transparent 35%, var(--md-sys-color-background) 85%)` (`app.js:1711`).
  pub const RAMP_FROM: f32 = 0.35;
  pub const RAMP_TO: f32 = 0.85;

  /// Fondu des calques A et B : `.bg-mesh-layer` demande `500ms cubic-bezier(0.2, 0, 0, 1)` (`styles.css:161`) mais `#bg-mesh-a,
  /// #bg-mesh-b { transition: opacity 500ms ease }` (`styles.css:9186-9193`) l'emporte par spécificité (identifiant). En pratique ce
  /// fondu ne tourne jamais : `.bg-mesh-layer[style*="opacity: 0"] { display: none !important }` (`styles.css:11750`) masque le calque
  /// dès que `updateMeshGradient` pose `opacity: 0` en ligne (`app.js:1675-1677`, `1721`), et un calque qui passe de `display: none` à
  /// affiché ne lance pas de transition. Les changements de clé, l'activation et la coupure sont donc des coupures nettes.
  pub const FADE_MS: f32 = 500.0;
  pub const FADE_CURVE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
}

/// Défilement des paroles et transition de la ligne active (`15-paroles`). Durées en ms, courbes `cubic-bezier`.
pub mod lyrics {
  /// `.lyric-line { opacity: 0.48 }` : opacité d'une ligne inactive (`styles.css:2558`).
  pub const LINE_OPACITY: f32 = 0.48;
  /// `applyLyricsEase` : `--lyric-duration` vaut `0.48 s` pour `inout`, `0.45 s` pour les autres courbes, divisée par `riff_lyrics_speed`
  /// (`app.js:6160-6161`) ; elle pilote `transition: opacity, transform` de `.lyric-line` (`styles.css:2562`).
  pub const DURATION_INOUT_MS: f32 = 480.0;
  pub const DURATION_OTHER_MS: f32 = 450.0;
  /// `animateLyricsScroll` : `Math.max(160, Math.min(480, 320 + |delta| * 0.28)) / speed` (`app.js:6181`).
  pub const SCROLL_BASE_MS: f32 = 320.0;
  pub const SCROLL_PER_PX: f32 = 0.28;
  pub const SCROLL_MIN_MS: f32 = 160.0;
  pub const SCROLL_MAX_MS: f32 = 480.0;
  /// `LYRIC_EASINGS[...].css` : courbes `--lyric-ease` appliquées aux transitions CSS (`app.js:6139-6147`). `linear` : `None`.
  pub const CURVE_INOUT: [f32; 4] = [0.786, 0.648, 0.0, 0.935];
  pub const CURVE_QUAD: [f32; 4] = [0.45, 0.0, 0.55, 1.0];
  pub const CURVE_SINE: [f32; 4] = [0.37, 0.0, 0.63, 1.0];
  pub const CURVE_OUT: [f32; 4] = [0.22, 1.0, 0.36, 1.0];
  pub const CURVE_OUTSINE: [f32; 4] = [0.39, 0.575, 0.565, 1.0];
  pub const CURVE_EXPO: [f32; 4] = [0.16, 1.0, 0.3, 1.0];
}

/// Notification gouttelette (`.riff-toast`, `styles.css:4622-4723`, `app.js:455-549`), en pixels écran.
pub mod toast {
  use super::{Color, css};

  /// Décalage vertical du texte, en pixels écran. Mesuré sur `03-toast` : le texte de Riff occupe les lignes 12 à 24 de la
  /// zone de capture, celui d'iced 13 à 25 (Chromium et iced ne placent pas la ligne de texte de la même façon).
  pub const TEXT_NUDGE: f32 = -1.0;
  /// `--trb` : rayon des coins concaves `::before`/`::after` (`styles.css:4648`, `4669-4671`).
  pub const FILLET: f32 = css(18.0);
  /// `padding: 10px 22px 12px 22px` (`styles.css:4653`).
  pub const PADDING_TOP: f32 = css(10.0);
  pub const PADDING_BOTTOM: f32 = css(12.0);
  pub const PADDING_X: f32 = css(22.0);
  /// `min-width` et `max-width` (`styles.css:4651-4652`).
  pub const MIN_WIDTH: f32 = css(220.0);
  pub const MAX_WIDTH: f32 = css(460.0);
  /// `border-radius: 0 0 22px 22px` (`styles.css:4654`).
  pub const RADIUS: f32 = css(22.0);
  /// `gap` entre l'icône et le texte (`styles.css:4660`).
  pub const GAP: f32 = css(12.0);
  /// SVG de l'icône : 18 px, viewBox 24 (`app.js:488-497`).
  pub const ICON: f32 = css(18.0);
  /// `.toast-text` : 14 px, graisse 500 (`styles.css:4683-4691`).
  pub const TEXT: f32 = css(14.0);
  /// `.toast-text { max-width: 420px }` (`styles.css:4689`).
  pub const TEXT_MAX_WIDTH: f32 = css(420.0);
  /// Couleur de l'icône `success` (`styles.css:4707`) et `warning` (`4715`).
  pub const SUCCESS: Color = Color::from_rgb8(0x38, 0x8e, 0x3c);
  pub const WARNING: Color = Color::from_rgb8(0xf9, 0xa8, 0x25);
  /// `.riff-share-spinner-toast` : boîte de 44 px, rayon bas de 14 px (`styles.css:9707-9714`).
  pub const SPINNER_BOX: f32 = css(44.0);
  pub const SPINNER_RADIUS: f32 = css(14.0);
  /// `.g-spinner` du toast : 20 px (`app.js:10481`).
  pub const SPINNER_SIZE: f32 = css(20.0);
  /// `box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4)` (`styles.css:9715`) : décalage, flou, opacité.
  pub const SPINNER_SHADOW_OFFSET: f32 = css(4.0);
  pub const SPINNER_SHADOW_BLUR: f32 = css(16.0);
  pub const SPINNER_SHADOW_ALPHA: f32 = 0.4;
  /// `dropletToastIn` : 340 ms, `cubic-bezier(0.22, 1.35, 0.36, 1)`, image clé à 65 % (`styles.css:4666`, `4672`).
  pub const ENTER_MS: u64 = 340;
  pub const ENTER_CURVE: [f32; 4] = [0.22, 1.35, 0.36, 1.0];
  pub const ENTER_KEY: f32 = 0.65;
  /// `dropletToastOut` : 200 ms, `cubic-bezier(0.4, 0, 1, 1)` (`styles.css:4668`, `4673`).
  pub const LEAVE_MS: u64 = 200;
  pub const LEAVE_CURVE: [f32; 4] = [0.4, 0.0, 1.0, 1.0];
  /// Retrait du toast spinner, 180 ms après `.leaving` (`app.js:10485`).
  pub const SPINNER_LEAVE_MS: u64 = 180;
  /// Durée restante minimale après un survol : `if (remaining < 500) remaining = 500` (`app.js:537`).
  pub const HOVER_FLOOR_MS: u64 = 500;
}

/// Halo de la pochette de la barre de lecture `#m3c-canvas` : boucle `m3cLoop` et dessin `m3cDraw` (`app.js:16768-16990`).
pub mod halo {
  /// `m3cLoop` avance d'une étape par image d'écran (`requestAnimationFrame`) : les approches de survol et de vitesse sont
  /// `valeur += (cible - valeur) * facteur` à chaque image (`app.js:16961`, `16968`), sans lien avec le temps écoulé.
  /// Base de 240 Hz (écran et captures de référence, échantillons toutes les 4,1 ms). riff-rs les
  /// calcule sur le temps : après `dt` ms, `reste = (1 - facteur)^(dt / FRAME_MS)`, ce qui redonne l'étape de Riff à 240 Hz et
  /// reste cohérent à 60 Hz. Les seuils d'arrêt (valeur collée à la cible) sont gardés.
  pub const FRAME_MS: f64 = 1000.0 / 240.0;
  /// Survol : `hoverProgress` vers 1 ou 0, facteur 0,2, arrêt sous 0,002 (`app.js:16958-16964`).
  pub const HOVER_FACTOR: f32 = 0.2;
  pub const HOVER_EPSILON: f32 = 0.002;
  /// `hoverScale = 1 + hoverProgress * 0.16` et `hoverLift = hoverProgress * 4` pixels CSS (`app.js:16863-16864`).
  pub const HOVER_SCALE: f32 = 0.16;
  pub const HOVER_LIFT: f32 = 4.0;
  /// Vitesse de rotation `rotRate` vers `targetRate` (1 en lecture, 0,25 en pause), facteur 0,1, arrêt sous 0,01
  /// (`app.js:16791`, `16966-16971`).
  pub const RATE_FACTOR: f32 = 0.1;
  pub const RATE_EPSILON: f32 = 0.01;
  pub const RATE_PLAYING: f32 = 1.0;
  pub const RATE_PAUSED: f32 = 0.25;
  /// Un tour en 14 s à la vitesse 1 (`baseSpeed`, `app.js:16977`), pas borné à 64 ms (`app.js:16974`).
  pub const TURN_MS: f32 = 14_000.0;
  pub const MAX_STEP_MS: f32 = 64.0;
  /// `phase += 0.0075 * delta`, en lecture seulement (`app.js:16980`).
  pub const PHASE_PER_MS: f32 = 0.0075;
  /// `shapes.js:305-306` : une forme est tenue 2000 ms depuis le début de la période, la transition en prend 1250
  /// (`m3cShapeRadii`, `app.js:16856-16861`). Tolérance d'arrondi sur la somme des pas de 1000/240 ms.
  pub const HOLD_MS: f64 = 2000.0;
  pub const MORPH_MS: f64 = 1250.0;
  pub const CLOCK_TOLERANCE_MS: f64 = 1e-6;
  /// `m3cDraw` : étapes de l'anneau, départ en haut, onde de 26 lobes d'amplitude 1,5 px (`app.js:16921`, `16926`, `16947`).
  pub const RING_STEPS: usize = 240;
  pub const WAVE_LOBES: f32 = 26.0;
  pub const WAVE_AMPLITUDE: f32 = 1.5;
  /// `setLineDash([0.1, 5.5])` à `globalAlpha = 0.4` pour la partie restante (`app.js:16930`, `16932`) : un point rond tous les
  /// 5,6 px de l'anneau, coupé à la main (pas de `LineDash`).
  pub const DASH_LENGTH: f32 = 0.1;
  pub const DASH_PITCH: f32 = 5.6;
  pub const REST_ALPHA: f32 = 0.4;
  /// Image : `ceil(cover * 1,06)` (`app.js:16910`).
  pub const IMAGE_SCALE: f32 = 1.06;
}

/// Spinners à forme morphante `.m3-btn-shape-spinner` (`app.js:9944-9980`, `shapes.js:465-475`), bouton lecture compris.
pub mod shape_spinner {
  use super::css;

  /// `STEP_MS` : une forme se fond dans la suivante en 1,2 s (`app.js:9950`).
  pub const STEP_MS: u64 = 1200;
  /// Tour complet en 3 s sur l'horloge de la page : `(now % 3000) / 3000` (`app.js:9963`).
  pub const TURN_MS: u64 = 3000;
  /// Une forme tirée évite les 3 dernières de l'historique, borné à 10 (`shapes.js:467-471`).
  pub const RECENT: usize = 3;
  pub const HISTORY: usize = 10;
  /// Un échantillon sur quatre est tracé (`app.js:9965`).
  pub const SAMPLE_STEP: usize = 4;
  /// `viewBox="-180 -180 360 360"` (`index.html:2828`).
  pub const VIEW_BOX: f32 = 360.0;
  /// `#playbar-spinner` : 30 x 30 (`index.html:2828`).
  pub const PLAY_SIZE: f32 = css(30.0);
  /// Anti-rebond avant `.buffering` et le spinner (`app.js:14053`).
  pub const PLAY_DEBOUNCE_MS: u64 = 500;
  /// `transition: opacity 260ms, transform 260ms cubic-bezier(0.2, 0, 0, 1)` de l'icône (`styles.css:3137`, `3147`).
  pub const PLAY_FADE_MS: u64 = 260;
  pub const PLAY_FADE_CURVE: [f32; 4] = [0.2, 0.0, 0.0, 1.0];
  /// `.buffering #play-icon { transform: scale(0.65) }` (`styles.css:3150-3152`).
  pub const PLAY_ICON_SCALE: f32 = 0.65;
}

/// `.g-spinner` (`styles.css:6248-6251`) : valeurs de `g-rot` et `g-dash`, dans le viewBox de 48 de Riff.
pub mod g_spinner {
  /// Durée de `g-rot` (linéaire) et de `g-dash`, en millisecondes (`styles.css:6248`, `6249`).
  pub const PERIOD_MS: u64 = 1400;
  /// `ease-in-out` : `cubic-bezier(0.42, 0, 0.58, 1)`, appliquée à chaque intervalle de `g-dash` (`styles.css:6249`).
  pub const DASH_CURVE: [f32; 4] = [0.42, 0.0, 0.58, 1.0];
  /// `viewBox="0 0 48 48"`, `r="20"`, `stroke-width: 4` (`app.js:10481`, `styles.css:6249`).
  pub const VIEW_BOX: f32 = 48.0;
  pub const RADIUS: f32 = 20.0;
  pub const STROKE: f32 = 4.0;
  /// Images clés de `g-dash` : `stroke-dasharray` 1 puis 90 (`styles.css:6251`).
  pub const DASH_START: f32 = 1.0;
  pub const DASH_END: f32 = 90.0;
  /// `stroke-dashoffset` à 50 % puis à 100 % (`styles.css:6251`).
  pub const OFFSET_MID: f32 = -35.0;
  pub const OFFSET_END: f32 = -124.0;
}
#[cfg(test)]
mod tests {
  use super::*;
  use crate::settings::{LegacyStorage, NativeSettings, ThemeMode};
  use crate::theme::ActiveTheme;
  use material_colors::color::Rgb;
  use serde_json::json;

  fn rgb8(color: Color) -> [u8; 3] {
    let [r, g, b, _] = color.into_rgba8();
    [r, g, b]
  }

  fn channels(color: Rgb) -> [u8; 3] {
    [color.red, color.green, color.blue]
  }

  /// Les quatre clés de thème du `storage.json` du profil capturé (`riff_theme_*`) : accent `#7342d7`,
  /// luminosité 66, intensité 85, mode `auto` (thème de l'album, accent au repos). Le `_note` de
  /// `tokens.json` annonce `#5247d1` et 20, mais ces valeurs ne redonnent pas les fonds mesurés.
  fn user_settings() -> NativeSettings {
    let storage = json!({
      "riff_theme_accent": "#7342d7",
      "riff_theme_intensity": "85",
      "riff_theme_brightness": "66",
      "riff_theme_mode": "auto",
    });
    NativeSettings::imported_from(&LegacyStorage::from_entries(storage.as_object().unwrap()))
  }

  #[test]
  fn the_user_settings_give_the_rest_tokens_of_riff() {
    let scheme = ActiveTheme::from_accent(&user_settings()).scheme;
    let expected = [
      ("primary", scheme.primary, [180, 159, 224]),
      ("on-primary", scheme.on_primary, [0x2c, 0x1d, 0x49]),
      (
        "on-primary-container",
        scheme.on_primary_container,
        [0xb4, 0x9f, 0xe0],
      ),
      ("secondary", scheme.secondary, [0xba, 0xaf, 0xd0]),
      (
        "secondary-container",
        scheme.secondary_container,
        [0x36, 0x31, 0x3f],
      ),
      (
        "on-secondary-container",
        scheme.on_secondary_container,
        [0xd5, 0xcf, 0xe3],
      ),
      ("tertiary", scheme.tertiary, [0xba, 0xaf, 0xd0]),
      (
        "tertiary-container",
        scheme.tertiary_container,
        [0x36, 0x31, 0x3f],
      ),
      ("background", scheme.background, [0x12; 3]),
      ("surface", scheme.surface, [0x14; 3]),
      ("surface-dim", scheme.surface_dim, [0x12; 3]),
      ("surface-bright", scheme.surface_bright, [0x3b; 3]),
      (
        "surface-container-lowest",
        scheme.surface_container_lowest,
        [0x0d; 3],
      ),
      (
        "surface-container-low",
        scheme.surface_container_low,
        [0x1a; 3],
      ),
      ("surface-container", scheme.surface_container, [0x21; 3]),
      (
        "surface-container-high",
        scheme.surface_container_high,
        [0x29; 3],
      ),
      (
        "surface-container-highest",
        scheme.surface_container_highest,
        [0x33; 3],
      ),
      ("on-surface", scheme.on_surface, [0xe2, 0xe2, 0xe9]),
      (
        "on-surface-variant",
        scheme.on_surface_variant,
        [0xc4, 0xc6, 0xd0],
      ),
      ("outline", scheme.outline, [0x8e, 0x90, 0x99]),
      (
        "outline-variant",
        scheme.outline_variant,
        [0x28, 0x2a, 0x33],
      ),
      ("error", scheme.error, [0xf2, 0xb8, 0xb5]),
      ("on-error", scheme.on_error, [0x60, 0x14, 0x10]),
    ];
    for (role, color, tokens_json) in expected {
      assert_eq!(channels(color), tokens_json, "--md-sys-color-{role}");
    }
    assert_eq!(channels(scheme.primary), rgb8(accent::REST));
    assert_eq!(channels(scheme.on_primary), rgb8(accent::ON_REST));
    assert_eq!(channels(scheme.surface_dim), rgb8(surface::PAGE));
    assert_eq!(channels(scheme.surface_container_low), rgb8(surface::LOW));
    assert_eq!(channels(scheme.surface_container), rgb8(surface::CONTAINER));
    assert_eq!(channels(scheme.surface_container_high), rgb8(surface::HIGH));
  }

  #[test]
  fn a_fresh_install_has_the_defaults_of_riff() {
    let settings = NativeSettings::default();
    assert_eq!(settings.theme_accent, "#5247D1");
    assert_eq!(
      (settings.theme_brightness, settings.theme_intensity),
      (20, 85)
    );
    assert_eq!(settings.theme_mode, ThemeMode::Dark);
    assert_eq!(settings.palette_style, "classic");
  }

  #[test]
  fn the_window_top_matches_the_measured_screen_sizes() {
    // `layout.json`, champ `px` : titlebar 37,8 ; window-btn-* 39,69 x 26,45 ; search-box 41,58 ; top-nav 60,47.
    assert!((shell::TITLEBAR_HEIGHT - 37.8).abs() < 1e-2);
    assert!((shell::WINDOW_BUTTON_WIDTH - 39.69).abs() < 1e-2);
    assert!((shell::WINDOW_BUTTON_HEIGHT - 26.45).abs() < 1e-1);
    assert!((shell::SEARCH_HEIGHT - 41.58).abs() < 1e-2);
    assert!((shell::TOP_NAV_HEIGHT - 60.47).abs() < 1e-1);
    assert!((shell::SEARCH_MAX_WIDTH - 529.19).abs() < 1e-1);
  }

  #[test]
  fn the_scale_matches_the_measured_screen_sizes() {
    assert!((text::BODY - 17.955).abs() < 1e-3);
    assert!((text::CARD_TITLE - 13.7025).abs() < 1e-3);
    assert!((radius::BUTTON - 13.23).abs() < 1e-3);
  }
}
