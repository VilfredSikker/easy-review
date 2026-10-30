import { describe, expect, it } from "bun:test";
import { longAge, shortAge } from "./relativeTime";

const NOW = Date.parse("2026-10-30T12:00:00Z");
const before = (ms: number) => NOW - ms;
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

describe("shortAge", () => {
  it("scales through units", () => {
    expect(shortAge(before(10_000), NOW)).toBe("now");
    expect(shortAge(before(21 * MIN), NOW)).toBe("21m");
    expect(shortAge(before(3 * HOUR), NOW)).toBe("3h");
    expect(shortAge(before(2 * DAY), NOW)).toBe("2d");
    expect(shortAge(before(14 * DAY), NOW)).toBe("2w");
    expect(shortAge(before(90 * DAY), NOW)).toBe("3mo");
    expect(shortAge(before(800 * DAY), NOW)).toBe("2y");
  });

  it("accepts ISO strings and clamps the future", () => {
    expect(shortAge(new Date(before(5 * MIN)).toISOString(), NOW)).toBe("5m");
    expect(shortAge(NOW + HOUR, NOW)).toBe("now");
  });

  it("is empty for unparseable input", () => {
    expect(shortAge("nope", NOW)).toBe("");
  });
});

describe("longAge", () => {
  it("renders prose ages", () => {
    expect(longAge(before(5_000), NOW)).toBe("just now");
    expect(longAge(before(21 * MIN), NOW)).toBe("21 min ago");
    expect(longAge(before(3 * HOUR), NOW)).toBe("3 h ago");
    expect(longAge(before(2 * DAY), NOW)).toBe("2 d ago");
    expect(longAge("nope", NOW)).toBe("");
  });
});
