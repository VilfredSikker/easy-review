import { untrack } from "svelte";
import { AnnotationDraftCache } from "$lib/annotationDraftCache";
import type { AppSnapshot } from "$lib/types";

const cache = new AnnotationDraftCache();
function createDraft() {
  const draft = $state({ replyText: "", showReply: false, askAiText: "", showAskAi: false });
  return draft;
}
export const annotationDrafts = {
  get: (snapshot: AppSnapshot | null, kind: "thread" | "finding", id: string) => untrack(() =>
    cache.get(snapshot, kind, id, createDraft),
  ),
  sync: (snapshot: AppSnapshot | null) => untrack(() => cache.sync(snapshot)),
};
