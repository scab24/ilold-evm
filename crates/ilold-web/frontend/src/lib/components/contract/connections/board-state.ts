export type Direction = 'callers' | 'both' | 'callees';

export interface Step {
  opened: string;
  to: string | null;
}

export interface Board {
  main: string;
  open: string[];
  col: Record<string, number>;
  trail: Step[];
  start: string | null;
  recent: string[];
  fn: string | null;
  dir: Direction;
  all: boolean;
}

export function fnKey(contract: string, member: string): string {
  return `${contract}|${member}`;
}

export function keyParts(key: string): { contract: string; member: string } {
  const at = key.indexOf('|');
  return at < 0 ? { contract: key, member: '' } : { contract: key.slice(0, at), member: key.slice(at + 1) };
}

export const contractOf = (key: string) => keyParts(key).contract;

export function readBoard(main: string, params: URLSearchParams): Board {
  const open = [main];
  const col: Record<string, number> = { [main]: 0 };
  for (const entry of (params.get('open') ?? '').split(',')) {
    const at = entry.lastIndexOf(':');
    const c = entry.slice(0, at);
    const k = Number(entry.slice(at + 1));
    if (at > 0 && c !== main && !open.includes(c) && Number.isInteger(k) && k !== 0) {
      open.push(c);
      col[c] = k;
    }
  }
  const inBoard = (key: string | null) => key !== null && open.includes(contractOf(key));
  const trail = (params.get('trail') ?? '')
    .split('~')
    .filter(Boolean)
    .map((s) => {
      const { contract, member } = keyParts(s);
      return { opened: contract, to: member ? s : null };
    })
    .filter((t) => open.includes(t.opened));
  const start = params.get('start');
  const fn = params.get('fn');
  const recent = (params.get('recent') ?? '').split(',').filter((c) => open.includes(c));
  const dir = (['callers', 'callees'] as const).find((d) => d === params.get('dir')) ?? 'both';
  return {
    main,
    open,
    col,
    trail,
    start: trail.length && inBoard(start) ? start : null,
    recent: [...open.filter((c) => !recent.includes(c)), ...recent],
    fn: inBoard(fn) ? fn : null,
    dir,
    all: params.get('all') === '1',
  };
}

export function boardQuery(b: Board): Record<string, string | null> {
  return {
    open: b.open.filter((c) => c !== b.main).map((c) => `${c}:${b.col[c]}`).join(',') || null,
    trail: b.trail.map((t) => t.to ?? t.opened).join('~') || null,
    start: b.start,
    fn: b.fn,
    recent: b.open.length > 2 ? b.recent.join(',') : null,
    dir: b.dir === 'both' ? null : b.dir,
    all: b.all ? '1' : null,
  };
}

export function expanded(b: Board): Set<string> {
  return new Set(b.recent.filter((c) => b.open.includes(c)).slice(-2));
}

export function collapsed(b: Board): string[] {
  const e = expanded(b);
  return b.open.filter((c) => !e.has(c));
}

export function touch(b: Board, c: string): Board {
  return { ...b, recent: [...b.recent.filter((x) => x !== c), c] };
}

export function reset(b: Board): Board {
  return { ...b, open: [b.main], col: { [b.main]: 0 }, trail: [], start: null, recent: [b.main], fn: null };
}

export function openStep(b: Board, c: string, col: number, key: string | null): Board {
  const next = touch(b.open.includes(c) ? b : { ...b, open: [...b.open, c], col: { ...b.col, [c]: col } }, c);
  return {
    ...next,
    trail: [...b.trail, { opened: c, to: key }],
    start: b.trail.length ? b.start : b.fn,
    fn: key ?? b.fn,
  };
}

export function closeBox(b: Board, c: string): Board {
  const k = b.col[c];
  if (c === b.main || k === undefined) return b;
  const gone = new Set(b.open.filter((o) => o !== b.main && Math.sign(b.col[o]) === Math.sign(k) && Math.abs(b.col[o]) >= Math.abs(k)));
  const col = Object.fromEntries(Object.entries(b.col).filter(([o]) => !gone.has(o)));
  const cut = b.trail.findIndex((t) => gone.has(t.opened));
  const trail = cut >= 0 ? b.trail.slice(0, cut) : b.trail;
  const fn = b.fn && gone.has(contractOf(b.fn)) ? (trail.length ? (trail.at(-1)!.to ?? null) : b.start) : b.fn;
  return { ...b, open: b.open.filter((o) => !gone.has(o)), col, recent: b.recent.filter((o) => !gone.has(o)), trail, start: trail.length ? b.start : null, fn };
}

export function popStep(b: Board): Board | null {
  const last = b.trail.at(-1);
  if (!last) return null;
  const trail = b.trail.slice(0, -1);
  const drop = last.opened !== b.main && !trail.some((t) => t.opened === last.opened);
  const open = drop ? b.open.filter((o) => o !== last.opened) : b.open;
  const col = drop ? Object.fromEntries(Object.entries(b.col).filter(([o]) => o !== last.opened)) : b.col;
  let fn = trail.length ? (trail.at(-1)!.to ?? b.fn) : b.start;
  if (fn && !open.includes(contractOf(fn))) fn = null;
  return { ...b, open, col, trail, start: trail.length ? b.start : null, recent: b.recent.filter((o) => open.includes(o)), fn };
}
