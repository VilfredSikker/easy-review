import type {
  AiSnapshot,
  AppSnapshot,
  CommitSummary,
  FileSnapshot,
  HunkSnapshot,
  LineSnapshot,
  PrInfo,
  PrSnapshot,
  ProjectSnapshot,
  SpanSnapshot,
  TabSummary,
  ThreadSnapshot,
  WorktreeSnapshot,
} from "$lib/types";

// ─── helpers ────────────────────────────────────────────────────────────────

const span = (text: string, color = ""): SpanSnapshot => ({ text, color });

const ctx = (old_num: number, new_num: number, text: string, color = ""): LineSnapshot => ({
  old_num,
  new_num,
  kind: "context",
  text,
  spans: [span(text, color)],
});
const add = (new_num: number, text: string, color = ""): LineSnapshot => ({
  old_num: null,
  new_num,
  kind: "add",
  text,
  spans: [span(text, color)],
});
const del = (old_num: number, text: string, color = ""): LineSnapshot => ({
  old_num,
  new_num: null,
  kind: "del",
  text,
  spans: [span(text, color)],
});

// ─── threads ────────────────────────────────────────────────────────────────

export const commentThread: ThreadSnapshot = {
  id: "thread-comment-1",
  kind: "comment",
  file: "packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte",
  line: 38,
  source: "local",
  synced: false,
  stale: false,
  resolved: false,
  root: {
    id: "thread-comment-1",
    author: "you",
    kind: "you",
    timestamp: new Date(Date.now() - 8 * 60 * 1000).toISOString(),
    body_markdown:
      "Should this be typed against SchemaMediaProperties instead? SchemaFullMedia includes fields we don't need at the option level.",
  },
  replies: [
    {
      id: "thread-comment-1-r1",
      author: "AI",
      kind: "ai",
      timestamp: new Date(Date.now() - 6 * 60 * 1000).toISOString(),
      body_markdown:
        "SchemaMediaProperties is a strict subset — just id, name, kind. Narrowing here is safe; only handleExperimentOptionSelect reads from this prop.",
    },
    {
      id: "thread-comment-1-r2",
      author: "maria-c",
      kind: "human",
      timestamp: new Date(Date.now() - 2 * 60 * 1000).toISOString(),
      body_markdown: "Agree. Worth doing — I'll tweak in this PR.",
    },
  ],
  promoted_to: null,
};

export const questionThread: ThreadSnapshot = {
  id: "thread-question-1",
  kind: "question",
  file: "packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte",
  line: 144,
  source: "local",
  synced: false,
  stale: false,
  resolved: false,
  root: {
    id: "thread-question-1",
    author: "you",
    kind: "you",
    timestamp: new Date(Date.now() - 12 * 60 * 1000).toISOString(),
    body_markdown:
      "Should handleExperimentOptionSelect live on the parent component instead?",
  },
  replies: [
    {
      id: "thread-question-1-r1",
      author: "AI",
      kind: "ai",
      timestamp: new Date(Date.now() - 10 * 60 * 1000).toISOString(),
      body_markdown:
        "The combobox owns comboboxOpen and uses it locally for show/hide. Lifting handleExperimentOptionSelect up would force the parent to manage that toggle too. Recommend keeping it here.",
    },
  ],
  promoted_to: null,
};

// ─── hunks ──────────────────────────────────────────────────────────────────

const mediaComboboxHunk1: HunkSnapshot = {
  header: "@@ -36,7 +36,11 @@ export type Props = {",
  old_start: 36,
  old_count: 7,
  new_start: 36,
  new_count: 11,
  lines: [
    ctx(36, 36, "  showPropertyInputs?: boolean;"),
    ctx(37, 37, "  /** When true, clicking an item always adds it (for bulk multi-instance mode) */"),
    add(38, "  experimentOptions?: ExperimentPropertyOption<SchemaFullMedia>[];"),
    add(39, "  onExperimentOptionSelect?: ("),
    add(40, "    _option: ExperimentPropertyOption<SchemaFullMedia>,"),
    add(41, "  ) => void;"),
    ctx(38, 42, "  isSinglePlate?: boolean;"),
    ctx(39, 43, "  trigger?: Snippet<"),
    ctx(40, 44, "    ["),
    ctx(41, 45, "      {"),
    ctx(42, 46, "        // …existing fields"),
    ctx(43, 47, "      },"),
    ctx(44, 48, "    ]"),
    ctx(45, 49, "  >;"),
  ],
  threads: [commentThread, questionThread],
};

const mediaComboboxHunk2: HunkSnapshot = {
  header: "@@ -137,3 +141,12 @@ function MediaCombobox() {",
  old_start: 137,
  old_count: 3,
  new_start: 141,
  new_count: 12,
  lines: [
    ctx(137, 141, "  comboboxOpen = false;"),
    ctx(138, 142, "}"),
    add(143, ""),
    add(144, "function handleExperimentOptionSelect("),
    add(145, "  option: ExperimentPropertyOption<SchemaFullMedia>,"),
    add(146, ") {"),
    add(147, "  onExperimentOptionSelect?.(option);"),
    add(148, "  comboboxOpen = false;"),
    add(149, "}"),
  ],
  threads: [],
};

// ─── files ──────────────────────────────────────────────────────────────────

const fileBase: Omit<FileSnapshot, "path" | "additions" | "deletions"> = {
  status: "modified",
  reviewed: false,
  compacted: false,
  risk: null,
  finding_count: 0,
  comment_count: 0,
  question_count: 0,
  hunks: [],
  source_index: 0,
  cache_key: "",
};

export const fileMediaCombobox: FileSnapshot = {
  ...fileBase,
  path: "packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte",
  additions: 39,
  deletions: 0,
  risk: "med",
  finding_count: 1,
  comment_count: 1,
  question_count: 1,
  hunks: [mediaComboboxHunk1, mediaComboboxHunk2],
};

export const fileVariantWarningCopy: FileSnapshot = {
  ...fileBase,
  path: "packages/discovery-platform/src/lib/variant-warning-copy.ts",
  additions: 8,
  deletions: 6,
  risk: "high",
  finding_count: 2,
};

export const fileVariantWarningTest: FileSnapshot = {
  ...fileBase,
  path: "packages/discovery-platform/src/lib/variant-warning-copy.test.ts",
  additions: 2,
  deletions: 2,
};

export const fileExperimentTemplate: FileSnapshot = {
  ...fileBase,
  path: "packages/discovery-platform/src/lib/experiment-template-resolution.ts",
  additions: 4,
  deletions: 0,
  risk: "low",
  finding_count: 1,
};

export const filePageTest: FileSnapshot = {
  ...fileBase,
  path: "packages/discovery-platform/src/routes/page.test.ts",
  additions: 0,
  deletions: 0,
  reviewed: true,
};

// ─── AI snapshots ───────────────────────────────────────────────────────────

export const aiWithFindings: AiSnapshot = {
  fresh: true,
  stale_reason: null,
  summary_markdown:
    "4 findings across 3 files. Two high-risk issues in variant-warning-copy.ts around fallback handling.",
  agent_summaries: {},
  high: 2,
  med: 1,
  low: 1,
  local_comment_count: 1,
  github_comment_count: 0,
  comments: 1,
  questions: 1,
  unpushed: 1,
  threads: [commentThread, questionThread],
  findings: [
    {
      id: "finding-1",
      file: "packages/discovery-platform/src/lib/variant-warning-copy.ts",
      line: 42,
      hunk_index: 0,
      severity: "high",
      title: "Fallback returns undefined when severity is missing — callers expect a string.",
      message_markdown: "",
      promoted_to: null,
      thread_id: null,
      expert_label: null,
      agent_label: "General",
    },
    {
      id: "finding-2",
      file: "packages/discovery-platform/src/lib/variant-warning-copy.ts",
      line: 67,
      hunk_index: 0,
      severity: "high",
      title: "Mapped key uses raw user input — risks collision with reserved _mismatchType.",
      message_markdown: "",
      promoted_to: null,
      thread_id: null,
      expert_label: null,
      agent_label: "General",
    },
    {
      id: "finding-medium-1",
      file: "packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte",
      line: 40,
      hunk_index: 0,
      severity: "med",
      title: "_option prefix signals unused but the param is used — drop the underscore.",
      message_markdown: "",
      promoted_to: null,
      thread_id: null,
      expert_label: null,
      agent_label: "General",
    },
    {
      id: "finding-4",
      file: "packages/discovery-platform/src/lib/experiment-template-resolution.ts",
      line: 18,
      hunk_index: 0,
      severity: "low",
      title: "Variable opts shadows outer scope — readability nit.",
      message_markdown: "",
      promoted_to: null,
      thread_id: null,
      expert_label: null,
      agent_label: "General",
    },
  ],
  has_review_json: true,
  eligible_comment_count: 0,
};

export const aiEmpty: AiSnapshot = {
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
  unpushed: 0,
  threads: [],
  findings: [],
  has_review_json: false,
  eligible_comment_count: 0,
};

function professorFinding(id: string, file: string, line: number, title: string): AiSnapshot["findings"][0] {
  return {
    id,
    file,
    line,
    hunk_index: 0,
    severity: "low",
    title,
    message_markdown: "",
    promoted_to: null,
    thread_id: null,
    expert_label: null,
    agent_label: "Professor",
  };
}

/** Professor-only review (matches single-agent dropdown UX). */
export const aiProfessorOnly: AiSnapshot = {
  fresh: false,
  stale_reason: "Review was generated for an older diff. Re-run or validate the review.",
  summary_markdown: null,
  agent_summaries: {
    Professor:
      "This branch adds replicate deviation APIs backed by a three-part hash and shared SQL CTEs.\n\nTests isolate pure helpers from DB/HTTP; the metric validation path still lacks 422 coverage.",
  },
  high: 0,
  med: 1,
  low: 14,
  local_comment_count: 0,
  github_comment_count: 0,
  comments: 0,
  questions: 0,
  unpushed: 0,
  threads: [],
  findings: [
    professorFinding("prof-1", "test_replicates.py", 1, "Pure-function unit tests isolated from DB and HTTP"),
    professorFinding("prof-2", "queries_replicates.py", 3, "Three-part replicate hash design"),
    professorFinding("prof-3", "queries_replicates.py", 46, "_PHYSICAL_WELL_KEY_CTE as a reusable, exported SQL fragment"),
    professorFinding("prof-4", "test_experiments.py", 2895, "No test for 422 paths in metric validation logic"),
    ...Array.from({ length: 10 }, (_, i) =>
      professorFinding(`prof-extra-${i}`, `src/module_${i}.py`, i + 10, `Teaching insight ${i + 1}`),
    ),
    {
      ...professorFinding("prof-med", "api_models.py", 3246, 'metric_name="" sentinel ambiguous vs str | None'),
      severity: "med",
    },
  ],
  has_review_json: true,
  eligible_comment_count: 0,
};

/** General + Professor + Security for multi-agent dropdown. */
export const aiMultiAgent: AiSnapshot = {
  ...aiWithFindings,
  agent_summaries: {
    Security:
      "Auth changes add JWT validation before handlers but skip audience checks — treat as blocking until `aud` is verified.",
    Professor:
      "JWT validation runs early in the request path, which is the right layering for this service.",
  },
  findings: [
    ...aiWithFindings.findings,
    professorFinding("prof-1", "src/auth.rs", 10, "JWT validation runs before handler body"),
    {
      id: "sec-1",
      file: "src/auth.rs",
      line: 22,
      hunk_index: 0,
      severity: "high",
      title: "Token parsed without audience check — verify issuer and aud claims.",
      message_markdown: "",
      promoted_to: null,
      thread_id: null,
      expert_label: null,
      agent_label: "Security",
    },
  ],
  high: 3,
  med: 1,
  low: 2,
};

// ─── PR snapshots ───────────────────────────────────────────────────────────

export const prDraft: PrSnapshot = {
  number: 1090,
  title: "DEV-5008 Show experiment params in selection dropdowns",
  state: "draft",
  base: "main",
  head: "show-experiment-params",
  url: "https://github.com/reshape/easy-review/pull/1090",
  author: "vilfred",
};

// ─── worktrees + commits ────────────────────────────────────────────────────

export const worktreesMulti: WorktreeSnapshot[] = [
  { path: "/Users/vilfred/Projects/discovery-platform", branch: "show-experiment-params", is_current: true, is_pr: false, pr_number: null, is_merged: false },
  { path: "/Users/vilfred/Projects/discovery-platform/.worktrees/fix-forward-button", branch: "fix-forward-button", is_current: false, is_pr: true, pr_number: 142, is_merged: false },
  { path: "/Users/vilfred/Projects/.codex/worktrees/c175", branch: "c175", is_current: false, is_pr: false, pr_number: null, is_merged: true },
];

export const commitsRich: CommitSummary[] = [
  { sha: "2afab9e0", title: "Fix lint errors in variant warning copy typings", author: "Vilfred", age: "45m" },
  { sha: "d1a60769", title: "Add experiment-option resolver to combobox", author: "Claude", age: "1d" },
  { sha: "54a9a3d2", title: "Wire warning metadata through dropdowns", author: "Vilfred", age: "2h" },
  { sha: "2356d55a", title: "Merge branch 'main' into show-experiment-params", author: "Vilfred", age: "1h" },
];

/** Active working-tree tab — ScopeSelector shows unstaged/staged/commits even when pr is set. */
export const tabsWorkingActive: TabSummary[] = [
  {
    idx: 0,
    label: "show-experiment-params",
    kind: "working",
    branch: "show-experiment-params",
    pr_number: 142,
    repo_root: "/Users/vilfred/Projects/discovery-platform",
    is_active: true,
    change_token: "",
  },
];

// ─── multi-folder files (for tree visualization) ────────────────────────────

export const multiFolderFiles: FileSnapshot[] = [
  fileMediaCombobox,
  fileVariantWarningCopy,
  fileVariantWarningTest,
  fileExperimentTemplate,
  filePageTest,
  { ...fileBase, path: "apps/web/src/routes/+page.svelte", additions: 5, deletions: 3 },
  { ...fileBase, path: "apps/web/src/routes/api/health/+server.ts", additions: 12, deletions: 0, risk: "low", finding_count: 1 },
  { ...fileBase, path: "apps/admin/src/lib/auth/middleware.ts", additions: 7, deletions: 2 },
  { ...fileBase, path: "infra/terraform/main.tf", additions: 24, deletions: 0 },
  { ...fileBase, path: "infra/terraform/variables.tf", additions: 6, deletions: 0 },
  { ...fileBase, path: "README.md", additions: 1, deletions: 1 },
];

// ─── snapshot builders ──────────────────────────────────────────────────────

const baseSnapshot: AppSnapshot = {
  mode: "branch",
  branch: "show-experiment-params",
  base: "main",
  input_mode: "normal",
  files: [fileMediaCombobox, fileVariantWarningCopy, fileVariantWarningTest, fileExperimentTemplate, filePageTest],
  selected_file: 0,
  current_hunk: 0,
  filter: null,
  reviewed_count: 2,
  total_count: 5,
  ai: aiWithFindings,
  pr: prDraft,
  panels: { left: true, tree: true, right: true },
  theme: "dark",
  watch_active: true,
  watch_status: { active: true, branch: null, root_path: null },
  worktrees: worktreesMulti,
  projects: [],
  local_branch: null,
  notification: null,
  tabs: tabsWorkingActive,
  active_tab: 0,
  bg_loading: { pr_list: false, gh_status: false, gh_comments: false },
  commits: commitsRich,
};

/** Snapshot used for full-page mock recreation. */
export const richSnapshot: AppSnapshot = { ...baseSnapshot };

const remoteOnlyRecentPr: PrInfo = {
  number: 123,
  title: "Fix flaky auth callback",
  head_ref: "fix-auth-callback",
  state: "OPEN",
  is_draft: false,
  author: "octocat",
  assignees: [],
  reviewers: [],
  checks_state: null,
  review_decision: null,
  merged_at: null,
  approved_by_me: false,
  base_ref: "main",
  head_oid: "abc123",
  updated_at: "2026-05-22T10:00:00Z",
};

export const remoteOnlyProject: ProjectSnapshot = {
  id: "remote-owner-repo",
  name: "owner/repo",
  root_path: "",
  remote: "owner/repo",
  remote_only: true,
  is_active: true,
  local_branches: [],
  auto_branches: [],
  saved_prs: [],
  my_prs: [],
  prs_to_review: [],
  recent_prs: [remoteOnlyRecentPr],
  recently_merged: [],
  pr_cache_stale: false,
  pr_cache_age_ms: 30_000,
};

export const remoteOnlyProjectSnapshot: AppSnapshot = {
  ...baseSnapshot,
  branch: "PR #123",
  projects: [remoteOnlyProject],
  tabs: [
    {
      idx: 0,
      label: "owner/repo#123",
      kind: "remote_pr",
      branch: null,
      pr_number: 123,
      repo_root: "",
      is_active: true,
      change_token: "",
    },
  ],
  active_tab: 0,
};

/** Snapshot for the "empty / no AI data" scenario. */
export const emptySnapshot: AppSnapshot = {
  ...baseSnapshot,
  ai: aiEmpty,
  pr: null,
  reviewed_count: 0,
  total_count: 0,
  files: [],
  commits: [],
};

/** Snapshot for the multi-folder file tree scenario. */
export const multiFolderSnapshot: AppSnapshot = {
  ...baseSnapshot,
  files: multiFolderFiles,
  total_count: multiFolderFiles.length,
  reviewed_count: 1,
};

/** Classic file-tree mock: mixed git statuses, findings, deep folder breadcrumb. */
export const classicTreeFiles: FileSnapshot[] = [
  {
    ...fileBase,
    path: "packages/discovery-platform/src/lib/components/combobox/media/MediaCombobox.svelte",
    status: "modified",
    additions: 39,
    deletions: 0,
    risk: "med",
    finding_count: 1,
    comment_count: 1,
    question_count: 1,
    source_index: 0,
  },
  {
    ...fileBase,
    path: "packages/discovery-platform/src/lib/variant-warning-copy.ts",
    status: "added",
    additions: 8,
    deletions: 0,
    risk: "high",
    finding_count: 2,
    source_index: 1,
  },
  {
    ...fileBase,
    path: "apps/admin/src/lib/auth/middleware.ts",
    status: "modified",
    additions: 7,
    deletions: 2,
    risk: "low",
    finding_count: 0,
    source_index: 2,
  },
  {
    ...fileBase,
    path: "packages/discovery-platform/src/routes/page.test.ts",
    status: "deleted",
    additions: 0,
    deletions: 12,
    reviewed: true,
    source_index: 3,
  },
  {
    ...fileBase,
    path: "README.md",
    status: "modified",
    additions: 1,
    deletions: 1,
    reviewed: true,
    source_index: 4,
  },
];

export const classicTreeSnapshot: AppSnapshot = {
  ...baseSnapshot,
  files: classicTreeFiles,
  selected_file: 0,
  total_count: classicTreeFiles.length,
  reviewed_count: 2,
  ai: aiWithFindings,
};
