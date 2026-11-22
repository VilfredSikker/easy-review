import { invoke } from "@tauri-apps/api/core";
import { asErError, resourceClient } from "$lib/protocol/client";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { FileSnapshot } from "$lib/types";
import { diffPreview } from "./diffPreview.svelte";
import { imagePreview } from "./imagePreview.svelte";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { DiffAdapter, diffKey, prListRow, type PrHead } from "./diffAdapter";
import { hunksResource } from "./hunksResource.svelte";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();

function activePrHead(): PrHead | null {
  const tab = tabsResource.activeTab;
  const row = prListRow(resources, tab?.remote ?? null, tab?.pr_number ?? null);
  return row ? { head_oid: row.headOid, head_ref: row.headRef, base_ref: row.baseRef } : null;
}

/** The active tab's file list, mode, base and staleness, from the snapshot or `diff(tab)` by build. */
export const diffResource = new DiffAdapter({
  flagOn,
  store: resources,
  snapshot: () => app.appSnapshot,
  activeKey: () => tabsResource.activeKey,
  keyAt: (idx) => tabsResource.keyAt(idx),
  prHead: activePrHead,
  // The threads come from the App's snapshot until `annotations(tab)` serves them.
  hunks: { get: (hash) => hunksResource.get(hash), threads: () => app.appSnapshot?.ai?.threads ?? [] },
});

/**
 * Fetches the active tab's hunks for `files` by the hashes their rows name,
 * when the page draws them that way; false when the App's snapshot carries them.
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

/** The tab whose previews and paths come from its content: the active one, while the page draws it by hash. */
function contentTab(): TabKey | null {
  return diffResource.hashed() ? tabsResource.activeKey : null;
}

/** Every path the active diff holds, from the content or the App by view. */
export async function readDiffPaths(): Promise<string[]> {
  const tab = contentTab();
  if (tab === null) return invoke<string[]>("list_diff_paths");
  return (await readOnce({ kind: "diff_paths", tab }, "diff_paths")).paths;
}

/** Shows `key` while the effect that called this runs. */
function showDiff(key: TabKey | null): (() => void) | undefined {
  return key === null ? undefined : resources.show(diffKey(key));
}

// With the flag on the page reads the diff fields from `diff(tab)`, the diff
// commands go through er_send, the active tab's diff is shown, and so is the
// diff and PR list of every PR tab in the strip, whose built-from head the
// stale mark compares with the list's.
if (flagOn) {
  app.overlaySnapshot((raw) => diffResource.overlay(raw));
  app.routeCmds((command, args) => diffResource.route(command, args));
  diffPreview.route((path, context) => {
    const tab = contentTab();
    return tab === null ? null : readOnce({ kind: "file_preview", tab, path, context }, "file_preview");
  });
  imagePreview.route((path, context) => {
    const tab = contentTab();
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
}
