import test from 'node:test';
import assert from 'node:assert/strict';
import type { ProjectConnections } from '$lib/api/rest';
import { lineStyle } from '../../utils/links.ts';
import { LINES, edges, items, linesParam, readLines } from './map-graph.ts';
import connections from '../../api/fixtures/project_connections.json' with { type: 'json' };

const ALL = items([], connections as ProjectConnections, null);
const drawn = (param: string | null) => edges(ALL, (id) => id, readLines(param)).map((e) => [e.id, e.state, e.look, lineStyle(e.look, e.basis)]);
const counted = (param: string | null) =>
  edges(ALL, (id) => id, readLines(param)).map((e) => [e.id, e.look, e.forward, e.back, e.items.map((i) => `${i.line}${i.value ? '+value' : ''} ${i.row?.function}`)]);

const AMBIGUOUS = { color: '#f778ba', width: 1.8, dash: '' };
const VALUE = { color: '#7aba8a', width: 2.2, dash: '' };
const LIBRARY = { color: '#8b949e', width: 1.3, dash: '' };
const CALLS = { color: '#c8d0dc', width: 1.6, dash: '' };
const READS = { color: '#56d4dd', width: 1.3, dash: '' };

test('the Lines legend names ambiguous calls apart from unresolved ones', () => {
  assert.deepEqual(LINES.map((l) => [l.id, l.label]), [
    ['calls', 'Calls'], ['reads', 'Reads'], ['value', 'Value'], ['library', 'Library'], ['inherits', 'Inherits'], ['holds', 'Holds'],
    ['iface', 'Via interface'], ['ambiguous', 'Ambiguous'], ['unresolved', 'Unresolved'],
  ]);
});

test('the map draws ambiguous calls in their own colour, unresolved calls in amber, and the Ambiguous pill filters them', () => {
  assert.deepEqual(drawn(null), [
    ['Curve|DenA|ambiguous', 2, 'ambiguous', AMBIGUOUS],
    ['Curve|DenB|ambiguous', 2, 'ambiguous', AMBIGUOUS],
    ['Curve|Hook|value', 2, 'value', VALUE],
    ['Curve|Hook|reads', 1, 'reads', READS],
    ['Curve|Payout|library', 1, 'library', LIBRARY],
    ['Curve|Token|value', 2, 'value', VALUE],
    ['Hook|PoolMath|library', 1, 'library', LIBRARY],
    ['Curve|Router|value', 2, 'value', VALUE],
    ['Curve|Router|reads', 1, 'reads', READS],
    ['Hook|out:IPoolManager|unresolved', 2, 'unresolved', { color: '#f0b72f', width: 1.8, dash: '' }],
  ]);
  assert.deepEqual(drawn('ambiguous-dim').slice(0, 2), [['Curve|DenA|ambiguous', 1, 'ambiguous', AMBIGUOUS], ['Curve|DenB|ambiguous', 1, 'ambiguous', AMBIGUOUS]]);
  assert.deepEqual(drawn('ambiguous-hide').map(([id]) => id), [
    'Curve|Hook|value', 'Curve|Hook|reads', 'Curve|Payout|library', 'Curve|Token|value', 'Hook|PoolMath|library', 'Curve|Router|value', 'Curve|Router|reads', 'Hook|out:IPoolManager|unresolved',
  ]);
  assert.equal(linesParam(readLines('ambiguous-hide')), 'ambiguous-hide');
});

test('a line between two boxes carries one kind, and its count is the calls of that kind', () => {
  assert.deepEqual(counted(null), [
    ['Curve|DenA|ambiguous', 'ambiguous', 1, 0, ['calls Curve::buy']],
    ['Curve|DenB|ambiguous', 'ambiguous', 1, 0, ['calls Curve::buy']],
    ['Curve|Hook|value', 'value', 1, 0, ['calls+value CurveBase::_graduate']],
    ['Curve|Hook|reads', 'reads', 1, 0, ['reads CurveBase::_graduate']],
    ['Curve|Payout|library', 'library', 1, 0, ['library Curve::buy']],
    ['Curve|Token|value', 'value', 2, 0, ['calls+value Curve::buy', 'calls+value Payout::pay']],
    ['Hook|PoolMath|library', 'library', 1, 0, ['library Hook::fee']],
    ['Curve|Router|value', 'value', 0, 1, ['calls+value Router::buyWith']],
    ['Curve|Router|reads', 'reads', 0, 1, ['reads Router::quote']],
    ['Hook|out:IPoolManager|unresolved', 'unresolved', 2, 0, ['unresolved Hook::migrate', 'unresolved+value Hook::migrate']],
  ]);
  assert.deepEqual(drawn('value-dim').slice(2, 4), [['Curve|Hook|calls', 2, 'calls', CALLS], ['Curve|Hook|reads', 1, 'reads', READS]]);
});
