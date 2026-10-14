//! # lifeline-ground
//!
//! Road movement: a road network, truck performance on slopes and road
//! classes, and routing that trades travel time against risk.
//!
//! Routing minimises `time + w · E`, where `E = ∫ λ dt` is the expected number
//! of interdictions along the route and `w` (seconds per expected loss) says
//! how much delay is worth one unit of risk. Sweeping `w` from zero upwards
//! traces the Pareto front of fastest-but-exposed to slower-but-covered
//! routes; [`RoutingContext::options`] returns that front.

use lifeline_core::raster::Raster;
use lifeline_core::risk::{
    polyline_length, resample, segment_exposure, simplify, survival, RiskModel,
};
use lifeline_core::search::{dijkstra, grid_astar};
use lifeline_core::stats::Rng;
use lifeline_core::terrain::TerrainGrid;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoadClass {
    Paved,
    Gravel,
    Track,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Town {
    pub id: usize,
    pub name: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Road {
    pub a: usize,
    pub b: usize,
    pub class: RoadClass,
    /// Centre line from town `a` to town `b`
    pub points: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoadNetwork {
    pub towns: Vec<Town>,
    pub roads: Vec<Road>,
}

const TOWN_NAMES: [&str; 24] = [
    "Aldren", "Brask", "Corvin", "Dunmere", "Elsby", "Farrow", "Galen", "Harth", "Ilsen", "Jarrow",
    "Kesten", "Lorne", "Marlow", "Nesby", "Orrin", "Pellam", "Quarry", "Rendle", "Selby", "Tarn",
    "Ulver", "Vance", "Wexley", "Yarrow",
];

impl RoadNetwork {
    /// Generate a plausible network on `terrain`: towns favour low, flat
    /// ground; towns are linked by a Gabriel graph (connected, planar and
    /// road-like: no edge passes close to a third town); each road is
    /// laid out by A* over the terrain so it follows valleys instead of
    /// climbing ridges. The minimum spanning tree becomes paved trunk roads.
    pub fn generate(terrain: &TerrainGrid, seed: u64, towns: usize) -> Self {
        let towns = place_towns(terrain, seed, towns.clamp(2, TOWN_NAMES.len()));
        let edges = gabriel(&towns);
        let trunk = minimum_spanning_tree(&towns, &edges);
        let roads = edges
            .iter()
            .enumerate()
            .filter_map(|(k, &(a, b))| {
                let points = lay_road(terrain, [towns[a].x, towns[a].y], [towns[b].x, towns[b].y])?;
                let steep = max_grade(terrain, &points) > 0.12;
                let class = if trunk.contains(&k) {
                    RoadClass::Paved
                } else if steep {
                    RoadClass::Track
                } else {
                    RoadClass::Gravel
                };
                Some(Road {
                    a,
                    b,
                    class,
                    points,
                })
            })
            .collect();
        RoadNetwork { towns, roads }
    }

    /// True if every town can reach every other.
    pub fn is_connected(&self) -> bool {
        if self.towns.is_empty() {
            return true;
        }
        let mut seen = vec![false; self.towns.len()];
        let mut stack = vec![0];
        seen[0] = true;
        while let Some(t) = stack.pop() {
            for r in &self.roads {
                for (from, to) in [(r.a, r.b), (r.b, r.a)] {
                    if from == t && !seen[to] {
                        seen[to] = true;
                        stack.push(to);
                    }
                }
            }
        }
        seen.into_iter().all(|s| s)
    }
}

fn place_towns(terrain: &TerrainGrid, seed: u64, count: usize) -> Vec<Town> {
    let mut rng = Rng::new(seed ^ 0x7057_4E55);
    let [x0, y0, x1, y1] = terrain.bounds();
    let margin = 0.06 * (x1 - x0);
    let min_dist = 0.55 * (x1 - x0) / (count as f64).sqrt();
    let (lo, hi) = terrain
        .heights
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &h| {
            (a.min(h), b.max(h))
        });
    let mut out: Vec<Town> = Vec::new();
    for attempt in 0..count * 400 {
        if out.len() == count {
            break;
        }
        let x = x0 + margin + rng.uniform() * (x1 - x0 - 2.0 * margin);
        let y = y0 + margin + rng.uniform() * (y1 - y0 - 2.0 * margin);
        let t = (terrain.at(x, y) - lo) / (hi - lo).max(1e-9);
        // prefer valleys and gentle ground; relax spacing if we struggle
        let spacing = if attempt > count * 200 {
            0.7 * min_dist
        } else {
            min_dist
        };
        let flat_enough = terrain.grade(x, y) < 0.15;
        if rng.uniform() > (1.0 - t).powf(1.5) || !flat_enough {
            continue;
        }
        if out.iter().all(|o| (o.x - x).hypot(o.y - y) >= spacing) {
            out.push(Town {
                id: out.len(),
                name: TOWN_NAMES[out.len()].into(),
                x,
                y,
            });
        }
    }
    out
}

fn dist(a: &Town, b: &Town) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Edge (a, b) survives if no third town lies inside the circle whose
/// diameter is ab. Long edges that skip past a town are dropped.
fn gabriel(towns: &[Town]) -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for a in 0..towns.len() {
        for b in a + 1..towns.len() {
            let (cx, cy) = (
                (towns[a].x + towns[b].x) / 2.0,
                (towns[a].y + towns[b].y) / 2.0,
            );
            let r = dist(&towns[a], &towns[b]) / 2.0;
            let blocked = (0..towns.len())
                .any(|c| c != a && c != b && (towns[c].x - cx).hypot(towns[c].y - cy) < r);
            if !blocked {
                edges.push((a, b));
            }
        }
    }
    edges
}

/// Kruskal; returns indices into `edges`.
fn minimum_spanning_tree(towns: &[Town], edges: &[(usize, usize)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&i, &j| {
        let di = dist(&towns[edges[i].0], &towns[edges[i].1]);
        let dj = dist(&towns[edges[j].0], &towns[edges[j].1]);
        di.total_cmp(&dj)
    });
    let mut parent: Vec<usize> = (0..towns.len()).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        p[x] = r;
        r
    }
    let mut tree = Vec::new();
    for k in order {
        let (a, b) = edges[k];
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra] = rb;
            tree.push(k);
        }
    }
    tree
}

fn cell_of(t: &TerrainGrid, p: [f64; 2]) -> (usize, usize) {
    let i = ((p[0] - t.x0) / t.dx).round().clamp(0.0, (t.nx - 1) as f64) as usize;
    let j = ((p[1] - t.y0) / t.dy).round().clamp(0.0, (t.ny - 1) as f64) as usize;
    (i, j)
}

fn cell_center(t: &TerrainGrid, c: (usize, usize)) -> [f64; 2] {
    [t.x0 + c.0 as f64 * t.dx, t.y0 + c.1 as f64 * t.dy]
}

/// Lay a road between two points: A* where steep steps cost much more than
/// flat ones (`len · (1 + 40 · grade²)`), then smooth the staircase.
fn lay_road(t: &TerrainGrid, a: [f64; 2], b: [f64; 2]) -> Option<Vec<[f64; 2]>> {
    let (start, goal) = (cell_of(t, a), cell_of(t, b));
    let h = |c: (usize, usize)| t.heights[c.1 * t.nx + c.0];
    let (path, _) = grid_astar(
        t.nx,
        t.ny,
        start,
        goal,
        |from, to| {
            let len = t.dx * (((from.0 != to.0) as u8 + (from.1 != to.1) as u8) as f64).sqrt();
            let grade = (h(to) - h(from)).abs() / len;
            Some(len * (1.0 + 40.0 * grade * grade))
        },
        |c| {
            let p = cell_center(t, c);
            (p[0] - b[0]).hypot(p[1] - b[1])
        },
    )?;
    let mut points: Vec<[f64; 2]> = path.into_iter().map(|c| cell_center(t, c)).collect();
    points[0] = a;
    *points.last_mut().unwrap() = b;
    Some(simplify(&points, 0.6 * t.dx))
}

fn max_grade(t: &TerrainGrid, points: &[[f64; 2]]) -> f64 {
    resample(points, t.dx)
        .windows(2)
        .map(|w| {
            let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]).max(1e-9);
            (t.at(w[1][0], w[1][1]) - t.at(w[0][0], w[0][1])).abs() / len
        })
        .fold(0.0, f64::max)
}

/// Truck performance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Truck {
    pub capacity_kg: f64,
    pub speed_paved_kmh: f64,
    pub speed_gravel_kmh: f64,
    pub speed_track_kmh: f64,
    pub speed_offroad_kmh: f64,
    /// Loading or unloading time at each end of a trip [min]
    pub handling_min: f64,
    /// Multiplier on risk-site rates (1 = fully exposed)
    pub detectability: f64,
}

impl Truck {
    /// Speed [m/s] on a surface at a given grade: a 10 % climb roughly
    /// halves speed (`v / (1 + 10 · grade)`).
    pub fn speed(&self, surface: Option<RoadClass>, grade: f64) -> f64 {
        let kmh = match surface {
            Some(RoadClass::Paved) => self.speed_paved_kmh,
            Some(RoadClass::Gravel) => self.speed_gravel_kmh,
            Some(RoadClass::Track) => self.speed_track_kmh,
            None => self.speed_offroad_kmh,
        };
        kmh / 3.6 / (1.0 + 10.0 * grade)
    }
}

/// A route from origin to destination.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroundRoute {
    pub points: Vec<[f64; 2]>,
    pub length_m: f64,
    pub offroad_m: f64,
    pub time_s: f64,
    /// Expected interdictions along the route
    pub exposure: f64,
    pub survival: f64,
    /// The risk weight [s per expected loss] that produced this route
    pub risk_weight: f64,
}

/// Travel time and exposure along a polyline on one surface.
fn traverse(
    t: &TerrainGrid,
    field: &Raster,
    truck: &Truck,
    surface: Option<RoadClass>,
    points: &[[f64; 2]],
) -> (f64, f64) {
    let (mut time, mut exposure) = (0.0, 0.0);
    for w in resample(points, t.dx).windows(2) {
        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if len == 0.0 {
            continue;
        }
        let grade = (t.at(w[1][0], w[1][1]) - t.at(w[0][0], w[0][1])).abs() / len;
        let dt = len / truck.speed(surface, grade);
        time += dt;
        exposure += segment_exposure(field, w[0], w[1], dt);
    }
    (time, exposure)
}

struct Edge {
    points: Vec<[f64; 2]>,
    time: f64,
    exposure: f64,
    offroad: f64,
}

/// Everything needed to route trucks across one scenario. Road travel times
/// and exposures are computed once when the context is built.
pub struct RoutingContext<'a> {
    terrain: &'a TerrainGrid,
    network: &'a RoadNetwork,
    field: &'a Raster,
    risk: &'a RiskModel,
    truck: Truck,
    roads: Vec<Option<(f64, f64)>>,
}

impl<'a> RoutingContext<'a> {
    pub fn new(
        terrain: &'a TerrainGrid,
        network: &'a RoadNetwork,
        field: &'a Raster,
        risk: &'a RiskModel,
        truck: Truck,
    ) -> Self {
        let roads = network
            .roads
            .iter()
            .map(|r| {
                let blocked = r
                    .points
                    .windows(2)
                    .any(|w| risk.blocks(w[0], w[1], terrain.dx * 0.5));
                (!blocked).then(|| traverse(terrain, field, &truck, Some(r.class), &r.points))
            })
            .collect();
        Self {
            terrain,
            network,
            field,
            risk,
            truck,
            roads,
        }
    }

    fn offroad(&self, a: [f64; 2], b: [f64; 2]) -> Option<Edge> {
        if self.risk.blocks(a, b, self.terrain.dx * 0.5) {
            return None;
        }
        let points = vec![a, b];
        let (time, exposure) = traverse(self.terrain, self.field, &self.truck, None, &points);
        Some(Edge {
            offroad: (b[0] - a[0]).hypot(b[1] - a[1]),
            points,
            time,
            exposure,
        })
    }

    /// Best route for one risk weight `w` [s per expected loss].
    pub fn route(&self, from: [f64; 2], to: [f64; 2], w: f64) -> Option<GroundRoute> {
        let n = self.network.towns.len();
        let (src, dst) = (n, n + 1);
        let mut edges: Vec<Edge> = Vec::new();
        let mut adjacency: Vec<Vec<(usize, usize, f64)>> = vec![Vec::new(); n + 2];
        let link = |edges: &mut Vec<Edge>,
                    adj: &mut Vec<Vec<(usize, usize, f64)>>,
                    a: usize,
                    b: usize,
                    e: Edge| {
            let cost = e.time + w * e.exposure;
            edges.push(e);
            adj[a].push((b, edges.len() - 1, cost));
        };
        for (k, r) in self.network.roads.iter().enumerate() {
            let Some((time, exposure)) = self.roads[k] else {
                continue;
            };
            let fwd = Edge {
                points: r.points.clone(),
                time,
                exposure,
                offroad: 0.0,
            };
            let mut back_pts = r.points.clone();
            back_pts.reverse();
            let back = Edge {
                points: back_pts,
                time,
                exposure,
                offroad: 0.0,
            };
            link(&mut edges, &mut adjacency, r.a, r.b, fwd);
            link(&mut edges, &mut adjacency, r.b, r.a, back);
        }
        // off-road access to the three nearest towns at each end, plus a
        // direct cross-country leg
        let nearest = |p: [f64; 2]| {
            let mut v: Vec<usize> = (0..n).collect();
            v.sort_by(|&i, &j| {
                let di = (self.network.towns[i].x - p[0]).hypot(self.network.towns[i].y - p[1]);
                let dj = (self.network.towns[j].x - p[0]).hypot(self.network.towns[j].y - p[1]);
                di.total_cmp(&dj)
            });
            v.truncate(3);
            v
        };
        for t in nearest(from) {
            let town = &self.network.towns[t];
            if let Some(e) = self.offroad(from, [town.x, town.y]) {
                link(&mut edges, &mut adjacency, src, t, e);
            }
        }
        for t in nearest(to) {
            let town = &self.network.towns[t];
            if let Some(e) = self.offroad([town.x, town.y], to) {
                link(&mut edges, &mut adjacency, t, dst, e);
            }
        }
        if let Some(e) = self.offroad(from, to) {
            link(&mut edges, &mut adjacency, src, dst, e);
        }

        let (path, _) = dijkstra(&adjacency, src, dst)?;
        let mut points: Vec<[f64; 2]> = Vec::new();
        let (mut time, mut exposure, mut offroad) = (0.0, 0.0, 0.0);
        for k in path {
            let e = &edges[k];
            if !points.is_empty() {
                points.pop();
            }
            points.extend_from_slice(&e.points);
            time += e.time;
            exposure += e.exposure;
            offroad += e.offroad;
        }
        Some(GroundRoute {
            length_m: polyline_length(&points),
            points,
            offroad_m: offroad,
            time_s: time,
            exposure,
            survival: survival(exposure),
            risk_weight: w,
        })
    }

    /// Pareto front of routes from fastest to safest.
    pub fn options(&self, from: [f64; 2], to: [f64; 2]) -> Vec<GroundRoute> {
        const WEIGHTS_H: [f64; 7] = [0.0, 0.25, 1.0, 4.0, 16.0, 64.0, 256.0];
        let mut routes: Vec<GroundRoute> = WEIGHTS_H
            .iter()
            .filter_map(|w| self.route(from, to, w * 3600.0))
            .collect();
        routes.dedup_by(|a, b| {
            (a.time_s - b.time_s).abs() < 1e-6 && (a.exposure - b.exposure).abs() < 1e-12
        });
        let front: Vec<GroundRoute> = routes
            .iter()
            .filter(|r| {
                !routes.iter().any(|o| {
                    o.time_s <= r.time_s
                        && o.exposure <= r.exposure
                        && (o.time_s < r.time_s || o.exposure < r.exposure)
                })
            })
            .cloned()
            .collect();
        let mut front = front;
        front.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
        front
    }
}
