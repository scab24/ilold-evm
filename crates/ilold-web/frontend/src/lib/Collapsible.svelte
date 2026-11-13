<script lang="ts">
  let { children, title = '', open = false, count }: {
    children: any;
    title?: string;
    open?: boolean;
    count?: number;
  } = $props();

  let isOpen = $state(open);
</script>

<div class="my-1">
  <button class="flex items-center gap-1 w-full bg-transparent border-none py-1 px-0.5 cursor-pointer font-[inherit] text-left hover:bg-hover hover:rounded-sm" onclick={() => isOpen = !isOpen}>
    <span class="text-[12px] text-root w-3 text-center">{isOpen ? '▼' : '▶'}</span>
    <span class="text-[13px] text-text-light font-bold flex-1">{title}</span>
    {#if count !== undefined}
      <span class="text-[12px] text-root font-semibold">{count}</span>
    {/if}
  </button>
  {#if isOpen}
    <div class="py-0.5 pl-1">
      {@render children()}
    </div>
  {/if}
</div>
