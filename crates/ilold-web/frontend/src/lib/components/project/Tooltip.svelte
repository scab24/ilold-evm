<script lang="ts">
  import type { Snippet } from 'svelte';

  let { x, y, within, children }: { x: number; y: number; within: HTMLElement | null; children: Snippet } = $props();

  const GAP = 12;
  const EDGE = 8;
  let w = $state(0);
  let h = $state(0);

  const bounds = $derived(within?.getBoundingClientRect() ?? { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight, width: window.innerWidth });

  function along(at: number, size: number, low: number, high: number): number {
    const after = at + GAP;
    const start = after + size > high - EDGE ? at - GAP - size : after;
    return Math.max(low + EDGE, Math.min(start, high - EDGE - size));
  }
</script>

<div
  class="tip"
  role="tooltip"
  bind:offsetWidth={w}
  bind:offsetHeight={h}
  style:left="{along(x, w, bounds.left, bounds.right)}px"
  style:top="{along(y, h, bounds.top, bounds.bottom)}px"
  style:max-width="{Math.min(440, bounds.width - 2 * EDGE)}px"
  class:measuring={w === 0}
>{@render children()}</div>
