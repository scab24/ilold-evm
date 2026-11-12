<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon, { type IconName } from './Icon.svelte';

  interface Props {
    icon: IconName;
    tone?: '' | 'val' | 'amb';
    title: string;
    meta?: Snippet;
    line?: number | null;
    on?: boolean;
    onclick?: () => void;
    onsource?: () => void;
  }

  let { icon, tone = '', title, meta, line = null, on = false, onclick, onsource }: Props = $props();
</script>

<li class="li" class:on>
  {#if onclick}
    <button class="rb" type="button" {onclick} aria-pressed={on}>
      <Icon name={icon} class={tone} />
      <span class="rb-body"><span class="rt" {title}>{title}</span>{#if meta}<span class="rs">{@render meta()}</span>{/if}</span>
    </button>
  {:else}
    <span class="rb">
      <Icon name={icon} class={tone} />
      <span class="rb-body"><span class="rt" {title}>{title}</span>{#if meta}<span class="rs">{@render meta()}</span>{/if}</span>
    </span>
  {/if}
  {#if line !== null}
    {#if onsource}
      <button class="src" type="button" onclick={onsource} title="Open the source at line {line}">:{line}</button>
    {:else}
      <span class="src">:{line}</span>
    {/if}
  {/if}
</li>
