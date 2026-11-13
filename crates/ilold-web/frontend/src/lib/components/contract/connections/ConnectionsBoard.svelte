<script lang="ts">
  import { SvelteMap } from 'svelte/reactivity';
  import type { Connections, ContractDetail, FunctionSummary } from '$lib/api/rest';
  import { contractConnections, contractDetail } from '$lib/stores/project-data';
  import { notifyError } from '$lib/components/ui/notifications.svelte';
  import { lineStyle, AMBIGUOUS_COLOR } from '$lib/utils/links';
  import { restricts } from '$lib/utils/access';
  import Tooltip from '$lib/components/chrome/Tooltip.svelte';
  import { build, edgeBasis, edgeLook, linksOf, rowLabel, LIBRARIES, OUTSIDE, type BoardEdge, type Link } from './board-links';
  import { arrange, assignPorts, geometry, pathD, placeLabels, routeLong, type Placed, type Wire } from './board-layout';
  import { closeBox, contractOf, keyParts, openStep, popStep, touch, type Board } from './board-state';
  import { applyBoard, currentBoard, currentLines, openMain, pan } from './board-view.svelte';

  let { main, libraries }: { main: string; libraries: Set<string> } = $props();

  const ACCENT = '#8bb8e8';
  const board = $derived(currentBoard(main));
  const lines = $derived(currentLines());
  const conns = new SvelteMap<string, Connections>();
  const details = new SvelteMap<string, ContractDetail>();
  let scroller = $state<HTMLDivElement | null>(null);
  let width = $state(0);
  let tip = $state<{ x: number; y: number; wire: Wire } | null>(null);

  $effect(() => {
    for (const c of board.open) {
      if (!conns.has(c)) contractConnections(c).then((v) => conns.set(c, v), (e) => notifyError(`load connections of ${c}`, e));
      if (!details.has(c)) contractDetail(c).then((v) => details.set(c, v), () => {});
    }
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
    return { G, wires: live, lit, valueRows };
  });

  function styleOf(w: Wire): { color: string; width: number; dash: string } {
    const look = edgeLook(w.src, lines);
    const s = look === 'ambiguous' ? lineStyle('calls', edgeBasis(w.src), 'AMBIGUOUS') : lineStyle(look, edgeBasis(w.src));
    if (w.on && (look === 'calls' || look === 'library' || look === 'reads')) return { ...s, color: ACCENT, width: s.width + 0.6 };
    return w.on ? { ...s, width: s.width + 0.6 } : s;
  }
  const markerId = (color: string) => `bm-${color.replace(/[^a-z0-9]/gi, '')}`;
  const colors = $derived([...new Set((scene?.wires ?? []).map((w) => styleOf(w).color))]);

  let panned = '';
  $effect(() => {
    const key = `${board.open.join(',')}|${board.recent.join(',')}|${width}`;
    if (!scene || !scroller || key === panned) return;
    panned = key;
    const G = scene.G;
    const cw = scroller.clientWidth;
    const open = G.nodes.filter((n) => n.ref.type === 'open');
    const strips = G.nodes.filter((n) => n.ref.type === 'strip');
    let left = 0;
    if (G.W > cw && open.length) {
      const L = Math.min(...open.map((n) => n.x));
      const R = Math.max(...open.map((n) => n.x + n.w));
      const newest = open.find((n) => n.ref.c === board.recent.at(-1)) ?? open.at(-1)!;
      const oc = newest.col + (newest.col < 0 ? -1 : 1);
      const outer = G.nodes.filter((n) => n.col === oc && n.ref.type !== 'strip');
      const oR = Math.max(R, ...outer.map((n) => n.x + n.w));
      const oL = Math.min(L, ...outer.map((n) => n.x));
      const sL = strips.length ? Math.min(...strips.map((n) => n.x)) : L;
      const fits = (a: number, b: number) => b - a + 40 <= cw;
      if (strips.length && fits(sL, R)) left = 0;
      else if (newest.col >= 0 && fits(L, oR)) left = L - 20;
      else if (newest.col < 0 && fits(oL, R)) left = R + 20 - cw;
      else if (fits(L, R)) left = (L + R) / 2 - cw / 2;
      else left = newest.x + newest.w / 2 - cw / 2;
    }
    scroller.scrollTo({ left: Math.max(0, Math.min(left, G.W - cw)), top: 0 });
    countHidden();
  });

  function countHidden() {
    if (!scene || !scroller) return;
    const l = scroller.scrollLeft;
    const r = l + scroller.clientWidth;
    pan.left = scene.G.nodes.filter((n) => n.x < l - 4).length;
    pan.right = scene.G.nodes.filter((n) => n.x + n.w > r + 4).length;
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
  function openFolded(c: string, col: number, row: string | null) {
    if (c === OUTSIDE || c === LIBRARIES) return;
    applyBoard(openStep(board, c, col, row && !keyParts(row).member.includes('::') ? row : null));
  }
  function clickNode(n: Placed) {
    if (n.ref.type === 'strip') applyBoard(touch(board, n.ref.c));
    else if (n.ref.type === 'folded') openFolded(n.ref.c, n.ref.col, null);
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
    return r.length === 1 && r[0] === 'callee' ? `${owner} calls` : r.length === 1 ? `Calls ${owner}` : `${owner} links`;
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
  {#if !scene}
    <div class="state-msg">Loading connections…</div>
  {:else}
    {@const G = scene.G}
    <svg width={Math.max(G.W, width)} height={Math.max(G.H, 200)} role="application" aria-label="Connections board">
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
          <path d={w.d} stroke={w.dim ? '#4a5568' : s.color} stroke-width={s.width} stroke-dasharray={s.dash} fill="none" marker-end="url(#{markerId(s.color)})" />
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
            <rect class="box" x={n.x} y={n.y} width={n.w} height={n.h} rx="7" />
            <text class="name" x={n.x + 12} y={n.y + 20}>{fit(name, n.w - 24, NAME_FONT)}</text>
            <text class={n.ref.c === OUTSIDE ? 'amb' : 'meta'} x={n.x + 12} y={n.y + 37}>{n.ref.n} {n.ref.n === 1 ? 'link' : 'links'}</text>
          </g>
          {#each n.rows as { cy, row } (row.key)}
            <g class="row nrow" class:soft={!!board.fn && !scene.lit.has(`${n.id}|${row.key}`)} role="button" tabindex="-1" aria-label="{name} {row.label}{openable ? `. Opens ${n.ref.c} here` : ''}" onclick={() => openable && openFolded(n.ref.c, n.ref.col, row.key)} onkeydown={(e) => onSvgKey(e, () => openable && openFolded(n.ref.c, n.ref.col, row.key))}>
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
  .lbl text { font: 600 12px var(--font-sans); fill: var(--color-text-light); }
</style>
