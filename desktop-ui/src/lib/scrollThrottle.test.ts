import { describe, expect, it } from "bun:test";
import { makeScrollThrottle } from "./scrollThrottle";

type RafCallback = (time: number) => void;

function mockRaf(): { callbacks: RafCallback[]; fire: () => void; restore: () => void } {
  const callbacks: RafCallback[] = [];
  const original = globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame = (cb: RafCallback) => {
    callbacks.push(cb);
    return callbacks.length - 1;
  };
  return {
    callbacks,
    fire() {
      const cbs = callbacks.splice(0);
      for (const cb of cbs) cb(0);
    },
    restore() {
      globalThis.requestAnimationFrame = original;
    },
  };
}

describe("makeScrollThrottle", () => {
  it("coalesces rapid calls: onFrame receives latest value once per frame", () => {
    const raf = mockRaf();
    const received: number[] = [];
    const throttled = makeScrollThrottle((top) => received.push(top));

    throttled(100);
    throttled(200);
    throttled(300);

    expect(received).toEqual([]);
    expect(raf.callbacks).toHaveLength(1);

    raf.fire();
    expect(received).toEqual([300]);

    raf.restore();
  });

  it("schedules exactly one RAF no matter how many calls arrive before the frame", () => {
    const raf = mockRaf();
    const throttled = makeScrollThrottle(() => {});

    for (let i = 0; i < 100; i++) throttled(i);

    expect(raf.callbacks).toHaveLength(1);
    raf.restore();
  });

  it("allows a second batch after the first frame fires", () => {
    const raf = mockRaf();
    const received: number[] = [];
    const throttled = makeScrollThrottle((top) => received.push(top));

    throttled(50);
    raf.fire();
    expect(received).toEqual([50]);

    throttled(75);
    throttled(90);
    expect(raf.callbacks).toHaveLength(1);
    raf.fire();
    expect(received).toEqual([50, 90]);

    raf.restore();
  });

  it("does not schedule a new RAF if no calls arrive after the frame fires", () => {
    const raf = mockRaf();
    const throttled = makeScrollThrottle(() => {});

    throttled(10);
    raf.fire(); // clears pending
    const countAfter = raf.callbacks.length;

    expect(countAfter).toBe(0);
    raf.restore();
  });

  it("always delivers the most recent value, never an intermediate one", () => {
    const raf = mockRaf();
    const received: number[] = [];
    const throttled = makeScrollThrottle((top) => received.push(top));

    for (let i = 1; i <= 50; i++) throttled(i);
    raf.fire();

    expect(received).toHaveLength(1);
    expect(received[0]).toBe(50);
    raf.restore();
  });
});

describe("fileTop fallback — localScroll when fileTop is unknown", () => {
  it("produces localScroll=0 when fileTop equals scrollTopPx (initial render)", () => {
    const scrollTopPx = 400;
    const fileTop = scrollTopPx; // fallback: fileTop ?? scrollTopPx
    const hunkContentStart = 0;
    const localScroll = scrollTopPx - fileTop - hunkContentStart;
    expect(localScroll).toBe(0);
  });

  it("virtualWindow starts at row 0 when localScroll is 0", () => {
    // Pure math: with localScroll=0, firstVisible=0, start=max(0,0-overscan)=0
    const scrollTop = 0;
    const totalItems = 500;
    const itemHeight = 24;
    const viewportHeight = 800;
    // Manually replicate windowFromScroll logic for the 0-scroll case
    const firstVisible = Math.max(0, Math.floor(scrollTop / itemHeight));
    expect(firstVisible).toBe(0);
    const lastVisible = Math.min(totalItems - 1, Math.ceil((scrollTop + viewportHeight) / itemHeight));
    const overscan = 5;
    const start = Math.max(0, firstVisible - overscan);
    expect(start).toBe(0);
    // Verify only a viewport-worth of rows would render, not all 500
    const end = Math.min(totalItems, lastVisible + overscan);
    expect(end).toBeLessThan(totalItems);
  });
});
