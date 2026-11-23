import { plugin } from "bun";
import { describe, expect, it, mock } from "bun:test";
import { compileModule } from "svelte/compiler";

// The export store runs on runes, which only the svelte compiler turns into
// code; bun loads every other module as plain TypeScript.
plugin({
  name: "export-options-runes",
  setup(build) {
    build.onLoad({ filter: /stores\/exportOptions\.svelte\.ts$/ }, async ({ path }) => {
      const ts = await Bun.file(path).text();
      const js = new Bun.Transpiler({ loader: "ts" }).transformSync(ts);
      return { contents: compileModule(js, { generate: "client", filename: path }).js.code, loader: "js" };
    });
  },
});

// export_review rejects with an ErError object. Bun keeps a module mock for
// the whole run, so every other command goes on to the real invoke.
const tauri = await import("@tauri-apps/api/core");
const realInvoke = tauri.invoke;
void mock.module("@tauri-apps/api/core", () => ({
  ...tauri,
  invoke: (cmd: string, args?: Record<string, unknown>) =>
    cmd === "export_review"
      ? // eslint-disable-next-line prefer-promise-reject-errors -- Tauri rejects with the serialized ErError, a plain object
        Promise.reject({ kind: "internal", message: "no review is open" })
      : realInvoke(cmd, args),
}));

const toasts: [string, string][] = [];
void mock.module("./pageView.svelte", () => ({ pageView: { current: null, overlay: () => {}, readTabs: () => {} } }));
void mock.module("./app.svelte", () => ({
  app: {
    routeCmds: () => {},
    showToast: (kind: string, message: string) => toasts.push([kind, message]),
  },
}));

const { exportOptions } = await import("./exportOptions.svelte");

describe("exportOptions.quickCopy", () => {
  it("toasts the ErError's message when the export fails", async () => {
    await exportOptions.quickCopy();
    expect(toasts).toEqual([["error", "Export failed: no review is open"]]);
  });
});
