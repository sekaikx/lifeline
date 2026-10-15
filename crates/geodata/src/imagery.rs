//! True-colour Sentinel-2 imagery for the map (contains modified Copernicus
//! Sentinel data, processed by Microsoft Planetary Computer).
//!
//! The newest low-cloud scenes from the last two months are stacked: the
//! clearest goes on top and later ones only fill pixels it does not cover,
//! because one scene rarely covers a whole area.

use crate::dem::decode_png;
use crate::{pc, Area, Fetcher};
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub date: String,
    pub cloud: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Imagery {
    /// PNG, north up, covering `bounds`
    pub png_base64: String,
    pub width: usize,
    pub height: usize,
    /// (west, south, east, north) in local metres
    pub bounds: [f64; 4],
    pub scenes: Vec<Scene>,
    pub attribution: String,
}

pub fn imagery(fetcher: &Fetcher, area: &Area, max_px: usize) -> Result<Imagery, String> {
    area.validate()?;
    let size = ((area.size_m / 10.0) as usize).clamp(256, max_px.max(256));
    let mut items = pc::search(
        fetcher,
        area,
        "sentinel-2-l2a",
        Some(pc::last_days(60)),
        Some(25.0),
        12,
    )?;
    if items.is_empty() {
        items = pc::search(
            fetcher,
            area,
            "sentinel-2-l2a",
            Some(pc::last_days(365)),
            Some(60.0),
            12,
        )?;
    }
    if items.is_empty() {
        return Err("no Sentinel-2 scene found for this area in the last year".into());
    }
    items.sort_by(|a, b| a.cloud.total_cmp(&b.cloud));

    let mut rgba = vec![0u8; size * size * 4];
    let mut used = Vec::new();
    for item in items.iter().take(6) {
        let png = pc::crop_png(
            fetcher,
            area,
            "sentinel-2-l2a",
            &item.id,
            size,
            "assets=visual&asset_bidx=visual%7C1%2C2%2C3&nodata=0",
        )?;
        let (w, h, c, px) = decode_png(&png)?;
        if w != size || h != size || c != 4 {
            return Err(format!("unexpected imagery format {w}x{h}x{c}"));
        }
        let mut filled = false;
        for (dst, src) in rgba
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(px.as_chunks::<4>().0)
        {
            if dst[3] == 0 && src[3] > 0 {
                dst.copy_from_slice(src);
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
        if rgba.as_chunks::<4>().0.iter().all(|p| p[3] > 0) {
            break;
        }
    }

    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, size as u32, size as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    let h = area.size_m / 2.0;
    let years: std::collections::BTreeSet<&str> = used.iter().map(|s| &s.date[..4]).collect();
    Ok(Imagery {
        png_base64: base64::engine::general_purpose::STANDARD.encode(out),
        width: size,
        height: size,
        bounds: [-h, -h, h, h],
        attribution: format!(
            "Contains modified Copernicus Sentinel data {}, processed by Microsoft Planetary Computer",
            years.into_iter().collect::<Vec<_>>().join(", ")
        ),
        scenes: used,
    })
}
