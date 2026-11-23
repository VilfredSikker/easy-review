import { invoke } from "@tauri-apps/api/core";
import { resourceClient } from "$lib/protocol/client";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { PendingReview, ReviewLineTarget } from "$lib/reviewSubmit";
import { app } from "./app.svelte";
import { GithubWrites, reviewLine } from "./githubWrites";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const client = resourceClient();

/** The GitHub senders, through `er_send`. */
export const githubWrites = new GithubWrites({ store: resources, activeKey: () => tabsResource.activeKey });

app.routeCmds((command, args) => githubWrites.route(command, args));

/** The active tab's key, which the pending review is read and written on; null before the tab strip arrives. */
function sentTab(): TabKey | null {
  return tabsResource.activeKey;
}

/** A refusal as the old commands reject: its text. */
async function asText<T>(run: () => Promise<T>): Promise<T> {
  try {
    return await run();
  } catch (e) {
    throw (e as ErError).message ?? String(e);
  }
}

/** The viewer's pending review on the active tab's PR. */
export async function getPendingReview(): Promise<PendingReview> {
  const tab = sentTab();
  if (tab === null) return invoke<PendingReview>("get_pending_review");
  const [entry] = await asText(() => client.fetch([{ kind: "pending_review", tab }]));
  if (entry.value.kind !== "pending_review") throw new Error("not a pending review");
  return entry.value as unknown as PendingReview;
}

/** Adds a line comment to the pending review and reads it back. */
export async function addPendingReviewComment(target: ReviewLineTarget, text: string): Promise<PendingReview> {
  const tab = sentTab();
  if (tab === null) return invoke<PendingReview>("add_pending_review_comment", { target, text });
  await asText(() => githubWrites.send({ kind: "add_pending_review_comment", tab, target: reviewLine(target), body: text }));
  return getPendingReview();
}

/** Removes one comment from the pending review and reads it back. */
export async function removePendingReviewComment(commentId: string): Promise<PendingReview> {
  const tab = sentTab();
  if (tab === null) return invoke<PendingReview>("remove_pending_review_comment", { commentId });
  await asText(() => githubWrites.send({ kind: "remove_pending_review_comment", tab, commentId }));
  return getPendingReview();
}
