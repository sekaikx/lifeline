import { LayerTable } from "../airdrop/ScenarioPanel";
import type { Wind } from "../airdrop/types";
import { NumberField, NumberInput, Section, SelectField } from "../components/Field";
import { useEffect, useState } from "react";
import { fmtPct } from "../lib/format";
import type { Placing } from "./MapView";
import type { Area, PlanScenario, WaterDetector, WindProvider } from "./types";

type Update = (fn: (s: PlanScenario) => PlanScenario) => void;

const windPresets: Record<Wind["type"], Wind> = {
  none: { type: "none" },
  uniform: { type: "uniform", speed: 6, from_deg: 250 },
  power_law: { type: "power_law", speed_ref: 4, from_deg: 250, z_ref: 10, alpha: 1 / 7, ground_level: 0 },
  layered: {
    type: "layered",
    layers: [
      { altitude: 0, speed: 4, from_deg: 240 },
      { altitude: 500, speed: 7, from_deg: 255 },
      { altitude: 1500, speed: 11, from_deg: 270 },
    ],
  },
};

const PRESETS: { id: string; label: string; area: Area | null }[] = [
  { id: "generated", label: "Generated valley (synthetic)", area: null },
  { id: "gorkha", label: "Gorkha, Nepal", area: { name: "Gorkha", lat: 28.0, lon: 84.63, size_m: 30_000, cell_m: 100 } },
  { id: "dadu", label: "Dadu, Pakistan", area: { name: "Dadu", lat: 26.75, lon: 67.75, size_m: 40_000, cell_m: 100 } },
  { id: "kahramanmaras", label: "Kahramanmaraş, Türkiye", area: { name: "Kahramanmaraş", lat: 37.58, lon: 36.93, size_m: 30_000, cell_m: 100 } },
  { id: "custom", label: "Custom location", area: null },
];

export interface DataActions {
  busy: string | null;
  loadArea: (area: Area | null) => void;
  forecast: (provider: WindProvider, hours: number) => void;
  detectFlooding: (date: string | null, detector: WaterDetector) => void;
  floodSummary: string | null;
  /** path of the installed learned water model, if any */
  waterModel: string | null;
  /** Truck access lost after flood / impassable masks; derived from the plan */
  accessNote: string | null;
}

export function SetupPanel({
  scenario: s,
  update,
  placing,
  setPlacing,
  actions,
}: {
  scenario: PlanScenario;
  update: Update;
  placing: Placing;
  setPlacing: (p: Placing) => void;
  actions: DataActions;
}) {
  const real = s.world.type === "real" ? s.world : null;
  const f = s.fleet;
  const setTrucks = (patch: Partial<typeof f.trucks>) => update((x) => ({ ...x, fleet: { ...x.fleet, trucks: { ...x.fleet.trucks, ...patch } } }));
  const setTruck = (patch: Partial<typeof f.trucks.truck>) => setTrucks({ truck: { ...f.trucks.truck, ...patch } });
  const setDrones = (patch: Partial<typeof f.drones>) => update((x) => ({ ...x, fleet: { ...x.fleet, drones: { ...x.fleet.drones, ...patch } } }));
  const setDrone = (patch: Partial<typeof f.drones.drone>) => setDrones({ drone: { ...f.drones.drone, ...patch } });
  const setAir = (patch: Partial<typeof f.aircraft>) => update((x) => ({ ...x, fleet: { ...x.fleet, aircraft: { ...x.fleet.aircraft, ...patch } } }));
  const wind = s.weather.wind;

  return (
    <aside className="setup">
      <AreaSection scenario={s} actions={actions} />

      <Section title="Depot">
        <label className="field">
          <span>Name</span>
          <input className="text-input" value={s.depot.name} onChange={(e) => update((x) => ({ ...x, depot: { ...x.depot, name: e.target.value } }))} />
        </label>
        <label className="toggle">
          <input type="checkbox" checked={s.depot.airstrip} onChange={(e) => update((x) => ({ ...x, depot: { ...x.depot, airstrip: e.target.checked } }))} />
          Airstrip (aircraft can operate)
        </label>
        <p className="note">Drag the black square on the map to move the depot.</p>
      </Section>

      <Section title="Trucks">
        <NumberField label="Vehicles" min={0} max={50} integer step={1} value={f.trucks.count} onChange={(v) => setTrucks({ count: v })} />
        <NumberField label="Payload" unit="kg" min={1} step={250} value={f.trucks.truck.capacity_kg} onChange={(v) => setTruck({ capacity_kg: v })} />
        <NumberField label="Paved road" unit="km/h" min={1} step={5} value={f.trucks.truck.speed_paved_kmh} onChange={(v) => setTruck({ speed_paved_kmh: v })} />
        <NumberField label="Gravel road" unit="km/h" min={1} step={5} value={f.trucks.truck.speed_gravel_kmh} onChange={(v) => setTruck({ speed_gravel_kmh: v })} />
        <NumberField label="Off-road" unit="km/h" min={1} step={1} value={f.trucks.truck.speed_offroad_kmh} onChange={(v) => setTruck({ speed_offroad_kmh: v })} />
        <NumberField label="Loading" hint="at each end" unit="min" min={0} step={5} value={f.trucks.truck.handling_min} onChange={(v) => setTruck({ handling_min: v })} />
        <NumberField label="Cost" unit="/ h" min={0} step={10} value={f.trucks.cost_per_hour} onChange={(v) => setTrucks({ cost_per_hour: v })} />
      </Section>

      <Section title="Drones">
        <NumberField label="Drones" min={0} max={100} integer step={1} value={f.drones.count} onChange={(v) => setDrones({ count: v })} />
        <NumberField label="Payload" unit="kg" min={0.1} step={1} value={f.drones.drone.payload_kg} onChange={(v) => setDrone({ payload_kg: v })} />
        <NumberField label="Battery" unit="Wh" min={1} step={100} value={f.drones.drone.battery_wh} onChange={(v) => setDrone({ battery_wh: v })} />
        <NumberField label="Airspeed" unit="m/s" min={1} step={1} value={f.drones.drone.airspeed_ms} onChange={(v) => setDrone({ airspeed_ms: v })} />
        <NumberField label="Cruise height" unit="m AGL" min={1} step={10} value={f.drones.drone.cruise_agl_m} onChange={(v) => setDrone({ cruise_agl_m: v })} />
        <NumberField label="Cost" unit="/ h" min={0} step={10} value={f.drones.cost_per_hour} onChange={(v) => setDrones({ cost_per_hour: v })} />
      </Section>

      <Section title="Aircraft" open={false}>
        <NumberField label="Aircraft" min={0} max={20} integer step={1} value={f.aircraft.count} onChange={(v) => setAir({ count: v })} />
        <NumberField label="Payload" unit="kg" min={1} step={500} value={f.aircraft.capacity_kg} onChange={(v) => setAir({ capacity_kg: v })} />
        <NumberField label="Bundle mass" unit="kg" min={1} step={50} value={f.aircraft.bundle.mass} onChange={(v) => setAir({ bundle: { ...f.aircraft.bundle, mass: v } })} />
        <NumberField label="Cruise speed" unit="m/s" min={1} step={5} value={f.aircraft.cruise_ms} onChange={(v) => setAir({ cruise_ms: v })} />
        <NumberField label="Drop height" unit="m AGL" min={50} step={50} value={f.aircraft.drop_height_agl} onChange={(v) => setAir({ drop_height_agl: v })} />
        <NumberField label="Drop zone radius" unit="m" min={10} step={25} value={f.aircraft.dz_radius_m} onChange={(v) => setAir({ dz_radius_m: v })} />
        <NumberField label="Rigging" hint="before each sortie" unit="min" min={0} step={5} value={f.aircraft.rigging_min} onChange={(v) => setAir({ rigging_min: v })} />
        <NumberField label="Cost" unit="/ h" min={0} step={100} value={f.aircraft.cost_per_hour} onChange={(v) => setAir({ cost_per_hour: v })} />
        <p className="note">Drop accuracy comes from the airdrop physics engine; tune it in the Airdrop lab.</p>
      </Section>

      <Section title="Risk">
        <NumberField
          label="Tolerance"
          hint="minimum chance a trip arrives"
          unit="%"
          min={0}
          max={100}
          step={1}
          value={Math.round(s.settings.min_delivery_prob * 1000) / 10}
          onChange={(v) => update((x) => ({ ...x, settings: { ...x.settings, min_delivery_prob: v / 100 } }))}
        />
        <div className="list-head">
          <span>Observed areas</span>
          <button className="link-btn" aria-pressed={placing === "site"} onClick={() => setPlacing(placing === "site" ? null : "site")}>
            {placing === "site" ? "Click the map…" : "Add"}
          </button>
        </div>
        {s.risk.sites.map((site, i) => (
          <div key={site.id} className="list-item">
            <input
              className="text-input"
              value={site.name}
              aria-label="Name"
              onChange={(e) => update((x) => ({ ...x, risk: { ...x.risk, sites: x.risk.sites.map((q, j) => (j === i ? { ...q, name: e.target.value } : q)) } }))}
            />
            <div className="pair">
              <NumberInput
                ariaLabel="Range"
                unit="m"
                min={100}
                step={250}
                value={site.range}
                onChange={(v) => update((x) => ({ ...x, risk: { ...x.risk, sites: x.risk.sites.map((q, j) => (j === i ? { ...q, range: v } : q)) } }))}
              />
              <NumberInput
                ariaLabel="Losses per minute in sight"
                unit="/min"
                min={0}
                step={0.005}
                value={site.rate_per_min}
                onChange={(v) => update((x) => ({ ...x, risk: { ...x.risk, sites: x.risk.sites.map((q, j) => (j === i ? { ...q, rate_per_min: v } : q)) } }))}
              />
              <button className="row-btn" aria-label={`Remove ${site.name}`} onClick={() => update((x) => ({ ...x, risk: { ...x.risk, sites: x.risk.sites.filter((_, j) => j !== i) } }))}>
                ✕
              </button>
            </div>
          </div>
        ))}
        <div className="list-head">
          <span>Hazard areas</span>
          <button className="link-btn" aria-pressed={placing === "hazard"} onClick={() => setPlacing(placing === "hazard" ? null : "hazard")}>
            {placing === "hazard" ? "Click the map…" : "Add"}
          </button>
        </div>
        {s.risk.hazards.map((h, i) => (
          <div key={h.id} className="list-item">
            <input
              className="text-input"
              value={h.name}
              aria-label="Name"
              onChange={(e) => update((x) => ({ ...x, risk: { ...x.risk, hazards: x.risk.hazards.map((q, j) => (j === i ? { ...q, name: e.target.value } : q)) } }))}
            />
            <div className="pair">
              <label className="toggle small">
                <input
                  type="checkbox"
                  checked={h.impassable}
                  onChange={(e) => update((x) => ({ ...x, risk: { ...x.risk, hazards: x.risk.hazards.map((q, j) => (j === i ? { ...q, impassable: e.target.checked } : q)) } }))}
                />
                Impassable
              </label>
              <NumberInput
                ariaLabel="Losses per minute inside"
                unit="/min"
                min={0}
                step={0.05}
                value={h.rate_per_min}
                onChange={(v) => update((x) => ({ ...x, risk: { ...x.risk, hazards: x.risk.hazards.map((q, j) => (j === i ? { ...q, rate_per_min: v } : q)) } }))}
              />
              <button className="row-btn" aria-label={`Remove ${h.name}`} onClick={() => update((x) => ({ ...x, risk: { ...x.risk, hazards: x.risk.hazards.filter((_, j) => j !== i) } }))}>
                ✕
              </button>
            </div>
          </div>
        ))}
        <FloodControls real={!!real} actions={actions} />
        {s.risk.masks.map((m, i) => (
          <div key={m.id} className="list-item">
            <div className="pair">
              <span>{m.name}</span>
              <label className="toggle small">
                <input
                  type="checkbox"
                  checked={m.impassable}
                  onChange={(e) => update((x) => ({ ...x, risk: { ...x.risk, masks: x.risk.masks.map((q, j) => (j === i ? { ...q, impassable: e.target.checked } : q)) } }))}
                />
                Impassable
              </label>
              <button className="row-btn" aria-label={`Remove ${m.name}`} onClick={() => update((x) => ({ ...x, risk: { ...x.risk, masks: x.risk.masks.filter((_, j) => j !== i) } }))}>
                ✕
              </button>
            </div>
            <span className="note">
              {m.mask.cells.length.toLocaleString("en-US")} cells · {m.source}
            </span>
          </div>
        ))}
        <p className="note">
          An observed area only counts where it has line of sight, so ridges hide routes. Rates are expected losses per
          minute in view.
        </p>
      </Section>

      <Section title="Wind" open={false}>
        <WindForecastControls real={!!real} actions={actions} source={s.weather.source} />
        <SelectField
          label="Model"
          value={wind.type}
          options={[
            { value: "none", label: "Calm" },
            { value: "uniform", label: "Uniform" },
            { value: "layered", label: "Sounding (layers)" },
          ]}
          onChange={(t) => update((x) => ({ ...x, weather: { ...x.weather, wind: windPresets[t], source: "Entered by hand" } }))}
        />
        {wind.type === "uniform" && (
          <>
            <NumberField label="Speed" unit="m/s" min={0} step={1} value={wind.speed} onChange={(v) => update((x) => ({ ...x, weather: { ...x.weather, wind: { ...wind, speed: v }, source: "Entered by hand" } }))} />
            <NumberField label="From" unit="°" min={0} max={360} step={10} value={wind.from_deg} onChange={(v) => update((x) => ({ ...x, weather: { ...x.weather, wind: { ...wind, from_deg: v }, source: "Entered by hand" } }))} />
          </>
        )}
        {wind.type === "layered" && <LayerTable wind={wind} setWind={(w) => update((x) => ({ ...x, weather: { ...x.weather, wind: w, source: "Entered by hand" } }))} />}
        <p className="note">Altitudes are above sea level. Wind sets drone ground speed and airdrop drift.</p>
      </Section>

      {s.world.type === "generated" && (
        <Section title="Generated terrain" open={false}>
          <NumberField label="Seed" min={0} integer step={1} value={s.world.seed} onChange={(v) => update((x) => (x.world.type === "generated" ? { ...x, world: { ...x.world, seed: v } } : x))} />
          <NumberField label="Relief" unit="m" min={0} step={50} value={s.world.relief_m} onChange={(v) => update((x) => (x.world.type === "generated" ? { ...x, world: { ...x.world, relief_m: v } } : x))} />
          <NumberField label="Towns" min={2} max={24} integer step={1} value={s.world.towns} onChange={(v) => update((x) => (x.world.type === "generated" ? { ...x, world: { ...x.world, towns: v } } : x))} />
          <p className="note">Synthetic terrain for training and testing. Requests and risk keep their positions.</p>
        </Section>
      )}
    </aside>
  );
}

function AreaSection({ scenario: s, actions }: { scenario: PlanScenario; actions: DataActions }) {
  const current = s.world.type === "real" ? s.world : null;
  const [preset, setPreset] = useState(() => (current ? PRESETS.find((p) => p.area?.name === current.name)?.id ?? "custom" : "generated"));
  const [draft, setDraft] = useState<Area>(current ?? PRESETS[1].area!);
  useEffect(() => {
    if (current) setDraft(current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current?.name, current?.lat, current?.lon]);
  return (
    <Section title="Area">
      <p className="area-now">
        {current ? (
          <>
            <strong>{current.name}</strong>
            <span className="num muted">
              {" "}
              {current.lat.toFixed(3)}°, {current.lon.toFixed(3)}° · {current.size_m / 1000} km square
            </span>
          </>
        ) : (
          <>
            <strong>Generated valley</strong>
            <span className="muted"> · synthetic terrain</span>
          </>
        )}
      </p>
      <SelectField
        label="Load"
        value={preset}
        options={PRESETS.map((p) => ({ value: p.id, label: p.label }))}
        onChange={(id) => {
          setPreset(id);
          const a = PRESETS.find((p) => p.id === id)?.area;
          if (a) setDraft(a);
        }}
      />
      {preset === "custom" && (
        <>
          <label className="field">
            <span>Name</span>
            <input className="text-input" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
          </label>
          <NumberField label="Latitude" unit="°" min={-80} max={80} step={0.01} value={draft.lat} onChange={(v) => setDraft({ ...draft, lat: v })} />
          <NumberField label="Longitude" unit="°" min={-180} max={180} step={0.01} value={draft.lon} onChange={(v) => setDraft({ ...draft, lon: v })} />
        </>
      )}
      {preset !== "generated" && (
        <>
          <NumberField label="Size" unit="km" min={2} max={80} step={5} value={draft.size_m / 1000} onChange={(v) => setDraft({ ...draft, size_m: v * 1000 })} />
          <NumberField label="Grid" unit="m" min={30} max={500} step={10} value={draft.cell_m} onChange={(v) => setDraft({ ...draft, cell_m: v })} />
        </>
      )}
      <button className="btn" disabled={!!actions.busy} onClick={() => actions.loadArea(preset === "generated" ? null : draft)}>
        {actions.busy ?? "Load area and example plan"}
      </button>
      <p className="note">
        Real areas use open data: elevation (SRTM), roads and settlements (OpenStreetMap) and the current forecast. The
        first load of an area can take a minute; it is cached afterwards.
      </p>
    </Section>
  );
}

function WindForecastControls({ real, actions, source }: { real: boolean; actions: DataActions; source: string | null }) {
  const [provider, setProvider] = useState<WindProvider>("met-norway");
  const [hours, setHours] = useState(0);
  return (
    <>
      <p className="note" style={{ marginTop: 0 }}>
        Now using: {source ?? "manual profile"}
      </p>
      {real && (
        <>
          <SelectField<WindProvider>
            label="Forecast"
            value={provider}
            options={[
              { value: "met-norway", label: "MET Norway" },
              { value: "open-meteo", label: "Open-Meteo" },
            ]}
            onChange={setProvider}
          />
          <NumberField label="Hours ahead" unit="h" min={0} max={48} step={1} value={hours} onChange={setHours} />
          <button className="btn" disabled={!!actions.busy} onClick={() => actions.forecast(provider, hours)}>
            Use forecast
          </button>
          <p className="note">
            {provider === "met-norway"
              ? "Surface wind, extended aloft with a power law. Free for any use with attribution."
              : "Full vertical profile. Free for non-commercial use; commercial use needs an API key."}
          </p>
        </>
      )}
    </>
  );
}

function FloodControls({ real, actions }: { real: boolean; actions: DataActions }) {
  const [date, setDate] = useState("");
  const [detector, setDetector] = useState<WaterDetector>("index");
  if (!real) return null;
  const used = actions.waterModel ? detector : "index";
  return (
    <div className="flood">
      <div className="list-head">
        <span>Satellite flood check</span>
      </div>
      <label className="field">
        <span>
          As of<span className="hint">empty for the latest images</span>
        </span>
        <input className="text-input" type="date" value={date} onChange={(e) => setDate(e.target.value)} />
      </label>
      {actions.waterModel && (
        <SelectField
          label="Detector"
          value={used}
          options={[
            { value: "index", label: "Water index" },
            { value: "model", label: "Trained U-Net" },
          ]}
          onChange={setDetector}
        />
      )}
      <button className="btn" disabled={!!actions.busy} onClick={() => actions.detectFlooding(date || null, used)}>
        Detect new water
      </button>
      {actions.floodSummary && <p className="note">{actions.floodSummary}</p>}
      {actions.accessNote && <p className="note access-note">{actions.accessNote}</p>}
    </div>
  );
}

export const floodSummary = (
  newFrac: number,
  obscured: number,
  scenes: { date: string }[],
  detector: WaterDetector = "index",
) =>
  `New water on ${fmtPct(newFrac, 1)} of the area; ${fmtPct(obscured)} could not be assessed (cloud, shadow, snow). Sentinel-2 ${[...new Set(scenes.map((x) => x.date))].join(", ")}, ${detector === "model" ? "trained U-Net" : "water index"}.`;
