<script lang="ts">
  import type { StorageAccess } from '$lib/api/rest';
  import KeyChips from './KeyChips.svelte';

  let { accesses, onvariable }: { accesses: StorageAccess[]; onvariable?: (variable: string) => void } = $props();

  // a path rooted at a storage parameter names no state variable
  function variableOf(access: StorageAccess): string | null {
    return access.param === null ? access.path.split(/[[.]/)[0] : null;
  }
</script>

{#each accesses as access}
  {@const variable = variableOf(access)}
  <div class="access" data-testid="storage-access">
    <span class="access-kind" class:write={access.kind === 'write'}>{access.kind === 'write' ? 'W' : 'R'}</span>
    {#if variable && onvariable}
      <button class="access-var" title="Who touches {variable}" onclick={() => onvariable(variable)}>{access.path}</button>
    {:else}
      <span class="access-path">{access.path}</span>
    {/if}
    {#if access.modifier}<span class="access-dim">in {access.modifier}</span>{/if}
    <span class="access-keys"><KeyChips keys={access.keys} /></span>
  </div>
{/each}

<style>
  .access { display: flex; align-items: center; flex-wrap: wrap; gap: 5px; padding: 2px 4px; font-size: 11px; }
  .access-kind { font-size: 9px; font-weight: 700; width: 14px; text-align: center; border-radius: 3px; color: var(--color-text-muted); background: color-mix(in srgb, var(--color-text-dim) 15%, transparent); }
  .access-kind.write { color: var(--color-accent-hover); background: color-mix(in srgb, var(--color-accent) 15%, transparent); }
  .access-path, .access-var { font-family: 'JetBrains Mono', monospace; color: var(--color-text); }
  .access-var { background: transparent; border: none; padding: 0; cursor: pointer; text-decoration: underline dotted; }
  .access-var:hover { color: var(--color-accent-hover); }
  .access-dim { font-size: 9px; color: var(--color-text-dim); }
  .access-keys { display: inline-flex; flex-wrap: wrap; }
</style>
