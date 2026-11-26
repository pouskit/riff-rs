//! Le traitement d'un bloc ne doit effectuer aucune allocation.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use riff_audio::AudioSettings;
use riff_audio::analyser::SampleRing;
use riff_audio::dsp::eq::EqGains;
use riff_audio::dsp::hq::{HqEngine, HqSettings};
use riff_audio::dsp::waveshaper::Oversample;
use riff_audio::dsp::{Chain, ChainConfig};
use riff_audio::mixer::{Mixer, MixerCommand, MixerControls, voice_channel};

// Drapeau et compteur locaux au fil : les tests tournent en parallèle dans le même processus,
// un compteur global mélangerait les allocations (et la remise à zéro) de l'un avec celles de l'autre.
thread_local! {
  static COUNTING: Cell<bool> = const { Cell::new(false) };
  static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

impl CountingAllocator {
  fn record() {
    // `try_with` : le stockage local du fil peut être détruit pendant la fin du fil.
    let _ = COUNTING.try_with(|flag| {
      if flag.get() {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
      }
    });
  }
}

// SAFETY: délègue à `System` sans changer les paramètres ; le compteur ne touche pas à la mémoire.
unsafe impl GlobalAlloc for CountingAllocator {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    Self::record();
    // SAFETY: mêmes préconditions que l'appelant.
    unsafe { System.alloc(layout) }
  }

  unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
    Self::record();
    // SAFETY: mêmes préconditions que l'appelant.
    unsafe { System.dealloc(ptr, layout) }
  }

  unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
    Self::record();
    // SAFETY: mêmes préconditions que l'appelant.
    unsafe { System.alloc_zeroed(layout) }
  }

  unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
    Self::record();
    // SAFETY: mêmes préconditions que l'appelant.
    unsafe { System.realloc(ptr, layout, new_size) }
  }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn count_allocations(work: impl FnOnce()) -> usize {
  ALLOCATIONS.with(|count| count.set(0));
  COUNTING.with(|flag| flag.set(true));
  work();
  COUNTING.with(|flag| flag.set(false));
  ALLOCATIONS.with(Cell::get)
}

fn all_on() -> AudioSettings {
  AudioSettings {
    reverb: 60.0,
    echo: 30.0,
    distortion: 50.0,
    muffled: 70.0,
    eq: EqGains {
      low: 2.0,
      mid: -1.0,
      high: 3.0,
      bands: [1.0; 10],
    },
    ..AudioSettings::default()
  }
}

#[test]
fn counter_detects_an_allocation() {
  let count = count_allocations(|| drop(std::hint::black_box(vec![0u8; 64])));
  assert!(count > 0, "le compteur doit voir une allocation");
}

#[test]
fn processing_a_block_does_not_allocate() {
  let mut chain = Chain::new(ChainConfig {
    sample_rate: 48_000.0,
    hq_oversample: Oversample::X2,
    output_stage: true,
  });
  chain.set_taps(
    Some(Arc::new(SampleRing::new(4096))),
    Some(Arc::new(SampleRing::new(4096))),
  );
  chain.set_settings(&all_on());
  chain.set_hq(
    true,
    &HqSettings {
      engine: HqEngine::Glyphs7B,
      ..HqSettings::default()
    },
  );
  chain.set_volume(0.85, false);

  let mut left = vec![0.25f32; 512];
  let mut right = vec![0.25f32; 512];
  // Couvre plusieurs blocs du grand étage de convolution (1 024 échantillons).
  let count = count_allocations(|| {
    for _ in 0..64 {
      chain.process(&mut left, &mut right);
    }
  });
  assert_eq!(count, 0, "allocations dans le traitement");
}

#[test]
fn changing_settings_in_the_callback_does_not_allocate() {
  let mut chain = Chain::new(ChainConfig {
    sample_rate: 48_000.0,
    hq_oversample: Oversample::X2,
    output_stage: true,
  });
  let mut left = vec![0.25f32; 128];
  let mut right = vec![0.25f32; 128];
  let settings = all_on();
  let hq = HqSettings {
    engine: HqEngine::Fyro11,
    bass: 10.0,
    vocal: 5.0,
    air: -5.0,
  };
  let count = count_allocations(|| {
    for step in 0..32 {
      let mut changed = settings;
      changed.distortion = step as f32;
      changed.muffled = step as f32 * 3.0;
      chain.set_settings(&changed);
      chain.set_hq(step % 2 == 0, &hq);
      chain.set_volume(0.5, false);
      chain.set_normalize_gain_db(step as f32 * 0.1);
      chain.fade_out(0.25);
      chain.fade_in(0.35);
      chain.begin_pause_fade();
      chain.end_pause_fade();
      chain.flush_effects();
      chain.process(&mut left, &mut right);
    }
  });
  assert_eq!(count, 0, "allocations dans les réglages");
}

#[test]
fn mixer_render_with_commands_and_crossfade_does_not_allocate() {
  let chain = Chain::new(ChainConfig {
    sample_rate: 48_000.0,
    hq_oversample: Oversample::X2,
    output_stage: true,
  });
  let (mut commands, command_rx) = rtrb::RingBuffer::new(64);
  let (trash_tx, mut trash) = rtrb::RingBuffer::new(16);
  let controls = Arc::new(MixerControls::new(0.85, false));
  let mut mixer = Mixer::new(chain, command_rx, trash_tx, controls.clone(), 48_000.0);

  let mut voices = Vec::new();
  let mut producers = Vec::new();
  for _ in 0..3 {
    let (mut producer, voice, _shared) = voice_channel(24_000, 48_000);
    for _ in 0..48_000 {
      assert!(producer.push(0.25f32).is_ok());
    }
    voices.push(voice);
    producers.push(producer);
  }
  let (Some(third), Some(second), Some(first)) = (voices.pop(), voices.pop(), voices.pop()) else {
    unreachable!("trois voix créées");
  };
  assert!(commands.push(MixerCommand::SetVoice(first)).is_ok());
  assert!(commands.push(MixerCommand::Settings(all_on())).is_ok());
  assert!(
    commands
      .push(MixerCommand::Crossfade {
        next: second,
        seconds: 0.5
      })
      .is_ok()
  );

  let mut left = vec![0.0f32; 480];
  let mut right = vec![0.0f32; 480];
  let count = count_allocations(|| {
    controls.set_volume(0.5, false);
    controls.set_normalize_db(3.0);
    for _ in 0..80 {
      mixer.render(&mut left, &mut right);
    }
  });
  assert_eq!(count, 0, "allocations dans le rendu");

  // Remplacement de voix : l'ancienne part à la corbeille, sans libération dans le rendu.
  assert!(commands.push(MixerCommand::SetVoice(third)).is_ok());
  let count = count_allocations(|| mixer.render(&mut left, &mut right));
  assert_eq!(count, 0, "allocations au remplacement de voix");
  assert!(trash.pop().is_ok());
}

#[test]
fn mixer_render_through_every_command_seek_underrun_and_end_does_not_allocate() {
  let mut chain = Chain::new(ChainConfig {
    sample_rate: 48_000.0,
    hq_oversample: Oversample::X2,
    output_stage: true,
  });
  chain.set_taps(
    Some(Arc::new(SampleRing::new(4096))),
    Some(Arc::new(SampleRing::new(4096))),
  );
  let (mut commands, command_rx) = rtrb::RingBuffer::new(64);
  let (trash_tx, mut trash) = rtrb::RingBuffer::new(16);
  let controls = Arc::new(MixerControls::new(0.85, false));
  let mut mixer = Mixer::new(chain, command_rx, trash_tx, controls.clone(), 48_000.0);

  // Voix courte : se termine et se vide pendant la mesure.
  let (mut producer, voice, shared) = voice_channel(6_000, 48_000);
  for _ in 0..(2 * 6_000) {
    assert!(producer.push(0.25f32).is_ok());
  }
  shared.mark_finished();
  assert!(commands.push(MixerCommand::SetVoice(voice)).is_ok());
  assert!(commands.push(MixerCommand::Settings(all_on())).is_ok());
  assert!(
    commands
      .push(MixerCommand::Hq {
        enabled: true,
        settings: HqSettings::default()
      })
      .is_ok()
  );
  assert!(commands.push(MixerCommand::FadeOut(0.2)).is_ok());
  assert!(commands.push(MixerCommand::FadeIn(0.2)).is_ok());
  assert!(commands.push(MixerCommand::BeginPauseFade).is_ok());
  assert!(commands.push(MixerCommand::Pause).is_ok());
  assert!(commands.push(MixerCommand::Resume).is_ok());
  assert!(commands.push(MixerCommand::FlushEffects).is_ok());

  let mut left = vec![0.0f32; 480];
  let mut right = vec![0.0f32; 480];
  let count = count_allocations(|| {
    // Lecture, fin de la voix et sous-alimentation (tampon vide) après la fin.
    for _ in 0..40 {
      mixer.render(&mut left, &mut right);
    }
  });
  assert_eq!(count, 0, "allocations en lecture, fin et sous-alimentation");
  assert!(shared.is_drained());

  // Déplacement : le callback vide le tampon puis acquitte, sans allocation.
  let generation = shared.request_seek();
  shared.mark_seek_decoded(generation);
  let count = count_allocations(|| {
    for _ in 0..4 {
      mixer.render(&mut left, &mut right);
    }
  });
  assert_eq!(count, 0, "allocations au déplacement");
  assert!(shared.seek_acknowledged(generation));

  // Retrait de la voix : elle part à la corbeille, sans libération dans le rendu.
  assert!(commands.push(MixerCommand::ClearVoice).is_ok());
  controls.set_volume(0.0, true);
  let count = count_allocations(|| mixer.render(&mut left, &mut right));
  assert_eq!(count, 0, "allocations au retrait de la voix");
  assert!(trash.pop().is_ok());
}
