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

