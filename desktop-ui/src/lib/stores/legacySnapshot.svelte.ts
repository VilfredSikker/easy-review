import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppSnapshot, PollResponse, TabSummary } from "../pageTypes";
import { pageView } from "./pageView.svelte";

/** A fallback read in case a revision event is missed. */
const SAFETY_MS = 30_000;

/**
 * The App's snapshot, read only for what no resource serves yet: the agent
 * commands and their log, the background tasks, the loading flags, and the
 * tab rows' live labels, change tokens and local PRs' `owner/repo`. Nothing
 * else on the page reads it; each field leaves here when a resource carries
 * it.
 */
class LegacySnapshot {
  private raw = $state<AppSnapshot | null>(null);
  private timer: ReturnType<typeof setTimeout> | null = null;
  private unlisten: Promise<UnlistenFn> | null = null;
  private inFlight = false;
  private again = false;

  /** The App's tab rows, which the tab strip reads for what its resource lacks. */
  get tabRows(): readonly TabSummary[] {
    return this.raw?.tabs ?? [];
  }

  /** Lays the fields no resource serves over the page's view. */
  overlay(view: AppSnapshot): AppSnapshot {
    const raw = this.raw;
    if (!raw) return view;
    return {
      ...view,
      bg_loading: raw.bg_loading,
      agent_commands: raw.agent_commands,
      agent_log: raw.agent_log,
      background_tasks: raw.background_tasks,
    };
  }

  start(): void {
    if (this.timer !== null) return;
    void invoke<AppSnapshot>("get_snapshot").then(
      (snap) => {
        this.raw = snap;
      },
      () => {},
    );
    this.unlisten = listen<number>("er://revision", () => void this.read());
    const tick = async () => {
      await this.read();
      if (this.timer !== null) this.timer = setTimeout(tick, SAFETY_MS);
    };
    this.timer = setTimeout(tick, SAFETY_MS);
  }

  stop(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    void this.unlisten?.then((stop) => stop());
    this.unlisten = null;
  }

  /** One `poll`; a revision that lands meanwhile reads once more after it. */
  private async read(): Promise<void> {
    if (this.inFlight) {
      this.again = true;
      return;
    }
    this.inFlight = true;
    try {
      const next = await invoke<PollResponse>("poll");
      if (next.snapshot !== null) this.raw = next.snapshot;
    } catch {
      // The window may be closing.
    } finally {
      this.inFlight = false;
      if (this.again) {
        this.again = false;
        void this.read();
      }
    }
  }
}

export const legacySnapshot = new LegacySnapshot();
pageView.overlay((view) => legacySnapshot.overlay(view));
