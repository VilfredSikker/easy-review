import type { AppSnapshot } from "./types";

/** Fields the backend uses to refuse an optimistic write on a switched view. */
export type SnapshotViewParts = {
  active_tab: number;
  repo_root: string;
  pr_number: number | null;
  branch: string;
  mode: string;
};

/**
 * Identity of the review view a snapshot belongs to. Chrome-only polls must not
 * keep `prev.ai` across a change here — that leaves Branch/Review/Notes badges
 * stuck on the previous PR while the card shows the new one (or vice versa).
 */
export function snapshotViewParts(snap: AppSnapshot): SnapshotViewParts {
  const tab =
    snap.tabs?.find((t) => t.is_active) ??
    (typeof snap.active_tab === "number" ? snap.tabs?.[snap.active_tab] : undefined);
  // Tab fields only. `github.number` / `detected_pr_number` arriving later
  // must not look like a view change (that deferred chrome and stuck status).
  // Same for `snapshot.branch`/`base` filling in on a remote stub after gh
  // status lands — that is chrome, not a different review view.
  return {
    active_tab: snap.active_tab,
    repo_root: tab?.repo_root ?? "",
    pr_number: tab?.pr_number ?? null,
    branch: tab?.branch ?? "",
    mode: snap.mode ?? "",
  };
}

export function snapshotViewIdentity(snap: AppSnapshot): string {
  const p = snapshotViewParts(snap);
  return `${p.active_tab}|${p.repo_root}|${p.pr_number ?? ""}|${p.branch}|${p.mode}`;
}

export type ChromeMergeAiSource = "prev" | "next";

/**
 * Patch chrome/sidebar fields from a poll snapshot while keeping diff hunks/spans.
 * `aiSource: "prev"` is the normal chrome-only path (same view).
 * `aiSource: "next"` is required when the view identity changed — never keep the
 * previous PR's AI/pr onto the new chrome (chrome stubs carry empty AI).
 */
export function mergeChromeSnapshot(
  prev: AppSnapshot,
  next: AppSnapshot,
  aiSource: ChromeMergeAiSource = "prev",
): AppSnapshot {
  return {
    ...next,
    mode: prev.mode,
    branch: prev.branch.trim() ? prev.branch : next.branch,
    base: prev.base.trim() ? prev.base : next.base,
    input_mode: prev.input_mode,
    files: prev.files,
    selected_file: prev.selected_file,
    current_hunk: prev.current_hunk,
    filter: prev.filter,
    reviewed_count: prev.reviewed_count,
    total_count: prev.total_count,
    ai: aiSource === "next" ? next.ai : prev.ai,
    pr: aiSource === "next" ? next.pr : prev.pr,
    ui_annotations: aiSource === "next" ? next.ui_annotations : prev.ui_annotations,
    browser: aiSource === "next" ? next.browser : prev.browser,
    filter_suggestions: prev.filter_suggestions,
    commits: prev.commits,
    selected_commit_sha: prev.selected_commit_sha,
  };
}

/** True when a chrome-style poll may merge onto `prev` (same view identity). */
export function canChromeMerge(
  prev: AppSnapshot | null,
  next: AppSnapshot,
  opts: { chromeOnly: boolean; contentChanged: boolean },
): boolean {
  if (prev === null) return false;
  if (!(opts.chromeOnly || !opts.contentChanged)) return false;
  return snapshotViewIdentity(prev) === snapshotViewIdentity(next);
}

/**
 * Chrome-style poll whose view identity differs. Never merge files across
 * views (that poisons the tab cache with the other tab's diff).
 */
export function canChromeMergeTakingNextAi(
  prev: AppSnapshot | null,
  next: AppSnapshot,
  opts: { chromeOnly: boolean; contentChanged: boolean },
): boolean {
  if (prev === null) return false;
  if (!(opts.chromeOnly || !opts.contentChanged)) return false;
  return snapshotViewIdentity(prev) !== snapshotViewIdentity(next);
}

/**
 * Chrome-only poll for a *different* view (including Branch vs PR Diff):
 * skip applying it. Chrome stubs carry empty AI/files; merging them would
 * keep the previous view's diff. Wait for the full content snapshot.
 */
export function shouldDeferChromeIdentityChange(
  prev: AppSnapshot | null,
  next: AppSnapshot,
  opts: { chromeOnly: boolean; contentChanged: boolean },
): boolean {
  return opts.chromeOnly && canChromeMergeTakingNextAi(prev, next, opts);
}

/** Pure helper so poll generation discard is unit-testable. */
export function isStaleSnapshotGeneration(
  captured: number,
  current: number,
): boolean {
  return captured !== current;
}
