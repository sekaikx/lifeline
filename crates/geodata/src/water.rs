//! New surface water (likely flooding) from Sentinel-2.
//!
//! Method, per terrain cell:
//!
//! 1. Usable: only pixels Sentinel-2's scene classification calls
//!    vegetation, bare soil or water (SCL 4, 5, 6), at least two cells away
//!    from any cloud, cloud shadow or cirrus (shadows and cloud edges read
//!    as water otherwise), and not snow or ice in WorldCover (class 70).
//! 2. Water now, by one of two detectors:
//!    - index (built in): MNDWI = (green − SWIR) / (green + SWIR) above
//!      0.05 on surface reflectance, bands B03 and B11 (Xu 2006), or SCL
//!      water;
//!    - learned (optional): a U-Net trained on hand-labelled flood scenes
//!      (see `ml/`), run at about 20 m and counted wet where most of a
//!      cell's clear pixels are water.
//! 3. Normally water: ESA WorldCover 2021 class 80 (permanent water bodies),
//!    sampled at three times the grid resolution so narrow rivers are not
//!    lost, then widened by one cell for river banks.
//! 4. New water = water now and not normally water. Specks smaller than
//!    four cells are dropped as noise.
//!
//! Optical imagery cannot see through cloud; the result reports how much of
//! the area was obscured, and a flood under cloud is not detected. A radar
//! (Sentinel-1) detector is the remedy and fits the same interface.

use crate::pc::{self, Array, Item};
use crate::water_model::{WaterModel, MODEL_BANDS};
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
    /// `"index"` or `"model"`
    pub detector: String,
    pub method: String,
}

/// Scene classes trusted for a water decision: vegetation, bare soil, water.
const USABLE_SCL: [u8; 3] = [4, 5, 6];
/// Cloud shadow, medium and high probability cloud, thin cirrus.
const CLOUDY_SCL: [u8; 4] = [3, 8, 9, 10];
/// Cells around cloud treated as obscured.
const CLOUD_BUFFER: usize = 2;
const MNDWI_THRESHOLD: f64 = 0.05;
/// Ground resolution the learned detector runs at [m].
const MODEL_RESOLUTION: f64 = 20.0;

/// Surface reflectance from an L2A digital number. Processing baseline
/// 04.00 (January 2022) added 1000 to every value so dark pixels are not
/// clipped; Planetary Computer serves the values unharmonised.
fn reflectance(dn: f64, item: &Item) -> f64 {
    let offset = if item.baseline >= 4.0 { 1000.0 } else { 0.0 };
    ((dn - offset) / 10_000.0).max(0.0)
}

/// Per-cell classification inputs, rows south to north like the terrain.
pub struct Observation {
    pub n: usize,
    /// `Some(score)` where usable; the cell is water when the score is above
    /// the threshold (MNDWI for the index detector)
    pub score: Vec<Option<f64>>,
    pub scl_water: Vec<bool>,
    pub permanent: Vec<bool>,
}

/// Steps 1-4 on already resampled inputs (kept separate for testing).
pub fn classify(obs: &Observation, threshold: f64) -> (Vec<bool>, f64, f64) {
    let total = obs.n * obs.n;
    let mut new = vec![false; total];
    let (mut obscured, mut water) = (0usize, 0usize);
    for (k, flag) in new.iter_mut().enumerate() {
        match obs.score[k] {
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

/// Grow a mask by `r` cells (square neighbourhood), one axis at a time.
fn dilate(mask: &[bool], n: usize, r: usize) -> Vec<bool> {
    let pass = |src: &[bool], horizontal: bool| {
        let mut out = vec![false; src.len()];
        for line in 0..n {
            // count of set cells in the window around each position
            let idx = |p: usize| {
                if horizontal {
                    line * n + p
                } else {
                    p * n + line
                }
            };
            let mut count = 0usize;
            for p in 0..r.min(n) {
                count += usize::from(src[idx(p)]);
            }
            for p in 0..n {
                if p + r < n {
                    count += usize::from(src[idx(p + r)]);
                }
                if p > r {
                    count -= usize::from(src[idx(p - r - 1)]);
                }
                out[idx(p)] = count > 0;
            }
        }
        out
    };
    pass(&pass(mask, true), false)
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
/// for now). A past date reviews a historical event. With `model` the
/// learned detector decides what is water, otherwise the MNDWI index.
pub fn detect_new_water(
    fetcher: &Fetcher,
    area: &Area,
    terrain: &TerrainGrid,
    as_of: Option<i64>,
    model: Option<&WaterModel>,
) -> Result<WaterDetection, String> {
    area.validate()?;
    let n = terrain.nx;
    if terrain.ny != n {
        return Err("water detection needs a square terrain grid".into());
    }
    // Mask cells are centred on the terrain's grid points, so the n x n
    // rasters must cover one cell more than the area (half a cell each side)
    // for pixel centres to fall on the grid points.
    let grid = Area {
        size_m: n as f64 * terrain.dx,
        ..area.clone()
    };
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
    items.sort_by(|a, b| b.datetime.cmp(&a.datetime));

    // assemble the newest usable observation for each cell
    let total = n * n;
    let mut score: Vec<Option<f64>> = vec![None; total];
    let mut scl_water = vec![false; total];
    let mut used = Vec::new();
    for item in items.iter().take(4) {
        let filled = match model {
            None => index_scene(fetcher, &grid, item, n, &mut score, &mut scl_water)?,
            Some(m) => model_scene(fetcher, &grid, item, n, terrain.dx, m, &mut score)?,
        };
        if filled {
            used.push(Scene {
                id: item.id.clone(),
                date: item.datetime.chars().take(10).collect(),
                cloud: item.cloud,
            });
        }
        if score.iter().all(Option::is_some) {
            break;
        }
    }
    let flip = |row: usize| n - 1 - row; // arrays run north to south

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
            &grid,
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
                    70 => score[k] = None,
                    _ => {}
                }
            }
        }
    }
    let permanent = dilate(&permanent, n, 1);

    let obs = Observation {
        n,
        score,
        scl_water,
        permanent,
    };
    // the learned detector scores a cell by its wet share minus one half
    let threshold = if model.is_some() {
        0.0
    } else {
        MNDWI_THRESHOLD
    };
    let (new, obscured, water) = classify(&obs, threshold);
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
        detector: if model.is_some() { "model" } else { "index" }.into(),
        method: match model {
            None => "Sentinel-2 MNDWI > 0.05 or SCL water on clear pixels, minus ESA WorldCover 2021 permanent water".into(),
            Some(m) => format!(
                "U-Net water segmentation ({}) on clear Sentinel-2 pixels at {:.0} m, minus ESA WorldCover 2021 permanent water",
                m.path.file_name().and_then(|f| f.to_str()).unwrap_or("model"),
                terrain.dx / model_factor(terrain.dx) as f64
            ),
        },
    })
}

/// Model pixels per cell side, so the network runs near [`MODEL_RESOLUTION`].
fn model_factor(cell_m: f64) -> usize {
    ((cell_m / MODEL_RESOLUTION).round() as usize).clamp(1, 4)
}

/// Clear-sky mask for one scene: SCL cloud classes widened by `buffer` pixels.
fn cloud_mask(a: &Array, scl_band: usize, size: usize, buffer: usize) -> Vec<bool> {
    let mut cloud = vec![false; size * size];
    for row in 0..size {
        for col in 0..size {
            cloud[row * size + col] = CLOUDY_SCL.contains(&(a.at(scl_band, row, col) as u8));
        }
    }
    dilate(&cloud, size, buffer)
}

/// MNDWI per cell from one scene, filling cells still unassessed. Returns
/// whether the scene contributed.
fn index_scene(
    fetcher: &Fetcher,
    area: &Area,
    item: &Item,
    n: usize,
    score: &mut [Option<f64>],
    scl_water: &mut [bool],
) -> Result<bool, String> {
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
    let cloud = cloud_mask(&a, 2, n, CLOUD_BUFFER);
    let mut filled = false;
    for row in 0..n {
        for col in 0..n {
            let k = (n - 1 - row) * n + col;
            if score[k].is_some() || cloud[row * n + col] {
                continue;
            }
            let scl = a.at(2, row, col) as u8;
            if a.at(3, row, col) == 0.0 || !USABLE_SCL.contains(&scl) {
                continue;
            }
            let green = reflectance(a.at(0, row, col), item);
            let swir = reflectance(a.at(1, row, col), item);
            if green + swir <= 0.0 {
                continue;
            }
            score[k] = Some((green - swir) / (green + swir));
            scl_water[k] = scl == 6;
            filled = true;
        }
    }
    Ok(filled)
}

/// Learned detector on one scene at about [`MODEL_RESOLUTION`]; a cell is
/// assessed when at least half its pixels are clear, and scored by its wet
/// share minus one half.
fn model_scene(
    fetcher: &Fetcher,
    area: &Area,
    item: &Item,
    n: usize,
    cell_m: f64,
    model: &WaterModel,
    score: &mut [Option<f64>],
) -> Result<bool, String> {
    let k = model_factor(cell_m);
    let size = n * k;
    let assets: String = MODEL_BANDS.iter().map(|b| format!("assets={b}&")).collect();
    let a = pc::crop_array(
        fetcher,
        area,
        "sentinel-2-l2a",
        &item.id,
        size,
        &format!("{assets}assets=SCL&nodata=0"),
    )?;
    let bands = MODEL_BANDS.len();
    if a.shape != [bands + 2, size, size] {
        return Err(format!("unexpected band array shape {:?}", a.shape));
    }
    let plane = size * size;
    let mut refl = vec![0.0f32; bands * plane];
    for b in 0..bands {
        for p in 0..plane {
            refl[b * plane + p] = reflectance(a.data[b * plane + p], item) as f32;
        }
    }
    let prob = model.predict(&refl, size, size)?;
    let cloud = cloud_mask(&a, bands, size, CLOUD_BUFFER * k);
    let mut filled = false;
    for row in 0..n {
        for col in 0..n {
            let cell = (n - 1 - row) * n + col;
            if score[cell].is_some() {
                continue;
            }
            let (mut clear, mut wet) = (0usize, 0usize);
            for r in row * k..(row + 1) * k {
                for c in col * k..(col + 1) * k {
                    let p = r * size + c;
                    let scl = a.at(bands, r, c) as u8;
                    if cloud[p] || a.at(bands + 1, r, c) == 0.0 || !USABLE_SCL.contains(&scl) {
                        continue;
                    }
                    clear += 1;
                    wet += usize::from(prob[p] > 0.5);
                }
            }
            if 2 * clear >= k * k && clear > 0 {
                score[cell] = Some(wet as f64 / clear as f64 - 0.5);
                filled = true;
            }
        }
    }
    Ok(filled)
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
        let mut corner = vec![false; 25];
        corner[0] = true;
        let d = dilate(&corner, 5, 2);
        let set: Vec<usize> = (0..25).filter(|k| d[*k]).collect();
        assert_eq!(set, vec![0, 1, 2, 5, 6, 7, 10, 11, 12]);
    }

    #[test]
    fn reflectance_removes_the_2022_offset() {
        let item = |baseline| Item {
            id: String::new(),
            datetime: String::new(),
            cloud: 0.0,
            baseline,
        };
        assert!((reflectance(1500.0, &item(5.1)) - 0.05).abs() < 1e-12);
        assert!((reflectance(1500.0, &item(3.0)) - 0.15).abs() < 1e-12);
        assert_eq!(reflectance(800.0, &item(4.0)), 0.0);
    }

    #[test]
    fn new_water_excludes_permanent_cloud_and_specks() {
        let n = 6;
        let mut score = vec![Some(-0.4); n * n];
        let mut permanent = vec![false; n * n];
        // a 2x3 flooded field, a single wet pixel, a lake, a cloud
        for k in [7, 8, 9, 13, 14, 15] {
            score[k] = Some(0.3);
        }
        score[30] = Some(0.5);
        for k in [4, 5, 10, 11] {
            score[k] = Some(0.6);
            permanent[k] = true;
        }
        score[35] = None;
        let obs = Observation {
            n,
            score,
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
