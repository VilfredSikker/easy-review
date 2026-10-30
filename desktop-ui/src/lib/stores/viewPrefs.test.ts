import { afterEach, describe, expect, it } from "bun:test";
import { loadFlag, saveFlag } from "./viewPrefs";

const realStorage = globalThis.localStorage;

function useStorage(store: Map<string, string>, throws = false) {
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (k: string) => {
        if (throws) throw new Error("blocked");
        return store.get(k) ?? null;
      },
      setItem: (k: string, v: string) => {
        if (throws) throw new Error("quota");
        store.set(k, v);
      },
    },
  });
}

afterEach(() => {
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: realStorage });
});

describe("view prefs", () => {
  it("round-trips a flag", () => {
    const store = new Map<string, string>();
    useStorage(store);
    saveFlag("k", true);
    expect(store.get("k")).toBe("1");
    expect(loadFlag("k", false)).toBe(true);
    saveFlag("k", false);
    expect(loadFlag("k", true)).toBe(false);
  });

  it("falls back when unset or unreadable", () => {
    useStorage(new Map([["junk", "yes"]]));
    expect(loadFlag("missing", true)).toBe(true);
    expect(loadFlag("junk", false)).toBe(false);
    useStorage(new Map(), true);
    expect(loadFlag("k", true)).toBe(true);
    expect(() => saveFlag("k", true)).not.toThrow();
  });
});
