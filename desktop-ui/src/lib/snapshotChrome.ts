import type { AppSnapshot } from "./types";

/**
 * Identity of the review view a snapshot belongs to. Chrome-only polls must not
 * keep `prev.ai` across a change here — that leaves Branch/Review/Notes badges
 * stuck on the previous PR while the card shows the new one (or vice versa).
 */
export function snapshotViewIdentity(snap: AppSnapshot): string {
  const tab =
    snap.tabs?.find((t) => t.is_active) ??
    (typeof snap.active_tab === "number" ? snap.tabs?.[snap.active_tab] : undefined);
  const pr =
    snap.pr?.number ?? tab?.pr_number ?? snap.github?.number ?? null;
  const root = tab?.repo_root ?? "";
  const branch = snap.branch ?? tab?.branch ?? "";
  const mode = snap.mode ?? "";
  return `${snap.active_tab}|${root}|${pr ?? ""}|${branch}|${mode}`;
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
    branch: prev.branch,
    base: prev.base,
    input_mode: prev.input_mode,
    files: prev.files,
    selected_file: prev.selected_file,
    current_hunk: prev.current_hunk,
    filter: prev.filter,
    reviewed_count: prev.reviewed_count,
    total_count: prev.total_count,
    ai: aiSource === "next" ? next.ai : prev.ai,
    pr: aiSource === "next" ? next.pr : prev.pr,
    ui_annotations: prev.ui_annotations,
    browser: prev.browser,
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
 * Chrome-style poll (chrome_only or content unchanged) for a *different* view:
 * merge chrome but take `next.ai` / `next.pr` so badges cannot stick on the old PR.
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

/** Pure helper so poll generation discard is unit-testable. */
export function isStaleSnapshotGeneration(
  captured: number,
  current: number,
): boolean {
  return captured !== current;
}
