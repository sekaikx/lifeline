//! # lifeline-supply
//!
//! What is on the shelf, where, until when, and who moved it.
//!
//! - **Catalogue.** Items with their class of supply (I-X), unit weight
//!   and volume, cold-chain range and transit limit, hazardous-goods
//!   number, shelf life and reorder level. Controlled items (narcotics,
//!   weapons, ammunition) need a second person to witness every movement.
//! - **Ledger.** Stock is never edited; it is the sum of an append-only
//!   ledger of receipts, issues, transfers, counts, holds and disposals.
//!   Each entry carries the SHA-256 of the previous one, so any later edit
//!   of the file breaks the chain and [`Inventory::verify`] says where.
//! - **Allocation.** Against a delivery plan, stock is picked first
//!   expiring, first out, for requests in plan priority order; lots that
//!   would expire before arrival are skipped; shortfalls are listed; and
//!   every vehicle trip gets a load manifest with its lots, weight, cold
//!   chain, hazardous and controlled flags.

mod allocate;
mod csv;
mod example;
mod ledger;

pub use allocate::{allocate, Allocation, Manifest, ManifestLine, Pick, Shortfall, TransitBreach};
pub use csv::{export_ledger, export_stock, import_stock, ImportReport};
pub use ledger::{Action, Tx, TxDraft};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Classes of supply, as used by NATO and most armed forces, with
/// humanitarian goods in their usual places.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum SupplyClass {
    /// Subsistence: food, water
    I,
    /// Clothing, tents, tools, hygiene and household kits
    II,
    /// Fuel, oil and lubricants
    III,
    /// Construction and shelter materials
    IV,
    /// Ammunition and explosives
    V,
    /// Personal items
    VI,
    /// Major end items: generators, vehicles, water purification units
    VII,
    /// Medical: drugs, blood, vaccines, consumables
    VIII,
    /// Repair parts
    IX,
    /// Other material for civil and humanitarian programmes
    #[default]
    X,
}

impl SupplyClass {
    pub const ALL: [SupplyClass; 10] = [
        SupplyClass::I,
        SupplyClass::II,
        SupplyClass::III,
        SupplyClass::IV,
        SupplyClass::V,
        SupplyClass::VI,
        SupplyClass::VII,
        SupplyClass::VIII,
        SupplyClass::IX,
        SupplyClass::X,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SupplyClass::I => "I · Food and water",
            SupplyClass::II => "II · Equipment and kits",
            SupplyClass::III => "III · Fuel",
            SupplyClass::IV => "IV · Construction",
            SupplyClass::V => "V · Ammunition",
            SupplyClass::VI => "VI · Personal",
            SupplyClass::VII => "VII · Major items",
            SupplyClass::VIII => "VIII · Medical",
            SupplyClass::IX => "IX · Repair parts",
            SupplyClass::X => "X · Civil programmes",
        }
    }

    /// Parse "VIII", "8", "class 8", "medical" and the like.
    pub fn parse(s: &str) -> Option<SupplyClass> {
        let t = s.trim().to_lowercase();
        let t = t.trim_start_matches("class").trim();
        let roman = ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];
        if let Some(k) = roman.iter().position(|r| *r == t) {
            return Some(SupplyClass::ALL[k]);
        }
        if let Ok(n) = t.parse::<usize>() {
            return (1..=10).contains(&n).then(|| SupplyClass::ALL[n - 1]);
        }
        let words: [(&str, SupplyClass); 9] = [
            ("food", SupplyClass::I),
            ("water", SupplyClass::I),
            ("fuel", SupplyClass::III),
            ("shelter", SupplyClass::IV),
            ("construction", SupplyClass::IV),
            ("ammunition", SupplyClass::V),
            ("medical", SupplyClass::VIII),
            ("health", SupplyClass::VIII),
            ("spare", SupplyClass::IX),
        ];
        words.iter().find(|(w, _)| t.contains(w)).map(|(_, c)| *c)
    }
}

/// Storage and transport temperature range for a cold-chain item.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColdChain {
    pub min_c: f64,
    pub max_c: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Stock number or SKU, unique
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub class: SupplyClass,
    /// Unit of issue: "unit", "box", "litre", "kit"...
    pub unit: String,
    /// Weight of one unit of issue, packed [kg]
    pub unit_kg: f64,
    #[serde(default)]
    pub unit_m3: Option<f64>,
    #[serde(default)]
    pub cold_chain: Option<ColdChain>,
    /// Longest the item may spend in transit [h] (cold boxes, perishables)
    #[serde(default)]
    pub max_transit_h: Option<f64>,
    /// UN number and hazard class for dangerous goods, e.g. "UN1203 class 3"
    #[serde(default)]
    pub hazmat: Option<String>,
    /// Movements need a second person as witness
    #[serde(default)]
    pub controlled: bool,
    /// Alert when available stock falls to this many units
    #[serde(default)]
    pub reorder_level: f64,
    #[serde(default)]
    pub notes: String,
}

impl Item {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err("an item needs a stock number and a name".into());
        }
        if !(self.unit_kg > 0.0 && self.unit_kg.is_finite()) {
            return Err(format!("{}: unit weight must be positive", self.name));
        }
        if self.max_transit_h.is_some_and(|h| !(h > 0.0)) {
            return Err(format!("{}: transit limit must be positive", self.name));
        }
        if self.cold_chain.is_some_and(|c| !(c.min_c <= c.max_c)) {
            return Err(format!("{}: cold chain minimum above maximum", self.name));
        }
        if !(self.reorder_level >= 0.0) {
            return Err(format!("{}: reorder level must not be negative", self.name));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    #[default]
    Depot,
    Warehouse,
    ForwardPoint,
    Facility,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: LocationKind,
}

/// Stock of one lot at one place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StockLine {
    pub item: String,
    pub lot: String,
    /// `YYYY-MM-DD`
    pub expiry: Option<String>,
    pub location: String,
    pub available: f64,
    /// On hold: quarantined, damaged or awaiting inspection
    pub held: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Alert {
    /// Available stock at or under the reorder level
    Low {
        item: String,
        available: f64,
        reorder_level: f64,
    },
    /// Expires within the warning window
    Expiring {
        item: String,
        lot: String,
        location: String,
        expiry: String,
        days: i64,
        quantity: f64,
    },
    /// Past its expiry date and still counted as available
    Expired {
        item: String,
        lot: String,
        location: String,
        expiry: String,
        quantity: f64,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub locations: Vec<Location>,
    #[serde(default)]
    pub ledger: Vec<Tx>,
}

/// Everything the UI shows at once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub items: Vec<Item>,
    pub locations: Vec<Location>,
    pub stock: Vec<StockLine>,
    pub alerts: Vec<Alert>,
    /// Newest entries first, at most 500
    pub recent: Vec<Tx>,
    pub ledger_len: usize,
    /// `None` when the chain checks out, else what is wrong
    pub integrity: Option<String>,
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` date.
pub fn days(date: &str) -> Option<i64> {
    let mut it = date.trim().get(..10)?.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

impl Inventory {
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn location(&self, id: &str) -> Option<&Location> {
        self.locations.iter().find(|l| l.id == id)
    }

    /// Add or replace a catalogue item.
    pub fn upsert_item(&mut self, item: Item) -> Result<(), String> {
        item.validate()?;
        match self.items.iter_mut().find(|i| i.id == item.id) {
            Some(i) => *i = item,
            None => self.items.push(item),
        }
        Ok(())
    }

    pub fn upsert_location(&mut self, loc: Location) -> Result<(), String> {
        if loc.id.trim().is_empty() || loc.name.trim().is_empty() {
            return Err("a location needs an id and a name".into());
        }
        match self.locations.iter_mut().find(|l| l.id == loc.id) {
            Some(l) => *l = loc,
            None => self.locations.push(loc),
        }
        Ok(())
    }

    /// Stock by lot and place, from the ledger. Lines that have gone to
    /// zero are left out.
    pub fn stock(&self) -> Vec<StockLine> {
        type Key = (String, String, String);
        fn at<'a>(
            by: &'a mut BTreeMap<Key, StockLine>,
            t: &Tx,
            loc: Option<&str>,
        ) -> &'a mut StockLine {
            let loc = loc.unwrap_or_default().to_string();
            by.entry((t.item.clone(), t.lot.clone(), loc.clone()))
                .or_insert_with(|| StockLine {
                    item: t.item.clone(),
                    lot: t.lot.clone(),
                    expiry: t.expiry.clone(),
                    location: loc,
                    available: 0.0,
                    held: 0.0,
                })
        }
        let mut by: BTreeMap<Key, StockLine> = BTreeMap::new();
        for t in &self.ledger {
            let (from, to) = (t.from.as_deref(), t.to.as_deref());
            match t.action {
                Action::Receive => at(&mut by, t, to).available += t.qty,
                Action::Issue | Action::Dispose => at(&mut by, t, from).available -= t.qty,
                Action::Transfer => {
                    at(&mut by, t, from).available -= t.qty;
                    at(&mut by, t, to).available += t.qty;
                }
                Action::Adjust => at(&mut by, t, to.or(from)).available += t.qty,
                Action::Hold => {
                    let l = at(&mut by, t, from);
                    l.available -= t.qty;
                    l.held += t.qty;
                }
                Action::Release => {
                    let l = at(&mut by, t, from);
                    l.held -= t.qty;
                    l.available += t.qty;
                }
                Action::DisposeHeld => at(&mut by, t, from).held -= t.qty,
            }
        }
        by.into_values()
            .filter(|l| l.available.abs() > 1e-9 || l.held.abs() > 1e-9)
            .collect()
    }

    /// Available units of `item` at `location` (all locations if `None`).
    pub fn available(&self, item: &str, location: Option<&str>) -> f64 {
        self.stock()
            .iter()
            .filter(|l| l.item == item && location.is_none_or(|loc| l.location == loc))
            .map(|l| l.available)
            .sum()
    }

    /// Low stock, and lots expiring within `warn_days` of `today`.
    pub fn alerts(&self, today: &str, warn_days: i64) -> Vec<Alert> {
        let mut out = Vec::new();
        let stock = self.stock();
        for item in &self.items {
            let available: f64 = stock
                .iter()
                .filter(|l| l.item == item.id)
                .map(|l| l.available)
                .sum();
            if item.reorder_level > 0.0 && available <= item.reorder_level {
                out.push(Alert::Low {
                    item: item.id.clone(),
                    available,
                    reorder_level: item.reorder_level,
                });
            }
        }
        let Some(now) = days(today) else {
            return out;
        };
        for l in stock.iter().filter(|l| l.available > 0.0) {
            let Some(exp) = l.expiry.as_deref() else {
                continue;
            };
            let Some(d) = days(exp) else { continue };
            if d < now {
                out.push(Alert::Expired {
                    item: l.item.clone(),
                    lot: l.lot.clone(),
                    location: l.location.clone(),
                    expiry: exp.to_string(),
                    quantity: l.available,
                });
            } else if d - now <= warn_days {
                out.push(Alert::Expiring {
                    item: l.item.clone(),
                    lot: l.lot.clone(),
                    location: l.location.clone(),
                    expiry: exp.to_string(),
                    days: d - now,
                    quantity: l.available,
                });
            }
        }
        out
    }

    pub fn snapshot(&self, today: &str) -> Snapshot {
        Snapshot {
            items: self.items.clone(),
            locations: self.locations.clone(),
            stock: self.stock(),
            alerts: self.alerts(today, 30),
            recent: self.ledger.iter().rev().take(500).cloned().collect(),
            ledger_len: self.ledger.len(),
            integrity: self.verify().err(),
        }
    }

    /// Weight [kg] and tightest transit limit [h] of a request's lines.
    pub fn weigh(
        &self,
        lines: &[lifeline_planner::RequestLine],
    ) -> Result<(f64, Option<f64>), String> {
        let mut kg = 0.0;
        let mut limit: Option<f64> = None;
        for l in lines {
            let item = self
                .item(&l.item)
                .ok_or_else(|| format!("unknown item {}", l.item))?;
            kg += item.unit_kg * l.quantity;
            if let Some(h) = item.max_transit_h {
                limit = Some(limit.map_or(h, |x| x.min(h)));
            }
        }
        Ok((kg, limit))
    }

    /// Read a stored inventory.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Write atomically: a crash mid-write leaves the previous file.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.part");
        let text = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Where the app keeps the inventory: `supply.json` in the platform
    /// data directory.
    pub fn default_path() -> Option<std::path::PathBuf> {
        std::env::var_os("LIFELINE_SUPPLY")
            .map(std::path::PathBuf::from)
            .or_else(|| dirs::data_dir().map(|d| d.join("lifeline").join("supply.json")))
    }

    pub fn example(today: &str) -> Self {
        example::inventory(today)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(days("1970-01-01"), Some(0));
        assert_eq!(days("2000-03-01"), Some(11_017));
        assert_eq!(days("2026-10-16T12:00:00Z"), Some(20_742));
        assert_eq!(days("2026-13-01"), None);
        assert_eq!(days("soon"), None);
    }

    #[test]
    fn classes_parse() {
        assert_eq!(SupplyClass::parse("VIII"), Some(SupplyClass::VIII));
        assert_eq!(SupplyClass::parse("class 3"), Some(SupplyClass::III));
        assert_eq!(
            SupplyClass::parse("Medical consumables"),
            Some(SupplyClass::VIII)
        );
        assert_eq!(SupplyClass::parse("11"), None);
    }
}
