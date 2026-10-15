//! # lifeline-planner
//!
//! Turns a scenario (world, depot, fleet, risk, weather, requests) into a
//! delivery plan.
//!
//! 1. **Risk fields.** One exposure field per altitude band: trucks on the
//!    ground, drones at cruise height, aircraft at drop height. Each is the sum
//!    of risk-site rates over the cells a site can see, plus hazard areas
//!    for ground vehicles.
//! 2. **Options.** For every request and every mode the planner works out the
//!    route, trips needed, time to deliver, unit turnaround, and the
//!    probability that each trip's cargo arrives usable (survival times, for
//!    airdrops, the chance of landing inside the drop zone).
//! 3. **Schedule.** Requests are taken in order of priority, then deadline.
//!    Trips go to the unit of a mode that is free first. Among the modes that
//!    meet the risk tolerance, the request gets the cheapest one that still
//!    arrives on time; if none can, the one that arrives first; if no mode
//!    meets the tolerance, the safest. This list-scheduling heuristic is
//!    simple to explain and audit; it is not a global optimum.

pub mod model;

pub use model::*;

use lifeline_airdrop::dynamics::{Environment, Model};
use lifeline_airdrop::integrate::Method;
use lifeline_airdrop::montecarlo::{self, fraction_within, MonteCarloSetup};
use lifeline_airdrop::release::{solve_release_point, Aircraft, DropProblem};
use lifeline_core::raster::Raster;
use lifeline_core::risk::{exposure_field, polyline_length, segment_exposure, survival};
use lifeline_core::terrain::Terrain;
use lifeline_core::vec3::Vec3;
use lifeline_drone::DronePlanner;
use lifeline_ground::RoutingContext;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Exposure fields [1/s] for the three altitude bands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RiskLayers {
    pub ground: Raster,
    pub drone: Raster,
    pub air: Raster,
}

pub fn risk_layers(world: &World, s: &PlanScenario) -> RiskLayers {
    let t = &world.terrain;
    let f = &s.fleet;
    RiskLayers {
        ground: exposure_field(t, &s.risk, 2.5, f.trucks.truck.detectability),
        drone: exposure_field(
            t,
            &s.risk,
            f.drones.drone.cruise_agl_m,
            f.drones.drone.detectability,
        ),
        air: exposure_field(
            t,
            &s.risk,
            f.aircraft.drop_height_agl,
            f.aircraft.detectability,
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteAlternative {
    pub time_h: f64,
    pub distance_km: f64,
    pub survival: f64,
}

/// Mode-specific numbers shown in the option comparison.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OptionDetail {
    Truck {
        offroad_km: f64,
        survival: f64,
        /// Pareto front from fastest to safest
        alternatives: Vec<RouteAlternative>,
    },
    Drone {
        energy_wh: f64,
        usable_wh: f64,
        comms_fraction: f64,
        survival: f64,
        survival_round_trip: f64,
    },
    Airdrop {
        release_point: [f64; 2],
        bundles_per_sortie: usize,
        /// Chance a bundle lands inside the drop zone
        dz_probability: f64,
        /// Chance the aircraft reaches the release point
        survival: f64,
        cep_m: f64,
        time_of_fall_s: f64,
    },
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModeOption {
    pub mode: Mode,
    pub feasible: bool,
    pub reason: Option<String>,
    pub trips: usize,
    pub load_per_trip_kg: f64,
    /// Departure to cargo on target [h]
    pub one_way_h: f64,
    /// Departure to unit ready for the next trip [h]
    pub cycle_h: f64,
    /// Probability a trip's cargo arrives usable
    pub delivery_prob: f64,
    /// Operating cost of one trip
    pub cost_per_trip: f64,
    pub route: Vec<[f64; 2]>,
    pub distance_km: f64,
    pub detail: OptionDetail,
}

impl ModeOption {
    fn infeasible(mode: Mode, reason: impl Into<String>) -> Self {
        ModeOption {
            mode,
            feasible: false,
            reason: Some(reason.into()),
            trips: 0,
            load_per_trip_kg: 0.0,
            one_way_h: 0.0,
            cycle_h: 0.0,
            delivery_prob: 0.0,
            cost_per_trip: 0.0,
            route: Vec::new(),
            distance_km: 0.0,
            detail: OptionDetail::None,
        }
    }
}

/// Pick the fastest candidate that meets the tolerance, else the safest.
fn choose<T>(
    items: Vec<T>,
    prob: impl Fn(&T) -> f64,
    time: impl Fn(&T) -> f64,
    tolerance: f64,
) -> Option<T> {
    let (ok, rest): (Vec<T>, Vec<T>) = items.into_iter().partition(|i| prob(i) >= tolerance);
    if !ok.is_empty() {
        ok.into_iter().min_by(|a, b| time(a).total_cmp(&time(b)))
    } else {
        rest.into_iter().max_by(|a, b| prob(a).total_cmp(&prob(b)))
    }
}

/// The scheduling rule: within tolerance, cheapest on-time option, else the
/// earliest; if nothing meets the tolerance, the safest.
fn pick_mode<T>(
    items: Vec<T>,
    prob: impl Fn(&T) -> f64,
    finish: impl Fn(&T) -> f64,
    cost: impl Fn(&T) -> f64,
    due: f64,
    tolerance: f64,
) -> Option<T> {
    let (safe, risky): (Vec<T>, Vec<T>) = items.into_iter().partition(|i| prob(i) >= tolerance);
    if safe.is_empty() {
        return risky.into_iter().max_by(|a, b| {
            prob(a)
                .total_cmp(&prob(b))
                .then(finish(b).total_cmp(&finish(a)))
        });
    }
    let (on_time, late): (Vec<T>, Vec<T>) = safe.into_iter().partition(|i| finish(i) <= due);
    if on_time.is_empty() {
        late.into_iter()
            .min_by(|a, b| finish(a).total_cmp(&finish(b)))
    } else {
        on_time.into_iter().min_by(|a, b| {
            cost(a)
                .total_cmp(&cost(b))
                .then(finish(a).total_cmp(&finish(b)))
        })
    }
}

/// Trips one request may need before a mode counts as infeasible.
const MAX_TRIPS: usize = 1_000;

fn trips_for(demand: f64, capacity: f64) -> usize {
    (demand / capacity).ceil().max(1.0) as usize
}

struct Evaluator<'a> {
    s: &'a PlanScenario,
    world: &'a World,
    layers: &'a RiskLayers,
    terrain: Terrain,
    env: Environment,
}

impl Evaluator<'_> {
    fn depot(&self) -> [f64; 2] {
        [self.s.depot.x, self.s.depot.y]
    }

    fn truck(&self, ctx: &RoutingContext, r: &Request) -> ModeOption {
        let fleet = &self.s.fleet.trucks;
        if fleet.count == 0 {
            return ModeOption::infeasible(Mode::Truck, "no trucks in the fleet");
        }
        let options = ctx.options(self.depot(), [r.x, r.y]);
        let alternatives = options
            .iter()
            .map(|o| RouteAlternative {
                time_h: o.time_s / 3600.0,
                distance_km: o.length_m / 1000.0,
                survival: o.survival,
            })
            .collect();
        let Some(route) = choose(
            options,
            |o| o.survival,
            |o| o.time_s,
            self.s.settings.min_delivery_prob,
        ) else {
            return ModeOption::infeasible(
                Mode::Truck,
                "no ground route: blocked by impassable areas",
            );
        };
        let t = &fleet.truck;
        let handling = t.handling_min / 60.0;
        let drive = route.time_s / 3600.0;
        ModeOption {
            mode: Mode::Truck,
            feasible: true,
            reason: None,
            trips: trips_for(r.demand_kg, t.capacity_kg),
            load_per_trip_kg: r.demand_kg.min(t.capacity_kg),
            one_way_h: handling + drive,
            cycle_h: 2.0 * (handling + drive),
            delivery_prob: route.survival,
            cost_per_trip: 2.0 * (handling + drive) * fleet.cost_per_hour,
            distance_km: route.length_m / 1000.0,
            detail: OptionDetail::Truck {
                offroad_km: route.offroad_m / 1000.0,
                survival: route.survival,
                alternatives,
            },
            route: route.points,
        }
    }

    fn drone(&self, r: &Request) -> ModeOption {
        let fleet = &self.s.fleet.drones;
        if fleet.count == 0 {
            return ModeOption::infeasible(Mode::Drone, "no drones in the fleet");
        }
        let d = fleet.drone;
        let cruise_msl = self.world.terrain.at(r.x, r.y) + d.cruise_agl_m;
        let wind = self
            .s
            .weather
            .wind
            .velocity(Vec3::new(r.x, r.y, cruise_msl));
        let planner = DronePlanner {
            terrain: &self.world.terrain,
            field: &self.layers.drone,
            spec: d,
            wind: [wind.x, wind.y],
            rho: self.env.atmosphere.density(cruise_msl),
            antenna_m: 10.0,
        };
        let load = r.demand_kg.min(d.payload_kg);
        let routes: Vec<_> = [0.0, 1.0, 8.0, 64.0]
            .iter()
            .map(|w| planner.route(self.depot(), [r.x, r.y], load, w * 3600.0))
            .collect();
        let first_reason = routes[0].reason.clone();
        let feasible: Vec<_> = routes.into_iter().filter(|x| x.feasible).collect();
        let Some(route) = choose(
            feasible,
            |x| x.survival_out,
            |x| x.time_out_s,
            self.s.settings.min_delivery_prob,
        ) else {
            return ModeOption::infeasible(
                Mode::Drone,
                first_reason.unwrap_or_else(|| "no feasible drone route".into()),
            );
        };
        let turnaround = d.turnaround_min / 60.0;
        let cycle_h = (route.time_out_s + route.time_back_s) / 3600.0 + turnaround;
        ModeOption {
            mode: Mode::Drone,
            feasible: true,
            reason: None,
            trips: trips_for(r.demand_kg, d.payload_kg),
            load_per_trip_kg: load,
            one_way_h: route.time_out_s / 3600.0,
            cycle_h,
            delivery_prob: route.survival_out,
            cost_per_trip: cycle_h * fleet.cost_per_hour,
            distance_km: route.length_m / 1000.0,
            detail: OptionDetail::Drone {
                energy_wh: route.energy_out_wh + route.energy_back_wh,
                usable_wh: d.usable_wh(),
                comms_fraction: route.comms_fraction,
                survival: route.survival_out,
                survival_round_trip: route.survival_round,
            },
            route: route.points,
        }
    }

    fn airdrop(&self, r: &Request) -> ModeOption {
        let a = &self.s.fleet.aircraft;
        if a.count == 0 {
            return ModeOption::infeasible(Mode::Airdrop, "no aircraft in the fleet");
        }
        if !self.s.depot.airstrip {
            return ModeOption::infeasible(Mode::Airdrop, "the depot has no airstrip");
        }
        let bundles = (a.capacity_kg / a.bundle.mass).floor() as usize;
        if bundles == 0 {
            return ModeOption::infeasible(
                Mode::Airdrop,
                "one bundle is heavier than the aircraft can carry",
            );
        }
        let depot = self.depot();
        let track = (r.x - depot[0])
            .atan2(r.y - depot[1])
            .to_degrees()
            .rem_euclid(360.0);
        let aircraft = Aircraft {
            height_agl: a.drop_height_agl,
            ground_speed: a.drop_speed_ms,
            track_deg: track,
        };
        let model = Model::new(&a.bundle, &self.env);
        let problem = DropProblem {
            model: &model,
            terrain: &self.terrain,
            aircraft,
            target: [r.x, r.y],
            method: Method::Rk4,
            dt: 0.1,
            t_max: 900.0,
        };
        let release = match solve_release_point(&problem, 0.5) {
            Ok(rel) => rel,
            Err(e) => return ModeOption::infeasible(Mode::Airdrop, e.to_string()),
        };
        let mc = montecarlo::run(&MonteCarloSetup {
            payload: &a.bundle,
            env: &self.env,
            terrain: &self.terrain,
            aircraft,
            release_point: release.release_point,
            target: [r.x, r.y],
            method: Method::Rk4,
            dt: 0.1,
            t_max: 900.0,
            uncertainty: a.uncertainty,
            samples: a.samples,
            seed: self.s.settings.seed,
        });
        let dz = fraction_within(&mc.impacts, [r.x, r.y], a.dz_radius_m) * mc.landed_fraction;
        let rp = release.release_point;
        let leg = (rp[0] - depot[0]).hypot(rp[1] - depot[1]);
        let fly = leg / a.cruise_ms;
        let exposure = segment_exposure(&self.layers.air, depot, rp, fly);
        let reach = survival(exposure);
        let per_sortie = bundles as f64 * a.bundle.mass;
        let rig = a.rigging_min / 60.0;
        let cycle_h = rig + 2.0 * fly / 3600.0 + a.turnaround_min / 60.0;
        ModeOption {
            mode: Mode::Airdrop,
            feasible: true,
            reason: None,
            trips: trips_for(r.demand_kg, per_sortie),
            load_per_trip_kg: r.demand_kg.min(per_sortie),
            one_way_h: rig + (fly + release.time_of_fall) / 3600.0,
            cycle_h,
            delivery_prob: reach * dz,
            cost_per_trip: cycle_h * a.cost_per_hour,
            route: vec![depot, rp, [r.x, r.y]],
            distance_km: polyline_length(&[depot, rp]) / 1000.0,
            detail: OptionDetail::Airdrop {
                release_point: rp,
                bundles_per_sortie: bundles,
                dz_probability: dz,
                survival: reach,
                cep_m: mc.cep,
                time_of_fall_s: release.time_of_fall,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trip {
    pub request_id: String,
    pub depart_h: f64,
    pub arrive_h: f64,
    pub ready_h: f64,
    pub load_kg: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitSchedule {
    pub unit: String,
    pub mode: Mode,
    pub trips: Vec<Trip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub request_id: String,
    /// `None` if no mode can serve the request
    pub mode: Option<Mode>,
    pub reason: Option<String>,
    pub trips: usize,
    pub first_arrival_h: f64,
    pub completion_h: f64,
    pub on_time: bool,
    pub expected_kg: f64,
    pub delivery_prob: f64,
    pub cost: f64,
    /// Meets the scenario's minimum delivery probability
    pub within_tolerance: bool,
    pub options: Vec<ModeOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub demand_kg: f64,
    pub expected_kg: f64,
    /// Expected kg arriving before each request's deadline
    pub expected_on_time_kg: f64,
    pub critical_total: usize,
    pub critical_on_time: usize,
    pub makespan_h: f64,
    pub cost: f64,
    pub unserved: usize,
    pub below_tolerance: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// One per request, in the scenario's order
    pub assignments: Vec<Assignment>,
    pub schedule: Vec<UnitSchedule>,
    pub summary: Summary,
    pub compute_ms: f64,
}

/// Place `trips` trips on the earliest-free units; returns the trips (with
/// unit index) and does not modify `free`.
fn tentative(free: &[f64], opt: &ModeOption, r: &Request) -> Vec<(usize, Trip)> {
    let mut free = free.to_vec();
    let mut left = r.demand_kg;
    (0..opt.trips)
        .map(|_| {
            let (u, &start) = free
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(b.1))
                .expect("unit");
            let load = left.min(opt.load_per_trip_kg);
            left -= load;
            free[u] = start + opt.cycle_h;
            (
                u,
                Trip {
                    request_id: r.id.clone(),
                    depart_h: start,
                    arrive_h: start + opt.one_way_h,
                    ready_h: start + opt.cycle_h,
                    load_kg: load,
                },
            )
        })
        .collect()
}

pub fn plan(world: &World, s: &PlanScenario) -> Result<Plan, String> {
    s.validate()?;
    if world.spec != s.world {
        return Err("the world does not match the scenario's world spec".into());
    }
    let start = Instant::now();
    let layers = risk_layers(world, s);
    let ev = Evaluator {
        s,
        world,
        layers: &layers,
        terrain: Terrain::Grid(world.terrain.clone()),
        env: Environment {
            atmosphere: s.weather.atmosphere.clone(),
            wind: s.weather.wind.clone(),
        },
    };
    let ctx = RoutingContext::new(
        &world.terrain,
        &world.network,
        &layers.ground,
        &s.risk,
        s.fleet.trucks.truck,
    );

    let options: Vec<Vec<ModeOption>> = s
        .requests
        .par_iter()
        .map(|r| {
            Mode::ALL
                .iter()
                .map(|&m| {
                    if !r.mode.allows(m) {
                        return ModeOption::infeasible(m, "excluded for this request");
                    }
                    let o = match m {
                        Mode::Truck => ev.truck(&ctx, r),
                        Mode::Drone => ev.drone(r),
                        Mode::Airdrop => ev.airdrop(r),
                    };
                    // a tiny capacity would otherwise schedule millions of trips
                    if o.trips > MAX_TRIPS {
                        return ModeOption::infeasible(
                            m,
                            format!("needs {} trips (at most {MAX_TRIPS})", o.trips),
                        );
                    }
                    o
                })
                .collect()
        })
        .collect();

    let counts = [
        s.fleet.trucks.count,
        s.fleet.drones.count,
        s.fleet.aircraft.count,
    ];
    let mut free: Vec<Vec<f64>> = counts.iter().map(|&n| vec![0.0; n]).collect();
    let mut units: Vec<Vec<UnitSchedule>> = Mode::ALL
        .iter()
        .zip(counts)
        .map(|(&m, n)| {
            (0..n)
                .map(|i| UnitSchedule {
                    unit: format!("{} {}", m.label(), i + 1),
                    mode: m,
                    trips: Vec::new(),
                })
                .collect()
        })
        .collect();

    let mut order: Vec<usize> = (0..s.requests.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (&s.requests[a], &s.requests[b]);
        ra.priority
            .cmp(&rb.priority)
            .then(ra.due_h.total_cmp(&rb.due_h))
            .then(a.cmp(&b))
    });

    let tolerance = s.settings.min_delivery_prob;
    let mut assignments: Vec<Option<Assignment>> = vec![None; s.requests.len()];
    for k in order {
        let r = &s.requests[k];
        let candidates: Vec<(usize, Vec<(usize, Trip)>)> = options[k]
            .iter()
            .enumerate()
            .filter(|(_, o)| o.feasible)
            .map(|(mi, o)| (mi, tentative(&free[mi], o, r)))
            .collect();
        let finish =
            |trips: &Vec<(usize, Trip)>| trips.iter().map(|t| t.1.arrive_h).fold(0.0, f64::max);
        let cost = |mi: usize| options[k][mi].cost_per_trip * options[k][mi].trips as f64;
        let chosen = pick_mode(
            candidates,
            |(mi, _)| options[k][*mi].delivery_prob,
            |(_, t)| finish(t),
            |(mi, _)| cost(*mi),
            r.due_h,
            tolerance,
        );
        let Some((mi, trips)) = chosen else {
            let reasons: Vec<String> = options[k]
                .iter()
                .filter_map(|o| {
                    o.reason
                        .as_ref()
                        .map(|why| format!("{}: {why}", o.mode.label()))
                })
                .collect();
            assignments[k] = Some(Assignment {
                request_id: r.id.clone(),
                mode: None,
                reason: Some(reasons.join("; ")),
                trips: 0,
                first_arrival_h: 0.0,
                completion_h: 0.0,
                on_time: false,
                expected_kg: 0.0,
                delivery_prob: 0.0,
                cost: 0.0,
                within_tolerance: false,
                options: options[k].clone(),
            });
            continue;
        };
        let opt = &options[k][mi];
        let completion = finish(&trips);
        let first = trips
            .iter()
            .map(|t| t.1.arrive_h)
            .fold(f64::INFINITY, f64::min);
        let expected: f64 = trips.iter().map(|t| t.1.load_kg * opt.delivery_prob).sum();
        let total_cost = opt.cost_per_trip * opt.trips as f64;
        for (u, trip) in trips {
            free[mi][u] = trip.ready_h;
            units[mi][u].trips.push(trip);
        }
        assignments[k] = Some(Assignment {
            request_id: r.id.clone(),
            mode: Some(opt.mode),
            reason: None,
            trips: opt.trips,
            first_arrival_h: first,
            completion_h: completion,
            on_time: completion <= r.due_h,
            expected_kg: expected,
            delivery_prob: opt.delivery_prob,
            cost: total_cost,
            within_tolerance: opt.delivery_prob >= tolerance,
            options: options[k].clone(),
        });
    }

    let assignments: Vec<Assignment> = assignments
        .into_iter()
        .map(|a| a.expect("every request handled"))
        .collect();
    let schedule: Vec<UnitSchedule> = units.into_iter().flatten().collect();
    let due = |id: &str| {
        s.requests
            .iter()
            .find(|r| r.id == id)
            .map_or(0.0, |r| r.due_h)
    };
    let prob = |id: &str| {
        assignments
            .iter()
            .find(|a| a.request_id == id)
            .map_or(0.0, |a| a.delivery_prob)
    };
    let expected_on_time_kg = schedule
        .iter()
        .flat_map(|u| &u.trips)
        .filter(|t| t.arrive_h <= due(&t.request_id))
        .map(|t| t.load_kg * prob(&t.request_id))
        .sum();
    let critical: Vec<&Assignment> = assignments
        .iter()
        .zip(&s.requests)
        .filter(|(_, r)| r.priority == Priority::Critical)
        .map(|(a, _)| a)
        .collect();
    let summary = Summary {
        demand_kg: s.requests.iter().map(|r| r.demand_kg).sum(),
        expected_kg: assignments.iter().map(|a| a.expected_kg).sum(),
        expected_on_time_kg,
        critical_total: critical.len(),
        critical_on_time: critical
            .iter()
            .filter(|a| a.on_time && a.mode.is_some())
            .count(),
        makespan_h: assignments
            .iter()
            .map(|a| a.completion_h)
            .fold(0.0, f64::max),
        cost: assignments.iter().map(|a| a.cost).sum(),
        unserved: assignments.iter().filter(|a| a.mode.is_none()).count(),
        below_tolerance: assignments
            .iter()
            .filter(|a| a.mode.is_some() && !a.within_tolerance)
            .count(),
    };
    Ok(Plan {
        assignments,
        schedule,
        summary,
        compute_ms: start.elapsed().as_secs_f64() * 1e3,
    })
}
