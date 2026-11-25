// Calque `.bg-mesh-layer` : `linear-gradient(to bottom, transparent 35%, background 85%)` sur deux `radial-gradient(ellipse at ...)`
// et le fond, dans une boîte qui dépasse la zone dessinée de `inset` de chaque côté, puis `filter: blur()` et `transform`.
struct Uniforms {
  first: vec4<f32>,
  second: vec4<f32>,
  background: vec4<f32>,
  points: vec4<f32>,
  // échelle, décalage x, décalage y (fractions du calque)
  transform: vec4<f32>,
  // largeur et hauteur de la zone dessinée, `inset`, rayon des coins (pixels physiques)
  area: vec4<f32>,
  // écart type du flou en pixels physiques, dans le repère du calque
  blur: vec4<f32>,
};

@group(0) @binding(0) var<uniform> mesh: Uniforms;

struct VertexOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32) -> VertexOut {
  let x = f32((index << 1u) & 2u);
  let y = f32(index & 2u);
  var out: VertexOut;
  out.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
  out.uv = vec2<f32>(x, y);
  return out;
}

// `farthest-corner` : même rapport que `farthest-side`, agrandi de racine de 2.
fn radial(q: vec2<f32>, center: vec2<f32>, stop: f32) -> f32 {
  let side = max(center, vec2<f32>(1.0) - center) * 1.41421356;
  let distance = length((q - center) / max(side, vec2<f32>(0.001)));
  return clamp(1.0 - distance / stop, 0.0, 1.0);
}

// Couleur du calque au point `q` (fractions de sa boîte) : les dégradés sont à alpha prémultiplié, donc la couleur reste celle de
// l'arrêt de départ pendant que l'alpha tombe à zéro.
fn content(q: vec2<f32>) -> vec3<f32> {
  var color = mesh.background.rgb;
  color = mix(color, mesh.second.rgb, radial(q, mesh.points.zw, 0.65) * mesh.second.a);
  color = mix(color, mesh.first.rgb, radial(q, mesh.points.xy, 0.70) * mesh.first.a);
  let ramp = clamp((q.y - 0.35) / 0.5, 0.0, 1.0);
  return mix(color, mesh.background.rgb, ramp);
}

fn inside(local: vec2<f32>, layer: vec2<f32>) -> bool {
  return all(local >= vec2<f32>(0.0)) && all(local <= layer);
}

// Fonction de répartition de la loi normale centrée réduite (erf d'Abramowitz et Stegun 7.1.26, erreur < 1,5e-7).
fn normal_cdf(z: f32) -> f32 {
  let x = abs(z) * 0.70710678;
  let t = 1.0 / (1.0 + 0.3275911 * x);
  let poly = ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t;
  let erf = 1.0 - poly * exp(-x * x);
  return 0.5 * (1.0 + select(-erf, erf, z >= 0.0));
}

fn covered(z0: f32, z1: f32, sigma: f32) -> f32 {
  return normal_cdf(z1 / sigma) - normal_cdf(z0 / sigma);
}

// Flou gaussien du calque, dont le bord est une coupure franche vers le fond de la fenêtre (le fond de `body` est la même couleur).
// Part de calque couvrante : exacte, produit de deux différences de la loi normale. Couleur du calque : moyenne pondérée de 9 x 9
// échantillons espacés de 0,75 sigma (de -3 à +3 sigma), limitée aux échantillons qui tombent dans la boîte.
fn blurred(local: vec2<f32>, layer: vec2<f32>, sigma: f32) -> vec3<f32> {
  let weight = covered(-local.x, layer.x - local.x, sigma) * covered(-local.y, layer.y - local.y, sigma);
  var sum = vec3<f32>(0.0);
  var total = 0.0;
  for (var j = -4; j <= 4; j = j + 1) {
    for (var i = -4; i <= 4; i = i + 1) {
      let offset = vec2<f32>(f32(i), f32(j)) * 0.75;
      let at = local + offset * sigma;
      if (inside(at, layer)) {
        let w = exp(-0.5 * dot(offset, offset));
        sum = sum + content(at / layer) * w;
        total = total + w;
      }
    }
  }
  let average = select(mesh.background.rgb, sum / max(total, 1e-6), total > 0.0);
  return mix(mesh.background.rgb, average, weight);
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
  let size = mesh.area.xy;
  let inset = mesh.area.z;
  let layer = size + vec2<f32>(2.0 * inset);
  let center = layer * 0.5;
  // `scale(s) translate(x, y)` : le décalage d'abord, puis l'échelle autour du centre ; on remonte du pixel au calque.
  let local = center + (in.uv * size + vec2<f32>(inset) - center) / mesh.transform.x - mesh.transform.yz * layer;
  var color = mesh.background.rgb;
  if (mesh.blur.x > 0.01) {
    color = blurred(local, layer, mesh.blur.x);
  } else if (inside(local, layer)) {
    color = content(local / layer);
  }
  // Coins arrondis de l'aperçu (`overflow: hidden` + `border-radius`) : couverture du rectangle arrondi.
  var cover = 1.0;
  let radius = mesh.area.w;
  if (radius > 0.0) {
    let half = size * 0.5;
    let corner = max(abs(in.uv * size - half) - (half - vec2<f32>(radius)), vec2<f32>(0.0));
    cover = clamp(0.5 - (length(corner) - radius), 0.0, 1.0);
  }
  return vec4<f32>(color * cover, cover);
}
