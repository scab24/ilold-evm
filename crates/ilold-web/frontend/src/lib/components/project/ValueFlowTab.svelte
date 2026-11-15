<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { MarkerType, type Edge, type FitViewOptions, type Node } from '@xyflow/svelte';
  import type { ValueFlowPairs, ValueTransfer } from '$lib/api/rest';
  import { setNodes, setEdges, clearGraph, type GraphNodeData } from '$lib/stores/graph.svelte';
  import { valueFlow } from '$lib/stores/project-data';
  import { ASSET_ORDER, POOL_ASSET, POOL_ASSET_LONG, amountSuffix, assetName, describeTransfers } from '$lib/utils/value';
  import { RELATION_STYLE, UNRESOLVED_COLOR, splitRef } from '$lib/utils/links';
  import { openFunctionLine } from '$lib/utils/ide-links';
  import { setQuery } from '$lib/utils/query';
  import GraphCanvasFlow from '$lib/components/contract/GraphCanvasFlow.svelte';
  import BoxNode, { type BoxNodeData } from './BoxNode.svelte';
  import CaptionNode from './CaptionNode.svelte';
  import ChipEdge, { type ChipEdgeData } from './ChipEdge.svelte';
  import Icon from '$lib/components/chrome/Icon.svelte';
  import Tooltip from '$lib/components/chrome/Tooltip.svelte';
  import { selectionKeys } from './selection-keys';
  import Row from '$lib/components/chrome/Row.svelte';
  import Seg from '$lib/components/chrome/Seg.svelte';
  import LegendBar from '$lib/components/chrome/LegendBar.svelte';
  import { READABLE_FIT, addPort, anchors, placeChips, sidesBetween, type Port } from './chips';
  import { assetCounts, buildFlow, type FlowArrow, type FlowParty } from './value-graph';

  const VALUE = RELATION_STYLE.value.color;
  const AMBER = UNRESOLVED_COLOR;
  const ACCENT = 'var(--color-accent-hover)';
  const WIDTH = 200;
  const GAP_X = 130;
  const GAP_Y = 34;
  const CAPTIONS = ['Pays in', 'Inside the project', '', 'Leaves the project'];

  let data = $state<ValueFlowPairs | null>(null);
  let error = $state<string | null>(null);
  let tip = $state<{ x: number; y: number; arrow: FlowArrow } | null>(null);
  let fit: ((options?: FitViewOptions) => Promise<boolean>) | null = null;
  let canvasEl = $state<HTMLElement | null>(null);

  const counts = $derived(data ? assetCounts(data) : new Map<string, number>());
  const asset = $derived.by(() => {
    const a = page.url.searchParams.get('asset');
    return a !== null && counts.has(a) ? a : null;
  });
  const total = $derived([...counts.values()].reduce((a, b) => a + b, 0));
  const flow = $derived(data ? buildFlow(data, asset) : null);
  const partyOf = $derived(new Map((flow?.parties ?? []).map((p) => [p.key, p])));
  const arrow = $derived(flow?.arrows.find((a) => a.id === page.url.searchParams.get('arrow')) ?? null);
  const party = $derived(partyOf.get(page.url.searchParams.get('sel') ?? '') ?? null);
  const selected = $derived(party?.key ?? null);
  const picked = $derived(arrow?.id ?? null);

  const shown = $derived.by(() => {
    if (!flow) return [] as FlowArrow[];
    if (arrow) return [arrow];
    if (party) return flow.arrows.filter((a) => a.from === party.key || a.to === party.key);
    return flow.arrows;
  });

  onMount(async () => {
    try {
      data = await valueFlow();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  });
  onDestroy(clearGraph);

  $effect(() => {
    const keys = arrow || party ? [...new Set(shown.flatMap((a) => [a.from, a.to]))] : [];
    const nodes = keys.map((id) => ({ id }));
    const timer = setTimeout(() => fit?.(nodes.length ? { ...READABLE_FIT, nodes, duration: 250 } : { ...READABLE_FIT, duration: 250 }), 120);
    return () => clearTimeout(timer);
  });

  function partyLines(p: FlowParty): BoxNodeData['lines'] {
    if (p.kind === 'users') {
      const names = p.expressions.slice(0, 3).join(', ');
      return [{ text: p.expressions.length > 3 ? `${names} +${p.expressions.length - 3}` : names, tone: 'mono' }];
    }
    if (p.kind === 'declared') return [{ text: 'outside the project' }];
    const parts = [p.pays ? `pays out ${p.pays}` : '', p.receives ? `receives ${p.receives}` : ''].filter(Boolean);
    return [{ text: parts.join(' · ') }];
  }

  function height(p: FlowParty, ports: number): number {
    const text = partyLines(p)?.[0]?.text ?? '';
    const lines = p.kind === 'users' ? Math.max(1, Math.ceil(text.length / 26)) : 1;
    return Math.max(40 + lines * 18, ports * 13 + 18);
  }

  const chipText = (a: FlowArrow) => (a.pool ? `${POOL_ASSET} · ${a.transfers.length}` : `${a.assets.join(' + ')}${a.transfers.length > 1 ? ` · ${a.transfers.length}` : ''}`);

  const layout = $derived.by(() => {
    if (!flow) return null;
    const { parties, arrows } = flow;
    const columns = [...new Set(parties.map((p) => p.column))].sort();
    const colX = new Map(columns.map((c, i) => [c, i * (WIDTH + GAP_X)]));
    const members = new Map(columns.map((c) => [c, parties.filter((p) => p.column === c).sort((a, b) => b.pays + b.receives - (a.pays + a.receives) || a.label.localeCompare(b.label))]));
    const rank = new Map<string, number>();
    const index = () => members.forEach((list) => list.forEach((p, i) => rank.set(p.key, i / Math.max(1, list.length - 1))));
    index();
    for (let sweep = 0; sweep < 4; sweep++) {
      for (const list of members.values()) {
        const mean = (p: FlowParty) => {
          const near = arrows.filter((a) => a.from === p.key || a.to === p.key).map((a) => rank.get(a.from === p.key ? a.to : a.from) ?? 0.5);
          return near.length ? near.reduce((x, y) => x + y, 0) / near.length : 0.5;
        };
        list.sort((a, b) => mean(a) - mean(b));
      }
      index();
    }

    const portCount = (key: string) => arrows.filter((a) => a.from === key || a.to === key).length;
    const h = new Map(parties.map((p) => [p.key, height(p, portCount(p.key))]));
    const colH = (list: FlowParty[]) => list.reduce((sum, p) => sum + (h.get(p.key) ?? 0) + GAP_Y, -GAP_Y);
    const tallest = Math.max(...[...members.values()].map(colH));
    const pos = new Map<string, { x: number; y: number }>();
    for (const [column, list] of members) {
      let top = 44 + (tallest - colH(list)) / 2;
      for (const p of list) {
        pos.set(p.key, { x: colX.get(column) ?? 0, y: top });
        top += (h.get(p.key) ?? 0) + GAP_Y;
      }
    }
    const rect = (key: string) => ({ ...pos.get(key)!, w: WIDTH, h: h.get(key) ?? 0 });

    const ports = new Map<string, Port[]>();
    for (const a of arrows) {
      const from = partyOf.get(a.from)!;
      const to = partyOf.get(a.to)!;
      const [s, t] = sidesBetween({ column: from.column, y: 0 }, { column: to.column, y: 0 });
      addPort(ports, a.from, { id: `${a.id}:s`, side: s, type: 'source', order: pos.get(a.to)!.y });
      addPort(ports, a.to, { id: `${a.id}:t`, side: t, type: 'target', order: pos.get(a.from)!.y });
    }
    const anchor = anchors(ports, rect);
    const order = [...arrows].sort((a, b) => Number(b.pool) - Number(a.pool) || b.transfers.length - a.transfers.length);
    const chipAt = placeChips(order.map((a) => ({ id: a.id, s: anchor.get(`${a.id}:s`)!, t: anchor.get(`${a.id}:t`)!, text: chipText(a) })), parties.map((p) => rect(p.key)));
    return { columns, colX, pos, h, ports, chipAt };
  });

  const nodes = $derived.by((): Node<GraphNodeData>[] => {
    if (!flow || !layout) return [];
    const touched = new Set(shown.flatMap((a) => [a.from, a.to]));
    const focus = arrow !== null || party !== null;
    const list: Node<GraphNodeData>[] = flow.parties.map((p) => ({
      id: p.key,
      type: 'box',
      position: layout.pos.get(p.key)!,
      data: {
        _type: 'box',
        label: p.label,
        icon: p.kind === 'declared' ? 'out' : undefined,
        lines: partyLines(p),
        ports: layout.ports.get(p.key) ?? [],
        variant: p.kind === 'users' ? 'cluster' : '',
        width: WIDTH,
        height: layout.h.get(p.key),
        on: p.key === selected,
        dim: focus && !touched.has(p.key),
      } satisfies BoxNodeData,
    }));
    for (const column of layout.columns) {
      const caption = CAPTIONS[column];
      if (caption) list.push({ id: `caption:${column}`, type: 'caption', position: { x: layout.colX.get(column) ?? 0, y: 0 }, selectable: false, data: { _type: 'box', label: caption } });
    }
    return list;
  });

  const edges = $derived.by((): Edge[] => {
    if (!flow || !layout) return [];
    const focus = arrow !== null || party !== null;
    return flow.arrows.map((a) => {
      const on = a.id === picked;
      const lit = !focus || shown.includes(a);
      const color = on ? ACCENT : a.pool ? AMBER : VALUE;
      const at = layout.chipAt.get(a.id) ?? null;
      return {
        id: a.id,
        type: 'chip',
        source: a.from,
        target: a.to,
        sourceHandle: `${a.id}:s`,
        targetHandle: `${a.id}:t`,
        zIndex: on ? 2 : undefined,
        markerEnd: { type: MarkerType.ArrowClosed, color, width: 16, height: 16 },
        style: `stroke: ${color}; stroke-width: ${on ? 2.4 : a.pool ? 1.8 : 2}; opacity: ${lit ? 1 : a.pool ? 0.45 : 0.16}`,
        data: {
          chip: !focus || lit ? chipText(a) : undefined,
          icon: a.pool ? 'unres' : 'value',
          tone: a.pool ? 'amb' : 'val',
          on,
          labelAt: on ? (at ?? 0.5) : at,
          onpick: pickArrow,
        } satisfies ChipEdgeData,
      };
    });
  });

  $effect(() => setNodes(nodes));
  $effect(() => setEdges(edges));

  function pickArrow(id: string) {
    setQuery({ arrow: id === picked ? null : id, sel: null });
  }
  function pickParty(node: Node<GraphNodeData>) {
    if (node.type !== 'box') return;
    setQuery({ sel: node.id === selected ? null : node.id, arrow: null });
  }
  function setAsset(name: string | null) {
    setQuery({ asset: name, arrow: null });
  }
  const opens = $derived(party?.kind === 'contract' && !party.label.includes(' or ') ? party.label : null);
  const onKey = selectionKeys(() => ({
    selected: party !== null || arrow !== null,
    clear: () => setQuery({ sel: null, arrow: null }),
    open: opens ? () => goto(`/contract/${encodeURIComponent(opens!)}`) : null,
  }));

  function onArrowHover(edge: Edge, event: MouseEvent) {
    const a = flow?.arrows.find((x) => x.id === edge.id);
    tip = a ? { x: event.clientX, y: event.clientY, arrow: a } : null;
  }

  function fnName(ref: string): string {
    const { contract, member } = splitRef(ref);
    return contract ? `${contract}.${member}` : member;
  }

  interface Line { key: string; transfer: ValueTransfer; to: string; contexts: string[] }

  const groups = $derived.by(() => {
    const lines = new Map<string, Line>();
    for (const a of shown) {
      const to = partyOf.get(a.to)?.label ?? a.to;
      for (const t of a.transfers) {
        const key = `${t.function}|${t.span.start_line}|${t.span.start_col}|${to}`;
        const line = lines.get(key) ?? { key, transfer: t, to, contexts: [] };
        if (!line.contexts.includes(t.in_context)) line.contexts.push(t.in_context);
        lines.set(key, line);
      }
    }
    const byAsset = new Map<string, Line[]>();
    for (const line of lines.values()) {
      const name = assetName(line.transfer);
      (byAsset.get(name) ?? byAsset.set(name, []).get(name)!).push(line);
    }
    return [...byAsset.entries()].sort((a, b) => ASSET_ORDER.indexOf(a[0]) - ASSET_ORDER.indexOf(b[0]));
  });

  const functions = $derived(new Set((flow?.transfers ?? []).map((t) => t.function)).size);

  function verb(t: ValueTransfer): string {
    if (t.operation === 'mint') return 'mints';
    if (t.operation === 'burn') return 'burns';
    return t.direction === 'in' ? 'receives' : t.direction === 'out' ? 'sends' : 'moves';
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="toolbar" role="toolbar" aria-label="Value flow">
  <span class="crumb-here">Value flow</span>
  <span class="tlabel">Asset</span>
  <Seg label="Asset" options={[{ id: '', label: `All ${total}` }, ...ASSET_ORDER.filter((a) => counts.has(a)).map((name) => ({ id: name, label: `${name} ${counts.get(name)}`, amb: name === POOL_ASSET, title: name === POOL_ASSET ? POOL_ASSET_LONG : name }))]} value={asset ?? ''} onpick={(id) => setAsset(id || null)} />
</div>

<div class="work">
  <div class="canvas" bind:this={canvasEl}>
    {#if error}
      <div class="state-msg err">{error}</div>
    {:else if !data}
      <div class="state-msg">Loading value flow…</div>
    {:else if data.pairs.length === 0}
      <div class="state-msg">No value moves between parties in this project.</div>
    {:else}
      <GraphCanvasFlow
        tools={false}
        extraNodeTypes={{ box: BoxNode, caption: CaptionNode } as any}
        edgeTypes={{ chip: ChipEdge } as any}
        onnodetap={pickParty}
        onbackgroundtap={() => setQuery({ sel: null, arrow: null })}
        onedgetap={(e) => pickArrow(e.id)}
        onedgemouseenter={onArrowHover}
        onedgemouseleave={() => (tip = null)}
        canDeleteNodes={false}
        keepPositions={false}
        onready={(api) => (fit = api.fitView)}
        initialFit={READABLE_FIT}
        minZoom={0.2}
      />
    {/if}
  </div>
  <aside class="insp" aria-label="Value moves">
    {#if arrow}
      <h1>{partyOf.get(arrow.from)?.label} → {partyOf.get(arrow.to)?.label}</h1>
      <p class="sub">{arrow.transfers.length} {arrow.transfers.length === 1 ? 'transfer' : 'transfers'} · {arrow.assets.join(' + ')}</p>
    {:else if party}
      <h1>{party.label}</h1>
      <p class="sub">{[party.pays ? `pays out ${party.pays}` : '', party.receives ? `receives ${party.receives}` : ''].filter(Boolean).join(' · ')}</p>
      {#if party.kind === 'users'}<p class="sub mono">{party.expressions.join(', ')}</p>{/if}
      {#if opens}<div class="actions"><button type="button" class="btn" onclick={() => goto(`/contract/${encodeURIComponent(opens!)}`)}>Open functions <span class="tlabel">Enter</span></button></div>{/if}
    {:else}
      <h1>Value moves</h1>
      <p class="sub">{flow?.transfers.length ?? 0} value transfers in {functions} functions, calls to targets outside the project included</p>
    {/if}
    {#each groups as [name, lines] (name)}
      {@const moves = lines.reduce((n, l) => n + l.contexts.length, 0)}
      <h2 class:amb={name === POOL_ASSET}><Icon name={name === POOL_ASSET ? 'unres' : 'value'} /><span class="ht">{name === POOL_ASSET ? POOL_ASSET_LONG : name}</span><span class="n">{moves} {moves === 1 ? 'transfer' : 'transfers'}</span></h2>
      <ul class="list">
        {#each lines as line (line.key)}
          {@const t = line.transfer}
          <Row icon={t.via_library ? 'lib' : 'value'} tone={name === POOL_ASSET ? 'amb' : 'val'} title="{fnName(t.function)} → {line.to}" line={t.span.start_line} onsource={() => openFunctionLine(t.function, t.span.start_line)}>
            {#snippet meta()}
              <span class={name === POOL_ASSET ? 'amb' : 'val'}>{verb(t)} {name}</span>{amountSuffix(t)}{t.via_library ? ` · through ${t.via_library}` : ''}{line.contexts.some((c) => c !== splitRef(t.function).contract) ? ` · runs inside ${line.contexts.join(', ')}` : ''}
            {/snippet}
          </Row>
        {/each}
      </ul>
    {/each}
  </aside>
</div>

<LegendBar label="Key" hint="Hover an arrow for its transfers, click it to list them">
  <span class="key"><svg width="22" height="8" aria-hidden="true"><line x1="1" y1="4" x2="21" y2="4" style:stroke={VALUE} stroke-width="2" /></svg>Money moves this way</span>
  <span class="key amb"><svg width="22" height="8" aria-hidden="true"><line x1="1" y1="4" x2="21" y2="4" style:stroke={AMBER} stroke-width="2" /></svg>{POOL_ASSET_LONG}</span>
  <span class="key"><Icon name="lib" />Library code it runs</span>
</LegendBar>

{#if tip}
  <Tooltip x={tip.x} y={tip.y} within={canvasEl}>
    <b>{partyOf.get(tip.arrow.from)?.label} → {partyOf.get(tip.arrow.to)?.label}</b>
    {'\n'}{describeTransfers(tip.arrow.transfers)}
  </Tooltip>
{/if}
