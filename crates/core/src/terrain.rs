//! Ground elevation models (heights MSL in metres).
//!
//! The impact event is `z - elevation(x, y) = 0`, so any terrain model works
//! with every integrator. `Grid` is the hook for real elevation data (DEM
//! tiles): fill it from a GeoTIFF/SRTM reader and the rest is unchanged.
//! [`TerrainGrid::procedural`] generates reproducible synthetic terrain so
//! scenarios need no external data.

use crate::stats::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Terrain {
    Flat {
        elevation: f64,
    },
    /// A smooth Gaussian hill, useful to demonstrate terrain-aware drops.
    Hill {
        base: f64,
        height: f64,
        center_x: f64,
        center_y: f64,
        radius: f64,
    },
    /// Regular grid, bilinearly interpolated, clamped at the edges.
    Grid(TerrainGrid),
}

impl Default for Terrain {
    fn default() -> Self {
        Terrain::Flat { elevation: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainGrid {
    pub x0: f64,
    pub y0: f64,
    pub dx: f64,
    pub dy: f64,
    pub nx: usize,
    pub ny: usize,
    /// Row-major heights, `heights[j * nx + i]` at `(x0 + i dx, y0 + j dy)`.
    pub heights: Vec<f64>,
}

impl TerrainGrid {
    pub fn validate(&self) -> Result<(), String> {
        if self.nx < 2 || self.ny < 2 {
            return Err("terrain grid needs at least 2x2 points".into());
        }
        if self.heights.len() != self.nx * self.ny {
            return Err("terrain grid heights length must be nx * ny".into());
        }
        if self.dx <= 0.0 || self.dy <= 0.0 {
            return Err("terrain grid spacing must be positive".into());
        }
        Ok(())
    }

    /// Bilinear height at `(x, y)`, clamped to the grid.
    pub fn at(&self, x: f64, y: f64) -> f64 {
        let fx = ((x - self.x0) / self.dx).clamp(0.0, (self.nx - 1) as f64);
        let fy = ((y - self.y0) / self.dy).clamp(0.0, (self.ny - 1) as f64);
        let i = (fx.floor() as usize).min(self.nx - 2);
        let j = (fy.floor() as usize).min(self.ny - 2);
        let (tx, ty) = (fx - i as f64, fy - j as f64);
        let h = |i: usize, j: usize| self.heights[j * self.nx + i];
        let bottom = h(i, j) * (1.0 - tx) + h(i + 1, j) * tx;
        let top = h(i, j + 1) * (1.0 - tx) + h(i + 1, j + 1) * tx;
        bottom * (1.0 - ty) + top * ty
    }
}

impl Terrain {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Terrain::Grid(g) => g.validate(),
            Terrain::Hill { radius, .. } if *radius <= 0.0 => {
                Err("hill radius must be positive".into())
            }
            _ => Ok(()),
        }
    }

    pub fn elevation(&self, x: f64, y: f64) -> f64 {
        match self {
            Terrain::Flat { elevation } => *elevation,
            Terrain::Hill {
                base,
                height,
                center_x,
                center_y,
                radius,
            } => {
                let r2 = (x - center_x).powi(2) + (y - center_y).powi(2);
                base + height * (-r2 / (2.0 * radius * radius)).exp()
            }
            Terrain::Grid(g) => g.at(x, y),
        }
    }
}

/// Hash a lattice point to a uniform value in `[0, 1)`.
fn lattice(seed: u64, octave: u32, ix: i64, iy: i64) -> f64 {
    let key = seed
        ^ (octave as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (ix as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (iy as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    Rng::new(key).uniform()
}

/// Smooth value noise in `[0, 1)` with lattice spacing `cell`.
fn value_noise(seed: u64, octave: u32, x: f64, y: f64, cell: f64) -> f64 {
    let (fx, fy) = (x / cell, y / cell);
    let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - ix as f64, fy - iy as f64);
    // quintic fade gives continuous slopes, so roads and shading look natural
    let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (sx, sy) = (fade(tx), fade(ty));
    let v = |i: i64, j: i64| lattice(seed, octave, i, j);
    let bottom = v(ix, iy) * (1.0 - sx) + v(ix + 1, iy) * sx;
    let top = v(ix, iy + 1) * (1.0 - sx) + v(ix + 1, iy + 1) * sx;
    bottom * (1.0 - sy) + top * sy
}

impl TerrainGrid {
    /// Synthetic terrain: fractal value noise (five octaves) centred on the
    /// origin, `size` metres across with `cells` intervals per side. Heights
    /// run from `base` to roughly `base + relief`. Same seed, same terrain.
    pub fn procedural(seed: u64, size: f64, cells: usize, base: f64, relief: f64) -> Self {
        let n = cells.max(1) + 1;
        let dx = size / cells.max(1) as f64;
        let x0 = -size / 2.0;
        let mut heights = Vec::with_capacity(n * n);
        let mut raw = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (x0 + i as f64 * dx, x0 + j as f64 * dx);
                let mut h = 0.0;
                let mut amp = 1.0;
                let mut cell = size / 2.5;
                for octave in 0..5 {
                    h += amp * value_noise(seed, octave, x, y, cell);
                    amp *= 0.5;
                    cell *= 0.5;
                }
                raw.push(h);
            }
        }
        let lo = raw.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = raw.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        for h in raw {
            // squaring flattens the lowlands into valleys and sharpens ridges
            let t = (h - lo) / (hi - lo).max(1e-12);
            heights.push(base + relief * t * t);
        }
        TerrainGrid {
            x0,
            y0: x0,
            dx,
            dy: dx,
            nx: n,
            ny: n,
            heights,
        }
    }

    /// Steepest slope (rise over run) at `(x, y)` by central differences.
    pub fn grade(&self, x: f64, y: f64) -> f64 {
        let h = self.dx.min(self.dy);
        let gx = (self.at(x + h, y) - self.at(x - h, y)) / (2.0 * h);
        let gy = (self.at(x, y + h) - self.at(x, y - h)) / (2.0 * h);
        gx.hypot(gy)
    }

    pub fn bounds(&self) -> [f64; 4] {
        [
            self.x0,
            self.y0,
            self.x0 + (self.nx - 1) as f64 * self.dx,
            self.y0 + (self.ny - 1) as f64 * self.dy,
        ]
    }
}
