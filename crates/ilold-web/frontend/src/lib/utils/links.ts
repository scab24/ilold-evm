import type { Basis, ContractLink, RelationTag } from '$lib/api/rest';

export const LINK_COLOR = '#7ee787';
export const AMBIGUOUS_COLOR = '#f0b72f';

/** Stroke dash per basis: inheritance solid, selector match dashed, self dotted. */
export const BASIS_DASH: Record<Basis, string> = {
  inherits_interface: '',
  selector_match: '6 4',
  self: '2 3',
};

export const BASIS_LABEL: Record<Basis, string> = {
  inherits_interface: 'inherits interface',
  selector_match: 'selector match',
  self: 'this call',
};

export function tagColor(tag: RelationTag): string {
  return tag === 'AMBIGUOUS' ? AMBIGUOUS_COLOR : LINK_COLOR;
}

/** `<contract>::<function key>` split into its parts. */
export function splitRef(ref: string): { contract: string; member: string } {
  const at = ref.lastIndexOf('::');
  return at < 0 ? { contract: '', member: ref } : { contract: ref.slice(0, at), member: ref.slice(at + 2) };
}

export function describeLink(link: ContractLink): string {
  const via = link.via_state_vars.length ? ` · address in ${link.via_state_vars.join(', ')}` : '';
  return `${BASIS_LABEL[link.basis]} · ${link.tag}${via}`;
}
