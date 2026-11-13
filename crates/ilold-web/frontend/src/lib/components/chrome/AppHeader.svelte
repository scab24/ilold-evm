<script lang="ts" module>
  export type ProjectTab = 'map' | 'value' | 'route';
  export type Mode = 'cfg' | 'sequences' | 'session' | 'connections';

  export const TABS: { id: ProjectTab; label: string }[] = [
    { id: 'map', label: 'Map' },
    { id: 'value', label: 'Value flow' },
    { id: 'route', label: 'Route' },
  ];

  export const MODES: { id: Mode; label: string; title: string; param: string }[] = [
    { id: 'cfg', label: 'CFG', title: 'Control-flow graph view', param: 'cfg' },
    { id: 'sequences', label: 'Seq', title: 'Function sequence view', param: 'seq' },
    { id: 'session', label: 'Session', title: 'Session mode: click a function to add a step', param: 'session' },
    { id: 'connections', label: 'Connections', title: 'Calls between this contract and others, function to function', param: 'connections' },
  ];

  export const DEFAULT_MODE: Mode = 'connections';

  export function modeOf(param: string | null): Mode {
    return MODES.find((m) => m.param === param)?.id ?? DEFAULT_MODE;
  }

  export function modeParam(mode: Mode): string | null {
    return mode === DEFAULT_MODE ? null : MODES.find((m) => m.id === mode)!.param;
  }

  export function tabHref(id: ProjectTab): string {
    return id === 'map' ? '/' : `/?tab=${id}`;
  }
</script>

<script lang="ts">
  import { togglePalette } from '$lib/stores/palette.svelte';
  import Seg from './Seg.svelte';

  let { count, tab = null, contract = null, mode = null, onmode }: {
    count: number | null;
    tab?: ProjectTab | null;
    contract?: string | null;
    mode?: Mode | null;
    onmode?: (mode: Mode) => void;
  } = $props();
</script>

<header class="top" class:wb={contract !== null}>
  <a class="brand" href="/" title="Project map">ilold</a>
  <span class="sub-brand wide-only">execution path analyzer</span>
  {#if count !== null}<span class="sub-brand">{count} contracts</span>{/if}
  <Seg label="Project views" options={TABS.map((t) => ({ ...t, href: tabHref(t.id) }))} value={tab} />
  {#if contract !== null}
    <span class="tlabel" aria-hidden="true">›</span>
    <span class="crumb-here contract-name" title={contract}>{contract}</span>
    <Seg label="View mode" options={MODES} value={mode} onpick={onmode} />
  {/if}
  <button type="button" class="btn search-btn" onclick={togglePalette}>⌘K Search</button>
</header>
