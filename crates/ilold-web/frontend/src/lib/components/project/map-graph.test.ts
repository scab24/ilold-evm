import test from 'node:test';
import assert from 'node:assert/strict';
import type { ProjectConnections } from '$lib/api/rest';
import { lineStyle } from '../../utils/links.ts';
import { LINES, edges, items, linesParam, readLines } from './map-graph.ts';
import connections from '../../api/fixtures/project_connections.json' with { type: 'json' };

const ALL = items([], connections as ProjectConnections, null);
const drawn = (param: string | null) => edges(ALL, (id) => id, readLines(param)).map((e) => [e.id, e.state, e.look, lineStyle(e.look, e.basis)]);

const AMBIGUOUS = { color: '#f778ba', width: 1.8, dash: '' };
const VALUE = { color: '#7aba8a', width: 2.2, dash: '' };
const LIBRARY = { color: '#8b949e', width: 1.3, dash: '' };

test('the Lines legend names ambiguous calls apart from unresolved ones', () => {
  assert.deepEqual(LINES.map((l) => [l.id, l.label]), [
    ['calls', 'Calls'], ['reads', 'Reads'], ['value', 'Value'], ['library', 'Library'], ['inherits', 'Inherits'], ['holds', 'Holds'],
    ['iface', 'Via interface'], ['ambiguous', 'Ambiguous'], ['unresolved', 'Unresolved'],
  ]);
});

test('the map draws ambiguous calls in their own colour, unresolved calls in amber, and the Ambiguous pill filters them', () => {
  assert.deepEqual(drawn(null), [
    ['Curve|DenA', 2, 'ambiguous', AMBIGUOUS],
    ['Curve|DenB', 2, 'ambiguous', AMBIGUOUS],
    ['Curve|Hook', 2, 'value', VALUE],
    ['Curve|Payout', 1, 'library', LIBRARY],
    ['Curve|Token', 2, 'value', VALUE],
    ['Hook|PoolMath', 1, 'library', LIBRARY],
    ['Curve|Router', 2, 'value', VALUE],
    ['Hook|out:IPoolManager', 2, 'unresolved', { color: '#f0b72f', width: 1.8, dash: '' }],
  ]);
  assert.deepEqual(drawn('ambiguous-dim').slice(0, 2), [['Curve|DenA', 1, 'ambiguous', AMBIGUOUS], ['Curve|DenB', 1, 'ambiguous', AMBIGUOUS]]);
  assert.deepEqual(drawn('ambiguous-hide').map(([id]) => id), ['Curve|Hook', 'Curve|Payout', 'Curve|Token', 'Hook|PoolMath', 'Curve|Router', 'Hook|out:IPoolManager']);
  assert.equal(linesParam(readLines('ambiguous-hide')), 'ambiguous-hide');
});
