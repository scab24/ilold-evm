import { getFunctionSource } from '$lib/api/rest';
import { notifyError } from '$lib/components/ui/notifications.svelte';
import { splitRef } from '$lib/utils/links';

export function buildIdeLink(absPath: string, line: number, col?: number): string {
  const encoded = encodeURI(absPath);
  const loc = col && col > 0 ? `${line}:${col}` : `${line}`;
  return `vscode://file${encoded.startsWith('/') ? '' : '/'}${encoded}:${loc}`;
}

export function openInIde(absPath: string, line: number, col?: number): void {
  window.open(buildIdeLink(absPath, line, col));
}

export async function openFunctionLine(ref: string, line: number): Promise<void> {
  const { contract, member } = splitRef(ref);
  try {
    openInIde((await getFunctionSource(contract, member)).file_path, line);
  } catch (e) {
    notifyError('open source', e);
  }
}
