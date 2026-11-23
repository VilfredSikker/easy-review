import { invoke } from "@tauri-apps/api/core";
import { resourceClient } from "$lib/protocol/client";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { ReviewRevision } from "$lib/protocol/generated/ReviewRevision";
import { annotationsResource } from "./annotationsResource.svelte";

const client = resourceClient();

/** One key read on fetch; a refusal rejects with its text, as the old commands do. */
async function fetchOne(key: ResourceKey): Promise<Resource> {
  try {
    const [entry] = await client.fetch([key]);
    return entry.value;
  } catch (e) {
    throw (e as ErError).message;
  }
}

/**
 * The active tab's review as the reviewer sees it, as JSON. The fetch answers
 * none for a tab with no review; that rejects, as the old command does.
 */
export async function readReviewJson(): Promise<string> {
  const tab = annotationsResource.fetchedTab();
  if (tab === null) return invoke<string>("read_review_json", { revisionId: null });
  const value = await fetchOne({ kind: "review_json", tab });
  if (value.kind === "review_json" && value.json !== null) return value.json;
  throw new Error("No review.json found");
}

/** The active tab's review revisions: at most the current one. */
export async function listReviewRevisions(): Promise<ReviewRevision[]> {
  const tab = annotationsResource.fetchedTab();
  if (tab === null) return invoke<ReviewRevision[]>("list_review_revisions");
  const value = await fetchOne({ kind: "review_revisions", tab });
  return value.kind === "review_revisions" ? value.revisions : [];
}

/** A saved annotation screenshot as a `data:` URL. */
export async function readAnnotationScreenshot(path: string): Promise<string> {
  const value = await fetchOne({ kind: "annotation_screenshot", path });
  return value.kind === "annotation_screenshot" ? value.data_url : "";
}
