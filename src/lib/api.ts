import { invoke } from "@tauri-apps/api/core";
import type {
  DropReport,
  MonteCarloReport,
  NumericsReport,
  ProfileRow,
  Scenario,
} from "../airdrop/types";
import type { Plan, PlanScenario, RiskLayers, World, WorldSpec } from "../plan/types";

/** True inside the desktop app (or a test harness that provides the IPC bridge). */
export const hasBackend = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api = {
  // planning
  examplePlan: () => invoke<PlanScenario>("example_plan"),
  world: (spec: WorldSpec) => invoke<World>("world", { spec }),
  plan: (scenario: PlanScenario) => invoke<Plan>("plan", { scenario }),
  riskLayers: (scenario: PlanScenario) => invoke<RiskLayers>("risk_layers", { scenario }),

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
  openText: () => invoke<string | null>("open_text"),
  saveText: (defaultName: string, extension: "json" | "csv", contents: string) =>
    invoke<string | null>("save_text", { defaultName, extension, contents }),
};

export const errorText = (e: unknown): string =>
  typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
