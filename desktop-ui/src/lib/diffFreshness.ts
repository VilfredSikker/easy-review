/**
 * Diff freshness model for the Branch-panel pill (issue #70 cache-state).
 *
 * One indicator, three states:
 * - "refreshing" — a diff fetch/revalidate is in flight (SWR open, background
 *   refresh, deferred first load, or manual resync). Absorbs the old
 *   standalone "Updating…" pill.
 * - "stale" — the rendered diff's head no longer matches the latest known
 *   head, OR the last successful sync is unknown / older than the threshold.
 * - "fresh" — the diff matches the latest known head and was confirmed
 *   recently.
 */

export type DiffFreshState = "fresh" | "refreshing" | "stale";

/** Last-sync age beyond which an otherwise-unconfirmed diff reads as stale. */
export const DIFF_STALE_AFTER_MS = 10 * 60 * 1000;

export interface DiffFreshInput {
  /** Any diff fetch in flight (`bg_loading.remote_pr_diff || bg_loading.tab_diff`). */
  refreshing: boolean;
  /** Backend-computed head mismatch (`snapshot.diff_outdated`). */
  outdated: boolean;
  /** `snapshot.diff_synced_at_epoch_ms` (null = never confirmed). */
  syncedAtMs: number | null;
  /** Current wall-clock time in epoch ms. */
  nowMs: number;
}

export function diffFreshState(input: DiffFreshInput): DiffFreshState {
  if (input.refreshing) return "refreshing";
  if (input.outdated) return "stale";
  if (input.syncedAtMs == null) return "stale";
  if (input.nowMs - input.syncedAtMs > DIFF_STALE_AFTER_MS) return "stale";
  return "fresh";
}

/** Compact relative time for the pill: "just now", "3m ago", "2h ago", "5d ago". */
export function relativeSyncTime(syncedAtMs: number | null, nowMs: number): string | null {
  if (syncedAtMs == null) return null;
  const ageMs = Math.max(0, nowMs - syncedAtMs);
  if (ageMs < 60_000) return "just now";
  const mins = Math.floor(ageMs / 60_000);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

/** First 7 chars of a SHA, or null when unknown/empty. */
export function shortOid(oid: string | null | undefined): string | null {
  const trimmed = (oid ?? "").trim();
  return trimmed.length > 0 ? trimmed.slice(0, 7) : null;
}

/** Multi-line tooltip spelling out the exact freshness state. */
export function diffFreshTooltip(opts: {
  state: DiffFreshState;
  headOid: string | null | undefined;
  latestOid: string | null | undefined;
  syncedAtMs: number | null;
  nowMs: number;
}): string {
  const lines: string[] = [];
  if (opts.state === "refreshing") {
    lines.push("Refreshing diff in the background");
  } else if (opts.state === "stale") {
    lines.push("Diff may be outdated");
  } else {
    lines.push("Diff is up to date");
  }
  const head = shortOid(opts.headOid);
  const latest = shortOid(opts.latestOid);
  if (head) lines.push(`Rendered head: ${head}`);
  if (latest && latest !== head) lines.push(`Latest head: ${latest}`);
  const rel = relativeSyncTime(opts.syncedAtMs, opts.nowMs);
  lines.push(rel ? `Last synced ${rel}` : "Last sync not confirmed yet");
  return lines.join("\n");
}
