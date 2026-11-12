<script lang="ts">
  import type { CatchClauseFact, SourceSpan } from '$lib/api/rest';
  import { catchLabel, SWALLOWED } from '$lib/utils/trycatch';

  let { clause }: { clause: CatchClauseFact } = $props();

  const lines = (s: SourceSpan) => (s.start_line === s.end_line ? `${s.start_line}` : `${s.start_line}–${s.end_line}`);
  const effects = $derived([
    ...clause.effects.writes.map((e) => ({ kind: 'W', ...e })),
    ...clause.effects.calls.map((e) => ({ kind: 'call', ...e })),
    ...clause.effects.emits.map((e) => ({ kind: 'emit', ...e })),
  ]);
</script>

<div class="clause" data-testid="catch-clause">
  <div class="clause-head">
    <span class="clause-kind">{catchLabel(clause)}</span>
    <span class="clause-lines">line {lines(clause.span)}</span>
    {#if clause.empty}<span class="swallowed">{SWALLOWED}</span>{/if}
    {#if clause.effects.reverts}<span class="reverts">reverts</span>{/if}
  </div>
  {#each effects as effect}
    <div class="effect" data-testid="catch-effect">
      <span class="effect-kind">{effect.kind}</span>
      <span class="effect-text">{effect.text}</span>
      <span class="clause-lines">{effect.span.start_line}</span>
    </div>
  {/each}
  {#if !clause.empty && effects.length === 0 && !clause.effects.reverts}
    <div class="effect clause-lines">no writes, calls or events</div>
  {/if}
</div>

<style>
  .clause { padding: 3px 4px; font-size: 11px; }
  .clause-head { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
  .clause-kind { font-family: 'JetBrains Mono', monospace; color: var(--color-danger-light); }
  .clause-lines { font-size: 9px; color: var(--color-text-dim); }
  .swallowed, .reverts { font-size: 9px; font-weight: 700; padding: 0 4px; border-radius: 3px; }
  .swallowed { color: var(--color-danger-light); background: color-mix(in srgb, var(--color-danger) 25%, transparent); }
  .reverts { color: var(--color-text-muted); background: color-mix(in srgb, var(--color-text-dim) 15%, transparent); }
  .effect { display: flex; align-items: center; gap: 5px; padding: 1px 0 1px 10px; }
  .effect-kind { font-size: 9px; font-weight: 700; min-width: 26px; color: var(--color-text-muted); }
  .effect-text { font-family: 'JetBrains Mono', monospace; color: var(--color-text); }
</style>
