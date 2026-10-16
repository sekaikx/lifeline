//! Export a scenario and its plan as GeoJSON, KML or Cursor-on-Target (CoT)
//! for GIS and tactical tools (QGIS, Google Earth, ATAK, WinTAK, iTAK).
//!
//! Only real areas can be exported. The planner works in metres east and
//! north of the area centre, and [`LocalFrame`] turns them back into latitude
//! and longitude. Risk masks are cell rasters and are not written.

use crate::model::{Mode, PlanScenario, Priority, Request, WorldSpec};
use crate::{Assignment, ModeOption, OptionDetail, Plan};
use lifeline_geodata::LocalFrame;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Output formats of [`export`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    GeoJson,
    Kml,
    Cot,
}

const NOTE: &str = "Risk masks (cell-based hazard rasters) are not included.";
// colours as 0xRRGGBB
const TRUCK_RGB: u32 = 0x2a78d6;
const DRONE_RGB: u32 = 0xeb6834;
const AIRDROP_RGB: u32 = 0x1baf7a;
const HAZARD_RGB: u32 = 0xc62f2f;
const CRITICAL_RGB: u32 = 0xc62f2f;
const HIGH_RGB: u32 = 0xeb6834;
const ROUTINE_RGB: u32 = 0x8a8a8a;
const DEPOT_RGB: u32 = 0x000000;
/// 40% opaque hazard fill
const HAZARD_FILL_ALPHA: u8 = 102;
const LINE_WIDTH: u32 = 4;
const DAY_S: i64 = 86_400;
/// 0001-01-01T00:00:00Z and 9999-12-31T23:59:59Z, the range [`iso_utc`] prints
const MIN_UNIX: i64 = -62_135_596_800;
const MAX_UNIX: i64 = 253_402_300_799;
/// KML folders, in display order
const FOLDERS: [&str; 5] = ["Depot", "Requests", "Routes", "Observed areas", "Hazards"];

/// The scenario and (if given) its plan as GeoJSON, KML (Google Earth, ATAK,
/// QGIS) or a Cursor-on-Target event stream (ATAK, WinTAK, iTAK).
/// Only real areas can be exported: a generated world has no place on Earth.
/// `now_unix` stamps the CoT events (Unix seconds, UTC).
pub fn export(
    s: &PlanScenario,
    plan: Option<&Plan>,
    format: ExportFormat,
    now_unix: i64,
) -> Result<String, String> {
    let WorldSpec::Real(area) = &s.world else {
        return Err("export needs a real area; generated terrain has no place on Earth".into());
    };
    area.validate()?;
    let frame = area.frame();
    let items = build_items(s, plan);
    match format {
        ExportFormat::GeoJson => geojson(s, &items, &frame),
        ExportFormat::Kml => Ok(kml(s, &items, &frame)),
        ExportFormat::Cot => Ok(cot(s, &items, &frame, now_unix)),
    }
}

/// File extension for a format: "geojson", "kml" or "cot".
pub fn file_extension(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::GeoJson => "geojson",
        ExportFormat::Kml => "kml",
        ExportFormat::Cot => "cot",
    }
}

/// One exported feature. Geometry stays in local metres until written.
struct Item {
    /// GeoJSON `kind` property
    kind: &'static str,
    /// Unique within the scenario; the CoT uid is `lifeline-{slug}-{id}`
    id: String,
    name: String,
    /// Plain-text description, used by KML and as CoT remarks
    text: String,
    /// KML style id
    style: &'static str,
    /// Stroke colour of routes and hazards
    rgb: u32,
    props: Map<String, Value>,
    geom: Geom,
}

enum Geom {
    Point([f64; 2]),
    /// Route vertices
    Line(Vec<[f64; 2]>),
    /// Closed ring: the first vertex is repeated at the end
    Ring(Vec<[f64; 2]>),
}

impl Geom {
    /// The first vertex (the anchor of a CoT event).
    fn first(&self) -> [f64; 2] {
        match self {
            Geom::Point(p) => *p,
            Geom::Line(v) | Geom::Ring(v) => v.first().copied().unwrap_or([0.0, 0.0]),
        }
    }
}

fn build_items(s: &PlanScenario, plan: Option<&Plan>) -> Vec<Item> {
    let mut items = Vec::new();
    let d = &s.depot;
    items.push(Item {
        kind: "depot",
        id: "depot".into(),
        name: d.name.clone(),
        text: if d.airstrip {
            "airstrip"
        } else {
            "no airstrip"
        }
        .into(),
        style: "depot",
        rgb: DEPOT_RGB,
        props: obj(json!({ "airstrip": d.airstrip })),
        geom: Geom::Point([d.x, d.y]),
    });

    for r in &s.requests {
        let a = plan.and_then(|p| p.assignments.iter().find(|a| a.request_id == r.id));
        let mut p = obj(json!({
            "id": r.id,
            "demand_kg": r.demand_kg,
            "priority": priority_name(r.priority),
            "due_h": r.due_h,
        }));
        let mut text = format!(
            "{} kg · {} · due {}",
            short(r.demand_kg, 1),
            priority_name(r.priority),
            hm(r.due_h)
        );
        if let Some(a) = a {
            p.extend(assignment_props(a));
            text.push_str(&match a.mode {
                Some(m) => format!(
                    " · {}, arrives {}, P={:.1}%",
                    m.label(),
                    hm(a.first_arrival_h),
                    a.delivery_prob * 100.0
                ),
                None => " · not served".to_string(),
            });
        }
        items.push(Item {
            kind: "request",
            id: r.id.clone(),
            name: r.name.clone(),
            text,
            style: priority_style(r.priority),
            rgb: priority_rgb(r.priority),
            props: p,
            geom: Geom::Point([r.x, r.y]),
        });

        // a route needs two vertices to be a line
        let Some(o) = a.and_then(chosen_option).filter(|o| o.route.len() >= 2) else {
            continue;
        };
        items.push(route_item(r, o));
        if let OptionDetail::Airdrop {
            release_point,
            bundles_per_sortie,
            dz_probability,
            ..
        } = &o.detail
        {
            items.push(Item {
                kind: "release_point",
                id: format!("{}-release", r.id),
                name: format!("Release point {}", r.id),
                text: format!(
                    "{bundles_per_sortie} bundles per sortie, {:.1}% in drop zone",
                    dz_probability * 100.0
                ),
                style: "release",
                rgb: AIRDROP_RGB,
                props: obj(json!({ "request_id": r.id })),
                geom: Geom::Point(*release_point),
            });
        }
    }

    for site in &s.risk.sites {
        items.push(Item {
            kind: "observed_area",
            id: site.id.clone(),
            name: site.name.clone(),
            text: format!(
                "range {} m, height {} m, {} per min",
                short(site.range, 0),
                short(site.height, 1),
                short(site.rate_per_min, 3)
            ),
            style: "observed",
            rgb: HAZARD_RGB,
            props: obj(json!({
                "id": site.id,
                "range_m": site.range,
                "height_m": site.height,
                "rate_per_min": site.rate_per_min,
            })),
            geom: Geom::Point([site.x, site.y]),
        });
    }

    for h in &s.risk.hazards {
        // a polygon needs three vertices; validation rejects fewer
        if h.polygon.len() < 3 {
            continue;
        }
        let Some(first) = h.polygon.first().copied() else {
            continue;
        };
        let mut ring = h.polygon.clone();
        ring.push(first);
        let mut text = Vec::new();
        if h.impassable {
            text.push("impassable".to_string());
        }
        text.push(format!("{} per min", short(h.rate_per_min, 3)));
        items.push(Item {
            kind: "hazard",
            id: h.id.clone(),
            name: h.name.clone(),
            text: text.join(", "),
            style: "hazard",
            rgb: HAZARD_RGB,
            props: obj(json!({
                "id": h.id,
                "impassable": h.impassable,
                "rate_per_min": h.rate_per_min,
            })),
            geom: Geom::Ring(ring),
        });
    }
    items
}

/// The option an assignment uses: the one for its chosen mode.
fn chosen_option(a: &Assignment) -> Option<&ModeOption> {
    let m = a.mode?;
    a.options.iter().find(|o| o.mode == m)
}

fn route_item(r: &Request, o: &ModeOption) -> Item {
    Item {
        kind: "route",
        id: format!("{}-{}", r.id, mode_name(o.mode)),
        name: format!("{} by {}", r.name, o.mode.label()),
        text: format!(
            "{} · {} km, {} one way",
            o.mode.label(),
            short(o.distance_km, 1),
            hm(o.one_way_h)
        ),
        style: route_style(o.mode),
        rgb: mode_rgb(o.mode),
        props: obj(json!({
            "request_id": r.id,
            "mode": mode_name(o.mode),
            "distance_km": o.distance_km,
            "one_way_h": o.one_way_h,
        })),
        geom: Geom::Line(o.route.clone()),
    }
}

fn assignment_props(a: &Assignment) -> Map<String, Value> {
    obj(json!({
        "mode": a.mode.map_or("none", mode_name),
        "trips": a.trips,
        "first_arrival_h": a.first_arrival_h,
        "completion_h": a.completion_h,
        "on_time": a.on_time,
        "delivery_prob": a.delivery_prob,
        "expected_kg": a.expected_kg,
    }))
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Truck => "truck",
        Mode::Drone => "drone",
        Mode::Airdrop => "airdrop",
    }
}

fn mode_rgb(m: Mode) -> u32 {
    match m {
        Mode::Truck => TRUCK_RGB,
        Mode::Drone => DRONE_RGB,
        Mode::Airdrop => AIRDROP_RGB,
    }
}

fn route_style(m: Mode) -> &'static str {
    match m {
        Mode::Truck => "route-truck",
        Mode::Drone => "route-drone",
        Mode::Airdrop => "route-airdrop",
    }
}

fn priority_name(p: Priority) -> &'static str {
    match p {
        Priority::Critical => "critical",
        Priority::High => "high",
        Priority::Routine => "routine",
    }
}

fn priority_style(p: Priority) -> &'static str {
    match p {
        Priority::Critical => "priority-critical",
        Priority::High => "priority-high",
        Priority::Routine => "priority-routine",
    }
}

fn priority_rgb(p: Priority) -> u32 {
    match p {
        Priority::Critical => CRITICAL_RGB,
        Priority::High => HIGH_RGB,
        Priority::Routine => ROUTINE_RGB,
    }
}

/// CoT event type for a feature kind.
fn cot_type(kind: &str) -> &'static str {
    match kind {
        "depot" => "a-f-G-U-C-I",
        "observed_area" => "a-h-G",
        "route" | "hazard" => "u-d-f",
        _ => "b-m-p-s-p-i",
    }
}

/// Folder of a feature kind in the KML document.
fn folder_of(kind: &str) -> &'static str {
    match kind {
        "depot" => "Depot",
        "request" => "Requests",
        "route" | "release_point" => "Routes",
        "observed_area" => "Observed areas",
        "hazard" => "Hazards",
        _ => "",
    }
}

/// Latitude and longitude [deg] of a local point, rounded to 6 decimals.
fn latlon(frame: &LocalFrame, p: [f64; 2]) -> (f64, f64) {
    let (lat, lon) = frame.to_geo(p[0], p[1]);
    (round6(lat), round6(lon))
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// Hours as h:mm, rounded to the minute.
fn hm(h: f64) -> String {
    let mins = (h * 60.0).round();
    if !mins.is_finite() {
        return "-".into();
    }
    // the float-to-int cast saturates, and unsigned_abs cannot overflow
    let mins = mins as i64;
    let sign = if mins < 0 { "-" } else { "" };
    let m = mins.unsigned_abs();
    format!("{sign}{}:{:02}", m / 60, m % 60)
}

/// `v` with `dp` decimals and trailing zeros dropped.
fn short(v: f64, dp: usize) -> String {
    let s = format!("{v:.dp$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Lowercase ASCII letters and digits, other runs become one dash.
fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "scenario".into()
    } else {
        out
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// KML aabbggrr colour string.
fn abgr(rgb: u32, alpha: u8) -> String {
    format!(
        "{alpha:02x}{:02x}{:02x}{:02x}",
        rgb & 0xff,
        (rgb >> 8) & 0xff,
        (rgb >> 16) & 0xff
    )
}

/// ARGB as the signed 32-bit integer CoT uses, opaque.
fn argb(rgb: u32) -> i32 {
    (0xff00_0000 | rgb) as i32
}

/// ISO-8601 UTC time of a Unix timestamp, e.g. `2023-11-14T22:13:20Z`.
/// Times outside years 1 to 9999 are clamped.
fn iso_utc(unix: i64) -> String {
    let unix = unix.clamp(MIN_UNIX, MAX_UNIX);
    let days = unix.div_euclid(DAY_S);
    let secs = unix.rem_euclid(DAY_S);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    )
}

fn geojson(s: &PlanScenario, items: &[Item], frame: &LocalFrame) -> Result<String, String> {
    let features: Vec<Value> = items
        .iter()
        .map(|it| {
            let mut props = it.props.clone();
            props.insert("kind".into(), json!(it.kind));
            props.insert("name".into(), json!(it.name));
            json!({
                "type": "Feature",
                "properties": Value::Object(props),
                "geometry": geojson_geometry(frame, &it.geom),
            })
        })
        .collect();
    let doc = json!({
        "type": "FeatureCollection",
        "properties": {"name": s.name, "generator": "Lifeline", "note": NOTE},
        "features": features,
    });
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

fn geojson_geometry(frame: &LocalFrame, g: &Geom) -> Value {
    // GeoJSON positions are [lon, lat]
    let pos = |p: &[f64; 2]| {
        let (lat, lon) = latlon(frame, *p);
        json!([lon, lat])
    };
    match g {
        Geom::Point(p) => json!({"type": "Point", "coordinates": pos(p)}),
        Geom::Line(v) => {
            let coords: Vec<Value> = v.iter().map(pos).collect();
            json!({"type": "LineString", "coordinates": coords})
        }
        Geom::Ring(v) => {
            let ring: Vec<Value> = v.iter().map(pos).collect();
            json!({"type": "Polygon", "coordinates": [ring]})
        }
    }
}

fn kml(s: &PlanScenario, items: &[Item], frame: &LocalFrame) -> String {
    let mut o = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<kml xmlns=\"http://www.opengis.net/kml/2.2\">\n<Document>\n",
    );
    o.push_str(&format!("<name>{}</name>\n", xml_escape(&s.name)));
    kml_styles(&mut o);
    for folder in FOLDERS {
        o.push_str(&format!("<Folder>\n<name>{folder}</name>\n"));
        for it in items.iter().filter(|it| folder_of(it.kind) == folder) {
            o.push_str(&format!(
                "<Placemark>\n<name>{}</name>\n<description>{}</description>\n<styleUrl>#{}</styleUrl>\n",
                xml_escape(&it.name),
                xml_escape(&it.text),
                it.style
            ));
            o.push_str(&kml_geometry(frame, &it.geom));
            o.push_str("</Placemark>\n");
        }
        o.push_str("</Folder>\n");
    }
    o.push_str("</Document>\n</kml>\n");
    o
}

fn kml_styles(o: &mut String) {
    for m in Mode::ALL {
        o.push_str(&format!(
            "<Style id=\"{}\"><LineStyle><color>{}</color><width>{LINE_WIDTH}</width></LineStyle></Style>\n",
            route_style(m),
            abgr(mode_rgb(m), 255)
        ));
    }
    let icons = [
        ("priority-critical", CRITICAL_RGB),
        ("priority-high", HIGH_RGB),
        ("priority-routine", ROUTINE_RGB),
        ("depot", DEPOT_RGB),
        ("observed", HAZARD_RGB),
        ("release", AIRDROP_RGB),
    ];
    for (id, rgb) in icons {
        o.push_str(&format!(
            "<Style id=\"{id}\"><IconStyle><color>{}</color></IconStyle></Style>\n",
            abgr(rgb, 255)
        ));
    }
    o.push_str(&format!(
        "<Style id=\"hazard\"><LineStyle><color>{}</color><width>2</width></LineStyle><PolyStyle><color>{}</color></PolyStyle></Style>\n",
        abgr(HAZARD_RGB, 255),
        abgr(HAZARD_RGB, HAZARD_FILL_ALPHA)
    ));
}

fn kml_geometry(frame: &LocalFrame, g: &Geom) -> String {
    match g {
        Geom::Point(p) => format!(
            "<Point><coordinates>{}</coordinates></Point>\n",
            kml_coords(frame, std::slice::from_ref(p))
        ),
        Geom::Line(v) => format!(
            "<LineString><coordinates>{}</coordinates></LineString>\n",
            kml_coords(frame, v)
        ),
        Geom::Ring(v) => format!(
            "<Polygon><outerBoundaryIs><LinearRing><coordinates>{}</coordinates></LinearRing></outerBoundaryIs></Polygon>\n",
            kml_coords(frame, v)
        ),
    }
}

/// `lon,lat` tuples separated by spaces.
fn kml_coords(frame: &LocalFrame, pts: &[[f64; 2]]) -> String {
    pts.iter()
        .map(|p| {
            let (lat, lon) = latlon(frame, *p);
            format!("{lon},{lat}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn cot(s: &PlanScenario, items: &[Item], frame: &LocalFrame, now_unix: i64) -> String {
    let time = iso_utc(now_unix);
    let stale = iso_utc(now_unix.saturating_add(DAY_S));
    let slug = slug(&s.name);
    let mut o = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<events>\n");
    for it in items {
        let uid = format!("lifeline-{slug}-{}", it.id);
        let (lat, lon) = latlon(frame, it.geom.first());
        o.push_str(&format!(
            "<event version=\"2.0\" uid=\"{}\" type=\"{}\" how=\"h-e\" time=\"{time}\" start=\"{time}\" stale=\"{stale}\">\n",
            xml_escape(&uid),
            cot_type(it.kind)
        ));
        o.push_str(&format!(
            "<point lat=\"{lat}\" lon=\"{lon}\" hae=\"0\" ce=\"9999999\" le=\"9999999\"/>\n"
        ));
        o.push_str(&format!(
            "<detail>\n<contact callsign=\"{}\"/>\n",
            xml_escape(&it.name)
        ));
        let vertices: &[[f64; 2]] = match &it.geom {
            Geom::Point(_) => &[],
            Geom::Line(v) | Geom::Ring(v) => v.as_slice(),
        };
        if !vertices.is_empty() {
            for p in vertices {
                let (la, lo) = latlon(frame, *p);
                o.push_str(&format!("<link point=\"{la},{lo}\"/>\n"));
            }
            o.push_str(&format!(
                "<strokeColor value=\"{}\"/>\n<strokeWeight value=\"{LINE_WIDTH}\"/>\n",
                argb(it.rgb)
            ));
        }
        o.push_str(&format!(
            "<remarks>{}</remarks>\n</detail>\n</event>\n",
            xml_escape(&it.text)
        ));
    }
    o.push_str("</events>\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GeneratedSpec, World, WorldSpec};
    use lifeline_geodata::Area;

    /// The example scenario on a real area, planned. The terrain is generated
    /// so that no network is needed.
    fn planned() -> (PlanScenario, Plan) {
        let world = World {
            spec: WorldSpec::Real(Area {
                name: "Test".into(),
                lat: 28.0,
                lon: 84.63,
                size_m: 24_000.0,
                cell_m: 150.0,
            }),
            ..World::generate(GeneratedSpec::default()).expect("generated terrain")
        };
        let mut s = PlanScenario::example(&world);
        s.name = "R&D <test>".into();
        let plan = crate::plan(&world, &s).expect("plan");
        (s, plan)
    }

    /// Every `&` must start an entity reference.
    fn assert_escaped(xml: &str) {
        for (i, _) in xml.match_indices('&') {
            let rest = &xml[i..];
            assert!(
                ["&amp;", "&lt;", "&gt;", "&quot;", "&apos;", "&#"]
                    .iter()
                    .any(|e| rest.starts_with(e)),
                "unescaped & at byte {i}"
            );
        }
    }

    /// Value of attribute `name` in the first tag of `s`.
    fn attr<'a>(s: &'a str, name: &str) -> &'a str {
        let key = format!(" {name}=\"");
        let start = s.find(&key).map_or(0, |i| i + key.len());
        let end = s[start..].find('"').map_or(s.len(), |j| start + j);
        &s[start..end]
    }

    #[test]
    fn geojson_carries_depot_requests_and_routes() {
        let (s, plan) = planned();
        let text = export(&s, Some(&plan), ExportFormat::GeoJson, 0).expect("geojson");
        let doc: Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(doc["properties"]["name"], "R&D <test>");
        let feats = doc["features"].as_array().expect("features");

        let depot = feats
            .iter()
            .find(|f| f["properties"]["kind"] == "depot")
            .expect("depot feature");
        let c = &depot["geometry"]["coordinates"];
        let (lon, lat) = (c[0].as_f64().unwrap(), c[1].as_f64().unwrap());
        assert!((lon - 84.63).abs() < 0.2 && (lat - 28.0).abs() < 0.2);

        let requests = feats
            .iter()
            .filter(|f| f["properties"]["kind"] == "request")
            .count();
        assert_eq!(requests, s.requests.len());
        assert!(feats
            .iter()
            .any(|f| f["properties"]["kind"] == "route" && f["geometry"]["type"] == "LineString"));

        let bare = export(&s, None, ExportFormat::GeoJson, 0).expect("geojson without plan");
        assert!(!bare.contains("\"route\""));
        assert_eq!(file_extension(ExportFormat::Cot), "cot");
    }

    #[test]
    fn kml_is_complete_and_escaped() {
        let (s, plan) = planned();
        let kml = export(&s, Some(&plan), ExportFormat::Kml, 0).expect("kml");
        assert!(kml.contains("<kml xmlns=\"http://www.opengis.net/kml/2.2\">"));
        assert!(kml.contains("<name>R&amp;D &lt;test&gt;</name>"));
        for r in &s.requests {
            assert!(kml.contains(&xml_escape(&r.name)), "missing {}", r.name);
        }
        assert_escaped(&kml);
    }

    #[test]
    fn cot_has_one_event_per_feature_with_later_stale() {
        let (s, plan) = planned();
        let cot = export(&s, Some(&plan), ExportFormat::Cot, 1_700_000_000).expect("cot");
        assert!(cot.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<events>"));

        let routes = plan.assignments.iter().filter(|a| a.mode.is_some()).count();
        let releases = plan
            .assignments
            .iter()
            .filter(|a| a.mode == Some(Mode::Airdrop))
            .count();
        let expected =
            1 + s.requests.len() + s.risk.sites.len() + routes + s.risk.hazards.len() + releases;
        let events: Vec<&str> = cot.split("<event ").skip(1).collect();
        assert_eq!(events.len(), expected);
        for e in events {
            // both are fixed-width ISO strings, so string order is time order
            assert!(attr(e, "stale") > attr(e, "time"), "{e}");
        }
        assert_escaped(&cot);
    }

    #[test]
    fn iso_utc_matches_known_times() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(i64::MAX), "9999-12-31T23:59:59Z");
    }

    #[test]
    fn generated_world_is_refused() {
        let world = World::generate(GeneratedSpec::default()).expect("generated");
        let s = PlanScenario::example(&world);
        let err = export(&s, None, ExportFormat::Kml, 0).unwrap_err();
        assert!(err.contains("no place on Earth"), "{err}");
    }
}
