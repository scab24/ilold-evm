<script lang="ts">
  import type { Mode } from '$lib/components/chrome/AppHeader.svelte';
  import { onMount } from 'svelte';
  import { subscribe, getConnectionState } from '$lib/api/ws';
  import type { ConnectionEvent, ConnectionState } from '$lib/api/types';

  let { mode, selectionCount }: { mode: Mode; selectionCount: number } = $props();

  let wsState = $state<ConnectionState>(getConnectionState());

  onMount(() => subscribe('connection', (event: ConnectionEvent) => {
    wsState = event.state;
  }));

  const LABEL: Record<ConnectionState, string> = { connected: 'Connected', connecting: 'Connecting…', disconnected: 'Offline' };
  const HINTS: Record<Mode, [string, string][]> = {
    cfg: [['H', 'Pan'], ['V', 'Select'], ['Delete', 'Remove'], ['⌘K', 'Search']],
    sequences: [['H', 'Pan'], ['V', 'Select'], ['Delete', 'Remove'], ['⌘K', 'Search']],
    session: [['H', 'Pan'], ['V', 'Select'], ['⌘K', 'Search']],
    connections: [['Backspace', 'Undo last step'], ['Esc', 'Clear selection'], ['⌘K', 'Search']],
  };
</script>

<footer class="statusbar" role="status" aria-live="polite">
  <span class="ws {wsState}"><span class="dot" aria-hidden="true"></span>{LABEL[wsState] ?? 'Offline'}</span>
  {#if selectionCount > 0}
    <span class="tlabel" aria-hidden="true">·</span>
    <span class="sel">{selectionCount} selected</span>
  {/if}
  <span class="hints" aria-label="Keyboard shortcuts">
    {#each HINTS[mode] as [k, text] (k)}<kbd>{k}</kbd>{text}{/each}
  </span>
</footer>

<style>
  .statusbar { display: flex; align-items: center; gap: 8px; padding: 0 20px; font-size: 12px; color: var(--root); background: var(--color-hover); border-top: 1px solid var(--color-border-subtle); overflow: hidden; white-space: nowrap; }
  .ws { display: inline-flex; align-items: center; gap: 6px; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--color-danger-light); }
  .ws.connected .dot { background: var(--color-success-light); }
  .ws.connecting .dot { background: var(--color-warning); }
  .sel { color: var(--color-accent-hover); }
  .hints { margin-left: auto; display: inline-flex; align-items: center; gap: 6px; overflow: hidden; }
  kbd { font: 12px var(--font-mono); color: var(--color-text); border: 1px solid var(--color-border-subtle); border-radius: 4px; padding: 0 4px; margin-left: 8px; }
</style>
