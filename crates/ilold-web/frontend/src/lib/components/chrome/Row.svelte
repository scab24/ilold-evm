<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon, { type IconName } from './Icon.svelte';

  interface Props {
    icon: IconName;
    tone?: '' | 'val' | 'amb' | 'ambig';
    title: string;
    meta?: Snippet;
    line: number;
    onsource?: () => void;
  }

  let { icon, tone = '', title, meta, line, onsource }: Props = $props();
  const parts = $derived(title.split(/(?<=[.,(])/));
</script>

<li class="li">
  <span class="rw">
    <Icon name={icon} class={tone} />
    <span class="rb-body"><span class="rt">{#each parts as part, i (i)}{part}<wbr />{/each}</span>{#if meta}<span class="rs">{@render meta()}</span>{/if}</span>
  </span>
  {#if onsource}
    <button class="src" type="button" onclick={onsource} title="Open the source at line {line}">:{line}</button>
  {:else}
    <span class="src">:{line}</span>
  {/if}
</li>
