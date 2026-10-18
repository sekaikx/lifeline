// Raster rendering for the planning map: shaded relief with contours, and
// the risk overlay. Both are drawn once into offscreen canvases and then
// scaled onto the map, so panning and zooming stay cheap.

import type { Raster, TerrainGrid } from "./types";

const hex = (c: string): [number, number, number] => {
  const v = parseInt(c.slice(1), 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
};

/** Piecewise-linear interpolation along a colour ramp, t clamped to 0..1. */
function rampColor(stops: [number, [number, number, number]][], t: number): [number, number, number] {
  const x = Math.min(Math.max(t, 0), 1);
  for (let i = 1; i < stops.length; i++) {
    const [tp, p] = stops[i];
    const [tn, n] = stops[i - 1];
    if (x <= tp) return [n[0] + (p[0] - n[0]) * ((x - tn) / (tp - tn || 1)), n[1] + (p[1] - n[1]) * ((x - tn) / (tp - tn || 1)), n[2] + (p[2] - n[2]) * ((x - tn) / (tp - tn || 1))];
  }
  return stops[stops.length - 1][1];
}

/** Bilinear height at fractional grid coordinates. */
function heightAt(t: TerrainGrid, fx: number, fy: number): number {
  const i = Math.min(Math.max(Math.floor(fx), 0), t.nx - 2);
  const j = Math.min(Math.max(Math.floor(fy), 0), t.ny - 2);
  const tx = Math.min(Math.max(fx - i, 0), 1);
  const ty = Math.min(Math.max(fy - j, 0), 1);
  const h = (a: number, b: number) => t.heights[b * t.nx + a];
  return (h(i, j) * (1 - tx) + h(i + 1, j) * tx) * (1 - ty) + (h(i, j + 1) * (1 - tx) + h(i + 1, j + 1) * tx) * ty;
}

export interface ReliefStyle {
  /** Hypsometric stops: [elevation fraction colour] from low ground to peaks */
  stops: [number, string][];
  contour: string;
  /** Contour interval [m]; every fifth line is drawn heavier */
  interval: number;
}

/** Valley floor to ridgeline, in one blended cartographic ramp per theme. */
export const reliefStyle = (mode: "light" | "dark"): ReliefStyle =>
  mode === "light"
    ? {
        stops: [
          [0.0, "#456b52"],
          [0.2, "#6d7d52"],
          [0.4, "#a08f5e"],
          [0.6, "#c0a877"],
          [0.8, "#a3907a"],
          [1.0, "#e9e6dc"],
        ],
        contour: "#756a55",
        interval: 25,
      }
    : {
        stops: [
          [0.0, "#131e26"],
          [0.2, "#1b333c"],
          [0.4, "#2f4f4f"],
          [0.6, "#54685c"],
          [0.8, "#7d8474"],
          [1.0, "#c3bfae"],
        ],
        contour: "#93a89a",
        interval: 25,
      };

/** The same ramp as a CSS gradient, for the small legend bar. */
export function hypsometricGradient(mode: "light" | "dark"): { css: string; n: number } {
  const s = reliefStyle(mode).stops;
  return {
    css: `linear-gradient(to right, ${s.map(([p, c]) => `${c} ${Math.round(p * 100)}%`).join(", ")})`,
    n: s.length,
  };
}

/**
 * Shaded relief: hypsometric tint over a two-light hillshade (north-west key
 * light per cartographic convention, with a soft east fill so lee slopes read
 * instead of vanishing), with contour lines where the contour index changes
 * between neighbouring pixels.
 */
export function renderRelief(t: TerrainGrid, style: ReliefStyle, scale = 4): HTMLCanvasElement {
  const w = (t.nx - 1) * scale + 1;
  const h = (t.ny - 1) * scale + 1;
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d")!;
  const img = ctx.createImageData(w, h);

  const z = new Float32Array(w * h);
  let lo = Infinity;
  let hi = -Infinity;
  for (let py = 0; py < h; py++)
    for (let px = 0; px < w; px++) {
      // canvas rows run north to south; grid rows run south to north
      const v = heightAt(t, px / scale, (h - 1 - py) / scale);
      z[py * w + px] = v;
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
  const span = hi - lo || 1;
  const stops: [number, [number, number, number]][] = style.stops.map(([p, c]) => [p, hex(c)]);
  const ink = hex(style.contour);
  const cell = t.dx / scale;
  // key light north-west at 45 degrees, fill from the east, both 45 degrees up
  const [kx, ky, kz] = [-0.5, 0.5, Math.SQRT1_2];
  const [fx, fy, fz] = [1, 0, Math.SQRT1_2];
  const band = (v: number, step: number) => Math.floor(v / step);

  for (let py = 0; py < h; py++)
    for (let px = 0; px < w; px++) {
      const k = py * w + px;
      const zl = z[py * w + Math.max(px - 1, 0)];
      const zr = z[py * w + Math.min(px + 1, w - 1)];
      const zu = z[Math.max(py - 1, 0) * w + px];
      const zd = z[Math.min(py + 1, h - 1) * w + px];
      // surface normal in (east, north, up); canvas y grows southwards
      const dzdx = (zr - zl) / (2 * cell);
      const dzdy = (zu - zd) / (2 * cell);
      const n = Math.hypot(dzdx, dzdy, 1);
      const key = Math.max(0, (-dzdx * kx - dzdy * ky + kz) / n);
      const fill = Math.max(0, (-dzdx * fx - dzdy * fy + fz) / n);
      const lambert = 0.5 + 0.5 * Math.min(1, (0.72 * (key / kz) + 0.28 * (fill / fz)) / 1.001);

      let col = rampColor(stops, (z[k] - lo) / span);
      col = [col[0] * lambert, col[1] * lambert, col[2] * lambert];

      // contours: pixel sits on a line if a neighbour is in a lower band
      const c = band(z[k], style.interval);
      const isLine = c > band(zr, style.interval) || c > band(zd, style.interval) || c > band(zl, style.interval) || c > band(zu, style.interval);
      if (isLine) {
        const index = c % 5 === 0;
        const a = index ? 0.45 : 0.18;
        col = [col[0] + (ink[0] - col[0]) * a, col[1] + (ink[1] - col[1]) * a, col[2] + (ink[2] - col[2]) * a];
      }
      img.data[k * 4] = col[0];
      img.data[k * 4 + 1] = col[1];
      img.data[k * 4 + 2] = col[2];
      img.data[k * 4 + 3] = 255;
    }
  ctx.putImageData(img, 0, 0);
  return canvas;
}

/** Risk overlay: alpha grows with the hazard rate, saturating at `full` [1/s]. */
export function renderRisk(r: Raster, color: string, full = 0.0007): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = r.nx;
  canvas.height = r.ny;
  const ctx = canvas.getContext("2d")!;
  const img = ctx.createImageData(r.nx, r.ny);
  const [cr, cg, cb] = hex(color);
  for (let j = 0; j < r.ny; j++)
    for (let i = 0; i < r.nx; i++) {
      const v = r.values[j * r.nx + i];
      const k = ((r.ny - 1 - j) * r.nx + i) * 4;
      img.data[k] = cr;
      img.data[k + 1] = cg;
      img.data[k + 2] = cb;
      img.data[k + 3] = v > 0 ? 40 + 110 * Math.min(1, v / full) : 0;
    }
  ctx.putImageData(img, 0, 0);
  return canvas;
}

/** Terrain height at a map point, for the cursor readout. */
export function elevation(t: TerrainGrid, x: number, y: number): number {
  return heightAt(t, (x - t.x0) / t.dx, (y - t.y0) / t.dy);
}
