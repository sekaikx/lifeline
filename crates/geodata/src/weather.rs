//! Wind profile for the planning area from a weather forecast.
//!
//! Two providers, chosen explicitly because their terms differ:
//!
//! * **MET Norway Locationforecast** (default): free for any use with
//!   attribution (CC BY 4.0 / NLOD). It gives surface (10 m) wind only, so
//!   the profile aloft is extrapolated with the 1/7 power law and marked
//!   approximate.
//! * **Open-Meteo**: winds at 10, 80, 120 and 180 m plus pressure levels up
//!   to 500 hPa, a real vertical profile. The free API is for non-commercial
//!   use; commercial use needs an API key (`OPEN_METEO_API_KEY`).

use crate::{unix_now, Area, Fetcher};
use lifeline_core::wind::{Wind, WindLayer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

const HEIGHTS: [u32; 4] = [10, 80, 120, 180];
const LEVELS: [u32; 10] = [1000, 975, 950, 925, 900, 850, 800, 700, 600, 500];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindProvider {
    #[default]
    MetNorway,
    OpenMeteo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindForecast {
    pub wind: Wind,
    /// Forecast hour used [Unix seconds]
    pub valid_unix: i64,
    /// Ground elevation of the forecast grid point [m]
    pub elevation: f64,
    pub source: String,
}

#[derive(Deserialize)]
struct Response {
    elevation: f64,
    hourly: HashMap<String, serde_json::Value>,
}

fn series(h: &HashMap<String, serde_json::Value>, key: &str) -> Vec<Option<f64>> {
    h.get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|x| x.as_f64()).collect())
        .unwrap_or_default()
}

/// Turn an Open-Meteo response into a profile for the hour nearest `at_unix`.
pub fn profile_from_open_meteo(json: &[u8], at_unix: i64) -> Result<WindForecast, String> {
    let r: Response =
        serde_json::from_slice(json).map_err(|e| format!("invalid Open-Meteo response: {e}"))?;
    let times: Vec<i64> = series(&r.hourly, "time")
        .iter()
        .filter_map(|t| t.map(|v| v as i64))
        .collect();
    let k = (0..times.len())
        .min_by_key(|&i| (times[i] - at_unix).abs())
        .ok_or("the forecast has no hours")?;

    let mut layers: Vec<WindLayer> = Vec::new();
    let mut push = |altitude: f64, speed: Option<f64>, dir: Option<f64>| {
        if let (Some(speed), Some(from_deg)) = (speed, dir) {
            layers.push(WindLayer {
                altitude,
                speed,
                from_deg,
            });
        }
    };
    for h in HEIGHTS {
        let s = series(&r.hourly, &format!("wind_speed_{h}m"));
        let d = series(&r.hourly, &format!("wind_direction_{h}m"));
        push(
            r.elevation + f64::from(h),
            s.get(k).copied().flatten(),
            d.get(k).copied().flatten(),
        );
    }
    let top_agl = r.elevation + f64::from(*HEIGHTS.last().unwrap());
    for p in LEVELS {
        let z = series(&r.hourly, &format!("geopotential_height_{p}hPa"))
            .get(k)
            .copied()
            .flatten();
        let Some(z) = z else { continue };
        // levels below the 180 m wind are underground or duplicate it
        if z <= top_agl + 50.0 {
            continue;
        }
        let s = series(&r.hourly, &format!("wind_speed_{p}hPa"));
        let d = series(&r.hourly, &format!("wind_direction_{p}hPa"));
        push(z, s.get(k).copied().flatten(), d.get(k).copied().flatten());
    }
    layers.sort_by(|a, b| a.altitude.total_cmp(&b.altitude));
    layers.dedup_by(|a, b| (a.altitude - b.altitude).abs() < 1.0);
    if layers.is_empty() {
        return Err("the forecast has no wind for that hour".into());
    }
    Ok(WindForecast {
        wind: Wind::Layered { layers },
        valid_unix: times[k],
        elevation: r.elevation,
        source: "Open-Meteo forecast (CC BY 4.0)".into(),
    })
}

/// Forecast wind profile at the centre of `area`, `hours_ahead` from now.
pub fn wind_profile(
    fetcher: &Fetcher,
    area: &Area,
    hours_ahead: f64,
    provider: WindProvider,
) -> Result<WindForecast, String> {
    area.validate()?;
    let at = unix_now() + (hours_ahead * 3_600.0) as i64;
    match provider {
        WindProvider::OpenMeteo => open_meteo(fetcher, area, at),
        WindProvider::MetNorway => met_norway(fetcher, area, at),
    }
}

fn open_meteo(fetcher: &Fetcher, area: &Area, at: i64) -> Result<WindForecast, String> {
    let mut vars: Vec<String> = Vec::new();
    for h in HEIGHTS {
        vars.push(format!("wind_speed_{h}m"));
        vars.push(format!("wind_direction_{h}m"));
    }
    for p in LEVELS {
        vars.push(format!("wind_speed_{p}hPa"));
        vars.push(format!("wind_direction_{p}hPa"));
        vars.push(format!("geopotential_height_{p}hPa"));
    }
    let (host, key) = match std::env::var("OPEN_METEO_API_KEY") {
        Ok(k) if !k.is_empty() => ("customer-api.open-meteo.com", format!("&apikey={k}")),
        _ => ("api.open-meteo.com", String::new()),
    };
    let url = format!(
        "https://{host}/v1/forecast?latitude={:.4}&longitude={:.4}&hourly={}&wind_speed_unit=ms&timeformat=unixtime&forecast_days=3&past_days=1{key}",
        area.lat,
        area.lon,
        vars.join(",")
    );
    let bytes = fetcher.get(&url, Some(Duration::from_secs(3_600)))?;
    profile_from_open_meteo(&bytes, at)
}

#[derive(Deserialize)]
struct MetResponse {
    geometry: MetGeometry,
    properties: MetProperties,
}

#[derive(Deserialize)]
struct MetGeometry {
    coordinates: Vec<f64>,
}

#[derive(Deserialize)]
struct MetProperties {
    timeseries: Vec<MetStep>,
}

#[derive(Deserialize)]
struct MetStep {
    time: String,
    data: MetData,
}

#[derive(Deserialize)]
struct MetData {
    instant: MetInstant,
}

#[derive(Deserialize)]
struct MetInstant {
    details: HashMap<String, f64>,
}

/// Heights above ground [m] at which the surface wind is extrapolated.
const EXTRAPOLATE_AGL: [f64; 6] = [10.0, 50.0, 150.0, 400.0, 1_000.0, 2_000.0];

/// Profile from a MET Norway response: surface wind at the hour nearest
/// `at_unix`, carried aloft as `v(h) = v10 · (h / 10)^(1/7)`.
pub fn profile_from_met_norway(json: &[u8], at_unix: i64) -> Result<WindForecast, String> {
    let r: MetResponse =
        serde_json::from_slice(json).map_err(|e| format!("invalid MET Norway response: {e}"))?;
    let elevation = r.geometry.coordinates.get(2).copied().unwrap_or(0.0);
    let step = r
        .properties
        .timeseries
        .iter()
        .filter_map(|s| parse_iso_utc(&s.time).map(|t| (t, s)))
        .min_by_key(|(t, _)| (t - at_unix).abs())
        .ok_or("the forecast has no time steps")?;
    let d = &step.1.data.instant.details;
    let (Some(speed), Some(dir)) = (d.get("wind_speed"), d.get("wind_from_direction")) else {
        return Err("the forecast has no wind for that hour".into());
    };
    let layers = EXTRAPOLATE_AGL
        .iter()
        .map(|h| WindLayer {
            altitude: elevation + h,
            speed: speed * (h / 10.0).powf(1.0 / 7.0),
            from_deg: *dir,
        })
        .collect();
    Ok(WindForecast {
        wind: Wind::Layered { layers },
        valid_unix: step.0,
        elevation,
        source: "MET Norway surface forecast (CC BY 4.0), extrapolated aloft with a 1/7 power law"
            .into(),
    })
}

fn met_norway(fetcher: &Fetcher, area: &Area, at: i64) -> Result<WindForecast, String> {
    let url = format!(
        "https://api.met.no/weatherapi/locationforecast/2.0/complete?lat={:.4}&lon={:.4}",
        area.lat, area.lon
    );
    let bytes = fetcher.get(&url, Some(Duration::from_secs(3_600)))?;
    profile_from_met_norway(&bytes, at)
}

/// Parse `YYYY-MM-DDTHH:MM:SSZ` (UTC) to Unix seconds.
pub fn parse_iso_utc(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Howard Hinnant's days_from_civil
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3_600 + mm * 60 + ss)
}
