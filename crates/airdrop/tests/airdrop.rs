//! Release-point solver and Monte Carlo dispersion.

use lifeline_airdrop::atmosphere::{Atmosphere, G0};
use lifeline_airdrop::dynamics::{Environment, Model, OdeSystem, Payload};
use lifeline_airdrop::integrate::Method;
use lifeline_airdrop::montecarlo::Uncertainty;
use lifeline_airdrop::release::{
    solve_release_on_track, solve_release_point, Aircraft, DropProblem,
};
use lifeline_airdrop::scenario::{drop_report, monte_carlo_report, Scenario};
use lifeline_airdrop::stats::{covariance2, Ellipse, Rng};
use lifeline_airdrop::terrain::Terrain;
use lifeline_airdrop::wind::Wind;
use lifeline_airdrop::{State, Vec3};

#[test]
fn vacuum_release_point_matches_hand_calculation() {
    // No air: the package keeps the aircraft's speed for sqrt(2h/g) seconds.
    let payload = Payload {
        mass: 100.0,
        cd_area: 1.0,
        parachute: None,
    };
    let env = Environment {
        atmosphere: Atmosphere::Vacuum,
        wind: Wind::None,
    };
    let model = Model::new(&payload, &env);
    let terrain = Terrain::default();
    let p = DropProblem {
        model: &model,
        terrain: &terrain,
        aircraft: Aircraft {
            height_agl: 500.0,
            ground_speed: 60.0,
            track_deg: 0.0,
        },
        target: [100.0, 200.0],
        method: Method::Rk4,
        dt: 0.1,
        t_max: 100.0,
    };
    let sol = solve_release_point(&p, 1e-6).unwrap();
    let throw = 60.0 * (2.0 * 500.0 / G0).sqrt();
    assert!(sol.converged);
    assert!((sol.release_point[0] - 100.0).abs() < 1e-6);
    assert!((sol.release_point[1] - (200.0 - throw)).abs() < 1e-6);
    assert!((sol.release_distance_before_target - throw).abs() < 1e-6);
}

#[test]
fn default_scenario_release_hits_target() {
    let s = Scenario::default();
    let r = drop_report(&s).unwrap();
    assert!(r.release.converged);
    assert!(
        r.release.miss_distance < 0.01,
        "miss {}",
        r.release.miss_distance
    );
    // flying east into a wind from ~W-SW: package is thrown/drifted east, so
    // the release is upwind (west) of the target
    assert!(r.release.release_point[0] < -100.0);
    // under canopy the descent is slow
    assert!(r.release.impact_vertical_speed < 10.0 && r.release.impact_vertical_speed > 5.0);
    let path_end = r.path.last().unwrap();
    assert!((path_end.pos.z - 0.0).abs() < 1e-6);
    assert!(r.path.iter().any(|p| p.fill > 0.99));

    // On-track (Brent) solution zeroes the along-track miss only; the cross
    // wind component leaves a cross-track miss that timing cannot fix.
    let on = r.on_track.unwrap();
    assert!(on.miss_distance - on.cross_track_miss.abs() < 0.01);
    assert!(on.cross_track_miss.abs() > 10.0);
}

/// Northward wind that strengthens with distance east, so the landing point
/// is a genuinely nonlinear function of the release point.
struct ShearedWind;
impl OdeSystem for ShearedWind {
    fn deriv(&self, _t: f64, y: &State) -> State {
        let k = 1.2 * 8.0 / (2.0 * 50.0); // rho CdA / 2m
        let wind = Vec3::new(0.0, 3.0 + 0.002 * y.pos.x, 0.0);
        let v_rel = y.vel - wind;
        State::new(
            y.vel,
            v_rel * (-k * v_rel.norm()) + Vec3::new(0.0, 0.0, -G0),
        )
    }
}

#[test]
fn newton_converges_with_position_dependent_wind_and_terrain() {
    let model = ShearedWind;
    let terrain = Terrain::Hill {
        base: 0.0,
        height: 80.0,
        center_x: 300.0,
        center_y: 300.0,
        radius: 150.0,
    };
    let p = DropProblem {
        model: &model,
        terrain: &terrain,
        aircraft: Aircraft {
            height_agl: 300.0,
            ground_speed: 50.0,
            track_deg: 45.0,
        },
        target: [250.0, 320.0],
        method: Method::Rk4,
        dt: 0.05,
        t_max: 500.0,
    };
    let sol = solve_release_point(&p, 1e-3).unwrap();
    assert!(sol.converged, "{sol:?}");
    assert!(sol.miss_distance < 1e-3);
    assert!(
        sol.iterations >= 2,
        "nonlinear problem should need >1 Newton step"
    );
    let brent = solve_release_on_track(&p, 1e-4).unwrap();
    assert!(brent.miss_distance >= brent.cross_track_miss.abs() - 1e-3);
}

#[test]
fn euler_release_differs_from_rk4() {
    let mut s = Scenario::default();
    s.integrator.dt = 0.5;
    let rk4 = drop_report(&s).unwrap().release.release_point;
    s.integrator.method = Method::Euler;
    let euler = drop_report(&s).unwrap().release.release_point;
    let gap = (rk4[0] - euler[0]).hypot(rk4[1] - euler[1]);
    assert!(
        gap > 0.5,
        "coarse Euler should plan a different release point, gap {gap}"
    );
}

#[test]
fn unstable_step_is_an_error_not_a_wrong_answer() {
    for (method, dt) in [
        (Method::Euler, 1.0),
        (Method::Euler, 2.0),
        (Method::Rk4, 5.0),
    ] {
        let mut s = Scenario::default();
        s.integrator.method = method;
        s.integrator.dt = dt;
        let err = drop_report(&s).unwrap_err();
        assert!(err.contains("diverged"), "{method:?} dt={dt}: {err}");
        assert!(monte_carlo_report(&s).is_err());
    }
    // a stable but coarse step still works
    let mut s = Scenario::default();
    s.integrator.method = Method::Euler;
    s.integrator.dt = 0.25;
    assert!(drop_report(&s).unwrap().release.miss_distance < 0.01);
}

#[test]
fn rng_is_reproducible_and_normal() {
    let mut a = Rng::new(7);
    let mut b = Rng::new(7);
    for _ in 0..100 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
    let mut r = Rng::new(123);
    let n = 200_000;
    let xs: Vec<f64> = (0..n).map(|_| r.normal()).collect();
    let mean = xs.iter().sum::<f64>() / n as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    assert!(mean.abs() < 0.01 && (var - 1.0).abs() < 0.01);
}

#[test]
fn ellipse_geometry() {
    let e = Ellipse::from_covariance([0.0, 0.0], [[4.0, 0.0], [0.0, 1.0]], 1.0 - (-0.5f64).exp());
    // p = 1 - e^{-1/2}  <=>  scale = 1, so axes are the standard deviations
    assert!((e.semi_major - 2.0).abs() < 1e-12 && (e.semi_minor - 1.0).abs() < 1e-12);
    assert!(e.angle.abs() < 1e-12);
    assert!(e.contains([1.9, 0.0]) && !e.contains([0.0, 1.1]));

    let mut r = Rng::new(9);
    let pts: Vec<[f64; 2]> = (0..50_000)
        .map(|_| {
            let (a, b) = (r.normal(), r.normal());
            [3.0 * a + b, b] // correlated
        })
        .collect();
    let cov = covariance2(&pts);
    let e95 = Ellipse::from_covariance([0.0, 0.0], cov, 0.95);
    let inside = pts.iter().filter(|p| e95.contains(**p)).count() as f64 / pts.len() as f64;
    assert!((inside - 0.95).abs() < 0.005, "{inside}");
}

#[test]
fn monte_carlo_ellipse_covers_samples_and_is_reproducible() {
    let mut s = Scenario::default();
    s.monte_carlo.samples = 1500;
    s.integrator.dt = 0.1;
    let a = monte_carlo_report(&s).unwrap();
    let b = monte_carlo_report(&s).unwrap();
    assert_eq!(a.result.impacts, b.result.impacts, "same seed, same answer");
    assert_eq!(a.result.landed_fraction, 1.0);
    let e95 = a
        .result
        .ellipses
        .iter()
        .find(|e| e.probability == 0.95)
        .unwrap();
    assert!(
        (e95.observed_fraction - 0.95).abs() < 0.03,
        "{}",
        e95.observed_fraction
    );
    let e50 = a
        .result
        .ellipses
        .iter()
        .find(|e| e.probability == 0.5)
        .unwrap();
    assert!((e50.observed_fraction - 0.5).abs() < 0.05);
    // aimed at the target: bias is small compared with the spread
    assert!(a.result.bias < 0.3 * a.result.cep);
    assert!(a.inside_drop_zone > 0.5);

    s.monte_carlo.seed = 1;
    let c = monte_carlo_report(&s).unwrap();
    assert_ne!(a.result.impacts, c.result.impacts);
}

#[test]
fn zero_uncertainty_collapses_the_ellipse() {
    let mut s = Scenario::default();
    s.monte_carlo.samples = 50;
    s.monte_carlo.uncertainty = Uncertainty {
        wind_sigma: 0.0,
        release_time_sigma: 0.0,
        nav_sigma: 0.0,
        drag_sigma: 0.0,
    };
    let r = monte_carlo_report(&s).unwrap();
    assert!(r.result.cep < 0.01);
    assert!(r.result.ellipses.iter().all(|e| e.semi_major < 1e-6));
}

#[test]
fn more_wind_uncertainty_means_bigger_ellipse() {
    let mut s = Scenario::default();
    s.monte_carlo.samples = 600;
    s.integrator.dt = 0.1;
    let small = monte_carlo_report(&s).unwrap().result.ellipses[1].semi_major;
    s.monte_carlo.uncertainty.wind_sigma = 3.0;
    let big = monte_carlo_report(&s).unwrap().result.ellipses[1].semi_major;
    assert!(big > 1.5 * small, "{small} -> {big}");
}

#[test]
fn scenario_round_trips_through_json_and_validates() {
    let s = Scenario::default();
    let json = serde_json::to_string_pretty(&s).unwrap();
    let back: Scenario = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);

    let mut bad = s.clone();
    bad.payload.mass = 0.0;
    assert!(drop_report(&bad).is_err());
    let mut bad = s;
    bad.integrator.dt = 0.0;
    assert!(monte_carlo_report(&bad).is_err());
}
