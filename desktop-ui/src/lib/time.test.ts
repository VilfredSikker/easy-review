import { describe, expect, it } from "bun:test";
import { timeAgo } from "./time";

/** ISO string `ms` milliseconds before now. */
const ago = (ms: number) => new Date(Date.now() - ms).toISOString();

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

describe("timeAgo", () => {
  it("returns 'just now' under a minute", () => {
    expect(timeAgo(ago(30_000))).toBe("just now");
  });

  it("formats minutes", () => {
    expect(timeAgo(ago(16 * MIN))).toBe("16m ago");
  });

  it("formats hours", () => {
    expect(timeAgo(ago(3 * HOUR))).toBe("3h ago");
  });

  it("formats days", () => {
    expect(timeAgo(ago(5 * DAY))).toBe("5d ago");
  });

  it("formats weeks", () => {
    expect(timeAgo(ago(14 * DAY))).toBe("2w ago");
  });

  it("formats months", () => {
    expect(timeAgo(ago(90 * DAY))).toBe("3mo ago");
  });

  it("formats years", () => {
    expect(timeAgo(ago(400 * DAY))).toBe("1y ago");
  });

  it("echoes a non-timestamp string unchanged", () => {
    expect(timeAgo("not-a-date")).toBe("not-a-date");
  });

  it("clamps a future timestamp to 'just now'", () => {
    // Clock skew or a future-dated commit must not leak a negative duration.
    expect(timeAgo(ago(-30_000))).toBe("just now");
    expect(timeAgo(ago(-3 * HOUR))).toBe("just now");
  });
});
