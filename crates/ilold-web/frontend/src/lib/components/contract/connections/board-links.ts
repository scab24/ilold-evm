import type { Basis, Connections, ContractCall, DispatchCandidate } from '$lib/api/rest';
import { itemLook, itemState, type Item, type Line, type LineState, type Look } from '$lib/components/project/map-graph';
import { splitRef } from '$lib/utils/links';
import { collapsed, expanded, fnKey, keyParts, type Board } from './board-state';

export const LIBRARIES = '#lib';
export const OUTSIDE = '#out';

export interface Link extends Item {
  row: ContractCall;
  callee: DispatchCandidate | null;
  self: boolean;
  fromKey: string;
  toKey: string;
}

export function member(ref: string): string {
  return splitRef(ref).member;
}

export function shortName(ref: string): string {
  return member(ref).split('(')[0];
}

function weakest(found: Basis[]): Basis | null {
  return (['selector_match', 'self'] as const).find((b) => found.includes(b)) ?? found[0] ?? null;
}

export function linksOf(all: Connections[], libraries: Set<string>): Link[] {
  const seen = new Set<string>();
  const found: Link[] = [];
  const keyOf = (contract: string, ref: string) => {
    const decl = splitRef(ref).contract;
    return libraries.has(decl) && decl !== contract ? fnKey(contract, ref) : fnKey(contract, member(ref));
  };
  const add = (row: ContractCall, to: string, callees: DispatchCandidate[]) => {
    const id = `${row.in_context}|${row.function}|${row.span.start_line}:${row.span.start_col}|${to}`;
    if (seen.has(id)) return;
    seen.add(id);
    const callee = callees[0] ?? null;
    const line: Link['line'] = row.kind === 'call' || row.kind === 'self' ? 'calls' : row.kind === 'read' ? 'reads' : 'library';
    found.push({
      from: row.in_context,
      to,
      line,
      iface: callees.some((d) => d.basis === 'inherits_interface'),
      value: row.transfers.length > 0,
      ambiguous: row.callees.some((d) => d.tag === 'AMBIGUOUS'),
      basis: weakest(callees.map((d) => d.basis)),
      row,
      callee,
      self: row.kind === 'self',
      fromKey: keyOf(row.in_context, row.function),
      toKey: callee ? (line === 'library' ? fnKey(LIBRARIES, callee.function) : keyOf(to, callee.function)) : fnKey(OUTSIDE, row.target ?? ''),
    });
  };
  for (const c of all) {
    for (const group of [...c.outgoing, ...c.incoming]) {
      for (const row of group.calls) {
        const to = row.kind === 'self' ? row.in_context : group.contract === row.in_context ? null : group.contract;
        const target = to ?? c.contract;
        if (row.kind !== 'self' && target === row.in_context) continue;
        add(row, target, row.callees.filter((d) => d.contract === target));
      }
    }
    for (const row of c.unresolved) add(row, OUTSIDE, []);
  }
  for (const l of found) if (l.to === OUTSIDE) l.line = 'unresolved';
  return found;
}

export interface BoardRow {
  key: string;
  label: string;
  title: string;
  amb?: boolean;
  bb?: number;
}

export interface BoardNode {
  id: string;
  type: 'open' | 'strip' | 'folded' | 'group';
  c: string;
  col: number;
  owner?: string;
  roles: Set<'caller' | 'callee'>;
  rows: Map<string, BoardRow>;
  n: number;
  bb?: number;
}

export interface BoardEdge {
  a: string;
  ar: string;
  b: string;
  br: string;
  self?: boolean;
  links: Link[];
}

export interface Built {
  nodes: BoardNode[];
  edges: BoardEdge[];
  dimmed: BoardEdge[];
}

export function rowLabel(key: string, box: string): string {
  const { contract, member: m } = keyParts(key);
  const ref = splitRef(m);
  const name = ref.member.split('(')[0];
  if (contract === OUTSIDE || contract === LIBRARIES || (ref.contract && ref.contract !== box)) return `${shortContract(ref.contract)}.${name}`;
  return name;
}

export function shortContract(name: string): string {
  return name.slice(name.lastIndexOf(':') + 1);
}

export function build(b: Board, links: Link[], lines: Record<Line, LineState>, functions: (c: string) => string[]): Built {
  const open = expanded(b);
  const strips = new Set(collapsed(b));
  const nodes = new Map<string, BoardNode>();
  const edges = new Map<string, BoardEdge>();
  const dimmed = new Map<string, BoardEdge>();
  const node = (id: string, o: Omit<BoardNode, 'id' | 'rows' | 'n' | 'roles'>) => {
    if (!nodes.has(id)) nodes.set(id, { id, rows: new Map(), n: 0, roles: new Set(), ...o });
    return nodes.get(id)!;
  };
  for (const c of b.open) if (open.has(c)) node(`O:${c}`, { type: 'open', c, col: b.col[c] });
  for (const c of strips) node(`K:${c}`, { type: 'strip', c, col: 0 });
  const addRow = (n: BoardNode, key: string, extra: Partial<BoardRow> = {}) => {
    if (n.type !== 'strip' && !n.rows.has(key)) n.rows.set(key, { key, label: rowLabel(key, n.c), title: keyParts(key).member, ...extra });
  };
  const outCol = (o: string, callee: boolean) => {
    const k = b.col[o];
    return k === 0 ? (callee ? 1 : -1) : k + Math.sign(k);
  };
  const folded = (p: string, col: number, owner: string, role: 'caller' | 'callee') => {
    if (strips.has(p)) return nodes.get(`K:${p}`)!;
    const n = node(`C:${p}@${col}`, { type: p.startsWith('#') ? 'group' : 'folded', c: p, col, owner });
    n.roles.add(role);
    return n;
  };
  const push = (into: Map<string, BoardEdge>, key: string, e: BoardEdge) => {
    const found = into.get(key);
    if (found) found.links.push(...e.links);
    else into.set(key, e);
  };
  for (const l of links) {
    const state = itemState(l, lines);
    if (!state) continue;
    if (l.self) {
      if (!open.has(l.from) || state === 1) continue;
      const n = nodes.get(`O:${l.from}`)!;
      addRow(n, l.fromKey);
      addRow(n, l.toKey);
      push(edges, `S|${l.fromKey}|${l.toKey}`, { a: n.id, ar: l.fromKey, b: n.id, br: l.toKey, self: true, links: [l] });
      continue;
    }
    const target = l.line === 'library' ? LIBRARIES : l.to;
    const ao = open.has(l.from);
    const bo = open.has(target);
    if ((!ao && !bo) || (!bo && b.dir === 'callers') || (!ao && b.dir === 'callees')) continue;
    const A = ao ? nodes.get(`O:${l.from}`)! : folded(l.from, outCol(target, false), target, 'caller');
    const Z = bo ? nodes.get(`O:${target}`)! : folded(target, outCol(l.from, true), l.from, 'callee');
    A.n++;
    Z.n++;
    if (state === 1) {
      push(dimmed, `${A.id}>${Z.id}`, { a: A.id, ar: '', b: Z.id, br: '', links: [l] });
      continue;
    }
    const ar = A.type === 'strip' ? '' : l.fromKey;
    const br = Z.type === 'strip' ? '' : l.toKey;
    if (ar) addRow(A, ar);
    if (br) addRow(Z, br, { amb: target === OUTSIDE });
    push(edges, `${A.id}|${ar}>${Z.id}|${br}`, { a: A.id, ar, b: Z.id, br, links: [l] });
  }
  if (b.fn && open.has(keyParts(b.fn).contract)) addRow(nodes.get(`O:${keyParts(b.fn).contract}`)!, b.fn);
  if (b.all) for (const c of open) for (const m of functions(c)) addRow(nodes.get(`O:${c}`)!, fnKey(c, m));
  const cols = [...nodes.values()].filter((n) => n.type !== 'strip').map((n) => n.col);
  const minCol = Math.min(...cols);
  for (const n of nodes.values()) if (n.type === 'strip') n.col = minCol - 1;
  return { nodes: [...nodes.values()], edges: [...edges.values()], dimmed: [...dimmed.values()] };
}

const PRIORITY: Look[] = ['unresolved', 'ambiguous', 'value', 'calls', 'library', 'reads'];

export function edgeLook(e: BoardEdge, lines: Record<Line, LineState>): Look {
  const looks = e.links.map((l) => itemLook(l, lines));
  return PRIORITY.find((p) => looks.includes(p)) ?? 'calls';
}

export function edgeBasis(e: BoardEdge): Basis | null {
  return weakest(e.links.map((l) => l.basis).filter((x): x is Basis => x !== null));
}
