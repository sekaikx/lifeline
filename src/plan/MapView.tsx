import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fmt, niceStep } from "../lib/format";
import type { Palette, ThemeMode } from "../lib/theme";
import { placeLabels, type LabelInput } from "./labels";
import { elevation, reliefStyle, renderRelief, renderRisk } from "./render";
import type { HazardMask, Imagery, Mode, Plan, PlanScenario, Point, RiskLayers, World } from "./types";

export type RiskLayer = "off" | "ground" | "drone" | "air";
export type BaseLayer = "relief" | "satellite";
export type Placing = null | "request" | "site" | "hazard";
type DragKind = "request" | "depot" | "site";

interface Props {
  world: World;
  scenario: PlanScenario;
  plan: Plan | null;
  layers: RiskLayers | null;
  palette: Palette;
  theme: ThemeMode;
  selected: string | null;
  onSelect: (id: string | null) => void;
  riskLayer: RiskLayer;
  placing: Placing;
  onPlace: (x: number, y: number) => void;
  onMove: (kind: DragKind, id: string, x: number, y: number) => void;
  baseLayer: BaseLayer;
  imagery: Pick<Imagery, "png_base64" | "bounds" | "attribution"> | null;
  /** Height of the map key overlay, kept clear of labels [px] */
  keyHeight: number;
}

/** Hatched raster of a hazard mask, at three pixels per cell. */
function renderMask(m: HazardMask, color: string): HTMLCanvasElement {
  const k = 3;
  const { nx, ny } = m.mask;
  const c = document.createElement("canvas");
  c.width = nx * k;
  c.height = ny * k;
  const ctx = c.getContext("2d")!;
  const img = ctx.createImageData(c.width, c.height);
  const v = parseInt(color.slice(1), 16);
  const [r, g, b] = [(v >> 16) & 255, (v >> 8) & 255, v & 255];
  for (const cell of m.mask.cells) {
    const i = cell % nx;
    const j = Math.floor(cell / nx);
    for (let dy = 0; dy < k; dy++)
      for (let dx = 0; dx < k; dx++) {
        const px = i * k + dx;
        const py = (ny - 1 - j) * k + dy;
        const on = (px + py) % 4 === 0; // diagonal hatch
        const o = (py * c.width + px) * 4;
        img.data[o] = r;
        img.data[o + 1] = g;
        img.data[o + 2] = b;
        img.data[o + 3] = on ? 210 : 35;
      }
  }
  ctx.putImageData(img, 0, 0);
  return c;
}

interface View {
  cx: number;
  cy: number;
  /** metres per pixel */
  mpp: number;
}

/** Ink drawn on a mode-coloured fill: dark, for contrast on all three hues. */
export const INK_ON_MODE = "#0b0b0b";

export const modeColor = (p: Palette, m: Mode | null) =>
  m === "truck" ? p.blue : m === "drone" ? p.orange : m === "airdrop" ? p.aqua : p.muted;

export function MapView(props: Props) {
  const { world, scenario, plan, layers, palette, theme, selected, riskLayer, placing, baseLayer, imagery } = props;
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [view, setView] = useState<View | null>(null);
  const readout = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<{ kind: DragKind; id: string; x: number; y: number; moved: boolean } | null>(null);
  const pan = useRef<{ x: number; y: number; view: View; moved: boolean } | null>(null);

  const t = world.terrain;
  const [bx0, by0, bx1, by1] = [t.x0, t.y0, t.x0 + (t.nx - 1) * t.dx, t.y0 + (t.ny - 1) * t.dy];

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const fit = useCallback((): View => {
    // Fit the area above the footer. The container can report zero size
    // before layout; never fit to less than 100 px.
    const [left, top, right, bottom] = [16, 16, size.w - 16, size.h - 36];
    const mpp = Math.max((bx1 - bx0) / Math.max(right - left, 100), (by1 - by0) / Math.max(bottom - top, 100));
    return {
      cx: (bx0 + bx1) / 2 - ((left + right) / 2 - size.w / 2) * mpp,
      cy: (by0 + by1) / 2 + ((top + bottom) / 2 - size.h / 2) * mpp,
      mpp,
    };
  }, [bx0, bx1, by0, by1, size.w, size.h]);
  const touched = useRef(false);
  useEffect(() => {
    if (!touched.current) setView(fit());
  }, [fit]);

  const v = view ?? fit();
  // Pointer and wheel events arrive faster than the screen refreshes; the
  // newest view and drag are kept here and committed once per frame.
  const frame = useRef(0);
  const liveView = useRef<View>(v);
  const liveDrag = useRef(drag);
  if (!frame.current) {
    liveView.current = v;
    liveDrag.current = drag;
  }
  const commit = () => {
    if (frame.current) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      setView(liveView.current);
      setDrag(liveDrag.current);
    });
  };
  useEffect(() => () => cancelAnimationFrame(frame.current), []);
  const sx = (x: number) => size.w / 2 + (x - v.cx) / v.mpp;
  const sy = (y: number) => size.h / 2 - (y - v.cy) / v.mpp;
  const toWorld = (x: number, y: number) => ({ e: v.cx + (x - size.w / 2) * v.mpp, n: v.cy - (y - size.h / 2) * v.mpp });
  const pts = (p: Point[]) => p.map(([x, y]) => `${sx(x).toFixed(1)},${sy(y).toFixed(1)}`).join(" ");

  // ------------------------------------------------------------ rasters
  // at most ~2000 px a side: finer adds nothing at screen zoom and costs seconds
  const relief = useMemo(() => renderRelief(t, reliefStyle(theme), Math.max(1, Math.min(4, Math.floor(2048 / Math.max(t.nx - 1, 1))))), [t, theme]);
  // roads in world coordinates, one path per class, built once per world
  const roadPaths = useMemo(() => {
    const by = new Map<string, Path2D>();
    for (const r of world.network.roads) {
      let path = by.get(r.class);
      if (!path) by.set(r.class, (path = new Path2D()));
      r.points.forEach(([x, y], k) => (k ? path.lineTo(x, y) : path.moveTo(x, y)));
    }
    return { by };
  }, [world]);
  const risk = useMemo(() => {
    if (!layers || riskLayer === "off") return null;
    return { img: renderRisk(layers[riskLayer], palette.critical), r: layers[riskLayer] };
  }, [layers, riskLayer, palette]);

  const [imgReady, setImgReady] = useState(0);
  const satellite = useMemo(() => {
    if (!imagery) return null;
    const img = new Image();
    img.onload = () => setImgReady((n) => n + 1);
    img.src = `data:image/png;base64,${imagery.png_base64}`;
    return img;
  }, [imagery]);
  const masks = useMemo(() => scenario.risk.masks.map((m) => ({ m, img: renderMask(m, palette.critical) })), [scenario.risk.masks, palette]);

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = size.w * dpr;
    c.height = size.h * dpr;
    const ctx = c.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = palette.surface;
    ctx.fillRect(0, 0, size.w, size.h);
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    // naturalWidth is 0 for an image that failed to decode; drawing it would throw
    if (baseLayer === "satellite" && satellite?.complete && satellite.naturalWidth > 0 && imagery) {
      const [w, sBound, e, n] = imagery.bounds;
      ctx.drawImage(satellite, sx(w), sy(n), (e - w) / v.mpp, (n - sBound) / v.mpp);
    } else {
      ctx.drawImage(relief, sx(bx0), sy(by1), (bx1 - bx0) / v.mpp, (by1 - by0) / v.mpp);
    }
    if (risk) {
      const { r } = risk;
      const half = r.cell / 2;
      ctx.drawImage(risk.img, sx(r.x0 - half), sy(r.y0 + (r.ny - 1) * r.cell + half), (r.nx * r.cell) / v.mpp, (r.ny * r.cell) / v.mpp);
    }
    ctx.imageSmoothingEnabled = false;
    for (const { m, img } of masks) {
      const half = m.mask.cell / 2;
      ctx.drawImage(img, sx(m.mask.x0 - half), sy(m.mask.y0 + (m.mask.ny - 1) * m.mask.cell + half), (m.mask.nx * m.mask.cell) / v.mpp, (m.mask.ny * m.mask.cell) / v.mpp);
    }
    ctx.imageSmoothingEnabled = true;

    // roads: a light casing under each line keeps them legible on any base.
    // Drawn as one path per class in world coordinates, so a dense network
    // costs a handful of strokes per frame instead of one per road.
    const onSatellite = baseLayer === "satellite";
    const far = v.mpp > 30;
    const k = 1 / v.mpp;
    ctx.save();
    ctx.setTransform(dpr * k, 0, 0, -dpr * k, dpr * (size.w / 2 - v.cx * k), dpr * (size.h / 2 + v.cy * k));
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    const px = (n: number) => n * v.mpp; // screen pixels to world metres
    for (const pass of ["casing", "line"] as const) {
      for (const cls of ["track", "gravel", "paved"]) {
        const path = roadPaths.by.get(cls);
        if (!path) continue;
        const paved = cls === "paved";
        if (far && cls === "track" && pass === "casing") continue;
        ctx.setLineDash(pass === "line" && cls === "track" ? [px(5), px(4)] : []);
        ctx.globalAlpha = far && cls === "track" ? 0.45 : pass === "casing" ? 0.85 : 1;
        ctx.strokeStyle = pass === "casing" ? (onSatellite ? "#000000" : palette.surface) : onSatellite ? "#f4f1e8" : palette.text2;
        ctx.lineWidth = px(pass === "casing" ? (paved ? 5 : 3.2) : paved ? 2.2 : cls === "gravel" ? 1.3 : 1.1);
        ctx.stroke(path);
      }
    }
    ctx.restore();
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
    ctx.fillStyle = onSatellite ? "#f4f1e8" : palette.text2;
    ctx.beginPath();
    for (const p of world.places) {
      ctx.moveTo(sx(p.x) + 2.5, sy(p.y));
      ctx.arc(sx(p.x), sy(p.y), 2.5, 0, Math.PI * 2);
    }
    ctx.fill();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [relief, roadPaths, risk, masks, satellite, imgReady, baseLayer, world, v.cx, v.cy, v.mpp, size, palette]);

  // ------------------------------------------------------------ interaction
  const pointerWorld = (ev: React.PointerEvent | React.WheelEvent) => {
    const rect = (ev.currentTarget as Element).getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    return { x, y, ...toWorld(x, y) };
  };

  const onPointerDown = (ev: React.PointerEvent<SVGSVGElement>) => {
    (ev.currentTarget as Element).setPointerCapture(ev.pointerId);
    const target = (ev.target as Element).closest("[data-drag]");
    if (target && !placing) {
      const kind = target.getAttribute("data-drag") as DragKind;
      const id = target.getAttribute("data-id") ?? "";
      const p = pointerWorld(ev);
      liveDrag.current = { kind, id, x: p.e, y: p.n, moved: false };
      setDrag(liveDrag.current);
      return;
    }
    pan.current = { x: ev.clientX, y: ev.clientY, view: v, moved: false };
  };

  const onPointerMove = (ev: React.PointerEvent<SVGSVGElement>) => {
    const p = pointerWorld(ev);
    // the readout is written straight to the DOM: re-rendering the map for
    // every mouse move was the main source of lag
    if (readout.current) readout.current.textContent = `${fmt(p.e / 1000, 2)} km E · ${fmt(p.n / 1000, 2)} km N · ${fmt(elevation(t, p.e, p.n), 0)} m`;
    const dragging = liveDrag.current;
    if (dragging) {
      liveDrag.current = { ...dragging, x: p.e, y: p.n, moved: true };
      commit();
      return;
    }
    const d = pan.current;
    if (d) {
      const dx = ev.clientX - d.x;
      const dy = ev.clientY - d.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) d.moved = true;
      if (d.moved) {
        touched.current = true;
        liveView.current = { ...d.view, cx: d.view.cx - dx * d.view.mpp, cy: d.view.cy + dy * d.view.mpp };
        commit();
      }
    }
  };

  const onPointerUp = (ev: React.PointerEvent<SVGSVGElement>) => {
    const dragging = liveDrag.current;
    if (dragging) {
      if (dragging.moved) props.onMove(dragging.kind, dragging.id, dragging.x, dragging.y);
      else if (dragging.kind === "request") props.onSelect(dragging.id);
      liveDrag.current = null;
      setDrag(null);
      return;
    }
    const d = pan.current;
    pan.current = null;
    if (d && !d.moved) {
      const p = pointerWorld(ev);
      if (placing) props.onPlace(p.e, p.n);
      else props.onSelect(null);
    }
  };

  const onWheel = (ev: React.WheelEvent<SVGSVGElement>) => {
    // zoom about the pointer, from the newest view so fast scrolling compounds
    const cur = liveView.current;
    const rect = ev.currentTarget.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    const e = cur.cx + (x - size.w / 2) * cur.mpp;
    const n = cur.cy - (y - size.h / 2) * cur.mpp;
    const mpp = Math.min(Math.max(cur.mpp * Math.exp(ev.deltaY * 0.0015), 1), (bx1 - bx0) / 200);
    touched.current = true;
    liveView.current = { mpp, cx: e - (x - size.w / 2) * mpp, cy: n + (y - size.h / 2) * mpp };
    commit();
  };

  const posOf = (kind: DragKind, id: string, x: number, y: number): Point =>
    drag && drag.kind === kind && drag.id === id ? [drag.x, drag.y] : [x, y];

  // ------------------------------------------------------------ data
  const byRequest = useMemo(() => new Map(plan?.assignments.map((a) => [a.request_id, a]) ?? []), [plan]);
  const selectedAssignment = selected ? byRequest.get(selected) : undefined;
  const scaleLen = niceStep(size.w * v.mpp, 8);
  const halo = { paintOrder: "stroke" as const, stroke: palette.surface, strokeWidth: 3, strokeLinejoin: "round" as const };
  const depot = posOf("depot", "depot", scenario.depot.x, scenario.depot.y);

  const strong = "600 11.5px system-ui, sans-serif";
  const regular = "11.5px system-ui, sans-serif";
  const labelInputs: LabelInput[] = [
    { key: "depot", text: scenario.depot.name, x: sx(depot[0]), y: sy(depot[1]), gap: 11, className: "pmap-label strong", font: strong },
    ...scenario.requests.map((r) => {
      const [x, y] = posOf("request", r.id, r.x, r.y);
      return { key: `r${r.id}`, text: r.name, x: sx(x), y: sy(y), gap: 15, className: "pmap-label strong", font: strong };
    }),
    ...scenario.risk.sites.map((q) => {
      const [x, y] = posOf("site", q.id, q.x, q.y);
      return { key: `s${q.id}`, text: q.name, x: sx(x), y: sy(y), gap: 10, className: "pmap-label critical", font: regular };
    }),
    ...scenario.risk.hazards.map((h) => {
      const cx = h.polygon.reduce((a, p) => a + p[0], 0) / h.polygon.length;
      const top = Math.max(...h.polygon.map((p) => p[1]));
      return {
        key: `h${h.id}`,
        text: h.impassable ? `${h.name} (impassable)` : h.name,
        x: sx(cx),
        y: sy(top) - 2,
        gap: 4,
        className: "pmap-label critical",
        font: regular,
      };
    }),
    // settlements: towns and cities always, villages once zoomed in
    ...world.places
      .filter((place) => place.kind === "city" || place.kind === "town" || v.mpp < 22)
      .map((place, k) => ({
      key: `p${k}`,
      text: place.name,
      x: sx(place.x),
      y: sy(place.y),
      gap: 6,
      className: "pmap-town",
      font: "11px system-ui, sans-serif",
      optional: true,
    })),
  ];
  const markerBox = (x: number, y: number, r: number) => ({ x0: x - r, y0: y - r, x1: x + r, y1: y + r });
  const obstacles = [
    markerBox(sx(depot[0]), sy(depot[1]), 9),
    ...scenario.requests.map((r) => {
      const [x, y] = posOf("request", r.id, r.x, r.y);
      return markerBox(sx(x), sy(y), 12);
    }),
    ...scenario.risk.sites.map((q) => {
      const [x, y] = posOf("site", q.id, q.x, q.y);
      return markerBox(sx(x), sy(y), 8);
    }),
    // keep clear of the map key, fit button and north arrow
    { x0: 0, y0: 0, x1: 364, y1: props.keyHeight + 14 },
    { x0: size.w - 120, y0: 0, x1: size.w, y1: 60 },
  ];
  const labels = placeLabels(labelInputs, obstacles, { w: size.w, h: size.h });
  const hint = placing ? "Click the map to place it · Esc to cancel" : "Drag to pan · scroll to zoom · drag markers to move them";

  return (
    <div className="pmap" ref={wrap} data-placing={placing ?? undefined}>
      <canvas ref={canvas} className="pmap-canvas" />
      <svg
        className="pmap-svg"
        width={size.w}
        height={size.h}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerLeave={() => {
          if (readout.current) readout.current.textContent = hint;
        }}
        onWheel={onWheel}
      >
        <defs>
          <pattern id="hatch" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <line x1="0" y1="0" x2="0" y2="6" stroke={palette.critical} strokeWidth="1.2" opacity="0.55" />
          </pattern>
          <pattern id="hatch-dense" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <line x1="0" y1="0" x2="0" y2="4" stroke={palette.critical} strokeWidth="1.6" opacity="0.7" />
          </pattern>
        </defs>

        {/* hazard areas */}
        {scenario.risk.hazards.map((h) => (
          <g key={h.id}>
            <polygon points={pts(h.polygon)} fill={`url(#${h.impassable ? "hatch-dense" : "hatch"})`} stroke={palette.critical} strokeWidth={1.4} />
          </g>
        ))}

        {/* routes of the plan */}
        {plan?.assignments.map((a) => {
          if (!a.mode) return null;
          const opt = a.options.find((o) => o.mode === a.mode);
          if (!opt || opt.route.length < 2) return null;
          const on = a.request_id === selected;
          const color = modeColor(palette, a.mode);
          const route = opt.route;
          return (
            <g key={a.request_id} opacity={selected && !on ? 0.45 : 1}>
              <polyline points={pts(route)} fill="none" stroke={palette.surface} strokeWidth={on ? 7 : 5} strokeLinejoin="round" strokeLinecap="round" />
              <polyline
                points={pts(route)}
                fill="none"
                stroke={color}
                strokeWidth={on ? 4 : 2.6}
                strokeLinejoin="round"
                strokeLinecap="round"
                strokeDasharray={a.mode === "airdrop" ? "1 7" : a.mode === "drone" ? "8 5" : undefined}
              />
              {a.mode === "airdrop" && opt.detail.type === "airdrop" && (
                <g stroke={color} strokeWidth={2.2}>
                  <line x1={sx(opt.detail.release_point[0]) - 5} y1={sy(opt.detail.release_point[1]) - 5} x2={sx(opt.detail.release_point[0]) + 5} y2={sy(opt.detail.release_point[1]) + 5} />
                  <line x1={sx(opt.detail.release_point[0]) - 5} y1={sy(opt.detail.release_point[1]) + 5} x2={sx(opt.detail.release_point[0]) + 5} y2={sy(opt.detail.release_point[1]) - 5} />
                </g>
              )}
            </g>
          );
        })}

        {/* the selected request's other options, for comparison */}
        {selectedAssignment?.options
          .filter((o) => o.feasible && o.mode !== selectedAssignment.mode && o.route.length > 1)
          .map((o) => (
            <polyline key={o.mode} points={pts(o.route)} fill="none" stroke={modeColor(palette, o.mode)} strokeWidth={1.6} strokeDasharray="2 4" opacity={0.9} />
          ))}

        {/* risk sites */}
        {scenario.risk.sites.map((s) => {
          const [x, y] = posOf("site", s.id, s.x, s.y);
          return (
            <g key={s.id} data-drag="site" data-id={s.id} className="pmap-handle">
              <circle cx={sx(x)} cy={sy(y)} r={s.range / v.mpp} fill="none" stroke={palette.critical} strokeWidth={1} strokeDasharray="3 4" opacity={0.8} />
              <path d={`M${sx(x)},${sy(y) - 7} L${sx(x) + 6.5},${sy(y) + 5} L${sx(x) - 6.5},${sy(y) + 5} Z`} fill={palette.critical} stroke={palette.surface} strokeWidth={1.5} />
            </g>
          );
        })}

        {/* depot */}
        <g data-drag="depot" data-id="depot" className="pmap-handle">
          <rect x={sx(depot[0]) - 7} y={sy(depot[1]) - 7} width={14} height={14} fill={palette.text} stroke={palette.surface} strokeWidth={2} />
        </g>

        {/* requests */}
        {scenario.requests.map((r) => {
          const [x, y] = posOf("request", r.id, r.x, r.y);
          const a = byRequest.get(r.id);
          const on = r.id === selected;
          const served = !!a?.mode;
          const late = a && served && !a.on_time;
          return (
            <g key={r.id} data-drag="request" data-id={r.id} className="pmap-handle">
              {on && <circle cx={sx(x)} cy={sy(y)} r={16} fill="none" stroke={palette.text} strokeWidth={2} />}
              <circle
                cx={sx(x)}
                cy={sy(y)}
                r={11}
                fill={served ? modeColor(palette, a!.mode) : palette.surface}
                stroke={served && !late ? palette.surface : palette.critical}
                strokeWidth={served && !late ? 2 : 2.5}
                strokeDasharray={late ? "3 2" : undefined}
              />
              <text x={sx(x)} y={sy(y) + 4} textAnchor="middle" className="pmap-id" fill={served ? INK_ON_MODE : palette.critical}>
                {r.id}
              </text>
            </g>
          );
        })}

        {/* labels, placed last so they never hide under markers */}
        <g pointerEvents="none">
          {labels.map((l) => (
            <text key={l.key} x={l.x} y={l.y} textAnchor={l.anchor} className={l.className} style={halo}>
              {l.text}
            </text>
          ))}
        </g>

        {/* north arrow and scale bar */}
        <g transform={`translate(${size.w - 28}, 28)`}>
          <path d="M0,-12 L6,6 L0,2 L-6,6 Z" fill={palette.text} />
          <text y={22} textAnchor="middle" className="pmap-label strong">
            N
          </text>
        </g>
        <g transform={`translate(${size.w - 24 - scaleLen / v.mpp}, 92)`}>
          <rect x={-6} y={-20} width={scaleLen / v.mpp + 12} height={28} fill={palette.surface} opacity={0.85} rx={3} />
          <line x1={0} x2={scaleLen / v.mpp} y1={0} y2={0} stroke={palette.text} strokeWidth={2} />
          <line x1={0} x2={0} y1={-4} y2={0} stroke={palette.text} strokeWidth={2} />
          <line x1={scaleLen / v.mpp} x2={scaleLen / v.mpp} y1={-4} y2={0} stroke={palette.text} strokeWidth={2} />
          <text x={scaleLen / v.mpp / 2} y={-7} textAnchor="middle" className="pmap-label">
            {scaleLen >= 1000 ? `${scaleLen / 1000} km` : `${scaleLen} m`}
          </text>
        </g>
      </svg>

      <div className="pmap-foot">
        <div className="pmap-readout num" ref={readout}>
          {hint}
        </div>
        <div className="pmap-attrib">
          {[...world.attribution, ...(baseLayer === "satellite" && imagery ? [imagery.attribution] : [])].join(" · ")}
        </div>
      </div>
      <button
        className="pmap-fit"
        onClick={() => {
          touched.current = false;
          setView(fit());
        }}
      >
        Fit
      </button>
    </div>
  );
}
