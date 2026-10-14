//! Fixed-step integrators and ground-impact detection.
//!
//! * forward Euler: 1 RHS evaluation per step, global error `O(dt)`
//! * classical Runge-Kutta (RK4): 4 evaluations per step, global error `O(dt^4)`
//!
//! Impact is located inside the final step with a cubic Hermite interpolant
//! built from the state and derivative at both ends of the step. That
//! interpolant is `O(dt^4)` accurate, so finding the impact never limits RK4.

use crate::dynamics::OdeSystem;
use crate::terrain::Terrain;
use crate::vec3::State;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Euler,
    Rk4,
}

impl Method {
    pub const ALL: [Method; 2] = [Method::Euler, Method::Rk4];

    pub fn evals_per_step(self) -> u64 {
        match self {
            Method::Euler => 1,
            Method::Rk4 => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Method::Euler => "euler",
            Method::Rk4 => "rk4",
        }
    }
}

pub fn euler_step<S: OdeSystem + ?Sized>(f: &S, t: f64, y: &State, dt: f64) -> State {
    *y + f.deriv(t, y) * dt
}

pub fn rk4_step<S: OdeSystem + ?Sized>(f: &S, t: f64, y: &State, dt: f64) -> State {
    let k1 = f.deriv(t, y);
    let k2 = f.deriv(t + 0.5 * dt, &(*y + k1 * (0.5 * dt)));
    let k3 = f.deriv(t + 0.5 * dt, &(*y + k2 * (0.5 * dt)));
    let k4 = f.deriv(t + dt, &(*y + k3 * dt));
    *y + (k1 + k2 * 2.0 + k3 * 2.0 + k4) * (dt / 6.0)
}

pub fn step<S: OdeSystem + ?Sized>(f: &S, method: Method, t: f64, y: &State, dt: f64) -> State {
    match method {
        Method::Euler => euler_step(f, t, y, dt),
        Method::Rk4 => rk4_step(f, t, y, dt),
    }
}

/// One stored point of a trajectory.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub t: f64,
    pub state: State,
}

/// Integrate from `t0` to exactly `t_end`; the last step is shortened if
/// `dt` does not divide the interval. Returns every step.
pub fn integrate_to<S: OdeSystem + ?Sized>(
    f: &S,
    method: Method,
    y0: State,
    t0: f64,
    t_end: f64,
    dt: f64,
) -> Vec<Sample> {
    assert!(dt > 0.0, "dt must be positive");
    let n = ((t_end - t0) / dt - 1e-9).ceil().max(0.0) as usize;
    let mut out = Vec::with_capacity(n + 1);
    let (mut t, mut y) = (t0, y0);
    out.push(Sample { t, state: y });
    for i in 0..n {
        let h = if i + 1 == n { t_end - t } else { dt };
        y = step(f, method, t, &y, h);
        t = if i + 1 == n { t_end } else { t + h };
        out.push(Sample { t, state: y });
    }
    out
}

/// Cubic Hermite interpolation between `y0` (at 0) and `y1` (at `dt`).
pub fn hermite(y0: &State, f0: &State, y1: &State, f1: &State, dt: f64, theta: f64) -> State {
    let (t2, t3) = (theta * theta, theta * theta * theta);
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + theta;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    *y0 * h00 + *f0 * (h10 * dt) + *y1 * h01 + *f1 * (h11 * dt)
}

fn height_above(terrain: &Terrain, y: &State) -> f64 {
    y.pos.z - terrain.elevation(y.pos.x, y.pos.y)
}

/// Bisection on the Hermite interpolant: returns the step fraction and state.
fn locate_impact<S: OdeSystem + ?Sized>(
    f: &S,
    terrain: &Terrain,
    t: f64,
    y0: &State,
    y1: &State,
    dt: f64,
) -> (f64, State) {
    let f0 = f.deriv(t, y0);
    let f1 = f.deriv(t + dt, y1);
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if height_above(terrain, &hermite(y0, &f0, y1, &f1, dt, mid)) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let theta = 0.5 * (lo + hi);
    (theta, hermite(y0, &f0, y1, &f1, dt, theta))
}

/// A trajectory from release to ground impact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trajectory {
    pub method: Method,
    pub dt: f64,
    /// Every integration step; the last sample is the interpolated impact.
    pub samples: Vec<Sample>,
    pub landed: bool,
    /// The solution blew up (step size beyond the method's stability limit).
    /// The last sample is then the last sane state, not an impact.
    pub diverged: bool,
    /// RHS evaluations used (the honest measure of cost).
    pub evaluations: u64,
}

impl Trajectory {
    pub fn last(&self) -> &Sample {
        self.samples.last().expect("trajectory is never empty")
    }
    pub fn impact_time(&self) -> f64 {
        self.last().t
    }
    pub fn impact_state(&self) -> State {
        self.last().state
    }
}

/// A speed no falling body can physically reach from `y0`: with drag a body
/// never gains more than free-fall speed over its height, so three times the
/// vacuum bound (plus margin for wind) only trips when the numerics explode.
pub fn divergence_speed(y0: &State, terrain: &Terrain) -> f64 {
    let h = height_above(terrain, y0).max(0.0) + 100.0;
    3.0 * (y0.vel.norm() + (2.0 * crate::atmosphere::G0 * h).sqrt()) + 100.0
}

/// Integrate one trajectory until it reaches the ground (or `t_max`).
///
/// With `record = false` only the first and impact samples are stored, which
/// is what Monte Carlo and the root finders need. Integration stops with
/// `diverged = true` if the state becomes non-finite or impossibly fast.
pub fn simulate<S: OdeSystem + ?Sized>(
    f: &S,
    method: Method,
    y0: State,
    dt: f64,
    t_max: f64,
    terrain: &Terrain,
    record: bool,
) -> Trajectory {
    assert!(dt > 0.0, "dt must be positive");
    let mut samples = vec![Sample { t: 0.0, state: y0 }];
    let mut evaluations = 0;
    let (mut t, mut y) = (0.0, y0);
    let speed_limit = divergence_speed(&y0, terrain);
    let finish = |samples, landed, diverged, evaluations| Trajectory {
        method,
        dt,
        samples,
        landed,
        diverged,
        evaluations,
    };
    if height_above(terrain, &y) <= 0.0 {
        return finish(samples, true, false, evaluations);
    }
    while t < t_max {
        let y_next = step(f, method, t, &y, dt);
        evaluations += method.evals_per_step();
        let speed = y_next.vel.norm();
        if !(speed < speed_limit && y_next.pos.norm().is_finite()) {
            if !record {
                samples.push(Sample { t, state: y });
            }
            return finish(samples, false, true, evaluations);
        }
        if height_above(terrain, &y_next) <= 0.0 {
            let (theta, hit) = locate_impact(f, terrain, t, &y, &y_next, dt);
            evaluations += 2;
            samples.push(Sample {
                t: t + theta * dt,
                state: hit,
            });
            return finish(samples, true, false, evaluations);
        }
        y = y_next;
        t += dt;
        if record {
            samples.push(Sample { t, state: y });
        }
    }
    if !record {
        samples.push(Sample { t, state: y });
    }
    finish(samples, false, false, evaluations)
}
