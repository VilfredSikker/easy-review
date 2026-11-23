import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Command } from "$lib/protocol/generated/Command";
import type { Risk } from "$lib/protocol/generated/Risk";
import { ResourceStore } from "./resourceStore";
import { RiskAdapter } from "./riskAdapter";

const TARGET = { repoRoot: "/work/widgets", remoteRepo: null };

function risk(paths: string[]): Risk {
  return {
    repo: "acme/widgets",
    default: "ai",
    rules: [
      { pattern: "db/**", tier: "high", readable: true, label: null, source: "user", fileCount: paths.filter((p) => p.startsWith("db/")).length },
    ],
    files: paths.map((path) => ({ path, tier: path.startsWith("db/") ? "high" : "ai", rule: null })),
  };
}

/** A backend that answers both builds' risk calls and records each one. */
function fakeBackend() {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  let version = 0;
  let paths: string[] = [];
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    switch (command) {
      case "er_send": {
        const sent = args.command as Extract<Command, { kind: "load_risk" }>;
        paths = sent.paths;
        version += 1;
        return { changed: [{ key: { kind: "risk", key: "acme/widgets" }, version }], created: "acme/widgets" } as T;
      }
      case "er_fetch":
        return (args.keys as unknown[]).map((key) => ({ key, version, value: { kind: "risk", ...risk(paths) } })) as T;
      case "er_versions":
        return (args.keys as unknown[]).map(() => version) as T;
      case "get_risk_rules": {
        const { files: _files, ...rules } = risk([]);
        return { ...rules, version: 1 } as T;
      }
      case "get_risk_files": {
        const { rules, files } = risk(args.paths as string[]);
        return { rules, files } as T;
      }
      default:
        throw new Error(`unexpected ${command}`);
    }
  };
  return { run, names: () => calls.map((c) => c.command), calls };
}

function adapter(flagOn: boolean) {
  const backend = fakeBackend();
  const store = new ResourceStore(resourceClient(backend.run, async () => () => {}), 60_000);
  return { backend, risk: new RiskAdapter({ flagOn, store, invoke: backend.run }) };
}

describe("RiskAdapter", () => {
  it("loads through er_send and reads risk(key) through er_fetch with the flag on", async () => {
    const { backend, risk: adapterRisk } = adapter(true);
    const view = await adapterRisk.load(TARGET, ["db/a.sql", "src/b.rs"]);
    expect(view.files.map((f) => f.path)).toEqual(["db/a.sql", "src/b.rs"]);
    expect(view.rules[0].fileCount).toBe(1);
    // A second load with other paths answers those paths, not the held ones.
    const again = await adapterRisk.load(TARGET, ["db/a.sql", "db/b.sql"]);
    expect(again.rules[0].fileCount).toBe(2);
    expect(backend.names().filter((n) => !n.startsWith("er_"))).toEqual([]);
    expect(backend.calls.find((c) => c.command === "er_send")?.args.command).toEqual({
      kind: "load_risk",
      repoRoot: "/work/widgets",
      remoteRepo: null,
      paths: ["db/a.sql", "src/b.rs"],
    });
  });
});
