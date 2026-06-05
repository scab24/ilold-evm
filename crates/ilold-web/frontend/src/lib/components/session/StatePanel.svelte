<script lang="ts">
  import { postCommand } from '$lib/api/session';
  import { getSteps } from '$lib/stores/session.svelte';

  interface Props {
    contract: string;
  }

  let { contract }: Props = $props();

  let stateVars = $state<any[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  let expandedVar = $state<string | null>(null);

  async function fetchState() {
    loading = true;
    error = null;
    try {
      const result: any = await postCommand('State', contract);
      const view = result.StateView ?? result;
      stateVars = view.summary ?? [];
    } catch (e: any) {
      error = e.message ?? 'Failed to fetch state';
      stateVars = [];
    } finally {
      loading = false;
    }
  }

  let prevStepCount = -1;

  $effect(() => {
    const count = getSteps().length;
    if (count !== prevStepCount) {
      prevStepCount = count;
      fetchState();
    }
  });

  function toggleTimeline(varName: string) {
    expandedVar = expandedVar === varName ? null : varName;
  }
</script>

<div class="flex flex-col overflow-hidden h-full" style="background: transparent;">
  <!-- Header — glass effect -->
  <div
    class="flex items-center justify-between px-3 py-2.5 flex-shrink-0"
    style="
      border-bottom: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
      background: linear-gradient(180deg, rgba(30, 30, 40, 0.85) 0%, rgba(24, 24, 30, 0.9) 100%);
      backdrop-filter: blur(16px) saturate(1.8);
      -webkit-backdrop-filter: blur(16px) saturate(1.8);
    "
  >
    <span class="text-[10px] text-text-muted uppercase tracking-wider font-semibold">State Variables</span>
    <button
      class="bg-transparent border text-text-muted cursor-pointer text-xs px-2 py-0.5 leading-none transition-colors duration-150 hover:text-accent-hover hover:border-accent disabled:opacity-40 disabled:cursor-default"
      style="
        border-radius: 6px;
        border-color: color-mix(in srgb, var(--color-border) 50%, transparent);
      "
      onclick={fetchState}
      disabled={loading}
      title="Refresh state"
    >
      {loading ? '...' : '↻'}
    </button>
  </div>

  <!-- Body -->
  <div class="panel-body flex-1 overflow-y-auto p-1.5">
    {#if loading && stateVars.length === 0}
      <div class="text-[11px] text-text-dim py-6 px-2 text-center italic">Loading state...</div>
    {:else if error && stateVars.length === 0}
      <div class="text-[11px] text-danger py-4 px-2 text-center">{error}</div>
    {:else if stateVars.length === 0}
      <div class="text-[11px] text-text-dim py-8 px-3 text-center italic leading-relaxed">
        No state changes yet. Explore functions to see variable mutations.
      </div>
    {:else}
      {#each stateVars as v}
        <button
          class="flex items-center gap-1.5 w-full px-2.5 py-2 bg-transparent border-none text-text text-[11px] font-mono cursor-pointer text-left transition-all duration-150 hover:text-accent-hover"
          style="
            border-radius: 8px;
            border-left: 2px solid {expandedVar === v.variable ? 'var(--color-accent)' : 'transparent'};
            background: {expandedVar === v.variable ? 'color-mix(in srgb, var(--color-accent) 5%, transparent)' : 'transparent'};
          "
          onclick={() => toggleTimeline(v.variable)}
        >
          <span class="text-accent-hover font-semibold flex-shrink-0 max-w-[120px] overflow-hidden text-ellipsis whitespace-nowrap">{v.variable}</span>
          <span class="flex-1 text-[10px] text-text-muted overflow-hidden text-ellipsis whitespace-nowrap">
            {#if v.changes?.length > 0}
              {v.changes[v.changes.length - 1]}
            {/if}
          </span>
          <span class="text-[9px] text-text-dim flex-shrink-0 w-2.5 text-center">{expandedVar === v.variable ? '▾' : '▸'}</span>
        </button>

        {#if expandedVar === v.variable}
          <div
            class="py-1.5 px-2.5 pb-2 pl-5 ml-2.5 mb-1"
            style="
              border-left: 2px solid color-mix(in srgb, var(--color-border) 40%, transparent);
              border-radius: 0 0 6px 0;
            "
          >
            {#if v.changes?.length > 0}
              {#each v.changes as change, i}
                <div
                  class="font-mono text-[10px] text-text-muted py-1"
                  style="border-bottom: {i < v.changes.length - 1 ? '1px solid color-mix(in srgb, var(--color-border) 20%, transparent)' : 'none'};"
                >{change}</div>
              {/each}
            {:else}
              <div class="text-[10px] text-text-dim py-1.5 italic">No changes recorded.</div>
            {/if}
          </div>
        {/if}
      {/each}
    {/if}
  </div>
</div>

<style>
  .panel-body::-webkit-scrollbar { width: 4px; }
  .panel-body::-webkit-scrollbar-track { background: transparent; }
  .panel-body::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: 2px; }
</style>
