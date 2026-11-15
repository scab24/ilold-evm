<script lang="ts" module>
  export interface SegOption<T extends string = string> {
    id: T;
    label: string;
    title?: string;
    disabled?: boolean;
    amb?: boolean;
    href?: string;
  }
</script>

<script lang="ts" generics="T extends string">
  let { label, options, value, onpick, wrap = false }: {
    label: string;
    options: SegOption<T>[];
    value: T | null;
    onpick?: (id: T) => void;
    wrap?: boolean;
  } = $props();
</script>

{#if options.some((o) => o.href)}
  <nav class="seg" class:wrap aria-label={label}>
    {#each options as o (o.id)}
      <a href={o.href} title={o.title} aria-current={value === o.id ? 'page' : undefined}>{o.label}</a>
    {/each}
  </nav>
{:else}
  <div class="seg" class:wrap role="group" aria-label={label}>
    {#each options as o (o.id)}
      <button type="button" class:amb={o.amb} title={o.title} disabled={o.disabled} aria-pressed={value === o.id} onclick={() => onpick?.(o.id)}>{o.label}</button>
    {/each}
  </div>
{/if}
