<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import RightPanel from "$lib/components/RightPanel.svelte";
  import BottomHints from "$lib/components/BottomHints.svelte";
  import type { AppSnapshot } from "$lib/types";

  interface PinnedItem {
    id: string;
    title: string;
    age: string;
  }
  interface ProjectGroup {
    name: string;
    activeBranch?: string;
    branches: {
      name: string;
      age: string;
      state: "active" | "fork" | "branch";
      kind?: "pr" | "branch";
      merged?: boolean;
      path?: string;
      prNumber?: number | null;
    }[];
    badge?: number;
  }

  interface Props {
    snapshot: AppSnapshot;
    pinned?: PinnedItem[];
    projects?: ProjectGroup[];
    /** The label shown after "Review" in the titlebar chip — mock uses the project name, not the branch. */
    titlebarSubtitle?: string;
  }
  const { snapshot, pinned, projects, titlebarSubtitle }: Props = $props();

  /**
   * Seed the global app store with the story's fixture so child components
   * (which all read from `app.snapshot`) render correctly. This is how we
   * recreate the full Tauri app without the Tauri shell.
   */
  $effect(() => {
    app.snapshot = snapshot;
  });

  const panels = $derived(snapshot.panels);
</script>

<div class="h-screen flex flex-col bg-[#0a0a0a] text-[#e8e8e8] overflow-hidden font-sans">
  <header class="h-11 border-b border-[#1f1f1f] bg-[#0d0d0d] flex items-center gap-1 shrink-0 px-3">
    <div class="flex items-center gap-2 mr-3">
      <span class="w-3 h-3 rounded-full bg-[#ff5f56]"></span>
      <span class="w-3 h-3 rounded-full bg-[#ffbd2e]"></span>
      <span class="w-3 h-3 rounded-full bg-[#27c93f]"></span>
    </div>

    <div class="flex items-center gap-0.5 mr-3 text-[#666]">
      <button class="w-7 h-7 rounded hover:bg-[#1a1a1a] flex items-center justify-center text-[#ff6a3d] bg-[#1a1a1a]" aria-label="Toggle left">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18"/></svg>
      </button>
      <button class="w-7 h-7 rounded hover:bg-[#1a1a1a] flex items-center justify-center opacity-40" aria-label="Back">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="15 18 9 12 15 6"/></svg>
      </button>
      <button class="w-7 h-7 rounded hover:bg-[#1a1a1a] flex items-center justify-center opacity-40" aria-label="Forward">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
      </button>
    </div>

    <div class="flex items-center gap-1">
      <div class="flex items-center gap-2 px-3 py-1 rounded-md bg-[#1a1a1a] border border-[#2a2a2a] text-sm">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
        <span>Review</span>
        <span class="mono text-[10px] text-[#666]">{titlebarSubtitle ?? snapshot.branch}</span>
      </div>
      <button class="w-7 h-7 rounded hover:bg-[#1a1a1a] flex items-center justify-center text-[#666]" aria-label="New tab">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
      </button>
    </div>

    <div class="ml-auto flex items-center gap-1 text-[#666]">
      <button class="w-7 h-7 rounded hover:bg-[#1a1a1a] flex items-center justify-center text-[#ff6a3d] bg-[#1a1a1a]" aria-label="Toggle tree">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z"/></svg>
      </button>
      <button class="text-xs text-[#999] hover:bg-[#1a1a1a] px-3 py-1 rounded-md mono">⌘K</button>
      <button class="w-7 h-7 rounded hover:bg-[#1a1a1a] flex items-center justify-center text-[#ff6a3d] bg-[#1a1a1a]" aria-label="Toggle right">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M15 3v18"/></svg>
      </button>
    </div>
  </header>

  <div class="flex-1 flex min-h-0">
    <LeftSidebar collapsed={!panels.left} pinnedOverride={pinned} />
    <main class="flex-1 flex min-w-0">
      <FileTree collapsed={!panels.tree} />
      <DiffView />
    </main>
    {#if panels.right}
      <RightPanel ai={snapshot.ai} pr={snapshot.pr} />
    {/if}
  </div>

  <BottomHints />
</div>
