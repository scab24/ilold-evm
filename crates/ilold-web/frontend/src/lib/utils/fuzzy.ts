
export function score(label: string, query: string): number {
  if (!query) return 1; // empty query keeps original order via stable sort
  const l = label.toLowerCase();
  const q = query.toLowerCase();
  if (l === q) return 1000;
  if (l.startsWith(q)) return 500 - l.length;
  const idx = l.indexOf(q);
  if (idx >= 0) return 100 - idx - Math.floor(l.length / 4);
  let qi = 0;
  for (let i = 0; i < l.length && qi < q.length; i++) {
    if (l[i] === q[qi]) qi++;
  }
  if (qi === q.length) return 10 - Math.floor(l.length / 4);
  return -1;
}

export function scoreWithKeywords(
  label: string,
  keywords: readonly string[] | undefined,
  query: string,
): number {
  let best = score(label, query);
  if (keywords) {
    for (const k of keywords) {
      const s = score(k, query);
      if (s > best) best = s;
    }
  }
  return best;
}

export function matchPositions(label: string, query: string): number[] {
  if (!query) return [];
  const l = label.toLowerCase();
  const q = query.toLowerCase();
  const idx = l.indexOf(q);
  if (idx >= 0) {
    const out: number[] = [];
    for (let i = 0; i < q.length; i++) out.push(idx + i);
    return out;
  }
  const out: number[] = [];
  let qi = 0;
  for (let i = 0; i < l.length && qi < q.length; i++) {
    if (l[i] === q[qi]) {
      out.push(i);
      qi++;
    }
  }
  return qi === q.length ? out : [];
}
