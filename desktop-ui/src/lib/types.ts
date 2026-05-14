export interface SpanSnapshot {
  text: string;
  color: string; // "#RRGGBB" or ""
}

export interface LineSnapshot {
  old_num: number | null;
  new_num: number | null;
  kind: "context" | "add" | "del" | "fold";
  spans: SpanSnapshot[];
}

export interface ThreadMessage {
  author: string;
  kind: "you" | "human" | "ai";
  timestamp: string;
  body_markdown: string;
}

export interface ThreadSnapshot {
  id: string;
  kind: "comment" | "question";
  file: string;
  line: number;
  source: string;
  synced: boolean;
  stale: boolean;
  resolved: boolean;
  root: ThreadMessage;
  replies: ThreadMessage[];
  /** For questions: id of the GitHub comment this thread was promoted to. */
  promoted_to: string | null;
}

export interface HunkSnapshot {
  header: string;
  old_start: number;
  old_count: number;
  new_start: number;
  new_count: number;
  lines: LineSnapshot[];
  threads: ThreadSnapshot[];
}

export interface FileSnapshot {
  path: string;
  status: "added" | "modified" | "deleted" | "renamed" | "copied" | "unmerged";
  additions: number;
  deletions: number;
  reviewed: boolean;
  compacted: boolean;
  risk: "high" | "med" | "low" | null;
  finding_count: number;
  comment_count: number;
  question_count: number;
  hunks: HunkSnapshot[];
}

export interface FlatFinding {
  id: string;
  file: string;
  line: number | null;
  severity: "high" | "med" | "low";
  title: string;
  message_markdown: string;
  /** Id of the GitHub comment this finding was promoted to. */
  promoted_to: string | null;
}

export interface AiSnapshot {
  fresh: boolean;
  summary_markdown: string | null;
  high: number;
  med: number;
  low: number;
  comments: number;
  questions: number;
  unpushed: number;
  threads: ThreadSnapshot[];
  findings: FlatFinding[];
}

export interface PrSnapshot {
  number: number;
  title: string;
  state: string;
  base: string;
  head: string;
}

export interface Panels {
  left: boolean;
  tree: boolean;
  right: boolean;
}

export interface WorktreeSnapshot {
  path: string;
  branch: string;
  is_current: boolean;
  is_pr: boolean;
  pr_number: number | null;
  is_merged: boolean;
}

export interface BranchInfo {
  name: string;
  upstream: string | null;
  is_current: boolean;
  is_merged: boolean;
  worktree_path: string | null;
}

export interface PrInfo {
  number: number;
  title: string;
  head_ref: string;
  state: "OPEN" | "MERGED" | "CLOSED";
  is_draft: boolean;
  author: string;
  assignees: string[];
  reviewers: string[];
  checks_state: "PASSING" | "FAILING" | "PENDING" | null;
  review_decision: "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED" | null;
  merged_at: string | null;
  approved_by_me: boolean;
}

export interface ProjectSnapshot {
  id: string;
  name: string;
  root_path: string;
  remote: string | null;
  is_active: boolean;
  /** Curated list — only the current branch plus user-added tracked branches. */
  local_branches: BranchInfo[];
  /** Recently-active local branches not already tracked (kept for internal use). */
  auto_branches: BranchInfo[];
  /** Open PRs authored by the current user. */
  my_prs: PrInfo[];
  /** Open PRs from others the current user hasn't approved yet (max 5). */
  prs_to_review: PrInfo[];
  /** Most recently merged PRs (max 5). */
  recently_merged: PrInfo[];
}

export interface CommitSummary {
  sha: string;
  title: string;
  author: string;
  age: string;
}

export interface TabSummary {
  idx: number;
  label: string;
  kind: "working" | "local_branch" | "remote_pr";
  branch: string | null;
  pr_number: number | null;
  repo_root: string;
  is_active: boolean;
}

export interface CheckSummary {
  name: string;
  /** "PENDING" | "COMPLETED" */
  status: string;
  /** e.g. "SUCCESS" | "FAILURE" | "NEUTRAL" — empty while status === "PENDING". */
  conclusion: string;
  url: string | null;
}

export interface GhCommentSummary {
  author: string;
  body: string;
  created_at: string;
  url: string;
}

export interface GhReviewSummary {
  author: string;
  /** "APPROVED" | "CHANGES_REQUESTED" | "COMMENTED" | "DISMISSED" */
  state: string;
  body: string;
  submitted_at: string;
}

export interface GithubStatusSnapshot {
  owner: string;
  repo: string;
  number: number;
  url: string;
  state: string;
  is_draft: boolean;
  title: string;
  author: string;
  head_ref: string;
  base_ref: string;
  /** "APPROVED" | "CHANGES_REQUESTED" | "REVIEW_REQUIRED" | null */
  review_decision: string | null;
  /** "MERGEABLE" | "CONFLICTING" | "UNKNOWN" | null */
  mergeable: string | null;
  labels: string[];
  checks: CheckSummary[];
  comments_count: number;
  reviews_count: number;
  recent_comments: GhCommentSummary[];
  recent_reviews: GhReviewSummary[];
  /** Unix seconds of last successful fetch, as a string. */
  last_updated: string | null;
}

export interface AppSnapshot {
  mode: "branch" | "unstaged" | "staged" | "history";
  /** Optional — populated by the engine when in history mode or branch-mode scope. */
  commits?: CommitSummary[];
  branch: string;
  base: string;
  input_mode: "normal" | "search" | "comment" | "filter" | "commit" | "confirm";
  files: FileSnapshot[];
  selected_file: number;
  current_hunk: number | null;
  filter: string | null;
  reviewed_count: number;
  total_count: number;
  ai: AiSnapshot;
  pr: PrSnapshot | null;
  panels: Panels;
  theme: string;
  watch_active: boolean;
  worktrees: WorktreeSnapshot[];
  projects: ProjectSnapshot[];
  local_branch: string | null;
  notification: string | null;
  tabs: TabSummary[];
  active_tab: number;
  /** Browser-view annotations for the active tab. */
  ui_annotations?: UiAnnotation[];
  /** Live GitHub status for the active tab (only when it's a remote PR with cached data). */
  github?: GithubStatusSnapshot | null;
  /** Which background fetches are currently in-flight. */
  bg_loading: LoadingFlags;
}

export interface LoadingFlags {
  pr_list: boolean;
  gh_status: boolean;
  gh_comments: boolean;
}

export interface PollResponse {
  revision: number;
  snapshot: AppSnapshot;
}

export interface UiAnnotation {
  id: string;
  /** Path portion of the URL (no query). */
  url: string;
  /** Best-effort CSS selector; null for cross-origin captures. */
  selector: string | null;
  box_x: number;
  box_y: number;
  box_w: number;
  box_h: number;
  viewport_w: number;
  viewport_h: number;
  text: string;
  timestamp: string;
  author: string;
  screenshot_path: string | null;
  stale: boolean;
  element_context?: string | null;
  dom_context?: UiDomContext | null;
}

export interface UiDomContext {
  selector?: string | null;
  summary?: string | null;
  node?: UiDomNodeContext | null;
  rect?: { left: number; top: number; width: number; height: number } | null;
  parent_chain?: UiDomNodeContext[];
  nearby_text?: string | null;
  outer_html?: string | null;
}

export interface UiDomNodeContext {
  tag?: string | null;
  id?: string | null;
  classes?: string[];
  role?: string | null;
  aria_label?: string | null;
  text?: string | null;
  attrs?: Record<string, string | null>;
}
