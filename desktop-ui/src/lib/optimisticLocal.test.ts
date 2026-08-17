import { describe, expect, it } from "vitest";
import {
  applyOptimisticOp,
  buildOptimisticOp,
  reapplyOptimisticOps,
  rollbackOptimisticOp,
} from "./optimisticLocal";
import { snapshotViewIdentity } from "./snapshotChrome";
import type {
  AiSnapshot,
  AppSnapshot,
  FileSnapshot,
  FlatFinding,
  HunkSnapshot,
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
  });

  it("reapply skips a different view identity", () => {
    const view = withThread(commentThread());
    const op = buildOptimisticOp("resolve_thread", { id: "c-1" }, view, { id: "opt-res" });
    const other = snap({ mode: "pr" });
    reapplyOptimisticOps(other, [op!]);
    expect(other.ai.threads).toEqual([]);
    expect(snapshotViewIdentity(view)).not.toBe(snapshotViewIdentity(other));
  });
});
