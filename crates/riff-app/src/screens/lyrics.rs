//! Panneau de paroles, vue plein écran, mini-paroles, éditeur et synchro manuelle.

use super::chrome::{tip, tips};
use super::sidebar::{faded as alpha, rgb};
use crate::app::{App, Message};
use crate::images::{ImageKey, SIZE_CARD, SIZE_HERO};
use crate::lyrics::layout::{LineBox, visible_lines};
use crate::lyrics::model::{WordState, split_parens};
use crate::lyrics::state::{self, LineLayout, SCROLL_ID, gradient_runs, lyric_font};
use crate::lyrics::tvk::{self, LineLook, TvkFont, TvkMessage, TvkSlider};
use crate::lyrics::update::{LyricsMessage, TVK_BAR_ZONE};
use crate::motion::{TvkPanelPose, ViewPose};
use crate::prefs::SettingsMessage;
use crate::prefs_visual::{VisualBool, VisualInt, VisualMessage};
use crate::theme::tokens::karaoke as k;
use crate::theme::{icons, tokens};
use crate::ui::PlayerMessage;

use crate::visuals::blur;
use crate::visuals::canvas::{PanelEars, TvkCanvas};
use crate::visuals::orbs::{GroupOpacity, Orbs};
use crate::visuals::settings::{TvkAlign, TvkAnim};
use crate::visuals::viz;
use crate::widgets::button::{filled, icon, text_button, tonal};
use crate::widgets::focus::focusable;
use crate::widgets::view_transition::{ScreenLayer, view_transition};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::image::FilterMethod;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::text::{LineHeight, Shaping, Span};
use iced::widget::{
  Column, Row, button, canvas, column, container, image, mouse_area, opaque, pick_list, rich_text,
  row, scrollable, slider, space, stack, svg, text, text_editor, toggler,
};
use iced::{
  Color, ContentFit, Element, Fill, Font, Gradient, Padding, Point, Radians, Rectangle, Shadow,
  Size, Theme, Vector, border, gradient, mouse,
};

/// `mask-image` de `.lyrics-container` : fondu sur 14 % en haut et en bas.
const MASK_SHARE: f32 = 0.14;
const VISIBLE_MARGIN: f32 = 200.0;
const SCRIM: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.5);
const CARD_WIDTH: f32 = 560.0;
const CARD_HEIGHT: f32 = 520.0;
const TRANSLATION_SIZE: f32 = 14.0;
/// Opacités de `.lyric-line`, `.lyric-word`, `.lyric-paren`.
const LINE_ALPHA: f32 = tokens::lyrics::LINE_OPACITY;
const WORD_ALPHA: f32 = 0.85;
const PAREN_ALPHA: f32 = 0.6;
const PAREN_ACTIVE_ALPHA: f32 = 0.82;
const UNSUNG_ALPHA: f32 = 0.38;
const MINI_FONT: f32 = 18.0;
const MINI_NEIGHBOURS: usize = 2;

fn lyric(message: LyricsMessage) -> Message {
  Message::Lyrics(message)
}

/// Style d'une surface de lignes.
#[derive(Debug, Clone, Copy)]
struct LineStyle {
  size: f32,
  font: Font,
  align: Horizontal,
  gap_min: f32,
  gap_max: f32,
  /// Espace entre deux lignes.
  spacing: f32,
  /// Opacité de la ligne entière (`opacity` de `.lyric-line`).
  opacity: f32,
  /// Opacité de groupe de la vue plein écran pendant `tvk-in` et `tvk-out` (1 au repos), multipliée à celle de la ligne.
  fade: f32,
  /// Vue plein écran : l'aspect des lignes suit l'animation choisie.
  tvk: bool,
  /// Pixels écran par pixel CSS des marges et des écarts fixes.
  scale: f32,
}

/// Aspect d'une ligne hors transition : les lignes inactives sont atténuées (`.lyric-line`).
fn rest_look(active: bool) -> LineLook {
  LineLook {
    opacity: if active { 1.0 } else { LINE_ALPHA },
    offset: 0.0,
    scale: 1.0,
  }
}

/// Interligne des lignes : `1.5` en plein écran, `1.45` ailleurs.
fn line_height(style: LineStyle) -> f32 {
  if style.tvk {
    state::TVK_LINE_HEIGHT
  } else {
    state::LINE_HEIGHT
  }
}

fn tvk_align(align: TvkAlign) -> Horizontal {
  match align {
    TvkAlign::Left => Horizontal::Left,
    TvkAlign::Center => Horizontal::Center,
    TvkAlign::Right => Horizontal::Right,
  }
}

/// Place une ligne ou une rangée selon l'alignement. Une ligne RTL à gauche se place à droite (voir `word_rows`).
fn place<'a>(
  content: impl Into<Element<'a, Message>>,
  align: Horizontal,
  rtl: bool,
) -> Element<'a, Message> {
  let content = content.into();
  match align {
    Horizontal::Center => container(content).center_x(Fill).into(),
    Horizontal::Right => container(content).align_right(Fill).into(),
    Horizontal::Left if rtl => container(content).align_right(Fill).into(),
    Horizontal::Left => content,
  }
}

fn word_color(
  colors: (Color, Color),
  state: Option<WordState>,
  active_line: bool,
  paren: bool,
  opacity: f32,
) -> Color {
  let (text_color, primary) = colors;
  if !active_line {
    let line = alpha(text_color, opacity * WORD_ALPHA);
    return if paren {
      alpha(line, PAREN_ALPHA)
    } else {
      line
    };
  }
  let color = match state {
    Some(WordState::Active) if !paren => primary,
    Some(WordState::Active | WordState::Past) => alpha(text_color, 0.88),
    _ => {
      if paren {
        alpha(text_color, PAREN_ACTIVE_ALPHA)
      } else {
        text_color
      }
    }
  };
  alpha(color, opacity)
}

/// Mots en tuiles (décision 1), rangée inversée en RTL.
fn word_rows<'a>(
  layout: &'a LineLayout,
  states: &[WordState],
  active_line: bool,
  colors: (Color, Color),
  style: LineStyle,
) -> Element<'a, Message> {
  let mut rows = Vec::with_capacity(layout.rows.len());
  for range in &layout.rows {
    let mut tiles: Vec<Element<'a, Message>> = Vec::with_capacity(range.len());
    for index in range.clone() {
      let word = &layout.words[index];
      let size = if word.paren {
        style.size * state::PAREN_SCALE
      } else {
        style.size
      };
      let color = word_color(
        colors,
        states.get(index).copied(),
        active_line,
        word.paren,
        style.opacity,
      );
      tiles.push(
        text(word.text.as_str())
          .size(size)
          .font(style.font)
          .line_height(LineHeight::Relative(line_height(style)))
          .shaping(Shaping::Advanced)
          .color(color)
          .into(),
      );
    }
    if layout.rtl {
      tiles.reverse();
    }
    let tiles = Row::from_vec(tiles)
      .spacing(state::WORD_GAP * style.scale)
      .align_y(Vertical::Bottom);
    // Pas d'`Alignment::Right` sur une ligne RTL : le texte disparaît.
    rows.push(place(tiles, style.align, layout.rtl));
  }
  Column::from_vec(rows).width(Fill).into()
}

/// Mode « gradient » : une couleur par caractère au niveau du front. Les parenthèses passent à la fin, plus petites.
fn gradient_line<'a>(
  text_value: &'a str,
  fill: f32,
  tightness: u32,
  rtl: bool,
  color: Color,
  style: LineStyle,
) -> Element<'a, Message> {
  let opacity = style.opacity;
  let split = split_parens(text_value);
  let shade = move |sung: f32| {
    alpha(
      color,
      (UNSUNG_ALPHA + (1.0 - UNSUNG_ALPHA) * sung) * opacity,
    )
  };
  let mut spans: Vec<Span<'a, (), Font>> = gradient_runs(&split.main, fill, tightness)
    .into_iter()
    .map(|(run, sung)| Span::new(run).color(shade(sung)))
    .collect();
  if !split.paren.is_empty() {
    let end = split.end_ratio.max(split.start_ratio + 0.1);
    let paren_fill = ((fill - split.start_ratio) / (end - split.start_ratio)).clamp(0.0, 1.0);
    let size = style.size * state::GRADIENT_PAREN_SCALE;
    spans.push(Span::new("  ").size(size));
    spans.extend(
      gradient_runs(&split.paren, paren_fill, tightness)
        .into_iter()
        .map(|(run, sung)| {
          Span::new(run)
            .size(size)
            .color(alpha(shade(sung), PAREN_ACTIVE_ALPHA))
        }),
    );
  }
  let line = rich_text(spans)
    .size(style.size)
    .font(style.font)
    .line_height(LineHeight::Relative(line_height(style)));
  // Les rangées d'une ligne qui passe à la ligne sont centrées dans leur boite (`text-align: center`).
  if style.tvk && style.align == Horizontal::Center && !rtl {
    return line.width(Fill).align_x(Horizontal::Center).into();
  }
  place(line, style.align, rtl)
}

fn gap_line<'a>(app: &App, active: bool, color: Color, style: LineStyle) -> Element<'a, Message> {
  let heights = if active {
    viz::gap_bars(
      &app.visuals.freq,
      app.playback.is_playing,
      style.gap_min,
      style.gap_max,
    )
  } else {
    [style.gap_min; 4]
  };
  let shown = alpha(color, style.opacity);
  let bar = |height: f32| {
    container(space())
      .width(3.0 * style.scale)
      .height(height)
      .style(move |_: &Theme| {
        container::Style::default()
          .background(shown)
          .border(border::rounded(2.0 * style.scale))
      })
      .into()
  };
  let bars: Vec<Element<'a, Message>> = heights.iter().map(|height| bar(*height)).collect();
  let content = row![
    Row::from_vec(bars)
      .spacing(3.0 * style.scale)
      .align_y(Vertical::Bottom),
    text("\u{266A}").size(14.0 * style.scale).color(shown)
  ]
  .spacing(6.0 * style.scale)
  .align_y(Vertical::Center);
  place(content, style.align, false)
}

fn line_view<'a>(
  app: &'a App,
  index: usize,
  line_box: LineBox,
  style: LineStyle,
) -> Element<'a, Message> {
  let state = &app.lyrics;
  let Some(lyrics) = state.lyrics.as_ref() else {
    return space().into();
  };
  let line = &lyrics.lines[index];
  let active = state.active == Some(index);
  let look = if style.tvk {
    app.tvk_look(index)
  } else {
    app.panel_look(index)
  };
  let style = LineStyle {
    size: style.size * look.scale,
    opacity: look.opacity * style.fade,
    ..style
  };
  let palette = app.theme.theme.palette();
  // `--md-sys-color-on-surface` : la couleur des paroles dans Riff. La vue plein écran la prend du schéma, le panneau garde `palette.text`.
  let text_color = if style.tvk {
    rgb(app.theme.scheme.on_surface)
  } else {
    palette.text
  };
  let colors = (text_color, palette.primary);
  let layout = &state.layout.lines[index];
  let body: Element<'a, Message> = if line.is_gap {
    gap_line(app, active, text_color, style)
  } else if layout.rows.is_empty() {
    let fill = if active {
      lyrics.gradient_fill(index, state.position)
    } else {
      0.0
    };
    gradient_line(
      &line.text,
      fill,
      app.settings.visual.lyrics_gradient_tightness,
      layout.rtl,
      text_color,
      style,
    )
  } else {
    let states = if state.word_mode {
      state.word_states(index)
    } else {
      Vec::new()
    };
    word_rows(layout, &states, active, colors, style)
  };
  let translation = state
    .translations
    .as_ref()
    .filter(|_| state.translate_on)
    .and_then(|items| items.get(index))
    .filter(|item| !item.is_empty());
  let content: Element<'a, Message> = match translation {
    Some(translated) => column![
      body,
      text(translated.as_str())
        .size(TRANSLATION_SIZE * style.scale)
        .shaping(Shaping::Advanced)
        .color(alpha(palette.text, 0.7 * style.fade))
    ]
    .width(Fill)
    .into(),
    None => body,
  };
  // Pas de `transform` dans iced : le décalage de `slide` descend le contenu dans la cellule.
  let cell = container(content)
    .padding(Padding {
      top: (state::LINE_PADDING_Y + look.offset) * style.scale,
      right: state::LINE_PADDING_X * style.scale,
      bottom: state::LINE_PADDING_Y * style.scale,
      left: state::LINE_PADDING_X * style.scale,
    })
    .width(Fill)
    .height(line_box.height);
  if line.time >= 0.0 {
    mouse_area(cell)
      .on_press(lyric(LyricsMessage::LineClicked(index)))
      .interaction(mouse::Interaction::Pointer)
      .into()
  } else {
    cell.into()
  }
}

/// Lignes des paroles : seules celles proches de la zone visible sont construites.
fn lyric_list<'a>(app: &'a App, style: LineStyle) -> Element<'a, Message> {
  let state = &app.lyrics;
  let boxes = &state.layout.boxes;
  if state.lyrics.is_none() {
    let label = if state.loading {
      "Loading..."
    } else {
      "No lyrics"
    };
    return container(text(app.i18n.t(label)).size(15))
      .center(Fill)
      .into();
  }
  let range = visible_lines(boxes, state.offset, state.viewport, VISIBLE_MARGIN);
  let top = boxes
    .get(range.start)
    .map_or(state.layout.content_height, |line| line.top);
  let bottom = range
    .end
    .checked_sub(1)
    .and_then(|last| boxes.get(last))
    .map_or(top, |line| line.top + line.height);
  let mut lines = Vec::with_capacity(range.len() * 2 + 2);
  lines.push(space().height(top).into());
  for index in range {
    if index > 0 && !lines.is_empty() && lines.len() > 1 {
      lines.push(space().height(style.spacing).into());
    }
    lines.push(line_view(app, index, boxes[index], style));
  }
  lines.push(
    space()
      .height((state.layout.content_height - bottom).max(0.0))
      .into(),
  );
  let list = scrollable(Column::from_vec(lines).width(Fill))
    .id(SCROLL_ID)
    .direction(Direction::Vertical(Scrollbar::hidden()))
    .on_scroll(|viewport| {
      lyric(LyricsMessage::Scrolled {
        offset: viewport.absolute_offset().y,
        viewport: viewport.bounds().height,
      })
    })
    .width(Fill)
    .height(Fill);
  // Le fondu repeint le fond : ni la vue plein écran, ni une bannière n'ont de couleur unie derrière les lignes.
  if style.tvk || app.banner_url().is_some() {
    return list.into();
  }
  let edge = super::right_panel::background(app);
  let veil = container(space())
    .width(Fill)
    .height(Fill)
    .style(move |_: &Theme| {
      let gradient = iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
        .add_stop(0.0, edge)
        .add_stop(MASK_SHARE, alpha(edge, 0.0))
        .add_stop(1.0 - MASK_SHARE, alpha(edge, 0.0))
        .add_stop(1.0, edge);
      container::Style::default()
        .background(iced::Background::Gradient(iced::Gradient::Linear(gradient)))
    });
  stack![list, veil].width(Fill).height(Fill).into()
}

/// Lignes des paroles dans `#lyrics-container` du panneau droit : police et marges en pixels CSS
/// convertis, fondu sur la couleur de fond du panneau.
pub(super) fn panel_lyrics(app: &App) -> Element<'_, Message> {
  let zoom = tokens::ZOOM;
  let style = LineStyle {
    size: tokens::css(state::PANEL_FONT),
    font: lyric_font(false),
    align: Horizontal::Left,
    gap_min: 3.0 * zoom,
    gap_max: 22.0 * zoom,
    spacing: tokens::css(state::LINE_SPACING),
    opacity: LINE_ALPHA,
    fade: 1.0,
    tvk: false,
    scale: zoom,
  };
  lyric_list(app, style)
}

fn svg_icon<'a>(bytes: &'static [u8], css_px: f32, color: Color) -> Element<'a, Message> {
  svg(svg::Handle::from_memory(bytes))
    .width(tokens::css(css_px))
    .height(tokens::css(css_px))
    .style(move |_: &Theme, _| svg::Style { color: Some(color) })
    .into()
}

/// Les trois fonds d'un bouton rond de la vue plein écran.
#[derive(Clone, Copy)]
struct RoundColors {
  rest: Color,
  hover: Color,
  press: Color,
  text: Color,
}

fn round_button<'a>(
  content: Element<'a, Message>,
  (width, height): (f32, f32),
  radius: f32,
  colors: RoundColors,
  message: Message,
) -> Element<'a, Message> {
  button(container(content).center(Fill))
    .width(width)
    .height(height)
    .padding(0)
    .on_press(message)
    .style(move |_: &Theme, status| {
      let background = match status {
        button::Status::Hovered => colors.hover,
        button::Status::Pressed => colors.press,
        _ => colors.rest,
      };
      button::Style {
        background: Some(background.into()),
        text_color: colors.text,
        border: border::rounded(radius),
        ..button::Style::default()
      }
    })
    .into()
}

fn mix_white(color: Color, share: f32) -> Color {
  Color {
    r: color.r + (1.0 - color.r) * share,
    g: color.g + (1.0 - color.g) * share,
    b: color.b + (1.0 - color.b) * share,
    ..color
  }
}

/// Pochette de `.tv-cover` : coins de 28 px et ombre `0 20px 60px rgba(0,0,0,0.5)` (`styles.css:6486-6495`).
fn tvk_cover(app: &App, side: f32, fade: f32) -> Element<'_, Message> {
  let track = app.player.current.as_ref();
  let url = track.and_then(|track| app.cover_of(track));
  let size = match url.as_deref() {
    Some(url) if app.images.get(&ImageKey::new(url, SIZE_HERO)).is_some() => SIZE_HERO,
    _ => SIZE_CARD,
  };
  let radius = k::COVER_RADIUS;
  let handle = url
    .as_deref()
    .and_then(|url| app.images.get(&ImageKey::new(url, size)));
  let picture: Element<'_, Message> = match handle {
    Some(handle) => image(handle.clone())
      .width(side)
      .height(side)
      .content_fit(ContentFit::Cover)
      .border_radius(radius)
      .opacity(fade)
      .into(),
    None => container(space())
      .width(side)
      .height(side)
      .style(move |_: &Theme| {
        container::Style::default()
          .background(Color::from_rgba(1.0, 1.0, 1.0, 0.08 * fade))
          .border(border::rounded(radius))
      })
      .into(),
  };
  container(picture)
    .style(move |_: &Theme| container::Style {
      border: border::rounded(radius),
      shadow: Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, k::COVER_SHADOW_ALPHA * fade),
        offset: Vector::new(0.0, k::COVER_SHADOW_OFFSET),
        blur_radius: k::COVER_SHADOW_BLUR,
      },
      ..container::Style::default()
    })
    .into()
}

/// `#tvk-output` : casque et nom de la sortie, la fin du nom s'efface (`mask-image` de `#tvk-output-name`, 28 px).
fn tvk_output(app: &App, color: Color) -> Element<'_, Message> {
  let name = app.i18n.t("Default output");
  let chars: Vec<char> = name.chars().collect();
  let fade_from = chars.len().saturating_sub(k::OUTPUT_FADE_CHARS);
  let spans: Vec<Span<'_, (), Font>> = chars
    .iter()
    .enumerate()
    .map(|(index, character)| {
      // Rampe linéaire sur les derniers caractères : le centre du dernier est à 1/8 de la zone de 28 px.
      let from_end = (chars.len() - index) as f32;
      let visible = if index >= fade_from {
        ((from_end - 0.5) / k::OUTPUT_FADE_CHARS as f32).clamp(0.0, 1.0)
      } else {
        1.0
      };
      Span::new(character.to_string()).color(alpha(color, visible))
    })
    .collect();
  row![
    svg_icon(
      include_bytes!("../../assets/icons/tvk-headset.svg"),
      18.0,
      color
    ),
    rich_text(spans)
      .size(tokens::css(13.0))
      .font(lyric_font(false)),
  ]
  .spacing(tokens::css(8.0))
  .align_y(Vertical::Center)
  .into()
}

/// Barre de progression `#tvk-squiggly-canvas` (`app.js:3521-3600`) : onde blanche jusqu'à la position, sans piste ni curseur
/// (`riff_progress_bg` absent, `no-knob`). Clic et glisser cherchent dans le morceau.
struct TvkScrubber {
  duration: f32,
  shown: f32,
  phase: f32,
  /// Opacité de groupe de la vue (`tvk-in`, `tvk-out`).
  fade: f32,
}

#[derive(Default)]
struct TvkDrag {
  dragging: bool,
}

impl TvkScrubber {
  fn time_at(&self, x: f32, width: f32) -> f32 {
    let pad = tokens::css(k::SCRUB_PAD);
    let span = width - 2.0 * pad;
    let position = if span > 0.0 { (x - pad) / span } else { 0.0 };
    position.clamp(0.0, 1.0) * self.duration
  }
}

impl canvas::Program<Message> for TvkScrubber {
  type State = TvkDrag;

  fn update(
    &self,
    state: &mut TvkDrag,
    event: &iced::Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> Option<canvas::Action<Message>> {
    let player = |message| Message::Player(message);
    match event {
      iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        if self.duration > 0.0 && cursor.is_over(bounds) =>
      {
        let at = cursor.position_in(bounds)?;
        state.dragging = true;
        Some(
          canvas::Action::publish(player(PlayerMessage::SeekPreview(
            self.time_at(at.x, bounds.width),
          )))
          .and_capture(),
        )
      }
      iced::Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
        Some(canvas::Action::publish(player(PlayerMessage::SeekPreview(
          self.time_at(position.x - bounds.x, bounds.width),
        ))))
      }
      iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
        state.dragging = false;
        Some(canvas::Action::publish(player(PlayerMessage::SeekRelease)))
      }
      _ => None,
    }
  }

  fn draw(
    &self,
    _state: &TvkDrag,
    renderer: &iced::Renderer,
    _theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<canvas::Geometry> {
    let mut frame = canvas::Frame::new(renderer, bounds.size());
    frame.scale(tokens::ZOOM);
    let width = bounds.width / tokens::ZOOM;
    let center_y = bounds.height / tokens::ZOOM / 2.0;
    let progress = if self.duration > 0.0 {
      (self.shown / self.duration).clamp(0.0, 1.0)
    } else {
      0.0
    };
    let end = k::SCRUB_PAD + progress * (width - 2.0 * k::SCRUB_PAD).max(0.0);
    if end > k::SCRUB_PAD {
      let phase = (self.phase * 3.5) % std::f32::consts::TAU;
      let wave_at = |x: f32| {
        let angle = (x - k::SCRUB_PAD) * std::f32::consts::TAU / k::SCRUB_WAVE_LENGTH + phase;
        // `taper` : l'amplitude monte sur 10 px au départ et retombe sur les 10 derniers.
        let taper = ((x - k::SCRUB_PAD) / 10.0)
          .min((end - x) / 10.0)
          .clamp(0.0, 1.0);
        Point::new(x, center_y + angle.sin() * k::SCRUB_AMPLITUDE * taper)
      };
      let wave = canvas::Path::new(|path| {
        path.move_to(wave_at(k::SCRUB_PAD));
        let mut x = k::SCRUB_PAD + 2.0;
        while x <= end {
          path.line_to(wave_at(x));
          x += 2.0;
        }
        path.line_to(wave_at(end));
      });
      frame.stroke(
        &wave,
        canvas::Stroke::default()
          .with_color(alpha(Color::WHITE, self.fade))
          .with_width(k::SCRUB_WIDTH)
          .with_line_cap(canvas::LineCap::Round)
          .with_line_join(canvas::LineJoin::Round),
      );
    }
    vec![frame.into_geometry()]
  }

  fn mouse_interaction(
    &self,
    state: &TvkDrag,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> mouse::Interaction {
    if state.dragging {
      mouse::Interaction::Grabbing
    } else if cursor.is_over(bounds) {
      mouse::Interaction::Pointer
    } else {
      mouse::Interaction::default()
    }
  }
}

/// Colonne `.tv-side` : pochette, titre, sortie audio, commandes et progression. Avec les paroles, la colonne fait la largeur
/// du `clamp(200px, 28vw, 420px)` ; sans paroles ou en mode pochette, elle est centrée et la pochette grandit.
fn tvk_side(
  app: &App,
  geo: tvk::Geometry,
  window: Size,
  centered: Option<f32>,
  fade: f32,
) -> Element<'_, Message> {
  let scheme = &app.theme.scheme;
  let page = page_width(window);
  let track = app.player.current.as_ref();
  let title = track.map_or_else(String::new, |track| track.title_text().to_string());
  let artist = track
    .and_then(|track| track.artist.clone())
    .unwrap_or_default();
  // `.tv-karaoke.tv-idle .tv-karaoke-meta` : 60 % avec les paroles seulement (`styles.css:6500`, `6835`).
  let faded = fade
    * if app.lyrics.tvk_idle && centered.is_none() {
      k::IDLE_META_OPACITY
    } else {
      1.0
    };
  let (cover_side, gap, title_size, artist_size, artist_alpha, meta_width) = match centered {
    None => (
      geo.side,
      24.0,
      (page * 0.024).clamp(20.0, 30.0),
      (page * 0.016).clamp(14.0, 18.0),
      0.75,
      geo.side,
    ),
    Some(cover_side) => (
      cover_side,
      16.0,
      28.0,
      18.0,
      0.7,
      tokens::css(600.0).min(window.width * 0.9),
    ),
  };
  let cover = tvk_cover(app, cover_side, fade);
  let meta = column![
    text(title)
      .size(tokens::css(title_size))
      .font(lyric_font(true))
      .line_height(LineHeight::Relative(1.25))
      .align_x(Horizontal::Center)
      .width(Fill)
      .shaping(Shaping::Advanced)
      .color(alpha(Color::WHITE, faded)),
    text(artist)
      .size(tokens::css(artist_size))
      .font(lyric_font(false))
      .line_height(LineHeight::Relative(1.3))
      .align_x(Horizontal::Center)
      .width(Fill)
      .shaping(Shaping::Advanced)
      .color(alpha(Color::WHITE, artist_alpha * faded)),
  ]
  .spacing(tokens::css(8.0))
  .width(meta_width);
  let output = tvk_output(app, alpha(rgb(scheme.on_surface_variant), 0.8 * fade));

  let playing = app.playback.is_playing;
  let on_press = |message| Message::Player(message);
  let white = |share: f32| Color::from_rgba(1.0, 1.0, 1.0, share * fade);
  let skip_colors = RoundColors {
    rest: white(0.16),
    hover: white(0.28),
    press: white(0.36),
    text: white(1.0),
  };
  let primary = rgb(scheme.primary);
  let play_colors = RoundColors {
    rest: alpha(primary, fade),
    hover: alpha(mix_white(primary, 0.08), fade),
    press: alpha(primary, fade),
    text: alpha(rgb(scheme.on_primary), fade),
  };
  let skip = |glyph: &'static str, message: PlayerMessage, label: &str| {
    tip(
      round_button(
        icons::glyph_px(glyph, 20.0),
        (tokens::css(44.0), tokens::css(44.0)),
        tokens::css(22.0),
        skip_colors,
        on_press(message),
      ),
      app.i18n.t(label),
    )
  };
  let (play_width, play_radius) = if playing {
    (k::PLAY_WIDTH_PLAYING, 20.0)
  } else {
    (k::PLAY_HEIGHT, 28.0)
  };
  let play_glyph = if app.player.loading || app.playback.buffering {
    "\u{2026}"
  } else if playing {
    icons::PAUSE
  } else {
    icons::PLAY
  };
  let play = tip(
    round_button(
      icons::glyph_px(play_glyph, 26.0),
      (tokens::css(play_width), tokens::css(k::PLAY_HEIGHT)),
      tokens::css(play_radius),
      play_colors,
      on_press(PlayerMessage::TogglePlay),
    ),
    app.i18n.t(tips::play(playing)),
  );
  // `.play-pause-wrapper` fait 70 px : le bouton de 78 px déborde de 4 px de chaque côté.
  let side_gap = tokens::css(16.0 - (play_width - k::PLAY_WRAPPER) / 2.0);
  let controls = row![
    skip(icons::PREVIOUS, PlayerMessage::Prev, tips::PREVIOUS),
    space().width(side_gap),
    play,
    space().width(side_gap),
    skip(icons::NEXT, PlayerMessage::Next, tips::NEXT),
  ]
  .align_y(Vertical::Center);

  let duration = app
    .playback
    .duration
    .map(|duration| duration.as_secs_f32())
    .or_else(|| {
      track
        .and_then(|track| track.duration)
        .map(|duration| duration as f32)
    })
    .unwrap_or(0.0);
  let position = app.playback.position.as_secs_f32();
  let scrubber = TvkScrubber {
    duration,
    shown: app
      .ui
      .seek_preview
      .unwrap_or(position)
      .min(duration.max(0.001)),
    phase: position,
    fade,
  };
  let progress_width = (page * 0.28).clamp(220.0, 400.0);
  // Le canvas est mesuré pendant l'animation d'ouverture (`scale(0.96)`) et garde cette largeur (`app.js:2881-2888`).
  let progress = container(
    canvas(scrubber)
      .width(tokens::css(progress_width * k::PROGRESS_CANVAS_SHARE))
      .height(tokens::css(k::PROGRESS_HEIGHT)),
  )
  .center_x(tokens::css(progress_width));

  // Les marges de Riff s'ajoutent à l'écart de la colonne ; la rangée des commandes fait 48 px (`.play-pause-wrapper`) et le bouton de 56 px déborde de 4 px.
  column![
    cover,
    space().height(tokens::css(gap)),
    meta,
    space().height(tokens::css(gap)),
    output,
    space().height(tokens::css(
      gap + k::OUTPUT_MARGIN_BOTTOM + k::CONTROLS_MARGIN_TOP - k::CONTROLS_OVERFLOW
    )),
    controls,
    space().height(tokens::css(
      gap + k::PROGRESS_MARGIN_TOP - k::CONTROLS_OVERFLOW
    )),
    progress,
  ]
  .align_x(Horizontal::Center)
  .into()
}

/// Boutons du coin haut droit (`.tv-karaoke-close`, `styles.css:8766-8778`) : pochette puis fermeture, 44 px, sans fond.
/// Au repos (`tv-idle`) ils sont invisibles et n'interceptent plus la souris.
fn tvk_corner(app: &App, fade: f32) -> Element<'_, Message> {
  if app.lyrics.tvk_idle {
    return space().into();
  }
  let hover = Color::from_rgba(1.0, 1.0, 1.0, 0.12 * fade);
  let colors = RoundColors {
    rest: Color::TRANSPARENT,
    hover,
    press: hover,
    text: alpha(Color::WHITE, fade),
  };
  let side = (tokens::css(44.0), tokens::css(44.0));
  let buttons = row![
    tip(
      round_button(
        svg_icon(
          include_bytes!("../../assets/icons/tvk-cover.svg"),
          22.0,
          alpha(Color::WHITE, fade)
        ),
        side,
        tokens::css(22.0),
        colors,
        tvk_message(TvkMessage::ToggleCoverMode),
      ),
      app.i18n.t("Toggle cover")
    ),
    tip(
      round_button(
        svg_icon(
          include_bytes!("../../assets/icons/tvk-close.svg"),
          24.0,
          alpha(Color::WHITE, fade)
        ),
        side,
        tokens::css(22.0),
        colors,
        lyric(LyricsMessage::CloseTvk),
      ),
      app.i18n.t(tips::CLOSE)
    ),
  ]
  .spacing(tokens::css(8.0));
  container(buttons)
    .width(Fill)
    .align_right(Fill)
    .padding(Padding {
      top: tokens::css(28.0),
      right: tokens::css(28.0),
      bottom: 0.0,
      left: 0.0,
    })
    .into()
}

fn page_width(window: Size) -> f32 {
  window.width / tokens::ZOOM
}

/// Vue plein écran (`#tv-karaoke`), au-dessus de toute la fenêtre, barre de lecture comprise (`position: fixed; inset: 0`).
#[must_use]
pub fn tvk(app: &App) -> Element<'_, Message> {
  // `tvk-in` et `tvk-out` : pas d'opacité de groupe dans iced, l'opacité du moment est portée par chaque calque de la vue.
  let pose = app.lyrics.tvk_motion.pose(app.now);
  let fade = pose.map_or(1.0, |pose| pose.opacity);
  // `closeTvKaraoke` rend les paroles à leur panneau tout de suite : la vue qui s'efface n'en montre plus.
  let closing = !app.lyrics.tvk_open;
  let background = tvk_ambient(app, fade);
  let visual = &app.settings.visual;
  let window = app.window.size.unwrap_or(crate::app::DEFAULT_WINDOW);
  let geo = tvk::geometry(window);
  let metrics = tvk::metrics(&visual.tvk);
  let style = LineStyle {
    size: state::tvk_font(window.width) * metrics.scale,
    font: tvk::face(&visual.tvk),
    align: tvk_align(visual.tvk.align),
    gap_min: tokens::css(6.0),
    gap_max: tokens::css(46.0),
    spacing: tvk::line_spacing(&visual.tvk),
    opacity: 1.0,
    fade,
    tvk: true,
    scale: tokens::ZOOM,
  };
  let split = app.tvk_lyrics_shown() && app.tvk_has_lyrics();
  let body: Element<'_, Message> = if split {
    let list: Element<'_, Message> = if closing {
      space().into()
    } else {
      lyric_list(app, style)
    };
    let lyrics = container(container(list).width(geo.line).height(Fill))
      .center_x(geo.content)
      .height(Fill);
    row![
      container(tvk_side(app, geo, window, None, fade))
        .width(geo.side)
        .center_y(Fill),
      space().width(geo.gap),
      lyrics,
    ]
    .padding(Padding {
      top: geo.pad_y,
      right: geo.pad_x,
      bottom: geo.pad_y,
      left: geo.pad_x,
    })
    .height(Fill)
    .into()
  } else {
    // `.tv-nolyrics` : pochette de `clamp(220px, 36vh, 440px)` ; `.tv-cover-mode` : `clamp(260px, 48vh, 560px)`.
    let cover = if app.tvk_lyrics_shown() {
      tvk::nolyrics_cover_side(window.height)
    } else {
      tvk::cover_side(window.height)
    };
    container(tvk_side(app, geo, window, Some(cover), fade))
      .center(Fill)
      .padding(
        Padding::new(geo.pad_y)
          .left(tokens::css(24.0))
          .right(tokens::css(24.0)),
      )
      .into()
  };
  // Un seul aplat opaque sous les orbes (`tvk_ambient`) : `k::BASE` est entièrement recouvert.
  let mut layers = stack![background, body].width(Fill).height(Fill);
  if app.tvk_visualizer_shown() {
    let width = (page_width(window) * 0.64).round().clamp(360.0, 760.0);
    let bars = TvkCanvas {
      heights: app.visuals.tvk.heights,
      color: alpha(
        Color::WHITE,
        visual.tvk_visualizer_opacity as f32 / 100.0 * fade,
      ),
    };
    layers = layers.push(
      container(
        canvas(bars)
          .width(tokens::css(width))
          .height(tokens::css(TVK_BAR_ZONE)),
      )
      .center_x(Fill)
      .align_bottom(Fill),
    );
  }
  layers = layers.push(tvk_corner(app, fade));
  // `#tvk-top-hover-zone` : 320 x 70 px en haut au centre, ouvre le panneau (`app.js:7495`).
  layers = layers.push(
    container(
      mouse_area(space().width(tokens::css(320.0)).height(tokens::css(70.0)))
        .on_enter(tvk_message(TvkMessage::SetPanel(true))),
    )
    .center_x(Fill),
  );
  if app
    .lyrics
    .panel_motion
    .visible(app.lyrics.tvk_panel, app.now)
  {
    layers = layers.push(tvk_panel(app, window));
  }
  // Mouvement de la souris : sort du repos (`resetTvIdleTimer`). Au repos le pointeur disparaît (`cursor: none`).
  let surface = mouse_area(layers)
    .on_move(|_| tvk_message(TvkMessage::Activity))
    .interaction(if app.lyrics.tvk_idle {
      mouse::Interaction::Hidden
    } else {
      mouse::Interaction::None
    });
  // `scale(s) translateY(y)` autour du centre de la fenêtre, sans voile : l'opacité est déjà dans les calques. `.closing` pose
  // `pointer-events: none`.
  let view = view_transition(opaque(surface), pose, None)
    .centered()
    .interactive(!closing);
  view.into()
}

/// Fond de la vue plein écran (`#tvk-ambient-bg`, `styles.css:6300-6313`) : l'aplat `#0d0d12` filtré, les quatre orbes de la
/// pochette qui dérivent, puis la vignette de `.tv-karaoke::before`. Le flou et les filtres de couleur sont précalculés dans les
/// images (`visuals::blur`).
fn tvk_ambient(app: &App, fade: f32) -> Element<'_, Message> {
  let [r, g, b] = blur::ambient_base();
  let group = GroupOpacity::at(fade);
  let base = container(space())
    .width(Fill)
    .height(Fill)
    .style(move |_: &Theme| {
      container::Style::default().background(alpha(Color::from_rgb8(r, g, b), group.base))
    });
  let orbs = Orbs::new(
    &app.visuals.tvk_orbs,
    app.lyrics.tvk_motion.orbit_seconds(app.now),
    group.orbs,
  );
  let vignette: Element<'_, Message> =
    super::backdrop::layer_image(&app.visuals.tvk_orbs[4], ContentFit::Fill, group.vignette)
      .unwrap_or_else(|| space().width(Fill).height(Fill).into());
  stack![base, canvas(orbs).width(Fill).height(Fill), vignette]
    .width(Fill)
    .height(Fill)
    .into()
}

fn tvk_message(message: TvkMessage) -> Message {
  lyric(LyricsMessage::Tvk(message))
}

fn visual_message(message: VisualMessage) -> Message {
  Message::Settings(SettingsMessage::Visual(message))
}

/// Une option de la liste des polices.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FontOption {
  font: TvkFont,
  label: String,
}

impl std::fmt::Display for FontOption {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(&self.label)
  }
}

/// Rangée de boutons dont un seul est choisi (`.tvk-pill-group`).
fn pills<'a, T: Copy + PartialEq + 'a>(
  items: Vec<(T, String)>,
  current: T,
  pick: impl Fn(T) -> Message,
) -> Element<'a, Message> {
  let buttons = items.into_iter().map(|(value, label)| {
    let message = Some(pick(value));
    if value == current {
      filled(label, message)
    } else {
      tonal(label, message)
    }
  });
  Row::with_children(buttons)
    .spacing(6)
    .wrap()
    .vertical_spacing(6)
    .into()
}

fn tvk_toggle<'a>(
  label: String,
  value: bool,
  change: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
  let off = change(!value);
  row![
    text(label).size(13).width(Fill),
    focusable(toggler(value).on_toggle(change)).on_activate(Some(off)),
  ]
  .align_y(Vertical::Center)
  .into()
}

fn tvk_slider<'a>(
  label: String,
  range: (i32, i32),
  step: i32,
  value: i32,
  shown: String,
  change: impl Fn(i32) -> Message + 'a,
) -> Element<'a, Message> {
  column![
    row![text(label).size(13).width(Fill), text(shown).size(13)],
    slider(range.0..=range.1, value, change)
      .step(step)
      .on_release(Message::Settings(SettingsMessage::CommitInt)),
  ]
  .spacing(4)
  .into()
}

/// Panneau de personnalisation (`#tvk-panel`), collé au haut de la fenêtre et centré (`.tv-karaoke .tvk-panel`, `styles.css:8839-8847`).
fn tvk_panel(app: &App, window: Size) -> Element<'_, Message> {
  let visual = &app.settings.visual;
  let style = &visual.tvk;
  let t = |key: &str| app.i18n.t(key);
  let fonts: Vec<FontOption> = tvk::fonts()
    .iter()
    .map(|(font, _)| FontOption {
      font: *font,
      label: t(font.label()),
    })
    .collect();
  let selected = fonts
    .iter()
    .find(|option| option.font == TvkFont::from_stored(&style.font))
    .cloned();
  let mut controls = column![
    column![
      text(t("Font")).size(13),
      pick_list(fonts, selected, |option: FontOption| tvk_message(
        TvkMessage::Font(option.font)
      ))
      .text_size(13)
      .width(Fill),
    ]
    .spacing(4),
    column![
      text(t("Alignment")).size(13),
      pills(
        [
          (TvkAlign::Left, "\u{25E7}", "Align left"),
          (TvkAlign::Center, "\u{25EB}", "Align center"),
          (TvkAlign::Right, "\u{25E8}", "Align right"),
        ]
        .into_iter()
        .map(|(align, glyph, label)| (align, format!("{glyph} {}", t(label))))
        .collect(),
        style.align,
        |align| tvk_message(TvkMessage::Align(align)),
      ),
    ]
    .spacing(4),
  ]
  .spacing(k::INNER_GAP);
  for key in [TvkSlider::Size, TvkSlider::Weight, TvkSlider::Gap] {
    let value = key.value(style);
    controls = controls.push(tvk_slider(
      t(key.label()),
      key.range(),
      key.step(),
      value,
      key.format(value),
      move |next| tvk_message(TvkMessage::Slider(key, next)),
    ));
  }
  let speed = TvkSlider::Speed.value(style);
  let animations = [
    (TvkAnim::Smooth, "Smooth"),
    (TvkAnim::Slide, "Slide"),
    (TvkAnim::Fade, "Fade"),
    (TvkAnim::Pop, "Pop"),
    (TvkAnim::None, "None"),
  ];
  controls = controls
    .push(
      column![
        text(t("Animation")).size(13),
        pills(
          animations
            .into_iter()
            .map(|(anim, label)| (anim, t(label)))
            .collect(),
          style.anim,
          |anim| tvk_message(TvkMessage::Anim(anim)),
        ),
      ]
      .spacing(4),
    )
    .push(tvk_slider(
      t(TvkSlider::Speed.label()),
      TvkSlider::Speed.range(),
      TvkSlider::Speed.step(),
      speed,
      TvkSlider::Speed.format(speed),
      |next| tvk_message(TvkMessage::Slider(TvkSlider::Speed, next)),
    ))
    .push(tvk_toggle(
      t("No-lyrics visualizer"),
      visual.tvk_visualizer,
      |next| tvk_message(TvkMessage::Visualizer(next)),
    ))
    .push(tvk_toggle(
      t("Connected wave style"),
      visual.viz_connected,
      |next| visual_message(VisualMessage::Bool(VisualBool::VizConnected, next)),
    ))
    .push(tvk_toggle(
      t("Visualizer with lyrics"),
      visual.tvk_visualizer_lyrics,
      |next| visual_message(VisualMessage::Bool(VisualBool::TvkVisualizerLyrics, next)),
    ));
  for (key, label) in [
    (VisualInt::TvkVisualizerOpacity, "Visualizer opacity"),
    (VisualInt::TvkBgBlur, "Background blur"),
  ] {
    let value = app.visual_int(key);
    controls = controls.push(tvk_slider(
      t(label),
      key.range(),
      key.step(),
      value,
      key.format(value),
      move |next| visual_message(VisualMessage::Int(key, next)),
    ));
  }
  controls = controls.push(
    row![
      space().width(Fill),
      text_button(t("Reset"), Some(tvk_message(TvkMessage::Reset)))
    ]
    .align_y(Vertical::Center),
  );
  // `.tvk-panel-inner` : contenu défilant sans barre, `max-height: min(76vh, 580px)`, remplissage 18/22/24 (`styles.css:8905-8912`).
  let drawer = rgb(app.theme.scheme.surface_container_high);
  let controls = controls.padding(Padding {
    top: k::INNER_PAD_TOP,
    right: k::INNER_PAD_X,
    bottom: k::INNER_PAD_BOTTOM,
    left: k::INNER_PAD_X,
  });
  let inner = scrollable(controls)
    .direction(Direction::Vertical(Scrollbar::hidden()))
    .width(Fill);
  // `mask-image` de 36 px en bas : iced n'a pas de masque. Le fond du panneau est un aplat sous le contenu, donc un dégradé de ce
  // fond vers son opaque par-dessus donne le même mélange que le masque (`contenu * m + fond * (1 - m)`). Il ne retire pas le contenu
  // des événements, qui restent actifs sous le dégradé comme sous un masque.
  let fade = gradient::Linear::new(Radians::PI)
    .add_stop(0.0, alpha(drawer, 0.0))
    .add_stop(1.0, drawer);
  // Le dégradé suit les coins arrondis du panneau : hors d'eux, le fond du panneau n'existe pas.
  let corners = border::Radius {
    top_left: 0.0,
    top_right: 0.0,
    bottom_right: k::PANEL_RADIUS,
    bottom_left: k::PANEL_RADIUS,
  };
  let inner = stack![
    inner,
    container(
      container(space())
        .width(Fill)
        .height(k::INNER_FADE)
        .style(move |_| {
          container::Style::default()
            .background(Gradient::Linear(fade))
            .border(border::rounded(corners))
        })
    )
    .width(Fill)
    .height(Fill)
    .align_bottom(Fill)
  ];
  // `translateY(-6px)` vers 0 en 220 ms : le contenu glisse, la découpe du panneau ne bouge pas.
  let inner_offset = app.lyrics.panel_motion.inner_offset(app.now);
  let inner = view_transition(
    inner,
    (inner_offset != 0.0).then_some(ViewPose {
      opacity: 1.0,
      offset_y: inner_offset,
      scale: 1.0,
    }),
    None,
  )
  .reveal(1.0, 0.0, 0.0);
  let width = k::PANEL_WIDTH.min(window.width / tokens::ZOOM - k::PANEL_MARGIN) * tokens::ZOOM;
  let max_height = (window.height * k::INNER_MAX_VH).min(k::INNER_MAX_HEIGHT);
  let body = container(inner)
    .width(width)
    .max_height(max_height)
    .style(move |_| {
      container::Style::default()
        .background(drawer)
        .border(border::rounded(corners))
    });
  // `tvk-panel` s'efface en un seul groupe : l'ombre porte aussi l'opacité du moment (alpha sur le fond, exact hors de la boîte).
  let motion = app.lyrics.panel_motion.pose(app.now);
  let pose = motion.unwrap_or(TvkPanelPose::REST);
  // Le quad d'iced qui porte une ombre laisse la rangée 0 de l'écran à 75 % de son fond (mesuré : sans ombre la rangée vaut le fond
  // du panneau). L'ombre est donc portée par un quad sans fond de mêmes bornes, sous le fond opaque qui recouvre cette rangée.
  let panel = container(body).style(move |_| {
    container::Style::default()
      .border(border::rounded(corners))
      .shadow(Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, k::PANEL_SHADOW_ALPHA * pose.opacity),
        offset: Vector::new(0.0, k::PANEL_SHADOW_Y),
        blur_radius: k::PANEL_SHADOW_BLUR,
      })
  });
  // `.open` : transitions de `clip-path` (découpe du haut vers le bas), `transform` (`translate3d(-50%, -18px, 0) scale(0.98)` autour du
  // haut centre) et `opacity`, dans les deux sens (`styles.css:8863-8882`). Le fondu est une opacité de groupe sur le fond de la vue :
  // un calque (`ScreenLayer`) rejoue ce fond (aplat, orbes, vignette) à `1 - opacité` par-dessus la boîte du panneau, à sa place à
  // l'écran, donc `panneau * opacité + fond * (1 - opacité)` en chaque pixel, orbes comprises. Fermé, le panneau ne reçoit plus
  // d'événements. Au repos la découpe `inset(0 -35px -20px -35px)` reste posée : elle borne l'ombre.
  // Les raccords sont dans le groupe d'opacité du panneau mais hors de sa boîte, donc hors du voile : ils portent l'opacité en alpha.
  let ears = canvas(PanelEars {
    radius: k::PANEL_EAR,
    color: alpha(drawer, pose.opacity),
  })
  .width(Fill)
  .height(k::PANEL_EAR);
  let view_pose = ViewPose {
    opacity: pose.opacity,
    offset_y: pose.offset_y,
    scale: pose.scale,
  };
  let mut framed =
    stack![container(opaque(panel)).padding(Padding::ZERO.left(k::PANEL_EAR).right(k::PANEL_EAR)),];
  if pose.opacity < 1.0 {
    framed = framed.push(
      ScreenLayer::new(tvk_veil(app, window, 1.0 - pose.opacity), view_pose, window)
        .margin(k::PANEL_EAR)
        .reveal(pose.reveal, k::PANEL_CLIP_BELOW),
    );
  }
  let framed = framed.push(ears);
  let panel = view_transition(framed, Some(view_pose), None)
    .margin(k::PANEL_EAR)
    .reveal(pose.reveal, k::PANEL_CLIP_SIDE, k::PANEL_CLIP_BELOW)
    .interactive(app.lyrics.tvk_panel);
  container(container(panel).width(width + 2.0 * k::PANEL_EAR))
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .into()
}

/// Dessous d'une opacité de groupe sur le fond de la vue : le fond (aplat, orbes, vignette) en une seule image, dessinée à `alpha`.
/// Des calques superposés ne donnent le bon mélange qu'en un pixel typique (`GroupOpacity`) ; une image unique le donne en chaque pixel.
fn tvk_veil(app: &App, window: Size, alpha: f32) -> Element<'_, Message> {
  let orbs = Orbs::new(
    &app.visuals.tvk_orbs,
    app.lyrics.tvk_motion.orbit_seconds(app.now),
    [1.0; 4],
  );
  let flat = orbs.flatten(
    blur::ambient_base(),
    &app.visuals.tvk_orbs[4],
    window,
    k::VEIL_STEP,
  );
  image(flat)
    .width(Fill)
    .height(Fill)
    .content_fit(ContentFit::Fill)
    .filter_method(FilterMethod::Linear)
    .opacity(alpha)
    .into()
}

/// Fenêtre d'édition (`#custom-lyrics-modal`).
#[must_use]
pub fn editor(app: &App) -> Option<Element<'_, Message>> {
  let editor = app.lyrics.editor.as_ref()?;
  let track = app.player.current.as_ref()?;
  let field = text_editor(&editor.0)
    .on_action(|action| lyric(LyricsMessage::EditorAction(action)))
    .height(Fill)
    .padding(10);
  let body = column![
    text(track.title_text().to_string())
      .size(18)
      .shaping(Shaping::Advanced),
    text(track.artist.clone().unwrap_or_default())
      .size(13)
      .shaping(Shaping::Advanced),
    field,
    row![
      space().width(Fill),
      tonal(
        app.i18n.t("Cancel"),
        Some(lyric(LyricsMessage::EditorClose))
      ),
      filled(app.i18n.t("Save"), Some(lyric(LyricsMessage::EditorSave))),
    ]
    .spacing(8),
  ]
  .spacing(10);
  Some(card(body.into()))
}

fn card<'a>(body: Element<'a, Message>) -> Element<'a, Message> {
  let surface = container(body)
    .padding(20)
    .width(CARD_WIDTH)
    .height(CARD_HEIGHT)
    .style(|theme: &Theme| {
      container::Style::default()
        .background(theme.extended_palette().background.weak.color)
        .color(theme.palette().text)
        .border(border::rounded(16))
    });
  opaque(
    container(surface)
      .center(Fill)
      .style(|_: &Theme| container::Style::default().background(SCRIM)),
  )
}

/// Synchro manuelle (`#sync-mode-overlay`).
#[must_use]
pub fn sync(app: &App) -> Option<Element<'_, Message>> {
  let sync = app.lyrics.sync.as_ref()?;
  let palette = app.theme.theme.palette();
  let rows = sync.lines.iter().enumerate().map(|(index, line)| {
    let color = if index == sync.current && line.time.is_none() {
      palette.primary
    } else if line.time.is_some() {
      palette.text
    } else {
      alpha(palette.text, 0.55)
    };
    let stamp = line.time.map_or_else(String::new, |time| {
      let minutes = (time / 60.0).floor();
      format!("[{}:{:05.2}]", minutes as u64, time - minutes * 60.0)
    });
    let content = row![
      text(stamp).size(12).color(alpha(palette.text, 0.6)),
      text(line.text.as_str())
        .size(16)
        .shaping(Shaping::Advanced)
        .color(color)
    ]
    .spacing(8)
    .align_y(Vertical::Center);
    button(content)
      .width(Fill)
      .style(crate::widgets::button::text_style)
      .on_press(lyric(LyricsMessage::SyncLine(index)))
      .into()
  });
  let progress = format!(
    "{} / {} {}",
    sync.synced(),
    sync.lines.len(),
    app.i18n.t("lines synced")
  );
  let body = column![
    row![
      text(app.i18n.t("Recording"))
        .size(14)
        .color(palette.primary)
        .width(Fill),
      tip(
        icon(icons::CLOSE, Some(lyric(LyricsMessage::SyncClose)), 28, 28),
        app.i18n.t("Close")
      )
    ]
    .align_y(Vertical::Center),
    text(app.i18n.t("Tap a line or press Space on beat")).size(13),
    row![
      tonal(
        app.i18n.t("Restart"),
        Some(lyric(LyricsMessage::SyncRestart))
      ),
      container(text(progress).size(13)).center_x(Fill),
      filled(
        app.i18n.t("Save and exit"),
        Some(lyric(LyricsMessage::SyncSave))
      ),
    ]
    .spacing(8)
    .align_y(Vertical::Center),
    scrollable(Column::with_children(rows).spacing(2).width(Fill)).height(Fill),
  ]
  .spacing(10)
  .height(Fill);
  Some(card(body.into()))
}

/// Fenêtre des mini-paroles (décision 6).
#[must_use]
pub fn mini(app: &App) -> Element<'_, Message> {
  let palette = app.theme.theme.palette();
  let state = &app.lyrics;
  let style = LineStyle {
    size: MINI_FONT,
    font: lyric_font(false),
    align: Horizontal::Center,
    gap_min: 3.0,
    gap_max: 18.0,
    spacing: 6.0,
    opacity: 1.0,
    fade: 1.0,
    tvk: false,
    scale: 1.0,
  };
  let bar = row![
    mouse_area(container(text("Riff").size(12)).padding([4, 8]).width(Fill))
      .on_press(Message::Lyrics(LyricsMessage::MiniDrag)),
    tip(
      icon(
        if app.settings.visual.mini_window.always_on_top {
          "\u{25C9}"
        } else {
          "\u{25CB}"
        },
        Some(lyric(LyricsMessage::MiniOnTop)),
        24,
        24
      ),
      app.i18n.t("Always on top")
    ),
    tip(
      icon(icons::CLOSE, Some(lyric(LyricsMessage::MiniClosed)), 24, 24),
      app.i18n.t("Close")
    ),
  ]
  .align_y(Vertical::Center);
  let body: Element<'_, Message> = match state.lyrics.as_ref() {
    Some(lyrics) if !state.layout.lines.is_empty() => {
      let center = state.active.unwrap_or(0);
      let first = center.saturating_sub(MINI_NEIGHBOURS);
      let last = (center + MINI_NEIGHBOURS + 1).min(lyrics.lines.len());
      let lines = (first..last).map(|index| {
        let line = &lyrics.lines[index];
        let active = state.active == Some(index);
        let style = LineStyle {
          opacity: rest_look(active).opacity,
          ..style
        };
        let content: Element<'_, Message> = if line.is_gap {
          gap_line(app, active, palette.text, style)
        } else if app.is_gradient_mode() {
          let fill = if active {
            lyrics.gradient_fill(index, state.position)
          } else {
            0.0
          };
          gradient_line(
            &line.text,
            fill,
            app.settings.visual.lyrics_gradient_tightness,
            state.layout.lines[index].rtl,
            palette.text,
            style,
          )
        } else {
          text(line.text.as_str())
            .size(MINI_FONT)
            .font(lyric_font(false))
            .shaping(Shaping::Advanced)
            .color(if active {
              palette.text
            } else {
              alpha(palette.text, LINE_ALPHA)
            })
            .into()
        };
        container(content).center_x(Fill).into()
      });
      Column::with_children(lines)
        .spacing(6)
        .align_x(Horizontal::Center)
        .width(Fill)
        .into()
    }
    _ => text(app.i18n.t("No lyrics")).size(14).into(),
  };
  let mut page = column![bar, container(body).center(Fill)].height(Fill);
  if app.mini_ticker() {
    page = page.push(crate::app::ticker());
  }
  container(page)
    .width(Fill)
    .height(Fill)
    .style(move |theme: &Theme| {
      container::Style::default()
        .background(theme.extended_palette().background.weaker.color)
        .color(palette.text)
    })
    .into()
}
