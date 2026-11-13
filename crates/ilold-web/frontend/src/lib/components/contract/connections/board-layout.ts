import type { BoardEdge, BoardNode, BoardRow, Built } from './board-links';

export const OPEN_W = 248;
export const FOLD_W = 178;
export const STRIP_W = 40;
const MARGIN = 20;
const BAND = 26;

export interface Placed {
  id: string;
  ref: BoardNode;
  col: number;
  x: number;
  y: number;
  w: number;
  h: number;
  head: number;
  rh: number;
  rows: { cy: number; row: BoardRow }[];
  rowAt: Map<string, number>;
}

export interface Band {
  owner: string;
  roles: Set<'caller' | 'callee'>;
  x: number;
  y: number;
  w: number;
  h: number;
  col: number;
}

export interface Geometry {
  nodes: Placed[];
  bands: Band[];
  gap: number;
  W: number;
  H: number;
}

const avg = (a: number[]) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : 1e3);

export function arrange(B: Built, openOrder: string[]): { k: number; list: BoardNode[] }[] {
  const cols = new Map<number, BoardNode[]>();
  for (const n of B.nodes) (cols.get(n.col) ?? cols.set(n.col, []).get(n.col)!).push(n);
  const keys = [...cols.keys()].sort((a, b) => a - b);
  const es = B.edges.filter((e) => !e.self);
  const pos = new Map<string, number>();
  const recalc = () =>
    keys.forEach((k) => {
      let i = 0;
      for (const n of cols.get(k)!) {
        pos.set(`${n.id}|`, i);
        i += 0.5;
        for (const r of n.rows.values()) pos.set(`${n.id}|${r.key}`, i++);
        i += 1.5;
      }
    });
  const group = (n: BoardNode) => (n.type === 'open' ? -1 : openOrder.indexOf(n.owner ?? ''));
  keys.forEach((k) => cols.get(k)!.sort((x, y) => group(x) - group(y) || y.n - x.n));
  const other = (e: BoardEdge, id: string, key: string | null) =>
    e.a === id && (key === null || e.ar === key) ? pos.get(`${e.b}|${e.br}`) : pos.get(`${e.a}|${e.ar}`);
  for (let it = 0; it < 3; it++) {
    recalc();
    keys.forEach((k) => {
      const col = cols.get(k)!;
      for (const n of col) {
        const rows = [...n.rows.values()];
        for (const r of rows) {
          const ps = es.filter((e) => (e.a === n.id && e.ar === r.key) || (e.b === n.id && e.br === r.key)).map((e) => other(e, n.id, r.key)).filter((x): x is number => x !== undefined);
          r.bb = avg(ps);
        }
        rows.sort((a, b) => a.bb! - b.bb! || a.label.localeCompare(b.label));
        n.rows = new Map(rows.map((r) => [r.key, r]));
        n.bb = avg(es.filter((e) => e.a === n.id || e.b === n.id).map((e) => other(e, n.id, null)).filter((x): x is number => x !== undefined));
      }
      col.sort((x, y) => group(x) - group(y) || x.bb! - y.bb!);
    });
  }
  return keys.map((k) => ({ k, list: cols.get(k)! }));
}

export function geometry(cols: { k: number; list: BoardNode[] }[], width: number, nameWidth: (s: string) => number): Geometry {
  const cw = (c: { list: BoardNode[] }) => (c.list.some((n) => n.type === 'strip') ? STRIP_W : c.list.some((n) => n.type === 'open') ? OPEN_W : FOLD_W);
  const sum = cols.reduce((a, c) => a + cw(c), 0);
  const gap = Math.max(52, Math.min(140, (width - 2 * MARGIN - sum) / Math.max(1, cols.length - 1)));
  const nodes: Placed[] = [];
  const bands: Band[] = [];
  let x = MARGIN;
  let H = 0;
  for (const c of cols) {
    const w = cw(c);
    let y = MARGIN;
    let band: Band | null = null;
    const close = () => {
      if (band) {
        band.h = y - 14 - band.y + 8;
        bands.push(band);
      }
    };
    for (const n of c.list) {
      if (n.type === 'strip') {
        const h = Math.max(96, nameWidth(n.c) + 40);
        nodes.push({ id: n.id, ref: n, col: c.k, x, y, w: STRIP_W, h, head: 0, rh: 0, rows: [], rowAt: new Map() });
        y += h + 10;
        continue;
      }
      const open = n.type === 'open';
      if (!open && (!band || band.owner !== n.owner)) {
        close();
        y += band || c.list[0] !== n ? 6 : 0;
        band = { owner: n.owner ?? '', roles: new Set(), x: 0, y, w: FOLD_W + 16, h: 0, col: c.k };
        y += BAND;
      }
      if (!open && band) for (const r of n.roles) band.roles.add(r);
      const head = open ? 62 : 44;
      const rh = open ? 24 : 22;
      const rows = [...n.rows.values()];
      const bw = open ? OPEN_W : FOLD_W;
      const bx = open ? x : c.k > 0 ? x : c.k < 0 ? x + w - FOLD_W : x + (w - FOLD_W) / 2;
      const h = head + rows.length * rh + (rows.length ? 8 : 0);
      if (band && !open) band.x = bx - 8;
      const placed: Placed = { id: n.id, ref: n, col: c.k, x: bx, y, w: bw, h, head, rh, rows: [], rowAt: new Map() };
      rows.forEach((row, i) => {
        const cy = y + head + i * rh + rh / 2;
        placed.rows.push({ cy, row });
        placed.rowAt.set(row.key, cy);
      });
      nodes.push(placed);
      y += h + 14;
    }
    close();
    H = Math.max(H, y);
    x += w + gap;
  }
  return { nodes, bands, gap, W: x - gap + MARGIN, H: H + MARGIN };
}

export type Side = 'left' | 'right';

export interface End {
  n: string;
  row: string;
  side?: Side;
  x?: number;
  y?: number;
}

export interface Wire {
  id: string;
  src: BoardEdge;
  a: End;
  b: End;
  same: Side;
  dim: boolean;
  soft: boolean;
  on: boolean;
  kmax?: number;
  wps?: [number, number, number][];
  g?: number[];
  d?: string;
  skip?: boolean;
  label?: { text: string; box?: { x: number; y: number; w: number; h: number } };
}

export function assignPorts(nodes: Placed[], wires: Wire[]): void {
  const N = new Map(nodes.map((n) => [n.id, n]));
  const cy = (n: Placed, end: End) => (end.row && n.rowAt.has(end.row) ? n.rowAt.get(end.row)! : n.y + n.h / 2);
  const buckets = new Map<string, { end: End; n: Placed; o: Placed; oe: End }[]>();
  for (const e of wires) {
    const A = N.get(e.a.n);
    const Z = N.get(e.b.n);
    if (!A || !Z) {
      e.skip = true;
      continue;
    }
    let sa: Side;
    let sb: Side;
    if (A.x + A.w <= Z.x - 6) [sa, sb] = ['right', 'left'];
    else if (Z.x + Z.w <= A.x - 6) [sa, sb] = ['left', 'right'];
    else sa = sb = e.same;
    e.a.side = sa;
    e.b.side = sb;
    for (const [end, n, o, oe] of [[e.a, A, Z, e.b], [e.b, Z, A, e.a]] as const) {
      const k = `${n.id}|${end.side}|${end.row}`;
      (buckets.get(k) ?? buckets.set(k, []).get(k)!).push({ end, n, o, oe });
    }
  }
  for (const list of buckets.values()) {
    const { n, end } = list[0];
    const side = end.side!;
    list.sort((p, q) => cy(p.o, p.oe) - cy(q.o, q.oe));
    const k = list.length;
    list.forEach((p, i) => {
      p.end.x = side === 'left' ? n.x : n.x + n.w;
      if (p.end.row && n.rowAt.has(p.end.row)) {
        const span = Math.min(12, (k - 1) * 4);
        p.end.y = n.rowAt.get(p.end.row)! - span / 2 + (k > 1 ? (i * span) / (k - 1) : 0);
      } else {
        const top = n.y + 12;
        const bottom = n.y + n.h - 12;
        p.end.y = top + ((bottom - top) * (i + 1)) / (k + 1);
      }
    });
  }
}

const DIR: Record<Side, number> = { left: -1, right: 1 };

export function pathD(e: Wire): string {
  const ax = DIR[e.a.side!];
  const bx = DIR[e.b.side!];
  const x1 = e.a.x! + ax * 2;
  const y1 = e.a.y!;
  const x2 = e.b.x! + bx * 4;
  const y2 = e.b.y!;
  if (e.wps?.length) {
    const fw = x2 > x1;
    let p = [x1, y1];
    let d = `M${x1},${y1}`;
    const seg = (q: number[]) => {
      const dx = (q[0] - p[0]) / 2;
      return ` C${p[0] + dx},${p[1]} ${q[0] - dx},${q[1]} ${q[0]},${q[1]}`;
    };
    e.wps.forEach(([l, r, y], i) => {
      const en = fw ? [l, y] : [r, y];
      const ex = fw ? [r, y] : [l, y];
      if (i === 0) {
        const dx = (en[0] - p[0]) / 2;
        e.g = [p[0], p[1], p[0] + dx, p[1], en[0] - dx, en[1], en[0], en[1]];
      }
      d += seg(en) + ` L${ex[0]},${ex[1]}`;
      p = ex;
    });
    return (e.d = d + seg([x2, y2]));
  }
  let k = e.a.side === e.b.side ? 34 + Math.abs(y2 - y1) * 0.22 : Math.max(28, Math.hypot(x2 - x1, y2 - y1) * 0.42);
  if (e.a.side !== e.b.side) k = Math.min(k, Math.max(12, Math.abs(x2 - x1) * 0.9));
  k = Math.min(e.kmax ?? 1e9, k);
  e.g = [x1, y1, x1 + ax * k, y1, x2 + bx * k, y2, x2, y2];
  return (e.d = `M${x1},${y1} C${e.g[2]},${e.g[3]} ${e.g[4]},${e.g[5]} ${x2},${y2}`);
}

function bez(g: number[], t: number): [number, number] {
  const u = 1 - t;
  return [
    u * u * u * g[0] + 3 * u * u * t * g[2] + 3 * u * t * t * g[4] + t * t * t * g[6],
    u * u * u * g[1] + 3 * u * u * t * g[3] + 3 * u * t * t * g[5] + t * t * t * g[7],
  ];
}

type Box = { x: number; y: number; w: number; h: number };
const hit = (a: Box, b: Box) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

export function placeLabels(wires: Wire[], nodes: Placed[], textWidth: (s: string) => number): void {
  const obstacles = nodes.map((n) => ({ x: n.x - 3, y: n.y - 3, w: n.w + 6, h: n.h + 6 }));
  const placed: Box[] = [];
  for (const e of wires) {
    if (!e.label || !e.g) continue;
    const w = textWidth(e.label.text) + 16;
    for (const t of [0.5, 0.42, 0.58, 0.34, 0.66, 0.26, 0.74]) {
      const [x, y] = bez(e.g, t);
      const r = { x: x - w / 2, y: y - 10, w, h: 20 };
      if (!obstacles.some((o) => hit(o, r)) && !placed.some((o) => hit(o, r))) {
        placed.push(r);
        e.label.box = r;
        break;
      }
    }
  }
}

export function routeLong(wires: Wire[], G: Geometry): void {
  const cols = new Map<number, { l: number; r: number; rects: [number, number][] }>();
  const col = (k: number) => cols.get(k) ?? cols.set(k, { l: 1e9, r: -1e9, rects: [] }).get(k)!;
  for (const n of G.nodes) {
    const c = col(n.col);
    c.l = Math.min(c.l, n.x);
    c.r = Math.max(c.r, n.x + n.w);
    c.rects.push([n.y, n.y + n.h]);
  }
  for (const b of G.bands) {
    const c = col(b.col);
    c.l = Math.min(c.l, b.x);
    c.r = Math.max(c.r, b.x + b.w);
    c.rects.push([b.y, b.y + b.h]);
  }
  const used = new Map<number, number[]>();
  const N = new Map(G.nodes.map((n) => [n.id, n]));
  for (const e of wires) {
    if (e.skip || e.src.self) continue;
    const A = N.get(e.a.n)!;
    const Z = N.get(e.b.n)!;
    if (Math.abs(A.col - Z.col) < 2) continue;
    const step = Z.col > A.col ? 1 : -1;
    const wps: [number, number, number][] = [];
    for (let k = A.col + step; k !== Z.col; k += step) {
      const C = cols.get(k);
      if (!C) continue;
      const want = e.a.y! + (e.b.y! - e.a.y!) * ((k - A.col) / (Z.col - A.col));
      const gaps: [number, number][] = [];
      let top = -60;
      for (const [y0, y1] of [...C.rects].sort((p, q) => p[0] - q[0])) {
        if (y0 - top >= 14) gaps.push([top, y0]);
        top = Math.max(top, y1);
      }
      gaps.push([top, top + 400]);
      let best = { y: want, d: 1e9, g1: want };
      for (const [g0, g1] of gaps) {
        const y = Math.min(g1 - 7, Math.max(g0 + 7, want));
        if (Math.abs(y - want) < best.d) best = { y, d: Math.abs(y - want), g1 };
      }
      const u = used.get(k) ?? used.set(k, []).get(k)!;
      let y = best.y;
      while (u.some((v) => Math.abs(v - y) < 7) && y + 7 < best.g1) y += 7;
      u.push(y);
      wps.push([C.l - 6, C.r + 6, y]);
    }
    if (wps.length) e.wps = wps;
  }
}
