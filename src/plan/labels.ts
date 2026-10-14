// Greedy map label placement. Labels are placed in priority order; each
// tries a few positions around its anchor and takes the first that overlaps
// neither an already placed label nor a marker. Labels marked `optional`
// (town names) are dropped when nothing fits; the rest fall back to the
// right-hand position.

export interface LabelInput {
  key: string;
  text: string;
  x: number;
  y: number;
  /** Clearance from the anchor to the label [px] */
  gap: number;
  className: string;
  font: string;
  optional?: boolean;
}

export interface PlacedLabel {
  key: string;
  text: string;
  x: number;
  y: number;
  anchor: "start" | "middle" | "end";
  className: string;
}

interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

const overlaps = (a: Box, b: Box) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;

let measureCtx: CanvasRenderingContext2D | null = null;
const widths = new Map<string, number>();

function textWidth(text: string, font: string): number {
  const k = `${font}|${text}`;
  const cached = widths.get(k);
  if (cached !== undefined) return cached;
  measureCtx ??= document.createElement("canvas").getContext("2d");
  let w = text.length * 6.5;
  if (measureCtx) {
    measureCtx.font = font;
    w = measureCtx.measureText(text).width;
  }
  widths.set(k, w);
  return w;
}

export function placeLabels(labels: LabelInput[], obstacles: Box[], bounds: { w: number; h: number }): PlacedLabel[] {
  const placed: Box[] = [];
  const out: PlacedLabel[] = [];
  const H = 14;
  for (const l of labels) {
    const w = textWidth(l.text, l.font) + 2;
    const candidates: { box: Box; x: number; y: number; anchor: PlacedLabel["anchor"] }[] = [
      { box: { x0: l.x + l.gap, y0: l.y - H / 2, x1: l.x + l.gap + w, y1: l.y + H / 2 }, x: l.x + l.gap, y: l.y + 4, anchor: "start" },
      { box: { x0: l.x - l.gap - w, y0: l.y - H / 2, x1: l.x - l.gap, y1: l.y + H / 2 }, x: l.x - l.gap, y: l.y + 4, anchor: "end" },
      { box: { x0: l.x - w / 2, y0: l.y - l.gap - H, x1: l.x + w / 2, y1: l.y - l.gap }, x: l.x, y: l.y - l.gap - 3, anchor: "middle" },
      { box: { x0: l.x - w / 2, y0: l.y + l.gap, x1: l.x + w / 2, y1: l.y + l.gap + H }, x: l.x, y: l.y + l.gap + 11, anchor: "middle" },
    ];
    const inView = (b: Box) => b.x0 >= 0 && b.y0 >= 0 && b.x1 <= bounds.w && b.y1 <= bounds.h;
    const free = candidates.find((c) => inView(c.box) && !placed.some((p) => overlaps(p, c.box)) && !obstacles.some((o) => overlaps(o, c.box)));
    const chosen = free ?? (l.optional ? undefined : candidates[0]);
    if (!chosen) continue;
    placed.push(chosen.box);
    out.push({ key: l.key, text: l.text, x: chosen.x, y: chosen.y, anchor: chosen.anchor, className: l.className });
  }
  return out;
}
