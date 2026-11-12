export type Side = 'l' | 'r' | 't' | 'b';
export interface Point { x: number; y: number }
export interface Rect { x: number; y: number; w: number; h: number }
export interface ChipItem { id: string; s: Point & { side: Side }; t: Point & { side: Side }; text: string }

export const FIT_PADDING = { top: '64px', right: '28px', bottom: '28px', left: '28px' } as const;

const CURVATURE = 0.25;
const CHIP_H = 20;
const TRIES = [0.5, 0.38, 0.62, 0.28, 0.72, 0.2, 0.8];

function offset(distance: number): number {
  return distance >= 0 ? 0.5 * distance : CURVATURE * 25 * Math.sqrt(-distance);
}

function control(p: Point, side: Side, other: Point): Point {
  if (side === 'l') return { x: p.x - offset(p.x - other.x), y: p.y };
  if (side === 'r') return { x: p.x + offset(other.x - p.x), y: p.y };
  if (side === 't') return { x: p.x, y: p.y - offset(p.y - other.y) };
  return { x: p.x, y: p.y + offset(other.y - p.y) };
}

export function bezierPoint(s: Point, sSide: Side, t: Point, tSide: Side, at: number): Point {
  const c1 = control(s, sSide, t);
  const c2 = control(t, tSide, s);
  const u = 1 - at;
  return {
    x: u * u * u * s.x + 3 * u * u * at * c1.x + 3 * u * at * at * c2.x + at * at * at * t.x,
    y: u * u * u * s.y + 3 * u * u * at * c1.y + 3 * u * at * at * c2.y + at * at * at * t.y,
  };
}

export function chipWidth(text: string): number {
  return text.length * 7 + 24;
}

function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

export function placeChips(items: ChipItem[], obstacles: Rect[]): Map<string, number | null> {
  const taken: Rect[] = [];
  const placed = new Map<string, number | null>();
  for (const item of items) {
    const w = chipWidth(item.text);
    let chosen: number | null = null;
    for (const at of TRIES) {
      const p = bezierPoint(item.s, item.s.side, item.t, item.t.side, at);
      const rect = { x: p.x - w / 2 - 3, y: p.y - CHIP_H / 2 - 2, w: w + 6, h: CHIP_H + 4 };
      if (taken.some((r) => overlaps(r, rect)) || obstacles.some((r) => overlaps(r, rect))) continue;
      taken.push(rect);
      chosen = at;
      break;
    }
    placed.set(item.id, chosen);
  }
  return placed;
}
