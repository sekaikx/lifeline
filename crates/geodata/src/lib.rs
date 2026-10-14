//! # lifeline-geodata
//!
//! Real-world inputs for a planning area, all from open data:
//!
//! | What | Source | Licence |
//! | --- | --- | --- |
//! | Elevation | AWS Terrain Tiles (SRTM, GMTED, ETOPO1 and others, via Mapzen/Tilezen) | open, attribution |
//! | Roads, settlements | OpenStreetMap via the Overpass API | ODbL |
//! | Wind | MET Norway Locationforecast (default) or Open-Meteo | CC BY 4.0; Open-Meteo's free API is non-commercial |
//! | Imagery, water index | Copernicus Sentinel-2 L2A via Microsoft Planetary Computer | Copernicus open data |
//! | Permanent water | ESA WorldCover 2021 via Planetary Computer | CC BY 4.0 |
//!
//! Everything is fetched through [`Fetcher`], which caches responses on disk
//! so an area that has been loaded once keeps working offline.

mod dem;
mod fetch;
mod frame;
mod imagery;
mod osm;
mod pc;
mod water;
mod water_model;
mod weather;

pub use dem::terrain;
pub use fetch::Fetcher;
pub use frame::LocalFrame;
pub use imagery::{imagery, Imagery, Scene};
pub use osm::{network_from_overpass, osm, Place};
pub use water::{classify, detect_new_water, Observation, WaterDetection};
pub use water_model::{WaterModel, MODEL_BANDS, MODEL_ENV};
pub use weather::{
    parse_iso_utc, profile_from_met_norway, profile_from_open_meteo, wind_profile, WindForecast,
    WindProvider,
};

use serde::{Deserialize, Serialize};

/// A square planning area centred on a latitude and longitude.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Side of the square [m]
    pub size_m: f64,
    /// Terrain grid spacing [m]
    pub cell_m: f64,
}

impl Area {
    pub fn validate(&self) -> Result<(), String> {
        if !(-80.0..=80.0).contains(&self.lat) || !(-180.0..=180.0).contains(&self.lon) {
            return Err("area: latitude must be within ±80° and longitude within ±180°".into());
        }
        if !(2_000.0..=80_000.0).contains(&self.size_m) {
            return Err("area: size must be 2-80 km".into());
        }
        if !(20.0..=500.0).contains(&self.cell_m) || self.size_m / self.cell_m > 600.0 {
            return Err("area: cell size 20-500 m and at most 600 cells per side".into());
        }
        Ok(())
    }

    pub fn frame(&self) -> LocalFrame {
        LocalFrame::new(self.lat, self.lon)
    }

    /// (south, west, north, east) in degrees.
    pub fn bbox(&self) -> [f64; 4] {
        self.frame().bbox(self.size_m)
    }

    pub fn cells(&self) -> usize {
        (self.size_m / self.cell_m).round().max(1.0) as usize
    }
}

/// Seconds since the Unix epoch, now.
pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `YYYY-MM-DD` for a Unix time (proleptic Gregorian calendar, UTC).
pub fn iso_date(unix: i64) -> String {
    // Howard Hinnant's civil_from_days
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_round_trip() {
        for t in [0, 951_782_400, 1_790_000_000, 1_662_336_000 + 3_723] {
            let s = format!(
                "{}T{:02}:{:02}:{:02}Z",
                iso_date(t),
                (t % 86_400) / 3_600,
                (t % 3_600) / 60,
                t % 60
            );
            assert_eq!(parse_iso_utc(&s), Some(t), "{s}");
        }
    }

    #[test]
    fn iso_dates() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(951_782_400), "2000-02-29");
        assert_eq!(iso_date(1_790_000_000), "2026-09-21");
    }
}
