//! A learned water detector: a small U-Net exported to ONNX by `ml/`.
//!
//! The model reads Sentinel-2 surface reflectance (0..1) for bands B02, B03,
//! B04, B08, B11 and B12 and returns a per-pixel water probability; the
//! preprocessing is part of the exported graph. Lifeline does not bundle
//! trained weights (see `ml/README.md` on the training data's terms), so the
//! model is loaded from a file the operator provides.

use rayon::prelude::*;
use std::path::{Path, PathBuf};
use tract_onnx::prelude::*;

/// Bands in the order the model expects.
pub const MODEL_BANDS: [&str; 6] = ["B02", "B03", "B04", "B08", "B11", "B12"];
/// Environment variable naming the model file.
pub const MODEL_ENV: &str = "LIFELINE_WATER_MODEL";

/// Output pixels per tile side; each tile also sees `HALO` pixels of context
/// on every side so predictions do not change at tile seams.
const TILE: usize = 256;
const HALO: usize = 32;
const SIDE: usize = TILE + 2 * HALO;

type Plan = std::sync::Arc<TypedSimplePlan>;

pub struct WaterModel {
    plan: Plan,
    pub path: PathBuf,
}

impl std::fmt::Debug for WaterModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaterModel")
            .field("path", &self.path)
            .finish()
    }
}

impl WaterModel {
    pub fn load(path: &Path) -> Result<Self, String> {
        let err = |e: TractError| format!("water model {}: {e}", path.display());
        let plan = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(err)?
            .with_input_fact(0, f32::fact([1, MODEL_BANDS.len(), SIDE, SIDE]).into())
            .map_err(err)?
            .into_optimized()
            .map_err(err)?
            .into_runnable()
            .map_err(err)?;
        Ok(Self {
            plan,
            path: path.to_path_buf(),
        })
    }

    /// Where Lifeline looks for a model when the environment names none,
    /// e.g. `~/.local/share/lifeline/models/water-unet.onnx` on Linux.
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("lifeline").join("models").join("water-unet.onnx"))
    }

    /// The model named by `LIFELINE_WATER_MODEL`, else the default path if a
    /// file is there.
    pub fn locate() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os(MODEL_ENV) {
            return Some(PathBuf::from(p));
        }
        Self::default_path().filter(|p| p.exists())
    }

    /// Water probability for a `rows × cols` scene. `reflectance` holds the
    /// six bands one after another, each row-major.
    pub fn predict(
        &self,
        reflectance: &[f32],
        rows: usize,
        cols: usize,
    ) -> Result<Vec<f32>, String> {
        let plane = rows * cols;
        if reflectance.len() != MODEL_BANDS.len() * plane {
            return Err("water model: band data does not match the scene size".into());
        }
        let tiles: Vec<(usize, usize)> = (0..rows)
            .step_by(TILE)
            .flat_map(|r| (0..cols).step_by(TILE).map(move |c| (r, c)))
            .collect();
        let outputs = tiles
            .par_iter()
            .map(|&(r0, c0)| self.tile(reflectance, rows, cols, r0, c0))
            .collect::<Result<Vec<_>, String>>()?;
        let mut prob = vec![0.0f32; plane];
        for (&(r0, c0), out) in tiles.iter().zip(outputs) {
            for r in r0..(r0 + TILE).min(rows) {
                for c in c0..(c0 + TILE).min(cols) {
                    prob[r * cols + c] = out[(r - r0 + HALO) * SIDE + (c - c0 + HALO)];
                }
            }
        }
        Ok(prob)
    }

    fn tile(
        &self,
        refl: &[f32],
        rows: usize,
        cols: usize,
        r0: usize,
        c0: usize,
    ) -> Result<Vec<f32>, String> {
        // replicate edge pixels where the tile runs past the scene
        let at = |v: isize, n: usize| v.clamp(0, n as isize - 1) as usize;
        let plane = rows * cols;
        let mut input = vec![0.0f32; MODEL_BANDS.len() * SIDE * SIDE];
        for b in 0..MODEL_BANDS.len() {
            for i in 0..SIDE {
                let r = at(r0 as isize + i as isize - HALO as isize, rows);
                for j in 0..SIDE {
                    let c = at(c0 as isize + j as isize - HALO as isize, cols);
                    input[(b * SIDE + i) * SIDE + j] = refl[b * plane + r * cols + c];
                }
            }
        }
        let tensor = Tensor::from_shape(&[1, MODEL_BANDS.len(), SIDE, SIDE], &input)
            .map_err(|e| e.to_string())?;
        let out = self
            .plan
            .run(tvec!(tensor.into()))
            .map_err(|e| format!("water model: {e}"))?;
        let view = out[0]
            .to_plain_array_view::<f32>()
            .map_err(|e| e.to_string())?;
        // anything else (half resolution, two classes) would be misread
        if view.len() != SIDE * SIDE {
            return Err(format!(
                "water model: output shape {:?}, expected one probability per input pixel",
                view.shape()
            ));
        }
        Ok(view.iter().copied().collect())
    }
}
