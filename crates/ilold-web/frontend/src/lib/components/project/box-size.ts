import type { BoxNodeData } from './BoxNode.svelte';

// mirrors the BoxNode stylesheet so layouts can size cards before they are measured
const HEAD = 27;
const BOTTOM = 9;
const LINE = 17;
const ROWS_TOP = 6;
const ROW = 26;

type Sized = Pick<BoxNodeData, 'lines' | 'rows'>;

export function boxHeight(data: Sized): number {
  return HEAD + BOTTOM + LINE * (data.lines?.length ?? 0) + (data.rows?.length ? ROWS_TOP + ROW * data.rows.length : 0);
}

export function rowCentre(data: Sized, index: number): number {
  return HEAD + LINE * (data.lines?.length ?? 0) + ROWS_TOP + ROW * index + ROW / 2;
}
