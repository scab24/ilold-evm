import type { ContractSummary, ProjectConnections } from '$lib/api/rest';
import { INTERFACES, LIBRARIES, OUTSIDE, outsideId, shortName, type Item } from './map-graph';

export const NODE_W = 190;
export const NODE_H = 66;
const GAP_Y = 14;
const PAD = 10;
const HEAD = 30;
const FOOT = 22;
const REGION_GAP = 28;
const COLUMN_GAP = 120;
const CLUSTER_H = 58;
const MEMBER_H = 46;

export type Fold = 'libraries' | 'interfaces' | 'outside';

export interface Groups {
  folders: Map<string, string[]>;
  libraries: string[];
  interfaces: string[];
  outside: string[];
}

export function groups(contracts: ContractSummary[], connections: ProjectConnections): Groups {
  const folders = new Map<string, string[]>();
  const libraries: string[] = [];
  const interfaces: string[] = [];
  for (const c of contracts) {
    if (c.kind === 'Interface') interfaces.push(c.name);
    else if (c.kind === 'Library') libraries.push(c.name);
    else (folders.get(c.folder) ?? folders.set(c.folder, []).get(c.folder)!).push(c.name);
  }
  const listed = new Set(contracts.map((c) => c.name));
  for (const pair of connections.pairs) if (!listed.has(pair.to) && !libraries.includes(pair.to)) libraries.push(pair.to);
  const outside = [...new Set(connections.unresolved.map((u) => outsideId(u.target)))];
  return { folders, libraries, interfaces, outside };
}

export function endpointOf(g: Groups, open: Set<Fold>): (id: string) => string {
  const lib = new Set(g.libraries);
  const ifc = new Set(g.interfaces);
  return (id) => {
    if (!open.has('libraries') && lib.has(id)) return LIBRARIES;
    if (!open.has('interfaces') && ifc.has(id)) return INTERFACES;
    if (!open.has('outside') && id.startsWith('out:')) return OUTSIDE;
    return id;
  };
}

export interface Placed { id: string; x: number; y: number; w: number; h: number; column: number; region: string; index: number }
export interface Region { id: string; label: string; x: number; y: number; w: number; h: number; tone: '' | 'amb'; fold: Fold | null }
export interface Layout { nodes: Map<string, Placed>; regions: Region[] }

const FOLD_LABEL: Record<Fold, string> = { libraries: 'Libraries', interfaces: 'Interfaces', outside: 'Outside the project' };

export function layout(g: Groups, open: Set<Fold>, all: Item[]): Layout {
  const score = new Map<string, number>();
  for (const i of all) {
    if (i.line !== 'calls' && i.line !== 'reads') continue;
    score.set(i.from, (score.get(i.from) ?? 0) + 1);
    score.set(i.to, (score.get(i.to) ?? 0) - 1);
  }
  const folderScore = (members: string[]) => members.reduce((s, m) => s + (score.get(m) ?? 0), 0);
  const folders = [...g.folders.entries()].sort((a, b) => folderScore(b[1]) - folderScore(a[1]) || a[0].localeCompare(b[0]));
  const columns = Math.max(1, Math.min(3, folders.length));
  interface Block { id: string; label: string; members: string[]; column: number; fold: Fold | null; across: number }
  const blocks: Block[] = folders.map(([folder, members], i) => ({
    id: `folder:${folder}`,
    label: `${folder || 'root'} · ${members.length}`,
    members: [...members].sort((a, b) => (score.get(b) ?? 0) - (score.get(a) ?? 0) || a.localeCompare(b)),
    column: Math.floor((i * columns) / folders.length),
    fold: null,
    across: 1,
  }));
  const rowH = (b: Block) => (b.fold ? MEMBER_H : NODE_H) + GAP_Y;
  const height = (b: Block) => {
    if (b.fold && !open.has(b.fold)) return CLUSTER_H;
    return HEAD + Math.ceil(b.members.length / b.across) * rowH(b) - GAP_Y + PAD + (b.fold ? FOOT : 0);
  };
  const columnHeight = (c: number) => blocks.filter((b) => b.column === c).reduce((s, b) => s + height(b) + REGION_GAP, 0);
  for (const fold of ['outside', 'libraries', 'interfaces'] as Fold[]) {
    const members = fold === 'outside' ? g.outside : fold === 'libraries' ? g.libraries : g.interfaces;
    if (!members.length) continue;
    const expanded = open.has(fold);
    const column = expanded ? columns : [...Array(columns).keys()].sort((a, b) => columnHeight(a) - columnHeight(b))[0];
    const count = fold === 'outside' ? `${members.length} targets` : `${members.length}`;
    const sorted = [...members].sort((a, b) => shortName(a).localeCompare(shortName(b)));
    blocks.push({ id: `fold:${fold}`, label: `${FOLD_LABEL[fold]} · ${count}`, members: sorted, column, fold, across: expanded ? Math.ceil(members.length / 9) : 1 });
  }

  const used = Math.max(...blocks.map((b) => b.column)) + 1;
  const tallest = Math.max(...[...Array(used).keys()].map(columnHeight));
  const nodes = new Map<string, Placed>();
  const regions: Region[] = [];
  let x = 0;
  for (let column = 0; column < used; column++) {
    const inColumn = blocks.filter((block) => block.column === column);
    const across = Math.max(1, ...inColumn.map((b) => (b.fold && !open.has(b.fold) ? 1 : b.across)));
    const width = across * NODE_W + (across - 1) * GAP_Y + 2 * PAD;
    let y = (tallest - columnHeight(column)) / 2;
    for (const b of inColumn) {
      const h = height(b);
      if (b.fold && !open.has(b.fold)) {
        const id = b.fold === 'outside' ? OUTSIDE : b.fold === 'libraries' ? LIBRARIES : INTERFACES;
        nodes.set(id, { id, x, y, w: NODE_W + 2 * PAD, h, column, region: b.id, index: 0 });
      } else {
        const w = b.across * NODE_W + (b.across - 1) * GAP_Y + 2 * PAD;
        regions.push({ id: b.id, label: b.label, x, y, w, h, tone: b.fold === 'outside' ? 'amb' : '', fold: b.fold });
        const perColumn = Math.ceil(b.members.length / b.across);
        b.members.forEach((m, i) => {
          const sub = Math.floor(i / perColumn);
          const index = i % perColumn;
          const nh = rowH(b) - GAP_Y;
          nodes.set(m, { id: m, x: x + PAD + sub * (NODE_W + GAP_Y), y: y + HEAD + index * rowH(b), w: NODE_W, h: nh, column, region: `${b.id}:${sub}`, index });
        });
      }
      y += h + REGION_GAP;
    }
    x += width + COLUMN_GAP;
  }
  return { nodes, regions };
}
