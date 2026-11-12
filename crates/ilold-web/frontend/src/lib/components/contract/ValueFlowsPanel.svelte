<script lang="ts">
  import Collapsible from '$lib/Collapsible.svelte';
  import type { ValueFlow } from '$lib/api/rest';
  import { describeFlow } from '$lib/utils/value';

  let { flows }: { flows: ValueFlow[] } = $props();

  const label = (f: string) => f.slice(f.lastIndexOf('::') + 2);
</script>

<div class="flows" data-testid="value-flows-panel">
  <Collapsible title="Value flows" count={flows.length} open={true}>
    {#if flows.length === 0}
      <div class="flow-empty">none</div>
    {/if}
    {#each flows as flow}
      <div class="flow-row {flow.direction}" title={flow.transfers.join('\n')}>
        <span class="flow-text">{describeFlow(flow)}</span>
        <span class="flow-functions">{flow.functions.map(label).join(', ')}</span>
      </div>
    {/each}
  </Collapsible>
</div>

<style>
  .flows { border-top: 1px solid var(--color-border-subtle); padding: 4px 6px; max-height: 30%; overflow-y: auto; }
  .flow-empty { font-size: 10px; color: var(--color-text-dim); padding: 2px 4px; }
  .flow-row { display: flex; flex-direction: column; padding: 3px 4px; font-size: 11px; border-radius: 4px; }
  .flow-row:hover { background: var(--color-hover); }
  .flow-row.in .flow-text { color: var(--color-success); }
  .flow-row.out .flow-text { color: var(--color-warning); }
  .flow-text { font-family: 'JetBrains Mono', monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .flow-functions { font-size: 9px; color: var(--color-text-dim); }
</style>
