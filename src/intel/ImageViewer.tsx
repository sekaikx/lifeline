import { useEffect, useRef, useState, type ReactNode } from "react";
import type { Finding } from "../ai/types";

const COLS = "ABCDEFGH";

export interface Marker {
  key: string;
  x: number;
  y: number;
  label: string;
  tone: "critical" | "good" | "accent" | "warn";
  active?: boolean;
}

export const findingTone = (f: Finding): Marker["tone"] =>
  ["flood", "damage", "blocked_route", "fire", "hazard"].includes(f.kind)
    ? "critical"
    : ["landing_zone", "drop_zone"].includes(f.kind)
      ? "good"
      : f.kind === "camp" || f.kind === "settlement"
        ? "accent"
        : "warn";

/**
 * A satellite image with pan and zoom, an optional A1-H8 reference grid and
 * markers in map metres. Panning moves a CSS transform, so it costs nothing
 * however large the image.
 */
export function ImageViewer({
  src,
  bounds,
  grid,
  markers,
  onMarker,
  children,
  overlay,
}: {
  src: string | null;
  /** west, south, east, north [m] */
  bounds: [number, number, number, number] | null;
  grid: boolean;
  markers: Marker[];
  onMarker?: (key: string) => void;
  children?: ReactNode;
  /** Second image drawn over the first, clipped at `overlay.split` (0-1) */
  overlay?: { src: string; split: number } | null;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [view, setView] = useState({ k: 1, x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number; vx: number; vy: number } | null>(null);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  // a new area starts fitted
  const boundsKey = bounds?.join(",") ?? "";
  useEffect(() => setView({ k: 1, x: 0, y: 0 }), [boundsKey]);

  const side = Math.max(Math.min(size.w, size.h) - 32, 100);
  const left = (size.w - side) / 2;
  const top = (size.h - side) / 2;
  const toPx = (x: number, y: number) => {
    if (!bounds) return [0, 0];
    const [w, s, e, n] = bounds;
    return [((x - w) / (e - w)) * side, ((n - y) / (n - s)) * side];
  };

  return (
    <div
      className="iv"
      ref={wrap}
      onPointerDown={(e) => {
        if ((e.target as Element).closest("[data-marker]")) return;
        (e.currentTarget as Element).setPointerCapture(e.pointerId);
        drag.current = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y };
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (d) setView((v) => ({ ...v, x: d.vx + e.clientX - d.x, y: d.vy + e.clientY - d.y }));
      }}
      onPointerUp={() => (drag.current = null)}
      onWheel={(e) => {
        const r = e.currentTarget.getBoundingClientRect();
        const px = e.clientX - r.left - left;
        const py = e.clientY - r.top - top;
        setView((v) => {
          const k = Math.min(Math.max(v.k * Math.exp(-e.deltaY * 0.0015), 1), 16);
          // keep the point under the cursor fixed
          return { k, x: px - ((px - v.x) * k) / v.k, y: py - ((py - v.y) * k) / v.k };
        });
      }}
      onDoubleClick={() => setView({ k: 1, x: 0, y: 0 })}
    >
      {src && bounds ? (
        <div
          className="iv-stage"
          style={{ left, top, width: side, height: side, transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})` }}
        >
          <img src={src} alt="" draggable={false} />
          {overlay && (
            <img
              src={overlay.src}
              alt=""
              draggable={false}
              style={{ clipPath: `inset(0 0 0 ${overlay.split * 100}%)` }}
            />
          )}
          <svg width={side} height={side} viewBox={`0 0 ${side} ${side}`}>
            {overlay && <line x1={overlay.split * side} x2={overlay.split * side} y1={0} y2={side} className="iv-split" style={{ strokeWidth: 2 / view.k }} />}
            {grid &&
              Array.from({ length: 7 }, (_, i) => (i + 1) * (side / 8)).map((p) => (
                <g key={p} className="iv-grid" style={{ strokeWidth: 1 / view.k }}>
                  <line x1={p} x2={p} y1={0} y2={side} />
                  <line y1={p} y2={p} x1={0} x2={side} />
                </g>
              ))}
            {grid &&
              Array.from({ length: 64 }, (_, k) => (
                <text
                  key={k}
                  x={(k % 8) * (side / 8) + 4 / view.k}
                  y={Math.floor(k / 8) * (side / 8) + 13 / view.k}
                  className="iv-cell"
                  style={{ fontSize: 11 / view.k }}
                >
                  {COLS[k % 8]}
                  {Math.floor(k / 8) + 1}
                </text>
              ))}
            {markers.map((m) => {
              const [x, y] = toPx(m.x, m.y);
              const r = (m.active ? 13 : 10) / view.k;
              return (
                <g key={m.key} data-marker className={`iv-marker ${m.tone}${m.active ? " active" : ""}`} onClick={() => onMarker?.(m.key)}>
                  <circle cx={x} cy={y} r={r} style={{ strokeWidth: 2 / view.k }} />
                  <text x={x} y={y + 4 / view.k} textAnchor="middle" style={{ fontSize: 11 / view.k }}>
                    {m.label}
                  </text>
                </g>
              );
            })}
          </svg>
        </div>
      ) : null}
      {children}
    </div>
  );
}
