<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { copyToClipboard } from "$lib/clipboard";

  interface RevisionSummary {
    revision_id: string;
    created_at: string;
    scope: string;
    diff_hash: string;
    active: boolean;
    agents: string[];
  }

  let revisions = $state<RevisionSummary[]>([]);
  let selectedRevisionId = $state<string>("");

  const commands = $derived(app.snapshot?.agent_commands ?? []);
  const log = $derived(app.snapshot?.agent_log ?? []);

  async function loadRevisions() {
    revisions = await invoke<RevisionSummary[]>("list_review_revisions");
    const preferred = revisions.find((r) => r.active) ?? revisions[0];
    selectedRevisionId = preferred?.revision_id ?? "";
  }

  async function copyReviewJson() {
    try {
      const content = await invoke<string>("read_review_json", { revisionId: selectedRevisionId || null });
      await copyToClipboard(content);
      app.showToast("success", `Copied review.json (${content.length} bytes)`);
    } catch {
      app.showToast("error", "No review.json found for selected revision");
    }
  }

  $effect(() => {
    app.snapshot?.active_tab;
    void loadRevisions();
  });
</script>

<div class="flex-1 min-w-0 overflow-hidden flex flex-col">
  <div class="h-10 px-4 border-b border-hairline bg-ink-870 flex items-center gap-2 text-sm">
    <span class="text-fg-2">Agent Output</span>
    {#if revisions.length > 0}
      <select bind:value={selectedRevisionId} class="ml-2 bg-bg border border-border rounded px-2 py-1 text-xs">
        {#each revisions as rev}
          <option value={rev.revision_id}>{rev.active ? "active · " : ""}{rev.revision_id}</option>
        {/each}
      </select>
    {/if}
    <button class="ml-auto px-2 py-1 text-xs border border-border rounded hover:bg-hover" onclick={copyReviewJson}>
      Copy review.json
    </button>
    <button class="px-2 py-1 text-xs border border-border rounded hover:bg-hover" onclick={() => app.setMainView("diff")}>
      Back to diff
    </button>
  </div>

  <div class="flex-1 overflow-y-auto p-4 space-y-4">
    {#if commands.length === 0 && log.length === 0}
      <div class="text-sm text-muted">No agent output yet</div>
    {/if}
    {#if commands.length > 0}
      <div class="rounded border border-hairline bg-card p-3">
        <div class="text-xs text-fg-3 mb-2">Command status</div>
        {#each commands as cmd}
          <div class="flex items-center text-xs font-mono mb-1">
            <span>{cmd.name}</span>
            <span class="ml-auto">{cmd.status}</span>
          </div>
        {/each}
      </div>
    {/if}
    {#if log.length > 0}
      <div class="rounded border border-hairline bg-card p-3">
        <div class="text-xs text-fg-3 mb-2">Log</div>
        {#each log as entry}
          <div class="text-[11px] font-mono break-all">{entry.text}</div>
        {/each}
      </div>
    {/if}
  </div>
</div>
