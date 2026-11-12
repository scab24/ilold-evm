import type { AccessKey, AccessKind, KeyKind, VariableAccess } from '$lib/api/rest';

export const KEY_KIND_LABEL: Record<KeyKind, string> = {
  msg_sender: 'msg.sender',
  parameter: 'param',
  state_var: 'state var',
  local: 'local',
  literal: 'literal',
  this: 'this',
  expression: 'expr',
};

/** Whose slot the outermost key selects. */
export const KEY_SCOPE: Record<KeyKind, string> = {
  msg_sender: 'per caller',
  parameter: 'chosen by an argument',
  state_var: 'shared',
  literal: 'shared',
  this: 'shared',
  local: 'computed',
  expression: 'computed',
};

export interface KeyRow {
  function: string;
  kind: AccessKind;
  keys: AccessKey[];
  line: number | null;
}

export interface KeyGroup {
  label: string;
  scope: string;
  rows: KeyRow[];
}

/** Rows of every site grouped by the kinds of their keys; accesses made in callees form their own group. */
export function groupByKeyKind(accesses: VariableAccess[]): KeyGroup[] {
  const groups = new Map<string, KeyGroup>();
  const add = (label: string, scope: string, row: KeyRow) => {
    if (!groups.has(label)) groups.set(label, { label, scope, rows: [] });
    groups.get(label)!.rows.push(row);
  };
  for (const access of accesses) {
    if (access.keys_reason === 'transitive') {
      add('in a callee', 'keys not tracked', { function: access.function, kind: access.kind, keys: [], line: null });
      continue;
    }
    for (const site of access.key_sites) {
      const row = { function: access.function, kind: access.kind, keys: site.keys, line: site.span.start_line };
      if (site.keys.length === 0) add('no index', 'whole variable', row);
      else add(site.keys.map((k) => KEY_KIND_LABEL[k.kind]).join(' › '), KEY_SCOPE[site.keys[0].kind], row);
    }
  }
  return [...groups.values()].sort((a, b) => a.label.localeCompare(b.label));
}
