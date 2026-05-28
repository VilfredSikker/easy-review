/** Arena flow diagnostics — always prefixed with `[er-arena]`. */
export function arenaLog(message: string, detail?: unknown): void {
  if (detail !== undefined) {
    console.log(`[er-arena] ${message}`, detail);
  } else {
    console.log(`[er-arena] ${message}`);
  }
}

export function arenaWarn(message: string, detail?: unknown): void {
  if (detail !== undefined) {
    console.warn(`[er-arena] ${message}`, detail);
  } else {
    console.warn(`[er-arena] ${message}`);
  }
}

export function arenaError(message: string, detail?: unknown): void {
  if (detail !== undefined) {
    console.error(`[er-arena] ${message}`, detail);
  } else {
    console.error(`[er-arena] ${message}`);
  }
}
