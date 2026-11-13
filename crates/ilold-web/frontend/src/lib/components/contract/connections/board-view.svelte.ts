import { goto } from '$app/navigation';
import { page } from '$app/state';
import { setQuery } from '$lib/utils/query';
import { linesParam, readLines, type Line, type LineState } from '$lib/components/project/map-graph';
import { boardQuery, readBoard, type Board } from './board-state';

export const pan = $state({ left: 0, right: 0, by: null as ((direction: number) => void) | null });

export function currentBoard(main: string): Board {
  return readBoard(main, page.url.searchParams);
}

export function currentLines(): Record<Line, LineState> {
  return readLines(page.url.searchParams.get('lines'));
}

export function applyBoard(next: Board): void {
  setQuery(boardQuery(next));
}

export function applyLines(next: Record<Line, LineState>): void {
  setQuery({ lines: linesParam(next) });
}

export function openMain(contract: string, fn: string | null = null): void {
  const query = fn ? `?fn=${encodeURIComponent(fn)}` : '';
  goto(`/contract/${encodeURIComponent(contract)}${query}`);
}
