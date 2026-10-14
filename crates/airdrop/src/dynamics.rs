//! Equations of motion for a point-mass payload.
//!
//! Newton's second law with gravity and quadratic drag on the *air-relative*
//! velocity `v_rel = v - w(r)`:
//!
//! ```text
//! dr/dt = v
//! dv/dt = g - (rho(z) CdA(t) / (2 m)) |v_rel| v_rel,     g = (0, 0, -g0)
//! ```
//!
//! `CdA(t)` is the package's own drag area plus whatever fraction of the
//! parachute has inflated by time `t` after release.

use crate::atmosphere::{Atmosphere, G0};
use crate::vec3::{State, Vec3};
use crate::wind::Wind;
use serde::{Deserialize, Serialize};

/// A right-hand side `f(t, y)` for an ODE `dy/dt = f(t, y)`.
pub trait OdeSystem {
    fn deriv(&self, t: f64, y: &State) -> State;
}

/// Static-line parachute with a power-law inflation model.
///
/// The canopy starts filling `deploy_delay` s after release and its drag
/// area grows as `cd_area * (tau / inflation_time)^fill_exponent` until fully
/// open. `inflation_time = 0` opens it instantly.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parachute {
    pub cd_area: f64,
    pub deploy_delay: f64,
    pub inflation_time: f64,
    pub fill_exponent: f64,
}

impl Parachute {
    pub fn fill_fraction(&self, t: f64) -> f64 {
        let tau = t - self.deploy_delay;
        if tau <= 0.0 {
            0.0
        } else if self.inflation_time <= 0.0 || tau >= self.inflation_time {
            1.0
        } else {
            (tau / self.inflation_time).powf(self.fill_exponent)
        }
    }

    /// Time after release at which the canopy is fully open.
    pub fn full_open_time(&self) -> f64 {
        self.deploy_delay + self.inflation_time.max(0.0)
    }
}

/// The dropped object: a supply bundle, or any generic projectile.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    /// kg
    pub mass: f64,
    /// Drag coefficient times reference area of the bare package [m^2].
    pub cd_area: f64,
    pub parachute: Option<Parachute>,
}

impl Payload {
    pub fn validate(&self) -> Result<(), String> {
        if !(self.mass > 0.0) {
            return Err("mass must be positive".into());
        }
        if self.cd_area < 0.0 {
            return Err("drag area must be non-negative".into());
        }
        if let Some(p) = self.parachute {
            if p.cd_area < 0.0
                || p.deploy_delay < 0.0
                || p.inflation_time < 0.0
                || p.fill_exponent <= 0.0
            {
                return Err(
                    "parachute parameters must be non-negative (fill exponent positive)".into(),
                );
            }
        }
        Ok(())
    }

    pub fn drag_area(&self, t: f64) -> f64 {
        self.cd_area
            + self
                .parachute
                .map_or(0.0, |p| p.cd_area * p.fill_fraction(t))
    }

    pub fn fill_fraction(&self, t: f64) -> f64 {
        self.parachute.map_or(0.0, |p| p.fill_fraction(t))
    }
}

/// Everything about the world the payload falls through.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    pub atmosphere: Atmosphere,
    pub wind: Wind,
}

/// The ODE right-hand side. `drag_scale` and `wind_offset` perturb a single
/// Monte Carlo sample without copying the environment.
#[derive(Clone, Copy, Debug)]
pub struct Model<'a> {
    pub payload: &'a Payload,
    pub env: &'a Environment,
    pub gravity: f64,
    pub drag_scale: f64,
    pub wind_offset: Vec3,
}

impl<'a> Model<'a> {
    pub fn new(payload: &'a Payload, env: &'a Environment) -> Self {
        Self {
            payload,
            env,
            gravity: G0,
            drag_scale: 1.0,
            wind_offset: Vec3::ZERO,
        }
    }

    pub fn wind_at(&self, pos: Vec3) -> Vec3 {
        self.env.wind.velocity(pos) + self.wind_offset
    }
}

impl OdeSystem for Model<'_> {
    fn deriv(&self, t: f64, y: &State) -> State {
        let v_rel = y.vel - self.wind_at(y.pos);
        let rho = self.env.atmosphere.density(y.pos.z);
        let k = rho * self.payload.drag_area(t) * self.drag_scale / (2.0 * self.payload.mass);
        let accel = v_rel * (-k * v_rel.norm()) + Vec3::new(0.0, 0.0, -self.gravity);
        State::new(y.vel, accel)
    }
}

/// Steady descent speed where drag balances weight: `sqrt(2 m g / (rho CdA))`.
pub fn terminal_velocity(mass: f64, cd_area: f64, rho: f64, gravity: f64) -> f64 {
    (2.0 * mass * gravity / (rho * cd_area)).sqrt()
}
