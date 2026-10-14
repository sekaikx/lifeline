//! Drone power, wind triangle and sortie planning.

use lifeline_core::raster::Raster;
use lifeline_core::terrain::TerrainGrid;
use lifeline_drone::{ground_speed, DronePlanner, DroneSpec};

fn spec() -> DroneSpec {
    DroneSpec {
        empty_mass_kg: 25.0,
        payload_kg: 15.0,
        battery_wh: 1800.0,
        reserve: 0.2,
        rotor_area_m2: 1.6,
        cd_area_m2: 0.3,
        airspeed_ms: 18.0,
        efficiency: 0.7,
        cruise_agl_m: 80.0,
        detectability: 0.5,
        turnaround_min: 10.0,
    }
}

fn flat(size: f64, cells: usize) -> TerrainGrid {
    let n = cells + 1;
    let d = size / cells as f64;
    TerrainGrid {
        x0: -size / 2.0,
        y0: -size / 2.0,
        dx: d,
        dy: d,
        nx: n,
        ny: n,
        heights: vec![0.0; n * n],
    }
}

#[test]
fn hover_power_matches_momentum_theory() {
    let s = spec();
    let rho = 1.225;
    let w = 40.0 * 9.80665;
    let ideal = w * (w / (2.0 * rho * s.rotor_area_m2)).sqrt();
    assert!((s.power(15.0, 0.0, rho) - 1.15 * ideal / 0.7).abs() < 1e-9);
    // forward flight needs less induced power, so cruise is cheaper than hover
    assert!(s.power(15.0, 15.0, rho) < s.power(15.0, 0.0, rho));
    // heavier and thinner air cost more
    assert!(s.power(15.0, 18.0, rho) > s.power(0.0, 18.0, rho));
    assert!(s.power(15.0, 18.0, 1.0) > s.power(15.0, 18.0, rho));
}

#[test]
fn wind_triangle() {
    let east = [1.0, 0.0];
    assert_eq!(ground_speed(18.0, [0.0, 0.0], east), Some(18.0));
    assert_eq!(ground_speed(18.0, [5.0, 0.0], east), Some(23.0));
    assert_eq!(ground_speed(18.0, [-5.0, 0.0], east), Some(13.0));
    let crab = ground_speed(18.0, [0.0, 6.0], east).unwrap();
    assert!((crab - (18.0f64 * 18.0 - 36.0).sqrt()).abs() < 1e-12);
    assert_eq!(
        ground_speed(18.0, [0.0, 19.0], east),
        None,
        "crosswind stronger than airspeed"
    );
    assert_eq!(
        ground_speed(18.0, [-20.0, 0.0], east),
        None,
        "headwind stronger than airspeed"
    );
}

#[test]
fn sortie_energy_range_and_wind() {
    let t = flat(30_000.0, 150);
    let field = Raster::new(t.x0, t.y0, t.dx, t.nx, t.ny, 0.0);
    let mk = |wind| DronePlanner {
        terrain: &t,
        field: &field,
        spec: spec(),
        wind,
        rho: 1.225,
        antenna_m: 10.0,
    };
    let calm = mk([0.0, 0.0]);
    let near = calm.route([0.0, 0.0], [4000.0, 0.0], 15.0, 0.0);
    assert!(near.feasible);
    assert!(
        (near.length_m - 4000.0).abs() < 1.0,
        "straight line in calm air: {}",
        near.length_m
    );
    assert!((near.time_out_s - 4000.0 / 18.0).abs() < 1.0);
    assert!(
        near.energy_back_wh < near.energy_out_wh,
        "flying home empty is cheaper"
    );
    assert_eq!(
        near.comms_fraction, 1.0,
        "flat ground: always in radio sight"
    );

    let far = calm.route([0.0, 0.0], [14_000.0, 0.0], 15.0, 0.0);
    assert!(!far.feasible);
    assert!(far.reason.as_deref().unwrap().contains("out of range"));

    let tail = mk([6.0, 0.0]).route([0.0, 0.0], [4000.0, 0.0], 15.0, 0.0);
    assert!(tail.time_out_s < near.time_out_s && tail.time_back_s > near.time_back_s);
}

#[test]
fn risk_weight_bends_the_path_away_from_exposure() {
    let t = flat(20_000.0, 100);
    let mut field = Raster::new(t.x0, t.y0, t.dx, t.nx, t.ny, 0.0);
    // a band of exposure across the direct line, open at the top
    for j in 0..field.ny {
        for i in 0..field.nx {
            let (x, y) = field.center(i, j);
            if x.abs() < 600.0 && y < 2500.0 {
                let k = field.index(i, j);
                field.values[k] = 0.05;
            }
        }
    }
    let p = DronePlanner {
        terrain: &t,
        field: &field,
        spec: spec(),
        wind: [0.0, 0.0],
        rho: 1.225,
        antenna_m: 10.0,
    };
    let direct = p.route([-3000.0, 0.0], [3000.0, 0.0], 5.0, 0.0);
    let careful = p.route([-3000.0, 0.0], [3000.0, 0.0], 5.0, 50.0 * 3600.0);
    assert!(
        careful.survival_out > direct.survival_out + 0.5,
        "{} vs {}",
        careful.survival_out,
        direct.survival_out
    );
    assert!(careful.length_m > direct.length_m);
    assert!(careful.survival_out > 0.99);
}
