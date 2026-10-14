//! `lifeline`: command-line front end for the planning engine.
//!
//! ```text
//! lifeline example > plan.json      # worked example scenario to edit
//! lifeline plan -s plan.json        # delivery plan as a table
//! lifeline plan -s plan.json --json # full plan for other tools
//! lifeline airdrop ...              # airdrop physics tools
//! ```

mod airdrop;

use clap::{Parser, Subcommand};
use lifeline_airdrop::scenario::{self as drop, Scenario as DropScenario};
use lifeline_planner::{plan, risk_layers, Mode, PlanScenario, World, WorldSpec};
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

#[derive(Subcommand)]
enum Command {
    /// Print the worked example scenario as JSON
    Example,
    /// Plan deliveries for a scenario (defaults to the worked example)
    Plan {
        #[arg(short, long)]
        scenario: Option<PathBuf>,
        /// Print the full plan as JSON
        #[arg(long)]
        json: bool,
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

fn example() -> Result<(World, PlanScenario), String> {
    let world = World::generate(WorldSpec::default())?;
    let s = PlanScenario::example(&world);
    Ok((world, s))
}

fn load_plan(path: &Option<PathBuf>) -> Result<(World, PlanScenario), String> {
    match path {
        None => example(),
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("cannot read {}: {e}", p.display()))?;
            let s: PlanScenario =
                serde_json::from_str(&text).map_err(|e| format!("invalid scenario: {e}"))?;
            Ok((World::generate(s.world)?, s))
        }
    }
}

/// Hours as `h:mm`, rounded to the minute.
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
        "{:<4} {:<20} {:>8} {:<9} {:<9} {:>5} {:>8} {:>6} {:>9}",
        "id", "request", "kg", "priority", "mode", "trips", "arrives", "due", "P(arrive)"
    );
    for (a, r) in p.assignments.iter().zip(&s.requests) {
        let mode = a.mode.map_or("none", Mode::label);
        println!(
            "{:<4} {:<20} {:>8.0} {:<9} {:<9} {:>5} {:>8} {:>6} {:>8.1}%{}",
            r.id,
            r.name,
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
        "example_plan" => json(serde_json::to_value(example()?.1)),
        "world" => {
            let spec: WorldSpec = serde_json::from_value(arg("spec")).map_err(|e| e.to_string())?;
            json(serde_json::to_value(World::generate(spec)?))
        }
        "plan" => {
            let s = plan_scenario()?;
            json(serde_json::to_value(plan(&World::generate(s.world)?, &s)?))
        }
        "risk_layers" => {
            let s = plan_scenario()?;
            json(serde_json::to_value(risk_layers(
                &World::generate(s.world)?,
                &s,
            )))
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
        Command::Example => print_json(&example()?.1),
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
