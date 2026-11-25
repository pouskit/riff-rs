//! Sortie audio par `cpal`, derrière un trait pour tester le moteur sans périphérique.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, ErrorKind, FromSample, Host, Sample, SampleFormat, SizedSample, StreamConfig};

use crate::AudioError;
use crate::mixer::Mixer;

const BUFFER_FRAMES: usize = 1024;

/// Description d'un périphérique de sortie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
  pub id: String,
  pub name: String,
  pub is_default: bool,
}

/// Ce que la sortie signale au contrôle.
pub(crate) enum OutputNotice {
  /// Flux inutilisable : à rouvrir.
  Lost(String),
  /// Le système a basculé le flux sur un autre périphérique : le flux reste actif.
  Rerouted,
}

/// Rappel de la sortie vers le contrôle.
pub(crate) type OutputNotifier = Arc<dyn Fn(OutputNotice) + Send + Sync>;

pub(crate) trait OutputStream {
  fn sample_rate(&self) -> u32;
  fn name(&self) -> String;
  /// Vrai si le flux suit le périphérique par défaut et que celui-ci a changé.
  fn device_changed(&self) -> bool {
    false
  }
  fn suspend(&self);
  fn resume(&self);
}

pub(crate) trait OutputBackend: Send {
  /// Ouvre la sortie : `build` reçoit la fréquence retenue et fournit le mélangeur du callback.
  fn open(
    &mut self,
    device: Option<&str>,
    build: &mut dyn FnMut(u32) -> Mixer,
    notifier: OutputNotifier,
  ) -> Result<Box<dyn OutputStream>, AudioError>;
}

/// Sortie réelle.
pub(crate) struct CpalBackend;

struct CpalStream {
  stream: cpal::Stream,
  sample_rate: u32,
  name: String,
  /// Identifiant du périphérique ouvert s'il a été choisi comme défaut, sinon `None`.
  default_id: Option<cpal::DeviceId>,
}

impl OutputStream for CpalStream {
  fn sample_rate(&self) -> u32 {
    self.sample_rate
  }

  fn name(&self) -> String {
    self.name.clone()
  }

  fn device_changed(&self) -> bool {
    let Some(opened) = &self.default_id else {
      return false;
    };
    let current = cpal::default_host()
      .default_output_device()
      .and_then(|device| device.id().ok());
    current.is_some_and(|id| id != *opened)
  }

  fn suspend(&self) {
    if let Err(err) = self.stream.pause() {
      tracing::warn!(error = %err, "suspension de la sortie refusée");
    }
  }

  fn resume(&self) {
    if let Err(err) = self.stream.play() {
      tracing::warn!(error = %err, "reprise de la sortie refusée");
    }
  }
}

fn stream_error(err: impl std::fmt::Display) -> AudioError {
  AudioError::Stream(err.to_string())
}

/// Choisit le périphérique et renvoie aussi l'identifiant du défaut si c'est lui qui est ouvert.
///
/// Le défaut est ouvert par son identifiant précis, pas par le périphérique virtuel de `cpal` :
/// mesuré sous Windows 11 sur un casque débranché, le virtuel échoue ensuite à chaque ouverture dans
/// tout le processus (`0x80010106`), même périphérique rebranché, alors que le précis réussit.
fn select_device(host: &Host, id: Option<&str>) -> Option<(Device, Option<cpal::DeviceId>)> {
  if let Some(id) = id
    && let Ok(parsed) = id.parse()
    && let Some(device) = host.device_by_id(&parsed)
  {
    return Some((device, None));
  }
  let default = host.default_output_device()?;
  let Ok(default_id) = default.id() else {
    return Some((default, None));
  };
  let device = host.device_by_id(&default_id).unwrap_or(default);
  Some((device, Some(default_id)))
}

fn device_name(device: &Device) -> String {
  device.description().map_or_else(
    |_| "périphérique inconnu".to_string(),
    |d| d.name().to_string(),
  )
}

impl OutputBackend for CpalBackend {
  fn open(
    &mut self,
    device: Option<&str>,
    build: &mut dyn FnMut(u32) -> Mixer,
    notifier: OutputNotifier,
  ) -> Result<Box<dyn OutputStream>, AudioError> {
    let host = cpal::default_host();
    let (device, default_id) = select_device(&host, device).ok_or(AudioError::NoDevice)?;
    let supported = device.default_output_config().map_err(stream_error)?;
    let sample_rate = supported.sample_rate();
    let config = supported.config();
    let mixer = build(sample_rate);
    let stream = match supported.sample_format() {
      SampleFormat::F32 => build_stream::<f32>(&device, config, mixer, notifier),
      SampleFormat::F64 => build_stream::<f64>(&device, config, mixer, notifier),
      SampleFormat::I16 => build_stream::<i16>(&device, config, mixer, notifier),
      SampleFormat::I32 => build_stream::<i32>(&device, config, mixer, notifier),
      SampleFormat::U16 => build_stream::<u16>(&device, config, mixer, notifier),
      SampleFormat::U8 => build_stream::<u8>(&device, config, mixer, notifier),
      other => Err(AudioError::Stream(format!(
        "format d'échantillon non géré : {other}"
      ))),
    }?;
    stream.play().map_err(stream_error)?;
    let name = device_name(&device);
    Ok(Box::new(CpalStream {
      stream,
      sample_rate,
      name,
      default_id,
    }))
  }
}

fn build_stream<T>(
  device: &Device,
  config: StreamConfig,
  mut mixer: Mixer,
  notifier: OutputNotifier,
) -> Result<cpal::Stream, AudioError>
where
  T: SizedSample + FromSample<f32> + Send + 'static,
{
  let channels = usize::from(config.channels).max(1);
  let mut left = vec![0.0f32; BUFFER_FRAMES];
  let mut right = vec![0.0f32; BUFFER_FRAMES];
  let data = move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
    for frames in output.chunks_mut(BUFFER_FRAMES * channels) {
      let count = frames.len() / channels;
      mixer.render(&mut left[..count], &mut right[..count]);
      interleave(frames, channels, &left[..count], &right[..count]);
    }
  };
  let error = move |err: cpal::Error| match err.kind() {
    ErrorKind::DeviceChanged => notifier(OutputNotice::Rerouted),
    ErrorKind::Xrun | ErrorKind::RealtimeDenied => {
      tracing::warn!(error = %err, "événement de sortie audio");
    }
    _ => notifier(OutputNotice::Lost(err.to_string())),
  };
  device
    .build_output_stream::<T, _, _>(config, data, error, None)
    .map_err(stream_error)
}

/// Écrit la sortie : stéréo sur les deux premiers canaux, moyenne en mono, silence au-delà.
fn interleave<T>(output: &mut [T], channels: usize, left: &[f32], right: &[f32])
where
  T: Sample + FromSample<f32>,
{
  for ((frame, l), r) in output.chunks_mut(channels).zip(left).zip(right) {
    if let [mono] = frame {
      *mono = T::from_sample((0.5 * (l + r)).clamp(-1.0, 1.0));
    } else {
      let (front, rest) = frame.split_at_mut(2);
      front[0] = T::from_sample(l.clamp(-1.0, 1.0));
      front[1] = T::from_sample(r.clamp(-1.0, 1.0));
      rest.iter_mut().for_each(|sample| *sample = T::EQUILIBRIUM);
    }
  }
}

/// Liste des périphériques de sortie.
pub(crate) fn list_devices() -> Vec<DeviceInfo> {
  let host = cpal::default_host();
  let default_id = host
    .default_output_device()
    .and_then(|device| device.id().ok());
  let Ok(devices) = host.output_devices() else {
    return Vec::new();
  };
  devices
    .filter_map(|device| {
      let id = device.id().ok()?;
      Some(DeviceInfo {
        is_default: default_id.as_ref() == Some(&id),
        id: id.to_string(),
        name: device_name(&device),
      })
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn stereo_goes_to_the_first_two_channels() {
    let mut out = [9.0f32; 8];
    interleave(&mut out, 4, &[0.5, -0.5], &[0.25, -0.25]);
    assert_eq!(out, [0.5, 0.25, 0.0, 0.0, -0.5, -0.25, 0.0, 0.0]);
  }

  #[test]
  fn mono_output_averages_the_channels() {
    let mut out = [9.0f32; 2];
    interleave(&mut out, 1, &[1.0, 0.5], &[0.0, 0.5]);
    assert_eq!(out, [0.5, 0.5]);
  }

  #[test]
  fn values_are_clipped_at_the_conversion() {
    let mut out = [0.0f32; 2];
    interleave(&mut out, 2, &[2.0], &[-2.0]);
    assert_eq!(out, [1.0, -1.0]);
    let mut ints = [0i16; 2];
    interleave(&mut ints, 2, &[2.0], &[-2.0]);
    assert!(ints[0] > 32_000 && ints[1] < -32_000);
  }
}
