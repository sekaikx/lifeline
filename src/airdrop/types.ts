// TypeScript mirrors of the serde types in crates/airdrop-core.
// Field names are snake_case because that is what serde emits.

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export type Method = "euler" | "rk4";

export interface Parachute {
  cd_area: number;
  deploy_delay: number;
  inflation_time: number;
  fill_exponent: number;
}

export interface Payload {
  mass: number;
  cd_area: number;
  parachute: Parachute | null;
}

export type Atmosphere =
  | { type: "isa"; temperature_offset: number }
  | { type: "constant"; rho: number }
  | { type: "vacuum" };

export interface WindLayer {
  altitude: number;
  speed: number;
  from_deg: number;
}

export type Wind =
  | { type: "none" }
  | { type: "uniform"; speed: number; from_deg: number }
  | {
      type: "power_law";
      speed_ref: number;
      from_deg: number;
      z_ref: number;
      alpha: number;
      ground_level: number;
    }
  | { type: "layered"; layers: WindLayer[] };

export type Terrain =
  | { type: "flat"; elevation: number }
  | {
      type: "hill";
      base: number;
      height: number;
      center_x: number;
      center_y: number;
      radius: number;
    }
  | {
      type: "grid";
      x0: number;
      y0: number;
      dx: number;
      dy: number;
      nx: number;
      ny: number;
      heights: number[];
    };

export interface Uncertainty {
  wind_sigma: number;
  release_time_sigma: number;
  nav_sigma: number;
  drag_sigma: number;
}

export interface Scenario {
  name: string;
  payload: Payload;
  environment: { atmosphere: Atmosphere; wind: Wind };
  terrain: Terrain;
  aircraft: { height_agl: number; ground_speed: number; track_deg: number };
  target: [number, number];
  drop_zone_radius: number;
  integrator: { method: Method; dt: number; t_max: number };
  monte_carlo: { samples: number; seed: number; uncertainty: Uncertainty };
}

export interface ReleaseSolution {
  solver: string;
  converged: boolean;
  iterations: number;
  release_point: [number, number];
  release_altitude: number;
  landing_point: Vec3;
  miss_distance: number;
  cross_track_miss: number;
  time_of_fall: number;
  impact_speed: number;
  impact_vertical_speed: number;
  along_track_travel: number;
  cross_track_drift: number;
  release_distance_before_target: number;
  release_lead_time: number;
}

export interface PathPoint {
  t: number;
  pos: Vec3;
  vel: Vec3;
  wind: Vec3;
  density: number;
  fill: number;
}

export interface Heightmap {
  x0: number;
  y0: number;
  dx: number;
  dy: number;
  nx: number;
  ny: number;
  heights: number[];
}

export interface DropReport {
  release: ReleaseSolution;
  on_track: ReleaseSolution | null;
  path: PathPoint[];
  chute_deploy_time: number | null;
  chute_full_time: number | null;
  wind_profile: { altitude: number; wind: Vec3 }[];
  heightmap: Heightmap;
  compute_ms: number;
}

export interface Ellipse {
  probability: number;
  center: [number, number];
  semi_major: number;
  semi_minor: number;
  /** radians counter-clockwise from east */
  angle: number;
  observed_fraction: number;
}

export interface MonteCarloReport {
  release: ReleaseSolution;
  result: {
    impacts: [number, number][];
    times_of_fall: number[];
    landed_fraction: number;
    mpi: [number, number];
    covariance: [[number, number], [number, number]];
    ellipses: Ellipse[];
    cep: number;
    cep_mpi: number;
    bias: number;
  };
  inside_drop_zone: number;
  compute_ms: number;
}

export interface MethodPath {
  label: string;
  points: [number, number][];
  impact_range: number;
  impact_error: number;
  evaluations: number;
}

export interface MethodSeries {
  method: Method;
  dts: number[];
  /** null where the run went unstable */
  errors: (number | null)[];
  evaluations: number[];
  unstable: boolean[];
  observed_order: number | null;
}

export interface ConvergenceStudy {
  id: string;
  title: string;
  reference: string;
  t_end: number;
  series: MethodSeries[];
}

export interface NumericsReport {
  comparisons: { id: string; title: string; dt: number; paths: MethodPath[] }[];
  studies: ConvergenceStudy[];
  compute_ms: number;
}

export interface ProfileRow {
  altitude: number;
  temperature: number;
  pressure: number;
  density: number;
  wind_east: number;
  wind_north: number;
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** Shape check for scenarios from files or storage; the engine validates values. */
export function isDropScenario(v: unknown): v is Scenario {
  if (!isObj(v) || !isObj(v.payload) || !isObj(v.environment) || !isObj(v.monte_carlo)) return false;
  return (
    isObj(v.environment.atmosphere) &&
    isObj(v.environment.wind) &&
    isObj(v.terrain) &&
    isObj(v.aircraft) &&
    Array.isArray(v.target) &&
    isObj(v.integrator) &&
    isObj(v.monte_carlo.uncertainty)
  );
}
