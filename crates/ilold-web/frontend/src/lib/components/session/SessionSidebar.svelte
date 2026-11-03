<script lang="ts">
  import { untrack } from 'svelte';
  import EmbeddedTerminal from './EmbeddedTerminal.svelte';
  import SessionTimeline from './SessionTimeline.svelte';
  import StatePanel from './StatePanel.svelte';
  import NodeInspector from '$lib/components/contract/NodeInspector.svelte';
  import { getScenarios, getActiveScenario } from '$lib/stores/session.svelte';
  import { promptScenarioName } from '$lib/scenarios/name';
  import { dispatchScenarioAction } from '$lib/scenarios/dispatch';

  let {
    contract,
    selectedNode = null,
    selectedPath = null,
    funcPaths = {},
    expandedFuncs = new Set<string>(),
    seqExpanded = new Map<string, boolean>(),
    mode = 'sequences',
    seqAnalysis = null,
    contractDetail = null,
    lookupBlock = () => null,
    onpathselect = () => {},
    onloadmorepaths = () => {},
    onexpandcfg = () => {},
  }: {
    contract: string;
    selectedNode?: any;
    selectedPath?: any;
    funcPaths?: Record<string, any>;
    expandedFuncs?: Set<string>;
    seqExpanded?: Map<string, boolean>;
    mode?: 'cfg' | 'sequences' | 'session';
    seqAnalysis?: any;
    contractDetail?: { name: string; functions?: any[]; inherited_functions?: any[] } | null;
    lookupBlock?: (blockId: string) => { statements: string[]; node_type: string } | null;
    onpathselect?: (funcName: string, path: any) => void;
    onloadmorepaths?: (funcName: string) => void;
    onexpandcfg?: (funcName: string, nodeId?: string) => void;
  } = $props();

  let open = $state(true);
  let activeTab: 'timeline' | 'state' | 'inspector' = $state('timeline');

  let prevSelectedId = $state<string | null>(null);
  $effect(() => {
    const currentId = selectedNode?.id ?? null;
    const prev = untrack(() => prevSelectedId);
    if (currentId && !prev) {
      activeTab = 'inspector';
    }
    prevSelectedId = currentId;
  });

  const scenarioEntries = $derived(Array.from(getScenarios().entries()));
  const activeScenario = $derived(getActiveScenario());

  async function switchScenario(name: string) {
    if (name === activeScenario) return;
    await dispatchScenarioAction({ Switch: { name } }, contract, 'switch');
  }

  async function newScenario() {
    const name = promptScenarioName();
    if (!name) return;
    await dispatchScenarioAction({ New: { name } }, contract, 'new');
  }

  async function deleteScenario(name: string) {
    await dispatchScenarioAction({ Delete: { name } }, contract, 'delete');
  }

  let sidebarWidth = $state(480);
  const MIN_WIDTH = 320;
  const MAX_WIDTH = 900;
  let draggingWidth = $state(false);

  function onWidthDragStart(e: MouseEvent) {
    e.preventDefault();
    draggingWidth = true;
    document.body.style.userSelect = 'none';
    const startX = e.clientX;
    const startW = sidebarWidth;

    function onMove(ev: MouseEvent) {
      const delta = startX - ev.clientX;
      sidebarWidth = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startW + delta));
    }
    function onUp() {
      draggingWidth = false;
      document.body.style.userSelect = '';
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }
</script>

<div
  class="flex flex-col flex-shrink-0 relative h-full"
  style="
    width: {open ? `${sidebarWidth}px` : '28px'};
    background: linear-gradient(180deg, rgba(20, 20, 28, 0.95) 0%, rgba(16, 16, 22, 0.98) 100%);
    border-left: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
  "
>
  <!-- Sidebar width drag handle (left edge) -->
  {#if open}
    <div
      class="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize z-10 {draggingWidth ? 'bg-accent-dark' : 'hover:bg-surface-alt'}"
      onmousedown={onWidthDragStart}
      role="separator"
      aria-orientation="vertical"
    ></div>
  {/if}

  <button
    class="absolute left-1 top-2 border cursor-pointer px-[3px] py-1 text-[10px] z-5 text-text-muted transition-colors duration-150 hover:text-accent-hover"
    style="
      border-radius: 6px;
      border-color: color-mix(in srgb, var(--color-border) 40%, transparent);
      background: rgba(30, 30, 40, 0.8);
      backdrop-filter: blur(8px);
    "
    onclick={() => open = !open}
  >
    {open ? '▸' : '◂'}
  </button>

  <div class="flex flex-col flex-1 min-h-0" class:hidden={!open}>
    <!-- Scenario selector — pill tabs, one per scenario plus a + button -->
    <div
      class="flex items-center gap-1 px-2 py-1.5 overflow-x-auto shrink-0"
      style="
        background: color-mix(in srgb, var(--color-surface) 60%, transparent);
        border-bottom: 1px solid color-mix(in srgb, var(--color-border) 30%, transparent);
      "
    >
      <span class="text-[9px] uppercase tracking-wider text-text-dim font-semibold mr-1 shrink-0">Scenarios</span>
      {#each scenarioEntries as [name, steps] (name)}
        {@const isActive = name === activeScenario}
        <div
          class="shrink-0 inline-flex items-stretch border transition-colors duration-150"
          style="
            border-radius: 999px;
            font-family: var(--font-mono), monospace;
            border-color: {isActive ? 'var(--color-accent)' : 'color-mix(in srgb, var(--color-border) 50%, transparent)'};
            background: {isActive ? 'color-mix(in srgb, var(--color-accent) 18%, transparent)' : 'rgba(30, 30, 40, 0.6)'};
          "
        >
          <button
            class="bg-transparent border-none {isActive ? 'text-accent-light' : 'text-text-muted hover:text-text'} transition-colors duration-150 cursor-pointer"
            style="padding: 3px {isActive ? '8px' : '4px'} 3px 8px; font-size: 10px; font-family: inherit; border-radius: {isActive ? '999px' : '999px 0 0 999px'};"
            onclick={() => switchScenario(name)}
            title={isActive ? `Active scenario: ${name}` : `Switch to ${name}`}
          >
            {name} <span class="text-text-dim ml-0.5">• {steps.length}</span>
          </button>
          {#if !isActive}
            <button
              class="bg-transparent border-none text-text-dim hover:text-danger transition-colors duration-150 cursor-pointer"
              style="padding: 3px 6px 3px 2px; font-size: 9px; font-family: inherit; border-radius: 0 999px 999px 0;"
              onclick={() => deleteScenario(name)}
              title={`Delete scenario ${name}`}
              aria-label={`Delete scenario ${name}`}
            >
              ✕
            </button>
          {/if}
        </div>
      {/each}
      <button
        class="shrink-0 border cursor-pointer text-text-muted hover:text-accent-hover transition-colors duration-150"
        style="
          padding: 3px 8px;
          border-radius: 999px;
          font-size: 11px;
          font-weight: 600;
          border-color: color-mix(in srgb, var(--color-border) 50%, transparent);
          background: rgba(30, 30, 40, 0.6);
        "
        onclick={newScenario}
        title="Create new scenario"
      >
        +
      </button>
    </div>

    <!-- Tab header — glass effect -->
    <div
      class="flex px-1.5 mb-0"
      style="
        border-bottom: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
        background: linear-gradient(180deg, rgba(30, 30, 40, 0.85) 0%, rgba(24, 24, 30, 0.9) 100%);
        backdrop-filter: blur(16px) saturate(1.8);
        -webkit-backdrop-filter: blur(16px) saturate(1.8);
      "
    >
      <button
        class="flex-1 py-2 bg-transparent border-none text-[10px] font-semibold uppercase tracking-wider cursor-pointer transition-colors duration-150 {activeTab === 'timeline' ? 'text-accent' : 'text-text-muted hover:text-text'}"
        style="border-bottom: 2px solid {activeTab === 'timeline' ? 'var(--color-accent)' : 'transparent'};"
        onclick={() => activeTab = 'timeline'}
      >
        Timeline
      </button>
      <button
        class="flex-1 py-2 bg-transparent border-none text-[10px] font-semibold uppercase tracking-wider cursor-pointer transition-colors duration-150 {activeTab === 'state' ? 'text-accent' : 'text-text-muted hover:text-text'}"
        style="border-bottom: 2px solid {activeTab === 'state' ? 'var(--color-accent)' : 'transparent'};"
        onclick={() => activeTab = 'state'}
      >
        State
      </button>
      <button
        class="flex-1 py-2 bg-transparent border-none text-[10px] font-semibold uppercase tracking-wider cursor-pointer transition-colors duration-150 {activeTab === 'inspector' ? 'text-accent' : 'text-text-muted hover:text-text'}"
        style="border-bottom: 2px solid {activeTab === 'inspector' ? 'var(--color-accent)' : 'transparent'};"
        onclick={() => activeTab = 'inspector'}
        title={selectedNode ? `Inspect: ${selectedNode._funcName || selectedNode.label}` : 'Inspector — select a node to populate'}
      >
        Inspector{#if selectedNode}<span class="ml-1 text-accent-light">●</span>{/if}
      </button>
    </div>

    <div class="flex-1 overflow-y-auto min-h-0 px-1">
      {#if activeTab === 'timeline'}
        <SessionTimeline {contract} />
      {:else if activeTab === 'state'}
        <StatePanel {contract} />
      {:else if contractDetail}
        <NodeInspector
          {selectedNode}
          {selectedPath}
          {funcPaths}
          {expandedFuncs}
          {seqExpanded}
          {mode}
          {seqAnalysis}
          contract={contractDetail}
          {lookupBlock}
          {onpathselect}
          {onloadmorepaths}
          {onexpandcfg}
        />
      {/if}
    </div>
  </div>

  <!-- Floating terminal (positions itself fixed, outside sidebar flow) -->
  <EmbeddedTerminal {contract} />
</div>
