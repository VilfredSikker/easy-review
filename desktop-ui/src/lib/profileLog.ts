import { devLogEnabled, GROUP_PROFILE, isDevLogInitialized } from "$lib/dev/log";

/**
 * Dev idle profiler — opt-in only.
 * Enable: `ER_DESKTOP_PROFILE_POLL=1 ER_LOG=profile ./scripts/tauri-dev.sh`
 * Or: `localStorage.setItem("erProfilePoll", "1"); location.reload()`
 * Disable override: `localStorage.setItem("erProfilePoll", "0"); location.reload()`
 */

const STORAGE_KEY = "erProfilePoll";

const lastByKind = new Map<string, number>();

function profileExplicitlyEnabled(): boolean {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "0") return false;
    if (stored === "1") return true;
  } catch {
    // ignore
  }
  return import.meta.env.VITE_ER_DESKTOP_PROFILE_POLL === "1";
}

function profileGroupEnabled(): boolean {
  const fromEnv = import.meta.env.VITE_ER_LOG as string | undefined;
  if (fromEnv?.trim()) {
    const parts = fromEnv
      .split(",")
      .map((p) => p.trim().toLowerCase())
      .filter(Boolean);
    if (parts.length === 0 || parts.some((p) => p === "all" || p === "*")) {
      return true;
    }
    return parts.includes(GROUP_PROFILE);
  }
  if (!isDevLogInitialized()) return false;
  return devLogEnabled(GROUP_PROFILE);
}

export function profileEnabled(): boolean {
  if (!import.meta.env.DEV) return false;
  if (!profileExplicitlyEnabled()) return false;
  return profileGroupEnabled();
}

function sinceLastMs(kind: string): number {
  const now = performance.now();
  const prev = lastByKind.get(kind);
  lastByKind.set(kind, now);
  return prev === undefined ? 0 : Math.round(now - prev);
}

export function profileLog(kind: string, fields: Record<string, string | number | boolean>): void {
  if (!profileEnabled()) return;
  const ts_ms = Math.round(performance.timeOrigin + performance.now());
  const payload = {
    kind,
    ts_ms,
    since_last_ms: sinceLastMs(kind),
    ...fields,
  };
  console.info("[er-profile]", payload);
}

/** Rate-limit noisy kinds (e.g. dev_height_fix). */
export function profileLogRateLimited(
  kind: string,
  fields: Record<string, string | number | boolean>,
  maxPerSec = 10,
): void {
  if (!profileEnabled()) return;
  const bucketKey = `__rl_${kind}`;
  const now = Date.now();
  const bucket = (globalThis as unknown as { __erProfileRl?: Map<string, { t: number; n: number }> })
    .__erProfileRl ?? new Map();
  (globalThis as unknown as { __erProfileRl: typeof bucket }).__erProfileRl = bucket;
  let entry = bucket.get(bucketKey);
  if (!entry || now - entry.t >= 1000) {
    entry = { t: now, n: 0 };
  }
  if (entry.n >= maxPerSec) return;
  entry.n += 1;
  bucket.set(bucketKey, entry);
  profileLog(kind, fields);
}
