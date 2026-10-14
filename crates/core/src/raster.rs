//! Regular grids of values over the map (exposure rates, visibility masks).

use serde::{Deserialize, Serialize};

/// Cell-centred raster: value `values[j * nx + i]` belongs to the square of
/// side `cell` centred on `(x0 + i * cell, y0 + j * cell)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Raster {
    pub x0: f64,
    pub y0: f64,
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    pub values: Vec<f32>,
}

impl Raster {
    pub fn new(x0: f64, y0: f64, cell: f64, nx: usize, ny: usize, fill: f32) -> Self {
        Self {
            x0,
            y0,
            cell,
            nx,
            ny,
            values: vec![fill; nx * ny],
        }
    }

    pub fn index(&self, i: usize, j: usize) -> usize {
        j * self.nx + i
    }

    pub fn center(&self, i: usize, j: usize) -> (f64, f64) {
        (
            self.x0 + i as f64 * self.cell,
            self.y0 + j as f64 * self.cell,
        )
    }

    /// Cell containing `(x, y)`, or `None` outside the raster.
    pub fn cell_of(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let fi = ((x - self.x0) / self.cell).round();
        let fj = ((y - self.y0) / self.cell).round();
        if fi < 0.0 || fj < 0.0 || fi >= self.nx as f64 || fj >= self.ny as f64 {
            return None;
        }
        Some((fi as usize, fj as usize))
    }

    /// Nearest-cell value; zero outside the raster.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        self.cell_of(x, y)
            .map_or(0.0, |(i, j)| self.values[self.index(i, j)] as f64)
    }

    pub fn max(&self) -> f32 {
        self.values.iter().cloned().fold(0.0, f32::max)
    }
}
