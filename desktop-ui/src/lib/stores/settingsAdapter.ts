import { SvelteMap } from "svelte/reactivity";
import type { Invoke } from "$lib/protocol/client";
import type { Command } from "$lib/protocol/generated/Command";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { ModelUpsert } from "$lib/protocol/generated/ModelUpsert";
import type { ProviderUpsert } from "$lib/protocol/generated/ProviderUpsert";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { UninstallOptions } from "$lib/protocol/generated/UninstallOptions";
import type { UninstallPreview } from "$lib/protocol/generated/UninstallPreview";
import type {
  ConfigFieldValue,
  GetConfigHubResponse,
  RefreshAiModelsResponse,
  ReviewModelGroups,
} from "$lib/types";
import type { ResourceStore } from "./resourceStore";

export const SETTINGS_KEY: ResourceKey = { kind: "settings" };

/** What the Settings page reads: the rows, the providers and the AI defaults. */
export type SettingsHub = Omit<GetConfigHubResponse, "warnings">;

export type NoticeLevel = "info" | "error" | "success";

interface SettingsAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show" | "send" | "whenAt">;
  invoke: Invoke;
  /** Shows the person a message. With the flag on the backend's notices do this. */
  notify: (level: NoticeLevel, message: string) => void;
}

/** A failed call's text: the old commands reject with a string, the new ones with an `ErError`. */
function message(e: unknown): string {
  if (typeof e === "object" && e !== null && typeof (e as ErError).message === "string") {
    return (e as ErError).message;
  }
  return String(e);
}

/**
 * The Settings page's data and its writes, behind one API in both builds.
 * With the flag off it calls the old commands and keeps their answer; with
 * it on it reads the `settings` resource and writes through `er_send`, and
 * soft warnings and model-listing errors arrive as notices. A write resolves
 * once its result can be read; a failed one rejects with its text.
 */
export class SettingsAdapter {
  private readonly local = new SvelteMap<"hub", SettingsHub>();
  private shown = false;

  constructor(private readonly deps: SettingsAdapterDeps) {}

  /** The settings; null until the first answer. */
  read(): SettingsHub | null {
    if (!this.deps.flagOn) return this.local.get("hub") ?? null;
    const held = this.deps.store.get(SETTINGS_KEY)?.value;
    return held?.kind === "settings" ? held : null;
  }

  /** Settings opened: the backend adopts a config file repaired since a failed load. */
  async open(): Promise<void> {
    if (!this.deps.flagOn) {
      this.keep(await this.old<GetConfigHubResponse>("get_config_hub", {}));
      return;
    }
    await this.send({ kind: "reload_settings" });
  }

  /** Writes one config field. */
  async patch(key: string, value: ConfigFieldValue): Promise<void> {
    if (!this.deps.flagOn) {
      this.keep(await this.old<GetConfigHubResponse>("apply_config_patch", { patch: { key, value } }));
      return;
    }
    await this.send({ kind: "apply_config_patch", key, value });
  }

  async setReviewModels(groups: ReviewModelGroups): Promise<void> {
    if (!this.deps.flagOn) {
      this.keep(await this.old<GetConfigHubResponse>("set_review_flow_models", { groups }));
      return;
    }
    await this.send({ kind: "set_review_flow_models", groups });
  }

  /** Answers the editor's soft warnings, which the form shows beside itself. */
  async upsertProvider(provider: ProviderUpsert): Promise<string[]> {
    if (!this.deps.flagOn) return this.keep(await this.old("upsert_ai_provider", { provider }));
    await this.send({ kind: "upsert_ai_provider", provider });
    return [];
  }

  async deleteProvider(providerId: string): Promise<void> {
    if (!this.deps.flagOn) {
      this.keep(await this.old<GetConfigHubResponse>("delete_ai_provider", { providerId }));
      return;
    }
    await this.send({ kind: "delete_ai_provider", providerId });
  }

  /** Answers the editor's soft warnings, which the form shows beside itself. */
  async upsertModel(providerId: string, model: ModelUpsert): Promise<string[]> {
    if (!this.deps.flagOn) return this.keep(await this.old("upsert_ai_model", { providerId, model }));
    await this.send({ kind: "upsert_ai_model", providerId, model });
    return [];
  }

  async deleteModel(providerId: string, modelId: string): Promise<void> {
    if (!this.deps.flagOn) {
      this.keep(await this.old<GetConfigHubResponse>("delete_ai_model", { providerId, modelId }));
      return;
    }
    await this.send({ kind: "delete_ai_model", providerId, modelId });
  }

  /** Lists one provider's models from its CLI; the outcome is shown to the person. */
  async refreshModels(providerId: string, label: string): Promise<void> {
    if (this.deps.flagOn) {
      await this.send({ kind: "refresh_ai_models", providerId, force: true });
      return;
    }
    const res = await this.old<RefreshAiModelsResponse>("refresh_ai_models", { providerId, force: true });
    const hub = this.local.get("hub");
    if (hub) this.local.set("hub", { ...hub, providers: res.providers });
    for (const error of res.errors) this.deps.notify("error", error);
    if (res.errors.length === 0) {
      const count = res.providers.find((p) => p.id === providerId)?.models.length ?? 0;
      this.deps.notify("success", `${label}: ${count} models`);
    }
  }

  async previewUninstall(options: UninstallOptions): Promise<UninstallPreview | null> {
    if (!this.deps.flagOn) return this.old<UninstallPreview>("preview_uninstall", { request: options });
    await this.send({ kind: "preview_uninstall", options });
    const held = this.deps.store.get(SETTINGS_KEY)?.value;
    return held?.kind === "settings" ? held.uninstallPreview : null;
  }

  /** Removes what `options` names; the app quits once it is done. */
  async runUninstall(options: UninstallOptions): Promise<void> {
    if (!this.deps.flagOn) {
      await this.old("run_uninstall", { request: options });
      return;
    }
    await this.send({ kind: "run_uninstall", options });
  }

  private async old<T>(command: string, args: Record<string, unknown>): Promise<T> {
    try {
      return await this.deps.invoke<T>(command, args);
    } catch (e) {
      throw message(e);
    }
  }

  /** Keeps an old command's answer and shows its soft warnings, as the page did. */
  private keep(res: GetConfigHubResponse): string[] {
    const { warnings = [], ...hub } = res;
    this.local.set("hub", hub);
    for (const warning of warnings) this.deps.notify("info", warning);
    return warnings;
  }

  /**
   * Sends `command` and waits until `settings` is held at the version it
   * moved to. The key is shown from the first write on, so what the write
   * changed can be read once this resolves.
   */
  private async send(command: Command): Promise<void> {
    if (!this.shown) {
      this.shown = true;
      this.deps.store.show(SETTINGS_KEY);
    }
    let ack;
    try {
      ack = await this.deps.store.send(command);
    } catch (e) {
      throw message(e);
    }
    const moved = ack.changed.find((c) => c.key.kind === "settings");
    if (moved) await this.deps.store.whenAt(SETTINGS_KEY, moved.version);
  }
}
