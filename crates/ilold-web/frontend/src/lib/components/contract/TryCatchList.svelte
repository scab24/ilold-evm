<script lang="ts">
  import type { TryCatchFact } from '$lib/api/rest';
  import { AMBIGUOUS_COLOR } from '$lib/utils/links';
  import CatchClauseView from './CatchClauseView.svelte';

  let { facts }: { facts: TryCatchFact[] } = $props();
</script>

{#each facts as fact}
  <div class="try" data-testid="try-catch">
    <div class="try-head">
      <span class="try-call">try {fact.guarded_call?.text ?? '?'}</span>
      <span class="try-lines">line {fact.span.start_line}</span>
      {#if fact.modifier}<span class="try-lines">in {fact.modifier}</span>{/if}
      {#if fact.tag === 'AMBIGUOUS'}<span class="try-ambiguous" style="color:{AMBIGUOUS_COLOR}" title={fact.reason ?? ''}>ambiguous</span>{/if}
    </div>
    {#each fact.catch_clauses as clause}
      <CatchClauseView {clause} />
    {/each}
  </div>
{/each}

<style>
  .try { padding: 2px 4px 4px; border-left: 2px solid color-mix(in srgb, var(--color-warning) 50%, transparent); margin-bottom: 4px; }
  .try-head { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; font-size: 11px; }
  .try-call { font-family: 'JetBrains Mono', monospace; color: var(--color-warning); }
  .try-lines { font-size: 9px; color: var(--color-text-dim); }
  .try-ambiguous { font-size: 9px; }
</style>
