import { describe, expect, it } from "vitest";
import {
  applyOptimisticThread,
  buildOptimisticThread,
  parseAddThreadArgs,
  reapplyOptimisticThreads,
  removeOptimisticThread,
} from "./optimisticComment";
import { snapshotViewIdentity } from "./snapshotChrome";
import type {
  AiSnapshot,
  AppSnapshot,
  FileSnapshot,
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

function pendingFor(
  command: "add_comment" | "add_question" | "add_note",
  view: AppSnapshot,
  args: Record<string, unknown>,
  id: string,
): ReturnType<typeof buildOptimisticThread> {
  const parsed = parseAddThreadArgs(command, args);
  if (!parsed) throw new Error("expected parsed args");
  const filePath = parsed.file || view.files[0]?.path || "";
  return buildOptimisticThread(command, parsed, snapshotViewIdentity(view), filePath, {
    id,
    nowIso: "2026-08-17T00:00:00.000Z",
  });
}

const commentArgs = {
  file: "src/a.ts",
  hunkIdx: 0,
  lineNum: 12,
  lineNumEnd: 14,
  text: "ship it",
  side: "RIGHT",
};

describe("parseAddThreadArgs", () => {
  it("reads camelCase composer args", () => {
    expect(parseAddThreadArgs("add_comment", commentArgs)).toEqual({
      file: "src/a.ts",
      hunkIdx: 0,
      lineNum: 12,
      lineNumEnd: 14,
      text: "ship it",
      side: "RIGHT",
    });
  });

  it("reads snake_case args and LEFT side", () => {
    expect(
      parseAddThreadArgs("add_question", {
        file: "src/a.ts",
        hunk_idx: 2,
        line_num: 8,
        line_num_end: null,
        text: "why?",
        side: "LEFT",
      }),
    ).toEqual({
      file: "src/a.ts",
      hunkIdx: 2,
      lineNum: 8,
      lineNumEnd: null,
      text: "why?",
      side: "LEFT",
    });
  });

  it("rejects empty text and unknown commands", () => {
    expect(parseAddThreadArgs("add_comment", { ...commentArgs, text: "  " })).toBeNull();
    expect(parseAddThreadArgs("reply_to_thread", commentArgs)).toBeNull();
  });
});

describe("applyOptimisticThread", () => {
  it("attaches a local comment and bumps comment counts", () => {
    const view = snap();
    const pending = pendingFor("add_comment", view, commentArgs, "opt-1");
    applyOptimisticThread(view, pending);

    expect(view.ai.threads.map((t) => t.id)).toEqual(["opt-1"]);
    expect(view.files[0].hunks[0].threads.map((t) => t.id)).toEqual(["opt-1"]);
    expect(view.ai.comments).toBe(1);
    expect(view.ai.local_comment_count).toBe(1);
    expect(view.ai.unpushed).toBe(1);
    expect(view.files[0].comment_count).toBe(1);
    expect(view.ai.threads[0]).toMatchObject({
      kind: "comment",
      source: "local",
      synced: false,
      stale: false,
      side: "RIGHT",
      line: 12,
      line_end: 14,
      root: { author: "you", body_markdown: "ship it" },
    });
  });

  it("bumps question counts and not unpushed", () => {
    const view = snap();
    applyOptimisticThread(
      view,
      pendingFor("add_question", view, { ...commentArgs, text: "why?" }, "opt-q"),
    );
    expect(view.ai.questions).toBe(1);
    expect(view.ai.unpushed).toBe(0);
    expect(view.files[0].question_count).toBe(1);
    expect(view.ai.threads[0].kind).toBe("question");
  });

  it("bumps notes only", () => {
    const view = snap();
    applyOptimisticThread(
      view,
      pendingFor("add_note", view, { ...commentArgs, text: "todo" }, "opt-n"),
    );
    expect(view.ai.notes).toBe(1);
    expect(view.ai.questions).toBe(0);
    expect(view.files[0].comment_count).toBe(0);
    expect(view.files[0].question_count).toBe(0);
    expect(view.ai.threads[0].kind).toBe("note");
  });

  it("still records ai.threads when the hunk is missing", () => {
    const view = snap({ files: [file({ hunks: [] })] });
    applyOptimisticThread(view, pendingFor("add_comment", view, commentArgs, "opt-1"));
    expect(view.ai.threads).toHaveLength(1);
    expect(view.files[0].hunks).toEqual([]);
    expect(view.ai.comments).toBe(1);
    expect(view.files[0].comment_count).toBe(1);
  });

  it("does not double-count an already present id", () => {
    const view = snap();
    const pending = pendingFor("add_comment", view, commentArgs, "opt-1");
    applyOptimisticThread(view, pending);
    applyOptimisticThread(view, pending);
    expect(view.ai.threads).toHaveLength(1);
    expect(view.files[0].hunks[0].threads).toHaveLength(1);
    expect(view.ai.comments).toBe(1);
    expect(view.files[0].comment_count).toBe(1);
  });
});

describe("removeOptimisticThread", () => {
  it("rolls back the thread and counts", () => {
    const view = snap();
    const pending = pendingFor("add_comment", view, commentArgs, "opt-1");
    applyOptimisticThread(view, pending);
    removeOptimisticThread(view, pending);
    expect(view.ai.threads).toEqual([]);
    expect(view.files[0].hunks[0].threads).toEqual([]);
    expect(view.ai.comments).toBe(0);
    expect(view.ai.local_comment_count).toBe(0);
    expect(view.ai.unpushed).toBe(0);
    expect(view.files[0].comment_count).toBe(0);
  });

  it("is a no-op when the id is already gone and clamps at zero", () => {
    const view = snap();
    const pending = pendingFor("add_comment", view, commentArgs, "opt-1");
    removeOptimisticThread(view, pending);
    expect(view.ai.comments).toBe(0);
    expect(view.files[0].comment_count).toBe(0);
  });
});

describe("reapplyOptimisticThreads", () => {
  it("restores a pending thread wiped by a stale snapshot", () => {
    const view = snap();
    const pending = pendingFor("add_comment", view, commentArgs, "opt-1");
    const stale = snap();
    reapplyOptimisticThreads(stale, [pending]);
    expect(stale.ai.threads.map((t: ThreadSnapshot) => t.id)).toEqual(["opt-1"]);
    expect(stale.files[0].hunks[0].threads).toHaveLength(1);
    expect(stale.ai.comments).toBe(1);
  });

  it("skips when the id is already present", () => {
    const view = snap();
    const pending = pendingFor("add_comment", view, commentArgs, "opt-1");
    applyOptimisticThread(view, pending);
    reapplyOptimisticThreads(view, [pending]);
    expect(view.ai.threads).toHaveLength(1);
    expect(view.ai.comments).toBe(1);
  });

  it("skips when the view identity does not match", () => {
    const view = snap();
    const pending = pendingFor("add_comment", view, commentArgs, "opt-1");
    const other = snap({
      mode: "pr",
      tabs: [{ ...tab(), kind: "remote_pr", pr_number: 12, label: "pr-12" }],
      pr: {
        number: 12,
        title: "x",
        state: "OPEN",
        base: "main",
        head: "feat",
        url: "",
        author: "",
      },
    });
    reapplyOptimisticThreads(other, [pending]);
    expect(other.ai.threads).toEqual([]);
    expect(other.ai.comments).toBe(0);
  });
});
