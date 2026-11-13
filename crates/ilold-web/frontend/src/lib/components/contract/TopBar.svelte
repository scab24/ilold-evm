<script lang="ts">
  import { toggleTerminal, isTerminalVisible } from '$lib/stores/terminal.svelte';
  import type { Mode } from '$lib/components/chrome/AppHeader.svelte';
  import Seg from '$lib/components/chrome/Seg.svelte';
  import BoardToolbar from './connections/BoardToolbar.svelte';
  import { applyBoard, currentBoard } from './connections/board-view.svelte';
  import type { Layout } from './connections/board-state';

  let {
    contract,
    mode,
    seqDirection,
    boardEmpty,
    oncenter,
    onseqdirection,
    onsessionback,
    onsessionclear,
  }: {
    contract: string;
    mode: Mode;
    seqDirection: Layout;
    boardEmpty: boolean;
    oncenter: () => void;
    onseqdirection: (dir: Layout) => void;
    onsessionback: () => void;
    onsessionclear: () => void;
  } = $props();

  const DIRECTIONS: { id: Layout; label: string; title: string }[] = [
    { id: 'TB', label: '↓ Down', title: 'Vertical layout' },
    { id: 'LR', label: '→ Right', title: 'Horizontal layout' },
  ];
  const board = $derived(mode === 'connections' ? currentBoard(contract) : null);
</script>

<div class="toolbar" role="toolbar" aria-label="Canvas">
  {#if mode === 'connections'}
    <BoardToolbar main={contract} empty={boardEmpty} />
  {/if}
  {#if board}
    <span class="tlabel">Layout</span>
    <Seg label="Board layout" options={DIRECTIONS.map((d) => ({ ...d, disabled: boardEmpty }))} value={board.layout} onpick={(layout) => applyBoard({ ...board, layout })} />
  {:else if mode === 'sequences'}
    <span class="tlabel">Layout</span>
    <Seg label="Sequence tree direction" options={DIRECTIONS} value={seqDirection} onpick={onseqdirection} />
  {/if}
  {#if mode === 'session'}
    <span class="tlabel">Active scenario</span>
    <button type="button" class="btn" onclick={onsessionback} title="Remove the last step of the active scenario">↶ Back</button>
    <button type="button" class="btn" onclick={onsessionclear} title="Remove every step of the active scenario">Clear</button>
  {/if}
  <span class="tools-end">
    <button type="button" class="btn" onclick={oncenter} disabled={boardEmpty} title={boardEmpty ? 'Nothing on the board to fit' : 'Fit every node in view'}>Center</button>
    <button type="button" class="btn mono" onclick={toggleTerminal} aria-pressed={isTerminalVisible()} title={isTerminalVisible() ? 'Hide terminal' : 'Show terminal'}>&gt;_ Terminal</button>
  </span>
</div>
