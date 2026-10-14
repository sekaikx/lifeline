//! The project's finish line: RK4 beats Euler at equal step size, converges at
//! the right order, and still wins when Euler gets 4x as many steps.

use lifeline_airdrop::convergence::{
    halving_steps, observed_order, reference_study, vertical_drag_study,
};
use lifeline_airdrop::dynamics::{Environment, Model, OdeSystem};
use lifeline_airdrop::integrate::{integrate_to, Method};
use lifeline_airdrop::roots::{brent, expand_bracket, newton_2d, RootError};
use lifeline_airdrop::scenario::{numerics_report, Scenario};
use lifeline_airdrop::{State, Vec3};

#[test]
fn rk4_beats_euler_at_every_equal_step_size() {
    let study = vertical_drag_study(&halving_steps(1.0, 7));
    let (euler, rk4) = (study.series(Method::Euler), study.series(Method::Rk4));
    for (i, dt) in euler.dts.iter().enumerate() {
        assert!(
            rk4.errors[i] < euler.errors[i] / 100.0,
            "dt={dt}: rk4 {} vs euler {}",
            rk4.errors[i],
            euler.errors[i]
        );
    }
}

#[test]
fn observed_orders_are_one_and_four() {
    let study = vertical_drag_study(&halving_steps(0.5, 6));
    let p_euler = study.series(Method::Euler).observed_order;
    let p_rk4 = study.series(Method::Rk4).observed_order;
    assert!((p_euler - 1.0).abs() < 0.1, "Euler order {p_euler}");
    assert!((p_rk4 - 4.0).abs() < 0.3, "RK4 order {p_rk4}");
}

#[test]
fn rk4_beats_euler_at_equal_work() {
    // Euler gets 4x the steps so both spend the same RHS evaluations.
    let study = vertical_drag_study(&halving_steps(1.0, 8));
    let (euler, rk4) = (study.series(Method::Euler), study.series(Method::Rk4));
    for i in 0..rk4.dts.len() - 2 {
        assert_eq!(rk4.evaluations[i], euler.evaluations[i + 2]);
        assert!(rk4.errors[i] < euler.errors[i + 2]);
    }
}

#[test]
fn full_model_converges_at_design_order() {
    let s = Scenario::default();
    let report = numerics_report(&s, 0.5).unwrap();
    let full = report
        .studies
        .iter()
        .find(|st| st.id == "full-smooth")
        .unwrap();
    assert!((full.series(Method::Euler).observed_order - 1.0).abs() < 0.15);
    assert!((full.series(Method::Rk4).observed_order - 4.0).abs() < 0.4);
    for c in &report.comparisons {
        let err = |l: &str| c.paths.iter().find(|p| p.label == l).unwrap().impact_error;
        assert!(
            err("rk4") < err("euler"),
            "{}: rk4 should land closer",
            c.id
        );
    }
    let vac = &report.comparisons[0];
    assert!(
        vac.paths
            .iter()
            .find(|p| p.label == "rk4")
            .unwrap()
            .impact_error
            < 1e-8
    );
}

#[test]
fn euler_is_unstable_through_canopy_opening_at_large_steps() {
    let report = numerics_report(&Scenario::default(), 0.5).unwrap();
    let st = report
        .studies
        .iter()
        .find(|st| st.id == "scenario")
        .unwrap();
    let euler = st.series(Method::Euler);
    assert!(
        euler.unstable[0],
        "dt = 1 s should blow up: {:?}",
        euler.errors
    );
    assert!(euler.errors[0].is_nan());
    assert!(!st.series(Method::Rk4).unstable.iter().any(|u| *u));
    assert!((euler.observed_order - 1.0).abs() < 0.1);
}

#[test]
fn observed_order_of_synthetic_data() {
    let dts = [0.1, 0.05, 0.025];
    let errs: Vec<f64> = dts.iter().map(|d: &f64| 3.0 * d.powi(2)).collect();
    assert!((observed_order(&dts, &errs) - 2.0).abs() < 1e-12);
}

#[test]
fn integrate_to_lands_exactly_on_end_time() {
    struct Decay;
    impl OdeSystem for Decay {
        fn deriv(&self, _t: f64, y: &State) -> State {
            *y * -1.0
        }
    }
    let y0 = State::new(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO);
    let run = integrate_to(&Decay, Method::Rk4, y0, 0.0, 1.0, 0.3);
    assert_eq!(run.last().unwrap().t, 1.0);
    assert!((run.last().unwrap().state.pos.x - (-1.0f64).exp()).abs() < 1e-4);
}

#[test]
fn reference_study_uses_finer_reference() {
    let env = Environment::default();
    let payload = lifeline_airdrop::dynamics::Payload {
        mass: 5.0,
        cd_area: 0.05,
        parachute: None,
    };
    let model = Model::new(&payload, &env);
    let y0 = State::new(Vec3::new(0.0, 0.0, 1000.0), Vec3::new(50.0, 0.0, 10.0));
    let st = reference_study("t", "t", &model, y0, 5.0, &halving_steps(0.5, 4));
    let rk4 = &st.series(Method::Rk4).errors;
    assert!(rk4.windows(2).all(|w| w[1] < w[0] / 10.0), "{rk4:?}");
    assert!(rk4[0] < 1e-2);
}

#[test]
fn brent_finds_roots() {
    let r = brent(|x| x * x - 2.0, 0.0, 2.0, 1e-14, 100).unwrap();
    assert!((r.x - 2f64.sqrt()).abs() < 1e-13);
    let r = brent(|x: f64| x.cos() - x, 0.0, 1.0, 1e-14, 100).unwrap();
    assert!((r.x.cos() - r.x).abs() < 1e-13);
    assert!(
        r.iterations < 12,
        "Brent should be superlinear, took {}",
        r.iterations
    );
    assert_eq!(
        brent(|x| x * x + 1.0, -1.0, 1.0, 1e-12, 100).unwrap_err(),
        RootError::NotBracketed
    );
    let (a, b) = expand_bracket(|x| x - 1000.0, -1.0, 1.0, 50).unwrap();
    assert!(a <= 1000.0 && b >= 1000.0);
}

#[test]
fn newton_2d_solves_nonlinear_system() {
    let r = newton_2d(
        |v| [v[0] * v[0] + v[1] * v[1] - 4.0, v[0] - v[1]],
        [1.0, 0.5],
        1e-7,
        1e-10,
        50,
    );
    assert!(r.converged);
    assert!((r.x[0] - 2f64.sqrt()).abs() < 1e-6 && (r.x[1] - 2f64.sqrt()).abs() < 1e-6);
}
