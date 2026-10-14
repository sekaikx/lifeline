//! Reproducible random numbers and 2-D dispersion statistics.

use serde::{Deserialize, Serialize};

/// xoshiro256** PRNG seeded through SplitMix64. Small, fast, and
/// bit-reproducible on every platform, so a Monte Carlo seed always gives the
/// same landing pattern.
#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
    spare: Option<f64>,
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let s = [
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
        ];
        Self { s, spare: None }
    }

    /// Independent stream `index` derived from `seed` (one per Monte Carlo
    /// sample, so results do not depend on thread scheduling).
    pub fn stream(seed: u64, index: u64) -> Self {
        let mut sm = seed ^ index.wrapping_mul(0xD1B5_4A32_D192_ED03);
        Self::new(splitmix64(&mut sm))
    }

    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform in `[0, 1)`.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Standard normal via the Box-Muller transform.
    pub fn normal(&mut self) -> f64 {
        if let Some(z) = self.spare.take() {
            return z;
        }
        let u1 = 1.0 - self.uniform(); // (0, 1], safe for ln
        let u2 = self.uniform();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = std::f64::consts::TAU * u2;
        self.spare = Some(r * theta.sin());
        r * theta.cos()
    }
}

pub fn mean2(points: &[[f64; 2]]) -> [f64; 2] {
    let n = points.len().max(1) as f64;
    let (sx, sy) = points
        .iter()
        .fold((0.0, 0.0), |(a, b), p| (a + p[0], b + p[1]));
    [sx / n, sy / n]
}

/// Sample covariance matrix `[[sxx, sxy], [sxy, syy]]`.
pub fn covariance2(points: &[[f64; 2]]) -> [[f64; 2]; 2] {
    let m = mean2(points);
    let n = (points.len().max(2) - 1) as f64;
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for p in points {
        let (dx, dy) = (p[0] - m[0], p[1] - m[1]);
        xx += dx * dx;
        xy += dx * dy;
        yy += dy * dy;
    }
    [[xx / n, xy / n], [xy / n, yy / n]]
}

pub fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        0.5 * (values[n / 2 - 1] + values[n / 2])
    }
}

/// Confidence ellipse of a bivariate normal.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ellipse {
    /// Probability mass enclosed (e.g. 0.95).
    pub probability: f64,
    pub center: [f64; 2],
    pub semi_major: f64,
    pub semi_minor: f64,
    /// Major-axis angle, radians counter-clockwise from east.
    pub angle: f64,
    /// Fraction of the actual samples that fell inside (a model check).
    pub observed_fraction: f64,
}

impl Ellipse {
    /// For a 2-D Gaussian, `d^2 = x^T S^-1 x` is chi-square with 2 degrees of
    /// freedom, whose quantile has the closed form `-2 ln(1 - p)`.
    pub fn from_covariance(center: [f64; 2], cov: [[f64; 2]; 2], probability: f64) -> Self {
        let (a, b, c) = (cov[0][0], cov[0][1], cov[1][1]);
        let mid = 0.5 * (a + c);
        let rad = (0.25 * (a - c).powi(2) + b * b).sqrt();
        let (l1, l2) = ((mid + rad).max(0.0), (mid - rad).max(0.0));
        let scale = (-2.0 * (1.0 - probability).ln()).sqrt();
        Self {
            probability,
            center,
            semi_major: scale * l1.sqrt(),
            semi_minor: scale * l2.sqrt(),
            angle: 0.5 * (2.0 * b).atan2(a - c),
            observed_fraction: f64::NAN,
        }
    }

    pub fn contains(&self, p: [f64; 2]) -> bool {
        let (dx, dy) = (p[0] - self.center[0], p[1] - self.center[1]);
        let (s, c) = self.angle.sin_cos();
        let u = dx * c + dy * s;
        let v = -dx * s + dy * c;
        if self.semi_minor <= 0.0 || self.semi_major <= 0.0 {
            return u.abs() <= self.semi_major + 1e-12 && v.abs() <= 1e-12;
        }
        (u / self.semi_major).powi(2) + (v / self.semi_minor).powi(2) <= 1.0
    }
}
