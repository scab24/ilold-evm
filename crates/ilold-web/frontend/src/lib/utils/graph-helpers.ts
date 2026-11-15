import dagre from '@dagrejs/dagre';
import type { Node, Edge } from '@xyflow/svelte';
import type { GraphNodeData } from '$lib/stores/graph.svelte';

export interface LayoutOptions {
  rankDir?: 'TB' | 'LR';
  nodeSep?: number;
  rankSep?: number;
  nodeWidth?: number;
  nodeHeight?: number;
  size?: (node: Node<GraphNodeData>) => { width: number; height: number };
  minlen?: (edge: Edge) => number;
}

// own-property lookup keeps names like `constructor` off Object.prototype
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
    size = () => ({ width: nodeWidth, height: nodeHeight }),
    minlen = () => 1,
  } = options;

  const g = new dagre.graphlib.Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({ rankdir: rankDir, nodesep: nodeSep, ranksep: rankSep });

  for (const node of nodes) {
    g.setNode(node.id, size(node));
  }

  for (const edge of edges) {
    g.setEdge(edge.source, edge.target, { minlen: minlen(edge) });
  }

  dagre.layout(g);

  return nodes.map((node) => {
    const pos = g.node(node.id);
    return {
      ...node,
      position: { x: pos.x - pos.width / 2, y: pos.y - pos.height / 2 },
    };
  });
}
