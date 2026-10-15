export const fmt = (v: number | null | undefined, digits = 1): string =>
  v == null || !Number.isFinite(v) ? "—" : v.toFixed(digits);

export const fmtSci = (v: number | null | undefined): string =>
  v == null || !Number.isFinite(v)
    ? "—"
    : v === 0
      ? "0"
      : Math.abs(v) >= 0.01 && Math.abs(v) < 1e4
        ? v.toPrecision(3)
        : v.toExponential(2);

// `+ 0` turns -0 (e.g. a sum over nothing) into 0, so it never prints as "-0"
export const fmtInt = (v: number): string => (Math.round(v) + 0).toLocaleString("en-US");

/** Compass bearing (deg true) of an ENU vector. */
export const bearing = (east: number, north: number): number =>
  ((Math.atan2(east, north) * 180) / Math.PI + 360) % 360;

/** Direction the wind blows FROM, for an ENU wind velocity. */
export const windFrom = (east: number, north: number): number => (bearing(east, north) + 180) % 360;

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Linear interpolation of the drop path at time t. */
export function samplePath<T extends { t: number }>(
  path: T[],
  t: number,
  lerp: (a: T, b: T, w: number) => T,
): T {
  if (t <= path[0].t) return path[0];
  const last = path[path.length - 1];
  if (t >= last.t) return last;
  let lo = 0;
  let hi = path.length - 1;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (path[mid].t <= t) lo = mid;
    else hi = mid;
  }
  const a = path[lo];
  const b = path[hi];
  return lerp(a, b, (t - a.t) / (b.t - a.t));
}

/** "Nice" grid spacing for a span in metres. */
export function niceStep(span: number, targetLines = 8): number {
  const raw = span / targetLines;
  const pow = 10 ** Math.floor(Math.log10(raw));
  for (const m of [1, 2, 2.5, 5, 10]) if (m * pow >= raw) return m * pow;
  return 10 * pow;
}

/** Hours as `h:mm`, rounded to the minute. */
export const fmtClock = (h: number): string => {
  if (!Number.isFinite(h)) return "—";
  const minutes = Math.round(h * 60);
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, "0")}`;
};

export const fmtKg = (kg: number): string =>
  kg >= 1000 ? `${(kg / 1000).toFixed(kg >= 100_000 ? 0 : 1)} t` : `${Math.round(kg)} kg`;

export const fmtPct = (p: number, digits = 0): string => `${(p * 100).toFixed(digits)}%`;

export const fmtMoney = (v: number): string => (Math.round(v) + 0).toLocaleString("en-US");
