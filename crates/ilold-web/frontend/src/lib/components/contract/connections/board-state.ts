export type Direction = 'callers' | 'both' | 'callees';

export interface Step {
  opened: string;
  to: string | null;
}

export interface Board {
  main: string;
  open: string[];
  col: Record<string, number>;
  parent: Record<string, string>;
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

const outward = (from: number, to: number) => Math.abs(to) === Math.abs(from) + 1 && (from === 0 || Math.sign(to) === Math.sign(from));

export function readBoard(main: string, params: URLSearchParams): Board {
  const open = [main];
  const col: Record<string, number> = { [main]: 0 };
  const parent: Record<string, string> = {};
  for (const entry of (params.get('open') ?? '').split(',')) {
    const gt = entry.indexOf('>');
    const at = entry.lastIndexOf(':');
    const up = entry.slice(0, gt);
    const c = entry.slice(gt + 1, at);
    const k = Number(entry.slice(at + 1));
    if (gt > 0 && at > gt + 1 && open.includes(up) && !open.includes(c) && Number.isInteger(k) && outward(col[up], k)) {
      open.push(c);
      col[c] = k;
      parent[c] = up;
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
    parent,
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
    open: b.open.filter((c) => c !== b.main).map((c) => `${b.parent[c]}>${c}:${b.col[c]}`).join(',') || null,
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
  return { ...b, open: [b.main], col: { [b.main]: 0 }, parent: {}, trail: [], start: null, recent: [b.main], fn: null };
}

export function openStep(b: Board, c: string, col: number, key: string | null, from: string): Board {
  const added = b.open.includes(c) ? b : { ...b, open: [...b.open, c], col: { ...b.col, [c]: col }, parent: { ...b.parent, [c]: from } };
  return {
    ...touch(added, c),
    trail: [...b.trail, { opened: c, to: key }],
    start: b.trail.length ? b.start : b.fn,
    fn: key ?? b.fn,
  };
}

function without(b: Board, gone: Set<string>, trail: Step[]): Board {
  const open = b.open.filter((o) => !gone.has(o));
  const keep = <T>(r: Record<string, T>) => Object.fromEntries(Object.entries(r).filter(([o]) => !gone.has(o)));
  const dropped = b.trail.filter((t) => !trail.includes(t));
  const stays = b.fn !== null && open.includes(contractOf(b.fn)) && !dropped.some((t) => t.to === b.fn);
  const fn = stays ? b.fn : ([...trail].reverse().find((t) => t.to)?.to ?? b.start);
  return {
    ...b,
    open,
    col: keep(b.col),
    parent: keep(b.parent),
    trail,
    start: trail.length ? b.start : null,
    recent: b.recent.filter((o) => !gone.has(o)),
    fn: fn && open.includes(contractOf(fn)) ? fn : null,
  };
}

export function closeBox(b: Board, c: string): Board {
  if (c === b.main || !b.open.includes(c)) return b;
  const gone = new Set([c]);
  for (const o of b.open) if (gone.has(b.parent[o])) gone.add(o);
  return without(b, gone, b.trail.filter((t) => !gone.has(t.opened)));
}

export function popStep(b: Board): Board | null {
  const last = b.trail.at(-1);
  if (!last) return null;
  const trail = b.trail.slice(0, -1);
  const kept = last.opened === b.main || trail.some((t) => t.opened === last.opened);
  return without(b, new Set(kept ? [] : [last.opened]), trail);
}
