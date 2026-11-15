import type { Edge, MarkerType, Node } from '@xyflow/svelte';
import type { BoxNodeData, BoxPort, BoxRow } from '$lib/components/project/BoxNode.svelte';
import type { ChipEdgeData } from '$lib/components/project/ChipEdge.svelte';
import type { GraphNodeData } from '$lib/stores/graph.svelte';
import { lookOf, lookTone, LIBRARIES, OUTSIDE, type Line, type LineState } from '../../project/map-graph.ts';
import { runDagreLayout } from '../../../utils/graph-helpers.ts';
import { lineStyle } from '../../../utils/links.ts';
import { boxHeight, rowCentre } from '../../project/box-size.ts';
import { NEAR_SOURCE, placeChips, type Anchor, type ChipItem, type Rect } from '../../project/chips.ts';
import { edgeBasis, type BoardEdge, type BoardNode, type Built } from './board-links.ts';
import { contractOf, keyParts, type Board, type Layout } from './board-state.ts';

const OPEN_W = 280;
const FOLD_W = 220;
const GAP = 16;
const NODE_SEP = 28;
const ROW_PORT_REACH = 14;
const ACCENT = 'var(--color-accent-hover)';
const PORTS: BoxPort[] = [
  { id: 'in', side: 'l', type: 'target' },
  { id: 'out', side: 'r', type: 'source' },
  { id: 'back', side: 'r', type: 'target' },
];

export const isBoardEdge = (id: string) => id.startsWith('cb:') || id.startsWith('cd:');

export interface Facts {
  entry: boolean;
  restricted: boolean;
  value: boolean;
}

interface BoardGraph {
  nodes: Node<BoxNodeData>[];
  edges: Edge[];
  wires: Map<string, BoardEdge>;
  layout: Edge[];
}

function foldName(card: Pick<BoardNode, 'c' | 'rows'>): string {
  if (card.c === LIBRARIES) return 'Libraries';
  if (card.c !== OUTSIDE) return card.c;
  const interfaces = [...new Set([...card.rows.keys()].map((k) => keyParts(k).member.split('::')[0]))];
  return interfaces.length === 1 ? `${interfaces[0]} (outside)` : 'Outside the project';
}

const count = (n: number) => `${n} ${n === 1 ? 'link' : 'links'}`;

export interface RowRef {
  id: string;
  row: string;
}

const sumLinks = (list: BoardEdge[]) => list.reduce((s, e) => s + e.links.length, 0);

export function boardGraph(built: Built, board: Board, lines: Record<Line, LineState>, facts: (c: string, member: string) => Facts | undefined, order: (c: string) => string[], hot: RowRef | null = null): BoardGraph {
  const cardById = new Map(built.nodes.map((n) => [n.id, n]));
  const selectedCard = board.fn ? `O:${contractOf(board.fn)}` : null;
  const touches = (id: string, row: string) => id === selectedCard && row === board.fn;
  const onEdge = (e: BoardEdge) => !!board.fn && (touches(e.a, e.ar) || touches(e.b, e.br));
  const hotEdge = (e: BoardEdge) => !!hot && ((e.a === hot.id && e.ar === hot.row) || (e.b === hot.id && e.br === hot.row));
  const litPorts = new Set(built.edges.filter((e) => !board.fn || onEdge(e)).flatMap((e) => [`${e.a}|${e.ar}`, `${e.b}|${e.br}`]));
  const valuePorts = new Set(built.edges.filter((e) => e.links.some((l) => l.value)).map((e) => `${e.a}|${e.ar}`));
  const rowRank = new Map<string, number>();

  const sidesOf = (n: BoardNode): ['calls' | 'called by', string, number][] => {
    const sides = new Map<string, ['calls' | 'called by', string, number]>();
    for (const e of [...built.edges, ...built.dimmed]) {
      const verb = e.a === n.id ? 'calls' : e.b === n.id ? 'called by' : null;
      if (!verb) continue;
      const other = cardById.get(verb === 'calls' ? e.b : e.a)!.c;
      const side = sides.get(`${verb}|${other}`) ?? [verb, other, 0];
      side[2] += e.links.length;
      sides.set(`${verb}|${other}`, side);
    }
    return [...sides.values()];
  };

  const card = (n: BoardNode): BoxNodeData => {
    const base = { _type: 'box' as const, ports: PORTS, rowPorts: true };
    if (n.type === 'open' || n.type === 'collapsed') {
      const main = n.c === board.main;
      const known = order(n.c);
      const at = (key: string) => {
        const i = known.indexOf(keyParts(key).member);
        return i < 0 ? known.length : i;
      };
      const rows: BoxRow[] = [...n.rows.values()]
        .sort((x, y) => at(x.key) - at(y.key) || x.label.localeCompare(y.label))
        .map((r, i) => {
          rowRank.set(`${n.id}|${r.key}`, i);
          const f = facts(n.c, keyParts(r.key).member);
          const value = valuePorts.has(`${n.id}|${r.key}`) || !!f?.value;
          const marks = [...(f?.restricted ? (['lock'] as const) : []), ...(value ? (['value'] as const) : [])];
          const out = sumLinks(built.edges.filter((e) => e.a === n.id && e.ar === r.key));
          const into = sumLinks(built.edges.filter((e) => e.b === n.id && e.br === r.key));
          const links = [out ? `${out}→` : '', into ? `${into}←` : ''].filter(Boolean).join(' ');
          const said = [f?.entry ? ', entry point' : '', f?.restricted ? ', restricted' : '', value ? ', moves value' : '', out ? `, ${out} out` : '', into ? `, ${into} in` : ''].join('');
          return { id: r.key, text: r.label, title: r.title, icon: f?.entry ? 'entry' : undefined, marks, count: links || undefined, on: board.fn === r.key, label: `${n.c}.${r.label}${said}` };
        });
      const collapsed = n.type === 'collapsed';
      return {
        ...base,
        label: n.c,
        variant: main ? 'main' : 'open',
        lines: [{ text: collapsed ? `${count(n.n)} · click to expand` : main ? 'main contract' : 'double-click to make it main' }],
        rows,
        rowAction: true,
        headAction: collapsed ? `Expand ${n.c}` : main ? `${n.c}, main contract` : `Make ${n.c} the main contract`,
        closable: main ? undefined : `Close ${n.c} on the board`,
        width: OPEN_W,
      };
    }
    const name = foldName(n);
    const openable = n.type === 'folded';
    const otherRank = (key: string) =>
      Math.min(...built.edges.filter((e) => (e.a === n.id && e.ar === key) || (e.b === n.id && e.br === key)).map((e) => rowRank.get(e.a === n.id ? `${e.b}|${e.br}` : `${e.a}|${e.ar}`) ?? 1e3));
    const rows: BoxRow[] = [...n.rows.values()]
      .sort((x, y) => otherRank(x.key) - otherRank(y.key) || x.label.localeCompare(y.label))
      .map((r) => ({ id: r.key, text: r.label, title: r.title, tone: r.tone ?? (litPorts.has(`${n.id}|${r.key}`) ? undefined : 'dim'), label: `${name} ${r.label}${openable ? `. Opens ${n.c} here` : ''}` }));
    return {
      ...base,
      label: name,
      icon: n.c === LIBRARIES ? 'lib' : n.c === OUTSIDE ? 'unres' : undefined,
      variant: n.c === OUTSIDE ? 'out' : n.type === 'group' ? 'cluster' : '',
      lines: sidesOf(n).map(([verb, other, links]) => ({ text: `${verb} ${other} · ${links}`, icon: verb === 'calls' ? 'call' : 'entry', tone: n.c === OUTSIDE ? 'amb' : undefined })),
      rows,
      rowAction: openable,
      headAction: openable ? `${name}, ${count(n.n)}. Opens it here` : undefined,
      width: FOLD_W,
    };
  };

  const openFirst = [...built.nodes].sort((x, y) => Number(x.type === 'folded' || x.type === 'group') - Number(y.type === 'folded' || y.type === 'group'));
  const dataById = new Map(openFirst.map((n) => [n.id, card(n)]));
  const nodes = built.nodes.map((n) => ({ id: n.id, type: 'box', position: { x: 0, y: 0 }, data: dataById.get(n.id)! }));

  const wires = new Map<string, BoardEdge>();
  const edges: Edge[] = [];
  const layout: Edge[] = [];
  const draw = (e: BoardEdge, id: string, dim: boolean) => {
    const fromCol = cardById.get(e.a)!.col;
    const toCol = cardById.get(e.b)!.col;
    const flip = fromCol > toCol;
    const [source, sourceRow, target, targetRow] = flip ? [e.b, e.br, e.a, e.ar] : [e.a, e.ar, e.b, e.br];
    const handle = (row: string, side: string) => (row ? `${row}:${side}` : side);
    const on = !dim && onEdge(e);
    const bright = on || (!dim && hotEdge(e));
    const look = lookOf(e.links, lines);
    const style = lineStyle(look, edgeBasis(e));
    const color = dim ? 'var(--color-text-dim)' : bright && (look === 'calls' || look === 'library' || look === 'reads') ? ACCENT : style.color;
    const marker = { type: 'arrowclosed' as MarkerType, color, width: 14, height: 14 };
    const lineNos = [...new Set(e.links.map((l) => l.row.span.start_line))].sort((x, y) => x - y);
    const sameCol = fromCol === toCol;
    const chip = on && !sameCol ? lineNos.map((n) => `:${n}`).join(' ') : undefined;
    wires.set(id, e);
    edges.push({
      id,
      type: sameCol ? 'smoothstep' : 'chip',
      source,
      target,
      sourceHandle: handle(sourceRow, 'out'),
      targetHandle: handle(targetRow, sameCol ? 'back' : 'in'),
      markerEnd: flip ? undefined : marker,
      markerStart: flip ? marker : undefined,
      zIndex: bright ? 2 : undefined,
      style: `stroke: ${color}; stroke-width: ${Math.round((style.width + (bright ? 0.6 : 0)) * 10) / 10}; opacity: ${bright ? 1 : dim ? 0.25 : 0.4}${style.dash ? `; stroke-dasharray: ${style.dash}` : ''}`,
      data: {
        chip,
        tone: lookTone(look),
        on,
      } satisfies ChipEdgeData,
    });
    if (!sameCol) layout.push({ id, source, target, data: { minlen: Math.abs(fromCol - toCol) } });
  };
  built.edges.forEach((e) => draw(e, `cb:${e.a}|${e.ar}>${e.b}|${e.br}`, false));
  built.dimmed.forEach((e) => draw(e, `cd:${e.a}>${e.b}`, true));
  return { nodes, edges, wires, layout };
}

export function shapeOf(graph: BoardGraph): string {
  return JSON.stringify(graph.nodes.map((n) => [n.id, n.data.width, boxHeight(n.data)]));
}

export function placeLabels(graph: BoardGraph, positionOf: (id: string) => { x: number; y: number } | undefined): Edge[] {
  const dataById = new Map(graph.nodes.map((n) => [n.id, n.data]));
  const rectOf = (id: string): Rect | null => {
    const p = positionOf(id);
    const data = dataById.get(id);
    return p && data ? { ...p, w: data.width ?? FOLD_W, h: boxHeight(data) } : null;
  };
  const anchor = (id: string, handle: string): Anchor | null => {
    const r = rectOf(id);
    if (!r) return null;
    const cut = handle.lastIndexOf(':');
    const row = cut < 0 ? '' : handle.slice(0, cut);
    const port = cut < 0 ? handle : handle.slice(cut + 1);
    const data = dataById.get(id)!;
    const index = row ? (data.rows ?? []).findIndex((x) => x.id === row) : -1;
    const side = port === 'in' ? 'l' : 'r';
    const reach = index >= 0 ? ROW_PORT_REACH : 0;
    const y = r.y + (index >= 0 ? rowCentre(data, index) : r.h * (port === 'in' ? 1 / 2 : port === 'out' ? 1 / 3 : 2 / 3));
    return { x: side === 'l' ? r.x - reach : r.x + r.w + reach, y, side };
  };
  const items: ChipItem[] = [];
  const arrowFirst = new Set<string>();
  for (const e of graph.edges) {
    const chip = (e.data as ChipEdgeData | undefined)?.chip;
    if (!chip) continue;
    const s = anchor(e.source, e.sourceHandle ?? 'out');
    const t = anchor(e.target, e.targetHandle ?? 'in');
    if (!s || !t) continue;
    if (e.markerStart) arrowFirst.add(e.id);
    items.push({ id: e.id, s: e.markerStart ? s : t, t: e.markerStart ? t : s, text: chip });
  }
  const boxes = graph.nodes.map((n) => rectOf(n.id)).filter((r): r is Rect => !!r);
  const chosen = placeChips(items, boxes, NEAR_SOURCE);
  return graph.edges.map((e) => {
    if (!chosen.has(e.id)) return e;
    const at = chosen.get(e.id)!;
    const labelAt = at === null ? null : arrowFirst.has(e.id) ? at : 1 - at;
    return { ...e, data: { ...(e.data as ChipEdgeData), labelAt } };
  });
}

export interface Known extends Rect {
  pinned: boolean;
  at: { x: number; y: number };
}

interface Card extends Rect {
  pinned: boolean;
  fixed: boolean;
  moved: boolean;
  yields?: boolean;
}

const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w + GAP && b.x < a.x + a.w + GAP && a.y < b.y + b.h + GAP && b.y < a.y + a.h + GAP;

function autoLayout(built: Built, graph: BoardGraph, layout: Layout): Map<string, { x: number; y: number }> {
  const laid = runDagreLayout(graph.nodes as Node<GraphNodeData>[], graph.layout, {
    rankDir: layout,
    nodeSep: NODE_SEP,
    rankSep: layout === 'LR' ? 120 : 90,
    size: (n) => ({ width: (n.data as BoxNodeData).width ?? FOLD_W, height: boxHeight(n.data as BoxNodeData) }),
    minlen: (e) => (e.data?.minlen as number | undefined) ?? 1,
  });
  const at = new Map(laid.map((n) => [n.id, { ...n.position }]));
  const dataById = new Map(graph.nodes.map((n) => [n.id, n.data]));
  const colOf = new Map(built.nodes.map((n) => [n.id, n.col]));
  const across = layout === 'LR' ? 'y' : 'x';
  const size = (id: string) => (layout === 'LR' ? boxHeight(dataById.get(id)!) : (dataById.get(id)!.width ?? FOLD_W));
  const portAt = (id: string, row: string) => {
    const data = dataById.get(id)!;
    const index = row && layout === 'LR' ? (data.rows ?? []).findIndex((r) => r.id === row) : -1;
    return at.get(id)![across] + (index < 0 ? size(id) / 2 : rowCentre(data, index));
  };
  const columns = [...new Set(colOf.values())].filter((c) => c !== 0).sort((x, y) => Math.abs(x) - Math.abs(y));
  for (const col of columns) {
    const ids = built.nodes.filter((n) => n.col === col).map((n) => n.id);
    const inner = (id: string) => Math.abs(colOf.get(id)!) < Math.abs(col);
    const weight = (id: string) => {
      if (cardOf(built, id)?.c === OUTSIDE) return Infinity;
      const ports = [...built.edges, ...built.dimmed].flatMap((e) => (e.a === id && inner(e.b) ? [portAt(e.b, e.br)] : e.b === id && inner(e.a) ? [portAt(e.a, e.ar)] : []));
      return ports.length ? ports.reduce((s, p) => s + p, 0) / ports.length : at.get(id)![across];
    };
    const weights = new Map(ids.map((id) => [id, weight(id)]));
    let next = Math.min(...ids.map((id) => at.get(id)![across]));
    for (const id of ids.sort((x, y) => weights.get(x)! - weights.get(y)! || x.localeCompare(y))) {
      at.get(id)![across] = next;
      next += size(id) + NODE_SEP;
    }
  }
  return at;
}

const cardOf = (built: Built, id: string) => built.nodes.find((n) => n.id === id);

export function place(built: Built, board: Board, graph: BoardGraph, known: (id: string) => Known | null): Node<BoxNodeData>[] {
  const cardById = new Map(built.nodes.map((n) => [n.id, n]));
  const anchor = (id: string) => {
    const n = cardById.get(id);
    return !n ? null : n.owner ? `O:${n.owner}` : board.parent[n.c] ? `O:${board.parent[n.c]}` : null;
  };
  const auto = graph.nodes.some((n) => !known(n.id)) ? autoLayout(built, graph, board.layout) : new Map<string, { x: number; y: number }>();
  const where = new Map<string, Card>();
  const grown: string[] = [];
  for (const n of graph.nodes) {
    const k = known(n.id);
    if (!k) continue;
    const h = boxHeight(n.data);
    if (h > k.h + 1) grown.push(n.id);
    where.set(n.id, { x: k.x, y: k.y, w: k.w, h: Math.max(k.h, h), pinned: k.pinned, fixed: k.pinned, moved: false });
  }
  const settle = (id: string, r: Card) => {
    const skip = new Set([id]);
    for (let hit = [...where].find(([o, t]) => !skip.has(o) && overlaps(t, r)); hit; hit = [...where].find(([o, t]) => !skip.has(o) && overlaps(t, r))) {
      const [o, t] = hit;
      if (r.yields) {
        r.y = t.y + t.h + GAP;
        r.moved = true;
      } else if (!t.fixed && (t.y >= r.y || r.pinned)) {
        t.y = r.y + r.h + GAP;
        t.moved = true;
        settle(o, t);
      } else if (!r.pinned) {
        r.y = t.y + t.h + GAP;
        r.moved = true;
      } else skip.add(o);
    }
  };
  for (const id of grown.sort((x, y) => where.get(x)!.y - where.get(y)!.y)) settle(id, where.get(id)!);
  const depth = (id: string) => {
    let d = 0;
    for (let at = anchor(id); at && d < built.nodes.length; at = anchor(at)) d++;
    return d;
  };
  const twinOf = (n: BoardNode) => (n.type === 'open' || n.type === 'collapsed' ? `C:${n.c}@${n.col}` : n.type === 'folded' ? `O:${n.c}` : null);
  const start = (id: string): { x: number; y: number; yields?: boolean } => {
    const n = cardById.get(id)!;
    const twin = twinOf(n);
    const before = twin ? known(twin) : null;
    if (before) return { x: before.x, y: before.y };
    const p = auto.get(id)!;
    for (let at = anchor(id), d = 0; at && d < built.nodes.length; at = anchor(at), d++) {
      const c = where.get(at);
      const q = auto.get(at);
      if (c && q) return { x: p.x + c.x - q.x, y: p.y + c.y - q.y, yields: true };
    }
    return { ...p, yields: where.size > 0 };
  };
  for (const n of graph.nodes.filter((x) => !where.has(x.id)).sort((x, y) => depth(x.id) - depth(y.id))) {
    const r = { ...start(n.id), w: n.data.width ?? FOLD_W, h: boxHeight(n.data), pinned: false, fixed: true, moved: true };
    where.set(n.id, r);
    settle(n.id, r);
  }
  return graph.nodes.map((n) => {
    const c = where.get(n.id)!;
    return { ...n, position: c.moved ? { x: c.x, y: c.y } : known(n.id)!.at };
  });
}
