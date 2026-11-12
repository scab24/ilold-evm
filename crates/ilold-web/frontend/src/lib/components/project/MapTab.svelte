<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { MarkerType, type Edge, type FitViewOptions, type Node } from '@xyflow/svelte';
  import {
    getContract, getProjectConnections, getProjectDepgraph, getValueFlow,
    type ContractCall, type ContractSummary, type ProjectConnections, type ProjectDepGraph,
  } from '$lib/api/rest';
  import { setNodes, setEdges, clearGraph, type GraphNodeData } from '$lib/stores/graph.svelte';
  import { splitRef } from '$lib/utils/links';
  import { openInIde } from '$lib/utils/ide-links';
  import { setQuery } from '$lib/utils/query';
  import GraphCanvasFlow from '$lib/components/contract/GraphCanvasFlow.svelte';
  import BoxNode, { type BoxNodeData, type BoxPort } from './BoxNode.svelte';
  import RegionNode, { type RegionNodeData } from './RegionNode.svelte';
  import ChipEdge, { type ChipEdgeData } from './ChipEdge.svelte';
  import Icon from './Icon.svelte';
  import Row from './Row.svelte';
  import { FIT_PADDING, placeChips, type Point, type Side } from './chips';
  import {
    DEFAULT_LINES, INTERFACES, LIBRARIES, LINES, OUTSIDE, STYLE,
    edges as mapEdges, itemState, items as mapItems, neighbourhood, nextState, shortName,
    type Item, type Line, type LineState, type MapEdge,
  } from './map-graph';
  import { endpointOf, groups as mapGroups, layout, type Fold } from './map-layout';

  let { contracts }: { contracts: ContractSummary[] } = $props();

  const ACCENT = '#8bb8e8';
  const FOLDS: Record<string, Fold> = { [LIBRARIES]: 'libraries', [INTERFACES]: 'interfaces', [OUTSIDE]: 'outside' };

  let connections = $state<ProjectConnections | null>(null);
  let depgraph = $state<ProjectDepGraph | null>(null);
  let payers = $state(new Set<string>());
  let access = $state(new Map<string, { entry: number; restricted: number }>());
  let error = $state<string | null>(null);
  let hovered = $state<string | null>(null);
  let tip = $state<{ x: number; y: number; edge: MapEdge } | null>(null);
  let find = $state('');
  let fit: ((options?: FitViewOptions) => Promise<boolean>) | null = null;

  const params = $derived(page.url.searchParams);
  const selected = $derived(params.get('c'));
  const pickedPair = $derived(params.get('pair'));
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
  const deployable = $derived(contracts.filter((c) => c.kind === 'Contract' && !c.is_dependency));
  const all = $derived(connections ? mapItems(contracts, connections, depgraph) : ([] as Item[]));
  const g = $derived(connections ? mapGroups(contracts, connections) : null);
  const endpoint = $derived(g ? endpointOf(g, open) : (id: string) => id);
  const drawn = $derived(mapEdges(all, endpoint, lines));
  const hood = $derived(selected ? neighbourhood(drawn, endpoint(selected), depth, direction) : null);

  onMount(async () => {
    try {
      [connections, depgraph] = await Promise.all([getProjectConnections(), getProjectDepgraph()]);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return;
    }
    getValueFlow().then((flow) => (payers = new Set(flow.pairs.map((p) => p.in_context))), () => {});
    for (const c of deployable) {
      getContract(c.name).then((detail) => {
        const fns = [...detail.functions, ...(detail.inherited_functions ?? [])];
        const restricted = fns.filter((f) => typeof f.access === 'object' && 'Restricted' in f.access).length;
        const entry = fns.filter((f) => f.access === 'Public').length + restricted;
        access = new Map(access).set(c.name, { entry, restricted });
      }, () => {});
    }
  });
  onDestroy(clearGraph);

  function label(id: string): string {
    if (id === OUTSIDE) return 'Outside the project';
    if (id === LIBRARIES) return 'Libraries';
    if (id === INTERFACES) return 'Interfaces';
    return shortName(id);
  }

  function boxLines(id: string): BoxNodeData['lines'] {
    if (!g) return [];
    if (id === OUTSIDE) {
      const calls = connections?.unresolved.reduce((n, u) => n + u.rows.length, 0) ?? 0;
      return [{ text: `${g.outside.length} targets · ${calls} unresolved calls`, tone: 'amb' }];
    }
    if (id === LIBRARIES) return [{ text: `${g.libraries.length} libraries · click to show` }];
    if (id === INTERFACES) return [{ text: `${g.interfaces.length} interfaces · click to show` }];
    if (id.startsWith('out:')) return [{ text: 'outside the project', tone: 'amb' }];
    const c = byName.get(id);
    if (!c) return [{ text: 'library from a dependency' }];
    if (c.kind !== 'Contract') return [{ text: c.kind.toLowerCase() }];
    const known = access.get(id);
    const result: NonNullable<BoxNodeData['lines']> = [{ text: known ? `${known.entry} entry · ${known.restricted} restricted` : 'contract' }];
    if (payers.has(id)) result.push({ text: 'moves value', tone: 'val', icon: 'value' });
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

  function draw() {
    if (!g) return;
    const placed = layout(g, open, all);
    const ports = new Map<string, (BoxPort & { order: number })[]>();
    const add = (id: string, port: BoxPort & { order: number }) => (ports.get(id) ?? ports.set(id, []).get(id)!).push(port);
    const geometry = new Map<string, { vertical: boolean; aFirst: boolean }>();
    const shown = drawn.filter((e) => placed.nodes.has(e.a) && placed.nodes.has(e.b));
    for (const e of shown) {
      const a = placed.nodes.get(e.a)!;
      const b = placed.nodes.get(e.b)!;
      let sides: [Side, Side];
      let vertical = false;
      if (a.column !== b.column) sides = a.column < b.column ? ['r', 'l'] : ['l', 'r'];
      else if (a.region === b.region && Math.abs(a.index - b.index) === 1) {
        vertical = true;
        sides = a.y < b.y ? ['b', 't'] : ['t', 'b'];
      } else sides = ['r', 'r'];
      geometry.set(e.id, { vertical, aFirst: vertical ? a.y < b.y : a.x <= b.x });
      add(e.a, { id: `${e.id}:a`, side: sides[0], type: 'source', order: sides[0] === 't' || sides[0] === 'b' ? b.x : b.y });
      add(e.b, { id: `${e.id}:b`, side: sides[1], type: 'target', order: sides[1] === 't' || sides[1] === 'b' ? a.x : a.y });
    }
    const anchor = new Map<string, Point & { side: Side }>();
    for (const [id, list] of ports) {
      list.sort((x, y) => x.order - y.order);
      const p = placed.nodes.get(id)!;
      for (const side of ['l', 'r', 't', 'b'] as const) {
        const at = list.filter((x) => x.side === side);
        at.forEach((port, i) => {
          const f = (i + 1) / (at.length + 1);
          const point = side === 'l' ? { x: p.x, y: p.y + p.h * f } : side === 'r' ? { x: p.x + p.w, y: p.y + p.h * f } : side === 't' ? { x: p.x + p.w * f, y: p.y } : { x: p.x + p.w * f, y: p.y + p.h };
          anchor.set(port.id, { ...point, side });
        });
      }
    }
    const focus = hood !== null;
    const inFocus = (e: MapEdge) => {
      if (pickedPair) return e.id === pickedPair;
      if (!hood) return true;
      const [a, b] = [hood.get(e.a), hood.get(e.b)];
      return a !== undefined && b !== undefined && Math.abs(a - b) === 1;
    };
    const texts = new Map(shown.map((e) => [e.id, chipText(e, geometry.get(e.id)!.vertical, geometry.get(e.id)!.aFirst)]));
    const boxes = [...placed.nodes.values()].map((p) => ({ x: p.x, y: p.y, w: p.w, h: p.h })).concat(placed.regions.map((r) => ({ x: r.x, y: r.y, w: r.w, h: 28 })));
    const ordered = [...shown].filter((e) => !focus || inFocus(e)).sort((x, y) => y.items.length - x.items.length);
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
          dim: focus && !hood!.has(p.id),
        } satisfies BoxNodeData,
      });
    }
    const edges: Edge[] = shown.map((e) => {
      const style = STYLE[e.look];
      const on = e.id === pickedPair;
      const lit = inFocus(e);
      const near = hovered !== null && (e.a === hovered || e.b === hovered);
      const color = on ? ACCENT : style.color;
      const opacity = on ? 1 : !lit ? 0.07 : e.state === 1 ? 0.35 : hovered && !near && !focus ? 0.3 : 1;
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
        zIndex: on ? 2 : undefined,
        style: `stroke: ${color}; stroke-width: ${style.width + (on || near ? 1 : 0)}; opacity: ${opacity}${style.dash ? `; stroke-dasharray: ${style.dash}` : ''}`,
        data: {
          chip: lit ? texts.get(e.id) : undefined,
          tone: e.look === 'unresolved' || e.look === 'ambiguous' ? 'amb' : e.look === 'value' ? 'val' : '',
          on,
          labelAt: on ? (chipAt.get(e.id) ?? 0.5) : (chipAt.get(e.id) ?? null),
          onpick: pickPair,
        } satisfies ChipEdgeData,
      };
    });
    setNodes(nodes);
    setEdges(edges);
  }

  $effect(draw);

  function toggleFold(fold: Fold, expand: boolean) {
    const next = new Set(open);
    if (expand) next.add(fold);
    else next.delete(fold);
    setQuery({ expand: [...next].sort().join(',') || null });
    setTimeout(() => fit?.({ padding: FIT_PADDING, duration: 250 }), 200);
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
    setQuery({ lines: changed.join(',') || null });
  }
  function search(event: SubmitEvent) {
    event.preventDefault();
    const q = find.trim().toLowerCase();
    if (!q || !g) return;
    const names = [...g.folders.values()].flat().concat(g.libraries, g.interfaces, g.outside);
    const hit = names.find((n) => shortName(n).toLowerCase() === q) ?? names.find((n) => shortName(n).toLowerCase().startsWith(q)) ?? names.find((n) => shortName(n).toLowerCase().includes(q));
    if (!hit) return;
    const fold = g.libraries.includes(hit) ? 'libraries' : g.interfaces.includes(hit) ? 'interfaces' : g.outside.includes(hit) ? 'outside' : null;
    const expand = fold && !open.has(fold) ? [...open, fold].sort().join(',') : null;
    setQuery({ c: hit, pair: null, ...(expand ? { expand } : {}) });
  }
  function onKey(event: KeyboardEvent) {
    const t = event.target;
    if (t instanceof HTMLInputElement || t instanceof HTMLSelectElement || t instanceof HTMLTextAreaElement) return;
    if (event.key === 'Enter' && selected) openContract(selected);
    else if (event.key === 'Escape' && (selected || pickedPair)) setQuery({ c: null, pair: null });
  }
  function onEdgeHover(edge: Edge, event: MouseEvent) {
    const e = drawn.find((x) => x.id === edge.id);
    tip = e ? { x: event.clientX, y: event.clientY, edge: e } : null;
  }

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
    const target = callee ? `${shortName(item.to)}.${member(callee.function)}` : (item.row.target ?? '?').replace('::', '.');
    const caller = item.from === selected ? member(item.row.function) : `${item.from}.${member(item.row.function)}`;
    return `${caller} → ${target}`;
  }
  function rowMeta(item: Item & { row: ContractCall }): string {
    const parts = [KIND_WORD[item.line]];
    if (item.iface) parts.push('via interface');
    else if (item.row.callees.some((d) => d.contract === item.to && d.basis === 'selector_match')) parts.push('selector match');
    if (item.row.via) parts.push(`address in ${member(item.row.via)}`);
    if (item.ambiguous) parts.push(`${item.row.callees.length} possible · AMBIGUOUS`);
    if (item.row.unresolved_reason) parts.push(`no project contract implements ${shortName(item.to)}`);
    if (item.row.in_try) parts.push('inside try');
    return parts.join(' · ');
  }
  const ORDER: Item['line'][] = ['calls', 'reads', 'library', 'unresolved'];
  const withRow = (list: Item[]) =>
    list
      .filter((i): i is Item & { row: ContractCall } => i.row !== null && itemState(i, lines) > 0)
      .sort((x, y) => itemState(y, lines) - itemState(x, lines) || ORDER.indexOf(x.line) - ORDER.indexOf(y.line) || x.to.localeCompare(y.to) || x.row.span.start_line - y.row.span.start_line);
  const outgoing = $derived(selected ? withRow(all.filter((i) => i.from === selected && i.line !== 'unresolved')) : []);
  const incoming = $derived(selected ? withRow(all.filter((i) => i.to === selected)) : []);
  const unresolved = $derived(selected ? withRow(all.filter((i) => i.from === selected && i.line === 'unresolved')) : []);
  const structural = $derived(selected ? all.filter((i) => i.row === null && (i.from === selected || i.to === selected) && itemState(i, lines) > 0) : []);
  const pair = $derived(pickedPair ? (drawn.find((e) => e.id === pickedPair) ?? null) : null);
  const counts = $derived(new Map((connections?.contracts ?? []).map((c) => [c.contract, c])));
  const top = $derived([...(connections?.contracts ?? [])].sort((a, b) => b.outgoing_calls + b.incoming_calls - (a.outgoing_calls + a.incoming_calls) || a.contract.localeCompare(b.contract)).slice(0, 5));
  const totals = $derived.by(() => {
    const pairs = connections?.pairs ?? [];
    const sum = (f: (p: (typeof pairs)[number]) => number) => pairs.reduce((n, p) => n + f(p), 0);
    const deployed = new Set(deployable.map((c) => c.name));
    return {
      calls: sum((p) => p.calls),
      reads: sum((p) => p.reads),
      library: sum((p) => p.library),
      iface: sum((p) => p.via_interface),
      value: sum((p) => (deployed.has(p.to) ? p.transfers : 0)),
      inherits: all.filter((i) => i.line === 'inherits').length,
      holds: all.filter((i) => i.line === 'holds').length,
      unresolved: connections?.unresolved.reduce((n, u) => n + u.rows.length, 0) ?? 0,
      targets: g?.outside.length ?? 0,
    };
  });
  const folderCount = $derived(new Set(contracts.map((c) => c.folder)).size);
</script>

<svelte:window onkeydown={onKey} />

{#snippet rows(list: (Item & { row: ContractCall })[])}
  <ul class="list">
    {#each list as item, i (i)}
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

<div class="toolbar">
  <button type="button" class="crumb" style="background: none; border: 0; cursor: pointer; padding: 0" onclick={() => select(null)}>Project</button>
  {#if selected}
    <span class="tlabel">›</span><span class="crumb-here">{label(selected)}</span>
  {:else}
    <span class="tlabel">Select a contract to explore around it</span>
  {/if}
  <div class="seg" role="group" aria-label="Direction">
    {#each [['callers', 'Callers'], ['both', 'Both'], ['callees', 'Callees']] as [id, text]}
      <button type="button" disabled={!selected} aria-pressed={selected !== null && direction === id} onclick={() => setQuery({ dir: id === 'both' ? null : id })}>{text}</button>
    {/each}
  </div>
  <span class="tlabel">Depth</span>
  <div class="seg" role="group" aria-label="Depth">
    {#each [1, 2] as d}
      <button type="button" disabled={!selected} aria-pressed={selected !== null && depth === d} onclick={() => setQuery({ depth: d === 1 ? null : '2' })}>{d}</button>
    {/each}
  </div>
  <form onsubmit={search} style="margin-left: auto">
    <input class="find" type="search" placeholder="Find a contract" aria-label="Find a contract" bind:value={find} />
  </form>
</div>

<div class="work">
  <div class="canvas">
    {#if error}
      <div class="state-msg err">{error}</div>
    {:else if !connections}
      <div class="state-msg">Analyzing…</div>
    {:else if contracts.length === 0}
      <div class="state-msg">No contracts found in this project.</div>
    {:else}
      <GraphCanvasFlow
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
        initialFit={{ padding: FIT_PADDING }}
        minZoom={0.1}
      />
    {/if}
  </div>
  <aside class="insp" aria-label="Details">
    {#if pair}
      <h1>{label(pair.a)} {pair.forward && pair.back ? '⇄' : pair.forward ? '→' : '←'} {label(pair.b)}</h1>
      <p class="sub">{pair.items.length} {pair.items.length === 1 ? 'link' : 'links'} on the map</p>
      <h2>Function to function<span class="n">{withRow(pair.items).length}</span></h2>
      {@render rows(withRow(pair.items))}
      {#if pair.items.some((i) => i.row === null)}
        <p class="note">{pair.items.filter((i) => i.row === null).map((i) => `${label(i.from)} ${i.line} ${label(i.to)}`).join(' · ')}</p>
      {/if}
      <div style="margin-top: 14px"><button type="button" class="btn" onclick={() => setQuery({ pair: null })}>Back</button></div>
    {:else if selected}
      {@const c = byName.get(selected)}
      {@const n = counts.get(selected)}
      <h1>{label(selected)}</h1>
      <p class="sub">{c ? `${c.folder || 'root'} · ${c.kind.toLowerCase()}` : selected.startsWith('out:') ? 'outside the project' : 'library from a dependency'}</p>
      {#if n}
        <div class="stat"><Icon name="call" /><span class="k">Calls and reads to other contracts</span><span class="v">{n.outgoing_calls}</span></div>
        <div class="stat"><Icon name="entry" /><span class="k">From other contracts</span><span class="v">{n.incoming_calls}</span></div>
      {/if}
      {#if access.get(selected)}
        {@const a = access.get(selected)!}
        <div class="stat"><Icon name="lock" /><span class="k">Entry points · restricted</span><span class="v">{a.entry} · {a.restricted}</span></div>
      {/if}
      {#if payers.has(selected)}<div class="stat"><Icon name="value" class="val" /><span class="k" style="color: var(--color-success-light)">Moves value</span></div>{/if}
      {#if c}<div style="margin-top: 12px"><button type="button" class="btn" onclick={() => openContract(selected)}>Open functions <span class="tlabel">Enter</span></button></div>{/if}
      {#if outgoing.length}<h2><Icon name="call" />Calls<span class="n">{outgoing.length}</span></h2>{@render rows(outgoing)}{/if}
      {#if incoming.length}<h2><Icon name="entry" />Called by<span class="n">{incoming.length}</span></h2>{@render rows(incoming)}{/if}
      {#if unresolved.length}<h2 class="amb"><Icon name="unres" />Unresolved<span class="n">{unresolved.length}</span></h2>{@render rows(unresolved)}{/if}
      {#if structural.length}
        <h2><Icon name="inherit" />Inherits and holds<span class="n">{structural.length}</span></h2>
        <p class="sub">{structural.map((i) => `${label(i.from)} ${i.line} ${label(i.to)}`).join(' · ')}</p>
      {/if}
      {#if !outgoing.length && !incoming.length && !unresolved.length}<p class="note">No line shown by the legend touches it.</p>{/if}
    {:else}
      <h1>Project</h1>
      <p class="sub">{contracts.length} contracts in {folderCount} folders · {deployable.length} deployable</p>
      <h2>Links between contracts<span class="n">function level</span></h2>
      <div class="stat"><Icon name="call" /><span class="k">Calls</span><span class="v">{totals.calls}</span></div>
      <div class="stat"><Icon name="read" /><span class="k">Reads</span><span class="v">{totals.reads}</span></div>
      <div class="stat"><Icon name="lib" /><span class="k">Library calls</span><span class="v">{totals.library}</span></div>
      <div class="stat"><Icon name="iface" /><span class="k">Via interface</span><span class="v">{totals.iface}</span></div>
      <div class="stat" style="color: var(--color-success-light)"><Icon name="value" /><span class="k">Value moves between contracts</span><span class="v">{totals.value}</span></div>
      <div class="stat"><Icon name="inherit" /><span class="k">Inherits a project contract</span><span class="v">{totals.inherits}</span></div>
      <div class="stat"><Icon name="deploy" /><span class="k">Holds the address of one</span><span class="v">{totals.holds}</span></div>
      {#if totals.unresolved}
        <ul class="list"><li class="li"><button type="button" class="rb" onclick={() => toggleFold('outside', true)}><Icon name="unres" class="amb" /><span class="rb-body"><span class="rt" style="color: var(--amber)">{totals.unresolved} unresolved calls</span><span class="rs">to {totals.targets} interfaces outside the project{open.has('outside') ? '' : ' · show them'}</span></span></button></li></ul>
      {/if}
      <h2>Most connected<span class="n">calls in and out</span></h2>
      <ul class="list">
        {#each top as t (t.contract)}
          <li class="li"><button type="button" class="rb" onclick={() => select(t.contract)}><Icon name="folder" /><span class="rb-body"><span class="rt">{t.contract}</span><span class="rs">{byName.get(t.contract)?.folder ?? ''}</span></span></button><span class="src" style="color: var(--root)">{t.outgoing_calls + t.incoming_calls}</span></li>
        {/each}
      </ul>
      <p class="note">Click a contract to see who it calls and who calls it. Click it again, or press Enter, to open its functions.</p>
    {/if}
  </aside>
</div>

<div class="legend">
  <span class="tlabel">Lines</span>
  {#each LINES as l (l.id)}
    {@const s = STYLE[l.id === 'iface' ? 'calls' : l.id]}
    <button type="button" class="fchip" data-s={lines[l.id]} title={l.tip} onclick={() => setLine(l.id)} aria-label="{l.label}: {lines[l.id]}">
      {#if l.id === 'iface'}<Icon name="iface" />{:else}<svg width="18" height="6" aria-hidden="true"><line x1="1" y1="3" x2="17" y2="3" stroke={s.color} stroke-width="2" stroke-dasharray={s.dash} /></svg>{/if}
      <span class="lbl">{l.label}</span>{#if lines[l.id] !== 'show'}<span class="st">{lines[l.id] === 'dim' ? 'dimmed' : 'hidden'}</span>{/if}
    </button>
  {/each}
  <span class="hint" title="Hover a line for counts, click it for the function list">Hover a line for counts, click it for the function list</span>
</div>

{#if tip}
  <div class="tip" style="left: {tip.x + 12}px; top: {tip.y + 12}px">
    <b>{label(tip.edge.a)} {tip.edge.forward && tip.edge.back ? '⇄' : tip.edge.forward ? '→' : '←'} {label(tip.edge.b)}</b>
    {#each (['calls', 'reads', 'library', 'unresolved', 'inherits', 'holds'] as const).filter((k) => tip!.edge.items.some((i) => i.line === k)) as k}
      {'\n'}{tally(k, tip.edge.items.filter((i) => i.line === k).length)}
    {/each}
    {#each tip.edge.items.filter((i) => i.row).slice(0, 8) as i}
      {'\n'}{label(i.from)}.{member(i.row!.function)} · line {i.row!.span.start_line}
    {/each}
    {#if tip.edge.items.filter((i) => i.row).length > 8}{'\n'}… click the line for all{/if}
  </div>
{/if}
