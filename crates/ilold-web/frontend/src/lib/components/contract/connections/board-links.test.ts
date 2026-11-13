import test from 'node:test';
import assert from 'node:assert/strict';
import type { Connections } from '$lib/api/rest';
import { DEFAULT_LINES } from '../../project/map-graph.ts';
import { build, linksOf, type Built } from './board-links.ts';
import { boxHeight } from '../../project/box-size.ts';
import { boardGraph, place, type Known, type Rect } from './board-graph.ts';
import curve from '../../../api/fixtures/connections.json' with { type: 'json' };
import hook from '../../../api/fixtures/connections_hook.json' with { type: 'json' };
import { openStep, readBoard, touch, type Board } from './board-state.ts';

const LIBS = new Set(['Payout', 'PoolMath']);
const LINKS = linksOf([curve, hook] as Connections[], LIBS);

function older(): Board {
  let b = readBoard('Curve', new URLSearchParams());
  b = openStep(b, 'Hook', 1, 'Hook|migrate', 'Curve');
  b = openStep(b, 'Token', 1, null, 'Curve');
  return touch(b, 'Curve');
}

const view = (B: Built) => ({
  nodes: B.nodes.map((n) => ({ id: n.id, type: n.type, c: n.c, col: n.col, owner: n.owner, roles: [...n.roles], rows: [...n.rows.keys()], n: n.n })),
  edges: B.edges.map((e) => [e.a, e.ar, e.b, e.br]),
  dimmed: B.dimmed.map((e) => [e.a, e.b]),
});

test('an older box folds in place: same id and column as when open, no rows, lines end on the card', () => {
  const B = build(older(), LINKS, DEFAULT_LINES, () => []);
  assert.deepEqual(view(B), {
    nodes: [
      { id: 'O:Curve', type: 'open', c: 'Curve', col: 0, owner: undefined, roles: [], rows: ['Curve|buy', 'Curve|_graduate', 'Curve|Payout::pay'], n: 9 },
      { id: 'O:Token', type: 'open', c: 'Token', col: 1, owner: undefined, roles: [], rows: ['Token|mint', 'Token|transfer'], n: 2 },
      { id: 'O:Hook', type: 'collapsed', c: 'Hook', col: 1, owner: undefined, roles: [], rows: [], n: 2 },
      { id: 'C:DenA@1', type: 'folded', c: 'DenA', col: 1, owner: 'Curve', roles: ['callee'], rows: ['DenA|deliver'], n: 1 },
      { id: 'C:DenB@1', type: 'folded', c: 'DenB', col: 1, owner: 'Curve', roles: ['callee'], rows: ['DenB|deliver'], n: 1 },
      { id: 'C:#libraries@1', type: 'group', c: '#libraries', col: 1, owner: 'Curve', roles: ['callee'], rows: [], n: 1 },
      { id: 'C:Router@-1', type: 'folded', c: 'Router', col: -1, owner: 'Curve', roles: ['caller'], rows: ['Router|buyWith'], n: 2 },
    ],
    edges: [
      ['O:Curve', 'Curve|buy', 'C:DenA@1', 'DenA|deliver'],
      ['O:Curve', 'Curve|buy', 'C:DenB@1', 'DenB|deliver'],
      ['O:Curve', 'Curve|_graduate', 'O:Hook', ''],
      ['O:Curve', 'Curve|buy', 'O:Token', 'Token|mint'],
      ['O:Curve', 'Curve|Payout::pay', 'O:Token', 'Token|transfer'],
      ['C:Router@-1', 'Router|buyWith', 'O:Curve', 'Curve|buy'],
    ],
    dimmed: [
      ['O:Curve', 'O:Hook'],
      ['O:Curve', 'C:#libraries@1'],
      ['C:Router@-1', 'O:Curve'],
    ],
  });
});

const PORTS = [
  { id: 'in', side: 'l', type: 'target' },
  { id: 'out', side: 'r', type: 'source' },
  { id: 'back', side: 'r', type: 'target' },
];
const FACTS = (c: string, m: string) => (c === 'Curve' && m === 'buy' ? { entry: true, restricted: false, value: false } : undefined);
const ORDER = (c: string) => (c === 'Curve' ? ['buy', '_graduate'] : []);
const selected = (): Board => ({ ...older(), fn: 'Curve|buy' });

test('the board graph draws cards with their rows and lines from caller to callee', () => {
  const g = boardGraph(build(selected(), LINKS, DEFAULT_LINES, () => []), selected(), DEFAULT_LINES, FACTS, ORDER);
  const data = new Map(g.nodes.map((n) => [n.id, n.data]));
  assert.deepEqual(data.get('O:Curve'), {
    _type: 'box', ports: PORTS, rowPorts: true, label: 'Curve', variant: 'main', lines: [{ text: 'main contract' }],
    rows: [
      { id: 'Curve|buy', text: 'buy', title: 'buy', icon: 'entry', marks: ['value'], on: true, label: 'Curve.buy, entry point, moves value' },
      { id: 'Curve|_graduate', text: '_graduate', title: '_graduate', icon: undefined, marks: ['value'], on: false, label: 'Curve._graduate, moves value' },
      { id: 'Curve|Payout::pay', text: 'Payout.pay', title: 'Payout::pay', icon: undefined, marks: ['value'], on: false, label: 'Curve.Payout.pay, moves value' },
    ],
    rowAction: true, headAction: 'Curve, main contract', closable: undefined, width: 260,
  });
  assert.deepEqual(data.get('O:Hook'), {
    _type: 'box', ports: PORTS, rowPorts: true, label: 'Hook', variant: 'open', lines: [{ text: '2 links · click to expand' }],
    rows: [], rowAction: true, headAction: 'Expand Hook', closable: 'Close Hook on the board', width: 260,
  });
  assert.deepEqual(data.get('C:DenA@1'), {
    _type: 'box', ports: PORTS, rowPorts: true, label: 'DenA', icon: undefined, variant: '', lines: [{ text: '1 link', tone: undefined }],
    rows: [{ id: 'DenA|deliver', text: 'deliver', title: 'deliver', tone: 'amb', label: 'DenA deliver. Opens DenA here' }],
    rowAction: true, headAction: 'DenA, 1 link. Opens it here', width: 220,
  });
  assert.deepEqual(data.get('C:#libraries@1'), {
    _type: 'box', ports: PORTS, rowPorts: true, label: 'Libraries', icon: 'lib', variant: 'cluster', lines: [{ text: '1 link', tone: undefined }],
    rows: [], rowAction: false, headAction: undefined, width: 220,
  });
  assert.deepEqual(data.get('O:Token')!.rows!.map((r) => [r.id, r.on, r.marks]), [['Token|mint', false, []], ['Token|transfer', false, []]]);
  assert.deepEqual(data.get('C:Router@-1')!.rows!.map((r) => [r.id, r.tone]), [['Router|buyWith', undefined]]);
  assert.deepEqual(
    g.edges.map((e) => [e.id, e.source, e.sourceHandle, e.target, e.targetHandle, e.markerEnd ? 'end' : 'start', (e.data as { chip?: string }).chip ?? null]),
    [
      ['cb:O:Curve|Curve|buy>C:DenA@1|DenA|deliver', 'O:Curve', 'Curve|buy:out', 'C:DenA@1', 'DenA|deliver:in', 'end', 'line 36'],
      ['cb:O:Curve|Curve|buy>C:DenB@1|DenB|deliver', 'O:Curve', 'Curve|buy:out', 'C:DenB@1', 'DenB|deliver:in', 'end', 'line 36'],
      ['cb:O:Curve|Curve|_graduate>O:Hook|', 'O:Curve', 'Curve|_graduate:out', 'O:Hook', 'in', 'end', null],
      ['cb:O:Curve|Curve|buy>O:Token|Token|mint', 'O:Curve', 'Curve|buy:out', 'O:Token', 'Token|mint:in', 'end', 'line 35 · moves value'],
      ['cb:O:Curve|Curve|Payout::pay>O:Token|Token|transfer', 'O:Curve', 'Curve|Payout::pay:out', 'O:Token', 'Token|transfer:in', 'end', null],
      ['cb:C:Router@-1|Router|buyWith>O:Curve|Curve|buy', 'C:Router@-1', 'Router|buyWith:out', 'O:Curve', 'Curve|buy:in', 'end', 'line 14 · moves value'],
      ['cd:O:Curve>O:Hook', 'O:Curve', 'out', 'O:Hook', 'in', 'end', null],
      ['cd:O:Curve>C:#libraries@1', 'O:Curve', 'out', 'C:#libraries@1', 'in', 'end', null],
      ['cd:C:Router@-1>O:Curve', 'C:Router@-1', 'out', 'O:Curve', 'in', 'end', null],
    ],
  );
  const style = new Map(g.edges.map((e) => [e.id, e.style]));
  assert.equal(style.get('cb:O:Curve|Curve|buy>O:Token|Token|mint'), 'stroke: #7aba8a; stroke-width: 2.8; opacity: 1');
  assert.equal(style.get('cb:O:Curve|Curve|Payout::pay>O:Token|Token|transfer'), 'stroke: #7aba8a; stroke-width: 2.2; opacity: 0.45');
  assert.equal(style.get('cd:O:Curve>O:Hook'), 'stroke: var(--color-text-dim); stroke-width: 1.3; opacity: 0.35');
  assert.deepEqual(g.layout.map((e) => [e.source, e.target, e.data?.minlen]), [
    ['O:Curve', 'C:DenA@1', 1], ['O:Curve', 'C:DenB@1', 1], ['O:Curve', 'O:Hook', 1], ['O:Curve', 'O:Token', 1], ['O:Curve', 'O:Token', 1], ['C:Router@-1', 'O:Curve', 1],
    ['O:Curve', 'O:Hook', 1], ['O:Curve', 'C:#libraries@1', 1], ['C:Router@-1', 'O:Curve', 1],
  ]);
});

test('a callee left of its caller is drawn from the callee side with the arrow at the start', () => {
  const B = build(selected(), LINKS, DEFAULT_LINES, () => []);
  B.nodes.find((n) => n.id === 'C:DenA@1')!.col = -2;
  const e = boardGraph(B, selected(), DEFAULT_LINES, FACTS, ORDER).edges[0];
  assert.deepEqual([e.source, e.sourceHandle, e.target, e.targetHandle, !!e.markerStart, !!e.markerEnd], ['C:DenA@1', 'DenA|deliver:out', 'O:Curve', 'Curve|buy:in', true, false]);
});

const hit = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const card = (x: number, y: number, w: number, h: number, pinned = false): Known => ({ x, y, w, h, pinned, at: pinned ? { x: 0, y: 0 } : { x, y } });

function graph(b: Board) {
  const B = build(b, LINKS, DEFAULT_LINES, () => []);
  return { B, g: boardGraph(B, b, DEFAULT_LINES, FACTS, ORDER) };
}

test('place lays a new board out by column and keeps every card apart', () => {
  const { B, g } = graph(selected());
  const at = new Map(place(B, selected(), g, () => null).map((n) => [n.id, { ...n.position, w: n.data.width!, h: 40 }]));
  assert.ok(at.get('C:Router@-1')!.x < at.get('O:Curve')!.x);
  for (const id of ['C:DenA@1', 'C:DenB@1', 'O:Token', 'O:Hook', 'C:#libraries@1']) assert.ok(at.get(id)!.x > at.get('O:Curve')!.x + 260, id);
  const all = [...at.values()];
  all.forEach((a, i) => all.slice(i + 1).forEach((b) => assert.ok(!hit(a, b))));
});

test('place keeps a dragged card, puts new cards beside their owner and below a dragged card in the way', () => {
  const { B, g } = graph(selected());
  const auto = new Map(place(B, selected(), g, () => null).map((n) => [n.id, n.position]));
  const dx = 1000 - auto.get('O:Curve')!.x;
  const dy = 500 - auto.get('O:Curve')!.y;
  const den = auto.get('C:DenA@1')!;
  const blocker = card(den.x + dx, den.y + dy, 260, 80, true);
  const known = (id: string) => (id === 'O:Curve' ? card(1000, 500, 260, 150) : id === 'O:Token' ? blocker : null);
  const at = new Map(place(B, selected(), g, known).map((n) => [n.id, n.position]));
  assert.deepEqual(at.get('O:Curve'), { x: 1000, y: 500 });
  assert.deepEqual(at.get('O:Token'), blocker.at, 'a dragged card keeps its stored position so the canvas keeps the drag');
  const router = at.get('C:Router@-1')!;
  assert.deepEqual([router.x - 1000, router.y - 500], [auto.get('C:Router@-1')!.x - auto.get('O:Curve')!.x, auto.get('C:Router@-1')!.y - auto.get('O:Curve')!.y]);
  assert.equal(at.get('C:DenA@1')!.x, blocker.x);
  assert.ok(at.get('C:DenA@1')!.y >= blocker.y + blocker.h, 'moved below the dragged card');
});

test('an opened box takes the place of its folded card and pushes the undragged cards below it down', () => {
  const b = openStep(readBoard('Curve', new URLSearchParams()), 'Hook', 1, 'Hook|migrate', 'Curve');
  const { B, g } = graph(b);
  const den = card(400, 300, 220, 60);
  const token = card(400, 380, 220, 60, true);
  const known = (id: string) =>
    ({ 'O:Curve': card(0, 0, 260, 300), 'C:Hook@1': card(400, 200, 220, 80), 'C:DenA@1': den, 'C:Token@1': token })[id] ?? null;
  const at = new Map(place(B, b, g, known).map((n) => [n.id, n.position]));
  const hook = at.get('O:Hook')!;
  assert.deepEqual(hook, { x: 400, y: 200 });
  const bottom = hook.y + boxHeight(g.nodes.find((n) => n.id === 'O:Hook')!.data);
  assert.ok(at.get('C:DenA@1')!.y >= bottom, 'pushed below the opened box');
  assert.deepEqual(at.get('C:Token@1'), token.at, 'a dragged card is never pushed');
});

test('a card that grows pushes the undragged cards it now covers down', () => {
  const { B, g } = graph(selected());
  const curve = g.nodes.find((n) => n.id === 'O:Curve')!;
  const known = (id: string) => (id === 'O:Curve' ? card(0, 0, 260, 60) : id === 'O:Token' ? card(0, 100, 220, 60) : id === 'C:DenA@1' ? card(400, 0, 220, 60) : null);
  const at = new Map(place(B, selected(), g, known).map((n) => [n.id, n.position]));
  assert.deepEqual(at.get('O:Curve'), { x: 0, y: 0 });
  assert.ok(at.get('O:Token')!.y >= boxHeight(curve.data), 'below the grown card');
  assert.deepEqual(at.get('C:DenA@1'), { x: 400, y: 0 }, 'a card beside it stays');
});
