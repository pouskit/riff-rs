//! Écho : ligne à retard 0,3 s, contre-réaction 0,35 (`createDelay(1.0)` de Riff).

const DELAY_SECS: f64 = 0.3;
const MAX_DELAY_SECS: f64 = 1.0;
const FEEDBACK: f32 = 0.35;
/// Chromium calcule la boucle d'après la sortie du quantum précédent : 128 échantillons de plus
/// par tour (mesuré, voir le test).
const FEEDBACK_EXTRA_FRAMES: usize = 128;

pub struct Echo {
  buffers: [Box<[f32]>; 2],
  write: usize,
  delay: usize,
}

impl Echo {
  pub fn new(fs: f64) -> Self {
    let len = (MAX_DELAY_SECS * fs).ceil() as usize + 1;
    Self {
      buffers: [
        vec![0.0; len].into_boxed_slice(),
        vec![0.0; len].into_boxed_slice(),
      ],
      write: 0,
      delay: (DELAY_SECS * fs).round() as usize,
    }
  }

  pub fn reset(&mut self) {
    self.buffers.iter_mut().for_each(|b| b.fill(0.0));
    self.write = 0;
  }

  /// Écrit la sortie de la ligne à retard (avant le gain `echo/100`) dans `out_*`.
  pub fn process(
    &mut self,
    in_left: &[f32],
    in_right: &[f32],
    out_left: &mut [f32],
    out_right: &mut [f32],
  ) {
    let len = self.buffers[0].len();
    let start = self.write;
    let channels = [(in_left, out_left), (in_right, out_right)];
    for (buffer, (input, output)) in self.buffers.iter_mut().zip(channels) {
      let mut write = start;
      let mut read = (start + len - self.delay) % len;
      let mut feedback = (start + len - self.delay - FEEDBACK_EXTRA_FRAMES) % len;
      for (x, y) in input.iter().zip(output.iter_mut()) {
        let delayed = buffer[read];
        *y = delayed;
        buffer[write] = *x + buffer[feedback] * FEEDBACK;
        write = if write + 1 == len { 0 } else { write + 1 };
        read = if read + 1 == len { 0 } else { read + 1 };
        feedback = if feedback + 1 == len { 0 } else { feedback + 1 };
      }
    }
    self.write = (start + in_left.len()) % len;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn impulse_response(fs: f64, seconds: f64, block: usize) -> Vec<f32> {
    let frames = (seconds * fs) as usize;
    let mut echo = Echo::new(fs);
    let mut input = vec![0.0; frames];
    input[0] = 1.0;
    let mut out_l = vec![0.0; frames];
    let mut out_r = vec![0.0; frames];
    for ((i, ol), or) in input
      .chunks(block)
      .zip(out_l.chunks_mut(block))
      .zip(out_r.chunks_mut(block))
    {
      echo.process(i, i, ol, or);
    }
    assert_eq!(out_l, out_r);
    out_l
  }

  /// Mesuré dans Chromium 154 (`OfflineAudioContext`) : 1er écho à 13 230 échantillons (44,1 kHz),
  /// puis 13 358 entre répétitions : la boucle ajoute un quantum de 128 échantillons.
  #[test]
  fn impulse_produces_repeats_like_chromium() {
    for fs in [44_100.0, 48_000.0] {
      let out = impulse_response(fs, 2.0, 128);
      let delay = (0.3 * fs).round() as usize;
      let mut expected = std::collections::BTreeMap::new();
      let (mut position, mut amplitude) = (delay, 1.0f32);
      while position < out.len() {
        expected.insert(position, amplitude);
        position += delay + 128;
        amplitude *= 0.35;
      }
      for (i, value) in out.iter().enumerate() {
        let want = expected.get(&i).copied().unwrap_or(0.0);
        assert!(
          (value - want).abs() < 1e-5,
          "fs={fs} i={i} got={value} want={want}"
        );
      }
    }
  }

  #[test]
  fn block_size_does_not_change_the_result() {
    assert_eq!(
      impulse_response(48_000.0, 1.0, 64),
      impulse_response(48_000.0, 1.0, 512)
    );
  }

  #[test]
  fn reset_clears_the_tail() {
    let mut echo = Echo::new(48_000.0);
    let input = vec![1.0; 128];
    let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
    echo.process(&input, &input, &mut l, &mut r);
    echo.reset();
    let silence = vec![0.0; 128];
    for _ in 0..200 {
      echo.process(&silence, &silence, &mut l, &mut r);
      assert!(l.iter().all(|v| *v == 0.0));
    }
  }
}
