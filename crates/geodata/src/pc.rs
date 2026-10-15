//! Microsoft Planetary Computer: STAC search and the data API, which crops
//! and resamples a scene to a bounding box and returns PNG or raw arrays.

use crate::{iso_date, unix_now, Area, Fetcher};

/// The `days` before now, as a search window.
pub(crate) fn last_days(days: i64) -> (i64, i64) {
    let now = unix_now();
    (now - days * 86_400, now)
}
use serde::Deserialize;
use std::time::Duration;

const STAC: &str = "https://planetarycomputer.microsoft.com/api/stac/v1/search";
const DATA: &str = "https://planetarycomputer.microsoft.com/api/data/v1/item/bbox";

#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub id: String,
    pub datetime: String,
    pub cloud: f64,
    /// Sentinel-2 processing baseline, e.g. 5.1 (0 when not reported)
    pub baseline: f64,
}

#[derive(Deserialize)]
struct Search {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    id: String,
    properties: serde_json::Value,
}

/// Items of `collection` intersecting `area`, newest first. `window` is a
/// (start, end) range in Unix seconds; `max_cloud` filters on `eo:cloud_cover`.
pub(crate) fn search(
    fetcher: &Fetcher,
    area: &Area,
    collection: &str,
    window: Option<(i64, i64)>,
    max_cloud: Option<f64>,
    limit: usize,
) -> Result<Vec<Item>, String> {
    let [s, w, n, e] = area.bbox();
    let mut body = serde_json::json!({
        "collections": [collection],
        "bbox": [w, s, e, n],
        "limit": limit,
        "sortby": [{"field": "datetime", "direction": "desc"}],
    });
    if let Some((from, to)) = window {
        body["datetime"] =
            format!("{}T00:00:00Z/{}T23:59:59Z", iso_date(from), iso_date(to)).into();
    }
    if let Some(c) = max_cloud {
        body["query"] = serde_json::json!({"eo:cloud_cover": {"lt": c}});
    }
    let bytes = fetcher.post(
        STAC,
        "application/json",
        &body.to_string(),
        Some(Duration::from_secs(6 * 3_600)),
    )?;
    let r: Search =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid STAC response: {e}"))?;
    Ok(r.features
        .into_iter()
        .map(|f| Item {
            datetime: f.properties["datetime"]
                .as_str()
                .or(f.properties["start_datetime"].as_str())
                .unwrap_or("")
                .to_string(),
            cloud: f.properties["eo:cloud_cover"].as_f64().unwrap_or(0.0),
            baseline: f.properties["s2:processing_baseline"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            id: f.id,
        })
        .collect())
}

fn crop_url(
    area: &Area,
    collection: &str,
    item: &str,
    w: usize,
    h: usize,
    ext: &str,
    query: &str,
) -> String {
    let [s, west, n, e] = area.bbox();
    format!("{DATA}/{west:.6},{s:.6},{e:.6},{n:.6}/{w}x{h}.{ext}?collection={collection}&item={item}&{query}")
}

/// RGBA PNG of `item` cropped to `area`; pixels outside the scene are transparent.
pub(crate) fn crop_png(
    fetcher: &Fetcher,
    area: &Area,
    collection: &str,
    item: &str,
    size: usize,
    query: &str,
) -> Result<Vec<u8>, String> {
    fetcher.get(
        &crop_url(area, collection, item, size, size, "png", query),
        Some(Duration::from_secs(30 * 86_400)),
    )
}

/// Raw band values: `shape = [bands + 1, rows, cols]`, the last band being
/// the data mask (0 outside the scene). Rows run north to south.
pub(crate) struct Array {
    pub shape: Vec<usize>,
    pub data: Vec<f64>,
}

impl Array {
    pub fn at(&self, band: usize, row: usize, col: usize) -> f64 {
        self.data[(band * self.shape[1] + row) * self.shape[2] + col]
    }
}

pub(crate) fn crop_array(
    fetcher: &Fetcher,
    area: &Area,
    collection: &str,
    item: &str,
    size: usize,
    query: &str,
) -> Result<Array, String> {
    let bytes = fetcher.get(
        &crop_url(area, collection, item, size, size, "npy", query),
        Some(Duration::from_secs(30 * 86_400)),
    )?;
    parse_npy(&bytes)
}

/// Minimal `.npy` reader for little-endian integer and float arrays.
pub(crate) fn parse_npy(b: &[u8]) -> Result<Array, String> {
    if b.len() < 10 || &b[..6] != b"\x93NUMPY" {
        return Err("not a .npy array".into());
    }
    let (header_len, start) = match b[6] {
        1 => (u16::from_le_bytes([b[8], b[9]]) as usize, 10),
        _ => (u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize, 12),
    };
    let header = std::str::from_utf8(&b[start..start + header_len]).map_err(|e| e.to_string())?;
    let field = |key: &str| -> Option<&str> {
        let i = header.find(key)? + key.len();
        Some(header[i..].trim_start_matches([' ', ':']))
    };
    let descr = field("'descr'")
        .and_then(|v| v.split('\'').nth(1))
        .ok_or("npy: no dtype")?;
    if field("'fortran_order'").is_some_and(|v| v.starts_with("True")) {
        return Err("npy: Fortran order not supported".into());
    }
    let shape_txt = field("'shape'")
        .and_then(|v| v.split(['(', ')']).nth(1))
        .ok_or("npy: no shape")?;
    let shape: Vec<usize> = shape_txt
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let n: usize = shape.iter().product();
    let raw = &b[start + header_len..];
    let data: Vec<f64> = match descr {
        "|u1" | "<u1" => raw.iter().take(n).map(|v| f64::from(*v)).collect(),
        "<u2" => raw
            .as_chunks::<2>()
            .0
            .iter()
            .take(n)
            .map(|c| f64::from(u16::from_le_bytes(*c)))
            .collect(),
        "<i2" => raw
            .as_chunks::<2>()
            .0
            .iter()
            .take(n)
            .map(|c| f64::from(i16::from_le_bytes(*c)))
            .collect(),
        "<f4" => raw
            .as_chunks::<4>()
            .0
            .iter()
            .take(n)
            .map(|c| f64::from(f32::from_le_bytes(*c)))
            .collect(),
        "<f8" => raw
            .as_chunks::<8>()
            .0
            .iter()
            .take(n)
            .map(|c| f64::from_le_bytes(*c))
            .collect(),
        other => return Err(format!("npy: unsupported dtype {other}")),
    };
    if data.len() != n || shape.len() != 3 {
        return Err("npy: truncated or unexpected shape".into());
    }
    Ok(Array { shape, data })
}
