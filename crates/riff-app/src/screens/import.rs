//! Écran d'import au premier lancement (avant l'accueil) et rapport d'import. Noms de clés seulement, jamais de valeur.

use crate::app::{App, Message};
use crate::i18n::I18n;
use crate::import_flow::{ImportChoice, ImportMessage, PendingImport};
use crate::import_report::{ImportReport, SecretState};
use crate::ui::UiMessage;
use crate::widgets::button::{filled, text_button, tonal};
use crate::window;
use iced::widget::{Stack, column, container, row, scrollable, space, text};
use iced::{Element, Fill, Length, Theme, border};

/// Largeur de la fenêtre du rapport.
pub const REPORT_WIDTH: f32 = 560.0;
const CARD_WIDTH: f32 = 480.0;
const REPORT_HEIGHT: f32 = 360.0;

fn choose(choice: ImportChoice) -> Message {
  Message::Import(ImportMessage::Choose(choice))
}

/// Une catégorie du rapport : titre et lignes déjà traduites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportSection {
  pub title: String,
  pub lines: Vec<String>,
}

/// Les quatre catégories du rapport, puis les secrets s'il y en a. Nom de la clé, raison traduite, état : pas de valeur.
#[must_use]
pub fn report_sections(i18n: &I18n, report: &ImportReport) -> Vec<ReportSection> {
  let keys = |title: &str, keys: &[String]| ReportSection {
    title: i18n.t(title),
    lines: keys.to_vec(),
  };
  let mut sections = vec![
    keys("Imported", &report.imported),
    keys("Invalid value, default kept", &report.invalid),
    ReportSection {
      title: i18n.t("No native equivalent"),
      lines: report
        .not_ported
        .iter()
        .map(|entry| format!("{}: {}", entry.key, i18n.t(entry.reason)))
        .collect(),
    },
    keys("Unknown keys", &report.unknown),
  ];
  if !report.secrets.is_empty() {
    sections.push(ReportSection {
      title: i18n.t("Secrets"),
      lines: report
        .secrets
        .iter()
        .map(|secret| {
          let state = match secret.state {
            SecretState::Imported => "Stored in the system vault",
            SecretState::ToReenter => "Not imported, enter it again",
          };
          format!("{}: {}", secret.key, i18n.t(state))
        })
        .collect(),
    });
  }
  sections
}

/// Contenu de la fenêtre du rapport.
#[must_use]
pub fn report_view<'a>(app: &'a App, report: &ImportReport) -> Element<'a, Message> {
  let mut body = column![].spacing(12).width(Fill);
  for section in report_sections(&app.i18n, report) {
    let mut block =
      column![text(format!("{} ({})", section.title, section.lines.len())).size(14)].spacing(2);
    if section.lines.is_empty() {
      block = block.push(text(app.i18n.t("None")).size(12));
    }
    for line in section.lines {
      block = block.push(text(line).size(12));
    }
    body = body.push(block);
  }
  column![
    scrollable(body).height(Length::Fixed(REPORT_HEIGHT)),
    row![
      space().width(Fill),
      text_button(
        app.i18n.t("Close"),
        Some(Message::Ui(UiMessage::ModalClose))
      )
    ],
  ]
  .spacing(12)
  .into()
}

fn card_style(theme: &Theme) -> container::Style {
  let palette = theme.extended_palette();
  container::Style::default()
    .background(palette.background.neutral.color)
    .color(palette.background.base.text)
    .border(border::rounded(16))
}

fn surface_style(theme: &Theme) -> container::Style {
  let palette = theme.extended_palette();
  container::Style::default()
    .background(palette.background.base.color)
    .color(palette.background.base.text)
}

/// Ce que l'écran annonce, ligne par ligne : libellé traduit et nombre.
#[must_use]
pub fn offer_lines(i18n: &I18n, pending: &PendingImport) -> Vec<String> {
  let offer = pending.offer;
  [
    ("Favorites", offer.favorites),
    ("Playlists", offer.playlists),
    ("Local files", offer.local_files),
    ("Settings", offer.settings),
  ]
  .into_iter()
  .map(|(label, count)| format!("{}: {count}", i18n.t(label)))
  .collect()
}

/// Écran plein fenêtre avant l'accueil. Les boutons de la fenêtre restent atteignables.
#[must_use]
pub fn view<'a>(app: &'a App, pending: &'a PendingImport) -> Element<'a, Message> {
  let working = pending.working;
  let mut counts = column![].spacing(4);
  for line in offer_lines(&app.i18n, pending) {
    counts = counts.push(text(line).size(14));
  }
  let card = container(
    column![
      text(app.i18n.t("Import your Riff data")).size(20),
      text(app.i18n.t(
        "Riff data was found on this computer. Import it into this app, or start with a clean slate."
      ))
      .size(13),
      counts,
      text(app.i18n.t("Nothing is written until you choose. Your Riff data is never modified.")).size(12),
      row![
        space().width(Fill),
        tonal(
          app.i18n.t("Start fresh"),
          (!working).then(|| choose(ImportChoice::Fresh))
        ),
        filled(
          app.i18n.t(if working {
            "Importing..."
          } else {
            "Import from Riff"
          }),
          (!working).then(|| choose(ImportChoice::Riff))
        ),
      ]
      .spacing(8),
    ]
    .spacing(14),
  )
  .padding(24)
  .width(CARD_WIDTH)
  .style(card_style);
  let bar = row![
    window::drag_zone().map(Message::Chrome),
    window::window_buttons(app.window.is_maximized).map(Message::Chrome),
  ]
  .height(window::BAR_HEIGHT);
  let page = container(column![bar, container(card).center(Fill)])
    .width(Fill)
    .height(Fill)
    .style(surface_style);
  let mut layers = Stack::new().width(Fill).height(Fill).push(page);
  if app.main_ticker() {
    layers = layers.push(crate::app::ticker());
  }
  if let Some(toast) = app.toast_layer() {
    layers = layers.push(toast);
  }
  layers.into()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::import_report::ImportOffer;
  use crate::settings::LegacyStorage;
  use riff_core::secrets::SecretName;

  fn report() -> ImportReport {
    let dir = crate::test_temp::TempDir::new("app-import-screen");
    let path = dir.join("storage.json");
    std::fs::write(
      &path,
      r#"{
        "riff_ui_lang": "VALEUR-LANG",
        "riff_theme_mode": "VALEUR-MODE",
        "riff_font": "VALEUR-FONT",
        "riff_cle_inconnue": "VALEUR-INCONNUE",
        "riff_lastfm_sk": "VALEUR-SECRET"
      }"#,
    )
    .unwrap();
    ImportReport::new(&LegacyStorage::open(&path))
  }

  #[test]
  fn the_four_categories_and_the_secrets_show_names_and_reasons_never_values() {
    let mut report = report();
    report.mark_secret_to_reenter(SecretName::LastfmSession);
    let sections = report_sections(&I18n::default(), &report);
    let titles: Vec<_> = sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(
      titles,
      [
        "Imported",
        "Invalid value, default kept",
        "No native equivalent",
        "Unknown keys",
        "Secrets"
      ]
    );
    assert_eq!(sections[0].lines, ["riff_ui_lang"]);
    assert_eq!(sections[1].lines, ["riff_theme_mode"]);
    assert_eq!(
      sections[2].lines,
      ["riff_font: The native font is fixed at build time"]
    );
    assert_eq!(sections[3].lines, ["riff_cle_inconnue"]);
    assert_eq!(
      sections[4].lines,
      ["riff_lastfm_sk: Not imported, enter it again"]
    );
    assert!(!format!("{sections:?}").contains("VALEUR"));
  }

  #[test]
  fn the_reasons_and_the_states_are_translated() {
    let i18n = I18n::load("ru", std::path::Path::new("Z:/absent"));
    let sections = report_sections(&i18n, &report());
    assert_ne!(
      sections[2].lines[0],
      "riff_font: The native font is fixed at build time"
    );
    assert_ne!(sections[0].title, "Imported");
  }

  #[test]
  fn every_text_of_the_screen_and_every_reason_is_a_key_translated_in_ru_and_uk() {
    use crate::i18n::UI_KEYS;
    use crate::import_report::NOT_PORTED;
    let mut texts = vec![
      "Import your Riff data",
      "Riff data was found on this computer. Import it into this app, or start with a clean slate.",
      "Nothing is written until you choose. Your Riff data is never modified.",
      "Start fresh",
      "Import from Riff",
      "Importing...",
      "Favorites",
      "Playlists",
      "Local files",
      "Settings",
      "Import report",
      "Imported",
      "Invalid value, default kept",
      "No native equivalent",
      "Unknown keys",
      "Secrets",
      "Stored in the system vault",
      "Not imported, enter it again",
      "None",
      "Close",
      "Could not save the setting.",
      "Translation cache, rebuilt natively",
      "Cache, rebuilt on demand",
    ];
    texts.extend(NOT_PORTED.iter().map(|(_, reason)| *reason));
    let none = std::path::Path::new("Z:/absent");
    let (ru, uk) = (I18n::load("ru", none), I18n::load("uk", none));
    for text in texts {
      assert!(UI_KEYS.contains(&text), "{text} absent de UI_KEYS");
      assert_ne!(ru.t(text), text, "{text} non traduit en ru");
      assert_ne!(uk.t(text), text, "{text} non traduit en uk");
    }
    for key in ["riff_i18n_fr", "riff_artist_meta_cache"] {
      let reason = crate::import_report::not_ported_reason(key).unwrap();
      assert!(UI_KEYS.contains(&reason), "{reason}");
    }
  }

  #[test]
  fn an_empty_report_still_has_its_four_categories_and_no_secrets() {
    let sections = report_sections(&I18n::default(), &ImportReport::default());
    assert_eq!(sections.len(), 4);
    assert!(sections.iter().all(|section| section.lines.is_empty()));
  }

  #[test]
  fn the_screen_announces_the_four_counts() {
    let pending = PendingImport {
      offer: ImportOffer {
        favorites: 12,
        playlists: 3,
        local_files: 4,
        settings: 40,
      },
      storage: LegacyStorage::default(),
      working: false,
    };
    assert_eq!(
      offer_lines(&I18n::default(), &pending),
      [
        "Favorites: 12",
        "Playlists: 3",
        "Local files: 4",
        "Settings: 40"
      ]
    );
  }
}
