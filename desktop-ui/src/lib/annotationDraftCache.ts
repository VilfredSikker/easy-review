import type { AppSnapshot, TabSummary } from "./types";

function reviewKey(tab: TabSummary): string {
  return JSON.stringify([tab.repo_root, tab.remote, tab.kind, tab.pr_number == null ? tab.branch : null, tab.pr_number]);
}

export interface AnnotationDraft {
  replyText: string;
  showReply: boolean;
  askAiText: string;
  showAskAi: boolean;
}

/** Review-owned drafts outlive virtualized annotation cards. */
export class AnnotationDraftCache {
  private drafts = new Map<string, AnnotationDraft>();

  get(snapshot: AppSnapshot | null, kind: "thread" | "finding", id: string,
    create: () => AnnotationDraft = () => ({ replyText: "", showReply: false, askAiText: "", showAskAi: false }),
  ): AnnotationDraft {
    const tab = snapshot?.tabs.find((t) => t.is_active) ?? snapshot?.tabs[snapshot.active_tab];
    const review = tab ? reviewKey(tab) : "no-review";
    const key = `${review}\0${snapshot?.mode ?? ""}\0${snapshot?.selected_commit_sha ?? ""}\0${kind}\0${id}`;
    let draft = this.drafts.get(key);
    if (!draft) {
      draft = create();
      this.drafts.set(key, draft);
    }
    return draft;
  }

  sync(snapshot: AppSnapshot | null): void {
    if (!snapshot) {
      this.drafts.clear();
      return;
    }
    const open = new Set(snapshot.tabs.map(reviewKey));
    for (const key of this.drafts.keys()) {
      if (!open.has(key.split("\0")[0])) this.drafts.delete(key);
    }
  }
}
