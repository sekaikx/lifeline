//! Stock against a delivery plan: picks, shortfalls and load manifests.

use crate::{days, Inventory};
use lifeline_planner::{Mode, Plan, PlanScenario};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A lot that can be drawn on: lot, expiry day, expiry, units left, location.
type PoolLot = (String, Option<i64>, Option<String>, f64, String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pick {
    pub request_id: String,
    pub item: String,
    pub lot: String,
    pub expiry: Option<String>,
    /// Where it is picked from
    pub location: String,
    pub qty: f64,
    pub kg: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shortfall {
    pub request_id: String,
    pub item: String,
    pub needed: f64,
    pub picked: f64,
    pub reason: String,
}

/// A request whose cargo would spend longer in transit than one of its
/// items allows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitBreach {
    pub request_id: String,
    pub item: String,
    pub transit_h: f64,
    pub limit_h: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestLine {
    pub item: String,
    pub name: String,
    pub lot: String,
    pub expiry: Option<String>,
    pub qty: f64,
    pub unit: String,
    pub kg: f64,
}

/// What one vehicle carries on one trip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub unit: String,
    pub mode: Mode,
    /// 1-based trip number of this unit
    pub trip: usize,
    pub depart_h: f64,
    pub arrive_h: f64,
    pub request_id: String,
    pub destination: String,
    pub lines: Vec<ManifestLine>,
    pub kg: f64,
    pub cold_chain: bool,
    /// Dangerous-goods entries on board
    pub hazmat: Vec<String>,
    pub controlled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Allocation {
    pub picks: Vec<Pick>,
    pub shortfalls: Vec<Shortfall>,
    pub transit_breaches: Vec<TransitBreach>,
    pub manifests: Vec<Manifest>,
    /// Requests the plan cannot serve; nothing is picked for them
    pub unserved: Vec<String>,
}

/// Allocate stock held at `locations` (the depot and its stores) to the
/// plan's requests, critical first,
/// first expiring first out. `today` is the plan start date (`YYYY-MM-DD`);
/// a lot is only picked if it is still in date on arrival.
pub fn allocate(
    inv: &Inventory,
    s: &PlanScenario,
    plan: &Plan,
    locations: &[String],
    today: &str,
) -> Result<Allocation, String> {
    let start = days(today).ok_or_else(|| format!("{today} is not a date"))?;
    if locations.is_empty() {
        return Err("choose where the stock is drawn from".into());
    }
    if let Some(bad) = locations.iter().find(|l| inv.location(l).is_none()) {
        return Err(format!("unknown location {bad}"));
    }
    let site = locations.join(", ");
    let assignment = |id: &str| plan.assignments.iter().find(|a| a.request_id == id);

    // stock that can be drawn on, by item
    let mut pool: HashMap<String, Vec<PoolLot>> = HashMap::new();
    for l in inv.stock() {
        if locations.contains(&l.location) && l.available > 1e-9 {
            pool.entry(l.item.clone()).or_default().push((
                l.lot.clone(),
                l.expiry.as_deref().and_then(days),
                l.expiry.clone(),
                l.available,
                l.location.clone(),
            ));
        }
    }
    for lots in pool.values_mut() {
        // first expiring first; undated lots last
        lots.sort_by(|a, b| {
            a.1.unwrap_or(i64::MAX)
                .cmp(&b.1.unwrap_or(i64::MAX))
                .then(a.0.cmp(&b.0))
                .then(a.4.cmp(&b.4))
        });
    }

    let mut order: Vec<usize> = (0..s.requests.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (&s.requests[a], &s.requests[b]);
        ra.priority
            .cmp(&rb.priority)
            .then(ra.due_h.total_cmp(&rb.due_h))
            .then(a.cmp(&b))
    });

    let mut out = Allocation::default();
    for k in order {
        let r = &s.requests[k];
        if r.items.is_empty() {
            continue;
        }
        let Some(a) = assignment(&r.id).filter(|a| a.mode.is_some()) else {
            out.unserved.push(r.id.clone());
            continue;
        };
        // the last trip lands at completion; stock must last until then
        let arrive_day = start + (a.completion_h / 24.0).ceil() as i64;
        let transit_h = a
            .options
            .iter()
            .find(|o| Some(o.mode) == a.mode)
            .map_or(0.0, |o| o.one_way_h);
        for line in &r.items {
            let item = inv
                .item(&line.item)
                .ok_or_else(|| format!("{}: unknown item {}", r.name, line.item))?;
            if let Some(limit) = item.max_transit_h {
                if transit_h > limit + 1e-9 {
                    out.transit_breaches.push(TransitBreach {
                        request_id: r.id.clone(),
                        item: item.id.clone(),
                        transit_h,
                        limit_h: limit,
                    });
                }
            }
            let mut need = line.quantity;
            let mut expired_skipped = 0.0;
            if let Some(lots) = pool.get_mut(&item.id) {
                for (lot, exp_day, expiry, units, at) in lots.iter_mut() {
                    if need <= 1e-9 {
                        break;
                    }
                    if *units <= 1e-9 {
                        continue;
                    }
                    if exp_day.is_some_and(|d| d < arrive_day) {
                        expired_skipped += *units;
                        continue;
                    }
                    let take = need.min(*units);
                    *units -= take;
                    need -= take;
                    out.picks.push(Pick {
                        request_id: r.id.clone(),
                        item: item.id.clone(),
                        lot: lot.clone(),
                        expiry: expiry.clone(),
                        location: at.clone(),
                        qty: take,
                        kg: take * item.unit_kg,
                    });
                }
            }
            if need > 1e-9 {
                out.shortfalls.push(Shortfall {
                    request_id: r.id.clone(),
                    item: item.id.clone(),
                    needed: line.quantity,
                    picked: line.quantity - need,
                    reason: if expired_skipped > 0.0 {
                        format!(
                            "{expired_skipped} {} at {site} would be out of date on arrival",
                            item.unit
                        )
                    } else {
                        format!("not enough in stock at {site}")
                    },
                });
            }
        }
    }

    out.manifests = manifests(inv, s, plan, &out.picks);
    Ok(out)
}

/// Spread each request's picks over its trips, filling trips in departure
/// order with whole units where they fit.
fn manifests(inv: &Inventory, s: &PlanScenario, plan: &Plan, picks: &[Pick]) -> Vec<Manifest> {
    // every trip, with the unit that flies or drives it
    let mut trips: Vec<(&str, Mode, usize, &lifeline_planner::Trip)> = Vec::new();
    for u in &plan.schedule {
        for (k, t) in u.trips.iter().enumerate() {
            trips.push((&u.unit, u.mode, k + 1, t));
        }
    }
    trips.sort_by(|a, b| a.3.depart_h.total_cmp(&b.3.depart_h).then(a.0.cmp(b.0)));

    let mut out = Vec::new();
    for r in &s.requests {
        let mut queue: Vec<Pick> = picks
            .iter()
            .filter(|p| p.request_id == r.id)
            .cloned()
            .collect();
        if queue.is_empty() {
            continue;
        }
        let mine: Vec<_> = trips.iter().filter(|t| t.3.request_id == r.id).collect();
        let n = mine.len();
        for (idx, (unit, mode, trip, t)) in mine.into_iter().enumerate() {
            let last = idx + 1 == n;
            let mut room = t.load_kg;
            let mut lines: Vec<ManifestLine> = Vec::new();
            for p in queue.iter_mut() {
                if p.qty <= 1e-9 {
                    continue;
                }
                let Some(item) = inv.item(&p.item) else {
                    continue;
                };
                let take = if last {
                    p.qty
                } else {
                    let whole = ((room + 1e-6) / item.unit_kg).floor();
                    let fit = if whole >= 1.0 {
                        whole
                    } else {
                        room / item.unit_kg
                    };
                    fit.min(p.qty).max(0.0)
                };
                if take <= 1e-9 {
                    continue;
                }
                p.qty -= take;
                room -= take * item.unit_kg;
                lines.push(ManifestLine {
                    item: item.id.clone(),
                    name: item.name.clone(),
                    lot: p.lot.clone(),
                    expiry: p.expiry.clone(),
                    qty: take,
                    unit: item.unit.clone(),
                    kg: take * item.unit_kg,
                });
                if room <= 1e-6 && !last {
                    break;
                }
            }
            let items: Vec<_> = lines.iter().filter_map(|l| inv.item(&l.item)).collect();
            out.push(Manifest {
                unit: unit.to_string(),
                mode: *mode,
                trip: *trip,
                depart_h: t.depart_h,
                arrive_h: t.arrive_h,
                request_id: r.id.clone(),
                destination: r.name.clone(),
                kg: lines.iter().map(|l| l.kg).sum(),
                cold_chain: items.iter().any(|i| i.cold_chain.is_some()),
                hazmat: items.iter().filter_map(|i| i.hazmat.clone()).collect(),
                controlled: items.iter().any(|i| i.controlled),
                lines,
            });
        }
    }
    out.sort_by(|a, b| a.depart_h.total_cmp(&b.depart_h).then(a.unit.cmp(&b.unit)));
    out
}
