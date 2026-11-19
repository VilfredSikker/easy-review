import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Command } from "$lib/protocol/generated/Command";
import type { Settings } from "$lib/protocol/generated/Settings";
import type { GetConfigHubResponse } from "$lib/types";
import { ResourceStore } from "./resourceStore";
import { SettingsAdapter, type NoticeLevel } from "./settingsAdapter";

function wireSettings(theme: string): Settings {
  return {
    settings: {
      general: [{ kind: "cycle", key: "display.theme", label: "Theme", description: "", options: [], value: theme }],
      app: [],
      terminal: [],
    },
    providers: [],
    reviewModels: { fast: [], standard: [], advanced: [] },
    activeEffort: null,
    familyOptions: [],
    uninstallPreview: null,
  };
}

const themeOf = (hub: { settings: { general: { kind: string; value?: unknown }[] } } | null) =>
  hub?.settings.general.find((f) => f.kind === "cycle")?.value;

/** A backend that serves both builds' commands and records each call. */
function fakeBackend() {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  let version = 1;
  let theme = "dark";
  let refuse: string | null = null;
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    switch (command) {
      case "er_fetch":
        // Slower than a microtask, so a write that did not wait reads the old value.
        await new Promise((r) => setTimeout(r, 5));
        return (args.keys as unknown[]).map((key) => ({
          key,
          version,
          value: { kind: "settings", ...wireSettings(theme) },
        })) as T;
      case "er_versions":
        return (args.keys as unknown[]).map(() => version) as T;
      case "er_send": {
        // Tauri rejects with the command's error value, here an `ErError`.
        // eslint-disable-next-line no-throw-literal -- the IPC error is a plain object, not an Error
        if (refuse) throw { kind: "refused", message: refuse };
        const sent = args.command as Command;
        if (sent.kind === "apply_config_patch") {
          theme = String(sent.value);
          version += 1;
        }
        return { changed: [{ key: { kind: "settings" }, version }], created: null } as T;
      }
      case "get_config_hub":
        return { ...wireSettings(theme), warnings: [] } as T;
      case "apply_config_patch": {
        // The old commands reject with the bare text.
        if (refuse) throw refuse;
        const patch = args.patch as { value: string };
        theme = patch.value;
        return { ...wireSettings(theme), warnings: ["Include {prompt} in args"] } as GetConfigHubResponse as T;
      }
      default:
        throw new Error(`unexpected ${command}`);
    }
  };
  return {
    run,
    calls,
    names: () => calls.map((c) => c.command),
    refuseWith(text: string) {
      refuse = text;
    },
  };
}

function adapter(flagOn: boolean) {
  const backend = fakeBackend();
  const store = new ResourceStore(resourceClient(backend.run, async () => () => {}), 60_000);
  const notices: [NoticeLevel, string][] = [];
  const settingsAdapter = new SettingsAdapter({
    flagOn,
    store,
    invoke: backend.run,
    notify: (level, message) => notices.push([level, message]),
  });
  return { backend, settings: settingsAdapter, notices };
}

describe("SettingsAdapter", () => {
  it("reads and writes through the old commands with the flag off", async () => {
    const { backend, settings, notices } = adapter(false);
    expect(settings.read()).toBeNull();
    await settings.open();
    expect(themeOf(settings.read())).toBe("dark");
    await settings.patch("display.theme", "paper");
    expect(themeOf(settings.read())).toBe("paper");
    expect(settings.read()).not.toHaveProperty("warnings");
    expect(notices).toEqual([["info", "Include {prompt} in args"]]);
    expect(backend.names()).toEqual(["get_config_hub", "apply_config_patch"]);
  });

  it("reads the settings resource and writes through er_send with the flag on", async () => {
    const { backend, settings } = adapter(true);
    await settings.open();
    expect(themeOf(settings.read())).toBe("dark");
    await settings.patch("display.theme", "paper");
    // The write resolves once the version its Ack names is held.
    expect(themeOf(settings.read())).toBe("paper");
    expect(backend.names().filter((n) => !n.startsWith("er_"))).toEqual([]);
    expect(backend.calls.filter((c) => c.command === "er_send").map((c) => c.args.command)).toEqual([
      { kind: "reload_settings" },
      { kind: "apply_config_patch", key: "display.theme", value: "paper" },
    ]);
  });

  it("rejects a refused write with its text in both builds", async () => {
    for (const flagOn of [false, true]) {
      const { backend, settings } = adapter(flagOn);
      await settings.open();
      backend.refuseWith("An earlier settings save was refused");
      await expect(settings.patch("display.theme", "x")).rejects.toBe("An earlier settings save was refused");
    }
  });
});
