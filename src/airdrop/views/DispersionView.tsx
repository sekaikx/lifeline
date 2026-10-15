import { useMemo } from "react";
import type uPlot from "uplot";
import { DropMap } from "../DropMap";
import { UChart } from "../../components/UChart";
import { bearing, fmt, fmtInt } from "../../lib/format";
import type { Palette } from "../../lib/theme";
import type { DropReport, MonteCarloReport, Scenario } from "../types";

export function DispersionView({
  scenario,
  report,
  mc,
  busy,
  palette,
  onRun,
  onExport,
}: {
  scenario: Scenario;
  report: DropReport;
  mc: MonteCarloReport | null;
  busy: boolean;
  palette: Palette;
  onRun: () => void;
  onExport: () => void;
}) {
  const hist = useMemo<uPlot.AlignedData | null>(() => {
    if (!mc) return null;
    const [tx, ty] = scenario.target;
    const miss = mc.result.impacts.map(([e, n]) => Math.hypot(e - tx, n - ty));
    // a loop, not Math.max(...miss): spreading 10^5 samples overflows the call stack
    const max = miss.reduce((a, b) => Math.max(a, b), 1);
    const bins = 30;
    const w = max / bins;
    const counts = new Array(bins).fill(0);
    for (const m of miss) counts[Math.min(bins - 1, Math.floor(m / w))]++;
    return [counts.map((_, i) => (i + 0.5) * w), counts.map((c) => (100 * c) / miss.length)];
  }, [mc, scenario.target]);

  const m = mc?.result;
  const u = scenario.monte_carlo.uncertainty;

  return (
    <div className="split">
      {mc ? (
        <DropMap
          scenario={scenario}
          report={report}
          mc={mc}
          mode="dispersion"
          palette={palette}
          legend={
            <>
              <strong style={{ color: "var(--text)" }}>Landing dispersion</strong>
              <span className="legend">
                <span>
                  <span className="swatch" style={{ background: palette.blue }} />
                  Impacts ({fmtInt(m!.impacts.length)})
                </span>
                <span>
                  <span className="line" style={{ borderColor: palette.orange }} />
                  50 % ellipse
                </span>
                <span>
                  <span className="line dashed" style={{ borderColor: palette.orange }} />
                  95 % ellipse
                </span>
                <span>
                  <span className="line" style={{ borderColor: palette.aqua }} />
                  CEP circle
                </span>
              </span>
              <span>✕ mean point of impact · dashed circle = drop zone</span>
            </>
          }
        />
      ) : (
        <div className="empty">{busy ? "Running Monte Carlo…" : "Press Run to simulate the landing dispersion."}</div>
      )}
      <div className="panel-col">
        <div className="card">
          <div className="card-head">
            <h3>Monte Carlo</h3>
            <div style={{ display: "flex", gap: 6 }}>
              <button className="btn" onClick={onExport} disabled={!mc}>
                Export CSV
              </button>
              <button className="btn primary" onClick={onRun} disabled={busy}>
                {busy ? "Running…" : "Run"}
              </button>
            </div>
          </div>
          <p>
            {fmtInt(scenario.monte_carlo.samples)} drops, seed {scenario.monte_carlo.seed}. Each draws a wind error (σ{" "}
            {fmt(u.wind_sigma, 1)} m/s), release timing error (σ {fmt(u.release_time_sigma, 2)} s), navigation error (σ{" "}
            {fmt(u.nav_sigma, 0)} m) and drag error (σ {fmt(u.drag_sigma * 100, 0)} %).
          </p>
          {m && mc && (
            <dl className="kv">
              <dt>CEP (about target)</dt>
              <dd>{fmt(m.cep)} m</dd>
              <dt>CEP (about MPI)</dt>
              <dd>{fmt(m.cep_mpi)} m</dd>
              <dt>Bias (MPI to target)</dt>
              <dd>{fmt(m.bias)} m</dd>
              <dt>Inside drop zone</dt>
              <dd>
                {fmt(mc.inside_drop_zone * 100)} % (r = {fmt(scenario.drop_zone_radius, 0)} m)
              </dd>
              {m.ellipses.map((e) => (
                <FragmentRow
                  key={e.probability}
                  label={`${fmt(e.probability * 100, 0)} % ellipse`}
                  value={`${fmt(e.semi_major * 2, 0)} × ${fmt(e.semi_minor * 2, 0)} m, axis ${fmt((90 - (e.angle * 180) / Math.PI + 360) % 180, 0)}°, holds ${fmt(e.observed_fraction * 100)} %`}
                />
              ))}
              <dt>Landed</dt>
              <dd>{fmt(m.landed_fraction * 100)} %</dd>
              <dt>Compute time</dt>
              <dd>{fmt(mc.compute_ms, 0)} ms</dd>
            </dl>
          )}
        </div>
        {m && (
          <div className="card">
            <h3>Why the ellipse is that shape</h3>
            <p>
              The long axis points {fmt((90 - (m.ellipses[0].angle * 180) / Math.PI + 360) % 180, 0)}° /{" "}
              {fmt(((90 - (m.ellipses[0].angle * 180) / Math.PI + 360) % 180) + 180, 0)}°. Timing errors stretch the
              pattern along the {fmt(scenario.aircraft.track_deg, 0)}° track; wind errors spread it in every direction
              by roughly σ<sub>wind</sub> × time of fall ≈ {fmt(u.wind_sigma * report.release.time_of_fall, 0)} m.
            </p>
            <p className="note">
              Track bearing from target to release point:{" "}
              {fmt(bearing(report.release.release_point[0] - scenario.target[0], report.release.release_point[1] - scenario.target[1]), 0)}°.
            </p>
          </div>
        )}
        {hist && (
          <div className="card">
            <h3>Miss distance distribution</h3>
            <UChart
              data={hist}
              palette={palette}
              xLabel="miss distance [m]"
              yLabel="% of drops"
              height={180}
              series={[{ label: "share", color: palette.blue, points: true }]}
            />
          </div>
        )}
      </div>
    </div>
  );
}

function FragmentRow({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt>{label}</dt>
      <dd style={{ whiteSpace: "normal" }}>{value}</dd>
    </>
  );
}
