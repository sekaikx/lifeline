//! Tauri shell: thin async commands over the planning engine.
//!
//! Heavy work runs on the blocking thread pool so the UI never stalls, and
//! every command returns `Result<_, String>` so errors reach the UI. The
//! loaded world (terrain, roads, places) is cached in memory, and all
//! downloaded open data is cached on disk, so an area loads once.

use lifeline_airdrop::scenario::{
    self as drop, DropReport, MonteCarloReport, NumericsReport, ProfileRow,
};
use lifeline_geodata::{Area, Fetcher, Imagery, WaterDetection, WindForecast, WindProvider};
use lifeline_planner::{Plan, PlanScenario, RiskLayers, World, WorldSpec};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

struct Data {
    world: Mutex<Option<Arc<World>>>,
    fetcher: Fetcher,
}

impl Data {
    fn new() -> Self {
        let fetcher = Fetcher::new(Fetcher::default_cache_dir())
            .or_else(|_| Fetcher::new(None))
            .expect("HTTP client");
        Self {
            world: Mutex::new(None),
            fetcher,
        }
    }

    fn world(&self, spec: &WorldSpec) -> Result<Arc<World>, String> {
        let mut slot = self.world.lock().map_err(|e| e.to_string())?;
        if let Some(w) = slot.as_ref().filter(|w| &w.spec == spec) {
            return Ok(Arc::clone(w));
        }
        let w = Arc::new(World::build(spec, Some(&self.fetcher))?);
        *slot = Some(Arc::clone(&w));
        Ok(w)
    }
}

type Shared<'a> = State<'a, Arc<Data>>;

/// Run `f` with the shared data on the blocking pool.
async fn blocking<T, F>(data: &Shared<'_>, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Data) -> Result<T, String> + Send + 'static,
{
    let data = Arc::clone(data);
    tauri::async_runtime::spawn_blocking(move || f(&data))
        .await
        .map_err(|e| e.to_string())?
}

async fn compute<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

// ------------------------------------------------------------------ planning

/// A worked example on generated terrain, or on a real area (which also
/// pulls the current wind forecast).
#[tauri::command]
async fn example_plan(data: Shared<'_>, area: Option<Area>) -> Result<PlanScenario, String> {
    blocking(&data, move |d| {
        let spec = area
            .clone()
            .map_or_else(WorldSpec::default, WorldSpec::Real);
        let world = d.world(&spec)?;
        let mut s = PlanScenario::example(&world);
        if let Some(a) = &area {
            if let Ok(f) =
                lifeline_geodata::wind_profile(&d.fetcher, a, 0.0, WindProvider::MetNorway)
            {
                s.weather.wind = f.wind;
                s.weather.source = Some(format!(
                    "{}, valid {} UTC",
                    f.source,
                    lifeline_geodata::iso_date(f.valid_unix)
                ));
            }
        }
        Ok(s)
    })
    .await
}

#[tauri::command]
async fn world(data: Shared<'_>, spec: WorldSpec) -> Result<World, String> {
    blocking(&data, move |d| Ok(World::clone(&*d.world(&spec)?))).await
}

#[tauri::command]
async fn plan(data: Shared<'_>, scenario: PlanScenario) -> Result<Plan, String> {
    blocking(&data, move |d| {
        lifeline_planner::plan(&*d.world(&scenario.world)?, &scenario)
    })
    .await
}

#[tauri::command]
async fn risk_layers(data: Shared<'_>, scenario: PlanScenario) -> Result<RiskLayers, String> {
    blocking(&data, move |d| {
        Ok(lifeline_planner::risk_layers(
            &*d.world(&scenario.world)?,
            &scenario,
        ))
    })
    .await
}

// ------------------------------------------------------------------ open data

#[tauri::command]
async fn wind_forecast(
    data: Shared<'_>,
    area: Area,
    provider: WindProvider,
    hours: f64,
) -> Result<WindForecast, String> {
    blocking(&data, move |d| {
        lifeline_geodata::wind_profile(&d.fetcher, &area, hours, provider)
    })
    .await
}

#[tauri::command]
async fn imagery(data: Shared<'_>, area: Area) -> Result<Imagery, String> {
    blocking(&data, move |d| {
        lifeline_geodata::imagery(&d.fetcher, &area, 1024)
    })
    .await
}

/// New surface water from Sentinel-2. `as_of` (Unix seconds) reviews a past date.
#[tauri::command]
async fn detect_flooding(
    data: Shared<'_>,
    area: Area,
    as_of: Option<i64>,
) -> Result<WaterDetection, String> {
    blocking(&data, move |d| {
        let world = d.world(&WorldSpec::Real(area.clone()))?;
        lifeline_geodata::detect_new_water(&d.fetcher, &area, &world.terrain, as_of)
    })
    .await
}

// ------------------------------------------------------------------ airdrop lab

#[tauri::command]
fn default_scenario() -> drop::Scenario {
    drop::Scenario::default()
}

#[tauri::command]
async fn drop_report(scenario: drop::Scenario) -> Result<DropReport, String> {
    compute(move || drop::drop_report(&scenario)).await
}

#[tauri::command]
async fn monte_carlo_report(scenario: drop::Scenario) -> Result<MonteCarloReport, String> {
    compute(move || drop::monte_carlo_report(&scenario)).await
}

#[tauri::command]
async fn numerics_report(scenario: drop::Scenario, dt: f64) -> Result<NumericsReport, String> {
    compute(move || drop::numerics_report(&scenario, dt)).await
}

#[tauri::command]
fn atmosphere_profile(scenario: drop::Scenario, z_max: f64, rows: usize) -> Vec<ProfileRow> {
    drop::atmosphere_profile(&scenario, z_max, rows)
}

// ------------------------------------------------------------------ files

/// Ask the user for a JSON file and return its text, or `None` if cancelled.
#[tauri::command]
async fn open_text(app: AppHandle) -> Result<Option<String>, String> {
    compute(move || {
        let Some(path) = app
            .dialog()
            .file()
            .add_filter("Scenario", &["json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        std::fs::read_to_string(&path)
            .map(Some)
            .map_err(|e| e.to_string())
    })
    .await
}

/// Save text the UI produced (scenario JSON or CSV) to a user-chosen file.
#[tauri::command]
async fn save_text(
    app: AppHandle,
    default_name: String,
    extension: String,
    contents: String,
) -> Result<Option<String>, String> {
    if !matches!(extension.as_str(), "json" | "csv") {
        return Err("only .json and .csv exports are supported".into());
    }
    compute(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(&default_name)
            .add_filter(extension.to_uppercase(), &[extension.as_str()])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, contents).map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Data::new()))
        .invoke_handler(tauri::generate_handler![
            example_plan,
            world,
            plan,
            risk_layers,
            wind_forecast,
            imagery,
            detect_flooding,
            default_scenario,
            drop_report,
            monte_carlo_report,
            numerics_report,
            atmosphere_profile,
            open_text,
            save_text,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lifeline");
}
