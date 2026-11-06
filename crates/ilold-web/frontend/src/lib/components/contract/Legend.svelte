<script lang="ts">
  import { AMBIGUOUS_COLOR, BASIS_DASH, BASIS_LABEL, LINK_COLOR } from '$lib/utils/links';
  import type { Basis } from '$lib/api/rest';

  let { mode }: { mode: 'cfg' | 'sequences' | 'session' } = $props();
</script>

<div
  class="fixed bottom-3 left-4 flex gap-3 text-[11px] text-text-muted px-3.5 py-2 z-10"
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
