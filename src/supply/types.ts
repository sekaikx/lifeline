// Mirrors of crates/supply.

import type { Mode } from "../plan/types";

export type SupplyClass = "i" | "ii" | "iii" | "iv" | "v" | "vi" | "vii" | "viii" | "ix" | "x";

export const CLASSES: { value: SupplyClass; label: string }[] = [
  { value: "i", label: "I · Food and water" },
  { value: "ii", label: "II · Equipment and kits" },
  { value: "iii", label: "III · Fuel" },
  { value: "iv", label: "IV · Construction" },
  { value: "v", label: "V · Ammunition" },
  { value: "vi", label: "VI · Personal" },
  { value: "vii", label: "VII · Major items" },
  { value: "viii", label: "VIII · Medical" },
  { value: "ix", label: "IX · Repair parts" },
  { value: "x", label: "X · Civil programmes" },
];

export const classLabel = (c: SupplyClass) => CLASSES.find((x) => x.value === c)?.label ?? c.toUpperCase();
export const classNumeral = (c: SupplyClass) => c.toUpperCase();

export interface ColdChain {
  min_c: number;
  max_c: number;
}

export interface Item {
  id: string;
  name: string;
  class: SupplyClass;
  unit: string;
  unit_kg: number;
  unit_m3: number | null;
  cold_chain: ColdChain | null;
  max_transit_h: number | null;
  hazmat: string | null;
  controlled: boolean;
  reorder_level: number;
  notes: string;
}

export type LocationKind = "depot" | "warehouse" | "forward_point" | "facility";

export interface Location {
  id: string;
  name: string;
  kind: LocationKind;
}

export interface StockLine {
  item: string;
  lot: string;
  expiry: string | null;
  location: string;
  available: number;
  held: number;
}

export type Alert =
  | { kind: "low"; item: string; available: number; reorder_level: number }
  | { kind: "expiring"; item: string; lot: string; location: string; expiry: string; days: number; quantity: number }
  | { kind: "expired"; item: string; lot: string; location: string; expiry: string; quantity: number };

export type Action = "receive" | "issue" | "transfer" | "adjust" | "hold" | "release" | "dispose" | "dispose_held";

export const ACTIONS: { value: Action; label: string; help: string }[] = [
  { value: "receive", label: "Receive", help: "Goods in to a location" },
  { value: "issue", label: "Issue", help: "Goods out to a request, unit or facility" },
  { value: "transfer", label: "Transfer", help: "Between two locations" },
  { value: "adjust", label: "Count", help: "Correct the count after a stock check (negative to reduce)" },
  { value: "hold", label: "Hold", help: "Quarantine: damaged, recalled, awaiting inspection" },
  { value: "release", label: "Release", help: "Back from hold to available" },
  { value: "dispose", label: "Write off", help: "Expired or destroyed available stock" },
  { value: "dispose_held", label: "Write off held", help: "Destroy stock that is on hold" },
];

export interface TxDraft {
  action: Action;
  item: string;
  lot: string;
  expiry: string | null;
  from: string | null;
  to: string | null;
  qty: number;
  reference: string | null;
  user: string;
  witness: string | null;
  note: string;
}

export interface Tx extends TxDraft {
  seq: number;
  ts: number;
  prev: string;
  hash: string;
}

export interface Snapshot {
  items: Item[];
  locations: Location[];
  stock: StockLine[];
  alerts: Alert[];
  recent: Tx[];
  ledger_len: number;
  integrity: string | null;
}

export interface ImportReport {
  rows: number;
  received: number;
  new_items: string[];
  new_locations: string[];
  errors: string[];
}

export interface Pick {
  request_id: string;
  item: string;
  lot: string;
  expiry: string | null;
  location: string;
  qty: number;
  kg: number;
}

export interface Shortfall {
  request_id: string;
  item: string;
  needed: number;
  picked: number;
  reason: string;
}

export interface TransitBreach {
  request_id: string;
  item: string;
  transit_h: number;
  limit_h: number;
}

export interface ManifestLine {
  item: string;
  name: string;
  lot: string;
  expiry: string | null;
  qty: number;
  unit: string;
  kg: number;
}

export interface Manifest {
  unit: string;
  mode: Mode;
  trip: number;
  depart_h: number;
  arrive_h: number;
  request_id: string;
  destination: string;
  lines: ManifestLine[];
  kg: number;
  cold_chain: boolean;
  hazmat: string[];
  controlled: boolean;
}

export interface Allocation {
  picks: Pick[];
  shortfalls: Shortfall[];
  transit_breaches: TransitBreach[];
  manifests: Manifest[];
  unserved: string[];
}

/** Local calendar date as YYYY-MM-DD. */
export const todayIso = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
