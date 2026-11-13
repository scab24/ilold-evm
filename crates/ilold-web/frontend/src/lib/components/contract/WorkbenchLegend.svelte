<script lang="ts">
  import type { Mode } from '$lib/components/chrome/AppHeader.svelte';
  import LegendBar from '$lib/components/chrome/LegendBar.svelte';
  import { LINES, nextState, type Line } from '$lib/components/project/map-graph';
  import { AMBIGUOUS_COLOR, lineStyle } from '$lib/utils/links';
  import { applyLines, currentLines } from './connections/board-view.svelte';

  let { mode }: { mode: Mode } = $props();

  const BOARD_LINES: Line[] = ['calls', 'reads', 'value', 'library', 'iface', 'unresolved'];
  const lines = $derived(currentLines());
  const pills = $derived(
    LINES.filter((l) => BOARD_LINES.includes(l.id)).map((l) => ({ ...l, state: lines[l.id], ...(l.id === 'iface' ? { icon: 'iface' as const } : { line: lineStyle(l.id) }) })),
  );
  const CFG_KEYS = [
    { color: 'var(--color-accent)', text: 'Function' },
    { color: 'var(--color-accent-dark)', text: 'Entry block' },
    { color: 'var(--color-success)', text: 'Return' },
    { color: 'var(--color-danger)', text: 'Revert' },
  ];
  const SEQ_KEYS = [
    { color: 'var(--color-accent)', text: 'State-changing' },
    { color: '', text: 'Read-only' },
    { color: 'var(--color-warning)', text: 'Conditions affected' },
  ];
</script>

{#if mode === 'connections'}
  <LegendBar label="Lines" wide foundBy {pills} onpill={(id) => applyLines({ ...lines, [id]: nextState(id as Line, lines[id as Line]) })} hint="Click a function to light its lines · Backspace undoes the last step" />
{:else}
  <LegendBar label="Key" foundBy hint={mode === 'cfg' ? 'Click a function to expand its CFG' : 'Click to expand · Shift+click adds a branch'}>
    {#each mode === 'cfg' ? CFG_KEYS : SEQ_KEYS as k (k.text)}
      <span class="key"><span class="dot" style:background={k.color || 'transparent'} class:hollow={!k.color}></span>{k.text}</span>
    {/each}
    <span class="key amb"><svg width="18" height="6" aria-hidden="true"><line x1="1" y1="3" x2="17" y2="3" stroke={AMBIGUOUS_COLOR} stroke-width="3" /></svg>ambiguous link</span>
  </LegendBar>
{/if}

<style>
  .dot { display: inline-block; width: 10px; height: 10px; border-radius: 5px; }
  .dot.hollow { border: 1px solid var(--root); }
</style>
