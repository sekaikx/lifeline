import { useCallback, useEffect, useRef, useState } from "react";
import { Segmented } from "../components/Field";
import { api, errorText } from "../lib/api";
import type { Palette, ThemeMode } from "../lib/theme";
import { MapView, modeColor, type BaseLayer, type Placing, type RiskLayer } from "./MapView";
import { PlanPanel } from "./PlanPanel";
import { floodSummary, SetupPanel, type DataActions } from "./SetupPanel";
import { Timeline } from "./Timeline";
import type { SatelliteChoice, SceneImage } from "../intel/types";
import type { Item } from "../supply/types";
import { isPlanScenario, type Area, type Plan, type PlanScenario, type Request, type RiskLayers, type World } from "./types";

/** What the rest of the app needs to know about the plan. */
export interface PlanState {
  scenario: PlanScenario;
  plan: Plan | null;
  world: World;
  area: Area | null;
}

/** Supply data the plan shows next to requests. */
export interface SupplyView {
  catalogue: Item[];
  available: Record<string, number>;
  shortfalls: Record<string, string[]>;
}

const STORAGE_KEY = "lifeline.plan.v2";

export const areaOf = (s: PlanScenario | null): Area | null =>
  s && s.world.type === "real"
    ? { name: s.world.name, lat: s.world.lat, lon: s.world.lon, size_m: s.world.size_m, cell_m: s.world.cell_m }
    : null;

function loadSaved(): PlanScenario | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const s: unknown = raw ? JSON.parse(raw) : null;
    return isPlanScenario(s) ? s : null;
  } catch {
    return null;
  }
}

function nextId(prefix: string, ids: string[]): string {
  const used = new Set(ids);
  for (let i = 1; ; i++) if (!used.has(`${prefix}${i}`)) return `${prefix}${i}`;
}

export interface WorkspaceHandle {
  scenario: PlanScenario | null;
  setScenario: (s: PlanScenario) => void;
  reset: () => void;
  /** Add a request (a fresh id is assigned) and select it */
  addRequest: (r: Omit<Request, "id">) => string;
  update: (fn: (s: PlanScenario) => PlanScenario) => void;
}

export function PlanWorkspace({
  palette,
  theme,
  onStatus,
  handle,
  satellite,
  supply,
  onState,
}: {
  palette: Palette;
  theme: ThemeMode;
  onStatus: (s: string) => void;
  handle: React.MutableRefObject<WorkspaceHandle | null>;
  satellite: SatelliteChoice;
  supply: SupplyView;
  onState: (s: PlanState | null) => void;
}) {
  const [scenario, setScenario] = useState<PlanScenario | null>(loadSaved);
  const [world, setWorld] = useState<World | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [layers, setLayers] = useState<RiskLayers | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [riskLayer, setRiskLayer] = useState<RiskLayer>("ground");
  const [placing, setPlacing] = useState<Placing>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [baseLayer, setBaseLayer] = useState<BaseLayer>("relief");
  const [imagery, setImagery] = useState<{ key: string; data: SceneImage } | null>(null);
  const [flood, setFlood] = useState<string | null>(null);
  const [waterModel, setWaterModel] = useState<string | null>(null);
  const [worldAttempt, setWorldAttempt] = useState(0);

  useEffect(() => {
    api
      .waterDetectors()
      .then((d) => setWaterModel(d.model))
      .catch(() => setWaterModel(null));
  }, []);

  const loadArea = useCallback((area: Area | null) => {
    setBusy(area ? `Loading ${area.name}…` : "Generating terrain…");
    setError(null);
    onStatus(area ? `Downloading elevation and OpenStreetMap data for ${area.name}` : "");
    api
      .examplePlan(area)
      .then((s) => {
        setScenario(s);
        setSelected(null);
        setFlood(null);
        if (!area) setBaseLayer("relief");
      })
      .catch((e) => setError(errorText(e)))
      .finally(() => setBusy(null));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const reset = useCallback(() => loadArea(areaOf(scenario)), [loadArea, scenario]);

  useEffect(() => {
    // stop after a failure instead of retrying in a loop; the error offers a retry
    if (!scenario && !busy && !error) loadArea(null);
  }, [scenario, busy, error, loadArea]);

  const update = useCallback((fn: (s: PlanScenario) => PlanScenario) => setScenario((s) => (s ? fn(s) : s)), []);
  handle.current = {
    scenario,
    setScenario: (s) => {
      setScenario(s);
      setSelected(null);
    },
    reset,
    update,
    addRequest: (r) => {
      const id = nextId("R", scenario?.requests.map((q) => q.id) ?? []);
      update((s) => ({ ...s, requests: [...s.requests, { ...r, id }] }));
      setSelected(id);
      return id;
    },
  };

  // keep the last scenario across restarts; written when edits pause, since
  // serialising a large scenario on every drag step is what made edits stutter
  useEffect(() => {
    if (!scenario) return;
    const timer = window.setTimeout(() => {
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(scenario));
      } catch {
        /* storage unavailable or full */
      }
    }, 800);
    return () => window.clearTimeout(timer);
  }, [scenario]);

  // the world only changes when its spec does
  const specKey = scenario ? JSON.stringify(scenario.world) : "";
  useEffect(() => {
    if (!scenario) return;
    let live = true;
    api
      .world(scenario.world)
      .then((w) => live && setWorld(w))
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [specKey, worldAttempt]);

  // satellite base: the scene and product chosen in Area intel (default: the
  // clearest recent Sentinel-2 mosaic), fetched when the layer is shown
  const area = areaOf(scenario);
  const areaKey = area ? JSON.stringify([area, satellite]) : "";
  useEffect(() => {
    if (baseLayer !== "satellite" || !area || imagery?.key === areaKey) return;
    let live = true;
    onStatus("Fetching satellite imagery…");
    api
      .sceneImage(area, satellite.query, satellite.product)
      .then((data) => {
        if (!live) return;
        setImagery({ key: areaKey, data });
        onStatus(`${data.scenes[0]?.sensor === "landsat" ? "Landsat" : data.scenes[0]?.sensor === "sentinel1" ? "Sentinel-1" : "Sentinel-2"} imagery from ${[...new Set(data.scenes.map((x) => x.date))].join(", ")}`);
      })
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseLayer, areaKey]);

  // replan shortly after edits stop
  // Scenario objects are replaced on every edit, so identity is the change
  // signal; no need to serialise the whole scenario to compare it.
  const generation = useRef(0);
  const layersKey = useRef<string | null>(null);
  useEffect(() => {
    if (!scenario) return;
    const id = ++generation.current;
    const timer = setTimeout(async () => {
      try {
        const r = await api.planAll(scenario, layersKey.current);
        if (id !== generation.current) return;
        setPlan(r.plan);
        if (r.layers) {
          setLayers(r.layers);
          layersKey.current = r.layers_key;
        }
        setError(null);
        onStatus(`Planned in ${Math.round(r.plan.compute_ms)} ms`);
      } catch (e) {
        if (id === generation.current) setError(errorText(e));
      }
    }, 120);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scenario]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPlacing(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // tell the app (assistant, supply, area intel) what is being planned
  useEffect(() => {
    onState(scenario && world ? { scenario, plan, world, area: areaOf(scenario) } : null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scenario, plan, world]);

  if (!scenario || !world) {
    return (
      <div className="empty">
        <div>
          <p>{error ?? busy ?? "Loading the area…"}</p>
          {!error && area && <p className="note">Elevation and OpenStreetMap data download once and are cached.</p>}
          {error && !busy && (
            <p className="empty-actions">
              {scenario && (
                <button
                  className="btn"
                  onClick={() => {
                    setError(null);
                    setWorldAttempt((n) => n + 1);
                  }}
                >
                  Try again
                </button>
              )}
              <button className="btn" onClick={() => loadArea(null)}>
                Start over on generated terrain
              </button>
            </p>
          )}
        </div>
      </div>
    );
  }

  const actions: DataActions = {
    busy,
    loadArea,
    floodSummary: flood,
    waterModel,
    forecast: (provider, hours) => {
      if (!area) return;
      setBusy("Fetching forecast…");
      api
        .windForecast(area, provider, hours)
        .then((f) =>
          update((x) => ({
            ...x,
            weather: { ...x.weather, wind: f.wind, source: `${f.source}, valid ${new Date(f.valid_unix * 1000).toISOString().slice(0, 16).replace("T", " ")} UTC` },
          })),
        )
        .catch((e) => setError(errorText(e)))
        .finally(() => setBusy(null));
    },
    detectFlooding: (date, detector) => {
      if (!area) return;
      setBusy("Analysing Sentinel-2…");
      const asOf = date ? Math.round(Date.parse(`${date}T12:00:00Z`) / 1000) : null;
      api
        .detectFlooding(area, asOf, detector)
        .then((d) => {
          const dates = [...new Set(d.scenes.map((x) => x.date))].join(", ");
          setFlood(floodSummary(d.new_water_fraction, d.obscured_fraction, d.scenes, d.detector));
          update((x) => ({
            ...x,
            risk: {
              ...x.risk,
              masks: [
                ...x.risk.masks.filter((m) => m.id !== "W1"),
                ...(d.mask.cells.length
                  ? [{ id: "W1", name: "New surface water", mask: d.mask, rate_per_min: 0, impassable: true, source: `Sentinel-2 ${dates}, ${d.detector === "model" ? "U-Net" : "MNDWI"}` }]
                  : []),
              ],
            },
          }));
        })
        .catch((e) => setError(errorText(e)))
        .finally(() => setBusy(null));
    },
  };

  const place = (x: number, y: number) => {
    if (placing === "request") {
      const id = nextId("R", scenario.requests.map((r) => r.id));
      const r: Request = { id, name: "New request", x, y, demand_kg: 200, priority: "high", due_h: 6, mode: "auto" };
      update((s) => ({ ...s, requests: [...s.requests, r] }));
      setSelected(id);
    } else if (placing === "site") {
      const id = nextId("S", scenario.risk.sites.map((q) => q.id));
      update((s) => ({
        ...s,
        risk: { ...s.risk, sites: [...s.risk.sites, { id, name: `Observed area ${id.slice(1)}`, x, y, height: 8, range: 3000, rate_per_min: 0.02 }] },
      }));
    } else if (placing === "hazard") {
      const id = nextId("H", scenario.risk.hazards.map((q) => q.id));
      const d = 500;
      update((s) => ({
        ...s,
        risk: {
          ...s.risk,
          hazards: [
            ...s.risk.hazards,
            { id, name: `Hazard ${id.slice(1)}`, polygon: [[x - d, y - d], [x + d, y - d], [x + d, y + d], [x - d, y + d]], rate_per_min: 0.2, impassable: false },
          ],
        },
      }));
    }
    setPlacing(null);
  };

  const move = (kind: "request" | "depot" | "site", id: string, x: number, y: number) => {
    if (kind === "depot") update((s) => ({ ...s, depot: { ...s.depot, x, y } }));
    else if (kind === "request") update((s) => ({ ...s, requests: s.requests.map((r) => (r.id === id ? { ...r, x, y } : r)) }));
    else update((s) => ({ ...s, risk: { ...s.risk, sites: s.risk.sites.map((q) => (q.id === id ? { ...q, x, y } : q)) } }));
  };

  return (
    <div className="plan-layout">
      <SetupPanel scenario={scenario} update={update} placing={placing} setPlacing={setPlacing} actions={actions} />
      <div className="plan-center">
        {error && (
          <div className="banner" role="alert">
            <strong>Cannot plan</strong>
            <span>{error}</span>
          </div>
        )}
        <div className="map-area">
          <MapView
            world={world}
            scenario={scenario}
            plan={plan}
            layers={layers}
            palette={palette}
            theme={theme}
            selected={selected}
            onSelect={setSelected}
            riskLayer={riskLayer}
            placing={placing}
            onPlace={place}
            onMove={move}
            keyHeight={area ? 146 : 78}
            baseLayer={area ? baseLayer : "relief"}
            imagery={imagery?.key === areaKey ? imagery.data : null}
          />
          <div className="map-key">
            {area && (
              <div className="map-key-row">
                <span className="map-key-title">Base</span>
                <Segmented
                  ariaLabel="Base map"
                  value={baseLayer}
                  options={[
                    { value: "relief", label: "Relief" },
                    { value: "satellite", label: "Satellite" },
                  ]}
                  onChange={setBaseLayer}
                />
              </div>
            )}
            <div className="map-key-row">
              <span className="map-key-title">Exposure</span>
              <Segmented
                ariaLabel="Exposure layer"
                value={riskLayer}
                options={[
                  { value: "off", label: "Off" },
                  { value: "ground", label: "Ground" },
                  { value: "drone", label: "Drone" },
                  { value: "air", label: "Aircraft" },
                ]}
                onChange={setRiskLayer}
              />
            </div>
            {area && (
              <div className="map-key-row">
                <span className="map-key-title">Export</span>
                <div className="segmented" role="group" aria-label="Export plan">
                  {(
                    [
                      ["geojson", "GeoJSON"],
                      ["kml", "KML"],
                      ["cot", "ATAK"],
                    ] as const
                  ).map(([f, label]) => (
                    <button
                      key={f}
                      type="button"
                      title={f === "cot" ? "Cursor-on-Target events for ATAK, WinTAK and iTAK" : f === "kml" ? "Google Earth, ATAK, QGIS" : "QGIS, ArcGIS, web maps"}
                      onClick={() =>
                        api
                          .exportPlan(scenario, plan, f)
                          .then((path) => path && onStatus(`Exported ${path}`))
                          .catch((e) => setError(errorText(e)))
                      }
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <div className="legend">
              <span>
                <span className="line" style={{ borderColor: modeColor(palette, "truck") }} />
                Truck
              </span>
              <span>
                <span className="line dashed" style={{ borderColor: modeColor(palette, "drone") }} />
                Drone
              </span>
              <span>
                <span className="line dotted" style={{ borderColor: modeColor(palette, "airdrop") }} />
                Aircraft
              </span>
              <span>
                <span className="swatch" style={{ background: palette.critical, opacity: 0.45 }} />
                In view of a risk
              </span>
            </div>
          </div>
        </div>
        {plan && (
          <div className="timeline-area">
            <div className="block-head">
              <h2>Fleet timeline</h2>
              <span className="note">hours from now · solid outbound, faint return</span>
            </div>
            <Timeline plan={plan} scenario={scenario} palette={palette} selected={selected} onSelect={setSelected} />
          </div>
        )}
      </div>
      <PlanPanel
        scenario={scenario}
        plan={plan}
        palette={palette}
        selected={selected}
        onSelect={setSelected}
        adding={placing === "request"}
        onAddRequest={() => setPlacing(placing === "request" ? null : "request")}
        catalogue={supply.catalogue}
        available={supply.available}
        shortfalls={supply.shortfalls}
        onChangeRequest={(id, patch) => update((s) => ({ ...s, requests: s.requests.map((r) => (r.id === id ? { ...r, ...patch } : r)) }))}
        onDeleteRequest={(id) => {
          update((s) => ({ ...s, requests: s.requests.filter((r) => r.id !== id) }));
          setSelected(null);
        }}
      />
    </div>
  );
}
