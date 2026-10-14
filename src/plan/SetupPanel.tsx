import { LayerTable } from "../airdrop/ScenarioPanel";
import type { Wind } from "../airdrop/types";
import { NumberField, NumberInput, Section, SelectField } from "../components/Field";
import type { Placing } from "./MapView";
import type { PlanScenario } from "./types";

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

export function SetupPanel({
  scenario: s,
  update,
  placing,
  setPlacing,
}: {
  scenario: PlanScenario;
  update: Update;
  placing: Placing;
  setPlacing: (p: Placing) => void;
}) {
  const f = s.fleet;
  const setTrucks = (patch: Partial<typeof f.trucks>) => update((x) => ({ ...x, fleet: { ...x.fleet, trucks: { ...x.fleet.trucks, ...patch } } }));
  const setTruck = (patch: Partial<typeof f.trucks.truck>) => setTrucks({ truck: { ...f.trucks.truck, ...patch } });
  const setDrones = (patch: Partial<typeof f.drones>) => update((x) => ({ ...x, fleet: { ...x.fleet, drones: { ...x.fleet.drones, ...patch } } }));
  const setDrone = (patch: Partial<typeof f.drones.drone>) => setDrones({ drone: { ...f.drones.drone, ...patch } });
  const setAir = (patch: Partial<typeof f.aircraft>) => update((x) => ({ ...x, fleet: { ...x.fleet, aircraft: { ...x.fleet.aircraft, ...patch } } }));
  const wind = s.weather.wind;

  return (
    <aside className="setup">
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
        <p className="note">
          An observed area only counts where it has line of sight, so ridges hide routes. Rates are expected losses per
          minute in view.
        </p>
      </Section>

      <Section title="Wind" open={false}>
        <SelectField
          label="Model"
          value={wind.type}
          options={[
            { value: "none", label: "Calm" },
            { value: "uniform", label: "Uniform" },
            { value: "layered", label: "Sounding (layers)" },
          ]}
          onChange={(t) => update((x) => ({ ...x, weather: { ...x.weather, wind: windPresets[t] } }))}
        />
        {wind.type === "uniform" && (
          <>
            <NumberField label="Speed" unit="m/s" min={0} step={1} value={wind.speed} onChange={(v) => update((x) => ({ ...x, weather: { ...x.weather, wind: { ...wind, speed: v } } }))} />
            <NumberField label="From" unit="°" min={0} max={360} step={10} value={wind.from_deg} onChange={(v) => update((x) => ({ ...x, weather: { ...x.weather, wind: { ...wind, from_deg: v } } }))} />
          </>
        )}
        {wind.type === "layered" && <LayerTable wind={wind} setWind={(w) => update((x) => ({ ...x, weather: { ...x.weather, wind: w } }))} />}
        <p className="note">Altitudes are above sea level. Wind sets drone ground speed and airdrop drift.</p>
      </Section>

      <Section title="Terrain" open={false}>
        <NumberField label="Seed" min={0} integer step={1} value={s.world.seed} onChange={(v) => update((x) => ({ ...x, world: { ...x.world, seed: v } }))} />
        <NumberField label="Relief" unit="m" min={0} step={50} value={s.world.relief_m} onChange={(v) => update((x) => ({ ...x, world: { ...x.world, relief_m: v } }))} />
        <NumberField label="Towns" min={2} max={24} integer step={1} value={s.world.towns} onChange={(v) => update((x) => ({ ...x, world: { ...x.world, towns: v } }))} />
        <p className="note">Terrain and roads are generated from these values. Requests and risk keep their positions.</p>
      </Section>
    </aside>
  );
}
