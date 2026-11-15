export type Side = 'l' | 'r' | 't' | 'b';
export interface Point { x: number; y: number }
export interface Rect { x: number; y: number; w: number; h: number }
export interface ChipItem { id: string; s: Point & { side: Side }; t: Point & { side: Side }; text: string }
export interface Port { id: string; side: Side; type: 'source' | 'target'; order: number }
export type Anchor = Point & { side: Side };

export const FIT_PADDING = { top: '28px', right: '28px', bottom: '28px', left: '28px' } as const;
export const READABLE_FIT = { padding: FIT_PADDING, minZoom: 1, maxZoom: 1.2 };

const CURVATURE = 0.25;
const CHIP_H = 20;
const TRIES = [0.5, 0.38, 0.62, 0.28, 0.72, 0.2, 0.8];
export const NEAR_SOURCE = [0.16, 0.24, 0.32, 0.1, 0.42, 0.5];

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

export function placeChips(items: ChipItem[], obstacles: Rect[], tries: number[] = TRIES): Map<string, number | null> {
  const taken: Rect[] = [];
  const placed = new Map<string, number | null>();
  for (const item of items) {
    const w = chipWidth(item.text);
    let chosen: number | null = null;
    for (const at of tries) {
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

export function sidesBetween(a: { column: number; y: number }, b: { column: number; y: number }, stacked = false): [Side, Side] {
  if (a.column !== b.column) return a.column < b.column ? ['r', 'l'] : ['l', 'r'];
  if (stacked) return a.y < b.y ? ['b', 't'] : ['t', 'b'];
  return ['r', 'r'];
}

export function addPort(ports: Map<string, Port[]>, node: string, port: Port): void {
  const list = ports.get(node);
  if (list) list.push(port);
  else ports.set(node, [port]);
}

export function anchors(ports: Map<string, Port[]>, rectOf: (node: string) => Rect): Map<string, Anchor> {
  const found = new Map<string, Anchor>();
  for (const [node, list] of ports) {
    list.sort((x, y) => x.order - y.order);
    const r = rectOf(node);
    for (const side of ['l', 'r', 't', 'b'] as const) {
      const at = list.filter((p) => p.side === side);
      at.forEach((port, i) => {
        const f = (i + 1) / (at.length + 1);
        const point = side === 'l' ? { x: r.x, y: r.y + r.h * f } : side === 'r' ? { x: r.x + r.w, y: r.y + r.h * f } : side === 't' ? { x: r.x + r.w * f, y: r.y } : { x: r.x + r.w * f, y: r.y + r.h };
        found.set(port.id, { ...point, side });
      });
    }
  }
  return found;
}
