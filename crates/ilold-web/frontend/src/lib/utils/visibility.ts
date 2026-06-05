
export function visibilityLabel(visibility: string | undefined): string | null {
  switch (visibility) {
    case 'Public': return 'pub';
    case 'External': return 'ext';
    case 'Internal': return 'int';
    case 'Private': return 'priv';
    default: return null;
  }
}

export function visibilityClass(visibility: string | undefined): string {
  switch (visibility) {
    case 'Public': return 'bg-accent-dark/30 text-accent-hover';
    case 'External': return 'bg-warning/20 text-warning';
    case 'Internal':
    case 'Private': return 'bg-border text-text-muted';
    default: return '';
  }
}
