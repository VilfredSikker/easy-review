import { app } from "./app.svelte";
import type { ProbeSnapshot } from "../types";

class ProbePass {
  active = $state(false);
  selected = $state<Set<string>>(new Set());

  enter() {
    this.active = true;
  }

  exit() {
    this.active = false;
    this.selected = new Set();
  }

  get probes(): ProbeSnapshot[] {
    return app.snapshot?.ai?.probes ?? [];
  }

  get filesClosed(): boolean {
    return app.snapshot?.ai?.probe_files_closed ?? true;
  }

  get openPaths(): string[] {
    return this.probes
      .filter((p) => p.stamp === "fail" || p.stamp === "empty")
      .map((p) => p.file);
  }

  get selectedIds(): string[] {
    return [...this.selected];
  }

  toggle(id: string) {
    const next = new Set(this.selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.selected = next;
  }

  selectAll() {
    this.selected = new Set(this.probes.filter((p) => !p.stamp).map((p) => p.id));
  }
}

export const probePass = new ProbePass();
