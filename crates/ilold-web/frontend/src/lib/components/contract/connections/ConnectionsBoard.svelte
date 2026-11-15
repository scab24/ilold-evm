<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import type { Edge, Node } from '@xyflow/svelte';
  import type { Connections, ContractDetail } from '$lib/api/rest';
  import { contractConnections, contractDetail } from '$lib/stores/project-data';
  import { getEdges, getNodes, setEdges, setNodes, type GraphNodeData } from '$lib/stores/graph.svelte';
  import { restricts } from '$lib/utils/access';
  import { functionIndex } from '$lib/utils/function-keys';
  import Tooltip from '$lib/components/chrome/Tooltip.svelte';
  import type { BoxNodeData } from '$lib/components/project/BoxNode.svelte';
  import { READABLE_FIT } from '$lib/components/project/chips';
  import { LIBRARIES, OUTSIDE } from '$lib/components/project/map-graph';
  import { build, linksOf, rowLabel, type BoardEdge, type BoardNode, type Link } from './board-links';
  import { boxHeight } from '$lib/components/project/box-size';
  import { boardGraph, isBoardEdge, place, placeLabels, shapeOf, type Facts, type Known, type RowRef } from './board-graph';
  import { closeBox, contractOf, keyParts, openStep, popStep, touch } from './board-state';
  import { applyBoard, currentBoard, currentLines, openMain } from './board-view.svelte';

  let { main, libraries, live, frame, reveal }: { main: string; libraries: Set<string>; live: (id: string) => Node | undefined; frame: (ids: string[], fallback: string, zoom: number, duration?: number) => void; reveal: (id: string) => void } = $props();

  const board = $derived(currentBoard(main));
  const lines = $derived(currentLines());
  const conns = new SvelteMap<string, Connections>();
  const details = new SvelteMap<string, ContractDetail>();
  const failed = new SvelteMap<string, string>();
  const detailFailed = new SvelteMap<string, string>();
  let layer = $state<HTMLDivElement | null>(null);
  let tip = $state<{ x: number; y: number; wire: BoardEdge; dim: boolean } | null>(null);

  $effect(() => {
    for (const c of board.open) {
      if (failed.has(c)) continue;
      if (!conns.has(c)) contractConnections(c).then((v) => conns.set(c, v), (e) => failed.set(c, e instanceof Error ? e.message : String(e)));
      if (!details.has(c) && !detailFailed.has(c)) contractDetail(c).then((v) => details.set(c, v), (e) => detailFailed.set(c, e instanceof Error ? e.message : String(e)));
    }
  });

  const lonely = $derived.by(() => {
    const c = conns.get(main);
    return !!c && !c.outgoing.length && !c.incoming.length && !c.unresolved.length;
  });
  const links = $derived(linksOf(board.open.map((c) => conns.get(c)).filter((c): c is Connections => !!c), libraries));
  const indexes = $derived(new Map([...details].map(([c, d]) => [c, functionIndex(d.functions, d.inherited_functions)])));
  const keysOf = (c: string) => [...new Set(indexes.get(c)?.values() ?? [])].map((f) => f.key);
  function factsOf(c: string, m: string): Facts | undefined {
    const f = indexes.get(c)?.get(m);
    if (!f) return undefined;
    const restricted = (typeof f.access === 'object' && 'Restricted' in f.access) || f.guards.some(restricts);
    return { entry: f.visibility === 'External' || f.visibility === 'Public', restricted, value: !!f.value_transfers?.length };
  }

  let hot = $state<RowRef | null>(null);
  let rowTip = $state<{ x: number; y: number } | null>(null);
  const built = $derived(!conns.has(main) || (lonely && !board.all) ? null : build(board, links, lines, keysOf));
  const scene = $derived.by(() => {
    if (!built) return null;
    const graph = boardGraph(built, board, lines, factsOf, keysOf, hot);
    for (const n of graph.nodes) {
      const missing = n.id.startsWith('O:') ? detailFailed.get(n.data.label) : undefined;
      if (missing) n.data = { ...n.data, lines: [...(n.data.lines ?? []), { text: `Function details did not load: ${missing}`, tone: 'amb' }] };
    }
    return { built, graph };
  });

  let shown: string[] | null = null;
  let picked: string | null = null;
  let placedShape: string | null = null;
  let placedLayout: string | null = null;
  let shownAll = false;

  onDestroy(() => {
    setNodes(getNodes().filter((n) => n.data._type !== 'box'));
    setEdges(getEdges().filter((e) => !isBoardEdge(e.id)));
  });

  $effect(() => {
    const s = scene;
    untrack(() => {
      const others = getNodes().filter((n) => n.data._type !== 'box');
      const otherEdges = getEdges().filter((e) => !isBoardEdge(e.id));
      if (!s) {
        setNodes(others);
        setEdges(otherEdges);
        placedShape = null;
        return;
      }
      const stored = new Map(getNodes().map((n) => [n.id, n.position]));
      const shape = `${board.layout}|${shapeOf(s.graph)}`;
      if (shape === placedShape) {
        const data = new Map(s.graph.nodes.map((n) => [n.id, n.data]));
        setNodes(getNodes().map((n) => (data.has(n.id) ? { ...n, data: data.get(n.id)! } : n)));
        setEdges([...otherEdges, ...placeLabels(s.graph, (id) => live(id)?.position ?? stored.get(id))]);
        if (board.fn && board.fn !== picked) reveal(`O:${contractOf(board.fn)}`);
        picked = board.fn;
        return;
      }
      const relaid = placedLayout !== null && placedLayout !== board.layout;
      placedShape = shape;
      placedLayout = board.layout;
      const known = (id: string): Known | null => {
        if (relaid) return null;
        const n = live(id);
        const at = stored.get(id);
        if (!n || !at || (n.data as GraphNodeData)._type !== 'box') return null;
        const data = n.data as BoxNodeData;
        const { x, y } = n.position;
        return { x, y, w: n.measured?.width ?? data.width ?? 0, h: n.measured?.height ?? boxHeight(data), pinned: x !== at.x || y !== at.y, at };
      };
      const placed = place(s.built, board, s.graph, known);
      const at = new Map(placed.map((n) => [n.id, n.position]));
      setNodes([...others, ...(placed as Node<GraphNodeData>[])]);
      setEdges([...otherEdges, ...placeLabels(s.graph, (id) => { const k = known(id); return k?.pinned ? k : at.get(id); })]);
      const ready = board.open.filter((c) => conns.has(c));
      const opened = ready.filter((c) => !shown?.includes(c));
      if (!shown || board.all !== shownAll || relaid) frame(s.graph.nodes.map((n) => n.id), `O:${main}`, READABLE_FIT.minZoom, shown ? 300 : 0);
      else if (opened.length) frame(opened.flatMap((c) => [`O:${c}`, ...(board.parent[c] ? [`O:${board.parent[c]}`] : [])]), `O:${opened.at(-1)}`, READABLE_FIT.minZoom, 300);
      else if (board.fn && board.fn !== picked) reveal(`O:${contractOf(board.fn)}`);
      shown = ready;
      shownAll = board.all;
      picked = board.fn;
    });
  });

  function selectRow(c: string, key: string) {
    applyBoard({ ...touch(board, c), fn: board.fn === key ? null : key });
  }
  function openFolded(n: BoardNode, row: string | null) {
    if (n.type !== 'folded' || !n.owner) return;
    applyBoard(openStep(board, n.c, n.col, row && !keyParts(row).member.includes('::') ? row : null, n.owner));
  }
  function switchMain(c: string) {
    if (c !== main && c !== OUTSIDE && c !== LIBRARIES) openMain(c);
  }

  export function tap(node: Node, event: MouseEvent) {
    const n = scene?.built.nodes.find((x) => x.id === node.id);
    if (!n) return;
    const t = event.target instanceof Element ? event.target : null;
    const row = t?.closest('[data-row]')?.getAttribute('data-row') ?? null;
    if (t?.closest('[data-close]')) applyBoard(closeBox(board, n.c));
    else if (row && n.type === 'open') selectRow(n.c, row);
    else if (row) openFolded(n, row);
    else if (n.type === 'collapsed') applyBoard(touch(board, n.c));
    else if (n.type === 'folded') openFolded(n, null);
    else if (n.type === 'open' && (event.detail === 2 || (event.detail === 0 && t?.closest('[data-head]')))) switchMain(n.c);
  }

  export function clear() {
    if (board.fn) applyBoard({ ...board, fn: null });
  }

  export function hover(edge: Edge | null, event?: MouseEvent) {
    const wire = edge ? scene?.graph.wires.get(edge.id) : undefined;
    tip = wire && event ? { x: event.clientX, y: event.clientY, wire, dim: edge!.id.startsWith('cd:') } : null;
  }

  function onKey(event: KeyboardEvent) {
    const t = event.target instanceof Element ? event.target : null;
    if (t?.closest('input, select, textarea, [contenteditable="true"]')) return;
    if (event.key === 'Backspace') {
      const next = popStep(board);
      event.preventDefault();
      if (next) applyBoard(next);
    } else if (event.key === 'Escape' && !event.defaultPrevented) clear();
  }

  const target = (l: Link) => (l.to === OUTSIDE ? rowLabel(l.toKey, '') : `${l.to === LIBRARIES ? '' : `${l.to}.`}${rowLabel(l.toKey, l.to)}`);
  const capped = (list: string[]) => list.slice(0, 6).concat(list.length > 6 ? [`and ${list.length - 6} more`] : []);

  function tipLines(w: BoardEdge): string[] {
    return capped(w.links.map((l) => `${rowLabel(l.fromKey, l.from)} → ${target(l)} · line ${l.row.span.start_line}${l.value ? ' · moves value' : ''}`));
  }

  function rowLines(r: RowRef): string[] {
    const touching = (built?.edges ?? []).filter((e) => (e.a === r.id && e.ar === r.row) || (e.b === r.id && e.br === r.row));
    return capped(
      touching.flatMap((e) =>
        e.links.map((l) => (e.a === r.id && e.ar === r.row ? `→ ${target(l)} · line ${l.row.span.start_line}` : `← ${l.from}.${rowLabel(l.fromKey, l.from)} · line ${l.row.span.start_line}`)),
      ),
    );
  }

  function onPointer(event: PointerEvent) {
    const t = event.target instanceof Element ? event.target : null;
    const hit = t?.closest('.svelte-flow__node [data-row]');
    const id = hit?.closest('.svelte-flow__node')?.getAttribute('data-id') ?? null;
    const row = hit?.getAttribute('data-row') ?? null;
    if (!id || !row) {
      if (hot) hot = null;
      rowTip = null;
      return;
    }
    if (hot?.id !== id || hot.row !== row) hot = { id, row };
    const r = hit!.getBoundingClientRect();
    rowTip = { x: r.left, y: r.bottom };
  }
</script>

<svelte:window onkeydown={onKey} onpointerover={onPointer} />

<div class="board-layer" bind:this={layer}>
  {#if failed.has(main) && !conns.has(main)}
    <div class="empty-state" role="alert">
      <div class="empty-title">Could not load the connections of {main}</div>
      <div class="empty-hint err">{failed.get(main)}</div>
      <button type="button" class="chip" onclick={() => failed.delete(main)}>Retry</button>
    </div>
  {:else if !conns.has(main)}
    <div class="empty-state"><div class="empty-hint">Loading connections…</div></div>
  {:else if lonely && !board.all}
    <div class="empty-state">
      <div class="empty-icon" aria-hidden="true">◇</div>
      <div class="empty-title">No connections</div>
      <div class="empty-hint">{main} calls no other contract and no other contract calls it.</div>
      <button type="button" class="chip" onclick={() => applyBoard({ ...board, all: true })}>Show all its functions</button>
    </div>
  {/if}
</div>

{#if hot && rowTip && rowLines(hot).length}
  <Tooltip x={rowTip.x} y={rowTip.y} within={layer}>{#each rowLines(hot) as line, i (i)}{i ? '\n' : ''}{line}{/each}</Tooltip>
{:else if tip}
  <Tooltip x={tip.x} y={tip.y} within={layer}>{#each tipLines(tip.wire) as line, i (i)}{i ? '\n' : ''}{line}{/each}{#if tip.dim}{'\n'}Dimmed in the legend: set it to shown to see each function{/if}</Tooltip>
{/if}

<style>
  .board-layer { position: absolute; inset: 0; z-index: 5; pointer-events: none; display: flex; align-items: center; justify-content: center; }
  .board-layer > :global(*) { pointer-events: auto; }
</style>
