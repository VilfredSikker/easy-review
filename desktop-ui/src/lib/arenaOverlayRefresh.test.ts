import { describe, expect, it } from "bun:test";
import { refreshOverlayIfCurrent, type OverlayIdentity } from "./arenaOverlayRefresh";

describe("arena overlay refresh ownership", () => {
  it("does not let delayed run A replace run B", async () => {
    let resolveA!: (value: string) => void;
    let current: OverlayIdentity = { runId: "A", flowOrigin: { erDir: "/repo-a/.er", runId: "A" } };
    let displayed = "snapshot B";
    const refresh = refreshOverlayIfCurrent(
      current,
      () => current,
      () => new Promise<string>((resolve) => { resolveA = resolve; }),
      (value) => { displayed = value; },
    );
    current = { runId: "B", flowOrigin: { erDir: "/repo-b/.er", runId: "B" } };
    resolveA("snapshot A");
    expect(await refresh).toBe(false);
    expect(displayed).toBe("snapshot B");
  });

  it("rejects a same-run refresh after its captured flow bucket changes", async () => {
    const current: OverlayIdentity = { runId: "run", flowOrigin: { erDir: "/repo-b/.er", runId: "run" } };
    let committed = false;
    const applied = await refreshOverlayIfCurrent(
      { runId: "run", flowOrigin: { erDir: "/repo-a/.er", runId: "run" } },
      () => current,
      async () => "stale",
      () => { committed = true; },
    );
    expect(applied).toBe(false);
    expect(committed).toBe(false);
  });
});
