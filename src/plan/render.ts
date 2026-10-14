// Raster rendering for the planning map: shaded relief with contours, and
// the risk overlay. Both are drawn once into offscreen canvases and then
// scaled onto the map, so panning and zooming stay cheap.

import type { Raster, TerrainGrid } from "./types";

const hex = (c: string): [number, number, number] => {
  const v = parseInt(c.slice(1), 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
};

const mix = (a: [number, number, number], b: [number, number, number], t: number) =>
  [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t] as const;

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
  low: string;
  high: string;
  contour: string;
  /** Contour interval [m]; every fifth line is drawn heavier */
  interval: number;
}

export const reliefStyle = (mode: "light" | "dark"): ReliefStyle =>
  mode === "light"
    ? { low: "#eef0e8", high: "#d6cdb8", contour: "#8c8371", interval: 25 }
    : { low: "#1c1f1b", high: "#3a362e", contour: "#a59c86", interval: 25 };

/**
 * Shaded relief: elevation tint, multiplied by Lambertian hillshade from the
 * north-west (the cartographic convention), with contour lines where the
 * contour index changes between neighbouring pixels.
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

  const low = hex(style.low);
  const high = hex(style.high);
  const ink = hex(style.contour);
  const cell = t.dx / scale;
  // light from the north-west, 45 degrees up
  const [lx, ly, lz] = [-Math.SQRT1_2 * Math.SQRT1_2, Math.SQRT1_2 * Math.SQRT1_2, Math.SQRT1_2];
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
      const shade = Math.max(0, (-dzdx * lx - dzdy * ly + lz) / n);
      const lambert = 0.55 + 0.45 * Math.min(1, shade / lz);

      const [r, g, b] = mix(low, high, (z[k] - lo) / (hi - lo || 1));
      let col = [r * lambert, g * lambert, b * lambert];

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
