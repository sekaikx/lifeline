import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import type { Palette } from "../lib/theme";

export interface SeriesSpec {
  label: string;
  color: string;
  dash?: number[];
  width?: number;
  points?: boolean;
  /** for log-scale charts: hide points that are null */
  spanGaps?: boolean;
}

interface Props {
  data: uPlot.AlignedData;
  series: SeriesSpec[];
  palette: Palette;
  xLabel: string;
  yLabel: string;
  logX?: boolean;
  logY?: boolean;
  height?: number;
  xFormat?: (v: number) => string;
  yFormat?: (v: number) => string;
}

/** Gridlines at decades only (plus 2 and 5 when the range spans few decades). */
const logSplits = (_u: uPlot, _axis: number, min: number, max: number): number[] => {
  const lo = Math.floor(Math.log10(min));
  const hi = Math.ceil(Math.log10(max));
  const mults = hi - lo <= 3 ? [1, 2, 5] : [1];
  const out: number[] = [];
  for (let e = lo; e <= hi; e++)
    for (const m of mults) {
      const v = m * 10 ** e;
      if (v >= min * 0.999 && v <= max * 1.001) out.push(v);
    }
  return out.length >= 2 ? out : [min, max];
};

const logLabel = (v: number): string =>
  v >= 0.01 && v < 1e5 ? String(+v.toPrecision(3)) : v.toExponential(0).replace("+", "");

/** Thin React wrapper around uPlot: fast canvas charts with a crosshair cursor. */
export function UChart({
  data,
  series,
  palette,
  xLabel,
  yLabel,
  logX,
  logY,
  height = 260,
  xFormat,
  yFormat,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const axis = (label: string, fmt: ((v: number) => string) | undefined, log: boolean | undefined, y: boolean): uPlot.Axis => ({
      label,
      size: y ? 64 : 50,
      splits: log ? logSplits : undefined,
      stroke: palette.text2,
      grid: { stroke: palette.grid, width: 1 },
      ticks: { stroke: palette.grid, width: 1 },
      labelFont: "12px system-ui",
      font: "11px system-ui",
      values: log
        ? (_u, vals) => vals.map((v) => (v == null ? "" : logLabel(v)))
        : fmt
          ? (_u, vals) => vals.map((v) => (v == null ? "" : fmt(v)))
          : undefined,
    });
    const opts: uPlot.Options = {
      width: el.clientWidth || 400,
      height,
      scales: {
        x: { time: false, distr: logX ? 3 : 1 },
        y: { distr: logY ? 3 : 1 },
      },
      axes: [axis(xLabel, xFormat, logX, false), axis(yLabel, yFormat, logY, true)],
      cursor: { drag: { x: true, y: true, uni: 20 }, points: { size: 8 } },
      legend: { live: true },
      series: [
        { label: xLabel, value: (_u, v) => (v == null ? "—" : xFormat ? xFormat(v) : String(+v.toPrecision(5))) },
        ...series.map(
          (s): uPlot.Series => ({
            label: s.label,
            stroke: s.color,
            width: s.width ?? 2,
            dash: s.dash,
            spanGaps: s.spanGaps ?? false,
            points: { show: s.points ?? false, size: 7, fill: s.color, stroke: palette.surface, width: 2 },
            value: (_u, v) => (v == null ? "—" : yFormat ? yFormat(v) : String(+v.toPrecision(4))),
          }),
        ),
      ],
    };
    const p = new uPlot(opts, data, el);
    plot.current = p;
    const ro = new ResizeObserver(() => p.setSize({ width: el.clientWidth, height }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      p.destroy();
      plot.current = null;
    };
    // Rebuild when structure or theme changes; data updates go through setData below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [palette, height, logX, logY, xLabel, yLabel, series.map((s) => s.label + s.color).join("|")]);

  useEffect(() => {
    plot.current?.setData(data);
  }, [data]);

  return <div ref={host} className="chart" />;
}
