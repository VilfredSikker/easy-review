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

/** The AI action behind a question or note reply: `ask` answers it,
 *  `elaborate` rewrites it into a self-contained one. */
export type AiAction = "ask" | "elaborate";

export interface ThreadMessage {
  id: string;
  author: string;
  kind: "you" | "human" | "ai";
  timestamp: string;
  body_markdown: string;
  origin?: "thread_reply" | "finding_response";
  source?: string;
  synced?: boolean;
  editable?: boolean;
  deletable?: boolean;
  /** The action that wrote this AI reply, and on the pending "…thinking"
   *  reply the action that is running. Absent on a person's text and on AI
   *  replies written before the field existed. */
  ai_action?: AiAction;
}

export interface FindingResponseSnapshot {
  id: string;
  author: string;
  kind: "you" | "human" | "ai";
  timestamp: string;
  body_markdown: string;
  /** "arbiter" marks a ruling the arbiter wrote, so a reader never has to
   *  recognise it by its sentence. */
  origin: "finding_response" | "arbiter";
  editable: boolean;
  deletable: boolean;
}

export interface ThreadSnapshot {
  id: string;
  kind: "comment" | "question" | "note";
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

export type FileKind = "production" | "test" | "storybook" | "generated" | "docs";

/** How closely a file needs reading (ADR 0048). `ai` declares no tier and
 *  leaves the file to the AI review's rating. */
export type RiskTier = "high" | "med" | "low" | "ai";

/** Which config table a risk rule came from: `[risk.<repo>]` or `[user_risk]`. */
export type RiskRuleSource = "project" | "user";

/** The rule that set a path's tier. */
export interface MatchedRiskRule {
  /** The rule key as written: a path, glob or file type. */
  key: string;
  label: string | null;
  source: RiskRuleSource;
}

export interface FileSnapshot {
  /** Complete document identity, independent of visible hunks. */
  preview_key?: string;
  path: string;
  status: "added" | "modified" | "deleted" | "renamed" | "copied" | "unmerged";
  /** Source path of a rename or copy; null for every other status. */
  old_path?: string | null;
  /** Git's similarity percentage for a rename or copy, when reported. */
  similarity?: number | null;
  /** File kind after the repo's `[file_kinds]` overrides — matches `kind:` filtering. */
  kind?: FileKind;
  additions: number;
  deletions: number;
  reviewed: boolean;
  compacted: boolean;
  /** The AI review's rating from review.json; null before a review. */
  risk: "high" | "med" | "low" | null;
  /** The tier the repo's risk rules give this path. For `ai`, show `risk`
   *  instead, or nothing before a review. Optional only for fixtures. */
  risk_tier?: RiskTier;
  /** The rule behind `risk_tier`; null when a default, or nothing, answered. */
  risk_rule?: MatchedRiskRule | null;
  finding_count: number;
  comment_count: number;
  question_count: number;
  hunks: HunkSnapshot[];
  /** True when the diff is large and this file's hunks haven't been parsed yet. */
  is_lazy_stub?: boolean;
  /** Index into the backend's full file list — pass to `select_file`. */
  source_index: number;
  /** Stable per-file content hash for the frontend highlight cache. */
  cache_key: string;
  /** Fingerprint of the full hunk payload (lines + inline threads). */
  delta_key?: string;
  /**
   * Differential snapshot: hunks omitted because we already hold identical
   * content for `delta_key`. Resolved by `resolveOmittedHunks` before the
   * snapshot is stored; never true on stored snapshots.
   */
  hunks_omitted?: boolean;
}

export interface RelatedFileRef {
  path: string;
  /** "test" | "style" | "story" | "snapshot" | "other" */
  kind: string;
  reason: string;
}

export interface TourFileRef {
  path: string;
  reason: string;
  findingIds: string[];
  /** Co-located files (tests/styles/stories/snapshots) nested under this row.
   *  Optional: absent on tour snapshots produced before this field existed —
   *  every reader must guard with `?? []`. */
  related?: RelatedFileRef[];
}

/**
 * A message the engine wants surfaced.
 *
 * Dedupe on `seq`, which advances on every notify — including a repeat of the
 * same text. The backend leaves this set forever, so comparing `message` would
 * swallow the second occurrence of any identical toast.
 */
export interface NotificationSnapshot {
  message: string;
  seq: number;
  long: boolean;
}

export interface PillarSnapshot {
  id: string;
  title: string;
  descriptionMarkdown: string;
  /** One-line summary for the pillar lists. Empty on a tour written before
   *  pillars had one: fall back to `descriptionMarkdown`. Optional for fixtures. */
  summaryMarkdown?: string;
  importance: number;
  foundation: boolean;
  files: TourFileRef[];
  reviewedCount: number;
  totalCount: number;
}

export interface TourSnapshot {
  /** True when a guided tour exists for the current view context (PR vs branch). */
  available: boolean;
  /** True when the tour matches the current diff. When false and `available`,
   *  the UI offers a "Re-run guide" affordance. */
  fresh: boolean;
  /** Which diff the tour is attached to: `"pr"` or `"branch"`. */
  scope: "pr" | "branch" | "";
  title: string;
  overviewMarkdown: string;
  pillars: PillarSnapshot[];
  /** When the tour was generated (ISO-8601); empty when the sidecar does not say. */
  createdAt?: string;
  /** Hash of the diff the tour was generated against — a diff hash, not a
   *  commit. The commit the on-screen diff came from is `diff_stale.built_from_oid`. */
  diffHash?: string;
}

/**
 * What the other view of a local PR tab holds (ADR 0004), so an empty view can
 * say where the artifacts are. A tour is reused across the two views on a
 * matching diff hash (ADR 0025): when `tour_shared`, this view's tour *is*
 * that bucket's, so never claim "no tour" here while `tour.available`.
 */
export interface OtherBucketSnapshot {
  view: "pr" | "branch";
  has_triage: boolean;
  has_review: boolean;
  /** Open findings there (resolved and arbiter-dropped left out). */
  finding_count: number;
  /** Pillars in the tour stored there; 0 with none. */
  pillar_count: number;
  tour_shared: boolean;
  /** Some artifact there was built against the diff on screen. */
  same_diff: boolean;
}

export interface FilterSuggestionSnapshot {
  kind: "kind" | "preset" | "history";
  name: string;
  expr: string;
  /** Files the suggestion selects; set for `kind` suggestions. */
  files?: number;
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

/** How much a finding's grade can be trusted. Lower is more trustworthy. */
export type Confidence = "confirmed" | "tentative" | "informational" | "dropped";

export interface FlatFinding {
  id: string;
  file: string;
  line: number | null;
  hunk_index: number | null;
  severity: "high" | "med" | "low";
  /** The producer's own grade, which an arbiter pass may have replaced. */
  confidence: Confidence;
  /** `producers · category`, built by the engine — the same tag the TUI draws. */
  lens_category: string;
  /** The user has fixed this one. Rows carrying it arrive in
   *  `resolved_findings` and render dimmed. */
  resolved: boolean;
  /** Specialized expert label when finding.category is an expert id. */
  expert_label: string | null;
  /** Agent that produced this finding (General, Security, Professor, …). */
  agent_label: string;
  /** Every lens that raised it; more than one means several experts found it. */
  raised_by: string[];
  /** Display names for every reviewer that contributed to the merged claim. */
  providers: string[];
  /** True when at least one contributing lens is the security lens. */
  security_stage: boolean;
  title: string;
  message_markdown: string;
  /** Id of the GitHub comment this finding was promoted to. */
  promoted_to: string | null;
  /** Id of the root comment thread created via "Ask AI" for this finding. */
  thread_id: string | null;
  /** AI validation replies stored on the finding in review.json. */
  responses?: FindingResponseSnapshot[];
}

/** Per-file risk assessment from review.json — distinct from line findings. */
export interface FileRiskSnapshot {
  path: string;
  risk: "high" | "med" | "low";
  risk_reason: string;
  summary: string;
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
  /** `isolated` | `contained` | `broad` | `unknown`. Optional for fixtures. */
  reach?: string;
  reach_reason?: string;
  touch_points?: string[];
  /** Only sent when the agent cited where the guard is checked. */
  guard?: { kind: string; name: string; evidence: string } | null;
  /** Evidenced reasons for routing security review. Empty means no evidence. */
  sensitive: { kind: string; evidence: string; note: string }[];
  /** When triage ran (ISO-8601); empty when the sidecar does not say. Optional for fixtures. */
  created_at?: string;
  /** Hash of the diff triage ran against. */
  diff_hash?: string;
}

/** Backend-owned progress for one review flow bound to a diff generation. */
export interface ReviewFlowTaskProgress {
  id: string;
  stage: "triage" | "finding" | "verifying";
  lens: string | null;
  label: string;
  purpose: string;
  provider_id: string;
  provider_label: string;
  model_id: string;
  model_label: string;
  status: "waiting" | "queued" | "running" | "done" | "failed" | "cancelled" | "skipped";
  error: string | null;
  background_task_id: string | null;
  round: number | null;
  total_rounds: number | null;
}

export interface ReviewFlowSnapshot {
  state: "triaging" | "skipped" | "awaiting_confirmation" | "finding" | "verifying" | "done" | "failed" | "cancelled";
  tasks?: ReviewFlowTaskProgress[];
  depth: "quick" | "standard" | "deep" | null;
  diff_hash: string;
  lenses: string[];
  pending_finders: string[];
  failed_finders: { lens: string; reason: string }[];
  successful_finder_providers: string[];
  successful_arbiter_provider: string | null;
  providers_used: string[];
  arena_run_id: string | null;
  estimated_cost: number | null;
  estimated_latency_seconds: number | null;
  kept: number;
  dropped: number;
  awaiting_accept: number;
  reason: string | null;
  draining: boolean;
}

export interface ChecklistItemSnapshot {
  id: string;
  text: string;
  /** Outcome category (`schema` / `tests` / `api` / `auth` / `plan`) — free-form,
   *  and empty for checklists generated before those categories existed. */
  category: string;
  checked: boolean;
  /** Finding ids this item is about, for linking into the review. */
  related_findings: string[];
  /** File paths this item is about, for jumping into the diff. */
  related_files: string[];
}

export interface ChecklistSnapshot {
  /** File order is the toggle address: this array index goes back over IPC. */
  items: ChecklistItemSnapshot[];
  /** False when the checklist was generated against a different diff. */
  fresh: boolean;
}

export interface DiagramPresetSnapshot {
  /** "mental-model" | "subsystems" | "flows" */
  kind: string;
  label: string;
  description: string;
}

export interface DiagramSnapshot {
  /** File-stem id (`diagrams/<id>.json`) — delete/regenerate target. */
  id: string;
  /** "mental-model" | "subsystems" | "flows" | "custom" */
  kind: string;
  title: string;
  /** User prompt for custom diagrams (empty for presets). */
  prompt: string;
  /** Bare mermaid source. */
  mermaid: string;
  /** True when the diagram's diff hash matches the current diff. */
  fresh: boolean;
  created_at: string;
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
  notes: number;
  unpushed: number;
  threads: ThreadSnapshot[];
  findings: FlatFinding[];
  /** Findings the arbiter ruled out, or folded into another. Reported so the
   *  card can say how many it is not showing. */
  arbiter_dropped: number;
  arbiter_merged: number;
  /** Verdicts that matched no finding — the grades exist but no longer describe
   *  this review, so re-running validation is what fixes it. */
  arbiter_unmatched: number;
  /** Findings whose confidence the arbiter regraded. */
  arbiter_regraded: number;
  /** Per-file risk assessments from review.json (not counted as findings). */
  file_risks: FileRiskSnapshot[];
  /** Findings the user has resolved. Out of `findings` so nothing reading that
   *  list changes meaning; the diff view shows these behind a toggle. */
  resolved_findings: FlatFinding[];
  /** Findings an arbiter ruled out. Counted, and listed when expanded. */
  dropped_findings: FlatFinding[];
  /** The gate this review defaults to, resolved by the engine. */
  min_trust_default: Confidence;
  /** When the review was generated (ISO-8601). Null with no review; empty when
   *  the sidecar does not say. Optional for fixtures. */
  review_created_at?: string | null;
  /** Hash of the diff the review was generated against; null with no review. */
  review_diff_hash?: string | null;
  /** Whether `{er_dir}/review.json` exists (batch validate target). */
  has_review_json: boolean;
  /** Top-level GitHub comments eligible for batch validate (!resolved, !outdated). */
  eligible_comment_count: number;
  triage: TriageSnapshot | null;
  /** The review checklist (`checklist.json`) — the outcomes a human verifies.
   *  Null when the view bucket has none. */
  checklist: ChecklistSnapshot | null;
  /** Mermaid diagrams of the diff (`diagrams/*.json`), for the Context tab. */
  diagrams: DiagramSnapshot[];
  /** Built-in generate presets from the engine catalog (never hand-rolled in UI). */
  diagram_presets: DiagramPresetSnapshot[];
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
  /** Repo (owner/repo slug) this worktree belongs to, from its own git remote. */
  remote: string | null;
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
  /** Most recently merged PRs, newest first (max 25; the sidebar pages them). */
  recently_merged: PrInfo[];
  /** True when cached PR data is older than TTL. */
  pr_cache_stale?: boolean;
  /** Age of cached PR data in ms. */
  pr_cache_age_ms?: number | null;
  /** Auto-run triage on new/updated open PRs while Desktop is open. */
  auto_triage?: boolean;
  /** Also triage your own open PRs when auto_triage is on. */
  auto_triage_own_prs?: boolean;
  /** When to auto-triage: new-and-push | new-only | review-requested */
  auto_triage_when?: string;
  /** Skip auto-triage when filtered diff exceeds this size (KB). 0 = no limit. */
  auto_triage_max_diff_kb?: number;
  /** Glob patterns excluded from AI review diffs. */
  review_ignore_globs?: string[];
  /** PR numbers hidden via sidebar Ignore. */
  dismissed_prs?: number[];
}

export interface CommitSummary {
  sha: string;
  title: string;
  author: string;
  /** ISO 8601 commit timestamp; render via `timeAgo()` from `$lib/time`. */
  committed_at: string;
  /** Null when unknown (a PR commit not in the local object DB), not zero. */
  additions?: number | null;
  deletions?: number | null;
  /** On no remote yet. Always false for commits listed from GitHub. */
  unpushed?: boolean;
}

export interface TabSummary {
  idx: number;
  label: string;
  kind: "working" | "local_branch" | "remote_pr";
  branch: string | null;
  pr_number: number | null;
  /** Repo (owner/repo slug) this tab actually views — may differ from the active project's remote. */
  remote: string | null;
  repo_root: string;
  is_active: boolean;
  change_token: string;
  /**
   * Diff built from an older commit than origin has. Active tab: same as
   * `AppSnapshot.diff_stale` being set. Other tabs: PR tabs only, so an
   * inactive branch tab reads false even when its base moved.
   */
  stale?: boolean;
}

export interface CheckSummary {
  name: string;
  /** "PENDING" | "COMPLETED" */
  status: string;
  /** e.g. "SUCCESS" | "FAILURE" | "NEUTRAL" — empty while status === "PENDING". */
  conclusion: string;
  url: string | null;
  /** ISO-8601; null until reached. */
  started_at?: string | null;
  completed_at?: string | null;
  /** Wall time of a finished check; null while running or untimed. */
  duration_secs?: number | null;
  /** Status-context text (a deploy preview's "Ready"), shown where there is no duration. */
  description?: string | null;
}

/** One person or team on a PR's reviewer list. */
export interface GhReviewerSummary {
  reviewer: string;
  is_team: boolean;
  /** A review is requested from them now. */
  requested: boolean;
  /** "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED" | "COMMENTED"; null until they review. */
  state: string | null;
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

/**
 * `gh stack` (github/gh-stack) state for the active tab's branch, driving the
 * BranchCard stack control.
 *
 * Present on every tab whose viewed branch is a local checkout — including
 * before the first lookup, when `layers` is empty and `unavailable` is `null`
 * (the control offers the lazy `refresh_stack` then). `null`/absent on tabs with
 * no such checkout (remote-PR tabs, local PR views whose head isn't checked
 * out), which is what hides the control.
 */
export interface StackSnapshot {
  /** Trunk the stack is rooted on (`main`), shown after the layers. */
  trunk: string;
  /** One entry per stack layer, ordered top-of-stack first (may lack a PR). */
  layers: StackLayerSnapshot[];
  /** 1-based position counted from the top of the stack (`1 / 3` is the top). */
  position: number | null;
  /** Layer count — the denominator of the `n / size` badge. */
  size: number;
  /** Why there's no stack: branch outside any stack, extension not installed. */
  unavailable: string | null;
  /** `unavailable` is a failed lookup the control can retry (no `gh`, auth, network). */
  retryable: boolean;
  /** A lookup is in flight. */
  loading: boolean;
  /** The PR list already shows this branch in a stack; gates the pre-lookup placeholder. */
  likely_stacked?: boolean;
}

export interface StackLayerSnapshot {
  branch: string;
  /** `null` for a layer whose PR doesn't exist yet. */
  pr_number: number | null;
  pr_url: string | null;
  /** State without the `current` marker: `open`, `merged`, `needs rebase`. */
  state: string;
  /** True for the branch this tab is viewing — the highlighted row. */
  is_current: boolean;
  /** True when the layer has a PR that can be opened for review. */
  enabled: boolean;
  needs_rebase: boolean;
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
  /** GitHub's merge-box verdict: "CLEAN" | "BEHIND" | "BLOCKED" | "DIRTY" | "UNSTABLE" | "HAS_HOOKS" | "DRAFT" | "UNKNOWN". */
  merge_state_status?: string | null;
  /** Head commit the PR points at now. A merge pins `pr_diff_head_oid` when set, else this. */
  head_oid?: string;
  /** Head branch lives in a fork; branch delete/restore is not offered. */
  is_cross_repository?: boolean;
  /** "MERGE" | "SQUASH" | "REBASE" while auto-merge is pending, else null. */
  auto_merge_method?: string | null;
  /** Looked up only once merged or closed; null while open or unknown. */
  head_branch_exists?: boolean | null;
  repo_merge?: RepoMergeSnapshot | null;
  /** Queued on a merge-queue branch; the PR stays open until the queue merges it. */
  in_merge_queue?: boolean;
  /** The base has a merge queue: merging enqueues and the queue picks the method. */
  base_has_merge_queue?: boolean;
  /** The viewer may merge past unmet requirements (`--admin`). Set only while blocked. */
  can_bypass_rules?: boolean;
  /** When the PR was opened (ISO-8601). `gh pr view` has no timeline, so the
   *  "opened" event is built from this (ADR 0026). */
  created_at?: string | null;
  commit_count?: number;
  /** Requested reviewers first, then everyone else who reviewed; the author is left out. */
  reviewers?: GhReviewerSummary[];
}

export interface RepoMergeSnapshot {
  merge_commit_allowed: boolean;
  squash_merge_allowed: boolean;
  rebase_merge_allowed: boolean;
  delete_branch_on_merge: boolean;
  /** Off by default on GitHub; auto-merge is offered only when on. */
  auto_merge_allowed?: boolean;
  /** "ADMIN" | "MAINTAIN" | "WRITE" | "TRIAGE" | "READ" */
  viewer_permission: string | null;
}

/** Files and +/- for one scope or slice of the diff. `files` is optional only for fixtures. */
export interface ScopeStat {
  files?: number;
  additions: number;
  deletions: number;
}

export interface DiffStaleSnapshot {
  kind: "pr_head" | "base";
  message: string;
  /** The PR head ("pr_head") or local base ref ("base") the on-screen diff was built from. */
  built_from_oid: string;
  /** What origin reports now; a Sync rebuilds against it. */
  latest_oid: string;
  /** Commits `latest_oid` adds. Absent when either commit is not local yet (never fetched for). */
  commits_behind?: number;
}

export interface LocalBranchDivergenceSnapshot {
  /** PR head from the PR-list cache that the worktree is compared to. */
  pr_head_oid: string;
  /** Unpushed commits on the worktree's HEAD. Absent when the PR head is not local yet. */
  ahead?: number;
  /** Absent on a chrome-only snapshot, which does not re-read the worktree. */
  unstaged_files?: number;
}

export interface AppSnapshot {
  /** Retained source context for read-only document requests. */
  preview_context_key?: string;
  mode: "branch" | "unstaged" | "staged" | "history" | "pr" | "conflicts" | "hidden" | "tour";
  /** Optional — populated by the engine when in history mode or branch-mode scope. */
  commits?: CommitSummary[];
  /** SHA of the selected commit when in history mode; null otherwise. */
  selected_commit_sha?: string | null;
  /** File and +/- counts for the unstaged / staged diffs (scope-selector counters). */
  unstaged_stat?: ScopeStat;
  staged_stat?: ScopeStat;
  /** PR detected for the active branch (PR-list cache). Drives the Local|PR Diff toggle. */
  detected_pr_number?: number | null;
  /** Set when the active diff is behind origin. Drives the "stale" pill, banner and Sync. */
  diff_stale?: DiffStaleSnapshot | null;
  /**
   * Head commit the on-screen PR diff was built from. Set only on PR Diff and
   * remote PR views; absent on a local-branch view.
   */
  pr_diff_head_oid?: string | null;
  /**
   * Local PR tab in the local branch view of a live checkout. Sent on every
   * snapshot; a chrome-only one omits `unstaged_files`, which
   * `mergeChromeSnapshot` carries over from the last full build.
   */
  local_branch_divergence?: LocalBranchDivergenceSnapshot | null;
  branch: string;
  base: string;
  input_mode: "normal" | "search" | "comment" | "filter" | "commit" | "confirm";
  files: FileSnapshot[];
  selected_file: number;
  current_hunk: number | null;
  filter: string | null;
  reviewed_count: number;
  total_count: number;
  /** Whole-diff line counts, independent of the file filter. `code` leaves out
   *  tests, Storybook, generated files and docs. Optional only for fixtures. */
  diff_stats?: {
    total: ScopeStat;
    code: ScopeStat;
  };
  ai: AiSnapshot;
  /** Review flow for the active managed diff bucket, when one exists. */
  review_flow?: ReviewFlowSnapshot | null;
  /** Exact active bucket identity used by flow commands, even while idle. */
  review_bucket_er_dir: string;
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
  notification: NotificationSnapshot | null;
  tabs: TabSummary[];
  active_tab: number;
  /** Browser-view annotations for the active tab. */
  ui_annotations?: UiAnnotation[];
  /** Per-tab browser pane (active tab only). */
  browser?: BrowserSnapshot;
  /** Live GitHub status for the active tab (only when it's a remote PR with cached data). */
  github?: GithubStatusSnapshot | null;
  /**
   * `gh stack` stack for the active tab's branch. `null`/absent on tabs whose
   * viewed branch isn't a local checkout (see {@link StackSnapshot}).
   */
  stack?: StackSnapshot | null;
  /** Which background fetches are currently in-flight. */
  bg_loading: LoadingFlags;
  /** Running/done/failed background AI commands for the active tab. */
  agent_commands?: AgentCommandStatus[];
  /** Recent agent log output for the active tab. */
  agent_log?: AgentLogEntry[];
  /** Human-readable label for the currently selected AI provider/model. */
  active_ai_label?: string;
  /** Display name of the active AI Hub provider, if one is selected. */
  active_ai_provider_label?: string | null;
  /** Display name of the active AI Hub model, if one is selected. */
  active_ai_model_label?: string | null;
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
  /** Guided tour for the active tab. `available` drives the Guide tab. */
  tour?: TourSnapshot;
  /** The other view's artifacts, on a local PR tab in its Local branch or PR
   *  diff view; null elsewhere. A chrome-only poll does not read it. */
  other_bucket?: OtherBucketSnapshot | null;
  /** Active arena runs across all tabs (tab-independent background runs). */
  background_arena_runs?: import("./types/arena").ArenaRunSummary[];
  /** Desktop app version from the backend (`CARGO_PKG_VERSION`). */
  app_version?: string;
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
  /** Derived from kind at snapshot time. Missing on older snapshots. */
  category?: string;
}

export interface BackgroundTaskSnapshot {
  id: string;
  kind: string;
  label: string;
  target_label: string;
  scope: string;
  repo_root: string;
  branch_label: string;
  pr_number?: number | null;
  remote_repo?: string | null;
  /** "queued" | "running" | "done" | "failed" */
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
  /** First diff load of a freshly-selected stub tab (background refresh running). */
  tab_diff?: boolean;
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
  /** Explicit levels from the shared catalog; empty means provider default. */
  effort_levels: string[];
  discovered?: boolean;
  args?: string | null;
}

export interface AiProviderInfo {
  id: string;
  label: string;
  models: AiModelInfo[];
  is_selected: boolean;
  family?: string | null;
  has_models_command?: boolean;
  command?: string | null;
  args?: string | null;
  models_command?: string | null;
}

/** Exact provider/model references used by the Review flow. */
export interface ReviewModelRef {
  provider_id: string;
  model_id: string;
}

/** Model groups used by Review triage, standard review, and deep verification. */
export interface ReviewModelGroups {
  fast: ReviewModelRef[];
  standard: ReviewModelRef[];
  advanced: ReviewModelRef[];
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

export type SettingsTab = "general" | "ai" | "review" | "projects" | "terminal";

export interface RefreshAiModelsResponse {
  providers: AiProviderInfo[];
  errors: string[];
}

/** One declared risk rule, for the Settings rule table. */
export interface RiskRuleSnapshot {
  pattern: string;
  /** The canonical tier, or the text as written when it does not read. */
  tier: string;
  /** Whether `tier` reads. An unreadable rule is listed but claims nothing. */
  readable: boolean;
  label: string | null;
  source: RiskRuleSource;
  /** Changed files this rule claims; zero is an idle rule. */
  fileCount: number;
}

/** One changed file resolved against the risk rules. */
export interface RiskFileSnapshot {
  path: string;
  tier: RiskTier;
  /** Null when a default, or nothing, answered. */
  rule: MatchedRiskRule | null;
}

/** One declared importance rule, as written. */
export interface ImportanceRuleSnapshot {
  matcher: string;
  tier: string;
}

/** One changed file resolved against those rules: the tier it reads as, and the
 *  rule that claimed it. `matchedRule` is null when no rule did, which leaves
 *  the default to answer. */
export interface ImportanceFileSnapshot {
  path: string;
  tier: string;
  matchedRule: string | null;
}

export interface DesktopSettingsSnapshot {
  general: ConfigHubField[];
  app: ConfigHubField[];
  terminal: ConfigHubField[];
  agentEffort: string;
  repoRoot: string;
  /** The rules key a project rule edit names, so it lands in the table this tab reads. */
  riskRepo: string;
  /** The active repo's project rules, then the user's. */
  riskRules: RiskRuleSnapshot[];
  /** What a path no rule claims resolves to. */
  riskDefault: RiskTier;
  /** The active tab's changed files, resolved. Empty when nothing is declared. */
  riskFiles: RiskFileSnapshot[];
  /** The pre-merge shape of `riskRules` (project rows only), read by the
   *  current Settings card until it moves to `riskRules`. */
  importanceRules: ImportanceRuleSnapshot[];
  importanceDefault: string;
  /** The active tab's changed files, resolved. Empty when the repo declares no
   *  rules, since then there is nothing to explain. */
  importanceFiles: ImportanceFileSnapshot[];
}

export interface GetConfigHubResponse {
  settings: DesktopSettingsSnapshot;
  providers: AiProviderInfo[];
  /** Models eligible for each Review flow stage. */
  reviewModels: ReviewModelGroups;
  activeEffort: string | null;
  /** Soft validation from last upsert (e.g. missing `{prompt}`). */
  warnings?: string[];
  /** Known family ids from the backend (`claude`, `codex`, …). */
  familyOptions?: string[];
}

export interface FeatureFlagsSnapshot {
  viewBranch: boolean;
  viewUnstaged: boolean;
  viewStaged: boolean;
  viewHistory: boolean;
  viewConflicts: boolean;
  viewHidden: boolean;
  viewTour: boolean;
}

export interface DisplayConfigSnapshot {
  lineNumbers: boolean;
  wrapLines: boolean;
  splitDiff: boolean;
  tabWidth: number;
}
