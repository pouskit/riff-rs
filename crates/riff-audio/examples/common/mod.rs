//! Lecture des options communes à `play` et `render`.

use riff_audio::{AudioSettings, HqEngine, HqSettings};

/// Options d'effets de la ligne de commande.
#[derive(Debug, Default)]
pub struct EffectOptions {
  pub settings: AudioSettings,
  pub hq: Option<HqSettings>,
}

fn number(flag: &str, value: Option<String>) -> Result<f32, String> {
  let value = value.ok_or_else(|| format!("{flag} : valeur manquante"))?;
  value
    .parse()
    .map_err(|_| format!("{flag} : nombre invalide : {value}"))
}

fn engine(value: Option<String>) -> Result<HqEngine, String> {
  match value.as_deref() {
    Some("hqmusic-3") => Ok(HqEngine::HqMusic3),
    Some("glyphs-7B") => Ok(HqEngine::Glyphs7B),
    Some("fyro1.1") => Ok(HqEngine::Fyro11),
    other => Err(format!("--hq : moteur inconnu : {other:?}")),
  }
}

fn bands(value: Option<String>) -> Result<[f32; 10], String> {
  let value = value.ok_or("--eq : valeur manquante")?;
  let gains: Vec<f32> = value
    .split(',')
    .map(|gain| {
      gain
        .trim()
        .parse()
        .map_err(|_| format!("--eq : nombre invalide : {gain}"))
    })
    .collect::<Result<_, _>>()?;
  gains
    .try_into()
    .map_err(|_| "--eq : 10 gains attendus".to_string())
}

impl EffectOptions {
  /// Applique `flag` s'il s'agit d'une option d'effet. Renvoie faux si elle est inconnue.
  ///
  /// # Errors
  /// Valeur absente ou invalide.
  pub fn apply(
    &mut self,
    flag: &str,
    args: &mut impl Iterator<Item = String>,
  ) -> Result<bool, String> {
    let settings = &mut self.settings;
    match flag {
      "--eq" => settings.eq.bands = bands(args.next())?,
      "--reverb" => settings.reverb = number(flag, args.next())?,
      "--echo" => settings.echo = number(flag, args.next())?,
      "--distortion" => settings.distortion = number(flag, args.next())?,
      "--speed" => settings.speed = number(flag, args.next())?,
      "--speed-pitch" => settings.speed_pitch = number(flag, args.next())?,
      "--pitch" => settings.pitch = number(flag, args.next())?,
      "--hq" => {
        self.hq = Some(HqSettings {
          engine: engine(args.next())?,
          ..HqSettings::default()
        });
      }
      _ => return Ok(false),
    }
    Ok(true)
  }
}
