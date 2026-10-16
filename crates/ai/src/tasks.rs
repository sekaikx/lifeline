//! What the assistant does, with the prompts that keep it to the facts.

use crate::client::{complete, Completion, Message, Part};
use crate::grid::{cell_center, overlay_grid, GRID};
use crate::settings::{AiSettings, Role};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const GROUNDING: &str = "\
You support the planning cell of a relief or military logistics operation using Lifeline, \
a resupply planner for trucks, drones and parachute airdrops. Lives can depend on your answer.
Rules:
- Use only the data given to you in CONTEXT and in the conversation. Quote the numbers you rely on.
- If the data does not answer the question, say so plainly and say what would. Never invent \
places, coordinates, quantities, times or capabilities.
- Separate what the data shows from your judgement, and flag uncertainty.
- Be brief and concrete. Planners read you under time pressure.";

/// Pull the first JSON object out of a model's answer (models wrap JSON in
/// prose or code fences more often than not).
fn extract_json(text: &str) -> Result<Value, String> {
    let start = text
        .find('{')
        .ok_or("the answer contained no JSON object")?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in text[start..].char_indices() {
        if in_str {
            match c {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&text[start..=start + i])
                        .map_err(|e| format!("invalid JSON: {e}"));
                }
            }
            _ => {}
        }
    }
    Err("the JSON object in the answer is incomplete".into())
}

/// Ask for JSON; on a malformed answer, ask once more with the error.
fn complete_json(
    settings: &AiSettings,
    role: Role,
    system: &str,
    mut messages: Vec<Message>,
    max_tokens: u32,
) -> Result<(Value, Completion), String> {
    let (p, model) = settings.resolve(role)?;
    let first = complete(p, model, system, &messages, max_tokens, 0.1)?;
    match extract_json(&first.text) {
        Ok(v) => Ok((v, first)),
        Err(e) => {
            messages.push(Message {
                role: "assistant".into(),
                content: vec![Part::Text {
                    text: first.text.clone(),
                }],
            });
            messages.push(Message::user(format!(
                "That was not usable ({e}). Reply with the JSON object only, no other text."
            )));
            let second = complete(p, model, system, &messages, max_tokens, 0.0)?;
            let v = extract_json(&second.text)
                .map_err(|e| format!("{} did not return usable JSON: {e}", second.model))?;
            Ok((v, second))
        }
    }
}

// ------------------------------------------------------------------ requests

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceRef {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

/// A catalogue entry the model may match items to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogueRef {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub unit_kg: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParsedItem {
    /// The catalogue item it was matched to
    pub item: Option<String>,
    pub name: String,
    pub quantity: f64,
    pub unit: String,
    /// Weight of one unit [kg]: the catalogue's when matched
    pub unit_kg: Option<f64>,
    /// `quantity * unit_kg`, computed here, never by the model
    pub kg: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParsedRequest {
    pub name: String,
    /// The known place it was matched to, if any
    pub place: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub demand_kg: f64,
    /// `critical`, `high` or `routine`
    pub priority: String,
    pub due_h: f64,
    /// `auto`, `truck`, `drone` or `airdrop`
    pub mode: String,
    pub items: Vec<ParsedItem>,
    /// What the model was unsure of or assumed
    pub notes: String,
    pub model: String,
}

fn match_place<'a>(name: &str, places: &'a [PlaceRef]) -> Option<&'a PlaceRef> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    places
        .iter()
        .find(|p| p.name.to_lowercase() == n)
        .or_else(|| {
            places.iter().find(|p| {
                let pn = p.name.to_lowercase();
                pn.contains(&n) || n.contains(&pn)
            })
        })
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|x| x.is_finite())
}

/// Check and clean a model's request, matching its place to known places.
fn validate_request(
    v: &Value,
    places: &[PlaceRef],
    catalogue: &[CatalogueRef],
    model: &str,
) -> Result<ParsedRequest, String> {
    // Small models are poor at arithmetic: they name items and counts, and
    // every weight is worked out here from the catalogue or a unit weight.
    let items: Vec<ParsedItem> = v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|it| {
                    let id = it["item"].as_str().map(str::trim).unwrap_or("");
                    let known = catalogue
                        .iter()
                        .find(|c| c.id.eq_ignore_ascii_case(id) && !id.is_empty());
                    let quantity = num(&it["quantity"]).unwrap_or(1.0).max(0.0);
                    let unit_kg = known
                        .map(|c| c.unit_kg)
                        .or_else(|| num(&it["unit_kg"]).filter(|k| *k > 0.0 && *k < 50_000.0));
                    let name = it["name"]
                        .as_str()
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty())
                        .or_else(|| known.map(|c| c.name.clone()))?;
                    Some(ParsedItem {
                        item: known.map(|c| c.id.clone()),
                        name,
                        quantity,
                        unit: known.map(|c| c.unit.clone()).unwrap_or_else(|| {
                            it["unit"].as_str().unwrap_or("").trim().to_string()
                        }),
                        unit_kg,
                        kg: unit_kg.map(|u| u * quantity),
                    })
                })
                .filter(|it| it.quantity > 0.0)
                .collect()
        })
        .unwrap_or_default();
    let all_weighed = !items.is_empty() && items.iter().all(|i| i.kg.is_some());
    let item_kg: f64 = items.iter().filter_map(|i| i.kg).sum();
    // a total stated in the message wins; otherwise the computed one
    let stated = num(&v["stated_total_kg"]).filter(|d| *d > 0.0);
    let demand = stated
        .or(all_weighed.then_some(item_kg))
        .or(num(&v["demand_kg"]).filter(|d| *d > 0.0))
        .or((item_kg > 0.0).then_some(item_kg))
        .ok_or("the message does not say how much is needed")?;
    if demand > 1_000_000.0 {
        return Err(format!("{demand} kg is not a plausible request"));
    }
    let priority = match v["priority"]
        .as_str()
        .unwrap_or("high")
        .to_lowercase()
        .as_str()
    {
        "critical" | "urgent" | "immediate" | "flash" => "critical",
        "routine" | "low" | "normal" => "routine",
        _ => "high",
    }
    .to_string();
    let due_h = num(&v["due_h"]).unwrap_or(match priority.as_str() {
        "critical" => 2.0,
        "high" => 8.0,
        _ => 24.0,
    });
    if !(0.05..=720.0).contains(&due_h) {
        return Err(format!("a deadline of {due_h} h is out of range"));
    }
    let mode = match v["mode"].as_str().unwrap_or("auto").to_lowercase().as_str() {
        m @ ("truck" | "drone" | "airdrop") => m.to_string(),
        _ => "auto".into(),
    };
    let place_name = v["place"].as_str().unwrap_or("");
    let place = match_place(place_name, places);
    let mut notes = v["notes"].as_str().unwrap_or("").trim().to_string();
    if place.is_none() && !place_name.trim().is_empty() {
        notes = format!(
            "{}\"{place_name}\" is not a known place on this map; place the request by hand.",
            if notes.is_empty() {
                String::new()
            } else {
                format!("{notes} ")
            }
        );
    }
    let name = v["name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| place.map(|p| p.name.clone()))
        .unwrap_or_else(|| "New request".into());
    Ok(ParsedRequest {
        name,
        place: place.map(|p| p.name.clone()),
        x: place.map(|p| p.x),
        y: place.map(|p| p.y),
        demand_kg: demand,
        priority,
        due_h,
        mode,
        items,
        notes,
        model: model.to_string(),
    })
}

/// Turn a free-text message ("radio: clinic at Pandrung needs 40 units O-neg
/// blood and 200 kg of dressings within 3 hours") into a request. Runs on
/// the `fast` model.
pub fn parse_request(
    settings: &AiSettings,
    text: &str,
    places: &[PlaceRef],
    catalogue: &[CatalogueRef],
) -> Result<ParsedRequest, String> {
    if text.trim().is_empty() {
        return Err("type or paste the message first".into());
    }
    let names: Vec<&str> = places.iter().take(400).map(|p| p.name.as_str()).collect();
    let system = format!(
        "{GROUNDING}\n\nTask: extract one supply request from a field message. Reply with a single JSON object:\n\
{{\"name\": short label for the request, \"place\": the destination exactly as in KNOWN PLACES or as written if not listed, \
\"priority\": \"critical\" (life-threatening, hours), \"high\" or \"routine\", \
\"due_h\": hours from now it is needed by (null if not stated), \
\"mode\": \"auto\" unless the message demands \"truck\", \"drone\" or \"airdrop\", \
\"items\": [{{\"item\": the CATALOGUE id if it is the same thing, else null, \"name\", \"quantity\": count in the unit of issue, \
\"unit\", \"unit_kg\": weight of ONE unit in kg, only for items not in the catalogue}}], \
\"stated_total_kg\": only if the message itself gives a total weight, else null, \
\"notes\": assumptions you made}}\n\
Do not multiply or add weights yourself; the software does that. Convert counts to the catalogue's unit of issue \
(e.g. 3 boxes of dressings with a catalogue unit of box = 3). Typical unit weights for items not in the catalogue: \
1 L water 1 kg, a day's food ration 0.6 kg, a family tent 25 kg, a hygiene kit 2 kg. \
Times like \"by 1800\" with no current time: put null and say so in notes."
    );
    let cat: Vec<String> = catalogue
        .iter()
        .take(300)
        .map(|c| format!("{} = {} (per {}, {} kg)", c.id, c.name, c.unit, c.unit_kg))
        .collect();
    let user = format!(
        "KNOWN PLACES: {}\n\nCATALOGUE:\n{}\n\nMESSAGE:\n{}",
        if names.is_empty() {
            "(none)".into()
        } else {
            names.join("; ")
        },
        if cat.is_empty() {
            "(none)".into()
        } else {
            cat.join("\n")
        },
        text.trim()
    );
    let (v, c) = complete_json(
        settings,
        Role::Fast,
        &system,
        vec![Message::user(user)],
        900,
    )?;
    validate_request(&v, places, catalogue, &c.model)
}

// ------------------------------------------------------------------ imagery

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// flood, damage, blocked_route, landing_zone, drop_zone, settlement,
    /// camp, fire, hazard, other
    pub kind: String,
    /// Grid cell, A1 (north-west) to H8 (south-east)
    pub cell: String,
    /// Centre of the cell in map metres
    pub x: f64,
    pub y: f64,
    pub note: String,
    /// low, medium or high
    pub confidence: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreaAssessment {
    pub summary: String,
    pub findings: Vec<Finding>,
    pub recommendations: Vec<String>,
    pub model: String,
    pub provider: String,
    pub local: bool,
    /// The image as the model saw it, with the grid
    pub grid_png_base64: String,
}

const KINDS: [&str; 10] = [
    "flood",
    "damage",
    "blocked_route",
    "landing_zone",
    "drop_zone",
    "settlement",
    "camp",
    "fire",
    "hazard",
    "other",
];

fn validate_assessment(v: &Value, bounds: [f64; 4]) -> (String, Vec<Finding>, Vec<String>) {
    let summary = v["summary"].as_str().unwrap_or("").trim().to_string();
    let findings = v["findings"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let cell = f["cell"].as_str()?.trim().to_ascii_uppercase();
                    let [x, y] = cell_center(&cell, bounds)?;
                    let kind = f["kind"].as_str().unwrap_or("other").to_lowercase();
                    let kind = if KINDS.contains(&kind.as_str()) {
                        kind
                    } else {
                        "other".into()
                    };
                    let confidence = match f["confidence"].as_str().unwrap_or("low") {
                        c @ ("low" | "medium" | "high") => c.to_string(),
                        _ => "low".into(),
                    };
                    Some(Finding {
                        kind,
                        cell,
                        x,
                        y,
                        note: f["note"].as_str().unwrap_or("").trim().to_string(),
                        confidence,
                    })
                })
                .take(40)
                .collect()
        })
        .unwrap_or_default();
    let recommendations = v["recommendations"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .take(10)
                .collect()
        })
        .unwrap_or_default();
    (summary, findings, recommendations)
}

/// Read a satellite image of the area with the `vision` model. `context`
/// carries what the planner already knows (cover statistics, the sensor and
/// date, roads, settlements, hazards, the plan) as JSON text.
pub fn assess_area(
    settings: &AiSettings,
    png_base64: &str,
    bounds: [f64; 4],
    context: &str,
) -> Result<AreaAssessment, String> {
    let grid = overlay_grid(png_base64)?;
    let km = (bounds[2] - bounds[0]) / 1000.0;
    let system = format!(
        "{GROUNDING}\n\nYou are also an imagery analyst. The image is a satellite view of a {km:.0} km square, north up, \
overlaid with a {GRID}x{GRID} grid: columns A-H west to east, rows 1-8 north to south, each cell {:.1} km wide. \
Grey hatched areas are cloud. Use CONTEXT for the sensor, date, colour scheme and measured statistics; \
trust the measured statistics over your visual impression.\n\
Reply with one JSON object: {{\"summary\": 3-5 sentences on what the area looks like and what matters for getting supplies in, \
\"findings\": [{{\"kind\": one of {}, \"cell\": e.g. \"C4\", \"note\": what you see and why it matters, \"confidence\": \"low\"|\"medium\"|\"high\"}}], \
\"recommendations\": [short actions for the planner]}}. \
Landing and drop zones need open, flat, dry ground away from water and settlements. \
List at most 12 findings; only what you can actually see.",
        km / GRID as f64,
        KINDS.join(", ")
    );
    let msg = Message {
        role: "user".into(),
        content: vec![
            Part::Text {
                text: format!("CONTEXT:\n{context}"),
            },
            Part::Image {
                png_base64: grid.clone(),
            },
        ],
    };
    let (v, c) = complete_json(settings, Role::Vision, &system, vec![msg], 2000)?;
    let (summary, findings, recommendations) = validate_assessment(&v, bounds);
    if summary.is_empty() && findings.is_empty() {
        return Err(format!("{} returned an empty assessment", c.model));
    }
    Ok(AreaAssessment {
        summary,
        findings,
        recommendations,
        model: c.model,
        provider: c.provider,
        local: c.local,
        grid_png_base64: grid,
    })
}

// ------------------------------------------------------------------ analyst

/// Answer the planner in conversation, grounded on `context` (JSON text of
/// the current scenario, plan, imagery statistics and supply position).
pub fn chat(
    settings: &AiSettings,
    context: &str,
    history: &[Message],
) -> Result<Completion, String> {
    if history.is_empty() {
        return Err("ask a question first".into());
    }
    let (p, model) = settings.resolve(Role::Chat)?;
    let system = format!(
        "{GROUNDING}\n\nYou are the operation's logistics analyst. When asked for a report use military-style brevity: \
SITUATION, ASSESSMENT, RECOMMENDATIONS, with times as hours from plan start. Use markdown lists, no tables.\n\nCONTEXT:\n{context}"
    );
    // keep the conversation within a modest window for small local models
    let start = history.len().saturating_sub(16);
    complete(p, model, &system, &history[start..], 1500, 0.3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_is_found_in_chatty_answers() {
        let v = extract_json("Sure! ```json\n{\"a\": {\"b\": \"}\"}, \"c\": 2}\n``` done").unwrap();
        assert_eq!(v["c"], 2);
        assert_eq!(v["a"]["b"], "}");
        assert!(extract_json("no json").is_err());
        assert!(extract_json("{\"a\": 1").is_err());
    }

    fn places() -> Vec<PlaceRef> {
        vec![
            PlaceRef {
                name: "Pandrung".into(),
                x: 100.0,
                y: 200.0,
            },
            PlaceRef {
                name: "Kyamuntar".into(),
                x: -50.0,
                y: 10.0,
            },
        ]
    }

    #[test]
    fn requests_are_checked_and_placed() {
        let cat = vec![CatalogueRef {
            id: "MED-BLD".into(),
            name: "O-neg red cells".into(),
            unit: "unit".into(),
            unit_kg: 0.5,
        }];
        // the model's own (wrong) totals are ignored: 40 x 0.5 + 2 x 1.5
        let v = json!({
            "name": "Clinic blood", "place": "pandrung", "demand_kg": 4.5,
            "priority": "URGENT", "due_h": "3", "mode": "helicopter",
            "items": [{"item": "med-bld", "name": "O-neg blood", "quantity": 40, "unit": "units", "unit_kg": 9},
                      {"item": null, "name": "dressings", "quantity": 2, "unit": "boxes", "unit_kg": "1.5"}]
        });
        let r = validate_request(&v, &places(), &cat, "m").unwrap();
        assert_eq!(r.place.as_deref(), Some("Pandrung"));
        assert_eq!((r.x, r.y), (Some(100.0), Some(200.0)));
        assert_eq!(r.items[0].item.as_deref(), Some("MED-BLD"));
        assert_eq!(r.items[0].kg, Some(20.0));
        assert_eq!(r.demand_kg, 23.0);
        // a total stated in the message wins
        let mut stated = v.clone();
        stated["stated_total_kg"] = json!(30);
        assert_eq!(
            validate_request(&stated, &places(), &cat, "m")
                .unwrap()
                .demand_kg,
            30.0
        );
        assert_eq!(r.priority, "critical");
        assert_eq!(r.due_h, 3.0);
        assert_eq!(r.mode, "auto");

        let unknown = json!({"place": "Atlantis", "demand_kg": 50});
        let r = validate_request(&unknown, &places(), &[], "m").unwrap();
        assert!(r.x.is_none() && r.notes.contains("Atlantis"));
        assert_eq!(r.due_h, 8.0);

        assert!(validate_request(&json!({"place": "Pandrung"}), &places(), &[], "m").is_err());
        assert!(
            validate_request(&json!({"demand_kg": 5, "due_h": 9999}), &places(), &[], "m").is_err()
        );
    }

    #[test]
    fn findings_outside_the_grid_are_dropped() {
        let v = json!({
            "summary": "Flooded plain.",
            "findings": [
                {"kind": "flood", "cell": "c4", "note": "water", "confidence": "high"},
                {"kind": "volcano", "cell": "B2", "note": "?", "confidence": "certain"},
                {"kind": "flood", "cell": "Z9", "note": "bad cell"}
            ],
            "recommendations": ["Avoid C4", ""]
        });
        let (s, f, r) = validate_assessment(&v, [-4000.0, -4000.0, 4000.0, 4000.0]);
        assert_eq!(s, "Flooded plain.");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].cell, "C4");
        assert_eq!([f[0].x, f[0].y], [-1500.0, 500.0]);
        assert_eq!(
            (f[1].kind.as_str(), f[1].confidence.as_str()),
            ("other", "low")
        );
        assert_eq!(r, vec!["Avoid C4".to_string()]);
    }
}
