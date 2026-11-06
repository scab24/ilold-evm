<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte';
  import type { BlockNodeData } from '$lib/stores/graph.svelte';
  import { catchLabel, SWALLOWED } from '$lib/utils/trycatch';

  let { data }: { data: BlockNodeData } = $props();

  let typeClass = $derived(
    data.node_type === 'Entry' ? 'entry'
    : data.node_type === 'Return' ? 'return'
    : data.node_type === 'Revert' ? 'revert'
    : data.node_type === 'LoopCondition' ? 'loop'
    : 'default'
  );

  let isGeneric = $derived(typeClass === 'default');

  let stmtCount = $derived(data.statements?.length ?? 0);
  let hasExternal = $derived(data.statements?.some(s => s.includes('\u2192') || s.includes('call')) ?? false);
  let hasStateWrite = $derived(data.statements?.some(s => /(?<![><!])=(?!=)/.test(s)) ?? false);

  let showBadges = $derived(isGeneric && stmtCount > 0);
</script>

<div
  class="block-node py-1 px-3 rounded-sm bg-hover border border-border font-mono text-[10px] min-w-[80px] text-center"
  class:entry={typeClass === 'entry'}
  class:return={typeClass === 'return'}
  class:revert={typeClass === 'revert'}
  class:loop={typeClass === 'loop'}
  class:dimmed={data._dimmed}
>
  <span>{data.label}</span>
  {#if showBadges}
    <div class="flex items-center justify-center gap-1 mt-0.5">
      <span class="text-[8px] text-text-dim">{stmtCount} stmts</span>
      {#if hasExternal}
        <span class="text-[8px] px-0.5 rounded bg-danger/20 text-danger-light">ext</span>
      {/if}
      {#if hasStateWrite}
        <span class="text-[8px] px-0.5 rounded bg-accent-dark/30 text-accent-hover">write</span>
      {/if}
    </div>
  {/if}
  {#if data.tryCatch}
    <div class="mt-0.5"><span class="text-[8px] px-0.5 rounded bg-warning/20 text-warning" data-testid="try-badge">try {data.tryCatch.guarded_call?.text ?? '?'}</span></div>
  {/if}
  {#if data.catchClause}
    <div class="flex items-center justify-center gap-1 mt-0.5">
      <span class="text-[8px] text-danger-light" data-testid="catch-label">{catchLabel(data.catchClause)}</span>
      {#if data.catchClause.empty}
        <span class="text-[8px] px-0.5 rounded bg-danger/30 text-danger-light font-bold" data-testid="swallowed-badge">{SWALLOWED}</span>
      {/if}
    </div>
  {/if}
</div>
<Handle type="target" id="t" position={Position.Top} />
<Handle type="source" id="b" position={Position.Bottom} />
<Handle type="target" id="l" position={Position.Left} />
<Handle type="source" id="r" position={Position.Right} />

<style>
  .block-node { color: var(--color-text-muted); }
  .block-node.entry {
    background: var(--color-tint-accent);
    border-color: var(--color-accent-dark);
    color: var(--color-accent-hover);
  }
  .block-node.return {
    background: var(--color-tint-success);
    border-color: var(--color-success);
    color: var(--color-success-light);
  }
  .block-node.revert {
    background: var(--color-tint-danger);
    border-color: var(--color-danger);
    color: var(--color-danger-light);
  }
  .block-node.loop {
    background: var(--color-tint-warning);
    border-color: var(--color-warning);
    color: var(--color-warning);
    border-radius: 50%;
    padding: 8px;
  }
  .block-node.dimmed {
    opacity: 0.2;
  }
</style>
