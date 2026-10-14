//! Serializable scenario and the high-level reports the app displays.
//!
//! Everything the GUI and CLI do goes through these functions, so the desktop
//! app, the command line and the tests all exercise the same code.

use crate::analytic::vacuum_state;
use crate::atmosphere::{Atmosphere, G0};
use crate::convergence::{halving_steps, reference_study, vertical_drag_study, ConvergenceStudy};
use crate::dynamics::{Environment, Model, Parachute, Payload};
use crate::integrate::{integrate_to, simulate, Method, Trajectory};
use crate::montecarlo::{self, fraction_within, MonteCarloResult, MonteCarloSetup, Uncertainty};
use crate::release::{
    solve_release_on_track, solve_release_point, Aircraft, DropProblem, ReleaseSolution,
};
use crate::terrain::Terrain;
use crate::vec3::{State, Vec3};
use crate::wind::{Wind, WindLayer};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntegratorSettings {
    pub method: Method,
    /// Step size [s]
    pub dt: f64,
    /// Give up after this much simulated time [s]
    pub t_max: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonteCarloSettings {
    pub samples: usize,
    pub seed: u64,
    pub uncertainty: Uncertainty,
}

/// A complete, self-contained drop scenario (what the GUI edits and saves).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub payload: Payload,
    pub environment: Environment,
    pub terrain: Terrain,
    pub aircraft: Aircraft,
    /// Desired point of impact (east, north) [m].
    pub target: [f64; 2],
    /// Radius of the usable drop zone around the target [m].
    pub drop_zone_radius: f64,
    pub integrator: IntegratorSettings,
    pub monte_carlo: MonteCarloSettings,
}

impl Default for Scenario {
    /// A generic 500 kg humanitarian supply bundle under a cargo parachute,
    /// dropped from 400 m above the drop zone in a veering, strengthening wind.
    fn default() -> Self {
        Scenario {
            name: "Humanitarian supply bundle".into(),
            payload: Payload {
                mass: 500.0,
                cd_area: 1.2,
                parachute: Some(Parachute {
                    cd_area: 120.0,
                    deploy_delay: 1.5,
                    inflation_time: 3.0,
                    fill_exponent: 2.0,
                }),
            },
            environment: Environment {
                atmosphere: Atmosphere::Isa {
                    temperature_offset: 0.0,
                },
                wind: Wind::Layered {
                    layers: vec![
                        WindLayer {
                            altitude: 0.0,
                            speed: 3.0,
                            from_deg: 240.0,
                        },
                        WindLayer {
                            altitude: 150.0,
                            speed: 5.0,
                            from_deg: 250.0,
                        },
                        WindLayer {
                            altitude: 400.0,
                            speed: 7.0,
                            from_deg: 260.0,
                        },
                        WindLayer {
                            altitude: 1000.0,
                            speed: 10.0,
                            from_deg: 270.0,
                        },
                    ],
                },
            },
            terrain: Terrain::Flat { elevation: 0.0 },
            aircraft: Aircraft {
                height_agl: 400.0,
                ground_speed: 67.0,
                track_deg: 90.0,
            },
            target: [0.0, 0.0],
            drop_zone_radius: 150.0,
            integrator: IntegratorSettings {
                method: Method::Rk4,
                dt: 0.05,
                t_max: 900.0,
            },
            monte_carlo: MonteCarloSettings {
                samples: 2000,
                seed: 42,
                uncertainty: Uncertainty {
                    wind_sigma: 1.0,
                    release_time_sigma: 0.5,
                    nav_sigma: 10.0,
                    drag_sigma: 0.05,
                },
            },
        }
    }
}

impl Scenario {
    pub fn validate(&self) -> Result<(), String> {
        self.payload.validate()?;
        self.environment.wind.validate()?;
        self.terrain.validate()?;
        let i = &self.integrator;
        if !(i.dt > 0.0 && i.dt <= 5.0) {
            return Err("step size must be in (0, 5] s".into());
        }
        if !(i.t_max > 0.0) || i.t_max / i.dt > 5.0e6 {
            return Err("t_max must be positive and need at most 5 million steps".into());
        }
        if !(self.aircraft.height_agl > 0.0) {
            return Err("release height must be above the ground".into());
        }
        if self.aircraft.ground_speed < 0.0 {
            return Err("ground speed must be non-negative".into());
        }
        if self.monte_carlo.samples == 0 || self.monte_carlo.samples > 200_000 {
            return Err("Monte Carlo samples must be between 1 and 200000".into());
        }
        if let Atmosphere::Constant { rho } = self.environment.atmosphere {
            if rho < 0.0 {
                return Err("density must be non-negative".into());
            }
        }
        Ok(())
    }

    pub fn model(&self) -> Model<'_> {
        Model::new(&self.payload, &self.environment)
    }

    pub fn problem<'a>(&'a self, model: &'a Model<'a>) -> DropProblem<'a, Model<'a>> {
        DropProblem {
            model,
            terrain: &self.terrain,
            aircraft: self.aircraft,
            target: self.target,
            method: self.integrator.method,
            dt: self.integrator.dt,
            t_max: self.integrator.t_max,
        }
    }
}

// ---------------------------------------------------------------- drop report

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathPoint {
    pub t: f64,
    pub pos: Vec3,
    pub vel: Vec3,
    pub wind: Vec3,
    pub density: f64,
    /// Parachute fill fraction 0..1
    pub fill: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindSample {
    pub altitude: f64,
    pub wind: Vec3,
}

/// Terrain heights sampled on a grid for 3-D rendering.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Heightmap {
    pub x0: f64,
    pub y0: f64,
    pub dx: f64,
    pub dy: f64,
    pub nx: usize,
    pub ny: usize,
    pub heights: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DropReport {
    pub release: ReleaseSolution,
    /// Same drop, but the aircraft keeps its track through the target and only
    /// the release instant is solved (Brent). `None` if no bracket exists.
    pub on_track: Option<ReleaseSolution>,
    pub path: Vec<PathPoint>,
    pub chute_deploy_time: Option<f64>,
    pub chute_full_time: Option<f64>,
    pub wind_profile: Vec<WindSample>,
    pub heightmap: Heightmap,
    pub compute_ms: f64,
}

fn path_points(model: &Model<'_>, traj: &Trajectory, max_points: usize) -> Vec<PathPoint> {
    let n = traj.samples.len();
    let stride = n.div_ceil(max_points.max(2)).max(1);
    let mut idx: Vec<usize> = (0..n).step_by(stride).collect();
    if idx.last() != Some(&(n - 1)) {
        idx.push(n - 1);
    }
    idx.into_iter()
        .map(|i| {
            let s = traj.samples[i];
            PathPoint {
                t: s.t,
                pos: s.state.pos,
                vel: s.state.vel,
                wind: model.wind_at(s.state.pos),
                density: model.env.atmosphere.density(s.state.pos.z),
                fill: model.payload.fill_fraction(s.t),
            }
        })
        .collect()
}

pub fn heightmap(terrain: &Terrain, center: [f64; 2], half_size: f64, n: usize) -> Heightmap {
    let n = n.max(2);
    let d = 2.0 * half_size / (n - 1) as f64;
    let (x0, y0) = (center[0] - half_size, center[1] - half_size);
    let mut heights = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            heights.push(terrain.elevation(x0 + i as f64 * d, y0 + j as f64 * d));
        }
    }
    Heightmap {
        x0,
        y0,
        dx: d,
        dy: d,
        nx: n,
        ny: n,
        heights,
    }
}

pub fn drop_report(s: &Scenario) -> Result<DropReport, String> {
    s.validate()?;
    let start = Instant::now();
    let model = s.model();
    let problem = s.problem(&model);
    let release = solve_release_point(&problem, 0.01).map_err(|e| e.to_string())?;
    let on_track = solve_release_on_track(&problem, 0.01).ok();
    let traj = problem.fly(release.release_point, true);
    let path = path_points(&model, &traj, 2000);

    let top = release.release_altitude;
    let bottom = s.terrain.elevation(s.target[0], s.target[1]);
    let wind_profile = (0..=24)
        .map(|i| {
            let z = bottom + (top - bottom) * i as f64 / 24.0;
            WindSample {
                altitude: z,
                wind: model.wind_at(Vec3::new(s.target[0], s.target[1], z)),
            }
        })
        .collect();

    let span = [
        (release.release_point[0] - s.target[0]).abs(),
        (release.release_point[1] - s.target[1]).abs(),
        s.drop_zone_radius,
        300.0,
    ]
    .into_iter()
    .fold(0.0, f64::max)
        * 1.6;

    Ok(DropReport {
        chute_deploy_time: s.payload.parachute.map(|p| p.deploy_delay),
        chute_full_time: s.payload.parachute.map(|p| p.full_open_time()),
        release,
        on_track,
        path,
        wind_profile,
        heightmap: heightmap(&s.terrain, s.target, span, 81),
        compute_ms: start.elapsed().as_secs_f64() * 1e3,
    })
}

// ---------------------------------------------------------------- numerics

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MethodPath {
    /// "euler", "rk4" or "exact"/"reference"
    pub label: String,
    /// (downrange distance, height) pairs [m]
    pub points: Vec<[f64; 2]>,
    /// Impact downrange distance [m]
    pub impact_range: f64,
    /// Error of the impact point against the exact/reference solution [m]
    pub impact_error: f64,
    pub evaluations: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MethodComparison {
    pub id: String,
    pub title: String,
    pub dt: f64,
    pub paths: Vec<MethodPath>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NumericsReport {
    pub comparisons: Vec<MethodComparison>,
    pub studies: Vec<ConvergenceStudy>,
    pub compute_ms: f64,
}

/// A thrown projectile used for the method comparisons: 45 deg, 120 m/s,
/// from 200 m. Returns Euler, RK4 and truth paths.
fn compare(
    id: &str,
    title: &str,
    model: &Model<'_>,
    dt: f64,
    truth_label: &str,
    truth: Trajectory,
) -> MethodComparison {
    let terrain = Terrain::default();
    let y0 = truth.samples[0].state;
    let truth_impact = truth.impact_state().pos;
    let to_points = |t: &Trajectory| -> Vec<[f64; 2]> {
        let stride = t.samples.len().div_ceil(600).max(1);
        let mut v: Vec<[f64; 2]> = t
            .samples
            .iter()
            .step_by(stride)
            .map(|s| [s.state.pos.x, s.state.pos.z])
            .collect();
        let last = t.impact_state().pos;
        v.push([last.x, last.z]);
        v
    };
    let mut paths: Vec<MethodPath> = Method::ALL
        .iter()
        .map(|&m| {
            let t = simulate(model, m, y0, dt, 600.0, &terrain, true);
            let hit = t.impact_state().pos;
            MethodPath {
                label: m.name().into(),
                points: to_points(&t),
                impact_range: hit.x,
                impact_error: (hit - truth_impact).norm(),
                evaluations: t.evaluations,
            }
        })
        .collect();
    paths.push(MethodPath {
        label: truth_label.into(),
        points: to_points(&truth),
        impact_range: truth_impact.x,
        impact_error: 0.0,
        evaluations: truth.evaluations,
    });
    MethodComparison {
        id: id.into(),
        title: title.into(),
        dt,
        paths,
    }
}

pub fn numerics_report(s: &Scenario, dt: f64) -> Result<NumericsReport, String> {
    s.validate()?;
    if !(dt > 0.0 && dt <= 5.0) {
        return Err("comparison step must be in (0, 5] s".into());
    }
    let start = Instant::now();
    let y0 = State::new(
        Vec3::new(0.0, 0.0, 200.0),
        Vec3::new(120.0 * 0.5f64.sqrt(), 0.0, 120.0 * 0.5f64.sqrt()),
    );

    // 1. vacuum: exact parabola available
    let vac_env = Environment {
        atmosphere: Atmosphere::Vacuum,
        wind: Wind::None,
    };
    let bare = Payload {
        parachute: None,
        ..s.payload
    };
    let vac_model = Model::new(&bare, &vac_env);
    let t_hit = crate::analytic::vacuum_impact_time(y0.pos.z, y0.vel.z, G0);
    let n = 400;
    let exact = Trajectory {
        method: Method::Rk4,
        dt: t_hit / n as f64,
        samples: (0..=n)
            .map(|i| {
                let t = t_hit * i as f64 / n as f64;
                crate::integrate::Sample {
                    t,
                    state: vacuum_state(t, &y0, G0),
                }
            })
            .collect(),
        landed: true,
        diverged: false,
        evaluations: 0,
    };
    let vacuum = compare(
        "vacuum",
        "No drag (vs exact parabola)",
        &vac_model,
        dt,
        "exact",
        exact,
    );

    // 2. the scenario's air and wind acting on a dense projectile
    let shot = Payload {
        mass: 20.0,
        cd_area: 0.02,
        parachute: None,
    };
    let shot_model = Model::new(&shot, &s.environment);
    let reference = simulate(
        &shot_model,
        Method::Rk4,
        y0,
        dt / 64.0,
        600.0,
        &Terrain::default(),
        true,
    );
    let drag = compare(
        "drag",
        "Drag + ISA + wind (vs RK4 at dt/64)",
        &shot_model,
        dt,
        "reference",
        reference,
    );

    // 3. convergence studies
    let dts = halving_steps(1.0, 7);
    let smooth_env = Environment {
        atmosphere: Atmosphere::Isa {
            temperature_offset: 0.0,
        },
        wind: Wind::PowerLaw {
            speed_ref: 8.0,
            from_deg: 250.0,
            z_ref: 10.0,
            alpha: 1.0 / 7.0,
            ground_level: 0.0,
        },
    };
    let smooth_model = Model::new(&shot, &smooth_env);
    let full = reference_study(
        "full-smooth",
        "Projectile with drag, ISA and shear wind",
        &smooth_model,
        y0,
        10.0,
        &dts,
    );

    // The scenario's own drop. The canopy opening is stiff: drag rises so fast
    // that explicit Euler is unstable for large steps (flagged as `unstable`).
    let scenario_model = s.model();
    let z0 = s.terrain.elevation(s.target[0], s.target[1]) + s.aircraft.height_agl;
    let drop_y0 = State::new(Vec3::new(0.0, 0.0, z0), s.aircraft.velocity());
    let t_end = s
        .payload
        .parachute
        .map_or(8.0, |p| p.full_open_time() + 6.0)
        .min(30.0);
    let scenario_study = reference_study(
        "scenario",
        "This scenario's drop through canopy opening",
        &scenario_model,
        drop_y0,
        t_end,
        &dts,
    );

    Ok(NumericsReport {
        comparisons: vec![vacuum, drag],
        studies: vec![vertical_drag_study(&dts), full, scenario_study],
        compute_ms: start.elapsed().as_secs_f64() * 1e3,
    })
}

// ---------------------------------------------------------------- monte carlo

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonteCarloReport {
    pub release: ReleaseSolution,
    pub result: MonteCarloResult,
    /// Fraction of loads landing inside the drop zone circle.
    pub inside_drop_zone: f64,
    pub compute_ms: f64,
}

pub fn monte_carlo_report(s: &Scenario) -> Result<MonteCarloReport, String> {
    s.validate()?;
    let start = Instant::now();
    let model = s.model();
    let release = solve_release_point(&s.problem(&model), 0.01).map_err(|e| e.to_string())?;
    let result = montecarlo::run(&MonteCarloSetup {
        payload: &s.payload,
        env: &s.environment,
        terrain: &s.terrain,
        aircraft: s.aircraft,
        release_point: release.release_point,
        target: s.target,
        method: s.integrator.method,
        dt: s.integrator.dt,
        t_max: s.integrator.t_max,
        uncertainty: s.monte_carlo.uncertainty,
        samples: s.monte_carlo.samples,
        seed: s.monte_carlo.seed,
    });
    Ok(MonteCarloReport {
        inside_drop_zone: fraction_within(&result.impacts, s.target, s.drop_zone_radius),
        release,
        result,
        compute_ms: start.elapsed().as_secs_f64() * 1e3,
    })
}

// ---------------------------------------------------------------- atmosphere

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileRow {
    pub altitude: f64,
    pub temperature: f64,
    pub pressure: f64,
    pub density: f64,
    pub wind_east: f64,
    pub wind_north: f64,
}

pub fn atmosphere_profile(s: &Scenario, z_max: f64, n: usize) -> Vec<ProfileRow> {
    let n = n.clamp(2, 2000);
    (0..n)
        .map(|i| {
            let z = z_max * i as f64 / (n - 1) as f64;
            let air = s.environment.atmosphere.air(z);
            let w = s
                .environment
                .wind
                .velocity(Vec3::new(s.target[0], s.target[1], z));
            ProfileRow {
                altitude: z,
                temperature: air.temperature,
                pressure: air.pressure,
                density: air.density,
                wind_east: w.x,
                wind_north: w.y,
            }
        })
        .collect()
}

/// Positions at a fixed time, for users who want raw integration output.
pub fn integrate_fixed(s: &Scenario, t_end: f64) -> Result<Vec<crate::integrate::Sample>, String> {
    s.validate()?;
    let model = s.model();
    let z0 = s.terrain.elevation(s.target[0], s.target[1]) + s.aircraft.height_agl;
    let y0 = State::new(Vec3::new(0.0, 0.0, z0), s.aircraft.velocity());
    Ok(integrate_to(
        &model,
        s.integrator.method,
        y0,
        0.0,
        t_end,
        s.integrator.dt,
    ))
}
