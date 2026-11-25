//! Formes de `shapes.js` : profils radiaux de 360 échantillons.

use std::f64::consts::{PI, TAU};

/// Nombre d'échantillons d'un profil.
pub const SAMPLE_COUNT: usize = 360;
/// Maintien d'une forme, puis durée de la transition, en secondes.
pub const HOLD_SECS: f32 = 2.0;
pub const MORPH_SECS: f32 = 1.25;

/// Noms des formes, dans l'ordre de `SHAPE_DEFINITIONS`.
pub const NAMES: [&str; 4] = [
  "Cookie7Sided",
  "Cookie9Sided",
  "Cookie12Sided",
  "Clover4Leaf",
];

const PROFILE_RADIUS: f64 = 160.0;
const BISECTION_STEPS: usize = 56;

type P = [f64; 2];
type Curve = [P; 4];

struct Cookie {
  petals: usize,
  center: P,
  stretch: f64,
  rim: P,
  handle: P,
  first_flank: [P; 3],
  second_flank: [P; 3],
  valley_handle: P,
}

const COOKIES: [Cookie; 3] = [
  Cookie {
    petals: 7,
    center: [160.0, 608.78580952],
    stretch: 1.00227424,
    rim: [154.0236, 15.1787],
    handle: [167.8238, 5.6686],
    first_flank: [
      [153.2414, 15.8961],
      [152.2361, 16.7034],
      [150.3202, 18.3516],
    ],
    second_flank: [[149.7071, 18.9139], [149.4061, 19.195], [149.1302, 19.4773]],
    valley_handle: [145.6613, 23.5231],
  },
  Cookie {
    petals: 9,
    center: [1503.0, 163.34198056],
    stretch: 1.01036691,
    rim: [154.3557, 11.7227],
    handle: [165.6694, 4.4348],
    first_flank: [
      [153.9368, 12.0547],
      [153.4294, 12.4257],
      [152.4353, 13.1735],
    ],
    second_flank: [
      [151.9994, 13.5097],
      [151.7843, 13.6784],
      [151.5812, 13.8451],
    ],
    valley_handle: [147.3456, 17.7843],
  },
  Cookie {
    petals: 12,
    center: [1974.0, 604.001],
    stretch: 1.0,
    rim: [152.8016, 8.376],
    handle: [163.2344, 3.4256],
    first_flank: [[152.6152, 8.4738], [152.3933, 8.5891], [151.9524, 8.8207]],
    second_flank: [[151.6913, 8.9609], [151.5576, 9.0305], [151.4338, 9.0971]],
    valley_handle: [145.4447, 12.6887],
  },
];

fn point_on_curve(curve: &Curve, t: f64) -> P {
  let rest = 1.0 - t;
  let (a, b, c, d) = (
    rest * rest * rest,
    3.0 * rest * rest * t,
    3.0 * rest * t * t,
    t * t * t,
  );
  [
    a * curve[0][0] + b * curve[1][0] + c * curve[2][0] + d * curve[3][0],
    a * curve[0][1] + b * curve[1][1] + c * curve[2][1] + d * curve[3][1],
  ]
}

fn straight_line(from: P, to: P) -> Curve {
  let step = [(to[0] - from[0]) / 3.0, (to[1] - from[1]) / 3.0];
  [
    from,
    [from[0] + step[0], from[1] + step[1]],
    [from[0] + 2.0 * step[0], from[1] + 2.0 * step[1]],
    to,
  ]
}

/// Le reste garde le signe du dividende, comme `%` en JS.
fn normalize_angle(difference: f64) -> f64 {
  let mut result = difference % TAU;
  if result > PI {
    result -= TAU;
  }
  if result <= -PI {
    result += TAU;
  }
  result
}

fn cookie_outline(cookie: &Cookie) -> Vec<Curve> {
  let lobe_step = TAU / cookie.petals as f64;
  let polar = |radius: f64, angle: f64| {
    [
      cookie.center[0] + radius * angle.cos(),
      cookie.center[1] + cookie.stretch * radius * angle.sin(),
    ]
  };
  let (rim, handle, first, second, valley) = (
    cookie.rim,
    cookie.handle,
    cookie.first_flank,
    cookie.second_flank,
    cookie.valley_handle,
  );
  let mut outline = Vec::with_capacity(cookie.petals * 6);
  for lobe in 0..cookie.petals {
    let base = -PI / 2.0 + lobe as f64 * lobe_step;
    let place = |p: P| polar(p[0], base + p[1] * (PI / 180.0));
    let mirror = |p: P| polar(p[0], base + lobe_step - p[1] * (PI / 180.0));
    let left = |p: P| place([p[0], -p[1]]);
    outline.push([left(rim), left(handle), place(handle), place(rim)]);
    outline.push([
      place(rim),
      place(first[0]),
      place(first[1]),
      place(first[2]),
    ]);
    outline.push([
      place(first[2]),
      place(second[0]),
      place(second[1]),
      place(second[2]),
    ]);
    outline.push([
      place(second[2]),
      place(valley),
      mirror(valley),
      mirror(second[2]),
    ]);
    outline.push([
      mirror(second[2]),
      mirror(second[1]),
      mirror(second[0]),
      mirror(first[2]),
    ]);
    outline.push([
      mirror(first[2]),
      mirror(first[1]),
      mirror(first[0]),
      mirror(rim),
    ]);
  }
  outline
}

fn clover_outline() -> Vec<Curve> {
  const CENTER: P = [1066.99925, 604.003];
  const OUTER_START: P = [38.73, -133.7908];
  const OUTER_HANDLE: P = [98.8705, -159.911];
  const INNER_START: P = [129.471, -28.79];
  const INNER_HANDLE: P = [121.4998, -10.42];
  let turn = |p: P, quarters: usize| {
    let (mut x, mut y) = (p[0], p[1]);
    for _ in 0..quarters {
      (x, y) = (-y, x);
    }
    [CENTER[0] + x, CENTER[1] + y]
  };
  let mut outline = Vec::with_capacity(16);
  for quarter in 0..4 {
    let outer = [
      turn(OUTER_START, quarter),
      turn(OUTER_HANDLE, quarter),
      turn([-OUTER_HANDLE[1], -OUTER_HANDLE[0]], quarter),
      turn([-OUTER_START[1], -OUTER_START[0]], quarter),
    ];
    let inner = [
      turn(INNER_START, quarter),
      turn(INNER_HANDLE, quarter),
      turn([INNER_HANDLE[0], -INNER_HANDLE[1]], quarter),
      turn([INNER_START[0], -INNER_START[1]], quarter),
    ];
    outline.push(outer);
    outline.push(straight_line(outer[3], inner[0]));
    outline.push(inner);
    outline.push(straight_line(inner[3], turn(OUTER_START, quarter + 1)));
  }
  outline
}

fn curve_bounds(curve: &Curve) -> [f64; 4] {
  let mut bounds = [
    f64::INFINITY,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NEG_INFINITY,
  ];
  let mut include = |p: P| {
    bounds[0] = bounds[0].min(p[0]);
    bounds[1] = bounds[1].min(p[1]);
    bounds[2] = bounds[2].max(p[0]);
    bounds[3] = bounds[3].max(p[1]);
  };
  include(curve[0]);
  include(curve[3]);
  for axis in 0..2 {
    let first = curve[1][axis] - curve[0][axis];
    let second = curve[2][axis] - curve[1][axis];
    let third = curve[3][axis] - curve[2][axis];
    let a = first - 2.0 * second + third;
    let b = 2.0 * (second - first);
    let mut roots = Vec::with_capacity(2);
    if a.abs() < 1e-12 {
      if b.abs() > 1e-12 {
        roots.push(-first / b);
      }
    } else {
      let discriminant = b * b - 4.0 * a * first;
      if discriminant >= 0.0 {
        let root = discriminant.sqrt();
        roots.push((-b + root) / (2.0 * a));
        roots.push((-b - root) / (2.0 * a));
      }
    }
    for t in roots {
      if t > 0.0 && t < 1.0 {
        include(point_on_curve(curve, t));
      }
    }
  }
  bounds
}

fn bounds_center(outline: &[Curve]) -> P {
  let mut total = [
    f64::INFINITY,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NEG_INFINITY,
  ];
  for curve in outline {
    let bounds = curve_bounds(curve);
    total[0] = total[0].min(bounds[0]);
    total[1] = total[1].min(bounds[1]);
    total[2] = total[2].max(bounds[2]);
    total[3] = total[3].max(bounds[3]);
  }
  [(total[0] + total[2]) / 2.0, (total[1] + total[3]) / 2.0]
}

fn angle_from(point: P, center: P) -> f64 {
  (point[1] - center[1]).atan2(point[0] - center[0])
}

struct Section {
  curve: Curve,
  start: f64,
  end: f64,
}

fn sections_of(outline: &[Curve], center: P) -> Vec<Section> {
  let mut angle = angle_from(outline[0][0], center);
  outline
    .iter()
    .map(|curve| {
      let start = angle;
      let end = start + normalize_angle(angle_from(curve[3], center) - start);
      angle = end;
      Section {
        curve: *curve,
        start,
        end,
      }
    })
    .collect()
}

fn ascending_sections(outline: &[Curve], center: P) -> Vec<Section> {
  let sections = sections_of(outline, center);
  let sweep = sections[sections.len() - 1].end - sections[0].start;
  if sweep >= 0.0 {
    return sections;
  }
  let reversed: Vec<Curve> = outline
    .iter()
    .rev()
    .map(|curve| [curve[3], curve[2], curve[1], curve[0]])
    .collect();
  sections_of(&reversed, center)
}

fn measure_radius(sections: &[Section], center: P, angle: f64) -> f64 {
  let first = sections[0].start;
  let target = first + (((angle - first) % TAU) + TAU) % TAU;
  let section = sections
    .iter()
    .find(|section| target >= section.start && target <= section.end)
    .unwrap_or(&sections[sections.len() - 1]);
  let (mut low, mut high) = (0.0, 1.0);
  for _ in 0..BISECTION_STEPS {
    let middle = f64::midpoint(low, high);
    let point = point_on_curve(&section.curve, middle);
    let angle = section.start + normalize_angle(angle_from(point, center) - section.start);
    if angle < target {
      low = middle;
    } else {
      high = middle;
    }
  }
  let point = point_on_curve(&section.curve, f64::midpoint(low, high));
  (point[0] - center[0]).hypot(point[1] - center[1])
}

fn compute_profile(outline: &[Curve]) -> Vec<f64> {
  let center = bounds_center(outline);
  let sections = ascending_sections(outline, center);
  let radii: Vec<f64> = (0..SAMPLE_COUNT)
    .map(|index| {
      measure_radius(
        &sections,
        center,
        -PI + TAU * index as f64 / SAMPLE_COUNT as f64,
      )
    })
    .collect();
  let scale = PROFILE_RADIUS / radii.iter().copied().fold(0.0, f64::max);
  radii.into_iter().map(|radius| radius * scale).collect()
}

/// Profil radial d'une forme.
#[must_use]
pub fn profile(index: usize) -> Vec<f64> {
  match COOKIES.get(index) {
    Some(cookie) => compute_profile(&cookie_outline(cookie)),
    None => compute_profile(&clover_outline()),
  }
}

/// Profils des 4 formes en `f32`, ce que dessine l'interface. `Float32Array` en JS.
#[must_use]
pub fn all_profiles() -> Vec<Vec<f32>> {
  (0..NAMES.len())
    .map(|index| {
      profile(index)
        .into_iter()
        .map(|radius| radius as f32)
        .collect()
    })
    .collect()
}

/// `easeInOutCubic`.
#[must_use]
pub fn ease_in_out_cubic(progress: f32) -> f32 {
  if progress < 0.5 {
    4.0 * progress * progress * progress
  } else {
    1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::Value;

  const TOLERANCE: f64 = 1e-3;

  fn reference() -> Value {
    serde_json::from_str(include_str!("shapes-reference.json")).unwrap()
  }

  fn reference_radii(index: usize) -> Vec<f64> {
    reference()["shapes"][index]["radii"]
      .as_array()
      .unwrap()
      .iter()
      .map(|value| value.as_f64().unwrap())
      .collect()
  }

  #[test]
  fn the_constants_are_those_of_the_javascript() {
    let reference = reference();
    assert_eq!(reference["sampleCount"], SAMPLE_COUNT);
    assert_eq!(
      reference["holdMs"].as_f64().unwrap() as f32 / 1000.0,
      HOLD_SECS
    );
    assert_eq!(
      reference["morphMs"].as_f64().unwrap() as f32 / 1000.0,
      MORPH_SECS
    );
    for (index, name) in NAMES.iter().enumerate() {
      assert_eq!(reference["shapes"][index]["name"], *name);
    }
  }

  #[test]
  fn each_sample_matches_the_javascript() {
    for (index, name) in NAMES.iter().enumerate() {
      let (ours, theirs) = (profile(index), reference_radii(index));
      assert_eq!(ours.len(), SAMPLE_COUNT);
      for (sample, (a, b)) in ours.iter().zip(&theirs).enumerate() {
        assert!(
          (a - b).abs() <= TOLERANCE,
          "{name} échantillon {sample} : {a} contre {b}"
        );
      }
    }
  }

  #[test]
  fn the_envelope_matches_the_javascript() {
    for (index, name) in NAMES.iter().enumerate() {
      let envelope = |radii: &[f64]| {
        (
          radii.iter().copied().fold(f64::INFINITY, f64::min),
          radii.iter().copied().fold(0.0, f64::max),
        )
      };
      let (ours, theirs) = (envelope(&profile(index)), envelope(&reference_radii(index)));
      assert!((ours.0 - theirs.0).abs() <= TOLERANCE, "{name}");
      assert!((ours.1 - theirs.1).abs() <= TOLERANCE, "{name}");
      assert!((ours.1 - 160.0).abs() <= TOLERANCE, "{name}");
    }
  }

  #[test]
  fn each_shape_has_the_symmetry_of_the_javascript() {
    // Écart entre le profil et sa rotation d'un lobe : même valeur que le code JS.
    for (index, lobes) in [(0, 7_usize), (1, 9), (2, 12), (3, 4)] {
      let step = SAMPLE_COUNT / lobes;
      let asymmetry = |radii: &[f64]| {
        (0..SAMPLE_COUNT)
          .map(|sample| (radii[sample] - radii[(sample + step) % SAMPLE_COUNT]).abs())
          .fold(0.0, f64::max)
      };
      let (ours, theirs) = (
        asymmetry(&profile(index)),
        asymmetry(&reference_radii(index)),
      );
      assert!(
        (ours - theirs).abs() <= 2.0 * TOLERANCE,
        "{} : {ours} contre {theirs}",
        NAMES[index]
      );
    }
  }
}
