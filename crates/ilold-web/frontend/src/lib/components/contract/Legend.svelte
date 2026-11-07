<script lang="ts">
  import { AMBIGUOUS_COLOR, BASIS_DASH, BASIS_LABEL, LINK_COLOR, splitRef } from '$lib/utils/links';
  import type { Basis, PhaseCandidate } from '$lib/api/rest';

  let { mode, phases = [], onfunction }: {
    mode: 'cfg' | 'sequences' | 'session';
    phases?: PhaseCandidate[];
    onfunction?: (ref: string) => void;
  } = $props();
</script>

{#snippet functions(label: string, refs: string[])}
  <span class="text-text-dim">{label}</span>
  {#each refs as ref}
    <button class="phase-fn" title={ref} onclick={() => onfunction?.(ref)}>{splitRef(ref).member}</button>
  {/each}
{/snippet}

<div
  class="fixed bottom-3 left-4 flex flex-col gap-1 text-[11px] text-text-muted px-3.5 py-2 z-10"
  style="
    border-radius: 10px;
    border: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
    background: linear-gradient(180deg, rgba(30, 30, 40, 0.85) 0%, rgba(24, 24, 30, 0.9) 100%);
    backdrop-filter: blur(16px) saturate(1.8);
    -webkit-backdrop-filter: blur(16px) saturate(1.8);
    box-shadow:
      0 4px 16px -4px rgba(0, 0, 0, 0.3),
      0 0 0 1px rgba(91, 155, 213, 0.04);
  "
>
  <div class="flex gap-3">
    {#if mode === 'cfg'}
      <span><span class="inline-block w-2 h-2 align-middle mr-1" style="border-radius: 4px; background: var(--color-accent); box-shadow: 0 0 6px color-mix(in srgb, var(--color-accent) 40%, transparent);"></span>Function</span>
      <span><span class="inline-block w-2 h-2 align-middle mr-1" style="border-radius: 4px; background: var(--color-accent-dark); box-shadow: 0 0 6px color-mix(in srgb, var(--color-accent-dark) 40%, transparent);"></span>Entry block</span>
      <span><span class="inline-block w-2 h-2 align-middle mr-1" style="border-radius: 4px; background: var(--color-success); box-shadow: 0 0 6px color-mix(in srgb, var(--color-success) 40%, transparent);"></span>Return</span>
      <span><span class="inline-block w-2 h-2 align-middle mr-1" style="border-radius: 4px; background: var(--color-danger); box-shadow: 0 0 6px color-mix(in srgb, var(--color-danger) 40%, transparent);"></span>Revert</span>
      <span class="text-text-dim">Click → expand CFG</span>
    {:else}
      <span><span class="inline-block w-2 h-2 align-middle mr-1" style="border-radius: 4px; background: var(--color-accent); box-shadow: 0 0 6px color-mix(in srgb, var(--color-accent) 40%, transparent);"></span>State-changing</span>
      <span><span class="inline-block w-2 h-2 align-middle mr-1" style="border-radius: 4px; border: 1px solid var(--color-text-muted); background: transparent;"></span>Read-only</span>
      <span><span class="inline-block w-2 h-2 align-middle mr-1" style="border-radius: 4px; background: var(--color-warning); box-shadow: 0 0 6px color-mix(in srgb, var(--color-warning) 40%, transparent);"></span>Conditions affected</span>
      <span class="text-text-dim">Click → expand · Shift+click → add branch</span>
    {/if}
    <span class="text-text-dim" data-testid="legend-links">Links:</span>
    {#each Object.entries(BASIS_DASH) as [basis, dash]}
      <span><svg class="inline-block align-middle mr-1" width="16" height="6" aria-hidden="true"><line x1="0" y1="3" x2="16" y2="3" stroke={LINK_COLOR} stroke-width="2" stroke-dasharray={dash} /></svg>{BASIS_LABEL[basis as Basis]}</span>
    {/each}
    <span><svg class="inline-block align-middle mr-1" width="16" height="6" aria-hidden="true"><line x1="0" y1="3" x2="16" y2="3" stroke={AMBIGUOUS_COLOR} stroke-width="3" /></svg>ambiguous</span>
  </div>
  {#if phases.length}
    <div class="flex flex-col gap-0.5 max-h-40 overflow-y-auto" data-testid="legend-phases">
      <span class="text-text-dim">Phases:</span>
      {#each phases as phase}
        <div class="flex flex-wrap items-center gap-1.5" title={phase.condition_texts.join('\n')}>
          <span class="phase-var">{splitRef(phase.state_var).member}</span>
          <span class="text-text-dim">{phase.type_name}</span>
          {@render functions('gates', phase.guarded_functions)}
          {@render functions('set by', phase.writers)}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .phase-var { color: var(--color-warning); font-family: 'JetBrains Mono', monospace; }
  .phase-fn { font-family: 'JetBrains Mono', monospace; color: var(--color-accent); cursor: pointer; }
  .phase-fn:hover { text-decoration: underline; }
</style>
