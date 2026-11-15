<script lang="ts">
  import Seg from '$lib/components/chrome/Seg.svelte';
  import { contractOf, expanded, reset, touch, type Direction } from './board-state';
  import { applyBoard, currentBoard } from './board-view.svelte';
  import { rowLabel } from './board-links';

  let { main, empty }: { main: string; empty: boolean } = $props();

  const board = $derived(currentBoard(main));
  const DIRECTIONS: { id: Direction; label: string }[] = [
    { id: 'callers', label: 'Callers' },
    { id: 'both', label: 'Both' },
    { id: 'callees', label: 'Callees' },
  ];

  const steps = $derived([...(board.start ? [board.start] : []), ...board.trail.map((t) => t.to ?? `#${t.opened}`)]);
  const shown = $derived(steps.length > 2 ? [steps[0], steps.slice(1, -1), steps.at(-1)!] : steps);

  function owner(step: string): string {
    const c = step.startsWith('#') ? '' : contractOf(step);
    return c === main ? '' : `${c}.`;
  }
  function label(step: string): string {
    return step.startsWith('#') ? step.slice(1) : `${owner(step)}${rowLabel(step, contractOf(step))}`;
  }
  function pick(step: string) {
    const c = step.startsWith('#') ? step.slice(1) : contractOf(step);
    const next = expanded(board).has(c) ? board : touch(board, c);
    applyBoard(step.startsWith('#') ? next : { ...next, fn: step });
  }
</script>

<div class="trail" role="group" aria-label="Opened steps. Backspace removes the last one" title="Backspace removes the last opened step">
  {#if !steps.length && !empty}
    <span class="tlabel hint" title="Click a function in a folded box to open its contract here">Click a function in a folded box to open its contract here</span>
  {:else}
    {#each shown as step, i (i)}
      {#if i}<span class="tlabel" aria-hidden="true">→</span>{/if}
      {#if Array.isArray(step)}
        <button type="button" class="chip" title={step.map(label).join(' → ')} aria-label="{step.length} more steps" onclick={() => pick(step.at(-1)!)}>+{step.length}</button>
      {:else}
        <button type="button" class="chip" class:on={board.fn === step} title="Select {label(step)}" onclick={() => pick(step)}>{#if !step.startsWith('#')}<span><span class="owner">{owner(step)}</span>{rowLabel(step, contractOf(step))}</span>{:else}{label(step)}{/if}</button>
      {/if}
    {/each}
  {/if}
</div>
<button type="button" class="btn" disabled={board.open.length === 1 && !board.fn} title={board.open.length === 1 && !board.fn ? 'Already at the start' : 'Back to the start. Backspace removes only the last step'} onclick={() => applyBoard(reset(board))}>Reset</button>
<Seg label="Direction" options={DIRECTIONS.map((d) => ({ ...d, disabled: empty }))} value={board.dir} onpick={(dir) => applyBoard({ ...board, dir })} />
<button type="button" class="btn" aria-pressed={board.all} disabled={empty} title="Also list functions with no links to other contracts" onclick={() => applyBoard({ ...board, all: !board.all })}>All functions</button>

<style>
  .trail { display: flex; align-items: center; gap: 6px; min-width: 0; overflow: hidden; flex: 1 1 auto; }
  .hint { overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .trail .chip { flex: none; max-width: 260px; overflow: hidden; text-overflow: ellipsis; font-family: var(--font-mono); font-weight: 500; }
  @media (max-width: 1279px) {
    .owner { display: none; }
  }
</style>
