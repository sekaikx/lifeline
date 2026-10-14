//! Risk along a route: line of sight, exposure fields and survival.
//!
//! Risk is modelled as a hazard rate. While a vehicle is exposed to a risk
//! source with rate `λ` (expected interdictions per unit time), the chance of
//! getting through a stretch of route is
//!
//! ```text
//! P(survive) = exp(-∫ λ(t) dt)
//! ```
//!
//! Two kinds of source:
//!
//! * a [`RiskSite`] (an observed checkpoint, a hostile position, an area of
//!   unrest) exposes only what it can **see**: terrain masking matters, so
//!   routes behind ridges are safer, and a drone at 60 m is seen from farther
//!   than a truck on the road;
//! * a [`HazardArea`] (minefield, flooding, contamination) is a polygon that
//!   affects ground movement inside it, and can be marked impassable.
//!
//! Rates are entered per minute, which is how people estimate them, and
//! stored per second in [`Raster`] fields.

use crate::raster::Raster;
use crate::terrain::TerrainGrid;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RiskSite {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    /// Observer height above ground [m]
    pub height: f64,
    /// Maximum distance at which it can interdict [m]
    pub range: f64,
    /// Expected interdictions per minute of exposure
    pub rate_per_min: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HazardArea {
    pub id: String,
    pub name: String,
    /// Polygon vertices (east, north) [m], not closed
    pub polygon: Vec<[f64; 2]>,
    /// Expected losses per minute inside, for ground vehicles
    pub rate_per_min: f64,
    /// Ground vehicles may not enter at all
    pub impassable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RiskModel {
    pub sites: Vec<RiskSite>,
    pub hazards: Vec<HazardArea>,
}

impl RiskModel {
    pub fn validate(&self) -> Result<(), String> {
        for s in &self.sites {
            if !(s.range > 0.0) || s.rate_per_min < 0.0 || s.height < 0.0 {
                return Err(format!(
                    "risk site {}: range must be positive, rate and height non-negative",
                    s.name
                ));
            }
        }
        for h in &self.hazards {
            if h.polygon.len() < 3 || h.rate_per_min < 0.0 {
                return Err(format!(
                    "hazard area {}: needs 3+ vertices and a non-negative rate",
                    h.name
                ));
            }
        }
        Ok(())
    }

    /// True if the segment `a -> b` enters any impassable hazard area.
    pub fn blocks(&self, a: [f64; 2], b: [f64; 2], step: f64) -> bool {
        let impassable: Vec<&HazardArea> = self.hazards.iter().filter(|h| h.impassable).collect();
        if impassable.is_empty() {
            return false;
        }
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let n = (len / step).ceil().max(1.0) as usize;
        (0..=n).any(|k| {
            let t = k as f64 / n as f64;
            let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            impassable.iter().any(|h| point_in_polygon(p, &h.polygon))
        })
    }
}

/// Even-odd ray casting test.
pub fn point_in_polygon(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Is the straight line between two points (heights MSL) clear of terrain?
/// Samples the terrain every `step` metres along the line.
pub fn line_of_sight(terrain: &TerrainGrid, from: [f64; 3], to: [f64; 3], step: f64) -> bool {
    let len = (to[0] - from[0]).hypot(to[1] - from[1]);
    let n = (len / step).ceil() as usize;
    (1..n).all(|k| {
        let t = k as f64 / n as f64;
        let x = from[0] + (to[0] - from[0]) * t;
        let y = from[1] + (to[1] - from[1]) * t;
        let z = from[2] + (to[2] - from[2]) * t;
        terrain.at(x, y) < z
    })
}

/// Cells (on the terrain's own grid) from which a target `target_agl`
/// metres above the ground is visible to `site` and within its range.
pub fn viewshed(terrain: &TerrainGrid, site: &RiskSite, target_agl: f64) -> Raster {
    let mut out = Raster::new(
        terrain.x0, terrain.y0, terrain.dx, terrain.nx, terrain.ny, 0.0,
    );
    let eye = [site.x, site.y, terrain.at(site.x, site.y) + site.height];
    let r = (site.range / terrain.dx).ceil() as i64;
    // A site just off the map still sees onto it, so clamp its cell.
    let clamp = |v: f64, n: usize| v.round().clamp(0.0, (n - 1) as f64) as usize;
    let ci = clamp((site.x - terrain.x0) / terrain.dx, terrain.nx);
    let cj = clamp((site.y - terrain.y0) / terrain.dy, terrain.ny);
    let (i0, i1) = (
        (ci as i64 - r).max(0) as usize,
        ((ci as i64 + r) as usize).min(terrain.nx - 1),
    );
    let (j0, j1) = (
        (cj as i64 - r).max(0) as usize,
        ((cj as i64 + r) as usize).min(terrain.ny - 1),
    );
    let step = terrain.dx * 0.5;
    for j in j0..=j1 {
        for i in i0..=i1 {
            let (x, y) = out.center(i, j);
            if (x - site.x).hypot(y - site.y) > site.range {
                continue;
            }
            let target = [x, y, terrain.at(x, y) + target_agl];
            if line_of_sight(terrain, eye, target, step) {
                let k = out.index(i, j);
                out.values[k] = 1.0;
            }
        }
    }
    out
}

/// Total hazard rate [1/s] at every cell for a vehicle travelling
/// `target_agl` metres above ground. `detectability` scales site rates
/// (a small drone is harder to interdict than a truck). Hazard areas only
/// apply to ground vehicles (`target_agl < 10 m`).
pub fn exposure_field(
    terrain: &TerrainGrid,
    risk: &RiskModel,
    target_agl: f64,
    detectability: f64,
) -> Raster {
    let mut field = Raster::new(
        terrain.x0, terrain.y0, terrain.dx, terrain.nx, terrain.ny, 0.0,
    );
    for site in &risk.sites {
        let seen = viewshed(terrain, site, target_agl);
        let rate = (site.rate_per_min / 60.0 * detectability) as f32;
        for (f, v) in field.values.iter_mut().zip(&seen.values) {
            *f += rate * v;
        }
    }
    if target_agl < 10.0 {
        for h in &risk.hazards {
            let rate = (h.rate_per_min / 60.0) as f32;
            for j in 0..field.ny {
                for i in 0..field.nx {
                    let (x, y) = field.center(i, j);
                    if point_in_polygon([x, y], &h.polygon) {
                        let k = field.index(i, j);
                        field.values[k] += rate;
                    }
                }
            }
        }
    }
    field
}

/// Expected interdictions `∫ λ dt` for crossing segment `a -> b` in
/// `duration` seconds at constant speed.
pub fn segment_exposure(field: &Raster, a: [f64; 2], b: [f64; 2], duration: f64) -> f64 {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = (len / (field.cell * 0.5)).ceil().max(1.0) as usize;
    let dt = duration / n as f64;
    (0..n)
        .map(|k| {
            let t = (k as f64 + 0.5) / n as f64;
            field.sample(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t) * dt
        })
        .sum()
}

/// Probability of getting through with total exposure `e`.
pub fn survival(e: f64) -> f64 {
    (-e).exp()
}

/// Douglas-Peucker polyline simplification with tolerance `eps` [m].
pub fn simplify(points: &[[f64; 2]], eps: f64) -> Vec<[f64; 2]> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (a, b) = (points[0], points[points.len() - 1]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    let dist = |p: &[f64; 2]| {
        if len == 0.0 {
            (p[0] - a[0]).hypot(p[1] - a[1])
        } else {
            ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len
        }
    };
    let (idx, dmax) = points[1..points.len() - 1]
        .iter()
        .enumerate()
        .map(|(i, p)| (i + 1, dist(p)))
        .fold((0, 0.0), |acc, x| if x.1 > acc.1 { x } else { acc });
    if dmax <= eps {
        return vec![a, b];
    }
    let mut left = simplify(&points[..=idx], eps);
    let right = simplify(&points[idx..], eps);
    left.pop();
    left.extend(right);
    left
}

/// Total length of a polyline [m].
pub fn polyline_length(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

/// Split a polyline so no piece is longer than `step`.
pub fn resample(points: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    for w in points.windows(2) {
        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        let n = (len / step).ceil().max(1.0) as usize;
        for k in 1..=n {
            let t = k as f64 / n as f64;
            out.push([
                w[0][0] + (w[1][0] - w[0][0]) * t,
                w[0][1] + (w[1][1] - w[0][1]) * t,
            ]);
        }
    }
    out
}
