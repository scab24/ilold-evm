import type { Basis, RelationTag } from '$lib/api/rest';

export type Relation = 'calls' | 'reads' | 'library' | 'value' | 'inherits' | 'holds' | 'unresolved';

export const AMBIGUOUS_COLOR = '#f0b72f';

export const RELATION_STYLE: Record<Relation, { color: string; width: number }> = {
  calls: { color: '#c8d0dc', width: 1.6 },
  reads: { color: '#56d4dd', width: 1.3 },
  library: { color: '#8b949e', width: 1.3 },
  value: { color: '#7aba8a', width: 2.2 },
  inherits: { color: '#d2a8ff', width: 1.3 },
  holds: { color: '#d4956b', width: 1.3 },
  unresolved: { color: AMBIGUOUS_COLOR, width: 1.8 },
};

export const BASIS_DASH: Record<Basis, string> = {
  inherits_interface: '',
  library: '',
  selector_match: '6 4',
  self: '2 3',
};

export const BASIS_LABEL: Record<Basis, string> = {
  inherits_interface: 'via interface',
  selector_match: 'selector match',
  self: 'this call',
  library: 'library',
};

export interface LineStyle { color: string; width: number; dash: string }

export function lineStyle(relation: Relation, basis: Basis | null = null, tag: RelationTag | null = null): LineStyle {
  const { color, width } = RELATION_STYLE[relation];
  return { color: tag === 'AMBIGUOUS' ? AMBIGUOUS_COLOR : color, width, dash: basis ? BASIS_DASH[basis] : '' };
}

export function tagColor(tag: RelationTag): string {
  return lineStyle('calls', null, tag).color;
}

/** `<contract>::<function key>` split into its parts. */
export function splitRef(ref: string): { contract: string; member: string } {
  const at = ref.lastIndexOf('::');
  return at < 0 ? { contract: '', member: ref } : { contract: ref.slice(0, at), member: ref.slice(at + 2) };
}
