export interface SpanSnapshot {
  text: string;
  color: string; // "#RRGGBB" or ""
}

export interface LineSnapshot {
  old_num: number | null;
  new_num: number | null;
  kind: "context" | "add" | "del" | "fold";
  /** Always-present plain text — use directly when spans are absent. */
  text: string;
  /** Syntax-highlighted spans — populated client-side by Shiki (not sent over IPC). */
  spans?: SpanSnapshot[];
}

export interface ThreadMessage {
  id: string;
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
  /** Inclusive end line when the thread spans multiple diff lines. */
  line_end?: number | null;
  /** Review side for range matching: LEFT | RIGHT. */
  side?: string;
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
  /** True when the diff is large and this file's hunks haven't been parsed yet. */
  is_lazy_stub?: boolean;
  /** Index into the backend's full file list — pass to `select_file`. */
  source_index: number;
  /** Stable hash for the frontend highlight cache. Changes when the diff changes. */
  cache_key: string;
}

export interface FilterSuggestionSnapshot {
  kind: "preset" | "history";
  name: string;
  expr: string;
}

export interface ExpertInfo {
  id: string;
  label: string;
  description: string;
}

export interface ReviewerInfo {
  kind: string;
  label: string;
  description: string;
}

export interface FlatFinding {
  id: string;
  file: string;
  line: number | null;
  hunk_index: number | null;
  severity: "high" | "med" | "low";
  /** Specialized expert label when finding.category is an expert id. */
  expert_label: string | null;
  /** Agent that produced this finding (General, Security, Professor, …). */
  agent_label: string;
  title: string;
  message_markdown: string;
  /** Id of the GitHub comment this finding was promoted to. */
  promoted_to: string | null;
  /** Id of the root comment thread created via "Ask AI" for this finding. */
  thread_id: string | null;
}

export interface TriagePriorityFileSnapshot {
  path: string;
  reason: string;
  risk: string;
}

export interface TriageSnapshot {
  fresh: boolean;
  first_impression: string;
  verdict_primary: string;
  experts: string[];
  rationale: string;
  confidence: string;
  priority_files: TriagePriorityFileSnapshot[];
  files_changed: number;
  approx_risk: string;
  domains: string[];
}

export interface AiSnapshot {
  fresh: boolean;
  stale_reason: string | null;
  summary_markdown: string | null;
  /** Per-agent markdown summaries from expert/professor sidecars (keyed by agent label). */
  agent_summaries: Record<string, string>;
  high: number;
  med: number;
  low: number;
  local_comment_count: number;
  github_comment_count: number;
  comments: number;
  questions: number;
  unpushed: number;
  threads: ThreadSnapshot[];
  findings: FlatFinding[];
  /** Whether `{er_dir}/review.json` exists (batch validate target). */
  has_review_json: boolean;
  /** Top-level GitHub comments eligible for batch validate (!resolved, !outdated). */
  eligible_comment_count: number;
  triage: TriageSnapshot | null;
}

export interface PrSnapshot {
  number: number;
  title: string;
  state: string;
  base: string;
  head: string;
  url: string;
  author: string;
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
  pr_number?: number | null;
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
  /** Base branch (e.g. "main"). Used as a hint when opening the PR. */
  base_ref: string;
  /** Head commit SHA. Used as the freshness key for the PR open cache. */
  head_oid: string;
  /** PR `updatedAt` ISO timestamp. Part of the freshness key. */
  updated_at: string;
}

export interface ProjectSnapshot {
  id: string;
  name: string;
  root_path: string;
  remote: string | null;
  remote_only?: boolean;
  is_active: boolean;
  /** Curated list — only the current branch plus user-added tracked branches. */
  local_branches: BranchInfo[];
  /** Recently-active local branches not already tracked (kept for internal use). */
  auto_branches: BranchInfo[];
  /** Manually bookmarked PRs. */
  saved_prs: PrInfo[];
  /** Open PRs authored by the current user. */
  my_prs: PrInfo[];
  /** Open PRs from others the current user hasn't approved yet (max 5). */
  prs_to_review: PrInfo[];
  /** PRs opened for review recently. */
  recent_prs: PrInfo[];
  /** Most recently merged PRs (max 5). */
  recently_merged: PrInfo[];
  /** True when cached PR data is older than TTL. */
  pr_cache_stale?: boolean;
  /** Age of cached PR data in ms. */
  pr_cache_age_ms?: number | null;
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
  change_token: string;
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
  body: string;
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
  is_authored_by_me: boolean;
}

export interface AppSnapshot {
  mode: "branch" | "unstaged" | "staged" | "history" | "pr" | "conflicts" | "hidden";
  /** Optional — populated by the engine when in history mode or branch-mode scope. */
  commits?: CommitSummary[];
  /** SHA of the selected commit when in history mode; null otherwise. */
  selected_commit_sha?: string | null;
  /** +/- counts for the unstaged / staged diffs (for scope-selector counters). */
  unstaged_stat?: { additions: number; deletions: number };
  staged_stat?: { additions: number; deletions: number };
  /** PR detected for the active branch (PR-list cache). Drives the Local|PR Diff toggle. */
  detected_pr_number?: number | null;
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
  /** Feature flags from ErConfig — controls which diff scopes appear in the UI. */
  features?: FeatureFlagsSnapshot;
  /** Display options from ErConfig (diff gear menu still uses localStorage for split/unified). */
  display?: DisplayConfigSnapshot;
  watch_active: boolean;
  watch_status: WatchStatusSnapshot;
  worktrees: WorktreeSnapshot[];
  projects: ProjectSnapshot[];
  local_branch: string | null;
  /** True when the viewed local branch is checked out (enables Unstaged/Staged/Commits scopes). */
  local_branch_checked_out?: boolean;
  notification: string | null;
  tabs: TabSummary[];
  active_tab: number;
  /** Browser-view annotations for the active tab. */
  ui_annotations?: UiAnnotation[];
  /** Per-tab browser pane (active tab only). */
  browser?: BrowserSnapshot;
  /** Live GitHub status for the active tab (only when it's a remote PR with cached data). */
  github?: GithubStatusSnapshot | null;
  /** Which background fetches are currently in-flight. */
  bg_loading: LoadingFlags;
  /** Running/done/failed background AI commands for the active tab. */
  agent_commands?: AgentCommandStatus[];
  /** Recent agent log output for the active tab. */
  agent_log?: AgentLogEntry[];
  /** Human-readable label for the currently selected AI provider/model. */
  active_ai_label?: string;
  /** Claude Code effort level (`low` … `max`). */
  active_ai_effort?: string | null;
  /** Filter presets + recent filter history for the active tab. */
  filter_suggestions?: FilterSuggestionSnapshot[];
  /** Session-scoped background review tasks across all tabs. */
  background_tasks?: BackgroundTaskSnapshot[];
  inbox_items?: InboxItemSnapshot[];
  inbox_unread_count?: number;
  inbox_last_refresh_ms?: number;
  arena_enabled?: boolean;
  active_arena_run?: string | null;
  arena_runs?: import("./types/arena").ArenaRunSummary[];
}

export interface InboxTargetSnapshot {
  project_id?: string | null;
  repo_root?: string | null;
  remote?: string | null;
  pr_number?: number | null;
  branch?: string | null;
  url?: string | null;
}

export interface InboxItemSnapshot {
  id: string;
  kind: string;
  severity: string;
  title: string;
  body: string;
  source: string;
  target: InboxTargetSnapshot;
  created_at_ms: number;
  read_at_ms?: number | null;
  dedupe_key: string;
}

export interface BackgroundTaskSnapshot {
  id: string;
  kind: string;
  label: string;
  target_label: string;
  scope: string;
  /** "running" | "done" | "failed" */
  status: string;
  error?: string | null;
  started_at_ms: number;
  finished_at_ms?: number | null;
  /** Last ~40 log entries for this task (from backend snapshot tail). */
  recent_log?: AgentLogEntry[];
  /** Path to the debug log file written after task completion, if available. */
  debug_log_path?: string | null;
}


export interface WatchStatusSnapshot {
  active: boolean;
  branch: string | null;
  root_path: string | null;
}

export interface LoadingFlags {
  pr_list: boolean;
  gh_status: boolean;
  gh_comments: boolean;
}

export interface AgentCommandStatus {
  name: string;
  /** "running" | "done" | "failed" */
  status: string;
  error?: string | null;
}

export interface AgentLogEntry {
  command_name: string;
  /** "stdout" | "stderr" | "status" */
  source: string;
  text: string;
}

export interface AiModelInfo {
  id: string;
  label: string;
  is_selected: boolean;
  description?: string | null;
  cost_per_1k_in?: number | null;
  cost_per_1k_out?: number | null;
  avg_latency_ms?: number | null;
}

export interface AiProviderInfo {
  id: string;
  label: string;
  models: AiModelInfo[];
  is_selected: boolean;
}

export interface PollResponse {
  revision: number;
  content_revision: number;
  chrome_revision: number;
  /** Merge sidebar/chrome only; keep existing file hunks and highlight spans. */
  chrome_only: boolean;
  /** Full snapshot; `null` when both revisions are unchanged since last poll. */
  snapshot: AppSnapshot | null;
}

export interface BrowserSnapshot {
  url: string;
  layout: "hidden" | "split" | "fullscreen";
  split_ratio: number;
  annotate_mode: boolean;
  show_tooltips: boolean;
}

export interface UiAnnotation {
  id: string;
  /** Canonical origin + path page key. Older rows may contain path-only values. */
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

export type {
  ArenaConfig,
  ArenaFinding,
  ArenaRun,
  ArenaRunSnapshot,
  ArenaRunSummary,
  ArenaScope,
  ArenaSeverity,
  Ballot,
  FunnelStages,
  MatrixRow,
  Reviewer,
  RunStatus,
  Verdict,
  Vote,
} from "./types/arena";

export interface UiDomNodeContext {
  tag?: string | null;
  id?: string | null;
  classes?: string[];
  role?: string | null;
  aria_label?: string | null;
  text?: string | null;
  attrs?: Record<string, string | null>;
}

/** Patch value for `apply_config_patch` (matches Rust `ConfigFieldValue`). */
export type ConfigFieldValue = boolean | string | number;

export type ConfigHubField =
  | { kind: "section"; title: string }
  | {
      kind: "bool";
      key: string;
      label: string;
      description: string;
      value: boolean;
    }
  | {
      kind: "cycle";
      key: string;
      label: string;
      description: string;
      options: string[];
      value: string;
    }
  | {
      kind: "text";
      key: string;
      label: string;
      description: string;
      placeholder: string;
      value: string;
      strict: boolean;
    }
  | { kind: "listEntry"; key: string; label: string; index: number }
  | { kind: "listAdd"; key: string; label: string };

export type SettingsTab = "general" | "terminal";

export interface DesktopSettingsSnapshot {
  general: ConfigHubField[];
  app: ConfigHubField[];
  terminal: ConfigHubField[];
  agentEffort: string;
  hasLocalConfig: boolean;
  /** Repo `.er-config.toml` `[display].theme` — overrides global for the TUI. */
  localThemeOverride?: string | null;
  repoRoot: string;
}

export interface GetConfigHubResponse {
  settings: DesktopSettingsSnapshot;
  providers: AiProviderInfo[];
}

export interface FeatureFlagsSnapshot {
  viewBranch: boolean;
  viewUnstaged: boolean;
  viewStaged: boolean;
  viewHistory: boolean;
  viewConflicts: boolean;
  viewHidden: boolean;
}

export interface DisplayConfigSnapshot {
  lineNumbers: boolean;
  wrapLines: boolean;
  splitDiff: boolean;
  tabWidth: number;
}
