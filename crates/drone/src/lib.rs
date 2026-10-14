//! # lifeline-drone
//!
//! Multirotor delivery: power from momentum theory, ground speed from the
//! wind triangle, battery feasibility, and risk-aware path planning.
//!
//! **Power.** A rotor supporting weight `W` over disk area `A` induces a
//! velocity `v_h = √(W / 2ρA)` in hover. In level forward flight at airspeed
//! `V` the induced velocity drops (Glauert): `v_i² = (−V² + √(V⁴ + 4 v_h⁴)) / 2`.
//! Total electrical power is
//!
//! ```text
//! P = k_p · (W · v_i + ½ ρ CdA V³) / η
//! ```
//!
//! with `k_p = 1.15` for blade profile losses and `η` the motor and propeller
//! efficiency.
//!
//! **Wind triangle.** Flying a track with unit vector `u` at airspeed `V_a` in
//! wind `w`, the drone crabs into the crosswind `w⊥` and its ground speed is
//! `w∥ + √(V_a² − w⊥²)`. If `|w⊥| ≥ V_a` it cannot hold the track.

use lifeline_core::atmosphere::G0;
use lifeline_core::raster::Raster;
use lifeline_core::risk::{
    line_of_sight, polyline_length, resample, segment_exposure, simplify, survival,
};
use lifeline_core::search::grid_astar;
use lifeline_core::terrain::TerrainGrid;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneSpec {
    pub empty_mass_kg: f64,
    pub payload_kg: f64,
    pub battery_wh: f64,
    /// Fraction of the battery held back as reserve
    pub reserve: f64,
    /// Total rotor disk area [m²]
    pub rotor_area_m2: f64,
    /// Body drag area [m²]
    pub cd_area_m2: f64,
    pub airspeed_ms: f64,
    /// Motor and propeller efficiency
    pub efficiency: f64,
    /// Cruise height above ground [m]
    pub cruise_agl_m: f64,
    /// Multiplier on risk-site rates (a small drone is harder to hit)
    pub detectability: f64,
    /// Battery swap and reload at the depot [min]
    pub turnaround_min: f64,
}

impl DroneSpec {
    pub fn validate(&self) -> Result<(), String> {
        let positive = [
            self.empty_mass_kg,
            self.payload_kg,
            self.battery_wh,
            self.rotor_area_m2,
            self.airspeed_ms,
            self.efficiency,
        ];
        if positive.iter().any(|v| !(*v > 0.0))
            || !(0.0..1.0).contains(&self.reserve)
            || self.efficiency > 1.0
        {
            return Err("drone: masses, battery, rotor area, airspeed and efficiency must be positive; reserve in [0, 1)".into());
        }
        Ok(())
    }

    pub fn usable_wh(&self) -> f64 {
        self.battery_wh * (1.0 - self.reserve)
    }

    /// Electrical power [W] at airspeed `v` carrying `payload` kg in air of density `rho`.
    pub fn power(&self, payload: f64, v: f64, rho: f64) -> f64 {
        let weight = (self.empty_mass_kg + payload) * G0;
        let vh2 = weight / (2.0 * rho * self.rotor_area_m2);
        let vi2 = (-v * v + (v.powi(4) + 4.0 * vh2 * vh2).sqrt()) / 2.0;
        let induced = weight * vi2.sqrt();
        let parasitic = 0.5 * rho * self.cd_area_m2 * v.powi(3);
        1.15 * (induced + parasitic) / self.efficiency
    }
}

/// Ground speed along unit direction `u` at airspeed `va` in wind `w`, or
/// `None` if the crosswind is too strong or the headwind wins.
pub fn ground_speed(va: f64, w: [f64; 2], u: [f64; 2]) -> Option<f64> {
    let along = w[0] * u[0] + w[1] * u[1];
    let cross = -w[0] * u[1] + w[1] * u[0];
    if cross.abs() >= va {
        return None;
    }
    let gs = along + (va * va - cross * cross).sqrt();
    (gs > 0.5).then_some(gs)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneRoute {
    pub points: Vec<[f64; 2]>,
    pub length_m: f64,
    pub time_out_s: f64,
    pub time_back_s: f64,
    pub energy_out_wh: f64,
    pub energy_back_wh: f64,
    /// Expected interdictions on the loaded outbound leg
    pub exposure_out: f64,
    /// Probability the cargo arrives
    pub survival_out: f64,
    /// Probability the drone gets there and back
    pub survival_round: f64,
    /// Fraction of the outbound leg with radio line of sight to the depot
    pub comms_fraction: f64,
    pub feasible: bool,
    pub reason: Option<String>,
}

/// Plans drone sorties over one scenario.
pub struct DronePlanner<'a> {
    pub terrain: &'a TerrainGrid,
    /// Exposure field at the drone's cruise height
    pub field: &'a Raster,
    pub spec: DroneSpec,
    /// Wind at cruise height (east, north) [m/s]
    pub wind: [f64; 2],
    pub rho: f64,
    /// Ground antenna height at the depot [m]
    pub antenna_m: f64,
}

impl DronePlanner<'_> {
    fn leg(&self, points: &[[f64; 2]], payload: f64) -> Option<(f64, f64, f64)> {
        let (mut t, mut e, mut x) = (0.0, 0.0, 0.0);
        for w in points.windows(2) {
            let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            if len == 0.0 {
                continue;
            }
            let u = [(w[1][0] - w[0][0]) / len, (w[1][1] - w[0][1]) / len];
            let gs = ground_speed(self.spec.airspeed_ms, self.wind, u)?;
            let dt = len / gs;
            t += dt;
            e += self.spec.power(payload, self.spec.airspeed_ms, self.rho) * dt / 3600.0;
            x += segment_exposure(self.field, w[0], w[1], dt);
        }
        Some((t, e, x))
    }

    fn comms(&self, points: &[[f64; 2]], depot: [f64; 2]) -> f64 {
        let antenna = [
            depot[0],
            depot[1],
            self.terrain.at(depot[0], depot[1]) + self.antenna_m,
        ];
        let samples = resample(points, 200.0);
        let seen = samples
            .iter()
            .filter(|p| {
                let drone = [
                    p[0],
                    p[1],
                    self.terrain.at(p[0], p[1]) + self.spec.cruise_agl_m,
                ];
                line_of_sight(self.terrain, antenna, drone, self.terrain.dx * 0.5)
            })
            .count();
        seen as f64 / samples.len().max(1) as f64
    }

    /// Plan a sortie carrying `payload` kg. `w` is the risk weight in seconds
    /// per expected loss (0 = fastest path).
    pub fn route(&self, from: [f64; 2], to: [f64; 2], payload: f64, w: f64) -> DroneRoute {
        let f = self.field;
        let cell = |p: [f64; 2]| {
            let i = ((p[0] - f.x0) / f.cell)
                .round()
                .clamp(0.0, (f.nx - 1) as f64) as usize;
            let j = ((p[1] - f.y0) / f.cell)
                .round()
                .clamp(0.0, (f.ny - 1) as f64) as usize;
            (i, j)
        };
        let max_gs = self.spec.airspeed_ms + self.wind[0].hypot(self.wind[1]);
        let goal = cell(to);
        let found = grid_astar(
            f.nx,
            f.ny,
            cell(from),
            goal,
            |a, b| {
                let (pa, pb) = (f.center(a.0, a.1), f.center(b.0, b.1));
                let len = (pb.0 - pa.0).hypot(pb.1 - pa.1);
                let gs = ground_speed(
                    self.spec.airspeed_ms,
                    self.wind,
                    [(pb.0 - pa.0) / len, (pb.1 - pa.1) / len],
                )?;
                let dt = len / gs;
                let rate = 0.5 * (f.values[f.index(a.0, a.1)] + f.values[f.index(b.0, b.1)]) as f64;
                Some(dt + w * rate * dt)
            },
            |c| {
                let p = f.center(c.0, c.1);
                let q = f.center(goal.0, goal.1);
                (p.0 - q.0).hypot(p.1 - q.1) / max_gs
            },
        );
        let infeasible = |reason: String, points: Vec<[f64; 2]>| DroneRoute {
            length_m: polyline_length(&points),
            points,
            time_out_s: 0.0,
            time_back_s: 0.0,
            energy_out_wh: 0.0,
            energy_back_wh: 0.0,
            exposure_out: 0.0,
            survival_out: 0.0,
            survival_round: 0.0,
            comms_fraction: 0.0,
            feasible: false,
            reason: Some(reason),
        };
        let Some((cells, _)) = found else {
            return infeasible("wind too strong to hold any track".into(), vec![from, to]);
        };
        let mut points: Vec<[f64; 2]> = cells
            .iter()
            .map(|c| {
                let p = f.center(c.0, c.1);
                [p.0, p.1]
            })
            .collect();
        points[0] = from;
        *points.last_mut().unwrap() = to;
        let points = simplify(&points, 0.75 * f.cell);
        let mut back = points.clone();
        back.reverse();
        let (Some(out), Some(ret)) = (self.leg(&points, payload), self.leg(&back, 0.0)) else {
            return infeasible(
                "crosswind exceeds airspeed on part of the route".into(),
                points,
            );
        };
        let need = out.1 + ret.1;
        let usable = self.spec.usable_wh();
        let (feasible, reason) = if need > usable {
            (
                false,
                Some(format!(
                    "out of range: round trip needs {need:.0} Wh, {usable:.0} Wh usable"
                )),
            )
        } else {
            (true, None)
        };
        DroneRoute {
            length_m: polyline_length(&points),
            comms_fraction: self.comms(&points, from),
            points,
            time_out_s: out.0,
            time_back_s: ret.0,
            energy_out_wh: out.1,
            energy_back_wh: ret.1,
            exposure_out: out.2,
            survival_out: survival(out.2),
            survival_round: survival(out.2 + ret.2),
            feasible,
            reason,
        }
    }
}
