<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { MarkerType, type Edge, type FitViewOptions, type Node } from '@xyflow/svelte';
  import { type Basis, type ContractCall, type ContractSummary, type ProjectConnections, type ProjectDepGraph } from '$lib/api/rest';
  import { setNodes, setEdges, clearGraph, type GraphNodeData } from '$lib/stores/graph.svelte';
  import { projectConnections, projectDepgraph } from '$lib/stores/project-data';
  import { BASIS_LABEL, lineStyle, splitRef } from '$lib/utils/links';
  import { openInIde } from '$lib/utils/ide-links';
  import { setQuery } from '$lib/utils/query';
  import { score } from '$lib/utils/fuzzy';
  import GraphCanvasFlow from '$lib/components/contract/GraphCanvasFlow.svelte';
  import BoxNode, { type BoxNodeData } from './BoxNode.svelte';
  import RegionNode, { type RegionNodeData } from './RegionNode.svelte';
  import ChipEdge, { type ChipEdgeData } from './ChipEdge.svelte';
  import Icon from './Icon.svelte';
  import Tooltip from './Tooltip.svelte';
  import { selectionKeys } from './selection-keys';
  import Row from './Row.svelte';
  import { READABLE_FIT, addPort, anchors, placeChips, sidesBetween, type Port } from './chips';
  import {
    DEFAULT_LINES, INTERFACES, LIBRARIES, LINES, OUTSIDE,
    edgeStyle, edges as mapEdges, itemState, items as mapItems, neighbourhood, nextState, packageOf, shortName,
    type Item, type Line, type LineState, type MapEdge,
  } from './map-graph';
  import { endpointOf, groups as mapGroups, layout, type Fold } from './map-layout';

  let { contracts }: { contracts: ContractSummary[] } = $props();

  const ACCENT = 'var(--color-accent-hover)';
  const FOLDS: Record<string, Fold> = { [LIBRARIES]: 'libraries', [INTERFACES]: 'interfaces', [OUTSIDE]: 'outside' };

  let connections = $state<ProjectConnections | null>(null);
  let depgraph = $state<ProjectDepGraph | null>(null);
  let error = $state<string | null>(null);
  let hovered = $state<string | null>(null);
  let tip = $state<{ x: number; y: number; edge: MapEdge } | null>(null);
  let find = $state('');
  let missed = $state<string | null>(null);
  let fit: ((options?: FitViewOptions) => Promise<boolean>) | null = null;
  let canvasEl = $state<HTMLElement | null>(null);

  const params = $derived(page.url.searchParams);
  const depth = $derived(params.get('depth') === '2' ? 2 : 1);
  const direction = $derived((['callers', 'callees'] as const).find((d) => d === params.get('dir')) ?? 'both');
  const open = $derived(new Set((params.get('expand') ?? '').split(',').filter((f): f is Fold => f === 'libraries' || f === 'interfaces' || f === 'outside')));
  const lines = $derived.by(() => {
    const state = { ...DEFAULT_LINES };
    for (const entry of (params.get('lines') ?? '').split(',')) {
      const [line, value] = entry.split('-') as [Line, LineState];
      if (line in state && (value === 'show' || value === 'dim' || (value === 'hide' && line !== 'unresolved'))) state[line] = value;
    }
    return state;
  });

  const byName = $derived(new Map(contracts.map((c) => [c.name, c])));
  const summary = $derived(new Map((connections?.contracts ?? []).map((c) => [c.contract, c])));
  const all = $derived(connections ? mapItems(contracts, connections, depgraph) : ([] as Item[]));
  const g = $derived(connections ? mapGroups(contracts, connections) : null);
  const names = $derived(g ? [...g.folders.values()].flat().concat(g.libraries, g.interfaces, g.outside) : []);
  const endpoint = $derived(g ? endpointOf(g, open) : (id: string) => id);
  const drawn = $derived(mapEdges(all, endpoint, lines));
  const selected = $derived.by(() => {
    const c = params.get('c');
    return c !== null && names.includes(c) ? c : null;
  });
  const pair = $derived(drawn.find((e) => e.id === params.get('pair')) ?? null);
  const pickedPair = $derived(pair?.id ?? null);
  const hood = $derived(selected ? neighbourhood(drawn, endpoint(selected), depth, direction) : null);
  const folderIds = $derived([...(g?.folders.keys() ?? [])].map((f) => `folder:${f}`));
  const firstFit = $derived<FitViewOptions>({ ...READABLE_FIT, nodes: (hood ? [...hood.keys()] : folderIds).map((id) => ({ id })) });

  onMount(async () => {
    try {
      [connections, depgraph] = await Promise.all([projectConnections(), projectDepgraph()]);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  });
  onDestroy(clearGraph);

  function label(id: string): string {
    if (id === OUTSIDE) return 'Outside the project';
    if (id === LIBRARIES) return 'Libraries';
    if (id === INTERFACES) return 'Interfaces';
    return shortName(id);
  }

  const clash = $derived.by(() => {
    const seen = new Map<string, number>();
    for (const n of names) seen.set(shortName(n), (seen.get(shortName(n)) ?? 0) + 1);
    return new Set([...seen].filter(([, n]) => n > 1).map(([name]) => name));
  });
  function named(id: string): string {
    return clash.has(label(id)) && !byName.has(id) ? `${label(id)} (${packageOf(id)})` : label(id);
  }

  function boxLines(id: string): BoxNodeData['lines'] {
    if (!g || !connections) return [];
    if (id === OUTSIDE) return [{ text: `${g.outside.length} targets · ${connections.totals.unresolved} unresolved calls`, tone: 'amb' }];
    if (id === LIBRARIES) return [{ text: `${g.libraries.length} libraries · click to show` }];
    if (id === INTERFACES) return [{ text: `${g.interfaces.length} interfaces · click to show` }];
    if (id.startsWith('out:')) return [{ text: 'outside the project', tone: 'amb' }];
    const c = byName.get(id);
    if (!c) return [{ text: clash.has(shortName(id)) ? `library from ${packageOf(id)}` : 'library from a dependency' }];
    if (c.kind !== 'Contract') return [{ text: c.kind.toLowerCase() }];
    const s = summary.get(id);
    if (!s) return [{ text: 'contract' }];
    const result: NonNullable<BoxNodeData['lines']> = [{ text: `${s.entry_points} entry · ${s.restricted} restricted` }];
    if (s.moves_value) result.push({ text: 'moves value', tone: 'val', icon: 'value' });
    return result;
  }

  function variant(id: string): BoxNodeData['variant'] {
    if (id === OUTSIDE || id.startsWith('out:')) return 'out';
    if (id === LIBRARIES || id === INTERFACES) return 'cluster';
    const kind = byName.get(id)?.kind;
    return kind === 'Abstract' ? 'abstract' : kind === 'Interface' ? 'iface' : '';
  }

  function chipText(e: MapEdge, vertical: boolean, aFirst: boolean): string {
    if (!e.back || !e.forward) return String(e.forward + e.back);
    const [first, second] = aFirst ? [e.forward, e.back] : [e.back, e.forward];
    return vertical ? `${first} down · ${second} up` : `${first} → · ${second} ←`;
  }

  const scene = $derived.by(() => {
    if (!g) return null;
    const placed = layout(g, open, all);
    const ports = new Map<string, Port[]>();
    const geometry = new Map<string, { vertical: boolean; aFirst: boolean }>();
    const shown = drawn.filter((e) => placed.nodes.has(e.a) && placed.nodes.has(e.b));
    for (const e of shown) {
      const a = placed.nodes.get(e.a)!;
      const b = placed.nodes.get(e.b)!;
      const vertical = a.column === b.column && a.region === b.region && Math.abs(a.index - b.index) === 1;
      const sides = sidesBetween(a, b, vertical);
      geometry.set(e.id, { vertical, aFirst: vertical ? a.y < b.y : a.x <= b.x });
      addPort(ports, e.a, { id: `${e.id}:a`, side: sides[0], type: 'source', order: sides[0] === 't' || sides[0] === 'b' ? b.x : b.y });
      addPort(ports, e.b, { id: `${e.id}:b`, side: sides[1], type: 'target', order: sides[1] === 't' || sides[1] === 'b' ? a.x : a.y });
    }
    const anchor = anchors(ports, (id) => placed.nodes.get(id)!);
    const lit = new Set(
      shown
        .filter((e) => {
          if (pickedPair) return e.id === pickedPair;
          if (!hood) return true;
          const [a, b] = [hood.get(e.a), hood.get(e.b)];
          return a !== undefined && b !== undefined && Math.abs(a - b) === 1;
        })
        .map((e) => e.id),
    );
    const texts = new Map(shown.map((e) => [e.id, chipText(e, geometry.get(e.id)!.vertical, geometry.get(e.id)!.aFirst)]));
    const boxes = [...placed.nodes.values()].map((p) => ({ x: p.x, y: p.y, w: p.w, h: p.h })).concat(placed.regions.map((r) => ({ x: r.x, y: r.y, w: r.w, h: 28 })));
    const ordered = shown.filter((e) => lit.has(e.id)).sort((x, y) => y.items.length - x.items.length);
    const chipAt = placeChips(ordered.map((e) => ({ id: e.id, s: anchor.get(`${e.id}:a`)!, t: anchor.get(`${e.id}:b`)!, text: texts.get(e.id)! })), boxes);

    const nodes: Node<GraphNodeData>[] = placed.regions.map((r) => ({
      id: r.id,
      type: 'region',
      position: { x: r.x, y: r.y },
      draggable: false,
      selectable: false,
      zIndex: -1,
      data: { _type: 'box', label: r.label, width: r.w, height: r.h, tone: r.tone, onfold: r.fold ? () => toggleFold(r.fold!, false) : undefined } satisfies RegionNodeData,
    }));
    for (const p of placed.nodes.values()) {
      nodes.push({
        id: p.id,
        type: 'box',
        position: { x: p.x, y: p.y },
        draggable: false,
        data: {
          _type: 'box',
          label: label(p.id),
          icon: p.id === LIBRARIES ? 'lib' : p.id === INTERFACES ? 'iface' : p.id === OUTSIDE ? 'unres' : undefined,
          lines: boxLines(p.id),
          ports: ports.get(p.id) ?? [],
          variant: variant(p.id),
          width: p.w,
          height: p.h,
          on: p.id === selected,
          dim: hood !== null && !hood.has(p.id),
          faded: () => near !== null && !near.has(p.id),
        } satisfies BoxNodeData,
      });
    }
    return { shown, lit, texts, chipAt, nodes };
  });

  const near = $derived.by(() => {
    if (hovered === null || !scene) return null;
    const ids = new Set([hovered]);
    for (const e of scene.shown) {
      if (scene.lit.has(e.id) && (e.a === hovered || e.b === hovered)) ids.add(e.a).add(e.b);
    }
    return ids;
  });

  const flowEdges = $derived.by((): Edge[] => {
    if (!scene) return [];
    return scene.shown.map((e) => {
      const style = edgeStyle(e);
      const on = e.id === pickedPair;
      const lit = scene.lit.has(e.id);
      const incident = lit && hovered !== null && (e.a === hovered || e.b === hovered);
      const color = on ? ACCENT : style.color;
      const opacity = on || incident ? 1 : !lit ? 0.07 : hovered !== null ? 0.05 : e.state === 1 ? 0.35 : 1;
      const marker = { type: MarkerType.ArrowClosed, color, width: 14, height: 14 };
      return {
        id: e.id,
        type: 'chip',
        source: e.a,
        target: e.b,
        sourceHandle: `${e.id}:a`,
        targetHandle: `${e.id}:b`,
        markerEnd: e.forward ? marker : undefined,
        markerStart: e.back ? marker : undefined,
        zIndex: on ? 2 : incident ? 1 : undefined,
        animated: incident,
        style: `stroke: ${color}; stroke-width: ${style.width + (on || incident ? 1 : 0)}; opacity: ${opacity}${style.dash ? `; stroke-dasharray: ${style.dash}` : ''}`,
        data: {
          chip: lit ? scene.texts.get(e.id) : undefined,
          tone: e.look === 'unresolved' || e.look === 'ambiguous' ? 'amb' : e.look === 'value' ? 'val' : '',
          on,
          faded: hovered !== null && !on && !incident,
          labelAt: on ? (scene.chipAt.get(e.id) ?? 0.5) : (scene.chipAt.get(e.id) ?? null),
          onpick: pickPair,
        } satisfies ChipEdgeData,
      };
    });
  });

  $effect(() => {
    if (scene) setNodes(scene.nodes);
  });
  $effect(() => setEdges(flowEdges));

  function focusLater(ids: () => string[]) {
    setTimeout(() => {
      const found = ids();
      fit?.({ ...READABLE_FIT, nodes: (found.length ? found : folderIds).map((id) => ({ id })), duration: 250 });
    }, 200);
  }
  function members(fold: Fold): string[] {
    if (!g) return [];
    return fold === 'libraries' ? g.libraries : fold === 'interfaces' ? g.interfaces : g.outside;
  }
  function toggleFold(fold: Fold, expand: boolean) {
    const next = new Set(open);
    if (expand) next.add(fold);
    else next.delete(fold);
    setQuery({ expand: [...next].sort().join(',') || null, pair: null });
    focusLater(() => (expand ? members(fold) : []));
  }
  function select(id: string | null) {
    setQuery({ c: id, pair: null });
  }
  function openContract(id: string) {
    if (byName.has(id)) goto(`/contract/${encodeURIComponent(id)}`);
  }
  function tap(node: Node<GraphNodeData>) {
    if (node.type === 'region') return;
    if (FOLDS[node.id]) return toggleFold(FOLDS[node.id], true);
    if (node.id === selected) return openContract(node.id);
    select(node.id);
  }
  function pickPair(id: string) {
    setQuery({ pair: id === pickedPair ? null : id });
  }
  function setLine(line: Line) {
    const next = { ...lines, [line]: nextState(line, lines[line]) };
    const changed = LINES.filter((l) => next[l.id] !== DEFAULT_LINES[l.id]).map((l) => `${l.id}-${next[l.id]}`);
    setQuery({ lines: changed.join(',') || null, pair: null });
  }
  function tier(id: string): number {
    const kind = byName.get(id)?.kind;
    return kind === undefined ? 2 : kind === 'Library' || kind === 'Interface' ? 1 : 0;
  }
  function search(event: SubmitEvent) {
    event.preventDefault();
    const q = find.trim();
    missed = null;
    if (!q || !g || !connections) return;
    const ranked = names
      .map((n) => ({ n, tier: tier(n), s: score(shortName(n), q) }))
      .filter((r) => r.s >= 0)
      .sort((a, b) => a.tier - b.tier || b.s - a.s || Number(summary.has(b.n)) - Number(summary.has(a.n)) || a.n.localeCompare(b.n));
    const hit = ranked[0]?.n;
    if (!hit) {
      missed = q;
      return;
    }
    const fold = g.libraries.includes(hit) ? 'libraries' : g.interfaces.includes(hit) ? 'interfaces' : g.outside.includes(hit) ? 'outside' : null;
    const expand = fold && !open.has(fold) ? [...open, fold].sort().join(',') : null;
    setQuery({ c: hit, pair: null, ...(expand ? { expand } : {}) });
    focusLater(() => (hood ? [...hood.keys()] : [hit]));
  }
  const onKey = selectionKeys(() => ({
    selected: selected !== null || pickedPair !== null,
    clear: () => setQuery({ c: null, pair: null }),
    open: selected ? () => openContract(selected!) : null,
  }));
  function onEdgeHover(edge: Edge, event: MouseEvent) {
    const e = drawn.find((x) => x.id === edge.id);
    tip = e ? { x: event.clientX, y: event.clientY, edge: e } : null;
  }

  const BASIS_KEY: [Basis, string][] = [['inherits_interface', 'interface'], ['selector_match', 'selector'], ['self', 'this']];

  const KIND_WORD: Record<Item['line'], string> = { calls: 'call', reads: 'read', library: 'library call', inherits: 'inherits', holds: 'holds', unresolved: 'target not resolved' };

  const TALLY: Record<Item['line'], [string, string]> = {
    calls: ['call', 'calls'], reads: ['read', 'reads'], library: ['library call', 'library calls'],
    unresolved: ['unresolved call', 'unresolved calls'], inherits: ['inheritance', 'inheritances'], holds: ['held address', 'held addresses'],
  };
  function tally(line: Item['line'], n: number): string {
    return `${n} ${TALLY[line][n === 1 ? 0 : 1]}`;
  }
  function member(ref: string): string {
    return splitRef(ref).member;
  }
  function rowTitle(item: Item & { row: ContractCall }): string {
    const callee = item.row.callees.find((d) => d.contract === item.to);
    const target = callee ? `${shortName(item.to)}.${member(callee.function)}` : (item.row.target?.replace('::', '.') ?? label(item.to));
    const caller = item.from === selected ? member(item.row.function) : `${item.from}.${member(item.row.function)}`;
    return `${caller} → ${target}`;
  }
  function rowId(item: Item & { row: ContractCall }): string {
    return `${item.from}>${item.to}|${item.row.function}|${item.row.span.start_line}:${item.row.span.start_col}`;
  }
  function rowMeta(item: Item & { row: ContractCall }, tag = true): string {
    const parts = [KIND_WORD[item.line]];
    if (named(item.to) !== label(item.to)) parts.push(`in ${packageOf(item.to)}`);
    const basis = item.row.callees.find((d) => d.contract === item.to)?.basis;
    if (item.iface) parts.push('via interface');
    else if (basis === 'selector_match' || basis === 'self') parts.push(BASIS_LABEL[basis]);
    if (item.row.via) parts.push(`address in ${member(item.row.via)}`);
    if (item.ambiguous) parts.push(tag ? `${item.row.callees.length} possible · AMBIGUOUS` : `${item.row.callees.length} possible`);
    if (item.row.unresolved_reason) parts.push(item.row.unresolved_reason === 'dynamic_address' ? 'the call names no declaration' : `no project contract implements ${shortName(item.to)}`);
    if (item.row.in_try) parts.push('inside try');
    return parts.join(' · ');
  }
  const hasRow = (i: Item): i is Item & { row: ContractCall } => i.row !== null;
  const ORDER: Item['line'][] = ['calls', 'reads', 'library', 'unresolved'];
  const withRow = (list: Item[]) =>
    list
      .filter((i): i is Item & { row: ContractCall } => hasRow(i) && itemState(i, lines) > 0)
      .sort((x, y) => itemState(y, lines) - itemState(x, lines) || ORDER.indexOf(x.line) - ORDER.indexOf(y.line) || x.to.localeCompare(y.to) || x.row.span.start_line - y.row.span.start_line);
  const outgoing = $derived(selected ? withRow(all.filter((i) => i.from === selected && (i.line === 'calls' || i.line === 'reads'))) : []);
  const libraryCalls = $derived(selected ? withRow(all.filter((i) => i.from === selected && i.line === 'library')) : []);
  const incoming = $derived(selected ? withRow(all.filter((i) => i.to === selected)) : []);
  const unresolved = $derived(selected ? withRow(all.filter((i) => i.from === selected && i.line === 'unresolved')) : []);
  const structural = $derived(selected ? all.filter((i) => i.row === null && (i.from === selected || i.to === selected) && itemState(i, lines) > 0) : []);
  const top = $derived([...(connections?.contracts ?? [])].sort((a, b) => b.outgoing_calls + b.incoming_calls - (a.outgoing_calls + a.incoming_calls) || a.contract.localeCompare(b.contract)).slice(0, 5));
  const inherits = $derived(all.filter((i) => i.line === 'inherits').length);
  const holds = $derived(all.filter((i) => i.line === 'holds').length);
  const folderCount = $derived(new Set(contracts.map((c) => c.folder)).size);
  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  function sites(list: (Item & { row: ContractCall })[], of?: number): string {
    const n = new Set(list.map((i) => `${i.row.in_context}|${i.row.function}|${i.row.span.start_line}:${i.row.span.start_col}`)).size;
    return of !== undefined && n < of ? `${n} of ${of} shown` : plural(n, 'call', 'calls');
  }
  function kinds(list: Item[]): string {
    return (['calls', 'reads', 'library', 'unresolved', 'inherits', 'holds'] as const)
      .map((k) => [k, list.filter((i) => i.line === k).length] as const)
      .filter(([, n]) => n > 0)
      .map(([k, n]) => tally(k, n))
      .join(' · ');
  }
</script>

{#snippet rows(list: (Item & { row: ContractCall })[])}
  <ul class="list">
    {#each list as item (rowId(item))}
      <Row
        icon={item.line === 'unresolved' ? 'unres' : item.value ? 'value' : item.line === 'reads' ? 'read' : item.line === 'library' ? 'lib' : 'call'}
        tone={item.line === 'unresolved' || item.ambiguous ? 'amb' : item.value ? 'val' : ''}
        title={rowTitle(item)}
        line={item.row.span.start_line}
        onsource={item.row.file ? () => openInIde(item.row.file!, item.row.span.start_line) : undefined}
      >
        {#snippet meta()}{rowMeta(item)}{#if item.value}{' · '}<span class="val">moves value</span>{/if}{/snippet}
      </Row>
    {/each}
  </ul>
{/snippet}

<svelte:window onkeydown={onKey} />

<div class="toolbar" role="toolbar" aria-label="Map">
  <button type="button" class="crumb crumb-btn" onclick={() => select(null)}>Project</button>
  {#if selected}
    <span class="tlabel">›</span><span class="crumb-here">{label(selected)}</span>
  {:else}
    <span class="tlabel">Select a contract to explore around it</span>
  {/if}
  <div class="seg" role="group" aria-label="Direction">
    {#each [['callers', 'Callers'], ['both', 'Both'], ['callees', 'Callees']] as [id, text] (id)}
      <button type="button" disabled={!selected} title={selected ? undefined : 'Select a contract first'} aria-pressed={selected !== null && direction === id} onclick={() => setQuery({ dir: id === 'both' ? null : id })}>{text}</button>
    {/each}
  </div>
  <span class="tlabel">Depth</span>
  <div class="seg" role="group" aria-label="Depth">
    {#each [1, 2] as d (d)}
      <button type="button" disabled={!selected} title={selected ? undefined : 'Select a contract first'} aria-pressed={selected !== null && depth === d} onclick={() => setQuery({ depth: d === 1 ? null : '2' })}>{d}</button>
    {/each}
  </div>
  <form class="find-form" onsubmit={search}>
    <input class="find" type="search" placeholder="Find a contract" aria-label="Find a contract" bind:value={find} oninput={() => (missed = null)} />
  </form>
  {#if missed}<span class="tlabel amb" role="status">No match for “{missed}”</span>{/if}
</div>

<div class="work">
  <div class="canvas" bind:this={canvasEl}>
    {#if error}
      <div class="state-msg err">{error}</div>
    {:else if !connections}
      <div class="state-msg">Analyzing…</div>
    {:else if contracts.length === 0}
      <div class="state-msg">No contracts found in this project.</div>
    {:else}
      <GraphCanvasFlow
        tools={false}
        extraNodeTypes={{ box: BoxNode, region: RegionNode } as any}
        edgeTypes={{ chip: ChipEdge } as any}
        onnodetap={tap}
        onbackgroundtap={() => (selected || pickedPair) && setQuery({ c: null, pair: null })}
        onedgetap={(e) => pickPair(e.id)}
        onnodemouseenter={(n) => (hovered = n.type === 'box' ? n.id : null)}
        onnodemouseleave={() => (hovered = null)}
        onedgemouseenter={onEdgeHover}
        onedgemouseleave={() => (tip = null)}
        onready={(api) => (fit = api.fitView)}
        canDeleteNodes={false}
        keepPositions={false}
        initialFit={firstFit}
        minZoom={0.1}
      />
    {/if}
  </div>
  <aside class="insp" aria-label="Details">
    {#if pair}
      <h1>{named(pair.a)} {pair.forward && pair.back ? '⇄' : pair.forward ? '→' : '←'} {named(pair.b)}</h1>
      <p class="sub">{kinds(pair.items)}</p>
      <h2><span class="ht">Function to function</span><span class="n">{sites(withRow(pair.items))}</span></h2>
      {@render rows(withRow(pair.items))}
      {#if pair.items.some((i) => i.row === null)}
        <p class="note">{pair.items.filter((i) => i.row === null).map((i) => `${named(i.from)} ${i.line} ${named(i.to)}`).join(' · ')}</p>
      {/if}
      <div class="actions"><button type="button" class="btn" onclick={() => setQuery({ pair: null })}>Back</button></div>
    {:else if selected}
      {@const c = byName.get(selected)}
      {@const n = summary.get(selected)}
      <h1>{label(selected)}</h1>
      <p class="sub">{c ? `${c.folder || 'root'} · ${c.kind.toLowerCase()}` : selected.startsWith('out:') ? 'outside the project' : `library from ${packageOf(selected)}`}</p>
      {#if n}
        <div class="stat"><Icon name="call" /><span class="k">Calls and reads to other contracts</span><span class="v">{n.outgoing_calls}</span></div>
        <div class="stat"><Icon name="entry" /><span class="k">Calls and reads from other contracts</span><span class="v">{n.incoming_calls}</span></div>
        <div class="stat"><Icon name="lock" /><span class="k">Entry points · restricted</span><span class="v">{n.entry_points} · {n.restricted}</span></div>
        {#if n.moves_value}<div class="stat val"><Icon name="value" /><span class="k">Moves value</span></div>{/if}
      {/if}
      {#if c}<div class="actions"><button type="button" class="btn" onclick={() => openContract(selected)}>Open functions <span class="tlabel">Enter</span></button></div>{/if}
      {#if outgoing.length}<h2><Icon name="call" /><span class="ht">Calls and reads out</span><span class="n">{sites(outgoing, n?.outgoing_calls)}</span></h2>{@render rows(outgoing)}{/if}
      {#if incoming.length}<h2><Icon name="entry" /><span class="ht">Called by</span><span class="n">{sites(incoming, n?.incoming_calls)}</span></h2>{@render rows(incoming)}{/if}
      {#if libraryCalls.length}<h2><Icon name="lib" /><span class="ht">Library calls</span><span class="n">{sites(libraryCalls)}</span></h2>{@render rows(libraryCalls)}{/if}
      {#if unresolved.length}<h2 class="amb"><Icon name="unres" /><span class="ht">Unresolved</span><span class="n">{sites(unresolved)}</span></h2>{@render rows(unresolved)}{/if}
      {#if outgoing.length || incoming.length || libraryCalls.length}<p class="note">Each row is one call site; a call with several possible targets is listed under each.</p>{/if}
      {#if structural.length}
        <h2><Icon name="inherit" /><span class="ht">Inherits and holds</span><span class="n">{structural.length}</span></h2>
        <p class="sub">{structural.map((i) => `${named(i.from)} ${i.line} ${named(i.to)}`).join(' · ')}</p>
      {/if}
      {#if !outgoing.length && !incoming.length && !libraryCalls.length && !unresolved.length}<p class="note">No line shown by the legend touches it.</p>{/if}
    {:else}
      <h1>Project</h1>
      <p class="sub">{plural(contracts.length, 'contract', 'contracts')} in {plural(folderCount, 'folder', 'folders')} · {connections?.contracts.length ?? 0} deployable</p>
      <h2><span class="ht">Call sites between contracts</span></h2>
      {#if connections}
        {@const t = connections.totals}
        <div class="stat"><Icon name="call" /><span class="k">Calls</span><span class="v">{t.calls}</span></div>
        <div class="stat"><Icon name="read" /><span class="k">Reads</span><span class="v">{t.reads}</span></div>
        <div class="stat"><Icon name="lib" /><span class="k">Library calls</span><span class="v">{t.library}</span></div>
        <div class="stat"><Icon name="iface" /><span class="k">Via interface</span><span class="v">{t.via_interface}</span></div>
        <div class="stat val" title="Value flow also counts the moves made on calls to targets outside the project"><Icon name="value" /><span class="k">Value transfers on these calls</span><span class="v">{t.transfers}</span></div>
        <div class="stat"><Icon name="inherit" /><span class="k">Inherits a project contract</span><span class="v">{inherits}</span></div>
        <div class="stat"><Icon name="deploy" /><span class="k">Holds the address of one</span><span class="v">{holds}</span></div>
        {#if t.unresolved}
          <ul class="list"><li class="li"><button type="button" class="rb" onclick={() => toggleFold('outside', true)}><Icon name="unres" class="amb" /><span class="rb-body"><span class="rt amb">{plural(t.unresolved, 'unresolved call', 'unresolved calls')}</span><span class="rs">to {plural(g?.outside.length ?? 0, 'target', 'targets')} outside the project{open.has('outside') ? '' : ' · show them'}</span></span></button></li></ul>
        {/if}
      {/if}
      <h2><span class="ht">Most connected</span><span class="n">calls and reads in and out</span></h2>
      <ul class="list">
        {#each top as t (t.contract)}
          <li class="li"><button type="button" class="rb" onclick={() => select(t.contract)}><Icon name="folder" /><span class="rb-body"><span class="rt">{t.contract}</span><span class="rs">{byName.get(t.contract)?.folder ?? ''}</span></span></button><span class="src count">{t.outgoing_calls + t.incoming_calls}</span></li>
        {/each}
      </ul>
      <p class="note">Click a contract to see who it calls and who calls it. Click it again, or press Enter, to open its functions.</p>
    {/if}
  </aside>
</div>

<div class="legend">
  <span class="tlabel">Lines</span>
  {#each LINES as l (l.id)}
    {@const s = l.id === 'iface' ? null : lineStyle(l.id)}
    <button type="button" class="fchip" data-s={lines[l.id]} title={l.tip} onclick={() => setLine(l.id)} aria-label="{l.label}: {lines[l.id]}">
      {#if !s}<Icon name="iface" />{:else}<svg width="18" height="6" aria-hidden="true"><line x1="1" y1="3" x2="17" y2="3" style:stroke={s.color} stroke-width="2" stroke-dasharray={s.dash} /></svg>{/if}
      <span class="lbl">{l.label}</span>{#if lines[l.id] !== 'show'}<span class="st">{lines[l.id] === 'dim' ? 'dimmed' : 'hidden'}</span>{/if}
    </button>
  {/each}
  <span class="tlabel basis-key">Found by</span>
  {#each BASIS_KEY as [basis, text] (basis)}
    {@const s = lineStyle('calls', basis)}
    <span class="key basis-key" title={BASIS_LABEL[basis]}><svg width="18" height="6" aria-hidden="true"><line x1="1" y1="3" x2="17" y2="3" style:stroke={s.color} stroke-width="2" stroke-dasharray={s.dash} /></svg>{text}</span>
  {/each}
  <span class="hint">Hover a line for counts · click for functions</span>
</div>

{#if tip}
  {@const called = tip.edge.items.filter(hasRow)}
  <Tooltip x={tip.x} y={tip.y} within={canvasEl}>
    <b>{named(tip.edge.a)} {tip.edge.forward && tip.edge.back ? '⇄' : tip.edge.forward ? '→' : '←'} {named(tip.edge.b)}</b>
    {#each (['calls', 'reads', 'library', 'unresolved', 'inherits', 'holds'] as const).filter((k) => tip!.edge.items.some((i) => i.line === k)) as k (k)}
      {'\n'}{tally(k, tip.edge.items.filter((i) => i.line === k).length)}
    {/each}
    {#if called.some((i) => i.ambiguous)}{'\n'}{called.filter((i) => i.ambiguous).length} AMBIGUOUS{/if}
    {#each called.slice(0, 8) as i (rowId(i))}
      {'\n'}{label(i.from)}.{member(i.row.function)} · line {i.row.span.start_line} · {rowMeta(i, false)}
    {/each}
    {#if called.length > 8}{'\n'}… click the line for all{/if}
  </Tooltip>
{/if}
