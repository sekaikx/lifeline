// TypeScript mirrors of the serde types in crates/planner, crates/ground,
// crates/drone and crates/core. Field names are snake_case as serde emits them.

import type { Atmosphere, Payload, Uncertainty, Wind } from "../airdrop/types";

export type Point = [number, number];

export interface GeneratedSpec {
  seed: number;
  size_m: number;
  cells: number;
  base_m: number;
  relief_m: number;
  towns: number;
}

/** A square area on Earth, centred on lat/lon. */
export interface Area {
  name: string;
  lat: number;
  lon: number;
  size_m: number;
  cell_m: number;
}

export type WorldSpec = ({ type: "generated" } & GeneratedSpec) | ({ type: "real" } & Area);

export interface Place {
  name: string;
  kind: string;
  x: number;
  y: number;
  population: number | null;
}

export interface TerrainGrid {
  x0: number;
  y0: number;
  dx: number;
  dy: number;
  nx: number;
  ny: number;
  heights: number[];
}

export type RoadClass = "paved" | "gravel" | "track";

export interface World {
  spec: WorldSpec;
  terrain: TerrainGrid;
  network: {
    towns: { id: number; name: string; x: number; y: number }[];
    roads: { a: number; b: number; class: RoadClass; points: Point[] }[];
  };
  places: Place[];
  attribution: string[];
}

export interface Raster {
  x0: number;
  y0: number;
  cell: number;
  nx: number;
  ny: number;
  values: number[];
}

export interface RiskLayers {
  ground: Raster;
  drone: Raster;
  air: Raster;
}

export type Mode = "truck" | "drone" | "airdrop";
export type ModeChoice = "auto" | Mode;
export type Priority = "critical" | "high" | "routine";

export interface Request {
  id: string;
  name: string;
  x: number;
  y: number;
  demand_kg: number;
  priority: Priority;
  due_h: number;
  mode: ModeChoice;
}

export interface RiskSite {
  id: string;
  name: string;
  x: number;
  y: number;
  height: number;
  range: number;
  rate_per_min: number;
}

export interface HazardArea {
  id: string;
  name: string;
  polygon: Point[];
  rate_per_min: number;
  impassable: boolean;
}

export interface CellMask {
  x0: number;
  y0: number;
  cell: number;
  nx: number;
  ny: number;
  cells: number[];
}

export interface HazardMask {
  id: string;
  name: string;
  mask: CellMask;
  rate_per_min: number;
  impassable: boolean;
  source: string;
}

export interface Scene {
  id: string;
  date: string;
  cloud: number;
}

export interface WindForecast {
  wind: Wind;
  valid_unix: number;
  elevation: number;
  source: string;
}

export type WindProvider = "met-norway" | "open-meteo";

export interface Imagery {
  png_base64: string;
  width: number;
  height: number;
  /** west, south, east, north in local metres */
  bounds: [number, number, number, number];
  scenes: Scene[];
  attribution: string;
}

export type WaterDetector = "index" | "model";

export interface WaterDetection {
  mask: CellMask;
  scenes: Scene[];
  obscured_fraction: number;
  water_fraction: number;
  new_water_fraction: number;
  detector: WaterDetector;
  method: string;
}

export interface WaterDetectors {
  model: string | null;
  expected_path: string | null;
}

export interface Truck {
  capacity_kg: number;
  speed_paved_kmh: number;
  speed_gravel_kmh: number;
  speed_track_kmh: number;
  speed_offroad_kmh: number;
  handling_min: number;
  detectability: number;
}

export interface DroneSpec {
  empty_mass_kg: number;
  payload_kg: number;
  battery_wh: number;
  reserve: number;
  rotor_area_m2: number;
  cd_area_m2: number;
  airspeed_ms: number;
  efficiency: number;
  cruise_agl_m: number;
  detectability: number;
  turnaround_min: number;
}

export interface AircraftFleet {
  count: number;
  capacity_kg: number;
  bundle: Payload;
  cruise_ms: number;
  drop_speed_ms: number;
  drop_height_agl: number;
  rigging_min: number;
  turnaround_min: number;
  detectability: number;
  dz_radius_m: number;
  uncertainty: Uncertainty;
  samples: number;
  cost_per_hour: number;
}

export interface PlanScenario {
  name: string;
  world: WorldSpec;
  depot: { name: string; x: number; y: number; airstrip: boolean };
  requests: Request[];
  fleet: {
    trucks: { count: number; truck: Truck; cost_per_hour: number };
    drones: { count: number; drone: DroneSpec; cost_per_hour: number };
    aircraft: AircraftFleet;
  };
  risk: { sites: RiskSite[]; hazards: HazardArea[]; masks: HazardMask[] };
  weather: { atmosphere: Atmosphere; wind: Wind; source: string | null };
  settings: { min_delivery_prob: number; seed: number };
}

export type OptionDetail =
  | {
      type: "truck";
      offroad_km: number;
      survival: number;
      alternatives: { time_h: number; distance_km: number; survival: number }[];
    }
  | {
      type: "drone";
      energy_wh: number;
      usable_wh: number;
      comms_fraction: number;
      survival: number;
      survival_round_trip: number;
    }
  | {
      type: "airdrop";
      release_point: Point;
      bundles_per_sortie: number;
      dz_probability: number;
      survival: number;
      cep_m: number;
      time_of_fall_s: number;
    }
  | { type: "none" };

export interface ModeOption {
  mode: Mode;
  feasible: boolean;
  reason: string | null;
  trips: number;
  load_per_trip_kg: number;
  one_way_h: number;
  cycle_h: number;
  delivery_prob: number;
  cost_per_trip: number;
  route: Point[];
  distance_km: number;
  detail: OptionDetail;
}

export interface Assignment {
  request_id: string;
  mode: Mode | null;
  reason: string | null;
  trips: number;
  first_arrival_h: number;
  completion_h: number;
  on_time: boolean;
  expected_kg: number;
  delivery_prob: number;
  cost: number;
  within_tolerance: boolean;
  options: ModeOption[];
}

export interface Trip {
  request_id: string;
  depart_h: number;
  arrive_h: number;
  ready_h: number;
  load_kg: number;
}

export interface Plan {
  assignments: Assignment[];
  schedule: { unit: string; mode: Mode; trips: Trip[] }[];
  summary: {
    demand_kg: number;
    expected_kg: number;
    expected_on_time_kg: number;
    critical_total: number;
    critical_on_time: number;
    makespan_h: number;
    cost: number;
    unserved: number;
    below_tolerance: number;
  };
  compute_ms: number;
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/**
 * Shape check for scenarios from files or storage: enough that the UI can
 * render them. Values are validated by the engine, which reports errors.
 */
export function isPlanScenario(v: unknown): v is PlanScenario {
  if (!isObj(v) || !isObj(v.world) || !isObj(v.depot) || !Array.isArray(v.requests)) return false;
  if (v.world.type !== "generated" && v.world.type !== "real") return false;
  const { fleet, risk, weather } = v;
  // files saved before flood masks existed have none; the engine defaults them too
  if (isObj(risk) && risk.masks === undefined) risk.masks = [];
  return (
    isObj(fleet) &&
    isObj(fleet.trucks) &&
    isObj(fleet.trucks.truck) &&
    isObj(fleet.drones) &&
    isObj(fleet.drones.drone) &&
    isObj(fleet.aircraft) &&
    isObj(fleet.aircraft.bundle) &&
    isObj(risk) &&
    Array.isArray(risk.sites) &&
    Array.isArray(risk.hazards) &&
    Array.isArray(risk.masks) &&
    isObj(weather) &&
    isObj(weather.wind) &&
    isObj(v.settings)
  );
}
