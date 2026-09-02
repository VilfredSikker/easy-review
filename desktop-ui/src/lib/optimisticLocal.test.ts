import { describe, expect, it } from "vitest";
import {
  applyOptimisticOp,
  buildOptimisticOp,
  optimisticInvokeArgs,
  reapplyOptimisticOps,
  rollbackOptimisticOp,
} from "./optimisticLocal";
import { snapshotViewIdentity } from "./snapshotChrome";
import type {
  AiSnapshot,
  AppSnapshot,
  FileSnapshot,
  FindingResponseSnapshot,
  FlatFinding,
  HunkSnapshot,
  InboxItemSnapshot,
  PrInfo,
  ProjectSnapshot,
  TabSummary,
  ThreadSnapshot,
} from "./types";

function emptyAi(overrides: Partial<AiSnapshot> = {}): AiSnapshot {
  return {
    fresh: true,
    stale_reason: null,
    summary_markdown: null,
    agent_summaries: {},
    high: 0,
    med: 0,
    low: 0,
    local_comment_count: 0,
    github_comment_count: 0,
    comments: 0,
    questions: 0,
    notes: 0,
    unpushed: 0,
    threads: [],
    findings: [],
    has_review_json: false,
    eligible_comment_count: 0,
    triage: null,
    diagrams: [],
    diagram_presets: [],
    ...overrides,
  };
}

function hunk(overrides: Partial<HunkSnapshot> = {}): HunkSnapshot {
  return {
    header: "@@ -1,1 +1,1 @@",
    old_start: 1,
    old_count: 1,
    new_start: 1,
    new_count: 1,
    lines: [],
    threads: [],
    ...overrides,
  };
}

function file(overrides: Partial<FileSnapshot> = {}): FileSnapshot {
  return {
    path: "src/a.ts",
    status: "modified",
    additions: 1,
    deletions: 0,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks: [hunk()],
    source_index: 0,
    cache_key: "k",
    ...overrides,
  };
}

function tab(): TabSummary {
  return {
    idx: 0,
    label: "branch",
    kind: "local_branch",
    branch: "feat",
    pr_number: null,
    remote: null,
    repo_root: "/repo",
    is_active: true,
    change_token: "t",
  };
}

function snap(overrides: Partial<AppSnapshot> = {}): AppSnapshot {
  return {
    mode: "branch",
    branch: "feat",
    base: "main",
    input_mode: "normal",
    files: [file()],
    selected_file: 0,
    current_hunk: null,
    filter: null,
    reviewed_count: 0,
    total_count: 1,
    ai: emptyAi(),
    pr: null,
    panels: { left: true, tree: true, right: true },
    theme: "graphite",
    watch_active: false,
    watch_status: { active: false, branch: null, root_path: null },
    worktrees: [],
    projects: [],
    local_branch: "feat",
    notification: null,
    tabs: [tab()],
    active_tab: 0,
    bg_loading: {
      pr_list: false,
      gh_status: false,
      gh_comments: false,
      tab_diff: false,
    },
    ...overrides,
  };
}

function commentThread(id = "c-1"): ThreadSnapshot {
  return {
    id,
    kind: "comment",
    file: "src/a.ts",
    line: 12,
    line_end: 12,
    side: "RIGHT",
    source: "local",
    synced: false,
    stale: false,
    resolved: false,
    root: {
      id,
      author: "you",
      kind: "you",
      timestamp: "2026-08-17T00:00:00.000Z",
      body_markdown: "ship it",
    },
    replies: [],
    promoted_to: null,
  };
}

function withThread(thread: ThreadSnapshot): AppSnapshot {
  const h = hunk({ threads: [thread] });
  const f = file({ hunks: [h], comment_count: thread.kind === "comment" ? 1 : 0 });
  return snap({
    files: [f],
    ai: emptyAi({
      threads: [thread],
      comments: thread.kind === "comment" ? 1 : 0,
      questions: thread.kind === "question" ? 1 : 0,
      notes: thread.kind === "note" ? 1 : 0,
      local_comment_count: thread.kind === "comment" ? 1 : 0,
      unpushed: thread.kind === "comment" ? 1 : 0,
    }),
  });
}

const now = "2026-08-17T00:00:00.000Z";

describe("reply / resolve / delete / edit", () => {
  it("appends a local reply and rolls it back", () => {
    const view = withThread(commentThread());
    const op = buildOptimisticOp(
      "reply_to_thread",
      { parentId: "c-1", text: "also this" },
      view,
      { nowIso: now, id: "opt-r" },
    );
    expect(op?.type).toBe("reply");
    applyOptimisticOp(view, op!);
    expect(view.ai.threads[0].replies).toHaveLength(1);
    expect(view.files[0].hunks[0].threads[0].replies[0].body_markdown).toBe("also this");
    rollbackOptimisticOp(view, op!);
    expect(view.ai.threads[0].replies).toEqual([]);
  });

  it("skips resolve when the thread is already resolved", () => {
    const thread = commentThread();
    thread.resolved = true;
    const view = withThread(thread);
    expect(buildOptimisticOp("resolve_thread", { id: "c-1" }, view)).toBeNull();
  });

  it("marks a thread resolved and restores on rollback", () => {
    const view = withThread(commentThread());
    const op = buildOptimisticOp("resolve_thread", { id: "c-1" }, view, { id: "opt-res" });
    applyOptimisticOp(view, op!);
    expect(view.ai.threads[0].resolved).toBe(true);
    expect(view.files[0].hunks[0].threads[0].resolved).toBe(true);
    rollbackOptimisticOp(view, op!);
    expect(view.ai.threads[0].resolved).toBe(false);
  });

  it("deletes a root thread and restores counts", () => {
    const view = withThread(commentThread());
    const op = buildOptimisticOp("delete_thread", { id: "c-1" }, view, { id: "opt-d" });
    applyOptimisticOp(view, op!);
    expect(view.ai.threads).toEqual([]);
    expect(view.ai.comments).toBe(0);
    rollbackOptimisticOp(view, op!);
    expect(view.ai.threads.map((t) => t.id)).toEqual(["c-1"]);
    expect(view.ai.comments).toBe(1);
  });

  it("deletes a reply and puts it back at the same index", () => {
    const thread = commentThread();
    thread.replies = [
      {
        id: "r-1",
        author: "you",
        kind: "you",
        timestamp: now,
        body_markdown: "first",
      },
      {
        id: "r-2",
        author: "you",
        kind: "you",
        timestamp: now,
        body_markdown: "second",
      },
    ];
    const view = withThread(thread);
    const op = buildOptimisticOp("delete_thread", { id: "r-1" }, view, { id: "opt-dr" });
    applyOptimisticOp(view, op!);
    expect(view.ai.threads[0].replies.map((r) => r.id)).toEqual(["r-2"]);
    rollbackOptimisticOp(view, op!);
    expect(view.ai.threads[0].replies.map((r) => r.id)).toEqual(["r-1", "r-2"]);
  });

  it("edits a root body and restores the previous text", () => {
    const view = withThread(commentThread());
    const op = buildOptimisticOp(
      "update_thread_message",
      { id: "c-1", body: "revised" },
      view,
      { id: "opt-e" },
    );
    applyOptimisticOp(view, op!);
    expect(view.ai.threads[0].root.body_markdown).toBe("revised");
    rollbackOptimisticOp(view, op!);
    expect(view.ai.threads[0].root.body_markdown).toBe("ship it");
  });
});

describe("dismiss / promote / bulk / annotation", () => {
  it("hides a finding and restores severity counts", () => {
    const finding: FlatFinding = {
      id: "f-1",
      file: "src/a.ts",
      line: 3,
      hunk_index: 0,
      severity: "high",
      expert_label: null,
      agent_label: "General",
      title: "bug",
      message_markdown: "nope",
      promoted_to: null,
      thread_id: null,
    };
    const view = snap({
      files: [file({ finding_count: 1 })],
      ai: emptyAi({ findings: [finding], high: 1 }),
    });
    const op = buildOptimisticOp(
      "dismiss_finding",
      { findingId: "f-1" },
      view,
      { id: "opt-f" },
    );
    applyOptimisticOp(view, op!);
    expect(view.ai.findings).toEqual([]);
    expect(view.ai.high).toBe(0);
    expect(view.files[0].finding_count).toBe(0);
    rollbackOptimisticOp(view, op!);
    expect(view.ai.findings.map((f) => f.id)).toEqual(["f-1"]);
    expect(view.ai.high).toBe(1);
  });

  it("promotes a question to a comment and restores the question on rollback", () => {
    const q: ThreadSnapshot = {
      ...commentThread("q-1"),
      kind: "question",
    };
    const h = hunk({ threads: [q] });
    const view = snap({
      files: [file({ hunks: [h], question_count: 1 })],
      ai: emptyAi({ threads: [q], questions: 1 }),
    });
    const op = buildOptimisticOp(
      "promote_to_comment",
      { id: "q-1", body: "as a comment" },
      view,
      { nowIso: now, id: "opt-p" },
    );
    applyOptimisticOp(view, op!);
    expect(view.ai.threads.map((t) => t.kind)).toEqual(["comment"]);
    expect(view.ai.questions).toBe(0);
    expect(view.ai.comments).toBe(1);
    rollbackOptimisticOp(view, op!);
    expect(view.ai.threads.map((t) => t.id)).toEqual(["q-1"]);
    expect(view.ai.questions).toBe(1);
    expect(view.ai.comments).toBe(0);
  });

  it("marks every file in a pillar reviewed and rolls the flags back", () => {
    const files = [
      file({ path: "a.ts", reviewed: false }),
      file({ path: "b.ts", reviewed: false, source_index: 1 }),
    ];
    const view = snap({
      files,
      total_count: 2,
      reviewed_count: 0,
      tour: {
        available: true,
        fresh: true,
        scope: "branch",
        title: "Guide",
        overviewMarkdown: "",
        pillars: [
          {
            id: "p1",
            title: "core",
            descriptionMarkdown: "",
            importance: 1,
            foundation: false,
            files: [
              { path: "a.ts", reason: "", findingIds: [] },
              { path: "b.ts", reason: "", findingIds: [] },
            ],
            reviewedCount: 0,
            totalCount: 2,
          },
        ],
      },
    });
    const op = buildOptimisticOp(
      "bulk_review_pillar",
      { pillarId: "p1" },
      view,
      { id: "opt-b" },
    );
    applyOptimisticOp(view, op!);
    expect(view.files.every((f) => f.reviewed)).toBe(true);
    expect(view.reviewed_count).toBe(2);
    expect(view.tour?.pillars[0].reviewedCount).toBe(2);
    rollbackOptimisticOp(view, op!);
    expect(view.files.every((f) => !f.reviewed)).toBe(true);
    expect(view.reviewed_count).toBe(0);
  });

  it("adds and removes a UI annotation", () => {
    const view = snap({ ui_annotations: [] });
    const add = buildOptimisticOp(
      "add_ui_annotation",
      { url: "https://ex/app", text: "pin", bbox: [1, 2, 3, 4], viewport: [800, 600] },
      view,
      { nowIso: now, id: "opt-a" },
    );
    applyOptimisticOp(view, add!);
    expect(view.ui_annotations).toHaveLength(1);
    expect(view.ui_annotations[0]).toMatchObject({ url: "https://ex/app", text: "pin" });
    const del = buildOptimisticOp(
      "delete_ui_annotation",
      { id: "opt-a" },
      view,
      { id: "opt-da" },
    );
    applyOptimisticOp(view, del!);
    expect(view.ui_annotations).toEqual([]);
    rollbackOptimisticOp(view, del!);
    expect(view.ui_annotations).toHaveLength(1);
    expect(view.ui_annotations[0]).toMatchObject({ url: "https://ex/app", text: "pin" });
  });

  it("reapply skips a different view identity", () => {
    const view = withThread(commentThread());
    const op = buildOptimisticOp("resolve_thread", { id: "c-1" }, view, { id: "opt-res" });
    const other = snap({ mode: "pr" });
    reapplyOptimisticOps(other, [op!]);
    expect(other.ai.threads).toEqual([]);
    expect(snapshotViewIdentity(view)).not.toBe(snapshotViewIdentity(other));
  });

  it("paints Review all on the snapshot Other pillar", () => {
    const files = [
      file({ path: "a.ts", reviewed: false }),
      file({ path: "orphan.ts", reviewed: false, source_index: 1 }),
    ];
    const view = snap({
      files,
      total_count: 2,
      tour: {
        available: true,
        fresh: true,
        scope: "branch",
        title: "Guide",
        overviewMarkdown: "",
        pillars: [
          {
            id: "p1",
            title: "core",
            descriptionMarkdown: "",
            importance: 1,
            foundation: false,
            files: [{ path: "a.ts", reason: "", findingIds: [] }],
            reviewedCount: 0,
            totalCount: 1,
          },
          {
            id: "__other__",
            title: "Other changes",
            descriptionMarkdown: "",
            importance: 0,
            foundation: false,
            files: [{ path: "orphan.ts", reason: "", findingIds: [] }],
            reviewedCount: 0,
            totalCount: 1,
          },
        ],
      },
    });
    const op = buildOptimisticOp(
      "bulk_review_pillar",
      { pillarId: "__other__" },
      view,
      { id: "opt-o" },
    );
    expect(op?.type).toBe("bulk-reviewed");
    applyOptimisticOp(view, op!);
    expect(view.files.find((f) => f.path === "orphan.ts")?.reviewed).toBe(true);
    expect(view.files.find((f) => f.path === "a.ts")?.reviewed).toBe(false);
    expect(view.tour?.pillars[1].reviewedCount).toBe(1);
  });

  it("hides a finding's linked thread with the finding", () => {
    const thread = commentThread("t-linked");
    const finding: FlatFinding = {
      id: "f-1",
      file: "src/a.ts",
      line: 12,
      hunk_index: 0,
      severity: "high",
      expert_label: null,
      agent_label: "General",
      title: "bug",
      message_markdown: "nope",
      promoted_to: null,
      thread_id: "t-linked",
    };
    const h = hunk({ threads: [thread] });
    const view = snap({
      files: [file({ hunks: [h], finding_count: 1, comment_count: 1 })],
      ai: emptyAi({ threads: [thread], findings: [finding], high: 1, comments: 1 }),
    });
    const op = buildOptimisticOp("dismiss_finding", { findingId: "f-1" }, view, { id: "opt-f" });
    applyOptimisticOp(view, op!);
    expect(view.ai.findings).toEqual([]);
    expect(view.ai.threads).toEqual([]);
    rollbackOptimisticOp(view, op!);
    expect(view.ai.threads.map((t) => t.id)).toEqual(["t-linked"]);
  });

  it("mints a persistable id for add_comment and add_ui_annotation", () => {
    const comment = buildOptimisticOp(
      "add_comment",
      { file: "src/a.ts", hunkIdx: 0, lineNum: 1, text: "hi" },
      snap(),
    );
    expect(comment?.type).toBe("add-thread");
    if (comment?.type !== "add-thread") throw new Error("expected add-thread");
    expect(comment.pending.id.startsWith("c-")).toBe(true);
    expect(optimisticInvokeArgs("add_comment", { text: "hi" }, comment).id).toBe(
      comment.pending.id,
    );
    const commentArgs = optimisticInvokeArgs(
      "add_comment",
      { text: "hi" },
      comment,
      { active_tab: 0, repo_root: "/repo", pr_number: null, branch: "feat", mode: "branch" },
    );
    expect(commentArgs.view).toEqual({
      active_tab: 0,
      repo_root: "/repo",
      pr_number: null,
      branch: "feat",
      mode: "branch",
    });

    const ann = buildOptimisticOp(
      "add_ui_annotation",
      { url: "https://ex/app", text: "pin", bbox: [1, 2, 3, 4], viewport: [800, 600] },
      snap({ ui_annotations: [] }),
    );
    expect(ann?.type).toBe("add-annotation");
    if (ann?.type !== "add-annotation") throw new Error("expected add-annotation");
    expect(ann.annotation.id.startsWith("ui-")).toBe(true);
    expect(optimisticInvokeArgs("add_ui_annotation", { text: "pin" }, ann).id).toBe(
      ann.annotation.id,
    );
  });

  it("deletes a thread that only lives on a hunk copy", () => {
    const thread = commentThread("hunk-only");
    const view = snap({
      files: [file({ hunks: [hunk({ threads: [thread] })], comment_count: 1 })],
      ai: emptyAi(),
    });
    const op = buildOptimisticOp("delete_thread", { id: "hunk-only" }, view, { id: "opt-d" });
    expect(op?.type).toBe("delete-root");
    applyOptimisticOp(view, op!);
    expect(view.files[0].hunks[0].threads).toEqual([]);
    expect(view.files[0].comment_count).toBe(0);
    rollbackOptimisticOp(view, op!);
    expect(view.files[0].hunks[0].threads.map((t) => t.id)).toEqual(["hunk-only"]);
    expect(view.files[0].comment_count).toBe(1);
  });
});

function project(overrides: Partial<ProjectSnapshot> = {}): ProjectSnapshot {
  return {
    id: "p1",
    name: "proj",
    root_path: "/repo",
    remote: "o/r",
    is_active: true,
    local_branches: [],
    auto_branches: [],
    saved_prs: [],
    my_prs: [],
    prs_to_review: [],
    recent_prs: [],
    recently_merged: [],
    ...overrides,
  };
}

function pr(number: number, overrides: Partial<PrInfo> = {}): PrInfo {
  return {
    number,
    title: `PR ${number}`,
    head_ref: "feat",
    state: "OPEN",
    is_draft: false,
    author: "ann",
    assignees: [],
    reviewers: [],
    checks_state: null,
    review_decision: null,
    merged_at: null,
    approved_by_me: false,
    base_ref: "main",
    head_oid: "abc",
    updated_at: now,
    ...overrides,
  };
}

function inboxItem(id: string, overrides: Partial<InboxItemSnapshot> = {}): InboxItemSnapshot {
  return {
    id,
    kind: "pr_review_requested",
    severity: "info",
    title: id,
    body: "",
    source: "gh",
    target: {},
    created_at_ms: 1,
    read_at_ms: null,
    dedupe_key: id,
    ...overrides,
  };
}

/** A snapshot from a different review view (tab 1, other branch). */
function otherView(overrides: Partial<AppSnapshot> = {}): AppSnapshot {
  return snap({
    active_tab: 1,
    branch: "other",
    tabs: [{ ...tab(), idx: 1, branch: "other" }],
    ...overrides,
  });
}

describe("global chrome ops: inbox", () => {
  it("marks a group read, recounts, reapplies across views, and rolls back", () => {
    const view = snap({
      inbox_items: [inboxItem("a"), inboxItem("b"), inboxItem("c", { read_at_ms: 5 })],
      inbox_unread_count: 2,
    });
    const op = buildOptimisticOp("mark_inbox_items_read", { ids: ["a", "c"] }, view, {
      nowIso: now,
      id: "opt-i",
    });
    if (op?.type !== "inbox-read") throw new Error("expected inbox-read");
    // Already-read `c` is not captured.
    expect(op.ids).toEqual(["a"]);
    applyOptimisticOp(view, op);
    expect(view.inbox_items![0].read_at_ms).toBe(Date.parse(now));
    expect(view.inbox_items![1].read_at_ms).toBeNull();
    expect(view.inbox_unread_count).toBe(1);
    // Idempotent on the same object.
    applyOptimisticOp(view, op);
    expect(view.inbox_unread_count).toBe(1);

    // A fresh snapshot from another view still gets the op (global), and an
    // item that arrived after the click is left alone.
    const fresh = otherView({
      inbox_items: [inboxItem("a"), inboxItem("b"), inboxItem("d")],
      inbox_unread_count: 3,
    });
    expect(snapshotViewIdentity(fresh)).not.toBe(op.viewIdentity);
    reapplyOptimisticOps(fresh, [op]);
    expect(fresh.inbox_items!.map((i) => i.read_at_ms == null)).toEqual([false, true, true]);
    expect(fresh.inbox_unread_count).toBe(2);

    rollbackOptimisticOp(view, op);
    expect(view.inbox_items![0].read_at_ms).toBeNull();
    expect(view.inbox_unread_count).toBe(2);
  });

  it("skips IPC when nothing visible changes, but always runs mark-all", () => {
    const view = snap({ inbox_items: [inboxItem("c", { read_at_ms: 5 })], inbox_unread_count: 0 });
    expect(buildOptimisticOp("mark_inbox_item_read", { id: "c" }, view)).toBeNull();
    expect(buildOptimisticOp("mark_inbox_items_read", { ids: ["zzz"] }, view)).toBeNull();
    // Hidden-kind items are not in the snapshot; the backend still marks them.
    expect(buildOptimisticOp("mark_all_inbox_read", {}, view)?.type).toBe("inbox-read");
  });

  it("clears only read items in a group and restores them on rollback", () => {
    const view = snap({
      inbox_items: [
        inboxItem("a"),
        inboxItem("b", { read_at_ms: 5 }),
        inboxItem("c", { read_at_ms: 6 }),
      ],
      inbox_unread_count: 1,
    });
    const op = buildOptimisticOp("clear_inbox_items", { ids: ["a", "b"] }, view, { id: "opt-c" });
    if (op?.type !== "inbox-clear") throw new Error("expected inbox-clear");
    // Unread `a` stays even though it was listed (backend contract).
    expect(op.removed.map((i) => i.id)).toEqual(["b"]);
    applyOptimisticOp(view, op);
    expect(view.inbox_items!.map((i) => i.id)).toEqual(["a", "c"]);
    expect(view.inbox_unread_count).toBe(1);
    rollbackOptimisticOp(view, op);
    expect(view.inbox_items!.map((i) => i.id).sort()).toEqual(["a", "b", "c"]);

    const none = snap({ inbox_items: [inboxItem("a")] });
    expect(buildOptimisticOp("clear_read_inbox_items", {}, none)?.type).toBe("inbox-clear");
    expect(buildOptimisticOp("clear_inbox_items", { ids: ["a"] }, none)).toBeNull();
  });
});

describe("global chrome ops: saved and ignored PRs", () => {
  it("pins at the head with the sidebar row, keeps the hydrated row on reapply, unpins", () => {
    const hydrated = pr(7, { author: "bob" });
    const view = snap({
      projects: [project({ my_prs: [hydrated], saved_prs: [pr(1), pr(2)] })],
    });
    const op = buildOptimisticOp(
      "save_pr",
      { projectId: "p1", prNumber: 7, title: "" },
      view,
      { id: "opt-p" },
    );
    if (op?.type !== "saved-pr") throw new Error("expected saved-pr");
    expect(op.prevIndex).toBe(-1);
    expect(op.pr.author).toBe("bob");
    applyOptimisticOp(view, op);
    expect(view.projects[0].saved_prs.map((p) => p.number)).toEqual([7, 1, 2]);

    // The backend already inserted its own row: a reapply keeps that object.
    const confirmedRow = pr(7, { author: "backend" });
    const fresh = otherView({
      projects: [project({ saved_prs: [confirmedRow, pr(1), pr(2)] })],
    });
    reapplyOptimisticOps(fresh, [op]);
    expect(fresh.projects[0].saved_prs[0]).toBe(confirmedRow);
    expect(fresh.projects[0].saved_prs).toHaveLength(3);

    rollbackOptimisticOp(view, op);
    expect(view.projects[0].saved_prs.map((p) => p.number)).toEqual([1, 2]);

    const unpin = buildOptimisticOp("unsave_pr", { projectId: "p1", prNumber: 2 }, view, {
      id: "opt-u",
    });
    if (unpin?.type !== "saved-pr") throw new Error("expected saved-pr");
    expect(unpin.prevIndex).toBe(1);
    applyOptimisticOp(view, unpin);
    expect(view.projects[0].saved_prs.map((p) => p.number)).toEqual([1]);
    // A later pin of the same number must not be duplicated by the rollback.
    view.projects[0].saved_prs = [pr(2), pr(1)];
    rollbackOptimisticOp(view, unpin);
    expect(view.projects[0].saved_prs.map((p) => p.number)).toEqual([1, 2]);
  });

  it("stubs an unknown PR with a non-empty title and caps the list at 50", () => {
    const many = Array.from({ length: 50 }, (_, i) => pr(100 + i));
    const view = snap({ projects: [project({ saved_prs: many })] });
    const op = buildOptimisticOp(
      "save_pr",
      { projectId: "p1", prNumber: 9, title: "  " },
      view,
      { id: "opt-s" },
    );
    if (op?.type !== "saved-pr") throw new Error("expected saved-pr");
    expect(op.pr.title).toBe("PR #9");
    applyOptimisticOp(view, op);
    expect(view.projects[0].saved_prs).toHaveLength(50);
    expect(view.projects[0].saved_prs[0].number).toBe(9);
    expect(buildOptimisticOp("save_pr", { projectId: "nope", prNumber: 9 }, view)).toBeNull();
  });

  it("ignores a PR by hiding its sidebar rows and restores them on rollback", () => {
    const view = snap({
      projects: [
        project({
          my_prs: [pr(1), pr(7)],
          prs_to_review: [pr(7)],
          recently_merged: [pr(3)],
          dismissed_prs: [3],
        }),
      ],
    });
    const op = buildOptimisticOp(
      "dismiss_remote_pr",
      { projectId: "p1", prNumber: 7 },
      view,
      { id: "opt-d" },
    );
    if (op?.type !== "dismissed-pr") throw new Error("expected dismissed-pr");
    expect(op.wasDismissed).toBe(false);
    expect(op.removed.map((r) => `${r.list}:${r.index}`)).toEqual(["my_prs:1", "prs_to_review:0"]);
    applyOptimisticOp(view, op);
    const p = view.projects[0];
    expect([...(p.dismissed_prs ?? [])].sort()).toEqual([3, 7]);
    expect(p.my_prs.map((x) => x.number)).toEqual([1]);
    expect(p.prs_to_review).toEqual([]);
    rollbackOptimisticOp(view, op);
    expect(p.dismissed_prs).toEqual([3]);
    expect(p.my_prs.map((x) => x.number)).toEqual([1, 7]);
    expect(p.prs_to_review.map((x) => x.number)).toEqual([7]);

    const undo = buildOptimisticOp(
      "undismiss_remote_pr",
      { projectId: "p1", prNumber: 3 },
      view,
      { id: "opt-un" },
    );
    if (undo?.type !== "dismissed-pr") throw new Error("expected dismissed-pr");
    applyOptimisticOp(view, undo);
    expect(p.dismissed_prs).toEqual([]);
    rollbackOptimisticOp(view, undo);
    expect(p.dismissed_prs).toEqual([3]);
  });

  it("does not attach a view to global invoke args", () => {
    const view = snap({ inbox_items: [inboxItem("a")] });
    const op = buildOptimisticOp("mark_inbox_item_read", { id: "a" }, view);
    if (!op) throw new Error("expected op");
    const args = optimisticInvokeArgs("mark_inbox_item_read", { id: "a" }, op, {
      active_tab: 0,
      repo_root: "/repo",
      pr_number: null,
      branch: "feat",
      mode: "branch",
    });
    expect(args).toEqual({ id: "a" });
  });
});

describe("finding-thread ops", () => {
  const finding = (overrides: Partial<FlatFinding> = {}): FlatFinding => ({
    id: "f-1",
    file: "src/a.ts",
    line: 12,
    hunk_index: 0,
    severity: "high",
    expert_label: null,
    agent_label: "General",
    title: "bug",
    message_markdown: "nope",
    promoted_to: null,
    thread_id: null,
    ...overrides,
  });
  const response = (id: string, body = "looks real"): FindingResponseSnapshot => ({
    id,
    author: "AI",
    kind: "ai",
    timestamp: now,
    body_markdown: body,
    origin: "finding_response",
    editable: false,
    deletable: true,
  });

  it("removes the linked thread but keeps the finding", () => {
    const t = commentThread("c-f");
    const view = snap({
      files: [file({ hunks: [hunk({ threads: [t] })], comment_count: 1, finding_count: 1 })],
      ai: emptyAi({
        threads: [t],
        comments: 1,
        local_comment_count: 1,
        unpushed: 1,
        findings: [finding({ thread_id: "c-f" })],
        high: 1,
      }),
    });
    const op = buildOptimisticOp("remove_finding_thread", { findingId: "f-1" }, view, {
      id: "opt-rt",
    });
    if (op?.type !== "remove-finding-thread") throw new Error("expected remove-finding-thread");
    applyOptimisticOp(view, op);
    expect(view.ai.threads).toEqual([]);
    expect(view.files[0].hunks[0].threads).toEqual([]);
    expect(view.ai.findings).toHaveLength(1);
    expect(view.ai.findings[0].thread_id).toBeNull();
    expect(view.ai.comments).toBe(0);
    rollbackOptimisticOp(view, op);
    expect(view.ai.threads.map((x) => x.id)).toEqual(["c-f"]);
    expect(view.ai.findings[0].thread_id).toBe("c-f");
    expect(view.ai.comments).toBe(1);
  });

  it("promotes a finding to a comment at its line and restores it on rollback", () => {
    const view = snap({
      files: [file({ finding_count: 1 })],
      ai: emptyAi({ findings: [finding()], high: 1 }),
    });
    const op = buildOptimisticOp(
      "promote_finding_to_comment",
      { findingId: "f-1", body: "as comment" },
      view,
      { nowIso: now, id: "opt-pf" },
    );
    if (op?.type !== "promote-finding") throw new Error("expected promote-finding");
    applyOptimisticOp(view, op);
    expect(view.ai.findings).toEqual([]);
    expect(view.ai.high).toBe(0);
    expect(view.files[0].finding_count).toBe(0);
    expect(view.ai.threads.map((t) => [t.kind, t.line, t.root.body_markdown])).toEqual([
      ["comment", 12, "as comment"],
    ]);
    expect(view.ai.comments).toBe(1);
    expect(view.files[0].hunks[0].threads).toHaveLength(1);
    rollbackOptimisticOp(view, op);
    expect(view.ai.findings.map((f) => f.id)).toEqual(["f-1"]);
    expect(view.ai.high).toBe(1);
    expect(view.ai.threads).toEqual([]);
    expect(view.ai.comments).toBe(0);
  });

  it("deletes and edits one AI response without touching the others", () => {
    const pending = { ...response(""), body_markdown: "…thinking", deletable: false };
    const view = snap({
      ai: emptyAi({
        findings: [finding({ responses: [response("fr-1"), response("fr-2", "second"), pending] })],
      }),
    });
    // The pending "…thinking" row has no id and cannot be deleted.
    expect(
      buildOptimisticOp("delete_finding_response", { findingId: "f-1", responseId: "" }, view),
    ).toBeNull();
    const del = buildOptimisticOp(
      "delete_finding_response",
      { findingId: "f-1", responseId: "fr-1" },
      view,
      { id: "opt-dr" },
    );
    if (del?.type !== "delete-finding-response") throw new Error("expected delete-finding-response");
    applyOptimisticOp(view, del);
    expect(view.ai.findings[0].responses!.map((r) => r.id)).toEqual(["fr-2", ""]);
    rollbackOptimisticOp(view, del);
    expect(view.ai.findings[0].responses!.map((r) => r.id)).toEqual(["fr-1", "fr-2", ""]);

    const edit = buildOptimisticOp(
      "update_finding_response",
      { findingId: "f-1", responseId: "fr-2", body: "edited" },
      view,
      { id: "opt-er" },
    );
    if (edit?.type !== "edit-finding-response") throw new Error("expected edit-finding-response");
    applyOptimisticOp(view, edit);
    expect(view.ai.findings[0].responses![1].body_markdown).toBe("edited");
    rollbackOptimisticOp(view, edit);
    expect(view.ai.findings[0].responses![1].body_markdown).toBe("second");
  });

  it("replies to a finding: appends to its thread, or paints a new one and links it", () => {
    const t = commentThread("c-f");
    const withT = snap({
      files: [file({ hunks: [hunk({ threads: [t] })], comment_count: 1 })],
      ai: emptyAi({ threads: [t], comments: 1, findings: [finding({ thread_id: "c-f" })] }),
    });
    const reply = buildOptimisticOp(
      "reply_to_finding",
      { findingId: "f-1", body: "agree", aiAssist: false },
      withT,
      { nowIso: now, id: "opt-fr" },
    );
    if (reply?.type !== "reply-finding") throw new Error("expected reply-finding");
    expect(reply.target.kind).toBe("existing");
    applyOptimisticOp(withT, reply);
    expect(withT.ai.threads[0].replies.map((r) => r.body_markdown)).toEqual(["agree"]);
    // The backend reuses the painted id so the confirm lands on the same row.
    expect(optimisticInvokeArgs("reply_to_finding", { findingId: "f-1" }, reply).id).toBe("opt-fr");
    rollbackOptimisticOp(withT, reply);
    expect(withT.ai.threads[0].replies).toEqual([]);

    const bare = snap({ ai: emptyAi({ findings: [finding()] }) });
    const first = buildOptimisticOp(
      "reply_to_finding",
      { findingId: "f-1", body: "first", aiAssist: false },
      bare,
      { nowIso: now, id: "opt-new" },
    );
    if (first?.type !== "reply-finding") throw new Error("expected reply-finding");
    expect(first.target.kind).toBe("new");
    applyOptimisticOp(bare, first);
    expect(bare.ai.findings[0].thread_id).toBe("opt-new");
    expect(bare.ai.threads.map((x) => x.id)).toEqual(["opt-new"]);
    rollbackOptimisticOp(bare, first);
    expect(bare.ai.findings[0].thread_id).toBeNull();
    expect(bare.ai.threads).toEqual([]);

    // AI-assisted: nothing to paint, but the op exists so the IPC still runs.
    const ai = buildOptimisticOp(
      "reply_to_finding",
      { findingId: "f-1", body: "", aiAssist: true },
      bare,
    );
    if (ai?.type !== "reply-finding") throw new Error("expected reply-finding");
    expect(ai.target.kind).toBe("ai");
    expect(optimisticInvokeArgs("reply_to_finding", { findingId: "f-1" }, ai).id).toBeUndefined();
  });
});
