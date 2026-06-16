<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { openPrUrlModal } from "$lib/stores/prUrlModal.svelte";
  import { startWindowDrag } from "$lib/windowDrag";
  import { openExternalUrl } from "$lib/openExternalUrl";
  import AppMark from "$lib/components/AppMark.svelte";

  interface Props {
    /** When true, render as a dismissible layer over an active review. */
    overlay?: boolean;
  }

  const { overlay = false }: Props = $props();

  let prUrl = $state("");
  let prUrlInput: HTMLInputElement | null = $state(null);

  const canSubmitPrUrl = $derived(prUrl.trim().length > 0);

  function syncPrUrlFromInput() {
    if (prUrlInput) prUrl = prUrlInput.value;
  }

  const projects = $derived(app.snapshot?.projects ?? []);
  let selectedProjectId = $state<string>("");
  let branchName = $state<string>("");

  async function openPrUrl() {
    syncPrUrlFromInput();
    const url = (prUrlInput?.value ?? prUrl).trim();
    if (!url) return;
    await app.cmd("open_pr_url", { url });
    prUrl = "";
    app.showEmptyState = false;
  }

  async function openWorktree() {
    await app.cmd("open_worktree", {});
    app.showEmptyState = false;
  }

  async function openProjectBranch() {
    const projectId = selectedProjectId || projects[0]?.id;
    if (!projectId || !branchName.trim()) return;
    await app.cmd("open_project_branch", {
      projectId,
      branch: branchName.trim(),
    });
    branchName = "";
    app.showEmptyState = false;
  }

  const hasActiveReview = $derived(
    overlay ||
      (app.snapshot?.tabs?.length ?? 0) > 0 ||
      (app.snapshot?.files.length ?? 0) > 0,
  );

  function backToReview() {
    app.showEmptyState = false;
  }

  function connectGitHub() {
    void openExternalUrl("https://github.com/login");
  }

  function dismiss(e: KeyboardEvent) {
    if (e.key === "Escape" && hasActiveReview) {
      app.showEmptyState = false;
    }
  }

  const rootClass = $derived(
    overlay
      ? "h-full flex flex-col bg-bg text-fg"
      : "h-screen flex flex-col bg-bg text-fg",
  );
</script>

<svelte:window onkeydown={dismiss} />

<div class={rootClass}>
  <!-- Minimal top bar -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header
    class="titlebar-drag h-11 px-4 border-b border-hairline bg-ink-870 flex items-center gap-2 shrink-0"
    style="padding-left: env(titlebar-area-x, 80px)"
    data-tauri-drag-region
    onmousedown={startWindowDrag}
  >
    <AppMark size={20} />
    <span class="text-sm">Easy Review</span>
    {#if hasActiveReview}
      <button
        type="button"
        onclick={backToReview}
        class="ml-auto text-xs text-fg-3 hover:text-fg-1 px-2 py-1 rounded-md hover:bg-hover"
      >
        Back to review
      </button>
    {/if}
  </header>

  <div class="flex flex-1 min-h-0">
    <!-- Left sidebar with hints -->
    <aside class="w-60 shrink-0 bg-surface border-r border-hairline p-3">
      <div class="text-[11px] uppercase tracking-wider text-muted px-2 mb-2">Recents</div>
      <div class="text-xs text-muted px-2 py-1.5">No reviews yet</div>

      <div class="text-[11px] uppercase tracking-wider text-muted px-2 mt-6 mb-2">Get started</div>
      <button onclick={openWorktree} class="w-full text-left px-2 py-1.5 rounded-md hover:bg-hover text-sm text-fg-2 flex items-center gap-2">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 7l9-5 9 5v10l-9 5-9-5V7z"/></svg>
        Add project
      </button>
      <button
        type="button"
        onclick={() => void openPrUrlModal()}
        class="w-full text-left px-2 py-1.5 rounded-md hover:bg-hover text-sm text-fg-3 flex items-center gap-2"
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/></svg>
        Open PR by URL
      </button>
      <button
        type="button"
        onclick={connectGitHub}
        class="w-full text-left px-2 py-1.5 rounded-md hover:bg-hover text-sm text-fg-3 flex items-center gap-2"
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/></svg>
        Connect GitHub
      </button>
    </aside>

    <!-- Main welcome -->
    <main class="flex-1 flex items-center justify-center p-8 overflow-y-auto">
      <div class="max-w-2xl w-full">
        <div class="mb-10">
          <AppMark size={48} class="mb-5" />
          <h1 class="text-3xl font-semibold tracking-tight mb-2">Review code, then your app.</h1>
          <p class="text-fg-3 text-lg">Diff review and live UI annotation in one workspace. Local-first.</p>
        </div>

        <div class="grid grid-cols-2 gap-3 mb-6">
          <button onclick={openWorktree} class="group text-left p-5 rounded-xl border border-border hover:border-accent hover:bg-card transition">
            <div class="flex items-center gap-3 mb-2">
              <div class="w-9 h-9 rounded-lg bg-hover border border-border flex items-center justify-center group-hover:border-accent">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-fg-2"><path d="M3 7l9-5 9 5v10l-9 5-9-5V7z"/></svg>
              </div>
              <div class="font-medium">Open a local repo</div>
            </div>
            <p class="text-sm text-fg-3">Pick any git repo. Reviews are stored in <span class="mono text-fg-2">.er/</span> alongside your code.</p>
            <div class="mt-3 text-[11px] text-muted mono">⌘O</div>
          </button>

          <button
            type="button"
            onclick={() => void openPrUrlModal()}
            class="group text-left p-5 rounded-xl border border-border hover:border-accent hover:bg-card transition"
          >
            <div class="flex items-center gap-3 mb-2">
              <div class="w-9 h-9 rounded-lg bg-hover border border-border flex items-center justify-center group-hover:border-accent">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" class="text-fg-2"><path d="M12 0C5.4 0 0 5.4 0 12c0 5.3 3.4 9.8 8.2 11.4.6.1.8-.3.8-.6v-2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2 1-.3 2-.4 3-.4s2 .1 3 .4c2.3-1.6 3.3-1.2 3.3-1.2.7 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6C20.6 21.8 24 17.3 24 12c0-6.6-5.4-12-12-12z"/></svg>
              </div>
              <div class="font-medium">Open a PR</div>
            </div>
            <p class="text-sm text-fg-3">Paste a GitHub URL. We'll open the PR for review.</p>
            <div class="mt-3 text-[11px] text-muted mono">⌘⇧O</div>
          </button>
        </div>

        <!-- Paste field -->
        <div class="rounded-xl border border-border bg-surface p-3 flex items-center gap-3 mb-8 min-w-0">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 text-muted"><circle cx="12" cy="12" r="10"/><path d="M2 12h20"/></svg>
          <input
            bind:this={prUrlInput}
            bind:value={prUrl}
            oninput={syncPrUrlFromInput}
            onpaste={() => queueMicrotask(syncPrUrlFromInput)}
            onchange={syncPrUrlFromInput}
            onkeydown={(e) => e.key === "Enter" && void openPrUrl()}
            class="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted mono"
            placeholder="Paste a GitHub PR URL…"
          />
          <button
            type="button"
            onclick={openPrUrl}
            disabled={!canSubmitPrUrl}
            class="shrink-0 px-3 py-1.5 rounded-md bg-accent hover:bg-accent/90 disabled:opacity-40 disabled:cursor-not-allowed text-on-accent text-xs font-medium"
          >
            Review
          </button>
        </div>

        <!-- Open a specific branch in a known project -->
        {#if projects.length > 0}
          <div class="rounded-xl border border-border bg-surface p-4 mb-8">
            <div class="flex items-center gap-3 mb-3">
              <div class="w-9 h-9 rounded-lg bg-hover border border-border flex items-center justify-center">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-fg-2"><circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M6 9v6"/><path d="M13 6h3a2 2 0 0 1 2 2v3"/><path d="m15 9 3 2 3-2"/></svg>
              </div>
              <div>
                <div class="font-medium">Open a specific branch</div>
                <p class="text-sm text-fg-3">Pick a known project and switch to a branch on entry.</p>
              </div>
            </div>
            <div class="flex items-center gap-2">
              <select
                bind:value={selectedProjectId}
                class="flex-1 bg-bg border border-hairline rounded-md px-2 py-1.5 text-sm text-fg outline-none"
              >
                {#each projects as p (p.id)}
                  <option value={p.id}>{p.name}</option>
                {/each}
              </select>
              <input
                bind:value={branchName}
                onkeydown={(e) => e.key === "Enter" && openProjectBranch()}
                placeholder="branch-name"
                class="flex-1 bg-bg border border-hairline rounded-md px-2 py-1.5 text-sm text-fg outline-none placeholder:text-muted mono"
              />
              <button
                onclick={openProjectBranch}
                disabled={!branchName.trim()}
                class="px-3 py-1.5 rounded-md bg-accent hover:bg-accent/90 disabled:opacity-40 disabled:cursor-not-allowed text-on-accent text-xs font-medium"
              >Open</button>
            </div>
          </div>
        {/if}

        <div class="border-t border-hairline pt-6">
          <div class="text-xs uppercase tracking-wider text-muted mb-3">Tips</div>
          <ul class="space-y-1.5 text-sm text-fg-3">
            <li class="flex items-start gap-2"><span class="kbd mt-0.5">⌘K</span><span>Command palette — every action is reachable with the keyboard.</span></li>
            <li class="flex items-start gap-2"><span class="kbd mt-0.5">b</span><span>Open a browser tab and annotate any DOM element to attach to a review.</span></li>
            <li class="flex items-start gap-2"><span class="kbd mt-0.5">⌘E</span><span>Export a review as a markdown prompt to hand off to your coding agent.</span></li>
          </ul>
        </div>
      </div>
    </main>
  </div>
</div>
