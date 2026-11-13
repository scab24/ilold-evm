import type { AssetKind, BalanceRead, ValueDirection, ValueFlow, ValueReason, ValueTransfer } from '$lib/api/rest';

/** The running contract stands for a missing sender or owner. */
export const SELF = 'this';

export const DIRECTION_ARROW: Record<ValueDirection, string> = { in: '↘ in', out: '↗ out', other: '↔ between' };

const ASSET_NAME: Record<AssetKind, string> = {
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

export function assetName(fact: { asset_kind: AssetKind; reason?: ValueReason | null }): string {
  return fact.reason === 'currency' ? POOL_ASSET : ASSET_NAME[fact.asset_kind];
}

/** `ERC-20 token`, or the candidates of an undecided asset. */
export function assetLabel(fact: { asset_kind: AssetKind; asset_candidates: AssetKind[]; token_expr: string | null; reason?: ValueReason | null }): string {
  const kind = fact.asset_kind === 'unknown' && fact.asset_candidates.length ? fact.asset_candidates.map((k) => ASSET_NAME[k]).join(' or ') : assetName(fact);
  return fact.token_expr ? `${kind} ${fact.token_expr}` : kind;
}

/** `from → to: amount`, with the token id when there is one. */
export function describeTransfer(t: ValueTransfer): string {
  const amount = [t.amount_expr, t.id_expr ? `#${t.id_expr}` : null].filter(Boolean).join(' ');
  return `${t.operation} ${assetLabel(t)}: ${t.from_expr ?? SELF} → ${t.to_expr ?? '∅'}${amount ? ` (${amount})` : ''}`;
}

export function describeRead(r: BalanceRead): string {
  return `balance ${assetLabel(r)} of ${r.owner_expr ?? SELF}${r.id_expr ? ` #${r.id_expr}` : ''}`;
}

/** One line per transfer, for a tooltip. */
export function describeTransfers(transfers: ValueTransfer[]): string {
  return transfers.map(describeTransfer).join('\n');
}

export function describeFlow(flow: ValueFlow): string {
  return `${DIRECTION_ARROW[flow.direction]} ${assetLabel({ ...flow, asset_candidates: [] })} ${flow.direction === 'in' ? '←' : '→'} ${flow.counterparty_expr ?? '∅'}`;
}
