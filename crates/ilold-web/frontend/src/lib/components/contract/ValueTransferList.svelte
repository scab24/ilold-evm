<script lang="ts">
  import type { BalanceRead, ValueTransfer } from '$lib/api/rest';
  import { AMBIGUOUS_COLOR } from '$lib/utils/links';
  import { assetLabel, describeRead, SELF } from '$lib/utils/value';

  let { transfers = [], reads = [] }: { transfers?: ValueTransfer[]; reads?: BalanceRead[] } = $props();
</script>

{#each transfers as t}
  <div class="value" data-testid="value-transfer">
    <div class="value-head">
      <span class="value-op">{t.operation}</span>
      <span class="value-asset">{assetLabel(t)}</span>
      <span class="value-line">line {t.span.start_line}</span>
      {#if t.modifier}<span class="value-line">in {t.modifier}</span>{/if}
      {#if t.tag === 'AMBIGUOUS'}<span class="value-ambiguous" style="color:{AMBIGUOUS_COLOR}" title={t.reason ?? ''}>ambiguous</span>{/if}
    </div>
    <div class="value-row"><span class="value-label">from</span>{t.from_expr ?? SELF}</div>
    <div class="value-row"><span class="value-label">to</span>{t.to_expr ?? '∅'}</div>
    <div class="value-row"><span class="value-label">amount</span>{t.amount_expr ?? '—'}{#if t.id_expr}<span class="value-line"> id {t.id_expr}</span>{/if}</div>
    {#if t.via_library}<div class="value-row"><span class="value-label">via</span>{t.via_library}</div>{/if}
  </div>
{/each}
{#each reads as r}
  <div class="value read" data-testid="balance-read" title={r.reason ?? ''}>
    <span class="value-line">line {r.span.start_line}</span> {describeRead(r)}
    {#if r.tag === 'AMBIGUOUS'}<span class="value-ambiguous" style="color:{AMBIGUOUS_COLOR}">ambiguous</span>{/if}
  </div>
{/each}

<style>
  .value { padding: 2px 4px 4px; border-left: 2px solid color-mix(in srgb, var(--color-success) 50%, transparent); margin-bottom: 4px; font-size: 11px; }
  .value.read { border-left-style: dotted; }
  .value-head { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
  .value-op { font-family: 'JetBrains Mono', monospace; color: var(--color-success); }
  .value-asset { font-family: 'JetBrains Mono', monospace; }
  .value-row { font-family: 'JetBrains Mono', monospace; padding-left: 6px; }
  .value-label { display: inline-block; width: 48px; font-size: 9px; color: var(--color-text-dim); }
  .value-line { font-size: 9px; color: var(--color-text-dim); }
  .value-ambiguous { font-size: 9px; }
</style>
