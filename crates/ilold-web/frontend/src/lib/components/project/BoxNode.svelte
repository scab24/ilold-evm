<script lang="ts" module>
  import type { IconName } from '$lib/components/chrome/Icon.svelte';

  export type Tone = '' | 'val' | 'amb' | 'mono' | 'dim';
  export interface BoxLine { text: string; tone?: Tone; icon?: IconName }
  export interface BoxPort { id: string; side: 'l' | 'r' | 't' | 'b'; type: 'source' | 'target' }
  export interface BoxRow { id: string; text: string; icon?: IconName; tone?: Tone; title?: string; marks?: IconName[]; on?: boolean; label?: string }

  export interface BoxNodeData {
    [key: string]: unknown;
    _type: 'box';
    label: string;
    icon?: IconName;
    lines?: BoxLine[];
    rows?: BoxRow[];
    ports?: BoxPort[];
    rowPorts?: boolean;
    rowAction?: boolean;
    headAction?: string;
    closable?: string;
    variant?: '' | 'out' | 'cluster' | 'abstract' | 'iface' | 'route' | 'open' | 'main';
    width?: number;
    height?: number;
    on?: boolean;
    dim?: boolean;
    faded?: () => boolean;
  }
</script>

<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte';
  import Icon from '$lib/components/chrome/Icon.svelte';

  let { data }: { data: BoxNodeData } = $props();

  const POSITION = { l: Position.Left, r: Position.Right, t: Position.Top, b: Position.Bottom };

  function spread(side: BoxPort['side']): Map<string, string> {
    const ports = (data.ports ?? []).filter((p) => p.side === side);
    const vertical = side === 'l' || side === 'r';
    return new Map(ports.map((p, i) => [p.id, `${vertical ? 'top' : 'left'}: ${((i + 1) * 100) / (ports.length + 1)}%`]));
  }
  const lead = $derived(!!data.rows?.some((r) => r.icon));
  const placed = $derived(new Map([...spread('l'), ...spread('r'), ...spread('t'), ...spread('b')]));
</script>

<div class="box {data.variant ?? ''}" class:on={data.on} class:dim={data.dim || data.faded?.()} style:width={data.width ? `${data.width}px` : undefined} style:min-height={data.height ? `${data.height}px` : undefined}>
  <div class="name">
    {#if data.icon}<Icon name={data.icon} size={14} />{/if}
    {#if data.headAction}<button type="button" class="head" data-head aria-label={data.headAction} title={data.label}>{data.label}</button>{:else}<span title={data.label}>{data.label}</span>{/if}
    {#if data.closable}<button type="button" class="x" data-close aria-label={data.closable} title="Close">×</button>{/if}
  </div>
  {#each data.lines ?? [] as line (line.text)}
    <div class="meta {line.tone ?? ''}">{#if line.icon}<Icon name={line.icon} size={13} />{/if}<span>{line.text}</span></div>
  {/each}
  {#if data.rows?.length}
    <ul class="rows">
      {#each data.rows as row (row.id)}
        <li class="row {row.tone ?? ''}" class:on={row.on} title={row.title ?? row.text}>
          {#if data.rowAction}<button type="button" class="hit" data-row={row.id} aria-pressed={row.on ?? false} aria-label={row.label ?? row.text}></button>{/if}
          {#if row.icon}<Icon name={row.icon} size={13} />{:else if data.rowAction && lead}<span class="lead" aria-hidden="true"></span>{/if}<span>{row.text}</span>
          {#if row.marks?.length}<span class="marks">{#each row.marks as mark (mark)}<Icon name={mark} size={13} />{/each}</span>{/if}
          {#if data.rowPorts}
            <Handle type="target" id="{row.id}:in" position={Position.Left} class="row-in" />
            <Handle type="source" id="{row.id}:out" position={Position.Right} class="row-out" />
            <Handle type="target" id="{row.id}:back" position={Position.Right} class="row-out" />
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>
{#each data.ports ?? [] as port (port.id)}
  <Handle type={port.type} id={port.id} position={POSITION[port.side]} style={placed.get(port.id)} />
{/each}

<style>
  .box { position: relative; box-sizing: border-box; min-width: 150px; padding: 8px 12px 9px; border-radius: 6px; background: var(--color-surface-alt); border: 1.25px solid var(--color-text-dim); cursor: pointer; font: 12px/1.45 var(--font-sans); color: var(--root); transition: opacity 0.15s; }
  .box:hover { border-color: var(--color-accent-hover); }
  .box.on { background: var(--color-tint-accent); border: 2px solid var(--color-accent); }
  .box.dim { opacity: 0.3; }
  .box.abstract { border-style: dashed; }
  .box.iface { border-style: dotted; }
  .box.cluster { border-style: dashed; background: var(--color-surface); }
  .box.out { border: 1.25px dashed var(--amber); background: var(--color-tint-warning); }
  .box.route { background: var(--color-tint-accent); border: 2px solid var(--color-accent); }
  .box.open, .box.main { background: var(--color-tint-accent); border: 2px solid var(--color-accent-dark); }
  .box.main { border-color: var(--color-accent); }
  .box.route.on { border-color: var(--color-accent-hover); box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent-hover) 35%, transparent); }
  .name { display: flex; align-items: center; gap: 6px; font: 600 13px var(--font-sans); color: var(--color-text-light); white-space: nowrap; }
  .name span, .name .head { overflow: hidden; text-overflow: ellipsis; }
  .head { all: unset; cursor: pointer; overflow: hidden; text-overflow: ellipsis; }
  .x { all: unset; margin-left: auto; width: 24px; height: 22px; border-radius: 5px; text-align: center; font: 600 14px/22px var(--font-sans); color: var(--root); cursor: pointer; }
  .x:hover { background: var(--color-hover); color: var(--color-text-light); }
  .head:focus-visible, .x:focus-visible, .hit:focus-visible { outline: 2px solid var(--color-accent-hover); outline-offset: 1px; border-radius: 5px; }
  .meta { display: flex; align-items: flex-start; gap: 5px; overflow-wrap: anywhere; }
  .meta :global(.pj-icon) { margin-top: 2px; }
  .val { color: var(--color-success-light); }
  .amb { color: var(--amber); }
  .mono { font-family: var(--font-mono); }
  .rows { list-style: none; margin: 6px 0 0; padding: 0; }
  .row { position: relative; display: flex; align-items: center; gap: 6px; height: 26px; font: 13px var(--font-mono); color: var(--color-text); white-space: nowrap; }
  .row span { overflow: hidden; text-overflow: ellipsis; }
  .row.amb { font: 12px var(--font-sans); color: var(--amber); }
  .row.dim { color: var(--root); }
  .row.on { background: var(--color-tint-accent); border-radius: 4px; padding-left: 6px; box-shadow: inset 3px 0 0 var(--color-accent); color: var(--color-text-light); }
  .row:has(.hit):hover { background: var(--color-hover); }
  .hit { all: unset; position: absolute; inset: 0; cursor: pointer; }
  .lead { flex: none; width: 13px; }
  .marks { display: flex; gap: 4px; margin-left: auto; color: var(--root); }
  .row :global(.svelte-flow__handle) { opacity: 0; }
  .row :global(.svelte-flow__handle.row-in) { left: -14px; }
  .row :global(.svelte-flow__handle.row-out) { right: -14px; left: auto; }
  :global(.svelte-flow__node-box .svelte-flow__handle) { width: 4px; height: 4px; min-width: 0; min-height: 0; border: 0; background: transparent; }
</style>
