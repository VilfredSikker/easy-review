<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal as XTerm } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";

  interface Props {
    fixture: string;
    /** Optional: render the toolbar mock (no real PTY wiring). */
    showToolbar?: boolean;
    /** Optional: branch shown in the toolbar mock. */
    branch?: string;
  }
  const { fixture, showToolbar = false, branch = "main" }: Props = $props();

  let el = $state<HTMLDivElement | null>(null);
  let term: XTerm | null = null;

  onMount(() => {
    if (!el) return;
    term = new XTerm({
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      fontSize: 13,
      cursorBlink: true,
      theme: { background: "#0e0e0e", foreground: "#e6e6e6", cursor: "#ff6a3d" },
    });
    term.open(el);
    term.resize(100, 20);
    term.write(fixture);
  });

  onDestroy(() => {
    term?.dispose();
    term = null;
  });
</script>

<div style="height: 400px; background: #0e0e0e; display: flex; flex-direction: column;">
  {#if showToolbar}
    <div class="h-6 shrink-0 border-b border-hairline bg-ink-870 flex items-center gap-2 px-2 text-[11px] mono">
      <span class="text-fg-3 truncate">{branch}</span>
      <button type="button" class="px-1.5 py-0.5 rounded text-fg-3 hover:bg-hover hover:text-fg">
        Insert: git checkout {branch}
      </button>
      <div class="flex-1"></div>
      <button type="button" class="w-5 h-5 flex items-center justify-center rounded text-fg-3 hover:bg-hover hover:text-fg" aria-label="Close terminal">
        ×
      </button>
    </div>
  {/if}
  <div bind:this={el} style="flex: 1; min-height: 0; padding: 8px;"></div>
</div>
