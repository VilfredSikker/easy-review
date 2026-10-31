import { invoke } from "@tauri-apps/api/core";
import { app } from "./app.svelte";
import {
  addedToast,
  decisionEvent,
  initialDecision,
  pendingThreadIds,
  submittedToast,
  type PendingReview,
  type ReviewDecision,
  type ReviewLineTarget,
} from "$lib/reviewSubmit";

/**
 * The reviewer's GitHub pending review for the active PR, plus the Review
 * popover's open state and draft. GitHub owns the pending comments; this
 * re-reads them after every change rather than painting them ahead, since
 * each change is a network round trip that can be refused (ADR 0049).
 */
class PendingReviewStore {
  review = $state<PendingReview | null>(null);
  /** The PR `review` was read for (`pendingReviewKey`). */
  key = $state<string | null>(null);
  loading = $state(false);
  busy = $state(false);
  error = $state<string | null>(null);

  open = $state(false);
  decision = $state<ReviewDecision>("approve");
  summary = $state("");

  get count(): number {
    return this.review?.comments.length ?? 0;
  }

  get threadIds(): Set<string> {
    return pendingThreadIds(this.review);
  }

  isPending(threadId: string): boolean {
    return this.threadIds.has(threadId);
  }

  /** Follow the active PR. A different PR drops the old draft and its summary. */
  follow(key: string | null): void {
    if (key === this.key) return;
    this.key = key;
    this.review = null;
    this.error = null;
    this.summary = "";
    this.open = false;
    if (key) void this.refresh();
  }

  async refresh(): Promise<void> {
    const key = this.key;
    if (!key) return;
    this.loading = true;
    try {
      const review = await invoke<PendingReview>("get_pending_review");
      if (this.key === key) {
        this.review = review;
        this.error = null;
      }
    } catch (e) {
      if (this.key === key) this.error = String(e);
    } finally {
      if (this.key === key) this.loading = false;
    }
  }

  show(): void {
    this.decision = initialDecision(app.snapshot?.github);
    this.open = true;
    void this.refresh();
  }

  toggle(): void {
    if (this.open) this.open = false;
    else this.show();
  }

  /** "Start a review" / "Add review comment". True when GitHub took it. */
  async add(target: ReviewLineTarget, text: string): Promise<boolean> {
    const key = this.key;
    this.busy = true;
    try {
      const review = await invoke<PendingReview>("add_pending_review_comment", { target, text });
      if (this.key === key) this.review = review;
      app.showToast("success", addedToast(review.comments.length), undefined, {
        action: { label: "Review", onClick: () => this.show() },
      });
      void app.cmd("pull_github_comments", { force: true });
      return true;
    } catch (e) {
      app.showToast("error", String(e));
      return false;
    } finally {
      this.busy = false;
    }
  }

  /** "Add single comment": posted now, outside the review. */
  async postSingle(target: ReviewLineTarget, text: string): Promise<boolean> {
    this.busy = true;
    try {
      await app.cmd("post_github_line_comment", { target, text }, { throwOnError: true });
      app.showToast("success", "Comment posted to GitHub");
      void app.cmd("pull_github_comments", { force: true });
      return true;
    } catch {
      // app.cmd already raised the error toast.
      return false;
    } finally {
      this.busy = false;
    }
  }

  async remove(commentId: string): Promise<void> {
    const key = this.key;
    this.busy = true;
    try {
      const review = await invoke<PendingReview>("remove_pending_review_comment", { commentId });
      if (this.key === key) this.review = review;
      void app.cmd("pull_github_comments", { force: true });
    } catch (e) {
      app.showToast("error", String(e));
    } finally {
      this.busy = false;
    }
  }

  async submit(): Promise<void> {
    const decision = this.decision;
    const count = this.count;
    this.busy = true;
    try {
      await app.cmd(
        "submit_pending_review",
        { mode: decisionEvent(decision), summary: this.summary },
        { throwOnError: true },
      );
    } catch {
      // The popover stays open with the draft; app.cmd raised the toast.
      return;
    } finally {
      this.busy = false;
    }
    this.open = false;
    this.summary = "";
    app.showToast("success", submittedToast(decision, count));
    void this.refresh();
    void app.cmd("pull_github_comments", { force: true });
  }
}

export const pendingReview = new PendingReviewStore();
