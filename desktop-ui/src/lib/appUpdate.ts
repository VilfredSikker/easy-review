import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "downloading"; percent: number | null }
  | { kind: "installing" }
  | { kind: "failed"; message: string };

/** The plugin's surface this flow touches, so tests can drive it without Tauri. */
export interface UpdaterDeps {
  check: () => Promise<{
    downloadAndInstall: (
      onEvent: (
        e:
          | { event: "Started"; data: { contentLength?: number } }
          | { event: "Progress"; data: { chunkLength: number } }
          | { event: "Finished" },
      ) => void,
    ) => Promise<void>;
  } | null>;
  relaunch: () => Promise<void>;
}

const tauriDeps: UpdaterDeps = { check, relaunch };

/**
 * Download, verify and install the latest release, then relaunch. Returns false
 * when the updater has nothing to install (e.g. the release carries no updater
 * manifest), so the caller can fall back to the release page.
 */
export async function installAppUpdate(
  onPhase: (phase: UpdatePhase) => void,
  deps: UpdaterDeps = tauriDeps,
): Promise<boolean> {
  try {
    const update = await deps.check();
    if (!update) return false;
    let total: number | null = null;
    let received = 0;
    onPhase({ kind: "downloading", percent: null });
    await update.downloadAndInstall((e) => {
      if (e.event === "Started") {
        total = e.data.contentLength ?? null;
      } else if (e.event === "Progress") {
        received += e.data.chunkLength;
        onPhase({
          kind: "downloading",
          percent: total ? Math.min(100, Math.round((received / total) * 100)) : null,
        });
      } else {
        onPhase({ kind: "installing" });
      }
    });
    await deps.relaunch();
    return true;
  } catch (err) {
    onPhase({ kind: "failed", message: err instanceof Error ? err.message : String(err) });
    return true;
  }
}
