<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { getProject, type ProjectSummary } from '$lib/api/rest';
  import { AMBIGUOUS_COLOR } from '$lib/utils/links';
  import { setSearchContext } from '$lib/stores/search.svelte';
  import { togglePalette, setPaletteCommands, clearPaletteCommands } from '$lib/stores/palette.svelte';
  import type { Command } from '$lib/commands/registry';
  import ErrorToasts from '$lib/components/ui/ErrorToasts.svelte';
  import MapTab from '$lib/components/project/MapTab.svelte';
  import ValueFlowTab from '$lib/components/project/ValueFlowTab.svelte';
  import RouteTab from '$lib/components/project/RouteTab.svelte';
  import '$lib/components/project/project.css';

  const TABS = [
    { id: 'map', label: 'Map' },
    { id: 'value', label: 'Value flow' },
    { id: 'route', label: 'Route' },
  ] as const;
  type Tab = (typeof TABS)[number]['id'];

  let project = $state<ProjectSummary | null>(null);
  let failed = $state(false);

  const tab = $derived<Tab>(TABS.find((t) => t.id === page.url.searchParams.get('tab'))?.id ?? 'map');

  function tabHref(id: Tab): string {
    return id === 'map' ? '/' : `/?tab=${id}`;
  }

  onMount(async () => {
    setSearchContext(null);
    try {
      project = await getProject();
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

<div class="pj" style:--amber={AMBIGUOUS_COLOR}>
  <header class="top">
    <span class="brand">ilold</span>
    <span class="sub-brand">execution path analyzer</span>
    {#if project}<span class="sub-brand">{project.contracts.length} contracts</span>{/if}
    <nav class="views" aria-label="Project views">
      {#each TABS as t}
        <a href={tabHref(t.id)} aria-current={tab === t.id ? 'page' : undefined}>{t.label}</a>
      {/each}
    </nav>
    <button type="button" class="btn search-btn" onclick={togglePalette}>⌘K Search</button>
  </header>

  {#if !project}
    <div class="state-msg" class:err={failed} style="grid-row: 2 / 5">{failed ? 'Failed to connect. Is "ilold serve" running?' : 'Analyzing…'}</div>
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
