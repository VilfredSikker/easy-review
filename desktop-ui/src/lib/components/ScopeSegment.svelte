<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { railView } from "$lib/stores/railView.svelte";
  import { pressedScope, type LocalScope } from "$lib/scopeRail";

  const view = railView();
  const snapshot = $derived(app.snapshot);

  const LABELS: Record<LocalScope, string> = {
    branch: "Branch",
    unstaged: "Unstaged",
    staged: "Staged",
  };

  // One option is no choice, and the PR diff view has no working tree to scope.
  const visible = $derived(view.scopes.length > 1 && !view.isPr);

  const pressed = $derived(pressedScope(view.mode));

  function fileCount(scope: LocalScope): number | null {
    if (scope === "unstaged") return snapshot?.unstaged_stat?.files ?? null;
    if (scope === "staged") return snapshot?.staged_stat?.files ?? null;
    return null;
  }

  function choose(scope: LocalScope) {
    if (scope === pressed && view.mode !== "history") return;
    void app.cmd("set_mode", { mode: scope });
  }
</script>

{#if visible}
  <div
    class="flex p-[2px] rounded-[7px] bg-hover"
    role="group"
    aria-label="Local scope"
  >
    {#each view.scopes as scope (scope)}
      {@const count = fileCount(scope)}
      {@const on = pressed === scope}
      <button
        type="button"
        class="flex-1 min-w-0 px-[9px] py-[2px] rounded-[5px] text-[12px] font-medium whitespace-nowrap {on
          ? 'bg-card text-fg shadow-[0_0_0_1px_var(--color-border)]'
          : 'text-fg-2 hover:text-fg'}"
        aria-pressed={on}
        onclick={() => choose(scope)}
      >
        {LABELS[scope]}{#if count != null}&nbsp;<span class="text-muted">{count}</span>{/if}
      </button>
    {/each}
  </div>
{/if}
