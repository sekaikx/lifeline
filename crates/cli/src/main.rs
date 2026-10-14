//! `lifeline`: command-line front end for the planning engine.
//!
//! ```text
//! lifeline example > plan.json                     # worked example, generated terrain
//! lifeline example --lat 28 --lon 84.63 > gorkha.json  # worked example on a real area
//! lifeline plan -s gorkha.json                     # delivery plan as a table
//! lifeline plan -s gorkha.json --json              # full plan for other tools
//! lifeline wind --lat 28 --lon 84.63               # forecast wind profile
//! lifeline flood -s gorkha.json [--date 2022-09-05] # detect new surface water
//! lifeline airdrop ...                             # airdrop physics tools
//!
//! Real areas download open data (elevation, OpenStreetMap, Sentinel-2,
//! forecasts) and cache it, by default in the platform cache directory.
//! ```

mod airdrop;

use clap::{Parser, Subcommand};
use lifeline_airdrop::scenario::{self as drop, Scenario as DropScenario};
use lifeline_geodata::{Area, Fetcher, WindProvider};
use lifeline_planner::{plan, risk_layers, GeneratedSpec, Mode, PlanScenario, World, WorldSpec};
use serde::Serialize;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "lifeline",
    version,
    about = "Resupply planning under uncertainty"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args, Clone)]
struct AreaArgs {
    /// Latitude of the area centre (omit for generated terrain)
    #[arg(long, allow_hyphen_values = true)]
    lat: Option<f64>,
    /// Longitude of the area centre
    #[arg(long, allow_hyphen_values = true)]
    lon: Option<f64>,
    /// Side of the square area [km]
    #[arg(long, default_value_t = 30.0)]
    size_km: f64,
    /// Terrain grid spacing [m]
    #[arg(long, default_value_t = 100.0)]
    cell_m: f64,
    /// Name for the area
    #[arg(long, default_value = "Area")]
    name: String,
}

impl AreaArgs {
    fn area(&self) -> Option<Area> {
        Some(Area {
            name: self.name.clone(),
            lat: self.lat?,
            lon: self.lon?,
            size_m: self.size_km * 1000.0,
            cell_m: self.cell_m,
        })
    }
}

#[derive(Subcommand)]
enum Command {
    /// Print a worked example scenario as JSON
    Example {
        #[command(flatten)]
        area: AreaArgs,
    },
    /// Plan deliveries for a scenario (defaults to the worked example)
    Plan {
        #[arg(short, long)]
        scenario: Option<PathBuf>,
        /// Print the full plan as JSON
        #[arg(long)]
        json: bool,
    },
    /// Forecast wind profile for an area
    Wind {
        #[command(flatten)]
        area: AreaArgs,
        /// Hours from now
        #[arg(long, default_value_t = 0.0)]
        hours: f64,
        /// Use Open-Meteo (full profile; free for non-commercial use or with OPEN_METEO_API_KEY)
        #[arg(long)]
        open_meteo: bool,
    },
    /// Detect new surface water (likely flooding) in a scenario's real area
    Flood {
        #[arg(short, long)]
        scenario: PathBuf,
        /// Review a past date (YYYY-MM-DD) instead of now
        #[arg(long)]
        date: Option<String>,
    },
    /// Airdrop physics: release point, dispersion, numerics, atmosphere
    Airdrop {
        #[command(subcommand)]
        command: airdrop::AirdropCommand,
    },
    /// Answer one `{"cmd": ..., "args": {...}}` request from stdin with JSON,
    /// mirroring the desktop app's commands (scripting and UI tests).
    #[command(hide = true)]
    Rpc,
}

pub fn print_json<T: Serialize>(v: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(v).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn fetcher() -> Result<Fetcher, String> {
    Fetcher::new(Fetcher::default_cache_dir())
}

fn example(area: Option<Area>) -> Result<(World, PlanScenario), String> {
    let world = match area {
        None => World::generate(GeneratedSpec::default())?,
        Some(a) => {
            eprintln!(
                "loading {} from open data (elevation, OpenStreetMap)…",
                a.name
            );
            World::build(&WorldSpec::Real(a), Some(&fetcher()?))?
        }
    };
    let mut s = PlanScenario::example(&world);
    if let WorldSpec::Real(a) = &world.spec {
        // use the current forecast where one is available
        match lifeline_geodata::wind_profile(&fetcher()?, a, 0.0, WindProvider::MetNorway) {
            Ok(f) => {
                s.weather.wind = f.wind;
                s.weather.source = Some(format!(
                    "{}, valid {} UTC",
                    f.source,
                    lifeline_geodata::iso_date(f.valid_unix)
                ));
            }
            Err(e) => eprintln!("no wind forecast ({e}); using the example profile"),
        }
    }
    Ok((world, s))
}

fn build_world(spec: &WorldSpec) -> Result<World, String> {
    match spec {
        WorldSpec::Generated(_) => World::build(spec, None),
        WorldSpec::Real(_) => World::build(spec, Some(&fetcher()?)),
    }
}

fn load_plan(path: &Option<PathBuf>) -> Result<(World, PlanScenario), String> {
    match path {
        None => example(None),
        Some(p) => {
            let s = read_plan(p)?;
            Ok((build_world(&s.world)?, s))
        }
    }
}

/// Hours as `h:mm`, rounded to the minute.
fn read_plan(p: &PathBuf) -> Result<PlanScenario, String> {
    let text =
        std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("invalid scenario: {e}"))
}

fn hours(h: f64) -> String {
    let minutes = (h * 60.0).round() as i64;
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

fn print_plan(world: &World, s: &PlanScenario) -> Result<(), String> {
    let p = plan(world, s)?;
    println!(
        "{}  ({} requests, planned in {:.0} ms)\n",
        s.name,
        s.requests.len(),
        p.compute_ms
    );
    println!(
        "{:<4} {:<32} {:>8} {:<9} {:<9} {:>5} {:>8} {:>6} {:>9}",
        "id", "request", "kg", "priority", "mode", "trips", "arrives", "due", "P(arrive)"
    );
    for (a, r) in p.assignments.iter().zip(&s.requests) {
        let mode = a.mode.map_or("none", Mode::label);
        println!(
            "{:<4} {:<32} {:>8.0} {:<9} {:<9} {:>5} {:>8} {:>6} {:>8.1}%{}",
            r.id,
            r.name.chars().take(32).collect::<String>(),
            r.demand_kg,
            format!("{:?}", r.priority).to_lowercase(),
            mode,
            a.trips,
            if a.mode.is_some() {
                hours(a.completion_h)
            } else {
                "-".into()
            },
            hours(r.due_h),
            100.0 * a.delivery_prob,
            if a.mode.is_none() {
                format!("  unserved: {}", a.reason.as_deref().unwrap_or(""))
            } else if !a.on_time {
                "  late".into()
            } else {
                String::new()
            }
        );
    }
    let m = &p.summary;
    println!(
        "\nexpected delivered {:.0} of {:.0} kg ({:.0} kg on time), critical on time {}/{}, last arrival {}, cost {:.0}",
        m.expected_kg,
        m.demand_kg,
        m.expected_on_time_kg,
        m.critical_on_time,
        m.critical_total,
        hours(m.makespan_h),
        m.cost
    );
    Ok(())
}

#[derive(serde::Deserialize)]
struct RpcRequest {
    cmd: String,
    #[serde(default)]
    args: serde_json::Value,
}

fn rpc() -> Result<serde_json::Value, String> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).map_err(|e| e.to_string())?;
    let req: RpcRequest = serde_json::from_str(&input).map_err(|e| e.to_string())?;
    let arg = |key: &str| req.args[key].clone();
    let drop_scenario = || -> Result<DropScenario, String> {
        serde_json::from_value(arg("scenario")).map_err(|e| format!("invalid scenario: {e}"))
    };
    let plan_scenario = || -> Result<PlanScenario, String> {
        serde_json::from_value(arg("scenario")).map_err(|e| format!("invalid scenario: {e}"))
    };
    let num = |key: &str, default: f64| req.args[key].as_f64().unwrap_or(default);
    let json = |v: Result<serde_json::Value, serde_json::Error>| v.map_err(|e| e.to_string());
    match req.cmd.as_str() {
        "example_plan" => {
            let area: Option<Area> = serde_json::from_value(arg("area")).ok();
            json(serde_json::to_value(example(area)?.1))
        }
        "world" => {
            let spec: WorldSpec = serde_json::from_value(arg("spec")).map_err(|e| e.to_string())?;
            json(serde_json::to_value(build_world(&spec)?))
        }
        "plan" => {
            let s = plan_scenario()?;
            json(serde_json::to_value(plan(&build_world(&s.world)?, &s)?))
        }
        "risk_layers" => {
            let s = plan_scenario()?;
            json(serde_json::to_value(risk_layers(
                &build_world(&s.world)?,
                &s,
            )))
        }
        "wind_forecast" => {
            let area: Area = serde_json::from_value(arg("area")).map_err(|e| e.to_string())?;
            let provider: WindProvider =
                serde_json::from_value(arg("provider")).unwrap_or_default();
            json(serde_json::to_value(lifeline_geodata::wind_profile(
                &fetcher()?,
                &area,
                num("hours", 0.0),
                provider,
            )?))
        }
        "imagery" => {
            let area: Area = serde_json::from_value(arg("area")).map_err(|e| e.to_string())?;
            json(serde_json::to_value(lifeline_geodata::imagery(
                &fetcher()?,
                &area,
                1024,
            )?))
        }
        "detect_flooding" => {
            let area: Area = serde_json::from_value(arg("area")).map_err(|e| e.to_string())?;
            let world = build_world(&WorldSpec::Real(area.clone()))?;
            let as_of = req.args["asOf"].as_i64();
            json(serde_json::to_value(lifeline_geodata::detect_new_water(
                &fetcher()?,
                &area,
                &world.terrain,
                as_of,
            )?))
        }
        "default_scenario" => json(serde_json::to_value(DropScenario::default())),
        "drop_report" => json(serde_json::to_value(drop::drop_report(&drop_scenario()?)?)),
        "monte_carlo_report" => json(serde_json::to_value(drop::monte_carlo_report(
            &drop_scenario()?,
        )?)),
        "numerics_report" => json(serde_json::to_value(drop::numerics_report(
            &drop_scenario()?,
            num("dt", 0.5),
        )?)),
        "atmosphere_profile" => json(serde_json::to_value(drop::atmosphere_profile(
            &drop_scenario()?,
            num("zMax", 11_000.0),
            num("rows", 111.0) as usize,
        ))),
        other => Err(format!("unknown command {other}")),
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Example { area } => print_json(&example(area.area())?.1),
        Command::Wind {
            area,
            hours,
            open_meteo,
        } => {
            let a = area.area().ok_or("--lat and --lon are required")?;
            let provider = if open_meteo {
                WindProvider::OpenMeteo
            } else {
                WindProvider::MetNorway
            };
            let f = lifeline_geodata::wind_profile(&fetcher()?, &a, hours, provider)?;
            println!(
                "{} (valid {} UTC, grid point at {:.0} m)",
                f.source,
                lifeline_geodata::iso_date(f.valid_unix),
                f.elevation
            );
            if let lifeline_core::wind::Wind::Layered { layers } = &f.wind {
                println!("{:>9} {:>8} {:>6}", "alt m", "m/s", "from°");
                for l in layers {
                    println!("{:>9.0} {:>8.1} {:>6.0}", l.altitude, l.speed, l.from_deg);
                }
            }
            Ok(())
        }
        Command::Flood { scenario, date } => {
            let s = read_plan(&scenario)?;
            let WorldSpec::Real(area) = &s.world else {
                return Err("flood detection needs a scenario on a real area".into());
            };
            let world = build_world(&s.world)?;
            let as_of = match date {
                Some(d) => Some(
                    lifeline_geodata::parse_iso_utc(&format!("{d}T12:00:00Z"))
                        .ok_or("--date must be YYYY-MM-DD")?,
                ),
                None => None,
            };
            let w = lifeline_geodata::detect_new_water(&fetcher()?, area, &world.terrain, as_of)?;
            let km2 = |f: f64| f * area.size_m * area.size_m / 1e6;
            println!("method        {}", w.method);
            println!(
                "scenes        {}",
                w.scenes
                    .iter()
                    .map(|s| format!("{} ({:.0}% cloud)", s.date, s.cloud))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "water         {:.1} km² ({:.1}% of the area)",
                km2(w.water_fraction),
                100.0 * w.water_fraction
            );
            println!(
                "new water     {:.1} km² ({:.1}%), {} cells",
                km2(w.new_water_fraction),
                100.0 * w.new_water_fraction,
                w.mask.cells.len()
            );
            println!(
                "not assessed  {:.1}% (cloud, shadow, snow, no data)",
                100.0 * w.obscured_fraction
            );
            Ok(())
        }
        Command::Plan { scenario, json } => {
            let (world, s) = load_plan(&scenario)?;
            if json {
                print_json(&plan(&world, &s)?)
            } else {
                print_plan(&world, &s)
            }
        }
        Command::Airdrop { command } => airdrop::run(command),
        Command::Rpc => {
            let reply = match rpc() {
                Ok(v) => serde_json::json!({ "ok": v }),
                Err(e) => serde_json::json!({ "error": e }),
            };
            println!("{reply}");
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
