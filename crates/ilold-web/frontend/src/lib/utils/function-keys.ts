import type { FunctionSummary } from '$lib/api/rest';

/** Maps every way the API names a function (key, signature, own declaration key) to its summary. */
export function functionIndex(own: FunctionSummary[] = [], inherited: FunctionSummary[] = []): Map<string, FunctionSummary> {
  const all = [...own, ...inherited];
  const index = new Map<string, FunctionSummary>();
  const add = (ref: string, f: FunctionSummary) => { if (!index.has(ref)) index.set(ref, f); };
  for (const f of all) add(f.key, f);
  for (const f of all) add(f.signature, f);
  // sequence trees and analyses name own functions by their declaration key: the name when unique among them
  for (const f of own) add(f.name, f);
  return index;
}
