<script lang="ts">
  import type { Mode } from '$lib/components/chrome/AppHeader.svelte';
  import { untrack } from 'svelte';
  import EmbeddedTerminal from './EmbeddedTerminal.svelte';
  import SessionTimeline from './SessionTimeline.svelte';
  import StatePanel from './StatePanel.svelte';
  import NodeInspector from '$lib/components/contract/NodeInspector.svelte';
  import type { CytoscapeGraph } from '$lib/api/rest';
  import { getScenarios, getActiveScenario } from '$lib/stores/session.svelte';
  import { promptScenarioName } from '$lib/scenarios/name';
  import { dispatchScenarioAction } from '$lib/scenarios/dispatch';
  import Seg from '$lib/components/chrome/Seg.svelte';

  type Tab = 'timeline' | 'state' | 'inspector';
  const TABS: { id: Tab; label: string; title?: string }[] = [
    { id: 'timeline', label: 'Timeline' },
    { id: 'state', label: 'State' },
    { id: 'inspector', label: 'Inspector' },
  ];

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
    callgraph = null,
    onviewsource = () => {},
  }: {
    contract: string;
    selectedNode?: any;
    selectedPath?: any;
    funcPaths?: Record<string, any>;
    expandedFuncs?: Set<string>;
    seqExpanded?: Map<string, boolean>;
    mode?: Mode;
    seqAnalysis?: any;
    contractDetail?: { name: string; functions?: any[]; inherited_functions?: any[] } | null;
    lookupBlock?: (blockId: string) => { statements: string[]; node_type: string } | null;
    onpathselect?: (funcName: string, path: any) => void;
    onloadmorepaths?: (funcName: string) => void;
    onexpandcfg?: (funcName: string, nodeId?: string) => void;
    callgraph?: CytoscapeGraph | null;
    onviewsource?: (contract: string, func: string) => void;
  } = $props();

  let open = $state(true);
  let activeTab: Tab = $state('timeline');

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

  let sidebarWidth = $state(typeof window !== 'undefined' && window.innerWidth < 1200 ? 300 : 380);
  const MIN_WIDTH = 284;
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
    background: var(--color-surface);
    border-left: 1px solid var(--color-border-subtle);
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
    <div class="ssb-head">
      <span class="tlabel">Scenarios</span>
      {#each scenarioEntries as [name, steps] (name)}
        {@const isActive = name === activeScenario}
        <span class="scn">
          <button type="button" class="chip" class:on={isActive} onclick={() => switchScenario(name)} title={isActive ? `Active scenario: ${name}` : `Switch to ${name}`}>{name} · {steps.length}</button>
          {#if !isActive}
            <button type="button" class="scn-x" onclick={() => deleteScenario(name)} title={`Delete scenario ${name}`} aria-label={`Delete scenario ${name}`}>✕</button>
          {/if}
        </span>
      {/each}
      <button type="button" class="chip" onclick={newScenario} title="Create new scenario" aria-label="Create new scenario">+</button>
    </div>
    <div class="ssb-tabs">
      <Seg label="Sidebar" options={TABS.map((t) => (t.id === 'inspector' ? { ...t, label: selectedNode ? 'Inspector ●' : 'Inspector', title: selectedNode ? `Inspect: ${selectedNode._funcName || selectedNode.label}` : 'Select a node to inspect it' } : t))} value={activeTab} onpick={(t) => (activeTab = t)} />
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
          {callgraph}
          {onviewsource}
        />
      {/if}
    </div>
  </div>

  <!-- Floating terminal (positions itself fixed, outside sidebar flow) -->
  <EmbeddedTerminal {contract} />
</div>

<style>
  .ssb-head { display: flex; align-items: center; gap: 6px; padding: 8px 10px 8px 32px; overflow-x: auto; flex: none; border-bottom: 1px solid var(--color-border-subtle); }
  .scn { display: inline-flex; align-items: center; flex: none; }
  .scn-x { background: none; border: 0; padding: 0 4px; color: var(--root); cursor: pointer; font-size: 12px; }
  .scn-x:hover { color: var(--color-danger-light); }
  .ssb-tabs { display: flex; padding: 8px 10px; flex: none; border-bottom: 1px solid var(--color-border-subtle); }
</style>
