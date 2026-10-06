<script lang="ts">
  import CodeMirror from 'svelte-codemirror-editor';
  import { javascript } from '@codemirror/lang-javascript';
  import { oneDark } from '@codemirror/theme-one-dark';
  import { EditorView } from '@codemirror/view';
  import DraggablePanel from '$lib/DraggablePanel.svelte';
  import { getFunctionSource, type FunctionSourceResponse } from '$lib/api/rest';

  let { contract, func, onclose }: {
    contract: string;
    func: string;
    onclose: () => void;
  } = $props();

  let source = $state<string>('');
  let filePath = $state<string>('');
  let startLine = $state<number>(0);
  let endLine = $state<number>(0);
  let error = $state<string | null>(null);
  let loading = $state<boolean>(true);

  const STORAGE_KEY = 'ilold.codeViewer.fontSize';
  const DEFAULT_FONT = 13;
  const MIN_FONT = 10;
  const MAX_FONT = 22;

  function readStoredFont(): number {
    if (typeof localStorage === 'undefined') return DEFAULT_FONT;
    const raw = localStorage.getItem(STORAGE_KEY);
    const n = raw ? parseInt(raw, 10) : NaN;
    return Number.isFinite(n) && n >= MIN_FONT && n <= MAX_FONT ? n : DEFAULT_FONT;
  }

  let fontSize = $state<number>(readStoredFont());
  let wrap = $state<boolean>(false);
  let copyState = $state<'idle' | 'copied' | 'error'>('idle');

  const lang = javascript();

  const initialWidth = typeof window !== 'undefined'
    ? Math.min(720, Math.max(420, Math.floor(window.innerWidth * 0.75)))
    : 720;

  $effect(() => {
    const currentContract = contract;
    const currentFunc = func;
    loading = true;
    error = null;
    source = '';
    getFunctionSource(currentContract, currentFunc)
      .then((res: FunctionSourceResponse) => {
        if (currentFunc !== func || currentContract !== contract) return;
        source = res.source;
        filePath = res.file_path;
        startLine = res.span.start_line;
        endLine = res.span.end_line;
        loading = false;
      })
      .catch((e) => {
        if (currentFunc !== func || currentContract !== contract) return;
        error = (e as Error).message ?? String(e);
        loading = false;
      });
  });

  $effect(() => {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(STORAGE_KEY, String(fontSize));
  });

  const fileLabel = $derived(filePath ? filePath.split('/').pop() : '');
  const totalLines = $derived(source ? source.split('\n').length : 0);

  function zoomIn() { fontSize = Math.min(MAX_FONT, fontSize + 1); }
  function zoomOut() { fontSize = Math.max(MIN_FONT, fontSize - 1); }
  function zoomReset() { fontSize = DEFAULT_FONT; }

  async function copy() {
    if (!source) return;
    try {
      await navigator.clipboard.writeText(source);
      copyState = 'copied';
    } catch {
      copyState = 'error';
    }
    setTimeout(() => {
      if (copyState !== 'idle') copyState = 'idle';
    }, 1500);
  }

  function onWindowKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onclose();
      return;
    }
    const mod = e.metaKey || e.ctrlKey;
    if (!mod) return;
    if (e.key === '=' || e.key === '+') {
      e.preventDefault();
      zoomIn();
    } else if (e.key === '-' || e.key === '_') {
      e.preventDefault();
      zoomOut();
    } else if (e.key === '0') {
      e.preventDefault();
      zoomReset();
    }
  }

  const cmStyles = $derived({
    '&': {
      fontSize: `${fontSize}px`,
      fontFamily: 'var(--font-mono), "Cascadia Code", "SF Mono", "Menlo", monospace',
      fontFeatureSettings: '"liga" 1, "calt" 1',
      lineHeight: '1.6',
      height: '100%',
      WebkitFontSmoothing: 'antialiased' as any,
      MozOsxFontSmoothing: 'grayscale' as any,
    },
    '.cm-content': {
      padding: '14px 18px',
      caretColor: 'var(--color-accent)',
    },
    '.cm-gutters': {
      backgroundColor: 'transparent',
      borderRight: '1px solid color-mix(in srgb, var(--color-border) 25%, transparent)',
      color: 'var(--color-text-dim)',
    },
    '.cm-lineNumbers': { padding: '0 6px' },
    '.cm-activeLineGutter': {
      backgroundColor: 'color-mix(in srgb, var(--color-accent) 8%, transparent)',
    },
    '.cm-activeLine': {
      backgroundColor: 'color-mix(in srgb, var(--color-accent) 5%, transparent)',
    },
    '.cm-scroller': { fontFamily: 'inherit' },
  });

  const cmExtensions = $derived(wrap ? [EditorView.lineWrapping] : []);
</script>

<svelte:window onkeydown={onWindowKey} />

<DraggablePanel title={`${func} — ${fileLabel}:${startLine || '?'}`} x={60} y={80} width={initialWidth} {onclose}>
  <div class="viewer">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="tool-group" aria-label="Zoom controls">
        <button class="tool-btn icon" onclick={zoomOut} disabled={fontSize <= MIN_FONT} title="Zoom out (Cmd+-)">−</button>
        <span class="tool-value" title="Current font size">{fontSize}px</span>
        <button class="tool-btn icon" onclick={zoomIn} disabled={fontSize >= MAX_FONT} title="Zoom in (Cmd+=)">+</button>
        <button class="tool-btn icon" onclick={zoomReset} title="Reset zoom (Cmd+0)">⟲</button>
      </div>
      <div class="tool-group">
        <button
          class="tool-btn {wrap ? 'active' : ''}"
          onclick={() => wrap = !wrap}
          title="Toggle word wrap"
          aria-pressed={wrap}
        >↵ Wrap</button>
        <button
          class="tool-btn {copyState === 'copied' ? 'success' : copyState === 'error' ? 'danger' : ''}"
          onclick={copy}
          disabled={!source}
          title="Copy source (Cmd+C after selecting)"
        >
          {#if copyState === 'copied'}✓ Copied{:else if copyState === 'error'}✕ Failed{:else}⎘ Copy{/if}
        </button>
      </div>
    </div>

    <!-- Editor body -->
    <div class="source-container">
      {#if loading}
        <div class="state-msg">Loading source…</div>
      {:else if error}
        <div class="state-msg error">{error}</div>
      {:else}
        <CodeMirror
          value={source}
          {lang}
          theme={oneDark}
          readonly
          extensions={cmExtensions}
          styles={cmStyles}
        />
      {/if}
    </div>

    <!-- Status bar -->
    <div class="status-bar">
      <span class="status-file" title={filePath}>{fileLabel || '—'}</span>
      <span class="status-sep">·</span>
      <span>Lines {startLine || 0}–{endLine || 0}</span>
      <span class="status-sep">·</span>
      <span>{totalLines} line{totalLines === 1 ? '' : 's'}</span>
    </div>
  </div>
</DraggablePanel>

<style>
  .viewer {
    display: flex;
    flex-direction: column;
    height: 440px;
    min-height: 260px;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 7px 10px;
    border-bottom: 1px solid color-mix(in srgb, var(--color-border) 25%, transparent);
    background: linear-gradient(180deg, rgba(30, 30, 40, 0.55) 0%, rgba(22, 22, 28, 0.65) 100%);
    flex-shrink: 0;
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
  }
  .tool-group {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .tool-btn {
    appearance: none;
    background: transparent;
    border: 1px solid transparent;
    color: var(--color-text-muted);
    font-family: inherit;
    font-size: 11px;
    padding: 4px 10px;
    border-radius: 6px;
    cursor: pointer;
    transition: color 120ms ease, background 120ms ease, border-color 120ms ease, transform 80ms ease;
  }
  .tool-btn.icon {
    min-width: 28px;
    padding: 4px 8px;
    font-size: 14px;
    line-height: 1;
    font-weight: 500;
  }
  .tool-btn:hover:not(:disabled) {
    color: var(--color-accent-hover);
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
  }
  .tool-btn:active:not(:disabled) {
    transform: scale(0.96);
  }
  .tool-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .tool-btn.active {
    color: var(--color-accent-light);
    background: color-mix(in srgb, var(--color-accent) 16%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-accent) 50%, transparent);
  }
  .tool-btn.success {
    color: var(--color-success, #5a9a6a);
    background: color-mix(in srgb, var(--color-success, #5a9a6a) 12%, transparent);
  }
  .tool-btn.danger {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
  }
  .tool-value {
    font-size: 10px;
    color: var(--color-text-dim);
    min-width: 34px;
    text-align: center;
    font-family: var(--font-mono, monospace);
    font-variant-numeric: tabular-nums;
  }

  .source-container {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .source-container :global(.cm-editor) {
    height: 100%;
    background: transparent;
  }
  .source-container :global(.cm-editor.cm-focused) {
    outline: none;
  }
  .source-container :global(.cm-scroller) {
    overflow: auto;
  }
  .source-container :global(.cm-scroller::-webkit-scrollbar) {
    width: 10px;
    height: 10px;
  }
  .source-container :global(.cm-scroller::-webkit-scrollbar-thumb) {
    background: color-mix(in srgb, var(--color-border) 60%, transparent);
    border-radius: 10px;
    border: 2px solid transparent;
    background-clip: padding-box;
  }
  .source-container :global(.cm-scroller::-webkit-scrollbar-thumb:hover) {
    background: color-mix(in srgb, var(--color-accent) 50%, transparent);
    background-clip: padding-box;
  }

  .state-msg {
    padding: 20px 18px;
    font-size: 12px;
    color: var(--color-text-muted);
    font-family: var(--font-mono, monospace);
  }
  .state-msg.error { color: var(--color-danger); }

  .status-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 12px;
    font-size: 10px;
    color: var(--color-text-dim);
    font-family: var(--font-mono, monospace);
    font-variant-numeric: tabular-nums;
    border-top: 1px solid color-mix(in srgb, var(--color-border) 25%, transparent);
    background: rgba(12, 12, 18, 0.35);
    flex-shrink: 0;
  }
  .status-file {
    font-weight: 600;
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 40%;
  }
  .status-sep { opacity: 0.4; }
</style>
