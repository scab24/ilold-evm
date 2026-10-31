import dagre from '@dagrejs/dagre';
import type { Node, Edge } from '@xyflow/svelte';
import type { GraphNodeData } from '$lib/stores/graph.svelte';

export interface LayoutOptions {
  rankDir?: 'TB' | 'LR';
  nodeSep?: number;
  rankSep?: number;
  nodeWidth?: number;
  nodeHeight?: number;
}

// own-property lookup, so names like `constructor` never hit Object.prototype
export function ownEntry<T>(record: Record<string, T> | undefined, key: string): T | undefined {
  return record && Object.hasOwn(record, key) ? record[key] : undefined;
}

export function runDagreLayout(
  nodes: Node<GraphNodeData>[],
  edges: Edge[],
  options: LayoutOptions = {},
): Node<GraphNodeData>[] {
  const {
    rankDir = 'TB',
    nodeSep = 30,
    rankSep = 45,
    nodeWidth = 150,
    nodeHeight = 40,
  } = options;

  const g = new dagre.graphlib.Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: rankDir, nodesep: nodeSep, ranksep: rankSep });

  for (const node of nodes) {
    g.setNode(node.id, { width: nodeWidth, height: nodeHeight });
  }

  for (const edge of edges) {
    g.setEdge(edge.source, edge.target);
  }

  dagre.layout(g);

  return nodes.map((node) => {
    const pos = g.node(node.id);
    return {
      ...node,
      position: { x: pos.x - nodeWidth / 2, y: pos.y - nodeHeight / 2 },
    };
  });
}


export interface PackOptions {
  nodeWidth: number;
  nodeHeight: number;
  nodeSep: number;
  rankSep: number;
  /** Target width / height of the whole drawing. */
  aspect: number;
}

/** A rank taller than the row limit wraps into sub-columns; the limit needing the least zoom-out wins. */
export function packLayeredLR(nodes: Node<GraphNodeData>[], options: PackOptions): Node<GraphNodeData>[] {
  if (nodes.length === 0) return nodes;
  const { nodeWidth, nodeHeight, nodeSep, rankSep, aspect } = options;

  const byX = new Map<number, Node<GraphNodeData>[]>();
  for (const n of nodes) {
    const key = Math.round(n.position.x);
    const rank = byX.get(key) ?? [];
    rank.push(n);
    byX.set(key, rank);
  }
  const ranks = [...byX.entries()].sort((a, b) => a[0] - b[0]).map(([, r]) => r.sort((a, b) => a.position.y - b.position.y));
  const tallest = Math.max(...ranks.map((r) => r.length));

  const colStep = nodeWidth + nodeSep;
  const rowStep = nodeHeight + nodeSep;
  const size = (maxRows: number) => {
    const columns = ranks.reduce((sum, r) => sum + Math.ceil(r.length / maxRows), 0);
    const width = columns * nodeWidth + (columns - ranks.length) * nodeSep + (ranks.length - 1) * rankSep;
    const height = Math.min(maxRows, tallest) * rowStep - nodeSep;
    return { width, height };
  };
  let best = tallest;
  let bestCost = Infinity;
  for (let rows = 1; rows <= tallest; rows++) {
    const { width, height } = size(rows);
    const cost = Math.max(width / aspect, height);
    if (cost < bestCost) {
      bestCost = cost;
      best = rows;
    }
  }

  const height = size(best).height;
  const placed = new Map<string, { x: number; y: number }>();
  let x = 0;
  for (const rank of ranks) {
    const columns = Math.ceil(rank.length / best);
    const rows = Math.min(rank.length, best);
    const top = (height - (rows * rowStep - nodeSep)) / 2;
    rank.forEach((n, i) => {
      placed.set(n.id, { x: x + Math.floor(i / best) * colStep, y: top + (i % best) * rowStep });
    });
    x += columns * nodeWidth + (columns - 1) * nodeSep + rankSep;
  }
  return nodes.map((n) => ({ ...n, position: placed.get(n.id) ?? n.position }));
}
