<script lang="ts" module>
  import type { LineStyle } from '$lib/utils/links';
  import type { IconName } from './Icon.svelte';

  export type PillState = 'show' | 'dim' | 'hide';

  export interface LegendPill {
    id: string;
    label: string;
    tip: string;
    state: PillState;
    line?: LineStyle;
    icon?: IconName;
  }
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';

  let { label, pills = [], onpill, hint, wide = false, children }: {
    label: string;
    pills?: LegendPill[];
    onpill?: (id: string) => void;
    hint?: string;
    wide?: boolean;
    children?: Snippet;
  } = $props();

  const WORD: Record<PillState, string> = { show: 'shown', dim: 'dimmed', hide: 'hidden' };
</script>

{#snippet swatch(line: LineStyle)}
  <svg width="18" height="6" aria-hidden="true"><line x1="1" y1="3" x2="17" y2="3" style:stroke={line.color} stroke-width="2" stroke-dasharray={line.dash} /></svg>
{/snippet}

<div class="legend" class:lines={wide}>
  <span class="tlabel">{label}</span>
  {#each pills as p (p.id)}
    <button type="button" class="fchip" data-s={p.state} title={p.tip} onclick={() => onpill?.(p.id)} aria-label="{p.label}: {WORD[p.state]}">
      {#if p.line}{@render swatch(p.line)}{:else if p.icon}<Icon name={p.icon} />{/if}
      <span class="lbl">{p.label}</span>{#if p.state !== 'show'}<span class="st">{WORD[p.state]}</span>{/if}
    </button>
  {/each}
  {@render children?.()}
  {#if hint}<span class="hint">{hint}</span>{/if}
</div>
