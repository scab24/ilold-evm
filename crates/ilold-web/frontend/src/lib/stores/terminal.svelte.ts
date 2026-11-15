let visible = $state(false);

export function isTerminalVisible(): boolean {
  return visible;
}

export function showTerminal(): void {
  visible = true;
}

export function hideTerminal(): void {
  visible = false;
}

export function toggleTerminal(): void {
  visible = !visible;
}
