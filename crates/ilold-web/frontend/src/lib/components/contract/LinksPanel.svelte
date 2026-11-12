<script lang="ts">
  import { goto } from '$app/navigation';
  import Collapsible from '$lib/Collapsible.svelte';
  import type { ContractLink } from '$lib/api/rest';
  import { BASIS_DASH, BASIS_LABEL, describeLink, tagColor } from '$lib/utils/links';

  let { name, outgoing, incoming }: { name: string; outgoing: ContractLink[]; incoming: ContractLink[] } = $props();

  function open(contract: string) {
    if (contract !== name) goto(`/contract/${encodeURIComponent(contract)}`);
  }
</script>

<div class="links" data-testid="links-panel">
  {#each [{ title: 'Calls into', links: outgoing, other: (l: ContractLink) => l.to_contract }, { title: 'Called by', links: incoming, other: (l: ContractLink) => l.from_contract }] as side}
    <Collapsible title={side.title} count={side.links.length} open={true}>
      {#if side.links.length === 0}
        <div class="link-empty">none</div>
      {/if}
      {#each side.links as link}
        <button class="link-row" title={describeLink(link)} onclick={() => open(side.other(link))}>
          <svg width="18" height="6" aria-hidden="true"><line x1="0" y1="3" x2="18" y2="3" stroke={tagColor(link.tag)} stroke-width="2" stroke-dasharray={BASIS_DASH[link.basis]} /></svg>
          <span class="link-name">{side.other(link)}</span>
          <span class="link-basis">{BASIS_LABEL[link.basis]}</span>
          {#if link.tag === 'AMBIGUOUS'}<span class="link-ambiguous">ambiguous</span>{/if}
        </button>
      {/each}
    </Collapsible>
  {/each}
</div>

<style>
  .links { border-top: 1px solid var(--color-border-subtle); padding: 4px 6px; max-height: 40%; overflow-y: auto; }
  .link-empty { font-size: 10px; color: var(--color-text-dim); padding: 2px 4px; }
  .link-row {
    display: flex; align-items: center; gap: 6px; width: 100%; padding: 3px 4px; font-size: 11px;
    background: transparent; border: none; color: var(--color-text); cursor: pointer; text-align: left; border-radius: 4px;
  }
  .link-row:hover { background: var(--color-hover); }
  .link-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .link-basis { font-size: 9px; color: var(--color-text-dim); }
  .link-ambiguous { font-size: 9px; color: var(--color-warning); }
</style>
