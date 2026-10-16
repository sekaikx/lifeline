//! Scenario types: the world, the depot, the fleet and the delivery requests.

use lifeline_airdrop::dynamics::{Parachute, Payload};
use lifeline_airdrop::montecarlo::Uncertainty;
use lifeline_core::atmosphere::Atmosphere;
use lifeline_core::risk::{HazardArea, RiskModel, RiskSite};
use lifeline_core::terrain::TerrainGrid;
use lifeline_core::wind::{Wind, WindLayer};
use lifeline_drone::DroneSpec;
use lifeline_geodata::{Area, Fetcher, Place};
use lifeline_ground::{RoadClass, RoadNetwork, Truck};
use serde::{Deserialize, Serialize};

/// Parameters of a generated world. The same spec always gives the same
/// terrain and roads, so scenarios stay small and shareable.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneratedSpec {
    pub seed: u64,
    /// Side of the square map [m]
    pub size_m: f64,
    /// Grid intervals per side
    pub cells: usize,
    /// Lowest ground [m MSL]
    pub base_m: f64,
    /// Height of the highest ridges above the lowest ground [m]
    pub relief_m: f64,
    pub towns: usize,
}

impl Default for GeneratedSpec {
    fn default() -> Self {
        GeneratedSpec {
            seed: 11,
            size_m: 24_000.0,
            cells: 160,
            base_m: 120.0,
            relief_m: 520.0,
            towns: 18,
        }
    }
}

/// Where the map comes from: a reproducible synthetic world (for training
/// and tests) or a real area built from open data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorldSpec {
    Generated(GeneratedSpec),
    Real(Area),
}

impl Default for WorldSpec {
    fn default() -> Self {
        WorldSpec::Generated(GeneratedSpec::default())
    }
}

impl WorldSpec {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            WorldSpec::Generated(g) => {
                if !(2_000.0..=200_000.0).contains(&g.size_m) || !(16..=400).contains(&g.cells) {
                    return Err("world: size must be 2-200 km and cells 16-400".into());
                }
                if !(2..=24).contains(&g.towns) || g.relief_m < 0.0 {
                    return Err("world: 2-24 towns, non-negative relief".into());
                }
                Ok(())
            }
            WorldSpec::Real(a) => a.validate(),
        }
    }

    pub fn size_m(&self) -> f64 {
        match self {
            WorldSpec::Generated(g) => g.size_m,
            WorldSpec::Real(a) => a.size_m,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            WorldSpec::Generated(_) => "Generated valley",
            WorldSpec::Real(a) => &a.name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct World {
    pub spec: WorldSpec,
    pub terrain: TerrainGrid,
    pub network: RoadNetwork,
    /// Named settlements, largest first
    pub places: Vec<Place>,
    /// Data sources to credit on the map
    pub attribution: Vec<String>,
}

impl World {
    /// A generated world (no network needed).
    pub fn generate(spec: GeneratedSpec) -> Result<Self, String> {
        Self::build(&WorldSpec::Generated(spec), None)
    }

    /// Build any world. Real areas download elevation and OpenStreetMap
    /// data through `fetcher` (cached on disk).
    pub fn build(spec: &WorldSpec, fetcher: Option<&Fetcher>) -> Result<Self, String> {
        spec.validate()?;
        match spec {
            WorldSpec::Generated(g) => {
                let terrain =
                    TerrainGrid::procedural(g.seed, g.size_m, g.cells, g.base_m, g.relief_m);
                let network = RoadNetwork::generate(&terrain, g.seed, g.towns);
                let places = network
                    .towns
                    .iter()
                    .map(|t| Place {
                        name: t.name.clone(),
                        kind: "town".into(),
                        x: t.x,
                        y: t.y,
                        population: None,
                    })
                    .collect();
                Ok(World {
                    spec: spec.clone(),
                    terrain,
                    network,
                    places,
                    attribution: vec!["Synthetic terrain".into()],
                })
            }
            WorldSpec::Real(area) => {
                let fetcher = fetcher.ok_or("a real area needs network access to load")?;
                let terrain = lifeline_geodata::terrain(fetcher, area)?;
                let (network, places) = lifeline_geodata::osm(fetcher, area)?;
                Ok(World {
                    spec: spec.clone(),
                    terrain,
                    network,
                    places,
                    attribution: vec![
                        "Elevation: AWS Terrain Tiles (SRTM and others)".into(),
                        "Roads and places © OpenStreetMap contributors, ODbL".into(),
                    ],
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Truck,
    Drone,
    Airdrop,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Truck, Mode::Drone, Mode::Airdrop];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Truck => "Truck",
            Mode::Drone => "Drone",
            Mode::Airdrop => "Aircraft",
        }
    }
}

/// Which mode a request may use: let the planner choose, or force one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeChoice {
    #[default]
    Auto,
    Truck,
    Drone,
    Airdrop,
}

impl ModeChoice {
    pub fn allows(self, m: Mode) -> bool {
        matches!(
            (self, m),
            (ModeChoice::Auto, _)
                | (ModeChoice::Truck, Mode::Truck)
                | (ModeChoice::Drone, Mode::Drone)
                | (ModeChoice::Airdrop, Mode::Airdrop)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Critical,
    High,
    Routine,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub demand_kg: f64,
    pub priority: Priority,
    /// Needed within this many hours of the plan start
    pub due_h: f64,
    #[serde(default)]
    pub mode: ModeChoice,
    /// What is asked for, by catalogue item (see `lifeline-supply`). When
    /// present the UI keeps `demand_kg` equal to the lines' total weight.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<RequestLine>,
    /// Longest the cargo may be in transit [h], e.g. for a cold chain.
    /// Modes slower than this are not used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_transit_h: Option<f64>,
}

/// A quantity of one catalogue item in a request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestLine {
    pub item: String,
    pub quantity: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Depot {
    pub name: String,
    pub x: f64,
    pub y: f64,
    /// Aircraft can only operate from a depot with an airstrip
    pub airstrip: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TruckFleet {
    pub count: usize,
    pub truck: Truck,
    /// Operating cost while out on a trip [currency per hour]
    pub cost_per_hour: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneFleet {
    pub count: usize,
    pub drone: DroneSpec,
    pub cost_per_hour: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AircraftFleet {
    pub count: usize,
    /// Cargo per sortie [kg]
    pub capacity_kg: f64,
    /// One parachute bundle
    pub bundle: Payload,
    pub cruise_ms: f64,
    /// Ground speed on the drop run [m/s]
    pub drop_speed_ms: f64,
    pub drop_height_agl: f64,
    /// Rigging and loading before each sortie [min]
    pub rigging_min: f64,
    pub turnaround_min: f64,
    pub detectability: f64,
    /// A bundle counts as delivered if it lands within this radius [m]
    pub dz_radius_m: f64,
    pub uncertainty: Uncertainty,
    /// Monte Carlo samples per drop estimate
    pub samples: usize,
    pub cost_per_hour: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fleet {
    pub trucks: TruckFleet,
    pub drones: DroneFleet,
    pub aircraft: AircraftFleet,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub atmosphere: Atmosphere,
    pub wind: Wind,
    /// Where the wind came from ("manual", or the forecast and its time)
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Prefer options whose cargo arrives with at least this probability
    pub min_delivery_prob: f64,
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanScenario {
    pub name: String,
    pub world: WorldSpec,
    pub depot: Depot,
    pub requests: Vec<Request>,
    pub fleet: Fleet,
    pub risk: RiskModel,
    pub weather: Weather,
    pub settings: Settings,
}

impl PlanScenario {
    pub fn validate(&self) -> Result<(), String> {
        self.world.validate()?;
        self.risk.validate()?;
        self.fleet.drones.drone.validate()?;
        self.fleet.aircraft.bundle.validate()?;
        self.weather.wind.validate()?;
        // a temperature offset below about -216 K or a negative constant
        // density gives negative air density, and NaN drone energy passes
        // the battery check
        let atmosphere = &self.weather.atmosphere;
        if [-500.0, 0.0, 11_000.0]
            .iter()
            .any(|z| !(atmosphere.density(*z) >= 0.0 && atmosphere.density(*z).is_finite()))
        {
            return Err("weather: air density must be non-negative".into());
        }
        let t = &self.fleet.trucks.truck;
        if [
            t.capacity_kg,
            t.speed_paved_kmh,
            t.speed_gravel_kmh,
            t.speed_track_kmh,
            t.speed_offroad_kmh,
        ]
        .iter()
        .any(|v| !(*v > 0.0))
        {
            return Err("truck: capacity and speeds must be positive".into());
        }
        let a = &self.fleet.aircraft;
        if !(a.capacity_kg > 0.0
            && a.cruise_ms > 0.0
            && a.drop_speed_ms > 0.0
            && a.drop_height_agl > 0.0
            && a.dz_radius_m > 0.0)
        {
            return Err(
                "aircraft: capacity, speeds, drop height and drop-zone radius must be positive"
                    .into(),
            );
        }
        if !(1..=20_000).contains(&a.samples) {
            return Err("aircraft: 1-20000 Monte Carlo samples".into());
        }
        // negative times, costs or detectability give negative trip times and
        // negative risk, which breaks the shortest-path searches
        let d = &self.fleet.drones.drone;
        if [
            t.handling_min,
            t.detectability,
            self.fleet.trucks.cost_per_hour,
            d.detectability,
            d.turnaround_min,
            self.fleet.drones.cost_per_hour,
            a.rigging_min,
            a.turnaround_min,
            a.detectability,
            a.cost_per_hour,
        ]
        .iter()
        .any(|v| !(*v >= 0.0))
        {
            return Err("fleet: handling, turnaround and rigging times, detectability and costs must be non-negative".into());
        }
        if !(0.0..=1.0).contains(&self.settings.min_delivery_prob) {
            return Err("risk tolerance must be between 0 and 1".into());
        }
        let half = self.world.size_m() / 2.0;
        let on_map = |x: f64, y: f64| x.abs() <= half && y.abs() <= half;
        if !on_map(self.depot.x, self.depot.y) {
            return Err("the depot is outside the map".into());
        }
        for (k, r) in self.requests.iter().enumerate() {
            if !(r.demand_kg > 0.0) || !(r.due_h > 0.0) {
                return Err(format!("{}: demand and deadline must be positive", r.name));
            }
            if r.max_transit_h.is_some_and(|h| !(h > 0.0)) {
                return Err(format!("{}: the transit limit must be positive", r.name));
            }
            if r.items.iter().any(|l| !(l.quantity > 0.0)) {
                return Err(format!("{}: item quantities must be positive", r.name));
            }
            if !on_map(r.x, r.y) {
                return Err(format!("{}: outside the map", r.name));
            }
            if self.requests[..k].iter().any(|o| o.id == r.id) {
                return Err(format!("request id {} is used twice", r.id));
            }
        }
        Ok(())
    }

    /// A worked example on any world: a depot with an airstrip in the
    /// south-west, seven requests from a 12 kg medical resupply to a 6 t
    /// water delivery, three observed road stretches and two hazard areas.
    /// On a real map the requests sit on real settlements.
    pub fn example(world: &World) -> PlanScenario {
        let places = &world.places;
        let half = world.spec.size_m() / 2.0;
        let dist = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        let near = |x: f64, y: f64, skip: &[usize]| {
            (0..places.len())
                .filter(|i| !skip.contains(i))
                .min_by(|&a, &b| {
                    dist([places[a].x, places[a].y], [x, y])
                        .total_cmp(&dist([places[b].x, places[b].y], [x, y]))
                })
        };
        let fallback = |x: f64, y: f64| {
            (
                format!("{:.0} km E, {:.0} km N", x / 1000.0, y / 1000.0),
                x,
                y,
            )
        };
        let d = near(-0.65 * half, -0.65 * half, &[]);
        let (dname, dx, dy) = d.map_or_else(
            || fallback(-0.65 * half, -0.65 * half),
            |i| (places[i].name.clone(), places[i].x, places[i].y),
        );
        let depot = Depot {
            name: format!("{dname} depot"),
            x: dx,
            y: dy,
            airstrip: true,
        };

        let mut used: Vec<usize> = d.into_iter().collect();
        let mut pick = |fx: f64, fy: f64| match near(fx * half, fy * half, &used) {
            Some(i) => {
                used.push(i);
                (places[i].name.clone(), places[i].x, places[i].y)
            }
            None => fallback(fx * half, fy * half),
        };
        let specs = [
            (0.55, 0.60, "Field clinic", 350.0, Priority::Critical, 3.0),
            (0.70, -0.45, "Water point", 6000.0, Priority::High, 10.0),
            (-0.55, 0.70, "Shelter site", 2500.0, Priority::Routine, 24.0),
            (
                -0.30,
                -0.30,
                "Blood and vaccines",
                12.0,
                Priority::Critical,
                1.5,
            ),
            (0.05, 0.75, "School kitchen", 900.0, Priority::Routine, 18.0),
        ];
        let real = matches!(world.spec, WorldSpec::Real(_));
        let mut requests: Vec<Request> = specs
            .iter()
            .enumerate()
            .map(|(k, &(fx, fy, name, kg, priority, due))| {
                let (place, x, y) = pick(fx, fy);
                Request {
                    id: format!("R{}", k + 1),
                    name: if real {
                        format!("{name}, {place}")
                    } else {
                        name.into()
                    },
                    x,
                    y,
                    demand_kg: kg,
                    priority,
                    due_h: due,
                    mode: ModeChoice::Auto,
                    items: Vec::new(),
                    max_transit_h: None,
                }
            })
            .collect();

        // Two off-road sites: a relay post on high ground near the depot,
        // and a village cut off from the roads (the settlement farthest from
        // any road, or high ground far from the depot when every settlement
        // is on a road).
        let t = &world.terrain;
        let highest = |min: f64, max: f64| {
            let mut best = (f64::NEG_INFINITY, 0.0, 0.0);
            for j in 0..t.ny {
                for i in 0..t.nx {
                    let (x, y) = (t.x0 + i as f64 * t.dx, t.y0 + j as f64 * t.dy);
                    let r = dist([x, y], [depot.x, depot.y]);
                    let h = t.heights[j * t.nx + i];
                    if (min..max).contains(&r) && h > best.0 {
                        best = (h, x, y);
                    }
                }
            }
            (best.1, best.2)
        };
        let (rx, ry) = highest(0.0, 0.75 * half);
        requests.push(Request {
            id: "R6".into(),
            name: "Relay post".into(),
            x: rx,
            y: ry,
            demand_kg: 25.0,
            priority: Priority::High,
            due_h: 4.0,
            mode: ModeChoice::Auto,
            items: Vec::new(),
            max_transit_h: None,
        });
        let off_road = |p: &Place| {
            world
                .network
                .towns
                .iter()
                .map(|n| dist([n.x, n.y], [p.x, p.y]))
                .fold(f64::INFINITY, f64::min)
        };
        let remote = places
            .iter()
            .filter(|p| dist([p.x, p.y], [depot.x, depot.y]) > 0.6 * half)
            .map(|p| (off_road(p), p))
            .filter(|(d, _)| *d > 1_500.0)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let (vname, vx, vy) = match remote {
            Some((_, p)) => (format!("Mountain village, {}", p.name), p.x, p.y),
            None => {
                let (x, y) = highest(1.08 * half, 1.83 * half);
                ("Mountain village".to_string(), x, y)
            }
        };
        requests.push(Request {
            id: "R7".into(),
            name: vname,
            x: vx,
            y: vy,
            demand_kg: 1_500.0,
            priority: Priority::Critical,
            due_h: 1.5,
            mode: ModeChoice::Auto,
            items: Vec::new(),
            max_transit_h: None,
        });

        // Risk sites on high ground overlooking paved roads, spread out from
        // the depot: one road stretch closest to each of three distances.
        let paved: Vec<&lifeline_ground::Road> = world
            .network
            .roads
            .iter()
            .filter(|r| r.class == RoadClass::Paved)
            .collect();
        let mid = |r: &lifeline_ground::Road| r.points[r.points.len() / 2];
        let mut chosen: Vec<[f64; 2]> = Vec::new();
        for target in [0.45, 0.85, 1.25] {
            let best = paved
                .iter()
                .map(|r| mid(r))
                .filter(|m| chosen.iter().all(|c| dist(*c, *m) > 4_000.0))
                .min_by(|a, b| {
                    let da = (dist(*a, [depot.x, depot.y]) - target * half).abs();
                    let db = (dist(*b, [depot.x, depot.y]) - target * half).abs();
                    da.total_cmp(&db)
                });
            chosen.extend(best);
        }
        let sites = chosen
            .iter()
            .enumerate()
            .map(|(k, m)| {
                // highest point 1-2.5 km from the road
                let mut top = (f64::NEG_INFINITY, m[0], m[1]);
                for a in 0..16 {
                    for rr in [1_000.0, 1_750.0, 2_500.0] {
                        let ang = a as f64 * std::f64::consts::TAU / 16.0;
                        let (x, y) = (m[0] + rr * ang.cos(), m[1] + rr * ang.sin());
                        let h = t.at(x, y);
                        if h > top.0 {
                            top = (h, x, y);
                        }
                    }
                }
                RiskSite {
                    id: format!("S{}", k + 1),
                    name: format!("Observed sector {}", k + 1),
                    x: top.1,
                    y: top.2,
                    height: 8.0,
                    range: 3_000.0,
                    rate_per_min: 0.02,
                }
            })
            .collect();

        // hazard areas on unpaved roads at mid distance
        let mut unpaved: Vec<&lifeline_ground::Road> = world
            .network
            .roads
            .iter()
            .filter(|r| r.class != RoadClass::Paved)
            .collect();
        unpaved.sort_by(|a, b| {
            (dist(mid(a), [depot.x, depot.y]) - half)
                .abs()
                .total_cmp(&(dist(mid(b), [depot.x, depot.y]) - half).abs())
        });
        let square = |c: [f64; 2], s: f64| {
            vec![
                [c[0] - s, c[1] - s],
                [c[0] + s, c[1] - s],
                [c[0] + s, c[1] + s],
                [c[0] - s, c[1] + s],
            ]
        };
        let mut hazards = Vec::new();
        if let Some(r) = unpaved.first() {
            hazards.push(HazardArea {
                id: "H1".into(),
                name: "Washed-out crossing".into(),
                polygon: square(mid(r), 450.0),
                rate_per_min: 0.0,
                impassable: true,
            });
        }
        if let Some(r) = unpaved
            .iter()
            .find(|r| dist(mid(r), mid(unpaved[0])) > 5_000.0)
        {
            hazards.push(HazardArea {
                id: "H2".into(),
                name: "Reported mines".into(),
                polygon: square(mid(r), 600.0),
                rate_per_min: 0.25,
                impassable: false,
            });
        }

        PlanScenario {
            name: if real {
                format!("{} resupply", world.spec.name())
            } else {
                "Valley resupply".into()
            },
            world: world.spec.clone(),
            depot,
            requests,
            fleet: Fleet::default(),
            risk: RiskModel {
                sites,
                hazards,
                masks: Vec::new(),
            },
            weather: Weather {
                atmosphere: Atmosphere::Isa {
                    temperature_offset: 0.0,
                },
                wind: Wind::Layered {
                    layers: vec![
                        WindLayer {
                            altitude: 0.0,
                            speed: 4.0,
                            from_deg: 240.0,
                        },
                        WindLayer {
                            altitude: 500.0,
                            speed: 7.0,
                            from_deg: 255.0,
                        },
                        WindLayer {
                            altitude: 1500.0,
                            speed: 11.0,
                            from_deg: 270.0,
                        },
                    ],
                },
                source: Some("Example profile, not a forecast".into()),
            },
            settings: Settings {
                min_delivery_prob: 0.9,
                seed: 42,
            },
        }
    }
}

impl Default for Fleet {
    fn default() -> Self {
        Fleet {
            trucks: TruckFleet {
                count: 3,
                truck: Truck {
                    capacity_kg: 3_000.0,
                    speed_paved_kmh: 60.0,
                    speed_gravel_kmh: 35.0,
                    speed_track_kmh: 18.0,
                    speed_offroad_kmh: 8.0,
                    handling_min: 20.0,
                    detectability: 1.0,
                },
                cost_per_hour: 150.0,
            },
            drones: DroneFleet {
                count: 4,
                drone: DroneSpec {
                    empty_mass_kg: 25.0,
                    payload_kg: 15.0,
                    battery_wh: 1_800.0,
                    reserve: 0.2,
                    rotor_area_m2: 1.6,
                    cd_area_m2: 0.3,
                    airspeed_ms: 18.0,
                    efficiency: 0.7,
                    cruise_agl_m: 80.0,
                    detectability: 0.5,
                    turnaround_min: 10.0,
                },
                cost_per_hour: 60.0,
            },
            aircraft: AircraftFleet {
                count: 1,
                capacity_kg: 4_000.0,
                bundle: Payload {
                    mass: 500.0,
                    cd_area: 1.2,
                    parachute: Some(Parachute {
                        cd_area: 120.0,
                        deploy_delay: 1.5,
                        inflation_time: 3.0,
                        fill_exponent: 2.0,
                    }),
                },
                cruise_ms: 110.0,
                drop_speed_ms: 67.0,
                drop_height_agl: 400.0,
                rigging_min: 40.0,
                turnaround_min: 45.0,
                detectability: 1.0,
                dz_radius_m: 200.0,
                uncertainty: Uncertainty {
                    wind_sigma: 1.0,
                    release_time_sigma: 0.5,
                    nav_sigma: 10.0,
                    drag_sigma: 0.05,
                },
                samples: 300,
                cost_per_hour: 4_000.0,
            },
        }
    }
}
