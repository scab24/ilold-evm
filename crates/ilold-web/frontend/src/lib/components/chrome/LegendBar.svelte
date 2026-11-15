<script lang="ts" module>
  import { lineStyle, type LineStyle } from '$lib/utils/links';
  import type { Basis } from '$lib/api/rest';
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

  let { label, pills = [], onpill, hint, wide = false, foundBy = false, children }: {
    label: string;
    pills?: LegendPill[];
    onpill?: (id: string) => void;
    hint?: string;
    wide?: boolean;
    foundBy?: boolean;
    children?: Snippet;
  } = $props();

  const BASIS_KEY: [Basis, string][] = [['inherits_interface', 'interface'], ['selector_match', 'selector'], ['self', 'this']];

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
  {#if foundBy}
    <span class="basis" title="How the call target was found: through the interface it implements, by selector match, or a this call">
      <span class="tlabel">Found by</span>
      {#each BASIS_KEY as [basis, text] (basis)}
        <span class="key">{@render swatch(lineStyle('calls', basis))}{text}</span>
      {/each}
    </span>
  {/if}
  {#if hint}<span class="hint">{hint}</span>{/if}
</div>
