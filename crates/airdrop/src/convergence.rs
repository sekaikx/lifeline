//! Convergence studies: how fast does each method's error shrink with `dt`?
//!
//! For a method of order `p`, global error behaves like `C dt^p`, so on a
//! log-log plot of error against step size the points fall on a line of slope
//! `p`. [`observed_order`] measures that slope by least squares.

use crate::analytic::vertical_drag_fall;
use crate::atmosphere::{Atmosphere, G0};
use crate::dynamics::{Environment, Model, OdeSystem, Payload};
use crate::integrate::{integrate_to, Method};
use crate::vec3::{State, Vec3};
use crate::wind::Wind;
use serde::{Deserialize, Serialize};

/// Errors below this are floating-point round-off, not truncation error, and
/// are excluded from the slope fit.
pub const ROUNDOFF_FLOOR: f64 = 1e-10;
/// An error above this many metres means the method went numerically unstable.
pub const UNSTABLE_ERROR: f64 = 1e6;
/// The order is fitted on this many of the smallest step sizes, where the
/// error is in its asymptotic `C dt^p` regime.
pub const FIT_POINTS: usize = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MethodSeries {
    pub method: Method,
    pub dts: Vec<f64>,
    pub errors: Vec<f64>,
    pub evaluations: Vec<u64>,
    /// `true` where the run blew up (its error is reported as `NaN`/null).
    pub unstable: Vec<bool>,
    pub observed_order: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConvergenceStudy {
    pub id: String,
    pub title: String,
    pub reference: String,
    pub t_end: f64,
    pub series: Vec<MethodSeries>,
}

impl ConvergenceStudy {
    pub fn series(&self, method: Method) -> &MethodSeries {
        self.series
            .iter()
            .find(|s| s.method == method)
            .expect("method present")
    }
}

/// Least-squares slope of `ln(error)` against `ln(dt)`, fitted on the
/// [`FIT_POINTS`] smallest steps whose error is above round-off.
pub fn observed_order(dts: &[f64], errors: &[f64]) -> f64 {
    let mut pts: Vec<(f64, f64)> = dts
        .iter()
        .zip(errors)
        .filter(|(_, e)| **e > ROUNDOFF_FLOOR && e.is_finite())
        .map(|(d, e)| (d.ln(), e.ln()))
        .collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts.truncate(FIT_POINTS);
    if pts.len() < 2 {
        return f64::NAN;
    }
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    sxy / sxx
}

/// Step sizes halving from `dt_max`.
pub fn halving_steps(dt_max: f64, count: usize) -> Vec<f64> {
    (0..count).map(|i| dt_max / 2f64.powi(i as i32)).collect()
}

#[allow(clippy::too_many_arguments)]
fn study<S, E>(
    id: &str,
    title: &str,
    reference: &str,
    f: &S,
    y0: State,
    t_end: f64,
    dts: &[f64],
    error: E,
) -> ConvergenceStudy
where
    S: OdeSystem + ?Sized,
    E: Fn(&State) -> f64,
{
    let series = Method::ALL
        .iter()
        .map(|&method| {
            let mut errors = Vec::with_capacity(dts.len());
            let mut evaluations = Vec::with_capacity(dts.len());
            for &dt in dts {
                let run = integrate_to(f, method, y0, 0.0, t_end, dt);
                errors.push(error(&run.last().unwrap().state));
                evaluations.push((run.len() as u64 - 1) * method.evals_per_step());
            }
            let unstable: Vec<bool> = errors
                .iter()
                .map(|e| !(e.is_finite() && *e < UNSTABLE_ERROR))
                .collect();
            for (e, bad) in errors.iter_mut().zip(&unstable) {
                if *bad {
                    *e = f64::NAN;
                }
            }
            let observed_order = observed_order(dts, &errors);
            MethodSeries {
                method,
                dts: dts.to_vec(),
                errors,
                evaluations,
                unstable,
                observed_order,
            }
        })
        .collect();
    ConvergenceStudy {
        id: id.into(),
        title: title.into(),
        reference: reference.into(),
        t_end,
        series,
    }
}

/// Vertical fall with quadratic drag in uniform air vs the exact tanh solution.
pub fn vertical_drag_study(dts: &[f64]) -> ConvergenceStudy {
    let payload = Payload {
        mass: 10.0,
        cd_area: 0.1,
        parachute: None,
    };
    let rho = 1.225;
    let env = Environment {
        atmosphere: Atmosphere::Constant { rho },
        wind: Wind::None,
    };
    let model = Model::new(&payload, &env);
    let k = rho * payload.cd_area / (2.0 * payload.mass);
    let z0 = 10_000.0;
    let t_end = 10.0;
    let (_, fallen) = vertical_drag_fall(t_end, k, 0.0, G0);
    let exact_z = z0 - fallen;
    study(
        "vertical-drag",
        "Vertical fall with quadratic drag",
        "exact tanh solution",
        &model,
        State::new(Vec3::new(0.0, 0.0, z0), Vec3::ZERO),
        t_end,
        dts,
        |s| (s.pos.z - exact_z).abs(),
    )
}

/// Any model vs a fine-step RK4 reference (`dt_min / 64`).
pub fn reference_study<S: OdeSystem + ?Sized>(
    id: &str,
    title: &str,
    f: &S,
    y0: State,
    t_end: f64,
    dts: &[f64],
) -> ConvergenceStudy {
    let dt_ref = dts.iter().cloned().fold(f64::INFINITY, f64::min) / 64.0;
    let reference = integrate_to(f, Method::Rk4, y0, 0.0, t_end, dt_ref)
        .last()
        .unwrap()
        .state;
    study(id, title, "RK4 at dt/64", f, y0, t_end, dts, |s| {
        (s.pos - reference.pos).norm()
    })
}
