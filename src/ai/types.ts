// Mirrors of crates/ai.

export type ProviderKind = "open_ai" | "anthropic";
export type Role = "chat" | "vision" | "fast";

export interface ModelRef {
  provider: string;
  model: string;
}

export interface PublicProvider {
  id: string;
  name: string;
  kind: ProviderKind;
  base_url: string;
  local: boolean;
  has_key: boolean;
  /** Sent only when changed: "" clears the stored key */
  new_key?: string | null;
}

export interface AiSettings {
  enabled: boolean;
  providers: PublicProvider[];
  chat: ModelRef | null;
  vision: ModelRef | null;
  fast: ModelRef | null;
  local_only: boolean;
  path?: string | null;
}

export interface Preset {
  id: string;
  name: string;
  kind: ProviderKind;
  base_url: string;
  local: boolean;
  needs_key: boolean;
  free: boolean;
  url: string;
  chat: string | null;
  vision: string | null;
  fast: string | null;
  note: string;
}

export interface ModelInfo {
  id: string;
  free: boolean | null;
  vision: boolean | null;
}

export type Part = { type: "text"; text: string } | { type: "image"; png_base64: string };

export interface Message {
  role: "user" | "assistant";
  content: Part[];
}

export interface Completion {
  text: string;
  provider: string;
  model: string;
  local: boolean;
  ms: number;
}

export interface PlaceRef {
  name: string;
  x: number;
  y: number;
}

export interface CatalogueRef {
  id: string;
  name: string;
  unit: string;
  unit_kg: number;
}

export interface ParsedItem {
  item: string | null;
  name: string;
  quantity: number;
  unit: string;
  unit_kg: number | null;
  kg: number | null;
}

export interface ParsedRequest {
  name: string;
  place: string | null;
  x: number | null;
  y: number | null;
  demand_kg: number;
  priority: "critical" | "high" | "routine";
  due_h: number;
  mode: "auto" | "truck" | "drone" | "airdrop";
  items: ParsedItem[];
  notes: string;
  model: string;
}

export interface Finding {
  kind: string;
  cell: string;
  x: number;
  y: number;
  note: string;
  confidence: "low" | "medium" | "high";
}

export interface AreaAssessment {
  summary: string;
  findings: Finding[];
  recommendations: string[];
  model: string;
  provider: string;
  local: boolean;
  grid_png_base64: string;
}
