//! Satellite explorer: any date, any of three missions, and the spectral
//! products an analyst reads an area with.
//!
//! | Sensor | Collection | Resolution | Since | Sees |
//! | --- | --- | --- | --- | --- |
//! | Sentinel-2 | `sentinel-2-l2a` | 10-20 m | 2017 | optical, 6 bands used |
//! | Landsat 4-9 | `landsat-c2-l2` | 30 m | 1982 | optical, 6 bands used |
//! | Sentinel-1 | `sentinel-1-rtc` | 10 m | 2014 | C-band radar, through cloud, day and night |
//!
//! Every product is computed here from surface reflectance (or radar
//! backscatter) on one square grid, so scenes from different sensors and
//! years line up pixel for pixel and can be compared.
//!
//! Optical pixels under cloud, cloud shadow or outside the scene are left
//! for the next scene of a mosaic to fill; a single chosen scene shows them
//! as cloud.

use crate::pc::{self, Item};
use crate::{iso_date, Area, Fetcher};
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensor {
    #[default]
    Sentinel2,
    Landsat,
    Sentinel1,
}

impl Sensor {
    pub fn collection(self) -> &'static str {
        match self {
            Sensor::Sentinel2 => "sentinel-2-l2a",
            Sensor::Landsat => "landsat-c2-l2",
            Sensor::Sentinel1 => "sentinel-1-rtc",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sensor::Sentinel2 => "Sentinel-2",
            Sensor::Landsat => "Landsat",
            Sensor::Sentinel1 => "Sentinel-1 radar",
        }
    }

    pub fn is_radar(self) -> bool {
        self == Sensor::Sentinel1
    }

    fn assets(self) -> &'static str {
        match self {
            Sensor::Sentinel2 => {
                "assets=B02&assets=B03&assets=B04&assets=B08&assets=B11&assets=B12&assets=SCL&nodata=0"
            }
            Sensor::Landsat => {
                "assets=blue&assets=green&assets=red&assets=nir08&assets=swir16&assets=swir22&assets=qa_pixel&nodata=0"
            }
            Sensor::Sentinel1 => "assets=vv&assets=vh&nodata=0",
        }
    }

    /// Bands in the array, the data mask excluded.
    fn bands(self) -> usize {
        if self.is_radar() {
            2
        } else {
            7
        }
    }

    fn attribution(self, years: &str) -> String {
        match self {
            Sensor::Sentinel2 | Sensor::Sentinel1 => format!(
                "Contains modified Copernicus Sentinel data {years}, processed by Microsoft Planetary Computer"
            ),
            Sensor::Landsat => format!(
                "Landsat Collection 2 {years}, courtesy of the U.S. Geological Survey, via Microsoft Planetary Computer"
            ),
        }
    }
}

/// What to draw from a scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Product {
    /// Natural colour (red, green, blue)
    #[default]
    TrueColor,
    /// Near infrared, red, green: healthy vegetation shows bright red
    FalseColor,
    /// SWIR, near infrared, blue: crops, bare soil and wet ground separate
    Agriculture,
    /// NDVI, vegetation vigour
    Vegetation,
    /// NDMI, moisture in vegetation and soil
    Moisture,
    /// MNDWI open water over a muted base
    Water,
    /// NBR, burn scars and severely damaged ground
    Burn,
    /// Radar: VV, VH and their ratio as red, green, blue
    Radar,
}

impl Product {
    pub fn optical() -> [Product; 7] {
        [
            Product::TrueColor,
            Product::FalseColor,
            Product::Agriculture,
            Product::Vegetation,
            Product::Moisture,
            Product::Water,
            Product::Burn,
        ]
    }

    fn fits(self, sensor: Sensor) -> bool {
        match self {
            Product::Water => true,
            Product::Radar => sensor.is_radar(),
            _ => !sensor.is_radar(),
        }
    }
}

/// A scene that can be viewed, from [`list_scenes`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneInfo {
    pub id: String,
    pub sensor: Sensor,
    /// `YYYY-MM-DD`
    pub date: String,
    pub datetime: String,
    /// Cloud cover over the whole scene [%]; `None` for radar
    pub cloud: Option<f64>,
    pub platform: String,
    /// Radar pass direction, empty for optical
    pub orbit: String,
}

/// Scenes of `sensor` over `area` between two Unix times, newest first.
pub fn list_scenes(
    fetcher: &Fetcher,
    area: &Area,
    sensor: Sensor,
    from: i64,
    to: i64,
    max_cloud: Option<f64>,
    limit: usize,
) -> Result<Vec<SceneInfo>, String> {
    area.validate()?;
    if to < from {
        return Err("the end of the period is before its start".into());
    }
    let cloud = if sensor.is_radar() { None } else { max_cloud };
    let items = pc::search(
        fetcher,
        area,
        sensor.collection(),
        Some((from, to)),
        cloud,
        limit.clamp(1, 200),
    )?;
    Ok(items.into_iter().map(|i| scene_info(sensor, &i)).collect())
}

fn scene_info(sensor: Sensor, i: &Item) -> SceneInfo {
    SceneInfo {
        id: i.id.clone(),
        sensor,
        date: i.datetime.chars().take(10).collect(),
        datetime: i.datetime.clone(),
        cloud: (!sensor.is_radar()).then_some(i.cloud),
        platform: i.platform.clone(),
        orbit: i.orbit.clone(),
    }
}

/// Which imagery to use: one scene by id, or a mosaic of the clearest
/// scenes in the `window_days` up to `as_of`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneQuery {
    #[serde(default)]
    pub sensor: Sensor,
    /// A scene id from [`list_scenes`]; `None` for a mosaic
    #[serde(default)]
    pub scene: Option<String>,
    /// End of the mosaic window (Unix seconds); `None` for now
    #[serde(default)]
    pub as_of: Option<i64>,
    #[serde(default = "default_window")]
    pub window_days: u32,
    /// Scenes cloudier than this [%] are not used in a mosaic
    #[serde(default = "default_cloud")]
    pub max_cloud: f64,
}

fn default_window() -> u32 {
    60
}

fn default_cloud() -> f64 {
    40.0
}

impl SceneQuery {
    pub fn latest(sensor: Sensor) -> Self {
        SceneQuery {
            sensor,
            scene: None,
            as_of: None,
            window_days: default_window(),
            max_cloud: default_cloud(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    NoData,
    Clear,
    Cloud,
    Shadow,
    Snow,
}

/// One pixel. Optical bands are surface reflectance: blue, green, red,
/// near infrared, SWIR 1.6 µm, SWIR 2.2 µm. Radar uses the first two for
/// VV and VH backscatter (linear power).
#[derive(Clone, Copy, Debug, Default)]
struct Pixel {
    state: State,
    /// The sensor's own classification says water
    flagged_water: bool,
    b: [f32; 6],
}

const BLUE: usize = 0;
const GREEN: usize = 1;
const RED: usize = 2;
const NIR: usize = 3;
const SWIR1: usize = 4;
const SWIR2: usize = 5;
const VV: usize = 0;
const VH: usize = 1;

fn nd(a: f32, b: f32) -> Option<f32> {
    let s = a + b;
    (s > 1e-4).then(|| (a - b) / s)
}

impl Pixel {
    fn ndvi(&self) -> Option<f32> {
        nd(self.b[NIR], self.b[RED])
    }
    fn ndmi(&self) -> Option<f32> {
        nd(self.b[NIR], self.b[SWIR1])
    }
    fn mndwi(&self) -> Option<f32> {
        nd(self.b[GREEN], self.b[SWIR1])
    }
    fn nbr(&self) -> Option<f32> {
        nd(self.b[NIR], self.b[SWIR2])
    }
    fn vv_db(&self) -> f32 {
        10.0 * self.b[VV].max(1e-6).log10()
    }
    fn vh_db(&self) -> f32 {
        10.0 * self.b[VH].max(1e-6).log10()
    }
}

/// Open-water thresholds: MNDWI on reflectance (Xu 2006), and VV
/// backscatter for radar, where calm water is a dark mirror.
const MNDWI_WATER: f32 = 0.05;
const VV_WATER_DB: f32 = -18.0;

fn is_water(sensor: Sensor, p: &Pixel) -> bool {
    if p.state != State::Clear {
        return false;
    }
    if sensor.is_radar() {
        p.vv_db() < VV_WATER_DB
    } else {
        p.flagged_water || p.mndwi().is_some_and(|v| v > MNDWI_WATER)
    }
}

/// Pixels north to south, `n` by `n`, for one scene.
fn read_scene(
    fetcher: &Fetcher,
    area: &Area,
    sensor: Sensor,
    item: &Item,
    n: usize,
) -> Result<Vec<Pixel>, String> {
    let a = pc::crop_array(
        fetcher,
        area,
        sensor.collection(),
        &item.id,
        n,
        sensor.assets(),
    )?;
    let bands = sensor.bands();
    if a.shape != [bands + 1, n, n] {
        return Err(format!(
            "unexpected {} array shape {:?}",
            sensor.label(),
            a.shape
        ));
    }
    let mut out = vec![Pixel::default(); n * n];
    for row in 0..n {
        for col in 0..n {
            let p = &mut out[row * n + col];
            if a.at(bands, row, col) == 0.0 {
                continue;
            }
            match sensor {
                Sensor::Sentinel1 => {
                    let (vv, vh) = (a.at(0, row, col), a.at(1, row, col));
                    if !(vv > 0.0 && vv.is_finite()) {
                        continue;
                    }
                    p.b[VV] = vv as f32;
                    p.b[VH] = vh.max(0.0) as f32;
                    p.state = State::Clear;
                }
                Sensor::Sentinel2 => {
                    for k in 0..6 {
                        p.b[k] = crate::water::reflectance(a.at(k, row, col), item) as f32;
                    }
                    p.state = match a.at(6, row, col) as u8 {
                        0 | 1 => State::NoData,
                        3 => State::Shadow,
                        8..=10 => State::Cloud,
                        11 => State::Snow,
                        _ => State::Clear,
                    };
                    p.flagged_water = a.at(6, row, col) as u8 == 6;
                }
                Sensor::Landsat => {
                    for k in 0..6 {
                        // Collection 2 level 2 scaling
                        p.b[k] = (a.at(k, row, col) * 2.75e-5 - 0.2).max(0.0) as f32;
                    }
                    let qa = a.at(6, row, col) as u32;
                    let bit = |i: u32| qa >> i & 1 == 1;
                    p.state = if bit(0) {
                        State::NoData
                    } else if bit(3) || bit(1) || bit(2) {
                        State::Cloud
                    } else if bit(4) {
                        State::Shadow
                    } else if bit(5) {
                        State::Snow
                    } else {
                        State::Clear
                    };
                    p.flagged_water = bit(7);
                }
            }
        }
    }
    Ok(out)
}

/// Grid side for an area: about 20 m pixels, between 256 and 768.
pub fn grid_size(area: &Area) -> usize {
    ((area.size_m / 20.0) as usize).clamp(256, 768)
}

/// Assemble the pixels for a query. Returns the pixels and the scenes that
/// contributed.
fn assemble(
    fetcher: &Fetcher,
    area: &Area,
    q: &SceneQuery,
    n: usize,
) -> Result<(Vec<Pixel>, Vec<SceneInfo>), String> {
    area.validate()?;
    let sensor = q.sensor;
    let items: Vec<Item> = match &q.scene {
        Some(id) => {
            // the date in the id narrows the search to that day
            let item = find_item(fetcher, area, sensor, id, q.as_of)?;
            vec![item]
        }
        None => {
            let end = q.as_of.unwrap_or_else(crate::unix_now);
            let start = end - i64::from(q.window_days.clamp(1, 3650)) * 86_400;
            let cloud = (!sensor.is_radar()).then_some(q.max_cloud);
            let mut items = pc::search(
                fetcher,
                area,
                sensor.collection(),
                Some((start, end)),
                cloud,
                20,
            )?;
            if items.is_empty() {
                return Err(format!(
                    "no {} scene between {} and {}{}",
                    sensor.label(),
                    iso_date(start),
                    iso_date(end),
                    if sensor.is_radar() {
                        String::new()
                    } else {
                        format!(" with under {:.0} % cloud", q.max_cloud)
                    }
                ));
            }
            if !sensor.is_radar() {
                // clearest first; newest first among equals (search order)
                items.sort_by(|a, b| a.cloud.total_cmp(&b.cloud));
            }
            items
        }
    };

    let mut px = vec![Pixel::default(); n * n];
    let mut used = Vec::new();
    let single = items.len() == 1;
    for item in items.iter().take(6) {
        let scene = read_scene(fetcher, area, sensor, item, n)?;
        let mut filled = false;
        for (dst, src) in px.iter_mut().zip(&scene) {
            let take = match (dst.state, src.state) {
                (_, State::NoData) => false,
                (State::NoData, _) => true,
                // a clear pixel replaces cloud, shadow or snow of an earlier scene
                (State::Clear, _) => false,
                (_, State::Clear) => true,
                _ => false,
            };
            if take {
                *dst = *src;
                filled = true;
            }
        }
        if filled || single {
            used.push(scene_info(sensor, item));
        }
        if px.iter().all(|p| p.state == State::Clear) {
            break;
        }
    }
    if used.is_empty() {
        return Err(format!(
            "the {} scenes found do not cover this area",
            sensor.label()
        ));
    }
    Ok((px, used))
}

fn find_item(
    fetcher: &Fetcher,
    area: &Area,
    sensor: Sensor,
    id: &str,
    hint: Option<i64>,
) -> Result<Item, String> {
    let day = scene_day(id).or(hint);
    let window = day.map(|d| (d - 2 * 86_400, d + 2 * 86_400));
    let items = pc::search(fetcher, area, sensor.collection(), window, None, 100)?;
    items
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| format!("scene {id} not found over this area"))
}

/// The acquisition day encoded in a scene id, as Unix seconds at noon UTC.
/// Sentinel-2: `S2B_MSIL2A_20220905T055649_...`; Sentinel-1:
/// `S1A_IW_GRDH_1SDV_20221011T001845_...`; Landsat: `LC08_L2SP_150041_20220905_...`.
pub fn scene_day(id: &str) -> Option<i64> {
    id.split('_').find_map(|part| {
        let digits = part.get(..8)?;
        if !digits.bytes().all(|b| b.is_ascii_digit())
            || !(part.len() == 8 || part.as_bytes()[8] == b'T')
        {
            return None;
        }
        let y: i64 = digits[..4].parse().ok()?;
        if !(1972..=2100).contains(&y) {
            return None;
        }
        crate::parse_iso_utc(&format!(
            "{}-{}-{}T12:00:00Z",
            &digits[..4],
            &digits[4..6],
            &digits[6..8]
        ))
    })
}

/// A colour on a legend: value and `#rrggbb`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LegendStop {
    pub value: f64,
    pub color: String,
    pub label: Option<String>,
}

/// Shares of the assessed area. Fractions are of the whole grid; the
/// cover classes add up to `clear_fraction`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneStats {
    /// Could be read (not cloud, shadow, snow or outside the scene)
    pub clear_fraction: f64,
    pub cloud_fraction: f64,
    pub snow_fraction: f64,
    pub water_fraction: f64,
    /// NDVI above 0.6
    pub dense_vegetation_fraction: f64,
    /// NDVI 0.3-0.6
    pub moderate_vegetation_fraction: f64,
    /// NDVI 0.15-0.3
    pub sparse_vegetation_fraction: f64,
    /// NDVI below 0.15 and not water: bare soil, rock, built-up, burnt
    pub bare_fraction: f64,
    pub mean_ndvi: Option<f64>,
    pub mean_ndmi: Option<f64>,
    /// Radar only: mean VV backscatter [dB]
    pub mean_vv_db: Option<f64>,
}

fn stats(sensor: Sensor, px: &[Pixel]) -> SceneStats {
    let total = px.len().max(1) as f64;
    let mut s = SceneStats::default();
    let (mut ndvi, mut ndmi, mut vv, mut nv) = (0.0, 0.0, 0.0, 0usize);
    for p in px {
        match p.state {
            State::Cloud | State::Shadow => s.cloud_fraction += 1.0,
            State::Snow => s.snow_fraction += 1.0,
            State::NoData => {}
            State::Clear => {
                s.clear_fraction += 1.0;
                if is_water(sensor, p) {
                    s.water_fraction += 1.0;
                }
                if sensor.is_radar() {
                    vv += f64::from(p.vv_db());
                    nv += 1;
                    continue;
                }
                let (Some(v), Some(m)) = (p.ndvi(), p.ndmi()) else {
                    continue;
                };
                ndvi += f64::from(v);
                ndmi += f64::from(m);
                nv += 1;
                if is_water(sensor, p) {
                    continue;
                }
                match v {
                    v if v > 0.6 => s.dense_vegetation_fraction += 1.0,
                    v if v > 0.3 => s.moderate_vegetation_fraction += 1.0,
                    v if v > 0.15 => s.sparse_vegetation_fraction += 1.0,
                    _ => s.bare_fraction += 1.0,
                }
            }
        }
    }
    for f in [
        &mut s.clear_fraction,
        &mut s.cloud_fraction,
        &mut s.snow_fraction,
        &mut s.water_fraction,
        &mut s.dense_vegetation_fraction,
        &mut s.moderate_vegetation_fraction,
        &mut s.sparse_vegetation_fraction,
        &mut s.bare_fraction,
    ] {
        *f /= total;
    }
    if nv > 0 {
        if sensor.is_radar() {
            s.mean_vv_db = Some(vv / nv as f64);
        } else {
            s.mean_ndvi = Some(ndvi / nv as f64);
            s.mean_ndmi = Some(ndmi / nv as f64);
        }
    }
    s
}

type Rgb = [u8; 3];

fn hex(c: &str) -> Rgb {
    let v = u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// Piecewise-linear colour ramp.
fn ramp(stops: &[(f32, &str)], v: f32) -> Rgb {
    let first = stops[0];
    if v <= first.0 {
        return hex(first.1);
    }
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if v <= b {
            let t = (v - a) / (b - a);
            let (ca, cb) = (hex(ca), hex(cb));
            return std::array::from_fn(|k| {
                (f32::from(ca[k]) + (f32::from(cb[k]) - f32::from(ca[k])) * t).round() as u8
            });
        }
    }
    hex(stops[stops.len() - 1].1)
}

const NDVI_RAMP: [(f32, &str); 6] = [
    (-0.2, "#8c510a"),
    (0.1, "#d8b365"),
    (0.25, "#f2e6b1"),
    (0.4, "#b8e186"),
    (0.6, "#4dac26"),
    (0.85, "#14632a"),
];
const NDMI_RAMP: [(f32, &str); 5] = [
    (-0.4, "#a6611a"),
    (-0.1, "#dfc27d"),
    (0.1, "#e8eef0"),
    (0.35, "#4393c3"),
    (0.6, "#08306b"),
];
const NBR_RAMP: [(f32, &str); 5] = [
    (-0.5, "#5c0018"),
    (-0.1, "#d6604d"),
    (0.1, "#fddbc7"),
    (0.3, "#a6d96a"),
    (0.6, "#1a9641"),
];
const WATER_COLOR: &str = "#1f78d1";
const VEG_WATER: &str = "#3d6f9e";
const CLOUD: [u8; 4] = [205, 208, 212, 150];

fn legend(stops: &[(f32, &str)], labels: &[(usize, &str)]) -> Vec<LegendStop> {
    stops
        .iter()
        .enumerate()
        .map(|(k, (v, c))| LegendStop {
            value: f64::from(*v),
            color: (*c).to_string(),
            label: labels
                .iter()
                .find(|(i, _)| *i == k)
                .map(|(_, l)| (*l).to_string()),
        })
        .collect()
}

/// Display stretch for reflectance: 0-0.3 to 0-255 with a mild gamma.
fn stretch(r: f32) -> u8 {
    ((r / 0.3).clamp(0.0, 1.0).powf(1.0 / 1.25) * 255.0).round() as u8
}

fn db_stretch(db: f32, lo: f32, hi: f32) -> u8 {
    (((db - lo) / (hi - lo)).clamp(0.0, 1.0) * 255.0).round() as u8
}

fn grey(p: &Pixel, sensor: Sensor) -> Rgb {
    let l = if sensor.is_radar() {
        db_stretch(p.vv_db(), -25.0, 0.0)
    } else {
        stretch(0.3 * p.b[RED] + 0.59 * p.b[GREEN] + 0.11 * p.b[BLUE])
    };
    // muted, so the colour drawn over it stands out
    let l = 60 + (u16::from(l) * 150 / 255) as u8;
    [l, l, l]
}

fn colour(sensor: Sensor, product: Product, p: &Pixel) -> Option<Rgb> {
    let rgb = match product {
        Product::TrueColor => [stretch(p.b[RED]), stretch(p.b[GREEN]), stretch(p.b[BLUE])],
        Product::FalseColor => [
            stretch(p.b[NIR] * 0.6),
            stretch(p.b[RED]),
            stretch(p.b[GREEN]),
        ],
        Product::Agriculture => [
            stretch(p.b[SWIR1] * 0.75),
            stretch(p.b[NIR] * 0.6),
            stretch(p.b[BLUE] * 1.4),
        ],
        // water reads as "no vegetation" too; show it as water instead
        Product::Vegetation if is_water(sensor, p) => hex(VEG_WATER),
        Product::Vegetation => ramp(&NDVI_RAMP, p.ndvi()?),
        Product::Moisture => ramp(&NDMI_RAMP, p.ndmi()?),
        Product::Burn => ramp(&NBR_RAMP, p.nbr()?),
        Product::Water => {
            if is_water(sensor, p) {
                hex(WATER_COLOR)
            } else {
                grey(p, sensor)
            }
        }
        Product::Radar => {
            let (vv, vh) = (p.vv_db(), p.vh_db());
            [
                db_stretch(vv, -20.0, 0.0),
                db_stretch(vh, -28.0, -6.0),
                db_stretch(vv - vh, 0.0, 15.0),
            ]
        }
    };
    Some(rgb)
}

/// A rendered scene with its numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneImage {
    /// PNG, north up, covering `bounds`
    pub png_base64: String,
    pub width: usize,
    pub height: usize,
    /// (west, south, east, north) in local metres
    pub bounds: [f64; 4],
    pub sensor: Sensor,
    pub product: Product,
    pub scenes: Vec<SceneInfo>,
    pub stats: SceneStats,
    /// Colour scale for index products; empty for composites
    pub legend: Vec<LegendStop>,
    pub attribution: String,
}

fn product_legend(product: Product) -> Vec<LegendStop> {
    match product {
        Product::Vegetation => legend(
            &NDVI_RAMP,
            &[(0, "bare"), (3, "crops, grass"), (5, "dense")],
        ),
        Product::Moisture => legend(&NDMI_RAMP, &[(0, "dry"), (4, "wet")]),
        Product::Burn => legend(&NBR_RAMP, &[(0, "burnt"), (4, "healthy")]),
        Product::Water => vec![LegendStop {
            value: 1.0,
            color: WATER_COLOR.into(),
            label: Some("open water".into()),
        }],
        _ => Vec::new(),
    }
}

pub(crate) fn encode_png(rgba: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn attribution(sensor: Sensor, scenes: &[SceneInfo]) -> String {
    let years: std::collections::BTreeSet<&str> =
        scenes.iter().filter_map(|s| s.date.get(..4)).collect();
    sensor.attribution(&years.into_iter().collect::<Vec<_>>().join(", "))
}

/// Render `product` for a query.
pub fn scene_image(
    fetcher: &Fetcher,
    area: &Area,
    q: &SceneQuery,
    product: Product,
) -> Result<SceneImage, String> {
    if !product.fits(q.sensor) {
        return Err(format!(
            "{product:?} is not available from {}",
            q.sensor.label()
        ));
    }
    let n = grid_size(area);
    let (px, scenes) = assemble(fetcher, area, q, n)?;
    let mut rgba = vec![0u8; n * n * 4];
    for (dst, p) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(&px) {
        *dst = match p.state {
            State::NoData => [0, 0, 0, 0],
            State::Cloud | State::Shadow if !q.sensor.is_radar() => CLOUD,
            _ => match colour(q.sensor, product, p) {
                Some([r, g, b]) => [r, g, b, 255],
                None => [0, 0, 0, 0],
            },
        };
    }
    let h = area.size_m / 2.0;
    Ok(SceneImage {
        png_base64: base64::engine::general_purpose::STANDARD.encode(encode_png(&rgba, n, n)?),
        width: n,
        height: n,
        bounds: [-h, -h, h, h],
        sensor: q.sensor,
        product,
        stats: stats(q.sensor, &px),
        legend: product_legend(product),
        attribution: attribution(q.sensor, &scenes),
        scenes,
    })
}

/// What changed between two looks at an area.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChangeStats {
    /// Clear in both looks
    pub compared_fraction: f64,
    /// NDVI fell by more than 0.2 from vegetated ground
    pub vegetation_loss_fraction: f64,
    /// NDVI rose by more than 0.2
    pub vegetation_gain_fraction: f64,
    /// Water after, dry before
    pub new_water_fraction: f64,
    /// Water before, dry after
    pub receded_water_fraction: f64,
    /// dNBR above 0.27 (moderate burn severity or worse)
    pub burn_fraction: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeReport {
    pub png_base64: String,
    pub width: usize,
    pub height: usize,
    pub bounds: [f64; 4],
    pub before: Vec<SceneInfo>,
    pub after: Vec<SceneInfo>,
    pub stats: ChangeStats,
    pub legend: Vec<LegendStop>,
    pub method: String,
    pub attribution: String,
}

const LOSS: &str = "#d7301f";
const GAIN: &str = "#31a354";
const NEW_WATER: &str = "#1f78d1";
const RECEDED: &str = "#9ecae1";
const BURN: &str = "#7a0177";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Change {
    None,
    Loss,
    Gain,
    NewWater,
    Receded,
    Burn,
}

fn change(sb: Sensor, b: &Pixel, sa: Sensor, a: &Pixel) -> Option<Change> {
    if b.state != State::Clear || a.state != State::Clear {
        return None;
    }
    let (wb, wa) = (is_water(sb, b), is_water(sa, a));
    if wa && !wb {
        return Some(Change::NewWater);
    }
    if wb && !wa {
        return Some(Change::Receded);
    }
    if sb.is_radar() || sa.is_radar() || wa {
        return Some(Change::None);
    }
    let (Some(vb), Some(va)) = (b.ndvi(), a.ndvi()) else {
        return Some(Change::None);
    };
    let dnbr = match (b.nbr(), a.nbr()) {
        (Some(x), Some(y)) => x - y,
        _ => 0.0,
    };
    Some(if dnbr > 0.27 && charred(a) && vb > 0.2 && va < 0.4 {
        Change::Burn
    } else if va - vb < -0.2 && vb > 0.3 {
        Change::Loss
    } else if va - vb > 0.2 {
        Change::Gain
    } else {
        Change::None
    })
}

/// Burnt ground is dark in both red and near infrared, unlike cleared
/// fields or landslide scars, which are bright.
fn charred(p: &Pixel) -> bool {
    (p.b[RED] + p.b[NIR]) / 2.0 < 0.15
}

/// Compare two looks at the area. Optical pairs report vegetation, water
/// and burn changes; a pair with radar on either side reports water only.
pub fn compare(
    fetcher: &Fetcher,
    area: &Area,
    before: &SceneQuery,
    after: &SceneQuery,
) -> Result<ChangeReport, String> {
    let n = grid_size(area);
    let (pb, sb) = assemble(fetcher, area, before, n)?;
    let (pa, sa) = assemble(fetcher, area, after, n)?;
    let mut rgba = vec![0u8; n * n * 4];
    let mut st = ChangeStats::default();
    for (k, dst) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (b, a) = (&pb[k], &pa[k]);
        let Some(c) = change(before.sensor, b, after.sensor, a) else {
            *dst = if a.state == State::NoData || b.state == State::NoData {
                [0, 0, 0, 0]
            } else {
                CLOUD
            };
            continue;
        };
        st.compared_fraction += 1.0;
        let color = match c {
            Change::None => None,
            Change::Loss => Some((LOSS, &mut st.vegetation_loss_fraction)),
            Change::Gain => Some((GAIN, &mut st.vegetation_gain_fraction)),
            Change::NewWater => Some((NEW_WATER, &mut st.new_water_fraction)),
            Change::Receded => Some((RECEDED, &mut st.receded_water_fraction)),
            Change::Burn => Some((BURN, &mut st.burn_fraction)),
        };
        let [r, g, bl] = match color {
            Some((c, f)) => {
                *f += 1.0;
                hex(c)
            }
            None => grey(a, after.sensor),
        };
        *dst = [r, g, bl, 255];
    }
    let total = (n * n) as f64;
    for f in [
        &mut st.compared_fraction,
        &mut st.vegetation_loss_fraction,
        &mut st.vegetation_gain_fraction,
        &mut st.new_water_fraction,
        &mut st.receded_water_fraction,
        &mut st.burn_fraction,
    ] {
        *f /= total;
    }
    let item = |c: &str, l: &str| LegendStop {
        value: 1.0,
        color: c.into(),
        label: Some(l.into()),
    };
    let radar = before.sensor.is_radar() || after.sensor.is_radar();
    let mut legend = vec![item(NEW_WATER, "new water"), item(RECEDED, "water receded")];
    if !radar {
        legend.extend([
            item(LOSS, "vegetation loss"),
            item(GAIN, "vegetation gain"),
            item(BURN, "burn scar"),
        ]);
    }
    let h = area.size_m / 2.0;
    let mut credits = vec![attribution(before.sensor, &sb)];
    let a2 = attribution(after.sensor, &sa);
    if a2 != credits[0] {
        credits.push(a2);
    }
    Ok(ChangeReport {
        png_base64: base64::engine::general_purpose::STANDARD.encode(encode_png(&rgba, n, n)?),
        width: n,
        height: n,
        bounds: [-h, -h, h, h],
        before: sb,
        after: sa,
        stats: st,
        legend,
        method: if radar {
            "Open water by radar backscatter (VV below -18 dB) or MNDWI above 0.05, compared on pixels clear in both looks".into()
        } else {
            "On pixels clear in both looks: water by MNDWI above 0.05 or the sensor's water class; vegetation loss or gain where NDVI changed by more than 0.2; burn scars where dNBR exceeds 0.27".into()
        },
        attribution: credits.join(" · "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn optical(b: [f32; 6]) -> Pixel {
        Pixel {
            state: State::Clear,
            flagged_water: false,
            b,
        }
    }

    // typical surface reflectances: blue, green, red, NIR, SWIR1, SWIR2
    const FOREST: [f32; 6] = [0.03, 0.05, 0.03, 0.35, 0.15, 0.07];
    const WATER: [f32; 6] = [0.06, 0.07, 0.05, 0.03, 0.01, 0.005];
    const BARE: [f32; 6] = [0.12, 0.16, 0.2, 0.25, 0.32, 0.28];
    const BURNT: [f32; 6] = [0.04, 0.05, 0.06, 0.1, 0.18, 0.2];

    #[test]
    fn indices_separate_cover_types() {
        let (f, w, b) = (optical(FOREST), optical(WATER), optical(BARE));
        assert!(f.ndvi().unwrap() > 0.8);
        assert!(b.ndvi().unwrap() < 0.15);
        assert!(is_water(Sensor::Sentinel2, &w));
        assert!(!is_water(Sensor::Sentinel2, &f));
        assert!(!is_water(Sensor::Sentinel2, &b));
        let s = stats(Sensor::Sentinel2, &[f, w, b, Pixel::default()]);
        assert_eq!(s.clear_fraction, 0.75);
        assert_eq!(s.water_fraction, 0.25);
        assert_eq!(s.dense_vegetation_fraction, 0.25);
        assert_eq!(s.bare_fraction, 0.25);
    }

    #[test]
    fn change_classes() {
        let s2 = Sensor::Sentinel2;
        let c = |b, a| change(s2, &optical(b), s2, &optical(a)).unwrap();
        assert_eq!(c(FOREST, FOREST), Change::None);
        assert_eq!(c(FOREST, WATER), Change::NewWater);
        assert_eq!(c(WATER, BARE), Change::Receded);
        assert_eq!(c(FOREST, BURNT), Change::Burn);
        assert_eq!(c(FOREST, BARE), Change::Loss);
        assert_eq!(c(BARE, FOREST), Change::Gain);
        let cloudy = Pixel {
            state: State::Cloud,
            ..optical(FOREST)
        };
        assert_eq!(change(s2, &cloudy, s2, &optical(BARE)), None);
    }

    #[test]
    fn radar_reads_calm_water_as_dark() {
        let r = |vv: f32| Pixel {
            state: State::Clear,
            flagged_water: false,
            b: [vv, vv / 5.0, 0.0, 0.0, 0.0, 0.0],
        };
        assert!(is_water(Sensor::Sentinel1, &r(0.005))); // -23 dB
        assert!(!is_water(Sensor::Sentinel1, &r(0.1))); // -10 dB
        let s = stats(Sensor::Sentinel1, &[r(0.005), r(0.1)]);
        assert_eq!(s.water_fraction, 0.5);
        assert!(s.mean_ndvi.is_none() && s.mean_vv_db.is_some());
    }

    #[test]
    fn ramp_interpolates_and_clamps() {
        let stops = [(0.0, "#000000"), (1.0, "#ff8040")];
        assert_eq!(ramp(&stops, -1.0), [0, 0, 0]);
        assert_eq!(ramp(&stops, 0.5), [128, 64, 32]);
        assert_eq!(ramp(&stops, 2.0), [255, 128, 64]);
    }

    #[test]
    fn scene_days_from_ids() {
        let day = |s| scene_day(s).map(iso_date);
        assert_eq!(
            day("S2B_MSIL2A_20220905T055649_R091_T42RUQ_20220905T090412").as_deref(),
            Some("2022-09-05")
        );
        assert_eq!(
            day("S1D_IW_GRDH_1SDV_20261011T001845_20261011T001910_004960_0095BA_rtc").as_deref(),
            Some("2026-10-11")
        );
        assert_eq!(
            day("LT05_L2SP_141041_19900714_02_T2").as_deref(),
            Some("1990-07-14")
        );
        assert_eq!(scene_day("no-date-here"), None);
    }

    #[test]
    fn products_match_sensors() {
        assert!(Product::Vegetation.fits(Sensor::Landsat));
        assert!(!Product::Vegetation.fits(Sensor::Sentinel1));
        assert!(Product::Radar.fits(Sensor::Sentinel1));
        assert!(!Product::Radar.fits(Sensor::Sentinel2));
        assert!(Product::Water.fits(Sensor::Sentinel1) && Product::Water.fits(Sensor::Sentinel2));
    }
}
