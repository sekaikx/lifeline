//! Parsers tested against recorded responses (no network).

use lifeline_core::wind::Wind;
use lifeline_geodata::{
    network_from_overpass, parse_iso_utc, profile_from_met_norway, profile_from_open_meteo,
    LocalFrame,
};

const OVERPASS: &[u8] = include_bytes!("fixtures/overpass-gorkha.json");
const MET: &[u8] = include_bytes!("fixtures/met-norway-gorkha.json");
const OPEN_METEO: &[u8] = include_bytes!("fixtures/open-meteo-sample.json");

fn layers(w: &Wind) -> Vec<(f64, f64, f64)> {
    match w {
        Wind::Layered { layers } => layers
            .iter()
            .map(|l| (l.altitude, l.speed, l.from_deg))
            .collect(),
        other => panic!("expected layered wind, got {other:?}"),
    }
}

#[test]
fn osm_ways_become_a_graph_split_at_junctions() {
    let frame = LocalFrame::new(27.997, 84.628);
    let (net, places) = network_from_overpass(OVERPASS, &frame, 1_000.0).unwrap();
    assert!(net.roads.len() >= 60, "{} roads", net.roads.len());
    assert!(net.towns.len() > 50);
    // every road starts and ends on a graph node
    for r in &net.roads {
        let (a, b) = (&net.towns[r.a], &net.towns[r.b]);
        assert_eq!(r.points[0], [a.x, a.y]);
        assert_eq!(*r.points.last().unwrap(), [b.x, b.y]);
        assert_ne!(r.a, r.b);
    }
    // junctions have several roads; most of the graph hangs together
    let mut degree = vec![0; net.towns.len()];
    for r in &net.roads {
        degree[r.a] += 1;
        degree[r.b] += 1;
    }
    assert!(
        degree.iter().filter(|d| **d >= 3).count() > 10,
        "real junctions expected"
    );
    // clipped to the area, in local metres around the frame origin
    assert!(net
        .towns
        .iter()
        .all(|t| t.x.abs() <= 1_000.0 && t.y.abs() <= 1_000.0));
    assert!(net
        .roads
        .iter()
        .flat_map(|r| &r.points)
        .all(|p| p[0].abs() <= 1_000.0 && p[1].abs() <= 1_000.0));
    let (unclipped, _) = network_from_overpass(OVERPASS, &frame, 1e9).unwrap();
    // whole ways reach kilometres beyond the box until clipped
    let reach = |n: &lifeline_ground::RoadNetwork| {
        n.towns
            .iter()
            .map(|t| t.x.abs().max(t.y.abs()))
            .fold(0.0, f64::max)
    };
    assert!(reach(&unclipped) > 5_000.0 && reach(&net) <= 1_000.0);
    assert!(
        places
            .iter()
            .any(|p| p.name == "Gorkha" && p.kind == "city"),
        "{places:?}"
    );
}

#[test]
fn met_norway_profile_is_extrapolated_from_surface() {
    let t0 = parse_iso_utc("2026-10-14T21:00:00Z").unwrap();
    let f = profile_from_met_norway(MET, t0).unwrap();
    assert_eq!(f.valid_unix, t0);
    let l = layers(&f.wind);
    assert!((l[0].0 - (f.elevation + 10.0)).abs() < 1e-9);
    // 1/7 power law: speed at 2 km AGL is (200)^(1/7) = 2.13 times the 10 m wind
    let ratio = l.last().unwrap().1 / l[0].1;
    assert!((ratio - 200f64.powf(1.0 / 7.0)).abs() < 1e-9);
    assert!(l.windows(2).all(|w| w[1].0 > w[0].0 && w[1].2 == w[0].2));
    assert!(f.source.contains("MET Norway"));
}

#[test]
fn open_meteo_profile_merges_heights_and_pressure_levels() {
    let f = profile_from_open_meteo(OPEN_METEO, 1_790_000_100).unwrap();
    assert_eq!(f.valid_unix, 1_790_000_000);
    let l = layers(&f.wind);
    let alts: Vec<f64> = l.iter().map(|x| x.0).collect();
    // 10/80/120/180 m above the 1250 m grid point, then 850 and 700 hPa;
    // 1000 hPa (110 m) is underground and 500 hPa is missing at this hour
    assert_eq!(alts, vec![1260.0, 1330.0, 1370.0, 1430.0, 1495.0, 3120.0]);
    assert_eq!(l[5], (3120.0, 11.0, 280.0));
}

#[test]
fn malformed_responses_are_errors() {
    let frame = LocalFrame::new(0.0, 0.0);
    assert!(network_from_overpass(b"<html>busy</html>", &frame, 1e3).is_err());
    assert!(profile_from_met_norway(b"{}", 0).is_err());
    assert!(profile_from_open_meteo(b"{\"elevation\": 1, \"hourly\": {}}", 0).is_err());
}
