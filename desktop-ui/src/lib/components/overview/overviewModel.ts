import type { AppSnapshot, CheckSummary, GithubStatusSnapshot } from "$lib/types";
import { resolveContextIdentity, resolveContextTitle } from "$lib/contextIdentity";
import { githubStatusForActiveTab } from "$lib/prUrl";
import { mergeBoxModel, type MergeBoxTone } from "$lib/prMergeBox";
import { filesByPillar } from "$lib/pillarFiles";
import { baseName } from "$lib/shortFileName";
import { reviewFlowControls } from "$lib/reviewFlow";

// ── Header ──────────────────────────────────────────────────────────────────

export interface OverviewHeader {
  title: string;
  prNumber: number | null;
  /** GitHub login of the PR's author; null off a PR. */
  author: string | null;
  base: string;
  head: string;
  /** Null when neither GitHub nor the local log has counted them. */
  commits: number | null;
  /** When the PR was opened (ISO); null off a PR or before GitHub answers. */
  openedAt: string | null;
}

/**
 * The page title and its meta line. A local branch has no PR title, so it
 * reads the branch the same way the top bar does, and leaves out what only a
 * PR has (author, opening time) rather than inventing it.
 */
export function overviewHeader(snap: AppSnapshot | null): OverviewHeader {
  const { title, prNumber } = resolveContextTitle(snap);
  const { branch, base } = resolveContextIdentity(snap);
  const github = snap ? githubStatusForActiveTab(snap) : null;
  const pr = snap?.pr && (prNumber == null || snap.pr.number === prNumber) ? snap.pr : null;
  return {
    title,
    prNumber,
    author: github?.author || pr?.author || null,
    base,
    head: branch,
    commits: github?.commit_count ?? snap?.commits?.length ?? null,
    openedAt: github?.created_at ?? null,
  };
}

// ── Status ──────────────────────────────────────────────────────────────────

export interface PrStatus {
  label: string;
  tone: MergeBoxTone;
}

function lowerFirst(s: string): string {
  return s ? s[0].toLowerCase() + s.slice(1) : s;
}

/**
 * "Open · ready to merge": the PR's state, then the merge box's verdict. Both
 * come from the merge box's own model, so the aside and the box under it never
 * disagree about whether the PR can merge.
 */
export function prStatus(github: GithubStatusSnapshot): PrStatus {
  const model = mergeBoxModel(github);
  if (model.phase === "merged") return { label: "Merged", tone: "merged" };
  if (model.phase === "closed") return { label: "Closed", tone: "muted" };
  if (github.is_draft) return { label: "Draft", tone: "muted" };
  const verdict = model.headline ? ` · ${lowerFirst(model.headline)}` : "";
  return { label: `Open${verdict}`, tone: model.tone };
}

// ── Checks ──────────────────────────────────────────────────────────────────

export type CheckState = "ok" | "pend" | "fail" | "neutral";

export interface CheckRow {
  name: string;
  state: CheckState;
  /** Duration of a finished check, its status text, or "pending". */
  detail: string;
  url: string | null;
}

/** "41s", "1m 52s", "1h 3m": the precision a CI duration is read at. */
export function formatDuration(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s}s`;
  if (s < 3600) {
    const m = Math.floor(s / 60);
    return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  }
  const h = Math.floor(s / 3600);
  return `${h}h ${Math.floor((s % 3600) / 60)}m`;
}

// `gh pr checks` spells conclusions either way, as countChecks notes.
function checkState(c: CheckSummary): CheckState {
  if (c.status === "PENDING") return "pend";
  if (c.conclusion === "SUCCESS" || c.conclusion === "pass") return "ok";
  if (c.conclusion === "FAILURE" || c.conclusion === "fail") return "fail";
  return "neutral";
}

// A deploy preview has no duration, only status text such as "Ready".
function checkDetail(c: CheckSummary, state: CheckState): string {
  if (state === "pend") return "pending";
  if (c.duration_secs != null) return formatDuration(c.duration_secs);
  return c.description?.trim() ?? "";
}

/** Failing checks first, then running ones: what blocks the merge reads first. */
export function checkRows(checks: readonly CheckSummary[]): CheckRow[] {
  const ORDER: Record<CheckState, number> = { fail: 0, pend: 1, ok: 2, neutral: 3 };
  return checks
    .map((c) => {
      const state = checkState(c);
      return { name: c.name, state, detail: checkDetail(c, state), url: c.url };
    })
    .sort((a, b) => ORDER[a.state] - ORDER[b.state]);
}

// ── Quick actions ───────────────────────────────────────────────────────────

export type Generator = "triage" | "review" | "tour";

export interface GeneratorState {
  id: Generator;
  /** When this view's artifact was made; null when it has none. */
  at: string | null;
  busy: boolean;
  /** A Deep review flow is waiting for the reviewer to confirm its estimate. */
  confirm?: boolean;
}

type GeneratorSnapshot = Pick<
  AppSnapshot,
  "ai" | "tour" | "background_tasks" | "tabs" | "local_branch" | "branch" | "review_flow"
>;

// A review fans out into expert finders, each its own task.
function taskMatches(kind: string, id: Generator): boolean {
  if (id === "review") return kind === "review" || kind.startsWith("expert:");
  return kind === id;
}

/**
 * Triage, AI review and Tour as the quick actions show them: the age of what
 * this view already has, and whether a run for this branch is in flight. A run
 * for another tab does not spin this page's button.
 */
// The review flow is the review ⌘K → a → r runs; a Deep estimate waiting on
// confirmation is not busy, it is the button's next click.
function reviewGenerator(snap: GeneratorSnapshot | null, taskBusy: boolean): GeneratorState {
  const flow = reviewFlowControls(snap?.review_flow);
  return {
    id: "review",
    at: snap?.ai?.review_created_at ?? null,
    busy: taskBusy || (flow.busy && !flow.confirm),
    confirm: flow.confirm,
  };
}

export function generatorStates(snap: GeneratorSnapshot | null): GeneratorState[] {
  const tab = snap?.tabs.find((t) => t.is_active);
  const branch = snap?.local_branch ?? snap?.branch;
  const live = (snap?.background_tasks ?? []).filter(
    (t) =>
      (t.status === "queued" || t.status === "running") &&
      t.repo_root === tab?.repo_root &&
      t.branch_label === branch,
  );
  const busy = (id: Generator) => live.some((t) => taskMatches(t.kind, id));
  return [
    { id: "triage", at: snap?.ai?.triage?.created_at ?? null, busy: busy("triage") },
    reviewGenerator(snap, busy("review")),
    { id: "tour", at: snap?.tour?.available ? (snap.tour.createdAt ?? null) : null, busy: busy("tour") },
  ];
}

/**
 * The command the Overview's AI review button sends: the same review flow
 * (triage, finders, verification) as the palette's Review entry, not the
 * single general review.
 */
export function reviewFlowCommand(
  snap: Pick<AppSnapshot, "review_flow" | "review_bucket_er_dir"> | null,
  scope: string | null,
): { command: "start_review_flow" | "confirm_review_flow"; args: Record<string, unknown> } | null {
  const erDir = snap?.review_bucket_er_dir;
  if (!scope || !erDir) return null;
  const controls = reviewFlowControls(snap?.review_flow);
  if (controls.confirm) return { command: "confirm_review_flow", args: { erDir } };
  if (controls.busy) return null;
  return { command: "start_review_flow", args: { scope, erDir } };
}

/** The newest arena run and how many there are, for the Arena button's caption. */
export function latestArenaRun(
  snap: Pick<AppSnapshot, "arena_runs"> | null,
): { count: number; at: string } | null {
  const runs = snap?.arena_runs ?? [];
  if (runs.length === 0) return null;
  const at = runs.map((r) => r.created_at).sort().at(-1) ?? "";
  return { count: runs.length, at };
}

// ── Files by pillar ─────────────────────────────────────────────────────────

export interface AsideFile {
  path: string;
  /** The engine's index, which `select_file` takes. */
  sourceIndex: number;
  name: string;
  /** Short language tag from the extension ("TS", "SQL"); empty without one. */
  lang: string;
  reviewed: boolean;
  renamed: boolean;
}

export interface AsideFileGroup {
  /** Pillar title; null when the diff has no tour to group by. */
  title: string | null;
  additions: number;
  deletions: number;
  files: AsideFile[];
}

export function langTag(path: string): string {
  const name = baseName(path);
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "";
  return name.slice(dot + 1).slice(0, 4).toUpperCase();
}

/**
 * The aside's file list, grouped the way the Tour walks it. Without a tour the
 * diff is one untitled group: grouping by folder would be a second structure
 * the reader has to learn for one page.
 */
export function asideFileGroups(snap: Pick<AppSnapshot, "files" | "tour"> | null): AsideFileGroup[] {
  const files = snap?.files ?? [];
  if (files.length === 0) return [];
  const pillars = snap?.tour?.available ? snap.tour.pillars : [];
  const groups =
    pillars.length > 0
      ? filesByPillar(pillars, files).map((g) => ({ title: g.title as string | null, files: g.files }))
      : [{ title: null, files }];
  return groups
    .filter((g) => g.files.length > 0)
    .map((g) => ({
      title: g.title,
      additions: g.files.reduce((n, f) => n + f.additions, 0),
      deletions: g.files.reduce((n, f) => n + f.deletions, 0),
      files: g.files.map((f) => ({
        path: f.path,
        sourceIndex: f.source_index,
        name: baseName(f.path),
        lang: langTag(f.path),
        reviewed: f.reviewed,
        renamed: f.status === "renamed",
      })),
    }));
}
