<script lang="ts" module>
  export interface RegionNodeData {
    [key: string]: unknown;
    _type: 'box';
    label: string;
    width: number;
    height: number;
    tone?: '' | 'amb';
    onfold?: () => void;
  }
</script>

<script lang="ts">
  import Icon from '$lib/components/chrome/Icon.svelte';

  let { data }: { data: RegionNodeData } = $props();
</script>

<div class="region {data.tone ?? ''}" style:width="{data.width}px" style:height="{data.height}px">
  <div class="title"><Icon name={data.tone === 'amb' ? 'unres' : 'folder'} size={14} />{data.label}</div>
  {#if data.onfold}<button type="button" class="fold nodrag" onclick={(e) => { e.stopPropagation(); data.onfold?.(); }}>Fold</button>{/if}
</div>

<style>
  .region { position: relative; box-sizing: border-box; border-radius: 10px; border: 1px solid var(--color-border); background: rgba(255, 255, 255, 0.018); cursor: default; }
  .region.amb { border: 1px dashed color-mix(in srgb, var(--amber) 45%, transparent); }
  .title { display: flex; align-items: center; gap: 6px; padding: 6px 10px; white-space: nowrap; overflow: hidden; font: 600 12px var(--font-sans); letter-spacing: 0.04em; color: var(--root); }
  .amb .title { color: var(--amber); }
  .fold { position: absolute; bottom: 3px; left: 50%; transform: translateX(-50%); padding: 0 8px; background: none; border: 0; font: 12px var(--font-sans); color: var(--color-accent-hover); cursor: pointer; }
</style>
