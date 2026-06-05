<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { goto } from '$app/navigation';
  import type { Node, Edge } from '@xyflow/svelte';
  import {
    getProjectMap,
    getProjectDepgraph,
    type ProjectMap,
    type ProjectDepGraph,
  } from '$lib/api/rest';
  import { setSearchContext } from '$lib/stores/search.svelte';
  import { togglePalette, setPaletteCommands, clearPaletteCommands } from '$lib/stores/palette.svelte';
  import type { Command } from '$lib/commands/registry';
  import GraphCanvasFlow from '$lib/components/contract/GraphCanvasFlow.svelte';
  import {
    setNodes, setEdges, getNodes, getEdges, clearGraph,
    type GraphNodeData,
  } from '$lib/stores/graph.svelte';
  import { runDagreLayout } from '$lib/utils/graph-helpers';

  const ROOT_COLOR = '#8b949e';
  const EDGE_COLOR: Record<string, string> = {
    inherits: '#d2a8ff',
    calls: '#ffa657',
    holds: '#56d4dd',
  };
  const NODE_KINDS = ['contract', 'abstract', 'interface', 'library'] as const;
  const EDGE_KINDS = ['inherits', 'calls', 'holds'] as const;

  let projectMap: ProjectMap | null = $state(null);
  let dep: ProjectDepGraph | null = $state(null);
  let error: string | null = $state(null);
  let legendFolders: { folder: string; color: string }[] = $state([]);
  let nodeFilter = $state<Record<string, boolean>>({ contract: true, abstract: true, interface: true, library: true });
  let edgeFilter = $state<Record<string, boolean>>({ inherits: true, calls: true, holds: true });
  let fitView: ((opts?: any) => Promise<boolean>) | null = null;
  let hoveredId: string | null = null;

  onMount(async () => {
    setSearchContext(null);
    try {
      [projectMap, dep] = await Promise.all([getProjectMap(), getProjectDepgraph()]);
      buildGraph(dep);
    } catch (e) {
      error = 'Failed to connect. Is "ilold serve" running?';
    }
  });

  function topLevel(folder: string): string {
    return folder === '' ? '' : folder.split('/')[0];
  }

  function colorMap(folders: string[]): Map<string, string> {
    const groups = [...new Set(folders.map(topLevel))].sort();
    const nonRoot = groups.filter((g) => g !== '');
    const map = new Map<string, string>();
    nonRoot.forEach((g, i) => {
      const hue = Math.round((i * 360) / Math.max(nonRoot.length, 1));
      map.set(g, `hsl(${hue}, 60%, 66%)`);
    });
    if (groups.includes('')) map.set('', ROOT_COLOR);
    return map;
  }

  function buildGraph(d: ProjectDepGraph) {
    const colors = colorMap(d.nodes.map((n) => n.data.folder));
    legendFolders = [...colors.entries()]
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([group, color]) => ({ folder: group === '' ? '(root)' : group, color }));

    const nodes: Node<GraphNodeData>[] = d.nodes.map((n) => ({
      id: n.data.id,
      type: 'contract',
      position: { x: 0, y: 0 },
      data: {
        _type: 'contract',
        label: n.data.label,
        kind: n.data.kind,
        folder: n.data.folder,
        layer: n.data.layer,
        color: colors.get(topLevel(n.data.folder)) ?? ROOT_COLOR,
      },
    }));
    const edges: Edge[] = d.edges.map((e) => ({
      id: e.data.id,
      source: e.data.source,
      target: e.data.target,
      sourceHandle: 'r',
      targetHandle: 'l',
      data: { kind: e.data.kind, kinds: e.data.kinds, stroke: EDGE_COLOR[e.data.kind] ?? ROOT_COLOR },
    }));
    const laid = runDagreLayout(nodes, edges, {
      rankDir: 'LR',
      nodeWidth: 170,
      nodeHeight: 54,
      nodeSep: 28,
      rankSep: 110,
    });
    setNodes(laid);
    setEdges(edges);
    applyVisual();
    queueMicrotask(() => fitView?.({ padding: 0.15 }));
  }

  function applyVisual() {
    const neighbors = new Set<string>();
    if (hoveredId) {
      neighbors.add(hoveredId);
      for (const e of getEdges()) {
        if (e.source === hoveredId) neighbors.add(e.target);
        if (e.target === hoveredId) neighbors.add(e.source);
      }
    }

    setNodes(getNodes().map((n) => {
      const d = n.data as GraphNodeData & { kind: string };
      const kindOn = nodeFilter[d.kind] ?? true;
      const dimmed = !kindOn || (hoveredId != null && !neighbors.has(n.id));
      return { ...n, data: { ...n.data, _dimmed: dimmed } };
    }));

    setEdges(getEdges().map((e) => {
      const kinds: string[] = (e.data?.kinds as string[]) ?? [];
      const stroke = (e.data?.stroke as string) ?? ROOT_COLOR;
      const visible = kinds.some((k) => edgeFilter[k] ?? true);
      const incident = hoveredId != null && (e.source === hoveredId || e.target === hoveredId);
      const opacity = !hoveredId ? 1 : incident ? 1 : 0.05;
      const width = incident ? 2.5 : 1.5;
      return {
        ...e,
        hidden: !visible,
        animated: incident,
        style: `stroke: ${stroke}; stroke-width: ${width}; opacity: ${opacity}`,
      };
    }));
  }

  function toggleNode(kind: string) {
    nodeFilter = { ...nodeFilter, [kind]: !nodeFilter[kind] };
    applyVisual();
  }
  function toggleEdge(kind: string) {
    edgeFilter = { ...edgeFilter, [kind]: !edgeFilter[kind] };
    applyVisual();
  }

  function onHover(node: Node<GraphNodeData>) {
    hoveredId = node.id;
    applyVisual();
  }
  function offHover() {
    hoveredId = null;
    applyVisual();
  }

  function openContract(node: Node<GraphNodeData>) {
    goto(`/contract/${encodeURIComponent(node.id)}`);
  }

  $effect(() => {
    if (!projectMap) {
      setPaletteCommands([]);
      return;
    }
    const seen = new Set<string>();
    const cmds: Command[] = [];
    for (const c of projectMap.contracts) {
      if (seen.has(c.name)) continue;
      seen.add(c.name);
      cmds.push({
        id: `contract:${c.name}`,
        label: c.name,
        category: 'Contract' as const,
        icon: '◈',
        detail: c.kind,
        keywords: ['contract', 'open', 'navigate'],
        run: () => goto(`/contract/${encodeURIComponent(c.name)}`),
      });
    }
    setPaletteCommands(cmds);
  });

  onDestroy(() => {
    clearPaletteCommands();
    clearGraph();
  });
</script>

<div class="fixed inset-0 flex flex-col bg-dark">
  <div class="flex items-center gap-2.5 px-4 py-2 bg-hover border-b border-border-subtle z-10 shrink-0">
    <span class="text-lg font-bold text-text">ilold</span>
    <span class="text-xs text-text-dim">execution path analyzer</span>
    {#if dep}
      <span class="text-xs text-text-muted">{dep.nodes.length} contracts · {dep.edges.length} relationships</span>
    {/if}
    <div class="ml-auto flex gap-1">
      <button class="bg-hover border border-border-subtle text-accent-hover px-3 py-1 rounded-sm cursor-pointer text-xs hover:border-accent" onclick={togglePalette}>⌘K Search</button>
    </div>
  </div>

  {#if error}
    <div class="p-6 text-danger">{error}</div>
  {:else if !dep}
    <div class="p-6 text-text-muted">Analyzing...</div>
  {:else if dep.nodes.length === 0}
    <div class="p-6 text-text-muted">No contracts found in this project.</div>
  {:else}
    <div class="flex-1 min-h-0 relative">
      <GraphCanvasFlow
        onnodetap={openContract}
        onnodemouseenter={onHover}
        onnodemouseleave={offHover}
        canDeleteNodes={false}
        onready={(api) => { fitView = api.fitView; queueMicrotask(() => api.fitView({ padding: 0.15 })); }}
      />

      <div
        class="absolute top-3 right-3 z-10 flex flex-col gap-1.5 px-3 py-2.5 text-[11px]"
        style="
          border-radius: 10px;
          border: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
          background: linear-gradient(180deg, rgba(30, 30, 40, 0.88) 0%, rgba(24, 24, 30, 0.92) 100%);
          backdrop-filter: blur(16px) saturate(1.6);
          -webkit-backdrop-filter: blur(16px) saturate(1.6);
          box-shadow: 0 4px 16px -4px rgba(0, 0, 0, 0.3);
        "
      >
        <div class="flex items-center gap-1.5">
          <span class="text-text-muted uppercase tracking-wide text-[9px] w-12">Relations</span>
          {#each EDGE_KINDS as k}
            <button
              type="button"
              onclick={() => toggleEdge(k)}
              class="px-1.5 py-0.5 rounded-sm border cursor-pointer transition-opacity {edgeFilter[k] ? 'border-transparent text-text' : 'border-border-subtle text-text-dim opacity-50'}"
              style={edgeFilter[k] ? `background: color-mix(in srgb, ${EDGE_COLOR[k]} 22%, transparent); color: ${EDGE_COLOR[k]}` : ''}
            >{k}</button>
          {/each}
        </div>
        <div class="flex items-center gap-1.5">
          <span class="text-text-muted uppercase tracking-wide text-[9px] w-12">Kind</span>
          {#each NODE_KINDS as k}
            <button
              type="button"
              onclick={() => toggleNode(k)}
              class="px-1.5 py-0.5 rounded-sm border cursor-pointer {nodeFilter[k] ? 'border-accent text-text' : 'border-border-subtle text-text-dim opacity-50'}"
            >{k}</button>
          {/each}
        </div>
      </div>

      <div
        class="absolute bottom-3 left-4 z-10 max-w-[340px] max-h-[42%] overflow-y-auto text-[11px] px-3 py-2.5"
        style="
          border-radius: 10px;
          border: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
          background: linear-gradient(180deg, rgba(30, 30, 40, 0.88) 0%, rgba(24, 24, 30, 0.92) 100%);
          backdrop-filter: blur(16px) saturate(1.6);
          -webkit-backdrop-filter: blur(16px) saturate(1.6);
          box-shadow: 0 4px 16px -4px rgba(0, 0, 0, 0.3);
        "
      >
        <div class="text-text-muted uppercase tracking-wide text-[9px] mb-1 font-semibold">Kind</div>
        <div class="flex flex-wrap gap-x-3 gap-y-1 mb-2 text-text-dim">
          <span class="border-[1.5px] border-solid border-text-muted rounded px-1">contract</span>
          <span class="border-[1.5px] border-dashed border-text-muted rounded px-1">abstract</span>
          <span class="border-[1.5px] border-dotted border-text-muted rounded px-1">interface</span>
          <span class="border-[3px] border-double border-text-muted rounded px-1">library</span>
        </div>
        <div class="text-text-muted uppercase tracking-wide text-[9px] mb-1 font-semibold">Subsystem</div>
        <div class="flex flex-wrap gap-x-3 gap-y-1">
          {#each legendFolders as f}
            <span class="flex items-center gap-1"><span class="inline-block w-2.5 h-2.5 rounded-sm" style="background: {f.color}"></span>{f.folder}</span>
          {/each}
        </div>
      </div>
    </div>
    <div class="px-4 py-1.5 bg-hover border-t border-border-subtle text-[11px] text-text-muted shrink-0">
      click a contract to open its functions · hover to highlight its relationships · toggle Relations/Kind to filter · ⌘K to search
    </div>
  {/if}
</div>
