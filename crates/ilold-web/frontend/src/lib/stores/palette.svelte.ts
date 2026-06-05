
import type { Command } from '$lib/commands/registry';

let _open = $state(false);
let _commands = $state<Command[]>([]);

export function isPaletteOpen(): boolean { return _open; }
export function openPalette() { _open = true; }
export function closePalette() { _open = false; }
export function togglePalette() { _open = !_open; }

export function getPaletteCommands(): Command[] { return _commands; }

export function setPaletteCommands(cmds: Command[]) { _commands = cmds; }

export function clearPaletteCommands() { _commands = []; }
