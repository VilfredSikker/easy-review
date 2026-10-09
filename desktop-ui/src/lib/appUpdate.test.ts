import { describe, it, expect } from "bun:test";
import { installAppUpdate, type UpdatePhase, type UpdaterDeps } from "./appUpdate";

function deps(overrides: Partial<UpdaterDeps> & { events?: unknown[] } = {}) {
  const calls = { relaunched: 0 };
  const events = overrides.events ?? [
    { event: "Started", data: { contentLength: 200 } },
    { event: "Progress", data: { chunkLength: 50 } },
    { event: "Progress", data: { chunkLength: 150 } },
    { event: "Finished" },
  ];
  const d: UpdaterDeps = {
    check:
      overrides.check ??
      (async () => ({
        downloadAndInstall: async (onEvent) => {
          for (const e of events) onEvent(e as Parameters<typeof onEvent>[0]);
        },
      })),
    relaunch:
      overrides.relaunch ??
      (async () => {
        calls.relaunched += 1;
      }),
  };
  return { d, calls };
}

describe("installAppUpdate", () => {
  it("reports progress, installs, then relaunches", async () => {
    const { d, calls } = deps();
    const phases: UpdatePhase[] = [];
    const handled = await installAppUpdate((p) => phases.push(p), d);

    expect(handled).toBe(true);
    expect(phases).toEqual([
      { kind: "downloading", percent: null },
      { kind: "downloading", percent: 25 },
      { kind: "downloading", percent: 100 },
      { kind: "installing" },
    ]);
    expect(calls.relaunched).toBe(1);
  });

  it("leaves percent unknown when the server sends no length", async () => {
    const { d } = deps({
      events: [{ event: "Started", data: {} }, { event: "Progress", data: { chunkLength: 10 } }],
    });
    const phases: UpdatePhase[] = [];
    await installAppUpdate((p) => phases.push(p), d);
    expect(phases).toContainEqual({ kind: "downloading", percent: null });
    expect(phases.some((p) => p.kind === "downloading" && p.percent !== null)).toBe(false);
  });

  it("returns false with nothing to install so the caller can open the release page", async () => {
    const { d, calls } = deps({ check: async () => null });
    const phases: UpdatePhase[] = [];
    expect(await installAppUpdate((p) => phases.push(p), d)).toBe(false);
    expect(phases).toEqual([]);
    expect(calls.relaunched).toBe(0);
  });

  it("surfaces a failed download and does not relaunch", async () => {
    const { d, calls } = deps({
      check: async () => ({
        downloadAndInstall: async () => {
          throw new Error("signature verification failed");
        },
      }),
    });
    const phases: UpdatePhase[] = [];
    expect(await installAppUpdate((p) => phases.push(p), d)).toBe(true);
    expect(phases.at(-1)).toEqual({ kind: "failed", message: "signature verification failed" });
    expect(calls.relaunched).toBe(0);
  });
});
