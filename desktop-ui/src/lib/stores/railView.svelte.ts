import { pageView } from "./pageView.svelte";
import { tabsResource } from "./tabsResource.svelte";
import { sessionResource } from "./sessionResource.svelte";
import { localScopes, viewIsPr, type LocalScope } from "$lib/scopeRail";
import type { TabSummary } from "$lib/pageTypes";

/**
 * Per tab, whether the last view seen outside History was the PR diff view.
 *
 * The engine returns History to the view it was entered from
 * (`history_return_mode`) but does not send which. The rail needs it to label
 * "All changes" and to keep the local scope segment off a PR commit. Module
 * scope, so it survives the rail unmounting while the Overview is shown.
 */
const lastSourceWasPr = $state<Record<string, boolean>>({});

function tabKey(tab: TabSummary | null): string {
  if (!tab) return "";
  return `${tab.idx}:${tab.repo_root}:${tab.branch ?? ""}:${tab.pr_number ?? ""}`;
}

/** Shared view facts for the scope segment and the commits section. Call
 *  during component init: it registers the effect that records the source. */
export function railView() {
  const snapshot = $derived(pageView.current);
  const mode = $derived(snapshot?.mode ?? "branch");
  const activeTab = $derived(tabsResource.activeTab);
  const key = $derived(tabKey(activeTab));
  const remote = $derived(activeTab?.kind === "remote_pr");
  const readOnly = $derived(
    remote || (activeTab?.kind === "local_branch" && !snapshot?.local_branch_checked_out),
  );
  const tourScope = $derived(snapshot?.tour?.scope);
  const isPr = $derived(viewIsPr(mode, tourScope, remote, lastSourceWasPr[key] ?? false));
  const scopes = $derived<LocalScope[]>(
    localScopes({ remote, readOnly, features: sessionResource.features }),
  );

  $effect(() => {
    if (!snapshot || mode === "history") return;
    const wasPr = viewIsPr(mode, tourScope, remote, false);
    if (lastSourceWasPr[key] !== wasPr) lastSourceWasPr[key] = wasPr;
  });

  return {
    get mode() {
      return mode;
    },
    get remote() {
      return remote;
    },
    get isPr() {
      return isPr;
    },
    get scopes() {
      return scopes;
    },
  };
}
