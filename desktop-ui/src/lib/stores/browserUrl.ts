// Pure helpers used by the browser-view store. Kept out of the `.svelte.ts`
// runtime so they are importable from Bun-driven unit tests.

import { invoke } from "@tauri-apps/api/core";

export const DEFAULT_DEV_URL = "http://localhost:5173";
export const BLANK_BROWSER_URL = "about:blank";

export function toProxyUrl(url: string): string {
  if (!url.trim()) return BLANK_BROWSER_URL;
  if (url === BLANK_BROWSER_URL) return url;
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
 * Canonical form used to decide whether two URLs point at the same page.
 * Strips the proxy scheme, normalizes the root path so `host` and `host/`
 * compare equal, and lowercases the protocol+host. Query and hash are
 * preserved so genuine in-page navigations are still seen as different.
 */
export function canonicalizeBrowserUrl(url: string): string {
  if (!url) return "";
  if (url === BLANK_BROWSER_URL) return BLANK_BROWSER_URL;
  const real = fromProxyUrl(url);
  try {
    const u = new URL(/^[a-z]+:\/\//i.test(real) ? real : `http://${real}`);
    const pathname = u.pathname === "" ? "/" : u.pathname;
    // Hash-only navigation should update the URL bar, but it should not force
    // the iframe to reload and re-trigger the content-script location report.
    return `${u.protocol.toLowerCase()}//${u.host.toLowerCase()}${pathname}${u.search}`;
  } catch {
    return real.toLowerCase();
  }
}

/** True when two URLs refer to the same page after canonicalization. */
export function sameBrowserUrl(a: string, b: string): boolean {
  return canonicalizeBrowserUrl(a) === canonicalizeBrowserUrl(b);
}

/**
 * Canonical page identity for persisted UI annotations. Uses origin + path,
 * deliberately ignoring query and hash so pins survive common SPA/router noise
 * while still not leaking across different localhost apps or domains.
 */
export function pageKey(url: string): string {
  const real = fromProxyUrl(url);
  try {
    const u = new URL(/^[a-z]+:\/\//i.test(real) ? real : `http://${real}`);
    const pathname = u.pathname || "/";
    return `${u.protocol.toLowerCase()}//${u.host.toLowerCase()}${pathname}`;
  } catch {
    return urlPath(real);
  }
}

/** Compatibility for annotations saved before page keys included origin. */
export function annotationMatchesPage(annotationUrl: string, currentUrl: string): boolean {
  return annotationUrl === pageKey(currentUrl) || annotationUrl === urlPath(currentUrl);
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
 * Strip query + hash from a URL, returning just the path. Kept for display and
 * legacy annotation compatibility; new annotation storage should use pageKey().
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
