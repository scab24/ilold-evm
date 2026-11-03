<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { SvelteFlow, SvelteFlowProvider, Background, Controls, Panel, SelectionMode, useSvelteFlow, type FitViewOptions, type NodeTypes } from '@xyflow/svelte';
  import type { Node, Edge } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';

  import FunctionNode from './nodes/FunctionNode.svelte';
  import BlockNode from './nodes/BlockNode.svelte';
  import SequenceNode from './nodes/SequenceNode.svelte';
  import ContractNode from './nodes/ContractNode.svelte';
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
    /** Fit once, after the pane has a size and every visible node is measured. */
    initialFit?: FitViewOptions;
    minZoom?: number;
  }

  let { onnodetap, onbackgroundtap, oncontextmenu, onready, onnodesdelete, canDeleteNodes = true, onselectionchange, onnodemouseenter, onnodemouseleave, initialFit, minZoom = 0.1 }: Props = $props();

  const deleteKeysProp = $derived<string[] | null>(canDeleteNodes ? ['Delete', 'Backspace'] : null);

  const nodeTypes: NodeTypes = {
    function: FunctionNode,
    block: BlockNode,
    sequence: SequenceNode,
    contract: ContractNode,
  } as unknown as NodeTypes;


  let flowNodes = $state<Node[]>([]);
  let flowEdges = $state<Edge[]>([]);

  $effect(() => {
    const storeNodes = getNodes();
    const liveByID = new Map(untrack(() => flowNodes).map(n => [n.id, n]));
    flowNodes = storeNodes.map(sn => {
      const live = liveByID.get(sn.id);
      if (live?.position) {
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

  // a fit before layout clamps to minZoom, so wait for real pane dimensions and measured nodes
  $effect(() => {
    if (initialFitDone || !initialFit || !flowFitView || paneWidth === 0 || paneHeight === 0) return;
    const visible = flowNodes.filter((n) => !n.hidden);
    if (visible.length === 0 || !visible.every((n) => n.measured?.width && n.measured?.height)) return;
    initialFitDone = true;
    const fit = flowFitView;
    const options = initialFit;
    requestAnimationFrame(() => { fit(options); });
  });

  export function getLiveNode(id: string): Node | undefined {
    return flowNodes.find(n => n.id === id);
  }

  let canvasMode = $state<'pan' | 'select'>('pan');
  let panOnDragProp = $derived<boolean | number[]>(canvasMode === 'pan' ? true : [1, 2]);
  let selectionOnDragProp = $derived(canvasMode === 'select');

  function handleKeydown(e: KeyboardEvent) {
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

<!--
  oncontextmenu on the wrapper suppresses the NATIVE browser context menu
  for the whole canvas (pane + nodes + controls). Without this, right-clicking
  a node would fire onnodecontextmenu AND the browser's native menu, which
  immediately dismisses our custom ContextMenu popover.
-->
<div class="graph-canvas-flow" bind:clientWidth={paneWidth} bind:clientHeight={paneHeight} oncontextmenu={(e) => e.preventDefault()}>
  <SvelteFlowProvider>
    <!--
      Canvas modes (toggled via the top-left Panel toolbar or V/H shortcuts):
      - `pan`    → left-drag pans; lasso disabled.
      - `select` → left-drag draws a rectangular lasso (Excalidraw-style);
                   middle/right-drag still pans.
      Shared in both modes:
      - shift+click on a node    → add/remove from selection (multiSelectionKey)
      - drag any selected node   → move the whole group together (native)
      - partial overlap counts   → lasso only needs to touch a node (selectionMode)
      - right-click on a node    → branch/context menu (onnodecontextmenu)
    -->
    <SvelteFlow
      bind:nodes={flowNodes}
      bind:edges={flowEdges}
      {nodeTypes}
      onnodeclick={({ event, node }) => onnodetap?.(node as Node<GraphNodeData>, event as MouseEvent)}
      onpaneclick={() => onbackgroundtap?.()}
      onnodecontextmenu={({ event, node }) => {
        (event as MouseEvent).preventDefault();
        oncontextmenu?.(event as MouseEvent, node as Node<GraphNodeData>);
      }}
      ondelete={({ nodes }) => onnodesdelete?.(nodes as unknown as Node<GraphNodeData>[])}
      onselectionchange={({ nodes }) => onselectionchange?.(nodes as unknown as Node<GraphNodeData>[])}
      onnodepointerenter={({ node }) => onnodemouseenter?.(node as Node<GraphNodeData>)}
      onnodepointerleave={({ node }) => onnodemouseleave?.(node as Node<GraphNodeData>)}
      oninit={() => {
        const { fitView } = useSvelteFlow();
        flowFitView = fitView;
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
    </SvelteFlow>
  </SvelteFlowProvider>
</div>

<style>
  .graph-canvas-flow {
    width: 100%;
    height: 100%;
  }
  :global(.svelte-flow .svelte-flow__edge-text) {
    font-size: 11px;
    fill: var(--color-accent-hover);
  }

  :global(.svelte-flow__node.selected) {
    outline: 2px solid var(--color-accent);
    outline-offset: 3px;
    border-radius: 10px;
  }
</style>
