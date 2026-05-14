<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal as XTerm } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";
  import { app } from "$lib/stores/app.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";

  interface Props {
    sessionId: string;
    cwd: string;
    visible: boolean;
  }

  const { sessionId, cwd, visible }: Props = $props();

  const branch = $derived(app.snapshot?.branch ?? "");

  /**
   * Inserts `git checkout <branch>` at the PTY without a trailing newline.
   * The user must press Enter to confirm — this is intentional (spec): we
   * never auto-execute branch switches.
   */
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
    // Return focus to xterm so the user can press Enter immediately.
    term?.focus();
  }

  let containerEl = $state<HTMLDivElement | null>(null);
  let term: XTerm | null = null;
  let unlistenOutput: (() => void) | null = null;
  let unlistenExit: (() => void) | null = null;
  let resizeObs: ResizeObserver | null = null;
  /**
   * Approx character cell width in px. Measured once at mount; used to
   * compute cols/rows on resize without pulling in `@xterm/addon-fit`.
   * JetBrains Mono at 13px is roughly 7.8px wide × 17px tall.
   */
  const CHAR_W = 7.8;
  const CHAR_H = 17;

  // Encode/decode bytes — Tauri serializes `Vec<u8>` as a JS number array.
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
      theme: {
        background: "#0e0e0e",
        foreground: "#e6e6e6",
        cursor: "#ff6a3d",
        cursorAccent: "#0e0e0e",
        selectionBackground: "#3a3a3a",
        black: "#1a1a1a",
        red: "#f4a3a3",
        green: "#9ad79a",
        yellow: "#e6c87a",
        blue: "#7aa8e6",
        magenta: "#c89af0",
        cyan: "#7ad7d7",
        white: "#e6e6e6",
        brightBlack: "#5e5e5e",
        brightRed: "#ffb3b3",
        brightGreen: "#aae8aa",
        brightYellow: "#ffd98a",
        brightBlue: "#9abdf2",
        brightMagenta: "#d8aaf8",
        brightCyan: "#9ae8e8",
        brightWhite: "#ffffff",
      },
    });

    term.open(containerEl);

    const { invoke } = await import("@tauri-apps/api/core");
    const { listen } = await import("@tauri-apps/api/event");

    // Subscribe BEFORE spawning so we never miss the first chunk.
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

    // Pipe keystrokes back to the PTY. xterm gives us a UTF-8 string; encode.
    const encoder = new TextEncoder();
    term.onData((data) => {
      const bytes = Array.from(encoder.encode(data));
      invoke("terminal_write", { sessionId, bytes }).catch(() => {});
    });

    // Drive PTY resize from the container size.
    resizeObs = new ResizeObserver(() => applyResize());
    resizeObs.observe(containerEl);
    applyResize();
  }

  function applyResize() {
    if (!term || !containerEl) return;
    const rect = containerEl.getBoundingClientRect();
    const cols = Math.max(20, Math.floor(rect.width / CHAR_W));
    const rows = Math.max(4, Math.floor(rect.height / CHAR_H));
    term.resize(cols, rows);
    import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("terminal_resize", { sessionId, rows, cols }))
      .catch(() => {});
  }

  async function teardown() {
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
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("terminal_close", { sessionId });
    } catch {
      /* ignore */
    }
  }

  // Mount when first visible. If `visible` flips false, we tear down — the
  // shell dies (spec is fine with no scrollback persistence across toggle).
  $effect(() => {
    if (visible && containerEl && !term) {
      mountTerminal();
    } else if (!visible && term) {
      teardown();
    }
  });

  onMount(() => {
    if (visible) mountTerminal();
  });

  onDestroy(() => {
    teardown();
  });
</script>

<div class="w-full h-full flex flex-col bg-ink-900 overflow-hidden">
  <!--
    Thin toolbar: branch label + insert-checkout shortcut + close.
    Height is locked at 24px so the existing container resize math (CHAR_H
    rows) keeps working — only the xterm area is measured for PTY sizing.
  -->
  <div
    class="h-6 shrink-0 border-b border-hairline bg-ink-870 flex items-center gap-2 px-2 text-[11px] mono"
  >
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
      onclick={() => terminal.toggle()}
      title="Close terminal"
      aria-label="Close terminal"
    >
      ×
    </button>
  </div>
  <div bind:this={containerEl} class="flex-1 min-h-0 p-2 overflow-hidden"></div>
</div>

<style>
  /* xterm renders its own canvas; make sure it fills our box. */
  :global(.xterm) {
    height: 100%;
    width: 100%;
  }
  :global(.xterm-viewport) {
    background-color: transparent !important;
  }
</style>
