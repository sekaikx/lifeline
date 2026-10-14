//! Roads and settlements from OpenStreetMap (© OpenStreetMap contributors,
//! ODbL), fetched through the Overpass API.
//!
//! OSM ways become a routable graph: a graph node sits at every way end and
//! every node shared by two or more ways (a junction); each stretch of way
//! between graph nodes becomes one [`Road`]. Surface comes from the
//! `surface` tag when present, otherwise from the road class.

use crate::{Area, Fetcher, LocalFrame};
use lifeline_core::risk::simplify;
use lifeline_ground::{Road, RoadClass, RoadNetwork, Town};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Public Overpass instances, tried in order: the main server is often busy.
const OVERPASS: [&str; 3] = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
];

/// A named settlement, for labels and as a natural delivery point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    /// city, town, village or hamlet
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub population: Option<u64>,
}

#[derive(Deserialize)]
struct Response {
    elements: Vec<Element>,
}

#[derive(Deserialize)]
struct Element {
    #[serde(rename = "type")]
    kind: String,
    id: i64,
    lat: Option<f64>,
    lon: Option<f64>,
    #[serde(default)]
    nodes: Vec<i64>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

const HIGHWAYS: &str = "motorway|trunk|primary|secondary|tertiary|unclassified|residential|track|motorway_link|trunk_link|primary_link|secondary_link|tertiary_link";

fn classify(tags: &HashMap<String, String>) -> Option<RoadClass> {
    if matches!(
        tags.get("access").map(String::as_str),
        Some("no" | "private")
    ) {
        return None;
    }
    let highway = tags.get("highway")?.as_str();
    if let Some(surface) = tags.get("surface") {
        match surface.as_str() {
            "paved" | "asphalt" | "concrete" | "concrete:plates" | "paving_stones" | "sett"
            | "chipseal" => return Some(RoadClass::Paved),
            "gravel" | "fine_gravel" | "compacted" | "pebblestone" => {
                return Some(RoadClass::Gravel)
            }
            "unpaved" | "dirt" | "earth" | "ground" | "mud" | "sand" | "grass" => {
                return Some(if highway == "track" {
                    RoadClass::Track
                } else {
                    RoadClass::Gravel
                })
            }
            _ => {}
        }
    }
    Some(match highway {
        "motorway" | "trunk" | "primary" | "secondary" | "tertiary" | "motorway_link"
        | "trunk_link" | "primary_link" | "secondary_link" | "tertiary_link" => RoadClass::Paved,
        "track" => RoadClass::Track,
        _ => RoadClass::Gravel,
    })
}

/// Build the road graph and place list from an Overpass JSON response.
///
/// Overpass returns whole ways, which can run far outside the area, so roads
/// are clipped to the square `|x|, |y| <= clip` (local metres): a way that
/// leaves and re-enters becomes separate roads.
pub fn network_from_overpass(
    json: &[u8],
    frame: &LocalFrame,
    clip: f64,
) -> Result<(RoadNetwork, Vec<Place>), String> {
    let inside = |p: &[f64; 2]| p[0].abs() <= clip && p[1].abs() <= clip;
    let r: Response =
        serde_json::from_slice(json).map_err(|e| format!("invalid Overpass response: {e}"))?;
    let mut coords: HashMap<i64, [f64; 2]> = HashMap::new();
    let mut ways = Vec::new();
    let mut places = Vec::new();
    for e in &r.elements {
        match e.kind.as_str() {
            "node" => {
                if let (Some(lat), Some(lon)) = (e.lat, e.lon) {
                    let p = frame.to_local(lat, lon);
                    coords.insert(e.id, p);
                    if let (Some(kind), Some(name), true) =
                        (e.tags.get("place"), e.tags.get("name"), inside(&p))
                    {
                        places.push(Place {
                            name: e
                                .tags
                                .get("name:en")
                                .or(e.tags.get("int_name"))
                                .unwrap_or(name)
                                .clone(),
                            kind: kind.clone(),
                            x: p[0],
                            y: p[1],
                            population: e
                                .tags
                                .get("population")
                                .and_then(|v| v.replace([',', ' '], "").parse().ok()),
                        });
                    }
                }
            }
            "way" => {
                if let Some(class) = classify(&e.tags) {
                    if e.nodes.len() >= 2 {
                        ways.push((class, &e.nodes));
                    }
                }
            }
            _ => {}
        }
    }

    // junctions: nodes used by more than one way, or more than once by one way
    let mut uses: HashMap<i64, u32> = HashMap::new();
    for (_, nodes) in &ways {
        for n in nodes.iter() {
            *uses.entry(*n).or_insert(0) += 1;
        }
    }

    let mut index: HashMap<i64, usize> = HashMap::new();
    let mut towns: Vec<Town> = Vec::new();
    let mut graph_node = |id: i64, p: [f64; 2], towns: &mut Vec<Town>| {
        *index.entry(id).or_insert_with(|| {
            towns.push(Town {
                id: towns.len(),
                name: String::new(),
                x: p[0],
                y: p[1],
            });
            towns.len() - 1
        })
    };

    let mut roads = Vec::new();
    for (class, nodes) in ways {
        let all: Vec<(i64, [f64; 2])> = nodes
            .iter()
            .filter_map(|n| coords.get(n).map(|p| (*n, *p)))
            .collect();
        // runs of consecutive nodes inside the clip square
        for run in all.split(|(_, p)| !inside(p)) {
            if run.len() < 2 {
                continue;
            }
            let mut start = 0;
            for k in 1..run.len() {
                let last = k == run.len() - 1;
                if last || uses.get(&run[k].0).copied().unwrap_or(0) > 1 {
                    let a = graph_node(run[start].0, run[start].1, &mut towns);
                    let b = graph_node(run[k].0, run[k].1, &mut towns);
                    if a != b {
                        let line: Vec<[f64; 2]> = run[start..=k].iter().map(|(_, p)| *p).collect();
                        roads.push(Road {
                            a,
                            b,
                            class,
                            points: simplify(&line, 4.0),
                        });
                    }
                    start = k;
                }
            }
        }
    }
    places.sort_by(|a, b| {
        b.population
            .unwrap_or(0)
            .cmp(&a.population.unwrap_or(0))
            .then(a.name.cmp(&b.name))
    });
    Ok((RoadNetwork { towns, roads }, places))
}

/// Download roads usable by trucks and named settlements inside `area`.
pub fn osm(fetcher: &Fetcher, area: &Area) -> Result<(RoadNetwork, Vec<Place>), String> {
    area.validate()?;
    let [s, w, n, e] = area.bbox();
    let bbox = format!("{s:.6},{w:.6},{n:.6},{e:.6}");
    let query = format!(
        "[out:json][timeout:120];\
         way[\"highway\"~\"^({HIGHWAYS})$\"]({bbox});out body;>;out skel qt;\
         node[\"place\"~\"^(city|town|village|hamlet)$\"]({bbox});out body;"
    );
    let body = format!("data={}", urlencode(&query));
    let mut errors = Vec::new();
    for attempt in 0..2 {
        for server in OVERPASS {
            match fetcher.post(
                server,
                "application/x-www-form-urlencoded",
                &body,
                Some(Duration::from_secs(7 * 86_400)),
            ) {
                Ok(bytes) => {
                    return network_from_overpass(&bytes, &area.frame(), area.size_m / 2.0)
                }
                Err(e) => errors.push(e),
            }
        }
        if attempt == 0 {
            std::thread::sleep(Duration::from_secs(5));
        }
    }
    Err(format!(
        "OpenStreetMap is unavailable: {}",
        errors.join("; ")
    ))
}

pub(crate) fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
