<script lang="ts">
  import { splitRef } from '$lib/utils/links';
  import type { PhaseCandidate } from '$lib/api/rest';

  let { phases = [], onfunction }: {
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

{#if phases.length}
<div
  class="fixed bottom-20 left-72 flex flex-col gap-1 text-[11px] text-text-muted px-3.5 py-2 z-10"
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
{/if}

<style>
  .phase-var { color: var(--color-warning); font-family: 'JetBrains Mono', monospace; }
  .phase-fn { font-family: 'JetBrains Mono', monospace; color: var(--color-accent); cursor: pointer; }
  .phase-fn:hover { text-decoration: underline; }
</style>
