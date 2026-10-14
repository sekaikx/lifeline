import { useMemo, useState } from "react";
import type uPlot from "uplot";
import { Segmented } from "../../components/Field";
import { UChart } from "../../components/UChart";
import { fmt, fmtInt, fmtSci } from "../../lib/format";
import type { Palette } from "../../lib/theme";
import type { MethodPath, NumericsReport } from "../types";

const DT_CHOICES = [0.05, 0.1, 0.25, 0.5, 1, 2];

/** Merge several (x, y) polylines onto one shared x axis for uPlot (mode 1). */
function alignPaths(paths: MethodPath[]): uPlot.AlignedData {
  const xs = Array.from(new Set(paths.flatMap((p) => p.points.map((q) => q[0])))).sort((a, b) => a - b);
  const index = new Map(xs.map((x, i) => [x, i]));
  const ys = paths.map((p) => {
    const col: (number | null)[] = new Array(xs.length).fill(null);
    for (const [x, y] of p.points) col[index.get(x)!] = y;
    return col;
  });
  return [xs, ...ys] as uPlot.AlignedData;
}

export function NumericsView({
  report,
  dt,
  setDt,
  palette,
}: {
  report: NumericsReport | null;
  dt: number;
  setDt: (dt: number) => void;
  palette: Palette;
}) {
  const [cmpId, setCmpId] = useState("vacuum");
  const [studyId, setStudyId] = useState("full-smooth");

  const cmp = report?.comparisons.find((c) => c.id === cmpId) ?? report?.comparisons[0];
  const study = report?.studies.find((s) => s.id === studyId) ?? report?.studies[0];

  const colorOf = (label: string) => (label === "euler" ? palette.orange : label === "rk4" ? palette.blue : palette.text2);
  const nameOf = (label: string) => ({ euler: "Euler", rk4: "RK4", exact: "Exact", reference: "Reference" })[label] ?? label;

  const cmpData = useMemo(() => (cmp ? alignPaths(cmp.paths) : null), [cmp]);
  const convData = useMemo<uPlot.AlignedData | null>(() => {
    if (!study) return null;
    const dts = study.series[0].dts;
    return [dts, ...study.series.map((s) => s.errors.map((e) => (e == null || e <= 0 ? null : e)))] as uPlot.AlignedData;
  }, [study]);
  const workData = useMemo<uPlot.AlignedData | null>(() => {
    if (!study) return null;
    const paths: MethodPath[] = study.series.map((s) => ({
      label: s.method,
      points: s.evaluations.map((n, i) => [n, s.errors[i] ?? NaN] as [number, number]).filter(([, e]) => e > 0 && Number.isFinite(e)),
      impact_range: 0,
      impact_error: 0,
      evaluations: 0,
    }));
    return alignPaths(paths);
  }, [study]);

  if (!report || !cmp || !study || !cmpData || !convData || !workData) {
    return <div className="empty">Computing convergence studies…</div>;
  }

  const unstable = study.series.flatMap((s) => s.dts.filter((_, i) => s.unstable[i]).map((d) => `${nameOf(s.method)} at dt = ${d} s`));
  const euler = cmp.paths.find((p) => p.label === "euler")!;
  const rk4 = cmp.paths.find((p) => p.label === "rk4")!;

  return (
    <div className="grid2">
      <div className="card">
        <div className="card-head">
          <h3>Euler vs RK4 vs truth</h3>
          <Segmented
            ariaLabel="Comparison case"
            value={cmp.id}
            options={report.comparisons.map((c) => ({ value: c.id, label: c.id === "vacuum" ? "No drag" : "Drag + wind" }))}
            onChange={setCmpId}
          />
        </div>
        <p>
          {cmp.title}. A projectile fired at 45° and 120 m/s from 200 m, integrated with step{" "}
          <select className="select" style={{ width: 80, display: "inline-block" }} value={dt} aria-label="Comparison step" onChange={(e) => setDt(Number(e.target.value))}>
            {DT_CHOICES.map((d) => (
              <option key={d} value={d}>
                {d} s
              </option>
            ))}
          </select>
        </p>
        <UChart
          data={cmpData}
          palette={palette}
          xLabel="downrange [m]"
          yLabel="height [m]"
          height={280}
          series={cmp.paths.map((p) => ({
            label: nameOf(p.label),
            color: colorOf(p.label),
            dash: p.label === "exact" || p.label === "reference" ? [6, 5] : undefined,
            spanGaps: true,
          }))}
        />
      </div>

      <div className="card">
        <h3>Impact error at dt = {cmp.dt} s</h3>
        <table className="data">
          <thead>
            <tr>
              <th>Method</th>
              <th>Range [m]</th>
              <th>Impact error [m]</th>
              <th>RHS evaluations</th>
            </tr>
          </thead>
          <tbody>
            {cmp.paths.map((p) => (
              <tr key={p.label}>
                <td>
                  <span className="swatch" style={{ background: colorOf(p.label) }} />
                  {nameOf(p.label)}
                </td>
                <td>{fmt(p.impact_range, 2)}</td>
                <td>{p.label === "euler" || p.label === "rk4" ? fmtSci(p.impact_error) : "—"}</td>
                <td>{p.evaluations ? p.evaluations.toLocaleString() : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p style={{ marginTop: 12 }}>
          {rk4.impact_error < 1e-9 ? (
            <>
              Euler misses the impact point by <strong style={{ color: "var(--text)" }}>{fmt(euler.impact_error, 1)} m</strong>; RK4 is
              exact to floating-point round-off. Without drag the path is a parabola (a polynomial of degree 2), and RK4
              integrates polynomials up to degree 4 exactly.
            </>
          ) : (
            <>
              At the same step RK4 lands{" "}
              <strong style={{ color: "var(--text)" }}>{fmtInt(euler.impact_error / rk4.impact_error)}×</strong> closer to the truth than
              Euler ({fmtSci(rk4.impact_error)} m vs {fmt(euler.impact_error, 1)} m) while costing{" "}
              {fmt(rk4.evaluations / euler.evaluations, 1)}× the derivative evaluations.
            </>
          )}
        </p>
        <p className="note">
          Euler takes each step along the slope at its start, so on a curving trajectory it always overshoots outward;
          RK4 averages four slopes across the step and cancels the error through 4th order.
        </p>
      </div>

      <div className="card">
        <div className="card-head">
          <h3>Convergence: error vs step size</h3>
          <select className="select" style={{ width: 300 }} value={study.id} aria-label="Convergence study" onChange={(e) => setStudyId(e.target.value)}>
            {report.studies.map((s) => (
              <option key={s.id} value={s.id}>
                {s.title}
              </option>
            ))}
          </select>
        </div>
        <UChart
          data={convData}
          palette={palette}
          logX
          logY
          xLabel="step size dt [s]"
          yLabel="position error [m]"
          height={280}
          xFormat={(v) => String(+v.toPrecision(3))}
          yFormat={fmtSci}
          series={study.series.map((s) => ({
            label: `${nameOf(s.method)} · order ${fmt(s.observed_order, 2)}`,
            color: colorOf(s.method),
            points: true,
          }))}
        />
        <p className="note">
          Error at t = {study.t_end} s against the {study.reference}. Slope on log-log axes = order of accuracy: Euler
          halves its error when dt halves (order 1); RK4 divides it by 16 (order 4).
          {unstable.length > 0 && <> Unstable runs (no point drawn): {unstable.join(", ")}.</>}
        </p>
      </div>

      <div className="card">
        <h3>Work vs precision</h3>
        <UChart
          data={workData}
          palette={palette}
          logX
          logY
          xLabel="RHS evaluations"
          yLabel="position error [m]"
          height={280}
          xFormat={(v) => (v >= 1000 ? `${+(v / 1000).toPrecision(3)}k` : String(+v.toPrecision(3)))}
          yFormat={fmtSci}
          series={study.series.map((s) => ({ label: nameOf(s.method), color: colorOf(s.method), points: true, spanGaps: true }))}
        />
        <p className="note">
          The fair comparison: same computational cost on the x axis. RK4 costs 4 evaluations per step but is still far
          more accurate for any given budget.
        </p>
      </div>
    </div>
  );
}
