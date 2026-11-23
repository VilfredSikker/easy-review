import type { AppUpdate } from "$lib/protocol/generated/AppUpdate";
import type { Command } from "$lib/protocol/generated/Command";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Session } from "$lib/protocol/generated/Session";
import type { Settings } from "$lib/protocol/generated/Settings";
import type {
  AiProviderInfo,
  DisplayConfigSnapshot,
  FeatureFlagsSnapshot,
  ReviewerInfo,
  WatchStatusSnapshot,
} from "$lib/pageTypes";
import type { ResourceStore } from "./resourceStore";
import { errorText, SETTINGS_KEY } from "./settingsAdapter";

export const SESSION_KEY: ResourceKey = { kind: "session" };

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
  store: Pick<ResourceStore, "get" | "show" | "send" | "whenAt">;
}

/**
 * The app chrome and the AI picker behind one API: the theme and view
 * switches, the AI pick's labels, the watcher, the version and the update
 * check, read from `settings` and `session` and written through `er_send`.
 * Until a resource's first fetch answers, its fields read as the defaults.
 */
export class SessionAdapter {
  constructor(private readonly deps: SessionAdapterDeps) {
    deps.store.show(SETTINGS_KEY);
    deps.store.show(SESSION_KEY);
  }

  get theme(): string {
    return this.settings()?.theme ?? DEFAULT_THEME;
  }

  get features(): FeatureFlagsSnapshot {
    return this.settings()?.features ?? DEFAULT_FEATURES;
  }

  get display(): DisplayConfigSnapshot {
    return this.settings()?.display ?? DEFAULT_DISPLAY;
  }

  get arenaEnabled(): boolean {
    return this.settings()?.arenaEnabled ?? true;
  }

  get configLoadError(): string | null {
    return this.session()?.config_load_error ?? null;
  }

  get activeAiLabel(): string {
    return this.session()?.active_ai_label ?? "";
  }

  get activeAiEffort(): string | null {
    return this.session()?.active_ai_effort ?? null;
  }

  get appVersion(): string {
    const version = this.session()?.app_version;
    return version?.trim() || "0.0.0";
  }

  get watchStatus(): WatchStatusSnapshot {
    return this.session()?.watch_status ?? NOT_WATCHING;
  }

  get watchActive(): boolean {
    return this.watchStatus.active;
  }

  /** The last update check's answer; null until one finished. */
  get update(): AppUpdate | null {
    return this.session()?.update ?? null;
  }

  /** Asks whether a newer release exists; the answer shows in `update`. */
  async checkUpdate(): Promise<void> {
    await this.send({ kind: "check_app_update" });
  }

  /** The providers with the current pick marked. */
  async providers(): Promise<AiProviderInfo[]> {
    await this.deps.store.whenAt(SETTINGS_KEY, 0);
    return (this.settings()?.providers ?? []) as AiProviderInfo[];
  }

  /** The specialized reviewers the picker lists. */
  async reviewers(): Promise<ReviewerInfo[]> {
    await this.deps.store.whenAt(SESSION_KEY, 0);
    return this.session()?.reviewers ?? [];
  }

  /** Saves the default provider and model; every pick saves. */
  async setAiSelection(providerId: string, modelId: string | null, effort?: string): Promise<void> {
    await this.send({ kind: "set_ai_selection", providerId, modelId, effort: effort ?? null });
  }

  /** Saves the default effort; null is Auto. */
  async setAiEffort(effort: string | null): Promise<void> {
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
