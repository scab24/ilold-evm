<script lang="ts">
  let { children, title = '', x = 12, y = 50, width = 300, onclose }: {
    children: any;
    title?: string;
    x?: number;
    y?: number;
    width?: number;
    onclose?: () => void;
  } = $props();

  let posX = $state(x);
  let posY = $state(y);
  let w = $state(width);
  let h = $state(0);
  let panelEl: HTMLDivElement;
  let mode: 'idle' | 'drag' | 'resize-se' | 'resize-e' | 'resize-s' = 'idle';
  let startX = 0;
  let startY = 0;
  let startW = 0;
  let startH = 0;

  function onDragStart(e: MouseEvent) {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === 'INPUT' || tag === 'BUTTON' || tag === 'A') return;
    mode = 'drag';
    startX = e.clientX - posX;
    startY = e.clientY - posY;
    listen();
  }

  function onResizeStart(which: 'resize-se' | 'resize-e' | 'resize-s', e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    mode = which;
    startX = e.clientX;
    startY = e.clientY;
    startW = w;
    startH = h > 0 ? h : panelEl.offsetHeight;
    listen();
  }

  function onMove(e: MouseEvent) {
    if (mode === 'drag') {
      posX = Math.max(0, Math.min(window.innerWidth - 100, e.clientX - startX));
      posY = Math.max(0, Math.min(window.innerHeight - 40, e.clientY - startY));
    } else if (mode === 'resize-se') {
      w = Math.max(180, startW + (e.clientX - startX));
      h = Math.max(100, startH + (e.clientY - startY));
    } else if (mode === 'resize-e') {
      w = Math.max(180, startW + (e.clientX - startX));
    } else if (mode === 'resize-s') {
      h = Math.max(100, startH + (e.clientY - startY));
    }
  }

  function onEnd() {
    mode = 'idle';
    window.removeEventListener('mousemove', onMove);
    window.removeEventListener('mouseup', onEnd);
  }

  function listen() {
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onEnd);
  }
</script>

<div
  class="draggable-panel fixed z-50 flex flex-col max-h-[calc(100vh-20px)] overflow-hidden min-w-[180px]"
  bind:this={panelEl}
  style="
    left:{posX}px;
    top:{posY}px;
    width:{w}px;
    {h > 0 ? `height:${h}px;` : ''}
    border-radius: 12px;
    border: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
    background: linear-gradient(180deg, rgba(30, 30, 40, 0.92) 0%, rgba(20, 20, 28, 0.96) 100%);
    backdrop-filter: blur(16px) saturate(1.8);
    -webkit-backdrop-filter: blur(16px) saturate(1.8);
    box-shadow:
      0 25px 50px -12px rgba(0, 0, 0, 0.5),
      0 12px 24px -8px rgba(0, 0, 0, 0.3),
      0 0 0 1px rgba(91, 155, 213, 0.05),
      0 0 60px -20px rgba(91, 155, 213, 0.06);
  "
>
  <!-- Title bar — glass header -->
  <div
    class="flex items-center py-2 px-3 cursor-grab select-none gap-1.5 shrink-0 active:cursor-grabbing"
    style="
      border-bottom: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
      background: linear-gradient(180deg, rgba(30, 30, 40, 0.85) 0%, rgba(24, 24, 30, 0.9) 100%);
      backdrop-filter: blur(16px) saturate(1.8);
      -webkit-backdrop-filter: blur(16px) saturate(1.8);
    "
    onmousedown={onDragStart}
  >
    <span class="text-[11px] font-semibold text-accent-hover flex-1 overflow-hidden text-ellipsis whitespace-nowrap tracking-wide uppercase">{title}</span>
    {#if onclose}
      <button
        class="bg-transparent border-none text-text-dim cursor-pointer text-xs py-0.5 px-1.5 transition-colors duration-150 hover:text-text"
        style="border-radius: 6px;"
        onclick={onclose}
      >✕</button>
    {/if}
  </div>
  <div class="drag-body flex-1 overflow-y-auto pb-3.5">
    {@render children()}
  </div>
  <!-- Resize edges -->
  <div class="resize-e" onmousedown={(e) => onResizeStart('resize-e', e)}></div>
  <div class="resize-s" onmousedown={(e) => onResizeStart('resize-s', e)}></div>
  <div class="resize-se" onmousedown={(e) => onResizeStart('resize-se', e)}></div>
</div>

<style>
  .drag-body {
    scrollbar-width: thin;
    scrollbar-color: var(--color-border) transparent;
  }

  .resize-e {
    position: absolute; top: 10px; right: -3px; bottom: 10px; width: 6px;
    cursor: ew-resize;
  }
  .resize-s {
    position: absolute; left: 10px; bottom: -3px; right: 10px; height: 6px;
    cursor: ns-resize;
  }
  .resize-se {
    position: absolute; bottom: 0; right: 0; width: 18px; height: 18px;
    cursor: nwse-resize; border-radius: 0 0 12px 0;
  }
  .resize-se::after {
    content: '';
    position: absolute; bottom: 4px; right: 4px;
    width: 8px; height: 8px;
    border-right: 2px solid var(--color-text-dim);
    border-bottom: 2px solid var(--color-text-dim);
    opacity: 0.6;
  }
  .resize-se:hover::after { opacity: 1; border-color: var(--color-accent-hover); }
</style>
