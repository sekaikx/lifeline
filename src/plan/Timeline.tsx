import { useEffect, useRef, useState } from "react";
import { fmtClock } from "../lib/format";
import type { Palette } from "../lib/theme";
import { modeColor } from "./MapView";
import type { Plan, PlanScenario, Trip } from "./types";

const ROW = 22;
const LABEL = 84;
const AXIS = 22;

/** One row per vehicle; the outbound leg is solid, the return leg faint. */
export function Timeline({
  plan,
  scenario,
  palette,
  selected,
  onSelect,
}: {
  plan: Plan;
  scenario: PlanScenario;
  palette: Palette;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(600);
  const [hover, setHover] = useState<{ trip: Trip; unit: string; x: number; y: number } | null>(null);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const rows = plan.schedule;
  const due = selected ? scenario.requests.find((r) => r.id === selected)?.due_h : undefined;
  const end = Math.max(1, ...rows.flatMap((u) => u.trips.map((t) => t.ready_h)), due ?? 0);
  const step = end <= 3 ? 0.5 : end <= 8 ? 1 : end <= 16 ? 2 : 4;
  const span = Math.ceil(end / step) * step;
  const plot = Math.max(100, width - LABEL - 12);
  const x = (h: number) => LABEL + (h / span) * plot;
  const height = AXIS + rows.length * ROW + 4;
  const ticks = Array.from({ length: Math.round(span / step) + 1 }, (_, i) => i * step);
  const name = (id: string) => scenario.requests.find((r) => r.id === id)?.name ?? id;

  return (
    <div className="timeline" ref={wrap}>
      <svg width={width} height={height}>
        {ticks.map((h) => (
          <g key={h}>
            <line x1={x(h)} x2={x(h)} y1={AXIS - 4} y2={height} stroke={palette.grid} />
            <text x={x(h)} y={12} textAnchor="middle" className="tl-tick">
              {fmtClock(h)}
            </text>
          </g>
        ))}
        {rows.map((u, i) => {
          const y = AXIS + i * ROW;
          const color = modeColor(palette, u.mode);
          return (
            <g key={u.unit}>
              <text x={0} y={y + 15} className="tl-unit">
                {u.unit}
              </text>
              <line x1={LABEL} x2={LABEL + plot} y1={y + ROW - 0.5} y2={y + ROW - 0.5} stroke={palette.grid} />
              {u.trips.map((t, k) => {
                const dim = selected && t.request_id !== selected;
                const w = Math.max(2, x(t.arrive_h) - x(t.depart_h));
                return (
                  <g
                    key={k}
                    opacity={dim ? 0.3 : 1}
                    onClick={() => onSelect(t.request_id)}
                    onPointerEnter={(e) => setHover({ trip: t, unit: u.unit, x: e.nativeEvent.offsetX, y: y })}
                    onPointerLeave={() => setHover(null)}
                    style={{ cursor: "pointer" }}
                  >
                    <rect x={x(t.arrive_h)} y={y + 5} width={Math.max(0, x(t.ready_h) - x(t.arrive_h))} height={12} fill={color} opacity={0.22} />
                    <rect x={x(t.depart_h)} y={y + 4} width={w} height={14} rx={2} fill={color} />
                    {w > 26 && (
                      <text x={x(t.depart_h) + 5} y={y + 15} className="tl-bar">
                        {t.request_id}
                      </text>
                    )}
                  </g>
                );
              })}
            </g>
          );
        })}
        {due !== undefined && (
          <g>
            <line x1={x(due)} x2={x(due)} y1={AXIS - 4} y2={height} stroke={palette.critical} strokeDasharray="4 3" strokeWidth={1.5} />
            <text x={x(due) + 4} y={AXIS + 8} className="tl-due">
              {selected} due
            </text>
          </g>
        )}
      </svg>
      {hover && (
        <div className="tooltip" style={{ left: Math.min(hover.x + 12, width - 220), top: hover.y + ROW + 4 }}>
          <strong>{hover.unit}</strong> · {hover.trip.request_id} {name(hover.trip.request_id)}
          <br />
          departs {fmtClock(hover.trip.depart_h)}, arrives {fmtClock(hover.trip.arrive_h)}, back {fmtClock(hover.trip.ready_h)}
          <br />
          {Math.round(hover.trip.load_kg).toLocaleString("en-US")} kg
        </div>
      )}
    </div>
  );
}
