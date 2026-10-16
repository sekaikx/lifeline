// Mirrors of crates/geodata/src/explore.rs.

export type Sensor = "sentinel2" | "landsat" | "sentinel1";
export type Product =
  | "true_color"
  | "false_color"
  | "agriculture"
  | "vegetation"
  | "moisture"
  | "water"
  | "burn"
  | "radar";

export interface SceneInfo {
  id: string;
  sensor: Sensor;
  date: string;
  datetime: string;
  /** whole-scene cloud cover [%]; null for radar */
  cloud: number | null;
  platform: string;
  orbit: string;
}

export interface SceneQuery {
  sensor: Sensor;
  /** one scene; null for a clear-sky mosaic up to as_of */
  scene: string | null;
  /** Unix seconds; null for now */
  as_of: number | null;
  window_days: number;
  max_cloud: number;
}

export interface LegendStop {
  value: number;
  color: string;
  label: string | null;
}

export interface SceneStats {
  clear_fraction: number;
  cloud_fraction: number;
  snow_fraction: number;
  water_fraction: number;
  dense_vegetation_fraction: number;
  moderate_vegetation_fraction: number;
  sparse_vegetation_fraction: number;
  bare_fraction: number;
  mean_ndvi: number | null;
  mean_ndmi: number | null;
  mean_vv_db: number | null;
}

export interface SceneImage {
  png_base64: string;
  width: number;
  height: number;
  bounds: [number, number, number, number];
  sensor: Sensor;
  product: Product;
  scenes: SceneInfo[];
  stats: SceneStats;
  legend: LegendStop[];
  attribution: string;
}

export interface ChangeStats {
  compared_fraction: number;
  vegetation_loss_fraction: number;
  vegetation_gain_fraction: number;
  new_water_fraction: number;
  receded_water_fraction: number;
  burn_fraction: number;
}

export interface ChangeReport {
  png_base64: string;
  width: number;
  height: number;
  bounds: [number, number, number, number];
  before: SceneInfo[];
  after: SceneInfo[];
  stats: ChangeStats;
  legend: LegendStop[];
  method: string;
  attribution: string;
}

export const SENSORS: { value: Sensor; label: string; since: number; note: string }[] = [
  { value: "sentinel2", label: "Sentinel-2", since: 2017, note: "10 m optical, every 5 days" },
  { value: "landsat", label: "Landsat", since: 1982, note: "30 m optical, back to 1982" },
  { value: "sentinel1", label: "Radar", since: 2014, note: "Sentinel-1, sees through cloud and at night" },
];

export const PRODUCTS: { value: Product; label: string; radar: boolean | null; help: string }[] = [
  { value: "true_color", label: "True colour", radar: false, help: "As the eye sees it" },
  { value: "false_color", label: "Infrared", radar: false, help: "Healthy vegetation bright red; water dark" },
  { value: "agriculture", label: "Agriculture", radar: false, help: "Crops green, bare soil pink, wet ground dark" },
  { value: "vegetation", label: "Vegetation", radar: false, help: "NDVI: plant vigour, brown to deep green" },
  { value: "moisture", label: "Moisture", radar: false, help: "NDMI: dry brown to wet blue" },
  { value: "water", label: "Water", radar: null, help: "Open water in blue over a grey base" },
  { value: "burn", label: "Burn", radar: false, help: "NBR: burn scars dark red" },
  { value: "radar", label: "Radar", radar: true, help: "VV, VH, ratio: water black, towns bright" },
];

export const productFits = (p: Product, s: Sensor) => {
  const r = PRODUCTS.find((x) => x.value === p)?.radar;
  return r === null || r === (s === "sentinel1");
};

/** What the plan map uses as its satellite base. */
export interface SatelliteChoice {
  query: SceneQuery;
  product: Product;
}

export const defaultSatellite: SatelliteChoice = {
  query: { sensor: "sentinel2", scene: null, as_of: null, window_days: 60, max_cloud: 40 },
  product: "true_color",
};
