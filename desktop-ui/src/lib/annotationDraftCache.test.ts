import { describe, expect, test } from "bun:test";
import type { TabSummary } from "./types";
import { AnnotationDraftCache } from "./annotationDraftCache";
import { richSnapshot } from "./stories/fixtures";

describe("review annotation drafts", () => {
  test("restores text and open composers after cards unmount and content refreshes", () => {
    const cache = new AnnotationDraftCache();
    const draft = cache.get(richSnapshot, "thread", "one");
    draft.replyText = "Unsent reply";
    draft.showReply = true;
    draft.askAiText = "Unsent AI context";
    draft.showAskAi = true;
    const refreshed = { ...richSnapshot, preview_context_key: "updated-content" };
    expect(cache.get(refreshed, "thread", "one")).toBe(draft);
    expect(cache.get(refreshed, "thread", "one")).toEqual({ replyText: "Unsent reply", showReply: true, askAiText: "Unsent AI context", showAskAi: true });
  });
  test("isolates annotations, reviews, diff scopes and History commits", () => {
    const cache = new AnnotationDraftCache();
    cache.get(richSnapshot, "thread", "same-id").replyText = "Reply";
    expect(cache.get(richSnapshot, "finding", "same-id").replyText).toBe("");
    expect(cache.get(richSnapshot, "thread", "other-id").replyText).toBe("");
    expect(cache.get({ ...richSnapshot, mode: "staged" }, "thread", "same-id").replyText).toBe("");
    expect(cache.get({ ...richSnapshot, selected_commit_sha: "other-commit" }, "thread", "same-id").replyText).toBe("");
    const other = { ...richSnapshot, tabs: richSnapshot.tabs.map((t) => ({ ...t, repo_root: "/another-review" })) };
    expect(cache.get(other, "thread", "same-id").replyText).toBe("");
  });
  test("isolates remote repositories with matching PR and annotation IDs", () => {
    const cache = new AnnotationDraftCache();
    const first: TabSummary = { ...richSnapshot.tabs[0], idx: 0, kind: "remote_pr", remote: "owner/first", pr_number: 42, is_active: true };
    const second = { ...first, idx: 1, remote: "owner/second", is_active: false };
    const before = { ...richSnapshot, tabs: [first, second], active_tab: 0 };
    const other = { ...before, tabs: [{ ...first, is_active: false }, { ...second, is_active: true }], active_tab: 1 };
    cache.get(before, "thread", "same-id").replyText = "First review draft";
    expect(cache.get(other, "thread", "same-id").replyText).toBe("");
    cache.get(other, "thread", "same-id").replyText = "Second review draft";
    const closed = { ...other, tabs: [{ ...second, idx: 0, is_active: true }], active_tab: 0 };
    cache.sync(closed);
    expect(cache.get(closed, "thread", "same-id").replyText).toBe("Second review draft");
    expect(cache.get(before, "thread", "same-id").replyText).toBe("");
  });
  test("retains drafts when tabs reindex and clears closed reviews", () => {
    const cache = new AnnotationDraftCache();
    const active = { ...richSnapshot.tabs[0], idx: 1, is_active: true };
    const before = { ...richSnapshot, active_tab: 1, tabs: [{ ...active, idx: 0, repo_root: "/other", is_active: false }, active] };
    cache.get(before, "finding", "one").replyText = "Keep this finding reply";
    const after = { ...richSnapshot, active_tab: 0, tabs: [{ ...active, idx: 0 }] };
    cache.sync(after);
    expect(cache.get(after, "finding", "one").replyText).toBe("Keep this finding reply");
    cache.sync({ ...after, tabs: [] });
    expect(cache.get(after, "finding", "one").replyText).toBe("");
    cache.get(after, "thread", "one").replyText = "Another draft";
    cache.sync(null);
    expect(cache.get(after, "thread", "one").replyText).toBe("");
  });
});
