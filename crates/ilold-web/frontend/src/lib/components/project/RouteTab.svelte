<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { page } from '$app/state';
  import { MarkerType, type Edge, type Node } from '@xyflow/svelte';
  import { getRoute, getValueFlow, type ContractSummary, type RouteCall, type Routes, type ValueTransfer } from '$lib/api/rest';
  import { setNodes, setEdges, clearGraph, type GraphNodeData } from '$lib/stores/graph.svelte';
  import { RELATION_STYLE, splitRef } from '$lib/utils/links';
  import { openInIde } from '$lib/utils/ide-links';
  import { setQuery } from '$lib/utils/query';
  import GraphCanvasFlow from '$lib/components/contract/GraphCanvasFlow.svelte';
  import BoxNode, { type BoxNodeData, type BoxRow } from './BoxNode.svelte';
  import ChipEdge, { type ChipEdgeData } from './ChipEdge.svelte';
  import Icon from './Icon.svelte';
  import Row from './Row.svelte';
  import { FIT_PADDING, NEAR_SOURCE, placeChips } from './chips';
  import { transferKey } from './value-graph';
  import { assetName } from '$lib/utils/value';

  let { contracts }: { contracts: ContractSummary[] } = $props();

  const ROUTE = 'var(--color-accent-hover)';
  const VALUE = RELATION_STYLE.value.color;
  const WIDTH = 220;
  const GAP_X = 230;
  const NOT_RECORDED = 'not-recorded';
  const OTHER_ROUTES = 'other-routes';

  const deployable = $derived(contracts.filter((c) => c.kind === 'Contract' && !c.is_dependency).map((c) => c.name).sort());
  const known = (name: string | null) => (name !== null && deployable.includes(name) ? name : '');
  const from = $derived(known(page.url.searchParams.get('from')));
  const to = $derived(known(page.url.searchParams.get('to')));
  const same = $derived(from !== '' && from === to);
  const reads = $derived(page.url.searchParams.get('reads') === '1');

  let routes = $state<Routes | null>(null);
  let error = $state<string | null>(null);
  let moved = $state<Map<string, ValueTransfer> | null>(null);

  onMount(async () => {
    try {
      moved = new Map((await getValueFlow()).transfers.map((t) => [transferKey(t), t]));
    } catch {
      moved = new Map();
    }
  });
  onDestroy(clearGraph);

  $effect(() => {
    const [a, b, r] = [from, to, reads];
    routes = null;
    error = null;
    if (!a || !b || a === b) return;
    getRoute(a, b, r).then(
      (res) => { if (a === from && b === to && r === reads) routes = res; },
      (e) => { error = e instanceof Error ? e.message : String(e); },
    );
  });

  const chosen = $derived.by(() => {
    const count = routes?.routes.length ?? 0;
    const asked = Number.parseInt(page.url.searchParams.get('route') ?? '1', 10);
    return count === 0 || !Number.isInteger(asked) ? 0 : Math.min(Math.max(asked, 1), count) - 1;
  });
  const route = $derived(routes?.routes[chosen] ?? null);

  function member(ref: string): string {
    return splitRef(ref).member;
  }
  function transfersOf(call: RouteCall): ValueTransfer[] {
    return call.call.transfers.map((id) => moved?.get(transferKey({ in_context: call.call.in_context, id }))).filter((t): t is ValueTransfer => !!t);
  }
  function valueText(call: RouteCall): string {
    const ts = transfersOf(call);
    if (!ts.length) return call.call.transfers.length ? 'moves value' : '';
    return ts.map((t) => `sends ${assetName(t)}${t.amount_expr ? ` · amount ${t.amount_expr}` : ''}`).join(' · ');
  }

  function draw() {
    if (!moved) return;
    if (!routes || routes.routes.length === 0) {
      setNodes([]);
      setEdges([]);
      return;
    }
    const depth = routes.routes[0].steps.length;
    const at = new Map<string, { column: number; entries: string[]; exits: string[]; mine: Set<string>; hidden: boolean; on: boolean }>();
    const node = (column: number, contract: string) => {
      const id = `${column}:${contract}`;
      if (!at.has(id)) at.set(id, { column, entries: [], exits: [], mine: new Set(), hidden: false, on: false });
      return at.get(id)!;
    };
    const edges = new Map<string, { source: string; target: string; s: string; t: string; lines: number[]; assets: string[]; value: boolean; on: boolean }>();
    routes.routes.forEach((r, ri) => {
      r.steps.forEach((step, si) => {
        const a = node(si, step.from);
        const b = node(si + 1, step.to);
        a.on ||= ri === chosen;
        b.on ||= ri === chosen;
        for (const call of step.calls) {
          const s = member(call.call.function);
          const t = member(call.callee.function);
          if (!a.exits.includes(s)) a.exits.push(s);
          if (!b.entries.includes(t)) b.entries.push(t);
          if (ri === chosen) {
            a.mine.add(s);
            b.mine.add(t);
          }
          if (!call.recorded) a.hidden = true;
          const id = `${si}:${step.from}:${s}>${step.to}:${t}`;
          const e = edges.get(id) ?? { source: `${si}:${step.from}`, target: `${si + 1}:${step.to}`, s, t, lines: [], assets: [], value: false, on: false };
          if (!e.lines.includes(call.call.span.start_line)) e.lines.push(call.call.span.start_line);
          e.value ||= call.call.transfers.length > 0;
          for (const name of transfersOf(call).map(assetName)) if (!e.assets.includes(name)) e.assets.push(name);
          e.on ||= ri === chosen;
          edges.set(id, e);
        }
      });
    });

    const rowsOf = new Map<string, BoxRow[]>();
    for (const [id, n] of at) {
      const other = (f: string) => n.on && !n.mine.has(f);
      const rows: BoxRow[] = n.entries.filter((f) => !other(f)).map((f) => ({ id: f, text: f, icon: 'entry' }));
      if (n.hidden && n.column > 0) rows.push({ id: NOT_RECORDED, text: 'steps inside not recorded', icon: 'unres', tone: 'amb', title: 'How the entry reaches the next call is not in the call graph' });
      for (const f of n.exits) if (!n.entries.includes(f) && !other(f)) rows.push({ id: f, text: f, icon: 'call' });
      const others = [...new Set([...n.entries, ...n.exits])].filter(other);
      if (others.length) rows.push({ id: OTHER_ROUTES, text: `${others.length} more on other routes`, tone: 'dim', title: others.join(', ') });
      rowsOf.set(id, rows);
    }
    const shows = (node: string, f: string) => (rowsOf.get(node)?.some((r) => r.id === f) ? f : OTHER_ROUTES);
    const columns = new Map<number, string[]>();
    for (const [id, n] of at) (columns.get(n.column) ?? columns.set(n.column, []).get(n.column)!).push(id);
    const height = (id: string) => 56 + 26 * (rowsOf.get(id)?.length ?? 0);
    const stack = (ids: string[]) => ids.reduce((sum, id) => sum + height(id) + 40, -40);
    const tallest = Math.max(...[...columns.values()].map(stack));
    const nodes: Node<GraphNodeData>[] = [];
    for (const [column, ids] of columns) {
      ids.sort((x, y) => Number(at.get(y)!.on) - Number(at.get(x)!.on) || x.localeCompare(y));
      let top = (tallest - stack(ids)) / 2;
      for (const id of ids) {
        const n = at.get(id)!;
        const rows = rowsOf.get(id)!;
        const contract = id.slice(id.indexOf(':') + 1);
        nodes.push({
          id,
          type: 'box',
          position: { x: column * (WIDTH + GAP_X), y: top },
          data: {
            _type: 'box',
            label: contract,
            lines: [{ text: column === 0 ? 'start' : column === depth ? 'end' : `step ${column + 1}` }],
            rows,
            rowPorts: true,
            variant: n.on ? 'route' : '',
            width: WIDTH,
            dim: !n.on,
          } satisfies BoxNodeData,
        });
        top += height(id) + 40;
      }
    }

    const rowY = new Map<string, number>();
    for (const n of nodes) {
      const rows = (n.data as BoxNodeData).rows ?? [];
      rows.forEach((r, i) => rowY.set(`${n.id}/${r.id}`, n.position.y + 63 + 26 * i));
    }
    const text = (e: { lines: number[]; value: boolean; assets: string[] }) => {
      const lines = [...e.lines].sort((x, y) => x - y);
      return `${lines.length > 1 ? 'lines' : 'line'} ${lines.join(', ')}${e.value ? ` · sends ${e.assets.join(' + ') || 'value'}` : ''}`;
    };
    const lit = [...edges.entries()].filter(([, e]) => e.on);
    const boxes = nodes.map((n) => ({ x: n.position.x, y: n.position.y, w: WIDTH, h: height(n.id) }));
    const chipAt = placeChips(
      lit.map(([id, e]) => {
        const s = nodes.find((n) => n.id === e.source)!;
        const t = nodes.find((n) => n.id === e.target)!;
        return { id, s: { x: s.position.x + WIDTH, y: rowY.get(`${s.id}/${shows(s.id, e.s)}`) ?? s.position.y, side: 'r' as const }, t: { x: t.position.x, y: rowY.get(`${t.id}/${shows(t.id, e.t)}`) ?? t.position.y, side: 'l' as const }, text: text(e) };
      }),
      boxes,
      NEAR_SOURCE,
    );

    setNodes(nodes);
    setEdges([...edges.entries()].map(([id, e]) => {
      const color = e.value ? VALUE : ROUTE;
      return {
        id,
        type: 'chip',
        source: e.source,
        target: e.target,
        sourceHandle: `${shows(e.source, e.s)}:out`,
        targetHandle: `${shows(e.target, e.t)}:in`,
        markerEnd: { type: MarkerType.ArrowClosed, color, width: 16, height: 16 },
        style: `stroke: ${color}; stroke-width: ${e.on ? 2 : 1.4}; opacity: ${e.on ? 1 : 0.25}`,
        data: {
          chip: e.on ? text(e) : undefined,
          icon: e.value ? 'value' : undefined,
          tone: e.value ? 'val' : '',
          labelAt: chipAt.get(id) ?? null,
        } satisfies ChipEdgeData,
      } satisfies Edge;
    }));
  }

  $effect(draw);

  function pick(key: 'from' | 'to', value: string) {
    setQuery({ [key]: value || null, route: null });
  }
</script>

<div class="toolbar">
  <span class="crumb-here">Route</span>
  <label class="tlabel" for="route-from">From</label>
  <select id="route-from" class="pick" value={from} onchange={(e) => pick('from', e.currentTarget.value)}>
    <option value="">Pick a contract</option>
    {#each deployable as name}<option value={name} disabled={name === to}>{name}</option>{/each}
  </select>
  <button type="button" class="btn" onclick={() => setQuery({ from: to || null, to: from || null, route: null })} disabled={!from && !to}><Icon name="swap" />Swap</button>
  <label class="tlabel" for="route-to">To</label>
  <select id="route-to" class="pick" value={to} onchange={(e) => pick('to', e.currentTarget.value)}>
    <option value="">Pick a contract</option>
    {#each deployable as name}<option value={name} disabled={name === from}>{name}</option>{/each}
  </select>
  <button type="button" class="btn" aria-pressed={reads} onclick={() => setQuery({ reads: reads ? null : '1', route: null })}>Include reads</button>
</div>

<div class="work">
  <div class="canvas">
    {#if !from || !to}
      <div class="state-msg">Pick a start and an end contract to see how one reaches the other.</div>
    {:else if same}
      <div class="state-msg">Pick two different contracts: a route goes from one contract to another.</div>
    {:else if error}
      <div class="state-msg err">{error}</div>
    {:else if !routes}
      <div class="state-msg">Finding routes…</div>
    {:else if routes.routes.length === 0}
      <div class="state-msg">No route from {from} to {to} following calls{reads ? ' and reads' : ''}.{reads ? '' : ' Including reads may find one.'}</div>
    {:else}
      {#key `${from}>${to}>${reads}`}
        <GraphCanvasFlow
          extraNodeTypes={{ box: BoxNode } as any}
          edgeTypes={{ chip: ChipEdge } as any}
          onnodetap={(n) => {
            const i = routes?.routes.findIndex((r) => r.steps.some((s, si) => `${si}:${s.from}` === n.id || `${si + 1}:${s.to}` === n.id)) ?? -1;
            if (i >= 0 && i !== chosen) setQuery({ route: String(i + 1) });
          }}
          canDeleteNodes={false}
        keepPositions={false}
          initialFit={{ padding: FIT_PADDING, maxZoom: 1.1 }}
          minZoom={0.2}
        />
      {/key}
    {/if}
  </div>
  <aside class="insp" aria-label="Route">
    {#if routes && route}
      <h1>{routes.from} → {routes.to}</h1>
      <p class="sub">Route {chosen + 1} of {routes.routes.length} · {route.steps.length} {route.steps.length === 1 ? 'hop' : 'hops'} · following calls{reads ? ' and reads' : ''}</p>
      {#if routes.routes.length > 1}
        <div class="seg" role="group" aria-label="Route" style="margin-top: 10px; flex-wrap: wrap">
          {#each routes.routes as _, i}
            <button type="button" aria-pressed={i === chosen} onclick={() => setQuery({ route: String(i + 1) })}>{i + 1}</button>
          {/each}
        </div>
      {/if}
      {#if routes.truncated}
        <p class="note amb"><Icon name="unres" /> More routes of this length exist; the first {routes.routes.length} are shown.</p>
      {/if}
      {#each route.steps as step, si}
        <h2><Icon name="call" />{si + 1} · {step.from} → {step.to}<span class="n">{step.calls.length}</span></h2>
        <ul class="list">
          {#each step.calls as call}
            {@const value = valueText(call)}
            <Row icon={call.recorded ? (value ? 'value' : 'call') : 'unres'} tone={call.recorded ? (value ? 'val' : '') : 'amb'} title="{member(call.call.function)} → {step.to}.{member(call.callee.function)}" line={call.call.span.start_line} onsource={call.call.file ? () => openInIde(call.call.file!, call.call.span.start_line) : undefined}>
              {#snippet meta()}
                {call.call.kind}{#if value}{' · '}<span class="val">{value}</span>{/if}{call.call.in_try ? ' · inside try' : ''}{#if !call.recorded}{' · '}<span class="amb">not recorded from the previous hop</span>{/if}
              {/snippet}
            </Row>
          {/each}
        </ul>
        {#if step.calls.some((c) => !c.recorded)}
          <p class="note amb">How {step.from} gets from the previous hop to these calls is not recorded: the call graph has no internal path between them.</p>
        {/if}
      {/each}
    {:else}
      <h1>Route</h1>
      <p class="sub">The shortest ways one contract reaches another, function to function.</p>
    {/if}
  </aside>
</div>

<div class="legend">
  <span class="tlabel">Key</span>
  <span class="key"><svg width="22" height="8" aria-hidden="true"><line x1="1" y1="4" x2="21" y2="4" style:stroke={ROUTE} stroke-width="2" /></svg>Route, function to function</span>
  <span class="key"><svg width="22" height="8" aria-hidden="true"><line x1="1" y1="4" x2="21" y2="4" style:stroke={VALUE} stroke-width="2" /></svg>Hop that moves value</span>
  <span class="key amb"><Icon name="unres" />Steps inside a contract not recorded</span>
  <span class="key"><svg width="22" height="8" aria-hidden="true"><line x1="1" y1="4" x2="21" y2="4" style:stroke={ROUTE} stroke-width="2" opacity="0.3" /></svg>Other route (dimmed)</span>
</div>
