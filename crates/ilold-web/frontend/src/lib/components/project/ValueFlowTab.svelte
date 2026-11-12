<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { page } from '$app/state';
  import { MarkerType, type Edge, type Node } from '@xyflow/svelte';
  import { getValueFlow, type ValueFlowPairs, type ValueTransfer } from '$lib/api/rest';
  import { setNodes, setEdges, clearGraph, type GraphNodeData } from '$lib/stores/graph.svelte';
  import { describeTransfers } from '$lib/utils/value';
  import { splitRef } from '$lib/utils/links';
  import { openFunctionLine } from '$lib/utils/ide-links';
  import { setQuery } from '$lib/utils/query';
  import GraphCanvasFlow from '$lib/components/contract/GraphCanvasFlow.svelte';
  import BoxNode, { type BoxNodeData, type BoxPort } from './BoxNode.svelte';
  import CaptionNode from './CaptionNode.svelte';
  import ChipEdge, { type ChipEdgeData } from './ChipEdge.svelte';
  import Icon from './Icon.svelte';
  import Row from './Row.svelte';
  import { FIT_PADDING, placeChips, type Point, type Side } from './chips';
  import { ASSET_ORDER, POOL_ASSET, POOL_ASSET_LONG, assetCounts, assetName, buildFlow, type FlowArrow, type FlowParty } from './value-graph';

  const VALUE = '#7aba8a';
  const AMBER = '#f0b72f';
  const ACCENT = '#8bb8e8';
  const WIDTH = 200;
  const GAP_X = 130;
  const GAP_Y = 34;
  const CAPTIONS = ['Pays in', 'Inside the project', '', 'Leaves the project'];

  let data = $state<ValueFlowPairs | null>(null);
  let error = $state<string | null>(null);
  let tip = $state<{ x: number; y: number; arrow: FlowArrow } | null>(null);

  const asset = $derived(page.url.searchParams.get('asset'));
  const selected = $derived(page.url.searchParams.get('sel'));
  const picked = $derived(page.url.searchParams.get('arrow'));
  const counts = $derived(data ? assetCounts(data) : new Map<string, number>());
  const total = $derived([...counts.values()].reduce((a, b) => a + b, 0));
  const flow = $derived(data ? buildFlow(data, asset) : null);
  const partyOf = $derived(new Map((flow?.parties ?? []).map((p) => [p.key, p])));
  const arrow = $derived(flow?.arrows.find((a) => a.id === picked) ?? null);
  const party = $derived(selected ? (partyOf.get(selected) ?? null) : null);

  const shown = $derived.by(() => {
    if (!flow) return [] as FlowArrow[];
    if (arrow) return [arrow];
    if (party) return flow.arrows.filter((a) => a.from === party.key || a.to === party.key);
    return flow.arrows;
  });

  onMount(async () => {
    try {
      data = await getValueFlow();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  });
  onDestroy(clearGraph);

  function partyLines(p: FlowParty): BoxNodeData['lines'] {
    if (p.kind === 'users') {
      const names = p.expressions.slice(0, 3).join(', ');
      return [{ text: p.expressions.length > 3 ? `${names}…` : names, tone: 'mono' }];
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

  function draw() {
    if (!flow) return;
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

    const sides = (a: FlowArrow): ['l' | 'r', 'l' | 'r'] => {
      const from = partyOf.get(a.from)!;
      const to = partyOf.get(a.to)!;
      if (from.column === to.column) return ['r', 'r'];
      return from.column < to.column ? ['r', 'l'] : ['l', 'r'];
    };
    const ports = new Map<string, (BoxPort & { order: number })[]>();
    const add = (key: string, port: BoxPort & { order: number }) => (ports.get(key) ?? ports.set(key, []).get(key)!).push(port);
    for (const a of arrows) {
      const [s, t] = sides(a);
      add(a.from, { id: `${a.id}:s`, side: s, type: 'source', order: pos.get(a.to)!.y });
      add(a.to, { id: `${a.id}:t`, side: t, type: 'target', order: pos.get(a.from)!.y });
    }
    const anchor = new Map<string, Point & { side: Side }>();
    for (const [key, list] of ports) {
      list.sort((a, b) => a.order - b.order);
      const p = pos.get(key)!;
      for (const side of ['l', 'r'] as const) {
        const at = list.filter((x) => x.side === side);
        at.forEach((port, i) => anchor.set(port.id, { x: p.x + (side === 'r' ? WIDTH : 0), y: p.y + ((h.get(key) ?? 0) * (i + 1)) / (at.length + 1), side }));
      }
    }

    const chipText = (a: FlowArrow) => (a.pool ? `${POOL_ASSET} · ${a.transfers.length}` : `${a.assets.join(' + ')}${a.transfers.length > 1 ? ` · ${a.transfers.length}` : ''}`);
    const boxes = parties.map((p) => ({ ...pos.get(p.key)!, w: WIDTH, h: h.get(p.key) ?? 0 }));
    const touched = new Set(shown.flatMap((a) => [a.from, a.to]));
    const focus = arrow !== null || party !== null;
    const order = arrows.filter((a) => !focus || shown.includes(a)).sort((a, b) => Number(b.pool) - Number(a.pool) || b.transfers.length - a.transfers.length);
    const chipAt = placeChips(order.map((a) => ({ id: a.id, s: anchor.get(`${a.id}:s`)!, t: anchor.get(`${a.id}:t`)!, text: chipText(a) })), boxes);

    const nodes: Node<GraphNodeData>[] = parties.map((p) => ({
      id: p.key,
      type: 'box',
      position: pos.get(p.key)!,
      data: {
        _type: 'box',
        label: p.label,
        icon: p.kind === 'declared' ? 'out' : undefined,
        lines: partyLines(p),
        ports: ports.get(p.key) ?? [],
        variant: p.kind === 'users' ? 'cluster' : '',
        width: WIDTH,
        height: h.get(p.key),
        on: p.key === selected,
        dim: focus && !touched.has(p.key),
      } satisfies BoxNodeData,
    }));
    for (const column of columns) {
      const caption = CAPTIONS[column];
      if (caption) nodes.push({ id: `caption:${column}`, type: 'caption', position: { x: colX.get(column) ?? 0, y: 0 }, selectable: false, data: { _type: 'box', label: caption } });
    }

    const edges: Edge[] = arrows.map((a) => {
      const on = a.id === picked;
      const lit = !focus || shown.includes(a);
      const color = on ? ACCENT : a.pool ? AMBER : VALUE;
      return {
        id: a.id,
        type: 'chip',
        source: a.from,
        target: a.to,
        sourceHandle: `${a.id}:s`,
        targetHandle: `${a.id}:t`,
        zIndex: on ? 2 : undefined,
        markerEnd: { type: MarkerType.ArrowClosed, color, width: 16, height: 16 },
        style: `stroke: ${color}; stroke-width: ${on ? 2.4 : a.pool ? 1.8 : 2}; opacity: ${lit ? 1 : a.pool ? 0.45 : 0.16}${a.pool ? '; stroke-dasharray: 5 4' : ''}`,
        data: {
          chip: chipText(a),
          icon: a.pool ? 'unres' : 'value',
          tone: a.pool ? 'amb' : 'val',
          on,
          labelAt: on ? (chipAt.get(a.id) ?? 0.5) : (chipAt.get(a.id) ?? null),
          onpick: pickArrow,
        } satisfies ChipEdgeData,
      };
    });
    setNodes(nodes);
    setEdges(edges);
  }

  $effect(draw);

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

<div class="toolbar">
  <span class="crumb-here">Value flow</span>
  <span class="tlabel">Asset</span>
  <div class="seg" role="group" aria-label="Asset">
    <button type="button" aria-pressed={asset === null} onclick={() => setAsset(null)}>All {total}</button>
    {#each ASSET_ORDER.filter((a) => counts.has(a)) as name}
      <button type="button" class:amb={name === POOL_ASSET} aria-pressed={asset === name} onclick={() => setAsset(name)} title={name === POOL_ASSET ? POOL_ASSET_LONG : name}>{name} {counts.get(name)}</button>
    {/each}
  </div>
</div>

<div class="work">
  <div class="canvas">
    {#if error}
      <div class="state-msg err">{error}</div>
    {:else if !data}
      <div class="state-msg">Loading value flow…</div>
    {:else if data.pairs.length === 0}
      <div class="state-msg">No value moves between parties in this project.</div>
    {:else}
      <GraphCanvasFlow
        extraNodeTypes={{ box: BoxNode, caption: CaptionNode } as any}
        edgeTypes={{ chip: ChipEdge } as any}
        onnodetap={pickParty}
        onbackgroundtap={() => setQuery({ sel: null, arrow: null })}
        onedgetap={(e) => pickArrow(e.id)}
        onedgemouseenter={onArrowHover}
        onedgemouseleave={() => (tip = null)}
        canDeleteNodes={false}
        keepPositions={false}
        initialFit={{ padding: FIT_PADDING }}
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
    {:else}
      <h1>Value moves</h1>
      <p class="sub">{flow?.transfers.length ?? 0} transfers in {functions} functions</p>
    {/if}
    {#each groups as [name, lines] (name)}
      <h2 class:amb={name === POOL_ASSET}><Icon name={name === POOL_ASSET ? 'unres' : 'value'} />{name === POOL_ASSET ? POOL_ASSET_LONG : name}<span class="n">{lines.reduce((n, l) => n + l.contexts.length, 0)}</span></h2>
      <ul class="list">
        {#each lines as line (line.key)}
          {@const t = line.transfer}
          <Row icon={t.via_library ? 'lib' : 'value'} tone={name === POOL_ASSET ? 'amb' : 'val'} title="{fnName(t.function)} → {line.to}" line={t.span.start_line} onsource={() => openFunctionLine(t.function, t.span.start_line)}>
            {#snippet meta()}
              <span class={name === POOL_ASSET ? 'amb' : 'val'}>{verb(t)} {name}</span>{t.amount_expr ? ` · amount ${t.amount_expr}` : ''}{t.via_library ? ` · through ${t.via_library}` : ''}{line.contexts.some((c) => c !== splitRef(t.function).contract) ? ` · runs inside ${line.contexts.join(', ')}` : ''} · <span title="Transfer id">{t.id}</span>
            {/snippet}
          </Row>
        {/each}
      </ul>
    {/each}
  </aside>
</div>

<div class="legend">
  <span class="tlabel">Key</span>
  <span class="key"><svg width="22" height="8" aria-hidden="true"><line x1="1" y1="4" x2="21" y2="4" stroke={VALUE} stroke-width="2" /></svg>Money moves this way</span>
  <span class="key amb"><svg width="22" height="8" aria-hidden="true"><line x1="1" y1="4" x2="21" y2="4" stroke={AMBER} stroke-width="2" stroke-dasharray="5 4" /></svg>{POOL_ASSET_LONG}</span>
  <span class="key"><Icon name="lib" />Library code it runs</span>
  <span class="hint">Hover an arrow for its transfers, click it to list them</span>
</div>

{#if tip}
  <div class="tip" style="left: {tip.x + 12}px; top: {tip.y + 12}px">
    <b>{partyOf.get(tip.arrow.from)?.label} → {partyOf.get(tip.arrow.to)?.label}</b>
    {'\n'}{describeTransfers(tip.arrow.transfers)}
  </div>
{/if}
