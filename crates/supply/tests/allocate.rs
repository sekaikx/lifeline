//! Stock against a real plan on the generated valley.

use lifeline_planner::*;
use lifeline_supply::{allocate, Inventory};

const TODAY: &str = "2026-10-16";

fn line(item: &str, quantity: f64) -> RequestLine {
    RequestLine {
        item: item.into(),
        quantity,
    }
}

/// The example scenario with item lines on three requests; demand and the
/// transit limit follow from the lines, as the app keeps them.
fn scenario(inv: &Inventory) -> PlanScenario {
    let world = World::generate(GeneratedSpec::default()).unwrap();
    let mut s = PlanScenario::example(&world);
    let lines = [
        // R4, blood and vaccines: more blood than the first lot holds
        (
            "R4",
            vec![line("MED-BLD-ONEG", 30.0), line("MED-VAC-MR", 10.0)],
        ),
        // R1, field clinic: dressings, and more ORS than is in date
        (
            "R1",
            vec![line("MED-DRS-TRM", 40.0), line("MED-ORS-SAC", 200.0)],
        ),
        // R3, shelter: tents and the blankets of which there are only 12
        (
            "R3",
            vec![line("EQP-TNT-FAM", 20.0), line("EQP-BLK-WL", 20.0)],
        ),
    ];
    for (id, items) in lines {
        let r = s.requests.iter_mut().find(|r| r.id == id).unwrap();
        let (kg, limit) = inv.weigh(&items).unwrap();
        r.demand_kg = kg;
        r.max_transit_h = limit;
        r.items = items;
    }
    s
}

fn sites() -> Vec<String> {
    vec!["depot".into(), "cold".into()]
}

#[test]
fn picks_first_expiring_in_date_stock_and_reports_shortfalls() {
    let inv = Inventory::example(TODAY);
    assert!(inv.verify().is_ok());
    let s = scenario(&inv);
    let p = plan(&World::generate(GeneratedSpec::default()).unwrap(), &s).unwrap();
    let a = allocate(&inv, &s, &p, &sites(), TODAY).unwrap();

    // blood: the lot expiring in 6 days goes first, the rest from the next
    let blood: Vec<_> = a
        .picks
        .iter()
        .filter(|k| k.item == "MED-BLD-ONEG")
        .collect();
    assert_eq!(blood[0].lot, "RBC-0412");
    assert_eq!(blood[0].location, "cold");
    assert_eq!(blood.iter().map(|k| k.qty).sum::<f64>(), 30.0);

    // ORS: the expired lot is never picked; 180 in date for 200 asked
    assert!(a.picks.iter().all(|k| k.lot != "ORS-2409"));
    let ors = a
        .shortfalls
        .iter()
        .find(|x| x.item == "MED-ORS-SAC")
        .unwrap();
    assert_eq!((ors.needed, ors.picked), (200.0, 180.0));
    assert!(ors.reason.contains("out of date"), "{}", ors.reason);

    // blankets: 12 of 20
    let blk = a
        .shortfalls
        .iter()
        .find(|x| x.item == "EQP-BLK-WL")
        .unwrap();
    assert_eq!(blk.picked, 12.0);

    // every picked kilogram is on exactly one manifest
    let picked: f64 = a.picks.iter().map(|k| k.kg).sum();
    let loaded: f64 = a.manifests.iter().map(|m| m.kg).sum();
    assert!((picked - loaded).abs() < 1e-6, "{picked} vs {loaded}");
    // the blood flight is flagged as cold chain
    assert!(a
        .manifests
        .iter()
        .any(|m| m.request_id == "R4" && m.cold_chain));
    assert!(a.transit_breaches.is_empty(), "{:?}", a.transit_breaches);
}

#[test]
fn the_planner_respects_a_transit_limit() {
    let inv = Inventory::example(TODAY);
    let world = World::generate(GeneratedSpec::default()).unwrap();
    let mut s = scenario(&inv);
    // force blood onto a truck: four hours is not enough by road here,
    // so the request cannot be served rather than spoiling on the way
    let r4 = s.requests.iter_mut().find(|r| r.id == "R4").unwrap();
    r4.mode = ModeChoice::Truck;
    r4.max_transit_h = Some(0.1);
    let p = plan(&world, &s).unwrap();
    let a4 = p.assignments.iter().find(|a| a.request_id == "R4").unwrap();
    assert_eq!(a4.mode, None);
    assert!(a4.reason.as_deref().unwrap_or("").contains("in transit"));
    let a = allocate(&inv, &s, &p, &sites(), TODAY).unwrap();
    assert!(a.unserved.contains(&"R4".to_string()));
    assert!(a.picks.iter().all(|k| k.request_id != "R4"));
}

#[test]
fn rejects_unknown_locations_and_dates() {
    let inv = Inventory::example(TODAY);
    let s = scenario(&inv);
    let p = plan(&World::generate(GeneratedSpec::default()).unwrap(), &s).unwrap();
    assert!(allocate(&inv, &s, &p, &["nowhere".into()], TODAY).is_err());
    assert!(allocate(&inv, &s, &p, &sites(), "today").is_err());
    assert!(allocate(&inv, &s, &p, &[], TODAY).is_err());
}
