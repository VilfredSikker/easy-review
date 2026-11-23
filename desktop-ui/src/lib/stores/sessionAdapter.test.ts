import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Command } from "$lib/protocol/generated/Command";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Session } from "$lib/protocol/generated/Session";
import type { Settings } from "$lib/protocol/generated/Settings";
import { ResourceStore } from "./resourceStore";
import { SessionAdapter } from "./sessionAdapter";
import { mirrorPanelToggle } from "./panelMirror";

const FEATURES = {
  viewBranch: true,
  viewUnstaged: true,
  viewStaged: true,
  viewHistory: true,
  viewConflicts: true,
  viewHidden: true,
  viewTour: false,
};
const DISPLAY = { lineNumbers: true, wrapLines: false, splitDiff: true, tabWidth: 2 };
const PROVIDER = {
  id: "echo",
  label: "Echo",
  models: [],
  is_selected: true,
  family: null,
  has_models_command: false,
  command: null,
  args: null,
  models_command: null,
};

function wireSettings(theme: string): Settings {
  return {
    settings: { general: [], app: [], terminal: [] },
    providers: [PROVIDER],
    reviewModels: { fast: [], standard: [], advanced: [] },
    activeEffort: null,
    familyOptions: [],
    uninstallPreview: null,
    theme,
    features: FEATURES,
    display: DISPLAY,
    arenaEnabled: false,
  };
}

function wireSession(model: string, update: Session["update"]): Session {
  return {
    active_ai_label: `Echo / ${model}`,
    active_ai_provider_label: "Echo",
    active_ai_model_label: model,
    active_ai_effort: "high",
    config_load_error: "config.toml: bad",
    app_version: "1.0.0",
    watch_status: { active: true, branch: "feature", root_path: "/work" },
    update,
    reviewers: [{ kind: "security", label: "Security", description: "" }],
  };
}

/** The chrome the page reads for the state the backend below serves. */
const CHROME = {
  theme: "paper",
  features: FEATURES,
  display: DISPLAY,
  arenaEnabled: false,
  configLoadError: "config.toml: bad",
  activeAiEffort: "high",
  appVersion: "1.0.0",
  watchActive: true,
  watchStatus: { active: true, branch: "feature", root_path: "/work" },
};

const UPDATE = { current: "1.0.0", latest: "9.9.9", update_available: true, release_url: null };

/** A backend that serves the resource commands and records each call. */
function fakeBackend() {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  let version = 1;
  let model = "first";
  let update: Session["update"] = null;
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    switch (command) {
      case "er_fetch":
        // Slower than a microtask, so a write that did not wait reads the old value.
        await new Promise((r) => setTimeout(r, 5));
        return (args.keys as ResourceKey[]).map((key) => ({
          key,
          version,
          value:
            key.kind === "settings"
              ? { kind: "settings", ...wireSettings("paper") }
              : { kind: "session", ...wireSession(model, update) },
        })) as T;
      case "er_versions":
        return (args.keys as unknown[]).map(() => version) as T;
      case "er_send": {
        const sent = args.command as Command;
        if (sent.kind === "set_ai_selection") model = sent.modelId ?? "";
        if (sent.kind === "check_app_update") update = UPDATE;
        version += 1;
        const changed = [
          { key: { kind: "settings" }, version },
          { key: { kind: "session" }, version },
        ];
        return { changed, created: null } as T;
      }
      case "list_ai_providers":
        return [PROVIDER] as T;
      case "list_ai_reviewers":
        return [{ kind: "security", label: "Security", description: "" }] as T;
      case "check_app_update":
        return UPDATE as T;
      case "toggle_panel":
        return undefined as T;
      case "set_ai_selection":
      case "set_ai_effort":
        if (args.modelId === "refused" || args.effort === "refused") throw new Error("no such model");
        return {} as T;
      default:
        throw new Error(`unexpected ${command}`);
    }
  };
  return { run, calls, names: () => calls.map((c) => c.command) };
}

function adapter() {
  const backend = fakeBackend();
  const store = new ResourceStore(resourceClient(backend.run, async () => () => {}), 60_000);
  return { backend, store, session: new SessionAdapter({ store }) };
}

/** Every chrome field the page reads, as the adapter answers it. */
function chrome(session: SessionAdapter) {
  return {
    theme: session.theme,
    features: session.features,
    display: session.display,
    arenaEnabled: session.arenaEnabled,
    configLoadError: session.configLoadError,
    activeAiEffort: session.activeAiEffort,
    appVersion: session.appVersion,
    watchActive: session.watchActive,
    watchStatus: session.watchStatus,
  };
}

const until = async (done: () => boolean) => {
  for (let i = 0; i < 100 && !done(); i++) await new Promise((r) => setTimeout(r, 5));
};

describe("SessionAdapter", () => {
  it("reads settings and session and writes through er_send", async () => {
    const { backend, session } = adapter();
    expect(await session.providers()).toEqual([PROVIDER]);
    expect((await session.reviewers()).map((r) => r.kind)).toEqual(["security"]);
    expect(chrome(session)).toEqual(CHROME);

    await session.setAiSelection("echo", "second");
    // The pick resolves once the session its Ack names is held.
    expect(session.activeAiLabel).toBe("Echo / second");
    await session.setAiEffort("low");
    await session.checkUpdate();
    await until(() => session.update !== null);
    expect(session.update?.latest).toBe("9.9.9");

    expect(backend.names().filter((n) => !n.startsWith("er_"))).toEqual([]);
    expect(backend.calls.filter((c) => c.command === "er_send").map((c) => c.args.command)).toEqual([
      { kind: "set_ai_selection", providerId: "echo", modelId: "second", effort: null },
      { kind: "set_ai_effort", effort: "low" },
      { kind: "check_app_update" },
    ]);
  });
});

describe("mirrorPanelToggle", () => {
  it("mirrors a toggle through er_send", async () => {
    const backend = fakeBackend();
    const store = new ResourceStore(resourceClient(backend.run, async () => () => {}), 60_000);
    await mirrorPanelToggle("tree", { store });
    expect(backend.calls).toEqual([{ command: "er_send", args: { command: { kind: "toggle_panel", panel: "tree" } } }]);
  });
});
