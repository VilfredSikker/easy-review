import { devLogEnabled, devLogPrefix, GROUP_ARENA } from "$lib/dev/log";

/** Arena flow diagnostics — prefixed with `[er-arena]` when enabled. */
export function arenaLog(message: string, detail?: unknown): void {
  if (!devLogEnabled(GROUP_ARENA)) return;
  const prefix = devLogPrefix(GROUP_ARENA);
  if (detail !== undefined) {
    console.log(`${prefix} ${message}`, detail);
  } else {
    console.log(`${prefix} ${message}`);
  }
}

export function arenaWarn(message: string, detail?: unknown): void {
  if (!devLogEnabled(GROUP_ARENA)) return;
  const prefix = devLogPrefix(GROUP_ARENA);
  if (detail !== undefined) {
    console.warn(`${prefix} ${message}`, detail);
  } else {
    console.warn(`${prefix} ${message}`);
  }
}

export function arenaError(message: string, detail?: unknown): void {
  if (!devLogEnabled(GROUP_ARENA)) return;
  const prefix = devLogPrefix(GROUP_ARENA);
  if (detail !== undefined) {
    console.error(`${prefix} ${message}`, detail);
  } else {
    console.error(`${prefix} ${message}`);
  }
}
