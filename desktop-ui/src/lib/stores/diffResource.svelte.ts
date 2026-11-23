import { asErError, resourceClient } from "$lib/protocol/client";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { FileSnapshot } from "$lib/pageTypes";
import { diffPreview } from "./diffPreview.svelte";
import { imagePreview } from "./imagePreview.svelte";
import { annotationsResource } from "./annotationsResource.svelte";
import { app } from "./app.svelte";
import { DiffAdapter, diffKey, prListRow, type PrHead } from "./diffAdapter";
import { hunksResource } from "./hunksResource.svelte";
import { pageView } from "./pageView.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

function activePrHead(): PrHead | null {
  const tab = tabsResource.activeTab;
  const row = prListRow(resources, tab?.remote ?? null, tab?.pr_number ?? null);
  return row ? { head_oid: row.headOid, head_ref: row.headRef, base_ref: row.baseRef } : null;
}

/** The active tab's file list, mode, base and staleness, from `diff(tab)`. */
export const diffResource = new DiffAdapter({
  store: resources,
  activeKey: () => tabsResource.activeKey,
  keyAt: (idx) => tabsResource.keyAt(idx),
  prHead: activePrHead,
  // The threads come from `annotations(tab)` joined with `jobs` in every view.
  hunks: {
    get: (hash) => hunksResource.get(hash),
    threads: () => annotationsResource.threads() ?? [],
  },
});

/**
 * Fetches the active tab's hunks for `files` by the hashes their rows name,
 * once its diff is held; false before then.
 */
export async function loadHashedHunks(files: readonly FileSnapshot[]): Promise<boolean> {
  const tab = tabsResource.activeKey;
  if (!diffResource.hashed() || tab === null) return false;
  await hunksResource.load(tab, files.map((f) => ({ path: f.path, hash: f.cache_key })));
  return true;
}

/** A refused read, shown by its message alone, as the old commands' errors were. */
class ReadRefused extends Error {
  override toString(): string {
    return this.message;
  }
}

/** One `er_fetch` of a key read on each fetch and never pushed. */
async function readOnce<K extends Resource["kind"]>(key: ResourceKey, kind: K): Promise<Extract<Resource, { kind: K }>> {
  let value: Resource | undefined;
  try {
    value = (await resourceClient().fetch([key]))[0]?.value;
  } catch (e) {
    throw new ReadRefused(asErError(e).message);
  }
  if (value?.kind !== kind) throw new ReadRefused(`expected ${kind}`);
  return value as Extract<Resource, { kind: K }>;
}

/** Every path the active diff holds, from the content. */
export async function readDiffPaths(): Promise<string[]> {
  const tab = tabsResource.activeKey;
  if (tab === null) return [];
  return (await readOnce({ kind: "diff_paths", tab }, "diff_paths")).paths;
}

/** Shows `key` while the effect that called this runs. */
function showDiff(key: TabKey | null): (() => void) | undefined {
  return key === null ? undefined : resources.show(diffKey(key));
}

// The page reads the diff fields from `diff(tab)`, the diff commands go
// through er_send, the active tab's diff is shown, and so is the diff and PR
// list of every PR tab in the strip, whose built-from head the stale mark
// compares with the list's.
pageView.overlay((raw) => diffResource.overlay(raw));
app.routeCmds((command, args) => diffResource.route(command, args));
diffPreview.route((path, context) => {
  const tab = tabsResource.activeKey;
  return tab === null ? null : readOnce({ kind: "file_preview", tab, path, context }, "file_preview");
});
imagePreview.route((path, context) => {
  const tab = tabsResource.activeKey;
  return tab === null ? null : readOnce({ kind: "image_preview", tab, path, context }, "image_preview");
});
$effect.root(() => {
  $effect(() => showDiff(tabsResource.activeKey));
  $effect(() => {
    const releases = tabsResource.prRows().flatMap(({ key, remote }) => [
      resources.show(diffKey(key)),
      resources.show({ kind: "prs", remote }),
    ]);
    return () => {
      for (const release of releases) release();
    };
  });
});
