<script lang="ts" module>
  import type { IconName } from './Icon.svelte';

  export interface ChipEdgeData {
    [key: string]: unknown;
    chip?: string;
    icon?: IconName;
    tone?: '' | 'val' | 'amb';
    on?: boolean;
    faded?: boolean;
    labelAt?: number | null;
    onpick?: (id: string) => void;
  }
</script>

<script lang="ts">
  import { BaseEdge, EdgeLabel, getBezierPath, Position, type EdgeProps } from '@xyflow/svelte';
  import Icon from './Icon.svelte';
  import { bezierPoint, type Side } from './chips';

  let { id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, style, markerEnd, data }: EdgeProps = $props();

  const d = $derived((data ?? {}) as ChipEdgeData);
  const geometry = $derived(getBezierPath({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition }));
  const SIDE: Record<Position, Side> = { [Position.Left]: 'l', [Position.Right]: 'r', [Position.Top]: 't', [Position.Bottom]: 'b' };
  const label = $derived(
    d.labelAt === null ? null : bezierPoint({ x: sourceX, y: sourceY }, SIDE[sourcePosition], { x: targetX, y: targetY }, SIDE[targetPosition], d.labelAt ?? 0.5),
  );
</script>

<BaseEdge {id} path={geometry[0]} {style} {markerEnd} interactionWidth={14} />
{#if d.chip && label}
  <EdgeLabel x={label.x} y={label.y} onclick={() => d.onpick?.(id)}>
    <span class="chip {d.tone ?? ''}" class:on={d.on} class:faded={d.faded}>{#if d.icon}<Icon name={d.icon} size={13} />{/if}{d.chip}</span>
  </EdgeLabel>
{/if}
