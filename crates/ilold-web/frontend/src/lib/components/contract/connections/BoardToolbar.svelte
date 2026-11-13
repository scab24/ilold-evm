<script lang="ts">
  import Seg from '$lib/components/chrome/Seg.svelte';
  import { contractOf, expanded, reset, touch, type Direction } from './board-state';
  import { applyBoard, currentBoard, pan } from './board-view.svelte';
  import { rowLabel } from './board-links';

  let { main }: { main: string } = $props();

  const board = $derived(currentBoard(main));
  const DIRECTIONS: { id: Direction; label: string }[] = [
    { id: 'callers', label: 'Callers' },
    { id: 'both', label: 'Both' },
    { id: 'callees', label: 'Callees' },
  ];

  const steps = $derived([...(board.start ? [board.start] : []), ...board.trail.map((t) => t.to ?? `#${t.opened}`)]);
  const shown = $derived(steps.length > 2 ? [steps[0], steps.slice(1, -1), steps.at(-1)!] : steps);

  function label(step: string): string {
    if (step.startsWith('#')) return step.slice(1);
    const c = contractOf(step);
    return c === main ? rowLabel(step, c) : `${c}.${rowLabel(step, c)}`;
  }
  function pick(step: string) {
    const c = step.startsWith('#') ? step.slice(1) : contractOf(step);
    const next = expanded(board).has(c) ? board : touch(board, c);
    applyBoard(step.startsWith('#') ? next : { ...next, fn: step });
  }
</script>

<div class="trail" role="group" aria-label="Opened steps. Backspace removes the last one" title="Backspace removes the last opened step">
  {#if !steps.length}
    <span class="tlabel">Click a function in a folded box to open its contract here</span>
  {:else}
    {#each shown as step, i (i)}
      {#if i}<span class="tlabel" aria-hidden="true">→</span>{/if}
      {#if Array.isArray(step)}
        <button type="button" class="chip" title={step.map(label).join(' → ')} aria-label="{step.length} more steps" onclick={() => pick(step.at(-1)!)}>+{step.length}</button>
      {:else}
        <button type="button" class="chip" class:on={board.fn === step} title="Select {label(step)}" onclick={() => pick(step)}>{label(step)}</button>
      {/if}
    {/each}
  {/if}
</div>
{#if pan.left}<button type="button" class="btn" title="{pan.left} more boxes on the left" aria-label="Scroll left: {pan.left} more boxes" onclick={() => pan.by?.(-1)}>← {pan.left}</button>{/if}
{#if pan.right}<button type="button" class="btn" title="{pan.right} more boxes on the right" aria-label="Scroll right: {pan.right} more boxes" onclick={() => pan.by?.(1)}>{pan.right} →</button>{/if}
<button type="button" class="btn" disabled={board.open.length === 1 && !board.fn} title={board.open.length === 1 && !board.fn ? 'Already at the start' : 'Back to the start. Backspace removes only the last step'} onclick={() => applyBoard(reset(board))}>Reset</button>
<Seg label="Direction" options={DIRECTIONS} value={board.dir} onpick={(dir) => applyBoard({ ...board, dir })} />
<button type="button" class="btn" aria-pressed={board.all} title="Also list functions with no links to other contracts" onclick={() => applyBoard({ ...board, all: !board.all })}>All functions</button>

<style>
  .trail { display: flex; align-items: center; gap: 6px; min-width: 0; overflow: hidden; flex: 1 1 auto; }
  .trail .chip { max-width: 220px; overflow: hidden; text-overflow: ellipsis; font-family: var(--font-mono); font-weight: 500; }
</style>
