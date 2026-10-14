//! Road generation and risk-aware routing.

use lifeline_core::raster::Raster;
use lifeline_core::risk::{exposure_field, HazardArea, RiskModel, RiskSite};
use lifeline_core::terrain::TerrainGrid;
use lifeline_ground::{Road, RoadClass, RoadNetwork, RoutingContext, Town, Truck};

fn truck() -> Truck {
    Truck {
        capacity_kg: 3000.0,
        speed_paved_kmh: 60.0,
        speed_gravel_kmh: 36.0,
        speed_track_kmh: 18.0,
        speed_offroad_kmh: 6.0,
        handling_min: 20.0,
        detectability: 1.0,
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

/// Two towns joined by a fast direct road past a risk site, and a slower
/// detour through a third town out of its sight.
fn two_routes() -> (TerrainGrid, RoadNetwork, RiskModel) {
    let t = flat(20_000.0, 100);
    let towns = vec![
        Town {
            id: 0,
            name: "A".into(),
            x: -6000.0,
            y: 0.0,
        },
        Town {
            id: 1,
            name: "B".into(),
            x: 6000.0,
            y: 0.0,
        },
        Town {
            id: 2,
            name: "C".into(),
            x: 0.0,
            y: 7000.0,
        },
    ];
    let roads = vec![
        Road {
            a: 0,
            b: 1,
            class: RoadClass::Paved,
            points: vec![[-6000.0, 0.0], [6000.0, 0.0]],
        },
        Road {
            a: 0,
            b: 2,
            class: RoadClass::Paved,
            points: vec![[-6000.0, 0.0], [0.0, 7000.0]],
        },
        Road {
            a: 2,
            b: 1,
            class: RoadClass::Paved,
            points: vec![[0.0, 7000.0], [6000.0, 0.0]],
        },
    ];
    let risk = RiskModel {
        sites: vec![RiskSite {
            id: "S".into(),
            name: "S".into(),
            x: 0.0,
            y: -1500.0,
            height: 5.0,
            range: 2500.0,
            rate_per_min: 0.2,
        }],
        hazards: vec![],
    };
    (t, RoadNetwork { towns, roads }, risk)
}

#[test]
fn generated_network_is_connected_reproducible_and_follows_valleys() {
    let t = TerrainGrid::procedural(3, 20_000.0, 120, 100.0, 500.0);
    let a = RoadNetwork::generate(&t, 3, 15);
    let b = RoadNetwork::generate(&t, 3, 15);
    assert_eq!(a, b);
    assert_eq!(a.towns.len(), 15);
    assert!(a.is_connected());
    assert!(
        a.roads.len() >= a.towns.len(),
        "more than a tree: {} roads",
        a.roads.len()
    );
    assert!(
        a.roads
            .iter()
            .filter(|r| r.class == RoadClass::Paved)
            .count()
            == a.towns.len() - 1,
        "MST is paved"
    );
    // roads end at their towns
    for r in &a.roads {
        let (s, e) = (r.points[0], r.points[r.points.len() - 1]);
        assert_eq!(s, [a.towns[r.a].x, a.towns[r.a].y]);
        assert_eq!(e, [a.towns[r.b].x, a.towns[r.b].y]);
    }
    // laid-out roads climb less than straight lines between the same towns
    let climb = |pts: &[[f64; 2]]| -> f64 {
        let fine = lifeline_core::risk::resample(pts, t.dx);
        fine.windows(2)
            .map(|w| (t.at(w[1][0], w[1][1]) - t.at(w[0][0], w[0][1])).abs())
            .sum()
    };
    let (road, straight): (f64, f64) = a
        .roads
        .iter()
        .map(|r| {
            (
                climb(&r.points),
                climb(&[r.points[0], r.points[r.points.len() - 1]]),
            )
        })
        .fold((0.0, 0.0), |acc, x| (acc.0 + x.0, acc.1 + x.1));
    assert!(
        road < straight,
        "roads should avoid climbing: {road:.0} m vs {straight:.0} m"
    );
}

#[test]
fn risk_weight_trades_time_for_safety() {
    let (t, net, risk) = two_routes();
    let field = exposure_field(&t, &risk, 2.5, 1.0);
    let ctx = RoutingContext::new(&t, &net, &field, &risk, truck());
    let fast = ctx.route([-6000.0, 0.0], [6000.0, 0.0], 0.0).unwrap();
    let safe = ctx
        .route([-6000.0, 0.0], [6000.0, 0.0], 100.0 * 3600.0)
        .unwrap();
    assert!(
        (fast.time_s - 12_000.0 / (60.0 / 3.6)).abs() < 1.0,
        "12 km at 60 km/h: {}",
        fast.time_s
    );
    assert!(
        fast.survival < 0.5,
        "direct road passes the site: {}",
        fast.survival
    );
    assert!(
        safe.survival > 0.999,
        "detour is out of sight: {}",
        safe.survival
    );
    assert!(safe.time_s > fast.time_s);
    assert!(safe.points.contains(&[0.0, 7000.0]));

    let front = ctx.options([-6000.0, 0.0], [6000.0, 0.0]);
    assert_eq!(front.len(), 2, "both routes are Pareto-optimal");
    assert!(front[0].time_s < front[1].time_s && front[0].survival < front[1].survival);
}

#[test]
fn impassable_area_closes_a_road() {
    let (t, net, mut risk) = two_routes();
    risk.sites.clear();
    risk.hazards.push(HazardArea {
        id: "H".into(),
        name: "flood".into(),
        polygon: vec![
            [-500.0, -500.0],
            [500.0, -500.0],
            [500.0, 500.0],
            [-500.0, 500.0],
        ],
        rate_per_min: 0.0,
        impassable: true,
    });
    let field = Raster::new(t.x0, t.y0, t.dx, t.nx, t.ny, 0.0);
    let ctx = RoutingContext::new(&t, &net, &field, &risk, truck());
    let r = ctx.route([-6000.0, 0.0], [6000.0, 0.0], 0.0).unwrap();
    assert!(
        r.points.contains(&[0.0, 7000.0]),
        "must detour round the flood"
    );
}

#[test]
fn offroad_access_and_slope_slowdown() {
    let (t, net, risk) = two_routes();
    let field = Raster::new(t.x0, t.y0, t.dx, t.nx, t.ny, 0.0);
    let ctx = RoutingContext::new(&t, &net, &field, &risk, truck());
    // 1 km off the road at each end
    let r = ctx
        .route([-6000.0, -1000.0], [6000.0, 1000.0], 0.0)
        .unwrap();
    assert!((r.offroad_m - 2000.0).abs() < 1.0, "{}", r.offroad_m);
    let tk = truck();
    assert!(
        (tk.speed(None, 0.1) - tk.speed(None, 0.0) / 2.0).abs() < 1e-12,
        "10 % grade halves speed"
    );
    assert!(tk.speed(Some(RoadClass::Paved), 0.0) > tk.speed(Some(RoadClass::Gravel), 0.0));
}
