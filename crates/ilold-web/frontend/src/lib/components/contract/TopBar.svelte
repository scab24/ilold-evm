<script lang="ts">
  import { toggleTerminal, isTerminalVisible } from '$lib/stores/terminal.svelte';

  let {
    contractName,
    mode,
    seqDirection,
    onmodechange,
    onsearch,
    oncenter,
    onseqdirection,
    onsessionback,
    onsessionclear,
  }: {
    contractName: string;
    mode: 'cfg' | 'sequences' | 'session';
    seqDirection: 'TB' | 'LR';
    onmodechange: (mode: 'cfg' | 'sequences' | 'session') => void;
    onsearch: () => void;
    oncenter: () => void;
    onseqdirection: (dir: 'TB' | 'LR') => void;
    onsessionback: () => void;
    onsessionclear: () => void;
  } = $props();

  const isMac = typeof navigator !== 'undefined'
    && /Mac|iPhone|iPad|iPod/i.test(navigator.platform);
  const modKey = isMac ? '⌘' : 'Ctrl';
</script>

<nav
  class="topbar flex items-center gap-1 h-11 px-3 shrink-0 select-none"
  aria-label="Primary toolbar"
>
  <!-- Brand + breadcrumb -->
  <a
    href="/"
    class="brand-link flex items-center gap-1.5 text-text-muted no-underline px-1.5 py-1 transition-colors duration-150 hover:text-accent-hover"
    title="Back to contracts"
  >
    <span class="text-[11px] font-bold tracking-wide uppercase">Ilold</span>
    <span class="text-text-dim text-[12px]">›</span>
  </a>
  <span class="text-[11px] font-bold text-text tracking-wide uppercase truncate max-w-[220px]" title={contractName}>
    {contractName}
  </span>

  <span class="divider"></span>

  <!-- Mode switcher -->
  <div class="seg" role="group" aria-label="View mode">
    <button
      class="seg-btn"
      class:active={mode === 'cfg'}
      onclick={() => onmodechange('cfg')}
      title="Control-flow graph view"
    >CFG</button>
    <button
      class="seg-btn"
      class:active={mode === 'sequences'}
      onclick={() => onmodechange('sequences')}
      title="Function sequence view"
    >Seq</button>
    <button
      class="seg-btn"
      class:active={mode === 'session'}
      onclick={() => onmodechange('session')}
      title="Session mode — click in sidebar to add steps"
    >Session</button>
  </div>

  {#if mode === 'sequences'}
    <span class="divider"></span>
    <div class="seg" role="group" aria-label="Sequence tree direction">
      <button
        class="seg-btn"
        class:active={seqDirection === 'TB'}
        onclick={() => onseqdirection('TB')}
        title="Vertical layout"
      >↓</button>
      <button
        class="seg-btn"
        class:active={seqDirection === 'LR'}
        onclick={() => onseqdirection('LR')}
        title="Horizontal layout"
      >→</button>
    </div>
  {/if}

  <span class="divider"></span>

  <!-- Session controls -->
  <button
    class="tool-btn"
    onclick={onsessionback}
    title="Back — remove last step of active scenario"
    aria-label="Remove last step"
  >↶</button>
  <button
    class="tool-btn danger-hover"
    onclick={onsessionclear}
    title="Clear — remove all steps of active scenario"
    aria-label="Clear active scenario"
  >🗑</button>

  <!-- Utility cluster pushed to the right -->
  <div class="ml-auto flex items-center gap-1">
    <button
      class="tool-btn search-btn"
      onclick={onsearch}
      title="Search functions, state vars and more"
    >
      <span>Search</span>
      <kbd class="kbd">{modKey}K</kbd>
    </button>
    <button
      class="tool-btn"
      onclick={oncenter}
      title="Center all nodes in view"
      aria-label="Center canvas"
    >Center</button>
    <span class="divider"></span>
    <button
      class="tool-btn mono"
      class:active={isTerminalVisible()}
      onclick={toggleTerminal}
      title={isTerminalVisible() ? 'Hide terminal' : 'Show terminal'}
      aria-pressed={isTerminalVisible()}
    >&gt;_</button>
  </div>
</nav>

<style>
  .topbar {
    background: linear-gradient(180deg, rgba(30, 30, 40, 0.88) 0%, rgba(22, 22, 28, 0.92) 100%);
    border-bottom: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
    backdrop-filter: blur(16px) saturate(1.8);
    -webkit-backdrop-filter: blur(16px) saturate(1.8);
    z-index: 20;
  }

  .brand-link {
    border-radius: 6px;
  }

  .divider {
    width: 1px;
    height: 18px;
    margin: 0 4px;
    background: color-mix(in srgb, var(--color-border) 45%, transparent);
    flex-shrink: 0;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--color-surface) 70%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-border) 30%, transparent);
  }
  .seg-btn {
    appearance: none;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: inherit;
    font-size: 11px;
    font-weight: 500;
    padding: 4px 10px;
    border-radius: 6px;
    cursor: pointer;
    transition: color 120ms ease, background 120ms ease;
  }
  .seg-btn:hover:not(.active) {
    color: var(--color-accent-hover);
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
  }
  .seg-btn.active {
    color: var(--color-accent-light);
    background: color-mix(in srgb, var(--color-accent) 18%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-accent) 45%, transparent);
  }

  .tool-btn {
    appearance: none;
    background: transparent;
    border: 1px solid transparent;
    color: var(--color-text-muted);
    font-family: inherit;
    font-size: 11px;
    padding: 4px 10px;
    border-radius: 6px;
    cursor: pointer;
    transition: color 120ms ease, background 120ms ease, border-color 120ms ease;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .tool-btn:hover {
    color: var(--color-accent-hover);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
  }
  .tool-btn.danger-hover:hover {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
  }
  .tool-btn.active {
    color: var(--color-accent-light);
    background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    border-color: color-mix(in srgb, var(--color-accent) 50%, transparent);
  }
  .tool-btn.mono {
    font-family: var(--font-mono, monospace);
  }
  .search-btn {
    padding-right: 6px;
  }

  .kbd {
    font-family: var(--font-mono, monospace);
    font-size: 9px;
    font-weight: 600;
    color: var(--color-text-dim);
    background: color-mix(in srgb, var(--color-border) 35%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
    border-radius: 4px;
    padding: 1px 5px;
    line-height: 1.1;
    letter-spacing: 0.2px;
  }
</style>
