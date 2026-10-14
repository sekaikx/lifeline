//! New surface water (likely flooding) from Sentinel-2.
//!
//! Method, per terrain cell:
//!
//! 1. Usable: only pixels Sentinel-2's scene classification calls
//!    vegetation, bare soil or water (SCL 4, 5, 6), at least two cells away
//!    from any cloud, cloud shadow or cirrus (shadows and cloud edges read
//!    as water otherwise), and not snow or ice in WorldCover (class 70).
//! 2. Water now: MNDWI = (green − SWIR) / (green + SWIR) above 0.05, using
//!    bands B03 and B11 (Xu 2006), or SCL water.
//! 3. Normally water: ESA WorldCover 2021 class 80 (permanent water bodies),
//!    sampled at three times the grid resolution so narrow rivers are not
//!    lost, then widened by one cell for river banks.
//! 4. New water = water now and not normally water. Specks smaller than
//!    four cells are dropped as noise.
//!
//! Optical imagery cannot see through cloud; the result reports how much of
//! the area was obscured, and a flood under cloud is not detected. A radar
//! (Sentinel-1) detector is the remedy and fits the same interface.

use crate::pc::{self, Array};
use crate::{Area, Fetcher, Scene};
use lifeline_core::risk::CellMask;
use lifeline_core::terrain::TerrainGrid;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaterDetection {
    /// New water on the terrain grid
    pub mask: CellMask,
    pub scenes: Vec<Scene>,
    /// Share of the area that could not be assessed (cloud, snow, gaps)
    pub obscured_fraction: f64,
    /// Share of the area that is water now
    pub water_fraction: f64,
    /// Share of the area that is new water
    pub new_water_fraction: f64,
    pub method: String,
}

/// Scene classes trusted for a water decision: vegetation, bare soil, water.
const USABLE_SCL: [u8; 3] = [4, 5, 6];
/// Cloud shadow, medium and high probability cloud, thin cirrus.
const CLOUDY_SCL: [u8; 4] = [3, 8, 9, 10];
/// Cells around cloud treated as obscured.
const CLOUD_BUFFER: usize = 2;
const MNDWI_THRESHOLD: f64 = 0.05;

/// Per-cell classification inputs, rows south to north like the terrain.
pub struct Observation {
    pub n: usize,
    /// `Some(mndwi)` where usable
    pub mndwi: Vec<Option<f64>>,
    pub scl_water: Vec<bool>,
    pub permanent: Vec<bool>,
}

/// Steps 1-4 on already resampled inputs (kept separate for testing).
pub fn classify(obs: &Observation, threshold: f64) -> (Vec<bool>, f64, f64) {
    let total = obs.n * obs.n;
    let mut new = vec![false; total];
    let (mut obscured, mut water) = (0usize, 0usize);
    for (k, flag) in new.iter_mut().enumerate() {
        match obs.mndwi[k] {
            None => obscured += 1,
            Some(v) => {
                let wet = v > threshold || obs.scl_water[k];
                if wet {
                    water += 1;
                }
                *flag = wet && !obs.permanent[k];
            }
        }
    }
    remove_specks(&mut new, obs.n, 4);
    (
        new,
        obscured as f64 / total as f64,
        water as f64 / total as f64,
    )
}

/// Grow a mask by `r` cells (square neighbourhood).
fn dilate(mask: &[bool], n: usize, r: usize) -> Vec<bool> {
    let mut out = mask.to_vec();
    for k in (0..mask.len()).filter(|&k| mask[k]) {
        let (i, j) = (k % n, k / n);
        for jj in j.saturating_sub(r)..=(j + r).min(n - 1) {
            for ii in i.saturating_sub(r)..=(i + r).min(n - 1) {
                out[jj * n + ii] = true;
            }
        }
    }
    out
}

/// Drop 4-connected components smaller than `min` cells.
fn remove_specks(mask: &mut [bool], n: usize, min: usize) {
    let mut seen = vec![false; mask.len()];
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        let mut comp = Vec::new();
        seen[start] = true;
        while let Some(k) = stack.pop() {
            comp.push(k);
            let (i, j) = (k % n, k / n);
            let mut visit = |ii: usize, jj: usize| {
                let q = jj * n + ii;
                if mask[q] && !seen[q] {
                    seen[q] = true;
                    stack.push(q);
                }
            };
            if i > 0 {
                visit(i - 1, j);
            }
            if i + 1 < n {
                visit(i + 1, j);
            }
            if j > 0 {
                visit(i, j - 1);
            }
            if j + 1 < n {
                visit(i, j + 1);
            }
        }
        if comp.len() < min {
            for k in comp {
                mask[k] = false;
            }
        }
    }
}

/// Detect new water over `area` on the grid of `terrain`, from the newest
/// clear observations in the 30 days up to `as_of` (Unix seconds; `None`
/// for now). A past date reviews a historical event.
pub fn detect_new_water(
    fetcher: &Fetcher,
    area: &Area,
    terrain: &TerrainGrid,
    as_of: Option<i64>,
) -> Result<WaterDetection, String> {
    area.validate()?;
    let n = terrain.nx;
    if terrain.ny != n {
        return Err("water detection needs a square terrain grid".into());
    }
    let end = as_of.unwrap_or_else(crate::unix_now);
    let mut items = pc::search(
        fetcher,
        area,
        "sentinel-2-l2a",
        Some((end - 30 * 86_400, end)),
        Some(60.0),
        10,
    )?;
    if items.is_empty() {
        return Err(format!(
            "no Sentinel-2 scene in the 30 days up to {}",
            crate::iso_date(end)
        ));
    }
    // newest first, but skip very cloudy passes when a clearer one exists
    items.sort_by(|a, b| b.datetime.cmp(&a.datetime));

    // assemble the newest usable observation for each cell
    let total = n * n;
    let mut mndwi: Vec<Option<f64>> = vec![None; total];
    let mut scl_water = vec![false; total];
    let mut used = Vec::new();
    let flip = |row: usize| n - 1 - row; // arrays run north to south
    for item in items.iter().take(4) {
        let a: Array = pc::crop_array(
            fetcher,
            area,
            "sentinel-2-l2a",
            &item.id,
            n,
            "assets=B03&assets=B11&assets=SCL&nodata=0",
        )?;
        if a.shape != [4, n, n] {
            return Err(format!("unexpected band array shape {:?}", a.shape));
        }
        let mut cloud = vec![false; total];
        for row in 0..n {
            for col in 0..n {
                cloud[flip(row) * n + col] = CLOUDY_SCL.contains(&(a.at(2, row, col) as u8));
            }
        }
        let cloud = dilate(&cloud, n, CLOUD_BUFFER);
        let mut filled = false;
        for row in 0..n {
            for col in 0..n {
                let k = flip(row) * n + col;
                if mndwi[k].is_some() || cloud[k] {
                    continue;
                }
                let (green, swir, scl, valid) = (
                    a.at(0, row, col),
                    a.at(1, row, col),
                    a.at(2, row, col) as u8,
                    a.at(3, row, col),
                );
                if valid == 0.0 || !USABLE_SCL.contains(&scl) || green + swir <= 0.0 {
                    continue;
                }
                mndwi[k] = Some((green - swir) / (green + swir));
                scl_water[k] = scl == 6;
                filled = true;
            }
        }
        if filled {
            used.push(Scene {
                id: item.id.clone(),
                date: item.datetime.chars().take(10).collect(),
                cloud: item.cloud,
            });
        }
        if mndwi.iter().all(Option::is_some) {
            break;
        }
    }

    // permanent water and snow from WorldCover, supersampled 3x per cell
    let mut permanent = vec![false; total];
    let wc = pc::search(fetcher, area, "esa-worldcover", None, None, 4)?;
    let newest = wc
        .iter()
        .map(|i| i.datetime.clone())
        .max()
        .unwrap_or_default();
    let fine = 3 * n;
    for item in wc.iter().filter(|i| i.datetime == newest) {
        let a = pc::crop_array(
            fetcher,
            area,
            "esa-worldcover",
            &item.id,
            fine,
            "assets=map&nodata=0",
        )?;
        if a.shape != [2, fine, fine] {
            return Err(format!("unexpected WorldCover array shape {:?}", a.shape));
        }
        for row in 0..fine {
            for col in 0..fine {
                let k = flip(row / 3) * n + col / 3;
                match a.at(0, row, col) as u8 {
                    80 => permanent[k] = true,
                    70 => mndwi[k] = None,
                    _ => {}
                }
            }
        }
    }
    let permanent = dilate(&permanent, n, 1);

    let obs = Observation {
        n,
        mndwi,
        scl_water,
        permanent,
    };
    let (new, obscured, water) = classify(&obs, MNDWI_THRESHOLD);
    let cells: Vec<u32> = new
        .iter()
        .enumerate()
        .filter(|(_, w)| **w)
        .map(|(k, _)| k as u32)
        .collect();
    let new_fraction = cells.len() as f64 / total as f64;
    Ok(WaterDetection {
        mask: CellMask { x0: terrain.x0, y0: terrain.y0, cell: terrain.dx, nx: n, ny: n, cells },
        scenes: used,
        obscured_fraction: obscured,
        water_fraction: water,
        new_water_fraction: new_fraction,
        method: "Sentinel-2 MNDWI > 0.05 or SCL water on clear pixels, minus ESA WorldCover 2021 permanent water".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dilation_grows_by_radius() {
        let mut m = vec![false; 25];
        m[12] = true;
        let d = dilate(&m, 5, 1);
        assert_eq!(d.iter().filter(|x| **x).count(), 9);
        assert!(d[6] && d[18] && !d[0]);
    }

    #[test]
    fn new_water_excludes_permanent_cloud_and_specks() {
        let n = 6;
        let mut mndwi = vec![Some(-0.4); n * n];
        let mut permanent = vec![false; n * n];
        // a 2x3 flooded field, a single wet pixel, a lake, a cloud
        for k in [7, 8, 9, 13, 14, 15] {
            mndwi[k] = Some(0.3);
        }
        mndwi[30] = Some(0.5);
        for k in [4, 5, 10, 11] {
            mndwi[k] = Some(0.6);
            permanent[k] = true;
        }
        mndwi[35] = None;
        let obs = Observation {
            n,
            mndwi,
            scl_water: vec![false; n * n],
            permanent,
        };
        let (new, obscured, water) = classify(&obs, MNDWI_THRESHOLD);
        let flagged: Vec<usize> = (0..n * n).filter(|k| new[*k]).collect();
        assert_eq!(flagged, vec![7, 8, 9, 13, 14, 15]);
        assert!((obscured - 1.0 / 36.0).abs() < 1e-12);
        assert!((water - 11.0 / 36.0).abs() < 1e-12);
    }
}
