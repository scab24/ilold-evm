<script lang="ts">
  interface Props {
    menu: {
      x: number;
      y: number;
      nodeId: string;
      funcName: string;
      nodeType: string;
      sessionStep?: { stepIndex: number };
    } | null;
    expandedFuncs: Set<string>;
    seqExpanded: Map<string, boolean>;
    mode: 'cfg' | 'sequences' | 'session';
    onexpandcfg: (funcName: string, nodeId: string) => void;
    onremovefunc: (funcName: string) => void;
    onremovenode: (nodeId: string) => void;
    onforkscenario: (stepIndex: number) => void;
    onremovefromhere: (stepIndex: number) => void;
    onviewsource: (funcName: string) => void;
    onopenide: (funcName: string) => void;
    onclose: () => void;
  }

  let { menu, expandedFuncs, seqExpanded, mode, onexpandcfg, onremovefunc, onremovenode, onforkscenario, onremovefromhere, onviewsource, onopenide, onclose }: Props = $props();

  const canViewSource = $derived(
    menu != null && (menu.nodeType === 'function' || menu.nodeType === 'seq-next')
  );

  const showCanvasActions = $derived(mode !== 'session');
</script>

<svelte:window onkeydown={(e) => { if (menu && e.key === 'Escape') onclose(); }} />

{#if menu}
  <div
    class="fixed z-60 p-1.5 min-w-[170px]"
    style="
      left:{menu.x}px;
      top:{menu.y}px;
      border-radius: 10px;
      border: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
      background: linear-gradient(180deg, rgba(30, 30, 40, 0.88) 0%, rgba(24, 24, 30, 0.92) 100%);
      backdrop-filter: blur(16px) saturate(1.8);
      -webkit-backdrop-filter: blur(16px) saturate(1.8);
      box-shadow:
        0 12px 40px -8px rgba(0, 0, 0, 0.45),
        0 4px 16px -4px rgba(0, 0, 0, 0.25),
        0 0 0 1px rgba(91, 155, 213, 0.05);
    "
  >
    {#if showCanvasActions && menu.nodeType === 'function'}
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-accent-hover"
        style="border-radius: 6px;"
        onclick={() => onexpandcfg(menu!.funcName, menu!.nodeId)}
      >
        {expandedFuncs.has(menu.funcName) ? '▼ Collapse CFG' : '▶ Expand CFG'}
      </button>
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-danger"
        style="border-radius: 6px;"
        onclick={() => onremovefunc(menu!.funcName)}
      >
        ✕ Remove from canvas
      </button>
    {:else if showCanvasActions && menu.nodeType === 'seq-next'}
      {#if seqExpanded.has(menu.nodeId)}
        <button
          class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-accent-hover"
          style="border-radius: 6px;"
          onclick={() => onexpandcfg(menu!.funcName, menu!.nodeId)}
        >
          ▼ Collapse
        </button>
      {/if}
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-danger"
        style="border-radius: 6px;"
        onclick={() => onremovenode(menu!.nodeId)}
      >
        ✕ Remove node
      </button>
    {:else if showCanvasActions && menu.nodeType === 'block'}
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-accent-hover"
        style="border-radius: 6px;"
        onclick={() => onexpandcfg(menu!.funcName, menu!.nodeId)}
      >
        ▼ Collapse CFG
      </button>
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-danger"
        style="border-radius: 6px;"
        onclick={() => onremovefunc(menu!.funcName)}
      >
        ✕ Remove function
      </button>
    {/if}
    {#if menu.sessionStep}
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-accent-hover"
        style="border-radius: 6px;"
        onclick={() => onforkscenario(menu!.sessionStep!.stepIndex)}
      >
        ⎇ Fork scenario here
      </button>
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-danger"
        style="border-radius: 6px;"
        onclick={() => onremovefromhere(menu!.sessionStep!.stepIndex)}
      >
        ✕ Remove from here
      </button>
    {/if}
    {#if canViewSource}
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-accent-hover"
        style="border-radius: 6px;"
        onclick={() => onviewsource(menu!.funcName)}
      >
        {"{}"} View source
      </button>
      <button
        class="block w-full px-3 py-1.5 bg-transparent border-none text-text text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-accent-hover"
        style="border-radius: 6px;"
        onclick={() => onopenide(menu!.funcName)}
      >
        ↗ Open in code
      </button>
    {/if}
    <!-- Separator before cancel -->
    <div class="my-1" style="height: 1px; background: color-mix(in srgb, var(--color-border) 30%, transparent);"></div>
    <button
      class="block w-full px-3 py-1.5 bg-transparent border-none text-text-muted text-xs cursor-pointer text-left font-[inherit] transition-colors duration-150 hover:text-text"
      style="border-radius: 6px;"
      onclick={onclose}
    >Cancel</button>
  </div>
{/if}
