import { invoke } from "@tauri-apps/api/core";
import { resourceClient } from "$lib/protocol/client";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { PendingReview, ReviewLineTarget } from "$lib/reviewSubmit";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app } from "./app.svelte";
import { GithubWrites, reviewLine } from "./githubWrites";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";

const flagOn = v1ServiceRequested();
const client = resourceClient();

/** The GitHub senders, through `er_send` with the flag on. */
export const githubWrites = new GithubWrites({ flagOn, store: resources, activeKey: () => tabsResource.activeKey });

if (flagOn) app.routeCmds((command, args) => githubWrites.route(command, args));

/** The active tab's key while the pending review goes through the resources; null with the flag off. */
function sentTab(): TabKey | null {
  return flagOn ? tabsResource.activeKey : null;
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
