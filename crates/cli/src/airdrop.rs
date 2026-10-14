//! `lifeline airdrop ...`: the airdrop physics tools.
//!
//! ```text
//! lifeline airdrop scenario > drop.json   # default drop scenario
//! lifeline airdrop drop -s drop.json      # release point and trajectory
//! lifeline airdrop montecarlo -n 5000     # landing dispersion
//! lifeline airdrop numerics               # Euler vs RK4 convergence
//! lifeline airdrop profile --csv          # ISA and wind table
//! ```

use crate::print_json;
use clap::{Subcommand, ValueEnum};
use lifeline_airdrop::integrate::Method;
use lifeline_airdrop::scenario::{
    atmosphere_profile, drop_report, monte_carlo_report, numerics_report, Scenario,
};
use std::path::PathBuf;

#[derive(Clone, Copy, ValueEnum)]
enum MethodArg {
    Euler,
    Rk4,
}

#[derive(clap::Args)]
pub struct Common {
    /// Scenario JSON file (defaults to the built-in humanitarian drop)
    #[arg(short, long)]
    scenario: Option<PathBuf>,
    /// Override the integration method
    #[arg(long, value_enum)]
    method: Option<MethodArg>,
    /// Override the integration step [s]
    #[arg(long)]
    dt: Option<f64>,
    /// Print the full report as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
pub enum AirdropCommand {
    /// Print the default scenario as JSON (a template to edit)
    Scenario,
    /// Solve the release point and simulate the drop
    Drop(Common),
    /// Monte Carlo landing dispersion around the solved release point
    Montecarlo {
        #[command(flatten)]
        common: Common,
        /// Number of samples
        #[arg(short = 'n', long)]
        samples: Option<usize>,
        #[arg(long)]
        seed: Option<u64>,
    },
    /// Euler vs RK4 comparisons and convergence studies
    Numerics {
        #[command(flatten)]
        common: Common,
        /// Step size for the side-by-side comparison [s]
        #[arg(long = "compare-dt", default_value_t = 0.5)]
        compare_dt: f64,
    },
    /// Atmosphere and wind profile table
    Profile {
        #[command(flatten)]
        common: Common,
        #[arg(long, default_value_t = 11_000.0)]
        z_max: f64,
        #[arg(long, default_value_t = 23)]
        rows: usize,
        /// CSV instead of a text table
        #[arg(long)]
        csv: bool,
    },
}

pub fn load(c: &Common) -> Result<Scenario, String> {
    let mut s = match &c.scenario {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("invalid scenario: {e}"))?
        }
        None => Scenario::default(),
    };
    if let Some(m) = c.method {
        s.integrator.method = match m {
            MethodArg::Euler => Method::Euler,
            MethodArg::Rk4 => Method::Rk4,
        };
    }
    if let Some(dt) = c.dt {
        s.integrator.dt = dt;
    }
    Ok(s)
}

pub fn run(command: AirdropCommand) -> Result<(), String> {
    match command {
        AirdropCommand::Scenario => print_json(&Scenario::default()),
        AirdropCommand::Drop(c) => {
            let s = load(&c)?;
            let r = drop_report(&s)?;
            if c.json {
                return print_json(&r);
            }
            let rel = &r.release;
            println!("scenario         {}", s.name);
            println!(
                "integrator       {} dt = {} s",
                s.integrator.method.name(),
                s.integrator.dt
            );
            println!(
                "release point    E {:9.1} m  N {:9.1} m  at {:.0} m MSL",
                rel.release_point[0], rel.release_point[1], rel.release_altitude
            );
            println!(
                "release lead     {:.1} m before target ({:.2} s)",
                rel.release_distance_before_target, rel.release_lead_time
            );
            println!(
                "drift            along {:.1} m, cross {:.1} m",
                rel.along_track_travel, rel.cross_track_drift
            );
            println!(
                "time of fall     {:.2} s, impact {:.2} m/s ({:.2} m/s vertical)",
                rel.time_of_fall, rel.impact_speed, rel.impact_vertical_speed
            );
            println!(
                "miss             {:.4} m  ({} after {} iterations)",
                rel.miss_distance,
                if rel.converged {
                    "converged"
                } else {
                    "NOT converged"
                },
                rel.iterations
            );
            if let Some(on) = &r.on_track {
                println!(
                    "on-track timing  release {:.1} m before target, cross-track miss {:.1} m",
                    on.release_distance_before_target, on.cross_track_miss
                );
            }
            Ok(())
        }
        AirdropCommand::Montecarlo {
            common,
            samples,
            seed,
        } => {
            let mut s = load(&common)?;
            if let Some(n) = samples {
                s.monte_carlo.samples = n;
            }
            if let Some(seed) = seed {
                s.monte_carlo.seed = seed;
            }
            let r = monte_carlo_report(&s)?;
            if common.json {
                return print_json(&r);
            }
            let m = &r.result;
            println!(
                "samples          {} ({:.1} % landed) in {:.0} ms",
                s.monte_carlo.samples,
                100.0 * m.landed_fraction,
                r.compute_ms
            );
            println!(
                "mean impact      E {:.1} m  N {:.1} m  (bias {:.1} m)",
                m.mpi[0], m.mpi[1], m.bias
            );
            println!(
                "CEP              {:.1} m about target, {:.1} m about MPI",
                m.cep, m.cep_mpi
            );
            for e in &m.ellipses {
                println!(
                    "{:>3.0} % ellipse     {:.1} x {:.1} m, major axis {:.0} deg true, holds {:.1} % of samples",
                    100.0 * e.probability,
                    e.semi_major,
                    e.semi_minor,
                    (90.0 - e.angle.to_degrees()).rem_euclid(180.0),
                    100.0 * e.observed_fraction
                );
            }
            println!(
                "inside DZ        {:.1} % within {:.0} m of target",
                100.0 * r.inside_drop_zone,
                s.drop_zone_radius
            );
            Ok(())
        }
        AirdropCommand::Numerics { common, compare_dt } => {
            let s = load(&common)?;
            let r = numerics_report(&s, compare_dt)?;
            if common.json {
                return print_json(&r);
            }
            for c in &r.comparisons {
                println!("{} (dt = {} s)", c.title, c.dt);
                for p in &c.paths {
                    println!(
                        "  {:<10} range {:9.2} m   impact error {:10.3e} m   {} RHS evals",
                        p.label, p.impact_range, p.impact_error, p.evaluations
                    );
                }
            }
            for st in &r.studies {
                println!(
                    "\n{} (error at t = {} s vs {})",
                    st.title, st.t_end, st.reference
                );
                println!("  {:>9} {:>14} {:>14}", "dt [s]", "Euler [m]", "RK4 [m]");
                let (e, k) = (&st.series[0], &st.series[1]);
                for i in 0..e.dts.len() {
                    let fmt = |v: f64| {
                        if v.is_nan() {
                            "unstable".to_string()
                        } else {
                            format!("{v:.3e}")
                        }
                    };
                    println!(
                        "  {:>9.5} {:>14} {:>14}",
                        e.dts[i],
                        fmt(e.errors[i]),
                        fmt(k.errors[i])
                    );
                }
                println!(
                    "  observed order: Euler {:.2}, RK4 {:.2}",
                    e.observed_order, k.observed_order
                );
            }
            Ok(())
        }
        AirdropCommand::Profile {
            common,
            z_max,
            rows,
            csv,
        } => {
            let s = load(&common)?;
            let p = atmosphere_profile(&s, z_max, rows);
            if common.json {
                return print_json(&p);
            }
            if csv {
                println!(
                    "altitude_m,temperature_k,pressure_pa,density_kg_m3,wind_east_ms,wind_north_ms"
                );
                for r in &p {
                    println!(
                        "{},{},{},{},{},{}",
                        r.altitude, r.temperature, r.pressure, r.density, r.wind_east, r.wind_north
                    );
                }
            } else {
                println!(
                    "{:>9} {:>9} {:>11} {:>10} {:>8} {:>8}",
                    "z [m]", "T [K]", "p [Pa]", "rho", "wE m/s", "wN m/s"
                );
                for r in &p {
                    println!(
                        "{:>9.0} {:>9.2} {:>11.1} {:>10.5} {:>8.2} {:>8.2}",
                        r.altitude, r.temperature, r.pressure, r.density, r.wind_east, r.wind_north
                    );
                }
            }
            Ok(())
        }
    }
}
