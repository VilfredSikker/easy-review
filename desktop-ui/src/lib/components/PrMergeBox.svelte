<script lang="ts">
  import type { GithubStatusSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { anchoredMenuPosition } from "$lib/anchoredMenu";
  import AnchoredMenu from "$lib/components/ui/AnchoredMenu.svelte";
  import {
    confirmLabel,
    effectiveMerge,
    mergeBoxModel,
    mergeButtonLabel,
    mergeHeadNote,
    mergeRequest,
    methodLabel,
    pickMethod,
    prTarget,
    type MenuItem,
    type MergeBoxTone,
    type MergeMethod,
    type PrActionKind,
    type PrActionRequest,
    type PrTarget,
  } from "$lib/prMergeBox";

  interface Props {
    github: GithubStatusSnapshot;
  }

  const { github }: Props = $props();

  const model = $derived(mergeBoxModel(github));
  const prDiffHead = $derived(app.snapshot?.pr_diff_head_oid ?? null);
  const headNote = $derived(mergeHeadNote(github, prDiffHead));

  // ── Menus (fixed: the right panel clips overflow) ──────────────────────
  let methodMenu = $state<{ top: number; left: number } | null>(null);
  let overflowMenu = $state<{ top: number; left: number } | null>(null);

  function anchor(e: MouseEvent) {
    const el = e.currentTarget as HTMLElement;
    return anchoredMenuPosition(el.getBoundingClientRect(), window.innerWidth);
  }

  // ── Merge method, remembered per repo ───────────────────────────────────
  const methodKey = $derived(`er.mergeMethod.${github.owner}/${github.repo}`);
  let rememberedMethod = $state<string | null>(null);
  $effect(() => {
    try {
      rememberedMethod = localStorage.getItem(methodKey);
    } catch {
      rememberedMethod = null;
    }
  });
  const method = $derived(pickMethod(model.methods, rememberedMethod));

  function chooseMethod(m: MergeMethod) {
    rememberedMethod = m;
    methodMenu = null;
    try {
      localStorage.setItem(methodKey, m);
    } catch {
      // Private window or blocked storage: the choice lasts for this session.
    }
  }

  // ── Running actions ────────────────────────────────────────────────────
  let busy = $state<PrActionKind | null>(null);
  let confirming = $state<PrActionRequest | null>(null);
  // The PR the confirm was built for; the backend refuses if it is no longer
  // the active tab's PR.
  let confirmTarget = $state<PrTarget | null>(null);
  let deleteAfterMerge = $state(true);
  // "Merge without waiting for requirements": GitHub's bypass-rules checkbox.
  let bypass = $state(false);
  const merge = $derived(effectiveMerge(model, bypass));
  // GitHub would refuse a merge pinned to a head the PR has moved past.
  const mergeBlocked = $derived(confirming?.kind === "merge" && headNote?.blocksMerge === true);

  async function run(action: PrActionRequest, pr: PrTarget) {
    if (busy) return;
    busy = action.kind;
    confirming = null;
    overflowMenu = null;
    try {
      await app.cmd("run_github_pr_action", { pr, action });
    } finally {
      busy = null;
    }
  }

  function ask(action: PrActionRequest) {
    confirming = action;
    confirmTarget = prTarget(github);
  }

  function pick(item: MenuItem) {
    overflowMenu = null;
    if (item.destructive) ask(item.action);
    else void run(item.action, prTarget(github));
  }

  function startMerge() {
    if (!merge?.enabled) return;
    const del = model.offerDeleteOnMerge && deleteAfterMerge;
    const bypassNow = bypass && model.offerBypass;
    ask(mergeRequest(github, method, { auto: merge.auto, deleteBranch: del, prDiffHeadOid: prDiffHead, bypass: bypassNow }));
  }

  function confirmNow() {
    if (!confirming || !confirmTarget || mergeBlocked) return;
    const del = model.offerDeleteOnMerge && deleteAfterMerge;
    const action =
      confirming.kind === "merge"
        ? mergeRequest(github, confirming.method, {
            auto: confirming.auto,
            deleteBranch: del,
            prDiffHeadOid: prDiffHead,
            bypass: confirming.admin,
          })
        : confirming;
    void run(action, confirmTarget);
  }

  // Drop a pending confirm when the PR it was built for changes underneath.
  // Keyed on a derived string: `github` is a new object on every poll, so
  // reading it directly would cancel the confirm every 30s.
  const confirmKey = $derived(
    `${github.owner}/${github.repo}#${github.number}:${github.head_oid ?? ""}:${github.state}`,
  );
  $effect(() => {
    void confirmKey;
    confirming = null;
    bypass = false;
  });

  const TONE: Record<MergeBoxTone, { dot: string; text: string; border: string }> = {
    ok: { dot: "bg-add-fg", text: "text-add-fg", border: "border-add-fg/40" },
    warn: { dot: "bg-risk-med", text: "text-risk-med", border: "border-risk-med/40" },
    danger: { dot: "bg-del-fg", text: "text-del-fg", border: "border-del-fg/40" },
    muted: { dot: "bg-fg-3", text: "text-fg-2", border: "border-hairline" },
    merged: { dot: "bg-periwinkle", text: "text-periwinkle", border: "border-periwinkle/40" },
  };
  const tone = $derived(TONE[model.tone]);
  const mergeTone = $derived(bypass && model.offerBypass ? "bg-del-fg" : "bg-add-fg");

  const btn =
    "px-2 py-1 rounded text-[11px] font-medium disabled:opacity-50 disabled:cursor-not-allowed transition-opacity";
  const ghostBtn = `${btn} border border-hairline bg-card text-fg-2 hover:text-fg-1 hover:border-border`;
</script>

<div class="rounded-md border {tone.border} bg-card" data-testid="merge-box">
  <!-- Headline -->
  <div class="flex items-start gap-2 px-2 py-1.5">
    <span class="mt-[5px] w-1.5 h-1.5 rounded-full shrink-0 {tone.dot}"></span>
    <div class="flex-1 min-w-0">
      <div class="text-[12px] font-medium {tone.text}">{model.headline}</div>
      {#if model.detail}
        <div class="text-[11px] text-muted truncate" title={model.detail}>{model.detail}</div>
      {/if}
    </div>
    {#if model.menu.length > 0}
      <button
        type="button"
        onclick={(e) => (overflowMenu = overflowMenu ? null : anchor(e))}
        disabled={busy !== null}
        aria-label="More pull request actions"
        aria-haspopup="menu"
        aria-expanded={overflowMenu !== null}
        title="More pull request actions"
        class="p-1 rounded text-muted hover:text-fg-2 hover:bg-fg-3/10 disabled:opacity-50"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <circle cx="5" cy="12" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="19" cy="12" r="2"/>
        </svg>
      </button>
    {/if}
  </div>

  {#if model.canWrite}
    <div class="border-t border-hairline px-2 py-1.5 flex flex-col gap-1.5">
      {#if confirming}
        <!-- Confirm step -->
        <div class="text-[11px] text-fg-1">{confirmLabel(confirming, github)}</div>
        {#if confirming.kind === "merge"}
          {#if !confirming.auto}
            {#if model.offerDeleteOnMerge}
              <label class="flex items-center gap-1.5 text-[11px] text-fg-2 cursor-pointer">
                <input type="checkbox" bind:checked={deleteAfterMerge} />
                Delete <span class="font-mono truncate">{github.head_ref}</span> after merge
              </label>
            {:else if github.repo_merge?.delete_branch_on_merge}
              <div class="text-[10px] text-muted">GitHub deletes the branch after merge (repo setting).</div>
            {:else if model.mergeQueue}
              <div class="text-[10px] text-muted">
                The queue merges it with the repository's method. Delete the branch here once it has merged.
              </div>
            {/if}
          {/if}
          <!-- Auto-merge pins the head too, so the note applies to both. -->
          {#if headNote}
            <div class="text-[10px] {headNote.warn ? 'text-warning' : 'text-muted'}">{headNote.text}</div>
          {/if}
        {/if}
        <div class="flex items-center gap-1.5">
          <button
            type="button"
            onclick={confirmNow}
            disabled={busy !== null || mergeBlocked}
            class="{btn} {confirming.kind === 'merge' && !confirming.admin ? 'bg-add-fg' : 'bg-del-fg'} text-on-accent hover:opacity-90"
          >{confirming.kind === "merge" ? "Confirm merge" : "Confirm"}</button>
          <button
            type="button"
            onclick={() => (confirming = null)}
            disabled={busy !== null}
            class="text-[11px] text-muted hover:text-fg-2 px-2 py-1 rounded disabled:opacity-50"
          >Cancel</button>
        </div>
      {:else if merge}
        <!-- Merge split button -->
        <div class="flex items-center gap-1.5 flex-wrap">
          <div class="inline-flex rounded overflow-hidden">
            <button
              type="button"
              onclick={startMerge}
              disabled={!merge.enabled || busy !== null}
              title={merge.reason ?? methodLabel(method)}
              class="{btn} rounded-none {mergeTone} text-on-accent hover:opacity-90"
            >{busy === "merge" ? "Merging…" : mergeButtonLabel(method, merge.auto, model.mergeQueue)}</button>
            {#if model.methods.length > 1}
              <button
                type="button"
                onclick={(e) => (methodMenu = methodMenu ? null : anchor(e))}
                disabled={busy !== null}
                aria-label="Choose merge method"
                aria-haspopup="menu"
                aria-expanded={methodMenu !== null}
                class="{btn} rounded-none {mergeTone} text-on-accent hover:opacity-90 border-l border-black/20 px-1.5"
              >
                <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>
              </button>
            {/if}
          </div>
          {#if model.updateProminent}
            <button
              type="button"
              onclick={() => run({ kind: "update_branch", rebase: false }, prTarget(github))}
              disabled={busy !== null}
              class={ghostBtn}
              title="Merge {github.base_ref} into {github.head_ref} on GitHub"
            >{busy === "update_branch" ? "Updating…" : "Update branch"}</button>
          {/if}
        </div>
        {#if merge.reason}
          <div class="text-[10px] text-muted">{merge.reason}</div>
        {/if}
        {#if model.offerBypass}
          <label class="flex items-start gap-1.5 text-[11px] text-del-fg cursor-pointer">
            <input type="checkbox" class="mt-[2px]" bind:checked={bypass} disabled={busy !== null} />
            Merge without waiting for requirements to be met (bypass rules)
          </label>
        {/if}
      {:else if model.readyPrimary}
        <div>
          <button
            type="button"
            onclick={() => run({ kind: "mark_ready" }, prTarget(github))}
            disabled={busy !== null}
            class="{btn} bg-accent text-on-accent hover:opacity-90"
          >{busy === "mark_ready" ? "Marking ready…" : "Ready for review"}</button>
        </div>
      {:else if model.autoMerge}
        <div>
          <button
            type="button"
            onclick={() => run({ kind: "disable_auto_merge" }, prTarget(github))}
            disabled={busy !== null}
            class={ghostBtn}
          >{busy === "disable_auto_merge" ? "Disabling…" : "Disable auto-merge"}</button>
        </div>
      {:else if model.primaryActions.length > 0}
        <div class="flex items-center gap-1.5 flex-wrap">
          {#each model.primaryActions as item (item.action.kind)}
            <button
              type="button"
              onclick={() => pick(item)}
              disabled={busy !== null}
              class={item.destructive ? `${btn} border border-del-fg/40 text-del-fg hover:bg-del-fg/10` : ghostBtn}
            >{busy === item.action.kind ? "Working…" : item.label}</button>
          {/each}
        </div>
      {/if}
    </div>
  {:else if model.phase === "open"}
    <div class="border-t border-hairline px-2 py-1.5 text-[10px] text-muted">
      You need write access to merge or change this pull request.
    </div>
  {/if}
</div>

{#if methodMenu}
  <AnchoredMenu pos={methodMenu} onClose={() => (methodMenu = null)}>
    {#each model.methods as m (m)}
      <button
        type="button"
        role="menuitemradio"
        aria-checked={m === method}
        onclick={() => chooseMethod(m)}
        class="w-full text-left px-3 py-1.5 text-[12px] flex items-center gap-2 text-ink-100 hover:bg-ink-700"
      >
        <span class="w-3 shrink-0 inline-flex items-center justify-center">
          {#if m === method}
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-accent" aria-hidden="true"><path d="M5 13l4 4L19 7"/></svg>
          {/if}
        </span>
        {methodLabel(m)}
      </button>
    {/each}
  </AnchoredMenu>
{/if}

{#if overflowMenu}
  <AnchoredMenu pos={overflowMenu} onClose={() => (overflowMenu = null)}>
    {#each model.menu as item, i (i)}
      <button
        type="button"
        role="menuitem"
        onclick={() => pick(item)}
        class="w-full text-left px-3 py-1.5 text-[12px] hover:bg-ink-700 {item.destructive ? 'text-del-fg' : 'text-ink-100'}"
      >{item.label}</button>
    {/each}
  </AnchoredMenu>
{/if}
