import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Command } from "$lib/protocol/generated/Command";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Session } from "$lib/protocol/generated/Session";
import type { Settings } from "$lib/protocol/generated/Settings";
import { ResourceStore } from "./resourceStore";
import { SessionAdapter, type SessionSlice } from "./sessionAdapter";
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

/** What the flag-off snapshot carries for the same state. */
const SNAPSHOT: SessionSlice = {
  theme: "paper",
  features: FEATURES,
  display: DISPLAY,
  arena_enabled: false,
  config_load_error: "config.toml: bad",
  active_ai_label: "Echo / first",
  active_ai_effort: "high",
  app_version: "1.0.0",
  watch_active: true,
  watch_status: { active: true, branch: "feature", root_path: "/work" },
};

const UPDATE = { current: "1.0.0", latest: "9.9.9", update_available: true, release_url: null };

/** A backend that serves both builds' commands and records each call. */
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
        if (args.modelId === "refused" || args.effort === "refused") throw "no such model";
        return {} as T;
      default:
        throw new Error(`unexpected ${command}`);
    }
  };
  return { run, calls, names: () => calls.map((c) => c.command) };
}

function adapter(flagOn: boolean) {
  const backend = fakeBackend();
  const store = new ResourceStore(resourceClient(backend.run, async () => () => {}), 60_000);
  const cmds: [string, Record<string, unknown>][] = [];
  let snapshotReads = 0;
  const session = new SessionAdapter({
    flagOn,
    store,
    invoke: backend.run,
    snapshot: () => {
      snapshotReads += 1;
      return SNAPSHOT;
    },
    cmd: async (command, args) => {
      cmds.push([command, args]);
    },
  });
  return { backend, store, session, cmds, snapshotReads: () => snapshotReads };
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
  it("reads the snapshot and calls the old commands with the flag off", async () => {
    const { backend, session, cmds } = adapter(false);
    expect(chrome(session)).toMatchObject({ theme: "paper", features: FEATURES, display: DISPLAY, arenaEnabled: false });
    expect(session.activeAiLabel).toBe("Echo / first");
    expect(await session.providers()).toEqual([PROVIDER]);
    expect((await session.reviewers()).map((r) => r.kind)).toEqual(["security"]);
    await session.checkUpdate();
    expect(session.update?.latest).toBe("9.9.9");
    // Settings invokes the picks itself; the palette sends them through app.cmd.
    await session.setAiSelection("echo", "second", "low");
    await session.setAiEffort(null);
    await session.setAiSelection("echo", "third", undefined, "app");
    expect(backend.calls.filter((c) => c.command.startsWith("set_ai_"))).toEqual([
      { command: "set_ai_selection", args: { providerId: "echo", modelId: "second", effort: "low", persist: true } },
      { command: "set_ai_effort", args: { effort: null, persist: true } },
    ]);
    expect(cmds).toEqual([["set_ai_selection", { providerId: "echo", modelId: "third", persist: true }]]);
    expect(backend.names().filter((n) => n.startsWith("er_"))).toEqual([]);
  });

  it("rejects a refused Settings pick with the flag off, so Settings sees the failure", async () => {
    const { session, cmds } = adapter(false);
    await expect(session.setAiSelection("echo", "refused")).rejects.toBe("no such model");
    await expect(session.setAiEffort("refused")).rejects.toBe("no such model");
    expect(cmds).toEqual([]);
  });

  it("reads settings and session and writes through er_send with the flag on", async () => {
    const { backend, session, cmds, snapshotReads } = adapter(true);
    expect(await session.providers()).toEqual([PROVIDER]);
    expect((await session.reviewers()).map((r) => r.kind)).toEqual(["security"]);
    // The same answers the flag-off snapshot gives, from the resources alone.
    expect(chrome(session)).toEqual(chrome(adapter(false).session));

    await session.setAiSelection("echo", "second");
    // The pick resolves once the session its Ack names is held.
    expect(session.activeAiLabel).toBe("Echo / second");
    await session.setAiEffort("low");
    await session.checkUpdate();
    await until(() => session.update !== null);
    expect(session.update?.latest).toBe("9.9.9");

    expect(snapshotReads()).toBe(0);
    expect(cmds).toEqual([]);
    expect(backend.names().filter((n) => !n.startsWith("er_"))).toEqual([]);
    expect(backend.calls.filter((c) => c.command === "er_send").map((c) => c.args.command)).toEqual([
      { kind: "set_ai_selection", providerId: "echo", modelId: "second", effort: null },
      { kind: "set_ai_effort", effort: "low" },
      { kind: "check_app_update" },
    ]);
  });
});

describe("mirrorPanelToggle", () => {
  it("mirrors a toggle through the old command or er_send by build", async () => {
    for (const flagOn of [false, true]) {
      const backend = fakeBackend();
      const store = new ResourceStore(resourceClient(backend.run, async () => () => {}), 60_000);
      await mirrorPanelToggle("tree", { flagOn, store, invoke: backend.run });
      expect(backend.calls).toEqual([
        flagOn
          ? { command: "er_send", args: { command: { kind: "toggle_panel", panel: "tree" } } }
          : { command: "toggle_panel", args: { panel: "tree" } },
      ]);
    }
  });
});
