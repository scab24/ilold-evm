<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import type { ProjectSummary } from '$lib/api/rest';
  import { projectSummary } from '$lib/stores/project-data';
  import { RELATION_STYLE, UNRESOLVED_COLOR } from '$lib/utils/links';
  import { setSearchContext } from '$lib/stores/search.svelte';
  import { setPaletteCommands, clearPaletteCommands } from '$lib/stores/palette.svelte';
  import type { Command } from '$lib/commands/registry';
  import ErrorToasts from '$lib/components/ui/ErrorToasts.svelte';
  import MapTab from '$lib/components/project/MapTab.svelte';
  import ValueFlowTab from '$lib/components/project/ValueFlowTab.svelte';
  import RouteTab from '$lib/components/project/RouteTab.svelte';
  import AppHeader, { TABS, type ProjectTab } from '$lib/components/chrome/AppHeader.svelte';
  import '$lib/components/chrome/chrome.css';

  let project = $state<ProjectSummary | null>(null);
  let failed = $state(false);

  const tab = $derived<ProjectTab>(TABS.find((t) => t.id === page.url.searchParams.get('tab'))?.id ?? 'map');

  onMount(async () => {
    setSearchContext(null);
    try {
      project = await projectSummary();
    } catch {
      failed = true;
    }
  });

  $effect(() => {
    if (!project) {
      setPaletteCommands([]);
      return;
    }
    const seen = new Set<string>();
    const cmds: Command[] = [];
    for (const c of project.contracts) {
      if (seen.has(c.name)) continue;
      seen.add(c.name);
      cmds.push({
        id: `contract:${c.name}`,
        label: c.name,
        category: 'Contract' as const,
        icon: '◈',
        detail: c.kind,
        keywords: ['contract', 'open', 'navigate'],
        run: () => goto(`/contract/${encodeURIComponent(c.name)}`),
      });
    }
    setPaletteCommands(cmds);
  });

  onDestroy(clearPaletteCommands);
</script>

<div class="pj" style:--amber={UNRESOLVED_COLOR} style:--ambiguous={RELATION_STYLE.ambiguous.color}>
  <AppHeader count={project?.contracts.length ?? null} {tab} />

  {#if !project}
    <div class="state-msg full" class:err={failed}>{failed ? 'Failed to connect. Is "ilold serve" running?' : 'Analyzing…'}</div>
  {:else}
    {#key tab}
      {#if tab === 'value'}
        <ValueFlowTab />
      {:else if tab === 'route'}
        <RouteTab contracts={project.contracts} />
      {:else}
        <MapTab contracts={project.contracts} />
      {/if}
    {/key}
  {/if}
</div>
<ErrorToasts />
