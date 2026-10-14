//! Terrain generation, line of sight, exposure fields and path search.

use lifeline_core::raster::Raster;
use lifeline_core::risk::{
    exposure_field, line_of_sight, point_in_polygon, polyline_length, resample, segment_exposure,
    simplify, survival, viewshed, HazardArea, RiskModel, RiskSite,
};
use lifeline_core::search::{dijkstra, grid_astar};
use lifeline_core::terrain::TerrainGrid;

fn flat(size: f64, cells: usize) -> TerrainGrid {
    let n = cells + 1;
    TerrainGrid {
        x0: -size / 2.0,
        y0: -size / 2.0,
        dx: size / cells as f64,
        dy: size / cells as f64,
        nx: n,
        ny: n,
        heights: vec![100.0; n * n],
    }
}

/// Flat ground with a straight north-south ridge through x = 0.
fn ridge(size: f64, cells: usize, height: f64) -> TerrainGrid {
    let mut t = flat(size, cells);
    for j in 0..t.ny {
        for i in 0..t.nx {
            let x = t.x0 + i as f64 * t.dx;
            t.heights[j * t.nx + i] = 100.0 + height * (-(x / 300.0).powi(2)).exp();
        }
    }
    t
}

fn site(x: f64, y: f64, range: f64) -> RiskSite {
    RiskSite {
        id: "S".into(),
        name: "S".into(),
        x,
        y,
        height: 5.0,
        range,
        rate_per_min: 0.6,
    }
}

#[test]
fn procedural_terrain_is_reproducible_and_scaled() {
    let a = TerrainGrid::procedural(7, 10_000.0, 64, 200.0, 500.0);
    let b = TerrainGrid::procedural(7, 10_000.0, 64, 200.0, 500.0);
    let c = TerrainGrid::procedural(8, 10_000.0, 64, 200.0, 500.0);
    assert_eq!(a, b);
    assert_ne!(a.heights, c.heights);
    let lo = a.heights.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = a.heights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    assert!((lo - 200.0).abs() < 1e-9 && (hi - 700.0).abs() < 1e-9);
    assert_eq!(a.bounds(), [-5_000.0, -5_000.0, 5_000.0, 5_000.0]);
    // smooth: the steepest east-west or north-south step stays below 1:1
    let h = |i: usize, j: usize| a.heights[j * a.nx + i];
    let mut steepest: f64 = 0.0;
    for j in 0..a.ny - 1 {
        for i in 0..a.nx - 1 {
            steepest = steepest
                .max((h(i + 1, j) - h(i, j)).abs())
                .max((h(i, j + 1) - h(i, j)).abs());
        }
    }
    assert!(
        steepest / a.dx < 1.0,
        "unrealistically steep: {}",
        steepest / a.dx
    );
}

#[test]
fn ridge_blocks_line_of_sight() {
    let t = ridge(4_000.0, 80, 200.0);
    assert!(line_of_sight(
        &t,
        [-1000.0, 0.0, 105.0],
        [-700.0, 300.0, 102.0],
        25.0
    ));
    assert!(!line_of_sight(
        &t,
        [-1000.0, 0.0, 105.0],
        [1000.0, 0.0, 102.0],
        25.0
    ));
    // high enough to see over it
    assert!(line_of_sight(
        &t,
        [-1000.0, 0.0, 600.0],
        [1000.0, 0.0, 600.0],
        25.0
    ));
}

#[test]
fn viewshed_respects_range_and_terrain_masking() {
    let open = flat(4_000.0, 80);
    let v = viewshed(&open, &site(0.0, 0.0, 1_000.0), 2.0);
    let visible = v.values.iter().filter(|x| **x > 0.0).count() as f64;
    let expected = std::f64::consts::PI * (1_000.0 / open.dx).powi(2);
    assert!(
        (visible / expected - 1.0).abs() < 0.08,
        "{visible} cells vs ~{expected}"
    );

    let t = ridge(4_000.0, 80, 200.0);
    let v = viewshed(&t, &site(-800.0, 0.0, 3_000.0), 2.0);
    assert_eq!(
        v.sample(-400.0, 0.0),
        1.0,
        "own side of the ridge is visible"
    );
    assert_eq!(v.sample(800.0, 0.0), 0.0, "far side is masked");
    let drone = viewshed(&t, &site(-800.0, 0.0, 3_000.0), 600.0);
    assert_eq!(
        drone.sample(800.0, 0.0),
        1.0,
        "a target 600 m up clears the ridge"
    );
}

#[test]
fn exposure_and_survival() {
    let t = flat(4_000.0, 80);
    let risk = RiskModel {
        sites: vec![site(0.0, 0.0, 1_000.0)],
        hazards: vec![HazardArea {
            id: "H".into(),
            name: "H".into(),
            polygon: vec![
                [1_500.0, -200.0],
                [1_900.0, -200.0],
                [1_900.0, 200.0],
                [1_500.0, 200.0],
            ],
            rate_per_min: 0.3,
            impassable: true,
        }],
    };
    let ground = exposure_field(&t, &risk, 2.0, 1.0);
    assert!(
        (ground.sample(0.0, 500.0) - 0.01).abs() < 1e-6,
        "0.6/min = 0.01/s"
    );
    assert!(
        (ground.sample(1_700.0, 0.0) - 0.005).abs() < 1e-6,
        "hazard 0.3/min"
    );
    let air = exposure_field(&t, &risk, 300.0, 0.5);
    assert_eq!(
        air.sample(1_700.0, 0.0),
        0.0,
        "hazard areas do not affect aircraft"
    );
    assert!(
        (air.sample(0.0, 500.0) - 0.005).abs() < 1e-6,
        "detectability scales the rate"
    );

    // crossing 2 km through the site's 1 km circle in 200 s at 0.01/s
    let e = segment_exposure(&ground, [-1_500.0, 0.0], [1_500.0 - 1e-9, 0.0], 300.0);
    assert!((e - 2.0).abs() < 0.1, "{e}");
    assert!((survival(e) - (-e).exp()).abs() < 1e-15);
    assert!(risk.blocks([1_000.0, 0.0], [2_500.0, 0.0], 20.0));
    assert!(!risk.blocks([1_000.0, 500.0], [2_500.0, 500.0], 20.0));
}

#[test]
fn polygon_and_polyline_helpers() {
    let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    assert!(point_in_polygon([5.0, 5.0], &sq));
    assert!(!point_in_polygon([15.0, 5.0], &sq));
    let line: Vec<[f64; 2]> = (0..=10)
        .map(|i| [i as f64, 0.01 * (i % 2) as f64])
        .collect();
    assert_eq!(simplify(&line, 0.1), vec![[0.0, 0.0], [10.0, 0.0]]);
    let corner = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
    assert_eq!(simplify(&corner, 0.1).len(), 3);
    assert!((polyline_length(&corner) - 20.0).abs() < 1e-12);
    assert_eq!(resample(&corner, 2.5).len(), 9);
}

#[test]
fn astar_finds_shortest_grid_path_and_detours() {
    let (nx, ny) = (30, 30);
    let wall = |c: (usize, usize)| c.0 == 15 && c.1 < 25;
    let cost = |a: (usize, usize), b: (usize, usize)| {
        if wall(b) {
            None
        } else {
            Some(((a.0 as f64 - b.0 as f64).powi(2) + (a.1 as f64 - b.1 as f64).powi(2)).sqrt())
        }
    };
    let h = |c: (usize, usize)| ((c.0 as f64 - 29.0).powi(2) + (c.1 as f64 - 0.0).powi(2)).sqrt();
    let (path, total) = grid_astar(nx, ny, (0, 0), (29, 0), cost, h).unwrap();
    assert_eq!(path.first(), Some(&(0, 0)));
    assert_eq!(path.last(), Some(&(29, 0)));
    assert!(path.iter().all(|c| !wall(*c)));
    assert!(path.iter().any(|c| c.1 >= 25), "must go round the wall");
    assert!(total > 29.0);
    // without the wall it is a straight line
    let (_, direct) = grid_astar(
        nx,
        ny,
        (0, 0),
        (29, 0),
        |a, b| Some(((a.0 as f64 - b.0 as f64).abs()).max((a.1 as f64 - b.1 as f64).abs())),
        |c| (29 - c.0) as f64,
    )
    .unwrap();
    assert!((direct - 29.0).abs() < 1e-9);
}

#[test]
fn dijkstra_matches_hand_example() {
    // 0 -> 1 -> 3 costs 2, 0 -> 2 -> 3 costs 5, direct 0 -> 3 costs 4
    let adj = vec![
        vec![(1, 0, 1.0), (2, 1, 1.0), (3, 2, 4.0)],
        vec![(3, 3, 1.0)],
        vec![(3, 4, 4.0)],
        vec![],
    ];
    let (edges, cost) = dijkstra(&adj, 0, 3).unwrap();
    assert_eq!(edges, vec![0, 3]);
    assert_eq!(cost, 2.0);
    assert!(dijkstra(&adj, 3, 0).is_none());
}

#[test]
fn raster_lookup() {
    let r = Raster::new(0.0, 0.0, 10.0, 3, 3, 1.0);
    assert_eq!(r.cell_of(14.0, 6.0), Some((1, 1)));
    assert_eq!(r.cell_of(-6.0, 0.0), None);
    assert_eq!(r.sample(100.0, 100.0), 0.0);
}
