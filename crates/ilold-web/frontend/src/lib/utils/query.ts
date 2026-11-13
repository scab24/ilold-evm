import { goto } from '$app/navigation';
import { page } from '$app/state';

const SELECTION = ['c', 'pair', 'sel', 'arrow', 'from', 'to', 'mode'];

export function setQuery(updates: Record<string, string | null>): void {
  const url = new URL(page.url);
  for (const [name, value] of Object.entries(updates)) {
    if (value === null || value === '') url.searchParams.delete(name);
    else url.searchParams.set(name, value);
  }
  if (url.search === page.url.search) return;
  const selects = SELECTION.some((name) => url.searchParams.get(name) !== page.url.searchParams.get(name));
  goto(url, { replaceState: !selects, keepFocus: true, noScroll: true });
}
