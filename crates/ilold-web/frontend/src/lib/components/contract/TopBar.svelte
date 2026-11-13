<script lang="ts">
  import { toggleTerminal, isTerminalVisible } from '$lib/stores/terminal.svelte';
  import type { Mode } from '$lib/components/chrome/AppHeader.svelte';
  import Seg from '$lib/components/chrome/Seg.svelte';
  import BoardToolbar from './connections/BoardToolbar.svelte';

  let {
    contract,
    mode,
    seqDirection,
    oncenter,
    onseqdirection,
    onsessionback,
    onsessionclear,
  }: {
    contract: string;
    mode: Mode;
    seqDirection: 'TB' | 'LR';
    oncenter: () => void;
    onseqdirection: (dir: 'TB' | 'LR') => void;
    onsessionback: () => void;
    onsessionclear: () => void;
  } = $props();

  const DIRECTIONS: { id: 'TB' | 'LR'; label: string; title: string }[] = [
    { id: 'TB', label: '↓ Down', title: 'Vertical layout' },
    { id: 'LR', label: '→ Right', title: 'Horizontal layout' },
  ];
</script>

<div class="toolbar" role="toolbar" aria-label="Canvas">
  {#if mode === 'connections'}
    <BoardToolbar main={contract} />
  {/if}
  {#if mode === 'sequences'}
    <span class="tlabel">Layout</span>
    <Seg label="Sequence tree direction" options={DIRECTIONS} value={seqDirection} onpick={onseqdirection} />
  {/if}
  {#if mode === 'session'}
    <span class="tlabel">Active scenario</span>
    <button type="button" class="btn" onclick={onsessionback} title="Remove the last step of the active scenario">↶ Back</button>
    <button type="button" class="btn" onclick={onsessionclear} title="Remove every step of the active scenario">Clear</button>
  {/if}
  <span class="tools-end">
    <button type="button" class="btn" onclick={oncenter} title="Fit every node in view">Center</button>
    <button type="button" class="btn mono" onclick={toggleTerminal} aria-pressed={isTerminalVisible()} title={isTerminalVisible() ? 'Hide terminal' : 'Show terminal'}>&gt;_ Terminal</button>
  </span>
</div>
