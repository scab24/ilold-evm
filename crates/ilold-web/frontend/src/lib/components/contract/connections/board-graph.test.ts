import test from 'node:test';
import assert from 'node:assert/strict';
import type { Node } from '@xyflow/svelte';
import type { Connections, ContractCall } from '$lib/api/rest';
import type { BoxNodeData } from '../../project/BoxNode.svelte';
import { DEFAULT_LINES } from '../../project/map-graph.ts';
import { boxHeight } from '../../project/box-size.ts';
import { build, linksOf, type Link } from './board-links.ts';
import { boardGraph, place, shapeOf, type Known } from './board-graph.ts';
import curve from '../../../api/fixtures/connections.json' with { type: 'json' };
import hook from '../../../api/fixtures/connections_hook.json' with { type: 'json' };
import { closeBox, openStep, readBoard, touch, type Board } from './board-state.ts';

const LIBS = new Set(['Payout', 'PoolMath']);
const LINKS = linksOf([curve, hook] as Connections[], LIBS);
const FACTS = () => undefined;
const ORDER = (c: string) => (c === 'Curve' ? ['buy', '_graduate'] : []);
const START = readBoard('Curve', new URLSearchParams());

type Placed = Node<BoxNodeData>[];

function laid(board: Board, known: (id: string) => Known | null = () => null, links: Link[] = LINKS) {
  const built = build(board, links, DEFAULT_LINES, () => []);
  const graph = boardGraph(built, board, DEFAULT_LINES, FACTS, ORDER);
  return { graph, nodes: place(built, board, graph, known) };
}

function knownFrom(nodes: Placed, dragged: Record<string, { x: number; y: number }> = {}) {
  return (id: string): Known | null => {
    const n = nodes.find((x) => x.id === id);
    if (!n) return null;
    const live = dragged[id] ?? n.position;
    return { ...live, w: n.data.width!, h: boxHeight(n.data), pinned: !!dragged[id], at: n.position };
  };
}

const positions = (nodes: Placed) => Object.fromEntries(nodes.map((n) => [n.id, n.position]));
const column = (nodes: Placed, x: number) => nodes.filter((n) => n.position.x === x).sort((a, b) => a.position.y - b.position.y).map((n) => n.id);

test('a dragged card keeps its place while a box opens and closes, and the closed box folds back where it was open', () => {
  const first = laid(START).nodes;
  const dragged = { 'C:Token@1': { x: 900, y: 600 } };
  const opened = openStep(START, 'Hook', 1, 'Hook|migrate', 'Curve');
  const afterOpen = laid(opened, knownFrom(first, dragged)).nodes;
  assert.deepEqual(positions(afterOpen)['O:Hook'], positions(first)['C:Hook@1']);
  assert.deepEqual(positions(afterOpen)['C:Token@1'], positions(first)['C:Token@1']);
  const hookDragged = { ...dragged, 'O:Hook': { x: 700, y: 900 } };
  const afterClose = laid(closeBox(opened, 'Hook'), knownFrom(afterOpen, hookDragged)).nodes;
  const before = positions(afterOpen);
  assert.deepEqual(positions(afterClose), {
    'O:Curve': before['O:Curve'],
    'C:DenA@1': before['C:DenA@1'],
    'C:DenB@1': before['C:DenB@1'],
    'C:Hook@1': hookDragged['O:Hook'],
    'C:Token@1': before['C:Token@1'],
    'C:#libraries@1': before['C:#libraries@1'],
    'C:Router@-1': before['C:Router@-1'],
  });
});

test('an older box folds in place at its exact position and the box opened after it takes its folded card place', () => {
  const one = openStep(START, 'Hook', 1, 'Hook|migrate', 'Curve');
  const before = laid(one).nodes;
  const older = touch(openStep(one, 'Token', 1, null, 'Curve'), 'Curve');
  const after = laid(older, knownFrom(before)).nodes;
  const was = positions(before);
  assert.deepEqual(positions(after), {
    'O:Curve': was['O:Curve'],
    'O:Token': was['C:Token@1'],
    'O:Hook': was['O:Hook'],
    'C:DenA@1': was['C:DenA@1'],
    'C:DenB@1': was['C:DenB@1'],
    'C:#libraries@1': was['C:#libraries@1'],
    'C:Router@-1': was['C:Router@-1'],
  });
});

const card = (x: number, y: number, w: number, h: number): Known => ({ x, y, w, h, pinned: false, at: { x, y } });

test('two boxes opened in the same column each take their folded card place, the second below the first once it grows', () => {
  const grid: Record<string, Known> = {
    'O:Curve': card(0, 0, 260, 140),
    'C:Token@1': card(400, 0, 220, 60),
    'C:Hook@1': card(400, 80, 220, 60),
    'C:DenA@1': card(400, 160, 220, 60),
    'C:DenB@1': card(400, 240, 220, 60),
    'C:#libraries@1': card(400, 320, 220, 60),
    'C:Router@-1': card(-400, 0, 220, 60),
  };
  const token = openStep(START, 'Token', 1, null, 'Curve');
  const first = laid(token, (id) => grid[id] ?? null).nodes;
  const both = openStep(token, 'Hook', 1, 'Hook|migrate', 'Curve');
  const second = laid(both, knownFrom(first));
  const height = (id: string) => boxHeight(second.graph.nodes.find((n) => n.id === id)!.data);
  const at = positions(second.nodes);
  assert.deepEqual(
    Object.fromEntries(Object.entries(at).filter(([id]) => !id.endsWith('@2'))),
    {
      'O:Curve': { x: 0, y: 0 },
      'O:Token': { x: 400, y: 0 },
      'O:Hook': { x: 400, y: Math.max(80, height('O:Token') + 16) },
    },
  );
  assert.deepEqual(Object.keys(at).filter((id) => id.endsWith('@2')).sort(), ['C:#libraries@2', 'C:#outside@2']);
  assert.deepEqual(column(second.nodes, at['C:#outside@2'].x).at(-1), 'C:#outside@2', 'outside the project sits at the end of its column');
});

function back(): Connections[] {
  const row: ContractCall = { ...curve.incoming[0].calls[0], in_context: 'Hook', function: 'Hook::migrate', target: 'ICurve::buy', span: { ...curve.incoming[0].calls[0].span, start_line: 50 } } as ContractCall;
  const c = structuredClone(curve) as Connections;
  const h = structuredClone(hook) as Connections;
  c.incoming.push({ ...c.incoming[0], contract: 'Hook', calls: [row] });
  h.outgoing.push({ ...h.outgoing[0], contract: 'Curve', calls: [row] });
  return [c, h];
}

test('a cycle between two open boxes draws both directions between them and adds no folded copy of either', () => {
  const links = linksOf(back(), LIBS);
  const board = openStep(START, 'Hook', 1, 'Hook|migrate', 'Curve');
  const { graph } = laid(board, () => null, links);
  assert.deepEqual(graph.nodes.map((n) => n.id).sort(), ['C:#libraries@1', 'C:#libraries@2', 'C:#outside@2', 'C:DenA@1', 'C:DenB@1', 'C:Router@-1', 'C:Token@1', 'O:Curve', 'O:Hook']);
  assert.deepEqual(
    graph.edges.filter((e) => e.source === 'O:Hook' || e.target === 'O:Hook').map((e) => [e.id, e.source, e.sourceHandle, e.target, e.targetHandle, e.markerEnd ? 'end' : 'start']),
    [
      ['cb:O:Curve|Curve|_graduate>O:Hook|Hook|migrate', 'O:Curve', 'Curve|_graduate:out', 'O:Hook', 'Hook|migrate:in', 'end'],
      ['cb:O:Hook|Hook|migrate>O:Curve|Curve|buy', 'O:Curve', 'Curve|buy:out', 'O:Hook', 'Hook|migrate:in', 'start'],
      ['cb:O:Hook|Hook|migrate>C:#outside@2|#outside|IPoolManager::unlock', 'O:Hook', 'Hook|migrate:out', 'C:#outside@2', '#outside|IPoolManager::unlock:in', 'end'],
      ['cb:O:Hook|Hook|migrate>C:#outside@2|#outside|IPoolManager::settle', 'O:Hook', 'Hook|migrate:out', 'C:#outside@2', '#outside|IPoolManager::settle:in', 'end'],
      ['cd:O:Curve>O:Hook', 'O:Curve', 'out', 'O:Hook', 'in', 'end'],
      ['cd:O:Hook>C:#libraries@2', 'O:Hook', 'out', 'C:#libraries@2', 'in', 'end'],
    ],
  );
});

test('selecting a function keeps the shape of the board, so every card keeps its exact position', () => {
  const plain = laid(START);
  const picked = { ...START, fn: 'Curve|buy' };
  const again = laid(picked, knownFrom(plain.nodes));
  assert.equal(shapeOf(again.graph), shapeOf(plain.graph));
  assert.deepEqual(positions(again.nodes), positions(plain.nodes));
  assert.ok(shapeOf(laid(openStep(START, 'Hook', 1, null, 'Curve')).graph) !== shapeOf(plain.graph), 'opening a box changes the shape');
});

test('a new board stacks each column in the order of the rows its cards link to', () => {
  const { nodes } = laid(START);
  const x = positions(nodes)['C:DenA@1'].x;
  assert.deepEqual(column(nodes, x), ['C:DenA@1', 'C:DenB@1', 'C:#libraries@1', 'C:Hook@1', 'C:Token@1']);
});
