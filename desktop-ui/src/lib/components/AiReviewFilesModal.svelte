<script lang="ts" module>
  let openModal: (() => void) | null = null;

  export function openAiReviewFilesModal(): void {
    openModal?.();
  }
</script>

<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import ReviewerPickerList from "$lib/components/ReviewerPickerList.svelte";
  import { openProfessorFocusModal } from "$lib/components/ProfessorFocusModal.svelte";
  import { closeAiActionPalette } from "$lib/components/AiActionPalette.svelte";
  import { fileTreeCollapse } from "$lib/stores/fileTreeCollapse.svelte";
  import { filesByPathMap } from "$lib/treeFromPaths";
  import type { FileSnapshot } from "$lib/types";

  type ReviewScope = "branch" | "unstaged" | "staged";
  type SubView = "files" | "reviewers";

  let open = $state(false);
  let subView = $state<SubView>("files");
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let pickerFiles = $state<FileSnapshot[]>([]);
  let selected = $state<Set<string>>(new Set());
  let selectedReviewers = $state<Set<string>>(new Set());
  let reviewerHighlight = $state(0);
  let submitting = $state(false);
  let treeHostEl: HTMLDivElement | null = $state(null);
  let reviewerPickerRef = $state<{ moveHighlight: (d: number) => void; toggleHighlighted: () => void } | null>(null);

  const mode = $derived(app.snapshot?.mode);
  const reviewScope = $derived(
    mode === "branch" || mode === "unstaged" || mode === "staged" ? (mode as ReviewScope) : null,
  );

  const scopeLabel = $derived(
    mode === "branch"
      ? "All changes"
      : mode === "unstaged"
        ? "Unstaged"
        : mode === "staged"
          ? "Staged"
          : "—",
  );

  const selectedCount = $derived(selected.size);
  const reviewerCount = $derived(selectedReviewers.size);

  function pathsToFileSnapshots(paths: string[]): FileSnapshot[] {
    const byPath = filesByPathMap(app.snapshot?.files ?? []);
    return paths.map((path, i) => {
      const existing = byPath.get(path);
      if (existing) return existing;
      return {
        path,
        status: "modified",
        additions: 0,
        deletions: 0,
        reviewed: false,
        compacted: false,
        risk: null,
        finding_count: 0,
        comment_count: 0,
        question_count: 0,
        hunks: [],
        source_index: i,
        cache_key: path,
      };
    });
  }

  function close() {
    open = false;
    subView = "files";
    loading = false;
    loadError = null;
    pickerFiles = [];
    selected = new Set();
    selectedReviewers = new Set();
    reviewerHighlight = 0;
    submitting = false;
  }

  async function loadFiles() {
    loading = true;
    loadError = null;
    try {
      const paths = await invoke<string[]>("list_diff_paths");
      pickerFiles = pathsToFileSnapshots(paths);
      selected = new Set(paths);
      for (const f of pickerFiles) {
        fileTreeCollapse.expandAncestorsOf(f.path);
      }
    } catch (e) {
      loadError = String(e);
      pickerFiles = [];
      selected = new Set();
    } finally {
      loading = false;
      await tick();
      treeHostEl?.querySelector<HTMLElement>('[role="tree"]')?.focus();
    }
  }

  function openFromOutside() {
    if (!reviewScope) {
      app.showToast("error", "Switch to All changes, Unstaged, or Staged to review files");
      return;
    }
    closeAiActionPalette();
    subView = "files";
    open = true;
    void loadFiles();
  }

  function markAll() {
    selected = new Set(pickerFiles.map((f) => f.path));
  }

  function unmarkAll() {
    selected = new Set();
  }

  function onSelectedPathsChange(paths: Set<string>) {
    selected = paths;
  }

  function goToReviewers() {
    if (selectedCount === 0) return;
    subView = "reviewers";
    reviewerHighlight = 0;
  }

  function goBackToFiles() {
    subView = "files";
  }

  async function runReviewers() {
    if (!reviewScope || reviewerCount === 0 || submitting) return;
    const scope = reviewScope;
    const paths = [...selected];
    const kinds = [...selectedReviewers];
    if (kinds.includes("professor")) {
      close();
      openProfessorFocusModal(scope, kinds, paths);
      return;
    }
    submitting = true;
    try {
      close();
      await app.cmd("run_ai_scoped_review", {
        scope,
        paths,
        reviewerKinds: kinds,
        focusPrompt: null,
      });
    } finally {
      submitting = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      if (subView === "reviewers") goBackToFiles();
      else close();
      return;
    }
    if (subView === "reviewers") {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        reviewerPickerRef?.moveHighlight(1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        reviewerPickerRef?.moveHighlight(-1);
      } else if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        if (e.key === " " && document.activeElement?.tagName !== "BUTTON") {
          reviewerPickerRef?.toggleHighlighted();
        } else if (e.key === "Enter" && reviewerCount > 0) {
          void runReviewers();
        }
      }
    }
  }

  onMount(() => {
    openModal = openFromOutside;
    return () => {
      openModal = null;
    };
  });
</script>

<ModalShell
  {open}
  ariaLabel="Review selected files"
  onClose={close}
  onKeydown={handleKeydown}
  closeOnEscape={false}
  panelClass="fixed left-1/2 -translate-x-1/2 top-[6vh] z-[252] bg-ink-800 border border-ink-500 rounded-lg shadow-2xl w-[min(920px,calc(100vw-2rem))] h-[min(80vh,880px)] max-h-[calc(100vh-3rem)] flex flex-col overflow-hidden outline-none"
>
  <div class="px-4 pt-3 pb-2 border-b border-ink-600 flex items-center gap-2 shrink-0">
    <span class="text-xs text-ink-300 font-mono">
      {subView === "files" ? "Review selected files" : "Choose reviewers"}
    </span>
    <span class="text-[10px] text-ink-400 font-mono ml-1">
      {scopeLabel}{#if subView === "files" && pickerFiles.length > 0} · {pickerFiles.length} files{/if}
    </span>
    <kbd class="ml-auto shrink-0 text-[10px] font-mono px-1.5 py-0.5 rounded bg-ink-650 border border-ink-500 text-ink-400">Esc</kbd>
  </div>

  {#if subView === "files"}
    <div class="px-4 py-2 border-b border-ink-600 flex items-center gap-2 shrink-0">
      <button
        type="button"
        class="text-xs text-ink-300 hover:text-ink-100 disabled:opacity-40"
        disabled={loading || pickerFiles.length === 0}
        onclick={markAll}
      >
        Mark all
      </button>
      <span class="text-ink-600">·</span>
      <button
        type="button"
        class="text-xs text-ink-300 hover:text-ink-100 disabled:opacity-40"
        disabled={loading || pickerFiles.length === 0}
        onclick={unmarkAll}
      >
        Unmark all
      </button>
      <span class="ml-auto text-[10px] text-ink-400 font-mono">{selectedCount} selected</span>
    </div>

    <div bind:this={treeHostEl} class="flex-1 min-h-0 flex flex-col overflow-hidden">
      {#if loading}
        <p class="px-4 py-6 text-sm text-ink-400">Loading files…</p>
      {:else if loadError}
        <p class="px-4 py-6 text-sm text-del-fg">{loadError}</p>
      {:else if pickerFiles.length === 0}
        <p class="px-4 py-6 text-sm text-ink-400">No files in this view.</p>
      {:else}
        <FileTree
          pickerMode={true}
          embedded={true}
          files={pickerFiles}
          selectedPaths={selected}
          onSelectedPathsChange={onSelectedPathsChange}
          onPickerEnter={goToReviewers}
        />
      {/if}
    </div>

    <div class="px-4 py-3 border-t border-ink-600 flex items-center justify-end gap-2 shrink-0">
      <Button variant="ghost" onclick={close}>Cancel</Button>
      <Button
        variant="primary"
        disabled={loading || !!loadError || selectedCount === 0}
        onclick={goToReviewers}
      >
        Choose reviewers…
      </Button>
    </div>
  {:else}
    <div class="flex-1 min-h-0 flex flex-col overflow-hidden">
      <ReviewerPickerList
        bind:this={reviewerPickerRef}
        selected={selectedReviewers}
        onSelectedChange={(s) => (selectedReviewers = s)}
        bind:highlightIdx={reviewerHighlight}
      />
    </div>
    <div class="px-4 py-3 border-t border-ink-600 flex items-center justify-end gap-2 shrink-0">
      <Button variant="ghost" onclick={goBackToFiles}>Back</Button>
      <Button
        variant="primary"
        disabled={reviewerCount === 0 || submitting}
        onclick={() => void runReviewers()}
      >
        Run {reviewerCount} reviewer{reviewerCount === 1 ? "" : "s"}
      </Button>
    </div>
  {/if}
</ModalShell>
