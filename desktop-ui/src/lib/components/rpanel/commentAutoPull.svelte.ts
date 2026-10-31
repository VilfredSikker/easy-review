import { app } from "$lib/stores/app.svelte";
import { commentAutoPullKey } from "$lib/prUrl";

// Keys already pulled this session. Module-level so remounting the panel (it
// is keyed on the view identity) does not pull the same PR's comments again.
// eslint-disable-next-line svelte/prefer-svelte-reactivity -- once-per-PR cache, never read reactively
const pulledKeys = new Set<string>();
const MAX_KEYS = 128;

function remember(key: string) {
  pulledKeys.add(key);
  if (pulledKeys.size > MAX_KEYS) {
    const oldest = pulledKeys.values().next().value;
    if (oldest) pulledKeys.delete(oldest);
  }
}

/**
 * Pull a PR's GitHub comments once, the first time the right panel shows it.
 * The panel owns this rather than one tab, so the Comments count in the tab
 * strip is right before the reader opens that tab. Call during component init.
 */
export function useCommentAutoPull(): void {
  let inFlight = $state<string | null>(null);

  $effect(() => {
    if (app.pendingTabSwitch) return;
    const key = commentAutoPullKey(app.snapshot);
    if (!key || inFlight === key || pulledKeys.has(key)) return;
    inFlight = key;
    void (async () => {
      await app.cmd("pull_github_comments");
      // A tab switch mid-pull means the result belongs to another view; leave
      // the key unremembered so this PR pulls again when it comes back.
      if (!app.pendingTabSwitch && commentAutoPullKey(app.snapshot) === key) remember(key);
      if (inFlight === key) inFlight = null;
    })();
  });
}
