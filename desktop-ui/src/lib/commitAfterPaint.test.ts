import { afterEach, describe, expect, it } from "bun:test";
import {
  scheduleAfterPaint,
  shouldCommitWrapColsImmediately,
} from "./commitAfterPaint";

describe("shouldCommitWrapColsImmediately", () => {
  it("commits the first measured width immediately", () => {
    expect(shouldCommitWrapColsImmediately(80, null)).toBe(true);
  });

  it("defers later width changes", () => {
    expect(shouldCommitWrapColsImmediately(90, 80)).toBe(false);
  });

  it("does not treat a still-unmeasured panel as ready", () => {
    expect(shouldCommitWrapColsImmediately(null, null)).toBe(false);
  });

  it("defers wrapping turning off after a measured width", () => {
    expect(shouldCommitWrapColsImmediately(null, 80)).toBe(false);
  });
});

describe("scheduleAfterPaint", () => {
  const nativeRaf = globalThis.requestAnimationFrame;
  const nativeCancel = globalThis.cancelAnimationFrame;

  afterEach(() => {
    globalThis.requestAnimationFrame = nativeRaf;
    globalThis.cancelAnimationFrame = nativeCancel;
  });

  it("runs the callback after two animation frames", () => {
    const pending = new Map<number, FrameRequestCallback>();
    let seq = 0;
    globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      const id = ++seq;
      pending.set(id, cb);
      return id;
    }) as typeof requestAnimationFrame;
    globalThis.cancelAnimationFrame = ((id: number) => {
      pending.delete(id);
    }) as typeof cancelAnimationFrame;

    let ran = false;
    scheduleAfterPaint(() => {
      ran = true;
    });
    expect(ran).toBe(false);
    pending.get(1)?.(0);
    pending.delete(1);
    expect(ran).toBe(false);
    pending.get(2)?.(0);
    expect(ran).toBe(true);
  });

  it("cancel skips the callback", () => {
    const pending = new Map<number, FrameRequestCallback>();
    let seq = 0;
    globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      const id = ++seq;
      pending.set(id, cb);
      return id;
    }) as typeof requestAnimationFrame;
    globalThis.cancelAnimationFrame = ((id: number) => {
      pending.delete(id);
    }) as typeof cancelAnimationFrame;

    let ran = false;
    const cancel = scheduleAfterPaint(() => {
      ran = true;
    });
    cancel();
    expect(pending.size).toBe(0);
    expect(ran).toBe(false);
  });
});
