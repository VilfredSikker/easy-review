<script lang="ts">
  import { onDestroy } from "svelte";
  import { Terminal as XTerm } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";
  import { app } from "$lib/stores/app.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import { themeByName, xtermThemeFor } from "$lib/themes";

  interface Props {
    sessionId: string;
    cwd: string;
    visible: boolean;
    /** Increment to refit after drawer resize. */
    refitToken?: number;
  }

  const { sessionId, cwd, visible, refitToken = 0 }: Props = $props();

  const branch = $derived(app.snapshot?.branch ?? "");

  const xtermTheme = $derived(xtermThemeFor(themeByName(app.snapshot?.theme)));
  // Re-theme a live terminal when the app theme changes.
  $effect(() => {
    if (term) term.options.theme = xtermTheme;
  });

  async function insertCheckoutCommand() {
    if (!branch) return;
    const { invoke } = await import("@tauri-apps/api/core");
    const encoder = new TextEncoder();
    const bytes = Array.from(encoder.encode(`git checkout ${branch}`));
    try {
      await invoke("terminal_write", { sessionId, bytes });
    } catch {
      /* ignore — PTY may have exited */
    }
    term?.focus();
  }

  let containerEl = $state<HTMLDivElement | null>(null);
  let term: XTerm | null = null;
  let unlistenOutput: (() => void) | null = null;
  let unlistenExit: (() => void) | null = null;
  let resizeObs: ResizeObserver | null = null;
  let mountedSessionId = $state<string | null>(null);
  /** After killSession, block remount until the drawer is hidden and reopened. */
  let sessionEnded = $state(false);

  const CHAR_W = 7.8;
  const CHAR_H = 17;

  function bytesToUint8Array(bytes: number[] | Uint8Array): Uint8Array {
    return bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  }

  async function mountTerminal() {
    if (term || !containerEl) return;

    term = new XTerm({
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      scrollback: 10000,
      theme: xtermTheme,
    });

    term.open(containerEl);

    const { invoke } = await import("@tauri-apps/api/core");
    const { listen } = await import("@tauri-apps/api/event");

    unlistenOutput = await listen<{ session_id: string; bytes: number[] }>(
      "terminal-output",
      (event) => {
        if (event.payload.session_id !== sessionId) return;
        term?.write(bytesToUint8Array(event.payload.bytes));
      },
    );
    unlistenExit = await listen<{ session_id: string }>(
      "terminal-exit",
      (event) => {
        if (event.payload.session_id !== sessionId) return;
        term?.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n");
      },
    );

    try {
      await invoke("terminal_spawn", { sessionId, cwd });
    } catch (e) {
      term.write(`\r\n\x1b[31mfailed to spawn shell: ${String(e)}\x1b[0m\r\n`);
      return;
    }

    const encoder = new TextEncoder();
    term.onData((data) => {
      const bytes = Array.from(encoder.encode(data));
      invoke("terminal_write", { sessionId, bytes }).catch(() => {});
    });

    resizeObs = new ResizeObserver(() => applyResize());
    resizeObs.observe(containerEl);
    mountedSessionId = sessionId;
    applyResize();
    if (visible) term.focus();
  }

  function applyResize() {
    if (!term || !containerEl || !visible) return;
    const rect = containerEl.getBoundingClientRect();
    if (rect.height < CHAR_H * 2 || rect.width < CHAR_W * 4) return;
    const cols = Math.max(20, Math.floor(rect.width / CHAR_W));
    const rows = Math.max(4, Math.floor(rect.height / CHAR_H));
    term.resize(cols, rows);
    import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("terminal_resize", { sessionId, rows, cols }))
      .catch(() => {});
    term.scrollToBottom();
  }

  function disposeXterm() {
    if (unlistenOutput) {
      unlistenOutput();
      unlistenOutput = null;
    }
    if (unlistenExit) {
      unlistenExit();
      unlistenExit = null;
    }
    if (resizeObs) {
      resizeObs.disconnect();
      resizeObs = null;
    }
    if (term) {
      term.dispose();
      term = null;
    }
    mountedSessionId = null;
  }

  async function killSession() {
    sessionEnded = true;
    const sid = mountedSessionId ?? sessionId;
    disposeXterm();
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("terminal_close", { sessionId: sid });
    } catch {
      /* ignore */
    }
    terminal.open = false;
  }

  $effect(() => {
    if (!visible && sessionEnded) {
      sessionEnded = false;
    }
  });

  $effect(() => {
    const sid = sessionId;
    if (mountedSessionId && mountedSessionId !== sid && term) {
      void disposeXterm();
    }
    if (visible && containerEl && !term && !sessionEnded) {
      void mountTerminal();
    }
  });

  $effect(() => {
    void refitToken;
    if (visible && term) {
      applyResize();
    }
  });

  $effect(() => {
    if (visible && term) {
      applyResize();
      term.focus();
    }
  });

  onDestroy(() => {
    disposeXterm();
  });
</script>

<div class="w-full h-full flex flex-col bg-ink-900 overflow-hidden {visible ? '' : 'pointer-events-none'}">
  <div
    class="h-6 shrink-0 border-b border-hairline bg-ink-870 flex items-center gap-2 px-2 text-[11px] mono"
  >
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-muted shrink-0">
      <polyline points="4 17 10 11 4 5" /><line x1="12" y1="19" x2="20" y2="19" />
    </svg>
    <span class="text-fg-3 truncate">{branch || "—"}</span>
    <button
      type="button"
      class="px-1.5 py-0.5 rounded text-fg-3 hover:bg-hover hover:text-fg disabled:opacity-40 disabled:hover:bg-transparent"
      disabled={!branch}
      onclick={insertCheckoutCommand}
      title="Type 'git checkout {branch}' into the terminal (you press Enter)"
    >
      Insert: git checkout {branch}
    </button>
    <div class="flex-1"></div>
    <button
      type="button"
      class="w-5 h-5 flex items-center justify-center rounded text-fg-3 hover:bg-hover hover:text-fg"
      onclick={() => void killSession()}
      title="End session (kills shell)"
      aria-label="End session"
    >
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
      </svg>
    </button>
    <button
      type="button"
      class="w-5 h-5 flex items-center justify-center rounded text-fg-3 hover:bg-hover hover:text-fg"
      onclick={() => terminal.toggle()}
      title="Hide terminal"
      aria-label="Hide terminal"
    >
      <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    </button>
  </div>
  <div bind:this={containerEl} class="flex-1 min-h-0 overflow-hidden"></div>
</div>

<style>
  :global(.xterm) {
    height: 100%;
    width: 100%;
  }
  :global(.xterm-viewport) {
    background-color: transparent !important;
  }
</style>
