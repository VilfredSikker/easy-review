// Pure helpers used by the browser-view store. Kept out of the `.svelte.ts`
// runtime so they are importable from Bun-driven unit tests.

import { invoke } from "@tauri-apps/api/core";

export const DEFAULT_DEV_URL = "http://localhost:5173";

export function toProxyUrl(url: string): string {
  if (!url) return "";
  if (url.startsWith("erp://") || url.startsWith("erps://")) return url;
  if (url.startsWith("data:")) return url;
  try {
    const u = new URL(/^https?:\/\//i.test(url) ? url : `http://${url}`);
    if (u.protocol === "https:") {
      return `erps://${u.host}${u.pathname}${u.search}${u.hash}`;
    }
    if (u.protocol === "http:") {
      return `erp://${u.host}${u.pathname}${u.search}${u.hash}`;
    }
    return url;
  } catch {
    return url;
  }
}

export function fromProxyUrl(url: string): string {
  if (url.startsWith("erps://")) return `https://${url.slice("erps://".length)}`;
  if (url.startsWith("erp://")) return `http://${url.slice("erp://".length)}`;
  return url;
}

/**
 * Best-effort default dev-server URL. Asks the backend to inspect the project's
 * `package.json` (scripts.dev / scripts.start) and infer the dev port. Returns
 * the Vite default when the backend yields no answer, or when `invoke` is
 * unavailable (e.g. unit tests, web preview).
 */
export async function defaultDevUrl(repoRoot?: string): Promise<string> {
  if (!repoRoot) return DEFAULT_DEV_URL;
  try {
    const detected = await invoke<string | null>("detect_dev_url", { repoRoot });
    return detected ?? DEFAULT_DEV_URL;
  } catch {
    // Backend not reachable (test/storybook environment) — fall back.
    return DEFAULT_DEV_URL;
  }
}

/**
 * Strip query + hash from a URL, returning just the path. Used to bucket
 * annotations per-page (per the spec, multi-page apps store path only).
 */
export function urlPath(url: string): string {
  try {
    const u = new URL(url);
    return u.pathname || "/";
  } catch {
    const q = url.indexOf("?");
    const h = url.indexOf("#");
    const candidates = [q, h].filter((i) => i >= 0);
    if (candidates.length === 0) return url;
    const cut = Math.min(...candidates);
    return url.slice(0, cut);
  }
}
