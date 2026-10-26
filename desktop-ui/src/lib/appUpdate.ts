import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "checking" }
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
 * when the manifest names no newer version, so the caller can fall back to the
 * release page. A release without `latest.json` is not that case: the plugin
 * rejects the 404, and it surfaces as a `failed` phase.
 */
export async function installAppUpdate(
  onPhase: (phase: UpdatePhase) => void,
  deps: UpdaterDeps = tauriDeps,
): Promise<boolean> {
  try {
    // Reported before the network round trip, so the caller can lock the
    // button and a second click cannot start a parallel install.
    onPhase({ kind: "checking" });
    const update = await deps.check();
    if (!update) {
      onPhase({ kind: "idle" });
      return false;
    }
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
