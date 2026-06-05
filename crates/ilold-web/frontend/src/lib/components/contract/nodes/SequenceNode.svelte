<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte';
  import type { SequenceNodeData } from '$lib/stores/graph.svelte';
  import { visibilityLabel, visibilityClass } from '$lib/utils/visibility';

  let { data }: { data: SequenceNodeData } = $props();

  let hasConditions = $derived((data._transition?.conditions_affected?.length ?? 0) > 0);
  let hasShared = $derived((data._transition?.shared_state?.length ?? 0) > 0);
  let sharedVars: string[] = $derived(data._transition?.shared_state ?? []);
  let visibleVars = $derived(sharedVars.slice(0, 2));
  let overflowCount = $derived(Math.max(0, sharedVars.length - 2));

  let visLabel = $derived(visibilityLabel(data.visibility));
  let visClass = $derived(visibilityClass(data.visibility));
  let hasAccessControl = $derived((data.modifiers?.length ?? 0) > 0);
</script>

<div
  class="seq-node py-1 px-2.5 rounded-sm bg-surface-alt border-[1.5px] border-accent text-text font-mono text-[11px] min-w-[80px] text-center flex flex-col items-center gap-0.5"
  class:readonly={data.readOnly}
  class:has-conditions={hasConditions}
  class:has-shared={hasShared}
  class:dimmed={data._dimmed}
>
  <div class="flex items-center gap-1.5">
    {#if hasConditions}
      <span class="text-[10px] text-warning" title="Conditions affected">&#x26A0;</span>
    {/if}
    {#if data.readOnly}
      <span class="text-[9px] text-text-muted" title="Read-only">&#x25CE;</span>
    {/if}
    <span>{data.label}</span>
    {#if visLabel}
      <span class="text-[8px] px-1 rounded {visClass}">{visLabel}</span>
    {/if}
    {#if hasAccessControl}
      <span class="text-[9px]" title={data.modifiers?.join(', ')}>&#x1F512;</span>
    {/if}
    {#if data.pathCount}
      <span class="text-[9px] text-text-dim">{data.pathCount}p</span>
    {/if}
    <span class="text-[8px] text-text-dim ml-auto">&#x25B6;</span>
  </div>
  {#if hasShared && visibleVars.length > 0}
    <div class="flex items-center gap-0.5 flex-wrap justify-center">
      {#each visibleVars as v}
        <span class="text-[8px] bg-accent-dark/30 text-accent-hover px-1 rounded">{v}</span>
      {/each}
      {#if overflowCount > 0}
        <span class="text-[8px] text-text-dim">+{overflowCount}</span>
      {/if}
    </div>
  {/if}
</div>
<Handle type="target" id="t" position={Position.Top} />
<Handle type="source" id="b" position={Position.Bottom} />
<Handle type="target" id="l" position={Position.Left} />
<Handle type="source" id="r" position={Position.Right} />

<style>
  .seq-node.readonly {
    border-color: var(--color-text-muted);
    color: var(--color-text-muted);
  }
  .seq-node.has-conditions {
    background: var(--color-tint-warning);
    border-color: var(--color-warning);
    border-width: 2px;
  }
  .seq-node.has-shared {
    border-style: dashed;
  }
  .seq-node.dimmed {
    opacity: 0.25;
    pointer-events: none;
  }
</style>
