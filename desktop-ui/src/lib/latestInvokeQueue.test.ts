import { describe, expect, it } from "vitest";
import { LATEST_INVOKE_SKIPPED, createLatestInvokeQueue } from "./latestInvokeQueue";

describe("createLatestInvokeQueue", () => {
  it("skips an earlier slot after a newer enqueue supersedes it", async () => {
    const q = createLatestInvokeQueue();
    let current = 1;
    const calls: number[] = [];

    const first = q.enqueue(
      () => current === 1,
      async () => {
        calls.push(1);
        return "a";
      },
    );
    current = 2;
    const second = q.enqueue(
      () => current === 2,
      async () => {
        calls.push(2);
        return "b";
      },
    );

    expect(await first).toBe(LATEST_INVOKE_SKIPPED);
    expect(await second).toBe("b");
    expect(calls).toEqual([2]);
  });

  it("runs slots in enqueue order when each is still current", async () => {
    const q = createLatestInvokeQueue();
    const order: string[] = [];
    let releaseFirst: () => void = () => {};
    const firstGate = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });

    const first = q.enqueue(
      () => true,
      async () => {
        order.push("first-start");
        await firstGate;
        order.push("first-end");
        return 1;
      },
    );
    const second = q.enqueue(
      () => true,
      async () => {
        order.push("second");
        return 2;
      },
    );

    await Promise.resolve();
    expect(order).toEqual(["first-start"]);
    releaseFirst();
    expect(await first).toBe(1);
    expect(await second).toBe(2);
    expect(order).toEqual(["first-start", "first-end", "second"]);
  });

  it("still runs the next slot after a rejected invoke", async () => {
    const q = createLatestInvokeQueue();
    const first = q.enqueue(
      () => true,
      async () => {
        throw new Error("boom");
      },
    );
    const second = q.enqueue(
      () => true,
      async () => "ok",
    );

    await expect(first).rejects.toThrow("boom");
    expect(await second).toBe("ok");
  });
});
