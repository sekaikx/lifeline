//! Elevation from AWS Terrain Tiles (Terrarium encoding).
//!
//! Tiles are 256 px Web Mercator PNGs; each pixel stores height as
//! `R·256 + G + B/256 − 32768` metres. Sources are SRTM and others; see
//! <https://github.com/tilezen/joerd/blob/master/docs/attribution.md>.

use crate::{Area, Fetcher};
use lifeline_core::terrain::TerrainGrid;
use std::collections::HashMap;

const TILE: f64 = 256.0;

pub(crate) fn mercator_px(lat: f64, lon: f64, z: u32) -> (f64, f64) {
    let n = TILE * 2f64.powi(z as i32);
    let x = (lon + 180.0) / 360.0 * n;
    let phi = lat.to_radians();
    let y = (1.0 - (phi.tan() + 1.0 / phi.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// Ground resolution of a zoom level at a latitude [m/px].
fn resolution(z: u32, lat: f64) -> f64 {
    156_543.034 * lat.to_radians().cos() / 2f64.powi(z as i32)
}

pub(crate) fn decode_png(bytes: &[u8]) -> Result<(usize, usize, usize, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let channels = info.color_type.samples();
    buf.truncate(info.buffer_size());
    Ok((info.width as usize, info.height as usize, channels, buf))
}

struct Tiles<'a> {
    fetcher: &'a Fetcher,
    z: u32,
    cache: HashMap<(i64, i64), Vec<f32>>,
}

impl Tiles<'_> {
    fn tile(&mut self, tx: i64, ty: i64) -> Result<&Vec<f32>, String> {
        if !self.cache.contains_key(&(tx, ty)) {
            let n = 1i64 << self.z;
            let url = format!(
                "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{}/{}/{}.png",
                self.z,
                tx.rem_euclid(n),
                ty.clamp(0, n - 1)
            );
            let bytes = self.fetcher.get(&url, None)?;
            let (w, h, c, px) = decode_png(&bytes)?;
            if w != 256 || h != 256 || c < 3 {
                return Err(format!("unexpected terrain tile format {w}x{h}x{c}"));
            }
            let heights = px
                .chunks(c)
                .map(|p| {
                    (f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0) - 32_768.0
                })
                .collect();
            self.cache.insert((tx, ty), heights);
        }
        Ok(&self.cache[&(tx, ty)])
    }

    fn pixel(&mut self, px: i64, py: i64) -> Result<f64, String> {
        let (tx, ty) = (px.div_euclid(256), py.div_euclid(256));
        let t = self.tile(tx, ty)?;
        Ok(f64::from(
            t[(py.rem_euclid(256) * 256 + px.rem_euclid(256)) as usize],
        ))
    }

    fn bilinear(&mut self, x: f64, y: f64) -> Result<f64, String> {
        // pixel centres sit at half-integer coordinates
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
        let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
        let a = self.pixel(x0, y0)?;
        let b = self.pixel(x0 + 1, y0)?;
        let c = self.pixel(x0, y0 + 1)?;
        let d = self.pixel(x0 + 1, y0 + 1)?;
        Ok((a * (1.0 - tx) + b * tx) * (1.0 - ty) + (c * (1.0 - tx) + d * tx) * ty)
    }
}

/// Elevation grid over `area`, centred on its origin like the generated
/// terrain. Sea is clamped to 0 m (tiles carry bathymetry).
pub fn terrain(fetcher: &Fetcher, area: &Area) -> Result<TerrainGrid, String> {
    area.validate()?;
    let z = (8..=14)
        .find(|&z| resolution(z, area.lat) <= area.cell_m * 0.75)
        .unwrap_or(14);
    let frame = area.frame();
    let cells = area.cells();
    let n = cells + 1;
    let dx = area.size_m / cells as f64;
    let x0 = -area.size_m / 2.0;
    let mut tiles = Tiles {
        fetcher,
        z,
        cache: HashMap::new(),
    };
    let mut heights = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let (lat, lon) = frame.to_geo(x0 + i as f64 * dx, x0 + j as f64 * dx);
            let (px, py) = mercator_px(lat, lon, z);
            heights.push(tiles.bilinear(px, py)?.max(0.0));
        }
    }
    Ok(TerrainGrid {
        x0,
        y0: x0,
        dx,
        dy: dx,
        nx: n,
        ny: n,
        heights,
    })
}
