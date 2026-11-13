<script lang="ts" module>
  export type IconName =
    | 'call' | 'read' | 'value' | 'lib' | 'inherit' | 'iface' | 'unres' | 'entry' | 'lock'
    | 'folder' | 'out' | 'loop' | 'swap' | 'route' | 'deploy' | 'search';

  const PATHS: Record<IconName, string> = {
    call: '<path d="M2 8h10M9 4.5 12.5 8 9 11.5"/>',
    read: '<path d="M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z"/><circle cx="8" cy="8" r="2"/>',
    value: '<circle cx="8" cy="8" r="5.5"/><circle cx="8" cy="8" r="2.5"/>',
    lib: '<path d="M3 2.5h7.5a1.5 1.5 0 0 1 1.5 1.5v9.5H4.5A1.5 1.5 0 0 1 3 12zM3 12a1.5 1.5 0 0 1 1.5-1.5H12"/>',
    inherit: '<path d="M8 14V4M4 7.5 8 3.5l4 4"/>',
    iface: '<path d="M1.5 8h7"/><circle cx="11.5" cy="8" r="3"/>',
    unres: '<circle cx="8" cy="8" r="6"/><path d="M6.2 6.3a1.9 1.9 0 1 1 2.6 1.8c-.6.3-.8.7-.8 1.3M8 11.2v.1"/>',
    entry: '<path d="M10 3h3v10h-3M2 8h8M7 5l3 3-3 3"/>',
    lock: '<rect x="3.5" y="7" width="9" height="6.5" rx="1.5"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2"/>',
    folder: '<path d="M2 4.5a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/>',
    out: '<path d="M9 3h4v4M13 3 7.5 8.5M11 9.5V13H3V5h3.5"/>',
    loop: '<path d="M12.5 6A5 5 0 1 0 13 9M13 3v3h-3"/>',
    swap: '<path d="M3 5.5h10M10 2.5l3 3-3 3M13 10.5H3M6 7.5l-3 3 3 3"/>',
    route: '<circle cx="3.5" cy="12" r="1.8"/><circle cx="12.5" cy="4" r="1.8"/><path d="M5 11c4 0 2-7 6-7"/>',
    deploy: '<rect x="2.5" y="2.5" width="11" height="11" rx="2"/><path d="M8 5.5v5M5.5 8h5"/>',
    search: '<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5 14 14"/>',
  };
</script>

<script lang="ts">
  let { name, size = 16, class: cls = '' }: { name: IconName; size?: number; class?: string } = $props();
</script>

<svg class="pj-icon {cls}" width={size} height={size} viewBox="0 0 16 16" aria-hidden="true">{@html PATHS[name]}</svg>

<style>
  .pj-icon {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    vertical-align: -3px;
  }
</style>
