/** Dev log groups — mirrors Rust `ER_LOG` / `--logs` (see `crates/er-engine/src/dev_log.rs`). */

export const GROUP_ARENA = "arena";
export const GROUP_PROFILE = "profile";
export const GROUP_ERP = "erp";
export const GROUP_APP = "app";

let filter: Set<string> | null = null;
let initialized = false;

function parseSpec(raw: string | undefined): Set<string> | null {
  if (!raw?.trim()) return null;
  const parts = raw
    .split(",")
    .map((p) => p.trim().toLowerCase())
    .filter(Boolean);
  if (parts.length === 0 || parts.some((p) => p === "all" || p === "*")) {
    return null;
  }
  return new Set(parts);
}

/** Call once on app mount (dev builds). */
export async function initDevLog(): Promise<void> {
  if (!import.meta.env.DEV || initialized) return;
  initialized = true;
  const fromEnv = parseSpec(import.meta.env.VITE_ER_LOG as string | undefined);
  if (fromEnv) {
    filter = fromEnv;
    return;
  }
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const groups = await invoke<string[] | null>("dev_log_filter");
    filter = groups?.length ? new Set(groups.map((g) => g.toLowerCase())) : null;
  } catch {
    filter = null;
  }
}

/** True after `initDevLog()` has run (dev builds). */
export function isDevLogInitialized(): boolean {
  return initialized;
}

export function devLogEnabled(group: string): boolean {
  if (!import.meta.env.DEV) return false;
  if (!filter) return true;
  return filter.has(group.toLowerCase());
}

export function devLogPrefix(group: string): string {
  return `[er-${group}]`;
}
