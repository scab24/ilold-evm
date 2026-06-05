<script lang="ts">
  import type { ContractDetail } from '$lib/api/rest';
  import { visibilityLabel } from '$lib/utils/visibility';

  let {
    contract,
    canvasFuncs,
    mode,
    onadd,
    onremove,
  }: {
    contract: ContractDetail;
    canvasFuncs: Set<string>;
    mode: 'cfg' | 'sequences' | 'session';
    onadd: (func: string) => void;
    onremove: (func: string) => void;
  } = $props();

  let sidebarOpen = $state(true);
  let query = $state('');

  type Visibility = 'Public' | 'External' | 'Internal' | 'Private';
  const ALL_VISIBILITIES: Visibility[] = ['External', 'Public', 'Internal', 'Private'];
  let visFilter = $state<Set<Visibility>>(new Set<Visibility>(['Public', 'External']));
  let onlyAccessControl = $state(false);

  type Row = {
    name: string;
    visibility: string;
    path_count: number;
    modifiers: string[];
    source: 'own' | 'inherited';
  };
  const allRows = $derived<Row[]>([
    ...(contract.functions ?? []).map((f): Row => ({
      name: f.name,
      visibility: f.visibility,
      path_count: f.path_count,
      modifiers: f.modifiers ?? [],
      source: 'own',
    })),
    ...(contract.inherited_functions ?? []).map((f): Row => ({
      name: f.name,
      visibility: f.visibility,
      path_count: f.path_count,
      modifiers: f.modifiers ?? [],
      source: 'inherited',
    })),
  ]);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return allRows.filter((r) => {
      if (!visFilter.has(r.visibility as Visibility)) return false;
      if (onlyAccessControl && r.modifiers.length === 0) return false;
      if (q && !r.name.toLowerCase().includes(q)) return false;
      return true;
    });
  });

  const ownFiltered = $derived(filtered.filter((r) => r.source === 'own'));
  const inheritedFiltered = $derived(filtered.filter((r) => r.source === 'inherited'));
  const totalCount = $derived(allRows.length);
  const visibleCount = $derived(filtered.length);
  const canvasCount = $derived(canvasFuncs.size);
  const canClear = $derived(mode !== 'session' && canvasCount > 0);

  function toggleVisibility(v: Visibility) {
    const next = new Set(visFilter);
    if (next.has(v)) next.delete(v);
    else next.add(v);
    visFilter = next;
  }

  function resetFilters() {
    query = '';
    visFilter = new Set<Visibility>(['Public', 'External']);
    onlyAccessControl = false;
  }

  function clearCanvas() {
    if (!canClear) return;
    const toRemove = Array.from(canvasFuncs);
    for (const name of toRemove) onremove(name);
  }

  function visShort(v: string): string {
    return visibilityLabel(v) ?? '';
  }
</script>

<div
  class="sidebar flex flex-col shrink-0 transition-[width] duration-200 {sidebarOpen ? 'w-[240px]' : 'w-8'}"
>
  <!-- Header -->
  <div class="header flex items-center justify-between px-2.5 py-2">
    {#if sidebarOpen}
      <span class="text-[10px] text-text-muted uppercase tracking-wider font-semibold">
        Functions <span class="text-text-dim font-normal">· {visibleCount}/{totalCount}</span>
      </span>
    {/if}
    <button
      class="collapse-btn bg-transparent border-none text-text-muted cursor-pointer text-[11px] px-1 py-0.5 transition-colors duration-150 hover:text-accent-hover"
      onclick={() => sidebarOpen = !sidebarOpen}
      aria-label={sidebarOpen ? 'Collapse sidebar' : 'Expand sidebar'}
    >{sidebarOpen ? '◂' : '▸'}</button>
  </div>

  {#if sidebarOpen}
    <!-- Search input -->
    <div class="px-2 pt-2 pb-1">
      <div class="search-wrap">
        <span class="search-icon" aria-hidden="true">⌕</span>
        <input
          type="text"
          class="search-input"
          placeholder="Search functions..."
          bind:value={query}
          aria-label="Search functions"
        />
        {#if query}
          <button
            class="search-clear"
            onclick={() => query = ''}
            title="Clear search"
            aria-label="Clear search"
          >✕</button>
        {/if}
      </div>
    </div>

    <!-- Filter chips + canvas clear -->
    <div class="filters px-2 pb-2">
      {#each ALL_VISIBILITIES as v}
        <button
          class="chip"
          class:active={visFilter.has(v)}
          onclick={() => toggleVisibility(v)}
          title="Toggle {v} visibility"
        >{visShort(v)}</button>
      {/each}
      <button
        class="chip"
        class:active={onlyAccessControl}
        onclick={() => onlyAccessControl = !onlyAccessControl}
        title="Only functions with modifiers (access control)"
        aria-pressed={onlyAccessControl}
      >🔒</button>
      {#if canClear}
        <button
          class="chip chip-clear"
          onclick={clearCanvas}
          title="Remove every function from the canvas"
          aria-label="Clear canvas"
        >Clear · {canvasCount}</button>
      {/if}
    </div>

    <!-- Function list -->
    <div class="flex-1 overflow-y-auto p-1.5">
      {#if visibleCount === 0}
        <div class="empty-state">
          {#if query || onlyAccessControl || visFilter.size < ALL_VISIBILITIES.length}
            <div class="empty-title">No match</div>
            <button class="reset-btn" onclick={resetFilters}>Reset filters</button>
          {:else}
            <div class="empty-title">No functions</div>
          {/if}
        </div>
      {:else}
        {#if ownFiltered.length > 0}
          {#each ownFiltered as row, i (row.source + ':' + row.name + ':' + i)}
            {@render functionRow(row)}
          {/each}
        {/if}

        {#if inheritedFiltered.length > 0}
          {#if ownFiltered.length > 0}
            <div class="section-divider">Inherited</div>
          {/if}
          {#each inheritedFiltered as row, i (row.source + ':' + row.name + ':' + i)}
            {@render functionRow(row)}
          {/each}
        {/if}
      {/if}
    </div>
  {/if}
</div>

{#snippet functionRow(row: Row)}
  {@const onCanvas = canvasFuncs.has(row.name)}
  {@const inSession = mode === 'session'}
  {@const showActive = onCanvas && !inSession}
  <div
    class="row-wrap"
    class:active={showActive}
    class:inherited={row.source === 'inherited'}
  >
    <button
      class="row-main"
      onclick={() => onadd(row.name)}
      title={inSession
        ? 'Add step to active scenario'
        : (onCanvas ? 'On canvas — click ✕ to remove' : 'Add to canvas')}
    >
      <span class="row-name">{row.name}</span>
      <span class="row-vis {visShort(row.visibility)}">{visShort(row.visibility)}</span>
      {#if row.modifiers.length > 0}
        <span class="row-lock" title={row.modifiers.join(', ')}>🔒</span>
      {/if}
      <span class="row-paths">{row.path_count}p</span>
    </button>
    {#if showActive}
      <button
        class="row-toggle"
        onclick={(e) => { e.stopPropagation(); onremove(row.name); }}
        title="Remove from canvas"
        aria-label="Remove {row.name} from canvas"
      >
        <span class="icon-check" aria-hidden="true">✓</span>
        <span class="icon-remove" aria-hidden="true">✕</span>
      </button>
    {/if}
  </div>
{/snippet}

<style>
  .sidebar {
    background: linear-gradient(180deg, rgba(30, 30, 40, 0.9) 0%, rgba(20, 20, 28, 0.95) 100%);
    border-right: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
  }
  .header {
    border-bottom: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
    background: linear-gradient(180deg, rgba(30, 30, 40, 0.85) 0%, rgba(24, 24, 30, 0.9) 100%);
    backdrop-filter: blur(16px) saturate(1.8);
    -webkit-backdrop-filter: blur(16px) saturate(1.8);
  }
  .collapse-btn { border-radius: 6px; }

  .search-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }
  .search-icon {
    position: absolute;
    left: 8px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--color-text-dim);
    font-size: 13px;
    line-height: 1;
    pointer-events: none;
  }
  .search-input {
    width: 100%;
    appearance: none;
    background: color-mix(in srgb, var(--color-surface) 80%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
    color: var(--color-text);
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    padding: 5px 24px 5px 24px;
    border-radius: 6px;
    outline: none;
    transition: border-color 120ms ease, background 120ms ease;
  }
  .search-input::placeholder { color: var(--color-text-dim); }
  .search-input:focus {
    border-color: color-mix(in srgb, var(--color-accent) 60%, transparent);
    background: color-mix(in srgb, var(--color-surface) 95%, transparent);
  }
  .search-clear {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    background: transparent;
    border: none;
    color: var(--color-text-dim);
    cursor: pointer;
    padding: 2px 5px;
    border-radius: 4px;
    font-size: 10px;
    line-height: 1;
    transition: color 120ms ease, background 120ms ease;
  }
  .search-clear:hover {
    color: var(--color-text);
    background: color-mix(in srgb, var(--color-border) 40%, transparent);
  }

  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    appearance: none;
    background: color-mix(in srgb, var(--color-surface) 70%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
    color: var(--color-text-dim);
    font-family: var(--font-mono, monospace);
    font-size: 9px;
    font-weight: 600;
    padding: 2px 7px;
    border-radius: 999px;
    cursor: pointer;
    transition: color 120ms ease, background 120ms ease, border-color 120ms ease;
  }
  .chip:hover { color: var(--color-text-muted); }
  .chip.active {
    color: var(--color-accent-light);
    background: color-mix(in srgb, var(--color-accent) 18%, transparent);
    border-color: color-mix(in srgb, var(--color-accent) 60%, transparent);
  }
  .chip-clear {
    margin-left: auto;
    color: var(--color-danger);
    border-color: color-mix(in srgb, var(--color-danger) 40%, transparent);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
  }
  .chip-clear:hover {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 22%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 70%, transparent);
  }

  .section-divider {
    font-size: 9px;
    font-weight: 700;
    color: var(--color-text-dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 10px 8px 4px;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 24px 12px;
    text-align: center;
  }
  .empty-title {
    font-size: 11px;
    color: var(--color-text-muted);
    font-family: var(--font-mono, monospace);
  }
  .reset-btn {
    appearance: none;
    background: transparent;
    border: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
    color: var(--color-accent-hover);
    font-family: inherit;
    font-size: 10px;
    padding: 3px 10px;
    border-radius: 6px;
    cursor: pointer;
    transition: color 120ms ease, background 120ms ease;
  }
  .reset-btn:hover {
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
  }

  .row-wrap {
    display: flex;
    align-items: center;
    gap: 2px;
    border-radius: 6px;
    transition: background 120ms ease;
  }
  .row-wrap:hover {
    background: color-mix(in srgb, var(--color-accent) 6%, transparent);
  }
  .row-wrap.active {
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
  }
  .row-wrap.inherited { opacity: 0.85; }

  .row-main {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 5px 8px;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    cursor: pointer;
    text-align: left;
    border-radius: 6px;
  }
  .row-wrap:hover .row-main { color: var(--color-text); }
  .row-wrap.active .row-main { color: var(--color-accent-hover); }

  .row-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-vis {
    font-size: 8px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: 3px;
    text-transform: lowercase;
    font-family: inherit;
  }
  .row-vis.pub { background: color-mix(in srgb, var(--color-accent-dark) 40%, transparent); color: var(--color-accent-hover); }
  .row-vis.ext { background: color-mix(in srgb, var(--color-warning) 20%, transparent); color: var(--color-warning); }
  .row-vis.int, .row-vis.priv {
    background: color-mix(in srgb, var(--color-border) 60%, transparent);
    color: var(--color-text-muted);
  }

  .row-lock { font-size: 10px; line-height: 1; }

  .row-paths {
    font-size: 9px;
    color: var(--color-text-dim);
    padding: 0 5px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--color-border) 35%, transparent);
    font-variant-numeric: tabular-nums;
  }

  .row-toggle {
    appearance: none;
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 4px 6px;
    margin-right: 2px;
    border-radius: 4px;
    line-height: 1;
    color: var(--color-accent);
    transition: color 120ms ease, background 120ms ease;
  }
  .row-toggle .icon-check { font-size: 10px; text-shadow: 0 0 6px var(--color-accent); }
  .row-toggle .icon-remove { display: none; font-size: 10px; }
  .row-wrap:hover .row-toggle .icon-check { display: none; }
  .row-wrap:hover .row-toggle .icon-remove { display: inline; }
  .row-toggle:hover {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
  }
</style>
