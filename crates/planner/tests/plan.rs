//! End-to-end planning on the example scenario.

use lifeline_planner::*;
use std::sync::OnceLock;

fn world() -> &'static World {
    static W: OnceLock<World> = OnceLock::new();
    W.get_or_init(|| World::generate(GeneratedSpec::default()).unwrap())
}

fn example() -> PlanScenario {
    PlanScenario::example(world())
}

fn assignment<'a>(p: &'a Plan, id: &str) -> &'a Assignment {
    p.assignments.iter().find(|a| a.request_id == id).unwrap()
}

#[test]
fn example_uses_each_mode_for_its_strength() {
    let s = example();
    let p = plan(world(), &s).unwrap();
    assert_eq!(p.assignments.len(), s.requests.len());
    assert_eq!(p.summary.unserved, 0);
    assert_eq!(p.summary.critical_on_time, p.summary.critical_total);
    assert!(
        p.assignments
            .iter()
            .all(|a| a.on_time && a.within_tolerance),
        "{:#?}",
        p.assignments
    );
    // bulk water goes by road, the 12 kg medical load by drone,
    // and the cut-off mountain village by air
    assert_eq!(assignment(&p, "R2").mode, Some(Mode::Truck));
    assert_eq!(assignment(&p, "R4").mode, Some(Mode::Drone));
    assert_eq!(assignment(&p, "R7").mode, Some(Mode::Airdrop));
    assert!(p.summary.expected_kg <= p.summary.demand_kg);
    assert!(p.summary.expected_kg > 0.9 * p.summary.demand_kg);
}

#[test]
fn schedule_is_consistent() {
    let s = example();
    let p = plan(world(), &s).unwrap();
    for unit in &p.schedule {
        for t in &unit.trips {
            assert!(t.depart_h <= t.arrive_h && t.arrive_h <= t.ready_h);
        }
        for w in unit.trips.windows(2) {
            assert!(
                w[1].depart_h >= w[0].ready_h - 1e-9,
                "{} double-booked",
                unit.unit
            );
        }
    }
    for a in &p.assignments {
        let r = s.requests.iter().find(|r| r.id == a.request_id).unwrap();
        let trips: Vec<_> = p
            .schedule
            .iter()
            .flat_map(|u| &u.trips)
            .filter(|t| t.request_id == a.request_id)
            .collect();
        assert_eq!(trips.len(), a.trips);
        let carried: f64 = trips.iter().map(|t| t.load_kg).sum();
        assert!(
            (carried - r.demand_kg).abs() < 1e-6,
            "{}: carried {carried} of {}",
            r.id,
            r.demand_kg
        );
    }
}

#[test]
fn forcing_a_mode_and_removing_a_fleet() {
    let mut s = example();
    s.requests[1].mode = ModeChoice::Airdrop; // water by air: 6 t needs two sorties
    let p = plan(world(), &s).unwrap();
    let a = assignment(&p, "R2");
    assert_eq!(a.mode, Some(Mode::Airdrop));
    assert_eq!(a.trips, 2);
    let truck_opt = &a.options[0];
    assert!(
        !truck_opt.feasible && truck_opt.reason.as_deref() == Some("excluded for this request")
    );

    let mut s = example();
    s.fleet.aircraft.count = 0;
    let p = plan(world(), &s).unwrap();
    let village = assignment(&p, "R7");
    assert_ne!(village.mode, Some(Mode::Airdrop));
    assert!(
        !village.on_time,
        "without the aircraft the village is reached late"
    );
}

#[test]
fn stricter_tolerance_never_lowers_delivery_probability() {
    let mut s = example();
    s.settings.min_delivery_prob = 0.5;
    let loose = plan(world(), &s).unwrap();
    s.settings.min_delivery_prob = 0.995;
    let strict = plan(world(), &s).unwrap();
    for (a, b) in loose.assignments.iter().zip(&strict.assignments) {
        if a.mode == b.mode {
            assert!(
                b.delivery_prob >= a.delivery_prob - 1e-12,
                "{}",
                a.request_id
            );
        }
    }
    let mean = |p: &Plan| {
        p.assignments.iter().map(|a| a.delivery_prob).sum::<f64>() / p.assignments.len() as f64
    };
    assert!(mean(&strict) >= mean(&loose));
}

#[test]
fn unreachable_request_is_reported_not_dropped() {
    let mut s = example();
    s.fleet.aircraft.count = 0;
    s.fleet.drones.count = 0;
    // surround the request with an impassable area
    let r = &s.requests[0];
    let (x, y) = (r.x, r.y);
    s.risk.hazards.push(lifeline_core::risk::HazardArea {
        id: "HX".into(),
        name: "flooded".into(),
        polygon: vec![
            [x - 2000.0, y - 2000.0],
            [x + 2000.0, y - 2000.0],
            [x + 2000.0, y + 2000.0],
            [x - 2000.0, y + 2000.0],
        ],
        rate_per_min: 0.0,
        impassable: true,
    });
    let p = plan(world(), &s).unwrap();
    let a = assignment(&p, "R1");
    assert_eq!(a.mode, None);
    assert!(a.reason.as_deref().unwrap().contains("blocked"));
    assert_eq!(p.summary.unserved, 1);
}

#[test]
fn scenario_round_trips_and_validates() {
    let s = example();
    let json = serde_json::to_string(&s).unwrap();
    let back: PlanScenario = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
    let mut bad = s.clone();
    bad.requests[0].demand_kg = 0.0;
    assert!(plan(world(), &bad).is_err());
    let mut other = s;
    other.world = WorldSpec::Generated(GeneratedSpec {
        seed: 99,
        ..GeneratedSpec::default()
    });
    assert!(plan(world(), &other).unwrap_err().contains("world"));
}

#[test]
fn rejects_scenarios_that_would_mislead_the_plan() {
    let check = |edit: &dyn Fn(&mut PlanScenario), what: &str| {
        let mut s = example();
        edit(&mut s);
        assert!(plan(world(), &s).is_err(), "{what} accepted");
    };
    // duplicate ids mixed up deadlines and probabilities in the summary
    check(
        &|s| s.requests[1].id = s.requests[0].id.clone(),
        "duplicate id",
    );
    // negative detectability made routing loop on negative costs
    check(
        &|s| s.fleet.trucks.truck.detectability = -1.0,
        "negative detectability",
    );
    check(
        &|s| s.fleet.trucks.truck.handling_min = -600.0,
        "negative handling",
    );
    check(&|s| s.fleet.aircraft.cost_per_hour = -1.0, "negative cost");
    // a request 1000 km off a 24 km map was planned on clamped terrain
    check(&|s| s.requests[0].x = 1.0e6, "request off the map");
    check(&|s| s.depot.y = -1.0e6, "depot off the map");
    // negative air density gave NaN drone energy that passed the battery check
    check(
        &|s| {
            s.weather.atmosphere = lifeline_core::atmosphere::Atmosphere::Isa {
                temperature_offset: -300.0,
            }
        },
        "negative density",
    );
}

#[test]
fn absurd_trip_counts_are_infeasible_not_scheduled() {
    // 0.1 g per truck used to schedule 15 million trips for one request
    let mut s = example();
    s.fleet.trucks.truck.capacity_kg = 1e-4;
    let p = plan(world(), &s).unwrap();
    let truck = &assignment(&p, "R7").options[0];
    assert!(!truck.feasible && truck.reason.as_deref().unwrap().contains("trips"));
    assert!(p.schedule.iter().map(|u| u.trips.len()).sum::<usize>() < 100);
}
