<script lang="ts">
  import { tick } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import type { Connections, ContractDetail, FunctionSummary } from '$lib/api/rest';
  import { contractConnections, contractDetail } from '$lib/stores/project-data';
  import { restricts } from '$lib/utils/access';
  import Tooltip from '$lib/components/chrome/Tooltip.svelte';
  import { edgeStyle, lookOf, LIBRARIES, OUTSIDE } from '$lib/components/project/map-graph';
  import { build, edgeBasis, linksOf, rowLabel, type Link } from './board-links';
  import { arrange, assignPorts, columnSpans, geometry, panLeft, pathD, placeLabels, routeLong, type Placed, type Wire } from './board-layout';
  import { closeBox, contractOf, keyParts, openStep, popStep, touch, type Board } from './board-state';
  import { applyBoard, currentBoard, currentLines, openMain, pan } from './board-view.svelte';

  let { main, libraries }: { main: string; libraries: Set<string> } = $props();

  const ACCENT = '#8bb8e8';
  const board = $derived(currentBoard(main));
  const lines = $derived(currentLines());
  const conns = new SvelteMap<string, Connections>();
  const details = new SvelteMap<string, ContractDetail>();
  const failed = new SvelteMap<string, string>();
  let scroller = $state<HTMLDivElement | null>(null);
  let width = $state(0);
  let tip = $state<{ x: number; y: number; wire: Wire } | null>(null);

  $effect(() => {
    for (const c of board.open) {
      if (failed.has(c)) continue;
      if (!conns.has(c)) contractConnections(c).then((v) => conns.set(c, v), (e) => failed.set(c, e instanceof Error ? e.message : String(e)));
      if (!details.has(c)) contractDetail(c).then((v) => details.set(c, v), () => {});
    }
  });

  const lonely = $derived.by(() => {
    const c = conns.get(main);
    return !!c && !c.outgoing.length && !c.incoming.length && !c.unresolved.length;
  });
  const links = $derived(linksOf(board.open.map((c) => conns.get(c)).filter((c): c is Connections => !!c), libraries));
  const fnInfo = (c: string, m: string): FunctionSummary | undefined => {
    const d = details.get(c);
    return d ? [...d.functions, ...(d.inherited_functions ?? [])].find((f) => f.key === m) : undefined;
  };
  const functionsOf = (c: string) => {
    const d = details.get(c);
    return d ? [...d.functions, ...(d.inherited_functions ?? [])].map((f) => f.key) : [];
  };

  const ctx = typeof document === 'undefined' ? null : document.createElement('canvas').getContext('2d');
  function textWidth(text: string, font = '600 12px -apple-system, BlinkMacSystemFont, sans-serif'): number {
    if (!ctx) return text.length * 7;
    ctx.font = font;
    return ctx.measureText(text).width;
  }
  function fit(text: string, max: number, font?: string): string {
    if (textWidth(text, font) <= max) return text;
    let s = text;
    while (s.length > 1 && textWidth(`${s}…`, font) > max) s = s.slice(0, -1);
    return `${s}…`;
  }
  const NAME_FONT = '600 13px -apple-system, BlinkMacSystemFont, sans-serif';
  const MONO_FONT = "13px 'JetBrains Mono', ui-monospace, monospace";
  const MONO12 = "12px 'JetBrains Mono', ui-monospace, monospace";

  const scene = $derived.by(() => {
    if (!conns.has(main) || width === 0) return null;
    const B = build(board, links, lines, functionsOf);
    const cols = arrange(B, board.open);
    const G = geometry(cols, width, (s) => textWidth(s, NAME_FONT));
    const N = new Map(G.nodes.map((n) => [n.id, n]));
    const fnBox = board.fn ? `O:${contractOf(board.fn)}` : null;
    const touches = (id: string, row: string) => id === fnBox && row === board.fn;
    const wires: Wire[] = B.edges.map((e, i) => {
      const on = !!board.fn && (touches(e.a, e.ar) || touches(e.b, e.br));
      const lineNos = [...new Set(e.links.map((l) => l.row.span.start_line))].sort((a, b) => a - b);
      const value = e.links.some((l) => l.value);
      return {
        id: `w${i}`, src: e, a: { n: e.a, row: e.ar }, b: { n: e.b, row: e.br },
        same: (N.get(e.a)?.col ?? 0) < 0 ? 'left' : 'right', dim: false, soft: !!board.fn && !on, on,
        kmax: e.self ? G.gap * 0.42 : undefined,
        label: on ? { text: `${lineNos.length > 1 ? 'lines' : 'line'} ${lineNos.join(', ')}${value ? ' · moves value' : ''}` } : undefined,
      };
    });
    B.dimmed.forEach((e, i) => wires.push({ id: `d${i}`, src: e, a: { n: e.a, row: '' }, b: { n: e.b, row: '' }, same: 'right', dim: true, soft: false, on: false }));
    wires.sort((x, y) => (x.dim ? 0 : x.soft ? 1 : 2) - (y.dim ? 0 : y.soft ? 1 : 2));
    assignPorts(G.nodes, wires);
    routeLong(wires, G);
    const live = wires.filter((w) => !w.skip);
    live.forEach(pathD);
    placeLabels(live.filter((w) => w.on), G.nodes, (s) => textWidth(s));
    const lit = new Set(live.filter((w) => !w.dim && !w.soft).flatMap((w) => [`${w.a.n}|${w.a.row}`, `${w.b.n}|${w.b.row}`]));
    const valueRows = new Set(live.filter((w) => w.src.links.some((l) => l.value)).map((w) => `${w.a.n}|${w.a.row}`));
    const heads = new Map(live.map((w) => [`${w.b.n}|${w.b.side}|${Math.round(w.b.y!)}`, w]));
    const strip = new Set(G.nodes.filter((n) => n.ref.type === 'strip').map((n) => n.id));
    const chips = new Map<string, { x: number; y: number; side: string; n: number }>();
    for (const w of live) {
      for (const end of [w.a, w.b]) {
        if (!strip.has(end.n)) continue;
        const k = `${end.n}|${end.side}`;
        const c = chips.get(k) ?? chips.set(k, { x: end.x!, y: end.y!, side: end.side!, n: 0 }).get(k)!;
        c.n += w.src.links.length;
      }
    }
    return { G, wires: live, lit, valueRows, arrowed: new Set(heads.values()), chips: [...chips.values()].filter((c) => c.n > 1) };
  });

  function styleOf(w: Wire): { color: string; width: number; dash: string } {
    const look = lookOf(w.src.links, lines);
    const s = edgeStyle({ look, basis: edgeBasis(w.src) });
    if (w.on && (look === 'calls' || look === 'library' || look === 'reads')) return { ...s, color: ACCENT, width: s.width + 0.6 };
    return w.on ? { ...s, width: s.width + 0.6 } : s;
  }
  const markerId = (color: string) => `bm-${color.replace(/[^a-z0-9]/gi, '')}`;
  const colors = $derived([...new Set((scene?.wires ?? []).map((w) => styleOf(w).color))]);

  let panned = '';
  let room = $state(0);
  $effect(() => {
    if (!scene || !scroller) return;
    const G = scene.G;
    const cw = scroller.clientWidth;
    const key = `${board.open.join(',')}|${board.recent.join(',')}|${width}|${G.W}`;
    const chosen = board.fn ? G.nodes.find((n) => n.id === `O:${contractOf(board.fn!)}`) : undefined;
    const view = scroller.scrollLeft;
    const hidden = !!chosen && (chosen.x < view || chosen.x + chosen.w > view + cw);
    if (key === panned && !hidden) return;
    panned = key;
    const open = G.nodes.filter((n) => n.ref.type === 'open');
    const strips = G.nodes.filter((n) => n.ref.type === 'strip');
    const newest = open.find((n) => n.ref.c === board.recent.at(-1)) ?? open.at(-1);
    const span = (list: { x: number; w: number }[]): [number, number] => [Math.min(...list.map((n) => n.x)), Math.max(...list.map((n) => n.x + n.w))];
    const outward = newest ? columnSpans(G).find((c) => c.col === newest.col + (newest.col < 0 ? -1 : 1)) : undefined;
    const keep = [newest, chosen].filter((n): n is Placed => !!n);
    const targets: [number, number][] = [
      ...(strips.length ? [span([...strips, ...open])] : []),
      ...(outward ? [span([...keep, { x: outward.l, w: outward.r - outward.l }])] : []),
      ...(open.length ? [span([...open, ...keep])] : []),
      ...(keep.length ? [span(keep)] : []),
      ...(chosen ? [span([chosen])] : []),
    ];
    const left = panLeft(G, targets, cw, view);
    room = Math.max(0, left + cw - G.W);
    tick().then(() => {
      scroller?.scrollTo({ left, top: scroller.scrollTop });
      countHidden();
    });
  });

  function countHidden() {
    if (!scene || !scroller) return;
    const l = scroller.scrollLeft;
    const r = l + scroller.clientWidth;
    const pad = (n: Placed) => (n.ref.type === 'open' || n.ref.type === 'strip' ? 0 : 8);
    pan.left = scene.G.nodes.filter((n) => n.x - pad(n) < l - 1).length;
    pan.right = scene.G.nodes.filter((n) => n.x + n.w + pad(n) > r + 1).length;
  }
  $effect(() => {
    pan.by = (direction) => scroller?.scrollBy({ left: direction * scroller.clientWidth * 0.6, behavior: 'smooth' });
    return () => {
      pan.by = null;
      pan.left = pan.right = 0;
    };
  });

  function selectRow(c: string, key: string) {
    applyBoard({ ...touch(board, c), fn: board.fn === key ? null : key });
  }
  function openFolded(n: Placed, row: string | null) {
    const { c, col, owner } = n.ref;
    if (n.ref.type !== 'folded' || !owner) return;
    applyBoard(openStep(board, c, col, row && !keyParts(row).member.includes('::') ? row : null, owner));
  }
  function clickNode(n: Placed) {
    if (n.ref.type === 'strip') applyBoard(touch(board, n.ref.c));
    else openFolded(n, null);
  }
  function switchMain(c: string) {
    if (c !== main && c !== OUTSIDE && c !== LIBRARIES) openMain(c);
  }
  function onKey(event: KeyboardEvent) {
    const t = event.target instanceof Element ? event.target : null;
    if (t?.closest('input, select, textarea, [contenteditable="true"]')) return;
    if (event.key === 'Backspace') {
      const next = popStep(board);
      event.preventDefault();
      if (next) applyBoard(next);
    } else if (event.key === 'Escape' && board.fn) applyBoard({ ...board, fn: null });
  }
  function onSvgKey(event: KeyboardEvent, act: () => void) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      act();
    }
  }
  function tipLines(w: Wire): string[] {
    return w.src.links.slice(0, 6).map((l: Link) => `${rowLabel(l.fromKey, l.from)} → ${l.to === OUTSIDE ? rowLabel(l.toKey, '') : `${l.to === LIBRARIES ? '' : `${l.to}.`}${rowLabel(l.toKey, l.to)}`} · line ${l.row.span.start_line}${l.value ? ' · moves value' : ''}`).concat(w.src.links.length > 6 ? [`and ${w.src.links.length - 6} more`] : []);
  }
  function bandTitle(owner: string, roles: Set<string>): string {
    const r = [...roles];
    return r.length === 1 && r[0] === 'callee' ? `${owner} calls` : r.length === 1 ? `Callers of ${owner}` : `${owner} links`;
  }
  function foldName(n: Placed): string {
    if (n.ref.c === LIBRARIES) return 'Libraries';
    if (n.ref.c === OUTSIDE) {
      const ifaces = [...new Set([...n.ref.rows.keys()].map((k) => keyParts(k).member.split('::')[0]))];
      return ifaces.length === 1 ? `${ifaces[0]} (outside)` : 'Outside the project';
    }
    return n.ref.c;
  }
  function entry(f: FunctionSummary | undefined) {
    return !!f && (f.visibility === 'External' || f.visibility === 'Public');
  }
  function restricted(f: FunctionSummary | undefined) {
    return !!f && ((typeof f.access === 'object' && 'Restricted' in f.access) || f.guards.some(restricts));
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="board" bind:this={scroller} bind:clientWidth={width} onscroll={countHidden}>
  {#if failed.has(main) && !conns.has(main)}
    <div class="empty-state" role="alert">
      <div class="empty-title">Could not load the connections of {main}</div>
      <div class="empty-hint err">{failed.get(main)}</div>
      <button type="button" class="chip" onclick={() => failed.delete(main)}>Retry</button>
    </div>
  {:else if !scene}
    <div class="empty-state"><div class="empty-hint">Loading connections…</div></div>
  {:else if lonely && !board.all}
    <div class="empty-state">
      <div class="empty-icon" aria-hidden="true">◇</div>
      <div class="empty-title">No connections</div>
      <div class="empty-hint">{main} calls no other contract and no other contract calls it.</div>
      <button type="button" class="chip" onclick={() => applyBoard({ ...board, all: true })}>Show all its functions</button>
    </div>
  {:else}
    {@const G = scene.G}
    <svg width={Math.max(G.W + room, width)} height={Math.max(G.H, 200)} role="application" aria-label="Connections board">
      <defs>
        {#each colors as color (color)}
          <marker id={markerId(color)} viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,1L9,5L0,9z" fill={color} /></marker>
        {/each}
      </defs>
      {#each G.bands as band, i (i)}
        <g class="band"><rect x={band.x} y={band.y} width={band.w} height={band.h} rx="10" /><text x={band.x + 10} y={band.y + 17}>{fit(bandTitle(band.owner, band.roles), band.w - 20)}</text></g>
      {/each}
      {#each scene.wires as w (w.id)}
        {@const s = styleOf(w)}
        <g class="wire" class:soft={w.soft} class:dim={w.dim} role="presentation" onmousemove={(e) => (tip = { x: e.clientX, y: e.clientY, wire: w })} onmouseleave={() => (tip = null)}>
          <path d={w.d} stroke={w.dim ? '#4a5568' : s.color} stroke-width={s.width} stroke-dasharray={s.dash} fill="none" marker-end={scene.arrowed.has(w) ? `url(#${markerId(s.color)})` : undefined} />
          <path class="hit" d={w.d} />
        </g>
      {/each}
      {#each G.nodes as n (n.id)}
        {#if n.ref.type === 'strip'}
          {@const cx = n.x + n.w / 2}
          {@const cy = n.y + n.h / 2}
          <g class="node strip" class:main={n.ref.c === main} role="button" tabindex="0" aria-label="{n.ref.c}, folded on the board. Enter expands it" onclick={() => clickNode(n)} onkeydown={(e) => onSvgKey(e, () => clickNode(n))}>
            <title>Expand {n.ref.c}</title>
            <rect class="box" x={n.x} y={n.y} width={n.w} height={n.h} rx="8" />
            <text class="name" x={cx} y={cy} text-anchor="middle" dominant-baseline="central" transform="rotate(-90 {cx} {cy})">{n.ref.c}</text>
          </g>
        {:else if n.ref.type === 'open'}
          {@const c = n.ref.c}
          <g class="node open" class:main={c === main} role="button" tabindex="0" aria-label={c === main ? `${c}, main contract` : `${c}. Double-click or Enter makes it the main contract`} ondblclick={() => switchMain(c)} onkeydown={(e) => onSvgKey(e, () => switchMain(c))}>
            <title>{c}</title>
            <rect class="box" x={n.x} y={n.y} width={n.w} height={n.h} rx="8" />
            <text class="big" x={n.x + 14} y={n.y + 25}>{fit(c, n.w - (c === main ? 28 : 56), '600 15px -apple-system, sans-serif')}</text>
            <text class="meta" x={n.x + 14} y={n.y + 47}>{c === main ? 'main contract' : 'double-click to make it main'}</text>
          </g>
          {#if c !== main}
            <g class="xbtn" role="button" tabindex="0" aria-label="Close {c} on the board" onclick={() => applyBoard(closeBox(board, c))} onkeydown={(e) => onSvgKey(e, () => applyBoard(closeBox(board, c)))}>
              <title>Close</title><rect x={n.x + n.w - 34} y={n.y + 8} width="26" height="24" rx="5" /><text x={n.x + n.w - 21} y={n.y + 25} text-anchor="middle">×</text>
            </g>
          {/if}
          {#each n.rows as { cy, row } (row.key)}
            {@const m = keyParts(row.key).member}
            {@const f = fnInfo(c, m)}
            {@const on = board.fn === row.key}
            {@const val = scene.valueRows.has(`${n.id}|${row.key}`) || !!f?.value_transfers?.length}
            <g class="row" class:sel={on} role="button" tabindex="0" aria-pressed={on} aria-label="{c}.{row.label}{entry(f) ? ', entry point' : ''}{restricted(f) ? ', restricted' : ''}{val ? ', moves value' : ''}" onclick={() => selectRow(c, row.key)} onkeydown={(e) => onSvgKey(e, () => selectRow(c, row.key))}>
              <title>{row.title}</title>
              <rect class="rbg" x={n.x + 4} y={cy - n.rh / 2 + 1} width={n.w - 8} height={n.rh - 2} rx="5" />
              <rect class="bar" x={n.x + 4} y={cy - n.rh / 2 + 4} width="3" height={n.rh - 8} rx="1.5" />
              {#if entry(f)}<text class="ico" x={n.x + 14} y={cy + 4}>→</text>{/if}
              <text class="t-row" x={n.x + 32} y={cy + 4.5}>{fit(row.label, n.w - 42 - (restricted(f) ? 18 : 0) - (val ? 18 : 0), MONO_FONT)}</text>
              {#if restricted(f)}<text class="ico" x={n.x + n.w - (val ? 40 : 22)} y={cy + 4}>🔒</text>{/if}
              {#if val}<circle class="val" cx={n.x + n.w - 15} cy={cy} r="5" />{/if}
            </g>
          {/each}
        {:else}
          {@const name = foldName(n)}
          {@const openable = n.ref.type === 'folded'}
          <g class="node fold" class:out={n.ref.c === OUTSIDE} class:cluster={n.ref.type === 'group'} role="button" tabindex="0" aria-label="{name}, {n.ref.n} links{openable ? '. Enter opens it here' : ''}" onclick={() => clickNode(n)} ondblclick={() => switchMain(n.ref.c)} onkeydown={(e) => onSvgKey(e, () => clickNode(n))}>
            <title>{name}</title>
            <rect class="box" x={n.x} y={n.y} width={n.w} height={n.h} rx="7" />
            <text class="name" x={n.x + 12} y={n.y + 20}>{fit(name, n.w - 24, NAME_FONT)}</text>
            <text class={n.ref.c === OUTSIDE ? 'amb' : 'meta'} x={n.x + 12} y={n.y + 37}>{n.ref.n} {n.ref.n === 1 ? 'link' : 'links'}</text>
          </g>
          {#each n.rows as { cy, row } (row.key)}
            <g class="row nrow" class:soft={!!board.fn && !scene.lit.has(`${n.id}|${row.key}`)} role="button" tabindex="-1" aria-label="{name} {row.label}{openable ? `. Opens ${n.ref.c} here` : ''}" onclick={() => openFolded(n, row.key)} onkeydown={(e) => onSvgKey(e, () => openFolded(n, row.key))}>
              <title>{row.title}</title>
              <rect class="rbg" x={n.x + 4} y={cy - n.rh / 2 + 1} width={n.w - 8} height={n.rh - 2} rx="5" />
              <text class="t-nrow" class:amb={row.amb} x={n.x + 12} y={cy + 4}>{fit(row.label, n.w - 24, MONO12)}</text>
            </g>
          {/each}
        {/if}
      {/each}
      {#each scene.wires as w (w.id)}
        {#if !w.dim}
          {#each [w.a, w.b] as end, i (i)}
            {#if end.row}<circle class="port" class:on={!w.soft} cx={end.x} cy={end.y} r="3" />{/if}
          {/each}
        {/if}
        {#if w.label?.box}
          {@const r = w.label.box}
          {@const s = styleOf(w)}
          <g class="lbl"><rect x={r.x} y={r.y} width={r.w} height={r.h} rx="10" style:stroke={s.color} /><text x={r.x + 8} y={r.y + 14}>{w.label.text}</text></g>
        {/if}
      {/each}
      {#each scene.chips as c (`${c.x}|${c.y}|${c.side}`)}
        {@const cw = String(c.n).length * 7.5 + 12}
        <g class="pchip" aria-hidden="true"><rect x={c.side === 'left' ? c.x - cw - 12 : c.x + 12} y={c.y - 10} width={cw} height="20" rx="10" /><text x={c.side === 'left' ? c.x - cw / 2 - 12 : c.x + 12 + cw / 2} y={c.y + 4} text-anchor="middle">{c.n}</text></g>
      {/each}
    </svg>
  {/if}
</div>
<div class="fade l" class:on={pan.left > 0}></div>
<div class="fade r" class:on={pan.right > 0}></div>

{#if tip}
  <Tooltip x={tip.x} y={tip.y} within={scroller}>{#each tipLines(tip.wire) as line, i (i)}{i ? '\n' : ''}{line}{/each}{#if tip.wire.dim}{'\n'}Dimmed in the legend: set it to shown to see each function{/if}</Tooltip>
{/if}

<style>
  .board { position: absolute; inset: 0; overflow: auto; background: var(--color-dark); }
  .fade { position: absolute; top: 0; bottom: 0; width: 28px; pointer-events: none; opacity: 0; transition: opacity 150ms; }
  .fade.on { opacity: 1; }
  .fade.l { left: 0; background: linear-gradient(90deg, var(--color-dark), transparent); }
  .fade.r { right: 0; background: linear-gradient(-90deg, var(--color-dark), transparent); }
  svg { display: block; }
  svg :focus { outline: none; }
  .band rect { fill: rgba(255, 255, 255, 0.018); stroke: var(--color-border-subtle); }
  .band text { font: 600 12px var(--font-sans); fill: var(--root); }
  .wire .hit { stroke: transparent; stroke-width: 10; fill: none; }
  .wire.soft path:first-child { opacity: 0.45; }
  .wire.dim path:first-child { opacity: 0.35; }
  .node { cursor: pointer; }
  .node .box { fill: var(--color-surface-alt); stroke: var(--color-text-dim); stroke-width: 1.25; }
  .node.open .box { fill: var(--color-tint-accent); stroke: var(--color-accent-dark); stroke-width: 2; }
  .node.open.main .box, .node.strip.main .box { stroke: var(--color-accent); }
  .node.strip .box { stroke: var(--color-accent-dark); stroke-width: 1.5; }
  .node.strip:hover .box, .node.fold:hover .box { stroke: var(--color-accent-hover); }
  .node.cluster .box { fill: var(--color-surface); stroke-dasharray: 5 3; }
  .node.out .box { fill: var(--color-tint-warning); stroke: var(--amber); stroke-dasharray: 5 3; }
  .node:focus-visible .box, .row:focus-visible .rbg, .xbtn:focus-visible rect { stroke: var(--color-accent-hover); stroke-width: 2; }
  .big { font: 600 15px var(--font-sans); fill: var(--color-text-light); }
  .name { font: 600 13px var(--font-sans); fill: var(--color-text-light); }
  .meta { font: 12px var(--font-sans); fill: var(--root); }
  .amb { fill: var(--amber); }
  .row { cursor: pointer; }
  .row .rbg { fill: transparent; }
  .row:hover .rbg { fill: var(--color-hover); }
  .row.sel .rbg { fill: var(--color-tint-accent); }
  .row .bar { fill: transparent; }
  .row.sel .bar { fill: var(--color-accent); }
  .row.soft text { opacity: 0.6; }
  .t-row { font: 13px var(--font-mono); fill: var(--color-text-light); }
  .t-nrow { font: 12px var(--font-mono); fill: var(--color-text); }
  .t-nrow.amb { fill: var(--amber); }
  .ico { font: 12px var(--font-sans); fill: var(--root); }
  .val { fill: none; stroke: var(--color-success-light); stroke-width: 1.5; }
  .xbtn { cursor: pointer; }
  .xbtn rect { fill: transparent; stroke: transparent; }
  .xbtn:hover rect { fill: var(--color-hover); stroke: var(--color-border-subtle); }
  .xbtn text { font: 600 14px var(--font-sans); fill: var(--root); }
  .port { fill: var(--root); }
  .port.on { fill: var(--color-accent-hover); }
  .lbl rect { fill: var(--color-surface); }
  .pchip rect { fill: var(--color-surface); stroke: var(--color-accent-dark); }
  .pchip text { font: 600 12px var(--font-sans); fill: var(--color-text-light); }
  .lbl text { font: 600 12px var(--font-sans); fill: var(--color-text-light); }
</style>
