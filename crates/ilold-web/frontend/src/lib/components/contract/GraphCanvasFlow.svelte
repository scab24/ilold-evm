<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { SvelteFlow, SvelteFlowProvider, Background, Controls, Panel, SelectionMode, useSvelteFlow, type EdgeTypes, type FitViewOptions, type NodeTypes } from '@xyflow/svelte';
  import type { Node, Edge } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';

  import FunctionNode from './nodes/FunctionNode.svelte';
  import BlockNode from './nodes/BlockNode.svelte';
  import SequenceNode from './nodes/SequenceNode.svelte';
  import {
    getNodes,
    getEdges,
    type GraphNodeData,
  } from '$lib/stores/graph.svelte';

  interface Props {
    onnodetap?: (node: Node<GraphNodeData>, event: MouseEvent) => void;
    onbackgroundtap?: () => void;
    oncontextmenu?: (event: MouseEvent, node: Node<GraphNodeData>) => void;
    onready?: (api: { fitView: (opts?: any) => Promise<boolean> }) => void;
    onnodesdelete?: (nodes: Node<GraphNodeData>[]) => void;
    canDeleteNodes?: boolean;
    onselectionchange?: (nodes: Node<GraphNodeData>[]) => void;
    onnodemouseenter?: (node: Node<GraphNodeData>) => void;
    onnodemouseleave?: (node: Node<GraphNodeData>) => void;
    onedgemouseenter?: (edge: Edge, event: MouseEvent) => void;
    onedgemouseleave?: (edge: Edge) => void;
    onedgetap?: (edge: Edge) => void;
    extraNodeTypes?: NodeTypes;
    edgeTypes?: EdgeTypes;
    keepPositions?: boolean;
    followStore?: boolean;
    initialFit?: FitViewOptions;
    minZoom?: number;
    tools?: boolean;
  }

  let { onnodetap, onbackgroundtap, oncontextmenu, onready, onnodesdelete, canDeleteNodes = true, onselectionchange, onnodemouseenter, onnodemouseleave, onedgemouseenter, onedgemouseleave, onedgetap, extraNodeTypes, edgeTypes, keepPositions = true, followStore = false, initialFit, minZoom = 0.1, tools = true }: Props = $props();

  const deleteKeysProp = $derived<string[] | null>(canDeleteNodes ? ['Delete', 'Backspace'] : null);

  const nodeTypes = $derived({
    function: FunctionNode,
    block: BlockNode,
    sequence: SequenceNode,
    ...extraNodeTypes,
  } as unknown as NodeTypes);


  let flowNodes = $state<Node[]>([]);
  let flowEdges = $state<Edge[]>([]);

  let stored = new Map<string, { x: number; y: number }>();

  $effect(() => {
    const storeNodes = getNodes();
    const liveByID = new Map(untrack(() => flowNodes).map(n => [n.id, n]));
    const before = stored;
    stored = new Map(storeNodes.map((sn) => [sn.id, sn.position]));
    flowNodes = storeNodes.map(sn => {
      const live = liveByID.get(sn.id);
      const moved = before.get(sn.id);
      if (keepPositions && live?.position && (!followStore || !moved || (moved.x === sn.position.x && moved.y === sn.position.y))) {
        return { ...sn, position: live.position } as Node;
      }
      return sn as Node;
    });
  });

  $effect(() => {
    const storeEdges = getEdges();
    flowEdges = storeEdges;
  });

  let flowFitView = $state<((opts?: FitViewOptions) => Promise<boolean>) | null>(null);
  let paneWidth = $state(0);
  let paneHeight = $state(0);
  let initialFitDone = false;

  // a fit before layout clamps to minZoom
  function measured(options: FitViewOptions): boolean {
    if (!flowFitView || paneWidth === 0 || paneHeight === 0) return false;
    const wanted = options.nodes ? new Set(options.nodes.map((n) => n.id)) : null;
    const visible = flowNodes.filter((n) => !n.hidden && (!wanted || wanted.has(n.id)));
    return visible.length > 0 && visible.every((n) => n.measured?.width && n.measured?.height);
  }

  function fitLater(options: FitViewOptions) {
    const fit = flowFitView!;
    requestAnimationFrame(() => { fit(options); });
  }

  $effect(() => {
    if (initialFitDone || !initialFit || !measured(initialFit)) return;
    initialFitDone = true;
    fitLater(initialFit);
  });

  let view: Pick<ReturnType<typeof useSvelteFlow>, 'getViewport' | 'setViewport'> | null = null;

  interface Aim {
    ids: string[];
    fallback: string | null;
    zoom: number | null;
    duration: number;
  }
  let aim = $state<Aim | null>(null);
  const margin = $derived({ top: tools ? 64 : 28, right: 28, bottom: 28, left: 56 });

  $effect(() => {
    const a = aim;
    if (!a || !view || !paneWidth || !paneHeight) return;
    const cards = flowNodes.filter((n) => a.ids.includes(n.id) && !n.hidden);
    if (!cards.length || !cards.every((n) => n.measured?.width && n.measured?.height)) return;
    aim = null;
    const now = view.getViewport();
    const zoom = a.zoom ?? now.zoom;
    const bounds = (list: Node[]) => {
      const x = Math.min(...list.map((n) => n.position.x));
      const y = Math.min(...list.map((n) => n.position.y));
      return { x, y, w: Math.max(...list.map((n) => n.position.x + n.measured!.width!)) - x, h: Math.max(...list.map((n) => n.position.y + n.measured!.height!)) - y };
    };
    let r = bounds(cards);
    const fallback = cards.find((n) => n.id === a.fallback);
    if (fallback && (r.w * zoom > paneWidth - margin.left - margin.right || r.h * zoom > paneHeight - margin.top - margin.bottom)) r = bounds([fallback]);
    const along = (start: number, size: number, at: number, low: number, high: number) => {
      const startAtLow = low - start * zoom;
      const endAtHigh = high - (start + size) * zoom;
      if (endAtHigh < startAtLow) return startAtLow;
      return a.zoom === null ? Math.min(Math.max(at, startAtLow), endAtHigh) : (startAtLow + endAtHigh) / 2;
    };
    const x = along(r.x, r.w, now.x, margin.left, paneWidth - margin.right);
    const y = along(r.y, r.h, now.y, margin.top, paneHeight - margin.bottom);
    if (x !== now.x || y !== now.y || zoom !== now.zoom) view.setViewport({ x, y, zoom }, { duration: a.duration });
  });

  export function reveal(id: string) {
    aim = { ids: [id], fallback: null, zoom: null, duration: 300 };
  }

  export function frame(ids: string[], fallback: string, zoom: number, duration = 0) {
    aim = { ids, fallback, zoom, duration };
  }

  export function getLiveNode(id: string): Node | undefined {
    return flowNodes.find(n => n.id === id);
  }

  let canvasMode = $state<'pan' | 'select'>('pan');
  let panOnDragProp = $derived<boolean | number[]>(canvasMode === 'pan' ? true : [1, 2]);
  let selectionOnDragProp = $derived(canvasMode === 'select');

  function handleKeydown(e: KeyboardEvent) {
    if (!tools) return;
    const t = e.target;
    if (
      t instanceof HTMLInputElement ||
      t instanceof HTMLTextAreaElement ||
      t instanceof HTMLSelectElement ||
      (t instanceof HTMLElement && t.isContentEditable)
    ) {
      return;
    }
    if (e.key === 'v' || e.key === 'V') {
      canvasMode = 'select';
    } else if (e.key === 'h' || e.key === 'H') {
      canvasMode = 'pan';
    } else if (e.key === 'Escape') {
      canvasMode = 'pan';
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleKeydown);
    return () => window.removeEventListener('keydown', handleKeydown);
  });
</script>

<!-- the native menu would dismiss the node ContextMenu popover -->
<div class="graph-canvas-flow" bind:clientWidth={paneWidth} bind:clientHeight={paneHeight} oncontextmenu={(e) => e.preventDefault()}>
  <SvelteFlowProvider>
    <SvelteFlow
      bind:nodes={flowNodes}
      bind:edges={flowEdges}
      {nodeTypes}
      {edgeTypes}
      onnodeclick={({ event, node }) => onnodetap?.(node as Node<GraphNodeData>, event as MouseEvent)}
      onpaneclick={() => onbackgroundtap?.()}
      onedgeclick={({ edge }) => onedgetap?.(edge)}
      onnodecontextmenu={({ event, node }) => {
        (event as MouseEvent).preventDefault();
        oncontextmenu?.(event as MouseEvent, node as Node<GraphNodeData>);
      }}
      ondelete={({ nodes }) => onnodesdelete?.(nodes as unknown as Node<GraphNodeData>[])}
      onselectionchange={({ nodes }) => onselectionchange?.(nodes as unknown as Node<GraphNodeData>[])}
      onnodepointerenter={({ node }) => onnodemouseenter?.(node as Node<GraphNodeData>)}
      onnodepointerleave={({ node }) => onnodemouseleave?.(node as Node<GraphNodeData>)}
      onedgepointerenter={({ edge, event }) => onedgemouseenter?.(edge, event as MouseEvent)}
      onedgepointerleave={({ edge }) => onedgemouseleave?.(edge)}
      oninit={() => {
        const { fitView, getViewport, setViewport } = useSvelteFlow();
        flowFitView = fitView;
        view = { getViewport, setViewport };
        onready?.({ fitView });
      }}
      {minZoom}
      maxZoom={4}
      colorMode="dark"
      panOnDrag={panOnDragProp}
      selectionOnDrag={selectionOnDragProp}
      selectionMode={SelectionMode.Partial}
      multiSelectionKey="Shift"
      deleteKey={deleteKeysProp}
    >
      <Background />
      <Controls />
      {#if tools}
        <Panel position="top-left">
          <div class="bg-surface border border-border rounded-sm p-1 flex gap-1 shadow-[0_4px_16px_var(--color-shadow)]">
            <button
              type="button"
              title="Pan mode (H)"
              aria-label="Pan mode"
              aria-pressed={canvasMode === 'pan'}
              onclick={() => (canvasMode = 'pan')}
              class="flex items-center justify-center w-7 h-7 rounded-sm border-none cursor-pointer transition-colors {canvasMode === 'pan' ? 'bg-accent text-surface' : 'bg-surface-alt text-text-muted hover:bg-hover hover:text-accent-hover'}"
            >
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M18 11V6a2 2 0 0 0-4 0v5" />
                <path d="M14 10V4a2 2 0 0 0-4 0v7" />
                <path d="M10 10.5V6a2 2 0 0 0-4 0v9" />
                <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2a8 8 0 0 1-7.92-6.96c-.2-1.58-.1-2.56 1.05-3.25a2 2 0 0 1 2.87 1.21" />
              </svg>
            </button>
            <button
              type="button"
              title="Select mode (V)"
              aria-label="Select mode"
              aria-pressed={canvasMode === 'select'}
              onclick={() => (canvasMode = 'select')}
              class="flex items-center justify-center w-7 h-7 rounded-sm border-none cursor-pointer transition-colors {canvasMode === 'select' ? 'bg-accent text-surface' : 'bg-surface-alt text-text-muted hover:bg-hover hover:text-accent-hover'}"
            >
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M3 3l7.5 18 2.25-7.5L20.25 11.25 3 3z" />
                <path d="M13 13l6 6" />
              </svg>
            </button>
          </div>
        </Panel>
      {/if}
    </SvelteFlow>
  </SvelteFlowProvider>
</div>

<style>
  .graph-canvas-flow {
    width: 100%;
    height: 100%;
  }
  :global(.svelte-flow .svelte-flow__edge-text) {
    font-size: 12px;
    fill: var(--color-accent-hover);
  }

  :global(.svelte-flow__node.selected) {
    outline: 2px solid var(--color-accent);
    outline-offset: 3px;
    border-radius: 10px;
  }
</style>
