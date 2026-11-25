//! Dégradé « mesh » (`updateMeshGradient`) : shader `wgpu`, un seul passage (décision 3).
//!
//! Le calque de Riff (`.bg-mesh-layer`) dépasse la fenêtre de `inset` ; `meshFloat` le déplace (`scale` puis `translate`), et
//! `filter: blur()` s'applique dans son repère avant la transformation. Le shader refait ces trois étapes pixel par pixel : pas de
//! texture, donc le banc rend n'importe quel instant à partir de l'horloge de `App`.

use crate::theme::tokens::mesh as tokens;
use crate::visuals::settings::MeshConfig;
use crate::visuals::state::bezier_ease;
use iced::widget::shader::{self, Viewport};
use iced::{Color, Rectangle, mouse, wgpu};
use std::time::Duration;

/// Nombre de dessins du maillage dans une même image : le calque de fond (0) et l'aperçu des réglages (1).
const SLOTS: u64 = 2;
/// `minUniformBufferOffsetAlignment` par défaut de `wgpu` : un bloc d'uniformes par dessin.
const SLOT_STRIDE: u64 = 256;

/// `meshKey` (`app.js:1695`) : couleurs finales des deux ellipses, mode, luminosité et suivi de l'intensité. Quand elle change,
/// `updateMeshGradient` fait sortir l'autre calque de `display: none` : son `meshFloat` repart de zéro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshKey {
  pub first: [u8; 3],
  pub second: [u8; 3],
  pub dark: bool,
  pub brightness: u32,
  pub follow_intensity: bool,
}

/// Paramètres du dessin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshProgram {
  pub config: MeshConfig,
  pub first: Color,
  pub second: Color,
  pub background: Color,
  pub brightness: f32,
  pub dark: bool,
  /// Avancement de `meshFloat` : de 0 à 2 (voir [`phase`]).
  pub phase: f32,
  /// `inset` négatif du calque, en pixels logiques.
  pub inset: f32,
  /// Rayon des coins de la zone dessinée, en pixels logiques (aperçu : `overflow: hidden` et `border-radius`).
  pub radius: f32,
  /// Bloc d'uniformes du dessin, voir [`SLOTS`].
  pub slot: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
  first: [f32; 4],
  second: [f32; 4],
  background: [f32; 4],
  points: [f32; 4],
  transform: [f32; 4],
  area: [f32; 4],
  blur: [f32; 4],
}

/// Avancement de `meshFloat` après `elapsed` : l'animation `alternate` va de 0 à 1 puis revient de 1 à 2, chaque aller durant
/// `period_secs` (`--mesh-speed`, `app.js:16149`).
#[must_use]
pub fn phase(elapsed: Duration, period_secs: f32) -> f32 {
  ((elapsed.as_secs_f64() / f64::from(period_secs)) % 2.0) as f32
}

/// `@keyframes meshFloat` (`styles.css:169-173`) : renvoie `(échelle, x, y)`. Le retour d'une itération `alternate` rejoue le
/// chemin à l'envers ; `ease-in-out` s'applique à chacun des deux segments entre images clés.
#[must_use]
pub fn mesh_float(phase: f32) -> (f32, f32, f32) {
  let progress = if phase <= 1.0 { phase } else { 2.0 - phase };
  let [first, middle, last] = tokens::FLOAT_KEYS;
  let (from, to, part) = if progress < 0.5 {
    (first, middle, progress * 2.0)
  } else {
    (middle, last, (progress - 0.5) * 2.0)
  };
  let eased = bezier_ease(tokens::FLOAT_CURVE, part);
  let lerp = |a: f32, b: f32| a + (b - a) * eased;
  (lerp(from.0, to.0), lerp(from.1, to.1), lerp(from.2, to.2))
}

/// Opacité d'un calque pendant la `transition: opacity` de `#bg-mesh-a` et `#bg-mesh-b` (`styles.css:9186-9193`). Riff n'en voit
/// jamais l'effet (voir [`tokens::FADE_MS`]) : les calques changent par coupure franche, cette fonction n'est donc pas branchée.
#[must_use]
pub fn layer_fade(elapsed_ms: f32) -> f32 {
  bezier_ease(tokens::FADE_CURVE, elapsed_ms / tokens::FADE_MS)
}

impl MeshProgram {
  fn uniforms(&self) -> Uniforms {
    let (alpha_first, alpha_second) = if self.dark {
      tokens::ALPHA_DARK
    } else {
      tokens::ALPHA_LIGHT
    };
    let (scale, x, y) = mesh_float(self.phase);
    let rgba = |color: Color, alpha: f32| [color.r, color.g, color.b, alpha * self.brightness];
    Uniforms {
      first: rgba(self.first, alpha_first),
      second: rgba(self.second, alpha_second),
      background: [self.background.r, self.background.g, self.background.b, 1.0],
      points: [
        self.config.p1.x / 100.0,
        self.config.p1.y / 100.0,
        self.config.p2.x / 100.0,
        self.config.p2.y / 100.0,
      ],
      transform: [scale, x, y, 0.0],
      area: [0.0; 4],
      blur: [0.0; 4],
    }
  }
}

impl<Message> shader::Program<Message> for MeshProgram {
  type State = ();
  type Primitive = MeshPrimitive;

  fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> MeshPrimitive {
    MeshPrimitive {
      uniforms: self.uniforms(),
      inset: self.inset,
      radius: self.radius,
      // `--mesh-blur` est en pixels CSS : même zoom que le reste de l'interface.
      blur: crate::theme::tokens::css(self.config.blur),
      slot: u64::from(self.slot).min(SLOTS - 1),
    }
  }
}

/// Primitive du dessin.
#[derive(Debug)]
pub struct MeshPrimitive {
  uniforms: Uniforms,
  inset: f32,
  radius: f32,
  blur: f32,
  slot: u64,
}

impl std::fmt::Debug for Uniforms {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str("Uniforms")
  }
}

/// Ressources du dessin.
pub struct MeshPipeline {
  pipeline: wgpu::RenderPipeline,
  buffer: wgpu::Buffer,
  bind_group: wgpu::BindGroup,
}

impl shader::Pipeline for MeshPipeline {
  fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("mesh"),
      source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("mesh"),
      entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
          ty: wgpu::BufferBindingType::Uniform,
          has_dynamic_offset: true,
          min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
        },
        count: None,
      }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("mesh"),
      bind_group_layouts: &[&layout],
      push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("mesh"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &module,
        entry_point: Some("vs"),
        buffers: &[],
        compilation_options: wgpu::PipelineCompilationOptions::default(),
      },
      primitive: wgpu::PrimitiveState::default(),
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      fragment: Some(wgpu::FragmentState {
        module: &module,
        entry_point: Some("fs"),
        targets: &[Some(wgpu::ColorTargetState {
          format,
          blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
          write_mask: wgpu::ColorWrites::ALL,
        })],
        compilation_options: wgpu::PipelineCompilationOptions::default(),
      }),
      multiview: None,
      cache: None,
    });
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("mesh"),
      size: SLOT_STRIDE * SLOTS,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
      label: Some("mesh"),
      layout: &layout,
      entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
          buffer: &buffer,
          offset: 0,
          size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
        }),
      }],
    });
    Self {
      pipeline,
      buffer,
      bind_group,
    }
  }
}

impl shader::Primitive for MeshPrimitive {
  type Pipeline = MeshPipeline;

  fn prepare(
    &self,
    pipeline: &mut MeshPipeline,
    _device: &wgpu::Device,
    queue: &wgpu::Queue,
    bounds: &Rectangle,
    viewport: &Viewport,
  ) {
    let scale = viewport.scale_factor();
    let mut uniforms = self.uniforms;
    uniforms.area = [
      bounds.width * scale,
      bounds.height * scale,
      self.inset * scale,
      self.radius * scale,
    ];
    uniforms.blur = [self.blur * scale, 0.0, 0.0, 0.0];
    queue.write_buffer(
      &pipeline.buffer,
      self.slot * SLOT_STRIDE,
      bytemuck::bytes_of(&uniforms),
    );
  }

  fn draw(&self, pipeline: &MeshPipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
    render_pass.set_pipeline(&pipeline.pipeline);
    render_pass.set_bind_group(0, &pipeline.bind_group, &[(self.slot * SLOT_STRIDE) as u32]);
    render_pass.draw(0..3, 0..1);
    true
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn close(actual: (f32, f32, f32), expected: (f32, f32, f32)) -> bool {
    (actual.0 - expected.0).abs() < 1e-5
      && (actual.1 - expected.1).abs() < 1e-5
      && (actual.2 - expected.2).abs() < 1e-5
  }

  #[test]
  fn the_float_starts_at_rest_and_comes_back() {
    assert_eq!(mesh_float(0.0), (1.0, 0.0, 0.0));
    assert_eq!(mesh_float(2.0), (1.0, 0.0, 0.0));
  }

  #[test]
  fn the_float_passes_by_its_middle_key_and_ends_on_the_last() {
    // `styles.css:171-173` : 50 % `scale(1.06) translate(2%, 1.5%)`, 100 % `scale(1.03) translate(-1.5%, 1%)`.
    assert!(close(mesh_float(0.5), (1.06, 0.02, 0.015)));
    assert!(close(mesh_float(1.0), (1.03, -0.015, 0.01)));
  }

  #[test]
  fn each_segment_has_its_own_ease_in_out() {
    // `cubic-bezier(0.42, 0, 0.58, 1)` vaut 0,5 à mi-segment : milieu de chaque segment entre deux clés.
    assert!(close(mesh_float(0.25), (1.03, 0.01, 0.0075)));
    assert!(close(mesh_float(0.75), (1.045, 0.0025, 0.0125)));
    // À un quart de segment la courbe vaut 0,1291 (et non 0,25 : l'ancien lissage global donnait 0,156 sur tout le cycle).
    let (scale, x, y) = mesh_float(0.125);
    let eased = bezier_ease(tokens::FLOAT_CURVE, 0.25);
    assert!((eased - 0.1291).abs() < 1e-3, "{eased}");
    assert!(close(
      (scale, x, y),
      (1.0 + 0.06 * eased, 0.02 * eased, 0.015 * eased)
    ));
    // Le segment 2 repart de zéro : la courbe n'est pas appliquée une seule fois sur tout l'aller.
    assert!(close(mesh_float(0.625), {
      let eased = bezier_ease(tokens::FLOAT_CURVE, 0.25);
      (
        1.06 - 0.03 * eased,
        0.02 - 0.035 * eased,
        0.015 - 0.005 * eased,
      )
    }));
  }

  #[test]
  fn the_return_replays_the_path_backwards() {
    for step in 0..=20 {
      let at = step as f32 / 20.0;
      assert!(close(mesh_float(at), mesh_float(2.0 - at)), "{at}");
    }
    assert!(close(mesh_float(1.5), mesh_float(0.5)));
  }

  #[test]
  fn the_phase_follows_the_period_of_the_speed() {
    // Vitesse 20 : `120 / (20 * 0.2 + 1)` = 24 s (`app.js:16149`).
    let at = |ms: u64| phase(Duration::from_millis(ms), 24.0);
    assert_eq!(at(0), 0.0);
    assert!((at(6000) - 0.25).abs() < 1e-6);
    assert!((at(12_000) - 0.5).abs() < 1e-6);
    assert!((at(18_000) - 0.75).abs() < 1e-6);
    assert!((at(24_000) - 1.0).abs() < 1e-6);
    assert!((at(36_000) - 1.5).abs() < 1e-6);
    assert!(at(48_000).abs() < 1e-6);
    assert!(
      (at(48_000 + 6000) - 0.25).abs() < 1e-6,
      "l'itération suivante repart de l'aller"
    );
  }

  #[test]
  fn the_layer_fade_follows_the_ease_of_the_id_selector() {
    // `ease` (`styles.css:9186-9193`) : 0 à 0 ms, 0,8024 à 250 ms, 1 à 500 ms.
    assert_eq!(layer_fade(0.0), 0.0);
    assert!(
      (layer_fade(250.0) - 0.8024).abs() < 2e-3,
      "{}",
      layer_fade(250.0)
    );
    assert_eq!(layer_fade(500.0), 1.0);
    assert_eq!(layer_fade(900.0), 1.0);
  }

  #[test]
  fn the_brightness_scales_the_alphas_of_the_theme() {
    let program = MeshProgram {
      config: MeshConfig::default(),
      first: Color::WHITE,
      second: Color::BLACK,
      background: Color::BLACK,
      brightness: 0.5,
      dark: true,
      phase: 0.0,
      inset: 0.0,
      radius: 0.0,
      slot: 0,
    };
    let uniforms = program.uniforms();
    assert!((uniforms.first[3] - 0.275).abs() < 1e-6);
    assert!((uniforms.second[3] - 0.225).abs() < 1e-6);
    assert_eq!(uniforms.points, [0.2, 0.0, 0.85, 0.05]);
    let light = MeshProgram {
      dark: false,
      brightness: 1.0,
      ..program
    }
    .uniforms();
    assert!((light.first[3] - 0.22).abs() < 1e-6);
  }

  /// Miroir CPU de `mesh.wgsl` (`content`) et référence du flou : somme dense, sans l'approximation du shader.
  mod blur {
    const LAYER: (f64, f64) = (1960.0, 1160.0);
    const FIRST: [f64; 4] = [0.32, 0.28, 0.82, 0.55];
    const SECOND: [f64; 4] = [0.60, 0.50, 0.30, 0.45];
    const BACKGROUND: [f64; 3] = [0.07, 0.07, 0.08];

    fn radial(q: (f64, f64), center: (f64, f64), stop: f64) -> f64 {
      let side = |c: f64| c.max(1.0 - c) * std::f64::consts::SQRT_2;
      let distance = (((q.0 - center.0) / side(center.0)).powi(2)
        + ((q.1 - center.1) / side(center.1)).powi(2))
      .sqrt();
      (1.0 - distance / stop).clamp(0.0, 1.0)
    }

    fn content(local: (f64, f64)) -> [f64; 3] {
      let q = (local.0 / LAYER.0, local.1 / LAYER.1);
      let mut color = BACKGROUND;
      for (layer, center, stop) in [(SECOND, (0.85, 0.05), 0.65), (FIRST, (0.2, 0.0), 0.70)] {
        let part = radial(q, center, stop) * layer[3];
        for (channel, base) in color.iter_mut().enumerate() {
          *base += (layer[channel] - *base) * part;
        }
      }
      let ramp = ((q.1 - 0.35) / 0.5).clamp(0.0, 1.0);
      let mut out = color;
      for (channel, value) in out.iter_mut().enumerate() {
        *value += (BACKGROUND[channel] - *value) * ramp;
      }
      out
    }

    fn inside(at: (f64, f64)) -> bool {
      (0.0..=LAYER.0).contains(&at.0) && (0.0..=LAYER.1).contains(&at.1)
    }

    fn cdf(z: f64) -> f64 {
      // Même approximation que le shader (Abramowitz et Stegun 7.1.26).
      let x = z.abs() * std::f64::consts::FRAC_1_SQRT_2;
      let t = 1.0 / (1.0 + 0.327_591_1 * x);
      let poly = ((((1.061_405_429 * t - 1.453_152_027) * t + 1.421_413_741) * t - 0.284_496_736)
        * t
        + 0.254_829_592)
        * t;
      let erf = 1.0 - poly * (-x * x).exp();
      0.5 * (1.0 + if z >= 0.0 { erf } else { -erf })
    }

    /// Ce que calcule `blurred` du shader.
    pub fn approx(local: (f64, f64), sigma: f64) -> [f64; 3] {
      let covered = |at: f64, size: f64| cdf((size - at) / sigma) - cdf(-at / sigma);
      let weight = covered(local.0, LAYER.0) * covered(local.1, LAYER.1);
      let mut sum = [0.0; 3];
      let mut total = 0.0;
      for j in -4..=4 {
        for i in -4..=4 {
          let offset = (f64::from(i) * 0.75, f64::from(j) * 0.75);
          let at = (local.0 + offset.0 * sigma, local.1 + offset.1 * sigma);
          if inside(at) {
            let w = (-0.5 * (offset.0 * offset.0 + offset.1 * offset.1)).exp();
            for (acc, value) in sum.iter_mut().zip(content(at)) {
              *acc += value * w;
            }
            total += w;
          }
        }
      }
      let mut out = BACKGROUND;
      if total > 0.0 {
        for (channel, value) in out.iter_mut().enumerate() {
          *value += (sum[channel] / total - *value) * weight;
        }
      }
      out
    }

    /// Flou gaussien exact (au pas de 0,2 sigma sur +-4 sigma), le dehors du calque valant le fond.
    pub fn exact(local: (f64, f64), sigma: f64) -> [f64; 3] {
      let mut sum = [0.0; 3];
      let mut total = 0.0;
      let step = 0.2;
      let count = (4.0 / step) as i32;
      for j in -count..=count {
        for i in -count..=count {
          let offset = (f64::from(i) * step, f64::from(j) * step);
          let at = (local.0 + offset.0 * sigma, local.1 + offset.1 * sigma);
          let w = (-0.5 * (offset.0 * offset.0 + offset.1 * offset.1)).exp();
          let value = if inside(at) { content(at) } else { BACKGROUND };
          for (acc, value) in sum.iter_mut().zip(value) {
            *acc += value * w;
          }
          total += w;
        }
      }
      sum.map(|value| value / total)
    }

    pub fn layer() -> (f64, f64) {
      LAYER
    }
  }

  #[test]
  fn the_blur_approximation_stays_within_one_and_a_half_levels() {
    let (width, height) = blur::layer();
    let mut worst = 0.0_f64;
    for sigma in [4.0, 15.0, 28.0, 56.0] {
      // Toute la fenêtre (bords et coins compris : le flou y mêle le fond) sur une grille de 14 x 9.
      for column in 0..=13 {
        for row in 0..=8 {
          let local = (
            40.0 + (width - 80.0) * f64::from(column) / 13.0,
            40.0 + (height - 80.0) * f64::from(row) / 8.0,
          );
          let (a, b) = (blur::approx(local, sigma), blur::exact(local, sigma));
          for channel in 0..3 {
            worst = worst.max((a[channel] - b[channel]).abs() * 255.0);
          }
        }
      }
    }
    assert!(worst < 1.5, "écart maximal : {worst:.3} niveaux sur 255");
  }
}
