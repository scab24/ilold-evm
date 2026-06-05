
export function buildIdeLink(absPath: string, line: number, col?: number): string {
  const encoded = encodeURI(absPath);
  const loc = col && col > 0 ? `${line}:${col}` : `${line}`;
  return `vscode://file${encoded.startsWith('/') ? '' : '/'}${encoded}:${loc}`;
}

export function openInIde(absPath: string, line: number, col?: number): void {
  window.open(buildIdeLink(absPath, line, col));
}
