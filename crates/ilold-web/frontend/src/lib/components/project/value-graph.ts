import type { AssetKind, Party, ValueFlowPairs, ValuePair, ValueReason, ValueTransfer } from '$lib/api/rest';

export const ASSET_NAME: Record<AssetKind, string> = {
  native: 'ETH',
  erc20: 'ERC-20',
  erc721: 'ERC-721',
  erc1155: 'ERC-1155',
  erc6909: 'ERC-6909',
  unknown: 'unknown asset',
};
export const POOL_ASSET = 'ETH or token';
export const POOL_ASSET_LONG = 'ETH or token, set by the pool';
export const ASSET_ORDER = ['ETH', 'ERC-20', 'ERC-721', 'ERC-1155', 'ERC-6909', POOL_ASSET, 'unknown asset'];

export function assetName(fact: { asset_kind: AssetKind; reason: ValueReason | null }): string {
  return fact.reason === 'currency' ? POOL_ASSET : ASSET_NAME[fact.asset_kind];
}

export type PartyKind = 'contract' | 'declared' | 'users';

export interface FlowParty {
  key: string;
  label: string;
  kind: PartyKind;
  column: number;
  pays: number;
  receives: number;
  expressions: string[];
}

export interface FlowArrow {
  id: string;
  from: string;
  to: string;
  pool: boolean;
  assets: string[];
  transfers: ValueTransfer[];
}

export interface Flow {
  parties: FlowParty[];
  arrows: FlowArrow[];
  transfers: ValueTransfer[];
}

function partyKey(party: Party, side: 'from' | 'to'): string {
  if (party.kind === 'contract') return `c:${party.name.join(' | ')}`;
  if (party.kind === 'declared') return `d:${party.name}`;
  return side === 'from' ? 'u:in' : 'u:out';
}

function partyKind(party: Party): PartyKind {
  return party.kind === 'expression' ? 'users' : party.kind;
}

function partyLabel(party: Party): string {
  if (party.kind === 'contract') return party.name.join(' or ');
  if (party.kind === 'declared') return party.name;
  return 'Users and addresses';
}

export function transferKey(t: { in_context: string; id: string }): string {
  return `${t.in_context}|${t.id}`;
}

export function assetCounts(data: ValueFlowPairs): Map<string, number> {
  const seen = new Map<string, string>();
  for (const pair of data.pairs) for (const id of pair.transfers) seen.set(`${pair.in_context}|${id}`, assetName(pair));
  const counts = new Map<string, number>();
  for (const asset of seen.values()) counts.set(asset, (counts.get(asset) ?? 0) + 1);
  return counts;
}

export function buildFlow(data: ValueFlowPairs, asset: string | null): Flow {
  const byKey = new Map(data.transfers.map((t) => [transferKey(t), t]));
  const parties = new Map<string, FlowParty>();
  const arrows = new Map<string, FlowArrow>();
  const moved = new Map<string, ValueTransfer>();

  const party = (p: Party, side: 'from' | 'to'): FlowParty => {
    const key = partyKey(p, side);
    let found = parties.get(key);
    if (!found) {
      found = { key, label: partyLabel(p), kind: partyKind(p), column: 0, pays: 0, receives: 0, expressions: [] };
      parties.set(key, found);
    }
    if (p.kind === 'expression' && p.name && !found.expressions.includes(p.name)) found.expressions.push(p.name);
    return found;
  };

  for (const pair of data.pairs.filter((p: ValuePair) => asset === null || assetName(p) === asset)) {
    const from = party(pair.from, 'from');
    const to = party(pair.to, 'to');
    const pool = pair.reason === 'currency';
    const id = `${from.key}>${to.key}${pool ? '~pool' : ''}`;
    let arrow = arrows.get(id);
    if (!arrow) {
      arrow = { id, from: from.key, to: to.key, pool, assets: [], transfers: [] };
      arrows.set(id, arrow);
    }
    const name = assetName(pair);
    if (!arrow.assets.includes(name)) arrow.assets.push(name);
    for (const ref of pair.transfers) {
      const t = byKey.get(`${pair.in_context}|${ref}`);
      if (!t || arrow.transfers.includes(t)) continue;
      arrow.transfers.push(t);
      moved.set(transferKey(t), t);
      from.pays += 1;
      to.receives += 1;
    }
  }

  for (const arrow of arrows.values()) arrow.assets.sort((a, b) => ASSET_ORDER.indexOf(a) - ASSET_ORDER.indexOf(b));
  const paidByContract = new Set([...arrows.values()].filter((a) => a.from.startsWith('c:')).map((a) => a.to));
  for (const p of parties.values()) {
    if (p.key === 'u:in') p.column = 0;
    else if (p.key === 'u:out') p.column = 3;
    else if (p.kind === 'declared') p.column = p.receives > 0 ? 3 : 0;
    else p.column = paidByContract.has(p.key) ? 2 : 1;
  }

  return { parties: [...parties.values()], arrows: [...arrows.values()], transfers: [...moved.values()] };
}
