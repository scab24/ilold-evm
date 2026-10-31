<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import '@xterm/xterm/css/xterm.css';
  import { isTerminalVisible, hideTerminal } from '$lib/stores/terminal.svelte';
  import { contractQuery } from '$lib/api/session';

  interface Props {
    contract: string;
  }

  let { contract }: Props = $props();

  let terminalEl: HTMLDivElement;
  let terminal: Terminal;
  let fitAddon: FitAddon;
  let ws: WebSocket | null = null;
  let wsContract: string | null = null;
  let resizeObserver: ResizeObserver | null = null;

  let maximized = $state(false);
  let connected = $state(false);
  let fadeIn = $state(false);
  let termCols = $state(0);
  let termRows = $state(0);
  let ready = $state(false);

  $effect(() => {
    if (isTerminalVisible() && fitAddon) {
      requestAnimationFrame(() => fitAddon?.fit());
    }
  });

  // the PTY starts when first shown and stays alive while hidden, until the page ends
  $effect(() => {
    if (ready && isTerminalVisible() && !ws) connect();
  });

  // the page can switch contracts without remounting
  $effect(() => {
    const current = contract;
    untrack(() => {
      if (!ws || wsContract === current) return;
      disconnect();
      terminal?.reset();
      if (isTerminalVisible()) connect();
    });
  });

  /** The server then ends the attached REPL. */
  function disconnect() {
    const socket = ws;
    ws = null;
    wsContract = null;
    connected = false;
    if (socket) {
      socket.onclose = null;
      socket.onmessage = null;
      socket.close();
    }
  }

  function onPageHide() {
    disconnect();
  }

  function onPageShow(e: PageTransitionEvent) {
    if (e.persisted && ready && isTerminalVisible() && !ws) connect();
  }

  let prevPosX = 0;
  let prevPosY = 0;
  let prevWidth = 600;
  let prevHeight = 300;

  let posX = $state(0);
  let posY = $state(0);

  let panelWidth = $state(600);
  let panelHeight = $state(300);
  const MIN_W = 400;
  const MIN_H = 200;
  const TITLE_BAR_H = 36;

  let dragging = $state(false);

  function onDragStart(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    document.body.style.userSelect = 'none';
    const startX = e.clientX;
    const startY = e.clientY;
    const startPosX = posX;
    const startPosY = posY;

    function onMove(ev: MouseEvent) {
      posX = startPosX + (ev.clientX - startX);
      posY = startPosY + (ev.clientY - startY);
      clampToViewport();
    }
    function onUp() {
      dragging = false;
      document.body.style.userSelect = '';
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  function onResizeStart(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    document.body.style.userSelect = 'none';
    const startX = e.clientX;
    const startY = e.clientY;
    const startW = panelWidth;
    const startH = panelHeight;

    function onMove(ev: MouseEvent) {
      const maxW = window.innerWidth - posX - 16;
      const maxH = window.innerHeight - posY - 16;
      panelWidth = Math.min(maxW, Math.max(MIN_W, startW + (ev.clientX - startX)));
      panelHeight = Math.min(maxH, Math.max(MIN_H, startH + (ev.clientY - startY)));
    }
    function onUp() {
      document.body.style.userSelect = '';
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  function closePanel() {
    hideTerminal();
  }

  function toggleMaximize() {
    if (maximized) {
      posX = prevPosX;
      posY = prevPosY;
      panelWidth = prevWidth;
      panelHeight = prevHeight;
      maximized = false;
    } else {
      prevPosX = posX;
      prevPosY = posY;
      prevWidth = panelWidth;
      prevHeight = panelHeight;
      posX = 16;
      posY = 16;
      panelWidth = window.innerWidth - 32;
      panelHeight = window.innerHeight - 32;
      maximized = true;
    }
    requestAnimationFrame(() => fitAddon?.fit());
  }

  let fontSize = $state(13);
  const MIN_FONT = 9;
  const MAX_FONT = 22;

  function changeFontSize(delta: number) {
    fontSize = Math.min(MAX_FONT, Math.max(MIN_FONT, fontSize + delta));
    if (terminal) {
      terminal.options.fontSize = fontSize;
      requestAnimationFrame(() => fitAddon?.fit());
    }
  }

  function clearTerminal() {
    terminal?.clear();
  }

  function clampToViewport() {
    posX = Math.max(0, Math.min(posX, window.innerWidth - panelWidth));
    posY = Math.max(0, Math.min(posY, window.innerHeight - panelHeight));
  }

  onMount(() => {
    posX = window.innerWidth - panelWidth - 16;
    posY = window.innerHeight - panelHeight - 16;

    requestAnimationFrame(() => { fadeIn = true; });

    window.addEventListener('resize', clampToViewport);
    window.addEventListener('pagehide', onPageHide);
    window.addEventListener('pageshow', onPageShow);

    terminal = new Terminal({
      cursorBlink: true,
      fontSize,
      fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', 'SF Mono', 'Menlo', ui-monospace, monospace",
      lineHeight: 1.2,
      cursorStyle: 'bar',
      cursorWidth: 2,
      theme: {
        background: '#0d1117',
        foreground: '#e6edf3',
        cursor: '#58a6ff',
        cursorAccent: '#0d1117',
        selectionBackground: '#264f78',
        selectionForeground: '#ffffff',
        black: '#484f58',
        red: '#ff7b72',
        green: '#3fb950',
        yellow: '#d29922',
        blue: '#58a6ff',
        magenta: '#bc8cff',
        cyan: '#39c5cf',
        white: '#b1bac4',
        brightBlack: '#6e7681',
        brightRed: '#ffa198',
        brightGreen: '#56d364',
        brightYellow: '#e3b341',
        brightBlue: '#79c0ff',
        brightMagenta: '#d2a8ff',
        brightCyan: '#56d4dd',
        brightWhite: '#f0f6fc',
      },
    });

    fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(terminalEl);
    requestAnimationFrame(() => fitAddon.fit());

    terminal.onData((data) => {
      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(new TextEncoder().encode(data));
      }
    });

    terminal.onResize(({ cols, rows }) => {
      termCols = cols;
      termRows = rows;
      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'resize', cols, rows }));
      }
    });

    terminal.attachCustomKeyEventHandler((e: KeyboardEvent) => {
      if (e.type !== 'keydown') return true;
      if ((e.ctrlKey || e.metaKey) && (e.key === '=' || e.key === '+')) {
        e.preventDefault();
        changeFontSize(1);
        return false;
      }
      if ((e.ctrlKey || e.metaKey) && e.key === '-') {
        e.preventDefault();
        changeFontSize(-1);
        return false;
      }
      return true;
    });

    resizeObserver = new ResizeObserver(() => fitAddon.fit());
    resizeObserver.observe(terminalEl);
    ready = true;
  });

  function connect() {
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    wsContract = contract;
    const socket = new WebSocket(`${protocol}//${location.host}/ws/pty${contractQuery(contract)}`);
    socket.binaryType = 'arraybuffer';
    ws = socket;

    socket.onopen = () => {
      connected = true;
      const { cols, rows } = terminal;
      termCols = cols;
      termRows = rows;
      socket.send(JSON.stringify({ type: 'resize', cols, rows }));
    };

    socket.onmessage = (event) => {
      if (event.data instanceof ArrayBuffer) {
        terminal.write(new Uint8Array(event.data));
      } else {
        terminal.write(event.data);
      }
    };

    socket.onclose = () => {
      connected = false;
      terminal.write('\r\n\x1b[90m[Disconnected]\x1b[0m\r\n');
    };
  }

  onDestroy(() => {
    window.removeEventListener('resize', clampToViewport);
    window.removeEventListener('pagehide', onPageHide);
    window.removeEventListener('pageshow', onPageShow);
    resizeObserver?.disconnect();
    disconnect();
    terminal?.dispose();
  });
</script>

<!-- Outer container: premium floating panel -->
<div
  class="fixed z-50 flex flex-col overflow-hidden"
  class:opacity-0={!fadeIn}
  class:opacity-100={fadeIn}
  class:hidden={!isTerminalVisible()}
  style="
    left: {posX}px;
    top: {posY}px;
    width: {panelWidth}px;
    height: {panelHeight}px;
    border-radius: 12px;
    border: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
    box-shadow:
      0 25px 50px -12px rgba(0, 0, 0, 0.5),
      0 12px 24px -8px rgba(0, 0, 0, 0.3),
      0 0 0 1px rgba(91, 155, 213, 0.05),
      0 0 80px -20px rgba(91, 155, 213, 0.08);
    transition: height 200ms cubic-bezier(0.4, 0, 0.2, 1), opacity 300ms ease-out;
  "
>
  <!-- Title bar (drag handle) — glass effect -->
  <div
    class="flex items-center justify-between px-3 shrink-0 select-none {dragging ? 'cursor-grabbing' : 'cursor-grab'}"
    style="
      height: {TITLE_BAR_H}px;
      background: linear-gradient(180deg, rgba(30, 30, 40, 0.85) 0%, rgba(24, 24, 30, 0.9) 100%);
      backdrop-filter: blur(16px) saturate(1.8);
      -webkit-backdrop-filter: blur(16px) saturate(1.8);
      border-bottom: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
    "
    onmousedown={onDragStart}
    role="toolbar"
  >
    <!-- Left: traffic light buttons -->
    <div class="flex items-center gap-2">
      <div class="flex items-center gap-1.5">
        <!-- Close (red) -->
        <button
          class="w-3 h-3 rounded-full border-none cursor-pointer transition-all duration-150 hover:brightness-125"
          style="background: var(--color-danger);"
          title="Close"
          aria-label="Close terminal"
          onmousedown={(e: MouseEvent) => e.stopPropagation()}
          onclick={() => closePanel()}
        ></button>
        <!-- Hide (yellow) — kept alongside red for the macOS traffic-light
             aesthetic. Both close the terminal; reopen via TopBar >_. -->
        <button
          class="w-3 h-3 rounded-full border-none cursor-pointer transition-all duration-150 hover:brightness-125"
          style="background: var(--color-warning);"
          title="Hide terminal"
          aria-label="Hide terminal"
          onmousedown={(e: MouseEvent) => e.stopPropagation()}
          onclick={() => closePanel()}
        ></button>
        <!-- Maximize (green/success) -->
        <button
          class="w-3 h-3 rounded-full border-none cursor-pointer transition-all duration-150 hover:brightness-125"
          style="background: var(--color-success);"
          title={maximized ? 'Restore' : 'Maximize'}
          aria-label={maximized ? 'Restore terminal size' : 'Maximize terminal'}
          onmousedown={(e: MouseEvent) => e.stopPropagation()}
          onclick={() => toggleMaximize()}
        ></button>
      </div>

      <!-- PTY link status — compact dot only. The docked StatusBar already
           exposes the main WS state; labelling this one "connected" side
           by side would have read as duplicate info. -->
      <div class="flex items-center gap-1.5 ml-3" title={connected ? 'PTY connected' : 'PTY disconnected'}>
        <span
          class="w-1.5 h-1.5 rounded-full inline-block"
          style="background: {connected ? 'var(--color-success-light)' : 'var(--color-danger-light)'}; box-shadow: 0 0 6px {connected ? 'var(--color-success)' : 'var(--color-danger)'};"
          aria-label={connected ? 'PTY connected' : 'PTY disconnected'}
        ></span>
        <span class="text-[9px] text-text-dim tracking-wider uppercase">PTY</span>
      </div>
    </div>

    <!-- Center: title -->
    <span
      class="text-[11px] font-semibold tracking-wider uppercase absolute left-1/2 -translate-x-1/2"
      style="color: var(--color-text-muted);"
    >Terminal</span>

    <!-- Right: spacer for symmetry -->
    <div class="w-16"></div>
  </div>

  <!-- Terminal body -->
  <div
    class="flex-1 min-h-0 relative"
    style="
      background: rgba(13, 17, 23, 0.97);
    "
  >
    <!-- Inner shadow overlay for depth -->
    <div
      class="absolute inset-x-0 top-0 h-4 z-10 pointer-events-none"
      style="background: linear-gradient(180deg, rgba(0, 0, 0, 0.25) 0%, transparent 100%);"
    ></div>
    <div bind:this={terminalEl} class="h-full w-full" />
  </div>

  <!-- Status bar (bottom) — glass style matching the docked StatusBar so
       both strips read as the same UI layer (same height, gradient, font
       size). Cols×rows and PTY dot stay inside the terminal because they
       describe the xterm instance, not the global app. -->
  <div
      class="flex items-center justify-between px-3 shrink-0 select-none"
      style="
        height: 24px;
        background: linear-gradient(180deg, rgba(22, 22, 28, 0.92) 0%, rgba(16, 16, 22, 0.95) 100%);
        backdrop-filter: blur(16px) saturate(1.8);
        -webkit-backdrop-filter: blur(16px) saturate(1.8);
        border-top: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
      "
    >
      <div class="flex items-center gap-3">
        <span class="text-[10px] text-text-dim font-mono tracking-wide">
          {termCols}×{termRows}
        </span>
        <span
          class="w-1 h-1 rounded-full inline-block"
          style="background: {connected ? 'var(--color-success)' : 'var(--color-danger)'};"
          aria-label={connected ? 'PTY connected' : 'PTY disconnected'}
        ></span>
      </div>
      <div class="flex items-center gap-2">
        <!-- Font size controls -->
        <div class="flex items-center gap-0.5">
          <button
            class="bg-transparent border-none text-text-dim hover:text-accent-hover cursor-pointer text-[11px] w-5 h-5 flex items-center justify-center rounded transition-colors duration-150"
            onclick={() => changeFontSize(-1)}
            title="Decrease font size"
            aria-label="Decrease font size"
          >−</button>
          <span class="text-[10px] text-text-dim font-mono min-w-[24px] text-center">{fontSize}px</span>
          <button
            class="bg-transparent border-none text-text-dim hover:text-accent-hover cursor-pointer text-[11px] w-5 h-5 flex items-center justify-center rounded transition-colors duration-150"
            onclick={() => changeFontSize(1)}
            title="Increase font size"
            aria-label="Increase font size"
          >+</button>
        </div>
        <button
          class="bg-transparent border-none text-text-dim hover:text-text-muted cursor-pointer text-[10px] px-1.5 py-0 rounded tracking-wide transition-colors duration-150"
          onclick={clearTerminal}
          title="Clear terminal"
        >
          clear
        </button>
      </div>
  </div>

  <!-- Resize handle (bottom-right corner) — diagonal grip pattern -->
  <div
      class="absolute bottom-0 right-0 w-5 h-5 cursor-nwse-resize z-10"
      onmousedown={onResizeStart}
      role="separator"
      aria-label="Resize terminal"
    >
      <svg class="absolute bottom-1 right-1 opacity-30 hover:opacity-60 transition-opacity duration-150" width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="var(--color-text-muted)" stroke-width="1">
        <line x1="9" y1="1" x2="1" y2="9" />
        <line x1="9" y1="4" x2="4" y2="9" />
        <line x1="9" y1="7" x2="7" y2="9" />
      </svg>
  </div>
</div>
