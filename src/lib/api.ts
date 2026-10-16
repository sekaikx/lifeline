import { invoke } from "@tauri-apps/api/core";
import type {
  AiSettings,
  AreaAssessment,
  CatalogueRef,
  Completion,
  Message,
  ModelInfo,
  ParsedRequest,
  PlaceRef,
  Preset,
} from "../ai/types";
import type { ChangeReport, Product, SceneImage, SceneInfo, SceneQuery, Sensor } from "../intel/types";
import type { Allocation, ImportReport, Item, Location, Snapshot, Tx, TxDraft } from "../supply/types";
import type {
  DropReport,
  MonteCarloReport,
  NumericsReport,
  ProfileRow,
  Scenario,
} from "../airdrop/types";
import type {
  Area,
  Imagery,
  Plan,
  PlanResult,
  PlanScenario,
  RiskLayers,
  WaterDetection,
  WaterDetector,
  WaterDetectors,
  WindForecast,
  WindProvider,
  World,
  WorldSpec,
} from "../plan/types";

/** True inside the desktop app (or a test harness that provides the IPC bridge). */
export const hasBackend = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api = {
  // planning
  examplePlan: (area: Area | null) => invoke<PlanScenario>("example_plan", { area }),
  world: (spec: WorldSpec) => invoke<World>("world", { spec }),
  plan: (scenario: PlanScenario) => invoke<Plan>("plan", { scenario }),
  riskLayers: (scenario: PlanScenario) => invoke<RiskLayers>("risk_layers", { scenario }),
  /** Plan and exposure fields together; the fields come back only when they differ from `haveLayers`. */
  planAll: (scenario: PlanScenario, haveLayers: string | null) =>
    invoke<PlanResult>("plan_all", { scenario, haveLayers }),

  // open data for real areas
  windForecast: (area: Area, provider: WindProvider, hours: number) =>
    invoke<WindForecast>("wind_forecast", { area, provider, hours }),
  imagery: (area: Area) => invoke<Imagery>("imagery", { area }),
  waterDetectors: () => invoke<WaterDetectors>("water_detectors"),
  detectFlooding: (area: Area, asOf: number | null, detector: WaterDetector) =>
    invoke<WaterDetection>("detect_flooding", { area, asOf, detector }),

  // satellite explorer
  listScenes: (area: Area, sensor: Sensor, from: number, to: number, maxCloud: number | null, limit = 80) =>
    invoke<SceneInfo[]>("list_scenes", { area, sensor, from, to, maxCloud, limit }),
  sceneImage: (area: Area, query: SceneQuery, product: Product) =>
    invoke<SceneImage>("scene_image", { area, query, product }),
  compareScenes: (area: Area, before: SceneQuery, after: SceneQuery) =>
    invoke<ChangeReport>("compare_scenes", { area, before, after }),

  // assistant
  aiPresets: () => invoke<Preset[]>("ai_presets"),
  aiSettings: () => invoke<AiSettings>("ai_settings"),
  aiSaveSettings: (settings: AiSettings) => invoke<AiSettings>("ai_save_settings", { settings }),
  aiModels: (provider: string) => invoke<ModelInfo[]>("ai_models", { provider }),
  aiChat: (context: string, messages: Message[]) => invoke<Completion>("ai_chat", { context, messages }),
  aiParseRequest: (text: string, places: PlaceRef[], catalogue: CatalogueRef[]) =>
    invoke<ParsedRequest>("ai_parse_request", { text, places, catalogue }),
  aiAssessArea: (png: string, bounds: [number, number, number, number], context: string) =>
    invoke<AreaAssessment>("ai_assess_area", { png, bounds, context }),

  // supply
  supplySnapshot: (today: string) => invoke<Snapshot>("supply_snapshot", { today }),
  supplyApply: (draft: TxDraft) => invoke<Tx>("supply_apply", { draft }),
  supplyUpsertItem: (item: Item) => invoke<null>("supply_upsert_item", { item }),
  supplyUpsertLocation: (location: Location) => invoke<null>("supply_upsert_location", { location }),
  supplyImport: (text: string, user: string, location: string) =>
    invoke<ImportReport>("supply_import", { text, user, location }),
  supplyExport: (kind: "stock" | "ledger") => invoke<string>("supply_export", { kind }),
  supplyLoadExample: (today: string) => invoke<null>("supply_load_example", { today }),
  supplyAllocate: (scenario: PlanScenario, plan: Plan, locations: string[], today: string) =>
    invoke<Allocation>("supply_allocate", { scenario, plan, locations, today }),

  // exports
  exportPlan: (scenario: PlanScenario, plan: Plan | null, format: "geojson" | "kml" | "cot") =>
    invoke<string | null>("export_plan", { scenario, plan, format }),

  // airdrop lab
  defaultScenario: () => invoke<Scenario>("default_scenario"),
  drop: (scenario: Scenario) => invoke<DropReport>("drop_report", { scenario }),
  monteCarlo: (scenario: Scenario) =>
    invoke<MonteCarloReport>("monte_carlo_report", { scenario }),
  numerics: (scenario: Scenario, dt: number) =>
    invoke<NumericsReport>("numerics_report", { scenario, dt }),
  profile: (scenario: Scenario, zMax: number, rows: number) =>
    invoke<ProfileRow[]>("atmosphere_profile", { scenario, zMax, rows }),

  // files
  openText: (kind: "scenario" | "csv" = "scenario") => invoke<string | null>("open_text", { kind }),
  saveText: (defaultName: string, extension: "json" | "csv" | "md" | "txt", contents: string) =>
    invoke<string | null>("save_text", { defaultName, extension, contents }),
};

export const errorText = (e: unknown): string =>
  typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
