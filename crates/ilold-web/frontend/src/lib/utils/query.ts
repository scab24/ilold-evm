import { goto } from '$app/navigation';
import { page } from '$app/state';

export function setQuery(updates: Record<string, string | null>, push = false): void {
  const url = new URL(page.url);
  for (const [key, value] of Object.entries(updates)) {
    if (value === null || value === '') url.searchParams.delete(key);
    else url.searchParams.set(key, value);
  }
  if (url.search === page.url.search) return;
  goto(url, { replaceState: !push, keepFocus: true, noScroll: true });
}
