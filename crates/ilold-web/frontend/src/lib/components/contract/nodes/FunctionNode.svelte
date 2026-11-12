<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte';
  import type { FunctionNodeData } from '$lib/stores/graph.svelte';
  import { visibilityLabel, visibilityClass } from '$lib/utils/visibility';
  import { describeTransfers } from '$lib/utils/value';

  let { data }: { data: FunctionNodeData } = $props();

  let visLabel = $derived(visibilityLabel(data.visibility));
  let visClass = $derived(visibilityClass(data.visibility));

  let mutIcon = $derived(
    data.mutability === 'View' || data.mutability === 'Pure' ? '\u{1F441}'
    : data.mutability === 'Payable' ? '\u{1F4B0}'
    : null
  );

  let hasAccessControl = $derived(
    (data.modifiers?.length ?? 0) > 0
  );

  let transfers = $derived(data.value_transfers?.length ? describeTransfers(data.value_transfers) : null);

  let hasBadges = $derived(
    visLabel != null || mutIcon != null || (data.path_count != null && data.path_count > 0) || hasAccessControl || transfers != null
  );

  let scenarioName = $derived(data._scenario ?? null);
  let scenarioActive = $derived(
    data._sessionStep === true &&
    data._activeScenario != null &&
    data._scenariosPassingThrough != null &&
    data._scenariosPassingThrough.includes(data._activeScenario)
  );
  let scenarioMuted = $derived(
    data._sessionStep === true &&
    data._activeScenario != null &&
    data._scenariosPassingThrough != null &&
    !data._scenariosPassingThrough.includes(data._activeScenario)
  );
</script>

<div
  class="function-node py-1.5 px-4 rounded-md bg-surface-alt border-[1.5px] border-accent text-text font-mono text-xs font-semibold min-w-[100px] text-center cursor-pointer"
  class:external={data.is_external}
  class:dimmed={data._dimmed}
  class:scenario-active={scenarioActive}
  class:scenario-muted={scenarioMuted}
>
  <span>{data.label}</span>
  {#if scenarioName}
    <span
      class="inline-block ml-1 text-[8px] px-1 rounded bg-accent-dark/30 text-accent-hover align-middle"
      title={`Scenario: ${scenarioName}`}
    >{scenarioName}</span>
  {/if}
  {#if hasBadges}
    <div class="flex items-center justify-center gap-1 mt-0.5">
      {#if visLabel}
        <span class="text-[8px] px-1 rounded {visClass}">{visLabel}</span>
      {/if}
      {#if mutIcon}
        <span class="text-[9px]">{mutIcon}</span>
      {/if}
      {#if data.path_count != null && data.path_count > 0}
        <span class="text-[8px] text-text-dim">{data.path_count}p</span>
      {/if}
      {#if hasAccessControl}
        <span class="text-[9px]" title={data.modifiers?.join(', ')}>&#x1F512;</span>
      {/if}
      {#if transfers}
        <span class="text-[9px] text-success-light" title={transfers} data-testid="node-value-mark">⇄</span>
      {/if}
    </div>
  {/if}
</div>
<Handle type="target" id="t" position={Position.Top} />
<Handle type="source" id="b" position={Position.Bottom} />
<Handle type="target" id="l" position={Position.Left} />
<Handle type="source" id="r" position={Position.Right} />

<style>
  .function-node.external {
    border-style: dashed;
    border-color: var(--color-danger);
    color: var(--color-danger-light);
    font-size: 11px;
  }
  .function-node.dimmed {
    opacity: 0.55;
  }
  .function-node.scenario-active {
    box-shadow: 0 0 8px var(--color-accent, rgba(88, 166, 255, 0.4));
  }
  .function-node.scenario-muted {
    opacity: 0.7;
  }
</style>
