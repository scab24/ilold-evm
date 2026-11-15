<script lang="ts">
  import type { Mode } from '$lib/components/chrome/AppHeader.svelte';
  import LegendBar from '$lib/components/chrome/LegendBar.svelte';
  import Icon, { type IconName } from '$lib/components/chrome/Icon.svelte';
  import { LINES, nextState, type Line } from '$lib/components/project/map-graph';
  import { lineStyle } from '$lib/utils/links';
  import { applyLines, currentLines } from './connections/board-view.svelte';

  let { mode }: { mode: Mode } = $props();

  const BOARD_LINES: Line[] = ['calls', 'reads', 'value', 'library', 'iface', 'unresolved'];
  const lines = $derived(currentLines());
  const pills = $derived(
    LINES.filter((l) => BOARD_LINES.includes(l.id)).map((l) => ({ ...l, state: lines[l.id], ...(l.id === 'iface' ? { icon: 'iface' as const } : { line: lineStyle(l.id) }) })),
  );
  const CARD_MARKS: { icon: IconName; text: string }[] = [
    { icon: 'entry', text: 'entry point' },
    { icon: 'lock', text: 'restricted' },
    { icon: 'value', text: 'moves value' },
    { icon: 'unres', text: 'outside the project' },
  ];
  const CFG_KEYS = [
    { color: 'var(--color-accent)', text: 'Function' },
    { color: 'var(--color-accent-dark)', text: 'Entry block' },
    { color: 'var(--color-success)', text: 'Return' },
    { color: 'var(--color-danger)', text: 'Revert' },
  ];
  const SESSION_LINES = [
    { color: 'var(--color-accent-hover)', dash: '', text: 'Active scenario' },
    { color: 'var(--color-accent)', dash: '4 4', text: 'Fork' },
    { color: 'var(--color-text-dim)', dash: '', text: 'Other scenario' },
  ];
  const SEQ_KEYS = [
    { color: 'var(--color-accent)', text: 'State-changing' },
    { color: '', text: 'Read-only' },
    { color: 'var(--color-warning)', text: 'Conditions affected' },
  ];
</script>

{#if mode === 'connections'}
  <LegendBar label="Lines" wide foundBy {pills} onpill={(id) => applyLines({ ...lines, [id]: nextState(id as Line, lines[id as Line]) })} hint="Click a function to light its lines · Backspace undoes the last step">
    {#each CARD_MARKS as k (k.text)}
      <span class="key mark" title={k.text}><Icon name={k.icon} size={13} /><span class="mark-text">{k.text}</span></span>
    {/each}
    <span class="key mark-count" title="Calls out of the function and calls into it, counted per call site"><span class="mono">2→ 1←</span>calls out, in</span>
  </LegendBar>
{:else if mode === 'session'}
  <LegendBar label="Key" foundBy hint="Each step is one call, left to right · Back removes the last step">
    <span class="key"><span class="dot" style:background="var(--color-accent)"></span>Step</span>
    {#each SESSION_LINES as k (k.text)}
      <span class="key"><svg width="18" height="6" aria-hidden="true"><line x1="1" y1="3" x2="17" y2="3" stroke={k.color} stroke-width="2" stroke-dasharray={k.dash} /></svg>{k.text}</span>
    {/each}
  </LegendBar>
{:else}
  <LegendBar label="Key" foundBy hint={mode === 'cfg' ? 'Click a function to expand its CFG' : 'Click to expand · Shift+click adds a branch'}>
    {#each mode === 'cfg' ? CFG_KEYS : SEQ_KEYS as k (k.text)}
      <span class="key"><span class="dot" style:background={k.color || 'transparent'} class:hollow={!k.color}></span>{k.text}</span>
    {/each}
  </LegendBar>
{/if}

<style>
  .dot { display: inline-block; width: 10px; height: 10px; border-radius: 5px; }
  .dot.hollow { border: 1px solid var(--root); }
  .mark { gap: 4px; }
  @media (max-width: 1599px) {
    .mark-text, .mark-count { display: none; }
  }
</style>
