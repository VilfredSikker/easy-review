import { SvelteMap } from "svelte/reactivity";
import type { Invoke } from "$lib/protocol/client";
import type { AppUpdate } from "$lib/protocol/generated/AppUpdate";
import type { Command } from "$lib/protocol/generated/Command";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Session } from "$lib/protocol/generated/Session";
import type { Settings } from "$lib/protocol/generated/Settings";
import type {
  AiProviderInfo,
  AppSnapshot,
  DisplayConfigSnapshot,
  FeatureFlagsSnapshot,
  ReviewerInfo,
  WatchStatusSnapshot,
} from "$lib/pageTypes";
import type { ResourceStore } from "./resourceStore";
import { errorText, SETTINGS_KEY } from "./settingsAdapter";

export const SESSION_KEY: ResourceKey = { kind: "session" };

/** The snapshot fields the adapter reads with the flag off. */
export type SessionSlice = Pick<
  AppSnapshot,
  | "theme"
  | "features"
  | "display"
  | "arena_enabled"
  | "config_load_error"
  | "active_ai_label"
  | "active_ai_effort"
  | "app_version"
  | "watch_active"
  | "watch_status"
>;

export type Panel = "left" | "tree" | "right";

const DEFAULT_THEME = "graphite";
const DEFAULT_FEATURES: FeatureFlagsSnapshot = {
  viewBranch: true,
  viewUnstaged: true,
  viewStaged: true,
  viewHistory: true,
  viewConflicts: true,
  viewHidden: true,
  viewTour: true,
};
const DEFAULT_DISPLAY: DisplayConfigSnapshot = {
  lineNumbers: true,
  wrapLines: false,
  splitDiff: false,
  tabWidth: 4,
};
const NOT_WATCHING: WatchStatusSnapshot = { active: false, branch: null, root_path: null };

interface SessionAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show" | "send" | "whenAt">;
  invoke: Invoke;
  snapshot: () => SessionSlice | null;
  /** `app.cmd`: the palette's flag-off pick, whose answered snapshot the page takes in. */
  cmd: (command: string, args: Record<string, unknown>) => Promise<unknown>;
}

/**
 * How a flag-off pick reaches `set_ai_selection`. `invoke` calls it as
 * Settings always has and rejects on failure; `app` goes through `app.cmd`
 * as the palette always has, which takes in the snapshot the command answers
 * and toasts a failure itself.
 */
export type OldPickRoute = "invoke" | "app";

/**
 * The app chrome and the AI picker behind one API in both builds: the theme
 * and view switches, the AI pick's labels, the watcher, the version and the
 * update check. With the flag off it reads `app.snapshot` and calls the old
 * commands; with it on it reads `settings` and `session` and writes through
 * `er_send`, and reads nothing from the snapshot. Until a resource's first
 * fetch answers, its fields read as the defaults.
 */
export class SessionAdapter {
  private readonly local = new SvelteMap<"update", AppUpdate>();

  constructor(private readonly deps: SessionAdapterDeps) {
    if (deps.flagOn) {
      deps.store.show(SETTINGS_KEY);
      deps.store.show(SESSION_KEY);
    }
  }

  get theme(): string {
    if (!this.deps.flagOn) return this.deps.snapshot()?.theme ?? DEFAULT_THEME;
    return this.settings()?.theme ?? DEFAULT_THEME;
  }

  get features(): FeatureFlagsSnapshot {
    if (!this.deps.flagOn) return this.deps.snapshot()?.features ?? DEFAULT_FEATURES;
    return this.settings()?.features ?? DEFAULT_FEATURES;
  }

  get display(): DisplayConfigSnapshot {
    if (!this.deps.flagOn) return this.deps.snapshot()?.display ?? DEFAULT_DISPLAY;
    return this.settings()?.display ?? DEFAULT_DISPLAY;
  }

  get arenaEnabled(): boolean {
    if (!this.deps.flagOn) return this.deps.snapshot()?.arena_enabled !== false;
    return this.settings()?.arenaEnabled ?? true;
  }

  get configLoadError(): string | null {
    if (!this.deps.flagOn) return this.deps.snapshot()?.config_load_error ?? null;
    return this.session()?.config_load_error ?? null;
  }

  get activeAiLabel(): string {
    if (!this.deps.flagOn) return this.deps.snapshot()?.active_ai_label ?? "";
    return this.session()?.active_ai_label ?? "";
  }

  get activeAiEffort(): string | null {
    if (!this.deps.flagOn) return this.deps.snapshot()?.active_ai_effort ?? null;
    return this.session()?.active_ai_effort ?? null;
  }

  get appVersion(): string {
    const version = this.deps.flagOn ? this.session()?.app_version : this.deps.snapshot()?.app_version;
    return version?.trim() || "0.0.0";
  }

  get watchStatus(): WatchStatusSnapshot {
    if (!this.deps.flagOn) return this.deps.snapshot()?.watch_status ?? NOT_WATCHING;
    return this.session()?.watch_status ?? NOT_WATCHING;
  }

  get watchActive(): boolean {
    if (!this.deps.flagOn) return this.deps.snapshot()?.watch_active ?? false;
    return this.watchStatus.active;
  }

  /** The last update check's answer; null until one finished. */
  get update(): AppUpdate | null {
    if (!this.deps.flagOn) return this.local.get("update") ?? null;
    return this.session()?.update ?? null;
  }

  /** Asks whether a newer release exists; the answer shows in `update`. */
  async checkUpdate(): Promise<void> {
    if (!this.deps.flagOn) {
      this.local.set("update", await this.old<AppUpdate>("check_app_update", {}));
      return;
    }
    await this.send({ kind: "check_app_update" });
  }

  /** The providers with the current pick marked. */
  async providers(): Promise<AiProviderInfo[]> {
    if (!this.deps.flagOn) return this.old<AiProviderInfo[]>("list_ai_providers", {});
    await this.deps.store.whenAt(SETTINGS_KEY, 0);
    return (this.settings()?.providers ?? []) as AiProviderInfo[];
  }

  /** The specialized reviewers the picker lists. */
  async reviewers(): Promise<ReviewerInfo[]> {
    if (!this.deps.flagOn) return this.old<ReviewerInfo[]>("list_ai_reviewers", {});
    await this.deps.store.whenAt(SESSION_KEY, 0);
    return this.session()?.reviewers ?? [];
  }

  /** Saves the default provider and model; every pick saves. `route` matters only with the flag off. */
  async setAiSelection(
    providerId: string,
    modelId: string | null,
    effort?: string,
    route: OldPickRoute = "invoke",
  ): Promise<void> {
    if (!this.deps.flagOn) {
      const args = { providerId, modelId, ...(effort === undefined ? {} : { effort }), persist: true };
      if (route === "app") await this.deps.cmd("set_ai_selection", args);
      else await this.old("set_ai_selection", args);
      return;
    }
    await this.send({ kind: "set_ai_selection", providerId, modelId, effort: effort ?? null });
  }

  /** Saves the default effort; null is Auto. With the flag off it rejects on failure, as Settings expects. */
  async setAiEffort(effort: string | null): Promise<void> {
    if (!this.deps.flagOn) {
      await this.old("set_ai_effort", { effort, persist: true });
      return;
    }
    await this.send({ kind: "set_ai_effort", effort });
  }

  private settings(): Settings | null {
    const held = this.deps.store.get(SETTINGS_KEY)?.value;
    return held?.kind === "settings" ? held : null;
  }

  private session(): Session | null {
    const held = this.deps.store.get(SESSION_KEY)?.value;
    return held?.kind === "session" ? held : null;
  }

  private async old<T>(command: string, args: Record<string, unknown>): Promise<T> {
    try {
      return await this.deps.invoke<T>(command, args);
    } catch (e) {
      throw errorText(e);
    }
  }

  /** Sends `command` and waits until every key its `Ack` names is held at that version. */
  private async send(command: Command): Promise<void> {
    let ack;
    try {
      ack = await this.deps.store.send(command);
    } catch (e) {
      throw errorText(e);
    }
    await Promise.all(ack.changed.map((c) => this.deps.store.whenAt(c.key, c.version)));
  }
}
