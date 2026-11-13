<script lang="ts">
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

  let connections = $state<Connections | null>(null);
  let detail = $state<ContractDetail | null>(null);

  $effect(() => {
    const name = contract;
    connections = null;
    detail = null;
    contractConnections(name).then((c) => { if (name === contract) connections = c; }, () => {});
    contractDetail(name).then((d) => { if (name === contract) detail = d; }, () => {});
  });
</script>

<div class="insp flat" data-testid="connections-inspector">
  {#if member && connections}
    <FunctionConnections {contract} {member} {connections} {detail} {main} {titled} {onopen} {onsource} />
  {/if}
  {#if detail}
    <ContractSummary {detail} {connections} heading={!!member || !titled} {onphase} />
  {:else if !connections}
    <p class="note">Loading connections…</p>
  {/if}
  {#if titled && !member}
    <p class="note">Pick a function on the left or on the board to list what it calls, who calls it and where in the code.</p>
  {/if}
</div>
