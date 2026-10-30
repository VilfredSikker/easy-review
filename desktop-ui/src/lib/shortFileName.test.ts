import { describe, expect, it } from "bun:test";
import { baseName, shortFileName } from "./shortFileName";

describe("shortFileName", () => {
  it("leads with the descriptive part of a timestamped migration", () => {
    expect(shortFileName("20261029093502_add_threads.sql")).toEqual({
      main: "add_threads.sql",
      stamp: "093502",
      version: "20261029093502",
    });
  });

  it("leaves ordinary names alone", () => {
    expect(shortFileName("schema.sql")).toEqual({ main: "schema.sql", stamp: "", version: "" });
  });

  it("does not treat short or unseparated digit runs as timestamps", () => {
    expect(shortFileName("2026102909_x.sql").stamp).toBe("");
    expect(shortFileName("20261029093502.sql").stamp).toBe("");
  });
});

describe("baseName", () => {
  it("strips directories", () => {
    expect(baseName("a/b/c.ts")).toBe("c.ts");
    expect(baseName("c.ts")).toBe("c.ts");
  });
});
