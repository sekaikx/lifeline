//! Tauri shell: thin async commands over the planning engine.
//!
//! Heavy work runs on the blocking thread pool so the UI never stalls, and
//! every command returns `Result<_, String>` so validation errors reach the
//! UI. The generated world (terrain and roads) is cached: editing requests
//! or risk does not regenerate it.

use lifeline_airdrop::scenario::{
    self as drop, DropReport, MonteCarloReport, NumericsReport, ProfileRow,
};
use lifeline_planner::{Plan, PlanScenario, RiskLayers, World, WorldSpec};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct WorldCache(Mutex<Option<Arc<World>>>);

impl WorldCache {
    fn get(&self, spec: WorldSpec) -> Result<Arc<World>, String> {
        let mut slot = self.0.lock().map_err(|e| e.to_string())?;
        if let Some(w) = slot.as_ref().filter(|w| w.spec == spec) {
            return Ok(Arc::clone(w));
        }
        let w = Arc::new(World::generate(spec)?);
        *slot = Some(Arc::clone(&w));
        Ok(w)
    }
}

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

// ------------------------------------------------------------------ planning

#[tauri::command]
async fn example_plan(cache: State<'_, WorldCache>) -> Result<PlanScenario, String> {
    let world = cache.get(WorldSpec::default())?;
    Ok(PlanScenario::example(&world))
}

#[tauri::command]
async fn world(cache: State<'_, WorldCache>, spec: WorldSpec) -> Result<World, String> {
    Ok(World::clone(&*cache.get(spec)?))
}

#[tauri::command]
async fn plan(cache: State<'_, WorldCache>, scenario: PlanScenario) -> Result<Plan, String> {
    let world = cache.get(scenario.world)?;
    blocking(move || lifeline_planner::plan(&world, &scenario)).await
}

#[tauri::command]
async fn risk_layers(
    cache: State<'_, WorldCache>,
    scenario: PlanScenario,
) -> Result<RiskLayers, String> {
    let world = cache.get(scenario.world)?;
    blocking(move || Ok(lifeline_planner::risk_layers(&world, &scenario))).await
}

// ------------------------------------------------------------------ airdrop lab

#[tauri::command]
fn default_scenario() -> drop::Scenario {
    drop::Scenario::default()
}

#[tauri::command]
async fn drop_report(scenario: drop::Scenario) -> Result<DropReport, String> {
    blocking(move || drop::drop_report(&scenario)).await
}

#[tauri::command]
async fn monte_carlo_report(scenario: drop::Scenario) -> Result<MonteCarloReport, String> {
    blocking(move || drop::monte_carlo_report(&scenario)).await
}

#[tauri::command]
async fn numerics_report(scenario: drop::Scenario, dt: f64) -> Result<NumericsReport, String> {
    blocking(move || drop::numerics_report(&scenario, dt)).await
}

#[tauri::command]
fn atmosphere_profile(scenario: drop::Scenario, z_max: f64, rows: usize) -> Vec<ProfileRow> {
    drop::atmosphere_profile(&scenario, z_max, rows)
}

// ------------------------------------------------------------------ files

/// Ask the user for a JSON file and return its text, or `None` if cancelled.
/// The UI parses it, so one command serves both scenario kinds.
#[tauri::command]
async fn open_text(app: AppHandle) -> Result<Option<String>, String> {
    blocking(move || {
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
    blocking(move || {
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
        .manage(WorldCache::default())
        .invoke_handler(tauri::generate_handler![
            example_plan,
            world,
            plan,
            risk_layers,
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
