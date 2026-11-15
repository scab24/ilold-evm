<script lang="ts">
  import type { Mode } from '$lib/components/chrome/AppHeader.svelte';
  import type { Connections, ContractDetail, FunctionSummary } from '$lib/api/rest';
  import Icon from '$lib/components/chrome/Icon.svelte';
  import { visibilityLabel } from '$lib/utils/visibility';
  import { describeGuard, formatAccess, isAccessGuard, isConditional, restricts } from '$lib/utils/access';
  import { describeTries } from '$lib/utils/trycatch';
  import { describeTransfers } from '$lib/utils/value';

  let {
    contract,
    canvasFuncs,
    mode,
    onadd,
    onremove,
    counts,
    onconnections,
  }: {
    contract: ContractDetail;
    canvasFuncs: Set<string>;
    mode: Mode;
    onadd: (func: string) => void;
    onremove: (func: string) => void;
    counts: Pick<Connections, 'outgoing_contracts' | 'incoming_contracts' | 'outgoing_calls' | 'incoming_calls'> | null;
    onconnections: () => void;
  } = $props();

  let sidebarOpen = $state(typeof window === 'undefined' || window.innerWidth >= 1200);
  let query = $state('');

  type Visibility = 'Public' | 'External' | 'Internal' | 'Private';
  const ALL_VISIBILITIES: Visibility[] = ['External', 'Public', 'Internal', 'Private'];
  let visFilter = $state<Set<Visibility>>(new Set<Visibility>(['Public', 'External']));
  let onlyAccessControl = $state(false);

  type Row = {
    name: string;
    key: string;
    visibility: string;
    path_count: number;
    guarded: string | null;
    conditionallyGuarded: string | null;
    stateGuarded: string | null;
    tries: string | null;
    swallows: boolean;
    transfers: string | null;
    source: 'own' | 'inherited';
  };
  function toRow(f: FunctionSummary, source: Row['source']): Row {
    const accessGuards = f.guards.filter(restricts);
    const conditionalGuards = f.guards.filter((g) => isAccessGuard(g) && isConditional(g));
    const stateGuards = f.guards.filter((g) => !isAccessGuard(g));
    const restricted = (typeof f.access === 'object' && 'Restricted' in f.access) || accessGuards.length > 0;
    return {
      name: f.name,
      key: f.key,
      visibility: f.visibility,
      path_count: f.path_count,
      guarded: restricted ? [formatAccess(f.access), ...accessGuards.map(describeGuard)].join('\n') : null,
      conditionallyGuarded: conditionalGuards.length > 0
        ? ['Checked on some paths only', ...conditionalGuards.map(describeGuard)].join('\n')
        : null,
      stateGuarded: stateGuards.length > 0 ? stateGuards.map(describeGuard).join('\n') : null,
      tries: f.try_catches?.length ? describeTries(f.try_catches) : null,
      swallows: f.try_catches?.some((t) => t.catch_clauses.some((c) => c.empty)) ?? false,
      transfers: f.value_transfers?.length ? describeTransfers(f.value_transfers) : null,
      source,
    };
  }
  const allRows = $derived<Row[]>([
    ...(contract.functions ?? []).map((f) => toRow(f, 'own')),
    ...(contract.inherited_functions ?? []).map((f) => toRow(f, 'inherited')),
  ]);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return allRows.filter((r) => {
      if (!visFilter.has(r.visibility as Visibility)) return false;
      if (onlyAccessControl && !r.guarded) return false;
      if (q && !r.name.toLowerCase().includes(q)) return false;
      return true;
    });
  });

  const ownFiltered = $derived(filtered.filter((r) => r.source === 'own'));
  const inheritedFiltered = $derived(filtered.filter((r) => r.source === 'inherited'));
  const totalCount = $derived(allRows.length);
  const visibleCount = $derived(filtered.length);
  const canvasCount = $derived(canvasFuncs.size);
  const canClear = $derived(mode !== 'session' && mode !== 'connections' && canvasCount > 0);

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
  class="sidebar flex flex-col shrink-0 transition-[width] duration-200 {sidebarOpen ? 'w-[260px]' : 'w-8'}"
>
  <div class="header flex items-center justify-between px-2.5 py-2">
    {#if sidebarOpen}
      <h2 class="title">Functions <span class="count">{visibleCount}/{totalCount}</span></h2>
    {/if}
    <button
      class="collapse-btn"
      onclick={() => sidebarOpen = !sidebarOpen}
      aria-label={sidebarOpen ? 'Collapse sidebar' : 'Expand sidebar'}
    >{sidebarOpen ? '◂' : '▸'}</button>
  </div>

  {#if sidebarOpen}
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

    <div class="filters px-2 pb-2">
      {#each ALL_VISIBILITIES as v}
        <button
          class="chip"
          class:on={visFilter.has(v)}
          aria-pressed={visFilter.has(v)}
          onclick={() => toggleVisibility(v)}
          title="Toggle {v} visibility"
        >{visShort(v)}</button>
      {/each}
      <button
        class="chip"
        class:on={onlyAccessControl}
        onclick={() => onlyAccessControl = !onlyAccessControl}
        title="Only access-controlled functions (caller or role checks on every path)"
        aria-pressed={onlyAccessControl}
      >🔒</button>
      {#if canClear}
        <button
          class="chip clear"
          onclick={clearCanvas}
          title="Remove every function from the canvas"
          aria-label="Clear canvas"
        >Clear · {canvasCount}</button>
      {/if}
    </div>

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
          {#each ownFiltered as r, i (r.source + ':' + r.key + ':' + i)}
            {@render functionRow(r)}
          {/each}
        {/if}

        {#if inheritedFiltered.length > 0}
          {#if ownFiltered.length > 0}
            <div class="section-divider">Inherited</div>
          {/if}
          {#each inheritedFiltered as r, i (r.source + ':' + r.key + ':' + i)}
            {@render functionRow(r)}
          {/each}
        {/if}
      {/if}
    </div>
    <button type="button" class="conn" data-testid="connections-line" onclick={onconnections} title={counts ? `Other contracts it calls or reads → ${counts.outgoing_contracts} · that call or read it ← ${counts.incoming_contracts}\nCalls and reads behind them → ${counts.outgoing_calls} · ← ${counts.incoming_calls}\nLibraries and ${contract.name} itself not counted. Opens Connections` : 'Opens Connections'}>
      <Icon name="call" /><span>Connections · <b>{counts?.outgoing_contracts ?? '…'}</b> → · <b>{counts?.incoming_contracts ?? '…'}</b> ←</span>
    </button>
  {/if}
</div>

{#snippet functionRow(row: Row)}
  {@const onCanvas = canvasFuncs.has(row.key)}
  {@const inSession = mode === 'session'}
  {@const showActive = onCanvas && !inSession}
  <div
    class="row-wrap"
    class:active={showActive}
    class:inherited={row.source === 'inherited'}
  >
    <button
      class="row-main"
      onclick={() => onadd(row.key)}
      title={inSession
        ? 'Add step to active scenario'
        : mode === 'connections' ? (onCanvas ? 'Selected on the board' : 'Select on the board')
        : (onCanvas ? 'On canvas — click ✕ to remove' : 'Add to canvas')}
    >
      <span class="row-name" title={row.key}>{row.name}{#if row.key !== row.name}<span class="row-sig">{row.key.slice(row.name.length)}</span>{/if}</span>
      <span class="row-vis {visShort(row.visibility)}">{visShort(row.visibility)}</span>
      {#if row.guarded}
        <span class="row-lock" title={row.guarded}>🔒</span>
      {/if}
      {#if row.conditionallyGuarded}
        <span class="row-lock row-conditional" title={row.conditionallyGuarded}>🔓</span>
      {/if}
      {#if row.stateGuarded}
        <span class="row-lock" title={row.stateGuarded}>🚦</span>
      {/if}
      {#if row.tries}
        <span class="row-try" class:swallows={row.swallows} title={row.tries} data-testid="try-mark">try</span>
      {/if}
      {#if row.transfers}
        <span class="row-value" title={row.transfers} data-testid="value-mark">⇄</span>
      {/if}
      <span class="row-paths">{row.path_count}p</span>
    </button>
    {#if showActive}
      <button
        class="row-toggle"
        onclick={(e) => { e.stopPropagation(); onremove(row.key); }}
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
    background: var(--color-surface);
    border-right: 1px solid var(--color-border-subtle);
  }
  .header {
    min-height: 46px;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .title { margin: 0; font-size: 13px; font-weight: 700; color: var(--color-text-light); }
  .count { margin-left: 4px; font-weight: 600; color: var(--root); }
  .collapse-btn {
    background: none; border: 1px solid transparent; border-radius: 6px; padding: 2px 6px;
    color: var(--root); cursor: pointer; font-size: 12px;
  }
  .conn {
    display: flex; align-items: center; gap: 8px; flex: none; padding: 12px 14px; border: 0;
    border-top: 1px solid var(--color-border-subtle); background: none; color: var(--color-text); font: 13px var(--font-sans); cursor: pointer; text-align: left;
  }
  .conn:hover { background: var(--color-hover); color: var(--color-accent-hover); }
  .conn b { color: var(--color-text-light); font-variant-numeric: tabular-nums; }
  .collapse-btn:hover { color: var(--color-accent-hover); border-color: var(--color-border-subtle); }

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
    font-size: 12px;
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
    font-size: 12px;
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
  .chip.clear { margin-left: auto; color: var(--color-danger-light); border-color: color-mix(in srgb, var(--color-danger) 50%, transparent); }

  .section-divider {
    font-size: 12px;
    font-weight: 700;
    color: var(--root);
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
    font-size: 12px;
    color: var(--color-text-muted);
    font-family: var(--font-mono, monospace);
  }
  .reset-btn {
    appearance: none;
    background: transparent;
    border: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
    color: var(--color-accent-hover);
    font-family: inherit;
    font-size: 12px;
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
    font-size: 12px;
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
  .row-sig { color: var(--color-text-dim); font-weight: normal; }

  .row-vis {
    font-size: 12px;
    font-weight: 600;
    padding: 0 6px;
    border-radius: 999px;
    text-transform: lowercase;
    font-family: inherit;
  }
  .row-vis.pub { background: color-mix(in srgb, var(--color-accent-dark) 40%, transparent); color: var(--color-accent-hover); }
  .row-vis.ext { background: color-mix(in srgb, var(--color-warning) 20%, transparent); color: var(--color-warning); }
  .row-vis.int, .row-vis.priv {
    background: color-mix(in srgb, var(--color-border) 60%, transparent);
    color: var(--color-text-muted);
  }

  .row-lock { font-size: 12px; line-height: 1; }
  .row-conditional { opacity: 0.6; }
  .row-try {
    font-size: 12px;
    font-weight: 700;
    padding: 0 6px;
    border-radius: 999px;
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 18%, transparent);
  }
  .row-value {
    font-size: 12px;
    font-weight: 700;
    padding: 0 6px;
    border-radius: 999px;
    color: var(--color-success-light);
    background: color-mix(in srgb, var(--color-success) 22%, transparent);
  }
  .row-try.swallows {
    color: var(--color-danger-light);
    background: color-mix(in srgb, var(--color-danger) 25%, transparent);
  }

  .row-paths {
    font-size: 12px;
    color: var(--color-root);
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
  .row-toggle .icon-check { font-size: 12px; text-shadow: 0 0 6px var(--color-accent); }
  .row-toggle .icon-remove { display: none; font-size: 12px; }
  .row-wrap:hover .row-toggle .icon-check { display: none; }
  .row-wrap:hover .row-toggle .icon-remove { display: inline; }
  .row-toggle:hover {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
  }
</style>
