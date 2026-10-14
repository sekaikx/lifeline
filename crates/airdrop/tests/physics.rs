//! Atmosphere, wind, dynamics and analytic-solution checks.

use lifeline_airdrop::analytic::{vacuum_impact_time, vacuum_state, vertical_drag_fall};
use lifeline_airdrop::atmosphere::{isa, Atmosphere, G0, RHO0};
use lifeline_airdrop::dynamics::{
    terminal_velocity, Environment, Model, OdeSystem, Parachute, Payload,
};
use lifeline_airdrop::integrate::{simulate, Method};
use lifeline_airdrop::terrain::{Terrain, TerrainGrid};
use lifeline_airdrop::wind::{heading_vector, wind_vector, Wind, WindLayer};
use lifeline_airdrop::{State, Vec3};

fn rel(a: f64, b: f64) -> f64 {
    ((a - b) / b).abs()
}

#[test]
fn isa_matches_standard_tables() {
    // U.S. Standard Atmosphere 1976, geometric altitude.
    let sl = isa(0.0);
    assert!((sl.temperature - 288.15).abs() < 1e-9);
    assert!((sl.pressure - 101_325.0).abs() < 1e-6);
    assert!((sl.density - 1.2250).abs() < 1e-4);
    assert!((RHO0 - 1.225).abs() < 1e-3);

    for (z, rho, t) in [
        (1_000.0, 1.1117, 281.65),
        (5_000.0, 0.73643, 255.68),
        (10_000.0, 0.41351, 223.25),
        (20_000.0, 0.088_910, 216.65),
    ] {
        let a = isa(z);
        assert!(
            rel(a.density, rho) < 1e-3,
            "density at {z}: {} vs {rho}",
            a.density
        );
        assert!(
            (a.temperature - t).abs() < 0.02,
            "temperature at {z}: {}",
            a.temperature
        );
    }
}

#[test]
fn isa_is_continuous_at_layer_boundaries() {
    // geometric altitudes of the 11 km and 20 km geopotential boundaries
    for h in [11_000.0_f64, 20_000.0, 32_000.0, 47_000.0] {
        let z = 6_356_766.0 * h / (6_356_766.0 - h);
        let (lo, hi) = (isa(z - 1e-6), isa(z + 1e-6));
        assert!(rel(lo.pressure, hi.pressure) < 1e-9);
        assert!((lo.temperature - hi.temperature).abs() < 1e-6);
    }
}

#[test]
fn isa_density_decreases_with_altitude() {
    let mut prev = f64::INFINITY;
    for i in 0..=800 {
        let rho = isa(i as f64 * 100.0).density;
        assert!(rho < prev);
        prev = rho;
    }
}

#[test]
fn hot_day_is_thinner() {
    let hot = Atmosphere::Isa {
        temperature_offset: 20.0,
    };
    assert!(hot.density(1000.0) < Atmosphere::default().density(1000.0));
}

#[test]
fn wind_direction_convention() {
    // wind FROM the west blows TOWARDS the east
    let w = wind_vector(10.0, 270.0);
    assert!((w.x - 10.0).abs() < 1e-12 && w.y.abs() < 1e-12);
    let w = wind_vector(5.0, 0.0); // from north -> towards south
    assert!((w.y + 5.0).abs() < 1e-12);
    let h = heading_vector(90.0);
    assert!((h.x - 1.0).abs() < 1e-12 && h.y.abs() < 1e-12);
}

#[test]
fn layered_wind_interpolates_and_clamps() {
    let wind = Wind::Layered {
        layers: vec![
            WindLayer {
                altitude: 0.0,
                speed: 0.0,
                from_deg: 270.0,
            },
            WindLayer {
                altitude: 100.0,
                speed: 10.0,
                from_deg: 270.0,
            },
        ],
    };
    assert!((wind.velocity(Vec3::new(0.0, 0.0, 50.0)).x - 5.0).abs() < 1e-12);
    assert!((wind.velocity(Vec3::new(0.0, 0.0, 500.0)).x - 10.0).abs() < 1e-12);
    assert!(wind.velocity(Vec3::new(0.0, 0.0, -10.0)).norm() < 1e-12);
    let bad = Wind::Layered {
        layers: vec![
            WindLayer {
                altitude: 100.0,
                speed: 1.0,
                from_deg: 0.0,
            },
            WindLayer {
                altitude: 50.0,
                speed: 1.0,
                from_deg: 0.0,
            },
        ],
    };
    assert!(bad.validate().is_err());
}

#[test]
fn drag_opposes_air_relative_velocity() {
    let payload = Payload {
        mass: 1.0,
        cd_area: 0.5,
        parachute: None,
    };
    let env = Environment {
        atmosphere: Atmosphere::Constant { rho: 1.2 },
        wind: Wind::Uniform {
            speed: 10.0,
            from_deg: 270.0,
        },
    };
    let model = Model {
        gravity: 0.0,
        ..Model::new(&payload, &env)
    };
    // moving with the wind: no drag at all
    let d = model.deriv(0.0, &State::new(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)));
    assert!(d.vel.norm() < 1e-12);
    // at rest in a westerly: pushed east
    let d = model.deriv(0.0, &State::new(Vec3::ZERO, Vec3::ZERO));
    assert!(d.vel.x > 0.0);
    assert!((d.vel.x - 0.5 * 1.2 * 0.5 * 100.0).abs() < 1e-9);
}

#[test]
fn package_reaches_terminal_velocity_and_drifts_with_wind() {
    let payload = Payload {
        mass: 100.0,
        cd_area: 25.0,
        parachute: None,
    };
    let env = Environment {
        atmosphere: Atmosphere::Constant { rho: 1.2 },
        wind: Wind::Uniform {
            speed: 6.0,
            from_deg: 180.0,
        },
    };
    let model = Model::new(&payload, &env);
    let traj = simulate(
        &model,
        Method::Rk4,
        State::new(Vec3::new(0.0, 0.0, 2000.0), Vec3::ZERO),
        0.05,
        1e4,
        &Terrain::default(),
        true,
    );
    let hit = traj.impact_state();
    let vt = terminal_velocity(100.0, 25.0, 1.2, G0);
    assert!((-hit.vel.z - vt).abs() < 1e-3);
    // horizontal velocity has relaxed to the wind (from south -> towards north)
    assert!(hit.vel.x.abs() < 1e-6 && (hit.vel.y - 6.0).abs() < 1e-3);
}

#[test]
fn vacuum_conserves_energy_and_hits_on_time() {
    let payload = Payload {
        mass: 3.0,
        cd_area: 1.0,
        parachute: None,
    };
    let env = Environment {
        atmosphere: Atmosphere::Vacuum,
        wind: Wind::None,
    };
    let model = Model::new(&payload, &env);
    let y0 = State::new(Vec3::new(0.0, 0.0, 500.0), Vec3::new(30.0, 0.0, 20.0));
    let traj = simulate(&model, Method::Rk4, y0, 0.1, 1e3, &Terrain::default(), true);
    let energy = |s: &State| 0.5 * s.vel.dot(s.vel) + G0 * s.pos.z;
    for s in &traj.samples {
        assert!((energy(&s.state) - energy(&y0)).abs() < 1e-8);
    }
    // RK4 is exact for polynomials of degree <= 4; the Hermite impact search
    // is exact for cubics, so impact time matches to round-off.
    let t_exact = vacuum_impact_time(500.0, 20.0, G0);
    assert!((traj.impact_time() - t_exact).abs() < 1e-9);
    let exact = vacuum_state(t_exact, &y0, G0);
    assert!((traj.impact_state().pos - exact.pos).norm() < 1e-8);
}

#[test]
fn analytic_drag_solution_satisfies_ode() {
    let k = 0.01;
    let (v1, s1) = vertical_drag_fall(3.0, k, 5.0, G0);
    let (v2, s2) = vertical_drag_fall(3.0 + 1e-6, k, 5.0, G0);
    let dvdt = (v2 - v1) / 1e-6;
    assert!((dvdt - (G0 - k * v1 * v1)).abs() < 1e-4);
    assert!(((s2 - s1) / 1e-6 - v1).abs() < 1e-4);
    let (v_late, _) = vertical_drag_fall(1e4, k, 0.0, G0);
    assert!((v_late - (G0 / k).sqrt()).abs() < 1e-9);
}

#[test]
fn parachute_inflation_profile() {
    let p = Parachute {
        cd_area: 100.0,
        deploy_delay: 2.0,
        inflation_time: 4.0,
        fill_exponent: 2.0,
    };
    assert_eq!(p.fill_fraction(1.0), 0.0);
    assert!((p.fill_fraction(4.0) - 0.25).abs() < 1e-12);
    assert_eq!(p.fill_fraction(10.0), 1.0);
    let payload = Payload {
        mass: 500.0,
        cd_area: 1.0,
        parachute: Some(p),
    };
    assert!((payload.drag_area(10.0) - 101.0).abs() < 1e-12);
}

#[test]
fn impact_on_terrain_grid_and_hill() {
    let payload = Payload {
        mass: 1.0,
        cd_area: 0.0,
        parachute: None,
    };
    let env = Environment {
        atmosphere: Atmosphere::Vacuum,
        wind: Wind::None,
    };
    let model = Model::new(&payload, &env);
    let y0 = State::new(Vec3::new(10.0, 10.0, 300.0), Vec3::ZERO);

    let grid = Terrain::Grid(TerrainGrid {
        x0: 0.0,
        y0: 0.0,
        dx: 20.0,
        dy: 20.0,
        nx: 2,
        ny: 2,
        heights: vec![0.0, 200.0, 0.0, 200.0],
    });
    assert!(grid.validate().is_ok());
    assert!((grid.elevation(10.0, 10.0) - 100.0).abs() < 1e-12);
    let hit = simulate(&model, Method::Rk4, y0, 0.1, 100.0, &grid, false).impact_state();
    assert!((hit.pos.z - 100.0).abs() < 1e-9);

    let hill = Terrain::Hill {
        base: 0.0,
        height: 120.0,
        center_x: 10.0,
        center_y: 10.0,
        radius: 50.0,
    };
    let hit = simulate(&model, Method::Rk4, y0, 0.1, 100.0, &hill, false).impact_state();
    assert!((hit.pos.z - 120.0).abs() < 1e-9);
}
