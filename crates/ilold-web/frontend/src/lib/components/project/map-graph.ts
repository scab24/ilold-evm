import type { ContractCall, ContractSummary, ProjectConnections, ProjectDepGraph } from '$lib/api/rest';
import { AMBIGUOUS_COLOR } from '$lib/utils/links';

export type Line = 'calls' | 'reads' | 'value' | 'library' | 'inherits' | 'holds' | 'iface' | 'unresolved';
export type LineState = 'show' | 'dim' | 'hide';
export type Look = Exclude<Line, 'iface'> | 'ambiguous';

export const LINES: { id: Line; label: string; tip: string }[] = [
  { id: 'calls', label: 'Calls', tip: 'Calls that can change state, including deploying a contract with new' },
  { id: 'reads', label: 'Reads', tip: 'Calls to read-only functions and public values (getters)' },
  { id: 'value', label: 'Value', tip: 'Calls that move ETH or tokens' },
  { id: 'library', label: 'Library', tip: 'Calls into libraries; the code runs inside the caller' },
  { id: 'inherits', label: 'Inherits', tip: 'A contract inherits a project interface or base' },
  { id: 'holds', label: 'Holds', tip: 'A state variable holds the address of the other contract' },
  { id: 'iface', label: 'Via interface', tip: 'Target matched through the interface it implements (inherits_interface)' },
  { id: 'unresolved', label: 'Unresolved', tip: 'Nothing in the project answers the call (an interface outside the project). Can be dimmed, never hidden' },
];

export const DEFAULT_LINES: Record<Line, LineState> = {
  calls: 'show', reads: 'dim', value: 'show', library: 'dim', inherits: 'hide', holds: 'hide', iface: 'show', unresolved: 'show',
};

export const STYLE: Record<Look, { color: string; width: number; dash: string }> = {
  calls: { color: 'var(--root)', width: 1.6, dash: '' },
  reads: { color: 'var(--root)', width: 1.3, dash: '2 4' },
  library: { color: 'var(--color-text-muted)', width: 1.3, dash: '7 4' },
  value: { color: 'var(--color-success-light)', width: 2.2, dash: '' },
  inherits: { color: '#d2a8ff', width: 1.3, dash: '10 4' },
  holds: { color: '#56d4dd', width: 1.3, dash: '3 3' },
  ambiguous: { color: AMBIGUOUS_COLOR, width: 2, dash: '' },
  unresolved: { color: AMBIGUOUS_COLOR, width: 1.8, dash: '5 4' },
};

const RANK: Record<LineState, number> = { hide: 0, dim: 1, show: 2 };
const PRIORITY: Look[] = ['unresolved', 'ambiguous', 'value', 'calls', 'library', 'reads', 'holds', 'inherits'];

export function nextState(line: Line, state: LineState): LineState {
  if (line === 'unresolved') return state === 'show' ? 'dim' : 'show';
  return state === 'show' ? 'dim' : state === 'dim' ? 'hide' : 'show';
}

export interface Item {
  from: string;
  to: string;
  line: Exclude<Line, 'value' | 'iface'>;
  iface: boolean;
  value: boolean;
  ambiguous: boolean;
  row: ContractCall | null;
}

export const OUTSIDE = '#outside';
export const LIBRARIES = '#libraries';
export const INTERFACES = '#interfaces';
export const outsideId = (target: string) => `out:${target}`;

export function shortName(key: string): string {
  return key.slice(key.lastIndexOf(':') + 1);
}

export function items(contracts: ContractSummary[], connections: ProjectConnections, depgraph: ProjectDepGraph | null): Item[] {
  const listed = new Set(contracts.map((c) => c.name));
  const found: Item[] = [];
  for (const pair of connections.pairs) {
    for (const row of pair.rows) {
      if (row.kind === 'self') continue;
      const mine = row.callees.filter((d) => d.contract === pair.to);
      found.push({
        from: pair.from,
        to: pair.to,
        line: row.kind === 'call' ? 'calls' : row.kind === 'read' ? 'reads' : 'library',
        iface: mine.some((d) => d.basis === 'inherits_interface'),
        value: row.transfers.length > 0,
        ambiguous: mine.some((d) => d.tag === 'AMBIGUOUS'),
        row,
      });
    }
  }
  for (const group of connections.unresolved) {
    for (const row of group.rows) {
      found.push({ from: group.from, to: outsideId(group.target), line: 'unresolved', iface: false, value: row.transfers.length > 0, ambiguous: false, row });
    }
  }
  for (const c of contracts) {
    for (const base of c.inherit_keys) {
      if (listed.has(base)) found.push({ from: c.name, to: base, line: 'inherits', iface: false, value: false, ambiguous: false, row: null });
    }
  }
  for (const e of depgraph?.edges ?? []) {
    if (e.data.kinds.includes('holds')) found.push({ from: e.data.source, to: e.data.target, line: 'holds', iface: false, value: false, ambiguous: false, row: null });
  }
  return found;
}

export function itemState(item: Item, lines: Record<Line, LineState>): number {
  let rank = RANK[lines[item.line]];
  if (item.value) rank = Math.max(rank, RANK[lines.value]);
  if (item.iface) rank = Math.min(rank, RANK[lines.iface]);
  if (item.line === 'unresolved') rank = Math.max(rank, 1);
  return rank;
}

export function itemLook(item: Item, lines: Record<Line, LineState>): Look {
  if (item.line === 'unresolved') return 'unresolved';
  if (item.ambiguous) return 'ambiguous';
  if (item.value && lines.value === 'show') return 'value';
  return item.line;
}

export interface MapEdge {
  id: string;
  a: string;
  b: string;
  items: Item[];
  forward: number;
  back: number;
  state: number;
  look: Look;
}

export function edges(all: Item[], endpoint: (id: string) => string, lines: Record<Line, LineState>): MapEdge[] {
  const byPair = new Map<string, MapEdge>();
  for (const item of all) {
    const state = itemState(item, lines);
    if (state === 0) continue;
    const from = endpoint(item.from);
    const to = endpoint(item.to);
    if (from === to) continue;
    const [a, b] = from < to ? [from, to] : [to, from];
    const id = `${a}|${b}`;
    const edge = byPair.get(id) ?? { id, a, b, items: [], forward: 0, back: 0, state: 0, look: 'calls' as Look };
    edge.items.push(item);
    if (from === a) edge.forward += 1;
    else edge.back += 1;
    edge.state = Math.max(edge.state, state);
    byPair.set(id, edge);
  }
  for (const edge of byPair.values()) {
    const looks = edge.items.filter((i) => itemState(i, lines) === edge.state).map((i) => itemLook(i, lines));
    edge.look = PRIORITY.find((l) => looks.includes(l)) ?? 'calls';
  }
  return [...byPair.values()];
}

export function neighbourhood(all: MapEdge[], start: string, depth: number, direction: 'callers' | 'callees' | 'both'): Map<string, number> {
  const near = new Map([[start, 0]]);
  let frontier = [start];
  for (let step = 1; step <= depth; step++) {
    const next: string[] = [];
    for (const node of frontier) {
      for (const e of all) {
        const other = e.a === node ? e.b : e.b === node ? e.a : null;
        if (!other || near.has(other)) continue;
        const outward = (e.a === node && e.forward > 0) || (e.b === node && e.back > 0);
        const inward = (e.a === node && e.back > 0) || (e.b === node && e.forward > 0);
        if ((direction !== 'callers' && outward) || (direction !== 'callees' && inward)) {
          near.set(other, step);
          next.push(other);
        }
      }
    }
    frontier = next;
  }
  return near;
}
