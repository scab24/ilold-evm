<script lang="ts">
  import type { VariableInfo } from '$lib/api/rest';
  import { whoTouches } from '$lib/api/session';
  import { groupByKeyKind } from '$lib/utils/keys';
  import KeyChips from './KeyChips.svelte';

  let { contract, variable, onclose }: { contract: string; variable: string; onclose: () => void } = $props();

  let info = $state<VariableInfo | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    info = null;
    error = null;
    whoTouches(variable, contract).then((v) => (info = v), (e) => (error = e.message));
  });

  const groups = $derived(info ? groupByKeyKind(info.accesses) : []);
</script>

<div class="who" data-testid="who-touches">
  <div class="who-head">
    <span>Who touches <b>{variable}</b></span>
    <button class="who-close" title="Close" onclick={onclose}>×</button>
  </div>
  {#if error}
    <div class="who-error">{error}</div>
  {:else if !info}
    <div class="who-dim">Loading…</div>
  {:else}
    {#each groups as group}
      <div class="who-group" data-testid="who-group">
        <span class="who-label">{group.label}</span>
        <span class="who-scope">{group.scope}</span>
      </div>
      {#each group.rows as row}
        <div class="who-row">
          <span class="who-kind" class:write={row.kind === 'write'}>{row.kind === 'write' ? 'W' : 'R'}</span>
          <span class="who-fn">{row.function}</span>
          <KeyChips keys={row.keys} />
          {#if row.line !== null}<span class="who-dim">line {row.line}</span>{/if}
        </div>
      {/each}
    {/each}
  {/if}
</div>

<style>
  .who { margin-top: 6px; padding: 6px; border: 1px solid var(--color-border-subtle); border-radius: 6px; font-size: 11px; }
  .who-head { display: flex; justify-content: space-between; align-items: center; margin-bottom: 4px; color: var(--color-text-muted); }
  .who-close { background: transparent; border: none; color: var(--color-text-dim); cursor: pointer; font-size: 14px; }
  .who-group { display: flex; gap: 6px; align-items: baseline; margin-top: 6px; padding-top: 4px; border-top: 1px solid var(--color-border-subtle); }
  .who-label { font-size: 10px; font-weight: 600; text-transform: uppercase; color: var(--color-text-muted); }
  .who-scope { font-size: 10px; color: var(--color-accent-hover); }
  .who-row { display: flex; align-items: center; flex-wrap: wrap; gap: 5px; padding: 2px 4px; }
  .who-kind { font-size: 9px; font-weight: 700; width: 14px; text-align: center; color: var(--color-text-muted); }
  .who-kind.write { color: var(--color-accent-hover); }
  .who-fn { font-family: 'JetBrains Mono', monospace; color: var(--color-text); }
  .who-dim { font-size: 10px; color: var(--color-text-dim); }
  .who-error { color: var(--color-danger); }
</style>
