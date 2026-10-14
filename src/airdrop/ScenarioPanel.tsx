import type { Atmosphere, Scenario, Terrain, Wind } from "./types";
import { NumberField, NumberInput, Section, Segmented, SelectField } from "../components/Field";

type Update = (fn: (s: Scenario) => Scenario) => void;

const defaultWinds: Record<Wind["type"], Wind> = {
  none: { type: "none" },
  uniform: { type: "uniform", speed: 6, from_deg: 270 },
  power_law: { type: "power_law", speed_ref: 4, from_deg: 250, z_ref: 10, alpha: 1 / 7, ground_level: 0 },
  layered: {
    type: "layered",
    layers: [
      { altitude: 0, speed: 3, from_deg: 240 },
      { altitude: 150, speed: 5, from_deg: 250 },
      { altitude: 400, speed: 7, from_deg: 260 },
      { altitude: 1000, speed: 10, from_deg: 270 },
    ],
  },
};

const defaultAtmospheres: Record<Atmosphere["type"], Atmosphere> = {
  isa: { type: "isa", temperature_offset: 0 },
  constant: { type: "constant", rho: 1.225 },
  vacuum: { type: "vacuum" },
};

const defaultTerrains: Record<"flat" | "hill", Terrain> = {
  flat: { type: "flat", elevation: 0 },
  hill: { type: "hill", base: 0, height: 60, center_x: 150, center_y: 80, radius: 120 },
};

export function ScenarioPanel({ scenario: s, update }: { scenario: Scenario; update: Update }) {
  const chute = s.payload.parachute;
  const wind = s.environment.wind;
  const atm = s.environment.atmosphere;
  const terrain = s.terrain;

  const setWind = (w: Wind) => update((x) => ({ ...x, environment: { ...x.environment, wind: w } }));
  const setAtm = (a: Atmosphere) => update((x) => ({ ...x, environment: { ...x.environment, atmosphere: a } }));
  const setTerrain = (t: Terrain) => update((x) => ({ ...x, terrain: t }));

  return (
    <>
      <Section title="Payload">
        <NumberField
          label="Mass"
          unit="kg"
          min={0.001}
          step={10}
          value={s.payload.mass}
          onChange={(v) => update((x) => ({ ...x, payload: { ...x.payload, mass: v } }))}
        />
        <NumberField
          label="Drag area CdA"
          hint="bare package"
          unit="m²"
          min={0}
          step={0.1}
          value={s.payload.cd_area}
          onChange={(v) => update((x) => ({ ...x, payload: { ...x.payload, cd_area: v } }))}
        />
      </Section>

      <Section title="Parachute">
        <label className="toggle">
          <input
            type="checkbox"
            checked={chute !== null}
            onChange={(e) =>
              update((x) => ({
                ...x,
                payload: {
                  ...x.payload,
                  parachute: e.target.checked
                    ? { cd_area: 120, deploy_delay: 1.5, inflation_time: 3, fill_exponent: 2 }
                    : null,
                },
              }))
            }
          />
          Static-line parachute
        </label>
        {chute && (
          <>
            {(
              [
                ["Canopy CdA", "cd_area", "m²", 1, 0],
                ["Deploy delay", "deploy_delay", "s", 0.1, 0],
                ["Inflation time", "inflation_time", "s", 0.1, 0],
                ["Fill exponent", "fill_exponent", "", 0.1, 0.1],
              ] as const
            ).map(([label, key, unit, step, min]) => (
              <NumberField
                key={key}
                label={label}
                unit={unit}
                step={step}
                min={min}
                value={chute[key]}
                onChange={(v) =>
                  update((x) => ({
                    ...x,
                    payload: { ...x.payload, parachute: { ...x.payload.parachute!, [key]: v } },
                  }))
                }
              />
            ))}
          </>
        )}
      </Section>

      <Section title="Aircraft & target">
        <NumberField
          label="Release height"
          hint="above drop zone"
          unit="m AGL"
          min={1}
          step={10}
          value={s.aircraft.height_agl}
          onChange={(v) => update((x) => ({ ...x, aircraft: { ...x.aircraft, height_agl: v } }))}
        />
        <NumberField
          label="Ground speed"
          unit="m/s"
          min={0}
          step={1}
          value={s.aircraft.ground_speed}
          onChange={(v) => update((x) => ({ ...x, aircraft: { ...x.aircraft, ground_speed: v } }))}
        />
        <NumberField
          label="Track"
          unit="° true"
          min={0}
          max={360}
          step={5}
          value={s.aircraft.track_deg}
          onChange={(v) => update((x) => ({ ...x, aircraft: { ...x.aircraft, track_deg: v } }))}
        />
        <NumberField
          label="Target east"
          unit="m"
          step={10}
          value={s.target[0]}
          onChange={(v) => update((x) => ({ ...x, target: [v, x.target[1]] }))}
        />
        <NumberField
          label="Target north"
          unit="m"
          step={10}
          value={s.target[1]}
          onChange={(v) => update((x) => ({ ...x, target: [x.target[0], v] }))}
        />
        <NumberField
          label="Drop zone radius"
          unit="m"
          min={1}
          step={10}
          value={s.drop_zone_radius}
          onChange={(v) => update((x) => ({ ...x, drop_zone_radius: v }))}
        />
      </Section>

      <Section title="Wind">
        <SelectField
          label="Model"
          value={wind.type}
          options={[
            { value: "none", label: "Calm" },
            { value: "uniform", label: "Uniform" },
            { value: "power_law", label: "Shear (power law)" },
            { value: "layered", label: "Sounding (layers)" },
          ]}
          onChange={(t) => setWind(defaultWinds[t])}
        />
        {wind.type === "uniform" && (
          <>
            <NumberField label="Speed" unit="m/s" min={0} step={0.5} value={wind.speed} onChange={(v) => setWind({ ...wind, speed: v })} />
            <NumberField label="From" unit="° true" min={0} max={360} step={10} value={wind.from_deg} onChange={(v) => setWind({ ...wind, from_deg: v })} />
          </>
        )}
        {wind.type === "power_law" && (
          <>
            <NumberField label="Speed at ref. height" unit="m/s" min={0} step={0.5} value={wind.speed_ref} onChange={(v) => setWind({ ...wind, speed_ref: v })} />
            <NumberField label="Reference height" unit="m" min={0.1} step={1} value={wind.z_ref} onChange={(v) => setWind({ ...wind, z_ref: v })} />
            <NumberField label="Shear exponent α" hint="1/7 ≈ open terrain" min={0} max={1} step={0.01} value={wind.alpha} onChange={(v) => setWind({ ...wind, alpha: v })} />
            <NumberField label="From" unit="° true" min={0} max={360} step={10} value={wind.from_deg} onChange={(v) => setWind({ ...wind, from_deg: v })} />
          </>
        )}
        {wind.type === "layered" && <LayerTable wind={wind} setWind={setWind} />}
        <p className="note">Directions are where the wind blows from, as in a weather report.</p>
      </Section>

      <Section title="Atmosphere & terrain" open={false}>
        <SelectField
          label="Atmosphere"
          value={atm.type}
          options={[
            { value: "isa", label: "ISA" },
            { value: "constant", label: "Constant density" },
            { value: "vacuum", label: "Vacuum" },
          ]}
          onChange={(t) => setAtm(defaultAtmospheres[t])}
        />
        {atm.type === "isa" && (
          <NumberField label="Temperature offset" hint="ISA+15 = hot day" unit="K" step={1} value={atm.temperature_offset} onChange={(v) => setAtm({ ...atm, temperature_offset: v })} />
        )}
        {atm.type === "constant" && (
          <NumberField label="Density" unit="kg/m³" min={0} step={0.01} value={atm.rho} onChange={(v) => setAtm({ ...atm, rho: v })} />
        )}
        <SelectField
          label="Terrain"
          value={terrain.type === "hill" ? "hill" : "flat"}
          options={[
            { value: "flat", label: "Flat" },
            { value: "hill", label: "Hill" },
          ]}
          onChange={(t) => setTerrain(defaultTerrains[t])}
        />
        {terrain.type === "flat" && (
          <NumberField label="Elevation" unit="m MSL" step={10} value={terrain.elevation} onChange={(v) => setTerrain({ ...terrain, elevation: v })} />
        )}
        {terrain.type === "hill" && (
          <>
            <NumberField label="Base elevation" unit="m MSL" step={10} value={terrain.base} onChange={(v) => setTerrain({ ...terrain, base: v })} />
            <NumberField label="Hill height" unit="m" step={10} value={terrain.height} onChange={(v) => setTerrain({ ...terrain, height: v })} />
            <NumberField label="Hill east" unit="m" step={10} value={terrain.center_x} onChange={(v) => setTerrain({ ...terrain, center_x: v })} />
            <NumberField label="Hill north" unit="m" step={10} value={terrain.center_y} onChange={(v) => setTerrain({ ...terrain, center_y: v })} />
            <NumberField label="Hill radius" unit="m" min={1} step={10} value={terrain.radius} onChange={(v) => setTerrain({ ...terrain, radius: v })} />
          </>
        )}
      </Section>

      <Section title="Integrator">
        <div className="field">
          <label>Method</label>
          <Segmented
            ariaLabel="Integration method"
            value={s.integrator.method}
            options={[
              { value: "euler", label: "Euler" },
              { value: "rk4", label: "RK4" },
            ]}
            onChange={(m) => update((x) => ({ ...x, integrator: { ...x.integrator, method: m } }))}
          />
        </div>
        <NumberField
          label="Step size dt"
          unit="s"
          min={0.001}
          max={5}
          step={0.01}
          value={s.integrator.dt}
          onChange={(v) => update((x) => ({ ...x, integrator: { ...x.integrator, dt: v } }))}
        />
      </Section>

      <Section title="Monte Carlo" open={false}>
        <NumberField
          label="Samples"
          min={1}
          max={200000}
          integer
          step={500}
          value={s.monte_carlo.samples}
          onChange={(v) => update((x) => ({ ...x, monte_carlo: { ...x.monte_carlo, samples: v } }))}
        />
        <NumberField
          label="Seed"
          min={0}
          integer
          step={1}
          value={s.monte_carlo.seed}
          onChange={(v) => update((x) => ({ ...x, monte_carlo: { ...x.monte_carlo, seed: v } }))}
        />
        {(
          [
            ["Wind error σ", "wind_sigma", "m/s", 0.1, "per component"],
            ["Release timing σ", "release_time_sigma", "s", 0.1, "green-light error"],
            ["Navigation σ", "nav_sigma", "m", 1, "position at release"],
            ["Drag area σ", "drag_sigma", "×", 0.01, "fraction, 0.05 = 5 %"],
          ] as const
        ).map(([label, key, unit, step, hint]) => (
          <NumberField
            key={key}
            label={label}
            hint={hint}
            unit={unit}
            min={0}
            step={step}
            value={s.monte_carlo.uncertainty[key]}
            onChange={(v) =>
              update((x) => ({
                ...x,
                monte_carlo: { ...x.monte_carlo, uncertainty: { ...x.monte_carlo.uncertainty, [key]: v } },
              }))
            }
          />
        ))}
      </Section>
    </>
  );
}

export function LayerTable({
  wind,
  setWind,
}: {
  wind: Extract<Wind, { type: "layered" }>;
  setWind: (w: Wind) => void;
}) {
  const layers = wind.layers;
  const set = (i: number, key: "altitude" | "speed" | "from_deg", v: number) =>
    setWind({ ...wind, layers: layers.map((l, j) => (j === i ? { ...l, [key]: v } : l)) });
  const sorted = layers.every((l, i) => i === 0 || l.altitude > layers[i - 1].altitude);
  return (
    <>
      <table className="layers">
        <thead>
          <tr>
            <th>Alt m</th>
            <th>m/s</th>
            <th>From °</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {layers.map((l, i) => (
            <tr key={i}>
              <td>
                <NumberInput ariaLabel={`Layer ${i + 1} altitude`} step={50} value={l.altitude} onChange={(v) => set(i, "altitude", v)} />
              </td>
              <td>
                <NumberInput ariaLabel={`Layer ${i + 1} speed`} min={0} step={0.5} value={l.speed} onChange={(v) => set(i, "speed", v)} />
              </td>
              <td>
                <NumberInput ariaLabel={`Layer ${i + 1} direction`} min={0} max={360} step={10} value={l.from_deg} onChange={(v) => set(i, "from_deg", v)} />
              </td>
              <td>
                <button
                  className="row-btn"
                  aria-label={`Remove layer ${i + 1}`}
                  disabled={layers.length <= 1}
                  onClick={() => setWind({ ...wind, layers: layers.filter((_, j) => j !== i) })}
                >
                  ✕
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {!sorted && <p className="note" style={{ color: "var(--critical)" }}>Altitudes must increase down the table.</p>}
      <button
        className="btn"
        onClick={() => {
          const top = layers[layers.length - 1];
          setWind({ ...wind, layers: [...layers, { ...top, altitude: top.altitude + 200 }] });
        }}
      >
        + Add layer
      </button>
    </>
  );
}
