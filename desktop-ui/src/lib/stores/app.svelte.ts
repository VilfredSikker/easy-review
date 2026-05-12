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
}

export const app = new AppStore();
