import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot } from "../types";

class AppStore {
  snapshot = $state<AppSnapshot | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);

  async load() {
    this.loading = true;
    try {
      this.snapshot = await invoke<AppSnapshot>("get_snapshot");
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
    }
  }

  async togglePanel(panel: "left" | "tree" | "right") {
    try {
      this.snapshot = await invoke<AppSnapshot>("toggle_panel", { panel });
    } catch (e) {
      console.error("togglePanel failed:", e);
    }
  }

  async cmd(command: string, args?: Record<string, unknown>) {
    try {
      this.snapshot = await invoke<AppSnapshot>(command, args);
    } catch (e) {
      console.error(`${command} failed:`, e);
    }
  }
}

export const app = new AppStore();
