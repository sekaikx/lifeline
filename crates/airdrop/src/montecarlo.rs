//! Monte Carlo landing dispersion.
//!
//! Each sample perturbs the drop with independent Gaussian errors:
//!
//! * a constant wind error added to the forecast profile (east and north),
//! * a release-timing error (the "green light" is late or early),
//! * a navigation error in the aircraft's position at release,
//! * a fractional error in drag area (canopy and rigging variation).
//!
//! Samples run in parallel with rayon. Each sample owns an RNG stream derived
//! from `(seed, index)`, so results are identical however threads schedule.

use crate::dynamics::{Environment, Model, Payload};
use crate::integrate::{simulate, Method};
use crate::release::Aircraft;
use crate::stats::{covariance2, mean2, median, Ellipse, Rng};
use crate::terrain::Terrain;
use crate::vec3::{State, Vec3};
use crate::wind::heading_vector;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Uncertainty {
    /// 1-sigma wind error per horizontal component [m/s].
    pub wind_sigma: f64,
    /// 1-sigma release timing error [s].
    pub release_time_sigma: f64,
    /// 1-sigma aircraft position error per horizontal axis [m].
    pub nav_sigma: f64,
    /// 1-sigma fractional drag-area error (0.05 = 5 %).
    pub drag_sigma: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct MonteCarloSetup<'a> {
    pub payload: &'a Payload,
    pub env: &'a Environment,
    pub terrain: &'a Terrain,
    pub aircraft: Aircraft,
    pub release_point: [f64; 2],
    pub target: [f64; 2],
    pub method: Method,
    pub dt: f64,
    pub t_max: f64,
    pub uncertainty: Uncertainty,
    pub samples: usize,
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub impacts: Vec<[f64; 2]>,
    pub times_of_fall: Vec<f64>,
    pub landed_fraction: f64,
    /// Mean point of impact.
    pub mpi: [f64; 2],
    pub covariance: [[f64; 2]; 2],
    /// 50 % and 95 % confidence ellipses about the MPI.
    pub ellipses: Vec<Ellipse>,
    /// Circular error probable: median miss distance from the target [m].
    pub cep: f64,
    /// Median miss distance from the MPI (pure dispersion, no bias) [m].
    pub cep_mpi: f64,
    /// Distance from MPI to target (systematic bias) [m].
    pub bias: f64,
}

pub fn run(setup: &MonteCarloSetup<'_>) -> MonteCarloResult {
    let u = heading_vector(setup.aircraft.track_deg);
    let z0 = setup.terrain.elevation(setup.target[0], setup.target[1]) + setup.aircraft.height_agl;
    let unc = setup.uncertainty;

    let results: Vec<Option<([f64; 2], f64)>> = (0..setup.samples)
        .into_par_iter()
        .map(|i| {
            let mut rng = Rng::stream(setup.seed, i as u64);
            let wind_offset = Vec3::new(
                unc.wind_sigma * rng.normal(),
                unc.wind_sigma * rng.normal(),
                0.0,
            );
            let timing = unc.release_time_sigma * rng.normal() * setup.aircraft.ground_speed;
            let nav = (unc.nav_sigma * rng.normal(), unc.nav_sigma * rng.normal());
            let drag_scale = (1.0 + unc.drag_sigma * rng.normal()).max(0.05);

            let model = Model {
                drag_scale,
                wind_offset,
                ..Model::new(setup.payload, setup.env)
            };
            let pos = Vec3::new(
                setup.release_point[0] + u.x * timing + nav.0,
                setup.release_point[1] + u.y * timing + nav.1,
                z0,
            );
            let y0 = State::new(pos, setup.aircraft.velocity());
            let traj = simulate(
                &model,
                setup.method,
                y0,
                setup.dt,
                setup.t_max,
                setup.terrain,
                false,
            );
            traj.landed
                .then(|| (traj.impact_state().pos.xy(), traj.impact_time()))
        })
        .collect();

    let landed: Vec<_> = results.into_iter().flatten().collect();
    let impacts: Vec<[f64; 2]> = landed.iter().map(|(p, _)| *p).collect();
    let times_of_fall = landed.iter().map(|(_, t)| *t).collect();
    let mpi = mean2(&impacts);
    let covariance = covariance2(&impacts);

    let ellipses = [0.5, 0.95]
        .iter()
        .map(|&p| {
            let mut e = Ellipse::from_covariance(mpi, covariance, p);
            let inside = impacts.iter().filter(|q| e.contains(**q)).count();
            e.observed_fraction = inside as f64 / impacts.len().max(1) as f64;
            e
        })
        .collect();

    let dist = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let mut miss: Vec<f64> = impacts.iter().map(|p| dist(*p, setup.target)).collect();
    let mut spread: Vec<f64> = impacts.iter().map(|p| dist(*p, mpi)).collect();

    MonteCarloResult {
        landed_fraction: impacts.len() as f64 / setup.samples.max(1) as f64,
        cep: median(&mut miss),
        cep_mpi: median(&mut spread),
        bias: dist(mpi, setup.target),
        impacts,
        times_of_fall,
        mpi,
        covariance,
        ellipses,
    }
}

/// Fraction of impacts within `radius` of `center`.
pub fn fraction_within(impacts: &[[f64; 2]], center: [f64; 2], radius: f64) -> f64 {
    let n = impacts
        .iter()
        .filter(|p| (p[0] - center[0]).hypot(p[1] - center[1]) <= radius)
        .count();
    n as f64 / impacts.len().max(1) as f64
}
