import test from 'node:test';
import assert from 'node:assert/strict';
import { boardQuery, closeBox, collapsed, expanded, openStep, popStep, readBoard, reset, touch, type Board } from './board-state.ts';

const MAIN = 'Curve';
const HOOK = 'src/hooks/Hook.sol:Hook';
const DEN = 'Den';
const TOKEN = 'Token';

function fresh(main = MAIN): Board {
  return readBoard(main, new URLSearchParams());
}

function query(b: Board): URLSearchParams {
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(boardQuery(b))) if (v !== null) params.set(k, v);
  return params;
}

test('a board survives the URL round trip with qualified ids and overloads', () => {
  let b: Board = { ...fresh(), fn: `${MAIN}|buy(uint256)` };
  b = openStep(b, HOOK, 1, `${HOOK}|swap(uint256,address)`, MAIN);
  b = openStep(b, DEN, 2, `${DEN}|swap(uint256)`, HOOK);
  b = openStep(b, TOKEN, -1, null, MAIN);
  b = { ...b, dir: 'callees', all: true };
  assert.deepEqual(readBoard(MAIN, query(b)), b);
  assert.deepEqual(b.parent, { [HOOK]: MAIN, [DEN]: HOOK, [TOKEN]: MAIN });
  assert.deepEqual(b.trail.map((t) => t.to ?? t.opened), [`${HOOK}|swap(uint256,address)`, `${DEN}|swap(uint256)`, TOKEN]);
});

test('readBoard drops junk from the URL', () => {
  const params = new URLSearchParams({
    open: `${MAIN}>${HOOK}:1,${MAIN}>${MAIN}:1,${MAIN}>Bad:x,Ghost>Lost:2,${HOOK}>Skip:1,${MAIN}>Zero:0,${MAIN}>${HOOK}:-1`,
    trail: `Nowhere|f()~${HOOK}|g()`,
    start: 'Nowhere|f()',
    fn: 'Ghost|h()',
    recent: `Ghost,${HOOK}`,
    dir: 'sideways',
    all: 'yes',
  });
  assert.deepEqual(readBoard(MAIN, params), {
    main: MAIN,
    open: [MAIN, HOOK],
    col: { [MAIN]: 0, [HOOK]: 1 },
    parent: { [HOOK]: MAIN },
    trail: [{ opened: HOOK, to: `${HOOK}|g()` }],
    start: null,
    recent: [MAIN, HOOK],
    fn: null,
    dir: 'both',
    all: false,
  });
});

test('openStep opens a box once, keeps the first selection as start and selects the step', () => {
  const sel = `${MAIN}|buy(uint256)`;
  const one = openStep({ ...fresh(), fn: sel }, HOOK, 1, `${HOOK}|swap(uint256,address)`, MAIN);
  assert.deepEqual(one, {
    ...fresh(),
    open: [MAIN, HOOK],
    col: { [MAIN]: 0, [HOOK]: 1 },
    parent: { [HOOK]: MAIN },
    trail: [{ opened: HOOK, to: `${HOOK}|swap(uint256,address)` }],
    start: sel,
    recent: [MAIN, HOOK],
    fn: `${HOOK}|swap(uint256,address)`,
  });
  const again = openStep(one, HOOK, 1, null, MAIN);
  assert.deepEqual(again.open, one.open);
  assert.deepEqual(again.trail.length, 2);
  assert.equal(again.start, sel);
  assert.equal(again.fn, one.fn);
});

test('only the last two used boxes stay expanded and touch changes which', () => {
  let b = openStep(fresh(), HOOK, 1, null, MAIN);
  b = openStep(b, DEN, 2, null, HOOK);
  assert.deepEqual([...expanded(b)], [HOOK, DEN]);
  assert.deepEqual(collapsed(b), [MAIN]);
  b = touch(b, MAIN);
  assert.deepEqual([...expanded(b)], [DEN, MAIN]);
  assert.deepEqual(collapsed(b), [HOOK]);
  assert.deepEqual(b.recent, [HOOK, DEN, MAIN]);
});

test('popStep undoes the last step and keeps a box another step still uses', () => {
  const a = `${HOOK}|swap(uint256,address)`;
  const bKey = `${DEN}|swap(uint256)`;
  let b: Board = { ...fresh(), fn: `${MAIN}|buy(uint256)` };
  b = openStep(b, HOOK, 1, a, MAIN);
  b = openStep(b, DEN, 2, bKey, HOOK);
  b = openStep(b, HOOK, 1, `${HOOK}|swap(uint256)`, MAIN);
  const p1 = popStep(b)!;
  assert.deepEqual(p1.open, [MAIN, HOOK, DEN]);
  assert.equal(p1.fn, bKey);
  const p2 = popStep(p1)!;
  assert.deepEqual(p2.open, [MAIN, HOOK]);
  assert.deepEqual(p2.parent, { [HOOK]: MAIN });
  assert.equal(p2.fn, a);
  const p3 = popStep(p2)!;
  assert.deepEqual(p3, { ...fresh(), fn: `${MAIN}|buy(uint256)` });
  assert.equal(popStep(p3), null);
});

test('closeBox removes the box and what it opened, and keeps a sibling at the same column', () => {
  const sel = `${MAIN}|buy(uint256)`;
  let b: Board = { ...fresh(), fn: sel };
  b = openStep(b, HOOK, 1, `${HOOK}|swap(uint256,address)`, MAIN);
  b = openStep(b, DEN, 2, `${DEN}|swap(uint256)`, HOOK);
  b = openStep(b, TOKEN, 1, `${TOKEN}|mint()`, MAIN);
  const closed = closeBox(b, HOOK);
  assert.deepEqual(closed, {
    ...fresh(),
    open: [MAIN, TOKEN],
    col: { [MAIN]: 0, [TOKEN]: 1 },
    parent: { [TOKEN]: MAIN },
    trail: [{ opened: TOKEN, to: `${TOKEN}|mint()` }],
    start: sel,
    recent: [MAIN, TOKEN],
    fn: `${TOKEN}|mint()`,
  });
  const last = closeBox(closed, TOKEN);
  assert.deepEqual(last, { ...fresh(), fn: sel });
  assert.equal(closeBox(b, MAIN), b);
});

test('reset keeps the main contract, direction and all functions', () => {
  let b: Board = { ...fresh(), dir: 'callers', all: true, fn: `${MAIN}|buy(uint256)` };
  b = openStep(b, HOOK, 1, null, MAIN);
  assert.deepEqual(reset(b), { ...fresh(), dir: 'callers', all: true });
});
