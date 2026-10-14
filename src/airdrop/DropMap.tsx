import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { fmt, niceStep } from "../lib/format";
import type { Palette } from "../lib/theme";
import type { DropReport, MonteCarloReport, Scenario } from "./types";

interface Props {
  scenario: Scenario;
  report: DropReport;
  mc?: MonteCarloReport | null;
  mode: "plan" | "dispersion";
  palette: Palette;
  legend: ReactNode;
}

interface View {
  cx: number;
  cy: number;
  /** metres per pixel */
  mpp: number;
}

/** Top-down map in ENU metres with wheel zoom, drag pan and hover readout. */
export function DropMap({ scenario, report, mc, mode, palette, legend }: Props) {
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [view, setView] = useState<View | null>(null);
  const [cursor, setCursor] = useState<{ x: number; y: number; e: number; n: number } | null>(null);
  const drag = useRef<{ x: number; y: number; view: View } | null>(null);

  const rel = report.release;
  const [tx, ty] = scenario.target;
  const impacts = mode === "dispersion" ? (mc?.result.impacts ?? null) : null;

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const fitView = useCallback((): View => {
    const r = scenario.drop_zone_radius;
    const xs = [tx - r, tx + r, rel.landing_point.x];
    const ys = [ty - r, ty + r, rel.landing_point.y];
    if (mode === "plan") {
      xs.push(rel.release_point[0]);
      ys.push(rel.release_point[1]);
      if (report.on_track) {
        xs.push(report.on_track.release_point[0], report.on_track.landing_point.x);
        ys.push(report.on_track.release_point[1], report.on_track.landing_point.y);
      }
    } else if (mc) {
      for (const e of mc.result.ellipses) {
        xs.push(e.center[0] - e.semi_major, e.center[0] + e.semi_major);
        ys.push(e.center[1] - e.semi_major, e.center[1] + e.semi_major);
      }
    }
    const [x0, x1] = [Math.min(...xs), Math.max(...xs)];
    const [y0, y1] = [Math.min(...ys), Math.max(...ys)];
    const mpp = Math.max((x1 - x0) / (size.w * 0.8), (y1 - y0) / (size.h * 0.8), 0.05);
    return { cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, mpp };
  }, [tx, ty, rel, scenario.drop_zone_radius, mode, report.on_track, mc, size.w, size.h]);

  // Re-fit when the map changes mode or the user has not zoomed yet.
  const userMoved = useRef(false);
  useEffect(() => {
    if (!userMoved.current || view === null) setView(fitView());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fitView]);
  useEffect(() => {
    userMoved.current = false;
    setView(fitView());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  const v = view ?? fitView();
  const sx = (e: number) => size.w / 2 + (e - v.cx) / v.mpp;
  const sy = (n: number) => size.h / 2 - (n - v.cy) / v.mpp;
  const toWorld = (x: number, y: number) => ({ e: v.cx + (x - size.w / 2) * v.mpp, n: v.cy - (y - size.h / 2) * v.mpp });

  // ---------- impact dots on canvas
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = size.w * dpr;
    c.height = size.h * dpr;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size.w, size.h);
    if (!impacts) return;
    ctx.fillStyle = palette.blue;
    ctx.globalAlpha = impacts.length > 5000 ? 0.35 : 0.6;
    const r = impacts.length > 5000 ? 1.5 : 2.5;
    for (const [e, n] of impacts) {
      ctx.beginPath();
      ctx.arc(size.w / 2 + (e - v.cx) / v.mpp, size.h / 2 - (n - v.cy) / v.mpp, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }, [impacts, v.cx, v.cy, v.mpp, size, palette]);

  // ---------- grid
  const grid = useMemo(() => {
    const step = niceStep(size.w * v.mpp, 8);
    const left = v.cx - (size.w / 2) * v.mpp;
    const right = v.cx + (size.w / 2) * v.mpp;
    const bottom = v.cy - (size.h / 2) * v.mpp;
    const top = v.cy + (size.h / 2) * v.mpp;
    const xs: number[] = [];
    const ys: number[] = [];
    for (let e = Math.ceil(left / step) * step; e <= right; e += step) xs.push(e);
    for (let n = Math.ceil(bottom / step) * step; n <= top; n += step) ys.push(n);
    return { step, xs, ys };
  }, [v.cx, v.cy, v.mpp, size]);

  // ---------- hover
  const nearest = useMemo(() => {
    if (!cursor || !impacts) return null;
    let best = -1;
    let bestD = 64; // px^2 (8 px radius)
    impacts.forEach(([e, n], i) => {
      const d = (sx(e) - cursor.x) ** 2 + (sy(n) - cursor.y) ** 2;
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    });
    return best >= 0 ? best : null;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cursor, impacts, v]);

  const path = report.path;
  const iCanopy = path.findIndex((p) => p.fill >= 0.02);
  const toPts = (pts: typeof path) => pts.map((p) => `${sx(p.pos.x)},${sy(p.pos.y)}`).join(" ");
  const freefallPts = toPts(iCanopy < 0 ? path : path.slice(0, iCanopy + 1));
  const canopyPts = iCanopy < 0 ? "" : toPts(path.slice(iCanopy));

  const track = (scenario.aircraft.track_deg * Math.PI) / 180;
  const ue = Math.sin(track);
  const un = Math.cos(track);
  const far = 20 * size.w * v.mpp;
  const trackThrough = (px: number, py: number) => ({
    x1: sx(px - ue * far),
    y1: sy(py - un * far),
    x2: sx(px + ue * far),
    y2: sy(py + un * far),
  });

  const scaleLen = niceStep(size.w * v.mpp, 6);
  const surfaceWind = report.wind_profile[0]?.wind;
  const releaseWind = report.wind_profile[report.wind_profile.length - 1]?.wind;

  const onWheel = (ev: React.WheelEvent) => {
    const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const before = toWorld(x, y);
    const mpp = v.mpp * Math.exp(ev.deltaY * 0.0015);
    userMoved.current = true;
    setView({ mpp, cx: before.e - (x - size.w / 2) * mpp, cy: before.n + (y - size.h / 2) * mpp });
  };

  const marker = (id: string, color: string) => (
    <marker id={id} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill={color} />
    </marker>
  );

  const windArrow = (w: { x: number; y: number } | undefined, x: number, y: number, label: string) => {
    if (!w) return null;
    const s = Math.hypot(w.x, w.y);
    const k = 10; // px per m/s
    return (
      <g>
        <line x1={x} y1={y} x2={x + w.x * k} y2={y - w.y * k} stroke={palette.aqua} strokeWidth={2} markerEnd="url(#arrow-aqua)" />
        <text x={x + 8} y={y + 18}>
          {label} {fmt(s, 1)} m/s
        </text>
      </g>
    );
  };

  return (
    <div className="map-wrap" ref={wrap}>
      <canvas ref={canvas} style={{ position: "absolute", inset: 0, width: "100%", height: "100%", pointerEvents: "none" }} />
      <svg
        className="map"
        style={{ position: "absolute", inset: 0, background: "transparent", zIndex: 1 }}
        width={size.w}
        height={size.h}
        onWheel={onWheel}
        onPointerDown={(e) => {
          (e.currentTarget as Element).setPointerCapture(e.pointerId);
          drag.current = { x: e.clientX, y: e.clientY, view: v };
        }}
        onPointerUp={() => (drag.current = null)}
        onPointerLeave={() => setCursor(null)}
        onPointerMove={(e) => {
          const rect = e.currentTarget.getBoundingClientRect();
          const x = e.clientX - rect.left;
          const y = e.clientY - rect.top;
          if (drag.current) {
            const d = drag.current;
            userMoved.current = true;
            setView({ ...d.view, cx: d.view.cx - (e.clientX - d.x) * d.view.mpp, cy: d.view.cy + (e.clientY - d.y) * d.view.mpp });
          }
          setCursor({ x, y, ...toWorld(x, y) });
        }}
      >
        <defs>
          {marker("arrow-muted", palette.muted)}
          {marker("arrow-aqua", palette.aqua)}
        </defs>
        <g>
          {grid.xs.map((e) => (
            <g key={`x${e}`}>
              <line x1={sx(e)} x2={sx(e)} y1={0} y2={size.h} stroke={palette.grid} />
              <text x={sx(e) + 4} y={size.h - 6}>
                {fmt(e, 0)}
              </text>
            </g>
          ))}
          {grid.ys.map((n) => (
            <g key={`y${n}`}>
              <line x1={0} x2={size.w} y1={sy(n)} y2={sy(n)} stroke={palette.grid} />
              <text x={size.w - 6} y={sy(n) - 4} textAnchor="end">
                {fmt(n, 0)}
              </text>
            </g>
          ))}
        </g>

        {/* drop zone + target */}
        <circle cx={sx(tx)} cy={sy(ty)} r={scenario.drop_zone_radius / v.mpp} fill="none" stroke={palette.text2} strokeDasharray="6 5" />
        <g stroke={palette.text} strokeWidth={1.5}>
          <circle cx={sx(tx)} cy={sy(ty)} r={6} fill="none" />
          <line x1={sx(tx) - 11} x2={sx(tx) + 11} y1={sy(ty)} y2={sy(ty)} />
          <line x1={sx(tx)} x2={sx(tx)} y1={sy(ty) - 11} y2={sy(ty) + 11} />
        </g>
        <text x={sx(tx) + 10} y={sy(ty) + 22}>
          Target
        </text>

        {/* aircraft track through the solved release point */}
        <line {...trackThrough(rel.release_point[0], rel.release_point[1])} stroke={palette.muted} strokeDasharray="10 7" />

        {mode === "plan" && (
          <>
            {report.on_track && (
              <g>
                <line {...trackThrough(tx, ty)} stroke={palette.muted} strokeOpacity={0.45} strokeDasharray="2 6" />
                <line
                  x1={sx(tx)}
                  y1={sy(ty)}
                  x2={sx(report.on_track.landing_point.x)}
                  y2={sy(report.on_track.landing_point.y)}
                  stroke={palette.text2}
                  strokeDasharray="3 3"
                />
                <rect
                  x={sx(report.on_track.release_point[0]) - 5}
                  y={sy(report.on_track.release_point[1]) - 5}
                  width={10}
                  height={10}
                  transform={`rotate(45 ${sx(report.on_track.release_point[0])} ${sy(report.on_track.release_point[1])})`}
                  fill={palette.surface}
                  stroke={palette.text2}
                />
                <circle cx={sx(report.on_track.landing_point.x)} cy={sy(report.on_track.landing_point.y)} r={5} fill={palette.surface} stroke={palette.text2} />
                <text x={sx(report.on_track.landing_point.x) + 9} y={sy(report.on_track.landing_point.y) + 4}>
                  timing only: misses by {fmt(Math.abs(report.on_track.cross_track_miss), 0)} m
                </text>
              </g>
            )}
            <polyline points={freefallPts} fill="none" stroke={palette.orange} strokeWidth={2.5} />
            {canopyPts && <polyline points={canopyPts} fill="none" stroke={palette.blue} strokeWidth={2.5} />}
          </>
        )}

        {mode === "dispersion" && mc && (
          <g>
            {mc.result.ellipses.map((e) => (
              <ellipse
                key={e.probability}
                cx={sx(e.center[0])}
                cy={sy(e.center[1])}
                rx={Math.max(e.semi_major / v.mpp, 0.5)}
                ry={Math.max(e.semi_minor / v.mpp, 0.5)}
                transform={`rotate(${(-e.angle * 180) / Math.PI} ${sx(e.center[0])} ${sy(e.center[1])})`}
                fill="none"
                stroke={palette.orange}
                strokeWidth={2}
                strokeDasharray={e.probability > 0.9 ? "8 5" : undefined}
              />
            ))}
            <circle cx={sx(tx)} cy={sy(ty)} r={mc.result.cep / v.mpp} fill="none" stroke={palette.aqua} strokeWidth={2} />
            <g stroke={palette.text} strokeWidth={2}>
              <line x1={sx(mc.result.mpi[0]) - 6} y1={sy(mc.result.mpi[1]) - 6} x2={sx(mc.result.mpi[0]) + 6} y2={sy(mc.result.mpi[1]) + 6} />
              <line x1={sx(mc.result.mpi[0]) - 6} y1={sy(mc.result.mpi[1]) + 6} x2={sx(mc.result.mpi[0]) + 6} y2={sy(mc.result.mpi[1]) - 6} />
            </g>
          </g>
        )}

        {/* solved release point */}
        <g>
          <rect
            x={sx(rel.release_point[0]) - 6}
            y={sy(rel.release_point[1]) - 6}
            width={12}
            height={12}
            transform={`rotate(45 ${sx(rel.release_point[0])} ${sy(rel.release_point[1])})`}
            fill={palette.text}
          />
          <text x={sx(rel.release_point[0]) + 12} y={sy(rel.release_point[1]) - 10}>
            Release (CARP)
          </text>
        </g>

        {/* wind, north arrow and scale */}
        <g>
          <line x1={size.w - 40} y1={92} x2={size.w - 40} y2={58} stroke={palette.text2} strokeWidth={2} markerEnd="url(#arrow-muted)" />
          <text x={size.w - 40} y={50} textAnchor="middle" style={{ fontWeight: 600 }}>
            N
          </text>
          {windArrow(surfaceWind, size.w - 230, 70, "surface")}
          {windArrow(releaseWind, size.w - 230, 130, "release alt.")}
          <g transform={`translate(${size.w - 24 - scaleLen / v.mpp}, ${size.h - 36})`}>
            <line x1={0} x2={scaleLen / v.mpp} y1={0} y2={0} stroke={palette.text} strokeWidth={2} />
            <line x1={0} x2={0} y1={-4} y2={4} stroke={palette.text} strokeWidth={2} />
            <line x1={scaleLen / v.mpp} x2={scaleLen / v.mpp} y1={-4} y2={4} stroke={palette.text} strokeWidth={2} />
            <text x={scaleLen / v.mpp / 2} y={-8} textAnchor="middle">
              {fmt(scaleLen, 0)} m
            </text>
          </g>
        </g>

        {nearest !== null && impacts && (
          <circle cx={sx(impacts[nearest][0])} cy={sy(impacts[nearest][1])} r={6} fill="none" stroke={palette.text} strokeWidth={2} />
        )}
      </svg>
      <div className="map-overlay" style={{ zIndex: 2 }}>
        {legend}
      </div>
      <div className="map-tools" style={{ zIndex: 2 }}>
        <button
          className="icon-btn"
          onClick={() => {
            userMoved.current = false;
            setView(fitView());
          }}
        >
          Fit
        </button>
      </div>
      {cursor && (
        <div className="map-cursor" style={{ zIndex: 2 }}>
          E {fmt(cursor.e, 0)} m · N {fmt(cursor.n, 0)} m · grid {fmt(grid.step, 0)} m · scroll to zoom, drag to pan
        </div>
      )}
      {nearest !== null && impacts && cursor && (
        <div className="tooltip" style={{ left: cursor.x + 14, top: cursor.y + 14, zIndex: 3 }}>
          Drop #{nearest + 1}
          <br />
          E {fmt(impacts[nearest][0], 1)} m, N {fmt(impacts[nearest][1], 1)} m
          <br />
          Miss {fmt(Math.hypot(impacts[nearest][0] - tx, impacts[nearest][1] - ty), 1)} m
          {mc && (
            <>
              <br />
              Fall time {fmt(mc.result.times_of_fall[nearest], 1)} s
            </>
          )}
        </div>
      )}
    </div>
  );
}
