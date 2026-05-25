/** Dev idle profiler — enable: localStorage.setItem("erProfilePoll", "1"); location.reload() */

const STORAGE_KEY = "erProfilePoll";

const lastByKind = new Map<string, number>();

export function profileEnabled(): boolean {
  if (!import.meta.env.DEV) return false;
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
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
  console.info("[er-profile]", {
    kind,
    ts_ms,
    since_last_ms: sinceLastMs(kind),
    ...fields,
  });
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
