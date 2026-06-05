<script lang="ts">
  import { onMount } from 'svelte';
  import { subscribe, getConnectionState } from '$lib/api/ws';
  import type { ConnectionEvent, ConnectionState } from '$lib/api/types';
  import { getSteps } from '$lib/stores/session.svelte';

  let {
    mode,
    canvasCount,
    expandedCount,
    activeScenario,
    selectionCount,
  }: {
    mode: 'cfg' | 'sequences' | 'session';
    canvasCount: number;
    expandedCount: number;
    activeScenario: string;
    selectionCount: number;
  } = $props();

  let wsState = $state<ConnectionState>(getConnectionState());

  onMount(() => {
    const unsub = subscribe('connection', (event: ConnectionEvent) => {
      wsState = event.state;
    });
    return unsub;
  });

  const sessionStepsCount = $derived(mode === 'session' ? getSteps().length : 0);

  const wsDot = $derived(
    wsState === 'connected' ? '●'
    : wsState === 'connecting' ? '◐'
    : '○',
  );
  const wsLabel = $derived(
    wsState === 'connected' ? 'Connected'
    : wsState === 'connecting' ? 'Connecting…'
    : 'Offline',
  );
  const wsClass = $derived(
    wsState === 'connected' ? 'ws-ok'
    : wsState === 'connecting' ? 'ws-pending'
    : 'ws-down',
  );

  const showCanvasChips = $derived(mode !== 'session' && canvasCount > 0);
  const showScenarioChip = $derived(mode === 'session' && activeScenario.length > 0);
  const showSelectionChip = $derived(selectionCount > 0);
</script>

<!-- aria-live=polite: screen readers announce WS transitions without
     interrupting. role=status: it's an ambient status region, not a log. -->
<footer class="statusbar" role="status" aria-live="polite">
  <span class="ws {wsClass}" aria-label="WebSocket {wsLabel.toLowerCase()}">
    <span class="ws-dot" aria-hidden="true">{wsDot}</span>
    <span class="ws-label">{wsLabel}</span>
  </span>

  {#if showCanvasChips}
    <span class="sep" aria-hidden="true">·</span>
    <span class="chip" title="Functions currently rendered on the canvas">
      <span aria-hidden="true">⊙</span> {canvasCount} on canvas
    </span>
    {#if expandedCount > 0}
      <span class="chip dim" title="Functions with CFG expanded">
        · {expandedCount} expanded
      </span>
    {/if}
  {/if}

  {#if showScenarioChip}
    <span class="sep" aria-hidden="true">·</span>
    <span class="chip" title="Active scenario and step count">
      <span aria-hidden="true">⎇</span> {activeScenario} · {sessionStepsCount} steps
    </span>
  {/if}

  {#if showSelectionChip}
    <span class="sep" aria-hidden="true">·</span>
    <span class="chip accent" title="Nodes currently selected on the canvas">
      <span aria-hidden="true">⧉</span> {selectionCount} selected
    </span>
  {/if}

  <span class="hints" aria-label="Keyboard shortcuts">
    {#if mode === 'session'}
      <kbd>↶</kbd><span class="hint-label">Back</span>
      <span class="hint-sep">·</span>
      <kbd>🗑</kbd><span class="hint-label">Clear</span>
      <span class="hint-sep">·</span>
      <kbd>⌘K</kbd><span class="hint-label">Search</span>
    {:else}
      <kbd>H</kbd><span class="hint-label">Pan</span>
      <span class="hint-sep">·</span>
      <kbd>V</kbd><span class="hint-label">Select</span>
      <span class="hint-sep">·</span>
      <kbd>DEL</kbd><span class="hint-label">Remove</span>
      <span class="hint-sep">·</span>
      <kbd>⌘K</kbd><span class="hint-label">Search</span>
    {/if}
  </span>
</footer>

<style>
  .statusbar {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    height: 24px;
    padding: 0 10px;
    font-family: var(--font-mono, monospace);
    font-size: 10px;
    color: var(--color-text-muted);
    background: linear-gradient(180deg, rgba(22, 22, 28, 0.92) 0%, rgba(16, 16, 22, 0.95) 100%);
    border-top: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
    backdrop-filter: blur(16px) saturate(1.8);
    -webkit-backdrop-filter: blur(16px) saturate(1.8);
    overflow: hidden;
    white-space: nowrap;
    z-index: 15;
  }

  .sep {
    color: var(--color-text-dim);
    user-select: none;
  }

  .ws {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .ws-dot { line-height: 1; }
  .ws-ok { color: var(--color-success); }
  .ws-pending { color: var(--color-warning); }
  .ws-down { color: var(--color-danger); }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--color-text-muted);
  }
  .chip.dim { color: var(--color-text-dim); }
  .chip.accent { color: var(--color-accent-hover); }

  .hints {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--color-text-dim);
  }
  .hints kbd {
    font-family: var(--font-mono, monospace);
    font-size: 9px;
    color: var(--color-text-muted);
    background: color-mix(in srgb, var(--color-border) 35%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
    border-radius: 3px;
    padding: 0 4px;
    line-height: 1.4;
    letter-spacing: 0.2px;
  }
  .hint-label { padding-left: 1px; }
  .hint-sep { color: var(--color-border); margin: 0 2px; }
</style>
