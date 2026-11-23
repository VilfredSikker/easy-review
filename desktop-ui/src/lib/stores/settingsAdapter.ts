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
  ReviewModelGroups,
} from "$lib/pageTypes";
import type { ResourceStore } from "./resourceStore";

export const SETTINGS_KEY: ResourceKey = { kind: "settings" };

/** What the Settings page reads: the rows, the providers and the AI defaults. */
export type SettingsHub = Omit<GetConfigHubResponse, "warnings">;

interface SettingsAdapterDeps {
  store: Pick<ResourceStore, "get" | "show" | "send" | "whenAt">;
}

/** A failed call's text: the old commands reject with a string, the new ones with an `ErError`. */
export function errorText(e: unknown): string {
  if (typeof e === "object" && e !== null && typeof (e as ErError).message === "string") {
    return (e as ErError).message;
  }
  return String(e);
}

/**
 * The Settings page's data and its writes: it reads the `settings` resource
 * and writes through `er_send`, and soft warnings and model-listing errors
 * arrive as notices. A write resolves
 * once its result can be read; a failed one rejects with its text.
 */
export class SettingsAdapter {
  private shown = false;

  constructor(private readonly deps: SettingsAdapterDeps) {}

  /** The settings; null until the first answer. */
  read(): SettingsHub | null {
    const held = this.deps.store.get(SETTINGS_KEY)?.value;
    return held?.kind === "settings" ? held : null;
  }

  /** Settings opened: the backend adopts a config file repaired since a failed load. */
  async open(): Promise<void> {
    await this.send({ kind: "reload_settings" });
  }

  /** Writes one config field. */
  async patch(key: string, value: ConfigFieldValue): Promise<void> {
    await this.send({ kind: "apply_config_patch", key, value });
  }

  async setReviewModels(groups: ReviewModelGroups): Promise<void> {
    await this.send({ kind: "set_review_flow_models", groups });
  }

  /** Answers the editor's soft warnings, which the form shows beside itself. */
  async upsertProvider(provider: ProviderUpsert): Promise<string[]> {
    await this.send({ kind: "upsert_ai_provider", provider });
    return [];
  }

  async deleteProvider(providerId: string): Promise<void> {
    await this.send({ kind: "delete_ai_provider", providerId });
  }

  /** Answers the editor's soft warnings, which the form shows beside itself. */
  async upsertModel(providerId: string, model: ModelUpsert): Promise<string[]> {
    await this.send({ kind: "upsert_ai_model", providerId, model });
    return [];
  }

  async deleteModel(providerId: string, modelId: string): Promise<void> {
    await this.send({ kind: "delete_ai_model", providerId, modelId });
  }

  /** Lists one provider's models from its CLI; the outcome is shown to the person. */
  async refreshModels(providerId: string): Promise<void> {
    await this.send({ kind: "refresh_ai_models", providerId, force: true });
  }

  async previewUninstall(options: UninstallOptions): Promise<UninstallPreview | null> {
    await this.send({ kind: "preview_uninstall", options });
    const held = this.deps.store.get(SETTINGS_KEY)?.value;
    return held?.kind === "settings" ? held.uninstallPreview : null;
  }

  /** Removes what `options` names; the app quits once it is done. */
  async runUninstall(options: UninstallOptions): Promise<void> {
    await this.send({ kind: "run_uninstall", options });
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
      throw errorText(e);
    }
    const moved = ack.changed.find((c) => c.key.kind === "settings");
    if (moved) await this.deps.store.whenAt(SETTINGS_KEY, moved.version);
  }
}
