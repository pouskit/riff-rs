//! Fenêtre de téléchargement : formats, qualité, découpe avec onde et poignées.

use super::common::{chip, cover};
use crate::app::{App, Message};
use crate::download_ui::{
  DownloadBatch, DownloadMessage, DownloadState, DownloadTab, FORMATS, Handle, QUALITIES,
  TrimRange, WaveState,
};
use crate::images::SIZE_CARD;
use crate::library::format_duration;
use crate::ui::UiMessage;
use crate::widgets::button::{filled, text_button, tonal};
use crate::widgets::focus::focusable;
use iced::alignment::Vertical;
use iced::widget::canvas::{self, Action, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{Column, Row, column, row, text, toggler};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme, mouse};

const COVER: f32 = 56.0;
const WAVE_HEIGHT: f32 = 96.0;
const GRAB: f32 = 10.0;

fn send(message: DownloadMessage) -> Message {
  Message::Download(message)
}

/// Onde et poignées.
struct TrimCanvas<'a> {
  peaks: &'a [f32],
  range: TrimRange,
}

/// Poignée saisie.
#[derive(Default)]
struct CanvasState {
  dragging: Option<Handle>,
}

impl TrimCanvas<'_> {
  fn x_of(&self, seconds: f64, width: f32) -> f32 {
    (seconds / self.range.duration.max(0.001)) as f32 * width
  }

  fn seconds_at(&self, x: f32, width: f32) -> f64 {
    f64::from(x / width.max(1.0)) * self.range.duration
  }
}

impl canvas::Program<Message> for TrimCanvas<'_> {
  type State = CanvasState;

  fn update(
    &self,
    state: &mut CanvasState,
    event: &canvas::Event,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> Option<Action<Message>> {
    let canvas::Event::Mouse(event) = event else {
      return None;
    };
    match event {
      mouse::Event::ButtonPressed(mouse::Button::Left) => {
        let position = cursor.position_in(bounds)?;
        let start = (position.x - self.x_of(self.range.start, bounds.width)).abs();
        let end = (position.x - self.x_of(self.range.end, bounds.width)).abs();
        state.dragging = if start <= GRAB && start <= end {
          Some(Handle::Start)
        } else if end <= GRAB {
          Some(Handle::End)
        } else {
          None
        };
        state.dragging.map(|_| Action::capture())
      }
      mouse::Event::CursorMoved { position } => {
        let handle = state.dragging?;
        let seconds = self.seconds_at(position.x - bounds.x, bounds.width);
        Some(Action::publish(send(DownloadMessage::MoveHandle(handle, seconds))).and_capture())
      }
      mouse::Event::ButtonReleased(mouse::Button::Left) => {
        state.dragging.take().map(|_| Action::capture())
      }
      _ => None,
    }
  }

  fn draw(
    &self,
    _state: &CanvasState,
    renderer: &Renderer,
    theme: &Theme,
    bounds: Rectangle,
    _cursor: mouse::Cursor,
  ) -> Vec<Geometry> {
    let palette = theme.extended_palette();
    let mut frame = Frame::new(renderer, bounds.size());
    let middle = bounds.height / 2.0;
    let column_width = bounds.width / self.peaks.len().max(1) as f32;
    let (left, right) = (
      self.x_of(self.range.start, bounds.width),
      self.x_of(self.range.end, bounds.width),
    );
    for (index, peak) in self.peaks.iter().enumerate() {
      let x = index as f32 * column_width;
      let height = (peak * bounds.height).max(1.0);
      let color = if x >= left && x <= right {
        palette.primary.base.color
      } else {
        palette.background.strong.color
      };
      frame.fill_rectangle(
        Point::new(x, middle - height / 2.0),
        Size::new(column_width.max(1.0), height),
        color,
      );
    }
    for x in [left, right] {
      let line = Path::line(Point::new(x, 0.0), Point::new(x, bounds.height));
      frame.stroke(
        &line,
        Stroke::default().with_width(3.0).with_color(Color {
          a: 1.0,
          ..palette.primary.strong.color
        }),
      );
    }
    vec![frame.into_geometry()]
  }

  fn mouse_interaction(
    &self,
    state: &CanvasState,
    bounds: Rectangle,
    cursor: mouse::Cursor,
  ) -> mouse::Interaction {
    if state.dragging.is_some() {
      return mouse::Interaction::Grabbing;
    }
    let Some(position) = cursor.position_in(bounds) else {
      return mouse::Interaction::default();
    };
    let near = |seconds: f64| (position.x - self.x_of(seconds, bounds.width)).abs() <= GRAB;
    if near(self.range.start) || near(self.range.end) {
      mouse::Interaction::Grab
    } else {
      mouse::Interaction::default()
    }
  }
}

fn format_chips(app: &App) -> Element<'_, Message> {
  let chips = FORMATS.iter().map(|(format, _)| {
    chip(
      format.to_uppercase(),
      app.settings.download_format == *format,
      send(DownloadMessage::Format((*format).to_string())),
    )
  });
  Row::with_children(chips).spacing(4).wrap().into()
}

fn quality_chips(app: &App) -> Element<'_, Message> {
  let chips = QUALITIES.iter().map(|(quality, label)| {
    chip(
      app.i18n.t(label),
      app.ui.download.quality == *quality,
      send(DownloadMessage::Quality((*quality).to_string())),
    )
  });
  Row::with_children(chips).spacing(4).wrap().into()
}

fn file_tab(app: &App) -> Element<'_, Message> {
  let download = &app.ui.download;
  let effects = download.with_effects;
  let mut page = column![text(app.i18n.t("Audio format")).size(13), format_chips(app)].spacing(8);
  if !matches!(app.settings.download_format.as_str(), "flac" | "wav") {
    page = page
      .push(text(app.i18n.t("Bitrate quality")).size(13))
      .push(quality_chips(app));
  }
  if download.batch.is_some() {
    return page.into();
  }
  page
    .push(
      row![
        text(
          app
            .i18n
            .t("Apply current audio effects (Preset/High Quality)")
        )
        .size(13)
        .width(Fill),
        focusable(toggler(effects).on_toggle(|value| send(DownloadMessage::ToggleEffects(value))))
          .on_activate(Some(send(DownloadMessage::ToggleEffects(!effects)))),
      ]
      .align_y(Vertical::Center),
    )
    .into()
}

fn time_pill<'a>(label: String, seconds: f64) -> Element<'a, Message> {
  column![
    text(label).size(11),
    text(format_duration(seconds)).size(14)
  ]
  .spacing(2)
  .width(Fill)
  .into()
}

fn trim_tab(app: &App) -> Element<'_, Message> {
  let download = &app.ui.download;
  let Some(range) = download.trim else {
    return text("").into();
  };
  let stage: Element<'_, Message> = match &download.wave {
    WaveState::Ready(peaks) => Canvas::new(TrimCanvas { peaks, range })
      .width(Fill)
      .height(WAVE_HEIGHT)
      .into(),
    WaveState::Failed(error) => text(format!(
      "{} ({error})",
      app.i18n.t("Could not read the audio")
    ))
    .size(13)
    .into(),
    WaveState::Empty | WaveState::Loading => text(app.i18n.t("Loading...")).size(13).into(),
  };
  let saved = download
    .track
    .as_deref()
    .is_some_and(|track| app.saved_trim(track).is_some());
  let mut actions = row![
    tonal(
      app.i18n.t("Play selection"),
      matches!(download.wave, WaveState::Ready(_)).then_some(send(DownloadMessage::Preview))
    ),
    tonal(
      app.i18n.t("Save trim"),
      Some(send(DownloadMessage::SaveTrim))
    ),
  ]
  .spacing(8);
  if saved {
    actions = actions.push(text_button(
      app.i18n.t("Reset"),
      Some(send(DownloadMessage::ResetTrim)),
    ));
  }
  column![
    row![
      time_pill(app.i18n.t("Start"), range.start),
      time_pill(app.i18n.t("Duration"), range.length()),
      time_pill(app.i18n.t("End"), range.end)
    ]
    .spacing(8),
    stage,
    actions,
    text(
      app
        .i18n
        .t("Drag the handles. The selection is also used by the download.")
    )
    .size(12),
  ]
  .spacing(10)
  .into()
}

fn status_line(app: &App) -> Option<Element<'_, Message>> {
  match &app.ui.download.state {
    DownloadState::Idle => None,
    DownloadState::Running => Some(text(app.i18n.t("Downloading...")).size(13).into()),
    DownloadState::Done(name) => Some(
      text(format!("{} {name}", app.i18n.t("Saved:")))
        .size(13)
        .into(),
    ),
    DownloadState::Failed(error) => Some(text(error.clone()).size(13).style(text::danger).into()),
  }
}

/// Mode lot : nom et nombre de titres, format et qualité, sans découpe ni effets.
fn batch_view<'a>(app: &'a App, batch: &'a DownloadBatch) -> Element<'a, Message> {
  let total = batch.tracks.len();
  let count = format!("{total} track{}", if total == 1 { "" } else { "s" });
  let head = column![text(batch.name.as_str()).size(15), text(count).size(12)]
    .spacing(2)
    .clip(true);
  let running = app.ui.download.state == DownloadState::Running;
  let progress = app.batch_progress();
  let (close, cancel_label) = if running {
    (
      (!batch.cancelling).then_some(send(DownloadMessage::Cancel)),
      "Cancel",
    )
  } else {
    (Some(Message::Ui(UiMessage::ModalClose)), "Close")
  };
  let actions = row![
    text_button(app.i18n.t(cancel_label), close),
    filled(
      progress.unwrap_or_else(|| app.i18n.t("Download")),
      (!running).then_some(send(DownloadMessage::Start))
    ),
  ]
  .spacing(8);
  column![head, file_tab(app), actions].spacing(14).into()
}

/// Contenu de la fenêtre.
#[must_use]
pub fn view(app: &App) -> Element<'_, Message> {
  let download = &app.ui.download;
  if let Some(batch) = &download.batch {
    return batch_view(app, batch);
  }
  let Some(track) = download.track.as_deref() else {
    return text("").into();
  };
  let head = row![
    cover(app, track.thumb_url().as_deref(), COVER, SIZE_CARD, false),
    column![
      text(track.title_text().to_string()).size(15),
      text(track.artist_text()).size(12)
    ]
    .spacing(2)
    .width(Fill)
    .clip(true),
  ]
  .spacing(10)
  .align_y(Vertical::Center);
  let tabs = row![
    chip(
      app.i18n.t("Download file"),
      download.tab == DownloadTab::File,
      send(DownloadMessage::Tab(DownloadTab::File))
    ),
    chip(
      app.i18n.t("Trim best part"),
      download.tab == DownloadTab::Trim,
      send(DownloadMessage::Tab(DownloadTab::Trim))
    ),
  ]
  .spacing(4);
  let body = match download.tab {
    DownloadTab::File => file_tab(app),
    DownloadTab::Trim => trim_tab(app),
  };
  let running = download.state == DownloadState::Running;
  let label = if running {
    "Downloading..."
  } else {
    "Download"
  };
  let close = if running {
    send(DownloadMessage::Cancel)
  } else {
    Message::Ui(UiMessage::ModalClose)
  };
  let actions = row![
    text_button(
      app.i18n.t(if running { "Cancel" } else { "Close" }),
      Some(close)
    ),
    filled(
      app.i18n.t(label),
      (!running).then_some(send(DownloadMessage::Start))
    ),
  ]
  .spacing(8);
  let mut page = Column::new().spacing(14).push(head).push(tabs).push(body);
  if let Some(status) = status_line(app) {
    page = page.push(status);
  }
  page.push(actions).into()
}
