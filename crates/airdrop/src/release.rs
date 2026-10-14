//! Release-point solver (a Computed Air Release Point, "CARP").
//!
//! Given the aircraft's height, ground speed and track, the wind, and a target
//! on the ground: where must the package leave the aircraft so it lands on
//! the target?
//!
//! * [`solve_release_point`] solves the full 2-D problem `landing(r) = target`
//!   with damped Newton. The aircraft then flies a track through `r`.
//! * [`solve_release_on_track`] fixes the aircraft's track line (through the
//!   target) and solves only *when* to release, a 1-D problem handled with
//!   Brent's method. Crosswind drift then shows up as a cross-track miss that
//!   timing alone cannot remove.
//!
//! When the wind depends only on altitude and the ground is flat, landing
//! minus release is the same for every release point, so Newton converges in
//! one step. Terrain or position-dependent wind make the problem genuinely
//! nonlinear, which is why a real root finder is used.

use crate::dynamics::OdeSystem;
use crate::integrate::{simulate, Method, Trajectory};
use crate::roots::{brent, expand_bracket, newton_2d, RootError};
use crate::terrain::Terrain;
use crate::vec3::{State, Vec3};
use crate::wind::heading_vector;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aircraft {
    /// Release height above the target's ground elevation [m]. The aircraft
    /// flies level at `target elevation + height_agl` MSL.
    pub height_agl: f64,
    /// [m/s]
    pub ground_speed: f64,
    /// Ground track, degrees clockwise from north.
    pub track_deg: f64,
}

impl Aircraft {
    pub fn velocity(&self) -> Vec3 {
        heading_vector(self.track_deg) * self.ground_speed
    }

    /// Unit vector to the right of the track.
    pub fn right(&self) -> Vec3 {
        let u = heading_vector(self.track_deg);
        Vec3::new(u.y, -u.x, 0.0)
    }
}

/// Inputs shared by every drop simulation.
#[derive(Clone, Copy, Debug)]
pub struct DropProblem<'a, S: OdeSystem + ?Sized> {
    pub model: &'a S,
    pub terrain: &'a Terrain,
    pub aircraft: Aircraft,
    pub target: [f64; 2],
    pub method: Method,
    pub dt: f64,
    pub t_max: f64,
}

impl<S: OdeSystem + ?Sized> DropProblem<'_, S> {
    pub fn release_altitude(&self) -> f64 {
        self.terrain.elevation(self.target[0], self.target[1]) + self.aircraft.height_agl
    }

    pub fn release_state(&self, release: [f64; 2]) -> State {
        State::new(
            Vec3::new(release[0], release[1], self.release_altitude()),
            self.aircraft.velocity(),
        )
    }

    pub fn fly(&self, release: [f64; 2], record: bool) -> Trajectory {
        simulate(
            self.model,
            self.method,
            self.release_state(release),
            self.dt,
            self.t_max,
            self.terrain,
            record,
        )
    }

    /// Landing point, or NaN if the drop never reached the ground.
    fn landing_xy(&self, release: [f64; 2]) -> [f64; 2] {
        let traj = self.fly(release, false);
        if traj.landed {
            traj.impact_state().pos.xy()
        } else {
            [f64::NAN; 2]
        }
    }

    /// Fly from `release` and insist on a clean landing.
    fn fly_checked(&self, release: [f64; 2]) -> Result<Trajectory, ReleaseError> {
        let traj = self.fly(release, false);
        if traj.diverged {
            Err(ReleaseError::Diverged {
                method: self.method,
                dt: self.dt,
                t: traj.impact_time(),
            })
        } else if !traj.landed {
            Err(ReleaseError::NoImpact { t_max: self.t_max })
        } else {
            Ok(traj)
        }
    }
}

/// Why a release point could not be computed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReleaseError {
    /// The integration blew up: `dt` is beyond the method's stability limit.
    Diverged {
        method: Method,
        dt: f64,
        t: f64,
    },
    /// The package was still airborne at `t_max`.
    NoImpact {
        t_max: f64,
    },
    Root(RootError),
}

impl std::fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseError::Diverged { method, dt, t } => write!(
                f,
                "the {} integration diverged {t:.1} s after release: a step of {dt} s is beyond its stability \
                 limit (drag rises sharply while the canopy opens). Reduce the step size.",
                method.name().to_uppercase()
            ),
            ReleaseError::NoImpact { t_max } => {
                write!(f, "the package had not reached the ground after {t_max} s of simulated time")
            }
            ReleaseError::Root(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ReleaseError {}

impl From<RootError> for ReleaseError {
    fn from(e: RootError) -> Self {
        ReleaseError::Root(e)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReleaseSolution {
    pub solver: String,
    pub converged: bool,
    pub iterations: usize,
    pub release_point: [f64; 2],
    pub release_altitude: f64,
    pub landing_point: Vec3,
    /// Horizontal distance from landing point to target [m].
    pub miss_distance: f64,
    /// Part of the miss perpendicular to the track (right positive) [m].
    pub cross_track_miss: f64,
    pub time_of_fall: f64,
    pub impact_speed: f64,
    pub impact_vertical_speed: f64,
    /// Landing minus release, along track (forward throw + drift) [m].
    pub along_track_travel: f64,
    /// Landing minus release, across track, right positive [m].
    pub cross_track_drift: f64,
    /// Distance from release to target measured along the track [m].
    pub release_distance_before_target: f64,
    /// Seconds before the aircraft passes abeam the target that the release happens.
    pub release_lead_time: f64,
}

fn describe<S: OdeSystem + ?Sized>(
    p: &DropProblem<'_, S>,
    solver: &str,
    release: [f64; 2],
    iterations: usize,
    converged: bool,
) -> Result<ReleaseSolution, ReleaseError> {
    let traj = p.fly_checked(release)?;
    let hit = traj.impact_state();
    let u = heading_vector(p.aircraft.track_deg);
    let right = p.aircraft.right();
    let rel = Vec3::new(release[0], release[1], 0.0);
    let tgt = Vec3::new(p.target[0], p.target[1], 0.0);
    let land = Vec3::new(hit.pos.x, hit.pos.y, 0.0);
    let travel = land - rel;
    let miss = land - tgt;
    let before = (tgt - rel).dot(u);
    Ok(ReleaseSolution {
        solver: solver.into(),
        converged,
        iterations,
        release_point: release,
        release_altitude: p.release_altitude(),
        landing_point: hit.pos,
        miss_distance: miss.norm(),
        cross_track_miss: miss.dot(right),
        time_of_fall: traj.impact_time(),
        impact_speed: hit.vel.norm(),
        impact_vertical_speed: -hit.vel.z,
        along_track_travel: travel.dot(u),
        cross_track_drift: travel.dot(right),
        release_distance_before_target: before,
        release_lead_time: if p.aircraft.ground_speed > 0.0 {
            before / p.aircraft.ground_speed
        } else {
            0.0
        },
    })
}

/// Solve `landing(release) = target` in 2-D. `tol` is the allowed miss [m].
pub fn solve_release_point<S: OdeSystem + ?Sized>(
    p: &DropProblem<'_, S>,
    tol: f64,
) -> Result<ReleaseSolution, ReleaseError> {
    // First guess: drop over the target and shift back by the observed travel.
    let probe = p.fly_checked(p.target)?.impact_state().pos.xy();
    let guess = [2.0 * p.target[0] - probe[0], 2.0 * p.target[1] - probe[1]];
    let root = newton_2d(
        |r| {
            let l = p.landing_xy(r);
            [l[0] - p.target[0], l[1] - p.target[1]]
        },
        guess,
        1.0,
        tol,
        30,
    );
    describe(p, "newton-2d", root.x, root.iterations, root.converged)
}

/// Fix the track line through the target and solve for the release instant
/// with Brent's method.
pub fn solve_release_on_track<S: OdeSystem + ?Sized>(
    p: &DropProblem<'_, S>,
    tol: f64,
) -> Result<ReleaseSolution, ReleaseError> {
    p.fly_checked(p.target)?;
    let u = heading_vector(p.aircraft.track_deg);
    let point = |s: f64| [p.target[0] + s * u.x, p.target[1] + s * u.y];
    let along_miss = |s: f64| {
        let l = p.landing_xy(point(s));
        (l[0] - p.target[0]) * u.x + (l[1] - p.target[1]) * u.y
    };
    let probe = along_miss(0.0);
    let half = probe.abs().max(50.0) * 0.25;
    let (a, b) = expand_bracket(along_miss, -probe - half, -probe + half, 40)?;
    let root = brent(along_miss, a, b, tol, 100)?;
    describe(p, "brent-on-track", point(root.x), root.iterations, true)
}
