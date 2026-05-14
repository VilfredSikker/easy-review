/**
 * Token-level diff for highlighting intra-line changes between a paired
 * deletion + addition. LCS-based — naive O(n*m) DP, fine for short lines.
 *
 * Tokens are runs of whitespace or runs of non-whitespace. This keeps the
 * implementation simple while producing readable highlights for typical
 * code edits (variable renames, literal changes, signature tweaks).
 */

export interface Span {
  text: string;
  changed: boolean;
}

export interface WordDiffResult {
  old: Span[];
  new: Span[];
}

const MAX_TOKENS = 200;
const CACHE_LIMIT = 1000;
const cache = new Map<string, WordDiffResult>();

/** Split into runs of whitespace / non-whitespace. Empty input → []. */
function tokenize(s: string): string[] {
  if (!s) return [];
  const out: string[] = [];
  const re = /\s+|\S+/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(s)) !== null) out.push(m[0]);
  return out;
}

/** Collapse adjacent same-`changed` spans for cleaner DOM output. */
function coalesce(spans: Span[]): Span[] {
  if (spans.length <= 1) return spans;
  const out: Span[] = [];
  for (const span of spans) {
    const last = out[out.length - 1];
    if (last && last.changed === span.changed) {
      last.text += span.text;
    } else {
      out.push({ ...span });
    }
  }
  return out;
}

/** Build the LCS DP table for tokens. */
function lcsTable(a: string[], b: string[]): Uint16Array {
  const n = a.length;
  const m = b.length;
  // (n+1) * (m+1) table, row-major.
  const dp = new Uint16Array((n + 1) * (m + 1));
  const w = m + 1;
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      if (a[i] === b[j]) {
        dp[i * w + j] = dp[(i + 1) * w + (j + 1)] + 1;
      } else {
        const down = dp[(i + 1) * w + j];
        const right = dp[i * w + (j + 1)];
        dp[i * w + j] = down >= right ? down : right;
      }
    }
  }
  return dp;
}

/** Walk the DP table to emit spans for both sides. */
function walk(a: string[], b: string[], dp: Uint16Array): WordDiffResult {
  const n = a.length;
  const m = b.length;
  const w = m + 1;
  const oldSpans: Span[] = [];
  const newSpans: Span[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      oldSpans.push({ text: a[i], changed: false });
      newSpans.push({ text: b[j], changed: false });
      i++;
      j++;
    } else if (dp[(i + 1) * w + j] >= dp[i * w + (j + 1)]) {
      oldSpans.push({ text: a[i], changed: true });
      i++;
    } else {
      newSpans.push({ text: b[j], changed: true });
      j++;
    }
  }
  while (i < n) {
    oldSpans.push({ text: a[i], changed: true });
    i++;
  }
  while (j < m) {
    newSpans.push({ text: b[j], changed: true });
    j++;
  }
  return { old: coalesce(oldSpans), new: coalesce(newSpans) };
}

export function wordDiff(oldText: string, newText: string): WordDiffResult {
  const key = oldText + "\0" + newText;
  const hit = cache.get(key);
  if (hit) return hit;

  let result: WordDiffResult;
  if (oldText === newText) {
    result = {
      old: oldText ? [{ text: oldText, changed: false }] : [],
      new: newText ? [{ text: newText, changed: false }] : [],
    };
  } else if (!oldText) {
    result = { old: [], new: [{ text: newText, changed: true }] };
  } else if (!newText) {
    result = { old: [{ text: oldText, changed: true }], new: [] };
  } else {
    const a = tokenize(oldText);
    const b = tokenize(newText);
    if (a.length > MAX_TOKENS || b.length > MAX_TOKENS) {
      // Bail out for pathological lines — render as a single unchanged span.
      result = {
        old: [{ text: oldText, changed: false }],
        new: [{ text: newText, changed: false }],
      };
    } else {
      const dp = lcsTable(a, b);
      result = walk(a, b, dp);
    }
  }

  if (cache.size >= CACHE_LIMIT) {
    // Naive eviction: drop the oldest insertion (Map preserves insertion order).
    const firstKey = cache.keys().next().value;
    if (firstKey !== undefined) cache.delete(firstKey);
  }
  cache.set(key, result);
  return result;
}

/** Test helper — clear the memoization cache. */
export function _clearWordDiffCache(): void {
  cache.clear();
}
