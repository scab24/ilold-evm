<script lang="ts">
  let { data }: { data: { label: string } } = $props();
</script>

<div class="caption">{data.label}</div>

<style>
  .caption { font: 600 12px var(--font-sans); letter-spacing: 0.06em; text-transform: uppercase; color: var(--root); white-space: nowrap; pointer-events: none; }
</style>
