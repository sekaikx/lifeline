//! Demo stock for a first run of the Supply workspace: a main depot, a cold
//! store and a forward point, with lots that are expiring, expired and on
//! hold. Everything goes through the ledger, so the chain verifies.

use crate::{
    days, Action, ColdChain, Inventory, Item, Location, LocationKind, SupplyClass, TxDraft,
};

/// Demo stock as of `today` (`YYYY-MM-DD`).
pub(crate) fn inventory(today: &str) -> Inventory {
    let mut inv = Inventory::default();
    for (id, name, kind) in [
        ("depot", "Main depot", LocationKind::Depot),
        ("cold", "Cold store", LocationKind::Warehouse),
        ("fwd", "Forward supply point", LocationKind::ForwardPoint),
    ] {
        inv.upsert_location(Location {
            id: id.into(),
            name: name.into(),
            kind,
        })
        .expect("demo locations are valid");
    }

    let items = [
        Item {
            cold_chain: Some(ColdChain {
                min_c: 2.0,
                max_c: 6.0,
            }),
            max_transit_h: Some(4.0),
            reorder_level: 20.0,
            notes:
                "Keep at 2-6 °C in a validated cold box. Discard if out of range for more than 4 h."
                    .into(),
            ..plain(
                "MED-BLD-ONEG",
                "Red blood cells, O-negative",
                SupplyClass::VIII,
                "unit",
                0.5,
            )
        },
        Item {
            cold_chain: Some(ColdChain {
                min_c: 2.0,
                max_c: 8.0,
            }),
            max_transit_h: Some(12.0),
            reorder_level: 30.0,
            notes: "Keep at 2-8 °C and never freeze. Use opened vials within 6 h.".into(),
            ..plain(
                "MED-VAC-MR",
                "Measles-rubella vaccine, 10-dose vial",
                SupplyClass::VIII,
                "vial pack",
                0.4,
            )
        },
        Item {
            controlled: true,
            reorder_level: 5.0,
            notes: "Controlled drug: double lock. Every movement needs a witness.".into(),
            ..plain(
                "MED-OPI-MOR10",
                "Morphine 10 mg/ml injection, 1 ml ampoule",
                SupplyClass::VIII,
                "ampoule box",
                0.2,
            )
        },
        Item {
            reorder_level: 40.0,
            notes: "Store dry and away from direct sun.".into(),
            ..plain(
                "MED-DRS-TRM",
                "Trauma dressing, sterile, 10 x 10 cm",
                SupplyClass::VIII,
                "box",
                1.5,
            )
        },
        Item {
            reorder_level: 50.0,
            notes: "Store dry, below 30 °C. Check expiry at every issue.".into(),
            ..plain(
                "MED-ORS-SAC",
                "Oral rehydration salts, 1 L sachets",
                SupplyClass::VIII,
                "box",
                4.0,
            )
        },
        Item {
            reorder_level: 150.0,
            notes: "Stack no more than six cases high, out of the sun.".into(),
            ..plain(
                "SUB-WTR-12",
                "Bottled drinking water, 1 L",
                SupplyClass::I,
                "case 12x1L",
                12.5,
            )
        },
        Item {
            reorder_level: 40.0,
            notes: "Store below 25 °C. Issue first expiring, first out.".into(),
            ..plain(
                "SUB-RTE-9K",
                "Ready-to-eat ration, 2,400 kcal",
                SupplyClass::I,
                "case",
                9.0,
            )
        },
        Item {
            reorder_level: 60.0,
            notes: "Keep sealed against damp and pests.".into(),
            ..plain(
                "SUB-BIS-HE",
                "High-energy biscuits",
                SupplyClass::I,
                "carton",
                10.0,
            )
        },
        Item {
            reorder_level: 50.0,
            notes: "Keep dry. Do not store near fuel or chemicals.".into(),
            ..plain(
                "SUB-PUR-TAB",
                "Water purification tablets, 50s",
                SupplyClass::I,
                "box",
                0.5,
            )
        },
        Item {
            unit_m3: Some(0.5),
            reorder_level: 30.0,
            notes: "Check poles and pegs on receipt.".into(),
            ..plain(
                "EQP-TNT-FAM",
                "Family tent, 4 persons",
                SupplyClass::II,
                "unit",
                32.0,
            )
        },
        Item {
            reorder_level: 60.0,
            notes: "Store rolled or flat. Inspect for tears at issue.".into(),
            ..plain(
                "EQP-TRP-4X5",
                "Tarpaulin, 4 x 5 m, polyethylene",
                SupplyClass::II,
                "unit",
                4.0,
            )
        },
        Item {
            reorder_level: 100.0,
            notes: "Soap, jerrycan, towels and toothbrushes. Keep dry.".into(),
            ..plain(
                "EQP-HYG-KIT",
                "Family hygiene kit",
                SupplyClass::II,
                "kit",
                2.2,
            )
        },
        Item {
            reorder_level: 15.0,
            notes: "Ten blankets per bale. Keep under cover.".into(),
            ..plain(
                "EQP-BLK-WL",
                "Wool blanket",
                SupplyClass::II,
                "bale 10",
                15.0,
            )
        },
        Item {
            hazmat: Some("UN1202 class 3".into()),
            reorder_level: 30.0,
            notes: "Store outdoors in a ventilated, bunded area away from ignition sources.".into(),
            ..plain(
                "FUE-DSL-20L",
                "Diesel fuel, 20 L jerrycan",
                SupplyClass::III,
                "jerrycan 20L",
                18.0,
            )
        },
        Item {
            notes: "Service every 250 running hours.".into(),
            ..plain(
                "MAJ-GEN-5KVA",
                "Petrol generator, 5 kVA",
                SupplyClass::VII,
                "unit",
                85.0,
            )
        },
        Item {
            unit_m3: Some(1.2),
            notes: "Needs fuel and a trained operator.".into(),
            ..plain(
                "MAJ-WPU-120",
                "Water purification unit, 5 m3/h",
                SupplyClass::VII,
                "unit",
                120.0,
            )
        },
    ];
    for item in items {
        inv.upsert_item(item).expect("demo items are valid");
    }

    // Opening stock: one ledger entry per receipt, transfer or hold, with
    // timestamps ordered by entry number.
    let base = (base_day(today) - 1) * 86_400 + 8 * 3_600;
    let mut k = 0;
    let mut post = |inv: &mut Inventory, d: TxDraft| {
        k += 1;
        inv.apply_at(d, base + k * 60)
            .expect("opening stock is valid");
    };
    let exp = |offset: i64| Some(shift(today, offset));

    // Cold store
    post(
        &mut inv,
        receipt("MED-BLD-ONEG", "RBC-0412", exp(6), 24.0, "cold"),
    );
    post(
        &mut inv,
        receipt("MED-BLD-ONEG", "RBC-0418", exp(34), 40.0, "cold"),
    );
    post(
        &mut inv,
        receipt("MED-VAC-MR", "MR-2412", exp(16), 80.0, "cold"),
    );
    post(
        &mut inv,
        receipt("MED-VAC-MR", "MR-2611", exp(420), 150.0, "cold"),
    );

    // Main depot: medical
    post(
        &mut inv,
        TxDraft {
            witness: Some("Duty officer".into()),
            ..receipt("MED-OPI-MOR10", "MOR-2513", exp(548), 60.0, "depot")
        },
    );
    post(
        &mut inv,
        receipt("MED-DRS-TRM", "TDR-2601", exp(600), 120.0, "depot"),
    );
    post(
        &mut inv,
        receipt("MED-ORS-SAC", "ORS-2409", exp(-5), 25.0, "depot"),
    );
    post(
        &mut inv,
        receipt("MED-ORS-SAC", "ORS-2604", exp(300), 180.0, "depot"),
    );

    // Food and water
    post(
        &mut inv,
        receipt("SUB-WTR-12", "WTR-2701", exp(420), 300.0, "depot"),
    );
    post(
        &mut inv,
        receipt("SUB-WTR-12", "WTR-2701", exp(420), 60.0, "fwd"),
    );
    post(
        &mut inv,
        receipt("SUB-RTE-9K", "RTE-2510", exp(11), 90.0, "depot"),
    );
    post(
        &mut inv,
        receipt("SUB-RTE-9K", "RTE-2609", exp(630), 260.0, "depot"),
    );
    post(
        &mut inv,
        receipt("SUB-BIS-HE", "BIS-2510", exp(19), 70.0, "depot"),
    );
    post(
        &mut inv,
        receipt("SUB-BIS-HE", "BIS-2703", exp(400), 150.0, "depot"),
    );
    post(
        &mut inv,
        receipt("SUB-PUR-TAB", "WPT-2603", exp(700), 120.0, "depot"),
    );
    post(
        &mut inv,
        receipt("SUB-PUR-TAB", "WPT-2603", exp(700), 30.0, "fwd"),
    );

    // Shelter, hygiene and fuel
    post(
        &mut inv,
        receipt("EQP-TNT-FAM", "TNT-2409", None, 45.0, "depot"),
    );
    post(
        &mut inv,
        receipt("EQP-TRP-4X5", "TRP-2402", None, 150.0, "depot"),
    );
    post(
        &mut inv,
        receipt("EQP-TRP-4X5", "TRP-2507", None, 40.0, "depot"),
    );
    post(
        &mut inv,
        receipt("EQP-HYG-KIT", "HYG-2507", None, 300.0, "depot"),
    );
    post(
        &mut inv,
        receipt("EQP-HYG-KIT", "HYG-2507", None, 120.0, "fwd"),
    );
    post(
        &mut inv,
        receipt("EQP-BLK-WL", "BLK-2302", None, 12.0, "depot"),
    );
    post(
        &mut inv,
        receipt("FUE-DSL-20L", "DSL-2601", exp(330), 200.0, "depot"),
    );

    // Major items
    post(
        &mut inv,
        receipt("MAJ-GEN-5KVA", "GEN-2404", None, 4.0, "depot"),
    );
    post(
        &mut inv,
        receipt("MAJ-WPU-120", "WPU-2311", None, 2.0, "depot"),
    );

    // Move the expiring rations forward, first expiring, first out.
    post(
        &mut inv,
        TxDraft {
            action: Action::Transfer,
            item: "SUB-RTE-9K".into(),
            lot: "RTE-2510".into(),
            expiry: None,
            from: Some("depot".into()),
            to: Some("fwd".into()),
            qty: 30.0,
            reference: Some("Opening stock".into()),
            user: "Depot team".into(),
            witness: None,
            note: String::new(),
        },
    );

    // Tarpaulins from one lot are quarantined after a flood.
    post(
        &mut inv,
        TxDraft {
            action: Action::Hold,
            item: "EQP-TRP-4X5".into(),
            lot: "TRP-2507".into(),
            expiry: None,
            from: Some("depot".into()),
            to: None,
            qty: 40.0,
            reference: None,
            user: "Depot team".into(),
            witness: None,
            note: "Water damage, awaiting inspection".into(),
        },
    );

    inv
}

/// An item with no optional fields set; callers override what they need.
fn plain(id: &str, name: &str, class: SupplyClass, unit: &str, unit_kg: f64) -> Item {
    Item {
        id: id.into(),
        name: name.into(),
        class,
        unit: unit.into(),
        unit_kg,
        unit_m3: None,
        cold_chain: None,
        max_transit_h: None,
        hazmat: None,
        controlled: false,
        reorder_level: 0.0,
        notes: String::new(),
    }
}

/// A goods-in entry for `qty` of a lot at `to`.
fn receipt(item: &str, lot: &str, expiry: Option<String>, qty: f64, to: &str) -> TxDraft {
    TxDraft {
        action: Action::Receive,
        item: item.into(),
        lot: lot.into(),
        expiry,
        from: None,
        to: Some(to.into()),
        qty,
        reference: Some("Opening stock".into()),
        user: "Depot team".into(),
        witness: None,
        note: String::new(),
    }
}

/// Days since 1970-01-01 for `today`, or for 2026-01-01 if it does not parse.
fn base_day(today: &str) -> i64 {
    days(today).or_else(|| days("2026-01-01")).unwrap_or(0)
}

/// `today` moved by `days_offset` days, as `YYYY-MM-DD`.
fn shift(today: &str, days_offset: i64) -> String {
    civil(base_day(today) + days_offset)
}

/// `YYYY-MM-DD` for a count of days since 1970-01-01 (Howard Hinnant's
/// civil_from_days, the inverse of [`days`]).
fn civil(z: i64) -> String {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Alert;

    #[test]
    fn example_ledger_verifies_and_alerts() {
        let today = "2026-10-16";
        let inv = inventory(today);
        assert!(inv.verify().is_ok());
        assert!(inv.items.iter().all(|i| i.validate().is_ok()));
        for loc in ["depot", "cold", "fwd"] {
            assert!(
                inv.stock().iter().any(|l| l.location == loc),
                "no stock at {loc}"
            );
        }
        let alerts = inv.alerts(today, 30);
        assert!(alerts.iter().any(|a| matches!(a, Alert::Expired { .. })));
        assert!(alerts.iter().any(|a| matches!(a, Alert::Expiring { .. })));
        assert!(inv.available("MED-OPI-MOR10", None) > 0.0);
        assert!(inv.stock().iter().any(|l| l.held > 0.0));
    }

    #[test]
    fn dates_shift_across_months_and_years() {
        assert_eq!(shift("2026-02-27", 2), "2026-03-01");
        assert_eq!(shift("2024-12-31", 1), "2025-01-01");
        assert_eq!(shift("2026-10-16", 0), "2026-10-16");
        assert_eq!(shift("2026-10-16", -5), "2026-10-11");
        assert_eq!(shift("soon", 0), "2026-01-01");
    }
}
