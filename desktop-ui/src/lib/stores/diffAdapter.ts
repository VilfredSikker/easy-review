import type { Command } from "$lib/protocol/generated/Command";
import type { FileRow } from "$lib/protocol/generated/FileRow";
import type { PrRow } from "$lib/protocol/generated/PrRow";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabDiff } from "$lib/protocol/generated/TabDiff";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { BaseBehind } from "$lib/protocol/generated/BaseBehind";
import type { Hunk } from "$lib/protocol/generated/Hunk";
import type { AppSnapshot, DiffStaleSnapshot, FileSnapshot, ThreadSnapshot } from "$lib/types";
import { joinedHunks, threadsOn } from "$lib/threadJoin";
import type { ResourceStore } from "./resourceStore";

/** The snapshot fields `diff(tab)` replaces with the flag on. */
export type DiffSlice = Pick<
  AppSnapshot,
  | "files"
  | "mode"
  | "branch"
  | "base"
  | "total_count"
  | "diff_stats"
  | "filter"
  | "filter_suggestions"
  | "preview_context_key"
  | "review_bucket_er_dir"
  | "local_branch"
  | "local_branch_checked_out"
  | "unstaged_stat"
  | "staged_stat"
  | "diff_stale"
  | "pr_diff_head_oid"
  | "local_branch_divergence"
>;

/**
 * The views the workspace records but does not load. The App still shows
 * them, so the adapter reads their slice from the snapshot.
 */
export const RECORDED_MODES: ReadonlySet<string> = new Set(["history", "conflicts", "hidden"]);

/** The commands those views leave to the App, whose base and filter they read. */
const APP_IN_RECORDED: ReadonlySet<string> = new Set(["change_base", "set_filter", "clear_filter", "toggle_compacted"]);

export function diffKey(tab: TabKey): ResourceKey {
  return { kind: "diff", tab };
}

const EMPTY: DiffSlice = {
  files: [],
  mode: "branch",
  branch: "",
  base: "",
  total_count: 0,
  diff_stats: { total: { files: 0, additions: 0, deletions: 0 }, code: { files: 0, additions: 0, deletions: 0 } },
  filter: null,
  filter_suggestions: [],
  preview_context_key: "",
  review_bucket_er_dir: "",
  local_branch: null,
  local_branch_checked_out: false,
  unstaged_stat: { files: 0, additions: 0, deletions: 0 },
  staged_stat: { files: 0, additions: 0, deletions: 0 },
  diff_stale: undefined,
  pr_diff_head_oid: null,
  local_branch_divergence: undefined,
};

export function sliceFromSnapshot(snapshot: DiffSlice | null): DiffSlice {
  return snapshot ?? EMPTY;
}

/** A file row as the list shows it, before its hunks are fetched by hash. */
export function fileFromRow(row: FileRow): FileSnapshot {
  return { ...row, status: row.status as FileSnapshot["status"], hunks: [] } as FileSnapshot;
}

/** Where a file's hunks and threads come from with the flag on. */
export interface HunkSource {
  /** The hunks the page holds under a hash. */
  get: (hash: string) => Hunk[] | undefined;
  /** The tab's threads, which the hunks are joined with by path and line. */
  threads: () => readonly ThreadSnapshot[];
}

/**
 * The App's snapshot with `slice` in place of its fields. Each file draws
 * the hunks its hash names, with the threads that sit on them; a file whose
 * hunks the page does not hold yet is a stub, which the view fetches.
 */
export function overlaySnapshot(raw: AppSnapshot, slice: DiffSlice, hunks?: HunkSource): AppSnapshot {
  const threads = hunks?.threads() ?? [];
  const files = slice.files.map((row) => {
    if (row.compacted) return row;
    const held = hunks?.get(row.cache_key);
    if (!held) return { ...row, is_lazy_stub: row.additions + row.deletions > 0 };
    return { ...row, is_lazy_stub: false, hunks: joinedHunks(held, threadsOn(threads, row.path)) };
  });
  return { ...raw, ...slice, files };
}

/** The PR stale mark: the diff was built from one head while the PR list names another. */
export function prStale(builtFrom: string | null, latest: string | null): DiffStaleSnapshot | undefined {
  if (!builtFrom || !latest || builtFrom === latest) return undefined;
  return {
    kind: "pr_head",
    message: "PR head updated on origin — Sync to refresh",
    built_from_oid: builtFrom,
    latest_oid: latest,
  };
}

/** A branch view's stale mark: origin's tip of its base moved past the commit the diff was built from. */
export function baseStale(base: string, behind: BaseBehind | null): DiffStaleSnapshot | undefined {
  if (!behind) return undefined;
  const name = base.replace(/^origin\//, "");
  return {
    kind: "base",
    message: `${name} has new commits on origin — Sync to refresh`,
    built_from_oid: behind.built_from_oid,
    latest_oid: behind.latest_oid,
    ...(behind.commits_behind === null ? {} : { commits_behind: behind.commits_behind }),
  };
}

/** The PR list's row for a PR, from the `prs(remote)` resource the page holds. */
export function prListRow(
  store: Pick<ResourceStore, "get">,
  remote: string | null,
  number: number | null,
): PrRow | null {
  if (!remote || number === null) return null;
  const held = store.get({ kind: "prs", remote })?.value;
  if (held?.kind !== "prs") return null;
  return held.rows.find((row) => row.number === number) ?? null;
}

/**
 * Whether a tab's diff is behind. A PR diff compares the head it was built
 * from with its `prs(remote)` row. A branch view's base mark is the active
 * tab's only, from its `diff(tab)`'s `base_behind`.
 */
export function tabIsStale(
  store: Pick<ResourceStore, "get">,
  tab: { key: TabKey; pr_number: number | null; remote: string | null },
  isActive: boolean,
): boolean {
  const diff = store.get(diffKey(tab.key))?.value;
  const builtFrom = diff?.kind === "diff" ? diff.built_from_head_oid : null;
  if (!builtFrom) return isActive && diff?.kind === "diff" && baseStale(diff.base, diff.base_behind) !== undefined;
  const latest = prListRow(store, tab.remote, tab.pr_number)?.headOid || null;
  return prStale(builtFrom, latest) !== undefined;
}

/** The PR list's head and refs for the active tab's PR, when the list holds it. */
export interface PrHead {
  head_oid: string;
  head_ref: string;
  base_ref: string;
}

/** `diff(tab)` as the slice the page reads. */
export function sliceFromResource(diff: TabDiff, pr: PrHead | null): DiffSlice {
  const prStaleness = prStale(diff.built_from_head_oid, pr?.head_oid || null);
  const divergence = diff.local_branch_divergence;
  return {
    files: diff.files.map(fileFromRow),
    mode: diff.mode as DiffSlice["mode"],
    branch: diff.branch || pr?.head_ref || "",
    base: diff.base || pr?.base_ref || "",
    total_count: diff.total_count,
    diff_stats: diff.diff_stats,
    filter: diff.filter,
    filter_suggestions: diff.filter_suggestions as DiffSlice["filter_suggestions"],
    preview_context_key: diff.preview_context_key,
    review_bucket_er_dir: diff.review_bucket_er_dir,
    local_branch: diff.local_branch,
    local_branch_checked_out: diff.local_branch_checked_out,
    unstaged_stat: diff.unstaged_stat,
    staged_stat: diff.staged_stat,
    diff_stale: diff.built_from_head_oid ? prStaleness : baseStale(diff.base, diff.base_behind),
    pr_diff_head_oid: diff.pr_diff_head_oid,
    local_branch_divergence: divergence && {
      pr_head_oid: divergence.pr_head_oid,
      ...(divergence.ahead === null ? {} : { ahead: divergence.ahead }),
      unstaged_files: divergence.unstaged_files,
    },
  };
}

type Args = Record<string, unknown>;

/**
 * The wire command for one of the diff commands `app.cmd` was asked for, on
 * `tab`; undefined for any other command and for a switch to a view the App
 * still loads, which keeps its old route.
 */
export function wireDiffCommand(command: string, a: Args, tab: TabKey): Command | undefined {
  switch (command) {
    case "set_mode": {
      const mode = String(a.mode);
      if (RECORDED_MODES.has(mode)) return undefined;
      const prNumber = typeof a.prNumber === "number" ? a.prNumber : null;
      return { kind: "set_diff_mode", tab, mode, prNumber };
    }
    case "change_base":
      return { kind: "change_base", tab, base: String(a.branch) };
    case "set_filter":
      return { kind: "set_filter", tab, query: String(a.query ?? "") };
    case "clear_filter":
      return { kind: "set_filter", tab, query: "" };
    case "refresh_diff":
      return { kind: "refresh_diff", tab, sync: false };
    case "force_refresh_diff":
      return { kind: "refresh_diff", tab, sync: true };
    case "sync_pr":
      return { kind: "sync_pr", projectId: String(a.projectId), prNumber: Number(a.prNumber) };
    case "sync_branch":
      return { kind: "sync_branch", projectId: String(a.projectId), branch: String(a.branch) };
    case "toggle_compacted":
      // The App's toggle acts on its selected file; the wire names the file and the direction.
      if (typeof a.path !== "string") return undefined;
      return { kind: "set_file_compacted", tab, path: a.path, compacted: Boolean(a.compacted) };
    default:
      return undefined;
  }
}

interface DiffAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "send" | "whenAt">;
  snapshot: () => DiffSlice | null;
  /** The registry's active tab, whose diff the page shows. */
  activeKey: () => TabKey | null;
  /** The wire key of the strip's row at `idx`. */
  keyAt: (idx: number) => TabKey | null;
  /** The PR list's row for the active tab's PR. */
  prHead: () => PrHead | null;
  /** The hunks by hash and the threads they are joined with. */
  hunks?: HunkSource;
}

/**
 * The active tab's file list, mode, base and staleness: from `app.snapshot`
 * with the flag off, and from `diff(tab)` with it on. In the views the
 * workspace only records (History, Conflicts, Hidden) the flag-on read takes
 * the snapshot's slice too, since the App is what loads them.
 */
export class DiffAdapter {
  constructor(private readonly deps: DiffAdapterDeps) {}

  read(): DiffSlice {
    const snapshot = this.deps.snapshot();
    const held = this.held();
    if (!held || RECORDED_MODES.has(held.mode)) return sliceFromSnapshot(snapshot);
    return sliceFromResource(held, this.deps.prHead());
  }

  /** `raw` with this adapter's fields in place of the App's; the views the App loads keep its snapshot. */
  overlay(raw: AppSnapshot): AppSnapshot {
    const held = this.held();
    if (!held || RECORDED_MODES.has(held.mode)) return raw;
    return overlaySnapshot(raw, sliceFromResource(held, this.deps.prHead()), this.deps.hunks);
  }

  /** The active tab's diff as the resource holds it; null with the flag off or before the first fetch. */
  held(): TabDiff | null {
    if (!this.deps.flagOn) return null;
    const key = this.deps.activeKey();
    if (key === null) return null;
    const value = this.deps.store.get(diffKey(key))?.value;
    return value?.kind === "diff" ? value : null;
  }

  /** Whether the page draws the active tab's hunks by hash: the flag is on and the view is one the workspace loads. */
  hashed(): boolean {
    const held = this.held();
    return held !== null && !RECORDED_MODES.has(held.mode);
  }

  /** Why the active tab's last load failed; null while it loaded or loads. */
  get loadError(): string | null {
    return this.held()?.load_error ?? null;
  }

  get files(): FileSnapshot[] {
    return this.read().files;
  }

  /** Sends a wire command and resolves once every key its `Ack` names is held at that version. */
  async send(command: Command): Promise<void> {
    let ack;
    try {
      ack = await this.deps.store.send(command);
    } catch (e) {
      throw new Error((e as { message?: string }).message ?? String(e), { cause: e });
    }
    await Promise.all(ack.changed.map(({ key, version }) => this.deps.store.whenAt(key, version)));
  }

  /**
   * With the flag on, a diff command goes through `er_send` on the active
   * tab and resolves once every key its `Ack` names is held; null for any
   * other command, for a switch to a view the App loads or a base or filter
   * change while one shows, and for every command with the flag off. A click painted on a tab that is no longer active does nothing.
   */
  route(command: string, args: Args = {}): Promise<void> | null {
    if (!this.deps.flagOn) return null;
    const active = this.deps.activeKey();
    if (active === null) return null;
    const wire = wireDiffCommand(command, args, active);
    if (wire === undefined) return null;
    if (APP_IN_RECORDED.has(command) && RECORDED_MODES.has(this.held()?.mode ?? "")) return null;
    if (typeof args.tabIdx === "number" && this.deps.keyAt(args.tabIdx) !== active) return Promise.resolve();
    return this.send(wire);
  }
}
