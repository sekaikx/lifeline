// Derive truck access and staging advice from an existing plan.
// Flood already closes roads via impassable masks; this only reads planner options.

import type { Assignment, Mode, ModeOption, Plan, PlanScenario, Priority } from "./types";

export interface StagingRow {
  request_id: string;
  name: string;
  priority: Priority;
  truck_reason: string | null;
  /** True when this request had a truck route before the last flood check. */
  cut_by_flood: boolean;
  recommend: Mode | null;
}

export function modeOption(a: Assignment, mode: Mode): ModeOption | undefined {
  return a.options.find((o) => o.mode === mode);
}

export function truckBlocked(a: Assignment): boolean {
  const t = modeOption(a, "truck");
  return !!t && !t.feasible;
}

export function recommendAlternate(a: Assignment): Mode | null {
  for (const m of ["drone", "airdrop"] as const) {
    const o = modeOption(a, m);
    if (o?.feasible) return m;
  }
  return null;
}

export function hasImpassableMask(scenario: PlanScenario): boolean {
  return scenario.risk.masks.some((m) => m.impassable);
}

/** Snapshot of whether each request currently has a feasible truck option. */
export function truckFeasibility(plan: Plan | null): Record<string, boolean> {
  const out: Record<string, boolean> = {};
  if (!plan) return out;
  for (const a of plan.assignments) {
    const t = modeOption(a, "truck");
    out[a.request_id] = !!t?.feasible;
  }
  return out;
}

const PRIORITY_ORDER: Record<Priority, number> = { critical: 0, high: 1, routine: 2 };

const MODE_WORD: Record<Mode, string> = { truck: "truck", drone: "drone", airdrop: "aircraft" };

export function stagingSuggestions(
  plan: Plan | null,
  scenario: PlanScenario,
  preFloodTruck: Record<string, boolean> | null = null,
): StagingRow[] {
  if (!plan) return [];
  const rows: StagingRow[] = [];
  for (const a of plan.assignments) {
    if (!truckBlocked(a)) continue;
    const req = scenario.requests.find((r) => r.id === a.request_id);
    if (!req) continue;
    const truck = modeOption(a, "truck");
    rows.push({
      request_id: a.request_id,
      name: req.name,
      priority: req.priority,
      truck_reason: truck?.reason ?? null,
      cut_by_flood: preFloodTruck?.[a.request_id] === true,
      recommend: recommendAlternate(a),
    });
  }
  rows.sort(
    (a, b) => Number(b.cut_by_flood) - Number(a.cut_by_flood) || PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority] || a.request_id.localeCompare(b.request_id),
  );
  return rows;
}

/** One-line note for the flood controls / status area. */
export function accessNote(rows: StagingRow[]): string | null {
  if (!rows.length) return null;
  const cut = rows.filter((r) => r.cut_by_flood);
  const focus = cut.length ? cut : rows;
  const n = focus.length;
  const names = focus.slice(0, 3).map((r) => r.name);
  const more = n > 3 ? ` and ${n - 3} more` : "";
  const alts = [...new Set(focus.map((r) => r.recommend).filter((m): m is Mode => m != null))].map((m) => MODE_WORD[m]);
  const verb = cut.length ? "lost truck access after the flood check" : "have no truck route from the depot";
  const tip = alts.length ? ` Prefer ${alts.join(" / ")} for staging.` : " No alternate mode is feasible yet.";
  return `${n} request${n === 1 ? "" : "s"} ${verb} (${names.join(", ")}${more}).${tip}`;
}
