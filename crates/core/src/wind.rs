//! Wind field models.
//!
//! Meteorological reports give the direction the wind blows *from*, clockwise
//! from true north. Internally a wind is the ENU velocity the air moves
//! *towards*; [`wind_vector`] converts. Mixing the two up is the classic
//! airdrop bug that lands a load on the wrong side of the drop zone.

use crate::vec3::Vec3;
use serde::{Deserialize, Serialize};

/// ENU velocity of a wind of `speed` blowing *from* `from_deg`.
///
/// A 10 m/s wind from 270 deg (a westerly) pushes things east: `(10, 0, 0)`.
pub fn wind_vector(speed: f64, from_deg: f64) -> Vec3 {
    let th = from_deg.to_radians();
    Vec3::new(-speed * th.sin(), -speed * th.cos(), 0.0)
}

/// Unit ENU vector along a track measured clockwise from north.
pub fn heading_vector(track_deg: f64) -> Vec3 {
    let th = track_deg.to_radians();
    Vec3::new(th.sin(), th.cos(), 0.0)
}

/// One row of a wind sounding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindLayer {
    /// Altitude MSL [m]
    pub altitude: f64,
    /// Speed [m/s]
    pub speed: f64,
    /// Direction the wind blows from [deg true]
    pub from_deg: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Wind {
    #[default]
    None,
    /// Same wind at every altitude.
    Uniform { speed: f64, from_deg: f64 },
    /// Boundary-layer shear `speed(h) = speed_ref (h / z_ref)^alpha`, with
    /// `h` the height above `ground_level` (floored at 1 m). `alpha = 1/7`
    /// is the textbook open-terrain value.
    PowerLaw {
        speed_ref: f64,
        from_deg: f64,
        z_ref: f64,
        alpha: f64,
        ground_level: f64,
    },
    /// Sounding interpolated linearly (by components) in altitude; the end
    /// values are held outside the table.
    Layered { layers: Vec<WindLayer> },
}

impl Wind {
    /// Check the model is well formed (sorted, finite layers, ...).
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Wind::Layered { layers } => {
                if layers.is_empty() {
                    return Err("layered wind needs at least one layer".into());
                }
                if layers.windows(2).any(|w| w[1].altitude <= w[0].altitude) {
                    return Err("wind layers must have strictly increasing altitude".into());
                }
                Ok(())
            }
            Wind::PowerLaw { z_ref, .. } if *z_ref <= 0.0 => Err("z_ref must be positive".into()),
            _ => Ok(()),
        }
    }

    /// Air velocity at `pos`.
    pub fn velocity(&self, pos: Vec3) -> Vec3 {
        match self {
            Wind::None => Vec3::ZERO,
            Wind::Uniform { speed, from_deg } => wind_vector(*speed, *from_deg),
            Wind::PowerLaw {
                speed_ref,
                from_deg,
                z_ref,
                alpha,
                ground_level,
            } => {
                let h = (pos.z - ground_level).max(1.0);
                wind_vector(speed_ref * (h / z_ref).powf(*alpha), *from_deg)
            }
            Wind::Layered { layers } => layered(layers, pos.z),
        }
    }
}

fn layered(layers: &[WindLayer], z: f64) -> Vec3 {
    let first = layers[0];
    let last = layers[layers.len() - 1];
    if z <= first.altitude {
        return wind_vector(first.speed, first.from_deg);
    }
    if z >= last.altitude {
        return wind_vector(last.speed, last.from_deg);
    }
    // index of the first layer strictly above z
    let i = layers.partition_point(|l| l.altitude <= z);
    let (a, b) = (layers[i - 1], layers[i]);
    let w = (z - a.altitude) / (b.altitude - a.altitude);
    wind_vector(a.speed, a.from_deg) * (1.0 - w) + wind_vector(b.speed, b.from_deg) * w
}
