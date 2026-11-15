export interface SelectionKeys {
  selected: boolean;
  clear: () => void;
  open: (() => void) | null;
}

export function selectionKeys(current: () => SelectionKeys): (event: KeyboardEvent) => void {
  return (event) => {
    const t = event.target instanceof Element ? event.target : null;
    if (t?.closest('input, select, textarea')) return;
    const s = current();
    if (event.key === 'Escape' && s.selected) s.clear();
    else if (event.key === 'Enter' && s.open && !t?.closest('button, a, [role="button"]')) s.open();
  };
}
