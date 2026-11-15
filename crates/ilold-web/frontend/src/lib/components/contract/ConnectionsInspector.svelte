<script lang="ts">
  import { untrack } from 'svelte';
  import type { Connections, ContractDetail } from '$lib/api/rest';
  import { contractConnections, contractDetail } from '$lib/stores/project-data';
  import FunctionConnections from './FunctionConnections.svelte';
  import ContractSummary from './ContractSummary.svelte';

  let { main, contract, member, titled, onphase, onopen, onsource }: {
    main: string;
    contract: string;
    member: string | null;
    titled: boolean;
    onphase: (contract: string, member: string) => void;
    onopen: (contract: string) => void;
    onsource: (contract: string, func: string) => void;
  } = $props();

  let connections = $state.raw<Connections | null>(null);
  let detail = $state.raw<ContractDetail | null>(null);
  let failed = $state<string | null>(null);
  let attempt = $state(0);

  $effect(() => {
    const name = contract;
    void attempt;
    untrack(() => {
      connections = contractConnections.peek(name) ?? null;
      detail = contractDetail.peek(name) ?? null;
      failed = null;
      const fail = (e: unknown) => { if (name === contract) failed = e instanceof Error ? e.message : String(e); };
      if (!connections) contractConnections(name).then((c) => { if (name === contract) connections = c; }, fail);
      if (!detail) contractDetail(name).then((d) => { if (name === contract) detail = d; }, fail);
    });
  });
</script>

<div class="insp flat" data-testid="connections-inspector">
  {#if member && connections}
    <FunctionConnections {contract} {member} {connections} {detail} {main} {titled} {onopen} {onsource} />
  {/if}
  {#if detail}
    <ContractSummary {detail} {connections} heading={!!member || !titled} {onphase} />
  {:else if failed}
    <div class="empty-state" role="alert">
      <div class="empty-title">Could not load {contract}</div>
      <div class="empty-hint err">{failed}</div>
      <button type="button" class="chip" onclick={() => attempt++}>Retry</button>
    </div>
  {:else if !connections}
    <div class="empty-state"><div class="empty-hint">Loading connections…</div></div>
  {/if}
  {#if titled && !member && !failed}
    <div class="empty-state">
      <div class="empty-icon" aria-hidden="true">◇</div>
      <div class="empty-title">No function selected</div>
      <div class="empty-hint">Pick a function on the left or on the board to list what it calls, who calls it and where in the code.</div>
    </div>
  {/if}
</div>
