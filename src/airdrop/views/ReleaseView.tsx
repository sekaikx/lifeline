import { useMemo } from "react";
import type uPlot from "uplot";
import { DropMap } from "../DropMap";
import { UChart } from "../../components/UChart";
import { bearing, fmt } from "../../lib/format";
import type { Palette } from "../../lib/theme";
import type { DropReport, Scenario } from "../types";

export function ReleaseView({ scenario, report, palette }: { scenario: Scenario; report: DropReport; palette: Palette }) {
  const r = report.release;
  const on = report.on_track;
  const ground = report.path[report.path.length - 1].pos.z;

  const altitude = useMemo<uPlot.AlignedData>(
    () => [report.path.map((p) => p.t), report.path.map((p) => p.pos.z - ground)],
    [report, ground],
  );
  const speeds = useMemo<uPlot.AlignedData>(
    () => [
      report.path.map((p) => p.t),
      report.path.map((p) => Math.hypot(p.vel.x, p.vel.y, p.vel.z)),
      report.path.map((p) => -p.vel.z),
    ],
    [report],
  );
  const fill = useMemo<uPlot.AlignedData>(() => [report.path.map((p) => p.t), report.path.map((p) => p.fill * 100)], [report]);

  return (
    <div className="split">
      <DropMap
        scenario={scenario}
        report={report}
        mode="plan"
        palette={palette}
        legend={
          <>
            <strong style={{ color: "var(--text)" }}>Release plan (top view)</strong>
            <span className="legend">
              <span>
                <span className="line" style={{ borderColor: palette.orange }} />
                Free fall
              </span>
              <span>
                <span className="line" style={{ borderColor: palette.blue }} />
                Under canopy
              </span>
              <span>
                <span className="line dashed" style={{ borderColor: palette.muted }} />
                Aircraft track
              </span>
            </span>
            <span>◆ solved release point · ◇ timing-only release on the target track</span>
          </>
        }
      />
      <div className="panel-col">
        <div className="card">
          <h3>Computed air release point</h3>
          <dl className="kv">
            <dt>Release point (E, N)</dt>
            <dd>
              {fmt(r.release_point[0])}, {fmt(r.release_point[1])} m
            </dd>
            <dt>Release altitude</dt>
            <dd>{fmt(r.release_altitude, 0)} m MSL</dd>
            <dt>Distance before target</dt>
            <dd>{fmt(r.release_distance_before_target)} m</dd>
            <dt>Green light lead</dt>
            <dd>{fmt(r.release_lead_time, 2)} s</dd>
            <dt>Forward travel</dt>
            <dd>{fmt(r.along_track_travel)} m</dd>
            <dt>Crosswind drift</dt>
            <dd>
              {fmt(Math.abs(r.cross_track_drift))} m {r.cross_track_drift >= 0 ? "right" : "left"}
            </dd>
            <dt>Fly track through CARP</dt>
            <dd>{fmt(scenario.aircraft.track_deg, 0)}° true</dd>
            <dt>Time of fall</dt>
            <dd>{fmt(r.time_of_fall, 2)} s</dd>
            <dt>Impact speed</dt>
            <dd>
              {fmt(r.impact_speed, 2)} m/s ({fmt(r.impact_vertical_speed, 2)} vertical)
            </dd>
            <dt>Solver</dt>
            <dd>
              {r.solver}, {r.iterations} it., miss {r.miss_distance < 0.001 ? "< 1 mm" : `${fmt(r.miss_distance, 3)} m`}
            </dd>
          </dl>
        </div>
        {on && (
          <div className="card">
            <h3>Timing only (Brent on track)</h3>
            <p>
              Keep flying the {fmt(scenario.aircraft.track_deg, 0)}° track straight over the target and only choose
              when to release. Brent's method zeroes the along-track miss, but crosswind drift remains:
            </p>
            <dl className="kv">
              <dt>Release before target</dt>
              <dd>{fmt(on.release_distance_before_target)} m</dd>
              <dt>Cross-track miss</dt>
              <dd>
                {fmt(Math.abs(on.cross_track_miss))} m {on.cross_track_miss >= 0 ? "right" : "left"}
              </dd>
              <dt>Offset track needed</dt>
              <dd>
                {fmt(Math.abs(r.cross_track_drift))} m {r.cross_track_drift >= 0 ? "left" : "right"} (
                {fmt(bearing(r.release_point[0] - scenario.target[0], r.release_point[1] - scenario.target[1]), 0)}° from
                target)
              </dd>
            </dl>
          </div>
        )}
        <div className="card">
          <h3>Height above ground</h3>
          <UChart data={altitude} palette={palette} xLabel="time [s]" yLabel="height [m]" height={170} series={[{ label: "height", color: palette.blue }]} />
        </div>
        <div className="card">
          <h3>Speed</h3>
          <UChart
            data={speeds}
            palette={palette}
            xLabel="time [s]"
            yLabel="m/s"
            height={170}
            series={[
              { label: "total", color: palette.blue },
              { label: "descent rate", color: palette.orange },
            ]}
          />
        </div>
        {scenario.payload.parachute && (
          <div className="card">
            <h3>Canopy inflation</h3>
            <UChart data={fill} palette={palette} xLabel="time [s]" yLabel="fill [%]" height={150} series={[{ label: "fill", color: palette.blue }]} />
          </div>
        )}
      </div>
    </div>
  );
}
