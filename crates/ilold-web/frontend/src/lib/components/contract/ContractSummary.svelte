<script lang="ts">
  import type { Connections, ContractDetail } from '$lib/api/rest';
  import Icon from '$lib/components/chrome/Icon.svelte';
  import { splitRef } from '$lib/utils/links';
  import { describeFlow } from '$lib/utils/value';

  let { detail, connections, heading, onphase }: {
    detail: ContractDetail;
    connections: Connections | null;
    heading: boolean;
    onphase: (contract: string, member: string) => void;
  } = $props();

  const member = (ref: string) => splitRef(ref).member;
</script>

{#snippet functions(label: string, refs: string[])}
  <span class="tlabel">{label}</span>
  {#each refs as ref (ref)}
    <button type="button" class="chip mono" title={ref} onclick={() => onphase(detail.name, member(ref))}>{member(ref)}</button>
  {/each}
{/snippet}

{#if heading}
  <h2><Icon name="folder" /><span class="ht">{detail.name}</span></h2>
{:else}
  <h1>{detail.name}</h1>
{/if}
<p class="sub">{detail.kind.toLowerCase()} · {detail.functions.length + (detail.inherited_functions?.length ?? 0)} functions</p>
{#if connections}
  <div class="stat"><Icon name="call" /><span class="k">Calls and reads to other contracts</span><span class="v">{connections.outgoing_calls}</span></div>
  <div class="stat"><Icon name="entry" /><span class="k">Calls and reads from other contracts</span><span class="v">{connections.incoming_calls}</span></div>
{/if}

{#if detail.phase_candidates.length}
  <h2 data-testid="inspector-phases"><Icon name="lock" /><span class="ht">Phases</span><span class="n">{detail.phase_candidates.length}</span></h2>
  {#each detail.phase_candidates as phase (phase.state_var)}
    <div class="phase" title={phase.condition_texts.join('\n')}>
      <div><span class="mono phase-var">{member(phase.state_var)}</span> <span class="tlabel">{phase.type_name}</span></div>
      <div class="phase-fns">{@render functions('gates', phase.guarded_functions)}</div>
      {#if phase.writers.length}<div class="phase-fns">{@render functions('set by', phase.writers)}</div>{/if}
    </div>
  {/each}
{/if}

{#if detail.value_flows.length}
  <h2 data-testid="inspector-value-flows"><Icon name="value" /><span class="ht">Value flows</span><span class="n">{detail.value_flows.length}</span></h2>
  <ul class="list">
    {#each detail.value_flows as flow, i (i)}
      <li class="li" title={flow.transfers.join('\n')}>
        <span class="rw"><Icon name="value" class="val" /><span class="rb-body"><span class="rt">{describeFlow(flow)}</span><span class="rs">{flow.functions.map(member).join(', ')}</span></span></span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .phase { padding: 6px; }
  .phase-var { color: var(--color-warning); }
  .phase-fns { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin-top: 4px; }
</style>
