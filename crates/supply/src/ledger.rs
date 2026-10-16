//! The append-only stock ledger.

use crate::{days, Inventory};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Goods in, to `to`
    Receive,
    /// Goods out of `from` to a request or unit (`reference`)
    Issue,
    /// From one location to another
    Transfer,
    /// Stock count correction at `to` (`qty` may be negative)
    Adjust,
    /// Put available stock at `from` on hold (quarantine, damage, recall)
    Hold,
    /// Release held stock at `from` back to available
    Release,
    /// Write off available stock at `from` (expired, destroyed)
    Dispose,
    /// Write off held stock at `from`
    DisposeHeld,
}

/// A ledger entry as the operator enters it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TxDraft {
    pub action: Action,
    pub item: String,
    #[serde(default)]
    pub lot: String,
    #[serde(default)]
    pub expiry: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    pub qty: f64,
    /// Request, waybill or order number
    #[serde(default)]
    pub reference: Option<String>,
    pub user: String,
    /// Second person, required for controlled items
    #[serde(default)]
    pub witness: Option<String>,
    #[serde(default)]
    pub note: String,
}

/// A committed ledger entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tx {
    pub seq: u64,
    /// Unix seconds
    pub ts: i64,
    pub action: Action,
    pub item: String,
    pub lot: String,
    pub expiry: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub qty: f64,
    pub reference: Option<String>,
    pub user: String,
    pub witness: Option<String>,
    pub note: String,
    /// Hash of the previous entry (zeros for the first)
    pub prev: String,
    /// SHA-256 of this entry's content and `prev`
    pub hash: String,
}

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

impl Tx {
    fn digest(&self) -> String {
        // a fixed field order, independent of serde's, so the chain does not
        // depend on how the struct is serialised
        let fields = serde_json::json!([
            self.seq,
            self.ts,
            self.action,
            self.item,
            self.lot,
            self.expiry,
            self.from,
            self.to,
            format!("{:.6}", self.qty),
            self.reference,
            self.user,
            self.witness,
            self.note,
            self.prev,
        ]);
        let mut h = Sha256::new();
        h.update(fields.to_string().as_bytes());
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Inventory {
    /// Check and commit an entry; returns it as recorded.
    pub fn apply(&mut self, d: TxDraft) -> Result<Tx, String> {
        self.apply_at(d, now())
    }

    pub fn apply_at(&mut self, d: TxDraft, ts: i64) -> Result<Tx, String> {
        let item = self
            .item(&d.item)
            .ok_or_else(|| format!("unknown item {}", d.item))?
            .clone();
        let user = d.user.trim();
        if user.is_empty() {
            return Err("enter who is making this entry".into());
        }
        let witness = d
            .witness
            .as_deref()
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(str::to_string);
        if item.controlled {
            match &witness {
                None => {
                    return Err(format!(
                        "{} is a controlled item: a second person must witness the entry",
                        item.name
                    ))
                }
                Some(w) if w.eq_ignore_ascii_case(user) => {
                    return Err("the witness must be a different person".into())
                }
                _ => {}
            }
        }
        if !d.qty.is_finite() || (d.action != Action::Adjust && !(d.qty > 0.0)) {
            return Err("the quantity must be positive".into());
        }
        if d.action == Action::Adjust && d.qty == 0.0 {
            return Err("an adjustment of zero changes nothing".into());
        }
        if let Some(e) = &d.expiry {
            if days(e).is_none() {
                return Err(format!("expiry {e} is not a date (use YYYY-MM-DD)"));
            }
        }
        let loc = |l: &Option<String>, what: &str| -> Result<String, String> {
            let id = l
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("choose the {what} location"))?;
            self.location(id)
                .map(|x| x.id.clone())
                .ok_or_else(|| format!("unknown location {id}"))
        };
        let (from, to) = match d.action {
            Action::Receive => (None, Some(loc(&d.to, "receiving")?)),
            Action::Transfer => {
                let (f, t) = (loc(&d.from, "source")?, loc(&d.to, "destination")?);
                if f == t {
                    return Err("source and destination are the same".into());
                }
                (Some(f), Some(t))
            }
            Action::Adjust => (
                None,
                Some(loc(&d.to.clone().or(d.from.clone()), "counted")?),
            ),
            _ => (Some(loc(&d.from, "source")?), None),
        };
        // the lot's recorded expiry wins, so one lot never has two dates
        let stock = self.stock();
        let existing = stock
            .iter()
            .find(|l| l.item == d.item && l.lot == d.lot.trim());
        let expiry = existing.and_then(|l| l.expiry.clone()).or(d
            .expiry
            .clone()
            .map(|e| e.trim()[..10.min(e.trim().len())].to_string()));
        let here = |at: &str| {
            stock
                .iter()
                .find(|l| l.item == d.item && l.lot == d.lot.trim() && l.location == at)
        };
        let short = |have: f64, what: &str| -> Result<(), String> {
            if d.qty > have + 1e-9 {
                Err(format!(
                    "only {have} {} {what} in lot {} of {}",
                    item.unit,
                    if d.lot.trim().is_empty() {
                        "-"
                    } else {
                        d.lot.trim()
                    },
                    item.name
                ))
            } else {
                Ok(())
            }
        };
        match d.action {
            Action::Issue | Action::Transfer | Action::Hold | Action::Dispose => short(
                here(from.as_deref().unwrap_or_default()).map_or(0.0, |l| l.available),
                "available",
            )?,
            Action::Release | Action::DisposeHeld => short(
                here(from.as_deref().unwrap_or_default()).map_or(0.0, |l| l.held),
                "on hold",
            )?,
            Action::Adjust if d.qty < 0.0 => {
                let have = here(to.as_deref().unwrap_or_default()).map_or(0.0, |l| l.available);
                if -d.qty > have + 1e-9 {
                    return Err(format!("the count cannot go below zero (have {have})"));
                }
            }
            _ => {}
        }
        if matches!(
            d.action,
            Action::Adjust | Action::Dispose | Action::DisposeHeld
        ) && d.note.trim().is_empty()
        {
            return Err("give a reason for adjustments and write-offs".into());
        }
        let prev = self
            .ledger
            .last()
            .map_or_else(|| GENESIS.to_string(), |t| t.hash.clone());
        let mut tx = Tx {
            seq: self.ledger.last().map_or(1, |t| t.seq + 1),
            ts,
            action: d.action,
            item: d.item.clone(),
            lot: d.lot.trim().to_string(),
            expiry,
            from,
            to,
            qty: d.qty,
            reference: d
                .reference
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty()),
            user: user.to_string(),
            witness,
            note: d.note.trim().to_string(),
            prev,
            hash: String::new(),
        };
        tx.hash = tx.digest();
        self.ledger.push(tx.clone());
        Ok(tx)
    }

    /// Check the hash chain. Fails at the first entry that was changed,
    /// removed or reordered after it was written.
    pub fn verify(&self) -> Result<(), String> {
        let mut prev = GENESIS.to_string();
        for (k, t) in self.ledger.iter().enumerate() {
            if t.prev != prev {
                return Err(format!(
                    "ledger entry {} does not follow entry {}: entries were removed or reordered",
                    t.seq,
                    if k == 0 { 0 } else { self.ledger[k - 1].seq }
                ));
            }
            if t.digest() != t.hash {
                return Err(format!(
                    "ledger entry {} was altered after it was recorded",
                    t.seq
                ));
            }
            prev = t.hash.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Item, Location, SupplyClass};

    fn inv() -> Inventory {
        let mut i = Inventory::default();
        for (id, controlled) in [("WTR", false), ("MOR", true)] {
            i.upsert_item(Item {
                id: id.into(),
                name: id.into(),
                class: SupplyClass::I,
                unit: "unit".into(),
                unit_kg: 1.0,
                unit_m3: None,
                cold_chain: None,
                max_transit_h: None,
                hazmat: None,
                controlled,
                reorder_level: 0.0,
                notes: String::new(),
            })
            .unwrap();
        }
        for id in ["A", "B"] {
            i.upsert_location(Location {
                id: id.into(),
                name: id.into(),
                kind: Default::default(),
            })
            .unwrap();
        }
        i
    }

    fn draft(action: Action, item: &str, qty: f64) -> TxDraft {
        TxDraft {
            action,
            item: item.into(),
            lot: "L1".into(),
            expiry: Some("2027-01-31".into()),
            from: Some("A".into()),
            to: Some("A".into()),
            qty,
            reference: None,
            user: "Sgt Ito".into(),
            witness: None,
            note: String::new(),
        }
    }

    #[test]
    fn stock_follows_the_ledger() {
        let mut i = inv();
        i.apply(draft(Action::Receive, "WTR", 100.0)).unwrap();
        i.apply(TxDraft {
            to: Some("B".into()),
            ..draft(Action::Transfer, "WTR", 30.0)
        })
        .unwrap();
        i.apply(draft(Action::Issue, "WTR", 20.0)).unwrap();
        i.apply(draft(Action::Hold, "WTR", 10.0)).unwrap();
        assert_eq!(i.available("WTR", Some("A")), 40.0);
        assert_eq!(i.available("WTR", Some("B")), 30.0);
        assert_eq!(i.available("WTR", None), 70.0);
        let held: f64 = i.stock().iter().map(|l| l.held).sum();
        assert_eq!(held, 10.0);
        // cannot issue more than is there, or release more than is held
        assert!(i.apply(draft(Action::Issue, "WTR", 41.0)).is_err());
        assert!(i.apply(draft(Action::Release, "WTR", 11.0)).is_err());
        // write-offs need a reason
        assert!(i.apply(draft(Action::Dispose, "WTR", 1.0)).is_err());
        let mut d = draft(Action::Dispose, "WTR", 1.0);
        d.note = "torn".into();
        i.apply(d).unwrap();
        assert_eq!(i.available("WTR", Some("A")), 39.0);
        assert!(i.verify().is_ok());
    }

    #[test]
    fn controlled_items_need_a_different_witness() {
        let mut i = inv();
        let mut d = draft(Action::Receive, "MOR", 5.0);
        assert!(i.apply(d.clone()).unwrap_err().contains("witness"));
        d.witness = Some("sgt ito".into());
        assert!(i.apply(d.clone()).unwrap_err().contains("different"));
        d.witness = Some("Lt Sato".into());
        assert_eq!(i.apply(d).unwrap().witness.as_deref(), Some("Lt Sato"));
    }

    #[test]
    fn tampering_breaks_the_chain() {
        let mut i = inv();
        for q in [10.0, 20.0, 30.0] {
            i.apply_at(draft(Action::Receive, "WTR", q), 1_700_000_000)
                .unwrap();
        }
        assert!(i.verify().is_ok());
        let mut edited = i.clone();
        edited.ledger[1].qty = 2000.0;
        assert!(edited.verify().unwrap_err().contains("entry 2 was altered"));
        let mut removed = i.clone();
        removed.ledger.remove(1);
        assert!(removed
            .verify()
            .unwrap_err()
            .contains("removed or reordered"));
        // a round trip through JSON keeps the chain valid
        let back: Inventory = serde_json::from_str(&serde_json::to_string(&i).unwrap()).unwrap();
        assert!(back.verify().is_ok());
    }

    #[test]
    fn a_lot_keeps_its_first_expiry() {
        let mut i = inv();
        i.apply(draft(Action::Receive, "WTR", 1.0)).unwrap();
        let mut d = draft(Action::Receive, "WTR", 1.0);
        d.expiry = Some("2030-01-01".into());
        assert_eq!(i.apply(d).unwrap().expiry.as_deref(), Some("2027-01-31"));
        let mut bad = draft(Action::Receive, "WTR", 1.0);
        bad.lot = "L2".into();
        bad.expiry = Some("next year".into());
        assert!(i.apply(bad).is_err());
    }
}
