import test from 'node:test';
import assert from 'node:assert/strict';
import type { ValueTransfer } from '$lib/api/rest';
import { amountSuffix, describeTransfer } from './value.ts';

const rescue = {
  operation: 'transfer', asset_kind: 'native', asset_candidates: [], token_expr: null, from_expr: null, to_expr: 'to', amount_expr: 'amount', id_expr: null,
} as unknown as ValueTransfer;
const nft = { ...rescue, asset_kind: 'erc721', token_expr: 'nft', amount_expr: null, id_expr: 'tokenId' } as ValueTransfer;

test('the amount of a transfer is its source expression once in brackets, with the token id after #', () => {
  assert.deepEqual([amountSuffix(rescue), amountSuffix(nft), amountSuffix({ ...rescue, amount_expr: null })], [' (amount)', ' (#tokenId)', '']);
  assert.equal(describeTransfer(rescue), 'transfer ETH: this → to (amount)');
  assert.equal(describeTransfer(nft), 'transfer ERC-721 nft: this → to (#tokenId)');
});
