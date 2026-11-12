import type { AssetKind, BalanceRead, ValueDirection, ValueFlow, ValueTransfer } from '$lib/api/rest';

/** The running contract stands for a missing sender or owner. */
export const SELF = 'this';

export const DIRECTION_ARROW: Record<ValueDirection, string> = { in: '↘ in', out: '↗ out', other: '↔ between' };

/** `erc20 asset`, or the candidates of an undecided asset. */
export function assetLabel(fact: { asset_kind: AssetKind; asset_candidates: AssetKind[]; token_expr: string | null }): string {
  const kind = fact.asset_kind === 'unknown' && fact.asset_candidates.length ? fact.asset_candidates.join('|') : fact.asset_kind;
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
