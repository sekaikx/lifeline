//! Local planning frame: metres east and north of an origin.
//!
//! An equirectangular tangent plane using the WGS84 radii of curvature at the
//! origin. Over a 40 km area the scale error is below 0.1 %, far smaller than
//! the data it is applied to, and the mapping is linear in latitude and
//! longitude, so lat/lon rasters (imagery) map onto the plane by a simple
//! stretch.

use serde::{Deserialize, Serialize};

const A: f64 = 6_378_137.0;
const E2: f64 = 0.006_694_379_990_14;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocalFrame {
    pub lat0: f64,
    pub lon0: f64,
    /// metres per degree of latitude
    m_lat: f64,
    /// metres per degree of longitude
    m_lon: f64,
}

impl LocalFrame {
    pub fn new(lat0: f64, lon0: f64) -> Self {
        let phi = lat0.to_radians();
        let w = (1.0 - E2 * phi.sin().powi(2)).sqrt();
        // radii of curvature along the meridian and the prime vertical
        let meridional = A * (1.0 - E2) / (w * w * w);
        let prime_vertical = A / w;
        let per_degree = std::f64::consts::PI / 180.0;
        Self {
            lat0,
            lon0,
            m_lat: meridional * per_degree,
            m_lon: prime_vertical * phi.cos() * per_degree,
        }
    }

    pub fn to_local(&self, lat: f64, lon: f64) -> [f64; 2] {
        [
            (lon - self.lon0) * self.m_lon,
            (lat - self.lat0) * self.m_lat,
        ]
    }

    pub fn to_geo(&self, x: f64, y: f64) -> (f64, f64) {
        (self.lat0 + y / self.m_lat, self.lon0 + x / self.m_lon)
    }

    /// (south, west, north, east) of a square of side `size` centred on the origin.
    pub fn bbox(&self, size: f64) -> [f64; 4] {
        let h = size / 2.0;
        let (s, w) = self.to_geo(-h, -h);
        let (n, e) = self.to_geo(h, h);
        [s, w, n, e]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_degree_scales() {
        let eq = LocalFrame::new(0.0, 0.0);
        // WGS84: 110.574 km per degree of latitude and 111.320 km of longitude at the equator
        assert!((eq.to_local(1.0, 0.0)[1] - 110_574.0).abs() < 2.0);
        assert!((eq.to_local(0.0, 1.0)[0] - 111_320.0).abs() < 2.0);
        let mid = LocalFrame::new(45.0, 10.0);
        assert!((mid.to_local(45.0, 11.0)[0] - 78_847.0).abs() < 5.0);
    }

    #[test]
    fn round_trip() {
        let f = LocalFrame::new(28.0, 84.6);
        let (lat, lon) = f.to_geo(12_345.0, -6_789.0);
        let p = f.to_local(lat, lon);
        assert!((p[0] - 12_345.0).abs() < 1e-6 && (p[1] + 6_789.0).abs() < 1e-6);
        let [s, w, n, e] = f.bbox(20_000.0);
        assert!(s < 28.0 && n > 28.0 && w < 84.6 && e > 84.6);
    }
}
