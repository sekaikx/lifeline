import { useEffect, useRef, useState } from "react";
import { Scene3D, type Playback } from "../Scene3D";
import { fmt } from "../../lib/format";
import type { Palette } from "../../lib/theme";
import type { DropReport, MonteCarloReport, PathPoint, Scenario } from "../types";

const PRE_ROLL = 8; // seconds of approach shown before release

export function DropView({
  scenario,
  report,
  mc,
  palette,
}: {
  scenario: Scenario;
  report: DropReport;
  mc: MonteCarloReport | null;
  palette: Palette;
}) {
  const tEnd = report.release.time_of_fall + 3;
  const playback = useRef<Playback>({ t: -PRE_ROLL, playing: true, speed: 4, follow: false });
  const [hud, setHud] = useState<{ p: PathPoint; t: number }>({ p: report.path[0], t: -PRE_ROLL });
  const [playing, setPlaying] = useState(true);
  const [speed, setSpeed] = useState(4);
  const [follow, setFollow] = useState(false);
  const [showMc, setShowMc] = useState(true);
  const [resetView, setResetView] = useState(0);

  // restart the animation when the solution changes
  const key = `${report.release.release_point.join()}|${report.release.time_of_fall}`;
  useEffect(() => {
    if (playback.current.t > tEnd) playback.current.t = -PRE_ROLL;
  }, [key, tEnd]);

  const ground = report.path[report.path.length - 1].pos.z;
  const p = hud.p;
  const released = hud.t > 0;
  const speedNow = Math.hypot(p.vel.x, p.vel.y, p.vel.z);

  return (
    <div className="scene-wrap">
      <Scene3D
        scenario={scenario}
        report={report}
        impacts={showMc && mc ? mc.result.impacts : null}
        palette={palette}
        playback={playback}
        tStart={-PRE_ROLL}
        tEnd={tEnd}
        resetView={resetView}
        onTick={(pt, t) => setHud({ p: pt, t })}
      />
      <div className="scene-legend">
        <span>
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
              <span className="line" style={{ borderColor: palette.aqua }} />
              Wind (×8 length)
            </span>
            {mc && showMc && (
              <span>
                <span className="swatch" style={{ background: palette.blue }} />
                Monte Carlo impacts
              </span>
            )}
          </span>
        </span>
        <span className="note">Aircraft and package drawn ×6 scale · drag to orbit, scroll to zoom</span>
      </div>
      <div className="scene-tools">
        {mc && (
          <label className="icon-btn toggle">
            <input type="checkbox" checked={showMc} onChange={(e) => setShowMc(e.target.checked)} /> Impacts
          </label>
        )}
        <label className="icon-btn toggle">
          <input
            type="checkbox"
            checked={follow}
            onChange={(e) => {
              setFollow(e.target.checked);
              playback.current.follow = e.target.checked;
            }}
          />{" "}
          Follow
        </label>
        <button className="icon-btn" onClick={() => setResetView((n) => n + 1)}>
          Reset view
        </button>
      </div>
      <div className="hud">
        <button
          className="btn"
          style={{ minWidth: 64 }}
          onClick={() => {
            playback.current.playing = !playing;
            setPlaying(!playing);
          }}
        >
          {playing ? "Pause" : "Play"}
        </button>
        <select
          className="select"
          style={{ width: 70 }}
          value={speed}
          aria-label="Playback speed"
          onChange={(e) => {
            const s = Number(e.target.value);
            playback.current.speed = s;
            setSpeed(s);
          }}
        >
          {[1, 2, 4, 8, 16].map((s) => (
            <option key={s} value={s}>
              {s}×
            </option>
          ))}
        </select>
        <input
          type="range"
          aria-label="Time"
          min={-PRE_ROLL}
          max={tEnd}
          step={0.05}
          value={hud.t}
          onChange={(e) => {
            playback.current.t = Number(e.target.value);
            setHud((h) => ({ ...h, t: playback.current.t }));
          }}
        />
        <div className="readouts">
          <div>
            <span>t</span>
            {hud.t < 0 ? `release in ${fmt(-hud.t, 1)} s` : `${fmt(hud.t, 1)} s`}
          </div>
          <div>
            <span>height</span>
            {released ? fmt(p.pos.z - ground, 0) : fmt(scenario.aircraft.height_agl, 0)} m
          </div>
          <div>
            <span>speed</span>
            {released ? fmt(speedNow, 1) : fmt(scenario.aircraft.ground_speed, 1)} m/s
          </div>
          <div>
            <span>canopy</span>
            {released ? fmt(p.fill * 100, 0) : "0"} %
          </div>
        </div>
      </div>
    </div>
  );
}
