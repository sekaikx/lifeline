//! International Standard Atmosphere (ISA).
//!
//! The ISA (identical to the U.S. Standard Atmosphere 1976 below 86 km) is
//! piecewise: temperature is linear in *geopotential* altitude `H` inside each
//! layer, and pressure follows from the hydrostatic equation plus the ideal
//! gas law:
//!
//! ```text
//! gradient layer   (L != 0): p = p_b * (T / T_b)^(-g0 / (L R))
//! isothermal layer (L == 0): p = p_b * exp(-g0 (H - H_b) / (R T_b))
//! density:                   rho = p / (R T)
//! ```
//!
//! Public functions take *geometric* altitude above mean sea level in metres.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Standard gravity [m/s^2].
pub const G0: f64 = 9.806_65;
/// Specific gas constant of dry air [J/(kg K)].
pub const R_AIR: f64 = 287.052_87;
/// Earth radius used for the geopotential conversion [m].
pub const EARTH_RADIUS: f64 = 6_356_766.0;
/// Sea-level standard temperature [K].
pub const T0: f64 = 288.15;
/// Sea-level standard pressure [Pa].
pub const P0: f64 = 101_325.0;
/// Sea-level standard density [kg/m^3] (~1.225).
pub const RHO0: f64 = P0 / (R_AIR * T0);

/// (base geopotential altitude [m], lapse rate [K/m]) for each layer.
const LAYERS: [(f64, f64); 7] = [
    (0.0, -0.0065),
    (11_000.0, 0.0),
    (20_000.0, 0.0010),
    (32_000.0, 0.0028),
    (47_000.0, 0.0),
    (51_000.0, -0.0028),
    (71_000.0, -0.0020),
];
const TOP_GEOPOTENTIAL: f64 = 84_852.0;
const BOTTOM_GEOPOTENTIAL: f64 = -5_000.0;

#[derive(Clone, Copy, Debug)]
struct Layer {
    h_base: f64,
    lapse: f64,
    t_base: f64,
    p_base: f64,
}

fn pressure_in_layer(p_base: f64, t_base: f64, lapse: f64, dh: f64) -> f64 {
    if lapse == 0.0 {
        p_base * (-G0 * dh / (R_AIR * t_base)).exp()
    } else {
        p_base * ((t_base + lapse * dh) / t_base).powf(-G0 / (lapse * R_AIR))
    }
}

fn layers() -> &'static [Layer; 7] {
    static CELL: OnceLock<[Layer; 7]> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut out = [Layer {
            h_base: 0.0,
            lapse: 0.0,
            t_base: T0,
            p_base: P0,
        }; 7];
        for i in 0..LAYERS.len() {
            out[i].h_base = LAYERS[i].0;
            out[i].lapse = LAYERS[i].1;
            if i > 0 {
                let prev = out[i - 1];
                let dh = out[i].h_base - prev.h_base;
                out[i].t_base = prev.t_base + prev.lapse * dh;
                out[i].p_base = pressure_in_layer(prev.p_base, prev.t_base, prev.lapse, dh);
            }
        }
        out
    })
}

/// Geopotential altitude for a geometric altitude `z`.
pub fn geopotential_altitude(z: f64) -> f64 {
    EARTH_RADIUS * z / (EARTH_RADIUS + z)
}

/// Temperature, pressure and density at one altitude.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AirState {
    /// Kelvin
    pub temperature: f64,
    /// Pascal
    pub pressure: f64,
    /// kg/m^3
    pub density: f64,
}

/// Standard-day ISA properties at geometric altitude `z` [m].
///
/// Altitudes outside -5 km .. ~86 km are clamped to the model range; below
/// sea level the tropospheric layer is extrapolated, as standard tables do.
pub fn isa(z: f64) -> AirState {
    let h = geopotential_altitude(z).clamp(BOTTOM_GEOPOTENTIAL, TOP_GEOPOTENTIAL);
    let ls = layers();
    let layer = ls.iter().rev().find(|l| h >= l.h_base).unwrap_or(&ls[0]);
    let dh = h - layer.h_base;
    let temperature = layer.t_base + layer.lapse * dh;
    let pressure = pressure_in_layer(layer.p_base, layer.t_base, layer.lapse, dh);
    AirState {
        temperature,
        pressure,
        density: pressure / (R_AIR * temperature),
    }
}

/// Atmosphere model selectable from the UI / scenario file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Atmosphere {
    /// ISA pressure profile; `temperature_offset` [K] gives hot/cold days
    /// ("ISA+15" is `temperature_offset = 15`).
    Isa {
        #[serde(default)]
        temperature_offset: f64,
    },
    /// Uniform density, handy for analytic checks.
    Constant { rho: f64 },
    /// No air at all: zero drag.
    Vacuum,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Atmosphere::Isa {
            temperature_offset: 0.0,
        }
    }
}

impl Atmosphere {
    pub fn density(&self, z: f64) -> f64 {
        match *self {
            Atmosphere::Isa { temperature_offset } => {
                let a = isa(z);
                a.pressure / (R_AIR * (a.temperature + temperature_offset))
            }
            Atmosphere::Constant { rho } => rho,
            Atmosphere::Vacuum => 0.0,
        }
    }

    pub fn air(&self, z: f64) -> AirState {
        match *self {
            Atmosphere::Isa { temperature_offset } => {
                let a = isa(z);
                let t = a.temperature + temperature_offset;
                AirState {
                    temperature: t,
                    pressure: a.pressure,
                    density: a.pressure / (R_AIR * t),
                }
            }
            _ => {
                let a = isa(z);
                AirState {
                    density: self.density(z),
                    ..a
                }
            }
        }
    }
}
