import { useMemo } from "react";
import type uPlot from "uplot";
import { Segmented } from "../../components/Field";
import { UChart } from "../../components/UChart";
import { fmt } from "../../lib/format";
import type { Palette } from "../../lib/theme";
import type { ProfileRow, Scenario } from "../types";

export function AtmosphereView({
  scenario,
  rows,
  zMax,
  setZMax,
  palette,
}: {
  scenario: Scenario;
  rows: ProfileRow[] | null;
  zMax: number;
  setZMax: (z: number) => void;
  palette: Palette;
}) {
  const data = useMemo(() => {
    if (!rows) return null;
    const z = rows.map((r) => r.altitude / 1000);
    const col = (f: (r: ProfileRow) => number): uPlot.AlignedData => [z, rows.map(f)];
    return {
      density: col((r) => r.density),
      temperature: col((r) => r.temperature - 273.15),
      pressure: col((r) => r.pressure / 100),
      wind: [z, rows.map((r) => r.wind_east), rows.map((r) => r.wind_north)] as uPlot.AlignedData,
    };
  }, [rows]);

  if (!rows || !data) return <div className="empty">Computing profile…</div>;
  const sl = rows[0];
  const atm = scenario.environment.atmosphere;
  const releaseAlt = scenario.aircraft.height_agl;

  return (
    <div className="grid2">
      <div className="card span2">
        <div className="card-head">
          <h3>Atmosphere model</h3>
          <Segmented
            ariaLabel="Altitude range"
            value={String(zMax)}
            options={[
              { value: "2000", label: "0–2 km" },
              { value: "11000", label: "0–11 km" },
              { value: "32000", label: "0–32 km" },
            ]}
            onChange={(v) => setZMax(Number(v))}
          />
        </div>
        <p>
          {atm.type === "isa"
            ? `International Standard Atmosphere${atm.temperature_offset ? `, ISA${atm.temperature_offset > 0 ? "+" : ""}${atm.temperature_offset} K` : ""}: temperature falls 6.5 K per km to the tropopause at 11 km, pressure follows the hydrostatic equation and density the ideal gas law.`
            : atm.type === "constant"
              ? `Constant density ${atm.rho} kg/m³ at every altitude (temperature and pressure shown are ISA for reference).`
              : "Vacuum: no drag at all. Temperature and pressure shown are ISA for reference."}{" "}
          Sea-level density {fmt(sl.density, 4)} kg/m³. Drag scales with density, so a package dropped through thinner
          air falls faster and drifts less per metre of descent.
        </p>
      </div>
      <div className="card">
        <h3>Density</h3>
        <UChart data={data.density} palette={palette} xLabel="altitude [km]" yLabel="kg/m³" series={[{ label: "density", color: palette.blue }]} />
      </div>
      <div className="card">
        <h3>Temperature</h3>
        <UChart data={data.temperature} palette={palette} xLabel="altitude [km]" yLabel="°C" series={[{ label: "temperature", color: palette.orange }]} />
      </div>
      <div className="card">
        <h3>Pressure</h3>
        <UChart data={data.pressure} palette={palette} xLabel="altitude [km]" yLabel="hPa" series={[{ label: "pressure", color: palette.blue }]} />
      </div>
      <div className="card">
        <h3>Wind components</h3>
        <UChart
          data={data.wind}
          palette={palette}
          xLabel="altitude [km]"
          yLabel="m/s"
          series={[
            { label: "east", color: palette.blue },
            { label: "north", color: palette.orange },
          ]}
        />
        <p className="note">Release altitude in this scenario: {fmt(releaseAlt / 1000, 2)} km above the drop zone.</p>
      </div>
    </div>
  );
}
