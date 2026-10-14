//! Scenario types: the world, the depot, the fleet and the delivery requests.

use lifeline_airdrop::dynamics::{Parachute, Payload};
use lifeline_airdrop::montecarlo::Uncertainty;
use lifeline_core::atmosphere::Atmosphere;
use lifeline_core::risk::{HazardArea, RiskModel, RiskSite};
use lifeline_core::terrain::TerrainGrid;
use lifeline_core::wind::{Wind, WindLayer};
use lifeline_drone::DroneSpec;
use lifeline_ground::{RoadClass, RoadNetwork, Truck};
use serde::{Deserialize, Serialize};

/// Parameters of a generated world. The same spec always gives the same
/// terrain and roads, so scenarios stay small and shareable.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSpec {
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

impl Default for WorldSpec {
    fn default() -> Self {
        WorldSpec {
            seed: 11,
            size_m: 24_000.0,
            cells: 160,
            base_m: 120.0,
            relief_m: 520.0,
            towns: 18,
        }
    }
}

impl WorldSpec {
    pub fn validate(&self) -> Result<(), String> {
        if !(2_000.0..=200_000.0).contains(&self.size_m) || !(16..=400).contains(&self.cells) {
            return Err("world: size must be 2-200 km and cells 16-400".into());
        }
        if !(2..=24).contains(&self.towns) || self.relief_m < 0.0 {
            return Err("world: 2-24 towns, non-negative relief".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct World {
    pub spec: WorldSpec,
    pub terrain: TerrainGrid,
    pub network: RoadNetwork,
}

impl World {
    pub fn generate(spec: WorldSpec) -> Result<Self, String> {
        spec.validate()?;
        let terrain = TerrainGrid::procedural(
            spec.seed,
            spec.size_m,
            spec.cells,
            spec.base_m,
            spec.relief_m,
        );
        let network = RoadNetwork::generate(&terrain, spec.seed, spec.towns);
        Ok(World {
            spec,
            terrain,
            network,
        })
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
        if !(0.0..=1.0).contains(&self.settings.min_delivery_prob) {
            return Err("risk tolerance must be between 0 and 1".into());
        }
        for r in &self.requests {
            if !(r.demand_kg > 0.0) || !(r.due_h > 0.0) {
                return Err(format!("{}: demand and deadline must be positive", r.name));
            }
        }
        Ok(())
    }

    /// A worked example on the default world: a depot with an airstrip in
    /// the south-west, seven requests from a 12 kg medical resupply to a 6 t
    /// water delivery, three observed road sections and two hazard areas.
    pub fn example(world: &World) -> PlanScenario {
        let towns = &world.network.towns;
        let half = world.spec.size_m / 2.0;
        let near = |x: f64, y: f64, skip: &[usize]| {
            (0..towns.len())
                .filter(|i| !skip.contains(i))
                .min_by(|&a, &b| {
                    let da = (towns[a].x - x).hypot(towns[a].y - y);
                    let db = (towns[b].x - x).hypot(towns[b].y - y);
                    da.total_cmp(&db)
                })
                .unwrap()
        };
        let d = near(-0.65 * half, -0.65 * half, &[]);
        let depot = Depot {
            name: format!("{} depot", towns[d].name),
            x: towns[d].x,
            y: towns[d].y,
            airstrip: true,
        };

        let mut used = vec![d];
        let mut pick = |fx: f64, fy: f64| {
            let t = near(fx * half, fy * half, &used);
            used.push(t);
            &towns[t]
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
        let mut requests: Vec<Request> = specs
            .iter()
            .enumerate()
            .map(|(k, &(fx, fy, name, kg, priority, due))| {
                let t = pick(fx, fy);
                Request {
                    id: format!("R{}", k + 1),
                    name: name.into(),
                    x: t.x,
                    y: t.y,
                    demand_kg: kg,
                    priority,
                    due_h: due,
                    mode: ModeChoice::Auto,
                }
            })
            .collect();

        // Two off-road sites on high ground: a relay post close to the depot
        // and a mountain village cut off from the road network.
        let t = &world.terrain;
        let highest = |min: f64, max: f64| {
            let mut best = (f64::NEG_INFINITY, 0.0, 0.0);
            for j in 0..t.ny {
                for i in 0..t.nx {
                    let (x, y) = (t.x0 + i as f64 * t.dx, t.y0 + j as f64 * t.dy);
                    let r = (x - depot.x).hypot(y - depot.y);
                    let h = t.heights[j * t.nx + i];
                    if (min..max).contains(&r) && h > best.0 {
                        best = (h, x, y);
                    }
                }
            }
            (best.1, best.2)
        };
        let (rx, ry) = highest(0.0, 9_000.0);
        requests.push(Request {
            id: "R6".into(),
            name: "Relay post".into(),
            x: rx,
            y: ry,
            demand_kg: 25.0,
            priority: Priority::High,
            due_h: 4.0,
            mode: ModeChoice::Auto,
        });
        let (vx, vy) = highest(13_000.0, 22_000.0);
        requests.push(Request {
            id: "R7".into(),
            name: "Mountain village".into(),
            x: vx,
            y: vy,
            demand_kg: 1_500.0,
            priority: Priority::Critical,
            due_h: 1.5,
            mode: ModeChoice::Auto,
        });

        // Risk sites: on high ground overlooking the middle of three paved
        // roads, ordered by distance from the depot so the example is stable.
        let mut paved: Vec<&lifeline_ground::Road> = world
            .network
            .roads
            .iter()
            .filter(|r| r.class == RoadClass::Paved)
            .collect();
        let mid = |r: &lifeline_ground::Road| r.points[r.points.len() / 2];
        paved.sort_by(|a, b| {
            let (ma, mb) = (mid(a), mid(b));
            let da = (ma[0] - depot.x).hypot(ma[1] - depot.y);
            let db = (mb[0] - depot.x).hypot(mb[1] - depot.y);
            da.total_cmp(&db)
        });
        let chosen: Vec<&&lifeline_ground::Road> =
            paved.iter().skip(1).step_by(3).take(3).collect();
        let sites = chosen
            .iter()
            .enumerate()
            .map(|(k, r)| {
                let m = mid(r);
                // highest point 1-2.5 km from the road midpoint
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

        // hazard areas on two gravel roads
        let gravel: Vec<&lifeline_ground::Road> = world
            .network
            .roads
            .iter()
            .filter(|r| r.class != RoadClass::Paved)
            .collect();
        let square = |c: [f64; 2], s: f64| {
            vec![
                [c[0] - s, c[1] - s],
                [c[0] + s, c[1] - s],
                [c[0] + s, c[1] + s],
                [c[0] - s, c[1] + s],
            ]
        };
        let mut hazards = Vec::new();
        if let Some(r) = gravel.first() {
            hazards.push(HazardArea {
                id: "H1".into(),
                name: "Washed-out crossing".into(),
                polygon: square(mid(r), 450.0),
                rate_per_min: 0.0,
                impassable: true,
            });
        }
        if let Some(r) = gravel.get(gravel.len() / 2) {
            hazards.push(HazardArea {
                id: "H2".into(),
                name: "Reported mines".into(),
                polygon: square(mid(r), 600.0),
                rate_per_min: 0.25,
                impassable: false,
            });
        }

        PlanScenario {
            name: "Valley resupply".into(),
            world: world.spec,
            depot,
            requests,
            fleet: Fleet::default(),
            risk: RiskModel { sites, hazards },
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
